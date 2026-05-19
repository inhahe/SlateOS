//! `<linux/fb.h>` — Additional framebuffer constants (part 3).
//!
//! Supplementary framebuffer constants covering visual types,
//! acceleration flags, and rotate values.

// ---------------------------------------------------------------------------
// Framebuffer visual types
// ---------------------------------------------------------------------------

/// Monochrome (1 bit).
pub const FB_VISUAL_MONO01: u32 = 0;
/// Monochrome (0=white).
pub const FB_VISUAL_MONO10: u32 = 1;
/// True color.
pub const FB_VISUAL_TRUECOLOR: u32 = 2;
/// Pseudo color.
pub const FB_VISUAL_PSEUDOCOLOR: u32 = 3;
/// Direct color.
pub const FB_VISUAL_DIRECTCOLOR: u32 = 4;
/// Static pseudo color.
pub const FB_VISUAL_STATIC_PSEUDOCOLOR: u32 = 5;
/// Four CC visual.
pub const FB_VISUAL_FOURCC: u32 = 6;

// ---------------------------------------------------------------------------
// Framebuffer acceleration types
// ---------------------------------------------------------------------------

/// No acceleration.
pub const FB_ACCEL_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Framebuffer type
// ---------------------------------------------------------------------------

/// Packed pixels.
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;
/// Non-interleaved planes.
pub const FB_TYPE_PLANES: u32 = 1;
/// Interleaved planes.
pub const FB_TYPE_INTERLEAVED_PLANES: u32 = 2;
/// Text (not pixels).
pub const FB_TYPE_TEXT: u32 = 3;
/// EGA/VGA planes.
pub const FB_TYPE_VGA_PLANES: u32 = 4;
/// Four CC format.
pub const FB_TYPE_FOURCC: u32 = 5;

// ---------------------------------------------------------------------------
// Framebuffer rotation
// ---------------------------------------------------------------------------

/// No rotation.
pub const FB_ROTATE_UR: u32 = 0;
/// 90 degrees clockwise.
pub const FB_ROTATE_CW: u32 = 1;
/// 180 degrees.
pub const FB_ROTATE_UD: u32 = 2;
/// 270 degrees clockwise.
pub const FB_ROTATE_CCW: u32 = 3;

// ---------------------------------------------------------------------------
// Framebuffer blank modes
// ---------------------------------------------------------------------------

/// Unblank.
pub const FB_BLANK_UNBLANK: u32 = 0;
/// Normal blank (display off).
pub const FB_BLANK_NORMAL: u32 = 1;
/// VSYNC suspend.
pub const FB_BLANK_VSYNC_SUSPEND: u32 = 2;
/// HSYNC suspend.
pub const FB_BLANK_HSYNC_SUSPEND: u32 = 3;
/// Powerdown.
pub const FB_BLANK_POWERDOWN: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visual_types_distinct() {
        let types = [
            FB_VISUAL_MONO01, FB_VISUAL_MONO10, FB_VISUAL_TRUECOLOR,
            FB_VISUAL_PSEUDOCOLOR, FB_VISUAL_DIRECTCOLOR,
            FB_VISUAL_STATIC_PSEUDOCOLOR, FB_VISUAL_FOURCC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_fb_types_distinct() {
        let types = [
            FB_TYPE_PACKED_PIXELS, FB_TYPE_PLANES,
            FB_TYPE_INTERLEAVED_PLANES, FB_TYPE_TEXT,
            FB_TYPE_VGA_PLANES, FB_TYPE_FOURCC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_rotation_values_distinct() {
        let rots = [FB_ROTATE_UR, FB_ROTATE_CW, FB_ROTATE_UD, FB_ROTATE_CCW];
        for i in 0..rots.len() {
            for j in (i + 1)..rots.len() {
                assert_ne!(rots[i], rots[j]);
            }
        }
    }

    #[test]
    fn test_blank_modes_distinct() {
        let modes = [
            FB_BLANK_UNBLANK, FB_BLANK_NORMAL, FB_BLANK_VSYNC_SUSPEND,
            FB_BLANK_HSYNC_SUSPEND, FB_BLANK_POWERDOWN,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
