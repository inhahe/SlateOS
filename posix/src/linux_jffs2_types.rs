//! `<linux/jffs2.h>` — JFFS2 (Journalling Flash File System v2) constants.
//!
//! JFFS2 is a log-structured filesystem for NOR/NAND flash.
//! These constants define magic numbers, node types,
//! compression types, and inode flags.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// JFFS2 super magic.
pub const JFFS2_SUPER_MAGIC: u32 = 0x72B6;
/// JFFS2 magic bitmask.
pub const JFFS2_MAGIC_BITMASK: u16 = 0x1985;
/// Old magic bitmask.
pub const JFFS2_OLD_MAGIC_BITMASK: u16 = 0x1984;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Dirent node.
pub const JFFS2_NODETYPE_DIRENT: u16 = 0x2001;
/// Inode node.
pub const JFFS2_NODETYPE_INODE: u16 = 0x2002;
/// Clean marker.
pub const JFFS2_NODETYPE_CLEANMARKER: u16 = 0x2003;
/// Padding.
pub const JFFS2_NODETYPE_PADDING: u16 = 0x2004;
/// Summary.
pub const JFFS2_NODETYPE_SUMMARY: u16 = 0x2006;
/// Xattr.
pub const JFFS2_NODETYPE_XATTR: u16 = 0x2008;
/// Xref.
pub const JFFS2_NODETYPE_XREF: u16 = 0x2009;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// No compression.
pub const JFFS2_COMPR_NONE: u8 = 0x00;
/// Zero compression.
pub const JFFS2_COMPR_ZERO: u8 = 0x01;
/// RTime compression.
pub const JFFS2_COMPR_RTIME: u8 = 0x02;
/// Rubinmips compression.
pub const JFFS2_COMPR_RUBINMIPS: u8 = 0x03;
/// Copy compression.
pub const JFFS2_COMPR_COPY: u8 = 0x04;
/// Dynrubin compression.
pub const JFFS2_COMPR_DYNRUBIN: u8 = 0x05;
/// Zlib compression.
pub const JFFS2_COMPR_ZLIB: u8 = 0x06;
/// LZO compression.
pub const JFFS2_COMPR_LZO: u8 = 0x07;

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Inode is valid.
pub const JFFS2_INO_FLAG_VALID: u32 = 0x01;
/// Inode is obsolete.
pub const JFFS2_INO_FLAG_USERCOMPR: u32 = 0x02;

// ---------------------------------------------------------------------------
// Dirent types (matching Linux DT_*)
// ---------------------------------------------------------------------------

/// Unknown.
pub const JFFS2_DT_UNKNOWN: u8 = 0;
/// FIFO.
pub const JFFS2_DT_FIFO: u8 = 1;
/// Character device.
pub const JFFS2_DT_CHR: u8 = 2;
/// Directory.
pub const JFFS2_DT_DIR: u8 = 4;
/// Block device.
pub const JFFS2_DT_BLK: u8 = 6;
/// Regular file.
pub const JFFS2_DT_REG: u8 = 8;
/// Symbolic link.
pub const JFFS2_DT_LNK: u8 = 10;
/// Socket.
pub const JFFS2_DT_SOCK: u8 = 12;

// ---------------------------------------------------------------------------
// Xattr prefixes
// ---------------------------------------------------------------------------

/// User xattr.
pub const JFFS2_XPREFIX_USER: u8 = 1;
/// Security xattr.
pub const JFFS2_XPREFIX_SECURITY: u8 = 2;
/// ACL access.
pub const JFFS2_XPREFIX_ACL_ACCESS: u8 = 3;
/// ACL default.
pub const JFFS2_XPREFIX_ACL_DEFAULT: u8 = 4;
/// Trusted xattr.
pub const JFFS2_XPREFIX_TRUSTED: u8 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(JFFS2_SUPER_MAGIC, 0x72B6);
    }

    #[test]
    fn test_magic_bitmask() {
        assert_eq!(JFFS2_MAGIC_BITMASK, 0x1985);
    }

    #[test]
    fn test_node_types_distinct() {
        let types: [u16; 7] = [
            JFFS2_NODETYPE_DIRENT, JFFS2_NODETYPE_INODE,
            JFFS2_NODETYPE_CLEANMARKER, JFFS2_NODETYPE_PADDING,
            JFFS2_NODETYPE_SUMMARY, JFFS2_NODETYPE_XATTR,
            JFFS2_NODETYPE_XREF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_compr_types_sequential() {
        assert_eq!(JFFS2_COMPR_NONE, 0);
        assert_eq!(JFFS2_COMPR_ZERO, 1);
        assert_eq!(JFFS2_COMPR_LZO, 7);
    }

    #[test]
    fn test_dirent_types_distinct() {
        let types: [u8; 8] = [
            JFFS2_DT_UNKNOWN, JFFS2_DT_FIFO, JFFS2_DT_CHR,
            JFFS2_DT_DIR, JFFS2_DT_BLK, JFFS2_DT_REG,
            JFFS2_DT_LNK, JFFS2_DT_SOCK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_xprefix_distinct() {
        let prefixes: [u8; 5] = [
            JFFS2_XPREFIX_USER, JFFS2_XPREFIX_SECURITY,
            JFFS2_XPREFIX_ACL_ACCESS, JFFS2_XPREFIX_ACL_DEFAULT,
            JFFS2_XPREFIX_TRUSTED,
        ];
        for i in 0..prefixes.len() {
            for j in (i + 1)..prefixes.len() {
                assert_ne!(prefixes[i], prefixes[j]);
            }
        }
    }
}
