//! `<nl_types.h>` — Native language (message catalog) constants.
//!
//! The `catopen()`, `catgets()`, `catclose()` API provides access
//! to message catalogs for application internationalization.
//! These constants define catalog open flags and default set
//! numbers.

// ---------------------------------------------------------------------------
// catopen() flags (oflag parameter)
// ---------------------------------------------------------------------------

/// Use NL_CAT_LOCALE to select the catalog locale via LC_MESSAGES.
pub const NL_CAT_LOCALE: u32 = 1;

// ---------------------------------------------------------------------------
// Default set and message constants
// ---------------------------------------------------------------------------

/// Default message set number.
pub const NL_SETD: u32 = 1;
/// Maximum message set number.
pub const NL_SET_MAX: u32 = 255;
/// Maximum message text length (glibc value).
pub const NL_TEXT_MAX: u32 = 2048;
/// Maximum number of messages per set (glibc value).
pub const NL_MSG_MAX: u32 = 32767;

// ---------------------------------------------------------------------------
// nl_catd error value
// ---------------------------------------------------------------------------

/// Invalid catalog descriptor (from catopen on failure, cast from (nl_catd)-1).
pub const NL_CATD_ERROR: isize = -1;

// ---------------------------------------------------------------------------
// Message catalog magic numbers (for .cat file format)
// ---------------------------------------------------------------------------

/// Magic number for big-endian message catalog files.
pub const NL_CAT_MAGIC_BE: u32 = 0x960707FF;
/// Magic number for little-endian message catalog files.
pub const NL_CAT_MAGIC_LE: u32 = 0xFF070796;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cat_locale_is_one() {
        assert_eq!(NL_CAT_LOCALE, 1);
    }

    #[test]
    fn test_setd_is_one() {
        assert_eq!(NL_SETD, 1);
    }

    #[test]
    fn test_set_max() {
        assert_eq!(NL_SET_MAX, 255);
    }

    #[test]
    fn test_text_max() {
        assert_eq!(NL_TEXT_MAX, 2048);
    }

    #[test]
    fn test_msg_max() {
        assert_eq!(NL_MSG_MAX, 32767);
    }

    #[test]
    fn test_catd_error() {
        assert_eq!(NL_CATD_ERROR, -1);
    }

    #[test]
    fn test_magic_numbers_distinct() {
        assert_ne!(NL_CAT_MAGIC_BE, NL_CAT_MAGIC_LE);
    }

    #[test]
    fn test_magic_be_value() {
        assert_eq!(NL_CAT_MAGIC_BE, 0x960707FF);
    }

    #[test]
    fn test_magic_le_value() {
        assert_eq!(NL_CAT_MAGIC_LE, 0xFF070796);
    }

    #[test]
    fn test_limits_positive() {
        assert!(NL_SET_MAX > 0);
        assert!(NL_TEXT_MAX > 0);
        assert!(NL_MSG_MAX > 0);
    }
}
