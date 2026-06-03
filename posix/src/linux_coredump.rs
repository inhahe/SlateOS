//! `<linux/coredump.h>` — Core dump constants.
//!
//! When a process crashes (segfault, abort, etc.), the kernel can
//! write a core dump file containing the process memory and register
//! state. This module defines the ELF note types, dump filter flags,
//! and limits that control core dump generation.

// ---------------------------------------------------------------------------
// Core dump filter bits (/proc/<pid>/coredump_filter)
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
/// Dump huge-page private mappings.
pub const COREDUMP_FILTER_HUGETLB_PRIVATE: u32 = 1 << 5;
/// Dump huge-page shared mappings.
pub const COREDUMP_FILTER_HUGETLB_SHARED: u32 = 1 << 6;
/// Dump DAX private mappings.
pub const COREDUMP_FILTER_DAX_PRIVATE: u32 = 1 << 7;
/// Dump DAX shared mappings.
pub const COREDUMP_FILTER_DAX_SHARED: u32 = 1 << 8;

/// Default filter (anon private + anon shared).
pub const COREDUMP_FILTER_DEFAULT: u32 = COREDUMP_FILTER_ANON_PRIVATE | COREDUMP_FILTER_ANON_SHARED;

// ---------------------------------------------------------------------------
// ELF note types (for core dumps)
// ---------------------------------------------------------------------------

/// Process status (registers).
pub const NT_PRSTATUS: u32 = 1;
/// Process info (name, pid, etc.).
pub const NT_PRPSINFO: u32 = 3;
/// Task structure.
pub const NT_TASKSTRUCT: u32 = 4;
/// Auxiliary vector.
pub const NT_AUXV: u32 = 6;
/// Signal info.
pub const NT_SIGINFO: u32 = 0x53494749; // "SIGI"
/// File mappings.
pub const NT_FILE: u32 = 0x46494C45; // "FILE"
/// x86 FPU state (x87).
pub const NT_PRXFPREG: u32 = 0x46e62b7f;
/// x86 XSAVE state.
pub const NT_X86_XSTATE: u32 = 0x202;

// ---------------------------------------------------------------------------
// Core dump limits
// ---------------------------------------------------------------------------

/// Maximum core filename length.
pub const CORENAME_MAX_SIZE: usize = 128;
/// Default core pattern.
pub const CORE_PATTERN_DEFAULT: &str = "core";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_bits_powers_of_two() {
        let bits = [
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
        for bit in &bits {
            assert!(bit.is_power_of_two(), "0x{:x}", bit);
        }
    }

    #[test]
    fn test_filter_bits_no_overlap() {
        let bits = [
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
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
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
            NT_PRPSINFO,
            NT_TASKSTRUCT,
            NT_AUXV,
            NT_SIGINFO,
            NT_FILE,
            NT_PRXFPREG,
            NT_X86_XSTATE,
        ];
        for i in 0..notes.len() {
            for j in (i + 1)..notes.len() {
                assert_ne!(notes[i], notes[j]);
            }
        }
    }

    #[test]
    fn test_core_pattern() {
        assert_eq!(CORE_PATTERN_DEFAULT, "core");
    }

    #[test]
    fn test_corename_max() {
        assert_eq!(CORENAME_MAX_SIZE, 128);
    }
}
