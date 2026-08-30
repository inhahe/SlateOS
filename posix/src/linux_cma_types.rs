//! `<linux/cma.h>` — Contiguous Memory Allocator constants.
//!
//! CMA reserves regions of physical memory at boot time for use by
//! devices that require large physically-contiguous buffers (DMA
//! engines, camera ISPs, GPU framebuffers, codec input buffers).
//! When not needed by devices, CMA regions are available for normal
//! movable page allocations. When a contiguous allocation is needed,
//! the kernel migrates movable pages out and returns the contiguous
//! block. This avoids the fragmentation problems of trying to find
//! large contiguous regions at runtime.

// ---------------------------------------------------------------------------
// CMA allocation flags
// ---------------------------------------------------------------------------

/// Default allocation (migrate pages if needed).
pub const CMA_ALLOC_DEFAULT: u32 = 0x00;
/// Don't block waiting for migration (fail if not immediately available).
pub const CMA_ALLOC_NOWAIT: u32 = 0x01;
/// GFP_KERNEL context (can sleep, can migrate).
pub const CMA_ALLOC_GFP_KERNEL: u32 = 0x02;

// ---------------------------------------------------------------------------
// CMA area states
// ---------------------------------------------------------------------------

/// CMA area is not initialized.
pub const CMA_STATE_UNINIT: u32 = 0;
/// CMA area is initialized and available.
pub const CMA_STATE_ACTIVE: u32 = 1;
/// CMA area is fully allocated (no free contiguous space).
pub const CMA_STATE_FULL: u32 = 2;

// ---------------------------------------------------------------------------
// CMA alignment and size constraints
// ---------------------------------------------------------------------------

/// Minimum CMA alignment order (must be at least page-aligned).
pub const CMA_MIN_ALIGNMENT_ORDER: u32 = 0;
/// Default CMA region alignment (pages, order 8 = 256 pages = 1MB for 4K pages).
pub const CMA_DEFAULT_ALIGNMENT_ORDER: u32 = 8;
/// Maximum number of CMA areas.
pub const CMA_MAX_AREAS: u32 = 64;
/// Maximum alignment order for CMA allocation.
pub const CMA_MAX_ALIGNMENT_ORDER: u32 = 20;

// ---------------------------------------------------------------------------
// CMA bitmap granularity
// ---------------------------------------------------------------------------

/// Bitmap granularity order (tracks allocation in units of 2^order pages).
pub const CMA_BITMAP_GRANULARITY_ORDER: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [CMA_STATE_UNINIT, CMA_STATE_ACTIVE, CMA_STATE_FULL];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_alignment_ordering() {
        assert!(CMA_MIN_ALIGNMENT_ORDER <= CMA_DEFAULT_ALIGNMENT_ORDER);
        assert!(CMA_DEFAULT_ALIGNMENT_ORDER <= CMA_MAX_ALIGNMENT_ORDER);
    }

    #[test]
    fn test_limits() {
        assert!(CMA_MAX_AREAS > 0);
        assert!(CMA_MAX_ALIGNMENT_ORDER > 0);
    }
}
