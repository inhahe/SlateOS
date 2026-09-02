#!/usr/bin/env python3
"""Tests for `scripts/gittree.py` — reading many blobs out of one git process.

Run: `python scripts/test-gittree.py` (0 = pass, 1 = fail). No pytest
dependency, matching the other suites in this directory.

Why a separate suite from `test-pre-push-fmt-gate.py`
----------------------------------------------------

That one asks "does gate 7 reach the right verdict", which is the question
that matters, and it does exercise this module -- every case runs once with
the batch path and once without. But it can only ever see this module through
rustfmt's opinion of the files it wrote, so it cannot distinguish "the bytes
are right" from "the bytes are wrong in a way rustfmt does not mind", and it
says nothing at all about the parts gate 7 does not use.

The parts that need their own assertions are the ones where the batch protocol
can go quietly wrong:

* **Byte-identity.** The whole point is to substitute for `git cat-file blob`.
  If it is off by the trailing newline git appends after each payload, small
  text files still look fine and a binary file silently corrupts.
* **Not desynchronising.** The protocol is one request, one response, down one
  pipe. A missing object answers with a one-line "missing" and *no* payload;
  read a payload for it anyway and every later answer is shifted by one, which
  shows up as the wrong file's contents under the right file's name -- the
  worst possible failure for a tool whose job is to say what is in a file.
* **Large blobs.** `read(n)` on a pipe is entitled to return short. A 5.7 MB
  file is many pipe buffers, so it is the case that catches a naive read.
* **The stub rules.** A stub that overwrites a real sibling deletes that
  sibling's verdict; a stub that is zero bytes makes rustfmt report a diff on
  a file this module invented.
"""

from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gitenv  # noqa: E402

_REMOVED = gitenv.scrub_environ()

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(HERE)

# `gittree.py` is not an importable module name for `import gittree` to find in
# every caller's sys.path arrangement, and this suite must load exactly the
# file next to it rather than whatever else is named that.
_spec = importlib.util.spec_from_file_location(
    "gittree_under_test", os.path.join(HERE, "gittree.py")
)
assert _spec is not None and _spec.loader is not None
gittree = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gittree)

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
                          capture_output=True, check=False)


def build_repo(tmp: str) -> str:
    """A repo whose committed content is deliberately awkward.

    Every file here exists to break a specific plausible implementation, and
    the comment on each says which.
    """
    work = os.path.join(tmp, "r")
    os.makedirs(work, exist_ok=True)
    git(work, "init", "--quiet", "-b", "main", ".")
    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    git(work, "config", "commit.gpgsign", "false")
    # `core.autocrlf=false` so the bytes committed are the bytes written: this
    # suite asserts byte-identity, and an inherited autocrlf would make the
    # assertion about git's filters rather than about this module.
    git(work, "config", "core.autocrlf", "false")

    files: dict[str, bytes] = {
        # Ordinary.
        "src/main.rs": b"fn main() {}\n",
        # No trailing newline: git's batch protocol appends one after the
        # payload regardless, so a reader that trusts the stream instead of the
        # declared size gains a byte here.
        "src/nonl.rs": b"fn a() {}",
        # Empty: size 0, and the payload is *only* the appended newline.
        "src/empty.rs": b"",
        # CRLF held deliberately, to catch a text-mode read on Windows.
        "src/crlf.rs": b"fn b() {}\r\nfn c() {}\r\n",
        # NUL and high bytes: a text-mode or utf-8-decoding read mangles these.
        "data/blob.bin": bytes(range(256)) * 8,
        # Bigger than any pipe buffer, so a single `read(n)` will come back
        # short unless the reader loops.
        "data/big.bin": (b"0123456789abcdef" * 64 + b"\n") * 6000,
        # A module root whose children are the stub rules' subject matter.
        "mods/lib.rs": (
            b"//! Root.\n"
            b"pub mod present;\n"
            b"mod absent;\n"
            b"    pub(crate) mod indented;\n"
            b"pub mod dirmod;\n"
        ),
        "mods/present.rs": b"pub fn p() {}\n",
        "mods/dirmod/mod.rs": b"pub fn d() {}\n",
    }
    for rel, body in files.items():
        path = os.path.join(work, *rel.split("/"))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as fh:
            fh.write(body)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "content")
    return work


# --------------------------------------------------------------------------
# Cases
# --------------------------------------------------------------------------


def case_bytes_match_git(work: str) -> None:
    """Every blob, compared against `git cat-file blob` itself.

    The oracle is git, not a copy of the literals above: this is asserting
    that the batch protocol is read correctly, and a comparison against what
    the test wrote would still pass if git had transformed the content on the
    way in and this module transformed it back differently.
    """
    rels = subprocess.run(
        ["git", "ls-tree", "-r", "--name-only", "HEAD"], cwd=work,
        env=gitenv.clean_env(), capture_output=True, check=False,
    ).stdout.decode().split()
    mismatched = []
    with gittree.GitTree(work) as tree:
        for rel in rels:
            want = subprocess.run(
                ["git", "cat-file", "blob", "HEAD:" + rel], cwd=work,
                env=gitenv.clean_env(), capture_output=True, check=False,
            ).stdout
            if tree.read("HEAD", rel) != want:
                mismatched.append(rel)
    check("every blob is byte-identical to git cat-file", mismatched, [])
    check("the repo actually had files to compare", len(rels) > 6, True)


def case_missing_does_not_desync(work: str) -> None:
    """A missing path answers None and leaves the pipe usable.

    The interleaving matters more than the None: a reader that consumed a
    payload for the missing object would return `main.rs`'s bytes for
    `big.bin`, and every assertion after it would be about the wrong file.
    """
    with gittree.GitTree(work) as tree:
        first = tree.read("HEAD", "src/main.rs")
        gone = tree.read("HEAD", "no/such/path.rs")
        again = tree.read("HEAD", "src/main.rs")
        big = tree.read("HEAD", "data/big.bin")
    check("a missing path reads as None", gone, None)
    check("the read after a missing path is still correct", again, first)
    check("a large blob after a missing path is intact",
          big is not None and len(big) == (16 * 64 + 1) * 6000, True)


def case_read_after_close(work: str) -> None:
    """Using a closed GitTree raises rather than hanging on a dead pipe."""
    tree = gittree.GitTree(work)
    tree.read("HEAD", "src/main.rs")
    tree.close()
    tree.close()  # idempotent
    try:
        tree.read("HEAD", "src/main.rs")
    except gittree.GitTreeError:
        check("read after close raises GitTreeError", True, True)
    else:
        check("read after close raises GitTreeError", False, True)


def case_list_paths(work: str) -> None:
    with gittree.GitTree(work) as tree:
        every = tree.list_paths("HEAD")
        narrowed = tree.list_paths("HEAD", "mods")
    check("list_paths finds every committed file", len(every), 9)
    check("list_paths honours a pathspec", sorted(narrowed),
          ["mods/dirmod/mod.rs", "mods/lib.rs", "mods/present.rs"])


def case_materialise_layout(work: str) -> None:
    """Files land at their real relative paths, under forward slashes only.

    The relative layout is what lets a tool resolve siblings; the separator is
    what lets the caller strip the root back off to name a file the user can
    open. `os.path.join` would satisfy the first and quietly break the second.
    """
    with tempfile.TemporaryDirectory() as dest:
        written = gittree.materialise(
            "HEAD", dest, ["src/main.rs", "data/blob.bin", "src/nonl.rs"],
            stub_rust_mods=False, repo=work,
        )
        check("materialise reports what it wrote", len(written), 3)
        check("no backslash reaches the caller",
              [p for p in written if "\\" in p], [])
        check("paths keep their tree layout",
              [p.endswith("/src/main.rs") for p in written][:1], [True])
        with open(os.path.join(dest, "data", "blob.bin"), "rb") as fh:
            check("a binary file survives materialise",
                  fh.read(), bytes(range(256)) * 8)


def case_materialise_skips_absent(work: str) -> None:
    """A path not present at the revision is skipped, not invented.

    `git log --name-only` lists a file added by one commit in a push and
    deleted by a later one, so asking for something that is not there is
    normal traffic, not an error.
    """
    with tempfile.TemporaryDirectory() as dest:
        written = gittree.materialise(
            "HEAD", dest, ["src/main.rs", "src/never.rs"],
            stub_rust_mods=False, repo=work,
        )
        check("an absent path is dropped", len(written), 1)
        check("and no empty file is left in its place",
              os.path.exists(os.path.join(dest, "src", "never.rs")), False)


def case_stub_rules(work: str) -> None:
    """The four things the stub pass must get right.

    A stub exists so a tool that resolves `mod name;` relative to the file it
    is handed can proceed without dragging untouched siblings into the
    caller's verdict. It must therefore appear for every unresolved `mod`, must
    never displace a real sibling that is *in the list*, and must not be zero
    bytes -- rustfmt reports a diff on an empty file, which would make every
    stub look like a finding against a file nobody wrote.

    "In the list" is the whole rule, and it is worth being explicit that it is
    not "in the repository". A child that exists at the revision but is not in
    this push is exactly the file the stub is protecting from the verdict, so
    it gets stubbed over -- including a `name/mod.rs` directory module, whose
    stub sits at `name.rs` and would collide with it if both were present.
    That collision cannot happen, because the mirror only ever holds what the
    caller asked for. Both directions are asserted below.
    """

    def stub_run(paths: list[str]) -> str:
        dest = tempfile.mkdtemp()
        gittree.materialise("HEAD", dest, paths, stub_rust_mods=True, repo=work)
        return os.path.join(dest, "mods")

    mods = stub_run(["mods/lib.rs", "mods/present.rs"])

    def body(name: str) -> bytes | None:
        try:
            with open(os.path.join(mods, name), "rb") as fh:
                return fh.read()
        except OSError:
            return None

    check("an unresolved `mod` gets a stub", body("absent.rs"), b"\n")
    check("`pub(crate) mod`, indented, is recognised too",
          body("indented.rs"), b"\n")
    check("a real sibling in the list keeps its own bytes",
          body("present.rs"), b"pub fn p() {}\n")
    check("the stub is not zero bytes",
          os.path.getsize(os.path.join(mods, "absent.rs")), 1)
    check("a directory module NOT in the list is stubbed like any other child",
          body("dirmod.rs"), b"\n")

    both = stub_run(["mods/lib.rs", "mods/dirmod/mod.rs"])
    check("a directory module that IS in the list is not shadowed",
          os.path.exists(os.path.join(both, "dirmod.rs")), False)
    with open(os.path.join(both, "dirmod", "mod.rs"), "rb") as fh:
        check("and keeps its own bytes", fh.read(), b"pub fn d() {}\n")


def case_cli_emits_lf(work: str) -> None:
    """The CLI's output is LF-separated on every platform.

    This is the defect that shipped: `print` on Windows emits `\\r\\n`, the
    shell's `IFS= read -r` strips only the `\\n`, and gate 7 then handed
    rustfmt a filename with a carriage return on the end. rustfmt said "does
    not exist", and the gate reads any rustfmt failure as drift -- so a change
    meant to make pushes faster refused every clean file in one instead.
    Asserted on the raw bytes, because that is the only place it is visible.
    """
    with tempfile.TemporaryDirectory() as dest:
        proc = subprocess.run(
            [sys.executable, os.path.join(HERE, "gittree.py"), "materialise",
             "--rev", "HEAD", "--dest", dest],
            cwd=work, env=gitenv.clean_env(),
            input=b"src/main.rs\nsrc/nonl.rs\n",
            capture_output=True, check=False,
        )
        check("the CLI succeeds", proc.returncode, 0)
        check("no CR reaches stdout", b"\r" in proc.stdout, False)
        check("one line per file written",
              len([b for b in proc.stdout.split(b"\n") if b]), 2)


def case_cli_reports_crlf_input(work: str) -> None:
    """A path list that arrived with CRLF still names real files.

    The producer is a POSIX shell today, but this is a shared helper and the
    next caller may be a Windows Python. Tolerating the terminator on the way
    in costs nothing and removes a failure mode that is invisible on inspection
    -- the whole reason the mirrored defect above got as far as it did.
    """
    with tempfile.TemporaryDirectory() as dest:
        proc = subprocess.run(
            [sys.executable, os.path.join(HERE, "gittree.py"), "materialise",
             "--rev", "HEAD", "--dest", dest],
            cwd=work, env=gitenv.clean_env(),
            input=b"src/main.rs\r\nsrc/nonl.rs\r\n",
            capture_output=True, check=False,
        )
        check("CRLF input still materialises both files",
              len([b for b in proc.stdout.split(b"\n") if b]), 2)
        check("and the file is really there",
              os.path.exists(os.path.join(dest, "src", "main.rs")), True)


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        work = build_repo(tmp)
        for case in (case_bytes_match_git, case_missing_does_not_desync,
                     case_read_after_close, case_list_paths,
                     case_materialise_layout, case_materialise_skips_absent,
                     case_stub_rules, case_cli_emits_lf,
                     case_cli_reports_crlf_input):
            case(work)

    if failures:
        print(f"\n{len(failures)} gittree test(s) failed:", file=sys.stderr)
        for name in failures:
            print(f"  - {name}", file=sys.stderr)
        return 1
    print("\nall gittree tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
