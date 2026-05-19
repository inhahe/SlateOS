//! `<linux/nvme_ioctl.h>` — Additional NVMe constants (part 3).
//!
//! Supplementary NVMe constants covering admin commands,
//! status codes, and feature identifiers.

// ---------------------------------------------------------------------------
// NVMe admin command opcodes
// ---------------------------------------------------------------------------

/// Delete I/O submission queue.
pub const NVME_ADM_CMD_DELETE_SQ: u8 = 0x00;
/// Create I/O submission queue.
pub const NVME_ADM_CMD_CREATE_SQ: u8 = 0x01;
/// Get log page.
pub const NVME_ADM_CMD_GET_LOG_PAGE: u8 = 0x02;
/// Delete I/O completion queue.
pub const NVME_ADM_CMD_DELETE_CQ: u8 = 0x04;
/// Create I/O completion queue.
pub const NVME_ADM_CMD_CREATE_CQ: u8 = 0x05;
/// Identify.
pub const NVME_ADM_CMD_IDENTIFY: u8 = 0x06;
/// Abort.
pub const NVME_ADM_CMD_ABORT: u8 = 0x08;
/// Set features.
pub const NVME_ADM_CMD_SET_FEATURES: u8 = 0x09;
/// Get features.
pub const NVME_ADM_CMD_GET_FEATURES: u8 = 0x0A;
/// Async event request.
pub const NVME_ADM_CMD_ASYNC_EVENT: u8 = 0x0C;
/// Namespace management.
pub const NVME_ADM_CMD_NS_MGMT: u8 = 0x0D;
/// Firmware commit.
pub const NVME_ADM_CMD_FW_COMMIT: u8 = 0x10;
/// Firmware download.
pub const NVME_ADM_CMD_FW_DOWNLOAD: u8 = 0x11;
/// Namespace attachment.
pub const NVME_ADM_CMD_NS_ATTACH: u8 = 0x15;
/// Format NVM.
pub const NVME_ADM_CMD_FORMAT_NVM: u8 = 0x80;
/// Security send.
pub const NVME_ADM_CMD_SECURITY_SEND: u8 = 0x81;
/// Security receive.
pub const NVME_ADM_CMD_SECURITY_RECV: u8 = 0x82;

// ---------------------------------------------------------------------------
// NVMe I/O command opcodes
// ---------------------------------------------------------------------------

/// Flush.
pub const NVME_CMD_FLUSH: u8 = 0x00;
/// Write.
pub const NVME_CMD_WRITE: u8 = 0x01;
/// Read.
pub const NVME_CMD_READ: u8 = 0x02;
/// Write uncorrectable.
pub const NVME_CMD_WRITE_UNCOR: u8 = 0x04;
/// Compare.
pub const NVME_CMD_COMPARE: u8 = 0x05;
/// Write zeroes.
pub const NVME_CMD_WRITE_ZEROES: u8 = 0x08;
/// Dataset management.
pub const NVME_CMD_DSM: u8 = 0x09;
/// Verify.
pub const NVME_CMD_VERIFY: u8 = 0x0C;
/// Zone management send.
pub const NVME_CMD_ZONE_MGMT_SEND: u8 = 0x79;
/// Zone management receive.
pub const NVME_CMD_ZONE_MGMT_RECV: u8 = 0x7A;
/// Zone append.
pub const NVME_CMD_ZONE_APPEND: u8 = 0x7D;

// ---------------------------------------------------------------------------
// NVMe status codes (generic)
// ---------------------------------------------------------------------------

/// Success.
pub const NVME_SC_SUCCESS: u16 = 0x0;
/// Invalid opcode.
pub const NVME_SC_INVALID_OPCODE: u16 = 0x1;
/// Invalid field.
pub const NVME_SC_INVALID_FIELD: u16 = 0x2;
/// Command ID conflict.
pub const NVME_SC_CMDID_CONFLICT: u16 = 0x3;
/// Data transfer error.
pub const NVME_SC_DATA_XFER_ERROR: u16 = 0x4;
/// Abort due to power loss.
pub const NVME_SC_POWER_LOSS: u16 = 0x5;
/// Internal error.
pub const NVME_SC_INTERNAL: u16 = 0x6;

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
            NVME_ADM_CMD_FW_DOWNLOAD, NVME_ADM_CMD_NS_ATTACH,
            NVME_ADM_CMD_FORMAT_NVM, NVME_ADM_CMD_SECURITY_SEND,
            NVME_ADM_CMD_SECURITY_RECV,
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
            NVME_CMD_ZONE_MGMT_SEND, NVME_CMD_ZONE_MGMT_RECV,
            NVME_CMD_ZONE_APPEND,
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
            NVME_SC_INTERNAL,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
