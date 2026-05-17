//! `<drm/drm_mode.h>` — DRM mode setting (KMS) constants.
//!
//! Kernel Mode Setting (KMS) manages display output: connectors,
//! CRTCs, planes, encoders, and framebuffers. These constants
//! define the mode/connector/plane types and properties used by
//! DRM userspace (compositor, Xorg, Wayland).

// ---------------------------------------------------------------------------
// Connector types
// ---------------------------------------------------------------------------

/// Unknown connector.
#[allow(non_upper_case_globals)]
pub const DRM_MODE_CONNECTOR_Unknown: u32 = 0;
/// VGA connector.
pub const DRM_MODE_CONNECTOR_VGA: u32 = 1;
/// DVI-I connector.
pub const DRM_MODE_CONNECTOR_DVII: u32 = 2;
/// DVI-D connector.
pub const DRM_MODE_CONNECTOR_DVID: u32 = 3;
/// DVI-A connector.
pub const DRM_MODE_CONNECTOR_DVIA: u32 = 4;
/// Composite video.
#[allow(non_upper_case_globals)]
pub const DRM_MODE_CONNECTOR_Composite: u32 = 5;
/// S-Video.
pub const DRM_MODE_CONNECTOR_SVIDEO: u32 = 6;
/// LVDS (laptop panel).
pub const DRM_MODE_CONNECTOR_LVDS: u32 = 7;
/// Component video.
#[allow(non_upper_case_globals)]
pub const DRM_MODE_CONNECTOR_Component: u32 = 8;
/// DisplayPort.
#[allow(non_upper_case_globals)]
pub const DRM_MODE_CONNECTOR_DisplayPort: u32 = 10;
/// HDMI-A.
pub const DRM_MODE_CONNECTOR_HDMIA: u32 = 11;
/// HDMI-B.
pub const DRM_MODE_CONNECTOR_HDMIB: u32 = 12;
/// eDP (embedded DisplayPort).
#[allow(non_upper_case_globals)]
pub const DRM_MODE_CONNECTOR_eDP: u32 = 14;
/// Virtual (e.g., for remote display).
pub const DRM_MODE_CONNECTOR_VIRTUAL: u32 = 15;
/// DSI (MIPI Display Serial Interface).
pub const DRM_MODE_CONNECTOR_DSI: u32 = 16;
/// USB Type-C DisplayPort.
pub const DRM_MODE_CONNECTOR_USB: u32 = 19;

// ---------------------------------------------------------------------------
// Connector status
// ---------------------------------------------------------------------------

/// Connector has a display attached.
pub const DRM_MODE_CONNECTED: u32 = 1;
/// No display attached.
pub const DRM_MODE_DISCONNECTED: u32 = 2;
/// Cannot determine status.
pub const DRM_MODE_UNKNOWNCONNECTION: u32 = 3;

// ---------------------------------------------------------------------------
// Plane types
// ---------------------------------------------------------------------------

/// Overlay plane.
pub const DRM_PLANE_TYPE_OVERLAY: u32 = 0;
/// Primary plane (main scanout).
pub const DRM_PLANE_TYPE_PRIMARY: u32 = 1;
/// Cursor plane.
pub const DRM_PLANE_TYPE_CURSOR: u32 = 2;

// ---------------------------------------------------------------------------
// Mode flags
// ---------------------------------------------------------------------------

/// Positive horizontal sync.
pub const DRM_MODE_FLAG_PHSYNC: u32 = 1 << 0;
/// Negative horizontal sync.
pub const DRM_MODE_FLAG_NHSYNC: u32 = 1 << 1;
/// Positive vertical sync.
pub const DRM_MODE_FLAG_PVSYNC: u32 = 1 << 2;
/// Negative vertical sync.
pub const DRM_MODE_FLAG_NVSYNC: u32 = 1 << 3;
/// Interlaced mode.
pub const DRM_MODE_FLAG_INTERLACE: u32 = 1 << 4;
/// Double scan.
pub const DRM_MODE_FLAG_DBLSCAN: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Page flip flags
// ---------------------------------------------------------------------------

/// Request flip event on completion.
pub const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
/// Flip asynchronously (no vsync).
pub const DRM_MODE_PAGE_FLIP_ASYNC: u32 = 0x02;

// ---------------------------------------------------------------------------
// Atomic flags
// ---------------------------------------------------------------------------

/// Test only (don't apply changes).
pub const DRM_MODE_ATOMIC_TEST_ONLY: u32 = 0x0100;
/// Non-blocking commit.
pub const DRM_MODE_ATOMIC_NONBLOCK: u32 = 0x0200;
/// Allow modeset (mode change) in atomic commit.
pub const DRM_MODE_ATOMIC_ALLOW_MODESET: u32 = 0x0400;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_types_distinct() {
        let types = [
            DRM_MODE_CONNECTOR_Unknown, DRM_MODE_CONNECTOR_VGA,
            DRM_MODE_CONNECTOR_DVII, DRM_MODE_CONNECTOR_DVID,
            DRM_MODE_CONNECTOR_DVIA, DRM_MODE_CONNECTOR_Composite,
            DRM_MODE_CONNECTOR_SVIDEO, DRM_MODE_CONNECTOR_LVDS,
            DRM_MODE_CONNECTOR_Component, DRM_MODE_CONNECTOR_DisplayPort,
            DRM_MODE_CONNECTOR_HDMIA, DRM_MODE_CONNECTOR_HDMIB,
            DRM_MODE_CONNECTOR_eDP, DRM_MODE_CONNECTOR_VIRTUAL,
            DRM_MODE_CONNECTOR_DSI, DRM_MODE_CONNECTOR_USB,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_connector_status_distinct() {
        let statuses = [DRM_MODE_CONNECTED, DRM_MODE_DISCONNECTED, DRM_MODE_UNKNOWNCONNECTION];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_plane_types_distinct() {
        let types = [DRM_PLANE_TYPE_OVERLAY, DRM_PLANE_TYPE_PRIMARY, DRM_PLANE_TYPE_CURSOR];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_mode_flags_powers_of_two() {
        let flags = [
            DRM_MODE_FLAG_PHSYNC, DRM_MODE_FLAG_NHSYNC,
            DRM_MODE_FLAG_PVSYNC, DRM_MODE_FLAG_NVSYNC,
            DRM_MODE_FLAG_INTERLACE, DRM_MODE_FLAG_DBLSCAN,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_page_flip_flags_distinct() {
        assert_ne!(DRM_MODE_PAGE_FLIP_EVENT, DRM_MODE_PAGE_FLIP_ASYNC);
    }

    #[test]
    fn test_atomic_flags_distinct() {
        let flags = [
            DRM_MODE_ATOMIC_TEST_ONLY,
            DRM_MODE_ATOMIC_NONBLOCK,
            DRM_MODE_ATOMIC_ALLOW_MODESET,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
