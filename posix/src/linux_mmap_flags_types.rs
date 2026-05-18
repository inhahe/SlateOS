//! `<sys/mman.h>` — Memory mapping flag constants.
//!
//! These flags control the behavior of `mmap()` and related memory
//! mapping syscalls. They specify visibility (shared vs private),
//! placement hints, and special mapping semantics like anonymous
//! memory, fixed addresses, and huge page backing.

// ---------------------------------------------------------------------------
// Protection flags (prot argument)
// ---------------------------------------------------------------------------

/// Pages may not be accessed.
pub const PROT_NONE: u32 = 0x0;
/// Pages may be read.
pub const PROT_READ: u32 = 0x1;
/// Pages may be written.
pub const PROT_WRITE: u32 = 0x2;
/// Pages may be executed.
pub const PROT_EXEC: u32 = 0x4;

// ---------------------------------------------------------------------------
// Mapping type flags (flags argument)
// ---------------------------------------------------------------------------

/// Share mapping with other processes.
pub const MAP_SHARED: u32 = 0x01;
/// Private copy-on-write mapping.
pub const MAP_PRIVATE: u32 = 0x02;
/// Shared mapping with atomic validation.
pub const MAP_SHARED_VALIDATE: u32 = 0x03;

// ---------------------------------------------------------------------------
// Mapping modifier flags
// ---------------------------------------------------------------------------

/// Place mapping at exact address.
pub const MAP_FIXED: u32 = 0x10;
/// Anonymous mapping (not file-backed).
pub const MAP_ANONYMOUS: u32 = 0x20;
/// Grow mapping downward (stack-like).
pub const MAP_GROWSDOWN: u32 = 0x0100;
/// Mark as DONTFORK (exclude from child).
pub const MAP_DENYWRITE: u32 = 0x0800;
/// Lock pages in memory.
pub const MAP_LOCKED: u32 = 0x2000;
/// Don't reserve swap space.
pub const MAP_NORESERVE: u32 = 0x4000;
/// Populate (prefault) page tables.
pub const MAP_POPULATE: u32 = 0x8000;
/// Do not block on IO for population.
pub const MAP_NONBLOCK: u32 = 0x10000;
/// Allocate in process's stack area.
pub const MAP_STACK: u32 = 0x20000;
/// Create huge page mapping.
pub const MAP_HUGETLB: u32 = 0x40000;
/// MAP_FIXED that doesn't unmap existing.
pub const MAP_FIXED_NOREPLACE: u32 = 0x100000;

// ---------------------------------------------------------------------------
// Huge page size encoding (MAP_HUGETLB flag shifts)
// ---------------------------------------------------------------------------

/// Shift for huge page size in flags.
pub const MAP_HUGE_SHIFT: u32 = 26;
/// Mask for huge page size bits.
pub const MAP_HUGE_MASK: u32 = 0x3F;
/// 2 MiB huge page.
pub const MAP_HUGE_2MB: u32 = 21 << MAP_HUGE_SHIFT;
/// 1 GiB huge page.
pub const MAP_HUGE_1GB: u32 = 30 << MAP_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prot_flags_no_overlap() {
        let flags = [PROT_READ, PROT_WRITE, PROT_EXEC];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_prot_none_is_zero() {
        assert_eq!(PROT_NONE, 0);
    }

    #[test]
    fn test_map_type_values() {
        assert_eq!(MAP_SHARED, 0x01);
        assert_eq!(MAP_PRIVATE, 0x02);
        assert_eq!(MAP_SHARED_VALIDATE, 0x03);
    }

    #[test]
    fn test_map_anonymous_value() {
        assert_eq!(MAP_ANONYMOUS, 0x20);
    }

    #[test]
    fn test_map_fixed_noreplace() {
        assert_eq!(MAP_FIXED_NOREPLACE, 0x100000);
    }

    #[test]
    fn test_huge_page_encoding() {
        assert_eq!(MAP_HUGE_SHIFT, 26);
        assert_eq!(MAP_HUGE_2MB, 21 << 26);
        assert_eq!(MAP_HUGE_1GB, 30 << 26);
    }

    #[test]
    fn test_modifier_flags_distinct() {
        let flags = [
            MAP_FIXED, MAP_ANONYMOUS, MAP_GROWSDOWN,
            MAP_DENYWRITE, MAP_LOCKED, MAP_NORESERVE,
            MAP_POPULATE, MAP_NONBLOCK, MAP_STACK,
            MAP_HUGETLB, MAP_FIXED_NOREPLACE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
