//! `<linux/slimbus.h>` — SLIMbus (Serial Low-power Inter-chip Media Bus) constants.
//!
//! SLIMbus is a two-wire bus for connecting audio components in
//! mobile devices (codec ↔ modem ↔ BT ↔ AP). It supports up to
//! 32 devices, provides isochronous audio channels with guaranteed
//! bandwidth, and includes device discovery and enumeration.
//! Primarily used in Qualcomm SoCs for audio routing between
//! the application processor, WCD codec, and other audio devices.

// ---------------------------------------------------------------------------
// SLIMbus message types
// ---------------------------------------------------------------------------

/// Value element access (read/write codec registers).
pub const SLIM_MSG_VALUE: u32 = 0;
/// Information element (device status/capability).
pub const SLIM_MSG_INFO: u32 = 1;
/// Data channel management.
pub const SLIM_MSG_DATA_CHANNEL: u32 = 2;

// ---------------------------------------------------------------------------
// SLIMbus device states
// ---------------------------------------------------------------------------

/// Device not present (not enumerated).
pub const SLIM_DEVICE_ABSENT: u32 = 0;
/// Device present (enumerated, assigned logical address).
pub const SLIM_DEVICE_PRESENT: u32 = 1;
/// Device active (channels configured and running).
pub const SLIM_DEVICE_ACTIVE: u32 = 2;
/// Device sleeping (low-power state).
pub const SLIM_DEVICE_SLEEPING: u32 = 3;

// ---------------------------------------------------------------------------
// SLIMbus channel data types
// ---------------------------------------------------------------------------

/// Linear PCM (standard audio).
pub const SLIM_CH_LPCM: u32 = 0;
/// IEC 61937 (compressed audio passthrough).
pub const SLIM_CH_IEC61937: u32 = 1;
/// Non-audio data (control/status).
pub const SLIM_CH_NON_AUDIO: u32 = 2;

// ---------------------------------------------------------------------------
// SLIMbus data line protocol
// ---------------------------------------------------------------------------

/// Isochronous protocol (guaranteed bandwidth).
pub const SLIM_PROTO_ISO: u32 = 0;
/// Pushed protocol (source-driven timing).
pub const SLIM_PROTO_PUSH: u32 = 1;
/// Pulled protocol (sink-driven timing).
pub const SLIM_PROTO_PULL: u32 = 2;
/// Asynchronous protocol (packet-based, no timing guarantee).
pub const SLIM_PROTO_ASYNC: u32 = 3;

// ---------------------------------------------------------------------------
// SLIMbus clock rates
// ---------------------------------------------------------------------------

/// SLIMbus base clock: 24.576 MHz.
pub const SLIM_CLK_24576: u32 = 24_576_000;
/// SLIMbus reduced clock: 12.288 MHz (half rate).
pub const SLIM_CLK_12288: u32 = 12_288_000;
/// SLIMbus low-power clock: 6.144 MHz (quarter rate).
pub const SLIM_CLK_6144: u32 = 6_144_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let types = [SLIM_MSG_VALUE, SLIM_MSG_INFO, SLIM_MSG_DATA_CHANNEL];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_device_states_distinct() {
        let states = [
            SLIM_DEVICE_ABSENT, SLIM_DEVICE_PRESENT,
            SLIM_DEVICE_ACTIVE, SLIM_DEVICE_SLEEPING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_channel_types_distinct() {
        let types = [SLIM_CH_LPCM, SLIM_CH_IEC61937, SLIM_CH_NON_AUDIO];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            SLIM_PROTO_ISO, SLIM_PROTO_PUSH,
            SLIM_PROTO_PULL, SLIM_PROTO_ASYNC,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_clock_rates_ordered() {
        assert!(SLIM_CLK_6144 < SLIM_CLK_12288);
        assert!(SLIM_CLK_12288 < SLIM_CLK_24576);
    }
}
