//! `<drm/drm_mode.h>` (CRTC subset) — DRM CRTC (display controller) constants.
//!
//! A CRTC (CRT Controller, historical name) is a display pipeline
//! endpoint that reads pixel data from planes, applies gamma/color
//! correction, and generates the timed video signal sent to a
//! connector. Each CRTC drives one output at a time (though some
//! hardware supports cloning). Atomic modesetting commits configure
//! all CRTCs, planes, and connectors in a single transaction.

// ---------------------------------------------------------------------------
// CRTC page flip flags
// ---------------------------------------------------------------------------

/// Flip at next vblank (non-blocking).
pub const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
/// Async flip (tear, no vblank wait).
pub const DRM_MODE_PAGE_FLIP_ASYNC: u32 = 0x02;
/// Target specific vblank sequence.
pub const DRM_MODE_PAGE_FLIP_TARGET: u32 = 0x04;

// ---------------------------------------------------------------------------
// Atomic commit flags
// ---------------------------------------------------------------------------

/// Test only (validate but don't commit).
pub const DRM_MODE_ATOMIC_TEST_ONLY: u32 = 0x0100;
/// Non-blocking commit (return immediately).
pub const DRM_MODE_ATOMIC_NONBLOCK: u32 = 0x0200;
/// Allow modeset (mode change, not just page flip).
pub const DRM_MODE_ATOMIC_ALLOW_MODESET: u32 = 0x0400;

// ---------------------------------------------------------------------------
// VBlank event types
// ---------------------------------------------------------------------------

/// VBlank event (vertical blank interrupt).
pub const DRM_EVENT_VBLANK: u32 = 0x01;
/// Page flip complete event.
pub const DRM_EVENT_FLIP_COMPLETE: u32 = 0x02;
/// CRTC sequence event (custom vblank target).
pub const DRM_EVENT_CRTC_SEQUENCE: u32 = 0x03;

// ---------------------------------------------------------------------------
// Color LUT (Look-Up Table) properties
// ---------------------------------------------------------------------------

/// Degamma LUT (linearize input from sRGB).
pub const DRM_COLOR_LUT_DEGAMMA: u32 = 0;
/// CTM (Color Transform Matrix, 3x3).
pub const DRM_COLOR_LUT_CTM: u32 = 1;
/// Gamma LUT (apply output gamma curve).
pub const DRM_COLOR_LUT_GAMMA: u32 = 2;

// ---------------------------------------------------------------------------
// CRTC properties
// ---------------------------------------------------------------------------

/// CRTC is active (displaying).
pub const DRM_CRTC_ACTIVE: u32 = 1;
/// CRTC is inactive (powered down).
pub const DRM_CRTC_INACTIVE: u32 = 0;

// ---------------------------------------------------------------------------
// VRR (Variable Refresh Rate) states
// ---------------------------------------------------------------------------

/// VRR disabled (fixed refresh).
pub const DRM_VRR_DISABLED: u32 = 0;
/// VRR enabled (adaptive sync / FreeSync / G-Sync).
pub const DRM_VRR_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_flip_flags_no_overlap() {
        let flags = [
            DRM_MODE_PAGE_FLIP_EVENT, DRM_MODE_PAGE_FLIP_ASYNC,
            DRM_MODE_PAGE_FLIP_TARGET,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_atomic_flags_no_overlap() {
        let flags = [
            DRM_MODE_ATOMIC_TEST_ONLY, DRM_MODE_ATOMIC_NONBLOCK,
            DRM_MODE_ATOMIC_ALLOW_MODESET,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            DRM_EVENT_VBLANK, DRM_EVENT_FLIP_COMPLETE,
            DRM_EVENT_CRTC_SEQUENCE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_vrr_states() {
        assert_ne!(DRM_VRR_DISABLED, DRM_VRR_ENABLED);
    }
}
