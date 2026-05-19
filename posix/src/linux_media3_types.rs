//! `<linux/media.h>` — Additional media controller constants (part 3).
//!
//! Supplementary media controller constants covering entity types,
//! interface types, and link flags.

// ---------------------------------------------------------------------------
// Media entity types
// ---------------------------------------------------------------------------

/// Base entity type mask.
pub const MEDIA_ENT_T_DEVNODE: u32 = 0x00010000;
/// V4L2 sub-device base.
pub const MEDIA_ENT_T_V4L2_SUBDEV: u32 = 0x00020000;

// ---------------------------------------------------------------------------
// Media entity function types
// ---------------------------------------------------------------------------

/// Unknown.
pub const MEDIA_ENT_F_UNKNOWN: u32 = 0;
/// DTV demod.
pub const MEDIA_ENT_F_DTV_DEMOD: u32 = 0x00020001;
/// DTV Tuner.
pub const MEDIA_ENT_F_TUNER: u32 = 0x00020005;
/// I/O.
pub const MEDIA_ENT_F_IO_V4L: u32 = 0x00010001;
/// IO DTV.
pub const MEDIA_ENT_F_IO_DTV: u32 = 0x00010002;
/// IO VBI.
pub const MEDIA_ENT_F_IO_VBI: u32 = 0x00010003;
/// IO SWRADIO.
pub const MEDIA_ENT_F_IO_SWRADIO: u32 = 0x00010004;
/// Camera sensor.
pub const MEDIA_ENT_F_CAM_SENSOR: u32 = 0x00020002;
/// Flash.
pub const MEDIA_ENT_F_FLASH: u32 = 0x00020003;
/// Lens.
pub const MEDIA_ENT_F_LENS: u32 = 0x00020004;
/// Processing.
pub const MEDIA_ENT_F_PROC_VIDEO_PIXEL_FORMATTER: u32 = 0x00040002;

// ---------------------------------------------------------------------------
// Media link flags
// ---------------------------------------------------------------------------

/// Link is enabled.
pub const MEDIA_LNK_FL_ENABLED: u32 = 1 << 0;
/// Link is immutable.
pub const MEDIA_LNK_FL_IMMUTABLE: u32 = 1 << 1;
/// Dynamic link.
pub const MEDIA_LNK_FL_DYNAMIC: u32 = 1 << 2;
/// Data link.
pub const MEDIA_LNK_FL_LINK_TYPE: u32 = 0;
/// Interface link.
pub const MEDIA_LNK_FL_INTERFACE_LINK: u32 = 1 << 3;
/// Ancillary link.
pub const MEDIA_LNK_FL_ANCILLARY_LINK: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Media pad flags
// ---------------------------------------------------------------------------

/// Sink pad.
pub const MEDIA_PAD_FL_SINK: u32 = 1 << 0;
/// Source pad.
pub const MEDIA_PAD_FL_SOURCE: u32 = 1 << 1;
/// Must connect.
pub const MEDIA_PAD_FL_MUST_CONNECT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_functions_distinct() {
        let funcs = [
            MEDIA_ENT_F_UNKNOWN, MEDIA_ENT_F_DTV_DEMOD,
            MEDIA_ENT_F_TUNER, MEDIA_ENT_F_IO_V4L,
            MEDIA_ENT_F_IO_DTV, MEDIA_ENT_F_IO_VBI,
            MEDIA_ENT_F_IO_SWRADIO, MEDIA_ENT_F_CAM_SENSOR,
            MEDIA_ENT_F_FLASH, MEDIA_ENT_F_LENS,
            MEDIA_ENT_F_PROC_VIDEO_PIXEL_FORMATTER,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_link_flags_no_overlap() {
        let flags = [
            MEDIA_LNK_FL_ENABLED, MEDIA_LNK_FL_IMMUTABLE,
            MEDIA_LNK_FL_DYNAMIC, MEDIA_LNK_FL_INTERFACE_LINK,
            MEDIA_LNK_FL_ANCILLARY_LINK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_pad_flags_no_overlap() {
        let flags = [
            MEDIA_PAD_FL_SINK, MEDIA_PAD_FL_SOURCE,
            MEDIA_PAD_FL_MUST_CONNECT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
