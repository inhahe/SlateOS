//! `<linux/auxvec.h>` — Additional auxiliary vector constants.
//!
//! Supplementary auxiliary vector constants covering additional
//! AT_* entries for program startup information.

// ---------------------------------------------------------------------------
// Auxiliary vector entry types (AT_*)
// ---------------------------------------------------------------------------

/// Null terminator.
pub const AT_NULL: u64 = 0;
/// Program header table address.
pub const AT_PHDR: u64 = 3;
/// Size of program header entry.
pub const AT_PHENT: u64 = 4;
/// Number of program headers.
pub const AT_PHNUM: u64 = 5;
/// System page size.
pub const AT_PAGESZ: u64 = 6;
/// Base address of interpreter.
pub const AT_BASE: u64 = 7;
/// Flags.
pub const AT_FLAGS: u64 = 8;
/// Program entry point.
pub const AT_ENTRY: u64 = 9;
/// Real UID.
pub const AT_UID: u64 = 11;
/// Effective UID.
pub const AT_EUID: u64 = 12;
/// Real GID.
pub const AT_GID: u64 = 13;
/// Effective GID.
pub const AT_EGID: u64 = 14;
/// Platform string.
pub const AT_PLATFORM: u64 = 15;
/// Hardware capabilities.
pub const AT_HWCAP: u64 = 16;
/// Clock ticks per second.
pub const AT_CLKTCK: u64 = 17;
/// Secure mode boolean.
pub const AT_SECURE: u64 = 23;
/// Base platform string.
pub const AT_BASE_PLATFORM: u64 = 24;
/// Random bytes address (16 bytes).
pub const AT_RANDOM: u64 = 25;
/// Hardware capabilities 2.
pub const AT_HWCAP2: u64 = 26;
/// Filename of program.
pub const AT_EXECFN: u64 = 31;
/// vDSO entry point.
pub const AT_SYSINFO: u64 = 32;
/// vDSO ELF header address.
pub const AT_SYSINFO_EHDR: u64 = 33;
/// Minimum alignment of mmap.
pub const AT_MINSIGSTKSZ: u64 = 51;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entries_distinct() {
        let entries = [
            AT_NULL,
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
            AT_BASE_PLATFORM,
            AT_RANDOM,
            AT_HWCAP2,
            AT_EXECFN,
            AT_SYSINFO,
            AT_SYSINFO_EHDR,
            AT_MINSIGSTKSZ,
        ];
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                assert_ne!(entries[i], entries[j]);
            }
        }
    }

    #[test]
    fn test_null_is_zero() {
        assert_eq!(AT_NULL, 0);
    }

    #[test]
    fn test_vdso_entries() {
        assert_eq!(AT_SYSINFO, 32);
        assert_eq!(AT_SYSINFO_EHDR, 33);
    }

    #[test]
    fn test_uid_gid_entries() {
        assert!(AT_UID < AT_EUID);
        assert!(AT_GID < AT_EGID);
    }

    #[test]
    fn test_hwcap_entries() {
        assert!(AT_HWCAP < AT_HWCAP2);
    }
}
