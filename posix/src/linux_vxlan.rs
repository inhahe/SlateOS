//! `<linux/vxlan.h>` — VXLAN (Virtual eXtensible LAN) constants.
//!
//! VXLAN encapsulates layer-2 Ethernet frames in UDP datagrams
//! to create overlay networks. It supports up to 16 million
//! virtual networks (24-bit VNI), addressing the VLAN 4K limit.
//! Widely used in data center networking and cloud environments.

// ---------------------------------------------------------------------------
// VXLAN header
// ---------------------------------------------------------------------------

/// VXLAN UDP destination port (IANA standard).
pub const VXLAN_UDP_PORT: u16 = 4789;
/// Legacy VXLAN port (Linux default before standardization).
pub const VXLAN_UDP_PORT_LEGACY: u16 = 8472;
/// VXLAN header size (bytes).
pub const VXLAN_HLEN: usize = 8;

// ---------------------------------------------------------------------------
// VNI (VXLAN Network Identifier)
// ---------------------------------------------------------------------------

/// VNI field is 24 bits.
pub const VXLAN_VNI_BITS: u32 = 24;
/// Maximum VNI value.
pub const VXLAN_VNI_MAX: u32 = (1 << VXLAN_VNI_BITS) - 1;

// ---------------------------------------------------------------------------
// VXLAN header flags
// ---------------------------------------------------------------------------

/// VNI is valid (must be set in standard VXLAN).
pub const VXLAN_F_VNI: u32 = 1 << 3;
/// GBP (Group Based Policy) extension.
pub const VXLAN_F_GBP: u32 = 1 << 7;
/// GPE (Generic Protocol Extension).
pub const VXLAN_F_GPE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Netlink attributes (IFLA_VXLAN_*)
// ---------------------------------------------------------------------------

/// VNI.
pub const IFLA_VXLAN_ID: u16 = 1;
/// Multicast group (for BUM traffic).
pub const IFLA_VXLAN_GROUP: u16 = 2;
/// Local IP address.
pub const IFLA_VXLAN_LOCAL: u16 = 4;
/// TTL.
pub const IFLA_VXLAN_TTL: u16 = 5;
/// TOS.
pub const IFLA_VXLAN_TOS: u16 = 6;
/// Learning (dynamic FDB entries).
pub const IFLA_VXLAN_LEARNING: u16 = 7;
/// Ageing timer.
pub const IFLA_VXLAN_AGEING: u16 = 8;
/// FDB entry limit.
pub const IFLA_VXLAN_LIMIT: u16 = 9;
/// UDP port.
pub const IFLA_VXLAN_PORT: u16 = 15;
/// Collect metadata mode.
pub const IFLA_VXLAN_COLLECT_METADATA: u16 = 25;

// ---------------------------------------------------------------------------
// VXLAN device flags
// ---------------------------------------------------------------------------

/// Enable proxy ARP/NDP.
pub const VXLAN_DEV_F_PROXY: u32 = 1 << 0;
/// Enable route short circuit.
pub const VXLAN_DEV_F_RSC: u32 = 1 << 1;
/// Enable L2 MISS notification.
pub const VXLAN_DEV_F_L2MISS: u32 = 1 << 2;
/// Enable L3 MISS notification.
pub const VXLAN_DEV_F_L3MISS: u32 = 1 << 3;
/// Collect metadata.
pub const VXLAN_DEV_F_COLLECT_METADATA: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Link type
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND for vxlan.
pub const VXLAN_KIND: &str = "vxlan";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_ports_distinct() {
        assert_ne!(VXLAN_UDP_PORT, VXLAN_UDP_PORT_LEGACY);
    }

    #[test]
    fn test_vni_max() {
        assert_eq!(VXLAN_VNI_MAX, 0x00FFFFFF);
    }

    #[test]
    fn test_header_flags_distinct() {
        let flags = [VXLAN_F_VNI, VXLAN_F_GBP, VXLAN_F_GPE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_VXLAN_ID,
            IFLA_VXLAN_GROUP,
            IFLA_VXLAN_LOCAL,
            IFLA_VXLAN_TTL,
            IFLA_VXLAN_TOS,
            IFLA_VXLAN_LEARNING,
            IFLA_VXLAN_AGEING,
            IFLA_VXLAN_LIMIT,
            IFLA_VXLAN_PORT,
            IFLA_VXLAN_COLLECT_METADATA,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_dev_flags_powers_of_two() {
        let flags = [
            VXLAN_DEV_F_PROXY,
            VXLAN_DEV_F_RSC,
            VXLAN_DEV_F_L2MISS,
            VXLAN_DEV_F_L3MISS,
            VXLAN_DEV_F_COLLECT_METADATA,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_dev_flags_no_overlap() {
        let flags = [
            VXLAN_DEV_F_PROXY,
            VXLAN_DEV_F_RSC,
            VXLAN_DEV_F_L2MISS,
            VXLAN_DEV_F_L3MISS,
            VXLAN_DEV_F_COLLECT_METADATA,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_hlen() {
        assert_eq!(VXLAN_HLEN, 8);
    }
}
