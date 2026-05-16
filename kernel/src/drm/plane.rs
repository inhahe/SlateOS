//! DRM plane — a compositing layer within a CRTC.
//!
//! Each CRTC has at least a primary plane (the main framebuffer).
//! Optional cursor and overlay planes allow hardware-accelerated
//! compositing without CPU intervention.

extern crate alloc;
use alloc::vec::Vec;

use super::DrmObjectId;
use super::mode::PixelFormat;

// ---------------------------------------------------------------------------
// Plane
// ---------------------------------------------------------------------------

/// Type of plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneType {
    /// The main framebuffer — every CRTC has exactly one.
    Primary,
    /// Hardware cursor overlay.
    Cursor,
    /// Additional overlay plane (for video, picture-in-picture, etc.).
    Overlay,
}

/// A DRM plane — one compositing layer.
pub struct DrmPlane {
    /// Unique object ID.
    pub id: DrmObjectId,
    /// Type of plane.
    pub plane_type: PlaneType,
    /// Bitmask of CRTC indices that can use this plane.
    /// Bit N = this plane can be assigned to CRTC N.
    pub possible_crtcs: u32,
    /// Pixel formats supported by this plane.
    pub formats: Vec<PixelFormat>,
    /// Currently assigned framebuffer (None = plane disabled).
    pub fb: Option<DrmObjectId>,
    /// Currently assigned CRTC (None = unbound).
    pub crtc: Option<DrmObjectId>,
    /// Source rectangle (in framebuffer coordinates, 16.16 fixed point).
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
    /// Destination rectangle (in CRTC coordinates, integer pixels).
    pub dst_x: i32,
    pub dst_y: i32,
    pub dst_w: u32,
    pub dst_h: u32,
}
