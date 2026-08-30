//! `<sys/sendfile.h>` — sendfile() constants and related types.
//!
//! `sendfile()` copies data between file descriptors in kernel
//! space, avoiding the overhead of copying through userspace.
//! These constants define limits and related settings.

// ---------------------------------------------------------------------------
// sendfile() limits
// ---------------------------------------------------------------------------

/// Maximum bytes per sendfile() call (2 GiB - 1 page on x86_64).
pub const SENDFILE_MAX_BYTES: u64 = 0x7FFFF000;
/// Minimum count for sendfile() (1 byte).
pub const SENDFILE_MIN_BYTES: u64 = 1;

// ---------------------------------------------------------------------------
// sendfile() behaviour flags (via fcntl pipe size, indirect)
// ---------------------------------------------------------------------------

/// Default chunk size for internal sendfile pipe (if used).
pub const SENDFILE_CHUNK_SIZE: u32 = 65536;

// ---------------------------------------------------------------------------
// TCP cork / nopush (used with sendfile for performance)
// ---------------------------------------------------------------------------

/// TCP_CORK option (aggregate small sends).
pub const TCP_CORK: u32 = 3;
/// TCP_NOPUSH (BSD equivalent of TCP_CORK, not on Linux but reserved).
pub const TCP_NOPUSH: u32 = 4;

// ---------------------------------------------------------------------------
// sendfile64 offset type
// ---------------------------------------------------------------------------

/// Maximum offset value for sendfile64 (off64_t max).
pub const SENDFILE64_OFF_MAX: i64 = i64::MAX;
/// Offset indicating "use current file position".
pub const SENDFILE_OFF_CURRENT: i64 = -1;

// ---------------------------------------------------------------------------
// Related socket options for zero-copy
// ---------------------------------------------------------------------------

/// MSG_ZEROCOPY flag for send().
pub const MSG_ZEROCOPY: u32 = 0x4000000;
/// SO_ZEROCOPY socket option.
pub const SO_ZEROCOPY: u32 = 60;
/// Notification type for zerocopy completion.
pub const SO_EE_ORIGIN_ZEROCOPY: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_bytes() {
        assert_eq!(SENDFILE_MAX_BYTES, 0x7FFFF000);
    }

    #[test]
    fn test_min_bytes() {
        assert_eq!(SENDFILE_MIN_BYTES, 1);
    }

    #[test]
    fn test_chunk_size() {
        assert_eq!(SENDFILE_CHUNK_SIZE, 65536);
    }

    #[test]
    fn test_tcp_cork_value() {
        assert_eq!(TCP_CORK, 3);
    }

    #[test]
    fn test_tcp_cork_nopush_distinct() {
        assert_ne!(TCP_CORK, TCP_NOPUSH);
    }

    #[test]
    fn test_off_max() {
        assert_eq!(SENDFILE64_OFF_MAX, i64::MAX);
    }

    #[test]
    fn test_off_current() {
        assert_eq!(SENDFILE_OFF_CURRENT, -1);
    }

    #[test]
    fn test_zerocopy_flag() {
        assert_eq!(MSG_ZEROCOPY, 0x4000000);
    }

    #[test]
    fn test_zerocopy_distinct() {
        assert_ne!(SO_ZEROCOPY as u32, SO_EE_ORIGIN_ZEROCOPY);
    }
}
