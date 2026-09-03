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
# Gate 3 -- raced-globals.py
#
# The defect it looks for: a mutable process-global that two or more `#[test]`
# functions reach with no lock between them. libtest runs tests concurrently, so
# whichever one asserts on the value is asserting on what the other thread left.
#
# Four separate inputs decide its verdict, and each one has to be pinned
# separately -- the lesson gate 2 taught, where a first draft asserted only that
# the *source* came from the revision and left the checker free to answer the
# other half of the question from the disk. Here the four are: the `.rs` source,
# the baseline that forgives, the `Cargo.toml` that decides whether the crate's
# tests can run at all, and the set of files that exist. One case each.
# --------------------------------------------------------------------------

# A resettable atomic reached by two unserialised tests: `static NAME: Atomic*`
# is one of the two declaration shapes the checker matches, `.store(` is what
# makes it a *reset* rather than a monotonic counter (which is safe to share and
# deliberately not reported), and neither test body carries a lock hint.
_RACED = '''\
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn first() {
    COUNTER.store(1, Ordering::SeqCst);
    assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
}

#[test]
fn second() {
    COUNTER.store(2, Ordering::SeqCst);
    assert_eq!(COUNTER.load(Ordering::SeqCst), 2);
}
'''

_UNRACED = "pub fn nothing_to_see() {}\n"

_RG_BASELINE = "# nothing baselined yet\n"

# The key the baseline is written in: "<relpath>:<NAME>".
_RACE_KEY = "posix/src/race.rs:COUNTER"

# A manifest that positively demonstrates the crate has no target left for a
# test harness to attach to, which is the checker's one route to silencing a
# whole crate. Every clause is load-bearing: a `[lib]` that is merely absent is
# autodiscovered from `src/lib.rs` and testable, and a `[[bin]]` list that is
# absent means any binary present was autodiscovered and is testable too.
_NO_TEST_TARGET = '''\
[package]
name = "p"

[lib]
test = false

[[bin]]
name = "p"
path = "src/bin_p.rs"
test = false
'''

_PLAIN_MANIFEST = '[package]\nname = "p"\n'

# The same, minus the `[lib]` table: with nothing said about a library target,
# `src/lib.rs` is autodiscovered, which is a *fifth* thing the crate's
# testability can turn on and so a fifth file whose existence must be read from
# the tree under judgement.
_NO_LIB_TABLE = '''\
[package]
name = "p"

[[bin]]
name = "p"
path = "src/bin_p.rs"
test = false
'''


def _raced_repo(tmp: str, name: str, manifest: str = _PLAIN_MANIFEST,
                with_lib: bool = True) -> str:
    """A one-crate repository with the checker installed and nothing raced yet.

    `src/bin_p.rs` rather than `src/bin/p.rs` on purpose: the checker treats a
    `src/bin/` *directory* as an autodiscovered target that keeps the crate
    testable, which would make the manifest cases below unable to silence
    anything.

    Every fixture carries a raced file under `src/vendor/`, committed, in both
    trees. It is not part of any case's argument: it is there so that the
    checker's own `SKIP_DIRS` has to be doing its job on *both* sides of the
    seam for any case to pass. Handing that list to `files_under(prune=...)`
    rather than filtering the results is what keeps the disk side from
    descending a vendored tree in order to discard it, and this is what notices
    if the list stops being handed over at all -- on the disk, where it costs
    time, or in a revision, where it would start reporting races in code we did
    not write.
    """
    root = new_repo(tmp, name, ("raced-globals.py",))
    write(root, "scripts/raced-globals-baseline.txt", _RG_BASELINE)
    write(root, "posix/Cargo.toml", manifest)
    if with_lib:
        write(root, "posix/src/lib.rs", "pub fn ok() {}\n")
    write(root, "posix/src/bin_p.rs", "fn main() {}\n")
    write(root, "posix/src/vendor/dep.rs", _RACED)
    return root


def case_gate3_a_tidied_worktree_cannot_hide_a_committed_race(tmp: str) -> None:
    """The silent half: the race is in the commit, and no longer on the disk."""
    root = _raced_repo(tmp, "g3a")
    write(root, "posix/src/race.rs", _RACED)
    sha = commit(root)
    write(root, "posix/src/race.rs", _UNRACED)

    disk = run_checker(root, "raced-globals.py", "--check")
    rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
    check("gate 3: the disk sees nothing raced", disk.returncode, 0)
    check("gate 3: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 3: ...naming the global the commit races",
          "COUNTER" in rev.stdout + rev.stderr, True)


def case_gate3_an_uncommitted_race_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: an experiment on the disk, nothing raced in the commit."""
    root = _raced_repo(tmp, "g3b")
    sha = commit(root)
    write(root, "posix/src/race.rs", _RACED)

    disk = run_checker(root, "raced-globals.py", "--check")
    rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
    check("gate 3: the disk refuses the uncommitted race", disk.returncode, 1)
    check("gate 3: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate3_the_baseline_is_read_from_the_same_tree(tmp: str) -> None:
    """The ratchet must not be loosened by a line nobody is publishing.

    The baseline is itself a tracked, pushed file. Reading the source from the
    revision while reading the waiver list from the disk would mean an
    *uncommitted* line excuses a *committed* race -- and the reviewer would see
    the race added, the baseline untouched, and the gate green.
    """
    root = _raced_repo(tmp, "g3c")
    write(root, "posix/src/race.rs", _RACED)
    sha = commit(root)
    check("gate 3: the fixture starts refused",
          run_checker(root, "raced-globals.py", "--check",
                      "--head", sha).returncode, 1)

    # Forgive it in a *commit*, and un-forgive it on the *disk*.
    write(root, "scripts/raced-globals-baseline.txt",
          _RG_BASELINE + _RACE_KEY + "\n")
    sha = commit(root, "baseline it")
    write(root, "scripts/raced-globals-baseline.txt", _RG_BASELINE)

    disk = run_checker(root, "raced-globals.py", "--check")
    rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
    check("gate 3: the disk's baseline forgives nothing", disk.returncode, 1)
    check("gate 3: the commit's baseline forgives it, and passes",
          rev.returncode, 0)


def case_gate3_the_manifest_is_read_from_the_same_tree(tmp: str) -> None:
    """The third input, and the one that can silence an entire crate.

    Before reporting anything the checker asks whether `cargo test` builds a
    target for the crate at all: `#[test]`s in a crate with no test target never
    execute, and tests that cannot execute cannot interleave. That answer comes
    from `Cargo.toml`. So a checker reading the source from the revision and the
    manifest from the disk can be handed an uncommitted `test = false` and drop
    a committed race on the floor -- with no mention of the race in its output,
    because the whole crate is accounted for elsewhere.

    The race is identical in both trees here; only the manifest differs.
    """
    root = _raced_repo(tmp, "g3d")
    write(root, "posix/src/race.rs", _RACED)
    sha = commit(root)
    write(root, "posix/Cargo.toml", _NO_TEST_TARGET)

    disk = run_checker(root, "raced-globals.py", "--check")
    rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
    check("gate 3: the disk's manifest silences the crate", disk.returncode, 0)
    # ...for the stated reason, and not because the race stopped being detected
    # for some unrelated reason -- which would pass this case while proving
    # nothing about where the manifest was read from.
    check("gate 3: ...saying so, rather than merely finding nothing",
          "no test target" in disk.stdout, True)
    check("gate 3: the commit's manifest leaves it testable, and is refused",
          rev.returncode, 1)
    check("gate 3: ...naming the global", "COUNTER" in rev.stdout + rev.stderr,
          True)


def case_gate3_a_file_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The fourth input: the enumeration, not only the contents.

    The cases above all edit a file that exists on both sides, so a checker
    listing `.rs` files from the disk and reading their text from the revision
    passes every one of them. Here the raced file is gone from the working tree
    entirely -- a commit on a branch since cleaned up, or a file that only ever
    existed in what is being pushed.
    """
    root = _raced_repo(tmp, "g3e")
    write(root, "posix/src/race.rs", _RACED)
    sha = commit(root)
    remove(root, "posix/src/race.rs")

    disk = run_checker(root, "raced-globals.py", "--check")
    rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
    check("gate 3: the disk has no such file to judge", disk.returncode, 0)
    check("gate 3: the commit still has it, and is refused", rev.returncode, 1)
    check("gate 3: ...naming the global in a file the disk lacks",
          "COUNTER" in rev.stdout + rev.stderr, True)


def case_gate3_the_crate_boundary_is_found_in_the_same_tree(tmp: str) -> None:
    """Where the crate *starts* is itself read from a tree.

    Before it can ask whether a crate's tests run, the checker has to decide
    which crate a file belongs to, by walking up looking for a `Cargo.toml`.
    Reading that from the disk gives an answer about a crate layout that is not
    being pushed -- and the two answers are not close: no manifest at all means
    no crate, which means the "can these tests even run?" question is never
    asked and every race is reported.
    """
    root = _raced_repo(tmp, "g3g", manifest=_NO_TEST_TARGET)
    write(root, "posix/src/race.rs", _RACED)
    sha = commit(root)
    remove(root, "posix/Cargo.toml")

    disk = run_checker(root, "raced-globals.py", "--check")
    rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
    check("gate 3: with no manifest on the disk the race is reported",
          disk.returncode, 1)
    check("gate 3: the commit's manifest silences the crate", rev.returncode, 0)
    check("gate 3: ...for the stated reason", "no test target" in rev.stdout,
          True)


def case_gate3_the_silenced_crate_report_describes_the_commit(tmp: str) -> None:
    """A crate falling silent is a finding, and it too must be about the push.

    `#[test]`s in a crate `cargo test` builds no target for never run and are
    never even type-checked. The checker counts them and says so -- separately
    from the races, because tests that cannot execute cannot interleave. That
    count is the one output here that no exit code depends on, which is exactly
    why it needs its own case: a checker counting them off the disk reports a
    number about a tree nobody is publishing, and both runs still exit 0.
    """
    root = _raced_repo(tmp, "g3h", manifest=_NO_TEST_TARGET)
    write(root, "posix/src/onlycommit.rs",
          "#[test]\nfn t() {\n    assert!(true);\n}\n")
    sha = commit(root)
    remove(root, "posix/src/onlycommit.rs")

    disk = run_checker(root, "raced-globals.py", "--check")
    rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
    check("gate 3: neither tree is refused", (disk.returncode, rev.returncode),
          (0, 0))
    check("gate 3: the disk has no unrunnable test to report",
          "no test target" in disk.stdout, False)
    check("gate 3: the commit does, and it is reported",
          "no test target" in rev.stdout, True)


def case_gate3_every_test_target_probe_reads_the_same_tree(tmp: str) -> None:
    """Four files, any one of which keeps a crate's tests runnable.

    `crate_has_test_target` is a chain of short-circuiting probes, so no single
    fixture can exercise more than one of them: the first that finds a target
    answers, and the rest are never reached. Each therefore gets its own tiny
    repository, differing from its sibling only in which file the disk has that
    the commit does not.

    They are worth pinning individually because each is a whole-crate silencer
    working in the *false pass* direction: a probe answered from the disk says
    "these tests do run" about a commit in which they do not, or the reverse,
    and either way the crate's races are decided by a file that is not being
    pushed.
    """
    probes = (
        # (label, manifest, has a committed src/lib.rs, the disk-only file)
        ("an autodiscovered library", _NO_LIB_TABLE, False, "posix/src/lib.rs"),
        ("an integration test", _NO_TEST_TARGET, True, "posix/tests/it.rs"),
        ("an autodiscovered binary", _NO_TEST_TARGET, True, "posix/src/main.rs"),
        ("a src/bin binary", _NO_TEST_TARGET, True, "posix/src/bin/extra.rs"),
    )
    for i, (label, manifest, with_lib, only_on_disk) in enumerate(probes):
        root = _raced_repo(tmp, f"g3probe{i}", manifest=manifest,
                           with_lib=with_lib)
        write(root, "posix/src/race.rs", _RACED)
        sha = commit(root)
        # The file exists only on the disk, so only the disk's crate has a
        # target for a harness to attach to -- and only the disk's copy of the
        # race is therefore reachable by a test that runs.
        write(root, only_on_disk, "pub fn extra() {}\n")

        disk = run_checker(root, "raced-globals.py", "--check")
        rev = run_checker(root, "raced-globals.py", "--check", "--head", sha)
        check(f"gate 3: {label} on the disk makes the disk's crate testable",
              disk.returncode, 1)
        check(f"gate 3: ...and the commit, lacking {label}, is silenced",
              rev.returncode, 0)


def case_gate3_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1 -- the same contract gate 2 has with run-checker.sh.

    Exit 1 means "the checker found something" and gets the gate's refusal text
    printed over it, telling the author their tests race and offering the
    bypass. A revision that cannot be read says nothing about anybody's tests.
    """
    root = _raced_repo(tmp, "g3f")
    commit(root)
    proc = run_checker(root, "raced-globals.py", "--check", "--head", "nosuchrev")
    check("gate 3: an unreadable --head exits 2, not 1", proc.returncode, 2)


# --------------------------------------------------------------------------
# Gate 4 -- argv-utf8.py
#
# The defect it looks for: `std::env::args()`, whose iterator is a documented
# `unwrap`, so the utility panics before its own first statement on a filename
# holding a byte that is not UTF-8. One line makes one, and changing that line
# to `args_os()` unmakes it, so a fixture can disagree with the disk about a
# finding in a single edit.
#
# What it reads from a tree, and therefore what has to be made to differ: the
# `.rs` sources under `userspace/coreutils`, *which* `.rs` files are there at
# all, the baseline, and -- outside `--check` -- the survey of everything else
# under `userspace/`. Four inputs, four cases; a checker converted for the
# first three and left reading the disk for the fourth would pass a suite that
# only tested contents.
#
# The checker loads `strip_comments_and_strings` out of `raced-globals.py`
# through `srcload.py`, so both are installed alongside it. In the end-to-end
# fixtures that also switches gate 3 on, which is harmless -- these fixtures
# have no `static mut` and nothing raced -- and is why `_push` is told which
# refusal it is looking for.
# --------------------------------------------------------------------------

_G4_CHECKERS = ("argv-utf8.py", "raced-globals.py", "srcload.py")

_ARGV_PANIC = '''\
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let _ = args;
}
'''

_ARGV_OK = '''\
fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let _ = args;
}
'''

_AU_BASELINE = "# nothing known-panicking yet\n"

# The finding the fixtures below argue about, spelled the way the baseline
# spells it: `<path>:<rule>`.
_AU_KEY = "userspace/coreutils/src/bin/tool.rs:argv-as-string"


def _argv_repo(tmp: str, name: str) -> str:
    """A repository with the gated tree present and nothing panicking in it.

    `userspace/coreutils/src/bin/clean.rs` is in every fixture, committed, in
    both trees. It is not part of any case's argument -- it is there so that
    "the checker found nothing" can never be reached by the checker finding no
    *files*, which is the way this gate would fail silently if the enumeration
    were ever pointed at the wrong tree or the wrong prefix.
    """
    root = new_repo(tmp, name, _G4_CHECKERS)
    write(root, "scripts/argv-utf8-baseline.txt", _AU_BASELINE)
    write(root, "userspace/coreutils/src/bin/clean.rs", _ARGV_OK)
    return root


def case_gate4_a_tidied_worktree_cannot_hide_a_committed_panic(tmp: str) -> None:
    """The silent half: the commit panics on a legal filename, the disk does not."""
    root = _argv_repo(tmp, "g4a")
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)
    sha = commit(root)
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_OK)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk sees nothing that panics", disk.returncode, 0)
    check("gate 4: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 4: ...naming the rule the commit breaks",
          "argv-as-string" in rev.stdout + rev.stderr, True)


def case_gate4_an_uncommitted_panic_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: a half-converted bin on the disk, nothing wrong in the commit."""
    root = _argv_repo(tmp, "g4b")
    sha = commit(root)
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk refuses the uncommitted panic", disk.returncode, 1)
    check("gate 4: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate4_the_baseline_is_read_from_the_same_tree(tmp: str) -> None:
    """The ratchet must not be loosened by a line nobody is publishing.

    Sharper here than in gates 2 and 3, because this baseline is 49 entries of
    real backlog that authors edit routinely: reading it off the disk means an
    *uncommitted* waiver excuses a *committed* panic, the reviewer sees the
    panicking bin added and the baseline untouched, and the gate is green. It
    then stays green on every later push, because by then the waiver is
    committed too -- so the one push where the two disagree is the only chance
    anyone had to notice.
    """
    root = _argv_repo(tmp, "g4c")
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)
    sha = commit(root)
    check("gate 4: the fixture starts refused",
          run_checker(root, "argv-utf8.py", "--check",
                      "--head", sha).returncode, 1)

    # Forgive it in a *commit*, and un-forgive it on the *disk*.
    write(root, "scripts/argv-utf8-baseline.txt", _AU_BASELINE + _AU_KEY + "\n")
    sha = commit(root, "baseline it")
    write(root, "scripts/argv-utf8-baseline.txt", _AU_BASELINE)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk's baseline forgives nothing", disk.returncode, 1)
    check("gate 4: the commit's baseline forgives it, and passes",
          rev.returncode, 0)


def case_gate4_a_file_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The third input: the enumeration, not only the contents.

    The cases above all edit a file that exists on both sides, so a checker
    listing `.rs` files from the disk and reading their text from the revision
    passes every one of them. Here the panicking bin is gone from the working
    tree entirely -- a branch since cleaned up, or a file that exists only in
    what is being pushed.
    """
    root = _argv_repo(tmp, "g4d")
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)
    sha = commit(root)
    remove(root, "userspace/coreutils/src/bin/tool.rs")

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk has no such file to judge", disk.returncode, 0)
    check("gate 4: the commit still has it, and is refused", rev.returncode, 1)
    check("gate 4: ...naming the file the disk lacks",
          "tool.rs" in rev.stdout + rev.stderr, True)


def case_gate4_the_stale_half_of_the_ratchet_describes_the_commit(tmp: str) -> None:
    """A ratchet that does not shrink is a standing permission to regress.

    `--check` fails on a baseline line naming a finding that is no longer
    there, not only on a finding that is not in the baseline -- the sibling
    ratchet accumulated 17 dead lines before that guard existed. It is the
    *other* direction through the same two inputs, and it is the direction a
    conversion is likeliest to leave behind, because every case above is
    satisfied by getting `new` right and none of them touches `stale`.

    Here the fix is committed and the disk still has the defect: the commit's
    baseline line is dead and must be reported as such, while the disk's is
    live and must not be.
    """
    root = _argv_repo(tmp, "g4i")
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)
    write(root, "scripts/argv-utf8-baseline.txt", _AU_BASELINE + _AU_KEY + "\n")
    commit(root, "baselined backlog")
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_OK)
    sha = commit(root, "fix the bin, leave the baseline")
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk's baseline line is still earning its keep",
          disk.returncode, 0)
    check("gate 4: the commit's is dead, and the push is refused",
          rev.returncode, 1)
    # For the stated reason, and not because something else went wrong: a bare
    # exit 1 here is also what an unreadable revision or a crash would produce.
    check("gate 4: ...reported as fixed rather than as new",
          "FIXED " + _AU_KEY in rev.stdout, True)
    check("gate 4: ...and the disk reports nothing fixed",
          "FIXED" in disk.stdout, False)


def case_gate4_the_ungated_survey_reads_the_same_tree(tmp: str) -> None:
    """The fourth input, and the one no exit code depends on.

    A bare run also counts the findings under `userspace/` that the gate does
    *not* cover, so that the excluded scope is a number someone can argue with
    rather than a silence. Nothing fails on it, which is exactly why it needs
    its own case: a survey walking the disk while the gate walks the revision
    prints a number about a tree nobody is publishing, and both runs still exit
    0. The bare run is what a human uses, so a wrong number here is a wrong
    number in the only place anyone reads it.
    """
    root = _argv_repo(tmp, "g4j")
    write(root, "userspace/stub/src/main.rs", _ARGV_PANIC)
    sha = commit(root)
    remove(root, "userspace/stub/src/main.rs")

    disk = run_checker(root, "argv-utf8.py")
    rev = run_checker(root, "argv-utf8.py", "--head", sha)
    check("gate 4: neither tree is refused", (disk.returncode, rev.returncode),
          (0, 0))
    check("gate 4: the disk has nothing outside the gate to report",
          "outside the gate" in disk.stdout, False)
    check("gate 4: the commit does, and it is counted",
          "1 finding(s) in 1 file(s) under userspace, outside the gate"
          in disk.stdout + rev.stdout, True)


def case_gate4_build_output_is_skipped_on_the_side_that_can_see_it(tmp: str) -> None:
    """The rule that only the disk arm can break, so only it can pin.

    A revision never lists `target/` -- it is not tracked -- so the skip is
    unobservable there and this case is about the working-tree arm alone. That
    arm used to hand-roll its own walk with its own copy of the skip rule; the
    conversion deleted it and delegates to `gittree`. If that delegation is
    ever dropped for a plain `os.walk`, the checker starts reporting generated
    sources it did not write, in a directory of tens of gigabytes, and the gate
    becomes something people switch off rather than something they fix.

    The control matters as much as the case: identical content one directory
    across must still be found, or "nothing reported" would be met by a
    checker that had stopped looking at the disk altogether.
    """
    root = _argv_repo(tmp, "g4k")
    sha = commit(root)
    # After the commit, so it is on the disk and in no revision -- which is
    # what build output is.
    write(root, "userspace/coreutils/target/debug/build/gen.rs", _ARGV_PANIC)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: generated sources under target/ are not judged",
          (disk.returncode, rev.returncode), (0, 0))

    write(root, "userspace/coreutils/notes/gen.rs", _ARGV_PANIC)
    disk2 = run_checker(root, "argv-utf8.py", "--check")
    check("gate 4: ...while the same bytes elsewhere on the disk are",
          disk2.returncode, 1)


def case_gate4_a_tree_with_no_gated_sources_is_not_a_clean_tree(tmp: str) -> None:
    """The failure this whole tool exists to prevent: a clean report by accident.

    Every rule in `--selftest` proves the detector classifies a given file
    correctly; none of them notices the gate being pointed at nothing. If
    `userspace/coreutils` is renamed away, the listing comes back empty, no
    finding is new, and the gate passes forever.

    It is a per-tree question, which is why it belongs here: the commit is what
    disarms the gate, and the working tree -- where the author is mid-rename,
    or has simply not deleted the old directory yet -- still has the corpus and
    would answer that all is well. Exit 2, not 1, because the gate has lost its
    subject rather than found a defect.
    """
    root = _argv_repo(tmp, "g4l")
    commit(root)
    remove(root, "userspace/coreutils/src/bin/clean.rs")
    sha = commit(root, "the gated tree goes somewhere else")
    write(root, "userspace/coreutils/src/bin/clean.rs", _ARGV_OK)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk still has a corpus and is judged normally",
          disk.returncode, 0)
    check("gate 4: the commit has none, which is no verdict rather than a pass",
          rev.returncode, 2)
    check("gate 4: ...saying so, rather than exiting quietly",
          "nothing to judge" in rev.stderr, True)


def case_gate4_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1 -- the same contract gates 2 and 3 have with run-checker.sh.

    Exit 1 gets the gate's refusal printed over it, which here is nine
    paragraphs telling the author their utility dies on a legal filename and
    offering the bypass. A revision that cannot be read says nothing about
    anybody's utility.

    The status is checked *and* the message, because a status alone is not
    actionable. run-checker.sh keeps the log of a no-verdict run and points the
    author at it; if the text does not name the revision it could not open, the
    author is left with "gate 4 did not run" and nowhere to start. Naming it is
    also the only thing separating this from the other way to reach exit 2 --
    the corpus guard reaches the same status for an entirely different reason,
    and the two want opposite responses.
    """
    root = _argv_repo(tmp, "g4e")
    commit(root)
    proc = run_checker(root, "argv-utf8.py", "--check", "--head", "nosuchrev")
    check("gate 4: an unreadable --head exits 2, not 1", proc.returncode, 2)
    check("gate 4: ...naming the revision it could not read",
          "nosuchrev" in proc.stderr, True)


# --------------------------------------------------------------------------
# Gate 6 -- host-errmsg.py
#
# The defect it looks for: a diagnostic that interpolates an `io::Error` with
# `{e}`, so the utility prints the *host's* wording -- "The system cannot find
# the file specified. (os error 2)" -- where every reader expects strerror(3)'s
# "No such file or directory". Binding `let why = strerror(&e);` and printing
# `{why}` unmakes it, so one line decides whether a file is a finding.
#
# What it reads from a tree, and therefore what has to be made to differ: the
# `.rs` sources under `userspace/coreutils`, *which* `.rs` files are there at
# all, and `scripts/host-errmsg-baseline.txt`. The baseline is the input a
# sources-only conversion leaves behind, and it is a *suppression* list -- so
# leaving it on the disk is a false pass whose every visible symptom is
# identical to a clean tree. `--list` gets its own case for gate 4's reason:
# nothing exits non-zero on it, so a wrong tree there is invisible to every
# assertion about a verdict, and `--list` is the mode a person actually reads
# when they are burning the backlog down.
# --------------------------------------------------------------------------

_HE_HOST = '''\
fn main() {
    if let Err(e) = std::fs::metadata("x") {
        eprintln!("tool: cannot stat 'x': {e}");
    }
}
'''

_HE_OK = '''\
fn main() {
    if let Err(e) = std::fs::metadata("x") {
        let why = strerror(&e);
        eprintln!("tool: cannot stat 'x': {why}");
    }
}
'''

# Two offending sites in one file, for the `--list` case: `--check` keys on the
# file and would count this the same as `_HE_HOST`, so only a mode that counts
# *sites* can tell the two apart.
_HE_HOST_TWICE = '''\
fn main() {
    if let Err(e) = std::fs::metadata("x") {
        eprintln!("tool: cannot stat 'x': {e}");
        eprintln!("tool: giving up on 'x': {e}");
    }
}
'''

_HE_BASELINE = "# nothing known-wrong yet\n"

# The finding the fixtures argue about, spelled the way the baseline spells it.
_HE_KEY = "userspace/coreutils/src/bin/tool.rs:host-error-text"


def _errmsg_repo(tmp: str, name: str) -> str:
    """A repository with the gated tree present and nothing printing host text.

    `clean.rs` is committed in both trees for the reason `_argv_repo` explains:
    "the checker found nothing" must never be reachable by the checker finding
    no *files*, which is how this gate fails silently if the enumeration is
    pointed at the wrong tree or the wrong prefix.
    """
    root = new_repo(tmp, name, ("host-errmsg.py",))
    write(root, "scripts/host-errmsg-baseline.txt", _HE_BASELINE)
    write(root, "userspace/coreutils/src/bin/clean.rs", _HE_OK)
    return root


def case_gate6_a_tidied_worktree_cannot_hide_a_committed_host_message(tmp: str) -> None:
    """The silent half: the commit prints Windows' wording, the disk does not."""
    root = _errmsg_repo(tmp, "g6a")
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)
    sha = commit(root)
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_OK)

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: the disk prints POSIX's wording throughout", disk.returncode, 0)
    check("gate 6: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 6: ...naming the bin the commit breaks",
          "tool.rs" in rev.stdout + rev.stderr, True)


def case_gate6_an_uncommitted_host_message_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: a half-converted bin on the disk, nothing wrong in the commit."""
    root = _errmsg_repo(tmp, "g6b")
    sha = commit(root)
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: the disk refuses the uncommitted host message", disk.returncode, 1)
    check("gate 6: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate6_the_baseline_is_read_from_the_same_tree(tmp: str) -> None:
    """The input a sources-only conversion leaves on the disk.

    Converting the `.rs` walk and leaving `load_baseline` reading the disk
    passes both cases above, because neither of them edits the baseline. It is
    the sharpest of the three inputs here for the reason gate 4's is: this
    baseline is a live backlog that authors edit while they burn it down, so an
    *uncommitted* waiver excusing a *committed* regression is not a
    contrived state -- it is Tuesday. And it self-conceals on the next push,
    when the waiver is committed too.
    """
    root = _errmsg_repo(tmp, "g6c")
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)
    sha = commit(root)
    check("gate 6: the fixture starts refused",
          run_checker(root, "host-errmsg.py", "--check",
                      "--head", sha).returncode, 1)

    # Forgive it in a *commit*, and un-forgive it on the *disk*.
    write(root, "scripts/host-errmsg-baseline.txt", _HE_BASELINE + _HE_KEY + "\n")
    sha = commit(root, "baseline it")
    write(root, "scripts/host-errmsg-baseline.txt", _HE_BASELINE)

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: the disk's baseline forgives nothing", disk.returncode, 1)
    check("gate 6: the commit's baseline forgives it, and passes",
          rev.returncode, 0)


def case_gate6_a_file_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The enumeration, not only the contents.

    Every case above edits a file present in both trees, so a checker listing
    `.rs` files from the disk and reading their text from the revision passes
    all of them. Here the offending bin is not on the disk at all.
    """
    root = _errmsg_repo(tmp, "g6d")
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)
    sha = commit(root)
    remove(root, "userspace/coreutils/src/bin/tool.rs")

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: the disk has no such file to judge", disk.returncode, 0)
    check("gate 6: the commit still has it, and is refused", rev.returncode, 1)
    check("gate 6: ...naming the file the disk lacks",
          "tool.rs" in rev.stdout + rev.stderr, True)


def case_gate6_the_stale_half_of_the_ratchet_describes_the_commit(tmp: str) -> None:
    """The other direction through the same two inputs.

    `--check` also fails on a baseline line naming a bin that no longer has the
    defect -- 17 of this ratchet's 24 lines were dead when that guard was
    added, and every one of them was a bin that could have regressed all the
    way back under a green gate. Nothing above touches `stale`, so a conversion
    that got `new` right and left `stale` reading the disk passes every other
    gate-6 case in this file.

    Here the repair is committed and the disk still has the defect: the
    commit's baseline line is dead and must be reported, the disk's is live and
    must not be.
    """
    root = _errmsg_repo(tmp, "g6i")
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)
    write(root, "scripts/host-errmsg-baseline.txt", _HE_BASELINE + _HE_KEY + "\n")
    commit(root, "baselined backlog")
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_OK)
    sha = commit(root, "fix the bin, leave the baseline")
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: the disk's baseline line is still earning its keep",
          disk.returncode, 0)
    check("gate 6: the commit's is dead, and the push is refused",
          rev.returncode, 1)
    # For the stated reason, and not because something else went wrong: a bare
    # exit 1 is also what a crash or an unreadable revision would produce.
    check("gate 6: ...reported as fixed rather than as new",
          "FIXED " + _HE_KEY in rev.stdout, True)
    check("gate 6: ...and the disk reports nothing fixed",
          "FIXED" in disk.stdout, False)


def case_gate6_the_listing_reads_the_same_tree(tmp: str) -> None:
    """The mode no exit code depends on, and the one the backlog is read from.

    `--list` walks the tree itself rather than going through `findings`, and
    prints every *site* where `--check` keys on the file. Both runs exit 0
    whatever tree they read, so no assertion elsewhere in this file can see it
    pointed at the wrong one -- and it is what a person runs to decide which
    bin to convert next, so a wrong count here is wrong in the only place it is
    read.

    Two sites in one file, deliberately: a one-site fixture would make the
    site count and the file count agree, and this case would then pass against
    a `--list` that had quietly started counting files.
    """
    root = _errmsg_repo(tmp, "g6f")
    write(root, "userspace/coreutils/src/bin/tool.rs", _HE_HOST_TWICE)
    sha = commit(root)
    remove(root, "userspace/coreutils/src/bin/tool.rs")

    disk = run_checker(root, "host-errmsg.py", "--list")
    rev = run_checker(root, "host-errmsg.py", "--list", "--head", sha)
    check("gate 6: neither tree is refused by a listing",
          (disk.returncode, rev.returncode), (0, 0))
    check("gate 6: the disk has nothing left to list",
          "0 site(s) in 0 file(s)." in disk.stdout, True)
    check("gate 6: the commit's two sites are both counted",
          "2 site(s) in 1 file(s)." in rev.stdout, True)


def case_gate6_build_output_is_skipped_on_the_side_that_can_see_it(tmp: str) -> None:
    """The rule only the disk arm can break, so only it can pin.

    A revision never lists `target/`, so this is about the working-tree arm
    alone. That arm hand-rolled its own walk with its own copy of the skip
    rule; the conversion deleted it and delegates to `gittree`. If the
    delegation is ever dropped for a plain `os.walk`, the gate starts reporting
    generated sources nobody wrote, inside tens of gigabytes, and becomes
    something people switch off rather than fix.

    The control carries as much weight as the case: the same bytes one
    directory across must still be found, or "nothing reported" would be
    satisfied by a checker that had stopped reading the disk at all.
    """
    root = _errmsg_repo(tmp, "g6k")
    sha = commit(root)
    # After the commit, so it is on the disk and in no revision -- which is
    # what build output is.
    write(root, "userspace/coreutils/target/debug/build/gen.rs", _HE_HOST)

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: generated sources under target/ are not judged",
          (disk.returncode, rev.returncode), (0, 0))

    write(root, "userspace/coreutils/notes/gen.rs", _HE_HOST)
    disk2 = run_checker(root, "host-errmsg.py", "--check")
    check("gate 6: ...while the same bytes elsewhere on the disk are",
          disk2.returncode, 1)


def case_gate6_a_baseline_cannot_be_written_from_a_revision(tmp: str) -> None:
    """`--write-baseline` writes the disk; `--head` judges something else.

    Allowing both would write a suppression list describing a tree that is not
    the tree it lands in -- so the next `--check` reports findings the file
    does not mention and forgives lines nothing produces, and the author's only
    evidence is a file they just regenerated. Refused as a usage error rather
    than resolved to either tree, because either choice is a file whose name
    does not say which tree it describes.
    """
    root = _errmsg_repo(tmp, "g6g")
    sha = commit(root)
    proc = run_checker(root, "host-errmsg.py", "--write-baseline", "--head", sha)
    check("gate 6: --write-baseline with --head is refused", proc.returncode, 2)
    check("gate 6: ...saying which two flags disagree",
          "--write-baseline" in proc.stderr and "--head" in proc.stderr, True)


def case_gate6_a_tree_with_no_gated_sources_is_not_a_clean_tree(tmp: str) -> None:
    """The failure this whole tool exists to prevent: a clean report by accident.

    Every rule in `--selftest` proves the detector classifies a given file
    correctly; none notices the gate being pointed at nothing. Rename
    `userspace/coreutils` away and the listing comes back empty, no finding is
    new, and the gate passes forever.

    It is a per-tree question, which is what puts it in this file rather than
    in the checker's own suite: the *commit* is what disarms the gate, and the
    working tree -- where the author is mid-rename, or has simply not deleted
    the old directory -- still holds the corpus and would answer that all is
    well. This gate asked exactly that wrong question until 2026-09-03, from
    inside `--selftest`, against the disk. Exit 2, not 1: the gate has lost its
    subject rather than found a defect.
    """
    root = _errmsg_repo(tmp, "g6l")
    commit(root)
    remove(root, "userspace/coreutils/src/bin/clean.rs")
    sha = commit(root, "the gated tree goes somewhere else")
    write(root, "userspace/coreutils/src/bin/clean.rs", _HE_OK)

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: the disk still has a corpus and is judged normally",
          disk.returncode, 0)
    check("gate 6: the commit has none, which is no verdict rather than a pass",
          rev.returncode, 2)
    check("gate 6: ...saying so, rather than exiting quietly",
          "nothing to judge" in rev.stderr, True)


def case_gate6_a_baseline_absent_from_the_tree_is_not_a_pile_of_new_findings(tmp: str) -> None:
    """The second input's version of the same guard, with a louder failure.

    A missing corpus goes silent; a missing *baseline* goes loud and wrong. It
    reads as an empty backlog, so every bin the real file forgives becomes a
    NEW finding and the push is refused with gate 6's whole refusal printed
    over a list of bins nobody touched -- the false accusation
    `scripts/run-checker.sh` was written to argue is the worst thing a gate can
    do. Per-tree for the corpus guard's reason: it is a commit that moves the
    path, and the disk still has it.

    `--write-baseline` is excluded from the guard because it *creates* the
    file, and is checked here so that exclusion cannot be dropped: a bootstrap
    that refuses to bootstrap would be found only by whoever next moved the
    baseline, which is the same person this guard is protecting.
    """
    root = _errmsg_repo(tmp, "g6m")
    commit(root)
    remove(root, "scripts/host-errmsg-baseline.txt")
    sha = commit(root, "the baseline goes somewhere else")
    write(root, "scripts/host-errmsg-baseline.txt", _HE_BASELINE)

    disk = run_checker(root, "host-errmsg.py", "--check")
    rev = run_checker(root, "host-errmsg.py", "--check", "--head", sha)
    check("gate 6: the disk's baseline is where it always was", disk.returncode, 0)
    check("gate 6: the commit's is gone, which is no verdict", rev.returncode, 2)
    check("gate 6: ...naming the file rather than blaming a bin",
          "host-errmsg-baseline.txt" in rev.stderr, True)
    # `--list` never consults the baseline, so it must not be stopped by one.
    check("gate 6: a listing does not need the baseline it never reads",
          run_checker(root, "host-errmsg.py", "--list",
                      "--head", sha).returncode, 0)


def case_gate6_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1 -- the same contract the other converted gates have.

    Exit 1 gets this gate's refusal printed over it: eight paragraphs telling
    the author a utility of theirs prints Windows' wording, showing the
    want/got pair, and offering the bypass. A revision that could not be read
    says nothing about anybody's diagnostics. The message is asserted as well
    as the status, because run-checker.sh's no-verdict text sends the author to
    the checker's own output -- and if that output does not name the revision,
    the author has "gate 6 did not run" and nowhere to start.
    """
    root = _errmsg_repo(tmp, "g6e")
    commit(root)
    proc = run_checker(root, "host-errmsg.py", "--check", "--head", "nosuchrev")
    check("gate 6: an unreadable --head exits 2, not 1", proc.returncode, 2)
    check("gate 6: ...naming the revision it could not read",
          "nosuchrev" in proc.stderr, True)


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


def _push_fixture(tmp: str, name: str,
                  checkers: tuple[str, ...] = ("multicall-aliases.py",),
                  seed: dict[str, str] | None = None) -> str:
    """A repository with a remote and the real hook and `checkers` installed.

    Only the named checkers are installed, which is what makes the other gates
    skip themselves: each one tests for its own script and stands down when it
    is absent. So a gate-3 fixture is not silently also being judged by gate 2.
    """
    if seed is None:
        seed = {"scripts/multicall-aliases-baseline.txt": _EMPTY_BASELINE}
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
    copies = [(HOOK, os.path.join(hooks, "pre-push")),
              (LIB, os.path.join(work, "scripts", "run-checker.sh"))]
    for script in ("gittree.py", *checkers):
        copies.append((os.path.join(HERE, script),
                       os.path.join(work, "scripts", script)))
    for src, dst in copies:
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        with open(src, encoding="utf-8", newline="") as fh:
            body = fh.read()
        with open(dst, "w", encoding="utf-8", newline="") as fh:
            fh.write(body)
    os.chmod(os.path.join(hooks, "pre-push"), 0o755)

    git(work, "remote", "add", "origin", remote)
    for rel, text in seed.items():
        write(work, rel, text)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "install the gate")
    git(work, "push", "--quiet", "origin", "main")
    return work


# One line's worth of each gate's refusal paragraph, and nothing wider: the
# refusals are hand-wrapped, so a whole-sentence probe never matches and a
# correct refusal reads as an unrelated error. Kept free of the em dashes the
# hook uses, which do not survive a cp1252 console.
_G2_REFUSAL = "command name exists that nothing can run"
_G3_REFUSAL = "is raced by its own tests"
_G4_REFUSAL = "dies on a legal filename"
# Gate 6's own refusal sentence, not its summary line: "prints the host's error
# text" also occurs in the *checker's* FIX advice, which is printed by a
# `--check` run that the hook then goes on to allow. Matching it would call a
# fixture refused on the strength of text from a gate that did not refuse it.
_G6_REFUSAL = "The message is an interface. Anything that greps"


def _push(work: str, ref: str = "main",
          marker: str = _G2_REFUSAL) -> tuple[str, str]:
    """(verdict, output). `allowed`, `refused`, or `error:<...>`.

    Only the *named gate's* refusal counts as `refused`. A suite that accepted
    any refusal would pass on a fixture that trips some other gate and never
    reach the thing it is about.

    `ALLOW_FMT_DRIFT=1` because the fixture's `.rs` files are hand-written and
    gate 7 would rustfmt them; `test-pre-push-fmt-gate.py` covers that gate
    properly. Nothing else is bypassed -- the other gates skip themselves
    because this fixture does not install their checkers.
    """
    env = gitenv.clean_env()
    env["ALLOW_FMT_DRIFT"] = "1"
    proc = subprocess.run(["git", "push", "origin", ref], cwd=work, env=env,
                          capture_output=True, text=True, check=False)
    # Redact the fixture's own paths before anyone matches on the output. git
    # prints `To <remote>` and the hook prints its log path, so a case asserting
    # `"<alias>" in blob` can be satisfied by the *directory name* instead of by
    # anything a gate said. That is not hypothetical: a fixture called
    # `g2push-offbranch` made `"offbranch" in blob` true while the gate had
    # skipped itself entirely, and the case read as a pass.
    root = os.path.dirname(work)
    blob = proc.stdout + proc.stderr
    for spelling in (root, root.replace(os.sep, "/")):
        blob = blob.replace(spelling, "<fixture>")
    if proc.returncode == 0:
        return "allowed", blob
    if marker in blob:
        return "refused", blob
    return "error:" + blob.strip().replace("\n", " | ")[:600], blob


def _tally(blob: str) -> tuple[set[str], set[str]]:
    """(gates that ran, gates that skipped), from the hook's own summary.

    "The push was allowed" and "the gate was asked" are different claims, and
    this line is the only thing that tells them apart: a gate that skipped
    itself allows everything, including every fixture in this file. Parsed from
    the summary rather than matched as a substring because the two names appear
    in each other's neighbourhood -- `raced-global` is a prefix of
    `raced-global-selftest`, and a gate name occurring anywhere in the blob is
    not evidence that it is on the `ran:` side of it.
    """
    ran: set[str] = set()
    skipped: set[str] = set()
    for line in blob.splitlines():
        stripped = line.strip()
        for prefix, into in (("ran:", ran), ("skipped:", skipped)):
            if stripped.startswith(prefix):
                into.update(stripped[len(prefix):].split())
    return ran, skipped


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
          "unreachable-command" in _tally(blob)[0], True)


def case_gate2_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` while checked out on `main`.

    The two cases above both push the checked-out branch, so `HEAD` and the sha
    being pushed are the same commit -- which means neither of them can tell
    "read the sha" apart from "read HEAD". Every gate here decides *whether to
    run* through the `touches` helper, and a helper that asks its question about
    HEAD asks it about the wrong branch the moment the two differ: `main` has
    nothing unpushed under `userspace/`, so the gate skips itself and the alias
    on `feature` is published unjudged.

    That is the same defect the whole suite is about -- judging what is on hand
    rather than what is being sent -- one level up from the checker, in the
    predicate that decides if the checker runs at all. It is silent, and it is
    reached by a wholly ordinary push.
    """
    work = _push_fixture(tmp, "g2push-offbranch")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, "userspace/real/src/main.rs", _DISPATCH % "offbranch")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "an alias on a branch we will leave")
    # Back to a branch whose every commit is already on the remote, so a
    # HEAD-scoped `touches` finds nothing under userspace/ and skips the gate.
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature")
    check("gate 2 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 2 end to end: ...naming the alias on that other branch",
          "offbranch" in blob, True)


def case_gate3_the_hook_refuses_a_commit_the_worktree_no_longer_shows(tmp: str) -> None:
    """End to end: gate 3's own wiring, not gate 2's.

    Gate 2's end-to-end cases prove `touches` and the `$pushed_shas` loop are
    right, and prove nothing whatever about gate 3, which is a separate block
    with its own guard, its own loop and its own `--head "$sha"`. Dropping the
    flag from *this* invocation leaves every gate-2 case green.

    Gate 3 also runs `--selftest` before `--check` and disbelieves the result if
    that fails, so this case additionally covers the arm where the self-test is
    what decides -- a broken detector reports a clean tree, not a broken one.
    """
    work = _push_fixture(
        tmp, "g3push-hide", checkers=("raced-globals.py",),
        seed={"scripts/raced-globals-baseline.txt": _RG_BASELINE},
    )
    write(work, "posix/Cargo.toml", _PLAIN_MANIFEST)
    write(work, "posix/src/lib.rs", "pub fn ok() {}\n")
    write(work, "posix/src/race.rs", _RACED)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "two tests sharing one atomic")
    # The tidy-up that makes the disk lie.
    write(work, "posix/src/race.rs", _UNRACED)

    verdict, blob = _push(work, marker=_G3_REFUSAL)
    check("gate 3 end to end: the push is refused", verdict, "refused")
    check("gate 3 end to end: ...naming the global only the commit races",
          "COUNTER" in blob, True)


def case_gate3_the_hook_allows_a_clean_commit_under_a_dirty_worktree(tmp: str) -> None:
    """End to end, the other direction -- and the one that checks it ran.

    A gate that had skipped itself would also allow this, and would have allowed
    the case above too if its refusal came from somewhere else. The hook's own
    tally is the only thing that tells "passed" apart from "never asked".
    """
    work = _push_fixture(
        tmp, "g3push-wip", checkers=("raced-globals.py",),
        seed={"scripts/raced-globals-baseline.txt": _RG_BASELINE},
    )
    write(work, "posix/Cargo.toml", _PLAIN_MANIFEST)
    write(work, "posix/src/lib.rs", "pub fn ok() {}\n")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a crate with nothing shared in it")
    write(work, "posix/src/race.rs", _RACED)

    verdict, blob = _push(work, marker=_G3_REFUSAL)
    check("gate 3 end to end: an uncommitted race does not block", verdict,
          "allowed")
    check("gate 3 end to end: ...and the gate actually ran",
          "raced-global" in _tally(blob)[0], True)


def case_gate3_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` from `main`, for gate 3's loop.

    Gate 2's equivalent case pins `touches`, which is shared. This pins the part
    that is not: gate 3's own `for sha in $pushed_shas`. Pinning that loop to
    `git rev-parse HEAD` instead survives every other case in this file, because
    every other one pushes the branch it is standing on and so cannot tell the
    two apart -- which is precisely how the `touches` defect went unnoticed.
    """
    work = _push_fixture(
        tmp, "g3push-elsewhere", checkers=("raced-globals.py",),
        seed={"scripts/raced-globals-baseline.txt": _RG_BASELINE},
    )
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, "posix/Cargo.toml", _PLAIN_MANIFEST)
    write(work, "posix/src/lib.rs", "pub fn ok() {}\n")
    write(work, "posix/src/race.rs", _RACED)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a race on a branch we will leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G3_REFUSAL)
    check("gate 3 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 3 end to end: ...naming the global on that other branch",
          "COUNTER" in blob, True)


def case_gate3_the_hook_treats_a_deletion_as_nothing_to_judge(tmp: str) -> None:
    """A push that sends no commits must not be refused, or claim to have run.

    Deleting a remote branch is a push with an all-zero local sha, so
    `pushed_shas` is empty and there is no tree to judge. Two things must follow
    and neither is automatic: the push is allowed however broken the working
    tree is, and the tally says `skipped` rather than `ran` -- because a gate
    reporting that it ran, having looped over an empty list, is a gate whose
    summary line cannot be believed by anyone reading it.

    Note for whoever changes `touches` next: with the helper scoped to
    `$pushed_shas`, gate 3's own `[ -n "${pushed_shas# }" ]` guard is currently
    *unobservable* -- an empty list already makes `touches` false, so removing
    the guard changes no outcome, and mutation testing confirms no behavioural
    case can kill that mutant. It is kept, and pinned statically by
    `test-pre-push-gates.py`, because the redundancy is one edit deep: `touches`
    was HEAD-scoped until 2026-09-02, and under that spelling this exact push
    reached the loop with nothing in it.
    """
    work = _push_fixture(
        tmp, "g3push-delete", checkers=("raced-globals.py",),
        seed={"scripts/raced-globals-baseline.txt": _RG_BASELINE},
    )
    write(work, "posix/Cargo.toml", _PLAIN_MANIFEST)
    write(work, "posix/src/lib.rs", "pub fn ok() {}\n")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a clean crate")
    check("gate 3 end to end: the clean crate publishes",
          _push(work, marker=_G3_REFUSAL)[0], "allowed")
    git(work, "branch", "doomed")
    check("gate 3 end to end: the doomed branch publishes",
          _push(work, "doomed", marker=_G3_REFUSAL)[0], "allowed")

    # A race on the disk and nowhere else, so a gate reading the disk has
    # something to refuse a push that is sending nothing at all.
    write(work, "posix/src/race.rs", _RACED)
    verdict, blob = _push(work, ":doomed", marker=_G3_REFUSAL)
    check("gate 3 end to end: deleting a branch is not refused", verdict,
          "allowed")
    check("gate 3 end to end: ...and no gate claims to have judged it",
          "raced-global" in _tally(blob)[1], True)


def _g4_push_fixture(tmp: str, name: str) -> str:
    """A push fixture for gate 4, with the gated tree already published.

    The clean bin is committed and pushed by `_push_fixture` itself, so every
    case below adds exactly one commit and the gate has a non-empty gated tree
    to walk in both trees.
    """
    return _push_fixture(
        tmp, name, checkers=_G4_CHECKERS,
        seed={"scripts/argv-utf8-baseline.txt": _AU_BASELINE,
              "userspace/coreutils/src/bin/clean.rs": _ARGV_OK},
    )


def case_gate4_the_hook_refuses_a_commit_the_worktree_no_longer_shows(tmp: str) -> None:
    """End to end: gate 4's own wiring, not gate 2's or gate 3's.

    Each gate is a separate block with its own guard, its own loop and its own
    `--head "$sha"`. Dropping the flag from *this* invocation leaves every
    gate-2 and gate-3 case in this file green, which is why the end-to-end
    cases are per gate rather than one shared proof that the hook works.
    """
    work = _g4_push_fixture(tmp, "g4push-hide")
    write(work, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bin that reads argv as String")
    # The tidy-up that makes the disk lie.
    write(work, "userspace/coreutils/src/bin/tool.rs", _ARGV_OK)

    verdict, blob = _push(work, marker=_G4_REFUSAL)
    check("gate 4 end to end: the push is refused", verdict, "refused")
    check("gate 4 end to end: ...naming the bin only the commit breaks",
          "tool.rs" in blob, True)


def case_gate4_the_hook_allows_a_clean_commit_under_a_dirty_worktree(tmp: str) -> None:
    """End to end, the other direction -- and the one that checks it ran.

    A gate that had skipped itself would allow this, and would have allowed the
    case above too if that refusal came from somewhere else. The hook's own
    tally is the only thing that tells "passed" apart from "never asked" --
    and here it also tells gate 4 apart from gate 3, which this fixture
    installs as well because the checker loads its lexer out of it.
    """
    work = _g4_push_fixture(tmp, "g4push-wip")
    write(work, "userspace/coreutils/src/bin/other.rs", _ARGV_OK)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bin that carries the OsString")
    write(work, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)

    verdict, blob = _push(work, marker=_G4_REFUSAL)
    check("gate 4 end to end: an uncommitted panic does not block", verdict,
          "allowed")
    check("gate 4 end to end: ...and the gate actually ran",
          "argv-utf8" in _tally(blob)[0], True)


def case_gate4_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` from `main`, for gate 4's loop.

    Every other case in this file pushes the branch it is standing on, so in
    all of them `HEAD` and the sha being pushed are the same commit and
    `--head "$sha"` cannot be told apart from `git rev-parse HEAD`. That blind
    spot is what hid the `touches` defect until 2026-09-02, and it is per gate:
    gate 3's off-branch case says nothing about gate 4's loop.
    """
    work = _g4_push_fixture(tmp, "g4push-elsewhere")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a panic on a branch we will leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G4_REFUSAL)
    check("gate 4 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 4 end to end: ...naming the bin on that other branch",
          "tool.rs" in blob, True)


def _g6_push_fixture(tmp: str, name: str) -> str:
    """A push fixture for gate 6, with the gated tree already published.

    Only `host-errmsg.py` is installed, so every other gate stands down and a
    refusal here can only have come from gate 6.
    """
    return _push_fixture(
        tmp, name, checkers=("host-errmsg.py",),
        seed={"scripts/host-errmsg-baseline.txt": _HE_BASELINE,
              "userspace/coreutils/src/bin/clean.rs": _HE_OK},
    )


def case_gate6_the_hook_refuses_a_commit_the_worktree_no_longer_shows(tmp: str) -> None:
    """End to end: gate 6's own wiring, not gate 2's, 3's or 4's.

    Each gate is a separate block with its own guard, its own loop and its own
    `--head "$sha"`. Dropping the flag from *this* invocation leaves every
    other case in this file green, which is why the end-to-end proof is per
    gate rather than one shared demonstration that the hook works at all.
    """
    work = _g6_push_fixture(tmp, "g6push-hide")
    write(work, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bin that prints the host's wording")
    # The tidy-up that makes the disk lie.
    write(work, "userspace/coreutils/src/bin/tool.rs", _HE_OK)

    verdict, blob = _push(work, marker=_G6_REFUSAL)
    check("gate 6 end to end: the push is refused", verdict, "refused")
    check("gate 6 end to end: ...naming the bin only the commit breaks",
          "tool.rs" in blob, True)


def case_gate6_the_hook_allows_a_clean_commit_under_a_dirty_worktree(tmp: str) -> None:
    """End to end, the other direction -- and the one that checks it ran.

    A gate that had skipped itself would allow this, and would have allowed the
    case above too if that refusal came from elsewhere. The hook's own tally is
    the only thing that separates "gate 6 passed" from "gate 6 was never
    asked", and this gate has two ways to be skipped that the others do not
    share: `touches userspace/` and an empty pushed-sha list.
    """
    work = _g6_push_fixture(tmp, "g6push-wip")
    write(work, "userspace/coreutils/src/bin/other.rs", _HE_OK)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bin that binds strerror")
    write(work, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)

    verdict, blob = _push(work, marker=_G6_REFUSAL)
    check("gate 6 end to end: an uncommitted host message does not block",
          verdict, "allowed")
    check("gate 6 end to end: ...and the gate actually ran",
          "host-errmsg" in _tally(blob)[0], True)


def case_gate6_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` from `main`, for gate 6's loop.

    Every case that pushes the branch it is standing on has `HEAD` and the sha
    being pushed as the same commit, so `--head "$sha"` there is
    indistinguishable from `git rev-parse HEAD` -- the blind spot that hid the
    `touches` defect until 2026-09-02. It is per gate: gate 4's off-branch case
    says nothing about gate 6's loop.
    """
    work = _g6_push_fixture(tmp, "g6push-elsewhere")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, "userspace/coreutils/src/bin/tool.rs", _HE_HOST)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "host wording on a branch we will leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G6_REFUSAL)
    check("gate 6 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 6 end to end: ...naming the bin on that other branch",
          "tool.rs" in blob, True)


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
    case_gate2_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate2_an_unopenable_revision_is_not_a_finding,
    case_gate3_a_tidied_worktree_cannot_hide_a_committed_race,
    case_gate3_an_uncommitted_race_does_not_block_a_clean_push,
    case_gate3_the_baseline_is_read_from_the_same_tree,
    case_gate3_the_manifest_is_read_from_the_same_tree,
    case_gate3_a_file_absent_from_the_disk_is_still_judged,
    case_gate3_the_crate_boundary_is_found_in_the_same_tree,
    case_gate3_the_silenced_crate_report_describes_the_commit,
    case_gate3_every_test_target_probe_reads_the_same_tree,
    case_gate3_an_unopenable_revision_is_not_a_finding,
    case_gate3_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate3_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate3_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate3_the_hook_treats_a_deletion_as_nothing_to_judge,
    case_gate4_a_tidied_worktree_cannot_hide_a_committed_panic,
    case_gate4_an_uncommitted_panic_does_not_block_a_clean_push,
    case_gate4_the_baseline_is_read_from_the_same_tree,
    case_gate4_a_file_absent_from_the_disk_is_still_judged,
    case_gate4_the_stale_half_of_the_ratchet_describes_the_commit,
    case_gate4_the_ungated_survey_reads_the_same_tree,
    case_gate4_build_output_is_skipped_on_the_side_that_can_see_it,
    case_gate4_a_tree_with_no_gated_sources_is_not_a_clean_tree,
    case_gate4_an_unopenable_revision_is_not_a_finding,
    case_gate4_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate4_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate4_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate6_a_tidied_worktree_cannot_hide_a_committed_host_message,
    case_gate6_an_uncommitted_host_message_does_not_block_a_clean_push,
    case_gate6_the_baseline_is_read_from_the_same_tree,
    case_gate6_a_file_absent_from_the_disk_is_still_judged,
    case_gate6_the_stale_half_of_the_ratchet_describes_the_commit,
    case_gate6_the_listing_reads_the_same_tree,
    case_gate6_build_output_is_skipped_on_the_side_that_can_see_it,
    case_gate6_a_baseline_cannot_be_written_from_a_revision,
    case_gate6_a_tree_with_no_gated_sources_is_not_a_clean_tree,
    case_gate6_a_baseline_absent_from_the_tree_is_not_a_pile_of_new_findings,
    case_gate6_an_unopenable_revision_is_not_a_finding,
    case_gate6_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate6_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate6_the_hook_judges_a_branch_it_is_not_standing_on,
)


def main() -> int:
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes. Assert a floor, as the sibling suites do.
    if len(CASES) < 49:
        print(f"FATAL: only {len(CASES)} cases registered; the suite has at "
              f"least 49. The list is broken, not the code.")
        return 1
    # ...and each converted gate must be represented, through the real hook as
    # well as directly. A floor on the count alone would be met by any number of
    # gate-2 cases, and a floor on end-to-end cases alone was already met before
    # gate 3 was converted at all -- so both are counted per gate. Raise these
    # as each remaining checker is converted; they are the thing that notices a
    # gate's cases being deleted along with the gate's own wiring.
    for gate, floor, e2e_floor in (("gate2", 10, 3), ("gate3", 13, 4),
                                   ("gate4", 12, 3), ("gate6", 14, 3)):
        named = [c for c in CASES if c.__name__.startswith(f"case_{gate}_")]
        hooked = [c for c in named if "the_hook" in c.__name__]
        if len(named) < floor or len(hooked) < e2e_floor:
            print(f"FATAL: {gate} has {len(named)} case(s) of which "
                  f"{len(hooked)} end-to-end; it has at least {floor} and "
                  f"{e2e_floor}. The list is broken, not the code.")
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
