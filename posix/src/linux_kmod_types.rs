//! `<linux/kmod.h>` — Kernel module auto-loading constants.
//!
//! The kernel's module auto-loading mechanism (`request_module()`)
//! triggers userspace helpers to load modules on demand. These
//! constants define the helper paths, timeout values, and request
//! flags used by the kmod subsystem.

// ---------------------------------------------------------------------------
// Module auto-load request flags
// ---------------------------------------------------------------------------

/// Standard module request (blocking).
pub const KMOD_FLAG_BLOCKING: u32 = 0;
/// Non-blocking module request.
pub const KMOD_FLAG_ASYNC: u32 = 1;
/// Silent request (don't log failures).
pub const KMOD_FLAG_SILENT: u32 = 2;

// ---------------------------------------------------------------------------
// Module path/name limits
// ---------------------------------------------------------------------------

/// Maximum module name length.
pub const MODULE_NAME_LEN: u32 = 56;
/// Maximum module path length (including extension).
pub const MODULE_PATH_LEN: u32 = 256;

// ---------------------------------------------------------------------------
// Usermode helper limits
// ---------------------------------------------------------------------------

/// Maximum number of usermode helper arguments.
pub const UMH_MAX_ARGS: u32 = 32;
/// Maximum number of usermode helper environment variables.
pub const UMH_MAX_ENVP: u32 = 32;

// ---------------------------------------------------------------------------
// Usermode helper wait flags
// ---------------------------------------------------------------------------

/// Don't wait for helper to complete.
pub const UMH_NO_WAIT: u32 = 0;
/// Wait for helper exec (not exit).
pub const UMH_WAIT_EXEC: u32 = 1;
/// Wait for helper to finish.
pub const UMH_WAIT_PROC: u32 = 2;
/// Kill helper on thread exit.
pub const UMH_KILLABLE: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kmod_flags_distinct() {
        let flags = [KMOD_FLAG_BLOCKING, KMOD_FLAG_ASYNC, KMOD_FLAG_SILENT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_name_limits() {
        assert_eq!(MODULE_NAME_LEN, 56);
        assert_eq!(MODULE_PATH_LEN, 256);
        assert!(MODULE_NAME_LEN < MODULE_PATH_LEN);
    }

    #[test]
    fn test_umh_limits() {
        assert_eq!(UMH_MAX_ARGS, 32);
        assert_eq!(UMH_MAX_ENVP, 32);
    }

    #[test]
    fn test_umh_wait_flags_distinct() {
        let flags = [UMH_NO_WAIT, UMH_WAIT_EXEC, UMH_WAIT_PROC, UMH_KILLABLE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_no_wait_is_zero() {
        assert_eq!(UMH_NO_WAIT, 0);
    }
}
