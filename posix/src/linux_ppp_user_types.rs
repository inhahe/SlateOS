//! `<linux/ppp_defs.h>` — PPP HDLC framing and protocol numbers.
//!
//! Even though dial-up PPP is mostly historical, the same framing
//! turns up in mobile (3G/4G PPP-over-USB modems), L2TP, and PPPoE
//! over Ethernet. `pppd`, `accel-ppp`, and the kernel's
//! `drivers/net/ppp/` use these constants when wrapping/unwrapping
//! HDLC frames.

// ---------------------------------------------------------------------------
// HDLC framing bytes
// ---------------------------------------------------------------------------

/// Frame delimiter (0x7E) — start and end of every async-HDLC frame.
pub const PPP_FLAG: u8 = 0x7E;
/// Escape byte (0x7D) — XOR's the following byte with 0x20.
pub const PPP_ESCAPE: u8 = 0x7D;
/// All-stations broadcast address used on point-to-point links.
pub const PPP_ADDRESS: u8 = 0xFF;
/// Unnumbered information control byte.
pub const PPP_CONTROL: u8 = 0x03;

/// `PPP_FLAG ^ PPP_TRANS = 0x5E` — the escape transform constant.
pub const PPP_TRANS: u8 = 0x20;

// ---------------------------------------------------------------------------
// PPP protocol numbers (`<linux/ppp_defs.h>` `PPP_*` IDs)
// ---------------------------------------------------------------------------

pub const PPP_IP: u16 = 0x0021;
pub const PPP_AT: u16 = 0x0029;
pub const PPP_IPX: u16 = 0x002B;
pub const PPP_VJC_COMP: u16 = 0x002D;
pub const PPP_VJC_UNCOMP: u16 = 0x002F;
pub const PPP_MP: u16 = 0x003D;
pub const PPP_IPV6: u16 = 0x0057;
pub const PPP_COMPFRAG: u16 = 0x00FB;
pub const PPP_COMP: u16 = 0x00FD;
pub const PPP_MPLS_UC: u16 = 0x0281;
pub const PPP_MPLS_MC: u16 = 0x0283;
pub const PPP_IPCP: u16 = 0x8021;
pub const PPP_ATCP: u16 = 0x8029;
pub const PPP_IPXCP: u16 = 0x802B;
pub const PPP_IPV6CP: u16 = 0x8057;
pub const PPP_CCPFRAG: u16 = 0x80FB;
pub const PPP_CCP: u16 = 0x80FD;
pub const PPP_MPLSCP: u16 = 0x8281;
pub const PPP_LCP: u16 = 0xC021;
pub const PPP_PAP: u16 = 0xC023;
pub const PPP_LQR: u16 = 0xC025;
pub const PPP_CHAP: u16 = 0xC223;
pub const PPP_CBCP: u16 = 0xC029;

// ---------------------------------------------------------------------------
// Async control character map (`asyncmap`)
// ---------------------------------------------------------------------------

/// Default ACCM — escape every control character.
pub const PPP_ACCM_DEFAULT: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// FCS / MRU defaults
// ---------------------------------------------------------------------------

/// CRC-16 used for the frame-check sequence.
pub const PPP_FCS_GOOD: u16 = 0xF0B8;
/// Default MRU (maximum receive unit) per RFC 1661 §1.4.
pub const PPP_MRU: u32 = 1500;
/// Minimum allowable MRU.
pub const PPP_MTU_MIN: u32 = 68;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hdlc_framing_constants() {
        assert_eq!(PPP_FLAG, 0x7E);
        assert_eq!(PPP_ESCAPE, 0x7D);
        assert_eq!(PPP_ADDRESS, 0xFF);
        assert_eq!(PPP_CONTROL, 0x03);
        assert_eq!(PPP_TRANS, 0x20);
        // The escape transform turns FLAG into 0x5E and ESCAPE into 0x5D.
        assert_eq!(PPP_FLAG ^ PPP_TRANS, 0x5E);
        assert_eq!(PPP_ESCAPE ^ PPP_TRANS, 0x5D);
    }

    #[test]
    fn test_network_protocols_in_0x00_range() {
        // Network protocols (IP, IPv6, IPX, etc.) have the high byte at 0
        // and the low byte odd.
        let n = [PPP_IP, PPP_AT, PPP_IPX, PPP_IPV6, PPP_MPLS_UC, PPP_MPLS_MC];
        for &v in n.iter() {
            // Some MPLS ones live at 0x02xx — still in the network class
            // (high nibble 0).
            assert!((v >> 12) == 0);
            // Low byte is odd for protocol assignments per RFC 1700.
            assert_eq!(v & 1, 1);
        }
    }

    #[test]
    fn test_control_protocols_in_high_range() {
        // Control protocols use 0x8xxx (NCP) or 0xCxxx (LCP/auth).
        assert_eq!(PPP_IPCP, 0x8021);
        assert_eq!(PPP_IPV6CP, 0x8057);
        assert_eq!(PPP_LCP, 0xC021);
        assert_eq!(PPP_PAP, 0xC023);
        assert_eq!(PPP_CHAP, 0xC223);
        // NCP protocols share the low 13 bits with their network protocol.
        assert_eq!(PPP_IPCP & 0x1FFF, PPP_IP & 0x1FFF);
        assert_eq!(PPP_IPV6CP & 0x1FFF, PPP_IPV6 & 0x1FFF);
    }

    #[test]
    fn test_default_accm_is_all_ones() {
        assert_eq!(PPP_ACCM_DEFAULT, u32::MAX);
    }

    #[test]
    fn test_mtu_bounds_match_rfc_1661() {
        assert_eq!(PPP_MRU, 1500);
        assert_eq!(PPP_MTU_MIN, 68);
        assert!(PPP_MTU_MIN < PPP_MRU);
        // FCS-16 "good" residue per HDLC.
        assert_eq!(PPP_FCS_GOOD, 0xF0B8);
    }
}
