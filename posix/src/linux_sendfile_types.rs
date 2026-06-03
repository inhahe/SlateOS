//! `<linux/sendfile.h>` — Sendfile and splice constants.
//!
//! Constants for zero-copy data transfer operations
//! including sendfile, splice, tee, and vmsplice.

// ---------------------------------------------------------------------------
// Splice flags (SPLICE_F_*)
// ---------------------------------------------------------------------------

/// Move pages instead of copying.
pub const SPLICE_F_MOVE: u32 = 0x01;
/// Non-blocking operation.
pub const SPLICE_F_NONBLOCK: u32 = 0x02;
/// Expect more data.
pub const SPLICE_F_MORE: u32 = 0x04;
/// Gift pages to cache.
pub const SPLICE_F_GIFT: u32 = 0x08;

// ---------------------------------------------------------------------------
// Pipe buffer flags
// ---------------------------------------------------------------------------

/// Buffer can be merged.
pub const PIPE_BUF_FLAG_CAN_MERGE: u32 = 0x10;
/// Buffer is a gift.
pub const PIPE_BUF_FLAG_GIFT: u32 = 0x01;
/// Buffer is a packet.
pub const PIPE_BUF_FLAG_PACKET: u32 = 0x02;
/// Whole buffer.
pub const PIPE_BUF_FLAG_WHOLE: u32 = 0x04;
/// Loss notification.
pub const PIPE_BUF_FLAG_LOSS: u32 = 0x08;

// ---------------------------------------------------------------------------
// Pipe constants
// ---------------------------------------------------------------------------

/// Default pipe buffer size.
pub const PIPE_DEF_BUFFERS: u32 = 16;
/// Minimum pipe size.
pub const PIPE_MIN_DEF_BUFFERS: u32 = 2;
/// Maximum pipe size (1 MiB).
pub const PIPE_MAX_SIZE: u32 = 1 << 20;
/// Pipe buffer size (one page = 4096).
pub const PIPE_BUF: u32 = 4096;

// ---------------------------------------------------------------------------
// Pipe ioctl commands
// ---------------------------------------------------------------------------

/// Get pipe size.
pub const F_GETPIPE_SZ: u32 = 1032;
/// Set pipe size.
pub const F_SETPIPE_SZ: u32 = 1031;

// ---------------------------------------------------------------------------
// Copy file range flags
// ---------------------------------------------------------------------------

/// No flags (placeholder).
pub const COPY_FR_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Sendfile max count
// ---------------------------------------------------------------------------

/// Max bytes in single sendfile call.
pub const SENDFILE_MAX_COUNT: u64 = 0x7FFFF000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splice_flags_power_of_two() {
        let flags = [
            SPLICE_F_MOVE,
            SPLICE_F_NONBLOCK,
            SPLICE_F_MORE,
            SPLICE_F_GIFT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
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
    fn test_pipe_buf_flags_power_of_two() {
        let flags = [
            PIPE_BUF_FLAG_GIFT,
            PIPE_BUF_FLAG_PACKET,
            PIPE_BUF_FLAG_WHOLE,
            PIPE_BUF_FLAG_LOSS,
            PIPE_BUF_FLAG_CAN_MERGE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_pipe_sizes() {
        assert!(PIPE_MIN_DEF_BUFFERS <= PIPE_DEF_BUFFERS);
        assert!(PIPE_BUF > 0);
        assert!(PIPE_MAX_SIZE.is_power_of_two());
    }

    #[test]
    fn test_pipe_ioctls() {
        assert_ne!(F_GETPIPE_SZ, F_SETPIPE_SZ);
    }

    #[test]
    fn test_sendfile_max() {
        assert_eq!(SENDFILE_MAX_COUNT, 0x7FFFF000);
    }
}
