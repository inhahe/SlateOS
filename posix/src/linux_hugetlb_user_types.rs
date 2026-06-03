//! `<linux/mman.h>` / `<linux/hugetlb_encode.h>` — huge-page mmap flags.
//!
//! Huge pages back JVM heaps, PostgreSQL `huge_pages=on`, DPDK packet
//! buffers, and CUDA pinned-memory pools — anywhere TLB pressure is
//! the bottleneck. `mmap()` and `memfd_create()` callers select page
//! size with the encoded shift values below; the kernel rejects sizes
//! the architecture can't honor (4K base + 2M/1G on x86_64).

// ---------------------------------------------------------------------------
// MAP_HUGETLB encoding (in mmap flags) — page-size shift in bits 26..31
// ---------------------------------------------------------------------------

/// `MAP_HUGETLB` — request backing by huge pages.
pub const MAP_HUGETLB: u32 = 0x40000;
/// Shift for the encoded page-size selector.
pub const MAP_HUGE_SHIFT: u32 = 26;
/// 6-bit field width.
pub const MAP_HUGE_MASK: u32 = 0x3F;

// Pre-encoded constants for common page sizes (log2 of size in bytes).
/// 64 KiB huge page (some ARM and PPC configs).
pub const MAP_HUGE_64KB: u32 = 16 << MAP_HUGE_SHIFT;
/// 512 KiB.
pub const MAP_HUGE_512KB: u32 = 19 << MAP_HUGE_SHIFT;
/// 1 MiB.
pub const MAP_HUGE_1MB: u32 = 20 << MAP_HUGE_SHIFT;
/// 2 MiB (x86_64 PMD).
pub const MAP_HUGE_2MB: u32 = 21 << MAP_HUGE_SHIFT;
/// 8 MiB.
pub const MAP_HUGE_8MB: u32 = 23 << MAP_HUGE_SHIFT;
/// 16 MiB.
pub const MAP_HUGE_16MB: u32 = 24 << MAP_HUGE_SHIFT;
/// 32 MiB.
pub const MAP_HUGE_32MB: u32 = 25 << MAP_HUGE_SHIFT;
/// 256 MiB.
pub const MAP_HUGE_256MB: u32 = 28 << MAP_HUGE_SHIFT;
/// 512 MiB.
pub const MAP_HUGE_512MB: u32 = 29 << MAP_HUGE_SHIFT;
/// 1 GiB (x86_64 PUD).
pub const MAP_HUGE_1GB: u32 = 30 << MAP_HUGE_SHIFT;
/// 2 GiB.
pub const MAP_HUGE_2GB: u32 = 31 << MAP_HUGE_SHIFT;
/// 16 GiB.
pub const MAP_HUGE_16GB: u32 = 34u32.wrapping_shl(MAP_HUGE_SHIFT);

// ---------------------------------------------------------------------------
// memfd_create(2) flags
// ---------------------------------------------------------------------------

/// `MFD_HUGETLB` — back the memfd by huge pages.
pub const MFD_HUGETLB: u32 = 0x0004;
/// `MFD_CLOEXEC`.
pub const MFD_CLOEXEC: u32 = 0x0001;
/// `MFD_ALLOW_SEALING`.
pub const MFD_ALLOW_SEALING: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Sysfs / sysctl-visible page counts
// ---------------------------------------------------------------------------

/// Default name of the hugetlbfs filesystem.
pub const HUGETLBFS_NAME: &str = "hugetlbfs";
/// Magic value reported by `statfs()` on hugetlbfs.
pub const HUGETLBFS_MAGIC: u32 = 0x958458F6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_hugetlb_bit_is_pow2() {
        assert!(MAP_HUGETLB.is_power_of_two());
        // Must not collide with the page-size selector field.
        let selector_mask = MAP_HUGE_MASK << MAP_HUGE_SHIFT;
        assert_eq!(MAP_HUGETLB & selector_mask, 0);
    }

    #[test]
    fn test_page_size_encoding_round_trip() {
        // The encoded value's high field should decode back to the shift.
        assert_eq!(MAP_HUGE_2MB >> MAP_HUGE_SHIFT, 21);
        assert_eq!(MAP_HUGE_1GB >> MAP_HUGE_SHIFT, 30);
        // 2 MiB = 2^21, 1 GiB = 2^30.
        assert_eq!(1u64 << (MAP_HUGE_2MB >> MAP_HUGE_SHIFT), 2 * 1024 * 1024);
        assert_eq!(
            1u64 << (MAP_HUGE_1GB >> MAP_HUGE_SHIFT),
            1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_page_sizes_monotonic() {
        let s = [
            MAP_HUGE_64KB,
            MAP_HUGE_512KB,
            MAP_HUGE_1MB,
            MAP_HUGE_2MB,
            MAP_HUGE_8MB,
            MAP_HUGE_16MB,
            MAP_HUGE_32MB,
            MAP_HUGE_256MB,
            MAP_HUGE_512MB,
            MAP_HUGE_1GB,
            MAP_HUGE_2GB,
        ];
        for w in s.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn test_memfd_create_flags() {
        for &b in &[MFD_CLOEXEC, MFD_ALLOW_SEALING, MFD_HUGETLB] {
            assert!(b.is_power_of_two());
        }
        assert_ne!(MFD_CLOEXEC, MFD_ALLOW_SEALING);
        assert_ne!(MFD_ALLOW_SEALING, MFD_HUGETLB);
    }

    #[test]
    fn test_hugetlbfs_metadata() {
        // 0x958458F6 is the documented hugetlbfs statfs magic.
        assert_eq!(HUGETLBFS_MAGIC, 0x958458F6);
        assert_eq!(HUGETLBFS_NAME, "hugetlbfs");
    }
}
