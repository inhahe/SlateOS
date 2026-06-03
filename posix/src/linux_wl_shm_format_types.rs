//! Wayland `wl_shm` — shared memory pixel format constants.
//!
//! Wayland's `wl_shm` interface provides shared memory buffers for
//! software-rendered surfaces. The format codes identify pixel layout
//! in these buffers. They match DRM/GBM FourCC codes where possible,
//! ensuring zero-copy path from software rendering to display.

// ---------------------------------------------------------------------------
// Core pixel formats (wl_shm_format)
// ---------------------------------------------------------------------------

/// ARGB 8-8-8-8 (alpha in high byte).
pub const WL_SHM_FORMAT_ARGB8888: u32 = 0;
/// XRGB 8-8-8-8 (alpha ignored).
pub const WL_SHM_FORMAT_XRGB8888: u32 = 1;

// ---------------------------------------------------------------------------
// Extended formats (matching DRM FourCC)
// ---------------------------------------------------------------------------

/// RGB 5-6-5 (16 bpp, no alpha).
pub const WL_SHM_FORMAT_RGB565: u32 = 0x3631_5852;
/// BGR 5-6-5.
pub const WL_SHM_FORMAT_BGR565: u32 = 0x3631_5842;
/// ARGB 1-5-5-5.
pub const WL_SHM_FORMAT_ARGB1555: u32 = 0x3531_5241;
/// XRGB 1-5-5-5 (alpha ignored).
pub const WL_SHM_FORMAT_XRGB1555: u32 = 0x3531_5258;
/// ARGB 4-4-4-4.
pub const WL_SHM_FORMAT_ARGB4444: u32 = 0x3434_5241;
/// ABGR 8-8-8-8.
pub const WL_SHM_FORMAT_ABGR8888: u32 = 0x3432_4241;
/// XBGR 8-8-8-8.
pub const WL_SHM_FORMAT_XBGR8888: u32 = 0x3432_4258;
/// RGBA 8-8-8-8.
pub const WL_SHM_FORMAT_RGBA8888: u32 = 0x3432_4152;
/// RGBX 8-8-8-8.
pub const WL_SHM_FORMAT_RGBX8888: u32 = 0x3432_5852;
/// BGRA 8-8-8-8.
pub const WL_SHM_FORMAT_BGRA8888: u32 = 0x3432_4142;
/// BGRX 8-8-8-8.
pub const WL_SHM_FORMAT_BGRX8888: u32 = 0x3432_5842;

// ---------------------------------------------------------------------------
// NV12/NV21 (for video surfaces)
// ---------------------------------------------------------------------------

/// NV12: Y + interleaved UV (4:2:0).
pub const WL_SHM_FORMAT_NV12: u32 = 0x3231_564E;
/// NV21: Y + interleaved VU (4:2:0).
pub const WL_SHM_FORMAT_NV21: u32 = 0x3132_564E;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_formats_distinct() {
        assert_ne!(WL_SHM_FORMAT_ARGB8888, WL_SHM_FORMAT_XRGB8888);
    }

    #[test]
    fn test_core_format_values() {
        assert_eq!(WL_SHM_FORMAT_ARGB8888, 0);
        assert_eq!(WL_SHM_FORMAT_XRGB8888, 1);
    }

    #[test]
    fn test_extended_formats_distinct() {
        let fmts = [
            WL_SHM_FORMAT_RGB565,
            WL_SHM_FORMAT_BGR565,
            WL_SHM_FORMAT_ARGB1555,
            WL_SHM_FORMAT_XRGB1555,
            WL_SHM_FORMAT_ARGB4444,
            WL_SHM_FORMAT_ABGR8888,
            WL_SHM_FORMAT_XBGR8888,
            WL_SHM_FORMAT_RGBA8888,
            WL_SHM_FORMAT_RGBX8888,
            WL_SHM_FORMAT_BGRA8888,
            WL_SHM_FORMAT_BGRX8888,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_nv_formats_distinct() {
        assert_ne!(WL_SHM_FORMAT_NV12, WL_SHM_FORMAT_NV21);
    }

    #[test]
    fn test_extended_formats_nonzero() {
        assert_ne!(WL_SHM_FORMAT_RGB565, 0);
        assert_ne!(WL_SHM_FORMAT_ABGR8888, 0);
    }
}
