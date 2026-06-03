//! `<linux/dma-fence.h>` — DMA fence (GPU synchronization) constants.
//!
//! DMA fences synchronize work between CPU and GPU, and between
//! different GPU engines (3D, video decode, compute, display). A fence
//! represents a point in a command stream; it signals when all commands
//! before that point have completed. Fences enable efficient pipelining:
//! the CPU can submit multiple frames of work without waiting for each
//! to complete, and the display controller can wait for rendering to
//! finish before scanning out a framebuffer.

// ---------------------------------------------------------------------------
// Fence states
// ---------------------------------------------------------------------------

/// Fence is unsignaled (work not yet complete).
pub const DMA_FENCE_UNSIGNALED: u32 = 0;
/// Fence is signaled (all prior work complete).
pub const DMA_FENCE_SIGNALED: u32 = 1;
/// Fence encountered an error.
pub const DMA_FENCE_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Fence flags
// ---------------------------------------------------------------------------

/// Fence has been signaled (atomic flag).
pub const DMA_FENCE_FLAG_SIGNALED: u32 = 0x01;
/// Fence has a timestamp recorded.
pub const DMA_FENCE_FLAG_TIMESTAMP: u32 = 0x02;
/// Fence enable signal-on-any behavior.
pub const DMA_FENCE_FLAG_ENABLE_SIGNAL: u32 = 0x04;
/// Fence was imported from another device.
pub const DMA_FENCE_FLAG_IMPORTED: u32 = 0x08;

// ---------------------------------------------------------------------------
// Sync file / fence array
// ---------------------------------------------------------------------------

/// Merge fences (all must signal = AND).
pub const SYNC_MERGE_AND: u32 = 0;
/// Any fence signals = OR (first to signal unblocks).
pub const SYNC_MERGE_OR: u32 = 1;

// ---------------------------------------------------------------------------
// Fence wait options
// ---------------------------------------------------------------------------

/// Wait forever (no timeout).
pub const FENCE_WAIT_INFINITE: i64 = -1;
/// Don't wait, just check (poll).
pub const FENCE_WAIT_NOWAIT: i64 = 0;

// ---------------------------------------------------------------------------
// Fence timeline types
// ---------------------------------------------------------------------------

/// Point fence (single signal event).
pub const FENCE_TYPE_POINT: u32 = 0;
/// Timeline fence (monotonically increasing sequence number).
pub const FENCE_TYPE_TIMELINE: u32 = 1;

// ---------------------------------------------------------------------------
// DRM syncobj operations
// ---------------------------------------------------------------------------

/// Create a sync object.
pub const DRM_SYNCOBJ_CREATE: u32 = 0;
/// Destroy a sync object.
pub const DRM_SYNCOBJ_DESTROY: u32 = 1;
/// Wait for a sync object to signal.
pub const DRM_SYNCOBJ_WAIT: u32 = 2;
/// Reset a sync object (unsignal it).
pub const DRM_SYNCOBJ_RESET: u32 = 3;
/// Signal a sync object from CPU.
pub const DRM_SYNCOBJ_SIGNAL: u32 = 4;
/// Transfer a fence between sync objects.
pub const DRM_SYNCOBJ_TRANSFER: u32 = 5;
/// Timeline wait for specific point.
pub const DRM_SYNCOBJ_TIMELINE_WAIT: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fence_states_distinct() {
        let states = [DMA_FENCE_UNSIGNALED, DMA_FENCE_SIGNALED, DMA_FENCE_ERROR];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_fence_flags_no_overlap() {
        let flags = [
            DMA_FENCE_FLAG_SIGNALED,
            DMA_FENCE_FLAG_TIMESTAMP,
            DMA_FENCE_FLAG_ENABLE_SIGNAL,
            DMA_FENCE_FLAG_IMPORTED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fence_types_distinct() {
        assert_ne!(FENCE_TYPE_POINT, FENCE_TYPE_TIMELINE);
    }

    #[test]
    fn test_syncobj_ops_distinct() {
        let ops = [
            DRM_SYNCOBJ_CREATE,
            DRM_SYNCOBJ_DESTROY,
            DRM_SYNCOBJ_WAIT,
            DRM_SYNCOBJ_RESET,
            DRM_SYNCOBJ_SIGNAL,
            DRM_SYNCOBJ_TRANSFER,
            DRM_SYNCOBJ_TIMELINE_WAIT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_wait_constants() {
        assert!(FENCE_WAIT_INFINITE < 0);
        assert_eq!(FENCE_WAIT_NOWAIT, 0);
    }
}
