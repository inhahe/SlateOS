//! `<drm/drm.h>` — DRM lease (display resource delegation) constants.
//!
//! DRM leases allow a process to delegate exclusive access to a
//! subset of display resources (connectors, CRTCs, planes) to
//! another process. This enables multi-seat displays, VR compositors,
//! and per-output display servers without root privileges.

// ---------------------------------------------------------------------------
// Lease flags
// ---------------------------------------------------------------------------

/// Close-on-exec for the lease fd.
pub const DRM_LEASE_FLAGS_CLOEXEC: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Lease object types (used in listing/revoking)
// ---------------------------------------------------------------------------

/// Lease includes a CRTC.
pub const DRM_LEASE_OBJECT_CRTC: u32 = 0;
/// Lease includes a connector.
pub const DRM_LEASE_OBJECT_CONNECTOR: u32 = 1;
/// Lease includes a plane.
pub const DRM_LEASE_OBJECT_PLANE: u32 = 2;

// ---------------------------------------------------------------------------
// Lease states
// ---------------------------------------------------------------------------

/// Lease is active (lessee has access).
pub const DRM_LEASE_STATE_ACTIVE: u32 = 0;
/// Lease has been revoked (lessee lost access).
pub const DRM_LEASE_STATE_REVOKED: u32 = 1;

// ---------------------------------------------------------------------------
// DRM master capabilities / auth
// ---------------------------------------------------------------------------

/// Client is DRM master (can set modes, create leases).
pub const DRM_CLIENT_CAP_MASTER: u32 = 0;
/// Client supports atomic modesetting.
pub const DRM_CLIENT_CAP_ATOMIC: u64 = 2;
/// Client supports universal planes.
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 3;
/// Client supports aspect ratio mode info.
pub const DRM_CLIENT_CAP_ASPECT_RATIO: u64 = 4;
/// Client supports writeback connectors.
pub const DRM_CLIENT_CAP_WRITEBACK_CONNECTORS: u64 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lease_flag() {
        assert_eq!(DRM_LEASE_FLAGS_CLOEXEC, 1);
    }

    #[test]
    fn test_object_types_distinct() {
        assert_ne!(DRM_LEASE_OBJECT_CRTC, DRM_LEASE_OBJECT_CONNECTOR);
        assert_ne!(DRM_LEASE_OBJECT_CONNECTOR, DRM_LEASE_OBJECT_PLANE);
        assert_ne!(DRM_LEASE_OBJECT_CRTC, DRM_LEASE_OBJECT_PLANE);
    }

    #[test]
    fn test_lease_states() {
        assert_ne!(DRM_LEASE_STATE_ACTIVE, DRM_LEASE_STATE_REVOKED);
    }

    #[test]
    fn test_client_caps_distinct() {
        let caps = [
            DRM_CLIENT_CAP_ATOMIC, DRM_CLIENT_CAP_UNIVERSAL_PLANES,
            DRM_CLIENT_CAP_ASPECT_RATIO, DRM_CLIENT_CAP_WRITEBACK_CONNECTORS,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}
