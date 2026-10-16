### A-AN-OPEN-FILE-FOLLOWS-ITS-NAME -- 2026-10-01 -- FIXED the same day for memfs and ext4 (steps 1-5 of the plan below; design-decisions §1508, §1509) (lane A)

**In short:** opening a file here gives a program a handle to the file's
*name*, not to the file. Every read and write looks the name up again. So a
file renamed or deleted while a program has it open is lost to that
program, or quietly swapped for whatever takes the name next. POSIX keeps an
open file alive and in place until its last close, and programs depend on
it every day.

**What breaks:**
- **Deleting an open file.** SQLite opens each temporary file (sorts, temp
  tables, statement journals) and deletes it at once, keeping only the
  descriptor (`SQLITE_OPEN_DELETEONCLOSE`). Here the next write fails with
  `NotFound`. CPython's `tempfile.TemporaryFile` does the same. So does
  libc's `tmpfile`, which keeps a visible name for this reason (lane B's
  tmpfile entry). `O_TMPFILE` and `SYS_FS_TMPFILE` are refused.
- **Renaming an open file.** Log rotation (`mv app.log app.log.1`), an
  editor saving by rename over the file another program has open, `mv` of
  a file being written. The writer's next write fails, or lands in a new
  file that took the old name.
- **Everything keyed by a handle's path:** record and `flock` locks fall
  back to the open-time name, and so do the identity a lock reports and
  `/proc/<pid>/fd`.

**Where:** `fs::handle::OpenFile { path }`; `read`, `write`, `read_at`,
`write_at`, `fstat` and `ftruncate` go through `Vfs::*_resolved(path)`.
Directory handles already carry an identity (`dir_pin`) and check it; file
handles do not.

**The proper fix:** a handle holds its file, the mount and the inode, not
its name.
- Each filesystem gains inode-addressed I/O.
- A file unlinked while open stays, with its blocks, until the last handle
  closes: an orphan list, as ext4 keeps one for exactly this, and a
  refcount on memfs nodes.
- Then `O_TMPFILE` and `SYS_FS_TMPFILE` become real, `tmpfile` can unlink at
  once, and every lock key is the file's identity.

It touches the VFS and each writable filesystem (memfs, ext4, FAT), so it
is a task of its own with a boot of its own. Lane A takes it after
`A-ST_DEV-IS-ZERO-FOR-EVERY-FILE`.

**Reproduce:** open a file, delete it, write through the handle. On memfs,
the root of the boot test, the write *re-creates* the file under its old
name: `write_at` creates a missing path, so the file comes back holding
zeros up to the offset, then the data. On Linux the write succeeds and the
data stays readable through the handle, and nowhere else, until the close.

**Two more faces of the same design, found 2026-10-01:**
- **A handle never sees another writer's growth.** `read`, `pread`,
  `SEEK_END`, `SEEK_DATA` and `SEEK_HOLE` use a size cached in the handle
  at open, updated only by that handle's own writes. So a reader stops at
  the size the file had when it opened it:
  - `tail -f` prints nothing new, although `fstat` shows the file growing;
  - two processes sharing a SQLite database read short pages from the one
    the other has grown.
- **An `O_APPEND` write lands at the cached end**, not at the file's real
  end. Two appenders through separate opens overwrite each other.

**The plan** (lane A, now):
1. `FileSystem` gains inode-addressed calls: read, write, truncate,
   metadata, and pin/unpin, which keep an inode alive while open. They
   default to `NotSupported`, which keeps today's path behaviour for
   filesystems without inodes (FAT, the pseudo filesystems).
2. memfs implements them. An open count on a node keeps an unlinked node
   alive until its last unpin.
3. `fs::handle` holds the object (filesystem, `fs_id`, inode) for a
   regular file on such a filesystem. Its I/O goes through it, sizes come
   from the file at each call, and the final close unpins. Unmount
   refuses while any file on it is open.
4. ext4: the same calls, plus the on-disk orphan list (`s_last_orphan`,
   chained through `i_dtime`), so a file unlinked while open survives
   until its last close and is reclaimed at the next mount after a crash.
   The superblock field exists today and nothing reads or writes it.
5. `O_TMPFILE` and `SYS_FS_TMPFILE`: create, open, unlink.

**Done, 2026-10-01** (design-decisions §1508):
- **Steps 1-3:** commit 82d5088e3. Test: `fs::handle`'s `test_held_files`.
- **Step 4, ext4:**
  - The inode-addressed calls.
  - Pins per inode.
  - The orphan list, written at an unlink of a held file and read back at
    mount. `Ext4Fs::open` frees anything a crash left there before the
    filesystem is mounted.
  - A replacing rename orphans a held target the same way.
  - Test: `ext4::self_test`'s held-file rung, on the boot test's `/mnt`.
    The free-inode count shows the inode kept while held and freed at the
    last close.
- **Step 5, `O_TMPFILE` and `SYS_FS_TMPFILE`** (design-decisions §1509):
  - A file is made with no name at the filesystem: on ext4, an inode on
    the orphan list from birth.
  - It goes at its last close, or is named first: `linkat` of a descriptor
    by `AT_EMPTY_PATH` or `/proc/self/fd/N`, and the native
    `SYS_FS_LINK_HANDLE` (1096).
  - Tests: `test_held_files` rung 14; `ext4::self_test`'s unnamed-file
    rung on `/mnt`; `dispatch`'s `test_dispatch_tmpfile`; the Linux flags
    in `test_linux_create_modes`.
- **Still open:** on FAT and the pseudo filesystems a handle still goes by
  name, so everything above applies to them as before, and `O_TMPFILE`
  there is `EOPNOTSUPP`.

**Second pass, 2026-10-01** -- what steps 1-4 left, found while building
step 5:
- **A close racing a call could free the file under it.** A call took a
  copy of the handle's `FileObject` and let go of the table; a close on
  another thread then gave the hold back at once. For a file deleted while
  open that freed the inode -- on ext4, its blocks back in the free pool --
  while the call was still reading or writing it. Now the hold is a
  `FileHold` behind an `Arc`, shared by the descriptions and by each call
  in progress, and given back by the last of them, as Linux's `fdget`
  keeps a `struct file` alive across a syscall. Test: `test_held_files`
  rung 7 (the file and its mount outlive the close while a call has it).
- **`handle_path` handed out a name the file no longer had.** Every caller
  acting by a handle's name -- the Linux `fchmod`, `fchown`, `ftruncate`,
  `fallocate`, `futimens`, `fexecve`, `fstatfs` -- acted on whatever had
  the name now. `handle_path` now checks that the name still names the held
  file (`NotFound` if not). `handle_name` is the unchecked name, for
  display: `/proc/<pid>/fd`, `/proc/<pid>/maps`, `SYS_FS_HANDLE_PATH`. Test:
  rung 8.
- **`flock` keyed on the open-time name.** A file renamed while open kept
  no lock against a handle opened under its new name, and a new file under
  its old name inherited them. The handle calls (native
  `SYS_FS_FLOCK_HANDLE`, Linux `flock`, the release at close) now key on
  `fs::handle::lock_key`: the identity of the file held. Test: rung 9.
- **Linux `fchdir` and a file descriptor as a directory.** A descriptor of
  a file is `ENOTDIR` before its name is looked at, and `fchdir`'s
  directory check no longer applies a jailed caller's jail twice
  (`stat_resolved`).
- **The Linux fd calls went by the descriptor's name.** `fchmod`,
  `fchown`, `fchownat`/`fchmodat2` with `AT_EMPTY_PATH`, `futimens`,
  `ftruncate`, `fallocate` and `fstatfs` looked the name up again, through
  the path calls. So a file deleted while open could not be truncated or
  grown through its own descriptor (SQLite's temporary files), a renamed
  one was missed, and a jailed caller's jail was applied to the name twice.
  They now act through `fs::handle::HandleFile`: the file the handle
  holds, or for a file held by name the host path captured at open. New
  inode-addressed calls back it: `FileSystem::chmod_ino`, `chown_ino`,
  `utimes_ino`, `fallocate_ino` (memfs and ext4), `Vfs::object_set_*`,
  `object_fallocate` and `object_statvfs`, and the `*_resolved` path calls.
  A change through a handle opened in a read-only volume is refused
  (`OpenFile::ro_volume`, taken at open, as Linux checks the mount a file
  was opened on). Tests: `test_held_files` rungs 10-13; `linux`'s fallocate
  range test now goes through a handle.
- **Still open:** `fexecve` of a file renamed or deleted since it was
  opened is `ENOENT`. Exec loads by name, and the name is now checked; Linux
  execs the open file. The fix is an exec that reads its image through the
  handle.
