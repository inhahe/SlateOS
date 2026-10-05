## 962. State the kernel keeps about a file outside its filesystem ends when the file's last name is removed — not when its last handle closes

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** the kernel keeps four kinds of note about files in memory — who
may open a file beyond its owner and group (an ACL), `chattr`-style flags,
seals, and searchable attributes. Each is filed under the file's inode number,
and when a file is deleted its inode number goes to the next file created, so
the notes have to be thrown away when their file goes or a stranger inherits
them. The question is what "goes" means. A file can be deleted while a program
still has it open, and the operating system keeps it alive until that program
closes it. The notes are now thrown away when the last *name* is removed,
without waiting for the last close.

**What was decided.** `fs::perfile` drops a table's entry when the VFS removes
an object's last name: a regular file with `nlinks <= 1`, a directory, or the
object a replacing rename displaces. It does not track open handles.

**Why the last name is enough.** Every consumer of these tables reaches them
through a name. ACLs are consulted when a path is resolved, the flags and seals
on path operations (by `kshell` today), and the attributes are found by path
queries. With the last name gone, none of them can consult an entry for this
file again. Keeping the entry until the last close would keep it only for
readers that do not exist.

**Alternatives considered.**

| option | why not |
|---|---|
| End it at the last close, as POSIX frees the inode | needs an open count per identity that the VFS does not keep, maintained on every open and close, the hottest path in the VFS, to preserve state nothing can read |
| Add a generation number to `FileId`, as NFS file handles do | makes a reused number miss rather than match, which fixes the inheritance, but entries would then never be removed: the tables would grow without bound (the seal table refuses new seals after 512) |
| Store the state in the inode: ACLs as `system.posix_acl_access` xattrs, flags as inode flags | the right long-term home, which ends with the inode by construction and survives a reboot; also a change to what persists on disk, and to the ACL hot path, larger than the bug. Recorded in `known-issues.md` `A-PER-FILE-STATE-OUTLIVED-ITS-FILE` |

**Revisit if** a consumer appears that reaches this state through a handle
rather than a name. Seal enforcement on an unlinked, still-open file is the
likely one, since that is how Linux uses `memfd` seals. At that point the end
of an identity has to wait for its last handle.

**Where this bites:** `kernel/src/fs/perfile.rs` (`Unlinked::from_meta`,
`object_unlinked`); the capture sites in `kernel/src/fs/vfs.rs`
(`unlinked_object`, `displaced_object`).
