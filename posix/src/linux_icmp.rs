//! `<linux/icmp.h>` — ICMP protocol header and types.
//!
//! Provides the ICMP header structure and message type/code constants.

pub use crate::socket::IPPROTO_ICMP;

// ---------------------------------------------------------------------------
// ICMP message types
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
/// Router advertisement.
pub const ICMP_ROUTERADVERT: u8 = 9;
/// Router solicitation.
pub const ICMP_ROUTERSOLICIT: u8 = 10;
/// Time exceeded.
pub const ICMP_TIME_EXCEEDED: u8 = 11;
/// Alias.
pub const ICMP_TIMXCEED: u8 = ICMP_TIME_EXCEEDED;
/// Parameter problem.
pub const ICMP_PARAMETERPROB: u8 = 12;
/// Timestamp request.
pub const ICMP_TIMESTAMP: u8 = 13;
/// Timestamp reply.
pub const ICMP_TIMESTAMPREPLY: u8 = 14;
/// Information request (deprecated).
pub const ICMP_INFO_REQUEST: u8 = 15;
/// Information reply (deprecated).
pub const ICMP_INFO_REPLY: u8 = 16;
/// Address mask request.
pub const ICMP_ADDRESS: u8 = 17;
/// Address mask reply.
pub const ICMP_ADDRESSREPLY: u8 = 18;

// ---------------------------------------------------------------------------
// Destination Unreachable codes
// ---------------------------------------------------------------------------

/// Network unreachable.
pub const ICMP_NET_UNREACH: u8 = 0;
/// Host unreachable.
pub const ICMP_HOST_UNREACH: u8 = 1;
/// Protocol unreachable.
pub const ICMP_PROT_UNREACH: u8 = 2;
/// Port unreachable.
pub const ICMP_PORT_UNREACH: u8 = 3;
/// Fragmentation needed and DF set.
pub const ICMP_FRAG_NEEDED: u8 = 4;
/// Source route failed.
pub const ICMP_SR_FAILED: u8 = 5;
/// Network unknown.
pub const ICMP_NET_UNKNOWN: u8 = 6;
/// Host unknown.
pub const ICMP_HOST_UNKNOWN: u8 = 7;

// ---------------------------------------------------------------------------
// Time Exceeded codes
// ---------------------------------------------------------------------------

/// TTL exceeded in transit.
pub const ICMP_EXC_TTL: u8 = 0;
/// Fragment reassembly time exceeded.
pub const ICMP_EXC_FRAGTIME: u8 = 1;

// ---------------------------------------------------------------------------
// Redirect codes
// ---------------------------------------------------------------------------

/// Redirect for network.
pub const ICMP_REDIR_NET: u8 = 0;
/// Redirect for host.
pub const ICMP_REDIR_HOST: u8 = 1;
/// Redirect for TOS and network.
pub const ICMP_REDIR_NETTOS: u8 = 2;
/// Redirect for TOS and host.
pub const ICMP_REDIR_HOSTTOS: u8 = 3;

// ---------------------------------------------------------------------------
// ICMP header
// ---------------------------------------------------------------------------

/// ICMP header (8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Icmphdr {
    /// Message type (ICMP_*).
    pub type_: u8,
    /// Type sub-code.
    pub code: u8,
    /// Header checksum.
    pub checksum: u16,
    /// Identifier (for echo request/reply).
    pub id: u16,
    /// Sequence number (for echo request/reply).
    pub sequence: u16,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icmphdr_size() {
        assert_eq!(core::mem::size_of::<Icmphdr>(), 8);
    }

    #[test]
    fn test_echo_types() {
        assert_eq!(ICMP_ECHO, 8);
        assert_eq!(ICMP_ECHOREPLY, 0);
    }

    #[test]
    fn test_message_types_distinct() {
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
            ICMP_NET_UNKNOWN,
            ICMP_HOST_UNKNOWN,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_redirect_codes() {
        assert_eq!(ICMP_REDIR_NET, 0);
        assert_eq!(ICMP_REDIR_HOSTTOS, 3);
    }

    #[test]
    fn test_ipproto_icmp() {
        assert_eq!(IPPROTO_ICMP, 1);
    }
}
