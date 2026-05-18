//! `<elf.h>` — ELF note section type constants.
//!
//! ELF note sections (`.note.*`) carry auxiliary metadata in ELF
//! binaries and core dumps.  These constants define note types
//! for GNU, Linux core dumps, and other standard note owners.

// ---------------------------------------------------------------------------
// GNU note types (owner "GNU")
// ---------------------------------------------------------------------------

/// ABI tag — identifies minimum kernel/OS version.
pub const NT_GNU_ABI_TAG: u32 = 1;
/// Hardware capabilities (hwcap).
pub const NT_GNU_HWCAP: u32 = 2;
/// Build ID — unique binary identifier.
pub const NT_GNU_BUILD_ID: u32 = 3;
/// Gold version — linker version.
pub const NT_GNU_GOLD_VERSION: u32 = 4;
/// Property note — GNU program properties (CET, BTI).
pub const NT_GNU_PROPERTY_TYPE_0: u32 = 5;

// ---------------------------------------------------------------------------
// GNU ABI OS values (within NT_GNU_ABI_TAG)
// ---------------------------------------------------------------------------

/// GNU ABI tag: Linux.
pub const ELF_NOTE_OS_LINUX: u32 = 0;
/// GNU ABI tag: GNU (Hurd).
pub const ELF_NOTE_OS_GNU: u32 = 1;
/// GNU ABI tag: Solaris.
pub const ELF_NOTE_OS_SOLARIS2: u32 = 2;
/// GNU ABI tag: FreeBSD.
pub const ELF_NOTE_OS_FREEBSD: u32 = 3;

// ---------------------------------------------------------------------------
// Core dump note types (owner "CORE" or "LINUX")
// ---------------------------------------------------------------------------

/// Process status (prstatus).
pub const NT_PRSTATUS: u32 = 1;
/// Floating-point registers (fpregset).
pub const NT_FPREGSET: u32 = 2;
/// Process info (prpsinfo).
pub const NT_PRPSINFO: u32 = 3;
/// Task structure.
pub const NT_TASKSTRUCT: u32 = 4;
/// Auxiliary vector (auxv).
pub const NT_AUXV: u32 = 6;
/// Signal info.
pub const NT_SIGINFO: u32 = 0x53494749;
/// File mappings.
pub const NT_FILE: u32 = 0x46494C45;
/// x86 XSAVE extended state.
pub const NT_X86_XSTATE: u32 = 0x202;
/// ARM VFP registers.
pub const NT_ARM_VFP: u32 = 0x400;
/// ARM TLS registers.
pub const NT_ARM_TLS: u32 = 0x401;
/// ARM hardware breakpoint registers.
pub const NT_ARM_HW_BREAK: u32 = 0x402;
/// ARM hardware watchpoint registers.
pub const NT_ARM_HW_WATCH: u32 = 0x403;

// ---------------------------------------------------------------------------
// Note section alignment
// ---------------------------------------------------------------------------

/// Standard alignment for ELF note sections (4 bytes).
pub const ELF_NOTE_ALIGN: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gnu_note_types_distinct() {
        let types = [
            NT_GNU_ABI_TAG, NT_GNU_HWCAP, NT_GNU_BUILD_ID,
            NT_GNU_GOLD_VERSION, NT_GNU_PROPERTY_TYPE_0,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_abi_tag_is_one() {
        assert_eq!(NT_GNU_ABI_TAG, 1);
    }

    #[test]
    fn test_build_id_is_three() {
        assert_eq!(NT_GNU_BUILD_ID, 3);
    }

    #[test]
    fn test_os_values_distinct() {
        let oses = [
            ELF_NOTE_OS_LINUX, ELF_NOTE_OS_GNU,
            ELF_NOTE_OS_SOLARIS2, ELF_NOTE_OS_FREEBSD,
        ];
        for i in 0..oses.len() {
            for j in (i + 1)..oses.len() {
                assert_ne!(oses[i], oses[j]);
            }
        }
    }

    #[test]
    fn test_linux_is_zero() {
        assert_eq!(ELF_NOTE_OS_LINUX, 0);
    }

    #[test]
    fn test_core_note_types_distinct() {
        let types = [
            NT_PRSTATUS, NT_FPREGSET, NT_PRPSINFO,
            NT_TASKSTRUCT, NT_AUXV, NT_SIGINFO, NT_FILE,
            NT_X86_XSTATE, NT_ARM_VFP, NT_ARM_TLS,
            NT_ARM_HW_BREAK, NT_ARM_HW_WATCH,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_note_align() {
        assert_eq!(ELF_NOTE_ALIGN, 4);
        assert!(ELF_NOTE_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_siginfo_magic() {
        assert_eq!(NT_SIGINFO, 0x53494749); // "SIGI" in ASCII
    }

    #[test]
    fn test_file_magic() {
        assert_eq!(NT_FILE, 0x46494C45); // "FILE" in ASCII
    }
}
