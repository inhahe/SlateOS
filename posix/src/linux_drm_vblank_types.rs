//! `<drm/drm.h>` — DRM vblank (vertical blank) event constants.
//!
//! Vblank events synchronize display updates with the monitor's
//! refresh cycle. Applications request vblank notifications to
//! implement tear-free rendering and frame pacing. The kernel
//! timestamps each vblank for precise frame timing.

// ---------------------------------------------------------------------------
// Vblank event types
// ---------------------------------------------------------------------------

/// Relative vblank wait (wait N vblanks from now).
pub const DRM_VBLANK_RELATIVE: u32 = 0x01;
/// Absolute vblank wait (wait until vblank sequence N).
pub const DRM_VBLANK_ABSOLUTE: u32 = 0x00;

// ---------------------------------------------------------------------------
// Vblank request flags
// ---------------------------------------------------------------------------

/// Request high-CRTC-index (bits 24-31 encode CRTC pipe).
pub const DRM_VBLANK_HIGH_CRTC_MASK: u32 = 0x0000_003E;
/// Request a vblank event (async notification).
pub const DRM_VBLANK_EVENT: u32 = 0x0400_0000;
/// Flip complete event (page flip done).
pub const DRM_VBLANK_FLIP_CRTC: u32 = 0x0800_0000;
/// Signal nextonmiss (don't wait if already missed).
pub const DRM_VBLANK_NEXTONMISS: u32 = 0x1000_0000;
/// Secondary display output.
pub const DRM_VBLANK_SECONDARY: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// Page flip flags (drmModePageFlip)
// ---------------------------------------------------------------------------

/// Generate page flip event on completion.
pub const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
/// Async page flip (don't wait for vblank).
pub const DRM_MODE_PAGE_FLIP_ASYNC: u32 = 0x02;
/// Target a specific vblank sequence.
pub const DRM_MODE_PAGE_FLIP_TARGET: u32 = 0x08;

// ---------------------------------------------------------------------------
// Atomic commit flags
// ---------------------------------------------------------------------------

/// Test-only commit (validate but don't apply).
pub const DRM_MODE_ATOMIC_TEST_ONLY: u32 = 0x0100;
/// Non-blocking commit (return immediately).
pub const DRM_MODE_ATOMIC_NONBLOCK: u32 = 0x0200;
/// Allow modeset in atomic commit.
pub const DRM_MODE_ATOMIC_ALLOW_MODESET: u32 = 0x0400;
/// Generate page flip event for each plane.
pub const DRM_MODE_ATOMIC_PAGE_FLIP_EVENT: u32 = 0x01;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vblank_types() {
        assert_ne!(DRM_VBLANK_RELATIVE, DRM_VBLANK_ABSOLUTE);
    }

    #[test]
    fn test_vblank_flags_distinct() {
        let flags = [
            DRM_VBLANK_EVENT, DRM_VBLANK_FLIP_CRTC,
            DRM_VBLANK_NEXTONMISS, DRM_VBLANK_SECONDARY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_page_flip_flags_distinct() {
        let flags = [
            DRM_MODE_PAGE_FLIP_EVENT, DRM_MODE_PAGE_FLIP_ASYNC,
            DRM_MODE_PAGE_FLIP_TARGET,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_atomic_flags_distinct() {
        let flags = [
            DRM_MODE_ATOMIC_TEST_ONLY, DRM_MODE_ATOMIC_NONBLOCK,
            DRM_MODE_ATOMIC_ALLOW_MODESET,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
