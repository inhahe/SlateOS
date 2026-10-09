### A-PINNED-CREATES-ARE-OWNED-BY-ROOT -- 2026-10-02 -- FIXED (lane A)

**Status:** FIXED on lane-a-wip 2026-10-02, awaiting a boot -- found while
putting the attribute rules into the pinned-directory calls. Both now call
`init_new_owner` under the creation's guard; `vfs::owner_self_test` makes a
directory and a symlink through a held directory as a uid-1000 process, and
a directory through a held set-group-ID one.

**In short:** a directory or symlink made through a held directory --
`SYS_FS_MKDIRAT_PINNED`, `SYS_FS_SYMLINKAT_PINNED`, which lane B's `cp -r`
and lane D's `mkdirat` use -- belongs to root, whoever made it: the path-based
`mkdir` and `symlink` give a new node its creator's owner and group
(`init_new_owner`, since 2026-10-02), and these two were missed.

**Where:** `kernel/src/fs/vfs.rs`, `Vfs::mkdir_at_pinned` and
`Vfs::symlink_at_pinned`.

**Proper fix:** call `init_new_owner` under the same guard as the creation,
as `mkdir_mode` and `symlink` do, with the set-group-ID directory rule.
