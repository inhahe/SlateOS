//! `<linux/if_vlan.h>` — VLAN (802.1Q) constants.
//!
//! VLANs partition a physical network into isolated broadcast domains.
//! 802.1Q adds a 4-byte tag to Ethernet frames containing a 12-bit
//! VLAN ID (1-4094) and 3-bit priority. Linux supports VLAN interfaces
//! (one per VID per parent device) and bridge VLAN filtering.

// ---------------------------------------------------------------------------
// VLAN limits
// ---------------------------------------------------------------------------

/// Minimum VLAN ID.
pub const VLAN_VID_MIN: u16 = 1;
/// Maximum VLAN ID.
pub const VLAN_VID_MAX: u16 = 4094;
/// VLAN ID mask (12 bits).
pub const VLAN_VID_MASK: u16 = 0x0FFF;
/// Priority mask (3 bits, shifted).
pub const VLAN_PRIO_MASK: u16 = 0xE000;
/// Priority shift.
pub const VLAN_PRIO_SHIFT: u16 = 13;
/// CFI/DEI bit.
pub const VLAN_CFI_MASK: u16 = 0x1000;

// ---------------------------------------------------------------------------
// VLAN ETH types
// ---------------------------------------------------------------------------

/// 802.1Q VLAN ethertype.
pub const ETH_P_8021Q: u16 = 0x8100;
/// 802.1ad (QinQ) VLAN ethertype.
pub const ETH_P_8021AD: u16 = 0x88A8;

// ---------------------------------------------------------------------------
// VLAN ioctl commands
// ---------------------------------------------------------------------------

/// Add VLAN interface.
pub const ADD_VLAN_CMD: u32 = 0;
/// Delete VLAN interface.
pub const DEL_VLAN_CMD: u32 = 1;
/// Set VLAN name type.
pub const SET_VLAN_NAME_TYPE_CMD: u32 = 2;
/// Set VLAN flags.
pub const SET_VLAN_FLAG_CMD: u32 = 3;

// ---------------------------------------------------------------------------
// VLAN flags
// ---------------------------------------------------------------------------

/// Enable GVRP (VLAN registration protocol).
pub const VLAN_FLAG_GVRP: u32 = 0x02;
/// Reorder header (remove VLAN tag on receive).
pub const VLAN_FLAG_REORDER_HDR: u32 = 0x01;
/// Loose binding (don't follow parent state changes).
pub const VLAN_FLAG_LOOSE_BINDING: u32 = 0x04;
/// Enable MVRP (newer VLAN registration protocol).
pub const VLAN_FLAG_MVRP: u32 = 0x08;
/// Bridge VLAN binding.
pub const VLAN_FLAG_BRIDGE_BINDING: u32 = 0x10;

// ---------------------------------------------------------------------------
// VLAN header size
// ---------------------------------------------------------------------------

/// Size of VLAN tag in bytes.
pub const VLAN_HLEN: u32 = 4;
/// Ethernet + VLAN header size.
pub const VLAN_ETH_HLEN: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vid_range() {
        assert!(VLAN_VID_MIN < VLAN_VID_MAX);
        assert_eq!(VLAN_VID_MIN, 1);
        assert_eq!(VLAN_VID_MAX, 4094);
    }

    #[test]
    fn test_masks_no_overlap() {
        assert_eq!(VLAN_VID_MASK & VLAN_PRIO_MASK, 0);
        assert_eq!(VLAN_VID_MASK & VLAN_CFI_MASK, 0);
        assert_eq!(VLAN_CFI_MASK & VLAN_PRIO_MASK, 0);
    }

    #[test]
    fn test_ethertypes_distinct() {
        assert_ne!(ETH_P_8021Q, ETH_P_8021AD);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            ADD_VLAN_CMD,
            DEL_VLAN_CMD,
            SET_VLAN_NAME_TYPE_CMD,
            SET_VLAN_FLAG_CMD,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            VLAN_FLAG_REORDER_HDR,
            VLAN_FLAG_GVRP,
            VLAN_FLAG_LOOSE_BINDING,
            VLAN_FLAG_MVRP,
            VLAN_FLAG_BRIDGE_BINDING,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
