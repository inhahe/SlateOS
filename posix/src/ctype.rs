//! C character classification and conversion functions.
//!
//! Implements the `<ctype.h>` interface: `isalpha`, `isdigit`, `isalnum`,
//! `isspace`, `isupper`, `islower`, `isprint`, `iscntrl`, `ispunct`,
//! `isxdigit`, `isgraph`, `isblank`, `isascii`, `toascii`, `toupper`,
//! `tolower`.
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
