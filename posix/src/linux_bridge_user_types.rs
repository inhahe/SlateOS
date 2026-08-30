//! `<linux/if_bridge.h>` — Linux bridge driver user-facing constants.
//!
//! The bridge driver implements transparent Ethernet bridging plus
//! STP/RSTP/MRP, VLAN filtering, multicast snooping, and per-port
//! state. This module covers the netlink IFLA_BR_* attribute IDs,
//! the per-port states, and the BPDU constants.

// ---------------------------------------------------------------------------
// STP / RSTP port states (`BR_STATE_*`)
// ---------------------------------------------------------------------------

pub const BR_STATE_DISABLED: u32 = 0;
pub const BR_STATE_LISTENING: u32 = 1;
pub const BR_STATE_LEARNING: u32 = 2;
pub const BR_STATE_FORWARDING: u32 = 3;
pub const BR_STATE_BLOCKING: u32 = 4;

// ---------------------------------------------------------------------------
// BPDU constants (802.1D / 802.1Q)
// ---------------------------------------------------------------------------

/// Bridge group address (LLDP/STP destination MAC, last byte 0x00).
pub const BR_GROUP_ADDR_BYTE5: u8 = 0x00;
pub const BR_GROUP_ADDR_PREFIX_LEN: usize = 5;
/// First 5 bytes of "01:80:C2:00:00:0X" group MAC.
pub const BR_GROUP_ADDR_PREFIX: [u8; 5] = [0x01, 0x80, 0xC2, 0x00, 0x00];

// ---------------------------------------------------------------------------
// IFLA_BR_* attributes (dense from 0..50+)
// ---------------------------------------------------------------------------

pub const IFLA_BR_UNSPEC: u32 = 0;
pub const IFLA_BR_FORWARD_DELAY: u32 = 1;
pub const IFLA_BR_HELLO_TIME: u32 = 2;
pub const IFLA_BR_MAX_AGE: u32 = 3;
pub const IFLA_BR_AGEING_TIME: u32 = 4;
pub const IFLA_BR_STP_STATE: u32 = 5;
pub const IFLA_BR_PRIORITY: u32 = 6;
pub const IFLA_BR_VLAN_FILTERING: u32 = 7;
pub const IFLA_BR_VLAN_PROTOCOL: u32 = 8;
pub const IFLA_BR_GROUP_FWD_MASK: u32 = 9;
pub const IFLA_BR_ROOT_ID: u32 = 10;
pub const IFLA_BR_BRIDGE_ID: u32 = 11;
pub const IFLA_BR_ROOT_PORT: u32 = 12;
pub const IFLA_BR_ROOT_PATH_COST: u32 = 13;
pub const IFLA_BR_TOPOLOGY_CHANGE: u32 = 14;
pub const IFLA_BR_TOPOLOGY_CHANGE_DETECTED: u32 = 15;
pub const IFLA_BR_HELLO_TIMER: u32 = 16;
pub const IFLA_BR_TCN_TIMER: u32 = 17;
pub const IFLA_BR_TOPOLOGY_CHANGE_TIMER: u32 = 18;
pub const IFLA_BR_GC_TIMER: u32 = 19;
pub const IFLA_BR_GROUP_ADDR: u32 = 20;
pub const IFLA_BR_FDB_FLUSH: u32 = 21;
pub const IFLA_BR_MCAST_ROUTER: u32 = 22;
pub const IFLA_BR_MCAST_SNOOPING: u32 = 23;
pub const IFLA_BR_MCAST_QUERY_USE_IFADDR: u32 = 24;
pub const IFLA_BR_MCAST_QUERIER: u32 = 25;

// ---------------------------------------------------------------------------
// Default STP timers (in jiffies-equivalent centiseconds * 100)
// ---------------------------------------------------------------------------

/// Default forward-delay (15 s).
pub const BR_DEFAULT_FORWARD_DELAY_S: u32 = 15;

/// Default hello-time (2 s).
pub const BR_DEFAULT_HELLO_TIME_S: u32 = 2;

/// Default max-age (20 s).
pub const BR_DEFAULT_MAX_AGE_S: u32 = 20;

/// Default MAC ageing time (5 minutes).
pub const BR_DEFAULT_AGEING_TIME_S: u32 = 300;

/// Default bridge priority (mid-range — 0x8000).
pub const BR_DEFAULT_PRIORITY: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_states_dense_0_to_4() {
        let s = [
            BR_STATE_DISABLED,
            BR_STATE_LISTENING,
            BR_STATE_LEARNING,
            BR_STATE_FORWARDING,
            BR_STATE_BLOCKING,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // DISABLED is the initial state.
        assert_eq!(BR_STATE_DISABLED, 0);
    }

    #[test]
    fn test_group_addr_prefix() {
        // 01:80:C2:00:00:0X family covers BPDU, LACP, LLDP.
        assert_eq!(BR_GROUP_ADDR_PREFIX, [0x01, 0x80, 0xC2, 0x00, 0x00]);
        assert_eq!(BR_GROUP_ADDR_PREFIX_LEN, 5);
        assert_eq!(BR_GROUP_ADDR_BYTE5, 0x00);
        // The "01:" first byte marks it as multicast (LSB=1).
        assert_eq!(BR_GROUP_ADDR_PREFIX[0] & 0x01, 1);
    }

    #[test]
    fn test_ifla_br_attrs_dense_0_to_25() {
        let a = [
            IFLA_BR_UNSPEC,
            IFLA_BR_FORWARD_DELAY,
            IFLA_BR_HELLO_TIME,
            IFLA_BR_MAX_AGE,
            IFLA_BR_AGEING_TIME,
            IFLA_BR_STP_STATE,
            IFLA_BR_PRIORITY,
            IFLA_BR_VLAN_FILTERING,
            IFLA_BR_VLAN_PROTOCOL,
            IFLA_BR_GROUP_FWD_MASK,
            IFLA_BR_ROOT_ID,
            IFLA_BR_BRIDGE_ID,
            IFLA_BR_ROOT_PORT,
            IFLA_BR_ROOT_PATH_COST,
            IFLA_BR_TOPOLOGY_CHANGE,
            IFLA_BR_TOPOLOGY_CHANGE_DETECTED,
            IFLA_BR_HELLO_TIMER,
            IFLA_BR_TCN_TIMER,
            IFLA_BR_TOPOLOGY_CHANGE_TIMER,
            IFLA_BR_GC_TIMER,
            IFLA_BR_GROUP_ADDR,
            IFLA_BR_FDB_FLUSH,
            IFLA_BR_MCAST_ROUTER,
            IFLA_BR_MCAST_SNOOPING,
            IFLA_BR_MCAST_QUERY_USE_IFADDR,
            IFLA_BR_MCAST_QUERIER,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_default_stp_timers() {
        // STP defaults from 802.1D.
        assert_eq!(BR_DEFAULT_FORWARD_DELAY_S, 15);
        assert_eq!(BR_DEFAULT_HELLO_TIME_S, 2);
        assert_eq!(BR_DEFAULT_MAX_AGE_S, 20);
        // 2 * (forward_delay - 1) >= max_age — the canonical STP invariant.
        assert!(2 * (BR_DEFAULT_FORWARD_DELAY_S - 1) >= BR_DEFAULT_MAX_AGE_S);
        // Default ageing time (5 minutes).
        assert_eq!(BR_DEFAULT_AGEING_TIME_S, 300);
        assert_eq!(BR_DEFAULT_AGEING_TIME_S / 60, 5);
    }

    #[test]
    fn test_default_priority_is_midrange() {
        // 0x8000 — the IEEE 802.1D default bridge priority.
        assert_eq!(BR_DEFAULT_PRIORITY, 0x8000);
        assert!(BR_DEFAULT_PRIORITY.is_power_of_two());
    }

    #[test]
    fn test_mcast_attrs_clustered() {
        // Multicast snooping / querier attributes sit at 22..25.
        for v in [
            IFLA_BR_MCAST_ROUTER,
            IFLA_BR_MCAST_SNOOPING,
            IFLA_BR_MCAST_QUERY_USE_IFADDR,
            IFLA_BR_MCAST_QUERIER,
        ] {
            assert!((22..=25).contains(&v));
        }
    }
}
