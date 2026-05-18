//! `<linux/uinput.h>` — Additional uinput constants.
//!
//! Supplementary uinput constants covering device setup,
//! IOCTL commands, and ABS axis configuration.

// ---------------------------------------------------------------------------
// IOCTL commands
// ---------------------------------------------------------------------------

/// Create device.
pub const UI_DEV_CREATE: u32 = 0x00005501;
/// Destroy device.
pub const UI_DEV_DESTROY: u32 = 0x00005502;
/// Device setup.
pub const UI_DEV_SETUP: u32 = 0x405C5503;
/// ABS setup.
pub const UI_ABS_SETUP: u32 = 0x40185504;

// ---------------------------------------------------------------------------
// Set event type IOCTLs
// ---------------------------------------------------------------------------

/// Set EV bit.
pub const UI_SET_EVBIT: u32 = 0x40045564;
/// Set KEY bit.
pub const UI_SET_KEYBIT: u32 = 0x40045565;
/// Set REL bit.
pub const UI_SET_RELBIT: u32 = 0x40045566;
/// Set ABS bit.
pub const UI_SET_ABSBIT: u32 = 0x40045567;
/// Set MSC bit.
pub const UI_SET_MSCBIT: u32 = 0x40045568;
/// Set LED bit.
pub const UI_SET_LEDBIT: u32 = 0x40045569;
/// Set SND bit.
pub const UI_SET_SNDBIT: u32 = 0x4004556A;
/// Set FF bit.
pub const UI_SET_FFBIT: u32 = 0x4004556B;
/// Set PHYS.
pub const UI_SET_PHYS: u32 = 0x4004556C;
/// Set SW bit.
pub const UI_SET_SWBIT: u32 = 0x4004556D;
/// Set PROP bit.
pub const UI_SET_PROPBIT: u32 = 0x4004556E;

// ---------------------------------------------------------------------------
// Feedback effects
// ---------------------------------------------------------------------------

/// Begin FF upload.
pub const UI_BEGIN_FF_UPLOAD: u32 = 0xC06855C8;
/// End FF upload.
pub const UI_END_FF_UPLOAD: u32 = 0x406855C9;
/// Begin FF erase.
pub const UI_BEGIN_FF_ERASE: u32 = 0xC01055CA;
/// End FF erase.
pub const UI_END_FF_ERASE: u32 = 0x401055CB;

// ---------------------------------------------------------------------------
// Max sizes
// ---------------------------------------------------------------------------

/// Max name size.
pub const UINPUT_MAX_NAME_SIZE: u32 = 80;

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// uinput version.
pub const UINPUT_VERSION: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_ioctls_distinct() {
        let ioctls = [UI_DEV_CREATE, UI_DEV_DESTROY, UI_DEV_SETUP, UI_ABS_SETUP];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_set_ioctls_distinct() {
        let ioctls = [
            UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_RELBIT,
            UI_SET_ABSBIT, UI_SET_MSCBIT, UI_SET_LEDBIT,
            UI_SET_SNDBIT, UI_SET_FFBIT, UI_SET_PHYS,
            UI_SET_SWBIT, UI_SET_PROPBIT,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_ff_ioctls_distinct() {
        let ioctls = [
            UI_BEGIN_FF_UPLOAD, UI_END_FF_UPLOAD,
            UI_BEGIN_FF_ERASE, UI_END_FF_ERASE,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_max_name_size() {
        assert_eq!(UINPUT_MAX_NAME_SIZE, 80);
    }

    #[test]
    fn test_version() {
        assert_eq!(UINPUT_VERSION, 5);
    }
}
