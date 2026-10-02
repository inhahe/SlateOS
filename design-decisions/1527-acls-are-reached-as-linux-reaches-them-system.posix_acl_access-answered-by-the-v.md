## 1527. ACLs are reached as Linux reaches them: `system.posix_acl_access`, answered by the VFS from the ACL table the permission check reads

**Date:** 2026-10-02 · **Decided by:** Claude (operator-approved scope: §978, "the per-file metadata tables: `acl`, `fcomment`, `queryable` and `tags` ... get syscall-layer doors") · **Lane:** A

**In short:** an ACL -- "user 2000 may also read this file" -- was enforced by
the kernel but could be set only by a command typed into the kernel's own
shell; `setfacl` got "Operation not supported". On Linux, `setfacl` and
`getfacl` read and write a file's ACL as the extended attribute
`system.posix_acl_access`. Now that attribute works here too: the VFS answers
it from the ACL table the permission check reads and writes it into that
table, so what `setfacl` sets is what refuses. No new system call was needed;
the twelve xattr calls already reach the VFS.

**What it does, as Linux does it:**
- The value is Linux's layout (a version-2 header, then a `{tag, perm, id}`
  entry each), parsed and checked as `posix_acl_valid` checks it: the
  entries in order, the three base entries once, a mask wherever there is a
  named entry. Anything else is `EINVAL`.
- Only the file's owner or root may set or remove one (`EPERM`), and no one
  on an immutable or append-only file.
- Setting one sets the file's mode to the one the ACL means; an ACL that says
  no more than a mode is kept as the mode alone, as Linux keeps no ACL then.
- A `chmod` is written into an existing ACL -- owner, mask (or owning
  group) and others -- so the check and `getfacl` both see the new mode.
- `listxattr` names it when the file has one; reading a file without one is
  `ENODATA`.
- By path and through a descriptor alike, under the filesystem's lock, by
  the file's identity.

| Where the door is | For | Against |
|---|---|---|
| **The xattr name, in the VFS (chosen -- Linux's interface)** | `setfacl`/`getfacl` and every library that reads ACLs work unchanged; nothing new in either ABI | the VFS's xattr calls carry a special case |
| A new `SYS_ACL_SET`/`GET` | an explicit door | nothing on Linux calls it; lane D would have to teach libacl a second route |

**Not done:**
- Default ACLs (`system.posix_acl_default`, which new files in a directory
  inherit): the table keeps none, so the name stays `EOPNOTSUPP`, as on a
  filesystem without them, and `setfacl -d` says so.
- Persistence: an ACL lives in memory and is gone at reboot. Its home is
  the filesystem's own xattr (known-issues `A-PER-FILE-STATE-OUTLIVED-ITS-FILE`).
- Clearing the set-group-ID bit when an ACL is set by someone outside the
  file's group, which Linux's `posix_acl_update_mode` does.
