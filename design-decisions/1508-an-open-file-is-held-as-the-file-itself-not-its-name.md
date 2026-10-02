## 1508. An open file is held as the file itself, not its name

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** when a program opened a file, the system remembered only its
name and looked that name up again for every read and write. So a file
deleted or renamed while a program had it open was lost to that program, or
silently swapped for whatever took the name next. Reads also stopped at the
size the file had at open, so a program never saw another program's
additions. Now an open file is held as the file itself, as on Linux. It
survives a rename and a delete until the last program closes it, and every
read sees the file as it is now.

**What changed** (known-issues `A-AN-OPEN-FILE-FOLLOWS-ITS-NAME`):
- **`FileSystem` gains inode-addressed calls:** pin/unpin, read, write,
  append, truncate, metadata. memfs and ext4 implement them.
- **`Vfs::open_object` returns a `FileObject`** (the mount plus the inode),
  which `fs::handle` holds for a regular file. All its I/O goes through it.
- **A file whose last name goes while held lives on,** unnamed, until its
  last close. On memfs an open count keeps the node. On ext4 the inode goes
  on the on-disk orphan list (`s_last_orphan`, chained through `i_dtime`),
  and the next mount frees anything a crash left there.
- **Sizes are the file's own at each call.** `O_APPEND` finds the end and
  writes in one hold of the filesystem lock.

**Alternatives, the main one:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Handles hold (mount, inode), filesystems pin (chosen)** | open files behave as on Linux on memfs and ext4 | small, local to the handle layer and two filesystems; everything path-based elsewhere is untouched | two ways for a handle to reach its file: FAT and the pseudo filesystems still go by name |
| B. A Linux-style inode and dentry cache in the VFS | every filesystem behind one in-memory inode | one model for all | rewrites the VFS and every filesystem; months of churn for the same behaviour on the two filesystems that matter |
| C. Keep names; follow renames, refuse unlinks of open files | a rename updates every handle's name; an unlink of an open file fails | no filesystem changes | `unlink` of an open file is POSIX-legal and common (SQLite); refusing it breaks programs |

**Smaller decisions that came with it:**

| decision | alternative | why this one |
|---|---|---|
| Access is checked at open only | re-check the path's permissions on every write, as before | POSIX: a descriptor survives a later `chmod`. Re-checking also cannot work for a file with no name |
| An unmount refuses while a file on the mount is held (`DeviceBusy`) | lazy unmount: detach now, free at the last close | Linux's default `umount` answers `EBUSY`; lazy unmount would leave I/O reaching a detached filesystem |
| `ftruncate` leaves the offset alone | clamp it to the new end, as before | POSIX says so: a later write leaves a hole |
| Per-file state (ACL, seals, flags) of an unlinked held file ends at its last release | end it at the unlink, as before | a write-sealed file must stay sealed to the handle still writing it |
| ext4 orphans go on the on-disk list | keep them in memory only | a crash with a deleted-but-open file would leak its blocks until `fsck`; the list is what ext4 has for this, and Linux reads it too |

**Consequences:**
- On FAT and the pseudo filesystems a handle still goes by name. They have
  no stable inode numbers to hold.
- `O_TMPFILE` and `SYS_FS_TMPFILE` can now be built (create, hold, unlink)
  and are not yet; they still refuse.

**Revisit** if FAT needs held files: it would need an object identity of
its own, such as the first cluster.
