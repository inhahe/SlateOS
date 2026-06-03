//! `<linux/bcache.h>` — Block cache (bcache) constants.
//!
//! Bcache uses fast storage (SSD) as a cache for slow storage (HDD).
//! It supports writeback, writethrough, and writearound caching
//! policies, with automatic cache management and tiered storage.

// ---------------------------------------------------------------------------
// Cache modes
// ---------------------------------------------------------------------------

/// Writethrough (safe, writes go to backing device immediately).
pub const BCACHE_MODE_WRITETHROUGH: u8 = 0;
/// Writeback (fast, writes cached on SSD first).
pub const BCACHE_MODE_WRITEBACK: u8 = 1;
/// Writearound (writes bypass cache, only reads cached).
pub const BCACHE_MODE_WRITEAROUND: u8 = 2;
/// None (cache disabled, passthrough).
pub const BCACHE_MODE_NONE: u8 = 3;

// ---------------------------------------------------------------------------
// Cache states
// ---------------------------------------------------------------------------

/// Cache device not attached.
pub const BCACHE_STATE_DETACHED: u8 = 0;
/// Cache clean (no dirty data).
pub const BCACHE_STATE_CLEAN: u8 = 1;
/// Cache has dirty data.
pub const BCACHE_STATE_DIRTY: u8 = 2;
/// Cache inconsistent (needs recovery).
pub const BCACHE_STATE_INCONSISTENT: u8 = 3;

// ---------------------------------------------------------------------------
// Cache replacement policies
// ---------------------------------------------------------------------------

/// Least Recently Used.
pub const BCACHE_REPL_LRU: u8 = 0;
/// First In First Out.
pub const BCACHE_REPL_FIFO: u8 = 1;
/// Random replacement.
pub const BCACHE_REPL_RANDOM: u8 = 2;

// ---------------------------------------------------------------------------
// Superblock magic and version
// ---------------------------------------------------------------------------

/// Bcache superblock magic (first 8 bytes).
pub const BCACHE_SB_MAGIC: u64 = 0xC68573F6_4E616963;
/// Cache superblock version.
pub const BCACHE_SB_VERSION_CACHE: u32 = 3;
/// Backing superblock version.
pub const BCACHE_SB_VERSION_BDEV: u32 = 1;

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Large bucket support.
pub const BCACHE_FEAT_LARGE_BUCKET: u32 = 1 << 0;
/// Obso bucket size (obsolete).
pub const BCACHE_FEAT_OBSO_LARGE_BUCKET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_modes_distinct() {
        let modes = [
            BCACHE_MODE_WRITETHROUGH,
            BCACHE_MODE_WRITEBACK,
            BCACHE_MODE_WRITEAROUND,
            BCACHE_MODE_NONE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            BCACHE_STATE_DETACHED,
            BCACHE_STATE_CLEAN,
            BCACHE_STATE_DIRTY,
            BCACHE_STATE_INCONSISTENT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_replacement_policies_distinct() {
        let policies = [BCACHE_REPL_LRU, BCACHE_REPL_FIFO, BCACHE_REPL_RANDOM];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_sb_versions_distinct() {
        assert_ne!(BCACHE_SB_VERSION_CACHE, BCACHE_SB_VERSION_BDEV);
    }
}
