//! `<linux/module.h>` — Kernel module management constants.
//!
//! Constants for the `init_module`, `finit_module`, and `delete_module`
//! syscalls used by modprobe, insmod, rmmod, and systemd-modules-load.

use crate::errno;

// ---------------------------------------------------------------------------
// Module init flags
// ---------------------------------------------------------------------------

/// Module init: ignore unknown module parameters.
pub const MODULE_INIT_IGNORE_MODVERSIONS: u32 = 1;
/// Module init: ignore kernel version mismatch.
pub const MODULE_INIT_IGNORE_VERMAGIC: u32 = 2;
/// Module init: compressed module (let kernel decompress).
pub const MODULE_INIT_COMPRESSED_FILE: u32 = 4;

// ---------------------------------------------------------------------------
// Module delete flags
// ---------------------------------------------------------------------------

/// Force module removal even if in use.
pub const O_TRUNC_DELETE: u32 = 1;
/// Non-blocking: return EWOULDBLOCK if module is in use.
pub const O_NONBLOCK_DELETE: u32 = 2;

// ---------------------------------------------------------------------------
// Module state (from /sys/module/*/initstate)
// ---------------------------------------------------------------------------

/// Module is live (loaded and initialized).
pub const MODULE_STATE_LIVE: u32 = 0;
/// Module is being initialized.
pub const MODULE_STATE_COMING: u32 = 1;
/// Module is being removed.
pub const MODULE_STATE_GOING: u32 = 2;
/// Module is unformed (allocation complete, but not initialized).
pub const MODULE_STATE_UNFORMED: u32 = 3;

// ---------------------------------------------------------------------------
// Syscalls
// ---------------------------------------------------------------------------

/// Load a kernel module from a file descriptor.
///
/// Stub — returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn finit_module(_fd: i32, _params: *const u8, _flags: u32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Remove a kernel module.
///
/// Stub — returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn delete_module(_name: *const u8, _flags: u32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_flags_powers_of_two() {
        let flags = [
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f} not power of 2");
        }
    }

    #[test]
    fn test_states_sequential() {
        assert_eq!(MODULE_STATE_LIVE, 0);
        assert_eq!(MODULE_STATE_COMING, 1);
        assert_eq!(MODULE_STATE_GOING, 2);
        assert_eq!(MODULE_STATE_UNFORMED, 3);
    }

    #[test]
    fn test_finit_module_stub() {
        let ret = finit_module(-1, core::ptr::null(), 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_delete_module_stub() {
        let ret = delete_module(core::ptr::null(), 0);
        assert_eq!(ret, -1);
    }
}
