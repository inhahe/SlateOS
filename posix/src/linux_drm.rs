//! `<linux/drm.h>` — Direct Rendering Manager base ioctls.
//!
//! DRM is the kernel framework for GPU access. These are the
//! device-agnostic base ioctls; driver-specific ioctls (i915, amdgpu,
//! etc.) extend this set.

// ---------------------------------------------------------------------------
// DRM ioctl base
// ---------------------------------------------------------------------------

/// DRM ioctl base number.
pub const DRM_IOCTL_BASE: u8 = b'd';

// ---------------------------------------------------------------------------
// DRM ioctl commands
// ---------------------------------------------------------------------------

/// Get DRM version.
pub const DRM_IOCTL_VERSION: u64 = 0xC0406400;
/// Get unique bus ID string.
pub const DRM_IOCTL_GET_UNIQUE: u64 = 0xC0106401;
/// Get magic authentication token.
pub const DRM_IOCTL_GET_MAGIC: u64 = 0x80046402;
/// Set client capabilities.
pub const DRM_IOCTL_SET_CLIENT_CAP: u64 = 0x40106410;
/// Get device capabilities.
pub const DRM_IOCTL_GET_CAP: u64 = 0xC010640C;

// GEM (Graphics Execution Manager)
/// Close a GEM handle.
pub const DRM_IOCTL_GEM_CLOSE: u64 = 0x40086409;
/// Convert GEM name to handle (flink).
pub const DRM_IOCTL_GEM_FLINK: u64 = 0xC008640A;
/// Open a GEM name.
pub const DRM_IOCTL_GEM_OPEN: u64 = 0xC010640B;

// Mode setting
/// Get resources (connectors, CRTCs, encoders).
pub const DRM_IOCTL_MODE_GETRESOURCES: u64 = 0xC04064A0;
/// Get connector info.
pub const DRM_IOCTL_MODE_GETCONNECTOR: u64 = 0xC05064A7;
/// Get encoder info.
pub const DRM_IOCTL_MODE_GETENCODER: u64 = 0xC01464A6;
/// Get CRTC info.
pub const DRM_IOCTL_MODE_GETCRTC: u64 = 0xC06864A1;
/// Set CRTC.
pub const DRM_IOCTL_MODE_SETCRTC: u64 = 0xC06864A2;
/// Add framebuffer.
pub const DRM_IOCTL_MODE_ADDFB: u64 = 0xC01C64AE;
/// Add framebuffer (v2, multi-planar).
pub const DRM_IOCTL_MODE_ADDFB2: u64 = 0xC04C64B8;
/// Remove framebuffer.
pub const DRM_IOCTL_MODE_RMFB: u64 = 0xC00464AF;
/// Page flip.
pub const DRM_IOCTL_MODE_PAGE_FLIP: u64 = 0xC01864B0;
/// Atomic modeset.
pub const DRM_IOCTL_MODE_ATOMIC: u64 = 0xC03864BC;
/// Create dumb buffer.
pub const DRM_IOCTL_MODE_CREATE_DUMB: u64 = 0xC02064B2;
/// Map dumb buffer.
pub const DRM_IOCTL_MODE_MAP_DUMB: u64 = 0xC01064B3;
/// Destroy dumb buffer.
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u64 = 0xC00464B4;
/// Get plane resources.
pub const DRM_IOCTL_MODE_GETPLANERESOURCES: u64 = 0xC01064B5;
/// Get plane info.
pub const DRM_IOCTL_MODE_GETPLANE: u64 = 0xC02064B6;

// PRIME (fd ↔ handle)
/// Export handle to DMA-BUF fd.
pub const DRM_IOCTL_PRIME_HANDLE_TO_FD: u64 = 0xC00C642D;
/// Import DMA-BUF fd to handle.
pub const DRM_IOCTL_PRIME_FD_TO_HANDLE: u64 = 0xC00C642E;

// ---------------------------------------------------------------------------
// DRM capabilities
// ---------------------------------------------------------------------------

/// DRM_CAP_DUMB_BUFFER: device supports dumb buffers.
pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
/// DRM_CAP_PRIME: PRIME (fd passing) support.
pub const DRM_CAP_PRIME: u64 = 0x5;
/// DRM_CAP_TIMESTAMP_MONOTONIC: vblank timestamps use CLOCK_MONOTONIC.
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
/// DRM_CAP_ASYNC_PAGE_FLIP: async page flip support.
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x7;
/// DRM_CAP_ADDFB2_MODIFIERS: supports modifiers in ADDFB2.
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
/// DRM_CAP_CRTC_IN_VBLANK_EVENT: CRTC info in vblank events.
pub const DRM_CAP_CRTC_IN_VBLANK_EVENT: u64 = 0x12;

/// PRIME import capability.
pub const DRM_PRIME_CAP_IMPORT: u64 = 0x1;
/// PRIME export capability.
pub const DRM_PRIME_CAP_EXPORT: u64 = 0x2;

// ---------------------------------------------------------------------------
// DRM client capabilities
// ---------------------------------------------------------------------------

/// Client wants stereo 3D modes.
pub const DRM_CLIENT_CAP_STEREO_3D: u64 = 1;
/// Client wants universal planes.
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;
/// Client supports atomic modesetting.
pub const DRM_CLIENT_CAP_ATOMIC: u64 = 3;
/// Client wants writeback connectors.
pub const DRM_CLIENT_CAP_WRITEBACK_CONNECTORS: u64 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_base() {
        assert_eq!(DRM_IOCTL_BASE, b'd');
    }

    #[test]
    fn test_core_ioctls_distinct() {
        let cmds = [
            DRM_IOCTL_VERSION, DRM_IOCTL_GET_UNIQUE,
            DRM_IOCTL_GET_MAGIC, DRM_IOCTL_SET_CLIENT_CAP,
            DRM_IOCTL_GET_CAP,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_modeset_ioctls_distinct() {
        let cmds = [
            DRM_IOCTL_MODE_GETRESOURCES, DRM_IOCTL_MODE_GETCONNECTOR,
            DRM_IOCTL_MODE_GETCRTC, DRM_IOCTL_MODE_SETCRTC,
            DRM_IOCTL_MODE_ADDFB, DRM_IOCTL_MODE_ADDFB2,
            DRM_IOCTL_MODE_RMFB, DRM_IOCTL_MODE_PAGE_FLIP,
            DRM_IOCTL_MODE_ATOMIC, DRM_IOCTL_MODE_CREATE_DUMB,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_prime_ioctls() {
        assert_ne!(DRM_IOCTL_PRIME_HANDLE_TO_FD, DRM_IOCTL_PRIME_FD_TO_HANDLE);
    }

    #[test]
    fn test_client_caps_distinct() {
        let caps = [
            DRM_CLIENT_CAP_STEREO_3D,
            DRM_CLIENT_CAP_UNIVERSAL_PLANES,
            DRM_CLIENT_CAP_ATOMIC,
            DRM_CLIENT_CAP_WRITEBACK_CONNECTORS,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_drm_caps_distinct() {
        let caps = [
            DRM_CAP_DUMB_BUFFER, DRM_CAP_PRIME,
            DRM_CAP_TIMESTAMP_MONOTONIC, DRM_CAP_ASYNC_PAGE_FLIP,
            DRM_CAP_ADDFB2_MODIFIERS, DRM_CAP_CRTC_IN_VBLANK_EVENT,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}
