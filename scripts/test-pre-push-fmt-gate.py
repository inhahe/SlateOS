#!/usr/bin/env python3
"""Behavioural tests for pre-push gate 7 (rustfmt drift), and specifically for
*which tree* it reads.

Run: `python scripts/test-pre-push-fmt-gate.py` (0 = pass, 1 = fail). No pytest
dependency, matching the other suites in this directory.

Why this suite exists
---------------------

`test-pre-push-gates.py` tests the hook's *shape* -- that the header's count
matches the implemented gates, that every gate has a bypass. It passes just as
happily whether gate 7 reads the bytes being published or the bytes on disk,
because both are a gate with a section and a bypass.

That distinction is not academic. Gate 7 originally enumerated the .rs files
from the pushed commit range and then read their contents from
`$repo_root/$file` -- the working tree. Those are the same bytes right up until
you commit and keep editing, at which point they are not, and on 2026-09-02
commits 861f4d80e and 09a436956 reached origin/lane-b unformatted for exactly
that reason: the working tree had been tidied after they were committed and
before the push ran, so the gate approved bytes it would have rejected.

The same confusion has a second face, a false *fail*: uncommitted drift in a
file the push happens to touch blocks a commit that is perfectly clean. The
gate's own refusal message promised this could not happen ("never complaining
about someone else's code"), and it could.

So the two cases below that actually matter are `false_pass` and `false_fail`.
Both are written to fail against the old implementation and pass against the
new one -- a green run against only the obvious cases (committed-clean passes,
committed-dirty is refused) is worth nothing here, because the old code passed
those too. That is the whole lesson of the bug.

Why it scrubs its environment at import
---------------------------------------

Same reason as `test-pre-push-identity-gate.py`: this builds a git fixture in a
temp directory, and on 2026-08-29 a suite that did that leaked `user.name
selftest` into the shared config of all three lane worktrees, permanently
misattributing 33 commits. `gitenv.scrub_environ()` is the fix. See
`design-decisions.md` §637.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gitenv  # noqa: E402

_REMOVED = gitenv.scrub_environ()

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HOOK = os.path.join(REPO_ROOT, "scripts", "hooks", "pre-push")
LIB = os.path.join(REPO_ROOT, "scripts", "run-checker.sh")

# Deliberately mangled: rustfmt reindents the body and collapses the blank run.
# Written as source rather than as a "here is the formatted version" pair so the
# expected output can be computed by the rustfmt actually installed, rather than
# pinned to whatever this machine had on the day the test was written.
UGLY = "fn main() {\nlet x=1;\n\n\n    println!(\"{x}\");\n}\n"

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}",
              file=sys.stderr)
        failures.append(label)


def git(cwd: str, *args: str) -> subprocess.CompletedProcess:
    """Run git in `cwd`, never inheriting a repository binding.

    `check=False`: a refused push is the thing under test, so several of these
    are expected to exit non-zero.
    """
    return subprocess.run(
        ["git", *args], cwd=cwd, env=gitenv.clean_env(),
        capture_output=True, text=True, check=False,
    )


def rustfmt_available() -> bool:
    if shutil.which("rustfmt") is None:
        return False
    proc = subprocess.run(["rustfmt", "--version"], capture_output=True,
                          text=True, check=False)
    return proc.returncode == 0


def pretty(text: str) -> str:
    """What rustfmt makes of `text` -- computed, not assumed."""
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "f.rs")
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
        subprocess.run(["rustfmt", "--edition", "2024", path],
                       capture_output=True, check=False)
        with open(path, "r", encoding="utf-8", newline="") as fh:
            return fh.read()


def write(work: str, rel: str, text: str) -> None:
    path = os.path.join(work, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as fh:
        fh.write(text)


def commit(work: str, rel: str, message: str) -> None:
    git(work, "add", "--", rel)
    git(work, "commit", "--quiet", "-m", message)


def push_verdict(work: str) -> str:
    """`allowed`, `refused`, or `error:<...>` for a push of main.

    Only gate 7's refusal counts as `refused`; anything else the hook rejects
    for is an `error`, because a test that accepted any refusal would pass on a
    fixture that trips gate 1 and never reach the formatting logic at all.
    """
    proc = git(work, "push", "origin", "main")
    blob = proc.stdout + proc.stderr
    if proc.returncode == 0:
        return "allowed"
    # Matched on one line's worth: the refusal text wraps between "rustfmt" and
    # "formats it", so the obvious whole-sentence probe never matches and every
    # correct refusal reads as an unrelated error.
    if "a file above is not formatted the way rustfmt" in blob:
        return "refused"
    return "error:" + blob.strip().replace("\n", " | ")[:400]


def build_fixture(tmp: str) -> str:
    """A repo with the real hook installed and one clean commit on origin."""
    os.makedirs(tmp, exist_ok=True)
    remote = os.path.join(tmp, "remote.git")
    work = os.path.join(tmp, "w")
    git(tmp, "init", "--quiet", "--bare", remote)
    git(tmp, "init", "--quiet", "-b", "main", work)
    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    # No signing key here; an inherited `commit.gpgsign=true` would fail every
    # commit below and surface as a gate result rather than as what it is.
    git(work, "config", "commit.gpgsign", "false")

    hooks = os.path.join(work, ".git", "hooks")
    os.makedirs(hooks, exist_ok=True)
    with open(HOOK, "r", encoding="utf-8", newline="") as src:
        body = src.read()
    dst = os.path.join(hooks, "pre-push")
    with open(dst, "w", encoding="utf-8", newline="") as out:
        out.write(body)
    os.chmod(dst, 0o755)

    # The hook sources `scripts/run-checker.sh` and refuses to push without it.
    # Copied in rather than stubbed, and written untracked, for the reasons
    # `test-pre-push-identity-gate.py` sets out at length.
    os.makedirs(os.path.join(work, "scripts"), exist_ok=True)
    with open(LIB, "r", encoding="utf-8", newline="") as src:
        lib_body = src.read()
    with open(os.path.join(work, "scripts", "run-checker.sh"), "w",
              encoding="utf-8", newline="") as out:
        out.write(lib_body)

    git(work, "remote", "add", "origin", remote)
    write(work, "a.txt", "one\n")
    commit(work, "a.txt", "clean commit")
    git(work, "push", "--quiet", "origin", "main")
    return work


# --------------------------------------------------------------------------
# The cases.
#
# Each builds its own fixture. Sharing one would let an earlier case's push
# move `origin/main`, and the pushed range every gate reads is defined against
# exactly that ref -- so a shared fixture would make the cases order-dependent
# in a way that is invisible until one is reordered.
# --------------------------------------------------------------------------

def case_committed_clean(tmp: str) -> None:
    """Baseline. Formatted and committed: nothing to complain about."""
    work = build_fixture(os.path.join(tmp, "clean"))
    write(work, "c/src/main.rs", pretty(UGLY))
    commit(work, "c/src/main.rs", "add a formatted file")
    check("a formatted commit pushes", push_verdict(work), "allowed")


def case_committed_dirty(tmp: str) -> None:
    """Baseline. Unformatted and committed: refused, as the gate always did."""
    work = build_fixture(os.path.join(tmp, "dirty"))
    write(work, "c/src/main.rs", UGLY)
    commit(work, "c/src/main.rs", "add an unformatted file")
    check("an unformatted commit is refused", push_verdict(work), "refused")


def case_false_pass(tmp: str) -> None:
    """The bug, in the shape that actually shipped.

    Commit unformatted bytes, then tidy the working tree without committing --
    the exact sequence that put 861f4d80e on origin. The published blob is
    still unformatted, so the push must still be refused. The old gate read the
    tidy working copy and allowed it.
    """
    work = build_fixture(os.path.join(tmp, "falsepass"))
    write(work, "c/src/main.rs", UGLY)
    commit(work, "c/src/main.rs", "add an unformatted file")
    write(work, "c/src/main.rs", pretty(UGLY))          # fixed, NOT committed
    check("an uncommitted fix does not launder the commit",
          push_verdict(work), "refused")


def case_false_fail(tmp: str) -> None:
    """The same confusion, mirrored.

    The commit is clean; only the working tree is messy, and that mess is not
    being published. Refusing here breaks the ordinary commit-then-keep-editing
    workflow and contradicts the gate's own refusal text.
    """
    work = build_fixture(os.path.join(tmp, "falsefail"))
    write(work, "c/src/main.rs", pretty(UGLY))
    commit(work, "c/src/main.rs", "add a formatted file")
    write(work, "c/src/main.rs", UGLY)                  # drift, NOT committed
    check("uncommitted drift does not block a clean commit",
          push_verdict(work), "allowed")


def case_untouched_submodule(tmp: str) -> None:
    """A module root must not drag its untouched children into the verdict.

    `child.rs` is unformatted and already on origin, so it is not in this push.
    Handing rustfmt the module root resolves and checks every child, which is
    how an untouched file ends up failing someone else's push; the stubs the
    mirror seeds are what stop it.
    """
    work = build_fixture(os.path.join(tmp, "submodule"))
    write(work, "c/src/child.rs", UGLY)
    write(work, "c/src/lib.rs", "pub mod child;\n")
    git(work, "add", "--", "c/src/child.rs", "c/src/lib.rs")
    git(work, "commit", "--quiet", "-m", "seed an unformatted child")
    # Published with the bypass: the point is to get it onto origin, not to
    # test that the gate would have caught it -- `case_committed_dirty` does
    # that. Without this it is in the next push's range and legitimately fails.
    env = gitenv.clean_env()
    env["ALLOW_FMT_DRIFT"] = "1"
    subprocess.run(["git", "push", "--quiet", "origin", "main"], cwd=work,
                   env=env, capture_output=True, text=True, check=False)

    write(work, "c/src/lib.rs", "//! Root.\n\npub mod child;\n")
    commit(work, "c/src/lib.rs", "touch only the module root")
    check("an untouched submodule does not fail the root's push",
          push_verdict(work), "allowed")


def case_added_then_deleted(tmp: str) -> None:
    """A file added and removed inside one push range has no bytes to check.

    `--diff-filter=ACMR` still lists it, so something has to drop it. The old
    code dropped it with `[ -f ]` against the working tree, which worked by
    coincidence -- the file is absent there too. Reading the tip has to reach
    the same answer deliberately.
    """
    work = build_fixture(os.path.join(tmp, "transient"))
    write(work, "c/src/gone.rs", UGLY)
    commit(work, "c/src/gone.rs", "add a file")
    git(work, "rm", "--quiet", "--", "c/src/gone.rs")
    git(work, "commit", "--quiet", "-m", "and remove it again")
    check("a file added then deleted in the same push is skipped",
          push_verdict(work), "allowed")


def case_bypass(tmp: str) -> None:
    """The documented escape hatch still works on the new code path."""
    work = build_fixture(os.path.join(tmp, "bypass"))
    write(work, "c/src/main.rs", UGLY)
    commit(work, "c/src/main.rs", "add an unformatted file")
    env = gitenv.clean_env()
    env["ALLOW_FMT_DRIFT"] = "1"
    proc = subprocess.run(["git", "push", "origin", "main"], cwd=work,
                          env=env, capture_output=True, text=True, check=False)
    check("ALLOW_FMT_DRIFT=1 publishes it anyway", proc.returncode, 0)


def main() -> int:
    if not rustfmt_available():
        # The gate itself skips without rustfmt, so there is nothing to assert.
        # Reported rather than silently passing: a suite that prints "all
        # passed" having run nothing is worse than no suite.
        print("SKIP  rustfmt is not on PATH — gate 7 skips too, nothing to test")
        return 0

    with tempfile.TemporaryDirectory() as tmp:
        for case in (case_committed_clean, case_committed_dirty,
                     case_false_pass, case_false_fail,
                     case_untouched_submodule, case_added_then_deleted,
                     case_bypass):
            case(tmp)

    if failures:
        print(f"\n{len(failures)} pre-push fmt-gate test(s) failed:",
              file=sys.stderr)
        for name in failures:
            print(f"  - {name}", file=sys.stderr)
        return 1
    print("\nall pre-push fmt-gate tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
