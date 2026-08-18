#!/usr/bin/env python3
"""Rank kernel functions by how much stack their prologue claims.

    python scripts/stack-frames.py [--profile debug|release] [--top N]
    python scripts/stack-frames.py --elf path/to/kernel [--top N]
    python scripts/stack-frames.py --diff BEFORE_ELF AFTER_ELF

Why this exists
---------------
The per-task stack census in `sched` (printed every boot) answers *which task*
came closest to its canary.  It structurally cannot answer *which function*
put it there, because a watermark is a high-water mark on a stack, not an
attribution to code.  This closes that gap statically: it reads the compiled
kernel and reports, per function, the stack its prologue allocates.

Two traps it is built to avoid:

1. **Stack-probe chains.**  A function needing more than a page does not emit
   one `sub $N, %rsp`.  It emits `sub $0x1000, %rsp; movq $0, (%rsp)` repeated
   a page at a time, so a guard page can never be jumped over untouched.  A
   naive "first sub in the prologue" reading therefore reports exactly 4096 for
   every large function -- hiding precisely the ones worth finding.  This sums
   the whole chain.
2. **Profile matters enormously.**  At `opt-level = 0` no by-value move is
   elided, so a large struct returned by value costs a full copy in every frame
   it passes through.  `scripts/boot-test.sh` builds *debug* by default, so
   debug is the profile whose numbers explain a canary halt -- measuring
   release and concluding the kernel is fine is a real way to be wrong here.
   The same spawn path measured 40640 bytes in debug and 8960 in release.

This was written to diagnose A-INTERMITTENT-STACK-CANARY-HALT-AT-REAP-TIME,
where it found `FpuState` (4096 bytes, embedded by value in `Task`) costing
40640 bytes of a 64 KiB stack across three spawn-path frames.

`--peak` is for functions `scripts/split-frames.py` has split.  After a split the
per-symbol ranking is *misleading in the flattering direction*: it shows the
shrunken outer frame and lists the cases separately, so a function whose peak
did not move at all looks like a large win.  What is actually on the stack while
a case runs is `outer + that case`, so `--peak` groups each `::case` back under
its parent and reports the deepest sum.  It is the number that decides whether a
split was worth doing: `linux_fd::self_test` split into one case measured
28 224 -> 18 896 by the ranking and 28 224 -> 28 240 by this, and was reverted.

Caveats: this is a static lower bound.  It misses dynamic `alloca`, any
re-adjustment after the prologue, and of course says nothing about call depth
-- a 500-byte frame recursing 100 deep will not show up here.
"""

from __future__ import annotations

import argparse
import glob
import os
import re
import shutil
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

SYM_RE = re.compile(r"^[0-9a-f]+ <(.+)>:\s*$")
SUB_RE = re.compile(r"^[0-9a-f]+:\s+subq?\s+\$(0x[0-9a-f]+|[0-9]+),\s*%rsp")
# Frame setup is finished once real work starts.
CALL_RE = re.compile(r"^[0-9a-f]+:\s+(callq?|jmpq?)\s")


def find_objdump() -> str:
    for name in ("llvm-objdump", "objdump"):
        p = shutil.which(name)
        if p:
            return p
    # rustup ships llvm-objdump inside the toolchain's rustlib bin dir.
    home = os.path.expanduser("~/.rustup/toolchains")
    hits = glob.glob(
        os.path.join(home, "*", "lib", "rustlib", "*", "bin", "llvm-objdump*")
    )
    if hits:
        return hits[0]
    sys.exit("error: no llvm-objdump/objdump found (looked on PATH and in ~/.rustup)")


def disassemble(elf: str) -> str:
    if not os.path.exists(elf):
        sys.exit(
            f"error: no such kernel binary: {elf}\n"
            f"       build one first, e.g. (cd kernel && cargo build)"
        )
    out = subprocess.run(
        [find_objdump(), "-d", "--no-show-raw-insn", elf],
        capture_output=True,
        text=True,
        errors="replace",
    )
    if out.returncode != 0:
        sys.exit(f"error: objdump failed:\n{out.stderr[:2000]}")
    return out.stdout


def frames(disasm: str) -> dict[str, int]:
    """Map symbol -> bytes of stack claimed in its prologue."""
    result: dict[str, int] = {}
    cur: str | None = None
    acc = 0
    done = False

    def flush() -> None:
        nonlocal cur, acc, done
        if cur is not None and acc > 0:
            # Keep the max if a symbol somehow appears twice.
            result[cur] = max(result.get(cur, 0), acc)
        cur, acc, done = None, 0, False

    for line in disasm.splitlines():
        m = SYM_RE.match(line)
        if m:
            flush()
            cur = m.group(1)
            continue
        if cur is None or done:
            continue
        body = line.strip()
        m = SUB_RE.match(body)
        if m:
            val = m.group(1)
            acc += int(val, 16) if val.startswith("0x") else int(val)
        elif CALL_RE.match(body):
            done = True
    flush()
    return result


MANGLE_TAIL = re.compile(r"17h[0-9a-f]{16}E$")


def last_component(path: str) -> str | None:
    """`kernel_main` from `_ZN6kernel11kernel_main` -- the length-prefixed tail.

    Needed because a `#[unsafe(no_mangle)]` function appears in the symbol
    table under its plain name while its nested `fn case`s are still mangled
    under the *mangled* path, so the prefix match below finds no parent for
    them.  That silently dropped `kernel_main` -- the largest frame in the
    kernel, and the whole reason `--peak` exists -- from the report, which is
    the failure mode where a measurement tool answers a question it did not
    actually ask.
    """
    if not path.startswith("_ZN"):
        return None
    body, last, i = path[3:], None, 0
    while i < len(body):
        j = i
        while j < len(body) and body[j].isdigit():
            j += 1
        if j == i:
            break
        n = int(body[i:j])
        last = body[j : j + n]
        i = j + n
    return last


def peaks(table: dict[str, int]) -> list[tuple[int, int, int, int, str]]:
    """`(peak, outer, deepest case, how many cases, name)` per split function.

    Rust mangles a nested `fn case` as its parent's path with `4case` appended
    before the hash, so stripping the hash makes parent and children share a
    prefix.  All of a parent's cases collapse to one entry here, keeping the
    deepest -- which is the one that sets the peak, since only one case is live
    at a time.

    **The nesting can be more than one deep, and must be followed.**  A case
    large enough to be worth splitting again gets `fn case` inside `fn case`,
    and `net::httpd::self_test` is exactly that: one level of pairing reported
    its peak as 3 248 (outer 128 + case 3 120) when the case in question calls a
    nested case claiming a further 5 104, for a true peak of 8 352.  Reporting
    the shallow number is worse than reporting nothing, because it is the
    number a split is judged by -- so the walk below is recursive and only
    top-level parents are listed.
    """
    by_path: dict[str, int] = {}
    counts: dict[str, int] = {}
    for sym, n in table.items():
        p = MANGLE_TAIL.sub("", sym)
        by_path[p] = max(by_path.get(p, 0), n)
        counts[p] = counts.get(p, 0) + 1

    parent_of: dict[str, str] = {}
    children: dict[str, list[str]] = {}
    for p in by_path:
        if not p.endswith("4case"):
            continue
        par = p[: -len("4case")]
        if par not in by_path:
            bare = last_component(par)
            par = bare if bare and bare in by_path else ""
        if not par:
            continue
        parent_of[p] = par
        children.setdefault(par, []).append(p)

    def chain(p: str) -> tuple[int, int]:
        """`(deepest stack below p, how many case bodies below it)`."""
        deep, n = 0, 0
        for c in children.get(p, ()):
            d, k = chain(c)
            deep = max(deep, by_path[c] + d)
            n += counts[c] + k
        return deep, n

    rows = []
    for p in by_path:
        if p in parent_of or p not in children:
            continue
        deep, n = chain(p)
        rows.append((by_path[p] + deep, by_path[p], deep, n, p))
    rows.sort(reverse=True)
    return rows


def elf_for(profile: str) -> str:
    return os.path.join(REPO, "target", "x86_64-unknown-none", profile, "kernel")


def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--profile",
        default="debug",
        choices=("debug", "release"),
        help="which build to measure (default: debug -- the profile "
        "boot-test.sh uses, and the one canary halts come from)",
    )
    ap.add_argument("--elf", help="measure this binary instead of --profile's")
    ap.add_argument("--top", type=int, default=40, help="how many to list (default 40)")
    ap.add_argument("--min", type=int, default=0, help="only report frames >= N bytes")
    ap.add_argument("--filter", help="only report symbols containing this substring")
    ap.add_argument(
        "--peak",
        action="store_true",
        help="for functions split into `case` fns, report outer + deepest case "
        "-- what is really on the stack, which the per-symbol ranking hides",
    )
    ap.add_argument(
        "--diff",
        nargs=2,
        metavar=("BEFORE_ELF", "AFTER_ELF"),
        help="compare two binaries and report what moved",
    )
    args = ap.parse_args()

    if args.diff:
        before = frames(disassemble(args.diff[0]))
        after = frames(disassemble(args.diff[1]))
        rows = []
        for sym in set(before) | set(after):
            b, a = before.get(sym, 0), after.get(sym, 0)
            if b != a:
                rows.append((a - b, b, a, sym))
        rows.sort()
        print(f"{'delta':>9}  {'before':>8}  {'after':>8}  symbol")
        for d, b, a, sym in rows[: args.top]:
            print(f"{d:>+9}  {b:>8}  {a:>8}  {sym}")
        if len(rows) > 2 * args.top:
            print("   ...")
        for d, b, a, sym in rows[-args.top :]:
            print(f"{d:>+9}  {b:>8}  {a:>8}  {sym}")
        return

    elf = args.elf or elf_for(args.profile)
    table = frames(disassemble(elf))

    if args.peak:
        rows = peaks(table)
        if args.filter:
            rows = [r for r in rows if args.filter in r[4]]
        rows = [r for r in rows if r[0] >= args.min]
        print(elf)
        print(f"{'peak':>8}  {'outer':>8}  {'case':>8}  {'n':>3}  symbol\n")
        for peak, outer, case, n, sym in rows[: args.top]:
            print(f"{peak:8d}  {outer:8d}  {case:8d}  {n:3d}  {sym}")
        return

    rows = sorted(((n, s) for s, n in table.items()), reverse=True)
    if args.filter:
        rows = [r for r in rows if args.filter in r[1]]
    rows = [r for r in rows if r[0] >= args.min]

    big = sum(1 for n in table.values() if n >= 4096)
    print(elf)
    print(
        f"{len(table)} prologues; {big} claim >= 4 KiB "
        f"(a 64 KiB task stack is the budget)\n"
    )
    for n, sym in rows[: args.top]:
        print(f"{n:8d}  {sym}")


if __name__ == "__main__":
    main()
