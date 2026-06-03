//! `<linux/auxvec.h>` — Auxiliary vector (AT_*) constants.
//!
//! The auxiliary vector is an array of key-value pairs passed to a
//! new process on the stack during exec(). It provides information
//! the dynamic linker and C library need: page size, entry point,
//! UID/GID, vDSO address, hardware capabilities, random bytes for
//! stack canaries, etc. The kernel populates it; userspace reads it
//! via getauxval() or by walking the stack after envp[].

// ---------------------------------------------------------------------------
// Auxiliary vector entry types (AT_*)
// ---------------------------------------------------------------------------

/// End of auxiliary vector.
pub const AT_NULL: u32 = 0;
/// Ignored entry.
pub const AT_IGNORE: u32 = 1;
/// File descriptor of the program being executed.
pub const AT_EXECFD: u32 = 2;
/// Program header table address.
pub const AT_PHDR: u32 = 3;
/// Size of one program header entry.
pub const AT_PHENT: u32 = 4;
/// Number of program header entries.
pub const AT_PHNUM: u32 = 5;
/// System page size.
pub const AT_PAGESZ: u32 = 6;
/// Interpreter (dynamic linker) base address.
pub const AT_BASE: u32 = 7;
/// Flags (unused, always 0).
pub const AT_FLAGS: u32 = 8;
/// Entry point of the program.
pub const AT_ENTRY: u32 = 9;
/// Not ELF (interpreter is a.out format).
pub const AT_NOTELF: u32 = 10;
/// Real UID of the process.
pub const AT_UID: u32 = 11;
/// Effective UID.
pub const AT_EUID: u32 = 12;
/// Real GID.
pub const AT_GID: u32 = 13;
/// Effective GID.
pub const AT_EGID: u32 = 14;
/// CPU/platform string pointer.
pub const AT_PLATFORM: u32 = 15;
/// Hardware capabilities bitmask.
pub const AT_HWCAP: u32 = 16;
/// Clock ticks per second (for times()).
pub const AT_CLKTCK: u32 = 17;
/// Was process executed setuid/setgid?
pub const AT_SECURE: u32 = 23;
/// Pointer to base platform string.
pub const AT_BASE_PLATFORM: u32 = 24;
/// 16 random bytes (for stack canary/ASLR seed).
pub const AT_RANDOM: u32 = 25;
/// Extended hardware capabilities.
pub const AT_HWCAP2: u32 = 26;
/// Filename of the executed program.
pub const AT_EXECFN: u32 = 31;
/// vDSO entry point (address of the vDSO shared object).
pub const AT_SYSINFO_EHDR: u32 = 33;
/// Minimum mmap address hint.
pub const AT_MINSIGSTKSZ: u32 = 51;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_at_values_distinct() {
        let vals = [
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
            AT_SECURE,
            AT_BASE_PLATFORM,
            AT_RANDOM,
            AT_HWCAP2,
            AT_EXECFN,
            AT_SYSINFO_EHDR,
            AT_MINSIGSTKSZ,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_at_null_is_zero() {
        assert_eq!(AT_NULL, 0);
    }

    #[test]
    fn test_uid_gid_entries() {
        // UID/GID entries should be consecutive
        assert_eq!(AT_EUID, AT_UID + 1);
        assert_eq!(AT_GID, AT_EUID + 1);
        assert_eq!(AT_EGID, AT_GID + 1);
    }
}
