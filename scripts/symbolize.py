#!/usr/bin/env python3
"""Turn the raw addresses in a kernel panic into `symbol+offset`.

    python scripts/symbolize.py 0xffffffff811a4089 0xffffffff810a6457
    python scripts/symbolize.py --log build/serial-test.txt
    python scripts/symbolize.py --profile release --log build/serial-test.txt
    python scripts/symbolize.py --elf build/kernel-5570b6b38.elf --log LOG
    python scripts/symbolize.py --self-test          # check the tool itself

Why this exists
---------------
`idt.rs` prints a fault as a faulting RIP, a stack scan of return-address
candidates, and a backtrace -- all as bare 64-bit hex, because a kernel that
is halting cannot look symbols up in its own ELF.  Every panic therefore
arrives as a wall of `0xffffffff811a4089`, and until this script existed each
one was read by hand, or not read at all: the HDA fault that broke boot 91 was
first triaged purely from the surrounding serial lines, which said *where the
boot had got to* but not *which function was running*.

The lookup is the obvious one -- `llvm-nm -n` gives every defined symbol sorted
by address, and a bisect finds the one containing a query -- with three details
that matter in practice:

* **The stack scan is mostly not addresses.**  `idt.rs` dumps every 8-byte word
  near RSP that *could* be a return address, so the list is full of data
  pointers and physmap addresses.  `--log` annotates only words that land
  inside a `t`/`T` (text) symbol, which is what separates the two.
* **A symbol's extent comes from `nm --print-size`, not from the gap to its
  neighbour.**  See below -- this is the one detail the script originally got
  wrong, and it made the tool answer confidently rather than not at all.
* **The ELF must be the one that booted.**  `boot-test.sh` stages a *stripped*
  copy, so the symbols live in `target/<triple>/<profile>/kernel`; a rebuild
  since the panic invalidates every address.  `--log` prints the ELF's mtime
  next to the log's so a stale pairing is visible rather than silently wrong.

Why "nearest preceding symbol" is not the answer
------------------------------------------------
The first version of this script bisected one address-sorted list of *all*
symbols and reported whatever came before the query.  The failure mode is the
bad one: it does not answer `??`, it answers a plausible name.  Two defects,
and it is worth being precise about which one was actually doing the damage --
the original bug report guessed, and the guess was half wrong.

1. **Gap-to-the-next-symbol is not a size.**  Taking a symbol's extent as the
   distance to its neighbour makes the last symbol before a section boundary
   appear to own the entire alignment gap.  `--max-offset` cannot rescue this,
   because it has to be *large* -- release LTO produces genuinely huge inlined
   functions -- so it cannot separate "inside a 150 KiB `kernel_main`" from
   "hundreds of KiB past a 2 KiB font table".  **This one reproduces exactly**:
   against the current debug ELF, `0xffffffff8254ea30` used to resolve to
   `drm::ati::KNOWN_DEVICES+0x3c28`, a *sixteen-byte* array claiming an address
   15 KiB past its own end.  It is the mechanism behind the `font::FONT_DATA+
   <hundreds of KiB>` frame seen while triaging
   `B-VIRTIO-GPU-FLAT-SCANOUT-WILD-WRITE`.
2. **Nearest-preceding ignores kind**, so a data symbol below a code address
   can outrank the function containing it.  Real in principle, and free to fix
   once the table is split -- but **it does not reproduce in this kernel**, and
   the honest thing is to say so: a scan of all 119729 sized text symbols finds
   *zero* with a non-text symbol interleaved inside their extent.  Which means
   the sibling frame in that same triage, `kernel::KERNEL_BOOT_STACK+0xcc323
   [b]`, was not caused by this.  `KERNEL_BOOT_STACK` records a 2 MiB size, so
   `+0xcc323` lies genuinely inside it and is a *correct* answer for a `.bss`
   address.  For it to have been a text address, the ELF consulted must have had
   a different layout from the one that panicked -- i.e. it was the stale-ELF
   failure, which `pick_profile` below fixed on the same day.  Kind-aware search
   is still the right structure; it just was not the bug.

The fix is to stop guessing.  `nm --print-size` reports a real `st_size` for
**every** symbol in this kernel (122051 of 122051 rows; only 55 text symbols
carry size 0, and those are the naked-asm ISR stubs and the `__text_start`
style section markers, which genuinely have none).  So:

* an extent is `addr + st_size` when a size is recorded, and the flat
  `--max-offset` cap does **not** apply to it -- a real size is better evidence
  than a heuristic, and applying both would reject the large inlined functions
  the cap was widened for in the first place;
* only the size-0 stragglers fall back to gap-to-the-next-address, bounded by
  `--max-offset`, and the gap is measured against *every* symbol rather than
  the same-kind neighbour, which is the tighter bound;
* the search runs over text symbols first and falls back to the full table, so
  a code address can no longer be captured by a data symbol that merely sits
  below it.

`?? (no symbol covers this address)` is a correct answer and is now reachable.
Naming a variable for a code address never was.

Note `--profile`: it defaults to `auto`, which reads whichever kernel ELF was
built most recently -- `boot-test.sh` builds debug for every run except
`--bench`, which builds release. Pass `--profile debug|release` to pin it. The
chosen ELF and its mtime are always reported, because an address resolved
against the other profile's binary gives `??` at best and a confidently wrong
symbol at worst.
"""

from __future__ import annotations

import argparse
import bisect
import glob
import os
import re
import shutil
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TRIPLE = "x86_64-unknown-none"

# `llvm-nm -n --print-size`: `ADDR SIZE T name`.  Both llvm-nm and GNU nm omit
# the SIZE column for a symbol that has none, so both shapes must be accepted --
# and the sized one must be tried first, since `ADDR SIZE T name` also matches
# the unsized pattern with SIZE mistaken for the kind.
NM_SIZED_RE = re.compile(r"^([0-9a-fA-F]+)\s+([0-9a-fA-F]+)\s+(\S)\s+(.*)$")
NM_RE = re.compile(r"^([0-9a-fA-F]+)\s+(\S)\s+(.*)$")

# A kernel address as `idt.rs` prints it.  Anchored to the `0x` so a bare hex
# column (e.g. `error=0x2`) is only picked up when it is address-shaped.
ADDR_RE = re.compile(r"0x(f{8}[0-9a-f]{8}|[0-9a-f]{12,16})")

TEXT_KINDS = set("tT")


def find_nm() -> str:
    for name in ("llvm-nm", "nm"):
        p = shutil.which(name)
        if p:
            return p
    home = os.path.expanduser("~/.rustup/toolchains")
    hits = glob.glob(os.path.join(home, "*", "lib", "rustlib", "*", "bin", "llvm-nm*"))
    if hits:
        return hits[0]
    sys.exit("error: no llvm-nm/nm found (looked on PATH and in ~/.rustup)")


def demangle(sym: str) -> str:
    """Strip Rust's `_ZN..17h<hash>E` mangling down to a readable path.

    `llvm-nm -C` would do this, but it is inconsistent across the toolchains on
    this machine (the `-windows-gnu` one leaves v0 symbols alone), and the tail
    hash is noise in a backtrace either way.
    """
    if not sym.startswith("_ZN") or not sym.endswith("E"):
        return sym
    body, out, i = sym[3:-1], [], 0
    while i < len(body):
        j = i
        while j < len(body) and body[j].isdigit():
            j += 1
        if j == i:
            return sym
        n = int(body[i:j])
        part = body[j : j + n]
        i = j + n
        # The trailing `17h<16 hex>` is the disambiguating hash, not a path
        # component; a reader never wants it.
        if not (part.startswith("h") and len(part) == 17):
            out.append(part)
    return "::".join(out)


class _View:
    """A bisect-able slice of the symbol table: either all of it, or text only.

    Two views exist because the kind of a symbol has to be part of the *search*,
    not a filter applied to its result.  Filtering afterwards answers `??` for a
    code address whose nearest preceding symbol happens to be a variable, when
    the containing function is sitting right there a little further back.
    """

    def __init__(self, rows: list[tuple[int, int, str, str]], ends: list[int | None]):
        self.rows = rows
        self.ends = ends
        self.addrs = [r[0] for r in rows]

    def lookup(self, addr: int, max_offset: int):
        """`(name, offset, kind)` for the symbol *covering* `addr`, else None."""
        j = bisect.bisect_right(self.addrs, addr) - 1
        if j < 0:
            return None
        # Several symbols can alias one address (`__requests_start` beside
        # `REQUESTS_START`, `__text_start` beside the first function).  Prefer
        # the one that carries a real size, then a text one: a backtrace entry
        # reading `foo+0x12` is more useful than the section marker beside it.
        base = self.addrs[j]
        cands = [k for k in range(j, -1, -1) if self.addrs[k] == base]

        def rank(k: int) -> tuple[int, int]:
            _, size, kind, _ = self.rows[k]
            return (1 if size else 0, 1 if kind in TEXT_KINDS else 0)

        for k in sorted(cands, key=rank, reverse=True):
            _, size, kind, name = self.rows[k]
            off = addr - base
            end = self.ends[k]
            if size:
                # A recorded st_size is hard evidence; `--max-offset` is a
                # heuristic for the absence of one, so it does not get a vote
                # here.  Applying both would reject exactly the huge LTO-inlined
                # functions the flat cap was widened to accommodate.
                if off < size:
                    return (name, off, kind)
            elif end is not None:
                if addr < end and off <= max_offset:
                    return (name, off, kind)
            elif off <= max_offset:
                # Size 0 and nothing after it in the whole table: the very last
                # symbol.  Only `--max-offset` can bound this one.
                return (name, off, kind)
        return None


class Symbols:
    def __init__(self, elf: str) -> None:
        if not os.path.exists(elf):
            sys.exit(
                f"error: no such kernel binary: {elf}\n"
                f"       build one first, e.g. (cd kernel && cargo build)"
            )
        self.elf = elf
        out = subprocess.run(
            [find_nm(), "--defined-only", "--print-size", "-n", elf],
            capture_output=True,
            text=True,
            errors="replace",
        )
        if out.returncode != 0:
            sys.exit(f"error: nm failed:\n{out.stderr[:2000]}")

        # (addr, size, kind, name); size 0 means "nm recorded none".
        rows: list[tuple[int, int, str, str]] = []
        for line in out.stdout.splitlines():
            m = NM_SIZED_RE.match(line)
            if m:
                rows.append(
                    (int(m.group(1), 16), int(m.group(2), 16), m.group(3), m.group(4).strip())
                )
                continue
            m = NM_RE.match(line)
            if m:
                rows.append((int(m.group(1), 16), 0, m.group(2), m.group(3).strip()))
        if not rows:
            sys.exit(f"error: nm reported no symbols for {elf}")
        rows.sort(key=lambda r: r[0])

        # The gap to the next *distinct* address, over the whole table.  Only
        # size-0 symbols use it, and measuring it against every symbol rather
        # than the same-kind neighbour is the tighter -- so safer -- bound: a
        # data symbol starting at X proves the size-0 function before it ended
        # by X, which a text-only walk would not have seen.
        all_addrs = [r[0] for r in rows]
        ends: list[int | None] = [None] * len(rows)
        for k, r in enumerate(rows):
            j = bisect.bisect_right(all_addrs, r[0])
            ends[k] = all_addrs[j] if j < len(rows) else None

        self.rows = rows
        self.all = _View(rows, ends)
        text = [(k, r) for k, r in enumerate(rows) if r[2] in TEXT_KINDS]
        self.text = _View([r for _, r in text], [ends[k] for k, _ in text])
        self.n_sized = sum(1 for r in rows if r[1])

    def lookup(self, addr: int, max_offset: int, text_only: bool = False):
        """`(name, offset, kind)` for the symbol containing `addr`, else None.

        Text symbols are searched first even when `text_only` is false, so that
        a code address cannot be captured by a data symbol that merely sits
        below it -- the `KERNEL_BOOT_STACK+0xcc323` failure this replaces.  The
        preference is safe in the other direction because sizes are exact: a
        genuine `.bss` address is not covered by any function, so the text view
        misses it and the full view answers.
        """
        hit = self.text.lookup(addr, max_offset)
        if hit is not None or text_only:
            return hit
        return self.all.lookup(addr, max_offset)

    def nearest_below(self, addr: int):
        """The closest symbol at or below `addr`, covering or not, for a miss.

        A bare `??` is honest but unhelpful; "0x40 past the end of `foo`" tells
        the reader whether they are looking at a slightly-stale ELF or at an
        address that was never a code pointer in the first place.
        """
        j = bisect.bisect_right(self.all.addrs, addr) - 1
        if j < 0:
            return None
        base, size, kind, name = self.rows[j]
        return (name, addr - base, kind, size)


def fmt(hit) -> str:
    name, off, kind = hit
    return f"{demangle(name)}+0x{off:x} [{kind}]" if off else f"{demangle(name)} [{kind}]"


def elf_for(profile: str) -> str:
    return os.path.join(REPO, "target", TRIPLE, profile, "kernel")


def self_test(syms: "Symbols", max_offset: int) -> int:
    """Check the lookup against the ELF itself.  `--self-test`; 0 = all good.

    The cases are derived from the binary at run time rather than hard-coded,
    because a hard-coded address is stale the moment the kernel is rebuilt --
    which is the same class of mistake this whole script exists to catch.
    """
    fails: list[str] = []

    def check(name: str, cond: bool, detail: str) -> None:
        print(f"[symbolize] {'ok  ' if cond else 'FAIL'} {name}")
        if not cond:
            print(f"[symbolize]      {detail}")
            fails.append(name)

    sized_text = [r for r in syms.rows if r[2] in TEXT_KINDS and r[1] > 0x20]
    check(
        "the ELF has sized text symbols to test against",
        bool(sized_text),
        "nm reported no sized t/T symbols -- is --print-size supported here?",
    )
    if not sized_text:
        return 1

    # 1. Every sized function resolves to itself at its first and last byte.
    #    Sampled rather than exhaustive: 122k * 2 bisects is slow enough that
    #    nobody would run the check, and a check nobody runs is not a check.
    step = max(1, len(sized_text) // 400)
    bad_edge = None
    for r in sized_text[::step]:
        addr, size, _, name = r
        for probe, what in ((addr, "first byte"), (addr + size - 1, "last byte")):
            hit = syms.lookup(probe, max_offset)
            if hit is None or hit[0] != name:
                got = fmt(hit) if hit else "??"
                bad_edge = f"{demangle(name)} {what} 0x{probe:x} -> {got}"
                break
        if bad_edge:
            break
    check("a sampled function covers its own first and last byte", bad_edge is None, bad_edge or "")

    # 2. One past the end of a function is *not* that function.  This is the
    #    property gap-to-the-next-symbol could not express, and the one that
    #    produced `KNOWN_DEVICES+0x3c28` for a 16-byte array.
    gapped = None
    for addr, size, kind, name in syms.rows:
        if size == 0:
            continue
        j = bisect.bisect_right(syms.all.addrs, addr + size - 1)
        # Want a real hole after this symbol, so "one past the end" is not
        # simply the next symbol's first byte.
        if j < len(syms.rows) and syms.all.addrs[j] > addr + size + 0x100:
            gapped = (addr, size, kind, name)
            break
    if gapped is None:
        check("found a symbol with a hole after it", False, "no padding gap in this ELF?")
    else:
        addr, size, kind, name = gapped
        probe = addr + size + 0x40
        hit = syms.lookup(probe, max_offset)
        check(
            "an address in the padding after a symbol is not that symbol",
            hit is None or hit[0] != name,
            f"0x{probe:x} is 0x40 past the end of {demangle(name)} "
            f"(size 0x{size:x}) but resolved to {fmt(hit) if hit else '??'}",
        )

    # 3. A size-0 text symbol (the naked-asm ISR stubs) still resolves, via the
    #    gap fallback.  Losing these to an over-strict size check would be a
    #    regression in the opposite direction, and they are exactly the symbols
    #    an early-boot fault lands in.
    stub = next((r for r in syms.rows if r[2] in TEXT_KINDS and r[1] == 0), None)
    if stub is None:
        print("[symbolize] skip  no size-0 text symbol in this ELF")
    else:
        hit = syms.lookup(stub[0], max_offset)
        check(
            "a size-0 text symbol still resolves",
            hit is not None and hit[2] in TEXT_KINDS,
            f"0x{stub[0]:x} ({stub[3]}) -> {fmt(hit) if hit else '??'}",
        )

    # 4. text_only never answers with a data symbol -- the guarantee `--log`
    #    leans on to tell return addresses from the data words beside them.
    data = next((r for r in syms.rows if r[2] not in TEXT_KINDS and r[1] > 0x100), None)
    if data is not None:
        hit = syms.lookup(data[0] + 0x10, max_offset, text_only=True)
        check(
            "text_only refuses a data address",
            hit is None,
            f"0x{data[0] + 0x10:x} (inside {demangle(data[3])} [{data[2]}]) "
            f"-> {fmt(hit) if hit else '??'}",
        )

    if fails:
        print(f"[symbolize] {len(fails)} check(s) failed")
        return 1
    print(f"[symbolize] ok: all checks passed against {syms.elf}")
    return 0


def pick_profile(profile: str) -> tuple[str, str]:
    """Resolve `--profile` to a concrete `(profile, elf_path)`.

    `auto` picks whichever of the two kernel ELFs was built most recently,
    which is the one the log being read almost certainly came from.

    This exists because a fixed default is wrong roughly half the time and
    fails *silently*.  The default used to be `release`, justified in the help
    text as "what `boot-test.sh` builds" -- but that stopped being true on
    2026-08-14, when the script changed to build **debug** for every run except
    `--bench` (see `boot-test.sh`, "`--bench` DEFAULTS to `--release`; every
    other run defaults to debug").  From then on the common case -- resolve an
    address out of an ordinary boot log -- read the stale *release* ELF.  The
    lucky outcome is `??` for a symbol that plainly exists, which is what
    happened to the lockdep lock addresses on 2026-08-22; the unlucky one is a
    confidently wrong symbol, the exact failure the module docstring warns
    about, caused by the tool's own default.
    """
    if profile != "auto":
        return profile, elf_for(profile)
    built = [(p, elf_for(p)) for p in ("debug", "release") if os.path.exists(elf_for(p))]
    if not built:
        # Neither exists; hand back debug so `Symbols` emits its own build hint.
        return "debug", elf_for("debug")
    built.sort(key=lambda pe: os.path.getmtime(pe[1]), reverse=True)
    return built[0]


def mtime(path: str) -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(os.path.getmtime(path)))


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("addrs", nargs="*", help="addresses to resolve (hex, 0x optional)")
    ap.add_argument(
        "--profile",
        default="auto",
        choices=("auto", "debug", "release"),
        help="which build the panic came from (default: auto -- use whichever "
        "kernel ELF was built most recently, since that is the one the log came "
        "from; `boot-test.sh` builds debug for every run except --bench). An "
        "address resolved against the other profile's ELF gives `??`, or worse, "
        "a confidently wrong symbol -- so the chosen ELF is always reported",
    )
    ap.add_argument("--elf", help="use this binary instead of --profile's")
    ap.add_argument(
        "--log",
        help="annotate every address-shaped word in this serial log "
        "(build/serial-test.txt) that lands in a function",
    )
    ap.add_argument(
        "--max-offset",
        type=lambda s: int(s, 0),
        default=0x100000,
        help="bound the extent of a symbol nm reported *no size* for (default "
        "0x100000). It does not apply to a sized symbol -- a real st_size is "
        "better evidence than a flat cap, and in this kernel only ~55 text "
        "symbols (the naked-asm ISR stubs and the section markers) lack one. "
        "Large on purpose for those: release LTO inlines the boot path into "
        "one ~150 KiB `kernel_main`",
    )
    ap.add_argument(
        "--all",
        action="store_true",
        help="with --log, annotate data symbols too, not only functions",
    )
    ap.add_argument(
        "--self-test",
        action="store_true",
        help="check the lookup against the chosen ELF and exit (0 = all good)",
    )
    args = ap.parse_args()

    if not args.addrs and not args.log and not args.self_test:
        ap.error("give some addresses, or --log to annotate a serial log")

    if args.elf:
        elf, chosen = args.elf, "--elf"
    else:
        chosen, elf = pick_profile(args.profile)
    syms = Symbols(elf)

    if args.self_test:
        print(
            f"# elf {elf}  ({mtime(elf)}, {len(syms.rows)} symbols, "
            f"{syms.n_sized} sized, profile: {chosen})"
        )
        return self_test(syms, args.max_offset)

    # Say which binary answered, on stderr so stdout stays pipeable. Provenance
    # is not a nicety here: every wrong answer this tool can give comes from
    # reading the wrong ELF, and staying silent about which one it read is what
    # makes that failure hard to spot. `--log` prints its own banner (on stdout,
    # as part of the annotated report), so don't repeat it for a pure log run.
    if args.addrs:
        print(
            f"# elf {elf}  ({mtime(elf)}, {len(syms.rows)} symbols, profile: {chosen})",
            file=sys.stderr,
        )

    misses = 0
    for a in args.addrs:
        addr = int(a, 16)
        hit = syms.lookup(addr, args.max_offset)
        if hit:
            print(f"0x{addr:016x}  {fmt(hit)}")
            continue
        misses += 1
        # Say *why* nothing covers it.  `??` alone leaves the reader unable to
        # tell a slightly-stale ELF (lands just past a real function) from an
        # address that was never a code pointer (lands megabytes into `.bss`),
        # and those want opposite next steps.
        near = syms.nearest_below(addr)
        if near is None:
            print(f"0x{addr:016x}  ?? (below every symbol in this ELF)")
        else:
            name, off, kind, size = near
            extent = f"size 0x{size:x}" if size else "no recorded size"
            print(
                f"0x{addr:016x}  ?? (no symbol covers it; nearest below is "
                f"{demangle(name)} [{kind}] at -0x{off:x}, {extent})"
            )
    if misses and not args.elf and args.profile == "auto":
        other = "release" if chosen == "debug" else "debug"
        if os.path.exists(elf_for(other)):
            print(
                f"# {misses} address(es) resolved to `??`. If they came from a "
                f"{other} build, retry with --profile {other} -- an address is "
                f"only meaningful against the ELF that actually booted.",
                file=sys.stderr,
            )

    if args.log:
        print(
            f"# elf {elf}  ({mtime(elf)}, {len(syms.rows)} symbols, profile: {chosen})"
        )
        print(f"# log {args.log}  ({mtime(args.log)})")
        print("# a stale ELF resolves every address to a wrong symbol -- "
              "compare the two times above")
        with open(args.log, encoding="utf-8", errors="replace") as f:
            for n, line in enumerate(f, 1):
                line = line.rstrip("\n")
                hits = []
                for m in ADDR_RE.finditer(line):
                    addr = int(m.group(1), 16)
                    h = syms.lookup(addr, args.max_offset, text_only=not args.all)
                    if h:
                        hits.append(f"0x{addr:x} = {fmt(h)}")
                if hits:
                    print(f"{n}: {line}")
                    for h in hits:
                        print(f"        -> {h}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
