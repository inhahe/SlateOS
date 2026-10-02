//! libmagic's `ascmagic.c`: what a text file is -- the text rules run over it
//! re-encoded as UTF-8, then its character code and its line endings: `ASCII
//! text, with CRLF line terminators`.

use crate::buffer::Buffer;
use crate::encoding::{Unichar, file_encoding};
use crate::funcs::{Ms, file_replace};
use crate::magic::*;
use crate::printf::Arg;

/// `MAXLINELEN`: a line longer than this is a "very long line".
const MAXLINELEN: usize = 300;

/// `trim_nuls`: the buffer without the NULs at its end, keeping one byte.
fn trim_nuls(buf: &[u8]) -> usize {
    let mut n = buf.len();
    while n > 1 && buf[n - 1] == 0 {
        n -= 1;
    }
    n
}

/// `file_ascmagic`: 1 if the buffer is text and was described, 0 if not.
pub fn file_ascmagic(ms: &mut Ms, b: &Buffer<'_>, text: bool) -> i32 {
    let mut flen = trim_nuls(b.fbuf);
    // Do not trim to an odd length from an even one, which would lose the last
    // character of UTF-16 text.
    if flen & 1 == 1 && b.flen() & 1 == 0 {
        flen += 1;
    }
    let bb = b.with_data(b.fbuf.get(..flen).unwrap_or(b.fbuf));
    let enc = file_encoding(bb.fbuf, ms.encoding_max);
    if !enc.looks_text {
        return 0;
    }
    file_ascmagic_with_encoding(ms, &bb, &enc.ubuf, enc.code, enc.typ, text)
}

/// `file_ascmagic_with_encoding`.
#[allow(clippy::too_many_lines)]
pub fn file_ascmagic_with_encoding(
    ms: &mut Ms,
    b: &Buffer<'_>,
    ubuf: &[Unichar],
    code: &str,
    typ: &str,
    text: bool,
) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    let mut need_separator = false;
    let nbytes = trim_nuls(b.fbuf);
    // Fewer than two bytes: give up.
    if nbytes <= 1 {
        return 0;
    }
    let ulen = ubuf.len();
    if ulen > 0 && ms.flags & MAGIC_NO_CHECK_SOFT == 0 {
        // The text rules, over the characters re-encoded as UTF-8.
        let Some(utf8) = encode_utf8(ubuf) else {
            return 0;
        };
        let bb = Buffer::new(b.fd, Some(b.st), &utf8);
        let mut rv = crate::softmagic::file_softmagic(ms, &bb, None, TEXTTEST, text);
        if rv == 0 {
            rv = -1;
        } else {
            need_separator = true;
        }
        if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
            return if rv == -1 { 0 } else { 1 };
        }
    }
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }

    // Now the other details.
    let mut has_escapes = false;
    let mut has_backspace = false;
    let mut seen_cr = false;
    let (mut n_crlf, mut n_lf, mut n_cr, mut n_nel) = (0usize, 0usize, 0usize, 0usize);
    let mut last_line_end = usize::MAX;
    let mut has_long_lines = 0usize;
    for (i, &c) in ubuf.iter().enumerate() {
        if c == u64::from(b'\n') {
            if seen_cr {
                n_crlf += 1;
            } else {
                n_lf += 1;
            }
            last_line_end = i;
        } else if seen_cr {
            n_cr += 1;
        }
        seen_cr = c == u64::from(b'\r');
        if seen_cr {
            last_line_end = i;
        }
        // X3.64/ECMA-43 "next line".
        if c == 0x85 {
            n_nel += 1;
            last_line_end = i;
        }
        // A line *longer* than MAXLINELEN.
        if i > last_line_end.wrapping_add(MAXLINELEN) {
            let ll = i.wrapping_sub(last_line_end);
            if ll > has_long_lines {
                has_long_lines = ll;
            }
        }
        if c == 0o33 {
            has_escapes = true;
        }
        if c == 0x08 {
            has_backspace = true;
        }
    }

    if typ == "binary" {
        return 0;
    }
    let len = ms.o_blen;
    if mime != 0 {
        if mime & MAGIC_MIME_TYPE != 0 {
            if len != 0 {
                // The rules printed something: done, or a separator.
                if ms.flags & MAGIC_CONTINUE == 0 {
                    return 1;
                }
                if need_separator && ms.separator() == -1 {
                    return -1;
                }
            }
            if ms.print_str(b"text/plain") == -1 {
                return -1;
            }
        }
        return 1;
    }

    let mut executable = false;
    if len != 0 {
        match file_replace(ms, b" text$", b", ") {
            0 => match file_replace(ms, b" text executable$", b", ") {
                0 => {
                    if ms.print_str(b", ") == -1 {
                        return -1;
                    }
                }
                -1 => return -1,
                _ => executable = true,
            },
            -1 => return -1,
            _ => {}
        }
    }
    if ms.print_str(code.as_bytes()) == -1 {
        return -1;
    }
    if ms.printf(b" %s", &[Arg::Str(typ.as_bytes())]) == -1 {
        return -1;
    }
    if executable && ms.print_str(b" executable") == -1 {
        return -1;
    }
    if has_long_lines != 0
        && ms.printf(b", with very long lines (%zu)", &[Arg::I64(has_long_lines as u64)]) == -1
    {
        return -1;
    }
    // Line terminators only when one is not LF, or there are none.
    // Upstream's `(none of them) || crlf || cr || nel`, which is this.
    if n_lf == 0 || n_crlf != 0 || n_cr != 0 || n_nel != 0 {
        // One `file_printf` per piece, as upstream writes them.
        let mut pieces: Vec<&[u8]> = vec![b", with"];
        if n_crlf == 0 && n_cr == 0 && n_nel == 0 && n_lf == 0 {
            pieces.push(b" no");
        } else {
            if n_crlf != 0 {
                pieces.push(b" CRLF");
                if n_cr != 0 || n_lf != 0 || n_nel != 0 {
                    pieces.push(b",");
                }
            }
            if n_cr != 0 {
                pieces.push(b" CR");
                if n_lf != 0 || n_nel != 0 {
                    pieces.push(b",");
                }
            }
            if n_lf != 0 {
                pieces.push(b" LF");
                if n_nel != 0 {
                    pieces.push(b",");
                }
            }
            if n_nel != 0 {
                pieces.push(b" NEL");
            }
        }
        pieces.push(b" line terminators");
        for p in pieces {
            if ms.print_str(p) == -1 {
                return -1;
            }
        }
    }
    if has_escapes && ms.print_str(b", with escape sequences") == -1 {
        return -1;
    }
    if has_backspace && ms.print_str(b", with overstriking") == -1 {
        return -1;
    }
    1
}

/// `encode_utf8`: characters back to UTF-8 -- the old six-byte form for the
/// largest -- or `None` past `0x7fffffff`.
fn encode_utf8(ubuf: &[Unichar]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(ubuf.len().saturating_mul(6));
    for &u in ubuf {
        #[allow(clippy::cast_possible_truncation)]
        let byte = |v: u64| v as u8;
        if u <= 0x7f {
            out.push(byte(u));
            continue;
        }
        let (lead, n) = if u <= 0x7ff {
            ((u >> 6) + 0xc0, 1)
        } else if u <= 0xffff {
            ((u >> 12) + 0xe0, 2)
        } else if u <= 0x1f_ffff {
            ((u >> 18) + 0xf0, 3)
        } else if u <= 0x3ff_ffff {
            ((u >> 24) + 0xf8, 4)
        } else if u <= 0x7fff_ffff {
            ((u >> 30) + 0xfc, 5)
        } else {
            return None;
        };
        out.push(byte(lead));
        for k in (0..n).rev() {
            out.push(byte(((u >> (6 * k)) & 0x3f) + 0x80));
        }
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn utf8_is_rebuilt_as_upstream_builds_it() {
        assert_eq!(encode_utf8(&[0x41, 0xe9, 0x20ac, 0x1f600]).unwrap(), "A\u{e9}\u{20ac}\u{1f600}".as_bytes());
        assert_eq!(encode_utf8(&[0x8000_0000]), None);
    }

    #[test]
    fn nuls_are_trimmed_to_one_byte() {
        assert_eq!(trim_nuls(b"ab\0\0"), 2);
        assert_eq!(trim_nuls(b"\0\0"), 1);
    }
}
