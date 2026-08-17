//! Btrfs self-tests, driven by a synthetic volume built in RAM.
//!
//! # Why this file builds a whole filesystem
//!
//! Every hard part of a Btrfs driver is a *relationship between blocks*: a
//! logical address that only resolves once the chunk tree has been
//! bootstrapped, a leaf whose item data grows upward from the end of the block,
//! an internal node whose child pointers carry the generation the child must
//! have. None of that can be exercised by feeding a hand-written byte array to
//! one parser — a byte array proves the parser accepts what the *test author*
//! believed the format to be, which is exactly the belief under test.
//!
//! So [`build_image`] lays out a complete, structurally valid single-device
//! Btrfs volume: three chunks at logical addresses deliberately unequal to
//! their physical ones, a chunk tree, a root tree, and a **two-level** file
//! tree whose items span two leaves. The driver then mounts it through
//! [`MemorySource`] and reads it back with no device involved, on every boot.
//!
//! The builder is written *independently* of the parser — it appends fields in
//! order from the format documentation rather than calling any shared
//! serialiser, and it computes physical addresses from the layout table below
//! rather than from [`super::chunk::ChunkMap`]. A builder that reused the
//! parser's notion of the layout would agree with it by construction,
//! including where both are wrong.
//!
//! # What this does *not* prove
//!
//! The image is hashed with [`super::items::name_hash`] and checksummed with
//! [`super::sb::seal_csum`], so directory lookup and block validation are
//! tested for self-consistency, not for conformance to Linux. If the CRC32C
//! seed used for names were wrong, this suite would still pass — the same
//! wrong hash would be on both sides. Only a real `mkfs.btrfs` image can
//! settle that, and the reasoning behind the seed is recorded at
//! [`super::items::name_hash`] instead.
//!
//! # Volume layout
//!
//! ```text
//! physical            logical             contents
//! 0x010000                                superblock (mirror 0)
//! 0x080000..0x180000  0x100000..0x200000  SYSTEM chunk
//!   0x080000            0x100000            chunk-tree leaf
//! 0x180000..0x280000  0x200000..0x300000  METADATA chunk
//!   0x180000            0x200000            root-tree leaf
//!   0x181000            0x201000            FS-tree internal node (level 1)
//!   0x182000            0x202000            FS-tree leaf A (inodes 256-258)
//!   0x183000            0x203000            FS-tree leaf B (inodes 259-262)
//! 0x280000..0x300000  0x300000..0x380000  DATA chunk
//!   0x280000            0x300000            sub/data.bin, 8192 bytes
//!   0x282000            0x302000            sparse.bin tail, 4096 bytes
//!   0x283000            0x303000            prealloc.bin's reservation
//! ```
//!
//! Logical is offset from physical by 0x080000 on purpose: were the two equal,
//! a completely broken chunk map would still produce correct reads and the
//! bootstrap would be untested.
//!
//! ```text
//! 256  /                 259  /sub/data.bin    (regular extent, 8 KiB)
//! 257  /hello.txt        260  /link            (symlink, inline)
//! 258  /sub              261  /sparse.bin      (8 KiB hole + 4 KiB tail)
//!                        262  /prealloc.bin    (PREALLOC, must read zero)
//! ```

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::MemorySource;
use crate::fs::path::Path;
use crate::fs::vfs::{EntryType, FileSystem};
use crate::serial_println;

use super::BtrfsFs;
use super::btree::TreeReader;
use super::chunk::{
    BLOCK_GROUP_DATA, BLOCK_GROUP_DUP, BLOCK_GROUP_METADATA, BLOCK_GROUP_RAID0,
    BLOCK_GROUP_SYSTEM, ChunkEntry, ChunkMap, parse_chunk_item,
};
use super::items::{
    FILE_EXTENT_INLINE, FILE_EXTENT_PREALLOC, FILE_EXTENT_REG, FT_DIR, FT_REG_FILE, FT_SYMLINK,
    FileExtent, InodeItem, RootItem, name_hash, parse_dir_items, parse_file_extent,
};
use super::raw::{
    CHUNK_ITEM_KEY, CHUNK_TREE_OBJECTID, DEV_ITEM_KEY, DIR_INDEX_KEY, DIR_ITEM_KEY,
    EXTENT_DATA_KEY, EXTENT_TREE_OBJECTID, FIRST_CHUNK_TREE_OBJECTID, FS_TREE_OBJECTID, HEADER_LEN,
    INODE_ITEM_KEY, INODE_REF_KEY, ITEM_LEN, KEY_LEN, KEY_PTR_LEN, Key, MAGIC, ROOT_ITEM_KEY,
    ROOT_TREE_OBJECTID, read_u16, read_u32, read_u64, read_u8,
};
use super::sb::{
    FEATURE_INCOMPAT_BIG_METADATA, FEATURE_INCOMPAT_COMPRESS_ZSTD, FEATURE_INCOMPAT_EXTENDED_IREF,
    FEATURE_INCOMPAT_MIXED_BACKREF, FEATURE_INCOMPAT_NO_HOLES, FEATURE_INCOMPAT_SKINNY_METADATA,
    SUPER_INFO_SIZE, Superblock, seal_csum,
};

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Metadata block size. Deliberately equal to `sectorsize` and to the page
/// size, so a leaf is exactly one block and the arithmetic stays checkable by
/// hand; real volumes use 16 KiB, which the parser treats identically.
const NODESIZE: usize = 4096;
/// Data allocation unit.
const SECTORSIZE: u32 = 4096;
/// Total image length; 3.5 MiB, enough for all three chunks.
const IMAGE_LEN: usize = 0x38_0000;
/// `total_bytes` as the superblock reports it.
const TOTAL_BYTES: u64 = 0x38_0000;
/// Physical offset of superblock mirror 0.
const PHYS_SB: u64 = 0x1_0000;
/// The one device id in this single-device volume.
const DEVID: u64 = 1;
/// Transaction id every block in the image was written in.
const GEN: u64 = 7;

const LOG_SYS: u64 = 0x10_0000;
const PHYS_SYS: u64 = 0x08_0000;
const LEN_SYS: u64 = 0x10_0000;

const LOG_META: u64 = 0x20_0000;
const PHYS_META: u64 = 0x18_0000;
const LEN_META: u64 = 0x10_0000;

const LOG_DATA: u64 = 0x30_0000;
const PHYS_DATA: u64 = 0x28_0000;
const LEN_DATA: u64 = 0x08_0000;

const LOG_CHUNK_LEAF: u64 = 0x10_0000;
const LOG_ROOT_LEAF: u64 = 0x20_0000;
const LOG_FS_NODE: u64 = 0x20_1000;
const LOG_FS_LEAF_A: u64 = 0x20_2000;
const LOG_FS_LEAF_B: u64 = 0x20_3000;
/// An address inside the METADATA chunk that no block was ever written to.
const LOG_UNWRITTEN: u64 = 0x20_8000;

const LOG_EXT_DATA: u64 = 0x30_0000;
const LOG_EXT_SPARSE: u64 = 0x30_2000;
const LOG_EXT_PREALLOC: u64 = 0x30_3000;

// ---------------------------------------------------------------------------
// Contents
// ---------------------------------------------------------------------------

const INO_ROOT: u64 = 256;
const INO_HELLO: u64 = 257;
const INO_SUB: u64 = 258;
const INO_DATA: u64 = 259;
const INO_LINK: u64 = 260;
const INO_SPARSE: u64 = 261;
const INO_PREALLOC: u64 = 262;

const HELLO_TEXT: &[u8] = b"Hello, Btrfs!\n";
const LINK_TARGET: &[u8] = b"hello.txt";

/// Length of `sub/data.bin`, and of its single extent.
const DATA_LEN: usize = 8192;
/// Logical size of `sparse.bin`: an 8 KiB hole followed by a 4 KiB extent.
const SPARSE_SIZE: u64 = 12288;
/// Where `sparse.bin`'s only extent begins within the file.
const SPARSE_TAIL_OFF: u64 = 8192;
/// Length of that extent.
const SPARSE_TAIL_LEN: usize = 4096;
/// Length of `prealloc.bin`.
const PREALLOC_LEN: usize = 4096;
/// Byte written across `prealloc.bin`'s *physical* reservation.
///
/// Non-zero on purpose: a `PREALLOC` extent must read as zeroes, and if the
/// bytes on disk were also zeroes the test would pass even for a driver that
/// wrongly read them.
const PREALLOC_GARBAGE: u8 = 0xAB;

/// Distinct values in every numeric inode field, so a builder/parser
/// disagreement about *which* offset holds *which* field shows up as a wrong
/// value rather than as two zeroes that happen to match.
const TEST_UID: u32 = 1000;
const TEST_GID: u32 = 1001;
const ATIME_SEC: u64 = 1_000;
const ATIME_NSEC: u32 = 11;
const CTIME_SEC: u64 = 2_000;
const CTIME_NSEC: u32 = 22;
const MTIME_SEC: u64 = 3_000;
const MTIME_NSEC: u32 = 33;
const OTIME_SEC: u64 = 4_000;
const OTIME_NSEC: u32 = 44;

/// Volume UUID; arbitrary, and never checked by the driver.
const FSID: [u8; 16] = [
    0x5B, 0x71, 0x0F, 0xAC, 0x2E, 0x44, 0x4D, 0x11, 0x9A, 0x33, 0x0E, 0xC1, 0x77, 0x88, 0x52, 0x0D,
];

/// Volume label written into the superblock.
const LABEL: &[u8] = b"btrfs-selftest";

/// The five entries in the root directory, in creation order.
///
/// The `DIR_INDEX` offset *is* the creation order, so this table doubles as
/// the expected result of `readdir("/")`.
const ROOT_ENTRIES: [(u64, &[u8], u64, u8); 5] = [
    (2, b"hello.txt", INO_HELLO, FT_REG_FILE),
    (3, b"sub", INO_SUB, FT_DIR),
    (4, b"link", INO_LINK, FT_SYMLINK),
    (5, b"sparse.bin", INO_SPARSE, FT_REG_FILE),
    (6, b"prealloc.bin", INO_PREALLOC, FT_REG_FILE),
];

/// Total items across both FS-tree leaves; the expected length of a full walk.
const FS_TREE_ITEMS: usize = 31;

/// Byte `i` of `sub/data.bin`.
fn data_byte(i: usize) -> u8 {
    u8::try_from(i.wrapping_mul(31).wrapping_add(7) % 256).unwrap_or(0)
}

/// Byte `i` of `sparse.bin`'s tail extent.
fn sparse_byte(i: usize) -> u8 {
    u8::try_from(i.wrapping_mul(17).wrapping_add(113) % 256).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Byte-writing helpers
// ---------------------------------------------------------------------------

/// Copy `bytes` into `image` at `offset`, ignoring an out-of-range offset.
///
/// Silently ignoring is right for a *builder*: indexing would panic, and a
/// panic at boot takes the kernel down instead of reporting a failed check.
/// A builder bug then surfaces as a parse failure a few lines later, which is
/// a failed test rather than a dead machine.
fn put(image: &mut [u8], offset: usize, bytes: &[u8]) {
    if let Some(dst) = image.get_mut(offset..offset.saturating_add(bytes.len())) {
        dst.copy_from_slice(bytes);
    }
}

/// Write one byte at `offset`, ignoring an out-of-range offset.
fn put_u8(image: &mut [u8], offset: usize, value: u8) {
    if let Some(dst) = image.get_mut(offset) {
        *dst = value;
    }
}

/// XOR every bit of one byte, to corrupt it beyond any accidental match.
fn flip(image: &mut [u8], offset: usize) {
    if let Some(b) = image.get_mut(offset) {
        *b ^= 0xFF;
    }
}

/// `slice.len()` as a `u64`, saturating rather than casting.
fn u64_len(slice: &[u8]) -> u64 {
    u64::try_from(slice.len()).unwrap_or(u64::MAX)
}

/// `slice.len()` as a `u16`, saturating rather than casting.
fn u16_len(slice: &[u8]) -> u16 {
    u16::try_from(slice.len()).unwrap_or(u16::MAX)
}

/// A `usize` as a `u64`, saturating rather than casting.
///
/// `as` would be lossless on every target this kernel builds for, which is
/// exactly why it is banned: the cast that is fine today is the one nobody
/// re-examines when a 32-bit target appears.
fn to_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// A `u64` as a `usize`, saturating rather than casting.
fn to_usize(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// Translate a logical address to a physical one, from the layout table.
///
/// Deliberately *not* [`ChunkMap::map`]: the builder must place blocks where
/// the format documentation says they go, and the driver must independently
/// arrive at the same answer by reading the chunk items. Sharing one
/// implementation would make that agreement automatic and prove nothing.
fn phys_of(logical: u64) -> KernelResult<usize> {
    let (log, phys, len) = if logical >= LOG_DATA {
        (LOG_DATA, PHYS_DATA, LEN_DATA)
    } else if logical >= LOG_META {
        (LOG_META, PHYS_META, LEN_META)
    } else {
        (LOG_SYS, PHYS_SYS, LEN_SYS)
    };
    let delta = logical
        .checked_sub(log)
        .ok_or(KernelError::InvalidArgument)?;
    if delta >= len {
        return Err(KernelError::InvalidArgument);
    }
    let physical = phys
        .checked_add(delta)
        .ok_or(KernelError::InvalidArgument)?;
    usize::try_from(physical).map_err(|_| KernelError::InvalidArgument)
}

// ---------------------------------------------------------------------------
// On-disk item builders
// ---------------------------------------------------------------------------

/// Serialise a `btrfs_disk_key`: objectid(8), type(1), offset(8).
fn key_bytes(key: &Key) -> Vec<u8> {
    let mut out = Vec::with_capacity(KEY_LEN);
    out.extend_from_slice(&key.objectid.to_le_bytes());
    out.push(key.item_type);
    out.extend_from_slice(&key.offset.to_le_bytes());
    out
}

/// A `btrfs_inode_item`, built by appending fields in on-disk order.
///
/// Appending rather than writing at computed offsets is what makes this
/// checkable against the struct definition by reading straight down.
fn inode_item(size: u64, nlink: u32, mode: u32, nbytes: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(160);
    v.extend_from_slice(&GEN.to_le_bytes()); // generation @0
    v.extend_from_slice(&GEN.to_le_bytes()); // transid @8
    v.extend_from_slice(&size.to_le_bytes()); // size @16
    v.extend_from_slice(&nbytes.to_le_bytes()); // nbytes @24
    v.extend_from_slice(&0u64.to_le_bytes()); // block_group @32
    v.extend_from_slice(&nlink.to_le_bytes()); // nlink @40
    v.extend_from_slice(&TEST_UID.to_le_bytes()); // uid @44
    v.extend_from_slice(&TEST_GID.to_le_bytes()); // gid @48
    v.extend_from_slice(&mode.to_le_bytes()); // mode @52
    v.extend_from_slice(&0u64.to_le_bytes()); // rdev @56
    v.extend_from_slice(&0u64.to_le_bytes()); // flags @64
    v.extend_from_slice(&0u64.to_le_bytes()); // sequence @72
    v.resize(112, 0); // reserved[4] @80
    v.extend_from_slice(&ATIME_SEC.to_le_bytes()); // atime @112
    v.extend_from_slice(&ATIME_NSEC.to_le_bytes());
    v.extend_from_slice(&CTIME_SEC.to_le_bytes()); // ctime @124
    v.extend_from_slice(&CTIME_NSEC.to_le_bytes());
    v.extend_from_slice(&MTIME_SEC.to_le_bytes()); // mtime @136
    v.extend_from_slice(&MTIME_NSEC.to_le_bytes());
    v.extend_from_slice(&OTIME_SEC.to_le_bytes()); // otime @148
    v.extend_from_slice(&OTIME_NSEC.to_le_bytes());
    v
}

/// A `btrfs_inode_ref`: index(8), `name_len`(2), name.
///
/// The driver never parses these — `..` is resolved from the path the caller
/// used, not from a backref — but they are present because a real FS tree has
/// them, and their keys sort between the inode item and its directory entries.
/// Omitting them would make the search-and-skip logic easier than reality.
fn inode_ref(index: u64, name: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&index.to_le_bytes());
    v.extend_from_slice(&u16_len(name).to_le_bytes());
    v.extend_from_slice(name);
    v
}

/// A `btrfs_dir_item` body: location key, transid, `data_len`, `name_len`,
/// type, name.
fn dir_item(location: &Key, name: &[u8], ftype: u8) -> Vec<u8> {
    let mut v = key_bytes(location); // @0..17
    v.extend_from_slice(&GEN.to_le_bytes()); // transid @17
    v.extend_from_slice(&0u16.to_le_bytes()); // data_len @25 (xattrs only)
    v.extend_from_slice(&u16_len(name).to_le_bytes()); // name_len @27
    v.push(ftype); // type @29
    v.extend_from_slice(name); // @30
    v
}

/// A `btrfs_file_extent_item` holding its data inline.
fn inline_extent(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&GEN.to_le_bytes()); // generation @0
    v.extend_from_slice(&u64_len(data).to_le_bytes()); // ram_bytes @8
    v.push(0); // compression @16
    v.push(0); // encryption @17
    v.extend_from_slice(&0u16.to_le_bytes()); // other_encoding @18
    v.push(FILE_EXTENT_INLINE); // type @20
    v.extend_from_slice(data); // @21
    v
}

/// A `btrfs_file_extent_item` pointing at an extent elsewhere.
fn regular_extent(disk_bytenr: u64, offset: u64, num_bytes: u64, prealloc: bool) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&GEN.to_le_bytes()); // generation @0
    v.extend_from_slice(&num_bytes.to_le_bytes()); // ram_bytes @8
    v.push(0); // compression @16
    v.push(0); // encryption @17
    v.extend_from_slice(&0u16.to_le_bytes()); // other_encoding @18
    v.push(if prealloc {
        FILE_EXTENT_PREALLOC
    } else {
        FILE_EXTENT_REG
    }); // type @20
    v.extend_from_slice(&disk_bytenr.to_le_bytes()); // disk_bytenr @21
    v.extend_from_slice(&num_bytes.to_le_bytes()); // disk_num_bytes @29
    v.extend_from_slice(&offset.to_le_bytes()); // offset @37
    v.extend_from_slice(&num_bytes.to_le_bytes()); // num_bytes @45
    v
}

/// A `btrfs_root_item`, in the short (pre-UUID) 239-byte form.
fn root_item(bytenr: u64, generation: u64, level: u8, root_dirid: u64) -> Vec<u8> {
    let mut v = inode_item(0, 1, 0o040_755, 0); // inode @0..160
    v.extend_from_slice(&generation.to_le_bytes()); // generation @160
    v.extend_from_slice(&root_dirid.to_le_bytes()); // root_dirid @168
    v.extend_from_slice(&bytenr.to_le_bytes()); // bytenr @176
    v.extend_from_slice(&0u64.to_le_bytes()); // byte_limit @184
    v.extend_from_slice(&0u64.to_le_bytes()); // bytes_used @192
    v.extend_from_slice(&0u64.to_le_bytes()); // last_snapshot @200
    v.extend_from_slice(&0u64.to_le_bytes()); // flags @208
    v.extend_from_slice(&1u32.to_le_bytes()); // refs @216
    v.extend_from_slice(&key_bytes(&Key::default())); // drop_progress @220
    v.push(0); // drop_level @237
    v.push(level); // level @238
    v
}

/// A `btrfs_chunk` with a single stripe.
fn chunk_item(length: u64, flags: u64, devid: u64, physical: u64, num_stripes: u16) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&length.to_le_bytes()); // length @0
    v.extend_from_slice(&EXTENT_TREE_OBJECTID.to_le_bytes()); // owner @8
    v.extend_from_slice(&65_536u64.to_le_bytes()); // stripe_len @16
    v.extend_from_slice(&flags.to_le_bytes()); // type @24
    v.extend_from_slice(&4_096u32.to_le_bytes()); // io_align @32
    v.extend_from_slice(&4_096u32.to_le_bytes()); // io_width @36
    v.extend_from_slice(&SECTORSIZE.to_le_bytes()); // sector_size @40
    v.extend_from_slice(&num_stripes.to_le_bytes()); // num_stripes @44
    v.extend_from_slice(&0u16.to_le_bytes()); // sub_stripes @46

    // One 32-byte stripe per `num_stripes`: devid(8), offset(8), dev_uuid(16).
    // The count in the header and the number of stripes actually written must
    // agree, or the item is shorter than it claims and the parser is right to
    // refuse it — which is exactly what an earlier version of this builder did
    // by writing the count but always emitting one stripe.
    //
    // Stripes past the first are given a *different* devid and offset on
    // purpose. On a non-striped profile they are redundant copies that the
    // driver must ignore in favour of stripe 0; giving them the same values
    // would let a parser that read the wrong stripe pass anyway.
    for i in 0..num_stripes {
        let bump = u64::from(i).saturating_mul(0x10_0000);
        v.extend_from_slice(&devid.saturating_add(u64::from(i)).to_le_bytes());
        v.extend_from_slice(&physical.saturating_add(bump).to_le_bytes());
        v.extend_from_slice(&[0u8; 16]); // dev_uuid
    }
    v
}

/// A `btrfs_dev_item`, 98 bytes, of which only the leading devid is set.
///
/// Present solely so the chunk tree holds an item that sorts *before* the
/// chunks: the mount searches from `(256, CHUNK_ITEM, 0)` and must land past
/// this one, which an empty chunk tree would never test.
fn dev_item() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&DEVID.to_le_bytes());
    v.resize(98, 0);
    v
}

// ---------------------------------------------------------------------------
// Tree-block builders
// ---------------------------------------------------------------------------

/// Write the 101-byte `btrfs_header` common to leaves and internal nodes.
fn put_header(block: &mut [u8], bytenr: u64, generation: u64, owner: u64, nritems: u32, level: u8) {
    put(block, 32, &FSID); // fsid @32
    put(block, 48, &bytenr.to_le_bytes()); // bytenr @48
    put(block, 56, &0u64.to_le_bytes()); // flags @56
    put(block, 64, &FSID); // chunk_tree_uuid @64 (16 bytes)
    put(block, 80, &generation.to_le_bytes()); // generation @80
    put(block, 88, &owner.to_le_bytes()); // owner @88
    put(block, 96, &nritems.to_le_bytes()); // nritems @96
    put_u8(block, 100, level); // level @100
}

/// Build a leaf: header, an item array growing down, item data growing up.
///
/// The items are sorted here rather than by the caller because two of the key
/// types carry a *runtime* value in their offset — `DIR_ITEM` keys are hashed
/// from the name — so the caller cannot write them down in order. Strictly
/// increasing keys are then checked, because a duplicate key is not a
/// buildable leaf (colliding names share one item, concatenated in its body)
/// and would otherwise produce an image whose search results depend on which
/// of two equal keys the binary search happened to land on.
fn build_leaf(
    bytenr: u64,
    generation: u64,
    owner: u64,
    items: &[(Key, Vec<u8>)],
) -> KernelResult<Vec<u8>> {
    let mut sorted: Vec<(Key, &[u8])> = items.iter().map(|(k, v)| (*k, v.as_slice())).collect();
    sorted.sort_by_key(|(k, _)| k.sort_tuple());
    for pair in sorted.windows(2) {
        let (Some((a, _)), Some((b, _))) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if a.sort_tuple() >= b.sort_tuple() {
            return Err(KernelError::InvalidArgument);
        }
    }

    let mut block = vec![0u8; NODESIZE];
    let nritems = u32::try_from(sorted.len()).map_err(|_| KernelError::InvalidArgument)?;
    put_header(&mut block, bytenr, generation, owner, nritems, 0);

    // Data is placed from the end of the block backwards, so item 0's payload
    // sits highest. `item.offset` is measured from the end of the header, not
    // from the start of the block.
    let mut cursor = NODESIZE;
    for (i, (key, data)) in sorted.iter().enumerate() {
        cursor = cursor
            .checked_sub(data.len())
            .ok_or(KernelError::InvalidArgument)?;
        let slot = i
            .checked_mul(ITEM_LEN)
            .and_then(|v| v.checked_add(HEADER_LEN))
            .ok_or(KernelError::InvalidArgument)?;
        let slot_end = slot
            .checked_add(ITEM_LEN)
            .ok_or(KernelError::InvalidArgument)?;
        if slot_end > cursor {
            // The item array has grown into the data; the leaf is overfull.
            return Err(KernelError::InvalidArgument);
        }

        let rel = u32::try_from(cursor.saturating_sub(HEADER_LEN))
            .map_err(|_| KernelError::InvalidArgument)?;
        let size = u32::try_from(data.len()).map_err(|_| KernelError::InvalidArgument)?;
        put(&mut block, slot, &key_bytes(key));
        put(&mut block, slot.saturating_add(KEY_LEN), &rel.to_le_bytes());
        put(
            &mut block,
            slot.saturating_add(KEY_LEN).saturating_add(4),
            &size.to_le_bytes(),
        );
        put(&mut block, cursor, data);
    }

    seal_csum(&mut block)?;
    Ok(block)
}

/// Build an internal node from `(first key of subtree, blockptr, generation)`.
fn build_internal(
    bytenr: u64,
    generation: u64,
    owner: u64,
    level: u8,
    children: &[(Key, u64, u64)],
) -> KernelResult<Vec<u8>> {
    let mut block = vec![0u8; NODESIZE];
    let nritems = u32::try_from(children.len()).map_err(|_| KernelError::InvalidArgument)?;
    put_header(&mut block, bytenr, generation, owner, nritems, level);

    for (i, (key, blockptr, child_gen)) in children.iter().enumerate() {
        let slot = i
            .checked_mul(KEY_PTR_LEN)
            .and_then(|v| v.checked_add(HEADER_LEN))
            .ok_or(KernelError::InvalidArgument)?;
        if slot.saturating_add(KEY_PTR_LEN) > NODESIZE {
            return Err(KernelError::InvalidArgument);
        }
        put(&mut block, slot, &key_bytes(key));
        put(
            &mut block,
            slot.saturating_add(KEY_LEN),
            &blockptr.to_le_bytes(),
        );
        put(
            &mut block,
            slot.saturating_add(KEY_LEN).saturating_add(8),
            &child_gen.to_le_bytes(),
        );
    }

    seal_csum(&mut block)?;
    Ok(block)
}

/// Build the 4096-byte superblock, given the bootstrap chunk array.
fn build_superblock(sys_array: &[u8]) -> KernelResult<Vec<u8>> {
    let incompat = FEATURE_INCOMPAT_MIXED_BACKREF
        | FEATURE_INCOMPAT_BIG_METADATA
        | FEATURE_INCOMPAT_EXTENDED_IREF
        | FEATURE_INCOMPAT_SKINNY_METADATA
        | FEATURE_INCOMPAT_NO_HOLES;

    let mut sb = vec![0u8; SUPER_INFO_SIZE];
    put(&mut sb, 0x20, &FSID); // fsid
    put(&mut sb, 0x30, &PHYS_SB.to_le_bytes()); // bytenr
    put(&mut sb, 0x40, &MAGIC.to_le_bytes()); // magic
    put(&mut sb, 0x48, &GEN.to_le_bytes()); // generation
    put(&mut sb, 0x50, &LOG_ROOT_LEAF.to_le_bytes()); // root
    put(&mut sb, 0x58, &LOG_CHUNK_LEAF.to_le_bytes()); // chunk_root
    put(&mut sb, 0x60, &0u64.to_le_bytes()); // log_root
    put(&mut sb, 0x70, &TOTAL_BYTES.to_le_bytes()); // total_bytes
    put(&mut sb, 0x78, &0x10_0000u64.to_le_bytes()); // bytes_used
    put(&mut sb, 0x80, &6u64.to_le_bytes()); // root_dir_objectid
    put(&mut sb, 0x88, &1u64.to_le_bytes()); // num_devices
    put(&mut sb, 0x90, &SECTORSIZE.to_le_bytes()); // sectorsize
    let nodesize = u32::try_from(NODESIZE).map_err(|_| KernelError::InvalidArgument)?;
    put(&mut sb, 0x94, &nodesize.to_le_bytes()); // nodesize
    put(&mut sb, 0x98, &nodesize.to_le_bytes()); // leafsize (legacy)
    put(&mut sb, 0x9C, &SECTORSIZE.to_le_bytes()); // stripesize
    let sys_len = u32::try_from(sys_array.len()).map_err(|_| KernelError::InvalidArgument)?;
    put(&mut sb, 0xA0, &sys_len.to_le_bytes()); // sys_chunk_array_size
    put(&mut sb, 0xA4, &GEN.to_le_bytes()); // chunk_root_generation
    put(&mut sb, 0xBC, &incompat.to_le_bytes()); // incompat_flags
    put(&mut sb, 0xC4, &0u16.to_le_bytes()); // csum_type = CRC32C
    put_u8(&mut sb, 0xC6, 0); // root_level
    put_u8(&mut sb, 0xC7, 0); // chunk_root_level
    put_u8(&mut sb, 0xC8, 0); // log_root_level
    put(&mut sb, 0xC9, &dev_item()); // dev_item
    put(&mut sb, 0x12B, LABEL); // label
    put(&mut sb, 0x32B, sys_array); // sys_chunk_array

    seal_csum(&mut sb)?;
    Ok(sb)
}

// ---------------------------------------------------------------------------
// The image
// ---------------------------------------------------------------------------

/// The items of FS-tree leaf A: the root directory, `hello.txt`, and `sub`.
fn fs_leaf_a_items() -> Vec<(Key, Vec<u8>)> {
    let mut items: Vec<(Key, Vec<u8>)> = Vec::new();

    items.push((
        Key::new(INO_ROOT, INODE_ITEM_KEY, 0),
        inode_item(0, 1, 0o040_755, 0),
    ));
    items.push((
        Key::new(INO_ROOT, INODE_REF_KEY, INO_ROOT),
        inode_ref(0, b".."),
    ));

    for (index, name, ino, ftype) in ROOT_ENTRIES {
        let location = Key::new(ino, INODE_ITEM_KEY, 0);
        items.push((
            Key::new(INO_ROOT, DIR_ITEM_KEY, name_hash(name)),
            dir_item(&location, name, ftype),
        ));
        items.push((
            Key::new(INO_ROOT, DIR_INDEX_KEY, index),
            dir_item(&location, name, ftype),
        ));
    }

    items.push((
        Key::new(INO_HELLO, INODE_ITEM_KEY, 0),
        inode_item(u64_len(HELLO_TEXT), 1, 0o100_644, 0),
    ));
    items.push((
        Key::new(INO_HELLO, INODE_REF_KEY, INO_ROOT),
        inode_ref(2, b"hello.txt"),
    ));
    items.push((
        Key::new(INO_HELLO, EXTENT_DATA_KEY, 0),
        inline_extent(HELLO_TEXT),
    ));

    items.push((
        Key::new(INO_SUB, INODE_ITEM_KEY, 0),
        inode_item(0, 1, 0o040_755, 0),
    ));
    items.push((
        Key::new(INO_SUB, INODE_REF_KEY, INO_ROOT),
        inode_ref(3, b"sub"),
    ));
    let data_loc = Key::new(INO_DATA, INODE_ITEM_KEY, 0);
    items.push((
        Key::new(INO_SUB, DIR_ITEM_KEY, name_hash(b"data.bin")),
        dir_item(&data_loc, b"data.bin", FT_REG_FILE),
    ));
    items.push((
        Key::new(INO_SUB, DIR_INDEX_KEY, 2),
        dir_item(&data_loc, b"data.bin", FT_REG_FILE),
    ));

    items
}

/// The items of FS-tree leaf B: the four files with interesting extents.
fn fs_leaf_b_items() -> KernelResult<Vec<(Key, Vec<u8>)>> {
    let data_len = u64::try_from(DATA_LEN).map_err(|_| KernelError::InvalidArgument)?;
    let tail_len = u64::try_from(SPARSE_TAIL_LEN).map_err(|_| KernelError::InvalidArgument)?;
    let prealloc_len = u64::try_from(PREALLOC_LEN).map_err(|_| KernelError::InvalidArgument)?;

    let items: Vec<(Key, Vec<u8>)> = vec![
        // sub/data.bin — one plain extent, the ordinary case.
        (
            Key::new(INO_DATA, INODE_ITEM_KEY, 0),
            inode_item(data_len, 1, 0o100_644, data_len),
        ),
        (
            Key::new(INO_DATA, INODE_REF_KEY, INO_SUB),
            inode_ref(2, b"data.bin"),
        ),
        (
            Key::new(INO_DATA, EXTENT_DATA_KEY, 0),
            regular_extent(LOG_EXT_DATA, 0, data_len, false),
        ),
        // link — a symlink, whose target is the file's *contents*, stored
        // inline in the extent item rather than in any dedicated field.
        (
            Key::new(INO_LINK, INODE_ITEM_KEY, 0),
            inode_item(u64_len(LINK_TARGET), 1, 0o120_777, 0),
        ),
        (
            Key::new(INO_LINK, INODE_REF_KEY, INO_ROOT),
            inode_ref(4, b"link"),
        ),
        (
            Key::new(INO_LINK, EXTENT_DATA_KEY, 0),
            inline_extent(LINK_TARGET),
        ),
        // sparse.bin — no `EXTENT_DATA` item covers [0, 8192): with NO_HOLES
        // set, that absence *is* the hole, and the reader must invent the
        // zeroes rather than find them written down anywhere.
        (
            Key::new(INO_SPARSE, INODE_ITEM_KEY, 0),
            inode_item(SPARSE_SIZE, 1, 0o100_644, tail_len),
        ),
        (
            Key::new(INO_SPARSE, INODE_REF_KEY, INO_ROOT),
            inode_ref(5, b"sparse.bin"),
        ),
        (
            Key::new(INO_SPARSE, EXTENT_DATA_KEY, SPARSE_TAIL_OFF),
            regular_extent(LOG_EXT_SPARSE, 0, tail_len, false),
        ),
        // prealloc.bin — a real extent whose physical bytes are deliberately
        // *not* zero, so "PREALLOC reads as zeroes" is a genuine test.
        (
            Key::new(INO_PREALLOC, INODE_ITEM_KEY, 0),
            inode_item(prealloc_len, 1, 0o100_644, prealloc_len),
        ),
        (
            Key::new(INO_PREALLOC, INODE_REF_KEY, INO_ROOT),
            inode_ref(6, b"prealloc.bin"),
        ),
        (
            Key::new(INO_PREALLOC, EXTENT_DATA_KEY, 0),
            regular_extent(LOG_EXT_PREALLOC, 0, prealloc_len, true),
        ),
    ];

    Ok(items)
}

/// Lay out the whole volume.
fn build_image() -> KernelResult<Vec<u8>> {
    let mut image = vec![0u8; IMAGE_LEN];

    // --- the chunk tree, and the bootstrap copy of its system chunk ---
    let sys_chunk = chunk_item(LEN_SYS, BLOCK_GROUP_SYSTEM, DEVID, PHYS_SYS, 1);
    let chunk_items = [
        (Key::new(DEVID, DEV_ITEM_KEY, DEVID), dev_item()),
        (
            Key::new(FIRST_CHUNK_TREE_OBJECTID, CHUNK_ITEM_KEY, LOG_SYS),
            sys_chunk.clone(),
        ),
        (
            Key::new(FIRST_CHUNK_TREE_OBJECTID, CHUNK_ITEM_KEY, LOG_META),
            chunk_item(LEN_META, BLOCK_GROUP_METADATA, DEVID, PHYS_META, 1),
        ),
        (
            Key::new(FIRST_CHUNK_TREE_OBJECTID, CHUNK_ITEM_KEY, LOG_DATA),
            chunk_item(LEN_DATA, BLOCK_GROUP_DATA, DEVID, PHYS_DATA, 1),
        ),
    ];
    let chunk_leaf = build_leaf(LOG_CHUNK_LEAF, GEN, CHUNK_TREE_OBJECTID, &chunk_items)?;
    put(&mut image, phys_of(LOG_CHUNK_LEAF)?, &chunk_leaf);

    let mut sys_array = key_bytes(&Key::new(
        FIRST_CHUNK_TREE_OBJECTID,
        CHUNK_ITEM_KEY,
        LOG_SYS,
    ));
    sys_array.extend_from_slice(&sys_chunk);

    // --- the root tree ---
    let root_items = [
        (
            Key::new(ROOT_TREE_OBJECTID, ROOT_ITEM_KEY, 0),
            root_item(LOG_ROOT_LEAF, GEN, 0, 0),
        ),
        (
            Key::new(FS_TREE_OBJECTID, ROOT_ITEM_KEY, 0),
            root_item(LOG_FS_NODE, GEN, 1, INO_ROOT),
        ),
    ];
    let root_leaf = build_leaf(LOG_ROOT_LEAF, GEN, ROOT_TREE_OBJECTID, &root_items)?;
    put(&mut image, phys_of(LOG_ROOT_LEAF)?, &root_leaf);

    // --- the filesystem tree, two levels deep ---
    let leaf_a = build_leaf(LOG_FS_LEAF_A, GEN, FS_TREE_OBJECTID, &fs_leaf_a_items())?;
    let leaf_b = build_leaf(LOG_FS_LEAF_B, GEN, FS_TREE_OBJECTID, &fs_leaf_b_items()?)?;
    put(&mut image, phys_of(LOG_FS_LEAF_A)?, &leaf_a);
    put(&mut image, phys_of(LOG_FS_LEAF_B)?, &leaf_b);

    let fs_node = build_internal(
        LOG_FS_NODE,
        GEN,
        FS_TREE_OBJECTID,
        1,
        &[
            (Key::new(INO_ROOT, INODE_ITEM_KEY, 0), LOG_FS_LEAF_A, GEN),
            (Key::new(INO_DATA, INODE_ITEM_KEY, 0), LOG_FS_LEAF_B, GEN),
        ],
    )?;
    put(&mut image, phys_of(LOG_FS_NODE)?, &fs_node);

    // --- file data ---
    let data: Vec<u8> = (0..DATA_LEN).map(data_byte).collect();
    put(&mut image, phys_of(LOG_EXT_DATA)?, &data);

    let tail: Vec<u8> = (0..SPARSE_TAIL_LEN).map(sparse_byte).collect();
    put(&mut image, phys_of(LOG_EXT_SPARSE)?, &tail);

    let garbage = vec![PREALLOC_GARBAGE; PREALLOC_LEN];
    put(&mut image, phys_of(LOG_EXT_PREALLOC)?, &garbage);

    // --- the superblock, last, because it describes everything above ---
    let sb = build_superblock(&sys_array)?;
    put(
        &mut image,
        usize::try_from(PHYS_SB).map_err(|_| KernelError::InvalidArgument)?,
        &sb,
    );

    Ok(image)
}

/// Mount an image through [`MemorySource`].
fn mount_image(image: Vec<u8>) -> KernelResult<BtrfsFs> {
    BtrfsFs::open_source(Box::new(MemorySource::new(image)))
}

/// Overwrite bytes in the superblock and re-seal its checksum.
///
/// Re-sealing matters: without it every superblock edit would be caught by the
/// checksum, so a test meaning to check (say) feature-flag rejection would
/// actually be checking the checksum again.
fn patch_superblock(image: &mut [u8], offset: usize, bytes: &[u8]) -> KernelResult<()> {
    let start = usize::try_from(PHYS_SB).map_err(|_| KernelError::InvalidArgument)?;
    put(image, start.saturating_add(offset), bytes);
    let end = start.saturating_add(SUPER_INFO_SIZE);
    let block = image
        .get_mut(start..end)
        .ok_or(KernelError::InvalidArgument)?;
    seal_csum(block)
}

/// Overwrite bytes in a tree block and re-seal its checksum, for the same
/// reason [`patch_superblock`] does.
fn patch_block(image: &mut [u8], logical: u64, offset: usize, bytes: &[u8]) -> KernelResult<()> {
    let start = phys_of(logical)?;
    put(image, start.saturating_add(offset), bytes);
    let end = start.saturating_add(NODESIZE);
    let block = image
        .get_mut(start..end)
        .ok_or(KernelError::InvalidArgument)?;
    seal_csum(block)
}

// ---------------------------------------------------------------------------
// Check harness
// ---------------------------------------------------------------------------

/// Counts passing checks and records failing ones without stopping.
///
/// A failed assertion prints and continues; only a genuine `Err` from the
/// driver (propagated by `?` at the call site) ends a test group early,
/// because after that the group's later steps have nothing valid to run
/// against.
///
/// The distinction earns its keep. This suite runs *in the boot test*, and a
/// boot cycle is several minutes — so a harness that stops at the first
/// failure turns "fix N problems" into N boot cycles, and worse, reports a
/// green-looking six-failure suite as one failure. The first real run of this
/// file failed on a builder bug in group 3 of 7, which meant groups 4-7 were
/// silently never executed at all.
struct Checks {
    passed: u32,
    failed: u32,
}

/// One group of the self-test.
///
/// Runs its checks against the shared tally, returning `Err` only for a hard
/// error that makes its *own* remaining steps meaningless — a parse that could
/// not proceed, so everything downstream of it in that group would be asserting
/// against nothing. A failed assertion is not an error and does not stop the
/// group.
type TestGroup = fn(&mut Checks) -> KernelResult<()>;

impl Checks {
    const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    /// Assert `cond`, reporting `what` on failure.
    ///
    /// Never panics: a self-test that panics takes the kernel down before it
    /// can print the rest of its results, so one broken check would hide every
    /// check after it. Returns nothing, for the same reason — the tally is what
    /// decides the suite's verdict, so there is no failure for a caller to
    /// propagate and no `?` to write at the call site.
    fn check(&mut self, cond: bool, what: &str) {
        if cond {
            self.passed = self.passed.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
            serial_println!("[btrfs] SELF-TEST FAILED: {}", what);
        }
    }

    /// Assert that `got` failed with exactly `want`.
    ///
    /// A helper rather than `got == Err(want)` because most of the success
    /// types involved — `Superblock`, `Node`, `FileExtent`, `BtrfsFs` — do not
    /// implement `PartialEq`, and deriving it on production types purely so a
    /// test could spell an equality would be the test dictating the driver's
    /// API. Comparing the error alone is also the stricter check: it says
    /// *which* rejection happened, not merely that one did.
    fn check_err<T>(&mut self, got: KernelResult<T>, want: KernelError, what: &str) {
        self.check(got.err() == Some(want), what);
    }

    /// Assert two byte slices are equal, reporting the first difference.
    fn check_bytes(&mut self, got: &[u8], want: &[u8], what: &str) {
        if got.len() != want.len() {
            self.failed = self.failed.saturating_add(1);
            serial_println!(
                "[btrfs] SELF-TEST FAILED: {} - length {} != {}",
                what,
                got.len(),
                want.len()
            );
            return;
        }
        for (i, (a, b)) in got.iter().zip(want.iter()).enumerate() {
            if a != b {
                self.failed = self.failed.saturating_add(1);
                serial_println!(
                    "[btrfs] SELF-TEST FAILED: {} - byte {} is {:#04x}, expected {:#04x}",
                    what,
                    i,
                    a,
                    b
                );
                return;
            }
        }
        self.passed = self.passed.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// Scalars, keys and the name hash
// ---------------------------------------------------------------------------

// The `Result` is not superfluous here even though this group happens never to
// fail hard: every group is stored in one `[(&str, TestGroup); 7]` table, so
// they must share a signature. Dropping the wrapper from the one group that
// currently gets away without it would mean the table could no longer hold it.
#[allow(clippy::unnecessary_wraps)]
fn test_primitives(c: &mut Checks) -> KernelResult<()> {
    let buf: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

    c.check(read_u8(&buf, 0) == Ok(0x01), "read_u8");
    c.check(read_u16(&buf, 0) == Ok(0x0201), "read_u16 is little-endian");
    c.check(read_u32(&buf, 0) == Ok(0x0403_0201), "read_u32");
    c.check(read_u64(&buf, 0) == Ok(0x0807_0605_0403_0201), "read_u64");

    // A read that starts inside the buffer but ends outside it is the one that
    // a naive bounds check misses, so it is checked for every width.
    c.check(read_u8(&buf, 8).is_err(), "read_u8 past the end");
    c.check(read_u16(&buf, 7).is_err(), "read_u16 straddling the end");
    c.check(read_u32(&buf, 5).is_err(), "read_u32 straddling the end");
    c.check(read_u64(&buf, 1).is_err(), "read_u64 straddling the end");
    c.check(
        read_u64(&buf, usize::MAX).is_err(),
        "an offset that would overflow when the width is added",
    );

    let key = Key::new(0x1122_3344_5566_7788, 108, 0x00FF_EE00);
    let encoded = key_bytes(&key);
    c.check(encoded.len() == KEY_LEN, "a disk key is 17 bytes");
    c.check(
        Key::parse(&encoded, 0) == Ok(key),
        "a key survives a round trip through its on-disk form",
    );

    // `item_type` is the middle component of the ordering, not a tiebreak:
    // every item of one object is contiguous, sorted by type.
    c.check(
        Key::new(5, 1, 0).sort_tuple() < Key::new(5, 12, 0).sort_tuple(),
        "type orders within one objectid",
    );
    c.check(
        Key::new(5, 108, u64::MAX).sort_tuple() < Key::new(6, 1, 0).sort_tuple(),
        "objectid dominates type and offset",
    );
    c.check(
        Key::new(5, 84, 1).sort_tuple() < Key::new(5, 84, 2).sort_tuple(),
        "offset breaks ties within one type",
    );

    c.check(
        name_hash(b"hello.txt") == name_hash(b"hello.txt"),
        "the name hash is a function",
    );
    c.check(
        name_hash(b"hello.txt") != name_hash(b"hello.txu"),
        "the name hash separates names differing in one byte",
    );
    // The trap this guards: btrfs seeds CRC32C with `~1` and applies no final
    // inversion, whereas the checksum used on tree blocks is the fully
    // conditioned CRC. Using the latter for names would make every directory
    // lookup miss while every checksum still verified.
    c.check(
        u64::from(crate::crypto::crc32c(b"hello.txt")) != name_hash(b"hello.txt"),
        "the name hash is not the block checksum",
    );
    c.check(
        name_hash(b"") == u64::from(crate::crypto::crc32c_raw(!1u32, b"")),
        "the empty name hashes to the bare seed",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Leaf item bodies
// ---------------------------------------------------------------------------

fn test_items(c: &mut Checks) -> KernelResult<()> {
    let body = inode_item(1234, 3, 0o100_640, 4096);
    let inode = InodeItem::parse(&body)?;
    c.check(inode.size == 1234, "inode size");
    c.check(inode.nbytes == 4096, "inode nbytes");
    c.check(inode.nlink == 3, "inode nlink");
    c.check(inode.uid == TEST_UID, "inode uid");
    c.check(inode.gid == TEST_GID, "inode gid");
    c.check(inode.mode == 0o100_640, "inode mode");
    c.check(
        inode.atime_ns
            == ATIME_SEC
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::from(ATIME_NSEC)),
        "atime combines seconds and nanoseconds",
    );
    c.check(
        inode.ctime_ns
            == CTIME_SEC
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::from(CTIME_NSEC)),
        "ctime",
    );
    c.check(
        inode.mtime_ns
            == MTIME_SEC
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::from(MTIME_NSEC)),
        "mtime",
    );
    c.check(
        inode.otime_ns
            == OTIME_SEC
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::from(OTIME_NSEC)),
        "otime",
    );
    c.check(!inode.is_dir() && !inode.is_symlink(), "a regular file");

    // A timestamp beyond the representable range clamps rather than wrapping:
    // wrapping would present a file from the far future as ancient.
    let mut absurd = inode_item(0, 1, 0o100_644, 0);
    put(&mut absurd, 136, &u64::MAX.to_le_bytes());
    c.check(
        InodeItem::parse(&absurd).map(|i| i.mtime_ns) == Ok(u64::MAX),
        "an absurd timestamp saturates instead of wrapping",
    );

    c.check(
        InodeItem::parse(&inode_item(0, 1, 0o040_755, 0)).is_ok_and(|i| i.is_dir()),
        "mode 040000 is a directory",
    );
    c.check(
        InodeItem::parse(&inode_item(0, 1, 0o120_777, 0)).is_ok_and(|i| i.is_symlink()),
        "mode 120000 is a symlink",
    );
    c.check(
        InodeItem::parse(body.get(..159).unwrap_or_default()).is_err(),
        "a truncated inode item is refused, not zero-extended",
    );

    // Two entries packed into one item, which is what a hash collision looks
    // like on disk and why a lookup must compare names after the key matches.
    let loc_a = Key::new(INO_HELLO, INODE_ITEM_KEY, 0);
    let loc_b = Key::new(INO_SUB, INODE_ITEM_KEY, 0);
    let mut packed = dir_item(&loc_a, b"first", FT_REG_FILE);
    packed.extend_from_slice(&dir_item(&loc_b, b"second", FT_DIR));
    let entries = parse_dir_items(&packed)?;
    c.check(entries.len() == 2, "two dir items in one leaf item");
    c.check(
        entries.first().map(|e| e.name.as_slice()) == Some(b"first".as_slice()),
        "first packed name",
    );
    c.check(
        entries.get(1).map(|e| e.name.as_slice()) == Some(b"second".as_slice()),
        "second packed name",
    );
    c.check(
        entries.get(1).map(|e| e.location) == Some(loc_b),
        "the second entry's location survives the packing",
    );
    c.check(
        entries.get(1).map(|e| e.ftype) == Some(FT_DIR),
        "the second entry's type",
    );

    let inline = parse_file_extent(&inline_extent(HELLO_TEXT))?;
    match inline {
        FileExtent::Inline { ram_bytes, data } => {
            c.check(ram_bytes == u64_len(HELLO_TEXT), "inline ram_bytes");
            c.check_bytes(&data, HELLO_TEXT, "inline data");
        }
        FileExtent::Regular { .. } => c.check(false, "inline extent parsed as regular"),
    }

    let regular = parse_file_extent(&regular_extent(LOG_EXT_DATA, 512, 4096, false))?;
    match regular {
        FileExtent::Regular {
            disk_bytenr,
            offset,
            num_bytes,
            prealloc,
        } => {
            c.check(disk_bytenr == LOG_EXT_DATA, "regular disk_bytenr");
            c.check(offset == 512, "regular offset within the extent");
            c.check(num_bytes == 4096, "regular num_bytes");
            c.check(!prealloc, "a regular extent is not prealloc");
        }
        FileExtent::Inline { .. } => c.check(false, "regular extent parsed as inline"),
    }

    c.check(
        matches!(
            parse_file_extent(&regular_extent(LOG_EXT_DATA, 0, 4096, true)),
            Ok(FileExtent::Regular { prealloc: true, .. })
        ),
        "a prealloc extent is flagged as such",
    );

    // Encoded extents are refused rather than returned as-is: the caller
    // cannot tell that what it received is compressed, so passing it through
    // would be a successful read of wrong data.
    let mut compressed = inline_extent(HELLO_TEXT);
    put_u8(&mut compressed, 16, 1);
    c.check_err(
        parse_file_extent(&compressed),
        KernelError::NotSupported,
        "a compressed extent is refused",
    );
    let mut encrypted = inline_extent(HELLO_TEXT);
    put_u8(&mut encrypted, 17, 1);
    c.check_err(
        parse_file_extent(&encrypted),
        KernelError::NotSupported,
        "an encrypted extent is refused",
    );
    let mut encoded = inline_extent(HELLO_TEXT);
    put(&mut encoded, 18, &1u16.to_le_bytes());
    c.check_err(
        parse_file_extent(&encoded),
        KernelError::NotSupported,
        "an otherwise-encoded extent is refused",
    );
    let mut unknown = inline_extent(HELLO_TEXT);
    put_u8(&mut unknown, 20, 9);
    c.check_err(
        parse_file_extent(&unknown),
        KernelError::InvalidArgument,
        "an unknown extent kind is refused",
    );

    let root = RootItem::parse(&root_item(LOG_FS_NODE, GEN, 1, INO_ROOT))?;
    c.check(root.bytenr == LOG_FS_NODE, "root item bytenr");
    c.check(root.generation == GEN, "root item generation");
    c.check(root.level == 1, "root item level");
    c.check(root.root_dirid == INO_ROOT, "root item root_dirid");
    c.check(
        RootItem::parse(&[0u8; 238]).is_err(),
        "a root item shorter than the pre-UUID form is refused",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// The logical-to-physical map
// ---------------------------------------------------------------------------

/// The map the image's chunk tree describes, built by hand.
fn full_map() -> KernelResult<ChunkMap> {
    let mut map = ChunkMap::new();
    map.insert(ChunkEntry {
        logical: LOG_SYS,
        length: LEN_SYS,
        physical: PHYS_SYS,
        devid: DEVID,
        flags: BLOCK_GROUP_SYSTEM,
    })?;
    map.insert(ChunkEntry {
        logical: LOG_META,
        length: LEN_META,
        physical: PHYS_META,
        devid: DEVID,
        flags: BLOCK_GROUP_METADATA,
    })?;
    map.insert(ChunkEntry {
        logical: LOG_DATA,
        length: LEN_DATA,
        physical: PHYS_DATA,
        devid: DEVID,
        flags: BLOCK_GROUP_DATA,
    })?;
    Ok(map)
}

fn test_chunk_map(c: &mut Checks) -> KernelResult<()> {
    let key = Key::new(FIRST_CHUNK_TREE_OBJECTID, CHUNK_ITEM_KEY, LOG_META);
    let item = chunk_item(LEN_META, BLOCK_GROUP_METADATA, DEVID, PHYS_META, 1);
    let (entry, consumed) = parse_chunk_item(&key, &item)?;
    c.check(entry.logical == LOG_META, "chunk logical comes from the key");
    c.check(entry.physical == PHYS_META, "chunk physical is stripe 0");
    c.check(entry.length == LEN_META, "chunk length");
    c.check(entry.devid == DEVID, "chunk devid");
    c.check(consumed == 80, "a one-stripe chunk item is 48 + 32 bytes");

    // Two stripes on a non-striped profile is DUP or RAID1: a second *copy*,
    // not a second half. The item grows by one 32-byte stripe, and the mapping
    // must still come from stripe 0 — the builder gives stripe 1 a different
    // devid and offset precisely so reading the wrong one cannot pass.
    let dup = chunk_item(
        LEN_META,
        BLOCK_GROUP_METADATA | BLOCK_GROUP_DUP,
        DEVID,
        PHYS_META,
        2,
    );
    let (dup_entry, dup_consumed) = parse_chunk_item(&key, &dup)?;
    c.check(
        dup_consumed == 112,
        "the consumed length grows with the stripe count",
    );
    c.check(
        dup_entry.physical == PHYS_META && dup_entry.devid == DEVID,
        "a two-copy chunk maps through stripe 0, not stripe 1",
    );

    // The count in the header is what the parser must trust for the item's
    // length. An item that claims more stripes than it carries is truncated,
    // and reading a stripe out of the bytes that follow it in the leaf would
    // yield a plausible physical address pointing anywhere on the device.
    c.check(
        parse_chunk_item(&key, dup.get(..104).unwrap_or_default()).is_err(),
        "a chunk item shorter than its own stripe count is refused",
    );

    // A striped profile is refused rather than mapped approximately: the
    // wrong bytes would surface as a checksum failure on an unrelated block.
    c.check(
        parse_chunk_item(
            &key,
            &chunk_item(
                LEN_DATA,
                BLOCK_GROUP_DATA | BLOCK_GROUP_RAID0,
                DEVID,
                PHYS_DATA,
                2,
            ),
        ) == Err(KernelError::NotSupported),
        "a RAID0 chunk is refused",
    );
    c.check(
        parse_chunk_item(&Key::new(FIRST_CHUNK_TREE_OBJECTID, ROOT_ITEM_KEY, 0), &item).is_err(),
        "a chunk item under the wrong key type is refused",
    );
    c.check(
        parse_chunk_item(&key, &chunk_item(0, BLOCK_GROUP_METADATA, DEVID, PHYS_META, 1)).is_err(),
        "a zero-length chunk is refused",
    );
    c.check(
        parse_chunk_item(&key, &chunk_item(LEN_META, BLOCK_GROUP_METADATA, DEVID, PHYS_META, 0))
            .is_err(),
        "a chunk with no stripes is refused",
    );
    c.check(
        parse_chunk_item(&key, item.get(..79).unwrap_or_default()).is_err(),
        "a chunk item shorter than its stripe count claims is refused",
    );

    let map = full_map()?;
    c.check(map.len() == 3, "three chunks");
    c.check(!map.is_empty(), "a loaded map is not empty");
    c.check(map.map(LOG_META) == Ok(PHYS_META), "the start of a chunk");
    c.check(
        map.map(LOG_FS_LEAF_B) == Ok(PHYS_META.saturating_add(0x3000)),
        "an address inside a chunk keeps its offset",
    );
    c.check(
        map.map(LOG_SYS.saturating_sub(1)).is_err(),
        "an address below every chunk is unmapped",
    );
    c.check(
        map.map(LOG_DATA.saturating_add(LEN_DATA)).is_err(),
        "an address past the last chunk is unmapped",
    );

    c.check(
        map.map_range(LOG_FS_LEAF_A, to_u64(NODESIZE)) == Ok(PHYS_META.saturating_add(0x2000)),
        "a range inside one chunk maps",
    );
    // The check that matters: a range crossing a chunk boundary is contiguous
    // logically and generally not physically, so mapping only its first byte
    // would read whatever chunk happens to sit next on the device.
    c.check(
        map.map_range(LOG_META.saturating_sub(0x800), 0x1000).is_err(),
        "a range spanning two chunks is refused",
    );
    c.check(
        map.map_range(
            LOG_DATA.saturating_add(LEN_DATA).saturating_sub(16),
            32,
        )
        .is_err(),
        "a range running off the end of the last chunk is refused",
    );

    let mut dup = full_map()?;
    dup.insert(ChunkEntry {
        logical: LOG_META,
        length: LEN_META,
        physical: PHYS_META,
        devid: DEVID,
        flags: BLOCK_GROUP_METADATA,
    })?;
    c.check(
        dup.len() == 3,
        "re-inserting an identical chunk is a no-op, as the two-pass load needs",
    );
    c.check(
        dup.insert(ChunkEntry {
            logical: LOG_META,
            length: LEN_META,
            physical: 0,
            devid: DEVID,
            flags: BLOCK_GROUP_METADATA,
        })
        .is_err(),
        "a conflicting chunk at the same logical address is refused",
    );

    let mut boot = ChunkMap::new();
    let mut sys_array = key_bytes(&Key::new(
        FIRST_CHUNK_TREE_OBJECTID,
        CHUNK_ITEM_KEY,
        LOG_SYS,
    ));
    sys_array.extend_from_slice(&chunk_item(LEN_SYS, BLOCK_GROUP_SYSTEM, DEVID, PHYS_SYS, 1));
    c.check(
        boot.load_sys_array(&sys_array) == Ok(1),
        "the bootstrap array yields one chunk",
    );
    c.check(
        ChunkMap::new().load_sys_array(&[]).is_err(),
        "an empty bootstrap array leaves the chunk tree unreachable",
    );
    c.check(
        ChunkMap::new()
            .load_sys_array(sys_array.get(..90).unwrap_or_default())
            .is_err(),
        "a truncated bootstrap array is refused rather than half-read",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// The superblock
// ---------------------------------------------------------------------------

/// The 4096 bytes of mirror 0, lifted out of an image.
fn superblock_bytes(image: &[u8]) -> KernelResult<Vec<u8>> {
    let start = usize::try_from(PHYS_SB).map_err(|_| KernelError::InvalidArgument)?;
    let end = start
        .checked_add(SUPER_INFO_SIZE)
        .ok_or(KernelError::InvalidArgument)?;
    image
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or(KernelError::InvalidArgument)
}

fn test_superblock(c: &mut Checks) -> KernelResult<()> {
    let image = build_image()?;
    let raw = superblock_bytes(&image)?;
    let sb = Superblock::parse(&raw)?;

    c.check(sb.bytenr == PHYS_SB, "superblock knows where it lives");
    c.check(sb.generation == GEN, "superblock generation");
    c.check(sb.root == LOG_ROOT_LEAF, "root tree address");
    c.check(sb.chunk_root == LOG_CHUNK_LEAF, "chunk tree address");
    c.check(sb.log_root == 0, "no log tree: a clean unmount");
    c.check(sb.total_bytes == TOTAL_BYTES, "total bytes");
    c.check(sb.num_devices == 1, "single device");
    c.check(sb.sectorsize == SECTORSIZE, "sector size");
    c.check(sb.nodesize_usize() == Ok(NODESIZE), "node size");
    c.check(sb.chunk_root_generation == GEN, "chunk root generation");
    c.check(sb.fsid == FSID, "fsid");
    c.check_bytes(&sb.label, LABEL, "label is trimmed at the first NUL");
    c.check(
        sb.sys_chunk_array.len() == KEY_LEN.saturating_add(80),
        "the bootstrap array is one key plus one one-stripe chunk",
    );

    // Magic is checked before anything else, so a wrong one is reported as a
    // bad argument rather than as an I/O error.
    let mut bad_magic = image.clone();
    patch_superblock(&mut bad_magic, 0x40, &0u64.to_le_bytes())?;
    c.check_err(
        Superblock::parse(&superblock_bytes(&bad_magic)?),
        KernelError::InvalidArgument,
        "a superblock without the magic is refused",
    );
    c.check(
        mount_image(bad_magic).is_err(),
        "and the mount fails with it",
    );

    // An unverified checksum is worse than no checksum, because it reports
    // success — so an algorithm we cannot compute is a refusal, not a shrug.
    let mut bad_csum_type = image.clone();
    patch_superblock(&mut bad_csum_type, 0xC4, &2u16.to_le_bytes())?;
    c.check_err(
        Superblock::parse(&superblock_bytes(&bad_csum_type)?),
        KernelError::NotSupported,
        "a SHA-256 volume is refused rather than mounted unverified",
    );

    let mut zstd = image.clone();
    patch_superblock(
        &mut zstd,
        0xBC,
        &FEATURE_INCOMPAT_COMPRESS_ZSTD.to_le_bytes(),
    )?;
    c.check_err(
        Superblock::parse(&superblock_bytes(&zstd)?),
        KernelError::NotSupported,
        "an unsupported incompat flag is refused",
    );

    // The checksum covers everything after its own 32-byte field, so a change
    // anywhere past that is caught even though the field itself is untouched.
    let mut torn = image.clone();
    flip(
        &mut torn,
        usize::try_from(PHYS_SB)
            .map_err(|_| KernelError::InvalidArgument)?
            .saturating_add(0x50),
    );
    c.check_err(
        Superblock::parse(&superblock_bytes(&torn)?),
        KernelError::IoError,
        "an unsealed edit fails the checksum",
    );

    // A copy that disagrees about where it lives is not a superblock we
    // followed a pointer to; it is whatever else sits there with a plausible
    // magic. With only one mirror present, rejecting it means no mount.
    let mut displaced = image.clone();
    patch_superblock(&mut displaced, 0x30, &0x1234_u64.to_le_bytes())?;
    c.check(
        mount_image(displaced).is_err(),
        "a superblock claiming the wrong offset is discarded",
    );

    // Geometry that would size an allocation is bounded before it is used.
    let mut huge_node = image.clone();
    patch_superblock(&mut huge_node, 0x94, &(1u32 << 20).to_le_bytes())?;
    c.check(
        Superblock::parse(&superblock_bytes(&huge_node)?).is_err(),
        "an absurd nodesize is refused before it sizes a buffer",
    );
    let mut odd_sector = image;
    patch_superblock(&mut odd_sector, 0x90, &3000u32.to_le_bytes())?;
    c.check(
        Superblock::parse(&superblock_bytes(&odd_sector)?).is_err(),
        "a sectorsize that is not a power of two is refused",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// The B-tree
// ---------------------------------------------------------------------------

fn test_btree(c: &mut Checks) -> KernelResult<()> {
    let src = MemorySource::new(build_image()?);
    let map = full_map()?;
    let reader = TreeReader::new(&src, &map, NODESIZE);

    let leaf = reader.read_node(LOG_FS_LEAF_A, Some(GEN))?;
    c.check(leaf.header.is_leaf(), "leaf A is a leaf");
    c.check(leaf.header.bytenr == LOG_FS_LEAF_A, "leaf A knows its address");
    c.check(leaf.header.generation == GEN, "leaf A generation");
    c.check(leaf.header.owner == FS_TREE_OBJECTID, "leaf A owner");
    c.check(leaf.nritems() == 19, "leaf A item count");
    c.check(
        leaf.key_at(0) == Ok(Key::new(INO_ROOT, INODE_ITEM_KEY, 0)),
        "leaf A starts at the root directory's inode item",
    );
    c.check(
        leaf.key_at(19).is_err(),
        "a slot past nritems is out of range, not wrapped",
    );
    c.check(
        leaf.child_at(0).is_err(),
        "a leaf has no child pointers to follow",
    );

    let node = reader.read_node(LOG_FS_NODE, Some(GEN))?;
    c.check(!node.header.is_leaf(), "the FS root is an internal node");
    c.check(node.header.level == 1, "and one level above the leaves");
    c.check(node.nritems() == 2, "with two children");
    c.check(
        node.child_at(0) == Ok((LOG_FS_LEAF_A, GEN)),
        "child 0 points at leaf A with its generation",
    );
    c.check(
        node.child_at(1) == Ok((LOG_FS_LEAF_B, GEN)),
        "child 1 points at leaf B",
    );
    c.check(
        node.item_data(0).is_err(),
        "an internal node has no item payloads",
    );

    // The CoW-specific hazard: the previous version of a node is still on disk
    // and still checksums, so only the generation distinguishes it.
    c.check_err(
        reader.read_node(LOG_FS_LEAF_A, Some(GEN.saturating_add(1))),
        KernelError::IoError,
        "a block from the wrong generation is refused",
    );
    // A checksum cannot catch a *valid* block that is simply not the one
    // asked for; `bytenr` is what does.
    c.check(
        reader.read_node(LOG_FS_LEAF_B, Some(GEN)).map(|n| n.header.bytenr) == Ok(LOG_FS_LEAF_B),
        "leaf B reads back at its own address",
    );
    c.check(
        reader.read_node(LOG_UNWRITTEN, None).is_err(),
        "an address holding no block at all fails the checksum",
    );
    c.check(
        reader.read_node(LOG_DATA.saturating_add(LEN_DATA), None).is_err(),
        "an address outside every chunk is refused",
    );

    // A full ordered walk, which is the only thing that exercises climbing out
    // of one leaf and descending into the next: btrfs leaves carry no sibling
    // pointer, so the path itself is the iterator's state.
    let mut path = reader.search(LOG_FS_NODE, Some(GEN), &Key::new(0, 0, 0))?;
    let mut seen = 0usize;
    let mut last = Key::new(0, 0, 0);
    while path.current().is_some() {
        let key = path.current_key()?;
        if seen > 0 && key.sort_tuple() <= last.sort_tuple() {
            c.check(false, "the walk visited keys out of order");
        }
        last = key;
        seen = seen.saturating_add(1);
        if !reader.next(&mut path)? {
            break;
        }
    }
    c.check(
        seen == FS_TREE_ITEMS,
        "a forward walk visits every item in both leaves",
    );
    c.check(
        last == Key::new(INO_PREALLOC, EXTENT_DATA_KEY, 0),
        "and ends on the last item of leaf B",
    );
    c.check(
        !reader.next(&mut path)?,
        "advancing past the end reports exhaustion rather than looping",
    );

    // ... and the same walk backwards, which is what a mid-extent read needs.
    let mut back = reader.search(LOG_FS_NODE, Some(GEN), &Key::new(u64::MAX, 255, u64::MAX))?;
    c.check(
        back.current().is_none(),
        "a search past every key lands off the end of the leaf",
    );
    c.check(
        reader.prev(&mut back)?,
        "stepping back from off-the-end selects the last item",
    );
    c.check(
        back.current_key() == Ok(Key::new(INO_PREALLOC, EXTENT_DATA_KEY, 0)),
        "which is the last item",
    );
    let mut steps = 1usize;
    while reader.prev(&mut back)? {
        steps = steps.saturating_add(1);
    }
    c.check(
        steps == FS_TREE_ITEMS,
        "a backward walk visits every item too",
    );
    c.check(
        back.current_key() == Ok(Key::new(INO_ROOT, INODE_ITEM_KEY, 0)),
        "and ends on the first item of leaf A",
    );

    c.check(
        reader
            .find(LOG_FS_NODE, Some(GEN), &Key::new(INO_HELLO, EXTENT_DATA_KEY, 0))?
            .is_some(),
        "an exact key is found",
    );
    c.check(
        reader
            .find(LOG_FS_NODE, Some(GEN), &Key::new(INO_HELLO, EXTENT_DATA_KEY, 1))?
            .is_none(),
        "a near-miss key is not",
    );
    c.check(
        reader
            .find(LOG_FS_NODE, Some(GEN), &Key::new(9999, INODE_ITEM_KEY, 0))?
            .is_none(),
        "a key past every item is not",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// The volume, through the VFS surface
// ---------------------------------------------------------------------------

fn test_volume(c: &mut Checks) -> KernelResult<()> {
    let mut fs = mount_image(build_image()?)?;

    c.check(fs.fs_type() == "btrfs", "fs type");
    c.check(
        fs.chunk_count() == 3,
        "the mount assembled the full chunk map from the tree, not just the bootstrap",
    );
    c.check(
        fs.device_name().is_none(),
        "a memory-backed mount names no device",
    );
    c.check(
        fs.superblock().generation == GEN,
        "the mount kept the superblock it validated",
    );
    c.check(!fs.debug_stats().is_empty(), "debug stats are produced");

    // --- the root directory, in creation order ---
    let entries = fs.readdir(Path::new("/"))?;
    c.check(
        entries.len() == ROOT_ENTRIES.len(),
        "the root directory lists every entry",
    );
    for (i, (_, name, _, ftype)) in ROOT_ENTRIES.iter().enumerate() {
        let Some(entry) = entries.get(i) else {
            c.check(false, "missing root entry");
            continue;
        };
        c.check_bytes(entry.name.as_bytes(), name, "root entry name in DIR_INDEX order");
        let want = match *ftype {
            FT_DIR => EntryType::Directory,
            FT_SYMLINK => EntryType::Symlink,
            _ => EntryType::File,
        };
        c.check(entry.entry_type == want, "root entry type");
    }
    c.check(
        entries.first().map(|e| e.size) == Some(u64_len(HELLO_TEXT)),
        "a listed file carries its size",
    );
    c.check(
        entries.get(1).map(|e| e.size) == Some(0),
        "a listed directory does not pay for a size lookup",
    );
    c.check(
        entries.get(3).map(|e| e.size) == Some(SPARSE_SIZE),
        "a sparse file lists its logical size, not its allocated size",
    );

    let sub = fs.readdir(Path::new("/sub"))?;
    c.check(sub.len() == 1, "the subdirectory has one entry");
    c.check(
        sub.first().map(|e| e.name.as_bytes()) == Some(b"data.bin".as_slice()),
        "and it is data.bin",
    );

    // --- file contents ---
    c.check_bytes(
        &fs.read_file(Path::new("/hello.txt"))?,
        HELLO_TEXT,
        "an inline extent reads back",
    );

    let want_data: Vec<u8> = (0..DATA_LEN).map(data_byte).collect();
    c.check_bytes(
        &fs.read_file(Path::new("/sub/data.bin"))?,
        &want_data,
        "a regular extent reads back through the chunk map",
    );
    c.check_bytes(
        &fs.read_at(Path::new("/sub/data.bin"), 4000, 200)?,
        want_data.get(4000..4200).unwrap_or_default(),
        "a read starting inside an extent skips the right number of bytes",
    );
    c.check(
        fs.read_at(Path::new("/sub/data.bin"), 8000, 1000)?.len() == 192,
        "a read is clamped to the end of the file",
    );
    c.check(
        fs.read_at(Path::new("/sub/data.bin"), to_u64(DATA_LEN), 16)?
            .is_empty(),
        "a read at EOF returns nothing",
    );

    // A hole is the absence of an item, so nothing writes into the buffer and
    // the zeroes it was created with are the answer.
    let sparse = fs.read_file(Path::new("/sparse.bin"))?;
    c.check(
        sparse.len() == to_usize(SPARSE_SIZE),
        "a sparse file reads its whole logical length",
    );
    c.check(
        sparse
            .get(..to_usize(SPARSE_TAIL_OFF))
            .is_some_and(|h| h.iter().all(|b| *b == 0)),
        "the hole reads as zeroes",
    );
    let want_tail: Vec<u8> = (0..SPARSE_TAIL_LEN).map(sparse_byte).collect();
    c.check_bytes(
        sparse.get(to_usize(SPARSE_TAIL_OFF)..).unwrap_or_default(),
        &want_tail,
        "the tail extent follows the hole",
    );
    // The case `prev()` exists for: 10000 falls inside an extent that starts
    // at 8192, so a forward search alone would have skipped past it.
    c.check_bytes(
        &fs.read_at(Path::new("/sparse.bin"), 10_000, 100)?,
        want_tail.get(1808..1908).unwrap_or_default(),
        "a read starting mid-extent backs up to the extent containing it",
    );

    let prealloc = fs.read_file(Path::new("/prealloc.bin"))?;
    c.check(
        prealloc.len() == PREALLOC_LEN,
        "a preallocated file has its full length",
    );
    c.check(
        prealloc.iter().all(|b| *b == 0),
        "preallocated space reads as zeroes, not as what the allocator left there",
    );

    c.check_bytes(
        fs.readlink(Path::new("/link"))?.as_bytes(),
        LINK_TARGET,
        "a symlink target is read like file contents",
    );

    // --- path resolution ---
    c.check_bytes(
        &fs.read_file(Path::new("/sub/../hello.txt"))?,
        HELLO_TEXT,
        ".. unwinds the path the caller used",
    );
    c.check_bytes(
        &fs.read_file(Path::new("/./sub/./data.bin"))?,
        &want_data,
        ". is a no-op component",
    );
    c.check_bytes(
        &fs.read_file(Path::new("/../../hello.txt"))?,
        HELLO_TEXT,
        ".. at the root stays at the root rather than escaping the volume",
    );

    // --- metadata ---
    let meta = fs.metadata(Path::new("/hello.txt"))?;
    c.check(meta.ino == INO_HELLO, "metadata reports the inode number");
    c.check(meta.size == u64_len(HELLO_TEXT), "metadata size");
    c.check(meta.nlinks == 1, "metadata link count");
    c.check(
        meta.modified_ns == MTIME_SEC.saturating_mul(1_000_000_000).saturating_add(u64::from(MTIME_NSEC)),
        "metadata mtime",
    );
    c.check(
        meta.created_ns == OTIME_SEC.saturating_mul(1_000_000_000).saturating_add(u64::from(OTIME_NSEC)),
        "btrfs records a creation time and it is reported",
    );
    // A read-only mount that advertised write bits would be lying to
    // userspace, which would then act on it.
    c.check(meta.permissions == 0o444, "0644 is reported without write bits");
    c.check(
        fs.metadata(Path::new("/sub"))?.permissions == 0o555,
        "0755 keeps execute but loses write",
    );
    c.check(
        fs.metadata(Path::new("/sub/data.bin"))?.blocks == 16,
        "blocks are nbytes / 512",
    );

    let st = fs.stat(Path::new("/hello.txt"))?;
    c.check(st.entry_type == EntryType::File, "stat type");
    c.check(st.size == u64_len(HELLO_TEXT), "stat size");
    c.check_bytes(st.name.as_bytes(), b"hello.txt", "stat name");
    c.check(
        fs.stat(Path::new("/"))?.entry_type == EntryType::Directory,
        "the root is a directory",
    );
    c.check(
        fs.lstat(Path::new("/link"))?.entry_type == EntryType::Symlink,
        "lstat reports the link itself",
    );
    c.check(
        fs.lmetadata(Path::new("/link"))?.entry_type == EntryType::Symlink,
        "and so does lmetadata",
    );

    let info = fs.statvfs()?;
    c.check(info.read_only, "the mount is read-only");
    c.check(info.fs_type == "btrfs", "statvfs fs type");
    c.check(info.volume_label.as_bytes() == LABEL, "statvfs label");
    c.check(info.block_size == u64::from(SECTORSIZE), "statvfs block size");
    c.check(
        info.total_blocks == TOTAL_BYTES.checked_div(u64::from(SECTORSIZE)).unwrap_or(0),
        "statvfs total blocks",
    );
    c.check(info.free_blocks == 0, "a read-only mount reports no free space");
    c.check(info.max_name_len == 255, "statvfs max name length");

    // --- errors ---
    c.check(
        fs.read_file(Path::new("/nope")) == Err(KernelError::NotFound),
        "a missing name is NotFound",
    );
    c.check(
        fs.read_file(Path::new("/sub/nope")) == Err(KernelError::NotFound),
        "a missing name in a subdirectory is NotFound",
    );
    c.check_err(
        fs.readdir(Path::new("/hello.txt")),
        KernelError::NotADirectory,
        "listing a file is NotADirectory",
    );
    c.check(
        fs.read_file(Path::new("/hello.txt/x")) == Err(KernelError::NotADirectory),
        "descending through a file is NotADirectory",
    );
    c.check(
        fs.read_file(Path::new("/sub")) == Err(KernelError::IsADirectory),
        "reading a directory is IsADirectory",
    );
    c.check(
        fs.readlink(Path::new("/hello.txt")) == Err(KernelError::InvalidArgument),
        "readlink on a regular file is refused",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Corruption
// ---------------------------------------------------------------------------

fn test_corruption(c: &mut Checks) -> KernelResult<()> {
    // A block whose bytes were altered without re-sealing fails its checksum.
    // The FS tree is not touched during mount, so this surfaces on first use —
    // which is itself worth asserting: a lazily-read tree must not be assumed
    // validated by the mount having succeeded.
    let mut flipped = build_image()?;
    flip(&mut flipped, phys_of(LOG_FS_LEAF_A)?.saturating_add(200));
    let mut fs = mount_image(flipped)?;
    c.check_err(
        fs.readdir(Path::new("/")),
        KernelError::IoError,
        "a corrupt FS leaf fails its checksum on first use",
    );

    // The copy-on-write hazard: this block is intact and checksums perfectly,
    // it is simply the wrong version. Only the generation catches it.
    let mut stale = build_image()?;
    patch_block(&mut stale, LOG_FS_LEAF_B, 80, &GEN.saturating_sub(1).to_le_bytes())?;
    let mut fs = mount_image(stale)?;
    c.check(
        fs.readdir(Path::new("/")).is_ok(),
        "leaf A is still readable when leaf B is stale",
    );
    c.check(
        fs.read_file(Path::new("/sub/data.bin")) == Err(KernelError::IoError),
        "a stale-but-valid block is refused on its generation",
    );

    // A block that is valid and current but is not the one we followed a
    // pointer to. A checksum cannot see this; `bytenr` can.
    let mut displaced = build_image()?;
    patch_block(&mut displaced, LOG_FS_LEAF_A, 48, &LOG_FS_LEAF_B.to_le_bytes())?;
    let mut fs = mount_image(displaced)?;
    c.check_err(
        fs.readdir(Path::new("/")),
        KernelError::IoError,
        "a block that disagrees about its own address is refused",
    );

    // A corrupt item count would otherwise make every slot read an
    // out-of-bounds index.
    let mut overfull = build_image()?;
    patch_block(&mut overfull, LOG_FS_LEAF_A, 96, &10_000u32.to_le_bytes())?;
    let mut fs = mount_image(overfull)?;
    c.check_err(
        fs.readdir(Path::new("/")),
        KernelError::InvalidArgument,
        "an item count that cannot fit the block is refused",
    );

    // A level beyond BTRFS_MAX_LEVEL would drive an unbounded descent.
    let mut deep = build_image()?;
    patch_block(&mut deep, LOG_FS_NODE, 100, &[200u8])?;
    let mut fs = mount_image(deep)?;
    c.check_err(
        fs.readdir(Path::new("/")),
        KernelError::InvalidArgument,
        "an absurd tree level is refused",
    );

    // A device too short to hold the metadata it points at.
    let mut truncated = build_image()?;
    truncated.truncate(0x10_0000);
    c.check(
        mount_image(truncated).is_err(),
        "a truncated image cannot be mounted",
    );

    // A multi-device volume is refused at mount rather than during a descent,
    // because one SectorSource is one device no matter how correct the parser.
    let mut multi = build_image()?;
    patch_superblock(&mut multi, 0x88, &2u64.to_le_bytes())?;
    c.check_err(
        mount_image(multi),
        KernelError::NotSupported,
        "a multi-device volume is refused at mount, not during a descent",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run every Btrfs self-test.
///
/// Every group runs even if an earlier one failed, so one boot reports every
/// problem rather than the first one. A group that returns `Err` hit a hard
/// error rather than a failed assertion — a parse that could not proceed — and
/// its own remaining steps are skipped, but the groups after it are not.
///
/// # Errors
///
/// [`KernelError::InternalError`] if any check failed or any group errored.
/// Every failure has already been printed by the time this returns.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[btrfs] Running self-test...");

    let mut c = Checks::new();
    let mut aborted = 0u32;

    // Named so a hard error says *which* group stopped; without the name the
    // serial log shows an error with no indication of where it came from.
    let groups: [(&str, TestGroup); 7] = [
        ("primitives", test_primitives),
        ("items", test_items),
        ("chunk map", test_chunk_map),
        ("superblock", test_superblock),
        ("btree", test_btree),
        ("volume", test_volume),
        ("corruption", test_corruption),
    ];

    for (name, run) in groups {
        if let Err(e) = run(&mut c) {
            aborted = aborted.saturating_add(1);
            serial_println!("[btrfs] SELF-TEST ERROR in {}: {:?}", name, e);
        }
    }

    if c.failed == 0 && aborted == 0 {
        serial_println!(
            "[btrfs] Self-test passed ({} checks over a synthetic volume).",
            c.passed
        );
        return Ok(());
    }

    serial_println!(
        "[btrfs] Self-test FAILED: {} passed, {} failed, {} group(s) errored.",
        c.passed,
        c.failed,
        aborted
    );
    Err(KernelError::InternalError)
}
