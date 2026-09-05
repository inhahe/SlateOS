#!/usr/bin/env python3
"""Regression tests for `scripts/check-requests-not-deleted.py`.

Run: `python scripts/test-check-requests-not-deleted.py` (0 = pass, 1 = fail).
No pytest dependency, matching the other suites in this directory.

What this tests, and why it exists
----------------------------------

The checker already has a `--selftest` that verifies it can *see* a deletion.
This suite tests the thing the self-test structurally cannot: that running the
self-test does not damage the repository it is run from.

On 2026-08-29 it did. `pre-push` gained a gate that runs `--selftest` before
trusting the checker, and on its first real invocation the self-test's fixture
commands -- `git init`, `git add -A`, `git commit`, run with `cwd=<tempdir>` --
all operated on the repository being pushed instead. Git exports `GIT_DIR` into
every hook's environment, and an explicit `GIT_DIR` beats both `-C` and `cwd`.
The damage was: `core.bare=true` set on the shared repository (which broke the
`os` integration worktree entirely), the index replaced by three fixture files,
and two commits written onto `lane-a` whose tree is a single `requests/`
directory -- both of which reached `origin/main` before anyone looked, because
the gate itself reported success.

That is a bug a self-test cannot catch by construction: the self-test's verdict
is about the fixture, and the fixture *was* the repository. It needs an outside
observer, which is this file. The first test below is that observer -- it hands
the checker a hostile environment (a `GIT_DIR` naming a sacrificial repository,
exactly as a hook would) and asserts the sacrificial repository is untouched
afterwards: same HEAD, same branch list, same `core.bare`, same index.

The remaining tests cover behaviour that has no coverage anywhere else, because
`--selftest` repoints the module's globals and so cannot exercise `main()`'s
argument handling or its exit codes.
"""

from __future__ import annotations

import inspect
import os
import subprocess
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECKER = os.path.join(REPO_ROOT, "scripts", "check-requests-not-deleted.py")

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
    """The environment minus anything that would bind git to a repository.

    The harness must build its own fixtures in isolation for the same reason
    the checker must. It is not run from a hook today -- `boot-test.sh` runs
    every `scripts/test-*.py`, and nothing runs `boot-test.sh` from a hook --
    but "today" is the whole of the guarantee, and `git bisect run` and
    `git rebase --exec` set these variables too.
    """
    env = dict(os.environ)
    for name in list(env):
        if name.startswith("GIT_"):
            del env[name]
    return env


def git(cwd, *args, check_rc=True):
    proc = subprocess.run(["git", *args], cwd=cwd, env=_clean_env(),
                          capture_output=True, text=True, check=False)
    if check_rc and proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout.strip()


def run_checker(*args, env=None, cwd=None):
    proc = subprocess.run(
        [sys.executable, CHECKER, *args],
        cwd=cwd or REPO_ROOT,
        env=env if env is not None else _clean_env(),
        capture_output=True, text=True, check=False,
    )
    return proc.returncode, (proc.stdout or "") + (proc.stderr or "")


def _make_victim(tmp):
    """A small, complete repository that exists only to be damaged.

    Deliberately not a clone of this one: the test must be able to say "nothing
    changed" about every ref and every config key, which is only tractable for a
    repository the test built.
    """
    git(tmp, "init", "--quiet", "-b", "main")
    git(tmp, "config", "user.email", "victim@example.invalid")
    git(tmp, "config", "user.name", "victim")
    os.makedirs(os.path.join(tmp, "requests"))
    for name in ("keep-me.md", "and-me.md"):
        with open(os.path.join(tmp, "requests", name), "w", encoding="utf-8", newline="") as fh:
            fh.write("# a request that must survive\n")
    git(tmp, "add", "-A")
    git(tmp, "commit", "--quiet", "-m", "the victim's only commit")
    return git(tmp, "rev-parse", "HEAD")


def _probe(cwd, *args):
    """One snapshot field, as a string, even when git refuses to answer.

    Raising here would be wrong twice over. A damaged repository is exactly what
    this suite is looking for, so the probe that hits the damage is the one
    carrying the finding -- and the first observed damage was `core.bare=true`,
    which makes `git status` and `git ls-files` fail outright rather than return
    something different. Turning that into an exception aborts the run, loses
    the remaining environment shapes, and reports a traceback where a diff
    belongs.
    """
    proc = subprocess.run(["git", *args], cwd=cwd, env=_clean_env(),
                          capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        return f"<git {' '.join(args)} failed: {proc.stderr.strip()}>"
    return proc.stdout.strip()


def _snapshot(tmp):
    """Everything the 2026-08-29 incident actually changed, plus the refs."""
    return {
        "head": _probe(tmp, "rev-parse", "HEAD"),
        "branch": _probe(tmp, "rev-parse", "--abbrev-ref", "HEAD"),
        "refs": _probe(tmp, "for-each-ref", "--format=%(refname) %(objectname)"),
        "bare": _probe(tmp, "config", "--get", "core.bare"),
        "index": _probe(tmp, "ls-files"),
        "log": _probe(tmp, "log", "--oneline"),
        "status": _probe(tmp, "status", "--porcelain"),
    }


# The environments to run the self-test under. The first is the one that
# actually happened: `git push` exports `GIT_DIR` and nothing else, and that
# alone was enough -- with no `GIT_WORK_TREE` to contradict it, git took the
# fixture's temp directory as the work tree and wrote the commits into the
# repository being pushed.
#
# The later shapes are not embellishments of it but different failure modes.
# Setting `GIT_INDEX_FILE` as well makes the fixture's first `git commit` fail
# outright, so a broken checker is caught by its *exit code* and never gets far
# enough to damage anything; setting `GIT_WORK_TREE` makes the fixture write to
# the victim's own files. A test that used only the loudest of the three would
# prove the least, since the quiet one is the one that shipped.
_HOOK_ENVIRONMENTS = (
    ("GIT_DIR only, as `git push` sets it", ("GIT_DIR",)),
    ("GIT_DIR + GIT_WORK_TREE", ("GIT_DIR", "GIT_WORK_TREE")),
    ("GIT_DIR + GIT_WORK_TREE + GIT_INDEX_FILE",
     ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE")),
)


def test_the_selftest_cannot_damage_the_repository_it_runs_from():
    """The 2026-08-29 incident, as a test.

    A hook's environment is not approximated here: the variables are set to the
    values `git` itself sets, because the whole failure was that they outrank
    `cwd` and `-C`.
    """
    for label, names in _HOOK_ENVIRONMENTS:
        with tempfile.TemporaryDirectory(prefix="reqgate-victim-") as tmp:
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

            rc, out = run_checker("--selftest", env=hostile, cwd=tmp)
            if not check(f"[{label}] the self-test still passes", rc, 0):
                print("        --- checker output ---")
                print("        " + out.strip().replace("\n", "\n        "))

            after = _snapshot(tmp)
            for key in sorted(before):
                check(f"[{label}] the self-test left {key} alone",
                      after[key], before[key])


def test_the_checker_reads_the_repository_it_was_told_to():
    """The same confusion, in the gate rather than the self-test.

    `_git` picks the repository with `cwd=ROOT`. If `GIT_DIR` can override that,
    the gate silently reports on a repository nobody asked about -- and since a
    foreign repository has no `requests/`, it reports "clean".
    """
    with tempfile.TemporaryDirectory(prefix="reqgate-foreign-") as tmp:
        _make_victim(tmp)
        hostile = _clean_env()
        hostile["GIT_DIR"] = os.path.join(tmp, ".git")
        hostile["GIT_WORK_TREE"] = tmp

        rc, out = run_checker(env=hostile)
        check("the gate exits cleanly with GIT_DIR naming another repo", rc, 0)

        # "It exited 0" proves nothing on its own: the fixture has a `requests/`
        # directory and no deletions either, so a gate that read the *fixture*
        # would also print OK. The 12-character base in that line is the only
        # part of the output that names the repository it read -- and it has to
        # be the merge base the gate computed for itself, not one passed in with
        # `--base`, which is echoed verbatim and so identifies nothing.
        ours = _probe(REPO_ROOT, "merge-base", "HEAD", "origin/main")[:12]
        theirs = _probe(tmp, "rev-parse", "HEAD")[:12]
        check("the gate computed its base in the repo it was told to",
              f"base {ours}" in out, True)
        check("...and not in the one GIT_DIR named",
              f"base {theirs}" in out, False)


def test_bad_arguments_are_rejected_rather_than_ignored():
    """A gate that silently ignores an argument it cannot honour is a gate off.

    `--head` in particular is passed a `$sha` by `pre-push`; if a typo or an
    unfetched ref made that a no-op, the push gate would fall back to judging
    the worktree -- which is precisely the false negative `--head` exists to
    close, restored quietly.
    """
    rc, out = run_checker("--head", "definitely-not-a-commit")
    check("a --head that is not a commit exits 2", rc, 2)
    check("...and says so", "is not a commit" in out, True)

    rc, out = run_checker("--base", "definitely-not-a-commit")
    check("a --base that is not a commit exits 2", rc, 2)
    check("...and says so", "is not a commit" in out, True)


def test_a_repository_with_no_trunk_is_skipped_not_failed():
    """A fresh clone that has fetched nothing has no history to compare.

    That is "cannot answer", not "no deletions" -- but the two must not be
    conflated in the other direction either: failing here would make the gate
    unrunnable in a worktree that is merely new.

    The script has to be *copied into* the fixture to test this. `ROOT` is
    derived from `__file__`, so the checker always reads the tree it is stored
    in and cannot be pointed elsewhere by `cwd` -- which is the same property
    the two tests above rely on, seen from the other side.
    """
    with tempfile.TemporaryDirectory(prefix="reqgate-notrunk-") as tmp:
        _make_victim(tmp)
        git(tmp, "branch", "-m", "main", "not-main")
        scripts = os.path.join(tmp, "scripts")
        os.makedirs(scripts, exist_ok=True)
        # `gitenv.py` and `gittree.py` travel with it: the checker imports both,
        # so a copy without them does not start. Listing the dependencies here
        # rather than copying `scripts/` wholesale is deliberate -- this
        # assertion is what fails if the checker grows another import, which is
        # a thing worth being told about in a gate that has to run in a fresh
        # clone.
        #
        # `gittree.py` is the second one, and it arrived without this list being
        # updated: gate 9 learned to read the waiver list out of the commit
        # instead of off the disk, which needs a tree reader, and this test then
        # failed as designed -- `ModuleNotFoundError: No module named 'gittree'`,
        # surfacing as the no-trunk case exiting 1 instead of skipping with 0.
        # It is listed rather than reached transitively on purpose: the point of
        # the fixture is that the checker runs from a directory holding only
        # what it truly needs, and a copy that pulled in imports automatically
        # would stop being able to say what that is. `gittree` imports `gitenv`,
        # which is why the two-name list is still closed.
        for source in (CHECKER,
                       os.path.join(REPO_ROOT, "scripts", "gitenv.py"),
                       os.path.join(REPO_ROOT, "scripts", "gittree.py")):
            with open(source, encoding="utf-8") as src:
                body = src.read()
            with open(os.path.join(scripts, os.path.basename(source)),
                      "w", encoding="utf-8", newline="") as dst:
                dst.write(body)
        installed = os.path.join(scripts, os.path.basename(CHECKER))

        proc = subprocess.run([sys.executable, installed], cwd=tmp,
                              env=_clean_env(), capture_output=True,
                              text=True, check=False)
        out = (proc.stdout or "") + (proc.stderr or "")
        check("no origin/main and no main is a SKIP", proc.returncode, 0)
        check("...and says which refs it looked for",
              "origin/main" in out and "SKIP" in out, True)


def test_the_gate_is_clean_on_this_repository():
    """The plain run, as `boot-test.sh` and `pre-boot.py` make it."""
    rc, out = run_checker()
    check("this repository has no deleted requests", rc, 0)
    check("...and the gate said so", "OK" in out or "SKIP" in out, True)


def main():
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes. Assert a floor, as the sibling suites do.
    if len(tests) < 5:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 5. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        if inspect.signature(fn).parameters:
            print(f"FATAL: {name} takes arguments; these tests build their own "
                  f"fixtures and are called with none.")
            return 1
        fn()

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} check-requests-not-deleted tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
