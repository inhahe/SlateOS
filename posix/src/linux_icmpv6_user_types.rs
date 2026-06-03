//! `<linux/icmpv6.h>` — ICMPv6 message types, NDP, and MLD.
//!
//! ICMPv6 is mandatory for IPv6: it carries Neighbor Discovery
//! (`NS`/`NA`/`RS`/`RA`/Redirect), Multicast Listener Discovery,
//! Path MTU discovery, and `ping6`. `radvd`, `ndppd`, NetworkManager,
//! and the kernel router-advertisement code all rely on the type/code
//! constants below.

// ---------------------------------------------------------------------------
// ICMPv6 type values (struct icmp6hdr.icmp6_type)
// ---------------------------------------------------------------------------

/// Destination unreachable.
pub const ICMPV6_DEST_UNREACH: u8 = 1;
/// Packet too big (Path MTU discovery).
pub const ICMPV6_PKT_TOOBIG: u8 = 2;
/// Hop limit exceeded.
pub const ICMPV6_TIME_EXCEED: u8 = 3;
/// Parameter problem.
pub const ICMPV6_PARAMPROB: u8 = 4;
/// Echo request (ping6).
pub const ICMPV6_ECHO_REQUEST: u8 = 128;
/// Echo reply.
pub const ICMPV6_ECHO_REPLY: u8 = 129;
/// Multicast Listener Query.
pub const ICMPV6_MGM_QUERY: u8 = 130;
/// Multicast Listener Report.
pub const ICMPV6_MGM_REPORT: u8 = 131;
/// Multicast Listener Done.
pub const ICMPV6_MGM_REDUCTION: u8 = 132;
/// Router Solicitation.
pub const NDISC_ROUTER_SOLICITATION: u8 = 133;
/// Router Advertisement.
pub const NDISC_ROUTER_ADVERTISEMENT: u8 = 134;
/// Neighbor Solicitation.
pub const NDISC_NEIGHBOUR_SOLICITATION: u8 = 135;
/// Neighbor Advertisement.
pub const NDISC_NEIGHBOUR_ADVERTISEMENT: u8 = 136;
/// Redirect.
pub const NDISC_REDIRECT: u8 = 137;
/// MLDv2 Listener Report.
pub const ICMPV6_MLD2_REPORT: u8 = 143;
/// First info-type (>=128 ⇒ informational by RFC 4443).
pub const ICMPV6_INFOMSG_MASK: u8 = 0x80;

// ---------------------------------------------------------------------------
// Codes for ICMPV6_DEST_UNREACH
// ---------------------------------------------------------------------------

pub const ICMPV6_NOROUTE: u8 = 0;
pub const ICMPV6_ADM_PROHIBITED: u8 = 1;
pub const ICMPV6_NOT_NEIGHBOUR: u8 = 2;
pub const ICMPV6_ADDR_UNREACH: u8 = 3;
pub const ICMPV6_PORT_UNREACH: u8 = 4;
pub const ICMPV6_POLICY_FAIL: u8 = 5;
pub const ICMPV6_REJECT_ROUTE: u8 = 6;

// ---------------------------------------------------------------------------
// Codes for ICMPV6_TIME_EXCEED
// ---------------------------------------------------------------------------

pub const ICMPV6_EXC_HOPLIMIT: u8 = 0;
pub const ICMPV6_EXC_FRAGTIME: u8 = 1;

// ---------------------------------------------------------------------------
// Codes for ICMPV6_PARAMPROB
// ---------------------------------------------------------------------------

pub const ICMPV6_HDR_FIELD: u8 = 0;
pub const ICMPV6_UNK_NEXTHDR: u8 = 1;
pub const ICMPV6_UNK_OPTION: u8 = 2;

// ---------------------------------------------------------------------------
// Socket option for icmpv6 filter
// ---------------------------------------------------------------------------

/// `ICMPV6_FILTER` socket option.
pub const ICMPV6_FILTER: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_types_lt_128() {
        // RFC 4443: error messages have type 0..127.
        for &t in &[
            ICMPV6_DEST_UNREACH,
            ICMPV6_PKT_TOOBIG,
            ICMPV6_TIME_EXCEED,
            ICMPV6_PARAMPROB,
        ] {
            assert!(t < ICMPV6_INFOMSG_MASK);
            assert_eq!(t & ICMPV6_INFOMSG_MASK, 0);
        }
    }

    #[test]
    fn test_info_types_high_bit_set() {
        for &t in &[
            ICMPV6_ECHO_REQUEST,
            ICMPV6_ECHO_REPLY,
            ICMPV6_MGM_QUERY,
            ICMPV6_MGM_REPORT,
            ICMPV6_MGM_REDUCTION,
            NDISC_ROUTER_SOLICITATION,
            NDISC_ROUTER_ADVERTISEMENT,
            NDISC_NEIGHBOUR_SOLICITATION,
            NDISC_NEIGHBOUR_ADVERTISEMENT,
            NDISC_REDIRECT,
            ICMPV6_MLD2_REPORT,
        ] {
            assert!(t & ICMPV6_INFOMSG_MASK != 0);
        }
    }

    #[test]
    fn test_ndp_block_is_dense_133_to_137() {
        // RFC 4861 numbers NDP messages 133..137.
        assert_eq!(NDISC_ROUTER_SOLICITATION, 133);
        assert_eq!(NDISC_ROUTER_ADVERTISEMENT, 134);
        assert_eq!(NDISC_NEIGHBOUR_SOLICITATION, 135);
        assert_eq!(NDISC_NEIGHBOUR_ADVERTISEMENT, 136);
        assert_eq!(NDISC_REDIRECT, 137);
    }

    #[test]
    fn test_dest_unreach_codes_dense() {
        let c = [
            ICMPV6_NOROUTE,
            ICMPV6_ADM_PROHIBITED,
            ICMPV6_NOT_NEIGHBOUR,
            ICMPV6_ADDR_UNREACH,
            ICMPV6_PORT_UNREACH,
            ICMPV6_POLICY_FAIL,
            ICMPV6_REJECT_ROUTE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_paramprob_codes_dense() {
        assert_eq!(ICMPV6_HDR_FIELD, 0);
        assert_eq!(ICMPV6_UNK_NEXTHDR, 1);
        assert_eq!(ICMPV6_UNK_OPTION, 2);
    }

    #[test]
    fn test_filter_option_matches_v4() {
        // ICMPV6_FILTER mirrors ICMP_FILTER==1 for consistency.
        assert_eq!(ICMPV6_FILTER, 1);
    }
}
