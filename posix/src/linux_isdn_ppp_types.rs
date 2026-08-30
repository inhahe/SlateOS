//! `<linux/isdn_ppp.h>` — sync-PPP ISDN ioctl constants.
//!
//! The legacy ISDN-PPP driver provides PPP-over-ISDN multilink and
//! compression negotiation via `/dev/ipppN`. These constants cover
//! the ioctl numbers, multilink class flags, and CCP (Compression
//! Control Protocol) algorithm identifiers exchanged with userspace
//! pppd/ipppd.

// ---------------------------------------------------------------------------
// PPP multilink class identifiers
// ---------------------------------------------------------------------------

/// Multilink endpoint discriminator class: null.
pub const ISDN_PPP_MP_CLASS_NULL: u32 = 0;
/// Multilink endpoint discriminator class: local.
pub const ISDN_PPP_MP_CLASS_LOCAL: u32 = 1;
/// Multilink endpoint discriminator class: IP address.
pub const ISDN_PPP_MP_CLASS_IP: u32 = 2;
/// Multilink endpoint discriminator class: IEEE-802.1 MAC.
pub const ISDN_PPP_MP_CLASS_IEEE802_1: u32 = 3;
/// Multilink endpoint discriminator class: PPP magic-number block.
pub const ISDN_PPP_MP_CLASS_PPP_MAGIC: u32 = 4;
/// Multilink endpoint discriminator class: PSN (Public Switched Network).
pub const ISDN_PPP_MP_CLASS_PSN: u32 = 5;

// ---------------------------------------------------------------------------
// CCP compression algorithm IDs (mirrors RFC 1962 + IANA ppp-numbers)
// ---------------------------------------------------------------------------

/// OUI-encoded vendor-specific algorithm.
pub const ISDN_PPP_COMP_OUI: u8 = 0;
/// Predictor type 1.
pub const ISDN_PPP_COMP_PRED1: u8 = 1;
/// Predictor type 2.
pub const ISDN_PPP_COMP_PRED2: u8 = 2;
/// Puddle Jumper.
pub const ISDN_PPP_COMP_PJUMP: u8 = 3;
/// Stac LZS.
pub const ISDN_PPP_COMP_STAC: u8 = 17;
/// MPPC/MPPE (Microsoft PPC).
pub const ISDN_PPP_COMP_MPPC: u8 = 18;
/// Gandalf FZA.
pub const ISDN_PPP_COMP_FZA: u8 = 19;
/// BSD LZW.
pub const ISDN_PPP_COMP_BSD: u8 = 21;
/// Deflate (RFC 1979).
pub const ISDN_PPP_COMP_DEFLATE: u8 = 26;

// ---------------------------------------------------------------------------
// PPP multilink header bits (in the per-fragment header byte)
// ---------------------------------------------------------------------------

/// Beginning-of-fragment flag.
pub const ISDN_PPP_MP_BEGIN_FRAG: u32 = 1 << 7;
/// End-of-fragment flag.
pub const ISDN_PPP_MP_END_FRAG: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// ioctl base / numbers (struct ipppctrl_blk via IIOCNETxxx commands)
// ---------------------------------------------------------------------------

/// Magic byte identifying the ipppd ioctl group.
pub const ISDN_PPP_IOCTL_BASE: u8 = b't';

// ---------------------------------------------------------------------------
// MP receive queue limits
// ---------------------------------------------------------------------------

/// Number of fragments the multilink reassembly window holds.
pub const ISDN_PPP_MP_MAX_QUEUE: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mp_classes_distinct_and_zero_is_null() {
        let c = [
            ISDN_PPP_MP_CLASS_NULL,
            ISDN_PPP_MP_CLASS_LOCAL,
            ISDN_PPP_MP_CLASS_IP,
            ISDN_PPP_MP_CLASS_IEEE802_1,
            ISDN_PPP_MP_CLASS_PPP_MAGIC,
            ISDN_PPP_MP_CLASS_PSN,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // RFC 1990: class 0 means "null discriminator".
        assert_eq!(ISDN_PPP_MP_CLASS_NULL, 0);
    }

    #[test]
    fn test_ccp_alg_ids_distinct() {
        let a = [
            ISDN_PPP_COMP_OUI,
            ISDN_PPP_COMP_PRED1,
            ISDN_PPP_COMP_PRED2,
            ISDN_PPP_COMP_PJUMP,
            ISDN_PPP_COMP_STAC,
            ISDN_PPP_COMP_MPPC,
            ISDN_PPP_COMP_FZA,
            ISDN_PPP_COMP_BSD,
            ISDN_PPP_COMP_DEFLATE,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_mp_header_flag_bits() {
        assert!(ISDN_PPP_MP_BEGIN_FRAG.is_power_of_two());
        assert!(ISDN_PPP_MP_END_FRAG.is_power_of_two());
        assert_ne!(ISDN_PPP_MP_BEGIN_FRAG, ISDN_PPP_MP_END_FRAG);
        // Both flags must fit in a single byte (bits 6 and 7).
        assert!(ISDN_PPP_MP_BEGIN_FRAG <= 0xff);
        assert!(ISDN_PPP_MP_END_FRAG <= 0xff);
    }

    #[test]
    fn test_misc() {
        assert_eq!(ISDN_PPP_IOCTL_BASE, b't');
        assert!(ISDN_PPP_MP_MAX_QUEUE.is_power_of_two());
    }
}
