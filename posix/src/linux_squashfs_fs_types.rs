//! `<linux/squashfs_fs.h>` — SquashFS on-disk constants.
//!
//! Constants describing the on-disk layout of SquashFS — the
//! compressed read-only filesystem used for live CD/USB images,
//! initramfs, and many embedded firmware partitions. `mksquashfs`
//! and userspace `squashfs-tools` consume these.

// ---------------------------------------------------------------------------
// Superblock identity
// ---------------------------------------------------------------------------

/// "hsqs" little-endian — the on-disk magic at offset 0.
pub const SQUASHFS_MAGIC: u32 = 0x7371_7368;
/// "shsq" big-endian — legacy big-endian (PowerPC) images.
pub const SQUASHFS_MAGIC_SWAP: u32 = 0x6873_7173;

/// Current SquashFS major version.
pub const SQUASHFS_MAJOR: u16 = 4;
/// Current SquashFS minor version.
pub const SQUASHFS_MINOR: u16 = 0;

// ---------------------------------------------------------------------------
// Block size limits
// ---------------------------------------------------------------------------

/// Minimum block size (4 KiB).
pub const SQUASHFS_MIN_BLOCK_SIZE: u32 = 4096;
/// Maximum block size (1 MiB).
pub const SQUASHFS_MAX_BLOCK_SIZE: u32 = 1024 * 1024;
/// Default block size used by mksquashfs (128 KiB).
pub const SQUASHFS_DEFAULT_BLOCK_SIZE: u32 = 131_072;

// ---------------------------------------------------------------------------
// Compression-type codes (super block.compression)
// ---------------------------------------------------------------------------

/// gzip (default).
pub const SQUASHFS_COMP_ZLIB: u16 = 1;
/// LZMA (early SquashFS, obsolete in upstream).
pub const SQUASHFS_COMP_LZMA: u16 = 2;
/// LZO.
pub const SQUASHFS_COMP_LZO: u16 = 3;
/// XZ.
pub const SQUASHFS_COMP_XZ: u16 = 4;
/// LZ4.
pub const SQUASHFS_COMP_LZ4: u16 = 5;
/// Zstandard.
pub const SQUASHFS_COMP_ZSTD: u16 = 6;

// ---------------------------------------------------------------------------
// Inode-type codes (squashfs_base_inode_header.inode_type)
// ---------------------------------------------------------------------------

/// Directory inode (basic).
pub const SQUASHFS_DIR_TYPE: u16 = 1;
/// Regular file inode (basic).
pub const SQUASHFS_REG_TYPE: u16 = 2;
/// Symlink inode (basic).
pub const SQUASHFS_SYMLINK_TYPE: u16 = 3;
/// Block-device inode (basic).
pub const SQUASHFS_BLKDEV_TYPE: u16 = 4;
/// Character-device inode (basic).
pub const SQUASHFS_CHRDEV_TYPE: u16 = 5;
/// FIFO inode (basic).
pub const SQUASHFS_FIFO_TYPE: u16 = 6;
/// Socket inode (basic).
pub const SQUASHFS_SOCKET_TYPE: u16 = 7;
/// Directory inode (extended).
pub const SQUASHFS_LDIR_TYPE: u16 = 8;
/// Regular file inode (extended).
pub const SQUASHFS_LREG_TYPE: u16 = 9;
/// Symlink inode (extended).
pub const SQUASHFS_LSYMLINK_TYPE: u16 = 10;
/// Block-device inode (extended).
pub const SQUASHFS_LBLKDEV_TYPE: u16 = 11;
/// Character-device inode (extended).
pub const SQUASHFS_LCHRDEV_TYPE: u16 = 12;
/// FIFO inode (extended).
pub const SQUASHFS_LFIFO_TYPE: u16 = 13;
/// Socket inode (extended).
pub const SQUASHFS_LSOCKET_TYPE: u16 = 14;

// ---------------------------------------------------------------------------
// Misc layout limits
// ---------------------------------------------------------------------------

/// Maximum size of a metadata block (also the compressed bound).
pub const SQUASHFS_METADATA_SIZE: u32 = 8192;
/// Cached metadata blocks (SquashFS in-kernel default).
pub const SQUASHFS_CACHED_BLKS: u32 = 8;
/// Maximum filename length.
pub const SQUASHFS_NAME_LEN: u32 = 256;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_is_hsqs_ascii() {
        // SQUASHFS_MAGIC is the four ASCII bytes "hsqs" in
        // little-endian order on disk.
        assert_eq!(SQUASHFS_MAGIC.to_le_bytes(), *b"hsqs");
        assert_eq!(SQUASHFS_MAGIC_SWAP.to_be_bytes(), *b"hsqs");
    }

    #[test]
    fn test_block_size_bounds() {
        assert!(SQUASHFS_MIN_BLOCK_SIZE.is_power_of_two());
        assert!(SQUASHFS_MAX_BLOCK_SIZE.is_power_of_two());
        assert!(SQUASHFS_DEFAULT_BLOCK_SIZE.is_power_of_two());
        assert!(SQUASHFS_MIN_BLOCK_SIZE <= SQUASHFS_DEFAULT_BLOCK_SIZE);
        assert!(SQUASHFS_DEFAULT_BLOCK_SIZE <= SQUASHFS_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_compression_types_distinct() {
        let comps = [
            SQUASHFS_COMP_ZLIB,
            SQUASHFS_COMP_LZMA,
            SQUASHFS_COMP_LZO,
            SQUASHFS_COMP_XZ,
            SQUASHFS_COMP_LZ4,
            SQUASHFS_COMP_ZSTD,
        ];
        for i in 0..comps.len() {
            for j in (i + 1)..comps.len() {
                assert_ne!(comps[i], comps[j]);
            }
        }
    }

    #[test]
    fn test_inode_types_distinct_and_paired() {
        let basics = [
            SQUASHFS_DIR_TYPE,
            SQUASHFS_REG_TYPE,
            SQUASHFS_SYMLINK_TYPE,
            SQUASHFS_BLKDEV_TYPE,
            SQUASHFS_CHRDEV_TYPE,
            SQUASHFS_FIFO_TYPE,
            SQUASHFS_SOCKET_TYPE,
        ];
        let extended = [
            SQUASHFS_LDIR_TYPE,
            SQUASHFS_LREG_TYPE,
            SQUASHFS_LSYMLINK_TYPE,
            SQUASHFS_LBLKDEV_TYPE,
            SQUASHFS_LCHRDEV_TYPE,
            SQUASHFS_LFIFO_TYPE,
            SQUASHFS_LSOCKET_TYPE,
        ];
        // Extended = basic + 7 (the layout convention SquashFS uses).
        for (b, e) in basics.iter().zip(extended.iter()) {
            assert_eq!(*e, *b + 7);
        }
        // All 14 codes distinct.
        let all: [u16; 14] = [
            basics[0], basics[1], basics[2], basics[3], basics[4], basics[5],
            basics[6],
            extended[0], extended[1], extended[2], extended[3], extended[4],
            extended[5], extended[6],
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
    }

    #[test]
    fn test_layout_limits_sane() {
        assert!(SQUASHFS_METADATA_SIZE.is_power_of_two());
        assert!(SQUASHFS_CACHED_BLKS >= 1);
        assert!(SQUASHFS_NAME_LEN >= 64);
    }
}
