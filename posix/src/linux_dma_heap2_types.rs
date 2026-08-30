//! `<linux/dma-heap.h>` — Additional DMA-heap constants.
//!
//! Supplementary DMA-heap constants covering allocation flags,
//! heap types, and ioctl commands.

// ---------------------------------------------------------------------------
// DMA-heap allocation flags
// ---------------------------------------------------------------------------

/// Valid heap flags mask.
pub const DMA_HEAP_VALID_HEAP_FLAGS: u64 = 0;

/// Valid fd flags mask.
pub const DMA_HEAP_VALID_FD_FLAGS: u64 = 0o02000000 | 0o00004000;

// ---------------------------------------------------------------------------
// DMA-heap ioctl commands
// ---------------------------------------------------------------------------

/// Allocate buffer.
pub const DMA_HEAP_IOCTL_ALLOC: u32 = 0xC0184800;

// ---------------------------------------------------------------------------
// DMA-heap known heap names
// ---------------------------------------------------------------------------

/// System heap (CMA).
pub const DMA_HEAP_SYSTEM: u32 = 0;
/// CMA heap.
pub const DMA_HEAP_CMA: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_heap_flags() {
        assert_eq!(DMA_HEAP_VALID_HEAP_FLAGS, 0);
    }

    #[test]
    fn test_heap_types_distinct() {
        assert_ne!(DMA_HEAP_SYSTEM, DMA_HEAP_CMA);
    }

    #[test]
    fn test_ioctl_nonzero() {
        assert_ne!(DMA_HEAP_IOCTL_ALLOC, 0);
    }
}
