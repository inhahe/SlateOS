//! `<linux/mpls.h>` — Additional MPLS constants.
//!
//! Supplementary MPLS routing constants covering attribute types,
//! label values, and statistics types.

// ---------------------------------------------------------------------------
// MPLS routing attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const MPLS_IPTUNNEL_UNSPEC: u32 = 0;
/// Destination.
pub const MPLS_IPTUNNEL_DST: u32 = 1;
/// TTL.
pub const MPLS_IPTUNNEL_TTL: u32 = 2;

// ---------------------------------------------------------------------------
// MPLS label special values
// ---------------------------------------------------------------------------

/// IPv4 explicit null.
pub const MPLS_LABEL_IPV4NULL: u32 = 0;
/// Router alert.
pub const MPLS_LABEL_RTALERT: u32 = 1;
/// IPv6 explicit null.
pub const MPLS_LABEL_IPV6NULL: u32 = 2;
/// Implicit null.
pub const MPLS_LABEL_IMPLNULL: u32 = 3;
/// ELI (entropy label indicator).
pub const MPLS_LABEL_ENTROPY: u32 = 7;
/// GAL (generic associated channel).
pub const MPLS_LABEL_GAL: u32 = 13;
/// OAM alert.
pub const MPLS_LABEL_OAMALERT: u32 = 14;
/// Extension.
pub const MPLS_LABEL_EXTENSION: u32 = 15;
/// First unreserved label.
pub const MPLS_LABEL_FIRST_UNRESERVED: u32 = 16;

// ---------------------------------------------------------------------------
// MPLS stats attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const MPLS_STATS_UNSPEC: u32 = 0;
/// Link stats.
pub const MPLS_STATS_LINK: u32 = 1;

// ---------------------------------------------------------------------------
// MPLS route attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const RTA_MPLS_UNSPEC: u32 = 0;
/// TTL propagate.
pub const RTA_MPLS_TTL_PROPAGATE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iptunnel_attrs_distinct() {
        let attrs = [MPLS_IPTUNNEL_UNSPEC, MPLS_IPTUNNEL_DST, MPLS_IPTUNNEL_TTL];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_labels_distinct() {
        let labels = [
            MPLS_LABEL_IPV4NULL,
            MPLS_LABEL_RTALERT,
            MPLS_LABEL_IPV6NULL,
            MPLS_LABEL_IMPLNULL,
            MPLS_LABEL_ENTROPY,
            MPLS_LABEL_GAL,
            MPLS_LABEL_OAMALERT,
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
    fn test_first_unreserved_after_extension() {
        assert!(MPLS_LABEL_FIRST_UNRESERVED > MPLS_LABEL_EXTENSION);
    }

    #[test]
    fn test_stats_attrs_distinct() {
        assert_ne!(MPLS_STATS_UNSPEC, MPLS_STATS_LINK);
    }

    #[test]
    fn test_rta_attrs_distinct() {
        assert_ne!(RTA_MPLS_UNSPEC, RTA_MPLS_TTL_PROPAGATE);
    }
}
