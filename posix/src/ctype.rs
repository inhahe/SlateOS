//! C character classification and conversion functions.
//!
//! Implements the `<ctype.h>` interface: `isalpha`, `isdigit`, `isalnum`,
//! `isspace`, `isupper`, `islower`, `isprint`, `iscntrl`, `ispunct`,
//! `isxdigit`, `isgraph`, `isblank`, `isascii`, `toascii`, `toupper`,
//! `tolower`, plus POSIX.1-2008 `_l` locale variants of all the above.
//!
//! These operate on `int` values representing unsigned char values or EOF.
//! Characters outside 0-127 are treated as non-matching (C locale).

// ---------------------------------------------------------------------------
// Classification functions
// ---------------------------------------------------------------------------

/// Test for an alphabetic character (a-z, A-Z).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isalpha(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_alphabetic())
}

/// Test for a decimal digit (0-9).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isdigit(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_digit())
}

/// Test for an alphanumeric character.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isalnum(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_alphanumeric())
}

/// Test for a whitespace character.
///
/// Space, tab, newline, vertical tab, form feed, carriage return.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isspace(c: i32) -> i32 {
    // POSIX whitespace: space, \t, \n, \v (0x0B), \f (0x0C), \r.
    // Rust's is_ascii_whitespace() omits \v (vertical tab), so we
    // check manually.
    let b = c as u8;
    i32::from(b == b' ' || (b >= 0x09 && b <= 0x0D))
}

/// Test for an uppercase letter.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isupper(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_uppercase())
}

/// Test for a lowercase letter.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn islower(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_lowercase())
}

/// Test for a printing character (including space).
///
/// Printable characters are 0x20-0x7e.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isprint(c: i32) -> i32 {
    let u = c as u8;
    i32::from((0x20..=0x7e).contains(&u))
}

/// Test for a control character.
///
/// Control characters are 0x00-0x1f and 0x7f.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iscntrl(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_control())
}

/// Test for a punctuation character.
///
/// Printing characters that are not space or alphanumeric.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ispunct(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_punctuation())
}

/// Test for a hexadecimal digit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isxdigit(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_hexdigit())
}

/// Test for any printable character except space.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isgraph(c: i32) -> i32 {
    let u = c as u8;
    i32::from((0x21..=0x7e).contains(&u))
}

/// Test for a blank character (space or tab).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isblank(c: i32) -> i32 {
    i32::from(matches!(c as u8, b' ' | b'\t'))
}

/// Test whether a character is a 7-bit ASCII value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isascii(c: i32) -> i32 {
    i32::from((c & !0x7f) == 0)
}

// ---------------------------------------------------------------------------
// Conversion functions
// ---------------------------------------------------------------------------

/// Convert a character to its 7-bit ASCII equivalent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn toascii(c: i32) -> i32 {
    c & 0x7f
}

/// Convert a lowercase letter to uppercase.
///
/// If not lowercase, returns `c` unchanged.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn toupper(c: i32) -> i32 {
    if (c as u8).is_ascii_lowercase() {
        i32::from((c as u8).to_ascii_uppercase())
    } else {
        c
    }
}

/// Convert an uppercase letter to lowercase.
///
/// If not uppercase, returns `c` unchanged.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tolower(c: i32) -> i32 {
    if (c as u8).is_ascii_uppercase() {
        i32::from((c as u8).to_ascii_lowercase())
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// Locale-aware variants (_l suffix)
// ---------------------------------------------------------------------------
//
// POSIX.1-2008 locale-aware ctype functions.  Since we only support
// the C/POSIX locale, these all delegate to the non-locale versions.
// The `locale_t` parameter is accepted but ignored.

/// Locale type (opaque pointer).
type LocaleT = usize;

/// isalpha_l — locale-aware isalpha.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isalpha_l(c: i32, _locale: LocaleT) -> i32 { isalpha(c) }

/// isdigit_l — locale-aware isdigit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isdigit_l(c: i32, _locale: LocaleT) -> i32 { isdigit(c) }

/// isalnum_l — locale-aware isalnum.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isalnum_l(c: i32, _locale: LocaleT) -> i32 { isalnum(c) }

/// isspace_l — locale-aware isspace.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isspace_l(c: i32, _locale: LocaleT) -> i32 { isspace(c) }

/// isupper_l — locale-aware isupper.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isupper_l(c: i32, _locale: LocaleT) -> i32 { isupper(c) }

/// islower_l — locale-aware islower.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn islower_l(c: i32, _locale: LocaleT) -> i32 { islower(c) }

/// isprint_l — locale-aware isprint.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isprint_l(c: i32, _locale: LocaleT) -> i32 { isprint(c) }

/// iscntrl_l — locale-aware iscntrl.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iscntrl_l(c: i32, _locale: LocaleT) -> i32 { iscntrl(c) }

/// ispunct_l — locale-aware ispunct.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ispunct_l(c: i32, _locale: LocaleT) -> i32 { ispunct(c) }

/// isxdigit_l — locale-aware isxdigit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isxdigit_l(c: i32, _locale: LocaleT) -> i32 { isxdigit(c) }

/// isgraph_l — locale-aware isgraph.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isgraph_l(c: i32, _locale: LocaleT) -> i32 { isgraph(c) }

/// isblank_l — locale-aware isblank.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isblank_l(c: i32, _locale: LocaleT) -> i32 { isblank(c) }

/// toupper_l — locale-aware toupper.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn toupper_l(c: i32, _locale: LocaleT) -> i32 { toupper(c) }

/// tolower_l — locale-aware tolower.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tolower_l(c: i32, _locale: LocaleT) -> i32 { tolower(c) }

// ---------------------------------------------------------------------------
// glibc-compatible ctype lookup tables
// ---------------------------------------------------------------------------
//
// glibc-compiled programs may call __ctype_b_loc() to get a pointer to
// a pointer to a u16 classification table.  The table is indexed by
// (c + 128) to allow EOF (-1) indexing without UB.
//
// These are only needed for programs compiled against glibc headers
// that inline the is* macros.

/// glibc ctype bit flags.
const _ISU: u16 = 0x0100; // upper
const _ISL: u16 = 0x0200; // lower
const _ISA: u16 = 0x0400; // alpha = _ISU | _ISL
const _ISD: u16 = 0x0800; // digit
const _ISX: u16 = 0x1000; // xdigit
const _ISS: u16 = 0x2000; // space
const _ISP: u16 = 0x4000; // print
const _ISG: u16 = 0x8000; // graph (print minus space)
const _ISB: u16 = 0x0001; // blank (space, tab)
const _ISC: u16 = 0x0002; // cntrl
const _ISN: u16 = 0x0004; // punct
const _ISALNUM: u16 = 0x0008; // alnum = alpha | digit

/// Generate the classification flags for a single byte value.
const fn classify(c: u8) -> u16 {
    let mut f: u16 = 0;
    if c >= b'A' && c <= b'Z' { f |= _ISU | _ISA | _ISG | _ISP | _ISALNUM; }
    if c >= b'a' && c <= b'z' { f |= _ISL | _ISA | _ISG | _ISP | _ISALNUM; }
    if c >= b'0' && c <= b'9' { f |= _ISD | _ISG | _ISP | _ISALNUM; }
    if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r'
        || c == 0x0b || c == 0x0c { f |= _ISS; }
    if c == b' ' || c == b'\t' { f |= _ISB; }
    if c < 0x20 || c == 0x7f { f |= _ISC; }
    if c >= 0x20 && c <= 0x7e { f |= _ISP; }
    if c >= 0x21 && c <= 0x7e { f |= _ISG; }
    // Punct: printable + not alnum + not space
    if (c >= 0x21 && c <= 0x2f) || (c >= 0x3a && c <= 0x40)
        || (c >= 0x5b && c <= 0x60) || (c >= 0x7b && c <= 0x7e) { f |= _ISN; }
    // Hex digits.
    if (c >= b'a' && c <= b'f') || (c >= b'A' && c <= b'F') { f |= _ISX; }
    if c >= b'0' && c <= b'9' { f |= _ISX; }
    f
}

/// Build the full 384-entry ctype table at compile time.
///
/// Entries [-128..255] — indexed as table[c + 128].
/// Entries for c < 0 (except EOF) are zero.  EOF (-1) maps to index 127.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn build_ctype_table() -> [u16; 384] {
    let mut t = [0u16; 384];
    // Entries 128..384 cover byte values 0..255.
    let mut i: usize = 0;
    while i < 256 {
        t[i + 128] = classify(i as u8);
        i += 1;
    }
    t
}

/// The ctype classification table.
static CTYPE_TABLE: [u16; 384] = build_ctype_table();

/// glibc internal: return a pointer to the ctype classification table.
///
/// Programs compiled with glibc headers inline `isalpha(c)` as
/// `(*__ctype_b_loc())[(unsigned char)c] & flag`.
///
/// Returns a pointer to a pointer into the table at offset 128
/// (allowing indexing from -128 to 255).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ctype_b_loc() -> *const *const u16 {
    // Compute the pointer each call — avoids the Sync issue with
    // static raw pointers while remaining branchless.
    static mut CACHED: *const u16 = core::ptr::null();
    // SAFETY: single-threaded init; pointer is stable (points into
    // a static array that never moves).  Using addr_of_mut to comply
    // with Rust 2024 rules (no direct references to static mut).
    unsafe {
        let ptr = core::ptr::addr_of_mut!(CACHED);
        if ptr.read().is_null() {
            ptr.write(CTYPE_TABLE.as_ptr().add(128));
        }
        ptr.cast()
    }
}

/// Build the tolower table at compile time (384 entries, indexed by c+128).
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn build_tolower_table() -> [i32; 384] {
    let mut t = [0i32; 384];
    let mut i: usize = 0;
    while i < 384 {
        if i >= 128 {
            let c = (i - 128) as u8;
            if c >= b'A' && c <= b'Z' {
                t[i] = (c + 32) as i32;
            } else {
                t[i] = c as i32;
            }
        }
        // Entries below 128 (negative indices) are identity or zero.
        i += 1;
    }
    t
}

/// Build the toupper table at compile time.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn build_toupper_table() -> [i32; 384] {
    let mut t = [0i32; 384];
    let mut i: usize = 0;
    while i < 384 {
        if i >= 128 {
            let c = (i - 128) as u8;
            if c >= b'a' && c <= b'z' {
                t[i] = (c - 32) as i32;
            } else {
                t[i] = c as i32;
            }
        }
        i += 1;
    }
    t
}

static TOLOWER_TABLE: [i32; 384] = build_tolower_table();
static TOUPPER_TABLE: [i32; 384] = build_toupper_table();

/// glibc internal: return a pointer to the tolower conversion table.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ctype_tolower_loc() -> *const *const i32 {
    static mut CACHED: *const i32 = core::ptr::null();
    // SAFETY: single-threaded init; using addr_of_mut for Rust 2024.
    unsafe {
        let ptr = core::ptr::addr_of_mut!(CACHED);
        if ptr.read().is_null() {
            ptr.write(TOLOWER_TABLE.as_ptr().add(128));
        }
        ptr.cast()
    }
}

/// glibc internal: return a pointer to the toupper conversion table.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ctype_toupper_loc() -> *const *const i32 {
    static mut CACHED: *const i32 = core::ptr::null();
    // SAFETY: single-threaded init; using addr_of_mut for Rust 2024.
    unsafe {
        let ptr = core::ptr::addr_of_mut!(CACHED);
        if ptr.read().is_null() {
            ptr.write(TOUPPER_TABLE.as_ptr().add(128));
        }
        ptr.cast()
    }
}

/// glibc: return maximum number of bytes in a multibyte character.
///
/// For UTF-8 (our encoding), this is always 4.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ctype_get_mb_cur_max() -> usize {
    4 // UTF-8: up to 4 bytes per character.
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // isalpha
    // -----------------------------------------------------------------------

    #[test]
    fn test_isalpha_lowercase() {
        for c in b'a'..=b'z' {
            assert_ne!(isalpha(i32::from(c)), 0, "isalpha({}) should be true", c as char);
        }
    }

    #[test]
    fn test_isalpha_uppercase() {
        for c in b'A'..=b'Z' {
            assert_ne!(isalpha(i32::from(c)), 0, "isalpha({}) should be true", c as char);
        }
    }

    #[test]
    fn test_isalpha_digits() {
        for c in b'0'..=b'9' {
            assert_eq!(isalpha(i32::from(c)), 0, "isalpha({}) should be false", c as char);
        }
    }

    #[test]
    fn test_isalpha_space_and_punct() {
        assert_eq!(isalpha(i32::from(b' ')), 0);
        assert_eq!(isalpha(i32::from(b'!')), 0);
        assert_eq!(isalpha(i32::from(b'@')), 0);
    }

    // -----------------------------------------------------------------------
    // isdigit
    // -----------------------------------------------------------------------

    #[test]
    fn test_isdigit_digits() {
        for c in b'0'..=b'9' {
            assert_ne!(isdigit(i32::from(c)), 0, "isdigit({}) should be true", c as char);
        }
    }

    #[test]
    fn test_isdigit_letters() {
        assert_eq!(isdigit(i32::from(b'a')), 0);
        assert_eq!(isdigit(i32::from(b'Z')), 0);
    }

    #[test]
    fn test_isdigit_space_and_punct() {
        assert_eq!(isdigit(i32::from(b' ')), 0);
        assert_eq!(isdigit(i32::from(b'.')), 0);
    }

    // -----------------------------------------------------------------------
    // isalnum
    // -----------------------------------------------------------------------

    #[test]
    fn test_isalnum_alpha() {
        assert_ne!(isalnum(i32::from(b'a')), 0);
        assert_ne!(isalnum(i32::from(b'Z')), 0);
    }

    #[test]
    fn test_isalnum_digit() {
        assert_ne!(isalnum(i32::from(b'0')), 0);
        assert_ne!(isalnum(i32::from(b'9')), 0);
    }

    #[test]
    fn test_isalnum_non_alnum() {
        assert_eq!(isalnum(i32::from(b' ')), 0);
        assert_eq!(isalnum(i32::from(b'!')), 0);
        assert_eq!(isalnum(i32::from(b'/')), 0);
    }

    // -----------------------------------------------------------------------
    // isspace
    // -----------------------------------------------------------------------

    #[test]
    fn test_isspace_whitespace_chars() {
        assert_ne!(isspace(i32::from(b' ')), 0);   // space
        assert_ne!(isspace(i32::from(b'\t')), 0);  // tab
        assert_ne!(isspace(i32::from(b'\n')), 0);  // newline
        assert_ne!(isspace(i32::from(b'\r')), 0);  // carriage return
        assert_ne!(isspace(0x0b), 0);               // vertical tab
        assert_ne!(isspace(0x0c), 0);               // form feed
    }

    #[test]
    fn test_isspace_non_whitespace() {
        assert_eq!(isspace(i32::from(b'A')), 0);
        assert_eq!(isspace(i32::from(b'0')), 0);
        assert_eq!(isspace(i32::from(b'!')), 0);
    }

    // -----------------------------------------------------------------------
    // isupper / islower
    // -----------------------------------------------------------------------

    #[test]
    fn test_isupper() {
        for c in b'A'..=b'Z' {
            assert_ne!(isupper(i32::from(c)), 0);
        }
        for c in b'a'..=b'z' {
            assert_eq!(isupper(i32::from(c)), 0);
        }
        assert_eq!(isupper(i32::from(b'0')), 0);
    }

    #[test]
    fn test_islower() {
        for c in b'a'..=b'z' {
            assert_ne!(islower(i32::from(c)), 0);
        }
        for c in b'A'..=b'Z' {
            assert_eq!(islower(i32::from(c)), 0);
        }
        assert_eq!(islower(i32::from(b'5')), 0);
    }

    // -----------------------------------------------------------------------
    // isprint
    // -----------------------------------------------------------------------

    #[test]
    fn test_isprint_printable_range() {
        // Printable characters are 0x20 (space) through 0x7e (~).
        assert_ne!(isprint(0x20), 0); // space
        assert_ne!(isprint(0x7e), 0); // '~'
        assert_ne!(isprint(i32::from(b'A')), 0);
        assert_ne!(isprint(i32::from(b'5')), 0);
        assert_ne!(isprint(i32::from(b'!')), 0);
    }

    #[test]
    fn test_isprint_control_chars() {
        assert_eq!(isprint(0x00), 0); // NUL
        assert_eq!(isprint(0x1f), 0); // last control char
        assert_eq!(isprint(0x7f), 0); // DEL
    }

    // -----------------------------------------------------------------------
    // iscntrl
    // -----------------------------------------------------------------------

    #[test]
    fn test_iscntrl_control_range() {
        for c in 0x00..=0x1f {
            assert_ne!(iscntrl(c), 0, "iscntrl(0x{c:02x}) should be true");
        }
        assert_ne!(iscntrl(0x7f), 0); // DEL
    }

    #[test]
    fn test_iscntrl_non_control() {
        assert_eq!(iscntrl(0x20), 0); // space is printable, not control
        assert_eq!(iscntrl(i32::from(b'A')), 0);
        assert_eq!(iscntrl(0x7e), 0);
    }

    // -----------------------------------------------------------------------
    // ispunct
    // -----------------------------------------------------------------------

    #[test]
    fn test_ispunct_punctuation() {
        let punct_chars = b"!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
        for &c in punct_chars {
            assert_ne!(ispunct(i32::from(c)), 0, "ispunct({}) should be true", c as char);
        }
    }

    #[test]
    fn test_ispunct_non_punct() {
        assert_eq!(ispunct(i32::from(b'A')), 0);
        assert_eq!(ispunct(i32::from(b'0')), 0);
        assert_eq!(ispunct(i32::from(b' ')), 0);
    }

    // -----------------------------------------------------------------------
    // isxdigit
    // -----------------------------------------------------------------------

    #[test]
    fn test_isxdigit_hex_chars() {
        for c in b'0'..=b'9' {
            assert_ne!(isxdigit(i32::from(c)), 0);
        }
        for c in b'a'..=b'f' {
            assert_ne!(isxdigit(i32::from(c)), 0);
        }
        for c in b'A'..=b'F' {
            assert_ne!(isxdigit(i32::from(c)), 0);
        }
    }

    #[test]
    fn test_isxdigit_non_hex() {
        assert_eq!(isxdigit(i32::from(b'g')), 0);
        assert_eq!(isxdigit(i32::from(b'G')), 0);
        assert_eq!(isxdigit(i32::from(b'z')), 0);
        assert_eq!(isxdigit(i32::from(b' ')), 0);
    }

    // -----------------------------------------------------------------------
    // isgraph
    // -----------------------------------------------------------------------

    #[test]
    fn test_isgraph_visible_chars() {
        // Graph = printable except space: 0x21..=0x7e.
        assert_ne!(isgraph(0x21), 0); // '!'
        assert_ne!(isgraph(0x7e), 0); // '~'
        assert_ne!(isgraph(i32::from(b'A')), 0);
        assert_ne!(isgraph(i32::from(b'5')), 0);
    }

    #[test]
    fn test_isgraph_space_and_control() {
        assert_eq!(isgraph(0x20), 0); // space is NOT graph
        assert_eq!(isgraph(0x00), 0); // NUL
        assert_eq!(isgraph(0x7f), 0); // DEL
    }

    // -----------------------------------------------------------------------
    // isblank
    // -----------------------------------------------------------------------

    #[test]
    fn test_isblank_blank_chars() {
        assert_ne!(isblank(i32::from(b' ')), 0);
        assert_ne!(isblank(i32::from(b'\t')), 0);
    }

    #[test]
    fn test_isblank_non_blank_whitespace() {
        // Newline, carriage return are whitespace but not blank.
        assert_eq!(isblank(i32::from(b'\n')), 0);
        assert_eq!(isblank(i32::from(b'\r')), 0);
        assert_eq!(isblank(0x0b), 0); // vertical tab
    }

    #[test]
    fn test_isblank_non_whitespace() {
        assert_eq!(isblank(i32::from(b'A')), 0);
        assert_eq!(isblank(i32::from(b'0')), 0);
    }

    // -----------------------------------------------------------------------
    // isascii
    // -----------------------------------------------------------------------

    #[test]
    fn test_isascii_valid_range() {
        for c in 0..=127 {
            assert_ne!(isascii(c), 0, "isascii({c}) should be true");
        }
    }

    #[test]
    fn test_isascii_above_range() {
        assert_eq!(isascii(128), 0);
        assert_eq!(isascii(255), 0);
        assert_eq!(isascii(256), 0);
        assert_eq!(isascii(1000), 0);
    }

    #[test]
    fn test_isascii_negative() {
        assert_eq!(isascii(-1), 0);  // EOF
        assert_eq!(isascii(-128), 0);
    }

    // -----------------------------------------------------------------------
    // toupper / tolower
    // -----------------------------------------------------------------------

    #[test]
    fn test_toupper_lowercase_letters() {
        for c in b'a'..=b'z' {
            let expected = c - 32; // 'a' - 'A' = 32
            assert_eq!(toupper(i32::from(c)), i32::from(expected));
        }
    }

    #[test]
    fn test_toupper_already_upper() {
        for c in b'A'..=b'Z' {
            assert_eq!(toupper(i32::from(c)), i32::from(c));
        }
    }

    #[test]
    fn test_toupper_non_alpha() {
        assert_eq!(toupper(i32::from(b'0')), i32::from(b'0'));
        assert_eq!(toupper(i32::from(b' ')), i32::from(b' '));
        assert_eq!(toupper(i32::from(b'!')), i32::from(b'!'));
    }

    #[test]
    fn test_tolower_uppercase_letters() {
        for c in b'A'..=b'Z' {
            let expected = c + 32;
            assert_eq!(tolower(i32::from(c)), i32::from(expected));
        }
    }

    #[test]
    fn test_tolower_already_lower() {
        for c in b'a'..=b'z' {
            assert_eq!(tolower(i32::from(c)), i32::from(c));
        }
    }

    #[test]
    fn test_tolower_non_alpha() {
        assert_eq!(tolower(i32::from(b'5')), i32::from(b'5'));
        assert_eq!(tolower(i32::from(b' ')), i32::from(b' '));
        assert_eq!(tolower(i32::from(b'@')), i32::from(b'@'));
    }

    // -----------------------------------------------------------------------
    // toascii
    // -----------------------------------------------------------------------

    #[test]
    fn test_toascii_strips_high_bit() {
        assert_eq!(toascii(0x80), 0x00);
        assert_eq!(toascii(0xff), 0x7f);
        assert_eq!(toascii(0xc1), 0x41); // 0xc1 & 0x7f = 'A'
    }

    #[test]
    fn test_toascii_preserves_ascii() {
        for c in 0..=0x7f {
            assert_eq!(toascii(c), c);
        }
    }

    // -----------------------------------------------------------------------
    // Edge cases: boundaries (0, 31, 32, 126, 127)
    // -----------------------------------------------------------------------

    #[test]
    fn test_boundary_nul() {
        // NUL (0): control, not printable, not graph, not alpha, etc.
        assert_ne!(iscntrl(0), 0);
        assert_eq!(isprint(0), 0);
        assert_eq!(isgraph(0), 0);
        assert_eq!(isalpha(0), 0);
        assert_eq!(isdigit(0), 0);
        assert_eq!(isspace(0), 0);
        assert_eq!(isblank(0), 0);
        assert_eq!(ispunct(0), 0);
        assert_ne!(isascii(0), 0);
    }

    #[test]
    fn test_boundary_0x1f() {
        // 0x1f (US, last control char before space): control only.
        assert_ne!(iscntrl(0x1f), 0);
        assert_eq!(isprint(0x1f), 0);
        assert_eq!(isgraph(0x1f), 0);
        assert_ne!(isascii(0x1f), 0);
    }

    #[test]
    fn test_boundary_space_0x20() {
        // 0x20 (space): printable, blank, whitespace, NOT graph, NOT control.
        assert_ne!(isprint(0x20), 0);
        assert_ne!(isblank(0x20), 0);
        assert_ne!(isspace(0x20), 0);
        assert_eq!(isgraph(0x20), 0);
        assert_eq!(iscntrl(0x20), 0);
        assert_ne!(isascii(0x20), 0);
    }

    #[test]
    fn test_boundary_tilde_0x7e() {
        // 0x7e ('~'): printable, graph, punct, NOT control.
        assert_ne!(isprint(0x7e), 0);
        assert_ne!(isgraph(0x7e), 0);
        assert_ne!(ispunct(0x7e), 0);
        assert_eq!(iscntrl(0x7e), 0);
        assert_ne!(isascii(0x7e), 0);
    }

    #[test]
    fn test_boundary_del_0x7f() {
        // 0x7f (DEL): control, NOT printable, NOT graph, last ASCII char.
        assert_ne!(iscntrl(0x7f), 0);
        assert_eq!(isprint(0x7f), 0);
        assert_eq!(isgraph(0x7f), 0);
        assert_ne!(isascii(0x7f), 0);
    }

    // -----------------------------------------------------------------------
    // Edge cases: EOF (-1) and values > 127
    // -----------------------------------------------------------------------
    //
    // Implementation detail: the classification functions cast `c as u8`,
    // which truncates.  -1i32 as u8 = 255, which is not a valid ASCII
    // character, so most classifiers return false.  isascii checks the
    // full i32 value via bit masking and correctly rejects -1.

    #[test]
    fn test_eof_minus_one() {
        let eof: i32 = -1;
        assert_eq!(isascii(eof), 0);
        // toupper/tolower should pass through non-letter values unchanged.
        assert_eq!(toupper(eof), eof);
        assert_eq!(tolower(eof), eof);
    }

    #[test]
    fn test_values_above_127() {
        // Values 128..=255 are outside 7-bit ASCII.
        assert_eq!(isascii(128), 0);
        assert_eq!(isascii(200), 0);
        assert_eq!(isascii(255), 0);
        // Should not be classified as letters.
        assert_eq!(isalpha(200), 0);
    }

    #[test]
    fn test_negative_values() {
        // Various negative values should not crash or misclassify.
        assert_eq!(isascii(-1), 0);
        assert_eq!(isascii(-128), 0);
        assert_eq!(isalpha(-1), 0);
        assert_eq!(isdigit(-100), 0);
    }

    // -----------------------------------------------------------------------
    // Locale-aware variants (_l suffix)
    // -----------------------------------------------------------------------
    //
    // These should behave identically to their non-locale counterparts
    // since we only support the C locale.

    #[test]
    fn test_locale_variants_delegate() {
        let locale: LocaleT = 0; // dummy locale

        assert_eq!(isalpha_l(i32::from(b'A'), locale), isalpha(i32::from(b'A')));
        assert_eq!(isdigit_l(i32::from(b'5'), locale), isdigit(i32::from(b'5')));
        assert_eq!(isalnum_l(i32::from(b'z'), locale), isalnum(i32::from(b'z')));
        assert_eq!(isspace_l(i32::from(b' '), locale), isspace(i32::from(b' ')));
        assert_eq!(isupper_l(i32::from(b'A'), locale), isupper(i32::from(b'A')));
        assert_eq!(islower_l(i32::from(b'a'), locale), islower(i32::from(b'a')));
        assert_eq!(isprint_l(i32::from(b'!'), locale), isprint(i32::from(b'!')));
        assert_eq!(iscntrl_l(0x01, locale), iscntrl(0x01));
        assert_eq!(ispunct_l(i32::from(b'.'), locale), ispunct(i32::from(b'.')));
        assert_eq!(isxdigit_l(i32::from(b'f'), locale), isxdigit(i32::from(b'f')));
        assert_eq!(isgraph_l(i32::from(b'G'), locale), isgraph(i32::from(b'G')));
        assert_eq!(isblank_l(i32::from(b'\t'), locale), isblank(i32::from(b'\t')));
        assert_eq!(toupper_l(i32::from(b'a'), locale), toupper(i32::from(b'a')));
        assert_eq!(tolower_l(i32::from(b'A'), locale), tolower(i32::from(b'A')));
    }

    // -----------------------------------------------------------------------
    // glibc ctype table
    // -----------------------------------------------------------------------

    #[test]
    fn test_ctype_b_loc_not_null() {
        let pp = __ctype_b_loc();
        assert!(!pp.is_null());
        let p = unsafe { *pp };
        assert!(!p.is_null());
    }

    #[test]
    fn test_ctype_table_alpha_bit() {
        let pp = __ctype_b_loc();
        let p = unsafe { *pp };
        // Index 'A' should have the alpha bit set.
        let flags = unsafe { *p.offset(i32::from(b'A') as isize) };
        assert_ne!(flags & _ISA, 0, "ctype table should have alpha bit for 'A'");
        // Index '0' should NOT have the alpha bit.
        let flags = unsafe { *p.offset(i32::from(b'0') as isize) };
        assert_eq!(flags & _ISA, 0, "ctype table should not have alpha bit for '0'");
    }

    #[test]
    fn test_ctype_table_digit_bit() {
        let pp = __ctype_b_loc();
        let p = unsafe { *pp };
        let flags = unsafe { *p.offset(i32::from(b'5') as isize) };
        assert_ne!(flags & _ISD, 0, "ctype table should have digit bit for '5'");
    }

    #[test]
    fn test_ctype_tolower_loc_not_null() {
        let pp = __ctype_tolower_loc();
        assert!(!pp.is_null());
        let p = unsafe { *pp };
        assert!(!p.is_null());
    }

    #[test]
    fn test_ctype_tolower_table() {
        let pp = __ctype_tolower_loc();
        let p = unsafe { *pp };
        // 'A' should map to 'a'.
        let result = unsafe { *p.offset(i32::from(b'A') as isize) };
        assert_eq!(result, i32::from(b'a'));
        // '5' should map to '5' (unchanged).
        let result = unsafe { *p.offset(i32::from(b'5') as isize) };
        assert_eq!(result, i32::from(b'5'));
    }

    #[test]
    fn test_ctype_toupper_loc_not_null() {
        let pp = __ctype_toupper_loc();
        assert!(!pp.is_null());
        let p = unsafe { *pp };
        assert!(!p.is_null());
    }

    #[test]
    fn test_ctype_toupper_table() {
        let pp = __ctype_toupper_loc();
        let p = unsafe { *pp };
        // 'a' should map to 'A'.
        let result = unsafe { *p.offset(i32::from(b'a') as isize) };
        assert_eq!(result, i32::from(b'A'));
        // 'Z' should map to 'Z' (unchanged).
        let result = unsafe { *p.offset(i32::from(b'Z') as isize) };
        assert_eq!(result, i32::from(b'Z'));
    }

    #[test]
    fn test_ctype_get_mb_cur_max() {
        assert_eq!(__ctype_get_mb_cur_max(), 4);
    }

    // -----------------------------------------------------------------------
    // Locale-aware variants — individual function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_isalpha_l_all_lowercase() {
        for c in b'a'..=b'z' {
            assert_ne!(isalpha_l(i32::from(c), 0), 0);
        }
    }

    #[test]
    fn test_isdigit_l_all_digits() {
        for c in b'0'..=b'9' {
            assert_ne!(isdigit_l(i32::from(c), 0), 0);
        }
        assert_eq!(isdigit_l(i32::from(b'a'), 0), 0);
    }

    #[test]
    fn test_isalnum_l_rejects_punct() {
        assert_eq!(isalnum_l(i32::from(b'!'), 0), 0);
        assert_eq!(isalnum_l(i32::from(b'@'), 0), 0);
        assert_eq!(isalnum_l(i32::from(b' '), 0), 0);
    }

    #[test]
    fn test_isspace_l_all_whitespace() {
        assert_ne!(isspace_l(i32::from(b' '), 0), 0);
        assert_ne!(isspace_l(i32::from(b'\t'), 0), 0);
        assert_ne!(isspace_l(i32::from(b'\n'), 0), 0);
        assert_ne!(isspace_l(0x0b, 0), 0); // vertical tab
        assert_ne!(isspace_l(0x0c, 0), 0); // form feed
        assert_ne!(isspace_l(i32::from(b'\r'), 0), 0);
    }

    #[test]
    fn test_isupper_l_all_uppercase() {
        for c in b'A'..=b'Z' {
            assert_ne!(isupper_l(i32::from(c), 0), 0);
        }
        assert_eq!(isupper_l(i32::from(b'a'), 0), 0);
    }

    #[test]
    fn test_islower_l_all_lowercase() {
        for c in b'a'..=b'z' {
            assert_ne!(islower_l(i32::from(c), 0), 0);
        }
        assert_eq!(islower_l(i32::from(b'A'), 0), 0);
    }

    #[test]
    fn test_isprint_l_boundaries() {
        assert_ne!(isprint_l(0x20, 0), 0);  // space
        assert_ne!(isprint_l(0x7e, 0), 0);  // '~'
        assert_eq!(isprint_l(0x1f, 0), 0);  // US
        assert_eq!(isprint_l(0x7f, 0), 0);  // DEL
    }

    #[test]
    fn test_iscntrl_l_boundaries() {
        assert_ne!(iscntrl_l(0x00, 0), 0); // NUL
        assert_ne!(iscntrl_l(0x1f, 0), 0); // US
        assert_ne!(iscntrl_l(0x7f, 0), 0); // DEL
        assert_eq!(iscntrl_l(0x20, 0), 0); // space
    }

    #[test]
    fn test_ispunct_l_samples() {
        assert_ne!(ispunct_l(i32::from(b'!'), 0), 0);
        assert_ne!(ispunct_l(i32::from(b'@'), 0), 0);
        assert_ne!(ispunct_l(i32::from(b'~'), 0), 0);
        assert_eq!(ispunct_l(i32::from(b'A'), 0), 0);
    }

    #[test]
    fn test_isxdigit_l_boundaries() {
        assert_ne!(isxdigit_l(i32::from(b'0'), 0), 0);
        assert_ne!(isxdigit_l(i32::from(b'9'), 0), 0);
        assert_ne!(isxdigit_l(i32::from(b'a'), 0), 0);
        assert_ne!(isxdigit_l(i32::from(b'f'), 0), 0);
        assert_ne!(isxdigit_l(i32::from(b'A'), 0), 0);
        assert_ne!(isxdigit_l(i32::from(b'F'), 0), 0);
        assert_eq!(isxdigit_l(i32::from(b'g'), 0), 0);
        assert_eq!(isxdigit_l(i32::from(b'G'), 0), 0);
    }

    #[test]
    fn test_isgraph_l_space_excluded() {
        assert_ne!(isgraph_l(i32::from(b'A'), 0), 0);
        assert_eq!(isgraph_l(i32::from(b' '), 0), 0);
    }

    #[test]
    fn test_isblank_l_only_space_tab() {
        assert_ne!(isblank_l(i32::from(b' '), 0), 0);
        assert_ne!(isblank_l(i32::from(b'\t'), 0), 0);
        assert_eq!(isblank_l(i32::from(b'\n'), 0), 0);
    }

    #[test]
    fn test_toupper_l_conversion() {
        for c in b'a'..=b'z' {
            assert_eq!(toupper_l(i32::from(c), 0), i32::from(c - 32));
        }
        assert_eq!(toupper_l(i32::from(b'5'), 0), i32::from(b'5'));
    }

    #[test]
    fn test_tolower_l_conversion() {
        for c in b'A'..=b'Z' {
            assert_eq!(tolower_l(i32::from(c), 0), i32::from(c + 32));
        }
        assert_eq!(tolower_l(i32::from(b'9'), 0), i32::from(b'9'));
    }

    // -----------------------------------------------------------------------
    // Cross-classification consistency tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_graph_is_print_minus_space() {
        // For all ASCII chars: isgraph(c) iff (isprint(c) && c != ' ')
        for c in 0..=127i32 {
            let g = isgraph(c) != 0;
            let p = isprint(c) != 0;
            let is_space = c == i32::from(b' ');
            assert_eq!(g, p && !is_space, "isgraph/isprint mismatch at {c}");
        }
    }

    #[test]
    fn test_alnum_is_alpha_or_digit() {
        for c in 0..=127i32 {
            let an = isalnum(c) != 0;
            let a = isalpha(c) != 0;
            let d = isdigit(c) != 0;
            assert_eq!(an, a || d, "isalnum mismatch at {c}");
        }
    }

    #[test]
    fn test_alpha_is_upper_or_lower() {
        for c in 0..=127i32 {
            let a = isalpha(c) != 0;
            let u = isupper(c) != 0;
            let l = islower(c) != 0;
            assert_eq!(a, u || l, "isalpha mismatch at {c}");
        }
    }

    #[test]
    fn test_xdigit_superset_of_digit() {
        for c in 0..=127i32 {
            if isdigit(c) != 0 {
                assert_ne!(isxdigit(c), 0, "digit {c} should be xdigit too");
            }
        }
    }

    #[test]
    fn test_print_covers_graph_and_space() {
        // Every graph char is printable; space is printable but not graph
        for c in 0..=127i32 {
            if isgraph(c) != 0 {
                assert_ne!(isprint(c), 0, "graph char {c} must be printable");
            }
        }
        assert_ne!(isprint(i32::from(b' ')), 0);
        assert_eq!(isgraph(i32::from(b' ')), 0);
    }

    #[test]
    fn test_cntrl_and_print_disjoint() {
        // Control and printable chars should never overlap
        for c in 0..=127i32 {
            let ctrl = iscntrl(c) != 0;
            let prt = isprint(c) != 0;
            assert!(!(ctrl && prt), "char {c} is both control and printable");
        }
    }

    #[test]
    fn test_blank_subset_of_space() {
        // Every blank char should also be a space char
        for c in 0..=127i32 {
            if isblank(c) != 0 {
                assert_ne!(isspace(c), 0, "blank char {c} should be space too");
            }
        }
    }

    // -----------------------------------------------------------------------
    // glibc ctype table consistency tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ctype_table_space_bit() {
        let pp = __ctype_b_loc();
        let p = unsafe { *pp };
        let flags = unsafe { *p.offset(i32::from(b' ') as isize) };
        assert_ne!(flags & _ISS, 0, "space should have _ISS bit");
        assert_ne!(flags & _ISB, 0, "space should have _ISB (blank) bit");
        assert_ne!(flags & _ISP, 0, "space should have _ISP (print) bit");
    }

    #[test]
    fn test_ctype_table_control_char() {
        let pp = __ctype_b_loc();
        let p = unsafe { *pp };
        // NUL should have control bit
        let flags = unsafe { *p.offset(0) };
        assert_ne!(flags & _ISC, 0, "NUL should have control bit");
        assert_eq!(flags & _ISP, 0, "NUL should not have print bit");
    }

    #[test]
    fn test_ctype_table_punct_chars() {
        let pp = __ctype_b_loc();
        let p = unsafe { *pp };
        // '!' should have punct + print + graph bits
        let flags = unsafe { *p.offset(i32::from(b'!') as isize) };
        assert_ne!(flags & _ISN, 0, "'!' should have punct bit");
        assert_ne!(flags & _ISP, 0, "'!' should have print bit");
        assert_ne!(flags & _ISG, 0, "'!' should have graph bit");
    }

    #[test]
    fn test_ctype_table_xdigit() {
        let pp = __ctype_b_loc();
        let p = unsafe { *pp };
        // 'a' should have xdigit + alpha + lower bits
        let flags = unsafe { *p.offset(i32::from(b'a') as isize) };
        assert_ne!(flags & _ISX, 0, "'a' should have xdigit bit");
        assert_ne!(flags & _ISA, 0, "'a' should have alpha bit");
        assert_ne!(flags & _ISL, 0, "'a' should have lower bit");
    }

    #[test]
    fn test_ctype_table_upper_digit() {
        let pp = __ctype_b_loc();
        let p = unsafe { *pp };
        // 'Z' should have upper + alpha + alnum bits
        let flags = unsafe { *p.offset(i32::from(b'Z') as isize) };
        assert_ne!(flags & _ISU, 0, "'Z' should have upper bit");
        assert_ne!(flags & _ISA, 0, "'Z' should have alpha bit");
        assert_ne!(flags & _ISALNUM, 0, "'Z' should have alnum bit");
    }

    #[test]
    fn test_ctype_tolower_all_uppercase() {
        let pp = __ctype_tolower_loc();
        let p = unsafe { *pp };
        for c in b'A'..=b'Z' {
            let result = unsafe { *p.offset(i32::from(c) as isize) };
            assert_eq!(result, i32::from(c + 32), "tolower table: {c}");
        }
    }

    #[test]
    fn test_ctype_toupper_all_lowercase() {
        let pp = __ctype_toupper_loc();
        let p = unsafe { *pp };
        for c in b'a'..=b'z' {
            let result = unsafe { *p.offset(i32::from(c) as isize) };
            assert_eq!(result, i32::from(c - 32), "toupper table: {c}");
        }
    }

    #[test]
    fn test_ctype_tolower_digits_unchanged() {
        let pp = __ctype_tolower_loc();
        let p = unsafe { *pp };
        for c in b'0'..=b'9' {
            let result = unsafe { *p.offset(i32::from(c) as isize) };
            assert_eq!(result, i32::from(c));
        }
    }

    #[test]
    fn test_ctype_toupper_digits_unchanged() {
        let pp = __ctype_toupper_loc();
        let p = unsafe { *pp };
        for c in b'0'..=b'9' {
            let result = unsafe { *p.offset(i32::from(c) as isize) };
            assert_eq!(result, i32::from(c));
        }
    }

    // -----------------------------------------------------------------------
    // toupper / tolower roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_toupper_tolower_roundtrip() {
        for c in b'a'..=b'z' {
            let upper = toupper(i32::from(c));
            let back = tolower(upper);
            assert_eq!(back, i32::from(c), "roundtrip failed for '{}'", c as char);
        }
    }

    #[test]
    fn test_tolower_toupper_roundtrip() {
        for c in b'A'..=b'Z' {
            let lower = tolower(i32::from(c));
            let back = toupper(lower);
            assert_eq!(back, i32::from(c), "roundtrip failed for '{}'", c as char);
        }
    }

    // -----------------------------------------------------------------------
    // isspace: complete whitespace list
    // -----------------------------------------------------------------------

    #[test]
    fn test_isspace_exactly_six_chars() {
        let mut count = 0;
        for c in 0..=127i32 {
            if isspace(c) != 0 {
                count += 1;
            }
        }
        // POSIX whitespace: space, \t, \n, \v, \f, \r = 6 chars
        assert_eq!(count, 6, "exactly 6 ASCII whitespace characters");
    }

    #[test]
    fn test_isblank_exactly_two_chars() {
        let mut count = 0;
        for c in 0..=127i32 {
            if isblank(c) != 0 {
                count += 1;
            }
        }
        assert_eq!(count, 2, "exactly 2 blank characters (space and tab)");
    }
}
