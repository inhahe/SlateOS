## D-POSIX-FILE-SEALS-DIFFER-FROM-LINUX-IN-THREE-PLACES — `fcntl`'s file seals are Linux's except where the kernel's seals are wider, stricter, or missing one (lane D, 2026-10-07)
**Status:** OPEN -- known and deliberate in two places, a gap in the third

**In short:** a program can seal a file -- forbid, for the rest of its life,
that it be written, shrunk, grown or sealed further -- with `fcntl(fd,
F_ADD_SEALS, ...)`. Since 2026-10-07 that works here, through lane A's
`SYS_FS_ADD_SEALS` and `SYS_FS_GET_SEALS`, and a `memfd` starts sealed as
Linux makes one. Three things differ from Linux; programs written for Linux
are unaffected by the first two and refused cleanly by the third.

| | Linux | Here | Why |
|---|---|---|---|
| Which files | a `memfd` (or `tmpfs` file) made with `MFD_ALLOW_SEALING`; any other file is `EINVAL` | any file the caller has open for writing | the kernel's seals are the VFS's, kept by file identity on every filesystem (lane A's design-decisions 1526) |
| `F_SEAL_WRITE` and `ftruncate` | allowed: a sealed memfd's size is `F_SEAL_SHRINK`'s and `F_SEAL_GROW`'s business | refused (`EPERM`): the contents are fixed outright | the same decision: seals on any file are read as "the contents are fixed" |
| `F_SEAL_FUTURE_WRITE` (Linux 5.1) | refuses writable mappings made from now on, keeping existing ones | `EINVAL`: the kernel has no such seal | a gap: it needs mapping-aware rules the kernel's VFS seals do not have |

**Where:** `posix/src/fcntl_ops.rs`, `add_seals` and `get_seals`, pass the
bits straight through; the kernel decides. `posix/src/mman.rs`,
`initial_memfd_seals`, gives a new memfd Linux's starting seals.

**Reproduce:** the second and third rows: `memfd_create("x",
MFD_ALLOW_SEALING)`, `fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE)`, `ftruncate(fd,
0)` is 0 on Linux and `EPERM` here; `fcntl(fd, F_ADD_SEALS,
F_SEAL_FUTURE_WRITE)` is 0 on Linux and `EINVAL` here.
`services/ctest-seals` checks only what the two agree on, and says so.

**What would change it:** the first two rows are settled by lane A's
decision and would change only with it. The third needs `SYS_FS_ADD_SEALS`
to take 0x10 with Linux's meaning; the library passes the bit through
already, so it would need no change here.
