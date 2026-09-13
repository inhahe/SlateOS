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
#
# `toolchain` IS one of them, and was missing here until 2026-09-13. This list
# came from CLAUDE.md's lane table -- `posix/**, userspace/**, services/**,
# init/**` -- which is a shorthand that does not mention `toolchain/stubs`,
# although the fuller ownership list does. The result: `toolchain/stubs` had
# zero tests and this gate could not see it, while
# `check-pinned-target-build.py`, written a day later, scanned all five. Two
# gates of mine disagreeing about my own lane's scope is how a crate hides.
#
# MUST MATCH `SEARCH_ROOTS` in `scripts/check-pinned-target-build.py`. Each
# self-test asserts its own tuple against this literal, so editing one without
# the other fails immediately rather than silently narrowing coverage.
ROOTS = ("userspace", "services", "init", "posix", "toolchain")

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


def workspace_member_globs():
    """The `members` globs from the root Cargo.toml.

    Read from the file rather than asked of `cargo metadata`, so this stays a
    pure file scan that costs no build lock. The globs are simple enough --
    `userspace/*`, `posix`, `kernel` -- that a prefix match is exact for all of
    them, and a member entry with a `?` or `[...]` in it would be new.
    """
    try:
        with open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8") as fh:
            text = fh.read()
    except OSError:
        return []
    m = re.search(r"^members\s*=\s*\[(.*?)\]", text, re.S | re.M)
    if not m:
        return []
    return re.findall(r'"([^"]+)"', m.group(1))


def in_workspace(crate, globs):
    """Can `cargo test -p <this crate>` reach it?

    Six of the twelve untested crates are `services/*`, which the root
    Cargo.toml does not list -- they build for the SlateOS target on their own.
    `cargo test -p netstack` answers "package ID specification did not match
    any packages", so "add a test" is not the same instruction for them, and a
    baseline that did not say which kind a crate was would send the next reader
    at a wall.
    """
    for g in globs:
        if g == crate:
            return True
        if g.endswith("/*") and crate.startswith(g[:-1]) and "/" not in crate[len(g) - 1:]:
            return True
    return False


# A hand-written reason is anything after ` -- ` in a baseline line's comment.
# `--write-baseline` carries it forward, because a ratchet that erased the
# explanation every time it was regenerated would train people not to write
# one -- and "why can this crate not have a test" is the single most useful
# thing a line here can say.
REASON_SEP = " -- "


def read_baseline():
    """{crate: reason-or-empty} from the project's baseline file."""
    return read_baseline_from(BASELINE)


def read_baseline_from(path):
    """{crate: reason-or-empty} from any baseline file."""
    out = {}
    try:
        with open(path, encoding="utf-8") as fh:
            for ln in fh:
                name, _, comment = ln.partition("#")
                name = name.strip()
                if not name:
                    continue
                reason = ""
                if REASON_SEP in comment:
                    reason = comment.split(REASON_SEP, 1)[1].strip()
                out[name] = reason
    except OSError:
        pass
    return out


def baseline_text(untested, globs, existing):
    """The whole baseline file, as text.

    Pure, and taking `existing` as an argument rather than reading it, because
    the bug this shape prevents is an ordering one: the first version called
    `read_baseline()` INSIDE `open(BASELINE, "w")`, which truncates -- so every
    hand-written reason was silently erased the first time the list tightened.
    It was caught only because the reason being lost was one I had written
    minutes earlier and happened to look at.
    """
    out = []
    out.append("# Crates with no #[test] anywhere. Written by"
               " check-untested-crates.py --write-baseline.")
    out.append("# This list may only SHRINK. A crate that gains its first test"
               " must be removed from it.")
    out.append("#")
    out.append("# `ws` = the root Cargo.toml lists it, so"
               " `cargo test -p <name>` reaches it.")
    out.append("# `separate` = it is not a workspace member; it builds for the"
               " SlateOS target on its own,")
    out.append("#   and `cargo test -p` answers \"did not match any packages\"."
               " Adding a test to one of")
    out.append("#   these is a different job from adding one to a workspace"
               " member.")
    out.append("#")
    out.append("# Anything after \" -- \" in a line's comment is a human"
               " explanation and is preserved")
    out.append("#   when this file is regenerated.")
    for c, lines in sorted(untested):
        kind = "ws" if in_workspace(c, globs) else "separate"
        line = c + "  # " + str(lines) + " lines, " + kind
        reason = existing.get(c, "")
        if reason:
            line += REASON_SEP + reason
        out.append(line)
    return NL.join(out) + NL


def write_baseline(path, untested, globs):
    """Rewrite the baseline, carrying hand-written reasons forward."""
    existing = read_baseline_from(path)
    text = baseline_text(untested, globs, existing)
    with open(path, "w", encoding="utf-8", newline=NL) as fh:
        fh.write(text)


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

    # Workspace membership, because it decides what "add a test" even means.
    globs = workspace_member_globs()
    ck(globs, "the root Cargo.toml named no workspace members")
    ck(in_workspace("userspace/backup", globs),
       "userspace/* is a workspace member glob")
    ck(not in_workspace("services/init", globs),
       "services/* is NOT in the workspace; `cargo test -p init` cannot reach it")
    ck(in_workspace("posix", globs),
       "a bare (non-glob) member entry must match too")
    ck(not in_workspace("userspace/foo/bar", globs),
       "a glob one level deep must not match two levels")

    # A hand-written reason must survive `--write-baseline`. Without this the
    # explanation is erased every time the list tightens, which teaches people
    # not to write one.
    b = read_baseline()
    ck(isinstance(b, dict), "read_baseline must return crate -> reason")
    ck(all(isinstance(v, str) for v in b.values()),
       "every reason must be a string, even when empty")

    # The round trip, against a temporary file rather than the real baseline.
    # A type check would not have caught the bug this replaces: the first
    # version called `read_baseline()` INSIDE `open(BASELINE, "w")`, which
    # truncates, so the reasons were read back from an already-emptied file.
    # Only a genuine write-then-read says otherwise.
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        tmp = os.path.join(td, "baseline.txt")
        seeded = {"userspace/zzq": "cannot be tested because reasons"}
        with open(tmp, "w", encoding="utf-8", newline=NL) as fh:
            fh.write(baseline_text([("userspace/zzq", 10)], globs, seeded))
        ck(read_baseline_from(tmp).get("userspace/zzq")
           == "cannot be tested because reasons",
           "a reason must be readable back after being written")
        # Regenerate over the top, exactly as --write-baseline does.
        write_baseline(tmp, [("userspace/zzq", 10)], globs)
        ck(read_baseline_from(tmp).get("userspace/zzq")
           == "cannot be tested because reasons",
           "a hand-written reason must SURVIVE regeneration")
        # A crate that leaves the list takes its reason with it.
        write_baseline(tmp, [], globs)
        ck(read_baseline_from(tmp) == {},
           "a crate no longer untested must not linger in the baseline")

    # The roots, because getting them wrong is how toolchain/stubs stayed
    # invisible. Must match SEARCH_ROOTS in check-pinned-target-build.py.
    ck(sorted(ROOTS) == ["init", "posix", "services", "toolchain", "userspace"],
       "roots are " + repr(ROOTS) + " -- they must match "
       "check-pinned-target-build.py's SEARCH_ROOTS")

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

    globs = workspace_member_globs()

    if args.write_baseline:
        write_baseline(BASELINE, untested, globs)
        print("wrote " + str(len(untested)) + " crate(s) to " + BASELINE)
        return 0

    baseline = set(read_baseline())
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
        kind = "workspace member" if in_workspace(c, globs) else "built separately"
        print(mark + c + "  (" + format(lines, ",") + " lines, " + kind + ")")

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
