//! `<linux/bcache.h>` — bcache (block cache) constants.
//!
//! bcache provides SSD caching for slower block devices.
//! These constants define cache modes, superblock fields,
//! and state values.

// ---------------------------------------------------------------------------
// Cache modes (CACHE_MODE_*)
// ---------------------------------------------------------------------------

/// Writeback caching.
pub const CACHE_MODE_WRITETHROUGH: u32 = 0;
/// Writeback caching.
pub const CACHE_MODE_WRITEBACK: u32 = 1;
/// Writearound caching.
pub const CACHE_MODE_WRITEAROUND: u32 = 2;
/// No caching.
pub const CACHE_MODE_NONE: u32 = 3;

// ---------------------------------------------------------------------------
// Superblock magic
// ---------------------------------------------------------------------------

/// bcache superblock magic (offset 0).
pub const BCACHE_SB_MAGIC: u64 = 0xC68573F6_4E616963;
/// bcache label size.
pub const BCACHE_SB_LABEL_SIZE: u32 = 32;
/// bcache max csum size.
pub const BCACHE_SB_CSUM_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Superblock version
// ---------------------------------------------------------------------------

/// Backing device (v0).
pub const BCACHE_SB_VERSION_BDEV: u32 = 0;
/// Cache device (v0).
pub const BCACHE_SB_VERSION_CDEV: u32 = 1;
/// Backing device with data offset.
pub const BCACHE_SB_VERSION_BDEV_WITH_OFFSET: u32 = 2;
/// Backing device with features.
pub const BCACHE_SB_VERSION_BDEV_WITH_FEATURES: u32 = 3;
/// Cache device with features.
pub const BCACHE_SB_VERSION_CDEV_WITH_FEATURES: u32 = 4;

// ---------------------------------------------------------------------------
// Superblock flags
// ---------------------------------------------------------------------------

/// Block size in sectors.
pub const BCACHE_SB_BLOCK_SIZE_SHIFT: u32 = 0;
/// Block size mask (bits 0-7).
pub const BCACHE_SB_BLOCK_SIZE_MASK: u32 = 0xFF;
/// Bucket size in sectors.
pub const BCACHE_SB_BUCKET_SIZE_SHIFT: u32 = 8;

// ---------------------------------------------------------------------------
// Cache state (BDEV_STATE_*)
// ---------------------------------------------------------------------------

/// No state / not attached.
pub const BDEV_STATE_NONE: u32 = 0;
/// Clean state.
pub const BDEV_STATE_CLEAN: u32 = 1;
/// Dirty state.
pub const BDEV_STATE_DIRTY: u32 = 2;
/// Stale state.
pub const BDEV_STATE_STALE: u32 = 3;

// ---------------------------------------------------------------------------
// Cache replacement policies
// ---------------------------------------------------------------------------

/// Least recently used.
pub const CACHE_REPLACEMENT_LRU: u32 = 0;
/// First in first out.
pub const CACHE_REPLACEMENT_FIFO: u32 = 1;
/// Random.
pub const CACHE_REPLACEMENT_RANDOM: u32 = 2;

// ---------------------------------------------------------------------------
// Priority / GC
// ---------------------------------------------------------------------------

/// Minimum priority.
pub const BCACHE_PRIO_MIN: u16 = 0;
/// Maximum priority.
pub const BCACHE_PRIO_MAX: u16 = 0xFFFF;
/// Default sectors per bucket.
pub const BCACHE_BUCKET_SIZE_DEFAULT: u32 = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_modes_sequential() {
        assert_eq!(CACHE_MODE_WRITETHROUGH, 0);
        assert_eq!(CACHE_MODE_WRITEBACK, 1);
        assert_eq!(CACHE_MODE_WRITEAROUND, 2);
        assert_eq!(CACHE_MODE_NONE, 3);
    }

    #[test]
    fn test_sb_magic() {
        assert_eq!(BCACHE_SB_MAGIC, 0xC68573F6_4E616963);
    }

    #[test]
    fn test_sb_versions_distinct() {
        let versions = [
            BCACHE_SB_VERSION_BDEV, BCACHE_SB_VERSION_CDEV,
            BCACHE_SB_VERSION_BDEV_WITH_OFFSET,
            BCACHE_SB_VERSION_BDEV_WITH_FEATURES,
            BCACHE_SB_VERSION_CDEV_WITH_FEATURES,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_bdev_states_sequential() {
        assert_eq!(BDEV_STATE_NONE, 0);
        assert_eq!(BDEV_STATE_CLEAN, 1);
        assert_eq!(BDEV_STATE_DIRTY, 2);
        assert_eq!(BDEV_STATE_STALE, 3);
    }

    #[test]
    fn test_replacement_policies_sequential() {
        assert_eq!(CACHE_REPLACEMENT_LRU, 0);
        assert_eq!(CACHE_REPLACEMENT_FIFO, 1);
        assert_eq!(CACHE_REPLACEMENT_RANDOM, 2);
    }

    #[test]
    fn test_prio_range() {
        assert_eq!(BCACHE_PRIO_MIN, 0);
        assert_eq!(BCACHE_PRIO_MAX, 0xFFFF);
        assert!(BCACHE_PRIO_MIN < BCACHE_PRIO_MAX);
    }

    #[test]
    fn test_label_size() {
        assert_eq!(BCACHE_SB_LABEL_SIZE, 32);
    }

    #[test]
    fn test_block_size_mask() {
        assert_eq!(BCACHE_SB_BLOCK_SIZE_MASK, 0xFF);
    }
}
