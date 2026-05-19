//! `<linux/if_vlan.h>` — Additional VLAN constants.
//!
//! Supplementary VLAN constants covering VLAN flags,
//! protocol types, and QoS mapping.

// ---------------------------------------------------------------------------
// VLAN flags
// ---------------------------------------------------------------------------

/// Reorder header.
pub const VLAN_FLAG_REORDER_HDR: u32 = 0x1;
/// GVRP (GARP VLAN Registration Protocol).
pub const VLAN_FLAG_GVRP: u32 = 0x2;
/// Loose binding (don't follow master state).
pub const VLAN_FLAG_LOOSE_BINDING: u32 = 0x4;
/// MVRP (Multiple VLAN Registration Protocol).
pub const VLAN_FLAG_MVRP: u32 = 0x8;
/// Bridge binding.
pub const VLAN_FLAG_BRIDGE_BINDING: u32 = 0x10;

// ---------------------------------------------------------------------------
// VLAN protocol identifiers (EtherType)
// ---------------------------------------------------------------------------

/// 802.1Q VLAN.
pub const VLAN_ETH_P_8021Q: u16 = 0x8100;
/// 802.1ad (QinQ / service VLAN).
pub const VLAN_ETH_P_8021AD: u16 = 0x88A8;

// ---------------------------------------------------------------------------
// VLAN header constants
// ---------------------------------------------------------------------------

/// VLAN header length (4 bytes: TPID + TCI).
pub const VLAN_HLEN: u32 = 4;
/// Maximum VLAN ID.
pub const VLAN_VID_MASK: u16 = 0x0FFF;
/// Number of valid VLAN IDs.
pub const VLAN_N_VID: u16 = 4096;
/// PCP (Priority Code Point) mask.
pub const VLAN_PCP_MASK: u16 = 0xE000;
/// PCP shift.
pub const VLAN_PCP_SHIFT: u32 = 13;
/// DEI (Drop Eligible Indicator) mask.
pub const VLAN_DEI_MASK: u16 = 0x1000;
/// DEI shift.
pub const VLAN_DEI_SHIFT: u32 = 12;

// ---------------------------------------------------------------------------
// QoS/priority mapping
// ---------------------------------------------------------------------------

/// Number of priority levels (0-7).
pub const VLAN_PRIO_MAX: u32 = 7;
/// Number of skb priorities that can be mapped.
pub const VLAN_SKB_PRIO_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct() {
        let flags = [
            VLAN_FLAG_REORDER_HDR, VLAN_FLAG_GVRP,
            VLAN_FLAG_LOOSE_BINDING, VLAN_FLAG_MVRP,
            VLAN_FLAG_BRIDGE_BINDING,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        assert_ne!(VLAN_ETH_P_8021Q, VLAN_ETH_P_8021AD);
    }

    #[test]
    fn test_vid_mask() {
        assert_eq!(VLAN_VID_MASK, 0x0FFF);
        assert_eq!(VLAN_N_VID, (VLAN_VID_MASK as u16) + 1);
    }

    #[test]
    fn test_tci_fields_no_overlap() {
        assert_eq!(
            (VLAN_VID_MASK as u32) & (VLAN_PCP_MASK as u32) & (VLAN_DEI_MASK as u32),
            0
        );
    }

    #[test]
    fn test_tci_fields_cover_16_bits() {
        let combined = VLAN_VID_MASK | VLAN_PCP_MASK | VLAN_DEI_MASK;
        assert_eq!(combined, 0xFFFF);
    }

    #[test]
    fn test_hlen() {
        assert_eq!(VLAN_HLEN, 4);
    }

    #[test]
    fn test_pcp_shift() {
        assert_eq!(VLAN_PCP_SHIFT, 13);
        assert_eq!(VLAN_DEI_SHIFT, 12);
    }
}
