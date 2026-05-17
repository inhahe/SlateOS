//! `<linux/udmabuf.h>` — Userspace DMA-BUF creation constants.
//!
//! udmabuf (/dev/udmabuf) allows unprivileged userspace to create
//! DMA-BUF objects from memfd pages. The resulting dma-buf can be
//! shared with DRM/KMS for zero-copy display, or with other DMA-BUF
//! importers. This enables QEMU, Wayland compositors, and virtual
//! display drivers to efficiently share framebuffers without kernel
//! driver modifications.

// ---------------------------------------------------------------------------
// udmabuf IOCTLs
// ---------------------------------------------------------------------------

/// Create a udmabuf from a single memfd range.
pub const UDMABUF_CREATE: u32 = 0x4010_7542;
/// Create a udmabuf from multiple memfd ranges (scatter-gather).
pub const UDMABUF_CREATE_LIST: u32 = 0x4018_7543;

// ---------------------------------------------------------------------------
// udmabuf flags
// ---------------------------------------------------------------------------

/// No special flags.
pub const UDMABUF_FLAGS_NONE: u32 = 0;
/// Close-on-exec for the returned dma-buf fd.
pub const UDMABUF_FLAGS_CLOEXEC: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// udmabuf limits
// ---------------------------------------------------------------------------

/// Maximum number of list entries in UDMABUF_CREATE_LIST.
pub const UDMABUF_MAX_LIST_ENTRIES: u32 = 1024;

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
    fn test_flags_distinct() {
        assert_ne!(UDMABUF_FLAGS_NONE, UDMABUF_FLAGS_CLOEXEC);
    }

    #[test]
    fn test_max_list_entries() {
        assert_eq!(UDMABUF_MAX_LIST_ENTRIES, 1024);
        assert!(UDMABUF_MAX_LIST_ENTRIES.is_power_of_two());
    }
}
