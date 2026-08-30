//! `<linux/huge_mm.h>` — Transparent Huge Pages (THP) constants.
//!
//! THP allows the kernel to automatically use huge pages (2 MiB on
//! x86_64) for anonymous memory mappings, improving TLB coverage
//! and reducing page table overhead for large allocations.

// ---------------------------------------------------------------------------
// THP modes (sysfs: /sys/kernel/mm/transparent_hugepage/enabled)
// ---------------------------------------------------------------------------

/// THP always enabled for all processes.
pub const THP_ALWAYS: u32 = 0;
/// THP enabled only for madvise(MADV_HUGEPAGE) regions.
pub const THP_MADVISE: u32 = 1;
/// THP disabled entirely.
pub const THP_NEVER: u32 = 2;

// ---------------------------------------------------------------------------
// THP defrag modes (/sys/kernel/mm/transparent_hugepage/defrag)
// ---------------------------------------------------------------------------

/// Always defrag (compact memory synchronously).
pub const THP_DEFRAG_ALWAYS: u32 = 0;
/// Defer defrag to khugepaged.
pub const THP_DEFRAG_DEFER: u32 = 1;
/// Defer + try madvise regions synchronously.
pub const THP_DEFRAG_DEFER_MADVISE: u32 = 2;
/// Only defrag for madvise regions.
pub const THP_DEFRAG_MADVISE: u32 = 3;
/// Never defrag for THP.
pub const THP_DEFRAG_NEVER: u32 = 4;

// ---------------------------------------------------------------------------
// Huge page sizes (x86_64)
// ---------------------------------------------------------------------------

/// 2 MiB huge page size (PMD level).
pub const HPAGE_PMD_SIZE: u64 = 2 * 1024 * 1024;
/// 1 GiB huge page size (PUD level).
pub const HPAGE_PUD_SIZE: u64 = 1024 * 1024 * 1024;
/// PMD page shift (21 bits for 2 MiB).
pub const HPAGE_PMD_SHIFT: u32 = 21;
/// PUD page shift (30 bits for 1 GiB).
pub const HPAGE_PUD_SHIFT: u32 = 30;

// ---------------------------------------------------------------------------
// khugepaged tunables (default values)
// ---------------------------------------------------------------------------

/// Default scan sleep interval (milliseconds).
pub const KHUGEPAGED_SCAN_SLEEP_DEFAULT: u32 = 10000;
/// Default number of pages to scan per pass.
pub const KHUGEPAGED_PAGES_TO_SCAN_DEFAULT: u32 = 4096;
/// Default allocation sleep on failure (milliseconds).
pub const KHUGEPAGED_ALLOC_SLEEP_DEFAULT: u32 = 60000;
/// Maximum pages per collapse operation.
pub const KHUGEPAGED_MAX_PTES_NONE_DEFAULT: u32 = 511;

// ---------------------------------------------------------------------------
// THP split actions
// ---------------------------------------------------------------------------

/// Split huge page into base pages.
pub const THP_SPLIT_FULL: u32 = 0;
/// Partial split (deferred).
pub const THP_SPLIT_PARTIAL: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [THP_ALWAYS, THP_MADVISE, THP_NEVER];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_defrag_modes_distinct() {
        let modes = [
            THP_DEFRAG_ALWAYS,
            THP_DEFRAG_DEFER,
            THP_DEFRAG_DEFER_MADVISE,
            THP_DEFRAG_MADVISE,
            THP_DEFRAG_NEVER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_hpage_pmd_size() {
        assert_eq!(HPAGE_PMD_SIZE, 2 * 1024 * 1024);
        assert_eq!(HPAGE_PMD_SIZE, 1u64 << HPAGE_PMD_SHIFT);
    }

    #[test]
    fn test_hpage_pud_size() {
        assert_eq!(HPAGE_PUD_SIZE, 1024 * 1024 * 1024);
        assert_eq!(HPAGE_PUD_SIZE, 1u64 << HPAGE_PUD_SHIFT);
    }

    #[test]
    fn test_khugepaged_defaults() {
        assert_eq!(KHUGEPAGED_SCAN_SLEEP_DEFAULT, 10000);
        assert_eq!(KHUGEPAGED_PAGES_TO_SCAN_DEFAULT, 4096);
    }

    #[test]
    fn test_split_actions_distinct() {
        assert_ne!(THP_SPLIT_FULL, THP_SPLIT_PARTIAL);
    }
}
