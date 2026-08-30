//! `<scsi/scsi.h>` — SCSI protocol constants.
//!
//! SCSI (Small Computer System Interface) defines the command set
//! used by disk drives, tape drives, optical drives, and many
//! other storage devices. Even NVMe and USB mass storage devices
//! translate to/from SCSI commands internally.

// ---------------------------------------------------------------------------
// SCSI command opcodes (common subset)
// ---------------------------------------------------------------------------

/// Test Unit Ready.
pub const TEST_UNIT_READY: u8 = 0x00;
/// Request Sense.
pub const REQUEST_SENSE: u8 = 0x03;
/// Read (6).
pub const READ_6: u8 = 0x08;
/// Write (6).
pub const WRITE_6: u8 = 0x0A;
/// Inquiry.
pub const INQUIRY: u8 = 0x12;
/// Mode Select (6).
pub const MODE_SELECT: u8 = 0x15;
/// Mode Sense (6).
pub const MODE_SENSE: u8 = 0x1A;
/// Start/Stop Unit.
pub const START_STOP: u8 = 0x1B;
/// Read Capacity (10).
pub const READ_CAPACITY: u8 = 0x25;
/// Read (10).
pub const READ_10: u8 = 0x28;
/// Write (10).
pub const WRITE_10: u8 = 0x2A;
/// Synchronize Cache (10).
pub const SYNCHRONIZE_CACHE: u8 = 0x35;
/// Write Buffer.
pub const WRITE_BUFFER: u8 = 0x3B;
/// Read Buffer.
pub const READ_BUFFER: u8 = 0x3C;
/// Unmap (TRIM/discard).
pub const UNMAP: u8 = 0x42;
/// Log Sense.
pub const LOG_SENSE: u8 = 0x4D;
/// Mode Select (10).
pub const MODE_SELECT_10: u8 = 0x55;
/// Mode Sense (10).
pub const MODE_SENSE_10: u8 = 0x5A;
/// Report LUNs.
pub const REPORT_LUNS: u8 = 0xA0;
/// Read (16).
pub const READ_16: u8 = 0x88;
/// Write (16).
pub const WRITE_16: u8 = 0x8A;
/// Write Same (16).
pub const WRITE_SAME_16: u8 = 0x93;
/// Service Action In (Read Capacity 16).
pub const SERVICE_ACTION_IN_16: u8 = 0x9E;

// ---------------------------------------------------------------------------
// SCSI device types
// ---------------------------------------------------------------------------

/// Direct access (disk).
pub const TYPE_DISK: u8 = 0x00;
/// Sequential access (tape).
pub const TYPE_TAPE: u8 = 0x01;
/// Printer.
pub const TYPE_PRINTER: u8 = 0x02;
/// Processor.
pub const TYPE_PROCESSOR: u8 = 0x03;
/// Write-once (WORM).
pub const TYPE_WORM: u8 = 0x04;
/// CD/DVD-ROM.
pub const TYPE_ROM: u8 = 0x05;
/// Scanner.
pub const TYPE_SCANNER: u8 = 0x06;
/// Optical memory.
pub const TYPE_MOD: u8 = 0x07;
/// Medium changer.
pub const TYPE_MEDIUM_CHANGER: u8 = 0x08;
/// Enclosure services.
pub const TYPE_ENCLOSURE: u8 = 0x0D;
/// Simplified direct access (SD card reader).
pub const TYPE_RBC: u8 = 0x0E;
/// OSD.
pub const TYPE_OSD: u8 = 0x11;
/// ZBC (zoned block commands).
pub const TYPE_ZBC: u8 = 0x14;
/// No device type (LUN not present).
pub const TYPE_NO_LUN: u8 = 0x7F;

// ---------------------------------------------------------------------------
// SCSI sense key codes
// ---------------------------------------------------------------------------

/// No sense data.
pub const NO_SENSE: u8 = 0x00;
/// Recovered error.
pub const RECOVERED_ERROR: u8 = 0x01;
/// Not ready.
pub const NOT_READY: u8 = 0x02;
/// Medium error.
pub const MEDIUM_ERROR: u8 = 0x03;
/// Hardware error.
pub const HARDWARE_ERROR: u8 = 0x04;
/// Illegal request.
pub const ILLEGAL_REQUEST: u8 = 0x05;
/// Unit attention.
pub const UNIT_ATTENTION: u8 = 0x06;
/// Data protect.
pub const DATA_PROTECT: u8 = 0x07;
/// Blank check.
pub const BLANK_CHECK: u8 = 0x08;
/// Aborted command.
pub const ABORTED_COMMAND: u8 = 0x0B;

// ---------------------------------------------------------------------------
// CDB sizes
// ---------------------------------------------------------------------------

/// 6-byte CDB.
pub const CDB_SIZE_6: usize = 6;
/// 10-byte CDB.
pub const CDB_SIZE_10: usize = 10;
/// 12-byte CDB.
pub const CDB_SIZE_12: usize = 12;
/// 16-byte CDB.
pub const CDB_SIZE_16: usize = 16;
/// Maximum CDB size.
pub const CDB_SIZE_MAX: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_cmds_distinct() {
        let cmds = [
            TEST_UNIT_READY,
            REQUEST_SENSE,
            READ_6,
            WRITE_6,
            INQUIRY,
            MODE_SELECT,
            MODE_SENSE,
            START_STOP,
            READ_CAPACITY,
            READ_10,
            WRITE_10,
            SYNCHRONIZE_CACHE,
            WRITE_BUFFER,
            READ_BUFFER,
            UNMAP,
            LOG_SENSE,
            MODE_SELECT_10,
            MODE_SENSE_10,
            REPORT_LUNS,
            READ_16,
            WRITE_16,
            WRITE_SAME_16,
            SERVICE_ACTION_IN_16,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [
            TYPE_DISK,
            TYPE_TAPE,
            TYPE_PRINTER,
            TYPE_PROCESSOR,
            TYPE_WORM,
            TYPE_ROM,
            TYPE_SCANNER,
            TYPE_MOD,
            TYPE_MEDIUM_CHANGER,
            TYPE_ENCLOSURE,
            TYPE_RBC,
            TYPE_OSD,
            TYPE_ZBC,
            TYPE_NO_LUN,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sense_keys_distinct() {
        let keys = [
            NO_SENSE,
            RECOVERED_ERROR,
            NOT_READY,
            MEDIUM_ERROR,
            HARDWARE_ERROR,
            ILLEGAL_REQUEST,
            UNIT_ATTENTION,
            DATA_PROTECT,
            BLANK_CHECK,
            ABORTED_COMMAND,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_cdb_sizes() {
        let sizes = [CDB_SIZE_6, CDB_SIZE_10, CDB_SIZE_12, CDB_SIZE_16];
        for i in 0..sizes.len() {
            assert!(sizes[i] <= CDB_SIZE_MAX);
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }
}
