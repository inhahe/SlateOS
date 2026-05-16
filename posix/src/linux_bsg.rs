//! `<linux/bsg.h>` — Block SCSI Generic constants.
//!
//! BSG provides a generic interface for sending SCSI commands
//! to devices through the block layer. It supersedes the older
//! sg (SCSI Generic) interface with better support for modern
//! transports (SAS, FC, iSCSI) and queue-based I/O.

// ---------------------------------------------------------------------------
// BSG protocol types
// ---------------------------------------------------------------------------

/// SCSI protocol.
pub const BSG_PROTOCOL_SCSI: u32 = 0;

// ---------------------------------------------------------------------------
// BSG sub-protocol types
// ---------------------------------------------------------------------------

/// Transport-layer sub-protocol.
pub const BSG_SUB_PROTOCOL_SCSI_TRANSPORT: u32 = 2;

// ---------------------------------------------------------------------------
// BSG flags
// ---------------------------------------------------------------------------

/// Queue at head (high priority).
pub const BSG_FLAG_Q_AT_HEAD: u32 = 1 << 0;
/// Queue at tail (normal priority).
pub const BSG_FLAG_Q_AT_TAIL: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// SG_IO directions (shared with sg.h)
// ---------------------------------------------------------------------------

/// No data transfer.
pub const SG_DXFER_NONE: i32 = -1;
/// Data to device (write).
pub const SG_DXFER_TO_DEV: i32 = -2;
/// Data from device (read).
pub const SG_DXFER_FROM_DEV: i32 = -3;
/// Bidirectional data transfer.
pub const SG_DXFER_TO_FROM_DEV: i32 = -4;

// ---------------------------------------------------------------------------
// SG_IO version
// ---------------------------------------------------------------------------

/// SG_IO v3 interface.
pub const SG_IO_V3: u32 = b'S' as u32;
/// SG_IO v4 interface.
pub const SG_IO_V4: u32 = b'Q' as u32;

// ---------------------------------------------------------------------------
// Status codes (from SCSI)
// ---------------------------------------------------------------------------

/// Good status.
pub const SAM_STAT_GOOD: u8 = 0x00;
/// Check condition.
pub const SAM_STAT_CHECK_CONDITION: u8 = 0x02;
/// Condition met.
pub const SAM_STAT_CONDITION_MET: u8 = 0x04;
/// Busy.
pub const SAM_STAT_BUSY: u8 = 0x08;
/// Reservation conflict.
pub const SAM_STAT_RESERVATION_CONFLICT: u8 = 0x18;
/// Task set full.
pub const SAM_STAT_TASK_SET_FULL: u8 = 0x28;
/// ACA active.
pub const SAM_STAT_ACA_ACTIVE: u8 = 0x30;
/// Task aborted.
pub const SAM_STAT_TASK_ABORTED: u8 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol() {
        assert_eq!(BSG_PROTOCOL_SCSI, 0);
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(BSG_FLAG_Q_AT_HEAD & BSG_FLAG_Q_AT_TAIL, 0);
    }

    #[test]
    fn test_dxfer_directions_distinct() {
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
    fn test_sg_io_versions() {
        assert_ne!(SG_IO_V3, SG_IO_V4);
    }

    #[test]
    fn test_sam_status_distinct() {
        let statuses = [
            SAM_STAT_GOOD, SAM_STAT_CHECK_CONDITION,
            SAM_STAT_CONDITION_MET, SAM_STAT_BUSY,
            SAM_STAT_RESERVATION_CONFLICT, SAM_STAT_TASK_SET_FULL,
            SAM_STAT_ACA_ACTIVE, SAM_STAT_TASK_ABORTED,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_sam_good() {
        assert_eq!(SAM_STAT_GOOD, 0);
    }
}
