//! `/proc/<pid>/coredump_filter` — per-process bitmask controlling which
//! memory regions are written into a core dump.
//!
//! The filter lets the user reduce dump size by excluding mapped files
//! (which already exist on disk) or shared memory (which other procs
//! still hold). Each bit corresponds to a category of VMA.

// ---------------------------------------------------------------------------
// Filter bit positions (MMF_DUMP_*)
// ---------------------------------------------------------------------------

pub const COREDUMP_BIT_ANON_PRIVATE: u32 = 0;
pub const COREDUMP_BIT_ANON_SHARED: u32 = 1;
pub const COREDUMP_BIT_MAPPED_PRIVATE: u32 = 2;
pub const COREDUMP_BIT_MAPPED_SHARED: u32 = 3;
pub const COREDUMP_BIT_ELF_HEADERS: u32 = 4;
pub const COREDUMP_BIT_HUGETLB_PRIVATE: u32 = 5;
pub const COREDUMP_BIT_HUGETLB_SHARED: u32 = 6;
pub const COREDUMP_BIT_DAX_PRIVATE: u32 = 7;
pub const COREDUMP_BIT_DAX_SHARED: u32 = 8;

// ---------------------------------------------------------------------------
// Filter values (1 << bit)
// ---------------------------------------------------------------------------

pub const COREDUMP_MASK_ANON_PRIVATE: u32 = 1 << COREDUMP_BIT_ANON_PRIVATE;
pub const COREDUMP_MASK_ANON_SHARED: u32 = 1 << COREDUMP_BIT_ANON_SHARED;
pub const COREDUMP_MASK_MAPPED_PRIVATE: u32 = 1 << COREDUMP_BIT_MAPPED_PRIVATE;
pub const COREDUMP_MASK_MAPPED_SHARED: u32 = 1 << COREDUMP_BIT_MAPPED_SHARED;
pub const COREDUMP_MASK_ELF_HEADERS: u32 = 1 << COREDUMP_BIT_ELF_HEADERS;
pub const COREDUMP_MASK_HUGETLB_PRIVATE: u32 = 1 << COREDUMP_BIT_HUGETLB_PRIVATE;
pub const COREDUMP_MASK_HUGETLB_SHARED: u32 = 1 << COREDUMP_BIT_HUGETLB_SHARED;
pub const COREDUMP_MASK_DAX_PRIVATE: u32 = 1 << COREDUMP_BIT_DAX_PRIVATE;
pub const COREDUMP_MASK_DAX_SHARED: u32 = 1 << COREDUMP_BIT_DAX_SHARED;

// ---------------------------------------------------------------------------
// Filter combinations
// ---------------------------------------------------------------------------

/// Kernel default for new processes (anon + ELF + HugeTLB private).
pub const COREDUMP_FILTER_DEFAULT: u32 = COREDUMP_MASK_ANON_PRIVATE
    | COREDUMP_MASK_ANON_SHARED
    | COREDUMP_MASK_ELF_HEADERS
    | COREDUMP_MASK_HUGETLB_PRIVATE;

/// Everything off — emit just the metadata.
pub const COREDUMP_FILTER_EMPTY: u32 = 0;
/// All nine documented bits set.
pub const COREDUMP_FILTER_ALL: u32 = 0x1FF;

// ---------------------------------------------------------------------------
// /proc location
// ---------------------------------------------------------------------------

pub const PROC_SELF_COREDUMP_FILTER: &str = "/proc/self/coredump_filter";
pub const PROC_PID_COREDUMP_FILTER_FMT: &str = "/proc/{}/coredump_filter";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bits_dense_0_to_8() {
        let b = [
            COREDUMP_BIT_ANON_PRIVATE,
            COREDUMP_BIT_ANON_SHARED,
            COREDUMP_BIT_MAPPED_PRIVATE,
            COREDUMP_BIT_MAPPED_SHARED,
            COREDUMP_BIT_ELF_HEADERS,
            COREDUMP_BIT_HUGETLB_PRIVATE,
            COREDUMP_BIT_HUGETLB_SHARED,
            COREDUMP_BIT_DAX_PRIVATE,
            COREDUMP_BIT_DAX_SHARED,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_masks_are_shifted_bits() {
        assert_eq!(COREDUMP_MASK_ANON_PRIVATE, 1);
        assert_eq!(COREDUMP_MASK_ANON_SHARED, 2);
        assert_eq!(COREDUMP_MASK_MAPPED_PRIVATE, 4);
        assert_eq!(COREDUMP_MASK_DAX_SHARED, 0x100);
    }

    #[test]
    fn test_all_mask_is_low_9_bits() {
        let or = COREDUMP_MASK_ANON_PRIVATE
            | COREDUMP_MASK_ANON_SHARED
            | COREDUMP_MASK_MAPPED_PRIVATE
            | COREDUMP_MASK_MAPPED_SHARED
            | COREDUMP_MASK_ELF_HEADERS
            | COREDUMP_MASK_HUGETLB_PRIVATE
            | COREDUMP_MASK_HUGETLB_SHARED
            | COREDUMP_MASK_DAX_PRIVATE
            | COREDUMP_MASK_DAX_SHARED;
        assert_eq!(or, COREDUMP_FILTER_ALL);
        assert_eq!(COREDUMP_FILTER_ALL.count_ones(), 9);
    }

    #[test]
    fn test_default_excludes_mapped_files() {
        assert_eq!(COREDUMP_FILTER_DEFAULT & COREDUMP_MASK_MAPPED_PRIVATE, 0);
        assert_eq!(COREDUMP_FILTER_DEFAULT & COREDUMP_MASK_MAPPED_SHARED, 0);
    }

    #[test]
    fn test_default_includes_anon_and_elf() {
        assert_ne!(COREDUMP_FILTER_DEFAULT & COREDUMP_MASK_ANON_PRIVATE, 0);
        assert_ne!(COREDUMP_FILTER_DEFAULT & COREDUMP_MASK_ELF_HEADERS, 0);
    }

    #[test]
    fn test_empty_filter_is_zero() {
        assert_eq!(COREDUMP_FILTER_EMPTY, 0);
    }

    #[test]
    fn test_proc_paths_well_formed() {
        assert!(PROC_SELF_COREDUMP_FILTER.starts_with("/proc/self/"));
        assert!(PROC_PID_COREDUMP_FILTER_FMT.contains("{}"));
    }
}
