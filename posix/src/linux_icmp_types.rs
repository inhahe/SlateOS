//! `<linux/icmp.h>` — ICMP/ICMPv6 message type constants.
//!
//! ICMP (Internet Control Message Protocol) carries error messages
//! and operational information for IP networks. ICMP is used by
//! ping, traceroute, Path MTU discovery, and various network error
//! reporting mechanisms. ICMPv6 additionally handles neighbor
//! discovery (replacing ARP) and router discovery.

// ---------------------------------------------------------------------------
// ICMPv4 message types
// ---------------------------------------------------------------------------

/// Echo reply (ping response).
pub const ICMP_ECHOREPLY: u8 = 0;
/// Destination unreachable.
pub const ICMP_DEST_UNREACH: u8 = 3;
/// Source quench (deprecated).
pub const ICMP_SOURCE_QUENCH: u8 = 4;
/// Redirect.
pub const ICMP_REDIRECT: u8 = 5;
/// Echo request (ping).
pub const ICMP_ECHO: u8 = 8;
/// Time exceeded (TTL).
pub const ICMP_TIME_EXCEEDED: u8 = 11;
/// Parameter problem.
pub const ICMP_PARAMETERPROB: u8 = 12;
/// Timestamp request.
pub const ICMP_TIMESTAMP: u8 = 13;
/// Timestamp reply.
pub const ICMP_TIMESTAMPREPLY: u8 = 14;
/// Address mask request (deprecated).
pub const ICMP_ADDRESS: u8 = 17;
/// Address mask reply (deprecated).
pub const ICMP_ADDRESSREPLY: u8 = 18;

// ---------------------------------------------------------------------------
// ICMP destination unreachable codes
// ---------------------------------------------------------------------------

/// Network unreachable.
pub const ICMP_NET_UNREACH: u8 = 0;
/// Host unreachable.
pub const ICMP_HOST_UNREACH: u8 = 1;
/// Protocol unreachable.
pub const ICMP_PROT_UNREACH: u8 = 2;
/// Port unreachable.
pub const ICMP_PORT_UNREACH: u8 = 3;
/// Fragmentation needed but DF set.
pub const ICMP_FRAG_NEEDED: u8 = 4;
/// Source route failed.
pub const ICMP_SR_FAILED: u8 = 5;

// ---------------------------------------------------------------------------
// ICMPv6 message types
// ---------------------------------------------------------------------------

/// ICMPv6 destination unreachable.
pub const ICMPV6_DEST_UNREACH: u8 = 1;
/// ICMPv6 packet too big.
pub const ICMPV6_PKT_TOOBIG: u8 = 2;
/// ICMPv6 time exceeded.
pub const ICMPV6_TIME_EXCEED: u8 = 3;
/// ICMPv6 parameter problem.
pub const ICMPV6_PARAMPROB: u8 = 4;
/// ICMPv6 echo request.
pub const ICMPV6_ECHO_REQUEST: u8 = 128;
/// ICMPv6 echo reply.
pub const ICMPV6_ECHO_REPLY: u8 = 129;
/// Router solicitation.
pub const ICMPV6_ROUTER_SOLICIT: u8 = 133;
/// Router advertisement.
pub const ICMPV6_ROUTER_ADVERT: u8 = 134;
/// Neighbor solicitation (like ARP request).
pub const ICMPV6_NEIGHBOR_SOLICIT: u8 = 135;
/// Neighbor advertisement (like ARP reply).
pub const ICMPV6_NEIGHBOR_ADVERT: u8 = 136;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icmpv4_types_distinct() {
        let types = [
            ICMP_ECHOREPLY,
            ICMP_DEST_UNREACH,
            ICMP_SOURCE_QUENCH,
            ICMP_REDIRECT,
            ICMP_ECHO,
            ICMP_TIME_EXCEEDED,
            ICMP_PARAMETERPROB,
            ICMP_TIMESTAMP,
            ICMP_TIMESTAMPREPLY,
            ICMP_ADDRESS,
            ICMP_ADDRESSREPLY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_unreach_codes_distinct() {
        let codes = [
            ICMP_NET_UNREACH,
            ICMP_HOST_UNREACH,
            ICMP_PROT_UNREACH,
            ICMP_PORT_UNREACH,
            ICMP_FRAG_NEEDED,
            ICMP_SR_FAILED,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_icmpv6_types_distinct() {
        let types = [
            ICMPV6_DEST_UNREACH,
            ICMPV6_PKT_TOOBIG,
            ICMPV6_TIME_EXCEED,
            ICMPV6_PARAMPROB,
            ICMPV6_ECHO_REQUEST,
            ICMPV6_ECHO_REPLY,
            ICMPV6_ROUTER_SOLICIT,
            ICMPV6_ROUTER_ADVERT,
            ICMPV6_NEIGHBOR_SOLICIT,
            ICMPV6_NEIGHBOR_ADVERT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_echo_pair() {
        assert_eq!(ICMP_ECHO, 8);
        assert_eq!(ICMP_ECHOREPLY, 0);
    }
}
