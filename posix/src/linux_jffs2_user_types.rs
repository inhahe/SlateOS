//! `<mtd/jffs2-user.h>` — JFFS2 userspace image-builder constants.
//!
//! Constants used to construct JFFS2 (Journalling Flash File System
//! version 2) images from userspace, e.g. by `mkfs.jffs2`. JFFS2 is
//! the standard FS for raw-MTD NOR/NAND flash on small embedded
//! Linux devices.

// ---------------------------------------------------------------------------
// JFFS2 node magic numbers (struct jffs2_unknown_node.magic)
// ---------------------------------------------------------------------------

/// Standard JFFS2 magic.
pub const JFFS2_MAGIC_BITMASK: u16 = 0x1985;
/// Old JFFS (v1) magic — pre-2.4 kernels; rejected by jffs2.
pub const JFFS2_OLD_MAGIC_BITMASK: u16 = 0x1984;

// ---------------------------------------------------------------------------
// Node type codes
// ---------------------------------------------------------------------------

/// In-band node-type mask.
pub const JFFS2_NODETYPE_MASK: u16 = 0x1fff;
/// Compression / no-clean bit (set on nodes that mustn't be erased).
pub const JFFS2_FEATURE_INCOMPAT: u16 = 0xc000;
/// "Feature: relocatable on incompatibility" bit.
pub const JFFS2_FEATURE_ROCOMPAT: u16 = 0x8000;
/// "Feature: read-only on incompatibility" bit.
pub const JFFS2_FEATURE_RWCOMPAT_COPY: u16 = 0x4000;
/// "Feature: delete on incompatibility" bit.
pub const JFFS2_FEATURE_RWCOMPAT_DELETE: u16 = 0x0000;

/// Directory entry node.
pub const JFFS2_NODETYPE_DIRENT: u16 = JFFS2_FEATURE_INCOMPAT | 0x0001;
/// Inode metadata node.
pub const JFFS2_NODETYPE_INODE: u16 = JFFS2_FEATURE_INCOMPAT | 0x0002;
/// Cleanmarker (per-erase-block sanity marker).
pub const JFFS2_NODETYPE_CLEANMARKER: u16 = JFFS2_FEATURE_RWCOMPAT_DELETE | 0x0003;
/// Padding (skip-over) node.
pub const JFFS2_NODETYPE_PADDING: u16 = JFFS2_FEATURE_RWCOMPAT_DELETE | 0x0004;
/// Summary node (per-erase-block summary).
pub const JFFS2_NODETYPE_SUMMARY: u16 = JFFS2_FEATURE_RWCOMPAT_DELETE | 0x0006;
/// Extended attribute node.
pub const JFFS2_NODETYPE_XATTR: u16 = JFFS2_FEATURE_INCOMPAT | 0x0008;
/// Reference to an existing xattr from an inode.
pub const JFFS2_NODETYPE_XREF: u16 = JFFS2_FEATURE_INCOMPAT | 0x0009;

// ---------------------------------------------------------------------------
// Compression-type codes (struct jffs2_raw_inode.compr)
// ---------------------------------------------------------------------------

/// No compression.
pub const JFFS2_COMPR_NONE: u8 = 0x00;
/// All-zeros optimisation (no data stored).
pub const JFFS2_COMPR_ZERO: u8 = 0x01;
/// RTIME (delta) compression.
pub const JFFS2_COMPR_RTIME: u8 = 0x02;
/// RUBIN compression (very rarely used).
pub const JFFS2_COMPR_RUBINMIPS: u8 = 0x03;
/// Copy/move sentinel.
pub const JFFS2_COMPR_COPY: u8 = 0x04;
/// Dynamic-RUBIN.
pub const JFFS2_COMPR_DYNRUBIN: u8 = 0x05;
/// zlib (most common).
pub const JFFS2_COMPR_ZLIB: u8 = 0x06;
/// LZO (fast, common on embedded).
pub const JFFS2_COMPR_LZO: u8 = 0x07;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_distinct_and_close() {
        // Old/new magics differ by 1 (1984 vs 1985 — Orwell joke).
        assert_eq!(JFFS2_MAGIC_BITMASK - JFFS2_OLD_MAGIC_BITMASK, 1);
        assert_ne!(JFFS2_MAGIC_BITMASK, JFFS2_OLD_MAGIC_BITMASK);
    }

    #[test]
    fn test_node_type_masks_are_disjoint() {
        // The feature bits live above the node-type mask.
        assert_eq!(JFFS2_NODETYPE_MASK & JFFS2_FEATURE_INCOMPAT, 0);
        assert_eq!(JFFS2_NODETYPE_MASK & JFFS2_FEATURE_ROCOMPAT, 0);
        assert_eq!(JFFS2_NODETYPE_MASK & JFFS2_FEATURE_RWCOMPAT_COPY, 0);
    }

    #[test]
    fn test_node_types_distinct() {
        let nodes = [
            JFFS2_NODETYPE_DIRENT,
            JFFS2_NODETYPE_INODE,
            JFFS2_NODETYPE_CLEANMARKER,
            JFFS2_NODETYPE_PADDING,
            JFFS2_NODETYPE_SUMMARY,
            JFFS2_NODETYPE_XATTR,
            JFFS2_NODETYPE_XREF,
        ];
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                assert_ne!(nodes[i], nodes[j]);
            }
        }
    }

    #[test]
    fn test_compr_codes_distinct() {
        let codes = [
            JFFS2_COMPR_NONE,
            JFFS2_COMPR_ZERO,
            JFFS2_COMPR_RTIME,
            JFFS2_COMPR_RUBINMIPS,
            JFFS2_COMPR_COPY,
            JFFS2_COMPR_DYNRUBIN,
            JFFS2_COMPR_ZLIB,
            JFFS2_COMPR_LZO,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
