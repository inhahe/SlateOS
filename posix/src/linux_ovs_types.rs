//! `<linux/openvswitch.h>` — Open vSwitch (OVS) kernel datapath constants.
//!
//! Open vSwitch uses a kernel datapath module for fast-path packet
//! forwarding and a userspace daemon (ovs-vswitchd) for flow
//! programming and control. The netlink interface communicates
//! between them: the kernel reports unmatched packets, the daemon
//! installs flow rules. OVS is the default virtual switch in
//! OpenStack, Kubernetes (via OVN), and enterprise SDN deployments.

// ---------------------------------------------------------------------------
// OVS datapath commands (OVS_DP_CMD_*)
// ---------------------------------------------------------------------------

/// Create a new datapath.
pub const OVS_DP_CMD_NEW: u32 = 1;
/// Delete a datapath.
pub const OVS_DP_CMD_DEL: u32 = 2;
/// Get datapath info.
pub const OVS_DP_CMD_GET: u32 = 3;
/// Set datapath parameters.
pub const OVS_DP_CMD_SET: u32 = 4;

// ---------------------------------------------------------------------------
// OVS vport commands (OVS_VPORT_CMD_*)
// ---------------------------------------------------------------------------

/// Create a new vport.
pub const OVS_VPORT_CMD_NEW: u32 = 1;
/// Delete a vport.
pub const OVS_VPORT_CMD_DEL: u32 = 2;
/// Get vport info.
pub const OVS_VPORT_CMD_GET: u32 = 3;
/// Set vport parameters.
pub const OVS_VPORT_CMD_SET: u32 = 4;

// ---------------------------------------------------------------------------
// OVS flow commands (OVS_FLOW_CMD_*)
// ---------------------------------------------------------------------------

/// Install a new flow.
pub const OVS_FLOW_CMD_NEW: u32 = 1;
/// Delete a flow.
pub const OVS_FLOW_CMD_DEL: u32 = 2;
/// Get flow info.
pub const OVS_FLOW_CMD_GET: u32 = 3;
/// Set flow actions.
pub const OVS_FLOW_CMD_SET: u32 = 4;

// ---------------------------------------------------------------------------
// OVS packet commands (OVS_PACKET_CMD_*)
// ---------------------------------------------------------------------------

/// Packet missed in kernel (upcall to userspace).
pub const OVS_PACKET_CMD_MISS: u32 = 1;
/// Packet action (userspace-requested).
pub const OVS_PACKET_CMD_ACTION: u32 = 2;
/// Execute packet with given actions.
pub const OVS_PACKET_CMD_EXECUTE: u32 = 3;

// ---------------------------------------------------------------------------
// OVS vport types
// ---------------------------------------------------------------------------

/// Netdev (regular network device) vport.
pub const OVS_VPORT_TYPE_NETDEV: u32 = 1;
/// Internal (OVS bridge port) vport.
pub const OVS_VPORT_TYPE_INTERNAL: u32 = 2;
/// GRE tunnel vport.
pub const OVS_VPORT_TYPE_GRE: u32 = 3;
/// VXLAN tunnel vport.
pub const OVS_VPORT_TYPE_VXLAN: u32 = 4;
/// Geneve tunnel vport.
pub const OVS_VPORT_TYPE_GENEVE: u32 = 5;

// ---------------------------------------------------------------------------
// OVS action types (OVS_ACTION_ATTR_*)
// ---------------------------------------------------------------------------

/// Output packet to a port.
pub const OVS_ACTION_ATTR_OUTPUT: u32 = 1;
/// Send packet to userspace.
pub const OVS_ACTION_ATTR_USERSPACE: u32 = 2;
/// Set packet fields.
pub const OVS_ACTION_ATTR_SET: u32 = 3;
/// Push VLAN tag.
pub const OVS_ACTION_ATTR_PUSH_VLAN: u32 = 4;
/// Pop VLAN tag.
pub const OVS_ACTION_ATTR_POP_VLAN: u32 = 5;
/// Sample packet (for sFlow/IPFIX).
pub const OVS_ACTION_ATTR_SAMPLE: u32 = 6;
/// Recirculate packet.
pub const OVS_ACTION_ATTR_RECIRC: u32 = 7;
/// Hash packet (for load balancing).
pub const OVS_ACTION_ATTR_HASH: u32 = 8;
/// Push MPLS label.
pub const OVS_ACTION_ATTR_PUSH_MPLS: u32 = 9;
/// Pop MPLS label.
pub const OVS_ACTION_ATTR_POP_MPLS: u32 = 10;
/// Set masked fields.
pub const OVS_ACTION_ATTR_SET_MASKED: u32 = 11;
/// Connection tracking action.
pub const OVS_ACTION_ATTR_CT: u32 = 12;
/// Truncate packet.
pub const OVS_ACTION_ATTR_TRUNC: u32 = 13;
/// Push Ethernet header.
pub const OVS_ACTION_ATTR_PUSH_ETH: u32 = 14;
/// Pop Ethernet header.
pub const OVS_ACTION_ATTR_POP_ETH: u32 = 15;
/// Clone action (copy and execute).
pub const OVS_ACTION_ATTR_CLONE: u32 = 16;
/// Check packet length and branch.
pub const OVS_ACTION_ATTR_CHECK_PKT_LEN: u32 = 17;
/// Push NSH header.
pub const OVS_ACTION_ATTR_PUSH_NSH: u32 = 18;
/// Pop NSH header.
pub const OVS_ACTION_ATTR_POP_NSH: u32 = 19;
/// Meter action.
pub const OVS_ACTION_ATTR_METER: u32 = 20;
/// Drop action (explicit).
pub const OVS_ACTION_ATTR_DROP: u32 = 21;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dp_commands_distinct() {
        let cmds = [
            OVS_DP_CMD_NEW,
            OVS_DP_CMD_DEL,
            OVS_DP_CMD_GET,
            OVS_DP_CMD_SET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_vport_types_distinct() {
        let types = [
            OVS_VPORT_TYPE_NETDEV,
            OVS_VPORT_TYPE_INTERNAL,
            OVS_VPORT_TYPE_GRE,
            OVS_VPORT_TYPE_VXLAN,
            OVS_VPORT_TYPE_GENEVE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_action_attrs_distinct() {
        let attrs = [
            OVS_ACTION_ATTR_OUTPUT,
            OVS_ACTION_ATTR_USERSPACE,
            OVS_ACTION_ATTR_SET,
            OVS_ACTION_ATTR_PUSH_VLAN,
            OVS_ACTION_ATTR_POP_VLAN,
            OVS_ACTION_ATTR_SAMPLE,
            OVS_ACTION_ATTR_RECIRC,
            OVS_ACTION_ATTR_HASH,
            OVS_ACTION_ATTR_PUSH_MPLS,
            OVS_ACTION_ATTR_POP_MPLS,
            OVS_ACTION_ATTR_SET_MASKED,
            OVS_ACTION_ATTR_CT,
            OVS_ACTION_ATTR_TRUNC,
            OVS_ACTION_ATTR_PUSH_ETH,
            OVS_ACTION_ATTR_POP_ETH,
            OVS_ACTION_ATTR_CLONE,
            OVS_ACTION_ATTR_CHECK_PKT_LEN,
            OVS_ACTION_ATTR_PUSH_NSH,
            OVS_ACTION_ATTR_POP_NSH,
            OVS_ACTION_ATTR_METER,
            OVS_ACTION_ATTR_DROP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_packet_commands_distinct() {
        let cmds = [
            OVS_PACKET_CMD_MISS,
            OVS_PACKET_CMD_ACTION,
            OVS_PACKET_CMD_EXECUTE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
