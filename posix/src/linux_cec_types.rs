//! `<linux/cec.h>` — Consumer Electronics Control (CEC) constants.
//!
//! CEC is a one-wire protocol carried over HDMI connections that
//! allows devices to control each other (e.g., TV powers on receiver
//! when turned on, remote controls a Blu-ray player through the TV).
//! The Linux CEC framework exposes /dev/cecN devices for userspace
//! to send and receive CEC messages.

// ---------------------------------------------------------------------------
// CEC logical addresses
// ---------------------------------------------------------------------------

/// TV.
pub const CEC_LOG_ADDR_TV: u32 = 0;
/// Recording device 1.
pub const CEC_LOG_ADDR_RECORD_1: u32 = 1;
/// Recording device 2.
pub const CEC_LOG_ADDR_RECORD_2: u32 = 2;
/// Tuner 1.
pub const CEC_LOG_ADDR_TUNER_1: u32 = 3;
/// Playback device 1.
pub const CEC_LOG_ADDR_PLAYBACK_1: u32 = 4;
/// Audio system (soundbar/receiver).
pub const CEC_LOG_ADDR_AUDIOSYSTEM: u32 = 5;
/// Tuner 2.
pub const CEC_LOG_ADDR_TUNER_2: u32 = 6;
/// Tuner 3.
pub const CEC_LOG_ADDR_TUNER_3: u32 = 7;
/// Playback device 2.
pub const CEC_LOG_ADDR_PLAYBACK_2: u32 = 8;
/// Recording device 3.
pub const CEC_LOG_ADDR_RECORD_3: u32 = 9;
/// Playback device 3.
pub const CEC_LOG_ADDR_PLAYBACK_3: u32 = 11;
/// Backup 1.
pub const CEC_LOG_ADDR_BACKUP_1: u32 = 12;
/// Backup 2.
pub const CEC_LOG_ADDR_BACKUP_2: u32 = 13;
/// Free use (unregistered).
pub const CEC_LOG_ADDR_FREEUSE: u32 = 14;
/// Broadcast address (all devices).
pub const CEC_LOG_ADDR_BROADCAST: u32 = 15;
/// Invalid/unregistered address.
pub const CEC_LOG_ADDR_INVALID: u32 = 0xFF;

// ---------------------------------------------------------------------------
// CEC opcodes (common messages)
// ---------------------------------------------------------------------------

/// Report physical address to all.
pub const CEC_MSG_REPORT_PHYSICAL_ADDR: u32 = 0x84;
/// "Active source" announcement.
pub const CEC_MSG_ACTIVE_SOURCE: u32 = 0x82;
/// Request active source.
pub const CEC_MSG_REQUEST_ACTIVE_SOURCE: u32 = 0x85;
/// Standby (power off).
pub const CEC_MSG_STANDBY: u32 = 0x36;
/// Image view on (wake up TV for playback).
pub const CEC_MSG_IMAGE_VIEW_ON: u32 = 0x04;
/// Text view on (wake up TV for text display).
pub const CEC_MSG_TEXT_VIEW_ON: u32 = 0x0D;
/// Give device power status.
pub const CEC_MSG_GIVE_DEVICE_POWER_STATUS: u32 = 0x8F;
/// Report power status.
pub const CEC_MSG_REPORT_POWER_STATUS: u32 = 0x90;
/// User control pressed (remote button).
pub const CEC_MSG_USER_CONTROL_PRESSED: u32 = 0x44;
/// User control released.
pub const CEC_MSG_USER_CONTROL_RELEASED: u32 = 0x45;
/// Give OSD name.
pub const CEC_MSG_GIVE_OSD_NAME: u32 = 0x46;
/// Set OSD name.
pub const CEC_MSG_SET_OSD_NAME: u32 = 0x47;

// ---------------------------------------------------------------------------
// CEC ioctl commands
// ---------------------------------------------------------------------------

/// Get adapter capabilities.
pub const CEC_ADAP_G_CAPS: u32 = 0xC048_A100;
/// Get adapter state.
pub const CEC_ADAP_G_LOG_ADDRS: u32 = 0xC138_A103;
/// Set adapter logical addresses.
pub const CEC_ADAP_S_LOG_ADDRS: u32 = 0xC138_A104;
/// Transmit a message.
pub const CEC_TRANSMIT: u32 = 0xC040_A105;
/// Receive a message.
pub const CEC_RECEIVE: u32 = 0xC040_A106;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logical_addrs_distinct() {
        let addrs = [
            CEC_LOG_ADDR_TV, CEC_LOG_ADDR_RECORD_1,
            CEC_LOG_ADDR_RECORD_2, CEC_LOG_ADDR_TUNER_1,
            CEC_LOG_ADDR_PLAYBACK_1, CEC_LOG_ADDR_AUDIOSYSTEM,
            CEC_LOG_ADDR_TUNER_2, CEC_LOG_ADDR_TUNER_3,
            CEC_LOG_ADDR_PLAYBACK_2, CEC_LOG_ADDR_RECORD_3,
            CEC_LOG_ADDR_PLAYBACK_3, CEC_LOG_ADDR_BACKUP_1,
            CEC_LOG_ADDR_BACKUP_2, CEC_LOG_ADDR_FREEUSE,
            CEC_LOG_ADDR_BROADCAST,
        ];
        for i in 0..addrs.len() {
            for j in (i + 1)..addrs.len() {
                assert_ne!(addrs[i], addrs[j]);
            }
        }
    }

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            CEC_MSG_REPORT_PHYSICAL_ADDR, CEC_MSG_ACTIVE_SOURCE,
            CEC_MSG_REQUEST_ACTIVE_SOURCE, CEC_MSG_STANDBY,
            CEC_MSG_IMAGE_VIEW_ON, CEC_MSG_TEXT_VIEW_ON,
            CEC_MSG_GIVE_DEVICE_POWER_STATUS, CEC_MSG_REPORT_POWER_STATUS,
            CEC_MSG_USER_CONTROL_PRESSED, CEC_MSG_USER_CONTROL_RELEASED,
            CEC_MSG_GIVE_OSD_NAME, CEC_MSG_SET_OSD_NAME,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            CEC_ADAP_G_CAPS, CEC_ADAP_G_LOG_ADDRS,
            CEC_ADAP_S_LOG_ADDRS, CEC_TRANSMIT, CEC_RECEIVE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_broadcast_is_15() {
        assert_eq!(CEC_LOG_ADDR_BROADCAST, 15);
    }
}
