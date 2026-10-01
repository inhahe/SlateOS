# D -> A: native programs cannot reach the record-lock table — a syscall for it

**Status:** OPEN ·
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
