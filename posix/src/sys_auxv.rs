//! `<sys/auxv.h>` — auxiliary vector access.
//!
//! Provides `getauxval()` for querying ELF auxiliary vector entries
//! and the `AT_*` constants that identify each entry type.

// ---------------------------------------------------------------------------
// AT_* constants (auxiliary vector entry types)
// ---------------------------------------------------------------------------

/// End of auxiliary vector.
pub const AT_NULL: u64 = 0;

/// Entry is ignored.
pub const AT_IGNORE: u64 = 1;

/// File descriptor of program interpreter.
pub const AT_EXECFD: u64 = 2;

/// Program header table address.
pub const AT_PHDR: u64 = 3;

/// Size of one program header entry.
pub const AT_PHENT: u64 = 4;

/// Number of program header entries.
pub const AT_PHNUM: u64 = 5;

/// System page size.
pub const AT_PAGESZ: u64 = 6;

/// Interpreter base address.
pub const AT_BASE: u64 = 7;

/// Flags.
pub const AT_FLAGS: u64 = 8;

/// Program entry point address.
pub const AT_ENTRY: u64 = 9;

/// Real UID.
pub const AT_UID: u64 = 11;

/// Effective UID.
pub const AT_EUID: u64 = 12;

/// Real GID.
pub const AT_GID: u64 = 13;

/// Effective GID.
pub const AT_EGID: u64 = 14;

/// Processor type string.
pub const AT_PLATFORM: u64 = 15;

/// CPU hardware capabilities bitmask.
pub const AT_HWCAP: u64 = 16;

/// Clock ticks per second.
pub const AT_CLKTCK: u64 = 17;

/// Was FPUCW.
pub const AT_FPUCW: u64 = 18;

/// String identifying actual CPU cache.
pub const AT_DCACHEBSIZE: u64 = 19;

/// Instruction cache block size.
pub const AT_ICACHEBSIZE: u64 = 20;

/// Unified cache block size.
pub const AT_UCACHEBSIZE: u64 = 21;

/// Boolean; was this a setuid/setgid exec?
pub const AT_SECURE: u64 = 23;

/// Filename of the program.
pub const AT_EXECFN: u64 = 31;

/// Address of the vDSO page.
pub const AT_SYSINFO_EHDR: u64 = 33;

/// Random bytes for stack canary.
pub const AT_RANDOM: u64 = 25;

/// Extended hardware capabilities.
pub const AT_HWCAP2: u64 = 26;

// ---------------------------------------------------------------------------
// Re-export getauxval
// ---------------------------------------------------------------------------

pub use crate::crt::getauxval;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_at_null() {
        assert_eq!(AT_NULL, 0);
    }

    #[test]
    fn test_at_pagesz_value() {
        assert_eq!(AT_PAGESZ, 6);
    }

    #[test]
    fn test_at_constants_distinct() {
        let consts = [
            AT_NULL, AT_IGNORE, AT_EXECFD, AT_PHDR, AT_PHENT,
            AT_PHNUM, AT_PAGESZ, AT_BASE, AT_FLAGS, AT_ENTRY,
            AT_UID, AT_EUID, AT_GID, AT_EGID, AT_PLATFORM,
            AT_HWCAP, AT_CLKTCK, AT_FPUCW, AT_DCACHEBSIZE,
            AT_ICACHEBSIZE, AT_UCACHEBSIZE, AT_SECURE,
            AT_RANDOM, AT_HWCAP2, AT_EXECFN, AT_SYSINFO_EHDR,
        ];
        for i in 0..consts.len() {
            for j in (i + 1)..consts.len() {
                assert_ne!(consts[i], consts[j], "AT_* constants must be distinct");
            }
        }
    }

    #[test]
    fn test_getauxval_pagesz() {
        let val = getauxval(AT_PAGESZ);
        // Should return the OS page size (16384 for our OS).
        assert!(val > 0, "AT_PAGESZ should return nonzero");
    }

    #[test]
    fn test_getauxval_unknown() {
        // Unknown type should return 0.
        let val = getauxval(0xFFFF_FFFF);
        assert_eq!(val, 0);
    }
}
