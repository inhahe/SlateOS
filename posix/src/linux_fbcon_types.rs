//! `<linux/fb.h>` — Framebuffer console constants.
//!
//! The Linux framebuffer subsystem provides a hardware-independent
//! interface for display devices. These constants define pixel
//! formats, visual types, acceleration flags, and ioctl commands.

// ---------------------------------------------------------------------------
// Framebuffer ioctl commands
// ---------------------------------------------------------------------------

/// Get variable screen info.
pub const FBIOGET_VSCREENINFO: u32 = 0x4600;
/// Set variable screen info.
pub const FBIOPUT_VSCREENINFO: u32 = 0x4601;
/// Get fixed screen info.
pub const FBIOGET_FSCREENINFO: u32 = 0x4602;
/// Get color map.
pub const FBIOGETCMAP: u32 = 0x4604;
/// Set color map.
pub const FBIOPUTCMAP: u32 = 0x4605;
/// Pan display (scroll).
pub const FBIOPAN_DISPLAY: u32 = 0x4606;
/// Blank/unblank display.
pub const FBIOBLANK: u32 = 0x4611;

// ---------------------------------------------------------------------------
// Visual types (fb_fix_screeninfo.visual)
// ---------------------------------------------------------------------------

/// Monochrome (1 bit per pixel).
pub const FB_VISUAL_MONO01: u32 = 0;
/// Monochrome (inverse).
pub const FB_VISUAL_MONO10: u32 = 1;
/// True color (direct RGB).
pub const FB_VISUAL_TRUECOLOR: u32 = 2;
/// Pseudo color (indexed palette).
pub const FB_VISUAL_PSEUDOCOLOR: u32 = 3;
/// Direct color (programmable per-channel LUT).
pub const FB_VISUAL_DIRECTCOLOR: u32 = 4;
/// Static pseudo color.
pub const FB_VISUAL_STATIC_PSEUDOCOLOR: u32 = 5;

// ---------------------------------------------------------------------------
// Framebuffer type (fb_fix_screeninfo.type)
// ---------------------------------------------------------------------------

/// Packed pixels.
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;
/// Non-interleaved bit planes.
pub const FB_TYPE_PLANES: u32 = 1;
/// Interleaved bit planes.
pub const FB_TYPE_INTERLEAVED_PLANES: u32 = 2;
/// Text mode (not graphics).
pub const FB_TYPE_TEXT: u32 = 3;
/// VGA planes (EGA/VGA).
pub const FB_TYPE_VGA_PLANES: u32 = 4;

// ---------------------------------------------------------------------------
// Blank modes (for FBIOBLANK)
// ---------------------------------------------------------------------------

/// Unblank.
pub const FB_BLANK_UNBLANK: u32 = 0;
/// Normal blanking (VESA blank).
pub const FB_BLANK_NORMAL: u32 = 1;
/// VESA DPMS standby.
pub const FB_BLANK_VSYNC_SUSPEND: u32 = 2;
/// VESA DPMS suspend.
pub const FB_BLANK_HSYNC_SUSPEND: u32 = 3;
/// VESA DPMS power off.
pub const FB_BLANK_POWERDOWN: u32 = 4;

// ---------------------------------------------------------------------------
// Acceleration flags
// ---------------------------------------------------------------------------

/// No hardware acceleration.
pub const FB_ACCEL_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            FBIOGET_VSCREENINFO,
            FBIOPUT_VSCREENINFO,
            FBIOGET_FSCREENINFO,
            FBIOGETCMAP,
            FBIOPUTCMAP,
            FBIOPAN_DISPLAY,
            FBIOBLANK,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_visual_types_distinct() {
        let visuals = [
            FB_VISUAL_MONO01,
            FB_VISUAL_MONO10,
            FB_VISUAL_TRUECOLOR,
            FB_VISUAL_PSEUDOCOLOR,
            FB_VISUAL_DIRECTCOLOR,
            FB_VISUAL_STATIC_PSEUDOCOLOR,
        ];
        for i in 0..visuals.len() {
            for j in (i + 1)..visuals.len() {
                assert_ne!(visuals[i], visuals[j]);
            }
        }
    }

    #[test]
    fn test_fb_types_distinct() {
        let types = [
            FB_TYPE_PACKED_PIXELS,
            FB_TYPE_PLANES,
            FB_TYPE_INTERLEAVED_PLANES,
            FB_TYPE_TEXT,
            FB_TYPE_VGA_PLANES,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_blank_modes_distinct() {
        let modes = [
            FB_BLANK_UNBLANK,
            FB_BLANK_NORMAL,
            FB_BLANK_VSYNC_SUSPEND,
            FB_BLANK_HSYNC_SUSPEND,
            FB_BLANK_POWERDOWN,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_accel_none() {
        assert_eq!(FB_ACCEL_NONE, 0);
    }

    #[test]
    fn test_unblank_is_zero() {
        assert_eq!(FB_BLANK_UNBLANK, 0);
    }
}
