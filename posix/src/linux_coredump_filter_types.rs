//! `<linux/coredump.h>` — Core dump filter flag constants.
//!
//! The coredump filter controls which memory mappings are included
//! in a process core dump. Each bit enables dumping of a specific
//! mapping type. The filter is set via `/proc/<pid>/coredump_filter`.

// ---------------------------------------------------------------------------
// Coredump filter flags
// ---------------------------------------------------------------------------

/// Dump anonymous private mappings.
pub const COREDUMP_FILTER_ANON_PRIVATE: u32 = 1 << 0;
/// Dump anonymous shared mappings.
pub const COREDUMP_FILTER_ANON_SHARED: u32 = 1 << 1;
/// Dump file-backed private mappings.
pub const COREDUMP_FILTER_MAPPED_PRIVATE: u32 = 1 << 2;
/// Dump file-backed shared mappings.
pub const COREDUMP_FILTER_MAPPED_SHARED: u32 = 1 << 3;
/// Dump ELF headers.
pub const COREDUMP_FILTER_ELF_HEADERS: u32 = 1 << 4;
/// Dump private hugepage mappings.
pub const COREDUMP_FILTER_HUGETLB_PRIVATE: u32 = 1 << 5;
/// Dump shared hugepage mappings.
pub const COREDUMP_FILTER_HUGETLB_SHARED: u32 = 1 << 6;
/// Dump DAX private mappings.
pub const COREDUMP_FILTER_DAX_PRIVATE: u32 = 1 << 7;
/// Dump DAX shared mappings.
pub const COREDUMP_FILTER_DAX_SHARED: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Default filter value
// ---------------------------------------------------------------------------

/// Default coredump filter: anon private + anon shared (bits 0,1).
pub const COREDUMP_FILTER_DEFAULT: u32 = COREDUMP_FILTER_ANON_PRIVATE | COREDUMP_FILTER_ANON_SHARED;

// ---------------------------------------------------------------------------
// Core dump signal constants
// ---------------------------------------------------------------------------

/// Maximum core file size indicator: unlimited.
pub const CORE_FILE_UNLIMITED: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// ELF core note types
// ---------------------------------------------------------------------------

/// Process status note (prstatus).
pub const NT_PRSTATUS: u32 = 1;
/// Floating-point registers note.
pub const NT_PRFPREG: u32 = 2;
/// Process info note (prpsinfo).
pub const NT_PRPSINFO: u32 = 3;
/// x86 XSTATE (AVX, etc.).
pub const NT_X86_XSTATE: u32 = 0x202;
/// Signal info note.
pub const NT_SIGINFO: u32 = 0x53494749;
/// File mappings note.
pub const NT_FILE: u32 = 0x46494C45;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_flags_power_of_two() {
        let flags = [
            COREDUMP_FILTER_ANON_PRIVATE,
            COREDUMP_FILTER_ANON_SHARED,
            COREDUMP_FILTER_MAPPED_PRIVATE,
            COREDUMP_FILTER_MAPPED_SHARED,
            COREDUMP_FILTER_ELF_HEADERS,
            COREDUMP_FILTER_HUGETLB_PRIVATE,
            COREDUMP_FILTER_HUGETLB_SHARED,
            COREDUMP_FILTER_DAX_PRIVATE,
            COREDUMP_FILTER_DAX_SHARED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_filter_flags_no_overlap() {
        let flags = [
            COREDUMP_FILTER_ANON_PRIVATE,
            COREDUMP_FILTER_ANON_SHARED,
            COREDUMP_FILTER_MAPPED_PRIVATE,
            COREDUMP_FILTER_MAPPED_SHARED,
            COREDUMP_FILTER_ELF_HEADERS,
            COREDUMP_FILTER_HUGETLB_PRIVATE,
            COREDUMP_FILTER_HUGETLB_SHARED,
            COREDUMP_FILTER_DAX_PRIVATE,
            COREDUMP_FILTER_DAX_SHARED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_default_filter() {
        assert_eq!(COREDUMP_FILTER_DEFAULT, 0x03);
    }

    #[test]
    fn test_note_types_distinct() {
        let notes = [
            NT_PRSTATUS,
            NT_PRFPREG,
            NT_PRPSINFO,
            NT_X86_XSTATE,
            NT_SIGINFO,
            NT_FILE,
        ];
        for i in 0..notes.len() {
            for j in (i + 1)..notes.len() {
                assert_ne!(notes[i], notes[j]);
            }
        }
    }

    #[test]
    fn test_nt_prstatus() {
        assert_eq!(NT_PRSTATUS, 1);
    }

    #[test]
    fn test_core_file_unlimited() {
        assert_eq!(CORE_FILE_UNLIMITED, u64::MAX);
    }
}
