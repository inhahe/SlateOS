#!/usr/bin/env python3
"""Regression tests for the sysroot staleness gate in `scripts/ctest-fixtures.py`.

Run: `python scripts/test-ctest-fixtures.py` (exit 0 = pass, 1 = fail).
No pytest dependency, for the same reason `test-bench-history.py` has none:
this has to run from a bare checkout, and the dependency would cost more than
it saves.

Why this file exists
--------------------
`ctest-fixtures.py` is the gate that stands between "the tree built" and "the
boot test may report PASS". It has three levels -- fixture mtimes, fixture
content stamps, and the sysroot those two are *measured against* -- and the
third had no tests at all while it was the one that kept failing.

The specific bug these tests pin down is
`A-SYSROOT-STALENESS-GATE-IS-WEDGED-BY-GIT-TOUCHING-A-FILE-IT-WATCHES`
(known-issues.md). The gate compared mtimes, on the reasoning that "was this
built after that was edited" is an *ordering* question a content hash cannot
answer. The reasoning is plausible and wrong in its premise: mtime does not
record when a file was edited, it records when it was **written**, and git
writes files it has not edited on every `checkout`, `merge` and `stash` -- which
this project mandates at the start of every task. Worse, the gate was
unsatisfiable by its own printed remedy, because PowerShell's `Copy-Item`
preserves source timestamps, so re-running the sysroot build could not move
`libc.a`'s mtime forward unless cargo happened to relink.

So the property under test is not "the hash is correct" but:

  * a file that is **written but unchanged** must NOT be reported (the bug), and
  * a file that is **changed** must always be reported (what the gate is for),

and that the mtime path -- still the fallback where python-less hosts live --
keeps its old behaviour, clearly labelled as the weaker answer so it can never
be mistaken for the stronger one.
"""

from __future__ import annotations

import contextlib
import inspect
import io
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import srcload  # noqa: E402

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "ctest-fixtures.py")

_FAILURES: list[str] = []


def load_module():
    """Import ctest-fixtures.py by path (its name is not a valid identifier).

    Loaded through `srcload` rather than `importlib`: a `SourceFileLoader`
    consults `__pycache__`, whose staleness check is `(mtime, size)` at
    one-second resolution, so two same-size writes inside one second leave the
    second one invisible and the suite validates bytecode that is not on disk.
    That has actually happened here. See `scripts/srcload.py`.
    """
    return srcload.load(SCRIPT, "ctest_fixtures")


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def _fake_tree(cf, tmpdir: str) -> Path:
    """Build a miniature repo and point the module's globals at it.

    The module derives `REPO`, `LIBC` and `SYSROOT_STAMP` at import time, and
    its functions read them at call time -- so rebinding the module attributes
    is enough, and no test has to touch the real tree. That matters: several of
    these tests deliberately corrupt inputs, and doing that in the checkout
    would wedge the very gate they are testing.
    """
    repo = Path(tmpdir)
    (repo / "posix" / "src").mkdir(parents=True)
    (repo / "toolchain" / "stubs" / "src").mkdir(parents=True)
    (repo / "toolchain" / "sysroot" / "lib").mkdir(parents=True)

    # write_bytes, not write_text: Python's text mode translates "\n" to CRLF on
    # Windows, which would give the fixture the very line endings
    # `test_text_inputs_are_crlf_folded` is trying to introduce -- and that test
    # would then be converting CRLF to "\r\r\n" and asserting nothing useful.
    (repo / "posix" / "Cargo.toml").write_bytes(b"[package]\nname = \"posix\"\n")
    (repo / "posix" / "src" / "lib.rs").write_bytes(b"pub fn a() {}\n")
    (repo / "posix" / "src" / "io.rs").write_bytes(b"pub fn b() {}\n")
    (repo / "toolchain" / "stubs" / "src" / "lib.rs").write_bytes(b"pub fn c() {}\n")
    (repo / "toolchain" / "build-sysroot.ps1").write_bytes(b"# flags\n")
    (repo / "toolchain" / "sysroot" / "lib" / "libc.a").write_bytes(b"!<arch>\n")

    cf.REPO = repo
    cf.LIBC = repo / "toolchain" / "sysroot" / "lib" / "libc.a"
    cf.SYSROOT_STAMP = repo / "toolchain" / "sysroot" / ".sysroot.stamp"
    return repo


def _age(path: Path, seconds: int) -> None:
    """Push a file's mtime `seconds` into the past."""
    now = time.time()
    os.utime(path, (now - seconds, now - seconds))


# --------------------------------------------------------------------------
# The bug this was written for
# --------------------------------------------------------------------------

def test_a_rewritten_but_unchanged_file_is_not_drift(cf, tmpdir):
    """git writing a file it did not edit must not wedge the gate.

    This is the whole defect. `git checkout --` restoring a file to
    byte-identical content, or a merge that touches a path it does not change,
    both bump mtime. Under the old gate that was indistinguishable from an edit.
    """
    repo = _fake_tree(cf, tmpdir)
    libc = cf.LIBC
    _age(libc, 600)
    for path in repo.rglob("*"):
        if path.is_file() and path != libc:
            _age(path, 1200)
    cf.SYSROOT_STAMP.write_text(cf.compute_sysroot(), encoding="utf-8", newline="\n")

    # What git does: rewrite the same bytes, leaving a fresh mtime.
    #
    # `write_bytes(read_bytes())` and not a `write_text(read_text())` round
    # trip, which is what this was until 2026-09-04. In text mode the read
    # folds CRLF to `\n` and the write turns every `\n` back into CRLF, so on
    # Windows this line did not rewrite the same bytes at all -- it converted
    # the fixture (written LF at :101) to CRLF. The test passed anyway, because
    # `compute_sysroot` hashes text inputs with CRLF folded to LF and so could
    # not see the difference. Passing for that reason means it was asserting
    # "a CRLF-ified file is not drift" on Windows and "an identical file is not
    # drift" on Linux -- two different claims from one line of source, and only
    # the second is the one the docstring above makes.
    ps1 = repo / "toolchain" / "build-sysroot.ps1"
    ps1.write_bytes(ps1.read_bytes())
    os.utime(ps1, None)

    mode, findings = cf.sysroot_staleness()
    check("a rewritten-but-unchanged file is not drift", (mode, findings), ("stamp", []))


def test_the_mtime_fallback_still_has_the_bug_it_is_the_fallback_for(cf, tmpdir):
    """Deliberate: the fallback is the *old* test, and it still false-positives.

    Kept and asserted rather than quietly improved, because the fallback exists
    for hosts that cannot run the hash at all. A reader who sees `mtime` in the
    output needs to know it can fire on a file nobody edited -- which is exactly
    why `_report_sysroot_staleness` says so in that mode.
    """
    repo = _fake_tree(cf, tmpdir)
    _age(cf.LIBC, 600)
    for path in repo.rglob("*"):
        if path.is_file() and path != cf.LIBC:
            _age(path, 1200)
    ps1 = repo / "toolchain" / "build-sysroot.ps1"
    os.utime(ps1, None)  # touched, not edited

    mode, findings = cf.sysroot_staleness()
    check("no stamp -> mtime mode", mode, "mtime")
    check("mtime mode reports the merely-touched file",
          findings, ["toolchain/build-sysroot.ps1"])


def test_a_real_edit_is_reported_by_the_stamp(cf, tmpdir):
    """The gate must still do its job: changed content is always drift."""
    repo = _fake_tree(cf, tmpdir)
    cf.SYSROOT_STAMP.write_text(cf.compute_sysroot(), encoding="utf-8", newline="\n")
    (repo / "posix" / "src" / "io.rs").write_text("pub fn b() { changed(); }\n", encoding="utf-8", newline="")

    mode, findings = cf.sysroot_staleness()
    check("a real edit is stamp drift", mode, "stamp")
    check("the drift names the file that changed",
          [f for f in findings if "posix/src/io.rs" in f] != [], True)
    check("and names only that file", len(findings), 1)


def test_an_edit_that_predates_libc_is_still_caught(cf, tmpdir):
    """Content beats ordering: an *older* file with changed content is drift.

    Unreachable by the mtime test by construction, and not hypothetical -- a
    merge can bring in a file whose mtime is older than the local `libc.a`.
    """
    repo = _fake_tree(cf, tmpdir)
    cf.SYSROOT_STAMP.write_text(cf.compute_sysroot(), encoding="utf-8", newline="\n")
    victim = repo / "posix" / "src" / "lib.rs"
    victim.write_bytes(b"pub fn a() { different(); }\n")
    # Every input older than libc.a, so the ordering test has nothing to say
    # about *any* of them -- the point is that content drift survives a tree
    # where mtime is uniformly innocent, not just where one file is.
    for path in repo.rglob("*"):
        if path.is_file() and path != cf.LIBC:
            _age(path, 99999)
    _age(cf.LIBC, 10)

    mode, findings = cf.sysroot_staleness()
    check("an older-but-changed file is stamp drift", mode, "stamp")
    check("stamp mode catches what mtime ordering cannot",
          [f for f in findings if "posix/src/lib.rs" in f] != [], True)

    # Control: the same tree under the fallback reports nothing at all.
    cf.SYSROOT_STAMP.unlink()
    mode2, findings2 = cf.sysroot_staleness()
    check("control: mtime mode misses it entirely", (mode2, findings2), ("mtime", []))


# --------------------------------------------------------------------------
# Stamp mechanics
# --------------------------------------------------------------------------

def test_stamp_roundtrip_is_clean(cf, tmpdir):
    _fake_tree(cf, tmpdir)
    cf.SYSROOT_STAMP.write_text(cf.compute_sysroot(), encoding="utf-8", newline="\n")
    check("a freshly written stamp reports no drift", cf.sysroot_staleness(), ("stamp", []))


def test_stamp_is_order_independent(cf, tmpdir):
    """Same tree, same bytes -- the stamp cannot depend on directory order."""
    _fake_tree(cf, tmpdir)
    check("compute_sysroot is deterministic", cf.compute_sysroot(), cf.compute_sysroot())


def test_a_new_input_is_reported(cf, tmpdir):
    """A source file that appeared after the build is a reason to rebuild."""
    repo = _fake_tree(cf, tmpdir)
    cf.SYSROOT_STAMP.write_text(cf.compute_sysroot(), encoding="utf-8", newline="\n")
    (repo / "posix" / "src" / "new.rs").write_text("pub fn d() {}\n", encoding="utf-8", newline="")

    mode, findings = cf.sysroot_staleness()
    check("a new input is drift", mode, "stamp")
    check("and is described as new",
          [f for f in findings if "new.rs" in f and "new input" in f] != [], True)


def test_a_deleted_input_is_reported(cf, tmpdir):
    repo = _fake_tree(cf, tmpdir)
    cf.SYSROOT_STAMP.write_text(cf.compute_sysroot(), encoding="utf-8", newline="\n")
    (repo / "posix" / "src" / "io.rs").unlink()

    mode, findings = cf.sysroot_staleness()
    check("a deleted input is drift", mode, "stamp")
    check("and is described as gone",
          [f for f in findings if "io.rs" in f and "no longer present" in f] != [], True)


def test_target_dir_is_excluded(cf, tmpdir):
    """Build output must not be an input, or the stamp is self-referential.

    If `target/` counted, building the sysroot would change its own inputs and
    no two consecutive runs could agree -- a gate that is red immediately after
    the build that was supposed to satisfy it.
    """
    repo = _fake_tree(cf, tmpdir)
    out = repo / "toolchain" / "stubs" / "target" / "release"
    out.mkdir(parents=True)
    (out / "libstubs.a").write_bytes(b"\x00" * 64)
    (out / "build.rs").write_text("// generated\n", encoding="utf-8", newline="")

    labels = [label for label, _p, _t in cf._sysroot_inputs()]
    check("target/ contributes no inputs",
          [lb for lb in labels if "/target/" in lb], [])


def test_dot_dirs_are_excluded(cf, tmpdir):
    repo = _fake_tree(cf, tmpdir)
    hidden = repo / "posix" / "src" / ".cache"
    hidden.mkdir()
    (hidden / "junk.rs").write_text("noise\n", encoding="utf-8", newline="")

    labels = [label for label, _p, _t in cf._sysroot_inputs()]
    check("dot-directories contribute no inputs",
          [lb for lb in labels if ".cache" in lb], [])


def test_text_inputs_are_crlf_folded(cf, tmpdir):
    """A `.rs` file differing only in line endings must not read as an edit.

    This is the `version 2` rule the fixture stamps already needed: with
    `core.autocrlf=input`, two worktrees of the same commit legitimately hold
    byte-different text files that git considers identical, so a raw hash makes
    the stamp a property of the working tree rather than of the commit.
    """
    repo = _fake_tree(cf, tmpdir)
    victim = repo / "posix" / "src" / "lib.rs"
    before = cf.compute_sysroot()
    victim.write_bytes(victim.read_bytes().replace(b"\n", b"\r\n"))
    check("CRLF vs LF in a .rs input does not change the stamp",
          cf.compute_sysroot(), before)


def test_unknown_suffixes_are_hashed_raw(cf, tmpdir):
    """Default-raw: folding a genuinely binary input could hide a real change."""
    repo = _fake_tree(cf, tmpdir)
    blob = repo / "posix" / "src" / "table.bin"
    blob.write_bytes(b"\x01\r\n\x02")
    before = cf.compute_sysroot()
    blob.write_bytes(b"\x01\n\x02")
    changed = cf.compute_sysroot() != before
    check("a CRLF-shaped byte change in a binary input IS drift", changed, True)


# --------------------------------------------------------------------------
# Edges
# --------------------------------------------------------------------------

def test_no_libc_is_not_a_verdict(cf, tmpdir):
    """A sysroot nobody has built yet is not a stale sysroot."""
    _fake_tree(cf, tmpdir)
    cf.LIBC.unlink()
    check("missing libc.a yields no findings", cf.sysroot_staleness(), ("", []))


def test_an_empty_stamp_falls_back_rather_than_passing(cf, tmpdir):
    """A truncated stamp must not read as 'nothing changed'.

    A zero-byte file would otherwise produce an empty recorded index, and an
    empty-vs-empty comparison is silence -- a check that cannot fire is
    indistinguishable from a check that passed.
    """
    repo = _fake_tree(cf, tmpdir)
    cf.SYSROOT_STAMP.write_text("", encoding="utf-8", newline="")
    _age(cf.LIBC, 600)
    for path in repo.rglob("*"):
        if path.is_file() and path != cf.LIBC:
            _age(path, 1200)
    os.utime(repo / "posix" / "src" / "lib.rs", None)

    mode, findings = cf.sysroot_staleness()
    check("an empty stamp falls back to mtime", mode, "mtime")
    check("and the fallback still reports", findings, ["posix/src/lib.rs"])


def test_stamp_and_mtime_modes_are_distinguishable(cf, tmpdir):
    """The reporter must never let the weaker answer read as the stronger one."""
    import io
    from contextlib import redirect_stdout

    _fake_tree(cf, tmpdir)
    buf = io.StringIO()
    with redirect_stdout(buf):
        cf._report_sysroot_staleness("mtime", ["posix/src/lib.rs"])
    mtime_text = buf.getvalue()

    buf = io.StringIO()
    with redirect_stdout(buf):
        cf._report_sysroot_staleness("stamp", ["input posix/src/lib.rs: recorded a... but on disk b..."])
    stamp_text = buf.getvalue()

    check("mtime mode says it is the fallback", "fallback" in mtime_text, True)
    check("stamp mode does not claim to be a fallback", "fallback" in stamp_text, False)
    check("mtime mode speaks of ordering", "OLDER than" in mtime_text, True)
    check("stamp mode speaks of content", "have since changed" in stamp_text, True)


def test_a_cpp_fixtures_source_is_an_input(cf, tmpdir):
    """A C++ fixture's source is `main.cpp`: an edit to it makes the ELF
    stale, as an edit to a C fixture's `main.c` does. Before `main.cpp` was
    an input, `ctest-cxx-throw` would have read as current after any change
    to the program it runs."""
    _fake_tree(cf, tmpdir)
    fx = Path(tmpdir) / "services" / "ctest-cxx"
    fx.mkdir(parents=True)
    (fx / "build.py").write_bytes(b"# recipe\n")
    (fx / "main.cpp").write_bytes(b"int main() { return 42; }\n")
    elf = cf.elf_of(fx)
    elf.write_bytes(b"ELF")
    for p in (fx / "build.py", fx / "main.cpp", cf.LIBC):
        _age(p, 100)
    _age(elf, 50)
    labels = [label for label, _p, _t in cf._inputs(fx)]
    check("main.cpp is an input", "main.cpp" in labels, True)
    check("an ELF newer than its inputs is current", cf.is_stale(fx), None)
    now = time.time()
    os.utime(fx / "main.cpp", (now, now))
    check("an ELF older than main.cpp is stale", cf.is_stale(fx), "older than main.cpp")


def test_a_c_fixtures_overlay_headers_are_an_input(cf, tmpdir):
    """Every C fixture is compiled with `-I posix/include`: a header edited
    there makes the ELF stale, as an edit to its own `main.c` does. Before
    the overlay was an input, `ctest-obstack` -- whose subject is
    `<obstack.h>`'s macros -- would have read as current after any change
    to them. A fastpy fixture is not compiled against the overlay, and is
    not made stale by it."""
    repo = _fake_tree(cf, tmpdir)
    header = repo / "posix" / "include" / "bits" / "x.h"
    header.parent.mkdir(parents=True)
    header.write_bytes(b"#define X 1\n")
    c_fx = repo / "services" / "ctest-x"
    py_fx = repo / "services" / "fastpy-x"
    for fx in (c_fx, py_fx):
        fx.mkdir(parents=True)
        (fx / "build.py").write_bytes(b"# recipe\n")
        _age(fx / "build.py", 100)
        cf.elf_of(fx).write_bytes(b"ELF")
        _age(cf.elf_of(fx), 50)
    (c_fx / "main.c").write_bytes(b"int main(void) { return 42; }\n")
    for p in (c_fx / "main.c", cf.LIBC, header):
        _age(p, 100)
    # The fastpy compiler's newest source is the real checkout's, whose age
    # this tree cannot set: none, for the length of the test.
    saved = cf._NEWEST_COMPILER_SOURCE
    cf._NEWEST_COMPILER_SOURCE = None
    try:
        cf._NEWEST_OVERLAY_HEADER.clear()
        labels = [label for label, _p, _t in cf._inputs(c_fx)]
        check("the newest overlay header is a C fixture's input",
              "posix/include/bits/x.h" in labels, True)
        check("a C fixture newer than the overlay is current", cf.is_stale(c_fx), None)
        now = time.time()
        os.utime(header, (now, now))
        cf._NEWEST_OVERLAY_HEADER.clear()
        check("a C fixture older than a header is stale", cf.is_stale(c_fx),
              "older than posix/include/bits/x.h")
        check("a fastpy fixture does not take the overlay", cf.is_stale(py_fx), None)
    finally:
        cf._NEWEST_COMPILER_SOURCE = saved
        cf._NEWEST_OVERLAY_HEADER.clear()


# --------------------------------------------------------------------------
# Which fastpy, and which fixtures
# --------------------------------------------------------------------------

def _usable_fastpy(path: Path) -> Path:
    """A directory `_fastpy_dir` accepts: one with `compiler/__init__.py`."""
    (path / "compiler").mkdir(parents=True)
    (path / "compiler" / "__init__.py").write_bytes(b"")
    return path


@contextlib.contextmanager
def _fastpy_search(cf, tmpdir: str, **env):
    """A repo at `<tmpdir>/os-lane-x` with the environment variables `env`
    set (None: unset), both put back afterwards. Yields the repo's parent."""
    saved_repo, saved_env = cf.REPO, {k: os.environ.get(k) for k in env}
    cf.REPO = Path(tmpdir) / "os-lane-x"
    cf.REPO.mkdir(exist_ok=True)
    for k, v in env.items():
        if v is None:
            os.environ.pop(k, None)
        else:
            os.environ[k] = v
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            yield Path(tmpdir)
    finally:
        cf.REPO = saved_repo
        for k, v in saved_env.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v


def test_fastpy_dir_takes_the_submodule_and_nothing_else(cf, tmpdir):
    """With no `$FASTPY_DIR`, the submodule `toolchain/fastpy` -- and nothing
    when it is not checked out, though a usable fastpy sits beside the repo
    and on `$PYTHONPATH`: building with a fastpy the tree does not pin is
    what the submodule exists to rule out."""
    other = _usable_fastpy(Path(tmpdir) / "elsewhere" / "fastpy")
    with _fastpy_search(cf, tmpdir, FASTPY_DIR=None, PYTHONPATH=str(other)) as parent:
        _usable_fastpy(parent / "fastpy")
        none = cf._fastpy_dir()
        pinned = _usable_fastpy(cf.REPO / "toolchain" / "fastpy")
        found = cf._fastpy_dir()
    check("no submodule checked out: nothing, sibling and PYTHONPATH ignored",
          none, None)
    check("the submodule", found, pinned)


def test_a_wrong_fastpy_dir_is_refused_not_fallen_through(cf, tmpdir):
    """A `$FASTPY_DIR` without `compiler/` is an error, even with the
    submodule checked out: building from a checkout the caller did not name
    is the failure this search must never have."""
    with _fastpy_search(cf, tmpdir, FASTPY_DIR=str(Path(tmpdir) / "missing"),
                        PYTHONPATH=None):
        _usable_fastpy(cf.REPO / "toolchain" / "fastpy")
        found = cf._fastpy_dir()
    check("a wrong FASTPY_DIR finds nothing", found, None)


def test_fastpy_dir_command_prints_only_the_path(cf, tmpdir):
    """`fastpy-dir`, which create-ext4-rootfs.sh reads, prints the checkout and
    nothing else on stdout, and exits 1 with nothing on it when there is none
    -- the search's own complaint about a wrong FASTPY_DIR included."""
    with _fastpy_search(cf, tmpdir, FASTPY_DIR=str(Path(tmpdir) / "missing"),
                        PYTHONPATH=None):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            missing_rc = cf.cmd_fastpy_dir()
        missing_out = out.getvalue()
    with _fastpy_search(cf, tmpdir, FASTPY_DIR=None, PYTHONPATH=None):
        pinned = _usable_fastpy(cf.REPO / "toolchain" / "fastpy")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            found_rc = cf.cmd_fastpy_dir()
        found_out = out.getvalue()
    check("none: exit 1", missing_rc, 1)
    check("none: nothing on stdout", missing_out, "")
    check("found: exit 0", found_rc, 0)
    check("found: the path alone", found_out, f"{pinned}\n")


def test_submodule_status_lines_are_read_as_git_documents_them(cf):
    """The first character of a `git submodule status` line."""
    sha = "5b11d8825e868140cf9c6350f92192c82fb0f3d7 toolchain/fastpy"
    check("space: current", cf._submodule_state(" " + sha + " (v0.3.6)"), "current")
    check("minus: absent", cf._submodule_state("-" + sha), "absent")
    check("plus: moved", cf._submodule_state("+" + sha + " (v0.3.5)"), "moved")
    check("U: conflict", cf._submodule_state("U" + sha), "conflict")


def _git(*args, cwd):
    import gitenv
    r = subprocess.run(["git", *args], cwd=cwd, env=gitenv.clean_env(),
                       capture_output=True, text=True, encoding="utf-8",
                       errors="replace", check=False)
    if r.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout.strip()


def test_build_checks_the_pinned_fastpy_out_and_keeps_local_work(cf, tmpdir):
    """`_sync_fastpy_submodule` against real git: a submodule never checked
    out, and one left at another commit (as a merge that moves the pin leaves
    it), are both checked out at the pin; one with changes to its files is
    refused and left as it is."""
    root = Path(tmpdir)
    home = root / "home"
    home.mkdir()
    # A local-path submodule needs protocol.file.allow, and commits need a
    # name. gitenv.clean_env() drops GIT_CONFIG_*, so the config goes where
    # git reads it regardless: a HOME of the test's own.
    (home / ".gitconfig").write_bytes(
        b"[user]\n\tname = t\n\temail = t@t\n"
        b"[protocol \"file\"]\n\tallow = always\n"
        b"[init]\n\tdefaultBranch = main\n")
    saved_home = {k: os.environ.get(k) for k in ("HOME", "USERPROFILE")}
    os.environ["HOME"] = str(home)
    os.environ["USERPROFILE"] = str(home)
    saved_repo = cf.REPO
    try:
        fp = root / "fastpy-origin"
        (fp / "compiler").mkdir(parents=True)
        _git("init", "-q", cwd=fp)
        (fp / "compiler" / "__init__.py").write_bytes(b"VERSION = 1\n")
        _git("add", ".", cwd=fp)
        _git("commit", "-q", "-m", "one", cwd=fp)
        first = _git("rev-parse", "HEAD", cwd=fp)
        (fp / "compiler" / "__init__.py").write_bytes(b"VERSION = 2\n")
        _git("commit", "-q", "-am", "two", cwd=fp)

        os_repo = root / "os"
        os_repo.mkdir()
        _git("init", "-q", cwd=os_repo)
        _git("submodule", "add", "-q", fp.as_posix(), "toolchain/fastpy", cwd=os_repo)
        sub = os_repo / "toolchain" / "fastpy"
        _git("checkout", "-q", first, cwd=sub)
        _git("add", "toolchain/fastpy", cwd=os_repo)
        _git("commit", "-q", "-m", "pin fastpy at one", cwd=os_repo)
        cf.REPO = os_repo

        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            # Left at another commit, as a merge that moved the pin leaves it.
            _git("checkout", "-q", "main", cwd=sub)
            moved = _git("submodule", "status", "--", "toolchain/fastpy", cwd=os_repo)
            synced_moved = cf._sync_fastpy_submodule()
            after_moved = _git("rev-parse", "HEAD", cwd=sub)
            # Never checked out, as in a fresh worktree.
            _git("submodule", "deinit", "-q", "--", "toolchain/fastpy", cwd=os_repo)
            absent = _git("submodule", "status", "--", "toolchain/fastpy", cwd=os_repo)
            synced_absent = cf._sync_fastpy_submodule()
            after_absent = _git("rev-parse", "HEAD", cwd=sub)
            current_again = cf._sync_fastpy_submodule()
            # Changes of its own: refused, and kept.
            (sub / "compiler" / "__init__.py").write_bytes(b"VERSION = 'mine'\n")
            refused = cf._sync_fastpy_submodule()
            kept = (sub / "compiler" / "__init__.py").read_bytes()
    finally:
        cf.REPO = saved_repo
        for k, v in saved_home.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
    check("left at another commit reads as moved", cf._submodule_state(moved), "moved")
    check("moved: synced", synced_moved, True)
    check("moved: now at the pin", after_moved, first)
    check("deinit reads as absent", cf._submodule_state(absent), "absent")
    check("absent: synced", synced_absent, True)
    check("absent: now at the pin", after_absent, first)
    check("at the pin: nothing to do", current_again, True)
    check("changes of its own: refused", refused, False)
    check("and left as they were", kept, b"VERSION = 'mine'\n")
    check("the refusal says why", "changes of its own" in out.getvalue(), True)


def test_only_prefers_the_fixture_named_exactly(cf, tmpdir):
    """`--only` takes the fixture called exactly that where there is one --
    some names are prefixes of others -- and otherwise every name containing it."""
    services = Path(tmpdir) / "services"
    for name in ("fastpy-ls", "fastpy-lseek", "ctest-fortify", "ctest-fortify-abort"):
        (services / name).mkdir(parents=True)
        (services / name / "build.py").write_bytes(b"# recipe\n")
    saved = cf.SERVICES
    cf.SERVICES = services
    try:
        names = {o: [f.name for f in cf.select_fixtures(o)]
                 for o in ("fastpy-ls", "ctest-fortify", "fortify", "lseek", None)}
    finally:
        cf.SERVICES = saved
    check("exact: fastpy-ls alone", names["fastpy-ls"], ["fastpy-ls"])
    check("exact: ctest-fortify alone", names["ctest-fortify"], ["ctest-fortify"])
    check("part: both fortify fixtures", names["fortify"],
          ["ctest-fortify", "ctest-fortify-abort"])
    check("part: lseek", names["lseek"], ["fastpy-lseek"])
    check("none: all four", len(names[None]), 4)


def main() -> int:
    cf = load_module()
    tests = [(name, fn) for name, fn in sorted(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 14:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 14. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        with tempfile.TemporaryDirectory() as tmpdir:
            args = {"cf": cf, "tmpdir": tmpdir}
            fn(**{p: args[p] for p in params if p in args})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} ctest-fixtures sysroot tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
