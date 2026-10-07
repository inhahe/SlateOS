# D → A — a NULL buffer is refused before the read is looked at, where Linux faults only at the copy

**Filed:** 2026-10-06 by lane D.
**Status:** OPEN -- for lane A (`kernel/src/syscall/handlers.rs`: the read and
write handlers). Not urgent: it changes only what a buggy call is told.

**In short:** some programs pass `read` or `write` a NULL buffer by
mistake. Test suites deliberately check what comes back. Linux checks
everything else first and faults only when it would copy, so:
- at the end of a file, the answer is 0;
- on an empty non-blocking pipe, `EAGAIN`;
- for a directory, `EISDIR`;
- on a descriptor open for the other direction, `EBADF`;
- `/dev/null` takes the write.

SlateOS refuses the NULL first. The kernel's `SYS_FS_READ` says `EINVAL`.
The C library therefore says `EFAULT` in front of it, as the nearest
honest answer. Since today the C library answers as Linux does for the
kinds it implements itself: eventfd, timerfd, inotify, epoll, and sockets
with no connection (design-decisions §1107's follow-up, measured on
Linux 6.6). For the kernel's kinds it cannot, because only the kernel
knows whether the file is at its end, the pipe empty, or the handle a
directory.

## What Linux 6.6 answers (measured, WSL)

| Call | Linux | SlateOS now |
|---|---|---|
| `read(file, NULL, 5)` mid-file | `EFAULT`, offset unchanged | `EFAULT` |
| `read(file, NULL, 5)` at end of file | 0 | `EFAULT` |
| `write(file, NULL, 5)` | `EFAULT` | `EFAULT` |
| `write(O_RDONLY file, NULL, 5)` / `read(O_WRONLY file, NULL, 5)` | `EBADF` | `EFAULT` |
| `read(directory, NULL, 5)` | `EISDIR` | `EFAULT` |
| `read(directory, buf, 0)` | `EISDIR` | 0 |
| `read(/dev/null, NULL, 5)` / `write(/dev/null, NULL, 5)` | 0 / 5 | `EFAULT` |
| `read(/dev/zero, NULL, 5)` | `EFAULT` | `EFAULT` |
| `read(pipe, NULL, 1)`, empty, non-blocking | `EAGAIN` | `EFAULT` |
| `read(pipe, NULL, 1)`, writers gone | 0 | `EFAULT` |
| `read(pipe, NULL, 1)` with data | `EFAULT`, the data kept | `EFAULT` |
| `write(pipe, NULL, 1)` / reader gone | `EFAULT` / `EPIPE` and `SIGPIPE` | `EFAULT` |
| `read(socketpair, NULL, 1)`, empty, non-blocking / with data | `EAGAIN` / `EFAULT` | `EFAULT` |

## The ask

In the native read and write handlers:
- `SYS_FS_READ`/`WRITE`;
- `SYS_PIPE_READ`/`TRY_READ`/`WRITE`/`TRY_WRITE`;
- `SYS_SOCKETPAIR_*`;
- `SYS_TTY_READ`;
- the pty reads and writes.

In each, check what Linux checks before its copy, and validate the user
buffer only where the copy happens: there, a NULL or unmapped buffer is
`EFAULT`. A read must not consume what it could not deliver. Linux's pipe
keeps its data, and a file's offset does not move. For a directory, a read
of any count, 0 included, is `EISDIR`. On x86-64 Linux, `access_ok` admits
NULL (design-decisions §1107 has the source), which is why every row
above reaches its own check.

When the kernel answers so, lane D removes the two shortcuts in front of
it in `posix/src/file.rs`, `read` and `write`: the NULL check, and the
zero count, which hides a directory's `EISDIR`. Tell lane D when it lands.
