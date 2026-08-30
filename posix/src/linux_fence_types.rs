//! `<linux/sync_file.h>` — Fence/sync file constants.
//!
//! Fence and sync file constants covering fence status,
//! sync_file info flags, and timeline semaphore types.

// ---------------------------------------------------------------------------
// Fence status values
// ---------------------------------------------------------------------------

/// Fence not yet signaled.
pub const FENCE_STATUS_ACTIVE: i32 = 0;
/// Fence signaled successfully.
pub const FENCE_STATUS_SIGNALED: i32 = 1;
/// Fence errored.
pub const FENCE_STATUS_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Sync file fence info flags
// ---------------------------------------------------------------------------

/// Fence is signaled.
pub const SYNC_FENCE_FLAG_SIGNALED: u32 = 1 << 0;
/// Fence has a timestamp.
pub const SYNC_FENCE_FLAG_TIMESTAMP_VALID: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// DRM syncobj flags
// ---------------------------------------------------------------------------

/// Create signaled.
pub const DRM_SYNCOBJ_CREATE_SIGNALED: u32 = 1 << 0;
/// Wait all.
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL: u32 = 1 << 0;
/// Wait for submit.
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT: u32 = 1 << 1;
/// Wait available.
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_AVAILABLE: u32 = 1 << 2;
/// Query first signaled.
pub const DRM_SYNCOBJ_QUERY_FLAGS_LAST_SUBMITTED: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// DRM syncobj ioctls
// ---------------------------------------------------------------------------

/// Create syncobj.
pub const DRM_IOCTL_SYNCOBJ_CREATE: u32 = 0xC008_64BF;
/// Destroy syncobj.
pub const DRM_IOCTL_SYNCOBJ_DESTROY: u32 = 0xC004_64C0;
/// Handle to FD.
pub const DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD: u32 = 0xC010_64C1;
/// FD to handle.
pub const DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE: u32 = 0xC010_64C2;
/// Wait.
pub const DRM_IOCTL_SYNCOBJ_WAIT: u32 = 0xC020_64C3;
/// Reset.
pub const DRM_IOCTL_SYNCOBJ_RESET: u32 = 0xC010_64C4;
/// Signal.
pub const DRM_IOCTL_SYNCOBJ_SIGNAL: u32 = 0xC010_64C5;
/// Timeline wait.
pub const DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT: u32 = 0xC020_64CA;
/// Query.
pub const DRM_IOCTL_SYNCOBJ_QUERY: u32 = 0xC020_64CB;
/// Transfer.
pub const DRM_IOCTL_SYNCOBJ_TRANSFER: u32 = 0xC020_64CC;
/// Timeline signal.
pub const DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL: u32 = 0xC010_64CD;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fence_status_distinct() {
        let statuses = [
            FENCE_STATUS_ACTIVE,
            FENCE_STATUS_SIGNALED,
            FENCE_STATUS_ERROR,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_sync_fence_flags() {
        assert!(SYNC_FENCE_FLAG_SIGNALED.is_power_of_two());
        assert!(SYNC_FENCE_FLAG_TIMESTAMP_VALID.is_power_of_two());
        assert_eq!(
            SYNC_FENCE_FLAG_SIGNALED & SYNC_FENCE_FLAG_TIMESTAMP_VALID,
            0
        );
    }

    #[test]
    fn test_syncobj_wait_flags_no_overlap() {
        let flags = [
            DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL,
            DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT,
            DRM_SYNCOBJ_WAIT_FLAGS_WAIT_AVAILABLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_syncobj_ioctls_distinct() {
        let cmds = [
            DRM_IOCTL_SYNCOBJ_CREATE,
            DRM_IOCTL_SYNCOBJ_DESTROY,
            DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD,
            DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE,
            DRM_IOCTL_SYNCOBJ_WAIT,
            DRM_IOCTL_SYNCOBJ_RESET,
            DRM_IOCTL_SYNCOBJ_SIGNAL,
            DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT,
            DRM_IOCTL_SYNCOBJ_QUERY,
            DRM_IOCTL_SYNCOBJ_TRANSFER,
            DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
