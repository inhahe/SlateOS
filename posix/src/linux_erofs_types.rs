//! `<linux/erofs_fs.h>` — EROFS (Enhanced Read-Only File System) constants.
//!
//! EROFS is a compressed read-only filesystem designed for
//! performance-critical scenarios. These constants define
//! superblock flags, inode formats, and compression algorithms.

// ---------------------------------------------------------------------------
// Superblock magic and sizes
// ---------------------------------------------------------------------------

/// EROFS super magic.
pub const EROFS_SUPER_MAGIC_V1: u32 = 0xE0F5E1E2;
/// Super block offset.
pub const EROFS_SUPER_OFFSET: u32 = 1024;
/// Block size shift (default 12 = 4 KiB).
pub const EROFS_BLKSIZ_BITS_DEFAULT: u32 = 12;

// ---------------------------------------------------------------------------
// Superblock feature flags (EROFS_FEATURE_*)
// ---------------------------------------------------------------------------

/// LZ4 0padding.
pub const EROFS_FEATURE_INCOMPAT_ZERO_PADDING: u32 = 0x00000001;
/// Compr cfgs.
pub const EROFS_FEATURE_INCOMPAT_COMPR_CFGS: u32 = 0x00000002;
/// Big pcluster.
pub const EROFS_FEATURE_INCOMPAT_BIG_PCLUSTER: u32 = 0x00000002;
/// Chunked file.
pub const EROFS_FEATURE_INCOMPAT_CHUNKED_FILE: u32 = 0x00000004;
/// Device table.
pub const EROFS_FEATURE_INCOMPAT_DEVICE_TABLE: u32 = 0x00000008;
/// Compression interlaced.
pub const EROFS_FEATURE_INCOMPAT_ZTAILPACKING: u32 = 0x00000010;
/// Fragments.
pub const EROFS_FEATURE_INCOMPAT_FRAGMENTS: u32 = 0x00000020;
/// Dedupe.
pub const EROFS_FEATURE_INCOMPAT_DEDUPE: u32 = 0x00000040;
/// Xattr filter.
pub const EROFS_FEATURE_INCOMPAT_XATTR_FILTER: u32 = 0x00000080;

/// Has inode checksum.
pub const EROFS_FEATURE_COMPAT_SB_CHKSUM: u32 = 0x00000001;
/// Mtime supported.
pub const EROFS_FEATURE_COMPAT_MTIME: u32 = 0x00000002;

// ---------------------------------------------------------------------------
// Inode formats (EROFS_INODE_LAYOUT_*)
// ---------------------------------------------------------------------------

/// Compact inode (32 bytes).
pub const EROFS_INODE_LAYOUT_COMPACT: u32 = 0;
/// Extended inode (64 bytes).
pub const EROFS_INODE_LAYOUT_EXTENDED: u32 = 1;

// ---------------------------------------------------------------------------
// Inode flat layout types
// ---------------------------------------------------------------------------

/// Plain (no compression).
pub const EROFS_INODE_FLAT_PLAIN: u32 = 0;
/// Compressed (full).
pub const EROFS_INODE_FLAT_COMPRESSION_LEGACY: u32 = 1;
/// Inline data (tail packing).
pub const EROFS_INODE_FLAT_INLINE: u32 = 2;
/// Compressed (new format).
pub const EROFS_INODE_FLAT_COMPRESSION: u32 = 3;
/// Chunk-based file.
pub const EROFS_INODE_CHUNK_BASED: u32 = 4;

// ---------------------------------------------------------------------------
// Compression algorithms (EROFS_COMPRESS_*)
// ---------------------------------------------------------------------------

/// LZ4 compression.
pub const Z_EROFS_COMPRESSION_LZ4: u32 = 0;
/// LZMA compression.
pub const Z_EROFS_COMPRESSION_LZMA: u32 = 1;
/// DEFLATE compression.
pub const Z_EROFS_COMPRESSION_DEFLATE: u32 = 2;
/// Zstd compression.
pub const Z_EROFS_COMPRESSION_ZSTD: u32 = 3;
/// Max compression algorithms.
pub const Z_EROFS_COMPRESSION_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Xattr prefixes
// ---------------------------------------------------------------------------

/// User xattr prefix.
pub const EROFS_XATTR_INDEX_USER: u32 = 1;
/// POSIX ACL access.
pub const EROFS_XATTR_INDEX_POSIX_ACL_ACCESS: u32 = 2;
/// POSIX ACL default.
pub const EROFS_XATTR_INDEX_POSIX_ACL_DEFAULT: u32 = 3;
/// Trusted xattr.
pub const EROFS_XATTR_INDEX_TRUSTED: u32 = 4;
/// Security xattr.
pub const EROFS_XATTR_INDEX_SECURITY: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(EROFS_SUPER_MAGIC_V1, 0xE0F5E1E2);
    }

    #[test]
    fn test_super_offset() {
        assert_eq!(EROFS_SUPER_OFFSET, 1024);
    }

    #[test]
    fn test_incompat_features_distinct() {
        // Note: COMPR_CFGS and BIG_PCLUSTER share value 0x2 in kernel
        let features = [
            EROFS_FEATURE_INCOMPAT_ZERO_PADDING,
            EROFS_FEATURE_INCOMPAT_CHUNKED_FILE,
            EROFS_FEATURE_INCOMPAT_DEVICE_TABLE,
            EROFS_FEATURE_INCOMPAT_ZTAILPACKING,
            EROFS_FEATURE_INCOMPAT_FRAGMENTS,
            EROFS_FEATURE_INCOMPAT_DEDUPE,
            EROFS_FEATURE_INCOMPAT_XATTR_FILTER,
        ];
        for i in 0..features.len() {
            for j in (i + 1)..features.len() {
                assert_ne!(features[i], features[j]);
            }
        }
    }

    #[test]
    fn test_inode_layouts() {
        assert_eq!(EROFS_INODE_LAYOUT_COMPACT, 0);
        assert_eq!(EROFS_INODE_LAYOUT_EXTENDED, 1);
    }

    #[test]
    fn test_flat_types_distinct() {
        let types = [
            EROFS_INODE_FLAT_PLAIN,
            EROFS_INODE_FLAT_COMPRESSION_LEGACY,
            EROFS_INODE_FLAT_INLINE,
            EROFS_INODE_FLAT_COMPRESSION,
            EROFS_INODE_CHUNK_BASED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_compression_algorithms_sequential() {
        assert_eq!(Z_EROFS_COMPRESSION_LZ4, 0);
        assert_eq!(Z_EROFS_COMPRESSION_LZMA, 1);
        assert_eq!(Z_EROFS_COMPRESSION_DEFLATE, 2);
        assert_eq!(Z_EROFS_COMPRESSION_ZSTD, 3);
        assert_eq!(Z_EROFS_COMPRESSION_MAX, 4);
    }

    #[test]
    fn test_xattr_indices_distinct() {
        let indices = [
            EROFS_XATTR_INDEX_USER,
            EROFS_XATTR_INDEX_POSIX_ACL_ACCESS,
            EROFS_XATTR_INDEX_POSIX_ACL_DEFAULT,
            EROFS_XATTR_INDEX_TRUSTED,
            EROFS_XATTR_INDEX_SECURITY,
        ];
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j]);
            }
        }
    }

    #[test]
    fn test_compat_features() {
        assert_ne!(EROFS_FEATURE_COMPAT_SB_CHKSUM, EROFS_FEATURE_COMPAT_MTIME);
    }
}
