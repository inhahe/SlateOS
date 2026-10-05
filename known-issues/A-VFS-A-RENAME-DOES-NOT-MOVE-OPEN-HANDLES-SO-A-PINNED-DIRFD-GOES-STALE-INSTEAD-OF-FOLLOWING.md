## A-VFS-A-RENAME-DOES-NOT-MOVE-OPEN-HANDLES-SO-A-PINNED-DIRFD-GOES-STALE-INSTEAD-OF-FOLLOWING (lane A)

**Status:** OPEN 2026-08-30

`OpenFile::path` in `kernel/src/fs/handle.rs` records the path a handle was
opened under and is never updated. `Vfs::rename` moves the object and leaves
every open handle naming the old location.

Before the pinned primitives, that made a descriptor silently *wrong* after a
rename — lane B's "consequence 2", the one that needs no attacker and which
they expected a ported build system to hit first. It is now *loud* instead:
`verify_pinned` sees the mismatch and the call fails `ESTALE`.

**That is better and still not POSIX.** A real `unlinkat` keeps working after
its directory is renamed, because the fd refers to the inode, not the name. A
tool that renames a directory it holds open will now get `ESTALE` where Linux
gives success.

**Proper fix.** Hook `Vfs::rename` / `rename_noreplace` / `rename_exchange` to
rewrite the stored path prefix of every open handle whose pinned identity
matches the moved object. This is *possible now and was not before*: it needs
to identify which handles refer to the moved object, which is exactly what
`OpenFile::dir_pin` supplies. It needs a lock-order decision — the rename holds
the filesystem mutex and the rewrite needs `OPEN_FILES` — so the scan should
collect under the fs lock and apply after, or the two must be ordered
deliberately and documented.

**Not done speculatively** because it is a user-visible behaviour fork: loud
`ESTALE` versus POSIX follow-the-inode. Raised with lane B in
`requests/a-b-the-at-family-now-has-three-primitives-that-resolve-the-handle.md`;
if it bites a port, that is this entry.
