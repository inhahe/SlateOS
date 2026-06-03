//! `<linux/rtnetlink.h>` — `NETLINK_ROUTE` message types.
//!
//! `iproute2` (`ip link`, `ip addr`, `ip route`, `ip rule`) and
//! `NetworkManager` all talk to the kernel over a `NETLINK_ROUTE`
//! socket using the `RTM_*` message types defined here. systemd-
//! networkd is built on the same protocol.

// ---------------------------------------------------------------------------
// `AF_NETLINK` family for routing
// ---------------------------------------------------------------------------

pub const NETLINK_ROUTE: u32 = 0;

// ---------------------------------------------------------------------------
// `RTM_*` — link / address / route / neigh CRUD (dense blocks of 4)
// ---------------------------------------------------------------------------

pub const RTM_BASE: u16 = 16;

pub const RTM_NEWLINK: u16 = 16;
pub const RTM_DELLINK: u16 = 17;
pub const RTM_GETLINK: u16 = 18;
pub const RTM_SETLINK: u16 = 19;

pub const RTM_NEWADDR: u16 = 20;
pub const RTM_DELADDR: u16 = 21;
pub const RTM_GETADDR: u16 = 22;

pub const RTM_NEWROUTE: u16 = 24;
pub const RTM_DELROUTE: u16 = 25;
pub const RTM_GETROUTE: u16 = 26;

pub const RTM_NEWNEIGH: u16 = 28;
pub const RTM_DELNEIGH: u16 = 29;
pub const RTM_GETNEIGH: u16 = 30;

pub const RTM_NEWRULE: u16 = 32;
pub const RTM_DELRULE: u16 = 33;
pub const RTM_GETRULE: u16 = 34;

pub const RTM_NEWQDISC: u16 = 36;
pub const RTM_DELQDISC: u16 = 37;
pub const RTM_GETQDISC: u16 = 38;

pub const RTM_NEWTCLASS: u16 = 40;
pub const RTM_DELTCLASS: u16 = 41;
pub const RTM_GETTCLASS: u16 = 42;

pub const RTM_NEWTFILTER: u16 = 44;
pub const RTM_DELTFILTER: u16 = 45;
pub const RTM_GETTFILTER: u16 = 46;

// ---------------------------------------------------------------------------
// `RTNLGRP_*` multicast groups (subset)
// ---------------------------------------------------------------------------

pub const RTNLGRP_LINK: u32 = 1;
pub const RTNLGRP_NOTIFY: u32 = 2;
pub const RTNLGRP_NEIGH: u32 = 3;
pub const RTNLGRP_TC: u32 = 4;
pub const RTNLGRP_IPV4_IFADDR: u32 = 5;
pub const RTNLGRP_IPV4_MROUTE: u32 = 6;
pub const RTNLGRP_IPV4_ROUTE: u32 = 7;
pub const RTNLGRP_IPV4_RULE: u32 = 8;
pub const RTNLGRP_IPV6_IFADDR: u32 = 9;
pub const RTNLGRP_IPV6_MROUTE: u32 = 10;
pub const RTNLGRP_IPV6_ROUTE: u32 = 11;
pub const RTNLGRP_IPV6_IFINFO: u32 = 12;

// ---------------------------------------------------------------------------
// Route scope / type / protocol selectors
// ---------------------------------------------------------------------------

pub const RT_SCOPE_UNIVERSE: u8 = 0;
pub const RT_SCOPE_SITE: u8 = 200;
pub const RT_SCOPE_LINK: u8 = 253;
pub const RT_SCOPE_HOST: u8 = 254;
pub const RT_SCOPE_NOWHERE: u8 = 255;

pub const RTN_UNSPEC: u8 = 0;
pub const RTN_UNICAST: u8 = 1;
pub const RTN_LOCAL: u8 = 2;
pub const RTN_BROADCAST: u8 = 3;
pub const RTN_ANYCAST: u8 = 4;
pub const RTN_MULTICAST: u8 = 5;
pub const RTN_BLACKHOLE: u8 = 6;
pub const RTN_UNREACHABLE: u8 = 7;
pub const RTN_PROHIBIT: u8 = 8;
pub const RTN_THROW: u8 = 9;
pub const RTN_NAT: u8 = 10;
pub const RTN_XRESOLVE: u8 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_netlink_route_is_family_zero() {
        assert_eq!(NETLINK_ROUTE, 0);
    }

    #[test]
    fn test_rtm_blocks_of_4() {
        // Each CRUD block (link/addr/route/neigh/rule/qdisc/tclass/tfilter)
        // takes a 4-wide slot starting from 16, even though only NEW/DEL/GET
        // are typically allocated. RTM_BASE anchors at 16.
        assert_eq!(RTM_BASE, 16);
        assert_eq!(RTM_NEWLINK, RTM_BASE);
        // 8 blocks * 4 = 32 → GETTFILTER lands at 16+30 = 46.
        assert_eq!(RTM_GETTFILTER, RTM_BASE + 30);
    }

    #[test]
    fn test_new_del_get_triples_consecutive() {
        // Every "new/del/get" triple sits at consecutive op numbers.
        assert_eq!(RTM_DELLINK, RTM_NEWLINK + 1);
        assert_eq!(RTM_GETLINK, RTM_NEWLINK + 2);
        assert_eq!(RTM_DELADDR, RTM_NEWADDR + 1);
        assert_eq!(RTM_GETROUTE, RTM_NEWROUTE + 2);
        assert_eq!(RTM_DELNEIGH, RTM_NEWNEIGH + 1);
        assert_eq!(RTM_GETTCLASS, RTM_NEWTCLASS + 2);
    }

    #[test]
    fn test_mcast_groups_dense_1_to_12() {
        let g = [
            RTNLGRP_LINK,
            RTNLGRP_NOTIFY,
            RTNLGRP_NEIGH,
            RTNLGRP_TC,
            RTNLGRP_IPV4_IFADDR,
            RTNLGRP_IPV4_MROUTE,
            RTNLGRP_IPV4_ROUTE,
            RTNLGRP_IPV4_RULE,
            RTNLGRP_IPV6_IFADDR,
            RTNLGRP_IPV6_MROUTE,
            RTNLGRP_IPV6_ROUTE,
            RTNLGRP_IPV6_IFINFO,
        ];
        for (i, &v) in g.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_rt_scope_well_known() {
        // RT_SCOPE_UNIVERSE / LINK / HOST / NOWHERE are the four
        // "named" scopes used by iproute2.
        assert_eq!(RT_SCOPE_UNIVERSE, 0);
        assert_eq!(RT_SCOPE_LINK, 253);
        assert_eq!(RT_SCOPE_HOST, 254);
        assert_eq!(RT_SCOPE_NOWHERE, 255);
        // SITE is the only other commonly-referenced value.
        assert_eq!(RT_SCOPE_SITE, 200);
    }

    #[test]
    fn test_rtn_types_dense_0_to_11() {
        let t = [
            RTN_UNSPEC,
            RTN_UNICAST,
            RTN_LOCAL,
            RTN_BROADCAST,
            RTN_ANYCAST,
            RTN_MULTICAST,
            RTN_BLACKHOLE,
            RTN_UNREACHABLE,
            RTN_PROHIBIT,
            RTN_THROW,
            RTN_NAT,
            RTN_XRESOLVE,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
