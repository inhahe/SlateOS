//! `<linux/dma-buf.h>` — DMA-BUF buffer sharing constants.
//!
//! DMA-BUF is the kernel framework for sharing buffers between
//! devices (GPU, video codec, camera, display) without copying.
//! An exporter creates a dma-buf and passes the file descriptor to
//! an importer. Both can map the buffer for their DMA engines.
//! IOCTLs on the dma-buf fd control cache synchronization. Used
//! extensively in graphics (GBM/EGL), video pipelines (V4L2→GPU),
//! and zero-copy multimedia.

// ---------------------------------------------------------------------------
// DMA-BUF IOCTLs
// ---------------------------------------------------------------------------

/// Begin CPU access (sync for CPU read/write).
pub const DMA_BUF_IOCTL_SYNC: u32 = 0x4008_6200;

// ---------------------------------------------------------------------------
// DMA-BUF sync flags
// ---------------------------------------------------------------------------

/// Sync for reading.
pub const DMA_BUF_SYNC_READ: u32 = 1 << 0;
/// Sync for writing.
pub const DMA_BUF_SYNC_WRITE: u32 = 1 << 1;
/// Sync for read and write.
pub const DMA_BUF_SYNC_RW: u32 = (1 << 0) | (1 << 1);
/// Start of CPU access.
pub const DMA_BUF_SYNC_START: u32 = 0 << 2;
/// End of CPU access.
pub const DMA_BUF_SYNC_END: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// DMA-BUF export flags
// ---------------------------------------------------------------------------

/// Buffer is readable by importer.
pub const DMA_BUF_FLAG_READ: u32 = 1 << 0;
/// Buffer is writable by importer.
pub const DMA_BUF_FLAG_WRITE: u32 = 1 << 1;
/// Close-on-exec flag for exported fd.
pub const DMA_BUF_FLAG_CLOEXEC: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// DMA-BUF heap IOCTLs (from /dev/dma_heap/<name>)
// ---------------------------------------------------------------------------

/// Allocate a buffer from a DMA heap.
pub const DMA_HEAP_IOCTL_ALLOC: u32 = 0x4010_4800;

// ---------------------------------------------------------------------------
// DMA heap allocation flags
// ---------------------------------------------------------------------------

/// Allocate cached (CPU-cached) memory.
pub const DMA_HEAP_FLAG_CACHED: u32 = 0;
/// Allocate uncached memory.
pub const DMA_HEAP_FLAG_UNCACHED: u32 = 1;

// ---------------------------------------------------------------------------
// Well-known DMA heap names
// ---------------------------------------------------------------------------

/// System heap (CMA or page allocator).
pub const DMA_HEAP_SYSTEM: &str = "system";
/// CMA (Contiguous Memory Allocator) heap.
pub const DMA_HEAP_CMA: &str = "linux,cma";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_flags_composition() {
        assert_eq!(DMA_BUF_SYNC_RW, DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE);
    }

    #[test]
    fn test_sync_read_write_no_overlap_with_direction() {
        // Read/write flags don't overlap with start/end flag
        assert_eq!(DMA_BUF_SYNC_READ & DMA_BUF_SYNC_END, 0);
        assert_eq!(DMA_BUF_SYNC_WRITE & DMA_BUF_SYNC_END, 0);
    }

    #[test]
    fn test_start_end_distinct() {
        assert_ne!(DMA_BUF_SYNC_START, DMA_BUF_SYNC_END);
    }

    #[test]
    fn test_export_flags_no_overlap() {
        let flags = [DMA_BUF_FLAG_READ, DMA_BUF_FLAG_WRITE, DMA_BUF_FLAG_CLOEXEC];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_heap_flags_distinct() {
        assert_ne!(DMA_HEAP_FLAG_CACHED, DMA_HEAP_FLAG_UNCACHED);
    }

    #[test]
    fn test_heap_names_distinct() {
        assert_ne!(DMA_HEAP_SYSTEM, DMA_HEAP_CMA);
    }
}
