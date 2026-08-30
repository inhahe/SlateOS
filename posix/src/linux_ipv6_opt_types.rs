//! `<netinet/in.h>` — IPv6 socket option constants.
//!
//! IPv6 socket options control hop limit, flow labels, multicast,
//! extension header handling, and IPv4-mapped address behavior.
//! These are set via setsockopt() at the IPPROTO_IPV6 level.

// ---------------------------------------------------------------------------
// IPv6 socket options (level IPPROTO_IPV6)
// ---------------------------------------------------------------------------

/// Set unicast hop limit (equivalent to IPv4 TTL).
pub const IPV6_UNICAST_HOPS: u32 = 16;
/// Set multicast hop limit.
pub const IPV6_MULTICAST_HOPS: u32 = 18;
/// Set multicast output interface.
pub const IPV6_MULTICAST_IF: u32 = 17;
/// Enable/disable multicast loopback.
pub const IPV6_MULTICAST_LOOP: u32 = 19;
/// Join a multicast group.
pub const IPV6_JOIN_GROUP: u32 = 20;
/// Leave a multicast group.
pub const IPV6_LEAVE_GROUP: u32 = 21;
/// Restrict socket to IPv6 only (no IPv4-mapped addresses).
pub const IPV6_V6ONLY: u32 = 26;
/// Receive packet info (destination address, interface).
pub const IPV6_RECVPKTINFO: u32 = 49;
/// Receive hop limit in ancillary data.
pub const IPV6_RECVHOPLIMIT: u32 = 51;
/// Receive traffic class in ancillary data.
pub const IPV6_RECVTCLASS: u32 = 66;
/// Set traffic class (DSCP + ECN).
pub const IPV6_TCLASS: u32 = 67;
/// Set flowlabel.
pub const IPV6_FLOWLABEL_MGR: u32 = 32;
/// Receive flowlabel in ancillary data.
pub const IPV6_FLOWINFO: u32 = 11;
/// Path MTU discovery mode.
pub const IPV6_MTU_DISCOVER: u32 = 23;
/// Receive path MTU.
pub const IPV6_MTU: u32 = 24;
/// Receive routing header.
pub const IPV6_RTHDR: u32 = 57;
/// Receive hop-by-hop options header.
pub const IPV6_HOPOPTS: u32 = 54;
/// Receive destination options header.
pub const IPV6_DSTOPTS: u32 = 59;
/// Set source address preference.
pub const IPV6_ADDR_PREFERENCES: u32 = 72;
/// Transparent proxy.
pub const IPV6_TRANSPARENT: u32 = 75;
/// Enable/disable autoflowlabel.
pub const IPV6_AUTOFLOWLABEL: u32 = 70;

// ---------------------------------------------------------------------------
// IPv6 address preference flags (IPV6_ADDR_PREFERENCES values)
// ---------------------------------------------------------------------------

/// Prefer source address: public (non-temporary).
pub const IPV6_PREFER_SRC_PUBLIC: u32 = 0x0002;
/// Prefer source address: temporary (privacy).
pub const IPV6_PREFER_SRC_TMP: u32 = 0x0001;
/// Prefer source address: home (MIPv6).
pub const IPV6_PREFER_SRC_HOME: u32 = 0x0400;
/// Prefer source address: care-of (MIPv6).
pub const IPV6_PREFER_SRC_COA: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv6_options_distinct() {
        let opts = [
            IPV6_UNICAST_HOPS,
            IPV6_MULTICAST_HOPS,
            IPV6_MULTICAST_IF,
            IPV6_MULTICAST_LOOP,
            IPV6_JOIN_GROUP,
            IPV6_LEAVE_GROUP,
            IPV6_V6ONLY,
            IPV6_RECVPKTINFO,
            IPV6_RECVHOPLIMIT,
            IPV6_RECVTCLASS,
            IPV6_TCLASS,
            IPV6_FLOWLABEL_MGR,
            IPV6_FLOWINFO,
            IPV6_MTU_DISCOVER,
            IPV6_MTU,
            IPV6_RTHDR,
            IPV6_HOPOPTS,
            IPV6_DSTOPTS,
            IPV6_ADDR_PREFERENCES,
            IPV6_TRANSPARENT,
            IPV6_AUTOFLOWLABEL,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_addr_prefs_distinct() {
        let prefs = [
            IPV6_PREFER_SRC_PUBLIC,
            IPV6_PREFER_SRC_TMP,
            IPV6_PREFER_SRC_HOME,
            IPV6_PREFER_SRC_COA,
        ];
        for i in 0..prefs.len() {
            for j in (i + 1)..prefs.len() {
                assert_ne!(prefs[i], prefs[j]);
            }
        }
    }

    #[test]
    fn test_v6only_value() {
        assert_eq!(IPV6_V6ONLY, 26);
    }
}
