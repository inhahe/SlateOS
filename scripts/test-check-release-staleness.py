#!/usr/bin/env python3
"""Regression tests for `scripts/check-release-staleness.py`.

Run: `python scripts/test-check-release-staleness.py` (0 = pass, 1 = fail).
No pytest dependency, matching the other suites in this directory.

What this tests, and why it exists
----------------------------------

The checker's `--selftest` builds a scratch repository and counts real commits
in it, to prove that records of runs (boot-history rows and the like) do not
count toward release staleness.  `pre-push` runs that self-test, and git
exports `GIT_DIR` into every hook's environment -- where it outranks the `cwd`
the scratch commands were given.  On 2026-09-26 the first version of the
self-test therefore wrote its seven fixture commits (`{}` over
`bench/boot-history.jsonl` and `bench/last-release-boot.json` among them)
onto the branch being pushed.  The push had already resolved its ref, so
nothing was published; the next boot test found `scripts/x.py` tracked and
missing and refused.  It was the second accident of this shape
(`scripts/gitenv.py` records the first, 2026-08-29, which did reach `main`).

A self-test cannot catch this about itself: its verdict is about the fixture,
and the fixture was the repository.  This file is the outside observer.  It runs
the self-test under each hostile environment shape `test-check-requests-not-
deleted.py` established -- `GIT_DIR` naming a sacrificial repository, as a hook
sets it -- and asserts the victim is untouched.  It also sabotages the first
line of defence (`gitenv.clean_env`) and checks that the second (the git-dir
guard) refuses before anything is written.
"""

from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECKER = os.path.join(REPO_ROOT, "scripts", "check-release-staleness.py")

_FAILURES: list[str] = []

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(errors="replace")
    except (AttributeError, ValueError):
        pass


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def _clean_env():
    """The environment minus anything that binds git to a repository.

    The harness builds its own fixtures in isolation for the same reason the
    checker must: `git bisect run` and `git rebase --exec` set these too.
    """
    env = dict(os.environ)
    for name in list(env):
        if name.startswith("GIT_"):
            del env[name]
    return env


def git(cwd, *args):
    proc = subprocess.run(["git", *args], cwd=cwd, env=_clean_env(),
                          capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout.strip()


def _probe(cwd, *args):
    """One snapshot field, as a string even when git refuses to answer: a
    damaged repository is what this suite looks for, so the probe that hits
    the damage carries the finding rather than aborting the run."""
    proc = subprocess.run(["git", *args], cwd=cwd, env=_clean_env(),
                          capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        return f"<git {' '.join(args)} failed: {proc.stderr.strip()}>"
    return proc.stdout.strip()


def _make_victim(tmp):
    """A small, complete repository that exists only to be damaged, holding
    the very files the 2026-09-26 self-test overwrote."""
    git(tmp, "init", "--quiet", "-b", "main")
    git(tmp, "config", "user.email", "victim@example.invalid")
    git(tmp, "config", "user.name", "victim")
    os.makedirs(os.path.join(tmp, "bench"))
    for name, text in (("boot-history.jsonl", '{"row": "real"}\n'),
                       ("last-release-boot.json", '{"sha": "real"}\n')):
        with open(os.path.join(tmp, "bench", name), "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
    git(tmp, "add", "-A")
    git(tmp, "commit", "--quiet", "-m", "the victim's only commit")


def _snapshot(tmp):
    return {
        "head": _probe(tmp, "rev-parse", "HEAD"),
        "branch": _probe(tmp, "rev-parse", "--abbrev-ref", "HEAD"),
        "refs": _probe(tmp, "for-each-ref", "--format=%(refname) %(objectname)"),
        "bare": _probe(tmp, "config", "--get", "core.bare"),
        "index": _probe(tmp, "ls-files", "--stage"),
        "log": _probe(tmp, "log", "--oneline"),
        "status": _probe(tmp, "status", "--porcelain"),
    }


# The shapes a hook environment takes; see test-check-requests-not-deleted.py
# for why each is a different failure mode rather than a louder version of the
# first.  The first is the one that happened.
_HOOK_ENVIRONMENTS = (
    ("GIT_DIR only, as `git push` sets it", ("GIT_DIR",)),
    ("GIT_DIR + GIT_WORK_TREE", ("GIT_DIR", "GIT_WORK_TREE")),
    ("GIT_DIR + GIT_WORK_TREE + GIT_INDEX_FILE",
     ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE")),
)


def test_the_selftest_cannot_damage_the_repository_it_runs_from():
    for label, names in _HOOK_ENVIRONMENTS:
        with tempfile.TemporaryDirectory(prefix="staleness-victim-",
                                         ignore_cleanup_errors=True) as tmp:
            _make_victim(tmp)
            before = _snapshot(tmp)
            values = {
                "GIT_DIR": os.path.join(tmp, ".git"),
                "GIT_WORK_TREE": tmp,
                "GIT_INDEX_FILE": os.path.join(tmp, ".git", "index"),
            }
            hostile = _clean_env()
            for name in names:
                hostile[name] = values[name]
            proc = subprocess.run([sys.executable, CHECKER, "--selftest"], cwd=tmp,
                                  env=hostile, capture_output=True, text=True, check=False)
            if not check(f"[{label}] the self-test still passes", proc.returncode, 0):
                print("        " + (proc.stdout + proc.stderr).strip().replace("\n", "\n        "))
            after = _snapshot(tmp)
            for key in sorted(before):
                check(f"[{label}] the self-test left {key} alone", after[key], before[key])


def _load_checker():
    spec = importlib.util.spec_from_file_location("check_release_staleness", CHECKER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_the_guard_refuses_before_writing_when_clean_env_is_gone():
    """The second line, with the first removed: `gitenv.clean_env` sabotaged to
    hand the environment through, GIT_DIR naming a decoy.  The self-test must
    raise before its first commit, and the decoy must have no commit."""
    module = _load_checker()
    saved = os.environ.get("GIT_DIR")
    with tempfile.TemporaryDirectory(prefix="staleness-decoy-",
                                     ignore_cleanup_errors=True) as decoy:
        git(decoy, "init", "--quiet")
        module.gitenv.clean_env = lambda base=None: dict(os.environ if base is None else base)
        os.environ["GIT_DIR"] = os.path.join(decoy, ".git")
        try:
            try:
                module._records_selftest()
                raised = False
            except module._Escaped:
                raised = True
        finally:
            if saved is None:
                del os.environ["GIT_DIR"]
            else:
                os.environ["GIT_DIR"] = saved
        check("the guard raises when the scratch repository resolves elsewhere", raised, True)
        check("the decoy received no commit",
              _probe(decoy, "rev-parse", "--verify", "-q", "HEAD").startswith("<git"), True)


def main():
    test_the_selftest_cannot_damage_the_repository_it_runs_from()
    test_the_guard_refuses_before_writing_when_clean_env_is_gone()
    if _FAILURES:
        print(f"\n{len(_FAILURES)} check(s) failed")
        return 1
    print("\nall release-staleness tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
