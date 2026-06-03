//! `<linux/mpls.h>` — MPLS (Multiprotocol Label Switching) constants.
//!
//! MPLS is a packet forwarding mechanism based on labels rather than
//! IP addresses. Linux supports MPLS routing for carrier-grade
//! networking. Managed via `ip -f mpls route` (iproute2).

// ---------------------------------------------------------------------------
// MPLS label constants
// ---------------------------------------------------------------------------

/// IPv4 Explicit NULL.
pub const MPLS_LABEL_IPV4NULL: u32 = 0;
/// Router Alert.
pub const MPLS_LABEL_RTALERT: u32 = 1;
/// IPv6 Explicit NULL.
pub const MPLS_LABEL_IPV6NULL: u32 = 2;
/// Implicit NULL (penultimate hop popping).
pub const MPLS_LABEL_IMPLNULL: u32 = 3;
/// Entropy Label Indicator.
pub const MPLS_LABEL_ENTROPY: u32 = 7;
/// Generic Associated Channel (GAL).
pub const MPLS_LABEL_GAL: u32 = 13;
/// OAM Alert.
pub const MPLS_LABEL_OAMALERT: u32 = 14;
/// Extension label.
pub const MPLS_LABEL_EXTENSION: u32 = 15;
/// First unreserved label.
pub const MPLS_LABEL_FIRST_UNRESERVED: u32 = 16;

// ---------------------------------------------------------------------------
// MPLS netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const MPLS_IPTUNNEL_UNSPEC: u16 = 0;
/// Destination label stack.
pub const MPLS_IPTUNNEL_DST: u16 = 1;
/// TTL propagation.
pub const MPLS_IPTUNNEL_TTL: u16 = 2;

// ---------------------------------------------------------------------------
// MPLS stats attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const MPLS_STATS_UNSPEC: u16 = 0;
/// Link stats.
pub const MPLS_STATS_LINK: u16 = 1;

// ---------------------------------------------------------------------------
// MPLS label stack entry helpers
// ---------------------------------------------------------------------------

/// Label mask (20 bits, shifted left 12).
pub const MPLS_LS_LABEL_MASK: u32 = 0xFFFFF000;
/// Label shift.
pub const MPLS_LS_LABEL_SHIFT: u32 = 12;
/// TC (Traffic Class) mask (3 bits).
pub const MPLS_LS_TC_MASK: u32 = 0x00000E00;
/// TC shift.
pub const MPLS_LS_TC_SHIFT: u32 = 9;
/// Bottom of stack mask (1 bit).
pub const MPLS_LS_S_MASK: u32 = 0x00000100;
/// Bottom of stack shift.
pub const MPLS_LS_S_SHIFT: u32 = 8;
/// TTL mask (8 bits).
pub const MPLS_LS_TTL_MASK: u32 = 0x000000FF;
/// TTL shift.
pub const MPLS_LS_TTL_SHIFT: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reserved_labels() {
        assert_eq!(MPLS_LABEL_IPV4NULL, 0);
        assert_eq!(MPLS_LABEL_IPV6NULL, 2);
        assert_eq!(MPLS_LABEL_IMPLNULL, 3);
        assert_eq!(MPLS_LABEL_FIRST_UNRESERVED, 16);
    }

    #[test]
    fn test_reserved_labels_distinct() {
        let labels = [
            MPLS_LABEL_IPV4NULL,
            MPLS_LABEL_RTALERT,
            MPLS_LABEL_IPV6NULL,
            MPLS_LABEL_IMPLNULL,
            MPLS_LABEL_ENTROPY,
            MPLS_LABEL_GAL,
            MPLS_LABEL_OAMALERT,
            MPLS_LABEL_EXTENSION,
        ];
        for i in 0..labels.len() {
            for j in (i + 1)..labels.len() {
                assert_ne!(labels[i], labels[j]);
            }
        }
    }

    #[test]
    fn test_masks_non_overlapping() {
        // Label, TC, S, TTL masks should not overlap.
        assert_eq!(MPLS_LS_LABEL_MASK & MPLS_LS_TC_MASK, 0);
        assert_eq!(MPLS_LS_TC_MASK & MPLS_LS_S_MASK, 0);
        assert_eq!(MPLS_LS_S_MASK & MPLS_LS_TTL_MASK, 0);
    }

    #[test]
    fn test_full_label_entry() {
        // All masks together should cover 32 bits.
        let combined = MPLS_LS_LABEL_MASK | MPLS_LS_TC_MASK | MPLS_LS_S_MASK | MPLS_LS_TTL_MASK;
        assert_eq!(combined, 0xFFFFFFFF);
    }
}
