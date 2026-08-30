//! `<linux/lwtunnel.h>` — Additional lightweight tunnel constants.
//!
//! Supplementary lightweight tunnel constants covering encapsulation
//! types, attribute types, and flags.

// ---------------------------------------------------------------------------
// Lightweight tunnel encapsulation types
// ---------------------------------------------------------------------------

/// Unspec.
pub const LWTUNNEL_ENCAP_NONE: u32 = 0;
/// MPLS.
pub const LWTUNNEL_ENCAP_MPLS: u32 = 1;
/// IP.
pub const LWTUNNEL_ENCAP_IP: u32 = 2;
/// ILA.
pub const LWTUNNEL_ENCAP_ILA: u32 = 3;
/// IPv6.
pub const LWTUNNEL_ENCAP_IP6: u32 = 4;
/// SEG6.
pub const LWTUNNEL_ENCAP_SEG6: u32 = 5;
/// BPF.
pub const LWTUNNEL_ENCAP_BPF: u32 = 6;
/// SEG6 local.
pub const LWTUNNEL_ENCAP_SEG6_LOCAL: u32 = 7;
/// RPL.
pub const LWTUNNEL_ENCAP_RPL: u32 = 8;
/// IO access manager.
pub const LWTUNNEL_ENCAP_IOAM6: u32 = 9;
/// XFRM.
pub const LWTUNNEL_ENCAP_XFRM: u32 = 10;

// ---------------------------------------------------------------------------
// Lightweight tunnel IP attributes
// ---------------------------------------------------------------------------

/// Unspec.
pub const LWTUNNEL_IP_UNSPEC: u32 = 0;
/// Tunnel ID.
pub const LWTUNNEL_IP_ID: u32 = 1;
/// Destination address.
pub const LWTUNNEL_IP_DST: u32 = 2;
/// Source address.
pub const LWTUNNEL_IP_SRC: u32 = 3;
/// TTL.
pub const LWTUNNEL_IP_TTL: u32 = 4;
/// TOS.
pub const LWTUNNEL_IP_TOS: u32 = 5;
/// Flags.
pub const LWTUNNEL_IP_FLAGS: u32 = 6;

// ---------------------------------------------------------------------------
// Lightweight tunnel IP6 attributes
// ---------------------------------------------------------------------------

/// Unspec.
pub const LWTUNNEL_IP6_UNSPEC: u32 = 0;
/// Tunnel ID.
pub const LWTUNNEL_IP6_ID: u32 = 1;
/// Destination address.
pub const LWTUNNEL_IP6_DST: u32 = 2;
/// Source address.
pub const LWTUNNEL_IP6_SRC: u32 = 3;
/// Hop limit.
pub const LWTUNNEL_IP6_HOPLIMIT: u32 = 4;
/// Traffic class.
pub const LWTUNNEL_IP6_TC: u32 = 5;
/// Flags.
pub const LWTUNNEL_IP6_FLAGS: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encap_types_distinct() {
        let types = [
            LWTUNNEL_ENCAP_NONE,
            LWTUNNEL_ENCAP_MPLS,
            LWTUNNEL_ENCAP_IP,
            LWTUNNEL_ENCAP_ILA,
            LWTUNNEL_ENCAP_IP6,
            LWTUNNEL_ENCAP_SEG6,
            LWTUNNEL_ENCAP_BPF,
            LWTUNNEL_ENCAP_SEG6_LOCAL,
            LWTUNNEL_ENCAP_RPL,
            LWTUNNEL_ENCAP_IOAM6,
            LWTUNNEL_ENCAP_XFRM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ip_attrs_distinct() {
        let attrs = [
            LWTUNNEL_IP_UNSPEC,
            LWTUNNEL_IP_ID,
            LWTUNNEL_IP_DST,
            LWTUNNEL_IP_SRC,
            LWTUNNEL_IP_TTL,
            LWTUNNEL_IP_TOS,
            LWTUNNEL_IP_FLAGS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ip6_attrs_distinct() {
        let attrs = [
            LWTUNNEL_IP6_UNSPEC,
            LWTUNNEL_IP6_ID,
            LWTUNNEL_IP6_DST,
            LWTUNNEL_IP6_SRC,
            LWTUNNEL_IP6_HOPLIMIT,
            LWTUNNEL_IP6_TC,
            LWTUNNEL_IP6_FLAGS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
