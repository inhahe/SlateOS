//! `<linux/iova.h>` — IOVA (I/O Virtual Address) allocator constants.
//!
//! The IOVA allocator manages I/O virtual address space for IOMMU
//! mappings. When devices perform DMA, they use IOVAs that the IOMMU
//! translates to physical addresses. The allocator provides fast
//! allocation/deallocation of address ranges for DMA mappings.

// ---------------------------------------------------------------------------
// IOVA allocation strategies
// ---------------------------------------------------------------------------

/// Bottom-up allocation (from lowest address).
pub const IOVA_ALLOC_FROM_LOW: u8 = 0;
/// Top-down allocation (from highest address, default for DMA).
pub const IOVA_ALLOC_FROM_HIGH: u8 = 1;

// ---------------------------------------------------------------------------
// IOVA address range constants
// ---------------------------------------------------------------------------

/// Default minimum IOVA (4 KiB, avoiding address 0).
pub const IOVA_START_PFN: u64 = 1;
/// Default DMA 32-bit limit (4 GiB boundary PFN at 4K pages).
pub const IOVA_DMA32_PFN_LIMIT: u64 = 0x100000;
/// Maximum 48-bit IOVA PFN (typical IOMMU limit).
pub const IOVA_MAX_48BIT_PFN: u64 = 0x1_0000_0000;

// ---------------------------------------------------------------------------
// IOVA cache (magazine) constants
// ---------------------------------------------------------------------------

/// Number of entries per CPU cache magazine.
pub const IOVA_MAG_SIZE: u32 = 128;
/// Maximum cached IOVAs per CPU.
pub const IOVA_MAX_CACHED: u32 = 1024;

// ---------------------------------------------------------------------------
// IOVA granularity
// ---------------------------------------------------------------------------

/// Minimum IOVA granularity (page size, 4096).
pub const IOVA_MIN_GRANULE: u32 = 4096;
/// Typical IOMMU page sizes supported.
pub const IOVA_PAGE_4K: u32 = 4096;
/// 2 MiB large page.
pub const IOVA_PAGE_2M: u32 = 2 * 1024 * 1024;
/// 1 GiB huge page.
pub const IOVA_PAGE_1G: u32 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_strategies_distinct() {
        assert_ne!(IOVA_ALLOC_FROM_LOW, IOVA_ALLOC_FROM_HIGH);
    }

    #[test]
    fn test_address_ordering() {
        assert!(IOVA_START_PFN < IOVA_DMA32_PFN_LIMIT);
        assert!(IOVA_DMA32_PFN_LIMIT < IOVA_MAX_48BIT_PFN);
    }

    #[test]
    fn test_cache_sizes() {
        assert!(IOVA_MAG_SIZE > 0);
        assert!(IOVA_MAG_SIZE < IOVA_MAX_CACHED);
    }

    #[test]
    fn test_page_sizes() {
        assert!(IOVA_PAGE_4K < IOVA_PAGE_2M);
        assert!(IOVA_PAGE_2M < IOVA_PAGE_1G);
        assert!(IOVA_PAGE_4K.is_power_of_two());
        assert!(IOVA_PAGE_2M.is_power_of_two());
        assert!(IOVA_PAGE_1G.is_power_of_two());
    }
}
