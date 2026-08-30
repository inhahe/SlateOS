//! `<linux/mm/vmscan.h>` — Page reclaim (kswapd/direct reclaim) constants.
//!
//! When memory is low, the kernel reclaims pages by writing dirty
//! pages to swap/disk and freeing clean pages. kswapd is a per-node
//! daemon that reclaims asynchronously in the background. Direct
//! reclaim happens synchronously when an allocation can't be
//! satisfied. The LRU (Least Recently Used) lists track page access
//! patterns to decide which pages to evict first.

// ---------------------------------------------------------------------------
// LRU list types
// ---------------------------------------------------------------------------

/// Inactive anonymous pages (candidates for swap-out).
pub const LRU_INACTIVE_ANON: u32 = 0;
/// Active anonymous pages (recently used, not swap candidates yet).
pub const LRU_ACTIVE_ANON: u32 = 1;
/// Inactive file pages (candidates for eviction).
pub const LRU_INACTIVE_FILE: u32 = 2;
/// Active file pages (recently used file data).
pub const LRU_ACTIVE_FILE: u32 = 3;
/// Unevictable pages (mlocked, pinned).
pub const LRU_UNEVICTABLE: u32 = 4;
/// Number of LRU lists.
pub const NR_LRU_LISTS: u32 = 5;

// ---------------------------------------------------------------------------
// Reclaim scan control flags
// ---------------------------------------------------------------------------

/// Only scan file-backed pages (not anonymous).
pub const RECLAIM_FILE_ONLY: u32 = 0x01;
/// Only scan anonymous pages (not file-backed).
pub const RECLAIM_ANON_ONLY: u32 = 0x02;
/// Don't trigger writeback (just evict clean pages).
pub const RECLAIM_NO_WRITEBACK: u32 = 0x04;
/// This is a memcg reclaim (not global).
pub const RECLAIM_MEMCG: u32 = 0x08;
/// Reclaim is triggered by compaction (need contiguous pages).
pub const RECLAIM_COMPACTION: u32 = 0x10;

// ---------------------------------------------------------------------------
// Watermark levels (free page thresholds)
// ---------------------------------------------------------------------------

/// Min watermark (below this = emergency, trigger OOM).
pub const WMARK_MIN: u32 = 0;
/// Low watermark (below this = kswapd wakes up).
pub const WMARK_LOW: u32 = 1;
/// High watermark (above this = kswapd sleeps).
pub const WMARK_HIGH: u32 = 2;
/// Promo watermark (above this = NUMA promotion allowed).
pub const WMARK_PROMO: u32 = 3;

// ---------------------------------------------------------------------------
// Reclaim priority (how aggressively to scan)
// ---------------------------------------------------------------------------

/// Default reclaim priority (initial scan fraction).
pub const DEF_PRIORITY: u32 = 12;
/// Maximum reclaim priority (scan everything).
pub const MAX_PRIORITY: u32 = 0;

// ---------------------------------------------------------------------------
// Swappiness range
// ---------------------------------------------------------------------------

/// Minimum swappiness (strongly prefer file pages over anonymous).
pub const SWAPPINESS_MIN: u32 = 0;
/// Default swappiness.
pub const SWAPPINESS_DEFAULT: u32 = 60;
/// Maximum swappiness (treat file and anonymous equally).
pub const SWAPPINESS_MAX: u32 = 200;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_lists_distinct() {
        let lrus = [
            LRU_INACTIVE_ANON,
            LRU_ACTIVE_ANON,
            LRU_INACTIVE_FILE,
            LRU_ACTIVE_FILE,
            LRU_UNEVICTABLE,
        ];
        assert_eq!(lrus.len(), NR_LRU_LISTS as usize);
        for i in 0..lrus.len() {
            for j in (i + 1)..lrus.len() {
                assert_ne!(lrus[i], lrus[j]);
            }
        }
    }

    #[test]
    fn test_reclaim_flags_no_overlap() {
        let flags = [
            RECLAIM_FILE_ONLY,
            RECLAIM_ANON_ONLY,
            RECLAIM_NO_WRITEBACK,
            RECLAIM_MEMCG,
            RECLAIM_COMPACTION,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_watermarks_ordered() {
        assert!(WMARK_MIN < WMARK_LOW);
        assert!(WMARK_LOW < WMARK_HIGH);
    }

    #[test]
    fn test_swappiness_range() {
        assert!(SWAPPINESS_MIN < SWAPPINESS_DEFAULT);
        assert!(SWAPPINESS_DEFAULT < SWAPPINESS_MAX);
    }
}
