//! `<linux/f2fs_fs.h>` — F2FS (Flash-Friendly File System) constants.
//!
//! F2FS is a log-structured filesystem optimized for NAND flash storage.
//! It uses a multi-head logging approach with hot/warm/cold data separation,
//! node address tables (NAT), and segment information tables (SIT) to
//! minimize write amplification on SSDs and eMMC.

// ---------------------------------------------------------------------------
// F2FS magic and superblock
// ---------------------------------------------------------------------------

/// F2FS superblock magic number.
pub const F2FS_MAGIC: u32 = 0xF2F5_2010;
/// Superblock offset (1024 bytes from partition start).
pub const F2FS_SUPER_OFFSET: u32 = 1024;
/// Default block size (4 KiB).
pub const F2FS_BLKSIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Inode types
// ---------------------------------------------------------------------------

/// Regular file.
pub const F2FS_FT_REG_FILE: u8 = 1;
/// Directory.
pub const F2FS_FT_DIR: u8 = 2;
/// Character device.
pub const F2FS_FT_CHRDEV: u8 = 3;
/// Block device.
pub const F2FS_FT_BLKDEV: u8 = 4;
/// FIFO.
pub const F2FS_FT_FIFO: u8 = 5;
/// Socket.
pub const F2FS_FT_SOCK: u8 = 6;
/// Symbolic link.
pub const F2FS_FT_SYMLINK: u8 = 7;

// ---------------------------------------------------------------------------
// Data temperature (hot/warm/cold classification)
// ---------------------------------------------------------------------------

/// Hot data (frequently modified).
pub const CURSEG_HOT_DATA: u8 = 0;
/// Warm data (moderately modified).
pub const CURSEG_WARM_DATA: u8 = 1;
/// Cold data (rarely modified).
pub const CURSEG_COLD_DATA: u8 = 2;
/// Hot node (directory NAT entries).
pub const CURSEG_HOT_NODE: u8 = 3;
/// Warm node (file NAT entries).
pub const CURSEG_WARM_NODE: u8 = 4;
/// Cold node (indirect nodes).
pub const CURSEG_COLD_NODE: u8 = 5;

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Encryption support.
pub const F2FS_FEATURE_ENCRYPT: u32 = 1 << 0;
/// Flexible inline xattr.
pub const F2FS_FEATURE_FLEXIBLE_INLINE_XATTR: u32 = 1 << 2;
/// Quota support.
pub const F2FS_FEATURE_QUOTA_INO: u32 = 1 << 3;
/// Inode creation time.
pub const F2FS_FEATURE_INODE_CRTIME: u32 = 1 << 4;
/// Verity (fs-verity).
pub const F2FS_FEATURE_VERITY: u32 = 1 << 5;
/// Case-insensitive directory lookup.
pub const F2FS_FEATURE_CASEFOLD: u32 = 1 << 8;
/// Compression support.
pub const F2FS_FEATURE_COMPRESSION: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Compression algorithms
// ---------------------------------------------------------------------------

/// LZO compression.
pub const F2FS_COMPRESS_LZO: u8 = 0;
/// LZ4 compression.
pub const F2FS_COMPRESS_LZ4: u8 = 1;
/// Zstd compression.
pub const F2FS_COMPRESS_ZSTD: u8 = 2;
/// LZORLE compression.
pub const F2FS_COMPRESS_LZORLE: u8 = 3;

// ---------------------------------------------------------------------------
// GC (garbage collection) modes
// ---------------------------------------------------------------------------

/// Background GC.
pub const GC_NORMAL: u8 = 0;
/// Foreground (urgent) GC.
pub const GC_URGENT: u8 = 1;
/// Idle GC (low priority).
pub const GC_IDLE: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_and_super() {
        assert_eq!(F2FS_MAGIC, 0xF2F5_2010);
        assert_eq!(F2FS_SUPER_OFFSET, 1024);
        assert_eq!(F2FS_BLKSIZE, 4096);
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            F2FS_FT_REG_FILE,
            F2FS_FT_DIR,
            F2FS_FT_CHRDEV,
            F2FS_FT_BLKDEV,
            F2FS_FT_FIFO,
            F2FS_FT_SOCK,
            F2FS_FT_SYMLINK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_cursegs_distinct() {
        let segs = [
            CURSEG_HOT_DATA,
            CURSEG_WARM_DATA,
            CURSEG_COLD_DATA,
            CURSEG_HOT_NODE,
            CURSEG_WARM_NODE,
            CURSEG_COLD_NODE,
        ];
        for i in 0..segs.len() {
            for j in (i + 1)..segs.len() {
                assert_ne!(segs[i], segs[j]);
            }
        }
    }

    #[test]
    fn test_feature_flags_no_overlap() {
        let feats = [
            F2FS_FEATURE_ENCRYPT,
            F2FS_FEATURE_FLEXIBLE_INLINE_XATTR,
            F2FS_FEATURE_QUOTA_INO,
            F2FS_FEATURE_INODE_CRTIME,
            F2FS_FEATURE_VERITY,
            F2FS_FEATURE_CASEFOLD,
            F2FS_FEATURE_COMPRESSION,
        ];
        for i in 0..feats.len() {
            assert!(feats[i].is_power_of_two());
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_compression_algos_distinct() {
        let algos = [
            F2FS_COMPRESS_LZO,
            F2FS_COMPRESS_LZ4,
            F2FS_COMPRESS_ZSTD,
            F2FS_COMPRESS_LZORLE,
        ];
        for i in 0..algos.len() {
            for j in (i + 1)..algos.len() {
                assert_ne!(algos[i], algos[j]);
            }
        }
    }

    #[test]
    fn test_gc_modes_distinct() {
        let modes = [GC_NORMAL, GC_URGENT, GC_IDLE];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
