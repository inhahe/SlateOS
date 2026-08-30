//! `<linux/ethtool.h>` — Ethtool link mode and speed constants.
//!
//! These constants define supported link speeds, duplex modes,
//! and auto-negotiation states used by the ethtool interface.

// ---------------------------------------------------------------------------
// Link speeds (Mbps)
// ---------------------------------------------------------------------------

/// 10 Mbps.
pub const SPEED_10: u32 = 10;
/// 100 Mbps.
pub const SPEED_100: u32 = 100;
/// 1 Gbps.
pub const SPEED_1000: u32 = 1000;
/// 2.5 Gbps.
pub const SPEED_2500: u32 = 2500;
/// 5 Gbps.
pub const SPEED_5000: u32 = 5000;
/// 10 Gbps.
pub const SPEED_10000: u32 = 10000;
/// 14 Gbps (InfiniBand FDR).
pub const SPEED_14000: u32 = 14000;
/// 20 Gbps.
pub const SPEED_20000: u32 = 20000;
/// 25 Gbps.
pub const SPEED_25000: u32 = 25000;
/// 40 Gbps.
pub const SPEED_40000: u32 = 40000;
/// 50 Gbps.
pub const SPEED_50000: u32 = 50000;
/// 56 Gbps (InfiniBand FDR10).
pub const SPEED_56000: u32 = 56000;
/// 100 Gbps.
pub const SPEED_100000: u32 = 100000;
/// 200 Gbps.
pub const SPEED_200000: u32 = 200000;
/// 400 Gbps.
pub const SPEED_400000: u32 = 400000;
/// 800 Gbps.
pub const SPEED_800000: u32 = 800000;
/// Unknown speed.
pub const SPEED_UNKNOWN: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Duplex modes
// ---------------------------------------------------------------------------

/// Half duplex.
pub const DUPLEX_HALF: u8 = 0;
/// Full duplex.
pub const DUPLEX_FULL: u8 = 1;
/// Unknown duplex.
pub const DUPLEX_UNKNOWN: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Auto-negotiation
// ---------------------------------------------------------------------------

/// Auto-negotiation disabled.
pub const AUTONEG_DISABLE: u8 = 0;
/// Auto-negotiation enabled.
pub const AUTONEG_ENABLE: u8 = 1;

// ---------------------------------------------------------------------------
// Port types
// ---------------------------------------------------------------------------

/// Twisted pair (RJ45).
pub const PORT_TP: u8 = 0;
/// AUI.
pub const PORT_AUI: u8 = 1;
/// MII (media independent interface).
pub const PORT_MII: u8 = 2;
/// Fibre optic.
pub const PORT_FIBRE: u8 = 3;
/// BNC.
pub const PORT_BNC: u8 = 4;
/// Direct attach copper.
pub const PORT_DA: u8 = 5;
/// None.
pub const PORT_NONE: u8 = 0xEF;
/// Other.
pub const PORT_OTHER: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speeds_distinct() {
        let speeds = [
            SPEED_10,
            SPEED_100,
            SPEED_1000,
            SPEED_2500,
            SPEED_5000,
            SPEED_10000,
            SPEED_14000,
            SPEED_20000,
            SPEED_25000,
            SPEED_40000,
            SPEED_50000,
            SPEED_56000,
            SPEED_100000,
            SPEED_200000,
            SPEED_400000,
            SPEED_800000,
            SPEED_UNKNOWN,
        ];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }

    #[test]
    fn test_speed_unknown() {
        assert_eq!(SPEED_UNKNOWN, u32::MAX);
    }

    #[test]
    fn test_duplex_modes_distinct() {
        let modes = [DUPLEX_HALF, DUPLEX_FULL, DUPLEX_UNKNOWN];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_autoneg_distinct() {
        assert_ne!(AUTONEG_DISABLE, AUTONEG_ENABLE);
    }

    #[test]
    fn test_port_types_distinct() {
        let ports = [
            PORT_TP, PORT_AUI, PORT_MII, PORT_FIBRE, PORT_BNC, PORT_DA, PORT_NONE, PORT_OTHER,
        ];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j]);
            }
        }
    }

    #[test]
    fn test_speed_1000() {
        assert_eq!(SPEED_1000, 1000);
    }
}
