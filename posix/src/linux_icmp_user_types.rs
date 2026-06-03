//! `<linux/icmp.h>` — ICMPv4 message types, codes, and ioctls.
//!
//! ICMP carries `ping`, `traceroute`, MTU discovery, and gateway
//! redirects. Userspace tools (`iputils`, `nmap`, `mtr`, BPF
//! ICMP-rate-limiters) reach the kernel via `IPPROTO_ICMP` sockets and
//! the constants below.

// ---------------------------------------------------------------------------
// `struct icmphdr.type` values
// ---------------------------------------------------------------------------

pub const ICMP_ECHOREPLY: u8 = 0;
pub const ICMP_DEST_UNREACH: u8 = 3;
pub const ICMP_SOURCE_QUENCH: u8 = 4;
pub const ICMP_REDIRECT: u8 = 5;
pub const ICMP_ECHO: u8 = 8;
pub const ICMP_TIME_EXCEEDED: u8 = 11;
pub const ICMP_PARAMETERPROB: u8 = 12;
pub const ICMP_TIMESTAMP: u8 = 13;
pub const ICMP_TIMESTAMPREPLY: u8 = 14;
pub const ICMP_INFO_REQUEST: u8 = 15;
pub const ICMP_INFO_REPLY: u8 = 16;
pub const ICMP_ADDRESS: u8 = 17;
pub const ICMP_ADDRESSREPLY: u8 = 18;
/// Last assigned ICMP type (RFC 6918 reserved boundary).
pub const NR_ICMP_TYPES: u8 = 18;

// ---------------------------------------------------------------------------
// Codes for ICMP_DEST_UNREACH
// ---------------------------------------------------------------------------

pub const ICMP_NET_UNREACH: u8 = 0;
pub const ICMP_HOST_UNREACH: u8 = 1;
pub const ICMP_PROT_UNREACH: u8 = 2;
pub const ICMP_PORT_UNREACH: u8 = 3;
pub const ICMP_FRAG_NEEDED: u8 = 4;
pub const ICMP_SR_FAILED: u8 = 5;
pub const ICMP_NET_UNKNOWN: u8 = 6;
pub const ICMP_HOST_UNKNOWN: u8 = 7;
pub const ICMP_HOST_ISOLATED: u8 = 8;
pub const ICMP_NET_ANO: u8 = 9;
pub const ICMP_HOST_ANO: u8 = 10;
pub const ICMP_NET_UNR_TOS: u8 = 11;
pub const ICMP_HOST_UNR_TOS: u8 = 12;
pub const ICMP_PKT_FILTERED: u8 = 13;
pub const ICMP_PREC_VIOLATION: u8 = 14;
pub const ICMP_PREC_CUTOFF: u8 = 15;
pub const NR_ICMP_UNREACH: u8 = 15;

// ---------------------------------------------------------------------------
// Codes for ICMP_REDIRECT
// ---------------------------------------------------------------------------

pub const ICMP_REDIR_NET: u8 = 0;
pub const ICMP_REDIR_HOST: u8 = 1;
pub const ICMP_REDIR_NETTOS: u8 = 2;
pub const ICMP_REDIR_HOSTTOS: u8 = 3;

// ---------------------------------------------------------------------------
// Codes for ICMP_TIME_EXCEEDED
// ---------------------------------------------------------------------------

pub const ICMP_EXC_TTL: u8 = 0;
pub const ICMP_EXC_FRAGTIME: u8 = 1;

// ---------------------------------------------------------------------------
// Filter for `IPPROTO_ICMP` raw sockets (struct icmp_filter)
// ---------------------------------------------------------------------------

/// `ICMP_FILTER` socket-level option.
pub const ICMP_FILTER: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_reply_pairs() {
        // Convention: every "request" pairs with a "reply".
        assert_eq!(ICMP_ECHOREPLY, 0);
        assert_eq!(ICMP_ECHO, 8);
        assert_eq!(ICMP_TIMESTAMP, 13);
        assert_eq!(ICMP_TIMESTAMPREPLY, 14);
        assert_eq!(ICMP_INFO_REQUEST, 15);
        assert_eq!(ICMP_INFO_REPLY, 16);
        assert_eq!(ICMP_ADDRESS, 17);
        assert_eq!(ICMP_ADDRESSREPLY, 18);
    }

    #[test]
    fn test_nr_icmp_types_matches_last() {
        assert_eq!(NR_ICMP_TYPES, ICMP_ADDRESSREPLY);
    }

    #[test]
    fn test_dest_unreach_codes_dense_0_to_15() {
        let c = [
            ICMP_NET_UNREACH,
            ICMP_HOST_UNREACH,
            ICMP_PROT_UNREACH,
            ICMP_PORT_UNREACH,
            ICMP_FRAG_NEEDED,
            ICMP_SR_FAILED,
            ICMP_NET_UNKNOWN,
            ICMP_HOST_UNKNOWN,
            ICMP_HOST_ISOLATED,
            ICMP_NET_ANO,
            ICMP_HOST_ANO,
            ICMP_NET_UNR_TOS,
            ICMP_HOST_UNR_TOS,
            ICMP_PKT_FILTERED,
            ICMP_PREC_VIOLATION,
            ICMP_PREC_CUTOFF,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(NR_ICMP_UNREACH, 15);
    }

    #[test]
    fn test_redirect_codes_dense_0_to_3() {
        assert_eq!(ICMP_REDIR_NET, 0);
        assert_eq!(ICMP_REDIR_HOST, 1);
        assert_eq!(ICMP_REDIR_NETTOS, 2);
        assert_eq!(ICMP_REDIR_HOSTTOS, 3);
    }

    #[test]
    fn test_time_exceeded_codes() {
        assert_eq!(ICMP_EXC_TTL, 0);
        assert_eq!(ICMP_EXC_FRAGTIME, 1);
    }

    #[test]
    fn test_filter_option_is_1() {
        // ICMP_FILTER is the only setsockopt-level option in <icmp.h>.
        assert_eq!(ICMP_FILTER, 1);
    }
}
