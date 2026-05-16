//! `<linux/if_bridge.h>` — Linux bridge (layer-2 switch) constants.
//!
//! The Linux bridge connects multiple Ethernet segments at layer 2,
//! forwarding frames based on MAC addresses. Used for VMs, containers,
//! and network virtualization. Configured via iproute2 or brctl.

// ---------------------------------------------------------------------------
// Bridge netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified bridge attribute.
pub const IFLA_BR_UNSPEC: u16 = 0;
/// Forward delay (in jiffies).
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

// ---------------------------------------------------------------------------
// Bridge port states (STP)
// ---------------------------------------------------------------------------

/// Port is disabled.
pub const BR_STATE_DISABLED: u8 = 0;
/// Port is listening (STP learning, no forwarding).
pub const BR_STATE_LISTENING: u8 = 1;
/// Port is learning (populating FDB, no forwarding).
pub const BR_STATE_LEARNING: u8 = 2;
/// Port is forwarding (normal operation).
pub const BR_STATE_FORWARDING: u8 = 3;
/// Port is blocking (STP blocked).
pub const BR_STATE_BLOCKING: u8 = 4;

// ---------------------------------------------------------------------------
// Bridge flags
// ---------------------------------------------------------------------------

/// Hairpin mode (allow frame to go back out the same port).
pub const BR_HAIRPIN_MODE: u32 = 1 << 0;
/// BPDU guard (shut down port on BPDU reception).
pub const BR_BPDU_GUARD: u32 = 1 << 1;
/// Root block (prevent port from becoming root).
pub const BR_ROOT_BLOCK: u32 = 1 << 2;
/// Multicast fast leave.
pub const BR_MULTICAST_FAST_LEAVE: u32 = 1 << 3;
/// Learning enabled.
pub const BR_LEARNING: u32 = 1 << 4;
/// Flooding enabled (unknown unicast).
pub const BR_FLOOD: u32 = 1 << 5;
/// Proxy ARP.
pub const BR_PROXYARP: u32 = 1 << 6;
/// Broadcast flood.
pub const BR_BCAST_FLOOD: u32 = 1 << 8;
/// Multicast flood.
pub const BR_MCAST_FLOOD: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// STP modes
// ---------------------------------------------------------------------------

/// No STP.
pub const BR_NO_STP: u32 = 0;
/// Kernel STP.
pub const BR_KERNEL_STP: u32 = 1;
/// User STP (rstp via userspace daemon).
pub const BR_USER_STP: u32 = 2;

// ---------------------------------------------------------------------------
// VLAN constants
// ---------------------------------------------------------------------------

/// Default bridge VLAN ID.
pub const BR_VLAN_DEFAULT_PVID: u16 = 1;
/// Maximum VLAN ID.
pub const BR_VLAN_MAX_ID: u16 = 4094;

// ---------------------------------------------------------------------------
// Link type
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND value for bridge.
pub const BRIDGE_KIND: &str = "bridge";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_BR_UNSPEC, IFLA_BR_FORWARD_DELAY, IFLA_BR_HELLO_TIME,
            IFLA_BR_MAX_AGE, IFLA_BR_AGEING_TIME, IFLA_BR_STP_STATE,
            IFLA_BR_PRIORITY, IFLA_BR_VLAN_FILTERING,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

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
    fn test_flags_powers_of_two() {
        let flags = [
            BR_HAIRPIN_MODE, BR_BPDU_GUARD, BR_ROOT_BLOCK,
            BR_MULTICAST_FAST_LEAVE, BR_LEARNING, BR_FLOOD,
            BR_PROXYARP, BR_BCAST_FLOOD, BR_MCAST_FLOOD,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            BR_HAIRPIN_MODE, BR_BPDU_GUARD, BR_ROOT_BLOCK,
            BR_MULTICAST_FAST_LEAVE, BR_LEARNING, BR_FLOOD,
            BR_PROXYARP, BR_BCAST_FLOOD, BR_MCAST_FLOOD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_stp_modes_distinct() {
        let modes = [BR_NO_STP, BR_KERNEL_STP, BR_USER_STP];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_vlan_range() {
        assert!(BR_VLAN_DEFAULT_PVID <= BR_VLAN_MAX_ID);
        assert_eq!(BR_VLAN_MAX_ID, 4094);
    }
}
