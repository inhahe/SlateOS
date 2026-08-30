//! `<linux/ipv6.h>` / `<netinet/ip6.h>` — IPv6 socket options.
//!
//! IPv6 socket-level configuration covers PMTU discovery, source-
//! address selection, IPsec, hop limits, multicast joins, and DSCP.
//! `radvd`, NetworkManager, every `dhcpcd6` and every CGN reach the
//! kernel through these names. The constants below match
//! `<linux/in6.h>` / `<linux/ipv6.h>`.

// ---------------------------------------------------------------------------
// SOL_IPV6 setsockopt names
// ---------------------------------------------------------------------------

pub const IPV6_ADDRFORM: u32 = 1;
pub const IPV6_2292PKTINFO: u32 = 2;
pub const IPV6_2292HOPOPTS: u32 = 3;
pub const IPV6_2292DSTOPTS: u32 = 4;
pub const IPV6_2292RTHDR: u32 = 5;
pub const IPV6_2292PKTOPTIONS: u32 = 6;
pub const IPV6_CHECKSUM: u32 = 7;
pub const IPV6_2292HOPLIMIT: u32 = 8;
pub const IPV6_NEXTHOP: u32 = 9;
pub const IPV6_AUTHHDR: u32 = 10;
pub const IPV6_UNICAST_HOPS: u32 = 16;
pub const IPV6_MULTICAST_IF: u32 = 17;
pub const IPV6_MULTICAST_HOPS: u32 = 18;
pub const IPV6_MULTICAST_LOOP: u32 = 19;
pub const IPV6_ADD_MEMBERSHIP: u32 = 20;
pub const IPV6_DROP_MEMBERSHIP: u32 = 21;
pub const IPV6_ROUTER_ALERT: u32 = 22;
pub const IPV6_MTU_DISCOVER: u32 = 23;
pub const IPV6_MTU: u32 = 24;
pub const IPV6_RECVERR: u32 = 25;
pub const IPV6_V6ONLY: u32 = 26;
pub const IPV6_JOIN_ANYCAST: u32 = 27;
pub const IPV6_LEAVE_ANYCAST: u32 = 28;
pub const IPV6_MULTICAST_ALL: u32 = 29;
pub const IPV6_ROUTER_ALERT_ISOLATE: u32 = 30;

// ---------------------------------------------------------------------------
// RFC 3542 advanced API
// ---------------------------------------------------------------------------

pub const IPV6_RECVPKTINFO: u32 = 49;
pub const IPV6_PKTINFO: u32 = 50;
pub const IPV6_RECVHOPLIMIT: u32 = 51;
pub const IPV6_HOPLIMIT: u32 = 52;
pub const IPV6_RECVHOPOPTS: u32 = 53;
pub const IPV6_HOPOPTS: u32 = 54;
pub const IPV6_RTHDRDSTOPTS: u32 = 55;
pub const IPV6_RECVRTHDR: u32 = 56;
pub const IPV6_RTHDR: u32 = 57;
pub const IPV6_RECVDSTOPTS: u32 = 58;
pub const IPV6_DSTOPTS: u32 = 59;
pub const IPV6_RECVPATHMTU: u32 = 60;
pub const IPV6_PATHMTU: u32 = 61;
pub const IPV6_DONTFRAG: u32 = 62;

// ---------------------------------------------------------------------------
// IPV6_MTU_DISCOVER modes (mirror IPv4)
// ---------------------------------------------------------------------------

pub const IPV6_PMTUDISC_DONT: u32 = 0;
pub const IPV6_PMTUDISC_WANT: u32 = 1;
pub const IPV6_PMTUDISC_DO: u32 = 2;
pub const IPV6_PMTUDISC_PROBE: u32 = 3;
pub const IPV6_PMTUDISC_INTERFACE: u32 = 4;
pub const IPV6_PMTUDISC_OMIT: u32 = 5;

// ---------------------------------------------------------------------------
// Address sizes
// ---------------------------------------------------------------------------

/// Octets in `struct in6_addr`.
pub const IN6_ADDR_LEN: usize = 16;
/// Octets in `struct sockaddr_in6`.
pub const SOCKADDR_IN6_LEN: usize = 28;
/// Minimum IPv6 MTU per RFC 8200.
pub const IPV6_MIN_MTU: u32 = 1280;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_sockopts_distinct() {
        let o = [
            IPV6_ADDRFORM,
            IPV6_CHECKSUM,
            IPV6_UNICAST_HOPS,
            IPV6_MULTICAST_IF,
            IPV6_MULTICAST_HOPS,
            IPV6_MULTICAST_LOOP,
            IPV6_ADD_MEMBERSHIP,
            IPV6_DROP_MEMBERSHIP,
            IPV6_MTU_DISCOVER,
            IPV6_MTU,
            IPV6_V6ONLY,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
    }

    #[test]
    fn test_rfc3542_block_dense_49_to_62() {
        let r = [
            IPV6_RECVPKTINFO,
            IPV6_PKTINFO,
            IPV6_RECVHOPLIMIT,
            IPV6_HOPLIMIT,
            IPV6_RECVHOPOPTS,
            IPV6_HOPOPTS,
            IPV6_RTHDRDSTOPTS,
            IPV6_RECVRTHDR,
            IPV6_RTHDR,
            IPV6_RECVDSTOPTS,
            IPV6_DSTOPTS,
            IPV6_RECVPATHMTU,
            IPV6_PATHMTU,
            IPV6_DONTFRAG,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, 49 + i);
        }
    }

    #[test]
    fn test_pmtu_modes_match_ipv4() {
        assert_eq!(IPV6_PMTUDISC_DONT, 0);
        assert_eq!(IPV6_PMTUDISC_WANT, 1);
        assert_eq!(IPV6_PMTUDISC_DO, 2);
        assert_eq!(IPV6_PMTUDISC_PROBE, 3);
        assert_eq!(IPV6_PMTUDISC_INTERFACE, 4);
        assert_eq!(IPV6_PMTUDISC_OMIT, 5);
    }

    #[test]
    fn test_address_sizes() {
        // 128-bit address.
        assert_eq!(IN6_ADDR_LEN, 16);
        // sin6_family(2)+port(2)+flowinfo(4)+addr(16)+scope_id(4) = 28.
        assert_eq!(SOCKADDR_IN6_LEN, 2 + 2 + 4 + 16 + 4);
        // RFC 8200 minimum MTU.
        assert_eq!(IPV6_MIN_MTU, 1280);
    }

    #[test]
    fn test_v6only_value_is_26() {
        // Many bug reports key on this option being numerically 26.
        assert_eq!(IPV6_V6ONLY, 26);
    }
}
