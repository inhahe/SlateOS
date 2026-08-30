//! `<linux/dma-mapping.h>` — DMA mapping attribute flags.
//!
//! DMA mapping attributes control cache behaviour, allocation
//! strategy, and coherency for DMA-capable memory. Drivers pass
//! these flags to `dma_alloc_attrs()` and `dma_map_single_attrs()`
//! to tune performance vs. correctness tradeoffs.

// ---------------------------------------------------------------------------
// DMA mapping attributes (DMA_ATTR_*)
// ---------------------------------------------------------------------------

/// Weak ordering: relaxed memory ordering for DMA writes.
pub const DMA_ATTR_WEAK_ORDERING: u64 = 1 << 1;
/// Write combine: use write-combining mapping for performance.
pub const DMA_ATTR_WRITE_COMBINE: u64 = 1 << 2;
/// No kernel mapping: don't create a CPU-accessible mapping.
pub const DMA_ATTR_NO_KERNEL_MAPPING: u64 = 1 << 4;
/// Skip CPU sync: caller handles cache maintenance manually.
pub const DMA_ATTR_SKIP_CPU_SYNC: u64 = 1 << 5;
/// Force contiguous: allocate physically contiguous memory.
pub const DMA_ATTR_FORCE_CONTIGUOUS: u64 = 1 << 6;
/// Alloc single pages: allocate from single-page pool.
pub const DMA_ATTR_ALLOC_SINGLE_PAGES: u64 = 1 << 7;
/// No warn: suppress allocation failure warnings.
pub const DMA_ATTR_NO_WARN: u64 = 1 << 8;
/// Privileged: DMA access uses privileged bus transactions.
pub const DMA_ATTR_PRIVILEGED: u64 = 1 << 9;

// ---------------------------------------------------------------------------
// DMA mapping error sentinel
// ---------------------------------------------------------------------------

/// Sentinel value indicating a DMA mapping error (~0).
pub const DMA_MAPPING_ERROR: u64 = !0u64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_are_power_of_two() {
        let attrs = [
            DMA_ATTR_WEAK_ORDERING,
            DMA_ATTR_WRITE_COMBINE,
            DMA_ATTR_NO_KERNEL_MAPPING,
            DMA_ATTR_SKIP_CPU_SYNC,
            DMA_ATTR_FORCE_CONTIGUOUS,
            DMA_ATTR_ALLOC_SINGLE_PAGES,
            DMA_ATTR_NO_WARN,
            DMA_ATTR_PRIVILEGED,
        ];
        for &a in &attrs {
            assert!(a.is_power_of_two(), "attr 0x{:X} is not power of two", a);
        }
    }

    #[test]
    fn test_attrs_no_overlap() {
        let attrs = [
            DMA_ATTR_WEAK_ORDERING,
            DMA_ATTR_WRITE_COMBINE,
            DMA_ATTR_NO_KERNEL_MAPPING,
            DMA_ATTR_SKIP_CPU_SYNC,
            DMA_ATTR_FORCE_CONTIGUOUS,
            DMA_ATTR_ALLOC_SINGLE_PAGES,
            DMA_ATTR_NO_WARN,
            DMA_ATTR_PRIVILEGED,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_eq!(
                    attrs[i] & attrs[j],
                    0,
                    "attrs 0x{:X} and 0x{:X} overlap",
                    attrs[i],
                    attrs[j]
                );
            }
        }
    }

    #[test]
    fn test_mapping_error() {
        assert_eq!(DMA_MAPPING_ERROR, u64::MAX);
    }

    #[test]
    fn test_attrs_composable() {
        let combined = DMA_ATTR_WRITE_COMBINE | DMA_ATTR_NO_WARN;
        assert_ne!(combined & DMA_ATTR_WRITE_COMBINE, 0);
        assert_ne!(combined & DMA_ATTR_NO_WARN, 0);
        assert_eq!(combined & DMA_ATTR_WEAK_ORDERING, 0);
    }
}
