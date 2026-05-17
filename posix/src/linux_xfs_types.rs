//! `<linux/xfs_fs.h>` — XFS filesystem constants.
//!
//! XFS is a high-performance journaling filesystem originally developed
//! by SGI for IRIX. It excels at parallel I/O, large files, and high
//! metadata throughput. It uses allocation groups for parallelism,
//! B+ trees for all on-disk structures, and delayed allocation.

// ---------------------------------------------------------------------------
// XFS magic numbers
// ---------------------------------------------------------------------------

/// XFS superblock magic ("XFSB").
pub const XFS_SUPER_MAGIC: u32 = 0x5846_5342;
/// AG free space block magic ("XAGF").
pub const XFS_AGF_MAGIC: u32 = 0x5841_4746;
/// AG inode header magic ("XAGI").
pub const XFS_AGI_MAGIC: u32 = 0x5841_4749;
/// Directory block magic (v5).
pub const XFS_DIR3_BLOCK_MAGIC: u32 = 0x5842_4433;

// ---------------------------------------------------------------------------
// Superblock feature flags (version 5)
// ---------------------------------------------------------------------------

/// CRC32C checksums on all metadata.
pub const XFS_SB_FEAT_INCOMPAT_FTYPE: u32 = 1 << 0;
/// Sparse inode chunks.
pub const XFS_SB_FEAT_INCOMPAT_SPINODES: u32 = 1 << 1;
/// Metadata UUID (for multi-device).
pub const XFS_SB_FEAT_INCOMPAT_META_UUID: u32 = 1 << 2;
/// Big timestamps (2038-safe).
pub const XFS_SB_FEAT_INCOMPAT_BIGTIME: u32 = 1 << 3;
/// Large extent counters.
pub const XFS_SB_FEAT_INCOMPAT_NREXT64: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Inode format types
// ---------------------------------------------------------------------------

/// Local (inline data in inode).
pub const XFS_DINODE_FMT_LOCAL: u8 = 0;
/// Extents (extent list in inode).
pub const XFS_DINODE_FMT_EXTENTS: u8 = 1;
/// B-tree (extent B-tree root in inode).
pub const XFS_DINODE_FMT_BTREE: u8 = 2;
/// Device (character/block device).
pub const XFS_DINODE_FMT_DEV: u8 = 3;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Minimum block size.
pub const XFS_MIN_BLOCKSIZE: u32 = 512;
/// Maximum block size.
pub const XFS_MAX_BLOCKSIZE: u32 = 65536;
/// Default block size.
pub const XFS_DEFAULT_BLOCKSIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Allocation group constants
// ---------------------------------------------------------------------------

/// Minimum AG size (16 MiB in 4K blocks).
pub const XFS_AG_MIN_BLOCKS: u32 = 4096;
/// Maximum AG count.
pub const XFS_MAX_AGNUMBER: u32 = 0xFFFF_FFFE;

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Realtime device data.
pub const XFS_DIFLAG_REALTIME: u16 = 1 << 0;
/// Preallocated (unwritten extents).
pub const XFS_DIFLAG_PREALLOC: u16 = 1 << 1;
/// Use newrtbm (realtime summary).
pub const XFS_DIFLAG_NEWRTBM: u16 = 1 << 2;
/// Immutable.
pub const XFS_DIFLAG_IMMUTABLE: u16 = 1 << 3;
/// Append-only.
pub const XFS_DIFLAG_APPEND: u16 = 1 << 4;
/// Synchronous writes.
pub const XFS_DIFLAG_SYNC: u16 = 1 << 5;
/// No atime updates.
pub const XFS_DIFLAG_NOATIME: u16 = 1 << 6;
/// No dump.
pub const XFS_DIFLAG_NODUMP: u16 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_numbers_distinct() {
        let magics = [XFS_SUPER_MAGIC, XFS_AGF_MAGIC, XFS_AGI_MAGIC, XFS_DIR3_BLOCK_MAGIC];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_feature_flags_no_overlap() {
        let feats = [
            XFS_SB_FEAT_INCOMPAT_FTYPE, XFS_SB_FEAT_INCOMPAT_SPINODES,
            XFS_SB_FEAT_INCOMPAT_META_UUID, XFS_SB_FEAT_INCOMPAT_BIGTIME,
            XFS_SB_FEAT_INCOMPAT_NREXT64,
        ];
        for i in 0..feats.len() {
            assert!(feats[i].is_power_of_two());
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_inode_formats_distinct() {
        let fmts = [
            XFS_DINODE_FMT_LOCAL, XFS_DINODE_FMT_EXTENTS,
            XFS_DINODE_FMT_BTREE, XFS_DINODE_FMT_DEV,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_block_sizes() {
        assert!(XFS_MIN_BLOCKSIZE <= XFS_DEFAULT_BLOCKSIZE);
        assert!(XFS_DEFAULT_BLOCKSIZE <= XFS_MAX_BLOCKSIZE);
        assert!(XFS_MIN_BLOCKSIZE.is_power_of_two());
        assert!(XFS_DEFAULT_BLOCKSIZE.is_power_of_two());
        assert!(XFS_MAX_BLOCKSIZE.is_power_of_two());
    }

    #[test]
    fn test_inode_flags_no_overlap() {
        let flags = [
            XFS_DIFLAG_REALTIME, XFS_DIFLAG_PREALLOC, XFS_DIFLAG_NEWRTBM,
            XFS_DIFLAG_IMMUTABLE, XFS_DIFLAG_APPEND, XFS_DIFLAG_SYNC,
            XFS_DIFLAG_NOATIME, XFS_DIFLAG_NODUMP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
