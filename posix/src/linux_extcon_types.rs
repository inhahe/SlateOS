//! `<linux/extcon.h>` — External connector (extcon) constants.
//!
//! The extcon framework manages external connectors that can carry
//! different signals. A single USB Type-C port can function as
//! USB host, USB device, DisplayPort, or charger. The extcon
//! subsystem detects what's connected, notifies interested drivers
//! (USB, display, charger), and coordinates role switching. It
//! provides cable state to consumer drivers via notifiers.

// ---------------------------------------------------------------------------
// External connector types
// ---------------------------------------------------------------------------

/// USB connector (generic).
pub const EXTCON_USB: u32 = 1;
/// USB host mode (Type-A behavior).
pub const EXTCON_USB_HOST: u32 = 2;
/// Charging port detected.
pub const EXTCON_CHG_USB_SDP: u32 = 3;
/// USB charging downstream port.
pub const EXTCON_CHG_USB_DCP: u32 = 4;
/// USB charging port (CDP).
pub const EXTCON_CHG_USB_CDP: u32 = 5;
/// Slow charger (non-standard).
pub const EXTCON_CHG_USB_SLOW: u32 = 10;
/// Fast charger (proprietary).
pub const EXTCON_CHG_USB_FAST: u32 = 11;
/// HDMI cable connected.
pub const EXTCON_DISP_HDMI: u32 = 20;
/// DisplayPort connected.
pub const EXTCON_DISP_DP: u32 = 21;
/// MHL (Mobile High-Definition Link) connected.
pub const EXTCON_DISP_MHL: u32 = 22;
/// VGA connected.
pub const EXTCON_DISP_VGA: u32 = 23;
/// 3.5mm headphone jack connected.
pub const EXTCON_JACK_HEADPHONE: u32 = 30;
/// Microphone connected.
pub const EXTCON_JACK_MICROPHONE: u32 = 31;
/// Line out connected.
pub const EXTCON_JACK_LINE_OUT: u32 = 32;
/// Dock connected.
pub const EXTCON_DOCK: u32 = 40;

// ---------------------------------------------------------------------------
// Extcon property types (per-cable properties)
// ---------------------------------------------------------------------------

/// USB VBUS supply (mV).
pub const EXTCON_PROP_USB_VBUS: u32 = 0;
/// USB data role (0=device, 1=host).
pub const EXTCON_PROP_USB_DATA_ROLE: u32 = 1;
/// USB Type-C orientation (0=normal, 1=flipped).
pub const EXTCON_PROP_USB_TYPEC_POLARITY: u32 = 2;
/// Display HPD (hot plug detect) state.
pub const EXTCON_PROP_DISP_HPD: u32 = 10;

// ---------------------------------------------------------------------------
// Extcon cable states
// ---------------------------------------------------------------------------

/// Cable is disconnected.
pub const EXTCON_STATE_DISCONNECTED: u32 = 0;
/// Cable is connected.
pub const EXTCON_STATE_CONNECTED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_types_distinct() {
        let types = [
            EXTCON_USB, EXTCON_USB_HOST, EXTCON_CHG_USB_SDP,
            EXTCON_CHG_USB_DCP, EXTCON_CHG_USB_CDP,
            EXTCON_CHG_USB_SLOW, EXTCON_CHG_USB_FAST,
            EXTCON_DISP_HDMI, EXTCON_DISP_DP,
            EXTCON_DISP_MHL, EXTCON_DISP_VGA,
            EXTCON_JACK_HEADPHONE, EXTCON_JACK_MICROPHONE,
            EXTCON_JACK_LINE_OUT, EXTCON_DOCK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_properties_distinct() {
        let props = [
            EXTCON_PROP_USB_VBUS, EXTCON_PROP_USB_DATA_ROLE,
            EXTCON_PROP_USB_TYPEC_POLARITY, EXTCON_PROP_DISP_HPD,
        ];
        for i in 0..props.len() {
            for j in (i + 1)..props.len() {
                assert_ne!(props[i], props[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(EXTCON_STATE_DISCONNECTED, EXTCON_STATE_CONNECTED);
    }
}
