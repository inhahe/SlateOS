//! `<linux/ethtool.h>` — Additional ethtool constants (batch 3).
//!
//! Supplementary ethtool constants covering link modes,
//! FEC encoding types, and module power modes.

// ---------------------------------------------------------------------------
// Ethtool FEC (Forward Error Correction) modes
// ---------------------------------------------------------------------------

/// No FEC.
pub const ETHTOOL_FEC_NONE: u32 = 1 << 0;
/// Auto FEC.
pub const ETHTOOL_FEC_AUTO: u32 = 1 << 1;
/// Off (no FEC active).
pub const ETHTOOL_FEC_OFF: u32 = 1 << 2;
/// Reed-Solomon FEC.
pub const ETHTOOL_FEC_RS: u32 = 1 << 3;
/// Base-R FEC (Fire Code).
pub const ETHTOOL_FEC_BASER: u32 = 1 << 4;
/// Low-latency RS-FEC.
pub const ETHTOOL_FEC_LLRS: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Ethtool link mode speeds
// ---------------------------------------------------------------------------

/// 10 Mbps half-duplex.
pub const ETHTOOL_LINK_MODE_10baseT_Half: u32 = 0;
/// 10 Mbps full-duplex.
pub const ETHTOOL_LINK_MODE_10baseT_Full: u32 = 1;
/// 100 Mbps half-duplex.
pub const ETHTOOL_LINK_MODE_100baseT_Half: u32 = 2;
/// 100 Mbps full-duplex.
pub const ETHTOOL_LINK_MODE_100baseT_Full: u32 = 3;
/// 1 Gbps half-duplex.
pub const ETHTOOL_LINK_MODE_1000baseT_Half: u32 = 4;
/// 1 Gbps full-duplex.
pub const ETHTOOL_LINK_MODE_1000baseT_Full: u32 = 5;
/// 10 Gbps full-duplex.
pub const ETHTOOL_LINK_MODE_10000baseT_Full: u32 = 12;
/// 25 Gbps full-duplex (CR).
pub const ETHTOOL_LINK_MODE_25000baseCR_Full: u32 = 31;
/// 50 Gbps full-duplex (CR2).
pub const ETHTOOL_LINK_MODE_50000baseCR2_Full: u32 = 34;
/// 100 Gbps full-duplex (CR4).
pub const ETHTOOL_LINK_MODE_100000baseCR4_Full: u32 = 38;
/// 200 Gbps full-duplex (CR4).
pub const ETHTOOL_LINK_MODE_200000baseCR4_Full: u32 = 66;
/// 400 Gbps full-duplex (CR8).
pub const ETHTOOL_LINK_MODE_400000baseCR8_Full: u32 = 82;

// ---------------------------------------------------------------------------
// Ethtool module power modes
// ---------------------------------------------------------------------------

/// Module: low power.
pub const ETHTOOL_MODULE_POWER_MODE_LOW: u32 = 1;
/// Module: high power.
pub const ETHTOOL_MODULE_POWER_MODE_HIGH: u32 = 2;

// ---------------------------------------------------------------------------
// Ethtool PHY tunable IDs
// ---------------------------------------------------------------------------

/// PHY downshift count.
pub const ETHTOOL_PHY_DOWNSHIFT: u32 = 1;
/// PHY fast link down.
pub const ETHTOOL_PHY_FAST_LINK_DOWN: u32 = 2;
/// PHY energy detect power down.
pub const ETHTOOL_PHY_EDPD: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fec_modes_power_of_two() {
        let modes = [
            ETHTOOL_FEC_NONE,
            ETHTOOL_FEC_AUTO,
            ETHTOOL_FEC_OFF,
            ETHTOOL_FEC_RS,
            ETHTOOL_FEC_BASER,
            ETHTOOL_FEC_LLRS,
        ];
        for m in &modes {
            assert!(m.is_power_of_two(), "0x{:08x} not power of two", m);
        }
    }

    #[test]
    fn test_fec_modes_no_overlap() {
        let modes = [
            ETHTOOL_FEC_NONE,
            ETHTOOL_FEC_AUTO,
            ETHTOOL_FEC_OFF,
            ETHTOOL_FEC_RS,
            ETHTOOL_FEC_BASER,
            ETHTOOL_FEC_LLRS,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_link_modes_distinct() {
        let modes = [
            ETHTOOL_LINK_MODE_10baseT_Half,
            ETHTOOL_LINK_MODE_10baseT_Full,
            ETHTOOL_LINK_MODE_100baseT_Half,
            ETHTOOL_LINK_MODE_100baseT_Full,
            ETHTOOL_LINK_MODE_1000baseT_Half,
            ETHTOOL_LINK_MODE_1000baseT_Full,
            ETHTOOL_LINK_MODE_10000baseT_Full,
            ETHTOOL_LINK_MODE_25000baseCR_Full,
            ETHTOOL_LINK_MODE_50000baseCR2_Full,
            ETHTOOL_LINK_MODE_100000baseCR4_Full,
            ETHTOOL_LINK_MODE_200000baseCR4_Full,
            ETHTOOL_LINK_MODE_400000baseCR8_Full,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_module_power_modes_distinct() {
        assert_ne!(
            ETHTOOL_MODULE_POWER_MODE_LOW,
            ETHTOOL_MODULE_POWER_MODE_HIGH
        );
    }

    #[test]
    fn test_phy_tunables_distinct() {
        let tunables = [
            ETHTOOL_PHY_DOWNSHIFT,
            ETHTOOL_PHY_FAST_LINK_DOWN,
            ETHTOOL_PHY_EDPD,
        ];
        for i in 0..tunables.len() {
            for j in (i + 1)..tunables.len() {
                assert_ne!(tunables[i], tunables[j]);
            }
        }
    }
}
