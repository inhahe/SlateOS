//! `<linux/mpls.h>` — MPLS (Multiprotocol Label Switching) constants.
//!
//! MPLS is a label-based forwarding mechanism used in carrier and
//! data center networks. Instead of looking up the destination IP
//! at each hop, routers push/swap/pop 20-bit labels in a shim header.
//! Linux supports MPLS forwarding since 4.1 and MPLS-in-UDP/GRE
//! encapsulation. Netlink configures MPLS routes and label tables.
//! Used by MPLS-enabled routers, VPN (L3VPN/L2VPN), and segment
//! routing.

// ---------------------------------------------------------------------------
// MPLS label values (well-known / reserved)
// ---------------------------------------------------------------------------

/// IPv4 Explicit NULL label (pop and forward as IPv4).
pub const MPLS_LABEL_IPV4NULL: u32 = 0;
/// Router Alert label.
pub const MPLS_LABEL_RTALERT: u32 = 1;
/// IPv6 Explicit NULL label (pop and forward as IPv6).
pub const MPLS_LABEL_IPV6NULL: u32 = 2;
/// Implicit NULL label (penultimate hop popping).
pub const MPLS_LABEL_IMPLNULL: u32 = 3;
/// Entropy Label Indicator (RFC 6790).
pub const MPLS_LABEL_ENTROPY: u32 = 7;
/// Generic Associated Channel (GAL, RFC 5586).
pub const MPLS_LABEL_GAL: u32 = 13;
/// OAM Alert label (RFC 6428).
pub const MPLS_LABEL_OAM: u32 = 14;
/// Extension label (RFC 7274).
pub const MPLS_LABEL_EXTENSION: u32 = 15;
/// First unreserved label value.
pub const MPLS_LABEL_FIRST_UNRESERVED: u32 = 16;

// ---------------------------------------------------------------------------
// MPLS header bit positions
// ---------------------------------------------------------------------------

/// Label field shift (bits 12-31).
pub const MPLS_LS_LABEL_SHIFT: u32 = 12;
/// Label field mask.
pub const MPLS_LS_LABEL_MASK: u32 = 0xFFFFF000;
/// TC (Traffic Class) field shift (bits 9-11).
pub const MPLS_LS_TC_SHIFT: u32 = 9;
/// TC field mask.
pub const MPLS_LS_TC_MASK: u32 = 0x0000_0E00;
/// Bottom-of-Stack bit shift (bit 8).
pub const MPLS_LS_S_SHIFT: u32 = 8;
/// Bottom-of-Stack mask.
pub const MPLS_LS_S_MASK: u32 = 0x0000_0100;
/// TTL field shift (bits 0-7).
pub const MPLS_LS_TTL_SHIFT: u32 = 0;
/// TTL field mask.
pub const MPLS_LS_TTL_MASK: u32 = 0x0000_00FF;

// ---------------------------------------------------------------------------
// MPLS route netlink attributes (RTA_*)
// ---------------------------------------------------------------------------

/// Route destination (label value).
pub const MPLS_RTA_DST: u32 = 1;
/// Route newdst (replacement label stack for swap).
pub const MPLS_RTA_NEWDST: u32 = 2;
/// Via (next hop address) attribute.
pub const MPLS_RTA_VIA: u32 = 3;
/// TTL propagation attribute.
pub const MPLS_RTA_TTL_PROPAGATE: u32 = 4;

// ---------------------------------------------------------------------------
// MPLS iptunnel attributes
// ---------------------------------------------------------------------------

/// Output label stack for encapsulation.
pub const MPLS_IPTUNNEL_DST: u32 = 1;
/// TTL for encapsulated packets.
pub const MPLS_IPTUNNEL_TTL: u32 = 2;

// ---------------------------------------------------------------------------
// MPLS label stack maximum
// ---------------------------------------------------------------------------

/// Maximum label stack depth.
pub const MAX_MPLS_LABEL_STACK_DEPTH: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reserved_labels_distinct() {
        let labels = [
            MPLS_LABEL_IPV4NULL, MPLS_LABEL_RTALERT,
            MPLS_LABEL_IPV6NULL, MPLS_LABEL_IMPLNULL,
            MPLS_LABEL_ENTROPY, MPLS_LABEL_GAL,
            MPLS_LABEL_OAM, MPLS_LABEL_EXTENSION,
        ];
        for i in 0..labels.len() {
            for j in (i + 1)..labels.len() {
                assert_ne!(labels[i], labels[j]);
            }
        }
    }

    #[test]
    fn test_reserved_labels_below_first_unreserved() {
        assert!(MPLS_LABEL_IPV4NULL < MPLS_LABEL_FIRST_UNRESERVED);
        assert!(MPLS_LABEL_RTALERT < MPLS_LABEL_FIRST_UNRESERVED);
        assert!(MPLS_LABEL_IPV6NULL < MPLS_LABEL_FIRST_UNRESERVED);
        assert!(MPLS_LABEL_IMPLNULL < MPLS_LABEL_FIRST_UNRESERVED);
        assert!(MPLS_LABEL_ENTROPY < MPLS_LABEL_FIRST_UNRESERVED);
        assert!(MPLS_LABEL_GAL < MPLS_LABEL_FIRST_UNRESERVED);
        assert!(MPLS_LABEL_OAM < MPLS_LABEL_FIRST_UNRESERVED);
        assert!(MPLS_LABEL_EXTENSION < MPLS_LABEL_FIRST_UNRESERVED);
        assert_eq!(MPLS_LABEL_FIRST_UNRESERVED, 16);
    }

    #[test]
    fn test_header_masks_no_overlap() {
        assert_eq!(MPLS_LS_LABEL_MASK & MPLS_LS_TC_MASK, 0);
        assert_eq!(MPLS_LS_TC_MASK & MPLS_LS_S_MASK, 0);
        assert_eq!(MPLS_LS_S_MASK & MPLS_LS_TTL_MASK, 0);
        assert_eq!(MPLS_LS_LABEL_MASK & MPLS_LS_TTL_MASK, 0);
    }

    #[test]
    fn test_header_masks_cover_all_bits() {
        let combined = MPLS_LS_LABEL_MASK | MPLS_LS_TC_MASK
            | MPLS_LS_S_MASK | MPLS_LS_TTL_MASK;
        assert_eq!(combined, 0xFFFF_FFFF);
    }

    #[test]
    fn test_rta_attrs_distinct() {
        let attrs = [
            MPLS_RTA_DST, MPLS_RTA_NEWDST,
            MPLS_RTA_VIA, MPLS_RTA_TTL_PROPAGATE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_iptunnel_attrs_distinct() {
        assert_ne!(MPLS_IPTUNNEL_DST, MPLS_IPTUNNEL_TTL);
    }

    #[test]
    fn test_max_stack_depth() {
        assert_eq!(MAX_MPLS_LABEL_STACK_DEPTH, 16);
        assert!(MAX_MPLS_LABEL_STACK_DEPTH > 0);
    }
}
