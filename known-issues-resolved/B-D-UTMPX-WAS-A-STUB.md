### [D] B-D-UTMPX-WAS-A-STUB — 2026-09-26 — FIXED 2026-09-26 (libc); the file and its writers requested

**Where:** `posix/src/utmpx.rs` -- `setutxent`, `getutxent`, `getutxid`,
`getutxline`, `pututxline`, `endutxent`, `utmpxname`, `updwtmpx`, and
glibc's names for them (`getutent_r` and the other `_r` calls are new).

**In short:** Unix keeps a small file of who is logged in where,
`/var/run/utmp`, and a history of logins, `/var/log/wtmp`; `who`, `w`,
`last` and `getlogin` read them, and `login` and `sshd` write them. The C
library's calls for this were stubs: every read found nothing, and a write
reported success without writing anything. They are glibc's now.

**What it does, as glibc 2.39's `login/utmp_file.c` does:**

- The file is glibc's: 384-byte records with 32-bit time fields -- what every
  Linux tool reads, and what this tree's `utmpfile` crate reads for `who`,
  `last`, `w` and `finger`. A C program holds musl's 400-byte `struct utmpx`;
  each record is converted on its way in and out.
- `getutxid` matches the time records by type and the process records by
  `ut_id` (by `ut_line` when an id is empty); `getutxline` finds login and
  user records by line; both read forward from where the reading stopped.
- `pututxline` overwrites the record it matches -- the last one read first --
  or appends, and returns the caller's own pointer; a record written in part
  is cut off again (`ENOSPC`). `updwtmpx` appends to the history the same way.
- A missing file is never created: every call answers with the failed
  `open`, as glibc's do.
- Locks are `fcntl(F_SETLKW)`, as glibc's -- which this libc does not enforce
  yet (`requests/b-a-advisory-record-locking-is-a-stub-that-always-succeeds.md`).

**What it waits on, outside the libc** (`requests/d-b-utmp-is-a-real-file-now-create-it-and-write-logins.md`):
nothing creates `/var/run/utmp` or `/var/log/wtmp` at boot, and nothing
writes a record at login -- so until something does, `who` still sees
nobody, as it does on a Linux system whose init never made the file.
