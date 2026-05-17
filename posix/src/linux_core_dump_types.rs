//! `<linux/coredump.h>` — Core dump constants.
//!
//! When a process crashes (segfault, abort, etc.), the kernel can
//! write a core dump: an ELF file containing the process's memory
//! image, register state, signal information, and other metadata.
//! Core dumps are controlled per-process (via ulimit -c or
//! /proc/<pid>/coredump_filter) and system-wide (via
//! /proc/sys/kernel/core_pattern, which can pipe to a handler).

// ---------------------------------------------------------------------------
// Core dump filter bits (coredump_filter)
// ---------------------------------------------------------------------------

/// Dump anonymous private mappings.
pub const COREDUMP_FILTER_ANON_PRIVATE: u32 = 0x01;
/// Dump anonymous shared mappings.
pub const COREDUMP_FILTER_ANON_SHARED: u32 = 0x02;
/// Dump file-backed private mappings.
pub const COREDUMP_FILTER_MAPPED_PRIVATE: u32 = 0x04;
/// Dump file-backed shared mappings.
pub const COREDUMP_FILTER_MAPPED_SHARED: u32 = 0x08;
/// Dump ELF headers (for identifying shared libraries).
pub const COREDUMP_FILTER_ELF_HEADERS: u32 = 0x10;
/// Dump huge pages (private).
pub const COREDUMP_FILTER_HUGETLB_PRIVATE: u32 = 0x20;
/// Dump huge pages (shared).
pub const COREDUMP_FILTER_HUGETLB_SHARED: u32 = 0x40;
/// Dump DAX (direct-access) private mappings.
pub const COREDUMP_FILTER_DAX_PRIVATE: u32 = 0x80;

/// Default filter (anonymous private + anonymous shared).
pub const COREDUMP_FILTER_DEFAULT: u32 = COREDUMP_FILTER_ANON_PRIVATE
    | COREDUMP_FILTER_ANON_SHARED;

// ---------------------------------------------------------------------------
// Core dump ELF note types (NT_*)
// ---------------------------------------------------------------------------

/// Process status (registers, signal info).
pub const NT_PRSTATUS: u32 = 1;
/// Floating-point registers.
pub const NT_PRFPREG: u32 = 2;
/// Process info (pid, ppid, pgrp, etc.).
pub const NT_PRPSINFO: u32 = 3;
/// Platform-specific auxiliary info.
pub const NT_AUXV: u32 = 6;
/// Siginfo (signal that caused the dump).
pub const NT_SIGINFO: u32 = 0x5349_4749;
/// File mappings (which files were mapped).
pub const NT_FILE: u32 = 0x4649_4C45;

// ---------------------------------------------------------------------------
// Core dump size limits
// ---------------------------------------------------------------------------

/// Unlimited core dump size.
pub const CORE_UNLIMITED: u64 = u64::MAX;
/// No core dump (size limit = 0).
pub const CORE_DISABLED: u64 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_bits_no_overlap() {
        let bits = [
            COREDUMP_FILTER_ANON_PRIVATE, COREDUMP_FILTER_ANON_SHARED,
            COREDUMP_FILTER_MAPPED_PRIVATE, COREDUMP_FILTER_MAPPED_SHARED,
            COREDUMP_FILTER_ELF_HEADERS, COREDUMP_FILTER_HUGETLB_PRIVATE,
            COREDUMP_FILTER_HUGETLB_SHARED, COREDUMP_FILTER_DAX_PRIVATE,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_default_filter() {
        assert_ne!(COREDUMP_FILTER_DEFAULT & COREDUMP_FILTER_ANON_PRIVATE, 0);
        assert_ne!(COREDUMP_FILTER_DEFAULT & COREDUMP_FILTER_ANON_SHARED, 0);
        assert_eq!(COREDUMP_FILTER_DEFAULT & COREDUMP_FILTER_MAPPED_PRIVATE, 0);
    }

    #[test]
    fn test_note_types_distinct() {
        let types = [NT_PRSTATUS, NT_PRFPREG, NT_PRPSINFO, NT_AUXV, NT_SIGINFO, NT_FILE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
