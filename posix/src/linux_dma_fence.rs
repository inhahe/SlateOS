//! `<linux/dma-fence.h>` — DMA fence synchronization constants.
//!
//! DMA fences are the kernel's mechanism for synchronizing GPU and
//! DMA operations across drivers and hardware. Used by DRM/KMS,
//! V4L2, and the DMA-BUF subsystem for implicit/explicit sync.

// ---------------------------------------------------------------------------
// Fence flags
// ---------------------------------------------------------------------------

/// Fence has been signaled.
pub const DMA_FENCE_FLAG_SIGNALED_BIT: u32 = 0;
/// Fence was signaled with a timestamp.
pub const DMA_FENCE_FLAG_TIMESTAMP_BIT: u32 = 1;
/// Enable SW signaling (interrupt on signal).
pub const DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT: u32 = 2;
/// User-defined fence flags start here.
pub const DMA_FENCE_FLAG_USER_BITS: u32 = 3;

// ---------------------------------------------------------------------------
// Sync file ioctl commands
// ---------------------------------------------------------------------------

/// Get sync file info.
pub const SYNC_IOC_FILE_INFO: u32 = 0xC020_3E04;
/// Merge two sync files.
pub const SYNC_IOC_MERGE: u32 = 0xC020_3E01;

// ---------------------------------------------------------------------------
// Fence status values
// ---------------------------------------------------------------------------

/// Fence signaled (positive = signaled).
pub const DMA_FENCE_STATUS_SIGNALED: i32 = 1;
/// Fence active (not yet signaled).
pub const DMA_FENCE_STATUS_ACTIVE: i32 = 0;
/// Fence error.
pub const DMA_FENCE_STATUS_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// DMA fence chain/timeline constants
// ---------------------------------------------------------------------------

/// No timeline point (any).
pub const DMA_FENCE_NO_SEQNO: u64 = 0;

// ---------------------------------------------------------------------------
// Sync file fence info flags
// ---------------------------------------------------------------------------

/// Fence info: signaled.
pub const SYNC_FENCE_FLAG_SIGNALED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_bits_distinct() {
        let bits = [
            DMA_FENCE_FLAG_SIGNALED_BIT,
            DMA_FENCE_FLAG_TIMESTAMP_BIT,
            DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT,
            DMA_FENCE_FLAG_USER_BITS,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_status_values() {
        assert!(DMA_FENCE_STATUS_SIGNALED > 0);
        assert_eq!(DMA_FENCE_STATUS_ACTIVE, 0);
        assert!(DMA_FENCE_STATUS_ERROR < 0);
    }

    #[test]
    fn test_sync_ioctls_distinct() {
        assert_ne!(SYNC_IOC_FILE_INFO, SYNC_IOC_MERGE);
    }

    #[test]
    fn test_no_seqno() {
        assert_eq!(DMA_FENCE_NO_SEQNO, 0);
    }

    #[test]
    fn test_sync_fence_flag() {
        assert_eq!(SYNC_FENCE_FLAG_SIGNALED, 1);
    }
}
