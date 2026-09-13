#!/usr/bin/env python3
"""Refuse a NEW crate that ships with no tests at all.

A crate with no `#[test]` anywhere reports `test result: ok. 0 passed` — which
is not a green suite, it is an empty one, and the two are indistinguishable in
the summary line anybody actually reads. `userspace/backup` sat like that for
its whole life: 1,336 lines of backup and restore, no tests, and a manifest
parser that silently turned a corrupted file size into zero. Writing the first
nine tests found that defect in the first hour.

A RATCHET, NOT A SWEEP. Twelve crates in lane B's trees are in this state
today, totalling 14,301 lines; they are listed in
`scripts/untested-crates-baseline.txt` and are not failures. What fails is a
crate that is not on the list and has no tests — a new one, or a renamed one.

THE BASELINE SHRINKS BOTH WAYS. When a crate on the list gains its first test,
this reports the line as stale and refuses until it is removed, because a
ratchet that does not tighten when the work is done is a standing permission
for that crate to lose its tests again unnoticed.

WHAT THIS DELIBERATELY DOES NOT CLAIM. Counting `#[test]` says nothing about
whether the tests are any good, and one trivial assertion satisfies it. That
is not an argument against the check — the gap between "no tests" and "one
test" is the one that hides a whole crate from every suite — but it is the
reason this is a floor and not a quality measure.

    python scripts/check-untested-crates.py
    python scripts/check-untested-crates.py --write-baseline
    python scripts/check-untested-crates.py --selftest
"""

import argparse
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NL = chr(10)

# Lane B's trees. A crate outside these belongs to another lane, and a gate
# scoped wider than its owner can fix blocks people who cannot act on it.
ROOTS = ("userspace", "services", "init", "posix")

BASELINE = os.path.join(ROOT, "scripts", "untested-crates-baseline.txt")

TEST_ATTR = re.compile(r"#\[\s*test\s*\]")
# `#[tokio::test]`, `#[rstest]` and friends would also be tests; none are used
# in this tree, and matching them blindly would let `#[test_only_helper]` count.
# If one is adopted, add it here rather than loosening the pattern.


def crate_dirs():
    """Every directory holding a Cargo.toml under the scanned roots."""
    out = []
    for r in ROOTS:
        base = os.path.join(ROOT, r)
        if not os.path.isdir(base):
            continue
        for name in sorted(os.listdir(base)):
            d = os.path.join(base, name)
            if os.path.isfile(os.path.join(d, "Cargo.toml")):
                out.append(r + "/" + name)
    return out


def count_tests(crate):
    """(number of #[test] attributes, lines of Rust) in a crate."""
    tests = 0
    lines = 0
    base = os.path.join(ROOT, crate.replace("/", os.sep))
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
        for fn in filenames:
            if not fn.endswith(".rs"):
                continue
            try:
                with open(os.path.join(dirpath, fn), encoding="utf-8",
                          errors="replace") as fh:
                    src = fh.read()
            except OSError:
                continue
            lines += src.count(NL)
            tests += len(TEST_ATTR.findall(src))
    return tests, lines


def read_baseline():
    try:
        with open(BASELINE, encoding="utf-8") as fh:
            return {
                ln.split("#", 1)[0].strip()
                for ln in fh
                if ln.split("#", 1)[0].strip()
            }
    except OSError:
        return set()


def selftest():
    bad = 0
    checks = 0

    def ck(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    ck(bool(TEST_ATTR.search("#[test]")), "the plain attribute must match")
    ck(bool(TEST_ATTR.search("    #[test]" + NL)), "indented must match")
    ck(bool(TEST_ATTR.search("#[ test ]")), "spaced must match")
    # The near-misses. Each of these would make a crate look tested when it is
    # not, which is the one direction this check must never fail in.
    ck(not TEST_ATTR.search("#[cfg(test)]"),
       "#[cfg(test)] is a module guard, not a test -- a crate can have the "
       "module and no tests in it")
    ck(not TEST_ATTR.search("#[test_only_helper]"),
       "an attribute merely starting with 'test' is not a test")
    # A KNOWN LIMIT, asserted so it stays known. A commented-out `#[test]`
    # still matches, so a crate whose only test is commented out reads as
    # tested. That is a different and rarer problem than a crate with none,
    # and stripping comments to catch it would mean parsing Rust. If this
    # assertion ever fails, someone has made the pattern smarter and this
    # comment is the thing to update.
    ck(bool(TEST_ATTR.search("// #[test]")),
       "a commented-out test is expected to still match; the limit has moved")

    # The corpus must be real. A scan that found no crates would report a
    # clean tree however broken it was.
    crates = crate_dirs()
    ck(len(crates) > 50,
       "only " + str(len(crates)) + " crate(s) found -- the scan has lost its "
       "subject")
    ck("userspace/backup" in crates,
       "a known crate is missing from the scan")
    # And a crate known to have tests must read as tested, so a regex that
    # matched nothing could not pass everything.
    n, _ = count_tests("userspace/backup")
    ck(n > 0, "userspace/backup should have tests now, found " + str(n))

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--write-baseline", action="store_true",
                    help="rewrite the baseline from the current tree")
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    crates = crate_dirs()
    if not crates:
        print("check-untested-crates: no crate found under "
              + "/, ".join(ROOTS) + "/ -- nothing to judge.", file=sys.stderr)
        return 2

    untested = []
    for c in crates:
        n, lines = count_tests(c)
        if n == 0:
            untested.append((c, lines))

    if args.write_baseline:
        with open(BASELINE, "w", encoding="utf-8", newline=NL) as fh:
            fh.write("# Crates with no #[test] anywhere. Written by"
                     " check-untested-crates.py --write-baseline." + NL)
            fh.write("# This list may only SHRINK. A crate that gains its"
                     " first test must be removed from it." + NL)
            for c, lines in sorted(untested):
                fh.write(c + "  # " + str(lines) + " lines" + NL)
        print("wrote " + str(len(untested)) + " crate(s) to " + BASELINE)
        return 0

    baseline = read_baseline()
    names = {c for c, _ in untested}
    new = sorted(n for n in names if n not in baseline)
    stale = sorted(b for b in baseline if b not in names)

    total = sum(lines for _, lines in untested)
    print("check-untested-crates: " + str(len(crates)) + " crate(s); "
          + str(len(untested)) + " with no tests (" + format(total, ",")
          + " lines); " + str(len(new)) + " not in the baseline, "
          + str(len(stale)) + " baseline line(s) now stale.")

    for c, lines in sorted(untested):
        mark = "NEW " if c in set(new) else "    "
        print(mark + c + "  (" + format(lines, ",") + " lines)")

    if stale:
        print("", file=sys.stderr)
        print("These crates have tests now and must leave the baseline:",
              file=sys.stderr)
        for b in stale:
            print("  " + b, file=sys.stderr)
        print("Run: python scripts/check-untested-crates.py --write-baseline",
              file=sys.stderr)
        return 1

    if new:
        print("", file=sys.stderr)
        print("A crate above has no `#[test]` anywhere. Its suite reports "
              "`0 passed`, which reads exactly like a passing one.",
              file=sys.stderr)
        print("userspace/backup was in this state for its whole life: 1,336 "
              "lines, and writing its first nine tests found a manifest "
              "parser that turned a corrupted file size into zero.",
              file=sys.stderr)
        print("Add a test. If the crate genuinely cannot have one, say why in "
              "scripts/untested-crates-baseline.txt -- but the list is meant "
              "to shrink, so prefer the test.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
