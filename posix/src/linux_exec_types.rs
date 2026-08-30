//! `<unistd.h>` — exec family constants.
//!
//! The `exec*()` functions replace the current process image with
//! a new one.  These constants define the AT_* flags for execveat(),
//! environment handling, and related limits.

// ---------------------------------------------------------------------------
// execveat() flags
// ---------------------------------------------------------------------------

/// Empty path — execute the file referred to by dirfd.
pub const AT_EMPTY_PATH_EXEC: u32 = 0x1000;
/// Follow symlinks in the final path component (default).
pub const AT_SYMLINK_NOFOLLOW_EXEC: u32 = 0x100;

// ---------------------------------------------------------------------------
// exec limits
// ---------------------------------------------------------------------------

/// Maximum total size of argv + envp (bytes, Linux default).
pub const ARG_MAX: u32 = 131072; // 128 KiB
/// Maximum length of a single argument or environment string.
pub const MAX_ARG_STRLEN: u32 = 131072;
/// Maximum number of arguments in argv.
pub const MAX_ARG_STRINGS: u32 = 0x7FFFFFFF;

// ---------------------------------------------------------------------------
// ELF interpreter limits
// ---------------------------------------------------------------------------

/// Maximum length of the ELF interpreter path (#! line).
pub const BINPRM_BUF_SIZE: u32 = 256;
/// Maximum recursion depth for script interpreters.
pub const BINPRM_MAX_RECURSION: u32 = 4;

// ---------------------------------------------------------------------------
// Environment variable limits
// ---------------------------------------------------------------------------

/// Maximum number of environment variables (practical limit).
pub const ENV_MAX_VARS: u32 = 65536;
/// Maximum length of an environment variable name.
pub const ENV_NAME_MAX: u32 = 4096;

// ---------------------------------------------------------------------------
// Close-on-exec flag
// ---------------------------------------------------------------------------

/// Close file descriptor on exec (FD_CLOEXEC).
pub const FD_CLOEXEC: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_max() {
        assert_eq!(ARG_MAX, 131072);
    }

    #[test]
    fn test_max_arg_strlen() {
        assert_eq!(MAX_ARG_STRLEN, 131072);
    }

    #[test]
    fn test_max_arg_strings() {
        assert_eq!(MAX_ARG_STRINGS, 0x7FFFFFFF);
    }

    #[test]
    fn test_binprm_buf_size() {
        assert_eq!(BINPRM_BUF_SIZE, 256);
    }

    #[test]
    fn test_binprm_max_recursion() {
        assert_eq!(BINPRM_MAX_RECURSION, 4);
    }

    #[test]
    fn test_fd_cloexec() {
        assert_eq!(FD_CLOEXEC, 1);
    }

    #[test]
    fn test_env_limits() {
        assert!(ENV_MAX_VARS > 0);
        assert!(ENV_NAME_MAX > 0);
    }

    #[test]
    fn test_at_flags_distinct() {
        assert_ne!(AT_EMPTY_PATH_EXEC, AT_SYMLINK_NOFOLLOW_EXEC);
    }
}
