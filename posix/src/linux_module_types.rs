//! `<linux/module.h>` — Kernel module loading constants.
//!
//! Linux kernel modules are loadable code that extends kernel
//! functionality at runtime (drivers, filesystems, network protocols).
//! The finit_module() and init_module() syscalls load modules, while
//! delete_module() removes them. Flags control initialization behavior
//! and forced operations.

// ---------------------------------------------------------------------------
// init_module / finit_module flags
// ---------------------------------------------------------------------------

/// Ignore module version magic mismatch.
pub const MODULE_INIT_IGNORE_MODVERSIONS: u32 = 1 << 0;
/// Ignore kernel version mismatch.
pub const MODULE_INIT_IGNORE_VERMAGIC: u32 = 1 << 1;
/// Module is being live-patched.
pub const MODULE_INIT_COMPRESSED_FILE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// delete_module flags
// ---------------------------------------------------------------------------

/// Force removal even if module is in use.
pub const O_TRUNC_MODULE: u32 = 1 << 0;
/// Non-blocking removal (return EWOULDBLOCK if busy).
pub const O_NONBLOCK_MODULE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Module state values
// ---------------------------------------------------------------------------

/// Module is live (normal operation).
pub const MODULE_STATE_LIVE: u32 = 0;
/// Module is being initialized.
pub const MODULE_STATE_COMING: u32 = 1;
/// Module is being removed.
pub const MODULE_STATE_GOING: u32 = 2;
/// Module failed to initialize.
pub const MODULE_STATE_UNFORMED: u32 = 3;

// ---------------------------------------------------------------------------
// Module taint flags (tracking why a module is "tainted")
// ---------------------------------------------------------------------------

/// Proprietary module (no GPL license).
pub const TAINT_PROPRIETARY_MODULE: u32 = 1 << 0;
/// Module was force-loaded.
pub const TAINT_FORCED_MODULE: u32 = 1 << 1;
/// Out-of-tree module.
pub const TAINT_OOT_MODULE: u32 = 1 << 2;
/// Module was force-unloaded.
pub const TAINT_FORCED_RMMOD: u32 = 1 << 3;
/// Staging driver.
pub const TAINT_STAGING: u32 = 1 << 4;
/// Unsigned module.
pub const TAINT_UNSIGNED_MODULE: u32 = 1 << 5;
/// Module uses TEST license.
pub const TAINT_TEST: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_flags_no_overlap() {
        let flags = [
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_delete_flags_no_overlap() {
        assert_eq!(O_TRUNC_MODULE & O_NONBLOCK_MODULE, 0);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            MODULE_STATE_LIVE,
            MODULE_STATE_COMING,
            MODULE_STATE_GOING,
            MODULE_STATE_UNFORMED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_taint_flags_no_overlap() {
        let flags = [
            TAINT_PROPRIETARY_MODULE,
            TAINT_FORCED_MODULE,
            TAINT_OOT_MODULE,
            TAINT_FORCED_RMMOD,
            TAINT_STAGING,
            TAINT_UNSIGNED_MODULE,
            TAINT_TEST,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
