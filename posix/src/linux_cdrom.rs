//! `<linux/cdrom.h>` — CD-ROM device interface.
//!
//! Provides ioctl constants for CD-ROM and DVD drives.

// ---------------------------------------------------------------------------
// CDROM ioctl commands
// ---------------------------------------------------------------------------

/// Pause audio playback.
pub const CDROMPAUSE: u64 = 0x5301;
/// Resume audio playback.
pub const CDROMRESUME: u64 = 0x5302;
/// Play audio (MSF addresses).
pub const CDROMPLAYMSF: u64 = 0x5303;
/// Read Q subchannel data.
pub const CDROMSUBCHNL: u64 = 0x530B;
/// Read TOC (table of contents) header.
pub const CDROMREADTOCHDR: u64 = 0x5305;
/// Read TOC entry.
pub const CDROMREADTOCENTRY: u64 = 0x5306;
/// Stop drive motor.
pub const CDROMSTOP: u64 = 0x5307;
/// Start drive motor.
pub const CDROMSTART: u64 = 0x5308;
/// Eject disc.
pub const CDROMEJECT: u64 = 0x5309;
/// Read volume.
pub const CDROMVOLREAD: u64 = 0x5313;
/// Set volume.
pub const CDROMVOLCTRL: u64 = 0x530A;
/// Read raw data (2352 bytes/sector).
pub const CDROMREADRAW: u64 = 0x5314;
/// Read mode 1 data (2048 bytes/sector).
pub const CDROMREADMODE1: u64 = 0x530D;
/// Read mode 2 data (2336 bytes/sector).
pub const CDROMREADMODE2: u64 = 0x530E;
/// Reset drive.
pub const CDROMRESET: u64 = 0x5312;
/// Get drive status.
pub const CDROM_DRIVE_STATUS: u64 = 0x5326;
/// Get disc status.
pub const CDROM_DISC_STATUS: u64 = 0x5327;
/// Check media changed.
pub const CDROM_MEDIA_CHANGED: u64 = 0x5325;
/// Lock/unlock door.
pub const CDROM_LOCKDOOR: u64 = 0x5329;
/// Close tray.
pub const CDROMCLOSETRAY: u64 = 0x5319;
/// Set speed (DVD).
pub const CDROM_SET_SPEED: u64 = 0x5322;
/// Get MCN (media catalog number).
pub const CDROM_GET_MCN: u64 = 0x5311;
/// Get last session info.
pub const CDROM_LAST_WRITTEN: u64 = 0x5395;

// ---------------------------------------------------------------------------
// Drive status values (returned by CDROM_DRIVE_STATUS)
// ---------------------------------------------------------------------------

/// No information available.
pub const CDS_NO_INFO: i32 = 0;
/// No disc in drive.
pub const CDS_NO_DISC: i32 = 1;
/// Tray is open.
pub const CDS_TRAY_OPEN: i32 = 2;
/// Drive not ready.
pub const CDS_DRIVE_NOT_READY: i32 = 3;
/// Disc OK.
pub const CDS_DISC_OK: i32 = 4;

// ---------------------------------------------------------------------------
// Disc status values (returned by CDROM_DISC_STATUS)
// ---------------------------------------------------------------------------

/// Audio disc.
pub const CDS_AUDIO: i32 = 100;
/// Data disc (mode 1).
pub const CDS_DATA_1: i32 = 101;
/// Data disc (mode 2).
pub const CDS_DATA_2: i32 = 102;
/// XA disc (mode 2, form 1 or 2).
pub const CDS_XA_2_1: i32 = 103;
/// XA disc.
pub const CDS_XA_2_2: i32 = 104;
/// Mixed disc.
pub const CDS_MIXED: i32 = 105;

// ---------------------------------------------------------------------------
// MSF address
// ---------------------------------------------------------------------------

/// Minute:Second:Frame address.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CdromMsf {
    /// Start minute.
    pub cdmsf_min0: u8,
    /// Start second.
    pub cdmsf_sec0: u8,
    /// Start frame.
    pub cdmsf_frame0: u8,
    /// End minute.
    pub cdmsf_min1: u8,
    /// End second.
    pub cdmsf_sec1: u8,
    /// End frame.
    pub cdmsf_frame1: u8,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdrom_msf_size() {
        assert_eq!(core::mem::size_of::<CdromMsf>(), 6);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            CDROMPAUSE, CDROMRESUME, CDROMPLAYMSF, CDROMSUBCHNL,
            CDROMREADTOCHDR, CDROMREADTOCENTRY, CDROMSTOP,
            CDROMSTART, CDROMEJECT, CDROMRESET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_drive_status_values() {
        assert_eq!(CDS_NO_INFO, 0);
        assert_eq!(CDS_DISC_OK, 4);
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
    fn test_disc_status_values() {
        assert_eq!(CDS_AUDIO, 100);
        assert_eq!(CDS_MIXED, 105);
    }

    #[test]
    fn test_msf_address() {
        let msf = CdromMsf {
            cdmsf_min0: 0, cdmsf_sec0: 2, cdmsf_frame0: 0,
            cdmsf_min1: 5, cdmsf_sec1: 30, cdmsf_frame1: 0,
        };
        assert_eq!(msf.cdmsf_min1, 5);
    }
}
