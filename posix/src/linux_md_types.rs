//! `<linux/raid/md_u.h>` — Linux MD (Multiple Devices) RAID constants.
//!
//! MD (also called mdraid) implements software RAID in the kernel:
//! mirroring (RAID-1), striping (RAID-0), parity (RAID-5/6), and
//! combinations thereof. It operates below the filesystem layer
//! on raw block devices.

// ---------------------------------------------------------------------------
// RAID levels
// ---------------------------------------------------------------------------

/// RAID-0 (striping, no redundancy).
pub const MD_LEVEL_RAID0: i32 = 0;
/// RAID-1 (mirroring).
pub const MD_LEVEL_RAID1: i32 = 1;
/// RAID-4 (dedicated parity disk).
pub const MD_LEVEL_RAID4: i32 = 4;
/// RAID-5 (distributed parity).
pub const MD_LEVEL_RAID5: i32 = 5;
/// RAID-6 (dual distributed parity).
pub const MD_LEVEL_RAID6: i32 = 6;
/// RAID-10 (mirrored stripes).
pub const MD_LEVEL_RAID10: i32 = 10;
/// Linear (concatenation).
pub const MD_LEVEL_LINEAR: i32 = -1;
/// Multipath (failover).
pub const MD_LEVEL_MULTIPATH: i32 = -4;

// ---------------------------------------------------------------------------
// Array states
// ---------------------------------------------------------------------------

/// Array is clean (consistent).
pub const MD_STATE_CLEAN: u32 = 0;
/// Array is active (read-write).
pub const MD_STATE_ACTIVE: u32 = 1;
/// Array has write-intent bitmap.
pub const MD_STATE_BITMAP: u32 = 2;
/// Array is degraded (missing disk).
pub const MD_STATE_DEGRADED: u32 = 3;
/// Array is reshaping.
pub const MD_STATE_RESHAPE: u32 = 4;
/// Array is recovering.
pub const MD_STATE_RECOVER: u32 = 5;
/// Array is read-only.
pub const MD_STATE_READONLY: u32 = 6;

// ---------------------------------------------------------------------------
// Disk states
// ---------------------------------------------------------------------------

/// Disk is faulty.
pub const MD_DISK_FAULTY: u32 = 1 << 0;
/// Disk is active member.
pub const MD_DISK_ACTIVE: u32 = 1 << 1;
/// Disk is in sync.
pub const MD_DISK_SYNC: u32 = 1 << 2;
/// Disk removed.
pub const MD_DISK_REMOVED: u32 = 1 << 3;
/// Disk is write-mostly (RAID-1).
pub const MD_DISK_WRITEMOSTLY: u32 = 1 << 4;
/// Disk is a journal device.
pub const MD_DISK_JOURNAL: u32 = 1 << 5;
/// Disk is a failfast device.
pub const MD_DISK_FAILFAST: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// RAID-5/6 parity algorithms
// ---------------------------------------------------------------------------

/// Left-symmetric parity layout.
pub const MD_PARITY_LEFT_SYMMETRIC: u8 = 0;
/// Right-symmetric parity layout.
pub const MD_PARITY_RIGHT_SYMMETRIC: u8 = 1;
/// Left-asymmetric parity layout.
pub const MD_PARITY_LEFT_ASYMMETRIC: u8 = 2;
/// Right-asymmetric parity layout.
pub const MD_PARITY_RIGHT_ASYMMETRIC: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raid_levels_distinct() {
        let levels = [
            MD_LEVEL_RAID0,
            MD_LEVEL_RAID1,
            MD_LEVEL_RAID4,
            MD_LEVEL_RAID5,
            MD_LEVEL_RAID6,
            MD_LEVEL_RAID10,
            MD_LEVEL_LINEAR,
            MD_LEVEL_MULTIPATH,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_disk_state_flags_no_overlap() {
        let flags = [
            MD_DISK_FAULTY,
            MD_DISK_ACTIVE,
            MD_DISK_SYNC,
            MD_DISK_REMOVED,
            MD_DISK_WRITEMOSTLY,
            MD_DISK_JOURNAL,
            MD_DISK_FAILFAST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_parity_layouts_distinct() {
        let layouts = [
            MD_PARITY_LEFT_SYMMETRIC,
            MD_PARITY_RIGHT_SYMMETRIC,
            MD_PARITY_LEFT_ASYMMETRIC,
            MD_PARITY_RIGHT_ASYMMETRIC,
        ];
        for i in 0..layouts.len() {
            for j in (i + 1)..layouts.len() {
                assert_ne!(layouts[i], layouts[j]);
            }
        }
    }

    #[test]
    fn test_array_states_distinct() {
        let states = [
            MD_STATE_CLEAN,
            MD_STATE_ACTIVE,
            MD_STATE_BITMAP,
            MD_STATE_DEGRADED,
            MD_STATE_RESHAPE,
            MD_STATE_RECOVER,
            MD_STATE_READONLY,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
