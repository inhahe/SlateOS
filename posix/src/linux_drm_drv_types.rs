//! `<drm/drm_drv.h>` — DRM driver feature/capability flag constants.
//!
//! `drm_driver.driver_features` tells the DRM core which subsystems a
//! KMS/GPU driver implements (mode-setting, GEM, syncobj, render
//! nodes, etc.). KMS clients reach these bits through
//! `DRM_GET_CAP` / `drm_set_client_cap` ioctls. Mesa, libdrm, weston,
//! and gnome-shell match against the bits below.

// ---------------------------------------------------------------------------
// drm_driver.driver_features bits (DRIVER_*)
// ---------------------------------------------------------------------------

/// Driver supports the GEM memory-manager subsystem.
pub const DRIVER_GEM: u32 = 1 << 2;
/// Driver supports mode-setting (KMS).
pub const DRIVER_MODESET: u32 = 1 << 3;
/// Driver supports prime FD import/export (DMA-BUF).
pub const DRIVER_PRIME: u32 = 1 << 4;
/// Driver supports render nodes (`/dev/dri/renderD*`).
pub const DRIVER_RENDER: u32 = 1 << 5;
/// Driver supports atomic mode-setting.
pub const DRIVER_ATOMIC: u32 = 1 << 6;
/// Driver supports DRM sync objects (`drm_syncobj`).
pub const DRIVER_SYNCOBJ: u32 = 1 << 7;
/// Driver's syncobjs support timelines.
pub const DRIVER_SYNCOBJ_TIMELINE: u32 = 1 << 8;
/// Driver exposes only userspace compute paths (no display).
pub const DRIVER_COMPUTE_ACCEL: u32 = 1 << 9;
/// Driver uses managed-resource allocations (drmm_*).
pub const DRIVER_GEM_GPUVA: u32 = 1 << 10;
/// Driver supports buffer "cursor" pinning across atomic commits.
pub const DRIVER_CURSOR_HOTSPOT: u32 = 1 << 11;

// ---------------------------------------------------------------------------
// DRM_GET_CAP capability IDs (passed to drmGetCap())
// ---------------------------------------------------------------------------

/// Supports dumb (CPU-mapped) buffer allocation.
pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
/// Supports v-blank events for any CRTC.
pub const DRM_CAP_VBLANK_HIGH_CRTC: u64 = 0x2;
/// Reports preferred dumb-buffer depth (bpp).
pub const DRM_CAP_DUMB_PREFERRED_DEPTH: u64 = 0x3;
/// Driver prefers shadow-buffer flush vs. direct writes.
pub const DRM_CAP_DUMB_PREFER_SHADOW: u64 = 0x4;
/// Driver supports PRIME export/import.
pub const DRM_CAP_PRIME: u64 = 0x5;
/// Driver supports 64-bit per-CRTC time stamps.
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
/// Driver supports async-page-flip.
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x7;
/// Driver reports CRTC-relative cursor width.
pub const DRM_CAP_CURSOR_WIDTH: u64 = 0x8;
/// Driver reports CRTC-relative cursor height.
pub const DRM_CAP_CURSOR_HEIGHT: u64 = 0x9;
/// Driver supports modifier-tagged formats.
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
/// Driver supports page-flip on planes.
pub const DRM_CAP_PAGE_FLIP_TARGET: u64 = 0x11;
/// Driver supports CRTC-CRC sampling.
pub const DRM_CAP_CRTC_IN_VBLANK_EVENT: u64 = 0x12;
/// Syncobj capability advertised to clients.
pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
/// Syncobj timelines supported.
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;

// ---------------------------------------------------------------------------
// DRM_PRIME flags returned to userspace
// ---------------------------------------------------------------------------

/// Driver can export prime FDs.
pub const DRM_PRIME_CAP_EXPORT: u64 = 0x1;
/// Driver can import prime FDs.
pub const DRM_PRIME_CAP_IMPORT: u64 = 0x2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_feature_bits_distinct_pow2() {
        let f = [
            DRIVER_GEM,
            DRIVER_MODESET,
            DRIVER_PRIME,
            DRIVER_RENDER,
            DRIVER_ATOMIC,
            DRIVER_SYNCOBJ,
            DRIVER_SYNCOBJ_TIMELINE,
            DRIVER_COMPUTE_ACCEL,
            DRIVER_GEM_GPUVA,
            DRIVER_CURSOR_HOTSPOT,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_cap_ids_distinct() {
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
    fn test_prime_flags_distinct_pow2() {
        assert!(DRM_PRIME_CAP_EXPORT.is_power_of_two());
        assert!(DRM_PRIME_CAP_IMPORT.is_power_of_two());
        assert_ne!(DRM_PRIME_CAP_EXPORT, DRM_PRIME_CAP_IMPORT);
    }
}
