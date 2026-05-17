//! `<linux/elfcore.h>` — Core dump note type constants.
//!
//! When a process crashes (SIGSEGV, SIGABRT, etc.) and core dumps are
//! enabled, the kernel writes an ELF core file containing the process
//! state at crash time. The core file uses ELF NOTE sections to store
//! register state, signal info, file mappings, and other metadata.
//! Tools like GDB parse these notes to provide post-mortem debugging.

// ---------------------------------------------------------------------------
// ELF note types (n_type in Elf64_Nhdr)
// ---------------------------------------------------------------------------

/// Process status (registers, signal info).
pub const NT_PRSTATUS: u32 = 1;
/// Floating-point registers.
pub const NT_PRFPREG: u32 = 2;
/// Process info (command name, PID, etc).
pub const NT_PRPSINFO: u32 = 3;
/// Task structure (deprecated).
pub const NT_TASKSTRUCT: u32 = 4;
/// Auxiliary vector.
pub const NT_AUXV: u32 = 6;
/// Signal info (siginfo_t).
pub const NT_SIGINFO: u32 = 0x5349_4749;
/// File mappings.
pub const NT_FILE: u32 = 0x4649_4C45;

// ---------------------------------------------------------------------------
// Architecture-specific note types
// ---------------------------------------------------------------------------

/// x86 extended state (XSAVE).
pub const NT_X86_XSTATE: u32 = 0x202;
/// ARM VFP registers.
pub const NT_ARM_VFP: u32 = 0x400;
/// ARM TLS register.
pub const NT_ARM_TLS: u32 = 0x401;
/// ARM SVE (Scalable Vector Extension) registers.
pub const NT_ARM_SVE: u32 = 0x405;
/// ARM PAC (Pointer Authentication) mask.
pub const NT_ARM_PAC_MASK: u32 = 0x406;

// ---------------------------------------------------------------------------
// Core file limits
// ---------------------------------------------------------------------------

/// Maximum core file size (0 = unlimited, system default).
pub const CORE_UNLIMITED: u64 = 0;
/// Core filename pattern max length.
pub const CORE_PATTERN_MAX: u32 = 128;

// ---------------------------------------------------------------------------
// Coredump filter flags (per /proc/PID/coredump_filter)
// ---------------------------------------------------------------------------

/// Dump anonymous private mappings.
pub const COREDUMP_FILTER_ANON_PRIVATE: u32 = 1 << 0;
/// Dump anonymous shared mappings.
pub const COREDUMP_FILTER_ANON_SHARED: u32 = 1 << 1;
/// Dump file-backed private mappings.
pub const COREDUMP_FILTER_MAPPED_PRIVATE: u32 = 1 << 2;
/// Dump file-backed shared mappings.
pub const COREDUMP_FILTER_MAPPED_SHARED: u32 = 1 << 3;
/// Dump ELF headers (for better GDB support).
pub const COREDUMP_FILTER_ELF_HEADERS: u32 = 1 << 4;
/// Dump hugetlb private mappings.
pub const COREDUMP_FILTER_HUGETLB_PRIVATE: u32 = 1 << 5;
/// Dump hugetlb shared mappings.
pub const COREDUMP_FILTER_HUGETLB_SHARED: u32 = 1 << 6;
/// Dump DAX private mappings.
pub const COREDUMP_FILTER_DAX_PRIVATE: u32 = 1 << 7;
/// Dump DAX shared mappings.
pub const COREDUMP_FILTER_DAX_SHARED: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_types_distinct() {
        let types = [
            NT_PRSTATUS, NT_PRFPREG, NT_PRPSINFO,
            NT_TASKSTRUCT, NT_AUXV, NT_SIGINFO, NT_FILE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_arch_note_types_distinct() {
        let types = [
            NT_X86_XSTATE, NT_ARM_VFP, NT_ARM_TLS,
            NT_ARM_SVE, NT_ARM_PAC_MASK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_filter_flags_no_overlap() {
        let flags = [
            COREDUMP_FILTER_ANON_PRIVATE, COREDUMP_FILTER_ANON_SHARED,
            COREDUMP_FILTER_MAPPED_PRIVATE, COREDUMP_FILTER_MAPPED_SHARED,
            COREDUMP_FILTER_ELF_HEADERS, COREDUMP_FILTER_HUGETLB_PRIVATE,
            COREDUMP_FILTER_HUGETLB_SHARED, COREDUMP_FILTER_DAX_PRIVATE,
            COREDUMP_FILTER_DAX_SHARED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_siginfo_magic() {
        // NT_SIGINFO = "SIGI" in ASCII
        assert_eq!(NT_SIGINFO, 0x5349_4749);
    }

    #[test]
    fn test_file_magic() {
        // NT_FILE = "FILE" in ASCII
        assert_eq!(NT_FILE, 0x4649_4C45);
    }
}
