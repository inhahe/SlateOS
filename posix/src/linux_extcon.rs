//! `<linux/extcon.h>` — External connector (extcon) subsystem constants.
//!
//! The extcon subsystem detects and reports the state of external
//! connectors: USB cables, HDMI, docking stations, headphones, etc.
//! It's used by charge controllers, audio subsystems, and display
//! managers to adapt behavior based on what's physically connected.

// ---------------------------------------------------------------------------
// External connector types
// ---------------------------------------------------------------------------

/// USB host connector.
pub const EXTCON_USB_HOST: u32 = 1;
/// USB device connector.
pub const EXTCON_USB: u32 = 2;
/// Charging downstream port (USB charger).
pub const EXTCON_CHG_USB_SDP: u32 = 3;
/// Dedicated charging port.
pub const EXTCON_CHG_USB_DCP: u32 = 4;
/// Charging downstream port.
pub const EXTCON_CHG_USB_CDP: u32 = 5;
/// Proprietary charger.
pub const EXTCON_CHG_USB_ACA: u32 = 6;
/// HDMI connector.
pub const EXTCON_DISP_HDMI: u32 = 7;
/// MHL connector.
pub const EXTCON_DISP_MHL: u32 = 8;
/// DisplayPort connector.
pub const EXTCON_DISP_DP: u32 = 9;
/// VGA connector.
pub const EXTCON_DISP_VGA: u32 = 10;
/// Dock connector.
pub const EXTCON_DOCK: u32 = 11;
/// Mechanical insertion (e.g., SD card).
pub const EXTCON_MECHANICAL: u32 = 12;
/// 3.5mm jack (headphones).
pub const EXTCON_JACK_HEADPHONE: u32 = 13;
/// Microphone jack.
pub const EXTCON_JACK_MICROPHONE: u32 = 14;
/// Line-out jack.
pub const EXTCON_JACK_LINE_OUT: u32 = 15;

// ---------------------------------------------------------------------------
// Extcon property IDs (per-cable properties)
// ---------------------------------------------------------------------------

/// USB speed (for USB cables).
pub const EXTCON_PROP_USB_SPEED: u32 = 0;
/// USB type-C orientation.
pub const EXTCON_PROP_USB_TYPEC_POLARITY: u32 = 1;
/// Display HPD (hot plug detect).
pub const EXTCON_PROP_DISP_HPD: u32 = 2;

// ---------------------------------------------------------------------------
// Cable states
// ---------------------------------------------------------------------------

/// Cable disconnected.
pub const EXTCON_STATE_DISCONNECTED: u8 = 0;
/// Cable connected.
pub const EXTCON_STATE_CONNECTED: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_types_distinct() {
        let types = [
            EXTCON_USB_HOST,
            EXTCON_USB,
            EXTCON_CHG_USB_SDP,
            EXTCON_CHG_USB_DCP,
            EXTCON_CHG_USB_CDP,
            EXTCON_CHG_USB_ACA,
            EXTCON_DISP_HDMI,
            EXTCON_DISP_MHL,
            EXTCON_DISP_DP,
            EXTCON_DISP_VGA,
            EXTCON_DOCK,
            EXTCON_MECHANICAL,
            EXTCON_JACK_HEADPHONE,
            EXTCON_JACK_MICROPHONE,
            EXTCON_JACK_LINE_OUT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_property_ids_distinct() {
        let props = [
            EXTCON_PROP_USB_SPEED,
            EXTCON_PROP_USB_TYPEC_POLARITY,
            EXTCON_PROP_DISP_HPD,
        ];
        for i in 0..props.len() {
            for j in (i + 1)..props.len() {
                assert_ne!(props[i], props[j]);
            }
        }
    }

    #[test]
    fn test_states() {
        assert_ne!(EXTCON_STATE_DISCONNECTED, EXTCON_STATE_CONNECTED);
    }
}
