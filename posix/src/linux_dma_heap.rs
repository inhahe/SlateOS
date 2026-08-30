//! `<linux/dma-heap.h>` — DMA-BUF heap allocation constants.
//!
//! DMA-BUF heaps provide a standardized interface for allocating
//! DMA-capable buffers from different memory backends (system
//! memory, CMA, carveouts). Replaces the Android ION allocator.

// ---------------------------------------------------------------------------
// Heap names
// ---------------------------------------------------------------------------

/// System heap (uses page allocator + CMA fallback).
pub const DMA_HEAP_SYSTEM: &str = "system";
/// CMA heap (Contiguous Memory Allocator).
pub const DMA_HEAP_CMA: &str = "linux,cma";

// ---------------------------------------------------------------------------
// Allocation flags
// ---------------------------------------------------------------------------

/// No specific flags (default allocation).
pub const DMA_HEAP_ALLOC_FLAGS_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// ioctl definitions
// ---------------------------------------------------------------------------

/// DMA heap ioctl magic.
pub const DMA_HEAP_IOCTL_MAGIC: u8 = b'H';

/// Allocate a buffer.
pub const DMA_HEAP_IOCTL_ALLOC: u32 = 0;

// ---------------------------------------------------------------------------
// Device paths
// ---------------------------------------------------------------------------

/// DMA heap device directory.
pub const DMA_HEAP_DEV_DIR: &str = "/dev/dma_heap";
/// System heap device path.
pub const DMA_HEAP_DEV_SYSTEM: &str = "/dev/dma_heap/system";

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

/// Minimum allocation alignment (page size).
pub const DMA_HEAP_MIN_ALIGN: usize = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_names_distinct() {
        assert_ne!(DMA_HEAP_SYSTEM, DMA_HEAP_CMA);
    }

    #[test]
    fn test_default_flags() {
        assert_eq!(DMA_HEAP_ALLOC_FLAGS_DEFAULT, 0);
    }

    #[test]
    fn test_dev_paths_distinct() {
        assert_ne!(DMA_HEAP_DEV_DIR, DMA_HEAP_DEV_SYSTEM);
    }

    #[test]
    fn test_min_align_power_of_two() {
        assert!(DMA_HEAP_MIN_ALIGN.is_power_of_two());
    }
}
