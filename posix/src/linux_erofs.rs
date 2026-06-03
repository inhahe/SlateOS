//! `<linux/erofs_fs.h>` — EROFS (Enhanced Read-Only File System) constants.
//!
//! EROFS is a lightweight read-only filesystem designed for performance
//! and space efficiency. It's commonly used for Android system partitions,
//! container images, and firmware. It supports transparent compression
//! (LZ4, LZMA) and inline data.

// ---------------------------------------------------------------------------
// EROFS magic and superblock
// ---------------------------------------------------------------------------

/// EROFS filesystem magic number.
pub const EROFS_MAGIC: u32 = 0xE0F5_E1E2;
/// Superblock offset in the partition.
pub const EROFS_SUPER_OFFSET: u32 = 1024;
/// Default block size (4 KiB).
pub const EROFS_BLKSIZ: u32 = 4096;

// ---------------------------------------------------------------------------
// Inode format / layout
// ---------------------------------------------------------------------------

/// Compact inode (32 bytes).
pub const EROFS_INODE_LAYOUT_COMPACT: u8 = 0;
/// Extended inode (64 bytes).
pub const EROFS_INODE_LAYOUT_EXTENDED: u8 = 1;

// ---------------------------------------------------------------------------
// Data layout types
// ---------------------------------------------------------------------------

/// Flat plain (no compression, data follows inode inline area).
pub const EROFS_INODE_FLAT_PLAIN: u8 = 0;
/// Compressed (generic).
pub const EROFS_INODE_FLAT_COMPRESSION_LEGACY: u8 = 1;
/// Flat inline (tail data packed inline after inode).
pub const EROFS_INODE_FLAT_INLINE: u8 = 2;
/// Compressed with new-style indexes.
pub const EROFS_INODE_FLAT_COMPRESSION: u8 = 3;
/// Chunk-based (large files split into indexed chunks).
pub const EROFS_INODE_CHUNK_BASED: u8 = 4;

// ---------------------------------------------------------------------------
// Compression algorithms
// ---------------------------------------------------------------------------

/// LZ4 compression.
pub const EROFS_COMPRESS_LZ4: u8 = 0;
/// LZMA compression.
pub const EROFS_COMPRESS_LZMA: u8 = 1;
/// Deflate (zlib) compression.
pub const EROFS_COMPRESS_DEFLATE: u8 = 2;

// ---------------------------------------------------------------------------
// Feature flags (compat)
// ---------------------------------------------------------------------------

/// Superblock checksum present.
pub const EROFS_FEATURE_COMPAT_SB_CHKSUM: u32 = 1 << 0;
/// Multi-device support.
pub const EROFS_FEATURE_COMPAT_MTIME: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Feature flags (incompat)
// ---------------------------------------------------------------------------

/// Compressed inodes present.
pub const EROFS_FEATURE_INCOMPAT_COMPRESSED: u32 = 1 << 0;
/// Chunked files present.
pub const EROFS_FEATURE_INCOMPAT_CHUNKED: u32 = 1 << 2;
/// Device table present.
pub const EROFS_FEATURE_INCOMPAT_DEVICE_TABLE: u32 = 1 << 3;
/// Compression configs in superblock.
pub const EROFS_FEATURE_INCOMPAT_COMPR_CFGS: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// File type in directory entries
// ---------------------------------------------------------------------------

/// Unknown.
pub const EROFS_FT_UNKNOWN: u8 = 0;
/// Regular file.
pub const EROFS_FT_REG_FILE: u8 = 1;
/// Directory.
pub const EROFS_FT_DIR: u8 = 2;
/// Character device.
pub const EROFS_FT_CHRDEV: u8 = 3;
/// Block device.
pub const EROFS_FT_BLKDEV: u8 = 4;
/// FIFO.
pub const EROFS_FT_FIFO: u8 = 5;
/// Socket.
pub const EROFS_FT_SOCK: u8 = 6;
/// Symbolic link.
pub const EROFS_FT_SYMLINK: u8 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_and_super() {
        assert_eq!(EROFS_MAGIC, 0xE0F5_E1E2);
        assert_eq!(EROFS_SUPER_OFFSET, 1024);
        assert_eq!(EROFS_BLKSIZ, 4096);
    }

    #[test]
    fn test_inode_layouts_distinct() {
        assert_ne!(EROFS_INODE_LAYOUT_COMPACT, EROFS_INODE_LAYOUT_EXTENDED);
    }

    #[test]
    fn test_data_layouts_distinct() {
        let layouts = [
            EROFS_INODE_FLAT_PLAIN,
            EROFS_INODE_FLAT_COMPRESSION_LEGACY,
            EROFS_INODE_FLAT_INLINE,
            EROFS_INODE_FLAT_COMPRESSION,
            EROFS_INODE_CHUNK_BASED,
        ];
        for i in 0..layouts.len() {
            for j in (i + 1)..layouts.len() {
                assert_ne!(layouts[i], layouts[j]);
            }
        }
    }

    #[test]
    fn test_compression_algos_distinct() {
        let algos = [
            EROFS_COMPRESS_LZ4,
            EROFS_COMPRESS_LZMA,
            EROFS_COMPRESS_DEFLATE,
        ];
        for i in 0..algos.len() {
            for j in (i + 1)..algos.len() {
                assert_ne!(algos[i], algos[j]);
            }
        }
    }

    #[test]
    fn test_incompat_features_no_overlap() {
        let feats = [
            EROFS_FEATURE_INCOMPAT_COMPRESSED,
            EROFS_FEATURE_INCOMPAT_CHUNKED,
            EROFS_FEATURE_INCOMPAT_DEVICE_TABLE,
            EROFS_FEATURE_INCOMPAT_COMPR_CFGS,
        ];
        for i in 0..feats.len() {
            assert!(feats[i].is_power_of_two());
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            EROFS_FT_UNKNOWN,
            EROFS_FT_REG_FILE,
            EROFS_FT_DIR,
            EROFS_FT_CHRDEV,
            EROFS_FT_BLKDEV,
            EROFS_FT_FIFO,
            EROFS_FT_SOCK,
            EROFS_FT_SYMLINK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
