//! `<linux/dma-buf.h>` — Additional DMA-BUF constants.
//!
//! Supplementary DMA-BUF constants covering sync flags,
//! ioctl commands, heap flags, and export flags.

// ---------------------------------------------------------------------------
// DMA-BUF sync flags
// ---------------------------------------------------------------------------

/// Sync for read.
pub const DMA_BUF_SYNC_READ: u64 = 1 << 0;
/// Sync for write.
pub const DMA_BUF_SYNC_WRITE: u64 = 1 << 1;
/// Sync for read+write.
pub const DMA_BUF_SYNC_RW: u64 = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE;
/// Start sync.
pub const DMA_BUF_SYNC_START: u64 = 0 << 2;
/// End sync.
pub const DMA_BUF_SYNC_END: u64 = 1 << 2;
/// Valid flags mask.
pub const DMA_BUF_SYNC_VALID_FLAGS_MASK: u64 =
    DMA_BUF_SYNC_RW | DMA_BUF_SYNC_END;

// ---------------------------------------------------------------------------
// DMA-BUF ioctl commands
// ---------------------------------------------------------------------------

/// Sync ioctl.
pub const DMA_BUF_IOCTL_SYNC: u32 = 0x4008_6200;
/// Set name.
pub const DMA_BUF_SET_NAME_A: u32 = 0x4004_6201;
/// Set name (compat).
pub const DMA_BUF_SET_NAME_B: u32 = 0x4008_6201;

// ---------------------------------------------------------------------------
// DMA heap flags
// ---------------------------------------------------------------------------

/// Cached CPU mappings.
pub const DMA_HEAP_VALID_FD_FLAGS: u32 = 0x03;
/// CMA heap.
pub const DMA_HEAP_VALID_HEAP_FLAGS: u32 = 0x00;

// ---------------------------------------------------------------------------
// DMA fence flags
// ---------------------------------------------------------------------------

/// Fence signaled.
pub const DMA_FENCE_FLAG_SIGNALED_BIT: u32 = 0;
/// Fence timestamp valid.
pub const DMA_FENCE_FLAG_TIMESTAMP_BIT: u32 = 1;
/// Fence enable signaling.
pub const DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT: u32 = 2;
/// User bits start.
pub const DMA_FENCE_FLAG_USER_BITS: u32 = 3;

// ---------------------------------------------------------------------------
// Sync file constants
// ---------------------------------------------------------------------------

/// Sync file merge.
pub const SYNC_IOC_MERGE: u32 = 0xC020_3E01;
/// Sync file info.
pub const SYNC_IOC_FILE_INFO: u32 = 0xC020_3E04;

// ---------------------------------------------------------------------------
// DMA-BUF export flags
// ---------------------------------------------------------------------------

/// Close on exec.
pub const DMA_BUF_CLOEXEC: u32 = 0x80000;
/// Allow map read.
pub const DMA_BUF_RDONLY: u32 = 0x00;
/// Allow map write.
pub const DMA_BUF_WRONLY: u32 = 0x01;
/// Allow map read/write.
pub const DMA_BUF_RDWR: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_read_write() {
        assert_eq!(DMA_BUF_SYNC_RW, DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE);
    }

    #[test]
    fn test_sync_flags_power_of_two() {
        assert!(DMA_BUF_SYNC_READ.is_power_of_two());
        assert!(DMA_BUF_SYNC_WRITE.is_power_of_two());
        assert!(DMA_BUF_SYNC_END.is_power_of_two());
    }

    #[test]
    fn test_sync_start_end() {
        assert_eq!(DMA_BUF_SYNC_START, 0);
        assert_ne!(DMA_BUF_SYNC_START, DMA_BUF_SYNC_END);
    }

    #[test]
    fn test_valid_flags_mask() {
        assert_eq!(DMA_BUF_SYNC_VALID_FLAGS_MASK, DMA_BUF_SYNC_RW | DMA_BUF_SYNC_END);
    }

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [DMA_BUF_IOCTL_SYNC, DMA_BUF_SET_NAME_A, DMA_BUF_SET_NAME_B];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_fence_bits_distinct() {
        let bits = [
            DMA_FENCE_FLAG_SIGNALED_BIT, DMA_FENCE_FLAG_TIMESTAMP_BIT,
            DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT, DMA_FENCE_FLAG_USER_BITS,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_sync_ioc_distinct() {
        assert_ne!(SYNC_IOC_MERGE, SYNC_IOC_FILE_INFO);
    }

    #[test]
    fn test_export_access_distinct() {
        let access = [DMA_BUF_RDONLY, DMA_BUF_WRONLY, DMA_BUF_RDWR];
        for i in 0..access.len() {
            for j in (i + 1)..access.len() {
                assert_ne!(access[i], access[j]);
            }
        }
    }
}
