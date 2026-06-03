//! `<linux/migrate_mode.h>` — Page migration mode and reason constants.
//!
//! Page migration moves physical pages between NUMA nodes or
//! between different memory types (e.g., regular RAM to persistent
//! memory). These constants define migration modes and reasons.

// ---------------------------------------------------------------------------
// Migration modes
// ---------------------------------------------------------------------------

/// Asynchronous migration (non-blocking).
pub const MIGRATE_ASYNC: u32 = 0;
/// Synchronous migration (may block).
pub const MIGRATE_SYNC_LIGHT: u32 = 1;
/// Full synchronous migration (waits for writeback).
pub const MIGRATE_SYNC: u32 = 2;
/// Synchronous but without dirty page writeback.
pub const MIGRATE_SYNC_NO_COPY: u32 = 3;

// ---------------------------------------------------------------------------
// Migration reasons (for tracing/statistics)
// ---------------------------------------------------------------------------

/// Compaction-triggered migration.
pub const MR_COMPACTION: u32 = 0;
/// NUMA balancing auto-migration.
pub const MR_NUMA_MISPLACED: u32 = 1;
/// Memory policy change (mbind/set_mempolicy).
pub const MR_MEMPOLICY_MBIND: u32 = 2;
/// Syscall-initiated migration (move_pages).
pub const MR_SYSCALL: u32 = 3;
/// CMA allocation migration.
pub const MR_CMA: u32 = 4;
/// Memory hotplug offline migration.
pub const MR_MEMORY_HOTPLUG: u32 = 5;
/// Memory failure migration.
pub const MR_MEMORY_FAILURE: u32 = 6;
/// Longterm pinning migration.
pub const MR_LONGTERM_PIN: u32 = 7;
/// Demotion to slow memory tier.
pub const MR_DEMOTION: u32 = 8;
/// Promotion to fast memory tier.
pub const MR_PROMOTION: u32 = 9;

// ---------------------------------------------------------------------------
// move_pages flags
// ---------------------------------------------------------------------------

/// Move all pages (even if not on expected node).
pub const MPOL_MF_MOVE_FLAG: u32 = 1 << 1;
/// Move all users' pages (requires privilege).
pub const MPOL_MF_MOVE_ALL_FLAG: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Migration result status codes (per-page in move_pages status array)
// ---------------------------------------------------------------------------

/// Page is on the requested node (success).
pub const MIGRATE_STATUS_OK: i32 = 0;
/// Page is busy and could not be moved.
pub const MIGRATE_STATUS_EBUSY: i32 = -16;
/// Invalid page or address.
pub const MIGRATE_STATUS_EFAULT: i32 = -14;
/// Operation not permitted.
pub const MIGRATE_STATUS_EPERM: i32 = -1;
/// No memory available on target node.
pub const MIGRATE_STATUS_ENOMEM: i32 = -12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            MIGRATE_ASYNC,
            MIGRATE_SYNC_LIGHT,
            MIGRATE_SYNC,
            MIGRATE_SYNC_NO_COPY,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_async_is_zero() {
        assert_eq!(MIGRATE_ASYNC, 0);
    }

    #[test]
    fn test_reasons_distinct() {
        let reasons = [
            MR_COMPACTION,
            MR_NUMA_MISPLACED,
            MR_MEMPOLICY_MBIND,
            MR_SYSCALL,
            MR_CMA,
            MR_MEMORY_HOTPLUG,
            MR_MEMORY_FAILURE,
            MR_LONGTERM_PIN,
            MR_DEMOTION,
            MR_PROMOTION,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_status_ok() {
        assert_eq!(MIGRATE_STATUS_OK, 0);
    }

    #[test]
    fn test_status_errors_negative() {
        assert!(MIGRATE_STATUS_EBUSY < 0);
        assert!(MIGRATE_STATUS_EFAULT < 0);
        assert!(MIGRATE_STATUS_EPERM < 0);
        assert!(MIGRATE_STATUS_ENOMEM < 0);
    }

    #[test]
    fn test_move_flags_no_overlap() {
        assert_eq!(MPOL_MF_MOVE_FLAG & MPOL_MF_MOVE_ALL_FLAG, 0);
    }
}
