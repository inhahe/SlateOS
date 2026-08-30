//! `<linux/dma-direction.h>` — DMA transfer direction constants.
//!
//! DMA direction tells the DMA mapping API which way data flows
//! between CPU memory and a device. The direction controls cache
//! maintenance (flush before device read, invalidate before CPU
//! read) and IOMMU permission setup. Getting it wrong causes
//! stale data or security violations.

// ---------------------------------------------------------------------------
// DMA direction codes (enum dma_data_direction)
// ---------------------------------------------------------------------------

/// Bidirectional: device may both read and write the buffer.
pub const DMA_BIDIRECTIONAL: u32 = 0;
/// To device: CPU writes, device reads (e.g. transmit buffer).
pub const DMA_TO_DEVICE: u32 = 1;
/// From device: device writes, CPU reads (e.g. receive buffer).
pub const DMA_FROM_DEVICE: u32 = 2;
/// No DMA transfer (used for unmapping or error).
pub const DMA_NONE: u32 = 3;

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
    fn test_directions_sequential() {
        assert_eq!(DMA_BIDIRECTIONAL, 0);
        assert_eq!(DMA_TO_DEVICE, 1);
        assert_eq!(DMA_FROM_DEVICE, 2);
        assert_eq!(DMA_NONE, 3);
    }

    #[test]
    fn test_bidirectional_is_zero() {
        // Bidirectional is the most permissive, used as default
        assert_eq!(DMA_BIDIRECTIONAL, 0);
    }

    #[test]
    fn test_none_is_last() {
        assert!(DMA_NONE > DMA_TO_DEVICE);
        assert!(DMA_NONE > DMA_FROM_DEVICE);
    }
}
