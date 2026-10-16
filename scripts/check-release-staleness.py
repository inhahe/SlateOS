#!/usr/bin/env python3
"""Gate: refuse to push when the release-profile boot test is stale.

The operator (Q46 → §914) chose option C: debug by default, with a periodic
release boot test.  The standing objection to C was that "periodic" degrades
to "never" when the trigger is a human's intention to remember.  This gate
replaces human memory with a commit count: it measures kernel-touching commits
since the last release boot and fails the pre-push past a threshold.

The mechanism is deliberate: it is attached to an *event the tree already
produces* (a commit count past a threshold), not to a calendar date, so a
quiet month costs nothing and a busy week triggers it.  The only way past it
is to run the measurement or to consciously raise the threshold — and raising
it is a visible diff.

Usage:
    python scripts/check-release-staleness.py [--self-test] [--quiet]

Exit codes:
    0  the release boot is fresh enough, or self-tests passed
    1  the release boot is stale — run `./scripts/boot-test.sh --profile=release`
    2  internal error (missing file, bad JSON, git failure)

Machine-read constants and their locations:
    BASELINE_PATH  bench/last-release-boot.json   the SHA of the last release boot
    THRESHOLD      100                             max kernel commits before staleness
    PATHS          kernel/ bench/                  what counts as "kernel-touching"
    RECORDS        bench/boot-history.jsonl etc.   ...except these, which record runs
"""
from __future__ import annotations

import json
import os
import subprocess
import sys

# `gitenv.clean_env()` is load-bearing in the self-test's scratch repository,
# not hygiene: `pre-push` runs `--selftest`, git exports GIT_DIR into a hook's
# environment, and GIT_DIR outranks `cwd`. On 2026-09-26 the first version of
# that self-test, without it, wrote seven commits of fixture files -- `{}` over
# bench/boot-history.jsonl and last-release-boot.json among them -- onto the
# branch being pushed. The push had already resolved its ref, so nothing was
# published; see scripts/gitenv.py for the 2026-08-29 accident that was.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gitenv  # noqa: E402

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

BASELINE_PATH = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "bench", "last-release-boot.json",
)

# Subsystem paths whose commits count toward staleness.  A commit touching
# only docs/ or scripts/ does not make the release binary stale.
PATHS = ("kernel/", "bench/")

# Files under PATHS that record runs rather than change what is run.  Every
# boot test commits its row to boot-history.jsonl as a commit of its own, so
# counting them made the gate count boots: on 2026-09-26, 29 of the 101
# "kernel-touching" commits since the last release boot were boot records, the
# 101st -- the one that tripped the gate on a publish -- was the row recording
# the boot being published, and only 72 touched kernel/.  The baseline file is
# a record too: recording a release boot must not count toward the next one.
RECORDS = (
    "bench/boot-history.jsonl",
    "bench/history.jsonl",
    "bench/last-release-boot.json",
)


def pathspecs():
    """The pathspec `git rev-list` counts: PATHS, minus RECORDS."""
    return list(PATHS) + [":(exclude)%s" % record for record in RECORDS]

# The gate fires when kernel-touching commits since the baseline exceed this.
# 100 is roughly "a day or two of active work" at this project's measured
# commit rate (~100 commits/calendar-day).  High enough that it does not fire
# every push, low enough that it fires before a week passes unnoticed.
THRESHOLD = 100

# ---------------------------------------------------------------------------
# Implementation
# ---------------------------------------------------------------------------


def _repo_root():
    """Return the git toplevel, or None."""
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--show-toplevel"],
            stderr=subprocess.DEVNULL,
            text=True,
        )
        return out.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None


def _commit_count_since(sha, paths, cwd=None):
    """Count commits touching *paths* (pathspecs) since *sha* (exclusive).

    With no *cwd*, in the ambient repository -- under `pre-push` that is the
    one named by the hook's GIT_DIR, which is the one to check.  With a *cwd*
    (the self-test's scratch repository), with the environment's repository
    bindings removed, so that *cwd* is the repository actually counted.
    """
    cmd = ["git", "rev-list", "--count", "%s..HEAD" % sha, "--"]
    cmd.extend(paths)
    env = gitenv.clean_env() if cwd is not None else None
    out = subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL, cwd=cwd, env=env)
    return int(out.strip())


def _sha_exists(sha):
    """Return True if *sha* names a reachable object."""
    try:
        subprocess.check_output(
            ["git", "cat-file", "-t", sha],
            stderr=subprocess.DEVNULL,
        )
        return True
    except subprocess.CalledProcessError:
        return False


def load_baseline(path=BASELINE_PATH):
    """Return the baseline dict, or (None, reason)."""
    if not os.path.isfile(path):
        return None, "baseline file does not exist: %s" % path
    try:
        with open(path, "r", encoding="utf-8", newline="") as fh:
            data = json.load(fh)
    except (json.JSONDecodeError, OSError) as exc:
        return None, "cannot read baseline: %s" % exc
    sha = data.get("sha")
    if not sha or not isinstance(sha, str):
        return None, "baseline has no 'sha' field"
    return data, None


def check(quiet=False):
    """Run the staleness check.  Returns exit code."""
    data, err = load_baseline()
    if err:
        if not quiet:
            print("[release-staleness] %s" % err, file=sys.stderr)
        return 2

    sha = data["sha"]
    if not _sha_exists(sha):
        if not quiet:
            print(
                "[release-staleness] baseline SHA %s is not reachable "
                "(shallow clone or force-pushed history?)" % sha,
                file=sys.stderr,
            )
        return 2

    try:
        count = _commit_count_since(sha, pathspecs())
    except (subprocess.CalledProcessError, ValueError) as exc:
        if not quiet:
            print(
                "[release-staleness] git rev-list failed: %s" % exc,
                file=sys.stderr,
            )
        return 2

    date = data.get("date", "unknown")

    if count > THRESHOLD:
        print(
            "[release-staleness] STALE: %d kernel-touching commit(s) since "
            "last release boot (threshold %d, baseline %s from %s)"
            % (count, THRESHOLD, sha[:9], date),
        )
        print("")
        print(
            "  Run:  ./scripts/boot-test.sh --profile=release",
        )
        print(
            "  Then update bench/last-release-boot.json with the new SHA.",
        )
        print("")
        return 1

    if not quiet:
        print(
            "[release-staleness] ok — %d kernel commit(s) since last release "
            "boot at %s (%s), threshold %d"
            % (count, sha[:9], date, THRESHOLD),
        )
    return 0


# ---------------------------------------------------------------------------
# Self-tests
# ---------------------------------------------------------------------------

import tempfile as _tf


def _selftest():
    """Verify the gate's own logic in isolation."""
    ok = 0
    fail = 0

    # --- load_baseline ---

    # Missing file
    data, err = load_baseline("/nonexistent/path/to/file.json")
    if err and data is None:
        print("  ok   missing file is an error")
        ok += 1
    else:
        print("  FAIL missing file")
        fail += 1

    # Valid file
    with _tf.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, newline=""
    ) as fh:
        json.dump({"sha": "abc123", "date": "2026-01-01"}, fh)
        tmp = fh.name
    try:
        data, err = load_baseline(tmp)
        if err is None and data["sha"] == "abc123":
            print("  ok   valid baseline is read")
            ok += 1
        else:
            print("  FAIL valid baseline: %s" % err)
            fail += 1
    finally:
        os.unlink(tmp)

    # No sha field
    with _tf.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, newline=""
    ) as fh:
        json.dump({"date": "2026-01-01"}, fh)
        tmp = fh.name
    try:
        data, err = load_baseline(tmp)
        if err and "sha" in err:
            print("  ok   missing sha field is an error")
            ok += 1
        else:
            print("  FAIL missing sha field")
            fail += 1
    finally:
        os.unlink(tmp)

    # Bad JSON
    with _tf.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, newline=""
    ) as fh:
        fh.write("{bad json")
        tmp = fh.name
    try:
        data, err = load_baseline(tmp)
        if err:
            print("  ok   bad JSON is an error")
            ok += 1
        else:
            print("  FAIL bad JSON")
            fail += 1
    finally:
        os.unlink(tmp)

    # --- _sha_exists (uses real repo) ---
    if _repo_root():
        if _sha_exists("HEAD"):
            print("  ok   HEAD exists")
            ok += 1
        else:
            print("  FAIL HEAD should exist")
            fail += 1

        if not _sha_exists("0000000000000000000000000000000000000000"):
            print("  ok   zero SHA does not exist")
            ok += 1
        else:
            print("  FAIL zero SHA should not exist")
            fail += 1

    # --- what counts: kernel and bench changes, not records of runs ---
    try:
        counted = _records_selftest()
    except _Escaped as escaped:
        print("  FAIL the scratch repository resolved to %s, not itself -- "
              "refused before writing anything" % escaped)
        fail += 1
        counted = []
    if counted is None:
        print("  skip what-counts cases: no usable git")
    else:
        for name, got, want in counted:
            if got == want:
                print("  ok   %s (%d)" % (name, got))
                ok += 1
            else:
                print("  FAIL %s: counted %d, want %d" % (name, got, want))
                fail += 1

    print("")
    total = ok + fail
    print("%d self-test case(s), %d failed" % (total, fail))
    return 1 if fail else 0


class _Escaped(Exception):
    """The scratch repository resolved to some other repository."""


def _records_selftest():
    """Count real commits in a scratch repository; None if git cannot run.

    Returns (case, counted, wanted) triples.  Behavioural rather than a check
    of the RECORDS tuple: what matters is what `git rev-list` counts through
    `pathspecs()`, exclusion syntax included.

    Raises `_Escaped` -- before anything is written -- if git does not resolve
    the scratch directory as its own repository.  Every git call here carries
    `gitenv.clean_env()`; the check is the second line, because the first line
    was once missing and the result was commits on the branch being pushed.
    """
    git = ["git", "-c", "user.name=staleness-selftest", "-c",
           "user.email=staleness-selftest@localhost", "-c", "commit.gpgsign=false"]

    def run(args, cwd):
        return subprocess.check_output(git + args, cwd=cwd, text=True,
                                       stderr=subprocess.DEVNULL,
                                       env=gitenv.clean_env()).strip()

    def same_path(a, b):
        return os.path.normcase(os.path.realpath(a)) == os.path.normcase(os.path.realpath(b))

    def commit(cwd, rel, text):
        full = os.path.join(cwd, *rel.split("/"))
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "a", encoding="utf-8", newline="") as fh:
            fh.write(text + "\n")
        run(["add", rel], cwd)
        run(["commit", "-q", "--no-verify", "-m", rel], cwd)

    try:
        with _tf.TemporaryDirectory(ignore_cleanup_errors=True) as repo:
            run(["init", "-q"], repo)
            # The git directory, not the toplevel: with GIT_DIR set and no
            # GIT_WORK_TREE, git takes the *current directory* as the top of
            # the worktree, so --show-toplevel names the scratch directory
            # while every write goes to GIT_DIR's repository.
            git_dir = run(["rev-parse", "--absolute-git-dir"], repo)
            if not same_path(git_dir, os.path.join(repo, ".git")):
                raise _Escaped(git_dir)
            commit(repo, "README", "base")
            base = run(["rev-parse", "HEAD"], repo)
            cases = []
            commit(repo, "kernel/src/a.rs", "fn a() {}")
            cases.append(("a kernel commit counts",
                          _commit_count_since(base, pathspecs(), repo), 1))
            commit(repo, "bench/boot-history.jsonl", "{}")
            commit(repo, "bench/history.jsonl", "{}")
            commit(repo, "bench/last-release-boot.json", "{}")
            cases.append(("records of runs do not count",
                          _commit_count_since(base, pathspecs(), repo), 1))
            commit(repo, "bench/baselines.toml", "x = 1")
            cases.append(("a bench change that is not a record counts",
                          _commit_count_since(base, pathspecs(), repo), 2))
            commit(repo, "scripts/x.py", "pass")
            cases.append(("a scripts/ change does not count",
                          _commit_count_since(base, pathspecs(), repo), 2))
            return cases
    except (OSError, subprocess.CalledProcessError, ValueError):
        return None


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main():
    args = sys.argv[1:]
    if "--self-test" in args or "--selftest" in args:
        return _selftest()
    quiet = "--quiet" in args
    return check(quiet=quiet)


if __name__ == "__main__":
    sys.exit(main())
