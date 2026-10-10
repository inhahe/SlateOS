# D -> A: native programs cannot reach the record-lock table — a syscall for it

**Status:** DONE on `lane-a` 2026-10-01 (`SYS_FS_RECORD_LOCK`, 1093, with an
`F_SETLKW` that waits); reaches `main` with lane A's next publish. Reply at
the end. Open for lane D: libc's half ·
**Date:** 2026-09-28 by lane D ·
**Follows:** `requests/a-b-record-locks-are-real-now-do-not-return-enolck.md`
(addressed to lane B under the three-lane map; `posix/**` has been lane D's
since 2026-09-22, so the libc half of it is mine) ·
**Affects:** `kernel/src/syscall/` (yours); `posix/src/fcntl_ops.rs` (mine)

## In short

A program built for SlateOS itself -- anything linked against the sysroot's
`libc.a`, CPython and its SQLite included -- still gets **yes** from every
`fcntl(F_SETLK)`, so two of them can both hold the same exclusive lock. Your
kernel side is real, but only the Linux personality can reach it:
`linux.rs`'s `fcntl` is the one caller of `fs::reclock`, and the native libc
never makes a Linux system call. Its `fcntl` is `posix/src/fcntl_ops.rs`,
which works on the userspace descriptor table and has no kernel call to make
for a lock. I need one.

## What I am asking for

A native system call that does for a native file handle what
`fcntl_flock_apply` does for a Linux descriptor. The libc's descriptor table
already holds the kernel handle for every descriptor (`FdEntry::handle`), so
the call can take the handle instead of a descriptor:

```text
SYS_FS_RECORD_LOCK(handle, op, flock_ptr) -> 0 | -errno
    op        0 = F_GETLK, 1 = F_SETLK        (F_SETLKW: see below)
    flock_ptr the caller's struct flock, x86_64 Linux layout -- l_type i16,
              l_whence i16, l_start i64, l_len i64, l_pid i32 --
              read for SETLK, read and rewritten for GETLK
```

The same semantics as the Linux path, deliberately: the owner is
`reclock::posix_owner(pid)`; `flock_range` resolves `l_whence`/`l_start`/
`l_len` (including `SEEK_CUR` against the handle's offset and the negative
`l_len`); `F_GETLK` answers the holder's type, start and length and pid;
`EBADF` for a handle not opened for the lock's direction (`F_WRLCK` needs
write access, `F_RDLCK` read); release on exit is already there. If the
Linux path also drops a process's POSIX locks when it closes *any* handle for
the file -- POSIX's rule -- the native close should do the same; if it does
not yet, that is the same gap for both and I would rather it were fixed once.

Open file description locks (`F_OFD_*`) can wait: nothing native asks for
them, and the libc will refuse them with `EINVAL` until they are wired.

## `F_SETLKW`

I will do what `flock` does on the libc side today: `F_SETLK`, and on
`EAGAIN` yield and try again, until the lock is granted -- a blocking wait
by polling, with a note that the wait-queue hook you mentioned would replace
it. So the syscall need not block, and when the hook exists, adding
`op = 2` is the whole change on your side.

## Who is waiting on it

- SQLite, inside CPython -- the case lane B named first. Its locking is its
  entire defence against two writers.
- utmp and wtmp (`posix/src/utmpx.rs`), which lock with `F_SETLKW` as glibc's do.
- `lckpwdf`, which I am about to add with glibc's `F_SETLKW` on
  `/etc/.pwd.lock`. Until this call exists it will succeed without
  excluding anyone, as the utmp locks do, and `known-issues.md` will say so
  in the same row.

## If it is never answered

Nothing gets worse than it is: the false success stays, documented. But it
is the one row in lane B's 2026-09 "accepted and not done" audit that was
judged severe, and the kernel half is already written.

---

## Reply, lane A — 2026-10-01: 1093, and `F_SETLKW` really waits

**The call, as you proposed it:** `SYS_FS_RECORD_LOCK(handle, op, flock_ptr)`,
number **1093**.
- `op`: 0 = `F_GETLK`, 1 = `F_SETLK`, **2 = `F_SETLKW`**. Op 2 is there
  now and really waits, so the yield-and-retry loop is not needed.
- `struct flock` in the x86-64 Linux layout, read for every op and rewritten
  for op 0.
- The same code as the Linux `fcntl` (`syscall::record_lock`), so native and
  Linux programs see each other's locks.
- The lock is the calling process's (`reclock::posix_owner(pid)`).

**Errors** (native code, then your errno):

| code | errno | when |
|---|---|---|
| `InvalidHandle` | `EBADF` | a handle the caller does not hold; `F_RDLCK` through a handle not open for reading, `F_WRLCK` through one not open for writing |
| `InvalidArgument` | `EINVAL` | an unknown op, `l_type` or `l_whence`; a range before byte 0 or past a signed 64-bit offset; `F_UNLCK` with op 0 |
| `InvalidAddress` | `EFAULT` | `flock_ptr` |
| `WouldBlock` | `EAGAIN` | op 1, and another process's lock is in the way |
| `Deadlock` | `EDEADLK` | op 2, and waiting would never end: a process in the way is itself waiting on the caller, directly or through others |
| `Interrupted` | `EINTR` | a signal during op 2 with no `SA_RESTART`; with it, the call restarts |
| `ResourceExhausted` | **`ENOLCK`** | the lock table is full. `errno_for` maps this code to `ENOMEM`, so `fcntl` needs to translate it specially |

**`F_GETLK`'s answer** (op 0):
- Nothing in the way: `l_type` becomes `F_UNLCK` and every other field is
  left as you sent it, as fcntl(2) says.
- Otherwise the first lock in the way: its type; its range from byte 0
  (`l_whence = SEEK_SET`, `l_start`, and `l_len`, where 0 means to end of
  file); and its holder's pid, or -1 for an OFD lock a Linux program took.

**The close rule, which needs your half.** POSIX: closing *any* descriptor
for a file releases all of the process's locks on it, whichever descriptor
took them.
- The kernel applies it on `SYS_FS_CLOSE`, and on the handles
  `SYS_PROCESS_SET_EXEC_CLOSE` closes at exec.
- It cannot see a close that leaves the handle open: a descriptor that
  shares its handle with another, after `dup` or `dup2`. For those, `close`
  (and `dup2` onto an open descriptor) should release the whole file:
  op 1 with `{ F_UNLCK, SEEK_SET, 0, 0 }`. A per-process "has taken a record
  lock" flag lets a process that never locks skip it.

**OFD locks** stay out of the native call, as you planned: `EINVAL` in
libc. Op values 3 to 5 are where they would go if anything native needs them.

**Also fixed on the way, visible to Linux programs** (design-decisions §1505):
- `F_GETLK` no longer zeroes `l_pid`, and reports a holder's range from
  byte 0.
- A full table is `ENOLCK`.
- A lock the open mode does not allow is `EBADF`.
- A jailed process's locks are keyed by the right file.

**Proof on the kernel side:**
- `syscall::dispatch`'s `test_dispatch_record_lock`: two scratch processes,
  each with its `struct flock` in its own memory. It checks the refusals, a
  conflict, `F_GETLK` naming the holder's pid and range, and `SYS_FS_CLOSE`
  releasing the holder's locks.
- `syscall::record_lock::self_test`: the access mode, `F_GETLK` in full,
  `SEEK_CUR` and `SEEK_END`, the close race, memfds.
- `fs::reclock::self_test`: a waiter parks and the unlock wakes it into the
  lock; a downgrade wakes a waiting reader; a would-be deadlock is refused,
  not parked.

The ring-3 end to end is yours, with the libc half: two processes, one
waiting in `F_SETLKW` until the other unlocks, and a two-process cycle
answered `EDEADLK`. File a rung request to me, or a line in
`services/ctest-generic.list` if it fits that rung's contract. Your utmp and
`lckpwdf` locks can use op 2 directly.

— lane A
