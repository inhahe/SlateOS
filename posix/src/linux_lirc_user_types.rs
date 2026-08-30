//! `<linux/lirc.h>` — Linux Infrared Remote Control userspace API.
//!
//! lircd, ir-keytable, and any program that drives a remote-control
//! transmitter (e.g. ATtinyIR, IguanaIR) open `/dev/lirc0` and
//! configure send/receive modes, carrier, duty cycle, and protocol
//! filters using the ioctls below.

// ---------------------------------------------------------------------------
// ioctl group letter and timeout sentinel
// ---------------------------------------------------------------------------

/// Magic letter for /dev/lirc ioctls ('i').
pub const LIRC_IOC_MAGIC: u8 = b'i';

/// Special "infinity" timeout — disable receive timeout.
pub const LIRC_VALUE_MASK: u32 = 0x00ff_ffff;
/// Pulse / space bit (set => pulse).
pub const LIRC_MODE2_PULSE: u32 = 0x0100_0000;
/// Space marker.
pub const LIRC_MODE2_SPACE: u32 = 0x0000_0000;
/// Frequency event.
pub const LIRC_MODE2_FREQUENCY: u32 = 0x0200_0000;
/// Timeout event.
pub const LIRC_MODE2_TIMEOUT: u32 = 0x0300_0000;
/// Overflow event.
pub const LIRC_MODE2_OVERFLOW: u32 = 0x0400_0000;

// ---------------------------------------------------------------------------
// Send / receive modes (LIRC_GET_REC_MODE / LIRC_SET_REC_MODE)
// ---------------------------------------------------------------------------

/// Raw pulse/space samples (default for receive).
pub const LIRC_MODE_RAW: u32 = 0x0000_0001;
/// Decoded scancodes (one u32 per code).
pub const LIRC_MODE_PULSE: u32 = 0x0000_0002;
/// Mode-2 (pulse/space with metadata in high byte).
pub const LIRC_MODE_MODE2: u32 = 0x0000_0004;
/// Scancode mode (struct lirc_scancode).
pub const LIRC_MODE_SCANCODE: u32 = 0x0000_0008;
/// LIRC code (legacy fixed-size).
pub const LIRC_MODE_LIRCCODE: u32 = 0x0000_0010;

// ---------------------------------------------------------------------------
// Capability bits (LIRC_GET_FEATURES return value)
// ---------------------------------------------------------------------------

/// Can send raw.
pub const LIRC_CAN_SEND_RAW: u32 = LIRC_MODE_RAW;
/// Can send mode2.
pub const LIRC_CAN_SEND_MODE2: u32 = LIRC_MODE_MODE2;
/// Can send pulse/scancode.
pub const LIRC_CAN_SEND_PULSE: u32 = LIRC_MODE_PULSE;
/// Can set send carrier frequency.
pub const LIRC_CAN_SET_SEND_CARRIER: u32 = 0x0000_0100;
/// Can set send duty cycle.
pub const LIRC_CAN_SET_SEND_DUTY_CYCLE: u32 = 0x0000_0200;
/// Can set transmitter mask.
pub const LIRC_CAN_SET_TRANSMITTER_MASK: u32 = 0x0000_0400;
/// Can receive raw.
pub const LIRC_CAN_REC_RAW: u32 = LIRC_MODE_RAW << 16;
/// Can receive mode2.
pub const LIRC_CAN_REC_MODE2: u32 = LIRC_MODE_MODE2 << 16;
/// Can set receive carrier filter.
pub const LIRC_CAN_SET_REC_CARRIER: u32 = 0x0001_0000;
/// Can measure the carrier of an incoming signal.
pub const LIRC_CAN_MEASURE_CARRIER: u32 = 0x0200_0000;

// ---------------------------------------------------------------------------
// ioctl numbers
// ---------------------------------------------------------------------------

/// `LIRC_GET_FEATURES` — query capability bitmap.
pub const LIRC_GET_FEATURES: u32 = 0x8004_6900;
/// `LIRC_GET_SEND_MODE` — query the current send mode.
pub const LIRC_GET_SEND_MODE: u32 = 0x8004_6901;
/// `LIRC_GET_REC_MODE` — query the current receive mode.
pub const LIRC_GET_REC_MODE: u32 = 0x8004_6902;
/// `LIRC_SET_SEND_MODE` — set the send mode.
pub const LIRC_SET_SEND_MODE: u32 = 0x4004_6911;
/// `LIRC_SET_REC_MODE` — set the receive mode.
pub const LIRC_SET_REC_MODE: u32 = 0x4004_6912;
/// `LIRC_SET_SEND_CARRIER` — set the send-carrier frequency (Hz).
pub const LIRC_SET_SEND_CARRIER: u32 = 0x4004_6913;
/// `LIRC_SET_REC_CARRIER` — set the receive-carrier filter.
pub const LIRC_SET_REC_CARRIER: u32 = 0x4004_6914;
/// `LIRC_SET_SEND_DUTY_CYCLE` — set the duty cycle (1..99%).
pub const LIRC_SET_SEND_DUTY_CYCLE: u32 = 0x4004_6915;
/// `LIRC_SET_TRANSMITTER_MASK` — choose transmitters.
pub const LIRC_SET_TRANSMITTER_MASK: u32 = 0x4004_6917;
/// `LIRC_SET_REC_TIMEOUT` — set the receive timeout (µs).
pub const LIRC_SET_REC_TIMEOUT: u32 = 0x4004_6918;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_i() {
        assert_eq!(LIRC_IOC_MAGIC, b'i');
    }

    #[test]
    fn test_mode2_tags_distinct_and_in_high_byte() {
        // The metadata tag lives in bits 24..31; the pulse value
        // lives in LIRC_VALUE_MASK (bits 0..23). They must not
        // overlap.
        let tags = [
            LIRC_MODE2_SPACE,
            LIRC_MODE2_PULSE,
            LIRC_MODE2_FREQUENCY,
            LIRC_MODE2_TIMEOUT,
            LIRC_MODE2_OVERFLOW,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
            assert_eq!(tags[i] & LIRC_VALUE_MASK, 0);
        }
    }

    #[test]
    fn test_modes_distinct_pow2() {
        let m = [
            LIRC_MODE_RAW,
            LIRC_MODE_PULSE,
            LIRC_MODE_MODE2,
            LIRC_MODE_SCANCODE,
            LIRC_MODE_LIRCCODE,
        ];
        for &b in &m {
            assert!(b.is_power_of_two());
        }
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_send_can_bits_match_modes() {
        // CAN_SEND_* bits are the same as the corresponding mode bits;
        // CAN_REC_* live in the high half.
        assert_eq!(LIRC_CAN_SEND_RAW, LIRC_MODE_RAW);
        assert_eq!(LIRC_CAN_SEND_MODE2, LIRC_MODE_MODE2);
        assert_eq!(LIRC_CAN_REC_RAW, LIRC_MODE_RAW << 16);
        assert_eq!(LIRC_CAN_REC_MODE2, LIRC_MODE_MODE2 << 16);
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_i() {
        let ops = [
            LIRC_GET_FEATURES,
            LIRC_GET_SEND_MODE,
            LIRC_GET_REC_MODE,
            LIRC_SET_SEND_MODE,
            LIRC_SET_REC_MODE,
            LIRC_SET_SEND_CARRIER,
            LIRC_SET_REC_CARRIER,
            LIRC_SET_SEND_DUTY_CYCLE,
            LIRC_SET_TRANSMITTER_MASK,
            LIRC_SET_REC_TIMEOUT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'i' (0x69) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'i' as u32);
        }
    }
}
