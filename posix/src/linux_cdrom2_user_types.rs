//! `<linux/cdrom.h>` (part 2) — CD-ROM control ioctls.
//!
//! The CDROM driver exposes a uniform ioctl surface across SCSI/ATAPI
//! optical drives. This module covers the disc-state ioctls and the
//! tray/lock controls, complementing the audio-playback subset in the
//! base module.

// ---------------------------------------------------------------------------
// CDROM control ioctls (type 0x53 = 'S')
// ---------------------------------------------------------------------------

pub const CDROMPAUSE: u32 = 0x5301;
pub const CDROMRESUME: u32 = 0x5302;
pub const CDROMPLAYMSF: u32 = 0x5303;
pub const CDROMPLAYTRKIND: u32 = 0x5304;
pub const CDROMREADTOCHDR: u32 = 0x5305;
pub const CDROMREADTOCENTRY: u32 = 0x5306;
pub const CDROMSTOP: u32 = 0x5307;
pub const CDROMSTART: u32 = 0x5308;
pub const CDROMEJECT: u32 = 0x5309;
pub const CDROMVOLCTRL: u32 = 0x530A;
pub const CDROMSUBCHNL: u32 = 0x530B;
pub const CDROMREADMODE2: u32 = 0x530C;
pub const CDROMREADMODE1: u32 = 0x530D;
pub const CDROMREADAUDIO: u32 = 0x530E;
pub const CDROMEJECT_SW: u32 = 0x530F;

// ---------------------------------------------------------------------------
// Tray / disc status (`CDROM_DRIVE_STATUS` return)
// ---------------------------------------------------------------------------

pub const CDS_NO_INFO: u32 = 0;
pub const CDS_NO_DISC: u32 = 1;
pub const CDS_TRAY_OPEN: u32 = 2;
pub const CDS_DRIVE_NOT_READY: u32 = 3;
pub const CDS_DISC_OK: u32 = 4;

// ---------------------------------------------------------------------------
// Disc-type (`CDROM_DISC_STATUS` return)
// ---------------------------------------------------------------------------

pub const CDS_AUDIO: u32 = 100;
pub const CDS_DATA_1: u32 = 101;
pub const CDS_DATA_2: u32 = 102;
pub const CDS_XA_2_1: u32 = 103;
pub const CDS_XA_2_2: u32 = 104;
pub const CDS_MIXED: u32 = 105;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_ioctls_in_s_family() {
        for v in [
            CDROMPAUSE, CDROMRESUME, CDROMPLAYMSF, CDROMPLAYTRKIND, CDROMREADTOCHDR,
            CDROMREADTOCENTRY, CDROMSTOP, CDROMSTART, CDROMEJECT, CDROMVOLCTRL,
            CDROMSUBCHNL, CDROMREADMODE2, CDROMREADMODE1, CDROMREADAUDIO, CDROMEJECT_SW,
        ] {
            // High byte of u16 ioctl number is the 'S' type (0x53).
            assert_eq!((v >> 8) & 0xFF, 0x53);
        }
    }

    #[test]
    fn test_ioctl_numbers_dense_0x01_to_0x0f() {
        let o = [
            CDROMPAUSE, CDROMRESUME, CDROMPLAYMSF, CDROMPLAYTRKIND, CDROMREADTOCHDR,
            CDROMREADTOCENTRY, CDROMSTOP, CDROMSTART, CDROMEJECT, CDROMVOLCTRL,
            CDROMSUBCHNL, CDROMREADMODE2, CDROMREADMODE1, CDROMREADAUDIO, CDROMEJECT_SW,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v & 0xFF, (i + 1) as u32);
        }
    }

    #[test]
    fn test_drive_status_dense_0_to_4() {
        let s = [
            CDS_NO_INFO,
            CDS_NO_DISC,
            CDS_TRAY_OPEN,
            CDS_DRIVE_NOT_READY,
            CDS_DISC_OK,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // NO_INFO is the "unknown" state — value 0.
        assert_eq!(CDS_NO_INFO, 0);
    }

    #[test]
    fn test_disc_types_dense_100_to_105() {
        let t = [
            CDS_AUDIO,
            CDS_DATA_1,
            CDS_DATA_2,
            CDS_XA_2_1,
            CDS_XA_2_2,
            CDS_MIXED,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, 100 + i);
        }
    }

    #[test]
    fn test_disc_types_disjoint_from_drive_status() {
        // Drive-status values are in [0..4]; disc-types are in [100..105].
        // No overlap is critical because the same field carries either.
        assert!(CDS_DISC_OK < CDS_AUDIO);
    }

    #[test]
    fn test_play_and_read_ioctls_well_known_numbers() {
        // CDROMPLAYMSF is 0x5303, the well-known MSF audio-play command.
        assert_eq!(CDROMPLAYMSF, 0x5303);
        // CDROMREADAUDIO is 0x530E — the digital-audio extraction op.
        assert_eq!(CDROMREADAUDIO, 0x530E);
    }
}
