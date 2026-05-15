//! `<values.h>` — legacy numerical limits.
//!
//! This header predates `<limits.h>` and `<float.h>`.  It provides
//! older names for machine-dependent limits.  Modern code should use
//! `<limits.h>` and `<float.h>` instead.  This module exists solely
//! for source compatibility with legacy programs.

// ---------------------------------------------------------------------------
// Integer limits (from <limits.h>)
// ---------------------------------------------------------------------------

/// Number of bits in a `char`.
pub const CHARBITS: i32 = 8;

/// Number of bits in a `short`.
pub const SHORTBITS: i32 = 16;

/// Number of bits in an `int`.
pub const INTBITS: i32 = 32;

/// Number of bits in a `long` (64-bit platform).
pub const LONGBITS: i32 = 64;

/// Maximum value of a `char` (unsigned).
pub const MAXCHAR: i32 = 127;

/// Maximum value of a `short`.
pub const MAXSHORT: i32 = 32767;

/// Maximum value of an `int`.
pub const MAXINT: i32 = 2147483647;

/// Maximum value of a `long`.
pub const MAXLONG: i64 = 9223372036854775807;

/// Minimum value of a `short`.
pub const MINSHORT: i32 = -32768;

/// Minimum value of an `int`.
pub const MININT: i32 = -2147483648;

/// Minimum value of a `long`.
pub const MINLONG: i64 = -9223372036854775808;

// ---------------------------------------------------------------------------
// Floating-point limits (from <float.h>)
// ---------------------------------------------------------------------------

/// Maximum `float` value.
pub const MAXFLOAT: f32 = 3.40282347e+38;

/// Maximum `double` value.
pub const MAXDOUBLE: f64 = 1.7976931348623157e+308;

/// Minimum positive normalized `float`.
pub const MINFLOAT: f32 = 1.17549435e-38;

/// Minimum positive normalized `double`.
pub const MINDOUBLE: f64 = 2.2250738585072014e-308;

/// Number of significant bits in a `float` mantissa.
pub const FSIGNIF: i32 = 24;

/// Number of significant bits in a `double` mantissa.
pub const DSIGNIF: i32 = 53;

// ---------------------------------------------------------------------------
// Misc legacy constants
// ---------------------------------------------------------------------------

/// Number of bits per byte.
pub const BITSPERBYTE: i32 = 8;

/// Logarithm base 2 of `MAXINT + 1` (i.e., number of value bits
/// in an `int`, not counting the sign bit).
pub const HIBITI: i32 = INTBITS - 1;

/// Logarithm base 2 of `MAXSHORT + 1`.
pub const HIBITS: i32 = SHORTBITS - 1;

/// Logarithm base 2 of `MAXLONG + 1`.
pub const HIBITL: i32 = LONGBITS - 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Bit-width constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_charbits() {
        assert_eq!(CHARBITS, 8);
    }

    #[test]
    fn test_shortbits() {
        assert_eq!(SHORTBITS, 16);
    }

    #[test]
    fn test_intbits() {
        assert_eq!(INTBITS, 32);
    }

    #[test]
    fn test_longbits() {
        assert_eq!(LONGBITS, 64);
    }

    #[test]
    fn test_bitsperbyte() {
        assert_eq!(BITSPERBYTE, CHARBITS);
    }

    // -----------------------------------------------------------------------
    // Integer max/min
    // -----------------------------------------------------------------------

    #[test]
    fn test_maxchar() {
        assert_eq!(MAXCHAR, i8::MAX as i32);
    }

    #[test]
    fn test_maxshort() {
        assert_eq!(MAXSHORT, i16::MAX as i32);
    }

    #[test]
    fn test_maxint() {
        assert_eq!(MAXINT, i32::MAX);
    }

    #[test]
    fn test_maxlong() {
        assert_eq!(MAXLONG, i64::MAX);
    }

    #[test]
    fn test_minshort() {
        assert_eq!(MINSHORT, i16::MIN as i32);
    }

    #[test]
    fn test_minint() {
        assert_eq!(MININT, i32::MIN);
    }

    #[test]
    fn test_minlong() {
        assert_eq!(MINLONG, i64::MIN);
    }

    #[test]
    fn test_min_max_relationship() {
        assert!(MINSHORT < 0);
        assert!(MAXSHORT > 0);
        assert!(MININT < 0);
        assert!(MAXINT > 0);
        assert!(MINLONG < 0);
        assert!(MAXLONG > 0);
    }

    // -----------------------------------------------------------------------
    // Float limits
    // -----------------------------------------------------------------------

    #[test]
    fn test_maxfloat() {
        assert_eq!(MAXFLOAT, f32::MAX);
    }

    #[test]
    fn test_maxdouble() {
        assert_eq!(MAXDOUBLE, f64::MAX);
    }

    #[test]
    fn test_minfloat() {
        assert_eq!(MINFLOAT, f32::MIN_POSITIVE);
    }

    #[test]
    fn test_mindouble() {
        assert_eq!(MINDOUBLE, f64::MIN_POSITIVE);
    }

    #[test]
    fn test_fsignif() {
        // f32 has 24 significant binary digits (including implicit 1).
        assert_eq!(FSIGNIF, 24);
    }

    #[test]
    fn test_dsignif() {
        // f64 has 53 significant binary digits (including implicit 1).
        assert_eq!(DSIGNIF, 53);
    }

    // -----------------------------------------------------------------------
    // HIBIT constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_hibiti() {
        assert_eq!(HIBITI, 31);
    }

    #[test]
    fn test_hibits() {
        assert_eq!(HIBITS, 15);
    }

    #[test]
    fn test_hibitl() {
        assert_eq!(HIBITL, 63);
    }

    // -----------------------------------------------------------------------
    // Cross-module: verify consistency with Rust primitive limits
    // -----------------------------------------------------------------------

    #[test]
    fn test_maxint_matches_i32() {
        assert_eq!(MAXINT, i32::MAX);
    }

    #[test]
    fn test_minint_matches_i32() {
        assert_eq!(MININT, i32::MIN);
    }

    #[test]
    fn test_maxshort_matches_i16() {
        assert_eq!(MAXSHORT, i16::MAX as i32);
    }

    #[test]
    fn test_maxlong_matches_i64() {
        assert_eq!(MAXLONG, i64::MAX);
    }
}
