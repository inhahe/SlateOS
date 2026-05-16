//! `<linux/if_vlan.h>` — 802.1Q VLAN interface constants.
//!
//! VLAN (Virtual LAN) support for creating tagged network interfaces
//! on top of physical Ethernet devices. Used by the `vconfig` and
//! `ip link add type vlan` commands.

// ---------------------------------------------------------------------------
// VLAN ioctl commands
// ---------------------------------------------------------------------------

/// Add a VLAN device.
pub const ADD_VLAN_CMD: i32 = 0;
/// Delete a VLAN device.
pub const DEL_VLAN_CMD: i32 = 1;
/// Set VLAN ingress priority map.
pub const SET_VLAN_INGRESS_PRIORITY_CMD: i32 = 2;
/// Set VLAN egress priority map.
pub const SET_VLAN_EGRESS_PRIORITY_CMD: i32 = 3;
/// Set VLAN flag.
pub const SET_VLAN_FLAG_CMD: i32 = 4;
/// Set VLAN name type.
pub const SET_VLAN_NAME_TYPE_CMD: i32 = 5;

// ---------------------------------------------------------------------------
// VLAN name types
// ---------------------------------------------------------------------------

/// Names like "vlan0005".
pub const VLAN_NAME_TYPE_PLUS_VID: u32 = 0;
/// Names like "eth0.5".
pub const VLAN_NAME_TYPE_RAW_PLUS_VID: u32 = 1;
/// Names like "vlan5" (no zero-padding).
pub const VLAN_NAME_TYPE_PLUS_VID_NO_PAD: u32 = 2;
/// Names like "eth0.0005".
pub const VLAN_NAME_TYPE_RAW_PLUS_VID_NO_PAD: u32 = 3;

// ---------------------------------------------------------------------------
// VLAN flags
// ---------------------------------------------------------------------------

/// VLAN reorder header flag.
pub const VLAN_FLAG_REORDER_HDR: u32 = 0x1;
/// GVRP (GARP VLAN Registration Protocol) enabled.
pub const VLAN_FLAG_GVRP: u32 = 0x2;
/// Loose binding (don't follow parent state).
pub const VLAN_FLAG_LOOSE_BINDING: u32 = 0x4;
/// MVRP (Multiple VLAN Registration Protocol) enabled.
pub const VLAN_FLAG_MVRP: u32 = 0x8;
/// Bridge binding.
pub const VLAN_FLAG_BRIDGE_BINDING: u32 = 0x10;

// ---------------------------------------------------------------------------
// VLAN protocol constants
// ---------------------------------------------------------------------------

/// 802.1Q VLAN ethertype.
pub const ETH_P_8021Q: u16 = 0x8100;
/// 802.1ad VLAN ethertype (QinQ).
pub const ETH_P_8021AD: u16 = 0x88A8;

/// Maximum VLAN ID.
pub const VLAN_VID_MASK: u16 = 0x0FFF;
/// Maximum number of VLANs (4096).
pub const VLAN_N_VID: u16 = 4096;
/// VLAN header length (4 bytes).
pub const VLAN_HLEN: usize = 4;
/// Ethernet + VLAN header total.
pub const VLAN_ETH_HLEN: usize = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_sequential() {
        assert_eq!(ADD_VLAN_CMD, 0);
        assert_eq!(DEL_VLAN_CMD, 1);
        assert_eq!(SET_VLAN_INGRESS_PRIORITY_CMD, 2);
        assert_eq!(SET_VLAN_EGRESS_PRIORITY_CMD, 3);
        assert_eq!(SET_VLAN_FLAG_CMD, 4);
        assert_eq!(SET_VLAN_NAME_TYPE_CMD, 5);
    }

    #[test]
    fn test_name_types_distinct() {
        let types = [
            VLAN_NAME_TYPE_PLUS_VID, VLAN_NAME_TYPE_RAW_PLUS_VID,
            VLAN_NAME_TYPE_PLUS_VID_NO_PAD, VLAN_NAME_TYPE_RAW_PLUS_VID_NO_PAD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            VLAN_FLAG_REORDER_HDR, VLAN_FLAG_GVRP,
            VLAN_FLAG_LOOSE_BINDING, VLAN_FLAG_MVRP,
            VLAN_FLAG_BRIDGE_BINDING,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not power of 2");
        }
    }

    #[test]
    fn test_vlan_limits() {
        assert_eq!(VLAN_VID_MASK, 0x0FFF);
        assert_eq!(VLAN_N_VID, 4096);
        assert_eq!(VLAN_HLEN, 4);
    }

    #[test]
    fn test_ethertypes() {
        assert_eq!(ETH_P_8021Q, 0x8100);
        assert_eq!(ETH_P_8021AD, 0x88A8);
    }
}
