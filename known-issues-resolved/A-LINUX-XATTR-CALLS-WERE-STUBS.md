### A-LINUX-XATTR-CALLS-WERE-STUBS -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** the twelve Linux calls on extended attributes (`setxattr`,
`getxattr`, `listxattr`, `removexattr`, each with an `l` and an `f` form)
were written before the filesystems kept attributes. They checked their
arguments, then answered "not supported" to a set or remove, "no such
attribute" to a get, and an empty list -- whatever the file held. A Linux
program saw no attributes on any file; `cp -a`, `tar --xattrs` and
`getfattr` all lost them.

**Fixed:** the calls are real, over the VFS and `fs::xattr_policy`, in
Linux 6.6's order:
- `setxattr` copies its flags, name, size and value before the path;
  the others look up the path first.
- A name is `ERANGE` empty or past 255 bytes; a value past 64 KiB is
  `E2BIG`.
- A reader's buffer is written only when the answer fits: `ERANGE`
  otherwise, or `E2BIG` when even 64 KiB, which a larger size is cut to,
  would not hold it.
- `XATTR_CREATE` and `XATTR_REPLACE` together are refused either way, as
  Linux's filesystems refuse them (`XattrSetMode::Neither`).
- The descriptor calls act through the file a descriptor holds, by its
  identity (`HandleFile`, `Vfs::object_*_xattr`), across a rename or an
  unlink. The ACL is asked by that identity too (`check_object_access`).

Tests: the argument checks in `syscall::linux::self_test` (no file); the
calls on `/tmp` files, the descriptor's file across a rename and an
unlink, and objects on no filesystem in `self_test_xattr_calls`.
