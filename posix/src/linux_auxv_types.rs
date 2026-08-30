//! `<elf.h>` / `<sys/auxv.h>` — Auxiliary vector entry types.
//!
//! The auxiliary vector (`auxv`) is passed by the kernel to a new
//! process on the stack at `execve()` time.  It provides runtime
//! information such as page size, entry point, and hardware
//! capabilities.  These constants identify each entry type.

// ---------------------------------------------------------------------------
// Auxiliary vector types (AT_*)
// ---------------------------------------------------------------------------

/// End of vector (sentinel).
pub const AT_NULL: u32 = 0;
/// Entry should be ignored.
pub const AT_IGNORE: u32 = 1;
/// File descriptor of the program being executed.
pub const AT_EXECFD: u32 = 2;
/// Program headers address.
pub const AT_PHDR: u32 = 3;
/// Size of a program header entry.
pub const AT_PHENT: u32 = 4;
/// Number of program headers.
pub const AT_PHNUM: u32 = 5;
/// System page size.
pub const AT_PAGESZ: u32 = 6;
/// Interpreter (dynamic linker) base address.
pub const AT_BASE: u32 = 7;
/// Flags (unused on Linux).
pub const AT_FLAGS: u32 = 8;
/// Entry point of the program.
pub const AT_ENTRY: u32 = 9;
/// Program is not ELF (set for a.out binaries).
pub const AT_NOTELF: u32 = 10;
/// Real UID of the process.
pub const AT_UID: u32 = 11;
/// Effective UID of the process.
pub const AT_EUID: u32 = 12;
/// Real GID of the process.
pub const AT_GID: u32 = 13;
/// Effective GID of the process.
pub const AT_EGID: u32 = 14;
/// CPU platform string.
pub const AT_PLATFORM: u32 = 15;
/// Hardware capabilities bitmask.
pub const AT_HWCAP: u32 = 16;
/// Clock ticks per second.
pub const AT_CLKTCK: u32 = 17;
/// FPU control word (was AT_FPUCW).
pub const AT_FPUCW: u32 = 18;
/// Data cache block size.
pub const AT_DCACHEBSIZE: u32 = 19;
/// Instruction cache block size.
pub const AT_ICACHEBSIZE: u32 = 20;
/// Unified cache block size.
pub const AT_UCACHEBSIZE: u32 = 21;
/// Secure mode boolean.
pub const AT_SECURE: u32 = 23;
/// Base platform string.
pub const AT_BASE_PLATFORM: u32 = 24;
/// Address of 16 random bytes.
pub const AT_RANDOM: u32 = 25;
/// Extended hardware capabilities.
pub const AT_HWCAP2: u32 = 26;
/// Filename of the executed program.
pub const AT_EXECFN: u32 = 31;
/// Address of the vDSO page.
pub const AT_SYSINFO_EHDR: u32 = 33;
/// Minimum stack size for signal delivery.
pub const AT_MINSIGSTKSZ: u32 = 51;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            AT_NULL,
            AT_IGNORE,
            AT_EXECFD,
            AT_PHDR,
            AT_PHENT,
            AT_PHNUM,
            AT_PAGESZ,
            AT_BASE,
            AT_FLAGS,
            AT_ENTRY,
            AT_NOTELF,
            AT_UID,
            AT_EUID,
            AT_GID,
            AT_EGID,
            AT_PLATFORM,
            AT_HWCAP,
            AT_CLKTCK,
            AT_FPUCW,
            AT_DCACHEBSIZE,
            AT_ICACHEBSIZE,
            AT_UCACHEBSIZE,
            AT_SECURE,
            AT_BASE_PLATFORM,
            AT_RANDOM,
            AT_HWCAP2,
            AT_EXECFN,
            AT_SYSINFO_EHDR,
            AT_MINSIGSTKSZ,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_null_is_zero() {
        assert_eq!(AT_NULL, 0);
    }

    #[test]
    fn test_pagesz_value() {
        assert_eq!(AT_PAGESZ, 6);
    }

    #[test]
    fn test_entry_value() {
        assert_eq!(AT_ENTRY, 9);
    }

    #[test]
    fn test_hwcap_value() {
        assert_eq!(AT_HWCAP, 16);
    }

    #[test]
    fn test_random_value() {
        assert_eq!(AT_RANDOM, 25);
    }

    #[test]
    fn test_sysinfo_ehdr() {
        assert_eq!(AT_SYSINFO_EHDR, 33);
    }

    #[test]
    fn test_minsigstksz() {
        assert_eq!(AT_MINSIGSTKSZ, 51);
    }
}
