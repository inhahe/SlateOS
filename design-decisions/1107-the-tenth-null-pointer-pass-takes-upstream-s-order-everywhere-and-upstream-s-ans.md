## 1107. The tenth NULL-pointer pass takes upstream's order everywhere, and upstream's answer in all but two named places

**Date:** 2026-09-25
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** libc checks a program's arguments in the order Linux does, so a
program that passes two bad arguments hears about the same one it would on
Linux. The tenth pass of the NULL-pointer audit (`known-issues.md` →
`D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT`) brought `clone`,
`clone3`, `mount`, `umount2`, `process_vm_readv`, `waitid`, `epoll_ctl`,
`epoll_wait`, the eventfd wrappers, `signalfd` and `inotify_add_watch` into
line. In two places it copies Linux's order but not Linux's answer, because
the answer would mislead a program on this system; in a third it copies glibc
down to a quirk. This entry records those three calls, so that nobody
"corrects" them back.

**1. A zero-byte `process_vm_readv`/`writev` is `ENOSYS`, where upstream returns
0.** Upstream returns 0 when the local vector holds no bytes
(mm/process_vm_access.c:275), and again when the remote one holds none
(:181), both before it looks the pid up. Here both calls are stubs: the native
ABI has no number for them (see the doc comment on `process_vm_readv`).

| Option | What changes | For | Against |
|---|---|---|---|
| (a) return 0, as upstream | `process_vm_readv(pid, NULL, 0, NULL, 0, 0)` succeeds | exact | a zero-length call is the shape of a feature probe; it would conclude the call exists, and the program's real transfers would then fail with `ENOSYS` after it had passed over its fallback (`ptrace`, `/proc/pid/mem`) |
| (b) **`ENOSYS` there (chosen)** | the same call fails with "not implemented" | the probe learns the truth | one input on which the answer differs from Linux |

The *order* is still upstream's: nothing after the early return is examined,
so an empty local vector with a bad pid is `ENOSYS`, never `ESRCH`. Revisit when
the native ABI gains the calls; then (a) is simply correct.

**2. `epoll_wait` with a NULL buffer faults where upstream faults — at
delivery — while `read` still faults at the range check.** On x86-64,
`access_ok` admits NULL: `valid_user_address` is a sign test
(arch/x86/include/asm/uaccess_64.h:57). So upstream's NULL fault happens at the
first copy, and for `epoll_wait` that is `ep_send_events` (fs/eventpoll.c:1736):
a NULL buffer with nothing ready waits out its timeout and returns 0, and a bad
`epfd` with a NULL buffer is `EBADF`. `epoll_wait` now does exactly that,
leaving an undelivered event pending and a oneshot armed, as upstream does.

`read` (the eighth pass, `posix/src/file.rs`) tests NULL where `access_ok` sits
instead. That gets its `EBADF` ordering right, because `ksys_read` looks the
descriptor up before the range check, but answers `EFAULT` where Linux's read
would have copied nothing — at end of file, on an empty non-blocking pipe, for a
directory's `EISDIR`. It stays that way for now because `read`'s per-kind arms
dereference the buffer themselves; moving the test to each copy is the proper
fix and is recorded in the audit entry's "What remains". Choosing the faithful
model for `epoll_wait` rather than matching `read`'s shortcut was the decision:
consistency with an approximation is not worth an observable divergence in the
new code.

**3. `eventfd_read`/`eventfd_write` are glibc's composition verbatim, quirk
included.** glibc's are `read`/`write` of eight bytes, returning -1 unless eight
moved (sysdeps/unix/sysv/linux/eventfd_read.c, eventfd_write.c). Two things
follow that look like bugs: a descriptor that is not an eventfd is read from or
written to rather than refused, and a *short* transfer — possible only on such a
descriptor — returns -1 with `errno` untouched, since `read` itself succeeded.

| Option | For | Against |
|---|---|---|
| (a) **glibc exactly (chosen)** | a program ported from Linux behaves identically, down to its error paths | the stale `errno` on a short transfer |
| (b) keep refusing non-eventfds with `EINVAL` (the old code) | looks safer | an errno glibc never gives, for a call glibc makes happily — the old code attributed it to the kernel's read, which has no such check |
| (c) set an errno of our own on a short transfer | no stale `errno` | invents a verdict, which is what the audit exists to remove |

The quirk is documented at both functions.
