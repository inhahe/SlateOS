//! `<locale.h>` — Locale category and mask constants.
//!
//! The locale system controls language, character encoding, number
//! formatting, date/time formatting, and other culture-specific
//! behaviour.  These constants identify locale categories for
//! `setlocale()` and `newlocale()`.

// ---------------------------------------------------------------------------
// Locale categories (LC_*)
// ---------------------------------------------------------------------------

/// Character classification and conversion.
pub const LC_CTYPE: u32 = 0;
/// Number formatting (decimal point, thousands separator).
pub const LC_NUMERIC: u32 = 1;
/// Date and time formatting.
pub const LC_TIME: u32 = 2;
/// String collation/sorting order.
pub const LC_COLLATE: u32 = 3;
/// Monetary formatting.
pub const LC_MONETARY: u32 = 4;
/// Natural-language messages (gettext).
pub const LC_MESSAGES: u32 = 5;
/// Set all categories simultaneously.
pub const LC_ALL: u32 = 6;
/// Paper size (LC_PAPER, glibc extension).
pub const LC_PAPER: u32 = 7;
/// Personal name formatting (glibc extension).
pub const LC_NAME: u32 = 8;
/// Address formatting (glibc extension).
pub const LC_ADDRESS: u32 = 9;
/// Telephone number formatting (glibc extension).
pub const LC_TELEPHONE: u32 = 10;
/// Units of measurement (glibc extension).
pub const LC_MEASUREMENT: u32 = 11;
/// Metadata/identification of the locale (glibc extension).
pub const LC_IDENTIFICATION: u32 = 12;

// ---------------------------------------------------------------------------
// Locale category masks (for newlocale())
// ---------------------------------------------------------------------------

/// Mask for LC_CTYPE.
pub const LC_CTYPE_MASK: u32 = 1 << LC_CTYPE;
/// Mask for LC_NUMERIC.
pub const LC_NUMERIC_MASK: u32 = 1 << LC_NUMERIC;
/// Mask for LC_TIME.
pub const LC_TIME_MASK: u32 = 1 << LC_TIME;
/// Mask for LC_COLLATE.
pub const LC_COLLATE_MASK: u32 = 1 << LC_COLLATE;
/// Mask for LC_MONETARY.
pub const LC_MONETARY_MASK: u32 = 1 << LC_MONETARY;
/// Mask for LC_MESSAGES.
pub const LC_MESSAGES_MASK: u32 = 1 << LC_MESSAGES;
/// Mask for LC_PAPER.
pub const LC_PAPER_MASK: u32 = 1 << LC_PAPER;
/// Mask for LC_NAME.
pub const LC_NAME_MASK: u32 = 1 << LC_NAME;
/// Mask for LC_ADDRESS.
pub const LC_ADDRESS_MASK: u32 = 1 << LC_ADDRESS;
/// Mask for LC_TELEPHONE.
pub const LC_TELEPHONE_MASK: u32 = 1 << LC_TELEPHONE;
/// Mask for LC_MEASUREMENT.
pub const LC_MEASUREMENT_MASK: u32 = 1 << LC_MEASUREMENT;
/// Mask for LC_IDENTIFICATION.
pub const LC_IDENTIFICATION_MASK: u32 = 1 << LC_IDENTIFICATION;
/// Mask for all categories (combine all masks).
pub const LC_ALL_MASK: u32 = LC_CTYPE_MASK | LC_NUMERIC_MASK | LC_TIME_MASK
    | LC_COLLATE_MASK | LC_MONETARY_MASK | LC_MESSAGES_MASK
    | LC_PAPER_MASK | LC_NAME_MASK | LC_ADDRESS_MASK
    | LC_TELEPHONE_MASK | LC_MEASUREMENT_MASK | LC_IDENTIFICATION_MASK;

// ---------------------------------------------------------------------------
// Special locale constants
// ---------------------------------------------------------------------------

/// Use the global locale (for uselocale()).
pub const LC_GLOBAL_LOCALE: isize = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categories_distinct() {
        let cats = [
            LC_CTYPE, LC_NUMERIC, LC_TIME, LC_COLLATE,
            LC_MONETARY, LC_MESSAGES, LC_ALL, LC_PAPER,
            LC_NAME, LC_ADDRESS, LC_TELEPHONE,
            LC_MEASUREMENT, LC_IDENTIFICATION,
        ];
        for i in 0..cats.len() {
            for j in (i + 1)..cats.len() {
                assert_ne!(cats[i], cats[j]);
            }
        }
    }

    #[test]
    fn test_ctype_is_zero() {
        assert_eq!(LC_CTYPE, 0);
    }

    #[test]
    fn test_all_is_six() {
        assert_eq!(LC_ALL, 6);
    }

    #[test]
    fn test_masks_are_powers_of_two() {
        let masks = [
            LC_CTYPE_MASK, LC_NUMERIC_MASK, LC_TIME_MASK,
            LC_COLLATE_MASK, LC_MONETARY_MASK, LC_MESSAGES_MASK,
            LC_PAPER_MASK, LC_NAME_MASK, LC_ADDRESS_MASK,
            LC_TELEPHONE_MASK, LC_MEASUREMENT_MASK,
            LC_IDENTIFICATION_MASK,
        ];
        for m in masks {
            assert!(m.is_power_of_two());
        }
    }

    #[test]
    fn test_masks_no_overlap() {
        let masks = [
            LC_CTYPE_MASK, LC_NUMERIC_MASK, LC_TIME_MASK,
            LC_COLLATE_MASK, LC_MONETARY_MASK, LC_MESSAGES_MASK,
            LC_PAPER_MASK, LC_NAME_MASK, LC_ADDRESS_MASK,
            LC_TELEPHONE_MASK, LC_MEASUREMENT_MASK,
            LC_IDENTIFICATION_MASK,
        ];
        for i in 0..masks.len() {
            for j in (i + 1)..masks.len() {
                assert_eq!(masks[i] & masks[j], 0);
            }
        }
    }

    #[test]
    fn test_all_mask_covers_all() {
        let combined = LC_CTYPE_MASK | LC_NUMERIC_MASK | LC_TIME_MASK
            | LC_COLLATE_MASK | LC_MONETARY_MASK | LC_MESSAGES_MASK
            | LC_PAPER_MASK | LC_NAME_MASK | LC_ADDRESS_MASK
            | LC_TELEPHONE_MASK | LC_MEASUREMENT_MASK
            | LC_IDENTIFICATION_MASK;
        assert_eq!(LC_ALL_MASK, combined);
    }

    #[test]
    fn test_mask_values() {
        assert_eq!(LC_CTYPE_MASK, 1);
        assert_eq!(LC_NUMERIC_MASK, 2);
        assert_eq!(LC_TIME_MASK, 4);
    }

    #[test]
    fn test_global_locale() {
        assert_eq!(LC_GLOBAL_LOCALE, -1);
    }
}
