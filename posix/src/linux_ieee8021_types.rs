//! `<linux/mrp_bridge.h>` — IEEE 802.1 protocol constants.
//!
//! Constants for IEEE 802.1 protocols covering MRP states,
//! STP states, and protocol identifiers.

// ---------------------------------------------------------------------------
// STP (Spanning Tree Protocol) states
// ---------------------------------------------------------------------------

/// STP: disabled.
pub const STP_STATE_DISABLED: u32 = 0;
/// STP: listening.
pub const STP_STATE_LISTENING: u32 = 1;
/// STP: learning.
pub const STP_STATE_LEARNING: u32 = 2;
/// STP: forwarding.
pub const STP_STATE_FORWARDING: u32 = 3;
/// STP: blocking.
pub const STP_STATE_BLOCKING: u32 = 4;

// ---------------------------------------------------------------------------
// MRP (Media Redundancy Protocol) ring states
// ---------------------------------------------------------------------------

/// MRP ring: open.
pub const MRP_RING_STATE_OPEN: u32 = 0;
/// MRP ring: closed.
pub const MRP_RING_STATE_CLOSED: u32 = 1;

// ---------------------------------------------------------------------------
// MRP port roles
// ---------------------------------------------------------------------------

/// MRP port: primary.
pub const MRP_PORT_ROLE_PRIMARY: u32 = 0;
/// MRP port: secondary.
pub const MRP_PORT_ROLE_SECONDARY: u32 = 1;
/// MRP port: none (not an MRP port).
pub const MRP_PORT_ROLE_NONE: u32 = 2;

// ---------------------------------------------------------------------------
// IEEE 802.1 protocol EtherTypes
// ---------------------------------------------------------------------------

/// STP/RSTP BPDU.
pub const ETH_P_STP: u16 = 0x0026;
/// 802.1X (EAP over LAN).
pub const ETH_P_PAE: u16 = 0x888E;
/// 802.1Q VLAN tag.
pub const ETH_P_8021Q: u16 = 0x8100;
/// 802.1ad QinQ (service VLAN).
pub const ETH_P_8021AD: u16 = 0x88A8;
/// LLDP (Link Layer Discovery Protocol).
pub const ETH_P_LLDP: u16 = 0x88CC;
/// MRP (Media Redundancy Protocol).
pub const ETH_P_MRP: u16 = 0x88E3;
/// CFM (Connectivity Fault Management).
pub const ETH_P_CFM: u16 = 0x8902;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stp_states_distinct() {
        let states = [
            STP_STATE_DISABLED, STP_STATE_LISTENING,
            STP_STATE_LEARNING, STP_STATE_FORWARDING,
            STP_STATE_BLOCKING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_mrp_ring_states_distinct() {
        assert_ne!(MRP_RING_STATE_OPEN, MRP_RING_STATE_CLOSED);
    }

    #[test]
    fn test_mrp_port_roles_distinct() {
        let roles = [
            MRP_PORT_ROLE_PRIMARY, MRP_PORT_ROLE_SECONDARY,
            MRP_PORT_ROLE_NONE,
        ];
        for i in 0..roles.len() {
            for j in (i + 1)..roles.len() {
                assert_ne!(roles[i], roles[j]);
            }
        }
    }

    #[test]
    fn test_ethertypes_distinct() {
        let types = [
            ETH_P_STP, ETH_P_PAE, ETH_P_8021Q,
            ETH_P_8021AD, ETH_P_LLDP, ETH_P_MRP, ETH_P_CFM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
