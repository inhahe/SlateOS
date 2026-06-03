//! `<linux/hugetlb.h>` — Huge page constants.
//!
//! Huge pages (2 MiB / 1 GiB on x86_64) reduce TLB pressure for
//! large-memory workloads. The hugetlb subsystem manages a pool
//! of pre-allocated huge pages. Transparent Huge Pages (THP)
//! provide automatic huge page usage without explicit allocation.

// ---------------------------------------------------------------------------
// Huge page sizes (x86_64)
// ---------------------------------------------------------------------------

/// 2 MiB huge page size.
pub const HPAGE_SIZE_2MB: u64 = 2 * 1024 * 1024;
/// 1 GiB huge page size.
pub const HPAGE_SIZE_1GB: u64 = 1024 * 1024 * 1024;

/// 2 MiB huge page shift.
pub const HPAGE_SHIFT_2MB: u32 = 21;
/// 1 GiB huge page shift.
pub const HPAGE_SHIFT_1GB: u32 = 30;

// ---------------------------------------------------------------------------
// Hugetlb mmap flags (used with mmap MAP_HUGETLB)
// ---------------------------------------------------------------------------

/// Huge page flag (ORed with MAP_HUGETLB).
pub const MAP_HUGE_SHIFT: u32 = 26;
/// Huge page size mask.
pub const MAP_HUGE_MASK: u32 = 0x3F;
/// 2 MiB (log2 = 21, 21 << 26).
pub const MAP_HUGE_2MB: u32 = 21 << MAP_HUGE_SHIFT;
/// 1 GiB (log2 = 30, 30 << 26).
pub const MAP_HUGE_1GB: u32 = 30 << MAP_HUGE_SHIFT;
/// 16 KiB (log2 = 14, for our OS page size).
pub const MAP_HUGE_16KB: u32 = 14 << MAP_HUGE_SHIFT;
/// 64 KiB.
pub const MAP_HUGE_64KB: u32 = 16 << MAP_HUGE_SHIFT;
/// 512 KiB.
pub const MAP_HUGE_512KB: u32 = 19 << MAP_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// THP (Transparent Huge Pages) modes
// ---------------------------------------------------------------------------

/// THP always enabled.
pub const THP_ALWAYS: u32 = 0;
/// THP madvise-only.
pub const THP_MADVISE: u32 = 1;
/// THP disabled.
pub const THP_NEVER: u32 = 2;

// ---------------------------------------------------------------------------
// THP defrag modes
// ---------------------------------------------------------------------------

/// Defrag always.
pub const THP_DEFRAG_ALWAYS: u32 = 0;
/// Defrag on madvise.
pub const THP_DEFRAG_MADVISE: u32 = 1;
/// Defrag deferred.
pub const THP_DEFRAG_DEFER: u32 = 2;
/// Defrag deferred + madvise.
pub const THP_DEFRAG_DEFER_MADVISE: u32 = 3;
/// Defrag never.
pub const THP_DEFRAG_NEVER: u32 = 4;

// ---------------------------------------------------------------------------
// Hugetlb pool management
// ---------------------------------------------------------------------------

/// Surplus huge pages allowed.
pub const HUGETLB_SURPLUS: u32 = 1 << 0;
/// Huge page is on free list.
pub const HUGETLB_FREE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huge_page_sizes() {
        assert_eq!(HPAGE_SIZE_2MB, 1u64 << HPAGE_SHIFT_2MB);
        assert_eq!(HPAGE_SIZE_1GB, 1u64 << HPAGE_SHIFT_1GB);
    }

    #[test]
    fn test_map_huge_values() {
        assert_eq!(MAP_HUGE_2MB, 21 << 26);
        assert_eq!(MAP_HUGE_1GB, 30 << 26);
    }

    #[test]
    fn test_map_huge_distinct() {
        let sizes = [
            MAP_HUGE_2MB,
            MAP_HUGE_1GB,
            MAP_HUGE_16KB,
            MAP_HUGE_64KB,
            MAP_HUGE_512KB,
        ];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_thp_modes_distinct() {
        let modes = [THP_ALWAYS, THP_MADVISE, THP_NEVER];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_thp_defrag_distinct() {
        let modes = [
            THP_DEFRAG_ALWAYS,
            THP_DEFRAG_MADVISE,
            THP_DEFRAG_DEFER,
            THP_DEFRAG_DEFER_MADVISE,
            THP_DEFRAG_NEVER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_hugetlb_flags_no_overlap() {
        assert_eq!(HUGETLB_SURPLUS & HUGETLB_FREE, 0);
    }
}
