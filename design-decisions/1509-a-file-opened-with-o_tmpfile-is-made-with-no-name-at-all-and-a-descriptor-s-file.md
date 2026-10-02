## 1509. A file opened with `O_TMPFILE` is made with no name at all, and a descriptor's file can be given one without privilege

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can ask for a scratch file that has no name, so
that no other program can open it and nothing is left behind if it
crashes. That is Linux's `O_TMPFILE`, and our native `SYS_FS_TMPFILE`.
Here the file is now made with no name from the start, and it goes when
its last descriptor closes. A program that wants to keep the file can give
it a name once it is fully written. Linux programs do that with
`linkat(fd, "", ..., AT_EMPTY_PATH)` or through `/proc/self/fd/N`; native
programs use `SYS_FS_LINK_HANDLE`. A reader then never sees a half-written
file under that name.

**What changed** (known-issues `A-AN-OPEN-FILE-FOLLOWS-ITS-NAME`, step 5):
- `FileSystem::create_unnamed(dir, mode)` makes a regular file with no
  name, held once, and `link_held_ino(ino, path)` gives a held file a name.
  - memfs: a node with no links and one open.
  - ext4: an inode with link count 0, on the orphan list from birth, as
    Linux's `ext4_tmpfile` makes one. A crash leaves it to the next mount.
- `Vfs::create_unnamed_object` and `Vfs::link_object`; `fs::handle`'s
  `open_tmpfile`, `open_tmpfile_beneath` and `link_handle`.
- Linux: `O_TMPFILE` in `open`, `openat` and `openat2`. `linkat` of a
  descriptor works by `AT_EMPTY_PATH`, and by `/proc/self/fd/N` followed.
- Native: `SYS_FS_TMPFILE` (648) is real; `SYS_FS_LINK_HANDLE` is new
  (1096).

**Alternatives, the main one:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Made with no name, at the filesystem (chosen)** | the file never has a name | what `O_TMPFILE` promises: no one can open it, nothing left by a crash, no events about it | a filesystem call each in memfs and ext4 |
| B. Create under a hidden name, hold it, unlink it | the file has a guessable name for an instant | no filesystem changes | during that instant, another program can list and open the "private" file; a crash between leaves it; watchers see it made and removed |

**Smaller decisions that came with it:**

| decision | alternative | why this one |
|---|---|---|
| `AT_EMPTY_PATH` links a descriptor's file for any caller | require a privilege, as Linux does for `AT_EMPTY_PATH` | Linux lets anyone do the same through `/proc/self/fd/N`, which gnulib and systemd use. We emulate that path, so a privilege on the other would protect nothing |
| `/proc/self/fd/N` with `AT_SYMLINK_FOLLOW` is the descriptor's file, recognised in `linkat` | real magic links in the path walk | `linkat` is the one call programs use it with for this; magic links in the walk are a larger change of their own |
| A file deleted while open, or made with `O_EXCL`, cannot be given a name (`ENOENT`) | allow it | Linux's `I_LINKABLE` rule: a deleted file stays deleted |
| The native `SYS_FS_TMPFILE` makes mode 0600 | a mode argument | the call has none; a file no one else can name has no one else to share with |
| The unnamed file is shown as `dir/#ino` | no name at all | what Linux shows in `/proc/<pid>/fd`; `handle_path` refuses it, so nothing acts on that name |

**Consequences:**
- On FAT and the pseudo filesystems `O_TMPFILE` answers `EOPNOTSUPP`, as
  Linux does where a filesystem has no `->tmpfile`. glibc's `tmpfile`
  falls back to a named file there.
- Lane D's `tmpfile` can use `O_TMPFILE`, or unlink at once: both work on
  memfs and ext4.

**Revisit** if the path walk gains magic links: `/proc/self/fd/N` would
then resolve to the file everywhere, not only in `linkat`.
