//! `<linux/uinput.h>` — userspace input device creation.
//!
//! The uinput module allows userspace processes to create virtual
//! input devices (keyboards, mice, gamepads, etc.) via
//! `/dev/uinput`. Used by xdotool, Steam Input, and remote desktop.

pub use crate::linux_input::InputEvent;
pub use crate::linux_input::EV_SYN;
pub use crate::linux_input::EV_KEY;
pub use crate::linux_input::EV_REL;
pub use crate::linux_input::EV_ABS;

// ---------------------------------------------------------------------------
// uinput ioctl commands
// ---------------------------------------------------------------------------

/// Set event type bits.
pub const UI_SET_EVBIT: u64 = 0x40046564;
/// Set key/button bits.
pub const UI_SET_KEYBIT: u64 = 0x40046565;
/// Set relative axis bits.
pub const UI_SET_RELBIT: u64 = 0x40046566;
/// Set absolute axis bits.
pub const UI_SET_ABSBIT: u64 = 0x40046567;
/// Set misc bits.
pub const UI_SET_MSCBIT: u64 = 0x40046568;
/// Set LED bits.
pub const UI_SET_LEDBIT: u64 = 0x40046569;
/// Set sound bits.
pub const UI_SET_SNDBIT: u64 = 0x4004656A;
/// Set force-feedback bits.
pub const UI_SET_FFBIT: u64 = 0x4004656B;
/// Set switch bits.
pub const UI_SET_SWBIT: u64 = 0x4004656D;
/// Set property bits.
pub const UI_SET_PROPBIT: u64 = 0x4004656E;

/// Create the device.
pub const UI_DEV_CREATE: u64 = 0x5501;
/// Destroy the device.
pub const UI_DEV_DESTROY: u64 = 0x5502;

/// Create device with new-style setup.
pub const UI_DEV_SETUP: u64 = 0x405C5503;
/// Set absolute axis parameters.
pub const UI_ABS_SETUP: u64 = 0x40185504;

/// Get system-assigned device name.
pub const UI_GET_SYSNAME_BASE: u64 = 0x8000552C;
/// Get device version.
pub const UI_GET_VERSION: u64 = 0x8004552D;

// ---------------------------------------------------------------------------
// uinput_setup struct
// ---------------------------------------------------------------------------

/// Maximum device name length.
pub const UINPUT_MAX_NAME_SIZE: usize = 80;

/// Input device ID (matches `struct input_id`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InputId {
    /// Bus type.
    pub bustype: u16,
    /// Vendor ID.
    pub vendor: u16,
    /// Product ID.
    pub product: u16,
    /// Version.
    pub version: u16,
}

/// New-style uinput device setup.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UinputSetup {
    /// Device ID.
    pub id: InputId,
    /// Device name.
    pub name: [u8; UINPUT_MAX_NAME_SIZE],
    /// Force feedback effects max.
    pub ff_effects_max: u32,
}

impl UinputSetup {
    /// Create a zeroed uinput setup.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Absolute axis setup.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UinputAbsSetup {
    /// Axis code.
    pub code: u16,
    /// Padding.
    _pad: u16,
    /// Minimum value.
    pub minimum: i32,
    /// Maximum value.
    pub maximum: i32,
    /// Fuzz (noise filter).
    pub fuzz: i32,
    /// Flat (dead zone).
    pub flat: i32,
    /// Resolution.
    pub resolution: i32,
}

impl UinputAbsSetup {
    /// Create a zeroed absolute axis setup.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_id_size() {
        assert_eq!(core::mem::size_of::<InputId>(), 8);
    }

    #[test]
    fn test_uinput_setup_zeroed() {
        let setup = UinputSetup::zeroed();
        assert_eq!(setup.id.bustype, 0);
        assert_eq!(setup.id.vendor, 0);
        assert_eq!(setup.ff_effects_max, 0);
        assert_eq!(setup.name[0], 0);
    }

    #[test]
    fn test_uinput_abs_setup_zeroed() {
        let abs = UinputAbsSetup::zeroed();
        assert_eq!(abs.code, 0);
        assert_eq!(abs.minimum, 0);
        assert_eq!(abs.maximum, 0);
        assert_eq!(abs.fuzz, 0);
        assert_eq!(abs.flat, 0);
    }

    #[test]
    fn test_ioctl_set_commands_distinct() {
        let cmds = [
            UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_RELBIT,
            UI_SET_ABSBIT, UI_SET_MSCBIT, UI_SET_LEDBIT,
            UI_SET_SNDBIT, UI_SET_FFBIT, UI_SET_SWBIT,
            UI_SET_PROPBIT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dev_create_destroy() {
        assert_ne!(UI_DEV_CREATE, UI_DEV_DESTROY);
    }

    #[test]
    fn test_cross_module_ev_types() {
        assert_eq!(EV_SYN, crate::linux_input::EV_SYN);
        assert_eq!(EV_KEY, crate::linux_input::EV_KEY);
        assert_eq!(EV_REL, crate::linux_input::EV_REL);
        assert_eq!(EV_ABS, crate::linux_input::EV_ABS);
    }
}
