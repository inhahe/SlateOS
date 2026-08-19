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

import importlib.util
import inspect
import os
import shutil
import subprocess
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BOOT_TEST = os.path.join(REPO_ROOT, "scripts", "boot-test.sh")

_FAILURES = []


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
    """Import a `scripts/*.py` by path (their names are not identifiers)."""
    name = os.path.basename(path).replace("-", "_").removesuffix(".py")
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    # Registered before execution because a module defining `@dataclass` types
    # is looked up by name in `sys.modules` while its own body is still running
    # (dataclasses resolves annotations against the defining module). Skipping
    # this raises an opaque `'NoneType' object has no attribute '__dict__'`
    # from inside dataclasses.py, which says nothing about the real cause.
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


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


class ScratchRepo:
    """A throwaway git repo shaped like this one, for running the check in.

    Real git, not a mock: the whole question is what *this* git does with a
    `:(exclude)` pathspec, and a mock would answer it with whatever the author
    already believed.
    """

    def __init__(self):
        self.path = tempfile.mkdtemp(prefix="slateos-boot-test-")
        # The path *as bash names it*, which on this machine is not the path as
        # Python names it: `tempfile` hands back `C:\Users\...`, and the MSYS
        # bash on PATH cannot open a drive-letter path with backslashes -- it
        # reports `fatal: cannot change to 'C:\...': No such file or directory`
        # from `git -C`, which the real check swallows with `2>/dev/null` and
        # turns into a permanent "dirty". Asking bash for its own `pwd` is how
        # boot-test.sh itself derives PROJECT_ROOT (`cd "$SCRIPT_DIR/.." &&
        # pwd`), so this reproduces the production form rather than inventing
        # one.
        self.bash_path = subprocess.run(
            ["bash", "-c", "pwd"], cwd=self.path,
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
            # repo is then created by Windows git and inspected by the bash on
            # PATH, which is WSL, whose git has its own config. The two
            # disagree about line-ending normalisation, so every file reads as
            # modified and the whole suite reports "dirty" for reasons that
            # have nothing to do with the check under test.
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
          command line that MSYS reconstructs -- it arrives as a space. The
          fragment `BT_DIRTY=0\\ngit ...` then becomes `BT_DIRTY=0 git ...`,
          which is a *temporary environment assignment for git*, so the check
          runs, git's exit status is honoured, and `$BT_DIRTY` is still unset
          when it is read. The test failed by printing an empty string, not by
          disagreeing about dirtiness -- a mangling that looks like a bug in
          the thing under test.
        * An absolute Windows path handed to MSYS bash cannot be opened at all;
          see `layout-sweep.py`'s BOOT_TEST for the same lesson learned the
          expensive way. The script is therefore written into `cwd` and named
          relatively.

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
        proc = subprocess.run(["bash", name], cwd=where,
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
    repo = ScratchRepo()
    try:
        check("a freshly committed tree is clean", repo.dirty(fragment), 0)

        repo.append("bench/history.jsonl")
        check("...and stays clean after the bench recorder appends to it",
              repo.dirty(fragment), 0)

        repo.append("bench/boot-history.jsonl")
        check("...and after the boot recorder appends to it",
              repo.dirty(fragment), 0)

        repo.append("kernel/src/main.rs")
        check("but a source edit is still dirty, records or no records",
              repo.dirty(fragment), 1)

        repo.restore("kernel/src/main.rs")
        check("...and reverting the source makes it clean again",
              repo.dirty(fragment), 0)

        repo.append("kernel/src/main.rs")
        repo.stage("kernel/src/main.rs")
        check("a *staged* source edit is dirty too (the diff is against HEAD, "
              "not the index)", repo.dirty(fragment), 1)
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
    repo = ScratchRepo()
    try:
        repo.append("kernel/src/main.rs")
        elsewhere = tempfile.mkdtemp(prefix="slateos-elsewhere-")
        try:
            check("a source edit is seen from an unrelated cwd",
                  repo.dirty(fragment, cwd=elsewhere), 1)
            check("...and the excluded records still are not",
                  _only_records_dirty(fragment, elsewhere), 0)
        finally:
            shutil.rmtree(elsewhere, ignore_errors=True)
    finally:
        repo.close()


def _only_records_dirty(fragment, cwd):
    repo = ScratchRepo()
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
    for _name, fn in tests:
        fn(**{p: {}[p] for p in inspect.signature(fn).parameters})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print("all boot-test tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
