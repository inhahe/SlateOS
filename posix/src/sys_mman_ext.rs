//! Extended memory management constants (Linux-specific).
//!
//! Additional `mmap`/`madvise`/`mremap` flags not part of the base
//! POSIX `<sys/mman.h>` definitions.

// Re-export core mman API.
pub use crate::sys_mman::*;

// ---------------------------------------------------------------------------
// Additional MAP flags
// ---------------------------------------------------------------------------

/// Map huge pages (2MiB on x86_64).
pub const MAP_HUGETLB: i32 = 0x40000;

/// Stack-like mapping (grows down).
pub const MAP_STACK: i32 = 0x20000;

/// Lock the mapped pages into memory.
pub const MAP_LOCKED: i32 = 0x2000;

// ---------------------------------------------------------------------------
// Additional MADV flags
// ---------------------------------------------------------------------------

/// Free pages (lazy reclaim).
pub const MADV_FREE: i32 = 8;

/// Remove pages.
pub const MADV_REMOVE: i32 = 9;

/// Do not inherit across fork.
pub const MADV_DONTFORK: i32 = 10;

/// Inherit across fork (undo `DONTFORK`).
pub const MADV_DOFORK: i32 = 11;

/// Poison page (for hardware error simulation).
pub const MADV_HWPOISON: i32 = 100;

/// Soft offline page (non-fatal error).
pub const MADV_SOFT_OFFLINE: i32 = 101;

/// Advise for merged pages (KSM).
pub const MADV_MERGEABLE: i32 = 12;

/// Advise against merging.
pub const MADV_UNMERGEABLE: i32 = 13;

/// Mark pages as hugepage-eligible.
pub const MADV_HUGEPAGE: i32 = 14;

/// Mark pages as not hugepage-eligible.
pub const MADV_NOHUGEPAGE: i32 = 15;

/// Do not dump pages in core file.
pub const MADV_DONTDUMP: i32 = 16;

/// Include pages in core file.
pub const MADV_DODUMP: i32 = 17;

/// Will need pages soon (hint).
pub const MADV_WILLNEED_ALT: i32 = MADV_WILLNEED;

/// Wipe page contents on fork.
pub const MADV_WIPEONFORK: i32 = 18;

/// Keep page contents on fork (undo `WIPEONFORK`).
pub const MADV_KEEPONFORK: i32 = 19;

/// Cold pages (good candidates for reclaim).
pub const MADV_COLD: i32 = 20;

/// Immediately page out.
pub const MADV_PAGEOUT: i32 = 21;

// ---------------------------------------------------------------------------
// mremap flags
// ---------------------------------------------------------------------------

/// Allow remapping to move to a new address.
pub const MREMAP_MAYMOVE: i32 = 1;

/// Remap to a fixed address.
pub const MREMAP_FIXED: i32 = 2;

/// Do not unmap the old mapping.
pub const MREMAP_DONTUNMAP: i32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_hugetlb() {
        assert_ne!(MAP_HUGETLB, 0);
    }

    #[test]
    fn test_map_ext_flags_distinct() {
        let flags = [MAP_HUGETLB, MAP_STACK, MAP_LOCKED];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_madv_flags_distinct() {
        let flags = [
            MADV_FREE,
            MADV_REMOVE,
            MADV_DONTFORK,
            MADV_DOFORK,
            MADV_MERGEABLE,
            MADV_UNMERGEABLE,
            MADV_HUGEPAGE,
            MADV_NOHUGEPAGE,
            MADV_DONTDUMP,
            MADV_DODUMP,
            MADV_WIPEONFORK,
            MADV_KEEPONFORK,
            MADV_COLD,
            MADV_PAGEOUT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_mremap_flags() {
        assert_eq!(MREMAP_MAYMOVE, 1);
        assert_eq!(MREMAP_FIXED, 2);
        assert_eq!(MREMAP_DONTUNMAP, 4);
    }

    #[test]
    fn test_mremap_flags_powers_of_two() {
        let flags = [MREMAP_MAYMOVE, MREMAP_FIXED, MREMAP_DONTUNMAP];
        for &f in &flags {
            assert!(f.count_ones() == 1, "MREMAP flag {f} should be power of 2");
        }
    }

    #[test]
    fn test_hugetlb_no_overlap() {
        // MAP_HUGETLB should not overlap base MAP_ flags.
        assert_eq!(MAP_HUGETLB & MAP_SHARED, 0);
        assert_eq!(MAP_HUGETLB & MAP_PRIVATE, 0);
        assert_eq!(MAP_HUGETLB & MAP_FIXED, 0);
        assert_eq!(MAP_HUGETLB & MAP_ANONYMOUS, 0);
    }
}
