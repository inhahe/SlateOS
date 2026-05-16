//! Framebuffer pixel format and visual type constants.
//!
//! Supplements linux_fb.rs with pixel format definitions,
//! visual types, and acceleration capabilities used by
//! framebuffer drivers and userspace graphics code.

// ---------------------------------------------------------------------------
// Framebuffer pixel types (FB_TYPE_*)
// ---------------------------------------------------------------------------

/// Packed pixels.
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;
/// Non-interleaved planes.
pub const FB_TYPE_PLANES: u32 = 1;
/// Interleaved planes.
pub const FB_TYPE_INTERLEAVED_PLANES: u32 = 2;
/// Text / character cells.
pub const FB_TYPE_TEXT: u32 = 3;
/// VGA planes.
pub const FB_TYPE_VGA_PLANES: u32 = 4;
/// Four-CC pixel format.
pub const FB_TYPE_FOURCC: u32 = 5;

// ---------------------------------------------------------------------------
// Framebuffer visual types (FB_VISUAL_*)
// ---------------------------------------------------------------------------

/// Monochrome (1 bit per pixel).
pub const FB_VISUAL_MONO01: u32 = 0;
/// Monochrome (inverted).
pub const FB_VISUAL_MONO10: u32 = 1;
/// True color (direct RGB).
pub const FB_VISUAL_TRUECOLOR: u32 = 2;
/// Pseudo color (palette).
pub const FB_VISUAL_PSEUDOCOLOR: u32 = 3;
/// Direct color.
pub const FB_VISUAL_DIRECTCOLOR: u32 = 4;
/// Static pseudo color (read-only palette).
pub const FB_VISUAL_STATIC_PSEUDOCOLOR: u32 = 5;
/// Four-CC visual.
pub const FB_VISUAL_FOURCC: u32 = 6;

// ---------------------------------------------------------------------------
// Framebuffer acceleration (FB_ACCEL_*)
// ---------------------------------------------------------------------------

/// No hardware acceleration.
pub const FB_ACCEL_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Framebuffer rotation
// ---------------------------------------------------------------------------

/// No rotation.
pub const FB_ROTATE_UR: u32 = 0;
/// 90° clockwise.
pub const FB_ROTATE_CW: u32 = 1;
/// 180° rotation.
pub const FB_ROTATE_UD: u32 = 2;
/// 270° clockwise (90° counter-clockwise).
pub const FB_ROTATE_CCW: u32 = 3;

// ---------------------------------------------------------------------------
// Framebuffer blanking modes (FB_BLANK_*)
// ---------------------------------------------------------------------------

/// Unblank.
pub const FB_BLANK_UNBLANK: u32 = 0;
/// Normal blanking (screen dark).
pub const FB_BLANK_NORMAL: u32 = 1;
/// VESA blanking — HSYNC off.
pub const FB_BLANK_VSYNC_SUSPEND: u32 = 2;
/// VESA blanking — VSYNC off.
pub const FB_BLANK_HSYNC_SUSPEND: u32 = 3;
/// DPMS off (full power down).
pub const FB_BLANK_POWERDOWN: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
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
    fn test_visuals_distinct() {
        let vis = [
            FB_VISUAL_MONO01, FB_VISUAL_MONO10,
            FB_VISUAL_TRUECOLOR, FB_VISUAL_PSEUDOCOLOR,
            FB_VISUAL_DIRECTCOLOR, FB_VISUAL_STATIC_PSEUDOCOLOR,
            FB_VISUAL_FOURCC,
        ];
        for i in 0..vis.len() {
            for j in (i + 1)..vis.len() {
                assert_ne!(vis[i], vis[j]);
            }
        }
    }

    #[test]
    fn test_rotations_distinct() {
        let rots = [FB_ROTATE_UR, FB_ROTATE_CW, FB_ROTATE_UD, FB_ROTATE_CCW];
        for i in 0..rots.len() {
            for j in (i + 1)..rots.len() {
                assert_ne!(rots[i], rots[j]);
            }
        }
    }

    #[test]
    fn test_blanking_distinct() {
        let blanks = [
            FB_BLANK_UNBLANK, FB_BLANK_NORMAL,
            FB_BLANK_VSYNC_SUSPEND, FB_BLANK_HSYNC_SUSPEND,
            FB_BLANK_POWERDOWN,
        ];
        for i in 0..blanks.len() {
            for j in (i + 1)..blanks.len() {
                assert_ne!(blanks[i], blanks[j]);
            }
        }
    }
}
