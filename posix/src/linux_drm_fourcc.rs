//! `<drm/drm_fourcc.h>` — DRM pixel format FourCC codes.
//!
//! DRM uses FourCC (four character code) values to identify pixel
//! formats for framebuffers, planes, and overlays. These codes are
//! shared between kernel DRM/KMS and userspace (Mesa, Wayland, etc.).

// ---------------------------------------------------------------------------
// Helper macro-equivalent constants
// ---------------------------------------------------------------------------

/// Construct a FourCC code from 4 bytes.
pub const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

// ---------------------------------------------------------------------------
// Common pixel formats
// ---------------------------------------------------------------------------

/// 32-bit XRGB (8-8-8-8, no alpha).
pub const DRM_FORMAT_XRGB8888: u32 = fourcc(b'X', b'R', b'2', b'4');
/// 32-bit ARGB (8-8-8-8, with alpha).
pub const DRM_FORMAT_ARGB8888: u32 = fourcc(b'A', b'R', b'2', b'4');
/// 32-bit XBGR (8-8-8-8, no alpha).
pub const DRM_FORMAT_XBGR8888: u32 = fourcc(b'X', b'B', b'2', b'4');
/// 32-bit ABGR (8-8-8-8, with alpha).
pub const DRM_FORMAT_ABGR8888: u32 = fourcc(b'A', b'B', b'2', b'4');
/// 24-bit RGB (8-8-8).
pub const DRM_FORMAT_RGB888: u32 = fourcc(b'R', b'G', b'2', b'4');
/// 24-bit BGR (8-8-8).
pub const DRM_FORMAT_BGR888: u32 = fourcc(b'B', b'G', b'2', b'4');
/// 16-bit RGB (5-6-5).
pub const DRM_FORMAT_RGB565: u32 = fourcc(b'R', b'G', b'1', b'6');
/// NV12 (YUV 4:2:0, 2 planes).
pub const DRM_FORMAT_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
/// NV21 (YVU 4:2:0, 2 planes).
pub const DRM_FORMAT_NV21: u32 = fourcc(b'N', b'V', b'2', b'1');
/// YUV420 (3 planes).
pub const DRM_FORMAT_YUV420: u32 = fourcc(b'Y', b'U', b'1', b'2');
/// P010 (10-bit YUV, 2 planes).
pub const DRM_FORMAT_P010: u32 = fourcc(b'P', b'0', b'1', b'0');

// ---------------------------------------------------------------------------
// Format modifiers (vendor-neutral)
// ---------------------------------------------------------------------------

/// No modifier / linear layout.
pub const DRM_FORMAT_MOD_LINEAR: u64 = 0;
/// Invalid modifier sentinel.
pub const DRM_FORMAT_MOD_INVALID: u64 = 0x00FF_FFFF_FFFF_FFFF;

// ---------------------------------------------------------------------------
// Vendor modifier prefixes (bits 56-63)
// ---------------------------------------------------------------------------

/// No vendor.
pub const DRM_FORMAT_MOD_VENDOR_NONE: u64 = 0;
/// Intel vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_INTEL: u64 = 0x01;
/// AMD vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_AMD: u64 = 0x02;
/// Nvidia vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_NVIDIA: u64 = 0x03;
/// Samsung vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_SAMSUNG: u64 = 0x04;
/// ARM vendor modifier.
pub const DRM_FORMAT_MOD_VENDOR_ARM: u64 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_construction() {
        let code = fourcc(b'X', b'R', b'2', b'4');
        assert_eq!(code & 0xFF, b'X' as u32);
        assert_eq!((code >> 8) & 0xFF, b'R' as u32);
        assert_eq!((code >> 16) & 0xFF, b'2' as u32);
        assert_eq!((code >> 24) & 0xFF, b'4' as u32);
    }

    #[test]
    fn test_formats_distinct() {
        let formats = [
            DRM_FORMAT_XRGB8888, DRM_FORMAT_ARGB8888,
            DRM_FORMAT_XBGR8888, DRM_FORMAT_ABGR8888,
            DRM_FORMAT_RGB888, DRM_FORMAT_BGR888,
            DRM_FORMAT_RGB565, DRM_FORMAT_NV12,
            DRM_FORMAT_NV21, DRM_FORMAT_YUV420, DRM_FORMAT_P010,
        ];
        for i in 0..formats.len() {
            for j in (i + 1)..formats.len() {
                assert_ne!(formats[i], formats[j]);
            }
        }
    }

    #[test]
    fn test_modifiers_distinct() {
        assert_ne!(DRM_FORMAT_MOD_LINEAR, DRM_FORMAT_MOD_INVALID);
    }

    #[test]
    fn test_vendors_distinct() {
        let vendors = [
            DRM_FORMAT_MOD_VENDOR_NONE, DRM_FORMAT_MOD_VENDOR_INTEL,
            DRM_FORMAT_MOD_VENDOR_AMD, DRM_FORMAT_MOD_VENDOR_NVIDIA,
            DRM_FORMAT_MOD_VENDOR_SAMSUNG, DRM_FORMAT_MOD_VENDOR_ARM,
        ];
        for i in 0..vendors.len() {
            for j in (i + 1)..vendors.len() {
                assert_ne!(vendors[i], vendors[j]);
            }
        }
    }
}
