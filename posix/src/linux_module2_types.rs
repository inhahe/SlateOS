//! `<linux/module.h>` — Additional kernel module constants.
//!
//! Supplementary module constants covering init_module flags,
//! module states, and finit_module flags.

// ---------------------------------------------------------------------------
// init_module / finit_module flags
// ---------------------------------------------------------------------------

/// Module init: suppress version check.
pub const MODULE_INIT_IGNORE_MODVERSIONS: u32 = 0x0001;
/// Module init: suppress vermagic check.
pub const MODULE_INIT_IGNORE_VERMAGIC: u32 = 0x0002;
/// Module init: compressed module.
pub const MODULE_INIT_COMPRESSED_FILE: u32 = 0x0004;

// ---------------------------------------------------------------------------
// delete_module flags
// ---------------------------------------------------------------------------

/// Force remove.
pub const O_TRUNC_MOD: u32 = 0x0001;
/// Non-blocking remove.
pub const O_NONBLOCK_MOD: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Module states
// ---------------------------------------------------------------------------

/// Module is live.
pub const MODULE_STATE_LIVE: u32 = 0;
/// Module is coming (loading).
pub const MODULE_STATE_COMING: u32 = 1;
/// Module is going (unloading).
pub const MODULE_STATE_GOING: u32 = 2;
/// Unformed (early allocation).
pub const MODULE_STATE_UNFORMED: u32 = 3;

// ---------------------------------------------------------------------------
// Module taint flags
// ---------------------------------------------------------------------------

/// Proprietary module.
pub const MODULE_TAINT_PROPRIETARY: u32 = 1 << 0;
/// Force loaded.
pub const MODULE_TAINT_FORCED_MODULE: u32 = 1 << 1;
/// CPU out of spec.
pub const MODULE_TAINT_CPU_OUT_OF_SPEC: u32 = 1 << 2;
/// Force unloaded.
pub const MODULE_TAINT_FORCED_RMMOD: u32 = 1 << 3;
/// Staging driver.
pub const MODULE_TAINT_STAGING: u32 = 1 << 4;
/// Unsigned module.
pub const MODULE_TAINT_UNSIGNED_MODULE: u32 = 1 << 5;
/// Out-of-tree.
pub const MODULE_TAINT_OOT_MODULE: u32 = 1 << 6;
/// Livepatch.
pub const MODULE_TAINT_LIVEPATCH: u32 = 1 << 7;
/// Test module.
pub const MODULE_TAINT_TEST: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Module section flags
// ---------------------------------------------------------------------------

/// Init section.
pub const MODULE_SECT_INIT: u32 = 0x01;
/// Core section.
pub const MODULE_SECT_CORE: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_flags_power_of_two() {
        let flags = [
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

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
            MODULE_STATE_LIVE, MODULE_STATE_COMING,
            MODULE_STATE_GOING, MODULE_STATE_UNFORMED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_taint_flags_power_of_two() {
        let flags = [
            MODULE_TAINT_PROPRIETARY, MODULE_TAINT_FORCED_MODULE,
            MODULE_TAINT_CPU_OUT_OF_SPEC, MODULE_TAINT_FORCED_RMMOD,
            MODULE_TAINT_STAGING, MODULE_TAINT_UNSIGNED_MODULE,
            MODULE_TAINT_OOT_MODULE, MODULE_TAINT_LIVEPATCH,
            MODULE_TAINT_TEST,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_taint_flags_no_overlap() {
        let flags = [
            MODULE_TAINT_PROPRIETARY, MODULE_TAINT_FORCED_MODULE,
            MODULE_TAINT_CPU_OUT_OF_SPEC, MODULE_TAINT_FORCED_RMMOD,
            MODULE_TAINT_STAGING, MODULE_TAINT_UNSIGNED_MODULE,
            MODULE_TAINT_OOT_MODULE, MODULE_TAINT_LIVEPATCH,
            MODULE_TAINT_TEST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_section_flags() {
        assert_ne!(MODULE_SECT_INIT, MODULE_SECT_CORE);
    }
}
