//! `<linux/fb.h>` — Additional framebuffer device constants.
//!
//! Supplementary framebuffer constants covering visual types,
//! acceleration flags, ioctl commands, and rotate modes.

// ---------------------------------------------------------------------------
// FB visual types (FB_VISUAL_*)
// ---------------------------------------------------------------------------

/// Monochrome 1-bit.
pub const FB_VISUAL_MONO01: u32 = 0;
/// Monochrome 0-bit.
pub const FB_VISUAL_MONO10: u32 = 1;
/// True color.
pub const FB_VISUAL_TRUECOLOR: u32 = 2;
/// Pseudo color.
pub const FB_VISUAL_PSEUDOCOLOR: u32 = 3;
/// Direct color.
pub const FB_VISUAL_DIRECTCOLOR: u32 = 4;
/// Static pseudo color.
pub const FB_VISUAL_STATIC_PSEUDOCOLOR: u32 = 5;
/// Four-color grayscale.
pub const FB_VISUAL_FOURCC: u32 = 6;

// ---------------------------------------------------------------------------
// FB type (FB_TYPE_*)
// ---------------------------------------------------------------------------

/// Packed pixels.
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;
/// Non-interleaved planes.
pub const FB_TYPE_PLANES: u32 = 1;
/// Interleaved planes.
pub const FB_TYPE_INTERLEAVED_PLANES: u32 = 2;
/// Text mode (non-graphic).
pub const FB_TYPE_TEXT: u32 = 3;
/// EGA/VGA planes.
pub const FB_TYPE_VGA_PLANES: u32 = 4;
/// FOURCC-based.
pub const FB_TYPE_FOURCC: u32 = 5;

// ---------------------------------------------------------------------------
// FB acceleration flags (FB_ACCEL_*)
// ---------------------------------------------------------------------------

/// No acceleration.
pub const FB_ACCEL_NONE: u32 = 0;
/// Amiga Blitter.
pub const FB_ACCEL_ATARIBLITT: u32 = 1;
/// Amiga Blitter.
pub const FB_ACCEL_AMIGABLITT: u32 = 2;
/// S3 Trio64.
pub const FB_ACCEL_S3_TRIO64: u32 = 3;
/// NCR 77C32BLT.
pub const FB_ACCEL_NCR_77C32BLT: u32 = 4;
/// S3 Virge.
pub const FB_ACCEL_S3_VIRGE: u32 = 5;
/// ATI Mach64 GX.
pub const FB_ACCEL_ATI_MACH64GX: u32 = 6;

// ---------------------------------------------------------------------------
// FB ioctl commands
// ---------------------------------------------------------------------------

/// Get screen info (variable).
pub const FBIOGET_VSCREENINFO: u32 = 0x4600;
/// Set screen info (variable).
pub const FBIOPUT_VSCREENINFO: u32 = 0x4601;
/// Get screen info (fixed).
pub const FBIOGET_FSCREENINFO: u32 = 0x4602;
/// Get color map.
pub const FBIOGETCMAP: u32 = 0x4604;
/// Set color map.
pub const FBIOPUTCMAP: u32 = 0x4605;
/// Pan display.
pub const FBIOPAN_DISPLAY: u32 = 0x4606;
/// Blank/unblank.
pub const FBIOBLANK: u32 = 0x4611;

// ---------------------------------------------------------------------------
// FB blank modes (FB_BLANK_*)
// ---------------------------------------------------------------------------

/// Unblank.
pub const FB_BLANK_UNBLANK: u32 = 0;
/// Normal blanking.
pub const FB_BLANK_NORMAL: u32 = 1;
/// VESA blank (hsync off).
pub const FB_BLANK_VSYNC_SUSPEND: u32 = 2;
/// VESA blank (vsync off).
pub const FB_BLANK_HSYNC_SUSPEND: u32 = 3;
/// Power down.
pub const FB_BLANK_POWERDOWN: u32 = 4;

// ---------------------------------------------------------------------------
// FB rotate modes
// ---------------------------------------------------------------------------

/// No rotation.
pub const FB_ROTATE_UR: u32 = 0;
/// 90° clockwise.
pub const FB_ROTATE_CW: u32 = 1;
/// 180°.
pub const FB_ROTATE_UD: u32 = 2;
/// 90° counter-clockwise.
pub const FB_ROTATE_CCW: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visual_types_distinct() {
        let visuals = [
            FB_VISUAL_MONO01,
            FB_VISUAL_MONO10,
            FB_VISUAL_TRUECOLOR,
            FB_VISUAL_PSEUDOCOLOR,
            FB_VISUAL_DIRECTCOLOR,
            FB_VISUAL_STATIC_PSEUDOCOLOR,
            FB_VISUAL_FOURCC,
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
            FB_TYPE_FOURCC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_accel_distinct() {
        let accels = [
            FB_ACCEL_NONE,
            FB_ACCEL_ATARIBLITT,
            FB_ACCEL_AMIGABLITT,
            FB_ACCEL_S3_TRIO64,
            FB_ACCEL_NCR_77C32BLT,
            FB_ACCEL_S3_VIRGE,
            FB_ACCEL_ATI_MACH64GX,
        ];
        for i in 0..accels.len() {
            for j in (i + 1)..accels.len() {
                assert_ne!(accels[i], accels[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [
            FBIOGET_VSCREENINFO,
            FBIOPUT_VSCREENINFO,
            FBIOGET_FSCREENINFO,
            FBIOGETCMAP,
            FBIOPUTCMAP,
            FBIOPAN_DISPLAY,
            FBIOBLANK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_blank_modes_distinct() {
        let blanks = [
            FB_BLANK_UNBLANK,
            FB_BLANK_NORMAL,
            FB_BLANK_VSYNC_SUSPEND,
            FB_BLANK_HSYNC_SUSPEND,
            FB_BLANK_POWERDOWN,
        ];
        for i in 0..blanks.len() {
            for j in (i + 1)..blanks.len() {
                assert_ne!(blanks[i], blanks[j]);
            }
        }
    }

    #[test]
    fn test_rotate_modes_distinct() {
        let rots = [FB_ROTATE_UR, FB_ROTATE_CW, FB_ROTATE_UD, FB_ROTATE_CCW];
        for i in 0..rots.len() {
            for j in (i + 1)..rots.len() {
                assert_ne!(rots[i], rots[j]);
            }
        }
    }
}
