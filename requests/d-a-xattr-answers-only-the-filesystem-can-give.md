# D → A: four `*xattr` answers only the filesystem can give

**Status:** OPEN · **Filed:** 2026-09-26 by lane D · **Priority:** low --
nothing is blocked; each is an edge that a copying tool's error handling
sees.

## In short

The libc's `getxattr` family now makes Linux 6.6's argument checks itself,
in Linux's order (`posix/src/xattr.rs`, the thirty-eighth NULL-pointer pass,
`B-D-XATTR-SIZES-AND-BUFFERS-WERE-NOT-LINUXS` in `known-issues.md`): the
name's `ERANGE`s, a value's `E2BIG` and `EFAULT`, and a NULL buffer answered
from a length query. What is left needs the filesystem's knowledge. Each
item cites `kernel/src/syscall/handlers.rs` or the VFS as they stood on
2026-09-26, and Linux 6.6's `fs/xattr.c`.

## 1. A buffer that is too small is written before `ERANGE`

`sys_fs_get_xattr` copies `min(len, capacity)` bytes and returns the whole
length, which the libc turns into `ERANGE`. Linux's filesystems answer
`-ERANGE` into a kernel buffer, and `do_getxattr` copies out only on success,
so the caller's buffer is as it was. A caller that retries with a bigger
buffer cannot tell; one that keeps a fallback or a sentinel in the buffer
can. `sys_fs_list_xattrs` already writes nothing unless the whole list fits
-- the ask is the same for a value: copy only when `len <= capacity`.

## 2. The buffer is checked before the lookup

Both handlers `validate_user_write(buf, capacity)` before looking anything
up, so a bad non-NULL buffer is `EFAULT` where Linux answers from the lookup
(`ENODATA` for a missing attribute, 0 for an empty one) and touches the
buffer only in `copy_to_user`, for the bytes it copies. `copy_to_user`
already re-validates the destination, as the handler's comment says, so the
early check can go. (The libc now passes a NULL buffer as capacity 0, so the
handlers' `arg == 0 && capacity > 0` arms are unreachable from it.)

## 3. Names in no namespace are stored

memfs stores any key, and ext4 files an unprefixed one under index 0.
Linux's `xattr_resolve_name` refuses a name no handler claims (`foo`,
`usr.x`) with `EOPNOTSUPP`, and a bare prefix (`user.`) with `EINVAL`, for
get, set and remove alike -- so such names are never listed either. Linux's
ext4 has handlers for `user.`, `trusted.`, `security.` and `system.` (the
POSIX ACL names).

## 4. The `trusted.*` and `user.*` rules

Linux's `xattr_permission` gates `trusted.*` on `CAP_SYS_ADMIN` -- `EPERM`
to write, `ENODATA` to read, so the attribute looks absent -- and allows
`user.*` only on regular files and directories (`EPERM` to write and
`ENODATA` to read anything else), with a sticky directory's attributes
writable only by its owner. I found neither in the VFS (`check_path_access`
is the ordinary permission check); if they are somewhere else, say so and
this item goes.

## Not asked

The native ABI's own refusals, which the libc now answers before they are
reached -- a name over 255 bytes, a value over 64 KiB or NULL with a size,
and `XATTR_CREATE` with `XATTR_REPLACE` (answered from a probe,
design-decisions §661) all as `EINVAL` -- are the native ABI's business and
can stay as they are.
