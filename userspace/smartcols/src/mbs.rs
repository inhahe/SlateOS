//! util-linux's `lib/mbsalign.c`: how many screen cells a string takes, how
//! it is made safe to print, and how it is cut to a width.
//!
//! Every function works on a C string -- bytes up to the first NUL -- and
//! decodes it as glibc's `mbrtowc` does: in a UTF-8 locale through
//! [`quoting::next_mb`], and in any other every byte above 0x7f is an invalid
//! sequence, which is what glibc's C locale makes of it (`utf8` says which).
//! `iswprint` is [`quoting::printable_char`] and `wcwidth` is
//! [`charwidth::char_width`], the same answers every other port here gives.
//! `isprint`/`iscntrl` on a single byte are the C locale's, which a UTF-8
//! locale shares for every byte (none above 0x7f is either).

use quoting::{Mb, next_mb, printable_char};

/// The bytes of a C string: up to its first NUL.
#[must_use]
pub fn c_str(s: &[u8]) -> &[u8] {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    s.get(..end).unwrap_or_default()
}

/// `iscntrl` on one byte.
#[must_use]
pub fn is_cntrl(b: u8) -> bool {
    b < 0x20 || b == 0x7f
}

/// `isprint` on one byte.
#[must_use]
pub fn is_print(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// One `mbrtowc` step at the front of `s`: the character and its length, or
/// `None` for either failure -- which every caller here treats as one byte.
/// (Upstream's strings are NUL-terminated, so a sequence cut short is always
/// followed by a byte that cannot continue it: `-1`, never `-2`.)
fn step(s: &[u8], utf8: bool) -> Option<(char, usize)> {
    if !utf8 {
        let &b = s.first()?;
        return b.is_ascii().then_some((char::from(b), 1));
    }
    match next_mb(s)? {
        Mb::Char(c, n) => Some((c, n)),
        Mb::Invalid | Mb::Incomplete => None,
    }
}

/// `wcwidth`, where only a printable character is asked: its cells.
fn cells(c: char) -> usize {
    charwidth::char_width(c).unwrap_or(0)
}

/// Whether `s` converts whole, as `mbstowcs` asks: in a UTF-8 locale, valid
/// UTF-8; in any other, ASCII.
fn decode(s: &[u8], utf8: bool) -> Option<&str> {
    if utf8 || s.is_ascii() {
        std::str::from_utf8(s).ok()
    } else {
        None
    }
}

/// `mbs_nwidth(buf, bufsz)`: the cells `buf` takes, ignoring control and
/// non-printable characters, and skipping an escape sequence that looks like
/// a colour -- any control byte followed by `[` and then up to an `m`.
#[must_use]
pub fn mbs_nwidth(buf: &[u8], utf8: bool) -> usize {
    let buf = c_str(buf);
    let last = buf.len().saturating_sub(1);
    let mut width = 0usize;
    let mut i = 0usize;
    while let Some(&b) = buf.get(i) {
        if is_cntrl(b) {
            i = i.saturating_add(1);
            if buf.get(i) == Some(&b'[') {
                // `while (*e && e < last && *e != 'm') e++; if (*e == 'm')`:
                // the scan stops at the last byte, which may be the `m`.
                let mut e = i;
                while e < last && buf.get(e).is_some_and(|&c| c != b'm') {
                    e = e.saturating_add(1);
                }
                if buf.get(e) == Some(&b'm') {
                    i = e.saturating_add(1);
                }
            }
            continue;
        }
        match step(buf.get(i..).unwrap_or_default(), utf8) {
            Some((c, n)) => {
                if printable_char(c) {
                    width = width.saturating_add(cells(c));
                }
                i = i.saturating_add(n);
            }
            None => i = i.saturating_add(1),
        }
    }
    width
}

/// `mbs_width(s)`.
#[must_use]
pub fn mbs_width(s: &[u8], utf8: bool) -> usize {
    mbs_nwidth(s, utf8)
}

/// `mbs_safe_nwidth(buf, bufsz, &sz)`: the cells and bytes `buf` takes once
/// [`mbs_safe_encode`] has written every control, non-printable or invalid
/// byte as `\x??` -- and the backslash of a literal `\x`, so that it cannot be
/// read as one.
#[must_use]
pub fn mbs_safe_nwidth(buf: &[u8], utf8: bool) -> (usize, usize) {
    let buf = c_str(buf);
    let (mut width, mut bytes) = (0usize, 0usize);
    let mut i = 0usize;
    while let Some(&b) = buf.get(i) {
        let escaped_x = b == b'\\' && buf.get(i.saturating_add(1)) == Some(&b'x');
        if escaped_x || is_cntrl(b) {
            width = width.saturating_add(4);
            bytes = bytes.saturating_add(4);
            i = i.saturating_add(1);
            continue;
        }
        match step(buf.get(i..).unwrap_or_default(), utf8) {
            None => {
                let n = if is_print(b) { 1 } else { 4 };
                width = width.saturating_add(n);
                bytes = bytes.saturating_add(n);
                i = i.saturating_add(1);
            }
            Some((c, n)) if !printable_char(c) => {
                width = width.saturating_add(n.saturating_mul(4));
                bytes = bytes.saturating_add(n.saturating_mul(4));
                i = i.saturating_add(n);
            }
            Some((c, n)) => {
                width = width.saturating_add(cells(c));
                bytes = bytes.saturating_add(n);
                i = i.saturating_add(n);
            }
        }
    }
    (width, bytes)
}

/// `mbs_safe_width(s)`.
#[must_use]
pub fn mbs_safe_width(s: &[u8], utf8: bool) -> usize {
    mbs_safe_nwidth(s, utf8).0
}

/// Append `\x%02x` for `b`.
fn push_hex(out: &mut Vec<u8>, b: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.extend_from_slice(b"\\x");
    out.push(HEX.get(usize::from(b >> 4)).copied().unwrap_or(b'0'));
    out.push(HEX.get(usize::from(b & 0xf)).copied().unwrap_or(b'0'));
}

/// `mbs_safe_encode_to_buffer(s, &width, buf, safechars)`: `s` with every
/// control, non-printable or invalid byte written `\x??`, and its width in
/// cells. A byte in `safechars` is copied as it is. `None` for an empty
/// string, as upstream returns NULL for one.
#[must_use]
pub fn mbs_safe_encode(s: &[u8], safechars: Option<&[u8]>, utf8: bool) -> Option<(Vec<u8>, usize)> {
    let s = c_str(s);
    if s.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(s.len());
    let mut width = 0usize;
    let mut i = 0usize;
    while let Some(&b) = s.get(i) {
        if safechars.is_some_and(|set| set.contains(&b)) {
            out.push(b);
            i = i.saturating_add(1);
            continue;
        }
        let escaped_x = b == b'\\' && s.get(i.saturating_add(1)) == Some(&b'x');
        if escaped_x || is_cntrl(b) {
            push_hex(&mut out, b);
            width = width.saturating_add(4);
            i = i.saturating_add(1);
            continue;
        }
        match step(s.get(i..).unwrap_or_default(), utf8) {
            None => {
                if is_print(b) {
                    out.push(b);
                    width = width.saturating_add(1);
                } else {
                    push_hex(&mut out, b);
                    width = width.saturating_add(4);
                }
                i = i.saturating_add(1);
            }
            Some((c, n)) if !printable_char(c) => {
                for &byte in s.get(i..i.saturating_add(n)).unwrap_or_default() {
                    push_hex(&mut out, byte);
                    width = width.saturating_add(4);
                }
                i = i.saturating_add(n);
            }
            Some((c, n)) => {
                out.extend_from_slice(s.get(i..i.saturating_add(n)).unwrap_or_default());
                width = width.saturating_add(cells(c));
                i = i.saturating_add(n);
            }
        }
    }
    Some((out, width))
}

/// `mbs_truncate(str, &width)`: cut `s` to at most `width` cells, and set
/// `width` to the cells kept. A character with no width (`wcwidth` -1)
/// becomes U+FFFD and counts one cell, as upstream's `wc_truncate` makes it.
///
/// A string that does not convert is left whole and `width` untouched:
/// upstream's `mbstowcs` fails on it and it returns the byte length as is.
/// Returns the byte length kept.
pub fn mbs_truncate(s: &mut Vec<u8>, width: &mut usize, utf8: bool) -> usize {
    let text = c_str(s).to_vec();
    let Some(decoded) = decode(&text, utf8) else {
        s.truncate(text.len());
        return text.len();
    };
    // `wc_truncate`: the characters that fit in `width` cells.
    let mut kept = String::new();
    let mut used = 0usize;
    for c in decoded.chars() {
        let (c, w) = match charwidth::char_width(c) {
            Some(w) => (c, w),
            None => ('\u{FFFD}', 1),
        };
        if used.saturating_add(w) > *width {
            break;
        }
        used = used.saturating_add(w);
        kept.push(c);
    }
    *width = used;
    // `wcstombs(str, wcs, bytes)`: converted back into the original string's
    // room, stopping before a character that no longer fits -- a U+FFFD that
    // replaced a one-byte control does not.
    let mut out = Vec::with_capacity(text.len());
    let mut room = [0u8; 4];
    for c in kept.chars() {
        let enc = c.encode_utf8(&mut room).as_bytes();
        if out.len().saturating_add(enc.len()) > text.len() {
            break;
        }
        out.extend_from_slice(enc);
    }
    *s = out;
    s.len()
}

/// `mbs_align_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// `mbsalign_with_padding(src, ..., &width, align, 0, padchar)` for the one
/// caller here, the title: `src` fitted into `width` cells, truncated or
/// padded with `pad` as `align` says, and `width` set to the cells the text
/// itself takes. A string that does not convert is taken byte by byte, as
/// upstream's unibyte path does.
#[must_use]
pub fn mbsalign(src: &[u8], width: &mut usize, align: Align, pad: u8, utf8: bool) -> Vec<u8> {
    let src = c_str(src);
    let (text, n_cols): (Vec<u8>, usize) = if let Some(s) = decode(src, utf8) {
        // `wc_ensure_printable`, then `rpl_wcswidth`.
        let printable: String = s
            .chars()
            .map(|c| if printable_char(c) { c } else { '\u{FFFD}' })
            .collect();
        let cols = printable
            .chars()
            .map(|c| charwidth::char_width(c).unwrap_or(1))
            .fold(0usize, usize::saturating_add);
        if cols > *width || printable.as_bytes() != src {
            let mut bytes = printable.into_bytes();
            let mut w = *width;
            mbs_truncate(&mut bytes, &mut w, utf8);
            (bytes, w)
        } else {
            (src.to_vec(), cols)
        }
    } else {
        let n = src.len().min(*width);
        (src.get(..n).unwrap_or_default().to_vec(), n)
    };
    let n_spaces = width.saturating_sub(n_cols);
    *width = n_cols;
    let (start, end) = match align {
        Align::Left => (0, n_spaces),
        Align::Right => (n_spaces, 0),
        Align::Center => (n_spaces.div_ceil(2), n_spaces / 2),
    };
    let mut out = vec![pad; start];
    out.extend_from_slice(&text);
    out.extend(std::iter::repeat_n(pad, end));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn width_counts_cells_and_skips_what_is_not_printed() {
        assert_eq!(mbs_width(b"abc", true), 3);
        assert_eq!(mbs_width("caf\u{e9}".as_bytes(), true), 4);
        assert_eq!(mbs_width("\u{4e00}".as_bytes(), true), 2);
        // A colour sequence costs nothing.
        assert_eq!(mbs_width(b"\x1b[1;31mred\x1b[0m", true), 3);
        assert_eq!(mbs_width(b"a\tb", true), 2);
        // An invalid byte costs nothing either.
        assert_eq!(mbs_width(b"a\xffb", true), 2);
        assert_eq!(mbs_width(b"", true), 0);
    }

    #[test]
    fn safe_width_is_the_width_once_encoded() {
        assert_eq!(mbs_safe_nwidth(b"abc", true), (3, 3));
        assert_eq!(mbs_safe_nwidth(b"a\tb", true), (6, 6));
        assert_eq!(mbs_safe_nwidth(b"a\xffb", true), (6, 6));
        // `\x` is written `\x5cx`.
        assert_eq!(mbs_safe_nwidth(b"\\x", true), (5, 5));
        assert_eq!(mbs_safe_nwidth("\u{4e00}".as_bytes(), true), (2, 3));
        // U+0085 is valid UTF-8 and not printable: both bytes are escaped.
        assert_eq!(mbs_safe_nwidth("\u{85}".as_bytes(), true), (8, 8));
    }

    #[test]
    fn safe_encoding_escapes_as_upstream_does() {
        assert_eq!(mbs_safe_encode(b"", None, true), None);
        assert_eq!(
            mbs_safe_encode(b"a b", None, true),
            Some((b"a b".to_vec(), 3))
        );
        assert_eq!(
            mbs_safe_encode(b"a\tb\\xc\xff", None, true),
            Some((b"a\\x09b\\x5cxc\\xff".to_vec(), 16))
        );
        assert_eq!(
            mbs_safe_encode("\u{85}".as_bytes(), None, true),
            Some((b"\\xc2\\x85".to_vec(), 8))
        );
        // A safe character is copied, control or not.
        assert_eq!(
            mbs_safe_encode(b"a\nb", Some(b"\n"), true),
            Some((b"a\nb".to_vec(), 2))
        );
    }

    #[test]
    fn truncation_counts_cells_not_bytes() {
        let mut s = "ab\u{4e00}c".as_bytes().to_vec();
        let mut w = 3;
        assert_eq!(mbs_truncate(&mut s, &mut w, true), 2);
        assert_eq!((s.as_slice(), w), (&b"ab"[..], 2));
        let mut s = "ab\u{4e00}c".as_bytes().to_vec();
        let mut w = 4;
        mbs_truncate(&mut s, &mut w, true);
        assert_eq!((s, w), ("ab\u{4e00}".as_bytes().to_vec(), 4));
        // An undecodable string is left alone.
        let mut s = b"abc\xffdef".to_vec();
        let mut w = 2;
        assert_eq!(mbs_truncate(&mut s, &mut w, true), 7);
        assert_eq!(w, 2);
        // A one-byte control becomes U+FFFD, which no longer fits its room.
        let mut s = b"\x01".to_vec();
        let mut w = 5;
        assert_eq!(mbs_truncate(&mut s, &mut w, true), 0);
        assert_eq!(w, 1);
    }

    #[test]
    fn alignment_pads_and_truncates() {
        let mut w = 6;
        assert_eq!(
            mbsalign(b"abc", &mut w, Align::Right, b' ', true),
            b"   abc"
        );
        assert_eq!(w, 3);
        let mut w = 6;
        assert_eq!(
            mbsalign(b"abc", &mut w, Align::Center, b'-', true),
            b"--abc-"
        );
        let mut w = 2;
        assert_eq!(mbsalign(b"abc", &mut w, Align::Left, b' ', true), b"ab");
    }

    #[test]
    fn outside_a_utf8_locale_every_high_byte_is_invalid() {
        assert_eq!(mbs_width("caf\u{e9}".as_bytes(), false), 3);
        assert_eq!(mbs_safe_nwidth("caf\u{e9}".as_bytes(), false), (11, 11));
        assert_eq!(
            mbs_safe_encode("caf\u{e9}".as_bytes(), None, false),
            Some((b"caf\\xc3\\xa9".to_vec(), 11))
        );
    }
}
