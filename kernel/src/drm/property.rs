//! DRM property system for atomic modesetting.
//!
//! Properties are named key-value pairs attached to DRM objects.
//! The atomic API uses properties to describe state changes.
//! This is a simplified version of Linux's property system —
//! we use strongly-typed enum values instead of generic u64 blobs.
//!
//! For now this module defines the property types; the full property
//! infrastructure (per-object property tables, get/set API) will be
//! added when the userspace syscall interface is implemented.

// ---------------------------------------------------------------------------
// Property types
// ---------------------------------------------------------------------------

/// Standard DRM properties that can be set via atomic commits.
///
/// Each variant corresponds to a well-known property on a specific
/// object type.  Custom driver-specific properties can be added as
/// new variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrmProperty {
    // --- CRTC properties ---
    /// Whether the CRTC is active (bool).
    CrtcActive,
    /// Display mode (index into connector's mode list).
    CrtcModeId,

    // --- Plane properties ---
    /// Framebuffer object ID for the plane.
    PlaneFbId,
    /// CRTC that the plane is bound to.
    PlaneCrtcId,
    /// Source X (16.16 fixed point).
    PlaneSrcX,
    /// Source Y (16.16 fixed point).
    PlaneSrcY,
    /// Source width (16.16 fixed point).
    PlaneSrcW,
    /// Source height (16.16 fixed point).
    PlaneSrcH,
    /// Destination X (integer).
    PlaneCrtcX,
    /// Destination Y (integer).
    PlaneCrtcY,
    /// Destination width (integer).
    PlaneCrtcW,
    /// Destination height (integer).
    PlaneCrtcH,

    // --- Connector properties ---
    /// CRTC ID that the connector is bound to.
    ConnectorCrtcId,
}

impl DrmProperty {
    /// Human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CrtcActive => "ACTIVE",
            Self::CrtcModeId => "MODE_ID",
            Self::PlaneFbId => "FB_ID",
            Self::PlaneCrtcId => "CRTC_ID",
            Self::PlaneSrcX => "SRC_X",
            Self::PlaneSrcY => "SRC_Y",
            Self::PlaneSrcW => "SRC_W",
            Self::PlaneSrcH => "SRC_H",
            Self::PlaneCrtcX => "CRTC_X",
            Self::PlaneCrtcY => "CRTC_Y",
            Self::PlaneCrtcW => "CRTC_W",
            Self::PlaneCrtcH => "CRTC_H",
            Self::ConnectorCrtcId => "CRTC_ID",
        }
    }
}
