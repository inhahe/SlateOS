//! `<linux/auxvec.h>` — Auxiliary vector entry types.
//!
//! The auxiliary vector (auxv) is placed on the stack by the kernel
//! during exec and provides runtime information to the dynamic linker
//! (ld-linux.so) and libc. Accessed via `getauxval()`.

pub use crate::crt::getauxval;

// ---------------------------------------------------------------------------
// AT_* auxiliary vector types
// ---------------------------------------------------------------------------

/// End of vector.
pub const AT_NULL: u64 = 0;
/// Entry should be ignored.
pub const AT_IGNORE: u64 = 1;
/// File descriptor of program.
pub const AT_EXECFD: u64 = 2;
/// Program header table address.
pub const AT_PHDR: u64 = 3;
/// Size of program header entry.
pub const AT_PHENT: u64 = 4;
/// Number of program headers.
pub const AT_PHNUM: u64 = 5;
/// System page size.
pub const AT_PAGESZ: u64 = 6;
/// Interpreter base address.
pub const AT_BASE: u64 = 7;
/// Flags.
pub const AT_FLAGS: u64 = 8;
/// Entry point of program.
pub const AT_ENTRY: u64 = 9;
/// Program is not ELF.
pub const AT_NOTELF: u64 = 10;
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
/// Hardware capabilities (hwcap).
pub const AT_HWCAP: u64 = 16;
/// Clock ticks per second.
pub const AT_CLKTCK: u64 = 17;
/// Secure mode (set-uid, set-gid, etc.).
pub const AT_SECURE: u64 = 23;
/// Base platform string.
pub const AT_BASE_PLATFORM: u64 = 24;
/// Address of 16 random bytes.
pub const AT_RANDOM: u64 = 25;
/// Hardware capabilities 2 (hwcap2).
pub const AT_HWCAP2: u64 = 26;
/// Filename of executed program.
pub const AT_EXECFN: u64 = 31;
/// vDSO base address.
pub const AT_SYSINFO_EHDR: u64 = 33;
/// Minimum stack alignment.
pub const AT_MINSIGSTKSZ: u64 = 51;

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
    fn test_at_types_distinct() {
        let types = [
            AT_NULL, AT_IGNORE, AT_EXECFD, AT_PHDR,
            AT_PHENT, AT_PHNUM, AT_PAGESZ, AT_BASE,
            AT_FLAGS, AT_ENTRY, AT_NOTELF, AT_UID,
            AT_EUID, AT_GID, AT_EGID, AT_PLATFORM,
            AT_HWCAP, AT_CLKTCK, AT_SECURE, AT_RANDOM,
            AT_HWCAP2, AT_EXECFN, AT_SYSINFO_EHDR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pagesz_entry() {
        assert_eq!(AT_PAGESZ, 6);
    }

    #[test]
    fn test_secure() {
        assert_eq!(AT_SECURE, 23);
    }

    #[test]
    fn test_getauxval_reexport() {
        // Query for page size — stub returns 0.
        let val = getauxval(AT_PAGESZ);
        let _ = val; // Accept any return value.
    }
}
