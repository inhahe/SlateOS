//! `<linux/if_hsr.h>` — HSR/PRP (High-availability Seamless Redundancy) constants.
//!
//! HSR (IEC 62439-3) provides zero-switchover-time redundancy for
//! Ethernet networks by sending each frame over two independent paths.
//! PRP (Parallel Redundancy Protocol) is the related standard that
//! works with standard (non-HSR) switches. Both are used in
//! industrial automation (IEC 61850 substations), rail signaling,
//! and other applications requiring fault-tolerant networking with
//! no packet loss during link failures.

// ---------------------------------------------------------------------------
// HSR netlink commands
// ---------------------------------------------------------------------------

/// Get HSR node table.
pub const HSR_CMD_GET_NODE_STATUS: u32 = 1;
/// Get HSR node list.
pub const HSR_CMD_GET_NODE_LIST: u32 = 2;
/// Set HSR parameters.
pub const HSR_CMD_SET_NODE_STATUS: u32 = 3;
/// Get HSR info.
pub const HSR_CMD_GET_NODE_INFO: u32 = 4;

// ---------------------------------------------------------------------------
// HSR netlink attributes (HSR_A_*)
// ---------------------------------------------------------------------------

/// Node MAC address attribute.
pub const HSR_A_NODE_ADDR: u32 = 1;
/// Interface index (slave A).
pub const HSR_A_IFINDEX: u32 = 2;
/// Node address B (for dual-homed nodes).
pub const HSR_A_NODE_ADDR_B: u32 = 3;
/// Interface 1 (slave A) index.
pub const HSR_A_IF1_IFINDEX: u32 = 4;
/// Interface 2 (slave B) index.
pub const HSR_A_IF2_IFINDEX: u32 = 5;
/// Node age (time since last frame from this node).
pub const HSR_A_IF1_AGE: u32 = 6;
/// Interface 2 age.
pub const HSR_A_IF2_AGE: u32 = 7;
/// Interface 1 sequence number.
pub const HSR_A_IF1_SEQ: u32 = 8;
/// Interface 2 sequence number.
pub const HSR_A_IF2_SEQ: u32 = 9;

// ---------------------------------------------------------------------------
// HSR versions
// ---------------------------------------------------------------------------

/// HSR v0 (IEC 62439-3:2010).
pub const HSR_VERSION_0: u32 = 0;
/// HSR v1 (IEC 62439-3:2012).
pub const HSR_VERSION_1: u32 = 1;

// ---------------------------------------------------------------------------
// HSR/PRP protocol values
// ---------------------------------------------------------------------------

/// PRP protocol (Parallel Redundancy Protocol).
pub const HSR_PROTOCOL_PRP: u32 = 0;
/// HSR protocol.
pub const HSR_PROTOCOL_HSR: u32 = 1;

// ---------------------------------------------------------------------------
// HSR supervision frame types
// ---------------------------------------------------------------------------

/// Supervision frame — HSR node.
pub const HSR_TLV_ANNOUNCE: u32 = 22;
/// Supervision frame — life check.
pub const HSR_TLV_LIFE_CHECK: u32 = 23;
/// End of TLV list.
pub const HSR_TLV_EOT: u32 = 0;

// ---------------------------------------------------------------------------
// HSR IFLA attributes (netlink interface config)
// ---------------------------------------------------------------------------

/// HSR slave1 interface.
pub const IFLA_HSR_SLAVE1: u32 = 1;
/// HSR slave2 interface.
pub const IFLA_HSR_SLAVE2: u32 = 2;
/// HSR multicast spec.
pub const IFLA_HSR_MULTICAST_SPEC: u32 = 3;
/// HSR supervision MAC address.
pub const IFLA_HSR_SUPERVISION_ADDR: u32 = 4;
/// HSR sequence number.
pub const IFLA_HSR_SEQ_NR: u32 = 5;
/// HSR version.
pub const IFLA_HSR_VERSION: u32 = 6;
/// HSR protocol (HSR vs PRP).
pub const IFLA_HSR_PROTOCOL: u32 = 7;
/// HSR interlink port.
pub const IFLA_HSR_INTERLINK: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            HSR_CMD_GET_NODE_STATUS,
            HSR_CMD_GET_NODE_LIST,
            HSR_CMD_SET_NODE_STATUS,
            HSR_CMD_GET_NODE_INFO,
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
            HSR_A_NODE_ADDR,
            HSR_A_IFINDEX,
            HSR_A_NODE_ADDR_B,
            HSR_A_IF1_IFINDEX,
            HSR_A_IF2_IFINDEX,
            HSR_A_IF1_AGE,
            HSR_A_IF2_AGE,
            HSR_A_IF1_SEQ,
            HSR_A_IF2_SEQ,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_versions_distinct() {
        assert_ne!(HSR_VERSION_0, HSR_VERSION_1);
    }

    #[test]
    fn test_protocols_distinct() {
        assert_ne!(HSR_PROTOCOL_PRP, HSR_PROTOCOL_HSR);
    }

    #[test]
    fn test_tlv_types_distinct() {
        let tlvs = [HSR_TLV_ANNOUNCE, HSR_TLV_LIFE_CHECK, HSR_TLV_EOT];
        for i in 0..tlvs.len() {
            for j in (i + 1)..tlvs.len() {
                assert_ne!(tlvs[i], tlvs[j]);
            }
        }
    }

    #[test]
    fn test_ifla_attrs_distinct() {
        let attrs = [
            IFLA_HSR_SLAVE1,
            IFLA_HSR_SLAVE2,
            IFLA_HSR_MULTICAST_SPEC,
            IFLA_HSR_SUPERVISION_ADDR,
            IFLA_HSR_SEQ_NR,
            IFLA_HSR_VERSION,
            IFLA_HSR_PROTOCOL,
            IFLA_HSR_INTERLINK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
