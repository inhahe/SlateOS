## 1171. The C library answers `/dev/fd/N`, `/dev/stdin` and `/proc/self/fd` for a native program, reopening as Linux does where it can

**Date:** 2026-10-05
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** on Linux, `/dev/fd/3` means "my file descriptor 3" and
`/dev/stdin` "my standard input", and shell scripts and programs use those
names all the time -- `diff <(sort a) <(sort b)` hands `diff` two of them.
On SlateOS a native program's file descriptors belong to its C library, not
the kernel, so the kernel cannot answer these names: `/dev/fd` did not exist,
and `/dev/stdin` was the console whatever standard input really was. The C
library now answers them itself, before the kernel is asked, the way it
already answers `/dev/ptmx`. Opening one gives what Linux gives -- a fresh
open of the same file or pipe -- except in two rare cases written down
below.

| Option | What a program sees |
|---|---|
| **A. The C library answers the names** (chosen) | `/dev/fd/N`, `/dev/stdin` and kin, and `/proc/self/fd` behave as on Linux for open, stat, lstat, statx, access, readlink and opendir; two edge cases differ |
| B. The kernel answers them | needs the kernel to see a native process's descriptor table, which today lives in userspace by design; a large change to lane A's process model |
| C. Duplicate (BSD's `fdescfs`), not reopen | simpler, but a file reopened through `/dev/fd/N` shares N's position, where Linux -- our reference -- starts at 0 |

**Why A.** The descriptors are the C library's, so it is the one place that
knows what `/dev/fd/3` is. The library already answers two such names,
`/dev/ptmx` and `/dev/pts/<n>`, for the same reason (lane A declined a
kernel device-node mechanism for them). B would move the descriptor table
into the kernel for the sake of a few names.

**Why reopen, not duplicate (A over C).** Linux 6.6, measured under WSL: a
file opened through its `/dev/fd/N` link is a new open file description,
starting at offset 0 whatever N's offset is; `O_TRUNC` truncates it; the
access mode may be wider than N's, as far as the file's own permissions
allow; an unlinked file still reopens. The kernel's `SYS_FS_DUP` makes a
new handle on the same file with a cursor of its own, so the library gets
Linux's semantics for a file, unlinked ones included, by duplicating the
handle and moving the new cursor to 0. Asked for more access than N has, it
opens the path N was opened by -- while that path is still the same file --
so the permission check is the file's own.

**Where it differs, on purpose:**

* A pipe end asked for the other direction is `EACCES`. Linux reopens the
  pipe's inode, and can hand out either end; the library holds one end's
  handle only.
* A file asked for more access than N has, after it was renamed or
  unlinked, is `EACCES`: Linux reopens the inode, and the library can widen
  access only by the path.
* Terminals, sockets and the rest are duplicated, so the new descriptor
  shares N's status flags (`O_NONBLOCK`, `O_APPEND`).

**What changes for bash:** its configure answers stay `bash_cv_dev_fd=absent`
and `bash_cv_dev_stdin=absent` until a boot rung shows process substitution
working through these names on SlateOS; then they become `standard` and
`present`, and `<(...)` stops needing FIFOs (`scripts/bash-spike/cross2.sh`).

**Where:** `posix/src/fdname.rs`; the call sites in `posix/src/file.rs`
(`open`, `openat`, `stat`, `lstat`, `statx`, `access`, `readlink`) and
`posix/src/dirent.rs` (`fdopendir`).
