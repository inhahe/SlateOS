//! `<linux/media.h>` — Additional media controller constants.
//!
//! Supplementary media framework constants covering
//! entity functions, pad flags, and link flags.

// ---------------------------------------------------------------------------
// Entity function types (MEDIA_ENT_F_*)
// ---------------------------------------------------------------------------

/// Unknown entity.
pub const MEDIA_ENT_F_UNKNOWN: u32 = 0x00000000;
/// V4L2 sub-device (generic).
pub const MEDIA_ENT_F_V4L2_SUBDEV_UNKNOWN: u32 = 0x00010000;
/// DTV demod.
pub const MEDIA_ENT_F_DTV_DEMOD: u32 = 0x00020001;
/// Tuner.
pub const MEDIA_ENT_F_TUNER: u32 = 0x00020002;
/// Digital TV demux.
pub const MEDIA_ENT_F_TS_DEMUX: u32 = 0x00020003;
/// DTV CA.
pub const MEDIA_ENT_F_DTV_CA: u32 = 0x00020004;
/// DTV net decaps.
pub const MEDIA_ENT_F_DTV_NET_DECAP: u32 = 0x00020005;
/// IO V4L.
pub const MEDIA_ENT_F_IO_V4L: u32 = 0x00030001;
/// IO DTV.
pub const MEDIA_ENT_F_IO_DTV: u32 = 0x00030002;
/// IO VBI.
pub const MEDIA_ENT_F_IO_VBI: u32 = 0x00030003;
/// IO SWRADIO.
pub const MEDIA_ENT_F_IO_SWRADIO: u32 = 0x00030004;
/// Camera sensor.
pub const MEDIA_ENT_F_CAM_SENSOR: u32 = 0x00040001;
/// Flash LED.
pub const MEDIA_ENT_F_FLASH: u32 = 0x00040002;
/// Lens controller.
pub const MEDIA_ENT_F_LENS: u32 = 0x00040003;
/// ATV decoder.
pub const MEDIA_ENT_F_ATV_DECODER: u32 = 0x00050001;
/// DTV decoder.
pub const MEDIA_ENT_F_DTV_DECODER: u32 = 0x00050002;
/// Processing entity.
pub const MEDIA_ENT_F_PROC_VIDEO_COMPOSER: u32 = 0x00060001;
/// Pixel formatter.
pub const MEDIA_ENT_F_PROC_VIDEO_PIXEL_FORMATTER: u32 = 0x00060002;
/// Pixel encoder.
pub const MEDIA_ENT_F_PROC_VIDEO_PIXEL_ENC_CONV: u32 = 0x00060003;
/// LUT.
pub const MEDIA_ENT_F_PROC_VIDEO_LUT: u32 = 0x00060004;
/// Scaler.
pub const MEDIA_ENT_F_PROC_VIDEO_SCALER: u32 = 0x00060005;
/// Statistics.
pub const MEDIA_ENT_F_PROC_VIDEO_STATISTICS: u32 = 0x00060006;
/// Encoder.
pub const MEDIA_ENT_F_PROC_VIDEO_ENCODER: u32 = 0x00060007;
/// Decoder.
pub const MEDIA_ENT_F_PROC_VIDEO_DECODER: u32 = 0x00060008;

// ---------------------------------------------------------------------------
// Pad flags (MEDIA_PAD_FL_*)
// ---------------------------------------------------------------------------

/// Pad is a sink.
pub const MEDIA_PAD_FL_SINK: u32 = 1 << 0;
/// Pad is a source.
pub const MEDIA_PAD_FL_SOURCE: u32 = 1 << 1;
/// Pad must connect.
pub const MEDIA_PAD_FL_MUST_CONNECT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Link flags (MEDIA_LNK_FL_*)
// ---------------------------------------------------------------------------

/// Link is enabled.
pub const MEDIA_LNK_FL_ENABLED: u32 = 1 << 0;
/// Link is immutable.
pub const MEDIA_LNK_FL_IMMUTABLE: u32 = 1 << 1;
/// Dynamic link.
pub const MEDIA_LNK_FL_DYNAMIC: u32 = 1 << 2;
/// Link type data.
pub const MEDIA_LNK_FL_LINK_TYPE: u32 = 0xF << 28;
/// Data link.
pub const MEDIA_LNK_FL_DATA_LINK: u32 = 0 << 28;
/// Interface link.
pub const MEDIA_LNK_FL_INTERFACE_LINK: u32 = 1 << 28;
/// Ancillary link.
pub const MEDIA_LNK_FL_ANCILLARY_LINK: u32 = 2 << 28;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_functions_distinct() {
        let funcs = [
            MEDIA_ENT_F_UNKNOWN, MEDIA_ENT_F_V4L2_SUBDEV_UNKNOWN,
            MEDIA_ENT_F_DTV_DEMOD, MEDIA_ENT_F_TUNER,
            MEDIA_ENT_F_TS_DEMUX, MEDIA_ENT_F_IO_V4L,
            MEDIA_ENT_F_CAM_SENSOR, MEDIA_ENT_F_FLASH,
            MEDIA_ENT_F_ATV_DECODER, MEDIA_ENT_F_DTV_DECODER,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_pad_flags_power_of_two() {
        assert!(MEDIA_PAD_FL_SINK.is_power_of_two());
        assert!(MEDIA_PAD_FL_SOURCE.is_power_of_two());
        assert!(MEDIA_PAD_FL_MUST_CONNECT.is_power_of_two());
    }

    #[test]
    fn test_pad_flags_no_overlap() {
        assert_eq!(MEDIA_PAD_FL_SINK & MEDIA_PAD_FL_SOURCE, 0);
    }

    #[test]
    fn test_link_flags_power_of_two() {
        assert!(MEDIA_LNK_FL_ENABLED.is_power_of_two());
        assert!(MEDIA_LNK_FL_IMMUTABLE.is_power_of_two());
        assert!(MEDIA_LNK_FL_DYNAMIC.is_power_of_two());
    }

    #[test]
    fn test_link_types_distinct() {
        assert_ne!(MEDIA_LNK_FL_DATA_LINK, MEDIA_LNK_FL_INTERFACE_LINK);
        assert_ne!(MEDIA_LNK_FL_INTERFACE_LINK, MEDIA_LNK_FL_ANCILLARY_LINK);
    }
}
