## TD-B-LOCKFILE-IS-NOT-PROCMAILS (lane B, 2026-09-26) — **FIXED** 2026-10-01

**Resolution.** `userspace/lockfile` is now procmail 3.24's `lockfile.c` as
Ubuntu builds it (3.24-1ubuntu2), with what it calls from `exopen.c`,
`acommon.c`, `authenticate.c` and `mcommon.c`, function by function: the
unique temporary (`_` pid separator time `.` host, procmail's base 64), the
`fstat`/`lstat` check, the hard link with NFS's false failure caught, the
`EXDEV` fallback, `-l` as the age of a stale lock, the second pass that
releases what was taken, and procmail's messages, version text and sysexits.
`scripts/lockfile-diff.sh` runs it against Ubuntu's (fetched with
`apt-get download`, no root needed) over 46 cases, comparing output, status
and the files left behind -- mode, link count, contents: 46 agree. 33 unit
tests drive the control flow over a fake system (a lying `link`, `EXDEV`, a
name length limit, a signal). Licence: GPL-2.0-or-later OR Artistic-1.0,
`userspace/lockfile/licenses/`. The entry below is as it was filed.

**In short:** `lockfile` -- the command scripts use to create a lock file
the way procmail does -- is a SlateOS approximation, not a port. It became
its own program on 2026-09-26 (it had been an unreachable personality of
`flock`), and its code moved unchanged, so its differences from procmail's
`lockfile(1)` did too. Found while splitting it out; not yet measured against
procmail's, which WSL can provide (`apt install procmail`).

**Where it is known to differ** (`userspace/lockfile/src/main.rs`):
- `-l locktimeout` is used as a deadline for giving up; in procmail it is the
  age after which an existing lock file is considered stale and removed.
- A bad number (`-r x`, `-l x`, `-s x`) silently becomes a default rather
  than being refused.
- The messages (`giving up on lock file`) and the exit statuses are not
  procmail's, and `-ml`/`-mu` (the user's mailbox) are approximated.

**The proper fix:** a port of procmail 3.24's `lockfile.c`, measured by a
`lockfile-diff.sh` against WSL's, as `flock` and `getopt` were.
