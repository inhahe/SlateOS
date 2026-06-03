//! `<linux/jffs2.h>` — JFFS2 (Journalling Flash File System v2) constants.
//!
//! JFFS2 is a log-structured filesystem for NOR and NAND flash.
//! It writes data and metadata as nodes in a circular log, with
//! garbage collection reclaiming space. While superseded by UBIFS
//! for large NAND, JFFS2 remains common on small NOR flash in
//! embedded systems (routers, IoT devices).

// ---------------------------------------------------------------------------
// JFFS2 magic numbers
// ---------------------------------------------------------------------------

/// JFFS2 old magic bitmask.
pub const JFFS2_OLD_MAGIC_BITMASK: u16 = 0x1984;
/// JFFS2 magic bitmask (current).
pub const JFFS2_MAGIC_BITMASK: u16 = 0x1985;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Inode node.
pub const JFFS2_NODETYPE_INODE: u16 = 0x2001;
/// Directory entry node.
pub const JFFS2_NODETYPE_DIRENT: u16 = 0x2002;
/// Clean marker.
pub const JFFS2_NODETYPE_CLEANMARKER: u16 = 0x2003;
/// Padding node.
pub const JFFS2_NODETYPE_PADDING: u16 = 0x2004;
/// Summary node.
pub const JFFS2_NODETYPE_SUMMARY: u16 = 0x2006;
/// Extended attribute node.
pub const JFFS2_NODETYPE_XATTR: u16 = 0x2007;
/// Xattr reference node.
pub const JFFS2_NODETYPE_XREF: u16 = 0x2008;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// No compression.
pub const JFFS2_COMPR_NONE: u8 = 0x00;
/// Zlib compression.
pub const JFFS2_COMPR_ZLIB: u8 = 0x06;
/// LZMA compression.
pub const JFFS2_COMPR_LZMA: u8 = 0x07;
/// RTime compression (simple run-time).
pub const JFFS2_COMPR_RTIME: u8 = 0x05;

// ---------------------------------------------------------------------------
// File types (in directory entries)
// ---------------------------------------------------------------------------

/// Unknown.
pub const JFFS2_DT_UNKNOWN: u8 = 0;
/// Regular file.
pub const JFFS2_DT_REG: u8 = 8;
/// Directory.
pub const JFFS2_DT_DIR: u8 = 4;
/// Character device.
pub const JFFS2_DT_CHR: u8 = 2;
/// Block device.
pub const JFFS2_DT_BLK: u8 = 6;
/// FIFO.
pub const JFFS2_DT_FIFO: u8 = 1;
/// Socket.
pub const JFFS2_DT_SOCK: u8 = 12;
/// Symbolic link.
pub const JFFS2_DT_LNK: u8 = 10;

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Append only.
pub const JFFS2_INO_FLAG_APPEND: u32 = 1 << 0;
/// Immutable.
pub const JFFS2_INO_FLAG_IMMUTABLE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_values() {
        assert_ne!(JFFS2_OLD_MAGIC_BITMASK, JFFS2_MAGIC_BITMASK);
    }

    #[test]
    fn test_node_types_distinct() {
        let types = [
            JFFS2_NODETYPE_INODE,
            JFFS2_NODETYPE_DIRENT,
            JFFS2_NODETYPE_CLEANMARKER,
            JFFS2_NODETYPE_PADDING,
            JFFS2_NODETYPE_SUMMARY,
            JFFS2_NODETYPE_XATTR,
            JFFS2_NODETYPE_XREF,
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
            JFFS2_COMPR_NONE,
            JFFS2_COMPR_ZLIB,
            JFFS2_COMPR_LZMA,
            JFFS2_COMPR_RTIME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dir_types_distinct() {
        let types = [
            JFFS2_DT_UNKNOWN,
            JFFS2_DT_REG,
            JFFS2_DT_DIR,
            JFFS2_DT_CHR,
            JFFS2_DT_BLK,
            JFFS2_DT_FIFO,
            JFFS2_DT_SOCK,
            JFFS2_DT_LNK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_inode_flags_no_overlap() {
        assert_eq!(JFFS2_INO_FLAG_APPEND & JFFS2_INO_FLAG_IMMUTABLE, 0);
    }
}
