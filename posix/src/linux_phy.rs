//! `<linux/phy.h>` — Ethernet PHY transceiver constants.
//!
//! PHY (Physical layer) transceivers handle the electrical signaling
//! for Ethernet. The Linux PHYLIB framework manages PHY devices,
//! link state, auto-negotiation, and speed/duplex configuration.

// ---------------------------------------------------------------------------
// PHY interface modes
// ---------------------------------------------------------------------------

/// Internal (no external PHY).
pub const PHY_INTERFACE_MODE_NA: u32 = 0;
/// Internal.
pub const PHY_INTERFACE_MODE_INTERNAL: u32 = 1;
/// MII (10/100 Mbps, 4-bit data).
pub const PHY_INTERFACE_MODE_MII: u32 = 2;
/// GMII (10/100/1000 Mbps, 8-bit data).
pub const PHY_INTERFACE_MODE_GMII: u32 = 3;
/// SGMII (serial gigabit).
pub const PHY_INTERFACE_MODE_SGMII: u32 = 4;
/// TBI (ten-bit interface).
pub const PHY_INTERFACE_MODE_TBI: u32 = 5;
/// Rev MII.
pub const PHY_INTERFACE_MODE_REVMII: u32 = 6;
/// RMII (reduced MII).
pub const PHY_INTERFACE_MODE_RMII: u32 = 7;
/// Rev RMII.
pub const PHY_INTERFACE_MODE_REVRMII: u32 = 8;
/// RGMII (reduced gigabit MII).
pub const PHY_INTERFACE_MODE_RGMII: u32 = 9;
/// RGMII with ID (internal delay).
pub const PHY_INTERFACE_MODE_RGMII_ID: u32 = 10;
/// RGMII with RXID.
pub const PHY_INTERFACE_MODE_RGMII_RXID: u32 = 11;
/// RGMII with TXID.
pub const PHY_INTERFACE_MODE_RGMII_TXID: u32 = 12;
/// RTBI.
pub const PHY_INTERFACE_MODE_RTBI: u32 = 13;
/// SMII.
pub const PHY_INTERFACE_MODE_SMII: u32 = 14;
/// XGMII (10 Gbps MII).
pub const PHY_INTERFACE_MODE_XGMII: u32 = 15;
/// XLGMII (40 Gbps).
pub const PHY_INTERFACE_MODE_XLGMII: u32 = 16;
/// MOCA (Multimedia over Coax).
pub const PHY_INTERFACE_MODE_MOCA: u32 = 17;
/// QSGMII (quad SGMII).
pub const PHY_INTERFACE_MODE_QSGMII: u32 = 18;
/// TRGMII.
pub const PHY_INTERFACE_MODE_TRGMII: u32 = 19;
/// 100Base-X.
pub const PHY_INTERFACE_MODE_100BASEX: u32 = 20;
/// 1000Base-X.
pub const PHY_INTERFACE_MODE_1000BASEX: u32 = 21;
/// 2500Base-X.
pub const PHY_INTERFACE_MODE_2500BASEX: u32 = 22;
/// 5GBase-R.
pub const PHY_INTERFACE_MODE_5GBASER: u32 = 23;
/// RXAUI.
pub const PHY_INTERFACE_MODE_RXAUI: u32 = 24;
/// XAUI.
pub const PHY_INTERFACE_MODE_XAUI: u32 = 25;
/// 10GBase-KR.
pub const PHY_INTERFACE_MODE_10GBASER: u32 = 26;
/// USXGMII.
pub const PHY_INTERFACE_MODE_USXGMII: u32 = 27;
/// 10GBase-KR.
pub const PHY_INTERFACE_MODE_10GKR: u32 = 28;
/// 25GBase-R.
pub const PHY_INTERFACE_MODE_25GBASER: u32 = 29;

/// Maximum interface mode value.
pub const PHY_INTERFACE_MODE_MAX: u32 = 30;

// ---------------------------------------------------------------------------
// PHY state machine states
// ---------------------------------------------------------------------------

/// PHY device is down.
pub const PHY_DOWN: u32 = 0;
/// PHY device is ready (initialized).
pub const PHY_READY: u32 = 1;
/// PHY is halted (powered down).
pub const PHY_HALTED: u32 = 2;
/// PHY is up (link may not be up).
pub const PHY_UP: u32 = 3;
/// PHY is running (link is up).
pub const PHY_RUNNING: u32 = 4;
/// No link.
pub const PHY_NOLINK: u32 = 5;
/// Cable test.
pub const PHY_CABLETEST: u32 = 6;

// ---------------------------------------------------------------------------
// Link speeds
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
/// 25 Gbps.
pub const SPEED_25000: u32 = 25000;
/// 40 Gbps.
pub const SPEED_40000: u32 = 40000;
/// 50 Gbps.
pub const SPEED_50000: u32 = 50000;
/// 100 Gbps.
pub const SPEED_100000: u32 = 100000;
/// Unknown speed.
pub const SPEED_UNKNOWN: u32 = 0xFFFF_FFFF;

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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_modes_distinct() {
        let modes = [
            PHY_INTERFACE_MODE_NA,
            PHY_INTERFACE_MODE_INTERNAL,
            PHY_INTERFACE_MODE_MII,
            PHY_INTERFACE_MODE_GMII,
            PHY_INTERFACE_MODE_SGMII,
            PHY_INTERFACE_MODE_TBI,
            PHY_INTERFACE_MODE_RMII,
            PHY_INTERFACE_MODE_RGMII,
            PHY_INTERFACE_MODE_RGMII_ID,
            PHY_INTERFACE_MODE_RGMII_RXID,
            PHY_INTERFACE_MODE_RGMII_TXID,
            PHY_INTERFACE_MODE_XGMII,
            PHY_INTERFACE_MODE_QSGMII,
            PHY_INTERFACE_MODE_1000BASEX,
            PHY_INTERFACE_MODE_2500BASEX,
            PHY_INTERFACE_MODE_10GBASER,
            PHY_INTERFACE_MODE_USXGMII,
            PHY_INTERFACE_MODE_25GBASER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            PHY_DOWN,
            PHY_READY,
            PHY_HALTED,
            PHY_UP,
            PHY_RUNNING,
            PHY_NOLINK,
            PHY_CABLETEST,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_speeds_distinct() {
        let speeds = [
            SPEED_10,
            SPEED_100,
            SPEED_1000,
            SPEED_2500,
            SPEED_5000,
            SPEED_10000,
            SPEED_25000,
            SPEED_40000,
            SPEED_50000,
            SPEED_100000,
            SPEED_UNKNOWN,
        ];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }

    #[test]
    fn test_duplex_values() {
        assert_eq!(DUPLEX_HALF, 0);
        assert_eq!(DUPLEX_FULL, 1);
        assert_eq!(DUPLEX_UNKNOWN, 0xFF);
    }

    #[test]
    fn test_interface_mode_max() {
        assert!(PHY_INTERFACE_MODE_25GBASER < PHY_INTERFACE_MODE_MAX);
    }
}
