//! Quoted-string and XML-text "cooking" (`cook.c`, and `configfile.c`'s
//! `cook_xml_text`): escape processing done in place on a NUL-terminated
//! buffer, the result never longer than its source.

use crate::charmap::{HEX_DIGIT, OCT_DIGIT, WHITESPACE, at, is, starts_with, strstr, strtoul};

/// Write `b` at `i`, if the buffer reaches that far. Every write here lands
/// at or before the byte being read, which is inside the buffer.
fn put(s: &mut [u8], i: usize, b: u8) {
    if let Some(slot) = s.get_mut(i) {
        *slot = b;
    }
}

/// `ao_string_cook_escape_char`: the character a backslash escape starting
/// at `i` (the byte after the backslash) stands for, and how many bytes it
/// used. `None` is the escape running into the terminator.
///
/// Unknown escapes stand for themselves; `\x` takes at most two hex digits
/// and is a plain `x` with none; octal takes at most three digits and
/// saturates at 0xFF; `\` before CR LF is one newline.
fn escape_char(s: &[u8], i: usize, nl: u8) -> Option<(u8, usize)> {
    let c = at(s, i);
    let next = i.saturating_add(1);
    let res = match c {
        0 => return None,
        b'\r' => {
            if at(s, next) != b'\n' {
                return Some((b'\r', 1));
            }
            return Some((nl, 2));
        }
        b'\n' => return Some((nl, 1)),
        b'a' => 0x07,
        b'b' => 0x08,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0b,
        b'x' | b'X' => {
            if is(at(s, next), HEX_DIGIT) {
                let mut ct = 1usize;
                if is(at(s, next.saturating_add(1)), HEX_DIGIT) {
                    ct = 2;
                }
                let digits: Vec<u8> = (0..ct).map(|k| at(s, next.saturating_add(k))).collect();
                let (v, _) = strtoul(&digits, 0, 16);
                return Some(((v & 0xff) as u8, ct.saturating_add(1)));
            }
            c
        }
        b'0'..=b'7' => {
            let mut ct = 1usize;
            while ct < 3 && is(at(s, i.saturating_add(ct)), OCT_DIGIT) {
                ct = ct.saturating_add(1);
            }
            let digits: Vec<u8> = (0..ct).map(|k| at(s, i.saturating_add(k))).collect();
            let (v, _) = strtoul(&digits, 0, 8);
            return Some((u8::try_from(v.min(0xff)).unwrap_or(0xff), ct));
        }
        _ => c,
    };
    Some((res, 1))
}

/// `contiguous_quote`: after a closing quote at `ps - 1`, whether another
/// quoted string follows (C-style concatenation, with `//` and `/* */`
/// comments allowed between). `Ok(pos)` is the byte after the new opening
/// quote, with the new quote character; `Err(Some(pos))` is where the text
/// that ended the string starts; `Err(None)` is text that could not be read.
fn contiguous_quote(s: &[u8], mut ps: usize) -> Result<(usize, u8), Option<usize>> {
    loop {
        while is(at(s, ps), WHITESPACE) && at(s, ps) != 0 {
            ps = ps.saturating_add(1);
        }
        match at(s, ps) {
            q @ (b'"' | b'\'') => return Ok((ps.saturating_add(1), q)),
            b'/' => match at(s, ps.saturating_add(1)) {
                b'/' => {
                    let nl = crate::charmap::strchr(s, ps, b'\n');
                    match nl {
                        Some(n) => ps = n,
                        None => return Err(None),
                    }
                }
                b'*' => match strstr(s, ps.saturating_add(2), b"*/") {
                    Some(end) => ps = end.saturating_add(2),
                    None => return Err(None),
                },
                _ => return Err(None),
            },
            _ => return Err(Some(ps)),
        }
    }
}

/// `ao_string_cook`: the quoted string whose opening quote is at `start`,
/// unquoted in place (the result starts at `start`). Double quotes process
/// every escape; single quotes only `\\`, `\'` and `\#`; a backslash before a
/// newline is dropped with it in both. Adjacent quoted strings join. Returns
/// where scanning stopped, `None` when the text ran out first -- the result
/// is NUL-terminated either way.
pub fn string_cook(s: &mut [u8], start: usize) -> Option<usize> {
    let mut q = at(s, start);
    let mut d = start;
    let mut src = start.saturating_add(1);
    loop {
        while at(s, src) == q {
            put(s, d, 0);
            match contiguous_quote(s, src.saturating_add(1)) {
                Ok((next, nq)) => {
                    src = next;
                    q = nq;
                }
                Err(stop) => return stop,
            }
        }
        let c = at(s, src);
        put(s, d, c);
        d = d.saturating_add(1);
        src = src.saturating_add(1);
        match c {
            0 => return None,
            b'\\' => {
                if at(s, src) == b'\n' {
                    src = src.saturating_add(1);
                    d = d.saturating_sub(1);
                } else if q != b'\'' {
                    // A backslash at the very end: the C stores the NUL it
                    // read over the backslash, ending the string there.
                    let Some((ch, ct)) = escape_char(s, src, b'\n') else {
                        put(s, d.saturating_sub(1), 0);
                        return None;
                    };
                    put(s, d.saturating_sub(1), ch);
                    src = src.saturating_add(ct);
                } else if let e @ (b'\\' | b'\'' | b'#') = at(s, src) {
                    put(s, d.saturating_sub(1), e);
                    src = src.saturating_add(1);
                }
            }
            _ => {}
        }
    }
}

/// `trim_quotes`: an argument that starts with a quote is cooked in place.
pub fn trim_quotes(s: &mut [u8], arg: usize) {
    if matches!(at(s, arg), b'"' | b'\'') {
        // Where the scan stopped is of no interest here; the cooked string
        // is terminated in place whether or not a closing quote was found.
        let _ = string_cook(s, arg);
    }
}

/// `parse_xml_encoding`: the character an `&...;` entity after `&` (at `i`)
/// names, and the position after it. NUL for anything unrecognised -- which
/// ends the cooked text there.
fn xml_entity(s: &[u8], i: usize) -> (u8, usize) {
    const NAMES: [(&[u8], u8); 12] = [
        (b"amp;", b'&'),
        (b"lt;", b'<'),
        (b"gt;", b'>'),
        (b"ff;", 0x0c),
        (b"ht;", b'\t'),
        (b"cr;", b'\r'),
        (b"vt;", 0x0b),
        (b"bel;", 0x07),
        (b"nl;", b'\n'),
        (b"space;", b' '),
        (b"quot;", b'"'),
        (b"apos;", b'\''),
    ];
    let mut p = i;
    let numeric = at(s, p) == b'#' || is(at(s, p), crate::charmap::DEC_DIGIT);
    if numeric {
        if at(s, p) == b'#' {
            p = p.saturating_add(1);
        }
        let mut base = 10;
        match at(s, p) {
            b'x' | b'X' => {
                base = 16;
                p = p.saturating_add(1);
            }
            b'0' if at(s, p.saturating_add(1)) == b'0' => base = 16,
            _ => {}
        }
        let (v, end) = strtoul(s, p, base);
        if at(s, end) != b';' || v > 0x7f {
            return (0, i);
        }
        return (u8::try_from(v).unwrap_or(0), end.saturating_add(1));
    }
    for (name, value) in NAMES {
        if starts_with(s, p, name) {
            p = p.saturating_add(name.len());
            return (value, p);
        }
    }
    (0, i)
}

/// `cook_xml_text`: `&entity;` and `%XX` processed in place from `start`.
pub fn cook_xml_text(s: &mut [u8], start: usize) {
    let mut src = start;
    let mut d = start;
    loop {
        let ch = at(s, src);
        src = src.saturating_add(1);
        match ch {
            0 => {
                put(s, d, 0);
                return;
            }
            b'&' => {
                let (c, next) = xml_entity(s, src);
                src = next;
                put(s, d, c);
                d = d.saturating_add(1);
                if c == 0 {
                    return;
                }
            }
            b'%' => {
                let b0 = at(s, src);
                let b1 = at(s, src.saturating_add(1));
                src = src.saturating_add(2);
                if b0 == 0 || b1 == 0 {
                    put(s, d, 0);
                    return;
                }
                let (v, _) = strtoul(&[b0, b1], 0, 16);
                put(s, d, (v & 0xff) as u8);
                d = d.saturating_add(1);
            }
            _ => {
                put(s, d, ch);
                d = d.saturating_add(1);
            }
        }
    }
}
