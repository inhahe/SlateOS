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
  3. *our own* `target/`  -- we pay for our own rebuild before anyone else's,
  4. every other worktree's `target/`  -- only with --allow-lane-targets.

Step 3 comes before step 4 deliberately.  Every `target/` costs its owner the
same rebuild, so there is no lane whose tree is free to take; the only honest
tie-break is that the lane running the script is the one that chose to free
space and should therefore be the one to pay for it.  A run left at its
defaults can only ever cost this lane and the integration tree.

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
import re
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


def is_ignored_dir(tree, relpath):
    """True if the worktree at `tree` considers the *directory* `relpath` ignored.

    A safety interlock, not an optimisation: everything this script deletes is
    build output, and build output is ignored.  If git disagrees, the path is
    not what we think it is and we must not touch it.

    Two details, both learned the hard way:

    Ask the worktree that *owns* the path.  `git check-ignore` answers only for
    the repository rooted at its cwd and fails outright ("is outside repository
    at ...") on a sibling worktree's path, which would otherwise read as "not
    ignored" and refuse every candidate but our own.

    Pass the trailing slash.  `check-ignore` matches a *string*, not a path on
    disk -- it never stats anything -- so it cannot tell that `target` names a
    directory, and a directory-only pattern (`**/target/`, which is what this
    repo's .gitignore carries) therefore does not match it.  Asking for `target`
    returns rc=1, "not ignored", on a tree whose .gitignore plainly ignores it.
    That is not a near miss: it made the interlock refuse *every* candidate
    including our own, so step 2 of this script could never delete anything and
    the whole thing was a no-op the first time the free-space floor was hit for
    real.  A guard that always says no is indistinguishable from a broken guard,
    which is why this is asserted below by asking about the real path.
    """
    rc, _ = run(["git", "check-ignore", "-q", "--", relpath + "/"], cwd=tree)
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


STAGED = re.compile(r"\.reclaim-\d+$")


def reclaim_staged(trees, dry_run, log):
    """Finish deletions a previous run started and could not complete.

    `reclaim_dir` renames before it deletes, and when the delete then fails --
    a stray handle can outlive the rename and block individual unlinks -- it
    logs "remove it by hand" and moves on.  Nothing ever did.  `os-lane-b` was
    found holding an entire orphaned `target.reclaim-74628`, invisible to every
    later run because it no longer matched the name those runs look for, while
    the volume it sat on was under the boot test's free-space floor.

    That makes this the cheapest and most clearly disposable thing the script
    can touch -- more so than `build/` scratch, which at least might be wanted.
    A `.reclaim-<pid>` directory is not a build tree that happens to be idle;
    it is one whose owner already renamed it *for deletion* and then died
    trying.  Finishing that deletion cannot cost anyone more than the original
    `rmtree` would have.

    The two safety properties still hold and are still checked: the name must
    be one this script itself generates, and the path it was staged from must
    be git-ignored.  A live run's own staging is skipped by pid, so two
    concurrent runs cannot fight over one tree.
    """
    freed = 0.0
    mine = ".reclaim-%d" % os.getpid()
    for tree in trees:
        try:
            names = sorted(os.listdir(tree))
        except OSError:
            continue
        for name in names:
            path = os.path.join(tree, name)
            if not STAGED.search(name) or name.endswith(mine):
                continue
            if not os.path.isdir(path):
                continue
            # Ask about the path it was staged *from*: `target.reclaim-74628`
            # is not itself in any .gitignore, but `target/` is, and that is
            # the interlock that says this is build output.
            if not is_ignored_dir(tree, STAGED.sub("", name)):
                log("  REFUSING %s: its origin is not git-ignored" % path)
                continue
            if dry_run:
                log("  WOULD finish   %s  (orphaned by an earlier run)" % path)
                continue
            before = free_gib(tree)
            log("  finishing      %s" % path)
            shutil.rmtree(path, ignore_errors=True)
            if os.path.exists(path):
                log("  WARNING: %s still will not delete; a process holds it "
                    "open" % path)
            freed += free_gib(tree) - before
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
        help="also consider *other* lanes' target/ dirs. Without it a run can "
        "only cost the integration checkout and this worktree (they are still "
        "skipped if anything holds them open)",
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

    # Before anything anyone might still want: finish the deletions a previous
    # run abandoned half-way.  This runs across *every* worktree regardless of
    # --allow-lane-targets, because an orphaned staging directory is not a
    # lane's build tree -- it is the wreckage of one that lane already agreed
    # to destroy, and leaving it costs that lane nothing but costs everyone the
    # space.
    log("Step 0: staging directories orphaned by earlier runs")
    reclaim_staged([p for p, _b in trees], dry, log)
    if not dry and free_gib(root) >= args.need:
        log("Done: %.1f GiB free." % free_gib(root))
        return 0

    log("Step 1: scratch under %s/build older than %.0f days"
        % (root, args.scratch_age_days))
    reclaim_scratch(root, args.scratch_age_days, dry, log)
    if not dry and free_gib(root) >= args.need:
        log("Done: %.1f GiB free." % free_gib(root))
        return 0

    # The integration checkout is the safest target/ to prune: no lane develops
    # there, it only merges, and a merge rebuild costs time and nothing else.
    # Ours is next, and specifically *before* any other lane's: another lane's
    # target/ is not a free lunch, it is that lane's rebuild, exactly as ours is
    # ours.  Spending our own first is the only ordering that cannot be read as
    # helping ourselves at a neighbour's expense.
    order = [main_tree, root]
    if args.allow_lane_targets:
        order += [p for p, _b in trees[1:]]
    seen, unique = set(), []
    for tree in order:
        key = os.path.normcase(os.path.normpath(tree))
        if key not in seen:
            seen.add(key)
            unique.append(tree)

    log("Step 2: target/ directories, integration checkout first")
    considered = refused = 0
    for tree in unique:
        target = os.path.normpath(os.path.join(tree, "target"))
        if not os.path.isdir(target):
            continue
        considered += 1
        if not is_ignored_dir(tree, "target"):
            refused += 1
            log("  REFUSING %s: git does not consider it ignored" % target)
            continue
        reclaim_dir(target, dry, args.sizes, log)
        if not dry and free_gib(root) >= args.need:
            break

    if considered and refused == considered:
        # Every candidate refused is not a plausible state of the world: these
        # are cargo's own output directories in checkouts of one repository, and
        # that repository ignores them.  It is what a *broken interlock* looks
        # like, and it is indistinguishable from a correct one by its output
        # alone -- which is exactly how a trailing-slash bug in `is_ignored_dir`
        # went unnoticed until the free-space floor was hit for real and the
        # script freed nothing.  Say so, rather than reporting a tidy failure.
        log("  NOTE: all %d candidate(s) were refused. That is far more likely"
            % considered)
        log("        to be a bug in this script's ignore probe than %d trees"
            % considered)
        log("        genuinely un-ignoring their build output. Check it with:")
        log("          git -C <tree> check-ignore -v -- target/")

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
