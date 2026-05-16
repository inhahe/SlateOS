//! `<linux/if_bridge.h>` — bridge interface attributes.
//!
//! Used with RTM_NEWLINK/RTM_GETLINK netlink messages when the
//! interface kind is "bridge" or "bridge_slave".

// ---------------------------------------------------------------------------
// Bridge ioctl commands (legacy, via /dev/net/bridge)
// ---------------------------------------------------------------------------

/// Add bridge.
pub const BRCTL_ADD_BRIDGE: u64 = 2;
/// Delete bridge.
pub const BRCTL_DEL_BRIDGE: u64 = 3;
/// Add port to bridge.
pub const BRCTL_ADD_IF: u64 = 4;
/// Remove port from bridge.
pub const BRCTL_DEL_IF: u64 = 5;
/// Get bridge info.
pub const BRCTL_GET_BRIDGE_INFO: u64 = 6;
/// Get port list.
pub const BRCTL_GET_PORT_LIST: u64 = 7;
/// Set bridge forward delay.
pub const BRCTL_SET_BRIDGE_FORWARD_DELAY: u64 = 8;
/// Set bridge hello time.
pub const BRCTL_SET_BRIDGE_HELLO_TIME: u64 = 9;
/// Set bridge max age.
pub const BRCTL_SET_BRIDGE_MAX_AGE: u64 = 10;
/// Set ageing time.
pub const BRCTL_SET_AGEING_TIME: u64 = 11;
/// Set bridge STP state.
pub const BRCTL_SET_BRIDGE_STP_STATE: u64 = 14;
/// Set bridge priority.
pub const BRCTL_SET_BRIDGE_PRIORITY: u64 = 15;
/// Get FDB entries.
pub const BRCTL_GET_FDB_ENTRIES: u64 = 17;

// ---------------------------------------------------------------------------
// Bridge netlink attributes (IFLA_BR_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_BR_UNSPEC: u16 = 0;
/// Forward delay.
pub const IFLA_BR_FORWARD_DELAY: u16 = 1;
/// Hello time.
pub const IFLA_BR_HELLO_TIME: u16 = 2;
/// Max age.
pub const IFLA_BR_MAX_AGE: u16 = 3;
/// Ageing time.
pub const IFLA_BR_AGEING_TIME: u16 = 4;
/// STP state.
pub const IFLA_BR_STP_STATE: u16 = 5;
/// Bridge priority.
pub const IFLA_BR_PRIORITY: u16 = 6;
/// VLAN filtering.
pub const IFLA_BR_VLAN_FILTERING: u16 = 7;
/// VLAN protocol.
pub const IFLA_BR_VLAN_PROTOCOL: u16 = 8;
/// Group FWD mask.
pub const IFLA_BR_GROUP_FWD_MASK: u16 = 9;
/// Root path cost.
pub const IFLA_BR_ROOT_PATH_COST: u16 = 11;
/// Root port.
pub const IFLA_BR_ROOT_PORT: u16 = 12;
/// Multicast router.
pub const IFLA_BR_MCAST_ROUTER: u16 = 22;
/// Multicast snooping.
pub const IFLA_BR_MCAST_SNOOPING: u16 = 23;
/// Multicast query use IFADDR.
pub const IFLA_BR_MCAST_QUERY_USE_IFADDR: u16 = 24;
/// VLAN default PVID.
pub const IFLA_BR_VLAN_DEFAULT_PVID: u16 = 39;
/// NF call iptables.
pub const IFLA_BR_NF_CALL_IPTABLES: u16 = 42;
/// NF call ip6tables.
pub const IFLA_BR_NF_CALL_IP6TABLES: u16 = 43;
/// NF call arptables.
pub const IFLA_BR_NF_CALL_ARPTABLES: u16 = 44;

// ---------------------------------------------------------------------------
// Bridge port states
// ---------------------------------------------------------------------------

/// Port is disabled.
pub const BR_STATE_DISABLED: u8 = 0;
/// Port is listening.
pub const BR_STATE_LISTENING: u8 = 1;
/// Port is learning.
pub const BR_STATE_LEARNING: u8 = 2;
/// Port is forwarding.
pub const BR_STATE_FORWARDING: u8 = 3;
/// Port is blocking.
pub const BR_STATE_BLOCKING: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brctl_commands_distinct() {
        let cmds = [
            BRCTL_ADD_BRIDGE, BRCTL_DEL_BRIDGE, BRCTL_ADD_IF,
            BRCTL_DEL_IF, BRCTL_GET_BRIDGE_INFO, BRCTL_GET_PORT_LIST,
            BRCTL_SET_BRIDGE_FORWARD_DELAY, BRCTL_SET_BRIDGE_HELLO_TIME,
            BRCTL_SET_BRIDGE_MAX_AGE, BRCTL_SET_AGEING_TIME,
            BRCTL_SET_BRIDGE_STP_STATE, BRCTL_SET_BRIDGE_PRIORITY,
            BRCTL_GET_FDB_ENTRIES,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ifla_br_attrs_sequential() {
        assert_eq!(IFLA_BR_UNSPEC, 0);
        assert_eq!(IFLA_BR_FORWARD_DELAY, 1);
        assert_eq!(IFLA_BR_HELLO_TIME, 2);
        assert_eq!(IFLA_BR_MAX_AGE, 3);
        assert_eq!(IFLA_BR_AGEING_TIME, 4);
        assert_eq!(IFLA_BR_STP_STATE, 5);
        assert_eq!(IFLA_BR_PRIORITY, 6);
    }

    #[test]
    fn test_port_states_sequential() {
        assert_eq!(BR_STATE_DISABLED, 0);
        assert_eq!(BR_STATE_LISTENING, 1);
        assert_eq!(BR_STATE_LEARNING, 2);
        assert_eq!(BR_STATE_FORWARDING, 3);
        assert_eq!(BR_STATE_BLOCKING, 4);
    }

    #[test]
    fn test_nf_call_attrs_distinct() {
        assert_ne!(IFLA_BR_NF_CALL_IPTABLES, IFLA_BR_NF_CALL_IP6TABLES);
        assert_ne!(IFLA_BR_NF_CALL_IP6TABLES, IFLA_BR_NF_CALL_ARPTABLES);
    }
}
