//! `<linux/btrfs.h>` — Btrfs filesystem constants.
//!
//! Btrfs is a copy-on-write B-tree filesystem for Linux supporting
//! snapshots, subvolumes, checksums, RAID, compression, deduplication,
//! and online defragmentation. It aims to be the next-generation
//! Linux filesystem.

// ---------------------------------------------------------------------------
// Btrfs magic
// ---------------------------------------------------------------------------

/// Btrfs superblock magic ("_BHRfS_M").
pub const BTRFS_MAGIC: u64 = 0x4D5F_5346_5248_425F;

// ---------------------------------------------------------------------------
// Object types (key type field)
// ---------------------------------------------------------------------------

/// Inode item.
pub const BTRFS_INODE_ITEM_KEY: u8 = 1;
/// Inode reference.
pub const BTRFS_INODE_REF_KEY: u8 = 12;
/// Extended inode reference.
pub const BTRFS_INODE_EXTREF_KEY: u8 = 13;
/// Directory item.
pub const BTRFS_DIR_ITEM_KEY: u8 = 84;
/// Directory index.
pub const BTRFS_DIR_INDEX_KEY: u8 = 96;
/// Extent data.
pub const BTRFS_EXTENT_DATA_KEY: u8 = 108;
/// Root item.
pub const BTRFS_ROOT_ITEM_KEY: u8 = 132;
/// Extent item.
pub const BTRFS_EXTENT_ITEM_KEY: u8 = 168;
/// Chunk item.
pub const BTRFS_CHUNK_ITEM_KEY: u8 = 228;
/// Device item.
pub const BTRFS_DEV_ITEM_KEY: u8 = 216;

// ---------------------------------------------------------------------------
// Predefined tree root object IDs
// ---------------------------------------------------------------------------

/// Root tree.
pub const BTRFS_ROOT_TREE_OBJECTID: u64 = 1;
/// Extent tree.
pub const BTRFS_EXTENT_TREE_OBJECTID: u64 = 2;
/// Chunk tree.
pub const BTRFS_CHUNK_TREE_OBJECTID: u64 = 3;
/// Device tree.
pub const BTRFS_DEV_TREE_OBJECTID: u64 = 4;
/// Filesystem tree (default subvolume).
pub const BTRFS_FS_TREE_OBJECTID: u64 = 5;
/// Checksum tree.
pub const BTRFS_CSUM_TREE_OBJECTID: u64 = 7;
/// Free space tree.
pub const BTRFS_FREE_SPACE_TREE_OBJECTID: u64 = 10;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// No compression.
pub const BTRFS_COMPRESS_NONE: u8 = 0;
/// Zlib compression.
pub const BTRFS_COMPRESS_ZLIB: u8 = 1;
/// LZO compression.
pub const BTRFS_COMPRESS_LZO: u8 = 2;
/// Zstd compression.
pub const BTRFS_COMPRESS_ZSTD: u8 = 3;

// ---------------------------------------------------------------------------
// RAID profiles (block group flags)
// ---------------------------------------------------------------------------

/// Single device (no redundancy).
pub const BTRFS_BLOCK_GROUP_SINGLE: u64 = 0;
/// RAID0 (stripe, no redundancy).
pub const BTRFS_BLOCK_GROUP_RAID0: u64 = 1 << 3;
/// RAID1 (mirror).
pub const BTRFS_BLOCK_GROUP_RAID1: u64 = 1 << 4;
/// DUP (duplicate on same device).
pub const BTRFS_BLOCK_GROUP_DUP: u64 = 1 << 5;
/// RAID10 (stripe + mirror).
pub const BTRFS_BLOCK_GROUP_RAID10: u64 = 1 << 6;
/// RAID5.
pub const BTRFS_BLOCK_GROUP_RAID5: u64 = 1 << 7;
/// RAID6.
pub const BTRFS_BLOCK_GROUP_RAID6: u64 = 1 << 8;
/// RAID1C3 (3 copies).
pub const BTRFS_BLOCK_GROUP_RAID1C3: u64 = 1 << 9;
/// RAID1C4 (4 copies).
pub const BTRFS_BLOCK_GROUP_RAID1C4: u64 = 1 << 10;

// ---------------------------------------------------------------------------
// Block group type flags
// ---------------------------------------------------------------------------

/// Data block group.
pub const BTRFS_BLOCK_GROUP_DATA: u64 = 1 << 0;
/// System block group.
pub const BTRFS_BLOCK_GROUP_SYSTEM: u64 = 1 << 1;
/// Metadata block group.
pub const BTRFS_BLOCK_GROUP_METADATA: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Subvolume flags
// ---------------------------------------------------------------------------

/// Subvolume is read-only.
pub const BTRFS_SUBVOL_RDONLY: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Checksum types
// ---------------------------------------------------------------------------

/// CRC32C checksum.
pub const BTRFS_CSUM_TYPE_CRC32: u16 = 0;
/// xxHash checksum.
pub const BTRFS_CSUM_TYPE_XXHASH: u16 = 1;
/// SHA-256 checksum.
pub const BTRFS_CSUM_TYPE_SHA256: u16 = 2;
/// BLAKE2b checksum.
pub const BTRFS_CSUM_TYPE_BLAKE2: u16 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(BTRFS_MAGIC, 0x4D5F_5346_5248_425F);
    }

    #[test]
    fn test_key_types_distinct() {
        let types = [
            BTRFS_INODE_ITEM_KEY,
            BTRFS_INODE_REF_KEY,
            BTRFS_INODE_EXTREF_KEY,
            BTRFS_DIR_ITEM_KEY,
            BTRFS_DIR_INDEX_KEY,
            BTRFS_EXTENT_DATA_KEY,
            BTRFS_ROOT_ITEM_KEY,
            BTRFS_EXTENT_ITEM_KEY,
            BTRFS_CHUNK_ITEM_KEY,
            BTRFS_DEV_ITEM_KEY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_tree_objectids_distinct() {
        let ids = [
            BTRFS_ROOT_TREE_OBJECTID,
            BTRFS_EXTENT_TREE_OBJECTID,
            BTRFS_CHUNK_TREE_OBJECTID,
            BTRFS_DEV_TREE_OBJECTID,
            BTRFS_FS_TREE_OBJECTID,
            BTRFS_CSUM_TREE_OBJECTID,
            BTRFS_FREE_SPACE_TREE_OBJECTID,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_compression_types_distinct() {
        let types = [
            BTRFS_COMPRESS_NONE,
            BTRFS_COMPRESS_ZLIB,
            BTRFS_COMPRESS_LZO,
            BTRFS_COMPRESS_ZSTD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_raid_profiles_no_overlap() {
        let profiles = [
            BTRFS_BLOCK_GROUP_RAID0,
            BTRFS_BLOCK_GROUP_RAID1,
            BTRFS_BLOCK_GROUP_DUP,
            BTRFS_BLOCK_GROUP_RAID10,
            BTRFS_BLOCK_GROUP_RAID5,
            BTRFS_BLOCK_GROUP_RAID6,
            BTRFS_BLOCK_GROUP_RAID1C3,
            BTRFS_BLOCK_GROUP_RAID1C4,
        ];
        for i in 0..profiles.len() {
            assert!(profiles[i].is_power_of_two());
            for j in (i + 1)..profiles.len() {
                assert_eq!(profiles[i] & profiles[j], 0);
            }
        }
    }

    #[test]
    fn test_block_group_types_no_overlap() {
        let types = [
            BTRFS_BLOCK_GROUP_DATA,
            BTRFS_BLOCK_GROUP_SYSTEM,
            BTRFS_BLOCK_GROUP_METADATA,
        ];
        for i in 0..types.len() {
            assert!(types[i].is_power_of_two());
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_checksum_types_distinct() {
        let types = [
            BTRFS_CSUM_TYPE_CRC32,
            BTRFS_CSUM_TYPE_XXHASH,
            BTRFS_CSUM_TYPE_SHA256,
            BTRFS_CSUM_TYPE_BLAKE2,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
