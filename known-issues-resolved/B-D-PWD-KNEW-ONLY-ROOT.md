### [D] B-D-PWD-KNEW-ONLY-ROOT — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/pwd.rs`, `posix/src/shadow.rs`, and the new
`posix/src/nss_files.rs`.

**What it was.** The C library's user, group and shadow databases were one
built-in `root` entry and nothing else. `getpwnam`, `getpwuid`, `getgrnam`,
`getgrgid`, `getgrouplist`, `getspnam` and the enumerations never read
`/etc/passwd`, `/etc/group` or `/etc/shadow`, so every C program -- the owner
column of `ls -l`, `id`, `su`, Python's `pwd` module -- knew no user but root,
whatever the files held (they are generated from `/etc/users.yaml`,
design-decisions §353). A NULL name was "not found" where glibc faults, a
reentrant group lookup assumed the caller's buffer was 8-byte aligned, and
`getpwent_r` and `getgrent_r` did not exist.

**Fix.** The three files are read as glibc 2.39's `nss_files` reads them:
its line syntax (comments and blank lines skipped, leading white space
dropped, numbers as `strtou32`, NIS `+`/`-` lines skipped by lookups and kept
by enumeration), its `_r` results (0 for found and for not found, `ERANGE`,
`errno` set to the return), the non-reentrant forms' own buffer, and
`getgrouplist` as `initgroups_dyn`. `/etc/shadow`'s privilege is the file's
own: without it the lookup fails with `open`'s `EACCES`. A missing file is
still the built-in root (design-decisions §1113).

**What remains.** `getlogin` still answers `root` rather than the terminal's
utmp entry; `initgroups` still succeeds without setting anything (the kernel
keeps no supplementary groups); `fgetpwent`, `putpwent` and their group and
shadow counterparts do not exist.
