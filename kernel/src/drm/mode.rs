//! Display mode and pixel format definitions.
//!
//! A [`DrmMode`] describes a display configuration: resolution, refresh
//! rate, and timing parameters.  For paravirtualized backends (Limine,
//! virtio-gpu) most timing fields are synthetic; for real hardware
//! drivers they become the actual CRTC timing register values.
//!
//! [`PixelFormat`] enumerates supported pixel layouts.  The existing
//! kernel code assumes BGRA8888 (what Limine and virtio-gpu provide);
//! this enum makes the assumption explicit and extensible.

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Pixel format
// ---------------------------------------------------------------------------

/// Pixel storage format.
///
/// Naming convention follows DRM/Linux fourcc:
/// - Letter order is channel order in increasing memory address.
/// - The suffix is bits per channel.
///
/// Example: `Xrgb8888` means bytes in memory are B, G, R, X (little-endian
/// u32 = 0xXXRRGGBB).  X = don't-care (alpha ignored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PixelFormat {
    /// 32 bpp, alpha ignored.  Memory: B G R X (LE u32 = 0x__RRGGBB).
    /// This is what Limine and virtio-gpu typically provide.
    Xrgb8888 = 0x3432_3258, // 'X', 'R', '2', '4' in ASCII → fourcc XR24
    /// 32 bpp with alpha.  Memory: B G R A (LE u32 = 0xAARRGGBB).
    Argb8888 = 0x3432_5241, // AR24
    /// 32 bpp, alpha ignored.  Memory: R G B X (LE u32 = 0x__BBGGRR).
    Xbgr8888 = 0x3432_4258, // XB24
    /// 32 bpp with alpha.  Memory: R G B A (LE u32 = 0xAABBGGRR).
    Abgr8888 = 0x3432_4241, // AB24
    /// 16 bpp.  Memory: G[2:0]B[4:0], R[4:0]G[5:3] (LE u16).
    Rgb565 = 0x3631_4752, // RG16
}

impl PixelFormat {
    /// Bytes per pixel for this format.
    #[must_use]
    pub const fn bpp(self) -> u32 {
        match self {
            Self::Xrgb8888 | Self::Argb8888 | Self::Xbgr8888 | Self::Abgr8888 => 4,
            Self::Rgb565 => 2,
        }
    }

    /// Compute pitch (bytes per row) for the given width.
    ///
    /// Aligns to 64-byte boundary for cache-line efficiency and GPU
    /// alignment requirements.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn pitch(self, width: u32) -> u32 {
        let raw = width * self.bpp();
        // Round up to next 64-byte boundary.
        (raw + 63) & !63
    }

    /// Human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Xrgb8888 => "XRGB8888",
            Self::Argb8888 => "ARGB8888",
            Self::Xbgr8888 => "XBGR8888",
            Self::Abgr8888 => "ABGR8888",
            Self::Rgb565 => "RGB565",
        }
    }

    /// Whether this format has an alpha channel.
    #[must_use]
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Argb8888 | Self::Abgr8888)
    }
}

// ---------------------------------------------------------------------------
// Display mode
// ---------------------------------------------------------------------------

/// A display mode (resolution + timing).
///
/// Modeled after Linux `drm_display_mode` but simplified.  For
/// paravirtualized backends most timing fields are zero (the host
/// handles actual signal timing).  For real hardware drivers, these
/// become the CRTC timing register values.
#[derive(Debug, Clone, Copy)]
pub struct DrmMode {
    /// Horizontal active pixels.
    pub hdisplay: u32,
    /// Vertical active lines.
    pub vdisplay: u32,
    /// Refresh rate in Hz.
    pub vrefresh: u32,
    /// Pixel clock in kHz (0 for virtual displays).
    pub clock: u32,
    /// Total horizontal pixels (active + blanking).
    pub htotal: u32,
    /// Total vertical lines (active + blanking).
    pub vtotal: u32,
    /// Mode flags (interlace, doublescan, etc.).
    pub flags: DrmModeFlags,
    /// Human-readable name (e.g., "1920x1080@60").
    pub name: [u8; 32],
    /// Length of the name string.
    pub name_len: usize,
}

impl DrmMode {
    /// Create a mode from resolution and refresh rate.
    ///
    /// Timing parameters are synthesized (suitable for virtual displays).
    #[must_use]
    pub fn from_resolution(width: u32, height: u32, hz: u32) -> Self {
        let mut name = [0u8; 32];
        let name_len = format_mode_name(&mut name, width, height, hz);
        Self {
            hdisplay: width,
            vdisplay: height,
            vrefresh: hz,
            clock: 0,
            htotal: width,
            vtotal: height,
            flags: DrmModeFlags::empty(),
            name,
            name_len,
        }
    }

    /// Get the name as a byte slice.
    #[must_use]
    pub fn name_str(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

impl PartialEq for DrmMode {
    fn eq(&self, other: &Self) -> bool {
        self.hdisplay == other.hdisplay
            && self.vdisplay == other.vdisplay
            && self.vrefresh == other.vrefresh
    }
}

impl Eq for DrmMode {}

/// Mode flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrmModeFlags(u32);

impl DrmModeFlags {
    /// No flags.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Interlaced mode.
    pub const INTERLACE: Self = Self(1 << 0);
    /// Double-scan mode.
    pub const DOUBLESCAN: Self = Self(1 << 1);
    /// Preferred mode (as reported by EDID).
    pub const PREFERRED: Self = Self(1 << 2);
}

/// Format a mode name like "1920x1080@60" into a fixed buffer.
///
/// Returns the number of bytes written.
fn format_mode_name(buf: &mut [u8; 32], w: u32, h: u32, hz: u32) -> usize {
    // Simple integer-to-string without alloc.
    let mut pos = 0;
    pos = write_u32(buf, pos, w);
    if pos < 31 {
        buf[pos] = b'x';
        pos = pos.saturating_add(1);
    }
    pos = write_u32(buf, pos, h);
    if pos < 31 {
        buf[pos] = b'@';
        pos = pos.saturating_add(1);
    }
    pos = write_u32(buf, pos, hz);
    pos
}

/// Write a u32 as decimal digits into `buf` starting at `pos`.
fn write_u32(buf: &mut [u8; 32], mut pos: usize, mut val: u32) -> usize {
    if val == 0 {
        if pos < 32 {
            buf[pos] = b'0';
            return pos.saturating_add(1);
        }
        return pos;
    }
    // Write digits in reverse, then swap.
    let start = pos;
    while val > 0 && pos < 32 {
        #[allow(clippy::arithmetic_side_effects)]
        {
            buf[pos] = b'0' + (val % 10) as u8;
            val /= 10;
        }
        pos = pos.saturating_add(1);
    }
    // Reverse the digits.
    if start < pos {
        let mut i = start;
        #[allow(clippy::arithmetic_side_effects)]
        let mut j = pos - 1;
        while i < j {
            buf.swap(i, j);
            i = i.saturating_add(1);
            j = j.saturating_sub(1);
        }
    }
    pos
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub(crate) fn self_test() -> KernelResult<()> {
    // PixelFormat bpp.
    assert_eq!(PixelFormat::Xrgb8888.bpp(), 4);
    assert_eq!(PixelFormat::Rgb565.bpp(), 2);

    // PixelFormat pitch (64-byte aligned).
    assert_eq!(PixelFormat::Xrgb8888.pitch(1920), 7680); // 1920*4 = 7680, already 64-aligned
    assert_eq!(PixelFormat::Xrgb8888.pitch(100), 448); // 100*4 = 400, next 64 = 448

    // Mode name formatting.
    let mode = DrmMode::from_resolution(1920, 1080, 60);
    let name = mode.name_str();
    if name != b"1920x1080@60" {
        serial_println!("[drm]   FAIL: mode name mismatch");
        return Err(KernelError::InternalError);
    }

    serial_println!("[drm]   PixelFormat + DrmMode: OK");
    Ok(())
}
