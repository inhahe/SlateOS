//! libmagic's `is_csv.c`: comma-separated values -- text whose first lines,
//! up to ten, have the same number of commas outside quotes.

use crate::buffer::Buffer;
use crate::funcs::Ms;
use crate::magic::{MAGIC_APPLE, MAGIC_EXTENSION, MAGIC_MIME, MAGIC_MIME_ENCODING};
use crate::printf::Arg;

/// `CSV_LINES`: how many lines are looked at.
const CSV_LINES: usize = 10;

/// `eatquote`: past a quoted field. `""` inside one is an escaped quote.
fn eatquote(s: &[u8], mut uc: usize) -> usize {
    let mut quote = false;
    while uc < s.len() {
        let c = s[uc];
        uc += 1;
        if c != b'"' {
            // We already got one: done.
            if quote {
                return uc - 1;
            }
            continue;
        }
        if quote {
            // Quote-quote escapes.
            quote = false;
            continue;
        }
        // The first quote.
        quote = true;
    }
    s.len()
}

/// `csv_parse`.
fn csv_parse(s: &[u8]) -> bool {
    let (mut nf, mut tf, mut nl) = (0usize, 0usize, 0usize);
    let mut uc = 0usize;
    while uc < s.len() {
        let c = s[uc];
        uc += 1;
        match c {
            b'"' => uc = eatquote(s, uc),
            b',' => nf += 1,
            b'\n' => {
                nl += 1;
                if nl == CSV_LINES {
                    return tf != 0 && tf == nf;
                }
                if tf == 0 {
                    // The first line, and no fields: give up.
                    if nf == 0 {
                        return false;
                    }
                    tf = nf;
                } else if tf != nf {
                    return false;
                }
                nf = 0;
            }
            _ => {}
        }
    }
    tf != 0 && nl >= 2
}

/// `file_is_csv`. `code` is the character code the encoding test named.
pub fn file_is_csv(ms: &mut Ms, b: &Buffer<'_>, looks_text: bool, code: Option<&str>) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if !looks_text {
        return 0;
    }
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    if !csv_parse(b.fbuf) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    if mime != 0 {
        if ms.print_str(b"text/csv") == -1 {
            return -1;
        }
        return 1;
    }
    let (c, sp): (&[u8], &[u8]) = match code {
        Some(c) => (c.as_bytes(), b" "),
        None => (b"", b""),
    };
    if ms.printf(b"CSV %s%stext", &[Arg::Str(c), Arg::Str(sp)]) == -1 {
        return -1;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_is_recognised_as_upstream_recognises_it() {
        assert!(csv_parse(b"a,b,c\n1,2,3\n"));
        assert!(csv_parse(b"a,\"b,x\",c\n1,2,3\n"));
        assert!(!csv_parse(b"a,b\n1,2,3\n"));
        assert!(!csv_parse(b"abc\ndef\n"));
        assert!(!csv_parse(b"a,b\n"));
        // An escaped quote inside a quoted field.
        assert!(csv_parse(b"\"a\"\"b\",c\nd,e\n"));
    }
}
