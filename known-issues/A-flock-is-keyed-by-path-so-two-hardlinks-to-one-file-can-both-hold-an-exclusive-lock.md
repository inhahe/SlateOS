### [A] `flock()` is keyed by path, so two hardlinks to one file can both hold an exclusive lock -- 2026-09-21
**Status:** OPEN (found while designing byte-range record locking, which will key on `FileId` from the start)

**In short:** a program can ask the kernel to lock a file so no one else
touches it. The kernel remembers the lock under the *name* used, not the
file itself. A file can have two names, so two programs using different
names for the same file are both told they hold an exclusive lock.

**Measured, not inferred:**

| fact | where |
|---|---|
| the table is keyed by path | `vfs.rs` `flock_resolved`: `table.iter().position(|e| e.path.as_path() == path)` |
| the entry stores a path, not an id | `struct PathLockEntry { path: PathBuf, locks: Vec<FileLock> }` |
| a second name is reachable | `Vfs::link` exists, with ext4 and memfs backends |
| an inode identity exists and is unused here | `FileId` = `(fs_id, ino)`, never reused, already keying the page cache |

Linux's `flock()` is per-**inode** -- it attaches to the open file
description, which refers to the inode -- so two hardlinks conflict there
and do not here. That is a divergence, not a design choice: nothing in the
code says path-keying was intended, and `FileId` was sitting right there.

**Severity is low and the reason is worth stating**, because it is the same
reason the seal table's identical defect is low: advisory locks only bind
programs that ask. Nothing in this tree asks yet. The cost is latent -- it
becomes real the first time two cooperating processes rely on it, which is
exactly when it will be hardest to see.

**This is the third path-keyed security-ish table found this week**, after
`fs/sealing.rs` (seals keyed by `PathBuf`, no canonicalisation) and the
`immutable.rs` flag set. The shape repeats because a path is the argument
you already have and an inode id is one lookup away. Worth a rule rather
than three separate fixes: **a table that answers "may this be modified?"
must key on the thing being modified, and a name is not that thing.**

**Fix:** key on `FileId`, resolved once at lock time. `funlock_all(owner)`
and the `handle::close` release path both work unchanged -- they iterate
by owner, not by key. The new byte-range record-lock table (the ask in
`requests/b-a-advisory-record-locking-is-a-stub-that-always-succeeds.md`)
will key on `FileId` from the start, so this is the older table catching up
rather than a new convention.
