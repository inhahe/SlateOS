//! `<linux/tc_act/tc_vlan.h>` — TC VLAN action constants.
//!
//! The vlan action pushes, pops, or modifies 802.1Q VLAN tags on
//! packets. It is commonly used for VLAN tagging at ingress/egress
//! of switch ports and for translating between VLANs.

// ---------------------------------------------------------------------------
// VLAN action types
// ---------------------------------------------------------------------------

/// Pop (remove) outer VLAN tag.
pub const TCA_VLAN_ACT_POP: u8 = 1;
/// Push (add) a VLAN tag.
pub const TCA_VLAN_ACT_PUSH: u8 = 2;
/// Modify existing VLAN tag fields.
pub const TCA_VLAN_ACT_MODIFY: u8 = 3;
/// Pop Ethernet header (for Q-in-Q).
pub const TCA_VLAN_ACT_POP_ETH: u8 = 4;
/// Push Ethernet header (for Q-in-Q).
pub const TCA_VLAN_ACT_PUSH_ETH: u8 = 5;

// ---------------------------------------------------------------------------
// VLAN netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_VLAN_UNSPEC: u16 = 0;
/// Timer info.
pub const TCA_VLAN_TM: u16 = 1;
/// Parameters.
pub const TCA_VLAN_PARMS: u16 = 2;
/// Push VLAN ID.
pub const TCA_VLAN_PUSH_VLAN_ID: u16 = 3;
/// Push VLAN protocol (0x8100 or 0x88A8).
pub const TCA_VLAN_PUSH_VLAN_PROTOCOL: u16 = 4;
/// Padding.
pub const TCA_VLAN_PAD: u16 = 5;
/// Push VLAN priority (PCP).
pub const TCA_VLAN_PUSH_VLAN_PRIORITY: u16 = 6;
/// Push destination MAC (for push_eth).
pub const TCA_VLAN_PUSH_ETH_DST: u16 = 7;
/// Push source MAC (for push_eth).
pub const TCA_VLAN_PUSH_ETH_SRC: u16 = 8;

// ---------------------------------------------------------------------------
// VLAN protocols (EtherTypes)
// ---------------------------------------------------------------------------

/// 802.1Q VLAN tag protocol.
pub const VLAN_ETH_TYPE_8021Q: u16 = 0x8100;
/// 802.1ad (Q-in-Q / provider bridging) tag protocol.
pub const VLAN_ETH_TYPE_8021AD: u16 = 0x88A8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            TCA_VLAN_ACT_POP, TCA_VLAN_ACT_PUSH, TCA_VLAN_ACT_MODIFY,
            TCA_VLAN_ACT_POP_ETH, TCA_VLAN_ACT_PUSH_ETH,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_VLAN_UNSPEC, TCA_VLAN_TM, TCA_VLAN_PARMS,
            TCA_VLAN_PUSH_VLAN_ID, TCA_VLAN_PUSH_VLAN_PROTOCOL,
            TCA_VLAN_PAD, TCA_VLAN_PUSH_VLAN_PRIORITY,
            TCA_VLAN_PUSH_ETH_DST, TCA_VLAN_PUSH_ETH_SRC,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_vlan_protocols_distinct() {
        assert_ne!(VLAN_ETH_TYPE_8021Q, VLAN_ETH_TYPE_8021AD);
    }
}
