//! `<linux/bcache.h>` — Additional bcache constants.
//!
//! Supplementary bcache constants covering cache modes,
//! states, superblock fields, and feature flags.

// ---------------------------------------------------------------------------
// Bcache cache modes
// ---------------------------------------------------------------------------

/// Write-through mode.
pub const CACHE_MODE_WRITETHROUGH: u32 = 0;
/// Write-back mode.
pub const CACHE_MODE_WRITEBACK: u32 = 1;
/// Write-around mode.
pub const CACHE_MODE_WRITEAROUND: u32 = 2;
/// No cache (passthrough).
pub const CACHE_MODE_NONE: u32 = 3;

// ---------------------------------------------------------------------------
// Bcache states
// ---------------------------------------------------------------------------

/// Cache is active.
pub const BDEV_STATE_NONE: u32 = 0;
/// Clean state.
pub const BDEV_STATE_CLEAN: u32 = 1;
/// Dirty state.
pub const BDEV_STATE_DIRTY: u32 = 2;
/// Stale state.
pub const BDEV_STATE_STALE: u32 = 3;

// ---------------------------------------------------------------------------
// Bcache superblock magic
// ---------------------------------------------------------------------------

/// Bcache superblock magic.
pub const BCACHE_SB_MAGIC: u64 = 0xc68573f6_ce4a9034;
/// Bcache journal magic.
pub const JSET_MAGIC: u64 = 0x245e_2d53_a189_0732;
/// Bcache btree node magic.
pub const BSET_MAGIC: u64 = 0x9065_21a4_e183_7c0e;

// ---------------------------------------------------------------------------
// Bcache superblock fields
// ---------------------------------------------------------------------------

/// Superblock version: backing device.
pub const BCACHE_SB_VERSION_BDEV: u32 = 1;
/// Superblock version: cache device.
pub const BCACHE_SB_VERSION_CDEV: u32 = 3;
/// Superblock version: backing device with data offset.
pub const BCACHE_SB_VERSION_BDEV_WITH_OFFSET: u32 = 4;
/// Superblock version: cache device with UUID.
pub const BCACHE_SB_VERSION_CDEV_WITH_UUID: u32 = 3;

// ---------------------------------------------------------------------------
// Bcache btree node sizes
// ---------------------------------------------------------------------------

/// Minimum bucket size (in sectors).
pub const BCACHE_MIN_BUCKET_SIZE: u32 = 128;
/// Maximum bucket size (in sectors).
pub const BCACHE_MAX_BUCKET_SIZE: u32 = 1 << 20;
/// Superblock size (in sectors).
pub const BCACHE_SB_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Bcache cache replacement policies
// ---------------------------------------------------------------------------

/// LRU replacement.
pub const CACHE_REPLACEMENT_LRU: u32 = 0;
/// FIFO replacement.
pub const CACHE_REPLACEMENT_FIFO: u32 = 1;
/// Random replacement.
pub const CACHE_REPLACEMENT_RANDOM: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_modes_distinct() {
        let modes = [
            CACHE_MODE_WRITETHROUGH,
            CACHE_MODE_WRITEBACK,
            CACHE_MODE_WRITEAROUND,
            CACHE_MODE_NONE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_bdev_states_distinct() {
        let states = [
            BDEV_STATE_NONE,
            BDEV_STATE_CLEAN,
            BDEV_STATE_DIRTY,
            BDEV_STATE_STALE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_magics_distinct() {
        let magics = [BCACHE_SB_MAGIC, JSET_MAGIC, BSET_MAGIC];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_bucket_sizes() {
        assert_eq!(BCACHE_MIN_BUCKET_SIZE, 128);
        assert!(BCACHE_MIN_BUCKET_SIZE < BCACHE_MAX_BUCKET_SIZE);
        assert!(BCACHE_MAX_BUCKET_SIZE.is_power_of_two());
    }

    #[test]
    fn test_replacement_policies_distinct() {
        let policies = [
            CACHE_REPLACEMENT_LRU,
            CACHE_REPLACEMENT_FIFO,
            CACHE_REPLACEMENT_RANDOM,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }
}
