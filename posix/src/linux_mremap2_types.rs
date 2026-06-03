//! `<linux/mman.h>` — Additional mremap constants.
//!
//! Supplementary mremap constants covering remap flags
//! and virtual memory relocation options.

// ---------------------------------------------------------------------------
// mremap flags (MREMAP_*)
// ---------------------------------------------------------------------------

/// May move to a new address.
pub const MREMAP_MAYMOVE: u32 = 1 << 0;
/// Fixed destination address.
pub const MREMAP_FIXED: u32 = 1 << 1;
/// Don't unmap old mapping.
pub const MREMAP_DONTUNMAP: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// mmap MAP_* flag additions
// ---------------------------------------------------------------------------

/// Shared mapping.
pub const MAP_SHARED_MREMAP: u32 = 0x01;
/// Private (copy-on-write) mapping.
pub const MAP_PRIVATE_MREMAP: u32 = 0x02;
/// Fixed address.
pub const MAP_FIXED_MREMAP: u32 = 0x10;
/// Anonymous mapping.
pub const MAP_ANONYMOUS_MREMAP: u32 = 0x20;
/// Grow downward (stack).
pub const MAP_GROWSDOWN_MREMAP: u32 = 0x00000100;
/// Don't reserve swap.
pub const MAP_NORESERVE_MREMAP: u32 = 0x00004000;
/// Populate page tables.
pub const MAP_POPULATE_MREMAP: u32 = 0x00008000;
/// Non-blocking population.
pub const MAP_NONBLOCK_MREMAP: u32 = 0x00010000;
/// Stack-like mapping.
pub const MAP_STACK_MREMAP: u32 = 0x00020000;
/// Use huge pages.
pub const MAP_HUGETLB_MREMAP: u32 = 0x00040000;
/// Lock pages in memory.
pub const MAP_LOCKED_MREMAP: u32 = 0x00002000;
/// Sync mapping (DAX).
pub const MAP_SYNC_MREMAP: u32 = 0x00080000;
/// Fixed but don't fail if address taken.
pub const MAP_FIXED_NOREPLACE_MREMAP: u32 = 0x00100000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mremap_flags_power_of_two() {
        assert!(MREMAP_MAYMOVE.is_power_of_two());
        assert!(MREMAP_FIXED.is_power_of_two());
        assert!(MREMAP_DONTUNMAP.is_power_of_two());
    }

    #[test]
    fn test_mremap_flags_no_overlap() {
        let flags = [MREMAP_MAYMOVE, MREMAP_FIXED, MREMAP_DONTUNMAP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_map_flags_distinct() {
        let flags = [
            MAP_SHARED_MREMAP,
            MAP_PRIVATE_MREMAP,
            MAP_FIXED_MREMAP,
            MAP_ANONYMOUS_MREMAP,
            MAP_GROWSDOWN_MREMAP,
            MAP_NORESERVE_MREMAP,
            MAP_POPULATE_MREMAP,
            MAP_NONBLOCK_MREMAP,
            MAP_STACK_MREMAP,
            MAP_HUGETLB_MREMAP,
            MAP_LOCKED_MREMAP,
            MAP_SYNC_MREMAP,
            MAP_FIXED_NOREPLACE_MREMAP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_shared_private_no_overlap() {
        assert_eq!(MAP_SHARED_MREMAP & MAP_PRIVATE_MREMAP, 0);
    }
}
