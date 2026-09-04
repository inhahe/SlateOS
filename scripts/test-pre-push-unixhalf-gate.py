#!/usr/bin/env python3
"""Behavioural tests for pre-push gate 12 (coreutils' unix half).

Run: `python scripts/test-pre-push-unixhalf-gate.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory.

Why gate 12 needs a behavioural suite of its own
------------------------------------------------

`test-pre-push-gates.py` tests the hook's *shape*: that the header's count
matches the implemented gates, that each has its own bypass, that every gate
reading a tree tells its checker which one. Gate 12 satisfies all of that no
matter what it does at run time, because it has no `--selftest` to run and its
subject is a compiler rather than a script whose findings can be inspected.

And this is the gate whose whole correctness argument is a run-time property.
Every other gate answers a question about the commits being pushed, via
`--head`. Gate 12 cannot: cargo compiles files on disk, not a revision. It
therefore establishes that the disk *is* the push -- one ref, its sha equal to
HEAD, no modified tracked file, no untracked file -- and declines if it is not.
That guard is four `elif` arms of inline shell. Delete any one of them and the
hook still parses, still passes the structural suite, still reports "12 ran",
and now compiles a tree nobody is publishing while claiming to have judged the
one that is. Which is, precisely, the defect gate 7 shipped: "a gate can be
structurally impeccable and answer the wrong question."

So this suite drives the real hook, over real `git push` calls, and asserts on
whether the checker was *invoked* -- not merely on whether the push was allowed.
Those two come apart in exactly the cases that matter: a declined gate and a
passing gate both allow the push.

The checker is a stub
---------------------

The real `scripts/coreutils-check.sh` compiles a 124-binary crate through WSL
and takes minutes. The hook locates it at `$repo_root/scripts/coreutils-check.sh`,
so the fixture repository supplies its own, which records that it ran (into a
file outside the work tree, so it cannot itself become the untracked file case
5 is about) and exits with whatever code the case wants.

Stubbing is right here and not a compromise. What is under test is the gate's
decision to run the checker and its reading of the answer -- 0 allow, 1 refuse,
2 decline. What the real script does with those codes is its own concern, and
`coreutils-check.sh --help` documents them; a suite that ran the real thing
would spend six minutes to test the same four branches.

Why this suite scrubs its environment at import
-----------------------------------------------

It builds a git fixture in a temp directory, which is the exact shape of
program that on 2026-08-29 wrote `user.name selftest` into the shared config
of all three lane worktrees and put 33 permanently-misattributed commits on
origin. `gitenv.scrub_environ()` is what stops an inherited `GIT_DIR` from
outranking `cwd=<tmp>`. See `design-decisions.md` §637 and gate 10.
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

    `check=False`: a refused push is a thing under test, so a non-zero exit is
    data rather than an error.
    """
    return subprocess.run(
        ["git", *args], cwd=cwd, env=env or gitenv.clean_env(),
        capture_output=True, text=True, check=False,
    )


# The stub the fixture installs as `scripts/coreutils-check.sh`. It answers with
# `$UNIXHALF_STUB_RC` and records its invocation by appending to
# `$UNIXHALF_STUB_LOG`, which lives outside the work tree.
#
# The decline arm prints its reason to *stderr* and prints it first, because
# that is what `run_checker --may-skip` quotes back: four conditions must hold
# for a skip, and one of them is that the checker said something. A stub that
# exited 2 in silence would test the no-verdict path instead, and pass while
# looking like it had tested the skip.
STUB = """#!/usr/bin/env bash
set -eu
echo "$*" >> "$UNIXHALF_STUB_LOG"
if [ "${UNIXHALF_STUB_RC:-0}" = "2" ]; then
  echo "coreutils-check: no WSL on this host, so the unix half of these" >&2
  echo "  crates cannot be compiled here at all." >&2
  exit 2
fi
if [ "${UNIXHALF_STUB_RC:-0}" = "1" ]; then
  echo "error[E0599]: no method named \\`as_raw_fd\\` found" >&2
  exit 1
fi
echo ""
echo "=== summary ==="
echo "result:   clean (linux half checked)"
exit 0
"""


def build_fixture(tmp: str) -> tuple[str, str]:
    """A repo with the real hook installed, a stub checker, and a clean push.

    Returns `(work, stub_log)`.
    """
    remote = os.path.join(tmp, "remote.git")
    work = os.path.join(tmp, "w")
    stub_log = os.path.join(tmp, "stub-invocations.txt")
    git(tmp, "init", "--quiet", "--bare", remote)
    git(tmp, "init", "--quiet", "-b", "main", work)

    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    # No signing key exists here, and an inherited `commit.gpgsign=true` would
    # fail every commit below and be reported as a gate result.
    git(work, "config", "commit.gpgsign", "false")

    hooks = os.path.join(work, ".git", "hooks")
    os.makedirs(hooks, exist_ok=True)
    with open(HOOK, "r", encoding="utf-8", newline="") as src:
        body = src.read()
    dst = os.path.join(hooks, "pre-push")
    with open(dst, "w", encoding="utf-8", newline="") as out:
        out.write(body)
    os.chmod(dst, 0o755)

    scripts = os.path.join(work, "scripts")
    os.makedirs(scripts, exist_ok=True)
    # Copied, not stubbed: the gate's reading of exit code 2 *is*
    # `run_checker --may-skip`, so a stubbed library would test the stub.
    with open(LIB, "r", encoding="utf-8", newline="") as src:
        lib_body = src.read()
    with open(os.path.join(scripts, "run-checker.sh"), "w",
              encoding="utf-8", newline="") as out:
        out.write(lib_body)
    stub = os.path.join(scripts, "coreutils-check.sh")
    with open(stub, "w", encoding="utf-8", newline="\n") as out:
        out.write(STUB)
    os.chmod(stub, 0o755)

    git(work, "remote", "add", "origin", remote)
    # Both are *committed*, unlike the identity suite's untracked library. They
    # have to be: this gate declines on an untracked file, so a fixture that
    # left them lying loose would take the decline path in every case and the
    # suite would pass without ever running the checker.
    with open(os.path.join(work, "a.txt"), "w", encoding="utf-8",
              newline="") as fh:
        fh.write("one\n")
    git(work, "add", "a.txt", "scripts")
    git(work, "commit", "--quiet", "-m", "clean commit")
    # The seed push needs the stub's environment like every other push here:
    # this commit adds `scripts/coreutils-check.sh`, so it is in the gate's own
    # scope and the gate fires on it. Without the environment the stub dies on
    # `set -u`, the gate refuses, and -- the part worth naming, because it is
    # how this was found -- the seed commit stays *unpushed* and rides along in
    # the next case's push, silently widening that case's scope past what it is
    # about. A fixture whose setup is refused does not fail; it lies.
    env = gitenv.clean_env()
    env["UNIXHALF_STUB_RC"] = "0"
    env["UNIXHALF_STUB_LOG"] = stub_log
    seed = git(work, "push", "--quiet", "origin", "main", env=env)
    if seed.returncode != 0:
        raise RuntimeError("fixture seed push was refused:\n"
                           + seed.stdout + seed.stderr)
    return work, stub_log


def commit_file(work: str, path: str, body: str = "x\n") -> None:
    full = os.path.join(work, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "w", encoding="utf-8", newline="") as fh:
        fh.write(body)
    git(work, "add", path)
    git(work, "commit", "--quiet", "-m", f"commit adding {path}")


def push(work: str, stub_log: str, *, rc: str = "0", refspec: str = "main",
         extra: dict[str, str] | None = None) -> tuple[str, str, int]:
    """Push and report `(verdict, transcript, times the stub ran)`."""
    if os.path.exists(stub_log):
        os.remove(stub_log)
    env = gitenv.clean_env()
    env["UNIXHALF_STUB_RC"] = rc
    env["UNIXHALF_STUB_LOG"] = stub_log
    env.update(extra or {})
    proc = git(work, "push", "origin", refspec, env=env)
    blob = proc.stdout + proc.stderr
    runs = 0
    if os.path.exists(stub_log):
        with open(stub_log, encoding="utf-8") as fh:
            runs = len([ln for ln in fh if ln.strip()])
    verdict = "allowed" if proc.returncode == 0 else "refused"
    return verdict, blob, runs


def main() -> int:
    if _REMOVED:
        print(f"note: ignored inherited {', '.join(sorted(_REMOVED))}")
    if not os.path.isfile(HOOK):
        print(f"FAIL  the hook exists at {HOOK}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="unixhalf-gate-") as tmp:
        work, log = build_fixture(tmp)

        # --- 1. out of scope ------------------------------------------------
        # A push touching neither the crate nor the checker must not pay for a
        # compile. This is the scope claim in the gate's comment, and it is
        # what keeps the other two lanes out of it entirely.
        commit_file(work, "docs/note.txt")
        verdict, blob, runs = push(work, log)
        check("a push touching nothing relevant is allowed", verdict, "allowed")
        check("...and does not compile anything", runs, 0)
        check("...and the tally does not claim the gate ran",
              "coreutils-unix-half" in blob.split("ran:")[-1].split("skipped:")[0],
              False)

        # --- 2. in scope, clean, checker passes -----------------------------
        commit_file(work, "userspace/coreutils/note.txt")
        verdict, blob, runs = push(work, log)
        check("a push touching userspace/coreutils runs the checker", runs, 1)
        check("...with --only linux, so it is the unix half that is compiled",
              "--only linux" in open(log, encoding="utf-8").read()
              if os.path.exists(log) else False, True)
        check("...and a clean verdict allows the push", verdict, "allowed")

        # --- 3. the checker finds a broken cfg(unix) arm --------------------
        commit_file(work, "userspace/coreutils/note.txt", "two\n")
        verdict, blob, runs = push(work, log, rc="1")
        check("a failing unix half refuses the push", verdict, "refused")
        check("...naming its own bypass, so the author does not reach for "
              "--no-verify", "ALLOW_UNCHECKED_UNIX_HALF=1 git push" in blob, True)
        check("...and saying why the host build did not catch it",
              "cfg(unix)" in blob, True)

        # --- 4. no WSL on this host -----------------------------------------
        # Exit 2 is a decline, not a failure: refusing every push on a machine
        # without WSL is how a hook gets uninstalled. But it must not read as a
        # pass either, and it must be tallied as skipped even though
        # `run_checker --may-skip` returned 0 for it.
        verdict, blob, runs = push(work, log, rc="2")
        check("a checker that cannot run declines rather than refusing",
              verdict, "allowed")
        check("...and says so loudly", "SKIPPED coreutils-unix-half" in blob, True)
        check("...in the words that stop it reading as a pass",
              "This is not a pass." in blob, True)
        tally = blob.split("skipped:")[-1]
        check("...and the tally records it as skipped, not as having run",
              "coreutils-unix-half" in tally, True)

        # --- 5. a dirty working tree ----------------------------------------
        # The heart of it. Cargo compiles the directory; if the directory is
        # not the push, the gate must decline instead of answering about the
        # wrong tree. A gate missing this arm would report a pass here.
        commit_file(work, "userspace/coreutils/note.txt", "three\n")
        with open(os.path.join(work, "a.txt"), "w", encoding="utf-8",
                  newline="") as fh:
            fh.write("edited, not committed\n")
        verdict, blob, runs = push(work, log)
        check("an uncommitted edit makes the gate decline", runs, 0)
        check("...saying which condition failed",
              "uncommitted changes" in blob, True)
        check("...and does not block the push", verdict, "allowed")
        git(work, "checkout", "--", "a.txt")

        # --- 6. an untracked file -------------------------------------------
        # A new module file is a build input that is not in the commit, so the
        # compiler would read something the remote will never receive.
        commit_file(work, "userspace/coreutils/note.txt", "four\n")
        with open(os.path.join(work, "userspace", "coreutils", "extra.txt"),
                  "w", encoding="utf-8", newline="") as fh:
            fh.write("not committed\n")
        verdict, blob, runs = push(work, log)
        check("an untracked file makes the gate decline", runs, 0)
        check("...saying which condition failed", "untracked files" in blob, True)
        os.remove(os.path.join(work, "userspace", "coreutils", "extra.txt"))

        # --- 7. pushing a branch you are not standing on --------------------
        # `git push origin side:side` from `main` is ordinary, and it is the
        # exact shape that let `touches()` skip every gate before it was fixed.
        # Here the working tree is `main`, so compiling it would judge neither
        # the commits being sent nor anything the remote will see.
        verdict, blob, runs = push(work, log)   # drain: land case 6's commit
        git(work, "checkout", "--quiet", "-b", "side")
        commit_file(work, "userspace/coreutils/note.txt", "five\n")
        git(work, "checkout", "--quiet", "main")
        verdict, blob, runs = push(work, log, refspec="side:side")
        check("pushing a branch that is not checked out makes the gate decline",
              runs, 0)
        check("...saying which condition failed",
              "not the one" in blob and "checked out" in blob, True)
        check("...and does not block the push", verdict, "allowed")

        # --- 8. the bypass --------------------------------------------------
        # It must switch the *work* off, not merely the refusal: a bypass that
        # still paid for the compile would be one nobody used.
        git(work, "checkout", "--quiet", "side")
        commit_file(work, "userspace/coreutils/note.txt", "six\n")
        verdict, blob, runs = push(
            work, log, rc="1", refspec="side:side",
            extra={"ALLOW_UNCHECKED_UNIX_HALF": "1"})
        check("the bypass allows a push the checker would have refused",
              verdict, "allowed")
        check("...and does not compile anything", runs, 0)

    if failures:
        print(f"\n{len(failures)} check(s) failed:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print("\nall unix-half gate checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
