//! Transparent Huge Pages — sysfs knobs and `madvise(2)` hints.
//!
//! THP lets the kernel back anonymous mappings (and tmpfs/shmem) with
//! 2 MiB pages instead of 4 KiB, cutting TLB misses for large
//! workloads. The policy is selected per-mapping by `MADV_HUGEPAGE`
//! and globally by `/sys/kernel/mm/transparent_hugepage/enabled`.

// ---------------------------------------------------------------------------
// `madvise(2)` hints related to THP
// ---------------------------------------------------------------------------

pub const MADV_HUGEPAGE: u32 = 14;
pub const MADV_NOHUGEPAGE: u32 = 15;
pub const MADV_COLLAPSE: u32 = 25;

// ---------------------------------------------------------------------------
// sysfs control paths
// ---------------------------------------------------------------------------

pub const SYS_THP_ROOT: &str = "/sys/kernel/mm/transparent_hugepage";
pub const SYS_THP_ENABLED: &str = "/sys/kernel/mm/transparent_hugepage/enabled";
pub const SYS_THP_DEFRAG: &str = "/sys/kernel/mm/transparent_hugepage/defrag";
pub const SYS_THP_USE_ZERO_PAGE: &str = "/sys/kernel/mm/transparent_hugepage/use_zero_page";
pub const SYS_THP_SHMEM_ENABLED: &str = "/sys/kernel/mm/transparent_hugepage/shmem_enabled";
pub const SYS_THP_HPAGE_PMD_SIZE: &str = "/sys/kernel/mm/transparent_hugepage/hpage_pmd_size";

// khugepaged tunables
pub const SYS_KHUGEPAGED_ROOT: &str = "/sys/kernel/mm/transparent_hugepage/khugepaged";
pub const SYS_KHUGEPAGED_DEFRAG: &str =
    "/sys/kernel/mm/transparent_hugepage/khugepaged/defrag";
pub const SYS_KHUGEPAGED_PAGES_TO_SCAN: &str =
    "/sys/kernel/mm/transparent_hugepage/khugepaged/pages_to_scan";
pub const SYS_KHUGEPAGED_SCAN_SLEEP_MS: &str =
    "/sys/kernel/mm/transparent_hugepage/khugepaged/scan_sleep_millisecs";
pub const SYS_KHUGEPAGED_ALLOC_SLEEP_MS: &str =
    "/sys/kernel/mm/transparent_hugepage/khugepaged/alloc_sleep_millisecs";

// ---------------------------------------------------------------------------
// Tristate string values for `enabled` / `defrag`
// ---------------------------------------------------------------------------

pub const THP_ALWAYS: &str = "always";
pub const THP_MADVISE: &str = "madvise";
pub const THP_NEVER: &str = "never";
pub const THP_DEFER: &str = "defer";
pub const THP_DEFER_MADVISE: &str = "defer+madvise";

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// 2 MiB — the PMD-level huge page size on x86_64.
pub const HPAGE_PMD_SIZE: usize = 2 * 1024 * 1024;
/// 1 GiB — the PUD-level huge page size (used for `MAP_HUGETLB | 30 << 26`).
pub const HPAGE_PUD_SIZE: usize = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// /proc/meminfo huge-page counter labels
// ---------------------------------------------------------------------------

pub const PROC_MEMINFO_ANON_HUGE_PAGES: &str = "AnonHugePages";
pub const PROC_MEMINFO_SHMEM_HUGE_PAGES: &str = "ShmemHugePages";
pub const PROC_MEMINFO_FILE_HUGE_PAGES: &str = "FileHugePages";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_madv_pair_adjacent() {
        // MADV_HUGEPAGE and MADV_NOHUGEPAGE were added together; the
        // kernel assigned 14/15 to keep them adjacent.
        assert_eq!(MADV_HUGEPAGE, 14);
        assert_eq!(MADV_NOHUGEPAGE, MADV_HUGEPAGE + 1);
        assert_eq!(MADV_COLLAPSE, 25);
    }

    #[test]
    fn test_sysfs_paths_under_thp_root() {
        let p = [
            SYS_THP_ENABLED,
            SYS_THP_DEFRAG,
            SYS_THP_USE_ZERO_PAGE,
            SYS_THP_SHMEM_ENABLED,
            SYS_THP_HPAGE_PMD_SIZE,
            SYS_KHUGEPAGED_ROOT,
        ];
        for path in p {
            assert!(path.starts_with(SYS_THP_ROOT));
        }
        // khugepaged subtree paths sit under SYS_KHUGEPAGED_ROOT.
        let kh = [
            SYS_KHUGEPAGED_DEFRAG,
            SYS_KHUGEPAGED_PAGES_TO_SCAN,
            SYS_KHUGEPAGED_SCAN_SLEEP_MS,
            SYS_KHUGEPAGED_ALLOC_SLEEP_MS,
        ];
        for path in kh {
            assert!(path.starts_with(SYS_KHUGEPAGED_ROOT));
        }
    }

    #[test]
    fn test_tristate_values_distinct() {
        let v = [THP_ALWAYS, THP_MADVISE, THP_NEVER, THP_DEFER, THP_DEFER_MADVISE];
        for a in 0..v.len() {
            for b in (a + 1)..v.len() {
                assert_ne!(v[a], v[b]);
            }
        }
    }

    #[test]
    fn test_hpage_sizes_are_huge() {
        // 2 MiB and 1 GiB exactly.
        assert_eq!(HPAGE_PMD_SIZE, 0x20_0000);
        assert_eq!(HPAGE_PUD_SIZE, 0x4000_0000);
        // PUD is 512x PMD on x86_64.
        assert_eq!(HPAGE_PUD_SIZE / HPAGE_PMD_SIZE, 512);
    }

    #[test]
    fn test_meminfo_labels_distinct() {
        let l = [
            PROC_MEMINFO_ANON_HUGE_PAGES,
            PROC_MEMINFO_SHMEM_HUGE_PAGES,
            PROC_MEMINFO_FILE_HUGE_PAGES,
        ];
        for label in l {
            assert!(label.ends_with("HugePages"));
        }
        assert_ne!(PROC_MEMINFO_ANON_HUGE_PAGES, PROC_MEMINFO_SHMEM_HUGE_PAGES);
        assert_ne!(PROC_MEMINFO_ANON_HUGE_PAGES, PROC_MEMINFO_FILE_HUGE_PAGES);
        assert_ne!(PROC_MEMINFO_SHMEM_HUGE_PAGES, PROC_MEMINFO_FILE_HUGE_PAGES);
    }
}
