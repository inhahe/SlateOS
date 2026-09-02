#!/usr/bin/env python3
"""One-off measurement: how many `userspace/*` CLI crates print a *report*
about work they never did?

This is NOT a `check-*.py` gate.  `pre-boot.py` globs `scripts/check-*.py`
into every lane's gate, and the tree this measures belongs to one lane, so
naming it that way would hand the other two lanes a red gate they cannot
clear.  It is a measuring instrument for a `requests/` file, run by hand.

## What it looks for

A crate is counted as **fabricating** when its whole source:

  * names at least one thing that only exists at run time -- a path from
    argv, a file, a device, a socket -- in the text it prints, AND
  * contains no call that could have looked at one.

The second half is the load-bearing test, and it is deliberately crude:
the set of ways a Rust program can touch the outside world is small and
enumerable (`std::fs`, `File`, `OpenOptions`, `read_to_string`, `metadata`,
`std::net`, `Command`, `libc::`, `nix::`, an `unsafe` FFI block, or a
workspace crate that does one of those on its behalf).  A crate that
contains none of them, prints numbers, and exits 0 has produced its answer
out of its own source text.

## What it deliberately does not flag

  * Crates that only print help/usage and exit -- `--help` is a report
    about the program itself, which the program does know.
  * Crates whose printed output is derived from argv alone and is *honest*
    about that (`echo`, `basename`, `printf`, `seq`, `yes`, `true`).  These
    are listed in PURE_ARGV below, because for them "no I/O" is correct.
  * Crates that shell out.  Delegating is not fabricating.

## Why "prints a number" is part of the test

A stub that says "not implemented" and exits 1 is honest and harmless.
The dangerous shape is the one that prints a plausible measurement -- a
duration, a bitrate, a device name, a PASS -- because a plausible
measurement is indistinguishable from a real one to the caller, and the
exit code says it worked.  So the scan requires evidence of *asserted
fact*, not merely absence of I/O.

Usage:  python scripts/audit-cli-fabrication.py [--list] [--limit N]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Any one of these means the crate can, in principle, have looked at the
# world before printing.  The list is intentionally generous: a false
# negative (a fabricator we miss) understates the problem, which is the
# safe direction for a number that is going into a request.
IO_MARKERS = (
    "std::fs",
    "fs::read",
    "fs::write",
    "fs::metadata",
    "fs::File",
    "File::open",
    "File::create",
    "OpenOptions",
    "read_to_string",
    "read_dir",
    "std::net",
    "TcpStream",
    "UdpSocket",
    "process::Command",
    "Command::new",
    "libc::",
    "nix::",
    "unsafe {",
    "io::stdin",
    "stdin()",
    "BufReader",
    "symlink_metadata",
    "std::env::var",   # reading the environment is looking at the world
)

# Evidence that the crate asserts a fact about something outside itself.
# A bare `println!("hello")` is not a claim; `println!("  Duration: ...")`
# with a number in it is.
FACT_PATTERNS = (
    re.compile(r'println!\("[^"]*\b\d+\.\d+\b'),          # a measurement
    re.compile(r'println!\("[^"]*\b\d{3,}\b'),            # a big count
    re.compile(r'println!\("[^"]*(?i:PASS|OK|found|success)'),
    re.compile(r'println!\("[^"]*(?i:bitrate|duration|fps|Hz|kb/s|MB|GB)'),
)

# Tools whose entire correct behaviour is a pure function of argv.  For
# these, "does no I/O" is the specification, not a defect.
PURE_ARGV = {
    "echo", "basename", "dirname", "printf", "seq", "yes", "true", "false",
    "expr", "test", "sleep", "arch", "uname-lite", "factor", "shuf-lite",
}


def crate_sources(crate: Path) -> str:
    src = crate / "src"
    if not src.is_dir():
        return ""
    parts = []
    for f in sorted(src.rglob("*.rs")):
        try:
            parts.append(f.read_text(encoding="utf-8", errors="replace"))
        except OSError:
            pass
    return "\n".join(parts)


def strip_tests(text: str) -> str:
    """Drop `#[cfg(test)]` modules -- a test fixture is not a claim."""
    idx = text.find("#[cfg(test)]")
    return text if idx < 0 else text[:idx]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="print every hit")
    ap.add_argument("--limit", type=int, default=40, help="cap --list output")
    args = ap.parse_args()

    userspace = ROOT / "userspace"
    if not userspace.is_dir():
        print("no userspace/ directory", file=sys.stderr)
        return 2

    total = 0
    fabricating: list[str] = []
    for crate in sorted(p for p in userspace.iterdir() if p.is_dir()):
        text = crate_sources(crate)
        if not text:
            continue
        total += 1
        if crate.name in PURE_ARGV:
            continue
        body = strip_tests(text)
        if any(m in body for m in IO_MARKERS):
            continue
        if any(p.search(body) for p in FACT_PATTERNS):
            fabricating.append(crate.name)

    print(f"userspace crates with sources : {total}")
    print(f"assert a fact, do no I/O      : {len(fabricating)}")
    if total:
        print(f"                              : {100 * len(fabricating) / total:.1f}%")
    if args.list:
        for name in fabricating[: args.limit]:
            print(f"  {name}")
        if len(fabricating) > args.limit:
            print(f"  ... and {len(fabricating) - args.limit} more")
    return 0


if __name__ == "__main__":
    sys.exit(main())
