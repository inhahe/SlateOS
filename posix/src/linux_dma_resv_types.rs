//! `<linux/dma-resv.h>` — DMA reservation object usage constants.
//!
//! DMA reservation objects (dma_resv) track shared and exclusive
//! fences on a buffer. They implement the implicit synchronisation
//! model: when a buffer is submitted to a GPU for rendering
//! (exclusive) or texturing (shared), the appropriate fence is
//! attached. Subsequent users wait on the fence automatically,
//! ensuring correct ordering without explicit application-level
//! synchronisation.

// ---------------------------------------------------------------------------
// Reservation usage types (enum dma_resv_usage)
// ---------------------------------------------------------------------------

/// Kernel-internal usage (highest priority, always waited on).
pub const DMA_RESV_USAGE_KERNEL: u32 = 0;
/// Write / exclusive usage (rendering into buffer).
pub const DMA_RESV_USAGE_WRITE: u32 = 1;
/// Read / shared usage (sampling from buffer).
pub const DMA_RESV_USAGE_READ: u32 = 2;
/// Bookkeeping usage (lowest priority, may be skipped).
pub const DMA_RESV_USAGE_BOOKKEEP: u32 = 3;

// ---------------------------------------------------------------------------
// Reservation object lock types
// ---------------------------------------------------------------------------

/// Lock for read access (shared).
pub const DMA_RESV_LOCK_READ: u32 = 0;
/// Lock for write access (exclusive).
pub const DMA_RESV_LOCK_WRITE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_types_distinct() {
        let usages = [
            DMA_RESV_USAGE_KERNEL,
            DMA_RESV_USAGE_WRITE,
            DMA_RESV_USAGE_READ,
            DMA_RESV_USAGE_BOOKKEEP,
        ];
        for i in 0..usages.len() {
            for j in (i + 1)..usages.len() {
                assert_ne!(usages[i], usages[j]);
            }
        }
    }

    #[test]
    fn test_usage_priority_order() {
        // Lower value = higher priority
        assert!(DMA_RESV_USAGE_KERNEL < DMA_RESV_USAGE_WRITE);
        assert!(DMA_RESV_USAGE_WRITE < DMA_RESV_USAGE_READ);
        assert!(DMA_RESV_USAGE_READ < DMA_RESV_USAGE_BOOKKEEP);
    }

    #[test]
    fn test_lock_types_distinct() {
        assert_ne!(DMA_RESV_LOCK_READ, DMA_RESV_LOCK_WRITE);
    }

    #[test]
    fn test_usage_sequential() {
        assert_eq!(DMA_RESV_USAGE_KERNEL, 0);
        assert_eq!(DMA_RESV_USAGE_WRITE, 1);
        assert_eq!(DMA_RESV_USAGE_READ, 2);
        assert_eq!(DMA_RESV_USAGE_BOOKKEEP, 3);
    }
}
