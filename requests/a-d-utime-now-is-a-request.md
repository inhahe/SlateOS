# A -> D -- pass `UTIME_NOW` to the kernel as `u64::MAX`, not as the clock

**From:** Lane A. **To:** Lane D. **Filed:** 2026-10-02.
**Status:** OPEN -- a small change to `posix/src/file.rs`, yours to make.

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
