//! `<linux/udmabuf.h>` — Userspace DMA-BUF creation constants.
//!
//! udmabuf allows userspace to create DMA-BUF objects backed by
//! anonymous memory (memfd). Useful for sharing buffers between
//! GPU, display, and camera subsystems without kernel driver
//! involvement in allocation.

// ---------------------------------------------------------------------------
// ioctl commands
// ---------------------------------------------------------------------------

/// Create a udmabuf from a single memfd region.
pub const UDMABUF_CREATE: u32 = 0x42;
/// Create a udmabuf from multiple memfd regions (scatter-gather).
pub const UDMABUF_CREATE_LIST: u32 = 0x43;

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// No specific flags.
pub const UDMABUF_FLAGS_DEFAULT: u32 = 0;
/// Cloexec on the returned fd.
pub const UDMABUF_FLAGS_CLOEXEC: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Device path
// ---------------------------------------------------------------------------

/// udmabuf device path.
pub const UDMABUF_DEV_PATH: &str = "/dev/udmabuf";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of scatter-gather entries in CREATE_LIST.
pub const UDMABUF_MAX_LIST_ENTRIES: u32 = 1024;

/// Maximum buffer size (default, can be changed via module param).
pub const UDMABUF_MAX_SIZE_DEFAULT: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(UDMABUF_CREATE, UDMABUF_CREATE_LIST);
    }

    #[test]
    fn test_flags() {
        assert_eq!(UDMABUF_FLAGS_DEFAULT, 0);
        assert!(UDMABUF_FLAGS_CLOEXEC.is_power_of_two());
    }

    #[test]
    fn test_max_list_entries() {
        assert!(UDMABUF_MAX_LIST_ENTRIES > 0);
    }

    #[test]
    fn test_max_size() {
        assert_eq!(UDMABUF_MAX_SIZE_DEFAULT, 64 * 1024 * 1024);
    }
}
