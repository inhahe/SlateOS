//! DRM CRTC — a scanout engine.
//!
//! A CRTC (CRT Controller) reads pixel data from a framebuffer and
//! drives a connector through an encoder.  It controls timing (mode),
//! gamma correction, and which planes are visible.
//!
//! In our simplified model, each CRTC has exactly one primary plane
//! and optionally a cursor plane.  Overlay planes are added later.

use super::DrmObjectId;
use super::mode::DrmMode;

// ---------------------------------------------------------------------------
// CRTC
// ---------------------------------------------------------------------------

/// A DRM CRTC — one scanout engine.
pub struct DrmCrtc {
    /// Unique object ID.
    pub id: DrmObjectId,
    /// Whether this CRTC is actively scanning out.
    pub active: bool,
    /// Current display mode (resolution + timing).
    pub mode: Option<DrmMode>,
    /// The primary plane for this CRTC.
    pub primary_plane: DrmObjectId,
    /// Optional cursor plane.
    pub cursor_plane: Option<DrmObjectId>,
    /// Gamma LUT size (0 = no gamma support).
    pub gamma_size: u32,
    /// Index of this CRTC within the device's CRTC list.
    /// Used for the `possible_crtcs` bitmask in planes.
    pub index: u32,
}
