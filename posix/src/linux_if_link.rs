//! `<linux/if_link.h>` — link-layer attributes for rtnetlink.
//!
//! IFLA_* attributes are used in RTM_NEWLINK / RTM_GETLINK netlink
//! messages to describe and configure network interfaces.

// ---------------------------------------------------------------------------
// IFLA_* attribute types (interface link attributes)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_UNSPEC: u16 = 0;
/// Interface address (hardware).
pub const IFLA_ADDRESS: u16 = 1;
/// Broadcast address.
pub const IFLA_BROADCAST: u16 = 2;
/// Interface name.
pub const IFLA_IFNAME: u16 = 3;
/// MTU.
pub const IFLA_MTU: u16 = 4;
/// Link type.
pub const IFLA_LINK: u16 = 5;
/// Queueing discipline.
pub const IFLA_QDISC: u16 = 6;
/// Interface statistics.
pub const IFLA_STATS: u16 = 7;
/// Cost (unused).
pub const IFLA_COST: u16 = 8;
/// Priority (unused).
pub const IFLA_PRIORITY: u16 = 9;
/// Master device.
pub const IFLA_MASTER: u16 = 10;
/// Wireless information.
pub const IFLA_WIRELESS: u16 = 11;
/// Protocol-specific info.
pub const IFLA_PROTINFO: u16 = 12;
/// Transmit queue length.
pub const IFLA_TXQLEN: u16 = 13;
/// MAP (memory-mapped I/O).
pub const IFLA_MAP: u16 = 14;
/// Weight (unused).
pub const IFLA_WEIGHT: u16 = 15;
/// Operational state.
pub const IFLA_OPERSTATE: u16 = 16;
/// Link mode.
pub const IFLA_LINKMODE: u16 = 17;
/// Link info (nested).
pub const IFLA_LINKINFO: u16 = 18;
/// Network namespace PID.
pub const IFLA_NET_NS_PID: u16 = 19;
/// Interface alias.
pub const IFLA_IFALIAS: u16 = 20;
/// Number of VFs.
pub const IFLA_NUM_VF: u16 = 21;
/// VF info list (nested).
pub const IFLA_VFINFO_LIST: u16 = 22;
/// 64-bit interface statistics.
pub const IFLA_STATS64: u16 = 23;
/// VF ports (nested).
pub const IFLA_VF_PORTS: u16 = 24;
/// Port self (nested).
pub const IFLA_PORT_SELF: u16 = 25;
/// AF-specific (nested).
pub const IFLA_AF_SPEC: u16 = 26;
/// Group.
pub const IFLA_GROUP: u16 = 27;
/// Network namespace fd.
pub const IFLA_NET_NS_FD: u16 = 28;
/// Extended interface info.
pub const IFLA_EXT_MASK: u16 = 29;
/// Promiscuity count.
pub const IFLA_PROMISCUITY: u16 = 30;
/// Number of TX queues.
pub const IFLA_NUM_TX_QUEUES: u16 = 31;
/// Number of RX queues.
pub const IFLA_NUM_RX_QUEUES: u16 = 32;
/// Carrier state.
pub const IFLA_CARRIER: u16 = 33;
/// Physical port ID.
pub const IFLA_PHYS_PORT_ID: u16 = 34;
/// Carrier changes count.
pub const IFLA_CARRIER_CHANGES: u16 = 35;
/// Physical switch ID.
pub const IFLA_PHYS_SWITCH_ID: u16 = 36;
/// Link network namespace ID.
pub const IFLA_LINK_NETNSID: u16 = 37;
/// Physical port name.
pub const IFLA_PHYS_PORT_NAME: u16 = 38;
/// Protocol (lower device).
pub const IFLA_PROTO_DOWN: u16 = 39;
/// GSO max segments.
pub const IFLA_GSO_MAX_SEGS: u16 = 40;
/// GSO max size.
pub const IFLA_GSO_MAX_SIZE: u16 = 41;
/// XDP program (nested).
pub const IFLA_XDP: u16 = 43;
/// Minimum MTU.
pub const IFLA_MIN_MTU: u16 = 50;
/// Maximum MTU.
pub const IFLA_MAX_MTU: u16 = 51;

// ---------------------------------------------------------------------------
// IFLA_INFO_* subtypes (nested under IFLA_LINKINFO)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_INFO_UNSPEC: u16 = 0;
/// Link kind string (e.g. "veth", "bridge").
pub const IFLA_INFO_KIND: u16 = 1;
/// Type-specific data (nested).
pub const IFLA_INFO_DATA: u16 = 2;
/// xstats.
pub const IFLA_INFO_XSTATS: u16 = 3;
/// Slave kind string.
pub const IFLA_INFO_SLAVE_KIND: u16 = 4;
/// Slave-specific data.
pub const IFLA_INFO_SLAVE_DATA: u16 = 5;

// ---------------------------------------------------------------------------
// Operational states
// ---------------------------------------------------------------------------

/// Unknown state.
pub const IF_OPER_UNKNOWN: u8 = 0;
/// Interface is not present.
pub const IF_OPER_NOTPRESENT: u8 = 1;
/// Interface is down.
pub const IF_OPER_DOWN: u8 = 2;
/// Lower layer is down.
pub const IF_OPER_LOWERLAYERDOWN: u8 = 3;
/// Interface is in testing mode.
pub const IF_OPER_TESTING: u8 = 4;
/// Interface is dormant.
pub const IF_OPER_DORMANT: u8 = 5;
/// Interface is up.
pub const IF_OPER_UP: u8 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifla_attrs_sequential_start() {
        assert_eq!(IFLA_UNSPEC, 0);
        assert_eq!(IFLA_ADDRESS, 1);
        assert_eq!(IFLA_BROADCAST, 2);
        assert_eq!(IFLA_IFNAME, 3);
        assert_eq!(IFLA_MTU, 4);
    }

    #[test]
    fn test_ifla_attrs_distinct() {
        let attrs = [
            IFLA_UNSPEC, IFLA_ADDRESS, IFLA_BROADCAST, IFLA_IFNAME,
            IFLA_MTU, IFLA_LINK, IFLA_QDISC, IFLA_STATS,
            IFLA_MASTER, IFLA_OPERSTATE, IFLA_LINKINFO,
            IFLA_STATS64, IFLA_AF_SPEC, IFLA_XDP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ifla_info_subtypes() {
        assert_eq!(IFLA_INFO_UNSPEC, 0);
        assert_eq!(IFLA_INFO_KIND, 1);
        assert_eq!(IFLA_INFO_DATA, 2);
        assert_eq!(IFLA_INFO_XSTATS, 3);
        assert_eq!(IFLA_INFO_SLAVE_KIND, 4);
        assert_eq!(IFLA_INFO_SLAVE_DATA, 5);
    }

    #[test]
    fn test_oper_states_sequential() {
        assert_eq!(IF_OPER_UNKNOWN, 0);
        assert_eq!(IF_OPER_NOTPRESENT, 1);
        assert_eq!(IF_OPER_DOWN, 2);
        assert_eq!(IF_OPER_LOWERLAYERDOWN, 3);
        assert_eq!(IF_OPER_TESTING, 4);
        assert_eq!(IF_OPER_DORMANT, 5);
        assert_eq!(IF_OPER_UP, 6);
    }

    #[test]
    fn test_mtu_attrs() {
        assert_ne!(IFLA_MIN_MTU, IFLA_MAX_MTU);
        assert_ne!(IFLA_MTU, IFLA_MIN_MTU);
    }
}
