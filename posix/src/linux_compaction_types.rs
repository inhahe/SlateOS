//! `<linux/compaction.h>` — Memory compaction constants.
//!
//! Memory compaction moves allocated pages to reduce fragmentation,
//! creating large contiguous free regions needed for huge pages and
//! high-order allocations. It works by scanning from both ends of a
//! zone: a migration scanner finds movable pages, a free scanner
//! finds free pages, and pages are migrated to consolidate free space.
//! Compaction is triggered automatically when high-order allocations
//! fail, or proactively by kcompactd.

// ---------------------------------------------------------------------------
// Compaction results
// ---------------------------------------------------------------------------

/// Compaction not needed (enough free memory already).
pub const COMPACT_NOT_SUITABLE: u32 = 0;
/// Compaction was skipped (zone doesn't need it).
pub const COMPACT_SKIPPED: u32 = 1;
/// Compaction deferred (will retry later).
pub const COMPACT_DEFERRED: u32 = 2;
/// Compaction is continuing (more work to do).
pub const COMPACT_CONTINUE: u32 = 3;
/// Compaction completed successfully (got contiguous memory).
pub const COMPACT_SUCCESS: u32 = 4;
/// Compaction partially completed (some pages moved).
pub const COMPACT_PARTIAL: u32 = 5;
/// Compaction failed (couldn't make progress).
pub const COMPACT_NO_SUITABLE_PAGE: u32 = 6;

// ---------------------------------------------------------------------------
// Compaction priority levels
// ---------------------------------------------------------------------------

/// Normal priority (async, skip if contended).
pub const COMPACT_PRIO_ASYNC: u32 = 0;
/// Sync light (wait for migration but skip locked pages).
pub const COMPACT_PRIO_SYNC_LIGHT: u32 = 1;
/// Sync full (wait for everything, last resort).
pub const COMPACT_PRIO_SYNC_FULL: u32 = 2;

// ---------------------------------------------------------------------------
// Page mobility types (for grouping pages)
// ---------------------------------------------------------------------------

/// Unmovable pages (kernel data, slab objects).
pub const MIGRATE_UNMOVABLE: u32 = 0;
/// Movable pages (user pages, page cache).
pub const MIGRATE_MOVABLE: u32 = 1;
/// Reclaimable pages (can be freed and re-read from disk).
pub const MIGRATE_RECLAIMABLE: u32 = 2;
/// Pages in CMA region (Contiguous Memory Allocator).
pub const MIGRATE_CMA: u32 = 3;
/// Isolate pages (being migrated, don't touch).
pub const MIGRATE_ISOLATE: u32 = 4;
/// Number of migration types.
pub const MIGRATE_TYPES: u32 = 5;

// ---------------------------------------------------------------------------
// Compaction flags
// ---------------------------------------------------------------------------

/// Allow compaction to move pages between NUMA nodes.
pub const COMPACT_FLAG_CROSS_NODE: u32 = 0x01;
/// Compaction is proactive (not triggered by allocation failure).
pub const COMPACT_FLAG_PROACTIVE: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_results_distinct() {
        let results = [
            COMPACT_NOT_SUITABLE,
            COMPACT_SKIPPED,
            COMPACT_DEFERRED,
            COMPACT_CONTINUE,
            COMPACT_SUCCESS,
            COMPACT_PARTIAL,
            COMPACT_NO_SUITABLE_PAGE,
        ];
        for i in 0..results.len() {
            for j in (i + 1)..results.len() {
                assert_ne!(results[i], results[j]);
            }
        }
    }

    #[test]
    fn test_priorities_ordered() {
        assert!(COMPACT_PRIO_ASYNC < COMPACT_PRIO_SYNC_LIGHT);
        assert!(COMPACT_PRIO_SYNC_LIGHT < COMPACT_PRIO_SYNC_FULL);
    }

    #[test]
    fn test_migrate_types_distinct() {
        let types = [
            MIGRATE_UNMOVABLE,
            MIGRATE_MOVABLE,
            MIGRATE_RECLAIMABLE,
            MIGRATE_CMA,
            MIGRATE_ISOLATE,
        ];
        assert_eq!(types.len(), MIGRATE_TYPES as usize);
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(COMPACT_FLAG_CROSS_NODE & COMPACT_FLAG_PROACTIVE, 0);
    }
}
