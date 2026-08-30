//! `<linux/videodev2.h>` (capability subset) — V4L2 device capability flags.
//!
//! Capability flags describe what a V4L2 device can do: capture video,
//! output video, support streaming I/O, provide hardware acceleration,
//! etc. Applications query capabilities via `VIDIOC_QUERYCAP` to
//! determine the correct I/O path before opening a stream.

// ---------------------------------------------------------------------------
// Device capability flags (v4l2_capability.capabilities)
// ---------------------------------------------------------------------------

/// Device supports video capture (single-plane).
pub const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
/// Device supports video output (single-plane).
pub const V4L2_CAP_VIDEO_OUTPUT: u32 = 0x0000_0002;
/// Device supports video overlay.
pub const V4L2_CAP_VIDEO_OVERLAY: u32 = 0x0000_0004;
/// Device supports raw VBI capture.
pub const V4L2_CAP_VBI_CAPTURE: u32 = 0x0000_0010;
/// Device supports raw VBI output.
pub const V4L2_CAP_VBI_OUTPUT: u32 = 0x0000_0020;
/// Device supports sliced VBI capture.
pub const V4L2_CAP_SLICED_VBI_CAPTURE: u32 = 0x0000_0040;
/// Device supports sliced VBI output.
pub const V4L2_CAP_SLICED_VBI_OUTPUT: u32 = 0x0000_0080;
/// Device supports RDS capture.
pub const V4L2_CAP_RDS_CAPTURE: u32 = 0x0000_0100;
/// Device supports video output overlay.
pub const V4L2_CAP_VIDEO_OUTPUT_OVERLAY: u32 = 0x0000_0200;
/// Device supports hardware frequency seeking.
pub const V4L2_CAP_HW_FREQ_SEEK: u32 = 0x0000_0400;
/// Device supports RDS output.
pub const V4L2_CAP_RDS_OUTPUT: u32 = 0x0000_0800;
/// Device supports multi-plane video capture.
pub const V4L2_CAP_VIDEO_CAPTURE_MPLANE: u32 = 0x0000_1000;
/// Device supports multi-plane video output.
pub const V4L2_CAP_VIDEO_OUTPUT_MPLANE: u32 = 0x0000_2000;
/// Device supports mem-to-mem (codec / scaler).
pub const V4L2_CAP_VIDEO_M2M_MPLANE: u32 = 0x0000_4000;
/// Device supports single-plane mem-to-mem.
pub const V4L2_CAP_VIDEO_M2M: u32 = 0x0000_8000;
/// Device has a tuner.
pub const V4L2_CAP_TUNER: u32 = 0x0001_0000;
/// Device has audio input.
pub const V4L2_CAP_AUDIO: u32 = 0x0002_0000;
/// Device has a radio receiver.
pub const V4L2_CAP_RADIO: u32 = 0x0004_0000;
/// Device has a modulator (radio transmitter).
pub const V4L2_CAP_MODULATOR: u32 = 0x0008_0000;
/// Device supports SDR capture.
pub const V4L2_CAP_SDR_CAPTURE: u32 = 0x0010_0000;
/// Device supports extended pixel format.
pub const V4L2_CAP_EXT_PIX_FORMAT: u32 = 0x0020_0000;
/// Device supports SDR output.
pub const V4L2_CAP_SDR_OUTPUT: u32 = 0x0040_0000;
/// Device supports metadata capture.
pub const V4L2_CAP_META_CAPTURE: u32 = 0x0080_0000;
/// Device supports read/write I/O.
pub const V4L2_CAP_READWRITE: u32 = 0x0100_0000;
/// Device supports streaming I/O (mmap or userptr).
pub const V4L2_CAP_STREAMING: u32 = 0x0400_0000;
/// Device supports metadata output.
pub const V4L2_CAP_META_OUTPUT: u32 = 0x0800_0000;
/// Device supports touch interface.
pub const V4L2_CAP_TOUCH: u32 = 0x1000_0000;
/// Device supports I/O Media Controller.
pub const V4L2_CAP_IO_MC: u32 = 0x2000_0000;
/// Capability is per device-node, not per device.
pub const V4L2_CAP_DEVICE_CAPS: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_flags_are_power_of_two() {
        let caps = [
            V4L2_CAP_VIDEO_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT,
            V4L2_CAP_VIDEO_OVERLAY,
            V4L2_CAP_VBI_CAPTURE,
            V4L2_CAP_VBI_OUTPUT,
            V4L2_CAP_SLICED_VBI_CAPTURE,
            V4L2_CAP_SLICED_VBI_OUTPUT,
            V4L2_CAP_RDS_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT_OVERLAY,
            V4L2_CAP_HW_FREQ_SEEK,
            V4L2_CAP_RDS_OUTPUT,
            V4L2_CAP_VIDEO_CAPTURE_MPLANE,
            V4L2_CAP_VIDEO_OUTPUT_MPLANE,
            V4L2_CAP_VIDEO_M2M_MPLANE,
            V4L2_CAP_VIDEO_M2M,
            V4L2_CAP_TUNER,
            V4L2_CAP_AUDIO,
            V4L2_CAP_RADIO,
            V4L2_CAP_MODULATOR,
            V4L2_CAP_SDR_CAPTURE,
            V4L2_CAP_EXT_PIX_FORMAT,
            V4L2_CAP_SDR_OUTPUT,
            V4L2_CAP_META_CAPTURE,
            V4L2_CAP_READWRITE,
            V4L2_CAP_STREAMING,
            V4L2_CAP_META_OUTPUT,
            V4L2_CAP_TOUCH,
            V4L2_CAP_IO_MC,
            V4L2_CAP_DEVICE_CAPS,
        ];
        for &c in &caps {
            assert!(c.is_power_of_two(), "cap 0x{:08X} is not power of two", c);
        }
    }

    #[test]
    fn test_cap_flags_no_overlap() {
        let caps = [
            V4L2_CAP_VIDEO_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT,
            V4L2_CAP_VIDEO_OVERLAY,
            V4L2_CAP_VBI_CAPTURE,
            V4L2_CAP_VBI_OUTPUT,
            V4L2_CAP_SLICED_VBI_CAPTURE,
            V4L2_CAP_SLICED_VBI_OUTPUT,
            V4L2_CAP_RDS_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT_OVERLAY,
            V4L2_CAP_READWRITE,
            V4L2_CAP_STREAMING,
            V4L2_CAP_DEVICE_CAPS,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(
                    caps[i] & caps[j],
                    0,
                    "caps 0x{:08X} and 0x{:08X} overlap",
                    caps[i],
                    caps[j]
                );
            }
        }
    }

    #[test]
    fn test_common_caps() {
        assert_eq!(V4L2_CAP_VIDEO_CAPTURE, 1);
        assert_eq!(V4L2_CAP_STREAMING, 0x0400_0000);
        assert_eq!(V4L2_CAP_DEVICE_CAPS, 0x8000_0000);
    }
}
