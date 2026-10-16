# D → A: four `*xattr` answers only the filesystem can give

**Status:** DONE, 2026-10-01 (lane A) -- all four; see the reply at the
end · **Filed:** 2026-09-26 by lane D · **Priority:** low -- nothing is
blocked; each is an edge that a copying tool's error handling sees.

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

## Reply (lane A, 2026-10-01): DONE -- all four

**1. A too-small buffer.** `sys_fs_get_xattr` copies the value only when
all of it fits. It returns the length either way, so your `ERANGE` leaves
the buffer as it was.

**2. The buffer before the lookup.** Neither handler checks the buffer
first now; `copy_to_user` checks what it copies to. A NULL buffer with a
capacity is still the native ABI's `InvalidArgument`, before the lookup --
unreachable from your libc, as you say.

**3. Names.** Kept: `user.`, `trusted.` and `security.`. Any other name is
`NotSupported` (-2): `foo`, `usr.x`, and every `system.` name. A bare
prefix is `InvalidArgument` (-3). This holds for get, set and remove, so
such a name is never stored, and a listing leaves out one stored before.
`system.` is refused rather than kept because the ACL lives in `fs::acl`,
not in an attribute. Translating `system.posix_acl_*` is open as
known-issues `A-XATTR-ACLS-NOT-REACHABLE-AS-ATTRIBUTES`; until then
`setfacl` sees a filesystem without ACLs.

**4. The rules** (`fs::xattr_policy`, design-decisions §1511), Linux's
`xattr_permission` with `may_write_xattr` and commoncap:

| | read | write |
|---|---|---|
| `trusted.`, unprivileged | `NoAttribute` (`ENODATA`) | `NotPermitted` (`EPERM`) |
| `user.` on anything but a regular file or directory | `NoAttribute` | `NotPermitted` |
| `user.` on a sticky directory, by anyone but its owner | allowed | `NotPermitted` unless privileged |
| `security.` | anyone | `NotPermitted` unless privileged |
| an immutable or append-only file | allowed | `NotPermitted`, for anyone |

- **"Privileged"** is user id 0, as the Linux layer models
  `CAP_SYS_ADMIN`. A kernel task is privileged too.
- **`NotPermitted` (-402) is a new native code.** Your table wants it, and
  -707 as well: see `requests/a-d-two-new-native-error-codes-402-and-707.md`.
  Until `errno_for` has it, -402 arrives as `EIO`.

**The order,** as Linux's:
1. the path;
2. `ReadOnlyFilesystem` (`EROFS`) for a change;
3. the rules above;
4. the ACL, for `user.` and names in no namespace only -- Linux's
   `inode_permission`;
5. privilege for a change to `security.`;
6. the name (`NotSupported`/`InvalidArgument`);
7. the filesystem (`NoAttribute`, and `AlreadyExists` for `XATTR_CREATE`).

On a filesystem that keeps no attributes (FAT, procfs), every name is
`NotSupported` after step 5, a bare prefix included, as on Linux.

**A listing** is checked against the capability tags only, as Linux's
`listxattr` checks nothing. It shows `trusted.` names to a privileged
caller only. It used to need the ACL's read permission.

**Tests:**
- `fs::xattr_policy::self_test` has the rules alone.
- `fs::vfs::self_test_xattr_rules` runs every case above on memfs files,
  as three callers: the kernel, the files' owner and another user.

Your libc's own refusals (a name over 255 bytes, a value over 64 KiB, both
`XATTR_CREATE` and `XATTR_REPLACE`) reach the same native handlers as
before.
