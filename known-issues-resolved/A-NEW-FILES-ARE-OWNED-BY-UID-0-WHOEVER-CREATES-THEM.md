### A-NEW-FILES-ARE-OWNED-BY-UID-0-WHOEVER-CREATES-THEM -- 2026-10-02 -- FIXED the same day (lane A)

**Status:** FIXED 2026-10-02 (lane A), awaiting a boot on `main`. Every VFS
creation path -- a file made by a write or `O_CREAT`, `mkdir`, `symlink`, a
socket node, `O_TMPFILE` -- now gives the node its creator's uid and gid
under the same hold of the filesystem's lock that made it
(`vfs::init_new_owner`, as Linux's `inode_init_owner`): the directory's
group instead in a set-group-ID directory, where a new directory is
set-group-ID too. An overwrite keeps the owner; kernel context makes root's.
The creator is read before the filesystem's lock is taken. Checked by
`vfs::owner_self_test`. The quota charge still counts uid 0 for every write
(`write_file_resolved`'s "until per-process identity is wired up").

**In short:** every file, directory, symlink and socket node a program
creates is owned by user 0 (root), group 0, whoever created it. Today every
process is uid 0, so nothing shows; the first login service that starts a
second user's programs makes their files root's -- unreadable to them where
the mode says owner-only, and counted against root's quota.

**Where:** `kernel/src/fs/memfs.rs` `MemFsNode::new` (`uid: 0, gid: 0`), the
ext4 `create_inode`, and every VFS creation path (`Vfs::mkdir_mode`,
`Vfs::symlink`, `Vfs::mknod_socket`, `fs::handle`'s `O_CREAT`), none of which
stamps the creator.

**Proper fix:** every creation path sets the new node's owner to the
caller's filesystem uid, and its group to the caller's filesystem gid -- or
the parent directory's gid when the parent is setgid -- in the same
operation as the creation, as Linux's `inode_init_owner` does.
