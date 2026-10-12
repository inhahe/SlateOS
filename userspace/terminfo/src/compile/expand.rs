//! `comp_expand.c`: a string capability written back as source --
//! `_nc_tic_expand`, which `infocmp` and `tic -I`/`-C` print every string
//! through.
//!
//! In terminfo source (`tic_format`), escape is `\E`, the characters that
//! end or start things (`,` `^` `\`) and a leading or trailing space are
//! escaped, and a short string that is mostly control characters shows
//! them as `^X` rather than as octal. In termcap's, `:` and `!` are octal
//! too, and every control character is `^X`. `numbers` chooses how a
//! character constant in a parameterised string is written: -1 as
//! `%{65}`, 1 as `%'A'`, 0 as it was.

use super::scan::cstr;

/// `MAX_TC_FIXUPS`.
const MAX_TC_FIXUPS: usize = 10;
/// `MIN_TC_FIXUPS`.
const MIN_TC_FIXUPS: i64 = 4;

/// `REALPRINT`: printable, and not DEL or eight-bit.
fn realprint(c: u8) -> bool {
    (0x20..0x7f).contains(&c)
}

/// `_nc_tic_expand (srcp, tic_format, numbers)`.
#[must_use]
pub fn tic_expand(srcp: Option<&[u8]>, tic_format: bool, numbers: i32) -> Vec<u8> {
    let s = srcp.map(cstr).unwrap_or_default();
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut buf: Vec<u8> = Vec::with_capacity(s.len().saturating_mul(4));
    let mut fixups: Vec<(u8, usize)> = Vec::new();
    let mut i = 0usize;
    while let Some(&ch) = s.get(i) {
        if ch == b'%' && realprint(at(i.saturating_add(1))) {
            buf.push(ch);
            i = i.saturating_add(1);
            match numbers {
                -1 => {
                    if at(i) == b'\''
                        && at(i.saturating_add(1)) != b'\\'
                        && realprint(at(i.saturating_add(1)))
                        && at(i.saturating_add(2)) == b'\''
                    {
                        buf.extend_from_slice(
                            format!("{{{}}}", at(i.saturating_add(1))).as_bytes(),
                        );
                        i = i.saturating_add(2);
                    } else {
                        buf.push(at(i));
                    }
                }
                1 => {
                    // "If we have a "%{number}", try to translate it into a
                    // "%'char'" form"
                    let mut done = false;
                    if at(i) == b'{' && at(i.saturating_add(1)).is_ascii_digit() {
                        let digits = s.get(i.saturating_add(1)..).unwrap_or_default();
                        let (value, used) = cstrtol::strtol(digits, 0);
                        let dst = i.saturating_add(1).saturating_add(used);
                        if at(dst) == b'}'
                            && value < 127
                            && let Ok(c) = u8::try_from(value)
                            && realprint(c)
                        {
                            buf.push(b'\'');
                            if c == b'\\' || c == b'\'' {
                                buf.push(b'\\');
                            }
                            buf.push(c);
                            buf.push(b'\'');
                            i = dst;
                            done = true;
                        }
                    }
                    if !done {
                        buf.push(at(i));
                    }
                }
                _ => {
                    // "minitel1 uses this"
                    if at(i) == b',' {
                        buf.push(b'\\');
                    }
                    buf.push(at(i));
                }
            }
        } else if ch == 128 {
            buf.extend_from_slice(b"\\0");
        } else if ch == 0x1b {
            buf.extend_from_slice(b"\\E");
        } else if ch == b'\\' && tic_format && (i == 0 || at(i.saturating_sub(1)) != b'^') {
            buf.extend_from_slice(b"\\\\");
        } else if ch == b' '
            && tic_format
            && (i == 0 || s.get(i..).unwrap_or_default().iter().all(|&c| c == b' '))
        {
            buf.extend_from_slice(b"\\s");
        } else if (ch == b',' || ch == b'^') && tic_format {
            buf.push(b'\\');
            buf.push(ch);
        } else if realprint(ch)
            && ch != b','
            && (ch != b':' || tic_format)
            && (ch != b'!' || tic_format)
            && ch != b'^'
        {
            buf.push(ch);
        } else if ch == b'\r' {
            buf.extend_from_slice(b"\\r");
        } else if ch == b'\n' {
            buf.extend_from_slice(b"\\n");
        } else if ch < 32 && at(i.saturating_add(1)).is_ascii_digit() {
            buf.push(b'^');
            buf.push(ch.wrapping_add(b'@'));
        } else {
            if fixups.len() < MAX_TC_FIXUPS && ((tic_format && ch == 127) || ch < 32) {
                fixups.push((ch, buf.len()));
            }
            buf.extend_from_slice(format!("\\{ch:03o}").as_bytes());
        }
        i = i.saturating_add(1);
    }

    // "If most of a short string is ASCII control characters, reformat the
    // string to show those in up-arrow format."
    let octals = i64::try_from(fixups.len()).unwrap_or(i64::MAX);
    let bufp = i64::try_from(buf.len()).unwrap_or(i64::MAX);
    if octals != 0 && (!tic_format || bufp.wrapping_sub(octals.wrapping_mul(4)) < MIN_TC_FIXUPS) {
        for &(ch, offset) in fixups.iter().rev() {
            let shown = if ch == 127 {
                b'?'
            } else {
                ch.wrapping_add(b'@')
            };
            let end = offset.saturating_add(4).min(buf.len());
            buf.splice(offset..end, [b'^', shown]);
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminfo_escapes_what_it_must() {
        assert_eq!(tic_expand(Some(b"\x1b[%p1%dm"), true, 0), b"\\E[%p1%dm");
        assert_eq!(tic_expand(Some(b"a,b^c\\d"), true, 0), b"a\\,b\\^c\\\\d");
        assert_eq!(tic_expand(Some(b" x "), true, 0), b"\\sx\\s");
        assert_eq!(tic_expand(Some(b"\r\n"), true, 0), b"\\r\\n");
        assert_eq!(tic_expand(Some(b"\x80"), true, 0), b"\\0");
    }

    #[test]
    fn a_short_control_string_shows_carets() {
        assert_eq!(tic_expand(Some(b"\x07"), true, 0), b"^G");
        assert_eq!(tic_expand(Some(b"\x08\x01"), true, 0), b"^H^A");
        // Long and mostly not control: octal stays.
        assert_eq!(tic_expand(Some(b"abcdefgh\x01"), true, 0), b"abcdefgh\\001");
        // Termcap: always carets, and its own specials octal.
        assert_eq!(
            tic_expand(Some(b"abcdefgh\x01:"), false, 0),
            b"abcdefgh^A\\072"
        );
    }

    #[test]
    fn character_constants_follow_the_numbers_option() {
        assert_eq!(tic_expand(Some(b"%p1%'A'%+"), true, -1), b"%p1%{65}%+");
        assert_eq!(tic_expand(Some(b"%p1%{65}%+"), true, 1), b"%p1%'A'%+");
        assert_eq!(tic_expand(Some(b"%p1%{39}%+"), true, 1), b"%p1%'\\''%+");
        assert_eq!(tic_expand(Some(b"%p1%{200}%+"), true, 1), b"%p1%{200}%+");
        assert_eq!(tic_expand(Some(b"%,"), true, 0), b"%\\,");
    }

    #[test]
    fn nothing_expands_to_nothing() {
        assert_eq!(tic_expand(None, true, 0), b"");
    }
}
