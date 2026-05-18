//! `<linux/netdevice.h>` — Network device private flag constants.
//!
//! These are kernel-internal flags for network devices, separate
//! from the user-visible IFF_* flags. They control hardware
//! offload capabilities, VLAN handling, and other NIC features.

// ---------------------------------------------------------------------------
// Net device features (NETIF_F_*)
// ---------------------------------------------------------------------------

/// Scatter/gather I/O.
pub const NETIF_F_SG: u64 = 1 << 0;
/// Hardware IP checksum.
pub const NETIF_F_IP_CSUM: u64 = 1 << 1;
/// Hardware checksum for all protocols.
pub const NETIF_F_HW_CSUM: u64 = 1 << 2;
/// Hardware VLAN tag insertion.
pub const NETIF_F_HW_VLAN_CTAG_TX: u64 = 1 << 3;
/// Hardware VLAN tag extraction.
pub const NETIF_F_HW_VLAN_CTAG_RX: u64 = 1 << 4;
/// Hardware VLAN tag filtering.
pub const NETIF_F_HW_VLAN_CTAG_FILTER: u64 = 1 << 5;
/// TCP segmentation offload (TSO).
pub const NETIF_F_TSO: u64 = 1 << 6;
/// UDP fragmentation offload.
pub const NETIF_F_UFO: u64 = 1 << 7;
/// Generic segmentation offload.
pub const NETIF_F_GSO: u64 = 1 << 8;
/// Generic receive offload.
pub const NETIF_F_GRO: u64 = 1 << 9;
/// Large receive offload.
pub const NETIF_F_LRO: u64 = 1 << 10;
/// TSO for IPv6.
pub const NETIF_F_TSO6: u64 = 1 << 11;
/// IPv6 checksum offload.
pub const NETIF_F_IPV6_CSUM: u64 = 1 << 12;
/// Receive hashing offload.
pub const NETIF_F_RXHASH: u64 = 1 << 13;
/// Receive checksum offload.
pub const NETIF_F_RXCSUM: u64 = 1 << 14;
/// Netns-local features.
pub const NETIF_F_NETNS_LOCAL: u64 = 1 << 15;
/// Transmit lockless.
pub const NETIF_F_LLTX: u64 = 1 << 16;
/// VLAN challenged (cannot handle VLANs).
pub const NETIF_F_VLAN_CHALLENGED: u64 = 1 << 17;
/// Hardware timestamping TX.
pub const NETIF_F_HW_TC: u64 = 1 << 18;
/// Loopback device.
pub const NETIF_F_LOOPBACK: u64 = 1 << 19;

// ---------------------------------------------------------------------------
// Net device flags (IFF_* private/kernel-only)
// ---------------------------------------------------------------------------

/// Interface is in 802.1Q VLAN mode.
pub const IFF_802_1Q_VLAN: u32 = 1 << 0;
/// Bonding/teaming master.
pub const IFF_BONDING: u32 = 1 << 5;
/// Interface is a bridge.
pub const IFF_BRIDGE_PORT: u32 = 1 << 8;
/// Open vSwitch datapath port.
pub const IFF_OVS_DATAPATH: u32 = 1 << 9;
/// Interface is a macvlan.
pub const IFF_MACVLAN: u32 = 1 << 21;
/// Interface has xfrm (IPsec) state.
pub const IFF_XMIT_DST_RELEASE: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// Net device name length
// ---------------------------------------------------------------------------

/// Maximum network device name length (including null).
pub const IFNAMSIZ: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_no_overlap() {
        let feats = [
            NETIF_F_SG, NETIF_F_IP_CSUM, NETIF_F_HW_CSUM,
            NETIF_F_HW_VLAN_CTAG_TX, NETIF_F_HW_VLAN_CTAG_RX,
            NETIF_F_HW_VLAN_CTAG_FILTER, NETIF_F_TSO,
            NETIF_F_UFO, NETIF_F_GSO, NETIF_F_GRO,
            NETIF_F_LRO, NETIF_F_TSO6, NETIF_F_IPV6_CSUM,
            NETIF_F_RXHASH, NETIF_F_RXCSUM, NETIF_F_NETNS_LOCAL,
            NETIF_F_LLTX, NETIF_F_VLAN_CHALLENGED,
            NETIF_F_HW_TC, NETIF_F_LOOPBACK,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_features_power_of_two() {
        let feats = [
            NETIF_F_SG, NETIF_F_IP_CSUM, NETIF_F_HW_CSUM,
            NETIF_F_TSO, NETIF_F_GSO, NETIF_F_GRO,
        ];
        for f in &feats {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_ifnamsiz() {
        assert_eq!(IFNAMSIZ, 16);
    }

    #[test]
    fn test_sg_is_bit0() {
        assert_eq!(NETIF_F_SG, 1);
    }
}
