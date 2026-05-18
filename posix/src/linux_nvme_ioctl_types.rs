//! `<linux/nvme_ioctl.h>` — NVMe ioctl command constants.
//!
//! These constants define the ioctl interface for sending NVMe
//! admin and I/O commands directly to NVMe devices from userspace,
//! bypassing the block layer.

// ---------------------------------------------------------------------------
// NVMe ioctl commands
// ---------------------------------------------------------------------------

/// Submit an admin command.
pub const NVME_IOCTL_ADMIN_CMD: u32 = 0xC048_4E41;
/// Submit an I/O command.
pub const NVME_IOCTL_IO_CMD: u32 = 0xC048_4E43;
/// Submit a command via passthrough (admin64).
pub const NVME_IOCTL_ADMIN64_CMD: u32 = 0xC080_4E41;
/// Submit a command via passthrough (io64).
pub const NVME_IOCTL_IO64_CMD: u32 = 0xC080_4E43;
/// Reset the NVMe controller.
pub const NVME_IOCTL_RESET: u32 = 0x4E44;
/// Rescan namespaces.
pub const NVME_IOCTL_RESCAN: u32 = 0x4E46;
/// Subsystem reset.
pub const NVME_IOCTL_SUBSYS_RESET: u32 = 0x4E45;

// ---------------------------------------------------------------------------
// NVMe generic status codes (SCT=0)
// ---------------------------------------------------------------------------

/// Command completed successfully.
pub const NVME_SC_SUCCESS: u16 = 0x0000;
/// Invalid command opcode.
pub const NVME_SC_INVALID_OPCODE: u16 = 0x0001;
/// Invalid field in command.
pub const NVME_SC_INVALID_FIELD: u16 = 0x0002;
/// Command ID conflict.
pub const NVME_SC_CMDID_CONFLICT: u16 = 0x0003;
/// Data transfer error.
pub const NVME_SC_DATA_XFER_ERROR: u16 = 0x0004;
/// Abort due to power loss.
pub const NVME_SC_POWER_LOSS: u16 = 0x0005;
/// Internal device error.
pub const NVME_SC_INTERNAL: u16 = 0x0006;
/// Command abort requested.
pub const NVME_SC_ABORT_REQ: u16 = 0x0007;
/// SQ deletion abort.
pub const NVME_SC_ABORT_QUEUE: u16 = 0x0008;
/// Namespace not ready.
pub const NVME_SC_NS_NOT_READY: u16 = 0x0082;

// ---------------------------------------------------------------------------
// NVMe command set identifiers
// ---------------------------------------------------------------------------

/// NVM command set.
pub const NVME_CSI_NVM: u8 = 0;
/// Zoned namespace command set.
pub const NVME_CSI_ZNS: u8 = 2;
/// Key-value command set.
pub const NVME_CSI_KV: u8 = 3;

// ---------------------------------------------------------------------------
// NVMe controller types
// ---------------------------------------------------------------------------

/// I/O controller.
pub const NVME_CTRL_IO: u8 = 1;
/// Discovery controller.
pub const NVME_CTRL_DISC: u8 = 2;
/// Admin-only controller.
pub const NVME_CTRL_ADMIN: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            NVME_IOCTL_ADMIN_CMD, NVME_IOCTL_IO_CMD,
            NVME_IOCTL_ADMIN64_CMD, NVME_IOCTL_IO64_CMD,
            NVME_IOCTL_RESET, NVME_IOCTL_RESCAN,
            NVME_IOCTL_SUBSYS_RESET,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
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
            NVME_SC_ABORT_QUEUE, NVME_SC_NS_NOT_READY,
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
    fn test_csi_distinct() {
        let csis = [NVME_CSI_NVM, NVME_CSI_ZNS, NVME_CSI_KV];
        for i in 0..csis.len() {
            for j in (i + 1)..csis.len() {
                assert_ne!(csis[i], csis[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_types_distinct() {
        let types = [NVME_CTRL_IO, NVME_CTRL_DISC, NVME_CTRL_ADMIN];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
