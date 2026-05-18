//! `<scsi/sg.h>` — SCSI Generic (SG) ioctl constants.
//!
//! The SCSI generic interface allows userspace to send SCSI
//! commands directly to devices via ioctl. These constants define
//! the SG_IO interface version, direction flags, and status codes.

// ---------------------------------------------------------------------------
// SG interface version
// ---------------------------------------------------------------------------

/// SG interface version 3 ('S').
pub const SG_IO: u32 = 0x2285;
/// SG interface version identifier.
pub const SG_INTERFACE_ID_ORIG: i32 = b'S' as i32;

// ---------------------------------------------------------------------------
// SG_IO direction flags (dxfer_direction)
// ---------------------------------------------------------------------------

/// No data transfer.
pub const SG_DXFER_NONE: i32 = -1;
/// Transfer from device to host (read).
pub const SG_DXFER_TO_DEV: i32 = -2;
/// Transfer from host to device (write).
pub const SG_DXFER_FROM_DEV: i32 = -3;
/// Bidirectional transfer.
pub const SG_DXFER_TO_FROM_DEV: i32 = -4;

// ---------------------------------------------------------------------------
// SCSI status codes
// ---------------------------------------------------------------------------

/// Good status (command completed successfully).
pub const SCSI_STATUS_GOOD: u8 = 0x00;
/// Check condition (sense data available).
pub const SCSI_STATUS_CHECK_CONDITION: u8 = 0x02;
/// Condition met.
pub const SCSI_STATUS_CONDITION_MET: u8 = 0x04;
/// Device is busy.
pub const SCSI_STATUS_BUSY: u8 = 0x08;
/// Reservation conflict.
pub const SCSI_STATUS_RESERVATION_CONFLICT: u8 = 0x18;
/// Task set full (queue full).
pub const SCSI_STATUS_TASK_SET_FULL: u8 = 0x28;
/// ACA active.
pub const SCSI_STATUS_ACA_ACTIVE: u8 = 0x30;
/// Task aborted.
pub const SCSI_STATUS_TASK_ABORTED: u8 = 0x40;

// ---------------------------------------------------------------------------
// SG info flags (returned in sg_io_hdr.info)
// ---------------------------------------------------------------------------

/// Command completed normally.
pub const SG_INFO_OK: u32 = 0x00;
/// Check condition occurred.
pub const SG_INFO_CHECK: u32 = 0x01;
/// Direct I/O performed.
pub const SG_INFO_DIRECT_IO: u32 = 0x02;
/// Mixed direct/indirect I/O.
pub const SG_INFO_MIXED_IO: u32 = 0x04;

// ---------------------------------------------------------------------------
// SG ioctl commands (other than SG_IO)
// ---------------------------------------------------------------------------

/// Get SG driver version.
pub const SG_GET_VERSION_NUM: u32 = 0x2282;
/// Set command timeout (jiffies).
pub const SG_SET_TIMEOUT: u32 = 0x2201;
/// Get command timeout.
pub const SG_GET_TIMEOUT: u32 = 0x2202;
/// Get SCSI command length.
pub const SG_GET_COMMAND_Q: u32 = 0x2270;
/// Set SCSI command queue.
pub const SG_SET_COMMAND_Q: u32 = 0x2271;
/// Set reserved buffer size.
pub const SG_SET_RESERVED_SIZE: u32 = 0x2275;
/// Get reserved buffer size.
pub const SG_GET_RESERVED_SIZE: u32 = 0x2272;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sg_io() {
        assert_eq!(SG_IO, 0x2285);
    }

    #[test]
    fn test_directions_distinct() {
        let dirs = [
            SG_DXFER_NONE, SG_DXFER_TO_DEV,
            SG_DXFER_FROM_DEV, SG_DXFER_TO_FROM_DEV,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_directions_negative() {
        assert!(SG_DXFER_NONE < 0);
        assert!(SG_DXFER_TO_DEV < 0);
        assert!(SG_DXFER_FROM_DEV < 0);
        assert!(SG_DXFER_TO_FROM_DEV < 0);
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            SCSI_STATUS_GOOD, SCSI_STATUS_CHECK_CONDITION,
            SCSI_STATUS_CONDITION_MET, SCSI_STATUS_BUSY,
            SCSI_STATUS_RESERVATION_CONFLICT,
            SCSI_STATUS_TASK_SET_FULL,
            SCSI_STATUS_ACA_ACTIVE, SCSI_STATUS_TASK_ABORTED,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_good_is_zero() {
        assert_eq!(SCSI_STATUS_GOOD, 0);
    }

    #[test]
    fn test_sg_ioctl_distinct() {
        let cmds = [
            SG_IO, SG_GET_VERSION_NUM, SG_SET_TIMEOUT,
            SG_GET_TIMEOUT, SG_GET_COMMAND_Q, SG_SET_COMMAND_Q,
            SG_SET_RESERVED_SIZE, SG_GET_RESERVED_SIZE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
