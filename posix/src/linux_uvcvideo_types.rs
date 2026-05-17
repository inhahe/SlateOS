//! `<linux/uvcvideo.h>` — USB Video Class (UVC) constants.
//!
//! UVC is the standard USB device class for webcams and video capture
//! devices. The Linux UVC driver (uvcvideo) exposes V4L2 devices with
//! additional UVC-specific controls accessible through extension unit
//! (XU) ioctls. These constants define the UVC protocol's control
//! selectors and extension unit interface.

// ---------------------------------------------------------------------------
// UVC ioctl commands (extension units)
// ---------------------------------------------------------------------------

/// Query an extension unit control.
pub const UVCIOC_CTRL_QUERY: u32 = 0xC00C_5521;
/// Map an extension unit control to V4L2.
pub const UVCIOC_CTRL_MAP: u32 = 0xC060_5520;

// ---------------------------------------------------------------------------
// Control query request types
// ---------------------------------------------------------------------------

/// Set the current value.
pub const UVC_SET_CUR: u32 = 0x01;
/// Get the current value.
pub const UVC_GET_CUR: u32 = 0x81;
/// Get the minimum value.
pub const UVC_GET_MIN: u32 = 0x82;
/// Get the maximum value.
pub const UVC_GET_MAX: u32 = 0x83;
/// Get the resolution (step size).
pub const UVC_GET_RES: u32 = 0x84;
/// Get the length of the control.
pub const UVC_GET_LEN: u32 = 0x85;
/// Get info (capabilities) of the control.
pub const UVC_GET_INFO: u32 = 0x86;
/// Get the default value.
pub const UVC_GET_DEF: u32 = 0x87;

// ---------------------------------------------------------------------------
// Processing unit control selectors
// ---------------------------------------------------------------------------

/// Brightness control.
pub const UVC_PU_BRIGHTNESS_CONTROL: u32 = 0x02;
/// Contrast control.
pub const UVC_PU_CONTRAST_CONTROL: u32 = 0x03;
/// Gain control.
pub const UVC_PU_GAIN_CONTROL: u32 = 0x04;
/// Saturation control.
pub const UVC_PU_SATURATION_CONTROL: u32 = 0x07;
/// Sharpness control.
pub const UVC_PU_SHARPNESS_CONTROL: u32 = 0x08;
/// White balance temperature.
pub const UVC_PU_WHITE_BALANCE_TEMPERATURE_CONTROL: u32 = 0x0A;
/// Backlight compensation.
pub const UVC_PU_BACKLIGHT_COMPENSATION_CONTROL: u32 = 0x01;
/// Power line frequency (anti-flicker).
pub const UVC_PU_POWER_LINE_FREQUENCY_CONTROL: u32 = 0x05;

// ---------------------------------------------------------------------------
// Camera terminal control selectors
// ---------------------------------------------------------------------------

/// Auto-exposure mode.
pub const UVC_CT_AE_MODE_CONTROL: u32 = 0x02;
/// Exposure time (absolute).
pub const UVC_CT_EXPOSURE_TIME_ABSOLUTE_CONTROL: u32 = 0x04;
/// Focus (absolute).
pub const UVC_CT_FOCUS_ABSOLUTE_CONTROL: u32 = 0x06;
/// Focus auto.
pub const UVC_CT_FOCUS_AUTO_CONTROL: u32 = 0x08;
/// Zoom (absolute).
pub const UVC_CT_ZOOM_ABSOLUTE_CONTROL: u32 = 0x0B;
/// Pan/tilt (absolute).
pub const UVC_CT_PANTILT_ABSOLUTE_CONTROL: u32 = 0x0D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        assert_ne!(UVCIOC_CTRL_QUERY, UVCIOC_CTRL_MAP);
    }

    #[test]
    fn test_query_requests_distinct() {
        let reqs = [
            UVC_SET_CUR, UVC_GET_CUR, UVC_GET_MIN, UVC_GET_MAX,
            UVC_GET_RES, UVC_GET_LEN, UVC_GET_INFO, UVC_GET_DEF,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_pu_controls_distinct() {
        let ctrls = [
            UVC_PU_BACKLIGHT_COMPENSATION_CONTROL,
            UVC_PU_BRIGHTNESS_CONTROL,
            UVC_PU_CONTRAST_CONTROL,
            UVC_PU_GAIN_CONTROL,
            UVC_PU_POWER_LINE_FREQUENCY_CONTROL,
            UVC_PU_SATURATION_CONTROL,
            UVC_PU_SHARPNESS_CONTROL,
            UVC_PU_WHITE_BALANCE_TEMPERATURE_CONTROL,
        ];
        for i in 0..ctrls.len() {
            for j in (i + 1)..ctrls.len() {
                assert_ne!(ctrls[i], ctrls[j]);
            }
        }
    }

    #[test]
    fn test_ct_controls_distinct() {
        let ctrls = [
            UVC_CT_AE_MODE_CONTROL,
            UVC_CT_EXPOSURE_TIME_ABSOLUTE_CONTROL,
            UVC_CT_FOCUS_ABSOLUTE_CONTROL,
            UVC_CT_FOCUS_AUTO_CONTROL,
            UVC_CT_ZOOM_ABSOLUTE_CONTROL,
            UVC_CT_PANTILT_ABSOLUTE_CONTROL,
        ];
        for i in 0..ctrls.len() {
            for j in (i + 1)..ctrls.len() {
                assert_ne!(ctrls[i], ctrls[j]);
            }
        }
    }
}
