//! `<linux/nvme_ioctl.h>` — NVMe device ioctls.
//!
//! NVMe (Non-Volatile Memory Express) is the standard interface for
//! PCIe-attached SSDs. These ioctls allow userspace to send admin
//! and I/O commands to NVMe devices.

// ---------------------------------------------------------------------------
// NVMe ioctl commands
// ---------------------------------------------------------------------------

/// Submit an admin command.
pub const NVME_IOCTL_ADMIN_CMD: u64 = 0xC0484E41;
/// Submit an I/O command.
pub const NVME_IOCTL_IO_CMD: u64 = 0xC0484E43;
/// Submit an admin command (64-bit).
pub const NVME_IOCTL_ADMIN64_CMD: u64 = 0xC0504E41;
/// Submit an I/O command (64-bit).
pub const NVME_IOCTL_IO64_CMD: u64 = 0xC0504E43;
/// Reset the controller.
pub const NVME_IOCTL_RESET: u64 = 0x4E44;
/// Subsystem reset.
pub const NVME_IOCTL_SUBSYS_RESET: u64 = 0x4E45;
/// Rescan namespaces.
pub const NVME_IOCTL_RESCAN: u64 = 0x4E46;
/// Get NVMe identifier.
pub const NVME_IOCTL_ID: u64 = 0x40044E40;

// ---------------------------------------------------------------------------
// NVMe admin command opcodes
// ---------------------------------------------------------------------------

/// Delete I/O submission queue.
pub const NVME_ADMIN_DELETE_SQ: u8 = 0x00;
/// Create I/O submission queue.
pub const NVME_ADMIN_CREATE_SQ: u8 = 0x01;
/// Get log page.
pub const NVME_ADMIN_GET_LOG_PAGE: u8 = 0x02;
/// Delete I/O completion queue.
pub const NVME_ADMIN_DELETE_CQ: u8 = 0x04;
/// Create I/O completion queue.
pub const NVME_ADMIN_CREATE_CQ: u8 = 0x05;
/// Identify.
pub const NVME_ADMIN_IDENTIFY: u8 = 0x06;
/// Abort.
pub const NVME_ADMIN_ABORT: u8 = 0x08;
/// Set features.
pub const NVME_ADMIN_SET_FEATURES: u8 = 0x09;
/// Get features.
pub const NVME_ADMIN_GET_FEATURES: u8 = 0x0A;
/// Async event request.
pub const NVME_ADMIN_ASYNC_EVENT: u8 = 0x0C;
/// Namespace management.
pub const NVME_ADMIN_NS_MGMT: u8 = 0x0D;
/// Firmware commit.
pub const NVME_ADMIN_FW_COMMIT: u8 = 0x10;
/// Firmware image download.
pub const NVME_ADMIN_FW_DOWNLOAD: u8 = 0x11;
/// Namespace attachment.
pub const NVME_ADMIN_NS_ATTACH: u8 = 0x15;
/// Format NVM.
pub const NVME_ADMIN_FORMAT_NVM: u8 = 0x80;
/// Security send.
pub const NVME_ADMIN_SECURITY_SEND: u8 = 0x81;
/// Security receive.
pub const NVME_ADMIN_SECURITY_RECV: u8 = 0x82;
/// Sanitize.
pub const NVME_ADMIN_SANITIZE: u8 = 0x84;

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
/// Dataset management (TRIM).
pub const NVME_CMD_DSM: u8 = 0x09;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            NVME_IOCTL_ADMIN_CMD,
            NVME_IOCTL_IO_CMD,
            NVME_IOCTL_ADMIN64_CMD,
            NVME_IOCTL_IO64_CMD,
            NVME_IOCTL_RESET,
            NVME_IOCTL_SUBSYS_RESET,
            NVME_IOCTL_RESCAN,
            NVME_IOCTL_ID,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

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
            NVME_ADMIN_FORMAT_NVM,
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
}
