//! `<linux/migrate.h>` — Page migration constants.
//!
//! Page migration moves physical pages between NUMA nodes or
//! memory zones while keeping virtual addresses stable. Used by
//! NUMA balancing, memory hotplug, compaction, and the
//! move_pages() syscall.

// ---------------------------------------------------------------------------
// Migration mode
// ---------------------------------------------------------------------------

/// Asynchronous migration (may fail, non-blocking).
pub const MIGRATE_ASYNC: u32 = 0;
/// Synchronous migration (light, may block briefly).
pub const MIGRATE_SYNC_LIGHT: u32 = 1;
/// Synchronous migration (full, blocks as needed).
pub const MIGRATE_SYNC: u32 = 2;
/// Synchronous, no copy (for NUMA balancing).
pub const MIGRATE_SYNC_NO_COPY: u32 = 3;

// ---------------------------------------------------------------------------
// Migration reason
// ---------------------------------------------------------------------------

/// Compaction.
pub const MR_COMPACTION: u32 = 0;
/// Memory hotplug offline.
pub const MR_MEMORY_HOTPLUG: u32 = 1;
/// Memory failure (poison).
pub const MR_MEMORY_FAILURE: u32 = 2;
/// syscall (move_pages, migrate_pages).
pub const MR_SYSCALL: u32 = 3;
/// mempolicy change.
pub const MR_MEMPOLICY_MBIND: u32 = 4;
/// NUMA misplaced page.
pub const MR_NUMA_MISPLACED: u32 = 5;
/// CMA allocation.
pub const MR_CONTIG_RANGE: u32 = 6;
/// Longterm pinning.
pub const MR_LONGTERM_PIN: u32 = 7;
/// Demotion to slow memory.
pub const MR_DEMOTION: u32 = 8;

// ---------------------------------------------------------------------------
// Move pages status codes (returned per-page by move_pages)
// ---------------------------------------------------------------------------

/// Page not present.
pub const MPAGES_STATUS_NOT_PRESENT: i32 = -2;
/// Page is in required node already.
pub const MPAGES_STATUS_SAME_NODE: i32 = -3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            MIGRATE_ASYNC, MIGRATE_SYNC_LIGHT,
            MIGRATE_SYNC, MIGRATE_SYNC_NO_COPY,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_reasons_distinct() {
        let reasons = [
            MR_COMPACTION, MR_MEMORY_HOTPLUG, MR_MEMORY_FAILURE,
            MR_SYSCALL, MR_MEMPOLICY_MBIND, MR_NUMA_MISPLACED,
            MR_CONTIG_RANGE, MR_LONGTERM_PIN, MR_DEMOTION,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_status_codes() {
        assert_ne!(MPAGES_STATUS_NOT_PRESENT, MPAGES_STATUS_SAME_NODE);
        assert!(MPAGES_STATUS_NOT_PRESENT < 0);
        assert!(MPAGES_STATUS_SAME_NODE < 0);
    }
}
