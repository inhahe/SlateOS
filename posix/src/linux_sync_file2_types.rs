//! `<linux/sync_file.h>` — Additional sync file/fence constants.
//!
//! Supplementary sync file constants covering fence flags,
//! status values, and ioctl commands.

// ---------------------------------------------------------------------------
// Fence flags
// ---------------------------------------------------------------------------

/// Fence signaled.
pub const DMA_FENCE_FLAG_SIGNALED_BIT: u32 = 0;
/// Fence timestamp enabled.
pub const DMA_FENCE_FLAG_TIMESTAMP_BIT: u32 = 1;
/// Fence enable signal on CPU.
pub const DMA_FENCE_FLAG_ENABLE_SIGNAL_BIT: u32 = 2;
/// User bits start here.
pub const DMA_FENCE_FLAG_USER_BITS: u32 = 3;

// ---------------------------------------------------------------------------
// Sync file status values
// ---------------------------------------------------------------------------

/// Active (not yet signaled).
pub const SYNC_FILE_STATUS_ACTIVE: i32 = 0;
/// Signaled successfully.
pub const SYNC_FILE_STATUS_SIGNALED: i32 = 1;
/// Error.
pub const SYNC_FILE_STATUS_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Sync file ioctl commands
// ---------------------------------------------------------------------------

/// Merge two sync files.
pub const SYNC_IOC_MERGE: u32 = 0x40083E01;
/// Get fence info.
pub const SYNC_IOC_FILE_INFO: u32 = 0xC0203E04;

// ---------------------------------------------------------------------------
// Sync file fence info flags
// ---------------------------------------------------------------------------

/// Fence is from timeline.
pub const SYNC_FENCE_FLAG_TIMELINE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fence_flag_bits_distinct() {
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
    fn test_status_values_distinct() {
        let statuses = [
            SYNC_FILE_STATUS_ACTIVE,
            SYNC_FILE_STATUS_SIGNALED,
            SYNC_FILE_STATUS_ERROR,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_cmds_distinct() {
        assert_ne!(SYNC_IOC_MERGE, SYNC_IOC_FILE_INFO);
    }

    #[test]
    fn test_timeline_flag() {
        assert!(SYNC_FENCE_FLAG_TIMELINE.is_power_of_two());
    }
}
