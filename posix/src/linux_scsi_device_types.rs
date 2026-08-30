//! `<scsi/scsi_proto.h>` (device type subset) — SCSI peripheral device types.
//!
//! The INQUIRY response reports the peripheral device type, which
//! tells the initiator what kind of device it's talking to (disk,
//! tape, CD-ROM, etc.). The Linux SCSI layer uses this to select
//! the correct upper-level driver (sd, st, sr, etc.).

// ---------------------------------------------------------------------------
// SCSI device types (from INQUIRY byte 0)
// ---------------------------------------------------------------------------

/// Direct-access block device (disk, SSD).
pub const TYPE_DISK: u8 = 0x00;
/// Sequential-access device (tape).
pub const TYPE_TAPE: u8 = 0x01;
/// Printer.
pub const TYPE_PRINTER: u8 = 0x02;
/// Processor (e.g., SES/SAF-TE enclosure services).
pub const TYPE_PROCESSOR: u8 = 0x03;
/// Write-once device (WORM).
pub const TYPE_WORM: u8 = 0x04;
/// CD/DVD-ROM.
pub const TYPE_ROM: u8 = 0x05;
/// Scanner.
pub const TYPE_SCANNER: u8 = 0x06;
/// Optical memory device (MO disc).
pub const TYPE_MOD: u8 = 0x07;
/// Medium changer (tape library robot).
pub const TYPE_MEDIUM_CHANGER: u8 = 0x08;
/// Storage array controller.
pub const TYPE_RAID: u8 = 0x0C;
/// Enclosure services device (SES).
pub const TYPE_ENCLOSURE: u8 = 0x0D;
/// Simplified direct-access (e.g., USB flash).
pub const TYPE_RBC: u8 = 0x0E;
/// Optical card reader/writer.
pub const TYPE_OCRW: u8 = 0x0F;
/// Bridge controller.
pub const TYPE_OSD: u8 = 0x11;
/// Automation/drive interface.
pub const TYPE_ZBC: u8 = 0x14;
/// Well-known logical unit.
pub const TYPE_WLUN: u8 = 0x1E;
/// No device type (LUN not present).
pub const TYPE_NO_LUN: u8 = 0x7F;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
            TYPE_RAID,
            TYPE_ENCLOSURE,
            TYPE_RBC,
            TYPE_OCRW,
            TYPE_OSD,
            TYPE_ZBC,
            TYPE_WLUN,
            TYPE_NO_LUN,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_common_types() {
        assert_eq!(TYPE_DISK, 0x00);
        assert_eq!(TYPE_TAPE, 0x01);
        assert_eq!(TYPE_ROM, 0x05);
    }

    #[test]
    fn test_no_lun_value() {
        assert_eq!(TYPE_NO_LUN, 0x7F);
    }

    #[test]
    fn test_types_fit_7_bits() {
        // Device type is 5 bits in INQUIRY, but TYPE_NO_LUN uses 7 bits
        // as a special sentinel
        let standard_types = [
            TYPE_DISK,
            TYPE_TAPE,
            TYPE_PRINTER,
            TYPE_PROCESSOR,
            TYPE_WORM,
            TYPE_ROM,
            TYPE_SCANNER,
            TYPE_MOD,
            TYPE_MEDIUM_CHANGER,
            TYPE_RAID,
            TYPE_ENCLOSURE,
            TYPE_RBC,
            TYPE_OCRW,
            TYPE_OSD,
            TYPE_ZBC,
        ];
        for &t in &standard_types {
            assert!(t < 0x20, "type 0x{:02X} doesn't fit in 5 bits", t);
        }
    }
}
