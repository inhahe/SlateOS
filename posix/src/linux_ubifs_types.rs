//! `<linux/ubifs.h>` — UBIFS (Unsorted Block Image File System) constants.
//!
//! UBIFS runs on top of UBI volumes (flash storage).
//! These constants define node types, compression modes,
//! and key types.

// ---------------------------------------------------------------------------
// Magic
// ---------------------------------------------------------------------------

/// UBIFS superblock magic.
pub const UBIFS_SUPER_MAGIC: u32 = 0x24051905;
/// Node magic (common header).
pub const UBIFS_NODE_MAGIC: u32 = 0x06101831;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Inode node.
pub const UBIFS_INO_NODE: u32 = 0;
/// Data node.
pub const UBIFS_DATA_NODE: u32 = 1;
/// Directory entry node.
pub const UBIFS_DENT_NODE: u32 = 2;
/// Extended attr entry.
pub const UBIFS_XENT_NODE: u32 = 3;
/// Truncation node.
pub const UBIFS_TRUN_NODE: u32 = 4;
/// Padding node.
pub const UBIFS_PAD_NODE: u32 = 5;
/// Superblock node.
pub const UBIFS_SB_NODE: u32 = 6;
/// Master node.
pub const UBIFS_MST_NODE: u32 = 7;
/// Commit start ref node.
pub const UBIFS_REF_NODE: u32 = 8;
/// Index node.
pub const UBIFS_IDX_NODE: u32 = 9;
/// Commit start node.
pub const UBIFS_CS_NODE: u32 = 10;
/// Orphan node.
pub const UBIFS_ORPH_NODE: u32 = 11;
/// Authentication node.
pub const UBIFS_AUTH_NODE: u32 = 12;
/// Signature node.
pub const UBIFS_SIG_NODE: u32 = 13;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// No compression.
pub const UBIFS_COMPR_NONE: u32 = 0;
/// LZO compression.
pub const UBIFS_COMPR_LZO: u32 = 1;
/// Zlib compression.
pub const UBIFS_COMPR_ZLIB: u32 = 2;
/// Zstd compression.
pub const UBIFS_COMPR_ZSTD: u32 = 3;

// ---------------------------------------------------------------------------
// Key hash types
// ---------------------------------------------------------------------------

/// R5 hash.
pub const UBIFS_KEY_HASH_R5: u32 = 0;
/// Test hash.
pub const UBIFS_KEY_HASH_TEST: u32 = 1;

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Compression override: compr.
pub const UBIFS_COMPR_FL: u32 = 0x01;
/// Synchronous dir.
pub const UBIFS_SYNC_FL: u32 = 0x02;
/// Immutable.
pub const UBIFS_IMMUTABLE_FL: u32 = 0x04;
/// Append only.
pub const UBIFS_APPEND_FL: u32 = 0x08;
/// Directory sync.
pub const UBIFS_DIRSYNC_FL: u32 = 0x10;
/// Xattr inode.
pub const UBIFS_XATTR_FL: u32 = 0x20;
/// Encrypted inode.
pub const UBIFS_CRYPT_FL: u32 = 0x40;

// ---------------------------------------------------------------------------
// LEB properties
// ---------------------------------------------------------------------------

/// Category count.
pub const UBIFS_LPT_FANOUT_SHIFT: u32 = 3;
/// Max LEBs per inode.
pub const UBIFS_MAX_LEBS: u32 = 0x7FFFFFFF;
/// Minimum I/O unit.
pub const UBIFS_MIN_IO_SZ: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(UBIFS_SUPER_MAGIC, 0x24051905);
    }

    #[test]
    fn test_node_magic() {
        assert_eq!(UBIFS_NODE_MAGIC, 0x06101831);
    }

    #[test]
    fn test_node_types_sequential() {
        assert_eq!(UBIFS_INO_NODE, 0);
        assert_eq!(UBIFS_DATA_NODE, 1);
        assert_eq!(UBIFS_SIG_NODE, 13);
    }

    #[test]
    fn test_node_types_distinct() {
        let types = [
            UBIFS_INO_NODE, UBIFS_DATA_NODE, UBIFS_DENT_NODE,
            UBIFS_XENT_NODE, UBIFS_TRUN_NODE, UBIFS_PAD_NODE,
            UBIFS_SB_NODE, UBIFS_MST_NODE, UBIFS_REF_NODE,
            UBIFS_IDX_NODE, UBIFS_CS_NODE, UBIFS_ORPH_NODE,
            UBIFS_AUTH_NODE, UBIFS_SIG_NODE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_compr_types_sequential() {
        assert_eq!(UBIFS_COMPR_NONE, 0);
        assert_eq!(UBIFS_COMPR_LZO, 1);
        assert_eq!(UBIFS_COMPR_ZLIB, 2);
        assert_eq!(UBIFS_COMPR_ZSTD, 3);
    }

    #[test]
    fn test_inode_flags_power_of_two() {
        let flags = [
            UBIFS_COMPR_FL, UBIFS_SYNC_FL, UBIFS_IMMUTABLE_FL,
            UBIFS_APPEND_FL, UBIFS_DIRSYNC_FL, UBIFS_XATTR_FL,
            UBIFS_CRYPT_FL,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_key_hash_types() {
        assert_eq!(UBIFS_KEY_HASH_R5, 0);
        assert_eq!(UBIFS_KEY_HASH_TEST, 1);
    }
}
