//! `<linux/sync_file.h>` — Software sync timeline constants.
//!
//! Software sync provides a debug/test timeline for the
//! sync framework. These constants define IOCTL commands
//! and timeline parameters.

// ---------------------------------------------------------------------------
// IOCTL base
// ---------------------------------------------------------------------------

/// SW sync IOCTL magic.
pub const SW_SYNC_IOC_MAGIC: u8 = b'W';

// ---------------------------------------------------------------------------
// IOCTL commands
// ---------------------------------------------------------------------------

/// Create fence.
pub const SW_SYNC_IOC_CREATE_FENCE: u32 = 0xC0085700;
/// Increment timeline.
pub const SW_SYNC_IOC_INC: u32 = 0x40045701;

// ---------------------------------------------------------------------------
// Fence states
// ---------------------------------------------------------------------------

/// Fence is active (not yet signaled).
pub const SW_SYNC_FENCE_ACTIVE: u32 = 0;
/// Fence is signaled.
pub const SW_SYNC_FENCE_SIGNALED: u32 = 1;
/// Fence has error.
pub const SW_SYNC_FENCE_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Timeline parameters
// ---------------------------------------------------------------------------

/// Default timeline start value.
pub const SW_SYNC_TIMELINE_START: u32 = 0;
/// Maximum timeline value.
pub const SW_SYNC_TIMELINE_MAX: u32 = 0x7FFFFFFF;

// ---------------------------------------------------------------------------
// Sync file info flags
// ---------------------------------------------------------------------------

/// Sync file fence is active.
pub const SYNC_FILE_FENCE_FLAG_ACTIVE: u32 = 0;
/// Sync file fence signaled.
pub const SYNC_FILE_FENCE_FLAG_SIGNALED: u32 = 1;
/// Timestamp available.
pub const SYNC_FILE_FENCE_FLAG_TIMESTAMP_VALID: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_magic() {
        assert_eq!(SW_SYNC_IOC_MAGIC, b'W');
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(SW_SYNC_IOC_CREATE_FENCE, SW_SYNC_IOC_INC);
    }

    #[test]
    fn test_fence_states_sequential() {
        assert_eq!(SW_SYNC_FENCE_ACTIVE, 0);
        assert_eq!(SW_SYNC_FENCE_SIGNALED, 1);
        assert_eq!(SW_SYNC_FENCE_ERROR, 2);
    }

    #[test]
    fn test_timeline_range() {
        assert_eq!(SW_SYNC_TIMELINE_START, 0);
        assert_eq!(SW_SYNC_TIMELINE_MAX, 0x7FFFFFFF);
        assert!(SW_SYNC_TIMELINE_START < SW_SYNC_TIMELINE_MAX);
    }

    #[test]
    fn test_sync_file_flags() {
        assert_eq!(SYNC_FILE_FENCE_FLAG_ACTIVE, 0);
        assert_eq!(SYNC_FILE_FENCE_FLAG_SIGNALED, 1);
        assert_eq!(SYNC_FILE_FENCE_FLAG_TIMESTAMP_VALID, 2);
    }
}
