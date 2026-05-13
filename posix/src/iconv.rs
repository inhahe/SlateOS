//! POSIX character set conversion (`<iconv.h>`).
//!
//! Provides `iconv_open`, `iconv`, `iconv_close` for character encoding
//! conversion.  Our OS uses UTF-8 internally, so only UTF-8 ↔ ASCII
//! (and identity) conversions are supported.  Requesting any other
//! encoding pair fails with `EINVAL`.
//!
//! Many programs (shells, editors, curses) call `iconv_open("UTF-8",
//! "UTF-8")` at startup to test locale support; this succeeds.

use crate::errno;

/// Opaque conversion descriptor.
///
/// We encode the conversion type in a small integer cast to a pointer.
/// - 1: identity (same encoding, e.g. UTF-8 → UTF-8)
/// - 2: ASCII → UTF-8 (passthrough for ASCII subset)
/// - 3: UTF-8 → ASCII (lossy — non-ASCII bytes replaced with '?')
/// - 4: Latin-1 → UTF-8 (0x00-0x7F passthrough, 0x80-0xFF → 2-byte UTF-8)
/// - 5: UTF-8 → Latin-1 (code points > U+00FF set EILSEQ)
/// - 6: Latin-1 → ASCII (lossy — 0x80-0xFF replaced with '?', one byte at a time)
pub type IconvT = isize;

/// Error return from `iconv_open`.
pub const ICONV_OPEN_ERR: IconvT = -1;

// ---------------------------------------------------------------------------
// Encoding identifiers (case-insensitive matching)
// ---------------------------------------------------------------------------

/// Check if a C string matches an encoding name (case-insensitive).
///
/// Normalises the input by stripping hyphens/underscores and
/// upper-casing, then compares against each alias.
///
/// Returns `true` if `s` matches any of the aliases.
fn matches_encoding(s: *const u8, aliases: &[&[u8]]) -> bool {
    if s.is_null() {
        return false;
    }

    // Read the C string, stripping hyphens/underscores and uppercasing.
    let mut normalized = [0u8; 32];
    let mut norm_len = 0usize;
    let mut i = 0usize;
    loop {
        // SAFETY: caller guarantees s is a valid C string; we stop at NUL.
        let byte = unsafe { *s.add(i) };
        if byte == 0 {
            break;
        }
        i = i.wrapping_add(1);
        // Skip hyphens and underscores for fuzzy matching.
        if byte == b'-' || byte == b'_' {
            continue;
        }
        if norm_len >= normalized.len() {
            return false; // Too long to be a known encoding name.
        }
        if let Some(slot) = normalized.get_mut(norm_len) {
            *slot = byte.to_ascii_uppercase();
        }
        norm_len = norm_len.wrapping_add(1);
    }

    for alias in aliases {
        if alias.len() == norm_len {
            let mut is_match = true;
            for (j, &expected) in alias.iter().enumerate() {
                let actual = normalized.get(j).copied().unwrap_or(0);
                if actual != expected.to_ascii_uppercase() {
                    is_match = false;
                    break;
                }
            }
            if is_match {
                return true;
            }
        }
    }
    false
}

/// UTF-8 encoding aliases (normalized to uppercase, no hyphens/underscores).
const UTF8_ALIASES: &[&[u8]] = &[
    b"UTF8",
];

/// ASCII encoding aliases.
const ASCII_ALIASES: &[&[u8]] = &[
    b"ASCII",
    b"USASCII",
    b"US",
    b"ANSI",
];

/// Latin-1 (ISO-8859-1) encoding aliases.
///
/// Latin-1 is a superset of ASCII: 0x00-0x7F are identical to ASCII,
/// but 0x80-0xFF map to Unicode code points U+0080-U+00FF and require
/// 2-byte UTF-8 encoding (0xC2-0xC3 prefix).  This must NOT be treated
/// as a simple ASCII alias.
const LATIN1_ALIASES: &[&[u8]] = &[
    b"ISO88591",
    b"LATIN1",
    b"L1",
];

/// Open a conversion descriptor.
///
/// Supports UTF-8, ASCII, and Latin-1 (ISO-8859-1) in any combination.
/// Returns `(IconvT)-1` and sets errno to `EINVAL` for unsupported
/// encoding pairs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iconv_open(tocode: *const u8, fromcode: *const u8) -> IconvT {
    let from_utf8 = matches_encoding(fromcode, UTF8_ALIASES);
    let from_ascii = matches_encoding(fromcode, ASCII_ALIASES);
    let from_latin1 = matches_encoding(fromcode, LATIN1_ALIASES);
    let to_utf8 = matches_encoding(tocode, UTF8_ALIASES);
    let to_ascii = matches_encoding(tocode, ASCII_ALIASES);
    let to_latin1 = matches_encoding(tocode, LATIN1_ALIASES);

    // Identity conversions.
    if (from_utf8 && to_utf8) || (from_ascii && to_ascii) || (from_latin1 && to_latin1) {
        return 1; // Identity descriptor.
    }

    // ASCII → UTF-8 (passthrough — ASCII is valid UTF-8).
    if from_ascii && to_utf8 {
        return 2;
    }

    // UTF-8 → ASCII (lossy).
    if from_utf8 && to_ascii {
        return 3;
    }

    // Latin-1 → UTF-8 (0x80-0xFF expand to 2-byte UTF-8 sequences).
    if from_latin1 && to_utf8 {
        return 4;
    }

    // UTF-8 → Latin-1 (code points > U+00FF fail with EILSEQ).
    if from_utf8 && to_latin1 {
        return 5;
    }

    // ASCII → Latin-1 (passthrough — ASCII is a subset of Latin-1).
    if from_ascii && to_latin1 {
        return 1; // Identity (ASCII bytes are valid Latin-1).
    }

    // Latin-1 → ASCII (lossy — non-ASCII bytes replaced with '?').
    if from_latin1 && to_ascii {
        return 6;
    }

    // Unsupported encoding pair.
    errno::set_errno(errno::EINVAL);
    ICONV_OPEN_ERR
}

/// Perform character set conversion.
///
/// Converts bytes from `*inbuf` to `*outbuf`, updating all four
/// pointer/size pairs as it progresses.
///
/// Returns the number of irreversible conversions (replacements),
/// or `(size_t)-1` on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn iconv(
    cd: IconvT,
    inbuf: *mut *const u8,
    inbytesleft: *mut usize,
    outbuf: *mut *mut u8,
    outbytesleft: *mut usize,
) -> usize {
    // NULL inbuf = reset conversion state (no state for our encodings).
    if inbuf.is_null() || unsafe { (*inbuf).is_null() } {
        return 0;
    }

    if outbuf.is_null() || unsafe { (*outbuf).is_null() } || inbytesleft.is_null() || outbytesleft.is_null() {
        errno::set_errno(errno::EINVAL);
        return usize::MAX;
    }

    let mut replacements: usize = 0;

    // SAFETY: all pointers verified non-null.
    let in_ptr = unsafe { &mut *inbuf };
    let in_left = unsafe { &mut *inbytesleft };
    let out_ptr = unsafe { &mut *outbuf };
    let out_left = unsafe { &mut *outbytesleft };

    match cd {
        1 | 2 => {
            // Identity or ASCII → UTF-8: copy bytes directly.
            while *in_left > 0 && *out_left > 0 {
                // SAFETY: in_left > 0 means *in_ptr is valid; out_left > 0 means *out_ptr is valid.
                let byte = unsafe { **in_ptr };
                unsafe { **out_ptr = byte; }
                *in_ptr = unsafe { (*in_ptr).add(1) };
                *out_ptr = unsafe { (*out_ptr).add(1) };
                *in_left = in_left.wrapping_sub(1);
                *out_left = out_left.wrapping_sub(1);
            }
        }
        3 => {
            // UTF-8 → ASCII: non-ASCII bytes become '?'.
            while *in_left > 0 && *out_left > 0 {
                let byte = unsafe { **in_ptr };
                if byte > 127 {
                    // Non-ASCII: skip the full UTF-8 sequence.
                    let seq_len = if byte & 0xE0 == 0xC0 {
                        2usize
                    } else if byte & 0xF0 == 0xE0 {
                        3usize
                    } else if byte & 0xF8 == 0xF0 {
                        4usize
                    } else {
                        1usize // Invalid UTF-8 lead byte — skip 1.
                    };
                    // Emit replacement character.
                    unsafe { **out_ptr = b'?'; }
                    *out_ptr = unsafe { (*out_ptr).add(1) };
                    *out_left = out_left.wrapping_sub(1);
                    replacements = replacements.wrapping_add(1);
                    // Consume the full sequence from input.
                    let skip = seq_len.min(*in_left);
                    *in_ptr = unsafe { (*in_ptr).add(skip) };
                    *in_left = in_left.wrapping_sub(skip);
                } else {
                    // ASCII byte — copy directly.
                    unsafe { **out_ptr = byte; }
                    *in_ptr = unsafe { (*in_ptr).add(1) };
                    *out_ptr = unsafe { (*out_ptr).add(1) };
                    *in_left = in_left.wrapping_sub(1);
                    *out_left = out_left.wrapping_sub(1);
                }
            }
        }
        4 => {
            // Latin-1 → UTF-8: 0x00-0x7F copy as-is, 0x80-0xFF
            // expand to 2-byte UTF-8 sequences.
            //
            // Latin-1 byte 0xAB (U+00AB) → UTF-8: 0xC2 0xAB
            // Latin-1 byte 0xFF (U+00FF) → UTF-8: 0xC3 0xBF
            //
            // Formula: for code point cp in 0x80..=0xFF:
            //   byte1 = 0xC0 | (cp >> 6)   = 0xC2 or 0xC3
            //   byte2 = 0x80 | (cp & 0x3F)
            while *in_left > 0 {
                let byte = unsafe { **in_ptr };
                if byte <= 0x7F {
                    // ASCII: 1 byte output.
                    if *out_left == 0 {
                        errno::set_errno(errno::E2BIG);
                        return usize::MAX;
                    }
                    unsafe { **out_ptr = byte; }
                    *out_ptr = unsafe { (*out_ptr).add(1) };
                    *out_left = out_left.wrapping_sub(1);
                } else {
                    // Latin-1 high byte: 2 bytes output.
                    if *out_left < 2 {
                        errno::set_errno(errno::E2BIG);
                        return usize::MAX;
                    }
                    let cp = byte as u32;
                    unsafe {
                        **out_ptr = (0xC0 | (cp >> 6)) as u8;
                        *(*out_ptr).add(1) = (0x80 | (cp & 0x3F)) as u8;
                    }
                    *out_ptr = unsafe { (*out_ptr).add(2) };
                    *out_left = out_left.wrapping_sub(2);
                }
                *in_ptr = unsafe { (*in_ptr).add(1) };
                *in_left = in_left.wrapping_sub(1);
            }
        }
        5 => {
            // UTF-8 → Latin-1: code points U+0000-U+00FF map to
            // single Latin-1 bytes.  Code points > U+00FF fail with
            // EILSEQ (not representable in Latin-1).
            while *in_left > 0 && *out_left > 0 {
                let byte = unsafe { **in_ptr };
                if byte <= 0x7F {
                    // ASCII — copy directly (valid in Latin-1).
                    unsafe { **out_ptr = byte; }
                    *in_ptr = unsafe { (*in_ptr).add(1) };
                    *out_ptr = unsafe { (*out_ptr).add(1) };
                    *in_left = in_left.wrapping_sub(1);
                    *out_left = out_left.wrapping_sub(1);
                } else if byte & 0xE0 == 0xC0 {
                    // 2-byte UTF-8 sequence.
                    if *in_left < 2 {
                        // Incomplete sequence.
                        errno::set_errno(errno::EINVAL);
                        return usize::MAX;
                    }
                    let b2 = unsafe { *(*in_ptr).add(1) };
                    // Validate continuation byte.
                    if b2 & 0xC0 != 0x80 {
                        errno::set_errno(errno::EILSEQ);
                        return usize::MAX;
                    }
                    let cp = (((byte & 0x1F) as u32) << 6) | ((b2 & 0x3F) as u32);
                    if cp < 0x80 {
                        // Overlong encoding (e.g. 0xC0 0x80 for U+0000):
                        // must reject per Unicode security guidelines.
                        errno::set_errno(errno::EILSEQ);
                        return usize::MAX;
                    }
                    if cp > 0xFF {
                        // Code point not representable in Latin-1.
                        errno::set_errno(errno::EILSEQ);
                        return usize::MAX;
                    }
                    unsafe { **out_ptr = cp as u8; }
                    *in_ptr = unsafe { (*in_ptr).add(2) };
                    *out_ptr = unsafe { (*out_ptr).add(1) };
                    *in_left = in_left.wrapping_sub(2);
                    *out_left = out_left.wrapping_sub(1);
                } else {
                    // 3-byte or 4-byte sequence: code point > U+00FF,
                    // not representable in Latin-1.
                    errno::set_errno(errno::EILSEQ);
                    return usize::MAX;
                }
            }
        }
        6 => {
            // Latin-1 → ASCII (lossy): non-ASCII bytes replaced with
            // '?', one byte at a time.  Unlike descriptor 3 (UTF-8→ASCII)
            // which skips multi-byte sequences, Latin-1 is a single-byte
            // encoding so each byte > 127 is an independent character.
            while *in_left > 0 && *out_left > 0 {
                let byte = unsafe { **in_ptr };
                if byte > 127 {
                    unsafe { **out_ptr = b'?'; }
                    replacements = replacements.wrapping_add(1);
                } else {
                    unsafe { **out_ptr = byte; }
                }
                *in_ptr = unsafe { (*in_ptr).add(1) };
                *out_ptr = unsafe { (*out_ptr).add(1) };
                *in_left = in_left.wrapping_sub(1);
                *out_left = out_left.wrapping_sub(1);
            }
        }
        _ => {
            errno::set_errno(errno::EBADF);
            return usize::MAX;
        }
    }

    // If input remains but output is full, set E2BIG.
    if *in_left > 0 {
        errno::set_errno(errno::E2BIG);
        return usize::MAX;
    }

    replacements
}

/// Close a conversion descriptor.
///
/// No resources to free — always succeeds.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iconv_close(_cd: IconvT) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    /// Null-terminated C string helper.
    fn cstr(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// Run an iconv conversion on `input` using descriptor `cd`.
    /// Returns (output_bytes, replacements) on success, or None on error.
    fn convert(cd: IconvT, input: &[u8]) -> core::option::Option<(Vec<u8>, usize)> {
        let mut inbuf = input.as_ptr();
        let mut inleft = input.len();
        let mut outbuf_storage = vec![0u8; input.len().wrapping_mul(4).max(64)];
        let mut outptr = outbuf_storage.as_mut_ptr();
        let mut outleft = outbuf_storage.len();

        let ret = unsafe {
            iconv(
                cd,
                &mut inbuf as *mut *const u8,
                &mut inleft,
                &mut outptr,
                &mut outleft,
            )
        };

        if ret == usize::MAX {
            return None;
        }

        let written = outbuf_storage.len().wrapping_sub(outleft);
        outbuf_storage.truncate(written);
        Some((outbuf_storage, ret))
    }

    // -----------------------------------------------------------------------
    // iconv_open — encoding name matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_open_utf8_to_utf8() {
        let from = cstr("UTF-8");
        let to = cstr("UTF-8");
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR);
        assert_eq!(cd, 1, "UTF-8 → UTF-8 should be identity (1)");
    }

    #[test]
    fn test_open_ascii_to_utf8() {
        let from = cstr("ASCII");
        let to = cstr("UTF-8");
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR);
        assert_eq!(cd, 2, "ASCII → UTF-8 should be descriptor 2");
    }

    #[test]
    fn test_open_utf8_to_ascii() {
        let from = cstr("UTF-8");
        let to = cstr("ASCII");
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR);
        assert_eq!(cd, 3, "UTF-8 → ASCII should be descriptor 3");
    }

    #[test]
    fn test_open_ascii_to_ascii() {
        let from = cstr("ASCII");
        let to = cstr("ASCII");
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR);
        assert_eq!(cd, 1, "ASCII → ASCII should be identity (1)");
    }

    #[test]
    fn test_open_case_insensitive() {
        // "utf-8", "Utf-8", "UTF-8" should all match.
        let lower = cstr("utf-8");
        let mixed = cstr("Utf-8");
        let upper = cstr("UTF-8");

        let cd1 = iconv_open(lower.as_ptr(), lower.as_ptr());
        let cd2 = iconv_open(mixed.as_ptr(), mixed.as_ptr());
        let cd3 = iconv_open(upper.as_ptr(), upper.as_ptr());
        assert_ne!(cd1, ICONV_OPEN_ERR);
        assert_ne!(cd2, ICONV_OPEN_ERR);
        assert_ne!(cd3, ICONV_OPEN_ERR);
    }

    #[test]
    fn test_open_strips_hyphens_and_underscores() {
        // "UTF8", "UTF_8", "UTF-8" should all work.
        let no_sep = cstr("UTF8");
        let underscore = cstr("UTF_8");
        let hyphen = cstr("UTF-8");

        let cd1 = iconv_open(no_sep.as_ptr(), no_sep.as_ptr());
        let cd2 = iconv_open(underscore.as_ptr(), underscore.as_ptr());
        let cd3 = iconv_open(hyphen.as_ptr(), hyphen.as_ptr());
        assert_ne!(cd1, ICONV_OPEN_ERR);
        assert_ne!(cd2, ICONV_OPEN_ERR);
        assert_ne!(cd3, ICONV_OPEN_ERR);
    }

    #[test]
    fn test_open_us_ascii_alias() {
        let us = cstr("US-ASCII");
        let utf8 = cstr("UTF-8");
        let cd = iconv_open(utf8.as_ptr(), us.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR, "US-ASCII should be recognized");
    }

    #[test]
    fn test_open_latin1_to_utf8() {
        let latin = cstr("LATIN1");
        let utf8 = cstr("UTF-8");
        let cd = iconv_open(utf8.as_ptr(), latin.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR, "LATIN1 should be recognized");
        assert_eq!(cd, 4, "Latin-1 → UTF-8 should be descriptor 4");
    }

    #[test]
    fn test_open_utf8_to_latin1() {
        let utf8 = cstr("UTF-8");
        let latin = cstr("LATIN1");
        let cd = iconv_open(latin.as_ptr(), utf8.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR, "UTF-8 → LATIN1 should be supported");
        assert_eq!(cd, 5, "UTF-8 → Latin-1 should be descriptor 5");
    }

    #[test]
    fn test_open_iso88591_to_utf8() {
        let iso = cstr("ISO-8859-1");
        let utf8 = cstr("UTF-8");
        let cd = iconv_open(utf8.as_ptr(), iso.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR, "ISO-8859-1 should be recognized");
        assert_eq!(cd, 4);
    }

    #[test]
    fn test_open_latin1_to_latin1_identity() {
        let l1 = cstr("LATIN1");
        let l2 = cstr("ISO-8859-1");
        let cd = iconv_open(l1.as_ptr(), l2.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR);
        assert_eq!(cd, 1, "Latin-1 → Latin-1 should be identity");
    }

    #[test]
    fn test_open_ascii_to_latin1() {
        let ascii = cstr("ASCII");
        let latin = cstr("LATIN1");
        let cd = iconv_open(latin.as_ptr(), ascii.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR);
        assert_eq!(cd, 1, "ASCII → Latin-1 should be identity (ASCII is subset)");
    }

    // -----------------------------------------------------------------------
    // Invalid / unsupported encodings
    // -----------------------------------------------------------------------

    #[test]
    fn test_open_unsupported_encoding() {
        let from = cstr("UTF-16");
        let to = cstr("UTF-8");
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        assert_eq!(cd, ICONV_OPEN_ERR, "UTF-16 is not supported");
    }

    #[test]
    fn test_open_unknown_encoding() {
        let from = cstr("EBCDIC");
        let to = cstr("UTF-8");
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        assert_eq!(cd, ICONV_OPEN_ERR);
    }

    #[test]
    fn test_open_null_from() {
        let to = cstr("UTF-8");
        let cd = iconv_open(to.as_ptr(), core::ptr::null());
        assert_eq!(cd, ICONV_OPEN_ERR);
    }

    #[test]
    fn test_open_null_to() {
        let from = cstr("UTF-8");
        let cd = iconv_open(core::ptr::null(), from.as_ptr());
        assert_eq!(cd, ICONV_OPEN_ERR);
    }

    // -----------------------------------------------------------------------
    // iconv_close
    // -----------------------------------------------------------------------

    #[test]
    fn test_close_identity() {
        let from = cstr("UTF-8");
        let to = cstr("UTF-8");
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR);
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn test_close_invalid_descriptor() {
        // iconv_close always succeeds (no resources to free).
        assert_eq!(iconv_close(99), 0);
        assert_eq!(iconv_close(-1), 0);
    }

    // -----------------------------------------------------------------------
    // Identity conversion (UTF-8 → UTF-8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_identity_ascii_bytes() {
        let input = b"Hello, world!";
        let (output, replacements) = convert(1, input).expect("identity conversion should succeed");
        assert_eq!(&output, input);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_identity_empty_input() {
        let (output, replacements) = convert(1, b"").expect("empty input should succeed");
        assert!(output.is_empty());
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_identity_multibyte_utf8() {
        // UTF-8 encoded "cafe\u{0301}" (e + combining accent) — multi-byte.
        let input = "caf\u{00E9}".as_bytes(); // "cafe" with e-acute (2-byte UTF-8)
        let (output, replacements) = convert(1, input).expect("identity should handle multi-byte");
        assert_eq!(&output, input);
        assert_eq!(replacements, 0);
    }

    // -----------------------------------------------------------------------
    // ASCII → UTF-8 (passthrough, descriptor 2)
    // -----------------------------------------------------------------------

    #[test]
    fn test_ascii_to_utf8_simple() {
        let input = b"Hello";
        let (output, replacements) = convert(2, input).expect("ASCII → UTF-8 should succeed");
        assert_eq!(&output, input, "ASCII bytes are valid UTF-8 unchanged");
        assert_eq!(replacements, 0);
    }

    // -----------------------------------------------------------------------
    // UTF-8 → ASCII (lossy, descriptor 3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf8_to_ascii_simple_ascii() {
        let input = b"Hello";
        let (output, replacements) = convert(3, input).expect("pure ASCII should pass through");
        assert_eq!(&output, input);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_utf8_to_ascii_replaces_non_ascii() {
        // "\xc3\xa9" is UTF-8 for 'e-acute' (U+00E9, 2-byte sequence).
        let input = b"caf\xc3\xa9";
        let (output, replacements) = convert(3, input).expect("conversion should succeed");
        assert_eq!(&output, b"caf?", "non-ASCII replaced with '?'");
        assert_eq!(replacements, 1);
    }

    #[test]
    fn test_utf8_to_ascii_3byte_sequence() {
        // Euro sign U+20AC: 0xE2 0x82 0xAC (3-byte UTF-8), followed by "100".
        let input = b"price: \xe2\x82\xac100";
        let (output, replacements) = convert(3, input).expect("3-byte sequence conversion");
        assert_eq!(&output, b"price: ?100");
        assert_eq!(replacements, 1);
    }

    #[test]
    fn test_utf8_to_ascii_4byte_sequence() {
        // U+1F600 (grinning face): 0xF0 0x9F 0x98 0x80 (4-byte UTF-8).
        let input = b"hi \xf0\x9f\x98\x80 there";
        let (output, replacements) = convert(3, input).expect("4-byte sequence conversion");
        assert_eq!(&output, b"hi ? there");
        assert_eq!(replacements, 1);
    }

    #[test]
    fn test_utf8_to_ascii_multiple_replacements() {
        // Two non-ASCII characters.
        let input = b"\xc3\xa9\xc3\xa8"; // e-acute + e-grave
        let (output, replacements) = convert(3, input).expect("multiple replacements");
        assert_eq!(&output, b"??");
        assert_eq!(replacements, 2);
    }

    // -----------------------------------------------------------------------
    // Buffer overflow / E2BIG
    // -----------------------------------------------------------------------

    #[test]
    fn test_output_buffer_too_small() {
        let input = b"Hello, World!";
        let mut inbuf = input.as_ptr();
        let mut inleft = input.len();
        // Tiny output buffer.
        let mut outbuf_storage = [0u8; 4];
        let mut outptr = outbuf_storage.as_mut_ptr();
        let mut outleft: usize = 4;

        let ret = unsafe {
            iconv(
                1, // identity
                &mut inbuf as *mut *const u8,
                &mut inleft,
                &mut outptr,
                &mut outleft,
            )
        };

        assert_eq!(ret, usize::MAX, "should fail when output buffer is full");
        assert_eq!(errno::get_errno(), errno::E2BIG);
        // The first 4 bytes should have been written.
        assert_eq!(&outbuf_storage, b"Hell");
        // inleft should reflect remaining input.
        assert_eq!(inleft, input.len().wrapping_sub(4));
    }

    // -----------------------------------------------------------------------
    // Invalid descriptor
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_descriptor() {
        let input = b"test";
        let mut inbuf = input.as_ptr();
        let mut inleft = input.len();
        let mut outbuf_storage = [0u8; 32];
        let mut outptr = outbuf_storage.as_mut_ptr();
        let mut outleft: usize = 32;

        let ret = unsafe {
            iconv(
                99, // invalid descriptor
                &mut inbuf as *mut *const u8,
                &mut inleft,
                &mut outptr,
                &mut outleft,
            )
        };

        assert_eq!(ret, usize::MAX);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -----------------------------------------------------------------------
    // Null inbuf (reset conversion state)
    // -----------------------------------------------------------------------

    #[test]
    fn test_null_inbuf_resets() {
        // Calling iconv with NULL inbuf should return 0 (state reset).
        let mut outleft: usize = 32;
        let mut outbuf_storage = [0u8; 32];
        let mut outptr = outbuf_storage.as_mut_ptr();

        let ret = unsafe {
            iconv(
                1,
                core::ptr::null_mut(), // null inbuf
                core::ptr::null_mut(),
                &mut outptr,
                &mut outleft,
            )
        };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_null_outbuf_is_error() {
        let input = b"test";
        let mut inbuf = input.as_ptr();
        let mut inleft = input.len();

        let ret = unsafe {
            iconv(
                1,
                &mut inbuf as *mut *const u8,
                &mut inleft,
                core::ptr::null_mut(), // null outbuf
                core::ptr::null_mut(),
            )
        };
        assert_eq!(ret, usize::MAX);
    }

    // -----------------------------------------------------------------------
    // matches_encoding internal tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_matches_encoding_null() {
        assert!(!matches_encoding(core::ptr::null(), UTF8_ALIASES));
    }

    #[test]
    fn test_matches_encoding_utf8_variants() {
        let v1 = cstr("UTF-8");
        let v2 = cstr("utf-8");
        let v3 = cstr("utf8");
        let v4 = cstr("UTF_8");
        assert!(matches_encoding(v1.as_ptr(), UTF8_ALIASES));
        assert!(matches_encoding(v2.as_ptr(), UTF8_ALIASES));
        assert!(matches_encoding(v3.as_ptr(), UTF8_ALIASES));
        assert!(matches_encoding(v4.as_ptr(), UTF8_ALIASES));
    }

    #[test]
    fn test_matches_encoding_ascii_variants() {
        let v1 = cstr("ASCII");
        let v2 = cstr("ascii");
        let v3 = cstr("US-ASCII");
        let v4 = cstr("us-ascii");
        let v5 = cstr("US");
        assert!(matches_encoding(v1.as_ptr(), ASCII_ALIASES));
        assert!(matches_encoding(v2.as_ptr(), ASCII_ALIASES));
        assert!(matches_encoding(v3.as_ptr(), ASCII_ALIASES));
        assert!(matches_encoding(v4.as_ptr(), ASCII_ALIASES));
        assert!(matches_encoding(v5.as_ptr(), ASCII_ALIASES));
    }

    #[test]
    fn test_matches_encoding_no_match() {
        let v = cstr("EBCDIC");
        assert!(!matches_encoding(v.as_ptr(), UTF8_ALIASES));
        assert!(!matches_encoding(v.as_ptr(), ASCII_ALIASES));
    }

    #[test]
    fn test_matches_encoding_latin1_variants() {
        let v1 = cstr("LATIN1");
        let v2 = cstr("latin1");
        let v3 = cstr("ISO-8859-1");
        let v4 = cstr("iso-8859-1");
        let v5 = cstr("L1");
        assert!(matches_encoding(v1.as_ptr(), LATIN1_ALIASES));
        assert!(matches_encoding(v2.as_ptr(), LATIN1_ALIASES));
        assert!(matches_encoding(v3.as_ptr(), LATIN1_ALIASES));
        assert!(matches_encoding(v4.as_ptr(), LATIN1_ALIASES));
        assert!(matches_encoding(v5.as_ptr(), LATIN1_ALIASES));
    }

    #[test]
    fn test_open_latin1_to_ascii() {
        let latin = cstr("LATIN1");
        let ascii = cstr("ASCII");
        let cd = iconv_open(ascii.as_ptr(), latin.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR, "Latin-1 → ASCII should be supported");
        assert_eq!(cd, 6, "Latin-1 → ASCII should be descriptor 6");
    }

    #[test]
    fn test_latin1_not_ascii() {
        // Latin-1 must NOT match ASCII aliases.
        let v = cstr("LATIN1");
        assert!(!matches_encoding(v.as_ptr(), ASCII_ALIASES));
        let v2 = cstr("ISO-8859-1");
        assert!(!matches_encoding(v2.as_ptr(), ASCII_ALIASES));
    }

    // -----------------------------------------------------------------------
    // Latin-1 → UTF-8 (descriptor 4)
    // -----------------------------------------------------------------------

    #[test]
    fn test_latin1_to_utf8_ascii_passthrough() {
        // ASCII subset should pass through unchanged.
        let input = b"Hello";
        let (output, replacements) = convert(4, input).expect("ASCII Latin-1 → UTF-8");
        assert_eq!(&output, b"Hello");
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_latin1_to_utf8_0x80() {
        // Latin-1 0x80 = U+0080 → UTF-8 0xC2 0x80
        let input = &[0x80u8];
        let (output, replacements) = convert(4, input).expect("0x80 Latin-1 → UTF-8");
        assert_eq!(&output, &[0xC2, 0x80]);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_latin1_to_utf8_0xff() {
        // Latin-1 0xFF = U+00FF (ÿ) → UTF-8 0xC3 0xBF
        let input = &[0xFFu8];
        let (output, replacements) = convert(4, input).expect("0xFF Latin-1 → UTF-8");
        assert_eq!(&output, &[0xC3, 0xBF]);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_latin1_to_utf8_e_acute() {
        // Latin-1 0xE9 = U+00E9 (é) → UTF-8 0xC3 0xA9
        let input = &[0xE9u8];
        let (output, replacements) = convert(4, input).expect("0xE9 Latin-1 → UTF-8");
        assert_eq!(&output, &[0xC3, 0xA9]);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_latin1_to_utf8_mixed() {
        // "café" in Latin-1: 0x63 0x61 0x66 0xE9
        let input = &[0x63, 0x61, 0x66, 0xE9];
        let (output, replacements) = convert(4, input).expect("café Latin-1 → UTF-8");
        // Expected: "caf" + UTF-8(é) = 0x63 0x61 0x66 0xC3 0xA9
        assert_eq!(&output, &[0x63, 0x61, 0x66, 0xC3, 0xA9]);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_latin1_to_utf8_all_high_bytes() {
        // Convert all Latin-1 bytes 0x80-0xBF (first batch: 0xC2 prefix)
        let input = &[0x80u8, 0xBF];
        let (output, _) = convert(4, input).expect("0x80-0xBF Latin-1 → UTF-8");
        assert_eq!(&output, &[0xC2, 0x80, 0xC2, 0xBF]);
    }

    #[test]
    fn test_latin1_to_utf8_second_batch() {
        // Convert Latin-1 bytes 0xC0-0xFF (second batch: 0xC3 prefix)
        let input = &[0xC0u8, 0xFF];
        let (output, _) = convert(4, input).expect("0xC0-0xFF Latin-1 → UTF-8");
        assert_eq!(&output, &[0xC3, 0x80, 0xC3, 0xBF]);
    }

    #[test]
    fn test_latin1_to_utf8_empty() {
        let (output, replacements) = convert(4, b"").expect("empty Latin-1 → UTF-8");
        assert!(output.is_empty());
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_latin1_to_utf8_output_too_small() {
        // 0xE9 needs 2 output bytes; provide only 1.
        let input = &[0xE9u8];
        let mut inbuf = input.as_ptr();
        let mut inleft = 1usize;
        let mut outbuf_storage = [0u8; 1]; // Only 1 byte
        let mut outptr = outbuf_storage.as_mut_ptr();
        let mut outleft = 1usize;

        let ret = unsafe {
            iconv(4, &mut inbuf as *mut *const u8, &mut inleft, &mut outptr, &mut outleft)
        };
        assert_eq!(ret, usize::MAX);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    // -----------------------------------------------------------------------
    // UTF-8 → Latin-1 (descriptor 5)
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf8_to_latin1_ascii() {
        let input = b"Hello";
        let (output, replacements) = convert(5, input).expect("ASCII UTF-8 → Latin-1");
        assert_eq!(&output, b"Hello");
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_utf8_to_latin1_e_acute() {
        // UTF-8 0xC3 0xA9 = U+00E9 (é) → Latin-1 0xE9
        let input = &[0xC3u8, 0xA9];
        let (output, replacements) = convert(5, input).expect("é UTF-8 → Latin-1");
        assert_eq!(&output, &[0xE9]);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_utf8_to_latin1_0xff() {
        // UTF-8 0xC3 0xBF = U+00FF (ÿ) → Latin-1 0xFF
        let input = &[0xC3u8, 0xBF];
        let (output, replacements) = convert(5, input).expect("ÿ UTF-8 → Latin-1");
        assert_eq!(&output, &[0xFF]);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_utf8_to_latin1_mixed() {
        // "café" in UTF-8: 0x63 0x61 0x66 0xC3 0xA9
        let input = &[0x63, 0x61, 0x66, 0xC3, 0xA9];
        let (output, replacements) = convert(5, input).expect("café UTF-8 → Latin-1");
        assert_eq!(&output, &[0x63, 0x61, 0x66, 0xE9]);
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_utf8_to_latin1_above_00ff_fails() {
        // Euro sign U+20AC: 0xE2 0x82 0xAC (3-byte UTF-8)
        // Not representable in Latin-1 → EILSEQ.
        let input = &[0xE2u8, 0x82, 0xAC];
        let result = convert(5, input);
        assert!(result.is_none(), "U+20AC should fail for Latin-1");
    }

    #[test]
    fn test_utf8_to_latin1_4byte_fails() {
        // U+1F600 (emoji): 0xF0 0x9F 0x98 0x80 (4-byte UTF-8)
        let input = &[0xF0u8, 0x9F, 0x98, 0x80];
        let result = convert(5, input);
        assert!(result.is_none(), "U+1F600 should fail for Latin-1");
    }

    #[test]
    fn test_utf8_to_latin1_2byte_above_ff_fails() {
        // U+0100 (Ā): 0xC4 0x80 — above U+00FF, not representable.
        let input = &[0xC4u8, 0x80];
        let result = convert(5, input);
        assert!(result.is_none(), "U+0100 should fail for Latin-1");
    }

    #[test]
    fn test_utf8_to_latin1_roundtrip() {
        // Convert Latin-1 → UTF-8 → Latin-1 for a range of bytes.
        for byte in 0x80u8..=0xFF {
            let input_l1 = &[byte];
            let (utf8, _) = convert(4, input_l1).expect("Latin-1 → UTF-8");
            let (back_l1, _) = convert(5, &utf8).expect("UTF-8 → Latin-1 roundtrip");
            assert_eq!(back_l1, &[byte], "roundtrip failed for byte 0x{byte:02X}");
        }
    }

    #[test]
    fn test_utf8_to_latin1_incomplete_sequence() {
        // Incomplete 2-byte sequence: 0xC3 without continuation.
        let input = &[0xC3u8];
        let result = convert(5, input);
        assert!(result.is_none(), "incomplete UTF-8 should fail");
    }

    // -----------------------------------------------------------------------
    // Latin-1 → ASCII (descriptor 6)
    // -----------------------------------------------------------------------

    #[test]
    fn test_latin1_to_ascii_pure_ascii() {
        let input = b"Hello";
        let (output, replacements) = convert(6, input).expect("ASCII Latin-1 → ASCII");
        assert_eq!(&output, b"Hello");
        assert_eq!(replacements, 0);
    }

    #[test]
    fn test_latin1_to_ascii_replaces_high_bytes() {
        // Latin-1 "café": 0x63 0x61 0x66 0xE9
        // Each byte > 127 is ONE character, replaced with ONE '?'.
        let input = &[0x63u8, 0x61, 0x66, 0xE9];
        let (output, replacements) = convert(6, input).expect("café Latin-1 → ASCII");
        assert_eq!(&output, b"caf?");
        assert_eq!(replacements, 1);
    }

    #[test]
    fn test_latin1_to_ascii_all_high_bytes() {
        // Three high bytes: each should produce exactly one '?'.
        let input = &[0xE9u8, 0xF1, 0xFC]; // é, ñ, ü
        let (output, replacements) = convert(6, input).expect("all-high Latin-1 → ASCII");
        assert_eq!(&output, b"???");
        assert_eq!(replacements, 3);
        // Critical: input and output must be the same length for Latin-1.
        // (UTF-8→ASCII would wrongly skip multi-byte sequences here.)
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn test_latin1_to_ascii_byte_0xe9_not_treated_as_utf8() {
        // Regression test: Latin-1 byte 0xE9 has the bit pattern 0xF0 == 0xE0
        // which the UTF-8→ASCII converter would interpret as a 3-byte sequence
        // leader, consuming 3 input bytes. Latin-1 → ASCII must consume only 1.
        let input = &[0xE9u8, 0x41, 0x42]; // é, A, B in Latin-1
        let (output, replacements) = convert(6, input).expect("0xE9 + AB");
        assert_eq!(&output, b"?AB");
        assert_eq!(replacements, 1);
    }

    // -----------------------------------------------------------------------
    // UTF-8 → Latin-1: overlong encoding rejection
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf8_to_latin1_overlong_c0_80_rejected() {
        // 0xC0 0x80 is an overlong encoding of U+0000 (NUL).
        // Must be rejected per Unicode security guidelines.
        let input = &[0xC0u8, 0x80];
        let result = convert(5, input);
        assert!(result.is_none(), "overlong 0xC0 0x80 should be rejected");
    }

    #[test]
    fn test_utf8_to_latin1_overlong_c1_bf_rejected() {
        // 0xC1 0xBF is an overlong encoding of U+007F (DEL).
        // Must be rejected — valid encoding is the 1-byte form 0x7F.
        let input = &[0xC1u8, 0xBF];
        let result = convert(5, input);
        assert!(result.is_none(), "overlong 0xC1 0xBF should be rejected");
    }

    #[test]
    fn test_utf8_to_latin1_overlong_c0_af_rejected() {
        // 0xC0 0xAF is an overlong encoding of U+002F ('/').
        let input = &[0xC0u8, 0xAF];
        let result = convert(5, input);
        assert!(result.is_none(), "overlong 0xC0 0xAF should be rejected");
    }

    #[test]
    fn test_utf8_to_latin1_minimal_2byte_c2_80_accepted() {
        // 0xC2 0x80 = U+0080 — the smallest valid 2-byte sequence.
        // Should be accepted (U+0080 is within Latin-1 range).
        let input = &[0xC2u8, 0x80];
        let (output, _) = convert(5, input).expect("0xC2 0x80 should be accepted");
        assert_eq!(&output, &[0x80u8]);
    }

    #[test]
    fn test_utf8_to_latin1_invalid_continuation_byte() {
        // 0xC3 followed by non-continuation byte (0x41 = 'A').
        let input = &[0xC3u8, 0x41];
        let result = convert(5, input);
        assert!(result.is_none(), "invalid continuation byte should be rejected");
    }
}
