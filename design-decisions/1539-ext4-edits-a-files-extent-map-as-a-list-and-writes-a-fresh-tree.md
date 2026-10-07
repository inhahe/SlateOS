## 1539. ext4 edits a file's extent map as a list and writes a fresh tree

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** the ext4 driver could not leave a hole in a file or fill one.
So making a file longer read the whole file into kernel memory and wrote it
back, and `truncate -s 10G` asked the kernel for ten gigabytes. A failed
kernel allocation halts the machine, so any program could stop it. Writing
into a hole failed, writing into preallocated space lost the data, and
nothing could find a file's holes (lane D's report). Every length change and
every write that needs a new block now goes through one engine: read the
file's whole block map, edit it as a list, and write a tree built afresh
from the list into new blocks. Growing a file costs nothing until it is
written, as on Linux.

**The engine** (`fs::ext4::extent_map`, `Ext4Driver::read_extent_map` /
`write_extent_tree`): truncation cuts the list and frees what it cut;
`write_extents` allocates the holes a write covers and marks unwritten
blocks written; `preallocate_extents` gives every hole in a range unwritten
blocks; `seek_data_hole` answers from the list. Data reaches its blocks
before the inode refers to them, the new tree is written before the inode,
and the old tree's blocks are freed after it.

**Choice 1 -- a fresh tree per change, not in-place surgery.** ext4's own
operations edit the tree where it stands: split an extent inside a full leaf,
insert in the middle, add a level. That is where ext4's subtlety lives, and
this driver writes no journal, so its only crash safety is write order. A tree
built in new blocks has that order by construction.

| | fresh tree (chosen) | in-place surgery |
|---|---|---|
| crash safety without a journal | by construction (new blocks, then the inode, then frees) | each split and insert needs its own ordering argument |
| cost per change | rewrite one leaf block per 340 extents (4 KiB blocks); none for 4 or fewer, nearly every file | one or two blocks |
| code | one builder, checkable without a disk | split, merge and grow paths, each its own bug surface |

*What changes:* a file with thousands of extents rewrites a few blocks of tree
on each write that changes its map. A write over blocks that already hold
data changes no map and takes the old in-place path.

**Choice 2 -- an old ext2/ext3 file (block-mapped, not extent-mapped) is
refused edits to its block map.** Growing it still works (a hole is a zero
pointer there too), and so do writes over blocks it already has. Shrinking
it, writing into its holes and preallocating answer `NotSupported`. The old
code "succeeded" by writing an extent tree into an inode marked as
block-mapped, which corrupted it. *What changes:* such operations on such
files fail instead of corrupting them. Images this system makes contain only
extent-mapped files.

**Choice 3 -- preallocation is all or nothing.** `fallocate` reserves every
block it was asked for or none, answering `DiskFull`. Linux may keep a partial
reservation. It used to succeed while reserving nothing, whenever the tree was
deeper than the inode or the request longer than one extent. *What changes:*
on a nearly full disk a large `fallocate` reserves nothing rather than part.

**Fixed with it:** `ee_len` is decoded as the ext4 specification says. 32768
is a full-length *initialized* extent, which the driver read as an empty
unwritten one, so a 128 MiB extent written by Linux read as missing. The
whole-file writer described any file as one extent of at most 32767 blocks,
so everything past 128 MiB was lost. It also needed one contiguous run, and
answered `DiskFull` on a fragmented disk with room to spare. Freeing blocks
now reads and writes each group's bitmap once, not once per block.
