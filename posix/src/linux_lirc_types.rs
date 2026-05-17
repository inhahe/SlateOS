//! `<linux/lirc.h>` — Linux Infrared Remote Control (LIRC) constants.
//!
//! LIRC provides an interface for infrared (IR) transmitters and
//! receivers. Applications open /dev/lircN to send and receive IR
//! signals (for remote control emulation or reception). The kernel
//! driver handles the timing of IR pulses and spaces.

// ---------------------------------------------------------------------------
// LIRC modes (driver capabilities)
// ---------------------------------------------------------------------------

/// Raw IR mode (pulse/space timing in microseconds).
pub const LIRC_MODE_RAW: u32 = 0x0000_0001;
/// Pulse mode (send raw pulses).
pub const LIRC_MODE_PULSE: u32 = 0x0000_0002;
/// Mode2 (receive raw pulse/space/timeout events).
pub const LIRC_MODE_MODE2: u32 = 0x0000_0004;
/// Scancode mode (decoded keycodes).
pub const LIRC_MODE_SCANCODE: u32 = 0x0000_0008;
/// LIRCCODE mode (raw protocol codes).
pub const LIRC_MODE_LIRCCODE: u32 = 0x0000_0010;

// ---------------------------------------------------------------------------
// LIRC ioctl commands
// ---------------------------------------------------------------------------

/// Get supported features.
pub const LIRC_GET_FEATURES: u32 = 0x8004_6900;
/// Set send mode.
pub const LIRC_SET_SEND_MODE: u32 = 0x4004_6911;
/// Set receive mode.
pub const LIRC_SET_REC_MODE: u32 = 0x4004_6912;
/// Get send mode.
pub const LIRC_GET_SEND_MODE: u32 = 0x8004_6901;
/// Get receive mode.
pub const LIRC_GET_REC_MODE: u32 = 0x8004_6902;
/// Set transmit carrier frequency (Hz).
pub const LIRC_SET_SEND_CARRIER: u32 = 0x4004_6913;
/// Set receive carrier frequency (Hz).
pub const LIRC_SET_REC_CARRIER: u32 = 0x4004_6914;
/// Set transmit duty cycle (percent).
pub const LIRC_SET_SEND_DUTY_CYCLE: u32 = 0x4004_6915;
/// Set receive timeout (microseconds).
pub const LIRC_SET_REC_TIMEOUT: u32 = 0x4004_6918;
/// Get minimum receive timeout.
pub const LIRC_GET_MIN_TIMEOUT: u32 = 0x8004_6908;
/// Get maximum receive timeout.
pub const LIRC_GET_MAX_TIMEOUT: u32 = 0x8004_6909;

// ---------------------------------------------------------------------------
// LIRC feature flags (from LIRC_GET_FEATURES)
// ---------------------------------------------------------------------------

/// Can send raw IR.
pub const LIRC_CAN_SEND_RAW: u32 = 0x0000_0001;
/// Can send in pulse mode.
pub const LIRC_CAN_SEND_PULSE: u32 = 0x0000_0002;
/// Can send in mode2.
pub const LIRC_CAN_SEND_MODE2: u32 = 0x0000_0004;
/// Can send scancodes.
pub const LIRC_CAN_SEND_SCANCODE: u32 = 0x0000_0008;
/// Can receive raw IR.
pub const LIRC_CAN_REC_RAW: u32 = 0x0000_0100;
/// Can receive in pulse mode.
pub const LIRC_CAN_REC_PULSE: u32 = 0x0000_0200;
/// Can receive in mode2.
pub const LIRC_CAN_REC_MODE2: u32 = 0x0000_0400;
/// Can receive scancodes.
pub const LIRC_CAN_REC_SCANCODE: u32 = 0x0000_0800;
/// Can receive LIRCCODE.
pub const LIRC_CAN_REC_LIRCCODE: u32 = 0x0000_1000;
/// Can set transmit carrier.
pub const LIRC_CAN_SET_SEND_CARRIER: u32 = 0x0001_0000;
/// Can set transmit duty cycle.
pub const LIRC_CAN_SET_SEND_DUTY_CYCLE: u32 = 0x0002_0000;
/// Can set receive timeout.
pub const LIRC_CAN_SET_REC_TIMEOUT: u32 = 0x0010_0000;

// ---------------------------------------------------------------------------
// Mode2 event types (embedded in 32-bit value)
// ---------------------------------------------------------------------------

/// Pulse event (IR on).
pub const LIRC_MODE2_PULSE: u32 = 0x0100_0000;
/// Space event (IR off).
pub const LIRC_MODE2_SPACE: u32 = 0x0000_0000;
/// Timeout event (end of signal).
pub const LIRC_MODE2_TIMEOUT: u32 = 0x0300_0000;
/// Mask for event type bits.
pub const LIRC_MODE2_MASK: u32 = 0xFF00_0000;
/// Mask for duration (microseconds).
pub const LIRC_VALUE_MASK: u32 = 0x00FF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_no_overlap() {
        let modes = [
            LIRC_MODE_RAW, LIRC_MODE_PULSE, LIRC_MODE_MODE2,
            LIRC_MODE_SCANCODE, LIRC_MODE_LIRCCODE,
        ];
        for i in 0..modes.len() {
            assert!(modes[i].is_power_of_two());
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            LIRC_GET_FEATURES, LIRC_SET_SEND_MODE, LIRC_SET_REC_MODE,
            LIRC_GET_SEND_MODE, LIRC_GET_REC_MODE,
            LIRC_SET_SEND_CARRIER, LIRC_SET_REC_CARRIER,
            LIRC_SET_SEND_DUTY_CYCLE, LIRC_SET_REC_TIMEOUT,
            LIRC_GET_MIN_TIMEOUT, LIRC_GET_MAX_TIMEOUT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_send_features_no_overlap() {
        let feats = [
            LIRC_CAN_SEND_RAW, LIRC_CAN_SEND_PULSE,
            LIRC_CAN_SEND_MODE2, LIRC_CAN_SEND_SCANCODE,
        ];
        for i in 0..feats.len() {
            assert!(feats[i].is_power_of_two());
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_mode2_events_distinct() {
        // Space is 0, so test differently.
        assert_ne!(LIRC_MODE2_PULSE, LIRC_MODE2_SPACE);
        assert_ne!(LIRC_MODE2_PULSE, LIRC_MODE2_TIMEOUT);
        assert_ne!(LIRC_MODE2_SPACE, LIRC_MODE2_TIMEOUT);
    }

    #[test]
    fn test_mode2_masks() {
        // Duration and type are non-overlapping fields.
        assert_eq!(LIRC_MODE2_MASK & LIRC_VALUE_MASK, 0);
        assert_eq!(LIRC_MODE2_MASK | LIRC_VALUE_MASK, 0xFFFF_FFFF);
    }
}
