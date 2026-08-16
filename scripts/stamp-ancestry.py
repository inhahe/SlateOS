#!/usr/bin/env python3
r"""Detect tracked build artifacts invalidated by a *merge* rather than by a commit.

Run this after every merge, next to ``scripts/ki_dupes.py``::

    python scripts/stamp-ancestry.py

Exit 0 = clean, 1 = stale stamps, 2 = could not verify (which is a failure, not
a skip -- see "A missing family is an error" below).

---------------------------------------------------------------------------
The defect this exists to catch
---------------------------------------------------------------------------
Some build products are tracked in git alongside a ``.stamp`` recording the
SHA-256 of every input they were built from (``scripts/ctest-fixtures.py``).
That closes *within-tree* drift completely: if you edit the source and forget
to rebuild, the stamp says so.

It does not close drift created by a merge, and it cannot:

    2026-08-16, on `main`:

                     c23cc33c0  merge origin/main into lane-b   11:11
                        /   \
       lane-b 12:22  5531f816c   2069cbd8e  lane-c: rebuild + re-stamp  13:09
       +13 libc symbols     \   /           (built against 11:11's libc.a)
                          b807390ff  main   <- both, and they disagree

``git merge-base --is-ancestor 5531f816c 2069cbd8e`` answers **no**. Lane C's
rebuild was correct on `lane-c` -- the libc it linked *was* what lane C's
``posix/src`` produced, and every gate passed honestly. Lane B's posix change
was correct on `lane-b` -- there were no rebuilt fixtures there to invalidate.
Only the merge is wrong, so **neither author's worktree could have shown it**,
and the stamp is *self-consistent and still wrong*: it faithfully records a
``libc.a`` that the merged tree's ``posix/src`` no longer produces (17 public
symbols short, including all eight ``posix_spawnattr_*`` accessors and
``pthread_kill``). See known-issues.md ->
``B-A-A-MERGE-OF-TWO-CORRECT-COMMITS-LEFT-NINE-FIXTURES-LINKING-A-LIBC-...``.

---------------------------------------------------------------------------
Why this check and not one of the ones we already have
---------------------------------------------------------------------------
Three gates already guard these artifacts, and this passes cleanly through the
reasoning of all three:

  * **mtime** (``create-ext4-rootfs.sh``) -- the script disqualifies itself at
    line 1213: "a fresh checkout stamps every file with one time, leaving no
    ordering to compare." And here nobody rebuilt against a known-stale libc;
    the libc went stale *underneath* a correct rebuild, in another worktree,
    afterwards.
  * **the content stamp** (``ctest-fixtures.py``) -- asks "does the ELF match
    the libc.a it was built against?" The answer is yes. Nothing asks whether
    *that* libc.a matches the source in the tree, and nothing cheaply can: the
    stamp check's whole design goal is to need no toolchain, while answering
    that question means actually building libc.a.
  * **SYSROOT_STALE** -- fires at image-build time, on a gitignored file, in
    whichever lane happens to build an image. The defect is created at *merge*
    time; an image build is a lane-local event that may not happen for hours.

So this check asks a question about **history** instead of about the working
tree, which is the property all three lack. It needs no toolchain, no build
outputs and no timestamps, runs in milliseconds, and gives the same answer in
a fresh clone as on the machine that did the merge -- the exact three
properties mtime lacks.

---------------------------------------------------------------------------
How it decides
---------------------------------------------------------------------------
For each family below, let ``S`` be the commit that last touched any of its
stamps. Any commit reachable from HEAD but **not** from ``S`` that touches the
family's sources is a commit whose effect on the artifact was never recorded.
``git log S..HEAD -- <sources>`` is exactly that set, in one call.

A named commit is then confirmed against the *trees*: if ``S:<src>`` and
``HEAD:<src>`` hash identically, the source was touched and put back (a change
and its revert), the artifact is fine, and we report it as a note rather than
an error. Listing commits gives a useful message; comparing trees gives the
correct verdict. Neither alone does both.

**Two source tiers, because over-sensitivity has a real cost.** A check that
cries wolf gets ``ALLOW_``-flagged into silence, so the root ``Cargo.toml`` --
whose ``[profile.release]`` genuinely does change libc.a, but which far more
often just gains a workspace member from an unrelated lane -- warns instead of
failing. That is a deliberate, known hole and not an oversight: an opt-level
change would slip through as a warning. The alternative was parsing the
manifest to see *which* table moved, which is fragile in a way that fails
silently, and this fails loudly-but-non-fatally.

**A missing family is an error (exit 2), not a skip.** An unstamped family is
one we can make no statement about, and "could not verify" must never render
as "fine" -- that is ``B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`` and the
``python``-vs-``python3`` probe in ``ctest-fixtures.py`` all over again.

**The family list is data, and its members are globs.** A tenth ctest fixture
is covered the day it lands, and a second artifact family costs four lines.
This is deliberate: the sibling defect in the entry that motivated the content
stamps happened precisely because a rule was replicated per directory and one
directory never got a copy.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import dataclass, field


@dataclass(frozen=True)
class Family:
    """One set of tracked artifacts and the sources that determine them."""

    #: Short tag for the log prefix. Every line carries it, so a family whose
    #: output is interleaved with another's is still attributable.
    tag: str
    name: str
    #: Pathspecs for the stamp files. ``:(glob)`` magic so ``*`` does not cross
    #: ``/`` -- plain git pathspec globbing is looser than it looks.
    stamps: tuple[str, ...]
    #: Paths whose change invalidates the stamps. Plain paths, not globs, so
    #: the tree-hash confirmation below can be exact.
    sources: tuple[str, ...]
    #: Sources that *can* matter but usually do not; a hit here warns.
    weak_sources: tuple[str, ...] = field(default=())
    why: str = ""


FAMILIES: tuple[Family, ...] = (
    Family(
        tag="ctest",
        name="services/ctest-* fixtures",
        stamps=(":(glob)services/ctest-*/*.stamp",),
        # libc.a is the `posix` crate's staticlib (toolchain/build-sysroot.ps1
        # copies target/.../libposix.a to sysroot/lib/libc.a), so the source
        # set is the crate *and its path dependencies* -- `tzrules` can change
        # libc.a without `posix/src` being touched at all -- plus the build
        # script itself, which is where the RUSTFLAGS that pick the float ABI
        # live and nowhere else.
        sources=("posix", "tzrules", "toolchain/build-sysroot.ps1"),
        weak_sources=("Cargo.toml",),
        why="the ELFs link toolchain/sysroot/lib/libc.a, built from posix/",
    ),
)


class GitError(RuntimeError):
    pass


def git(*args: str, allow_fail: bool = False) -> str:
    proc = subprocess.run(
        ("git", *args),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.returncode != 0:
        if allow_fail:
            return ""
        raise GitError(f"git {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout


def tracked(pathspecs: tuple[str, ...]) -> list[str]:
    out = git("ls-files", "--", *pathspecs)
    return [line for line in out.splitlines() if line]


def last_touching(rev: str, pathspecs: tuple[str, ...]) -> str:
    out = git("log", "--format=%H", "-1", rev, "--", *pathspecs).strip()
    return out


def commits_between(base: str, rev: str, paths: tuple[str, ...]) -> list[tuple[str, str]]:
    """Commits reachable from *rev* but not *base* that touch *paths*."""
    if not paths:
        return []
    out = git("log", "--format=%H%x1f%s", f"{base}..{rev}", "--", *paths)
    result = []
    for line in out.splitlines():
        if not line:
            continue
        sha, _, subject = line.partition("\x1f")
        result.append((sha, subject))
    return result


def tree_id(rev: str, path: str) -> str | None:
    """Hash of *path* (tree or blob) at *rev*, or None if it is absent there."""
    out = git("rev-parse", "--verify", "--quiet", f"{rev}:{path}", allow_fail=True)
    return out.strip() or None


def changed_paths(base: str, rev: str, paths: tuple[str, ...]) -> list[str]:
    """Subset of *paths* whose content genuinely differs between the two revs.

    A path touched by a commit and then restored to its original content is not
    a change to the artifact's inputs, however many commits are involved.
    """
    out = []
    for path in paths:
        before, after = tree_id(base, path), tree_id(rev, path)
        if before != after:
            out.append(path)
    return out


def check(family: Family, rev: str, verbose: bool) -> int:
    """Return 0 clean, 1 stale, 2 unverifiable."""
    label = f"[stamp:{family.tag}]"

    stamps = tracked(family.stamps)
    if not stamps:
        print(f"{label} ERROR no tracked stamp matches {' '.join(family.stamps)}")
        print(f"{label}       ({family.name}) -- refusing to report 'clean' for a")
        print(f"{label}       family it cannot see. Either the artifacts lost their")
        print(f"{label}       stamps, or this family's pathspec in")
        print(f"{label}       scripts/stamp-ancestry.py is stale.")
        return 2

    base = last_touching(rev, family.stamps)
    if not base:
        print(f"{label} ERROR {len(stamps)} stamp(s) tracked but no commit touches them")
        return 2

    if verbose:
        print(f"{label} {family.name}: {len(stamps)} stamp(s), last written by {base[:9]}")

    # A source path that does not exist is not a source path that never
    # changed -- but `git log -- <typo>` and a tree-hash comparison of two
    # absent trees both report exactly that, cheerfully and in green. A typo
    # in the table above would therefore *widen* the silence this script was
    # written to end. Check the config against the tree before trusting it.
    missing = [p for p in (*family.sources, *family.weak_sources) if tree_id(rev, p) is None]
    if missing:
        print(f"{label} ERROR declared source(s) absent at {rev}: {', '.join(missing)}")
        print(f"{label}       ({family.name}) -- a path that does not exist reads as a")
        print(f"{label}       path that never changed, so this family cannot be checked.")
        print(f"{label}       Fix the table in scripts/stamp-ancestry.py.")
        return 2

    status = 0

    hits = commits_between(base, rev, family.sources)
    if hits:
        moved = changed_paths(base, rev, family.sources)
        if moved:
            print(f"{label} STALE {family.name} -- {family.why}")
            print(f"{label}       stamps last written by {base[:9]}")
            print(f"{label}       {len(hits)} commit(s) since then change {', '.join(moved)}:")
            for sha, subject in hits:
                print(f"{label}         {sha[:9]}  {subject}")
            print(f"{label}       Those are in the tree but not in the artifacts. Neither")
            print(f"{label}       author's branch could show this; only the merge is wrong.")
            print(f"{label}       Confirm and repair:")
            print(f"{label}         powershell -File toolchain/build-sysroot.ps1")
            print(f"{label}         {sys.executable} scripts/ctest-fixtures.py check")
            status = 1
        elif verbose:
            print(
                f"{label} OK    {len(hits)} commit(s) touched the sources since "
                f"{base[:9]}, but the trees are identical (changed and reverted)"
            )

    weak = commits_between(base, rev, family.weak_sources)
    if weak and changed_paths(base, rev, family.weak_sources):
        print(f"{label} WARN  {', '.join(family.weak_sources)} changed since {base[:9]}.")
        print(f"{label}       Usually workspace-member churn, which cannot affect the")
        print(f"{label}       artifacts -- but a [profile.release] change can. Skim:")
        for sha, subject in weak:
            print(f"{label}         {sha[:9]}  {subject}")

    if status == 0 and not hits:
        print(f"{label} OK    {family.name}: no source commit outranks stamp commit {base[:9]}")
    return status


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Detect tracked artifacts invalidated by a merge rather than a commit.",
    )
    parser.add_argument(
        "--rev",
        default="HEAD",
        help="revision to check (default: HEAD)",
    )
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args(argv)

    try:
        git("rev-parse", "--git-dir")
    except GitError:
        print("stamp-ancestry: not inside a git repository.", file=sys.stderr)
        return 2

    worst = 0
    for family in FAMILIES:
        try:
            worst = max(worst, check(family, args.rev, args.verbose))
        except GitError as exc:
            print(f"stamp-ancestry: {exc}", file=sys.stderr)
            worst = max(worst, 2)
    return worst


if __name__ == "__main__":
    sys.exit(main())
