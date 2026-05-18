//! `<glob.h>` — POSIX glob pattern matching constants.
//!
//! The `glob()` function searches for pathnames matching a
//! shell-style pattern.  These constants control its flags
//! and define its error return codes.

// ---------------------------------------------------------------------------
// glob() flags
// ---------------------------------------------------------------------------

/// Return on read error (don't silently skip errors).
pub const GLOB_ERR: u32 = 1 << 0;
/// Append a slash to each directory match.
pub const GLOB_MARK: u32 = 1 << 1;
/// Sort results (default on; specify to force).
pub const GLOB_NOSORT: u32 = 1 << 2;
/// Reserve pglob->gl_offs slots at the beginning.
pub const GLOB_DOOFFS: u32 = 1 << 3;
/// If no match, return the pattern itself.
pub const GLOB_NOCHECK: u32 = 1 << 4;
/// Append results to a previous glob() call.
pub const GLOB_APPEND: u32 = 1 << 5;
/// Backslash is not an escape character.
pub const GLOB_NOESCAPE: u32 = 1 << 6;
/// Treat period as special for matching.
pub const GLOB_PERIOD: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// GNU extensions to glob() flags
// ---------------------------------------------------------------------------

/// Enable brace expansion ({a,b}).
pub const GLOB_BRACE: u32 = 1 << 10;
/// Do not sort matches.
pub const GLOB_NOMAGIC: u32 = 1 << 11;
/// Expand ~ (tilde) in patterns.
pub const GLOB_TILDE: u32 = 1 << 12;
/// Only match directories.
pub const GLOB_ONLYDIR: u32 = 1 << 13;
/// Like GLOB_TILDE but error on missing user.
pub const GLOB_TILDE_CHECK: u32 = 1 << 14;

// ---------------------------------------------------------------------------
// glob() error return codes
// ---------------------------------------------------------------------------

/// Successful completion (no error).
pub const GLOB_OK: i32 = 0;
/// Out of memory.
pub const GLOB_NOSPACE: i32 = 1;
/// Read error (abort).
pub const GLOB_ABORTED: i32 = 2;
/// No matches found.
pub const GLOB_NOMATCH: i32 = 3;
/// Not implemented (reserved).
pub const GLOB_NOSYS: i32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            GLOB_ERR, GLOB_MARK, GLOB_NOSORT, GLOB_DOOFFS,
            GLOB_NOCHECK, GLOB_APPEND, GLOB_NOESCAPE, GLOB_PERIOD,
            GLOB_BRACE, GLOB_NOMAGIC, GLOB_TILDE, GLOB_ONLYDIR,
            GLOB_TILDE_CHECK,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            GLOB_ERR, GLOB_MARK, GLOB_NOSORT, GLOB_DOOFFS,
            GLOB_NOCHECK, GLOB_APPEND, GLOB_NOESCAPE, GLOB_PERIOD,
            GLOB_BRACE, GLOB_NOMAGIC, GLOB_TILDE, GLOB_ONLYDIR,
            GLOB_TILDE_CHECK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_err_is_one() {
        assert_eq!(GLOB_ERR, 1);
    }

    #[test]
    fn test_brace_value() {
        assert_eq!(GLOB_BRACE, 1024);
    }

    #[test]
    fn test_error_codes_distinct() {
        let codes = [
            GLOB_OK, GLOB_NOSPACE, GLOB_ABORTED,
            GLOB_NOMATCH, GLOB_NOSYS,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_ok_is_zero() {
        assert_eq!(GLOB_OK, 0);
    }

    #[test]
    fn test_nomatch_value() {
        assert_eq!(GLOB_NOMATCH, 3);
    }
}
