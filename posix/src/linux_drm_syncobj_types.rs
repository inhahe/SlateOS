//! `<drm/drm.h>` — DRM sync object (timeline fence) constants.
//!
//! Sync objects provide GPU synchronization primitives that can be
//! shared between processes and devices. They wrap DMA fences and
//! support both binary (signaled/unsignaled) and timeline (monotonic
//! point values) semantics for explicit GPU-GPU and GPU-CPU sync.

// ---------------------------------------------------------------------------
// Sync object flags (create)
// ---------------------------------------------------------------------------

/// Create a signaled syncobj (immediately usable).
pub const DRM_SYNCOBJ_CREATE_SIGNALED: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Sync object wait flags
// ---------------------------------------------------------------------------

/// Wait for all syncobjs (AND semantics).
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL: u32 = 1 << 0;
/// Wait for any syncobj to be available (for submission).
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT: u32 = 1 << 1;
/// Wait for deadline (returns which are ready).
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_DEADLINE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Sync object handle operations
// ---------------------------------------------------------------------------

/// Transfer sync object handle to another process (FD export).
pub const DRM_SYNCOBJ_HANDLE_TO_FD_FLAGS_EXPORT_SYNC_FILE: u32 = 1 << 0;
/// Import a sync file into a syncobj.
pub const DRM_SYNCOBJ_FD_TO_HANDLE_FLAGS_IMPORT_SYNC_FILE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Timeline syncobj operations
// ---------------------------------------------------------------------------

/// Query timeline point value.
pub const DRM_SYNCOBJ_TIMELINE_QUERY: u32 = 0;
/// Signal a timeline point.
pub const DRM_SYNCOBJ_TIMELINE_SIGNAL: u32 = 1;
/// Wait for a timeline point.
pub const DRM_SYNCOBJ_TIMELINE_WAIT: u32 = 2;
/// Transfer timeline point between syncobjs.
pub const DRM_SYNCOBJ_TIMELINE_TRANSFER: u32 = 3;

// ---------------------------------------------------------------------------
// DMA fence status values
// ---------------------------------------------------------------------------

/// Fence not yet signaled.
pub const DMA_FENCE_STATUS_UNSIGNALED: i32 = 0;
/// Fence signaled successfully.
pub const DMA_FENCE_STATUS_SIGNALED: i32 = 1;
/// Fence signaled with error.
pub const DMA_FENCE_STATUS_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flag() {
        assert!(DRM_SYNCOBJ_CREATE_SIGNALED.is_power_of_two());
    }

    #[test]
    fn test_wait_flags_no_overlap() {
        let flags = [
            DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL,
            DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT,
            DRM_SYNCOBJ_WAIT_FLAGS_WAIT_DEADLINE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_timeline_ops_distinct() {
        let ops = [
            DRM_SYNCOBJ_TIMELINE_QUERY, DRM_SYNCOBJ_TIMELINE_SIGNAL,
            DRM_SYNCOBJ_TIMELINE_WAIT, DRM_SYNCOBJ_TIMELINE_TRANSFER,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_fence_status_distinct() {
        assert_ne!(DMA_FENCE_STATUS_UNSIGNALED, DMA_FENCE_STATUS_SIGNALED);
        assert_ne!(DMA_FENCE_STATUS_SIGNALED, DMA_FENCE_STATUS_ERROR);
        assert_ne!(DMA_FENCE_STATUS_UNSIGNALED, DMA_FENCE_STATUS_ERROR);
    }
}
