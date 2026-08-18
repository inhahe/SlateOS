#!/usr/bin/env python3
"""Tests for `scripts/reclaim-space.py`.

Why this exists
---------------

`reclaim-space.py` is a *remedy*: the boot test's free-space floor points at it
when it refuses to build.  A remedy that crashes is worse than no remedy,
because it fires only when the volume is already nearly full and the operator
is already blocked -- the least convenient moment to discover it.

Its history says the same thing.  Three separate defects have shipped in it:
it refused every `target/` it was asked to free, it never finished a deletion
it had abandoned, and it measured free space through a directory it had just
renamed out of existence (`FileNotFoundError`, which aborted the run on the
first candidate it managed to rename, so the later steps were unreachable).
All three share a shape: the *safety* logic was fine and the *bookkeeping
around it* was not, so the script reported success or died rather than doing
the one thing it exists to do.

The last of those is why `reclaim_dir` is tested here directly.  It is the only
function that renames, deletes and measures in one breath, and every one of
those three defects lived in it or in its callers.

Run:  python scripts/test-reclaim-space.py
Exit: 0 all passed, 1 one or more failed.
"""

from __future__ import annotations

import importlib.util
import os
import shutil
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))


def load_module():
    """Import reclaim-space.py, whose hyphen makes it un-importable by name."""
    path = os.path.join(HERE, "reclaim-space.py")
    spec = importlib.util.spec_from_file_location("reclaim_space", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load %s" % path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


FAILURES = []


def check(name, cond, detail=""):
    if cond:
        print("  ok    %s" % name)
    else:
        print("  FAIL  %s %s" % (name, detail))
        FAILURES.append(name)


def make_tree(parent, name="target"):
    """A directory with a file in it, so a rename has something to hold open."""
    path = os.path.join(parent, name)
    os.makedirs(os.path.join(path, "debug"), exist_ok=True)
    with open(os.path.join(path, "debug", "artifact.bin"), "wb") as fh:
        fh.write(b"\0" * 4096)
    return path


def test_reclaims_and_returns_a_number(mod, tmp):
    """The regression test for the FileNotFoundError.

    Before the fix this raised rather than returning, because `free_gib` was
    called on `path` *after* `path` had been renamed to `path.reclaim-<pid>`.
    Asserting on the return type is the point: the crash was in the
    measurement, not in the deletion, so a test that only checked the directory
    was gone would have passed while the script still aborted mid-run.
    """
    path = make_tree(tmp)
    freed = mod.reclaim_dir(path, dry_run=False, sizes=False, log=lambda _m: None)
    check("reclaim returns a float", isinstance(freed, float),
          "got %r" % (freed,))
    check("reclaim removes the directory", not os.path.exists(path))
    leftovers = [d for d in os.listdir(tmp) if ".reclaim-" in d]
    check("reclaim leaves no staging directory behind", leftovers == [],
          "found %r" % (leftovers,))


def test_dry_run_puts_it_back(mod, tmp):
    path = make_tree(tmp, "target-dry")
    freed = mod.reclaim_dir(path, dry_run=True, sizes=False, log=lambda _m: None)
    check("dry run returns a float", isinstance(freed, float))
    check("dry run leaves the directory in place", os.path.isdir(path))
    check("dry run keeps the contents",
          os.path.exists(os.path.join(path, "debug", "artifact.bin")))
    leftovers = [d for d in os.listdir(tmp) if ".reclaim-" in d]
    check("dry run leaves no staging directory behind", leftovers == [],
          "found %r" % (leftovers,))


def test_missing_path_is_not_an_error(mod, tmp):
    freed = mod.reclaim_dir(os.path.join(tmp, "nope"), dry_run=False,
                            sizes=False, log=lambda _m: None)
    check("absent candidate returns 0.0", freed == 0.0, "got %r" % (freed,))


def test_in_use_is_skipped_not_forced(mod, tmp):
    """The core safety property: an open handle inside must veto the delete.

    This is the whole reason the script renames instead of consulting
    timestamps, so it is worth asserting rather than trusting.  It is only
    meaningful on Windows, where a directory containing an open file cannot be
    renamed; POSIX permits the rename, so the assertion is skipped there rather
    than being made to pass by weakening it.
    """
    path = make_tree(tmp, "target-busy")
    held = open(os.path.join(path, "debug", "artifact.bin"), "rb")
    try:
        freed = mod.reclaim_dir(path, dry_run=False, sizes=False,
                                log=lambda _m: None)
        if sys.platform == "win32":
            check("in-use directory is skipped", freed == 0.0,
                  "got %r" % (freed,))
            check("in-use directory survives", os.path.isdir(path))
        else:
            print("  skip  in-use directory is skipped (POSIX allows the rename)")
    finally:
        held.close()


def test_veto_names_the_holder(mod, tmp):
    """A veto must say *who*, not just that it was vetoed.

    `known-issues.md` -> `A-RECLAIM-SPACE-CANNOT-FREE-A-LANE'S-OWN-TARGET` went
    unresolved precisely because "SKIP (in use)" stated a conclusion and threw
    away the evidence; by the time anyone read the log the holder had exited.
    So the attribution is a feature with a bug history, and it gets asserted.

    The holder here is *this* process, which is deliberate and is the case that
    caught the first version: the scan skipped its own pid, so a file held by
    the caller -- the one holder a caller can actually do something about --
    was the one holder it could never report.

    Windows-only: elsewhere the rename succeeds and there is no veto to
    attribute.
    """
    if sys.platform != "win32":
        print("  skip  veto names the holder (POSIX allows the rename)")
        return
    path = make_tree(tmp, "target-attributed")
    victim = os.path.join(path, "debug", "artifact.bin")
    held = open(victim, "rb")
    lines = []
    try:
        freed = mod.reclaim_dir(path, dry_run=False, sizes=False,
                                log=lines.append)
        check("attributed veto still frees nothing", freed == 0.0,
              "got %r" % (freed,))
        blob = "\n".join(lines)
        check("veto names the holding pid", ("pid %d" % os.getpid()) in blob,
              blob)
        check("veto names the held file",
              os.path.basename(victim).lower() in blob.lower(), blob)
        # The distinction the whole tool turns on: an empty result must never
        # be dressed up as "nothing holds it".
        check("veto does not claim the directory is free",
              "no holder visible" not in blob, blob)
    finally:
        held.close()


def test_classifies_worktrees_by_branch_not_by_name(mod, tmp):
    """Only `main` and `lane-<x>` belong to somebody; everything else is scratch.

    Classifying by branch rather than by directory name is the whole point: a
    directory called `os-lane-a` that is parked on a detached HEAD is a scratch
    checkout somebody named suggestively, and a directory called `os-scratch`
    sitting on `lane-b` is lane B's working tree.  Getting this backwards costs
    a lane its build output, so it is asserted rather than assumed.
    """
    for branch in ("main", "lane-a", "lane-b", "lane-c"):
        check("%r is owned, not scratch" % branch,
              not mod.is_scratch_worktree(branch))
    # "" is what `git worktree list --porcelain` reports for a detached HEAD,
    # which is what every bisect tree here actually is.
    for branch in ("", None, "lane-ab", "lane-1", "mainline", "wip/lane-a",
                   "bisect", "detached"):
        check("%r is scratch" % branch, mod.is_scratch_worktree(branch))


def test_scratch_targets_are_reclaimable_without_the_lane_flag(mod, tmp):
    """The ordering is what decides who pays, so it is asserted directly.

    Lane B measured (request `b-a-q47-...`, and the Q47 entry) that with the
    integration checkout already clean, the only candidate the defaults could
    offer a lane was its own `target/` -- i.e. every trip of the free-space
    floor cost that lane a full cold rebuild.  A dead bisect checkout was
    sitting right there, behind the same flag as a live lane tree.  These are
    the assertions that that is no longer true.
    """
    trees = [
        ("/repo/os", "main"),
        ("/repo/os-lane-a", "lane-a"),
        ("/repo/os-lane-b", "lane-b"),
        ("/repo/os-bisect-a", ""),
        ("/repo/os-straddle-scratch", ""),
    ]
    order = mod.candidate_order(trees, "/repo/os-lane-a", False)
    paths = [p for p, _w in order]

    check("scratch trees are candidates at the defaults",
          "/repo/os-bisect-a" in paths and "/repo/os-straddle-scratch" in paths,
          repr(paths))
    check("another lane's tree still is not, without the flag",
          "/repo/os-lane-b" not in paths, repr(paths))
    check("scratch comes before the integration checkout",
          paths.index("/repo/os-bisect-a") < paths.index("/repo/os"),
          repr(paths))
    check("the integration checkout comes before our own",
          paths.index("/repo/os") < paths.index("/repo/os-lane-a"),
          repr(paths))
    check("every candidate says which class put it there",
          all(w for _p, w in order), repr(order))

    # With the flag, other lanes join -- but strictly last, after we have
    # already offered up our own.
    order = mod.candidate_order(trees, "/repo/os-lane-a", True)
    paths = [p for p, _w in order]
    check("--allow-lane-targets admits the other lane",
          "/repo/os-lane-b" in paths, repr(paths))
    check("and puts it after our own tree",
          paths.index("/repo/os-lane-a") < paths.index("/repo/os-lane-b"),
          repr(paths))


def test_our_own_tree_is_never_promoted_by_being_detached(mod, tmp):
    """A detached lane worktree must not sweep itself up as scratch.

    If it did, the tree we are running in would be attacked *before* the
    integration checkout -- the one ordering the docstring promises against --
    and it would be labelled "no lane owns it", which is a false statement about
    the tree whose rebuild we are about to pay for.
    """
    trees = [("/repo/os", "main"), ("/repo/os-lane-a", "")]
    order = mod.candidate_order(trees, "/repo/os-lane-a", False)
    paths = [p for p, _w in order]
    check("our own detached tree appears exactly once",
          paths.count("/repo/os-lane-a") == 1, repr(paths))
    check("our own detached tree is still ordered after the integration tree",
          paths.index("/repo/os") < paths.index("/repo/os-lane-a"), repr(paths))
    why = dict(order)["/repo/os-lane-a"]
    check("and is labelled as ours to pay, not as unowned",
          "ours to pay" in why, why)


def main():
    mod = load_module()
    tmp = tempfile.mkdtemp(prefix="reclaim-test-")
    print("reclaim-space.py tests (scratch: %s)" % tmp)
    tests = (
        test_reclaims_and_returns_a_number,
        test_dry_run_puts_it_back,
        test_missing_path_is_not_an_error,
        test_in_use_is_skipped_not_forced,
        test_veto_names_the_holder,
        test_classifies_worktrees_by_branch_not_by_name,
        test_scratch_targets_are_reclaimable_without_the_lane_flag,
        test_our_own_tree_is_never_promoted_by_being_detached,
    )
    try:
        for test in tests:
            # Report a raised exception as a failure of *that* test and keep
            # going, rather than letting it abort the run.  This is not
            # defensiveness for its own sake: the defect this file was written
            # for surfaced as a FileNotFoundError, so an uncaught exception is
            # the single most likely way for these tests to fail, and a runner
            # that dies on the first one hides every later result.
            try:
                test(mod, tmp)
            except Exception as exc:  # noqa: BLE001 - reporting, not handling
                print("  FAIL  %s raised %s: %s"
                      % (test.__name__, type(exc).__name__, exc))
                FAILURES.append(test.__name__)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    if FAILURES:
        print("FAILED: %d of the checks above" % len(FAILURES))
        return 1
    print("PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
