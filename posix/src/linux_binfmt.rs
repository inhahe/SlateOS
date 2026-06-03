//! `<linux/binfmts.h>` — Binary format handler constants.
//!
//! The kernel's binary format subsystem (binfmt) is responsible for
//! recognizing executable file formats and setting up their execution
//! environment. Linux supports ELF, scripts (#!), flat binaries, and
//! misc formats (via binfmt_misc). The loader reads magic bytes,
//! sets up memory mappings, and transfers control.

// ---------------------------------------------------------------------------
// Binary format magic numbers
// ---------------------------------------------------------------------------

/// ELF magic: 0x7F 'E' 'L' 'F'.
pub const ELFMAG: [u8; 4] = [0x7F, b'E', b'L', b'F'];
/// Script magic: '#' '!'.
pub const SCRIPT_MAG: [u8; 2] = [b'#', b'!'];

// ---------------------------------------------------------------------------
// binfmt limits
// ---------------------------------------------------------------------------

/// Maximum interpreter path length (for #! scripts).
pub const BINPRM_BUF_SIZE: u32 = 256;
/// Maximum number of bytes examined for magic.
pub const BINPRM_HEADER_SIZE: u32 = 128;
/// Maximum recursion depth for script interpreters.
pub const BINPRM_MAX_RECURSION: u32 = 4;

// ---------------------------------------------------------------------------
// Stack/argument limits
// ---------------------------------------------------------------------------

/// Maximum number of arguments to execve.
pub const MAX_ARG_STRINGS: u32 = 0x7FFF_FFFF;
/// Maximum total argument+environment size (default, 1/4 of stack).
pub const MAX_ARG_STRLEN: u32 = 131072;
/// Default stack size for new processes (8 MiB).
pub const DEFAULT_STACK_SIZE: u32 = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Auxiliary vector types (AT_* passed to ELF programs on stack)
// ---------------------------------------------------------------------------

/// End of auxiliary vector.
pub const AT_NULL: u32 = 0;
/// File descriptor of program (if loaded via fexecve).
pub const AT_EXECFD: u32 = 2;
/// Program headers address.
pub const AT_PHDR: u32 = 3;
/// Size of each program header entry.
pub const AT_PHENT: u32 = 4;
/// Number of program headers.
pub const AT_PHNUM: u32 = 5;
/// System page size.
pub const AT_PAGESZ: u32 = 6;
/// Interpreter base address.
pub const AT_BASE: u32 = 7;
/// Flags (unused, always 0).
pub const AT_FLAGS: u32 = 8;
/// Entry point of the program.
pub const AT_ENTRY: u32 = 9;
/// Real UID.
pub const AT_UID: u32 = 11;
/// Effective UID.
pub const AT_EUID: u32 = 12;
/// Real GID.
pub const AT_GID: u32 = 13;
/// Effective GID.
pub const AT_EGID: u32 = 14;
/// CPU type (for multi-arch).
pub const AT_PLATFORM: u32 = 15;
/// Hardware capabilities bitmask.
pub const AT_HWCAP: u32 = 16;
/// Clock ticks per second.
pub const AT_CLKTCK: u32 = 17;
/// Secure mode (setuid/setgid).
pub const AT_SECURE: u32 = 23;
/// Pointer to vDSO.
pub const AT_SYSINFO_EHDR: u32 = 33;
/// 16 bytes of random data.
pub const AT_RANDOM: u32 = 25;
/// Extended hardware capabilities.
pub const AT_HWCAP2: u32 = 26;
/// Filename of the executable.
pub const AT_EXECFN: u32 = 31;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_magic() {
        assert_eq!(ELFMAG, [0x7F, b'E', b'L', b'F']);
    }

    #[test]
    fn test_script_magic() {
        assert_eq!(SCRIPT_MAG, [b'#', b'!']);
    }

    #[test]
    fn test_limits_positive() {
        assert!(BINPRM_BUF_SIZE > 0);
        assert!(BINPRM_HEADER_SIZE > 0);
        assert!(BINPRM_MAX_RECURSION > 0);
        assert!(MAX_ARG_STRLEN > 0);
        assert!(DEFAULT_STACK_SIZE > 0);
    }

    #[test]
    fn test_at_types_distinct() {
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
    fn test_at_null_is_zero() {
        assert_eq!(AT_NULL, 0);
    }
}
