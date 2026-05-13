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
    b"ISO88591",   // ISO-8859-1 treated as ASCII for our purposes
    b"LATIN1",
];

/// Open a conversion descriptor.
///
/// Supports UTF-8 ↔ UTF-8, ASCII ↔ UTF-8, and ASCII ↔ ASCII.
/// Returns `(IconvT)-1` and sets errno to `EINVAL` for unsupported
/// encoding pairs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iconv_open(tocode: *const u8, fromcode: *const u8) -> IconvT {
    let from_utf8 = matches_encoding(fromcode, UTF8_ALIASES);
    let from_ascii = matches_encoding(fromcode, ASCII_ALIASES);
    let to_utf8 = matches_encoding(tocode, UTF8_ALIASES);
    let to_ascii = matches_encoding(tocode, ASCII_ALIASES);

    // Identity conversion.
    if (from_utf8 && to_utf8) || (from_ascii && to_ascii) {
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
    fn test_open_latin1_alias() {
        let latin = cstr("LATIN1");
        let utf8 = cstr("UTF-8");
        let cd = iconv_open(utf8.as_ptr(), latin.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR, "LATIN1 should be recognized as ASCII alias");
    }

    #[test]
    fn test_open_iso88591_alias() {
        let iso = cstr("ISO-8859-1");
        let utf8 = cstr("UTF-8");
        let cd = iconv_open(utf8.as_ptr(), iso.as_ptr());
        assert_ne!(cd, ICONV_OPEN_ERR, "ISO-8859-1 should be recognized");
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
}
