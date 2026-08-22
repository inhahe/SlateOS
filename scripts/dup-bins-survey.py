#!/usr/bin/env python3
"""Survey the utility names that two crates both build, and say which side is
ahead.

## Why this exists

Forty-two binary names in this workspace are produced by *two* packages --
`userspace/coreutils` and a standalone `userspace/<name>` crate -- which cargo
resolves by letting whichever built last overwrite the other. Design decision
§359 settles that `coreutils` is the one home, but that the *code* which
survives is chosen per utility, because for about half of them the standalone
crate is the substantially larger implementation and `coreutils`'s namesake is
a stub. Deleting on the basis of "which crate wins" rather than "which program
is better" would silently drop working features; three names (`sha1sum`,
`sha512sum`, `w`) exist *only* in the standalone crates and would vanish
entirely.

So before anything is deleted, each pair needs a verdict. This produces the
raw material for that verdict, and -- once the port is done -- rechecks it.

## What it measures, and what that is worth

For each colliding name it reports the line count of each side and the set of
**command-line options each side's source mentions**, scraped as string
literals (`"-c"`, `"--format"`) and as `match` arms over bytes (`b'c'`). The
option sets are the interesting half: a utility that accepts `-c`, `-f`, `-t`
and `-L` where the other accepts none of them is not merely longer, it does
more.

This is a *heuristic*, deliberately. It cannot see an option that is parsed by
a shared helper, and it will happily report an option named in a `--help`
string that the parser rejects. It exists to rank forty-two pairs quickly so
that attention goes to the ones that differ, not to pronounce on any single
one -- every actual port is decided by reading both files. A pair this script
calls identical still gets read before either copy is removed.

Usage:
    python scripts/dup-bins-survey.py            # the table
    python scripts/dup-bins-survey.py --verbose  # plus the per-name option sets
    python scripts/dup-bins-survey.py stat tar   # only these names
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
COREUTILS_BIN = ROOT / "userspace" / "coreutils" / "src" / "bin"
USERSPACE = ROOT / "userspace"

# A short option is a dash and one character that is not a digit -- `-1` is far
# more often a numeric operand (`head -1`) or part of a format string than a
# flag, and including digits made the sets noise. A long option needs at least
# two characters after the dashes, which excludes the bare `--` end-of-options
# marker without having to special-case it.
_SHORT = re.compile(r'"(-[A-Za-z])"')
_LONG = re.compile(r'"(--[A-Za-z][-A-Za-z0-9]+)"')
# Byte-literal match arms: `b'c' =>` or `b'c' |`, how several of these parsers
# spell a short option once they are iterating a cluster like `-abc`.
#
# The trailing `=>`/`|` is required, and it is not decoration. A bare `b'X'`
# scan credited `tr` with `-A -F -X -Z` that it does not accept: the literals
# came from `for b in b'A'..=b'Z'` expanding `[:upper:]` and from `b'A'..=b'F'`
# decoding hex. Character-oriented utilities -- `tr`, `expand`, `fold`, `od` --
# are full of byte literals that are data, not flags, and they are exactly the
# ones this survey is used on.
_BYTE = re.compile(r"b'([A-Za-z])'\s*(?:=>|\|)")
# Unit tests are cut before scanning, for the same reason: an assertion like
# `assert_eq!(set2, vec![b'A', b'x'])` is not an option table. Cutting from the
# first `#[cfg(test)]` to end-of-file works because that is where every one of
# these files puts its tests; a `#[cfg(test)]` in the middle would cost us the
# code after it, which is a survey being conservative rather than wrong.
_TESTS = re.compile(r"^#\[cfg\(test\)\]", re.MULTILINE)
# `coreutils`'s newer parsers keep a table of long options with the `--` already
# stripped, because that is the form the resolver compares against after
# handling `--name=value` and GNU's unambiguous-abbreviation rule. Two shapes
# are in use, one carrying an arity and one carrying an enum tag:
#
#     const LONG_OPTIONS: &[(&str, Takes)] = &[("header-numbering", Takes::Required), …
#     const LONG_OPTIONS: &[(&str, Long)]  = &[("equal-width", Long::EqualWidth), …
#
# so the pattern is "a kebab-case literal, a comma, and a capitalised path" and
# not either specific type. The first version of this script looked only for
# `"--name"` literals, which these files do not contain -- so it credited every
# long option to the standalone side and called `nl`, `split`, `seq`, `comm`
# and `tr` "standalone ahead" when `coreutils` in fact implements a superset
# and is the side under a differential harness. Believing that would have
# deleted the tested implementation. It is the same mistake, in miniature, that
# the whole duplication caused with `bc`.
_TABLE = re.compile(r'\(\s*"([a-z][-a-z0-9]+)"\s*,\s*[A-Z][A-Za-z0-9]*(?:::|\s*\))')


def options(text: str) -> set[str]:
    if (m := _TESTS.search(text)) is not None:
        text = text[: m.start()]
    opts = set(_SHORT.findall(text)) | set(_LONG.findall(text))
    opts |= {"-" + c for c in _BYTE.findall(text)}
    opts |= {"--" + n for n in _TABLE.findall(text)}
    return opts


def crate_sources(crate: Path) -> list[Path]:
    src = crate / "src"
    return sorted(src.rglob("*.rs")) if src.is_dir() else []


def read(paths: list[Path]) -> tuple[set[str], int]:
    """The option set and the line count for one side of a pair.

    Options are scanned per file rather than over the concatenation, because
    the test cut is anchored to the *first* `#[cfg(test)]` and a concatenation
    would throw away every file after the first one that has tests. Line counts
    are taken from the whole text: a test module is still code that has to be
    ported or discarded.
    """
    texts = [p.read_text(encoding="utf-8", errors="replace") for p in paths]
    opts: set[str] = set()
    for t in texts:
        opts |= options(t)
    return opts, sum(t.count("\n") for t in texts)


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    verbose = "--verbose" in sys.argv[1:]

    if not COREUTILS_BIN.is_dir():
        print(f"no such directory: {COREUTILS_BIN}", file=sys.stderr)
        return 2

    cu_names = {p.stem for p in COREUTILS_BIN.glob("*.rs")}
    # `coreutils` also has multi-file bins (`src/bin/awk/main.rs`), which the
    # glob above misses; they collide just the same.
    cu_names |= {p.name for p in COREUTILS_BIN.iterdir() if p.is_dir()}

    names = sorted(
        n for n in cu_names
        if (USERSPACE / n / "Cargo.toml").is_file()
    )
    if args:
        names = [n for n in names if n in args]

    rows = []
    for name in names:
        cu_paths = crate_sources(USERSPACE / "coreutils")
        cu_paths = [
            p for p in cu_paths
            if p.stem == name and p.parent == COREUTILS_BIN
            or p.parent.name == name and p.parent.parent == COREUTILS_BIN
        ]
        st_paths = crate_sources(USERSPACE / name)
        if not cu_paths or not st_paths:
            continue
        cu_opts, cu_lines = read(cu_paths)
        st_opts, st_lines = read(st_paths)
        rows.append((name, cu_lines, st_lines, cu_opts, st_opts))

    print(f"{len(rows)} colliding names\n")
    hdr = f"{'name':<12} {'coreutils':>9} {'standalone':>10}  {'only cu':>7} {'only sa':>7}  verdict"
    print(hdr)
    print("-" * len(hdr))
    ahead_sa = ahead_cu = 0
    for name, cl, sl, co, so in rows:
        only_cu, only_sa = co - so, so - co
        # The verdict weighs options first and length second: a longer file that
        # accepts strictly fewer options is longer for some other reason.
        if len(only_sa) > len(only_cu) + 2:
            verdict = "standalone ahead"
            ahead_sa += 1
        elif len(only_cu) > len(only_sa) + 2:
            verdict = "coreutils ahead"
            ahead_cu += 1
        elif sl > cl * 2:
            verdict = "standalone ahead (size)"
            ahead_sa += 1
        elif cl > sl * 2:
            verdict = "coreutils ahead (size)"
            ahead_cu += 1
        else:
            verdict = "close -- read both"
        print(f"{name:<12} {cl:>9} {sl:>10}  {len(only_cu):>7} {len(only_sa):>7}  {verdict}")
        if verbose:
            if only_cu:
                print(f"             only coreutils: {' '.join(sorted(only_cu))}")
            if only_sa:
                print(f"             only standalone: {' '.join(sorted(only_sa))}")

    print(f"\ncoreutils ahead: {ahead_cu}   standalone ahead: {ahead_sa}   "
          f"close: {len(rows) - ahead_cu - ahead_sa}")
    print("\nEvery pair is read before either copy is deleted; this only ranks them.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
