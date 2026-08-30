//! `<linux/if_vlan.h>` — 802.1Q VLAN tag layout and ioctl ABI.
//!
//! Every bridge, every server-rack switch port, every container with
//! VLAN-tagged isolation runs through the constants here. The kernel
//! reads/writes the 4-byte VLAN tag with this field layout; `iproute2
//! ip link add link eth0 type vlan id N` ultimately hits the IFLA_VLAN
//! attributes below.

// ---------------------------------------------------------------------------
// VLAN tag (TCI) layout — 16 bits
// ---------------------------------------------------------------------------

/// Priority Code Point — top 3 bits.
pub const VLAN_PRIO_MASK: u16 = 0xE000;
/// PCP shift.
pub const VLAN_PRIO_SHIFT: u32 = 13;
/// Drop Eligible Indicator — bit 12.
pub const VLAN_CFI_MASK: u16 = 0x1000;
/// VLAN ID — bottom 12 bits.
pub const VLAN_VID_MASK: u16 = 0x0FFF;

// ---------------------------------------------------------------------------
// Special VIDs
// ---------------------------------------------------------------------------

/// Untagged-priority indicator.
pub const VLAN_N_VID: u32 = 4096;
/// Reserved "no VLAN" sentinel.
pub const VLAN_VID_NONE: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// EtherTypes for VLAN-tagged frames
// ---------------------------------------------------------------------------

/// 802.1Q (C-VLAN).
pub const ETH_P_8021Q: u16 = 0x8100;
/// 802.1ad (S-VLAN, QinQ).
pub const ETH_P_8021AD: u16 = 0x88A8;

// ---------------------------------------------------------------------------
// VLAN header size
// ---------------------------------------------------------------------------

/// Octets per VLAN tag (TPID + TCI).
pub const VLAN_HLEN: u32 = 4;
/// Ethernet header + VLAN tag.
pub const VLAN_ETH_HLEN: u32 = 18;

// ---------------------------------------------------------------------------
// IFLA_VLAN_* attributes (rtnetlink)
// ---------------------------------------------------------------------------

pub const IFLA_VLAN_UNSPEC: u32 = 0;
pub const IFLA_VLAN_ID: u32 = 1;
pub const IFLA_VLAN_FLAGS: u32 = 2;
pub const IFLA_VLAN_EGRESS_QOS: u32 = 3;
pub const IFLA_VLAN_INGRESS_QOS: u32 = 4;
pub const IFLA_VLAN_PROTOCOL: u32 = 5;

// ---------------------------------------------------------------------------
// Flag bits (struct ifla_vlan_flags.flags)
// ---------------------------------------------------------------------------

pub const VLAN_FLAG_REORDER_HDR: u32 = 0x1;
pub const VLAN_FLAG_GVRP: u32 = 0x2;
pub const VLAN_FLAG_LOOSE_BINDING: u32 = 0x4;
pub const VLAN_FLAG_MVRP: u32 = 0x8;
pub const VLAN_FLAG_BRIDGE_BINDING: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tci_field_layout_disjoint() {
        // Three sub-fields cover all 16 bits and don't overlap.
        assert_eq!(VLAN_PRIO_MASK | VLAN_CFI_MASK | VLAN_VID_MASK, 0xFFFF);
        assert_eq!(VLAN_PRIO_MASK & VLAN_CFI_MASK, 0);
        assert_eq!(VLAN_PRIO_MASK & VLAN_VID_MASK, 0);
        assert_eq!(VLAN_CFI_MASK & VLAN_VID_MASK, 0);
        // PRIO mask = 7 << 13.
        assert_eq!(VLAN_PRIO_MASK, 7 << VLAN_PRIO_SHIFT);
    }

    #[test]
    fn test_vid_count_matches_mask() {
        // 4096 distinct VIDs = 2^12 = VID_MASK + 1.
        assert_eq!(VLAN_N_VID, u32::from(VLAN_VID_MASK) + 1);
    }

    #[test]
    fn test_ethertypes_and_header_sizes() {
        assert_eq!(ETH_P_8021Q, 0x8100);
        assert_eq!(ETH_P_8021AD, 0x88A8);
        // EtherType (2 bytes) + TCI (2 bytes).
        assert_eq!(VLAN_HLEN, 4);
        // 6+6+(8100/TCI = 4)+(2 inner type) = 18.
        assert_eq!(VLAN_ETH_HLEN, 18);
    }

    #[test]
    fn test_ifla_vlan_attrs_dense() {
        let a = [
            IFLA_VLAN_UNSPEC,
            IFLA_VLAN_ID,
            IFLA_VLAN_FLAGS,
            IFLA_VLAN_EGRESS_QOS,
            IFLA_VLAN_INGRESS_QOS,
            IFLA_VLAN_PROTOCOL,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_flag_bits_pow2() {
        for &b in &[
            VLAN_FLAG_REORDER_HDR,
            VLAN_FLAG_GVRP,
            VLAN_FLAG_LOOSE_BINDING,
            VLAN_FLAG_MVRP,
            VLAN_FLAG_BRIDGE_BINDING,
        ] {
            assert!(b.is_power_of_two());
        }
    }
}
