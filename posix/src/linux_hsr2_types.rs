//! `<linux/if_hsr.h>` — Additional HSR/PRP (High-availability Seamless Redundancy) constants.
//!
//! Supplementary HSR constants covering netlink attributes,
//! protocol versions, and supervision frame types.

// ---------------------------------------------------------------------------
// HSR netlink commands
// ---------------------------------------------------------------------------

/// Create HSR device.
pub const HSR_C_RING_ERROR: u32 = 1;
/// Get node status.
pub const HSR_C_NODE_DOWN: u32 = 2;
/// Unspec command.
pub const HSR_C_UNSPEC: u32 = 0;
/// Get node list.
pub const HSR_C_GET_NODE_LIST: u32 = 3;
/// Set node list.
pub const HSR_C_SET_NODE_LIST: u32 = 4;
/// Get node status.
pub const HSR_C_GET_NODE_STATUS: u32 = 5;
/// Set node status.
pub const HSR_C_SET_NODE_STATUS: u32 = 6;

// ---------------------------------------------------------------------------
// HSR netlink attributes
// ---------------------------------------------------------------------------

/// Unspec attribute.
pub const HSR_A_UNSPEC: u32 = 0;
/// Network interface index.
pub const HSR_A_NODE_ADDR: u32 = 1;
/// Interface index (slave1).
pub const HSR_A_IFINDEX: u32 = 2;
/// MAC address of slave1 interface.
pub const HSR_A_IF1_AGE: u32 = 3;
/// MAC address of slave2 interface.
pub const HSR_A_IF2_AGE: u32 = 4;
/// Node address B.
pub const HSR_A_NODE_ADDR_B: u32 = 5;
/// Interface 1 sequence number.
pub const HSR_A_IF1_SEQ: u32 = 6;
/// Interface 2 sequence number.
pub const HSR_A_IF2_SEQ: u32 = 7;

// ---------------------------------------------------------------------------
// HSR protocol versions
// ---------------------------------------------------------------------------

/// HSR version 0 (IEC 62439-3:2010).
pub const HSR_VERSION_0: u8 = 0;
/// HSR version 1 (IEC 62439-3:2012).
pub const HSR_VERSION_1: u8 = 1;
/// PRP (Parallel Redundancy Protocol).
pub const PRP_VERSION: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            HSR_C_UNSPEC,
            HSR_C_RING_ERROR,
            HSR_C_NODE_DOWN,
            HSR_C_GET_NODE_LIST,
            HSR_C_SET_NODE_LIST,
            HSR_C_GET_NODE_STATUS,
            HSR_C_SET_NODE_STATUS,
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
            HSR_A_UNSPEC,
            HSR_A_NODE_ADDR,
            HSR_A_IFINDEX,
            HSR_A_IF1_AGE,
            HSR_A_IF2_AGE,
            HSR_A_NODE_ADDR_B,
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
        let versions = [HSR_VERSION_0, HSR_VERSION_1, PRP_VERSION];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }
}
