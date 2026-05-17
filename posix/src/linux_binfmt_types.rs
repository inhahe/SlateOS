//! `<linux/binfmts.h>` — Binary format handler constants.
//!
//! The binfmt subsystem handles executing different binary formats.
//! When exec() is called, the kernel tries each registered handler:
//! ELF (most common), script (#! interpreter), flat binary, misc
//! (user-registered via binfmt_misc). Each handler checks the file's
//! magic bytes and, if it matches, sets up the new process's memory
//! layout, loads segments, and transfers control to the entry point
//! or interpreter.

// ---------------------------------------------------------------------------
// Binary format types
// ---------------------------------------------------------------------------

/// ELF binary format.
pub const BINFMT_ELF: u32 = 0;
/// Script format (#!interpreter).
pub const BINFMT_SCRIPT: u32 = 1;
/// Miscellaneous format (binfmt_misc, user-registered).
pub const BINFMT_MISC: u32 = 2;
/// Flat binary format (uClinux, no MMU).
pub const BINFMT_FLAT: u32 = 3;
/// a.out format (legacy, mostly removed).
pub const BINFMT_AOUT: u32 = 4;

// ---------------------------------------------------------------------------
// exec() limits
// ---------------------------------------------------------------------------

/// Maximum argument+environment data size (default: 32 pages = 128 KiB).
pub const MAX_ARG_STRLEN: u32 = 131072;
/// Maximum number of arguments.
pub const MAX_ARG_STRINGS: u32 = 0x7FFF_FFFF;
/// Maximum length of interpreter path in #! scripts.
pub const BINPRM_BUF_SIZE: u32 = 256;

// ---------------------------------------------------------------------------
// binfmt flags
// ---------------------------------------------------------------------------

/// Binary needs executable stack.
pub const BINFMT_FLAG_EXEC_STACK: u32 = 0x01;
/// Binary requests ASLR (address space layout randomization).
pub const BINFMT_FLAG_ASLR: u32 = 0x02;
/// Binary has been read (credentials checked).
pub const BINFMT_FLAG_CREDENTIALS: u32 = 0x04;
/// Binary is setuid/setgid.
pub const BINFMT_FLAG_SUID: u32 = 0x08;
/// Binary uses security module labels.
pub const BINFMT_FLAG_SECURITY: u32 = 0x10;

// ---------------------------------------------------------------------------
// ELF interpreter search
// ---------------------------------------------------------------------------

/// Maximum interpreter nesting depth (script → ELF → ...).
pub const BINFMT_MAX_RECURSION: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_types_distinct() {
        let types = [
            BINFMT_ELF, BINFMT_SCRIPT, BINFMT_MISC,
            BINFMT_FLAT, BINFMT_AOUT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_limits_positive() {
        assert!(MAX_ARG_STRLEN > 0);
        assert!(MAX_ARG_STRINGS > 0);
        assert!(BINPRM_BUF_SIZE > 0);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            BINFMT_FLAG_EXEC_STACK, BINFMT_FLAG_ASLR,
            BINFMT_FLAG_CREDENTIALS, BINFMT_FLAG_SUID,
            BINFMT_FLAG_SECURITY,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_max_recursion() {
        assert!(BINFMT_MAX_RECURSION > 0);
        assert!(BINFMT_MAX_RECURSION < 16);
    }
}
