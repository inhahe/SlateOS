//! `<linux/cdrom.h>` — userspace CD-ROM ioctls and audio-format codes.
//!
//! cdda2wav, abcde, libdiscid, and util-linux's `eject` open
//! `/dev/cdrom`/`/dev/sr0` and issue the ioctls below to play
//! tracks, read TOC entries, eject the tray, and query drive
//! capabilities.

// ---------------------------------------------------------------------------
// ioctl numbers (legacy 0x5300 series)
// ---------------------------------------------------------------------------

/// Pause the current audio track.
pub const CDROMPAUSE: u32 = 0x5301;
/// Resume audio playback.
pub const CDROMRESUME: u32 = 0x5302;
/// Begin audio playback by MSF (minute/second/frame).
pub const CDROMPLAYMSF: u32 = 0x5303;
/// Begin audio playback by track/index.
pub const CDROMPLAYTRKIND: u32 = 0x5304;
/// Read the TOC header.
pub const CDROMREADTOCHDR: u32 = 0x5305;
/// Read one TOC entry.
pub const CDROMREADTOCENTRY: u32 = 0x5306;
/// Stop the motor.
pub const CDROMSTOP: u32 = 0x5307;
/// Start the motor.
pub const CDROMSTART: u32 = 0x5308;
/// Eject the disc tray.
pub const CDROMEJECT: u32 = 0x5309;
/// Set audio volume.
pub const CDROMVOLCTRL: u32 = 0x530a;
/// Read sub-channel data.
pub const CDROMSUBCHNL: u32 = 0x530b;
/// Read raw mode-2 frames.
pub const CDROMREADMODE2: u32 = 0x530c;
/// Read raw mode-1 frames.
pub const CDROMREADMODE1: u32 = 0x530d;
/// Read audio frames (CDDA).
pub const CDROMREADAUDIO: u32 = 0x530e;
/// Enable auto-eject on close.
pub const CDROMEJECT_SW: u32 = 0x530f;
/// Read multi-session table.
pub const CDROMMULTISESSION: u32 = 0x5310;
/// Get UPC barcode.
pub const CDROM_GET_MCN: u32 = 0x5311;
/// Reset the drive.
pub const CDROMRESET: u32 = 0x5312;
/// Query drive volume.
pub const CDROMVOLREAD: u32 = 0x5313;
/// Read raw blocks.
pub const CDROMREADRAW: u32 = 0x5314;
/// Get changer status.
pub const CDROM_DRIVE_STATUS: u32 = 0x5326;
/// Get disc status (data/audio/mixed/blank).
pub const CDROM_DISC_STATUS: u32 = 0x5327;
/// Get changer slot count.
pub const CDROM_CHANGER_NSLOTS: u32 = 0x5328;
/// Lock the door.
pub const CDROM_LOCKDOOR: u32 = 0x5329;
/// Soft-debug (debug-only).
pub const CDROM_DEBUG: u32 = 0x5330;
/// Query drive capabilities bitmap.
pub const CDROM_GET_CAPABILITY: u32 = 0x5331;

// ---------------------------------------------------------------------------
// Drive status values (CDROM_DRIVE_STATUS return)
// ---------------------------------------------------------------------------

/// Drive has no information.
pub const CDS_NO_INFO: u32 = 0;
/// No disc in drive.
pub const CDS_NO_DISC: u32 = 1;
/// Tray open.
pub const CDS_TRAY_OPEN: u32 = 2;
/// Drive not ready (e.g. spinning up).
pub const CDS_DRIVE_NOT_READY: u32 = 3;
/// Disc OK, ready.
pub const CDS_DISC_OK: u32 = 4;

// ---------------------------------------------------------------------------
// Disc status values (CDROM_DISC_STATUS return)
// ---------------------------------------------------------------------------

/// Audio CD.
pub const CDS_AUDIO: u32 = 100;
/// Data CD (mode 1).
pub const CDS_DATA_1: u32 = 101;
/// Data CD (mode 2 form 1).
pub const CDS_DATA_2: u32 = 102;
/// XA mode-2 form-1.
pub const CDS_XA_2_1: u32 = 103;
/// XA mode-2 form-2.
pub const CDS_XA_2_2: u32 = 104;
/// Mixed CD (data + audio tracks).
pub const CDS_MIXED: u32 = 105;

// ---------------------------------------------------------------------------
// Address format flags for play/read ioctls
// ---------------------------------------------------------------------------

/// Address in linear block (LBA) form.
pub const CDROM_LBA: u8 = 0x01;
/// Address in minute/second/frame (MSF) form.
pub const CDROM_MSF: u8 = 0x02;

// ---------------------------------------------------------------------------
// Frame size / TOC limits
// ---------------------------------------------------------------------------

/// One CD audio frame in bytes (2352).
pub const CD_FRAMESIZE_RAW: u32 = 2352;
/// One data frame (2048).
pub const CD_FRAMESIZE: u32 = 2048;
/// Frames per second on a Red Book CD.
pub const CD_FRAMES: u32 = 75;
/// Seconds per minute.
pub const CD_SECS: u32 = 60;
/// Maximum number of tracks.
pub const CDROM_LEADOUT: u8 = 0xaa;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct_and_in_group() {
        // All cdrom ioctls live in the 0x5300..=0x5331 range so the
        // kernel can dispatch with a single `(cmd & 0xff00) == 0x5300`
        // check.
        let ops = [
            CDROMPAUSE, CDROMRESUME, CDROMPLAYMSF, CDROMPLAYTRKIND,
            CDROMREADTOCHDR, CDROMREADTOCENTRY, CDROMSTOP, CDROMSTART,
            CDROMEJECT, CDROMVOLCTRL, CDROMSUBCHNL, CDROMREADMODE2,
            CDROMREADMODE1, CDROMREADAUDIO, CDROMEJECT_SW,
            CDROMMULTISESSION, CDROM_GET_MCN, CDROMRESET, CDROMVOLREAD,
            CDROMREADRAW, CDROM_DRIVE_STATUS, CDROM_DISC_STATUS,
            CDROM_CHANGER_NSLOTS, CDROM_LOCKDOOR, CDROM_DEBUG,
            CDROM_GET_CAPABILITY,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            assert!(ops[i] >= 0x5300);
            assert!(ops[i] <= 0x533f);
        }
    }

    #[test]
    fn test_drive_status_dense() {
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
    }

    #[test]
    fn test_disc_status_in_100_range() {
        let d = [
            CDS_AUDIO, CDS_DATA_1, CDS_DATA_2, CDS_XA_2_1, CDS_XA_2_2,
            CDS_MIXED,
        ];
        for &v in &d {
            // Disc status values live in 100.. to avoid collision with
            // drive status values.
            assert!(v >= 100);
            assert!(v < 200);
        }
        for i in 0..d.len() {
            for j in (i + 1)..d.len() {
                assert_ne!(d[i], d[j]);
            }
        }
    }

    #[test]
    fn test_address_flags_distinct() {
        assert_eq!(CDROM_LBA, 1);
        assert_eq!(CDROM_MSF, 2);
        assert_ne!(CDROM_LBA, CDROM_MSF);
    }

    #[test]
    fn test_frame_sizes_known() {
        // Red Book CDDA: 2352 bytes/frame; 75 frames/s; 60s/min.
        assert_eq!(CD_FRAMESIZE_RAW, 2352);
        assert_eq!(CD_FRAMESIZE, 2048);
        assert_eq!(CD_FRAMES, 75);
        assert_eq!(CD_SECS, 60);
        // Lead-out track number is the historical 0xaa marker.
        assert_eq!(CDROM_LEADOUT, 0xaa);
    }
}
