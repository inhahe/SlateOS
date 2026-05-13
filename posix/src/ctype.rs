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
#[unsafe(no_mangle)]
pub extern "C" fn isalpha(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_alphabetic())
}

/// Test for a decimal digit (0-9).
#[unsafe(no_mangle)]
pub extern "C" fn isdigit(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_digit())
}

/// Test for an alphanumeric character.
#[unsafe(no_mangle)]
pub extern "C" fn isalnum(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_alphanumeric())
}

/// Test for a whitespace character.
///
/// Space, tab, newline, vertical tab, form feed, carriage return.
#[unsafe(no_mangle)]
pub extern "C" fn isspace(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_whitespace())
}

/// Test for an uppercase letter.
#[unsafe(no_mangle)]
pub extern "C" fn isupper(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_uppercase())
}

/// Test for a lowercase letter.
#[unsafe(no_mangle)]
pub extern "C" fn islower(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_lowercase())
}

/// Test for a printing character (including space).
///
/// Printable characters are 0x20-0x7e.
#[unsafe(no_mangle)]
pub extern "C" fn isprint(c: i32) -> i32 {
    let u = c as u8;
    i32::from((0x20..=0x7e).contains(&u))
}

/// Test for a control character.
///
/// Control characters are 0x00-0x1f and 0x7f.
#[unsafe(no_mangle)]
pub extern "C" fn iscntrl(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_control())
}

/// Test for a punctuation character.
///
/// Printing characters that are not space or alphanumeric.
#[unsafe(no_mangle)]
pub extern "C" fn ispunct(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_punctuation())
}

/// Test for a hexadecimal digit.
#[unsafe(no_mangle)]
pub extern "C" fn isxdigit(c: i32) -> i32 {
    i32::from((c as u8).is_ascii_hexdigit())
}

/// Test for any printable character except space.
#[unsafe(no_mangle)]
pub extern "C" fn isgraph(c: i32) -> i32 {
    let u = c as u8;
    i32::from((0x21..=0x7e).contains(&u))
}

/// Test for a blank character (space or tab).
#[unsafe(no_mangle)]
pub extern "C" fn isblank(c: i32) -> i32 {
    i32::from(matches!(c as u8, b' ' | b'\t'))
}

/// Test whether a character is a 7-bit ASCII value.
#[unsafe(no_mangle)]
pub extern "C" fn isascii(c: i32) -> i32 {
    i32::from((c & !0x7f) == 0)
}

// ---------------------------------------------------------------------------
// Conversion functions
// ---------------------------------------------------------------------------

/// Convert a character to its 7-bit ASCII equivalent.
#[unsafe(no_mangle)]
pub extern "C" fn toascii(c: i32) -> i32 {
    c & 0x7f
}

/// Convert a lowercase letter to uppercase.
///
/// If not lowercase, returns `c` unchanged.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn isalpha_l(c: i32, _locale: LocaleT) -> i32 { isalpha(c) }

/// isdigit_l — locale-aware isdigit.
#[unsafe(no_mangle)]
pub extern "C" fn isdigit_l(c: i32, _locale: LocaleT) -> i32 { isdigit(c) }

/// isalnum_l — locale-aware isalnum.
#[unsafe(no_mangle)]
pub extern "C" fn isalnum_l(c: i32, _locale: LocaleT) -> i32 { isalnum(c) }

/// isspace_l — locale-aware isspace.
#[unsafe(no_mangle)]
pub extern "C" fn isspace_l(c: i32, _locale: LocaleT) -> i32 { isspace(c) }

/// isupper_l — locale-aware isupper.
#[unsafe(no_mangle)]
pub extern "C" fn isupper_l(c: i32, _locale: LocaleT) -> i32 { isupper(c) }

/// islower_l — locale-aware islower.
#[unsafe(no_mangle)]
pub extern "C" fn islower_l(c: i32, _locale: LocaleT) -> i32 { islower(c) }

/// isprint_l — locale-aware isprint.
#[unsafe(no_mangle)]
pub extern "C" fn isprint_l(c: i32, _locale: LocaleT) -> i32 { isprint(c) }

/// iscntrl_l — locale-aware iscntrl.
#[unsafe(no_mangle)]
pub extern "C" fn iscntrl_l(c: i32, _locale: LocaleT) -> i32 { iscntrl(c) }

/// ispunct_l — locale-aware ispunct.
#[unsafe(no_mangle)]
pub extern "C" fn ispunct_l(c: i32, _locale: LocaleT) -> i32 { ispunct(c) }

/// isxdigit_l — locale-aware isxdigit.
#[unsafe(no_mangle)]
pub extern "C" fn isxdigit_l(c: i32, _locale: LocaleT) -> i32 { isxdigit(c) }

/// isgraph_l — locale-aware isgraph.
#[unsafe(no_mangle)]
pub extern "C" fn isgraph_l(c: i32, _locale: LocaleT) -> i32 { isgraph(c) }

/// isblank_l — locale-aware isblank.
#[unsafe(no_mangle)]
pub extern "C" fn isblank_l(c: i32, _locale: LocaleT) -> i32 { isblank(c) }

/// toupper_l — locale-aware toupper.
#[unsafe(no_mangle)]
pub extern "C" fn toupper_l(c: i32, _locale: LocaleT) -> i32 { toupper(c) }

/// tolower_l — locale-aware tolower.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn __ctype_b_loc() -> *const *const u16 {
    // Compute the pointer each call — avoids the Sync issue with
    // static raw pointers while remaining branchless.
    static mut CACHED: *const u16 = core::ptr::null();
    // SAFETY: single-threaded init; pointer is stable (points into
    // a static array that never moves).
    unsafe {
        if CACHED.is_null() {
            CACHED = CTYPE_TABLE.as_ptr().add(128);
        }
        core::ptr::addr_of!(CACHED).cast()
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
#[unsafe(no_mangle)]
pub extern "C" fn __ctype_tolower_loc() -> *const *const i32 {
    static mut CACHED: *const i32 = core::ptr::null();
    unsafe {
        if CACHED.is_null() {
            CACHED = TOLOWER_TABLE.as_ptr().add(128);
        }
        core::ptr::addr_of!(CACHED).cast()
    }
}

/// glibc internal: return a pointer to the toupper conversion table.
#[unsafe(no_mangle)]
pub extern "C" fn __ctype_toupper_loc() -> *const *const i32 {
    static mut CACHED: *const i32 = core::ptr::null();
    unsafe {
        if CACHED.is_null() {
            CACHED = TOUPPER_TABLE.as_ptr().add(128);
        }
        core::ptr::addr_of!(CACHED).cast()
    }
}

/// glibc: return maximum number of bytes in a multibyte character.
///
/// For UTF-8 (our encoding), this is always 4.
#[unsafe(no_mangle)]
pub extern "C" fn __ctype_get_mb_cur_max() -> usize {
    4 // UTF-8: up to 4 bytes per character.
}
