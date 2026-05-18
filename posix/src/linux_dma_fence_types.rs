//! `<linux/dma-fence.h>` — DMA fence synchronisation constants.
//!
//! DMA fences coordinate access to shared buffers between CPU and
//! GPU (or between multiple GPUs). A fence represents a point in a
//! device command stream; it is "signaled" when all commands before
//! that point have completed. The fence framework provides wait,
//! callback, and timeout operations.

// ---------------------------------------------------------------------------
// Fence flags
// ---------------------------------------------------------------------------

/// Fence has been signaled (work complete).
pub const DMA_FENCE_FLAG_SIGNALED_BIT: u32 = 0;
/// Fence has a timestamp recorded.
pub const DMA_FENCE_FLAG_TIMESTAMP_BIT: u32 = 1;
/// Fence callback is enabled.
pub const DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT: u32 = 2;
/// User-defined flag start bit.
pub const DMA_FENCE_FLAG_USER_BITS: u32 = 3;

// ---------------------------------------------------------------------------
// Fence flag masks (computed from bit positions)
// ---------------------------------------------------------------------------

/// Mask for SIGNALED flag.
pub const DMA_FENCE_FLAG_SIGNALED: u32 = 1 << DMA_FENCE_FLAG_SIGNALED_BIT;
/// Mask for TIMESTAMP flag.
pub const DMA_FENCE_FLAG_TIMESTAMP: u32 = 1 << DMA_FENCE_FLAG_TIMESTAMP_BIT;
/// Mask for ENABLE_SIGNAL flag.
pub const DMA_FENCE_FLAG_ENABLE_SIGNAL: u32 = 1 << DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT;

// ---------------------------------------------------------------------------
// Fence wait timeout
// ---------------------------------------------------------------------------

/// Maximum wait timeout in jiffies (effectively infinite).
pub const DMA_FENCE_WAIT_TIMEOUT_MAX: i64 = i64::MAX;
/// No-wait (poll without blocking).
pub const DMA_FENCE_NO_WAIT: i64 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_bits_ordered() {
        assert!(DMA_FENCE_FLAG_SIGNALED_BIT < DMA_FENCE_FLAG_TIMESTAMP_BIT);
        assert!(DMA_FENCE_FLAG_TIMESTAMP_BIT < DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT);
        assert!(DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT < DMA_FENCE_FLAG_USER_BITS);
    }

    #[test]
    fn test_flag_masks_correct() {
        assert_eq!(DMA_FENCE_FLAG_SIGNALED, 1 << 0);
        assert_eq!(DMA_FENCE_FLAG_TIMESTAMP, 1 << 1);
        assert_eq!(DMA_FENCE_FLAG_ENABLE_SIGNAL, 1 << 2);
    }

    #[test]
    fn test_flag_masks_no_overlap() {
        let masks = [
            DMA_FENCE_FLAG_SIGNALED,
            DMA_FENCE_FLAG_TIMESTAMP,
            DMA_FENCE_FLAG_ENABLE_SIGNAL,
        ];
        for i in 0..masks.len() {
            assert!(masks[i].is_power_of_two());
            for j in (i + 1)..masks.len() {
                assert_eq!(masks[i] & masks[j], 0);
            }
        }
    }

    #[test]
    fn test_wait_timeouts() {
        assert_eq!(DMA_FENCE_NO_WAIT, 0);
        assert!(DMA_FENCE_WAIT_TIMEOUT_MAX > 0);
    }

    #[test]
    fn test_user_bits_after_internal() {
        // User-defined flags start after all internal flags
        assert_eq!(DMA_FENCE_FLAG_USER_BITS, 3);
    }
}
