//! `<linux/ax25.h>` — AX.25 amateur-radio link-layer address constants.
//!
//! AX.25 (X.25 over packet radio) addresses are 7-byte callsigns: six
//! shifted-ASCII characters plus a 1-byte SSID/control octet. The
//! Linux kernel handles them as `ax25_address` structures and exposes
//! a `AX25_*` family of address modifiers and SSID masks.

// ---------------------------------------------------------------------------
// Address shape
// ---------------------------------------------------------------------------

/// Callsign character count (excluding the SSID octet).
pub const AX25_ADDR_LEN: usize = 7;

/// SSID octet sits in the 7th address byte.
pub const AX25_ADDR_SSID_OFFSET: usize = 6;

/// Maximum digipeater hops permitted by the spec.
pub const AX25_MAX_DIGIS: usize = 8;

/// Net device hardware-type (`ARPHRD_AX25`).
pub const ARPHRD_AX25: u32 = 3;

/// Net device hardware-type for NET/ROM (uses AX.25 addressing).
pub const ARPHRD_NETROM: u32 = 0;

// ---------------------------------------------------------------------------
// SSID byte bit layout
// ---------------------------------------------------------------------------

/// HDLC end-address bit (last digipeater in the path).
pub const AX25_HBIT: u8 = 0x80;
/// "Has been repeated" bit (RR/digipeated flag).
pub const AX25_REPEATED: u8 = 0x80;
/// Command/response (high bit of source/dest SSID — flipped per role).
pub const AX25_CBIT: u8 = 0x80;
/// Reserved bits (always 0x60 in AX.25 v2 frames).
pub const AX25_RESERVED: u8 = 0x60;
/// SSID nibble (4 bits, values 0..15).
pub const AX25_SSID_MASK: u8 = 0x1E;
/// Extension bit — set on the final address byte.
pub const AX25_EBIT: u8 = 0x01;

// ---------------------------------------------------------------------------
// Standard callsigns
// ---------------------------------------------------------------------------

pub const AX25_NULL_CALL: &[u8; 7] = b"\x40\x40\x40\x40\x40\x40\x60";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_geometry() {
        // 6 callsign chars + 1 SSID byte = 7.
        assert_eq!(AX25_ADDR_LEN, 7);
        assert_eq!(AX25_ADDR_SSID_OFFSET, AX25_ADDR_LEN - 1);
        // Spec hard-caps digipeater hop count at 8.
        assert_eq!(AX25_MAX_DIGIS, 8);
        assert!(AX25_MAX_DIGIS.is_power_of_two());
    }

    #[test]
    fn test_hardware_types() {
        // ARPHRD_AX25=3 (canonical AX.25 link layer).
        assert_eq!(ARPHRD_AX25, 3);
        // NET/ROM piggybacks on AX.25 addressing but registers as
        // ARPHRD_NETROM=0 in Linux (the historic value).
        assert_eq!(ARPHRD_NETROM, 0);
    }

    #[test]
    fn test_ssid_bit_layout_disjoint() {
        // The single SSID byte is partitioned into:
        //   bit 7      = H/E/C bit (role-dependent)
        //   bits 6..5  = reserved (always 11)
        //   bits 4..1  = SSID nibble (0..15)
        //   bit 0      = extension bit
        assert_eq!(AX25_HBIT, 0x80);
        assert_eq!(AX25_REPEATED, 0x80);
        assert_eq!(AX25_CBIT, 0x80);
        assert_eq!(AX25_RESERVED, 0x60);
        assert_eq!(AX25_SSID_MASK, 0x1E);
        assert_eq!(AX25_EBIT, 0x01);
        // Disjoint partition of all 8 bits.
        let total = AX25_HBIT | AX25_RESERVED | AX25_SSID_MASK | AX25_EBIT;
        assert_eq!(total, 0xFF);
        assert_eq!(AX25_HBIT & AX25_RESERVED, 0);
        assert_eq!(AX25_RESERVED & AX25_SSID_MASK, 0);
        assert_eq!(AX25_SSID_MASK & AX25_EBIT, 0);
    }

    #[test]
    fn test_ssid_mask_holds_four_bits() {
        // SSID nibble is 4 bits, encoded in bits 4..1 (shifted by 1).
        let nibble = AX25_SSID_MASK >> 1;
        assert_eq!(nibble, 0x0F);
        assert_eq!(nibble.count_ones(), 4);
    }

    #[test]
    fn test_null_call_is_shifted_space_with_v2_ssid() {
        // AX.25 shifts ASCII left by 1 to free the LSB for the
        // extension bit. SPACE (0x20) becomes 0x40; the trailing SSID
        // byte for a v2 frame ends in 0x60 (reserved bits set).
        let mut expected = [0x40u8; 7];
        expected[6] = 0x60;
        assert_eq!(AX25_NULL_CALL, &expected);
        // Reserved bits must be present on the SSID octet.
        assert_eq!(AX25_NULL_CALL[6] & AX25_RESERVED, AX25_RESERVED);
        // Extension bit (LSB) is zero — this is *not* the last address.
        assert_eq!(AX25_NULL_CALL[6] & AX25_EBIT, 0);
        // SSID nibble of a null call is 0.
        assert_eq!(AX25_NULL_CALL[6] & AX25_SSID_MASK, 0);
    }
}
