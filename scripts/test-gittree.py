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


# --------------------------------------------------------------------------
# Tree cases
# --------------------------------------------------------------------------


def build_tree_repo(tmp: str, name: str = "t") -> str:
    """A repo whose disk and whose HEAD are deliberately the same.

    Same content both sides, so that `case_tree_agrees` can assert exact
    equality and mean it. The only things on disk that are not in the commit
    are build directories, which is the arrangement every real checkout is in
    and exactly what the prune rule exists to paper over.
    """
    work = os.path.join(tmp, name)
    os.makedirs(work, exist_ok=True)
    git(work, "init", "--quiet", "-b", "main", ".")
    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    git(work, "config", "commit.gpgsign", "false")
    git(work, "config", "core.autocrlf", "false")

    files: dict[str, bytes] = {
        "src/lib.rs": b"//! Root.\npub fn a() {}\n",
        # `target` as a *substring* of a real source file name. A prune rule
        # written as `"target" in rel` hides this file, and hides it silently:
        # the checker reports no findings for a file it never opened.
        "src/target_arch.rs": b"pub const ARCH: &str = \"x86_64\";\n",
        # The same trap one character further along. `target-` is the prefix
        # the seam prunes to match `.gitignore`'s `**/target-*/`, and that
        # pattern's trailing slash means directories only -- so this *file* is
        # tracked source and must survive every listing. A prune rule that
        # forgot the distinction would hide it from both sides at once, which
        # is the failure mode that leaves a checker reporting a clean tree.
        "posix/src/target-arch.rs": b"pub const A: u8 = 1;\n",
        # And the trap one step out again: a tracked *directory* whose name
        # merely *contains* `target-`. The rule is a prefix test; spelled as a
        # substring it would swallow this whole subtree, on both sides at once.
        "posix/src/cross-target-tests/keep.rs": b"pub fn keep() {}\n",
        "posix/src/a.rs": b"pub fn b() {}\n",
        "posix/src/deep/b.rs": b"pub fn c() {}\n",
        "docs/readme.md": b"# docs\n",
        # Not valid UTF-8 anywhere: `read_text` must replace rather than raise,
        # and `read_bytes` must hand back these bytes untouched.
        "bin.dat": b"\xff\xfe\x00\x01ok\x80\x81",
    }
    for rel, body in files.items():
        path = os.path.join(work, *rel.split("/"))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as fh:
            fh.write(body)
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "content")

    # Build output, written *after* the commit and never added -- which is
    # what `target/` always is. Two of them, one nested, because the prune
    # must be a component test at any depth rather than a check of the first
    # path element.
    # `target-lint/` and friends: alternate cargo build dirs, gitignored by
    # `**/target-*/`. On disk whenever a lane is mid-build, never in any
    # revision -- so an unpruned one makes the seam's answer depend on whether
    # another lane is running clippy right now. One at the root and one nested,
    # for the same reason `target/` has two.
    for rel in ("target/debug/junk.rs", "posix/src/target/leftover.rs",
                "target-lint/debug/junk.rs",
                "posix/src/target-hl2/leftover.rs"):
        path = os.path.join(work, *rel.split("/"))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as fh:
            fh.write(b"// build output\n")
    return work


def case_tree_agrees(work: str) -> None:
    """The seam's whole promise: a checker cannot tell the two apart.

    Asserted as exact equality over a checkout whose disk matches its HEAD,
    because "close enough" is not a property a checker can rely on -- gate 11
    resolves a link in one file against a definition in another, so a single
    path present on one side and absent on the other turns a real finding
    into a false one, or hides it.

    Content is compared too, not just the path lists. Two trees can enumerate
    identically and still disagree about every byte, which is precisely the
    failure mode of a checker that lists from git and then reads the disk --
    the bug this seam exists to make impossible.
    """
    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        for prefix in ("", "src", "posix", "posix/src", "docs"):
            check(f"files_under({prefix!r}) agrees",
                  disk.files_under(prefix), rev.files_under(prefix))
            check(f"entries({prefix!r}) agrees",
                  disk.entries(prefix), rev.entries(prefix))

        every = rev.files_under("")
        check("the fixture is the eight committed files", len(every), 8)
        check("every file's bytes agree",
              [p for p in every if disk.read_bytes(p) != rev.read_bytes(p)], [])
        check("every file is a file on both",
              [p for p in every if not (disk.is_file(p) and rev.is_file(p))], [])
        for d in ("src", "posix", "posix/src", "posix/src/deep"):
            check(f"is_dir({d!r}) agrees", disk.is_dir(d), rev.is_dir(d))


def case_tree_reads_the_commit_not_the_disk(work: str) -> None:
    """The bug being fixed, stated directly.

    Seven gates enumerate from the pushed commits and then read the disk, so
    a commit that introduces a fault passes whenever the disk has since been
    fixed, and a clean commit fails whenever the disk is dirty. `RevTree` must
    therefore be blind to the working tree in all three directions -- edited,
    added, and removed -- and `WorkTree` must still see all three, because the
    same checkers are run by hand and by the boot test where the disk is the
    right answer.

    A dedicated fixture rather than mutating a shared one: a suite whose cases
    must run in a particular order is a trap for whoever adds the next case.
    """
    edited = os.path.join(work, "src", "lib.rs")
    with open(edited, "wb") as fh:
        fh.write(b"//! Edited on disk, never committed.\n")
    with open(os.path.join(work, "src", "added.rs"), "wb") as fh:
        fh.write(b"pub fn added() {}\n")
    os.remove(os.path.join(work, "docs", "readme.md"))

    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        check("the disk shows the edit",
              disk.read_text("src/lib.rs").startswith("//! Edited"), True)
        check("the revision does not",
              rev.read_text("src/lib.rs").startswith("//! Root."), True)

        check("the disk shows a file added since the commit",
              disk.is_file("src/added.rs"), True)
        check("the revision does not", rev.is_file("src/added.rs"), False)
        check("and the revision does not list it",
              "src/added.rs" in rev.files_under("src"), False)

        check("the disk shows a file deleted since the commit as gone",
              disk.is_file("docs/readme.md"), False)
        check("the revision still has it", rev.is_file("docs/readme.md"), True)
        check("...and can still read it",
              rev.read_text("docs/readme.md"), "# docs\n")


def case_tree_agrees_in_a_linked_worktree(work: str) -> None:
    """The shape the gates actually run in, where `.git` is a file.

    Every lane works in a `git worktree`, and in one of those `.git` is not a
    directory but a one-line file pointing at the real object store. A prune
    that filters only `os.walk`'s `dirnames` therefore misses it *everywhere
    the hook is ever used*, while passing a fixture built with `git init`,
    where `.git` is a directory and the filter does apply.

    So this case does not build its fixture the convenient way. A suite whose
    fixture differs from the deployment in exactly the respect under test is
    the reason the defect it is meant to catch survives.
    """
    linked = os.path.join(os.path.dirname(work), "linked-" + os.path.basename(work))
    git(work, "worktree", "add", "--detach", "--quiet", linked, "HEAD")
    dot = os.path.join(linked, ".git")
    check("the fixture reproduces the real layout: .git is a file",
          os.path.isfile(dot), True)

    with gittree.WorkTree(linked) as disk, gittree.RevTree("HEAD", linked) as rev:
        check("the disk does not offer .git as a file",
              [p for p in disk.files_under("") if p == ".git"], [])
        check("files_under('') still agrees with the revision",
              disk.files_under(""), rev.files_under(""))
        check("entries('') still agrees with the revision",
              disk.entries(""), rev.entries(""))


def case_prune_is_by_component(work: str) -> None:
    """`target` the directory is skipped; `target_arch.rs` is not.

    A substring rule passes every other test in this file and fails only
    here, silently, by never opening a source file -- so the discriminating
    case is the one worth spending a fixture on.
    """
    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        for label, tree in (("disk", disk), ("rev", rev)):
            files = tree.files_under("")
            check(f"{label}: a file merely named target-something is kept",
                  "src/target_arch.rs" in files, True)
            check(f"{label}: nothing under a build directory is listed",
                  [p for p in files if "target" in p.split("/")], [])
            check(f"{label}: nor anything from .git",
                  [p for p in files if ".git" in p.split("/")], [])
            check(f"{label}: build dirs are not offered as entries",
                  [e for e, _ in tree.entries("posix/src")
                   if e.endswith("/target")], [])

        # The disk really does hold what the listing declines to show, so the
        # empty results above are the rule working rather than an empty tree.
        check("the build output is genuinely on disk",
              os.path.isfile(os.path.join(work, "target", "debug", "junk.rs")),
              True)
        check("asking for a build directory by name yields nothing anyway",
              disk.files_under("target"), [])
        check("...on both sides", rev.files_under("target"), [])


def case_alternate_build_dirs_are_pruned_by_shape(work: str) -> None:
    """`target-lint/` is build output; `target-arch.rs` is source.

    `.gitignore` matches the first family as `**/target-*/` and says why the
    shape rather than the names: they are however many concurrent cargo
    invocations a lane happens to have, named for whatever they were doing.
    The seam needs the same rule for a reason `.gitignore` does not have --
    these directories are gitignored, so `RevTree` can never list one, and
    they are on the disk whenever a lane is mid-build, so `WorkTree` lists all
    of it. Unpruned, the seam's answer depends on whether *another lane* is
    running clippy at that moment, which is the least actionable kind of
    disagreement a gate can have.

    The trailing slash in the `.gitignore` pattern is load-bearing and is the
    other half of this case: it matches directories only. A rule that pruned
    the prefix wherever it appeared would also hide the tracked file
    `posix/src/target-arch.rs` -- from *both* sides, so `case_tree_agrees`
    would stay green while a source file silently stopped being scanned.

    **One deliberate gap, recorded rather than papered over.** Mutating
    `RevTree.files_under`'s guard from `_pruned_prefix` back to `_pruned`
    survives this suite, and no assertion can catch it, because the two are
    genuinely indistinguishable there. They differ only when a prefix's *last*
    component starts with `target-`, and on a revision that case is already
    decided before the guard runs: either the prefix names a tracked file, and
    the `_fileset` branch answers first, or it names a directory, and
    `RevTree.__init__` has already dropped every path under it from the index,
    so the `startswith` filter returns the empty list the guard would have
    returned. The guard is kept anyway -- it is the correct call if the index
    filter ever changes, and the seam's rule is that both implementations
    spell the same thing the same way. That equivalence rests entirely on the
    index being pre-filtered, which is itself asserted ("disk and revision
    still list the same files", and the `target-hl2` checks above), so the day
    it stops holding this suite goes red rather than quietly losing the guard.
    """
    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        for label, tree in (("disk", disk), ("rev", rev)):
            files = tree.files_under("")
            check(f"{label}: an alternate build dir is skipped at the top",
                  [p for p in files if p.startswith("target-lint/")], [])
            check(f"{label}: ...and nested, at any depth",
                  [p for p in files if "target-hl2" in p.split("/")], [])
            check(f"{label}: a tracked file named target-something is kept",
                  "posix/src/target-arch.rs" in files, True)
            # `startswith`, not `in`. A substring spelling passes every other
            # assertion here and silently drops this subtree from both trees.
            check(f"{label}: a directory merely containing 'target-' is kept",
                  "posix/src/cross-target-tests/keep.rs" in files, True)
            # The discriminating query: asked for by its own name, a file is a
            # file even though its name starts with a pruned directory prefix.
            check(f"{label}: ...and is still returned when asked for by name",
                  tree.files_under("posix/src/target-arch.rs"),
                  ["posix/src/target-arch.rs"])
            check(f"{label}: an alternate build dir asked for by name is empty",
                  tree.files_under("target-lint"), [])
            check(f"{label}: nor is it offered as an entry",
                  [e for e, _ in tree.entries("")
                   if e.startswith("target-")], [])

        # As above: the empty results are the rule working, not an empty tree.
        check("the alternate build output is genuinely on disk",
              os.path.isfile(os.path.join(work, "target-lint", "debug",
                                          "junk.rs")),
              True)
        # And the two sides still agree exactly, which is the property all of
        # this exists to protect.
        check("disk and revision still list the same files",
              disk.files_under(""), rev.files_under(""))


def case_a_build_dir_prefix_is_not_walked(work: str) -> None:
    """The guard that looks redundant, and the reason it is not.

    `files_under("target")` answers `[]` with or without `WorkTree`'s prefix
    check, because the per-file prune drops everything the walk turns up. The
    difference is not the answer, it is the work: without the guard the walk
    descends the entire build directory in order to discard every result, and
    on this repository that is minutes of `stat()` happening inside a push
    gate.

    A mutation that deletes the guard is therefore invisible to every
    assertion about return values -- it survived the first clean mutation run
    for exactly that reason -- so this case asserts about the walk itself.
    Without it, the next reader deletes a line that provably changes nothing.
    """
    seen: list[str] = []
    real_walk = os.walk

    def counting_walk(top, *a, **kw):
        seen.append(top)
        return real_walk(top, *a, **kw)

    # Patched on the `os` module itself because `gittree` says `import os` and
    # resolves `os.walk` per call. Restored in `finally`; this suite is
    # single-threaded, so nothing else can observe the swap.
    os.walk = counting_walk
    try:
        with gittree.WorkTree(work) as disk:
            check("a build-dir prefix still answers nothing",
                  disk.files_under("target"), [])
            check("...and does so without walking it at all", seen, [])
            # The control. Without this, a probe that silently stopped
            # working would look identical to the property holding.
            seen.clear()
            check("a real prefix does return files",
                  bool(disk.files_under("posix")), True)
            check("...so the probe would have noticed a walk",
                  len(seen), 1)
            # The same property for the `target-*` family, which reaches the
            # guard through `_pruned_prefix` rather than through `_PRUNE`. It
            # needs its own assertion for the same reason the case exists: a
            # guard that used file rules here would let the walk descend and
            # still answer `[]`, so no assertion about results can see it.
            seen.clear()
            check("an alternate build-dir prefix also answers nothing",
                  disk.files_under("target-lint"), [])
            check("...and is likewise never walked", seen, [])
    finally:
        os.walk = real_walk


def case_the_walk_never_descends_into_build_output(work: str) -> None:
    """Pruning `dirnames`, not filtering results -- asserted about the walk.

    `files_under("")` answers the same list whether or not the walk descends
    into a build directory, because the per-file prune drops everything it
    would find there. The difference is the work, and on this repository a
    `target/` is tens of gigabytes: the answer arrives either way, minutes
    apart, inside a push gate.

    So this records every directory `os.walk` actually yields, rather than
    counting calls -- `os.walk` is invoked once per `files_under` however deep
    it goes, so a call count cannot see a descent at all.
    """
    visited: list[str] = []
    real_walk = os.walk

    def recording_walk(top, *a, **kw):
        # Re-yields the very list object `real_walk` handed over, because
        # `gittree` prunes it in place and `os.walk` reads that mutation back
        # to decide where to go next. Copying it would break the pruning this
        # case exists to observe.
        for dirpath, dirnames, filenames in real_walk(top, *a, **kw):
            visited.append(dirpath.replace("\\", "/"))
            yield dirpath, dirnames, filenames

    os.walk = recording_walk
    try:
        with gittree.WorkTree(work) as disk:
            files = disk.files_under("")
            # Directory components only -- the last component is the file's own
            # name, and `posix/src/target-arch.rs` is tracked source that must
            # stay in this list. Written the other way round, this assertion
            # demanded the seam hide it.
            check("the listing itself is clean of build output",
                  [p for p in files
                   if any(c == "target" or c.startswith("target-")
                          for c in p.split("/")[:-1])],
                  [])
            check("...and the walk never entered a plain target directory",
                  [d for d in visited if "target" in d.split("/")], [])
            check("...nor an alternate one",
                  [d for d in visited
                   if any(c.startswith("target-") for c in d.split("/"))],
                  [])
            # The control: without it, a probe that quietly stopped recording
            # would look exactly like the property holding.
            check("the probe was in fact recording directories",
                  any(d.endswith("/posix/src") for d in visited), True)
            # And the walk did reach real source, so the empty lists above are
            # the prune working rather than a walk that never started.
            check("...including one that merely contains 'target-'",
                  any(d.endswith("/cross-target-tests") for d in visited),
                  True)
    finally:
        os.walk = real_walk


def case_a_callers_own_prune_list_reaches_both_sides(work: str) -> None:
    """`files_under(..., prune=(...))`: the caller's skip list, on both trees.

    `_PRUNE` is the seam's own list and is not negotiable. Checkers have their
    own on top of it -- `raced-globals.py` skips `node_modules`, `vendor` and
    `third_party`, because a data race in vendored source is not this project's
    to report and never was. Before this parameter existed the only way to
    honour such a list was to filter the results, which gives the same answer
    and does the work anyway: a full descent of a vendored tree, inside a push
    gate, to throw every result away.

    Three things have to hold at once, and each is a way to get this wrong:
    the two implementations must agree (or the conversion changes findings);
    the rule must be component-wise, like `_PRUNE`'s (or `vendor_shim.rs`
    silently stops being scanned); and the default must be unchanged (or every
    existing caller quietly acquires a skip list it never asked for).
    """
    for rel in ("vendor/dep/x.rs", "posix/src/vendor/deep/y.rs",
                "posix/src/vendor_shim.rs",
                # A tracked *file* whose whole path is a name a caller prunes.
                # `files_under` has an early return for a prefix that names a
                # file, and that return is before the results filter -- so this
                # is the one shape where the prefix guard is the only thing
                # answering, on either implementation.
                "third_party"):
        path = os.path.join(work, *rel.split("/"))
        os.makedirs(os.path.dirname(path) or work, exist_ok=True)
        with open(path, "wb") as fh:
            fh.write(b"pub fn v() {}\n")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "vendored source, tracked")

    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        for label, tree in (("disk", disk), ("rev", rev)):
            plain = tree.files_under("")
            check(f"{label}: without a prune list, vendored files are listed",
                  "vendor/dep/x.rs" in plain, True)

            pruned = tree.files_under("", prune=("vendor",))
            check(f"{label}: a named directory is skipped at the top level",
                  "vendor/dep/x.rs" in pruned, False)
            check(f"{label}: ...and at any depth",
                  "posix/src/vendor/deep/y.rs" in pruned, False)
            # The same trap `_PRUNE` has: a substring rule passes both
            # assertions above and stops the checker ever opening this file.
            check(f"{label}: ...but a file merely named vendor-something is kept",
                  "posix/src/vendor_shim.rs" in pruned, True)
            check(f"{label}: nothing else is dropped",
                  sorted(set(plain) - set(pruned)),
                  ["posix/src/vendor/deep/y.rs", "vendor/dep/x.rs"])
            check(f"{label}: a plain file the caller did not name is kept",
                  "third_party" in pruned, True)
            check(f"{label}: asking for the pruned directory by name yields nothing",
                  tree.files_under("vendor", prune=("vendor",)), [])
            # The early return for a prefix that *is* a file happens before the
            # results are filtered, so only the prefix guard can answer here.
            # Without it one side hands back the file and the other does not.
            check(f"{label}: nor does asking for a pruned path that is a file",
                  tree.files_under("third_party", prune=("third_party",)), [])
            check(f"{label}: ...which is a real file when nobody prunes it",
                  tree.files_under("third_party"), ["third_party"])
            check(f"{label}: ...and the seam's own list still applies",
                  [p for p in pruned if "target" in p.split("/")], [])
            # A caller's list is *added* to `_PRUNE`, never substituted for it.
            # Substituting passes every assertion above, because the seam's own
            # names are pruned a second time while walking and while indexing --
            # it shows up only where `_PRUNE` alone decides, which is here.
            check(f"{label}: a build directory is still refused by name",
                  tree.files_under("target", prune=("vendor",)), [])

    # And it must prune *while walking*, not filter afterwards -- which is the
    # entire reason the parameter exists rather than the caller using a list
    # comprehension. No assertion about results can tell the two apart, so this
    # one is about the syscalls, as `case_a_build_dir_prefix_is_not_walked` is.
    scanned: list[str] = []
    real_scandir = os.scandir

    def counting_scandir(path=".", *a, **kw):
        scanned.append(str(path).replace("\\", "/"))
        return real_scandir(path, *a, **kw)

    os.scandir = counting_scandir
    try:
        with gittree.WorkTree(work) as disk:
            disk.files_under("", prune=("vendor",))
            check("the vendored directory is never even scanned",
                  [p for p in scanned if "vendor" in p.split("/")], [])
            # The control: without it, a probe that had stopped observing
            # anything would look exactly like the property holding.
            check("...and the probe was in fact observing scans",
                  len(scanned) > 1, True)
    finally:
        os.scandir = real_scandir


def case_missing_is_an_answer(work: str) -> None:
    """Absent is not an error, at any of the six entry points.

    Every checker converted to this seam walks a path list from git and reads
    each one; a push that adds a file and later deletes it puts a path in that
    list which is not there. If that raised, the gate would abort rather than
    judge -- and an aborted gate is exit 126, which the hook refuses to treat
    as a verdict. Absence has to be a value.
    """
    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        for label, tree in (("disk", disk), ("rev", rev)):
            probes = (
                ("read_bytes", lambda t: t.read_bytes("no/pe.rs"), None),
                ("read_text", lambda t: t.read_text("no/pe.rs"), None),
                ("is_file", lambda t: t.is_file("no/pe.rs"), False),
                ("is_dir", lambda t: t.is_dir("no/pe"), False),
                ("files_under", lambda t: t.files_under("no/pe"), []),
                ("entries", lambda t: t.entries("no/pe"), []),
            )
            for name, probe, want in probes:
                # The raise is caught here rather than left to propagate so it
                # is reported as *this* assertion failing. An escaping
                # exception is red too, but it aborts the run at the first
                # method and names none of them -- and "absence must not
                # raise" is the whole point of the case.
                try:
                    got: object = probe(tree)
                except Exception as exc:  # noqa: BLE001 - any raise is the bug
                    got = f"raised {type(exc).__name__}"
                check(f"{label}: {name} of a missing path", got, want)


def case_entries_flags_directories(work: str) -> None:
    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        # `posix/src` also holds `target/` and `target-hl2/` on disk, both
        # build output and neither in the commit, so this list doubles as the
        # entry-level statement of the prune rule: the two *directories* are
        # absent from both sides, while `target-arch.rs` -- a file whose name
        # merely starts the same way -- is present on both.
        want = [("posix/src/a.rs", False),
                ("posix/src/cross-target-tests", True),
                ("posix/src/deep", True),
                ("posix/src/target-arch.rs", False)]
        check("the disk marks which children are directories",
              disk.entries("posix/src"), want)
        check("and the revision infers the same from its path list",
              rev.entries("posix/src"), want)


def case_read_text_survives_bad_bytes(work: str) -> None:
    """Scan the file, do not skip it.

    The checkers look for patterns in source, and a file with one bad byte is
    still a file whose other 40 KB may contain a finding. `read_bytes` stays
    exact for the callers that need it.
    """
    raw = b"\xff\xfe\x00\x01ok\x80\x81"
    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        for label, tree in (("disk", disk), ("rev", rev)):
            check(f"{label}: bytes come back untouched", tree.read_bytes("bin.dat"), raw)
            text = tree.read_text("bin.dat")
            check(f"{label}: text is decoded, not refused", isinstance(text, str), True)
            check(f"{label}: the readable part is still readable", "ok" in text, True)
            check(f"{label}: strict decoding is available and does raise",
                  _raises(lambda: tree.read_text("bin.dat", errors="strict")), True)


def case_files_under_a_file(work: str) -> None:
    """A prefix that names a file is that one file, not nothing.

    The checkers' path lists mix directories and files freely -- gate 11 is
    handed crate directories, gate 8 individual sources -- so the two have to
    behave the same way through one call.
    """
    with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
        check("the disk answers with the file itself",
              disk.files_under("src/lib.rs"), ["src/lib.rs"])
        check("and so does the revision",
              rev.files_under("src/lib.rs"), ["src/lib.rs"])


def case_open_tree_chooses(work: str) -> None:
    """`--head` becomes a choice in exactly one place."""
    with gittree.open_tree(work) as absent:
        check("no head means the working tree",
              type(absent).__name__, "WorkTree")
    with gittree.open_tree(work, "HEAD") as present:
        check("a head means that revision",
              type(present).__name__, "RevTree")
        check("...and it is the revision that was asked for",
              present.rev, "HEAD")


def case_an_ambient_git_dir_does_not_redirect_the_read(work: str) -> None:
    """`-C` / `cwd` name a repository only until `GIT_DIR` disagrees.

    Every caller here selects its repository positionally -- `GitTree(repo)`
    passes `cwd=repo`, `list_paths` does the same -- and that is correct right
    up to the moment something in the environment has already chosen one. Git
    exports `GIT_DIR` into every hook, into `git bisect run`, into
    `git rebase --exec`; `scripts/gitenv.py` documents the full list and the
    2026-08-29 post-mortem behind it.

    The module docstring used to answer this with "nothing here writes to the
    repository: `cat-file` and `ls-tree` are reads." That is true and it is not
    the point. A read of the *wrong* repository does not corrupt anything; it
    silently answers a different question, and a self-test that builds a
    fixture and then reads the ambient repo through it does not fail -- it
    passes, for the wrong reason, which is strictly worse than failing. That is
    what happened to `quote-names.py --selftest` on 2026-09-04: its assertions
    held against a repository it had not built.

    So: a decoy repository, pointed at by `GIT_DIR` exactly as a hook would,
    with a file at the same path and different content. Both git-touching
    routes are exercised -- `read_bytes` goes through the `cat-file --batch`
    pipe, `files_under`/`is_file` through `ls-tree` -- because cleaning one
    and not the other leaves half the seam redirected.
    """
    decoy = os.path.join(os.path.dirname(work), "decoy-" + os.path.basename(work))
    os.makedirs(decoy, exist_ok=True)
    git(decoy, "init", "--quiet", "-b", "main", ".")
    git(decoy, "config", "user.name", "Real Person")
    git(decoy, "config", "user.email", "real@example.org.uk")
    git(decoy, "config", "commit.gpgsign", "false")
    for rel, body in (("src/lib.rs", b"//! DECOY. Never the right answer.\n"),
                      ("decoy-only.txt", b"only the decoy has this\n")):
        path = os.path.join(decoy, *rel.split("/"))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as fh:
            fh.write(body)
    git(decoy, "add", "--all")
    git(decoy, "commit", "--quiet", "-m", "decoy")

    saved = {k: os.environ.get(k) for k in ("GIT_DIR", "GIT_WORK_TREE")}
    os.environ["GIT_DIR"] = os.path.join(decoy, ".git")
    os.environ["GIT_WORK_TREE"] = decoy
    try:
        # The fixture is what a hook-run self-test would be handed, and the
        # environment is what the hook would have set behind its back.
        check("the decoy is genuinely a different repository",
              _decoy_head_differs(work, decoy), True)

        with gittree.RevTree("HEAD", work) as rev:
            check("cat-file reads the repository it was handed",
                  rev.read_text("src/lib.rs"), "//! Root.\npub fn a() {}\n")
            check("...not the one GIT_DIR names",
                  "DECOY" in (rev.read_text("src/lib.rs") or ""), False)
            check("ls-tree reads it too: the decoy's file is absent",
                  rev.is_file("decoy-only.txt"), False)
            check("...and the fixture's own files are listed",
                  rev.is_file("posix/src/deep/b.rs"), True)

        # `WorkTree` never runs git, so it cannot be redirected -- asserted
        # rather than assumed, because that is the property that makes the two
        # sides of the seam agree under a hook.
        with gittree.WorkTree(work) as disk, gittree.RevTree("HEAD", work) as rev:
            check("the two sides still agree with GIT_DIR set",
                  disk.files_under("posix/src"), rev.files_under("posix/src"))
    finally:
        for key, value in saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def _decoy_head_differs(work: str, decoy: str) -> bool:
    """Guard against a fixture that would pass without the fix."""
    a = git(work, "rev-parse", "HEAD").stdout.strip()
    b = git(decoy, "rev-parse", "HEAD").stdout.strip()
    return bool(a) and bool(b) and a != b


def _raises(fn: object) -> bool:
    try:
        fn()
    except Exception:
        return True
    return False


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        work = build_repo(tmp)
        for case in (case_bytes_match_git, case_missing_does_not_desync,
                     case_read_after_close, case_list_paths,
                     case_materialise_layout, case_materialise_skips_absent,
                     case_stub_rules, case_cli_emits_lf,
                     case_cli_reports_crlf_input):
            case(work)
        for i, tcase in enumerate((case_tree_agrees, case_prune_is_by_component,
                                   case_missing_is_an_answer,
                                   case_entries_flags_directories,
                                   case_read_text_survives_bad_bytes,
                                   case_files_under_a_file,
                                   case_open_tree_chooses,
                                   case_alternate_build_dirs_are_pruned_by_shape,
                                   case_a_build_dir_prefix_is_not_walked,
                                   case_the_walk_never_descends_into_build_output,
                                   case_a_callers_own_prune_list_reaches_both_sides,
                                   case_tree_agrees_in_a_linked_worktree,
                                   case_tree_reads_the_commit_not_the_disk,
                                   case_an_ambient_git_dir_does_not_redirect_the_read)):
            tcase(build_tree_repo(tmp, f"t{i}"))

    if failures:
        print(f"\n{len(failures)} gittree test(s) failed:", file=sys.stderr)
        for name in failures:
            print(f"  - {name}", file=sys.stderr)
        return 1
    print("\nall gittree tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
