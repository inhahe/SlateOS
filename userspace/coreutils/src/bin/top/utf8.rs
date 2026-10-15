//! `top`'s own multi-byte support (`UTF8_tab` and the `utf8_*` helpers):
//! how many bytes a character starts with, and how many columns it takes.
//!
//! Upstream reads every string as UTF-8 whatever the locale, and asks the
//! locale (`mbtowc`, `wcwidth`) only how wide a multi-byte character is. A
//! byte that cannot start a character makes the whole string "untranslated":
//! each function then answers as if every byte were one column.
//!
//! The strings here are C strings without their NUL: the end of the slice is
//! where upstream's scan would meet it. Where upstream steps past a NUL --
//! a lead byte that promises more bytes than the string has left -- the scan
//! here stops at the end instead of reading what lies beyond.

/// `UTF8_tab[b]`: the length of the sequence `b` begins, or -1 for a byte
/// that begins none (a continuation byte, the overlong `C0` and `C1`, and
/// everything past `F4`).
#[must_use]
pub fn tab(b: u8) -> i32 {
    match b {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => -1,
    }
}

/// `utf8_cols (p, n)`: the columns the `n`-byte character at the start of
/// `p` takes -- `wcwidth` of it, with a character `wcwidth` cannot show
/// taken as one column and a combining one as none.
///
/// Where `mbtowc` cannot decode the bytes (a locale that is not UTF-8, or a
/// sequence the string cuts short) upstream passes `wcwidth` whatever its
/// uninitialised `wchar_t` held. In Ubuntu's build that is a character of
/// no width -- `café` is three columns wide in the C locale, which
/// `scripts/top-diff.sh` measures -- so here such a character is none.
#[must_use]
pub fn cols(p: &[u8], n: i32) -> i32 {
    /// What `wcwidth` says of the character an undecodable sequence
    /// leaves behind.
    const UNDECODED: i32 = 0;
    if n <= 1 {
        return n;
    }
    let len = usize::try_from(n).unwrap_or(0);
    let Some(bytes) = p.get(..len) else {
        return UNDECODED;
    };
    let mut state = libcall::locale::MbState::new();
    match libcall::locale::mbrtowc(bytes, &mut state) {
        libcall::locale::Mb::Char(wc, _) => {
            let w = libcall::locale::wcwidth(wc);
            if w < 0 { 1 } else { w }
        }
        _ => UNDECODED,
    }
}

/// `utf8_delta (str)`: the bytes of `s` less the columns it takes -- 0 for a
/// string with a byte that starts no character.
#[must_use]
pub fn delta(s: &[u8]) -> i32 {
    let mut i = 0usize;
    let mut cnum = 0i32;
    while let Some(&b) = s.get(i) {
        let clen = tab(b);
        if clen < 0 {
            return 0;
        }
        cnum = cnum.saturating_add(cols(s.get(i..).unwrap_or_default(), clen));
        i = i.saturating_add(usize::try_from(clen).unwrap_or(1));
    }
    i32::try_from(i.min(s.len()))
        .unwrap_or(i32::MAX)
        .saturating_sub(cnum)
}

/// `utf8_embody (str, width)`: how many bytes of `s` fit in `width` columns
/// without splitting a character -- or `width` itself, for a string with a
/// byte that starts no character.
#[must_use]
pub fn embody(s: &[u8], width: i32) -> i32 {
    let mut i = 0usize;
    let mut cnum = 0i32;
    if width > 0 {
        while let Some(&b) = s.get(i) {
            let clen = tab(b);
            if clen < 0 {
                return width;
            }
            cnum = cnum.saturating_add(cols(s.get(i..).unwrap_or_default(), clen));
            if width < cnum {
                break;
            }
            i = i.saturating_add(usize::try_from(clen).unwrap_or(1));
        }
    }
    i32::try_from(i.min(s.len())).unwrap_or(i32::MAX)
}

/// `utf8_proper_col (str, col, tophysical)`: with `tophysical`, the byte
/// offset of logical column `col`; without, the logical column of byte
/// offset `col`. A string with a byte that starts no character answers
/// `col` unchanged.
#[must_use]
pub fn proper_col(s: &[u8], col: i32, tophysical: bool) -> i32 {
    let mut i = 0usize;
    let mut tlen = 0i32;
    let mut cnum = 0i32;
    while let Some(&b) = s.get(i) {
        let clen = tab(b);
        if clen < 0 {
            return col;
        }
        if cnum.saturating_add(1) > col && tophysical {
            break;
        }
        i = i.saturating_add(usize::try_from(clen).unwrap_or(1));
        tlen = tlen.saturating_add(clen);
        if tlen > col && !tophysical {
            break;
        }
        cnum = cnum.saturating_add(1);
    }
    if tophysical { tlen } else { cnum }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_one_column_a_byte() {
        assert_eq!(delta(b"hello"), 0);
        assert_eq!(embody(b"hello", 3), 3);
        assert_eq!(embody(b"hi", 9), 2);
        assert_eq!(embody(b"hello", 0), 0);
        assert_eq!(proper_col(b"hello", 2, true), 2);
        assert_eq!(proper_col(b"hello", 2, false), 2);
    }

    #[test]
    fn a_character_the_locale_cannot_decode_takes_no_column() {
        // The test runs in the C locale, as an unset environment leaves it.
        assert_eq!(cols(b"\xc3\xa9", 2), 0);
        assert_eq!(cols(b"\xe4\xb8\x80", 3), 0);
        // Cut short by the end of the string: the same.
        assert_eq!(cols(b"\xc3", 2), 0);
        // So "café" is three columns, and its padding grows by two.
        assert_eq!(delta(b"caf\xc3\xa9"), 2);
    }

    #[test]
    fn a_byte_that_starts_nothing_untranslates_the_string() {
        assert_eq!(tab(0x80), -1);
        assert_eq!(tab(0xc1), -1);
        assert_eq!(tab(0xf5), -1);
        assert_eq!(delta(b"a\x80b"), 0);
        assert_eq!(embody(b"a\x80b", 2), 2);
        assert_eq!(proper_col(b"a\x80b", 5, true), 5);
    }
}
