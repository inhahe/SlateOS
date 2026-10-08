## 1551. Named pipes are pipes found by their node; Linux opens them as pipe descriptors, native programs by a call of their own

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a named pipe (a FIFO, made by `mkfifo`) is a file-system name
that two programs open to talk through a pipe -- a shell script's
`mkfifo f; producer > f & consumer < f`, or make's job server. SlateOS had
none: `mkfifo` was refused. Now it has them, with POSIX's rules for who waits
for whom. Three choices below had real alternatives: how the opener's
descriptor is represented, how native programs reach it, and how strictly
pipe writes follow POSIX.

**What it is** (`kernel/src/ipc/fifo.rs`, `ipc/pipe.rs`): the node holds
nothing (`EntryType::Fifo`, made on memfs and ext4). Opening it attaches the
opener to a kernel pipe kept for the node -- found by the node's identity, so
renames and hard links lead to the same pipe -- while anyone has it open; the
pipe holds the node (pinned, so an unlinked FIFO goes on serving its openers)
and goes, with any bytes left, with the last close. Opens wait as Linux's
`fifo_open` (reader for writer, writer for reader; `O_NONBLOCK` and `O_RDWR`
as Linux).

### 1. A Linux opener gets a pipe descriptor, not a file descriptor

**Chosen:** a FIFO's Linux descriptor is an ordinary pipe end
(`HandleKind::Pipe`) on a pipe that knows its node; `O_RDWR` gets one handle
holding both ends (`PipeEnd::Both`). Only `fstat`, `/proc/<pid>/fd` and the
re-open through it ask the pipe for its node.

**Alternative:** keep it a file descriptor (`HandleKind::File`) whose handle
forwards I/O to the pipe.

| | pipe descriptor (chosen) | file descriptor forwarding |
|---|---|---|
| read, write, `poll`, `epoll`, `SIGPIPE`, `splice`, `F_GETPIPE_SZ`, `FIONREAD`, `O_NONBLOCK` | the pipe's, already right and tested | each needs a FIFO branch in the file paths |
| `fstat`, `/proc/self/fd` | a FIFO branch | the file's, already |
| Linux | a FIFO's descriptor is a pipe (`pipefifo_fops`) | -- |

Fewer branches, and the many pipe behaviours stay one implementation.

### 2. Native programs open one through `SYS_FIFO_OPEN`

**Chosen:** `SYS_FS_OPEN` answers a FIFO's node `NoSuchDeviceOrAddress`, as it
answers a socket's, and a new call `SYS_FIFO_OPEN` (1158) opens it for a pipe
handle; `SYS_FS_MKFIFO` (1157) makes one. Lane D's C library adds the two
calls (`requests/a-d-named-pipes-have-native-calls.md`).

**Alternative:** have `SYS_FS_OPEN` itself return a file handle that reads and
writes the pipe, so a native program works with no C-library change.

The alternative would make `read` and `write` work at once, but the C library
would then treat the descriptor as a file: `poll` would call it always ready,
`O_NONBLOCK` set by `fcntl` would not reach it, and a write to it whose reader
has gone would get no `SIGPIPE`. The C library already does all of that
right for pipes; giving it a pipe handle uses that. The cost is a C-library
change before native `mkfifo` works -- which it needs anyway, its `mkfifo`
answering `ENOSYS` today.

### 3. Pipe writes follow POSIX now: whole, and atomic up to `PIPE_BUF`

**Chosen:** a blocking pipe write returns once every byte is in; one of at
most `PIPE_BUF` (4096) bytes waits for room for all of it and goes in whole
(nonblocking: `EAGAIN` with nothing written); `POLLOUT` means room for a
`PIPE_BUF` write. For every pipe, not FIFOs alone.

**Before:** a write put in what fitted and returned that count. A program that
treats a short write as an error lost data, and two writers into one FIFO --
the reason to have a FIFO, often -- could interleave inside each other's
lines. `POLLOUT` with one byte free told a nonblocking writer of 4096 bytes to
go ahead into an `EAGAIN`, in a loop.

**What is not Linux's:** Linux keeps a pipe as 16 page-sized slots; a write of
more than `PIPE_BUF` into a pipe whose slots are all taken is `EAGAIN` there,
where this ring of bytes puts in what fits. POSIX allows both, and nothing
measured depends on the difference.
