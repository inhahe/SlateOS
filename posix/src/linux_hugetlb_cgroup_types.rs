//! `<linux/hugetlb_cgroup.h>` — HugeTLB cgroup controller constants.
//!
//! The hugetlb cgroup controller limits the amount of huge page
//! memory that processes in a cgroup can allocate. Huge pages (2 MiB
//! or 1 GiB on x86_64) are a limited resource that must be explicitly
//! reserved; without cgroup limits, one container could consume all
//! available huge pages, starving others. The controller tracks usage
//! per huge page size and enforces reservation limits.

// ---------------------------------------------------------------------------
// Huge page sizes (x86_64)
// ---------------------------------------------------------------------------

/// 2 MiB huge page size.
pub const HUGETLB_PAGE_SIZE_2MB: u32 = 2 * 1024 * 1024;
/// 1 GiB huge page size.
pub const HUGETLB_PAGE_SIZE_1GB: u32 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// HugeTLB cgroup limit types
// ---------------------------------------------------------------------------

/// Limit on committed (reserved + allocated) huge pages.
pub const HUGETLB_LIMIT_HARD: u32 = 0;
/// Reservation limit (how many can be reserved).
pub const HUGETLB_LIMIT_RSVD: u32 = 1;

// ---------------------------------------------------------------------------
// HugeTLB cgroup events
// ---------------------------------------------------------------------------

/// Hard limit was hit (allocation denied).
pub const HUGETLB_EVENT_MAX: u32 = 0;
/// Reservation limit was hit.
pub const HUGETLB_EVENT_RSVD_MAX: u32 = 1;

// ---------------------------------------------------------------------------
// HugeTLB cgroup stat types
// ---------------------------------------------------------------------------

/// Current usage in bytes.
pub const HUGETLB_STAT_CURRENT: u32 = 0;
/// Current reserved bytes.
pub const HUGETLB_STAT_RSVD: u32 = 1;
/// Maximum usage seen (high watermark).
pub const HUGETLB_STAT_MAX_USAGE: u32 = 2;
/// Number of allocation failures.
pub const HUGETLB_STAT_FAILCNT: u32 = 3;

// ---------------------------------------------------------------------------
// HugeTLB unlimited value
// ---------------------------------------------------------------------------

/// No limit (unlimited huge page allocation).
pub const HUGETLB_NO_LIMIT: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_sizes() {
        assert_eq!(HUGETLB_PAGE_SIZE_2MB, 2_097_152);
        assert_eq!(HUGETLB_PAGE_SIZE_1GB, 1_073_741_824);
        assert!(HUGETLB_PAGE_SIZE_1GB > HUGETLB_PAGE_SIZE_2MB);
    }

    #[test]
    fn test_limit_types_distinct() {
        assert_ne!(HUGETLB_LIMIT_HARD, HUGETLB_LIMIT_RSVD);
    }

    #[test]
    fn test_events_distinct() {
        assert_ne!(HUGETLB_EVENT_MAX, HUGETLB_EVENT_RSVD_MAX);
    }

    #[test]
    fn test_stat_types_distinct() {
        let stats = [
            HUGETLB_STAT_CURRENT, HUGETLB_STAT_RSVD,
            HUGETLB_STAT_MAX_USAGE, HUGETLB_STAT_FAILCNT,
        ];
        for i in 0..stats.len() {
            for j in (i + 1)..stats.len() {
                assert_ne!(stats[i], stats[j]);
            }
        }
    }

    #[test]
    fn test_no_limit_is_max() {
        assert_eq!(HUGETLB_NO_LIMIT, u64::MAX);
    }
}
