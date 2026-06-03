//! `<drm/drm_color_mgmt.h>` — DRM color-management LUT constants.
//!
//! Constants for the per-CRTC and per-plane color-management
//! pipeline exposed via DRM atomic properties (gamma/degamma LUTs,
//! CSC matrices, color-space tags). KMS clients such as the GNOME
//! / KDE color manager and night-light services consume these.

// ---------------------------------------------------------------------------
// Standard color-space tags (drm_color_encoding values)
// ---------------------------------------------------------------------------

/// ITU-R BT.601 encoding.
pub const DRM_COLOR_YCBCR_BT601: u32 = 0;
/// ITU-R BT.709 encoding.
pub const DRM_COLOR_YCBCR_BT709: u32 = 1;
/// ITU-R BT.2020 encoding.
pub const DRM_COLOR_YCBCR_BT2020: u32 = 2;

// ---------------------------------------------------------------------------
// YCbCr quantisation range (drm_color_range values)
// ---------------------------------------------------------------------------

/// Limited range (16..=235 luma).
pub const DRM_COLOR_YCBCR_LIMITED_RANGE: u32 = 0;
/// Full range (0..=255 luma).
pub const DRM_COLOR_YCBCR_FULL_RANGE: u32 = 1;

// ---------------------------------------------------------------------------
// HDR EOTF tags (drm_hdmi_eotf values)
// ---------------------------------------------------------------------------

/// Traditional gamma SDR.
pub const HDMI_EOTF_TRADITIONAL_GAMMA_SDR: u32 = 0;
/// Traditional gamma HDR.
pub const HDMI_EOTF_TRADITIONAL_GAMMA_HDR: u32 = 1;
/// SMPTE ST 2084 (PQ).
pub const HDMI_EOTF_SMPTE_ST2084: u32 = 2;
/// Hybrid Log-Gamma (HLG).
pub const HDMI_EOTF_BT_2100_HLG: u32 = 3;

// ---------------------------------------------------------------------------
// LUT-entry packing limits (struct drm_color_lut)
// ---------------------------------------------------------------------------

/// Maximum LUT size accepted by modern atomic KMS drivers.
pub const DRM_COLOR_LUT_SIZE_MAX: u32 = 4096;
/// Minimum LUT size — a 1024-entry table is the practical floor.
pub const DRM_COLOR_LUT_SIZE_MIN: u32 = 256;
/// Width of each LUT entry channel (red/green/blue + reserved).
pub const DRM_COLOR_LUT_CHANNEL_BITS: u32 = 16;

// ---------------------------------------------------------------------------
// CTM matrix dimensions (struct drm_color_ctm)
// ---------------------------------------------------------------------------

/// 3x3 color-transform matrix, 9 entries (S31.32 fixed-point).
pub const DRM_COLOR_CTM_ENTRIES: u32 = 9;
/// CTM coefficients use a 31.32 signed fixed-point format (in bytes).
pub const DRM_COLOR_CTM_ENTRY_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_encodings_distinct() {
        let e = [
            DRM_COLOR_YCBCR_BT601,
            DRM_COLOR_YCBCR_BT709,
            DRM_COLOR_YCBCR_BT2020,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_quant_ranges_distinct() {
        assert_ne!(
            DRM_COLOR_YCBCR_LIMITED_RANGE,
            DRM_COLOR_YCBCR_FULL_RANGE
        );
    }

    #[test]
    fn test_eotf_distinct() {
        let eotfs = [
            HDMI_EOTF_TRADITIONAL_GAMMA_SDR,
            HDMI_EOTF_TRADITIONAL_GAMMA_HDR,
            HDMI_EOTF_SMPTE_ST2084,
            HDMI_EOTF_BT_2100_HLG,
        ];
        for i in 0..eotfs.len() {
            for j in (i + 1)..eotfs.len() {
                assert_ne!(eotfs[i], eotfs[j]);
            }
        }
    }

    #[test]
    fn test_lut_sizes_sane() {
        assert!(DRM_COLOR_LUT_SIZE_MIN < DRM_COLOR_LUT_SIZE_MAX);
        assert!(DRM_COLOR_LUT_SIZE_MIN.is_power_of_two());
        assert!(DRM_COLOR_LUT_SIZE_MAX.is_power_of_two());
        assert_eq!(DRM_COLOR_LUT_CHANNEL_BITS, 16);
    }

    #[test]
    fn test_ctm_layout() {
        // CTM is a 3x3 matrix; total byte size must match the
        // documented struct drm_color_ctm.matrix size (72 bytes).
        assert_eq!(DRM_COLOR_CTM_ENTRIES, 9);
        assert_eq!(DRM_COLOR_CTM_ENTRIES * DRM_COLOR_CTM_ENTRY_SIZE, 72);
    }
}
