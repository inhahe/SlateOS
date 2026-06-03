//! `<linux/nvme.h>` (status subset) — NVMe completion status codes.
//!
//! NVMe status codes are 15-bit values in the completion queue entry.
//! The status code type (SCT) identifies the category, and the status
//! code (SC) gives the specific error. Status code 0 with SCT 0 means
//! successful completion.

// ---------------------------------------------------------------------------
// Status code types (SCT, bits 9:11 of status field)
// ---------------------------------------------------------------------------

/// Generic command status.
pub const NVME_SCT_GENERIC: u32 = 0x0;
/// Command-specific status.
pub const NVME_SCT_COMMAND_SPECIFIC: u32 = 0x1;
/// Media and data integrity errors.
pub const NVME_SCT_MEDIA: u32 = 0x2;
/// Path-related status.
pub const NVME_SCT_PATH: u32 = 0x3;
/// Vendor-specific status.
pub const NVME_SCT_VENDOR: u32 = 0x7;

// ---------------------------------------------------------------------------
// Generic status codes (SCT=0)
// ---------------------------------------------------------------------------

/// Successful completion.
pub const NVME_SC_SUCCESS: u32 = 0x0;
/// Invalid command opcode.
pub const NVME_SC_INVALID_OPCODE: u32 = 0x1;
/// Invalid field in command.
pub const NVME_SC_INVALID_FIELD: u32 = 0x2;
/// Command ID conflict.
pub const NVME_SC_CMDID_CONFLICT: u32 = 0x3;
/// Data transfer error.
pub const NVME_SC_DATA_XFER_ERROR: u32 = 0x4;
/// Command aborted due to power loss.
pub const NVME_SC_POWER_LOSS: u32 = 0x5;
/// Internal device error.
pub const NVME_SC_INTERNAL: u32 = 0x6;
/// Command abort requested.
pub const NVME_SC_ABORT_REQ: u32 = 0x7;
/// Command aborted due to SQ deletion.
pub const NVME_SC_ABORT_QUEUE: u32 = 0x8;
/// Command aborted due to failed fused command.
pub const NVME_SC_FUSED_FAIL: u32 = 0x9;
/// Command aborted due to missing fused command.
pub const NVME_SC_FUSED_MISSING: u32 = 0xA;
/// Invalid namespace or format.
pub const NVME_SC_INVALID_NS: u32 = 0xB;
/// LBA out of range.
pub const NVME_SC_LBA_RANGE: u32 = 0x80;
/// Capacity exceeded.
pub const NVME_SC_CAP_EXCEEDED: u32 = 0x81;
/// Namespace not ready.
pub const NVME_SC_NS_NOT_READY: u32 = 0x82;

// ---------------------------------------------------------------------------
// Media error status codes (SCT=2)
// ---------------------------------------------------------------------------

/// Write fault.
pub const NVME_SC_WRITE_FAULT: u32 = 0x80;
/// Unrecovered read error.
pub const NVME_SC_READ_ERROR: u32 = 0x81;
/// End-to-end guard check error.
pub const NVME_SC_GUARD_CHECK: u32 = 0x82;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sct_distinct() {
        let scts = [
            NVME_SCT_GENERIC,
            NVME_SCT_COMMAND_SPECIFIC,
            NVME_SCT_MEDIA,
            NVME_SCT_PATH,
            NVME_SCT_VENDOR,
        ];
        for i in 0..scts.len() {
            for j in (i + 1)..scts.len() {
                assert_ne!(scts[i], scts[j]);
            }
        }
    }

    #[test]
    fn test_success_is_zero() {
        assert_eq!(NVME_SC_SUCCESS, 0);
        assert_eq!(NVME_SCT_GENERIC, 0);
    }

    #[test]
    fn test_generic_codes_distinct() {
        let codes = [
            NVME_SC_SUCCESS,
            NVME_SC_INVALID_OPCODE,
            NVME_SC_INVALID_FIELD,
            NVME_SC_CMDID_CONFLICT,
            NVME_SC_DATA_XFER_ERROR,
            NVME_SC_POWER_LOSS,
            NVME_SC_INTERNAL,
            NVME_SC_ABORT_REQ,
            NVME_SC_ABORT_QUEUE,
            NVME_SC_FUSED_FAIL,
            NVME_SC_FUSED_MISSING,
            NVME_SC_INVALID_NS,
            NVME_SC_LBA_RANGE,
            NVME_SC_CAP_EXCEEDED,
            NVME_SC_NS_NOT_READY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
