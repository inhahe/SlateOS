//! `<linux/drm.h>` — DRM (Direct Rendering Manager) base constants.
//!
//! DRM provides the kernel interface for GPU management: mode setting
//! (display configuration), memory management (GEM/TTM buffer objects),
//! command submission, and synchronization. Userspace graphics stacks
//! (Mesa, Vulkan drivers) communicate with GPUs through DRM ioctls.

// ---------------------------------------------------------------------------
// DRM ioctl base
// ---------------------------------------------------------------------------

/// DRM ioctl magic number.
pub const DRM_IOCTL_BASE: u8 = b'd';

// ---------------------------------------------------------------------------
// DRM capabilities (DRM_CAP_*)
// ---------------------------------------------------------------------------

/// Can create dumb buffers.
pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
/// Can use VBLANK timestamps.
pub const DRM_CAP_VBLANK_HIGH_CRTC: u64 = 0x2;
/// Supports preferred depth for dumb buffers.
pub const DRM_CAP_DUMB_PREFERRED_DEPTH: u64 = 0x3;
/// Prefers shadow buffer for dumb buffers.
pub const DRM_CAP_DUMB_PREFER_SHADOW: u64 = 0x4;
/// Supports PRIME fd passing.
pub const DRM_CAP_PRIME: u64 = 0x5;
/// High-precision vblank timestamps.
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
/// Async page flip support.
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x7;
/// Cursor width.
pub const DRM_CAP_CURSOR_WIDTH: u64 = 0x8;
/// Cursor height.
pub const DRM_CAP_CURSOR_HEIGHT: u64 = 0x9;
/// Supports addfb2 modifiers.
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
/// Supports writeback connectors.
pub const DRM_CAP_WRITEBACK_CONNECTORS: u64 = 0x11;
/// Supports atomic modesetting.
pub const DRM_CAP_ATOMIC_ASYNC_PAGE_FLIP: u64 = 0x15;

// ---------------------------------------------------------------------------
// DRM PRIME flags
// ---------------------------------------------------------------------------

/// Can import PRIME fds.
pub const DRM_PRIME_CAP_IMPORT: u32 = 0x1;
/// Can export PRIME fds.
pub const DRM_PRIME_CAP_EXPORT: u32 = 0x2;

// ---------------------------------------------------------------------------
// DRM client capabilities
// ---------------------------------------------------------------------------

/// Understand stereo 3D modes.
pub const DRM_CLIENT_CAP_STEREO_3D: u64 = 1;
/// Understand universal planes.
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;
/// Understand atomic modesetting.
pub const DRM_CLIENT_CAP_ATOMIC: u64 = 3;
/// Understand writeback connectors.
pub const DRM_CLIENT_CAP_WRITEBACK_CONNECTORS: u64 = 5;

// ---------------------------------------------------------------------------
// GEM (Graphics Execution Manager) operations
// ---------------------------------------------------------------------------

/// Close a GEM handle.
pub const DRM_GEM_CLOSE: u32 = 0x09;
/// Open a GEM name (flink).
pub const DRM_GEM_FLINK: u32 = 0x0A;
/// Open from GEM name.
pub const DRM_GEM_OPEN: u32 = 0x0B;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_distinct() {
        let caps = [
            DRM_CAP_DUMB_BUFFER, DRM_CAP_VBLANK_HIGH_CRTC,
            DRM_CAP_DUMB_PREFERRED_DEPTH, DRM_CAP_DUMB_PREFER_SHADOW,
            DRM_CAP_PRIME, DRM_CAP_TIMESTAMP_MONOTONIC,
            DRM_CAP_ASYNC_PAGE_FLIP, DRM_CAP_CURSOR_WIDTH,
            DRM_CAP_CURSOR_HEIGHT, DRM_CAP_ADDFB2_MODIFIERS,
            DRM_CAP_WRITEBACK_CONNECTORS, DRM_CAP_ATOMIC_ASYNC_PAGE_FLIP,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_prime_caps_no_overlap() {
        assert_eq!(DRM_PRIME_CAP_IMPORT & DRM_PRIME_CAP_EXPORT, 0);
    }

    #[test]
    fn test_client_caps_distinct() {
        let caps = [
            DRM_CLIENT_CAP_STEREO_3D, DRM_CLIENT_CAP_UNIVERSAL_PLANES,
            DRM_CLIENT_CAP_ATOMIC, DRM_CLIENT_CAP_WRITEBACK_CONNECTORS,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_gem_ops_distinct() {
        let ops = [DRM_GEM_CLOSE, DRM_GEM_FLINK, DRM_GEM_OPEN];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}
