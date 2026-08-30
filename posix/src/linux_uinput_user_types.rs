//! `<linux/uinput.h>` — `/dev/uinput` userspace input-device transport.
//!
//! uinput is the workhorse for synthesizing keyboard/mouse/joystick
//! events from userspace: Wayland's libei, gamepad mappers (`Steam
//! Input`, `antimicrox`), and AT bridges open `/dev/uinput`,
//! configure the device with the ioctls below, then `write()` event
//! records of struct input_event.

// ---------------------------------------------------------------------------
// Magic letter
// ---------------------------------------------------------------------------

/// Magic letter for /dev/uinput ioctls ('U').
pub const UINPUT_IOCTL_BASE: u8 = b'U';

/// Maximum name length in `struct uinput_user_dev.name`.
pub const UINPUT_MAX_NAME_SIZE: usize = 80;

// ---------------------------------------------------------------------------
// Core lifecycle ioctls
// ---------------------------------------------------------------------------

/// `UI_DEV_CREATE` — finalize device with the configured event mask.
pub const UI_DEV_CREATE: u32 = 0x0000_5501;
/// `UI_DEV_DESTROY` — tear down the device.
pub const UI_DEV_DESTROY: u32 = 0x0000_5502;
/// `UI_DEV_SETUP` — set up the device using struct uinput_setup.
pub const UI_DEV_SETUP: u32 = 0x4054_5503;
/// `UI_ABS_SETUP` — set up an absolute axis with struct uinput_abs_setup.
pub const UI_ABS_SETUP: u32 = 0x401c_5504;

// ---------------------------------------------------------------------------
// Enable event types
// ---------------------------------------------------------------------------

/// `UI_SET_EVBIT` — enable an EV_KEY/EV_REL/EV_ABS/… type.
pub const UI_SET_EVBIT: u32 = 0x4004_5564;
/// `UI_SET_KEYBIT` — enable a KEY_* / BTN_* code.
pub const UI_SET_KEYBIT: u32 = 0x4004_5565;
/// `UI_SET_RELBIT` — enable a REL_* code.
pub const UI_SET_RELBIT: u32 = 0x4004_5566;
/// `UI_SET_ABSBIT` — enable an ABS_* code.
pub const UI_SET_ABSBIT: u32 = 0x4004_5567;
/// `UI_SET_MSCBIT` — enable an MSC_* code.
pub const UI_SET_MSCBIT: u32 = 0x4004_5568;
/// `UI_SET_LEDBIT` — enable an LED_* code.
pub const UI_SET_LEDBIT: u32 = 0x4004_5569;
/// `UI_SET_SNDBIT` — enable a SND_* code.
pub const UI_SET_SNDBIT: u32 = 0x4004_556a;
/// `UI_SET_FFBIT` — enable an FF_* code (force feedback).
pub const UI_SET_FFBIT: u32 = 0x4004_556b;
/// `UI_SET_PHYS` — set physical-location string (pointer arg).
pub const UI_SET_PHYS: u32 = 0x4008_556c;
/// `UI_SET_SWBIT` — enable a SW_* code (switch).
pub const UI_SET_SWBIT: u32 = 0x4004_556d;
/// `UI_SET_PROPBIT` — enable an INPUT_PROP_* property.
pub const UI_SET_PROPBIT: u32 = 0x4004_556e;

// ---------------------------------------------------------------------------
// Force-feedback feedback channel
// ---------------------------------------------------------------------------

/// `UI_BEGIN_FF_UPLOAD` — pop a pending FF upload request.
pub const UI_BEGIN_FF_UPLOAD: u32 = 0xc0a8_55c8;
/// `UI_END_FF_UPLOAD` — return the result of an upload.
pub const UI_END_FF_UPLOAD: u32 = 0x40a8_55c9;
/// `UI_BEGIN_FF_ERASE` — pop a pending FF erase request.
pub const UI_BEGIN_FF_ERASE: u32 = 0xc00c_55ca;
/// `UI_END_FF_ERASE` — return the result of an erase.
pub const UI_END_FF_ERASE: u32 = 0x400c_55cb;

// ---------------------------------------------------------------------------
// Returned device-attribute queries
// ---------------------------------------------------------------------------

/// `UI_GET_VERSION` — fetch the protocol version (u32 out).
pub const UI_GET_VERSION: u32 = 0x8004_552d;
/// `UI_GET_SYSNAME` — fetch sysfs name (variable-len string).
pub const UI_GET_SYSNAME_BASE: u32 = 0x8000_552c;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_u() {
        assert_eq!(UINPUT_IOCTL_BASE, b'U');
    }

    #[test]
    fn test_name_size() {
        // 80 chars matches /sys/class/input/*/name limits.
        assert_eq!(UINPUT_MAX_NAME_SIZE, 80);
    }

    #[test]
    fn test_lifecycle_ioctls_distinct_and_letter_u() {
        let l = [UI_DEV_CREATE, UI_DEV_DESTROY, UI_DEV_SETUP, UI_ABS_SETUP];
        for i in 0..l.len() {
            for j in (i + 1)..l.len() {
                assert_ne!(l[i], l[j]);
            }
            // Type byte 'U' (0x55) in bits 8..15.
            assert_eq!((l[i] >> 8) & 0xff, b'U' as u32);
        }
    }

    #[test]
    fn test_setbit_ioctls_distinct_and_letter_u() {
        let s = [
            UI_SET_EVBIT,
            UI_SET_KEYBIT,
            UI_SET_RELBIT,
            UI_SET_ABSBIT,
            UI_SET_MSCBIT,
            UI_SET_LEDBIT,
            UI_SET_SNDBIT,
            UI_SET_FFBIT,
            UI_SET_PHYS,
            UI_SET_SWBIT,
            UI_SET_PROPBIT,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
            assert_eq!((s[i] >> 8) & 0xff, b'U' as u32);
        }
    }

    #[test]
    fn test_ff_ioctls_distinct() {
        let f = [
            UI_BEGIN_FF_UPLOAD,
            UI_END_FF_UPLOAD,
            UI_BEGIN_FF_ERASE,
            UI_END_FF_ERASE,
        ];
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
            assert_eq!((f[i] >> 8) & 0xff, b'U' as u32);
        }
    }

    #[test]
    fn test_get_ioctls_use_letter_u() {
        assert_eq!((UI_GET_VERSION >> 8) & 0xff, b'U' as u32);
        assert_eq!((UI_GET_SYSNAME_BASE >> 8) & 0xff, b'U' as u32);
        assert_ne!(UI_GET_VERSION, UI_GET_SYSNAME_BASE);
    }
}
