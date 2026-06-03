//! `<linux/befs_fs.h>` — BeFS (Be File System) constants.
//!
//! BeFS is the native filesystem of BeOS/Haiku.
//! These constants define magic numbers, inode flags,
//! B+tree node types, and superblock parameters.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// BeFS magic 1 (big-endian "1SFB").
pub const BEFS_SUPER_MAGIC1: u32 = 0x42465331;
/// BeFS magic 2 (alternate "BEFS" on little-endian).
pub const BEFS_SUPER_MAGIC2: u32 = 0x31534642;
/// BeFS magic 3 (yet another variant).
pub const BEFS_SUPER_MAGIC3: u32 = 0x42454653;
/// Linux VFS super magic.
pub const BEFS_SUPER_MAGIC: u32 = 0x42465331;

// ---------------------------------------------------------------------------
// Block run parameters
// ---------------------------------------------------------------------------

/// Maximum run length (block run extent).
pub const BEFS_MAX_RUN_LENGTH: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Inode in use.
pub const BEFS_INODE_IN_USE: u32 = 0x00000001;
/// Has attributes.
pub const BEFS_INODE_ATTR_INODE: u32 = 0x00000004;
/// Is an index.
pub const BEFS_INODE_LOGGED: u32 = 0x00000008;
/// Deleted.
pub const BEFS_INODE_DELETED: u32 = 0x00000010;
/// Long symlink.
pub const BEFS_INODE_LONG_SYMLINK: u32 = 0x00000040;
/// Has permanent flags.
pub const BEFS_INODE_PERMANENT_FLAGS: u32 = 0x0000FFFF;

// ---------------------------------------------------------------------------
// Data stream types
// ---------------------------------------------------------------------------

/// Direct data in inode.
pub const BEFS_DATA_SMALL: u32 = 0;
/// Single indirect block run.
pub const BEFS_DATA_REGULAR: u32 = 1;
/// B+tree (large files).
pub const BEFS_DATA_LARGE: u32 = 2;

// ---------------------------------------------------------------------------
// B+tree node types
// ---------------------------------------------------------------------------

/// Interior (index) node.
pub const BEFS_BTREE_INTERIOR: u32 = 0;
/// Leaf node.
pub const BEFS_BTREE_LEAF: u32 = 1;
/// Overflow node.
pub const BEFS_BTREE_OVERFLOW: u32 = 2;
/// Duplicate node.
pub const BEFS_BTREE_DUPLICATE: u32 = 3;

// ---------------------------------------------------------------------------
// File types (BeOS MIME type indices)
// ---------------------------------------------------------------------------

/// Regular file.
pub const BEFS_TYPE_FILE: u32 = 0;
/// Directory.
pub const BEFS_TYPE_DIRECTORY: u32 = 1;
/// Symlink.
pub const BEFS_TYPE_SYMLINK: u32 = 2;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Maximum file name length.
pub const BEFS_NAME_LEN: u32 = 255;
/// Symlink length stored in inode.
pub const BEFS_SYMLINK_LEN: u32 = 144;
/// Number of direct block runs in inode.
pub const BEFS_NUM_DIRECT_BLOCKS: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(BEFS_SUPER_MAGIC, 0x42465331);
    }

    #[test]
    fn test_magics_related() {
        assert_eq!(BEFS_SUPER_MAGIC, BEFS_SUPER_MAGIC1);
    }

    #[test]
    fn test_inode_flags_distinct() {
        let flags = [
            BEFS_INODE_IN_USE,
            BEFS_INODE_ATTR_INODE,
            BEFS_INODE_LOGGED,
            BEFS_INODE_DELETED,
            BEFS_INODE_LONG_SYMLINK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_data_types_sequential() {
        assert_eq!(BEFS_DATA_SMALL, 0);
        assert_eq!(BEFS_DATA_REGULAR, 1);
        assert_eq!(BEFS_DATA_LARGE, 2);
    }

    #[test]
    fn test_btree_types_sequential() {
        assert_eq!(BEFS_BTREE_INTERIOR, 0);
        assert_eq!(BEFS_BTREE_LEAF, 1);
        assert_eq!(BEFS_BTREE_OVERFLOW, 2);
        assert_eq!(BEFS_BTREE_DUPLICATE, 3);
    }

    #[test]
    fn test_file_types_sequential() {
        assert_eq!(BEFS_TYPE_FILE, 0);
        assert_eq!(BEFS_TYPE_DIRECTORY, 1);
        assert_eq!(BEFS_TYPE_SYMLINK, 2);
    }

    #[test]
    fn test_name_length() {
        assert_eq!(BEFS_NAME_LEN, 255);
    }

    #[test]
    fn test_direct_blocks() {
        assert_eq!(BEFS_NUM_DIRECT_BLOCKS, 12);
    }

    #[test]
    fn test_max_run_length() {
        assert_eq!(BEFS_MAX_RUN_LENGTH, 0xFFFF);
    }
}
