//! `<linux/dma-mapping.h>` (coherent subset) — DMA coherency model constants.
//!
//! DMA coherent memory is mapped such that CPU writes are
//! immediately visible to the device and vice versa, without
//! explicit cache flush/invalidate. This is essential for
//! descriptor rings and mailboxes shared between CPU and device.
//! Non-coherent (streaming) DMA requires explicit sync operations
//! but can be faster on some architectures.

// ---------------------------------------------------------------------------
// Coherency model
// ---------------------------------------------------------------------------

/// Device is DMA-coherent (CPU caches are hardware-snooped).
pub const DMA_COHERENT: u32 = 0;
/// Device is non-coherent (CPU caches must be manually flushed).
pub const DMA_NON_COHERENT: u32 = 1;

// ---------------------------------------------------------------------------
// Sync direction (for non-coherent DMA)
// ---------------------------------------------------------------------------

/// Sync for CPU access (invalidate caches before CPU reads).
pub const DMA_SYNC_FOR_CPU: u32 = 0;
/// Sync for device access (flush caches before device reads).
pub const DMA_SYNC_FOR_DEVICE: u32 = 1;

// ---------------------------------------------------------------------------
// DMA mask helpers
// ---------------------------------------------------------------------------

/// 32-bit DMA mask: device can access first 4 GiB.
pub const DMA_MASK_32BIT: u64 = 0xFFFF_FFFF;
/// 36-bit DMA mask: device can access first 64 GiB (PAE).
pub const DMA_MASK_36BIT: u64 = 0xF_FFFF_FFFF;
/// 40-bit DMA mask: device can access first 1 TiB.
pub const DMA_MASK_40BIT: u64 = 0xFF_FFFF_FFFF;
/// 48-bit DMA mask: device can access first 256 TiB.
pub const DMA_MASK_48BIT: u64 = 0xFFFF_FFFF_FFFF;
/// 64-bit DMA mask: no address restriction.
pub const DMA_MASK_64BIT: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Coherent allocation size limits
// ---------------------------------------------------------------------------

/// Default maximum coherent allocation (4 MiB).
pub const DMA_COHERENT_MAX_DEFAULT: u32 = 4 * 1024 * 1024;
/// Minimum coherent allocation (one page).
pub const DMA_COHERENT_MIN: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coherency_distinct() {
        assert_ne!(DMA_COHERENT, DMA_NON_COHERENT);
    }

    #[test]
    fn test_sync_directions_distinct() {
        assert_ne!(DMA_SYNC_FOR_CPU, DMA_SYNC_FOR_DEVICE);
    }

    #[test]
    fn test_dma_masks_ordered() {
        assert!(DMA_MASK_32BIT < DMA_MASK_36BIT);
        assert!(DMA_MASK_36BIT < DMA_MASK_40BIT);
        assert!(DMA_MASK_40BIT < DMA_MASK_48BIT);
        assert!(DMA_MASK_48BIT < DMA_MASK_64BIT);
    }

    #[test]
    fn test_dma_masks_correct() {
        assert_eq!(DMA_MASK_32BIT, (1u64 << 32) - 1);
        assert_eq!(DMA_MASK_36BIT, (1u64 << 36) - 1);
        assert_eq!(DMA_MASK_40BIT, (1u64 << 40) - 1);
        assert_eq!(DMA_MASK_48BIT, (1u64 << 48) - 1);
    }

    #[test]
    fn test_coherent_alloc_limits() {
        assert!(DMA_COHERENT_MIN <= DMA_COHERENT_MAX_DEFAULT);
        assert!(DMA_COHERENT_MIN.is_power_of_two());
    }
}
