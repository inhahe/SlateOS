//! Cutting a generated `parser.c` into tokens.
//!
//! Only as much of C as the tree-sitter generator writes: identifiers,
//! integer, character and string literals, punctuation, comments, and
//! preprocessor lines (kept whole, as [`Tok::Directive`], because the
//! `#define`s carry the grammar's counts).

use crate::Error;

/// One token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tok<'a> {
    /// An identifier or keyword.
    Ident(&'a str),
    /// An integer literal, its suffix dropped.
    Int(i64),
    /// A character literal's value.
    Char(i64),
    /// A string literal's bytes, its escapes decoded.
    Str(Vec<u8>),
    /// An operator or a bracket.
    Punct(&'static str),
    /// A preprocessor line, without its `#`, continuations joined.
    Directive(String),
}

/// Punctuation, longest first so `&&` is not read as two `&`.
const PUNCTUATION: &[&str] = &[
    "...", "->", "&&", "||", "==", "!=", "<=", ">=", "<<", ">>", "++", "--", "{", "}", "[", "]",
    "(", ")", "<", ">", "=", "!", "&", "|", ",", ";", ".", "*", "+", "-", "/", "%", "?", ":", "~",
    "^",
];

/// `source` as tokens, with the line each starts on (from 1) for errors.
pub(crate) fn tokenize(source: &str) -> Result<Vec<(Tok<'_>, usize)>, Error> {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(source.len() / 4);
    let mut i = 0;
    let mut line = 1;
    // Whether only blanks have been seen since the last newline: where a `#`
    // begins a directive.
    let mut at_line_start = true;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'\n' => {
                line += 1;
                at_line_start = true;
                i += 1;
            }
            b' ' | b'\t' | b'\r' | b'\x0c' | b'\x0b' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while bytes.get(i).is_some_and(|&c| c != b'\n') {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let start_line = line;
                i += 2;
                loop {
                    match bytes.get(i) {
                        None => return Err(Error::at(start_line, "a comment never ends")),
                        Some(b'*') if bytes.get(i + 1) == Some(&b'/') => {
                            i += 2;
                            break;
                        }
                        Some(b'\n') => {
                            line += 1;
                            i += 1;
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            b'#' if at_line_start => {
                let start_line = line;
                let mut text = String::new();
                i += 1;
                while let Some(&c) = bytes.get(i) {
                    if c == b'\\' && bytes.get(i + 1) == Some(&b'\n') {
                        text.push(' ');
                        line += 1;
                        i += 2;
                    } else if c == b'\\' && bytes.get(i + 1) == Some(&b'\r') {
                        text.push(' ');
                        line += 1;
                        i += 3;
                    } else if c == b'\n' {
                        break;
                    } else {
                        text.push(char::from(c));
                        i += 1;
                    }
                }
                out.push((Tok::Directive(text.trim().to_owned()), start_line));
            }
            b'"' => {
                at_line_start = false;
                let (value, next) = string_literal(bytes, i + 1, line)?;
                out.push((Tok::Str(value), line));
                i = next;
            }
            b'\'' => {
                at_line_start = false;
                let (value, next) = char_literal(bytes, i + 1, line)?;
                out.push((Tok::Char(value), line));
                i = next;
            }
            b'0'..=b'9' => {
                at_line_start = false;
                let (value, next) = number(source, i, line)?;
                out.push((Tok::Int(value), line));
                i = next;
            }
            c if c == b'_' || c.is_ascii_alphabetic() => {
                at_line_start = false;
                let start = i;
                while bytes
                    .get(i)
                    .is_some_and(|&c| c == b'_' || c.is_ascii_alphanumeric())
                {
                    i += 1;
                }
                let word = source
                    .get(start..i)
                    .ok_or_else(|| Error::at(line, "an identifier is not text"))?;
                out.push((Tok::Ident(word), line));
            }
            _ => {
                at_line_start = false;
                let rest = bytes.get(i..).unwrap_or_default();
                let Some(p) = PUNCTUATION.iter().find(|p| rest.starts_with(p.as_bytes())) else {
                    return Err(Error::at(
                        line,
                        format!("a byte no C token starts with: {b:#04x}"),
                    ));
                };
                out.push((Tok::Punct(p), line));
                i += p.len();
            }
        }
    }
    Ok(out)
}

/// An integer literal starting at `start`: decimal, hex or octal, with any
/// `u`/`l` suffix. Returns the value and where the literal ends.
fn number(source: &str, start: usize, line: usize) -> Result<(i64, usize), Error> {
    let bytes = source.as_bytes();
    let mut end = start;
    while bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
        end += 1;
    }
    let text = source
        .get(start..end)
        .ok_or_else(|| Error::at(line, "a number is not text"))?;
    let digits = text.trim_end_matches(['u', 'U', 'l', 'L']);
    let parsed = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16)
    } else if digits.len() > 1 && digits.starts_with('0') {
        i64::from_str_radix(digits.get(1..).unwrap_or_default(), 8)
    } else {
        digits.parse()
    };
    parsed
        .map(|value| (value, end))
        .map_err(|_| Error::at(line, format!("not a number: {text}")))
}

/// One character of a literal, from `i`: its value and where it ends.
fn escaped(bytes: &[u8], i: usize, line: usize) -> Result<(i64, usize), Error> {
    let Some(&b) = bytes.get(i) else {
        return Err(Error::at(line, "a literal never ends"));
    };
    if b != b'\\' {
        // A character of the source: its UTF-8 bytes, one at a time, as C
        // reads them.
        return Ok((i64::from(b), i + 1));
    }
    let Some(&e) = bytes.get(i + 1) else {
        return Err(Error::at(line, "a literal never ends"));
    };
    let simple = match e {
        b'n' => Some(b'\n'),
        b't' => Some(b'\t'),
        b'r' => Some(b'\r'),
        b'0'..=b'7' => None,
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'f' => Some(0x0c),
        b'v' => Some(0x0b),
        b'\\' | b'\'' | b'"' | b'?' => Some(e),
        b'x' => None,
        _ => {
            return Err(Error::at(
                line,
                format!("an escape C has not got: \\{}", char::from(e)),
            ));
        }
    };
    if let Some(value) = simple {
        return Ok((i64::from(value), i + 2));
    }
    let (radix, first, most) = if e == b'x' {
        (16, i + 2, usize::MAX)
    } else {
        (8, i + 1, 3)
    };
    let mut end = first;
    while end - first < most
        && bytes
            .get(end)
            .is_some_and(|c| char::from(*c).is_digit(radix))
    {
        end += 1;
    }
    let digits = core::str::from_utf8(bytes.get(first..end).unwrap_or_default()).unwrap_or("");
    let value = i64::from_str_radix(digits, radix)
        .map_err(|_| Error::at(line, "an escape with no digits"))?;
    Ok((value, end))
}

/// A string literal's bytes, from just after its opening quote.
fn string_literal(bytes: &[u8], mut i: usize, line: usize) -> Result<(Vec<u8>, usize), Error> {
    let mut out = Vec::new();
    loop {
        match bytes.get(i) {
            None | Some(b'\n') => return Err(Error::at(line, "a string never ends")),
            Some(b'"') => return Ok((out, i + 1)),
            Some(_) => {
                let (value, next) = escaped(bytes, i, line)?;
                out.push(
                    u8::try_from(value)
                        .map_err(|_| Error::at(line, "a string escape past a byte"))?,
                );
                i = next;
            }
        }
    }
}

/// A character literal's value, from just after its opening quote.
fn char_literal(bytes: &[u8], i: usize, line: usize) -> Result<(i64, usize), Error> {
    let (value, next) = escaped(bytes, i, line)?;
    if bytes.get(next) != Some(&b'\'') {
        return Err(Error::at(
            line,
            "a character literal of more than one character",
        ));
    }
    Ok((value, next + 1))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn toks(source: &str) -> Vec<Tok<'_>> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    /// **Every kind of token the generator writes, and what it means.**
    #[test]
    fn the_generators_tokens_are_read() {
        assert_eq!(
            toks("if (lookahead == '\\t' || 0x2028 <= lookahead) ADVANCE(12);"),
            [
                Tok::Ident("if"),
                Tok::Punct("("),
                Tok::Ident("lookahead"),
                Tok::Punct("=="),
                Tok::Char(9),
                Tok::Punct("||"),
                Tok::Int(0x2028),
                Tok::Punct("<="),
                Tok::Ident("lookahead"),
                Tok::Punct(")"),
                Tok::Ident("ADVANCE"),
                Tok::Punct("("),
                Tok::Int(12),
                Tok::Punct(")"),
                Tok::Punct(";"),
            ]
        );
        assert_eq!(
            toks("[1] = \"a\\\"b\\\\\\n\", '\\'', '\\\\', 017, 65535u /* x */ // y\n"),
            [
                Tok::Punct("["),
                Tok::Int(1),
                Tok::Punct("]"),
                Tok::Punct("="),
                Tok::Str(b"a\"b\\\n".to_vec()),
                Tok::Punct(","),
                Tok::Char(i64::from(b'\'')),
                Tok::Punct(","),
                Tok::Char(i64::from(b'\\')),
                Tok::Punct(","),
                Tok::Int(0o17),
                Tok::Punct(","),
                Tok::Int(65535),
            ]
        );
    }

    /// **A directive is one token, continuations and all, and only at the
    /// start of a line.**
    #[test]
    fn directives_are_whole_lines() {
        let t = tokenize("#define A 1\n  #define B \\\n 2\nx # y").unwrap_err();
        assert!(t.to_string().contains("line 4"), "{t}");
        assert_eq!(
            toks("#define A 1\n  #define B \\\n 2\nx"),
            [
                Tok::Directive("define A 1".to_owned()),
                Tok::Directive("define B   2".to_owned()),
                Tok::Ident("x"),
            ]
        );
    }

    /// **Unfinished literals and unknown escapes are refused**, with their
    /// line.
    #[test]
    fn malformed_literals_are_refused() {
        for bad in ["\"abc", "'ab'", "'\\q'", "/* open", "\"\\x\""] {
            assert!(tokenize(bad).is_err(), "{bad}");
        }
        assert_eq!(toks("\"\\x41\\101\""), [Tok::Str(b"AA".to_vec())]);
    }
}
