//! `<drm/drm_fourcc.h>` — DRM pixel format FourCC codes.
//!
//! DRM/KMS uses FourCC (four-character code) values to identify pixel
//! formats for framebuffers, planes, and DMA-BUF imports. Each format
//! encodes the color space, channel ordering, bit depth, and memory
//! layout. Modifiers extend formats with tiling/compression info.

// ---------------------------------------------------------------------------
// Common pixel formats (DRM_FORMAT_*)
// ---------------------------------------------------------------------------

/// 32-bit XRGB (8:8:8:8, X=unused, little-endian).
pub const DRM_FORMAT_XRGB8888: u32 = 0x3441_5258; // 'XR24'
/// 32-bit ARGB (8:8:8:8, A=alpha, little-endian).
pub const DRM_FORMAT_ARGB8888: u32 = 0x3441_5241; // 'AR24'
/// 32-bit XBGR (8:8:8:8).
pub const DRM_FORMAT_XBGR8888: u32 = 0x3442_5258; // 'XB24'
/// 32-bit ABGR (8:8:8:8).
pub const DRM_FORMAT_ABGR8888: u32 = 0x3442_5241; // 'AB24'
/// 24-bit RGB (8:8:8).
pub const DRM_FORMAT_RGB888: u32 = 0x3842_4752; // 'RG24'
/// 24-bit BGR (8:8:8).
pub const DRM_FORMAT_BGR888: u32 = 0x3842_4742; // 'BG24'
/// 16-bit RGB (5:6:5).
pub const DRM_FORMAT_RGB565: u32 = 0x3647_5252; // 'RG16'
/// NV12: Y plane + interleaved UV (4:2:0).
pub const DRM_FORMAT_NV12: u32 = 0x3231_564E; // 'NV12'
/// NV21: Y plane + interleaved VU (4:2:0).
pub const DRM_FORMAT_NV21: u32 = 0x3132_564E; // 'NV21'
/// YUV420: Y + U + V separate planes (4:2:0).
pub const DRM_FORMAT_YUV420: u32 = 0x3231_5559; // 'YU12'
/// YUYV 4:2:2 packed.
pub const DRM_FORMAT_YUYV: u32 = 0x5659_5559; // 'YUYV'

// ---------------------------------------------------------------------------
// Format modifiers (vendor | modifier)
// ---------------------------------------------------------------------------

/// No modifier (linear layout).
pub const DRM_FORMAT_MOD_LINEAR: u64 = 0;
/// Invalid modifier sentinel.
pub const DRM_FORMAT_MOD_INVALID: u64 = 0x00FF_FFFF_FFFF_FFFF;

// ---------------------------------------------------------------------------
// Modifier vendor IDs
// ---------------------------------------------------------------------------

/// No vendor (generic modifiers).
pub const DRM_FORMAT_MOD_VENDOR_NONE: u64 = 0;
/// Intel vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_INTEL: u64 = 0x01;
/// AMD vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_AMD: u64 = 0x02;
/// NVIDIA vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_NVIDIA: u64 = 0x03;
/// Samsung vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_SAMSUNG: u64 = 0x04;
/// Qualcomm vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_QCOM: u64 = 0x05;
/// ARM vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_ARM: u64 = 0x08;

// ---------------------------------------------------------------------------
// Bits per pixel helpers
// ---------------------------------------------------------------------------

/// Bits per pixel for XRGB8888 / ARGB8888.
pub const DRM_FORMAT_BPP_32: u32 = 32;
/// Bits per pixel for RGB888.
pub const DRM_FORMAT_BPP_24: u32 = 24;
/// Bits per pixel for RGB565.
pub const DRM_FORMAT_BPP_16: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formats_distinct() {
        let fmts = [
            DRM_FORMAT_XRGB8888,
            DRM_FORMAT_ARGB8888,
            DRM_FORMAT_XBGR8888,
            DRM_FORMAT_ABGR8888,
            DRM_FORMAT_RGB888,
            DRM_FORMAT_BGR888,
            DRM_FORMAT_RGB565,
            DRM_FORMAT_NV12,
            DRM_FORMAT_NV21,
            DRM_FORMAT_YUV420,
            DRM_FORMAT_YUYV,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_modifier_vendors_distinct() {
        let vendors = [
            DRM_FORMAT_MOD_VENDOR_NONE,
            DRM_FORMAT_MOD_VENDOR_INTEL,
            DRM_FORMAT_MOD_VENDOR_AMD,
            DRM_FORMAT_MOD_VENDOR_NVIDIA,
            DRM_FORMAT_MOD_VENDOR_SAMSUNG,
            DRM_FORMAT_MOD_VENDOR_QCOM,
            DRM_FORMAT_MOD_VENDOR_ARM,
        ];
        for i in 0..vendors.len() {
            for j in (i + 1)..vendors.len() {
                assert_ne!(vendors[i], vendors[j]);
            }
        }
    }

    #[test]
    fn test_linear_modifier() {
        assert_eq!(DRM_FORMAT_MOD_LINEAR, 0);
        assert_ne!(DRM_FORMAT_MOD_LINEAR, DRM_FORMAT_MOD_INVALID);
    }

    #[test]
    fn test_bpp_values() {
        assert_eq!(DRM_FORMAT_BPP_32, 32);
        assert_eq!(DRM_FORMAT_BPP_24, 24);
        assert_eq!(DRM_FORMAT_BPP_16, 16);
    }
}
