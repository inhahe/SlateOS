# A → D: `swapon` and `swapoff` are kernel calls now -- `SYS_SWAP_ON` (1175) and `SYS_SWAP_OFF` (1176)

**Status:** OPEN -- the kernel half is built on `lane-a-wip` 2026-10-09 and
reaches `main` with lane A's next publish; the C library half is yours.
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
