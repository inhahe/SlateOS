//! `<linux/bug.h>` — Kernel bug/warn infrastructure constants.
//!
//! BUG() and WARN() are the kernel's assertion/diagnostic macros.
//! BUG() triggers a fatal error (oops/panic). WARN() logs a
//! warning with stack trace but continues execution. This module
//! defines the flags and constants for bug table entries.

// ---------------------------------------------------------------------------
// Bug flags
// ---------------------------------------------------------------------------

/// Warning (not fatal).
pub const BUGFLAG_WARNING: u32 = 1 << 0;
/// Once-only warning.
pub const BUGFLAG_ONCE: u32 = 1 << 1;
/// Done (once-only already triggered).
pub const BUGFLAG_DONE: u32 = 1 << 2;
/// No cut-here marker.
pub const BUGFLAG_NO_CUT_HERE: u32 = 1 << 3;
/// Taint on trigger.
pub const BUGFLAG_TAINT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Taint flags (kernel taint bits)
// ---------------------------------------------------------------------------

/// Proprietary module loaded.
pub const TAINT_PROPRIETARY_MODULE: u32 = 0;
/// Module was force-loaded.
pub const TAINT_FORCED_MODULE: u32 = 1;
/// CPU SMP mismatch.
pub const TAINT_CPU_OUT_OF_SPEC: u32 = 2;
/// Force rmmod used.
pub const TAINT_FORCED_RMMOD: u32 = 3;
/// Machine check exception occurred.
pub const TAINT_MACHINE_CHECK: u32 = 4;
/// Bad page reference.
pub const TAINT_BAD_PAGE: u32 = 5;
/// Userspace wrote to /dev/mem.
pub const TAINT_USER: u32 = 6;
/// Kernel oops occurred but continued.
pub const TAINT_DIE: u32 = 7;
/// ACPI DSDT overridden.
pub const TAINT_OVERRIDDEN_ACPI_TABLE: u32 = 8;
/// WARN occurred.
pub const TAINT_WARN: u32 = 9;
/// Staging driver loaded.
pub const TAINT_CRAP: u32 = 10;
/// Firmware workaround applied.
pub const TAINT_FIRMWARE_WORKAROUND: u32 = 11;
/// Out-of-tree module loaded.
pub const TAINT_OOT_MODULE: u32 = 12;
/// Unsigned module loaded.
pub const TAINT_UNSIGNED_MODULE: u32 = 13;
/// Soft lockup occurred.
pub const TAINT_SOFTLOCKUP: u32 = 14;
/// Live patched.
pub const TAINT_LIVEPATCH: u32 = 15;
/// AUX taint.
pub const TAINT_AUX: u32 = 16;
/// Random seed unsure.
pub const TAINT_RANDSTRUCT: u32 = 17;
/// Test module loaded.
pub const TAINT_TEST: u32 = 18;

// ---------------------------------------------------------------------------
// Taint flag characters (for /proc/sys/kernel/tainted display)
// ---------------------------------------------------------------------------

/// Taint character for proprietary module.
pub const TAINT_CHAR_PROPRIETARY: char = 'P';
/// Taint character for force-loaded module.
pub const TAINT_CHAR_FORCED: char = 'F';
/// Taint character for SMP issues.
pub const TAINT_CHAR_CPU: char = 'S';
/// Taint character for WARN.
pub const TAINT_CHAR_WARN: char = 'W';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bugflags_powers_of_two() {
        let flags = [
            BUGFLAG_WARNING, BUGFLAG_ONCE, BUGFLAG_DONE,
            BUGFLAG_NO_CUT_HERE, BUGFLAG_TAINT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_bugflags_no_overlap() {
        let flags = [
            BUGFLAG_WARNING, BUGFLAG_ONCE, BUGFLAG_DONE,
            BUGFLAG_NO_CUT_HERE, BUGFLAG_TAINT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_taint_bits_distinct() {
        let taints = [
            TAINT_PROPRIETARY_MODULE, TAINT_FORCED_MODULE,
            TAINT_CPU_OUT_OF_SPEC, TAINT_FORCED_RMMOD,
            TAINT_MACHINE_CHECK, TAINT_BAD_PAGE, TAINT_USER,
            TAINT_DIE, TAINT_OVERRIDDEN_ACPI_TABLE, TAINT_WARN,
            TAINT_CRAP, TAINT_FIRMWARE_WORKAROUND,
            TAINT_OOT_MODULE, TAINT_UNSIGNED_MODULE,
            TAINT_SOFTLOCKUP, TAINT_LIVEPATCH, TAINT_AUX,
            TAINT_RANDSTRUCT, TAINT_TEST,
        ];
        for i in 0..taints.len() {
            for j in (i + 1)..taints.len() {
                assert_ne!(taints[i], taints[j]);
            }
        }
    }

    #[test]
    fn test_taint_chars_distinct() {
        let chars = [
            TAINT_CHAR_PROPRIETARY, TAINT_CHAR_FORCED,
            TAINT_CHAR_CPU, TAINT_CHAR_WARN,
        ];
        for i in 0..chars.len() {
            for j in (i + 1)..chars.len() {
                assert_ne!(chars[i], chars[j]);
            }
        }
    }
}
