//! `<linux/nvme.h>` (admin command subset) — NVMe admin command opcodes.
//!
//! Admin commands manage the NVMe controller itself: creating/deleting
//! queues, identifying the device, getting log pages, setting features,
//! firmware management, and namespace operations. They are submitted
//! to the admin submission queue (queue ID 0).

// ---------------------------------------------------------------------------
// Admin command opcodes (nvme_admin_opcode)
// ---------------------------------------------------------------------------

/// Delete I/O submission queue.
pub const NVME_ADMIN_DELETE_SQ: u8 = 0x00;
/// Create I/O submission queue.
pub const NVME_ADMIN_CREATE_SQ: u8 = 0x01;
/// Get log page (error log, SMART, etc.).
pub const NVME_ADMIN_GET_LOG_PAGE: u8 = 0x02;
/// Delete I/O completion queue.
pub const NVME_ADMIN_DELETE_CQ: u8 = 0x04;
/// Create I/O completion queue.
pub const NVME_ADMIN_CREATE_CQ: u8 = 0x05;
/// Identify controller/namespace/list.
pub const NVME_ADMIN_IDENTIFY: u8 = 0x06;
/// Abort a previously submitted command.
pub const NVME_ADMIN_ABORT_CMD: u8 = 0x08;
/// Set feature (power management, temperature threshold, etc.).
pub const NVME_ADMIN_SET_FEATURES: u8 = 0x09;
/// Get feature value.
pub const NVME_ADMIN_GET_FEATURES: u8 = 0x0A;
/// Asynchronous event request.
pub const NVME_ADMIN_ASYNC_EVENT: u8 = 0x0C;
/// Namespace management (create/delete namespace).
pub const NVME_ADMIN_NS_MGMT: u8 = 0x0D;
/// Firmware commit (activate downloaded firmware).
pub const NVME_ADMIN_FW_COMMIT: u8 = 0x10;
/// Firmware image download.
pub const NVME_ADMIN_FW_DOWNLOAD: u8 = 0x11;
/// Namespace attachment (attach/detach to controller).
pub const NVME_ADMIN_NS_ATTACH: u8 = 0x15;
/// Format NVM (low-level format namespace).
pub const NVME_ADMIN_FORMAT_NVM: u8 = 0x80;
/// Security send.
pub const NVME_ADMIN_SECURITY_SEND: u8 = 0x81;
/// Security receive.
pub const NVME_ADMIN_SECURITY_RECV: u8 = 0x82;
/// Sanitize (secure erase all data).
pub const NVME_ADMIN_SANITIZE: u8 = 0x84;

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
            NVME_ADMIN_ABORT_CMD,
            NVME_ADMIN_SET_FEATURES,
            NVME_ADMIN_GET_FEATURES,
            NVME_ADMIN_ASYNC_EVENT,
            NVME_ADMIN_NS_MGMT,
            NVME_ADMIN_FW_COMMIT,
            NVME_ADMIN_FW_DOWNLOAD,
            NVME_ADMIN_NS_ATTACH,
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
    fn test_queue_ops() {
        assert_eq!(NVME_ADMIN_CREATE_SQ, 0x01);
        assert_eq!(NVME_ADMIN_CREATE_CQ, 0x05);
    }

    #[test]
    fn test_identify() {
        assert_eq!(NVME_ADMIN_IDENTIFY, 0x06);
    }

    #[test]
    fn test_firmware_ops() {
        assert_ne!(NVME_ADMIN_FW_COMMIT, NVME_ADMIN_FW_DOWNLOAD);
        assert_eq!(NVME_ADMIN_FW_DOWNLOAD, NVME_ADMIN_FW_COMMIT + 1);
    }
}
