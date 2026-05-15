//! POSIX `<nl_types.h>` — message catalog functions.
//!
//! Stubs for `catopen`, `catgets`, `catclose`.  Our OS does not have
//! a message catalog infrastructure (`.cat` files, `gencat`, etc.),
//! so `catopen` always returns `(nl_catd)(-1)` and `catgets` always
//! returns the default string.
//!
//! These stubs satisfy link-time references from programs that use
//! `catgets` for localisation, falling back to the built-in default
//! strings (which is the correct POSIX behavior when no catalog is
//! available).

use crate::errno;

/// `nl_catd` — message catalog descriptor.
///
/// An opaque handle returned by `catopen`.  On failure (which is always,
/// for our stub), the value is `(nl_catd)(-1)`.
pub type NlCatd = isize;

/// `NL_SETD` — default message set number.
pub const NL_SETD: i32 = 1;

/// `NL_CAT_LOCALE` — use `LC_MESSAGES` locale for catopen.
pub const NL_CAT_LOCALE: i32 = 1;

// ---------------------------------------------------------------------------
// catopen
// ---------------------------------------------------------------------------

/// `catopen` — open a message catalog.
///
/// Always fails with `(nl_catd)(-1)` since we have no catalog support.
/// Sets errno to ENOENT.
///
/// Per POSIX, a failed catopen does not prevent the program from
/// continuing — `catgets` with an invalid descriptor returns the
/// default string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn catopen(_name: *const u8, _oflag: i32) -> NlCatd {
    errno::set_errno(errno::ENOENT);
    -1
}

// ---------------------------------------------------------------------------
// catgets
// ---------------------------------------------------------------------------

/// `catgets` — read a message from a catalog.
///
/// Since `catopen` always fails, the catalog descriptor is always
/// invalid.  Per POSIX, when the catalog descriptor is invalid or
/// the message is not found, `catgets` returns `s` (the default
/// string).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn catgets(
    _catd: NlCatd,
    _set_id: i32,
    _msg_id: i32,
    s: *const u8,
) -> *const u8 {
    // Always return the default string.
    s
}

// ---------------------------------------------------------------------------
// catclose
// ---------------------------------------------------------------------------

/// `catclose` — close a message catalog.
///
/// Since we never successfully open a catalog, this is a no-op.
/// Returns -1 with EBADF per POSIX (invalid descriptor).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn catclose(_catd: NlCatd) -> i32 {
    errno::set_errno(errno::EBADF);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_nl_setd() {
        assert_eq!(NL_SETD, 1);
    }

    #[test]
    fn test_nl_cat_locale() {
        assert_eq!(NL_CAT_LOCALE, 1);
    }

    #[test]
    fn test_nl_catd_size() {
        // NlCatd is isize — pointer-sized.
        assert_eq!(core::mem::size_of::<NlCatd>(), core::mem::size_of::<isize>());
    }

    // -----------------------------------------------------------------------
    // catopen
    // -----------------------------------------------------------------------

    #[test]
    fn test_catopen_returns_invalid() {
        crate::errno::set_errno(0);
        let catd = catopen(b"messages.cat\0".as_ptr(), 0);
        assert_eq!(catd, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_catopen_null_name() {
        crate::errno::set_errno(0);
        let catd = catopen(core::ptr::null(), 0);
        assert_eq!(catd, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_catopen_with_nl_cat_locale() {
        crate::errno::set_errno(0);
        let catd = catopen(b"test.cat\0".as_ptr(), NL_CAT_LOCALE);
        assert_eq!(catd, -1);
    }

    // -----------------------------------------------------------------------
    // catgets
    // -----------------------------------------------------------------------

    #[test]
    fn test_catgets_returns_default_string() {
        let default = b"default message\0".as_ptr();
        let result = catgets(-1, NL_SETD, 1, default);
        assert_eq!(result, default);
    }

    #[test]
    fn test_catgets_returns_null_default() {
        let result = catgets(-1, 1, 1, core::ptr::null());
        assert!(result.is_null());
    }

    #[test]
    fn test_catgets_with_valid_looking_catd() {
        // Even with a non-(-1) catd, we return the default.
        let default = b"fallback\0".as_ptr();
        let result = catgets(42, 1, 1, default);
        assert_eq!(result, default);
    }

    #[test]
    fn test_catgets_various_set_msg_ids() {
        let s = b"test\0".as_ptr();
        // Different set/msg IDs should all return the default.
        assert_eq!(catgets(-1, 0, 0, s), s);
        assert_eq!(catgets(-1, 1, 1, s), s);
        assert_eq!(catgets(-1, 100, 200, s), s);
        assert_eq!(catgets(-1, -1, -1, s), s);
    }

    // -----------------------------------------------------------------------
    // catclose
    // -----------------------------------------------------------------------

    #[test]
    fn test_catclose_returns_error() {
        crate::errno::set_errno(0);
        let ret = catclose(-1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_catclose_any_descriptor() {
        crate::errno::set_errno(0);
        let ret = catclose(42);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -----------------------------------------------------------------------
    // Full workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_catopen_catgets_catclose_workflow() {
        // Typical usage pattern: open → get messages → close.
        let catd = catopen(b"app.cat\0".as_ptr(), NL_CAT_LOCALE);
        assert_eq!(catd, -1); // open fails

        // catgets falls back to default strings.
        let msg1 = catgets(catd, 1, 1, b"File not found\0".as_ptr());
        let msg2 = catgets(catd, 1, 2, b"Permission denied\0".as_ptr());
        assert_eq!(msg1, b"File not found\0".as_ptr());
        assert_eq!(msg2, b"Permission denied\0".as_ptr());

        // Close is a no-op (returns error since descriptor is invalid).
        let ret = catclose(catd);
        assert_eq!(ret, -1);
    }
}
