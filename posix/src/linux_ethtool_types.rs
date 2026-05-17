//! `<linux/ethtool.h>` — Ethtool interface constants.
//!
//! Ethtool provides a standardized interface for querying and
//! configuring Ethernet device parameters: link speed, duplex,
//! autonegotiation, ring buffer sizes, offload features, and
//! self-test capabilities.

// ---------------------------------------------------------------------------
// Link mode speed/duplex
// ---------------------------------------------------------------------------

/// Speed 10 Mbps.
pub const ETHTOOL_SPEED_10: u32 = 10;
/// Speed 100 Mbps.
pub const ETHTOOL_SPEED_100: u32 = 100;
/// Speed 1000 Mbps (1G).
pub const ETHTOOL_SPEED_1000: u32 = 1000;
/// Speed 2500 Mbps (2.5G).
pub const ETHTOOL_SPEED_2500: u32 = 2500;
/// Speed 5000 Mbps (5G).
pub const ETHTOOL_SPEED_5000: u32 = 5000;
/// Speed 10000 Mbps (10G).
pub const ETHTOOL_SPEED_10000: u32 = 10000;
/// Speed 25000 Mbps (25G).
pub const ETHTOOL_SPEED_25000: u32 = 25000;
/// Speed 50000 Mbps (50G).
pub const ETHTOOL_SPEED_50000: u32 = 50000;
/// Speed 100000 Mbps (100G).
pub const ETHTOOL_SPEED_100000: u32 = 100000;

// ---------------------------------------------------------------------------
// Feature flags (offloads)
// ---------------------------------------------------------------------------

/// Receive checksum offload.
pub const ETHTOOL_F_RX_CSUM: u32 = 1 << 0;
/// Transmit checksum offload.
pub const ETHTOOL_F_TX_CSUM: u32 = 1 << 1;
/// TCP Segmentation Offload.
pub const ETHTOOL_F_TSO: u32 = 1 << 2;
/// Generic Segmentation Offload.
pub const ETHTOOL_F_GSO: u32 = 1 << 3;
/// Generic Receive Offload.
pub const ETHTOOL_F_GRO: u32 = 1 << 4;
/// Large Receive Offload.
pub const ETHTOOL_F_LRO: u32 = 1 << 5;
/// Scatter-Gather.
pub const ETHTOOL_F_SG: u32 = 1 << 6;
/// VLAN TX insert.
pub const ETHTOOL_F_TX_VLAN: u32 = 1 << 7;
/// VLAN RX strip.
pub const ETHTOOL_F_RX_VLAN: u32 = 1 << 8;
/// Receive hashing (RSS).
pub const ETHTOOL_F_RXHASH: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Wake-on-LAN modes
// ---------------------------------------------------------------------------

/// Wake on PHY activity.
pub const WAKE_PHY: u32 = 1 << 0;
/// Wake on unicast frame.
pub const WAKE_UCAST: u32 = 1 << 1;
/// Wake on multicast frame.
pub const WAKE_MCAST: u32 = 1 << 2;
/// Wake on broadcast frame.
pub const WAKE_BCAST: u32 = 1 << 3;
/// Wake on ARP.
pub const WAKE_ARP: u32 = 1 << 4;
/// Wake on Magic Packet.
pub const WAKE_MAGIC: u32 = 1 << 5;
/// Wake on filter match.
pub const WAKE_FILTER: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Ring parameter IDs
// ---------------------------------------------------------------------------

/// RX ring size.
pub const ETHTOOL_RING_RX: u8 = 0;
/// TX ring size.
pub const ETHTOOL_RING_TX: u8 = 1;
/// RX mini ring.
pub const ETHTOOL_RING_RX_MINI: u8 = 2;
/// RX jumbo ring.
pub const ETHTOOL_RING_RX_JUMBO: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speeds_increasing() {
        let speeds = [
            ETHTOOL_SPEED_10, ETHTOOL_SPEED_100, ETHTOOL_SPEED_1000,
            ETHTOOL_SPEED_2500, ETHTOOL_SPEED_5000, ETHTOOL_SPEED_10000,
            ETHTOOL_SPEED_25000, ETHTOOL_SPEED_50000, ETHTOOL_SPEED_100000,
        ];
        for i in 1..speeds.len() {
            assert!(speeds[i] > speeds[i - 1]);
        }
    }

    #[test]
    fn test_features_no_overlap() {
        let feats = [
            ETHTOOL_F_RX_CSUM, ETHTOOL_F_TX_CSUM, ETHTOOL_F_TSO,
            ETHTOOL_F_GSO, ETHTOOL_F_GRO, ETHTOOL_F_LRO,
            ETHTOOL_F_SG, ETHTOOL_F_TX_VLAN, ETHTOOL_F_RX_VLAN,
            ETHTOOL_F_RXHASH,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_wol_modes_no_overlap() {
        let modes = [
            WAKE_PHY, WAKE_UCAST, WAKE_MCAST, WAKE_BCAST,
            WAKE_ARP, WAKE_MAGIC, WAKE_FILTER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_ring_ids_distinct() {
        let rings = [
            ETHTOOL_RING_RX, ETHTOOL_RING_TX,
            ETHTOOL_RING_RX_MINI, ETHTOOL_RING_RX_JUMBO,
        ];
        for i in 0..rings.len() {
            for j in (i + 1)..rings.len() {
                assert_ne!(rings[i], rings[j]);
            }
        }
    }
}
