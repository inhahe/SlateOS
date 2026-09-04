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

import gitenv  # noqa: E402
import src_digest as sd  # noqa: E402

# Every git command in this file drives a throwaway repository and picks it with
# `-C`. That does not beat an inherited `GIT_DIR`, which git exports into hooks,
# `git bisect run` and `git rebase --exec` -- so under any of those this suite
# would build its fixtures inside the real repository and commit to it. On
# 2026-08-29 the equivalent bug in `check-requests-not-deleted.py --selftest`
# did exactly that and published two commits deleting the whole tree; see
# `scripts/gitenv.py`. Scrubbing once here covers every child, which is the
# form that cannot be forgotten at one call site out of the dozen below.
gitenv.scrub_environ()

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


def _worktree_witness():
    """A cheap fingerprint of everything `src_digest_worktree` reads.

    HEAD plus `git status --porcelain`: between them they move whenever a
    commit lands, a file is staged, or a tracked file is edited, which is the
    complete set of things that legitimately change the worktree digest.
    """
    try:
        head = subprocess.run(["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
                              capture_output=True, check=True).stdout
        status = subprocess.run(["git", "-C", str(REPO_ROOT), "status", "--porcelain"],
                                capture_output=True, check=True).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    return head + b"\0" + status


def test_real_worktree_digest_is_stable():
    """Two calls in a row must agree, or nothing downstream can group at all.

    Bracketed by a witness of the worktree's mutable state, because this
    assertion has a precondition it cannot otherwise state: the tree must hold
    still. It does not always. On 2026-09-04 this failed during a sweep purely
    because a `git commit` landed in another shell between the two calls, and
    the digest is *supposed* to move when that happens -- it covers uncommitted
    state, that is the entire point of the `worktree` flavour.

    What made that worth fixing is not the flake. It is that the failure was
    reported as `the worktree digest is stable across calls -- got X, want Y`,
    which names the digest as the suspect when the digest was working exactly
    as designed. A test that reports a violated precondition in the words of a
    defect sends the reader to the wrong file, and this suite exists to catch
    checks that describe something other than what they looked at.

    So: if the witness moved, the tree moved, and the honest verdict is that
    the assertion could not be made -- not that it failed. If the witness held
    still and the digests still disagree, that is a real determinism defect and
    it fails as before.
    """
    before = _worktree_witness()
    try:
        first = sd.src_digest_worktree(REPO_ROOT)
        second = sd.src_digest_worktree(REPO_ROOT)
    except (OSError, subprocess.SubprocessError, RuntimeError) as exc:
        print(f"SKIP  real worktree digest ({exc})")
        return
    after = _worktree_witness()

    verdict = stability_verdict(before, after, first, second)
    if verdict == "no-witness":
        print("SKIP  worktree stability (cannot witness the tree)")
    elif verdict == "moved":
        print("SKIP  the worktree digest is stable across calls "
              "(the tree changed under the test -- a commit or an edit landed "
              "between the two calls, so the digests are correctly different)")
    else:
        check("the worktree digest is stable across calls", first, second)

    check("the worktree digest is tagged", first.startswith("full:"), True)


def stability_verdict(before, after, first, second):
    """`"no-witness"`, `"moved"`, or `"assert"` -- which of the three to do.

    Split out as a pure function precisely because the two skip arms are
    otherwise unreachable on a quiet tree, and an arm that never executes is
    the thing this suite is about. `test_the_stability_verdict_*` below drives
    all four combinations directly; monkeypatching the module to reach them
    would test the patch as much as the rule.
    """
    if before is None or after is None:
        return "no-witness"
    # Only "moved" when the tree actually moved AND the digests disagree. A
    # moving tree whose digests happen to match is not evidence of anything
    # wrong, so it is asserted normally rather than waved through -- skipping
    # it would throw away a real observation to avoid a failure that is not
    # happening.
    if before != after and first != second:
        return "moved"
    return "assert"


def test_the_stability_verdict_distinguishes_a_moving_tree_from_a_broken_digest():
    """The rule that decides skip-vs-assert, over all four combinations.

    The one that matters is the third: a tree that held still whose digests
    disagree must still be a failure. A precondition guard that also swallows
    the defect it was guarding against is worse than no guard, because it
    turns a loud failure into a silent skip -- and it would pass every test
    that only checked the flake had stopped flaking.
    """
    cases = [
        ("a moving tree with differing digests is not a failure",
         (b"A", b"B", "full:1", "full:2"), "moved"),
        ("a moving tree whose digests still agree is asserted anyway",
         (b"A", b"B", "full:1", "full:1"), "assert"),
        ("a STILL tree with differing digests is still a real failure",
         (b"A", b"A", "full:1", "full:2"), "assert"),
        ("a still tree with agreeing digests is asserted",
         (b"A", b"A", "full:1", "full:1"), "assert"),
        ("an unwitnessable tree cannot be asserted about",
         (None, b"A", "full:1", "full:2"), "no-witness"),
        ("...in either position",
         (b"A", None, "full:1", "full:2"), "no-witness"),
    ]
    for label, args, want in cases:
        check(label, stability_verdict(*args), want)

    # The witness must be able to tell this tree apart from a changed one, or
    # the guard above is a constant `"assert"` wearing a rule's clothes.
    here = _worktree_witness()
    if here is None:
        print("SKIP  witness sensitivity (git unavailable)")
        return
    check("the witness reads something", bool(here), True)
    check("the witness carries HEAD and the porcelain status",
          here.count(b"\0") >= 1, True)


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
