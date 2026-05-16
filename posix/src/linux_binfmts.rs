//! `<linux/binfmts.h>` — Binary format handler constants.
//!
//! The binfmt subsystem identifies and loads executable formats
//! (ELF, scripts, misc registered formats). These constants define
//! limits and flags for the exec path.

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum length of an interpreter path (e.g., `#!/usr/bin/env python`).
pub const BINPRM_BUF_SIZE: usize = 256;

/// Maximum number of arguments + environment strings combined.
/// (Actually limited by MAX_ARG_PAGES × PAGE_SIZE in the kernel.)
pub const MAX_ARG_STRLEN: usize = 131072;

/// Maximum number of argument pages (32 pages × 4096 = 128KiB with 4K pages,
/// but our OS uses 16KiB pages so this gives 512KiB).
pub const MAX_ARG_PAGES: usize = 32;

// ---------------------------------------------------------------------------
// binfmt_misc flags
// ---------------------------------------------------------------------------

/// Enable the binfmt handler.
pub const MISC_FMT_PRESERVE_ARGV0: u32 = 1 << 0;
/// Open the binary file on register, not on each exec.
pub const MISC_FMT_OPEN_BINARY: u32 = 1 << 1;
/// Credentials from the binfmt interpreter.
pub const MISC_FMT_CREDENTIALS: u32 = 1 << 2;
/// Open file for each exec (fix).
pub const MISC_FMT_OPEN_FILE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Stack security
// ---------------------------------------------------------------------------

/// Executable stack allowed.
pub const EXSTACK_DEFAULT: u32 = 0;
/// Disable executable stack.
pub const EXSTACK_DISABLE_X: u32 = 1;
/// Enable executable stack.
pub const EXSTACK_ENABLE_X: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binprm_buf_size() {
        assert_eq!(BINPRM_BUF_SIZE, 256);
    }

    #[test]
    fn test_max_arg_strlen() {
        assert_eq!(MAX_ARG_STRLEN, 131072);
    }

    #[test]
    fn test_misc_flags_powers_of_two() {
        let flags = [
            MISC_FMT_PRESERVE_ARGV0, MISC_FMT_OPEN_BINARY,
            MISC_FMT_CREDENTIALS, MISC_FMT_OPEN_FILE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f} not power of 2");
        }
    }

    #[test]
    fn test_exstack_values() {
        assert_eq!(EXSTACK_DEFAULT, 0);
        assert_eq!(EXSTACK_DISABLE_X, 1);
        assert_eq!(EXSTACK_ENABLE_X, 2);
    }
}
