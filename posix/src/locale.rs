//! C locale functions.
//!
//! Implements a minimal `<locale.h>` and POSIX extended locale
//! (`<xlocale.h>`) with C locale only.
//!
//! Functions: `setlocale`, `localeconv`, `newlocale`, `duplocale`,
//! `freelocale`, `uselocale`.
//!
//! All locale functions return the C locale — our OS doesn't support
//! other locales.  This is sufficient for programs that call
//! `setlocale(LC_ALL, "")` during initialization and for libraries
//! that use the POSIX 2008 extended locale functions.

/// Locale categories.
pub const LC_CTYPE: i32 = 0;
pub const LC_NUMERIC: i32 = 1;
pub const LC_TIME: i32 = 2;
pub const LC_COLLATE: i32 = 3;
pub const LC_MONETARY: i32 = 4;
pub const LC_MESSAGES: i32 = 5;
pub const LC_ALL: i32 = 6;

/// Numeric formatting information (lconv).
///
/// Matches the POSIX/C `struct lconv` layout.
#[repr(C)]
pub struct Lconv {
    pub decimal_point: *const u8,
    pub thousands_sep: *const u8,
    pub grouping: *const u8,
    pub int_curr_symbol: *const u8,
    pub currency_symbol: *const u8,
    pub mon_decimal_point: *const u8,
    pub mon_thousands_sep: *const u8,
    pub mon_grouping: *const u8,
    pub positive_sign: *const u8,
    pub negative_sign: *const u8,
    pub int_frac_digits: u8,
    pub frac_digits: u8,
    pub p_cs_precedes: u8,
    pub p_sep_by_space: u8,
    pub n_cs_precedes: u8,
    pub n_sep_by_space: u8,
    pub p_sign_posn: u8,
    pub n_sign_posn: u8,
}

// SAFETY: Lconv contains only *const u8 pointing to static string literals
// (which live for the entire program lifetime) and plain u8 values.
// The struct is immutable after initialization and safe to share.
unsafe impl Sync for Lconv {}

/// Static C locale lconv.
static C_LCONV: Lconv = Lconv {
    decimal_point: c".".as_ptr().cast::<u8>(),
    thousands_sep: c"".as_ptr().cast::<u8>(),
    grouping: c"".as_ptr().cast::<u8>(),
    int_curr_symbol: c"".as_ptr().cast::<u8>(),
    currency_symbol: c"".as_ptr().cast::<u8>(),
    mon_decimal_point: c"".as_ptr().cast::<u8>(),
    mon_thousands_sep: c"".as_ptr().cast::<u8>(),
    mon_grouping: c"".as_ptr().cast::<u8>(),
    positive_sign: c"".as_ptr().cast::<u8>(),
    negative_sign: c"".as_ptr().cast::<u8>(),
    int_frac_digits: 127, // CHAR_MAX = "not available"
    frac_digits: 127,
    p_cs_precedes: 127,
    p_sep_by_space: 127,
    n_cs_precedes: 127,
    n_sep_by_space: 127,
    p_sign_posn: 127,
    n_sign_posn: 127,
};

/// Set or query the program's locale.
///
/// Always returns `"C"` — we only support the C locale.
/// `locale` parameter is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setlocale(_category: i32, _locale: *const u8) -> *const u8 {
    c"C".as_ptr().cast::<u8>()
}

/// Get numeric formatting information.
///
/// Always returns the C locale formatting.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn localeconv() -> *const Lconv {
    &raw const C_LCONV
}

// ---------------------------------------------------------------------------
// POSIX 2008 extended locale (xlocale)
// ---------------------------------------------------------------------------

/// Opaque locale type for POSIX 2008 extended locale functions.
///
/// We use a simple tag value (1 = C locale).  A real implementation
/// would point to a locale data structure.
pub type LocaleT = usize;

/// The global locale (special value for `uselocale`).
pub const LC_GLOBAL_LOCALE: LocaleT = usize::MAX;

/// Sentinel value for the C locale.
const C_LOCALE_TAG: LocaleT = 1;

/// Create a new locale object.
///
/// Always returns a handle for the C locale regardless of the
/// `locale` string.  `base` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn newlocale(
    _category_mask: i32,
    _locale: *const u8,
    _base: LocaleT,
) -> LocaleT {
    C_LOCALE_TAG
}

/// Duplicate a locale object.
///
/// Returns a handle to the C locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn duplocale(_locobj: LocaleT) -> LocaleT {
    C_LOCALE_TAG
}

/// Free a locale object.
///
/// No-op — our locale objects are static tags with no heap allocation.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn freelocale(_locobj: LocaleT) {}

/// Set the thread-local locale.
///
/// Returns the previous locale.  Always returns the C locale
/// (thread-local locale storage is not implemented).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn uselocale(_newloc: LocaleT) -> LocaleT {
    C_LOCALE_TAG
}

// ---------------------------------------------------------------------------
// Category masks for newlocale
// ---------------------------------------------------------------------------

/// Mask for `LC_CTYPE`.
pub const LC_CTYPE_MASK: i32 = 1 << LC_CTYPE;
/// Mask for `LC_NUMERIC`.
pub const LC_NUMERIC_MASK: i32 = 1 << LC_NUMERIC;
/// Mask for `LC_TIME`.
pub const LC_TIME_MASK: i32 = 1 << LC_TIME;
/// Mask for `LC_COLLATE`.
pub const LC_COLLATE_MASK: i32 = 1 << LC_COLLATE;
/// Mask for `LC_MONETARY`.
pub const LC_MONETARY_MASK: i32 = 1 << LC_MONETARY;
/// Mask for `LC_MESSAGES`.
pub const LC_MESSAGES_MASK: i32 = 1 << LC_MESSAGES;
/// Mask for all categories (LC_CTYPE through LC_MESSAGES).
#[allow(clippy::cast_possible_truncation)]
pub const LC_ALL_MASK: i32 = (1 << LC_ALL) - 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- setlocale always returns "C" --

    #[test]
    fn test_setlocale_returns_c() {
        let result = setlocale(LC_ALL, core::ptr::null());
        assert!(!result.is_null());
        // Should point to "C\0".
        assert_eq!(unsafe { *result }, b'C');
        assert_eq!(unsafe { *result.add(1) }, 0);
    }

    #[test]
    fn test_setlocale_ignores_locale_arg() {
        let result = setlocale(LC_ALL, b"en_US.UTF-8\0".as_ptr());
        assert!(!result.is_null());
        assert_eq!(unsafe { *result }, b'C');
    }

    #[test]
    fn test_setlocale_all_categories() {
        for cat in [LC_CTYPE, LC_NUMERIC, LC_TIME, LC_COLLATE, LC_MONETARY, LC_MESSAGES, LC_ALL] {
            let result = setlocale(cat, core::ptr::null());
            assert!(!result.is_null());
            assert_eq!(unsafe { *result }, b'C');
        }
    }

    // -- localeconv returns valid C locale --

    #[test]
    fn test_localeconv_not_null() {
        let lc = localeconv();
        assert!(!lc.is_null());
    }

    #[test]
    fn test_localeconv_decimal_point() {
        let lc = localeconv();
        let dp = unsafe { (*lc).decimal_point };
        assert!(!dp.is_null());
        assert_eq!(unsafe { *dp }, b'.');
    }

    #[test]
    fn test_localeconv_frac_digits_char_max() {
        let lc = localeconv();
        // C locale: monetary fields are CHAR_MAX = 127 = "not available"
        assert_eq!(unsafe { (*lc).int_frac_digits }, 127);
        assert_eq!(unsafe { (*lc).frac_digits }, 127);
    }

    // -- Extended locale (xlocale) functions --

    #[test]
    fn test_newlocale_returns_c() {
        let loc = newlocale(LC_ALL_MASK, b"C\0".as_ptr(), 0);
        assert_eq!(loc, C_LOCALE_TAG);
    }

    #[test]
    fn test_duplocale_returns_c() {
        let loc = duplocale(C_LOCALE_TAG);
        assert_eq!(loc, C_LOCALE_TAG);
    }

    #[test]
    fn test_freelocale_no_panic() {
        // Should be a no-op without panicking.
        freelocale(C_LOCALE_TAG);
        freelocale(0);
    }

    #[test]
    fn test_uselocale_returns_c() {
        let prev = uselocale(C_LOCALE_TAG);
        assert_eq!(prev, C_LOCALE_TAG);
    }

    // -- Constants --

    #[test]
    fn test_category_values() {
        // Values must match glibc.
        assert_eq!(LC_CTYPE, 0);
        assert_eq!(LC_NUMERIC, 1);
        assert_eq!(LC_TIME, 2);
        assert_eq!(LC_COLLATE, 3);
        assert_eq!(LC_MONETARY, 4);
        assert_eq!(LC_MESSAGES, 5);
        assert_eq!(LC_ALL, 6);
    }

    #[test]
    fn test_category_masks() {
        assert_eq!(LC_CTYPE_MASK, 1);    // 1 << 0
        assert_eq!(LC_NUMERIC_MASK, 2);  // 1 << 1
        assert_eq!(LC_TIME_MASK, 4);     // 1 << 2
        assert_eq!(LC_COLLATE_MASK, 8);  // 1 << 3
        assert_eq!(LC_MONETARY_MASK, 16); // 1 << 4
        assert_eq!(LC_MESSAGES_MASK, 32); // 1 << 5
        // LC_ALL_MASK should cover all categories.
        assert_eq!(LC_ALL_MASK, 63);     // (1 << 6) - 1 = 63
    }

    #[test]
    fn test_lc_global_locale_sentinel() {
        assert_eq!(LC_GLOBAL_LOCALE, usize::MAX);
    }
}
