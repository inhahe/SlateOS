//! `<linux/rtnetlink.h>` — Routing netlink message types.
//!
//! NETLINK_ROUTE is the primary netlink protocol for network
//! configuration. It handles routes, links, addresses, neighbors,
//! and rules. Used by iproute2, NetworkManager, and systemd-networkd.

// ---------------------------------------------------------------------------
// RTM message types
// ---------------------------------------------------------------------------

/// New link (network interface).
pub const RTM_NEWLINK: u16 = 16;
/// Delete link.
pub const RTM_DELLINK: u16 = 17;
/// Get link info.
pub const RTM_GETLINK: u16 = 18;
/// Set link attributes.
pub const RTM_SETLINK: u16 = 19;

/// New address.
pub const RTM_NEWADDR: u16 = 20;
/// Delete address.
pub const RTM_DELADDR: u16 = 21;
/// Get address.
pub const RTM_GETADDR: u16 = 22;

/// New route.
pub const RTM_NEWROUTE: u16 = 24;
/// Delete route.
pub const RTM_DELROUTE: u16 = 25;
/// Get route.
pub const RTM_GETROUTE: u16 = 26;

/// New neighbor (ARP entry).
pub const RTM_NEWNEIGH: u16 = 28;
/// Delete neighbor.
pub const RTM_DELNEIGH: u16 = 29;
/// Get neighbor.
pub const RTM_GETNEIGH: u16 = 30;

/// New rule (policy routing).
pub const RTM_NEWRULE: u16 = 32;
/// Delete rule.
pub const RTM_DELRULE: u16 = 33;
/// Get rule.
pub const RTM_GETRULE: u16 = 34;

/// New queuing discipline.
pub const RTM_NEWQDISC: u16 = 36;
/// Delete qdisc.
pub const RTM_DELQDISC: u16 = 37;
/// Get qdisc.
pub const RTM_GETQDISC: u16 = 38;

// ---------------------------------------------------------------------------
// Route types (rtm_type)
// ---------------------------------------------------------------------------

/// Unspecified route.
pub const RTN_UNSPEC: u8 = 0;
/// Gateway or direct route.
pub const RTN_UNICAST: u8 = 1;
/// Local interface route.
pub const RTN_LOCAL: u8 = 2;
/// Broadcast route.
pub const RTN_BROADCAST: u8 = 3;
/// Anycast route.
pub const RTN_ANYCAST: u8 = 4;
/// Multicast route.
pub const RTN_MULTICAST: u8 = 5;
/// Blackhole (drop silently).
pub const RTN_BLACKHOLE: u8 = 6;
/// Unreachable (ICMP error).
pub const RTN_UNREACHABLE: u8 = 7;
/// Prohibit (ICMP error).
pub const RTN_PROHIBIT: u8 = 8;
/// Throw (continue route lookup in another table).
pub const RTN_THROW: u8 = 9;

// ---------------------------------------------------------------------------
// Route scopes (rtm_scope)
// ---------------------------------------------------------------------------

/// Universe (global route).
pub const RT_SCOPE_UNIVERSE: u8 = 0;
/// Site-local.
pub const RT_SCOPE_SITE: u8 = 200;
/// Link-local.
pub const RT_SCOPE_LINK: u8 = 253;
/// Host-local.
pub const RT_SCOPE_HOST: u8 = 254;
/// Nowhere.
pub const RT_SCOPE_NOWHERE: u8 = 255;

// ---------------------------------------------------------------------------
// Route tables
// ---------------------------------------------------------------------------

/// Unspecified table.
pub const RT_TABLE_UNSPEC: u8 = 0;
/// Default table.
pub const RT_TABLE_DEFAULT: u8 = 253;
/// Main routing table.
pub const RT_TABLE_MAIN: u8 = 254;
/// Local routing table.
pub const RT_TABLE_LOCAL: u8 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_msgs_distinct() {
        let msgs = [RTM_NEWLINK, RTM_DELLINK, RTM_GETLINK, RTM_SETLINK];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_route_msgs_distinct() {
        let msgs = [RTM_NEWROUTE, RTM_DELROUTE, RTM_GETROUTE];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_route_types_distinct() {
        let types = [
            RTN_UNSPEC, RTN_UNICAST, RTN_LOCAL, RTN_BROADCAST,
            RTN_ANYCAST, RTN_MULTICAST, RTN_BLACKHOLE,
            RTN_UNREACHABLE, RTN_PROHIBIT, RTN_THROW,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_scopes_distinct() {
        let scopes = [
            RT_SCOPE_UNIVERSE, RT_SCOPE_SITE,
            RT_SCOPE_LINK, RT_SCOPE_HOST, RT_SCOPE_NOWHERE,
        ];
        for i in 0..scopes.len() {
            for j in (i + 1)..scopes.len() {
                assert_ne!(scopes[i], scopes[j]);
            }
        }
    }

    #[test]
    fn test_tables_distinct() {
        let tables = [
            RT_TABLE_UNSPEC, RT_TABLE_DEFAULT,
            RT_TABLE_MAIN, RT_TABLE_LOCAL,
        ];
        for i in 0..tables.len() {
            for j in (i + 1)..tables.len() {
                assert_ne!(tables[i], tables[j]);
            }
        }
    }

    #[test]
    fn test_msg_groups() {
        // Each group starts at a multiple of 4
        assert_eq!(RTM_NEWLINK % 4, 0);
        assert_eq!(RTM_NEWADDR % 4, 0);
        assert_eq!(RTM_NEWROUTE % 4, 0);
        assert_eq!(RTM_NEWNEIGH % 4, 0);
    }
}
