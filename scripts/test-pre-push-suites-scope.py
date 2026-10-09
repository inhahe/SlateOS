#!/usr/bin/env python3
"""Tests for which scripts a push counts as changed, in `scripts/hooks/pre-push`.

Run: `python scripts/test-pre-push-suites-scope.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory.

What this tests, and why it exists
----------------------------------

Gate 20 refuses to publish a `scripts/*.py` whose own test suite fails, and it
finds those suites from the scripts the push changes. Until 2026-09-26 that
list came from `git rev-list ... | xargs -r git diff-tree ...`, which is right
for one commit and wrong for more: `git diff-tree A B` compares two trees
rather than each commit with its parent, and with three or more it is a usage
error. Both were silenced by `2>/dev/null`, so every push of two or more commits
reported "no scripts/*.py in this push has a paired test suite" and ran none --
the gate off, on exactly the pushes that carry the most change. The same
pipeline decided whether the hook's own `test-pre-push-*` suites ran.

Both now read `pushed_paths`, the command `touches_prepare` already used. This
suite lifts that function and the two pipelines out of the hook verbatim, runs
them under the hook's own shell against a scratch repository, and requires the
answer git gives commit by commit. The pushes are the ones the old pipeline got
wrong -- two commits, three commits, a change a later commit reverts -- and the
ones any version must get right: one commit, a root commit, a commit the remote
already has, and nothing published at all.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HOOK = os.path.join(REPO_ROOT, "scripts", "hooks", "pre-push")
sys.path.insert(0, os.path.join(REPO_ROOT, "scripts"))
import gitenv  # noqa: E402
import msysbash  # noqa: E402

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


def git(repo, *args):
    """git in the scratch repository, never the ambient one (scripts/gitenv.py)."""
    proc = subprocess.run(["git", *args], cwd=repo, env=gitenv.clean_env(),
                          capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout.strip()


def lifted(text):
    """`pushed_paths()` and the two pipelines that read it, exactly as the hook
    has them. Lifting rather than restating is the point: a copy here would go
    on passing after the hook changed."""
    fn = re.search(r"^pushed_paths\(\)\s*\{.*?^\}\n", text, re.MULTILINE | re.DOTALL)
    if fn is None:
        raise RuntimeError("the hook has no `pushed_paths() { ... }` any more")
    scripts = re.search(r"^\s*changed_scripts=\$\((.*)\)\s*$", text, re.MULTILINE)
    if scripts is None:
        raise RuntimeError("the hook no longer computes `changed_scripts=$(...)`")
    hook_suites = re.search(r"^\s*if (pushed_paths \| grep -q '\^scripts/hooks/pre-push\$')\s*$",
                            text, re.MULTILINE)
    if hook_suites is None:
        raise RuntimeError("the hook no longer asks `pushed_paths` whether it changed itself")
    return fn.group(0), scripts.group(1), hook_suites.group(1)


def run_hook_side(repo, pushed, remote, fn, scripts_cmd, hook_cmd):
    """(changed scripts, did it change the hook) as the hook's shell answers."""
    script = (
        f"pushed_shas='{pushed}'\n"
        f"remote_name='{remote}'\n"
        f"{fn}\n"
        f"printf '%s\\n' \"$({scripts_cmd})\"\n"
        f"if {hook_cmd}; then echo HOOK=yes; else echo HOOK=no; fi\n"
    )
    proc = subprocess.run([msysbash.bash(), "--posix", "-c", script], cwd=repo,
                          env=gitenv.clean_env(), capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"the lifted pipeline failed: {proc.stderr.strip()}")
    lines = [line for line in proc.stdout.splitlines() if line]
    hook = lines.pop() if lines and lines[-1].startswith("HOOK=") else "HOOK=?"
    return lines, hook == "HOOK=yes"


def git_side(repo, pushed, remote):
    """The same question asked commit by commit, the slow and obvious way."""
    commits = git(repo, "rev-list", *pushed.split(), "--not", f"--remotes={remote}").split()
    paths = set()
    for commit in commits:
        parents = git(repo, "rev-list", "--parents", "-n", "1", commit).split()[1:]
        if len(parents) > 1:
            continue  # a merge lists nothing, as `pushed_paths` documents
        base = parents[0] if parents else "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        out = git(repo, "diff", "--name-only", "--no-renames", base, commit)
        paths.update(p for p in out.splitlines() if p)
    return sorted(p for p in paths if p.startswith("scripts/")), "scripts/hooks/pre-push" in paths


def commit(repo, files, message):
    for rel, content in files.items():
        full = os.path.join(repo, *rel.split("/"))
        if content is None:
            os.remove(full)
            git(repo, "rm", "-q", "--cached", rel)
            continue
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8", newline="") as fh:
            fh.write(content)
        git(repo, "add", rel)
    git(repo, "commit", "-q", "--no-verify", "--allow-empty", "-m", message)
    return git(repo, "rev-parse", "HEAD")


def main() -> int:
    with open(HOOK, encoding="utf-8", newline="") as fh:
        fn, scripts_cmd, hook_cmd = lifted(fh.read())

    with tempfile.TemporaryDirectory(prefix="suites-scope-", ignore_cleanup_errors=True) as repo:
        git(repo, "init", "-q", "-b", "main")
        git(repo, "config", "user.email", "scope@example.invalid")
        git(repo, "config", "user.name", "scope")
        git(repo, "config", "commit.gpgsign", "false")

        root = commit(repo, {"scripts/root.py": "r\n", "README": "x\n"}, "root")
        published = commit(repo, {"scripts/old.py": "o\n"}, "already on the remote")
        git(repo, "update-ref", "refs/remotes/origin/main", published)

        one = commit(repo, {"scripts/a.py": "a\n"}, "one commit")
        two = commit(repo, {"scripts/b.sh": "b\n"}, "second commit")
        three = commit(repo, {"docs/x.md": "d\n", "scripts/hooks/pre-push": "h\n"}, "third commit")
        reverted = commit(repo, {"scripts/a.py": None}, "reverts the first commit's script")

        scenarios = [
            ("one commit", one, "origin"),
            ("two commits", two, "origin"),
            ("three commits, one touching the hook itself", three, "origin"),
            ("a change a later commit reverts still counts", reverted, "origin"),
            ("nothing new: the remote already has it", published, "origin"),
            ("a root commit, with no remote at all", root, "nowhere"),
        ]
        for label, pushed, remote in scenarios:
            got_scripts, got_hook = run_hook_side(repo, pushed, remote, fn, scripts_cmd, hook_cmd)
            want_scripts, want_hook = git_side(repo, pushed, remote)
            check(f"[{label}] the changed scripts are git's", got_scripts, want_scripts)
            check(f"[{label}] whether the hook changed itself is git's", got_hook, want_hook)

        # The case that found the bug, stated outright rather than only
        # compared: three commits, two scripts, and the hook.
        got_scripts, got_hook = run_hook_side(repo, three, "origin", fn, scripts_cmd, hook_cmd)
        check("three commits name both scripts and the hook",
              (got_scripts, got_hook),
              (["scripts/a.py", "scripts/b.sh", "scripts/hooks/pre-push"], True))

    if _FAILURES:
        print(f"\n{len(_FAILURES)} check(s) failed")
        return 1
    print("\nall suites-scope tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
