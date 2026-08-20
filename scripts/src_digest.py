"""Identity of the *source that was built*, for grouping benchmark arms.

# The problem this replaces

`bench-history.py`'s `layout_arms()` used to group a layout sweep's arms by
their recorded `commit`. That is a proxy for "same source", and it is a broken
one in both directions:

* **It splits arms that are identical.** A sweep takes ~75 minutes per arm. Any
  commit made while it runs -- including a documentation commit that cannot
  change a build -- lands a later arm on a different `commit` string. With
  `MIN_PADS_FOR_LAYOUT_BAND = 3`, six arms spread over six commits become six
  one-pad groups and produce *no band at all*, silently, after hours of QEMU.
  This has now happened to a real sweep: the six WHPX arms of 2026-08-19 were
  recorded under six different commits, every one of them a docs commit.
* **It merges arms that are not identical.** `dirty` is computed with
  `git diff --quiet HEAD`, which cannot see untracked files. The kernel
  `include_bytes!`s six ring-3 service binaries and every boot attaches
  `rootfs.ext4`; all seven are build artifacts and all seven are gitignored by
  design. Rebuild a service between two arms and both rows still say
  `dirty: false`, both still say the same `commit`, and two genuinely different
  kernels band together.

The second direction is the dangerous one. A band that is too *wide* dismisses
every regression inside it, silently, which is the direction that hides faults.
A band that is too *narrow* -- or absent -- merely leaves a movement ungraded,
and an ungraded movement is still treated as a regression. Every judgement call
in this module is resolved that way: when in doubt, split.

# What the digest covers

Two halves, because the build has two halves:

1. **Tracked build inputs** -- everything in the tree except an explicit list of
   documents that cannot change a build (see `EXCLUDED_*`). Identified by git
   blob hash, so this half is recoverable from a commit alone.
2. **Embedded and attached artifacts** -- the untracked binaries the kernel
   embeds, plus `rootfs.ext4`. Identified by SHA-256 of their bytes. This half
   is *not* recoverable from a commit, because the files are not in git.

The artifact list is **derived, never written down**: it is grepped out of
`kernel/src` exactly the way `scripts/bootstrap-worktree.sh` derives it (see
`embedded_artifact_paths` there). A service added to the kernel is covered
without anyone remembering to update a list here -- which is the property that
derivation was written for in the first place.

# Why there are two digest flavours, and why they must never compare equal

`src_digest_worktree()` measures a live tree and can see both halves. It
returns a digest tagged `full:`.

`src_digest_commit()` reconstructs the identity of a row recorded before this
field existed, from its `commit` alone. Half 2 is simply not available -- those
bytes were never stored anywhere. It returns a digest tagged `tracked:`.

The tags are load-bearing. A `tracked:` digest asserts strictly less than a
`full:` one, so allowing them to compare equal would let a row whose artifacts
are unknown band with a row whose artifacts are pinned -- re-admitting exactly
the merge-two-different-kernels failure described above, through the back door,
after the exclusion list was carefully written to keep it out. Tagging makes
that structurally impossible rather than a thing to remember.

Grouping historical rows on `tracked:` is not a weakening of anything: it
asserts precisely what `commit` + `dirty` already asserted about those rows.
It only stops them being split by commits that changed no build input.
"""

from __future__ import annotations

import hashlib
import os
import re
import subprocess

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

#: How many hex characters of the SHA-256 to keep. Sixteen is 64 bits; these
#: values are compared for equality within one history file of a few hundred
#: rows, never used as a security boundary, and a full 64-char hash makes the
#: JSONL rows unreadable for no gain.
DIGEST_CHARS = 16

# --------------------------------------------------------------------------
# What is not a build input
# --------------------------------------------------------------------------
#
# Explicit top-level paths only. A recursive rule such as "*.txt" would be much
# shorter and is exactly wrong: it would drop `kernel/src/fs/declared.txt`,
# `kernel/src/fs/existing_files.txt` and `kernel/ada/prebuilt/stamp.txt`, all of
# which really do change what gets built -- merging two genuinely different
# kernels into one band, which is the failure direction that hides faults.
#
# The rule is therefore kept shallow on purpose: nothing under `kernel/`,
# `toolchain/`, `services/`, `.cargo/` or `Cargo.{toml,lock}` can ever be
# dropped by a filename pattern, no matter what it is called. This was audited
# against the real tree rather than reasoned about -- see
# `check_exclusions_are_shallow()` below, which is a test, not a comment.

EXCLUDED_SUFFIXES = (".md",)

EXCLUDED_EXACT = frozenset({
    "api.txt", "claude_use.txt", "convo1.txt", "design desicions.txt",
    "design-review.txt", "design.txt", "differences from windows.txt",
    "dual_use.txt", "effort_level.txt", "ipc.txt", "todo.txt",
    "memory management.txt", "scheduler.txt", "other design decisions.txt",
    # The harness's own output. Both are tracked and both are appended to at
    # the *end* of a run, so a clean tree stops being clean the moment the
    # first run finishes. `boot-test.sh` already excludes these two from its
    # `dirty` check for the same reason; the same argument applies here and
    # for the same reason it is safe -- neither file is ever compiled.
    "bench/history.jsonl", "bench/boot-history.jsonl",
})

EXCLUDED_PREFIXES = ("requests/",)

#: Trees that must never lose a path to a filename pattern. Asserted, not
#: assumed -- see `check_exclusions_are_shallow`.
BUILD_TREES = ("kernel/", "toolchain/", "services/", ".cargo/", "net/",
               "posix/", "userspace/", "gui/", "apps/", "drivers/", "fs/")


def is_excluded(path: str) -> bool:
    """True if `path` cannot change what the build produces."""
    if path in EXCLUDED_EXACT:
        return True
    if path.startswith(EXCLUDED_PREFIXES):
        return True
    # Depth-0 only. A README inside a build tree is still not a build input in
    # practice, but keeping the rule shallow means no pattern can ever reach
    # into a source tree -- which is worth far more than the handful of nested
    # documents it declines to exclude. Over-inclusion only splits arms.
    if "/" not in path and path.endswith(EXCLUDED_SUFFIXES):
        return True
    return False


def check_exclusions_are_shallow(paths):
    """Paths under a build tree that the exclusion rules would drop.

    Returns a sorted list, which must be empty. This is the assertion the
    `kernel/src/fs/declared.txt` trap made necessary: the exclusion list is
    only safe as long as it cannot reach inside a tree that is compiled, and
    "cannot" is a claim about the *actual* tree, not about the regex.
    """
    return sorted(p for p in paths
                  if p.startswith(BUILD_TREES) and is_excluded(p))


# --------------------------------------------------------------------------
# git plumbing
# --------------------------------------------------------------------------


def _git(root: str, *args: str) -> bytes:
    return subprocess.run(["git", "-C", root, *args],
                          capture_output=True, check=True).stdout


def tracked_entries_from_commit(root: str, treeish: str):
    """`[(path, blob_sha), ...]` of build-relevant files at `treeish`.

    Raises `subprocess.CalledProcessError` if the commit cannot be resolved --
    which is a real possibility for an old row (a branch deleted, a worktree
    from another machine), and must be handled by the caller rather than
    papered over. Silently returning an empty list would give every
    unresolvable commit the *same* digest, merging unrelated sweeps.
    """
    out = _git(root, "ls-tree", "-r", "-z", treeish)
    entries = []
    for entry in out.split(b"\0"):
        if not entry:
            continue
        meta, _, raw_path = entry.partition(b"\t")
        path = raw_path.decode("utf-8", "surrogateescape")
        if is_excluded(path):
            continue
        entries.append((path, meta.split()[2].decode("ascii")))
    return entries


def tracked_entries_from_worktree(root: str):
    """`[(path, blob_sha), ...]` for the tree as it exists on disk *now*.

    Built as `ls-tree HEAD` overlaid with a rehash of only those paths that
    actually differ from HEAD. The obvious alternative -- a scratch-index
    `git add -A && git write-tree` -- was measured at 91 seconds on this repo
    and, worse, renormalises CRLF, so it does not reproduce HEAD's own tree on
    a clean checkout. The overlay is exact instead: `git hash-object` on a
    worktree file applies the same attribute filters as checkin, which was
    verified against known-CRLF files by comparing it to `ls-tree` output.
    """
    head = dict(tracked_entries_from_commit(root, "HEAD"))

    # Tracked paths that differ from HEAD, staged or not. `-z` because paths
    # here may contain anything except NUL; this project's own rule is that
    # OS-boundary data is bytes, and a filename with a newline must not be
    # able to desynchronise the parse.
    out = _git(root, "diff", "--name-status", "-z", "HEAD", "--")
    fields = out.split(b"\0")
    changed = []
    i = 0
    while i < len(fields):
        status = fields[i]
        if not status:
            break
        # Renames and copies carry two paths (R100 <old> <new>).
        if status[:1] in (b"R", b"C"):
            if i + 2 >= len(fields):
                break
            changed.append((b"R", fields[i + 1], fields[i + 2]))
            i += 3
        else:
            if i + 1 >= len(fields):
                break
            changed.append((status[:1], None, fields[i + 1]))
            i += 2

    rehash = []
    for status, old_raw, raw in changed:
        path = raw.decode("utf-8", "surrogateescape")
        if status == b"D":
            head.pop(path, None)
            continue
        if old_raw is not None:
            head.pop(old_raw.decode("utf-8", "surrogateescape"), None)
        if is_excluded(path):
            head.pop(path, None)
            continue
        rehash.append(path)

    if rehash:
        # One process for all of them; `--stdin-paths` reads NUL-free lines, so
        # fall back to one call per path if any name would be ambiguous.
        if any("\n" in p for p in rehash):
            for path in rehash:
                head[path] = _git(root, "hash-object", "--",
                                  path).decode("ascii").strip()
        else:
            payload = ("\n".join(rehash) + "\n").encode("utf-8",
                                                        "surrogateescape")
            proc = subprocess.run(
                ["git", "-C", root, "hash-object", "--stdin-paths"],
                input=payload, capture_output=True, check=True)
            shas = proc.stdout.decode("ascii").split()
            if len(shas) != len(rehash):
                raise RuntimeError(
                    f"git hash-object returned {len(shas)} hashes for "
                    f"{len(rehash)} paths; refusing to guess the pairing")
            for path, sha in zip(rehash, shas):
                head[path] = sha

    return sorted(head.items())


# --------------------------------------------------------------------------
# The untracked half
# --------------------------------------------------------------------------

#: Matches the artifact path *inside* an `include_bytes!` macro, yielding the
#: target triple and binary name as well as the service directory. Nothing
#: about the layout is assumed beyond what the kernel literally writes, so a
#: service that one day builds for a different triple needs no change here.
#: Deliberately identical in shape to the grep in `bootstrap-worktree.sh`.
_EMBED_RE = re.compile(
    r"services/[A-Za-z0-9_.-]+/target/[A-Za-z0-9_.-]+/release/[A-Za-z0-9_.-]+")

#: Attached at every boot as a second virtio-blk disk. Not embedded in the
#: kernel, but what it contains changes what the suite measures, so it belongs
#: to the identity of the measurement exactly as much as the services do.
EXTRA_ARTIFACTS = ("rootfs.ext4",)


def embedded_artifact_paths(root: str):
    """Repo-relative paths of every artifact the kernel embeds or attaches.

    Derived by scanning `kernel/src` for `include_bytes!`, never from a list.
    A written-down list is the shape of check that reports everything present
    on the day someone adds a service and forgets to update it.
    """
    found = set()
    kernel_src = os.path.join(root, "kernel", "src")
    for dirpath, _dirnames, filenames in os.walk(kernel_src):
        for name in filenames:
            if not name.endswith(".rs"):
                continue
            full = os.path.join(dirpath, name)
            try:
                with open(full, "r", encoding="utf-8", errors="replace") as fh:
                    text = fh.read()
            except OSError:
                continue
            if "include_bytes!" not in text:
                continue
            for line in text.splitlines():
                if "include_bytes!" in line:
                    found.update(_EMBED_RE.findall(line))
    return sorted(found) + list(EXTRA_ARTIFACTS)


def _hash_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def artifact_entries(root: str):
    """`[(path, sha256_or_ABSENT), ...]` for the embedded/attached artifacts.

    A missing artifact is recorded as the literal `"absent"` rather than
    skipped. Skipping would make "the file is not there" and "the file is there
    but empty" -- and, worse, "a service was removed" and "a service was never
    scanned" -- produce the same digest.
    """
    entries = []
    for rel in embedded_artifact_paths(root):
        full = os.path.join(root, rel.replace("/", os.sep))
        entries.append((rel, _hash_file(full) if os.path.isfile(full)
                        else "absent"))
    return entries


# --------------------------------------------------------------------------
# The digest itself
# --------------------------------------------------------------------------


def _digest(tracked, artifacts) -> str:
    h = hashlib.sha256()
    # Path *and* content for both halves: a file moved without being edited
    # changes the build (module paths, `include_bytes!` targets), so content
    # alone is not an identity.
    for label, entries in (("tracked", tracked), ("artifact", artifacts)):
        h.update(label.encode("ascii"))
        h.update(b"\0")
        for path, ident in entries:
            h.update(path.encode("utf-8", "surrogateescape"))
            h.update(b"\0")
            h.update(ident.encode("ascii"))
            h.update(b"\0")
    return h.hexdigest()[:DIGEST_CHARS]


def src_digest_worktree(root: str = REPO_ROOT) -> str:
    """Full source identity of the tree on disk, tagged `full:`.

    This is what a run should record about itself. It covers the untracked
    artifacts, so unlike `commit` + `dirty` it actually means "the kernel this
    produced is reproducible from this identity".
    """
    tracked = tracked_entries_from_worktree(root)
    return "full:" + _digest(tracked, artifact_entries(root))


def src_digest_commit(root: str, treeish: str) -> str:
    """Source identity reconstructed from a commit alone, tagged `tracked:`.

    For rows recorded before `src_digest` existed. The artifact half is
    unrecoverable -- those bytes were never stored -- so it is hashed as an
    explicit `unknown` marker rather than as an empty list, which would make it
    indistinguishable from a genuine tree that embeds nothing.
    """
    tracked = tracked_entries_from_commit(root, treeish)
    return "tracked:" + _digest(tracked, [("<artifacts>", "unknown")])


def main(argv=None) -> int:
    import argparse
    import sys

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=REPO_ROOT)
    parser.add_argument("--commit", help="derive from this treeish instead of "
                                         "measuring the working tree")
    args = parser.parse_args(argv)

    try:
        if args.commit:
            print(src_digest_commit(args.root, args.commit))
        else:
            print(src_digest_worktree(args.root))
    except (OSError, subprocess.SubprocessError, RuntimeError) as exc:
        # The caller (boot-test.sh) treats an empty result as "do not record a
        # digest", which is correct: an absent field is unknown, and unknown
        # splits. Printing a wrong digest would merge.
        print(f"src_digest: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
