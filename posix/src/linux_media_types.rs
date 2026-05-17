//! `<linux/media.h>` — Media controller API constants.
//!
//! The Media Controller API manages complex media pipelines (camera
//! sensors → ISP → video output, TV tuners → demodulators → demux).
//! It exposes the hardware topology as a graph of entities connected
//! by pads and links, allowing userspace to configure routing between
//! processing units.

// ---------------------------------------------------------------------------
// Entity types (major)
// ---------------------------------------------------------------------------

/// Base for all entity types.
pub const MEDIA_ENT_F_BASE: u32 = 0x0000_0000;
/// Unknown/unspecified entity function.
pub const MEDIA_ENT_F_UNKNOWN: u32 = MEDIA_ENT_F_BASE;
/// Default function (old API compat).
pub const MEDIA_ENT_F_V4L2_SUBDEV_UNKNOWN: u32 = 0x0002_0000;
/// DMA engine — video capture.
pub const MEDIA_ENT_F_IO_V4L: u32 = 0x0001_0001;
/// DMA engine — DVB demux.
pub const MEDIA_ENT_F_IO_DTV: u32 = 0x0001_0002;
/// DMA engine — video output.
pub const MEDIA_ENT_F_IO_VBI: u32 = 0x0001_0003;
/// Digital TV demodulator.
pub const MEDIA_ENT_F_DTV_DEMOD: u32 = 0x0002_0001;
/// Digital TV tuner.
pub const MEDIA_ENT_F_TUNER: u32 = 0x0002_0005;
/// Camera sensor.
pub const MEDIA_ENT_F_CAM_SENSOR: u32 = 0x0002_0101;
/// Flash LED.
pub const MEDIA_ENT_F_FLASH: u32 = 0x0002_0102;
/// Lens controller.
pub const MEDIA_ENT_F_LENS: u32 = 0x0002_0103;
/// Image Signal Processor.
pub const MEDIA_ENT_F_PROC_VIDEO_ISP: u32 = 0x0003_0001;

// ---------------------------------------------------------------------------
// Link flags
// ---------------------------------------------------------------------------

/// Link is enabled.
pub const MEDIA_LNK_FL_ENABLED: u32 = 1 << 0;
/// Link cannot be disabled.
pub const MEDIA_LNK_FL_IMMUTABLE: u32 = 1 << 1;
/// Link is dynamic (can change at runtime).
pub const MEDIA_LNK_FL_DYNAMIC: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Pad flags
// ---------------------------------------------------------------------------

/// Pad is a sink (receives data).
pub const MEDIA_PAD_FL_SINK: u32 = 1 << 0;
/// Pad is a source (produces data).
pub const MEDIA_PAD_FL_SOURCE: u32 = 1 << 1;
/// Pad must be connected.
pub const MEDIA_PAD_FL_MUST_CONNECT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Media controller ioctl commands
// ---------------------------------------------------------------------------

/// Get device info.
pub const MEDIA_IOC_DEVICE_INFO: u32 = 0xC100_7C00;
/// Enumerate entities.
pub const MEDIA_IOC_ENUM_ENTITIES: u32 = 0xC1F8_7C01;
/// Enumerate links.
pub const MEDIA_IOC_ENUM_LINKS: u32 = 0xC078_7C02;
/// Set up a link.
pub const MEDIA_IOC_SETUP_LINK: u32 = 0xC078_7C03;
/// Get topology (entities + pads + links).
pub const MEDIA_IOC_G_TOPOLOGY: u32 = 0xC080_7C04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_functions_distinct() {
        let funcs = [
            MEDIA_ENT_F_IO_V4L, MEDIA_ENT_F_IO_DTV, MEDIA_ENT_F_IO_VBI,
            MEDIA_ENT_F_DTV_DEMOD, MEDIA_ENT_F_TUNER,
            MEDIA_ENT_F_CAM_SENSOR, MEDIA_ENT_F_FLASH,
            MEDIA_ENT_F_LENS, MEDIA_ENT_F_PROC_VIDEO_ISP,
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
            MEDIA_LNK_FL_DYNAMIC,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
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
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            MEDIA_IOC_DEVICE_INFO, MEDIA_IOC_ENUM_ENTITIES,
            MEDIA_IOC_ENUM_LINKS, MEDIA_IOC_SETUP_LINK,
            MEDIA_IOC_G_TOPOLOGY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
