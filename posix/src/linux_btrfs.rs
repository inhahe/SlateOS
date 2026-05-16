//! `<linux/btrfs.h>` — Btrfs filesystem ioctl constants.
//!
//! Btrfs is a copy-on-write filesystem with snapshots, subvolumes,
//! checksums, compression, and RAID. This module defines ioctl
//! commands and flags for Btrfs management.

// ---------------------------------------------------------------------------
// Btrfs ioctl magic
// ---------------------------------------------------------------------------

/// Btrfs ioctl magic byte.
pub const BTRFS_IOCTL_MAGIC: u8 = 0x94;

// ---------------------------------------------------------------------------
// Btrfs ioctl command numbers
// ---------------------------------------------------------------------------

/// Snap create.
pub const BTRFS_IOC_SNAP_CREATE: u32 = 1;
/// Defrag.
pub const BTRFS_IOC_DEFRAG: u32 = 2;
/// Resize.
pub const BTRFS_IOC_RESIZE: u32 = 3;
/// Scan device.
pub const BTRFS_IOC_SCAN_DEV: u32 = 4;
/// Sync filesystem.
pub const BTRFS_IOC_SYNC: u32 = 8;
/// Clone range.
pub const BTRFS_IOC_CLONE: u32 = 9;
/// Add device.
pub const BTRFS_IOC_ADD_DEV: u32 = 10;
/// Remove device.
pub const BTRFS_IOC_RM_DEV: u32 = 11;
/// Balance start.
pub const BTRFS_IOC_BALANCE: u32 = 12;
/// Subvolume create.
pub const BTRFS_IOC_SUBVOL_CREATE: u32 = 14;
/// Snap destroy.
pub const BTRFS_IOC_SNAP_DESTROY: u32 = 15;
/// Defrag range.
pub const BTRFS_IOC_DEFRAG_RANGE: u32 = 16;
/// Tree search.
pub const BTRFS_IOC_TREE_SEARCH: u32 = 17;
/// INO lookup.
pub const BTRFS_IOC_INO_LOOKUP: u32 = 18;
/// Set default subvolume.
pub const BTRFS_IOC_DEFAULT_SUBVOL: u32 = 19;
/// Get space info.
pub const BTRFS_IOC_SPACE_INFO: u32 = 20;
/// Start scrub.
pub const BTRFS_IOC_SCRUB: u32 = 27;
/// Scrub cancel.
pub const BTRFS_IOC_SCRUB_CANCEL: u32 = 28;
/// Scrub progress.
pub const BTRFS_IOC_SCRUB_PROGRESS: u32 = 29;
/// Device info.
pub const BTRFS_IOC_DEV_INFO: u32 = 30;
/// FS info.
pub const BTRFS_IOC_FS_INFO: u32 = 31;
/// Get features.
pub const BTRFS_IOC_GET_FEATURES: u32 = 57;
/// Set features.
pub const BTRFS_IOC_SET_FEATURES: u32 = 58;
/// Get supported features.
pub const BTRFS_IOC_GET_SUPPORTED_FEATURES: u32 = 59;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// No compression.
pub const BTRFS_COMPRESS_NONE: u32 = 0;
/// Zlib compression.
pub const BTRFS_COMPRESS_ZLIB: u32 = 1;
/// LZO compression.
pub const BTRFS_COMPRESS_LZO: u32 = 2;
/// Zstd compression.
pub const BTRFS_COMPRESS_ZSTD: u32 = 3;

// ---------------------------------------------------------------------------
// Object types (tree key types)
// ---------------------------------------------------------------------------

/// Inode item.
pub const BTRFS_INODE_ITEM_KEY: u8 = 1;
/// Inode ref.
pub const BTRFS_INODE_REF_KEY: u8 = 12;
/// Dir item.
pub const BTRFS_DIR_ITEM_KEY: u8 = 84;
/// Dir index.
pub const BTRFS_DIR_INDEX_KEY: u8 = 96;
/// Extent data.
pub const BTRFS_EXTENT_DATA_KEY: u8 = 108;
/// Root item.
pub const BTRFS_ROOT_ITEM_KEY: u8 = 132;
/// Root ref.
pub const BTRFS_ROOT_REF_KEY: u8 = 156;
/// Extent item.
pub const BTRFS_EXTENT_ITEM_KEY: u8 = 168;
/// Chunk item.
pub const BTRFS_CHUNK_ITEM_KEY: u8 = 228;
/// Device item.
pub const BTRFS_DEV_ITEM_KEY: u8 = 216;

// ---------------------------------------------------------------------------
// Well-known tree IDs
// ---------------------------------------------------------------------------

/// Root tree.
pub const BTRFS_ROOT_TREE_OBJECTID: u64 = 1;
/// Extent tree.
pub const BTRFS_EXTENT_TREE_OBJECTID: u64 = 2;
/// Chunk tree.
pub const BTRFS_CHUNK_TREE_OBJECTID: u64 = 3;
/// Device tree.
pub const BTRFS_DEV_TREE_OBJECTID: u64 = 4;
/// FS tree.
pub const BTRFS_FS_TREE_OBJECTID: u64 = 5;
/// Checksum tree.
pub const BTRFS_CSUM_TREE_OBJECTID: u64 = 7;

// ---------------------------------------------------------------------------
// RAID profiles
// ---------------------------------------------------------------------------

/// Single (no redundancy).
pub const BTRFS_BLOCK_GROUP_SINGLE: u64 = 0;
/// RAID0.
pub const BTRFS_BLOCK_GROUP_RAID0: u64 = 1 << 3;
/// RAID1.
pub const BTRFS_BLOCK_GROUP_RAID1: u64 = 1 << 4;
/// DUP (two copies on same device).
pub const BTRFS_BLOCK_GROUP_DUP: u64 = 1 << 5;
/// RAID10.
pub const BTRFS_BLOCK_GROUP_RAID10: u64 = 1 << 6;
/// RAID5.
pub const BTRFS_BLOCK_GROUP_RAID5: u64 = 1 << 7;
/// RAID6.
pub const BTRFS_BLOCK_GROUP_RAID6: u64 = 1 << 8;
/// RAID1C3.
pub const BTRFS_BLOCK_GROUP_RAID1C3: u64 = 1 << 9;
/// RAID1C4.
pub const BTRFS_BLOCK_GROUP_RAID1C4: u64 = 1 << 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioc_cmds_distinct() {
        let cmds = [
            BTRFS_IOC_SNAP_CREATE, BTRFS_IOC_DEFRAG, BTRFS_IOC_RESIZE,
            BTRFS_IOC_SCAN_DEV, BTRFS_IOC_SYNC, BTRFS_IOC_CLONE,
            BTRFS_IOC_ADD_DEV, BTRFS_IOC_RM_DEV, BTRFS_IOC_BALANCE,
            BTRFS_IOC_SUBVOL_CREATE, BTRFS_IOC_SNAP_DESTROY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_compress_types_distinct() {
        let types = [
            BTRFS_COMPRESS_NONE, BTRFS_COMPRESS_ZLIB,
            BTRFS_COMPRESS_LZO, BTRFS_COMPRESS_ZSTD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_key_types_distinct() {
        let keys = [
            BTRFS_INODE_ITEM_KEY, BTRFS_INODE_REF_KEY,
            BTRFS_DIR_ITEM_KEY, BTRFS_DIR_INDEX_KEY,
            BTRFS_EXTENT_DATA_KEY, BTRFS_ROOT_ITEM_KEY,
            BTRFS_ROOT_REF_KEY, BTRFS_EXTENT_ITEM_KEY,
            BTRFS_CHUNK_ITEM_KEY, BTRFS_DEV_ITEM_KEY,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_tree_ids_distinct() {
        let ids = [
            BTRFS_ROOT_TREE_OBJECTID, BTRFS_EXTENT_TREE_OBJECTID,
            BTRFS_CHUNK_TREE_OBJECTID, BTRFS_DEV_TREE_OBJECTID,
            BTRFS_FS_TREE_OBJECTID, BTRFS_CSUM_TREE_OBJECTID,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_raid_profiles_no_overlap() {
        let raids = [
            BTRFS_BLOCK_GROUP_RAID0, BTRFS_BLOCK_GROUP_RAID1,
            BTRFS_BLOCK_GROUP_DUP, BTRFS_BLOCK_GROUP_RAID10,
            BTRFS_BLOCK_GROUP_RAID5, BTRFS_BLOCK_GROUP_RAID6,
            BTRFS_BLOCK_GROUP_RAID1C3, BTRFS_BLOCK_GROUP_RAID1C4,
        ];
        for i in 0..raids.len() {
            for j in (i + 1)..raids.len() {
                assert_eq!(raids[i] & raids[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctl_magic() {
        assert_eq!(BTRFS_IOCTL_MAGIC, 0x94);
    }
}
