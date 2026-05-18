//! `<linux/cdrom.h>` — SCSI CD-ROM (sr) driver constants.
//!
//! SCSI CD-ROM driver constants covering read/write modes,
//! error codes, subchannel types, and address format flags.

// ---------------------------------------------------------------------------
// CD-ROM address format flags
// ---------------------------------------------------------------------------

/// LBA addressing.
pub const CDROM_LBA: u8 = 0x01;
/// MSF addressing.
pub const CDROM_MSF: u8 = 0x02;

// ---------------------------------------------------------------------------
// CD-ROM subchannel data format
// ---------------------------------------------------------------------------

/// Current position.
pub const CDROM_SUBCHNL_CURRENT_POS: u8 = 0x01;
/// Media catalog number.
pub const CDROM_SUBCHNL_MEDIA_CATALOG: u8 = 0x02;
/// Track ISRC.
pub const CDROM_SUBCHNL_TRACK_ISRC: u8 = 0x03;

// ---------------------------------------------------------------------------
// CD-ROM audio status
// ---------------------------------------------------------------------------

/// Invalid status.
pub const CDROM_AUDIO_INVALID: u8 = 0x00;
/// Audio playing.
pub const CDROM_AUDIO_PLAY: u8 = 0x11;
/// Audio paused.
pub const CDROM_AUDIO_PAUSED: u8 = 0x12;
/// Audio completed.
pub const CDROM_AUDIO_COMPLETED: u8 = 0x13;
/// Audio error.
pub const CDROM_AUDIO_ERROR: u8 = 0x14;
/// No status.
pub const CDROM_AUDIO_NO_STATUS: u8 = 0x15;

// ---------------------------------------------------------------------------
// CD-ROM sector sizes
// ---------------------------------------------------------------------------

/// Cooked data sector (mode 1).
pub const CD_FRAMESIZE: u32 = 2048;
/// Raw data sector.
pub const CD_FRAMESIZE_RAW: u32 = 2352;
/// Mode 2 form 1.
pub const CD_FRAMESIZE_RAW0: u32 = 2336;
/// Sub-channel data size.
pub const CD_FRAMESIZE_SUB: u32 = 96;
/// Sector header size.
pub const CD_HEAD_SIZE: u32 = 4;
/// Sync field size.
pub const CD_SYNC_SIZE: u32 = 12;
/// EDC/ECC size.
pub const CD_EDC_SIZE: u32 = 4;
/// Zero field in mode 2.
pub const CD_ZERO_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// CD-ROM timing constants
// ---------------------------------------------------------------------------

/// Frames per second.
pub const CD_FRAMES: u32 = 75;
/// Seconds per minute.
pub const CD_SECS: u32 = 60;
/// MSF offset (2 seconds).
pub const CD_MSF_OFFSET: u32 = 150;
/// Maximum minutes addressable.
pub const CD_MINS: u32 = 74;

// ---------------------------------------------------------------------------
// Lead-in/lead-out constants
// ---------------------------------------------------------------------------

/// Lead-out track number.
pub const CDROM_LEADOUT: u8 = 0xAA;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_formats() {
        assert_eq!(CDROM_LBA, 0x01);
        assert_eq!(CDROM_MSF, 0x02);
        assert_ne!(CDROM_LBA, CDROM_MSF);
    }

    #[test]
    fn test_subchannel_types_distinct() {
        let types: [u8; 3] = [
            CDROM_SUBCHNL_CURRENT_POS,
            CDROM_SUBCHNL_MEDIA_CATALOG,
            CDROM_SUBCHNL_TRACK_ISRC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_audio_status_distinct() {
        let statuses: [u8; 6] = [
            CDROM_AUDIO_INVALID, CDROM_AUDIO_PLAY,
            CDROM_AUDIO_PAUSED, CDROM_AUDIO_COMPLETED,
            CDROM_AUDIO_ERROR, CDROM_AUDIO_NO_STATUS,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_sector_sizes() {
        assert_eq!(CD_FRAMESIZE, 2048);
        assert_eq!(CD_FRAMESIZE_RAW, 2352);
        assert_eq!(CD_FRAMESIZE_RAW0, 2336);
        assert!(CD_FRAMESIZE < CD_FRAMESIZE_RAW);
    }

    #[test]
    fn test_timing() {
        assert_eq!(CD_FRAMES, 75);
        assert_eq!(CD_SECS, 60);
        assert_eq!(CD_MSF_OFFSET, 150);
        assert_eq!(CD_MSF_OFFSET, CD_FRAMES * 2);
    }

    #[test]
    fn test_leadout() {
        assert_eq!(CDROM_LEADOUT, 0xAA);
    }
}
