### [A] `A-PER-FILE-STATE-OUTLIVED-ITS-FILE` — a deleted file's ACL, flags, seals and search attributes passed to the next file given its inode number -- 2026-09-25

**Status:** FIXED on lane-a 2026-09-25 (`fs::perfile`), awaiting a boot. What
remains open is listed at the end.

**In short:** four kernel tables remember things about files -- an ACL (who
may open it), `chattr`-style flags, seals, and the searchable attributes of
`fs::queryable`. Since 2026-09-21 they have filed each entry under the file's
identity (its filesystem plus inode number) rather than its name, so that two
names for one file share one entry. Nothing ever removed an entry when its
file was deleted. On ext4 the next file created is given the deleted file's
inode number, so that new file silently picked up the old one's ACL, flags,
seals and attributes -- including an ACL granting some other user access it
was never meant to have.

**Demonstrated, not inferred.** A host harness (the four modules and
`fs::perfile` compiled for the host, with a stub filesystem that reuses inode
numbers lowest-first, as ext4 does): plant state on `/tmp/hz-a`, delete it,
create `/tmp/hz-d`. The new file got `FileId { fs_id: 9, ino: 2 }`, the same
identity. With the fix it carries nothing; with the lifecycle events switched
off -- the code before the fix -- `acl`, `immutable`, `sealing` and
`queryable` all report it carrying the deleted file's entry.

**Why no boot saw it.** `/tmp` is memfs, which counts inode numbers up and
never reuses one; every identity rung runs there. The rootfs is ext4.

**Where the premise came from.** Lane C's
`TD-C-A-LIMIT-NEEDS-A-COUNT-AND-A-DELEGATION-NEEDS-A-WITNESS` table says
"`FileId` = `(fs_id, ino)`, never reused". Only the `fs_id` half is never
reused; the `FileId` docs never claimed more (they now say so outright). The
identity conversions of 2026-09-21 (dd-954 onward) were right to key on
identity. What they missed is that an identity has a lifetime.

**Severity.** Low today and pre-positioned, the same reasoning `acl.rs` gives
for its own keying fix: only a `kshell` command can create an ACL, seal or
flag entry, nothing in the VFS consults the flags or seals yet, and queryable
attributes are set only by callers of its API. Each of those is a future
syscall, and each would have armed this.

**The fix.** The VFS now reports the events that change what an identity
means, and each table registered in `fs::perfile::TABLES` handles them:

| event | sent from | table's response |
|---|---|---|
| last name removed (`nlinks <= 1`, or a directory) | `remove`, `rmdir`, `unlink_at_pinned`, and the name a replacing rename displaces | drop the entry |
| renamed | `rename`, `rename_noreplace`, `rename_at_pinned` | move the stored name (and a path key) at or under the old name |
| exchanged | `rename_exchange`, `rename_at_pinned(EXCHANGE)` | swap them |
| unmounted | `unmount` | drop every entry on that `fs_id` |

The identity is read with `lmetadata`, under the same filesystem lock as the
removal: removing a symlink must not end its target's state, and after the
removal the number may already be someone else's. Removing one of two hard
links ends nothing. A rename onto the moving file's own name (same path, or
two names of one inode) displaces nothing. Rationale for ending state at the
last name rather than the last close: design-decisions.md §962.

**Fixed alongside:**

- `queryable`'s value index was updated under whichever name each call used,
  so an attribute changed or removed through a second hard link left the old
  value indexed against the first name. Every index update now uses the
  record's own name, as `create_index` always did.
- `immutable::rename_path` and `queryable::rename_path` derived keys by
  looking names up, which is wrong at any moment (before a rename the new name
  holds nothing, after it the old name does). Both now rewrite stored names.
- `sealing::remove_on_delete` had no callers and compared names only; it now
  goes through the same identity-aware path.
- `acl.rs`'s module docs said ACLs were stored in the `system.posix_acl_access`
  xattr with the table as a cache. Nothing ever did that; the docs now say
  "in memory only".

**Verified by:** `fs::perfile::self_test` (six rungs through the real VFS on
`/tmp`: last name, replacing rename, rename, directory rename, exchange,
unmount), run by the boot after `queryable-attrs`. On the host it passes with
memfs-style and ext4-style numbering, and fails without the events.

**Still open:**

- *The reported name can go stale.* Each entry keeps the name it was set
  through, moved by later renames. If that name is removed while the file
  lives on under another hard link, `/proc` listings and query results show
  the removed name until the file goes. Nothing maps an identity back to its
  surviving names.
- *A rename scans every table.* It is nothing when the tables are empty, which
  is nearly always, and a few milliseconds at the 65,536 entries `queryable`
  and `immutable` allow. An ordered index by stored name would make it a range
  lookup if that ever matters.
- *The long-term home is the inode.* Linux keeps ACLs in the
  `system.posix_acl_access` xattr and the flags in the inode, so they end with
  it by construction and survive a reboot, which these in-memory tables do
  not. The VFS has working xattrs (ext4's included). Moving the tables there
  is a larger change, with on-disk format consequences, and would retire most
  of `fs::perfile`.
