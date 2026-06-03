//! `<drm/drm_mode.h>` (plane subset) — DRM plane constants.
//!
//! DRM planes are hardware overlay layers that the display controller
//! can composite. Each plane reads from a framebuffer and applies
//! position, scaling, rotation, and blending before the result is
//! sent to the CRTC for display. Primary planes show the main
//! desktop, cursor planes show the mouse pointer, and overlay planes
//! handle video playback or HUD elements without CPU compositing.

// ---------------------------------------------------------------------------
// Plane types
// ---------------------------------------------------------------------------

/// Primary plane (main display surface, one per CRTC).
pub const DRM_PLANE_TYPE_PRIMARY: u32 = 0;
/// Overlay plane (additional hardware layer).
pub const DRM_PLANE_TYPE_OVERLAY: u32 = 1;
/// Cursor plane (hardware cursor, typically 64x64).
pub const DRM_PLANE_TYPE_CURSOR: u32 = 2;

// ---------------------------------------------------------------------------
// Plane rotation flags
// ---------------------------------------------------------------------------

/// No rotation (0 degrees).
pub const DRM_MODE_ROTATE_0: u32 = 1 << 0;
/// 90 degrees clockwise.
pub const DRM_MODE_ROTATE_90: u32 = 1 << 1;
/// 180 degrees (upside down).
pub const DRM_MODE_ROTATE_180: u32 = 1 << 2;
/// 270 degrees clockwise (90 counter-clockwise).
pub const DRM_MODE_ROTATE_270: u32 = 1 << 3;
/// Horizontal flip (mirror).
pub const DRM_MODE_REFLECT_X: u32 = 1 << 4;
/// Vertical flip.
pub const DRM_MODE_REFLECT_Y: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Plane blending modes
// ---------------------------------------------------------------------------

/// No blending (source replaces destination).
pub const DRM_MODE_BLEND_NONE: u32 = 0;
/// Pre-multiplied alpha blending.
pub const DRM_MODE_BLEND_PREMULTI: u32 = 1;
/// Coverage-based alpha blending.
pub const DRM_MODE_BLEND_COVERAGE: u32 = 2;

// ---------------------------------------------------------------------------
// Plane scaling filters
// ---------------------------------------------------------------------------

/// Default scaling (hardware decides).
pub const DRM_SCALING_FILTER_DEFAULT: u32 = 0;
/// Nearest-neighbor scaling (no interpolation).
pub const DRM_SCALING_FILTER_NEAREST: u32 = 1;

// ---------------------------------------------------------------------------
// Plane color encoding
// ---------------------------------------------------------------------------

/// BT.601 color encoding (SD TV).
pub const DRM_COLOR_YCBCR_BT601: u32 = 0;
/// BT.709 color encoding (HD TV).
pub const DRM_COLOR_YCBCR_BT709: u32 = 1;
/// BT.2020 color encoding (UHD/HDR).
pub const DRM_COLOR_YCBCR_BT2020: u32 = 2;

// ---------------------------------------------------------------------------
// Plane color range
// ---------------------------------------------------------------------------

/// Limited range (16-235 for Y, 16-240 for CbCr).
pub const DRM_COLOR_YCBCR_LIMITED: u32 = 0;
/// Full range (0-255).
pub const DRM_COLOR_YCBCR_FULL: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plane_types_distinct() {
        assert_ne!(DRM_PLANE_TYPE_PRIMARY, DRM_PLANE_TYPE_OVERLAY);
        assert_ne!(DRM_PLANE_TYPE_OVERLAY, DRM_PLANE_TYPE_CURSOR);
        assert_ne!(DRM_PLANE_TYPE_PRIMARY, DRM_PLANE_TYPE_CURSOR);
    }

    #[test]
    fn test_rotation_flags_no_overlap() {
        let flags = [
            DRM_MODE_ROTATE_0,
            DRM_MODE_ROTATE_90,
            DRM_MODE_ROTATE_180,
            DRM_MODE_ROTATE_270,
            DRM_MODE_REFLECT_X,
            DRM_MODE_REFLECT_Y,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_blend_modes_distinct() {
        let modes = [
            DRM_MODE_BLEND_NONE,
            DRM_MODE_BLEND_PREMULTI,
            DRM_MODE_BLEND_COVERAGE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_color_encoding_distinct() {
        let encs = [
            DRM_COLOR_YCBCR_BT601,
            DRM_COLOR_YCBCR_BT709,
            DRM_COLOR_YCBCR_BT2020,
        ];
        for i in 0..encs.len() {
            for j in (i + 1)..encs.len() {
                assert_ne!(encs[i], encs[j]);
            }
        }
    }
}
