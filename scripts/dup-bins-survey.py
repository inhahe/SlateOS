#!/usr/bin/env python3
"""Survey the utility names that two crates both build, and say how to decide
between them.

## This script no longer pronounces a winner, and here is the evidence

It used to print a `verdict` column -- "standalone ahead", "coreutils ahead",
"close -- read both". Five of those verdicts have now been put to a real
differential harness, and the column went **nought for five**:

| pair | the verdict | what the harness said |
|---|---|---|
| `tee` | standalone ahead | coreutils 71/0, standalone 34/37 — **inverted** |
| `dd` | standalone ahead | coreutils 339/0, standalone 8/331 — **inverted** |
| `expand` | close — read both | 216/0 against 90/126 — a landslide |
| `comm` | close — read both | 197/0 against 104/93 — a landslide |
| `join` | close — read both | 305/0 against 126/179 — a landslide |

Both failure directions have one cause: **this script counts option names that
appear in the source, and an option that is named is not an option that
works.** The standalone `dd` mentions `bs` and implements none of its suffixes,
so `dd bs=1M` errors. The standalone `join` mentions `-a` and rejects `-a1`,
the ordinary spelling, costing it 64 cases. Counting mentions cannot see that,
and no refinement of the counting can -- the fact is not in the text.

"close" turned out to be the most misleading of the three rather than the most
balanced. A pair is called close when the two line counts are close, and the
smaller side is often smaller *precisely because a third of its options are
missing*. Three "close" pairs were measured and all three were landslides.

A third failure mode, which is about safety rather than ranking: **some
standalone crates are multicall binaries, so their row is not about one
program.** `userspace/stat` dispatches on `argv[0]` to `stat`, `readlink` and
`ln`, so the `stat` row credits the standalone with 14 options it does not have
-- `--backup`, `--symbolic` and `--no-target-directory` are `ln`'s,
`--canonicalize-missing` and `--no-newline` are `readlink`'s. Two consequences
follow, and the second is the dangerous one: the count compares three programs
against one, and `git rm -r userspace/stat` would delete a `readlink` and an
`ln` that nothing in the row mentions. Check what a crate's `argv[0]` dispatch
serves before removing it; `scripts/check-roadmap-done.py` catches the fallout
afterwards, but only for names the roadmap claims are done.

So the column now says what would actually settle it: whether the tree already
has a differential harness for this name, and the command to run. What this
script still measures honestly -- line counts and the mentioned-option sets --
is printed unchanged, because those are observations. The verdict was an
inference, and the inference did not hold.

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

import os
import re
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rustlex  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
COREUTILS_BIN = ROOT / "userspace" / "coreutils" / "src" / "bin"
USERSPACE = ROOT / "userspace"
# Where a `<name>-diff.sh` lives if the tree already has one for a pair.
SCRIPTS = Path(__file__).resolve().parent

# A short option is a dash and one character that is not a digit -- `-1` is far
# more often a numeric operand (`head -1`) or part of a format string than a
# flag, and including digits made the sets noise. A long option needs at least
# two characters after the dashes, which excludes the bare `--` end-of-options
# marker without having to special-case it.
_SHORT = re.compile(r'"(-[A-Za-z])"')
_LONG = re.compile(r'"(--[A-Za-z][-A-Za-z0-9]+)"')
# Byte-literal match arms: `b'c' =>` or `b'c' |`, how several of these parsers
# spell a short option once they are iterating a cluster like `-abc`. The `b`
# prefix is optional because the two sides disagree about it -- `coreutils`
# iterates `arg.as_bytes()` and matches `b'd'`, while several standalone crates
# iterate `arg.chars()` and match `'d'`. Reading only the byte form credited
# `coreutils` with `-c -d -s -C` as options `tr` alone had, when the standalone
# `tr` accepts all four; it just spells them `'d' => delete = true`.
#
# The trailing `=>`/`|` is required, and it is not decoration. A bare `b'X'`
# scan credited `tr` with `-A -F -X -Z` that it does not accept: the literals
# came from `for b in b'A'..=b'Z'` expanding `[:upper:]` and from `b'A'..=b'F'`
# decoding hex. Character-oriented utilities -- `tr`, `expand`, `fold`, `od` --
# are full of byte literals that are data, not flags, and they are exactly the
# ones this survey is used on.
#
# Dropping the `b` widens the same hazard, so the char form is subject to the
# locality rule below (only inside a function that mentions `--`) while the byte
# form is not. A range arm (`'a'..='z' =>`) is excluded by the negative
# lookahead: it is a classifier, never an option.
_BYTE = re.compile(r"b'([A-Za-z])'\s*(?:=>|\|)")
_CHAR = re.compile(r"(?<![A-Za-z0-9_])(?<!\.\.=)'([A-Za-z])'\s*(?:=>|\|)(?!=)")
# Unit tests are cut before scanning, for the same reason: an assertion like
# `assert_eq!(set2, vec![b'A', b'x'])` is not an option table. Cutting from the
# first `#[cfg(test)]` to end-of-file works because that is where every one of
# these files puts its tests; a `#[cfg(test)]` in the middle would cost us the
# code after it, which is a survey being conservative rather than wrong.
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
_TABLE = re.compile(r'\(\s*(?<!b)"([a-z][-a-z0-9]+)"\s*,\s*[A-Z][A-Za-z0-9]*(?:::|\s*\))')
# A third shape, in the bins that dispatch on the stripped name directly rather
# than through a table: `match typed { "regexp-extended" => out.ere = true, …`.
# `sed` is the one that made this necessary -- it was reported as missing
# `--expression`, `--in-place` and `--regexp-extended`, all three of which it
# has had all along, as match arms.
#
# This is the loosest of the three patterns: *any* `match` on a lowercase string
# has this shape, and an unrestricted scan credited `seq` with `--expr`,
# `--index`, `--length`, `--match`, `--substr` and `--yes`, none of which is an
# option of anything. So it is applied per function, and only inside a function
# that also mentions `--` somewhere -- which is where long-option dispatch
# lives, and is not where a `match` on a format specifier or a subcommand name
# lives. That is a locality argument rather than a syntactic one, so it still
# lets some noise through; `--verbose` prints the names precisely so that a
# reader can see an implausible one for what it is.
#
# The `(?<!b)` is the fourth bias this script has had, and the most instructive.
# `tr` matches POSIX character classes as *byte strings* -- `b"alpha" => …`,
# `b"digit"`, `b"xdigit"` -- because it is comparing against bytes taken from
# `[:alpha:]` inside a SET operand. Without the guard this read all twelve class
# names as long options and reported `tr` as "standalone ahead" by 12 to 6, on
# a pair where `coreutils` is the side under a differential harness. A `b`
# prefix is the difference between a string the program *prints or matches
# against user text* and one it compares against its own argv, and only the
# latter can be an option.
_ARM = re.compile(r'(?<!b)"([a-z][-a-z0-9]{2,})"\s*=>')
# Function starts, at any indentation, including `pub fn` and `async fn`. Used
# only to chop the file into regions for the rule above.
_FN = re.compile(r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s", re.MULTILINE)


def _local_options(text: str) -> tuple[set[str], set[str]]:
    """Long and short options from `match` arms, scanned per function.

    Returns `(long, short)`. Both patterns are too loose to run over a whole
    file -- any `match` on a lowercase string has the long shape, and any
    `match` on a character has the short one -- so both are confined to
    functions that mention `--` somewhere, which is where long-option dispatch
    lives and is not where a `match` on a format specifier or a character class
    lives. That is a locality argument rather than a syntactic one, so it still
    lets some noise through; `--verbose` prints the names precisely so a reader
    can see an implausible one for what it is.
    """
    starts = [m.start() for m in _FN.finditer(text)]
    bounds = list(zip([0] + starts, starts + [len(text)]))
    long_: set[str] = set()
    short: set[str] = set()
    for lo, hi in bounds:
        chunk = text[lo:hi]
        if "--" in chunk:
            long_ |= set(_ARM.findall(chunk))
            short |= set(_CHAR.findall(chunk))
    return long_, short


def options(text: str) -> set[str]:
    # THE TESTS ARE CUT BY `rustlex.live_code`, NOT AT THE FIRST `#[cfg(test)]`.
    #
    # This was `text[: _TESTS.search(text).start()]` -- everything before the
    # first line-start `#[cfg(test)]` -- which is only the tests when that
    # attribute's first appearance is the test module. Put one on a helper (a
    # `#[cfg(test)] use`, a test-only struct) and the cut lands at the top of
    # the file, taking the whole program with it.
    #
    # That is not hypothetical here. Measured across `userspace/*/src/main.rs`,
    # six crates lose more than fifty lines to the naive cut, and `stat` --
    # which IS one of the colliding pairs this survey ranks -- showed 262 of its
    # 2179 live lines. The result was a `stat` row reading 2840 standalone lines
    # against 2118 for coreutils, with "only sa = 0": the larger implementation
    # credited with not one option of its own. The line counts are measured
    # elsewhere and were never truncated, so the row contradicted itself in the
    # output for as long as it has existed.
    #
    # The stakes are the verdict. This survey exists to decide WHICH of two
    # implementations is deleted, and an undercount of one side's options is an
    # argument for deleting that side -- exactly the "silently drop working
    # features" outcome the module docstring is written to prevent.
    text, _masked = rustlex.live_code(text)
    opts = set(_SHORT.findall(text)) | set(_LONG.findall(text))
    opts |= {"-" + c for c in _BYTE.findall(text)}
    opts |= {"--" + n for n in _TABLE.findall(text)}
    long_, short = _local_options(text)
    opts |= {"--" + n for n in long_}
    opts |= {"-" + c for c in short}
    return opts


def crate_sources(crate: Path) -> list[Path]:
    src = crate / "src"
    return sorted(src.rglob("*.rs")) if src.is_dir() else []


_MODREF = re.compile(r"\b(?:coreutils|crate)::([a-z][a-z0-9_]*)")


def coreutils_modules(seeds: list[Path]) -> list[Path]:
    """The `coreutils` modules a bin reaches, transitively.

    ## Why this exists: the comparison was unfair by construction

    Until this was added, a row compared **one file** of `coreutils` --
    `src/bin/<name>.rs` -- against the standalone's **entire crate**. But
    `coreutils` deliberately factors shared behaviour into modules and the
    standalone crates do not, so the thing being counted on one side was a leaf
    and on the other a whole program.

    `sha256sum` is the clearest case. Its bin is 210 lines against the
    standalone's 1538, which reads as a rout. The bin is 210 lines *because*
    `--check`, the option table, the three checksum-file formats, the name
    escaping and the exit statuses all live in `digest.rs` -- 1363 lines, a port
    of upstream's `digest.c`, shared with `md5sum`. Counting it, the row is
    1573 against 1538 plus whatever `getopt.rs` contributes.

    **This is the mechanism behind every "standalone ahead" verdict**, including
    the two that were put to a harness and came back inverted. `tee` and `dd`
    were not ranked wrongly by bad luck; they were ranked wrongly by a
    comparison that reads a factored implementation's leaf and a copy-pasted
    one's entirety. The module docstring already said the scrape "cannot see an
    option that is parsed by a shared helper" -- what it did not draw is that
    the blindness is one-sided, and therefore a bias rather than noise.

    ## How

    Follow `coreutils::<mod>` from the bin and `crate::<mod>` from each module
    reached, to a fixed point. A textual scrape, like everything else here: it
    will follow a name inside a comment or a string, and that is the harmless
    direction -- it counts a module the bin might not use, where the previous
    behaviour was to count none of them at all.
    """
    src = USERSPACE / "coreutils" / "src"
    found: dict[Path, None] = {}
    queue = list(seeds)
    while queue:
        path = queue.pop()
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for name in set(_MODREF.findall(text)):
            for cand in (src / f"{name}.rs", src / name / "mod.rs"):
                if cand.is_file() and cand not in found:
                    found[cand] = None
                    queue.append(cand)
    return sorted(found)


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
        # The shared modules the bin reaches, without which this row compares a
        # leaf file against a whole crate. See `coreutils_modules`.
        cu_paths = cu_paths + coreutils_modules(cu_paths)
        cu_opts, cu_lines = read(cu_paths)
        st_opts, st_lines = read(st_paths)
        rows.append((name, cu_lines, st_lines, cu_opts, st_opts))

    print(f"{len(rows)} colliding names\n")
    hdr = (f"{'name':<12} {'coreutils':>9} {'standalone':>10}  "
           f"{'only cu':>7} {'only sa':>7}  how to decide")
    print(hdr)
    print("-" * len(hdr))
    with_harness = 0
    for name, cl, sl, co, so in rows:
        only_cu, only_sa = co - so, so - co
        # The only column here that has ever predicted a differential's answer.
        # A verdict derived from these counts went nought for five against the
        # harnesses -- see the module docstring -- so the counts are printed and
        # left to speak for themselves, and this column says what would settle
        # it instead.
        if (SCRIPTS / f"{name}-diff.sh").is_file():
            how = f"DIFF_PKG={name} bash scripts/{name}-diff.sh"
            with_harness += 1
        else:
            how = "no harness -- write one"
        print(f"{name:<12} {cl:>9} {sl:>10}  "
              f"{len(only_cu):>7} {len(only_sa):>7}  {how}")
        if verbose:
            if only_cu:
                print(f"             only coreutils: {' '.join(sorted(only_cu))}")
            if only_sa:
                print(f"             only standalone: {' '.join(sorted(only_sa))}")

    print(f"\n{with_harness} of {len(rows)} have a harness; "
          f"{len(rows) - with_harness} would need one written.")
    print("\nThe option counts say what each source MENTIONS. Five pairs ranked"
          "\nfrom them have since been measured, and the ranking was wrong every"
          "\ntime -- twice backwards, three times calling a landslide close. Run"
          "\nthe harness; do not delete on a count.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
