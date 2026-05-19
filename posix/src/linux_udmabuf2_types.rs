//! `<linux/udmabuf.h>` — Additional udmabuf constants.
//!
//! Supplementary udmabuf constants covering creation flags
//! and ioctl commands.

// ---------------------------------------------------------------------------
// udmabuf creation flags
// ---------------------------------------------------------------------------

/// No flags.
pub const UDMABUF_FLAGS_NONE: u32 = 0;
/// Cloexec.
pub const UDMABUF_FLAGS_CLOEXEC: u32 = 0o02000000;

// ---------------------------------------------------------------------------
// udmabuf ioctl commands
// ---------------------------------------------------------------------------

/// Create udmabuf.
pub const UDMABUF_CREATE: u32 = 0x40187542;
/// Create udmabuf (list variant).
pub const UDMABUF_CREATE_LIST: u32 = 0x40187543;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct() {
        assert_ne!(UDMABUF_FLAGS_NONE, UDMABUF_FLAGS_CLOEXEC);
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(UDMABUF_CREATE, UDMABUF_CREATE_LIST);
    }
}
