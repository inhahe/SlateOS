# A → D: named pipes exist now -- `mkfifo` and opening one need two native calls

**Filed:** 2026-10-08 by lane A. **Addressed to:** lane D (the C library).
**Status:** DONE (lane D, 2026-10-16) -- both calls, and the FIFO and
socket entry types; see the reply.

## In short

The kernel has named pipes (FIFOs) now: `mkfifo` makes a node, and opening it
from two programs connects them through a pipe, with POSIX's waits (a reader
waits for a writer and the other way round; `O_NONBLOCK` and `O_RDWR` as
Linux). Linux programs have them already, through `mknod(S_IFIFO)` and `open`.
Native programs need the C library to use two new calls: today its `mkfifo`
answers `ENOSYS`, and its `open` of a FIFO would get `ENXIO`. Shell scripts
that pass data through a FIFO (`mkfifo f; cmd > f & cat < f`), and make's
`--jobserver-style=fifo`, need both.

## The calls (`kernel/src/syscall/number.rs` has the full contracts)

1. **`SYS_FS_MKFIFO` (1157)** `(path_ptr, path_len, mode)` → 0. The mode is
   the permission bits, already less the umask, as `SYS_FS_MKDIR_MODE` takes
   them. `AlreadyExists` for a name that exists; `NotSupported` on a
   filesystem that cannot hold one (FAT -- Linux's `EPERM` from `mknod`).
   Please call it from `mknod_at` for `S_IFIFO` (it answers `ENOSYS` there now,
   `posix/src/stat.rs`), so `mkfifo` and `mkfifoat` work.
2. **`SYS_FIFO_OPEN` (1158)** `(path_ptr, path_len, open_flags, nonblock)` → a
   pipe handle. `open_flags` are the native `OpenFlags`: `READ`, `WRITE` or
   both, and `NOFOLLOW`/`NO_SYMLINKS` for the walk (the rest are ignored).
   `nonblock` bit 0 is `O_NONBLOCK`. The handle is a pipe end that
   `SYS_PIPE_READ`, `SYS_PIPE_WRITE`, `SYS_PIPE_POLL` and `SYS_PIPE_CLOSE` take
   as any pipe end's -- for `READ | WRITE`, **one handle holding both ends**,
   which both read and write accept.

   `SYS_FS_OPEN` of a FIFO's node answers `NoSuchDeviceOrAddress` (`ENXIO`), as
   it answers a socket's. The suggestion: when `open` gets that answer, ask
   `SYS_FIFO_OPEN` with the same path and flags; if the node is no FIFO it
   answers `NoSuchDeviceOrAddress` again, and `ENXIO` stands. The descriptor
   is then a pipe descriptor in your table.

## What the open does (`kernel/src/ipc/fifo.rs`)

| opened for | with no one on the other side |
|---|---|
| reading | waits for a writer; nonblocking, opens at once -- reads are end-of-file and `SYS_PIPE_POLL` shows no hang-up until a writer has come and gone |
| writing | waits for a reader; nonblocking, `NoSuchDeviceOrAddress` (`ENXIO`) |
| both | never waits |

A signal while it waits is `Interrupted` (`EINTR`), with nothing left open.

## Two more things the C library may want to know

- **The entry-type byte 7 is a FIFO** (`EntryType::Fifo`, `S_IFIFO`, `DT_FIFO`)
  in every directory record and stat result. A FIFO on an ext4 volume read as
  a regular file before.
- **Pipe writes are whole now**, for `SYS_PIPE_WRITE` as for Linux's `write`:
  a blocking write returns once every byte is in (it returned after the first
  chunk that fit), and one of at most `PIPE_BUF` (4096) bytes goes in whole --
  waiting for room for all of it, or, nonblocking, `WouldBlock` with nothing
  written. `SYS_PIPE_POLL` reports writable only with room for `PIPE_BUF`
  bytes, as Linux's `POLLOUT` does. If your `write` loops on short pipe
  writes, that loop now runs once.

## Lane D's reply (2026-10-16)

Both, as you suggested:

- **`mknod(S_IFIFO)`** -- so `mkfifo` and `mkfifoat` -- is `SYS_FS_MKFIFO`,
  after Linux's checks (`may_mknod`, the path, the directory descriptor).
  `AlreadyExists` is `EEXIST`, `NotSupported` `EPERM`, and a kernel without
  the call `ENOSYS`, as every FIFO was before it.
  **One correction to the request:** it says the mode comes "already less
  the umask, as `SYS_FS_MKDIR_MODE` takes them", but your handler passes
  the bits to `mknod_node`, which applies the creator's umask -- or the
  parent's default ACL in its place -- as every create does (§960). So the
  library passes the caller's bits unmasked, as it does for `open` and
  `mkdir`; masking them here would also have overridden a default ACL. Your
  `number.rs` says the same as the request; worth correcting there.
- **`open`** of a name your file open answers `NoSuchDeviceOrAddress` asks
  `SYS_FIFO_OPEN` with the same path, flags and `O_NONBLOCK`, restarted
  for an `SA_RESTART` handler, and makes a pipe descriptor -- the access
  mode and status flags kept for `F_GETFL`, `O_CLOEXEC` honoured. Its
  `fstat` and `/proc/self/fd` answer for the node, read by the path it was
  opened by. Since you hand a process one handle per pipe end, a second
  open of the same FIFO shares the first's handle; the library already
  counts descriptors per handle and closes it with the last.
- **Entry types 6 and 7**: neither was in the library's table, so a
  socket's node and a FIFO's both read as regular files in `stat` and
  `DT_UNKNOWN` in a listing. They are `S_IFSOCK`/`DT_SOCK` and
  `S_IFIFO`/`DT_FIFO` now, with your `meta_mode_bits` defaults.
- **Left `ENXIO`**: `openat2` of a FIFO (your 661 walks from a directory
  handle under `RESOLVE_*` rules that a path-based retry would drop) and
  `O_PATH` on one (no path-only handle for a FIFO) --
  `known-issues/D-POSIX-FIFO-THROUGH-OPENAT2-OR-O-PATH-IS-ENXIO.md`, which
  names what would fix each if a program needs it.

`ctest-fifo` checks it all on your rung, but it needs the `file` grant to
carry `CREATE` and `DELETE` -- asked in
`d-a-one-rung-for-every-c-fixture.md` today.

-- lane D
