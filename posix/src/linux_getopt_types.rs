//! `<getopt.h>` — Command-line option parsing constants.
//!
//! `getopt()`, `getopt_long()`, and `getopt_long_only()` parse
//! command-line arguments according to POSIX and GNU conventions.
//! These constants define return values, argument requirements,
//! and option flags.

// ---------------------------------------------------------------------------
// getopt() return values
// ---------------------------------------------------------------------------

/// All options have been processed.
pub const GETOPT_DONE: i32 = -1;
/// Option character not recognised.
pub const GETOPT_UNKNOWN: i32 = b'?' as i32;
/// Missing option argument (when optstring starts with ':').
pub const GETOPT_MISSING_ARG: i32 = b':' as i32;

// ---------------------------------------------------------------------------
// getopt_long option has_arg values
// ---------------------------------------------------------------------------

/// Option takes no argument.
pub const NO_ARGUMENT: u32 = 0;
/// Option requires an argument.
pub const REQUIRED_ARGUMENT: u32 = 1;
/// Option may have an optional argument.
pub const OPTIONAL_ARGUMENT: u32 = 2;

// ---------------------------------------------------------------------------
// getopt behaviour control (opterr, optind defaults)
// ---------------------------------------------------------------------------

/// Default value of opterr (1 = print error messages).
pub const GETOPT_OPTERR_DEFAULT: u32 = 1;
/// Default starting value of optind (1 = skip argv[0]).
pub const GETOPT_OPTIND_DEFAULT: u32 = 1;
/// Value of optopt when an unrecognised option is found.
pub const GETOPT_OPTOPT_UNKNOWN: u32 = 0;

// ---------------------------------------------------------------------------
// GNU getopt ordering modes (first char of optstring)
// ---------------------------------------------------------------------------

/// Permute: rearrange argv so non-option args come last.
pub const GETOPT_ORDER_PERMUTE: u8 = 0;
/// Return non-option args as if they were option '\1'.
pub const GETOPT_ORDER_RETURN_IN_ORDER: u8 = b'-';
/// Stop at first non-option argument (POSIX behaviour).
pub const GETOPT_ORDER_REQUIRE_ORDER: u8 = b'+';

// ---------------------------------------------------------------------------
// Long-option sentinel
// ---------------------------------------------------------------------------

/// Sentinel value for the `val` field to indicate getopt_long should
/// set `*flag` and return 0.
pub const GETOPT_LONG_SET_FLAG: i32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_done_is_negative() {
        assert_eq!(GETOPT_DONE, -1);
    }

    #[test]
    fn test_unknown_is_question_mark() {
        assert_eq!(GETOPT_UNKNOWN, 63); // '?'
    }

    #[test]
    fn test_missing_arg_is_colon() {
        assert_eq!(GETOPT_MISSING_ARG, 58); // ':'
    }

    #[test]
    fn test_has_arg_values_distinct() {
        let vals = [NO_ARGUMENT, REQUIRED_ARGUMENT, OPTIONAL_ARGUMENT];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_no_argument_is_zero() {
        assert_eq!(NO_ARGUMENT, 0);
    }

    #[test]
    fn test_required_argument_is_one() {
        assert_eq!(REQUIRED_ARGUMENT, 1);
    }

    #[test]
    fn test_optional_argument_is_two() {
        assert_eq!(OPTIONAL_ARGUMENT, 2);
    }

    #[test]
    fn test_opterr_default() {
        assert_eq!(GETOPT_OPTERR_DEFAULT, 1);
    }

    #[test]
    fn test_optind_default() {
        assert_eq!(GETOPT_OPTIND_DEFAULT, 1);
    }

    #[test]
    fn test_ordering_modes_distinct() {
        let modes = [
            GETOPT_ORDER_PERMUTE,
            GETOPT_ORDER_RETURN_IN_ORDER,
            GETOPT_ORDER_REQUIRE_ORDER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_long_set_flag() {
        assert_eq!(GETOPT_LONG_SET_FLAG, 0);
    }
}
