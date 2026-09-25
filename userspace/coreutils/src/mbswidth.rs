//! gnulib's `mbswidth (s, 0)`: how many terminal columns a string occupies.
//!
//! Not `s.len()` and not a count of characters. A wide character takes two
//! columns and a combining mark none, so a program that lines text up by
//! either count misaligns every column to the right of the first name that is
//! not ASCII. `df` sizes its columns by this and `pr` centres its page header
//! by it; `df` carried the only copy until `pr` needed the same answer.
//!
//! # The rules, with flags of zero
//!
//! gnulib's flags choose what happens to text that is not a printable
//! character, and every caller here passes zero, which is the lenient choice:
//!
//! - a printable character counts its `wcwidth` ([`charwidth::char_width`]);
//! - a character `wcwidth` calls non-printable counts **1**, unless it is a
//!   control character, which counts **0**;
//! - a byte that begins no valid sequence counts **1**, and the scan resumes at
//!   the next byte;
//! - a valid sequence cut short by the end of the string counts **1** for the
//!   whole remaining tail.
//!
//! # Only the multibyte branch
//!
//! gnulib has a second, byte-at-a-time branch for a locale whose characters
//! are all one byte (`MB_CUR_MAX == 1`, the `C` locale). The string layer here
//! is UTF-8 in every locale (`design-decisions.md` §356), so that branch cannot
//! be reached and is not written. Under GNU's `LC_ALL=C` a name holding `é`
//! is two columns wide; here it is one, as it is on screen.

use quoting::{Mb, next_mb};

/// The number of columns `s` occupies. See the module docs for the rules.
#[must_use]
pub fn mbswidth(s: &[u8]) -> usize {
    let mut width = 0usize;
    let mut rest = s;
    while let Some(mb) = next_mb(rest) {
        match mb {
            Mb::Char(c, n) => {
                // `wcwidth` answers -1 for a non-printable character, which
                // gnulib's flags-of-zero path turns into 1 -- except for a
                // control character, which it counts as 0.
                let w = charwidth::char_width(c).unwrap_or(usize::from(!c.is_control()));
                width = width.saturating_add(w);
                rest = rest.get(n..).unwrap_or_default();
            }
            Mb::Invalid => {
                width = width.saturating_add(1);
                rest = rest.get(1..).unwrap_or_default();
            }
            Mb::Incomplete => {
                width = width.saturating_add(1);
                break;
            }
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::mbswidth;

    #[test]
    fn width_is_measured_in_columns() {
        assert_eq!(mbswidth(b""), 0);
        assert_eq!(mbswidth(b"abc"), 3);
        // A wide character is two columns, and a combining mark is none.
        assert_eq!(mbswidth("\u{4e00}".as_bytes()), 2);
        assert_eq!(mbswidth("e\u{301}".as_bytes()), 1);
        assert_eq!(mbswidth("é".as_bytes()), 1);
    }

    #[test]
    fn a_control_character_is_no_columns() {
        assert_eq!(mbswidth(b"a\tb"), 2);
        assert_eq!(mbswidth(b"\x1b[0m"), 3);
        assert_eq!(mbswidth(b"\x7f"), 0);
    }

    #[test]
    fn undecodable_bytes_still_occupy_the_terminal() {
        // One column per byte that begins nothing...
        assert_eq!(mbswidth(b"a\xffb"), 3);
        assert_eq!(mbswidth(b"\xff\xfe"), 2);
        // ...and a lead byte followed by a byte that cannot continue it is
        // invalid on its own, the follower then counted for itself.
        assert_eq!(mbswidth(b"\xc3("), 2);
        // A sequence cut short by the end of the string is one column whatever
        // its length.
        assert_eq!(mbswidth(b"ab\xe4\xb8"), 3);
        assert_eq!(mbswidth(b"\xc3"), 1);
    }
}
