//! `<linux/cdrom.h>` — Additional CD-ROM ioctl constants.
//!
//! Supplementary CD-ROM constants covering ioctl commands,
//! disc status types, track modes, and capability flags.

// ---------------------------------------------------------------------------
// CD-ROM ioctl commands
// ---------------------------------------------------------------------------

/// Read table of contents header.
pub const CDROMREADTOCHDR: u32 = 0x5305;
/// Read TOC entry.
pub const CDROMREADTOCENTRY: u32 = 0x5306;
/// Stop disc.
pub const CDROMSTOP: u32 = 0x5307;
/// Start disc.
pub const CDROMSTART: u32 = 0x5308;
/// Eject disc.
pub const CDROMEJECT: u32 = 0x5309;
/// Set volume.
pub const CDROMVOLCTRL: u32 = 0x530A;
/// Read subchannel data.
pub const CDROMSUBCHNL: u32 = 0x530B;
/// Read mode 2 (2336 bytes).
pub const CDROMREADMODE2: u32 = 0x530C;
/// Read mode 1 (2048 bytes).
pub const CDROMREADMODE1: u32 = 0x530D;
/// Read raw (2352 bytes).
pub const CDROMREADRAW: u32 = 0x5314;
/// Multisession info.
pub const CDROMMULTISESSION: u32 = 0x5310;
/// Get volume.
pub const CDROMVOLREAD: u32 = 0x5313;
/// Pause playback.
pub const CDROMPAUSE: u32 = 0x5301;
/// Resume playback.
pub const CDROMRESUME: u32 = 0x5302;
/// Play MSF range.
pub const CDROMPLAYMSF: u32 = 0x5303;
/// Close tray.
pub const CDROMCLOSETRAY: u32 = 0x5319;
/// Reset drive.
pub const CDROMRESET: u32 = 0x5312;
/// Get disc status.
pub const CDROM_DISC_STATUS: u32 = 0x5327;
/// Get drive status.
pub const CDROM_DRIVE_STATUS: u32 = 0x5326;
/// Get capability.
pub const CDROM_GET_CAPABILITY: u32 = 0x5331;

// ---------------------------------------------------------------------------
// CD-ROM disc status values
// ---------------------------------------------------------------------------

/// No information.
pub const CDS_NO_INFO: i32 = 0;
/// No disc.
pub const CDS_NO_DISC: i32 = 1;
/// Tray open.
pub const CDS_TRAY_OPEN: i32 = 2;
/// Drive not ready.
pub const CDS_DRIVE_NOT_READY: i32 = 3;
/// Disc OK.
pub const CDS_DISC_OK: i32 = 4;
/// Audio disc.
pub const CDS_AUDIO: i32 = 100;
/// Data 1 disc.
pub const CDS_DATA_1: i32 = 101;
/// Data 2 disc.
pub const CDS_DATA_2: i32 = 102;
/// XA 2/1 disc.
pub const CDS_XA_2_1: i32 = 103;
/// XA 2/2 disc.
pub const CDS_XA_2_2: i32 = 104;
/// Mixed disc.
pub const CDS_MIXED: i32 = 105;

// ---------------------------------------------------------------------------
// CD-ROM track mode flags
// ---------------------------------------------------------------------------

/// Audio track.
pub const CDROM_AUDIO: u8 = 0x00;
/// Data track (mode 1).
pub const CDROM_DATA_MODE1: u8 = 0x01;
/// Data track (mode 2).
pub const CDROM_DATA_MODE2: u8 = 0x02;
/// XA track.
pub const CDROM_XA: u8 = 0x03;

// ---------------------------------------------------------------------------
// CD-ROM capability flags (CDC_*)
// ---------------------------------------------------------------------------

/// Close tray.
pub const CDC_CLOSE_TRAY: u32 = 1 << 0;
/// Open tray.
pub const CDC_OPEN_TRAY: u32 = 1 << 1;
/// Lock door.
pub const CDC_LOCK: u32 = 1 << 2;
/// Select speed.
pub const CDC_SELECT_SPEED: u32 = 1 << 3;
/// Select disc (changer).
pub const CDC_SELECT_DISC: u32 = 1 << 4;
/// Multi-session.
pub const CDC_MULTI_SESSION: u32 = 1 << 5;
/// Media changed.
pub const CDC_MCN: u32 = 1 << 6;
/// Play audio.
pub const CDC_PLAY_AUDIO: u32 = 1 << 8;
/// Reset drive.
pub const CDC_RESET: u32 = 1 << 9;
/// Drive status.
pub const CDC_DRIVE_STATUS: u32 = 1 << 11;
/// Generic packet.
pub const CDC_GENERIC_PACKET: u32 = 1 << 12;
/// CD-R.
pub const CDC_CD_R: u32 = 1 << 13;
/// CD-RW.
pub const CDC_CD_RW: u32 = 1 << 14;
/// DVD.
pub const CDC_DVD: u32 = 1 << 15;
/// DVD-R.
pub const CDC_DVD_R: u32 = 1 << 16;
/// DVD-RAM.
pub const CDC_DVD_RAM: u32 = 1 << 17;
/// MO drive.
pub const CDC_MO_DRIVE: u32 = 1 << 18;
/// MRW.
pub const CDC_MRW: u32 = 1 << 19;
/// MRW write.
pub const CDC_MRW_W: u32 = 1 << 20;
/// RAM.
pub const CDC_RAM: u32 = 1 << 21;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [
            CDROMREADTOCHDR,
            CDROMREADTOCENTRY,
            CDROMSTOP,
            CDROMSTART,
            CDROMEJECT,
            CDROMVOLCTRL,
            CDROMSUBCHNL,
            CDROMREADMODE2,
            CDROMREADMODE1,
            CDROMREADRAW,
            CDROMMULTISESSION,
            CDROMVOLREAD,
            CDROMPAUSE,
            CDROMRESUME,
            CDROMPLAYMSF,
            CDROMCLOSETRAY,
            CDROMRESET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_disc_status_distinct() {
        let vals = [
            CDS_NO_INFO,
            CDS_NO_DISC,
            CDS_TRAY_OPEN,
            CDS_DRIVE_NOT_READY,
            CDS_DISC_OK,
            CDS_AUDIO,
            CDS_DATA_1,
            CDS_DATA_2,
            CDS_XA_2_1,
            CDS_XA_2_2,
            CDS_MIXED,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_track_modes_distinct() {
        let modes: [u8; 4] = [CDROM_AUDIO, CDROM_DATA_MODE1, CDROM_DATA_MODE2, CDROM_XA];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_capability_flags_power_of_two() {
        let caps = [
            CDC_CLOSE_TRAY,
            CDC_OPEN_TRAY,
            CDC_LOCK,
            CDC_SELECT_SPEED,
            CDC_SELECT_DISC,
            CDC_MULTI_SESSION,
            CDC_MCN,
            CDC_PLAY_AUDIO,
            CDC_RESET,
            CDC_DRIVE_STATUS,
            CDC_GENERIC_PACKET,
            CDC_CD_R,
            CDC_CD_RW,
            CDC_DVD,
            CDC_DVD_R,
            CDC_DVD_RAM,
            CDC_MO_DRIVE,
            CDC_MRW,
            CDC_MRW_W,
            CDC_RAM,
        ];
        for c in &caps {
            assert!(c.is_power_of_two(), "0x{:08x} not power of two", c);
        }
    }

    #[test]
    fn test_capability_flags_no_overlap() {
        let caps = [
            CDC_CLOSE_TRAY,
            CDC_OPEN_TRAY,
            CDC_LOCK,
            CDC_SELECT_SPEED,
            CDC_SELECT_DISC,
            CDC_MULTI_SESSION,
            CDC_MCN,
            CDC_PLAY_AUDIO,
            CDC_RESET,
            CDC_DRIVE_STATUS,
            CDC_GENERIC_PACKET,
            CDC_CD_R,
            CDC_CD_RW,
            CDC_DVD,
            CDC_DVD_R,
            CDC_DVD_RAM,
            CDC_MO_DRIVE,
            CDC_MRW,
            CDC_MRW_W,
            CDC_RAM,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }
}
