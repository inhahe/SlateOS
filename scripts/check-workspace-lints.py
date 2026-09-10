#!/usr/bin/env python3
"""Find crates that are not subject to the project's own lint policy.

## What this looks for

`CLAUDE.md` requires `#![deny(clippy::all, clippy::pedantic)]` in every crate,
plus five defensive lints in non-test code:

    unwrap_used  expect_used  panic  indexing_slicing  arithmetic_side_effects

The workspace declares all of that in `[workspace.lints]`. A crate is only
subject to it if its manifest says

    [lints]
    workspace = true

or its source carries the equivalent inner attribute. **206 of 248 userspace
crates had neither the manifest line, and 128 had neither that nor the
attribute** when this was written on 2026-09-10. For those 128, `clippy::all`
is `warn` rather than `deny`, `pedantic` is off, and all five defensive lints
are off -- so "clippy clean" meant something much weaker than it does for the
other 120, and nothing anywhere said which kind of clean you had got.

## How it was found, which is the part worth repeating

Not by looking for it. Pre-push gate 12 compiles the unix-gated half of a
changed crate by building for linux; it had been running `cargo test` for that,
which needs a cross-linker this host does not have, so it failed the first time
it had a crate in scope and had never compiled anything. Fixing that produced a
sweep of all 57 crates with a unix arm -- all clean -- whose output carried

    warning: missing `[lints]` to inherit `[workspace.lints]`

for crate after crate. The compile was the question; the manifest warning was
the answer to a different and larger one.

## Why a ratchet rather than a fix

Adding `[lints] workspace = true` to one 2,700-line crate (`userspace/crond`)
produced 147 warnings: 68 unwraps, 42 arithmetic side-effects, 33 indexing and
slicing panics. Across 128 crates that is not a change, it is a programme. The
baseline records who is exempt today; `--check` fails when a NEW crate appears
without the lints, so the count can only fall.

## Usage

    python scripts/check-workspace-lints.py                 # report
    python scripts/check-workspace-lints.py --check         # 1 if a new one appeared
    python scripts/check-workspace-lints.py --update-baseline
    python scripts/check-workspace-lints.py --selftest
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path
from typing import NamedTuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gittree  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
BASELINE_REL = "scripts/workspace-lints-baseline.txt"
BASELINE = Path(__file__).resolve().parent / "workspace-lints-baseline.txt"

# Where crates live. `posix` and the workspace-root crates are single crates
# rather than directories of them, so they are named individually.
CRATE_ROOTS = ("userspace", "services", "init")

# Floors on what was INSPECTED, for the reason `check-read-defaults.py` has
# them: the healthy state of this gate is "no new exempt crates", and every
# number in that sentence counts something wrong. A walk that stopped finding
# manifests reports nothing new and passes, while reporting every pinned crate
# as fixed -- a total failure to look, spelled as the best possible news.
# Measured 2026-09-10: 251 manifests under those roots.
MIN_MANIFESTS = 80

_LINTS_TABLE = re.compile(r"^\s*\[lints(?:\.\w+)?\]\s*$", re.M)
_WORKSPACE_TRUE = re.compile(r"^\s*workspace\s*=\s*true\s*$", re.M)
_INNER_ATTR = re.compile(r"#!\[\s*(?:deny|warn)\s*\(\s*[^)]*clippy::all")


class Scan(NamedTuple):
    """Exempt crates, and how many manifests were looked at.

    Reported apart: `exempt` shrinking is the goal, `manifests` shrinking is a
    broken walk wearing the goal's clothes.
    """

    exempt: list[str]
    manifests: int


def _inherits(manifest_text: str) -> bool:
    """Whether this manifest opts into `[workspace.lints]`.

    The table may be `[lints]` or `[lints.clippy]`; what matters is a
    `workspace = true` inside one of them. Checked by finding the table and
    reading to the next table header rather than by a fixed window, because a
    manifest may put a comment between the two lines -- several here do.
    """
    m = _LINTS_TABLE.search(manifest_text)
    if m is None:
        return False
    rest = manifest_text[m.end():]
    nxt = re.search(r"^\s*\[", rest, re.M)
    section = rest[: nxt.start()] if nxt else rest
    return bool(_WORKSPACE_TRUE.search(section))


def survey(tree: gittree.Tree) -> Scan:
    """Every crate under `CRATE_ROOTS` not subject to the workspace lints."""
    exempt: list[str] = []
    manifests = 0
    for top in CRATE_ROOTS:
        for rel in sorted(tree.files_under(top)):
            if not rel.endswith("/Cargo.toml"):
                continue
            text = tree.read_text(rel)
            if text is None:
                continue
            manifests += 1
            if _inherits(text):
                continue
            # The inner attribute is the other way to be covered. It is weaker
            # -- it cannot express the five defensive lints as `[workspace.lints]`
            # does -- but a crate carrying it is not unlinted, and calling it
            # exempt would put 78 crates on this list that do have a policy.
            crate_dir = rel.rsplit("/", 1)[0]
            covered = False
            for src in (f"{crate_dir}/src/main.rs", f"{crate_dir}/src/lib.rs"):
                head = tree.read_text(src)
                if head and _INNER_ATTR.search(head[:4000]):
                    covered = True
                    break
            if not covered:
                exempt.append(crate_dir)
    return Scan(sorted(exempt), manifests)


def read_baseline(tree: gittree.Tree) -> set[str] | None:
    """The pinned set, out of the REVISION rather than the disk.

    `None` means the revision carries no baseline, which is not the same as an
    empty one: absent says this revision has no policy. Judging a commit by a
    file it does not carry is judging it by a rule it never had.
    """
    text = tree.read_text(BASELINE_REL)
    if text is None:
        return None
    out = set()
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


HEADER = """\
# Crates NOT subject to `[workspace.lints]`, and not carrying the inner
# `#![deny(clippy::all, ...)]` attribute either. Generated by
# `scripts/check-workspace-lints.py --update-baseline`.
#
# For a crate on this list, `clippy::all` is `warn` rather than `deny`,
# `pedantic` is off, and all five of CLAUDE.md's defensive lints --
# unwrap_used, expect_used, panic, indexing_slicing, arithmetic_side_effects --
# are off. "clippy clean" therefore means something much weaker for these than
# for the rest of the tree, and nothing else in the build says which kind of
# clean you got.
#
# THIS FILE SHOULD ONLY EVER SHRINK. Remove a line by adding
#
#     [lints]
#     workspace = true
#
# to that crate's Cargo.toml and fixing what it then reports. Expect real work:
# `userspace/crond`, 2,700 lines, produced 147 warnings on the day this list
# was made -- 68 unwraps, 42 arithmetic side-effects, 33 indexing and slicing
# panics. That is why this is a ledger and not a fix.
#
# Do NOT add a line to turn a red --check green: a new crate arriving without
# the lints is exactly what this gate exists to stop.
"""


def _self_test() -> int:
    """Fixtures for `_inherits`, which is the whole judgement here."""
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got  {got!r}\n          want {want!r}")

    expect("plain [lints] workspace = true",
           _inherits("[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n"), True)
    # A comment between the header and the key is real in this tree.
    expect("a comment between the table and the key does not hide it",
           _inherits("[lints]\n# why\nworkspace = true\n"), True)
    expect("[lints.clippy] counts too",
           _inherits("[lints.clippy]\nworkspace = true\n"), True)
    expect("no [lints] table at all",
           _inherits("[package]\nname = \"x\"\n"), False)
    # The key must be INSIDE the table: `workspace = true` under [package] is
    # a different setting entirely and must not be mistaken for this one.
    expect("workspace = true in another table is not this",
           _inherits("[package]\nworkspace = true\n\n[dependencies]\n"), False)
    expect("an empty [lints] table opts into nothing",
           _inherits("[lints]\n\n[dependencies]\nfoo = \"1\"\n"), False)
    expect("a later table ends the search",
           _inherits("[lints]\n\n[dependencies]\nworkspace = true\n"), False)

    expect("the inner attribute is recognised",
           bool(_INNER_ATTR.search("#![deny(clippy::all, clippy::pedantic)]")), True)
    expect("...in warn form too",
           bool(_INNER_ATTR.search("#![warn(clippy::all)]")), True)
    expect("...and an unrelated deny is not it",
           bool(_INNER_ATTR.search("#![deny(missing_docs)]")), False)

    print(f"check-workspace-lints: self-test "
          f"{'FAILED' if failures else 'passed'} ({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="exit 1 if a crate appeared that is not pinned")
    ap.add_argument("--update-baseline", action="store_true", dest="update")
    ap.add_argument("--selftest", "--self-test", dest="self_test",
                    action="store_true", help="run this script's own fixtures")
    ap.add_argument("--head", metavar="REV",
                    help="judge this revision rather than the working tree")
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

    try:
        tree = gittree.open_tree(str(ROOT), args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1: a revision that cannot be opened is not a finding
        # against anyone's code, and `run-checker.sh` reads 1 as one.
        print(f"check-workspace-lints: cannot read {args.head!r}: {exc}",
              file=sys.stderr)
        return 2

    with tree:
        scan = survey(tree)
        pinned = read_baseline(tree)

    if scan.manifests < MIN_MANIFESTS:
        for line in [
            "check-workspace-lints: REFUSING TO ANSWER -- the scan is too thin.",
            f"  Cargo.toml files read  {scan.manifests:>5}  (floor {MIN_MANIFESTS})",
            "",
            "This is not a tree where every crate inherits the lints; it is a",
            "walk that stopped finding manifests. Every pinned crate would",
            "report as fixed, so a total failure to look would arrive spelled",
            "as the best possible news.",
        ]:
            print(line, file=sys.stderr)
        return 2

    if args.update:
        BASELINE.write_text(
            HEADER + "".join(f"{c}\n" for c in scan.exempt),
            encoding="utf-8",
            # newline="" so Python does not translate to CRLF on Windows, which
            # would commit the exact corruption `scripts/check-eol.py` refuses.
            newline="",
        )
        print(f"wrote {BASELINE.relative_to(ROOT)} with {len(scan.exempt)} entries")
        return 0

    if not args.check:
        print(f"inspected {scan.manifests} Cargo.toml file(s) under "
              f"{'/, '.join(CRATE_ROOTS)}/")
        print(f"{len(scan.exempt)} crate(s) not subject to [workspace.lints]:\n")
        for c in scan.exempt:
            print(f"  {c}")
        return 0

    if pinned is None:
        print(f"no baseline at {BASELINE_REL}; run --update-baseline",
              file=sys.stderr)
        return 2

    current = set(scan.exempt)
    new = sorted(current - pinned)
    gone = sorted(pinned - current)

    if new:
        print(f"\n{len(new)} crate(s) NEWLY outside the workspace lint policy:\n",
              file=sys.stderr)
        for c in new:
            print(f"  {c}", file=sys.stderr)
        for line in [
            "",
            "For these, clippy::all is `warn` not `deny`, pedantic is off, and",
            "unwrap_used, expect_used, panic, indexing_slicing and",
            "arithmetic_side_effects are all off. Add to the crate's Cargo.toml:",
            "",
            "    [lints]",
            "    workspace = true",
            "",
            "Do NOT add a line to scripts/workspace-lints-baseline.txt instead;",
            "a new crate arriving unlinted is what this gate exists to stop.",
        ]:
            print(line, file=sys.stderr)
        sys.stdout.flush()
        return 1

    if gone:
        print(f"{len(gone)} crate(s) now inherit the lints -- run "
              f"--update-baseline to drop them:", file=sys.stderr)
        for c in gone:
            print(f"  {c}", file=sys.stderr)
        sys.stdout.flush()
        return 1

    print(f"ok -- {len(current)} pinned crate(s), none new; inspected "
          f"{scan.manifests} manifest(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
