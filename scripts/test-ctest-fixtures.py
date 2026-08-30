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

import importlib.util
import inspect
import os
import sys
import tempfile
import time
from pathlib import Path

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "ctest-fixtures.py")

_FAILURES: list[str] = []


def load_module():
    """Import ctest-fixtures.py by path (its name is not a valid identifier)."""
    spec = importlib.util.spec_from_file_location("ctest_fixtures", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


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
    ps1 = repo / "toolchain" / "build-sysroot.ps1"
    ps1.write_text(ps1.read_text(encoding="utf-8"), encoding="utf-8")
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
    (repo / "posix" / "src" / "io.rs").write_text("pub fn b() { changed(); }\n", encoding="utf-8")

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
    (repo / "posix" / "src" / "new.rs").write_text("pub fn d() {}\n", encoding="utf-8")

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
    (out / "build.rs").write_text("// generated\n", encoding="utf-8")

    labels = [label for label, _p, _t in cf._sysroot_inputs()]
    check("target/ contributes no inputs",
          [lb for lb in labels if "/target/" in lb], [])


def test_dot_dirs_are_excluded(cf, tmpdir):
    repo = _fake_tree(cf, tmpdir)
    hidden = repo / "posix" / "src" / ".cache"
    hidden.mkdir()
    (hidden / "junk.rs").write_text("noise\n", encoding="utf-8")

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
    cf.SYSROOT_STAMP.write_text("", encoding="utf-8")
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
