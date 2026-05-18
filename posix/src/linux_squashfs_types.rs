//! `<linux/squashfs_fs.h>` — SquashFS filesystem constants.
//!
//! SquashFS is a compressed read-only filesystem for Linux.
//! These constants define magic numbers, compression types,
//! inode types, and superblock flags.

// ---------------------------------------------------------------------------
// SquashFS magic and version
// ---------------------------------------------------------------------------

/// SquashFS magic number.
pub const SQUASHFS_MAGIC: u32 = 0x73717368;
/// Major version.
pub const SQUASHFS_MAJOR: u32 = 4;
/// Minor version.
pub const SQUASHFS_MINOR: u32 = 0;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Minimum block size (4 KiB).
pub const SQUASHFS_MIN_BLOCK_SIZE: u32 = 4096;
/// Maximum block size (1 MiB).
pub const SQUASHFS_MAX_BLOCK_SIZE: u32 = 1048576;
/// Default block size (128 KiB).
pub const SQUASHFS_DEFAULT_BLOCK_SIZE: u32 = 131072;
/// Metadata block size.
pub const SQUASHFS_METADATA_SIZE: u32 = 8192;

// ---------------------------------------------------------------------------
// Compression types
// ---------------------------------------------------------------------------

/// Gzip compression.
pub const SQUASHFS_COMP_GZIP: u32 = 1;
/// LZMA compression.
pub const SQUASHFS_COMP_LZMA: u32 = 2;
/// LZO compression.
pub const SQUASHFS_COMP_LZO: u32 = 3;
/// XZ compression.
pub const SQUASHFS_COMP_XZ: u32 = 4;
/// LZ4 compression.
pub const SQUASHFS_COMP_LZ4: u32 = 5;
/// Zstd compression.
pub const SQUASHFS_COMP_ZSTD: u32 = 6;

// ---------------------------------------------------------------------------
// Inode types
// ---------------------------------------------------------------------------

/// Basic directory.
pub const SQUASHFS_DIR_TYPE: u32 = 1;
/// Basic file.
pub const SQUASHFS_REG_TYPE: u32 = 2;
/// Basic symlink.
pub const SQUASHFS_SYMLINK_TYPE: u32 = 3;
/// Basic block device.
pub const SQUASHFS_BLKDEV_TYPE: u32 = 4;
/// Basic char device.
pub const SQUASHFS_CHRDEV_TYPE: u32 = 5;
/// Basic FIFO.
pub const SQUASHFS_FIFO_TYPE: u32 = 6;
/// Basic socket.
pub const SQUASHFS_SOCKET_TYPE: u32 = 7;
/// Extended directory.
pub const SQUASHFS_LDIR_TYPE: u32 = 8;
/// Extended file.
pub const SQUASHFS_LREG_TYPE: u32 = 9;
/// Extended symlink.
pub const SQUASHFS_LSYMLINK_TYPE: u32 = 10;
/// Extended block device.
pub const SQUASHFS_LBLKDEV_TYPE: u32 = 11;
/// Extended char device.
pub const SQUASHFS_LCHRDEV_TYPE: u32 = 12;
/// Extended FIFO.
pub const SQUASHFS_LFIFO_TYPE: u32 = 13;
/// Extended socket.
pub const SQUASHFS_LSOCKET_TYPE: u32 = 14;

// ---------------------------------------------------------------------------
// Superblock flags
// ---------------------------------------------------------------------------

/// Uncompressed inodes.
pub const SQUASHFS_UNCOMPRESSED_INODES: u32 = 0x0001;
/// Uncompressed data.
pub const SQUASHFS_UNCOMPRESSED_DATA: u32 = 0x0002;
/// Check data (unused).
pub const SQUASHFS_CHECK: u32 = 0x0004;
/// Uncompressed fragments.
pub const SQUASHFS_UNCOMPRESSED_FRAGMENTS: u32 = 0x0008;
/// No fragments.
pub const SQUASHFS_NO_FRAGMENTS: u32 = 0x0010;
/// Always use fragments.
pub const SQUASHFS_ALWAYS_FRAGMENTS: u32 = 0x0020;
/// Duplicate checking.
pub const SQUASHFS_DUPLICATES: u32 = 0x0040;
/// Exportable filesystem.
pub const SQUASHFS_EXPORTABLE: u32 = 0x0080;
/// Uncompressed xattrs.
pub const SQUASHFS_UNCOMPRESSED_XATTRS: u32 = 0x0100;
/// No xattrs.
pub const SQUASHFS_NO_XATTRS: u32 = 0x0200;
/// Compressor options present.
pub const SQUASHFS_COMP_OPT: u32 = 0x0400;
/// Uncompressed IDs.
pub const SQUASHFS_UNCOMPRESSED_IDS: u32 = 0x0800;

// ---------------------------------------------------------------------------
// Special values
// ---------------------------------------------------------------------------

/// Invalid block marker.
pub const SQUASHFS_INVALID_FRAG: u32 = 0xFFFFFFFF;
/// Invalid xattr marker.
pub const SQUASHFS_INVALID_XATTR: u32 = 0xFFFFFFFF;
/// Compressed bit.
pub const SQUASHFS_COMPRESSED_BIT: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(SQUASHFS_MAGIC, 0x73717368);
    }

    #[test]
    fn test_version() {
        assert_eq!(SQUASHFS_MAJOR, 4);
        assert_eq!(SQUASHFS_MINOR, 0);
    }

    #[test]
    fn test_block_size_ordering() {
        assert!(SQUASHFS_MIN_BLOCK_SIZE < SQUASHFS_DEFAULT_BLOCK_SIZE);
        assert!(SQUASHFS_DEFAULT_BLOCK_SIZE < SQUASHFS_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_block_sizes_power_of_two() {
        assert!(SQUASHFS_MIN_BLOCK_SIZE.is_power_of_two());
        assert!(SQUASHFS_MAX_BLOCK_SIZE.is_power_of_two());
        assert!(SQUASHFS_METADATA_SIZE.is_power_of_two());
    }

    #[test]
    fn test_comp_types_sequential() {
        assert_eq!(SQUASHFS_COMP_GZIP, 1);
        assert_eq!(SQUASHFS_COMP_LZMA, 2);
        assert_eq!(SQUASHFS_COMP_LZO, 3);
        assert_eq!(SQUASHFS_COMP_XZ, 4);
        assert_eq!(SQUASHFS_COMP_LZ4, 5);
        assert_eq!(SQUASHFS_COMP_ZSTD, 6);
    }

    #[test]
    fn test_inode_types_sequential() {
        assert_eq!(SQUASHFS_DIR_TYPE, 1);
        assert_eq!(SQUASHFS_REG_TYPE, 2);
        assert_eq!(SQUASHFS_SYMLINK_TYPE, 3);
        assert_eq!(SQUASHFS_BLKDEV_TYPE, 4);
        assert_eq!(SQUASHFS_CHRDEV_TYPE, 5);
        assert_eq!(SQUASHFS_FIFO_TYPE, 6);
        assert_eq!(SQUASHFS_SOCKET_TYPE, 7);
    }

    #[test]
    fn test_extended_types_offset() {
        assert_eq!(SQUASHFS_LDIR_TYPE, SQUASHFS_DIR_TYPE + 7);
        assert_eq!(SQUASHFS_LREG_TYPE, SQUASHFS_REG_TYPE + 7);
        assert_eq!(SQUASHFS_LSOCKET_TYPE, SQUASHFS_SOCKET_TYPE + 7);
    }

    #[test]
    fn test_sb_flags_power_of_two() {
        let flags = [
            SQUASHFS_UNCOMPRESSED_INODES, SQUASHFS_UNCOMPRESSED_DATA,
            SQUASHFS_CHECK, SQUASHFS_UNCOMPRESSED_FRAGMENTS,
            SQUASHFS_NO_FRAGMENTS, SQUASHFS_ALWAYS_FRAGMENTS,
            SQUASHFS_DUPLICATES, SQUASHFS_EXPORTABLE,
            SQUASHFS_UNCOMPRESSED_XATTRS, SQUASHFS_NO_XATTRS,
            SQUASHFS_COMP_OPT, SQUASHFS_UNCOMPRESSED_IDS,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_invalid_markers() {
        assert_eq!(SQUASHFS_INVALID_FRAG, 0xFFFFFFFF);
        assert_eq!(SQUASHFS_INVALID_XATTR, 0xFFFFFFFF);
    }

    #[test]
    fn test_compressed_bit() {
        assert!(SQUASHFS_COMPRESSED_BIT.is_power_of_two());
    }
}
