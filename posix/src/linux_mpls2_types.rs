//! `<linux/mpls.h>` — Additional MPLS constants.
//!
//! Supplementary MPLS constants covering label values,
//! header fields, and traffic class encoding.

// ---------------------------------------------------------------------------
// MPLS special label values
// ---------------------------------------------------------------------------

/// IPv4 Explicit NULL.
pub const MPLS_LABEL_IPV4_NULL: u32 = 0;
/// Router Alert.
pub const MPLS_LABEL_ROUTER_ALERT: u32 = 1;
/// IPv6 Explicit NULL.
pub const MPLS_LABEL_IPV6_NULL: u32 = 2;
/// Implicit NULL (PHP: Penultimate Hop Popping).
pub const MPLS_LABEL_IMPLICIT_NULL: u32 = 3;
/// ELI (Entropy Label Indicator).
pub const MPLS_LABEL_ENTROPY_INDICATOR: u32 = 7;
/// GAL (Generic Associated Channel Label).
pub const MPLS_LABEL_GAL: u32 = 13;
/// OAM Alert Label.
pub const MPLS_LABEL_OAM_ALERT: u32 = 14;
/// Extension label.
pub const MPLS_LABEL_EXTENSION: u32 = 15;
/// First unreserved label.
pub const MPLS_LABEL_FIRST_UNRESERVED: u32 = 16;
/// Maximum label value.
pub const MPLS_LABEL_MAX: u32 = (1 << 20) - 1;

// ---------------------------------------------------------------------------
// MPLS header field encoding
// ---------------------------------------------------------------------------

/// Label shift in MPLS header.
pub const MPLS_LS_LABEL_SHIFT: u32 = 12;
/// Label mask (20 bits).
pub const MPLS_LS_LABEL_MASK: u32 = 0xFFFFF000;
/// Traffic class shift.
pub const MPLS_LS_TC_SHIFT: u32 = 9;
/// Traffic class mask (3 bits).
pub const MPLS_LS_TC_MASK: u32 = 0x00000E00;
/// Bottom of stack bit shift.
pub const MPLS_LS_S_SHIFT: u32 = 8;
/// Bottom of stack mask (1 bit).
pub const MPLS_LS_S_MASK: u32 = 0x00000100;
/// TTL shift.
pub const MPLS_LS_TTL_SHIFT: u32 = 0;
/// TTL mask (8 bits).
pub const MPLS_LS_TTL_MASK: u32 = 0x000000FF;

// ---------------------------------------------------------------------------
// MPLS header size
// ---------------------------------------------------------------------------

/// Size of one MPLS label entry (4 bytes).
pub const MPLS_HLEN: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_labels_distinct() {
        let labels = [
            MPLS_LABEL_IPV4_NULL,
            MPLS_LABEL_ROUTER_ALERT,
            MPLS_LABEL_IPV6_NULL,
            MPLS_LABEL_IMPLICIT_NULL,
            MPLS_LABEL_ENTROPY_INDICATOR,
            MPLS_LABEL_GAL,
            MPLS_LABEL_OAM_ALERT,
            MPLS_LABEL_EXTENSION,
            MPLS_LABEL_FIRST_UNRESERVED,
        ];
        for i in 0..labels.len() {
            for j in (i + 1)..labels.len() {
                assert_ne!(labels[i], labels[j]);
            }
        }
    }

    #[test]
    fn test_max_label() {
        assert_eq!(MPLS_LABEL_MAX, 0xFFFFF);
    }

    #[test]
    fn test_header_fields_no_overlap() {
        assert_eq!(MPLS_LS_LABEL_MASK & MPLS_LS_TC_MASK, 0);
        assert_eq!(MPLS_LS_TC_MASK & MPLS_LS_S_MASK, 0);
        assert_eq!(MPLS_LS_S_MASK & MPLS_LS_TTL_MASK, 0);
    }

    #[test]
    fn test_header_fields_cover_32_bits() {
        let all = MPLS_LS_LABEL_MASK | MPLS_LS_TC_MASK | MPLS_LS_S_MASK | MPLS_LS_TTL_MASK;
        assert_eq!(all, 0xFFFFFFFF);
    }

    #[test]
    fn test_hlen() {
        assert_eq!(MPLS_HLEN, 4);
    }

    #[test]
    fn test_first_unreserved() {
        assert_eq!(MPLS_LABEL_FIRST_UNRESERVED, 16);
    }
}
