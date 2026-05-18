//! `<scsi/scsi_proto.h>` (opcode subset) — SCSI command operation codes.
//!
//! SCSI commands are identified by operation codes (opcodes) that
//! specify the action to perform (read, write, inquiry, etc.). These
//! are standardised by T10 (SPC, SBC, SSC specifications). The Linux
//! SCSI layer uses these when building CDBs (Command Descriptor
//! Blocks) for storage devices.

// ---------------------------------------------------------------------------
// Common SCSI opcodes
// ---------------------------------------------------------------------------

/// TEST UNIT READY: check if device is ready.
pub const TEST_UNIT_READY: u8 = 0x00;
/// REQUEST SENSE: retrieve sense data after error.
pub const REQUEST_SENSE: u8 = 0x03;
/// INQUIRY: identify device type and capabilities.
pub const INQUIRY: u8 = 0x12;
/// MODE SELECT (6): set device parameters.
pub const MODE_SELECT: u8 = 0x15;
/// MODE SENSE (6): read device parameters.
pub const MODE_SENSE: u8 = 0x1A;
/// START STOP UNIT: spin up/down or eject media.
pub const START_STOP: u8 = 0x1B;
/// READ CAPACITY (10): get device size.
pub const READ_CAPACITY: u8 = 0x25;
/// READ (10): read data blocks.
pub const READ_10: u8 = 0x28;
/// WRITE (10): write data blocks.
pub const WRITE_10: u8 = 0x2A;
/// SYNCHRONIZE CACHE (10): flush write cache.
pub const SYNCHRONIZE_CACHE: u8 = 0x35;
/// READ (6): read data (short CDB).
pub const READ_6: u8 = 0x08;
/// WRITE (6): write data (short CDB).
pub const WRITE_6: u8 = 0x0A;
/// REPORT LUNS: list logical units.
pub const REPORT_LUNS: u8 = 0xA0;
/// READ (16): read data (long LBA).
pub const READ_16: u8 = 0x88;
/// WRITE (16): write data (long LBA).
pub const WRITE_16: u8 = 0x8A;
/// SERVICE ACTION IN (16): extended commands (e.g. READ CAPACITY 16).
pub const SERVICE_ACTION_IN_16: u8 = 0x9E;
/// UNMAP: trim / discard blocks.
pub const UNMAP: u8 = 0x42;
/// WRITE SAME (16): write pattern to blocks.
pub const WRITE_SAME_16: u8 = 0x93;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            TEST_UNIT_READY, REQUEST_SENSE, INQUIRY,
            MODE_SELECT, MODE_SENSE, START_STOP,
            READ_CAPACITY, READ_10, WRITE_10,
            SYNCHRONIZE_CACHE, READ_6, WRITE_6,
            REPORT_LUNS, READ_16, WRITE_16,
            SERVICE_ACTION_IN_16, UNMAP, WRITE_SAME_16,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j],
                    "opcodes {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_common_values() {
        assert_eq!(TEST_UNIT_READY, 0x00);
        assert_eq!(INQUIRY, 0x12);
        assert_eq!(READ_10, 0x28);
        assert_eq!(WRITE_10, 0x2A);
    }

    #[test]
    fn test_read_write_pairs() {
        assert_ne!(READ_6, WRITE_6);
        assert_ne!(READ_10, WRITE_10);
        assert_ne!(READ_16, WRITE_16);
    }
}
