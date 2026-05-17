//! `<linux/mman.h>` — mremap() flag constants.
//!
//! mremap() resizes or moves an existing memory mapping. It can grow
//! a mapping in place if adjacent virtual address space is available,
//! or move it to a new location. It's used by realloc() implementations,
//! growing stacks, and dynamic buffer expansion without copying data
//! when the kernel can just remap pages.

// ---------------------------------------------------------------------------
// mremap flags
// ---------------------------------------------------------------------------

/// Allow the mapping to move to a new address if it cannot grow in place.
pub const MREMAP_MAYMOVE: u32 = 1;
/// Move to the exact address specified (new_address parameter).
pub const MREMAP_FIXED: u32 = 2;
/// Don't unmap the original mapping (create a new mapping at new address
/// that shares the same pages). Linux 5.7+.
pub const MREMAP_DONTUNMAP: u32 = 4;

// ---------------------------------------------------------------------------
// mmap flags (commonly used with mremap context)
// ---------------------------------------------------------------------------

/// Map at a fixed address (fail if unavailable).
pub const MAP_FIXED: u32 = 0x10;
/// Map at a fixed address, replacing any existing mapping.
pub const MAP_FIXED_NOREPLACE: u32 = 0x10_0000;
/// Private copy-on-write mapping.
pub const MAP_PRIVATE: u32 = 0x02;
/// Shared mapping (visible to other processes).
pub const MAP_SHARED: u32 = 0x01;
/// Anonymous mapping (not backed by file).
pub const MAP_ANONYMOUS: u32 = 0x20;
/// Don't reserve swap space.
pub const MAP_NORESERVE: u32 = 0x4000;
/// Populate page tables (prefault).
pub const MAP_POPULATE: u32 = 0x8000;
/// Lock pages in memory after mapping.
pub const MAP_LOCKED: u32 = 0x2000;
/// Grow downward (stacks).
pub const MAP_GROWSDOWN: u32 = 0x0100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mremap_flags_no_overlap() {
        let flags = [MREMAP_MAYMOVE, MREMAP_FIXED, MREMAP_DONTUNMAP];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_map_private_shared_distinct() {
        assert_ne!(MAP_PRIVATE, MAP_SHARED);
        assert_eq!(MAP_PRIVATE & MAP_SHARED, 0);
    }

    #[test]
    fn test_map_flags_distinct() {
        let flags = [
            MAP_SHARED, MAP_PRIVATE, MAP_FIXED, MAP_ANONYMOUS,
            MAP_GROWSDOWN, MAP_LOCKED, MAP_NORESERVE, MAP_POPULATE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_map_fixed_noreplace() {
        // FIXED_NOREPLACE is distinct from plain FIXED
        assert_ne!(MAP_FIXED, MAP_FIXED_NOREPLACE);
    }
}
