//! `<linux/firmware.h>` — Firmware loading constants.
//!
//! The firmware loading subsystem provides a kernel API for loading
//! binary firmware blobs from the filesystem (typically /lib/firmware).
//! Drivers request firmware by name; the kernel locates the file,
//! reads it into a buffer, and hands it to the driver.

// ---------------------------------------------------------------------------
// Firmware action values (for uevent)
// ---------------------------------------------------------------------------

/// Loading firmware.
pub const FW_ACTION_LOADING: u32 = 0;
/// Firmware loaded.
pub const FW_ACTION_LOADED: u32 = 1;
/// Firmware load aborted.
pub const FW_ACTION_ABORTED: u32 = 2;

// ---------------------------------------------------------------------------
// Firmware request flags
// ---------------------------------------------------------------------------

/// Optional firmware (don't warn if missing).
pub const FW_OPT_OPTIONAL: u32 = 1 << 0;
/// No cache (don't keep firmware after loading).
pub const FW_OPT_NOCACHE: u32 = 1 << 1;
/// Userspace helper fallback.
pub const FW_OPT_USERHELPER: u32 = 1 << 2;
/// No warning on failure.
pub const FW_OPT_NO_WARN: u32 = 1 << 3;
/// Firmware is platform-specific.
pub const FW_OPT_PLATFORM: u32 = 1 << 4;
/// Partial content allowed.
pub const FW_OPT_PARTIAL: u32 = 1 << 5;
/// Firmware loading is nowait (async).
pub const FW_OPT_NOWAIT: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Firmware upload status
// ---------------------------------------------------------------------------

/// Upload idle.
pub const FW_UPLOAD_IDLE: u32 = 0;
/// Upload receiving.
pub const FW_UPLOAD_RECEIVING: u32 = 1;
/// Upload preparing.
pub const FW_UPLOAD_PREPARING: u32 = 2;
/// Upload transferring.
pub const FW_UPLOAD_TRANSFERRING: u32 = 3;
/// Upload programming.
pub const FW_UPLOAD_PROGRAMMING: u32 = 4;

// ---------------------------------------------------------------------------
// Firmware upload error codes
// ---------------------------------------------------------------------------

/// No error.
pub const FW_UPLOAD_ERR_NONE: u32 = 0;
/// HW error during upload.
pub const FW_UPLOAD_ERR_HW_ERROR: u32 = 1;
/// Upload timeout.
pub const FW_UPLOAD_ERR_TIMEOUT: u32 = 2;
/// Upload canceled.
pub const FW_UPLOAD_ERR_CANCELED: u32 = 3;
/// Upload busy.
pub const FW_UPLOAD_ERR_BUSY: u32 = 4;
/// Invalid image size.
pub const FW_UPLOAD_ERR_INVALID_SIZE: u32 = 5;
/// Read-write error.
pub const FW_UPLOAD_ERR_RW_ERROR: u32 = 6;
/// Wearout.
pub const FW_UPLOAD_ERR_WEAROUT: u32 = 7;

// ---------------------------------------------------------------------------
// Firmware search paths
// ---------------------------------------------------------------------------

/// Default firmware path.
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
    fn test_actions_distinct() {
        let actions = [FW_ACTION_LOADING, FW_ACTION_LOADED, FW_ACTION_ABORTED];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_opt_flags_powers_of_two() {
        let flags = [
            FW_OPT_OPTIONAL,
            FW_OPT_NOCACHE,
            FW_OPT_USERHELPER,
            FW_OPT_NO_WARN,
            FW_OPT_PLATFORM,
            FW_OPT_PARTIAL,
            FW_OPT_NOWAIT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_opt_flags_no_overlap() {
        let flags = [
            FW_OPT_OPTIONAL,
            FW_OPT_NOCACHE,
            FW_OPT_USERHELPER,
            FW_OPT_NO_WARN,
            FW_OPT_PLATFORM,
            FW_OPT_PARTIAL,
            FW_OPT_NOWAIT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_upload_status_distinct() {
        let statuses = [
            FW_UPLOAD_IDLE,
            FW_UPLOAD_RECEIVING,
            FW_UPLOAD_PREPARING,
            FW_UPLOAD_TRANSFERRING,
            FW_UPLOAD_PROGRAMMING,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_upload_errors_distinct() {
        let errors = [
            FW_UPLOAD_ERR_NONE,
            FW_UPLOAD_ERR_HW_ERROR,
            FW_UPLOAD_ERR_TIMEOUT,
            FW_UPLOAD_ERR_CANCELED,
            FW_UPLOAD_ERR_BUSY,
            FW_UPLOAD_ERR_INVALID_SIZE,
            FW_UPLOAD_ERR_RW_ERROR,
            FW_UPLOAD_ERR_WEAROUT,
        ];
        for i in 0..errors.len() {
            for j in (i + 1)..errors.len() {
                assert_ne!(errors[i], errors[j]);
            }
        }
    }

    #[test]
    fn test_paths() {
        assert_ne!(FW_PATH_DEFAULT, FW_PATH_UPDATES);
        assert!(FW_PATH_UPDATES.starts_with(FW_PATH_DEFAULT));
    }
}
