//! `<drm/drm.h>` — DRM ioctl number and capability constants.
//!
//! The DRM subsystem uses ioctls for all userspace interaction.
//! Each DRM ioctl has a base number (0x00-0x3F for core, 0x40-0x9F
//! for driver-specific). Capabilities advertise what the driver and
//! hardware support without requiring test ioctls.

// ---------------------------------------------------------------------------
// DRM ioctl base numbers (DRM_IOCTL_BASE = 'd')
// ---------------------------------------------------------------------------

/// Get driver version info.
pub const DRM_IOCTL_NR_VERSION: u32 = 0x00;
/// Get unique (bus ID) string.
pub const DRM_IOCTL_NR_GET_UNIQUE: u32 = 0x01;
/// Authenticate via magic number.
pub const DRM_IOCTL_NR_AUTH_MAGIC: u32 = 0x11;
/// Get DRM capabilities.
pub const DRM_IOCTL_NR_GET_CAP: u32 = 0x0C;
/// Set client capability.
pub const DRM_IOCTL_NR_SET_CLIENT_CAP: u32 = 0x0D;
/// Open a GEM handle.
pub const DRM_IOCTL_NR_GEM_OPEN: u32 = 0x0B;
/// Close a GEM handle.
pub const DRM_IOCTL_NR_GEM_CLOSE: u32 = 0x09;
/// Create a GEM flink name.
pub const DRM_IOCTL_NR_GEM_FLINK: u32 = 0x0A;
/// PRIME handle to FD.
pub const DRM_IOCTL_NR_PRIME_HANDLE_TO_FD: u32 = 0x2D;
/// PRIME FD to handle.
pub const DRM_IOCTL_NR_PRIME_FD_TO_HANDLE: u32 = 0x2E;

// ---------------------------------------------------------------------------
// DRM capabilities (queried via DRM_IOCTL_GET_CAP)
// ---------------------------------------------------------------------------

/// Driver supports DRM_IOCTL_DUMB_BUFFER (dumb framebuffers).
pub const DRM_CAP_DUMB_BUFFER: u64 = 0x01;
/// Driver supports vblank high-CRTC (>1 display pipe).
pub const DRM_CAP_VBLANK_HIGH_CRTC: u64 = 0x02;
/// Driver supports PRIME (DMA-BUF) buffer sharing.
pub const DRM_CAP_PRIME: u64 = 0x05;
/// Driver supports timestamp monotonic clock.
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x06;
/// Driver supports async page flip.
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x07;
/// Cursor width (pixels).
pub const DRM_CAP_CURSOR_WIDTH: u64 = 0x08;
/// Cursor height (pixels).
pub const DRM_CAP_CURSOR_HEIGHT: u64 = 0x09;
/// Driver supports AddFB2 modifiers (tiling, compression).
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
/// Driver supports page flip target vblank.
pub const DRM_CAP_PAGE_FLIP_TARGET: u64 = 0x11;
/// Driver supports syncobj.
pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
/// Driver supports timeline syncobj.
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;

// ---------------------------------------------------------------------------
// DRM PRIME capability bits (value for DRM_CAP_PRIME)
// ---------------------------------------------------------------------------

/// Driver can export PRIME handles.
pub const DRM_PRIME_CAP_EXPORT: u32 = 0x01;
/// Driver can import PRIME handles.
pub const DRM_PRIME_CAP_IMPORT: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_numbers_distinct() {
        let nrs = [
            DRM_IOCTL_NR_VERSION,
            DRM_IOCTL_NR_GET_UNIQUE,
            DRM_IOCTL_NR_AUTH_MAGIC,
            DRM_IOCTL_NR_GET_CAP,
            DRM_IOCTL_NR_SET_CLIENT_CAP,
            DRM_IOCTL_NR_GEM_OPEN,
            DRM_IOCTL_NR_GEM_CLOSE,
            DRM_IOCTL_NR_GEM_FLINK,
            DRM_IOCTL_NR_PRIME_HANDLE_TO_FD,
            DRM_IOCTL_NR_PRIME_FD_TO_HANDLE,
        ];
        for i in 0..nrs.len() {
            for j in (i + 1)..nrs.len() {
                assert_ne!(nrs[i], nrs[j]);
            }
        }
    }

    #[test]
    fn test_capabilities_distinct() {
        let caps = [
            DRM_CAP_DUMB_BUFFER,
            DRM_CAP_VBLANK_HIGH_CRTC,
            DRM_CAP_PRIME,
            DRM_CAP_TIMESTAMP_MONOTONIC,
            DRM_CAP_ASYNC_PAGE_FLIP,
            DRM_CAP_CURSOR_WIDTH,
            DRM_CAP_CURSOR_HEIGHT,
            DRM_CAP_ADDFB2_MODIFIERS,
            DRM_CAP_PAGE_FLIP_TARGET,
            DRM_CAP_SYNCOBJ,
            DRM_CAP_SYNCOBJ_TIMELINE,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_prime_caps_no_overlap() {
        assert_eq!(DRM_PRIME_CAP_EXPORT & DRM_PRIME_CAP_IMPORT, 0);
    }
}
