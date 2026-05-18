//! `<linux/dm-ioctl.h>` — Device-mapper ioctl command constants.
//!
//! Device-mapper (DM) is the kernel component that maps logical
//! block devices onto physical devices. It underlies LVM, dm-crypt,
//! dm-raid, and other storage abstractions. These constants define
//! the ioctl interface for managing DM devices.

// ---------------------------------------------------------------------------
// DM ioctl version
// ---------------------------------------------------------------------------

/// Device-mapper ioctl major version.
pub const DM_VERSION_MAJOR: u32 = 4;
/// Device-mapper ioctl minor version.
pub const DM_VERSION_MINOR: u32 = 48;
/// Device-mapper ioctl patch version.
pub const DM_VERSION_PATCHLEVEL: u32 = 0;

// ---------------------------------------------------------------------------
// DM ioctl commands
// ---------------------------------------------------------------------------

/// Get device-mapper version.
pub const DM_VERSION: u32 = 0xC138_FD00;
/// Remove all DM devices.
pub const DM_REMOVE_ALL: u32 = 0xC138_FD01;
/// List all DM devices.
pub const DM_LIST_DEVICES: u32 = 0xC138_FD02;
/// Create a new DM device.
pub const DM_DEV_CREATE: u32 = 0xC138_FD03;
/// Remove a DM device.
pub const DM_DEV_REMOVE: u32 = 0xC138_FD04;
/// Rename a DM device.
pub const DM_DEV_RENAME: u32 = 0xC138_FD05;
/// Suspend a DM device.
pub const DM_DEV_SUSPEND: u32 = 0xC138_FD06;
/// Get device status.
pub const DM_DEV_STATUS: u32 = 0xC138_FD07;
/// Wait for an event on a DM device.
pub const DM_DEV_WAIT: u32 = 0xC138_FD08;
/// Load a new table into a DM device.
pub const DM_TABLE_LOAD: u32 = 0xC138_FD09;
/// Clear the inactive table.
pub const DM_TABLE_CLEAR: u32 = 0xC138_FD0A;
/// Get table dependencies.
pub const DM_TABLE_DEPS: u32 = 0xC138_FD0B;
/// Get table status.
pub const DM_TABLE_STATUS: u32 = 0xC138_FD0C;
/// List target types.
pub const DM_LIST_VERSIONS: u32 = 0xC138_FD0D;
/// Send a target-specific message.
pub const DM_TARGET_MSG: u32 = 0xC138_FD0E;
/// Set geometry.
pub const DM_DEV_SET_GEOMETRY: u32 = 0xC138_FD0F;
/// Arm a DM device for polling.
pub const DM_DEV_ARM_POLL: u32 = 0xC138_FD10;
/// Get target version.
pub const DM_GET_TARGET_VERSION: u32 = 0xC138_FD11;

// ---------------------------------------------------------------------------
// DM ioctl flags
// ---------------------------------------------------------------------------

/// Device is read-only.
pub const DM_READONLY_FLAG: u32 = 1 << 0;
/// Device is suspended.
pub const DM_SUSPEND_FLAG: u32 = 1 << 1;
/// Use UUID to identify device (not name).
pub const DM_PERSISTENT_DEV_FLAG: u32 = 1 << 3;
/// Skip lockfs on suspend.
pub const DM_SKIP_LOCKFS_FLAG: u32 = 1 << 5;
/// Skip flush on suspend.
pub const DM_NOFLUSH_FLAG: u32 = 1 << 6;
/// Query inactive table.
pub const DM_QUERY_INACTIVE_TABLE_FLAG: u32 = 1 << 4;
/// Deferred remove.
pub const DM_DEFERRED_REMOVE: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(DM_VERSION_MAJOR, 4);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            DM_VERSION, DM_REMOVE_ALL, DM_LIST_DEVICES,
            DM_DEV_CREATE, DM_DEV_REMOVE, DM_DEV_RENAME,
            DM_DEV_SUSPEND, DM_DEV_STATUS, DM_DEV_WAIT,
            DM_TABLE_LOAD, DM_TABLE_CLEAR, DM_TABLE_DEPS,
            DM_TABLE_STATUS, DM_LIST_VERSIONS, DM_TARGET_MSG,
            DM_DEV_SET_GEOMETRY, DM_DEV_ARM_POLL,
            DM_GET_TARGET_VERSION,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap_subset() {
        // Check a subset of single-bit flags
        let flags = [
            DM_READONLY_FLAG, DM_SUSPEND_FLAG,
            DM_SKIP_LOCKFS_FLAG, DM_NOFLUSH_FLAG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_readonly_flag() {
        assert_eq!(DM_READONLY_FLAG, 1);
    }
}
