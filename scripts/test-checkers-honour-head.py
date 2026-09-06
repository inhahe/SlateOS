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

import json
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


# The modules a checker needs beside it that are not the checker. Named once
# because there are two fixture builders -- `new_repo` and `_push_fixture` --
# and a list kept twice is a list that is right once. When `gittree` grew its
# `import gitenv`, this list was updated in `new_repo` only, and every
# `_push_fixture` case died of `ModuleNotFoundError` inside the hook, where the
# traceback surfaced as a gate verdict rather than as a missing file.
#
# `gitenv.py` travels with `gittree.py` because `gittree` imports it, and it
# imports it from its *own* directory -- which in a fixture is this copy, not
# the real `scripts/`. Omitting it does not degrade the fixture, it stops the
# checker starting at all.
SUPPORT = ("gittree.py", "gitenv.py")


def new_repo(tmp: str, name: str, checkers: tuple[str, ...]) -> str:
    """A repository with `checkers` (and their support modules) in `scripts/`."""
    root = os.path.join(tmp, name)
    os.makedirs(os.path.join(root, "scripts"))
    for script in (*SUPPORT, *checkers):
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
# `.rs` sources under `userspace/`, *which* `.rs` files are there at all, the
# baseline, the `Cargo.toml` of every crate -- which is what decides whether a
# crate is in scope at all -- and, outside `--check`, the survey of the crates
# that are not. Five inputs, five cases; a checker converted for four of them
# and left reading the disk for the fifth would pass a suite that only tested
# contents.
#
# The manifests are the input added on 2026-09-05, when the gate stopped being
# "the `userspace/coreutils` directory" and became "every crate that does not
# declare itself unimplemented by depending on `userspace/notimpl`". That moved
# a decision about *scope* into file contents, which is the one kind of input
# whose disk-vs-revision disagreement is completely silent: the gate does not
# report a crate it never judged.
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

# The manifest that puts a crate *out* of scope, and the one that leaves it in.
# The declaration is a dependency on `userspace/notimpl`; anything else --
# including no `[dependencies]` at all -- is a crate the gate judges.
_CARGO_STUB = '[package]\nname = "s"\n\n[dependencies]\nnotimpl = { path = "../notimpl" }\n'
_CARGO_REAL = '[package]\nname = "s"\nversion = "0.1.0"\n'

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

    `coreutils` is given no `Cargo.toml` on purpose. A crate is out of scope
    only if it *says* it is unimplemented, so a crate with no manifest at all
    is judged -- and the fixture that relies on that is the one that proves the
    default is the safe direction rather than a lucky accident.
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

    The stub has to *declare* itself one, or it is not outside the gate at all
    -- it is a crate with a panicking bin in it, and the run that was supposed
    to report a number would refuse the push instead.
    """
    root = _argv_repo(tmp, "g4j")
    write(root, "userspace/stub/Cargo.toml", _CARGO_STUB)
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


def case_gate4_the_scope_declaration_is_read_from_the_same_tree(tmp: str) -> None:
    """The fifth input: which crates are judged at all.

    Since 2026-09-05 a crate is out of scope only if its `Cargo.toml` depends
    on `userspace/notimpl`. That is a *scope* decision taken from file
    contents, and it is the most silent input this gate has: every other one
    fails by reporting the wrong thing, while this one fails by reporting
    nothing about a crate it decided not to look at. A checker reading
    manifests from the disk and sources from the revision would find the
    panicking bin, ask the disk whether its crate counts, be told no, and pass.

    The fixture is the edit that actually happens: a stub being fleshed out
    into a program. The commit deletes the `notimpl` dependency and puts a
    panicking `main` in -- so the commit must be refused -- while the working
    tree still carries the old manifest, under which there is nothing to judge.

    Both directions, because the pair is what pins it. The reverse fixture --
    the declaration added in the commit and absent from the disk -- is the one
    that catches a checker reading manifests from the revision but *ignoring*
    them, which the forward case alone cannot see.
    """
    root = _argv_repo(tmp, "g4n")
    write(root, "userspace/tool/Cargo.toml", _CARGO_STUB)
    write(root, "userspace/tool/src/main.rs", _ARGV_OK)
    commit(root, "a stub, declared")
    write(root, "userspace/tool/Cargo.toml", _CARGO_REAL)
    write(root, "userspace/tool/src/main.rs", _ARGV_PANIC)
    sha = commit(root, "fleshed out, and it reads argv as String")
    # The disk goes back to the manifest that exempts it, keeping the source
    # the commit added: the *only* thing that differs between the two trees'
    # verdicts is which manifest was read.
    write(root, "userspace/tool/Cargo.toml", _CARGO_STUB)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk's manifest still exempts the crate",
          disk.returncode, 0)
    check("gate 4: the commit's does not, and the push is refused",
          rev.returncode, 1)
    check("gate 4: ...naming the crate the disk excused",
          "userspace/tool/src/main.rs" in rev.stdout.replace("\\", "/"), True)

    # The other direction: exempted in the commit, judged on the disk.
    root = _argv_repo(tmp, "g4o")
    write(root, "userspace/tool/Cargo.toml", _CARGO_REAL)
    write(root, "userspace/tool/src/main.rs", _ARGV_PANIC)
    commit(root, "a program that panics")
    write(root, "userspace/tool/Cargo.toml", _CARGO_STUB)
    sha = commit(root, "declared unimplemented after all")
    write(root, "userspace/tool/Cargo.toml", _CARGO_REAL)

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk judges the crate its manifest puts in scope",
          disk.returncode, 1)
    check("gate 4: the commit's declaration takes it out, and passes",
          rev.returncode, 0)


def case_gate4_a_declaration_in_the_wrong_table_does_not_exempt(tmp: str) -> None:
    """The scope rule's own edge, end to end rather than in the self-test.

    `--selftest` rule 8 pins `declares_stub` on manifest text, which is where
    the reasoning lives. What it cannot see is the wiring: a checker that
    grepped the manifest for the crate name instead of calling that predicate
    would pass every self-test rule and still let any crate leave the gate by
    adding four characters to `[dev-dependencies]` -- a table no reviewer reads
    for scope, because it has never meant anything about scope.

    So this asserts the predicate is what actually decides, through the same
    entry point the hook uses.
    """
    root = _argv_repo(tmp, "g4p")
    write(root, "userspace/tool/Cargo.toml",
          '[package]\nname = "s"\n\n'
          '[dev-dependencies]\nnotimpl = { path = "../notimpl" }\n')
    write(root, "userspace/tool/src/main.rs", _ARGV_PANIC)
    sha = commit(root)

    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: a dev-dependency on notimpl does not exempt a crate",
          rev.returncode, 1)

    # The control: the same line in the table that does mean it.
    write(root, "userspace/tool/Cargo.toml", _CARGO_STUB)
    sha = commit(root, "declare it properly")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: ...and a real dependency on it does", rev.returncode, 0)


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


def case_gate4_a_baseline_absent_from_the_tree_is_not_four_new_findings(tmp: str) -> None:
    """The second input's version of the guard above, failing the other way.

    A missing corpus goes silent; a missing *baseline* goes loud and wrong. It
    reads back as an empty backlog rather than as an error, so every bin the
    real file forgives becomes a NEW finding and the push is refused with gate
    4's whole nine-paragraph refusal over a list of bins nobody touched -- the
    false accusation `scripts/run-checker.sh` exists to argue is the worst
    thing a gate can do. On a clean tree the same read calls every baseline
    line stale instead, which reads as "the backlog is fixed" over a commit
    that fixed nothing.

    Gate 4 shipped its `--head` conversion with the corpus half of this guard
    and not this half; gate 6 was converted the next day with both, and this
    case is the one that was missing transposed back. Per-tree for the corpus
    guard's reason: a commit moves the path, and the disk still has it.

    `--write-baseline` is excluded from the guard because it *creates* the
    file, and is asserted here so the exclusion cannot be dropped: a bootstrap
    that refuses to bootstrap would be found only by whoever next moved the
    baseline, who is the person this guard is protecting.
    """
    # The backlog must be non-empty, or this case cannot see the failure it is
    # named for. Against an *empty* baseline a missing one is merely a silent
    # false pass -- caught by the status, but the assertion below that no bin
    # is accused would then hold just as well against a checker with no guard
    # at all. So the fixture forgives a real finding, and taking the baseline
    # away turns that finding into an accusation.
    root = _argv_repo(tmp, "g4m")
    write(root, "userspace/coreutils/src/bin/tool.rs", _ARGV_PANIC)
    write(root, "scripts/argv-utf8-baseline.txt", _AU_BASELINE + _AU_KEY + "\n")
    commit(root)
    remove(root, "scripts/argv-utf8-baseline.txt")
    sha = commit(root, "the baseline goes somewhere else")
    write(root, "scripts/argv-utf8-baseline.txt", _AU_BASELINE + _AU_KEY + "\n")

    disk = run_checker(root, "argv-utf8.py", "--check")
    rev = run_checker(root, "argv-utf8.py", "--check", "--head", sha)
    check("gate 4: the disk's baseline still forgives the finding it names",
          disk.returncode, 0)
    check("gate 4: the commit's is gone, which is no verdict", rev.returncode, 2)
    check("gate 4: ...naming the file rather than blaming a bin",
          "argv-utf8-baseline.txt" in rev.stderr, True)
    # The load-bearing negative. Without the guard this run does not go quiet,
    # it goes *wrong*: the forgiven finding reads as new and the author is told
    # a bin they never touched dies on a legal filename.
    check("gate 4: ...and the forgiven bin is not accused of being new",
          "tool.rs" in rev.stdout, False)
    # The guard must not stop the one mode whose job is to create the file.
    # Checked on the disk arm: --write-baseline writes the working tree and is
    # refused outright with --head, so the revision arm cannot express this.
    remove(root, "scripts/argv-utf8-baseline.txt")
    boot = run_checker(root, "argv-utf8.py", "--write-baseline")
    check("gate 4: a bootstrap run still creates the baseline it lacks",
          boot.returncode, 0)


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
# Gate 5 -- getopt-ambiguity-check.py
#
# The defect it looks for: a `LONG_OPTIONS` table that disagrees with the one
# the real GNU utility carries. The table -- not the set of options we handle
# -- is what decides whether an abbreviation like `--v` is ambiguous, so a
# dropped entry silently changes what `yes --v` does.
#
# WHY THIS GATE'S CASES LOOK DIFFERENT FROM THE OTHERS'. Every other checker
# here compares a tree against a baseline file, so both sides of the comparison
# can be written into the fixture. This one compares a tree against *the host*:
# it asks the machine's own GNU `yes` what its table is, over WSL. Only one
# side is ours, and only that side is what `--head` selects. So the fixture
# carries a real utility's real table, and the mutation is a name removed from
# it -- which is the shape of four of the five defects that motivated the gate
# (`mv` lacked `--no-copy`, `rm` and `split` were each missing an entry).
#
# `yes` is the utility chosen because its table is two entries that have not
# moved in decades, so a case failing means the seam broke and not that a
# distribution shipped a different coreutils.
_YES_OK = """\
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), \
("version", Takes::Nothing)];
"""

# `--version` dropped. GNU has it and we would not, which is the dangerous
# direction: the abbreviation `--v` resolves against a table that no longer
# lists it.
_YES_BROKEN = """\
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing)];
"""


def _getopt_repo(tmp: str, name: str) -> str:
    return new_repo(tmp, name, ("getopt-ambiguity-check.py",))


_NO_GNU: list[bool] = []


def _gnu_userland_missing(root: str) -> bool:
    """Whether this host has no GNU utilities to compare against.

    The checker exits 0 with a note in that case -- correctly; a comparison
    that cannot be made has nothing to say -- but that makes both arms of every
    case below agree for a reason that has nothing to do with `--head`. Rather
    than let the group pass vacuously, it is detected and announced.

    Cached: the answer is a property of the host, and asking costs a WSL probe.
    """
    if not _NO_GNU:
        out = run_checker(root, "getopt-ambiguity-check.py", "yes")
        _NO_GNU.append("no GNU userland available" in out.stdout + out.stderr)
    return _NO_GNU[0]


def case_gate5_a_tidied_worktree_cannot_hide_a_committed_table(tmp: str) -> None:
    """The silent half: the commit drops an option, the disk has it back."""
    root = _getopt_repo(tmp, "g5a")
    write(root, "userspace/coreutils/src/bin/yes.rs", _YES_BROKEN)
    sha = commit(root)
    write(root, "userspace/coreutils/src/bin/yes.rs", _YES_OK)

    if _gnu_userland_missing(root):
        print("  SKIP gate 5: no GNU userland on this host to compare against")
        return
    disk = run_checker(root, "getopt-ambiguity-check.py", "yes")
    rev = run_checker(root, "getopt-ambiguity-check.py", "yes", "--head", sha)
    check("gate 5: the disk sees nothing wrong", disk.returncode, 0)
    check("gate 5: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 5: ...naming the option the commit dropped",
          "version" in rev.stdout + rev.stderr, True)


def case_gate5_an_uncommitted_edit_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: work in progress on the disk, nothing wrong in the commit."""
    root = _getopt_repo(tmp, "g5b")
    write(root, "userspace/coreutils/src/bin/yes.rs", _YES_OK)
    sha = commit(root)
    write(root, "userspace/coreutils/src/bin/yes.rs", _YES_BROKEN)

    if _gnu_userland_missing(root):
        print("  SKIP gate 5: no GNU userland on this host to compare against")
        return
    disk = run_checker(root, "getopt-ambiguity-check.py", "yes")
    rev = run_checker(root, "getopt-ambiguity-check.py", "yes", "--head", sha)
    check("gate 5: the disk refuses the uncommitted edit", disk.returncode, 1)
    check("gate 5: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate5_a_bin_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """Enumeration, not just reading, must come from the revision.

    A checker that listed the disk and read the revision would find no bin at
    all here and report a clean tree -- the same "found nothing, called it
    clean" failure the floor in `main` exists for, one level down.
    """
    root = _getopt_repo(tmp, "g5c")
    write(root, "userspace/coreutils/src/bin/yes.rs", _YES_BROKEN)
    sha = commit(root)
    remove(root, "userspace/coreutils/src/bin/yes.rs")

    if _gnu_userland_missing(root):
        print("  SKIP gate 5: no GNU userland on this host to compare against")
        return
    disk = run_checker(root, "getopt-ambiguity-check.py", "yes")
    rev = run_checker(root, "getopt-ambiguity-check.py", "yes", "--head", sha)
    check("gate 5: a deleted bin leaves the disk with nothing to say",
          disk.returncode, 0)
    check("gate 5: ...but the revision still carries it", rev.returncode, 1)
    check("gate 5: ...and it is counted as a table that was checked",
          "1 table(s) checked" in rev.stdout, True)


def case_gate5_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1: `run-checker.sh` reads 1 as a finding about a table."""
    root = _getopt_repo(tmp, "g5d")
    write(root, "userspace/coreutils/src/bin/yes.rs", _YES_OK)
    commit(root)

    proc = run_checker(root, "getopt-ambiguity-check.py", "--head", "nosuchrev")
    check("gate 5: an unopenable revision exits 2", proc.returncode, 2)
    check("gate 5: ...naming the revision it could not read",
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
# Gate 8 -- quote-names.py
#
# The defect it looks for: a diagnostic that interpolates a file name straight
# into its message, so a name containing a newline forges a line of the
# program's stderr.
#
# Converted on 2026-09-02 and given no behavioural cases at all until
# 2026-09-04 -- it was wired with `--head "$sha"`, listed in
# `test-pre-push-gates.py`'s HEAD_GATES, and therefore *asserted to be wired*
# for two days without one line of evidence that the flag changed what it
# decided. That is the gap this block closes, and it is worth naming because
# the two kinds of assertion look interchangeable in a summary and are not: a
# checker can accept `--head`, be called with it correctly, and ignore it, and
# only a fixture whose commit and worktree disagree can tell.
# --------------------------------------------------------------------------

_QN_LEAK = '''\
fn f(p: &std::path::Path) {
    eprintln!("cut: {p}: no such file");
}
'''

_QN_OK = '''\
fn f(p: &std::path::Path) {
    eprintln!("cut: {}: no such file", quotef_os(&p));
}
'''

# The baseline's key, spelled the way `quote-names-baseline.txt` spells it:
# `<path>:<count>`. The count is what makes this ratchet different from gates
# 4 and 6, whose baselines name findings -- so a case that wants to loosen the
# allowance raises a number rather than adding a line.
_QN_FILE = "userspace/coreutils/src/bin/tool.rs"
_QN_BASELINE = "# nothing known-unquoted yet\n"


def _quote_repo(tmp: str, name: str) -> str:
    """A repository with lane B's zone present and nothing leaking a name in it.

    `clean.rs` is in every fixture, committed, in both trees, and is not part
    of any case's argument. It is there so that "the checker found nothing" can
    never be reached by the checker finding no *files* -- which, for this gate
    in particular, is not hypothetical: reporting an empty corpus as a clean
    tree is the defect fixed the same day these cases were written.
    """
    root = new_repo(tmp, name, ("quote-names.py",))
    write(root, "scripts/quote-names-baseline.txt", _QN_BASELINE)
    write(root, "userspace/coreutils/src/bin/clean.rs", _QN_OK)
    return root


def case_gate8_a_tidied_worktree_cannot_hide_a_committed_leak(tmp: str) -> None:
    """The silent half: the commit interpolates a name, the disk quotes it."""
    root = _quote_repo(tmp, "g8a")
    write(root, _QN_FILE, _QN_LEAK)
    sha = commit(root)
    write(root, _QN_FILE, _QN_OK)

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk sees nothing unquoted", disk.returncode, 0)
    check("gate 8: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 8: ...naming the file the commit leaks from",
          "tool.rs" in rev.stdout + rev.stderr, True)


def case_gate8_an_uncommitted_leak_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: an unfinished edit on the disk, nothing wrong in the commit."""
    root = _quote_repo(tmp, "g8b")
    sha = commit(root)
    write(root, _QN_FILE, _QN_LEAK)

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk refuses the uncommitted leak", disk.returncode, 1)
    check("gate 8: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate8_the_baseline_is_read_from_the_same_tree(tmp: str) -> None:
    """The ratchet must not be loosened by a number nobody is publishing.

    This baseline is a *count* per file rather than a list of findings, which
    makes the failure quieter than gates 4 and 6: loosening it is editing a
    digit, not adding a line, and a digit changed in the working tree and not
    committed is invisible in review by construction. If the count came off the
    disk, an author could raise it, push the leak, and drop the edit -- and the
    hook's own advice not to raise a number would be enforced against a file
    nobody was publishing.
    """
    root = _quote_repo(tmp, "g8c")
    write(root, _QN_FILE, _QN_LEAK)
    sha = commit(root)
    check("gate 8: the fixture starts refused",
          run_checker(root, "quote-names.py", "--check",
                      "--head", sha).returncode, 1)

    # Forgive it in a *commit*, and un-forgive it on the *disk*.
    write(root, "scripts/quote-names-baseline.txt",
          _QN_BASELINE + f"{_QN_FILE}:1\n")
    sha = commit(root, "baseline it")
    write(root, "scripts/quote-names-baseline.txt", _QN_BASELINE)

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk's baseline forgives nothing", disk.returncode, 1)
    check("gate 8: the commit's baseline forgives it, and passes",
          rev.returncode, 0)


def case_gate8_a_raised_count_is_read_from_the_same_tree(tmp: str) -> None:
    """The count, not merely the presence of the line.

    The case above swaps a baseline *entry* in and out, which a checker reading
    the disk's file but the commit's entry-set would still pass. This one keeps
    the same entry in both trees and changes only the number: the commit
    forgives two sites, the disk forgives one, and the commit has two. There is
    no version of this that a per-file granularity can answer -- the difference
    between the trees is a single digit, which is exactly the granularity this
    ratchet chose and therefore exactly what has to move with the tree.
    """
    root = _quote_repo(tmp, "g8n")
    write(root, _QN_FILE, _QN_LEAK.replace(
        '    eprintln!("cut: {p}: no such file");\n',
        '    eprintln!("cut: {p}: no such file");\n'
        '    eprintln!("cut: {p}: is a directory");\n'))
    write(root, "scripts/quote-names-baseline.txt",
          _QN_BASELINE + f"{_QN_FILE}:2\n")
    sha = commit(root)
    write(root, "scripts/quote-names-baseline.txt",
          _QN_BASELINE + f"{_QN_FILE}:1\n")

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk's lower count makes the second site new",
          disk.returncode, 1)
    check("gate 8: the commit's own count forgives both", rev.returncode, 0)


def case_gate8_a_file_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The third input: the enumeration, not only the contents.

    Every case above edits a file that exists on both sides, so a checker
    listing `.rs` files from the disk and reading their text from the revision
    passes all of them. Here the leaking file is gone from the working tree
    entirely -- a branch since tidied, or a file that exists only in what is
    being pushed.
    """
    root = _quote_repo(tmp, "g8d")
    write(root, _QN_FILE, _QN_LEAK)
    sha = commit(root)
    remove(root, _QN_FILE)

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk has no such file to judge", disk.returncode, 0)
    check("gate 8: the commit still has it, and is refused", rev.returncode, 1)
    check("gate 8: ...naming the file the disk lacks",
          "tool.rs" in rev.stdout + rev.stderr, True)


def case_gate8_the_shrunk_half_of_the_ratchet_describes_the_commit(tmp: str) -> None:
    """The other direction through the same two inputs.

    Unlike gates 4 and 6, a stale baseline entry does not *fail* here -- it
    prints `fixed: <path> N -> M` and the run still exits 0, because this
    ratchet's entries are counts that shrink one site at a time rather than
    findings that are either present or not. That makes the report the only
    observable, and it is worth pinning for the reason the corpus guard exists:
    `fixed:` is an invitation to run `--write-baseline`, so it had better be
    describing the tree being published rather than whatever the author has
    open.

    Here the repair is committed and the disk still has the defect. The
    commit's entry is the stale one and must be reported; the disk's is live
    and must not be.
    """
    root = _quote_repo(tmp, "g8f")
    write(root, _QN_FILE, _QN_LEAK)
    write(root, "scripts/quote-names-baseline.txt",
          _QN_BASELINE + f"{_QN_FILE}:1\n")
    commit(root)
    write(root, _QN_FILE, _QN_OK)
    sha = commit(root, "repair it")
    write(root, _QN_FILE, _QN_LEAK)

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk's site is still there, so nothing is reported fixed",
          "fixed:" in disk.stdout, False)
    check("gate 8: ...and the disk run passes on its own baseline",
          disk.returncode, 0)
    check("gate 8: the commit's baseline entry is dead and is reported so",
          "fixed:" in rev.stdout, True)
    check("gate 8: ...naming the file whose count the commit dropped",
          "tool.rs" in rev.stdout, True)


def case_gate8_build_output_is_skipped_on_the_side_that_can_see_it(tmp: str) -> None:
    """The rule only the disk arm can break, so only it can pin.

    A revision never lists `target/`, so the skip is unobservable there. The
    working-tree arm reaches the disk through `gittree.WorkTree`, which prunes
    build directories by path component; if that is ever swapped back for a
    plain walk, this gate starts reporting generated sources in a directory of
    tens of gigabytes and becomes something people bypass rather than fix.

    The control carries the weight: the same bytes one directory across must
    still be found, or "nothing reported" would be satisfied by a checker that
    had stopped reading the disk at all.
    """
    root = _quote_repo(tmp, "g8g")
    sha = commit(root)
    # After the commit, so it is on the disk and in no revision -- which is
    # what build output is.
    write(root, "userspace/coreutils/target/debug/build/gen.rs", _QN_LEAK)

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: generated sources under target/ are not judged",
          (disk.returncode, rev.returncode), (0, 0))

    write(root, "userspace/coreutils/notes/gen.rs", _QN_LEAK)
    disk2 = run_checker(root, "quote-names.py", "--check")
    check("gate 8: ...while the same bytes elsewhere on the disk are",
          disk2.returncode, 1)


def case_gate8_a_tree_with_no_corpus_is_not_a_clean_tree(tmp: str) -> None:
    """The measured defect, as a per-tree question.

    Reported clean -- and worse than clean. Against a baseline still listing a
    file, an emptied corpus printed `fixed: ... 1 -> 0` and
    `ok -- 0 known sites in 0 files (1 improved)` and exited 0: the loss of the
    gate's own subject read as a burn-down, over wording that invites a
    `--write-baseline` discarding every site the ratchet guards.

    It belongs in *this* suite rather than only in the checker's self-test
    because it is a question about which tree is being read. The commit is what
    disarms the gate; the working tree -- where the author is mid-rename, or
    has simply not deleted the old directory yet -- still has the corpus and
    answers that all is well. Exit 2, not 1: the gate has lost its subject
    rather than found a defect, and gate 8's refusal tells an author their
    diagnostics leak names.
    """
    root = _quote_repo(tmp, "g8h")
    write(root, "scripts/quote-names-baseline.txt",
          _QN_BASELINE + f"{_QN_FILE}:1\n")
    write(root, _QN_FILE, _QN_LEAK)
    commit(root)
    remove(root, _QN_FILE)
    remove(root, "userspace/coreutils/src/bin/clean.rs")
    sha = commit(root, "lane B's zone goes somewhere else")
    write(root, "userspace/coreutils/src/bin/clean.rs", _QN_OK)
    write(root, _QN_FILE, _QN_LEAK)

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk still has a corpus and is judged normally",
          disk.returncode, 0)
    check("gate 8: the commit has none, which is no verdict rather than a pass",
          rev.returncode, 2)
    check("gate 8: ...saying so, rather than exiting quietly",
          "lost its subject" in rev.stderr, True)
    # The load-bearing negative, and the exact shape of the measured failure:
    # the run must not congratulate anyone on a burn-down it did not observe.
    check("gate 8: ...and the vanished corpus is not reported as progress",
          "improved" in rev.stdout, False)


def case_gate8_a_baseline_absent_from_the_tree_is_not_a_pile_of_new_findings(
        tmp: str) -> None:
    """The second input's version of the guard above, failing the other way.

    A missing corpus goes silent; a missing *baseline* goes loud and wrong. It
    reads back as an empty allowance rather than as an error, so every site the
    real file forgives becomes a NEW finding and the push is refused with gate
    8's whole refusal over diagnostics nobody touched. On the real tree that is
    1798 sites across 777 files.

    The checker's comment argued the empty read was "the safe direction -- it
    can only over-report". That is the argument `run-checker.sh` exists to
    reject: a false accusation is not the safe direction, it is the failure
    that gets a gate bypassed, and a bypassed gate protects nothing. Gates 4
    and 6 both exit 2 here; gate 8 shipped its conversion without the guard,
    and this case is the one that was missing transposed across.

    Per-tree for the corpus guard's reason: a commit moves the baseline, and
    the disk still has it.
    """
    # The allowance must be non-empty, or this case cannot see the failure it
    # is named for. Against an empty baseline a missing one is merely a silent
    # false pass -- so the fixture forgives a real site, and taking the
    # baseline away turns that forgiven site into an accusation.
    root = _quote_repo(tmp, "g8i")
    write(root, _QN_FILE, _QN_LEAK)
    write(root, "scripts/quote-names-baseline.txt",
          _QN_BASELINE + f"{_QN_FILE}:1\n")
    commit(root)
    remove(root, "scripts/quote-names-baseline.txt")
    sha = commit(root, "the baseline goes somewhere else")
    write(root, "scripts/quote-names-baseline.txt",
          _QN_BASELINE + f"{_QN_FILE}:1\n")

    disk = run_checker(root, "quote-names.py", "--check")
    rev = run_checker(root, "quote-names.py", "--check", "--head", sha)
    check("gate 8: the disk's baseline still forgives the site it names",
          disk.returncode, 0)
    check("gate 8: the commit's is gone, which is no verdict", rev.returncode, 2)
    check("gate 8: ...naming the file rather than blaming a diagnostic",
          "quote-names-baseline.txt" in rev.stderr, True)
    # Without the guard this run does not go quiet, it goes wrong: the forgiven
    # site reads as new and the author is told a file they never touched leaks
    # names into stderr.
    check("gate 8: ...and the forgiven site is not accused of being new",
          "NEW diagnostic" in rev.stdout + rev.stderr, False)
    # The guard must not stop the one mode whose job is to create the file.
    # Checked on the disk arm: `--write-baseline` writes the working tree and
    # is refused outright with `--head`, so the revision arm cannot say this.
    remove(root, "scripts/quote-names-baseline.txt")
    boot = run_checker(root, "quote-names.py", "--write-baseline")
    check("gate 8: a bootstrap run still creates the baseline it lacks",
          boot.returncode, 0)


def case_gate8_a_baseline_cannot_be_written_from_a_revision(tmp: str) -> None:
    """`--write-baseline --head` is refused, not quietly resolved either way.

    Recording a past commit's counts as the current allowance would un-fix
    everything repaired since; writing the *worktree's* counts while claiming
    to have read a revision would be worse, because the output would name a sha
    it did not use. Refusing is the only answer that is not a lie, and it is
    asserted here so a later tidy-up cannot pick one of the other two.
    """
    root = _quote_repo(tmp, "g8j")
    sha = commit(root)
    proc = run_checker(root, "quote-names.py", "--write-baseline", "--head", sha)
    check("gate 8: --write-baseline with --head is refused", proc.returncode, 2)
    check("gate 8: ...saying which two flags cannot be combined",
          "--write-baseline" in proc.stderr, True)


def case_gate8_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1 -- the contract every converted gate has with run-checker.sh.

    Exit 1 gets gate 8's refusal printed over it, which tells the author a
    diagnostic of theirs hands a file name control of stderr and offers the
    bypass. A revision that could not be read says nothing about anybody's
    diagnostics.

    The message is asserted as well as the status, and here that matters more
    than usual: this gate now has *two* routes to exit 2 -- an unreadable
    revision and a tree with no corpus -- and they want opposite responses from
    whoever reads the log. Naming the revision is what separates them.
    """
    root = _quote_repo(tmp, "g8k")
    commit(root)
    proc = run_checker(root, "quote-names.py", "--check", "--head", "nosuchrev")
    check("gate 8: an unreadable --head exits 2, not 1", proc.returncode, 2)
    check("gate 8: ...naming the revision it could not read",
          "nosuchrev" in proc.stderr, True)


# --------------------------------------------------------------------------
# Gate 9 -- check-requests-not-deleted.py
#
# The defect it looks for: a request file removed rather than stamped. Unlike
# every gate above it there is no corpus and no ratchet -- its subject is a
# *diff*, `base..head` under `requests/`, so the shape of a case is a two-commit
# history rather than a tree that disagrees with the disk about its contents.
#
# `--base` is passed explicitly throughout. Left to itself the checker resolves
# a merge base against `origin/main` or `main`, which in a fixture depends on
# what `init.defaultBranch` happens to be on the host; pinning it keeps every
# case below about the two inputs that are actually under test -- which commit
# is judged, and where the waiver list comes from.
# --------------------------------------------------------------------------

_G9_CHECKER = "check-requests-not-deleted.py"
_G9_REQ = "requests/a-b-one.md"
_G9_KEEP = "requests/a-b-two.md"


def _reqdel_repo(tmp: str, name: str) -> tuple[str, str]:
    """A repository with two requests committed. Returns (root, base sha).

    `a-b-two.md` is in every fixture and is never the subject of a case. It is
    there so that "no deletions found" can never be reached by `requests/`
    being empty or unmatched by the path filter -- the failure mode this
    checker's own docstring calls out as reading exactly like a healthy tree.
    """
    root = new_repo(tmp, name, (_G9_CHECKER,))
    write(root, _G9_REQ, "# a request\n\n**Status:** OPEN\n")
    write(root, _G9_KEEP, "# another request\n\n**Status:** OPEN\n")
    return root, commit(root)


def case_gate9_a_staged_restore_cannot_hide_a_committed_deletion(tmp: str) -> None:
    """The silent half, and the reason `--head` was added to this gate.

    A *staged* restore, not merely a present file: `git diff <base>` compares
    against the index-plus-worktree view, so an untracked file is simply absent
    and hides nothing. `git add` is the ordinary way to put a file back, and a
    merge that reintroduces one stages it for you -- which is what makes this
    reachable by accident rather than only by intent.
    """
    root, base = _reqdel_repo(tmp, "g9a")
    remove(root, _G9_REQ)
    sha = commit(root, "delete a request")
    write(root, _G9_REQ, "# restored, uncommitted\n")
    git(root, "add", _G9_REQ)

    disk = run_checker(root, _G9_CHECKER, "--base", base)
    rev = run_checker(root, _G9_CHECKER, "--base", base, "--head", sha)
    check("gate 9: the staged restore hides the deletion from the disk",
          disk.returncode, 0)
    check("gate 9: ...and the commit being pushed is refused anyway",
          rev.returncode, 1)
    check("gate 9: ...naming the request the commit removes",
          "a-b-one.md" in rev.stdout + rev.stderr, True)


def case_gate9_an_uncommitted_deletion_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: a file removed on the disk, nothing removed in the commit.

    The mirror image matters as much as the case above. A gate that blocks a
    push of unrelated clean commits over a local `rm` is a gate whose bypass
    variable gets exported in a shell profile, after which it protects nothing.
    """
    root, base = _reqdel_repo(tmp, "g9b")
    write(root, "requests/a-b-three.md", "# a third request\n")
    sha = commit(root, "file another request")
    remove(root, _G9_REQ)

    disk = run_checker(root, _G9_CHECKER, "--base", base)
    rev = run_checker(root, _G9_CHECKER, "--base", base, "--head", sha)
    check("gate 9: the disk refuses the uncommitted deletion", disk.returncode, 1)
    check("gate 9: ...but the commit being pushed removes nothing",
          rev.returncode, 0)


def case_gate9_the_allowlist_is_read_from_the_same_tree(tmp: str) -> None:
    """The second input, and the one the conversion left behind.

    `requests/.deletions-allowed` is a *waiver* list: a basename in it turns
    this gate's refusal into a note. Read off the disk while the diff is read
    from the commit, an author can waive a deletion in a file nobody is
    publishing -- write the basename, push the deletion, drop the edit -- and
    the request is gone from shared history with no record that a waiver was
    ever claimed. That is worse than the staged-restore hole this gate's
    `--head` was added for, because a staged restore leaves the file present
    somewhere and this leaves nothing at all.

    The hook already argues the point and enforces the wrong half of it: gate
    9's `touches` includes `requests/.deletions-allowed` so that "editing the
    waiver list must be the push that re-verifies it". Re-verifying it against
    the working tree's copy is not that.
    """
    root, base = _reqdel_repo(tmp, "g9c")
    remove(root, _G9_REQ)
    sha = commit(root, "delete a request, waiving nothing")
    check("gate 9: the fixture starts refused",
          run_checker(root, _G9_CHECKER, "--base", base,
                      "--head", sha).returncode, 1)

    # The waiver that exists only on the disk.
    write(root, "requests/.deletions-allowed",
          "# waivers\na-b-one.md  # folded into a-b-two.md\n")

    rev = run_checker(root, _G9_CHECKER, "--base", base, "--head", sha)
    check("gate 9: an uncommitted waiver does not excuse a committed deletion",
          rev.returncode, 1)

    # The control: committed, the same waiver must work. Without this the
    # assertion above is met by a checker that ignores the allowlist entirely,
    # which would be a different bug with the same green.
    committed = commit(root, "record the waiver")
    ok = run_checker(root, _G9_CHECKER, "--base", base, "--head", committed)
    check("gate 9: ...while a committed waiver does", ok.returncode, 0)
    check("gate 9: ...quoting the reason it was given",
          "folded into a-b-two.md" in ok.stdout, True)


def case_gate9_a_rename_is_not_a_deletion_in_either_tree(tmp: str) -> None:
    """The control on `-M`, per tree.

    Fixing a slug or sweeping into an archive directory must pass, or the gate
    makes tidying impossible and gets bypassed for the wrong reason. It is
    per-tree because `-M` is computed over the pair of trees being diffed: a
    checker that took the rename detection from one pair and the deletion list
    from another would report the move as a disappearance.
    """
    root, base = _reqdel_repo(tmp, "g9d")
    git(root, "mv", _G9_REQ, "requests/a-b-one-renamed.md")
    sha = commit(root, "fix the slug")

    disk = run_checker(root, _G9_CHECKER, "--base", base)
    rev = run_checker(root, _G9_CHECKER, "--base", base, "--head", sha)
    check("gate 9: a rename is not a deletion, on either side",
          (disk.returncode, rev.returncode), (0, 0))


def case_gate9_the_merge_base_is_taken_against_the_commit_being_judged(
        tmp: str) -> None:
    """The third input: the base, when nobody passes one.

    Every case above pins `--base`, so none of them can see this. Left to
    itself the checker resolves a merge base -- and it must resolve it against
    the commit it is *judging*, not against `HEAD`. Using HEAD's merge base
    while diffing another commit compares two unrelated points and reports
    every request that differs between them, which on a real branch is a list
    of other people's files.

    The fixture is the off-branch push in miniature: `main` carries a request
    that the branch under judgement never had, so a base taken from HEAD would
    make that request look deleted by a commit that never touched it.
    """
    root, _ = _reqdel_repo(tmp, "g9e")
    git(root, "branch", "-M", "main")
    git(root, "checkout", "--quiet", "-b", "feature")
    write(root, "requests/a-b-four.md", "# filed on the branch\n")
    sha = commit(root, "file a request on the branch")
    git(root, "checkout", "--quiet", "main")
    # A request that exists only on `main`. If the base came from HEAD -- which
    # is `main` -- the diff against `feature` would call this one deleted.
    write(root, "requests/a-b-five.md", "# filed on main\n")
    commit(root, "file a request on main")

    rev = run_checker(root, _G9_CHECKER, "--head", sha)
    check("gate 9: a branch other than HEAD is judged against its own base",
          rev.returncode, 0)
    check("gate 9: ...so main's own request is not reported as deleted by it",
          "a-b-five" in rev.stdout + rev.stderr, False)


def case_gate9_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1 -- the contract every converted gate has with run-checker.sh.

    Exit 1 gets gate 9's refusal printed over it, which tells the author they
    deleted a request, offers a `git checkout` to restore it, and explains how
    to stamp it. A revision that could not be read says nothing about anybody's
    requests, and the restore command it would print names a base it never
    resolved.
    """
    root, base = _reqdel_repo(tmp, "g9f")
    proc = run_checker(root, _G9_CHECKER, "--base", base, "--head", "nosuchrev")
    check("gate 9: an unreadable --head exits 2, not 1", proc.returncode, 2)
    check("gate 9: ...naming the revision it could not read",
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
    for script in (*SUPPORT, *checkers):
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
# Gate 5's second line, not its first: the first is "REFUSING to push — a
# long-option table disagrees with GNU's", and the em dash in it is the thing
# the comment above rules out.
_G5_REFUSAL = "does not match the one the real utility carries"
# Gate 6's own refusal sentence, not its summary line: "prints the host's error
# text" also occurs in the *checker's* FIX advice, which is printed by a
# `--check` run that the hook then goes on to allow. Matching it would call a
# fixture refused on the strength of text from a gate that did not refuse it.
_G6_REFUSAL = "The message is an interface. Anything that greps"
# Gate 8's refusal sentence, not its summary line. The summary is "REFUSING to
# push — a diagnostic above puts a file name straight into its message", whose
# em dash the comment above rules out; and the shorter phrases in it recur in
# the *checker's* own advice, which a run that the hook then allows still
# prints. "hands the name control of stderr" occurs once, in the hook, in the
# block that exits 1.
_G8_REFUSAL = "hands the name control of stderr"
# Gate 9's refusal paragraph, not its summary line. The summary is "REFUSING to
# push — a commit being published deletes a file under requests/", em dash and
# all; and "was deleted" is printed by the *checker*, which says it once per
# violation whether or not the hook goes on to refuse. This clause is in the
# heredoc that precedes `exit 1`, and nowhere else in the hook.
_G9_REFUSAL = "the argument that settled something"


def _push(work: str, ref: str = "main",
          marker: str = _G2_REFUSAL,
          extra_refs: tuple[str, ...] = ()) -> tuple[str, str]:
    """(verdict, output). `allowed`, `refused`, or `error:<...>`.

    Only the *named gate's* refusal counts as `refused`. A suite that accepted
    any refusal would pass on a fixture that trips some other gate and never
    reach the thing it is about.

    `extra_refs` pushes more than one branch in a single `git push`, which is
    the only way to make the hook's `for sha in $pushed_shas` loop run more than
    once: the list holds one sha per *ref*, not one per commit. A gate that read
    `${pushed_shas# }` as a single sha -- which one block of the hook genuinely
    does, guarded -- passes every single-ref fixture in this file.

    `ALLOW_FMT_DRIFT=1` because the fixture's `.rs` files are hand-written and
    gate 7 would rustfmt them; `test-pre-push-fmt-gate.py` covers that gate
    properly. Nothing else is bypassed -- the other gates skip themselves
    because this fixture does not install their checkers.
    """
    env = gitenv.clean_env()
    env["ALLOW_FMT_DRIFT"] = "1"
    proc = subprocess.run(["git", "push", "origin", ref, *extra_refs],
                          cwd=work, env=env,
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


def _g8_push_fixture(tmp: str, name: str) -> str:
    """A push fixture for gate 8, with lane B's zone already published.

    Only `quote-names.py` is installed, so every other gate stands down and a
    refusal here can only have come from gate 8. `clean.rs` is in the seed for
    the checker-level fixtures' reason -- so that a green run can never be
    reached by the gate finding no corpus -- and the baseline is seeded with it
    because gate 8 now refuses a tree that has no baseline at all, which would
    otherwise make the seed push itself the thing that fails.
    """
    return _push_fixture(
        tmp, name, checkers=("quote-names.py",),
        seed={"scripts/quote-names-baseline.txt": _QN_BASELINE,
              "userspace/coreutils/src/bin/clean.rs": _QN_OK},
    )


def case_gate8_the_hook_refuses_a_commit_the_worktree_no_longer_shows(
        tmp: str) -> None:
    """End to end: gate 8's own wiring, not some other gate's.

    Each gate is a separate block with its own guard, its own loop and its own
    `--head "$sha"`, so dropping the flag from *this* invocation leaves every
    other case in this file green. That is the argument for a per-gate
    end-to-end case, and gate 8 is the reason the argument is not theoretical:
    it was converted on 2026-09-02, wired correctly, asserted to be wired by
    `test-pre-push-gates.py`, and had no case like this one until 2026-09-04.
    """
    work = _g8_push_fixture(tmp, "g8push-hide")
    write(work, _QN_FILE, _QN_LEAK)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bin that interpolates a name")
    # The tidy-up that makes the disk lie.
    write(work, _QN_FILE, _QN_OK)

    verdict, blob = _push(work, marker=_G8_REFUSAL)
    check("gate 8 end to end: the push is refused", verdict, "refused")
    check("gate 8 end to end: ...naming the file only the commit breaks",
          "tool.rs" in blob, True)


def case_gate8_the_hook_allows_a_clean_commit_under_a_dirty_worktree(
        tmp: str) -> None:
    """End to end, the other direction -- and the one that checks it ran.

    A gate that had skipped itself would allow this, and would have allowed the
    case above too if that refusal came from elsewhere. The hook's own tally is
    the only thing separating "gate 8 passed" from "gate 8 was never asked", and
    gate 8 has three ways to stand down: the `ALLOW_UNQUOTED_NAMES` bypass, a
    `touches` scope that no file in the push matched, and an empty pushed-sha
    list.
    """
    work = _g8_push_fixture(tmp, "g8push-wip")
    write(work, "userspace/coreutils/src/bin/other.rs", _QN_OK)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bin that quotes the name properly")
    write(work, _QN_FILE, _QN_LEAK)

    verdict, blob = _push(work, marker=_G8_REFUSAL)
    check("gate 8 end to end: an uncommitted leak does not block",
          verdict, "allowed")
    check("gate 8 end to end: ...and the gate actually ran",
          "quote-names" in _tally(blob)[0], True)


def case_gate8_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` from `main`, for gate 8's loop.

    Every case that pushes the branch it is standing on has `HEAD` and the sha
    being pushed as the same commit, so `--head "$sha"` there is
    indistinguishable from `git rev-parse HEAD` -- the blind spot that hid the
    `touches` defect until 2026-09-02 and gate 5's private scope until
    2026-09-04. It is per gate: gate 6's off-branch case says nothing about
    gate 8's loop.
    """
    work = _g8_push_fixture(tmp, "g8push-elsewhere")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, _QN_FILE, _QN_LEAK)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "an unquoted name on a branch we leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G8_REFUSAL)
    check("gate 8 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 8 end to end: ...naming the file on that other branch",
          "tool.rs" in blob, True)


def _g9_push_fixture(tmp: str, name: str) -> str:
    """A push fixture for gate 9, with two requests already published.

    Only `check-requests-not-deleted.py` is installed, so a refusal here can
    only be gate 9's. Two requests rather than one for the checker-level
    fixtures' reason: `a-b-two.md` is never the subject of a case, so "no
    deletions found" can never be reached by `requests/` being empty or
    unmatched by the path filter -- the failure mode that reads exactly like a
    healthy tree.

    The requests are published in the seed commit, which matters more here than
    in the other fixtures: this gate diffs against the merge base with
    `origin/main`, so a request that was never pushed is not a request the base
    has, and deleting it would be invisible rather than refused.
    """
    return _push_fixture(
        tmp, name, checkers=("check-requests-not-deleted.py",),
        seed={_G9_REQ: "# a request\n\n**Status:** OPEN\n",
              _G9_KEEP: "# another request\n\n**Status:** OPEN\n"},
    )


def case_gate9_the_hook_refuses_a_commit_the_worktree_no_longer_shows(
        tmp: str) -> None:
    """End to end: gate 9's own wiring, and its own loop.

    Gate 9 is the only gate in the hook that calls its checker once per pushed
    sha -- the others hand their checker a single revision. So it has a way to
    be wrong that no other gate's end-to-end case can see: a loop that iterates
    zero times prints nothing, refuses nothing, and still reports the gate as
    having run.
    """
    work = _g9_push_fixture(tmp, "g9push-hide")
    remove(work, _G9_REQ)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "delete a landed request")
    # The tidy-up that makes the disk lie. Staged, because `git diff <base>`
    # cannot see an untracked file and an unstaged restore would hide nothing.
    write(work, _G9_REQ, "# restored, uncommitted\n")
    git(work, "add", _G9_REQ)

    verdict, blob = _push(work, marker=_G9_REFUSAL)
    check("gate 9 end to end: the push is refused", verdict, "refused")
    check("gate 9 end to end: ...naming the request only the commit removes",
          "a-b-one.md" in blob, True)


def case_gate9_the_hook_allows_a_clean_commit_under_a_dirty_worktree(
        tmp: str) -> None:
    """End to end, the other direction -- and the one that checks it ran.

    Gate 9 has four ways to stand down: the `ALLOW_REQUEST_DELETION` bypass, a
    `touches` scope matching neither `requests/` nor the checker nor the
    allowlist, an empty pushed-sha list, and the checker's own self-test
    failing. Any of them allows this push, and three of them would have allowed
    the case above too.
    """
    work = _g9_push_fixture(tmp, "g9push-wip")
    write(work, "requests/a-b-three.md", "# a third request\n")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "file another request")
    remove(work, _G9_REQ)

    verdict, blob = _push(work, marker=_G9_REFUSAL)
    check("gate 9 end to end: an uncommitted deletion does not block",
          verdict, "allowed")
    check("gate 9 end to end: ...and the gate actually ran",
          "request-deletion" in _tally(blob)[0], True)


def case_gate9_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` from `main`, for gate 9's loop.

    Every case that pushes the branch it is standing on has `HEAD` and the sha
    being pushed as the same commit, so `--head "$sha"` there is
    indistinguishable from reading `HEAD` -- and for this gate the merge base is
    a second thing that would silently come from the wrong place. The checker
    resolves it against whatever it is judging; if that were `HEAD` instead, the
    diff would compare two unrelated points and report `main`'s own files.
    """
    work = _g9_push_fixture(tmp, "g9push-elsewhere")
    git(work, "checkout", "--quiet", "-b", "feature")
    remove(work, _G9_REQ)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "delete a request on a branch we leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G9_REFUSAL)
    check("gate 9 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 9 end to end: ...naming the request on that other branch",
          "a-b-one.md" in blob, True)


def _g5_push_fixture(tmp: str, name: str) -> str:
    """A push fixture for gate 5, with nothing under `bin/` yet.

    Seeded deliberately *outside* `userspace/coreutils/src/bin/`, which is the
    gate's scope: at seed time the gate must find nothing to compare and stand
    down, so the seed push cannot be refused by the gate under test. A fixture
    whose setup is refused does not fail, it lies -- it pushes nothing, and the
    seed commit then rides along inside the case's own push and widens it.
    """
    return _push_fixture(
        tmp, name, checkers=("getopt-ambiguity-check.py",),
        seed={"userspace/coreutils/src/notes.txt": "not a bin\n"},
    )


def case_gate5_the_hook_refuses_a_commit_the_worktree_no_longer_shows(
        tmp: str) -> None:
    """End to end: gate 5's own wiring, not another gate's.

    Each gate is a separate block with its own guard, its own loop and its own
    `--head "$sha"`. Dropping the flag from *this* invocation leaves every
    other case in this file green, which is why the proof is per gate.
    """
    work = _g5_push_fixture(tmp, "g5push-hide")
    write(work, "userspace/coreutils/src/bin/yes.rs", _YES_BROKEN)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a table with an option dropped")
    # The tidy-up that makes the disk lie.
    write(work, "userspace/coreutils/src/bin/yes.rs", _YES_OK)

    if _gnu_userland_missing(work):
        print("  SKIP gate 5 end to end: no GNU userland on this host")
        return
    verdict, blob = _push(work, marker=_G5_REFUSAL)
    check("gate 5 end to end: the push is refused", verdict, "refused")
    check("gate 5 end to end: ...naming the option only the commit drops",
          "version" in blob, True)


def case_gate5_the_hook_allows_a_clean_commit_under_a_dirty_worktree(
        tmp: str) -> None:
    """End to end, the other direction -- and the one that checks it ran.

    A gate that had skipped itself would allow this, and would have allowed the
    case above too if that refusal came from elsewhere. The hook's own tally is
    what separates "gate 5 passed" from "gate 5 was never asked", and this gate
    has three ways to skip: no GNU userland, a scope that names no bin, and an
    empty pushed-sha list.
    """
    work = _g5_push_fixture(tmp, "g5push-wip")
    write(work, "userspace/coreutils/src/bin/yes.rs", _YES_OK)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a table that agrees with GNU")
    write(work, "userspace/coreutils/src/bin/yes.rs", _YES_BROKEN)

    if _gnu_userland_missing(work):
        print("  SKIP gate 5 end to end: no GNU userland on this host")
        return
    verdict, blob = _push(work, marker=_G5_REFUSAL)
    check("gate 5 end to end: an uncommitted table edit does not block",
          verdict, "allowed")
    check("gate 5 end to end: ...and the gate actually ran",
          "getopt-table" in _tally(blob)[0], True)


def case_gate5_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` from `main`, for gate 5's loop.

    Every case that pushes the branch it is standing on has `HEAD` and the sha
    being pushed as the same commit, so `--head "$sha"` there is
    indistinguishable from `git rev-parse HEAD`. It is per gate: gate 6's
    off-branch case says nothing about gate 5's loop.
    """
    work = _g5_push_fixture(tmp, "g5push-elsewhere")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, "userspace/coreutils/src/bin/yes.rs", _YES_BROKEN)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a dropped option on a branch we leave")
    git(work, "checkout", "--quiet", "main")

    if _gnu_userland_missing(work):
        print("  SKIP gate 5 end to end: no GNU userland on this host")
        return
    verdict, blob = _push(work, "feature", marker=_G5_REFUSAL)
    check("gate 5 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 5 end to end: ...naming the option on that other branch",
          "version" in blob, True)


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


# --------------------------------------------------------------------------
# Gate 11 -- check-doc-links.py
#
# The defect it looks for: `/// See [`old_name`]` left behind by a rename, so
# the link's final segment names nothing anywhere in the crate that compiles
# the file. One identifier decides it, which makes a commit and a worktree easy
# to make disagree.
#
# What it reads from a tree, and therefore what has to be made to differ:
#
#   the crate list      `crate_roots` -- which `Cargo.toml`s exist. Also the
#                       scope resolver: `--paths-from` names *paths*, and a
#                       path is turned into a crate against this list, so a
#                       crate the disk has never heard of silently reduces the
#                       scope to nothing and prints a pass.
#   the unit list       `units` -- which `.rs` files are under `src`, and which
#                       of them are `src/bin` targets. A package is not a
#                       namespace here: each bin is judged against its own
#                       definitions plus the library's, so *which* file is a
#                       bin changes the answer.
#   the library's text  the shared definition set, read once per crate and
#                       unioned into every bin. It is a separate read from the
#                       scanned file's own, and converting only the latter
#                       leaves every contents-only case below green.
#   the manifest        `[dependencies]` names resolve as links. That makes
#                       `Cargo.toml` a *suppression* input, with gate 6's
#                       baseline problem: leaving it on the disk is a false
#                       pass whose symptoms are identical to a clean tree.
# --------------------------------------------------------------------------

_DL_OK = '''\
/// See [`real`] for the rule this follows.
pub fn real() {}
'''

_DL_DEAD = '''\
/// See [`ghost_link_target`] for the rule this follows.
pub fn real() {}
'''

# A link to a name only the manifest can supply, for the suppression case.
_DL_DEP_LINK = '''\
/// Wraps [`ghostdep`], which does the actual work.
pub fn real() {}
'''

# A link to a name only the *library* can supply, for the shared-definitions
# case. Lives in a bin, so it is judged against `own | lib` rather than `own`.
_DL_SHARED_LINK = '''\
/// Delegates to [`shared_helper`] once the arguments are parsed.
pub fn go() {}
'''

_DL_MANIFEST = '[package]\nname = "real"\n'
_DL_MANIFEST_DEP = '[package]\nname = "real"\n\n[dependencies]\nghostdep = "1"\n'


def _doclinks_repo(tmp: str, name: str) -> str:
    """A crate under a scanned root with one clean, linked library file.

    `lib.rs` carries a *live* link rather than no link at all, for `_argv_repo`'s
    reason one step further in: "the checker found nothing" must not be
    reachable by the checker finding no files, and here it must also not be
    reachable by it finding no *links*. A fixture whose only doc comment is the
    one under test cannot tell a working scan from a scan that stopped reading
    doc comments entirely.
    """
    root = new_repo(tmp, name, ("check-doc-links.py",))
    write(root, "userspace/real/Cargo.toml", _DL_MANIFEST)
    write(root, "userspace/real/src/lib.rs", _DL_OK)
    return root


def case_gate11_a_tidied_worktree_cannot_hide_a_committed_dead_link(tmp: str) -> None:
    """The silent half: the commit points at a name that is gone, the disk does not."""
    root = _doclinks_repo(tmp, "g11a")
    write(root, "userspace/real/src/bin/tool.rs", _DL_DEAD)
    sha = commit(root)
    write(root, "userspace/real/src/bin/tool.rs", _DL_OK)

    disk = run_checker(root, "check-doc-links.py", "--check")
    rev = run_checker(root, "check-doc-links.py", "--check", "--head", sha)
    check("gate 11: the disk's links all resolve", disk.returncode, 0)
    check("gate 11: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 11: ...naming the target the commit cannot resolve",
          "ghost_link_target" in rev.stdout + rev.stderr, True)


def case_gate11_an_uncommitted_dead_link_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: a half-finished rename on the disk, nothing wrong in the commit."""
    root = _doclinks_repo(tmp, "g11b")
    write(root, "userspace/real/src/bin/tool.rs", _DL_OK)
    sha = commit(root)
    write(root, "userspace/real/src/bin/tool.rs", _DL_DEAD)

    disk = run_checker(root, "check-doc-links.py", "--check")
    rev = run_checker(root, "check-doc-links.py", "--check", "--head", sha)
    check("gate 11: the disk refuses the uncommitted dead link", disk.returncode, 1)
    check("gate 11: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate11_the_manifest_is_read_from_the_same_tree(tmp: str) -> None:
    """The suppression input a sources-only conversion leaves behind.

    A `[dependencies]` name links like a crate root -- `modechange`'s docs point
    at `ere`, and that is a working link no amount of reading `src/` can
    confirm. So the manifest silences findings, and reading it from the disk is
    gate 6's baseline problem exactly: a dependency present only on the disk
    forgives a commit that has no such dependency, and the push publishes a link
    that renders as literal text.
    """
    root = _doclinks_repo(tmp, "g11c")
    write(root, "userspace/real/src/bin/tool.rs", _DL_DEP_LINK)
    sha = commit(root)
    check("gate 11: the fixture starts refused",
          run_checker(root, "check-doc-links.py", "--check",
                      "--head", sha).returncode, 1)

    # Declare the dependency in a *commit*, and undeclare it on the *disk*.
    write(root, "userspace/real/Cargo.toml", _DL_MANIFEST_DEP)
    sha = commit(root, "depend on it")
    write(root, "userspace/real/Cargo.toml", _DL_MANIFEST)

    disk = run_checker(root, "check-doc-links.py", "--check")
    rev = run_checker(root, "check-doc-links.py", "--check", "--head", sha)
    check("gate 11: the disk's manifest declares nothing, so the link is dead",
          disk.returncode, 1)
    check("gate 11: the commit's manifest declares it, and it resolves",
          rev.returncode, 0)


def case_gate11_a_bin_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The enumeration, not only the contents.

    Both halves above edit a file present in both trees, so a checker listing
    `src/bin` from the disk and reading each entry's text from the revision
    passes them. Here the offending bin is not on the disk at all -- which is
    the ordinary state of a commit that adds a file and a worktree that has
    since had it deleted or moved.
    """
    root = _doclinks_repo(tmp, "g11d")
    write(root, "userspace/real/src/bin/tool.rs", _DL_DEAD)
    sha = commit(root)
    remove(root, "userspace/real/src/bin/tool.rs")

    disk = run_checker(root, "check-doc-links.py", "--check")
    rev = run_checker(root, "check-doc-links.py", "--check", "--head", sha)
    check("gate 11: the disk has no such bin and nothing to report",
          disk.returncode, 0)
    check("gate 11: ...and the commit's bin is judged regardless",
          rev.returncode, 1)


def case_gate11_the_librarys_definitions_are_read_from_the_same_tree(tmp: str) -> None:
    """The *other* read: the shared definition set, not the scanned file.

    Every case above is decided by the text of the file carrying the link. This
    one is decided by a file that carries no link at all: a bin is judged
    against its own definitions unioned with the library's, and `scan` reads
    those library files once per crate through a separate call. Converting
    `scan_file` and leaving `definitions` on the disk leaves all four cases
    above green while the library -- the half of the definition set that a
    rename most often moves -- is still read from whatever is lying around.
    """
    root = _doclinks_repo(tmp, "g11e")
    write(root, "userspace/real/src/lib.rs", "pub fn unrelated() {}\n")
    write(root, "userspace/real/src/bin/tool.rs", _DL_SHARED_LINK)
    sha = commit(root)
    # The helper the bin links to exists on the disk, and only there.
    write(root, "userspace/real/src/lib.rs", "pub fn shared_helper() {}\n")

    disk = run_checker(root, "check-doc-links.py", "--check")
    rev = run_checker(root, "check-doc-links.py", "--check", "--head", sha)
    check("gate 11: the disk's library defines the helper, so the link resolves",
          disk.returncode, 0)
    check("gate 11: the commit's library does not, and the link is dead",
          rev.returncode, 1)
    check("gate 11: ...naming the helper the commit's library lacks",
          "shared_helper" in rev.stdout + rev.stderr, True)


def case_gate11_a_crate_absent_from_the_disk_is_still_scanned(tmp: str) -> None:
    """The scope resolver, which fails *quietly* and in the passing direction.

    The hook does not ask for a whole-tree scan; it names the directories the
    push touched and lets `crates_touching` map them to crates. That mapping is
    done against the tree's own `Cargo.toml` list -- so if the list comes from
    the disk and the crate exists only in the commit, the scope reduces to zero
    crates and the checker prints "no scanned crate was touched" and exits 0.

    That is the worst failure mode in this file: not a wrong verdict but a
    *cheerful* one, on the exact push that adds a crate.
    """
    root = _doclinks_repo(tmp, "g11f")
    write(root, "userspace/extra/Cargo.toml", '[package]\nname = "extra"\n')
    write(root, "userspace/extra/src/lib.rs", _DL_DEAD)
    sha = commit(root)
    remove(root, "userspace/extra/Cargo.toml")
    remove(root, "userspace/extra/src/lib.rs")

    scope = "userspace/extra/src/lib.rs"
    disk = run_checker(root, "check-doc-links.py", "--check", scope)
    rev = run_checker(root, "check-doc-links.py", "--check", "--head", sha, scope)
    check("gate 11: the disk knows no such crate", disk.returncode, 0)
    # ...and says so in the words that make the false pass recognisable, rather
    # than in the words of a scan that ran and found nothing.
    check("gate 11: ...and it is the empty-scope pass, not a clean scan",
          "no scanned crate was touched" in disk.stdout, True)
    check("gate 11: the commit's crate is resolved and scanned", rev.returncode, 1)


def case_gate11_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """Exit 2, not 1 -- see gate 2's twin for why the distinction matters."""
    root = _doclinks_repo(tmp, "g11g")
    commit(root)
    proc = run_checker(root, "check-doc-links.py", "--check", "--head", "nosuchrev")
    check("gate 11: an unreadable --head exits 2, not 1", proc.returncode, 2)
    check("gate 11: ...naming the revision it could not read",
          "nosuchrev" in proc.stderr, True)


def case_gate11_a_tree_with_no_crates_is_not_a_tree_with_no_dead_links(tmp: str) -> None:
    """Gate 6's `_inputs_missing` rule, corpus half, for gate 11.

    A link is resolved *inside* a crate, so a tree holding no crate under any of
    the scanned roots gives this checker nothing to resolve against: the scan
    walks zero crates, finds zero findings, and reports a clean tree however
    broken the code in it is. That is a pass by accident, which is the one
    verdict a gate must not be able to reach.

    Per-tree, hence here: the *commit* is what disarms the gate -- an author
    part-way through moving `userspace/` still has it on disk, so the
    working-tree run answers that all is well while the thing being published
    has nothing in it at all.

    Note what is deliberately *not* asserted: this case removes the crate
    outright. A run merely *scoped* to nothing -- a push touching only
    `kernel/` -- keeps its pass, and
    `case_gate11_a_crate_absent_from_the_disk_is_still_scanned` pins that half.
    The two reach `scan` looking alike and must not be folded together.
    """
    root = _doclinks_repo(tmp, "g11h")
    commit(root)
    remove(root, "userspace/real/Cargo.toml")
    remove(root, "userspace/real/src/lib.rs")
    sha = commit(root, "the scanned roots hold no crate any more")
    write(root, "userspace/real/Cargo.toml", _DL_MANIFEST)
    write(root, "userspace/real/src/lib.rs", _DL_OK)

    disk = run_checker(root, "check-doc-links.py", "--check")
    rev = run_checker(root, "check-doc-links.py", "--check", "--head", sha)
    check("gate 11: the disk still has a crate and is judged normally",
          disk.returncode, 0)
    check("gate 11: the commit has none, which is no verdict rather than a pass",
          rev.returncode, 2)
    check("gate 11: ...saying so, rather than exiting quietly",
          "nothing to judge" in rev.stderr, True)


# Gate 11's refusal sentence. Deliberately not the checker's own finding line
# (`... names nothing in crate ...`), which is printed by a `--check` run the
# hook may then go on to allow -- gate 6's note records why that distinction is
# load-bearing.
_G11_REFUSAL = "links to a name that does not"

_G11_SEED = {
    "userspace/real/Cargo.toml": _DL_MANIFEST,
    "userspace/real/src/lib.rs": _DL_OK,
}


def _doclinks_push_fixture(tmp: str, name: str) -> str:
    return _push_fixture(tmp, name, ("check-doc-links.py",), dict(_G11_SEED))


def case_gate11_the_hook_refuses_a_commit_the_worktree_no_longer_shows(tmp: str) -> None:
    """End to end: gate 11's own wiring, not some other gate's.

    Gate 11 is the only converted gate that scopes itself with `--paths-from`
    rather than by scanning everything, so its loop has a second thing to get
    right: the file list and the `--head` must describe the *same* commit.
    Nothing in gates 2, 3, 4 or 6 exercises that pairing.
    """
    work = _doclinks_push_fixture(tmp, "g11push-hide")
    write(work, "userspace/real/src/bin/tool.rs", _DL_DEAD)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "link a name that is not there")
    write(work, "userspace/real/src/bin/tool.rs", _DL_OK)

    verdict, blob = _push(work, marker=_G11_REFUSAL)
    check("gate 11 end to end: the push is refused", verdict, "refused")
    check("gate 11 end to end: ...naming the target only the commit has",
          "ghost_link_target" in blob, True)


def case_gate11_the_hook_allows_a_clean_commit_under_a_dirty_worktree(tmp: str) -> None:
    """End to end: the false fail, and the proof the gate was actually asked.

    The `[ -d "$repo_root/$dl_file" ]` filter this gate used to carry is what
    makes the tally check more than a formality here: it dropped any pushed
    directory the worktree did not have, and a scope reduced to nothing is a
    gate that runs and cannot fail.
    """
    work = _doclinks_push_fixture(tmp, "g11push-wip")
    write(work, "userspace/real/src/bin/tool.rs", _DL_OK)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bin whose links all resolve")
    write(work, "userspace/real/src/bin/tool.rs", _DL_DEAD)

    verdict, blob = _push(work, marker=_G11_REFUSAL)
    check("gate 11 end to end: an uncommitted dead link does not block",
          verdict, "allowed")
    check("gate 11 end to end: ...and the gate actually ran",
          "doc-links" in _tally(blob)[0], True)


def case_gate11_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` while checked out on `main`.

    This gate derived its scope from `HEAD` rather than from `$pushed_shas`,
    which is a sharper version of the `touches` defect gate 2's twin describes:
    the list would come out empty, `[ -s ]` would set `skip_doclinks=1`, and the
    hook would report the gate as *skipped* -- a visible outcome nobody reads as
    a bug, on a push carrying exactly what the gate exists to catch.
    """
    work = _doclinks_push_fixture(tmp, "g11push-offbranch")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, "userspace/real/src/bin/tool.rs", _DL_DEAD)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a dead link on a branch we will leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G11_REFUSAL)
    check("gate 11 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 11 end to end: ...naming the target on that other branch",
          "ghost_link_target" in blob, True)


# --------------------------------------------------------------------------
# Gate 13 -- check-design-decisions-bands.py
#
# The defect it looks for: a section of `design-decisions.md` numbered outside
# its lane's band, or carrying no `**Lane:**` field. Cheap to make differ
# between the commit and the disk, because the whole input is one text file.
#
# This gate has a second input the others mostly do not: a *baseline* that
# grandfathers duplicate numbers. That makes it the sharpest instance of the
# property this suite exists for, because the baseline is the input that
# *forgives* -- reading it from the disk does not merely miss a finding, it
# actively waives one that is being pushed, and the waiver never has to be
# committed. Gate 9's `requests/.deletions-allowed` had exactly this hole; see
# `main()`'s note.
#
# It is also the third gate in a row to arrive here already broken. Gates 8 and
# 9 came back RED on the first run of their own cases, and so did this one:
# `main()` read the baseline from the commit and then overwrote it from disk
# one screen later, so `--head` honoured the document and not the baseline.
# Three of the checker's own comments asserted the pairing that its command line
# did not implement, and its `--selftest` did not notice because it calls
# `read_doc_and_baseline` directly and so never runs the wiring. Fixed the same
# day; `case_gate13_the_baseline_is_read_from_the_same_tree` is that regression.
# --------------------------------------------------------------------------

_DD_SECT, _DD_ENDASH = "\u00a7", "\u2013"

# The band table is the gate's own configuration -- it parses this, rather than
# hardcoding the bands -- so a fixture needs one or every section is out of band.
_DD_TABLE = "\n".join([
    "## Numbering and file order",
    "",
    "| Band | Owner | Status | Region |",
    "|---|---|---|---|",
    f"| {_DD_SECT}600{_DD_ENDASH}{_DD_SECT}699 | **lane A** | **open** | mid |",
    f"| {_DD_SECT}700{_DD_ENDASH}{_DD_SECT}799 | **lane B** | **open** | the tail |",
    "",
])


def _dd_section(number: int, lane: str | None) -> str:
    """One numbered decision. `lane=None` omits the `**Lane:**` field."""
    lane_line = f"**Lane:** {lane}\n" if lane else ""
    return (f"## {number}. a decision\n\n"
            f"**Date:** 2026-09-05\n"
            f"**Decided by:** Claude (autonomous)\n"
            f"{lane_line}\n"
            f"**In short:** something was decided.\n")


def _dd_doc(*sections: str) -> str:
    return _DD_TABLE + "\n" + "\n".join(sections)


def _dd_baseline(counts: dict[str, int]) -> str:
    return json.dumps({"file": "design-decisions.md", "counts": counts})


_DD_DOC_REL = "design-decisions.md"
_DD_BASE_REL = "scripts/design-decisions-baseline.json"

_DD_CHECKER = "check-design-decisions-bands.py"


def _bands_repo(tmp: str, name: str) -> str:
    """A repository holding a clean one-section document and an empty baseline."""
    root = new_repo(tmp, name, (_DD_CHECKER,))
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A")))
    write(root, _DD_BASE_REL, _dd_baseline({}))
    return root


def case_gate13_a_tidied_worktree_cannot_hide_a_committed_missing_lane_field(tmp: str) -> None:
    """The silent half, and the exact shape that turned `main` red on 2026-09-04.

    Lane C's section 811 landed with no `**Lane:**` field. Typing the field on
    disk without committing it is all it would have taken to make a
    worktree-reading gate approve the push that published it.
    """
    root = _bands_repo(tmp, "g13a")
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, None)))
    sha = commit(root)
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, "A")))

    disk = run_checker(root, _DD_CHECKER, "--quiet")
    rev = run_checker(root, _DD_CHECKER, "--quiet", "--head", sha)
    check("gate 13: the disk's sections all carry a lane", disk.returncode, 0)
    check("gate 13: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 13: ...naming the section only the commit has",
          "601" in rev.stdout + rev.stderr, True)


def case_gate13_an_uncommitted_violation_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: a half-written section on the disk, a clean commit."""
    root = _bands_repo(tmp, "g13b")
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, "A")))
    sha = commit(root)
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, None)))

    disk = run_checker(root, _DD_CHECKER, "--quiet")
    rev = run_checker(root, _DD_CHECKER, "--quiet", "--head", sha)
    check("gate 13: the disk refuses the uncommitted section", disk.returncode, 1)
    check("gate 13: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate13_the_baseline_is_read_from_the_same_tree(tmp: str) -> None:
    """The regression. A waiver that was never committed must not forgive.

    The baseline grandfathers duplicate numbers, so it is the input that
    *forgives* -- and an uncommitted `--update-baseline` is a single command.
    Reading it from disk means any duplicate in the commit can be waived by a
    file the reviewer never sees and the remote never receives.

    This is not hypothetical and it is not a hardening exercise: it is what
    `main()` did until 2026-09-05. It read the baseline out of the commit and
    then overwrote it with `load_baseline(args.baseline)` unconditionally a few
    lines later, so `--head` honoured the document and ignored the baseline.
    """
    root = _bands_repo(tmp, "g13c")
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, "A"),
                                     _dd_section(601, "A")))
    sha = commit(root)
    check("gate 13: a committed duplicate is a violation",
          run_checker(root, _DD_CHECKER, "--quiet", "--head", sha).returncode, 1)

    # Grandfather it on the disk only -- exactly what `--update-baseline` writes.
    # `600: 1` is not padding: the baseline is the whole grandfathered set
    # rather than a waiver list, so a number missing from it reads as new, and
    # waiving only the duplicate would swap one error for another.
    write(root, _DD_BASE_REL, _dd_baseline({"600": 1, "601": 2}))
    disk = run_checker(root, _DD_CHECKER, "--quiet")
    rev = run_checker(root, _DD_CHECKER, "--quiet", "--head", sha)
    check("gate 13: the disk's baseline grandfathers the duplicate",
          disk.returncode, 0)
    check("gate 13: ...and an UNCOMMITTED baseline does not forgive the commit",
          rev.returncode, 1)


def case_gate13_a_committed_baseline_does_grandfather_the_duplicate(tmp: str) -> None:
    """The other half, without which the case above proves nothing.

    A checker that ignored the baseline *entirely* passes
    `case_gate13_the_baseline_is_read_from_the_same_tree` -- it would refuse the
    duplicate in both runs and look correct. This is the probe-liveness half:
    the same waiver, committed, must actually clear the finding.
    """
    root = _bands_repo(tmp, "g13d")
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, "A"),
                                     _dd_section(601, "A")))
    dupe = commit(root)
    write(root, _DD_BASE_REL, _dd_baseline({"600": 1, "601": 2}))
    waived = commit(root, "baseline the duplicate")

    check("gate 13: a COMMITTED baseline does grandfather it",
          run_checker(root, _DD_CHECKER, "--quiet", "--head", waived).returncode, 0)
    check("gate 13: ...and the waiver is not backdated onto the commit before it",
          run_checker(root, _DD_CHECKER, "--quiet", "--head", dupe).returncode, 1)


def case_gate13_the_document_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The enumeration, not only the contents.

    A checker that listed the file from the disk and read its text from the
    revision passes every case above, because all of them edit a path present
    in both trees. Here the document is not on the disk at all -- the ordinary
    state of a commit that adds it and a worktree that has moved on.
    """
    root = _bands_repo(tmp, "g13e")
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, None)))
    sha = commit(root)
    remove(root, _DD_DOC_REL)

    disk = run_checker(root, _DD_CHECKER, "--quiet")
    rev = run_checker(root, _DD_CHECKER, "--quiet", "--head", sha)
    check("gate 13: the disk has no document, which is no verdict",
          disk.returncode, 2)
    check("gate 13: ...while the commit's document is judged normally",
          rev.returncode, 1)


def case_gate13_a_commit_that_deletes_the_document_is_not_a_pass(tmp: str) -> None:
    """Absence in the *commit* is an error, not an empty read.

    `GitTree.read` spells a missing path as `None`, and treating that as `""`
    would grade a commit that deletes `design-decisions.md` as having no
    numbering violations -- which is true, and exactly the wrong answer.
    """
    root = _bands_repo(tmp, "g13f")
    commit(root)
    git(root, "rm", "--quiet", _DD_DOC_REL)
    sha = commit(root, "delete the document")

    rev = run_checker(root, _DD_CHECKER, "--quiet", "--head", sha)
    check("gate 13: a commit deleting the document errors rather than passing",
          rev.returncode, 2)
    check("gate 13: ...saying so, rather than exiting quietly",
          "does not exist" in rev.stderr, True)


def case_gate13_a_baseline_absent_from_the_tree_is_not_a_pile_of_new_findings(tmp: str) -> None:
    """A moved baseline must not read as every grandfathered number turning new.

    Gate 8 shipped without this guard, so a commit that relocated its baseline
    would have been refused over 1798 diagnostics nobody had touched. The same
    commit here would turn every previously-waived duplicate into a violation.
    """
    root = _bands_repo(tmp, "g13g")
    write(root, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, "A"),
                                     _dd_section(601, "A")))
    write(root, _DD_BASE_REL, _dd_baseline({"600": 1, "601": 2}))
    commit(root)
    git(root, "rm", "--quiet", _DD_BASE_REL)
    sha = commit(root, "move the baseline")

    rev = run_checker(root, _DD_CHECKER, "--quiet", "--head", sha)
    check("gate 13: a commit with no baseline is no verdict, not a refusal",
          rev.returncode, 2)


def case_gate13_a_baseline_cannot_be_written_from_a_revision(tmp: str) -> None:
    """`--update-baseline` writes the disk, which `--head` is defined not to read.

    Answering this combination rather than refusing it would baseline the
    worktree while the caller believed it had baselined a commit -- and the
    result would then forgive whatever the worktree happened to contain.
    """
    root = _bands_repo(tmp, "g13h")
    sha = commit(root)

    rev = run_checker(root, _DD_CHECKER, "--head", sha, "--update-baseline")
    check("gate 13: --head with --update-baseline is refused",
          rev.returncode, 2)
    check("gate 13: ...naming the contradiction rather than a stack trace",
          "mutually exclusive" in rev.stderr, True)


def case_gate13_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """A rev that does not resolve is exit 2, not exit 1.

    Exit 1 is "the document breaks its bands", and a hook that could not read
    the commit at all must not print that. The two are different messages to
    the author and only one of them is actionable.
    """
    root = _bands_repo(tmp, "g13i")
    commit(root)

    rev = run_checker(root, _DD_CHECKER, "--quiet", "--head", "no-such-rev")
    check("gate 13: an unopenable revision is an error, not a violation",
          rev.returncode, 2)
    check("gate 13: ...saying the rev is not a commit",
          "not a commit" in rev.stderr, True)


# Gate 13's refusal sentence, from the hook's heredoc rather than the checker's
# own output: the checker prints its findings on a `--head` run whose exit the
# hook may still be about to allow, and the em dash in the hook's summary line
# ("REFUSING to push - design-decisions.md breaks its numbering bands") does not
# survive a cp1252 console. This clause occurs once, in the block that exits 1.
_G13_REFUSAL = "three insertion points are different line offsets"

_G13_SEED = {
    _DD_DOC_REL: _dd_doc(_dd_section(600, "A")),
    _DD_BASE_REL: _dd_baseline({}),
}


def _bands_push_fixture(tmp: str, name: str) -> str:
    return _push_fixture(tmp, name, (_DD_CHECKER,), dict(_G13_SEED))


def case_gate13_the_hook_refuses_a_commit_the_worktree_no_longer_shows(tmp: str) -> None:
    """End to end: gate 13's own wiring, not some other gate's.

    Gate 13 runs its checker once per pushed sha rather than once per push, so
    its loop has something to get wrong that a single-invocation gate does not:
    a range whose later commit is clean must not clear an earlier one that is
    not.
    """
    work = _bands_push_fixture(tmp, "g13push-hide")
    write(work, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, None)))
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a section with no lane field")
    write(work, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, "A")))

    verdict, blob = _push(work, marker=_G13_REFUSAL)
    check("gate 13 end to end: the push is refused", verdict, "refused")
    # `601`, not `Lane`: the hook's refusal heredoc says "Lane" itself, so that
    # probe is satisfied by boilerplate on any refusal. The section number comes
    # only from the checker's finding, and only the commit contains it.
    check("gate 13 end to end: ...naming the section only the commit has",
          "601" in blob, True)


def case_gate13_the_hook_allows_a_clean_commit_under_a_dirty_worktree(tmp: str) -> None:
    """End to end: the false fail, and the proof the gate was actually asked.

    The tally check is what makes this more than a formality. Gate 13 sets
    `skip_bands=1` from four separate conditions -- the bypass, a missing
    interpreter, a missing checker, and a `touches` scope that does not match --
    and a gate that skipped itself allows this fixture and every other one here.
    """
    work = _bands_push_fixture(tmp, "g13push-wip")
    write(work, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, "A")))
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a section that carries its lane")
    write(work, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, None)))

    verdict, blob = _push(work, marker=_G13_REFUSAL)
    check("gate 13 end to end: an uncommitted violation does not block",
          verdict, "allowed")
    check("gate 13 end to end: ...and the gate actually ran",
          "bands" in _tally(blob)[0], True)


def case_gate13_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` while checked out on `main`.

    A gate deriving its scope from `HEAD` rather than from `$pushed_shas` would
    report itself *skipped* here -- a visible outcome nobody reads as a bug, on
    a push carrying exactly what the gate exists to catch.
    """
    work = _bands_push_fixture(tmp, "g13push-offbranch")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, _DD_DOC_REL, _dd_doc(_dd_section(600, "A"),
                                     _dd_section(601, None)))
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bad section on a branch we will leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G13_REFUSAL)
    check("gate 13 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    # Not `_tally` here, unlike the allowed-push case above: a refusing gate
    # calls `exit 1` before the tally is printed, so on a refusal there is no
    # `ran:` line to parse and the probe would be vacuously false. The finding
    # itself is the evidence that the gate ran -- the hook cannot print a
    # section number it was never given.
    check("gate 13 end to end: ...naming the section on that other branch",
          "601" in blob, True)


# --------------------------------------------------------------------------
# Gate 14 -- check-accidental-headings.py
#
# The defect it looks for: a `---` on the line directly below prose, which
# Markdown reads as a setext underline. The paragraph's last line becomes an
# `<h2>` and the separator vanishes. One blank line is the whole difference
# between the two, which makes the commit-versus-disk fixture as cheap as it
# gets -- and makes the defect as easy to introduce.
#
# This gate is the first here with a *scope* flag as well as `--head`, and that
# is the thing worth testing hardest. `--changed-only` derives the corpus from
# `git diff-tree` against the pushed revision. Two ways to get that wrong are
# both silent:
#
#   * taking the change set from the working tree, or from HEAD, rather than
#     from the revision -- the same defect as reading the *document* from the
#     wrong tree, one level up, and invisible because the answer is usually the
#     same;
#   * omitting `--root`, which makes `diff-tree` report nothing at all for a
#     root commit. Every fixture in this suite starts with a root commit, so
#     that one would have turned this entire block green while checking
#     nothing. `case_gate14_a_root_commits_whole_tree_is_judged` is the probe
#     that would have caught it.
#
# The scoping is also a *correctness* requirement and not only a speed one: a
# lane must not be blocked from pushing by a pre-existing accidental heading in
# another lane's document, which it is forbidden to edit.
# `case_gate14_an_untouched_documents_existing_defect_does_not_block` pins that.
#
# WHAT THIS GATE DELIBERATELY DOES NOT DO. `$pushed_shas` holds one sha per
# *ref* being pushed -- the tip -- and not one per commit in the range. So a
# two-commit push that breaks a document and then fixes it is allowed, because
# what gets published is the tip's tree and the tip's tree is correct. That is
# not an oversight to be repaired later: refusing it would make an ordinary
# fix-up commit unpushable and leave rewriting history as the only way out,
# which is a far worse outcome than a momentarily-wrong blob in the middle of a
# branch that nothing renders. Every other converted gate here judges the tip of
# each ref for the same reason. What the `for sha in $pushed_shas` loop *is*
# load-bearing for is a push carrying more than one ref, and
# `case_gate14_the_hook_judges_a_second_ref_in_the_same_push` is what pins that.
# --------------------------------------------------------------------------

_AH_CHECKER = "check-accidental-headings.py"

# The defect and its fix, differing by one blank line and nothing else.
_AH_BAD = "# Doc\n\nan entry that ends here\n---\n\n## next\n"
_AH_GOOD = "# Doc\n\nan entry that ends here\n\n---\n\n## next\n"

_AH_FILLER = "# Filler\n\nnothing interesting.\n"

# Enough filler documents that an *unscoped* run reaches a verdict instead of
# refusing one. The checker's whole-corpus floor is `MIN_DOCS = 20`: a sweep
# that finds four documents has a broken enumeration, so below the floor it
# exits 2 -- "cannot reach a verdict" -- rather than reporting a clean tree.
# Three cases here run the checker unscoped (two over the worktree, one with
# `--head` and no `--changed-only`) and every one of them wants a real 0-or-1
# verdict, so the fixture has to be a corpus the checker will judge.
#
# Not derived from the checker's constant by import, deliberately: raising
# MIN_DOCS above this number breaks these fixtures loudly, with "below the
# floor" in the output, which is the correct way to be told that the floor and
# its fixtures have drifted apart. A number that followed the constant would
# make the drift invisible.
#
# The *push* fixtures below get no filler at all, and that is the other half of
# the same statement: the hook only ever invokes the checker with
# `--changed-only`, whose floor is zero, so a two-document repository is a
# corpus it must still judge. If the floor ever leaked into the scoped path,
# every end-to-end case here would fail with "below the floor".
_AH_FILLER_DOCS = 24


def _ah_repo(tmp: str, name: str) -> str:
    root = new_repo(tmp, name, (_AH_CHECKER,))
    for i in range(_AH_FILLER_DOCS):
        write(root, f"filler/f{i:02d}.md", _AH_FILLER)
    return root


def case_gate14_a_tidied_worktree_cannot_hide_a_committed_heading(tmp: str) -> None:
    """The silent half: the blank line typed on disk, never committed.

    This is the whole gate in one fixture. Fixing an accidental heading is
    literally pressing Enter, so the state where the disk is right and the
    commit is wrong is not an exotic one -- it is what the minute after
    noticing looks like.
    """
    root = _ah_repo(tmp, "g14a")
    write(root, "doc.md", _AH_BAD)
    sha = commit(root)
    write(root, "doc.md", _AH_GOOD)

    disk = run_checker(root, _AH_CHECKER, "--quiet")
    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                      "--changed-only")
    check("gate 14: the disk's document is clean", disk.returncode, 0)
    check("gate 14: ...and the commit is refused anyway", rev.returncode, 1)
    check("gate 14: ...naming the document only the commit has",
          "doc.md" in rev.stdout + rev.stderr, True)


def case_gate14_an_uncommitted_heading_does_not_block_a_clean_push(tmp: str) -> None:
    """The loud half: a half-edited document on the disk, a clean commit."""
    root = _ah_repo(tmp, "g14b")
    write(root, "doc.md", _AH_GOOD)
    sha = commit(root)
    write(root, "doc.md", _AH_BAD)

    disk = run_checker(root, _AH_CHECKER, "--quiet")
    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                      "--changed-only")
    check("gate 14: the disk refuses the uncommitted heading", disk.returncode, 1)
    check("gate 14: ...but the commit being pushed is clean", rev.returncode, 0)


def case_gate14_the_change_set_is_taken_from_the_same_tree(tmp: str) -> None:
    """`--changed-only` must diff the *revision*, not the working tree.

    The failure this pins is one level up from the usual one: the document
    could be read from the commit perfectly, and the gate still miss the
    finding because it asked the working tree *which documents to look at*. Here
    the second commit is the one that breaks `doc.md`, and by the time the gate
    runs the worktree has been restored -- so a change set derived from
    `git status` is empty and the corpus is empty and the verdict is a pass.
    """
    root = _ah_repo(tmp, "g14c")
    write(root, "doc.md", _AH_GOOD)
    commit(root)
    write(root, "doc.md", _AH_BAD)
    sha = commit(root)
    write(root, "doc.md", _AH_GOOD)

    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                      "--changed-only")
    check("gate 14: the change set comes from the revision", rev.returncode, 1)
    check("gate 14: ...and names the document that revision changed",
          "doc.md" in rev.stdout + rev.stderr, True)


def case_gate14_a_root_commits_whole_tree_is_judged(tmp: str) -> None:
    """`--root`, without which `diff-tree` reports nothing for a root commit.

    Every fixture in this suite -- every fixture in this *file* -- begins with a
    root commit. Omitting `--root` therefore does not fail one case: it turns
    the whole gate-14 block green while judging an empty corpus every time. The
    probe is worth its lines precisely because its absence is invisible.
    """
    root = _ah_repo(tmp, "g14root")
    write(root, "doc.md", _AH_BAD)
    sha = commit(root)                       # the first commit in the repository

    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                      "--changed-only")
    check("gate 14: a root commit's own tree is the change set",
          rev.returncode, 1)
    check("gate 14: ...and the finding names the document",
          "doc.md" in rev.stdout + rev.stderr, True)


def case_gate14_an_untouched_documents_existing_defect_does_not_block(tmp: str) -> None:
    """Scoping as correctness, not as speed.

    `known-issues.md` is written by three lanes and each may edit only its own
    entries. A gate that judged the whole corpus would refuse lane A's push over
    a `---` in a lane C entry -- satisfiable only by editing a file lane A is
    forbidden to touch, per roadmap.md. That is a gate with no legal way to go
    green, which is a gate that gets bypassed.

    The unscoped arm is asserted too, so this is a statement about what the flag
    *does* rather than about a corpus that happened to be clean.
    """
    root = _ah_repo(tmp, "g14scope")
    write(root, "theirs.md", _AH_BAD)
    write(root, "mine.md", _AH_GOOD)
    commit(root)
    write(root, "mine.md", _AH_GOOD + "\nan added paragraph.\n")
    sha = commit(root)

    scoped = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                         "--changed-only")
    whole = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha)
    check("gate 14: a defect in an untouched document does not block",
          scoped.returncode, 0)
    check("gate 14: ...and the defect is really there to be found",
          whole.returncode, 1)
    check("gate 14: ...in the document this commit did not touch",
          "theirs.md" in whole.stdout + whole.stderr, True)


def case_gate14_a_commit_touching_no_markdown_is_a_pass_not_a_floor(tmp: str) -> None:
    """Zero documents is a legitimate scoped corpus, and must not raise.

    The whole-corpus floor exists because a sweep that finds four documents has
    a broken enumeration. Applying the same floor to a scoped run would refuse
    every push that carries only code -- and the refusal would read as a
    checker crash (exit 2), which is the one verdict nobody debugs before
    reaching for the bypass.
    """
    root = _ah_repo(tmp, "g14nomd")
    write(root, "doc.md", _AH_GOOD)
    commit(root)
    write(root, "src/main.rs", "fn main() {}\n")
    sha = commit(root)

    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                      "--changed-only")
    check("gate 14: a commit with no Markdown passes", rev.returncode, 0)
    check("gate 14: ...without a word of complaint",
          (rev.stdout + rev.stderr).strip(), "")


def case_gate14_a_commit_that_deletes_a_document_is_not_a_crash(tmp: str) -> None:
    """A deleted document has no blob in the revision to fetch.

    Without `--diff-filter=d` the change set names it anyway and `cat-file`
    answers `missing`, which this checker raises on -- correctly, since silently
    skipping a document is a document reported clean. The result would be exit 2
    on every push that removes a Markdown file: a gate that refuses a correct
    push, in the shape hardest to diagnose.
    """
    root = _ah_repo(tmp, "g14del")
    write(root, "doomed.md", _AH_GOOD)
    write(root, "doc.md", _AH_GOOD)
    commit(root)
    remove(root, "doomed.md")
    sha = commit(root)

    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                      "--changed-only")
    check("gate 14: deleting a document is not a crash", rev.returncode, 0)


def case_gate14_a_document_absent_from_the_disk_is_still_judged(tmp: str) -> None:
    """The commit is the input; the worktree need not contain the file at all.

    A checker that fell back to reading the path from disk would report the
    document as unreadable -- and this one's worktree collector *skips*
    unreadable files, so the fallback's failure mode is a silent pass rather
    than an error.
    """
    root = _ah_repo(tmp, "g14gone")
    write(root, "doc.md", _AH_BAD)
    sha = commit(root)
    remove(root, "doc.md")

    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head", sha,
                      "--changed-only")
    check("gate 14: a document deleted from the disk is still judged",
          rev.returncode, 1)


def case_gate14_an_unopenable_revision_is_not_a_finding(tmp: str) -> None:
    """A bad revision must be exit 2, never exit 1.

    Exit 1 says "your document is wrong" and sends the author to the document.
    A typo'd sha is the checker failing to run, and `run_checker` distinguishes
    the two for the hook's message -- but only if the checker distinguishes them
    first.
    """
    root = _ah_repo(tmp, "g14badrev")
    write(root, "doc.md", _AH_GOOD)
    commit(root)

    rev = run_checker(root, _AH_CHECKER, "--quiet", "--head",
                      "0123456789012345678901234567890123456789",
                      "--changed-only")
    check("gate 14: an unopenable revision exits 2, not 1", rev.returncode, 2)
    check("gate 14: ...and says it could not reach a verdict",
          "cannot reach a verdict" in rev.stdout + rev.stderr, True)


def case_gate14_the_scope_flag_is_refused_without_a_revision(tmp: str) -> None:
    """`--changed-only` alone has no revision to diff, and must say so.

    Quietly ignoring it would leave the hook's invocation *looking* scoped while
    judging the whole corpus -- which is the pre-existing-defect problem above,
    reintroduced by a flag that reads as present.
    """
    root = _ah_repo(tmp, "g14noRev")
    write(root, "doc.md", _AH_GOOD)
    commit(root)

    rev = run_checker(root, _AH_CHECKER, "--quiet", "--changed-only")
    check("gate 14: --changed-only without --head is refused", rev.returncode, 2)
    check("gate 14: ...and names the missing flag",
          "needs --head" in rev.stdout + rev.stderr, True)


# The hook's refusal heredoc, clause-matched rather than headline-matched for
# `_G13_REFUSAL`'s reason: the em dash in the summary line does not survive a
# cp1252 console, and the clause below occurs once, in the block that exits 1.
_G14_REFUSAL = "renders that paragraph's last line as an"

_G14_SEED = {"other.md": _AH_FILLER}


def _ah_push_fixture(tmp: str, name: str) -> str:
    return _push_fixture(tmp, name, (_AH_CHECKER,), dict(_G14_SEED))


def case_gate14_the_hook_refuses_a_commit_the_worktree_no_longer_shows(tmp: str) -> None:
    """End to end: gate 14's own wiring, not some other gate's."""
    work = _ah_push_fixture(tmp, "g14push-hide")
    write(work, "doc.md", _AH_BAD)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a separator that renders as a heading")
    write(work, "doc.md", _AH_GOOD)

    verdict, blob = _push(work, marker=_G14_REFUSAL)
    check("gate 14 end to end: the push is refused", verdict, "refused")
    # `doc.md`, not the word "heading": the refusal heredoc says "heading" in
    # four places, so that probe is satisfied by boilerplate. The file name
    # comes only from the checker's finding.
    check("gate 14 end to end: ...naming the document only the commit has",
          "doc.md" in blob, True)


def case_gate14_the_hook_allows_a_clean_commit_under_a_dirty_worktree(tmp: str) -> None:
    """End to end: the false fail, plus the proof the gate was actually asked.

    The tally probe is what makes this more than a formality. Gate 14 sets
    `skip_headings=1` from five separate conditions -- the bypass, a missing
    interpreter, a missing checker, a `touches` scope that does not match, and
    an empty push -- and a gate that skipped itself allows this fixture and
    every other one here.
    """
    work = _ah_push_fixture(tmp, "g14push-wip")
    write(work, "doc.md", _AH_GOOD)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a properly separated document")
    write(work, "doc.md", _AH_BAD)

    verdict, blob = _push(work, marker=_G14_REFUSAL)
    check("gate 14 end to end: an uncommitted heading does not block",
          verdict, "allowed")
    check("gate 14 end to end: ...and the gate actually ran",
          "headings" in _tally(blob)[0], True)


def case_gate14_the_hook_judges_a_branch_it_is_not_standing_on(tmp: str) -> None:
    """End to end: `git push origin feature` while checked out on `main`."""
    work = _ah_push_fixture(tmp, "g14push-offbranch")
    git(work, "checkout", "--quiet", "-b", "feature")
    write(work, "doc.md", _AH_BAD)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a bad separator on a branch we will leave")
    git(work, "checkout", "--quiet", "main")

    verdict, blob = _push(work, "feature", marker=_G14_REFUSAL)
    check("gate 14 end to end: a branch other than HEAD is still judged",
          verdict, "refused")
    check("gate 14 end to end: ...naming the document on that other branch",
          "doc.md" in blob, True)


def case_gate14_the_hook_judges_a_second_ref_in_the_same_push(tmp: str) -> None:
    """The per-sha loop, and the only fixture shape that can tell it is there.

    `$pushed_shas` holds one sha per ref. Every other end-to-end case in this
    block pushes one branch, so all of them pass against a gate that judged
    `${pushed_shas# }` -- the first sha, as a scalar -- and ignored the rest.
    That is not a strawman shape: the unix-half gate a few hundred lines up the
    hook does exactly that, deliberately, behind a guard that stands the gate
    down when the push carries more than one ref.

    So this pushes two branches at once and puts the defect in the *second*,
    behind a clean first. A scalar read of the list judges `clean`, finds
    nothing, and publishes the accidental heading on `dirty`.

    The refusal must also still name the document -- a gate that refused because
    it could not cope with two refs would be a different bug wearing the same
    verdict.
    """
    work = _ah_push_fixture(tmp, "g14push-tworefs")
    git(work, "checkout", "--quiet", "-b", "clean")
    write(work, "ok.md", _AH_GOOD)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a properly separated document")
    git(work, "checkout", "--quiet", "main")
    git(work, "checkout", "--quiet", "-b", "dirty")
    write(work, "doc.md", _AH_BAD)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "a separator that renders as a heading")

    verdict, blob = _push(work, "clean", marker=_G14_REFUSAL,
                          extra_refs=("dirty",))
    check("gate 14 end to end: a clean first ref does not clear a second",
          verdict, "refused")
    check("gate 14 end to end: ...naming the document the second ref carries",
          "doc.md" in blob, True)


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
    case_gate4_the_scope_declaration_is_read_from_the_same_tree,
    case_gate4_a_declaration_in_the_wrong_table_does_not_exempt,
    case_gate4_build_output_is_skipped_on_the_side_that_can_see_it,
    case_gate4_a_tree_with_no_gated_sources_is_not_a_clean_tree,
    case_gate4_a_baseline_absent_from_the_tree_is_not_four_new_findings,
    case_gate4_an_unopenable_revision_is_not_a_finding,
    case_gate4_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate4_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate4_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate5_a_tidied_worktree_cannot_hide_a_committed_table,
    case_gate5_an_uncommitted_edit_does_not_block_a_clean_push,
    case_gate5_a_bin_absent_from_the_disk_is_still_judged,
    case_gate5_an_unopenable_revision_is_not_a_finding,
    case_gate5_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate5_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate5_the_hook_judges_a_branch_it_is_not_standing_on,
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
    case_gate8_a_tidied_worktree_cannot_hide_a_committed_leak,
    case_gate8_an_uncommitted_leak_does_not_block_a_clean_push,
    case_gate8_the_baseline_is_read_from_the_same_tree,
    case_gate8_a_raised_count_is_read_from_the_same_tree,
    case_gate8_a_file_absent_from_the_disk_is_still_judged,
    case_gate8_the_shrunk_half_of_the_ratchet_describes_the_commit,
    case_gate8_build_output_is_skipped_on_the_side_that_can_see_it,
    case_gate8_a_tree_with_no_corpus_is_not_a_clean_tree,
    case_gate8_a_baseline_absent_from_the_tree_is_not_a_pile_of_new_findings,
    case_gate8_a_baseline_cannot_be_written_from_a_revision,
    case_gate8_an_unopenable_revision_is_not_a_finding,
    case_gate8_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate8_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate8_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate9_a_staged_restore_cannot_hide_a_committed_deletion,
    case_gate9_an_uncommitted_deletion_does_not_block_a_clean_push,
    case_gate9_the_allowlist_is_read_from_the_same_tree,
    case_gate9_a_rename_is_not_a_deletion_in_either_tree,
    case_gate9_the_merge_base_is_taken_against_the_commit_being_judged,
    case_gate9_an_unopenable_revision_is_not_a_finding,
    case_gate9_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate9_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate9_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate11_a_tidied_worktree_cannot_hide_a_committed_dead_link,
    case_gate11_an_uncommitted_dead_link_does_not_block_a_clean_push,
    case_gate11_the_manifest_is_read_from_the_same_tree,
    case_gate11_a_bin_absent_from_the_disk_is_still_judged,
    case_gate11_the_librarys_definitions_are_read_from_the_same_tree,
    case_gate11_a_crate_absent_from_the_disk_is_still_scanned,
    case_gate11_an_unopenable_revision_is_not_a_finding,
    case_gate11_a_tree_with_no_crates_is_not_a_tree_with_no_dead_links,
    case_gate11_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate11_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate11_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate13_a_tidied_worktree_cannot_hide_a_committed_missing_lane_field,
    case_gate13_an_uncommitted_violation_does_not_block_a_clean_push,
    case_gate13_the_baseline_is_read_from_the_same_tree,
    case_gate13_a_committed_baseline_does_grandfather_the_duplicate,
    case_gate13_the_document_absent_from_the_disk_is_still_judged,
    case_gate13_a_commit_that_deletes_the_document_is_not_a_pass,
    case_gate13_a_baseline_absent_from_the_tree_is_not_a_pile_of_new_findings,
    case_gate13_a_baseline_cannot_be_written_from_a_revision,
    case_gate13_an_unopenable_revision_is_not_a_finding,
    case_gate13_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate13_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate13_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate14_a_tidied_worktree_cannot_hide_a_committed_heading,
    case_gate14_an_uncommitted_heading_does_not_block_a_clean_push,
    case_gate14_the_change_set_is_taken_from_the_same_tree,
    case_gate14_a_root_commits_whole_tree_is_judged,
    case_gate14_an_untouched_documents_existing_defect_does_not_block,
    case_gate14_a_commit_touching_no_markdown_is_a_pass_not_a_floor,
    case_gate14_a_commit_that_deletes_a_document_is_not_a_crash,
    case_gate14_a_document_absent_from_the_disk_is_still_judged,
    case_gate14_an_unopenable_revision_is_not_a_finding,
    case_gate14_the_scope_flag_is_refused_without_a_revision,
    case_gate14_the_hook_refuses_a_commit_the_worktree_no_longer_shows,
    case_gate14_the_hook_allows_a_clean_commit_under_a_dirty_worktree,
    case_gate14_the_hook_judges_a_branch_it_is_not_standing_on,
    case_gate14_the_hook_judges_a_second_ref_in_the_same_push,
)


def main() -> int:
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes. Assert a floor, as the sibling suites do.
    if len(CASES) < 119:
        print(f"FATAL: only {len(CASES)} cases registered; the suite has at "
              f"least 119. The list is broken, not the code.")
        return 1
    # ...and each converted gate must be represented, through the real hook as
    # well as directly. A floor on the count alone would be met by any number of
    # gate-2 cases, and a floor on end-to-end cases alone was already met before
    # gate 3 was converted at all -- so both are counted per gate. Raise these
    # as each remaining checker is converted; they are the thing that notices a
    # gate's cases being deleted along with the gate's own wiring.
    #
    # EVERY CONVERTED GATE IS NOW IN THIS TABLE. Gates 8 and 9 were out on
    # 2026-09-04 and gate 13 on 2026-09-05, and all three are the argument for
    # why "wired" was never the same claim as "covered": each had been
    # converted, wired with `--head "$sha"`, and asserted to be wired by
    # `test-pre-push-gates.py`'s HEAD_GATES -- and each came back RED on the
    # first run of its own cases. A checker can accept `--head`, be called with
    # it correctly, and still not honour it. Three for three.
    #
    # * Gate 8 had shipped without the missing-baseline guard gates 4 and 6 both
    #   carry, so a commit that moved `quote-names-baseline.txt` would have been
    #   refused with gate 8's full refusal over 1798 diagnostics nobody touched.
    # * Gate 9 read its *deletions* from the commit and its *permissions* --
    #   `requests/.deletions-allowed` -- from the working tree, so a waiver could
    #   be written, used and dropped without ever being published. Worse than
    #   the hole `--head` was added for, and reachable only by asking the gate a
    #   question with the two trees disagreeing.
    # * Gate 13 read the document from the commit and the *baseline* -- the
    #   input that grandfathers duplicate numbers, i.e. the one that forgives --
    #   from the disk, because `main()` overwrote the commit-read baseline with
    #   `load_baseline(args.baseline)` a few lines further down. Gate 9's hole
    #   exactly, in a gate written after gate 9's was found and fixed.
    #
    # The pattern in all three is worth naming, because it predicts the next
    # one: the half of the input that *forgives* is the half that gets read from
    # the disk. It is the input an author edits last, by running a
    # `--update-baseline` command, and it is the one whose absence from a commit
    # looks harmless. A converted gate is not covered until a case asks it about
    # a waiver that was never committed.
    #
    # So: when the next checker is converted, raise the overall floor and add
    # its row here. Do not add a row for a gate whose cases do not exist yet to
    # make the table look complete; add the cases, and the floor with them.
    #
    # A floor also rises when an existing gate gains an *input*, which is why
    # gate 4 reads 15 rather than 13. On 2026-09-05 its scope stopped being the
    # `userspace/coreutils` directory and became "every crate that does not
    # declare itself unimplemented by depending on `userspace/notimpl`", which
    # moved a decision about scope into file contents -- the one kind of input
    # whose disk-vs-revision disagreement is completely silent, because the gate
    # does not report a crate it never judged. The two cases that cover it are
    # held down here for the same reason as every other number in this table.
    for gate, floor, e2e_floor in (("gate2", 10, 3), ("gate3", 13, 4),
                                   ("gate4", 15, 3), ("gate5", 7, 3),
                                   ("gate6", 14, 3), ("gate8", 14, 3),
                                   ("gate9", 9, 3), ("gate11", 11, 3),
                                   ("gate13", 12, 3), ("gate14", 14, 4)):
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
