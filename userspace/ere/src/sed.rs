//! GNU sed's escapes: the layer GNU sed puts in front of the regex compiler.
//!
//! GNU sed has escapes that name bytes -- `\t`, `\n`, `\a`, `\f`, `\r`, `\v`,
//! `\dNNN`, `\oNNN`, `\xHH` and `\cX` -- and its `normalize_text` turns them into
//! the bytes before a regex is compiled, inside bracket expressions as well as
//! out of them: GNU `sed 's/[\t]/X/'` replaces a tab. The compiler itself, like
//! glibc's, has none of them (see [`crate::engine`]), which is why `grep`'s
//! `[\t]` is a backslash or a `t` while sed's is a tab. Every other escape
//! reaches the compiler as written: `\w`, `\(`, `\1` and `\.` are its business.
//!
//! It is a module of this crate rather than of `sed` for the reason [`crate::awk`]
//! is: there are two seds, userspace's and the kernel shell's, and an escape
//! layer each kept its own copy of would be two answers to one question.
//!
//! Besides [`regex`], sed's `y` command, its `a`/`i`/`c` text and the
//! replacement of `s` take the same byte-naming escapes with different rules
//! for the rest, so the pieces are public: [`control_byte`] and [`named_byte`].

use alloc::vec::Vec;

/// GNU's refusal of `\c\`, which it will not guess at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecursiveC;

impl RecursiveC {
    /// GNU's wording.
    #[must_use]
    pub fn message(self) -> &'static str {
        "recursive escaping after \\c not allowed"
    }
}

/// The escapes that name a control character by letter.
///
/// GNU converts these in the same pass as the numeric ones below, which is why
/// `\t` is a tab in a regular expression, in a replacement, in `y` and in `a`
/// text alike. `\b` is deliberately absent: GNU sed 4.0 read it as a backspace,
/// but every version since reads it as a word boundary, which is the regex
/// compiler's to interpret and not sed's.
#[must_use]
pub fn control_byte(c: u8) -> Option<u8> {
    Some(match c {
        b'a' => 0x07,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0b,
        _ => return None,
    })
}

/// One digit of `base`, or `None` if `c` is not one.
fn digit(c: u8, base: u8) -> Option<u8> {
    let v = match c {
        b'0'..=b'9' => c.wrapping_sub(b'0'),
        b'a'..=b'f' => c.wrapping_sub(b'a').wrapping_add(10),
        b'A'..=b'F' => c.wrapping_sub(b'A').wrapping_add(10),
        _ => return None,
    };
    (v < base).then_some(v)
}

/// What one of GNU's byte-naming escapes turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    /// The byte it names, and the index just past the escape.
    Byte(u8, usize),
    /// `\c` with the script ending right after it: GNU emits a lone backslash
    /// and drops the `c`. The index is just past the `c`.
    Backslash(usize),
    /// `\c\`, which GNU refuses — see [`RecursiveC`].
    Recursive,
}

/// Read `\xNN`, `\oNNN`, `\dNNN` or `\cX`, where `raw[i]` is the character
/// *after* the backslash.
///
/// `None` means the escape is none of those and belongs to whoever asked: `\w`
/// to the regex compiler, `\1` to the replacement parser, `\;` to nobody.
///
/// The three numeric forms are GNU's `convert_number`, measured rather than
/// recalled: **at most two** hexadecimal digits and **at most three** decimal
/// or octal ones, the value taken mod 256, and *no* digits at all is not an
/// error — the letter then denotes itself, which is why `sed 's/\x/Z/'`
/// replaces an `x`. So GNU reads `\x616` as `a` then `6`, `\d0977` as `a` then
/// `7`, `\x0061` as NUL then `61`, and `\d300` as `,`.
#[must_use]
pub fn named_byte(raw: &[u8], i: usize) -> Option<Named> {
    let letter = raw.get(i).copied()?;
    let (base, max) = match letter {
        b'x' => (16u8, 2usize),
        b'd' => (10, 3),
        b'o' => (8, 3),
        b'c' => {
            let after = i.saturating_add(1);
            return Some(match raw.get(after).copied() {
                None => Named::Backslash(after),
                Some(b'\\') => Named::Recursive,
                // GNU's own arithmetic: fold to upper case, then flip the bit
                // that separates a control code from its printable partner, so
                // `\cI` is a tab and `\c1` is `q`.
                Some(c) => Named::Byte(c.to_ascii_uppercase() ^ 0x40, after.saturating_add(1)),
            });
        }
        _ => return None,
    };
    // Accumulating in a `u8` is the mod-256 wrap, not an accident of width:
    // GNU stores the running value in a `char`, which is why `\d300` is `,`.
    let mut n: u8 = 0;
    let first = i.saturating_add(1);
    let mut j = first;
    let end = first.saturating_add(max);
    while j < end {
        let Some(d) = raw.get(j).copied().and_then(|c| digit(c, base)) else {
            break;
        };
        n = n.wrapping_mul(base).wrapping_add(d);
        j = j.saturating_add(1);
    }
    Some(if j == first {
        Named::Byte(letter, j)
    } else {
        Named::Byte(n, j)
    })
}

/// GNU's `normalize_text` for a regular expression: every escape that names a
/// byte becomes that byte, and every other escape is left for the regex
/// compiler.
///
/// The produced byte is **not** protected, which is GNU's behaviour and is
/// surprising enough to be worth stating: `\x2e` is the metacharacter `.` and
/// not a literal dot, so `sed 's/\x2e/Z/'` replaces the first character of any
/// line. `\x5c` is a bare backslash, so `sed 's/\x5c/Z/'` is a *trailing
/// backslash* error — which is exactly what GNU reports. Both measured.
///
/// It runs after the delimiter scan, so a delimiter it produces is a character
/// and not the end of the command: `sed 's/\x2f/Z/'` replaces a slash.
///
/// # Errors
/// [`RecursiveC`] for `\c\`.
pub fn regex(raw: &[u8]) -> Result<Vec<u8>, RecursiveC> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&n) = raw.get(i) else {
            out.push(b'\\');
            break;
        };
        if let Some(b) = control_byte(n) {
            out.push(b);
            i = i.saturating_add(1);
            continue;
        }
        match named_byte(raw, i) {
            Some(Named::Byte(b, next)) => {
                out.push(b);
                i = next;
            }
            Some(Named::Backslash(next)) => {
                out.push(b'\\');
                i = next;
            }
            Some(Named::Recursive) => return Err(RecursiveC),
            // Not ours: `\w`, `\(`, `\1`, `\.` all reach the compiler as written.
            None => {
                out.push(b'\\');
                out.push(n);
                i = i.saturating_add(1);
            }
        }
    }
    Ok(out)
}

/// [`regex`] as GNU sed runs it outside its default mode -- under
/// `POSIXLY_CORRECT` or `--posix` -- where `normalize_text` keeps track of
/// bracket expressions and leaves everything inside one alone, as POSIX says a
/// bracket's backslash is an ordinary member. So `[\t]` is a backslash or a
/// `t` there, where the default reads a tab: measured, sed 4.9,
/// `POSIXLY_CORRECT=1 sed 's/[\t]/X/'` leaves `a<TAB>b` as it was.
///
/// The tracking is GNU's `bracket_state`, character for character: `[` opens
/// a bracket, `[` followed by `:`, `.` or `=` opens one of the three
/// constructs inside it, the matching `:]`, `.]` or `=]` closes that, and a
/// `]` otherwise closes the bracket. It is cruder than the regex compiler's
/// own reading -- a `]` straight after `[` closes it here -- and that is
/// reproduced, not corrected, because it decides which escapes are converted.
///
/// # Errors
/// [`RecursiveC`] for `\c\` outside a bracket.
pub fn regex_posix(raw: &[u8]) -> Result<Vec<u8>, RecursiveC> {
    /// Outside any bracket.
    const OUTSIDE: u8 = 0;
    /// Inside a bracket, outside `[:`, `[.` and `[=`.
    const INSIDE: u8 = 1;
    let mut out = Vec::with_capacity(raw.len());
    // `OUTSIDE`, `INSIDE`, or the `:`, `.` or `=` of the construct open.
    let mut state = OUTSIDE;
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        let next = raw.get(i.saturating_add(1)).copied();
        if c == b'\\'
            && state == OUTSIDE
            && let Some(n) = next
        {
            i = i.saturating_add(1);
            if let Some(b) = control_byte(n) {
                out.push(b);
                i = i.saturating_add(1);
                continue;
            }
            match named_byte(raw, i) {
                Some(Named::Byte(b, after)) => {
                    out.push(b);
                    i = after;
                }
                Some(Named::Backslash(after)) => {
                    out.push(b'\\');
                    i = after;
                }
                Some(Named::Recursive) => return Err(RecursiveC),
                None => {
                    out.push(b'\\');
                    out.push(n);
                    i = i.saturating_add(1);
                }
            }
            continue;
        }
        let prev = i.checked_sub(1).and_then(|j| raw.get(j)).copied();
        let before_prev = i.checked_sub(2).and_then(|j| raw.get(j)).copied();
        match c {
            b'[' if state == OUTSIDE => state = INSIDE,
            b':' | b'.' | b'=' if state == INSIDE && prev == Some(b'[') => state = c,
            b']' if state == INSIDE => state = OUTSIDE,
            b']' if state != OUTSIDE && before_prev != Some(state) && prev == Some(state) => {
                state = INSIDE;
            }
            _ => {}
        }
        out.push(c);
        i = i.saturating_add(1);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bre;

    /// Outside the default mode, nothing inside a bracket is converted --
    /// and everything outside one still is. Measured, sed 4.9 under
    /// `POSIXLY_CORRECT`.
    #[test]
    fn posix_mode_leaves_a_brackets_escapes_alone() {
        assert_eq!(regex_posix(br"[\t]").unwrap(), br"[\t]");
        assert_eq!(regex_posix(br"\t[\t]\t").unwrap(), b"\t[\\t]\t");
        // A class inside the bracket does not close it at its own `]`.
        assert_eq!(
            regex_posix(br"[[:alpha:]\n]\n").unwrap(),
            b"[[:alpha:]\\n]\n"
        );
        // The default mode converts both.
        assert_eq!(regex(br"[\t]").unwrap(), b"[\t]");
    }

    /// Every row measured against GNU sed 4.9.
    #[test]
    fn the_byte_naming_escapes_become_bytes() {
        assert_eq!(regex(br"a\tb\nc").unwrap(), b"a\tb\nc");
        assert_eq!(regex(br"\a\f\r\v").unwrap(), b"\x07\x0c\r\x0b");
        assert_eq!(regex(br"\x41\o101\d065").unwrap(), b"AAA");
        assert_eq!(regex(br"\cI\c1").unwrap(), b"\tq");
        // Digits are bounded and the value wraps at 256.
        assert_eq!(regex(br"\x616").unwrap(), b"a6");
        assert_eq!(regex(br"\d0977").unwrap(), b"a7");
        assert_eq!(regex(br"\d300").unwrap(), b",");
        // No digits: the letter itself.
        assert_eq!(regex(br"\x").unwrap(), b"x");
        // Inside a bracket too: sed's `[\t]` is a tab.
        assert_eq!(regex(br"[\t]").unwrap(), b"[\t]");
        // A final `\c` is a backslash; `\c\` is refused.
        assert_eq!(regex(br"a\c").unwrap(), br"a\");
        assert_eq!(
            regex(br"\c\d").unwrap_err().message(),
            "recursive escaping after \\c not allowed"
        );
    }

    #[test]
    fn every_other_escape_reaches_the_compiler_as_written() {
        for kept in [&br"\w"[..], br"\(", br"\1", br"\.", br"\\", br"\b", br"\]"] {
            assert_eq!(regex(kept).unwrap(), kept);
        }
        assert_eq!(regex(b"a\\").unwrap(), b"a\\");
    }

    /// The layer and the compiler together give sed's answers: `[\.]` is a
    /// backslash or a dot (glibc's bracket), `[\t]` a tab (sed's escape).
    #[test]
    fn with_the_compiler_it_answers_as_gnu_sed_does() {
        let m = |pat: &[u8], s: &[u8]| {
            bre::compile(&regex(pat).unwrap(), false)
                .unwrap()
                .is_match(s)
                .unwrap()
        };
        assert!(m(br"^[\.]$", br"\"));
        assert!(m(br"^[\.]$", b"."));
        assert!(m(br"^[\t]$", b"\t"));
        assert!(!m(br"^[\t]$", b"t"));
        assert!(m(br"^\(.*\)\n\1$", b"x\nx"));
    }
}
