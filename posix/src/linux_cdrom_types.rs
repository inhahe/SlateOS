//! `<linux/cdrom.h>` — CD-ROM/DVD/Blu-ray drive constants.
//!
//! The Linux CDROM subsystem provides a unified interface for optical
//! disc drives. Supports audio CD playback, data disc reading,
//! disc burning (via SG_IO), and drive status queries. These ioctl
//! commands and constants are used by disc utilities, media players,
//! and burning software.

// ---------------------------------------------------------------------------
// CDROM ioctl commands
// ---------------------------------------------------------------------------

/// Open the disc tray.
pub const CDROMEJECT: u32 = 0x5309;
/// Close the disc tray.
pub const CDROMCLOSETRAY: u32 = 0x5319;
/// Lock/unlock the disc tray.
pub const CDROM_LOCKDOOR: u32 = 0x5329;
/// Get disc status.
pub const CDROM_DISC_STATUS: u32 = 0x5327;
/// Get drive status.
pub const CDROM_DRIVE_STATUS: u32 = 0x5326;
/// Set read speed.
pub const CDROM_SELECT_SPEED: u32 = 0x5322;
/// Read the disc's Table of Contents.
pub const CDROMREADTOCHDR: u32 = 0x5305;
/// Read a TOC entry.
pub const CDROMREADTOCENTRY: u32 = 0x5306;
/// Play audio from MSF position.
pub const CDROMPLAYMSF: u32 = 0x5303;
/// Stop audio playback.
pub const CDROMSTOP: u32 = 0x5307;
/// Pause audio playback.
pub const CDROMPAUSE: u32 = 0x5308;
/// Resume audio playback.
pub const CDROMRESUME: u32 = 0x530B;
/// Get last session (multisession discs).
pub const CDROM_LAST_WRITTEN: u32 = 0x5395;
/// Get media change event.
pub const CDROM_MEDIA_CHANGED: u32 = 0x5325;

// ---------------------------------------------------------------------------
// Disc status values
// ---------------------------------------------------------------------------

/// No information available.
pub const CDS_NO_INFO: u32 = 0;
/// No disc in drive.
pub const CDS_NO_DISC: u32 = 1;
/// Tray is open.
pub const CDS_TRAY_OPEN: u32 = 2;
/// Drive not ready.
pub const CDS_DRIVE_NOT_READY: u32 = 3;
/// Disc OK (ready).
pub const CDS_DISC_OK: u32 = 4;

// ---------------------------------------------------------------------------
// Disc types
// ---------------------------------------------------------------------------

/// Audio CD.
pub const CDS_AUDIO: u32 = 100;
/// Data CD (Mode 1).
pub const CDS_DATA_1: u32 = 101;
/// Data CD (Mode 2 / XA).
pub const CDS_DATA_2: u32 = 102;
/// Mixed mode (audio + data).
pub const CDS_MIXED: u32 = 105;

// ---------------------------------------------------------------------------
// Drive capabilities
// ---------------------------------------------------------------------------

/// Can read CD-R.
pub const CDC_CD_R: u32 = 0x2000;
/// Can read CD-RW.
pub const CDC_CD_RW: u32 = 0x4000;
/// Can read DVD.
pub const CDC_DVD: u32 = 0x8000;
/// Can read DVD-R.
pub const CDC_DVD_R: u32 = 0x0001_0000;
/// Can read DVD-RAM.
pub const CDC_DVD_RAM: u32 = 0x0002_0000;
/// Can play audio.
pub const CDC_PLAY_AUDIO: u32 = 0x0100;
/// Can lock tray.
pub const CDC_LOCK: u32 = 0x0004;
/// Can eject tray.
pub const CDC_OPEN_TRAY: u32 = 0x0001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            CDROMEJECT, CDROMCLOSETRAY, CDROM_LOCKDOOR,
            CDROM_DISC_STATUS, CDROM_DRIVE_STATUS,
            CDROM_SELECT_SPEED, CDROMREADTOCHDR,
            CDROMREADTOCENTRY, CDROMPLAYMSF, CDROMSTOP,
            CDROMPAUSE, CDROMRESUME, CDROM_LAST_WRITTEN,
            CDROM_MEDIA_CHANGED,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_status_values_distinct() {
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
        let types = [CDS_AUDIO, CDS_DATA_1, CDS_DATA_2, CDS_MIXED];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_capabilities_no_overlap() {
        let caps = [
            CDC_CD_R, CDC_CD_RW, CDC_DVD, CDC_DVD_R,
            CDC_DVD_RAM, CDC_PLAY_AUDIO, CDC_LOCK, CDC_OPEN_TRAY,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }
}
