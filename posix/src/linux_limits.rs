//! `<linux/limits.h>` — Linux kernel limits.
//!
//! These are limits defined by the Linux kernel that complement the
//! POSIX `<limits.h>` values.  They reflect kernel-imposed maximums
//! on names, paths, arguments, and various system resources.

// ---------------------------------------------------------------------------
// Name / path limits
// ---------------------------------------------------------------------------

/// Maximum bytes in a filename (not including null).
pub const NAME_MAX: usize = 255;

/// Maximum bytes in a pathname (including null).
pub const PATH_MAX: usize = 4096;

/// Maximum number of supplementary groups per process.
pub const NGROUPS_MAX: usize = 65536;

/// Maximum number of bytes in a hostname.
pub const HOST_NAME_MAX: usize = 64;

/// Maximum number of bytes in a login name.
pub const LOGIN_NAME_MAX: usize = 256;

/// Maximum number of bytes in a TTY name.
pub const TTY_NAME_MAX: usize = 32;

// ---------------------------------------------------------------------------
// Argument / environment limits
// ---------------------------------------------------------------------------

/// Maximum bytes for argv + envp.
pub const ARG_MAX: usize = 2097152; // 2 MiB

/// Maximum number of links to a single file.
pub const LINK_MAX: usize = 127;

/// Maximum number of bytes in a pipe buffer.
pub const PIPE_BUF: usize = 4096;

/// Maximum number of symbolic links followed during path resolution.
pub const SYMLOOP_MAX: usize = 40;

// ---------------------------------------------------------------------------
// Extended attribute limits
// ---------------------------------------------------------------------------

/// Maximum size of an extended attribute name.
pub const XATTR_NAME_MAX: usize = 255;

/// Maximum size of an extended attribute value.
pub const XATTR_SIZE_MAX: usize = 65536;

/// Maximum total size of extended attribute list.
pub const XATTR_LIST_MAX: usize = 65536;

// ---------------------------------------------------------------------------
// Signal / timer limits
// ---------------------------------------------------------------------------

/// Maximum number of real-time signals.
pub const RTSIG_MAX: usize = 32;

/// Maximum number of queued signals per process.
pub const SIGQUEUE_MAX: usize = 32;

/// Maximum number of timer IDs per process.
pub const TIMER_MAX: usize = 32;

// ---------------------------------------------------------------------------
// I/O limits
// ---------------------------------------------------------------------------

/// Maximum number of iovec structures for readv/writev.
pub const IOV_MAX: usize = 1024;

/// Maximum number of AIO requests.
pub const AIO_MAX: usize = 32768;

/// Maximum priority for AIO operations.
pub const AIO_PRIO_DELTA_MAX: usize = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_max() {
        assert_eq!(NAME_MAX, 255);
    }

    #[test]
    fn test_path_max() {
        assert_eq!(PATH_MAX, 4096);
    }

    #[test]
    fn test_arg_max() {
        assert_eq!(ARG_MAX, 2097152);
    }

    #[test]
    fn test_pipe_buf() {
        assert_eq!(PIPE_BUF, 4096);
    }

    #[test]
    fn test_ngroups_max() {
        assert_eq!(NGROUPS_MAX, 65536);
    }

    #[test]
    fn test_xattr_limits() {
        assert_eq!(XATTR_NAME_MAX, 255);
        assert_eq!(XATTR_SIZE_MAX, 65536);
        assert_eq!(XATTR_LIST_MAX, 65536);
    }

    #[test]
    fn test_iov_max() {
        assert_eq!(IOV_MAX, 1024);
    }

    #[test]
    fn test_host_name_max() {
        assert_eq!(HOST_NAME_MAX, 64);
    }

    #[test]
    fn test_all_positive() {
        let limits = [
            NAME_MAX, PATH_MAX, NGROUPS_MAX, HOST_NAME_MAX,
            LOGIN_NAME_MAX, TTY_NAME_MAX, ARG_MAX, LINK_MAX,
            PIPE_BUF, SYMLOOP_MAX, XATTR_NAME_MAX, XATTR_SIZE_MAX,
            XATTR_LIST_MAX, RTSIG_MAX, SIGQUEUE_MAX, TIMER_MAX,
            IOV_MAX, AIO_MAX, AIO_PRIO_DELTA_MAX,
        ];
        for &l in &limits {
            assert!(l > 0, "limit should be positive");
        }
    }

    #[test]
    fn test_path_max_gt_name_max() {
        assert!(PATH_MAX > NAME_MAX);
    }

    // -----------------------------------------------------------------------
    // Cross-module checks
    // -----------------------------------------------------------------------

    #[test]
    fn test_name_max_matches_sys_param() {
        // sys_param::MAXPATHLEN should match our PATH_MAX.
        assert_eq!(PATH_MAX, crate::sys_param::MAXPATHLEN as usize);
    }
}
