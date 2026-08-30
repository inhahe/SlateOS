//! `<fnmatch.h>` — Filename pattern matching constants.
//!
//! `fnmatch()` matches a string against a shell wildcard pattern
//! (similar to glob but for a single string, not file listing).
//! These constants control matching behaviour and define the
//! return codes.

// ---------------------------------------------------------------------------
// fnmatch() flags
// ---------------------------------------------------------------------------

/// Backslash is not treated as an escape character.
pub const FNM_NOESCAPE: u32 = 1 << 1;
/// Slash in string must be matched by slash in pattern.
pub const FNM_PATHNAME: u32 = 1 << 0;
/// Leading period in string must be matched explicitly.
pub const FNM_PERIOD: u32 = 1 << 2;
/// Alias for FNM_PATHNAME (BSD convention).
pub const FNM_FILE_NAME: u32 = FNM_PATHNAME;
/// Match leading dir — pattern matches if it matches an initial segment.
pub const FNM_LEADING_DIR: u32 = 1 << 3;
/// Case-insensitive matching (GNU extension).
pub const FNM_CASEFOLD: u32 = 1 << 4;
/// Use extended glob patterns (ksh-style, GNU extension).
pub const FNM_EXTMATCH: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// fnmatch() return codes
// ---------------------------------------------------------------------------

/// Pattern matched successfully.
pub const FNM_MATCH: i32 = 0;
/// Pattern did not match.
pub const FNM_NOMATCH: i32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            FNM_PATHNAME,
            FNM_NOESCAPE,
            FNM_PERIOD,
            FNM_LEADING_DIR,
            FNM_CASEFOLD,
            FNM_EXTMATCH,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            FNM_PATHNAME,
            FNM_NOESCAPE,
            FNM_PERIOD,
            FNM_LEADING_DIR,
            FNM_CASEFOLD,
            FNM_EXTMATCH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_file_name_is_pathname() {
        assert_eq!(FNM_FILE_NAME, FNM_PATHNAME);
    }

    #[test]
    fn test_pathname_is_one() {
        assert_eq!(FNM_PATHNAME, 1);
    }

    #[test]
    fn test_match_is_zero() {
        assert_eq!(FNM_MATCH, 0);
    }

    #[test]
    fn test_nomatch_is_one() {
        assert_eq!(FNM_NOMATCH, 1);
    }

    #[test]
    fn test_return_codes_distinct() {
        assert_ne!(FNM_MATCH, FNM_NOMATCH);
    }
}
