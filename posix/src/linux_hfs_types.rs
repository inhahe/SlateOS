//! `<linux/hfs_fs.h>` — HFS/HFS+ filesystem constants.
//!
//! HFS (Hierarchical File System) and HFS+ are Apple's
//! filesystems. These constants define magic numbers,
//! node types, and catalog record types.

// ---------------------------------------------------------------------------
// Signature / magic
// ---------------------------------------------------------------------------

/// HFS volume signature.
pub const HFS_SUPER_MAGIC: u32 = 0x4244;
/// HFS+ volume signature.
pub const HFSPLUS_SUPER_MAGIC: u32 = 0x482B;
/// HFSX volume signature (case-sensitive HFS+).
pub const HFSX_SUPER_MAGIC: u32 = 0x4858;
/// HFS wrapper signature.
pub const HFS_WRAPPER_MAGIC: u32 = 0x4C4B;

// ---------------------------------------------------------------------------
// B-tree node types
// ---------------------------------------------------------------------------

/// Index node.
pub const HFS_NODE_INDEX: u8 = 0x00;
/// Header node.
pub const HFS_NODE_HEADER: u8 = 0x01;
/// Map node.
pub const HFS_NODE_MAP: u8 = 0x02;
/// Leaf node.
pub const HFS_NODE_LEAF: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Catalog record types (HFS)
// ---------------------------------------------------------------------------

/// Directory record.
pub const HFS_CDR_DIR: u8 = 1;
/// File record.
pub const HFS_CDR_FIL: u8 = 2;
/// Directory thread.
pub const HFS_CDR_THD: u8 = 3;
/// File thread.
pub const HFS_CDR_FTH: u8 = 4;

// ---------------------------------------------------------------------------
// Catalog record types (HFS+)
// ---------------------------------------------------------------------------

/// HFS+ folder record.
pub const HFSPLUS_FOLDER: u16 = 1;
/// HFS+ file record.
pub const HFSPLUS_FILE: u16 = 2;
/// HFS+ folder thread.
pub const HFSPLUS_FOLDER_THREAD: u16 = 3;
/// HFS+ file thread.
pub const HFSPLUS_FILE_THREAD: u16 = 4;

// ---------------------------------------------------------------------------
// File attribute flags
// ---------------------------------------------------------------------------

/// File is locked.
pub const HFS_FLG_LOCKED: u16 = 0x0001;
/// File has thread.
pub const HFS_FLG_THREAD: u16 = 0x0002;
/// Has been copied (desktop).
pub const HFS_FLG_INITED: u16 = 0x0100;

// ---------------------------------------------------------------------------
// HFS+ extent descriptor limits
// ---------------------------------------------------------------------------

/// Extents per fork record.
pub const HFSPLUS_EXT_COUNT: u32 = 8;
/// Maximum file name length (characters).
pub const HFS_MAX_NAMELEN: u32 = 31;
/// HFS+ max name length (characters).
pub const HFSPLUS_MAX_NAMELEN: u32 = 255;

// ---------------------------------------------------------------------------
// Special CNID values (catalog node ID)
// ---------------------------------------------------------------------------

/// Root parent CNID.
pub const HFS_ROOT_PARENT_CNID: u32 = 1;
/// Root directory CNID.
pub const HFS_ROOT_CNID: u32 = 2;
/// Extents overflow file CNID.
pub const HFS_EXT_CNID: u32 = 3;
/// Catalog file CNID.
pub const HFS_CAT_CNID: u32 = 4;
/// Bad block file CNID.
pub const HFS_BAD_CNID: u32 = 5;
/// Allocation file CNID (HFS+).
pub const HFSPLUS_ALLOC_CNID: u32 = 6;
/// Startup file CNID (HFS+).
pub const HFSPLUS_START_CNID: u32 = 7;
/// Attributes file CNID (HFS+).
pub const HFSPLUS_ATTR_CNID: u32 = 8;
/// First user CNID.
pub const HFS_FIRST_USER_CNID: u32 = 16;

// ---------------------------------------------------------------------------
// Volume attributes (HFS+)
// ---------------------------------------------------------------------------

/// Volume is unmounted cleanly.
pub const HFSPLUS_VOL_UNMNT: u32 = 1 << 8;
/// Volume has spare blocks.
pub const HFSPLUS_VOL_SPARE_BLK: u32 = 1 << 9;
/// Volume has no cache.
pub const HFSPLUS_VOL_NOCACHE: u32 = 1 << 10;
/// Volume is hardware locked.
pub const HFSPLUS_VOL_HWLOCK: u32 = 1 << 7;
/// Software lock.
pub const HFSPLUS_VOL_SWLOCK: u32 = 1 << 14;
/// Journaled volume.
pub const HFSPLUS_VOL_JOURNALED: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_values_distinct() {
        let magics = [
            HFS_SUPER_MAGIC,
            HFSPLUS_SUPER_MAGIC,
            HFSX_SUPER_MAGIC,
            HFS_WRAPPER_MAGIC,
        ];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_node_types_distinct() {
        let types = [HFS_NODE_INDEX, HFS_NODE_HEADER, HFS_NODE_MAP, HFS_NODE_LEAF];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_hfs_catalog_types_sequential() {
        assert_eq!(HFS_CDR_DIR, 1);
        assert_eq!(HFS_CDR_FIL, 2);
        assert_eq!(HFS_CDR_THD, 3);
        assert_eq!(HFS_CDR_FTH, 4);
    }

    #[test]
    fn test_hfsplus_catalog_types_sequential() {
        assert_eq!(HFSPLUS_FOLDER, 1);
        assert_eq!(HFSPLUS_FILE, 2);
        assert_eq!(HFSPLUS_FOLDER_THREAD, 3);
        assert_eq!(HFSPLUS_FILE_THREAD, 4);
    }

    #[test]
    fn test_cnid_values_distinct() {
        let cnids = [
            HFS_ROOT_PARENT_CNID,
            HFS_ROOT_CNID,
            HFS_EXT_CNID,
            HFS_CAT_CNID,
            HFS_BAD_CNID,
            HFSPLUS_ALLOC_CNID,
            HFSPLUS_START_CNID,
            HFSPLUS_ATTR_CNID,
            HFS_FIRST_USER_CNID,
        ];
        for i in 0..cnids.len() {
            for j in (i + 1)..cnids.len() {
                assert_ne!(cnids[i], cnids[j]);
            }
        }
    }

    #[test]
    fn test_name_lengths() {
        assert_eq!(HFS_MAX_NAMELEN, 31);
        assert_eq!(HFSPLUS_MAX_NAMELEN, 255);
        assert!(HFS_MAX_NAMELEN < HFSPLUS_MAX_NAMELEN);
    }

    #[test]
    fn test_vol_attrs_distinct() {
        let attrs = [
            HFSPLUS_VOL_UNMNT,
            HFSPLUS_VOL_SPARE_BLK,
            HFSPLUS_VOL_NOCACHE,
            HFSPLUS_VOL_HWLOCK,
            HFSPLUS_VOL_SWLOCK,
            HFSPLUS_VOL_JOURNALED,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_vol_attrs_power_of_two() {
        let attrs = [
            HFSPLUS_VOL_UNMNT,
            HFSPLUS_VOL_SPARE_BLK,
            HFSPLUS_VOL_NOCACHE,
            HFSPLUS_VOL_HWLOCK,
            HFSPLUS_VOL_SWLOCK,
            HFSPLUS_VOL_JOURNALED,
        ];
        for a in &attrs {
            assert!(a.is_power_of_two(), "0x{:08x} not power of two", a);
        }
    }

    #[test]
    fn test_ext_count() {
        assert_eq!(HFSPLUS_EXT_COUNT, 8);
    }
}
