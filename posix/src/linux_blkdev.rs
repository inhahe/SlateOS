//! `<linux/blkdev.h>` — Block device constants.
//!
//! The block layer manages block devices (disks, SSDs, ramdisks)
//! and provides the I/O scheduler, request queue, and bio-based
//! I/O interface. This module defines queue limits, sector sizes,
//! and block device flags.

// ---------------------------------------------------------------------------
// Sector sizes
// ---------------------------------------------------------------------------

/// Standard sector size (bytes).
pub const SECTOR_SIZE: u32 = 512;
/// Sector shift (log2 of SECTOR_SIZE).
pub const SECTOR_SHIFT: u32 = 9;

// ---------------------------------------------------------------------------
// Block size limits
// ---------------------------------------------------------------------------

/// Minimum block size.
pub const BLK_MIN_BLOCK_SIZE: u32 = 512;
/// Maximum block size.
pub const BLK_MAX_BLOCK_SIZE: u32 = 65536;
/// Default block size.
pub const BLK_DEF_BLOCK_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Queue limits
// ---------------------------------------------------------------------------

/// Default maximum sectors per request.
pub const BLK_DEF_MAX_SECTORS: u32 = 2560;
/// Safe maximum sectors.
pub const BLK_SAFE_MAX_SECTORS: u32 = 255;
/// Maximum segments per request.
pub const BLK_MAX_SEGMENTS: u16 = 128;
/// Maximum segment size (bytes).
pub const BLK_MAX_SEGMENT_SIZE: u32 = 65536;

// ---------------------------------------------------------------------------
// Block device flags (GENHD_FL_*)
// ---------------------------------------------------------------------------

/// Removable media.
pub const GENHD_FL_REMOVABLE: u32 = 1 << 0;
/// Hidden device (no /dev entry).
pub const GENHD_FL_HIDDEN: u32 = 1 << 1;
/// No partition scanning.
pub const GENHD_FL_NO_PART: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Disk event flags
// ---------------------------------------------------------------------------

/// Media change event.
pub const DISK_EVENT_MEDIA_CHANGE: u32 = 1 << 0;
/// Eject request event.
pub const DISK_EVENT_EJECT_REQUEST: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// BLK feature flags
// ---------------------------------------------------------------------------

/// Supports rotational media.
pub const BLK_FEAT_ROTATIONAL: u32 = 1 << 0;
/// Supports write-same.
pub const BLK_FEAT_WRITE_SAME: u32 = 1 << 1;
/// Supports discard (TRIM).
pub const BLK_FEAT_DISCARD: u32 = 1 << 2;
/// Supports secure discard.
pub const BLK_FEAT_SECURE_DISCARD: u32 = 1 << 3;
/// Supports write zeroes.
pub const BLK_FEAT_WRITE_ZEROES: u32 = 1 << 4;
/// Supports DAX (direct access).
pub const BLK_FEAT_DAX: u32 = 1 << 5;
/// Supports zoned storage.
pub const BLK_FEAT_ZONED: u32 = 1 << 6;
/// Supports IO polling.
pub const BLK_FEAT_POLL: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sector_size() {
        assert_eq!(SECTOR_SIZE, 512);
        assert_eq!(1u32 << SECTOR_SHIFT, SECTOR_SIZE);
    }

    #[test]
    fn test_block_size_limits() {
        assert!(BLK_MIN_BLOCK_SIZE <= BLK_DEF_BLOCK_SIZE);
        assert!(BLK_DEF_BLOCK_SIZE <= BLK_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_queue_limits() {
        assert!(BLK_SAFE_MAX_SECTORS < BLK_DEF_MAX_SECTORS);
    }

    #[test]
    fn test_genhd_flags_powers_of_two() {
        let flags = [GENHD_FL_REMOVABLE, GENHD_FL_HIDDEN, GENHD_FL_NO_PART];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_genhd_flags_no_overlap() {
        let flags = [GENHD_FL_REMOVABLE, GENHD_FL_HIDDEN, GENHD_FL_NO_PART];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_disk_events_no_overlap() {
        assert_eq!(DISK_EVENT_MEDIA_CHANGE & DISK_EVENT_EJECT_REQUEST, 0);
    }

    #[test]
    fn test_feat_flags_powers_of_two() {
        let flags = [
            BLK_FEAT_ROTATIONAL,
            BLK_FEAT_WRITE_SAME,
            BLK_FEAT_DISCARD,
            BLK_FEAT_SECURE_DISCARD,
            BLK_FEAT_WRITE_ZEROES,
            BLK_FEAT_DAX,
            BLK_FEAT_ZONED,
            BLK_FEAT_POLL,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_feat_flags_no_overlap() {
        let flags = [
            BLK_FEAT_ROTATIONAL,
            BLK_FEAT_WRITE_SAME,
            BLK_FEAT_DISCARD,
            BLK_FEAT_SECURE_DISCARD,
            BLK_FEAT_WRITE_ZEROES,
            BLK_FEAT_DAX,
            BLK_FEAT_ZONED,
            BLK_FEAT_POLL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
