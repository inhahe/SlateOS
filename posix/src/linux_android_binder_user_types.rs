//! `<linux/android/binder.h>` — Android binder IPC commands and replies.
//!
//! Android's binder driver is the kernel side of the Android Inter-
//! Process Communication system. Userspace exchanges typed
//! transactions through `/dev/binder` (or `/dev/hwbinder`, `/dev/vndbinder`).

// ---------------------------------------------------------------------------
// Device paths
// ---------------------------------------------------------------------------

pub const DEV_BINDER: &str = "/dev/binder";
pub const DEV_HWBINDER: &str = "/dev/hwbinder";
pub const DEV_VNDBINDER: &str = "/dev/vndbinder";

// ---------------------------------------------------------------------------
// Driver constants
// ---------------------------------------------------------------------------

pub const BINDER_VM_SIZE_DEFAULT: usize = (1024 * 1024) - (4096 * 2);
pub const BINDER_VERSION_CURRENT: i32 = 8;

// ---------------------------------------------------------------------------
// Transaction flags (`flat_binder_object.flags`, `transaction.flags`)
// ---------------------------------------------------------------------------

pub const TF_ONE_WAY: u32 = 1 << 0;
pub const TF_ROOT_OBJECT: u32 = 1 << 2;
pub const TF_STATUS_CODE: u32 = 1 << 3;
pub const TF_ACCEPT_FDS: u32 = 1 << 4;
pub const TF_CLEAR_BUF: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// "BC_*" — commands written from userspace to driver
// ---------------------------------------------------------------------------

pub const BC_TRANSACTION: u32 = 0x4000_0040;
pub const BC_REPLY: u32 = 0x4000_0041;
pub const BC_FREE_BUFFER: u32 = 0x4000_0043;
pub const BC_INCREFS: u32 = 0x4000_0044;
pub const BC_ACQUIRE: u32 = 0x4000_0045;
pub const BC_RELEASE: u32 = 0x4000_0046;
pub const BC_DECREFS: u32 = 0x4000_0047;

// ---------------------------------------------------------------------------
// "BR_*" — return codes read by userspace from driver
// ---------------------------------------------------------------------------

pub const BR_ERROR: u32 = 0x8000_0000;
pub const BR_OK: u32 = 0x8000_0001;
pub const BR_TRANSACTION: u32 = 0x8000_0002;
pub const BR_REPLY: u32 = 0x8000_0003;
pub const BR_DEAD_REPLY: u32 = 0x8000_0005;
pub const BR_TRANSACTION_COMPLETE: u32 = 0x8000_0006;
pub const BR_NOOP: u32 = 0x8000_000C;

// ---------------------------------------------------------------------------
// Binder error codes
// ---------------------------------------------------------------------------

pub const BR_FAILED_REPLY: u32 = 0x8000_0011;

// ---------------------------------------------------------------------------
// /sys/class/binder paths
// ---------------------------------------------------------------------------

pub const SYS_CLASS_BINDER: &str = "/sys/class/binder";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_paths_distinct_and_under_dev() {
        for p in [DEV_BINDER, DEV_HWBINDER, DEV_VNDBINDER] {
            assert!(p.starts_with("/dev/"));
        }
        assert_ne!(DEV_BINDER, DEV_HWBINDER);
        assert_ne!(DEV_HWBINDER, DEV_VNDBINDER);
    }

    #[test]
    fn test_vm_size_default_is_1mib_minus_2_pages() {
        // The userspace AOSP default: 1 MiB - 2 pages of 4 KiB headroom.
        assert_eq!(BINDER_VM_SIZE_DEFAULT, 1024 * 1024 - 8192);
    }

    #[test]
    fn test_tf_flags_each_power_of_two() {
        for v in [
            TF_ONE_WAY,
            TF_ROOT_OBJECT,
            TF_STATUS_CODE,
            TF_ACCEPT_FDS,
            TF_CLEAR_BUF,
        ] {
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_bc_commands_in_user_to_kernel_range() {
        // BC_* commands sit in the 0x4000_00xx block.
        let cmds = [
            BC_TRANSACTION,
            BC_REPLY,
            BC_FREE_BUFFER,
            BC_INCREFS,
            BC_ACQUIRE,
            BC_RELEASE,
            BC_DECREFS,
        ];
        for v in cmds {
            assert_eq!(v & 0xFFFF_FF00, 0x4000_0000);
        }
        // refcount commands form a dense block 0x44..0x47.
        assert_eq!(BC_INCREFS, 0x4000_0044);
        assert_eq!(BC_DECREFS, 0x4000_0047);
        assert_eq!(BC_DECREFS - BC_INCREFS, 3);
    }

    #[test]
    fn test_br_return_codes_in_kernel_to_user_range() {
        // BR_* sits in the 0x8000_00xx block (high bit set).
        let rets = [
            BR_ERROR,
            BR_OK,
            BR_TRANSACTION,
            BR_REPLY,
            BR_DEAD_REPLY,
            BR_TRANSACTION_COMPLETE,
            BR_NOOP,
            BR_FAILED_REPLY,
        ];
        for v in rets {
            assert!(v & 0x8000_0000 != 0);
        }
        // ERROR=0, OK=1, TRANSACTION=2, REPLY=3 form a dense low block.
        assert_eq!(BR_ERROR & 0xFF, 0);
        assert_eq!(BR_OK & 0xFF, 1);
        assert_eq!(BR_TRANSACTION & 0xFF, 2);
        assert_eq!(BR_REPLY & 0xFF, 3);
    }

    #[test]
    fn test_version_is_8() {
        assert_eq!(BINDER_VERSION_CURRENT, 8);
        assert_eq!(SYS_CLASS_BINDER, "/sys/class/binder");
    }
}
