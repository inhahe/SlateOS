//! The text form of a 16-byte secret the compositor hands out -- an activation
//! token, an exported window: 32 hexadecimal digits, so it can travel through
//! an environment variable or another program's request as text.
//!
//! One copy, for both: the two were the same twenty lines, and a rule about
//! reading a secret's text (no sign, no space, nothing but the digits) that is
//! written twice is written slightly differently once.

use std::fmt::Write as _;

/// Length of the text form: two hexadecimal digits a byte.
pub(crate) const TEXT_LEN: usize = 32;

/// `bytes` as 32 lowercase hexadecimal digits.
pub(crate) fn to_text(bytes: [u8; 16]) -> String {
    let mut text = String::with_capacity(TEXT_LEN);
    for byte in bytes {
        // Writing into a `String` cannot fail; `fmt::Write` only says it
        // might because other writers can.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Read exactly 32 hexadecimal digits, either case; anything else -- a sign,
/// a space, a stray newline -- is `None` rather than a secret read leniently.
pub(crate) fn from_text(text: &[u8]) -> Option<[u8; 16]> {
    if text.len() != TEXT_LEN || !text.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (byte, pair) in bytes.iter_mut().zip(text.chunks_exact(2)) {
        // Both checked above: two ASCII hex digits are UTF-8, and parse.
        // `from_str_radix` alone would also take a leading `+`.
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(bytes)
}

/// Whether two secrets are the same, comparing every byte whatever the first
/// differing one, so that how long a comparison takes says nothing about how
/// much of a guess was right.
pub(crate) fn same(a: &[u8; 16], b: &[u8; 16]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |differ, (x, y)| differ | (x ^ y))
        == 0
}
