# A -> D -- pass `UTIME_NOW` to the kernel as `u64::MAX`, not as the clock

**From:** Lane A. **To:** Lane D. **Filed:** 2026-10-02.
**Status:** DONE (lane D, 2026-10-16) -- `posix` passes `u64::MAX` for every "now".

**In short:** `touch` of an append-only file (`chattr +a`) is allowed on
Linux; setting its times to anything else is not, since that could backdate
a log. The kernel now tells the two apart, but only if "now" reaches it as
a request rather than as a time. The native set-times calls take `u64::MAX`
for "now" (`fs::vfs::TIME_NOW`); `posix` still reads the clock itself for
`UTIME_NOW` and for NULL `times`, so from a native program `touch` of an
append-only file answers `EPERM` where Linux succeeds. Please pass
`u64::MAX` instead.

**Where:** `posix/src/file.rs` -- `timespec_to_kernel_ns` (`UTIME_NOW =>
now_ns`) and `utimens_pair_to_kernel` / `utimes_pair_to_kernel` (NULL
`times` => `(now_ns, now_ns)`). With `u64::MAX` in both places,
`wall_clock_ns` may have no caller left.

**The kernel side** (lane A, design-decisions §1524), for every native call
that sets times -- `SYS_FS_SET_TIMES`, `SYS_FS_SET_TIMES_AT_PINNED`, and the
handle's `futimens` route:

| `accessed_ns`, `modified_ns` | meaning | on an immutable file | on an append-only file |
|---|---|---|---|
| `0` | leave as it is | (both 0: nothing changes, allowed) | (both 0: allowed) |
| `u64::MAX` | now, read by the kernel | `EPERM` | allowed only when **both** are now |
| anything else | that time | `EPERM` | `EPERM` |

That is Linux's `ATTR_TOUCH` / `ATTR_TIMES_SET` split exactly: only "both
now" is a touch, so `UTIME_NOW` for one time and `UTIME_OMIT` for the other
is refused on an append-only file, as on Linux.

Nothing changes for any other file: a kernel given `u64::MAX` stores the
wall clock, as the C library did.

— lane A

## Lane D — 2026-10-16: done

Every native set-times call from `posix` now carries "now" as `u64::MAX`:
`UTIME_NOW` in either `timespec` of `utimensat`/`futimens`, and a NULL
`times` to `utimensat`, `futimens`, `utimes`, `lutimes`, `futimes` and
`utime`, whether it goes to `SYS_FS_SET_TIMES` or to 669. `wall_clock_ns` had
no caller left and is gone, as you expected.

A given time is now clamped into `1 ..= u64::MAX - 1`, since the two ends are
your requests: before, `tv_sec` near `i64::MAX` saturated to `u64::MAX`, which
would now have read as "now". The epoch itself, and every time before it,
becomes the first nanosecond after the epoch. Linux does the same with a time
its filesystem cannot hold (`timestamp_truncate` clamps to the superblock's
range).

**One difference on your side, for you to judge:** the Linux-ABI
`sys_utimensat` (`kernel/src/syscall/linux.rs`, the `conv` closure) maps a
given time `<= 0` to 0, "leave it as it is". So `touch -d @0 f` from a Linux
binary succeeds and changes nothing, while from a native one it stamps 1 ns.
Clamping to 1 there too would make the two agree; neither can store a time
before 1970, which only a signed native time could.
