//! `<drm/drm_mode.h>` (connector subset) — DRM connector type constants.
//!
//! A DRM connector represents a physical display output port (HDMI,
//! DisplayPort, VGA, etc.). Connectors have a type, a connection status
//! (connected/disconnected/unknown), and associated properties (EDID,
//! DPMS state, audio support). Hotplug detection monitors connectors
//! for cable insertion/removal events.

// ---------------------------------------------------------------------------
// Connector types
// ---------------------------------------------------------------------------

/// Unknown connector type.
pub const DRM_MODE_CONNECTOR_UNKNOWN: u32 = 0;
/// VGA (D-Sub 15-pin).
pub const DRM_MODE_CONNECTOR_VGA: u32 = 1;
/// DVI-I (integrated analog + digital).
pub const DRM_MODE_CONNECTOR_DVII: u32 = 2;
/// DVI-D (digital only).
pub const DRM_MODE_CONNECTOR_DVID: u32 = 3;
/// DVI-A (analog only).
pub const DRM_MODE_CONNECTOR_DVIA: u32 = 4;
/// Composite video.
pub const DRM_MODE_CONNECTOR_COMPOSITE: u32 = 5;
/// S-Video.
pub const DRM_MODE_CONNECTOR_SVIDEO: u32 = 6;
/// LVDS (laptop internal panel).
pub const DRM_MODE_CONNECTOR_LVDS: u32 = 7;
/// Component video.
pub const DRM_MODE_CONNECTOR_COMPONENT: u32 = 8;
/// 9-pin DIN.
pub const DRM_MODE_CONNECTOR_9PINDIN: u32 = 9;
/// DisplayPort.
pub const DRM_MODE_CONNECTOR_DISPLAYPORT: u32 = 10;
/// HDMI Type A.
pub const DRM_MODE_CONNECTOR_HDMIA: u32 = 11;
/// HDMI Type B.
pub const DRM_MODE_CONNECTOR_HDMIB: u32 = 12;
/// TV output.
pub const DRM_MODE_CONNECTOR_TV: u32 = 13;
/// eDP (embedded DisplayPort, laptop panels).
pub const DRM_MODE_CONNECTOR_EDP: u32 = 14;
/// Virtual (for virtual displays, VMs).
pub const DRM_MODE_CONNECTOR_VIRTUAL: u32 = 15;
/// DSI (Display Serial Interface, mobile/embedded).
pub const DRM_MODE_CONNECTOR_DSI: u32 = 16;
/// DPI (Display Pixel Interface).
pub const DRM_MODE_CONNECTOR_DPI: u32 = 17;
/// USB Type-C DisplayPort alt mode.
pub const DRM_MODE_CONNECTOR_USB: u32 = 19;

// ---------------------------------------------------------------------------
// Connection status
// ---------------------------------------------------------------------------

/// Connector has a display connected.
pub const DRM_MODE_CONNECTED: u32 = 1;
/// Connector has no display connected.
pub const DRM_MODE_DISCONNECTED: u32 = 2;
/// Connection status unknown (hotplug not supported).
pub const DRM_MODE_UNKNOWNCONNECTION: u32 = 3;

// ---------------------------------------------------------------------------
// DPMS (Display Power Management Signaling) states
// ---------------------------------------------------------------------------

/// Display on.
pub const DRM_MODE_DPMS_ON: u32 = 0;
/// Display standby (quick resume).
pub const DRM_MODE_DPMS_STANDBY: u32 = 1;
/// Display suspend (slower resume).
pub const DRM_MODE_DPMS_SUSPEND: u32 = 2;
/// Display off (signal removed).
pub const DRM_MODE_DPMS_OFF: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_types_distinct() {
        let types = [
            DRM_MODE_CONNECTOR_UNKNOWN, DRM_MODE_CONNECTOR_VGA,
            DRM_MODE_CONNECTOR_DVII, DRM_MODE_CONNECTOR_DVID,
            DRM_MODE_CONNECTOR_DVIA, DRM_MODE_CONNECTOR_COMPOSITE,
            DRM_MODE_CONNECTOR_SVIDEO, DRM_MODE_CONNECTOR_LVDS,
            DRM_MODE_CONNECTOR_COMPONENT, DRM_MODE_CONNECTOR_9PINDIN,
            DRM_MODE_CONNECTOR_DISPLAYPORT, DRM_MODE_CONNECTOR_HDMIA,
            DRM_MODE_CONNECTOR_HDMIB, DRM_MODE_CONNECTOR_TV,
            DRM_MODE_CONNECTOR_EDP, DRM_MODE_CONNECTOR_VIRTUAL,
            DRM_MODE_CONNECTOR_DSI, DRM_MODE_CONNECTOR_DPI,
            DRM_MODE_CONNECTOR_USB,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_connection_status_distinct() {
        let states = [
            DRM_MODE_CONNECTED, DRM_MODE_DISCONNECTED,
            DRM_MODE_UNKNOWNCONNECTION,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_dpms_states_ordered() {
        assert!(DRM_MODE_DPMS_ON < DRM_MODE_DPMS_STANDBY);
        assert!(DRM_MODE_DPMS_STANDBY < DRM_MODE_DPMS_SUSPEND);
        assert!(DRM_MODE_DPMS_SUSPEND < DRM_MODE_DPMS_OFF);
    }
}
