## D-POSIX-O-TMPFILE-MODE-IS-ALWAYS-0600 — a file made with `O_TMPFILE` has permissions 0600 whatever `open` asked, and `fchmod` cannot change them while it has no name (lane D, 2026-10-07)
**Status:** OPEN -- needs a mode for `SYS_FS_TMPFILE`, and a handle-based `chmod`, from lane A (`requests/d-a-a-handle-based-chmod-for-files-with-no-name.md`)

**In short:** `open(dir, O_TMPFILE | O_RDWR, 0644)` makes a file with no name,
which a program writes whole and then publishes with `linkat` -- the safe way
to replace a file. On Linux the published file has the permissions `open` was
given (0644, less the umask). Here it always has 0600, so a file meant to be
readable by everyone is readable only by its owner once published, and a
program that calls `fchmod(fd, 0644)` before `linkat` to fix that is told it
worked when nothing changed.

**Where:**

- `posix/src/file.rs`, `open_tmpfile`: the kernel's `SYS_FS_TMPFILE` takes no
  mode and makes the file 0600 (`kernel/src/syscall/number.rs`, "The file's
  mode is 0600").
- `posix/src/file.rs`, `fchmod`: it changes a file's permissions through the
  path the descriptor was opened by (`set_perms_path`), and a descriptor made
  with `O_TMPFILE` has none. With no path it answers 0 and changes nothing --
  right for a pipe or a socket, whose mode nothing reads, wrong for a file.

**Reproduce:** on a lane A kernel, `open("/tmp", O_TMPFILE | O_RDWR, 0644)`,
`linkat(fd, "", AT_FDCWD, "/tmp/f", AT_EMPTY_PATH)`, `stat("/tmp/f")`:
`st_mode & 0777` is 0600, where Linux gives 0644 & ~umask.

**The proper fix:** two kernel calls, asked of lane A in the request above:
a mode for `SYS_FS_TMPFILE` -- the kernel's `fs::handle::open_tmpfile` takes
one already; only the syscall handler fixes 0o600 -- which `open_tmpfile`
passes as `mode & ~umask`; and the handle-based twin of `SYS_FS_SET_PERMS`,
which `fchmod` uses for every file descriptor (it is also right for a named
file renamed or deleted since it was opened, which the path route gets
wrong). Until then the 0600 is the kernel's, and the library says so in
`open_tmpfile`'s documentation.
