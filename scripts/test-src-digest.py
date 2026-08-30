#!/usr/bin/env python3
"""Regression tests for `scripts/src_digest.py`.

Run: `python scripts/test-src-digest.py` (exit 0 = pass, 1 = fail).
No pytest dependency, matching `scripts/test-bench-history.py`: this has to run
from a bare checkout.

Why this file exists
--------------------
`src_digest` decides which benchmark arms are allowed to be compared with one
another. Both ways of getting it wrong are silent, and only one of them is
safe:

* **Too sensitive** -- arms that really are identical get different digests, so
  they split, so no band forms, so a movement stays ungraded and is therefore
  still treated as a regression. Wasteful, but it cannot hide a fault.
* **Too insensitive** -- arms built from *different* source share a digest, so
  they band together, so the band is inflated by a real code difference, so
  every regression inside that width is dismissed as placement noise. This one
  hides faults, and it does it while printing a reassuring number.

So the tests that matter most here are the ones that pin *sensitivity*: a
kernel edit must change the digest, a moved file must change the digest, a
changed service binary must change the digest. A test suite that only checked
"the six real arms group together" would pass just as happily if `src_digest`
returned a constant -- which is the single worst thing it could do, and the
exact shape of failure this project keeps rediscovering ("a check that cannot
fire is indistinguishable from a check that passes").

Most tests build a throwaway git repository rather than leaning on real
commits from this one. Real commits get garbage-collected, rewritten, and
eventually cease to exist; a test that depends on `d937ea7bd` still being
resolvable is a test that will one day fail for a reason that has nothing to do
with the code under test. The two tests that *do* use the real tree are the two
whose entire point is a claim about the real tree.
"""

from __future__ import annotations

import inspect
import os
import shutil
import subprocess
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO_ROOT, "scripts"))

import src_digest as sd  # noqa: E402

_FAILURES = []


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got:  {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def check_ne(label, got, other):
    if got != other:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        both sides are {got!r}, but they had to differ")
    _FAILURES.append(label)
    return False


# ---------------------------------------------------------------------------
# A throwaway repository
# ---------------------------------------------------------------------------


class Repo:
    """A minimal git repo with the directory shapes `src_digest` cares about."""

    def __init__(self):
        self.root = tempfile.mkdtemp(prefix="srcdigest-test-")
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Test")
        self.git("config", "commit.gpgsign", "false")

    def git(self, *args):
        return subprocess.run(["git", "-C", self.root, *args],
                              capture_output=True, check=True,
                              text=True).stdout

    def write(self, rel, text):
        full = os.path.join(self.root, rel.replace("/", os.sep))
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(text)

    def write_bytes(self, rel, data):
        full = os.path.join(self.root, rel.replace("/", os.sep))
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "wb") as fh:
            fh.write(data)

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message, "--no-gpg-sign")
        return self.git("rev-parse", "--short", "HEAD").strip()

    def close(self):
        shutil.rmtree(self.root, ignore_errors=True)


def base_repo():
    """A repo with a kernel, a doc, an embedded service and a gitignore."""
    repo = Repo()
    repo.write(".gitignore", "**/target/\n*.ext4\n")
    repo.write("kernel/src/main.rs",
               'static INIT: &[u8] = include_bytes!('
               '"../../services/init/target/x86_64-unknown-none/release/init");'
               "\nfn main() {}\n")
    repo.write("kernel/src/fs/declared.txt", "a declared file\n")
    repo.write("design.txt", "the spec\n")
    repo.write("notes.md", "a document\n")
    repo.write("requests/a-b-thing.md", "a request\n")
    repo.write_bytes(
        "services/init/target/x86_64-unknown-none/release/init", b"INITv1")
    repo.commit("base")
    return repo


# ---------------------------------------------------------------------------
# Sensitivity -- the tests that stop the digest becoming a constant
# ---------------------------------------------------------------------------


def test_kernel_edit_changes_the_digest():
    repo = base_repo()
    try:
        before = sd.src_digest_commit(repo.root, "HEAD")
        repo.write("kernel/src/main.rs", "fn main() { /* changed */ }\n")
        repo.commit("edit the kernel")
        after = sd.src_digest_commit(repo.root, "HEAD")
        check_ne("a kernel edit changes the digest", before, after)
    finally:
        repo.close()


def test_declared_txt_is_a_build_input():
    """The trap that ruled out a recursive `*.txt` exclusion.

    `kernel/src/fs/declared.txt` really does change what gets built. If a
    future edit "simplifies" the exclusion list into a suffix glob, this is the
    test that fails, and it fails in the direction that would otherwise merge
    two different kernels into one band.
    """
    repo = base_repo()
    try:
        before = sd.src_digest_commit(repo.root, "HEAD")
        repo.write("kernel/src/fs/declared.txt", "a declared file\nand more\n")
        repo.commit("edit a .txt inside the kernel")
        after = sd.src_digest_commit(repo.root, "HEAD")
        check_ne("kernel/src/fs/declared.txt is a build input", before, after)
    finally:
        repo.close()


def test_a_move_without_an_edit_changes_the_digest():
    """Content alone is not an identity: module paths depend on filenames."""
    repo = base_repo()
    try:
        before = sd.src_digest_commit(repo.root, "HEAD")
        repo.git("mv", "kernel/src/fs/declared.txt",
                 "kernel/src/fs/declared2.txt")
        repo.commit("move a file, unchanged")
        after = sd.src_digest_commit(repo.root, "HEAD")
        check_ne("a moved-but-unedited file changes the digest", before, after)
    finally:
        repo.close()


def test_changed_service_binary_changes_the_digest():
    """The hole a git-only digest would leave open.

    The service binary is gitignored, so `commit` is identical across this
    edit and `git diff --quiet HEAD` reports the tree clean. Only the artifact
    half of the digest can see it.
    """
    repo = base_repo()
    try:
        before = sd.src_digest_worktree(repo.root)
        repo.write_bytes(
            "services/init/target/x86_64-unknown-none/release/init", b"INITv2")
        after = sd.src_digest_worktree(repo.root)
        # The premise: git genuinely cannot see this change.
        check("git still reports the tree clean",
              repo.git("status", "--porcelain").strip(), "")
        check_ne("a rebuilt service binary changes the digest", before, after)
    finally:
        repo.close()


def test_absent_artifact_differs_from_empty_artifact():
    """"Not there" and "there but empty" must not be the same identity."""
    repo = base_repo()
    artifact = "services/init/target/x86_64-unknown-none/release/init"
    try:
        repo.write_bytes(artifact, b"")
        empty = sd.src_digest_worktree(repo.root)
        os.remove(os.path.join(repo.root, artifact.replace("/", os.sep)))
        absent = sd.src_digest_worktree(repo.root)
        check_ne("an absent artifact differs from an empty one", empty, absent)
    finally:
        repo.close()


# ---------------------------------------------------------------------------
# Insensitivity -- the reason the change was made at all
# ---------------------------------------------------------------------------


def test_doc_only_commit_does_not_change_the_digest():
    """The whole point: a docs commit made mid-sweep must not split the arms."""
    repo = base_repo()
    try:
        before = sd.src_digest_commit(repo.root, "HEAD")
        repo.write("notes.md", "a document, revised\n")
        repo.write("design.txt", "the spec, revised\n")
        repo.write("requests/a-b-thing.md", "a request, revised\n")
        repo.commit("documentation only")
        after = sd.src_digest_commit(repo.root, "HEAD")
        check("a doc-only commit leaves the digest alone", before, after)
    finally:
        repo.close()


def test_nested_markdown_is_still_a_build_input():
    """The exclusion is depth-0 only, so a nested .md cannot be dropped."""
    check("a nested README is not excluded",
          sd.is_excluded("kernel/src/README.md"), False)
    check("a top-level document is excluded", sd.is_excluded("notes.md"), True)
    check("a nested .txt is not excluded",
          sd.is_excluded("kernel/src/fs/declared.txt"), False)
    check("a request is excluded", sd.is_excluded("requests/a-b-c.md"), True)


def test_worktree_ignores_edits_to_excluded_files():
    """The harness appends to bench/*.jsonl at the end of every run.

    If those writes moved the digest, every arm after the first would get its
    own -- reproducing, in a new field, exactly the split that `dirty` used to
    cause and that this whole change exists to remove.
    """
    repo = base_repo()
    try:
        repo.write("bench/history.jsonl", '{"run": 1}\n')
        repo.commit("add a history file")
        before = sd.src_digest_worktree(repo.root)
        repo.write("bench/history.jsonl", '{"run": 1}\n{"run": 2}\n')
        after = sd.src_digest_worktree(repo.root)
        check("appending to bench/history.jsonl leaves the digest alone",
              before, after)
    finally:
        repo.close()


# ---------------------------------------------------------------------------
# The worktree overlay
# ---------------------------------------------------------------------------


def test_worktree_tracks_uncommitted_edits_and_returns():
    repo = base_repo()
    try:
        clean = sd.src_digest_worktree(repo.root)
        repo.write("kernel/src/main.rs", "fn main() { /* uncommitted */ }\n")
        dirty = sd.src_digest_worktree(repo.root)
        check_ne("an uncommitted kernel edit changes the digest", clean, dirty)
        repo.git("checkout", "--", "kernel/src/main.rs")
        check("reverting the edit restores the digest",
              sd.src_digest_worktree(repo.root), clean)
    finally:
        repo.close()


def test_worktree_handles_deletions():
    repo = base_repo()
    try:
        before = sd.src_digest_worktree(repo.root)
        os.remove(os.path.join(repo.root, "kernel", "src", "fs",
                               "declared.txt"))
        check_ne("deleting a tracked build input changes the digest",
                 before, sd.src_digest_worktree(repo.root))
    finally:
        repo.close()


def test_clean_worktree_matches_its_own_commit_on_the_tracked_half():
    """The two code paths must agree, or old and new rows measure differently.

    They are deliberately *tagged* apart at the top level (see the next test),
    but the tracked half underneath has to be computed identically -- otherwise
    the overlay is not a reconstruction of the commit at all, and the tags
    would be hiding a real disagreement rather than an honest gap.
    """
    repo = base_repo()
    try:
        worktree = sd._digest(sd.tracked_entries_from_worktree(repo.root), [])
        commit = sd._digest(sd.tracked_entries_from_commit(repo.root, "HEAD"),
                            [])
        check("clean worktree reproduces its commit's tracked half",
              worktree, commit)
    finally:
        repo.close()


def test_the_two_flavours_never_compare_equal():
    """A `tracked:` digest asserts strictly less than a `full:` one.

    Letting them compare equal would band a row whose artifacts are unknown
    against a row whose artifacts are pinned.
    """
    repo = base_repo()
    try:
        full = sd.src_digest_worktree(repo.root)
        tracked = sd.src_digest_commit(repo.root, "HEAD")
        check("the full digest is tagged", full.startswith("full:"), True)
        check("the derived digest is tagged",
              tracked.startswith("tracked:"), True)
        check_ne("the two flavours never compare equal", full, tracked)
    finally:
        repo.close()


def test_unresolvable_commit_raises():
    """It must not degrade to a constant.

    Returning a fixed digest for every unresolvable commit would give them all
    the same group key, banding unrelated sweeps together -- the unsafe
    direction. The caller is expected to catch this and fall back to a key that
    cannot merge; see `layout_arms`.
    """
    repo = base_repo()
    try:
        try:
            sd.src_digest_commit(repo.root, "0000000000000000000000000000000000000000")
        except subprocess.CalledProcessError:
            check("an unresolvable commit raises rather than degrading",
                  True, True)
        else:
            check("an unresolvable commit raises rather than degrading",
                  False, True)
    finally:
        repo.close()


# ---------------------------------------------------------------------------
# The artifact list is derived, not written down
# ---------------------------------------------------------------------------


def test_artifact_list_is_derived_from_include_bytes():
    repo = base_repo()
    try:
        found = sd.embedded_artifact_paths(repo.root)
        check("the embedded service is discovered",
              "services/init/target/x86_64-unknown-none/release/init" in found,
              True)
        # A service added to the kernel is covered without editing this module.
        repo.write("kernel/src/services.rs",
                   'static T: &[u8] = include_bytes!('
                   '"../../services/newsvc/target/x86_64-unknown-none/'
                   'release/newsvc");\n')
        found = sd.embedded_artifact_paths(repo.root)
        check("a newly embedded service is discovered without a list edit",
              "services/newsvc/target/x86_64-unknown-none/release/newsvc"
              in found, True)
        check("rootfs.ext4 is always in scope", "rootfs.ext4" in found, True)
    finally:
        repo.close()


# ---------------------------------------------------------------------------
# Claims about the real tree
# ---------------------------------------------------------------------------


def test_exclusions_cannot_reach_into_a_build_tree():
    """Audited against the actual tree, not reasoned about from the regex.

    This is the assertion the `declared.txt` trap made necessary, and it has to
    be checked against real paths because it is a claim about what filenames
    actually exist -- a rule that is safe today becomes unsafe the moment
    someone adds `kernel/foo.md`.
    """
    try:
        out = subprocess.run(["git", "-C", REPO_ROOT, "ls-files"],
                             capture_output=True, text=True, check=True).stdout
    except (OSError, subprocess.SubprocessError):
        print("SKIP  exclusions audit (git unavailable)")
        return
    paths = [p for p in out.split("\n") if p]
    check("the real tree has paths to audit", len(paths) > 1000, True)
    check("no build-tree path is excluded",
          sd.check_exclusions_are_shallow(paths), [])
    excluded = sum(1 for p in paths if sd.is_excluded(p))
    check("the exclusion list excludes something (it is not a no-op)",
          excluded > 0, True)


def test_real_worktree_digest_is_stable():
    """Two calls in a row must agree, or nothing downstream can group at all."""
    try:
        first = sd.src_digest_worktree(REPO_ROOT)
    except (OSError, subprocess.SubprocessError, RuntimeError) as exc:
        print(f"SKIP  real worktree digest ({exc})")
        return
    check("the worktree digest is stable across calls",
          first, sd.src_digest_worktree(REPO_ROOT))
    check("the worktree digest is tagged", first.startswith("full:"), True)


def main():
    """Auto-discover `test_*`, same contract as the other suites.

    The floor assertion is not ceremony: a suite that discovers zero tests
    prints nothing and exits 0, which is indistinguishable from a suite that
    passed -- the exact failure shape this file is about.
    """
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 16:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 16. Discovery is broken, not the code.")
        return 1
    for _, fn in sorted(tests):
        params = inspect.signature(fn).parameters
        fn(**{p: {} [p] for p in params})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} src_digest test groups passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
