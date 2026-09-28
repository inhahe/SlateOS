#!/usr/bin/env python3
"""Tests for `scripts/lane-claims.py` through its command line.

Run: `python scripts/test-lane-claims.py` (0 = pass, 1 = fail).

The script's `--self-test` covers its functions on a scratch directory. This
covers what it cannot: `main()`'s arguments and exit codes, the claim landing in
the git *common* directory of whatever repository it is run in, and a session
whose lane cannot be told being refused rather than recorded as lane "?".

Each case runs in a throwaway repository with `GIT_DIR` and its neighbours
removed from the environment: a git hook exports `GIT_DIR`, and a test that
inherited one would write its claims into the repository being pushed
(`test-check-requests-not-deleted.py` records the day that happened to another
script's self-test).

**The scripts run from a copy inside that repository, not from this tree.**
`which-lane.py` counts the worktree a script runs from as evidence of the
lane, so run from a lane's own worktree they answer that lane whatever the
environment says: `SLATEOS_LANE=E` then *contradicts* the worktree and the
lane cannot be told, and a session with no lane at all is taken for the
worktree's. The suite passed where it was written, in a worktree that names no
lane, and failed its first boot in `os-lane-c` (2026-09-28). A copy in the
throwaway repository runs from a worktree that is no lane's, so the
environment alone decides, as in the cases below. The three scripts import
only each other and the standard library.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "lane-claims.py"
#: What `lane-claims.py` loads beside itself, copied with it.
COPIED = ("lane-claims.py", "check-lane-signals.py", "which-lane.py")


def run(args, cwd, lane="C"):
    env = {k: v for k, v in os.environ.items()
           if not k.startswith("GIT_") and k not in ("SLATEOS_LANE", "ORCH2_AGENT_NAME")}
    if lane is not None:
        env["SLATEOS_LANE"] = lane
    script = Path(cwd) / "scripts" / "lane-claims.py"
    return subprocess.run([sys.executable, str(script), *args], cwd=cwd, env=env,
                          capture_output=True, text=True, check=False)


def main() -> int:
    failures = []

    def check(ok, what, out=None):
        if not ok:
            failures.append(what + (f"\n    {out.stdout.strip()} {out.stderr.strip()}" if out else ""))

    r = subprocess.run([sys.executable, str(SCRIPT), "--self-test"], capture_output=True,
                       text=True, check=False)
    check(r.returncode == 0, "--self-test failed", r)

    with tempfile.TemporaryDirectory(prefix="lane_claims_test_") as tmp:
        repo = Path(tmp)
        env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        subprocess.run(["git", "init", "-q", str(repo)], env=env, check=True)
        (repo / "scripts").mkdir()
        for name in COPIED:
            (repo / "scripts" / name).write_bytes((HERE / name).read_bytes())

        r = run(["--list"], repo)
        check(r.returncode == 0 and "no claims" in r.stdout, "an empty list", r)

        r = run(["--claim", "Build the widget", "--paths", "gui/widget"], repo)
        check(r.returncode == 0, "a claim was refused", r)
        stored = list((repo / ".git" / "coordination" / "claims").glob("*.txt"))
        check([p.name for p in stored] == ["c-build-the-widget.txt"],
              f"the claim is not in the common directory: {stored}")

        r = run(["--list"], repo, lane="E")
        check(r.returncode == 0 and "lane C" in r.stdout and "Build the widget" in r.stdout,
              "another lane does not see the claim", r)

        r = run(["--check", "gui/widget/src/lib.rs"], repo, lane="E")
        check(r.returncode == 1 and "Build the widget" in r.stdout,
              "a path inside the claim is not reported", r)
        r = run(["--check", "gui/other"], repo, lane="E")
        check(r.returncode == 0, "an unclaimed path is reported", r)

        r = run(["--release", "Build the widget"], repo, lane="E")
        check(r.returncode == 0 and "no claim" in r.stdout and stored[0].exists(),
              "another lane released the claim", r)
        r = run(["--release", "Build the widget"], repo)
        check(r.returncode == 0 and not stored[0].exists(), "its own lane could not release it", r)

        r = run(["--claim", "Anything"], repo, lane=None)
        check(r.returncode == 2, "a session with no lane was allowed to claim", r)

    if failures:
        print("test-lane-claims: FAILED")
        for f in failures:
            print("  " + f)
        return 1
    print("test-lane-claims: all 9 checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
