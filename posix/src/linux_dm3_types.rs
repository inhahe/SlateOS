//! `<linux/dm-ioctl.h>` — Additional device-mapper constants (part 3).
//!
//! Supplementary device-mapper constants covering ioctl flags,
//! target types, and version information.

// ---------------------------------------------------------------------------
// Device-mapper flags (DM_*_FLAG)
// ---------------------------------------------------------------------------

/// Read-only.
pub const DM_READONLY_FLAG: u32 = 1 << 0;
/// Suspend with no flush.
pub const DM_SUSPEND_FLAG: u32 = 1 << 1;
/// Persistent device.
pub const DM_PERSISTENT_DEV_FLAG: u32 = 1 << 3;
/// Status table.
pub const DM_STATUS_TABLE_FLAG: u32 = 1 << 4;
/// Active table present.
pub const DM_ACTIVE_PRESENT_FLAG: u32 = 1 << 5;
/// Inactive table present.
pub const DM_INACTIVE_PRESENT_FLAG: u32 = 1 << 6;
/// Buffer full flag.
pub const DM_BUFFER_FULL_FLAG: u32 = 1 << 8;
/// Skip lockfs.
pub const DM_SKIP_LOCKFS_FLAG: u32 = 1 << 9;
/// No flush.
pub const DM_NOFLUSH_FLAG: u32 = 1 << 10;
/// Query inactive table.
pub const DM_QUERY_INACTIVE_TABLE_FLAG: u32 = 1 << 11;
/// UUID to device.
pub const DM_UEVENT_GENERATED_FLAG: u32 = 1 << 13;
/// Secure erase.
pub const DM_SECURE_DATA_FLAG: u32 = 1 << 15;
/// Data out.
pub const DM_DATA_OUT_FLAG: u32 = 1 << 16;
/// Deferred remove.
pub const DM_DEFERRED_REMOVE: u32 = 1 << 17;
/// Internal suspend.
pub const DM_INTERNAL_SUSPEND_FLAG: u32 = 1 << 18;
/// IMA measurement flag.
pub const DM_IMA_MEASUREMENT_FLAG: u32 = 1 << 19;

// ---------------------------------------------------------------------------
// Device-mapper ioctl commands
// ---------------------------------------------------------------------------

/// Version.
pub const DM_VERSION_CMD: u32 = 0;
/// Remove all.
pub const DM_REMOVE_ALL_CMD: u32 = 1;
/// List devices.
pub const DM_LIST_DEVICES_CMD: u32 = 2;
/// Create device.
pub const DM_DEV_CREATE_CMD: u32 = 3;
/// Remove device.
pub const DM_DEV_REMOVE_CMD: u32 = 4;
/// Rename device.
pub const DM_DEV_RENAME_CMD: u32 = 5;
/// Suspend device.
pub const DM_DEV_SUSPEND_CMD: u32 = 6;
/// Get device status.
pub const DM_DEV_STATUS_CMD: u32 = 7;
/// Wait for event.
pub const DM_DEV_WAIT_CMD: u32 = 8;
/// Load table.
pub const DM_TABLE_LOAD_CMD: u32 = 9;
/// Clear table.
pub const DM_TABLE_CLEAR_CMD: u32 = 10;
/// Get table deps.
pub const DM_TABLE_DEPS_CMD: u32 = 11;
/// Get table status.
pub const DM_TABLE_STATUS_CMD: u32 = 12;
/// List versions.
pub const DM_LIST_VERSIONS_CMD: u32 = 13;
/// Target message.
pub const DM_TARGET_MSG_CMD: u32 = 14;
/// Set geometry.
pub const DM_DEV_SET_GEOMETRY_CMD: u32 = 15;
/// Arm poll.
pub const DM_DEV_ARM_POLL_CMD: u32 = 16;
/// Get target version.
pub const DM_GET_TARGET_VERSION_CMD: u32 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            DM_READONLY_FLAG,
            DM_SUSPEND_FLAG,
            DM_PERSISTENT_DEV_FLAG,
            DM_STATUS_TABLE_FLAG,
            DM_ACTIVE_PRESENT_FLAG,
            DM_INACTIVE_PRESENT_FLAG,
            DM_BUFFER_FULL_FLAG,
            DM_SKIP_LOCKFS_FLAG,
            DM_NOFLUSH_FLAG,
            DM_QUERY_INACTIVE_TABLE_FLAG,
            DM_UEVENT_GENERATED_FLAG,
            DM_SECURE_DATA_FLAG,
            DM_DATA_OUT_FLAG,
            DM_DEFERRED_REMOVE,
            DM_INTERNAL_SUSPEND_FLAG,
            DM_IMA_MEASUREMENT_FLAG,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            DM_READONLY_FLAG,
            DM_SUSPEND_FLAG,
            DM_PERSISTENT_DEV_FLAG,
            DM_STATUS_TABLE_FLAG,
            DM_ACTIVE_PRESENT_FLAG,
            DM_INACTIVE_PRESENT_FLAG,
            DM_BUFFER_FULL_FLAG,
            DM_SKIP_LOCKFS_FLAG,
            DM_NOFLUSH_FLAG,
            DM_QUERY_INACTIVE_TABLE_FLAG,
            DM_UEVENT_GENERATED_FLAG,
            DM_SECURE_DATA_FLAG,
            DM_DATA_OUT_FLAG,
            DM_DEFERRED_REMOVE,
            DM_INTERNAL_SUSPEND_FLAG,
            DM_IMA_MEASUREMENT_FLAG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            DM_VERSION_CMD,
            DM_REMOVE_ALL_CMD,
            DM_LIST_DEVICES_CMD,
            DM_DEV_CREATE_CMD,
            DM_DEV_REMOVE_CMD,
            DM_DEV_RENAME_CMD,
            DM_DEV_SUSPEND_CMD,
            DM_DEV_STATUS_CMD,
            DM_DEV_WAIT_CMD,
            DM_TABLE_LOAD_CMD,
            DM_TABLE_CLEAR_CMD,
            DM_TABLE_DEPS_CMD,
            DM_TABLE_STATUS_CMD,
            DM_LIST_VERSIONS_CMD,
            DM_TARGET_MSG_CMD,
            DM_DEV_SET_GEOMETRY_CMD,
            DM_DEV_ARM_POLL_CMD,
            DM_GET_TARGET_VERSION_CMD,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
