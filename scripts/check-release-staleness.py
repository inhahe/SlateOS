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
"""
from __future__ import annotations

import json
import os
import subprocess
import sys

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


def _commit_count_since(sha, paths):
    """Count commits touching *paths* since *sha* (exclusive)."""
    cmd = ["git", "rev-list", "--count", "%s..HEAD" % sha, "--"]
    cmd.extend(paths)
    out = subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL)
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
        count = _commit_count_since(sha, PATHS)
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

    print("")
    total = ok + fail
    print("%d self-test case(s), %d failed" % (total, fail))
    return 1 if fail else 0


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
