//! `<linux/hugetlb.h>` — Huge page (hugetlb) constants.
//!
//! Huge pages provide large contiguous physical memory allocations
//! (2 MiB or 1 GiB on x86-64) that reduce TLB pressure for
//! memory-intensive workloads. They are allocated via mmap with
//! MAP_HUGETLB, hugetlbfs mounts, or the memfd_create+MFD_HUGETLB
//! interface. The kernel manages pools of pre-allocated huge pages.

// ---------------------------------------------------------------------------
// Huge page sizes (common x86-64 values)
// ---------------------------------------------------------------------------

/// 2 MiB huge page size.
pub const HPAGE_SIZE_2MB: u64 = 2 * 1024 * 1024;
/// 1 GiB huge page size.
pub const HPAGE_SIZE_1GB: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// MAP_HUGETLB flag and size encoding
// ---------------------------------------------------------------------------

/// Use huge pages for mmap.
pub const MAP_HUGETLB: u32 = 0x0004_0000;
/// Shift for encoding page size in mmap flags.
pub const MAP_HUGE_SHIFT: u32 = 26;
/// Mask for encoded page size.
pub const MAP_HUGE_MASK: u32 = 0x3F;
/// Encode 2 MiB huge pages (21 = log2(2MiB)).
pub const MAP_HUGE_2MB: u32 = 21 << MAP_HUGE_SHIFT;
/// Encode 1 GiB huge pages (30 = log2(1GiB)).
pub const MAP_HUGE_1GB: u32 = 30 << MAP_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// memfd_create flags for huge pages
// ---------------------------------------------------------------------------

/// Use huge pages for memfd.
pub const MFD_HUGETLB: u32 = 0x0004;
/// 2 MiB huge pages for memfd.
pub const MFD_HUGE_2MB: u32 = 21 << MAP_HUGE_SHIFT;
/// 1 GiB huge pages for memfd.
pub const MFD_HUGE_1GB: u32 = 30 << MAP_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// hugetlbfs mount options (encoded in mount flags)
// ---------------------------------------------------------------------------

/// Default huge page pool size (0 = use system default).
pub const HUGETLB_DEFAULT_POOL: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_sizes() {
        assert_eq!(HPAGE_SIZE_2MB, 2 * 1024 * 1024);
        assert_eq!(HPAGE_SIZE_1GB, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_map_huge_encoding() {
        assert_eq!(MAP_HUGE_2MB >> MAP_HUGE_SHIFT, 21);
        assert_eq!(MAP_HUGE_1GB >> MAP_HUGE_SHIFT, 30);
    }

    #[test]
    fn test_mfd_huge_encoding() {
        assert_eq!(MFD_HUGE_2MB >> MAP_HUGE_SHIFT, 21);
        assert_eq!(MFD_HUGE_1GB >> MAP_HUGE_SHIFT, 30);
    }

    #[test]
    fn test_map_huge_2mb_1gb_distinct() {
        assert_ne!(MAP_HUGE_2MB, MAP_HUGE_1GB);
    }

    #[test]
    fn test_hugetlb_flag() {
        assert!(MAP_HUGETLB.is_power_of_two());
    }
}
