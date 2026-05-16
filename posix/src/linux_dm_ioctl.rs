//! `<linux/dm-ioctl.h>` — Device Mapper ioctl interface.
//!
//! Provides ioctl constants and the `DmIoctl` struct for interacting
//! with the kernel's device mapper (`/dev/mapper/control`).

// ---------------------------------------------------------------------------
// DM ioctl commands
// ---------------------------------------------------------------------------

/// DM ioctl magic number.
pub const DM_IOCTL: u8 = 0xFD;

/// Get DM version.
pub const DM_VERSION: u64 = 0xC138_FD00;
/// Remove all DM devices.
pub const DM_REMOVE_ALL: u64 = 0xC138_FD01;
/// List all DM devices.
pub const DM_LIST_DEVICES: u64 = 0xC138_FD02;
/// Create a DM device.
pub const DM_DEV_CREATE: u64 = 0xC138_FD03;
/// Remove a DM device.
pub const DM_DEV_REMOVE: u64 = 0xC138_FD04;
/// Rename a DM device.
pub const DM_DEV_RENAME: u64 = 0xC138_FD05;
/// Suspend/resume a DM device.
pub const DM_DEV_SUSPEND: u64 = 0xC138_FD06;
/// Get DM device status.
pub const DM_DEV_STATUS: u64 = 0xC138_FD07;
/// Wait for event on DM device.
pub const DM_DEV_WAIT: u64 = 0xC138_FD08;
/// Load a table into DM device.
pub const DM_TABLE_LOAD: u64 = 0xC138_FD09;
/// Clear a DM table.
pub const DM_TABLE_CLEAR: u64 = 0xC138_FD0A;
/// Get table dependencies.
pub const DM_TABLE_DEPS: u64 = 0xC138_FD0B;
/// Get table status.
pub const DM_TABLE_STATUS: u64 = 0xC138_FD0C;
/// List DM target versions.
pub const DM_LIST_VERSIONS: u64 = 0xC138_FD0D;
/// Send a message to a DM target.
pub const DM_TARGET_MSG: u64 = 0xC138_FD0E;
/// Set device geometry.
pub const DM_DEV_SET_GEOMETRY: u64 = 0xC138_FD0F;
/// Arm device poll.
pub const DM_DEV_ARM_POLL: u64 = 0xC138_FD10;
/// Get target version.
pub const DM_GET_TARGET_VERSION: u64 = 0xC138_FD11;

// ---------------------------------------------------------------------------
// DM flags
// ---------------------------------------------------------------------------

/// Device is read-only.
pub const DM_READONLY_FLAG: u32 = 1 << 0;
/// Device is suspended.
pub const DM_SUSPEND_FLAG: u32 = 1 << 1;
/// Persistent device (survives reboot).
pub const DM_PERSISTENT_DEV_FLAG: u32 = 1 << 3;
/// Status table (vs info).
pub const DM_STATUS_TABLE_FLAG: u32 = 1 << 4;
/// Active table present.
pub const DM_ACTIVE_PRESENT_FLAG: u32 = 1 << 5;
/// Inactive table present.
pub const DM_INACTIVE_PRESENT_FLAG: u32 = 1 << 6;
/// Buffer full.
pub const DM_BUFFER_FULL_FLAG: u32 = 1 << 8;
/// Skip lockfs.
pub const DM_SKIP_LOCKFS_FLAG: u32 = 1 << 9;
/// Skip bdget refcount.
pub const DM_NOFLUSH_FLAG: u32 = 1 << 10;
/// Query inactive table.
pub const DM_QUERY_INACTIVE_TABLE_FLAG: u32 = 1 << 11;
/// Uevent generation requested.
pub const DM_UEVENT_GENERATED_FLAG: u32 = 1 << 13;
/// UUID is set.
pub const DM_UUID_FLAG: u32 = 1 << 14;
/// Device is secure erase capable.
pub const DM_SECURE_DATA_FLAG: u32 = 1 << 15;
/// Deferred remove.
pub const DM_DEFERRED_REMOVE: u32 = 1 << 17;
/// Internal suspend.
pub const DM_INTERNAL_SUSPEND_FLAG: u32 = 1 << 18;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length for DM device name.
pub const DM_NAME_LEN: usize = 128;
/// Maximum length for DM UUID.
pub const DM_UUID_LEN: usize = 129;

// ---------------------------------------------------------------------------
// DM ioctl struct (simplified)
// ---------------------------------------------------------------------------

/// Device mapper ioctl header.
///
/// This is a simplified version of `struct dm_ioctl`. The real
/// struct is 312 bytes (0x138) with inline name/uuid arrays.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DmIoctl {
    /// Version (3 x u32).
    pub version: [u32; 3],
    /// Size of this struct + following data.
    pub data_size: u32,
    /// Offset to start of data.
    pub data_start: u32,
    /// Target-specific status.
    pub target_count: u32,
    /// Open reference count.
    pub open_count: i32,
    /// Flags (DM_*_FLAG).
    pub flags: u32,
    /// Event number.
    pub event_nr: u32,
    /// Padding.
    _padding: u32,
    /// Device number (dev_t).
    pub dev: u64,
    /// Device name.
    pub name: [u8; DM_NAME_LEN],
    /// Device UUID.
    pub uuid: [u8; DM_UUID_LEN],
    /// Padding to 0x138 (312) bytes.
    _data: [u8; 7],
}

impl DmIoctl {
    /// Create a zeroed `DmIoctl` with `data_size` set.
    pub fn zeroed() -> Self {
        let mut dm: Self = unsafe { core::mem::zeroed() };
        dm.data_size = core::mem::size_of::<Self>() as u32;
        dm.version = [4, 0, 0]; // DM version 4.0.0
        dm
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dm_ioctl_size() {
        // Should be 312 bytes (0x138).
        assert_eq!(core::mem::size_of::<DmIoctl>(), 312);
    }

    #[test]
    fn test_dm_ioctl_zeroed() {
        let dm = DmIoctl::zeroed();
        assert_eq!(dm.version[0], 4);
        assert_eq!(dm.data_size, 312);
        assert_eq!(dm.flags, 0);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            DM_VERSION, DM_REMOVE_ALL, DM_LIST_DEVICES,
            DM_DEV_CREATE, DM_DEV_REMOVE, DM_DEV_RENAME,
            DM_DEV_SUSPEND, DM_DEV_STATUS, DM_DEV_WAIT,
            DM_TABLE_LOAD, DM_TABLE_CLEAR,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_are_bits() {
        let flags = [
            DM_READONLY_FLAG, DM_SUSPEND_FLAG, DM_PERSISTENT_DEV_FLAG,
            DM_STATUS_TABLE_FLAG, DM_ACTIVE_PRESENT_FLAG,
            DM_INACTIVE_PRESENT_FLAG, DM_BUFFER_FULL_FLAG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "DM flags must not overlap");
            }
        }
    }

    #[test]
    fn test_name_len() {
        assert_eq!(DM_NAME_LEN, 128);
        assert_eq!(DM_UUID_LEN, 129);
    }
}
