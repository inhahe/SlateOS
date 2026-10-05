## D-POSIX-FCNTL-RECORD-LOCKS-NEVER-REACH-THE-KERNEL — a native program's `fcntl(F_SETLK)` still says yes to every lock (lane D, 2026-09-28) — **Status: OPEN (blocked on lane A: `requests/d-a-native-programs-cannot-reach-the-record-lock-table.md`)**

**In short:** two SlateOS programs can both "hold" the same exclusive file
lock. File locks are how SQLite keeps two writers from corrupting a
database, how `login` and `who` share the utmp file, and how the tools that
edit `/etc/passwd` keep out of each other's way (`lckpwdf`). A program built
for SlateOS gets its `fcntl` from this libc, whose record locking never asks
the kernel -- it answers "granted" to everything.

**Where:** `posix/src/fcntl_ops.rs`, `F_GETLK`/`F_SETLK`/`F_SETLKW`: the
first always reports "no conflicting lock", the other two always succeed.

**Why it is still open:** the kernel's lock table is real now (lane A,
`kernel/src/fs/reclock.rs`, 2026-09-21, with its release on exit and on
final close), but only the Linux personality reaches it -- `linux.rs`'s
`fcntl` is its one caller. A native program runs on this libc's descriptor
table, which holds a kernel file handle for each descriptor and has no
system call to lock through one. Lane A's answer to lane B
(`requests/a-b-record-locks-are-real-now-do-not-return-enolck.md`) was
written when `posix/**` was lane B's; under the six-lane map it is lane D's,
and the missing piece is a native system call, which is lane A's.

**Who is waiting on it:** SQLite inside CPython (lane B's original report,
`requests/b-a-advisory-record-locking-is-a-stub-that-always-succeeds.md`);
`posix/src/utmpx.rs`, which locks with `F_SETLKW` as glibc's does; and
`lckpwdf` (`posix/src/shadow.rs`, 2026-09-28), which locks
`/etc/.pwd.lock` the same way -- written against `fcntl` so that it becomes
real with nothing further to change.

**Proper fix:** lane A's native call (the request proposes
`SYS_FS_RECORD_LOCK(handle, op, flock *)` with `linux.rs`'s semantics), then
`fcntl_ops.rs` wired through it: `F_SETLK` and `F_GETLK` as they are asked,
`F_SETLKW` as `F_SETLK` retried with a yield until granted (the kernel does
not block for locks yet -- the same shape `flock` has), and `F_OFD_*`
refused with `EINVAL` until they are wired too.
