//! `<linux/lirc.h>` — Linux Infrared Remote Control constants.
//!
//! LIRC provides a unified interface for infrared transmitters and
//! receivers used by remote controls. Accessed via /dev/lircN
//! with ioctl commands and read/write for pulse/space data.

// ---------------------------------------------------------------------------
// LIRC modes
// ---------------------------------------------------------------------------

/// Send/receive raw pulse/space timing.
pub const LIRC_MODE_RAW: u32 = 0x0000_0001;
/// Send/receive pulse timing (microseconds).
pub const LIRC_MODE_PULSE: u32 = 0x0000_0002;
/// Send/receive mode2 (pulse/space pairs).
pub const LIRC_MODE_MODE2: u32 = 0x0000_0004;
/// Send/receive scancode.
pub const LIRC_MODE_SCANCODE: u32 = 0x0000_0008;
/// Legacy LIRCCODE mode.
pub const LIRC_MODE_LIRCCODE: u32 = 0x0000_0010;

// ---------------------------------------------------------------------------
// LIRC capabilities
// ---------------------------------------------------------------------------

/// Can receive raw data.
pub const LIRC_CAN_REC_RAW: u32 = LIRC_MODE_RAW << 16;
/// Can receive pulse data.
pub const LIRC_CAN_REC_PULSE: u32 = LIRC_MODE_PULSE << 16;
/// Can receive mode2 data.
pub const LIRC_CAN_REC_MODE2: u32 = LIRC_MODE_MODE2 << 16;
/// Can receive scancode data.
pub const LIRC_CAN_REC_SCANCODE: u32 = LIRC_MODE_SCANCODE << 16;
/// Can receive LIRCCODE.
pub const LIRC_CAN_REC_LIRCCODE: u32 = LIRC_MODE_LIRCCODE << 16;
/// Can send raw data.
pub const LIRC_CAN_SEND_RAW: u32 = LIRC_MODE_RAW;
/// Can send pulse data.
pub const LIRC_CAN_SEND_PULSE: u32 = LIRC_MODE_PULSE;
/// Can send mode2 data.
pub const LIRC_CAN_SEND_MODE2: u32 = LIRC_MODE_MODE2;
/// Can send scancode data.
pub const LIRC_CAN_SEND_SCANCODE: u32 = LIRC_MODE_SCANCODE;
/// Can set send carrier.
pub const LIRC_CAN_SET_SEND_CARRIER: u32 = 0x0000_0100;
/// Can set send duty cycle.
pub const LIRC_CAN_SET_SEND_DUTY_CYCLE: u32 = 0x0000_0200;
/// Can set transmitter mask.
pub const LIRC_CAN_SET_TRANSMITTER_MASK: u32 = 0x0000_0400;
/// Can set receive carrier.
pub const LIRC_CAN_SET_REC_CARRIER: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// Mode2 value types (in the upper bits of a mode2 sample)
// ---------------------------------------------------------------------------

/// Pulse (IR on).
pub const LIRC_MODE2_PULSE: u32 = 0x0100_0000;
/// Space (IR off).
pub const LIRC_MODE2_SPACE: u32 = 0x0000_0000;
/// Frequency.
pub const LIRC_MODE2_FREQUENCY: u32 = 0x0200_0000;
/// Timeout.
pub const LIRC_MODE2_TIMEOUT: u32 = 0x0300_0000;
/// Overflow.
pub const LIRC_MODE2_OVERFLOW: u32 = 0x0400_0000;

/// Mode2 value mask (lower 24 bits).
pub const LIRC_VALUE_MASK: u32 = 0x00FF_FFFF;
/// Mode2 type mask (upper 8 bits).
pub const LIRC_MODE2_MASK: u32 = 0xFF00_0000;

// ---------------------------------------------------------------------------
// LIRC scancode protocols
// ---------------------------------------------------------------------------

/// Unknown protocol.
pub const LIRC_SCANCODE_UNKNOWN: u16 = 0;
/// RC-5 protocol.
pub const LIRC_SCANCODE_RC5: u16 = 1;
/// RC-5 SZ protocol.
pub const LIRC_SCANCODE_RC5_SZ: u16 = 2;
/// JVC protocol.
pub const LIRC_SCANCODE_JVC: u16 = 3;
/// Sony protocol.
pub const LIRC_SCANCODE_SONY: u16 = 4;
/// NEC protocol.
pub const LIRC_SCANCODE_NEC: u16 = 5;
/// SANYO protocol.
pub const LIRC_SCANCODE_SANYO: u16 = 6;
/// MCE keyboard.
pub const LIRC_SCANCODE_MCE_KBD: u16 = 7;
/// RC-6 protocol.
pub const LIRC_SCANCODE_RC6: u16 = 8;
/// Sharp protocol.
pub const LIRC_SCANCODE_SHARP: u16 = 9;
/// XMP protocol.
pub const LIRC_SCANCODE_XMP: u16 = 10;
/// CEC protocol.
pub const LIRC_SCANCODE_CEC: u16 = 11;
/// IMON protocol.
pub const LIRC_SCANCODE_IMON: u16 = 12;
/// RC-MM protocol.
pub const LIRC_SCANCODE_RCMM: u16 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_are_powers_of_two() {
        let modes = [
            LIRC_MODE_RAW, LIRC_MODE_PULSE, LIRC_MODE_MODE2,
            LIRC_MODE_SCANCODE, LIRC_MODE_LIRCCODE,
        ];
        for mode in &modes {
            assert!(mode.is_power_of_two());
        }
    }

    #[test]
    fn test_mode2_types_distinct() {
        let types = [
            LIRC_MODE2_SPACE, LIRC_MODE2_PULSE, LIRC_MODE2_FREQUENCY,
            LIRC_MODE2_TIMEOUT, LIRC_MODE2_OVERFLOW,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_scancode_protocols_distinct() {
        let protos = [
            LIRC_SCANCODE_UNKNOWN, LIRC_SCANCODE_RC5,
            LIRC_SCANCODE_RC5_SZ, LIRC_SCANCODE_JVC,
            LIRC_SCANCODE_SONY, LIRC_SCANCODE_NEC,
            LIRC_SCANCODE_SANYO, LIRC_SCANCODE_MCE_KBD,
            LIRC_SCANCODE_RC6, LIRC_SCANCODE_SHARP,
            LIRC_SCANCODE_XMP, LIRC_SCANCODE_CEC,
            LIRC_SCANCODE_IMON, LIRC_SCANCODE_RCMM,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_masks() {
        assert_eq!(LIRC_VALUE_MASK | LIRC_MODE2_MASK, 0xFFFF_FFFF);
        assert_eq!(LIRC_VALUE_MASK & LIRC_MODE2_MASK, 0);
    }

    #[test]
    fn test_recv_caps_shifted() {
        assert_eq!(LIRC_CAN_REC_MODE2, LIRC_MODE_MODE2 << 16);
        assert_eq!(LIRC_CAN_REC_SCANCODE, LIRC_MODE_SCANCODE << 16);
    }
}
