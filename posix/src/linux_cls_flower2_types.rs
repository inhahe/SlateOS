//! `<linux/pkt_cls.h>` — Additional flower classifier constants.
//!
//! Supplementary traffic control flower classifier constants covering
//! attribute types and match key definitions.

// ---------------------------------------------------------------------------
// Flower classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_FLOWER_UNSPEC: u32 = 0;
/// Class ID.
pub const TCA_FLOWER_CLASSID: u32 = 1;
/// Indev.
pub const TCA_FLOWER_INDEV: u32 = 2;
/// Action.
pub const TCA_FLOWER_ACT: u32 = 3;
/// Ethernet destination.
pub const TCA_FLOWER_KEY_ETH_DST: u32 = 4;
/// Ethernet destination mask.
pub const TCA_FLOWER_KEY_ETH_DST_MASK: u32 = 5;
/// Ethernet source.
pub const TCA_FLOWER_KEY_ETH_SRC: u32 = 6;
/// Ethernet source mask.
pub const TCA_FLOWER_KEY_ETH_SRC_MASK: u32 = 7;
/// Ethernet type.
pub const TCA_FLOWER_KEY_ETH_TYPE: u32 = 8;
/// IP protocol.
pub const TCA_FLOWER_KEY_IP_PROTO: u32 = 9;
/// IPv4 source.
pub const TCA_FLOWER_KEY_IPV4_SRC: u32 = 10;
/// IPv4 source mask.
pub const TCA_FLOWER_KEY_IPV4_SRC_MASK: u32 = 11;
/// IPv4 destination.
pub const TCA_FLOWER_KEY_IPV4_DST: u32 = 12;
/// IPv4 destination mask.
pub const TCA_FLOWER_KEY_IPV4_DST_MASK: u32 = 13;
/// IPv6 source.
pub const TCA_FLOWER_KEY_IPV6_SRC: u32 = 14;
/// IPv6 source mask.
pub const TCA_FLOWER_KEY_IPV6_SRC_MASK: u32 = 15;
/// IPv6 destination.
pub const TCA_FLOWER_KEY_IPV6_DST: u32 = 16;
/// IPv6 destination mask.
pub const TCA_FLOWER_KEY_IPV6_DST_MASK: u32 = 17;
/// TCP source port.
pub const TCA_FLOWER_KEY_TCP_SRC: u32 = 18;
/// TCP destination port.
pub const TCA_FLOWER_KEY_TCP_DST: u32 = 19;
/// UDP source port.
pub const TCA_FLOWER_KEY_UDP_SRC: u32 = 20;
/// UDP destination port.
pub const TCA_FLOWER_KEY_UDP_DST: u32 = 21;
/// Flags.
pub const TCA_FLOWER_FLAGS: u32 = 22;
/// VLAN ID.
pub const TCA_FLOWER_KEY_VLAN_ID: u32 = 23;
/// VLAN priority.
pub const TCA_FLOWER_KEY_VLAN_PRIO: u32 = 24;
/// VLAN ethertype.
pub const TCA_FLOWER_KEY_VLAN_ETH_TYPE: u32 = 25;

// ---------------------------------------------------------------------------
// Flower classifier flags
// ---------------------------------------------------------------------------

/// Skip software.
pub const TCA_CLS_FLAGS_SKIP_SW: u32 = 1 << 0;
/// Skip hardware.
pub const TCA_CLS_FLAGS_SKIP_HW: u32 = 1 << 1;
/// In hardware.
pub const TCA_CLS_FLAGS_IN_HW: u32 = 1 << 2;
/// Not in hardware.
pub const TCA_CLS_FLAGS_NOT_IN_HW: u32 = 1 << 3;
/// Verbose.
pub const TCA_CLS_FLAGS_VERBOSE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flower_attrs_distinct() {
        let attrs = [
            TCA_FLOWER_UNSPEC, TCA_FLOWER_CLASSID, TCA_FLOWER_INDEV,
            TCA_FLOWER_ACT, TCA_FLOWER_KEY_ETH_DST,
            TCA_FLOWER_KEY_ETH_DST_MASK, TCA_FLOWER_KEY_ETH_SRC,
            TCA_FLOWER_KEY_ETH_SRC_MASK, TCA_FLOWER_KEY_ETH_TYPE,
            TCA_FLOWER_KEY_IP_PROTO, TCA_FLOWER_KEY_IPV4_SRC,
            TCA_FLOWER_KEY_IPV4_SRC_MASK, TCA_FLOWER_KEY_IPV4_DST,
            TCA_FLOWER_KEY_IPV4_DST_MASK, TCA_FLOWER_KEY_IPV6_SRC,
            TCA_FLOWER_KEY_IPV6_SRC_MASK, TCA_FLOWER_KEY_IPV6_DST,
            TCA_FLOWER_KEY_IPV6_DST_MASK, TCA_FLOWER_KEY_TCP_SRC,
            TCA_FLOWER_KEY_TCP_DST, TCA_FLOWER_KEY_UDP_SRC,
            TCA_FLOWER_KEY_UDP_DST, TCA_FLOWER_FLAGS,
            TCA_FLOWER_KEY_VLAN_ID, TCA_FLOWER_KEY_VLAN_PRIO,
            TCA_FLOWER_KEY_VLAN_ETH_TYPE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_cls_flags_no_overlap() {
        let flags = [
            TCA_CLS_FLAGS_SKIP_SW, TCA_CLS_FLAGS_SKIP_HW,
            TCA_CLS_FLAGS_IN_HW, TCA_CLS_FLAGS_NOT_IN_HW,
            TCA_CLS_FLAGS_VERBOSE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cls_flags_power_of_two() {
        let flags = [
            TCA_CLS_FLAGS_SKIP_SW, TCA_CLS_FLAGS_SKIP_HW,
            TCA_CLS_FLAGS_IN_HW, TCA_CLS_FLAGS_NOT_IN_HW,
            TCA_CLS_FLAGS_VERBOSE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not power of two", flag);
        }
    }
}
