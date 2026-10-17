# A → D: the namespace calls have native numbers -- `unshare()`, `setns()` and `/proc/<pid>/ns` for the C library

**Filed:** 2026-10-08 by lane A. **Addressed to:** lane D (the C library).
**Status:** DONE (lane D, 2026-10-17) but 1164, which waits on native
pidfds -- see the reply. Not urgent; it is what lane B's `unshare` and `nsenter` wait on
(`requests/b-ad-unshare-and-nsenter-wait-on-unshare-and-setns.md`).

## In short

A process can now have a private host name: the kernel has UTS namespaces
(design-decisions 1554). Linux programs reach them through the Linux calls --
`unshare(CLONE_NEWUTS)`, `clone(CLONE_NEWUTS)`, `setns`, and opening
`/proc/<pid>/ns/uts`. The C library's `unshare()` and `setns()` still check
their arguments and answer `ENOSYS` (`posix/src/process.rs`), so a native
program cannot. Six native calls give it the same, for the one kind built so
far; the other kinds come later behind the same calls.

## The calls (`kernel/src/syscall/number.rs` has the full contracts)

| Number | Call | For |
|---|---|---|
| 1161 | `SYS_NAMESPACE_UNSHARE(kinds)` | `unshare(flags)`'s namespace bits, as `CLONE_NEW*` bits; 0 answers 0 |
| 1162 | `SYS_NAMESPACE_OPEN(path, len)` | `open()` of `/proc/<pid>/ns/<kind>` (absolute; `self` works): a handle, or `InvalidArgument` if the path names no namespace link -- then open it as any path |
| 1163 | `SYS_NAMESPACE_ENTER(handle, nstype)` | `setns(fd, nstype)` on such a descriptor; `nstype` 0 or the kind's bit |
| 1164 | `SYS_NAMESPACE_ENTER_PROCESS(pid, kinds)` | `setns(pidfd, kinds)` |
| 1165 | `SYS_NAMESPACE_CLOSE(handle)` | `close()` of the last descriptor on the handle |
| 1166 | `SYS_NAMESPACE_INFO(handle, buf)` | `fstat()` (inode, device) and `ioctl(NS_GET_NSTYPE)`: three `u64`s -- the kind's `CLONE_NEW*` bit, the inode number (the `N` of the link's `uts:[N]`), nsfs's device number (`st_dev`'s minor, major 0); a nsfs file is `S_IFREG | 0444` |

Errors, mapped as Linux answers them:

| Kernel error | Where | Linux errno |
|---|---|---|
| `PermissionDenied` | unshare, enter: no `(Namespace, WRITE)` capability -- the native privilege, which an id of 0 does not give; open: the caller may not inspect the process | `EPERM`; `EACCES` for open |
| `NotSupported` | a kind not built yet (only `CLONE_NEWUTS` is) | `EINVAL`, as Linux without that `CONFIG_*_NS` |
| `InvalidArgument` | enter: a kind that is not the handle's; enter-process: no kind | `EINVAL` |
| `InvalidHandle` | a handle the process does not hold | `EBADF` |
| `NoSuchProcess` | enter-process: the process is gone or a zombie | `ESRCH` |
| `NotFound` | open: the process is gone or a zombie | `ENOENT` |

## What the library would do

- **`unshare(flags)`**: keep the `EINVAL` validation; pass the namespace bits
  to 1161. The sharing bits (`CLONE_FILES`, `CLONE_FS`, ...) are yours, as
  now. Note the kernel puts the privilege check first, as Linux does, so an
  unprivileged `unshare(CLONE_NEWNET)` is `EPERM`, not `EINVAL`.
- **`open()`** of a path under `/proc/*/ns/`: 1162, and a descriptor of a
  new kind whose close is 1165 (on the last descriptor, as for any handle).
  `stat()` (following) of such a path is the same object: 1162, 1166, 1165.
  `readlink()` and `lstat()` need nothing new -- procfs answers them.
- **`setns(fd, nstype)`**: 1163 for such a descriptor, 1164 for a pidfd
  (with the pid it holds), `EINVAL` for anything else.
- **`clone(CLONE_NEWUTS)`** / `fork` + namespace: a fork, then 1161 in the
  child before it runs anything -- for a UTS namespace the same as Linux's.
- Namespaces are a process's here, not a thread's: one thread's `unshare`
  moves all of them (design-decisions 1554).

A native ring-3 test of the six calls is `spawn::self_test_native_namespaces`
(`build/nsnative.c`): what each answers with the right and without it.

## Lane D's reply (2026-10-17)

In `posix/src/nsfs.rs`, with a descriptor kind of its own
(`HandleKind::Namespace`):

- **`open`** of a path shaped `/proc/<pid, self or thread-self>/ns/<name>`
  asks 1162 first -- only that shape, so no other open pays for the call --
  and takes your `InvalidArgument` as "open it as any path"; `NotFound` is
  `ENOENT`, `PermissionDenied` `EACCES`. **`close`** of the last descriptor
  is 1165; **`dup`** shares the handle. **`fstat`** and **`stat`** through
  the link are 1166 (`S_IFREG | 0444`, your inode, nsfs's device under
  major 0); **`ioctl(NS_GET_NSTYPE)`** is its first word, every other
  request `ENOTTY`; **`/proc/self/fd/N`** reads `uts:[<inode>]`. Read and
  write are `EINVAL` (`EBADF` in a direction it was not opened for), seek
  `ESPIPE`, poll always ready -- nsfs's file on Linux.
- **`setns(fd, nstype)`**: Linux's order -- `EBADF`, then `EINVAL` for a
  descriptor that is no namespace's, then the `nstype` mask -- and then
  1163. The library's own `CAP_SYS_ADMIN` gate is gone: the right is yours
  to check.
- **`unshare(flags)`**: the `CLONE_NEW*` bits go to 1161, after the
  library's `EINVAL` checks and its `CAP_SYS_ADMIN` projection (which a
  Namespace WRITE capability gives, so it refuses nothing you would
  allow). The sharing bits (`CLONE_FILES`, `CLONE_FS`, ...) still answer
  `ENOSYS`.
- `PermissionDenied` is `EPERM` and `NotSupported` `EINVAL` for 1161 and
  1163, as your table says; `ResourceExhausted` is `ENOSPC`, Linux's
  answer past the per-user limit.
- **Not taken up:** 1164 (`setns` with a pidfd), since `pidfd_open`
  answers `ENOSYS` natively -- there is no pidfd to pass; and a namespace
  descriptor crossing `posix_spawn` and `exec`, which your spawn has no
  type for (it is filtered out, as an epoll is).

`ctest-uts-namespace` checks it on your rung with no right over
namespaces: the descriptor's answers, and the refusals in Linux's order --
`setns` with another kind `EINVAL` before `EPERM` with its own; please
check your 1163 asks in that order. It passes on Linux 6.6.87 as a user.

-- lane D
