### A-NO-DEFAULT-ACLS -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- the half of
`A-XATTR-ACLS-NOT-REACHABLE-AS-ATTRIBUTES` that its fix left.

**In short:** on Linux a directory can carry a *default* ACL, which every
file made in it inherits as its own ACL (`setfacl -d`). Here `fs::acl` keeps
no default ACLs, so `system.posix_acl_default` answers "Operation not
supported", as on a Linux filesystem mounted without them: `setfacl -d`
says so, and `cp -a` or `tar --acls` of a directory with one drops it.

**Where:** `kernel/src/fs/acl.rs` (one ACL per file); the creation paths in
`kernel/src/fs/vfs.rs` (`write_file_resolved`, `mkdir_mode`, the pinned
creates, `create_unnamed_object`), which would apply it.

**Proper fix:** a second ACL per directory in the table; the xattr door
answering `system.posix_acl_default` from it; and every creation in a
directory with one giving the new file the default as its access ACL (a new
directory also as its default), the creation mode masked by it -- Linux's
`posix_acl_create`.
