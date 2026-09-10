#!/usr/bin/env python3
"""Tests for `commit_boot_history`, the one function in the harness that commits.

`bench/boot-history.jsonl` is tracked and appended to by every boot test, so the
worktree's normal state is dirty in a file all three lanes write, and a run's row
lives only in the working tree until somebody remembers to commit it. On
2026-08-24 a `git checkout -- bench/boot-history.jsonl`, run to clear the dirty
file before a merge, deleted two unrecorded boots including the passing one that
had just turned the tree green. Nothing warned, because dirty is what that file
always looks like. See known-issues.md ->
TD-A-BOOT-HISTORY-IS-A-TRACKED-FILE-EVERY-BOOT-DIRTIES.

`scripts/boot-test.sh` now commits the row itself, as its own one-file commit. A
harness that commits on its own behalf has to be harder to surprise than one that
does not, so each of its refusals gets a case here.

## Why the function is extracted rather than reimplemented

The same reason `test-pre-push-run-checker.py` gives: the thing worth pinning is
not "does a copy of this logic behave" but "does the shipped file behave". The
function is cut out of `scripts/boot-test.sh` by brace matching and evaluated, so
renaming or restructuring it fails loudly instead of leaving this suite grading a
copy that no longer ships.

## The case that matters most

`commits only the ledger when other work is staged`. The function runs at the end
of a boot test, in a worktree the operator may have staged work in, and
`git commit` without a pathspec would sweep it into a commit titled as a ledger
row. That is not a hypothetical risk worth one assertion -- it is the difference
between a bookkeeping convenience and a harness that silently rewrites your
commit.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BOOT = ROOT / "scripts" / "boot-test.sh"
LEDGER = "bench/boot-history.jsonl"

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label} -- got {got!r}, want {want!r}")
        failures.append(label)


def extract(name: str, text: str) -> str:
    """The `name() { ... }` block, by a closing brace at column 0."""
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if line.startswith(f"{name}() {{"):
            start = i
            break
    if start is None:
        raise SystemExit(
            f"cannot find `{name}() {{` in scripts/boot-test.sh -- it was renamed "
            "or restructured, and this suite is testing nothing. Update the "
            "extraction, do not delete the test."
        )
    for j in range(start + 1, len(lines)):
        if lines[j] == "}":
            return "\n".join(lines[start : j + 1])
    raise SystemExit(f"`{name}` has no closing brace at column 0")


def git(repo: Path, *args: str) -> str:
    out = subprocess.run(
        ["git", *args], cwd=repo, capture_output=True, text=True, check=False
    )
    return (out.stdout or "").strip()


def make_repo(tmp: Path) -> Path:
    repo = tmp / "repo"
    (repo / "bench").mkdir(parents=True)
    git(repo, "init", "--quiet", "-b", "main")
    git(repo, "config", "user.email", "t@example.invalid")
    git(repo, "config", "user.name", "t")
    (repo / LEDGER).write_text('{"row": 1}\n', encoding="utf-8", newline="")
    # A second file in the initial commit, so "the commit holds only the ledger"
    # cannot pass vacuously. It did: with the ledger as the repo's only file, that
    # assertion matched the INIT commit while bash had never run at all, and
    # reported ok beside five failures caused by the same silence.
    (repo / "README").write_text("fixture\n", encoding="utf-8", newline="")
    git(repo, "add", "-A")
    git(repo, "commit", "--quiet", "-m", "init")
    return repo


def run_fn(body: str, repo: Path) -> subprocess.CompletedProcess[str]:
    """Run the extracted function against `repo`.

    `PROJECT_ROOT` is `"."` and the working directory is set by Python, which is
    the one arrangement that needs no path translation at all. Three earlier
    spellings failed, every one of them looking like a defect in the function and
    being a defect in this suite:

    * `bash -c <multi-line string>` -- on Windows the argument goes through
      `CreateProcess`'s single command-line string and arrived as ONE line, so
      every `||` continuation and every argument became its own command. The tell
      was `/bin/bash: line 1: --absolute-git-dir: command not found`.
    * `input=<str>` with `text=True` -- Python translates `\\n` to `os.linesep`
      on the way to the child, so bash got CRLF and died on
      `syntax error near unexpected token $'{\\r'`.
    * a drive-letter `PROJECT_ROOT`, in both `C:/Users/...` and `/c/Users/...`
      form -- this MSYS `bash` can `cd` to neither, and every `cd` in the
      function failed with "No such file or directory", which reads exactly like
      a guard that does not work.

    Worth the paragraph because the pattern is the lesson: a test harness that
    cannot run the thing it tests reports the thing as broken, and each of those
    three reported between five and fifteen failures against code that was fine.
    """
    script = f'set -u\nPROJECT_ROOT="."\n{body}\ncommit_boot_history\n'
    # BYTES, not text. `text=True` makes Python translate every \n on the way to
    # the child's stdin into os.linesep, so on Windows bash received CRLF and
    # died on `syntax error near unexpected token $'{\r'`. Encoding here and
    # decoding the output by hand keeps the script exactly as written.
    done = subprocess.run(
        ["bash", "-s"],
        input=script.encode("utf-8"),
        capture_output=True,
        check=False,
        cwd=repo,
    )
    return subprocess.CompletedProcess(
        done.args,
        done.returncode,
        done.stdout.decode("utf-8", "replace"),
        done.stderr.decode("utf-8", "replace"),
    )


def append(repo: Path, row: str) -> None:
    with open(repo / LEDGER, "a", encoding="utf-8", newline="") as fh:
        fh.write(row + "\n")


def main() -> int:
    body = extract("commit_boot_history", BOOT.read_text(encoding="utf-8", errors="replace"))

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)

        print("group 1: the ordinary case")
        repo = make_repo(tmp)
        append(repo, '{"row": 2}')
        r = run_fn(body, repo)
        check("returns 0", r.returncode, 0)
        check("says it recorded the row", "recorded" in r.stdout + r.stderr, True)
        check("worktree is clean afterwards", git(repo, "status", "--porcelain"), "")
        check(
            "the commit holds only the ledger",
            git(repo, "show", "--name-only", "--format=", "HEAD"),
            LEDGER,
        )
        check("and names the branch in its subject",
              "on main" in git(repo, "log", "-1", "--format=%s"), True)

        print("group 2: nothing to do")
        before = git(repo, "rev-list", "--count", "HEAD")
        r = run_fn(body, repo)
        check("returns 0", r.returncode, 0)
        check(
            "makes no empty commit",
            git(repo, "rev-list", "--count", "HEAD"),
            before,
        )
        check("and says nothing", (r.stdout + r.stderr).strip(), "")

        print("group 3: other work is staged -- the case that matters")
        (repo / "other.txt").write_text("mine\n", encoding="utf-8", newline="")
        git(repo, "add", "other.txt")
        append(repo, '{"row": 3}')
        r = run_fn(body, repo)
        check("returns 0", r.returncode, 0)
        check(
            "commits the ledger alone",
            git(repo, "show", "--name-only", "--format=", "HEAD"),
            LEDGER,
        )
        check(
            "leaves the operator's work staged",
            git(repo, "diff", "--cached", "--name-only"),
            "other.txt",
        )
        git(repo, "commit", "--quiet", "-m", "other", "other.txt")

        print("group 4: a git operation is in progress")
        gitdir = Path(git(repo, "rev-parse", "--absolute-git-dir"))
        for marker, is_dir in (("MERGE_HEAD", False), ("rebase-merge", True)):
            append(repo, '{"row": 4}')
            target = gitdir / marker
            if is_dir:
                target.mkdir()
            else:
                target.write_text("x\n", encoding="utf-8", newline="")
            r = run_fn(body, repo)
            both = r.stdout + r.stderr
            check(f"{marker}: returns 0", r.returncode, 0)
            # The refusal must NAME the operation. git would refuse a pathspec
            # commit mid-merge on its own, so a test that only checked "the row
            # stayed dirty" would pass against a version whose guard never fires
            # and whose message says "could not commit" -- a true refusal with a
            # misleading reason. That is what this assertion caught in review.
            check(f"{marker}: names the operation", marker in both, True)
            check(
                f"{marker}: leaves the row uncommitted",
                git(repo, "status", "--porcelain", "--", LEDGER),
                f"M {LEDGER}",
            )
            if is_dir:
                target.rmdir()
            else:
                target.unlink()
        git(repo, "commit", "--quiet", "-m", "ledger", "--", LEDGER)

        print("group 5: not a git worktree")
        bare = tmp / "notgit"
        (bare / "bench").mkdir(parents=True)
        (bare / LEDGER).write_text('{"row": 9}\n', encoding="utf-8", newline="")
        r = run_fn(body, bare)
        check("returns 0", r.returncode, 0)
        check("silently", (r.stdout + r.stderr).strip(), "")

        print("group 6: the harness calls it")
        # Without this the function could be perfect and never invoked, which is
        # the same defect class as a gate nothing runs.
        text = BOOT.read_text(encoding="utf-8", errors="replace")
        calls = [
            ln for ln in text.splitlines()
            if ln.strip() == "commit_boot_history"
        ]
        check("boot-test.sh invokes it", len(calls) >= 1, True)

    if failures:
        print(f"\n{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("\nall checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
