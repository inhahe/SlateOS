## ext4 `rename` has the same self-rename and subtree bugs memfs just had (lane A)

**Status:** FIXED 2026-08-22 (`kernel/src/fs/ext4/vfs_impl.rs`), both bugs, along
the lines sketched under "The proper fix" below. Verified in CI against the live
`/mnt` ext4 mount — see "Regression test" at the end of this entry.

**In short:** Renaming an ext4 file onto its own name — `rename("a", "a")`, which
POSIX says must quietly do nothing — instead deletes the file's contents. And
moving a directory inside itself is not checked for at all. Neither is reachable
from the boot test today, but both are reachable from userspace.

### Bug 1: `rename(x, x)` frees the file

The replace path resolves the destination and, finding it exists, unlinks it:

```rust
let dest_inode = self.driver.read_inode(dest_ino)?;
dest_inode.i_links_count = dest_inode.i_links_count.saturating_sub(1);
if dest_inode.i_links_count == 0 {
    self.driver.free_inode_data(dest_ino, &dest_inode)?;   // <- the file's blocks
    self.driver.free_inode_number(dest_ino, false)?;       // <- the inode itself
}
```

When `from` and `to` are the same path, `dest_ino == src_ino`, so this frees the
*source's* data and inode. Execution then continues to `add_dir_entry(...,
src_ino, dst_name, ...)` at line 579, re-adding a directory entry that now points
at a freed inode, and `remove_dir_entry` at 589 removes the entry it just added
(same name, same parent). Net effect: contents gone, inode freed, directory entry
dangling or absent.

### Bug 2: no into-own-subtree check

`rename("/a", "/a/b/c")` is not rejected. ext4 adds the destination entry before
removing the source one, so it does not lose the subtree the way memfs did, but
it produces an unreachable directory cycle — `/a` becomes its own ancestor, which
`fsck` will report and which can hang any naive tree walk.

### The proper fix

Mirror what `MemFs::rename` now does, before either the unlink or the
`add_dir_entry`:

1. If `src_ino == dest_ino`, return `Ok(())` — that catches `rename(x, x)`
   through any spelling of the two paths, including hard links to the same
   inode, which is exactly what POSIX specifies (*"if the two names refer to the
   same existing file, rename() shall return successfully and perform no other
   action"*) and is stronger than memfs's string comparison.
2. Walk `dst_parent_ino` up through `..` and refuse with `InvalidArgument` if
   `src_ino` is encountered — only needed when the source is a directory.

Extend the ext4 `vfs_impl` self-test with the same four cases the VFS self-test
now covers for memfs.

### Regression test

`fs::ext4::self_test()` Phase 1 gained a *"rename semantics"* section, which runs
against the live `/mnt` ext4 mount in every boot test. Six cases:

| # | Case | Expected |
|---|---|---|
| 1 | rename over an existing file | replaces it, source gone |
| 2 | `rename(x, x)` | no-op, content intact |
| 3 | rename between two hard links to one inode | no-op, **both** names survive |
| 4 | rename onto a directory | `IsADirectory` |
| 5 | move a directory into its own subtree | `InvalidArgument`, tree intact |
| 6 | move a directory to a *different* parent | succeeds |

Case 3 is the one that justifies comparing inode numbers rather than path
strings: two different paths, one file. memfs's equivalent check is a string
comparison and would miss it — memfs has no hard links, so there it cannot
arise, but the ext4 form is the one POSIX actually specifies.

Case 6 exists to keep the fix from being over-broad: the loop check added for
case 5 walks `..` upwards, and an ordinary cross-parent directory move is
exactly the traffic it has to let through. A check that rejected case 5 by
rejecting all directory moves would pass every other test here.
