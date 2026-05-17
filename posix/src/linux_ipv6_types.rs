//! `<linux/ipv6.h>` — IPv6 socket option constants.
//!
//! IPv6 socket options control per-socket behavior for IPv6
//! connections: hop limit, multicast, flow labels, path MTU
//! discovery, and extension header handling. These are set via
//! setsockopt() at the SOL_IPV6 level.

// ---------------------------------------------------------------------------
// IPv6 socket options (SOL_IPV6 = 41)
// ---------------------------------------------------------------------------

/// Unicast hop limit.
pub const IPV6_UNICAST_HOPS: u32 = 16;
/// Multicast hop limit.
pub const IPV6_MULTICAST_HOPS: u32 = 18;
/// Multicast interface.
pub const IPV6_MULTICAST_IF: u32 = 17;
/// Multicast loopback.
pub const IPV6_MULTICAST_LOOP: u32 = 19;
/// Join multicast group.
pub const IPV6_JOIN_GROUP: u32 = 20;
/// Leave multicast group.
pub const IPV6_LEAVE_GROUP: u32 = 21;
/// Restrict socket to IPv6 only (no v4-mapped).
pub const IPV6_V6ONLY: u32 = 26;
/// Receive packet info (destination address, interface).
pub const IPV6_RECVPKTINFO: u32 = 49;
/// Receive hop limit.
pub const IPV6_RECVHOPLIMIT: u32 = 51;
/// Path MTU discovery mode.
pub const IPV6_MTU_DISCOVER: u32 = 23;
/// Get path MTU.
pub const IPV6_MTU: u32 = 24;
/// Set flow label.
pub const IPV6_FLOWLABEL_MGR: u32 = 32;
/// Receive flow info.
pub const IPV6_FLOWINFO: u32 = 11;
/// Traffic class (DSCP + ECN).
pub const IPV6_TCLASS: u32 = 67;
/// Receive traffic class.
pub const IPV6_RECVTCLASS: u32 = 66;
/// Transparent proxy.
pub const IPV6_TRANSPARENT: u32 = 75;
/// Receive original destination address.
pub const IPV6_RECVORIGDSTADDR: u32 = 74;

// ---------------------------------------------------------------------------
// Path MTU discovery modes
// ---------------------------------------------------------------------------

/// Don't do PMTU discovery.
pub const IPV6_PMTUDISC_DONT: u32 = 0;
/// Do PMTU discovery.
pub const IPV6_PMTUDISC_WANT: u32 = 1;
/// Always set DF (do PMTU).
pub const IPV6_PMTUDISC_DO: u32 = 2;
/// Set DF but ignore PMTU for routing.
pub const IPV6_PMTUDISC_PROBE: u32 = 3;
/// Use interface MTU.
pub const IPV6_PMTUDISC_INTERFACE: u32 = 4;
/// Omit DF for connected sockets.
pub const IPV6_PMTUDISC_OMIT: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_options_distinct() {
        let opts = [
            IPV6_UNICAST_HOPS, IPV6_MULTICAST_IF, IPV6_MULTICAST_HOPS,
            IPV6_MULTICAST_LOOP, IPV6_JOIN_GROUP, IPV6_LEAVE_GROUP,
            IPV6_V6ONLY, IPV6_MTU_DISCOVER, IPV6_MTU,
            IPV6_FLOWLABEL_MGR, IPV6_FLOWINFO, IPV6_RECVPKTINFO,
            IPV6_RECVHOPLIMIT, IPV6_TCLASS, IPV6_RECVTCLASS,
            IPV6_TRANSPARENT, IPV6_RECVORIGDSTADDR,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_pmtu_modes_distinct() {
        let modes = [
            IPV6_PMTUDISC_DONT, IPV6_PMTUDISC_WANT, IPV6_PMTUDISC_DO,
            IPV6_PMTUDISC_PROBE, IPV6_PMTUDISC_INTERFACE, IPV6_PMTUDISC_OMIT,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_v6only_value() {
        assert_eq!(IPV6_V6ONLY, 26);
    }
}
