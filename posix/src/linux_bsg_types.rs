//! `<linux/bsg.h>` — block SCSI generic (BSG) ioctl constants.
//!
//! BSG provides passthrough access to SCSI commands on block devices,
//! replacing the older SG_IO interface. It allows userspace tools
//! (sg_utils, smartmontools) to send SCSI CDBs directly to storage
//! devices for diagnostics, firmware updates, and management.

// ---------------------------------------------------------------------------
// BSG protocol types
// ---------------------------------------------------------------------------

/// SCSI protocol.
pub const BSG_PROTOCOL_SCSI: u32 = 0;

// ---------------------------------------------------------------------------
// BSG sub-protocol types
// ---------------------------------------------------------------------------

/// SCSI command (CDB).
pub const BSG_SUB_PROTOCOL_SCSI_CMD: u32 = 0;
/// SCSI transport management function.
pub const BSG_SUB_PROTOCOL_SCSI_TMF: u32 = 1;
/// SCSI transport (SMP for SAS).
pub const BSG_SUB_PROTOCOL_SCSI_TRANSPORT: u32 = 2;

// ---------------------------------------------------------------------------
// BSG flags
// ---------------------------------------------------------------------------

/// Queue at head (high priority).
pub const BSG_FLAG_Q_AT_HEAD: u32 = 0x01;
/// Queue at tail (normal priority).
pub const BSG_FLAG_Q_AT_TAIL: u32 = 0x10;

// ---------------------------------------------------------------------------
// SG_IO direction constants (used with BSG)
// ---------------------------------------------------------------------------

/// No data transfer.
pub const SG_DXFER_NONE: i32 = -1;
/// Data from device to user (read).
pub const SG_DXFER_TO_DEV: i32 = -2;
/// Data from user to device (write).
pub const SG_DXFER_FROM_DEV: i32 = -3;
/// Bidirectional data transfer.
pub const SG_DXFER_TO_FROM_DEV: i32 = -4;

// ---------------------------------------------------------------------------
// SG_IO status masks
// ---------------------------------------------------------------------------

/// SCSI status byte mask.
pub const SG_INFO_OK_MASK: u32 = 0x01;
/// Check condition (sense data available).
pub const SG_INFO_CHECK: u32 = 0x01;
/// Direct I/O used.
pub const SG_INFO_DIRECT_IO_MASK: u32 = 0x06;
/// Indirect I/O (buffered).
pub const SG_INFO_INDIRECT_IO: u32 = 0x00;
/// Direct I/O.
pub const SG_INFO_DIRECT_IO: u32 = 0x02;
/// Mixed I/O.
pub const SG_INFO_MIXED_IO: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_protocols_distinct() {
        let subs = [
            BSG_SUB_PROTOCOL_SCSI_CMD,
            BSG_SUB_PROTOCOL_SCSI_TMF,
            BSG_SUB_PROTOCOL_SCSI_TRANSPORT,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_bsg_flags() {
        assert_ne!(BSG_FLAG_Q_AT_HEAD, BSG_FLAG_Q_AT_TAIL);
        assert_eq!(BSG_FLAG_Q_AT_HEAD & BSG_FLAG_Q_AT_TAIL, 0);
    }

    #[test]
    fn test_sg_directions_distinct() {
        let dirs = [
            SG_DXFER_NONE,
            SG_DXFER_TO_DEV,
            SG_DXFER_FROM_DEV,
            SG_DXFER_TO_FROM_DEV,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_sg_directions_negative() {
        assert!(SG_DXFER_NONE < 0);
        assert!(SG_DXFER_TO_DEV < 0);
        assert!(SG_DXFER_FROM_DEV < 0);
        assert!(SG_DXFER_TO_FROM_DEV < 0);
    }

    #[test]
    fn test_protocol_value() {
        assert_eq!(BSG_PROTOCOL_SCSI, 0);
    }
}
