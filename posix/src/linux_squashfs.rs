//! `<linux/squashfs_fs.h>` — SquashFS read-only filesystem constants.
//!
//! SquashFS is a compressed read-only filesystem widely used for
//! Linux live CDs, embedded systems, container images (snap, AppImage),
//! and initramfs. It supports multiple compression algorithms and
//! achieves high compression ratios with fast random access.

// ---------------------------------------------------------------------------
// SquashFS magic and version
// ---------------------------------------------------------------------------

/// SquashFS magic number ("hsqs" little-endian).
pub const SQUASHFS_MAGIC: u32 = 0x7371_7368;
/// Major version.
pub const SQUASHFS_MAJOR: u16 = 4;
/// Minor version.
pub const SQUASHFS_MINOR: u16 = 0;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// Gzip (zlib).
pub const SQUASHFS_COMP_GZIP: u16 = 1;
/// LZMA.
pub const SQUASHFS_COMP_LZMA: u16 = 2;
/// LZO.
pub const SQUASHFS_COMP_LZO: u16 = 3;
/// XZ.
pub const SQUASHFS_COMP_XZ: u16 = 4;
/// LZ4.
pub const SQUASHFS_COMP_LZ4: u16 = 5;
/// Zstd.
pub const SQUASHFS_COMP_ZSTD: u16 = 6;

// ---------------------------------------------------------------------------
// Inode types
// ---------------------------------------------------------------------------

/// Basic directory.
pub const SQUASHFS_DIR_TYPE: u16 = 1;
/// Basic regular file.
pub const SQUASHFS_REG_TYPE: u16 = 2;
/// Basic symlink.
pub const SQUASHFS_SYMLINK_TYPE: u16 = 3;
/// Basic block device.
pub const SQUASHFS_BLKDEV_TYPE: u16 = 4;
/// Basic char device.
pub const SQUASHFS_CHRDEV_TYPE: u16 = 5;
/// Basic FIFO.
pub const SQUASHFS_FIFO_TYPE: u16 = 6;
/// Basic socket.
pub const SQUASHFS_SOCKET_TYPE: u16 = 7;
/// Extended directory.
pub const SQUASHFS_LDIR_TYPE: u16 = 8;
/// Extended regular file.
pub const SQUASHFS_LREG_TYPE: u16 = 9;
/// Extended symlink.
pub const SQUASHFS_LSYMLINK_TYPE: u16 = 10;
/// Extended block device.
pub const SQUASHFS_LBLKDEV_TYPE: u16 = 11;
/// Extended char device.
pub const SQUASHFS_LCHRDEV_TYPE: u16 = 12;
/// Extended FIFO.
pub const SQUASHFS_LFIFO_TYPE: u16 = 13;
/// Extended socket.
pub const SQUASHFS_LSOCKET_TYPE: u16 = 14;

// ---------------------------------------------------------------------------
// Superblock flags
// ---------------------------------------------------------------------------

/// Uncompressed inodes.
pub const SQUASHFS_UNCOMPRESSED_INODES: u16 = 1 << 0;
/// Uncompressed data.
pub const SQUASHFS_UNCOMPRESSED_DATA: u16 = 1 << 1;
/// No fragment blocks.
pub const SQUASHFS_NO_FRAGMENTS: u16 = 1 << 3;
/// Always use fragments.
pub const SQUASHFS_ALWAYS_FRAGMENTS: u16 = 1 << 4;
/// Deduplicated data.
pub const SQUASHFS_DUPLICATES: u16 = 1 << 5;
/// Filesystem is exportable (NFS).
pub const SQUASHFS_EXPORTABLE: u16 = 1 << 6;
/// Uncompressed xattrs.
pub const SQUASHFS_UNCOMPRESSED_XATTRS: u16 = 1 << 7;
/// No xattrs.
pub const SQUASHFS_NO_XATTRS: u16 = 1 << 8;
/// Compressor-specific options present.
pub const SQUASHFS_COMP_OPT: u16 = 1 << 9;

// ---------------------------------------------------------------------------
// Block size limits
// ---------------------------------------------------------------------------

/// Minimum block size (4 KiB).
pub const SQUASHFS_MIN_BLOCK_SIZE: u32 = 4096;
/// Maximum block size (1 MiB).
pub const SQUASHFS_MAX_BLOCK_SIZE: u32 = 1048576;
/// Default block size (128 KiB).
pub const SQUASHFS_DEFAULT_BLOCK_SIZE: u32 = 131072;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(SQUASHFS_MAGIC, 0x7371_7368);
    }

    #[test]
    fn test_compression_types_distinct() {
        let types = [
            SQUASHFS_COMP_GZIP,
            SQUASHFS_COMP_LZMA,
            SQUASHFS_COMP_LZO,
            SQUASHFS_COMP_XZ,
            SQUASHFS_COMP_LZ4,
            SQUASHFS_COMP_ZSTD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_inode_types_distinct() {
        let types = [
            SQUASHFS_DIR_TYPE,
            SQUASHFS_REG_TYPE,
            SQUASHFS_SYMLINK_TYPE,
            SQUASHFS_BLKDEV_TYPE,
            SQUASHFS_CHRDEV_TYPE,
            SQUASHFS_FIFO_TYPE,
            SQUASHFS_SOCKET_TYPE,
            SQUASHFS_LDIR_TYPE,
            SQUASHFS_LREG_TYPE,
            SQUASHFS_LSYMLINK_TYPE,
            SQUASHFS_LBLKDEV_TYPE,
            SQUASHFS_LCHRDEV_TYPE,
            SQUASHFS_LFIFO_TYPE,
            SQUASHFS_LSOCKET_TYPE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_superblock_flags_no_overlap() {
        let flags = [
            SQUASHFS_UNCOMPRESSED_INODES,
            SQUASHFS_UNCOMPRESSED_DATA,
            SQUASHFS_NO_FRAGMENTS,
            SQUASHFS_ALWAYS_FRAGMENTS,
            SQUASHFS_DUPLICATES,
            SQUASHFS_EXPORTABLE,
            SQUASHFS_UNCOMPRESSED_XATTRS,
            SQUASHFS_NO_XATTRS,
            SQUASHFS_COMP_OPT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_block_size_ordering() {
        assert!(SQUASHFS_MIN_BLOCK_SIZE < SQUASHFS_DEFAULT_BLOCK_SIZE);
        assert!(SQUASHFS_DEFAULT_BLOCK_SIZE < SQUASHFS_MAX_BLOCK_SIZE);
    }
}
