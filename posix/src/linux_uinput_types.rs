//! `<linux/uinput.h>` — User-space input device constants.
//!
//! uinput allows userspace programs to create virtual input devices
//! (/dev/uinput). The virtual device appears as a real device to the
//! rest of the system. Used by remote desktop software, custom input
//! drivers, accessibility tools, and testing frameworks to inject
//! keyboard, mouse, and touch events.

// ---------------------------------------------------------------------------
// uinput ioctl commands
// ---------------------------------------------------------------------------

/// Create the virtual device.
pub const UI_DEV_CREATE: u32 = 0x5501;
/// Destroy the virtual device.
pub const UI_DEV_DESTROY: u32 = 0x5502;
/// Set up the device (new API, replaces write to /dev/uinput).
pub const UI_DEV_SETUP: u32 = 0x405C_5503;
/// Enable an event type.
pub const UI_SET_EVBIT: u32 = 0x4004_5564;
/// Enable a key/button code.
pub const UI_SET_KEYBIT: u32 = 0x4004_5565;
/// Enable a relative axis.
pub const UI_SET_RELBIT: u32 = 0x4004_5566;
/// Enable an absolute axis.
pub const UI_SET_ABSBIT: u32 = 0x4004_5567;
/// Enable a misc event.
pub const UI_SET_MSCBIT: u32 = 0x4004_5568;
/// Enable an LED.
pub const UI_SET_LEDBIT: u32 = 0x4004_5569;
/// Enable a sound event.
pub const UI_SET_SNDBIT: u32 = 0x4004_556A;
/// Enable a force-feedback effect.
pub const UI_SET_FFBIT: u32 = 0x4004_556B;
/// Enable a switch.
pub const UI_SET_SWBIT: u32 = 0x4004_556D;
/// Enable a property.
pub const UI_SET_PROPBIT: u32 = 0x4004_556E;
/// Get the sysfs device path.
pub const UI_GET_SYSNAME: u32 = 0x814C_552C;
/// Get protocol version.
pub const UI_GET_VERSION: u32 = 0x8004_552D;

// ---------------------------------------------------------------------------
// Event types (EV_*)
// ---------------------------------------------------------------------------

/// Synchronization events.
pub const EV_SYN: u32 = 0x00;
/// Key/button press/release.
pub const EV_KEY: u32 = 0x01;
/// Relative axis movement (mouse).
pub const EV_REL: u32 = 0x02;
/// Absolute axis position (touchscreen, tablet).
pub const EV_ABS: u32 = 0x03;
/// Miscellaneous events.
pub const EV_MSC: u32 = 0x04;
/// Switch events (lid, headphone jack).
pub const EV_SW: u32 = 0x05;
/// LED control.
pub const EV_LED: u32 = 0x11;
/// Sound output.
pub const EV_SND: u32 = 0x12;
/// Force feedback.
pub const EV_FF: u32 = 0x15;

// ---------------------------------------------------------------------------
// Maximum values
// ---------------------------------------------------------------------------

/// Maximum event type value.
pub const EV_MAX: u32 = 0x1F;
/// Maximum key code.
pub const KEY_MAX: u32 = 0x2FF;
/// Maximum number of uinput devices.
pub const UINPUT_MAX_NAME_SIZE: u32 = 80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            UI_DEV_CREATE,
            UI_DEV_DESTROY,
            UI_DEV_SETUP,
            UI_SET_EVBIT,
            UI_SET_KEYBIT,
            UI_SET_RELBIT,
            UI_SET_ABSBIT,
            UI_SET_MSCBIT,
            UI_SET_LEDBIT,
            UI_SET_SNDBIT,
            UI_SET_FFBIT,
            UI_SET_SWBIT,
            UI_SET_PROPBIT,
            UI_GET_SYSNAME,
            UI_GET_VERSION,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_event_types_distinct() {
        let evs = [
            EV_SYN, EV_KEY, EV_REL, EV_ABS, EV_MSC, EV_SW, EV_LED, EV_SND, EV_FF,
        ];
        for i in 0..evs.len() {
            for j in (i + 1)..evs.len() {
                assert_ne!(evs[i], evs[j]);
            }
        }
    }

    #[test]
    fn test_event_types_within_max() {
        assert!(EV_KEY <= EV_MAX);
        assert!(EV_FF <= EV_MAX);
    }

    #[test]
    fn test_name_size() {
        assert_eq!(UINPUT_MAX_NAME_SIZE, 80);
    }
}
