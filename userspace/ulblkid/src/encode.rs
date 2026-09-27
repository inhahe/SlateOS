//! libblkid's string handling: `lib/encode.c`'s conversion of on-disk
//! labels to UTF-8, libblkid's `encode.c` (`blkid_encode_string`,
//! `blkid_safe_string`) and the white-space trimming of `strutils.h`.
//!
//! Every function works on bytes. A label on disk is whatever bytes the
//! filesystem stored, and what these produce is what upstream produces for
//! the same bytes -- invalid UTF-8 included where upstream lets it through.

/// `UL_ENCODE_UTF16BE`.
pub const ENCODE_UTF16BE: i32 = 0;
/// `UL_ENCODE_UTF16LE`.
pub const ENCODE_UTF16LE: i32 = 1;
/// `UL_ENCODE_LATIN1`.
pub const ENCODE_LATIN1: i32 = 2;

/// C's `isspace` in the C locale.
#[must_use]
pub fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `ul_encode_to_utf8(enc, dest, len, src, count)`: `src` decoded as `enc`
/// and written as UTF-8 into a buffer of `len` bytes, stopping at a NUL
/// character, at the end of `src`, or where the next character would not
/// fit with the terminator. Returns the bytes written, without the NUL.
///
/// Kept as upstream has it: a high surrogate not followed by a low one, and
/// a lone low surrogate, are encoded as three bytes each (not valid UTF-8);
/// an odd trailing byte of UTF-16 is dropped.
#[must_use]
pub fn encode_to_utf8(enc: i32, len: usize, src: &[u8]) -> Vec<u8> {
    let mut dest = Vec::new();
    let count = src.len();
    let at = |k: usize| u32::from(src.get(k).copied().unwrap_or(0));
    let mut i = 0usize;
    while i < count {
        let mut c: u32;
        if enc == ENCODE_UTF16LE {
            if i.saturating_add(2) > count {
                break;
            }
            c = (at(i.saturating_add(1)) << 8) | at(i);
            i = i.saturating_add(1);
        } else if enc == ENCODE_UTF16BE {
            if i.saturating_add(2) > count {
                break;
            }
            c = (at(i) << 8) | at(i.saturating_add(1));
            i = i.saturating_add(1);
        } else if enc == ENCODE_LATIN1 {
            c = at(i);
        } else {
            return Vec::new();
        }
        if (enc == ENCODE_UTF16LE || enc == ENCODE_UTF16BE)
            && (0xD800..=0xDBFF).contains(&c)
            && i.saturating_add(2) < count
        {
            let c2 = if enc == ENCODE_UTF16LE {
                (at(i.saturating_add(2)) << 8) | at(i.saturating_add(1))
            } else {
                (at(i.saturating_add(1)) << 8) | at(i.saturating_add(2))
            };
            if (0xDC00..=0xDFFF).contains(&c2) {
                // A surrogate pair: both halves are in range, so none of this
                // overflows.
                c = ((c.wrapping_sub(0xD800)) << 10)
                    .wrapping_add(c2.wrapping_sub(0xDC00))
                    .wrapping_add(0x10000);
                i = i.saturating_add(2);
            }
        }
        if c == 0 {
            break;
        }
        let j = dest.len();
        // The `as u8` casts keep the low bits, as C's `(uint8_t)` does.
        #[allow(clippy::cast_possible_truncation, reason = "C's (uint8_t) casts")]
        if c < 0x80 {
            if j.saturating_add(1) >= len {
                break;
            }
            dest.push(c as u8);
        } else if c < 0x800 {
            if j.saturating_add(2) >= len {
                break;
            }
            dest.push((0xc0 | (c >> 6)) as u8);
            dest.push((0x80 | (c & 0x3f)) as u8);
        } else if c < 0x10000 {
            if j.saturating_add(3) >= len {
                break;
            }
            dest.push((0xe0 | (c >> 12)) as u8);
            dest.push((0x80 | ((c >> 6) & 0x3f)) as u8);
            dest.push((0x80 | (c & 0x3f)) as u8);
        } else {
            if j.saturating_add(4) >= len {
                break;
            }
            dest.push((0xf0 | (c >> 18)) as u8);
            dest.push((0x80 | ((c >> 12) & 0x3f)) as u8);
            dest.push((0x80 | ((c >> 6) & 0x3f)) as u8);
            dest.push((0x80 | (c & 0x3f)) as u8);
        }
        i = i.saturating_add(1);
    }
    dest
}

/// `rtrim_whitespace(str)` of a C string: the string up to its first NUL,
/// trailing white space off.
#[must_use]
pub fn rtrim_whitespace(s: &[u8]) -> &[u8] {
    let s = crate::c_str(s);
    let end = s
        .iter()
        .rposition(|&b| !c_isspace(b))
        .map_or(0, |i| i.saturating_add(1));
    s.get(..end).unwrap_or_default()
}

/// `ltrim_whitespace(str)` of a C string: leading white space off.
#[must_use]
pub fn ltrim_whitespace(s: &[u8]) -> &[u8] {
    let s = crate::c_str(s);
    let start = s.iter().position(|&b| !c_isspace(b)).unwrap_or(s.len());
    s.get(start..).unwrap_or_default()
}

/// `__normalize_whitespace(src, sz, dst, len)`: runs of white space made one
/// space, none at either end, into a buffer of `len` bytes.
#[must_use]
pub fn normalize_whitespace(src: &[u8], len: usize) -> Vec<u8> {
    let mut dst = Vec::new();
    if src.is_empty() {
        return dst;
    }
    let mut nsp = 0usize;
    let mut intext = false;
    let mut i = 0usize;
    while i < src.len() && dst.len() < len.saturating_sub(1) {
        let c = src.get(i).copied().unwrap_or(0);
        if c_isspace(c) {
            nsp = nsp.saturating_add(1);
        } else {
            nsp = 0;
            intext = true;
        }
        if nsp > 1 || (nsp > 0 && !intext) {
            i = i.saturating_add(1);
        } else {
            dst.push(c);
            i = i.saturating_add(1);
        }
    }
    if nsp > 0 && !dst.is_empty() {
        dst.pop();
    }
    dst
}

/// `utf8_encoded_expected_len`.
fn expected_len(c: u8) -> usize {
    match c {
        0..0x80 => 1,
        _ if c & 0xe0 == 0xc0 => 2,
        _ if c & 0xf0 == 0xe0 => 3,
        _ if c & 0xf8 == 0xf0 => 4,
        _ if c & 0xfc == 0xf8 => 5,
        _ if c & 0xfe == 0xfc => 6,
        _ => 0,
    }
}

/// `utf8_encoded_valid_unichar(str)`: the length of the valid character at
/// the front of `s` -- 1 for ASCII -- or `None`.
#[must_use]
pub fn utf8_valid_len(s: &[u8]) -> Option<usize> {
    let &c = s.first()?;
    let len = expected_len(c);
    match len {
        0 => return None,
        1 => return Some(1),
        _ => {}
    }
    // Bytes past the end read as the terminator, which fails the check.
    let byte = |k: usize| s.get(k).copied().unwrap_or(0);
    if (0..len).any(|k| byte(k) & 0x80 != 0x80) {
        return None;
    }
    let lead_mask = match len {
        2 => 0x1f,
        3 => 0x0f,
        4 => 0x07,
        5 => 0x03,
        _ => 0x01,
    };
    let mut unichar = u32::from(c & lead_mask);
    for k in 1..len {
        let b = byte(k);
        if b & 0xc0 != 0x80 {
            return None;
        }
        unichar = (unichar << 6) | u32::from(b & 0x3f);
    }
    let encoded_len = match unichar {
        0..0x80 => 1,
        0x80..0x800 => 2,
        0x800..0x1_0000 => 3,
        0x1_0000..0x20_0000 => 4,
        0x20_0000..0x400_0000 => 5,
        _ => 6,
    };
    if encoded_len != len {
        return None;
    }
    let valid = unichar <= 0x10_ffff
        && unichar & 0xffff_f800 != 0xd800
        && !(unichar > 0xfdcf && unichar < 0xfdf0)
        && unichar & 0xffff != 0xffff;
    valid.then_some(len)
}

/// `is_whitelisted(c, NULL)`: `[0-9A-Za-z#+-.:=@_]`.
fn whitelisted(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"#+-.:=@_".contains(&c)
}

/// `blkid_encode_string(str, str_enc, len)`: valid UTF-8 characters as they
/// are, `[0-9A-Za-z#+-.:=@_]` as they are, every other byte -- a backslash
/// included -- as `\xHH`. `None` when the result would not fit a buffer of
/// `len` bytes with room to spare, as upstream fails.
#[must_use]
pub fn encode_string(s: &[u8], len: usize) -> Option<Vec<u8>> {
    if len == 0 {
        return None;
    }
    let s = crate::c_str(s);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let rest = s.get(i..).unwrap_or_default();
        let c = rest.first().copied().unwrap_or(0);
        match utf8_valid_len(rest) {
            Some(n) if n > 1 => {
                if len.saturating_sub(out.len()) < n {
                    return None;
                }
                out.extend_from_slice(rest.get(..n).unwrap_or_default());
                i = i.saturating_add(n);
            }
            _ => {
                if c == b'\\' || !whitelisted(c) {
                    if len.saturating_sub(out.len()) < 4 {
                        return None;
                    }
                    out.extend_from_slice(format!("\\x{c:02x}").as_bytes());
                } else {
                    if len.saturating_sub(out.len()) < 1 {
                        return None;
                    }
                    out.push(c);
                }
                i = i.saturating_add(1);
            }
        }
        if out.len().saturating_add(3) >= len {
            return None;
        }
    }
    if len.saturating_sub(out.len()) < 1 {
        return None;
    }
    Some(out)
}

/// `blkid_safe_string(str, str_safe, len)`: white space normalized, then
/// printable ASCII and valid UTF-8 kept, `\x` kept, each other white-space
/// byte and each byte of anything else made `_`. The result fits `len`
/// bytes with its NUL.
#[must_use]
pub fn safe_string(s: &[u8], len: usize) -> Vec<u8> {
    if len == 0 {
        return Vec::new();
    }
    let n = s
        .iter()
        .take(len)
        .position(|&b| b == 0)
        .unwrap_or(s.len().min(len));
    let mut out = normalize_whitespace(s.get(..n).unwrap_or_default(), len);
    let mut i = 0usize;
    while i < len && i < out.len() {
        let c = out.get(i).copied().unwrap_or(0);
        if c > 0x20 && c <= 0x7e {
            i = i.saturating_add(1);
        } else if c == b'\\' && out.get(i.saturating_add(1)) == Some(&b'x') {
            // Unreachable as written -- a backslash is printable ASCII and the
            // branch above takes it -- but upstream has it, so it is here.
            i = i.saturating_add(2);
        } else if c_isspace(c) {
            if let Some(b) = out.get_mut(i) {
                *b = b'_';
            }
            i = i.saturating_add(1);
        } else if let Some(n) = utf8_valid_len(out.get(i..).unwrap_or_default()) {
            i = i.saturating_add(n);
        } else {
            if let Some(b) = out.get_mut(i) {
                *b = b'_';
            }
            i = i.saturating_add(1);
        }
    }
    // `str_safe[len - 1] = '\0'`.
    out.truncate(len.saturating_sub(1));
    out
}
