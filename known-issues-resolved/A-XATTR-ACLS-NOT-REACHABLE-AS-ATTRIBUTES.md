### A-XATTR-ACLS-NOT-REACHABLE-AS-ATTRIBUTES -- 2026-10-01 -- FIXED (lane A)

**Status:** FIXED on lane-a-wip 2026-10-02, awaiting a boot, as the fix below describes it (design-decisions §1527): the VFS answers `system.posix_acl_access` from `fs::acl` and writes it there, with the mode rules; `vfs::self_test_acl_door` covers it. The default ACL is its own entry now, `A-NO-DEFAULT-ACLS`.

**In short:** on Linux, a file's POSIX ACL is also an extended attribute,
`system.posix_acl_access` (and `system.posix_acl_default` for a
directory's default ACL). Copying tools carry ACLs that way: `cp -a`,
`tar --acls`, `rsync -A`, and `getfacl`/`setfacl` themselves. Here the
ACL lives in `fs::acl` and the attribute names are refused (`EOPNOTSUPP`,
`fs::xattr_policy`). So those tools see a filesystem without ACL support,
and a copy loses the ACL quietly.

**Where:** `kernel/src/fs/xattr_policy.rs` (`resolve`) and
`kernel/src/fs/vfs.rs` (`Vfs::xattr_on`).

**The fix:** translate in the VFS between Linux's binary form and
`fs::acl`:
- the form is `posix_acl_xattr_header` (version 2), then 8-byte entries of
  tag, permission and id;
- setting the access ACL updates the file's mode, as Linux's
  `posix_acl_update_mode` does;
- an ACL the mode alone expresses is stored as the mode only;
- the default ACL needs `fs::acl` to keep one per directory.
