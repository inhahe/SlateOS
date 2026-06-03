//! `<elf.h>` — ELF auxiliary vector type constants.
//!
//! The auxiliary vector (auxv) is an array of tag-value pairs placed
//! on the process stack by the kernel at exec time. The dynamic
//! linker and C runtime use it to discover system capabilities,
//! page size, entry point address, and other runtime information
//! without making syscalls.

// ---------------------------------------------------------------------------
// Auxiliary vector types (a_type field)
// ---------------------------------------------------------------------------

/// End of auxiliary vector (sentinel).
pub const AT_NULL: u64 = 0;
/// Program file descriptor (if used).
pub const AT_EXECFD: u64 = 2;
/// Address of program headers in memory.
pub const AT_PHDR: u64 = 3;
/// Size of one program header entry.
pub const AT_PHENT: u64 = 4;
/// Number of program headers.
pub const AT_PHNUM: u64 = 5;
/// System page size.
pub const AT_PAGESZ: u64 = 6;
/// Base address of interpreter (ld.so).
pub const AT_BASE: u64 = 7;
/// Flags (unused on Linux).
pub const AT_FLAGS: u64 = 8;
/// Entry point of the program.
pub const AT_ENTRY: u64 = 9;
/// Real UID of the process.
pub const AT_UID: u64 = 11;
/// Effective UID of the process.
pub const AT_EUID: u64 = 12;
/// Real GID of the process.
pub const AT_GID: u64 = 13;
/// Effective GID of the process.
pub const AT_EGID: u64 = 14;
/// Processor type string (platform).
pub const AT_PLATFORM: u64 = 15;
/// Hardware capability bitmask.
pub const AT_HWCAP: u64 = 16;
/// Clock ticks per second (CLK_TCK).
pub const AT_CLKTCK: u64 = 17;
/// Set if running as setuid.
pub const AT_SECURE: u64 = 23;
/// Pointer to 16 random bytes.
pub const AT_RANDOM: u64 = 25;
/// Extended hardware capabilities.
pub const AT_HWCAP2: u64 = 26;
/// Filename of the executable.
pub const AT_EXECFN: u64 = 31;
/// Address of the vDSO page.
pub const AT_SYSINFO_EHDR: u64 = 33;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aux_types_distinct() {
        let types = [
            AT_NULL,
            AT_EXECFD,
            AT_PHDR,
            AT_PHENT,
            AT_PHNUM,
            AT_PAGESZ,
            AT_BASE,
            AT_FLAGS,
            AT_ENTRY,
            AT_UID,
            AT_EUID,
            AT_GID,
            AT_EGID,
            AT_PLATFORM,
            AT_HWCAP,
            AT_CLKTCK,
            AT_SECURE,
            AT_RANDOM,
            AT_HWCAP2,
            AT_EXECFN,
            AT_SYSINFO_EHDR,
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
    fn test_common_types() {
        assert_eq!(AT_PHDR, 3);
        assert_eq!(AT_PAGESZ, 6);
        assert_eq!(AT_ENTRY, 9);
        assert_eq!(AT_RANDOM, 25);
    }

    #[test]
    fn test_sysinfo_ehdr() {
        assert_eq!(AT_SYSINFO_EHDR, 33);
    }
}
