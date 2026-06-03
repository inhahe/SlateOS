//! `<linux/mctp.h>` / `drivers/net/mctp/mctp-serial.c` — MCTP-over-serial.
//!
//! MCTP (Management Component Transport Protocol) over an async serial
//! line uses a HDLC-like framing layer defined by DMTF DSP0253. These
//! constants cover frame delimiters, byte stuffing, framing limits, and
//! the FCS polynomial used by the kernel driver.

// ---------------------------------------------------------------------------
// Framing bytes (DSP0253 §3)
// ---------------------------------------------------------------------------

/// Frame begin / end flag byte.
pub const MCTP_SERIAL_FLAG: u8 = 0x7e;
/// Escape (byte-stuffing) byte.
pub const MCTP_SERIAL_ESC: u8 = 0x7d;
/// XOR applied to an escaped byte (flag or esc).
pub const MCTP_SERIAL_ESC_XOR: u8 = 0x20;
/// Revision byte placed in the frame header.
pub const MCTP_SERIAL_REVISION: u8 = 0x01;

// ---------------------------------------------------------------------------
// Frame size limits
// ---------------------------------------------------------------------------

/// Maximum MCTP payload that fits one serial frame (256 bytes, the
/// kernel's `MCTP_SERIAL_MTU`).
pub const MCTP_SERIAL_MTU: u32 = 256;

/// Maximum encoded frame length on the wire — payload + 2 flag bytes +
/// 1 revision + 1 length + 2 FCS, all worst-case-doubled by stuffing.
pub const MCTP_SERIAL_FRAME_MAX: u32 = (MCTP_SERIAL_MTU + 6) * 2;

/// Minimum well-formed frame: flag + rev + len + (zero payload) + fcs(2)
/// + flag.
pub const MCTP_SERIAL_FRAME_MIN: u32 = 6;

// ---------------------------------------------------------------------------
// FCS (CRC-16/X.25, DSP0253 §3.5)
// ---------------------------------------------------------------------------

/// Reflected polynomial used by the FCS (`x^16 + x^12 + x^5 + 1`).
pub const MCTP_SERIAL_FCS_POLY: u16 = 0x8408;
/// Initial / final XOR value (RFC 1662 / DSP0253 use this seed).
pub const MCTP_SERIAL_FCS_INIT: u16 = 0xffff;
/// Magic residue: FCS over a valid frame including its own FCS equals
/// this value.
pub const MCTP_SERIAL_FCS_GOOD: u16 = 0xf0b8;

// ---------------------------------------------------------------------------
// Decoder state machine
// ---------------------------------------------------------------------------

/// Waiting for the opening flag.
pub const MCTP_SERIAL_STATE_WAIT_SYNC: u32 = 0;
/// Reading the revision byte.
pub const MCTP_SERIAL_STATE_REVISION: u32 = 1;
/// Reading the length byte.
pub const MCTP_SERIAL_STATE_LENGTH: u32 = 2;
/// Accumulating payload bytes.
pub const MCTP_SERIAL_STATE_DATA: u32 = 3;
/// Reading the two FCS bytes.
pub const MCTP_SERIAL_STATE_FCS: u32 = 4;
/// Reading an escape sequence (XOR with 0x20 to recover).
pub const MCTP_SERIAL_STATE_ESC: u32 = 5;
/// Frame complete; awaiting closing flag.
pub const MCTP_SERIAL_STATE_DONE: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_and_esc_are_dsp0253_values() {
        // 0x7e (flag) and 0x7d (esc) are the HDLC-derived bytes the
        // standard mandates.
        assert_eq!(MCTP_SERIAL_FLAG, 0x7e);
        assert_eq!(MCTP_SERIAL_ESC, 0x7d);
        assert_ne!(MCTP_SERIAL_FLAG, MCTP_SERIAL_ESC);
        // Escape-XOR distinguishes the two stuffed bytes from each other.
        assert_ne!(MCTP_SERIAL_FLAG ^ MCTP_SERIAL_ESC_XOR, MCTP_SERIAL_FLAG);
        assert_ne!(MCTP_SERIAL_ESC ^ MCTP_SERIAL_ESC_XOR, MCTP_SERIAL_ESC);
    }

    #[test]
    fn test_frame_size_bounds() {
        assert!(MCTP_SERIAL_FRAME_MIN < MCTP_SERIAL_FRAME_MAX);
        // Worst-case stuffed frame must be at least double the MTU.
        assert!(MCTP_SERIAL_FRAME_MAX >= MCTP_SERIAL_MTU * 2);
    }

    #[test]
    fn test_fcs_constants() {
        // The reflected CRC-16/X.25 polynomial is 0x8408.
        assert_eq!(MCTP_SERIAL_FCS_POLY, 0x8408);
        assert_eq!(MCTP_SERIAL_FCS_INIT, 0xffff);
        // Residue must not coincide with init — that would defeat the
        // self-check.
        assert_ne!(MCTP_SERIAL_FCS_GOOD, MCTP_SERIAL_FCS_INIT);
    }

    #[test]
    fn test_states_distinct_and_ordered() {
        let s = [
            MCTP_SERIAL_STATE_WAIT_SYNC,
            MCTP_SERIAL_STATE_REVISION,
            MCTP_SERIAL_STATE_LENGTH,
            MCTP_SERIAL_STATE_DATA,
            MCTP_SERIAL_STATE_FCS,
            MCTP_SERIAL_STATE_ESC,
            MCTP_SERIAL_STATE_DONE,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
        // The wait state must be 0 so a zeroed decoder starts cold.
        assert_eq!(MCTP_SERIAL_STATE_WAIT_SYNC, 0);
    }
}
