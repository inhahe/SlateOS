#!/usr/bin/env python3
"""Regression tests for the parts of `scripts/boot-test.sh` that decide what a
run *claims about itself*.

Run: `python scripts/test-boot-test.py` (exit 0 = pass, 1 = fail).
No pytest dependency, for the same reason `test-bench-history.py` has none.

Why this file exists
--------------------
`boot-test.sh` is fifteen minutes of QEMU wrapped in a few hundred lines of
bookkeeping, and the bookkeeping is not incidental: every row it writes to
`bench/history.jsonl` carries a commit and a `dirty` flag, and downstream
analysis *drops rows* on the strength of them. A wrong flag is therefore not a
cosmetic mislabel -- it removes evidence, quietly, from a calculation that then
reports having found nothing.

The concrete history: the dirty flag came from a bare `git diff --quiet HEAD`,
and the two files it was checking include the two the harness itself appends to
on its way out. So the first run at a commit was clean and every run after it
said `"dirty": true` -- with no source change between them. `bench/history.jsonl`
still shows the signature (40515da89: one clean row, then five dirty). Since
`layout_arms()` in `bench-history.py` drops dirty records outright, a six-arm
layout sweep would have contributed one usable arm, two short of the minimum,
and reported no band at all after three hours of QEMU.

These tests run the check as it is actually written in the script -- extracted
from the source, not copied here -- against a scratch git repository.
"""

from __future__ import annotations

import inspect
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BOOT_TEST = os.path.join(REPO_ROOT, "scripts", "boot-test.sh")

sys.path.insert(0, os.path.join(REPO_ROOT, "scripts"))
import gitenv  # noqa: E402
import srcload  # noqa: E402

# The fixtures below are throwaway repositories picked with `-C` and `cwd=`.
# Neither beats an inherited `GIT_DIR`, which git exports into hooks,
# `git bisect run` and `git rebase --exec` -- so under any of those this suite
# would build its fixtures inside the real repository and commit to it. On
# 2026-08-29 the equivalent bug in `check-requests-not-deleted.py --selftest`
# did exactly that and published two commits deleting the whole tree; see
# `scripts/gitenv.py`.
#
# Scrubbing the process environment rather than passing `env=` per call matters
# more here than anywhere: this suite runs `bash`, and the bash runs git. A
# per-call `env=` on the Python side would not reach that git at all, because
# the variable would arrive through bash's own inherited environment.
gitenv.scrub_environ()

_FAILURES = []

# ---------------------------------------------------------------------------
# Fixture directories: naming, teardown, and why teardown is not one line.
#
# Every fixture in this file lives under the repo's own gitignored `build/`
# rather than the system temp directory, because the bash these tests spawn is
# sandboxed to the project tree and cannot see `%TEMP%`. That is deliberate and
# is explained again at each call site. What was *not* deliberate is what it
# cost, which was found on 2026-09-04 and is written up in `known-issues.md` as
# `A-FIXTURE-CLEANUP-LEAVES-EMPTY-DIRECTORIES-IN-BUILD-AND-CANNOT-TELL-YOU`:
#
#   * The three `mkdtemp` calls passed no `prefix=`, so a leaked fixture was
#     named `tmpXXXXXXXX` -- unattributable, and *unsweepable*, because
#     `build/tmp` is a real unrelated directory and `rm -rf build/tmp*` would
#     take it too. A leak nobody can safely glob for is a leak nobody cleans.
#   * Teardown was `shutil.rmtree(tmp, ignore_errors=True)`, which discards the
#     one signal it produces. Twenty-one directories had accumulated, and the
#     suite printed `PASSED` on every run that made them.
#
# The failure is not intermittent. Three runs each leaked exactly seven, and
# every leftover was *empty* -- so `rmtree` deleted the contents fine and failed
# only on the final `os.rmdir`, the ordinary Windows outcome when something
# still holds a handle on the directory in the moment after its last child goes
# away (see `open-questions.md` A-Q7). A 100% rate is what dictates the shape
# below: a retry that merely spins would fail all its attempts exactly as
# reliably as the single try did, so the retry *sleeps*, with a backoff, and
# reports how many attempts it needed.
#
# The retry calls `rmtree` again rather than `os.rmdir`, so that a fixture stuck
# on a *file* is retried too -- and if one ever survives, the report says
# whether it still has contents. That distinction is precisely the one
# `ignore_errors=True` could not make: an empty leftover is cosmetic, a
# non-empty one is a real disk leak, and the old code reported them identically,
# which is to say not at all.
FIXTURE_PREFIX = "slateos-boot-test-fixture-"

#: Attempts and the per-attempt backoff step. Ten attempts with a linear
#: backoff is ~2.75 s of sleeping in the worst case, per fixture that refuses --
#: bounded, and never paid at all when the first attempt works.
_RMTREE_ATTEMPTS = 10
_RMTREE_BACKOFF = 0.05

#: How many attempts each successful teardown actually needed. Reported by
#: `main()`. This is the number that decides whether the diagnosis above is
#: right: if teardowns routinely need more than a handful, the window is not
#: milliseconds and the retry is a plaster rather than a fix.
_FIXTURE_ATTEMPTS = []

#: Fixtures that survived every attempt: `(path, entries_left, last_error)`.
_FIXTURE_LEAKS = []


def _fixture_root():
    root = os.path.join(REPO_ROOT, "build")
    os.makedirs(root, exist_ok=True)
    return root


def new_fixture():
    """Create a fixture directory under `build/`, named so a leak is traceable."""
    return tempfile.mkdtemp(prefix=FIXTURE_PREFIX, dir=_fixture_root())


def drop_fixture(path):
    """Remove a fixture, retrying with a sleep, and record what it took.

    Never raises: a teardown that threw would fail a test that had already
    passed, which is the one thing the old `ignore_errors=True` got right. The
    difference is that a refusal is now *recorded* instead of discarded, so
    `main()` can say so.
    """
    for attempt in range(1, _RMTREE_ATTEMPTS + 1):
        try:
            shutil.rmtree(path)
        except FileNotFoundError:
            return
        except OSError as exc:
            last = exc
            time.sleep(_RMTREE_BACKOFF * attempt)
            continue
        _FIXTURE_ATTEMPTS.append(attempt)
        return
    try:
        left = len(os.listdir(path))
    except OSError:
        left = -1
    _FIXTURE_LEAKS.append((path, left, last))


def sweep_stale_fixtures():
    """Collect fixtures orphaned by an earlier run, before starting a new one.

    A run killed part-way -- Ctrl-C, or `run-timeout.py` firing on a genuine
    hang -- cannot run its own teardown, so without this the debris is
    permanent. Matching on `FIXTURE_PREFIX` is what makes the sweep safe to
    write at all: it can only ever name directories this file created, never
    `build/tmp` or any other artifact.
    """
    root = os.path.join(REPO_ROOT, "build")
    if not os.path.isdir(root):
        return 0
    stale = [os.path.join(root, n) for n in sorted(os.listdir(root))
             if n.startswith(FIXTURE_PREFIX)]
    for path in stale:
        drop_fixture(path)
    return len(stale)


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def load_script(path):
    """Import a `scripts/*.py` by path (their names are not identifiers).

    Loaded through `srcload` rather than `importlib`: a `SourceFileLoader`
    consults `__pycache__`, whose staleness check is `(mtime, size)` at
    one-second resolution, so two same-size writes inside one second leave the
    second one invisible and the suite validates bytecode that is not on disk.
    That has actually happened here. See `scripts/srcload.py`.

    `srcload` derives the same name this used to derive, and registers the
    module before running its body -- which both of this function's callers
    need, because they define `@dataclass` types and dataclasses looks the
    defining module up by name while the class body is still executing.
    """
    return srcload.load(path)


def extract_dirty_check(source=None):
    """The BT_DIRTY block, lifted verbatim out of `boot-test.sh`.

    Extracted rather than restated here on purpose. A copy of the command in
    this file would test the copy: someone could rewrite the check in the
    script, leave this alone, and watch a green suite certify a line that is no
    longer there. Reading the real text means the test either exercises what
    ships or fails loudly at extraction -- and a loud failure asking for the
    extractor to be updated is a fine outcome, where a silent pass is not.

    Returns the block as a shell fragment, starting at `BT_DIRTY=0` and
    continuing through the backslash-continued command that follows it.
    """
    if source is None:
        with open(BOOT_TEST, "r", encoding="utf-8") as handle:
            source = handle.read()
    lines = source.splitlines()
    try:
        start = next(i for i, line in enumerate(lines)
                     if line.strip() == "BT_DIRTY=0")
    except StopIteration:
        raise RuntimeError(
            "boot-test.sh no longer contains a line `BT_DIRTY=0`. The dirty "
            "flag is still recorded on every bench row, so the check exists "
            "somewhere -- update extract_dirty_check() to find it rather than "
            "deleting this test.")
    block = [lines[start]]
    index = start + 1
    while index < len(lines):
        block.append(lines[index])
        if not lines[index].rstrip().endswith("\\"):
            break
        index += 1
    fragment = "\n".join(block)
    if "diff --quiet HEAD" not in fragment:
        raise RuntimeError(
            f"the block after `BT_DIRTY=0` does not ask git for a diff:\n"
            f"{fragment}")
    return fragment


#: Every bash on this machine that can run a script at all.
#:
#: The suite runs against *all* of them, not against whichever one the OS hands
#: back, because on Windows those are not the same and the difference decides
#: the answer. `CreateProcess` searches `System32` before `PATH`, so a bare
#: `subprocess.run(["bash", ...])` launches WSL's bash when WSL is installed --
#: regardless of `PATH`, and regardless of `shutil.which("bash")`, which
#: implements a `PATH` search and therefore answers a different question than
#: the one that decides. Meanwhile `boot-test.sh` in production is run by Git's
#: MSYS bash. Testing under only one of them validates a check that the other
#: one will actually perform, and those two bashes carry *different gits* with
#: different configs -- which is exactly the axis (`core.autocrlf`, pathspec
#: handling) this check is sensitive to.
BASH_CANDIDATES = (
    r"C:\Program Files\Git\usr\bin\bash.exe",
    r"C:\Program Files\Git\bin\bash.exe",
    r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
    r"C:\Windows\System32\bash.exe",
    "/bin/bash",
    "bash",
)


def available_bashes(candidates=BASH_CANDIDATES):
    """The distinct, working bashes among the candidates.

    De-duplicated by what each one reports for its *own* `pwd` of a fixed
    directory, which is the property that actually distinguishes them (`/d/...`
    versus `/mnt/d/...`) -- not by path, since `bash` and an absolute candidate
    are frequently the same binary under two names and running the suite twice
    against one bash is wasted time dressed up as coverage.
    """
    found, seen = [], set()
    probe = tempfile.mkdtemp(prefix="slateos-bash-probe-")
    try:
        for candidate in candidates:
            if os.path.isabs(candidate) and not os.path.exists(candidate):
                continue
            try:
                proc = subprocess.run([candidate, "-c", "pwd"], cwd=probe,
                                      capture_output=True, text=True,
                                      timeout=60)
            except (OSError, subprocess.SubprocessError):
                continue
            if proc.returncode != 0:
                continue
            view = proc.stdout.strip()
            if not view or view in seen:
                continue
            seen.add(view)
            found.append(candidate)
    finally:
        shutil.rmtree(probe, ignore_errors=True)
    return found


class ScratchRepo:
    """A throwaway git repo shaped like this one, for running the check in.

    Real git, not a mock: the whole question is what *this* git does with a
    `:(exclude)` pathspec, and a mock would answer it with whatever the author
    already believed.
    """

    def __init__(self, bash="bash"):
        self.bash = bash
        self.path = tempfile.mkdtemp(prefix="slateos-boot-test-")
        # The path *as this bash names it*, which is not the path as Python
        # names it: `tempfile` hands back `C:\Users\...`, and WSL's bash cannot
        # open a drive-letter path with backslashes at all -- it reports
        # `fatal: cannot change to 'C:\...': No such file or directory` from
        # `git -C`, which the real check swallows with `2>/dev/null` and turns
        # into a permanent "dirty". Git's bash resolves the same directory to
        # `/c/Users/...`. Asking each bash for its own `pwd` is how
        # boot-test.sh itself derives PROJECT_ROOT (`cd "$SCRIPT_DIR/.." &&
        # pwd`), so this reproduces the production form rather than inventing
        # one -- and it is per-bash for the same reason.
        self.bash_path = subprocess.run(
            [bash, "-c", "pwd"], cwd=self.path,
            capture_output=True, text=True, check=True).stdout.strip()
        self._git("init", "-q", ".")
        self._git("config", "user.email", "test@example.invalid")
        self._git("config", "user.name", "test")
        for relative in ("bench/history.jsonl", "bench/boot-history.jsonl",
                         "kernel/src/main.rs", "scripts/boot-test.sh"):
            full = os.path.join(self.path, relative)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            # `newline="\n"` everywhere this class writes, and it is load-
            # bearing. Python's default text mode writes CRLF on Windows; the
            # repo is then created by Windows git and inspected by whichever
            # git the bash under test carries, and WSL's has its own config.
            # The two disagree about line-ending normalisation, so every file
            # reads as modified and the whole suite reports "dirty" for reasons
            # that have nothing to do with the check under test.
            with open(full, "w", encoding="utf-8", newline="\n") as handle:
                handle.write("seed\n")
        self._git("add", "-A")
        self._git("commit", "-qm", "seed")

    def _git(self, *args):
        return subprocess.run(["git", "-C", self.path, *args],
                              capture_output=True, text=True, check=False)

    def append(self, relative, text="more\n"):
        with open(os.path.join(self.path, relative), "a",
                  encoding="utf-8", newline="\n") as handle:
            handle.write(text)

    def restore(self, relative):
        self._git("checkout", "--", relative)

    def stage(self, relative):
        self._git("add", relative)

    def dirty(self, fragment, cwd=None):
        """Run the extracted check here; return BT_DIRTY as an int.

        Written to a file and run as `bash <relative-name>` rather than passed
        to `bash -c`, for two reasons that both bite on this machine only:

        * A newline inside a single argv entry does not survive the Windows
          command line the child reconstructs -- it arrives as a space. The
          fragment `BT_DIRTY=0\\ngit ...` then becomes `BT_DIRTY=0 git ...`,
          which is a *temporary environment assignment for git*, so the check
          runs, git's exit status is honoured, and `$BT_DIRTY` is still unset
          when it is read. The test failed by printing an empty string, not by
          disagreeing about dirtiness -- a mangling that looks like a bug in
          the thing under test.
        * An absolute Windows path handed to WSL's bash cannot be opened at
          all; see `layout-sweep.py`'s BOOT_TEST and `find_bash` for the same
          lesson learned the expensive way, twice. The script is therefore
          written into `cwd` and named relatively.

        The file is untracked, which is exactly why it is safe to leave in the
        repo while the check runs: `git diff HEAD` does not see untracked
        files, so the harness cannot perturb the measurement it is taking.
        """
        where = cwd or self.path
        script = (f'PROJECT_ROOT={_quote(self.bash_path)}\n'
                  f'{fragment}\n'
                  f'echo "$BT_DIRTY"\n')
        name = "_dirty-check.sh"
        with open(os.path.join(where, name), "w", encoding="utf-8",
                  newline="\n") as handle:
            handle.write(script)
        proc = subprocess.run([self.bash, name], cwd=where,
                              capture_output=True, text=True, check=False)
        out = proc.stdout.strip().splitlines()
        if out and out[-1] in ("0", "1"):
            return int(out[-1])
        return f"stdout={proc.stdout!r} stderr={proc.stderr!r}"

    def close(self):
        shutil.rmtree(self.path, ignore_errors=True)


def _quote(path):
    return "'" + path.replace("'", "'\\''") + "'"


def test_the_harnesss_own_records_do_not_make_the_next_run_look_dirty():
    """The regression: a run recording itself must not dirty the next one.

    Both halves matter. If the exclusion were dropped, case 2 and 3 flip to
    dirty and every repeat run at a commit is thrown away downstream; if the
    exclusion were widened to the whole tree, case 4 stops firing and a kernel
    edit gets benchmarked under the previous commit's name.
    """
    fragment = extract_dirty_check()
    bashes = available_bashes()
    check("at least one bash is available to run the check under",
          bool(bashes), True)
    for bash in bashes:
        tag = f"[{os.path.basename(os.path.dirname(bash)) or bash}]"
        repo = ScratchRepo(bash)
        try:
            check(f"{tag} a freshly committed tree is clean",
                  repo.dirty(fragment), 0)

            repo.append("bench/history.jsonl")
            check(f"{tag} ...and stays clean after the bench recorder appends",
                  repo.dirty(fragment), 0)

            repo.append("bench/boot-history.jsonl")
            check(f"{tag} ...and after the boot recorder appends to it",
                  repo.dirty(fragment), 0)

            repo.append("kernel/src/main.rs")
            check(f"{tag} but a source edit is still dirty, records or no "
                  f"records", repo.dirty(fragment), 1)

            repo.restore("kernel/src/main.rs")
            check(f"{tag} ...and reverting the source makes it clean again",
                  repo.dirty(fragment), 0)

            repo.append("kernel/src/main.rs")
            repo.stage("kernel/src/main.rs")
            check(f"{tag} a *staged* source edit is dirty too (the diff is "
                  f"against HEAD, not the index)", repo.dirty(fragment), 1)
        finally:
            repo.close()


def test_the_check_reads_the_tree_under_test_not_the_callers_cwd():
    """`git -C "$PROJECT_ROOT"` and a pathspec rooted at `.` must agree.

    A pathspec is resolved relative to the *current directory*, so a check
    written as `-- .` is only asking about the whole tree because `git -C` moved
    there first. With three worktrees on this machine and a sweep that invokes
    the script from its own cwd, a check that silently narrowed to a
    subdirectory would report a kernel edit as a clean tree.
    """
    fragment = extract_dirty_check()
    for bash in available_bashes():
        tag = f"[{os.path.basename(os.path.dirname(bash)) or bash}]"
        repo = ScratchRepo(bash)
        try:
            repo.append("kernel/src/main.rs")
            elsewhere = tempfile.mkdtemp(prefix="slateos-elsewhere-")
            try:
                check(f"{tag} a source edit is seen from an unrelated cwd",
                      repo.dirty(fragment, cwd=elsewhere), 1)
                check(f"{tag} ...and the excluded records still are not",
                      _only_records_dirty(fragment, elsewhere, bash), 0)
            finally:
                shutil.rmtree(elsewhere, ignore_errors=True)
        finally:
            repo.close()


def _only_records_dirty(fragment, cwd, bash="bash"):
    repo = ScratchRepo(bash)
    try:
        repo.append("bench/history.jsonl")
        return repo.dirty(fragment, cwd=cwd)
    finally:
        repo.close()


def test_the_excluded_paths_are_the_files_the_recorders_actually_write():
    """The exclusion list and the recorders must not drift apart.

    This is the failure the exclusion could grow on its own: rename the history
    file in `bench-history.py`, and the check silently starts counting the new
    name as source. Nothing would break loudly -- every second run would just
    quietly go back to claiming it was dirty, and the rows would quietly stop
    being usable for a layout band.
    """
    fragment = extract_dirty_check()
    bench = load_script(os.path.join(REPO_ROOT, "scripts", "bench-history.py"))
    boot = load_script(os.path.join(REPO_ROOT, "scripts", "boot-history.py"))
    for module, label in ((bench, "bench-history.py"),
                          (boot, "boot-history.py")):
        relative = os.path.relpath(module.DEFAULT_HISTORY,
                                   REPO_ROOT).replace(os.sep, "/")
        check(f"{label} writes {relative}, and the dirty check excludes it",
              f"':(exclude){relative}'" in fragment, True)


def extract_clippy_crash_pattern(source=None):
    """The regex `check_kernel_clippy` uses to tell a crash from a finding.

    Lifted out of the script for the same reason `extract_dirty_check` is: a
    pattern restated here would be a pattern this file tests and the script does
    not use.
    """
    if source is None:
        with open(BOOT_TEST, "r", encoding="utf-8") as handle:
            source = handle.read()
    marker = 'if grep -qE "'
    start = source.find(marker)
    if start == -1:
        raise RuntimeError(
            "boot-test.sh no longer contains the `grep -qE` that separates a "
            "crashed clippy-driver from a real lint finding. If the check moved, "
            "update this extractor; do not delete the test -- without the check, "
            "a linter that died of memory starvation is reported to the operator "
            "as a tree full of clippy::all violations.")
    rest = source[start + len(marker):]
    return rest[:rest.index('"')]


def test_a_crashed_linter_is_not_a_tree_full_of_lint_findings():
    """A tool that died produced no verdict, and must not be read as a clean one.

    This is exit 127 all over again, one gate along. `boot-history.py` used to
    file a host `fork()` failure as a kernel TIMEOUT because both arrive as a
    non-zero status; `check_kernel_clippy` had the same hole, and the log below
    is the real one from 2026-09-02, when clippy-driver died with
    STATUS_STACK_BUFFER_OVERRUN at 3.1 GiB of free commit while another lane
    built. The gate would have told the operator the kernel had deny-level lint
    violations and then listed none, because there were none to list.

    The discriminating fact is not the status code -- both are 101 -- but whether
    cargo reported a *judgement* or reported that its child never delivered one.
    """
    pattern = re.compile(extract_clippy_crash_pattern())

    crashed = (
        "error: could not compile `kernel` (bin \"kernel\"); 9911 warnings emitted\n"
        "\n"
        "Caused by:\n"
        "  process didn't exit successfully: `clippy-driver.exe ...` "
        "(exit code: 0xc0000409, STATUS_STACK_BUFFER_OVERRUN)\n"
    )
    check("a clippy-driver that died on an NTSTATUS is recognised as a crash",
          bool(pattern.search(crashed)), True)

    # The POSIX shape of the same event. Matching cargo's wording rather than
    # the Windows status code is what makes this hold on a Linux host too.
    signalled = (
        "error: could not compile `kernel` (bin \"kernel\")\n"
        "\n"
        "Caused by:\n"
        "  process didn't exit successfully: `clippy-driver` (signal: 11, "
        "SIGSEGV: invalid memory reference)\n"
    )
    check("a clippy-driver killed by a signal is recognised as a crash",
          bool(pattern.search(signalled)), True)

    check("a compiler ICE is recognised as a crash",
          bool(pattern.search("error: internal compiler error: unexpected panic\n")),
          True)

    # The control, and the half that actually matters: a real finding must still
    # reach the lint branch. A pattern that matched this too would convert every
    # genuine clippy::all violation into "the host is busy, try later" -- which
    # is a worse bug than the one being fixed, because it hides a defect in the
    # tree rather than merely misnaming one.
    real_finding = (
        "error: this loop never actually loops\n"
        "  --> kernel/src/fs/vfs.rs:412:5\n"
        "error: could not compile `kernel` (bin \"kernel\") due to 3 previous errors\n"
    )
    check("a genuine deny-level lint failure is NOT mistaken for a crash",
          bool(pattern.search(real_finding)), False)

    # `warning:` lines are the pedantic backlog and are present on every clean
    # run; nothing in them may trip the crash branch.
    backlog = (
        "warning: `panic` should not be present in production code\n"
        "warning: `kernel` (build script) generated 5 warnings\n"
    )
    check("the pedantic warning backlog does not look like a crash",
          bool(pattern.search(backlog)), False)


def extract_shell_function(name, source=None):
    """Lift one top-level function out of `boot-test.sh`, by name.

    Relies on the file's one formatting invariant: a top-level function opens
    with `name() {` in column 0 and closes with `}` in column 0. That is checked
    rather than assumed -- a silently-truncated extraction would produce a stub
    that passes every assertion made about it.
    """
    if source is None:
        with open(BOOT_TEST, "r", encoding="utf-8") as handle:
            source = handle.read()
    lines = source.splitlines()
    opener = f"{name}() {{"
    try:
        start = lines.index(opener)
    except ValueError:
        raise RuntimeError(
            f"boot-test.sh no longer defines `{name}` as a top-level function "
            f"opening with `{opener}` in column 0. If it was renamed or nested, "
            f"update this extractor; do not delete the test that uses it.")
    for end in range(start + 1, len(lines)):
        if lines[end] == "}":
            return "\n".join(lines[start:end + 1])
    raise RuntimeError(f"`{name}` in boot-test.sh has no closing `}}` in column 0")


def _run_clippy_gate(probe_body, commit_wait, timeout=60):
    """Drive the real `check_kernel_clippy` against a `cargo` that always crashes.

    Returns the `CompletedProcess`, or `None` if it had to be killed -- which is
    itself the finding this harness exists to detect.

    Everything is addressed *relatively*, from a cwd inside the fixture.
    Absolute paths do not survive the boundary: the `bash` on this host is MSYS,
    and MSYS only translates a Windows path when the caller is itself an MSYS
    shell. Spawned from Python it is not, so `bash D:/a/b.sh` reports "No such
    file or directory" -- exit 127, which reads as a failed assertion rather
    than as a harness that never started. Relative paths need no translation and
    are equally correct on POSIX.

    The fixture lives under the repo's own gitignored `build/` rather than the
    system temp directory for a second reason of the same kind: the sandbox this
    bash runs in grants it the project tree and not `%TEMP%`.
    """
    crash_log = (
        "error: could not compile `kernel` (bin \"kernel\"); 9911 warnings emitted\n"
        "\n"
        "Caused by:\n"
        "  process didn't exit successfully: `clippy-driver.exe` "
        "(exit code: 0xc0000409, STATUS_STACK_BUFFER_OVERRUN)\n"
    )

    tmp = new_fixture()
    try:
        with open(os.path.join(tmp, "crash.txt"), "w",
                  encoding="utf-8", newline="\n") as handle:
            handle.write(crash_log)

        cargo = os.path.join(tmp, "fake-cargo.sh")
        with open(cargo, "w", encoding="utf-8", newline="\n") as handle:
            # Writes to stderr and fails, exactly as cargo does when its child
            # dies. The gate redirects both streams into its log.
            handle.write("#!/usr/bin/env bash\n"
                         "cat ./crash.txt >&2\n"
                         "exit 101\n")
        os.chmod(cargo, 0o755)

        with open(os.path.join(tmp, "harness.sh"), "w",
                  encoding="utf-8", newline="\n") as handle:
            handle.write(
                "set -u\n"
                "PROJECT_ROOT=.\n"
                "CARGO=./fake-cargo.sh\n"
                "NO_BUILD=0\n"
                "BENCH_PROFILE=debug\n"
                "CARGO_PROFILE_ARGS=()\n"
                # Above anything the stub probe reports as "low", so a crash
                # looks memory-explained and takes the branch that retries.
                "MIN_COMMIT_FREE_MB=12288\n"
                # Via the documented env knob, not by assigning `COMMIT_WAIT`
                # directly. That used to work because the budget was a global;
                # since 2026-09-06 `check_commit_headroom` computes its own
                # `local COMMIT_WAIT` from `commit_wait_budget`, which would
                # shadow an injected global and silently restore the waiting
                # this harness needs suppressed. Driving the gate through the
                # same interface an operator has is what stops the harness
                # depending on which variables happen to be locals today.
                f"BOOT_TEST_COMMIT_WAIT={commit_wait}\n"
                # Only read when BOOT_TEST_COMMIT_WAIT is unset, but defined so
                # the extracted function is runnable rather than runnable-so-far:
                # a later test that omits the knob must not fail on `set -u`.
                "BOOT_TEST_START_EPOCH=$(date +%s)\n"
                "SERIAL_FILE=\n"
                "_COMMIT_PROBE_WARNED=0\n"
                # The probe is stubbed rather than the host read, so nothing
                # here depends on how much memory the machine running the suite
                # happens to have.
                + probe_body + "\n"
                + extract_commit_wait_budget() + "\n\n"
                + extract_shell_function("check_commit_headroom") + "\n\n"
                + extract_shell_function("check_kernel_clippy") + "\n\n"
                "check_kernel_clippy\n"
                # Only reached if the gate *returned* on a crash, which would
                # mean the boot proceeds having never been linted at all.
                "echo REACHED_END_AFTER_CRASH\n"
            )

        try:
            return subprocess.run(
                ["bash", "harness.sh"], cwd=tmp, timeout=timeout,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        except subprocess.TimeoutExpired:
            return None
    finally:
        # Retried-with-a-sleep, and a refusal is recorded rather than swallowed;
        # see FIXTURE_PREFIX at the top for why one line was not enough.
        drop_fixture(tmp)


# A probe that alternates below/above the floor on successive calls. This is the
# one host behaviour that makes the retry loop unbounded: each crash reads low
# enough to justify waiting, and each wait then reads high enough to authorise
# another attempt. A probe that is merely *always* low does not exercise the
# bound at all -- `check_commit_headroom` exits 5 before a second attempt is
# ever reached -- which is why the first version of this test passed without
# testing anything.
_ALTERNATING_PROBE = """\
measure_commit_free_mb() {
    local n
    n=$(( $(cat ./probe-count 2>/dev/null || echo 0) + 1 ))
    echo "$n" > ./probe-count
    if [ $(( n % 2 )) -eq 1 ]; then echo 1024; else echo 999999; fi
}
"""

_ALWAYS_STARVED_PROBE = "measure_commit_free_mb() { echo 1024; }\n"


def test_a_clippy_gate_that_keeps_crashing_still_terminates():
    """The retry must be bounded, because a gate that never returns is worse.

    `check_kernel_clippy` waits out commit starvation and re-runs rather than
    discarding a dependency build the crash did not invalidate. That turns a
    straight-line function into a loop, and the hazard a loop introduces is not
    a wrong answer but *no* answer: an unbounded retry spins forever inside a
    boot test the operator believes is building. Exit 6 -- "the gate produced no
    judgement" -- is a bad outcome; never reporting at all is a worse one.

    Driven with the alternating probe above, which is the input that would spin
    forever if the `attempt` guard were dropped. Verified by mutation: removing
    the guard from `boot-test.sh` makes this test hang until its timeout, and
    the assertions below then report it as unbounded rather than as a wrong
    status.
    """
    if not shutil.which("bash"):
        check("a clippy gate that keeps crashing still terminates "
              "[SKIPPED: no bash]", True, True)
        return

    done = _run_clippy_gate(_ALTERNATING_PROBE, commit_wait=0)
    if done is None:
        check("a clippy gate that keeps crashing still terminates",
              "still running at the timeout -- the retry is unbounded",
              "exit 6")
        return

    # Reported as the status rather than a bool: "got False, want True" names
    # the assertion but not the evidence, and for a terminating-status test the
    # evidence *is* which status arrived. Exit 127 in particular means the
    # harness never started, which must not be read as a verdict.
    tail = done.stderr.strip().splitlines()
    check("a clippy gate that keeps crashing still terminates",
          "exit 6" if done.returncode == 6
          else f"exit {done.returncode}: {tail[-1] if tail else '(no stderr)'}",
          "exit 6")

    # The distinguishing message. Without it a reader is told to wait out a
    # memory shortage that has already been waited out and did not explain the
    # crash -- and, more to the point, its absence is how a silently-removed
    # `attempt` guard shows up on a host where the loop happens to terminate
    # anyway: the second crash would be blamed on headroom instead of reported
    # as a repeat.
    check("...and says it crashed twice rather than blaming host memory again",
          "crashed TWICE" in done.stderr, True)

    check("...and does not fall through into the boot having never linted",
          "REACHED_END_AFTER_CRASH" in done.stdout, False)

    # Exit 1 is "your tree has deny-level lint violations". A crash must never
    # arrive there: that is the accusation this whole branch exists to prevent.
    check("...and never reports a crash as a lint finding",
          done.returncode == 1, False)


def test_a_host_that_never_recovers_is_reported_as_host_load_not_as_a_crash():
    """A shortage that outlasts the budget is exit 5, not exit 6.

    The two statuses call for opposite responses -- 5 means "retry later, it
    clears on its own", 6 means "stop retrying and look at the toolchain" -- so
    the gate must not collapse the first into the second just because the
    symptom it observed was a crash. With the probe pinned below the floor and
    no waiting budget, `check_commit_headroom` gives up inside the retry and
    that verdict is the one that must survive to the caller.
    """
    if not shutil.which("bash"):
        check("a host that never recovers is reported as host load "
              "[SKIPPED: no bash]", True, True)
        return

    done = _run_clippy_gate(_ALWAYS_STARVED_PROBE, commit_wait=0)
    if done is None:
        check("a host that never recovers is reported as host load",
              "still running at the timeout", "exit 5")
        return

    tail = done.stderr.strip().splitlines()
    check("a host that never recovers is reported as host load, not a crash",
          "exit 5" if done.returncode == 5
          else f"exit {done.returncode}: {tail[-1] if tail else '(no stderr)'}",
          "exit 5")
    check("...and the retry was attempted before giving up",
          "so memory explains it" in done.stderr, True)


def _run_prune_hook(free_gb, below_gb, pruner_rc=0):
    """Drive the real `prune_build_cache_if_low` against a stubbed volume.

    Returns `(rc, stdout, argv)` where `argv` is the line the fake pruner was
    invoked with, or `None` if it was never invoked at all -- which is the
    thing most of these assertions are about.

    `measure_free_gb` is stubbed rather than the host read, so nothing here
    depends on how full the machine running the suite happens to be. Same
    relative-path and `build/`-fixture constraints as `_run_clippy_gate`; see
    its docstring for why absolute paths do not survive the MSYS boundary.
    """
    tmp = new_fixture()
    try:
        os.makedirs(os.path.join(tmp, "target"))
        # A real Python file, because the hook picks the interpreter itself and
        # runs it; a shell stub would not be exercising the same call.
        with open(os.path.join(tmp, "prune-build-cache.py"), "w",
                  encoding="utf-8", newline="\n") as handle:
            handle.write(
                "import sys\n"
                "open('argv.txt', 'w').write(' '.join(sys.argv[1:]))\n"
                f"sys.exit({pruner_rc})\n")

        with open(os.path.join(tmp, "harness.sh"), "w",
                  encoding="utf-8", newline="\n") as handle:
            handle.write(
                "set -u\n"
                "PROJECT_ROOT=.\n"
                "SCRIPT_DIR=.\n"
                f"PRUNE_CACHE_BELOW_GB={below_gb}\n"
                f"measure_free_gb() {{ echo {free_gb}; }}\n"
                + extract_shell_function("prune_build_cache_if_low") + "\n\n"
                "prune_build_cache_if_low\n"
                # Printed only if the hook *returned*. A hook that exited would
                # take the boot test's PASSED banner down with it.
                "echo RETURNED rc=$?\n"
            )

        proc = subprocess.run(
            ["bash", "harness.sh"], cwd=tmp, timeout=60,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        argv_path = os.path.join(tmp, "argv.txt")
        argv = None
        if os.path.exists(argv_path):
            with open(argv_path, "r", encoding="utf-8") as handle:
                argv = handle.read()
        return proc.returncode, proc.stdout, argv
    finally:
        drop_fixture(tmp)


def test_the_cache_prune_only_fires_when_the_volume_is_getting_full():
    """Above the watermark it must cost nothing at all.

    The prune is minutes of metadata I/O over hundreds of thousands of
    directories. Running it on every green boot would add that to the tail of
    every run for no benefit, which is how a housekeeping step earns itself a
    `--no-prune-cache` in every invocation and stops running entirely.
    """
    _rc, out, argv = _run_prune_hook(free_gb=500, below_gb=100)
    check("plenty of space: the pruner is not run", argv, None)
    check("...and the hook says nothing about it", "pruning" in out, False)

    _rc, _out, argv = _run_prune_hook(free_gb=42, below_gb=100)
    check("below the watermark: the pruner is run", argv is not None, True)


def test_the_cache_prune_can_be_switched_off_outright():
    """0 means never, including when the volume is nearly full.

    An operator who has a reason to keep a cold cache -- a bisect that will
    want those units back, an investigation into the cache itself -- needs a
    switch that holds at the moment the hook would otherwise be most eager.
    Testing it only in the roomy case would not distinguish "disabled" from
    "not triggered".
    """
    _rc, _out, argv = _run_prune_hook(free_gb=1, below_gb=0)
    check("disabled: the pruner is not run even at 1 GiB free", argv, None)


def test_the_cache_prune_names_the_tree_that_was_just_built():
    """It must prune this worktree's target/, never the script's neighbour.

    `boot-test.sh` is shared by four worktrees. The pruner's own default is
    "the target/ beside the script", so leaving it implicit would prune the
    wrong lane's cache in any checkout whose scripts/ came from elsewhere --
    silently, and while that lane was still building.
    """
    _rc, _out, argv = _run_prune_hook(free_gb=42, below_gb=100)
    check("the tree under test is named explicitly",
          "--target-dir ./target" in (argv or ""), True)
    check("...and the run actually deletes rather than reporting",
          "--yes" in (argv or ""), True)


def test_a_failed_cache_prune_cannot_turn_a_green_boot_red():
    """Housekeeping must not be able to author a verdict.

    This runs after every gate has passed, so the only thing left for it to
    affect is the exit status -- and a boot test that reported FAILED because a
    disk cleanup hit a locked file would be read as the *kernel* having failed.
    That is a worse outcome than the space not being reclaimed, which is all
    that has actually gone wrong.
    """
    rc, out, argv = _run_prune_hook(free_gb=42, below_gb=100, pruner_rc=3)
    check("the pruner really was invoked and failed", argv is not None, True)
    check("the hook returns rather than exiting", "RETURNED" in out, True)
    check("...and reports success anyway", "RETURNED rc=0" in out, True)
    check("the harness itself is green", rc, 0)


def _dump_on_failure(ok, out):
    """Print the gate's whole output when an assertion about it failed.

    These assertions are substring tests, so `check`'s own `got: False` says
    only that something was absent and never what was there instead. The
    transcript is the diagnosis, and it is a few dozen lines -- cheap to print
    on failure, and it is not printed at all on the passing path.
    """
    if ok:
        return
    print("        --- check_python_suites output ---")
    for line in out.splitlines():
        print(f"        | {line}")


#: Enough fake suites to clear `check_python_suites`' own discovery floor of 10.
#: They are padding: each prints one summary line and exits 0, which is the
#: shape of every real passing suite.
_QUIET_SUITE = "print('all 3 nothing tests passed')\n"


def _run_python_suites(suites):
    """Drive the real `check_python_suites` over a fixture `scripts/` directory.

    `suites` maps a filename under `scripts/` to its Python source. Padding is
    added until the gate's discovery floor is cleared, because a fixture that
    trips the floor would exit 1 before reaching anything this test is about --
    and would do so with a message about discovery, which reads like the thing
    under test having failed.

    Returns `(returncode, combined output)`.
    """
    tmp = new_fixture()
    try:
        scripts_dir = os.path.join(tmp, "scripts")
        os.makedirs(scripts_dir)
        for name, body in suites.items():
            with open(os.path.join(scripts_dir, name), "w",
                      encoding="utf-8", newline="\n") as handle:
                handle.write(body)
        for n in range(12):
            pad = os.path.join(scripts_dir, f"test-pad{n}.py")
            if os.path.exists(pad):
                continue
            with open(pad, "w", encoding="utf-8", newline="\n") as handle:
                handle.write(_QUIET_SUITE)

        with open(os.path.join(tmp, "harness.sh"), "w",
                  encoding="utf-8", newline="\n") as handle:
            handle.write(
                "set -u\n"
                "PROJECT_ROOT=.\n"
                + extract_shell_function("check_python_suites") + "\n\n"
                "check_python_suites\n"
                f"echo \"GATE_RETURNED rc=$?\"\n"
            )

        proc = subprocess.run(
            ["bash", "harness.sh"], cwd=tmp, timeout=180,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        return proc.returncode, proc.stdout
    finally:
        drop_fixture(tmp)


def test_a_suite_that_skips_a_group_cannot_report_only_that_it_passed():
    """The defect this gate's own display created.

    A passing suite is reported by its last line and nothing else, so a suite
    that drops a group and still ends "all N passed" reports a skip that never
    reaches the log. Real instances: `test-bench-history.py` skips seven groups
    once the runs they name age out of `bench/history.jsonl`, and
    `test-rustemit.py`'s end-to-end group needs capstone. Both are correct to
    skip; neither was visible.
    """
    rc, out = _run_python_suites({
        "test-fake-skipper.py": (
            "print('SKIP  the end-to-end group: capstone is not installed')\n"
            "print('all 9 fake tests passed')\n"
        ),
    })
    ok = check("the gate still passes -- a skip is not a failure",
               "GATE_RETURNED rc=0" in out, True)
    ok &= check("the summary line is still shown",
                "all 9 fake tests passed" in out, True)
    ok &= check("and the skip is shown with it",
                "capstone is not installed" in out, True)
    ok &= check("the section's closing line carries the count",
                "1 group(s) SKIPPED in 1: test-fake-skipper.py" in out, True)
    ok &= check("the harness is green", rc, 0)
    _dump_on_failure(ok, out)


def test_every_skip_a_suite_reports_is_marked_not_just_the_first():
    """A regression, from the first draft of the annotation.

    `printf '  ^ %s\\n' "$skips"` passes all of a suite's skips as one argument
    containing newlines, so only the first came out marked and indented; the
    rest arrived flush left, where they read as output from the harness rather
    than from the suite that skipped. `test-bench-history.py` has seven skips
    that can fire together, so this is the shape the feature would first be used
    in -- and a mis-attributed skip is barely better than a hidden one.
    """
    rc, out = _run_python_suites({
        "test-fake-multiskipper.py": (
            "print('SKIP  ipc_channel control (history no longer holds those runs)')\n"
            "print('SKIP  baselines.toml parse (tomllib needs Python 3.11+)')\n"
            "print('SKIP  docs-commit grouping (git unavailable)')\n"
            "print('all 40 fake tests passed')\n"
        ),
    })
    marked = [ln for ln in out.splitlines() if ln.strip().startswith("^ SKIP")]
    ok = check("the gate passes", "GATE_RETURNED rc=0" in out, True)
    ok &= check("all three skips are marked", len(marked), 3)
    ok &= check("...each indented under its suite",
                sorted({len(ln) - len(ln.lstrip()) for ln in marked}), [8])
    ok &= check("...and all three are counted",
                "3 group(s) SKIPPED in 1: test-fake-multiskipper.py" in out, True)
    _dump_on_failure(ok, out)


def test_a_suite_that_merely_talks_about_skipping_is_not_flagged():
    """The discriminator, which is the whole reason the match is on token one.

    This project's tooling is largely *about* skips, so a substring search for
    "skip" flags a suite's PASS lines and every line the tool under test prints
    while skipping a malformed record. Surveyed 2026-09-03: those are all of
    today's matches across the real suites and not one of them is a suite skip.
    An annotation that is usually noise gets skimmed, which leaves the real skip
    exactly as hidden as it was -- so this case is not a nicety, it is what
    keeps the other test's output worth reading.
    """
    rc, out = _run_python_suites({
        "test-fake-talker.py": (
            "print('PASS  a boot that skipped nothing yields an empty tuple')\n"
            "print('check-boot-skips: skipping malformed record at bad.jsonl:1')\n"
            "print('all 32 fake tests passed')\n"
        ),
    })
    ok = check("the gate passes", "GATE_RETURNED rc=0" in out, True)
    ok &= check("nothing was reported as skipped", "SKIPPED in" in out, False)
    ok &= check("the closing line says so positively",
                "all passed, none skipped" in out, True)
    ok &= check("the harness is green", rc, 0)
    _dump_on_failure(ok, out)


def test_a_suite_that_exits_nonzero_to_report_a_skip_is_a_failure():
    """`test-bootstrap-worktree.py` exits 2 when there is no bash to drive.

    That is the right call by the suite -- "did not run" must not be reported as
    "passed" -- and it must stay a hard failure here rather than being absorbed
    into the skip count added above. The skip annotation is for a suite that ran
    and dropped a *group*; a suite that could not run at all has no verdict to
    contribute, and the boot proceeding on one would be the silent pass this
    whole gate exists to refuse.
    """
    rc, out = _run_python_suites({
        "test-fake-unrunnable.py": (
            "import sys\n"
            "print('SKIPPED: no bash interpreter on PATH')\n"
            "sys.exit(2)\n"
        ),
    })
    ok = check("the gate refuses to build", "GATE_RETURNED rc=0" in out, False)
    ok &= check("...naming the suite", "test-fake-unrunnable.py" in out, True)
    ok &= check("...and showing why", "no bash interpreter" in out, True)
    _dump_on_failure(ok, out)


def extract_commit_wait_budget(source=None):
    """`commit_wait_budget` and its floor, lifted verbatim out of `boot-test.sh`.

    Extracted rather than restated, for the reason `extract_dirty_check` gives:
    a copy here would be a copy this file tests and the script does not run.

    Returns a shell fragment defining `COMMIT_WAIT_FLOOR` and the function, so
    a caller can source it, set `BOOT_TEST_START_EPOCH` to whatever elapsed
    time it wants to simulate, and ask what the budget would be.
    """
    if source is None:
        with open(BOOT_TEST, "r", encoding="utf-8") as handle:
            source = handle.read()
    lines = source.splitlines()
    try:
        floor = next(i for i, line in enumerate(lines)
                     if line.startswith("COMMIT_WAIT_FLOOR="))
        start = next(i for i, line in enumerate(lines)
                     if line.strip() == "commit_wait_budget() {")
        end = next(i for i, line in enumerate(lines[start:], start)
                   if line == "}")
    except StopIteration:
        raise RuntimeError(
            "boot-test.sh no longer defines COMMIT_WAIT_FLOOR and "
            "commit_wait_budget(). The pre-build commit-headroom gate still "
            "gives up after some number of seconds, so the budget is decided "
            "somewhere -- update extract_commit_wait_budget() to find it "
            "rather than deleting this test. Without it, nothing stops the "
            "budget reverting to a constant, which on 2026-09-06 threw away a "
            "7402-second gate phase to avoid a 900-second wait.")
    return "\n".join([lines[floor]] + lines[start:end + 1])


def _budget(fragment, elapsed, bash="bash", env_override=None):
    """What the extracted function returns for a run `elapsed` seconds old."""
    now = int(time.time())
    script = (
        f"{fragment}\n"
        f"BOOT_TEST_START_EPOCH={now - elapsed}\n"
        "commit_wait_budget\n"
    )
    env = dict(os.environ)
    # Popped, not left alone: this suite's own shell may well have one set --
    # the operator's workaround for the very bug being tested is to export it --
    # and inheriting it would make every case below return the same number and
    # pass for the wrong reason.
    env.pop("BOOT_TEST_COMMIT_WAIT", None)
    if env_override is not None:
        env["BOOT_TEST_COMMIT_WAIT"] = env_override
    proc = subprocess.run([bash, "-c", script], capture_output=True,
                          text=True, timeout=60, env=env)
    if proc.returncode != 0:
        return f"exit {proc.returncode}: {proc.stderr.strip()}"
    return int(proc.stdout.strip())


def test_the_commit_wait_never_costs_more_than_it_saves():
    """Giving up must not be cheaper to trigger than the work it discards.

    On 2026-09-06 a run passed all 219 gates in 7402 seconds, reached the
    pre-build commit-headroom check, found another lane building, waited the
    flat 900 seconds it was configured for, and exited 5 -- discarding two and
    a half hours of passing gates to avoid a quarter of an hour of *sleeping*,
    and leaving the work to be redone on the same contended host.

    The property that prevents a recurrence is that the budget scales with what
    the run has already spent: never abandon an investment over a wait shorter
    than the investment. So the assertions below are about the relationship
    between elapsed time and budget, not about any particular constant -- the
    floor is free to move, and a test pinned to `3600` would fail for a change
    that is not a regression while passing for one that is.
    """
    fragment = extract_commit_wait_budget()
    bashes = available_bashes()
    if not bashes:
        check("a bash exists to run the extracted budget function", False, True)
        return
    bash = bashes[0]

    # The regression itself, stated as the value that used to ship. 900 is not
    # a threshold the function knows about; it is the number that produced the
    # failure, and the test is that no amount of elapsed time yields it.
    check("a run 7402s deep does not settle for the 900s that lost that run",
          _budget(fragment, 7402, bash) > 900, True)

    # The invariant, checked across the range the gate is actually called in:
    # 60s (a fast host reaching the build early) through 10435s (the longest
    # gate phase in bench/boot-history.jsonl as of 2026-09-06).
    for elapsed in (0, 60, 900, 3599, 3600, 3601, 7402, 10435):
        got = _budget(fragment, elapsed, bash)
        check(f"...{elapsed}s spent buys at least {elapsed}s of waiting",
              isinstance(got, int) and got >= elapsed, True)

    # Monotonic, and unbounded above the floor. A budget that stopped growing
    # would reintroduce the bug at whatever value it stopped at -- which is the
    # shape a "reasonable cap" takes, and why there is not one.
    deep, deeper = _budget(fragment, 7402, bash), _budget(fragment, 14804, bash)
    check("the budget keeps growing with the run, with no ceiling to cap it",
          isinstance(deeper, int) and deeper > deep, True)

    # Below the floor it is flat, because there the sunk cost is not what sets
    # the wait -- the length of the blocker is, and the blocker is another
    # lane's cargo build.
    check("a run only 60s deep still waits out a build, not 60 seconds",
          _budget(fragment, 60, bash) == _budget(fragment, 0, bash), True)
    check("...and that floor is longer than the longest build on record (1299s)",
          isinstance(_budget(fragment, 0, bash), int)
          and _budget(fragment, 0, bash) > 1299, True)

    # The knob means what it says. An operator in a hurry who asks for 5 seconds
    # must get 5, not the floor: a knob that silently rounds its own value up is
    # a knob whose documentation is wrong.
    check("an explicit BOOT_TEST_COMMIT_WAIT is honoured verbatim, not floored",
          _budget(fragment, 10435, bash, env_override="5"), 5)
    check("...including one larger than the adaptive budget would have been",
          _budget(fragment, 60, bash, env_override="99999"), 99999)


def _sq(text):
    """Single-quote for POSIX sh."""
    return "'" + text.replace("'", "'\\''") + "'"


def _shpath(path):
    """A filesystem path in the form the shell on this host will accept.

    On Windows the `bash` in PATH is MSYS, which treats `\\` as an escape and so
    cannot open `D:\\a\\b.sh` -- it reports "No such file or directory", which
    surfaces as exit 127 and looks exactly like a test whose assertion failed.
    Forward slashes work in both worlds, and this is a no-op on POSIX.
    """
    return path.replace(os.sep, "/")


def main():
    """Run every `test_*` in this file, in definition order.

    Same discovery-with-a-floor as `test-bench-history.py`: a discovery
    mechanism that discovers nothing looks exactly like a suite that passes.
    """
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 3:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 3. Discovery is broken, not the code.")
        return 1

    # Before anything, collect fixtures an earlier run could not: a suite killed
    # part-way never reaches its own `finally`. Announced only when it finds
    # something, because a line that prints "0" on every clean run is a line
    # nobody reads on the run where it says 3.
    swept = sweep_stale_fixtures()
    if swept:
        print(f"swept {swept} fixture director{'y' if swept == 1 else 'ies'} "
              f"left by an earlier run")

    for _name, fn in tests:
        fn(**{p: {}[p] for p in inspect.signature(fn).parameters})

    print()

    # Teardown accounting. The retry is only a fix if the window it assumes is
    # real, so say what it cost rather than asserting that it worked: a run
    # where every fixture came away first try is the claim, and any run that
    # needed more says so with the worst case named.
    if _FIXTURE_ATTEMPTS:
        worst = max(_FIXTURE_ATTEMPTS)
        retried = sum(1 for a in _FIXTURE_ATTEMPTS if a > 1)
        if worst > 1:
            print(f"fixture teardown: {len(_FIXTURE_ATTEMPTS)} removed, "
                  f"{retried} needed a retry, worst {worst} of "
                  f"{_RMTREE_ATTEMPTS} attempts")
    if _FIXTURE_LEAKS:
        # Not folded into _FAILURES: this is not a boot-test.sh defect, and a
        # red suite for a held file handle would be a flaky red that gets
        # bypassed. It is loud, named, and swept by the next run -- which is the
        # whole distance between this and the `ignore_errors=True` it replaces.
        print(f"WARNING: {len(_FIXTURE_LEAKS)} fixture(s) survived "
              f"{_RMTREE_ATTEMPTS} removal attempts:")
        for path, left, exc in _FIXTURE_LEAKS:
            shape = ("empty" if left == 0 else
                     "unreadable" if left < 0 else f"{left} entr"
                     f"{'y' if left == 1 else 'ies'} left")
            print(f"  {path} ({shape}): {exc}")
        print("  Empty means only the final rmdir failed -- cosmetic, and the "
              "next run sweeps it. Anything else is a real leak; see "
              "known-issues.md A-FIXTURE-CLEANUP-LEAVES-EMPTY-DIRECTORIES.")

    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print("all boot-test tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
