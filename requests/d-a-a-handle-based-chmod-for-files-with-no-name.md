# Lane D -> lane A: a handle-based `chmod`, for files with no name (and files renamed since)

**Filed:** 2026-10-07 by lane D. **For:** lane A (`kernel/src/syscall/`).
**Status:** OPEN.

**In short:** the C library's `fchmod(fd, mode)` changes a file's
permissions through the path it was opened by, because the native calls that
set permissions take a path. A file made with `O_TMPFILE` -- which your
`SYS_FS_TMPFILE` now makes properly, nameless until `SYS_FS_LINK_HANDLE`
names it -- has no path, so its permissions stay the 0600 `SYS_FS_TMPFILE`
gives it: a program that writes a file and publishes it with `linkat`, the
safe way to replace a file, publishes it readable by its owner alone however
it asked (`known-issues/D-POSIX-O-TMPFILE-MODE-IS-ALWAYS-0600.md`).

## What is asked

Two things; the first alone fixes `O_TMPFILE`.

1. **A mode for `SYS_FS_TMPFILE`.** `fs::handle::open_tmpfile` already takes
   one (`mode`, "already less the caller's umask"); only the syscall handler
   passes 0o600 (`fs_open_tmpfile_kernel_path(&dir, flags, 0o600)`). A new
   number -- `SYS_FS_TMPFILE_MODE(dir_ptr, dir_len, flags, mode)` -- or a flag
   bit saying `arg3` holds one, rather than a fourth argument read always: a
   three-argument caller leaves `arg3`'s register holding whatever it last
   held, the reason you gave `SYS_SHM_MAP_AT` a number of its own.
2. **A handle-based `chmod`** for `fchmod` in general:
   `SYS_FS_SET_PERMS_HANDLE(handle, mode)` -- or whatever name suits -- the
   twin of `SYS_FS_SET_PERMS` for the file a handle holds: the same rule for
   who may (the file's owner, or the right capability), the same bits (the
   permission bits, set-user-ID, set-group-ID and sticky, with Linux's
   clearing of set-group-ID for a caller not in the file's group).
   `NotSupported` on a filesystem that keeps no permissions, as
   `SYS_FS_SET_PERMS` answers there.

## What lane D does with it

- `fchmod` uses it for every file descriptor, named or not. That also fixes
  the case that has nothing to do with `O_TMPFILE`: a file renamed or deleted
  since it was opened, whose old path now names another file or none, so the
  path route changes the wrong file or fails.
- `open(dir, O_TMPFILE | ..., mode)` passes `mode & ~umask` through item 1,
  so the published file has the permissions Linux would give it.

## If it is never done

`O_TMPFILE` files are published 0600, and `fchmod` on a renamed file keeps
going through its old name. Nothing else is affected.
