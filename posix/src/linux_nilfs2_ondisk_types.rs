//! `<linux/nilfs2_ondisk.h>` — NILFS2 on-disk format constants.
//!
//! NILFS2 is a log-structured copy-on-write filesystem with full
//! checkpointing. These constants describe the on-disk superblock,
//! inode, and segment headers consumed by `mkfs.nilfs2` and the
//! offline `lscp` / `chcp` / `mkcp` tools.

// ---------------------------------------------------------------------------
// Superblock identity
// ---------------------------------------------------------------------------

/// NILFS2 superblock magic.
pub const NILFS_SUPER_MAGIC: u16 = 0x3434;
/// On-disk revision (major).
pub const NILFS_CURRENT_REV: u32 = 2;
/// On-disk minor revision.
pub const NILFS_MINOR_REV: u32 = 0;
/// Superblock offset within block 0 (bytes).
pub const NILFS_SB_OFFSET_BYTES: u32 = 1024;

// ---------------------------------------------------------------------------
// Block-size limits
// ---------------------------------------------------------------------------

/// Minimum block size in bytes (1 KiB).
pub const NILFS_MIN_BLOCK_SIZE: u32 = 1024;
/// Maximum block size in bytes (64 KiB).
pub const NILFS_MAX_BLOCK_SIZE: u32 = 65536;
/// log2 of the smallest block size.
pub const NILFS_MIN_BLOCK_SIZE_BITS: u32 = 10;

// ---------------------------------------------------------------------------
// Reserved inode numbers
// ---------------------------------------------------------------------------

/// Root directory inode number.
pub const NILFS_ROOT_INO: u32 = 2;
/// Data-file address translation (DAT) inode.
pub const NILFS_DAT_INO: u32 = 3;
/// Checkpoint file inode.
pub const NILFS_CPFILE_INO: u32 = 4;
/// Segment-usage file inode.
pub const NILFS_SUFILE_INO: u32 = 5;
/// Inode-file inode.
pub const NILFS_IFILE_INO: u32 = 6;
/// AT-file inode (snapshot list).
pub const NILFS_ATIME_INO: u32 = 7;
/// First inode number available to userspace.
pub const NILFS_USER_INO: u32 = 11;

// ---------------------------------------------------------------------------
// File-system state flags (super_block.s_state)
// ---------------------------------------------------------------------------

/// Filesystem is clean.
pub const NILFS_VALID_FS: u16 = 0x0001;
/// Filesystem had an error.
pub const NILFS_ERROR_FS: u16 = 0x0002;
/// fsck.nilfs2 requested.
pub const NILFS_RESIZE_FS: u16 = 0x0004;

// ---------------------------------------------------------------------------
// Segment / log boundaries
// ---------------------------------------------------------------------------

/// Magic word at the start of every segment summary.
pub const NILFS_SEGSUM_MAGIC: u32 = 0x1eaffa11;
/// Minimum segment size (bytes).
pub const NILFS_MIN_SEG_SIZE: u32 = 1 << 16;
/// Maximum segment size (bytes).
pub const NILFS_MAX_SEG_SIZE: u32 = 1 << 30;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_and_offset() {
        assert_eq!(NILFS_SUPER_MAGIC, 0x3434);
        assert_eq!(NILFS_SB_OFFSET_BYTES, 1024);
        assert!(NILFS_CURRENT_REV >= 1);
    }

    #[test]
    fn test_block_size_log_consistent() {
        // Block size bits must reproduce the minimum block size.
        assert_eq!(1u32 << NILFS_MIN_BLOCK_SIZE_BITS, NILFS_MIN_BLOCK_SIZE);
        assert!(NILFS_MIN_BLOCK_SIZE.is_power_of_two());
        assert!(NILFS_MAX_BLOCK_SIZE.is_power_of_two());
        assert!(NILFS_MIN_BLOCK_SIZE < NILFS_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_reserved_inos_distinct_and_below_user() {
        let inos = [
            NILFS_ROOT_INO,
            NILFS_DAT_INO,
            NILFS_CPFILE_INO,
            NILFS_SUFILE_INO,
            NILFS_IFILE_INO,
            NILFS_ATIME_INO,
        ];
        for i in 0..inos.len() {
            for j in (i + 1)..inos.len() {
                assert_ne!(inos[i], inos[j]);
            }
            // Every reserved inode must be below the user inode floor.
            assert!(inos[i] < NILFS_USER_INO);
        }
    }

    #[test]
    fn test_state_flags_distinct_bits() {
        let flags = [NILFS_VALID_FS, NILFS_ERROR_FS, NILFS_RESIZE_FS];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_segment_size_bounds_powers_of_two() {
        assert!(NILFS_MIN_SEG_SIZE.is_power_of_two());
        assert!(NILFS_MAX_SEG_SIZE.is_power_of_two());
        assert!(NILFS_MIN_SEG_SIZE < NILFS_MAX_SEG_SIZE);
        // Segment summary magic must be non-zero so an unwritten
        // (all-zero) segment is never mistaken for a valid one.
        assert_ne!(NILFS_SEGSUM_MAGIC, 0);
    }
}
