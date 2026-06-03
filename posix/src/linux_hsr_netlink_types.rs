//! `<linux/hsr_netlink.h>` — HSR/PRP redundancy-protocol netlink constants.
//!
//! Netlink attribute and notification constants for the HSR (High-
//! availability Seamless Redundancy) and PRP (Parallel Redundancy
//! Protocol) IEEE 62439-3 stack — used by configuration daemons to
//! manage redundant Ethernet rings.

// ---------------------------------------------------------------------------
// Generic-netlink attributes for HSR multicast group
// ---------------------------------------------------------------------------

/// Unspecified.
pub const HSR_A_UNSPEC: u32 = 0;
/// Node MAC address (binary 6 bytes).
pub const HSR_A_NODE_ADDR: u32 = 1;
/// HSR interface index.
pub const HSR_A_IFINDEX: u32 = 2;
/// Last-seen time on port A (jiffies).
pub const HSR_A_IF1_AGE: u32 = 3;
/// Last-seen time on port B.
pub const HSR_A_IF2_AGE: u32 = 4;
/// Node MAC address B (duplicate-detection helper).
pub const HSR_A_NODE_ADDR_B: u32 = 5;
/// Last-seen-from-A sequence number.
pub const HSR_A_IF1_SEQ: u32 = 6;
/// Last-seen-from-B sequence number.
pub const HSR_A_IF2_SEQ: u32 = 7;
/// Port-A interface index.
pub const HSR_A_IF1_IFINDEX: u32 = 8;
/// Port-B interface index.
pub const HSR_A_IF2_IFINDEX: u32 = 9;
/// Address (peer) field.
pub const HSR_A_ADDR_B_IFINDEX: u32 = 10;

// ---------------------------------------------------------------------------
// Generic-netlink command codes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const HSR_C_UNSPEC: u32 = 0;
/// Ring error — duplicate frame seen.
pub const HSR_C_RING_ERROR: u32 = 1;
/// Node down detected.
pub const HSR_C_NODE_DOWN: u32 = 2;
/// Get node status (request).
pub const HSR_C_GET_NODE_STATUS: u32 = 3;
/// Set node status (response).
pub const HSR_C_SET_NODE_STATUS: u32 = 4;
/// Get the active node list.
pub const HSR_C_GET_NODE_LIST: u32 = 5;
/// Set node list.
pub const HSR_C_SET_NODE_LIST: u32 = 6;

// ---------------------------------------------------------------------------
// Multicast group name (used by the generic-netlink family)
// ---------------------------------------------------------------------------

/// HSR generic-netlink family name.
pub const HSR_GENL_NAME: &str = "HSR";
/// HSR multicast group name.
pub const HSR_GENL_MCAST_GROUP_NAME: &str = "hsr-network";

// ---------------------------------------------------------------------------
// Protocol mode enumeration (used by HSR_LINK setlink attribute)
// ---------------------------------------------------------------------------

/// Modes: HSR v0.
pub const HSR_PROTOCOL_HSR: u32 = 0;
/// PRP.
pub const HSR_PROTOCOL_PRP: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            HSR_A_UNSPEC,
            HSR_A_NODE_ADDR,
            HSR_A_IFINDEX,
            HSR_A_IF1_AGE,
            HSR_A_IF2_AGE,
            HSR_A_NODE_ADDR_B,
            HSR_A_IF1_SEQ,
            HSR_A_IF2_SEQ,
            HSR_A_IF1_IFINDEX,
            HSR_A_IF2_IFINDEX,
            HSR_A_ADDR_B_IFINDEX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            HSR_C_UNSPEC,
            HSR_C_RING_ERROR,
            HSR_C_NODE_DOWN,
            HSR_C_GET_NODE_STATUS,
            HSR_C_SET_NODE_STATUS,
            HSR_C_GET_NODE_LIST,
            HSR_C_SET_NODE_LIST,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_names_nonempty_ascii() {
        assert!(!HSR_GENL_NAME.is_empty());
        assert!(HSR_GENL_NAME.is_ascii());
        assert!(!HSR_GENL_MCAST_GROUP_NAME.is_empty());
        assert!(HSR_GENL_MCAST_GROUP_NAME.is_ascii());
    }

    #[test]
    fn test_protocols_distinct() {
        assert_ne!(HSR_PROTOCOL_HSR, HSR_PROTOCOL_PRP);
    }
}
