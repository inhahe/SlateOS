//! DRM framebuffer object — a pixel buffer that can be scanned out.
//!
//! A framebuffer wraps a GEM buffer with format metadata (dimensions,
//! pitch, pixel format).  Planes reference framebuffers; the CRTC
//! scans out whichever framebuffer is on its primary plane.
//!
//! Framebuffers are separate from GEM objects because:
//! - A GEM object is raw memory; a framebuffer adds layout metadata.
//! - Multiple framebuffers can reference different subregions of one GEM.
//! - Multi-planar formats (YUV) need multiple GEM handles per framebuffer.

use super::DrmObjectId;
use super::mode::PixelFormat;

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

/// A DRM framebuffer object.
pub struct DrmFramebuffer {
    /// Unique object ID.
    pub id: DrmObjectId,
    /// GEM handle of the backing buffer.
    pub gem_handle: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row (may include padding for alignment).
    pub pitch: u32,
    /// Pixel format.
    pub format: PixelFormat,
    /// Byte offset into the GEM buffer (usually 0).
    pub offset: u32,
}
