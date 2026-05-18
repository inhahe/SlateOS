//! `<fcntl.h>` — splice/vmsplice/tee extended flag constants.
//!
//! `splice()`, `vmsplice()`, and `tee()` move data between file
//! descriptors and pipes without copying through userspace.
//! These constants define the flags and limits for these
//! zero-copy operations.

// ---------------------------------------------------------------------------
// splice() flags
// ---------------------------------------------------------------------------

/// Move pages instead of copying (hint to kernel).
pub const SPLICE_F_MOVE: u32 = 0x01;
/// Do not block on I/O.
pub const SPLICE_F_NONBLOCK: u32 = 0x02;
/// Expect more data will follow (hint to kernel).
pub const SPLICE_F_MORE: u32 = 0x04;
/// Try to splice to a pipe using the page cache gift mechanism.
pub const SPLICE_F_GIFT: u32 = 0x08;

// ---------------------------------------------------------------------------
// Pipe capacity limits (related to splice)
// ---------------------------------------------------------------------------

/// Default pipe buffer size (bytes, one page).
pub const PIPE_BUF_SIZE: u32 = 4096;
/// Default pipe capacity (number of buffers × page size = 64 KiB).
pub const PIPE_DEFAULT_CAPACITY: u32 = 65536;
/// Maximum pipe capacity (bytes, /proc/sys/fs/pipe-max-size default).
pub const PIPE_MAX_SIZE_DEFAULT: u32 = 1048576; // 1 MiB
/// Minimum pipe capacity (bytes).
pub const PIPE_MIN_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// vmsplice() iovec limits
// ---------------------------------------------------------------------------

/// Maximum number of iovec entries for vmsplice.
pub const VMSPLICE_IOV_MAX: u32 = 1024;

// ---------------------------------------------------------------------------
// tee() limits
// ---------------------------------------------------------------------------

/// Maximum bytes for a single tee() call (limited by pipe capacity).
pub const TEE_MAX_BYTES: u32 = 65536;

// ---------------------------------------------------------------------------
// fcntl pipe size operations
// ---------------------------------------------------------------------------

/// Get pipe capacity (fcntl F_GETPIPE_SZ).
pub const F_GETPIPE_SZ: u32 = 1032;
/// Set pipe capacity (fcntl F_SETPIPE_SZ).
pub const F_SETPIPE_SZ: u32 = 1031;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [SPLICE_F_MOVE, SPLICE_F_NONBLOCK, SPLICE_F_MORE, SPLICE_F_GIFT];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [SPLICE_F_MOVE, SPLICE_F_NONBLOCK, SPLICE_F_MORE, SPLICE_F_GIFT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_move_is_one() {
        assert_eq!(SPLICE_F_MOVE, 1);
    }

    #[test]
    fn test_pipe_buf_size() {
        assert_eq!(PIPE_BUF_SIZE, 4096);
    }

    #[test]
    fn test_pipe_default_capacity() {
        assert_eq!(PIPE_DEFAULT_CAPACITY, 65536);
    }

    #[test]
    fn test_pipe_max_gte_default() {
        assert!(PIPE_MAX_SIZE_DEFAULT >= PIPE_DEFAULT_CAPACITY);
    }

    #[test]
    fn test_fcntl_pipe_ops_distinct() {
        assert_ne!(F_GETPIPE_SZ, F_SETPIPE_SZ);
    }

    #[test]
    fn test_vmsplice_iov_max() {
        assert_eq!(VMSPLICE_IOV_MAX, 1024);
    }
}
