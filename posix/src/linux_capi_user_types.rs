//! `<linux/capi.h>` — Common ISDN Application Programming Interface.
//!
//! The CAPI 2.0 device (`/dev/capi20`) exposes ISDN controllers through
//! a unified API. This module covers the ioctls, the message header
//! format, and the well-known limits.

// ---------------------------------------------------------------------------
// Device path
// ---------------------------------------------------------------------------

pub const CAPI_DEVICE_PATH: &str = "/dev/capi20";

// ---------------------------------------------------------------------------
// `CAPI_*` ioctls (type 0x43 = 'C')
// ---------------------------------------------------------------------------

/// `_IOR('C', 0x21, u32)` — register application.
pub const CAPI_REGISTER: u32 = 0x4004_4321;

/// `_IOR('C', 0x22, [u8; 64])` — get manufacturer.
pub const CAPI_GET_MANUFACTURER: u32 = 0x4040_4322;

/// `_IOR('C', 0x23, struct capi_version)` — get protocol version.
pub const CAPI_GET_VERSION: u32 = 0x4010_4323;

/// `_IOR('C', 0x24, struct capi_serial_number)` — get serial number.
pub const CAPI_GET_SERIAL: u32 = 0x4008_4324;

/// `_IOR('C', 0x25, struct capi_profile)` — get controller profile.
pub const CAPI_GET_PROFILE: u32 = 0x4040_4325;

/// `_IO('C', 0x26)` — install firmware.
pub const CAPI_INSTALLED: u32 = 0x0000_4326;

// ---------------------------------------------------------------------------
// Message limits
// ---------------------------------------------------------------------------

/// Maximum CAPI message length (including header).
pub const CAPI_MAX_MSG_SIZE: usize = 2_048;

/// Length of the fixed CAPI message header (`Length`+`AppID`+`Command`+`Subcommand`+`MsgNumber`).
pub const CAPI_MSG_HEADER_SIZE: usize = 8;

/// Maximum simultaneous applications a CAPI controller can support.
pub const CAPI_MAX_APPLICATIONS: u32 = 240;

// ---------------------------------------------------------------------------
// CAPI message field offsets
// ---------------------------------------------------------------------------

pub const CAPI_MSG_OFF_LENGTH: usize = 0;
pub const CAPI_MSG_OFF_APP_ID: usize = 2;
pub const CAPI_MSG_OFF_COMMAND: usize = 4;
pub const CAPI_MSG_OFF_SUBCOMMAND: usize = 5;
pub const CAPI_MSG_OFF_MSG_NUMBER: usize = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_path_canonical() {
        assert_eq!(CAPI_DEVICE_PATH, "/dev/capi20");
        assert!(CAPI_DEVICE_PATH.starts_with("/dev/"));
    }

    #[test]
    fn test_ioctls_in_c_type_byte() {
        for v in [
            CAPI_REGISTER,
            CAPI_GET_MANUFACTURER,
            CAPI_GET_VERSION,
            CAPI_GET_SERIAL,
            CAPI_GET_PROFILE,
            CAPI_INSTALLED,
        ] {
            // All start with type byte 'C' = 0x43.
            assert_eq!((v >> 8) & 0xFF, 0x43);
        }
        // Number sub-field 0x21..0x26 is dense.
        assert_eq!(CAPI_INSTALLED & 0xFF, 0x26);
    }

    #[test]
    fn test_ior_direction_on_query_ioctls() {
        // _IOR sets bit 30 in the top word (0x4000_0000).
        for v in [
            CAPI_REGISTER,
            CAPI_GET_MANUFACTURER,
            CAPI_GET_VERSION,
            CAPI_GET_SERIAL,
            CAPI_GET_PROFILE,
        ] {
            assert_eq!(v >> 30, 0x1);
        }
        // CAPI_INSTALLED is _IO (no direction bits).
        assert_eq!(CAPI_INSTALLED >> 30, 0);
    }

    #[test]
    fn test_message_layout_offsets() {
        assert_eq!(CAPI_MSG_OFF_LENGTH, 0);
        assert_eq!(CAPI_MSG_OFF_APP_ID, 2);
        assert_eq!(CAPI_MSG_OFF_COMMAND, 4);
        assert_eq!(CAPI_MSG_OFF_SUBCOMMAND, 5);
        assert_eq!(CAPI_MSG_OFF_MSG_NUMBER, 6);
        // Header is 8 bytes: length(2) + app_id(2) + command(1) +
        // subcommand(1) + msg_number(2).
        assert_eq!(CAPI_MSG_HEADER_SIZE, 8);
    }

    #[test]
    fn test_message_size_limit() {
        assert_eq!(CAPI_MAX_MSG_SIZE, 2_048);
        assert!(CAPI_MAX_MSG_SIZE.is_power_of_two());
        // Header is small fraction of total message.
        assert!(CAPI_MSG_HEADER_SIZE < CAPI_MAX_MSG_SIZE / 8);
    }

    #[test]
    fn test_max_applications_fits_in_u8() {
        // app-id is a u16 field; 240 is the kernel's per-driver cap.
        assert_eq!(CAPI_MAX_APPLICATIONS, 240);
        assert!(CAPI_MAX_APPLICATIONS < 256);
    }
}
