//! `<linux/cec.h>` — Consumer Electronics Control constants.
//!
//! CEC is a protocol over HDMI that allows devices to control each
//! other (e.g., TV remote controlling a Blu-ray player). The Linux
//! CEC framework exposes devices via /dev/cecN.

// ---------------------------------------------------------------------------
// CEC message opcodes
// ---------------------------------------------------------------------------

/// Feature abort.
pub const CEC_MSG_FEATURE_ABORT: u8 = 0x00;
/// Image view on.
pub const CEC_MSG_IMAGE_VIEW_ON: u8 = 0x04;
/// Text view on.
pub const CEC_MSG_TEXT_VIEW_ON: u8 = 0x0D;
/// Active source.
pub const CEC_MSG_ACTIVE_SOURCE: u8 = 0x82;
/// Inactive source.
pub const CEC_MSG_INACTIVE_SOURCE: u8 = 0x9D;
/// Request active source.
pub const CEC_MSG_REQUEST_ACTIVE_SOURCE: u8 = 0x85;
/// Standby.
pub const CEC_MSG_STANDBY: u8 = 0x36;
/// Give device power status.
pub const CEC_MSG_GIVE_DEVICE_POWER_STATUS: u8 = 0x8F;
/// Report power status.
pub const CEC_MSG_REPORT_POWER_STATUS: u8 = 0x90;
/// Give physical address.
pub const CEC_MSG_GIVE_PHYSICAL_ADDR: u8 = 0x83;
/// Report physical address.
pub const CEC_MSG_REPORT_PHYSICAL_ADDR: u8 = 0x84;
/// Give OSD name.
pub const CEC_MSG_GIVE_OSD_NAME: u8 = 0x46;
/// Set OSD name.
pub const CEC_MSG_SET_OSD_NAME: u8 = 0x47;
/// User control pressed.
pub const CEC_MSG_USER_CONTROL_PRESSED: u8 = 0x44;
/// User control released.
pub const CEC_MSG_USER_CONTROL_RELEASED: u8 = 0x45;
/// Routing change.
pub const CEC_MSG_ROUTING_CHANGE: u8 = 0x80;
/// Set stream path.
pub const CEC_MSG_SET_STREAM_PATH: u8 = 0x86;
/// CEC version.
pub const CEC_MSG_CEC_VERSION: u8 = 0x9E;
/// Get CEC version.
pub const CEC_MSG_GET_CEC_VERSION: u8 = 0x9F;
/// Abort.
pub const CEC_MSG_ABORT: u8 = 0xFF;

// ---------------------------------------------------------------------------
// CEC logical addresses
// ---------------------------------------------------------------------------

/// TV.
pub const CEC_LOG_ADDR_TV: u8 = 0;
/// Recording device 1.
pub const CEC_LOG_ADDR_RECORD_1: u8 = 1;
/// Recording device 2.
pub const CEC_LOG_ADDR_RECORD_2: u8 = 2;
/// Tuner 1.
pub const CEC_LOG_ADDR_TUNER_1: u8 = 3;
/// Playback device 1.
pub const CEC_LOG_ADDR_PLAYBACK_1: u8 = 4;
/// Audio system.
pub const CEC_LOG_ADDR_AUDIOSYSTEM: u8 = 5;
/// Tuner 2.
pub const CEC_LOG_ADDR_TUNER_2: u8 = 6;
/// Tuner 3.
pub const CEC_LOG_ADDR_TUNER_3: u8 = 7;
/// Playback device 2.
pub const CEC_LOG_ADDR_PLAYBACK_2: u8 = 8;
/// Recording device 3.
pub const CEC_LOG_ADDR_RECORD_3: u8 = 9;
/// Tuner 4.
pub const CEC_LOG_ADDR_TUNER_4: u8 = 10;
/// Playback device 3.
pub const CEC_LOG_ADDR_PLAYBACK_3: u8 = 11;
/// Free use (backup 1).
pub const CEC_LOG_ADDR_BACKUP_1: u8 = 12;
/// Free use (backup 2).
pub const CEC_LOG_ADDR_BACKUP_2: u8 = 13;
/// Specific use (e.g., unregistered).
pub const CEC_LOG_ADDR_SPECIFIC: u8 = 14;
/// Broadcast address.
pub const CEC_LOG_ADDR_BROADCAST: u8 = 15;
/// Unregistered/invalid.
pub const CEC_LOG_ADDR_UNREGISTERED: u8 = 15;

// ---------------------------------------------------------------------------
// Power status
// ---------------------------------------------------------------------------

/// On.
pub const CEC_OP_POWER_STATUS_ON: u8 = 0;
/// Standby.
pub const CEC_OP_POWER_STATUS_STANDBY: u8 = 1;
/// In transition standby to on.
pub const CEC_OP_POWER_STATUS_TO_ON: u8 = 2;
/// In transition on to standby.
pub const CEC_OP_POWER_STATUS_TO_STANDBY: u8 = 3;

// ---------------------------------------------------------------------------
// CEC capabilities
// ---------------------------------------------------------------------------

/// Adapter has physical address.
pub const CEC_CAP_PHYS_ADDR: u32 = 1 << 0;
/// Adapter can transmit.
pub const CEC_CAP_TRANSMIT: u32 = 1 << 1;
/// Adapter can receive.
pub const CEC_CAP_PASSTHROUGH: u32 = 1 << 2;
/// Adapter supports RC (remote control) events.
pub const CEC_CAP_RC: u32 = 1 << 3;
/// Adapter supports monitoring all traffic.
pub const CEC_CAP_MONITOR_ALL: u32 = 1 << 4;
/// Adapter needs HPD (hot plug detect).
pub const CEC_CAP_NEEDS_HPD: u32 = 1 << 5;
/// Adapter supports monitoring pin.
pub const CEC_CAP_MONITOR_PIN: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// CEC ioctl commands (simplified)
// ---------------------------------------------------------------------------

/// Get adapter capabilities.
pub const CEC_ADAP_G_CAPS: u32 = 0x80A8_6100;
/// Get physical address.
pub const CEC_ADAP_G_PHYS_ADDR: u32 = 0x8002_6101;
/// Set physical address.
pub const CEC_ADAP_S_PHYS_ADDR: u32 = 0x4002_6102;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_opcodes_distinct() {
        let msgs = [
            CEC_MSG_FEATURE_ABORT,
            CEC_MSG_IMAGE_VIEW_ON,
            CEC_MSG_TEXT_VIEW_ON,
            CEC_MSG_ACTIVE_SOURCE,
            CEC_MSG_INACTIVE_SOURCE,
            CEC_MSG_REQUEST_ACTIVE_SOURCE,
            CEC_MSG_STANDBY,
            CEC_MSG_GIVE_DEVICE_POWER_STATUS,
            CEC_MSG_REPORT_POWER_STATUS,
            CEC_MSG_GIVE_PHYSICAL_ADDR,
            CEC_MSG_REPORT_PHYSICAL_ADDR,
            CEC_MSG_GIVE_OSD_NAME,
            CEC_MSG_SET_OSD_NAME,
            CEC_MSG_USER_CONTROL_PRESSED,
            CEC_MSG_USER_CONTROL_RELEASED,
            CEC_MSG_ROUTING_CHANGE,
            CEC_MSG_SET_STREAM_PATH,
            CEC_MSG_CEC_VERSION,
            CEC_MSG_GET_CEC_VERSION,
            CEC_MSG_ABORT,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_log_addrs_distinct() {
        // Addresses 0-14 are distinct; 15 is shared by broadcast/unregistered
        let addrs = [
            CEC_LOG_ADDR_TV,
            CEC_LOG_ADDR_RECORD_1,
            CEC_LOG_ADDR_RECORD_2,
            CEC_LOG_ADDR_TUNER_1,
            CEC_LOG_ADDR_PLAYBACK_1,
            CEC_LOG_ADDR_AUDIOSYSTEM,
            CEC_LOG_ADDR_TUNER_2,
            CEC_LOG_ADDR_TUNER_3,
            CEC_LOG_ADDR_PLAYBACK_2,
            CEC_LOG_ADDR_RECORD_3,
            CEC_LOG_ADDR_TUNER_4,
            CEC_LOG_ADDR_PLAYBACK_3,
            CEC_LOG_ADDR_BACKUP_1,
            CEC_LOG_ADDR_BACKUP_2,
            CEC_LOG_ADDR_SPECIFIC,
        ];
        for i in 0..addrs.len() {
            for j in (i + 1)..addrs.len() {
                assert_ne!(addrs[i], addrs[j]);
            }
        }
    }

    #[test]
    fn test_broadcast_is_unregistered() {
        assert_eq!(CEC_LOG_ADDR_BROADCAST, CEC_LOG_ADDR_UNREGISTERED);
        assert_eq!(CEC_LOG_ADDR_BROADCAST, 15);
    }

    #[test]
    fn test_power_status_distinct() {
        let statuses = [
            CEC_OP_POWER_STATUS_ON,
            CEC_OP_POWER_STATUS_STANDBY,
            CEC_OP_POWER_STATUS_TO_ON,
            CEC_OP_POWER_STATUS_TO_STANDBY,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_caps_are_powers_of_two() {
        let caps = [
            CEC_CAP_PHYS_ADDR,
            CEC_CAP_TRANSMIT,
            CEC_CAP_PASSTHROUGH,
            CEC_CAP_RC,
            CEC_CAP_MONITOR_ALL,
            CEC_CAP_NEEDS_HPD,
            CEC_CAP_MONITOR_PIN,
        ];
        for cap in &caps {
            assert!(cap.is_power_of_two(), "0x{:x} is not a power of two", cap);
        }
    }
}
