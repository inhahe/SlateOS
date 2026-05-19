//! `<linux/cdrom.h>` — Additional CD-ROM constants (part 3).
//!
//! Supplementary CD-ROM constants covering disc status,
//! media change flags, and drive capabilities.

// ---------------------------------------------------------------------------
// CD-ROM disc status
// ---------------------------------------------------------------------------

/// No information.
pub const CDS_NO_INFO: u32 = 0;
/// No disc.
pub const CDS_NO_DISC: u32 = 1;
/// Tray open.
pub const CDS_TRAY_OPEN: u32 = 2;
/// Drive not ready.
pub const CDS_DRIVE_NOT_READY: u32 = 3;
/// Disc OK.
pub const CDS_DISC_OK: u32 = 4;

// ---------------------------------------------------------------------------
// CD-ROM disc type
// ---------------------------------------------------------------------------

/// Audio disc.
pub const CDS_AUDIO: u32 = 100;
/// Data mode 1.
pub const CDS_DATA_1: u32 = 101;
/// Data mode 2.
pub const CDS_DATA_2: u32 = 102;
/// XA disc (2/1).
pub const CDS_XA_2_1: u32 = 103;
/// XA disc (2/2).
pub const CDS_XA_2_2: u32 = 104;
/// Mixed disc.
pub const CDS_MIXED: u32 = 105;

// ---------------------------------------------------------------------------
// CD-ROM drive capability flags
// ---------------------------------------------------------------------------

/// Close tray.
pub const CDC_CLOSE_TRAY: u32 = 0x1;
/// Open tray.
pub const CDC_OPEN_TRAY: u32 = 0x2;
/// Lock door.
pub const CDC_LOCK: u32 = 0x4;
/// Select speed.
pub const CDC_SELECT_SPEED: u32 = 0x8;
/// Select disc (changer).
pub const CDC_SELECT_DISC: u32 = 0x10;
/// Multiple sessions.
pub const CDC_MULTI_SESSION: u32 = 0x20;
/// Media changed.
pub const CDC_MCN: u32 = 0x40;
/// Media catalog number.
pub const CDC_MEDIA_CHANGED: u32 = 0x80;
/// Play audio.
pub const CDC_PLAY_AUDIO: u32 = 0x100;
/// Reset drive.
pub const CDC_RESET: u32 = 0x200;
/// CD-R drive.
pub const CDC_CD_R: u32 = 0x2000;
/// CD-RW drive.
pub const CDC_CD_RW: u32 = 0x4000;
/// DVD drive.
pub const CDC_DVD: u32 = 0x8000;
/// DVD-R drive.
pub const CDC_DVD_R: u32 = 0x10000;
/// DVD-RAM drive.
pub const CDC_DVD_RAM: u32 = 0x20000;
/// MO drive.
pub const CDC_MO_DRIVE: u32 = 0x40000;
/// MRW.
pub const CDC_MRW: u32 = 0x80000;
/// MRW-W.
pub const CDC_MRW_W: u32 = 0x100000;
/// RAM.
pub const CDC_RAM: u32 = 0x200000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disc_status_distinct() {
        let statuses = [
            CDS_NO_INFO, CDS_NO_DISC, CDS_TRAY_OPEN,
            CDS_DRIVE_NOT_READY, CDS_DISC_OK,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_disc_types_distinct() {
        let types = [
            CDS_AUDIO, CDS_DATA_1, CDS_DATA_2,
            CDS_XA_2_1, CDS_XA_2_2, CDS_MIXED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_capability_flags_power_of_two() {
        let caps = [
            CDC_CLOSE_TRAY, CDC_OPEN_TRAY, CDC_LOCK,
            CDC_SELECT_SPEED, CDC_SELECT_DISC, CDC_MULTI_SESSION,
            CDC_MCN, CDC_MEDIA_CHANGED, CDC_PLAY_AUDIO, CDC_RESET,
            CDC_CD_R, CDC_CD_RW, CDC_DVD, CDC_DVD_R,
            CDC_DVD_RAM, CDC_MO_DRIVE, CDC_MRW, CDC_MRW_W, CDC_RAM,
        ];
        for c in &caps {
            assert!(c.is_power_of_two());
        }
    }

    #[test]
    fn test_capability_flags_no_overlap() {
        let caps = [
            CDC_CLOSE_TRAY, CDC_OPEN_TRAY, CDC_LOCK,
            CDC_SELECT_SPEED, CDC_SELECT_DISC, CDC_MULTI_SESSION,
            CDC_MCN, CDC_MEDIA_CHANGED, CDC_PLAY_AUDIO, CDC_RESET,
            CDC_CD_R, CDC_CD_RW, CDC_DVD, CDC_DVD_R,
            CDC_DVD_RAM, CDC_MO_DRIVE, CDC_MRW, CDC_MRW_W, CDC_RAM,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }
}
