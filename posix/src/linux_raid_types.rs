//! `<linux/raid/md_u.h>` — Linux software RAID (md) constants.
//!
//! The md (multiple devices) subsystem provides software RAID
//! functionality. These constants define RAID levels, array
//! states, disk states, and ioctl commands.

// ---------------------------------------------------------------------------
// RAID levels
// ---------------------------------------------------------------------------

/// RAID-0 (striping, no redundancy).
pub const RAID_LEVEL_0: i32 = 0;
/// RAID-1 (mirroring).
pub const RAID_LEVEL_1: i32 = 1;
/// RAID-4 (striping with dedicated parity).
pub const RAID_LEVEL_4: i32 = 4;
/// RAID-5 (striping with distributed parity).
pub const RAID_LEVEL_5: i32 = 5;
/// RAID-6 (striping with double parity).
pub const RAID_LEVEL_6: i32 = 6;
/// RAID-10 (mirrored stripes).
pub const RAID_LEVEL_10: i32 = 10;
/// Linear (concatenation, no RAID).
pub const RAID_LEVEL_LINEAR: i32 = -1;

// ---------------------------------------------------------------------------
// Array states
// ---------------------------------------------------------------------------

/// Array is active and running.
pub const MD_STATE_ACTIVE: u32 = 0;
/// Array is clean (no dirty stripes).
pub const MD_STATE_CLEAN: u32 = 1;
/// Array is read-only.
pub const MD_STATE_READONLY: u32 = 2;
/// Array is inactive (stopped).
pub const MD_STATE_INACTIVE: u32 = 3;
/// Array is suspended.
pub const MD_STATE_SUSPENDED: u32 = 4;

// ---------------------------------------------------------------------------
// Disk states
// ---------------------------------------------------------------------------

/// Disk is active in the array.
pub const MD_DISK_ACTIVE: u32 = 1 << 0;
/// Disk is a sync target (rebuilding).
pub const MD_DISK_SYNC: u32 = 1 << 1;
/// Disk has been removed.
pub const MD_DISK_REMOVED: u32 = 1 << 2;
/// Disk is a cluster participant.
pub const MD_DISK_CLUSTER_ADD: u32 = 1 << 3;
/// Disk is a candidate for adding.
pub const MD_DISK_CANDIDATE: u32 = 1 << 4;
/// Disk is faulty.
pub const MD_DISK_FAULTY: u32 = 1 << 5;
/// Disk is a write-mostly device (RAID-1).
pub const MD_DISK_WRITEMOSTLY: u32 = 1 << 6;
/// Disk is a journal device.
pub const MD_DISK_JOURNAL: u32 = 1 << 7;
/// Disk is a failfast device.
pub const MD_DISK_FAILFAST: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// md ioctl commands
// ---------------------------------------------------------------------------

/// Get array info.
pub const GET_ARRAY_INFO: u32 = 0x0910;
/// Get disk info.
pub const GET_DISK_INFO: u32 = 0x0912;
/// Set array info.
pub const SET_ARRAY_INFO: u32 = 0x0923;
/// Set disk info.
pub const SET_DISK_INFO: u32 = 0x0924;
/// Run the array.
pub const RUN_ARRAY: u32 = 0x0930;
/// Stop the array.
pub const STOP_ARRAY: u32 = 0x0932;
/// Add a new disk.
pub const ADD_NEW_DISK: u32 = 0x0921;
/// Hot-add a disk.
pub const HOT_ADD_DISK: u32 = 0x0940;
/// Hot-remove a disk.
pub const HOT_REMOVE_DISK: u32 = 0x0941;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_distinct() {
        let levels = [
            RAID_LEVEL_LINEAR,
            RAID_LEVEL_0,
            RAID_LEVEL_1,
            RAID_LEVEL_4,
            RAID_LEVEL_5,
            RAID_LEVEL_6,
            RAID_LEVEL_10,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_linear_is_negative() {
        assert!(RAID_LEVEL_LINEAR < 0);
    }

    #[test]
    fn test_array_states_distinct() {
        let states = [
            MD_STATE_ACTIVE,
            MD_STATE_CLEAN,
            MD_STATE_READONLY,
            MD_STATE_INACTIVE,
            MD_STATE_SUSPENDED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_disk_flags_no_overlap() {
        let flags = [
            MD_DISK_ACTIVE,
            MD_DISK_SYNC,
            MD_DISK_REMOVED,
            MD_DISK_CLUSTER_ADD,
            MD_DISK_CANDIDATE,
            MD_DISK_FAULTY,
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
    fn test_disk_flags_power_of_two() {
        let flags = [
            MD_DISK_ACTIVE,
            MD_DISK_SYNC,
            MD_DISK_REMOVED,
            MD_DISK_CLUSTER_ADD,
            MD_DISK_CANDIDATE,
            MD_DISK_FAULTY,
            MD_DISK_WRITEMOSTLY,
            MD_DISK_JOURNAL,
            MD_DISK_FAILFAST,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            GET_ARRAY_INFO,
            GET_DISK_INFO,
            SET_ARRAY_INFO,
            SET_DISK_INFO,
            RUN_ARRAY,
            STOP_ARRAY,
            ADD_NEW_DISK,
            HOT_ADD_DISK,
            HOT_REMOVE_DISK,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
