//! `<linux/ksm.h>` — Kernel Samepage Merging (KSM) constants.
//!
//! KSM scans anonymous pages for identical content and merges
//! them copy-on-write, reducing memory usage for workloads with
//! many duplicate pages (e.g., virtual machines, containers).

// ---------------------------------------------------------------------------
// KSM madvise flags
// ---------------------------------------------------------------------------

/// Enable KSM merging for this region (via madvise).
pub const MADV_MERGEABLE: u32 = 12;
/// Disable KSM merging for this region (via madvise).
pub const MADV_UNMERGEABLE: u32 = 13;

// ---------------------------------------------------------------------------
// KSM modes (/sys/kernel/mm/ksm/run)
// ---------------------------------------------------------------------------

/// KSM stopped (not scanning).
pub const KSM_RUN_STOP: u32 = 0;
/// KSM running (actively scanning and merging).
pub const KSM_RUN_RUN: u32 = 1;
/// KSM unmerging all pages then stopping.
pub const KSM_RUN_UNMERGE: u32 = 2;

// ---------------------------------------------------------------------------
// KSM tunables (default values)
// ---------------------------------------------------------------------------

/// Default sleep between scans (milliseconds).
pub const KSM_SLEEP_DEFAULT: u32 = 20;
/// Default pages to scan per pass.
pub const KSM_PAGES_TO_SCAN_DEFAULT: u32 = 100;
/// Maximum KSM scan batch size.
pub const KSM_MAX_SCAN_BATCH: u32 = 4096;

// ---------------------------------------------------------------------------
// KSM process-level control (prctl)
// ---------------------------------------------------------------------------

/// Enable KSM for all compatible VMAs in this process.
pub const PR_SET_MEMORY_MERGE: u32 = 67;
/// Query whether KSM is enabled for this process.
pub const PR_GET_MEMORY_MERGE: u32 = 68;

// ---------------------------------------------------------------------------
// KSM page flags (for /proc/pid/pagemap)
// ---------------------------------------------------------------------------

/// Page is KSM-merged (shared CoW).
pub const KSM_PAGE_FLAG_KSM: u64 = 1 << 21;

// ---------------------------------------------------------------------------
// KSM advisor modes
// ---------------------------------------------------------------------------

/// No advisor (use fixed tunables).
pub const KSM_ADVISOR_NONE: u32 = 0;
/// Scan-time advisor (auto-tune pages_to_scan).
pub const KSM_ADVISOR_SCAN_TIME: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_madvise_flags_distinct() {
        assert_ne!(MADV_MERGEABLE, MADV_UNMERGEABLE);
    }

    #[test]
    fn test_madvise_values() {
        assert_eq!(MADV_MERGEABLE, 12);
        assert_eq!(MADV_UNMERGEABLE, 13);
    }

    #[test]
    fn test_run_modes_distinct() {
        let modes = [KSM_RUN_STOP, KSM_RUN_RUN, KSM_RUN_UNMERGE];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_run_stop_is_zero() {
        assert_eq!(KSM_RUN_STOP, 0);
    }

    #[test]
    fn test_prctl_ops_distinct() {
        assert_ne!(PR_SET_MEMORY_MERGE, PR_GET_MEMORY_MERGE);
    }

    #[test]
    fn test_ksm_page_flag() {
        assert!(KSM_PAGE_FLAG_KSM.is_power_of_two());
    }

    #[test]
    fn test_advisor_modes_distinct() {
        assert_ne!(KSM_ADVISOR_NONE, KSM_ADVISOR_SCAN_TIME);
    }

    #[test]
    fn test_sleep_default() {
        assert_eq!(KSM_SLEEP_DEFAULT, 20);
    }

    #[test]
    fn test_pages_to_scan_default() {
        assert_eq!(KSM_PAGES_TO_SCAN_DEFAULT, 100);
    }
}
