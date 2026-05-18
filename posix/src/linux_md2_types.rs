//! `<linux/raid/md_u.h>` — Additional MD RAID constants.
//!
//! Supplementary MD/RAID constants covering RAID levels,
//! array states, disk states, and layout algorithms.

// ---------------------------------------------------------------------------
// RAID levels
// ---------------------------------------------------------------------------

/// Linear (JBOD).
pub const LEVEL_LINEAR: i32 = -1;
/// RAID-0 (striping).
pub const LEVEL_RAID0: i32 = 0;
/// RAID-1 (mirroring).
pub const LEVEL_RAID1: i32 = 1;
/// RAID-4 (dedicated parity).
pub const LEVEL_RAID4: i32 = 4;
/// RAID-5 (distributed parity).
pub const LEVEL_RAID5: i32 = 5;
/// RAID-6 (dual parity).
pub const LEVEL_RAID6: i32 = 6;
/// RAID-10 (striped mirrors).
pub const LEVEL_RAID10: i32 = 10;
/// Multipath.
pub const LEVEL_MULTIPATH: i32 = -4;
/// Faulty (test level).
pub const LEVEL_FAULTY: i32 = -5;

// ---------------------------------------------------------------------------
// MD array states
// ---------------------------------------------------------------------------

/// Array is clean.
pub const MD_SB_CLEAN: u32 = 0;
/// Bitmap present.
pub const MD_SB_BITMAP_PRESENT: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// MD disk states (mdp_disk flags)
// ---------------------------------------------------------------------------

/// Disk is faulty.
pub const MD_DISK_FAULTY: u32 = 0;
/// Disk is active.
pub const MD_DISK_ACTIVE: u32 = 1;
/// Disk is a sync target.
pub const MD_DISK_SYNC: u32 = 2;
/// Disk removed.
pub const MD_DISK_REMOVED: u32 = 3;
/// Disk is a write-mostly member.
pub const MD_DISK_WRITEMOSTLY: u32 = 9;
/// Disk is a journal.
pub const MD_DISK_JOURNAL: u32 = 18;
/// Disk in failfast mode.
pub const MD_DISK_FAILFAST: u32 = 10;

// ---------------------------------------------------------------------------
// MD ioctl commands
// ---------------------------------------------------------------------------

/// Get array info.
pub const GET_ARRAY_INFO: u32 = 0x0910;
/// Get disk info.
pub const GET_DISK_INFO: u32 = 0x0912;
/// Run array.
pub const RUN_ARRAY: u32 = 0x0930;
/// Stop array.
pub const STOP_ARRAY: u32 = 0x0932;
/// Stop array (read-only).
pub const STOP_ARRAY_RO: u32 = 0x0933;
/// Restart array (read-write).
pub const RESTART_ARRAY_RW: u32 = 0x0934;
/// Add new disk.
pub const ADD_NEW_DISK: u32 = 0x0921;
/// Hot remove disk.
pub const HOT_REMOVE_DISK: u32 = 0x0922;
/// Hot add disk.
pub const HOT_ADD_DISK: u32 = 0x0923;
/// Set array info.
pub const SET_ARRAY_INFO: u32 = 0x0923;
/// Set disk info.
pub const SET_DISK_INFO: u32 = 0x0924;
/// Set bitmap file.
pub const SET_BITMAP_FILE: u32 = 0x092B;

// ---------------------------------------------------------------------------
// RAID-5/6 layout algorithms (ALGORITHM_*)
// ---------------------------------------------------------------------------

/// Left-asymmetric.
pub const ALGORITHM_LEFT_ASYMMETRIC: u32 = 0;
/// Right-asymmetric.
pub const ALGORITHM_RIGHT_ASYMMETRIC: u32 = 1;
/// Left-symmetric.
pub const ALGORITHM_LEFT_SYMMETRIC: u32 = 2;
/// Right-symmetric.
pub const ALGORITHM_RIGHT_SYMMETRIC: u32 = 3;
/// Parity first.
pub const ALGORITHM_PARITY_0: u32 = 4;
/// Parity last.
pub const ALGORITHM_PARITY_N: u32 = 5;

// ---------------------------------------------------------------------------
// MD superblock magic / version
// ---------------------------------------------------------------------------

/// MD superblock magic.
pub const MD_SB_MAGIC: u32 = 0xa92b4efc;
/// Major version.
pub const MD_SB_MAJOR_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raid_levels_distinct() {
        let levels = [
            LEVEL_LINEAR, LEVEL_RAID0, LEVEL_RAID1, LEVEL_RAID4,
            LEVEL_RAID5, LEVEL_RAID6, LEVEL_RAID10,
            LEVEL_MULTIPATH, LEVEL_FAULTY,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_disk_states_distinct() {
        let states = [
            MD_DISK_FAULTY, MD_DISK_ACTIVE, MD_DISK_SYNC,
            MD_DISK_REMOVED, MD_DISK_WRITEMOSTLY,
            MD_DISK_JOURNAL, MD_DISK_FAILFAST,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_cmds_values() {
        assert_eq!(GET_ARRAY_INFO, 0x0910);
        assert_eq!(GET_DISK_INFO, 0x0912);
        assert_eq!(RUN_ARRAY, 0x0930);
        assert_eq!(STOP_ARRAY, 0x0932);
    }

    #[test]
    fn test_algorithms_distinct() {
        let algs = [
            ALGORITHM_LEFT_ASYMMETRIC, ALGORITHM_RIGHT_ASYMMETRIC,
            ALGORITHM_LEFT_SYMMETRIC, ALGORITHM_RIGHT_SYMMETRIC,
            ALGORITHM_PARITY_0, ALGORITHM_PARITY_N,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_sb_magic() {
        assert_eq!(MD_SB_MAGIC, 0xa92b4efc);
        assert_eq!(MD_SB_MAJOR_VERSION, 1);
    }

    #[test]
    fn test_negative_levels() {
        assert!(LEVEL_LINEAR < 0);
        assert!(LEVEL_MULTIPATH < 0);
        assert!(LEVEL_FAULTY < 0);
    }
}
