//! `<monetary.h>` — Monetary formatting constants.
//!
//! `strfmon()` formats monetary values according to locale
//! conventions.  These constants define formatting flags and
//! the structure of `struct lconv` fields used by `localeconv()`.

// ---------------------------------------------------------------------------
// strfmon() conversion characters (for format string)
// ---------------------------------------------------------------------------

/// International monetary format (uses international currency symbol).
pub const STRFMON_INTL: u8 = b'i';
/// Local monetary format (uses local currency symbol).
pub const STRFMON_LOCAL: u8 = b'n';

// ---------------------------------------------------------------------------
// strfmon() flags (format string modifiers)
// ---------------------------------------------------------------------------

/// Left-justify the result.
pub const STRFMON_FLAG_LEFT: u8 = b'-';
/// Use parentheses for negative values.
pub const STRFMON_FLAG_PARENS: u8 = b'(';
/// Suppress the currency symbol.
pub const STRFMON_FLAG_NOSYM: u8 = b'!';
/// Force a sign (+/-) on positive values.
pub const STRFMON_FLAG_SIGN: u8 = b'+';

// ---------------------------------------------------------------------------
// lconv structure field indices (for localeconv())
// ---------------------------------------------------------------------------

/// Position: currency symbol precedes value.
pub const LCONV_P_CS_PRECEDES: u8 = 1;
/// Position: currency symbol follows value.
pub const LCONV_N_CS_PRECEDES: u8 = 0;
/// Separation: space between currency symbol and value.
pub const LCONV_P_SEP_BY_SPACE: u8 = 1;
/// Separation: no space between symbol and value.
pub const LCONV_N_SEP_BY_SPACE: u8 = 0;

// ---------------------------------------------------------------------------
// Sign position values (p_sign_posn / n_sign_posn in lconv)
// ---------------------------------------------------------------------------

/// Parentheses surround the value and currency symbol.
pub const LCONV_SIGN_PARENS: u8 = 0;
/// Sign precedes the value and currency symbol.
pub const LCONV_SIGN_BEFORE_ALL: u8 = 1;
/// Sign follows the value and currency symbol.
pub const LCONV_SIGN_AFTER_ALL: u8 = 2;
/// Sign immediately precedes the currency symbol.
pub const LCONV_SIGN_BEFORE_SYM: u8 = 3;
/// Sign immediately follows the currency symbol.
pub const LCONV_SIGN_AFTER_SYM: u8 = 4;

// ---------------------------------------------------------------------------
// Special value for grouping
// ---------------------------------------------------------------------------

/// No further grouping (terminator in grouping string).
pub const LCONV_NO_GROUPING: u8 = 0;
/// Repeat the previous grouping for all remaining digits.
pub const LCONV_REPEAT_GROUPING: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversion_chars_distinct() {
        assert_ne!(STRFMON_INTL, STRFMON_LOCAL);
    }

    #[test]
    fn test_conversion_chars_ascii() {
        assert!(STRFMON_INTL.is_ascii_lowercase());
        assert!(STRFMON_LOCAL.is_ascii_lowercase());
    }

    #[test]
    fn test_flag_chars_ascii() {
        assert!(STRFMON_FLAG_LEFT.is_ascii());
        assert!(STRFMON_FLAG_PARENS.is_ascii());
        assert!(STRFMON_FLAG_NOSYM.is_ascii());
        assert!(STRFMON_FLAG_SIGN.is_ascii());
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            STRFMON_FLAG_LEFT,
            STRFMON_FLAG_PARENS,
            STRFMON_FLAG_NOSYM,
            STRFMON_FLAG_SIGN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_sign_positions_distinct() {
        let positions = [
            LCONV_SIGN_PARENS,
            LCONV_SIGN_BEFORE_ALL,
            LCONV_SIGN_AFTER_ALL,
            LCONV_SIGN_BEFORE_SYM,
            LCONV_SIGN_AFTER_SYM,
        ];
        for i in 0..positions.len() {
            for j in (i + 1)..positions.len() {
                assert_ne!(positions[i], positions[j]);
            }
        }
    }

    #[test]
    fn test_sign_parens_is_zero() {
        assert_eq!(LCONV_SIGN_PARENS, 0);
    }

    #[test]
    fn test_grouping_values_distinct() {
        assert_ne!(LCONV_NO_GROUPING, LCONV_REPEAT_GROUPING);
    }

    #[test]
    fn test_repeat_grouping_value() {
        assert_eq!(LCONV_REPEAT_GROUPING, 0xFF);
    }
}
