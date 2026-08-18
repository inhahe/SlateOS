#!/usr/bin/env python3
"""Turn the raw addresses in a kernel panic into `symbol+offset`.

    python scripts/symbolize.py 0xffffffff811a4089 0xffffffff810a6457
    python scripts/symbolize.py --log build/serial-test.txt
    python scripts/symbolize.py --profile release --log build/serial-test.txt
    python scripts/symbolize.py --elf build/kernel-5570b6b38.elf --log LOG

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
* **Sizes are not in `nm`'s output**, so a symbol's extent is taken as the gap
  to the next one, and `--max-offset` is a second guard for the one case that
  cannot cover: a hit in the padding after the *last* symbol of a section.
  Its default is deliberately large (1 MiB), because in a release build LTO
  inlines nearly the whole boot path into `kernel_main` -- the HDA fault above
  resolved to `kernel_main+0x2330c`, and a 64 KiB cap answered `??` for it.
* **The ELF must be the one that booted.**  `boot-test.sh` stages a *stripped*
  copy, so the symbols live in `target/<triple>/<profile>/kernel`; a rebuild
  since the panic invalidates every address.  `--log` prints the ELF's mtime
  next to the log's so a stale pairing is visible rather than silently wrong.

Note `--profile release`: `--bench` runs build release, and a release address
resolved against the debug ELF gives a confidently wrong symbol.
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

# `llvm-nm -n`: `ADDR T name`, one per line.  Undefined/absolute symbols have
# no address and are dropped by `--defined-only`, but guard anyway.
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


class Symbols:
    def __init__(self, elf: str) -> None:
        if not os.path.exists(elf):
            sys.exit(
                f"error: no such kernel binary: {elf}\n"
                f"       build one first, e.g. (cd kernel && cargo build)"
            )
        self.elf = elf
        out = subprocess.run(
            [find_nm(), "--defined-only", "-n", elf],
            capture_output=True,
            text=True,
            errors="replace",
        )
        if out.returncode != 0:
            sys.exit(f"error: nm failed:\n{out.stderr[:2000]}")

        rows: list[tuple[int, str, str]] = []
        for line in out.stdout.splitlines():
            m = NM_RE.match(line)
            if m:
                rows.append((int(m.group(1), 16), m.group(2), m.group(3).strip()))
        if not rows:
            sys.exit(f"error: nm reported no symbols for {elf}")
        rows.sort(key=lambda r: r[0])

        self.addrs = [r[0] for r in rows]
        self.rows = rows
        # `nm` emits no sizes, so a symbol runs to the next *distinct* address.
        # Aliases at one address (`__requests_start` / `REQUESTS_START`) must
        # not give the first of them a zero-length extent.
        self.ends: list[int] = [0] * len(rows)
        nxt = rows[-1][0]
        for k in range(len(rows) - 1, -1, -1):
            if rows[k][0] != nxt:
                nxt = rows[k][0] if k == len(rows) - 1 else self.addrs[k + 1]
            self.ends[k] = nxt
        # Recompute plainly: end of k is the next address greater than addrs[k].
        for k in range(len(rows)):
            j = bisect.bisect_right(self.addrs, rows[k][0])
            self.ends[k] = self.addrs[j] if j < len(rows) else rows[-1][0] + 1

    def lookup(self, addr: int, max_offset: int, text_only: bool = False):
        """`(name, offset, kind)` for the symbol containing `addr`, else None."""
        j = bisect.bisect_right(self.addrs, addr) - 1
        if j < 0:
            return None
        # Prefer a text symbol when several alias one address: a backtrace entry
        # inside `foo` is more useful than the section marker beside it.
        cands = [k for k in range(j, -1, -1) if self.addrs[k] == self.addrs[j]]
        best = None
        for k in cands:
            _, kind, name = self.rows[k]
            if text_only and kind not in TEXT_KINDS:
                continue
            if best is None or (kind in TEXT_KINDS and best[2] not in TEXT_KINDS):
                best = (name, addr - self.addrs[k], kind)
        if best is None:
            return None
        if addr >= self.ends[j] or best[1] > max_offset:
            return None
        return best


def fmt(hit) -> str:
    name, off, kind = hit
    return f"{demangle(name)}+0x{off:x} [{kind}]" if off else f"{demangle(name)} [{kind}]"


def elf_for(profile: str) -> str:
    return os.path.join(REPO, "target", TRIPLE, profile, "kernel")


def mtime(path: str) -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(os.path.getmtime(path)))


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("addrs", nargs="*", help="addresses to resolve (hex, 0x optional)")
    ap.add_argument(
        "--profile",
        default="release",
        choices=("debug", "release"),
        help="which build the panic came from (default: release -- what "
        "`boot-test.sh` builds; a release address resolved against the debug "
        "ELF gives a confidently wrong symbol)",
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
        help="report `??` rather than a huge +offset past a symbol "
        "(default 0x100000); nm gives no sizes, so the last symbol before a "
        "section gap otherwise appears to own the whole gap. Large on purpose: "
        "release LTO inlines the boot path into one ~150 KiB `kernel_main`",
    )
    ap.add_argument(
        "--all",
        action="store_true",
        help="with --log, annotate data symbols too, not only functions",
    )
    args = ap.parse_args()

    if not args.addrs and not args.log:
        ap.error("give some addresses, or --log to annotate a serial log")

    elf = args.elf or elf_for(args.profile)
    syms = Symbols(elf)

    for a in args.addrs:
        addr = int(a, 16)
        hit = syms.lookup(addr, args.max_offset)
        print(f"0x{addr:016x}  {fmt(hit) if hit else '??'}")

    if args.log:
        print(f"# elf {elf}  ({mtime(elf)}, {len(syms.rows)} symbols)")
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
