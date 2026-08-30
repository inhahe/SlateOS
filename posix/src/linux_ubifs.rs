//! `<linux/ubifs.h>` — UBIFS (UBI File System) constants.
//!
//! UBIFS is a flash filesystem that runs on top of UBI volumes.
//! It's designed for raw NAND flash with journaling, compression,
//! and write-back caching. Unlike JFFS2, it scales well to large
//! flash devices with fast mount times via on-flash indexing.

// ---------------------------------------------------------------------------
// UBIFS magic numbers
// ---------------------------------------------------------------------------

/// UBIFS superblock node magic.
pub const UBIFS_SB_NODE_MAGIC: u32 = 0x0653_1B06;
/// Common UBIFS node magic.
pub const UBIFS_NODE_MAGIC: u32 = 0x0653_1B00;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Inode node.
pub const UBIFS_INO_NODE: u8 = 0;
/// Data node.
pub const UBIFS_DATA_NODE: u8 = 1;
/// Directory entry node.
pub const UBIFS_DENT_NODE: u8 = 2;
/// Extended attribute entry node.
pub const UBIFS_XENT_NODE: u8 = 3;
/// Truncation node.
pub const UBIFS_TRUN_NODE: u8 = 4;
/// Padding node.
pub const UBIFS_PAD_NODE: u8 = 5;
/// Superblock node.
pub const UBIFS_SB_NODE: u8 = 6;
/// Master node.
pub const UBIFS_MST_NODE: u8 = 7;
/// Reference node (journal).
pub const UBIFS_REF_NODE: u8 = 8;
/// Index node (TNC B-tree).
pub const UBIFS_IDX_NODE: u8 = 9;
/// Commit start node.
pub const UBIFS_CS_NODE: u8 = 10;
/// Orphan node.
pub const UBIFS_ORPH_NODE: u8 = 11;
/// Authentication node.
pub const UBIFS_AUTH_NODE: u8 = 12;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// No compression.
pub const UBIFS_COMPR_NONE: u8 = 0;
/// LZO compression.
pub const UBIFS_COMPR_LZO: u8 = 1;
/// zlib compression.
pub const UBIFS_COMPR_ZLIB: u8 = 2;
/// Zstd compression.
pub const UBIFS_COMPR_ZSTD: u8 = 3;

// ---------------------------------------------------------------------------
// File types (directory entries)
// ---------------------------------------------------------------------------

/// Regular file.
pub const UBIFS_ITYPE_REG: u8 = 0;
/// Directory.
pub const UBIFS_ITYPE_DIR: u8 = 1;
/// Symbolic link.
pub const UBIFS_ITYPE_LNK: u8 = 2;
/// Block device.
pub const UBIFS_ITYPE_BLK: u8 = 3;
/// Character device.
pub const UBIFS_ITYPE_CHR: u8 = 4;
/// FIFO.
pub const UBIFS_ITYPE_FIFO: u8 = 5;
/// Socket.
pub const UBIFS_ITYPE_SOCK: u8 = 6;

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Big LPT (large flash support).
pub const UBIFS_FLG_BIGLPT: u8 = 1 << 1;
/// Space fixup.
pub const UBIFS_FLG_SPACE_FIXUP: u8 = 1 << 2;
/// Double hash (case-insensitive).
pub const UBIFS_FLG_DOUBLE_HASH: u8 = 1 << 3;
/// Encryption support.
pub const UBIFS_FLG_ENCRYPTION: u8 = 1 << 4;
/// Authentication support.
pub const UBIFS_FLG_AUTHENTICATION: u8 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_numbers() {
        assert_ne!(UBIFS_SB_NODE_MAGIC, UBIFS_NODE_MAGIC);
    }

    #[test]
    fn test_node_types_distinct() {
        let types = [
            UBIFS_INO_NODE,
            UBIFS_DATA_NODE,
            UBIFS_DENT_NODE,
            UBIFS_XENT_NODE,
            UBIFS_TRUN_NODE,
            UBIFS_PAD_NODE,
            UBIFS_SB_NODE,
            UBIFS_MST_NODE,
            UBIFS_REF_NODE,
            UBIFS_IDX_NODE,
            UBIFS_CS_NODE,
            UBIFS_ORPH_NODE,
            UBIFS_AUTH_NODE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_compression_types_distinct() {
        let types = [
            UBIFS_COMPR_NONE,
            UBIFS_COMPR_LZO,
            UBIFS_COMPR_ZLIB,
            UBIFS_COMPR_ZSTD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            UBIFS_ITYPE_REG,
            UBIFS_ITYPE_DIR,
            UBIFS_ITYPE_LNK,
            UBIFS_ITYPE_BLK,
            UBIFS_ITYPE_CHR,
            UBIFS_ITYPE_FIFO,
            UBIFS_ITYPE_SOCK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_feature_flags_no_overlap() {
        let flags = [
            UBIFS_FLG_BIGLPT,
            UBIFS_FLG_SPACE_FIXUP,
            UBIFS_FLG_DOUBLE_HASH,
            UBIFS_FLG_ENCRYPTION,
            UBIFS_FLG_AUTHENTICATION,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
