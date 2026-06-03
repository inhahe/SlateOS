//! `<linux/dma-mapping.h>` — DMA (Direct Memory Access) constants.
//!
//! DMA allows hardware devices to access system memory directly
//! without CPU involvement. The DMA mapping API manages address
//! translation (IOVA ↔ physical), cache coherency, and bounce
//! buffers for devices with limited addressing capability.

// ---------------------------------------------------------------------------
// DMA direction flags
// ---------------------------------------------------------------------------

/// DMA bidirectional (both read and write).
pub const DMA_BIDIRECTIONAL: u32 = 0;
/// DMA to device (CPU writes, device reads).
pub const DMA_TO_DEVICE: u32 = 1;
/// DMA from device (device writes, CPU reads).
pub const DMA_FROM_DEVICE: u32 = 2;
/// No DMA transfer (for sync only).
pub const DMA_NONE: u32 = 3;

// ---------------------------------------------------------------------------
// DMA mapping attributes
// ---------------------------------------------------------------------------

/// Write-combine mapping (for framebuffers).
pub const DMA_ATTR_WRITE_COMBINE: u64 = 1 << 0;
/// Non-consistent (requires explicit sync).
pub const DMA_ATTR_NON_CONSISTENT: u64 = 1 << 1;
/// Skip CPU sync on unmap.
pub const DMA_ATTR_SKIP_CPU_SYNC: u64 = 1 << 2;
/// Force contiguous allocation.
pub const DMA_ATTR_FORCE_CONTIGUOUS: u64 = 1 << 3;
/// Allocate from restricted pool.
pub const DMA_ATTR_ALLOC_SINGLE_PAGES: u64 = 1 << 4;
/// Don't warn on allocation failure.
pub const DMA_ATTR_NO_WARN: u64 = 1 << 5;
/// Privileged access (for device-specific use).
pub const DMA_ATTR_PRIVILEGED: u64 = 1 << 6;

// ---------------------------------------------------------------------------
// DMA address limits
// ---------------------------------------------------------------------------

/// 32-bit DMA address mask.
pub const DMA_BIT_MASK_32: u64 = 0xFFFF_FFFF;
/// 64-bit DMA address mask.
pub const DMA_BIT_MASK_64: u64 = 0xFFFF_FFFF_FFFF_FFFF;
/// 24-bit DMA address mask (ISA DMA).
pub const DMA_BIT_MASK_24: u64 = 0x00FF_FFFF;

// ---------------------------------------------------------------------------
// Scatter-gather limits
// ---------------------------------------------------------------------------

/// Maximum segments per SG list (typical).
pub const SG_MAX_SEGMENTS: u32 = 128;
/// Maximum single segment size (common default).
pub const SG_MAX_SINGLE_ALLOC: u32 = 65536;

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
            DMA_ATTR_WRITE_COMBINE,
            DMA_ATTR_NON_CONSISTENT,
            DMA_ATTR_SKIP_CPU_SYNC,
            DMA_ATTR_FORCE_CONTIGUOUS,
            DMA_ATTR_ALLOC_SINGLE_PAGES,
            DMA_ATTR_NO_WARN,
            DMA_ATTR_PRIVILEGED,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_bit_masks_ascending() {
        assert!(DMA_BIT_MASK_24 < DMA_BIT_MASK_32);
        assert!(DMA_BIT_MASK_32 < DMA_BIT_MASK_64);
    }

    #[test]
    fn test_sg_limits_positive() {
        assert!(SG_MAX_SEGMENTS > 0);
        assert!(SG_MAX_SINGLE_ALLOC > 0);
    }
}
