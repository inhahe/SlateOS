//! `<linux/ethtool.h>` — Additional ethtool constants (part 4).
//!
//! Supplementary ethtool constants covering PHY tunable IDs,
//! module power modes, and FEC encoding types.

// ---------------------------------------------------------------------------
// Ethtool PHY tunable IDs
// ---------------------------------------------------------------------------

/// Unspec tunable.
pub const ETHTOOL_PHY_TUNABLE_UNSPEC: u32 = 0;
/// Downshift after retries.
pub const ETHTOOL_PHY_DOWNSHIFT: u32 = 1;
/// Fast link down.
pub const ETHTOOL_PHY_FAST_LINK_DOWN: u32 = 2;
/// EDPD (Energy Detect Power Down).
pub const ETHTOOL_PHY_EDPD: u32 = 3;

// ---------------------------------------------------------------------------
// Ethtool FEC encoding types
// ---------------------------------------------------------------------------

/// No FEC.
pub const ETHTOOL_FEC_NONE: u32 = 1 << 0;
/// Auto FEC.
pub const ETHTOOL_FEC_AUTO: u32 = 1 << 1;
/// Off.
pub const ETHTOOL_FEC_OFF: u32 = 1 << 2;
/// Reed-Solomon FEC.
pub const ETHTOOL_FEC_RS: u32 = 1 << 3;
/// Base-R FEC.
pub const ETHTOOL_FEC_BASER: u32 = 1 << 4;
/// Low-latency RS FEC.
pub const ETHTOOL_FEC_LLRS: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Ethtool module power modes
// ---------------------------------------------------------------------------

/// Low power mode.
pub const ETHTOOL_MODULE_POWER_MODE_LOW: u32 = 1;
/// High power mode.
pub const ETHTOOL_MODULE_POWER_MODE_HIGH: u32 = 2;

// ---------------------------------------------------------------------------
// Ethtool module power mode policy
// ---------------------------------------------------------------------------

/// High power always.
pub const ETHTOOL_MODULE_POWER_MODE_POLICY_HIGH: u32 = 1;
/// Auto power mode.
pub const ETHTOOL_MODULE_POWER_MODE_POLICY_AUTO: u32 = 2;

// ---------------------------------------------------------------------------
// Ethtool reset flags
// ---------------------------------------------------------------------------

/// Global reset (all).
pub const ETH_RESET_MGMT: u32 = 1 << 0;
/// IRQ reset.
pub const ETH_RESET_IRQ: u32 = 1 << 1;
/// DMA reset.
pub const ETH_RESET_DMA: u32 = 1 << 2;
/// Filter reset.
pub const ETH_RESET_FILTER: u32 = 1 << 3;
/// Offload reset.
pub const ETH_RESET_OFFLOAD: u32 = 1 << 4;
/// MAC reset.
pub const ETH_RESET_MAC: u32 = 1 << 5;
/// PHY reset.
pub const ETH_RESET_PHY: u32 = 1 << 6;
/// RAM reset.
pub const ETH_RESET_RAM: u32 = 1 << 7;
/// AP reset.
pub const ETH_RESET_AP: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phy_tunables_distinct() {
        let tunables = [
            ETHTOOL_PHY_TUNABLE_UNSPEC,
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

    #[test]
    fn test_fec_types_no_overlap() {
        let types = [
            ETHTOOL_FEC_NONE,
            ETHTOOL_FEC_AUTO,
            ETHTOOL_FEC_OFF,
            ETHTOOL_FEC_RS,
            ETHTOOL_FEC_BASER,
            ETHTOOL_FEC_LLRS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_power_modes_distinct() {
        assert_ne!(
            ETHTOOL_MODULE_POWER_MODE_LOW,
            ETHTOOL_MODULE_POWER_MODE_HIGH
        );
    }

    #[test]
    fn test_reset_flags_no_overlap() {
        let flags = [
            ETH_RESET_MGMT,
            ETH_RESET_IRQ,
            ETH_RESET_DMA,
            ETH_RESET_FILTER,
            ETH_RESET_OFFLOAD,
            ETH_RESET_MAC,
            ETH_RESET_PHY,
            ETH_RESET_RAM,
            ETH_RESET_AP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
