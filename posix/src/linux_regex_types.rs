//! `<regex.h>` — POSIX regular expression constants.
//!
//! POSIX defines a standard regular expression API (`regcomp`,
//! `regexec`, `regfree`).  These constants control compilation
//! flags, execution flags, and error codes.

// ---------------------------------------------------------------------------
// Compilation flags (cflags for regcomp)
// ---------------------------------------------------------------------------

/// Use extended regular expressions (ERE).
pub const REG_EXTENDED: u32 = 1;
/// Ignore case in match.
pub const REG_ICASE: u32 = 1 << 1;
/// Report only success/fail, not match positions.
pub const REG_NOSUB: u32 = 1 << 3;
/// Newline-sensitive matching (affects ^ and $).
pub const REG_NEWLINE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Execution flags (eflags for regexec)
// ---------------------------------------------------------------------------

/// Start of string is not beginning of line (affects ^).
pub const REG_NOTBOL: u32 = 1;
/// End of string is not end of line (affects $).
pub const REG_NOTEOL: u32 = 1 << 1;
/// Use pmatch[0] to constrain the search range (glibc extension).
pub const REG_STARTEND: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Error codes (from regcomp/regexec)
// ---------------------------------------------------------------------------

/// Successful match / no error.
pub const REG_OK: i32 = 0;
/// No match found.
pub const REG_NOMATCH: i32 = 1;
/// Invalid regular expression.
pub const REG_BADPAT: i32 = 2;
/// Invalid collating element.
pub const REG_ECOLLATE: i32 = 3;
/// Invalid character class.
pub const REG_ECTYPE: i32 = 4;
/// Trailing backslash.
pub const REG_EESCAPE: i32 = 5;
/// Invalid back-reference number.
pub const REG_ESUBREG: i32 = 6;
/// Unmatched bracket.
pub const REG_EBRACK: i32 = 7;
/// Unmatched parenthesis.
pub const REG_EPAREN: i32 = 8;
/// Unmatched brace.
pub const REG_EBRACE: i32 = 9;
/// Invalid content of brace quantifier.
pub const REG_BADBR: i32 = 10;
/// Invalid range end.
pub const REG_ERANGE: i32 = 11;
/// Out of memory.
pub const REG_ESPACE: i32 = 12;
/// Invalid repetition operator.
pub const REG_BADRPT: i32 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cflags_no_overlap() {
        let flags = [REG_EXTENDED, REG_ICASE, REG_NOSUB, REG_NEWLINE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cflags_powers_of_two() {
        assert!(REG_EXTENDED.is_power_of_two());
        assert!(REG_ICASE.is_power_of_two());
        assert!(REG_NOSUB.is_power_of_two());
        assert!(REG_NEWLINE.is_power_of_two());
    }

    #[test]
    fn test_extended_is_one() {
        assert_eq!(REG_EXTENDED, 1);
    }

    #[test]
    fn test_error_codes_distinct() {
        let codes = [
            REG_OK,
            REG_NOMATCH,
            REG_BADPAT,
            REG_ECOLLATE,
            REG_ECTYPE,
            REG_EESCAPE,
            REG_ESUBREG,
            REG_EBRACK,
            REG_EPAREN,
            REG_EBRACE,
            REG_BADBR,
            REG_ERANGE,
            REG_ESPACE,
            REG_BADRPT,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_ok_is_zero() {
        assert_eq!(REG_OK, 0);
    }

    #[test]
    fn test_nomatch_is_one() {
        assert_eq!(REG_NOMATCH, 1);
    }

    #[test]
    fn test_error_codes_sequential() {
        // Error codes 1..=13 are sequential
        assert_eq!(REG_NOMATCH, 1);
        assert_eq!(REG_BADRPT, 13);
    }
}
