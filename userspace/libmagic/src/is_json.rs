//! libmagic's `is_json.c`: JSON text, and newline-delimited JSON.
//!
//! A document is JSON when it parses as one value that holds an object or an
//! array; it is newline-delimited JSON when the value is followed by another
//! that starts with the same character. Upstream's parser is kept as it is:
//! a constant cut off by the end of the buffer (`tr`) still counts.

use crate::buffer::Buffer;
use crate::funcs::Ms;
use crate::magic::{MAGIC_APPLE, MAGIC_EXTENSION, MAGIC_MIME, MAGIC_MIME_ENCODING};
use crate::printf::Arg;

const JSON_ARRAY: usize = 0;
const JSON_CONSTANT: usize = 1;
const JSON_NUMBER: usize = 2;
const JSON_OBJECT: usize = 3;
const JSON_STRING: usize = 4;
const JSON_ARRAYN: usize = 5;
const JSON_MAX: usize = 6;

fn json_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t')
}

fn json_skip_space(s: &[u8], mut uc: usize) -> usize {
    while uc < s.len() && json_isspace(s[uc]) {
        uc += 1;
    }
    uc
}

/// `json_parse_string`: after the opening quote.
fn json_parse_string(s: &[u8], ucp: &mut usize) -> bool {
    let ue = s.len();
    let mut uc = *ucp;
    while uc < ue {
        let c = s[uc];
        uc += 1;
        match c {
            0 => break,
            b'\\' => {
                if uc == ue {
                    break;
                }
                let e = s[uc];
                uc += 1;
                match e {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        if ue - uc < 4 {
                            uc = ue;
                            break;
                        }
                        let mut ok = true;
                        for _ in 0..4 {
                            let h = s[uc];
                            uc += 1;
                            if !h.is_ascii_hexdigit() {
                                ok = false;
                                break;
                            }
                        }
                        if !ok {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            b'"' => {
                *ucp = uc;
                return true;
            }
            _ => {}
        }
    }
    *ucp = uc;
    false
}

/// `json_parse_array`: after the `[`.
fn json_parse_array(s: &[u8], ucp: &mut usize, st: &mut [usize; JSON_MAX], lvl: usize) -> bool {
    let ue = s.len();
    let mut uc = *ucp;
    while uc < ue {
        uc = json_skip_space(s, uc);
        if uc == ue {
            break;
        }
        if s[uc] != b']' {
            if !json_parse(s, &mut uc, st, lvl + 1) {
                break;
            }
            if uc == ue {
                break;
            }
            if s[uc] == b',' {
                uc += 1;
                continue;
            }
            if s[uc] != b']' {
                break;
            }
        }
        st[JSON_ARRAYN] += 1;
        *ucp = uc + 1;
        return true;
    }
    *ucp = uc;
    false
}

/// `json_parse_object`: after the `{`.
fn json_parse_object(s: &[u8], ucp: &mut usize, st: &mut [usize; JSON_MAX], lvl: usize) -> bool {
    let ue = s.len();
    let mut uc = *ucp;
    while uc < ue {
        uc = json_skip_space(s, uc);
        if uc == ue {
            break;
        }
        if s[uc] == b'}' {
            *ucp = uc + 1;
            return true;
        }
        let c = s[uc];
        uc += 1;
        if c != b'"' {
            break;
        }
        if !json_parse_string(s, &mut uc) {
            break;
        }
        uc = json_skip_space(s, uc);
        if uc == ue {
            break;
        }
        let c = s[uc];
        uc += 1;
        if c != b':' {
            break;
        }
        if !json_parse(s, &mut uc, st, lvl + 1) {
            break;
        }
        if uc == ue {
            break;
        }
        let c = s[uc];
        uc += 1;
        match c {
            b',' => {}
            b'}' => {
                *ucp = uc;
                return true;
            }
            _ => break,
        }
    }
    *ucp = uc;
    false
}

/// `json_parse_number`.
fn json_parse_number(s: &[u8], ucp: &mut usize) -> bool {
    let ue = s.len();
    let mut uc = *ucp;
    let mut got = false;
    if uc == ue {
        return false;
    }
    'out: {
        if s[uc] == b'-' {
            uc += 1;
        }
        while uc < ue && s[uc].is_ascii_digit() {
            got = true;
            uc += 1;
        }
        if uc == ue {
            break 'out;
        }
        if s[uc] == b'.' {
            uc += 1;
        }
        while uc < ue && s[uc].is_ascii_digit() {
            got = true;
            uc += 1;
        }
        if uc == ue {
            break 'out;
        }
        if got && (s[uc] == b'e' || s[uc] == b'E') {
            uc += 1;
            got = false;
            if uc == ue {
                break 'out;
            }
            if s[uc] == b'+' || s[uc] == b'-' {
                uc += 1;
            }
            while uc < ue && s[uc].is_ascii_digit() {
                got = true;
                uc += 1;
            }
        }
    }
    *ucp = uc;
    got
}

/// `json_parse_const`: `true`, `false` or `null`, after its first letter. The
/// cursor is put past the word first; a word the buffer cuts short matches.
fn json_parse_const(s: &[u8], ucp: &mut usize, word: &[u8]) -> bool {
    let ue = s.len();
    let mut uc = *ucp;
    // `*ucp += --len - 1`, with `len` the word's `sizeof`.
    *ucp = (*ucp + word.len() - 1).min(ue);
    let mut k = 1usize;
    while uc < ue && k < word.len() {
        let c = s[uc];
        uc += 1;
        if c != word[k] {
            return false;
        }
        k += 1;
    }
    true
}

/// `json_parse`: one value. At level 0 the answer is 1 for JSON, 2 for
/// newline-delimited JSON, 0 for neither.
fn json_parse(s: &[u8], ucp: &mut usize, st: &mut [usize; JSON_MAX], lvl: usize) -> bool {
    json_parse_level(s, ucp, st, lvl) != 0
}

fn json_parse_level(s: &[u8], ucp: &mut usize, st: &mut [usize; JSON_MAX], lvl: usize) -> i32 {
    let ue = s.len();
    let mut uc = json_skip_space(s, *ucp);
    let ouc = uc;
    let mut rv = false;
    if uc != ue {
        // Avoid recursion.
        if lvl > 500 {
            return 0;
        }
        let c = s[uc];
        uc += 1;
        let t = match c {
            b'"' => {
                rv = json_parse_string(s, &mut uc);
                JSON_STRING
            }
            b'[' => {
                rv = json_parse_array(s, &mut uc, st, lvl + 1);
                JSON_ARRAY
            }
            b'{' => {
                rv = json_parse_object(s, &mut uc, st, lvl + 1);
                JSON_OBJECT
            }
            b't' => {
                rv = json_parse_const(s, &mut uc, b"true");
                JSON_CONSTANT
            }
            b'f' => {
                rv = json_parse_const(s, &mut uc, b"false");
                JSON_CONSTANT
            }
            b'n' => {
                rv = json_parse_const(s, &mut uc, b"null");
                JSON_CONSTANT
            }
            _ => {
                uc -= 1;
                rv = json_parse_number(s, &mut uc);
                JSON_NUMBER
            }
        };
        if rv {
            st[t] += 1;
        }
        uc = json_skip_space(s, uc);
    }
    *ucp = uc;
    if lvl == 0 {
        if !rv {
            return 0;
        }
        let holds = st[JSON_ARRAYN] != 0 || st[JSON_OBJECT] != 0;
        if uc == ue {
            return i32::from(holds);
        }
        let first = s.get(ouc).copied().unwrap_or(0);
        if first == s[uc] && json_parse(s, &mut uc, st, 1) {
            return if st[JSON_ARRAYN] != 0 || st[JSON_OBJECT] != 0 { 2 } else { 0 };
        }
        return 0;
    }
    i32::from(rv)
}

/// `file_is_json`.
pub fn file_is_json(ms: &mut Ms, b: &Buffer<'_>) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let mut st = [0usize; JSON_MAX];
    let mut uc = 0usize;
    let jt = json_parse_level(b.fbuf, &mut uc, &mut st, 0);
    if jt == 0 {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    if mime != 0 {
        let t: &[u8] = if jt == 1 { b"json" } else { b"x-ndjson" };
        if ms.printf(b"application/%s", &[Arg::Str(t)]) == -1 {
            return -1;
        }
        return 1;
    }
    let p: &[u8] = if jt == 1 { b"" } else { b"New Line Delimited " };
    if ms.printf(b"%sJSON text data", &[Arg::Str(p)]) == -1 {
        return -1;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jt(s: &str) -> i32 {
        let mut st = [0usize; JSON_MAX];
        let mut uc = 0;
        json_parse_level(s.as_bytes(), &mut uc, &mut st, 0)
    }

    #[test]
    fn json_and_ndjson_as_upstream_tells_them() {
        assert_eq!(jt("{\"a\": [1, 2.5e3, true, null]}"), 1);
        assert_eq!(jt("[]"), 1);
        assert_eq!(jt("{\"a\":1}\n{\"b\":2}\n"), 2);
        // A bare value holds no object or array.
        assert_eq!(jt("\"x\""), 0);
        assert_eq!(jt("42"), 0);
        assert_eq!(jt("{\"a\":}"), 0);
        // A constant cut short by the end of the buffer still counts.
        assert_eq!(jt("[tr"), 0);
        assert_eq!(jt("[1]\n[2"), 0);
    }
}
