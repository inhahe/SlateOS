//! `<linux/ksm.h>` — Kernel Same-page Merging (KSM) constants.
//!
//! KSM scans memory for pages with identical content and merges
//! them into a single copy-on-write page. This saves memory when
//! running many similar processes (VMs with same guest OS, containers
//! with shared base images). KSM is opt-in via madvise(MADV_MERGEABLE)
//! on memory regions that are likely to share content. The ksmd daemon
//! periodically scans registered regions, computes page hashes, and
//! merges identical pages.

// ---------------------------------------------------------------------------
// KSM madvise flags
// ---------------------------------------------------------------------------

/// Mark region as mergeable (opt-in to KSM scanning).
pub const MADV_MERGEABLE: u32 = 12;
/// Mark region as not mergeable (opt-out of KSM).
pub const MADV_UNMERGEABLE: u32 = 13;

// ---------------------------------------------------------------------------
// KSM page states
// ---------------------------------------------------------------------------

/// Page is not tracked by KSM.
pub const KSM_PAGE_NONE: u32 = 0;
/// Page is being scanned (candidate for merge).
pub const KSM_PAGE_CANDIDATE: u32 = 1;
/// Page is merged (CoW shared with other processes).
pub const KSM_PAGE_SHARED: u32 = 2;
/// Page was merged but has been written (unshared, CoW triggered).
pub const KSM_PAGE_UNSHARED: u32 = 3;

// ---------------------------------------------------------------------------
// KSM run modes (controlled via /sys/kernel/mm/ksm/run)
// ---------------------------------------------------------------------------

/// KSM is stopped (not scanning).
pub const KSM_RUN_STOP: u32 = 0;
/// KSM is running (scanning and merging).
pub const KSM_RUN_RUN: u32 = 1;
/// KSM is unmerging (splitting all shared pages).
pub const KSM_RUN_UNMERGE: u32 = 2;

// ---------------------------------------------------------------------------
// KSM scan control parameters (defaults)
// ---------------------------------------------------------------------------

/// Default pages to scan per sleep interval.
pub const KSM_PAGES_TO_SCAN_DEFAULT: u32 = 100;
/// Default sleep interval between scans (ms).
pub const KSM_SLEEP_MS_DEFAULT: u32 = 20;

// ---------------------------------------------------------------------------
// KSM advisor modes (automatic tuning, 6.7+)
// ---------------------------------------------------------------------------

/// No advisor (manual tuning).
pub const KSM_ADVISOR_NONE: u32 = 0;
/// Scan-time advisor (auto-adjusts pages_to_scan).
pub const KSM_ADVISOR_SCAN_TIME: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_madvise_values_distinct() {
        assert_ne!(MADV_MERGEABLE, MADV_UNMERGEABLE);
    }

    #[test]
    fn test_page_states_distinct() {
        let states = [
            KSM_PAGE_NONE,
            KSM_PAGE_CANDIDATE,
            KSM_PAGE_SHARED,
            KSM_PAGE_UNSHARED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
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
    fn test_defaults_positive() {
        assert!(KSM_PAGES_TO_SCAN_DEFAULT > 0);
        assert!(KSM_SLEEP_MS_DEFAULT > 0);
    }

    #[test]
    fn test_advisor_modes_distinct() {
        assert_ne!(KSM_ADVISOR_NONE, KSM_ADVISOR_SCAN_TIME);
    }
}
