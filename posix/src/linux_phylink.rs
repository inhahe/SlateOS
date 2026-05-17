//! `<linux/phylink.h>` — PHY link state machine constants.
//!
//! Phylink sits between the MAC driver and the PHY driver, managing
//! link state transitions, in-band signaling (SGMII/1000BASE-X),
//! and SFP module hotplug. It handles the complexity of MAC-to-PHY
//! configuration that varies by interface mode.

// ---------------------------------------------------------------------------
// Link modes
// ---------------------------------------------------------------------------

/// PHY managed link (external PHY via MDIO).
pub const MLO_AN_PHY: u8 = 0;
/// Fixed link (no PHY, parameters hardcoded).
pub const MLO_AN_FIXED: u8 = 1;
/// In-band autonegotiation (SGMII/1000BASE-X).
pub const MLO_AN_INBAND: u8 = 2;

// ---------------------------------------------------------------------------
// Link state events
// ---------------------------------------------------------------------------

/// Link is down.
pub const PHYLINK_LINK_DOWN: u8 = 0;
/// Link is up.
pub const PHYLINK_LINK_UP: u8 = 1;

// ---------------------------------------------------------------------------
// PCS (Physical Coding Sublayer) modes
// ---------------------------------------------------------------------------

/// PCS provides in-band status (SGMII/802.3z word).
pub const PHYLINK_PCS_INBAND: u8 = 0;
/// PCS negotiation uses NBASE-T.
pub const PHYLINK_PCS_NBASET: u8 = 1;

// ---------------------------------------------------------------------------
// MAC capability flags
// ---------------------------------------------------------------------------

/// Supports 10 Mbps.
pub const MAC_10: u32 = 1 << 0;
/// Supports 100 Mbps.
pub const MAC_100: u32 = 1 << 1;
/// Supports 1000 Mbps.
pub const MAC_1000: u32 = 1 << 2;
/// Supports 2500 Mbps.
pub const MAC_2500: u32 = 1 << 3;
/// Supports 5000 Mbps.
pub const MAC_5000: u32 = 1 << 4;
/// Supports 10000 Mbps.
pub const MAC_10000: u32 = 1 << 5;
/// Supports half duplex.
pub const MAC_HALF_DUPLEX: u32 = 1 << 8;
/// Supports full duplex.
pub const MAC_FULL_DUPLEX: u32 = 1 << 9;
/// Supports pause frames.
pub const MAC_SYM_PAUSE: u32 = 1 << 10;
/// Supports asymmetric pause.
pub const MAC_ASYM_PAUSE: u32 = 1 << 11;

// ---------------------------------------------------------------------------
// Phylink configuration flags
// ---------------------------------------------------------------------------

/// Permit pause mode changes.
pub const PHYLINK_F_PAUSE: u32 = 1 << 0;
/// Permit EEE (Energy Efficient Ethernet).
pub const PHYLINK_F_EEE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_an_modes_distinct() {
        let modes = [MLO_AN_PHY, MLO_AN_FIXED, MLO_AN_INBAND];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_link_states_distinct() {
        assert_ne!(PHYLINK_LINK_DOWN, PHYLINK_LINK_UP);
    }

    #[test]
    fn test_mac_speed_caps_no_overlap() {
        let caps = [MAC_10, MAC_100, MAC_1000, MAC_2500, MAC_5000, MAC_10000];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_mac_caps_power_of_two() {
        let caps = [
            MAC_10, MAC_100, MAC_1000, MAC_2500, MAC_5000, MAC_10000,
            MAC_HALF_DUPLEX, MAC_FULL_DUPLEX, MAC_SYM_PAUSE, MAC_ASYM_PAUSE,
        ];
        for c in &caps {
            assert!(c.is_power_of_two());
        }
    }
}
