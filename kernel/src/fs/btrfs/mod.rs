//! Read-only Btrfs driver.
//!
//! Btrfs is a copy-on-write filesystem in which *everything* — including the
//! map from logical to physical addresses — is stored in B-trees keyed by
//! `(objectid, type, offset)`. That uniformity is what makes the driver
//! tractable: one node parser and one key search serve the chunk tree, the
//! root tree and every filesystem tree.
//!
//! # The bootstrap, which is the part that is genuinely awkward
//!
//! Every tree block is addressed *logically*, and logical addresses are
//! resolved through the chunk tree — which is itself a tree, at a logical
//! address. Reading the chunk tree therefore requires the chunk tree.
//!
//! Btrfs breaks the cycle by copying the handful of chunk items that cover the
//! metadata region into the superblock itself, as `sys_chunk_array`. So the
//! order is fixed and each step unlocks the next:
//!
//! 1. Read the superblock from a fixed *physical* offset — the only thing in
//!    the filesystem that does not need the map.
//! 2. Parse `sys_chunk_array` into a partial map, enough to cover
//!    `chunk_root`.
//! 3. Walk the chunk tree through that partial map and learn every remaining
//!    chunk. The map is now complete.
//! 4. Read the root tree at `root`, which lists the roots of all other trees.
//! 5. Look up `FS_TREE_OBJECTID` (5) there to get the default subvolume, and
//!    walk it for inodes, directory entries and file extents.
//!
//! # Scope: read-only, on purpose
//!
//! Writing to a CoW filesystem means allocating extents, updating the extent
//! tree's reference counts, and committing a transaction across several trees
//! atomically — a bug in any of which corrupts a volume that was fine before
//! we touched it. Read support is separately useful (mounting an existing
//! Linux volume) and cannot lose data. See `design-decisions.md` for the
//! matching decision on NTFS.

pub mod btree;
pub mod chunk;
pub mod items;
pub mod raw;
pub mod sb;
