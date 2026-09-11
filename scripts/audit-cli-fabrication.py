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

## The second rule, added 2026-09-10 after the first deletion

Everything above tests the *wording* of what a crate prints, and wording is
the wrong place to look for the worst cases.

  `cryptsetup luksFormat` printed `LUKS2 formatted successfully on /dev/sda1.`
  and exited 0 having written nothing to the device. There is no number in
  that sentence, no unit, no `PASS` -- so `FACT_PATTERNS` read it as harmless.
  It printed the genuine warning first (`This will overwrite data on /dev/sda1
  irrevocably`), which made its output indistinguishable from a real run down
  to the part that tells the user to be careful.

So the second rule ignores wording entirely and asks a structural question: a
crate that **builds a binary**, is **not a pure-argv tool**, and holds **no
call that could look at anything outside its own arguments** can only print
what was compiled into it. That is not a claim about what it says; it is a
claim about what it could possibly know.

The three clauses are all load-bearing:

  * *builds a binary* excludes libraries, where doing no I/O is unremarkable
    rather than a defect (`charwidth`, `bignum`, `ere`, `modechange`,
    `notimpl` are all in the tree and all fine).
  * *not pure-argv* is `PURE_ARGV`, where "does no I/O" is the specification
    (`echo`, `basename`, `seq`).
  * *no I/O marker* is the same crude enumeration used above, with the same
    known limits.

`FACT_PATTERNS` anchors every pattern on `println!("`, so rule 1 is blind to
a crate that prints through `writeln!`, `write!`, or a byte-slice helper --
which is not a hypothetical: the deleted `cryptsetup` made all 251 of its
output calls through `writeln!(out, ...)` and `hdparm` used
`print_out(b"...")`. Neither could have been caught by rule 1 whatever its
wording, and "successfully" is in `FACT_PATTERNS` already. Rule 2 covers that
gap for crates that do no I/O, which is the only place it could be covered
cheaply; for a crate that does real I/O and fabricates beside it, the macro
blindness still applies and `ALSO_FABRICATING` remains the only remedy.

**The two rules are reported separately and should stay separate**, because
they justify deletion under different halves of 1006. Rule 1 is *fabricating*
-- it states a fact it did not measure. Rule 2 is *inert* -- it may state no
fact at all and still be a command that does not work, which 1006 deletes just
the same. A crate whose entire behaviour is printing its own usage text is
inert, not fabricating, and blurring the two would make the audit's output a
worse description of what it found.

220 crates were deleted by rule 2 on 2026-09-10, after checking the set three
independent ways (dependencies, a wider net than `IO_MARKERS`, and reverse
dependencies) -- see the commit and `known-issues.md`. The warning above still
applies in full: do not delete straight from `--list`.

Usage:  python scripts/audit-cli-fabrication.py [--list] [--limit N]
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rustlex  # noqa: E402

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
    # NOT `std::net`, which was a marker until 2026-09-10 and exonerated
    # `ipcalc` for `use std::net::{Ipv4Addr, Ipv6Addr}`. Those are address
    # *types* -- parsing and arithmetic, no socket anywhere in the crate. The
    # module is mostly pure data structures; only the four names below open
    # anything, and all four were already listed, so this is a narrowing with
    # no loss of coverage.
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "process::Command",
    "Command::new",
    "libc::",
    "nix::",
    # `unsafe {` was a marker until 2026-09-10 and was the worst of them.
    #
    # A freestanding binary has to walk `argv` -- `slice::from_raw_parts(argv,
    # argc)` and a null-terminated-string scan per argument -- and both are
    # unsafe. So every crate that reads its own command line was exonerated
    # for doing exactly that, which is the one thing the audit is not
    # interested in: argv is the crate's input, not the world.
    #
    # It hid `fstrim`, `iw`, `modprobe` and `smartctl`, whose every unsafe
    # block is `cstr_to_slice`/`from_raw_parts` and nothing else. fstrim was
    # the one that gave it away: it carries a section banner reading
    # "Simulated Filesystem Operations -- in a real OS, these would issue
    # FITRIM ioctl, BLKDISCARD ioctl, etc. For now, we simulate the logic to
    # demonstrate output formatting."
    #
    # What actually reaches the kernel from a crate with no `libc::` and no
    # `extern "C" {` block is INLINE ASSEMBLY, which was never a marker:
    #
    #     core::arch::asm!("syscall", inlateout("rax") nr => ret, ...)
    #
    # `arp`, `traceroute` and `libservicebus` do precisely that and are real.
    # Dropping `unsafe {` without adding `asm!` in the same change would have
    # deleted all three -- the `cal` and `earlyoom` mistake a third time, so
    # the swap is deliberately one edit.
    "asm!",
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
    # Subnet arithmetic: given an address and a prefix it computes the
    # network, broadcast, mask and host range. Every answer is a function of
    # the arguments and there is nothing on the machine it could consult --
    # `ipcalc 10.0.0.0/8` is the same on every host in the world. It was
    # exonerated by `std::net` until that marker was narrowed above, and
    # belongs here rather than in the deletion set: rule 2 correctly asks the
    # question, and this is the answer.
    "ipcalc",
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
    """Drop every `#[cfg(test)]` module -- a test fixture is not a claim.

    # Why this is brace-matched rather than a truncation

    It used to be `text.find("#[cfg(test)]")` followed by `text[:idx]`, which
    is correct for one file whose tests sit at the bottom and **wrong for
    every multi-file crate**, because `crate_sources` concatenates all of
    `src/**/*.rs` before this runs. The first test module in the
    alphabetically-first file therefore discarded every later file.

    That skewed the audit in both directions at once, which is why it went
    unnoticed:

    * I/O markers in a later file were invisible, so a crate that genuinely
      reads the world could be reported as doing no I/O -- a FALSE POSITIVE,
      and this instrument's output was used to delete 2,285 crates.
    * `FACT_PATTERNS` in a later file were equally invisible, so a crate that
      does fabricate could be missed -- a false negative.

    `userspace/oils`, a shell with dozens of modules, measured as having no
    I/O of any kind.

    # Why this delegates to `rustlex.live_code` rather than matching braces here

    It matched braces over the RAW text, so a `{` or `}` inside a string or a
    comment moved the depth count and an item could close early or late.
    `live_code` matches over `strip_noise` output, where strings and comments
    are blanked, which is the same job done once and done right.

    The two could not be merged until 2026-09-11, and the reason is worth
    keeping: `live_code` used to CUT at the first `#[cfg(test)] mod` instead of
    blanking it and carrying on. Calling it from here would have reintroduced
    precisely the bug described above -- `crate_sources` concatenates every
    file in the crate, so a cut at the first test module discards every later
    file. Now that it blanks and continues, the contracts are identical.
    """
    live, _ = rustlex.live_code(text)
    return live


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


# Markers are matched on a word boundary, not as bare substrings, because
# `"nix::" in body` is true of `std::os::unix::ffi::OsStrExt`.
#
# That is not hypothetical and the way it surfaced is worth keeping. The only
# `nix::` in `userspace/cargo-bloat` was inside a line the crate INVENTS:
#
#     println!("   0.1%    0.2%   3.1K  std::sys::pal::unix::process");
#
# so a fabricated string was read as evidence that the crate had looked at
# something, and the audit exonerated it with its own fiction. It reads a file
# nowhere and prints a size table; rule 1 catches it the moment this is fixed.
#
# The lookbehind is only for word characters, so `io::stdin` still matches
# inside `std::io::stdin` (the preceding `:` is not a word character) while
# `nix::` no longer matches inside `unix::`.
def _one_marker_re(marker: str) -> re.Pattern[str]:
    """One marker, boundary-guarded. Cached so `--markers` is not quadratic."""
    rx = _MARKER_CACHE.get(marker)
    if rx is None:
        rx = re.compile(f"(?<![A-Za-z0-9_]){re.escape(marker)}")
        _MARKER_CACHE[marker] = rx
    return rx


_MARKER_CACHE: dict[str, re.Pattern[str]] = {}

_MARKER_RE = re.compile(
    "|".join(f"(?<![A-Za-z0-9_]){re.escape(m)}" for m in IO_MARKERS)
)


def has_io_marker(body: str) -> bool:
    """Could this crate, in principle, have looked at the world?"""
    return _MARKER_RE.search(body) is not None


def builds_binary(body: str) -> bool:
    """Does this crate produce a command, as opposed to a library?

    Text-only so that `--self-test` can exercise the rule on synthetic sources
    with no directory behind them. `main` overrides it with the real answer
    from the manifest, because a `fn main` inside a doc example would fool
    this and a `[[bin]]` target with a non-default path would be missed by it.
    """
    return re.search(r"\bfn\s+main\s*\(", body) is not None


def reason_to_delete(
    name: str, text: str, *, binary: bool | None = None
) -> str | None:
    """Why `design-decisions.md` 1006 deletes this crate, or None if it does not.

    Two rules, reported apart: see "The second rule" in the module docstring
    for why a crate that states no fact can still have to go.
    """
    if name in PURE_ARGV:
        return None
    body = strip_tests(text)
    has_io = has_io_marker(body)

    # Rule 1 -- it asserts something it could not have looked up. Named
    # exceptions are checked even though they do hold an I/O marker.
    if (not has_io or name in ALSO_FABRICATING) and any(
        p.search(body) for p in FACT_PATTERNS
    ):
        return "states a fact it did not measure"

    # Rule 2 -- it is a command that never looks at anything. A crate holding
    # a real I/O marker is out of scope here even when named above: the
    # exception list exists to catch invented *answers* beside real calls, and
    # a crate with real calls is not inert.
    if has_io:
        return None
    if binary is None:
        binary = builds_binary(body)
    if not binary:
        return None
    return "is a command that never looks at anything"


def fabricates(name: str, text: str) -> bool:
    """Does the crate `name`, with sources `text`, state a fact it never looked up?

    Both halves matter and the second is load-bearing: it must *assert*
    something, and it must contain no call that could have looked. Split out of
    `main` so `--self-test` can exercise it on synthetic sources -- in
    particular the two exemptions added on 2026-09-10, which were found by
    hand and would otherwise be pinned by nothing.

    This is **rule 1 alone**, deliberately. It kept its original meaning when
    rule 2 arrived, so that the fixtures pinning it still assert what they
    were written to assert; `reason_to_delete` is the union of both rules and
    is what `main` uses.
    """
    return reason_to_delete(name, text) == "states a fact it did not measure"


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

    # The multi-file bug: `crate_sources` concatenates every .rs file, so a
    # test module in an early file used to discard all the later ones.
    CONCAT = (
        "mod a {\n#[cfg(test)]\nmod tests { fn t() {} }\n}\n"
        + 'fn later() { std::fs::read("/x"); }\n'
        + FACT
    )
    expect("I/O in a file after the first test module is still seen",
           fabricates("probe", CONCAT), False)
    expect("...and the test module itself is still dropped",
           "#[cfg(test)]" in strip_tests(CONCAT), False)
    expect("...while the code after it survives",
           "std::fs::read" in strip_tests(CONCAT), True)
    expect("...and one that only prints usage is not",
           fabricates("probe", 'fn main() { println!("usage: probe [-v]"); }'), False)

    # --- rule 2: a command that never looks at anything --------------------
    #
    # The case that forced it. No number, no unit, no PASS -- FACT_PATTERNS
    # cannot see this, and it exited 0 having written nothing to the device.
    CLOCK = 'use std::time::SystemTime; '
    # Verbatim shape from the deleted crate, which matters: it prints through
    # `writeln!(out, ...)` and contains no `println!("` anywhere in 251 output
    # calls. FACT_PATTERNS anchors on `println!("`, so rule 1 could not see
    # this line whatever it said -- the blind spot was the output macro, not
    # only the wording. The word "successfully" is even in FACT_PATTERNS.
    LUKS = ('fn main() { let _ = writeln!(out, "LUKS{} formatted '
            'successfully on {}.", header.version, device); }')
    expect("the cryptsetup line is invisible to rule 1 -- wrong macro",
           fabricates("cryptsetup", LUKS), False)
    expect("...and is deleted anyway, as a command that looks at nothing",
           reason_to_delete("cryptsetup", LUKS),
           "is a command that never looks at anything")

    # The two rules stay distinct: a crate that only prints its own usage
    # states no fact at all, and is inert rather than fabricating.
    USAGE = 'fn main() { println!("usage: probe [-v]"); }'
    expect("a usage-only command is inert, not fabricating",
           reason_to_delete("probe", USAGE),
           "is a command that never looks at anything")
    expect("...and rule 1 still says it asserts nothing",
           fabricates("probe", USAGE), False)

    # A library doing no I/O is unremarkable -- charwidth, bignum, ere.
    expect("a library that does no I/O is not a defect",
           reason_to_delete("charwidthish", 'pub fn width(c: char) -> u8 { 1 }'),
           None)
    expect("...and the manifest overrides the text, both ways",
           reason_to_delete("probe", USAGE, binary=False), None)
    expect("...including a [[bin]] whose main is not in the text",
           reason_to_delete("probe", 'pub fn run() { println!("x"); }',
                            binary=True),
           "is a command that never looks at anything")

    # Rule 2 must not fire on a crate that genuinely reads the world, and
    # must not steal a rule-1 hit from one that fabricates beside real I/O.
    expect("a command that does real I/O is not inert",
           reason_to_delete("probe",
                            'fn main() { let _ = std::fs::read("/x"); }'),
           None)
    expect("a named exception is reported under rule 1, not rule 2",
           reason_to_delete("snapper", CLOCK + FACT),
           "states a fact it did not measure")
    expect("argv-only tools are exempt from both rules",
           reason_to_delete(sorted(PURE_ARGV)[0], USAGE), None)

    # `std::net` is mostly types. Importing an address is not opening a
    # socket, and `ipcalc` did nothing else with it.
    IPCALC = ('use std::net::Ipv4Addr;\nfn main() { '
              'println!("Network:   10.0.0.0/8"); }')
    expect("importing an address type is not networking",
           has_io_marker(IPCALC), False)
    expect("...and the socket types still are",
           has_io_marker("let s = TcpStream::connect(a)?;"), True)
    expect("...while ipcalc itself is answered by PURE_ARGV, not deletion",
           reason_to_delete("ipcalc", IPCALC), None)

    # Argv parsing is unsafe and is not I/O; inline assembly is I/O.
    ARGV = ('fn main(argc: i32, argv: *const *const u8) -> i32 { '
            'let a = unsafe { core::slice::from_raw_parts(argv, argc as usize) }; '
            'println!("/dev/sda: trimmed 256.0 MiB"); 0 }')
    expect("walking argv unsafely is not looking at the world",
           has_io_marker(ARGV), False)
    expect("...so a tool that only parses argv and reports is caught",
           reason_to_delete("fstrim", ARGV),
           "states a fact it did not measure")
    SYSCALL = ('fn main() { let r: i64; unsafe { core::arch::asm!("syscall", '
               'inlateout("rax") 39i64 => r); } println!("pid {}", r); }')
    expect("inline assembly reaches the kernel and does exonerate",
           has_io_marker(SYSCALL), True)
    expect("...so arp/traceroute/libservicebus survive the swap",
           reason_to_delete("arp", SYSCALL), None)

    # The marker matcher must not be fooled by a longer identifier that
    # happens to end in a marker -- `cargo-bloat` was exonerated by a
    # fabricated `std::sys::pal::unix::process` in its own output.
    BLOAT = ('fn main() { println!("   0.1%    0.2%   3.1K  '
             'std::sys::pal::unix::process"); }')
    expect("`unix::` in printed text is not evidence of I/O",
           has_io_marker(BLOAT), False)
    expect("...so the crate is caught, by rule 1, for the same line",
           reason_to_delete("cargo-bloat", BLOAT),
           "states a fact it did not measure")
    expect("a real nix:: call is still a marker",
           has_io_marker("let s = nix::sys::stat::stat(p)?;"), True)
    expect("...and io::stdin still matches inside std::io::stdin",
           has_io_marker("let mut b = String::new(); std::io::stdin();"), True)

    # `builds_binary` is text-only on purpose; pin what it can and cannot see.
    expect("builds_binary sees a plain main", builds_binary(USAGE), True)
    expect("...and an extern \"C\" one (9 crates had this shape)",
           builds_binary('pub extern "C" fn main(_a: i32) -> i32 { 0 }'), True)
    expect("...and does not invent one for a library",
           builds_binary("pub fn helper() {}"), False)

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


def _marker_report(userspace: Path) -> int:
    """Which crates does each marker exonerate *by itself*?

    Every marker bug found so far did its damage in exactly one way: a crate
    whose ONLY marker was the bad one. A marker that is always corroborated by
    another cannot hurt anyone even if it is wrong, so this report is the
    audit's own risk surface, smallest-first to read.

    Four were found by running this query by hand on 2026-09-10, which is why
    it is a flag now rather than a memory:

      `nix::`     matched inside `std::os::unix::ffi::OsStrExt` -- and in
                  `cargo-bloat` the match was inside a line the crate INVENTS,
                  so a fabrication exonerated its own author.
      `unsafe {`  is required to walk argv, so every crate that reads its own
                  command line was cleared for doing so. Hid four commands.
      `std::net`  matched `use std::net::Ipv4Addr` -- an address type, not a
                  socket. Hid `ipcalc`, which is pure argv arithmetic.
      (`asm!`)    the converse: inline assembly IS the syscall route for a
                  freestanding binary and was missing, so removing `unsafe {`
                  alone would have deleted three real programs.

    Read a line as: "if this marker is wrong, these crates are wrongly clean."
    """
    sole: dict[str, list[str]] = {}
    corroborated = set()
    for crate in sorted(p for p in userspace.iterdir() if p.is_dir()):
        text = crate_sources(crate)
        if not text:
            continue
        body = strip_tests(text)
        hit = [m for m in IO_MARKERS if _one_marker_re(m).search(body)]
        if len(hit) == 1:
            sole.setdefault(hit[0], []).append(crate.name)
        corroborated.update(hit)

    print("Markers that are the sole exoneration for some crate.")
    print("If the marker is wrong, these crates are wrongly clean.\n")
    for m, names in sorted(sole.items(), key=lambda kv: len(kv[1])):
        print(f"  {m!r:18s} {len(names):3d}  {', '.join(sorted(names))}")
    quiet = [m for m in IO_MARKERS if m not in sole]
    print(f"\nNever a sole exoneration ({len(quiet)}): always corroborated by")
    print("another marker, so a mistake in one of these costs nothing on its")
    print("own. They still want checking if they become the only hit.")
    print("  " + ", ".join(repr(m) for m in quiet))
    unused = [m for m in IO_MARKERS if m not in corroborated]
    if unused:
        print(f"\nMatched no crate at all ({len(unused)}) -- either the tree "
              f"stopped using them or they never worked:")
        print("  " + ", ".join(repr(m) for m in unused))
    sys.stdout.flush()
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="print every hit")
    ap.add_argument("--limit", type=int, default=40, help="cap --list output")
    ap.add_argument("--check", action="store_true",
                    help="verdict against the pinned baseline; 1 if it rose")
    ap.add_argument("--pin", action="store_true",
                    help="rewrite the baseline from the current tree")
    ap.add_argument("--markers", action="store_true",
                    help="per-marker: which crates rest on it alone")
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
    reasons: dict[str, str] = {}
    for crate in sorted(p for p in userspace.iterdir() if p.is_dir()):
        text = crate_sources(crate)
        if not text:
            continue
        total += 1
        # The manifest, not the sources, decides whether this is a command:
        # `builds_binary` reads text so the self-test can use it, and text can
        # be fooled either way (a `fn main` in a doc example, a `[[bin]]`
        # target whose path is not `src/main.rs`).
        manifest = crate / "Cargo.toml"
        try:
            cargo = manifest.read_text(encoding="utf-8", errors="replace")
        except OSError:
            cargo = ""
        binary = (crate / "src" / "main.rs").is_file() or "[[bin]]" in cargo
        why = reason_to_delete(crate.name, text, binary=binary)
        if why is not None:
            reasons[crate.name] = why
    fabricating = sorted(reasons)

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
            print(f"  ERROR {n} {reasons[n]}, and is not pinned",
                  file=sys.stderr)
        for n in stale:
            print(f"  ERROR {n} is pinned but no longer qualifies -- the pin "
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

    if args.markers:
        return _marker_report(userspace)

    rule1 = sum(1 for w in reasons.values() if w.startswith("states"))
    rule2 = len(reasons) - rule1
    print(f"userspace crates with sources : {total}")
    print(f"assert a fact, do no I/O      : {rule1}")
    print(f"commands that look at nothing : {rule2}")
    print(f"total deletable under 1006    : {len(fabricating)}")
    if total:
        print(f"                              : {100 * len(fabricating) / total:.1f}%")
    if args.list:
        for name in fabricating[: args.limit]:
            print(f"  {name} -- {reasons[name]}")
        if len(fabricating) > args.limit:
            print(f"  ... and {len(fabricating) - args.limit} more")
    return 0


if __name__ == "__main__":
    sys.exit(main())
