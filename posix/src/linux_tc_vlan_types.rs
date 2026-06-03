//! `<linux/tc_act/tc_vlan.h>` — TC VLAN action constants.
//!
//! Traffic control VLAN action constants covering attribute types
//! and action commands for VLAN tag manipulation.

// ---------------------------------------------------------------------------
// TC VLAN action commands
// ---------------------------------------------------------------------------

/// Push VLAN tag.
pub const TCA_VLAN_ACT_PUSH: u32 = 1;
/// Pop VLAN tag.
pub const TCA_VLAN_ACT_POP: u32 = 2;
/// Modify VLAN tag.
pub const TCA_VLAN_ACT_MODIFY: u32 = 3;
/// Pop Ethernet header.
pub const TCA_VLAN_ACT_POP_ETH: u32 = 4;
/// Push Ethernet header.
pub const TCA_VLAN_ACT_PUSH_ETH: u32 = 5;

// ---------------------------------------------------------------------------
// TC VLAN attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_VLAN_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_VLAN_TM: u32 = 1;
/// Parameters.
pub const TCA_VLAN_PARMS: u32 = 2;
/// Push VLAN ID.
pub const TCA_VLAN_PUSH_VLAN_ID: u32 = 3;
/// Push VLAN protocol.
pub const TCA_VLAN_PUSH_VLAN_PROTOCOL: u32 = 4;
/// Push VLAN priority.
pub const TCA_VLAN_PUSH_VLAN_PRIORITY: u32 = 5;
/// Push Ethernet destination.
pub const TCA_VLAN_PUSH_ETH_DST: u32 = 6;
/// Push Ethernet source.
pub const TCA_VLAN_PUSH_ETH_SRC: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_cmds_distinct() {
        let cmds = [
            TCA_VLAN_ACT_PUSH,
            TCA_VLAN_ACT_POP,
            TCA_VLAN_ACT_MODIFY,
            TCA_VLAN_ACT_POP_ETH,
            TCA_VLAN_ACT_PUSH_ETH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_VLAN_UNSPEC,
            TCA_VLAN_TM,
            TCA_VLAN_PARMS,
            TCA_VLAN_PUSH_VLAN_ID,
            TCA_VLAN_PUSH_VLAN_PROTOCOL,
            TCA_VLAN_PUSH_VLAN_PRIORITY,
            TCA_VLAN_PUSH_ETH_DST,
            TCA_VLAN_PUSH_ETH_SRC,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
