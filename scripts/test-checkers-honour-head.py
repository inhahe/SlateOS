#!/usr/bin/env python3
"""Do the push gates' checkers judge the commit, or whatever is on the disk?

Run: `python scripts/test-checkers-honour-head.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory.

Why this suite exists
---------------------

Seven push gates enumerate their files from the *pushed commit range* and then
read the contents off the *working tree* -- `known-issues.md` ->
`TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH`. That is
wrong in both directions at once, and one of the two is silent:

* **False pass.** A commit introduces the defect; the author then tidies the
  working tree without amending. The gate enumerates the commit, reads the
  tidied disk, finds nothing, and publishes the defect. Nobody is told.
* **False fail.** The working tree has an uncommitted defect; the commits being
  pushed are clean. The push is refused for code that is not being sent, and
  the author's only offered remedy is the gate's bypass -- which turns the gate
  off for the commits it *should* be judging.

`scripts/gittree.py`'s `Tree` seam is the fix: `--head <rev>` reads a revision,
its absence reads the disk, and the checker cannot tell which. This suite is
what stops that flag being decorative.

What it takes to test this, and why the obvious test is worthless
-----------------------------------------------------------------

A fixture where the commit and the disk *agree* proves nothing whatsoever: a
checker that ignores `--head` entirely passes it, and so does one that reads
the revision. The whole property lives in the disagreement. So every case here
builds a repository in which the commit and the working tree say different
things, and asserts that the verdict follows the flag rather than the disk.

Each case is therefore run **twice against the same fixture** -- once without
`--head` and once with it -- and the two must differ. An assertion that only
checks the `--head` run would stay green against a checker that had quietly
stopped reading the disk in the *other* mode, which is the mode the boot test
and every by-hand run use.

The fixture copies the checker and `gittree.py` into a throwaway repository, so
the checker's own `ROOT` (derived from `__file__`) is that repository and not
this one. Copying rather than importing is deliberate: these checkers are also
*executables* the hook invokes by path, and the thing under test is what that
invocation does.
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

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(HERE)

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}",
              file=sys.stderr)
        failures.append(label)


def git(cwd: str, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd, env=gitenv.clean_env(),
                          capture_output=True, text=True, check=False)


def write(root: str, rel: str, text: str) -> None:
    path = os.path.join(root, rel.replace("/", os.sep))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    # newline="" so the bytes on disk are the bytes asked for. A checker that
    # matches on `\n` would otherwise see `\r\n` on Windows in the working-tree
    # run and `\n` in the revision run, and the two arms would differ for a
    # reason that has nothing to do with the property under test.
    with open(path, "w", encoding="utf-8", newline="") as fh:
        fh.write(text)


def remove(root: str, rel: str) -> None:
    os.remove(os.path.join(root, rel.replace("/", os.sep)))


def new_repo(tmp: str, name: str, checkers: tuple[str, ...]) -> str:
    """A repository with `checkers` (and `gittree.py`) installed in `scripts/`."""
    root = os.path.join(tmp, name)
    os.makedirs(os.path.join(root, "scripts"))
    for script in ("gittree.py", *checkers):
        shutil.copy(os.path.join(HERE, script), os.path.join(root, "scripts", script))
    git(root, "init", "--quiet")
    git(root, "config", "user.email", "t@example.com")
    git(root, "config", "user.name", "t")
    return root


def commit(root: str, message: str = "c") -> str:
    git(root, "add", "-A")
    git(root, "commit", "--quiet", "-m", message)
    return git(root, "rev-parse", "HEAD").stdout.strip()


def run_checker(root: str, script: str, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, os.path.join(root, "scripts", script), *args],
        cwd=root, env=gitenv.clean_env(), capture_output=True, text=True,
        check=False,
    )


# --------------------------------------------------------------------------
# Gate 2 -- multicall-aliases.py
#
# The defect it looks for: a program that dispatches on the name it was invoked
# under, for a name nothing produces an executable for. One `Personality` arm is
# enough to make one, and removing the file removes it again, so the same
# fixture can be made to differ between the commit and the disk cheaply.
# --------------------------------------------------------------------------

_DISPATCH = '''\
fn main() {
    let arg0 = std::env::args().next().unwrap_or_default();
    match arg0.as_str() {
        "%s" => Personality::Ghost,
        _ => Personality::Real,
    };
}
'''

# The other dispatch shape the checker knows: one `Personality` arm carrying
# every spelling. Used where a case needs several aliases at once, so that one
# fixture can disagree with the disk about every *producer* kind at the same
# time.
_CHAIN_DISPATCH = '''\
enum Personality { Ghost, Real }

fn main() {
    let arg0 = std::env::args().next().unwrap_or_default();
    match arg0.as_str() {
        %s => Personality::Ghost,
        _ => Personality::Real,
    };
}
'''

_EMPTY_BASELINE = "# nothing known-unreachable yet\n"


def _alias_repo(tmp: str, name: str) -> str:
    root = new_repo(tmp, name, ("multicall-aliases.py",))
    write(root, "scripts/multicall-aliases-baseline.txt", _EMPTY_BASELINE)
    write(root, "userspace/real/Cargo.toml", '[package]\nname = "real"\n')
    write(root, "userspace/real/src/main.rs", "fn main() {}\n")
    return root


def case_gate2_a_tidied_worktree_cannot_hide_a_committed_alias(tmp: str) -> None:
    """The silent half: the commit has the defect, the disk no longer does."""
    root = _alias_repo(tmp, "g2a")
    write(root, "userspace/real/src/main.rs", _DISPATCH % "ghosttool")
    sha = commit(root)
    # The tidy-up that makes the disk lie: the dispatch is gone from the working
    # tree, but the commit about to be published still carries it.
    write(root, "userspace/real/src/main.rs", "fn main() {}\n")

    disk = run_checker(root, "multicall-aliases.py", "--check")
    rev = run_checker(root, "multicall-aliases.py", "--check", "--head", sha)
    check("gate 2: the disk sees nothing wrong", disk.returncode, 0)
    check("gate 2: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 2: ...naming the alias the commit introduced",
          "ghosttool" in rev.stdout + rev.stderr, True)


def case_gate2_an_uncommitted_alias_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: work in progress on the disk, nothing wrong in the commit."""
    root = _alias_repo(tmp, "g2b")
    sha = commit(root)
    write(root, "userspace/real/src/main.rs", _DISPATCH % "wipname")

    disk = run_checker(root, "multicall-aliases.py", "--check")
    rev = run_checker(root, "multicall-aliases.py", "--check", "--head", sha)
    check("gate 2: the disk refuses the uncommitted alias", disk.returncode, 1)
    check("gate 2: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate2_the_baseline_is_read_from_the_same_tree(tmp: str) -> None:
    """A waiver on the disk must not excuse a defect in the commit.

    The baseline is the checker's ratchet: an alias listed there is known and
    forgiven. Reading the code from the revision while reading the waiver list
    from the disk would mean an *uncommitted* line in the baseline silences a
    *committed* defect -- and the reviewer of the commit would see the alias
    added, the baseline not, and the gate green.
    """
    root = _alias_repo(tmp, "g2c")
    write(root, "userspace/real/src/main.rs", _DISPATCH % "ghosttool")
    sha_forgiven = commit(root)
    check("gate 2: the fixture starts refused",
          run_checker(root, "multicall-aliases.py", "--check",
                      "--head", sha_forgiven).returncode, 1)

    # Now forgive it in a *commit*, and un-forgive it on the *disk*.
    write(root, "scripts/multicall-aliases-baseline.txt",
          _EMPTY_BASELINE + "real:ghosttool\n")
    sha_forgiven = commit(root, "forgive")
    write(root, "scripts/multicall-aliases-baseline.txt", _EMPTY_BASELINE)

    disk = run_checker(root, "multicall-aliases.py", "--check")
    rev = run_checker(root, "multicall-aliases.py", "--check", "--head", sha_forgiven)
    check("gate 2: the disk's baseline still forgives nothing", disk.returncode, 1)
    check("gate 2: the commit forgives it, and is passed", rev.returncode, 0)


def case_gate2_a_missing_producer_directory_is_not_a_crash(tmp: str) -> None:
    """`userspace/coreutils/src/bin` does not exist in this fixture.

    On the disk that was a `FileNotFoundError` from `iterdir()` waiting to
    happen; through the seam an absent directory lists as nothing. Asserted
    because the alternative is exit 1 with a traceback, which
    `scripts/run-checker.sh` reads as "no verdict" and turns into a refused
    push naming a defect nobody has.
    """
    root = _alias_repo(tmp, "g2d")
    sha = commit(root)
    for label, proc in (("disk", run_checker(root, "multicall-aliases.py")),
                        ("rev", run_checker(root, "multicall-aliases.py",
                                            "--head", sha))):
        check(f"gate 2: the report runs with no coreutils bin dir ({label})",
              proc.returncode, 0)
        check(f"gate 2: ...without a traceback ({label})",
              "Traceback" in proc.stderr, False)


def case_gate2_every_producer_kind_is_read_from_the_tree(tmp: str) -> None:
    """Not just the dispatch -- every input the verdict depends on.

    Asserting only that the *dispatch* comes from the revision leaves the
    checker free to answer "is anything producing this name?" from the disk,
    and that half decides the verdict just as completely: an alias with a
    producer is reachable and passes. A checker reading the dispatch from the
    commit and the producers from the disk is still judging a tree that exists
    nowhere.

    So the commit here is *clean* -- every alias has a producer, one of each
    kind the checker knows -- and the disk has had all four producers deleted.
    Any producer read from the disk turns the passing commit into a refusal,
    and so does a producer kind that stops being recognised at all.
    """
    root = _alias_repo(tmp, "g2f")
    aliases = ("aliascrate", "aliascu", "aliascudir", "aliasstaged")
    write(root, "userspace/ghostcrate/src/main.rs",
          _CHAIN_DISPATCH % " | ".join(f'"{a}"' for a in aliases))
    # One producer of each kind the checker accepts.
    write(root, "userspace/aliascrate/Cargo.toml", '[package]\nname = "aliascrate"\n')
    write(root, "userspace/coreutils/src/bin/aliascu.rs", "fn main() {}\n")
    write(root, "userspace/coreutils/src/bin/aliascudir/main.rs", "fn main() {}\n")
    write(root, "scripts/create-ext4-rootfs.sh",
          '#!/bin/sh\ncp busybox "$root/bin/aliasstaged"\n')
    sha = commit(root)

    remove(root, "userspace/aliascrate/Cargo.toml")
    remove(root, "userspace/coreutils/src/bin/aliascu.rs")
    # The whole directory, not just the file inside it: a coreutils bin can be
    # a `<name>/` directory, and an emptied one is still a directory on the
    # disk. Git has no empty directories, so leaving it behind would make the
    # disk see a producer the revision does not -- a real difference between
    # the two trees, and the wrong one for this case to be about.
    shutil.rmtree(os.path.join(root, "userspace", "coreutils", "src", "bin",
                               "aliascudir"))
    remove(root, "scripts/create-ext4-rootfs.sh")

    disk = run_checker(root, "multicall-aliases.py", "--check")
    rev = run_checker(root, "multicall-aliases.py", "--check", "--head", sha)
    check("gate 2: the disk, missing every producer, refuses", disk.returncode, 1)
    check("gate 2: ...and the commit, which has them all, passes", rev.returncode, 0)
    # Which producers the disk complained about, so that a case passing for the
    # wrong reason -- one kind silently never recognised -- is visible.
    named = disk.stdout + disk.stderr
    for alias in aliases:
        check(f"gate 2: the disk run names {alias}", alias in named, True)


def case_gate2_a_crate_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The enumeration, not only the contents.

    The earlier cases edit a `main.rs` that exists on both sides, so a checker
    that lists crates from the disk and reads their text from the revision
    passes them. Here the whole crate directory is gone from the working tree
    -- the shape of a commit whose branch has since been cleaned, or of a file
    that only ever existed in the commit being pushed -- and its alias must
    still be judged.
    """
    root = _alias_repo(tmp, "g2g")
    write(root, "userspace/ghostcrate/src/main.rs", _DISPATCH % "ghostalias")
    sha = commit(root)
    shutil.rmtree(os.path.join(root, "userspace", "ghostcrate"))

    disk = run_checker(root, "multicall-aliases.py", "--check")
    rev = run_checker(root, "multicall-aliases.py", "--check", "--head", sha)
    check("gate 2: the disk has no such crate to judge", disk.returncode, 0)
    check("gate 2: the commit still has it, and is refused", rev.returncode, 1)
    check("gate 2: ...naming the alias of a crate the disk lacks",
          "ghostalias" in rev.stdout + rev.stderr, True)


# --------------------------------------------------------------------------
# The hook, not the checker.
#
# Everything above runs the checker directly, which leaves the seam between the
# hook and the checker untested -- and that seam is where the last two defects
# in these gates actually lived: gate 11 handed its scope to argv and died of a
# length limit before reading a file, and gate 7 read the disk while
# enumerating the commit. Neither is visible from either side alone.
#
# So this pushes for real, through the real hook, with a commit and a worktree
# that disagree. If `--head "$sha"` is ever dropped from the invocation, or the
# `$pushed_shas` loop iterates zero times, these two cases are what say so.
# --------------------------------------------------------------------------

HOOK = os.path.join(REPO_ROOT, "scripts", "hooks", "pre-push")
LIB = os.path.join(REPO_ROOT, "scripts", "run-checker.sh")


def _push_fixture(tmp: str, name: str) -> str:
    """A repository with a remote and the real hook and checker installed."""
    root = os.path.join(tmp, name)
    remote = os.path.join(root, "remote.git")
    work = os.path.join(root, "w")
    os.makedirs(root)
    git(root, "init", "--quiet", "--bare", remote)
    git(root, "init", "--quiet", "-b", "main", work)
    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    # An inherited `commit.gpgsign=true` would fail every commit below and
    # surface as a gate verdict rather than as what it is.
    git(work, "config", "commit.gpgsign", "false")

    hooks = os.path.join(work, ".git", "hooks")
    os.makedirs(hooks, exist_ok=True)
    for src, dst in ((HOOK, os.path.join(hooks, "pre-push")),
                     (LIB, os.path.join(work, "scripts", "run-checker.sh")),
                     (os.path.join(HERE, "multicall-aliases.py"),
                      os.path.join(work, "scripts", "multicall-aliases.py")),
                     (os.path.join(HERE, "gittree.py"),
                      os.path.join(work, "scripts", "gittree.py"))):
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        with open(src, encoding="utf-8", newline="") as fh:
            body = fh.read()
        with open(dst, "w", encoding="utf-8", newline="") as fh:
            fh.write(body)
    os.chmod(os.path.join(hooks, "pre-push"), 0o755)

    git(work, "remote", "add", "origin", remote)
    write(work, "scripts/multicall-aliases-baseline.txt", _EMPTY_BASELINE)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "install the gate")
    git(work, "push", "--quiet", "origin", "main")
    return work


def _push(work: str) -> tuple[str, str]:
    """(verdict, output). `allowed`, `refused`, or `error:<...>`.

    Only *gate 2's* refusal counts as `refused`. A suite that accepted any
    refusal would pass on a fixture that trips some other gate and never reach
    the thing it is about.

    `ALLOW_FMT_DRIFT=1` because the fixture's `.rs` files are hand-written and
    gate 7 would rustfmt them; `test-pre-push-fmt-gate.py` covers that gate
    properly. Nothing else is bypassed -- the other gates skip themselves
    because this fixture does not install their checkers.
    """
    env = gitenv.clean_env()
    env["ALLOW_FMT_DRIFT"] = "1"
    proc = subprocess.run(["git", "push", "origin", "main"], cwd=work, env=env,
                          capture_output=True, text=True, check=False)
    blob = proc.stdout + proc.stderr
    if proc.returncode == 0:
        return "allowed", blob
    # One line's worth: the refusal is a hand-wrapped paragraph, so a
    # whole-sentence probe never matches and a correct refusal would read as an
    # unrelated error.
    if "command name exists that nothing can run" in blob:
        return "refused", blob
    return "error:" + blob.strip().replace("\n", " | ")[:600], blob


def case_gate2_the_hook_refuses_a_commit_the_worktree_no_longer_shows(tmp: str) -> None:
    """End to end: the false pass, through the real hook.

    This is the shape that published two unformatted commits from gate 7 --
    commit the defect, tidy the worktree, push. The gate reads the tidy disk
    and approves what is actually being sent.
    """
    work = _push_fixture(tmp, "g2push-hide")
    write(work, "userspace/real/src/main.rs", _DISPATCH % "ghosttool")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "add a personality with no producer")
    write(work, "userspace/real/src/main.rs", "fn main() {}\n")

    verdict, blob = _push(work)
    check("gate 2 end to end: the push is refused", verdict, "refused")
    check("gate 2 end to end: ...naming the alias only the commit has",
          "ghosttool" in blob, True)


def case_gate2_the_hook_allows_a_clean_commit_under_a_dirty_worktree(tmp: str) -> None:
    """End to end: the false fail, through the real hook.

    Mirror of the case above, and the one that decides whether anyone keeps the
    gate switched on: a refusal here names code that is not being pushed, and
    the only remedy the message offers is the bypass -- which turns the gate off
    for the commits it should be judging.
    """
    work = _push_fixture(tmp, "g2push-wip")
    write(work, "userspace/real/src/main.rs", "fn main() {}\n")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a clean crate under userspace/")
    write(work, "userspace/real/src/main.rs", _DISPATCH % "wipname")

    verdict, blob = _push(work)
    check("gate 2 end to end: an uncommitted personality does not block",
          verdict, "allowed")
    # That it was *allowed* is not enough on its own: a gate that skipped
    # itself would also allow it, and would allow the case above too. The
    # hook's own tally is what distinguishes the two.
    check("gate 2 end to end: ...and the gate actually ran",
          "ran:" in blob and "unreachable-command" in blob.split("skipped:")[0],
          True)


def case_gate2_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1.

    `scripts/run-checker.sh` reads exit 1 as "the checker found something" and
    prints the gate's refusal text over it -- text that tells the author their
    code is wrong and offers the bypass. A revision that cannot be read is not
    a statement about anybody's code, so it must land in the no-verdict arm.
    """
    root = _alias_repo(tmp, "g2e")
    commit(root)
    proc = run_checker(root, "multicall-aliases.py", "--check", "--head", "nosuchrev")
    check("gate 2: an unreadable --head exits 2, not 1", proc.returncode, 2)


CASES = (
    case_gate2_a_tidied_worktree_cannot_hide_a_committed_alias,
    case_gate2_an_uncommitted_alias_does_not_block_a_clean_push,
    case_gate2_the_baseline_is_read_from_the_same_tree,
    case_gate2_a_missing_producer_directory_is_not_a_crash,
    case_gate2_every_producer_kind_is_read_from_the_tree,
    case_gate2_a_crate_absent_from_the_disk_is_still_judged,
    case_gate2_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate2_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate2_an_unopenable_revision_is_not_a_finding,
)


def main() -> int:
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes. Assert a floor, as the sibling suites do.
    if len(CASES) < 9:
        print(f"FATAL: only {len(CASES)} cases registered; the suite has at "
              f"least 9. The list is broken, not the code.")
        return 1
    # ...and at least one of them must go through the real hook. A floor on the
    # count alone would be met by nine direct-invocation cases, which is the
    # arrangement that left the hook->checker seam untested in the first place.
    end_to_end = [c for c in CASES if "the_hook" in c.__name__]
    if len(end_to_end) < 2:
        print(f"FATAL: {len(end_to_end)} end-to-end case(s) registered; the "
              f"suite has at least 2. The list is broken, not the code.")
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        for case in CASES:
            case(tmp)
    print()
    if failures:
        print(f"{len(failures)} FAILED: {', '.join(failures)}", file=sys.stderr)
        return 1
    print(f"all {len(CASES)} head-honouring cases passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
