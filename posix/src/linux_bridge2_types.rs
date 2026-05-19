//! `<linux/if_bridge.h>` — Additional bridge constants.
//!
//! Supplementary bridge constants covering bridge port states,
//! bridge flags, and VLAN filtering modes.

// ---------------------------------------------------------------------------
// Bridge port states (BR_STATE_*)
// ---------------------------------------------------------------------------

/// Port is disabled.
pub const BR_STATE_DISABLED: u32 = 0;
/// Port is listening.
pub const BR_STATE_LISTENING: u32 = 1;
/// Port is learning.
pub const BR_STATE_LEARNING: u32 = 2;
/// Port is forwarding.
pub const BR_STATE_FORWARDING: u32 = 3;
/// Port is blocking.
pub const BR_STATE_BLOCKING: u32 = 4;

// ---------------------------------------------------------------------------
// Bridge flags
// ---------------------------------------------------------------------------

/// Hairpin mode (reflect frames back to port).
pub const BR_HAIRPIN_MODE: u32 = 1 << 0;
/// BPDU guard.
pub const BR_BPDU_GUARD: u32 = 1 << 1;
/// Root block.
pub const BR_ROOT_BLOCK: u32 = 1 << 2;
/// Multicast fast leave.
pub const BR_MULTICAST_FAST_LEAVE: u32 = 1 << 3;
/// Admin disabled.
pub const BR_ADMIN_COST: u32 = 1 << 4;
/// Learning enabled.
pub const BR_LEARNING: u32 = 1 << 5;
/// Flood enabled.
pub const BR_FLOOD: u32 = 1 << 6;
/// Auto-isolate port.
pub const BR_AUTO_MASK: u32 = 1 << 7;
/// Proxy ARP.
pub const BR_PROXYARP: u32 = 1 << 8;
/// Learning sync.
pub const BR_LEARNING_SYNC: u32 = 1 << 9;
/// Proxy ARP WiFi.
pub const BR_PROXYARP_WIFI: u32 = 1 << 10;
/// Multicast flood.
pub const BR_MCAST_FLOOD: u32 = 1 << 11;
/// Broadcast flood.
pub const BR_BCAST_FLOOD: u32 = 1 << 12;
/// Neigh suppress.
pub const BR_NEIGH_SUPPRESS: u32 = 1 << 13;
/// Isolated port.
pub const BR_ISOLATED: u32 = 1 << 14;
/// Multicast to unicast.
pub const BR_MRP_AWARE: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Bridge VLAN flags
// ---------------------------------------------------------------------------

/// VLAN is PVID.
pub const BRIDGE_VLAN_INFO_PVID: u16 = 1 << 1;
/// VLAN packets should be untagged on egress.
pub const BRIDGE_VLAN_INFO_UNTAGGED: u16 = 1 << 2;
/// VLAN range start.
pub const BRIDGE_VLAN_INFO_RANGE_BEGIN: u16 = 1 << 3;
/// VLAN range end.
pub const BRIDGE_VLAN_INFO_RANGE_END: u16 = 1 << 4;
/// Only output to bridge group.
pub const BRIDGE_VLAN_INFO_BRENTRY: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_states_distinct() {
        let states = [
            BR_STATE_DISABLED, BR_STATE_LISTENING,
            BR_STATE_LEARNING, BR_STATE_FORWARDING,
            BR_STATE_BLOCKING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_bridge_flags_power_of_two() {
        let flags = [
            BR_HAIRPIN_MODE, BR_BPDU_GUARD, BR_ROOT_BLOCK,
            BR_MULTICAST_FAST_LEAVE, BR_ADMIN_COST, BR_LEARNING,
            BR_FLOOD, BR_AUTO_MASK, BR_PROXYARP, BR_LEARNING_SYNC,
            BR_PROXYARP_WIFI, BR_MCAST_FLOOD, BR_BCAST_FLOOD,
            BR_NEIGH_SUPPRESS, BR_ISOLATED, BR_MRP_AWARE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_bridge_flags_no_overlap() {
        let flags = [
            BR_HAIRPIN_MODE, BR_BPDU_GUARD, BR_ROOT_BLOCK,
            BR_MULTICAST_FAST_LEAVE, BR_ADMIN_COST, BR_LEARNING,
            BR_FLOOD, BR_AUTO_MASK, BR_PROXYARP, BR_LEARNING_SYNC,
            BR_PROXYARP_WIFI, BR_MCAST_FLOOD, BR_BCAST_FLOOD,
            BR_NEIGH_SUPPRESS, BR_ISOLATED, BR_MRP_AWARE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_vlan_flags_power_of_two() {
        let flags = [
            BRIDGE_VLAN_INFO_PVID, BRIDGE_VLAN_INFO_UNTAGGED,
            BRIDGE_VLAN_INFO_RANGE_BEGIN, BRIDGE_VLAN_INFO_RANGE_END,
            BRIDGE_VLAN_INFO_BRENTRY,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_vlan_flags_no_overlap() {
        let flags = [
            BRIDGE_VLAN_INFO_PVID, BRIDGE_VLAN_INFO_UNTAGGED,
            BRIDGE_VLAN_INFO_RANGE_BEGIN, BRIDGE_VLAN_INFO_RANGE_END,
            BRIDGE_VLAN_INFO_BRENTRY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
