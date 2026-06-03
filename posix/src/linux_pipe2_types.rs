//! `<linux/pipe_fs_i.h>` — Pipe flag and limit constants.
//!
//! Pipes are unidirectional byte streams for IPC. These constants
//! define flags for `pipe2()`, pipe buffer sizing, and splice
//! operation flags.

// ---------------------------------------------------------------------------
// pipe2 flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on both pipe ends.
pub const O_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking on both pipe ends.
pub const O_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// Pipe buffer limits
// ---------------------------------------------------------------------------

/// Default pipe buffer size (64 KiB = 16 pages of 4 KiB).
pub const PIPE_DEF_BUFFERS: u32 = 16;
/// Minimum pipe buffer size (1 page).
pub const PIPE_MIN_DEF_BUFFERS: u32 = 2;
/// Maximum pipe buffer size (unprivileged, via F_SETPIPE_SZ).
pub const PIPE_MAX_SIZE_DEFAULT: u32 = 1048576;

// ---------------------------------------------------------------------------
// F_SETPIPE_SZ / F_GETPIPE_SZ (fcntl commands for pipe sizing)
// ---------------------------------------------------------------------------

/// Set the pipe buffer size.
pub const F_SETPIPE_SZ: u32 = 1031;
/// Get the pipe buffer size.
pub const F_GETPIPE_SZ: u32 = 1032;

// ---------------------------------------------------------------------------
// Splice flags (for splice/tee/vmsplice)
// ---------------------------------------------------------------------------

/// Move pages instead of copying.
pub const SPLICE_F_MOVE: u32 = 0x01;
/// Don't block on pipe I/O.
pub const SPLICE_F_NONBLOCK: u32 = 0x02;
/// Hint: more data coming.
pub const SPLICE_F_MORE: u32 = 0x04;
/// Gift pages to the pipe (vmsplice).
pub const SPLICE_F_GIFT: u32 = 0x08;

// ---------------------------------------------------------------------------
// Pipe ioctl
// ---------------------------------------------------------------------------

/// Get number of bytes available in pipe.
pub const FIONREAD_PIPE: u32 = 0x541B;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe2_flags_distinct() {
        assert_ne!(O_CLOEXEC, O_NONBLOCK);
    }

    #[test]
    fn test_cloexec() {
        assert_eq!(O_CLOEXEC, 0o2000000);
    }

    #[test]
    fn test_pipe_size_fcntl_distinct() {
        assert_ne!(F_SETPIPE_SZ, F_GETPIPE_SZ);
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
    fn test_splice_flags_power_of_two() {
        assert!(SPLICE_F_MOVE.is_power_of_two());
        assert!(SPLICE_F_NONBLOCK.is_power_of_two());
        assert!(SPLICE_F_MORE.is_power_of_two());
        assert!(SPLICE_F_GIFT.is_power_of_two());
    }

    #[test]
    fn test_pipe_def_buffers() {
        assert_eq!(PIPE_DEF_BUFFERS, 16);
    }

    #[test]
    fn test_max_size_default() {
        assert_eq!(PIPE_MAX_SIZE_DEFAULT, 1048576);
    }
}
