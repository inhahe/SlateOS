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

## This is a count, and `--list` is not a delete list

Read the second half of the test again: *contains no call that could have
looked at one*. Any single I/O marker anywhere in the crate exonerates the
whole crate, which makes a false negative cheap and a false positive
possible -- and those two errors are not symmetric once the output is used to
decide what to **remove**.

Measured on 2026-09-10, when `design-decisions.md` 1006 turned this instrument
into a deletion list: of 2,287 crates flagged, exactly **7** consult anything
outside their own argv, and **2 of those 7 were real programs** -- `cal`, which
computes a genuine calendar from `SystemTime::now()`, and `earlyoom`, which
reads real `/proc` through `procinfo`. Both are now exonerated by markers added
below. The other five read an environment variable or stdin and go on to
fabricate their output regardless, so they stay flagged, correctly.

That is a false-positive rate of 2 in 2,287 -- but the two were found by
inspection, not by the audit, and nothing here can rule out a third. **Before a
bulk deletion, re-derive the list and review whatever it flags that also
consults the world.** Do not delete straight from `--list`.

The markers below were not free. Adding the clock also exonerated `snapper`,
which reads `SystemTime` for real snapshot timestamps and then fabricates the
file diff between two snapshots -- so the change traded two false positives for
one false negative. That is the right direction for a list used to delete
things, and the cost is named rather than absorbed: `ALSO_FABRICATING` carries
`snapper` back into the count. The underlying limit is structural and applies
to the original list just as much -- any single I/O call anywhere in a crate
exonerates every invented answer beside it, so this number has always been a
floor.

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
    # The clock is a system read like any other, and a program that asks it
    # what today is has not made today up. Missing from this list until
    # 2026-09-10, which flagged `cal` -- 711 lines of real calendar arithmetic
    # over `SystemTime::now()`, with a genuine month printed at the end of it.
    "SystemTime",
    "UNIX_EPOCH",
    "Instant::now",
    # "...or a workspace crate that does one of those on its behalf" -- this
    # module's own docstring, promising a class of marker that was never in the
    # list. `earlyoom` reads real `/proc` through `procinfo` and was flagged
    # anyway. The doc described the intent and the code did not implement it,
    # which is the same shape as a comment outliving the function under it.
    "procinfo::",
    "libcall::",
    "monoclock::",
    "randrange::",
)

# Crates that hold an I/O marker above and fabricate their output anyway.
#
# The whole-crate test cannot see these by construction: *any* marker anywhere
# in the sources exonerates the crate, so one real syscall covers for every
# invented answer beside it. That was already true of `std::fs` before this
# list grew; adding the clock made it true of one more crate, so the cost is
# recorded here rather than left as a silent loss.
#
# `snapper` reads `SystemTime` for snapshot timestamps and then computes the
# file changes between two snapshots with a function whose own comment says it
# "would walk the two snapshot directories" and "return a placeholder". The
# timestamps are real; the diff is not.
#
# This is a floor, not a ceiling: it holds what inspection has found, and
# nothing here can enumerate what it has not.
ALSO_FABRICATING = ("snapper",)

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
        if crate.name not in ALSO_FABRICATING and any(m in body for m in IO_MARKERS):
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
