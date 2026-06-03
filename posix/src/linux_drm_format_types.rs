//! `<drm/drm_fourcc.h>` — DRM fourcc pixel-format codes.
//!
//! DRM/KMS, GBM, EGL_EXT_image_dma_buf_import, Vulkan
//! VK_EXT_image_drm_format_modifier, V4L2 multi-planar, and Wayland
//! linux-dmabuf all key on these little-endian fourcc codes. Codes
//! below cover the formats that compositors and video pipelines
//! handle on the desktop and on common ARM SoCs.

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fourcc code from four ASCII chars in little-endian order
/// (matches the kernel's `fourcc_code()` macro).
const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

// ---------------------------------------------------------------------------
// RGB / BGR packed formats
// ---------------------------------------------------------------------------

/// 8-bit alpha-only (`C8 ` is paletted, this is a single-channel mask).
pub const DRM_FORMAT_C8: u32 = fourcc(b'C', b'8', b' ', b' ');
/// 24bpp RGB, B in low byte (`RG24`).
pub const DRM_FORMAT_RGB888: u32 = fourcc(b'R', b'G', b'2', b'4');
/// 24bpp BGR, R in low byte (`BG24`).
pub const DRM_FORMAT_BGR888: u32 = fourcc(b'B', b'G', b'2', b'4');
/// 32bpp xRGB8888 (alpha ignored, B in low byte).
pub const DRM_FORMAT_XRGB8888: u32 = fourcc(b'X', b'R', b'2', b'4');
/// 32bpp ARGB8888.
pub const DRM_FORMAT_ARGB8888: u32 = fourcc(b'A', b'R', b'2', b'4');
/// 32bpp xBGR8888.
pub const DRM_FORMAT_XBGR8888: u32 = fourcc(b'X', b'B', b'2', b'4');
/// 32bpp ABGR8888.
pub const DRM_FORMAT_ABGR8888: u32 = fourcc(b'A', b'B', b'2', b'4');
/// 16bpp RGB565 (5R-6G-5B, B in low bits).
pub const DRM_FORMAT_RGB565: u32 = fourcc(b'R', b'G', b'1', b'6');

// ---------------------------------------------------------------------------
// 10-bit-per-channel formats (HDR / WCG)
// ---------------------------------------------------------------------------

/// 32bpp 2-bit alpha + 10-bit RGB (`AR30`).
pub const DRM_FORMAT_ARGB2101010: u32 = fourcc(b'A', b'R', b'3', b'0');
/// 32bpp 2-bit unused + 10-bit RGB (`XR30`).
pub const DRM_FORMAT_XRGB2101010: u32 = fourcc(b'X', b'R', b'3', b'0');
/// 32bpp 2-bit alpha + 10-bit BGR (`AB30`).
pub const DRM_FORMAT_ABGR2101010: u32 = fourcc(b'A', b'B', b'3', b'0');

// ---------------------------------------------------------------------------
// YUV 4:2:0 multi-plane (the formats GPUs decode video into)
// ---------------------------------------------------------------------------

/// 12bpp Y plane + interleaved CrCb plane (`NV12`).
pub const DRM_FORMAT_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
/// 12bpp Y plane + interleaved CbCr plane (`NV21`).
pub const DRM_FORMAT_NV21: u32 = fourcc(b'N', b'V', b'2', b'1');
/// 16bpp Y plane + interleaved CrCb plane at 4:2:2 (`NV16`).
pub const DRM_FORMAT_NV16: u32 = fourcc(b'N', b'V', b'1', b'6');
/// 12bpp planar YUV 4:2:0 (`YV12`).
pub const DRM_FORMAT_YVU420: u32 = fourcc(b'Y', b'V', b'1', b'2');
/// 12bpp planar YUV 4:2:0, U before V (`YU12`).
pub const DRM_FORMAT_YUV420: u32 = fourcc(b'Y', b'U', b'1', b'2');

// ---------------------------------------------------------------------------
// Invalid / sentinel
// ---------------------------------------------------------------------------

/// Reserved fourcc meaning "no format".
pub const DRM_FORMAT_INVALID: u32 = 0;
/// DRM format modifier "no modifier" (`DRM_FORMAT_MOD_LINEAR`).
pub const DRM_FORMAT_MOD_LINEAR: u64 = 0;
/// DRM format modifier "invalid" sentinel.
pub const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_helper_matches_known_codes() {
        // XRGB8888 must spell "XR24" little-endian.
        assert_eq!(DRM_FORMAT_XRGB8888.to_le_bytes(), *b"XR24");
        assert_eq!(DRM_FORMAT_NV12.to_le_bytes(), *b"NV12");
        assert_eq!(DRM_FORMAT_RGB565.to_le_bytes(), *b"RG16");
    }

    #[test]
    fn test_rgb_formats_distinct() {
        let fmts = [
            DRM_FORMAT_C8,
            DRM_FORMAT_RGB888,
            DRM_FORMAT_BGR888,
            DRM_FORMAT_XRGB8888,
            DRM_FORMAT_ARGB8888,
            DRM_FORMAT_XBGR8888,
            DRM_FORMAT_ABGR8888,
            DRM_FORMAT_RGB565,
            DRM_FORMAT_ARGB2101010,
            DRM_FORMAT_XRGB2101010,
            DRM_FORMAT_ABGR2101010,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_yuv_formats_distinct() {
        let fmts = [
            DRM_FORMAT_NV12,
            DRM_FORMAT_NV21,
            DRM_FORMAT_NV16,
            DRM_FORMAT_YVU420,
            DRM_FORMAT_YUV420,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_modifier_sentinels() {
        assert_eq!(DRM_FORMAT_INVALID, 0);
        assert_eq!(DRM_FORMAT_MOD_LINEAR, 0);
        // INVALID modifier must be distinguishable from LINEAR.
        assert_ne!(DRM_FORMAT_MOD_INVALID, DRM_FORMAT_MOD_LINEAR);
    }
}
