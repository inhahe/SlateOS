### A-XATTR-NAMES-AND-PERMISSIONS-WERE-NOT-LINUXS -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** an extended attribute is a named piece of data kept beside a
file's contents. The kernel stored any name, for anyone, on anything:
- an ordinary program could write `trusted.` attributes, which Linux
  keeps for the administrator;
- `user.` attributes went onto links and devices, where Linux refuses
  them;
- names Linux would not accept were stored -- `foo` with no namespace, the
  bare prefix `user.`, and `system.posix_acl_access`, an ACL that governed
  nothing.

A program got a different answer than Linux gives at every one of these
edges. Lane D asked for the four answers only the kernel can give
(`requests/d-a-xattr-answers-only-the-filesystem-can-give.md`; their side
is `B-D-XATTR-SIZES-AND-BUFFERS-WERE-NOT-LINUXS`).

**Fixed** (design-decisions §1511):
- `fs::xattr_policy` holds Linux 6.6's rules. Every VFS xattr call goes
  through `Vfs::xattr_on`, which decides them in Linux's order, under the
  same hold of the filesystem's lock as the change.
- `KernelError::NotPermitted` (-402, `EPERM`) is new, for the refusals
  Linux answers `EPERM`. The libc's table wants it
  (`requests/a-d-two-new-native-error-codes-402-and-707.md`).
- `sys_fs_get_xattr` copies a value only when all of it fits, so a
  too-small buffer is left as it was. Neither getter nor lister checks the
  buffer before the lookup.
- ext4 reports its two POSIX ACL indexes as `system.posix_acl_access` and
  `system.posix_acl_default`, where it reported a name of no bytes.
- Tests: `fs::xattr_policy::self_test` has the rules alone;
  `fs::vfs::self_test_xattr_rules` runs them on memfs files as three
  callers; the ext4 key round-trip has the ACL names. `fs::handle`'s
  no-follow section uses a `trusted.` name now, since `user.` is refused
  on a link.

**Not changed:** a name stored before this under no namespace is left on
disk, neither listed nor read.
