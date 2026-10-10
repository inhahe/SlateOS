### A-LINUX-OPEN-OF-A-DIRECTORY-FOR-READING-WAS-EISDIR -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** a Linux program opening a directory with `open(dir, O_RDONLY)`
and no `O_DIRECTORY` got `EISDIR`. Linux opens it. Programs do this to
`fchdir` back later (`open(".", O_RDONLY)`), and SQLite does it to `fsync`
the directory a journal is in. `opendir` passes `O_DIRECTORY` and was not
affected.

**Fixed:** `OpenFlags::DIRECTORY_ALLOWED` opens a directory or a regular
file, whichever the path names. The Linux layer sets it for a read-only
open without `O_CREAT` or `O_TRUNC`. A directory is still refused anything
that would write it. Tests: `test_held_files` rung 12, and
`test_linux_create_modes`.
