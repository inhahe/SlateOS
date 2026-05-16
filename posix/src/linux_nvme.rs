//! `<linux/nvme.h>` — NVMe protocol constants.
//!
//! NVMe (Non-Volatile Memory Express) is the standard protocol
//! for PCIe-attached solid-state storage. This module defines
//! admin and I/O command opcodes, status codes, feature IDs,
//! and namespace/controller constants.

// ---------------------------------------------------------------------------
// Admin command opcodes
// ---------------------------------------------------------------------------

/// Delete I/O Submission Queue.
pub const NVME_ADM_CMD_DELETE_SQ: u8 = 0x00;
/// Create I/O Submission Queue.
pub const NVME_ADM_CMD_CREATE_SQ: u8 = 0x01;
/// Get Log Page.
pub const NVME_ADM_CMD_GET_LOG_PAGE: u8 = 0x02;
/// Delete I/O Completion Queue.
pub const NVME_ADM_CMD_DELETE_CQ: u8 = 0x04;
/// Create I/O Completion Queue.
pub const NVME_ADM_CMD_CREATE_CQ: u8 = 0x05;
/// Identify.
pub const NVME_ADM_CMD_IDENTIFY: u8 = 0x06;
/// Abort.
pub const NVME_ADM_CMD_ABORT: u8 = 0x08;
/// Set Features.
pub const NVME_ADM_CMD_SET_FEATURES: u8 = 0x09;
/// Get Features.
pub const NVME_ADM_CMD_GET_FEATURES: u8 = 0x0A;
/// Async Event Request.
pub const NVME_ADM_CMD_ASYNC_EVENT: u8 = 0x0C;
/// Namespace Management.
pub const NVME_ADM_CMD_NS_MGMT: u8 = 0x0D;
/// Firmware Commit.
pub const NVME_ADM_CMD_FW_COMMIT: u8 = 0x10;
/// Firmware Download.
pub const NVME_ADM_CMD_FW_DOWNLOAD: u8 = 0x11;
/// Format NVM.
pub const NVME_ADM_CMD_FORMAT_NVM: u8 = 0x80;
/// Security Send.
pub const NVME_ADM_CMD_SECURITY_SEND: u8 = 0x81;
/// Security Receive.
pub const NVME_ADM_CMD_SECURITY_RECV: u8 = 0x82;
/// Sanitize.
pub const NVME_ADM_CMD_SANITIZE: u8 = 0x84;

// ---------------------------------------------------------------------------
// I/O command opcodes
// ---------------------------------------------------------------------------

/// Flush.
pub const NVME_CMD_FLUSH: u8 = 0x00;
/// Write.
pub const NVME_CMD_WRITE: u8 = 0x01;
/// Read.
pub const NVME_CMD_READ: u8 = 0x02;
/// Write Uncorrectable.
pub const NVME_CMD_WRITE_UNCOR: u8 = 0x04;
/// Compare.
pub const NVME_CMD_COMPARE: u8 = 0x05;
/// Write Zeroes.
pub const NVME_CMD_WRITE_ZEROES: u8 = 0x08;
/// Dataset Management (TRIM/discard).
pub const NVME_CMD_DSM: u8 = 0x09;
/// Verify.
pub const NVME_CMD_VERIFY: u8 = 0x0C;
/// Reservation Register.
pub const NVME_CMD_RESV_REGISTER: u8 = 0x0D;
/// Reservation Report.
pub const NVME_CMD_RESV_REPORT: u8 = 0x0E;
/// Reservation Acquire.
pub const NVME_CMD_RESV_ACQUIRE: u8 = 0x11;
/// Reservation Release.
pub const NVME_CMD_RESV_RELEASE: u8 = 0x15;

// ---------------------------------------------------------------------------
// Status codes (generic)
// ---------------------------------------------------------------------------

/// Success.
pub const NVME_SC_SUCCESS: u16 = 0x0000;
/// Invalid Command Opcode.
pub const NVME_SC_INVALID_OPCODE: u16 = 0x0001;
/// Invalid Field in Command.
pub const NVME_SC_INVALID_FIELD: u16 = 0x0002;
/// Command ID Conflict.
pub const NVME_SC_CMDID_CONFLICT: u16 = 0x0003;
/// Data Transfer Error.
pub const NVME_SC_DATA_XFER_ERROR: u16 = 0x0004;
/// Abort due to Power Loss.
pub const NVME_SC_POWER_LOSS: u16 = 0x0005;
/// Internal Error.
pub const NVME_SC_INTERNAL: u16 = 0x0006;
/// Abort Requested.
pub const NVME_SC_ABORT_REQ: u16 = 0x0007;
/// Namespace Not Ready.
pub const NVME_SC_NS_NOT_READY: u16 = 0x0082;

// ---------------------------------------------------------------------------
// Queue sizes
// ---------------------------------------------------------------------------

/// Maximum queue depth.
pub const NVME_MAX_QUEUE_SIZE: u32 = 65536;
/// Minimum queue depth.
pub const NVME_MIN_QUEUE_SIZE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_cmds_distinct() {
        let cmds = [
            NVME_ADM_CMD_DELETE_SQ, NVME_ADM_CMD_CREATE_SQ,
            NVME_ADM_CMD_GET_LOG_PAGE, NVME_ADM_CMD_DELETE_CQ,
            NVME_ADM_CMD_CREATE_CQ, NVME_ADM_CMD_IDENTIFY,
            NVME_ADM_CMD_ABORT, NVME_ADM_CMD_SET_FEATURES,
            NVME_ADM_CMD_GET_FEATURES, NVME_ADM_CMD_ASYNC_EVENT,
            NVME_ADM_CMD_NS_MGMT, NVME_ADM_CMD_FW_COMMIT,
            NVME_ADM_CMD_FW_DOWNLOAD, NVME_ADM_CMD_FORMAT_NVM,
            NVME_ADM_CMD_SECURITY_SEND, NVME_ADM_CMD_SECURITY_RECV,
            NVME_ADM_CMD_SANITIZE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_io_cmds_distinct() {
        let cmds = [
            NVME_CMD_FLUSH, NVME_CMD_WRITE, NVME_CMD_READ,
            NVME_CMD_WRITE_UNCOR, NVME_CMD_COMPARE,
            NVME_CMD_WRITE_ZEROES, NVME_CMD_DSM, NVME_CMD_VERIFY,
            NVME_CMD_RESV_REGISTER, NVME_CMD_RESV_REPORT,
            NVME_CMD_RESV_ACQUIRE, NVME_CMD_RESV_RELEASE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            NVME_SC_SUCCESS, NVME_SC_INVALID_OPCODE,
            NVME_SC_INVALID_FIELD, NVME_SC_CMDID_CONFLICT,
            NVME_SC_DATA_XFER_ERROR, NVME_SC_POWER_LOSS,
            NVME_SC_INTERNAL, NVME_SC_ABORT_REQ,
            NVME_SC_NS_NOT_READY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_success_is_zero() {
        assert_eq!(NVME_SC_SUCCESS, 0);
    }

    #[test]
    fn test_queue_sizes() {
        assert!(NVME_MIN_QUEUE_SIZE < NVME_MAX_QUEUE_SIZE);
        assert_eq!(NVME_MIN_QUEUE_SIZE, 2);
    }
}
