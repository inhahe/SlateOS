//! `<linux/dm-ioctl.h>` — Device Mapper ioctl and target constants.
//!
//! Device Mapper (DM) provides a generic framework for creating
//! virtual block devices that map I/O to underlying physical devices.
//! It powers LVM, dm-crypt (LUKS), dm-raid, dm-thin, dm-cache,
//! dm-verity, and many other storage features.

// ---------------------------------------------------------------------------
// DM ioctl commands
// ---------------------------------------------------------------------------

/// Get DM version.
pub const DM_VERSION_CMD: u32 = 0;
/// Remove all devices.
pub const DM_REMOVE_ALL_CMD: u32 = 1;
/// List all DM devices.
pub const DM_LIST_DEVICES_CMD: u32 = 2;
/// Create a DM device.
pub const DM_DEV_CREATE_CMD: u32 = 3;
/// Remove a DM device.
pub const DM_DEV_REMOVE_CMD: u32 = 4;
/// Rename a DM device.
pub const DM_DEV_RENAME_CMD: u32 = 5;
/// Suspend a DM device.
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
/// Send message to target.
pub const DM_TARGET_MSG_CMD: u32 = 14;
/// Set device geometry.
pub const DM_DEV_SET_GEOMETRY_CMD: u32 = 15;

// ---------------------------------------------------------------------------
// DM flags
// ---------------------------------------------------------------------------

/// Device is read-only.
pub const DM_READONLY_FLAG: u32 = 1 << 0;
/// Device is suspended.
pub const DM_SUSPEND_FLAG: u32 = 1 << 1;
/// Device exists.
pub const DM_EXISTS_FLAG: u32 = 1 << 2;
/// Persistent device.
pub const DM_PERSISTENT_DEV_FLAG: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// DM target types (strings, used in table lines)
// ---------------------------------------------------------------------------

/// Linear mapping (1:1 offset).
pub const DM_TARGET_LINEAR: &str = "linear";
/// Striped (RAID-0).
pub const DM_TARGET_STRIPED: &str = "striped";
/// Mirror (RAID-1).
pub const DM_TARGET_MIRROR: &str = "mirror";
/// Snapshot.
pub const DM_TARGET_SNAPSHOT: &str = "snapshot";
/// Snapshot origin.
pub const DM_TARGET_SNAPSHOT_ORIGIN: &str = "snapshot-origin";
/// Error (all I/O fails).
pub const DM_TARGET_ERROR: &str = "error";
/// Zero (reads return zeros, writes discard).
pub const DM_TARGET_ZERO: &str = "zero";
/// Crypt (dm-crypt encryption).
pub const DM_TARGET_CRYPT: &str = "crypt";
/// Verity (dm-verity integrity).
pub const DM_TARGET_VERITY: &str = "verity";
/// Thin provisioning.
pub const DM_TARGET_THIN: &str = "thin";
/// Thin pool.
pub const DM_TARGET_THIN_POOL: &str = "thin-pool";
/// Cache (dm-cache).
pub const DM_TARGET_CACHE: &str = "cache";
/// Integrity (dm-integrity).
pub const DM_TARGET_INTEGRITY: &str = "integrity";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
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
            DM_TARGET_MSG_CMD,
            DM_DEV_SET_GEOMETRY_CMD,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            DM_READONLY_FLAG,
            DM_SUSPEND_FLAG,
            DM_EXISTS_FLAG,
            DM_PERSISTENT_DEV_FLAG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_target_names_distinct() {
        let targets = [
            DM_TARGET_LINEAR,
            DM_TARGET_STRIPED,
            DM_TARGET_MIRROR,
            DM_TARGET_SNAPSHOT,
            DM_TARGET_SNAPSHOT_ORIGIN,
            DM_TARGET_ERROR,
            DM_TARGET_ZERO,
            DM_TARGET_CRYPT,
            DM_TARGET_VERITY,
            DM_TARGET_THIN,
            DM_TARGET_THIN_POOL,
            DM_TARGET_CACHE,
            DM_TARGET_INTEGRITY,
        ];
        for i in 0..targets.len() {
            for j in (i + 1)..targets.len() {
                assert_ne!(targets[i], targets[j]);
            }
        }
    }
}
