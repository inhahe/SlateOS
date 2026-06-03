//! `<linux/cgroup/hugetlb.h>` — HugeTLB cgroup controller constants.
//!
//! The HugeTLB controller limits the amount of huge page memory
//! that can be allocated by processes within a cgroup. Limits
//! are set per huge page size (e.g., 2MB, 1GB on x86_64).

// ---------------------------------------------------------------------------
// Cgroup v2 interface file suffixes
// ---------------------------------------------------------------------------

/// Maximum huge page usage allowed (e.g., "hugetlb.2MB.max").
pub const HUGETLB_MAX_SUFFIX: &str = "max";
/// Current huge page usage.
pub const HUGETLB_CURRENT_SUFFIX: &str = "current";
/// Reserved huge page count.
pub const HUGETLB_RSV_MAX_SUFFIX: &str = "rsvd.max";
/// Reserved current usage.
pub const HUGETLB_RSV_CURRENT_SUFFIX: &str = "rsvd.current";
/// Events file.
pub const HUGETLB_EVENTS_SUFFIX: &str = "events";
/// Events (local).
pub const HUGETLB_EVENTS_LOCAL_SUFFIX: &str = "events.local";
/// NUMA statistics.
pub const HUGETLB_NUMA_STAT_SUFFIX: &str = "numa_stat";

// ---------------------------------------------------------------------------
// Controller prefix
// ---------------------------------------------------------------------------

/// Cgroup v2 controller name.
pub const HUGETLB_CONTROLLER: &str = "hugetlb";

// ---------------------------------------------------------------------------
// Common huge page size strings (x86_64)
// ---------------------------------------------------------------------------

/// 2 MB huge pages.
pub const HUGETLB_SIZE_2MB: &str = "2MB";
/// 1 GB huge pages.
pub const HUGETLB_SIZE_1GB: &str = "1GB";

// ---------------------------------------------------------------------------
// Event names
// ---------------------------------------------------------------------------

/// Event: allocation was rejected due to limit.
pub const HUGETLB_EVENT_MAX: &str = "max";
/// Event: reserved pages limit hit.
pub const HUGETLB_EVENT_RSV_MAX: &str = "rsvd.max";

// ---------------------------------------------------------------------------
// Special values
// ---------------------------------------------------------------------------

/// Unlimited (written as "max").
pub const HUGETLB_MAX_STR: &str = "max";

// ---------------------------------------------------------------------------
// Huge page sizes in bytes
// ---------------------------------------------------------------------------

/// 2 MB in bytes.
pub const HUGETLB_2MB_BYTES: u64 = 2 * 1024 * 1024;
/// 1 GB in bytes.
pub const HUGETLB_1GB_BYTES: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suffixes_distinct() {
        let suffixes = [
            HUGETLB_MAX_SUFFIX,
            HUGETLB_CURRENT_SUFFIX,
            HUGETLB_RSV_MAX_SUFFIX,
            HUGETLB_RSV_CURRENT_SUFFIX,
            HUGETLB_EVENTS_SUFFIX,
            HUGETLB_EVENTS_LOCAL_SUFFIX,
            HUGETLB_NUMA_STAT_SUFFIX,
        ];
        for i in 0..suffixes.len() {
            for j in (i + 1)..suffixes.len() {
                assert_ne!(suffixes[i], suffixes[j]);
            }
        }
    }

    #[test]
    fn test_size_strings_distinct() {
        assert_ne!(HUGETLB_SIZE_2MB, HUGETLB_SIZE_1GB);
    }

    #[test]
    fn test_event_names_distinct() {
        assert_ne!(HUGETLB_EVENT_MAX, HUGETLB_EVENT_RSV_MAX);
    }

    #[test]
    fn test_page_sizes_bytes() {
        assert_eq!(HUGETLB_2MB_BYTES, 2 * 1024 * 1024);
        assert_eq!(HUGETLB_1GB_BYTES, 1024 * 1024 * 1024);
        assert!(HUGETLB_2MB_BYTES < HUGETLB_1GB_BYTES);
    }

    #[test]
    fn test_page_sizes_power_of_two() {
        assert!(HUGETLB_2MB_BYTES.is_power_of_two());
        assert!(HUGETLB_1GB_BYTES.is_power_of_two());
    }

    #[test]
    fn test_controller_name() {
        assert_eq!(HUGETLB_CONTROLLER, "hugetlb");
    }
}
