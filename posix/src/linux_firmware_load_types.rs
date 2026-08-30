//! `<linux/firmware.h>` — Firmware loading subsystem constants.
//!
//! Many devices require firmware blobs to be loaded from the filesystem
//! at runtime (WiFi adapters, GPUs, DSPs, microcontrollers). The kernel
//! firmware loading API searches standard paths (/lib/firmware/), handles
//! fallback to userspace helpers (deprecated), supports direct loading
//! from builtin firmware (compiled into the kernel), and provides
//! caching to avoid re-reading the same blob on every resume.

// ---------------------------------------------------------------------------
// Firmware loading flags
// ---------------------------------------------------------------------------

/// Default behavior (blocking, from filesystem).
pub const FW_OPT_DEFAULT: u32 = 0x00;
/// Non-optional firmware (print warning if not found).
pub const FW_OPT_UEVENT: u32 = 0x01;
/// No userspace fallback helper.
pub const FW_OPT_NO_WARN: u32 = 0x02;
/// Don't cache firmware (release after use).
pub const FW_OPT_NOCACHE: u32 = 0x04;
/// Non-blocking load (async, uses callback).
pub const FW_OPT_NOWAIT: u32 = 0x08;
/// Fallback to platform firmware (EFI embedded).
pub const FW_OPT_FALLBACK_PLATFORM: u32 = 0x10;
/// Load full file into contiguous buffer.
pub const FW_OPT_FULL_NAME: u32 = 0x20;

// ---------------------------------------------------------------------------
// Firmware loading states
// ---------------------------------------------------------------------------

/// Firmware is not loaded.
pub const FW_STATE_NONE: u32 = 0;
/// Firmware is being loaded (I/O in progress).
pub const FW_STATE_LOADING: u32 = 1;
/// Firmware loaded successfully.
pub const FW_STATE_DONE: u32 = 2;
/// Firmware load failed (not found or error).
pub const FW_STATE_FAILED: u32 = 3;
/// Firmware load was aborted.
pub const FW_STATE_ABORTED: u32 = 4;

// ---------------------------------------------------------------------------
// Firmware search paths (priority order)
// ---------------------------------------------------------------------------

/// Primary firmware directory.
pub const FW_PATH_PRIMARY: u32 = 0;
/// Updates firmware directory (newer versions).
pub const FW_PATH_UPDATES: u32 = 1;
/// Vendor-specific firmware directory.
pub const FW_PATH_VENDOR: u32 = 2;
/// Builtin firmware (compiled into kernel image).
pub const FW_PATH_BUILTIN: u32 = 3;

// ---------------------------------------------------------------------------
// Firmware upload status (for user-initiated uploads)
// ---------------------------------------------------------------------------

/// Upload idle (no upload in progress).
pub const FW_UPLOAD_IDLE: u32 = 0;
/// Upload receiving data.
pub const FW_UPLOAD_RECEIVING: u32 = 1;
/// Upload programming device.
pub const FW_UPLOAD_PROGRAMMING: u32 = 2;
/// Upload complete.
pub const FW_UPLOAD_DONE: u32 = 3;
/// Upload failed.
pub const FW_UPLOAD_FAILED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            FW_OPT_UEVENT,
            FW_OPT_NO_WARN,
            FW_OPT_NOCACHE,
            FW_OPT_NOWAIT,
            FW_OPT_FALLBACK_PLATFORM,
            FW_OPT_FULL_NAME,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            FW_STATE_NONE,
            FW_STATE_LOADING,
            FW_STATE_DONE,
            FW_STATE_FAILED,
            FW_STATE_ABORTED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_paths_distinct() {
        let paths = [
            FW_PATH_PRIMARY,
            FW_PATH_UPDATES,
            FW_PATH_VENDOR,
            FW_PATH_BUILTIN,
        ];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_upload_states_distinct() {
        let states = [
            FW_UPLOAD_IDLE,
            FW_UPLOAD_RECEIVING,
            FW_UPLOAD_PROGRAMMING,
            FW_UPLOAD_DONE,
            FW_UPLOAD_FAILED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
