//! `<linux/if_vlan.h>` — 802.1Q VLAN tagging.
//!
//! A VLAN tag is a 32-bit field inserted into Ethernet frames between
//! the source MAC and the EtherType: 16-bit TPID + 16-bit TCI (PCP +
//! DEI + VID). Linux lets userspace create per-VLAN sub-interfaces
//! via netlink (`ip link add link eth0 type vlan id 100`).

// ---------------------------------------------------------------------------
// EtherType values for VLAN tags
// ---------------------------------------------------------------------------

/// 802.1Q customer VLAN (C-Tag).
pub const ETH_P_8021Q: u16 = 0x8100;
/// 802.1ad service VLAN (S-Tag, "Q-in-Q").
pub const ETH_P_8021AD: u16 = 0x88A8;

// ---------------------------------------------------------------------------
// VLAN tag field sizes / masks
// ---------------------------------------------------------------------------

/// VLAN tag overhead in bytes (TPID + TCI).
pub const VLAN_HLEN: usize = 4;

/// 12-bit VID space: 4096 IDs total.
pub const VLAN_N_VID: u16 = 4096;

/// VID 0 — packets with this ID are "priority-tagged only".
pub const VLAN_VID_PRIORITY_ONLY: u16 = 0;

/// VID 4095 — reserved by IEEE 802.1Q (must not be transmitted).
pub const VLAN_VID_RESERVED: u16 = 4095;

/// Bitmask for the 12-bit VID field inside the TCI word.
pub const VLAN_VID_MASK: u16 = 0x0FFF;
/// Drop-Eligible Indicator (bit 12 of TCI).
pub const VLAN_DEI_MASK: u16 = 0x1000;
/// 3-bit PCP (Priority Code Point) field (bits 13..15 of TCI).
pub const VLAN_PCP_MASK: u16 = 0xE000;
pub const VLAN_PCP_SHIFT: u32 = 13;

// ---------------------------------------------------------------------------
// rtnetlink link kind & VLAN-specific attributes
// ---------------------------------------------------------------------------

pub const VLAN_KIND: &str = "vlan";

pub const IFLA_VLAN_UNSPEC: u16 = 0;
pub const IFLA_VLAN_ID: u16 = 1;
pub const IFLA_VLAN_FLAGS: u16 = 2;
pub const IFLA_VLAN_EGRESS_QOS: u16 = 3;
pub const IFLA_VLAN_INGRESS_QOS: u16 = 4;
pub const IFLA_VLAN_PROTOCOL: u16 = 5;

// ---------------------------------------------------------------------------
// VLAN flags (`vlan_flags`)
// ---------------------------------------------------------------------------

pub const VLAN_FLAG_REORDER_HDR: u32 = 0x01;
pub const VLAN_FLAG_GVRP: u32 = 0x02;
pub const VLAN_FLAG_LOOSE_BINDING: u32 = 0x04;
pub const VLAN_FLAG_MVRP: u32 = 0x08;
pub const VLAN_FLAG_BRIDGE_BINDING: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethertype_values_match_ieee() {
        // 802.1Q C-Tag is 0x8100; the S-Tag for 802.1ad is 0x88A8.
        assert_eq!(ETH_P_8021Q, 0x8100);
        assert_eq!(ETH_P_8021AD, 0x88A8);
        assert_ne!(ETH_P_8021Q, ETH_P_8021AD);
    }

    #[test]
    fn test_vlan_overhead_and_vid_space() {
        // TPID(2) + TCI(2) = 4 bytes per tag.
        assert_eq!(VLAN_HLEN, 4);
        // 12-bit VID = 4096 values; 0 and 4095 are reserved.
        assert_eq!(VLAN_N_VID, 1 << 12);
        assert_eq!(VLAN_VID_PRIORITY_ONLY, 0);
        assert_eq!(VLAN_VID_RESERVED, VLAN_N_VID - 1);
    }

    #[test]
    fn test_tci_bitfields_tile_a_16_bit_word() {
        // VID + DEI + PCP must cover the full TCI word without overlap.
        assert_eq!(VLAN_VID_MASK & VLAN_DEI_MASK, 0);
        assert_eq!(VLAN_VID_MASK & VLAN_PCP_MASK, 0);
        assert_eq!(VLAN_DEI_MASK & VLAN_PCP_MASK, 0);
        assert_eq!(VLAN_VID_MASK | VLAN_DEI_MASK | VLAN_PCP_MASK, 0xFFFF);
        // PCP_MASK is exactly the top 3 bits.
        assert_eq!(VLAN_PCP_MASK >> VLAN_PCP_SHIFT, 0x7);
    }

    #[test]
    fn test_ifla_vlan_attrs_dense_0_to_5() {
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
    fn test_vlan_flags_dense_low_5_bits() {
        let f = [
            VLAN_FLAG_REORDER_HDR,
            VLAN_FLAG_GVRP,
            VLAN_FLAG_LOOSE_BINDING,
            VLAN_FLAG_MVRP,
            VLAN_FLAG_BRIDGE_BINDING,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        let mut or = 0u32;
        for v in f {
            or |= v;
        }
        assert_eq!(or, 0x1F);
    }

    #[test]
    fn test_kind_string() {
        // The rtnetlink "kind" must be lowercase exactly "vlan".
        assert_eq!(VLAN_KIND, "vlan");
    }
}
