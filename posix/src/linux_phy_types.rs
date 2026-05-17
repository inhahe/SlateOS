//! `<linux/phy.h>` — Ethernet PHY (Physical Layer) constants.
//!
//! Ethernet PHYs handle the physical signaling on the wire: encoding,
//! clock recovery, autonegotiation, and link status. The Linux PHY
//! framework abstracts differences between PHY chips behind a common
//! MDIO-accessed register model.

// ---------------------------------------------------------------------------
// PHY interface modes (phy_interface_t)
// ---------------------------------------------------------------------------

/// Internal (no external PHY).
pub const PHY_INTERFACE_MODE_INTERNAL: u8 = 0;
/// MII (10/100 Mbps).
pub const PHY_INTERFACE_MODE_MII: u8 = 1;
/// GMII (10/100/1000 Mbps).
pub const PHY_INTERFACE_MODE_GMII: u8 = 2;
/// SGMII (serial GMII).
pub const PHY_INTERFACE_MODE_SGMII: u8 = 3;
/// RGMII (Reduced GMII).
pub const PHY_INTERFACE_MODE_RGMII: u8 = 4;
/// RGMII with internal TX delay.
pub const PHY_INTERFACE_MODE_RGMII_TXID: u8 = 5;
/// RGMII with internal RX delay.
pub const PHY_INTERFACE_MODE_RGMII_RXID: u8 = 6;
/// RGMII with internal TX+RX delay.
pub const PHY_INTERFACE_MODE_RGMII_ID: u8 = 7;
/// RMII (Reduced MII).
pub const PHY_INTERFACE_MODE_RMII: u8 = 8;
/// QSGMII (Quad SGMII).
pub const PHY_INTERFACE_MODE_QSGMII: u8 = 9;
/// XGMII (10 Gbps).
pub const PHY_INTERFACE_MODE_XGMII: u8 = 10;
/// USXGMII (universal serial 10G).
pub const PHY_INTERFACE_MODE_USXGMII: u8 = 11;
/// 10GBASE-R.
pub const PHY_INTERFACE_MODE_10GBASER: u8 = 12;
/// 25GBASE-R.
pub const PHY_INTERFACE_MODE_25GBASER: u8 = 13;

// ---------------------------------------------------------------------------
// PHY speed constants
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
/// 100 Gbps.
pub const SPEED_100000: u32 = 100000;
/// Unknown speed.
pub const SPEED_UNKNOWN: u32 = 0xFFFFFFFF;

// ---------------------------------------------------------------------------
// PHY duplex modes
// ---------------------------------------------------------------------------

/// Half duplex.
pub const DUPLEX_HALF: u8 = 0;
/// Full duplex.
pub const DUPLEX_FULL: u8 = 1;
/// Unknown duplex.
pub const DUPLEX_UNKNOWN: u8 = 0xFF;

// ---------------------------------------------------------------------------
// PHY states
// ---------------------------------------------------------------------------

/// PHY device down.
pub const PHY_DOWN: u8 = 0;
/// PHY ready (configured, not started).
pub const PHY_READY: u8 = 1;
/// PHY halted (forced down).
pub const PHY_HALTED: u8 = 2;
/// PHY running (link monitoring active).
pub const PHY_RUNNING: u8 = 3;
/// PHY no link.
pub const PHY_NOLINK: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_modes_distinct() {
        let modes = [
            PHY_INTERFACE_MODE_INTERNAL, PHY_INTERFACE_MODE_MII,
            PHY_INTERFACE_MODE_GMII, PHY_INTERFACE_MODE_SGMII,
            PHY_INTERFACE_MODE_RGMII, PHY_INTERFACE_MODE_RGMII_TXID,
            PHY_INTERFACE_MODE_RGMII_RXID, PHY_INTERFACE_MODE_RGMII_ID,
            PHY_INTERFACE_MODE_RMII, PHY_INTERFACE_MODE_QSGMII,
            PHY_INTERFACE_MODE_XGMII, PHY_INTERFACE_MODE_USXGMII,
            PHY_INTERFACE_MODE_10GBASER, PHY_INTERFACE_MODE_25GBASER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_speeds_increasing() {
        let speeds = [
            SPEED_10, SPEED_100, SPEED_1000, SPEED_2500,
            SPEED_5000, SPEED_10000, SPEED_25000, SPEED_40000,
            SPEED_100000,
        ];
        for i in 1..speeds.len() {
            assert!(speeds[i] > speeds[i - 1]);
        }
    }

    #[test]
    fn test_duplex_distinct() {
        let dups = [DUPLEX_HALF, DUPLEX_FULL, DUPLEX_UNKNOWN];
        for i in 0..dups.len() {
            for j in (i + 1)..dups.len() {
                assert_ne!(dups[i], dups[j]);
            }
        }
    }

    #[test]
    fn test_phy_states_distinct() {
        let states = [PHY_DOWN, PHY_READY, PHY_HALTED, PHY_RUNNING, PHY_NOLINK];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
