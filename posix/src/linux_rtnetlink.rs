//! `<linux/rtnetlink.h>` — routing netlink message types and attributes.
//!
//! rtnetlink is the primary interface for managing routing tables,
//! network interfaces, addresses, and neighbors on Linux. Used by
//! iproute2 (ip command), NetworkManager, and systemd-networkd.

pub use crate::linux_netlink::NLM_F_CREATE;
pub use crate::linux_netlink::NLM_F_DUMP;
pub use crate::linux_netlink::NLM_F_EXCL;
pub use crate::linux_netlink::NLM_F_REQUEST;
pub use crate::linux_netlink::NLMSG_DONE;
pub use crate::linux_netlink::Nlmsghdr;

// ---------------------------------------------------------------------------
// RTM_* message types
// ---------------------------------------------------------------------------

/// New link (interface).
pub const RTM_NEWLINK: u16 = 16;
/// Delete link.
pub const RTM_DELLINK: u16 = 17;
/// Get link info.
pub const RTM_GETLINK: u16 = 18;
/// Set link.
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

/// New neighbor.
pub const RTM_NEWNEIGH: u16 = 28;
/// Delete neighbor.
pub const RTM_DELNEIGH: u16 = 29;
/// Get neighbor.
pub const RTM_GETNEIGH: u16 = 30;

/// New routing rule.
pub const RTM_NEWRULE: u16 = 32;
/// Delete routing rule.
pub const RTM_DELRULE: u16 = 33;
/// Get routing rule.
pub const RTM_GETRULE: u16 = 34;

/// New qdisc.
pub const RTM_NEWQDISC: u16 = 36;
/// Delete qdisc.
pub const RTM_DELQDISC: u16 = 37;
/// Get qdisc.
pub const RTM_GETQDISC: u16 = 38;

/// New traffic class.
pub const RTM_NEWTCLASS: u16 = 40;
/// Delete traffic class.
pub const RTM_DELTCLASS: u16 = 41;
/// Get traffic class.
pub const RTM_GETTCLASS: u16 = 42;

/// New traffic filter.
pub const RTM_NEWTFILTER: u16 = 44;
/// Delete traffic filter.
pub const RTM_DELTFILTER: u16 = 45;
/// Get traffic filter.
pub const RTM_GETTFILTER: u16 = 46;

/// New nexthop.
pub const RTM_NEWNEXTHOP: u16 = 104;
/// Delete nexthop.
pub const RTM_DELNEXTHOP: u16 = 105;
/// Get nexthop.
pub const RTM_GETNEXTHOP: u16 = 106;

// ---------------------------------------------------------------------------
// Ifinfomsg struct (RTM_*LINK payload)
// ---------------------------------------------------------------------------

/// Interface info message (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifinfomsg {
    /// Address family.
    pub ifi_family: u8,
    /// Padding.
    _pad: u8,
    /// Device type (ARPHRD_*).
    pub ifi_type: u16,
    /// Interface index.
    pub ifi_index: i32,
    /// Interface flags (IFF_*).
    pub ifi_flags: u32,
    /// Change mask.
    pub ifi_change: u32,
}

impl Ifinfomsg {
    /// Create a zeroed interface info message.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Rtmsg struct (RTM_*ROUTE payload)
// ---------------------------------------------------------------------------

/// Route message (12 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rtmsg {
    /// Address family.
    pub rtm_family: u8,
    /// Destination prefix length.
    pub rtm_dst_len: u8,
    /// Source prefix length.
    pub rtm_src_len: u8,
    /// TOS filter.
    pub rtm_tos: u8,
    /// Routing table ID.
    pub rtm_table: u8,
    /// Routing protocol.
    pub rtm_protocol: u8,
    /// Route scope.
    pub rtm_scope: u8,
    /// Route type.
    pub rtm_type: u8,
    /// Flags.
    pub rtm_flags: u32,
}

impl Rtmsg {
    /// Create a zeroed route message.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// RTA_* route attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const RTA_UNSPEC: u16 = 0;
/// Destination address.
pub const RTA_DST: u16 = 1;
/// Source address.
pub const RTA_SRC: u16 = 2;
/// Input interface index.
pub const RTA_IIF: u16 = 3;
/// Output interface index.
pub const RTA_OIF: u16 = 4;
/// Gateway address.
pub const RTA_GATEWAY: u16 = 5;
/// Priority.
pub const RTA_PRIORITY: u16 = 6;
/// Preferred source address.
pub const RTA_PREFSRC: u16 = 7;
/// Metrics (nested).
pub const RTA_METRICS: u16 = 8;
/// Multipath (nested).
pub const RTA_MULTIPATH: u16 = 9;
/// Flow.
pub const RTA_FLOW: u16 = 11;
/// Cacheinfo.
pub const RTA_CACHEINFO: u16 = 12;
/// Routing table.
pub const RTA_TABLE: u16 = 15;
/// Mark.
pub const RTA_MARK: u16 = 16;
/// Pref.
pub const RTA_PREF: u16 = 20;
/// Encap type.
pub const RTA_ENCAP_TYPE: u16 = 21;
/// Encap data.
pub const RTA_ENCAP: u16 = 22;

// ---------------------------------------------------------------------------
// Route types (rtm_type)
// ---------------------------------------------------------------------------

/// Unspecified route.
pub const RTN_UNSPEC: u8 = 0;
/// Gateway or direct.
pub const RTN_UNICAST: u8 = 1;
/// Local interface route.
pub const RTN_LOCAL: u8 = 2;
/// Broadcast route.
pub const RTN_BROADCAST: u8 = 3;
/// Anycast route.
pub const RTN_ANYCAST: u8 = 4;
/// Multicast route.
pub const RTN_MULTICAST: u8 = 5;
/// Blackhole (drop).
pub const RTN_BLACKHOLE: u8 = 6;
/// Unreachable.
pub const RTN_UNREACHABLE: u8 = 7;
/// Prohibit.
pub const RTN_PROHIBIT: u8 = 8;
/// Throw (continue lookup in another table).
pub const RTN_THROW: u8 = 9;

// ---------------------------------------------------------------------------
// Route protocols (rtm_protocol)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const RTPROT_UNSPEC: u8 = 0;
/// Redirect.
pub const RTPROT_REDIRECT: u8 = 1;
/// Kernel-generated.
pub const RTPROT_KERNEL: u8 = 2;
/// Boot-time.
pub const RTPROT_BOOT: u8 = 3;
/// Static route.
pub const RTPROT_STATIC: u8 = 4;
/// Routing protocol (zebra/bird).
pub const RTPROT_ZEBRA: u8 = 11;
/// DHCP route.
pub const RTPROT_DHCP: u8 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtm_types_grouped() {
        // Link messages: 16-19
        assert_eq!(RTM_NEWLINK, 16);
        assert_eq!(RTM_DELLINK, 17);
        assert_eq!(RTM_GETLINK, 18);
        // Addr messages: 20-22
        assert_eq!(RTM_NEWADDR, 20);
        // Route messages: 24-26
        assert_eq!(RTM_NEWROUTE, 24);
    }

    #[test]
    fn test_ifinfomsg_size() {
        assert_eq!(core::mem::size_of::<Ifinfomsg>(), 16);
    }

    #[test]
    fn test_rtmsg_size() {
        assert_eq!(core::mem::size_of::<Rtmsg>(), 12);
    }

    #[test]
    fn test_rta_attrs_sequential() {
        assert_eq!(RTA_UNSPEC, 0);
        assert_eq!(RTA_DST, 1);
        assert_eq!(RTA_SRC, 2);
        assert_eq!(RTA_OIF, 4);
        assert_eq!(RTA_GATEWAY, 5);
    }

    #[test]
    fn test_rtn_types_sequential() {
        assert_eq!(RTN_UNSPEC, 0);
        assert_eq!(RTN_UNICAST, 1);
        assert_eq!(RTN_LOCAL, 2);
        assert_eq!(RTN_BROADCAST, 3);
    }

    #[test]
    fn test_protocols() {
        assert_eq!(RTPROT_KERNEL, 2);
        assert_eq!(RTPROT_STATIC, 4);
        assert_eq!(RTPROT_DHCP, 16);
    }
}
