//! `<netinet/ip.h>` — IPv4 socket option constants.
//!
//! IP-level socket options control packet handling: TTL, TOS/DSCP,
//! multicast membership, fragmentation, and header inclusion.
//! These are set via setsockopt() at the IPPROTO_IP level.

// ---------------------------------------------------------------------------
// IP socket options (level IPPROTO_IP)
// ---------------------------------------------------------------------------

/// Type of Service field.
pub const IP_TOS: u32 = 1;
/// Time To Live.
pub const IP_TTL: u32 = 2;
/// Include IP header in raw socket data.
pub const IP_HDRINCL: u32 = 3;
/// IP options (source routing, record route, etc.).
pub const IP_OPTIONS: u32 = 4;
/// Set outgoing multicast interface.
pub const IP_MULTICAST_IF: u32 = 32;
/// Set multicast TTL.
pub const IP_MULTICAST_TTL: u32 = 33;
/// Enable/disable multicast loopback.
pub const IP_MULTICAST_LOOP: u32 = 34;
/// Join a multicast group.
pub const IP_ADD_MEMBERSHIP: u32 = 35;
/// Leave a multicast group.
pub const IP_DROP_MEMBERSHIP: u32 = 36;
/// Join a source-specific multicast group.
pub const IP_ADD_SOURCE_MEMBERSHIP: u32 = 39;
/// Leave a source-specific multicast group.
pub const IP_DROP_SOURCE_MEMBERSHIP: u32 = 40;
/// Block multicast from a source.
pub const IP_BLOCK_SOURCE: u32 = 38;
/// Unblock multicast from a source.
pub const IP_UNBLOCK_SOURCE: u32 = 37;
/// Receive destination address in ancillary data.
pub const IP_PKTINFO: u32 = 8;
/// Receive TOS in ancillary data.
pub const IP_RECVTOS: u32 = 13;
/// Receive TTL in ancillary data.
pub const IP_RECVTTL: u32 = 12;
/// Receive IP options in ancillary data.
pub const IP_RECVOPTS: u32 = 6;
/// Don't Fragment flag.
pub const IP_MTU_DISCOVER: u32 = 10;
/// Receive original destination address.
pub const IP_RECVORIGDSTADDR: u32 = 20;
/// Transparent proxy (TPROXY).
pub const IP_TRANSPARENT: u32 = 19;
/// Bind to device (like SO_BINDTODEVICE but IP-level).
pub const IP_FREEBIND: u32 = 15;

// ---------------------------------------------------------------------------
// IP_MTU_DISCOVER values
// ---------------------------------------------------------------------------

/// Don't set DF, fragment if needed.
pub const IP_PMTUDISC_DONT: u32 = 0;
/// Set DF for local, fragment for foreign.
pub const IP_PMTUDISC_WANT: u32 = 1;
/// Always set DF (path MTU discovery).
pub const IP_PMTUDISC_DO: u32 = 2;
/// Set DF but don't cache PMTU.
pub const IP_PMTUDISC_PROBE: u32 = 3;
/// Force DF and fail on exceeding MTU.
pub const IP_PMTUDISC_INTERFACE: u32 = 4;
/// Like INTERFACE but ignore dst PMTU.
pub const IP_PMTUDISC_OMIT: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_options_distinct() {
        let opts = [
            IP_TOS,
            IP_TTL,
            IP_HDRINCL,
            IP_OPTIONS,
            IP_MULTICAST_IF,
            IP_MULTICAST_TTL,
            IP_MULTICAST_LOOP,
            IP_ADD_MEMBERSHIP,
            IP_DROP_MEMBERSHIP,
            IP_PKTINFO,
            IP_RECVTOS,
            IP_RECVTTL,
            IP_MTU_DISCOVER,
            IP_RECVORIGDSTADDR,
            IP_TRANSPARENT,
            IP_FREEBIND,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_pmtu_disc_distinct() {
        let disc = [
            IP_PMTUDISC_DONT,
            IP_PMTUDISC_WANT,
            IP_PMTUDISC_DO,
            IP_PMTUDISC_PROBE,
            IP_PMTUDISC_INTERFACE,
            IP_PMTUDISC_OMIT,
        ];
        for i in 0..disc.len() {
            for j in (i + 1)..disc.len() {
                assert_ne!(disc[i], disc[j]);
            }
        }
    }

    #[test]
    fn test_multicast_options() {
        assert_ne!(IP_ADD_MEMBERSHIP, IP_DROP_MEMBERSHIP);
        assert_ne!(IP_MULTICAST_IF, IP_MULTICAST_TTL);
    }
}
