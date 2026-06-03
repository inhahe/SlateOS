//! `<linux/if_vlan.h>` — VLAN (802.1Q) constants.
//!
//! VLANs partition a physical network into logical segments at
//! layer 2. Each VLAN has a 12-bit ID (1-4094). The Linux kernel
//! supports VLAN sub-interfaces (e.g., eth0.100) and VLAN-aware
//! bridges for network virtualization.

// ---------------------------------------------------------------------------
// VLAN ID range
// ---------------------------------------------------------------------------

/// Minimum valid VLAN ID.
pub const VLAN_ID_MIN: u16 = 1;
/// Maximum valid VLAN ID.
pub const VLAN_ID_MAX: u16 = 4094;
/// Total number of VLAN IDs (12 bits, 0 and 4095 reserved).
pub const VLAN_N_VID: u16 = 4096;
/// VLAN ID mask (12 bits).
pub const VLAN_VID_MASK: u16 = 0x0FFF;

// ---------------------------------------------------------------------------
// VLAN tag fields
// ---------------------------------------------------------------------------

/// Priority Code Point mask (3 bits, bits 13-15).
pub const VLAN_PCP_MASK: u16 = 0xE000;
/// PCP shift (bit position).
pub const VLAN_PCP_SHIFT: u32 = 13;
/// Drop Eligible Indicator bit.
pub const VLAN_DEI_BIT: u16 = 0x1000;

// ---------------------------------------------------------------------------
// EtherType values
// ---------------------------------------------------------------------------

/// 802.1Q VLAN tag EtherType.
pub const ETH_P_8021Q: u16 = 0x8100;
/// 802.1ad (QinQ / S-VLAN) EtherType.
pub const ETH_P_8021AD: u16 = 0x88A8;

// ---------------------------------------------------------------------------
// VLAN header size
// ---------------------------------------------------------------------------

/// Size of a VLAN tag in bytes (TPID + TCI).
pub const VLAN_HLEN: usize = 4;
/// Ethernet header with VLAN tag.
pub const VLAN_ETH_HLEN: usize = 18;

// ---------------------------------------------------------------------------
// VLAN flags
// ---------------------------------------------------------------------------

/// Reorder header (strip VLAN tag on receive, add on transmit).
pub const VLAN_FLAG_REORDER_HDR: u32 = 1 << 0;
/// GVRP (GARP VLAN Registration Protocol) enabled.
pub const VLAN_FLAG_GVRP: u32 = 1 << 1;
/// Loose binding (don't follow master device state).
pub const VLAN_FLAG_LOOSE_BINDING: u32 = 1 << 2;
/// MVRP (Multiple VLAN Registration Protocol) enabled.
pub const VLAN_FLAG_MVRP: u32 = 1 << 3;
/// Bridge binding (used as bridge port).
pub const VLAN_FLAG_BRIDGE_BINDING: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Name type (how VLAN sub-interface is named)
// ---------------------------------------------------------------------------

/// Plus VID format (e.g., "eth0.100").
pub const VLAN_NAME_TYPE_PLUS_VID: u32 = 0;
/// Raw + VID (e.g., "eth0.0100").
pub const VLAN_NAME_TYPE_RAW_PLUS_VID: u32 = 1;
/// Plus VID without padding.
pub const VLAN_NAME_TYPE_PLUS_VID_NO_PAD: u32 = 2;
/// Raw + VID without padding.
pub const VLAN_NAME_TYPE_RAW_PLUS_VID_NO_PAD: u32 = 3;

// ---------------------------------------------------------------------------
// Link type
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND for VLAN device.
pub const VLAN_KIND: &str = "vlan";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vlan_id_range() {
        assert_eq!(VLAN_ID_MIN, 1);
        assert_eq!(VLAN_ID_MAX, 4094);
        assert!(VLAN_ID_MIN <= VLAN_ID_MAX);
    }

    #[test]
    fn test_vid_mask() {
        assert_eq!(VLAN_VID_MASK, 0x0FFF);
        assert_eq!(VLAN_VID_MASK as u16, VLAN_N_VID - 1);
    }

    #[test]
    fn test_tag_fields_no_overlap() {
        assert_eq!(VLAN_VID_MASK & VLAN_PCP_MASK, 0);
        assert_eq!(VLAN_VID_MASK & VLAN_DEI_BIT, 0);
        assert_eq!(VLAN_PCP_MASK & VLAN_DEI_BIT, 0);
    }

    #[test]
    fn test_ethertypes_distinct() {
        assert_ne!(ETH_P_8021Q, ETH_P_8021AD);
    }

    #[test]
    fn test_header_sizes() {
        assert_eq!(VLAN_HLEN, 4);
        assert_eq!(VLAN_ETH_HLEN, 18); // 14 (eth) + 4 (vlan)
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            VLAN_FLAG_REORDER_HDR,
            VLAN_FLAG_GVRP,
            VLAN_FLAG_LOOSE_BINDING,
            VLAN_FLAG_MVRP,
            VLAN_FLAG_BRIDGE_BINDING,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
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
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_name_types_distinct() {
        let types = [
            VLAN_NAME_TYPE_PLUS_VID,
            VLAN_NAME_TYPE_RAW_PLUS_VID,
            VLAN_NAME_TYPE_PLUS_VID_NO_PAD,
            VLAN_NAME_TYPE_RAW_PLUS_VID_NO_PAD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
