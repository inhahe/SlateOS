//! `<linux/binfmts.h>` — Additional binary format constants.
//!
//! Supplementary binfmt constants covering executable formats,
//! interpreter flags, and stack layout parameters.

// ---------------------------------------------------------------------------
// Binary format types
// ---------------------------------------------------------------------------

/// ELF format.
pub const BINFMT_ELF: u32 = 0;
/// Script (shebang).
pub const BINFMT_SCRIPT: u32 = 1;
/// Misc binary format.
pub const BINFMT_MISC: u32 = 2;
/// Flat binary.
pub const BINFMT_FLAT: u32 = 3;

// ---------------------------------------------------------------------------
// Binfmt misc flags
// ---------------------------------------------------------------------------

/// Preserve argv[0].
pub const MISC_FMT_PRESERVE_ARGV0: u32 = 1 << 0;
/// Open binary (pass fd).
pub const MISC_FMT_OPEN_BINARY: u32 = 1 << 1;
/// Credentials (setuid/setgid).
pub const MISC_FMT_CREDENTIALS: u32 = 1 << 2;
/// Open file (pass open fd).
pub const MISC_FMT_OPEN_FILE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Stack/argument limits
// ---------------------------------------------------------------------------

/// Maximum argument string length.
pub const MAX_ARG_STRLEN: u32 = 131072;
/// Maximum number of argument strings.
pub const MAX_ARG_STRINGS: u32 = 0x7FFFFFFF;
/// Binary format buffer size.
pub const BINPRM_BUF_SIZE: u32 = 256;

// ---------------------------------------------------------------------------
// ELF flags
// ---------------------------------------------------------------------------

/// ET_EXEC (executable).
pub const ET_EXEC: u16 = 2;
/// ET_DYN (shared object / PIE).
pub const ET_DYN: u16 = 3;
/// ET_CORE (core dump).
pub const ET_CORE: u16 = 4;

// ---------------------------------------------------------------------------
// ELF PT types
// ---------------------------------------------------------------------------

/// Null segment.
pub const PT_NULL: u32 = 0;
/// Loadable segment.
pub const PT_LOAD: u32 = 1;
/// Dynamic linking info.
pub const PT_DYNAMIC: u32 = 2;
/// Interpreter path.
pub const PT_INTERP: u32 = 3;
/// Note segment.
pub const PT_NOTE: u32 = 4;
/// Reserved.
pub const PT_SHLIB: u32 = 5;
/// Program header table.
pub const PT_PHDR: u32 = 6;
/// Thread-local storage.
pub const PT_TLS: u32 = 7;
/// GNU EH frame.
pub const PT_GNU_EH_FRAME: u32 = 0x6474E550;
/// GNU stack.
pub const PT_GNU_STACK: u32 = 0x6474E551;
/// GNU relro.
pub const PT_GNU_RELRO: u32 = 0x6474E552;
/// GNU property.
pub const PT_GNU_PROPERTY: u32 = 0x6474E553;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binfmt_types_distinct() {
        let types = [BINFMT_ELF, BINFMT_SCRIPT, BINFMT_MISC, BINFMT_FLAT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_misc_flags_power_of_two() {
        let flags = [
            MISC_FMT_PRESERVE_ARGV0, MISC_FMT_OPEN_BINARY,
            MISC_FMT_CREDENTIALS, MISC_FMT_OPEN_FILE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_stack_limits() {
        assert_eq!(MAX_ARG_STRLEN, 131072);
        assert_eq!(BINPRM_BUF_SIZE, 256);
    }

    #[test]
    fn test_et_types_distinct() {
        let types: [u16; 3] = [ET_EXEC, ET_DYN, ET_CORE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pt_types_distinct() {
        let types = [
            PT_NULL, PT_LOAD, PT_DYNAMIC, PT_INTERP, PT_NOTE,
            PT_SHLIB, PT_PHDR, PT_TLS, PT_GNU_EH_FRAME,
            PT_GNU_STACK, PT_GNU_RELRO, PT_GNU_PROPERTY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
