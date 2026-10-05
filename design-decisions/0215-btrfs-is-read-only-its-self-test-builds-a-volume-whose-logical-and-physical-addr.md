## §215 — Btrfs is read-only, its self-test builds a volume whose logical and physical addresses deliberately differ, and every tree block is validated four separate ways

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)
**Lane:** A
**Where:** `kernel/src/fs/btrfs/` (`raw.rs`, `sb.rs`, `chunk.rs`, `btree.rs`,
`items.rs`, `mod.rs`, `tests.rs`), `kernel/src/fs/blocksrc.rs` (hoisted out of
`fs::ntfs::source`), `kernel/src/fs/mod.rs`, `kernel/src/main.rs` (self-test
call site), `kernel/src/kshell.rs` and `kernel/src/syscall/handlers.rs` (mount
dispatch)

**In short:** Btrfs is the default filesystem on Fedora, SUSE and Synology
boxes, so a dual-boot or externally-formatted disk is unreadable to us without
a driver. This adds one that can *read* a Btrfs volume but refuses every
write. Four calls were made, each with a genuine alternative: read-only rather
than read-write; a fake disk built in memory rather than a test that only runs
when someone plugs in a real one; putting the fake disk's data at a *different*
address from where the filesystem says it is, so a broken address map cannot
accidentally look correct; and checking each block four ways instead of just
verifying its checksum.

### Decision 1 — read-only, and the mount says so

Btrfs is copy-on-write: it never overwrites a block in place, it writes a new
copy and then rewrites every parent pointer up to the tree root. A single
file write therefore touches the file's tree, the extent tree (which
reference-counts every extent — the thing that makes snapshots cheap, since a
snapshot is just a second reference), the free-space cache, and finally the
superblock, and **all of it has to become visible atomically**. Get the extent
tree's refcounts wrong and Btrfs will later free an extent that a snapshot
still points at; the volume then reads corrupt data from a file nobody
touched, and `btrfs check` cannot tell what the right answer was.

That is not a larger version of the read path — it is a transaction engine.
So `statvfs` reports `read_only: true`, `free_blocks: 0`, and every write path
returns `ReadOnlyFilesystem`.

*The consequence that needed a decision*: unlike NTFS (§210), Btrfs stores a
real POSIX mode, so there is a true answer to report from `metadata()`. It is
reported — masked with `0o555`. Passing the stored mode through unmasked would
show `0o644` on a file the mount will refuse to write, which is the exact lie
§210 rejected; dropping the stored mode entirely and inventing `0o444`/`0o555`
like NTFS would throw away real information (the execute bit, which is the
difference between a script that runs and one that does not). Masking keeps
what is true and removes what is not.

*Alternative rejected*: read-write. Not on grounds of effort — the issue is
that a subtly wrong refcount update is indistinguishable from a correct one
until a snapshot is deleted months later, and by then the evidence of which
extent was wrongly freed is gone. Revisit only with the extent tree, a real
transaction commit, and crash injection, not as an increment on this code.

*Note on the roadmap wording* ("CoW, snapshots, checksums"): all three are
implemented in the sense a reader needs them. CoW is why the generation check
below exists; a snapshot is an ordinary subvolume tree and readable as one;
checksums are verified on every block. What is absent is *creating* snapshots,
which is write support.

### Decision 2 — the self-test builds its own volume

Same reasoning as §210 Decision 2, and it applies with more force here. The
hard part of Btrfs is not any one parser, it is the **mount bootstrap**, and
the bootstrap is circular: every tree block is addressed logically, logical
addresses are resolved through the chunk tree, and the chunk tree is itself at
a logical address. Btrfs breaks the cycle by copying the chunk items that
cover the metadata region into the superblock (`sys_chunk_array`) at a fixed
*physical* offset. So the order is superblock → `sys_chunk_array` → chunk tree
→ root tree → the FS tree's root item → the FS tree, and each step is only
reachable once the previous one worked.

A device-gated test — the `fs::iso9660::self_test()` precedent, which prints
*"No ISO 9660 filesystem mounted — skipping"* on every CI boot — would leave
all five steps unexecuted. `tests.rs` builds the whole volume in RAM and drives
the driver over it through `fs::blocksrc::MemorySource` on every boot.

Two properties of the builder were deliberate, beyond the ones §210 lists:

- **Logical and physical addresses are a constant 0x08_0000 apart.** This is
  the decision that makes the suite worth having. The convenient thing to do
  is lay the image out so that a block's logical address *is* its file offset;
  then the chunk map could be a stub returning its argument and every read
  would still return the right bytes. The whole bootstrap would be untested
  while appearing green. Offsetting them means a wrong map reads the wrong
  block, and the block's own `bytenr` check then says so.
- **`phys_of()` is a second, independent implementation of the map**, written
  from the layout table rather than by calling `ChunkMap::map`. A builder that
  called the parser's mapper would agree with it by construction, including
  where both are wrong.
- **The PREALLOC extent's physical bytes are filled with `0xAB`, not zero.**
  "Preallocated space reads as zeroes" is only a real test if the disk holds
  something else there; against a zero-filled image, a driver that wrongly
  read the extent would pass.
- **The FS tree is two levels deep** (an internal node over two leaves), and
  `sparse.bin` has an 8 KiB hole followed by a tail extent. Neither is
  decoration: climbing out of one leaf and descending into the next is the
  only thing that exercises the path manipulation in `next()`/`prev()`, and a
  read landing mid-extent is the only thing that exercises `prev()` at all.
  Btrfs leaves carry no sibling pointer — CoW would have to rewrite both
  siblings on every split — so the search path *is* the iterator's state, and
  that is the code most likely to be wrong.

*Alternative rejected*: shipping a real `mkfs.btrfs` image as a fixture. More
authentic, and it would settle the honest gap below — but it is an opaque
binary in git nobody can review, it cannot be minimised to the cases above,
and adding a case means regenerating it on a Linux box.

*The honest limitation, recorded in the module doc*: the suite proves the
driver is self-consistent, not that it is conformant. `name_hash` and the
checksum sealer are used on **both** sides, so a wrong CRC32C seed would
produce a wrong-but-agreeing hash and every test would still pass. Only a real
`mkfs.btrfs` image can settle that. The seed is the one place this is a live
risk, because Btrfs's name hash is CRC32C seeded with `~1` and *without* the
usual final inversion — Linux's `crc32c()` applies neither pre- nor
post-conditioning, and btrfs supplies only the odd seed. Getting it wrong
would make every directory lookup miss while every checksum still verified,
i.e. present as an *empty filesystem* rather than as a hash bug.

### Decision 3 — four validations per tree block, not just the checksum

Each of these catches something no other one can, which is why all four are
kept rather than the checksum alone:

| Check | Catches | Why the others miss it |
|---|---|---|
| CRC32C over `[32, nodesize)` | Bit rot, a torn write, an unrelated block | — |
| `header.bytenr` == the address we followed | A *valid, correctly-checksummed* block that is simply not the one asked for | A checksum proves a block is intact, never that it is the right block. A one-chunk error in the map yields a real tree node from elsewhere on the volume, which passes every other check and then parses as garbage. |
| `header.generation` == what the parent recorded | The CoW-specific hazard: the **previous version** of this node is still on disk, still intact, and still checksums perfectly | Both the checksum and `bytenr` pass on a stale node — it was a genuine node at that address. Only the generation says it is the wrong *epoch*, which is a failure mode CoW filesystems have and overwrite-in-place ones do not. |
| `nritems` fits the block | A corrupt count | Without it every subsequent slot read is an out-of-bounds index derived from disk data. |

*Alternative rejected*: checksum only, on the grounds that the other three are
redundant when the disk is honest. They are — and they are the only defence
when it is not, or when *our own* chunk map has a bug, which is the more
likely case in a new driver.

### Decision 4 — non-striped profiles only, and refused at parse time

For single/DUP/RAID1/RAID1C3/RAID1C4, stripe 0 alone is a contiguous copy of
the whole chunk, so `physical = stripe0.offset + (logical − chunk_start)` is
exact. RAID0/10/5/6 interleave, and that formula silently returns the wrong
bytes for them. `parse_chunk_item` therefore **rejects striped profiles when
the chunk item is parsed**, so the mount fails with `NotSupported`, rather than
letting the mount succeed and having reads return plausible-looking garbage.

For the same reason `map_range` requires a whole range to lie inside one
chunk: two adjacent chunks are contiguous *logically* and generally are not
physically, so mapping only a range's first byte would read across a seam.

**How to reverse:** read-only is the load-bearing decision and undoing it is a
new project (extent tree + transactions), not an edit. Striped profiles are a
localised addition — a stripe-walking loop in `chunk.rs` plus a `read_logical`
that can issue several reads per block — and the refusal is a single check in
`parse_chunk_item`. The `0o555` mask is one line in `mod.rs`. The self-test's
address offset is `PHYS_* = LOG_* − 0x08_0000` in `tests.rs`; setting it to
zero would make the suite pass more easily and prove less, which is the point.
