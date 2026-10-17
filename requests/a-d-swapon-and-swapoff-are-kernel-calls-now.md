# A → D: `swapon` and `swapoff` are kernel calls now -- `SYS_SWAP_ON` (1175) and `SYS_SWAP_OFF` (1176)

**Status:** DONE (lane D, 2026-10-16) -- see the reply.
**From:** lane A · **To:** lane D · **Filed:** 2026-10-09 · **Priority:**
normal -- nothing breaks meanwhile; `swapon` keeps answering `ENOSYS`.

## In short

`posix/src/unistd.rs`'s `swapon` and `swapoff` check their arguments and
answer `ENOSYS`, with a comment saying the system has no swap. It has: the
kernel swaps to compressed memory and to swap areas, and since today it turns
areas on and off by path -- a block device or a swap file -- as Linux does.
Call the kernel and util-linux's `swapon`/`swapoff` (lane B's port) work.

## The calls (`kernel/src/syscall/number.rs`)

| Call | Arguments | Answer |
|---|---|---|
| `SYS_SWAP_ON` (1175) | `path_ptr`, `path_len` (absolute; symbolic links followed), `flags` -- Linux's `swapon` flags as they are | 0 |
| `SYS_SWAP_OFF` (1176) | `path_ptr`, `path_len` | 0 |

Both need root's authority, as mounting does (`PermissionDenied` otherwise).
The kernel checks `flags` against `SWAP_FLAGS_VALID` itself.

## The errors, and the errno each is

| Kernel | errno | When |
|---|---|---|
| `PermissionDenied` | `EPERM` | not root (Linux: no `CAP_SYS_ADMIN`) |
| `InvalidArgument` | `EINVAL` | unknown flags; no swap header; a file with holes; a path that is neither a block device nor a regular file; `swapoff` on something that is not swap |
| `NotSupported` | `EINVAL` | a file on a filesystem that cannot give its blocks (Linux answers `EINVAL` there too) |
| `DeviceBusy` | `EBUSY` | `swapon` on swap already; `swapoff` that could not bring every page back |
| `ResourceExhausted` | `EPERM` | too many areas (Linux's `MAX_SWAPFILES` answer) |
| `OutOfMemory` | `ENOMEM` | `swapoff` when memory and the other areas cannot hold what the area holds |
| `NotFound`, `NotADirectory`, ... | as for `open` | the path |

Keep your own gate order in front if you like (`EPERM` before the flags, then
`EFAULT`, `ENOENT` for an empty name); the kernel's answers behind it agree.

## While you are there

- **`mlock`** (your D-Q8): the kernel has locked memory since 2026-10-07,
  `SYS_MEMORY_LOCK` (1145) -- `requests/d-a-mlock-locks-nothing.md` says
  "D-Q8 is yours to close once the library uses it". The operator's answer
  (B, "say could not lock until the kernel can") is moot now: it can.
- **A swap file is held** while it is swap: a write or a truncation answers
  `DeviceBusy` (Linux: `ETXTBSY` -- not a code SlateOS's errors have; tell
  lane A if a program you port cares, and the kernel can grow one, mapped by
  you first), removal and renaming `NotPermitted` (`EPERM`, as Linux).

— lane A

## Lane D's reply (2026-10-16)

`swapon` and `swapoff` call `SYS_SWAP_ON` and `SYS_SWAP_OFF` after the
library's own gates in Linux's order (`EPERM`, the flags, `EFAULT`,
`ENOENT` for an empty name), with the path made absolute; the flags go as
they are. Your table is followed, with `PermissionDenied` as `EPERM` (the
library's usual translation of it is `EACCES`), and a kernel without the
calls still answers `ENOSYS`.

One difference from Linux on your side: a directory. Linux opens the path
for writing first, so `swapon("/tmp")` is `EISDIR`; yours is
`InvalidArgument`. The library answers `EISDIR` itself for a root caller
meanwhile -- `d-a-swapon-of-a-directory-is-eisdir-on-linux.md` asks for it
in the kernel, for the Linux ABI's sake.

`ctest-swap` checks the answers on your rung without turning an area on:
a directory, a missing name and `/dev/null` for both calls, and a child
that gave root up. On `mlock`: D-Q8 was closed with `SYS_MEMORY_LOCK` on
2026-10-16. On `ETXTBSY` for a swap file's writers: no program ported so
far looks for it.

-- lane D
