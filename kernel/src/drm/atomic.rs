//! Atomic modesetting — commit all display state changes at once.
//!
//! The atomic API ensures that display state transitions are either
//! fully applied or fully rejected (no partial updates that leave the
//! display in an inconsistent state).
//!
//! ## Flow
//!
//! 1. Build an [`AtomicState`] describing the desired changes.
//! 2. Call `atomic_check()` to validate (test-only, no hardware change).
//! 3. Call `atomic_commit()` to apply all changes to hardware.
//!
//! ## References
//!
//! - Linux `drivers/gpu/drm/drm_atomic.c`
//! - Linux `include/drm/drm_atomic.h`

extern crate alloc;
use alloc::vec::Vec;

use super::DrmObjectId;
use super::mode::DrmMode;

// ---------------------------------------------------------------------------
// Atomic state
// ---------------------------------------------------------------------------

/// A pending atomic modesetting state.
///
/// Collects all desired changes before committing them in one shot.
pub struct AtomicState {
    /// CRTC state changes.
    pub crtc_changes: Vec<CrtcState>,
    /// Plane state changes.
    pub plane_changes: Vec<PlaneState>,
    /// Connector state changes.
    pub connector_changes: Vec<ConnectorState>,
}

impl AtomicState {
    /// Create an empty atomic state (no changes).
    #[must_use]
    pub fn new() -> Self {
        Self {
            crtc_changes: Vec::new(),
            plane_changes: Vec::new(),
            connector_changes: Vec::new(),
        }
    }

    /// Whether this state has any changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.crtc_changes.is_empty()
            && self.plane_changes.is_empty()
            && self.connector_changes.is_empty()
    }
}

/// Desired state for a CRTC.
pub struct CrtcState {
    /// Which CRTC to modify.
    pub id: DrmObjectId,
    /// Set active state (None = don't change).
    pub active: Option<bool>,
    /// Set display mode (None = don't change, Some(None) = disable).
    pub mode: Option<Option<DrmMode>>,
}

/// Desired state for a plane.
pub struct PlaneState {
    /// Which plane to modify.
    pub id: DrmObjectId,
    /// Framebuffer to display (None = don't change, Some(None) = disable).
    pub fb_id: Option<Option<DrmObjectId>>,
    /// CRTC to bind to (None = don't change, Some(None) = unbind).
    pub crtc_id: Option<Option<DrmObjectId>>,
    /// Source rectangle in framebuffer coordinates.
    pub src_rect: Option<Rect>,
    /// Destination rectangle in CRTC coordinates.
    pub dst_rect: Option<IRect>,
}

/// Desired state for a connector.
pub struct ConnectorState {
    /// Which connector to modify.
    pub id: DrmObjectId,
    /// CRTC to bind to (None = don't change, Some(None) = unbind).
    pub crtc_id: Option<Option<DrmObjectId>>,
}

/// Rectangle (unsigned coordinates, for source rects).
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Rectangle (signed coordinates, for destination rects).
#[derive(Debug, Clone, Copy)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}
