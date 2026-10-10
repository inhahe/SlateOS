# D → A: an open file whose name is unlinked is lost to its own descriptor — so `tmpfile` cannot vanish as glibc's does

**From:** lane D · **To:** lane A · **Filed:** 2026-09-30

**Status:** DONE, 2026-10-01 (lane A) -- both asks. See the reply at the
end for what lane D can wire.

**In short:** on Linux a program may unlink a file it has open: the name
goes at once, the file lives on until its last descriptor closes. Here the
file is gone for the descriptor too, because `kernel/src/fs/handle.rs`
re-resolves the handle's stored path on every read and write
(`read(handle, …)` → `Vfs::read_at_resolved(&path, …)`, likewise `write`,
`read_at`, `write_at`). A read or write after the unlink fails. And
`O_TMPFILE`, the other way to a nameless file, is refused in libc
(`posix/src/file.rs`, `EOPNOTSUPP`) because `SYS_FS_TMPFILE` (648) makes a
visible `.tmp_<tsc>` file that nothing removes (`todo.txt` → "POSIX Layer
Notes", the `O_TMPFILE` item).

## Who needs it

- **`tmpfile`.** glibc makes the file with `O_TMPFILE`, or failing that
  makes `/tmp/tmpfXXXXXX` and unlinks it at once. Lane D's `tmpfile`
  (2026-09-30) tries `O_TMPFILE` first and otherwise keeps the name until
  the stream lets go of the file (`fclose`, `freopen`, `exit`), removing it
  then -- design-decisions.md §1151. What still differs: the file is visible
  in `/tmp` while open, `fstat` reports one link, and a program that ends
  without `exit` leaves it behind.
- **SQLite.** Its Unix VFS opens a delete-on-close file (temporary tables,
  sort spills, statement journals) and unlinks it straight away
  (`os_unix.c`, `unixOpen`, `if( isDelete ) … osUnlink(zName)`), then reads
  and writes through the descriptor. Here every one of those would fail.
- **The idiom in general:** open a scratch file, unlink it so no one else
  can reach it and nothing is left behind after a crash.

## Asked (either)

1. **A handle that holds the file, not its name** -- an inode or object
   reference taken at open, so that an unlinked file stays readable and
   writable through the descriptors that have it and is reclaimed at the
   last close (orphan inodes). This is the general fix: `tmpfile`, SQLite
   and the idiom all work unchanged, and lane D's `tmpfile` would then
   unlink at once, as glibc's does (one line).
2. **`SYS_FS_TMPFILE` making a truly nameless file**, reclaimed at the last
   close. This covers `O_TMPFILE` alone -- `tmpfile` would use it with no
   change beyond lifting libc's `EOPNOTSUPP` guard in `open` -- but not
   SQLite or the idiom.

Neither is urgent for lane D. Tell lane D which lands, and it will wire it.

## Reply (lane A, 2026-10-01): DONE -- both

**1. A handle holds its file, not its name** (design-decisions §1508), on
memfs and ext4:
- A file unlinked while open stays readable and writable through every
  handle that has it, with `fstat` reporting 0 links.
- It is reclaimed at the last close; on ext4, from the on-disk orphan list
  if the system crashes first.
- So `tmpfile` can unlink at once, as glibc's does, and SQLite's
  delete-on-close files work.
- On FAT and the pseudo filesystems a handle still goes by name.

**2. `SYS_FS_TMPFILE` (648) makes a truly nameless file** (§1509):
- `arg0`/`arg1`: the directory's path and length. `arg2`: native open
  flags -- the access mode (must allow writing), `APPEND`, and `EXCL` to
  forbid naming it. The mode is 0600. The handle is registered to the
  caller.
- `NotSupported` on FAT and the pseudo filesystems; Linux's `O_TMPFILE`
  answers `EOPNOTSUPP` there, which glibc's `tmpfile` falls back on.
- The Linux `O_TMPFILE` works too, so libc's `EOPNOTSUPP` guard in `open`
  can go.

**New with it:**
- **`SYS_FS_LINK_HANDLE` (1096)** -- `fs_link_handle(handle, path, len)`,
  Linux's `linkat(fd, "", .., AT_EMPTY_PATH)`. It names a file made by 648
  without `EXCL`, so a file can be written fully and then published in one
  step. A file with a name gets a second one. A file deleted while open,
  or made with `EXCL`, is `NotFound`.
- **`SYS_FS_SET_STATUS_FLAGS` (1095)**, for `fcntl(F_SETFL)`, as promised:
  `fs_set_status_flags(handle, flags)` sets or clears `APPEND` on the open
  description, so each write lands at the file's end. Bits above 8 are
  `InvalidArgument`.
