//! `<linux/fb.h>` — framebuffer device interface.
//!
//! Provides ioctl constants and data structures for the Linux
//! framebuffer device (`/dev/fb*`).

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Get variable screen info.
pub const FBIOGET_VSCREENINFO: u64 = 0x4600;
/// Set variable screen info.
pub const FBIOPUT_VSCREENINFO: u64 = 0x4601;
/// Get fixed screen info.
pub const FBIOGET_FSCREENINFO: u64 = 0x4602;
/// Get colormap.
pub const FBIOGETCMAP: u64 = 0x4604;
/// Set colormap.
pub const FBIOPUTCMAP: u64 = 0x4605;
/// Pan display (scroll).
pub const FBIOPAN_DISPLAY: u64 = 0x4606;
/// Blank/unblank display.
pub const FBIOBLANK: u64 = 0x4611;

// ---------------------------------------------------------------------------
// Framebuffer types (fix info)
// ---------------------------------------------------------------------------

/// Packed pixels.
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;
/// Non-interleaved planes.
pub const FB_TYPE_PLANES: u32 = 1;
/// Interleaved planes.
pub const FB_TYPE_INTERLEAVED_PLANES: u32 = 2;
/// Text (non-graphic).
pub const FB_TYPE_TEXT: u32 = 3;
/// VGA planes.
pub const FB_TYPE_VGA_PLANES: u32 = 4;
/// Four CC format.
pub const FB_TYPE_FOURCC: u32 = 5;

// ---------------------------------------------------------------------------
// Visual types (fix info)
// ---------------------------------------------------------------------------

/// Monochrome (1-bit).
pub const FB_VISUAL_MONO01: u32 = 0;
/// Monochrome (1-bit, inverted).
pub const FB_VISUAL_MONO10: u32 = 1;
/// True color.
pub const FB_VISUAL_TRUECOLOR: u32 = 2;
/// Pseudo color (indexed palette).
pub const FB_VISUAL_PSEUDOCOLOR: u32 = 3;
/// Direct color.
pub const FB_VISUAL_DIRECTCOLOR: u32 = 4;
/// Static pseudo color.
pub const FB_VISUAL_STATIC_PSEUDOCOLOR: u32 = 5;
/// Four CC visual.
pub const FB_VISUAL_FOURCC: u32 = 6;

// ---------------------------------------------------------------------------
// Blank modes
// ---------------------------------------------------------------------------

/// Display is on.
pub const FB_BLANK_UNBLANK: i32 = 0;
/// Display standby.
pub const FB_BLANK_NORMAL: i32 = 1;
/// VESA standby.
pub const FB_BLANK_VSYNC_SUSPEND: i32 = 2;
/// VESA suspend.
pub const FB_BLANK_HSYNC_SUSPEND: i32 = 3;
/// Display off.
pub const FB_BLANK_POWERDOWN: i32 = 4;

// ---------------------------------------------------------------------------
// Fixed screen info
// ---------------------------------------------------------------------------

/// Fixed screen information (hardware specific).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FbFixScreeninfo {
    /// Identification string (e.g. "vesa VGA").
    pub id: [u8; 16],
    /// Start of framebuffer memory (physical address).
    pub smem_start: u64,
    /// Length of framebuffer memory.
    pub smem_len: u32,
    /// Framebuffer type (FB_TYPE_*).
    pub type_: u32,
    /// Type aux (interleave for planes).
    pub type_aux: u32,
    /// Visual type (FB_VISUAL_*).
    pub visual: u32,
    /// X pan step (0 = no panning).
    pub xpanstep: u16,
    /// Y pan step (0 = no panning).
    pub ypanstep: u16,
    /// Y wrap step (0 = no wrapping).
    pub ywrapstep: u16,
    /// Padding.
    _pad: u16,
    /// Line length (bytes per scanline).
    pub line_length: u32,
    /// Memory-mapped I/O start (physical).
    pub mmio_start: u64,
    /// MMIO length.
    pub mmio_len: u32,
    /// Acceleration hardware type.
    pub accel: u32,
    /// Capabilities (bit flags).
    pub capabilities: u16,
    /// Reserved.
    _reserved: [u16; 3],
}

/// Variable screen information (user-changeable).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FbVarScreeninfo {
    /// Visible horizontal resolution.
    pub xres: u32,
    /// Visible vertical resolution.
    pub yres: u32,
    /// Virtual horizontal resolution.
    pub xres_virtual: u32,
    /// Virtual vertical resolution.
    pub yres_virtual: u32,
    /// Horizontal offset to virtual resolution.
    pub xoffset: u32,
    /// Vertical offset to virtual resolution.
    pub yoffset: u32,
    /// Bits per pixel.
    pub bits_per_pixel: u32,
    /// Grayscale flag (0 = color, 1 = grayscale).
    pub grayscale: u32,
    /// Red channel: offset, length, msb_right.
    pub red_offset: u32,
    pub red_length: u32,
    pub red_msb_right: u32,
    /// Green channel.
    pub green_offset: u32,
    pub green_length: u32,
    pub green_msb_right: u32,
    /// Blue channel.
    pub blue_offset: u32,
    pub blue_length: u32,
    pub blue_msb_right: u32,
    /// Alpha channel.
    pub transp_offset: u32,
    pub transp_length: u32,
    pub transp_msb_right: u32,
    /// Non-standard pixel format.
    pub nonstd: u32,
    /// Activation flags.
    pub activate: u32,
    /// Physical height (mm).
    pub height: u32,
    /// Physical width (mm).
    pub width: u32,
    /// Deprecated — acceleration flags.
    pub accel_flags: u32,
    /// Pixel clock (pico seconds).
    pub pixclock: u32,
    /// Left margin.
    pub left_margin: u32,
    /// Right margin.
    pub right_margin: u32,
    /// Upper margin.
    pub upper_margin: u32,
    /// Lower margin.
    pub lower_margin: u32,
    /// Horizontal sync length.
    pub hsync_len: u32,
    /// Vertical sync length.
    pub vsync_len: u32,
    /// Sync flags.
    pub sync: u32,
    /// VESA mode flag.
    pub vmode: u32,
    /// Rotation angle (0, 90, 180, 270).
    pub rotate: u32,
    /// Color space.
    pub colorspace: u32,
    /// Reserved.
    _reserved: [u32; 4],
}

impl FbFixScreeninfo {
    /// Create a zeroed `FbFixScreeninfo`.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

impl FbVarScreeninfo {
    /// Create a zeroed `FbVarScreeninfo`.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fb_fix_screeninfo_size() {
        assert!(core::mem::size_of::<FbFixScreeninfo>() >= 64);
    }

    #[test]
    fn test_fb_var_screeninfo_size() {
        assert!(core::mem::size_of::<FbVarScreeninfo>() >= 100);
    }

    #[test]
    fn test_var_screeninfo_zeroed() {
        let info = FbVarScreeninfo::zeroed();
        assert_eq!(info.xres, 0);
        assert_eq!(info.yres, 0);
        assert_eq!(info.bits_per_pixel, 0);
    }

    #[test]
    fn test_fix_screeninfo_zeroed() {
        let info = FbFixScreeninfo::zeroed();
        assert_eq!(info.smem_len, 0);
        assert_eq!(info.type_, FB_TYPE_PACKED_PIXELS);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            FBIOGET_VSCREENINFO, FBIOPUT_VSCREENINFO,
            FBIOGET_FSCREENINFO, FBIOGETCMAP, FBIOPUTCMAP,
            FBIOPAN_DISPLAY, FBIOBLANK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_fb_types_sequential() {
        assert_eq!(FB_TYPE_PACKED_PIXELS, 0);
        assert_eq!(FB_TYPE_FOURCC, 5);
    }

    #[test]
    fn test_visual_types_sequential() {
        assert_eq!(FB_VISUAL_MONO01, 0);
        assert_eq!(FB_VISUAL_FOURCC, 6);
    }

    #[test]
    fn test_blank_modes() {
        assert_eq!(FB_BLANK_UNBLANK, 0);
        assert_eq!(FB_BLANK_POWERDOWN, 4);
    }

    #[test]
    fn test_typical_1080p_setup() {
        let mut info = FbVarScreeninfo::zeroed();
        info.xres = 1920;
        info.yres = 1080;
        info.xres_virtual = 1920;
        info.yres_virtual = 1080;
        info.bits_per_pixel = 32;
        // ARGB8888 layout
        info.red_offset = 16;
        info.red_length = 8;
        info.green_offset = 8;
        info.green_length = 8;
        info.blue_offset = 0;
        info.blue_length = 8;
        info.transp_offset = 24;
        info.transp_length = 8;

        assert_eq!(info.xres * info.yres * (info.bits_per_pixel / 8),
                   1920 * 1080 * 4);
    }
}
