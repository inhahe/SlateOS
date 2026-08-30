//! `<linux/sendfile.h>` — sendfile(2) constants.
//!
//! sendfile(2) copies data between file descriptors in the kernel,
//! avoiding the overhead of transferring data to and from user
//! space. Commonly used for serving static files over sockets
//! in web servers.

// ---------------------------------------------------------------------------
// sendfile limits
// ---------------------------------------------------------------------------

/// Maximum bytes transferable in a single sendfile(2) call.
/// Limited by ssize_t max (0x7FFFF000 on 32-bit, larger on 64-bit).
pub const SENDFILE_MAX_BYTES_32: u64 = 0x7FFFF000;

/// Practical 64-bit sendfile limit (2 GiB - 1 page).
pub const SENDFILE_MAX_BYTES_64: u64 = 0x7FFFF000;

// ---------------------------------------------------------------------------
// Related splice flags (sendfile can use splice internally)
// ---------------------------------------------------------------------------

/// Non-blocking transfer (when used with splice backend).
pub const SF_NONBLOCK: u32 = 0x02;

// ---------------------------------------------------------------------------
// Socket options for zero-copy sendfile
// ---------------------------------------------------------------------------

/// TCP_CORK — accumulate small writes (useful with sendfile).
pub const TCP_CORK: u32 = 3;
/// TCP_NODELAY — disable Nagle's algorithm.
pub const TCP_NODELAY: u32 = 1;

// ---------------------------------------------------------------------------
// Common usage patterns
// ---------------------------------------------------------------------------

/// Suggested sendfile chunk size for large transfers (128 KiB).
pub const SF_CHUNK_SIZE: usize = 128 * 1024;

/// Number of bytes before the kernel yields to other tasks.
pub const SF_YIELD_THRESHOLD: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Related file operations
// ---------------------------------------------------------------------------

/// Offset sentinel: use current file position (pass NULL to sendfile).
pub const SF_USE_CURRENT_POS: i64 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_bytes() {
        assert_eq!(SENDFILE_MAX_BYTES_32, 0x7FFFF000);
        assert_eq!(SENDFILE_MAX_BYTES_64, 0x7FFFF000);
        assert!(SENDFILE_MAX_BYTES_32 > 0);
    }

    #[test]
    fn test_tcp_opts_distinct() {
        assert_ne!(TCP_CORK, TCP_NODELAY);
    }

    #[test]
    fn test_chunk_size() {
        assert_eq!(SF_CHUNK_SIZE, 128 * 1024);
        assert!(SF_CHUNK_SIZE > 0);
    }

    #[test]
    fn test_yield_threshold() {
        assert!(SF_YIELD_THRESHOLD > SF_CHUNK_SIZE);
    }

    #[test]
    fn test_current_pos_sentinel() {
        assert!(SF_USE_CURRENT_POS < 0);
    }
}
