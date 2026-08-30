//! `<wordexp.h>` — Shell-style word expansion constants.
//!
//! `wordexp()` performs shell-like word expansion on a string
//! (tilde expansion, variable substitution, command substitution,
//! field splitting, pathname expansion).  These constants control
//! the expansion behaviour and define error codes.

// ---------------------------------------------------------------------------
// wordexp() flags
// ---------------------------------------------------------------------------

/// Append results to a previous call.
pub const WRDE_APPEND: u32 = 1 << 0;
/// Reserve pwordexp->we_offs slots at the beginning.
pub const WRDE_DOOFFS: u32 = 1 << 1;
/// Do not run command substitution.
pub const WRDE_NOCMD: u32 = 1 << 2;
/// Reuse (do not free) allocated storage.
pub const WRDE_REUSE: u32 = 1 << 3;
/// Show errors on stderr (do not redirect).
pub const WRDE_SHOWERR: u32 = 1 << 4;
/// Treat undefined variables as errors.
pub const WRDE_UNDEF: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// wordexp() error codes
// ---------------------------------------------------------------------------

/// Successful expansion.
pub const WRDE_OK: i32 = 0;
/// Bad character in pattern.
pub const WRDE_BADCHAR: i32 = 1;
/// Bad variable (undefined, with WRDE_UNDEF set).
pub const WRDE_BADVAL: i32 = 2;
/// Command substitution requested (with WRDE_NOCMD set).
pub const WRDE_CMDSUB: i32 = 3;
/// Out of memory.
pub const WRDE_NOSPACE: i32 = 4;
/// Shell syntax error.
pub const WRDE_SYNTAX: i32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            WRDE_APPEND,
            WRDE_DOOFFS,
            WRDE_NOCMD,
            WRDE_REUSE,
            WRDE_SHOWERR,
            WRDE_UNDEF,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            WRDE_APPEND,
            WRDE_DOOFFS,
            WRDE_NOCMD,
            WRDE_REUSE,
            WRDE_SHOWERR,
            WRDE_UNDEF,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_append_is_one() {
        assert_eq!(WRDE_APPEND, 1);
    }

    #[test]
    fn test_error_codes_distinct() {
        let codes = [
            WRDE_OK,
            WRDE_BADCHAR,
            WRDE_BADVAL,
            WRDE_CMDSUB,
            WRDE_NOSPACE,
            WRDE_SYNTAX,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_ok_is_zero() {
        assert_eq!(WRDE_OK, 0);
    }

    #[test]
    fn test_error_codes_sequential() {
        assert_eq!(WRDE_BADCHAR, 1);
        assert_eq!(WRDE_SYNTAX, 5);
    }
}
