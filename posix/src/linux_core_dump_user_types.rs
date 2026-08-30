//! `<sys/procfs.h>` / `<elf.h>` — ELF core dump constants.
//!
//! When a process is killed by a fatal signal (or calls abort()),
//! the kernel writes an ELF core file describing its address space
//! and register state at the time of death. Debuggers read these
//! note segments to reconstruct the crash.

// ---------------------------------------------------------------------------
// ELF e_type for core files
// ---------------------------------------------------------------------------

/// `e_type` value identifying a core dump.
pub const ET_CORE: u16 = 4;

// ---------------------------------------------------------------------------
// Note types found in PT_NOTE segments of core files
// ---------------------------------------------------------------------------

/// General-purpose registers + signal info (struct elf_prstatus).
pub const NT_PRSTATUS: u32 = 1;
/// Floating-point register set.
pub const NT_FPREGSET: u32 = 2;
/// Process metadata (pid, command line, state) — struct elf_prpsinfo.
pub const NT_PRPSINFO: u32 = 3;
/// Full kernel task_struct (rarely used in userspace cores).
pub const NT_TASKSTRUCT: u32 = 4;
/// ELF auxiliary vector at process start.
pub const NT_AUXV: u32 = 6;
/// Mapped-file table — "NT_FILE" ("FILE" in ASCII).
pub const NT_FILE: u32 = 0x4649_4c45;
/// siginfo_t of the fatal signal.
pub const NT_SIGINFO: u32 = 0x5349_4749;
/// x86 extended FP state (XSAVE area).
pub const NT_X86_XSTATE: u32 = 0x202;
/// ARM vector floating point.
pub const NT_ARM_VFP: u32 = 0x400;
/// ARM TLS register.
pub const NT_ARM_TLS: u32 = 0x401;

// ---------------------------------------------------------------------------
// Note section name used by all of the above
// ---------------------------------------------------------------------------

/// Conventional name field for kernel-emitted notes ("CORE\0").
pub const NT_CORE_NAME: &str = "CORE";
/// LINUX-prefixed notes (architecture-specific extras).
pub const NT_LINUX_NAME: &str = "LINUX";

// ---------------------------------------------------------------------------
// /proc tunables that control core dumping
// ---------------------------------------------------------------------------

pub const PROC_SYS_CORE_PATTERN: &str = "/proc/sys/kernel/core_pattern";
pub const PROC_SYS_CORE_USES_PID: &str = "/proc/sys/kernel/core_uses_pid";
pub const PROC_SYS_SUID_DUMPABLE: &str = "/proc/sys/fs/suid_dumpable";
pub const PROC_SELF_COREDUMP_FILTER: &str = "/proc/self/coredump_filter";

/// Default core_pattern on most distros — just "core".
pub const CORE_PATTERN_DEFAULT: &str = "core";

// ---------------------------------------------------------------------------
// coredump_filter bits (which memory regions get dumped)
// ---------------------------------------------------------------------------

pub const MMF_DUMP_ANON_PRIVATE: u32 = 1 << 0;
pub const MMF_DUMP_ANON_SHARED: u32 = 1 << 1;
pub const MMF_DUMP_MAPPED_PRIVATE: u32 = 1 << 2;
pub const MMF_DUMP_MAPPED_SHARED: u32 = 1 << 3;
pub const MMF_DUMP_ELF_HEADERS: u32 = 1 << 4;
pub const MMF_DUMP_HUGETLB_PRIVATE: u32 = 1 << 5;
pub const MMF_DUMP_HUGETLB_SHARED: u32 = 1 << 6;
pub const MMF_DUMP_DAX_PRIVATE: u32 = 1 << 7;
pub const MMF_DUMP_DAX_SHARED: u32 = 1 << 8;

/// Default filter mask — anon + ELF headers + HugeTLB private.
pub const COREDUMP_FILTER_DEFAULT: u32 =
    MMF_DUMP_ANON_PRIVATE | MMF_DUMP_ANON_SHARED | MMF_DUMP_ELF_HEADERS | MMF_DUMP_HUGETLB_PRIVATE;

// ---------------------------------------------------------------------------
// suid_dumpable modes
// ---------------------------------------------------------------------------

pub const SUID_DUMP_DISABLE: u32 = 0;
pub const SUID_DUMP_USER: u32 = 1;
pub const SUID_DUMP_ROOT: u32 = 2;

// ---------------------------------------------------------------------------
// RLIMIT_CORE special values
// ---------------------------------------------------------------------------

/// RLIMIT_CORE resource number.
pub const RLIMIT_CORE: u32 = 4;
/// Unlimited core size.
pub const RLIM_INFINITY: u64 = u64::MAX;
/// Setting RLIMIT_CORE to 0 disables dumping entirely.
pub const CORE_DISABLED_LIMIT: u64 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_et_core_is_4() {
        assert_eq!(ET_CORE, 4);
    }

    #[test]
    fn test_classic_note_types_dense_small() {
        // NT_PRSTATUS..NT_TASKSTRUCT are 1..=4 (NT_PLATFORM = 5 omitted here).
        assert_eq!(NT_PRSTATUS, 1);
        assert_eq!(NT_FPREGSET, 2);
        assert_eq!(NT_PRPSINFO, 3);
        assert_eq!(NT_TASKSTRUCT, 4);
        assert_eq!(NT_AUXV, 6);
    }

    #[test]
    fn test_nt_file_is_ascii_file() {
        // 0x46 0x49 0x4c 0x45 == "FILE" (big-endian when printed).
        assert_eq!(NT_FILE.to_be_bytes(), *b"FILE");
    }

    #[test]
    fn test_nt_siginfo_is_ascii_sigi() {
        // 0x53 0x49 0x47 0x49 == "SIGI".
        assert_eq!(NT_SIGINFO.to_be_bytes(), *b"SIGI");
    }

    #[test]
    fn test_note_names_distinct() {
        assert_ne!(NT_CORE_NAME, NT_LINUX_NAME);
        assert_eq!(NT_CORE_NAME.len(), 4);
        assert_eq!(NT_LINUX_NAME.len(), 5);
    }

    #[test]
    fn test_proc_paths_well_formed() {
        for p in [
            PROC_SYS_CORE_PATTERN,
            PROC_SYS_CORE_USES_PID,
            PROC_SYS_SUID_DUMPABLE,
        ] {
            assert!(p.starts_with("/proc/sys/"));
        }
        assert!(PROC_SELF_COREDUMP_FILTER.starts_with("/proc/self/"));
        assert_eq!(CORE_PATTERN_DEFAULT, "core");
    }

    #[test]
    fn test_mmf_dump_bits_distinct_single_bit() {
        let f = [
            MMF_DUMP_ANON_PRIVATE,
            MMF_DUMP_ANON_SHARED,
            MMF_DUMP_MAPPED_PRIVATE,
            MMF_DUMP_MAPPED_SHARED,
            MMF_DUMP_ELF_HEADERS,
            MMF_DUMP_HUGETLB_PRIVATE,
            MMF_DUMP_HUGETLB_SHARED,
            MMF_DUMP_DAX_PRIVATE,
            MMF_DUMP_DAX_SHARED,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        // All nine bits OR together cover the low nine bits.
        let all = f.iter().copied().fold(0u32, |a, b| a | b);
        assert_eq!(all, 0x1FF);
    }

    #[test]
    fn test_default_filter_includes_anon_and_elf() {
        assert_ne!(COREDUMP_FILTER_DEFAULT & MMF_DUMP_ANON_PRIVATE, 0);
        assert_ne!(COREDUMP_FILTER_DEFAULT & MMF_DUMP_ANON_SHARED, 0);
        assert_ne!(COREDUMP_FILTER_DEFAULT & MMF_DUMP_ELF_HEADERS, 0);
        // Mapped files are NOT dumped by default (privacy + size).
        assert_eq!(COREDUMP_FILTER_DEFAULT & MMF_DUMP_MAPPED_PRIVATE, 0);
        assert_eq!(COREDUMP_FILTER_DEFAULT & MMF_DUMP_MAPPED_SHARED, 0);
    }

    #[test]
    fn test_suid_dump_modes_dense_0_to_2() {
        assert_eq!(SUID_DUMP_DISABLE, 0);
        assert_eq!(SUID_DUMP_USER, 1);
        assert_eq!(SUID_DUMP_ROOT, 2);
    }

    #[test]
    fn test_rlimit_core_constants() {
        assert_eq!(RLIMIT_CORE, 4);
        assert_eq!(RLIM_INFINITY, u64::MAX);
        assert_eq!(CORE_DISABLED_LIMIT, 0);
    }
}
