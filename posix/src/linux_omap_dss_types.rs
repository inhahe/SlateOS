//! `<video/omapfb_dss.h>` — OMAP Display Subsystem constants.
//!
//! Constants for the OMAP Display SubSystem (DSS) used on TI OMAP
//! SoCs (BeagleBoard, PandaBoard, OMAP5). Userspace omapdrm config
//! tools and panel test utilities consume these.

// ---------------------------------------------------------------------------
// Display interface types (struct omap_dss_device.type)
// ---------------------------------------------------------------------------

/// DPI (parallel RGB).
pub const OMAP_DISPLAY_TYPE_DPI: u32 = 1 << 0;
/// DBI (parallel command-mode panel).
pub const OMAP_DISPLAY_TYPE_DBI: u32 = 1 << 1;
/// SDI (serial display interface).
pub const OMAP_DISPLAY_TYPE_SDI: u32 = 1 << 2;
/// DSI (MIPI Display Serial Interface).
pub const OMAP_DISPLAY_TYPE_DSI: u32 = 1 << 3;
/// VENC (analog TV out).
pub const OMAP_DISPLAY_TYPE_VENC: u32 = 1 << 4;
/// HDMI.
pub const OMAP_DISPLAY_TYPE_HDMI: u32 = 1 << 5;
/// DVI.
pub const OMAP_DISPLAY_TYPE_DVI: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Channel identifiers (struct omap_overlay_manager.id)
// ---------------------------------------------------------------------------

/// LCD channel 0.
pub const OMAP_DSS_CHANNEL_LCD: u32 = 0;
/// Digital channel (TV/HDMI).
pub const OMAP_DSS_CHANNEL_DIGIT: u32 = 1;
/// LCD channel 1 (DSS3+).
pub const OMAP_DSS_CHANNEL_LCD2: u32 = 2;
/// LCD channel 2 (DSS3+).
pub const OMAP_DSS_CHANNEL_LCD3: u32 = 3;
/// Writeback channel.
pub const OMAP_DSS_CHANNEL_WB: u32 = 4;

// ---------------------------------------------------------------------------
// Overlay caps (struct omap_overlay.caps)
// ---------------------------------------------------------------------------

/// Overlay supports scaling.
pub const OMAP_DSS_OVL_CAP_SCALE: u32 = 1 << 0;
/// Overlay supports global alpha.
pub const OMAP_DSS_OVL_CAP_GLOBAL_ALPHA: u32 = 1 << 1;
/// Overlay supports premultiplied alpha.
pub const OMAP_DSS_OVL_CAP_PRE_MULT_ALPHA: u32 = 1 << 2;
/// Overlay supports zorder reordering.
pub const OMAP_DSS_OVL_CAP_ZORDER: u32 = 1 << 3;
/// Overlay supports replication (LCD pixel doubling).
pub const OMAP_DSS_OVL_CAP_POS: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Default panel timing limits
// ---------------------------------------------------------------------------

/// Maximum hsync width.
pub const OMAP_DSS_MAX_HSW: u32 = 256;
/// Maximum vsync width.
pub const OMAP_DSS_MAX_VSW: u32 = 256;
/// Maximum porch width.
pub const OMAP_DSS_MAX_PORCH: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_types_distinct_powers_of_two() {
        let types = [
            OMAP_DISPLAY_TYPE_DPI,
            OMAP_DISPLAY_TYPE_DBI,
            OMAP_DISPLAY_TYPE_SDI,
            OMAP_DISPLAY_TYPE_DSI,
            OMAP_DISPLAY_TYPE_VENC,
            OMAP_DISPLAY_TYPE_HDMI,
            OMAP_DISPLAY_TYPE_DVI,
        ];
        for &t in &types {
            assert!(t.is_power_of_two());
        }
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_channels_distinct() {
        let chans = [
            OMAP_DSS_CHANNEL_LCD,
            OMAP_DSS_CHANNEL_DIGIT,
            OMAP_DSS_CHANNEL_LCD2,
            OMAP_DSS_CHANNEL_LCD3,
            OMAP_DSS_CHANNEL_WB,
        ];
        for i in 0..chans.len() {
            for j in (i + 1)..chans.len() {
                assert_ne!(chans[i], chans[j]);
            }
        }
    }

    #[test]
    fn test_overlay_caps_distinct_powers_of_two() {
        let caps = [
            OMAP_DSS_OVL_CAP_SCALE,
            OMAP_DSS_OVL_CAP_GLOBAL_ALPHA,
            OMAP_DSS_OVL_CAP_PRE_MULT_ALPHA,
            OMAP_DSS_OVL_CAP_ZORDER,
            OMAP_DSS_OVL_CAP_POS,
        ];
        for &c in &caps {
            assert!(c.is_power_of_two());
        }
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_timing_limits_reasonable() {
        // Sanity: hsync/vsync widths must fit within a typical
        // panel-pixel column count and porches a typical scan line.
        assert!(OMAP_DSS_MAX_HSW >= 8 && OMAP_DSS_MAX_HSW <= 4096);
        assert!(OMAP_DSS_MAX_VSW >= 1 && OMAP_DSS_MAX_VSW <= 4096);
        assert!(OMAP_DSS_MAX_PORCH >= OMAP_DSS_MAX_HSW);
    }
}
