//! `<linux/icmpv6.h>` — ICMPv6 message type and code constants.
//!
//! ICMPv6 handles error reporting and diagnostic functions for
//! IPv6, including Neighbor Discovery Protocol (NDP), Path MTU
//! Discovery, and Multicast Listener Discovery (MLD).

// ---------------------------------------------------------------------------
// ICMPv6 error message types (0-127)
// ---------------------------------------------------------------------------

/// Destination unreachable.
pub const ICMPV6_DEST_UNREACH: u8 = 1;
/// Packet too big.
pub const ICMPV6_PKT_TOOBIG: u8 = 2;
/// Time exceeded.
pub const ICMPV6_TIME_EXCEEDED: u8 = 3;
/// Parameter problem.
pub const ICMPV6_PARAMPROB: u8 = 4;

// ---------------------------------------------------------------------------
// ICMPv6 informational message types (128-255)
// ---------------------------------------------------------------------------

/// Echo request (ping).
pub const ICMPV6_ECHO_REQUEST: u8 = 128;
/// Echo reply (pong).
pub const ICMPV6_ECHO_REPLY: u8 = 129;
/// Multicast listener query.
pub const ICMPV6_MLD_QUERY: u8 = 130;
/// Multicast listener report (v1).
pub const ICMPV6_MLD_REPORT: u8 = 131;
/// Multicast listener done.
pub const ICMPV6_MLD_DONE: u8 = 132;

// ---------------------------------------------------------------------------
// NDP message types
// ---------------------------------------------------------------------------

/// Router solicitation.
pub const ICMPV6_ROUTER_SOLICITATION: u8 = 133;
/// Router advertisement.
pub const ICMPV6_ROUTER_ADVERTISEMENT: u8 = 134;
/// Neighbor solicitation.
pub const ICMPV6_NEIGHBOR_SOLICITATION: u8 = 135;
/// Neighbor advertisement.
pub const ICMPV6_NEIGHBOR_ADVERTISEMENT: u8 = 136;
/// Redirect message.
pub const ICMPV6_REDIRECT: u8 = 137;

// ---------------------------------------------------------------------------
// MLDv2 message types
// ---------------------------------------------------------------------------

/// Multicast listener report v2.
pub const ICMPV6_MLD2_REPORT: u8 = 143;

// ---------------------------------------------------------------------------
// Destination Unreachable codes
// ---------------------------------------------------------------------------

/// No route to destination.
pub const ICMPV6_NOROUTE: u8 = 0;
/// Administratively prohibited.
pub const ICMPV6_ADM_PROHIBITED: u8 = 1;
/// Beyond scope of source address.
pub const ICMPV6_NOT_NEIGHBOUR: u8 = 2;
/// Address unreachable.
pub const ICMPV6_ADDR_UNREACH: u8 = 3;
/// Port unreachable.
pub const ICMPV6_PORT_UNREACH: u8 = 4;
/// Source address failed policy.
pub const ICMPV6_POLICY_FAIL: u8 = 5;
/// Reject route to destination.
pub const ICMPV6_REJECT_ROUTE: u8 = 6;

// ---------------------------------------------------------------------------
// Time Exceeded codes
// ---------------------------------------------------------------------------

/// Hop limit exceeded in transit.
pub const ICMPV6_EXC_HOPLIMIT: u8 = 0;
/// Fragment reassembly time exceeded.
pub const ICMPV6_EXC_FRAGTIME: u8 = 1;

// ---------------------------------------------------------------------------
// NDP option types
// ---------------------------------------------------------------------------

/// Source link-layer address option.
pub const NDP_OPT_SOURCE_LL_ADDR: u8 = 1;
/// Target link-layer address option.
pub const NDP_OPT_TARGET_LL_ADDR: u8 = 2;
/// Prefix information option.
pub const NDP_OPT_PREFIX_INFO: u8 = 3;
/// Redirected header option.
pub const NDP_OPT_REDIRECTED_HDR: u8 = 4;
/// MTU option.
pub const NDP_OPT_MTU: u8 = 5;
/// Route information option.
pub const NDP_OPT_ROUTE_INFO: u8 = 24;
/// Recursive DNS server option.
pub const NDP_OPT_RDNSS: u8 = 25;
/// DNS search list option.
pub const NDP_OPT_DNSSL: u8 = 31;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_types_distinct() {
        let types = [
            ICMPV6_DEST_UNREACH, ICMPV6_PKT_TOOBIG,
            ICMPV6_TIME_EXCEEDED, ICMPV6_PARAMPROB,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_info_types_distinct() {
        let types = [
            ICMPV6_ECHO_REQUEST, ICMPV6_ECHO_REPLY,
            ICMPV6_MLD_QUERY, ICMPV6_MLD_REPORT, ICMPV6_MLD_DONE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ndp_types_distinct() {
        let types = [
            ICMPV6_ROUTER_SOLICITATION, ICMPV6_ROUTER_ADVERTISEMENT,
            ICMPV6_NEIGHBOR_SOLICITATION, ICMPV6_NEIGHBOR_ADVERTISEMENT,
            ICMPV6_REDIRECT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dest_unreach_codes_distinct() {
        let codes = [
            ICMPV6_NOROUTE, ICMPV6_ADM_PROHIBITED,
            ICMPV6_NOT_NEIGHBOUR, ICMPV6_ADDR_UNREACH,
            ICMPV6_PORT_UNREACH, ICMPV6_POLICY_FAIL,
            ICMPV6_REJECT_ROUTE,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_ndp_options_distinct() {
        let opts = [
            NDP_OPT_SOURCE_LL_ADDR, NDP_OPT_TARGET_LL_ADDR,
            NDP_OPT_PREFIX_INFO, NDP_OPT_REDIRECTED_HDR,
            NDP_OPT_MTU, NDP_OPT_ROUTE_INFO,
            NDP_OPT_RDNSS, NDP_OPT_DNSSL,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_echo_request() {
        assert_eq!(ICMPV6_ECHO_REQUEST, 128);
    }

    #[test]
    fn test_error_types_below_128() {
        assert!(ICMPV6_DEST_UNREACH < 128);
        assert!(ICMPV6_PKT_TOOBIG < 128);
        assert!(ICMPV6_TIME_EXCEEDED < 128);
        assert!(ICMPV6_PARAMPROB < 128);
    }
}
