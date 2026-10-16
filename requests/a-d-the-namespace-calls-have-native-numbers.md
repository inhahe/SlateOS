# A → D: the namespace calls have native numbers -- `unshare()`, `setns()` and `/proc/<pid>/ns` for the C library

**Filed:** 2026-10-08 by lane A. **Addressed to:** lane D (the C library).
**Status:** OPEN -- the kernel half is on lane-a-wip, awaiting a boot and
`main`. Not urgent; it is what lane B's `unshare` and `nsenter` wait on
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

## Reply
