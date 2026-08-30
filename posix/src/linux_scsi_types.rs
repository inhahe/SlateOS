//! `<scsi/scsi.h>` — SCSI command and status constants.
//!
//! SCSI (Small Computer Systems Interface) defines a standard
//! command set for storage devices, scanners, tape drives, and
//! other peripherals. While originally for parallel buses, SCSI
//! commands are now transported over SAS, iSCSI, FC, USB, and NVMe.

// ---------------------------------------------------------------------------
// SCSI command opcodes (common)
// ---------------------------------------------------------------------------

/// Test Unit Ready.
pub const SCSI_CMD_TEST_UNIT_READY: u8 = 0x00;
/// Request Sense.
pub const SCSI_CMD_REQUEST_SENSE: u8 = 0x03;
/// Inquiry.
pub const SCSI_CMD_INQUIRY: u8 = 0x12;
/// Mode Select (6).
pub const SCSI_CMD_MODE_SELECT_6: u8 = 0x15;
/// Mode Sense (6).
pub const SCSI_CMD_MODE_SENSE_6: u8 = 0x1A;
/// Read Capacity (10).
pub const SCSI_CMD_READ_CAPACITY_10: u8 = 0x25;
/// Read (10).
pub const SCSI_CMD_READ_10: u8 = 0x28;
/// Write (10).
pub const SCSI_CMD_WRITE_10: u8 = 0x2A;
/// Synchronize Cache (10).
pub const SCSI_CMD_SYNC_CACHE_10: u8 = 0x35;
/// Read (16).
pub const SCSI_CMD_READ_16: u8 = 0x88;
/// Write (16).
pub const SCSI_CMD_WRITE_16: u8 = 0x8A;
/// Read Capacity (16).
pub const SCSI_CMD_READ_CAPACITY_16: u8 = 0x9E;
/// Unmap (TRIM/discard).
pub const SCSI_CMD_UNMAP: u8 = 0x42;
/// Report LUNs.
pub const SCSI_CMD_REPORT_LUNS: u8 = 0xA0;

// ---------------------------------------------------------------------------
// SCSI status codes
// ---------------------------------------------------------------------------

/// Good (command completed successfully).
pub const SCSI_STATUS_GOOD: u8 = 0x00;
/// Check Condition (sense data available).
pub const SCSI_STATUS_CHECK_CONDITION: u8 = 0x02;
/// Condition Met.
pub const SCSI_STATUS_CONDITION_MET: u8 = 0x04;
/// Busy.
pub const SCSI_STATUS_BUSY: u8 = 0x08;
/// Reservation Conflict.
pub const SCSI_STATUS_RESERVATION_CONFLICT: u8 = 0x18;
/// Task Set Full.
pub const SCSI_STATUS_TASK_SET_FULL: u8 = 0x28;
/// ACA Active.
pub const SCSI_STATUS_ACA_ACTIVE: u8 = 0x30;
/// Task Aborted.
pub const SCSI_STATUS_TASK_ABORTED: u8 = 0x40;

// ---------------------------------------------------------------------------
// SCSI sense keys
// ---------------------------------------------------------------------------

/// No Sense.
pub const SCSI_SENSE_NO_SENSE: u8 = 0x0;
/// Recovered Error.
pub const SCSI_SENSE_RECOVERED_ERROR: u8 = 0x1;
/// Not Ready.
pub const SCSI_SENSE_NOT_READY: u8 = 0x2;
/// Medium Error.
pub const SCSI_SENSE_MEDIUM_ERROR: u8 = 0x3;
/// Hardware Error.
pub const SCSI_SENSE_HARDWARE_ERROR: u8 = 0x4;
/// Illegal Request.
pub const SCSI_SENSE_ILLEGAL_REQUEST: u8 = 0x5;
/// Unit Attention.
pub const SCSI_SENSE_UNIT_ATTENTION: u8 = 0x6;
/// Data Protect.
pub const SCSI_SENSE_DATA_PROTECT: u8 = 0x7;
/// Aborted Command.
pub const SCSI_SENSE_ABORTED_COMMAND: u8 = 0xB;

// ---------------------------------------------------------------------------
// SCSI device types
// ---------------------------------------------------------------------------

/// Direct-access (disk).
pub const SCSI_TYPE_DISK: u8 = 0x00;
/// Sequential (tape).
pub const SCSI_TYPE_TAPE: u8 = 0x01;
/// CD/DVD-ROM.
pub const SCSI_TYPE_ROM: u8 = 0x05;
/// Enclosure services.
pub const SCSI_TYPE_ENCLOSURE: u8 = 0x0D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            SCSI_CMD_TEST_UNIT_READY,
            SCSI_CMD_REQUEST_SENSE,
            SCSI_CMD_INQUIRY,
            SCSI_CMD_MODE_SELECT_6,
            SCSI_CMD_MODE_SENSE_6,
            SCSI_CMD_READ_CAPACITY_10,
            SCSI_CMD_READ_10,
            SCSI_CMD_WRITE_10,
            SCSI_CMD_SYNC_CACHE_10,
            SCSI_CMD_READ_16,
            SCSI_CMD_WRITE_16,
            SCSI_CMD_READ_CAPACITY_16,
            SCSI_CMD_UNMAP,
            SCSI_CMD_REPORT_LUNS,
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
            SCSI_STATUS_GOOD,
            SCSI_STATUS_CHECK_CONDITION,
            SCSI_STATUS_CONDITION_MET,
            SCSI_STATUS_BUSY,
            SCSI_STATUS_RESERVATION_CONFLICT,
            SCSI_STATUS_TASK_SET_FULL,
            SCSI_STATUS_ACA_ACTIVE,
            SCSI_STATUS_TASK_ABORTED,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_sense_keys_distinct() {
        let keys = [
            SCSI_SENSE_NO_SENSE,
            SCSI_SENSE_RECOVERED_ERROR,
            SCSI_SENSE_NOT_READY,
            SCSI_SENSE_MEDIUM_ERROR,
            SCSI_SENSE_HARDWARE_ERROR,
            SCSI_SENSE_ILLEGAL_REQUEST,
            SCSI_SENSE_UNIT_ATTENTION,
            SCSI_SENSE_DATA_PROTECT,
            SCSI_SENSE_ABORTED_COMMAND,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [
            SCSI_TYPE_DISK,
            SCSI_TYPE_TAPE,
            SCSI_TYPE_ROM,
            SCSI_TYPE_ENCLOSURE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
