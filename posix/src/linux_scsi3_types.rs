//! `<linux/scsi/scsi.h>` — Additional SCSI constants (part 3).
//!
//! Supplementary SCSI constants covering sense key codes,
//! additional sense codes, and task management function values.

// ---------------------------------------------------------------------------
// SCSI sense keys
// ---------------------------------------------------------------------------

/// No sense.
pub const NO_SENSE: u8 = 0x00;
/// Recovered error.
pub const RECOVERED_ERROR: u8 = 0x01;
/// Not ready.
pub const NOT_READY: u8 = 0x02;
/// Medium error.
pub const MEDIUM_ERROR: u8 = 0x03;
/// Hardware error.
pub const HARDWARE_ERROR: u8 = 0x04;
/// Illegal request.
pub const ILLEGAL_REQUEST: u8 = 0x05;
/// Unit attention.
pub const UNIT_ATTENTION: u8 = 0x06;
/// Data protect.
pub const DATA_PROTECT: u8 = 0x07;
/// Blank check.
pub const BLANK_CHECK: u8 = 0x08;
/// Vendor specific.
pub const VENDOR_SPECIFIC: u8 = 0x09;
/// Copy aborted.
pub const COPY_ABORTED: u8 = 0x0A;
/// Aborted command.
pub const ABORTED_COMMAND: u8 = 0x0B;
/// Volume overflow.
pub const VOLUME_OVERFLOW: u8 = 0x0D;
/// Miscompare.
pub const MISCOMPARE: u8 = 0x0E;
/// Completed.
pub const SCSI_COMPLETED: u8 = 0x0F;

// ---------------------------------------------------------------------------
// SCSI task management function values
// ---------------------------------------------------------------------------

/// Abort task.
pub const TMF_ABORT_TASK: u32 = 1;
/// Abort task set.
pub const TMF_ABORT_TASK_SET: u32 = 2;
/// Clear ACA.
pub const TMF_CLEAR_ACA: u32 = 3;
/// Clear task set.
pub const TMF_CLEAR_TASK_SET: u32 = 4;
/// LUN reset.
pub const TMF_LUN_RESET: u32 = 5;
/// Target reset.
pub const TMF_TARGET_RESET: u32 = 6;
/// Logical unit reset.
pub const TMF_LOGICAL_UNIT_RESET: u32 = 7;
/// Query task.
pub const TMF_QUERY_TASK: u32 = 8;

// ---------------------------------------------------------------------------
// SCSI target responses
// ---------------------------------------------------------------------------

/// Function complete.
pub const TMF_RESP_FUNC_COMPLETE: u32 = 0;
/// Invalid frame.
pub const TMF_RESP_INVALID_FRAME: u32 = 2;
/// Not supported.
pub const TMF_RESP_FUNC_ESUPP: u32 = 4;
/// Function failed.
pub const TMF_RESP_FUNC_FAILED: u32 = 5;
/// Function succeeded.
pub const TMF_RESP_FUNC_SUCC: u32 = 8;
/// Incorrect LUN.
pub const TMF_RESP_INCORRECT_LUN: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sense_keys_distinct() {
        let keys = [
            NO_SENSE,
            RECOVERED_ERROR,
            NOT_READY,
            MEDIUM_ERROR,
            HARDWARE_ERROR,
            ILLEGAL_REQUEST,
            UNIT_ATTENTION,
            DATA_PROTECT,
            BLANK_CHECK,
            VENDOR_SPECIFIC,
            COPY_ABORTED,
            ABORTED_COMMAND,
            VOLUME_OVERFLOW,
            MISCOMPARE,
            SCSI_COMPLETED,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_tmf_values_distinct() {
        let tmfs = [
            TMF_ABORT_TASK,
            TMF_ABORT_TASK_SET,
            TMF_CLEAR_ACA,
            TMF_CLEAR_TASK_SET,
            TMF_LUN_RESET,
            TMF_TARGET_RESET,
            TMF_LOGICAL_UNIT_RESET,
            TMF_QUERY_TASK,
        ];
        for i in 0..tmfs.len() {
            for j in (i + 1)..tmfs.len() {
                assert_ne!(tmfs[i], tmfs[j]);
            }
        }
    }

    #[test]
    fn test_tmf_responses_distinct() {
        let resps = [
            TMF_RESP_FUNC_COMPLETE,
            TMF_RESP_INVALID_FRAME,
            TMF_RESP_FUNC_ESUPP,
            TMF_RESP_FUNC_FAILED,
            TMF_RESP_FUNC_SUCC,
            TMF_RESP_INCORRECT_LUN,
        ];
        for i in 0..resps.len() {
            for j in (i + 1)..resps.len() {
                assert_ne!(resps[i], resps[j]);
            }
        }
    }
}
