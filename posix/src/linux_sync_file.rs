//! `<linux/sync_file.h>` — Sync file (explicit fencing) constants.
//!
//! Sync files wrap DMA fence objects as file descriptors for
//! userspace synchronization. Used by the GPU, display, and
//! camera subsystems to signal completion of asynchronous
//! operations (implicit/explicit fencing in DRM/KMS).

// ---------------------------------------------------------------------------
// ioctl commands
// ---------------------------------------------------------------------------

/// Get sync file info (fence status).
pub const SYNC_IOC_FILE_INFO: u32 = 0x04;
/// Merge two sync files into one.
pub const SYNC_IOC_MERGE: u32 = 0x03;

// ---------------------------------------------------------------------------
// Fence status values
// ---------------------------------------------------------------------------

/// Fence is active (not yet signaled).
pub const SYNC_FENCE_STATUS_ACTIVE: i32 = 0;
/// Fence is signaled (operation complete).
pub const SYNC_FENCE_STATUS_SIGNALED: i32 = 1;
/// Fence has errored.
pub const SYNC_FENCE_STATUS_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Fence flags
// ---------------------------------------------------------------------------

/// Fence signals with timestamp.
pub const SYNC_FENCE_FLAG_TIMESTAMP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Poll events
// ---------------------------------------------------------------------------

/// Sync file becomes readable when signaled (use with poll/epoll).
pub const SYNC_POLL_EVENT: u32 = 0x0001; // POLLIN

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum name length for sync file debug name.
pub const SYNC_FILE_NAME_LEN: usize = 32;

/// Maximum number of fences in a merged sync file.
pub const SYNC_FILE_MAX_FENCES: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(SYNC_IOC_FILE_INFO, SYNC_IOC_MERGE);
    }

    #[test]
    fn test_fence_status_distinct() {
        let statuses = [
            SYNC_FENCE_STATUS_ACTIVE,
            SYNC_FENCE_STATUS_SIGNALED,
            SYNC_FENCE_STATUS_ERROR,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_name_len() {
        assert!(SYNC_FILE_NAME_LEN > 0);
    }

    #[test]
    fn test_max_fences() {
        assert!(SYNC_FILE_MAX_FENCES > 0);
    }
}
