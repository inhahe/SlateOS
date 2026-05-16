//! `<linux/media.h>` — Media controller constants.
//!
//! The media controller framework manages complex media hardware
//! pipelines (cameras with ISPs, video decoders, etc.) where
//! multiple V4L2/ALSA/DVB devices are connected in a graph.

// ---------------------------------------------------------------------------
// Media entity types
// ---------------------------------------------------------------------------

/// Unknown entity.
pub const MEDIA_ENT_F_UNKNOWN: u32 = 0x0000_0000;
/// V4L2 video subdevice.
pub const MEDIA_ENT_F_V4L2_SUBDEV_UNKNOWN: u32 = 0x0002_0000;
/// DTV demux (digital TV).
pub const MEDIA_ENT_F_DTV_DEMOD: u32 = 0x0002_0001;
/// Camera sensor.
pub const MEDIA_ENT_F_CAM_SENSOR: u32 = 0x0002_0002;
/// Flash LED.
pub const MEDIA_ENT_F_FLASH: u32 = 0x0002_0003;
/// Lens controller.
pub const MEDIA_ENT_F_LENS: u32 = 0x0002_0004;
/// TV tuner.
pub const MEDIA_ENT_F_TUNER: u32 = 0x0002_0005;
/// Image Signal Processor.
pub const MEDIA_ENT_F_PROC_VIDEO_ISP: u32 = 0x0003_0000;
/// Pixel formatter.
pub const MEDIA_ENT_F_PROC_VIDEO_PIXEL_FORMATTER: u32 = 0x0003_0001;
/// Video scaler.
pub const MEDIA_ENT_F_PROC_VIDEO_SCALER: u32 = 0x0003_0002;
/// Video statistics.
pub const MEDIA_ENT_F_PROC_VIDEO_STATISTICS: u32 = 0x0003_0003;
/// Video encoder.
pub const MEDIA_ENT_F_PROC_VIDEO_ENCODER: u32 = 0x0003_0004;
/// Video decoder.
pub const MEDIA_ENT_F_PROC_VIDEO_DECODER: u32 = 0x0003_0005;
/// I/O Video capture (V4L2).
pub const MEDIA_ENT_F_IO_V4L: u32 = 0x0001_0001;
/// I/O DTV.
pub const MEDIA_ENT_F_IO_DTV: u32 = 0x0001_0002;
/// I/O VBI.
pub const MEDIA_ENT_F_IO_VBI: u32 = 0x0001_0003;
/// I/O SWRADIO.
pub const MEDIA_ENT_F_IO_SWRADIO: u32 = 0x0001_0004;

// ---------------------------------------------------------------------------
// Media entity flags
// ---------------------------------------------------------------------------

/// Entity is the default (preferred) entity.
pub const MEDIA_ENT_FL_DEFAULT: u32 = 1 << 0;
/// Entity is a connector.
pub const MEDIA_ENT_FL_CONNECTOR: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Pad flags
// ---------------------------------------------------------------------------

/// Pad is a sink (input).
pub const MEDIA_PAD_FL_SINK: u32 = 1 << 0;
/// Pad is a source (output).
pub const MEDIA_PAD_FL_SOURCE: u32 = 1 << 1;
/// Pad must be connected.
pub const MEDIA_PAD_FL_MUST_CONNECT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Link flags
// ---------------------------------------------------------------------------

/// Link is enabled.
pub const MEDIA_LNK_FL_ENABLED: u32 = 1 << 0;
/// Link cannot be changed (immutable).
pub const MEDIA_LNK_FL_IMMUTABLE: u32 = 1 << 1;
/// Link is dynamic (can be en/disabled at runtime).
pub const MEDIA_LNK_FL_DYNAMIC: u32 = 1 << 2;
/// Data link (carries data).
pub const MEDIA_LNK_FL_LINK_TYPE: u32 = 0x0000_000F;
/// Data link type.
pub const MEDIA_LNK_FL_DATA_LINK: u32 = 0;
/// Interface link type.
pub const MEDIA_LNK_FL_INTERFACE_LINK: u32 = 1;
/// Ancillary link type.
pub const MEDIA_LNK_FL_ANCILLARY_LINK: u32 = 2;

// ---------------------------------------------------------------------------
// Media ioctl commands
// ---------------------------------------------------------------------------

/// Get device info.
pub const MEDIA_IOC_DEVICE_INFO: u32 = 0x8100_4D00;
/// Enumerate entities.
pub const MEDIA_IOC_ENUM_ENTITIES: u32 = 0xC1F8_4D01;
/// Enumerate links.
pub const MEDIA_IOC_ENUM_LINKS: u32 = 0xC030_4D02;
/// Setup link.
pub const MEDIA_IOC_SETUP_LINK: u32 = 0xC048_4D03;
/// Get topology.
pub const MEDIA_IOC_G_TOPOLOGY: u32 = 0xC038_4D04;
/// Request allocate.
pub const MEDIA_IOC_REQUEST_ALLOC: u32 = 0xC004_4D05;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_types_distinct() {
        let types = [
            MEDIA_ENT_F_UNKNOWN, MEDIA_ENT_F_IO_V4L,
            MEDIA_ENT_F_IO_DTV, MEDIA_ENT_F_IO_VBI,
            MEDIA_ENT_F_IO_SWRADIO, MEDIA_ENT_F_V4L2_SUBDEV_UNKNOWN,
            MEDIA_ENT_F_DTV_DEMOD, MEDIA_ENT_F_CAM_SENSOR,
            MEDIA_ENT_F_FLASH, MEDIA_ENT_F_LENS,
            MEDIA_ENT_F_TUNER, MEDIA_ENT_F_PROC_VIDEO_ISP,
            MEDIA_ENT_F_PROC_VIDEO_PIXEL_FORMATTER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pad_flags_are_powers_of_two() {
        let flags = [
            MEDIA_PAD_FL_SINK, MEDIA_PAD_FL_SOURCE,
            MEDIA_PAD_FL_MUST_CONNECT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_link_flags_bits() {
        assert!(MEDIA_LNK_FL_ENABLED.is_power_of_two());
        assert!(MEDIA_LNK_FL_IMMUTABLE.is_power_of_two());
        assert!(MEDIA_LNK_FL_DYNAMIC.is_power_of_two());
    }

    #[test]
    fn test_entity_flags() {
        assert!(MEDIA_ENT_FL_DEFAULT.is_power_of_two());
        assert!(MEDIA_ENT_FL_CONNECTOR.is_power_of_two());
    }

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            MEDIA_IOC_DEVICE_INFO, MEDIA_IOC_ENUM_ENTITIES,
            MEDIA_IOC_ENUM_LINKS, MEDIA_IOC_SETUP_LINK,
            MEDIA_IOC_G_TOPOLOGY, MEDIA_IOC_REQUEST_ALLOC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_link_type_values() {
        assert_eq!(MEDIA_LNK_FL_DATA_LINK, 0);
        assert_eq!(MEDIA_LNK_FL_INTERFACE_LINK, 1);
        assert_eq!(MEDIA_LNK_FL_ANCILLARY_LINK, 2);
    }
}
