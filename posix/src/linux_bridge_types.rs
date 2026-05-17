//! `<linux/if_bridge.h>` — Bridge and STP constants.
//!
//! Linux bridging implements a software Ethernet switch. Frames are
//! forwarded based on learned MAC addresses. Spanning Tree Protocol
//! (STP/RSTP) prevents loops in redundant topologies. VLAN filtering
//! provides per-port VLAN membership. Used by container networking,
//! VMs, and network virtualization.

// ---------------------------------------------------------------------------
// Bridge port states (STP)
// ---------------------------------------------------------------------------

/// Port is disabled.
pub const BR_STATE_DISABLED: u8 = 0;
/// Port is listening (STP: not forwarding, learning topology).
pub const BR_STATE_LISTENING: u8 = 1;
/// Port is learning (building MAC table).
pub const BR_STATE_LEARNING: u8 = 2;
/// Port is forwarding (normal operation).
pub const BR_STATE_FORWARDING: u8 = 3;
/// Port is blocking (STP: loop prevention).
pub const BR_STATE_BLOCKING: u8 = 4;

// ---------------------------------------------------------------------------
// Bridge netlink attributes (IFLA_BR_*)
// ---------------------------------------------------------------------------

/// Forward delay (STP parameter).
pub const IFLA_BR_FORWARD_DELAY: u32 = 1;
/// Hello time (STP keepalive interval).
pub const IFLA_BR_HELLO_TIME: u32 = 2;
/// Max age (STP BPDU timeout).
pub const IFLA_BR_MAX_AGE: u32 = 3;
/// Ageing time (MAC table entry timeout).
pub const IFLA_BR_AGEING_TIME: u32 = 4;
/// STP enable/disable.
pub const IFLA_BR_STP_STATE: u32 = 5;
/// Bridge priority (STP root election).
pub const IFLA_BR_PRIORITY: u32 = 6;
/// VLAN filtering enable.
pub const IFLA_BR_VLAN_FILTERING: u32 = 7;

// ---------------------------------------------------------------------------
// Bridge flags
// ---------------------------------------------------------------------------

/// Enable hairpin mode (send back to source port).
pub const BR_HAIRPIN_MODE: u32 = 1 << 0;
/// Enable BPDU guard (disable port on BPDU receipt).
pub const BR_BPDU_GUARD: u32 = 1 << 1;
/// Enable root guard (prevent port from becoming root).
pub const BR_ROOT_BLOCK: u32 = 1 << 2;
/// Fast leave (immediate IGMP leave processing).
pub const BR_MULTICAST_FAST_LEAVE: u32 = 1 << 3;
/// Learning enable.
pub const BR_LEARNING: u32 = 1 << 4;
/// Flooding enable.
pub const BR_FLOOD: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_states_distinct() {
        let states = [
            BR_STATE_DISABLED, BR_STATE_LISTENING, BR_STATE_LEARNING,
            BR_STATE_FORWARDING, BR_STATE_BLOCKING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_netlink_attrs_distinct() {
        let attrs = [
            IFLA_BR_FORWARD_DELAY, IFLA_BR_HELLO_TIME, IFLA_BR_MAX_AGE,
            IFLA_BR_AGEING_TIME, IFLA_BR_STP_STATE, IFLA_BR_PRIORITY,
            IFLA_BR_VLAN_FILTERING,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            BR_HAIRPIN_MODE, BR_BPDU_GUARD, BR_ROOT_BLOCK,
            BR_MULTICAST_FAST_LEAVE, BR_LEARNING, BR_FLOOD,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
