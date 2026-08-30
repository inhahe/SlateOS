//! `<linux/sync_file.h>` — Sync file (explicit fencing) constants.
//!
//! Sync files represent GPU/DMA fence synchronization points as file
//! descriptors. They allow userspace to wait for hardware operations
//! to complete and to chain operations across devices. A sync file
//! becomes "signaled" when the associated GPU/DMA operation finishes.
//! IOCTLs query fence status and merge multiple fences. Used by
//! Android's HWComposer, Vulkan (VK_KHR_external_fence_fd), and
//! DRM atomic modesetting.

// ---------------------------------------------------------------------------
// Sync file IOCTLs
// ---------------------------------------------------------------------------

/// Get sync file info (list of fences).
pub const SYNC_IOC_FILE_INFO: u32 = 0xC020_3E04;
/// Merge two sync files into one.
pub const SYNC_IOC_MERGE: u32 = 0xC020_3E01;

// ---------------------------------------------------------------------------
// Fence info flags
// ---------------------------------------------------------------------------

/// Fence has been signaled (operation complete).
pub const SYNC_FENCE_FLAG_SIGNALED: u32 = 1 << 0;
/// Fence timed out waiting for signal.
pub const SYNC_FENCE_FLAG_TIMESTAMP_VALID: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Fence status values
// ---------------------------------------------------------------------------

/// Fence is active (not yet signaled).
pub const SYNC_FENCE_STATUS_ACTIVE: i32 = 0;
/// Fence has been signaled successfully.
pub const SYNC_FENCE_STATUS_SIGNALED: i32 = 1;
/// Fence encountered an error.
pub const SYNC_FENCE_STATUS_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Sync file creation flags
// ---------------------------------------------------------------------------

/// Close-on-exec for the sync file fd.
pub const SYNC_FILE_CREATE_CLOEXEC: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Timeline semaphore IOCTLs (from DRM syncobj)
// ---------------------------------------------------------------------------

/// Query sync point (timeline value).
pub const SYNC_TIMELINE_QUERY: u32 = 0x00;
/// Signal a timeline sync point.
pub const SYNC_TIMELINE_SIGNAL: u32 = 0x01;
/// Wait for timeline to reach a value.
pub const SYNC_TIMELINE_WAIT: u32 = 0x02;
/// Transfer timeline point to another syncobj.
pub const SYNC_TIMELINE_TRANSFER: u32 = 0x03;

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
    fn test_fence_flags_no_overlap() {
        assert_eq!(
            SYNC_FENCE_FLAG_SIGNALED & SYNC_FENCE_FLAG_TIMESTAMP_VALID,
            0
        );
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
    fn test_timeline_ops_distinct() {
        let ops = [
            SYNC_TIMELINE_QUERY,
            SYNC_TIMELINE_SIGNAL,
            SYNC_TIMELINE_WAIT,
            SYNC_TIMELINE_TRANSFER,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}
