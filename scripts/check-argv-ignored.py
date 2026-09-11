#!/usr/bin/env python3
"""Refuse a coreutils bin that never reads `argv`.

A program that does not look at its command line cannot honour an option. It
does not refuse one either: it runs, prints what it always prints, and exits 0.
That is worse than an unimplemented option and worse than the refusing stub
design decision §1006 forbids, because **a refusal tells the caller it did not
get what it asked for** and this does not.

Both instances in the tree today were found by writing a differential harness,
one program at a time:

    $ date -d @0
    ours   Fri Sep 11 23:45:44 UTC 2026     <- the current time
    GNU    Thu Jan  1 00:00:00 UTC 1970

    $ uptime -s
    ours   up 00:26                          <- the uptime, not the boot time
    GNU    2026-09-11 19:24:28

    $ uptime --nosuchoption ; echo $?
    ours   up 00:26
           0

`date` scores 3 of 121 against GNU and `uptime` cannot score at all. Neither is
a subtle defect -- both files say in their own header that they take no
arguments -- but both shipped, and nothing in the tree noticed until somebody
went looking at that one program. This is the cheap check that notices the third
one.

## Why it is static

The obvious version runs each bin with a bogus option and asserts a non-zero
exit. That means executing 85 system binaries on a live machine, and the list
includes programs that write files, change system state and signal processes. A
sweep that is 95% harmless is not harmless; the risk is not worth the precision,
because the static question answers the same thing. **A bin whose source never
mentions `argv` cannot be reading it.**

## Delegation, which the first version got wrong

`md5sum.rs` and `sha256sum.rs` never mention `argv` either -- they are eleven
lines that hand off to `coreutils::digest::main`, which does all the parsing and
passes 113 of 113 against GNU. Reporting those two would have been two false
positives out of four findings, a 50% rate, and a gate with that rate gets
switched off before it catches anything. So a bin that calls `<module>::main(`
is credited with whatever that module does.

Usage:
    python scripts/check-argv-ignored.py               # check the worktree
    python scripts/check-argv-ignored.py --head HEAD   # check a revision
    python scripts/check-argv-ignored.py --list        # every bin and its verdict
    python scripts/check-argv-ignored.py --update-baseline
    python scripts/check-argv-ignored.py --selftest
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import gitenv  # noqa: E402,F401  (imported for its side effect; see gittree)
import gittree  # noqa: E402

BASELINE = Path(__file__).resolve().parent / "argv-ignored-baseline.txt"
BIN_REL = "userspace/coreutils/src/bin"
SRC_REL = "userspace/coreutils/src"

# Reading the command line, in any of the spellings this tree uses. `args_os` is
# the correct one; `env::args` is the one `scripts/argv-utf8.py` is burning down.
# Both count here -- this gate asks whether argv is READ, not whether it is read
# well.
READS_ARGV = re.compile(r"\bargs_os\s*\(|\benv::args\b|\bstd::env::args\b|\bargv\b")

# `coreutils::digest::main(&SHA256)` -- a bin that hands its whole command line
# to a shared entry point. The module is then asked the same question.
DELEGATES = re.compile(r"\b(?:coreutils::)?([a-z_][a-z0-9_]*)::main\s*\(")

# Below this, the scan found nothing to look at rather than a clean tree.
# Measured 2026-09-11: 85 top-level bins.
FLOOR = 20


def bin_sources(tree: gittree.Tree) -> dict[str, str]:
    """`{bin name: source}` for every top-level coreutils bin.

    A bin is `src/bin/<name>.rs` or `src/bin/<name>/main.rs`. Anything deeper is
    a module of a multi-file bin -- `src/bin/awk/ast.rs` is not a program, and
    counting it was the mistake that made `getopt-ambiguity-check`'s denominator
    read 95 instead of 85.
    """
    out: dict[str, str] = {}
    for rel in tree.files_under(BIN_REL):
        if not rel.endswith(".rs"):
            continue
        depth = rel.count("/") - BIN_REL.count("/")
        if depth == 1:
            name = rel.rsplit("/", 1)[-1][:-3]
        elif depth == 2 and rel.endswith("/main.rs"):
            name = rel.rsplit("/", 2)[-2]
        else:
            continue
        text = tree.read_text(rel)
        if text is not None:
            out[name] = text
    return out


def module_reads_argv(tree: gittree.Tree, module: str, seen: set[str]) -> bool:
    """Whether `coreutils::<module>` reads argv, following further delegation."""
    if module in seen:
        return False
    seen.add(module)
    for cand in (f"{SRC_REL}/{module}.rs", f"{SRC_REL}/{module}/mod.rs"):
        text = tree.read_text(cand)
        if text is None:
            continue
        if READS_ARGV.search(text):
            return True
        for nxt in set(DELEGATES.findall(text)):
            if module_reads_argv(tree, nxt, seen):
                return True
    return False


def offenders(tree: gittree.Tree) -> tuple[list[str], int]:
    """`(bins that never read argv, bins examined)`."""
    found: list[str] = []
    sources = bin_sources(tree)
    for name, text in sorted(sources.items()):
        if READS_ARGV.search(text):
            continue
        if any(module_reads_argv(tree, m, set()) for m in set(DELEGATES.findall(text))):
            continue
        found.append(name)
    return found, len(sources)


def read_baseline() -> set[str] | None:
    try:
        text = BASELINE.read_text(encoding="utf-8")
    except OSError:
        return None
    out = set()
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


HEADER = """# coreutils bins that never read `argv`, and so silently ignore every
# option they are given.
#
# Written by `scripts/check-argv-ignored.py --update-baseline`. A RATCHET: this
# file only ever shrinks. A name here is a program that answers a different
# question from the one it was asked, with exit 0.
#
# Do NOT add a name to turn a red --check green. Every entry is a program to
# finish or to replace, and each one carries the measurement that says so.
"""


def write_baseline(names: list[str]) -> int:
    lines = [HEADER, ""]
    lines.extend(sorted(names))
    BASELINE.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="")
    return len(names)


def self_test() -> int:
    """Proof that the rule holds, and proof that it can refuse.

    Fixtures rather than the tree, because a self-test that reads the tree
    passes for as long as the tree happens to be clean and says nothing about
    the rule.
    """
    cases: list[tuple[str, bool]] = []

    def case(label: str, ok: bool) -> None:
        cases.append((label, ok))

    case("args_os is reading argv", bool(READS_ARGV.search("let a = args_os();")))
    case("env::args is reading argv", bool(READS_ARGV.search("for a in env::args() {}")))
    case("std::env::args too", bool(READS_ARGV.search("std::env::args().skip(1)")))
    case("a bare `argv` mention counts", bool(READS_ARGV.search("// argv[0] is the name")))
    case("a clock read is not", not READS_ARGV.search("SystemTime::now()"))
    case("the word `arguments` in prose is not",
         not READS_ARGV.search("// takes no arguments at all"))

    case("delegation is recognised",
         DELEGATES.findall("coreutils::digest::main(&SHA256)") == ["digest"])
    case("...without the crate prefix too",
         DELEGATES.findall("digest::main(&X)") == ["digest"])
    case("a plain main() is not delegation", DELEGATES.findall("fn main() -> ExitCode {") == [])

    bad = [label for label, ok in cases if not ok]
    for label, ok in cases:
        print(f"{'ok  ' if ok else 'FAIL'}: {label}")
    print(f"\n{len(cases) - len(bad)} passed, {len(bad)} failed")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Refuse a coreutils bin that never reads argv.")
    ap.add_argument("--head", metavar="REV", help="judge this revision, not the worktree")
    ap.add_argument("--list", action="store_true", help="print every bin and its verdict")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--selftest", "--self-test", dest="selftest", action="store_true")
    args = ap.parse_args()

    # Before any scan, so broken logic cannot report a clean tree on its way to
    # being wrong.
    if args.selftest:
        return self_test()

    try:
        maker = (
            gittree.RevTree(args.head, str(ROOT))
            if args.head
            else gittree.WorkTree(str(ROOT))
        )
        with maker as tree:
            found, examined = offenders(tree)
    except (gittree.GitTreeError, OSError) as e:
        print(f"check-argv-ignored: cannot read the tree: {e}", file=sys.stderr)
        return 2

    if examined < FLOOR:
        print(
            f"check-argv-ignored: only {examined} bin(s) found under {BIN_REL}/."
            f" That is below the floor of {FLOOR}; refusing to report a clean"
            f" tree from a scan that found nothing to look at.",
            file=sys.stderr,
        )
        return 2

    if args.list:
        for name in found:
            print(f"{name}: never reads argv")
        print(f"\n{len(found)} of {examined} bin(s) never read argv")
        return 0

    if args.update_baseline:
        n = write_baseline(found)
        print(f"wrote {BASELINE.name} with {n} entr{'y' if n == 1 else 'ies'}")
        return 0

    allowed = read_baseline()
    if allowed is None:
        print(
            f"check-argv-ignored: {BASELINE.name} is missing. Run"
            f" --update-baseline once to record what is already like this.",
            file=sys.stderr,
        )
        return 2

    new = [n for n in found if n not in allowed]
    fixed = sorted(allowed - set(found))
    for f in fixed:
        print(f"fixed: {f} now reads argv -- run --update-baseline to record it")
    if not new:
        print(
            f"ok -- {examined} bin(s) examined, {len(found)} known to ignore argv,"
            f" 0 new ({len(fixed)} improved)"
        )
        return 0

    print(
        f"{len(new)} coreutils bin(s) never read argv, so every option given to"
        f" them is silently ignored:\n",
        file=sys.stderr,
    )
    for name in new:
        print(f"  {BIN_REL}/{name}.rs", file=sys.stderr)
    print(
        "\nA program that does not look at its command line cannot honour an"
        "\noption -- and does not refuse one either. It runs, prints what it"
        "\nalways prints, and exits 0. `date -d @0` answers with today;"
        "\n`uptime -s` answers with the uptime rather than the boot time."
        "\n\nRoute the bin through `coreutils::getopt`, or hand off to a shared"
        "\n`main` that does.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
