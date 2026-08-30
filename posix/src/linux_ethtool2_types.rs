//! `<linux/ethtool.h>` — Ethtool constants (extended).
//!
//! Extended ethtool constants covering link modes, ring
//! parameters, coalesce parameters, pause parameters,
//! and RSS (Receive Side Scaling) configuration.

// ---------------------------------------------------------------------------
// Ethtool link mode bits (ETHTOOL_LINK_MODE_*)
// ---------------------------------------------------------------------------

/// 10baseT Half.
pub const ETHTOOL_LINK_MODE_10baseT_Half: u32 = 0;
/// 10baseT Full.
pub const ETHTOOL_LINK_MODE_10baseT_Full: u32 = 1;
/// 100baseT Half.
pub const ETHTOOL_LINK_MODE_100baseT_Half: u32 = 2;
/// 100baseT Full.
pub const ETHTOOL_LINK_MODE_100baseT_Full: u32 = 3;
/// 1000baseT Half.
pub const ETHTOOL_LINK_MODE_1000baseT_Half: u32 = 4;
/// 1000baseT Full.
pub const ETHTOOL_LINK_MODE_1000baseT_Full: u32 = 5;
/// Auto-negotiation.
pub const ETHTOOL_LINK_MODE_Autoneg: u32 = 6;
/// Twisted Pair.
pub const ETHTOOL_LINK_MODE_TP: u32 = 7;
/// AUI.
pub const ETHTOOL_LINK_MODE_AUI: u32 = 8;
/// MII.
pub const ETHTOOL_LINK_MODE_MII: u32 = 9;
/// Fibre.
pub const ETHTOOL_LINK_MODE_FIBRE: u32 = 10;
/// BNC.
pub const ETHTOOL_LINK_MODE_BNC: u32 = 11;
/// 10000baseT Full.
pub const ETHTOOL_LINK_MODE_10000baseT_Full: u32 = 12;
/// Pause.
pub const ETHTOOL_LINK_MODE_Pause: u32 = 13;
/// Asymmetric Pause.
pub const ETHTOOL_LINK_MODE_Asym_Pause: u32 = 14;
/// 2500baseX Full.
pub const ETHTOOL_LINK_MODE_2500baseX_Full: u32 = 15;
/// Backplane.
pub const ETHTOOL_LINK_MODE_Backplane: u32 = 16;
/// 1000baseKX Full.
pub const ETHTOOL_LINK_MODE_1000baseKX_Full: u32 = 17;
/// 10000baseKX4 Full.
pub const ETHTOOL_LINK_MODE_10000baseKX4_Full: u32 = 18;
/// 10000baseKR Full.
pub const ETHTOOL_LINK_MODE_10000baseKR_Full: u32 = 19;
/// 25000baseCR Full.
pub const ETHTOOL_LINK_MODE_25000baseCR_Full: u32 = 31;
/// 50000baseCR2 Full.
pub const ETHTOOL_LINK_MODE_50000baseCR2_Full: u32 = 34;
/// 100000baseCR4 Full.
pub const ETHTOOL_LINK_MODE_100000baseCR4_Full: u32 = 38;

// ---------------------------------------------------------------------------
// Ethtool ring parameter commands
// ---------------------------------------------------------------------------

/// Get ring parameters.
pub const ETHTOOL_GRINGPARAM: u32 = 0x00000010;
/// Set ring parameters.
pub const ETHTOOL_SRINGPARAM: u32 = 0x00000011;

// ---------------------------------------------------------------------------
// Ethtool coalesce commands
// ---------------------------------------------------------------------------

/// Get coalesce parameters.
pub const ETHTOOL_GCOALESCE: u32 = 0x0000000E;
/// Set coalesce parameters.
pub const ETHTOOL_SCOALESCE: u32 = 0x0000000F;

// ---------------------------------------------------------------------------
// Ethtool pause commands
// ---------------------------------------------------------------------------

/// Get pause parameters.
pub const ETHTOOL_GPAUSEPARAM: u32 = 0x00000012;
/// Set pause parameters.
pub const ETHTOOL_SPAUSEPARAM: u32 = 0x00000013;

// ---------------------------------------------------------------------------
// Ethtool RSS / RX flow hash commands
// ---------------------------------------------------------------------------

/// Get RX flow hash.
pub const ETHTOOL_GRXFH: u32 = 0x00000029;
/// Set RX flow hash.
pub const ETHTOOL_SRXFH: u32 = 0x0000002A;
/// Get RX flow hash indirection table.
pub const ETHTOOL_GRXFHINDIR: u32 = 0x00000038;
/// Set RX flow hash indirection table.
pub const ETHTOOL_SRXFHINDIR: u32 = 0x00000039;
/// Get RSS hash key.
pub const ETHTOOL_GRSSH: u32 = 0x00000046;
/// Set RSS hash key.
pub const ETHTOOL_SRSSH: u32 = 0x00000047;

// ---------------------------------------------------------------------------
// Ethtool RX flow hash flags (RXH_*)
// ---------------------------------------------------------------------------

/// Layer 2 (MAC) destination.
pub const RXH_L2DA: u32 = 1 << 1;
/// VLAN tag.
pub const RXH_VLAN: u32 = 1 << 2;
/// Layer 3 protocol.
pub const RXH_L3_PROTO: u32 = 1 << 3;
/// IP source address.
pub const RXH_IP_SRC: u32 = 1 << 4;
/// IP destination address.
pub const RXH_IP_DST: u32 = 1 << 5;
/// L4 source port (byte 0-1).
pub const RXH_L4_B_0_1: u32 = 1 << 6;
/// L4 destination port (byte 2-3).
pub const RXH_L4_B_2_3: u32 = 1 << 7;
/// Discard.
pub const RXH_DISCARD: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_modes_distinct() {
        let modes = [
            ETHTOOL_LINK_MODE_10baseT_Half,
            ETHTOOL_LINK_MODE_10baseT_Full,
            ETHTOOL_LINK_MODE_100baseT_Half,
            ETHTOOL_LINK_MODE_100baseT_Full,
            ETHTOOL_LINK_MODE_1000baseT_Half,
            ETHTOOL_LINK_MODE_1000baseT_Full,
            ETHTOOL_LINK_MODE_Autoneg,
            ETHTOOL_LINK_MODE_TP,
            ETHTOOL_LINK_MODE_AUI,
            ETHTOOL_LINK_MODE_MII,
            ETHTOOL_LINK_MODE_FIBRE,
            ETHTOOL_LINK_MODE_BNC,
            ETHTOOL_LINK_MODE_10000baseT_Full,
            ETHTOOL_LINK_MODE_Pause,
            ETHTOOL_LINK_MODE_Asym_Pause,
            ETHTOOL_LINK_MODE_2500baseX_Full,
            ETHTOOL_LINK_MODE_Backplane,
            ETHTOOL_LINK_MODE_1000baseKX_Full,
            ETHTOOL_LINK_MODE_10000baseKX4_Full,
            ETHTOOL_LINK_MODE_10000baseKR_Full,
            ETHTOOL_LINK_MODE_25000baseCR_Full,
            ETHTOOL_LINK_MODE_50000baseCR2_Full,
            ETHTOOL_LINK_MODE_100000baseCR4_Full,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_ring_cmds_distinct() {
        assert_ne!(ETHTOOL_GRINGPARAM, ETHTOOL_SRINGPARAM);
    }

    #[test]
    fn test_coalesce_cmds_distinct() {
        assert_ne!(ETHTOOL_GCOALESCE, ETHTOOL_SCOALESCE);
    }

    #[test]
    fn test_pause_cmds_distinct() {
        assert_ne!(ETHTOOL_GPAUSEPARAM, ETHTOOL_SPAUSEPARAM);
    }

    #[test]
    fn test_rss_cmds_distinct() {
        let cmds = [
            ETHTOOL_GRXFH,
            ETHTOOL_SRXFH,
            ETHTOOL_GRXFHINDIR,
            ETHTOOL_SRXFHINDIR,
            ETHTOOL_GRSSH,
            ETHTOOL_SRSSH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rxh_flags_powers_of_two() {
        let flags = [
            RXH_L2DA,
            RXH_VLAN,
            RXH_L3_PROTO,
            RXH_IP_SRC,
            RXH_IP_DST,
            RXH_L4_B_0_1,
            RXH_L4_B_2_3,
            RXH_DISCARD,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "rxh flag {f:#x} not power of two");
        }
    }

    #[test]
    fn test_rxh_flags_no_overlap() {
        let flags = [
            RXH_L2DA,
            RXH_VLAN,
            RXH_L3_PROTO,
            RXH_IP_SRC,
            RXH_IP_DST,
            RXH_L4_B_0_1,
            RXH_L4_B_2_3,
            RXH_DISCARD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_10base_half_is_zero() {
        assert_eq!(ETHTOOL_LINK_MODE_10baseT_Half, 0);
    }
}
