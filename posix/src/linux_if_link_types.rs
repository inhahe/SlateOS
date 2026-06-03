//! `<linux/if_link.h>` — Netlink interface info attribute constants.
//!
//! These constants define the rtnetlink attributes for querying
//! and configuring network interfaces via the IFLA_* attribute
//! namespace in RTM_GETLINK/RTM_SETLINK messages.

// ---------------------------------------------------------------------------
// IFLA_* attributes (interface link attributes)
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const IFLA_UNSPEC: u16 = 0;
/// Interface hardware (MAC) address.
pub const IFLA_ADDRESS: u16 = 1;
/// Interface broadcast address.
pub const IFLA_BROADCAST: u16 = 2;
/// Interface name (string).
pub const IFLA_IFNAME: u16 = 3;
/// Interface MTU.
pub const IFLA_MTU: u16 = 4;
/// Master interface index (bridge, bond).
pub const IFLA_LINK: u16 = 5;
/// Interface queue discipline.
pub const IFLA_QDISC: u16 = 6;
/// Interface statistics (struct rtnl_link_stats).
pub const IFLA_STATS: u16 = 7;
/// Interface cost (routing).
pub const IFLA_COST: u16 = 8;
/// Interface priority.
pub const IFLA_PRIORITY: u16 = 9;
/// Master device index.
pub const IFLA_MASTER: u16 = 10;
/// Wireless extensions info.
pub const IFLA_WIRELESS: u16 = 11;
/// Protocol info.
pub const IFLA_PROTINFO: u16 = 12;
/// TX queue length.
pub const IFLA_TXQLEN: u16 = 13;
/// Interface map.
pub const IFLA_MAP: u16 = 14;
/// Interface weight.
pub const IFLA_WEIGHT: u16 = 15;
/// Operational state.
pub const IFLA_OPERSTATE: u16 = 16;
/// Link mode.
pub const IFLA_LINKMODE: u16 = 17;
/// Link info (nested: IFLA_INFO_KIND, etc.).
pub const IFLA_LINKINFO: u16 = 18;
/// Network namespace PID.
pub const IFLA_NET_NS_PID: u16 = 19;
/// Interface alias (alternate name).
pub const IFLA_IFALIAS: u16 = 20;
/// Number of VFs.
pub const IFLA_NUM_VF: u16 = 21;
/// Group.
pub const IFLA_GROUP: u16 = 27;
/// Network namespace FD.
pub const IFLA_NET_NS_FD: u16 = 28;
/// Extended mask.
pub const IFLA_EXT_MASK: u16 = 29;
/// Promiscuity count.
pub const IFLA_PROMISCUITY: u16 = 30;
/// Number of TX queues.
pub const IFLA_NUM_TX_QUEUES: u16 = 31;
/// Number of RX queues.
pub const IFLA_NUM_RX_QUEUES: u16 = 32;
/// GSO max size.
pub const IFLA_GSO_MAX_SIZE: u16 = 40;
/// GSO max segments.
pub const IFLA_GSO_MAX_SEGS: u16 = 41;
/// XDP program info (nested).
pub const IFLA_XDP: u16 = 43;

// ---------------------------------------------------------------------------
// IFLA_INFO_* (link info sub-attributes)
// ---------------------------------------------------------------------------

/// Interface type name (e.g. "veth", "bridge").
pub const IFLA_INFO_KIND: u16 = 1;
/// Type-specific data (nested).
pub const IFLA_INFO_DATA: u16 = 2;
/// Slave type name.
pub const IFLA_INFO_SLAVE_KIND: u16 = 4;
/// Slave-specific data (nested).
pub const IFLA_INFO_SLAVE_DATA: u16 = 5;

// ---------------------------------------------------------------------------
// Operational states
// ---------------------------------------------------------------------------

/// Unknown state.
pub const IF_OPER_UNKNOWN: u8 = 0;
/// Not present.
pub const IF_OPER_NOTPRESENT: u8 = 1;
/// Down.
pub const IF_OPER_DOWN: u8 = 2;
/// Lower layer down.
pub const IF_OPER_LOWERLAYERDOWN: u8 = 3;
/// Testing.
pub const IF_OPER_TESTING: u8 = 4;
/// Dormant.
pub const IF_OPER_DORMANT: u8 = 5;
/// Up.
pub const IF_OPER_UP: u8 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifla_attrs_distinct() {
        let attrs = [
            IFLA_UNSPEC,
            IFLA_ADDRESS,
            IFLA_BROADCAST,
            IFLA_IFNAME,
            IFLA_MTU,
            IFLA_LINK,
            IFLA_QDISC,
            IFLA_STATS,
            IFLA_COST,
            IFLA_PRIORITY,
            IFLA_MASTER,
            IFLA_WIRELESS,
            IFLA_PROTINFO,
            IFLA_TXQLEN,
            IFLA_MAP,
            IFLA_WEIGHT,
            IFLA_OPERSTATE,
            IFLA_LINKMODE,
            IFLA_LINKINFO,
            IFLA_NET_NS_PID,
            IFLA_IFALIAS,
            IFLA_NUM_VF,
            IFLA_GROUP,
            IFLA_NET_NS_FD,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_oper_states_distinct() {
        let states = [
            IF_OPER_UNKNOWN,
            IF_OPER_NOTPRESENT,
            IF_OPER_DOWN,
            IF_OPER_LOWERLAYERDOWN,
            IF_OPER_TESTING,
            IF_OPER_DORMANT,
            IF_OPER_UP,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ifla_info_distinct() {
        let infos = [
            IFLA_INFO_KIND,
            IFLA_INFO_DATA,
            IFLA_INFO_SLAVE_KIND,
            IFLA_INFO_SLAVE_DATA,
        ];
        for i in 0..infos.len() {
            for j in (i + 1)..infos.len() {
                assert_ne!(infos[i], infos[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(IFLA_UNSPEC, 0);
    }
}
