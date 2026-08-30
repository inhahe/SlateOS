//! `<linux/dmapool.h>` — DMA pool allocation constants.
//!
//! DMA pools provide efficient allocation of small, fixed-size,
//! DMA-coherent memory blocks from a pre-allocated region. They
//! avoid the overhead of calling `dma_alloc_coherent()` for every
//! small allocation (e.g., USB transfer descriptors, ring buffer
//! entries). The kernel pre-allocates pages and carves them into
//! fixed-size chunks aligned for DMA.

// ---------------------------------------------------------------------------
// DMA pool size constraints
// ---------------------------------------------------------------------------

/// Minimum useful pool allocation size (bytes).
pub const DMA_POOL_MIN_ALLOC: u32 = 4;
/// Maximum pool allocation size (one page, 4096 bytes on x86).
pub const DMA_POOL_MAX_ALLOC: u32 = 4096;
/// Default pool alignment (bytes).
pub const DMA_POOL_DEFAULT_ALIGN: u32 = 4;

// ---------------------------------------------------------------------------
// DMA allocation alignment requirements
// ---------------------------------------------------------------------------

/// Minimum DMA buffer alignment for most architectures.
pub const DMA_MIN_ALIGN: u32 = 64;
/// Cache-line alignment (common for DMA descriptors).
pub const DMA_CACHELINE_ALIGN: u32 = 64;
/// Page alignment (for DMA-coherent whole-page allocations).
pub const DMA_PAGE_ALIGN: u32 = 4096;

// ---------------------------------------------------------------------------
// DMA address special values
// ---------------------------------------------------------------------------

/// Invalid DMA address (used as error sentinel).
pub const DMA_ADDR_INVALID: u64 = 0;
/// Maximum 32-bit DMA address (4 GiB boundary).
pub const DMA_BIT_MASK_32: u64 = 0xFFFF_FFFF;
/// Maximum 64-bit DMA address (no restriction).
pub const DMA_BIT_MASK_64: u64 = u64::MAX;
/// Maximum 28-bit DMA address (ISA DMA).
pub const DMA_BIT_MASK_28: u64 = 0x0FFF_FFFF;
/// Maximum 24-bit DMA address (legacy ISA).
pub const DMA_BIT_MASK_24: u64 = 0x00FF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_constraints() {
        assert!(DMA_POOL_MIN_ALLOC <= DMA_POOL_MAX_ALLOC);
        assert!(DMA_POOL_MIN_ALLOC > 0);
        // Max should be at most one page
        assert!(DMA_POOL_MAX_ALLOC <= DMA_PAGE_ALIGN);
    }

    #[test]
    fn test_alignments_power_of_two() {
        assert!(DMA_POOL_DEFAULT_ALIGN.is_power_of_two());
        assert!(DMA_MIN_ALIGN.is_power_of_two());
        assert!(DMA_CACHELINE_ALIGN.is_power_of_two());
        assert!(DMA_PAGE_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_alignment_order() {
        assert!(DMA_POOL_DEFAULT_ALIGN <= DMA_MIN_ALIGN);
        assert!(DMA_MIN_ALIGN <= DMA_CACHELINE_ALIGN);
        assert!(DMA_CACHELINE_ALIGN <= DMA_PAGE_ALIGN);
    }

    #[test]
    fn test_bit_masks_ordered() {
        assert!(DMA_BIT_MASK_24 < DMA_BIT_MASK_28);
        assert!(DMA_BIT_MASK_28 < DMA_BIT_MASK_32);
        assert!(DMA_BIT_MASK_32 < DMA_BIT_MASK_64);
    }

    #[test]
    fn test_bit_masks_correct() {
        assert_eq!(DMA_BIT_MASK_32, (1u64 << 32) - 1);
        assert_eq!(DMA_BIT_MASK_24, (1u64 << 24) - 1);
        assert_eq!(DMA_BIT_MASK_28, (1u64 << 28) - 1);
    }

    #[test]
    fn test_dma_addr_invalid() {
        assert_eq!(DMA_ADDR_INVALID, 0);
    }
}
