### A-NO-DEFAULT-ACLS -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- fixed on `lane-a-wip` 2026-10-02 (design-decisions
§1531); to be stamped FIXED and moved to `known-issues-resolved/` once a boot
has tested it on `main`.

**Resolution.** `fs::acl` keeps directories' default ACLs in a second table
(`DEFAULTS`, forgotten with its directory as the access ACLs are), the
xattr door answers `system.posix_acl_default` from it -- a directory's
only: `EACCES` to set one on anything else, `ENODATA` to read one -- and
every route that makes a node (`write_file`, `open(O_CREAT)`, `mkdir`,
`mkdirat` through a held directory, a socket node, an unnamed file) gives
it what Linux's `posix_acl_create` gives: in a directory with a default
ACL, its mode and access ACL from the default narrowed to the mode asked
for, a new directory the default again, and no umask; elsewhere the mode
less the creator's umask. The umask moved into the kernel for that: the
Linux shim no longer applies it, and lane D is asked to stop libc doing so
(`requests/a-d-the-umask-is-the-kernels-to-apply-now.md`), since a mode
the umask has already narrowed cannot be widened again by a default ACL.
`vfs::self_test_create_modes` and `acl::self_test_inherit` hold it.

**In short:** on Linux a directory can carry a *default* ACL, which every
file made in it inherits as its own ACL (`setfacl -d`). Here `fs::acl` kept
no default ACLs, so `system.posix_acl_default` answered "Operation not
supported", as on a Linux filesystem mounted without them: `setfacl -d`
said so, and `cp -a` or `tar --acls` of a directory with one dropped it.

**Where:** `kernel/src/fs/acl.rs` (one ACL per file); the creation paths in
`kernel/src/fs/vfs.rs` (`write_file_resolved`, `mkdir_mode`, the pinned
creates, `create_unnamed_object`), which would apply it.

**Proper fix:** a second ACL per directory in the table; the xattr door
answering `system.posix_acl_default` from it; and every creation in a
directory with one giving the new file the default as its access ACL (a new
directory also as its default), the creation mode masked by it -- Linux's
`posix_acl_create`.
