//! `<linux/amigaffs.h>` — Amiga FFS/OFS filesystem constants.
//!
//! The Amiga Fast File System (FFS) and Old File System (OFS)
//! are native Amiga filesystems. These constants define
//! block types, header types, and bitmap parameters.

// ---------------------------------------------------------------------------
// Block type identifiers
// ---------------------------------------------------------------------------

/// Boot block type.
pub const AFFS_TYPE_HEADER: u32 = 2;
/// Data block type.
pub const AFFS_TYPE_DATA: u32 = 8;
/// List block type.
pub const AFFS_TYPE_LIST: u32 = 16;

// ---------------------------------------------------------------------------
// Header secondary types
// ---------------------------------------------------------------------------

/// Root block secondary type.
pub const AFFS_ST_ROOT: u32 = 1;
/// User directory.
pub const AFFS_ST_USERDIR: u32 = 2;
/// File header.
pub const AFFS_ST_FILE: u32 = 0xFFFFFFFD;
/// Soft link.
pub const AFFS_ST_SOFTLINK: u32 = 3;
/// Hard link to directory.
pub const AFFS_ST_LINKDIR: u32 = 4;
/// Hard link to file.
pub const AFFS_ST_LINKFILE: u32 = 0xFFFFFFFC;

// ---------------------------------------------------------------------------
// Filesystem subtypes
// ---------------------------------------------------------------------------

/// Old filesystem (OFS).
pub const AFFS_DOS_OFS: u32 = 0x444F5300;
/// Fast filesystem (FFS).
pub const AFFS_DOS_FFS: u32 = 0x444F5301;
/// OFS + international.
pub const AFFS_DOS_OFS_INTL: u32 = 0x444F5302;
/// FFS + international.
pub const AFFS_DOS_FFS_INTL: u32 = 0x444F5303;
/// OFS + dircache.
pub const AFFS_DOS_OFS_DC: u32 = 0x444F5304;
/// FFS + dircache.
pub const AFFS_DOS_FFS_DC: u32 = 0x444F5305;
/// Mask to identify DOS signature.
pub const AFFS_DOS_MASK: u32 = 0xFFFFFF00;

// ---------------------------------------------------------------------------
// Magic number
// ---------------------------------------------------------------------------

/// AFFS super magic for Linux VFS.
pub const AFFS_SUPER_MAGIC: u32 = 0xADFF;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum file name length.
pub const AFFS_MAX_NAME_LEN: u32 = 30;
/// Hash table size (root/dir blocks).
pub const AFFS_HASHTABLE_SIZE: u32 = 72;
/// Maximum links per file.
pub const AFFS_LINK_MAX: u32 = 65535;

// ---------------------------------------------------------------------------
// Bitmap / allocation
// ---------------------------------------------------------------------------

/// Bitmap valid marker.
pub const AFFS_BITMAP_VALID: u32 = 0xFFFFFFFF;
/// Bitmap invalid marker.
pub const AFFS_BITMAP_INVALID: u32 = 0x00000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_types_distinct() {
        let types = [AFFS_TYPE_HEADER, AFFS_TYPE_DATA, AFFS_TYPE_LIST];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_header_types_distinct() {
        let types = [
            AFFS_ST_ROOT, AFFS_ST_USERDIR, AFFS_ST_FILE,
            AFFS_ST_SOFTLINK, AFFS_ST_LINKDIR, AFFS_ST_LINKFILE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dos_types_distinct() {
        let types = [
            AFFS_DOS_OFS, AFFS_DOS_FFS, AFFS_DOS_OFS_INTL,
            AFFS_DOS_FFS_INTL, AFFS_DOS_OFS_DC, AFFS_DOS_FFS_DC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dos_mask() {
        assert_eq!(AFFS_DOS_OFS & AFFS_DOS_MASK, AFFS_DOS_FFS & AFFS_DOS_MASK);
    }

    #[test]
    fn test_super_magic() {
        assert_eq!(AFFS_SUPER_MAGIC, 0xADFF);
    }

    #[test]
    fn test_name_length() {
        assert_eq!(AFFS_MAX_NAME_LEN, 30);
    }

    #[test]
    fn test_bitmap_markers() {
        assert_ne!(AFFS_BITMAP_VALID, AFFS_BITMAP_INVALID);
    }

    #[test]
    fn test_hashtable_size() {
        assert_eq!(AFFS_HASHTABLE_SIZE, 72);
    }
}
