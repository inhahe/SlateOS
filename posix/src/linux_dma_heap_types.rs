//! `<linux/dma-heap.h>` — DMA-BUF heap allocation constants.
//!
//! DMA heaps provide a standardised interface for allocating DMA-able
//! buffers from specific memory pools (system, CMA, secure, etc.).
//! They replace the older ION allocator. The allocated buffers are
//! exported as DMA-BUF file descriptors for zero-copy sharing between
//! devices (GPU, camera, display, video codec).

// ---------------------------------------------------------------------------
// DMA heap ioctl commands
// ---------------------------------------------------------------------------

/// Allocate a buffer from this heap.
pub const DMA_HEAP_IOCTL_ALLOC: u32 = 0xC018_4800;

// ---------------------------------------------------------------------------
// Allocation flags
// ---------------------------------------------------------------------------

/// Allocated fd should be close-on-exec.
pub const DMA_HEAP_VALID_FD_FLAGS: u32 = 0x0008_0000; // O_CLOEXEC
/// Maximum valid heap flags (for validation).
pub const DMA_HEAP_VALID_HEAP_FLAGS: u64 = 0;

// ---------------------------------------------------------------------------
// Well-known heap names (sysfs paths under /dev/dma_heap/)
// ---------------------------------------------------------------------------

/// System heap (default, uses page allocator).
pub const DMA_HEAP_NAME_SYSTEM: &str = "system";
/// CMA heap (Contiguous Memory Allocator, for devices needing physically contiguous buffers).
pub const DMA_HEAP_NAME_CMA: &str = "reserved";

// ---------------------------------------------------------------------------
// DMA-BUF sync ioctl (on the fd returned by alloc)
// ---------------------------------------------------------------------------

/// Begin CPU access (cache invalidate / barrier).
pub const DMA_BUF_SYNC_START: u32 = 0;
/// End CPU access (cache flush / barrier).
pub const DMA_BUF_SYNC_END: u32 = 1;
/// Sync for reading.
pub const DMA_BUF_SYNC_READ: u32 = 1 << 0;
/// Sync for writing.
pub const DMA_BUF_SYNC_WRITE: u32 = 1 << 1;
/// Sync for both read and write.
pub const DMA_BUF_SYNC_RW: u32 = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE;

// ---------------------------------------------------------------------------
// DMA-BUF ioctl commands
// ---------------------------------------------------------------------------

/// Sync DMA-BUF (begin/end CPU access).
pub const DMA_BUF_IOCTL_SYNC: u32 = 0x4008_6200;
/// Export sync file from DMA-BUF fence.
pub const DMA_BUF_IOCTL_EXPORT_SYNC_FILE: u32 = 0xC004_6202;
/// Import sync file into DMA-BUF fence.
pub const DMA_BUF_IOCTL_IMPORT_SYNC_FILE: u32 = 0x4004_6203;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_names_distinct() {
        assert_ne!(DMA_HEAP_NAME_SYSTEM, DMA_HEAP_NAME_CMA);
    }

    #[test]
    fn test_sync_directions_no_overlap() {
        assert_eq!(DMA_BUF_SYNC_READ & DMA_BUF_SYNC_WRITE, 0);
        assert_eq!(DMA_BUF_SYNC_RW, DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE);
    }

    #[test]
    fn test_sync_start_end_distinct() {
        assert_ne!(DMA_BUF_SYNC_START, DMA_BUF_SYNC_END);
    }

    #[test]
    fn test_buf_ioctls_distinct() {
        let cmds = [
            DMA_BUF_IOCTL_SYNC,
            DMA_BUF_IOCTL_EXPORT_SYNC_FILE,
            DMA_BUF_IOCTL_IMPORT_SYNC_FILE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_sync_flags_are_powers_of_two() {
        assert!(DMA_BUF_SYNC_READ.is_power_of_two());
        assert!(DMA_BUF_SYNC_WRITE.is_power_of_two());
    }
}
