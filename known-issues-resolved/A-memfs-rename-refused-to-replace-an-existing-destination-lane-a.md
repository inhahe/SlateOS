## memfs `rename` refused to replace an existing destination (lane A)

**Status:** FIXED 2026-08-22 (`kernel/src/fs/memfs.rs`). Two further bugs found
in the same function are fixed with it; the ext4 equivalents of both are still
**OPEN** — see the next entry.

**In short:** Renaming a file onto a name that already exists is supposed to
replace it — that is how every editor, config writer and database saves a file
safely: write a temporary file, then rename it over the real one, so a crash
leaves either the whole old file or the whole new one and never half of each.
The in-memory filesystem refused to do it, failing instead with "already
exists". `/tmp` is always that filesystem, and in the automated boot test so is
`/`, which means the safe-save pattern simply did not work there.

### Evidence

Surfaced the first time `fs::vfs::self_test()` ran in CI (it had been behind the
`fat_ok` gate — see the entry above):

```
[vfs]   --- atomic write ---
WARNING: VFS self-test failed: AlreadyExists
```

`Vfs::atomic_write` (`kernel/src/fs/vfs.rs`) writes `.tmp_atomic_<n>` beside the
target, syncs, then `rename`s it over the target. Step 3 could never succeed
against an existing file on memfs.

### Root cause

`MemFs::rename` checked `to_children.contains_key(to_name)` and returned
`AlreadyExists` — that is `RENAME_NOREPLACE` semantics baked into the plain
operation. Both other filesystems already got this right: ext4
(`fs/ext4/vfs_impl.rs:529`, *"POSIX rename semantics: if the destination already
exists and is a regular file, replace it"*) and FAT (`fs/fat.rs:3802`, the same
comment) unlink the destination first. memfs was the sole outlier.

Note that no caller lost the no-replace behaviour: the VFS implements that flag
itself. `Vfs::rename_inner` stats the destination under the *same* per-mount lock
it then renames under, which is both where `Vfs::rename_noreplace` gets its
`AlreadyExists` and why that check is TOCTOU-free. A filesystem hardcoding the
refusal only removed the caller's ability to ask for the other behaviour.

### Two more bugs in the same function, fixed alongside

Both were latent because the `AlreadyExists` check happened to reject the first
one before it could do damage, and nothing exercised the second:

1. **Renaming an entry onto itself destroyed it.** `rename(x, x)` should be a
   no-op success (POSIX). The function detaches the source and re-inserts it at
   the destination — which, once replacement is allowed, means removing `x` and
   then inserting it back under the same key. Correct only by luck of ordering;
   now short-circuited explicitly before anything is detached.
2. **Moving a directory into its own subtree destroyed the subtree.** `/a` ->
   `/a/b/c` detached `/a` from its parent first, then walked to `/a/b` to insert
   it — a path that had just gone with it — so the walk failed `NotFound` and the
   owned node was dropped on the floor, taking the entire subtree with it. Now
   rejected with `InvalidArgument` (POSIX EINVAL) before the detach.

### Regression test

`fs::vfs::self_test()` gained a *"rename (replace semantics)"* section directly
ahead of the atomic-write one, covering all four behaviours: replace an existing
destination, self-rename is a no-op that keeps the content, a directory
destination is refused with `IsADirectory`, and a rejected into-own-subtree move
leaves the subtree intact.
