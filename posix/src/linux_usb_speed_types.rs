//! `<linux/usb/ch9.h>` — USB speed and link state constants.
//!
//! USB devices operate at different speeds depending on the protocol
//! version and device capabilities. The host controller and hub
//! negotiate the highest mutually-supported speed during enumeration.
//! Link states control power management of individual ports.

// ---------------------------------------------------------------------------
// USB device speeds
// ---------------------------------------------------------------------------

/// Speed unknown (not yet determined).
pub const USB_SPEED_UNKNOWN: u32 = 0;
/// Low speed (1.5 Mbit/s, USB 1.0 — keyboards, mice).
pub const USB_SPEED_LOW: u32 = 1;
/// Full speed (12 Mbit/s, USB 1.1).
pub const USB_SPEED_FULL: u32 = 2;
/// High speed (480 Mbit/s, USB 2.0).
pub const USB_SPEED_HIGH: u32 = 3;
/// Wireless USB (480 Mbit/s, deprecated).
pub const USB_SPEED_WIRELESS: u32 = 4;
/// SuperSpeed (5 Gbit/s, USB 3.0).
pub const USB_SPEED_SUPER: u32 = 5;
/// SuperSpeed+ (10 Gbit/s, USB 3.1 Gen 2).
pub const USB_SPEED_SUPER_PLUS: u32 = 6;

// ---------------------------------------------------------------------------
// USB link states (LPM - Link Power Management)
// ---------------------------------------------------------------------------

/// U0: Active (link fully operational).
pub const USB_SS_LINK_STATE_U0: u8 = 0;
/// U1: Standby (fast exit, ~10µs).
pub const USB_SS_LINK_STATE_U1: u8 = 1;
/// U2: Sleep (medium exit, ~100µs).
pub const USB_SS_LINK_STATE_U2: u8 = 2;
/// U3: Suspend (slow exit, ~10ms).
pub const USB_SS_LINK_STATE_U3: u8 = 3;
/// SS.Disabled (link disabled).
pub const USB_SS_LINK_STATE_SS_DISABLED: u8 = 4;
/// Rx.Detect (looking for partner).
pub const USB_SS_LINK_STATE_RX_DETECT: u8 = 5;
/// SS.Inactive (error state).
pub const USB_SS_LINK_STATE_SS_INACTIVE: u8 = 6;
/// Compliance mode (testing).
pub const USB_SS_LINK_STATE_COMPLIANCE: u8 = 10;
/// Loopback mode (testing).
pub const USB_SS_LINK_STATE_LOOPBACK: u8 = 11;

// ---------------------------------------------------------------------------
// USB 2.0 LPM (Link Power Management) states
// ---------------------------------------------------------------------------

/// L0: Active.
pub const USB2_LPM_L0: u8 = 0;
/// L1: Sleep (LPM-capable devices only).
pub const USB2_LPM_L1: u8 = 1;
/// L2: Suspend (classic USB suspend).
pub const USB2_LPM_L2: u8 = 2;
/// L3: Powered off.
pub const USB2_LPM_L3: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speeds_sequential() {
        assert_eq!(USB_SPEED_UNKNOWN, 0);
        assert_eq!(USB_SPEED_LOW, 1);
        assert_eq!(USB_SPEED_FULL, 2);
        assert_eq!(USB_SPEED_HIGH, 3);
        assert_eq!(USB_SPEED_SUPER, 5);
        assert_eq!(USB_SPEED_SUPER_PLUS, 6);
    }

    #[test]
    fn test_speeds_ordered() {
        assert!(USB_SPEED_LOW < USB_SPEED_FULL);
        assert!(USB_SPEED_FULL < USB_SPEED_HIGH);
        assert!(USB_SPEED_HIGH < USB_SPEED_SUPER);
        assert!(USB_SPEED_SUPER < USB_SPEED_SUPER_PLUS);
    }

    #[test]
    fn test_ss_link_states_distinct() {
        let states = [
            USB_SS_LINK_STATE_U0,
            USB_SS_LINK_STATE_U1,
            USB_SS_LINK_STATE_U2,
            USB_SS_LINK_STATE_U3,
            USB_SS_LINK_STATE_SS_DISABLED,
            USB_SS_LINK_STATE_RX_DETECT,
            USB_SS_LINK_STATE_SS_INACTIVE,
            USB_SS_LINK_STATE_COMPLIANCE,
            USB_SS_LINK_STATE_LOOPBACK,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_usb2_lpm_sequential() {
        assert_eq!(USB2_LPM_L0, 0);
        assert_eq!(USB2_LPM_L1, 1);
        assert_eq!(USB2_LPM_L2, 2);
        assert_eq!(USB2_LPM_L3, 3);
    }
}
