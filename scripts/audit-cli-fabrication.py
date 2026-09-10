#!/usr/bin/env python3
"""One-off measurement: how many `userspace/*` CLI crates print a *report*
about work they never did?

It began as a measuring instrument for a `requests/` file, run by hand, and
deliberately not named `check-*.py`: `pre-boot.py` globs those into every
lane's gate, and with 2,288 hits in one lane's tree that would have handed the
other two a red gate they could not clear.

**That reason expired on 2026-09-10, when the deletion landed.** The floor is
now zero, so `--check` is trivially satisfiable by every lane -- the way to
clear it is to not add a command that lies, which no lane wants to do anyway.
It is still not named `check-*`, because run bare it prints a report rather
than a verdict and the glob runs scripts bare; `pre-boot.py` invokes it by
name with `--check`, exactly as it does `scan-orphan-modules.py --check`.

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


BASELINE = ROOT / "scripts" / "cli-fabrication-baseline.txt"

BASELINE_HEADER = """\
# Commands pinned by scripts/audit-cli-fabrication.py --check.
#
# design-decisions.md 1006 (Decided by: Operator): a command that does not work
# is deleted, not kept as a refusing stub. 2,285 were deleted on 2026-09-10 and
# this file is the ratchet that stops a bulk generation putting them back.
#
# It is EMPTY, and that is the pinned state. Every name added here is a command
# that states a fact it did not measure -- so an entry is a defect being
# tolerated, not a rule being configured.
#
# The list may only SHRINK. A name pinned here that no longer fabricates is
# also a failure: it means the fix landed and the pin outlived it, which is how
# a baseline rots into a permission slip. Re-pin with:
#
#     python scripts/audit-cli-fabrication.py --pin
"""


def read_baseline() -> set[str] | None:
    """The pinned set, or None if the file is absent."""
    if not BASELINE.is_file():
        return None
    names = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            names.add(line)
    return names


def fabricates(name: str, text: str) -> bool:
    """Does the crate `name`, with sources `text`, state a fact it never looked up?

    Both halves matter and the second is load-bearing: it must *assert*
    something, and it must contain no call that could have looked. Split out of
    `main` so `--self-test` can exercise it on synthetic sources -- in
    particular the two exemptions added on 2026-09-10, which were found by
    hand and would otherwise be pinned by nothing.
    """
    if name in PURE_ARGV:
        return False
    body = strip_tests(text)
    if name not in ALSO_FABRICATING and any(m in body for m in IO_MARKERS):
        return False
    return any(p.search(body) for p in FACT_PATTERNS)


def _self_test() -> int:
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got {got!r}, want {want!r}")

    FACT = 'fn main() { println!("/dev/sda1: 1024000 sectors, PASS"); }'

    expect("a crate that asserts a fact and touches nothing is flagged",
           fabricates("probe", FACT), True)
    expect("...and one that only prints usage is not",
           fabricates("probe", 'fn main() { println!("usage: probe [-v]"); }'), False)

    # The two exemptions this file gained on 2026-09-10, each pinned by the
    # program that would have been deleted without it.
    expect("reading the clock is looking at the world (the `cal` case)",
           fabricates("probe",
                      'use std::time::SystemTime;\n' + FACT), False)
    expect("so is reading /proc through procinfo (the `earlyoom` case)",
           fabricates("probe", 'let m = procinfo::meminfo();\n' + FACT), False)
    expect("...and so is libcall, for the same reason",
           fabricates("probe", 'libcall::sync();\n' + FACT), False)

    # The cost of those exemptions, also pinned.
    expect("a named exception is flagged despite holding an I/O marker",
           fabricates("snapper", 'use std::time::SystemTime;\n' + FACT), True)
    expect("...and the same source under any other name is not",
           fabricates("notsnapper", 'use std::time::SystemTime;\n' + FACT), False)

    expect("ordinary file I/O still exonerates",
           fabricates("probe", 'std::fs::read("/x")?;\n' + FACT), False)
    expect("argv-only tools are exempt by name",
           fabricates(sorted(PURE_ARGV)[0], FACT), False)

    # Assertions only inside #[cfg(test)] are not claims the program makes.
    expect("a fact asserted only in tests is not a fact the program states",
           fabricates("probe",
                      '#[cfg(test)]\nmod tests {\n' + FACT + '\n}\n'), False)

    print(f"audit-cli-fabrication: self-test "
          f"{'FAILED' if failures else 'passed'} ({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="print every hit")
    ap.add_argument("--limit", type=int, default=40, help="cap --list output")
    ap.add_argument("--check", action="store_true",
                    help="verdict against the pinned baseline; 1 if it rose")
    ap.add_argument("--pin", action="store_true",
                    help="rewrite the baseline from the current tree")
    ap.add_argument("--self-test", "--selftest", dest="self_test",
                    action="store_true", help="run this script's own fixtures")
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

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
        if fabricates(crate.name, text):
            fabricating.append(crate.name)

    if args.pin:
        BASELINE.parent.mkdir(parents=True, exist_ok=True)
        BASELINE.write_text(
            BASELINE_HEADER + "".join(f"{n}\n" for n in fabricating),
            encoding="utf-8",
            # newline="" added by lane A 2026-09-10 to un-red main:
            # scripts/check-text-mode-writes.py refuses a text-mode write that
            # does not say what it wants line endings to be, and it runs before
            # the build, so every lane's boot test stopped here. Without it this
            # baseline would be written CRLF on Windows while git reported the
            # file clean against an `eol=lf` attribute.
            newline="",
        )
        print(f"audit-cli-fabrication: pinned {len(fabricating)} name(s) "
              f"in {BASELINE.relative_to(ROOT)}")
        return 0

    if args.check:
        pinned = read_baseline()
        if pinned is None:
            print(f"audit-cli-fabrication: no baseline at "
                  f"{BASELINE.relative_to(ROOT)}; run --pin", file=sys.stderr)
            return 2
        current = set(fabricating)
        new = sorted(current - pinned)
        stale = sorted(pinned - current)
        for n in new:
            print(f"  ERROR {n} states a fact it did not measure, and is not "
                  f"pinned", file=sys.stderr)
        for n in stale:
            print(f"  ERROR {n} is pinned but no longer fabricates -- the pin "
                  f"outlived the fix; re-pin", file=sys.stderr)
        sys.stdout.flush()
        if new or stale:
            print(f"audit-cli-fabrication: FAILED ({len(new)} new, "
                  f"{len(stale)} stale) -- design-decisions.md 1006: a command "
                  f"that does not work is deleted, not stubbed", file=sys.stderr)
            return 1
        print(f"audit-cli-fabrication: OK ({len(current)} pinned, "
              f"{total} crate(s) scanned)")
        return 0

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
