//! DRM/KMS (Kernel Mode Setting) operational constants.
//!
//! Additional KMS constants for DPMS power modes, content types,
//! colorspace, and encoder types not covered by drm_mode.h.

// ---------------------------------------------------------------------------
// DPMS power modes
// ---------------------------------------------------------------------------

/// Display on.
pub const DRM_MODE_DPMS_ON: u32 = 0;
/// Standby (partial power save).
pub const DRM_MODE_DPMS_STANDBY: u32 = 1;
/// Suspend (more aggressive power save).
pub const DRM_MODE_DPMS_SUSPEND: u32 = 2;
/// Off (display completely powered down).
pub const DRM_MODE_DPMS_OFF: u32 = 3;

// ---------------------------------------------------------------------------
// Content type (HDMI)
// ---------------------------------------------------------------------------

/// No declared content type.
pub const DRM_MODE_CONTENT_TYPE_NO_DATA: u32 = 0;
/// Graphics content.
pub const DRM_MODE_CONTENT_TYPE_GRAPHICS: u32 = 1;
/// Photo content.
pub const DRM_MODE_CONTENT_TYPE_PHOTO: u32 = 2;
/// Cinema content.
pub const DRM_MODE_CONTENT_TYPE_CINEMA: u32 = 3;
/// Game content (low latency).
pub const DRM_MODE_CONTENT_TYPE_GAME: u32 = 4;

// ---------------------------------------------------------------------------
// Encoder types
// ---------------------------------------------------------------------------

/// No encoder.
pub const DRM_MODE_ENCODER_NONE: u32 = 0;
/// DAC encoder.
pub const DRM_MODE_ENCODER_DAC: u32 = 1;
/// TMDS encoder (DVI/HDMI).
pub const DRM_MODE_ENCODER_TMDS: u32 = 2;
/// LVDS encoder.
pub const DRM_MODE_ENCODER_LVDS: u32 = 3;
/// TV DAC encoder.
pub const DRM_MODE_ENCODER_TVDAC: u32 = 4;
/// Virtual encoder.
pub const DRM_MODE_ENCODER_VIRTUAL: u32 = 5;
/// DSI encoder.
pub const DRM_MODE_ENCODER_DSI: u32 = 6;
/// DisplayPort MST encoder.
pub const DRM_MODE_ENCODER_DPMST: u32 = 7;
/// DPI encoder.
pub const DRM_MODE_ENCODER_DPI: u32 = 8;

// ---------------------------------------------------------------------------
// Colorspace
// ---------------------------------------------------------------------------

/// Default colorspace.
pub const DRM_MODE_COLORIMETRY_DEFAULT: u32 = 0;
/// sRGB.
pub const DRM_MODE_COLORIMETRY_SRGB: u32 = 1;
/// BT.709 (HD video).
pub const DRM_MODE_COLORIMETRY_BT709_YCC: u32 = 6;
/// BT.2020 RGB.
pub const DRM_MODE_COLORIMETRY_BT2020_RGB: u32 = 9;
/// BT.2020 YCC.
pub const DRM_MODE_COLORIMETRY_BT2020_YCC: u32 = 10;
/// DCI-P3 RGB (wide gamut).
pub const DRM_MODE_COLORIMETRY_DCI_P3_RGB: u32 = 11;

// ---------------------------------------------------------------------------
// VRR (Variable Refresh Rate)
// ---------------------------------------------------------------------------

/// VRR disabled.
pub const DRM_MODE_VRR_DISABLED: u32 = 0;
/// VRR enabled.
pub const DRM_MODE_VRR_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpms_modes_distinct() {
        let modes = [
            DRM_MODE_DPMS_ON, DRM_MODE_DPMS_STANDBY,
            DRM_MODE_DPMS_SUSPEND, DRM_MODE_DPMS_OFF,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_content_types_distinct() {
        let types = [
            DRM_MODE_CONTENT_TYPE_NO_DATA, DRM_MODE_CONTENT_TYPE_GRAPHICS,
            DRM_MODE_CONTENT_TYPE_PHOTO, DRM_MODE_CONTENT_TYPE_CINEMA,
            DRM_MODE_CONTENT_TYPE_GAME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_encoder_types_distinct() {
        let types = [
            DRM_MODE_ENCODER_NONE, DRM_MODE_ENCODER_DAC,
            DRM_MODE_ENCODER_TMDS, DRM_MODE_ENCODER_LVDS,
            DRM_MODE_ENCODER_TVDAC, DRM_MODE_ENCODER_VIRTUAL,
            DRM_MODE_ENCODER_DSI, DRM_MODE_ENCODER_DPMST,
            DRM_MODE_ENCODER_DPI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_colorspace_distinct() {
        let colors = [
            DRM_MODE_COLORIMETRY_DEFAULT, DRM_MODE_COLORIMETRY_SRGB,
            DRM_MODE_COLORIMETRY_BT709_YCC, DRM_MODE_COLORIMETRY_BT2020_RGB,
            DRM_MODE_COLORIMETRY_BT2020_YCC, DRM_MODE_COLORIMETRY_DCI_P3_RGB,
        ];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn test_vrr_states() {
        assert_ne!(DRM_MODE_VRR_DISABLED, DRM_MODE_VRR_ENABLED);
    }
}
