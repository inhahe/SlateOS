#!/usr/bin/env python3
"""Tests for `tree_is_push`, the precondition of pre-push gates 52-73.

Run: `python scripts/test-pre-push-tree-is-push.py` (0 = pass, 1 = fail).

Why this exists
---------------

Gates 52-73 are boot-test gates moved to the push (design-decisions 974, the
operator's answer to A-Q13). None of their checkers takes `--head`: each reads
the working tree. A working-tree gate judging a push is right only when the
working tree *is* the push, and wrong in both directions otherwise -- a defect
committed and then tidied on disk is published unseen, and a clean push is
refused for an uncommitted edit it does not carry
(`known-issues.md` -> TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH).

`tree_is_push <what> <path>...` is the guard: it lets a gate run only when one
ref is pushed, that ref's commit is checked out, and nothing under the gate's
own paths is modified or untracked. Otherwise the gate declines by name and
says it was NOT checked. Each property below is one way that guard could be
wrong, and a wrong guard is either a gate that judges the wrong tree or a gate
that never runs -- neither of which any other test in this tree would notice.

The function is cut out of the real hook, not copied, so the suite tests what
the hook runs. It runs in a scratch repository with the environment's
repository bindings removed (`gitenv.clean_env`), never in this one.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gitenv  # noqa: E402
import msysbash  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
HOOK = os.path.join(HERE, "hooks", "pre-push")

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}",
              file=sys.stderr)
        failures.append(label)


def extract_function() -> str:
    with open(HOOK, encoding="utf-8") as fh:
        text = fh.read()
    m = re.search(r"^tree_is_push\(\) \{\n.*?^\}\n", text, re.MULTILINE | re.DOTALL)
    if not m:
        raise SystemExit("tree_is_push() not found in scripts/hooks/pre-push")
    return m.group(0)


def git(repo: str, *args: str) -> str:
    proc = subprocess.run(["git", *args], cwd=repo, env=gitenv.clean_env(),
                          capture_output=True, text=True, check=True)
    return proc.stdout.strip()


def write(repo: str, rel: str, text: str) -> None:
    path = os.path.join(repo, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def run(repo: str, fn: str, pushed: str, *paths: str) -> tuple[int, str]:
    """Run `tree_is_push t <paths>` with `pushed_shas` set, in `repo`."""
    script = fn + f'pushed_shas="{pushed}"\ntree_is_push "gate T (test)"'
    script += "".join(f' "{p}"' for p in paths) + "\n"
    proc = subprocess.run([msysbash.bash(), "-c", script], cwd=repo,
                          env=gitenv.clean_env(), capture_output=True, text=True)
    return proc.returncode, proc.stderr


def main() -> int:
    fn = extract_function()
    repo = tempfile.mkdtemp(prefix="tree-is-push-")
    try:
        git(repo, "init", "-q")
        git(repo, "config", "user.name", "tree-is-push fixture")
        git(repo, "config", "user.email", "fixture@invalid")
        write(repo, "kernel/a.rs", "fn a() {}\n")
        write(repo, "scripts/b.sh", "echo b\n")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", "one")
        write(repo, "kernel/a.rs", "fn a() {}\nfn b() {}\n")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", "two")
        head = git(repo, "rev-parse", "HEAD")
        prev = git(repo, "rev-parse", "HEAD~1")

        rc, _ = run(repo, fn, f" {head}", "kernel/")
        check("a clean tree with the pushed commit checked out runs the gate", rc, 0)

        write(repo, "scripts/b.sh", "echo b changed\n")
        rc, err = run(repo, fn, f" {head}", "scripts/")
        check("an uncommitted edit under the gate's paths declines it", rc, 1)
        check("...and says so by the gate's name", "gate T (test) declines" in err, True)
        check("...and that it was NOT checked", "NOT checked" in err, True)
        rc, _ = run(repo, fn, f" {head}", "kernel/")
        check("an uncommitted edit elsewhere does not decline it", rc, 0)
        git(repo, "checkout", "-q", "--", "scripts/b.sh")

        rc, _ = run(repo, fn, f" {prev}", "kernel/")
        check("a push of a commit that is not checked out declines", rc, 1)

        rc, err = run(repo, fn, f" {head} {prev}", "kernel/")
        check("a push of two refs declines", rc, 1)
        check("...naming the count", "sends 2 refs" in err, True)

        write(repo, "kernel/new.rs", "fn n() {}\n")
        rc, _ = run(repo, fn, f" {head}", "kernel/")
        check("an untracked file under the gate's paths declines it", rc, 1)
        rc, _ = run(repo, fn, f" {head}", "scripts/")
        check("an untracked file elsewhere does not", rc, 0)
        os.remove(os.path.join(repo, "kernel", "new.rs"))

        rc, _ = run(repo, fn, f" {head}", "kernel/", "scripts/")
        check("several paths, all clean, run the gate", rc, 0)
    finally:
        shutil.rmtree(repo, ignore_errors=True)

    print()
    if failures:
        print(f"{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("all tree_is_push tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
