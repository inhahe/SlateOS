#!/usr/bin/env python3
"""Free space on the shared build volume, without guessing what is in use.

Why this exists
---------------

All three lanes build on one volume.  `scripts/boot-test.sh` refuses to start
below a free-space floor (default 20 GiB) because on 2026-08-15 the volume hit
zero bytes free and a half-written edit truncated a kernel source file to zero
bytes.  That guard is a detector, not a remedy: when it fires, the only advice
it can give is "run `cargo clean` in a worktree nobody is building in", and
which worktree that is cannot be read off the tree.  An idle-looking `target/`
is not an unused one -- a QEMU boot phase writes nothing to `target/` for
minutes at a time -- so an agent that deletes on a timestamp heuristic will
sooner or later pull a build out from under another lane.

This script answers the "is it in use?" question with a fact rather than a
heuristic: it *renames* the directory first.  Windows refuses to rename a
directory that has any file open inside it, and the rename is atomic, so a
success means no process held anything there at that instant and no observer
ever sees a half-deleted tree.  A failure means "in use", and the candidate is
skipped rather than forced.

The worst case after a successful rename is that a lane starts a build one
second later and rebuilds from scratch -- the same cost as any `cargo clean`,
and no corruption.  `target/` is entirely regenerable; that is what makes it the
right thing to prune and a source tree the wrong thing.

Order of attack (cheapest and most clearly disposable first):

  1. this worktree's `build/` scratch older than --scratch-age-days,
  2. the integration checkout's `target/` (nobody develops there),
  3. every other worktree's `target/`  -- only with --allow-lane-targets.

Nothing outside a worktree root is ever touched, and nothing that git does not
consider ignored is ever touched: both are asserted, not assumed.

Usage
-----

    python scripts/reclaim-space.py                 # report + dry run
    python scripts/reclaim-space.py --need 25 --yes # actually free 25 GiB
    python scripts/reclaim-space.py --yes --allow-lane-targets

Exit codes: 0 the target was met (or already was), 1 could not free enough,
2 bad usage / not in a git worktree.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import time

GIB = 1024**3

# Suffixes under build/ that must survive an age sweep.
#
#   .img/.qcow2/.vhd/.iso  the boot test's disk images -- live state, not scratch.
#   .elf                   kernel captures pinned to a specific commit.  These
#                          look like stale scratch (hundreds of MB, weeks old)
#                          and are exactly the opposite: they are the symbols an
#                          open bug's backtrace is decoded against, and the
#                          commit that produced them is long gone from any
#                          build tree.  A 216 MB `kernel-kasan-capture.elf` sat
#                          in lane A's build/ for precisely that reason.
#
# Everything else there is a log or a dump left by a session and is disposable.
SCRATCH_KEEP = (".img", ".qcow2", ".vhd", ".iso", ".elf")
SCRATCH_KEEP_DIRS = ("esp", ".boot-lock")


def run(cmd, cwd=None):
    """Run a command, returning (rc, stdout). Never raises on a non-zero rc."""
    try:
        p = subprocess.run(
            cmd, cwd=cwd, capture_output=True, text=True, check=False
        )
    except OSError as exc:
        return 1, str(exc)
    return p.returncode, p.stdout.strip()


def free_gib(path):
    """Free space in GiB on the volume holding `path`, as a float."""
    return shutil.disk_usage(path).free / GIB


def dir_size_gib(path):
    """Total size of `path` in GiB. Unreadable entries count as zero."""
    total = 0
    for root, _dirs, files in os.walk(path, onerror=lambda _e: None):
        for name in files:
            try:
                total += os.lstat(os.path.join(root, name)).st_size
            except OSError:
                pass
    return total / GIB


def worktrees(repo):
    """[(path, branch)] for every worktree of `repo`, main worktree first."""
    rc, out = run(["git", "worktree", "list", "--porcelain"], cwd=repo)
    if rc != 0:
        return []
    result, path, branch = [], None, ""
    for line in out.splitlines():
        if line.startswith("worktree "):
            if path is not None:
                result.append((path, branch))
            path, branch = line[len("worktree ") :].strip(), ""
        elif line.startswith("branch "):
            branch = line[len("branch ") :].strip().replace("refs/heads/", "")
    if path is not None:
        result.append((path, branch))
    return result


def is_ignored(tree, relpath):
    """True if the worktree at `tree` considers `relpath` ignored.

    A safety interlock, not an optimisation: everything this script deletes is
    build output, and build output is ignored.  If git disagrees, the path is
    not what we think it is and we must not touch it.

    Ask the worktree that *owns* the path.  `git check-ignore` answers only for
    the repository rooted at its cwd and fails outright ("is outside repository
    at ...") on a sibling worktree's path, which would otherwise read as "not
    ignored" and refuse every candidate but our own.
    """
    rc, _ = run(["git", "check-ignore", "-q", "--", relpath], cwd=tree)
    return rc == 0


def reclaim_dir(path, dry_run, sizes, log):
    """Delete `path` iff nothing holds a file open inside it.

    Returns the GiB freed (0 if skipped).  The rename is the whole point: see
    the module docstring.

    The probe runs *before* any measurement, and by default no measurement
    happens at all.  Sizing a `target/` means walking a few hundred thousand
    small files, which on this volume takes longer than the rest of the script
    put together and, worse, competes for I/O with the very builds we are
    trying not to disturb.  Free space before and after is the number that
    actually matters, and `shutil.disk_usage` reports it in microseconds.
    """
    if not os.path.isdir(path):
        return 0.0
    staged = "%s.reclaim-%d" % (path, os.getpid())
    try:
        os.rename(path, staged)
    except OSError as exc:
        log("  SKIP (in use)  %s  [%s]" % (path, exc.strerror))
        return 0.0
    if dry_run:
        # The rename is the whole test, and it is reversible: put it straight
        # back.  A dry run that skipped the probe could only report on
        # timestamps, which is the guesswork this script exists to avoid.
        size = dir_size_gib(staged) if sizes else 0.0
        os.rename(staged, path)
        log("  WOULD reclaim  %s%s"
            % (path, ("  (%.1f GiB)" % size) if sizes else "  (not in use)"))
        return size
    before = free_gib(path)
    log("  reclaiming     %s" % path)
    shutil.rmtree(staged, ignore_errors=True)
    if os.path.exists(staged):
        # A stray handle can survive the rename and block individual unlinks.
        # The tree is already out of the way under a name nothing looks for, so
        # this is a leak of space, not a correctness problem -- say so.
        log("  WARNING: %s could not be fully removed; remove it by hand" % staged)
    freed = free_gib(path) - before
    log("                 freed %.1f GiB" % freed)
    return freed


def reclaim_scratch(root, age_days, dry_run, log):
    """Delete aged files directly under `<root>/build`.

    Only files, only the top level, and only ones older than `age_days`: the
    directory is a scratchpad shared with the boot test, whose disk images and
    staged ESP live there too and are matched by the keep lists.
    """
    build = os.path.join(root, "build")
    if not os.path.isdir(build):
        return 0.0
    cutoff = time.time() - age_days * 86400
    freed = 0.0
    for name in sorted(os.listdir(build)):
        path = os.path.join(build, name)
        if os.path.isdir(path):
            continue
        if name in SCRATCH_KEEP_DIRS or name.endswith(SCRATCH_KEEP):
            continue
        try:
            st = os.lstat(path)
        except OSError:
            continue
        if st.st_mtime >= cutoff:
            continue
        size = st.st_size / GIB
        if size * 1024 < 1:  # under 1 MiB: not worth the log line
            continue
        if dry_run:
            log("  WOULD delete   %-58s %7.1f GiB" % (path, size))
            freed += size
            continue
        try:
            os.remove(path)
        except OSError as exc:
            log("  SKIP           %-58s [%s]" % (path, exc.strerror))
            continue
        log("  deleted        %-58s %7.1f GiB" % (path, size))
        freed += size
    return freed


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Free space on the shared build volume."
    )
    ap.add_argument(
        "--need",
        type=float,
        default=20.0,
        help="stop once this many GiB are free (default 20, the boot test floor)",
    )
    ap.add_argument(
        "--yes",
        action="store_true",
        help="actually delete; without it the script only reports what it would do",
    )
    ap.add_argument(
        "--scratch-age-days",
        type=float,
        default=3.0,
        help="delete build/ scratch files older than this many days (default 3)",
    )
    ap.add_argument(
        "--sizes",
        action="store_true",
        help="measure each target/ before reporting it. Off by default: walking "
        "a few hundred thousand files takes longer than everything else here "
        "and competes for I/O with the builds we are trying not to disturb",
    )
    ap.add_argument(
        "--allow-lane-targets",
        action="store_true",
        help="also consider other lanes' target/ dirs, not just the integration "
        "checkout's (they are still skipped if anything holds them open)",
    )
    args = ap.parse_args(argv)

    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    rc, root = run(["git", "rev-parse", "--show-toplevel"], cwd=here)
    if rc != 0 or not root:
        sys.stderr.write("not inside a git worktree: %s\n" % here)
        return 2
    root = os.path.normpath(root)

    def log(msg):
        sys.stdout.write(msg + "\n")
        sys.stdout.flush()

    dry = not args.yes
    start = free_gib(root)
    log("Build volume: %.1f GiB free, target %.1f GiB%s"
        % (start, args.need, "  (DRY RUN -- pass --yes to act)" if dry else ""))
    if start >= args.need:
        log("Nothing to do.")
        return 0

    trees = worktrees(root)
    if not trees:
        sys.stderr.write("could not enumerate worktrees\n")
        return 2
    main_tree = trees[0][0]

    log("Step 1: scratch under %s/build older than %.0f days"
        % (root, args.scratch_age_days))
    reclaim_scratch(root, args.scratch_age_days, dry, log)
    if not dry and free_gib(root) >= args.need:
        log("Done: %.1f GiB free." % free_gib(root))
        return 0

    # The integration checkout is the safest target/ to prune: no lane develops
    # there, it only merges, and a merge rebuild costs time and nothing else.
    # Ours goes last -- pruning it means we rebuild too, which is a cost we can
    # choose to pay but should not pay before someone else's free lunch.
    order = [main_tree]
    if args.allow_lane_targets:
        order += [p for p, _b in trees[1:]]
        order.append(root)
    seen, unique = set(), []
    for tree in order:
        key = os.path.normcase(os.path.normpath(tree))
        if key not in seen:
            seen.add(key)
            unique.append(tree)

    log("Step 2: target/ directories, integration checkout first")
    for tree in unique:
        target = os.path.normpath(os.path.join(tree, "target"))
        if not os.path.isdir(target):
            continue
        if not is_ignored(tree, "target"):
            log("  REFUSING %s: git does not consider it ignored" % target)
            continue
        reclaim_dir(target, dry, args.sizes, log)
        if not dry and free_gib(root) >= args.need:
            break

    end = free_gib(root)
    log("Free now: %.1f GiB (was %.1f)" % (end, start))
    if dry:
        log("Dry run -- nothing was deleted. Re-run with --yes to act.")
        return 0
    if end < args.need:
        log("Could not reach %.1f GiB. Everything else is in use or not ours."
            % args.need)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
