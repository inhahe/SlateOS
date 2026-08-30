"""List the cargo build trees in this worktree, and delete the unsanctioned ones.

Why this exists
---------------
Cargo serialises concurrent invocations that share a `CARGO_TARGET_DIR`
behind a build lock, so a foreground `cargo clippy` started next to a
background `cargo test --workspace` simply waits -- for the twenty-odd
minutes the workspace build takes. The obvious dodge is to give the second
run a target directory of its own, and over several sessions lane C dodged
it eleven separate times:

    target-cred  target-hl  target-hl2  target-hl3  target-hl4
    target-lint  target-probe  target-probe2  target-systray  target-test

Every one of those is a full debug build of a ~200-crate workspace, and they
are not small -- the ten above held roughly 190 GB between them. On
2026-08-20 they filled a 1.9 TB volume to 1.2 MB free, in the middle of the
whole-workspace gate that `CLAUDE.md` requires before a merge. What that run
actually printed was `ld: final link failed: No space left on device` and
`rustc-LLVM ERROR: IO failure on output stream`, which at least names the
cause; the C: volume filling earlier the same week did *not* -- it produced
"cannot find type `Option` in this scope" in a crate that plainly uses std,
and cost a full debugging session before the page file was identified. See
`known-issues.md` -> TD-A-FULL-PAGE-FILE-MAKES-RUSTC-REPORT-A-SOURCE-ERROR.

So the dodge is not free, and the cost does not land on the run that took
it. It lands on whichever run is unlucky enough to be in flight when the
volume fills, as a build error that does not mention the disk.

The fix is a fixed, per-lane set of target directories, an override that
defaults *into* that set rather than minting a new name (as
`scripts/reintro-list-hit-tests.py` does with `REINTRO_TARGET_DIR`), and
this script to notice when the rule has slipped again.

Usage
-----
Run from a lane worktree root::

    python scripts/prune-build-trees.py            # report only, changes nothing
    python scripts/prune-build-trees.py --prune    # delete the unsanctioned ones
    python scripts/prune-build-trees.py --lane C   # override lane detection

    # also reclaim sanctioned trees nothing has built into for a week
    python scripts/prune-build-trees.py --prune --prune-stale

Exit status: 0 when only sanctioned trees remain (or `--prune` made it so),
1 when unsanctioned trees are present and were only reported.

A **stale** sanctioned tree is reported but does not fail the check, because
being on the sanctioned list is permission to exist rather than a promise to
be useful: a lane that spent a week on a single crate has a warm tree it still
wants. It is worth reporting anyway -- the tree that actually filled the volume
here was not one of the eleven strays but the sanctioned `target`, 121.8 GB
that nothing had built into for five days, sitting in plain sight in a report
that only listed sizes. Staleness is measured from the profile subdirectories,
not from the top level, because cargo rewrites `.rustc_info.json` when it
merely probes the compiler; see `last_build_time`.

Safety
------
A directory is deleted only if all of these hold. Nothing else is touched,
and `--prune` on a tree that does not qualify is a no-op with a warning:

* its name matches ``target*`` and it sits directly in the worktree root;
* it contains a ``CACHEDIR.TAG`` whose first line carries the standard cache
  signature -- cargo writes this into every target directory it creates, so
  it is a positive identification of a build tree rather than a guess from
  the name;
* it holds no file tracked by git;
* it is not in the sanctioned set for this lane.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


def _load_detect_lane():
    """Borrow `detect_lane` from `which-lane.py`.

    Loaded by path rather than imported by name because the file has a hyphen
    in it, which is not a legal module name. Borrowed rather than reimplemented
    because there must be exactly one answer to "which lane am I": a second
    copy of that mapping is a second thing to update when an account is
    renamed, and the failure mode of getting it wrong here is deleting another
    lane's build trees.
    """
    path = Path(__file__).resolve().parent / "which-lane.py"
    spec = importlib.util.spec_from_file_location("which_lane", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.detect_lane


detect_lane = _load_detect_lane()

# The signature line the Cache Directory Tagging Specification requires, and
# which cargo writes verbatim. Matching on it -- rather than on the directory
# name alone -- is what makes deletion safe: a source directory someone named
# `targets/` cannot possibly carry it.
CACHEDIR_SIGNATURE = "Signature: 8a477f597d28d172789f06886806bc55"

# Lane -> the target directories that lane is entitled to keep.
#
# Two per lane is the working set: one for the long background run, one for
# the foreground check that would otherwise queue behind it. A third is a
# request to justify, not a default. Lane A and B's entries record what those
# lanes were actually using when this was written -- this script only ever
# prunes the worktree it is run from, so those lists are documentation of
# their convention, not an imposition on it.
SANCTIONED = {
    "A": {"target", "target-lint"},
    "B": {"target", "target-check", "target-test"},
    "C": {"target", "target-c", "target-clippy-c"},
}


def is_cargo_build_tree(path: Path) -> bool:
    """Whether `path` is a directory cargo created as a target directory."""
    tag = path / "CACHEDIR.TAG"
    try:
        with tag.open("r", encoding="utf-8", errors="replace") as handle:
            return handle.readline().strip() == CACHEDIR_SIGNATURE
    except OSError:
        return False


def holds_tracked_files(root: Path, path: Path) -> bool:
    """Whether git tracks anything inside `path`.

    A belt-and-braces check on top of `CACHEDIR.TAG`: build trees are
    gitignored, so a tracked file inside one means the directory is not what
    it appears to be, and it does not get deleted.
    """
    try:
        listed = subprocess.run(
            ["git", "ls-files", "--", path.name],
            cwd=root,
            capture_output=True,
            text=True,
            errors="replace",
            check=False,
        )
    except OSError:
        # No git to ask means no clearance to delete.
        return True
    return bool(listed.stdout.strip())


def last_build_time(path: Path) -> float | None:
    """When cargo last *built* into `path`, as a Unix timestamp.

    Read from the profile/target subdirectories rather than from the tree as a
    whole, for two separate reasons.

    The cheap one: walking several hundred thousand files to find a single
    maximum costs about as much as `tree_size_bytes`, and the answer is
    already visible one level down.

    The one that matters: the top level of a target directory holds
    `.rustc_info.json`, which cargo rewrites whenever it merely *probes* the
    compiler. A `cargo metadata` or a `cargo fmt` run in a directory that
    forgot to set `CARGO_TARGET_DIR` touches that file without building
    anything. Reading it would report a tree as fresh that has not been built
    into for days -- which is exactly what the stale 121.8 GB `target` here did
    on 2026-08-20, five days after its last build, and is the whole reason this
    function exists.

    Returns None when the directory cannot be listed or holds no subdirectory
    at all, which callers must not read as "old".
    """
    newest: float | None = None
    try:
        children = list(path.iterdir())
    except OSError:
        return None
    for child in children:
        try:
            if not child.is_dir():
                continue
            mtime = child.stat().st_mtime
        except OSError:
            # Vanished or unreadable. Skipping it can only make the tree look
            # older, which errs towards reporting rather than towards silence.
            continue
        if newest is None or mtime > newest:
            newest = mtime
    return newest


def age_days(mtime: float | None, now: float) -> float | None:
    if mtime is None:
        return None
    return max(0.0, (now - mtime) / 86400.0)


def age_label(age: float | None) -> str:
    if age is None:
        return "  never built"
    if age < 1.0:
        return "     today"
    return f"{age:6.1f} d ago"


def tree_size_bytes(path: Path) -> int:
    """Total size of `path`, ignoring anything that cannot be read.

    Walking a target tree is slow -- hundreds of thousands of small files --
    but the number is the whole point of the report: "eleven stray
    directories" does not motivate anyone, and "190 GB" does.
    """
    total = 0
    for dirpath, _dirnames, filenames in os.walk(path, onerror=lambda _e: None):
        for name in filenames:
            try:
                total += (Path(dirpath) / name).stat().st_size
            except OSError:
                # Vanished mid-walk, or a link we cannot follow. Undercounting
                # a directory we are about to delete is harmless.
                continue
    return total


def human(size: int) -> str:
    value = float(size)
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if value < 1024.0 or unit == "TB":
            return f"{value:.1f} {unit}" if unit != "B" else f"{int(value)} B"
        value /= 1024.0
    return f"{value:.1f} TB"


def worktree_root() -> Path | None:
    """The root of the worktree the cwd is in, or None if there is not one."""
    try:
        found = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            errors="replace",
            check=False,
        )
    except OSError:
        return None
    if found.returncode != 0:
        return None
    return Path(found.stdout.strip())


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Report, and optionally delete, unsanctioned cargo build trees."
    )
    parser.add_argument(
        "--prune",
        action="store_true",
        help="actually delete the unsanctioned trees (default: report only)",
    )
    parser.add_argument(
        "--lane",
        choices=sorted(SANCTIONED),
        help="override lane detection (default: from CLAUDE_CONFIG_DIR)",
    )
    parser.add_argument(
        "--stale-days",
        type=float,
        default=7.0,
        metavar="N",
        help="warn about a sanctioned tree not built into for N days (default: 7)",
    )
    parser.add_argument(
        "--prune-stale",
        action="store_true",
        help=(
            "with --prune, also delete sanctioned trees that are stale. Off by "
            "default: being on the list is permission to exist, and a lane that "
            "spent a week on one crate would lose a warm tree it still wants."
        ),
    )
    args = parser.parse_args()

    root = worktree_root()
    if root is None:
        print("not inside a git worktree -- run this from a lane worktree root", file=sys.stderr)
        return 2

    lane = args.lane or detect_lane()[0]
    if lane is None:
        print(
            "lane: UNKNOWN -- pass --lane A|B|C. Guessing here would mean\n"
            "deleting another lane's build trees, so this script will not.",
            file=sys.stderr,
        )
        return 2

    keep = SANCTIONED[lane]
    print(f"worktree:   {root}")
    print(f"lane:       {lane}")
    print(f"sanctioned: {', '.join(sorted(keep))}")
    print()

    candidates = sorted(
        p for p in root.glob("target*") if p.is_dir() and is_cargo_build_tree(p)
    )
    if not candidates:
        print("no cargo build trees found")
        return 0

    now = time.time()
    stray: list[tuple[Path, int]] = []
    stale: list[tuple[Path, int, float]] = []
    kept_total = 0
    for path in candidates:
        size = tree_size_bytes(path)
        age = age_days(last_build_time(path), now)
        stamp = age_label(age)
        if path.name in keep:
            kept_total += size
            if age is not None and age >= args.stale_days:
                stale.append((path, size, age))
                print(f"  STALE   {path.name:<20} {human(size):>10}  {stamp}")
            else:
                print(f"  keep    {path.name:<20} {human(size):>10}  {stamp}")
        elif holds_tracked_files(root, path):
            print(
                f"  SKIP    {path.name:<20} {human(size):>10}  {stamp}"
                "  (holds tracked files)"
            )
        else:
            stray.append((path, size))
            print(f"  stray   {path.name:<20} {human(size):>10}  {stamp}")

    print()
    print(f"sanctioned trees: {human(kept_total)}")

    if stale:
        stale_total = sum(size for _p, size, _a in stale)
        print(
            f"stale trees:      {human(stale_total)} in {len(stale)} directories "
            f"not built into for {args.stale_days:g}+ days"
        )
        if args.prune and args.prune_stale:
            # Fall through: they are appended to `stray` below and deleted with
            # the rest.
            stray.extend((path, size) for path, size, _a in stale)
        else:
            print(
                "  a sanctioned tree is allowed to exist, not guaranteed to be\n"
                "  wanted -- this one is on the list but nothing has built into\n"
                "  it. Re-run with --prune --prune-stale to reclaim it, or leave\n"
                "  it if you are about to build there again."
            )

    if not stray:
        print("no stray build trees")
        return 0

    stray_total = sum(size for _p, size in stray)
    print(f"stray trees:      {human(stray_total)} in {len(stray)} directories")

    if not args.prune:
        print()
        print("re-run with --prune to delete them")
        return 1

    print()
    failed = False
    for path, size in stray:
        print(f"removing {path.name} ({human(size)}) ...", flush=True)
        errors: list[str] = []
        # `onerror` is deprecated from 3.12 and `onexc` does not exist before
        # it, so pick by what the running interpreter actually has rather than
        # by version number.
        if sys.version_info >= (3, 12):
            shutil.rmtree(
                path,
                onexc=lambda _f, p, exc, acc=errors: acc.append(f"{p}: {exc}"),
            )
        else:
            shutil.rmtree(
                path,
                onerror=lambda _f, p, info, acc=errors: acc.append(f"{p}: {info[1]}"),
            )
        if path.exists():
            # Report the first few rather than all of them: a locked target
            # tree fails on every file it contains, and a thousand identical
            # permission errors bury the one line that says which cargo run
            # still has it open.
            failed = True
            print(f"  FAILED -- {len(errors)} error(s); first:", file=sys.stderr)
            for line in errors[:3]:
                print(f"    {line}", file=sys.stderr)
            print(
                "  a build tree that will not delete is usually still open:\n"
                "  check for a cargo or rustc process using it before retrying.",
                file=sys.stderr,
            )
        else:
            print(f"  removed {path.name}")

    if failed:
        return 1
    print(f"\nreclaimed {human(stray_total)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
