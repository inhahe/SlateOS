//! `<drm/drm.h>` — Direct Rendering Manager userspace ABI core.
//!
//! Every DRM/KMS client (X server, Weston, mutter, kmscube, Mesa)
//! talks to the kernel through `/dev/dri/cardN` or `/dev/dri/renderDN`.
//! The core ioctl namespace and capability flags below are stable
//! UAPI consumed by every Wayland/X stack on Linux.

// ---------------------------------------------------------------------------
// Device-node naming
// ---------------------------------------------------------------------------

/// `/dev/dri` directory.
pub const DRM_DIR_NAME: &str = "/dev/dri";
/// Primary node basename prefix.
pub const DRM_PRIMARY_MINOR_NAME: &str = "card";
/// Render node basename prefix.
pub const DRM_RENDER_MINOR_NAME: &str = "renderD";
/// First render-node minor number.
pub const DRM_RENDER_MINOR_BASE: u32 = 128;
/// Maximum minors of each type.
pub const DRM_MAX_MINOR: u32 = 64;

// ---------------------------------------------------------------------------
// Node types (drm_minor.type)
// ---------------------------------------------------------------------------

/// Primary node — full modesetting + ioctl access.
pub const DRM_NODE_PRIMARY: u32 = 0;
/// Control node — historical, no longer used.
pub const DRM_NODE_CONTROL: u32 = 1;
/// Render node — GPU access without modesetting.
pub const DRM_NODE_RENDER: u32 = 2;

// ---------------------------------------------------------------------------
// ioctl group / base
// ---------------------------------------------------------------------------

/// DRM ioctl type byte is 'd' (0x64).
pub const DRM_IOCTL_BASE: u32 = b'd' as u32;
/// `DRM_COMMAND_BASE` — start of driver-specific ioctls.
pub const DRM_COMMAND_BASE: u32 = 0x40;
/// `DRM_COMMAND_END` — last driver-specific ioctl number.
pub const DRM_COMMAND_END: u32 = 0xA0;

// ---------------------------------------------------------------------------
// DRM capabilities (DRM_IOCTL_GET_CAP)
// ---------------------------------------------------------------------------

/// `DRM_CAP_DUMB_BUFFER` — supports dumb framebuffer ioctls.
pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
/// `DRM_CAP_VBLANK_HIGH_CRTC` — vblank events report CRTC index.
pub const DRM_CAP_VBLANK_HIGH_CRTC: u64 = 0x2;
/// `DRM_CAP_DUMB_PREFERRED_DEPTH`.
pub const DRM_CAP_DUMB_PREFERRED_DEPTH: u64 = 0x3;
/// `DRM_CAP_DUMB_PREFER_SHADOW`.
pub const DRM_CAP_DUMB_PREFER_SHADOW: u64 = 0x4;
/// `DRM_CAP_PRIME` — PRIME export/import (DMA-BUF).
pub const DRM_CAP_PRIME: u64 = 0x5;
/// `DRM_CAP_TIMESTAMP_MONOTONIC` — vblank ts uses CLOCK_MONOTONIC.
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
/// `DRM_CAP_ASYNC_PAGE_FLIP`.
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x7;
/// `DRM_CAP_CURSOR_WIDTH`.
pub const DRM_CAP_CURSOR_WIDTH: u64 = 0x8;
/// `DRM_CAP_CURSOR_HEIGHT`.
pub const DRM_CAP_CURSOR_HEIGHT: u64 = 0x9;
/// `DRM_CAP_ADDFB2_MODIFIERS` — fb modifiers in AddFB2.
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
/// `DRM_CAP_PAGE_FLIP_TARGET`.
pub const DRM_CAP_PAGE_FLIP_TARGET: u64 = 0x11;
/// `DRM_CAP_CRTC_IN_VBLANK_EVENT`.
pub const DRM_CAP_CRTC_IN_VBLANK_EVENT: u64 = 0x12;
/// `DRM_CAP_SYNCOBJ` — sync objects supported.
pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
/// `DRM_CAP_SYNCOBJ_TIMELINE`.
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;

// ---------------------------------------------------------------------------
// PRIME flags
// ---------------------------------------------------------------------------

/// PRIME export: import side may write.
pub const DRM_PRIME_CAP_IMPORT: u64 = 0x1;
/// PRIME export side.
pub const DRM_PRIME_CAP_EXPORT: u64 = 0x2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_paths_and_bases() {
        assert_eq!(DRM_DIR_NAME, "/dev/dri");
        assert_eq!(DRM_PRIMARY_MINOR_NAME, "card");
        assert_eq!(DRM_RENDER_MINOR_NAME, "renderD");
        // Render nodes start at minor 128 so primary/render never alias.
        assert_eq!(DRM_RENDER_MINOR_BASE, 128);
        // Up to 64 minors per type fits the kernel's drm_minor array.
        assert_eq!(DRM_MAX_MINOR, 64);
    }

    #[test]
    fn test_node_types_dense() {
        assert_eq!(DRM_NODE_PRIMARY, 0);
        assert_eq!(DRM_NODE_CONTROL, 1);
        assert_eq!(DRM_NODE_RENDER, 2);
    }

    #[test]
    fn test_ioctl_letter_and_range() {
        // 'd' is the group letter for /dev/dri ioctls.
        assert_eq!(DRM_IOCTL_BASE, 0x64);
        // Driver-specific window must leave room for core (<0x40) and
        // post-driver (>0xA0) ranges.
        assert!(DRM_COMMAND_BASE < DRM_COMMAND_END);
        assert_eq!(DRM_COMMAND_BASE, 0x40);
        assert_eq!(DRM_COMMAND_END, 0xA0);
    }

    #[test]
    fn test_caps_distinct() {
        let c = [
            DRM_CAP_DUMB_BUFFER,
            DRM_CAP_VBLANK_HIGH_CRTC,
            DRM_CAP_DUMB_PREFERRED_DEPTH,
            DRM_CAP_DUMB_PREFER_SHADOW,
            DRM_CAP_PRIME,
            DRM_CAP_TIMESTAMP_MONOTONIC,
            DRM_CAP_ASYNC_PAGE_FLIP,
            DRM_CAP_CURSOR_WIDTH,
            DRM_CAP_CURSOR_HEIGHT,
            DRM_CAP_ADDFB2_MODIFIERS,
            DRM_CAP_PAGE_FLIP_TARGET,
            DRM_CAP_CRTC_IN_VBLANK_EVENT,
            DRM_CAP_SYNCOBJ,
            DRM_CAP_SYNCOBJ_TIMELINE,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_prime_caps_pow2_distinct() {
        assert!(DRM_PRIME_CAP_IMPORT.is_power_of_two());
        assert!(DRM_PRIME_CAP_EXPORT.is_power_of_two());
        assert_ne!(DRM_PRIME_CAP_IMPORT, DRM_PRIME_CAP_EXPORT);
    }
}
