//! `<linux/fb.h>` — legacy framebuffer device ABI.
//!
//! The fbdev interface predates DRM/KMS but is still the only
//! display path on early-boot consoles (fbcon), Plymouth boot
//! splashes, and some embedded systems. The ioctls and pixel-format
//! enums below are the stable userspace surface.

// ---------------------------------------------------------------------------
// Device paths
// ---------------------------------------------------------------------------

/// Framebuffer device naming pattern: `/dev/fb0`, `/dev/fb1`, …
pub const FB_DEV_PREFIX: &str = "/dev/fb";

// ---------------------------------------------------------------------------
// ioctls (group 'F' = 0x46)
// ---------------------------------------------------------------------------

/// `FBIOGET_VSCREENINFO`.
pub const FBIOGET_VSCREENINFO: u32 = 0x4600;
/// `FBIOPUT_VSCREENINFO`.
pub const FBIOPUT_VSCREENINFO: u32 = 0x4601;
/// `FBIOGET_FSCREENINFO`.
pub const FBIOGET_FSCREENINFO: u32 = 0x4602;
/// `FBIOGETCMAP` — read color map.
pub const FBIOGETCMAP: u32 = 0x4604;
/// `FBIOPUTCMAP` — set color map.
pub const FBIOPUTCMAP: u32 = 0x4605;
/// `FBIOPAN_DISPLAY` — pan/scroll.
pub const FBIOPAN_DISPLAY: u32 = 0x4606;
/// `FBIO_CURSOR` — software cursor (deprecated).
pub const FBIO_CURSOR: u32 = 0x4608;
/// `FBIOGET_CON2FBMAP` — query console→fb mapping.
pub const FBIOGET_CON2FBMAP: u32 = 0x460F;
/// `FBIOPUT_CON2FBMAP` — set console→fb mapping.
pub const FBIOPUT_CON2FBMAP: u32 = 0x4610;
/// `FBIOBLANK` — blank/unblank display.
pub const FBIOBLANK: u32 = 0x4611;
/// `FBIOGET_VBLANK` — query vblank state.
pub const FBIOGET_VBLANK: u32 = 0x4612;
/// `FBIO_WAITFORVSYNC` — block until next vsync.
pub const FBIO_WAITFORVSYNC: u32 = 0x4020_4620;

// ---------------------------------------------------------------------------
// Visual types (fb_fix_screeninfo.visual)
// ---------------------------------------------------------------------------

/// Monochrome 1=white, 0=black.
pub const FB_VISUAL_MONO01: u32 = 0;
/// Monochrome 1=black, 0=white.
pub const FB_VISUAL_MONO10: u32 = 1;
/// True color (direct RGB encoding).
pub const FB_VISUAL_TRUECOLOR: u32 = 2;
/// Pseudo color (palette mapped).
pub const FB_VISUAL_PSEUDOCOLOR: u32 = 3;
/// Direct color (palette + RGB).
pub const FB_VISUAL_DIRECTCOLOR: u32 = 4;
/// Static pseudo color (read-only palette).
pub const FB_VISUAL_STATIC_PSEUDOCOLOR: u32 = 5;

// ---------------------------------------------------------------------------
// Type (fb_fix_screeninfo.type)
// ---------------------------------------------------------------------------

/// Packed pixels.
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;
/// Non interleaved planes.
pub const FB_TYPE_PLANES: u32 = 1;
/// Interleaved planes.
pub const FB_TYPE_INTERLEAVED_PLANES: u32 = 2;
/// Text mode (deprecated).
pub const FB_TYPE_TEXT: u32 = 3;
/// VGA planar (deprecated).
pub const FB_TYPE_VGA_PLANES: u32 = 4;

// ---------------------------------------------------------------------------
// Activate flags (fb_var_screeninfo.activate)
// ---------------------------------------------------------------------------

/// Apply settings at next open.
pub const FB_ACTIVATE_NOW: u32 = 0;
/// Apply on next vsync.
pub const FB_ACTIVATE_NXTOPEN: u32 = 1;
/// Don't apply yet.
pub const FB_ACTIVATE_TEST: u32 = 2;
/// Mask of activation bits.
pub const FB_ACTIVATE_MASK: u32 = 15;

// ---------------------------------------------------------------------------
// Blank levels (FBIOBLANK arg)
// ---------------------------------------------------------------------------

/// Powered on.
pub const FB_BLANK_UNBLANK: u32 = 0;
/// Normal blank.
pub const FB_BLANK_NORMAL: u32 = 1;
/// Vsync suspend (DPMS standby).
pub const FB_BLANK_VSYNC_SUSPEND: u32 = 2;
/// Hsync suspend (DPMS suspend).
pub const FB_BLANK_HSYNC_SUSPEND: u32 = 3;
/// Powerdown (DPMS off).
pub const FB_BLANK_POWERDOWN: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_prefix() {
        assert_eq!(FB_DEV_PREFIX, "/dev/fb");
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_f() {
        let ops = [
            FBIOGET_VSCREENINFO,
            FBIOPUT_VSCREENINFO,
            FBIOGET_FSCREENINFO,
            FBIOGETCMAP,
            FBIOPUTCMAP,
            FBIOPAN_DISPLAY,
            FBIO_CURSOR,
            FBIOGET_CON2FBMAP,
            FBIOPUT_CON2FBMAP,
            FBIOBLANK,
            FBIOGET_VBLANK,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'F' (0x46) in bits 8..15 for legacy _IO ioctls.
            assert_eq!((ops[i] >> 8) & 0xff, b'F' as u32);
        }
        // WAITFORVSYNC uses _IOW encoding so the layout differs.
        assert_ne!(FBIO_WAITFORVSYNC, 0);
    }

    #[test]
    fn test_visual_types_dense() {
        let v = [
            FB_VISUAL_MONO01,
            FB_VISUAL_MONO10,
            FB_VISUAL_TRUECOLOR,
            FB_VISUAL_PSEUDOCOLOR,
            FB_VISUAL_DIRECTCOLOR,
            FB_VISUAL_STATIC_PSEUDOCOLOR,
        ];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_type_dense() {
        let t = [
            FB_TYPE_PACKED_PIXELS,
            FB_TYPE_PLANES,
            FB_TYPE_INTERLEAVED_PLANES,
            FB_TYPE_TEXT,
            FB_TYPE_VGA_PLANES,
        ];
        for (i, &x) in t.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_activate_mask() {
        assert_eq!(FB_ACTIVATE_NOW, 0);
        assert_eq!(FB_ACTIVATE_NXTOPEN, 1);
        assert_eq!(FB_ACTIVATE_TEST, 2);
        // Mask covers the low 4 bits.
        assert_eq!(FB_ACTIVATE_MASK, 0xF);
    }

    #[test]
    fn test_blank_levels_dense() {
        // FBIOBLANK accepts 0..4; matches DPMS standby/suspend/off.
        let b = [
            FB_BLANK_UNBLANK,
            FB_BLANK_NORMAL,
            FB_BLANK_VSYNC_SUSPEND,
            FB_BLANK_HSYNC_SUSPEND,
            FB_BLANK_POWERDOWN,
        ];
        for (i, &x) in b.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }
}
