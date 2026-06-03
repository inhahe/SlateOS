//! `<linux/splice.h>` — Splice/tee/vmsplice constants.
//!
//! splice(2) moves data between a file descriptor and a pipe
//! without copying through userspace. tee(2) duplicates data
//! between pipes. vmsplice(2) maps userspace memory into a pipe.
//! All use the kernel pipe buffer for zero-copy data movement.

// ---------------------------------------------------------------------------
// splice(2) flags
// ---------------------------------------------------------------------------

/// Move pages instead of copying (hint, not guaranteed).
pub const SPLICE_F_MOVE: u32 = 0x01;
/// Don't block on pipe I/O.
pub const SPLICE_F_NONBLOCK: u32 = 0x02;
/// Hint that more data will follow (set MSG_MORE on sockets).
pub const SPLICE_F_MORE: u32 = 0x04;
/// Gift pages to the pipe (for vmsplice, irrevocable transfer).
pub const SPLICE_F_GIFT: u32 = 0x08;

/// All valid splice flags.
pub const SPLICE_F_ALL: u32 = SPLICE_F_MOVE | SPLICE_F_NONBLOCK | SPLICE_F_MORE | SPLICE_F_GIFT;

// ---------------------------------------------------------------------------
// Pipe buffer constants
// ---------------------------------------------------------------------------

/// Default pipe buffer size (pages).
pub const PIPE_DEF_BUFFERS: u32 = 16;

/// Maximum pipe buffer size configurable via F_SETPIPE_SZ (1 MiB default limit).
pub const PIPE_MAX_SIZE_DEFAULT: u32 = 1024 * 1024;

/// Minimum pipe buffer size (one page).
pub const PIPE_MIN_SIZE: u32 = 1;

// ---------------------------------------------------------------------------
// fcntl pipe size commands
// ---------------------------------------------------------------------------

/// Set pipe buffer size (fcntl F_SETPIPE_SZ).
pub const F_SETPIPE_SZ: u32 = 1031;
/// Get pipe buffer size (fcntl F_GETPIPE_SZ).
pub const F_GETPIPE_SZ: u32 = 1032;

// ---------------------------------------------------------------------------
// Sysctl
// ---------------------------------------------------------------------------

/// Maximum unprivileged pipe buffer size sysctl.
pub const SYSCTL_PIPE_MAX_SIZE: &str = "fs.pipe-max-size";
/// Default number of pipe buffers sysctl.
pub const SYSCTL_PIPE_USER_PAGES_HARD: &str = "fs.pipe-user-pages-hard";
/// Soft limit sysctl.
pub const SYSCTL_PIPE_USER_PAGES_SOFT: &str = "fs.pipe-user-pages-soft";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splice_flags_powers_of_two() {
        let flags = [
            SPLICE_F_MOVE,
            SPLICE_F_NONBLOCK,
            SPLICE_F_MORE,
            SPLICE_F_GIFT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_splice_flags_no_overlap() {
        let flags = [
            SPLICE_F_MOVE,
            SPLICE_F_NONBLOCK,
            SPLICE_F_MORE,
            SPLICE_F_GIFT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_splice_f_all() {
        assert_eq!(SPLICE_F_ALL, 0x0F);
    }

    #[test]
    fn test_pipe_sizes() {
        assert!(PIPE_DEF_BUFFERS > 0);
        assert!(PIPE_MAX_SIZE_DEFAULT > 0);
        assert!(PIPE_MIN_SIZE > 0);
        assert!(PIPE_MIN_SIZE <= PIPE_DEF_BUFFERS);
    }

    #[test]
    fn test_fcntl_pipe_cmds_distinct() {
        assert_ne!(F_SETPIPE_SZ, F_GETPIPE_SZ);
    }

    #[test]
    fn test_sysctl_paths_distinct() {
        let paths = [
            SYSCTL_PIPE_MAX_SIZE,
            SYSCTL_PIPE_USER_PAGES_HARD,
            SYSCTL_PIPE_USER_PAGES_SOFT,
        ];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }
}
