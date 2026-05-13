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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn iconv_close(_cd: IconvT) -> i32 {
    0
}
