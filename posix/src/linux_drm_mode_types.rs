//! `<drm/drm_mode.h>` — DRM display mode constants.
//!
//! Display modes describe the timing parameters for driving a monitor:
//! resolution, refresh rate, pixel clock, blanking intervals, and
//! sync polarities. The modeline format originates from X11/XFree86
//! but is used throughout the Linux graphics stack. The kernel's KMS
//! (Kernel Mode Setting) subsystem uses modes to program display
//! controllers (CRTCs) to output video at the correct timing.

// ---------------------------------------------------------------------------
// Mode type flags
// ---------------------------------------------------------------------------

/// Built-in mode (from EDID or driver).
pub const DRM_MODE_TYPE_BUILTIN: u32 = 1 << 0;
/// Clock was reduced (pixel clock divided).
pub const DRM_MODE_TYPE_CLOCK_C: u32 = 1 << 1;
/// Preferred mode (monitor's native resolution).
pub const DRM_MODE_TYPE_PREFERRED: u32 = 1 << 3;
/// Default mode.
pub const DRM_MODE_TYPE_DEFAULT: u32 = 1 << 4;
/// Userspace-defined mode.
pub const DRM_MODE_TYPE_USERDEF: u32 = 1 << 5;
/// Driver-defined mode.
pub const DRM_MODE_TYPE_DRIVER: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Mode flags (sync, interlace, stereo)
// ---------------------------------------------------------------------------

/// Positive horizontal sync.
pub const DRM_MODE_FLAG_PHSYNC: u32 = 1 << 0;
/// Negative horizontal sync.
pub const DRM_MODE_FLAG_NHSYNC: u32 = 1 << 1;
/// Positive vertical sync.
pub const DRM_MODE_FLAG_PVSYNC: u32 = 1 << 2;
/// Negative vertical sync.
pub const DRM_MODE_FLAG_NVSYNC: u32 = 1 << 3;
/// Interlaced mode.
pub const DRM_MODE_FLAG_INTERLACE: u32 = 1 << 4;
/// Double-scan mode (each line displayed twice).
pub const DRM_MODE_FLAG_DBLSCAN: u32 = 1 << 5;
/// Composite sync.
pub const DRM_MODE_FLAG_CSYNC: u32 = 1 << 6;
/// Double-clocked mode.
pub const DRM_MODE_FLAG_DBLCLK: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// Standard resolutions (width x height)
// ---------------------------------------------------------------------------

/// VGA resolution.
pub const DRM_MODE_RES_VGA_W: u32 = 640;
/// VGA height.
pub const DRM_MODE_RES_VGA_H: u32 = 480;
/// 720p width.
pub const DRM_MODE_RES_720P_W: u32 = 1280;
/// 720p height.
pub const DRM_MODE_RES_720P_H: u32 = 720;
/// 1080p width.
pub const DRM_MODE_RES_1080P_W: u32 = 1920;
/// 1080p height.
pub const DRM_MODE_RES_1080P_H: u32 = 1080;
/// 4K UHD width.
pub const DRM_MODE_RES_4K_W: u32 = 3840;
/// 4K UHD height.
pub const DRM_MODE_RES_4K_H: u32 = 2160;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_flags_power_of_two() {
        let flags = [
            DRM_MODE_TYPE_BUILTIN, DRM_MODE_TYPE_CLOCK_C,
            DRM_MODE_TYPE_PREFERRED, DRM_MODE_TYPE_DEFAULT,
            DRM_MODE_TYPE_USERDEF, DRM_MODE_TYPE_DRIVER,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_mode_flags_power_of_two() {
        let flags = [
            DRM_MODE_FLAG_PHSYNC, DRM_MODE_FLAG_NHSYNC,
            DRM_MODE_FLAG_PVSYNC, DRM_MODE_FLAG_NVSYNC,
            DRM_MODE_FLAG_INTERLACE, DRM_MODE_FLAG_DBLSCAN,
            DRM_MODE_FLAG_CSYNC, DRM_MODE_FLAG_DBLCLK,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_resolutions_valid() {
        assert!(DRM_MODE_RES_VGA_W < DRM_MODE_RES_720P_W);
        assert!(DRM_MODE_RES_720P_W < DRM_MODE_RES_1080P_W);
        assert!(DRM_MODE_RES_1080P_W < DRM_MODE_RES_4K_W);
    }
}
