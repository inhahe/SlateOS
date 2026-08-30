//! `<linux/hsr_netlink.h>` — High-availability Seamless Redundancy.
//!
//! HSR (IEC 62439-3) and its sibling PRP (Parallel Redundancy Protocol)
//! give industrial Ethernet zero-failover-time redundancy by duplicating
//! every frame onto two ring ports. The Linux `hsr` driver exposes the
//! ring state to userspace (`iproute2`, `ethtool`) over the rtnetlink
//! attributes below.

// ---------------------------------------------------------------------------
// Link-info type strings (passed in IFLA_INFO_KIND)
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND value for HSR links.
pub const HSR_INFO_KIND: &str = "hsr";

// ---------------------------------------------------------------------------
// IFLA_HSR_* attributes
// ---------------------------------------------------------------------------

/// Sentinel.
pub const IFLA_HSR_UNSPEC: u32 = 0;
/// First slave interface (Slot A).
pub const IFLA_HSR_SLAVE1: u32 = 1;
/// Second slave interface (Slot B).
pub const IFLA_HSR_SLAVE2: u32 = 2;
/// Multicast address used by supervision frames.
pub const IFLA_HSR_MULTICAST_SPEC: u32 = 3;
/// Last-known sequence-number nonce (for debug).
pub const IFLA_HSR_SUPERVISION_ADDR: u32 = 4;
/// HSR protocol version (0=HSRv0, 1=HSRv1).
pub const IFLA_HSR_SEQ_NR: u32 = 5;
/// Per-interface sequence number.
pub const IFLA_HSR_VERSION: u32 = 6;
/// Protocol: 0=HSR, 1=PRP (extension in Linux 5.6+).
pub const IFLA_HSR_PROTOCOL: u32 = 7;

// ---------------------------------------------------------------------------
// hsr_version (struct hsr_priv.prot_version)
// ---------------------------------------------------------------------------

/// HSRv0 (IEC 62439-3 ed.1).
pub const HSR_V0: u32 = 0;
/// HSRv1 (IEC 62439-3 ed.2).
pub const HSR_V1: u32 = 1;
/// PRP-1 (Parallel Redundancy Protocol).
pub const PRP_V1: u32 = 2;

// ---------------------------------------------------------------------------
// EtherType for HSR-tagged frames
// ---------------------------------------------------------------------------

/// HSR tag EtherType (0x892F, registered to IEC).
pub const ETH_P_HSR: u16 = 0x892F;
/// PRP RCT trailer — no EtherType, marker is 0x88FB in some impls.
pub const ETH_P_PRP: u16 = 0x88FB;

// ---------------------------------------------------------------------------
// Supervision-frame multicast address (constant from IEC 62439-3)
// ---------------------------------------------------------------------------

/// Standard supervision-frame multicast MAC OUI prefix.
pub const HSR_SUPERVISION_MAC_PREFIX: [u8; 3] = [0x01, 0x15, 0x4E];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_kind() {
        assert_eq!(HSR_INFO_KIND, "hsr");
        // Must fit in IFNAMSIZ (16) minus NUL.
        assert!(HSR_INFO_KIND.len() < 16);
    }

    #[test]
    fn test_ifla_attributes_dense() {
        let a = [
            IFLA_HSR_UNSPEC,
            IFLA_HSR_SLAVE1,
            IFLA_HSR_SLAVE2,
            IFLA_HSR_MULTICAST_SPEC,
            IFLA_HSR_SUPERVISION_ADDR,
            IFLA_HSR_SEQ_NR,
            IFLA_HSR_VERSION,
            IFLA_HSR_PROTOCOL,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_protocol_versions_distinct() {
        assert_ne!(HSR_V0, HSR_V1);
        assert_ne!(HSR_V1, PRP_V1);
        // Dense 0,1,2.
        assert_eq!(HSR_V0, 0);
        assert_eq!(HSR_V1, 1);
        assert_eq!(PRP_V1, 2);
    }

    #[test]
    fn test_ethertypes_in_uapi_range() {
        // Both in the unassigned-Ethertype range 0x05DD..0xFFFF.
        assert!(ETH_P_HSR > 0x05DD);
        assert!(ETH_P_PRP > 0x05DD);
        assert_ne!(ETH_P_HSR, ETH_P_PRP);
    }

    #[test]
    fn test_supervision_mac_oui_is_iec() {
        // 01:15:4E:xx:xx:xx is the IEC-assigned supervision multicast OUI.
        assert_eq!(HSR_SUPERVISION_MAC_PREFIX, [0x01, 0x15, 0x4E]);
        // First byte's low bit set ⇒ multicast.
        assert_eq!(HSR_SUPERVISION_MAC_PREFIX[0] & 1, 1);
    }
}
