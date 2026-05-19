//! `<linux/target_core_user.h>` — Additional TCMU (Target Core Module in Userspace) constants.
//!
//! Supplementary TCMU constants covering opcodes,
//! device attribute flags, and mailbox flags.

// ---------------------------------------------------------------------------
// TCMU opcodes
// ---------------------------------------------------------------------------

/// Padding entry (no-op).
pub const TCMU_OP_PAD: u8 = 0;
/// Command entry.
pub const TCMU_OP_CMD: u8 = 1;

// ---------------------------------------------------------------------------
// TCMU mailbox flags
// ---------------------------------------------------------------------------

/// Mailbox version 1.
pub const TCMU_MAILBOX_VERSION: u32 = 2;
/// Mailbox flag: device gone.
pub const TCMU_MAILBOX_FLAG_CAP_OOOC: u32 = 1 << 0;
/// Mailbox flag: read length changed.
pub const TCMU_MAILBOX_FLAG_CAP_READ_LEN: u32 = 1 << 1;
/// Mailbox flag: TMR notification.
pub const TCMU_MAILBOX_FLAG_CAP_TMR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// TCMU command flags
// ---------------------------------------------------------------------------

/// Unknown operation.
pub const TCMU_UFLAG_UNKNOWN_OP: u32 = 1 << 0;
/// Read length changed.
pub const TCMU_UFLAG_READ_LEN: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// TCMU TMR (Task Management Request) types
// ---------------------------------------------------------------------------

/// Abort task.
pub const TCMU_TMR_ABORT_TASK: u32 = 1;
/// Abort task set.
pub const TCMU_TMR_ABORT_TASK_SET: u32 = 2;
/// Clear ACA.
pub const TCMU_TMR_CLEAR_ACA: u32 = 3;
/// Clear task set.
pub const TCMU_TMR_CLEAR_TASK_SET: u32 = 4;
/// LUN reset.
pub const TCMU_TMR_LUN_RESET: u32 = 5;
/// Target warm reset.
pub const TCMU_TMR_TARGET_WARM_RESET: u32 = 6;
/// Target cold reset.
pub const TCMU_TMR_TARGET_COLD_RESET: u32 = 7;
/// LUN reset pro.
pub const TCMU_TMR_LUN_RESET_PRO: u32 = 128;

// ---------------------------------------------------------------------------
// TCMU generic status
// ---------------------------------------------------------------------------

/// No status.
pub const TCMU_STATUS_NO_STATUS: u32 = 0;
/// Check condition.
pub const TCMU_STATUS_CHECK_CONDITION: u32 = 2;
/// Reservation conflict.
pub const TCMU_STATUS_RESERVATION_CONFLICT: u32 = 24;
/// Task set full.
pub const TCMU_STATUS_TASK_SET_FULL: u32 = 40;
/// ACA active.
pub const TCMU_STATUS_ACA_ACTIVE: u32 = 48;
/// Task aborted.
pub const TCMU_STATUS_TASK_ABORTED: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        assert_ne!(TCMU_OP_PAD, TCMU_OP_CMD);
    }

    #[test]
    fn test_mailbox_flags_power_of_two() {
        assert!(TCMU_MAILBOX_FLAG_CAP_OOOC.is_power_of_two());
        assert!(TCMU_MAILBOX_FLAG_CAP_READ_LEN.is_power_of_two());
        assert!(TCMU_MAILBOX_FLAG_CAP_TMR.is_power_of_two());
    }

    #[test]
    fn test_mailbox_flags_no_overlap() {
        let flags = [
            TCMU_MAILBOX_FLAG_CAP_OOOC,
            TCMU_MAILBOX_FLAG_CAP_READ_LEN,
            TCMU_MAILBOX_FLAG_CAP_TMR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cmd_flags_no_overlap() {
        assert_eq!(TCMU_UFLAG_UNKNOWN_OP & TCMU_UFLAG_READ_LEN, 0);
    }

    #[test]
    fn test_tmr_types_distinct() {
        let tmrs = [
            TCMU_TMR_ABORT_TASK, TCMU_TMR_ABORT_TASK_SET,
            TCMU_TMR_CLEAR_ACA, TCMU_TMR_CLEAR_TASK_SET,
            TCMU_TMR_LUN_RESET, TCMU_TMR_TARGET_WARM_RESET,
            TCMU_TMR_TARGET_COLD_RESET, TCMU_TMR_LUN_RESET_PRO,
        ];
        for i in 0..tmrs.len() {
            for j in (i + 1)..tmrs.len() {
                assert_ne!(tmrs[i], tmrs[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            TCMU_STATUS_NO_STATUS, TCMU_STATUS_CHECK_CONDITION,
            TCMU_STATUS_RESERVATION_CONFLICT, TCMU_STATUS_TASK_SET_FULL,
            TCMU_STATUS_ACA_ACTIVE, TCMU_STATUS_TASK_ABORTED,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
