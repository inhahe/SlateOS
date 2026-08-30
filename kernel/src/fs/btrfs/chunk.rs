//! The logical-to-physical map, and the bootstrap that makes it readable.
//!
//! Btrfs addresses every tree block and every file extent by a *logical*
//! address in a single flat address space. That space is carved into chunks,
//! each described by a `btrfs_chunk` item in the chunk tree, and each chunk
//! names the device(s) and physical offset(s) that back it. Nothing in the
//! filesystem can be read until enough of this map exists to translate the
//! address being read.
//!
//! # The circular dependency, and how the format breaks it
//!
//! The chunk tree lives at a logical address (`chunk_root` in the superblock),
//! so translating it requires the map that the chunk tree defines. Btrfs
//! solves this by copying the chunk items covering the system block groups
//! into `sys_chunk_array` inside the superblock, which is at a fixed
//! *physical* offset. Those items are enough — and only just enough — to
//! reach `chunk_root`; the full map is then read out of the tree itself.
//!
//! So a mount fills this structure twice: once from the superblock array
//! (partial), then again from the tree (complete). [`ChunkMap::insert`] is
//! idempotent for exactly that reason — the system chunks appear in both
//! sources, and the second pass must not double-count them.
//!
//! # Which chunk profiles are readable here
//!
//! For every profile that is not *striped* — single, DUP, RAID1, RAID1C3,
//! RAID1C4 — stripe 0 alone holds a complete, contiguous copy of the chunk,
//! so mapping is `physical = stripe0.offset + (logical - chunk_start)`. That
//! is the whole of the translation, and it is what this module implements.
//!
//! RAID0/RAID10/RAID5/RAID6 interleave the chunk across stripes at
//! `stripe_len` granularity, so a contiguous logical range is *not* a
//! contiguous physical range and the formula above would silently return the
//! wrong bytes. Those are rejected at parse time rather than mapped
//! approximately: on a checksummed filesystem a wrong mapping would surface
//! as a checksum failure on some unrelated block, which is a far harder thing
//! to debug than "this profile is not supported".

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::raw::{CHUNK_ITEM_KEY, FIRST_CHUNK_TREE_OBJECTID, KEY_LEN, Key, read_u16, read_u64};

/// Bytes of `btrfs_chunk` before the first stripe.
pub const CHUNK_HEADER_LEN: usize = 48;
/// Bytes per `btrfs_stripe`: devid(8) + offset(8) + `dev_uuid`(16).
pub const STRIPE_LEN: usize = 32;

// Block-group type flags, from the `type` field of a `btrfs_chunk`.

/// Chunk holds file data.
pub const BLOCK_GROUP_DATA: u64 = 1 << 0;
/// Chunk holds system metadata (the chunk tree itself).
pub const BLOCK_GROUP_SYSTEM: u64 = 1 << 1;
/// Chunk holds filesystem metadata.
pub const BLOCK_GROUP_METADATA: u64 = 1 << 2;
/// Striped across devices with no redundancy.
pub const BLOCK_GROUP_RAID0: u64 = 1 << 3;
/// Two full copies on different devices.
pub const BLOCK_GROUP_RAID1: u64 = 1 << 4;
/// Two full copies on the same device.
pub const BLOCK_GROUP_DUP: u64 = 1 << 5;
/// Striped and mirrored.
pub const BLOCK_GROUP_RAID10: u64 = 1 << 6;
/// Striped with single parity.
pub const BLOCK_GROUP_RAID5: u64 = 1 << 7;
/// Striped with double parity.
pub const BLOCK_GROUP_RAID6: u64 = 1 << 8;
/// Three full copies.
pub const BLOCK_GROUP_RAID1C3: u64 = 1 << 9;
/// Four full copies.
pub const BLOCK_GROUP_RAID1C4: u64 = 1 << 10;

/// Profiles under which stripe 0 is a contiguous copy of the whole chunk.
///
/// Note that *single* is the absence of every profile bit, so it is covered by
/// the check being `type & STRIPED_PROFILES == 0` rather than by membership in
/// a positive list.
pub const STRIPED_PROFILES: u64 =
    BLOCK_GROUP_RAID0 | BLOCK_GROUP_RAID10 | BLOCK_GROUP_RAID5 | BLOCK_GROUP_RAID6;

/// One chunk: a contiguous logical range and where it lives on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEntry {
    /// First logical address covered.
    pub logical: u64,
    /// Length in bytes.
    pub length: u64,
    /// Physical byte offset of stripe 0 on the device.
    pub physical: u64,
    /// Device id stripe 0 lives on.
    pub devid: u64,
    /// Block-group type and profile flags.
    pub flags: u64,
}

impl ChunkEntry {
    /// Whether `logical` falls inside this chunk.
    pub fn contains(&self, logical: u64) -> bool {
        logical >= self.logical && logical.saturating_sub(self.logical) < self.length
    }
}

/// Parse one `btrfs_chunk` item body, given the key that introduced it.
///
/// Returns the entry and the total on-disk length of the item, which the
/// `sys_chunk_array` walker needs in order to find the next key — the item is
/// variable-length (one `btrfs_stripe` per `num_stripes`) and there is no
/// length prefix in the array, so the parser's own arithmetic is the only
/// thing that keeps it in step.
pub fn parse_chunk_item(key: &Key, value: &[u8]) -> KernelResult<(ChunkEntry, usize)> {
    if key.item_type != CHUNK_ITEM_KEY || key.objectid != FIRST_CHUNK_TREE_OBJECTID {
        return Err(KernelError::InvalidArgument);
    }

    let length = read_u64(value, 0)?;
    let flags = read_u64(value, 24)?;
    let num_stripes = read_u16(value, 44)?;

    if length == 0 || num_stripes == 0 {
        return Err(KernelError::InvalidArgument);
    }
    if flags & STRIPED_PROFILES != 0 {
        return Err(KernelError::NotSupported);
    }

    let stripes_len = usize::from(num_stripes)
        .checked_mul(STRIPE_LEN)
        .ok_or(KernelError::InvalidArgument)?;
    let item_len = CHUNK_HEADER_LEN
        .checked_add(stripes_len)
        .ok_or(KernelError::InvalidArgument)?;
    if value.len() < item_len {
        return Err(KernelError::InvalidArgument);
    }

    // Stripe 0 only: for every profile that reaches here, the remaining
    // stripes are mirrors of it.
    let devid = read_u64(value, CHUNK_HEADER_LEN)?;
    let physical = read_u64(value, CHUNK_HEADER_LEN.saturating_add(8))?;

    // A chunk whose logical range wraps is not a range at all, and would make
    // `contains` and the sorted-order invariant both meaningless.
    key.offset
        .checked_add(length)
        .ok_or(KernelError::InvalidArgument)?;
    physical
        .checked_add(length)
        .ok_or(KernelError::InvalidArgument)?;

    Ok((
        ChunkEntry {
            logical: key.offset,
            length,
            physical,
            devid,
            flags,
        },
        item_len,
    ))
}

/// The assembled logical-to-physical map.
///
/// Entries are kept sorted by `logical` so lookup is a binary search. The map
/// is small — one entry per chunk, so tens of entries on a filesystem of any
/// ordinary size — but it is consulted on *every* tree-block read and every
/// file extent read, which is exactly the shape of thing that must not be a
/// linear scan by the time a directory walk touches a few hundred blocks.
#[derive(Debug, Default)]
pub struct ChunkMap {
    entries: Vec<ChunkEntry>,
}

impl ChunkMap {
    /// An empty map.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// How many chunks are mapped.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty (nothing can be read yet).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert a chunk, ignoring a repeat of one already known.
    ///
    /// Idempotence is load-bearing rather than defensive: the system chunks
    /// are read twice by design, once from `sys_chunk_array` and once from the
    /// chunk tree, and both copies describe the same range.
    ///
    /// A *conflicting* entry for the same logical start — same address,
    /// different length or physical — is rejected, because that is either a
    /// corrupt image or a parser that has lost its place in the item array,
    /// and silently keeping one of the two would turn either into wrong data
    /// rather than an error.
    pub fn insert(&mut self, entry: ChunkEntry) -> KernelResult<()> {
        match self
            .entries
            .binary_search_by_key(&entry.logical, |e| e.logical)
        {
            Ok(i) => {
                let existing = self.entries.get(i).ok_or(KernelError::InternalError)?;
                if *existing == entry {
                    Ok(())
                } else {
                    Err(KernelError::InvalidArgument)
                }
            }
            Err(i) => {
                self.entries.insert(i, entry);
                Ok(())
            }
        }
    }

    /// Translate one logical address to a physical byte offset.
    pub fn map(&self, logical: u64) -> KernelResult<u64> {
        self.lookup(logical).and_then(|e| {
            let delta = logical.saturating_sub(e.logical);
            e.physical
                .checked_add(delta)
                .ok_or(KernelError::InvalidArgument)
        })
    }

    /// Translate a logical *range*, requiring it to lie within a single chunk.
    ///
    /// This is the entry point every caller should use, because a range that
    /// spans two chunks is contiguous logically and generally *not* contiguous
    /// physically. Mapping only its first byte and then reading `len` bytes
    /// from there would quietly read whatever chunk happens to be adjacent on
    /// the device. Btrfs never places a tree node or a single extent across a
    /// chunk boundary, so a range that does span one is a corrupt image or a
    /// bad pointer — and should be reported as such.
    pub fn map_range(&self, logical: u64, len: u64) -> KernelResult<u64> {
        if len == 0 {
            return self.map(logical);
        }
        let entry = self.lookup(logical)?;
        let end = logical
            .checked_add(len)
            .ok_or(KernelError::InvalidArgument)?;
        let chunk_end = entry
            .logical
            .checked_add(entry.length)
            .ok_or(KernelError::InvalidArgument)?;
        if end > chunk_end {
            return Err(KernelError::InvalidArgument);
        }
        let delta = logical.saturating_sub(entry.logical);
        entry
            .physical
            .checked_add(delta)
            .ok_or(KernelError::InvalidArgument)
    }

    /// The chunk covering `logical`, or [`KernelError::InvalidArgument`].
    fn lookup(&self, logical: u64) -> KernelResult<&ChunkEntry> {
        // The last entry starting at or before `logical`.
        let idx = self.entries.partition_point(|e| e.logical <= logical);
        let entry = idx
            .checked_sub(1)
            .and_then(|i| self.entries.get(i))
            .ok_or(KernelError::InvalidArgument)?;
        if entry.contains(logical) {
            Ok(entry)
        } else {
            Err(KernelError::InvalidArgument)
        }
    }

    /// Load the bootstrap chunks embedded in the superblock.
    ///
    /// The array is a bare sequence of `(btrfs_disk_key, btrfs_chunk)` pairs
    /// with no count and no per-item length — the walker's arithmetic is the
    /// only synchronisation, so a single misparse would reinterpret the rest
    /// of the array as garbage. That is why [`parse_chunk_item`] returns the
    /// consumed length rather than having the caller recompute it, and why a
    /// key with an unexpected type is a hard error instead of something to
    /// skip past.
    pub fn load_sys_array(&mut self, array: &[u8]) -> KernelResult<usize> {
        let mut off = 0usize;
        let mut count = 0usize;

        while off < array.len() {
            // A trailing partial record means the array length disagrees with
            // its contents; better to say so than to read past it.
            let key = Key::parse(array, off)?;
            let value_off = off
                .checked_add(KEY_LEN)
                .ok_or(KernelError::InvalidArgument)?;
            let value = array.get(value_off..).ok_or(KernelError::InvalidArgument)?;

            let (entry, item_len) = parse_chunk_item(&key, value)?;
            self.insert(entry)?;
            count = count.saturating_add(1);

            off = value_off
                .checked_add(item_len)
                .ok_or(KernelError::InvalidArgument)?;
        }

        if count == 0 {
            // Without at least one system chunk there is no way to reach
            // `chunk_root`, so this is unmountable rather than merely odd.
            return Err(KernelError::InvalidArgument);
        }
        Ok(count)
    }
}
