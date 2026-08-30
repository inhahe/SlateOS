//! Btrfs on-disk primitives: little-endian scalars, keys, and tree headers.
//!
//! Everything Btrfs stores on disk is little-endian and, with one important
//! exception, unaligned as far as we are concerned: we parse out of a byte
//! buffer that was read from a device, so nothing may be reinterpreted as a
//! `#[repr(C)]` struct via a pointer cast. Every field is read by explicit
//! offset through the helpers here.
//!
//! That is not merely conservative style. A tree node is `nodesize` bytes read
//! into a `Vec<u8>` whose alignment is the allocator's business, and the item
//! array inside a leaf is packed to byte granularity — a `u64` at offset 0x31
//! is genuinely unaligned. On x86_64 an unaligned load happens to work, which
//! is exactly what makes the mistake survive testing and then fail somewhere
//! else; `read_u64` costs nothing at `-O2` because the compiler lowers it back
//! to a single load when it is safe to do so.

use crate::error::{KernelError, KernelResult};

/// Byte offset of the primary superblock from the start of the volume.
///
/// Btrfs deliberately places it at 64 KiB rather than 0 so that a partition
/// table, a bootloader, or another filesystem's boot sector can coexist in the
/// first 64 KiB without being clobbered.
pub const SUPERBLOCK_OFFSET: u64 = 0x1_0000;

/// `_BHRfS_M`, the superblock magic, stored at offset 0x40 within it.
pub const MAGIC: u64 = 0x4D5F_5366_5248_425F;

/// Length of a `btrfs_disk_key` on disk: objectid(8) + type(1) + offset(8).
pub const KEY_LEN: usize = 17;

/// Length of a `btrfs_header` on disk.
///
/// csum(32) + fsid(16) + bytenr(8) + flags(8) + chunk_tree_uuid(16) +
/// generation(8) + owner(8) + nritems(4) + level(1) = 101.
pub const HEADER_LEN: usize = 101;

/// Length of a `btrfs_item` (leaf slot descriptor): key(17) + offset(4) + size(4).
pub const ITEM_LEN: usize = KEY_LEN + 8;

/// Length of a `btrfs_key_ptr` (internal-node slot): key(17) + blockptr(8) + generation(8).
pub const KEY_PTR_LEN: usize = KEY_LEN + 16;

/// Size of the checksum field at the front of every superblock and tree node.
pub const CSUM_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Well-known object IDs
// ---------------------------------------------------------------------------

/// The tree of tree roots.
pub const ROOT_TREE_OBJECTID: u64 = 1;
/// The extent allocation tree.
pub const EXTENT_TREE_OBJECTID: u64 = 2;
/// The chunk tree (logical -> physical).
pub const CHUNK_TREE_OBJECTID: u64 = 3;
/// The device tree.
pub const DEV_TREE_OBJECTID: u64 = 4;
/// The default subvolume's filesystem tree.
pub const FS_TREE_OBJECTID: u64 = 5;
/// The checksum tree.
pub const CSUM_TREE_OBJECTID: u64 = 7;
/// The inode number of the root directory within a filesystem tree.
pub const FIRST_FREE_OBJECTID: u64 = 256;
/// The `objectid` every `CHUNK_ITEM` key carries in the chunk tree.
///
/// Numerically identical to [`FIRST_FREE_OBJECTID`], and deliberately named
/// separately: they are the same number used for two unrelated purposes, and
/// a reader who sees `256` in the chunk parser should not have to wonder
/// whether an inode number leaked into it.
pub const FIRST_CHUNK_TREE_OBJECTID: u64 = 256;

// ---------------------------------------------------------------------------
// Key types (the `type` byte of a `btrfs_disk_key`)
// ---------------------------------------------------------------------------

/// `btrfs_inode_item` — the inode itself.
pub const INODE_ITEM_KEY: u8 = 1;
/// `btrfs_inode_ref` — a name/parent backreference.
pub const INODE_REF_KEY: u8 = 12;
/// `btrfs_dir_item` — a directory entry hashed by name.
pub const DIR_ITEM_KEY: u8 = 84;
/// `btrfs_dir_item` in readdir order.
pub const DIR_INDEX_KEY: u8 = 96;
/// `btrfs_file_extent_item` — file data location.
pub const EXTENT_DATA_KEY: u8 = 108;
/// `btrfs_root_item` — a tree root, found in the root tree.
pub const ROOT_ITEM_KEY: u8 = 132;
/// `btrfs_chunk` — a logical-to-physical mapping, found in the chunk tree.
pub const CHUNK_ITEM_KEY: u8 = 228;
/// `btrfs_dev_item` — a device, found in the chunk tree.
pub const DEV_ITEM_KEY: u8 = 216;

// ---------------------------------------------------------------------------
// Little-endian scalar reads
// ---------------------------------------------------------------------------

/// Read a `u8` at `off`, or [`KernelError::InvalidArgument`] if out of range.
pub fn read_u8(buf: &[u8], off: usize) -> KernelResult<u8> {
    buf.get(off).copied().ok_or(KernelError::InvalidArgument)
}

/// Read a little-endian `u16` at `off`.
pub fn read_u16(buf: &[u8], off: usize) -> KernelResult<u16> {
    let end = off.checked_add(2).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let arr: [u8; 2] = s.try_into().map_err(|_| KernelError::InternalError)?;
    Ok(u16::from_le_bytes(arr))
}

/// Read a little-endian `u32` at `off`.
pub fn read_u32(buf: &[u8], off: usize) -> KernelResult<u32> {
    let end = off.checked_add(4).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let arr: [u8; 4] = s.try_into().map_err(|_| KernelError::InternalError)?;
    Ok(u32::from_le_bytes(arr))
}

/// Read a little-endian `u64` at `off`.
pub fn read_u64(buf: &[u8], off: usize) -> KernelResult<u64> {
    let end = off.checked_add(8).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let arr: [u8; 8] = s.try_into().map_err(|_| KernelError::InternalError)?;
    Ok(u64::from_le_bytes(arr))
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// A `btrfs_disk_key`: the total ordering that every Btrfs B-tree is sorted by.
///
/// The ordering is lexicographic on `(objectid, item_type, offset)` — and it is
/// worth being explicit that `item_type` is compared as the *middle* component,
/// not as a tiebreak. All of a file's items therefore sit contiguously under
/// its `objectid`, sorted by type, which is what makes "read this inode, then
/// its name refs, then its extents" a single ordered walk rather than three
/// searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Key {
    /// Which object this item belongs to (an inode number, a tree id, ...).
    pub objectid: u64,
    /// What kind of item this is; see the `*_KEY` constants.
    pub item_type: u8,
    /// Type-dependent: a file offset, a name hash, a tree generation, ...
    pub offset: u64,
}

impl Key {
    /// Build a key.
    pub const fn new(objectid: u64, item_type: u8, offset: u64) -> Self {
        Self {
            objectid,
            item_type,
            offset,
        }
    }

    /// Parse a key from `buf` at `off`.
    pub fn parse(buf: &[u8], off: usize) -> KernelResult<Self> {
        Ok(Self {
            objectid: read_u64(buf, off)?,
            item_type: read_u8(buf, off.checked_add(8).ok_or(KernelError::InvalidArgument)?)?,
            offset: read_u64(buf, off.checked_add(9).ok_or(KernelError::InvalidArgument)?)?,
        })
    }

    /// The comparison tuple, in on-disk sort order.
    ///
    /// Exposed because the B-tree search and the tests must agree on the
    /// ordering by construction rather than by two hand-written comparisons.
    pub const fn sort_tuple(&self) -> (u64, u8, u64) {
        (self.objectid, self.item_type, self.offset)
    }
}

// ---------------------------------------------------------------------------
// Tree node header
// ---------------------------------------------------------------------------

/// The `btrfs_header` shared by every tree block, leaf and internal alike.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    /// Logical address this block claims to live at.
    ///
    /// Checked against the address we followed to get here: a block that
    /// disagrees means the chunk mapping or a block pointer is wrong, and
    /// carrying on would parse unrelated bytes as a node.
    pub bytenr: u64,
    /// Generation (transaction id) the block was written in.
    pub generation: u64,
    /// Which tree owns this block.
    pub owner: u64,
    /// Number of slots in use.
    pub nritems: u32,
    /// 0 for a leaf; > 0 for an internal node.
    pub level: u8,
}

impl Header {
    /// Parse a tree-block header from the front of `buf`.
    ///
    /// The offsets follow directly from the field sizes listed on
    /// [`HEADER_LEN`]: csum(32) fsid(16) puts `bytenr` at 48, flags(8) and
    /// chunk_tree_uuid(**16**, not 8) put `generation` at 80, and the rest
    /// follows. The uuid's width is the easy mistake here — getting it wrong
    /// shifts every field from `generation` on by eight bytes, which reads
    /// `owner` out of `generation`'s slot and so fails the generation check on
    /// a block that is in fact perfectly good.
    pub fn parse(buf: &[u8]) -> KernelResult<Self> {
        Ok(Self {
            bytenr: read_u64(buf, 48)?,
            generation: read_u64(buf, 80)?,
            owner: read_u64(buf, 88)?,
            nritems: read_u32(buf, 96)?,
            level: read_u8(buf, 100)?,
        })
    }

    /// Whether this block is a leaf (holds items rather than child pointers).
    pub const fn is_leaf(&self) -> bool {
        self.level == 0
    }
}
