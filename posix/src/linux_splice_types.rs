//! `<linux/splice.h>` — splice/vmsplice/tee constants.
//!
//! splice() moves data between a file descriptor and a pipe without
//! copying through userspace. vmsplice() maps user pages into a pipe.
//! tee() duplicates pipe data without consuming it. These zero-copy
//! operations reduce CPU usage for high-throughput data forwarding
//! (proxies, file servers, log processors).

// ---------------------------------------------------------------------------
// splice flags (shared by splice, vmsplice, tee)
// ---------------------------------------------------------------------------

/// Move pages (don't copy) — hint only, may fall back to copy.
pub const SPLICE_F_MOVE: u32 = 0x01;
/// Don't block on I/O.
pub const SPLICE_F_NONBLOCK: u32 = 0x02;
/// Hint: more data will follow (for network sockets).
pub const SPLICE_F_MORE: u32 = 0x04;
/// Gift pages to kernel (for vmsplice: user gives up ownership).
pub const SPLICE_F_GIFT: u32 = 0x08;

// ---------------------------------------------------------------------------
// Pipe buffer limits
// ---------------------------------------------------------------------------

/// Default pipe buffer size (16 pages = 64 KiB on 4K pages).
pub const PIPE_DEF_BUFFERS: u32 = 16;
/// Maximum pipe buffer size (1 MiB by default, tunable).
pub const PIPE_MAX_SIZE_DEFAULT: u32 = 1024 * 1024;
/// Minimum pipe buffer size (1 page).
pub const PIPE_MIN_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splice_flags_no_overlap() {
        let flags = [SPLICE_F_MOVE, SPLICE_F_NONBLOCK, SPLICE_F_MORE, SPLICE_F_GIFT];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_pipe_sizes_ordering() {
        assert!(PIPE_MIN_SIZE < PIPE_MAX_SIZE_DEFAULT);
    }

    #[test]
    fn test_pipe_def_buffers_positive() {
        assert!(PIPE_DEF_BUFFERS > 0);
    }
}
