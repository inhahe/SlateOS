//! `<linux/module.h>` — Kernel module loading flag constants.
//!
//! These flags control the behavior of `init_module()` and
//! `finit_module()` syscalls when loading kernel modules. They
//! specify whether to suppress errors, force loading despite
//! version mismatches, or defer initialization.

// ---------------------------------------------------------------------------
// finit_module() / init_module() flags
// ---------------------------------------------------------------------------

/// Ignore module version magic mismatch.
pub const MODULE_INIT_IGNORE_MODVERSIONS: u32 = 0x01;
/// Ignore kernel version mismatch.
pub const MODULE_INIT_IGNORE_VERMAGIC: u32 = 0x02;
/// Module decompression handled by kernel.
pub const MODULE_INIT_COMPRESSED_FILE: u32 = 0x04;

// ---------------------------------------------------------------------------
// Module state values
// ---------------------------------------------------------------------------

/// Module is being loaded (init running).
pub const MODULE_STATE_COMING: u32 = 0;
/// Module is live (init complete).
pub const MODULE_STATE_LIVE: u32 = 1;
/// Module is being removed.
pub const MODULE_STATE_GOING: u32 = 2;
/// Module is unformed (allocation, before init).
pub const MODULE_STATE_UNFORMED: u32 = 3;

// ---------------------------------------------------------------------------
// delete_module() flags
// ---------------------------------------------------------------------------

/// Force module removal even if in use.
pub const O_TRUNC_MODULE: u32 = 0x01;
/// Non-blocking removal (fail if busy).
pub const O_NONBLOCK_MODULE: u32 = 0x02;

// ---------------------------------------------------------------------------
// Module taint flags
// ---------------------------------------------------------------------------

/// Proprietary module (no source).
pub const MODULE_TAINT_PROPRIETARY: u32 = 1 << 0;
/// Module was force-loaded.
pub const MODULE_TAINT_FORCED: u32 = 1 << 1;
/// Out-of-tree module.
pub const MODULE_TAINT_OOT: u32 = 1 << 2;
/// Module from staging tree.
pub const MODULE_TAINT_STAGING: u32 = 1 << 3;
/// Unsigned module.
pub const MODULE_TAINT_UNSIGNED: u32 = 1 << 4;
/// Module uses test-only API.
pub const MODULE_TAINT_TEST: u32 = 1 << 5;

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
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            MODULE_STATE_COMING,
            MODULE_STATE_LIVE,
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
            MODULE_TAINT_PROPRIETARY,
            MODULE_TAINT_FORCED,
            MODULE_TAINT_OOT,
            MODULE_TAINT_STAGING,
            MODULE_TAINT_UNSIGNED,
            MODULE_TAINT_TEST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_live_is_one() {
        assert_eq!(MODULE_STATE_LIVE, 1);
    }
}
