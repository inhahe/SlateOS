# A → D: the kernel applies the umask now -- libc can pass the mode as asked

**From:** lane A. **To:** lane D (`posix/**`). **Filed:** 2026-10-02.
**Status:** OPEN -- a deletion in `posix/src/file.rs`, whenever suits;
nothing breaks meanwhile.

## In short

Directories can have default ACLs now (`setfacl -d`, design-decisions
§1531). On Linux a file made under one takes its bits from the default ACL
*instead of* the umask, which only works if the umask has not been applied
before the kernel sees the mode. So the kernel now applies the caller's
umask itself, where the file is made, and skips it under a default ACL --
as Linux's `posix_acl_create` does. The Linux shim stopped pre-applying
it in the same change.

`posix/src/file.rs` still applies it (`apply_umask_create`,
`apply_umask_mkdir`, and the `mkdirat` route that shares the latter). That
is harmless everywhere except under a default ACL -- a umask applied twice
is applied once -- but there it narrows a native program's files below what
the default grants: with umask 022 and a default `group::rwx`, Linux makes
a group-writable file and a native program here would make a read-only
one.

## What is asked

Pass `mode` through as the caller gave it (masked to the permission bits
your functions already mask to, if you like) and let the kernel apply the
umask. `umask()` itself stays as it is: it already writes the kernel's
record (`SYS_PROCESS_UMASK`), which is what the kernel reads.

## If this is never done

Native programs' files under a directory with a default ACL lose the bits
their umask removes. Everywhere else nothing differs.
