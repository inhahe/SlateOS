//! util-linux's `include/carefulputc.h`: `fputs_careful`, the way a util-linux
//! program puts text that somebody else chose onto a terminal.
//!
//! `last` prints user and host names out of a wtmp file through it, and
//! `write` and `wall` a message from another user, so a name or a message
//! cannot carry an escape sequence to the reader's screen. What it keeps and
//! what it marks:
//!
//! - printable ASCII, and `\a`, `\t` and `\r`, as they are;
//! - every other ASCII byte -- the controls and DEL -- as `ctrl` and the byte
//!   with bit 6 flipped: `^A` as `*A` when `ctrl` is `*`, DEL as `*?`;
//! - a whole UTF-8 character that `iswprint` accepts, as it is; anything else
//!   at or above 0x80 -- a byte that begins no character, a sequence cut
//!   short, or a character that is not printable -- one byte at a time, each
//!   as `\` and three octal digits;
//! - a newline as a newline, or `\r\n` for a terminal in raw mode (`cr_lf`);
//! - and, given a `soft_width`, a line wrapped once it reaches that many
//!   columns, each line padded with blanks to the width first.
//!
//! The locale is UTF-8's, as every program here runs in; `iswprint` is
//! [`quoting::printable_char`], measured against glibc 2.39's table, and a
//! column is [`charwidth::char_width`]'s.

/// `fputs_careful (s, fp, ctrl, cr_lf, soft_width)`: what it writes for the C
/// string `s` -- up to its first NUL, as `strlen` sees it.
///
/// Upstream stops at the first write that fails; the caller here writes the
/// bytes and meets that failure itself.
#[must_use]
pub fn fputs_careful(s: &[u8], ctrl: u8, cr_lf: bool, soft_width: usize) -> Vec<u8> {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    let s = s.get(..end).unwrap_or_default();
    let soft = i64::try_from(soft_width).unwrap_or(i64::MAX);
    let mut out = Vec::with_capacity(s.len());
    // `int col`, which `\r` sets to -1 and `\a` takes one off.
    let mut col: i64 = 0;
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        match c {
            b'\t' => col = col.saturating_add(7i64.saturating_sub(col % 8).saturating_sub(1)),
            b'\r' => col = -1,
            0x07 => col = col.saturating_sub(1),
            _ => {}
        }

        if (soft != 0 && col >= soft) || c == b'\n' {
            if soft != 0 {
                // `fprintf (fp, "%*s", soft_width - col, "")`: a negative
                // width is a left-justified field of that many blanks.
                let pad = soft.saturating_sub(col).unsigned_abs();
                out.resize(
                    out.len().saturating_add(usize::try_from(pad).unwrap_or(0)),
                    b' ',
                );
                col = 0;
            }
            out.extend_from_slice(if cr_lf { b"\r\n" } else { b"\n" });
            if c == b'\n' {
                i = i.saturating_add(1);
                continue;
            }
        }

        if (0x20..0x7f).contains(&c) || c == 0x07 || c == b'\t' || c == b'\r' {
            out.push(c);
            col = col.saturating_add(1);
        } else if c >= 0x80 {
            // `mbtowc`: a printable character goes out whole; an invalid or
            // incomplete sequence, or a character `iswprint` refuses, costs
            // its first byte and the scan resumes at the next.
            if let Some(quoting::Mb::Char(ch, len)) =
                quoting::next_mb(s.get(i..).unwrap_or_default())
                && quoting::printable_char(ch)
            {
                out.extend_from_slice(s.get(i..i.saturating_add(len)).unwrap_or_default());
                if soft != 0 {
                    // `wcwidth`, whose -1 for a character `iswprint` accepts
                    // does not arise.
                    let w = charwidth::char_width(ch).map_or(-1, |w| i64::try_from(w).unwrap_or(0));
                    col = col.saturating_add(w);
                }
                i = i.saturating_add(len);
                continue;
            }
            // `"\\%3hho"`: three digits always, a byte this high being at
            // least 0o200.
            out.extend_from_slice(format!("\\{c:3o}").as_bytes());
            col = col.saturating_add(4);
        } else {
            out.push(ctrl);
            out.push(c ^ 0x40);
            col = col.saturating_add(2);
        }
        i = i.saturating_add(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fputs_careful;

    #[test]
    fn controls_are_marked_and_printables_kept() {
        assert_eq!(
            fputs_careful(b"a\x01b\x7fc\x1b[0m", b'*', false, 0),
            b"a*Ab*?c*[[0m"
        );
        assert_eq!(
            fputs_careful(b"bell\x07tab\tcr\r", b'*', false, 0),
            b"bell\x07tab\tcr\r"
        );
        assert_eq!(
            fputs_careful(b"one\ntwo\n", b'^', true, 0),
            b"one\r\ntwo\r\n"
        );
    }

    #[test]
    fn the_string_ends_at_its_first_nul() {
        assert_eq!(fputs_careful(b"root\0junk", b'*', false, 0), b"root");
    }

    #[test]
    fn bytes_that_are_not_a_printable_character_go_out_in_octal() {
        // A lone byte, a sequence cut short, and a C1 control -- valid UTF-8
        // that `iswprint` refuses, each of whose bytes is then escaped alone.
        assert_eq!(fputs_careful(b"\xe9t\xc3", b'*', false, 0), b"\\351t\\303");
        assert_eq!(
            fputs_careful("\u{85}".as_bytes(), b'*', false, 0),
            b"\\302\\205"
        );
        assert_eq!(
            fputs_careful("\u{2028}".as_bytes(), b'*', false, 0),
            b"\\342\\200\\250"
        );
        // A printable one goes out whole, wide or not.
        assert_eq!(
            fputs_careful("é漢\u{2000}".as_bytes(), b'*', false, 0),
            "é漢\u{2000}".as_bytes()
        );
    }

    #[test]
    fn a_soft_width_wraps_and_pads() {
        assert_eq!(fputs_careful(b"abcdef", b'*', false, 4), b"abcd\nef");
        // A newline pads the line it ends to the width.
        assert_eq!(fputs_careful(b"ab\ncd", b'*', false, 4), b"ab  \ncd");
        // Two columns of a wide character can carry a line past the width,
        // and the padding then counts the excess instead.
        assert_eq!(
            fputs_careful("abc漢d".as_bytes(), b'*', false, 4),
            "abc漢 \nd".as_bytes()
        );
        // A tab counts as reaching the seventh column of its eight, not the
        // eighth -- upstream's arithmetic -- so the next byte wraps at seven
        // and not at eight.
        assert_eq!(fputs_careful(b"a\tb", b'*', false, 7), b"a\t\nb");
        assert_eq!(fputs_careful(b"a\tb", b'*', false, 8), b"a\tb");
    }
}
