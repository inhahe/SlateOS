#!/usr/bin/env python3
"""Report which loops in a kernel binary straddle a 4 KiB guest page.

Why this exists
---------------
Under TCG a translation block is bounded by the guest page, so a loop whose
backward branch crosses a 4 KiB boundary cannot stay one directly-chained TB:
every iteration pays a dispatcher round-trip instead of a direct jump. Measured
on this project at ~1.7x (`known-issues.md`
B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x).

The penalty is *uniform per iteration* and *perfectly reproducible on one
binary*, which is exactly why every noise-suppression mechanism in the bench
harness is blind to it: the canary and the mean/min dispersion check both look
for variation, and there is none. It re-rolls only when unrelated code shifts a
function's address -- so it masquerades as a commit-to-commit regression, in
benchmarks the commit did not touch.

That is the case this script was written to settle, and it is the step 3 the
known-issues entry left open: "the check is mechanical (disassemble, locate the
backward branch, compare addr >> 12 at both ends)".

Two modes
---------
Single binary -- "does this function straddle right now?":

    python scripts/straddle-check.py <kernel-elf> [symbol-substring ...]

Two binaries -- "did an unrelated commit re-roll the layout lottery?":

    python scripts/straddle-check.py --compare <old-elf> <new-elf> [substr ...]

The compare mode is the one that actually settles an attribution argument, and
it is worth explaining why it is sound. Between two builds that differ only in
some *other* function, the untouched functions are byte-identical: same
instructions, same loop offsets relative to the function start, same spans.
Only the absolute addresses move. So for every function whose loop *signature*
-- the sorted list of (offset-from-function-start, span) pairs -- is unchanged,
any difference in straddle status is purely an address shift, i.e. exactly the
artifact we are hunting. Functions whose signature *did* change were genuinely
recompiled; those are reported separately and never counted as layout flips,
because for them "the code changed" is a live alternative explanation and this
script cannot rule it out.

That separation is the whole point. A flip in a function the commit did not
touch is evidence of an artifact; a flip in a function it recompiled is not
evidence of anything.

Two mechanisms, not one
-----------------------
A TCG translation block is cut by a page boundary *wherever* it falls, not only
at a backward branch. So layout has a second, weaker channel: a function whose
straight-line body spans a boundary pays a dispatcher round-trip once per
**call**, where a straddling loop pays one per **iteration**. Compare mode
reports both, and reports the crossing counts unconditionally.

That is not symmetry for its own sake. The first real investigation this script
was used for -- `http_build_response_1KiB`, a benchmark that doubled -- has a
hot path containing no straddling loop at all: its dominant memcpy is `rep
movsb`, which has no backward branch, and `__rust_alloc` has none either. A
loop-only tool answers "no flips here", which reads as "layout is innocent"
when in fact the tool never looked at the only mechanism still standing. The
crossing count had to be computed by hand that day; a check that must be
re-derived by hand each time is a check that will eventually be skipped.

Comparability for crossings is decided by **byte length**, not loop signature:
a function that grew across a boundary changed its code, and calling that a
layout flip would be the same wrong-direction error the recompiled quarantine
exists to prevent.

Limits worth stating, because a silent limit is a wrong answer later
--------------------------------------------------------------------
* Static analysis cannot know which loop is *hot*. A flip in a cold loop costs
  nothing. Loop span is used as the ordering proxy only because a longer loop
  body is likelier to matter -- it is not a hotness measure. Correlate the
  output against which benchmarks actually moved; do not read a flip as a cause
  on its own.
* Inlining means the loop that dominates a benchmark often does not live in the
  function that shares the benchmark's name. `--all` plus the callee list from
  a disassembly is how you find it; searching only the obvious symbol is how
  you get a false "no straddle here" (this already happened once -- see the
  negative result recorded in known-issues.md under the still-open half of
  B-BENCH-A-PERSISTENT-REGRESSION-IS-REPORTED-ONCE-THEN-ABSORBED-INTO-ITS-OWN-RANGE).
* `jmp` to a lower address inside a function is counted as a loop. Tail calls
  compiled to a backward `jmp` into another function are not, since the target
  falls outside the enclosing symbol and is dropped.
* A loop whose span is >= 4096 bytes *cannot* fit in a page, so it straddles
  unconditionally and carries no information -- and there are many of them
  (every outer dispatch loop in the kernel). They are marked `YES*` and kept
  out of the headline count, because a report where 90% of the "hits" are
  arithmetically forced is a report nobody can read. They also never appear in
  compare mode: a forced straddle cannot flip.
"""
from __future__ import annotations

import os
import re
import subprocess
import sys
from typing import NamedTuple

PAGE = 4096

#: Backward-branch targets are what identify a loop. Conditional jumps and the
#: unconditional `jmp` both count; `call` does not (it leaves the function).
_BRANCH = re.compile(
    r"^\s*([0-9a-f]{8,16}):\s+(?:[0-9a-f]{2} )+\s*"
    r"(j[a-z]+)\s+0x([0-9a-f]+)"
)
_SYMLINE = re.compile(r"^([0-9a-f]{8,16})\s+<(.+)>:$")
#: Any disassembled instruction, branch or not. Needed for the *straight-line*
#: half of the story: a TCG translation block is cut by a page boundary
#: wherever it falls, not only at a backward branch, so a call-heavy function
#: with no loop at all still pays one dispatcher round-trip per boundary it
#: spans. Tracking the last instruction in each function is what makes that
#: measurable instead of assumed.
_INSN = re.compile(r"^\s*([0-9a-f]{8,16}):\s+[0-9a-f]{2} ")


class Loop(NamedTuple):
    """One backward branch.

    `offset` is measured from the function's start, not from the image base:
    that is what makes a loop identifiable across two builds whose only
    difference is where the function landed.
    """

    offset: int      #: loop head, relative to the enclosing function's start
    span: int        #: bytes from loop head to the backward branch
    target: int      #: absolute address of the loop head, this build
    straddles: bool  #: does [target, target+span] cross a 4 KiB boundary?

    @property
    def forced(self) -> bool:
        """True when the loop is too long to fit in a page either way.

        Such a loop straddles at every possible address, so its straddle bit
        says nothing about layout and it can never flip between builds.
        """
        return self.span >= PAGE


class Extent(NamedTuple):
    """Where a function's code begins and ends in this build.

    `end` is the address of its *last instruction*, not of the next symbol:
    the gap between two functions is alignment padding, which is never
    executed, and counting it would invent page crossings that cost nothing.
    """

    start: int
    end: int

    @property
    def crossings(self) -> int:
        """How many 4 KiB boundaries the function's body spans.

        This is the straight-line analogue of `Loop.straddles`. It is paid once
        per *call* rather than once per iteration, so it is a far weaker effect
        than a straddling loop -- but it is the only other way layout can
        change a benchmark, so leaving it unmeasured means "no straddle flip"
        gets read as "layout is innocent" when the tool never looked.
        """
        return self.end // PAGE - self.start // PAGE

    @property
    def length(self) -> int:
        return self.end - self.start


class Func(NamedTuple):
    """Everything one disassembled function contributes to a comparison."""

    extent: Extent
    loops: list


def _toolchain_bin(name: str) -> str:
    """Locate an llvm-* tool from the rustup toolchain, or fall back to PATH.

    These ship with rustup, so no binutils install is needed -- but the
    toolchain directory name varies by host, so try a couple and then trust
    PATH rather than hard-coding one machine's layout.
    """
    home = os.path.expanduser("~")
    roots = os.path.join(home, ".rustup", "toolchains")
    if os.path.isdir(roots):
        for toolchain in sorted(os.listdir(roots)):
            candidate = os.path.join(
                roots, toolchain, "lib", "rustlib",
                toolchain.split("-", 1)[-1], "bin", name)
            for path in (candidate, candidate + ".exe"):
                if os.path.isfile(path):
                    return path
    return name


def demangle(sym: str) -> str:
    """Strip the Rust symbol hash so the same function matches across builds.

    The hash changes every build, so matching on the full mangled name would
    make this script useless for the exact comparison it exists to do.
    """
    return re.sub(r"17h[0-9a-f]{16}E$", "", sym)


def disassemble(binary: str) -> dict[str, Func]:
    """Disassemble `binary` into per-function loops and code extents.

    One objdump pass serves both analyses. That is not just for speed: running
    it twice would let the loop view and the crossing view disagree about
    which functions exist, and a disagreement between two halves of the same
    answer is worse than either half alone.
    """
    objdump = _toolchain_bin("llvm-objdump")
    proc = subprocess.run(
        [objdump, "-d", binary],
        capture_output=True, text=True, errors="replace", check=False)
    if proc.returncode != 0:
        raise SystemExit(
            f"straddle-check: {objdump} failed ({proc.returncode}):\n"
            f"{proc.stderr[:2000]}")

    loops: dict[str, list[Loop]] = {}
    ends: dict[str, int] = {}
    starts: dict[str, int] = {}
    current = None
    start = 0
    for line in proc.stdout.splitlines():
        sym = _SYMLINE.match(line)
        if sym:
            start = int(sym.group(1), 16)
            current = demangle(sym.group(2))
            loops.setdefault(current, [])
            # A name can repeat (identical generic instantiations, or the same
            # symbol emitted in two sections).  Keep the *widest* span seen, so
            # the crossing count is never understated by a later, shorter copy
            # silently replacing an earlier one.
            starts[current] = min(starts.get(current, start), start)
            ends[current] = max(ends.get(current, start), start)
            continue
        if current is None:
            continue
        insn = _INSN.match(line)
        if insn:
            ends[current] = max(ends[current], int(insn.group(1), 16))
        match = _BRANCH.match(line)
        if not match:
            continue
        addr = int(match.group(1), 16)
        target = int(match.group(3), 16)
        # Backward only: a forward jump is control flow, not a loop.  A target
        # below the function start is a tail call out of the function, not a
        # loop in it.
        if not (start <= target < addr):
            continue
        loops[current].append(Loop(
            offset=target - start,
            span=addr - target,
            target=target,
            straddles=(target // PAGE) != (addr // PAGE),
        ))
    return {name: Func(Extent(starts[name], ends[name]), loops[name])
            for name in loops}


def loops_by_function(binary: str) -> dict[str, list[Loop]]:
    """Disassemble `binary` and collect every backward branch, per function."""
    return {name: func.loops for name, func in disassemble(binary).items()}


def signature(loops: list[Loop]) -> tuple[tuple[int, int], ...]:
    """Identity of a function's control flow, independent of where it landed.

    Two builds agreeing on this means the function was not recompiled, so any
    straddle difference is an address shift and nothing else.
    """
    return tuple(sorted((loop.offset, loop.span) for loop in loops))


def _matches(func: str, wanted: list[str]) -> bool:
    return not wanted or any(w in func.lower() for w in wanted)


def _shorten(func: str, width: int) -> str:
    return func if len(func) <= width else "…" + func[-(width - 1):]


def report_single(binary: str, wanted: list[str], all_loops: bool,
                  min_span: int) -> int:
    by_func = loops_by_function(binary)

    rows = []
    for func, loops in by_func.items():
        if not loops or not _matches(func, wanted):
            continue
        # Keep the *longest* loop per function by default: that is the one
        # whose span decides the straddle probability, and reporting every
        # backward branch (including two-instruction `rep`-style loops) buries
        # it.  `--all` is for when the interesting loop is not the longest --
        # which is the case whenever something got inlined.
        chosen = loops if all_loops else [max(loops, key=lambda x: x.span)]
        for loop in chosen:
            if loop.span < min_span:
                continue
            rows.append((func, loop))

    if not rows:
        where = f" matching {wanted}" if wanted else ""
        print(f"No function{where} contains a backward branch of >= {min_span} "
              f"bytes. That is a suspicious answer, not a clean one -- check "
              f"the symbol substring and that {binary} is an unstripped ELF.")
        return 1

    # Real straddles first, then the arithmetically-forced ones, then the
    # clean loops -- so the informative rows are never buried under the outer
    # dispatch loops that straddle no matter where they land.
    rows.sort(key=lambda r: (not r[1].straddles, r[1].forced, -r[1].span))
    width = min(max(len(r[0]) for r in rows), 78)
    print(f"{'function':<{width}}  {'loop span':>9}  {'pages':>5}  straddles")
    for func, loop in rows:
        end = loop.target + loop.span
        pages = f"{loop.target // PAGE:x}->{end // PAGE:x}"
        mark = ("YES*" if loop.forced else "YES") if loop.straddles else "no"
        print(f"{_shorten(func, width):<{width}}  {loop.span:>7} B  "
              f"{pages:>5}  {mark}   [{loop.target:016x}-{end:016x}]")

    real = [r for r in rows if not r[1].forced]
    hits = sum(1 for r in real if r[1].straddles)
    forced = len(rows) - len(real)
    print(f"\n{hits} of {len(real)} page-sized loops straddle a 4 KiB page "
          f"({forced} more are longer than a page, marked YES*, and straddle "
          f"unavoidably -- they tell you nothing about layout).")
    return 0


class Flips(NamedTuple):
    """The classification of one build pair. See `classify_flips`."""

    gained: list      #: (func, new_loop, old_loop) -- moved ONTO a boundary
    lost: list        #: (func, new_loop, old_loop) -- moved OFF a boundary
    recompiled: list  #: functions whose machine code changed; not comparable
    only_new: int
    only_old: int


def classify_flips(old: dict[str, list[Loop]], new: dict[str, list[Loop]],
                   wanted: list[str], min_span: int) -> Flips:
    """Split every function into layout-flipped, recompiled, or unchanged.

    Kept free of file I/O so it can be tested against synthetic inputs: an
    identity comparison of a real binary passes vacuously whether or not the
    classification works, so the only way to know this fires is to hand it a
    pair that *must* produce a flip.
    """
    gained, lost, recompiled = [], [], []
    only_new = 0
    for func, new_loops in new.items():
        if not _matches(func, wanted):
            continue
        old_loops = old.get(func)
        if old_loops is None:
            only_new += 1
            continue
        if signature(old_loops) != signature(new_loops):
            if any(l.span >= min_span for l in new_loops + old_loops):
                recompiled.append(func)
            continue
        # Signatures match, so sorting both by (offset, span) pairs each loop
        # with its own counterpart -- the addresses are the only thing left
        # that can differ.
        for old_loop, new_loop in zip(sorted(old_loops), sorted(new_loops)):
            if new_loop.span < min_span or new_loop.forced:
                continue
            if new_loop.straddles and not old_loop.straddles:
                gained.append((func, new_loop, old_loop))
            elif old_loop.straddles and not new_loop.straddles:
                lost.append((func, new_loop, old_loop))
    only_old = sum(1 for f in old if f not in new and _matches(f, wanted))
    return Flips(gained, lost, recompiled, only_new, only_old)


class CrossFlips(NamedTuple):
    """The straight-line counterpart of `Flips`. See `classify_crossings`."""

    more: list    #: (func, old_count, new_count) -- spans more boundaries now
    fewer: list   #: (func, old_count, new_count) -- spans fewer
    resized: int  #: byte length changed, so the counts are not comparable


def classify_crossings(old: dict[str, Func], new: dict[str, Func],
                       wanted: list[str]) -> CrossFlips:
    """Find functions whose body spans a different number of page boundaries.

    Comparability is decided by **byte length**, not by loop signature. Two
    functions with the same loops can still differ in length (a changed call,
    a different constant), and a length change means the crossing count could
    have moved because the code grew rather than because it moved -- which is
    the same "the code changed, so this is not a layout finding" quarantine
    that `classify_flips` applies, expressed in the unit that matters here.

    A function can gain a crossing purely by moving: a 3 KiB function is
    entirely within one page at some addresses and spans two at others.
    """
    more, fewer, resized = [], [], 0
    for func, new_func in new.items():
        if not _matches(func, wanted):
            continue
        old_func = old.get(func)
        if old_func is None:
            continue
        if old_func.extent.length != new_func.extent.length:
            resized += 1
            continue
        old_n = old_func.extent.crossings
        new_n = new_func.extent.crossings
        if new_n > old_n:
            more.append((func, old_n, new_n))
        elif new_n < old_n:
            fewer.append((func, old_n, new_n))
    return CrossFlips(more, fewer, resized)


def report_compare(old_bin: str, new_bin: str, wanted: list[str],
                   min_span: int) -> int:
    old_funcs = disassemble(old_bin)
    new_funcs = disassemble(new_bin)
    old = {n: f.loops for n, f in old_funcs.items()}
    new = {n: f.loops for n, f in new_funcs.items()}
    flips = classify_flips(old, new, wanted, min_span)
    crossed = classify_crossings(old_funcs, new_funcs, wanted)
    gained, lost = flips.gained, flips.lost
    recompiled, only_new, only_old = (
        flips.recompiled, flips.only_new, flips.only_old)

    def _dump(title: str, rows, note: str) -> None:
        print(f"\n{title}: {len(rows)}")
        if not rows:
            return
        print(f"  ({note})")
        rows.sort(key=lambda r: -r[1].span)
        width = min(max(len(r[0]) for r in rows), 72)
        for func, new_loop, old_loop in rows:
            print(f"  {_shorten(func, width):<{width}}  {new_loop.span:>7} B  "
                  f"{old_loop.target:012x} -> {new_loop.target:012x}")

    def _count(by_func):
        return sum(1 for loops in by_func.values() for l in loops
                   if l.straddles and not l.forced and l.span >= min_span)

    print(f"old: {os.path.basename(old_bin)}  ({_count(old)} avoidable "
          f"straddling loops >= {min_span} B)")
    print(f"new: {os.path.basename(new_bin)}  ({_count(new)} avoidable "
          f"straddling loops >= {min_span} B)")

    _dump("STRADDLE GAINED (expect these to be SLOWER)", gained,
          "same machine code, moved onto a page boundary")
    _dump("STRADDLE LOST (expect these to be FASTER)", lost,
          "same machine code, moved off a page boundary")

    print(f"\nrecompiled (code changed, straddle not comparable): "
          f"{len(recompiled)}")
    for func in sorted(recompiled)[:40]:
        print(f"  {func}")
    if len(recompiled) > 40:
        print(f"  ... and {len(recompiled) - 40} more")
    print(f"only in new: {only_new}    only in old: {only_old}")

    # The straight-line half. Reported unconditionally, and *before* the
    # verdict below, because the whole hazard here is concluding "layout is
    # innocent" from a loop analysis that never looked at the other mechanism.
    print(f"\nPAGE CROSSINGS (straight-line; paid once per CALL, not per "
          f"iteration): {len(crossed.more)} span more, {len(crossed.fewer)} "
          f"span fewer, {crossed.resized} not comparable (byte length changed)")
    if crossed.more or crossed.fewer:
        rows = ([(f, o, n, "+") for f, o, n in crossed.more]
                + [(f, o, n, "-") for f, o, n in crossed.fewer])
        rows.sort(key=lambda r: (-abs(r[2] - r[1]), r[0]))
        width = min(max(len(r[0]) for r in rows), 72)
        for func, old_n, new_n, mark in rows[:20]:
            print(f"  {mark} {_shorten(func, width):<{width}}  "
                  f"{old_n} -> {new_n} boundaries")
        if len(rows) > 20:
            print(f"  ... and {len(rows) - 20} more")

    if not gained and not lost:
        print("\nNo untouched function changed straddle status. If benchmarks "
              "moved between these two builds, page-straddle layout is NOT "
              "the explanation -- but check the crossing rows above against "
              "the benchmark's call path before ruling layout out entirely, "
              "then look for a real code cause.")
    else:
        print(f"\n{len(gained)} functions got slower and {len(lost)} got "
              f"faster purely from where they landed. Cross-reference these "
              f"against which benchmarks moved before calling it the cause.")
    return 0


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    compare = False
    all_loops = False
    min_span = 0
    rest = []
    for arg in argv:
        if arg == "--compare":
            compare = True
        elif arg == "--all":
            all_loops = True
        elif arg.startswith("--min-span="):
            min_span = int(arg.split("=", 1)[1])
        else:
            rest.append(arg)

    need = 2 if compare else 1
    if len(rest) < need:
        print(__doc__)
        return 2
    for path in rest[:need]:
        if not os.path.isfile(path):
            print(f"straddle-check: no such file: {path}", file=sys.stderr)
            return 2

    wanted = [w.lower() for w in rest[need:]]
    if compare:
        return report_compare(rest[0], rest[1], wanted, min_span)
    return report_single(rest[0], wanted, all_loops, min_span)


if __name__ == "__main__":
    sys.exit(main())
