//! `<linux/dma-mapping.h>` — DMA mapping direction and attribute constants.
//!
//! DMA mapping translates between kernel virtual addresses and bus
//! addresses that devices use for Direct Memory Access. The mapping
//! direction and attributes control cache coherence behavior and
//! IOMMU configuration.

// ---------------------------------------------------------------------------
// DMA direction
// ---------------------------------------------------------------------------

/// Bidirectional DMA (device reads and writes).
pub const DMA_BIDIRECTIONAL: u8 = 0;
/// Device reads from memory (host → device).
pub const DMA_TO_DEVICE: u8 = 1;
/// Device writes to memory (device → host).
pub const DMA_FROM_DEVICE: u8 = 2;
/// No DMA transfer direction (for preallocation).
pub const DMA_NONE: u8 = 3;

// ---------------------------------------------------------------------------
// DMA mapping attributes
// ---------------------------------------------------------------------------

/// Non-consistent (streaming) mapping.
pub const DMA_ATTR_NON_CONSISTENT: u32 = 1 << 0;
/// Don't warn on mapping failure.
pub const DMA_ATTR_NO_WARN: u32 = 1 << 1;
/// Mapping won't be used for kernel access.
pub const DMA_ATTR_NO_KERNEL_MAPPING: u32 = 1 << 2;
/// Skip CPU synchronization.
pub const DMA_ATTR_SKIP_CPU_SYNC: u32 = 1 << 3;
/// Force contiguous allocation.
pub const DMA_ATTR_FORCE_CONTIGUOUS: u32 = 1 << 4;
/// Allocate from specific NUMA node only.
pub const DMA_ATTR_ALLOC_SINGLE_PAGES: u32 = 1 << 5;
/// Privileged DMA access (for IOMMU).
pub const DMA_ATTR_PRIVILEGED: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// DMA coherence modes
// ---------------------------------------------------------------------------

/// Device is fully coherent (no cache maintenance needed).
pub const DMA_COHERENT: u8 = 0;
/// Device is non-coherent (explicit sync required).
pub const DMA_NON_COHERENT: u8 = 1;

// ---------------------------------------------------------------------------
// DMA mask constants
// ---------------------------------------------------------------------------

/// 24-bit DMA mask (ISA DMA).
pub const DMA_BIT_MASK_24: u64 = (1 << 24) - 1;
/// 32-bit DMA mask (standard PCI).
pub const DMA_BIT_MASK_32: u64 = (1u64 << 32) - 1;
/// 36-bit DMA mask (PAE).
pub const DMA_BIT_MASK_36: u64 = (1u64 << 36) - 1;
/// 40-bit DMA mask.
pub const DMA_BIT_MASK_40: u64 = (1u64 << 40) - 1;
/// 48-bit DMA mask (most modern devices).
pub const DMA_BIT_MASK_48: u64 = (1u64 << 48) - 1;
/// 64-bit DMA mask (full address space).
pub const DMA_BIT_MASK_64: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directions_distinct() {
        let dirs = [DMA_BIDIRECTIONAL, DMA_TO_DEVICE, DMA_FROM_DEVICE, DMA_NONE];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_attrs_no_overlap() {
        let attrs = [
            DMA_ATTR_NON_CONSISTENT, DMA_ATTR_NO_WARN,
            DMA_ATTR_NO_KERNEL_MAPPING, DMA_ATTR_SKIP_CPU_SYNC,
            DMA_ATTR_FORCE_CONTIGUOUS, DMA_ATTR_ALLOC_SINGLE_PAGES,
            DMA_ATTR_PRIVILEGED,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_attrs_power_of_two() {
        let attrs = [
            DMA_ATTR_NON_CONSISTENT, DMA_ATTR_NO_WARN,
            DMA_ATTR_NO_KERNEL_MAPPING, DMA_ATTR_SKIP_CPU_SYNC,
            DMA_ATTR_FORCE_CONTIGUOUS, DMA_ATTR_ALLOC_SINGLE_PAGES,
            DMA_ATTR_PRIVILEGED,
        ];
        for a in &attrs {
            assert!(a.is_power_of_two());
        }
    }

    #[test]
    fn test_masks_increasing() {
        assert!(DMA_BIT_MASK_24 < DMA_BIT_MASK_32);
        assert!(DMA_BIT_MASK_32 < DMA_BIT_MASK_36);
        assert!(DMA_BIT_MASK_36 < DMA_BIT_MASK_40);
        assert!(DMA_BIT_MASK_40 < DMA_BIT_MASK_48);
        assert!(DMA_BIT_MASK_48 < DMA_BIT_MASK_64);
    }

    #[test]
    fn test_coherence_modes_distinct() {
        assert_ne!(DMA_COHERENT, DMA_NON_COHERENT);
    }
}
