//! `<linux/firmware.h>` — Firmware loading subsystem constants.
//!
//! The firmware loading subsystem allows kernel drivers to request
//! firmware files (binary blobs) from userspace at runtime. Files
//! are typically stored in /lib/firmware/ and loaded via the
//! request_firmware() API or the uevent-based firmware loader.

// ---------------------------------------------------------------------------
// Firmware loading flags
// ---------------------------------------------------------------------------

/// Optional firmware (don't warn if missing).
pub const FW_OPT_OPTIONAL: u32 = 1 << 0;
/// No caching (don't store in kernel memory after load).
pub const FW_OPT_NOCACHE: u32 = 1 << 1;
/// Uevent-based loading (userspace helper).
pub const FW_OPT_UEVENT: u32 = 1 << 2;
/// No uevent (direct filesystem access only).
pub const FW_OPT_NO_UEVENT: u32 = 1 << 3;
/// Partial loading (load in chunks).
pub const FW_OPT_PARTIAL: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Firmware upload status
// ---------------------------------------------------------------------------

/// Upload not started.
pub const FW_UPLOAD_STATUS_IDLE: u32 = 0;
/// Upload in progress.
pub const FW_UPLOAD_STATUS_RECEIVING: u32 = 1;
/// Upload preparing (validating).
pub const FW_UPLOAD_STATUS_PREPARING: u32 = 2;
/// Upload transferring to device.
pub const FW_UPLOAD_STATUS_TRANSFERRING: u32 = 3;
/// Upload programming device.
pub const FW_UPLOAD_STATUS_PROGRAMMING: u32 = 4;

// ---------------------------------------------------------------------------
// Firmware upload errors
// ---------------------------------------------------------------------------

/// No error.
pub const FW_UPLOAD_ERR_NONE: u32 = 0;
/// Hardware error during upload.
pub const FW_UPLOAD_ERR_HW_ERROR: u32 = 1;
/// Timeout during upload.
pub const FW_UPLOAD_ERR_TIMEOUT: u32 = 2;
/// Upload cancelled.
pub const FW_UPLOAD_ERR_CANCELED: u32 = 3;
/// Busy (another upload in progress).
pub const FW_UPLOAD_ERR_BUSY: u32 = 4;
/// Invalid firmware file.
pub const FW_UPLOAD_ERR_INVALID_SIZE: u32 = 5;
/// Read/write error.
pub const FW_UPLOAD_ERR_RW_ERROR: u32 = 6;

// ---------------------------------------------------------------------------
// Firmware paths
// ---------------------------------------------------------------------------

/// Default firmware search path.
pub const FW_PATH_DEFAULT: &str = "/lib/firmware";
/// Updates firmware path.
pub const FW_PATH_UPDATES: &str = "/lib/firmware/updates";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loading_flags_no_overlap() {
        let flags = [
            FW_OPT_OPTIONAL,
            FW_OPT_NOCACHE,
            FW_OPT_UEVENT,
            FW_OPT_NO_UEVENT,
            FW_OPT_PARTIAL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_upload_status_distinct() {
        let states = [
            FW_UPLOAD_STATUS_IDLE,
            FW_UPLOAD_STATUS_RECEIVING,
            FW_UPLOAD_STATUS_PREPARING,
            FW_UPLOAD_STATUS_TRANSFERRING,
            FW_UPLOAD_STATUS_PROGRAMMING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_upload_errors_distinct() {
        let errs = [
            FW_UPLOAD_ERR_NONE,
            FW_UPLOAD_ERR_HW_ERROR,
            FW_UPLOAD_ERR_TIMEOUT,
            FW_UPLOAD_ERR_CANCELED,
            FW_UPLOAD_ERR_BUSY,
            FW_UPLOAD_ERR_INVALID_SIZE,
            FW_UPLOAD_ERR_RW_ERROR,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }

    #[test]
    fn test_paths_nonempty() {
        assert!(!FW_PATH_DEFAULT.is_empty());
        assert!(!FW_PATH_UPDATES.is_empty());
    }
}
