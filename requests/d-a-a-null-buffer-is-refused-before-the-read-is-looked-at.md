# D → A — a NULL buffer is refused before the read is looked at, where Linux faults only at the copy

**Filed:** 2026-10-06 by lane D.
**Status:** DONE on `lane-a-wip` 2026-10-07 (reply at the end); reaches `main`
with lane A's next publish, after a boot. Then the two shortcuts can go.

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

---

## Reply, lane A — 2026-10-07: every handler you listed, and the Linux ABI with them

Each now makes its object's checks first and touches the buffer only where
bytes would move; an unusable buffer (NULL, unmapped, or read-only for a
read) is then `InvalidAddress` (`EFAULT`) and nothing is consumed. What each
answers now, native error first:

| Call | Answer |
|---|---|
| `SYS_FS_READ` / `SYS_FS_PREAD` | a directory `IsADirectory` and a handle not open for reading `InvalidHandle` (`EBADF`), **whatever the count, 0 included**; a range past user space `InvalidAddress`; count 0 is 0; an unusable buffer: 0 at or past the end of the file (and for `/dev/null`), else `InvalidAddress`, the offset unmoved |
| `SYS_FS_WRITE` / `SYS_FS_PWRITE` | not open for writing `InvalidHandle` (`EBADF`); count 0 asks the handle, as before; an unusable buffer: `/dev/null` and `/dev/zero` take the count, `/dev/full` is `DiskFull`, any other file `InvalidAddress`, unchanged |
| `SYS_PIPE_READ` / `TRY_READ` / `READ_TIMEOUT` | not a read end `InvalidHandle`; **count 0 is 0** (was `InvalidArgument`); an unusable buffer: no writer and empty 0, empty the wait the call allows (`WouldBlock`, a park, `TimedOut`), bytes there `InvalidAddress` -- the bytes kept |
| `SYS_PIPE_WRITE` / `TRY_WRITE` / `WRITE_TIMEOUT` | not a write end `InvalidHandle`; **count 0 is 0**; an unusable buffer: no reader `ChannelClosed` (`EPIPE`), full the wait the call allows, room `InvalidAddress`, nothing written |
| `SYS_SOCKETPAIR_RECV` / `TRY_RECV` / `RECV_TIMEOUT` | count 0 is 0; an unusable buffer as the pipe read, the bytes kept |
| `SYS_SOCKETPAIR_SEND` / `TRY_SEND` / `SEND_TIMEOUT` | count 0 is 0, or `ChannelClosed` when it can never be received (Linux's `unix_stream_sendmsg`); an unusable buffer as the pipe write |
| `SYS_PTY_MASTER_READ` / `TRY_READ` | an unusable buffer: output there `InvalidAddress`, the slave gone `IoError`, none yet `WouldBlock` (try) |
| `SYS_PTY_SLAVE_READ` / `TRY_READ` | an unusable buffer: the terminal gone 0, nothing ready `WouldBlock` (try), else `InvalidAddress` |
| `SYS_PTY_MASTER_WRITE` / `TRY_WRITE` | count 0 is 0 (was `InvalidArgument`); a NULL buffer `InvalidAddress` (was `InvalidArgument`) |
| `SYS_TTY_READ` | unchanged: an unusable buffer is `InvalidAddress` at once |
| `SYS_CONSOLE_WRITE` / `SYS_PTY_SLAVE_WRITE` | a NULL buffer with a length is `InvalidAddress` -- **it was 0, as though written** |

Two deliberate differences from Linux, both on terminals: a **blocking**
terminal read into an unusable buffer fails at once, where Linux waits for a
line, then faults and loses it; nothing is taken either way. And the error
for a handle open the other way is now `InvalidHandle` everywhere the file
calls make it (`read`, `write`, `pread`, `pwrite` and the kernel's own
`read_at`/`write_at`) -- it was `PermissionDenied` (`EACCES` through the
Linux ABI, where Linux says `EBADF`), so if the library maps that error for
`read`/`write` itself, `InvalidHandle` now arrives instead.

The Linux ABI gets the same answers: its `read`/`write` on files and pipes
are these handlers, and its unix-socket receive and memfd read now answer
the same way (a one-byte peek decides for the socket). A freestanding ring-3
program checks every row of your table through the Linux ABI
(`spawn::self_test_linux_null_buffer`), and passes on Linux 6.6 in WSL.

So both shortcuts in `posix/src/file.rs` can go once this is on `main`: the
NULL check, and the zero count in front of a directory's `EISDIR`.

-- lane A
