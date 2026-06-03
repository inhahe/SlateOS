//! `<linux/dm-ioctl.h>` — device-mapper `/dev/mapper/control` ioctls.
//!
//! libdevmapper, LVM2, cryptsetup, and dm-multipath open
//! `/dev/mapper/control` and issue the ioctls below to create,
//! suspend, message, and tear down dm targets (crypt, linear,
//! mirror, snapshot, thin, integrity, verity).

// ---------------------------------------------------------------------------
// Interface version (from struct dm_ioctl.version)
// ---------------------------------------------------------------------------

/// Current device-mapper interface major version.
pub const DM_VERSION_MAJOR: u32 = 4;
/// Current minor version (matches 5.10+ kernels).
pub const DM_VERSION_MINOR: u32 = 47;
/// Current patch level.
pub const DM_VERSION_PATCHLEVEL: u32 = 0;
/// Extension string baked into struct dm_ioctl.
pub const DM_VERSION_EXTRA: &[u8] = b"-ioctl (2022-07-28)";

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for dm ioctls.
pub const DM_IOCTL: u8 = 0xfd;

// ---------------------------------------------------------------------------
// ioctl command numbers (low byte of the cmd word)
// ---------------------------------------------------------------------------

/// `DM_VERSION_CMD` — query the kernel-side dm version.
pub const DM_VERSION_CMD: u32 = 0;
/// `DM_REMOVE_ALL_CMD` — destroy every dm device.
pub const DM_REMOVE_ALL_CMD: u32 = 1;
/// `DM_LIST_DEVICES_CMD` — list dm devices.
pub const DM_LIST_DEVICES_CMD: u32 = 2;
/// `DM_DEV_CREATE_CMD` — create a dm device.
pub const DM_DEV_CREATE_CMD: u32 = 3;
/// `DM_DEV_REMOVE_CMD` — destroy one dm device.
pub const DM_DEV_REMOVE_CMD: u32 = 4;
/// `DM_DEV_RENAME_CMD` — rename a dm device.
pub const DM_DEV_RENAME_CMD: u32 = 5;
/// `DM_DEV_SUSPEND_CMD` — suspend or resume a dm device.
pub const DM_DEV_SUSPEND_CMD: u32 = 6;
/// `DM_DEV_STATUS_CMD` — query device status.
pub const DM_DEV_STATUS_CMD: u32 = 7;
/// `DM_DEV_WAIT_CMD` — wait for state change.
pub const DM_DEV_WAIT_CMD: u32 = 8;
/// `DM_TABLE_LOAD_CMD` — load a (suspended) table.
pub const DM_TABLE_LOAD_CMD: u32 = 9;
/// `DM_TABLE_CLEAR_CMD` — clear any pending table.
pub const DM_TABLE_CLEAR_CMD: u32 = 10;
/// `DM_TABLE_DEPS_CMD` — list block-device dependencies.
pub const DM_TABLE_DEPS_CMD: u32 = 11;
/// `DM_TABLE_STATUS_CMD` — query table status / contents.
pub const DM_TABLE_STATUS_CMD: u32 = 12;
/// `DM_LIST_VERSIONS_CMD` — list registered target types.
pub const DM_LIST_VERSIONS_CMD: u32 = 13;
/// `DM_TARGET_MSG_CMD` — send a target-specific message.
pub const DM_TARGET_MSG_CMD: u32 = 14;
/// `DM_DEV_SET_GEOMETRY_CMD` — set CHS geometry on a dm device.
pub const DM_DEV_SET_GEOMETRY_CMD: u32 = 15;
/// `DM_DEV_ARM_POLL_CMD` — arm dm event notify polling.
pub const DM_DEV_ARM_POLL_CMD: u32 = 16;
/// `DM_GET_TARGET_VERSION_CMD` — query one target's version.
pub const DM_GET_TARGET_VERSION_CMD: u32 = 17;

// ---------------------------------------------------------------------------
// dm_ioctl.flags
// ---------------------------------------------------------------------------

/// Device is read-only.
pub const DM_READONLY_FLAG: u32 = 1 << 0;
/// Suspend was issued.
pub const DM_SUSPEND_FLAG: u32 = 1 << 1;
/// Operation was on a persistent device.
pub const DM_PERSISTENT_DEV_FLAG: u32 = 1 << 3;
/// Status reply requested.
pub const DM_STATUS_TABLE_FLAG: u32 = 1 << 4;
/// Active table is reported (vs inactive).
pub const DM_ACTIVE_PRESENT_FLAG: u32 = 1 << 5;
/// Inactive table is loaded.
pub const DM_INACTIVE_PRESENT_FLAG: u32 = 1 << 6;
/// Buffer was too small; userspace must retry.
pub const DM_BUFFER_FULL_FLAG: u32 = 1 << 8;
/// Skip BDI flag updates.
pub const DM_SKIP_BDGET_FLAG: u32 = 1 << 9;
/// Skip lock filesystem during suspend.
pub const DM_SKIP_LOCKFS_FLAG: u32 = 1 << 10;
/// Suspend without flush.
pub const DM_NOFLUSH_FLAG: u32 = 1 << 11;
/// Query info for inactive table.
pub const DM_QUERY_INACTIVE_TABLE_FLAG: u32 = 1 << 12;
/// Generate uevents on device events.
pub const DM_UEVENT_GENERATED_FLAG: u32 = 1 << 13;
/// Suspend before umount.
pub const DM_UUID_FLAG: u32 = 1 << 14;
/// Indicate secure data (zero on free).
pub const DM_SECURE_DATA_FLAG: u32 = 1 << 15;
/// Caller passed pre-allocated data buffer.
pub const DM_DATA_OUT_FLAG: u32 = 1 << 16;
/// Caller wants polling for events.
pub const DM_DEFERRED_REMOVE: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum length of the device name in struct dm_ioctl.
pub const DM_NAME_LEN: u32 = 128;
/// Maximum length of the device UUID.
pub const DM_UUID_LEN: u32 = 129;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_is_4() {
        // dm ABI major has been pinned at 4 since 2.6.
        assert_eq!(DM_VERSION_MAJOR, 4);
        // EXTRA string is non-empty (would otherwise fail the strncmp
        // in libdevmapper).
        assert!(!DM_VERSION_EXTRA.is_empty());
    }

    #[test]
    fn test_ioctl_magic_letter() {
        assert_eq!(DM_IOCTL, 0xfd);
    }

    #[test]
    fn test_cmd_numbers_dense_and_in_byte() {
        let c = [
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
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
            // Command numbers live in the low byte of the ioctl word.
            assert!(v < 256);
        }
    }

    #[test]
    fn test_flag_bits_distinct_pow2() {
        let f = [
            DM_READONLY_FLAG,
            DM_SUSPEND_FLAG,
            DM_PERSISTENT_DEV_FLAG,
            DM_STATUS_TABLE_FLAG,
            DM_ACTIVE_PRESENT_FLAG,
            DM_INACTIVE_PRESENT_FLAG,
            DM_BUFFER_FULL_FLAG,
            DM_SKIP_BDGET_FLAG,
            DM_SKIP_LOCKFS_FLAG,
            DM_NOFLUSH_FLAG,
            DM_QUERY_INACTIVE_TABLE_FLAG,
            DM_UEVENT_GENERATED_FLAG,
            DM_UUID_FLAG,
            DM_SECURE_DATA_FLAG,
            DM_DATA_OUT_FLAG,
            DM_DEFERRED_REMOVE,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_name_and_uuid_lengths() {
        // UUID is exactly one byte longer than name (so a trailing
        // NUL fits with the same alignment).
        assert_eq!(DM_NAME_LEN, 128);
        assert_eq!(DM_UUID_LEN, DM_NAME_LEN + 1);
    }
}
