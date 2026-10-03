### [D] B-D-XATTR-SIZES-AND-BUFFERS-WERE-NOT-LINUXS — 2026-09-26 — FIXED 2026-09-26 (libc); kernel half requested

**Where:** `posix/src/xattr.rs` -- every `*xattr` call. The kernel half is
lane A's: `sys_fs_get_xattr`, `sys_fs_set_xattr` and `sys_fs_list_xattrs`
(`kernel/src/syscall/handlers.rs`) and the VFS below them.

**In short:** Extended attributes are small named values kept with a file --
a comment, a security label, an access list. Tools that copy files with them
(`cp -a`, `rsync -X`, `tar --xattrs`) read the list and every value, and
decide from the error what to do when something does not fit. Ours answered
the edges differently from Linux: an empty or too-long name, a too-large
value and a NULL buffer were all "invalid argument", where Linux says "out of
range", "too big", "bad address" -- or, for a NULL buffer, often nothing at
all, since Linux complains about a buffer only when it has something to put
in it.

**What was wrong, against Linux 6.6 (fs/xattr.c):**

| | was | Linux, and now |
|---|---|---|
| an empty name | handed to the filesystem | `ERANGE` -- `strncpy_from_user` copied 0 bytes |
| a name of 256 bytes or more | `EINVAL` | `ERANGE` |
| a setter's value over 64 KiB | `EINVAL` | `E2BIG`, before the value is read |
| a setter's NULL value with a size | `EINVAL` | `EFAULT`, after the name and the size |
| a getter's or lister's NULL buffer with a size | `EINVAL`, before the lookup | the lookup's error, `ERANGE` if the result does not fit, 0 if it is empty; `EFAULT` only for a result that would be copied |
| a getter's or lister's size over 64 KiB | the buffer checked to its full stated size | 64 KiB (`XATTR_SIZE_MAX`, `XATTR_LIST_MAX`) |

**Still the kernel's, and asked of lane A** (`requests/d-a-xattr-answers-only-the-filesystem-can-give.md`):

- a getter's buffer that is too small is written up to its size before the
  `ERANGE`; Linux leaves it as it was (`listxattr` already writes nothing);
- a non-NULL buffer is checked against its stated size before the lookup,
  so a bad one is `EFAULT` where Linux gives the lookup's answer;
- a name in no namespace (`foo`, not `user.foo`) is stored, where Linux
  refuses it with `EOPNOTSUPP`, and a bare prefix (`user.`) with `EINVAL`;
- `trusted.*` is not gated on `CAP_SYS_ADMIN`, nor `user.*` kept to regular
  files and directories, as Linux's `xattr_permission` does.
