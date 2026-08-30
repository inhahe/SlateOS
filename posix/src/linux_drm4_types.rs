//! `<linux/drm.h>` — Additional DRM constants (part 4).
//!
//! Supplementary DRM constants covering mode connector types,
//! encoder types, and property types.

// ---------------------------------------------------------------------------
// DRM connector types
// ---------------------------------------------------------------------------

/// Unknown connector.
pub const DRM_MODE_CONNECTOR_Unknown: u32 = 0;
/// VGA.
pub const DRM_MODE_CONNECTOR_VGA: u32 = 1;
/// DVI-I.
pub const DRM_MODE_CONNECTOR_DVII: u32 = 2;
/// DVI-D.
pub const DRM_MODE_CONNECTOR_DVID: u32 = 3;
/// DVI-A.
pub const DRM_MODE_CONNECTOR_DVIA: u32 = 4;
/// Composite.
pub const DRM_MODE_CONNECTOR_Composite: u32 = 5;
/// SVIDEO.
pub const DRM_MODE_CONNECTOR_SVIDEO: u32 = 6;
/// LVDS.
pub const DRM_MODE_CONNECTOR_LVDS: u32 = 7;
/// Component.
pub const DRM_MODE_CONNECTOR_Component: u32 = 8;
/// 9-pin DIN.
pub const DRM_MODE_CONNECTOR_9PinDIN: u32 = 9;
/// DisplayPort.
pub const DRM_MODE_CONNECTOR_DisplayPort: u32 = 10;
/// HDMI-A.
pub const DRM_MODE_CONNECTOR_HDMIA: u32 = 11;
/// HDMI-B.
pub const DRM_MODE_CONNECTOR_HDMIB: u32 = 12;
/// TV.
pub const DRM_MODE_CONNECTOR_TV: u32 = 13;
/// eDP.
pub const DRM_MODE_CONNECTOR_eDP: u32 = 14;
/// Virtual.
pub const DRM_MODE_CONNECTOR_VIRTUAL: u32 = 15;
/// DSI.
pub const DRM_MODE_CONNECTOR_DSI: u32 = 16;
/// DPI.
pub const DRM_MODE_CONNECTOR_DPI: u32 = 17;
/// Writeback.
pub const DRM_MODE_CONNECTOR_WRITEBACK: u32 = 18;
/// SPI.
pub const DRM_MODE_CONNECTOR_SPI: u32 = 19;
/// USB.
pub const DRM_MODE_CONNECTOR_USB: u32 = 20;

// ---------------------------------------------------------------------------
// DRM encoder types
// ---------------------------------------------------------------------------

/// No encoder.
pub const DRM_MODE_ENCODER_NONE: u32 = 0;
/// DAC encoder.
pub const DRM_MODE_ENCODER_DAC: u32 = 1;
/// TMDS encoder.
pub const DRM_MODE_ENCODER_TMDS: u32 = 2;
/// LVDS encoder.
pub const DRM_MODE_ENCODER_LVDS: u32 = 3;
/// TVDAC encoder.
pub const DRM_MODE_ENCODER_TVDAC: u32 = 4;
/// Virtual encoder.
pub const DRM_MODE_ENCODER_VIRTUAL: u32 = 5;
/// DSI encoder.
pub const DRM_MODE_ENCODER_DSI: u32 = 6;
/// DPMST encoder.
pub const DRM_MODE_ENCODER_DPMST: u32 = 7;
/// DPI encoder.
pub const DRM_MODE_ENCODER_DPI: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_types_distinct() {
        let types = [
            DRM_MODE_CONNECTOR_Unknown,
            DRM_MODE_CONNECTOR_VGA,
            DRM_MODE_CONNECTOR_DVII,
            DRM_MODE_CONNECTOR_DVID,
            DRM_MODE_CONNECTOR_DVIA,
            DRM_MODE_CONNECTOR_Composite,
            DRM_MODE_CONNECTOR_SVIDEO,
            DRM_MODE_CONNECTOR_LVDS,
            DRM_MODE_CONNECTOR_Component,
            DRM_MODE_CONNECTOR_9PinDIN,
            DRM_MODE_CONNECTOR_DisplayPort,
            DRM_MODE_CONNECTOR_HDMIA,
            DRM_MODE_CONNECTOR_HDMIB,
            DRM_MODE_CONNECTOR_TV,
            DRM_MODE_CONNECTOR_eDP,
            DRM_MODE_CONNECTOR_VIRTUAL,
            DRM_MODE_CONNECTOR_DSI,
            DRM_MODE_CONNECTOR_DPI,
            DRM_MODE_CONNECTOR_WRITEBACK,
            DRM_MODE_CONNECTOR_SPI,
            DRM_MODE_CONNECTOR_USB,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_encoder_types_distinct() {
        let types = [
            DRM_MODE_ENCODER_NONE,
            DRM_MODE_ENCODER_DAC,
            DRM_MODE_ENCODER_TMDS,
            DRM_MODE_ENCODER_LVDS,
            DRM_MODE_ENCODER_TVDAC,
            DRM_MODE_ENCODER_VIRTUAL,
            DRM_MODE_ENCODER_DSI,
            DRM_MODE_ENCODER_DPMST,
            DRM_MODE_ENCODER_DPI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
