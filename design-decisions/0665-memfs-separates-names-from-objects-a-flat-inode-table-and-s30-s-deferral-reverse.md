## §665 — memfs separates names from objects: a flat inode table, and §30's deferral reversed

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous) — this reverses §30, which was also
`Claude (autonomous)` and explicitly marked reversible. No operator decision is
being overturned.

**In short:** The in-memory filesystem — which is what `/` and `/tmp` are on
every boot — stored each file *inside* the directory that named it. That makes
a **hard link** (two names for one file, what `ln` without `-s` creates)
impossible to express, so `ln foo bar` on `/tmp` failed with "operation not
permitted". It has been rewritten to keep files in one table and let
directories hold *numbers* pointing into it, so two names can point at the same
file. §30 decided in June to leave this alone and test hard links on the ext4
disk instead; that decision is now reversed, and this entry records why the
reasoning that supported it stopped holding.

### What changed

`MemFs` was `{ root: MemFsNode }` with `Dir(BTreeMap<name, MemFsNode>)` — a
tree of nodes owned by their parent. It is now:

```rust
struct MemFs { inodes: BTreeMap<u64, MemFsNode>, root_ino: u64 }
enum MemFsNodeKind { File(Vec<u8>), Dir(BTreeMap<PathBuf, u64>), Symlink(PathBuf) }
```

Every name-side mutation goes through one of five helpers — `insert_new`,
`add_link`, `take_child`, `drop_link`, `child_ino` — so the two halves of a
link (the entry in a directory, and the count on the inode) cannot get out of
step by a new call site forgetting one of them. `drop_link` is the only place
a node leaves `inodes`, and it frees only when the count reaches zero.

`link` and `link_no_follow` are implemented; hard links to **directories** are
refused with `PermissionDenied`, as on Linux, because the path resolver assumes
the directory graph is a tree and a cycle made of directory links would spin
without ever tripping the symlink-depth counter — no symlink is involved.

### Why §30's reasoning stopped holding

§30 rejected the refactor as "a sweeping rewrite of a core subsystem … with no
current consumer demanding it." Three of its four supporting claims have since
become false, and one was never quite right:

| §30 said | Now |
|---|---|
| "no current consumer demanding it" | `Vfs::link_at_pinned` — the primitive `cp -r`/`cp -al` needs — had its positive same-inode assertion **skipped** for exactly this reason. A recorded skip *is* a consumer. |
| "the practically important case already works [on ext4]" | True as stated, and it is the *"instead"* that does not survive. ext4 really is exercised every boot — `rootfs.ext4` is attached as vdb, `/mnt` is mounted, and the ring-3 `link(2)` rung ran on it in all 14 boots in the skip window. But "one filesystem honours `link`" is a weaker property than "every filesystem that claims `link` honours it", and it is the weaker one §30 bought. The assertions that needed the *root* filesystem to have hard links — `pinned linkat`'s positive half, and memfs's own coverage — had no way to run at all. |
| "large, risky" | It came to ~900 lines of diff in one file, compiled on the first `cargo check`, and *removed* code: `walk_mut`, `nlink_count`, `rename`'s subtree move, `rename_exchange`'s rollback, and `statvfs`'s recursive count all went away. |
| "memfs reporting no hard-link support is spec-correct" | Still true, and still not the point. `EPERM` is a *permitted* answer, not a good one; it made the filesystem under `/tmp` less capable than the one under `/mnt` for no reason a caller could predict. |

The general shape is the one §664 just paid for from the other direction: a
property that no test could observe, because the configuration that would
expose it was the one nobody ran.

### What it bought beyond hard links

The refactor was justified by hard links, but separating names from objects
fixed four things that were not on anyone's list:

- **`rename` moves one `u64`.** It used to move a whole subtree, so renaming a
  large directory was O(descendants) and had an intermediate state.
- **`rename_exchange` needs no rollback.** Both lookups now precede any
  mutation, so the all-or-nothing property is structural rather than restored
  by an unwind path that had to be right.
- **`rename` no longer silently destroys a file.** `BTreeMap::insert` returns
  the evicted value; the old code dropped it, which is correct only when the
  displaced name was the object's *last* name. It goes through `drop_link` now.
- **`statvfs` counts objects, not names.** The recursive walk would have
  reported a twice-linked file twice.
- **`walk` needs only `&self`.** The nested-borrow contortions the owning tree
  forced (mutably borrowing every ancestor for as long as a descendant was
  held) are gone.

### `nlink` for directories is not the link count

A directory's internal `links` is always 1 — it has one name. But
`MemFs::nlink_of` reports `2 + immediate subdirectory count` for directories,
the Unix `.`/`..` convention, because `find(1)`'s leaf optimisation subtracts 2
and stops descending once it has seen that many subdirectories. Reporting the
true name count (1) would make `find` skip every real directory. The two
numbers are deliberately different and the field's doc on `FileMeta::nlinks`
now says so.

### Tests

`memfs::self_test` gained a hard-link block whose every assertion fails against
an implementation that copies instead of linking: same inode from both names,
nlink 2, a write through one name read back through the other, unlink of one
name leaving the other readable, and the inode table returning to its prior
size once the last name goes. The follow/no-follow split is asserted by whether
the *new* name is itself a symlink, which is the only thing that distinguishes
them.

`vfs_selftest`'s `pinned linkat` skip is retired: the assertion is now stated
unconditionally, because one that tolerates the filesystem declining to do the
thing is one that passes when the thing stops working.

`self_test_linux_link` (ring 3) now falls back to `/tmp` when there is no
`/mnt`. This one is insurance rather than a fix: the rung is *not* currently
skipping — `/mnt` is mounted on every boot the harness runs, and the skip
window records no skip for it — so the fallback buys coverage for a diskless
configuration rather than restoring coverage that was missing. It still prefers
ext4 when a disk is present, since that is the only ring-3 exercise of ext4's
`link`.

An earlier draft of this entry (and of the commit message) asserted the boot
test "runs diskless" and that the ext4 rung therefore never executed. That was
wrong, and it was wrong in the direction that flatters the change — checked
against the serial log and `check-boot-skips.py --list` afterwards, not before
writing it down. The real gap was narrower and is stated in the table above.

### How to reverse

There is no reason to, but: the tree form is recoverable by inlining `node()`
at its call sites and folding the table back into `Dir`. Doing so re-breaks
hard links and re-introduces every item in the list above, so the reversal
would have to be motivated by something other than simplicity.
