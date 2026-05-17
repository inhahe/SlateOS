//! `<linux/usb/typec.h>` — USB Type-C connector constants.
//!
//! USB Type-C introduces a reversible connector with power delivery
//! negotiation, alternate modes (DisplayPort, Thunderbolt), and
//! role swapping. The Linux typec subsystem exposes connector state
//! and manages PD (Power Delivery) contracts.

// ---------------------------------------------------------------------------
// Port types
// ---------------------------------------------------------------------------

/// Sink-only port (device/UFP).
pub const TYPEC_PORT_SRC: u8 = 0;
/// Source-only port (host/DFP).
pub const TYPEC_PORT_SNK: u8 = 1;
/// Dual-role port (DRP).
pub const TYPEC_PORT_DRP: u8 = 2;

// ---------------------------------------------------------------------------
// Data roles
// ---------------------------------------------------------------------------

/// Upstream Facing Port (device).
pub const TYPEC_UFP: u8 = 0;
/// Downstream Facing Port (host).
pub const TYPEC_DFP: u8 = 1;

// ---------------------------------------------------------------------------
// Power roles
// ---------------------------------------------------------------------------

/// Sink (consuming power).
pub const TYPEC_SINK: u8 = 0;
/// Source (providing power).
pub const TYPEC_SOURCE: u8 = 1;

// ---------------------------------------------------------------------------
// Orientation
// ---------------------------------------------------------------------------

/// Orientation unknown / not applicable.
pub const TYPEC_ORIENTATION_NONE: u8 = 0;
/// Normal orientation.
pub const TYPEC_ORIENTATION_NORMAL: u8 = 1;
/// Reversed orientation.
pub const TYPEC_ORIENTATION_REVERSE: u8 = 2;

// ---------------------------------------------------------------------------
// Accessory modes
// ---------------------------------------------------------------------------

/// No accessory.
pub const TYPEC_ACCESSORY_NONE: u8 = 0;
/// Audio accessory.
pub const TYPEC_ACCESSORY_AUDIO: u8 = 1;
/// Debug accessory.
pub const TYPEC_ACCESSORY_DEBUG: u8 = 2;

// ---------------------------------------------------------------------------
// Power Delivery revision
// ---------------------------------------------------------------------------

/// PD revision 1.0.
pub const TYPEC_PD_REV10: u8 = 0;
/// PD revision 2.0.
pub const TYPEC_PD_REV20: u8 = 1;
/// PD revision 3.0.
pub const TYPEC_PD_REV30: u8 = 2;
/// PD revision 3.1.
pub const TYPEC_PD_REV31: u8 = 3;

// ---------------------------------------------------------------------------
// Power operation modes (CC pin advertisement)
// ---------------------------------------------------------------------------

/// USB default (500mA / 900mA).
pub const TYPEC_PWR_MODE_USB: u8 = 0;
/// 1.5A at 5V.
pub const TYPEC_PWR_MODE_1_5A: u8 = 1;
/// 3.0A at 5V.
pub const TYPEC_PWR_MODE_3_0A: u8 = 2;
/// Power Delivery negotiated.
pub const TYPEC_PWR_MODE_PD: u8 = 3;

// ---------------------------------------------------------------------------
// Alternate mode SVIDs
// ---------------------------------------------------------------------------

/// DisplayPort alternate mode (VESA).
pub const TYPEC_SVID_DISPLAYPORT: u16 = 0xFF01;
/// Thunderbolt alternate mode (Intel).
pub const TYPEC_SVID_THUNDERBOLT: u16 = 0x8087;
/// USB4 (USB-IF).
pub const TYPEC_SVID_USB4: u16 = 0x8000;

// ---------------------------------------------------------------------------
// Cable types
// ---------------------------------------------------------------------------

/// Passive cable.
pub const TYPEC_CABLE_PASSIVE: u8 = 0;
/// Active cable.
pub const TYPEC_CABLE_ACTIVE: u8 = 1;

// ---------------------------------------------------------------------------
// Cable plug type
// ---------------------------------------------------------------------------

/// Type-A plug.
pub const TYPEC_PLUG_TYPE_A: u8 = 0;
/// Type-B plug.
pub const TYPEC_PLUG_TYPE_B: u8 = 1;
/// Type-C plug.
pub const TYPEC_PLUG_TYPE_C: u8 = 2;
/// Captive cable.
pub const TYPEC_PLUG_CAPTIVE: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_types_distinct() {
        let ports = [TYPEC_PORT_SRC, TYPEC_PORT_SNK, TYPEC_PORT_DRP];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j]);
            }
        }
    }

    #[test]
    fn test_data_roles_distinct() {
        assert_ne!(TYPEC_UFP, TYPEC_DFP);
    }

    #[test]
    fn test_power_roles_distinct() {
        assert_ne!(TYPEC_SINK, TYPEC_SOURCE);
    }

    #[test]
    fn test_orientations_distinct() {
        let orients = [
            TYPEC_ORIENTATION_NONE, TYPEC_ORIENTATION_NORMAL,
            TYPEC_ORIENTATION_REVERSE,
        ];
        for i in 0..orients.len() {
            for j in (i + 1)..orients.len() {
                assert_ne!(orients[i], orients[j]);
            }
        }
    }

    #[test]
    fn test_accessory_modes_distinct() {
        let modes = [
            TYPEC_ACCESSORY_NONE, TYPEC_ACCESSORY_AUDIO,
            TYPEC_ACCESSORY_DEBUG,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_pd_revisions_distinct() {
        let revs = [
            TYPEC_PD_REV10, TYPEC_PD_REV20,
            TYPEC_PD_REV30, TYPEC_PD_REV31,
        ];
        for i in 0..revs.len() {
            for j in (i + 1)..revs.len() {
                assert_ne!(revs[i], revs[j]);
            }
        }
    }

    #[test]
    fn test_power_modes_distinct() {
        let modes = [
            TYPEC_PWR_MODE_USB, TYPEC_PWR_MODE_1_5A,
            TYPEC_PWR_MODE_3_0A, TYPEC_PWR_MODE_PD,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_svids_distinct() {
        let svids = [TYPEC_SVID_DISPLAYPORT, TYPEC_SVID_THUNDERBOLT, TYPEC_SVID_USB4];
        for i in 0..svids.len() {
            for j in (i + 1)..svids.len() {
                assert_ne!(svids[i], svids[j]);
            }
        }
    }

    #[test]
    fn test_cable_types_distinct() {
        assert_ne!(TYPEC_CABLE_PASSIVE, TYPEC_CABLE_ACTIVE);
    }

    #[test]
    fn test_plug_types_distinct() {
        let plugs = [
            TYPEC_PLUG_TYPE_A, TYPEC_PLUG_TYPE_B,
            TYPEC_PLUG_TYPE_C, TYPEC_PLUG_CAPTIVE,
        ];
        for i in 0..plugs.len() {
            for j in (i + 1)..plugs.len() {
                assert_ne!(plugs[i], plugs[j]);
            }
        }
    }
}
