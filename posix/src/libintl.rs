//! `<libintl.h>` — internationalization (gettext) stubs.
//!
//! Provides stub implementations of the gettext family of functions
//! used for message translation.
//!
//! ## Implementation
//!
//! Our OS does not support multiple locales or message catalogs.
//! All functions return the original (untranslated) message string,
//! which is the correct behavior for the "C" / POSIX locale.
//!
//! This satisfies link-time references from programs that call
//! gettext for i18n without requiring actual catalog files.

// ---------------------------------------------------------------------------
// gettext / dgettext / dcgettext
// ---------------------------------------------------------------------------

/// `gettext` — look up a message in the current text domain.
///
/// Returns the translation of `msgid` in the current locale.
///
/// Stub: returns `msgid` unchanged (C locale — no translation).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gettext(msgid: *const u8) -> *const u8 {
    msgid
}

/// `dgettext` — look up a message in a specific text domain.
///
/// Stub: returns `msgid` unchanged.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dgettext(
    _domainname: *const u8,
    msgid: *const u8,
) -> *const u8 {
    msgid
}

/// `dcgettext` — look up a message in a specific domain and category.
///
/// Stub: returns `msgid` unchanged.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dcgettext(
    _domainname: *const u8,
    msgid: *const u8,
    _category: i32,
) -> *const u8 {
    msgid
}

// ---------------------------------------------------------------------------
// ngettext / dngettext / dcngettext
// ---------------------------------------------------------------------------

/// `ngettext` — look up a message with plural form.
///
/// Returns `msgid1` when `n == 1`, `msgid2` otherwise (English/C
/// locale plural rule).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ngettext(
    msgid1: *const u8,
    msgid2: *const u8,
    n: u64,
) -> *const u8 {
    if n == 1 { msgid1 } else { msgid2 }
}

/// `dngettext` — plural lookup in a specific text domain.
///
/// Stub: returns `msgid1` when `n == 1`, `msgid2` otherwise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dngettext(
    _domainname: *const u8,
    msgid1: *const u8,
    msgid2: *const u8,
    n: u64,
) -> *const u8 {
    if n == 1 { msgid1 } else { msgid2 }
}

/// `dcngettext` — plural lookup in a specific domain and category.
///
/// Stub: returns `msgid1` when `n == 1`, `msgid2` otherwise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dcngettext(
    _domainname: *const u8,
    msgid1: *const u8,
    msgid2: *const u8,
    n: u64,
    _category: i32,
) -> *const u8 {
    if n == 1 { msgid1 } else { msgid2 }
}

// ---------------------------------------------------------------------------
// textdomain / bindtextdomain / bind_textdomain_codeset
// ---------------------------------------------------------------------------

/// Static storage for the current text domain name.
///
/// Default is "messages" per POSIX.
static mut CURRENT_DOMAIN: [u8; 256] = {
    let mut buf = [0u8; 256];
    buf[0] = b'm';
    buf[1] = b'e';
    buf[2] = b's';
    buf[3] = b's';
    buf[4] = b'a';
    buf[5] = b'g';
    buf[6] = b'e';
    buf[7] = b's';
    // buf[8] = 0  (already zero)
    buf
};

/// `textdomain` — set or query the current message domain.
///
/// If `domainname` is null, returns the current domain without
/// changing it.  Otherwise sets the domain and returns it.
///
/// The returned pointer is valid until the next call to `textdomain`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn textdomain(domainname: *const u8) -> *const u8 {
    if domainname.is_null() {
        // Return current domain.
        return unsafe { (&raw const CURRENT_DOMAIN).cast::<u8>() };
    }

    // Copy the new domain name into static storage.
    // SAFETY: single-threaded access.
    unsafe {
        let mut i = 0usize;
        // 256-byte buffer, reserve 1 for null.
        const MAX: usize = 255;
        while i < MAX {
            let b = *domainname.add(i);
            if b == 0 {
                break;
            }
            *(&raw mut CURRENT_DOMAIN).cast::<u8>().add(i) = b;
            i = i.wrapping_add(1);
        }
        *(&raw mut CURRENT_DOMAIN).cast::<u8>().add(i) = 0;
        (&raw const CURRENT_DOMAIN).cast::<u8>()
    }
}

/// `bindtextdomain` — bind a text domain to a directory.
///
/// Associates `domainname` with the directory `dirname` for catalog
/// lookup.
///
/// Stub: records nothing (no catalogs), returns `dirname`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn bindtextdomain(
    _domainname: *const u8,
    dirname: *const u8,
) -> *const u8 {
    dirname
}

/// `bind_textdomain_codeset` — set encoding for a text domain.
///
/// Stub: returns null (default encoding, which for us is always
/// ANSI_X3.4-1968 / ASCII).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn bind_textdomain_codeset(
    _domainname: *const u8,
    _codeset: *const u8,
) -> *const u8 {
    core::ptr::null()
}

// ---------------------------------------------------------------------------
// LC_MESSAGES category constant (for dcgettext/dcngettext)
// ---------------------------------------------------------------------------

/// `LC_MESSAGES` category value (matches our locale module).
pub const LC_MESSAGES: i32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // gettext
    // -----------------------------------------------------------------------

    #[test]
    fn test_gettext_returns_same_pointer() {
        let msg = b"Hello, world!\0".as_ptr();
        let result = gettext(msg);
        assert_eq!(result, msg);
    }

    #[test]
    fn test_gettext_null() {
        let result = gettext(core::ptr::null());
        assert!(result.is_null());
    }

    #[test]
    fn test_gettext_empty_string() {
        let msg = b"\0".as_ptr();
        let result = gettext(msg);
        assert_eq!(result, msg);
    }

    // -----------------------------------------------------------------------
    // dgettext
    // -----------------------------------------------------------------------

    #[test]
    fn test_dgettext_returns_msgid() {
        let msg = b"Some message\0".as_ptr();
        let result = dgettext(b"mydomain\0".as_ptr(), msg);
        assert_eq!(result, msg);
    }

    #[test]
    fn test_dgettext_null_domain() {
        let msg = b"msg\0".as_ptr();
        let result = dgettext(core::ptr::null(), msg);
        assert_eq!(result, msg);
    }

    #[test]
    fn test_dgettext_null_msgid() {
        let result = dgettext(b"dom\0".as_ptr(), core::ptr::null());
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // dcgettext
    // -----------------------------------------------------------------------

    #[test]
    fn test_dcgettext_returns_msgid() {
        let msg = b"translated\0".as_ptr();
        let result = dcgettext(b"dom\0".as_ptr(), msg, LC_MESSAGES);
        assert_eq!(result, msg);
    }

    #[test]
    fn test_dcgettext_ignores_category() {
        let msg = b"test\0".as_ptr();
        // Should return same result regardless of category.
        let r1 = dcgettext(core::ptr::null(), msg, 0);
        let r2 = dcgettext(core::ptr::null(), msg, LC_MESSAGES);
        let r3 = dcgettext(core::ptr::null(), msg, 99);
        assert_eq!(r1, msg);
        assert_eq!(r2, msg);
        assert_eq!(r3, msg);
    }

    // -----------------------------------------------------------------------
    // ngettext — plural forms
    // -----------------------------------------------------------------------

    #[test]
    fn test_ngettext_singular() {
        let s = b"1 file\0".as_ptr();
        let p = b"%d files\0".as_ptr();
        assert_eq!(ngettext(s, p, 1), s);
    }

    #[test]
    fn test_ngettext_plural_zero() {
        let s = b"1 file\0".as_ptr();
        let p = b"%d files\0".as_ptr();
        assert_eq!(ngettext(s, p, 0), p);
    }

    #[test]
    fn test_ngettext_plural_many() {
        let s = b"1 file\0".as_ptr();
        let p = b"%d files\0".as_ptr();
        assert_eq!(ngettext(s, p, 2), p);
        assert_eq!(ngettext(s, p, 100), p);
        assert_eq!(ngettext(s, p, u64::MAX), p);
    }

    // -----------------------------------------------------------------------
    // dngettext
    // -----------------------------------------------------------------------

    #[test]
    fn test_dngettext_singular() {
        let s = b"one\0".as_ptr();
        let p = b"many\0".as_ptr();
        assert_eq!(dngettext(b"dom\0".as_ptr(), s, p, 1), s);
    }

    #[test]
    fn test_dngettext_plural() {
        let s = b"one\0".as_ptr();
        let p = b"many\0".as_ptr();
        assert_eq!(dngettext(b"dom\0".as_ptr(), s, p, 5), p);
    }

    // -----------------------------------------------------------------------
    // dcngettext
    // -----------------------------------------------------------------------

    #[test]
    fn test_dcngettext_singular() {
        let s = b"item\0".as_ptr();
        let p = b"items\0".as_ptr();
        assert_eq!(dcngettext(core::ptr::null(), s, p, 1, LC_MESSAGES), s);
    }

    #[test]
    fn test_dcngettext_plural() {
        let s = b"item\0".as_ptr();
        let p = b"items\0".as_ptr();
        assert_eq!(dcngettext(core::ptr::null(), s, p, 3, LC_MESSAGES), p);
    }

    // -----------------------------------------------------------------------
    // textdomain
    // -----------------------------------------------------------------------

    #[test]
    fn test_textdomain_query() {
        // Query without changing.
        let result = textdomain(core::ptr::null());
        assert!(!result.is_null());
        // Should be "messages" or whatever was last set.
        let first = unsafe { *result };
        assert!(first.is_ascii(), "domain should be ASCII");
    }

    #[test]
    fn test_textdomain_set_and_query() {
        // Set a domain.
        let domain = b"myapp\0".as_ptr();
        let result = textdomain(domain);
        assert!(!result.is_null());

        // Verify by querying.
        let q = textdomain(core::ptr::null());
        assert!(!q.is_null());
        let name = unsafe { core::ffi::CStr::from_ptr(q.cast()) };
        assert_eq!(name.to_bytes(), b"myapp");

        // Restore default.
        textdomain(b"messages\0".as_ptr());
    }

    #[test]
    fn test_textdomain_overwrite() {
        textdomain(b"first\0".as_ptr());
        textdomain(b"second\0".as_ptr());
        let q = textdomain(core::ptr::null());
        let name = unsafe { core::ffi::CStr::from_ptr(q.cast()) };
        assert_eq!(name.to_bytes(), b"second");

        // Restore.
        textdomain(b"messages\0".as_ptr());
    }

    // -----------------------------------------------------------------------
    // bindtextdomain
    // -----------------------------------------------------------------------

    #[test]
    fn test_bindtextdomain_returns_dirname() {
        let dir = b"/usr/share/locale\0".as_ptr();
        let result = bindtextdomain(b"myapp\0".as_ptr(), dir);
        assert_eq!(result, dir);
    }

    #[test]
    fn test_bindtextdomain_null_dirname() {
        let result = bindtextdomain(b"myapp\0".as_ptr(), core::ptr::null());
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // bind_textdomain_codeset
    // -----------------------------------------------------------------------

    #[test]
    fn test_bind_textdomain_codeset_returns_null() {
        let result = bind_textdomain_codeset(
            b"myapp\0".as_ptr(),
            b"UTF-8\0".as_ptr(),
        );
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // LC_MESSAGES constant
    // -----------------------------------------------------------------------

    #[test]
    fn test_lc_messages() {
        assert_eq!(LC_MESSAGES, 5);
    }

    // -----------------------------------------------------------------------
    // Identity property: all translation functions are pass-through
    // -----------------------------------------------------------------------

    #[test]
    fn test_all_passthrough() {
        let msg = b"The quick brown fox\0".as_ptr();
        // Every translation function should return the message unchanged.
        assert_eq!(gettext(msg), msg);
        assert_eq!(dgettext(b"d\0".as_ptr(), msg), msg);
        assert_eq!(dcgettext(b"d\0".as_ptr(), msg, 0), msg);
        // ngettext with n=1 returns singular.
        assert_eq!(ngettext(msg, b"x\0".as_ptr(), 1), msg);
        assert_eq!(dngettext(b"d\0".as_ptr(), msg, b"x\0".as_ptr(), 1), msg);
        assert_eq!(dcngettext(b"d\0".as_ptr(), msg, b"x\0".as_ptr(), 1, 0), msg);
    }
}
