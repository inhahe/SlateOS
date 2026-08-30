//! `<linux/videodev2.h>` (control subset) — V4L2 control class and ID codes.
//!
//! V4L2 controls expose tuneable device parameters (brightness,
//! contrast, gain, exposure, etc.) to userspace. Controls are
//! organised into classes (user, camera, codec) and identified
//! by unique IDs. Applications enumerate available controls via
//! `VIDIOC_QUERYCTRL` and read/write them via `VIDIOC_G_CTRL`
//! and `VIDIOC_S_CTRL`.

// ---------------------------------------------------------------------------
// Control classes
// ---------------------------------------------------------------------------

/// User-class controls (basic image adjustments).
pub const V4L2_CTRL_CLASS_USER: u32 = 0x0098_0000;
/// Camera-class controls (exposure, focus, zoom).
pub const V4L2_CTRL_CLASS_CAMERA: u32 = 0x009A_0000;
/// FM modulator class.
pub const V4L2_CTRL_CLASS_FM_TX: u32 = 0x009B_0000;
/// Flash class (LED flash control).
pub const V4L2_CTRL_CLASS_FLASH: u32 = 0x009C_0000;
/// JPEG class (compression parameters).
pub const V4L2_CTRL_CLASS_JPEG: u32 = 0x009D_0000;
/// Image source class (sensor properties).
pub const V4L2_CTRL_CLASS_IMAGE_SOURCE: u32 = 0x009E_0000;
/// Image processing class.
pub const V4L2_CTRL_CLASS_IMAGE_PROC: u32 = 0x009F_0000;
/// Codec (stateful) class.
pub const V4L2_CTRL_CLASS_CODEC: u32 = 0x0099_0000;
/// FM receiver class.
pub const V4L2_CTRL_CLASS_FM_RX: u32 = 0x00A1_0000;
/// RF tuner class.
pub const V4L2_CTRL_CLASS_RF_TUNER: u32 = 0x00A2_0000;
/// Detect class (motion detection, face detection).
pub const V4L2_CTRL_CLASS_DETECT: u32 = 0x00A3_0000;
/// Codec (stateless) class.
pub const V4L2_CTRL_CLASS_CODEC_STATELESS: u32 = 0x00A4_0000;
/// Colorimetry class.
pub const V4L2_CTRL_CLASS_COLORIMETRY: u32 = 0x00A5_0000;

// ---------------------------------------------------------------------------
// User-class control IDs (CID base + offset)
// ---------------------------------------------------------------------------

/// CID base for user controls.
pub const V4L2_CID_BASE: u32 = V4L2_CTRL_CLASS_USER | 0x900;
/// Brightness.
pub const V4L2_CID_BRIGHTNESS: u32 = V4L2_CID_BASE;
/// Contrast.
pub const V4L2_CID_CONTRAST: u32 = V4L2_CID_BASE + 1;
/// Saturation.
pub const V4L2_CID_SATURATION: u32 = V4L2_CID_BASE + 2;
/// Hue.
pub const V4L2_CID_HUE: u32 = V4L2_CID_BASE + 3;
/// Audio volume.
pub const V4L2_CID_AUDIO_VOLUME: u32 = V4L2_CID_BASE + 5;
/// Audio balance.
pub const V4L2_CID_AUDIO_BALANCE: u32 = V4L2_CID_BASE + 6;
/// Audio bass.
pub const V4L2_CID_AUDIO_BASS: u32 = V4L2_CID_BASE + 7;
/// Audio treble.
pub const V4L2_CID_AUDIO_TREBLE: u32 = V4L2_CID_BASE + 8;
/// Audio mute.
pub const V4L2_CID_AUDIO_MUTE: u32 = V4L2_CID_BASE + 9;
/// Horizontal flip (mirror).
pub const V4L2_CID_HFLIP: u32 = V4L2_CID_BASE + 20;
/// Vertical flip.
pub const V4L2_CID_VFLIP: u32 = V4L2_CID_BASE + 21;
/// Power line frequency (anti-flicker).
pub const V4L2_CID_POWER_LINE_FREQUENCY: u32 = V4L2_CID_BASE + 24;
/// Sharpness.
pub const V4L2_CID_SHARPNESS: u32 = V4L2_CID_BASE + 27;
/// Backlight compensation.
pub const V4L2_CID_BACKLIGHT_COMPENSATION: u32 = V4L2_CID_BASE + 28;
/// Gain (analogue).
pub const V4L2_CID_GAIN: u32 = V4L2_CID_BASE + 19;

// ---------------------------------------------------------------------------
// Control types (v4l2_ctrl_type)
// ---------------------------------------------------------------------------

/// Integer control.
pub const V4L2_CTRL_TYPE_INTEGER: u32 = 1;
/// Boolean control.
pub const V4L2_CTRL_TYPE_BOOLEAN: u32 = 2;
/// Menu control (one of several named options).
pub const V4L2_CTRL_TYPE_MENU: u32 = 3;
/// Button control (trigger action).
pub const V4L2_CTRL_TYPE_BUTTON: u32 = 4;
/// 64-bit integer control.
pub const V4L2_CTRL_TYPE_INTEGER64: u32 = 5;
/// String control.
pub const V4L2_CTRL_TYPE_STRING: u32 = 7;
/// Bitmask control.
pub const V4L2_CTRL_TYPE_BITMASK: u32 = 8;
/// Integer menu control (menu with integer values).
pub const V4L2_CTRL_TYPE_INTEGER_MENU: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrl_classes_distinct() {
        let classes = [
            V4L2_CTRL_CLASS_USER,
            V4L2_CTRL_CLASS_CAMERA,
            V4L2_CTRL_CLASS_FM_TX,
            V4L2_CTRL_CLASS_FLASH,
            V4L2_CTRL_CLASS_JPEG,
            V4L2_CTRL_CLASS_IMAGE_SOURCE,
            V4L2_CTRL_CLASS_IMAGE_PROC,
            V4L2_CTRL_CLASS_CODEC,
            V4L2_CTRL_CLASS_FM_RX,
            V4L2_CTRL_CLASS_RF_TUNER,
            V4L2_CTRL_CLASS_DETECT,
            V4L2_CTRL_CLASS_CODEC_STATELESS,
            V4L2_CTRL_CLASS_COLORIMETRY,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_user_cids_derived_from_base() {
        assert_eq!(V4L2_CID_BRIGHTNESS, V4L2_CID_BASE);
        assert_eq!(V4L2_CID_CONTRAST, V4L2_CID_BASE + 1);
        assert_eq!(V4L2_CID_SATURATION, V4L2_CID_BASE + 2);
        assert_eq!(V4L2_CID_HUE, V4L2_CID_BASE + 3);
    }

    #[test]
    fn test_ctrl_types_distinct() {
        let types = [
            V4L2_CTRL_TYPE_INTEGER,
            V4L2_CTRL_TYPE_BOOLEAN,
            V4L2_CTRL_TYPE_MENU,
            V4L2_CTRL_TYPE_BUTTON,
            V4L2_CTRL_TYPE_INTEGER64,
            V4L2_CTRL_TYPE_STRING,
            V4L2_CTRL_TYPE_BITMASK,
            V4L2_CTRL_TYPE_INTEGER_MENU,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_classes_16bit_aligned() {
        // All classes have lower 16 bits = 0
        let classes = [
            V4L2_CTRL_CLASS_USER,
            V4L2_CTRL_CLASS_CAMERA,
            V4L2_CTRL_CLASS_CODEC,
        ];
        for &c in &classes {
            assert_eq!(c & 0xFFFF, 0, "class 0x{:08X} not 64K-aligned", c);
        }
    }
}
