//! `<scsi/scsi.h>` — SCSI constants (extended).
//!
//! Extended SCSI constants covering command opcodes, status
//! codes, sense key codes, device types, and message types.

// ---------------------------------------------------------------------------
// SCSI command opcodes
// ---------------------------------------------------------------------------

/// Test unit ready.
pub const SCSI_CMD_TEST_UNIT_READY: u8 = 0x00;
/// Request sense.
pub const SCSI_CMD_REQUEST_SENSE: u8 = 0x03;
/// Read(6).
pub const SCSI_CMD_READ_6: u8 = 0x08;
/// Write(6).
pub const SCSI_CMD_WRITE_6: u8 = 0x0A;
/// Inquiry.
pub const SCSI_CMD_INQUIRY: u8 = 0x12;
/// Mode select(6).
pub const SCSI_CMD_MODE_SELECT_6: u8 = 0x15;
/// Mode sense(6).
pub const SCSI_CMD_MODE_SENSE_6: u8 = 0x1A;
/// Start/stop unit.
pub const SCSI_CMD_START_STOP: u8 = 0x1B;
/// Read capacity(10).
pub const SCSI_CMD_READ_CAPACITY_10: u8 = 0x25;
/// Read(10).
pub const SCSI_CMD_READ_10: u8 = 0x28;
/// Write(10).
pub const SCSI_CMD_WRITE_10: u8 = 0x2A;
/// Verify(10).
pub const SCSI_CMD_VERIFY_10: u8 = 0x2F;
/// Synchronize cache(10).
pub const SCSI_CMD_SYNC_CACHE_10: u8 = 0x35;
/// Read(16).
pub const SCSI_CMD_READ_16: u8 = 0x88;
/// Write(16).
pub const SCSI_CMD_WRITE_16: u8 = 0x8A;
/// Verify(16).
pub const SCSI_CMD_VERIFY_16: u8 = 0x8F;
/// Synchronize cache(16).
pub const SCSI_CMD_SYNC_CACHE_16: u8 = 0x91;
/// Read capacity(16) / service action in.
pub const SCSI_CMD_SERVICE_ACTION_IN_16: u8 = 0x9E;
/// Report LUNs.
pub const SCSI_CMD_REPORT_LUNS: u8 = 0xA0;

// ---------------------------------------------------------------------------
// SCSI status codes
// ---------------------------------------------------------------------------

/// Good (command completed successfully).
pub const SCSI_STATUS_GOOD: u8 = 0x00;
/// Check condition (error — read sense data).
pub const SCSI_STATUS_CHECK_CONDITION: u8 = 0x02;
/// Condition met.
pub const SCSI_STATUS_CONDITION_MET: u8 = 0x04;
/// Busy.
pub const SCSI_STATUS_BUSY: u8 = 0x08;
/// Intermediate.
pub const SCSI_STATUS_INTERMEDIATE: u8 = 0x10;
/// Intermediate condition met.
pub const SCSI_STATUS_INTERMEDIATE_COND_MET: u8 = 0x14;
/// Reservation conflict.
pub const SCSI_STATUS_RESERVATION_CONFLICT: u8 = 0x18;
/// Command terminated (obsolete).
pub const SCSI_STATUS_COMMAND_TERMINATED: u8 = 0x22;
/// Task set full.
pub const SCSI_STATUS_TASK_SET_FULL: u8 = 0x28;
/// ACA active.
pub const SCSI_STATUS_ACA_ACTIVE: u8 = 0x30;
/// Task aborted.
pub const SCSI_STATUS_TASK_ABORTED: u8 = 0x40;

// ---------------------------------------------------------------------------
// SCSI sense keys
// ---------------------------------------------------------------------------

/// No sense data.
pub const SCSI_SENSE_NO_SENSE: u8 = 0x0;
/// Recovered error.
pub const SCSI_SENSE_RECOVERED_ERROR: u8 = 0x1;
/// Not ready.
pub const SCSI_SENSE_NOT_READY: u8 = 0x2;
/// Medium error.
pub const SCSI_SENSE_MEDIUM_ERROR: u8 = 0x3;
/// Hardware error.
pub const SCSI_SENSE_HARDWARE_ERROR: u8 = 0x4;
/// Illegal request.
pub const SCSI_SENSE_ILLEGAL_REQUEST: u8 = 0x5;
/// Unit attention.
pub const SCSI_SENSE_UNIT_ATTENTION: u8 = 0x6;
/// Data protect.
pub const SCSI_SENSE_DATA_PROTECT: u8 = 0x7;
/// Blank check.
pub const SCSI_SENSE_BLANK_CHECK: u8 = 0x8;
/// Vendor specific.
pub const SCSI_SENSE_VENDOR_SPECIFIC: u8 = 0x9;
/// Copy aborted.
pub const SCSI_SENSE_COPY_ABORTED: u8 = 0xA;
/// Aborted command.
pub const SCSI_SENSE_ABORTED_COMMAND: u8 = 0xB;
/// Volume overflow.
pub const SCSI_SENSE_VOLUME_OVERFLOW: u8 = 0xD;
/// Miscompare.
pub const SCSI_SENSE_MISCOMPARE: u8 = 0xE;
/// Completed.
pub const SCSI_SENSE_COMPLETED: u8 = 0xF;

// ---------------------------------------------------------------------------
// SCSI device types
// ---------------------------------------------------------------------------

/// Direct access (disk).
pub const SCSI_TYPE_DISK: u8 = 0x00;
/// Sequential access (tape).
pub const SCSI_TYPE_TAPE: u8 = 0x01;
/// Printer.
pub const SCSI_TYPE_PRINTER: u8 = 0x02;
/// Processor.
pub const SCSI_TYPE_PROCESSOR: u8 = 0x03;
/// Write-once (WORM).
pub const SCSI_TYPE_WORM: u8 = 0x04;
/// CD-ROM.
pub const SCSI_TYPE_ROM: u8 = 0x05;
/// Scanner.
pub const SCSI_TYPE_SCANNER: u8 = 0x06;
/// Optical memory.
pub const SCSI_TYPE_MOD: u8 = 0x07;
/// Medium changer.
pub const SCSI_TYPE_MEDIUM_CHANGER: u8 = 0x08;
/// Enclosure services.
pub const SCSI_TYPE_ENCLOSURE: u8 = 0x0D;
/// Simplified direct access.
pub const SCSI_TYPE_RBC: u8 = 0x0E;
/// No device type (0x1F).
pub const SCSI_TYPE_NO_LUN: u8 = 0x7F;

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
            SCSI_CMD_READ_6,
            SCSI_CMD_WRITE_6,
            SCSI_CMD_INQUIRY,
            SCSI_CMD_MODE_SELECT_6,
            SCSI_CMD_MODE_SENSE_6,
            SCSI_CMD_START_STOP,
            SCSI_CMD_READ_CAPACITY_10,
            SCSI_CMD_READ_10,
            SCSI_CMD_WRITE_10,
            SCSI_CMD_VERIFY_10,
            SCSI_CMD_SYNC_CACHE_10,
            SCSI_CMD_READ_16,
            SCSI_CMD_WRITE_16,
            SCSI_CMD_VERIFY_16,
            SCSI_CMD_SYNC_CACHE_16,
            SCSI_CMD_SERVICE_ACTION_IN_16,
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
            SCSI_STATUS_INTERMEDIATE,
            SCSI_STATUS_INTERMEDIATE_COND_MET,
            SCSI_STATUS_RESERVATION_CONFLICT,
            SCSI_STATUS_COMMAND_TERMINATED,
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
            SCSI_SENSE_BLANK_CHECK,
            SCSI_SENSE_VENDOR_SPECIFIC,
            SCSI_SENSE_COPY_ABORTED,
            SCSI_SENSE_ABORTED_COMMAND,
            SCSI_SENSE_VOLUME_OVERFLOW,
            SCSI_SENSE_MISCOMPARE,
            SCSI_SENSE_COMPLETED,
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
            SCSI_TYPE_PRINTER,
            SCSI_TYPE_PROCESSOR,
            SCSI_TYPE_WORM,
            SCSI_TYPE_ROM,
            SCSI_TYPE_SCANNER,
            SCSI_TYPE_MOD,
            SCSI_TYPE_MEDIUM_CHANGER,
            SCSI_TYPE_ENCLOSURE,
            SCSI_TYPE_RBC,
            SCSI_TYPE_NO_LUN,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_good_is_zero() {
        assert_eq!(SCSI_STATUS_GOOD, 0);
    }

    #[test]
    fn test_no_sense_is_zero() {
        assert_eq!(SCSI_SENSE_NO_SENSE, 0);
    }

    #[test]
    fn test_disk_type() {
        assert_eq!(SCSI_TYPE_DISK, 0);
    }

    #[test]
    fn test_inquiry_opcode() {
        assert_eq!(SCSI_CMD_INQUIRY, 0x12);
    }
}
