## D-POSIX-FIFO-THROUGH-OPENAT2-OR-O-PATH-IS-ENXIO — `openat2` of a named pipe, and `O_PATH` on one, answer `ENXIO` where Linux opens it (lane D, 2026-10-16) — OPEN
**Status:** OPEN

**In short:** a named pipe (FIFO) opens through `open` and `openat` since
2026-10-16: the kernel's file open answers "nothing to open here" (`ENXIO`)
for a FIFO's node, and the C library then asks lane A's FIFO open
(`SYS_FIFO_OPEN`) by the same path. Two opens do not take that second step,
and a program sees `ENXIO` where Linux gives it a descriptor:

- **`openat2`** (Linux's `openat2(2)`, with its `RESOLVE_*` rules). Its walk
  is the kernel's `SYS_FS_OPENAT2`, from a directory handle and under the
  rules asked; the FIFO open takes a path and walks it again from the root,
  so asking it would drop the rules -- `RESOLVE_BENEATH` above all, whose
  point is that the walk cannot leave a directory.
- **`O_PATH`**, which on Linux opens any node, a FIFO's included, without
  waiting and without opening the pipe: a descriptor only for naming it.
  The kernel has no path-only handle for a FIFO.

Both are rare -- `openat2` is new and mostly used by container runtimes,
`O_PATH` on a FIFO mostly by `fstatat`-style code that can stat the path
instead -- and neither loses data: the open fails, it does not open the
wrong thing.

**Where:** `posix/src/file.rs` -- `open_fifo` (the second step),
`open_resolved` (which skips it for `O_PATH`), `openat2_forward_inner` (which never
takes it).

**The proper fix:** in lane A's kernel, `SYS_FIFO_OPEN` taking a directory
handle and the `RESOLVE_*` rules as `SYS_FS_OPENAT2` does (or
`SYS_FS_OPENAT2` opening a FIFO itself), and a path-only handle for a FIFO's
node for `O_PATH`; then the library calls them. A request to lane A when a
program needs either.
