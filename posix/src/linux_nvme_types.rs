//! `<linux/nvme.h>` — NVM Express command set constants.
//!
//! NVMe is the standard interface for accessing non-volatile memory
//! (SSDs) over PCIe. It provides a command-based protocol with
//! submission and completion queues, enabling massive parallelism
//! and low latency for flash storage.

// ---------------------------------------------------------------------------
// Admin command opcodes
// ---------------------------------------------------------------------------

/// Delete I/O Submission Queue.
pub const NVME_ADMIN_DELETE_SQ: u8 = 0x00;
/// Create I/O Submission Queue.
pub const NVME_ADMIN_CREATE_SQ: u8 = 0x01;
/// Get Log Page.
pub const NVME_ADMIN_GET_LOG_PAGE: u8 = 0x02;
/// Delete I/O Completion Queue.
pub const NVME_ADMIN_DELETE_CQ: u8 = 0x04;
/// Create I/O Completion Queue.
pub const NVME_ADMIN_CREATE_CQ: u8 = 0x05;
/// Identify.
pub const NVME_ADMIN_IDENTIFY: u8 = 0x06;
/// Abort.
pub const NVME_ADMIN_ABORT: u8 = 0x08;
/// Set Features.
pub const NVME_ADMIN_SET_FEATURES: u8 = 0x09;
/// Get Features.
pub const NVME_ADMIN_GET_FEATURES: u8 = 0x0A;
/// Async Event Request.
pub const NVME_ADMIN_ASYNC_EVENT: u8 = 0x0C;
/// Namespace Management.
pub const NVME_ADMIN_NS_MGMT: u8 = 0x0D;
/// Firmware Commit.
pub const NVME_ADMIN_FW_COMMIT: u8 = 0x10;
/// Firmware Download.
pub const NVME_ADMIN_FW_DOWNLOAD: u8 = 0x11;
/// Format NVM.
pub const NVME_ADMIN_FORMAT_NVM: u8 = 0x80;
/// Security Send.
pub const NVME_ADMIN_SECURITY_SEND: u8 = 0x81;
/// Security Receive.
pub const NVME_ADMIN_SECURITY_RECV: u8 = 0x82;
/// Sanitize.
pub const NVME_ADMIN_SANITIZE: u8 = 0x84;

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

// ---------------------------------------------------------------------------
// NVMe status codes (generic)
// ---------------------------------------------------------------------------

/// Success.
pub const NVME_SC_SUCCESS: u16 = 0x0000;
/// Invalid Command Opcode.
pub const NVME_SC_INVALID_OPCODE: u16 = 0x0001;
/// Invalid Field in Command.
pub const NVME_SC_INVALID_FIELD: u16 = 0x0002;
/// Command Abort Requested.
pub const NVME_SC_ABORT_REQ: u16 = 0x0007;
/// Namespace Not Ready.
pub const NVME_SC_NS_NOT_READY: u16 = 0x0082;

// ---------------------------------------------------------------------------
// Queue entry sizes
// ---------------------------------------------------------------------------

/// Submission queue entry size (bytes).
pub const NVME_SQE_SIZE: u8 = 64;
/// Completion queue entry size (bytes).
pub const NVME_CQE_SIZE: u8 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_opcodes_distinct() {
        let ops = [
            NVME_ADMIN_DELETE_SQ,
            NVME_ADMIN_CREATE_SQ,
            NVME_ADMIN_GET_LOG_PAGE,
            NVME_ADMIN_DELETE_CQ,
            NVME_ADMIN_CREATE_CQ,
            NVME_ADMIN_IDENTIFY,
            NVME_ADMIN_ABORT,
            NVME_ADMIN_SET_FEATURES,
            NVME_ADMIN_GET_FEATURES,
            NVME_ADMIN_ASYNC_EVENT,
            NVME_ADMIN_NS_MGMT,
            NVME_ADMIN_FW_COMMIT,
            NVME_ADMIN_FW_DOWNLOAD,
            NVME_ADMIN_FORMAT_NVM,
            NVME_ADMIN_SECURITY_SEND,
            NVME_ADMIN_SECURITY_RECV,
            NVME_ADMIN_SANITIZE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_io_opcodes_distinct() {
        let ops = [
            NVME_CMD_FLUSH,
            NVME_CMD_WRITE,
            NVME_CMD_READ,
            NVME_CMD_WRITE_UNCOR,
            NVME_CMD_COMPARE,
            NVME_CMD_WRITE_ZEROES,
            NVME_CMD_DSM,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            NVME_SC_SUCCESS,
            NVME_SC_INVALID_OPCODE,
            NVME_SC_INVALID_FIELD,
            NVME_SC_ABORT_REQ,
            NVME_SC_NS_NOT_READY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_queue_entry_sizes() {
        assert_eq!(NVME_SQE_SIZE, 64);
        assert_eq!(NVME_CQE_SIZE, 16);
    }
}
