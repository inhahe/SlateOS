//! `<linux/fs.h>` — File descriptor limits and special fd constants.
//!
//! These constants define file descriptor limits, the standard
//! file descriptors, and related system-level defaults.

// ---------------------------------------------------------------------------
// Standard file descriptors
// ---------------------------------------------------------------------------

/// Standard input.
pub const STDIN_FILENO: u32 = 0;
/// Standard output.
pub const STDOUT_FILENO: u32 = 1;
/// Standard error.
pub const STDERR_FILENO: u32 = 2;

// ---------------------------------------------------------------------------
// File descriptor limits
// ---------------------------------------------------------------------------

/// Default soft limit for open file descriptors (RLIMIT_NOFILE).
pub const FD_SETSIZE: u32 = 1024;
/// Default hard limit for open file descriptors (typical).
pub const NR_OPEN_DEFAULT: u32 = 1048576;
/// Absolute maximum open files (kernel compile-time limit).
pub const NR_OPEN_MAX: u32 = 1073741816;

// ---------------------------------------------------------------------------
// File descriptor flags
// ---------------------------------------------------------------------------

/// Close-on-exec flag value.
pub const FD_CLOEXEC_FLAG: u32 = 1;

// ---------------------------------------------------------------------------
// dup3 flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the new fd (dup3 flag).
pub const O_CLOEXEC_DUP3: u32 = 0o2000000;

// ---------------------------------------------------------------------------
// close_range flags
// ---------------------------------------------------------------------------

/// Set close-on-exec instead of closing (close_range flag).
pub const CLOSE_RANGE_CLOEXEC: u32 = 1 << 2;
/// Unshare the fd table before closing (close_range flag).
pub const CLOSE_RANGE_UNSHARE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Special fd values
// ---------------------------------------------------------------------------

/// Invalid/uninitialized file descriptor.
pub const FD_INVALID: i32 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_fds_distinct() {
        let fds = [STDIN_FILENO, STDOUT_FILENO, STDERR_FILENO];
        for i in 0..fds.len() {
            for j in (i + 1)..fds.len() {
                assert_ne!(fds[i], fds[j]);
            }
        }
    }

    #[test]
    fn test_stdin() {
        assert_eq!(STDIN_FILENO, 0);
    }

    #[test]
    fn test_stdout() {
        assert_eq!(STDOUT_FILENO, 1);
    }

    #[test]
    fn test_stderr() {
        assert_eq!(STDERR_FILENO, 2);
    }

    #[test]
    fn test_fd_setsize() {
        assert_eq!(FD_SETSIZE, 1024);
    }

    #[test]
    fn test_nr_open_default() {
        assert_eq!(NR_OPEN_DEFAULT, 1048576);
    }

    #[test]
    fn test_close_range_flags_no_overlap() {
        assert_eq!(CLOSE_RANGE_CLOEXEC & CLOSE_RANGE_UNSHARE, 0);
    }

    #[test]
    fn test_fd_invalid() {
        assert_eq!(FD_INVALID, -1);
    }

    #[test]
    fn test_fd_cloexec_flag() {
        assert_eq!(FD_CLOEXEC_FLAG, 1);
    }
}
