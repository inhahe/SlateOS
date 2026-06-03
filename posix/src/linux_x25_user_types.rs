//! `<linux/x25.h>` — X.25 packet-layer protocol sockets.
//!
//! X.25 is a legacy ITU-T packet-switching protocol still used by
//! aviation (ATN) and banking-style fixed networks. Linux exposes it
//! as `AF_X25` sockets with a 15-digit destination "address" (NSAP).

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

pub const AF_X25: u32 = 9;
pub const PF_X25: u32 = AF_X25;

// ---------------------------------------------------------------------------
// `setsockopt(SOL_X25, …)` levels and options
// ---------------------------------------------------------------------------

pub const SOL_X25: u32 = 262;

pub const X25_QBITINCL: u32 = 1;

// ---------------------------------------------------------------------------
// `struct sockaddr_x25` field sizes
// ---------------------------------------------------------------------------

/// X.121 address is 15 BCD digits → 16 chars with NUL.
pub const X25_ADDR_LEN: usize = 16;

/// Maximum X.25 packet payload (bytes). The protocol caps it at 4096.
pub const X25_MAX_PACKET_SIZE: usize = 4096;
pub const X25_DEFAULT_PACKET_SIZE: usize = 128;

// ---------------------------------------------------------------------------
// Cause codes (`X25_CAUSE_*`)
// ---------------------------------------------------------------------------

pub const X25_CAUSE_NO_REASON: u8 = 0x00;
pub const X25_CAUSE_NUMBER_BUSY: u8 = 0x01;
pub const X25_CAUSE_OUT_OF_ORDER: u8 = 0x09;
pub const X25_CAUSE_REMOTE_PROCEDURE_ERROR: u8 = 0x11;
pub const X25_CAUSE_REVERSE_CHARGE_REFUSED: u8 = 0x19;
pub const X25_CAUSE_INVALID_CALL: u8 = 0x21;
pub const X25_CAUSE_ACCESS_BARRED: u8 = 0x0B;
pub const X25_CAUSE_NOT_OBTAINABLE: u8 = 0x0D;

// ---------------------------------------------------------------------------
// Facility codes (`X25_FAC_*`)
// ---------------------------------------------------------------------------

pub const X25_FAC_REVERSE: u8 = 0x01;
pub const X25_FAC_THROUGHPUT: u8 = 0x02;
pub const X25_FAC_PACKET_SIZE: u8 = 0x42;
pub const X25_FAC_WINDOW_SIZE: u8 = 0x43;
pub const X25_FAC_CALLING_AE: u8 = 0xCB;
pub const X25_FAC_CALLED_AE: u8 = 0xCA;

// ---------------------------------------------------------------------------
// Default window size and T-timers (centiseconds)
// ---------------------------------------------------------------------------

pub const X25_DEFAULT_WINDOW_SIZE: u8 = 2;
pub const X25_T20_DEFAULT_CS: u32 = 18_000; // 180 s
pub const X25_T21_DEFAULT_CS: u32 = 20_000; // 200 s
pub const X25_T22_DEFAULT_CS: u32 = 18_000;
pub const X25_T23_DEFAULT_CS: u32 = 18_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_x25_is_9() {
        // AF_X25 was assigned 9 in linux/socket.h.
        assert_eq!(AF_X25, 9);
        assert_eq!(PF_X25, AF_X25);
        assert_eq!(SOL_X25, 262);
    }

    #[test]
    fn test_addr_layout() {
        // 15 BCD digits + NUL = 16 bytes.
        assert_eq!(X25_ADDR_LEN, 16);
    }

    #[test]
    fn test_packet_size_bounds() {
        // Default packet size 128 (the X.25 standard recommendation).
        assert_eq!(X25_DEFAULT_PACKET_SIZE, 128);
        // Maximum is 4096 (2^12), the upper end of the negotiable range.
        assert_eq!(X25_MAX_PACKET_SIZE, 4096);
        assert!(X25_DEFAULT_PACKET_SIZE.is_power_of_two());
        assert!(X25_MAX_PACKET_SIZE.is_power_of_two());
        assert!(X25_DEFAULT_PACKET_SIZE < X25_MAX_PACKET_SIZE);
    }

    #[test]
    fn test_cause_codes_distinct() {
        let c = [
            X25_CAUSE_NO_REASON,
            X25_CAUSE_NUMBER_BUSY,
            X25_CAUSE_OUT_OF_ORDER,
            X25_CAUSE_REMOTE_PROCEDURE_ERROR,
            X25_CAUSE_REVERSE_CHARGE_REFUSED,
            X25_CAUSE_INVALID_CALL,
            X25_CAUSE_ACCESS_BARRED,
            X25_CAUSE_NOT_OBTAINABLE,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_facility_codes_in_two_classes() {
        // Class A facilities (single-byte parameter) start at 0x01,
        // class C facilities (two-byte parameter) start at 0x42, and
        // 0xCx are 4-byte facility codes.
        assert!(X25_FAC_REVERSE < 0x40);
        assert!(X25_FAC_THROUGHPUT < 0x40);
        assert!(X25_FAC_PACKET_SIZE >= 0x40 && X25_FAC_PACKET_SIZE < 0x80);
        assert!(X25_FAC_CALLING_AE >= 0xC0);
        assert!(X25_FAC_CALLED_AE >= 0xC0);
    }

    #[test]
    fn test_default_window_and_t20_t21() {
        // Default window of 2 (the lowest, most conservative value).
        assert_eq!(X25_DEFAULT_WINDOW_SIZE, 2);
        // T20 (restart) and T22 (reset) — 180 s.
        assert_eq!(X25_T20_DEFAULT_CS, 180 * 100);
        // T21 (call setup) — 200 s.
        assert_eq!(X25_T21_DEFAULT_CS, 200 * 100);
        assert_eq!(X25_T22_DEFAULT_CS, X25_T20_DEFAULT_CS);
        assert_eq!(X25_T23_DEFAULT_CS, X25_T20_DEFAULT_CS);
    }
}
