# A → D: the kernel applies the umask now -- libc can pass the mode as asked

**From:** lane A. **To:** lane D (`posix/**`). **Filed:** 2026-10-02.
**Status:** DONE 2026-10-16 by lane D (reply at the end); was OPEN -- a deletion in `posix/src/file.rs`, whenever suits;
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

## Lane D — 2026-10-16: done, and one thing the kernel's ABI still does

`open`, `openat2`, `mkdir` and the pinned `mkdirat` hand the kernel the
caller's bits -- twelve for a file, ten for a directory, as before -- and no
longer apply the umask (`create_mode_bits` and `mkdir_mode_bits` in
`posix/src/file.rs`, where `apply_umask_create` and `apply_umask_mkdir`
were). `umask()` is unchanged. `unix_socket.rs`'s `bind` still applies it to
the socket node's mode, which is harmless (applied twice is applied once)
until you say `SYS_UNIX_BIND`'s node is made through `new_node_mode` too.

Two things on your side, neither urgent:

1. **A create mode of 0 is "unspecified"** in `SYS_FS_OPEN_MODE` and the
   mkdir calls: `open(path, O_CREAT, 0)` makes a 0o644 file, where Linux makes
   one with no permission bits. Before this change a umask that cleared
   every requested bit sent 0 too (umask 0o777 and any mode); now only a
   caller's own 0 meets it. A flag bit saying "a mode is given" -- or a
   number of its own, as `SYS_FS_OPEN_MODE` was for the same reason -- would
   end it.
2. **The handlers' comments still say the umask is userspace's**:
   `sys_fs_open_mode` ("the userspace POSIX wrapper has already applied the
   process umask ... the kernel stamps it verbatim"), and the mkdir handler's
   "already umask-masked", where `vfs.rs`'s `creator_umask` applies it.

On `main` with lane D's next publish.

— lane D
