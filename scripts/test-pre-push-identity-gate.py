#!/usr/bin/env python3
"""Behavioural tests for pre-push gate 10 (fixture-identity refusal).

Run: `python scripts/test-pre-push-identity-gate.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory.

Why this is a second suite next to `test-pre-push-gates.py`
----------------------------------------------------------

That one tests the hook's *shape* -- that the header's count matches the
implemented gates, that every gate has its own bypass -- and says so: "Not the
gates' logic; each gate's checker has its own `--selftest`."

Gate 10 has no checker. It is fifteen lines of inline shell and a regular
expression, so there is nowhere for a `--selftest` to live, and the structural
suite would pass just as happily if the regex matched nothing at all. That is
not hypothetical for this particular gate: its regex is anchored at both ends,
and dropping either anchor leaves something that still compiles, still runs,
and still reports success on every push -- while either accepting the address
it exists to reject or rejecting addresses real people hold.

So this suite drives the actual hook, in an actual repository, over an actual
`git push`, and asserts on what happens.

What gate 10 is for
-------------------

On 2026-08-29 a self-test built a throwaway repository and drove it with
`cwd=<tmp>`; an inherited `GIT_DIR` outranked that, so its
`git config user.name selftest` landed in the **shared** config that all three
lane worktrees read. Two consequences, 70 minutes apart:

* the fixture's own commits -- whose tree is the repository deleted -- were
  pushed to `origin/lane-a` and `origin/main`;
* 33 commits of real work from two lanes are permanently authored
  `selftest <selftest@example.invalid>`.

Neither is repairable: undoing published history across three active lanes
needs a force-push, which this project forbids. Prevention at the push boundary
is the only move available, which is what gate 10 is.

Why this suite scrubs its environment at import
-----------------------------------------------

Because it is the same kind of program that caused the incident: it builds a
git fixture in a temp directory. `gitenv.scrub_environ()` is not a precaution
copied from a checklist -- a suite testing the fixture-leak gate, which itself
leaked into the repository, would be a fine joke and a real outage. See
`design-decisions.md` §637.
"""

from __future__ import annotations

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

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}",
              file=sys.stderr)
        failures.append(label)


def git(cwd: str, *args: str, env: dict[str, str] | None = None
        ) -> subprocess.CompletedProcess:
    """Run git in `cwd`, never inheriting a repository binding.

    `check=False`: several calls here are *expected* to fail, since a refused
    push is the thing under test.
    """
    return subprocess.run(
        ["git", *args], cwd=cwd, env=env or gitenv.clean_env(),
        capture_output=True, text=True, check=False,
    )


def push_verdict(work: str, env: dict[str, str] | None = None) -> str:
    """`allowed`, `refused`, or `error:<...>` for a push of main."""
    proc = git(work, "push", "origin", "main", env=env)
    blob = proc.stdout + proc.stderr
    if proc.returncode == 0:
        return "allowed"
    if "REFUSING to push" in blob and "reserved-domain identity" in blob:
        return "refused"
    return "error:" + blob.strip().replace("\n", " | ")[:400]


def build_fixture(tmp: str) -> str:
    """A repo with the real hook installed and a clean first commit pushed."""
    remote = os.path.join(tmp, "remote.git")
    work = os.path.join(tmp, "w")
    git(tmp, "init", "--quiet", "--bare", remote)
    git(tmp, "init", "--quiet", "-b", "main", work)

    # A deliberately awkward *legitimate* address: it ends in `.org.uk`, so a
    # regex that tested for `example.org` as a substring rather than as the
    # whole domain would reject it. Someone owns example.org.uk; nobody owns
    # example.org.
    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    # The fixture has no signing key, and an inherited `commit.gpgsign=true`
    # would fail every commit below and be reported as a gate result.
    git(work, "config", "commit.gpgsign", "false")

    hooks = os.path.join(work, ".git", "hooks")
    os.makedirs(hooks, exist_ok=True)
    with open(HOOK, "r", encoding="utf-8", newline="") as src:
        body = src.read()
    dst = os.path.join(hooks, "pre-push")
    with open(dst, "w", encoding="utf-8", newline="") as out:
        out.write(body)
    os.chmod(dst, 0o755)

    # The hook sources `scripts/run-checker.sh`, so a checkout without one is a
    # checkout it refuses to push from -- by design, since without the library
    # no gate can tell a checker that found something from one that crashed.
    #
    # Copied in rather than stubbed, for the same reason the run-checker suite
    # cuts the real function out of the real file: a stub here would pass while
    # the shipped library rotted. It is written *untracked*, because the fixture
    # commits exactly the files each case is about and an extra one in the tree
    # would show up in the very `git rev-list` the gates read.
    #
    # This is also the arrangement that caught the lookup bug rather than hiding
    # it. The hook is installed above as a plain copy at `.git/hooks/pre-push`,
    # which is the shape whose `dirname`s do *not* reach `scripts/`; putting the
    # library where a real checkout has it means the fallback is what has to
    # find it, and the fixture fails if that fallback is removed again.
    os.makedirs(os.path.join(work, "scripts"), exist_ok=True)
    with open(LIB, "r", encoding="utf-8", newline="") as src:
        lib_body = src.read()
    with open(os.path.join(work, "scripts", "run-checker.sh"), "w",
              encoding="utf-8", newline="") as out:
        out.write(lib_body)

    git(work, "remote", "add", "origin", remote)
    with open(os.path.join(work, "a.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("one\n")
    git(work, "add", "a.txt")
    git(work, "commit", "--quiet", "-m", "clean commit")
    return work


def commit_as(work: str, path: str, *, author: str | None = None,
              committer: str | None = None) -> None:
    """Add `path` and commit it, optionally wearing a borrowed identity."""
    with open(os.path.join(work, path), "w", encoding="utf-8", newline="") as fh:
        fh.write(path + "\n")
    git(work, "add", path)
    args = ["commit", "--quiet", "-m", f"commit adding {path}"]
    env = gitenv.clean_env()
    if author:
        env["GIT_AUTHOR_NAME"] = "selftest"
        env["GIT_AUTHOR_EMAIL"] = author
    if committer:
        env["GIT_COMMITTER_NAME"] = "selftest"
        env["GIT_COMMITTER_EMAIL"] = committer
    git(work, *args, env=env)


def main() -> int:
    if _REMOVED:
        print(f"note: ignored inherited {', '.join(sorted(_REMOVED))}")
    if not os.path.isfile(HOOK):
        print(f"FAIL  hook not found at {HOOK}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="identgate-") as tmp:
        work = build_fixture(tmp)

        # The gate must be invisible to ordinary work. A gate that fires on a
        # normal push is one that gets bypassed by habit, and a bypass used by
        # habit is not a gate.
        check("a clean identity pushes", push_verdict(work), "allowed")

        # The incident itself: both fields poisoned by a leaked shared config.
        commit_as(work, "b.txt", author="selftest@example.invalid",
                  committer="selftest@example.invalid")
        check("a fixture identity is refused", push_verdict(work), "refused")

        # ...and can still be published deliberately, which is what keeps the
        # blocked author from reaching for `--no-verify` and turning off the
        # other nine gates too.
        bypass = gitenv.clean_env()
        bypass["ALLOW_FIXTURE_IDENTITY"] = "1"
        check("the bypass publishes it anyway",
              push_verdict(work, env=bypass), "allowed")

        # Author clean, committer poisoned -- the shape a merge or a rebase
        # takes. Checking only `%ae` would let this through, and merges to
        # `main` are precisely the commits the other two lanes inherit.
        commit_as(work, "c.txt", committer="selftest@example.invalid")
        check("a poisoned committer alone is refused",
              push_verdict(work), "refused")

        # The mirror image, for the same reason in the other direction.
        bypassed = push_verdict(work, env=bypass)
        check("bypass clears the committer case too", bypassed, "allowed")
        commit_as(work, "d.txt", author="selftest@example.invalid")
        check("a poisoned author alone is refused",
              push_verdict(work), "refused")
        push_verdict(work, env=bypass)

        # Addresses that merely *look* like the reserved ones. Each is a domain
        # a real party can register, and each defeats one plausible way of
        # writing the regex: a substring test, an unanchored suffix, or a
        # missing `@`.
        for i, addr in enumerate((
            "dev@invalid.example.co.uk",   # `invalid` as a label, not the TLD
            "dev@test.mycompany.com",      # `test` as a subdomain
            "not-a-test@really.co.uk",     # reserved word in the local part
            "dev@examples.org",            # `examples`, not `example`
        )):
            name = f"e{i}.txt"
            commit_as(work, name, author=addr, committer=addr)
            check(f"a real address is not mistaken for a fixture: {addr}",
                  push_verdict(work), "allowed")

    if failures:
        print(f"\n{len(failures)} FAILED: {', '.join(failures)}",
              file=sys.stderr)
        return 1
    print("\nall pre-push identity-gate tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
