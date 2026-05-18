//! `<linux/hugetlb.h>` — Huge page flag and size constants.
//!
//! Huge pages (2 MiB or 1 GiB on x86_64) reduce TLB pressure for
//! large memory allocations. The kernel provides explicit huge page
//! support via `hugetlbfs` and transparent huge pages (THP). These
//! constants control allocation, reservation, and accounting.

// ---------------------------------------------------------------------------
// Huge page sizes (x86_64)
// ---------------------------------------------------------------------------

/// Standard huge page size: 2 MiB.
pub const HPAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
/// Gigantic page size: 1 GiB.
pub const HPAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;
/// Huge page shift for 2 MiB pages.
pub const HPAGE_SHIFT_2M: u32 = 21;
/// Huge page shift for 1 GiB pages.
pub const HPAGE_SHIFT_1G: u32 = 30;

// ---------------------------------------------------------------------------
// hugetlbfs mount options / mmap flags
// ---------------------------------------------------------------------------

/// Huge page size encoding shift in mmap flags.
pub const HUGETLB_FLAG_ENCODE_SHIFT: u32 = 26;
/// Mask for huge page size encoding.
pub const HUGETLB_FLAG_ENCODE_MASK: u32 = 0x3F;
/// Encode 2 MiB huge page in flags.
pub const HUGETLB_FLAG_ENCODE_2MB: u32 = 21 << HUGETLB_FLAG_ENCODE_SHIFT;
/// Encode 1 GiB huge page in flags.
pub const HUGETLB_FLAG_ENCODE_1GB: u32 = 30 << HUGETLB_FLAG_ENCODE_SHIFT;
/// Encode 64 KiB huge page in flags.
pub const HUGETLB_FLAG_ENCODE_64KB: u32 = 16 << HUGETLB_FLAG_ENCODE_SHIFT;
/// Encode 512 MiB huge page in flags.
pub const HUGETLB_FLAG_ENCODE_512MB: u32 = 29 << HUGETLB_FLAG_ENCODE_SHIFT;
/// Encode 16 MiB huge page in flags.
pub const HUGETLB_FLAG_ENCODE_16MB: u32 = 24 << HUGETLB_FLAG_ENCODE_SHIFT;

// ---------------------------------------------------------------------------
// memfd_create() hugetlb flags
// ---------------------------------------------------------------------------

/// memfd uses huge pages.
pub const MFD_HUGETLB: u32 = 0x0004;
/// memfd huge page size: 2 MiB.
pub const MFD_HUGE_2MB: u32 = 21 << HUGETLB_FLAG_ENCODE_SHIFT;
/// memfd huge page size: 1 GiB.
pub const MFD_HUGE_1GB: u32 = 30 << HUGETLB_FLAG_ENCODE_SHIFT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hpage_sizes() {
        assert_eq!(HPAGE_SIZE_2M, 2 * 1024 * 1024);
        assert_eq!(HPAGE_SIZE_1G, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_hpage_shifts() {
        assert_eq!(1u64 << HPAGE_SHIFT_2M, HPAGE_SIZE_2M);
        assert_eq!(1u64 << HPAGE_SHIFT_1G, HPAGE_SIZE_1G);
    }

    #[test]
    fn test_encode_shift() {
        assert_eq!(HUGETLB_FLAG_ENCODE_SHIFT, 26);
        assert_eq!(HUGETLB_FLAG_ENCODE_MASK, 0x3F);
    }

    #[test]
    fn test_encode_flags_distinct() {
        let flags = [
            HUGETLB_FLAG_ENCODE_2MB, HUGETLB_FLAG_ENCODE_1GB,
            HUGETLB_FLAG_ENCODE_64KB, HUGETLB_FLAG_ENCODE_512MB,
            HUGETLB_FLAG_ENCODE_16MB,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_memfd_hugetlb() {
        assert_eq!(MFD_HUGETLB, 0x0004);
    }

    #[test]
    fn test_mfd_huge_matches_encode() {
        assert_eq!(MFD_HUGE_2MB, HUGETLB_FLAG_ENCODE_2MB);
        assert_eq!(MFD_HUGE_1GB, HUGETLB_FLAG_ENCODE_1GB);
    }
}
