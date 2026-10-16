#!/usr/bin/env python3
"""Behavioural tests for pre-push gate 77 (what reaches main lies in a review).

Run: `python scripts/test-pre-push-review-gate.py` (0 = pass, 1 = fail).

The gate's verdict is `scripts/review-batch.py --check`, which has its own
self-test. What that self-test cannot see is the gate's wiring in the hook:
which pushes it judges, the arguments it hands the checker, what it does when
the checker refuses, its bypass, and what it does when the checker is not
there. Each of those once went wrong in a way only a real publish showed --
the call ran before the gates' library was loaded, it went around
`run_checker`, a missing checker failed every hook test's scratch repository
-- so this drives the real hook over real pushes, with a stand-in checker
committed in the fixture that records how it was called and answers as told.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gitenv  # noqa: E402

_REMOVED = gitenv.scrub_environ()

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HOOK = os.path.join(REPO_ROOT, "scripts", "hooks", "pre-push")
LIB = os.path.join(REPO_ROOT, "scripts", "run-checker.sh")

#: The stand-in checker: records its arguments, one JSON list a line, and
#: exits with FAKE_RB_EXIT.
STUB = """import json, os, sys
log = os.environ.get("FAKE_RB_LOG")
if log:
    with open(log, "a", encoding="utf-8") as f:
        f.write(json.dumps(sys.argv[1:]) + "\\n")
sys.exit(int(os.environ.get("FAKE_RB_EXIT", "0")))
"""

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}", file=sys.stderr)
        failures.append(label)


def git(cwd: str, *args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    """git in `cwd`, never inheriting a repository binding; `check=False`,
    since a refused push is the thing under test."""
    return subprocess.run(["git", *args], cwd=cwd, env=env or gitenv.clean_env(),
                          capture_output=True, text=True, check=False)


def write(path: str, text: str) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(text)


def build_fixture(tmp: str, name: str, with_checker: bool) -> str:
    """A repository with the real hook installed (as a copy) and the real
    gates' library beside it, and -- when `with_checker` -- the stand-in
    checker committed, as a real checkout carries review-batch.py."""
    remote = os.path.join(tmp, f"{name}.git")
    work = os.path.join(tmp, name)
    git(tmp, "init", "--quiet", "--bare", remote)
    git(tmp, "init", "--quiet", "-b", "main", work)
    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    git(work, "config", "commit.gpgsign", "false")
    with open(HOOK, encoding="utf-8", newline="") as f:
        write(os.path.join(work, ".git", "hooks", "pre-push"), f.read())
    os.chmod(os.path.join(work, ".git", "hooks", "pre-push"), 0o755)
    # Untracked, as in test-pre-push-identity-gate.py: the library the hook
    # sources, not a file of the pushes under test.
    with open(LIB, encoding="utf-8", newline="") as f:
        write(os.path.join(work, "scripts", "run-checker.sh"), f.read())
    git(work, "remote", "add", "origin", remote)
    write(os.path.join(work, "a.txt"), "one\n")
    git(work, "add", "a.txt")
    if with_checker:
        write(os.path.join(work, "scripts", "review-batch.py"), STUB)
        git(work, "add", "scripts/review-batch.py")
    git(work, "commit", "--quiet", "-m", "first")
    return work


def commit(work: str, name: str) -> str:
    write(os.path.join(work, name), name + "\n")
    git(work, "add", name)
    git(work, "commit", "--quiet", "-m", f"add {name}")
    return git(work, "rev-parse", "HEAD").stdout.strip()


def push(work: str, refspec: str, log: str, exit_code: int, **extra: str) -> tuple[int, str]:
    env = gitenv.clean_env()
    env.update({"FAKE_RB_LOG": log, "FAKE_RB_EXIT": str(exit_code), **extra})
    proc = git(work, "push", "origin", refspec, env=env)
    return proc.returncode, proc.stdout + proc.stderr


def calls(log: str) -> list[list[str]]:
    if not os.path.exists(log):
        return []
    with open(log, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def main() -> int:
    if _REMOVED:
        print(f"note: ignored inherited {', '.join(sorted(_REMOVED))}")
    if not os.path.isfile(HOOK):
        print(f"FAIL  hook not found at {HOOK}", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory(prefix="reviewgate-") as tmp:
        log = os.path.join(tmp, "calls.jsonl")
        work = build_fixture(tmp, "w", with_checker=True)
        first = git(work, "rev-parse", "HEAD").stdout.strip()

        rc, out = push(work, "main", log, 0)
        check("a reviewed publish to main is allowed", rc, 0)
        check("...judged by --check, with the remote and the pushed sha",
              calls(log), [["--check", "--remote", "origin", first]])

        sha = commit(work, "b.txt")
        rc, out = push(work, "main", log, 1)
        check("an unreviewed publish to main is refused", rc != 0, True)
        check("...saying why", "lies in no recorded review" in out, True)
        check("...and to review what was pushed, not HEAD",
              f"review-batch.py origin/main..{sha}" in out, True)

        before = len(calls(log))
        rc, out = push(work, "main", log, 1, ALLOW_UNREVIEWED="1")
        check("ALLOW_UNREVIEWED=1 publishes it anyway", rc, 0)
        check("...without asking the checker", len(calls(log)), before)

        sha = commit(work, "c.txt")
        rc, out = push(work, "HEAD:refs/heads/side", log, 1)
        check("a push to a lane branch is not judged", rc, 0)
        check("...and the checker is not asked", len(calls(log)), before)

        # The checker gone from the checkout while the commits carry it: the
        # gate has nothing to judge by, and must not wave the push through.
        os.remove(os.path.join(work, "scripts", "review-batch.py"))
        rc, out = push(work, "main", log, 0)
        check("a checkout that lost the checker is refused", rc != 0, True)
        check("...saying so", "has none to judge them by" in out, True)
        git(work, "checkout", "--", "scripts/review-batch.py")

        # A repository whose commits never carried it -- every other hook
        # test's scratch repository -- is skipped, and says so.
        bare = build_fixture(tmp, "nochecker", with_checker=False)
        rc, out = push(bare, "main", log, 1)
        check("a tree without the checker is not judged", rc, 0)
        check("...and the skip is said", "gate 77 skipped" in out, True)

    if failures:
        print(f"\n{len(failures)} review-gate test(s) failed", file=sys.stderr)
        return 1
    print("\nall review-gate tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
