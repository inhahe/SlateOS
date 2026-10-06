//! A small, strict JSON reader (RFC 8259), for Cloudflare's answers.
//!
//! An answer is read whole into a [`Value`]. Numbers are kept as the text
//! they were written as -- the one number this crate reads is an error code,
//! and a float would round a large one. Nesting deeper than [`MAX_DEPTH`] is
//! refused rather than followed, so a hostile answer cannot exhaust the
//! stack.

/// How deeply arrays and objects may nest. Cloudflare's answers nest four.
pub const MAX_DEPTH: usize = 64;

/// A JSON value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as written.
    Number(String),
    /// A string, escapes resolved.
    Str(String),
    /// An array.
    Array(Vec<Value>),
    /// An object, its members in the order written. A repeated name keeps
    /// both; [`Value::get`] answers the first.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The member `name` of an object; `None` for anything else.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string this is, if it is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The boolean this is, if it is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The elements of the array this is; empty for anything else.
    #[must_use]
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(items) => items,
            _ => &[],
        }
    }

    /// The number this is, as written, if it is one.
    #[must_use]
    pub fn as_number(&self) -> Option<&str> {
        match self {
            Value::Number(n) => Some(n),
            _ => None,
        }
    }
}

/// Why a text is not JSON: the byte offset reached, and what was wrong there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Byte offset into the text.
    pub at: usize,
    /// What was expected or found.
    pub what: &'static str,
}

/// Read `text` as one JSON value, with nothing but white space around it.
///
/// # Errors
///
/// [`Error`] when it is not.
pub fn parse(text: &str) -> Result<Value, Error> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i < p.s.len() {
        return Err(p.err("text after the value"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &'static str) -> Error {
        Error { at: self.i, what }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn bump(&mut self) {
        self.i = self.i.saturating_add(1);
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    fn literal(&mut self, word: &'static [u8], v: Value) -> Result<Value, Error> {
        let end = self.i.saturating_add(word.len());
        if self.s.get(self.i..end) == Some(word) {
            self.i = end;
            Ok(v)
        } else {
            Err(self.err("an unknown word"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("a value")),
            None => Err(self.err("a value, not the end")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, Error> {
        if depth >= MAX_DEPTH {
            return Err(self.err("nesting this deep"));
        }
        self.bump(); // {
        let mut members = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Value::Object(members));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("a member name"));
            }
            let name = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("':' after a member name"));
            }
            self.bump();
            self.ws();
            let v = self.value(depth.saturating_add(1))?;
            members.push((name, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b'}') => {
                    self.bump();
                    return Ok(Value::Object(members));
                }
                _ => return Err(self.err("',' or '}' in an object")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, Error> {
        if depth >= MAX_DEPTH {
            return Err(self.err("nesting this deep"));
        }
        self.bump(); // [
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Value::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth.saturating_add(1))?);
            self.ws();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b']') => {
                    self.bump();
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.err("',' or ']' in an array")),
            }
        }
    }

    /// A number, checked against RFC 8259's grammar and kept as written.
    fn number(&mut self) -> Result<Value, Error> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        match self.peek() {
            Some(b'0') => self.bump(),
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.bump();
                }
            }
            _ => return Err(self.err("a digit")),
        }
        if self.peek() == Some(b'.') {
            self.bump();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("a digit after '.'"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("a digit in an exponent"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        let text = self
            .s
            .get(start..self.i)
            .ok_or_else(|| self.err("a number"))?;
        // The grammar above admits ASCII alone.
        let text = core::str::from_utf8(text).map_err(|_| self.err("a number"))?;
        Ok(Value::Number(text.to_owned()))
    }

    /// Four hexadecimal digits of a `\u` escape.
    fn hex4(&mut self) -> Result<u32, Error> {
        let mut n = 0u32;
        for _ in 0..4 {
            let d = match self.peek() {
                Some(c @ b'0'..=b'9') => c.wrapping_sub(b'0'),
                Some(c @ b'a'..=b'f') => c.wrapping_sub(b'a').wrapping_add(10),
                Some(c @ b'A'..=b'F') => c.wrapping_sub(b'A').wrapping_add(10),
                _ => return Err(self.err("four hexadecimal digits after \\u")),
            };
            n = (n << 4) | u32::from(d);
            self.bump();
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String, Error> {
        self.bump(); // "
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.err("the end of a string")),
                Some(b'"') => {
                    self.bump();
                    return String::from_utf8(out).map_err(|_| self.err("UTF-8 in a string"));
                }
                Some(b'\\') => {
                    self.bump();
                    let c = match self.peek() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.bump();
                            let hi = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&hi) {
                                // A high surrogate: its low half must follow.
                                if self.peek() != Some(b'\\') {
                                    return Err(self.err("the low half of a surrogate pair"));
                                }
                                self.bump();
                                if self.peek() != Some(b'u') {
                                    return Err(self.err("the low half of a surrogate pair"));
                                }
                                self.bump();
                                let lo = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&lo) {
                                    return Err(self.err("the low half of a surrogate pair"));
                                }
                                0x10000u32
                                    .wrapping_add((hi.wrapping_sub(0xd800)) << 10)
                                    .wrapping_add(lo.wrapping_sub(0xdc00))
                            } else {
                                hi
                            };
                            let c = char::from_u32(code).ok_or_else(|| self.err("a character"))?;
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                            continue;
                        }
                        _ => return Err(self.err("an escape")),
                    };
                    self.bump();
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                Some(c) if c < 0x20 => return Err(self.err("no control character in a string")),
                Some(c) => {
                    out.push(c);
                    self.bump();
                }
            }
        }
    }
}

#[cfg(test)]
// A test states what it expects by failing loudly when it is not so.
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_cloudflare_answers() {
        let v = parse(
            r#"{"result":[{"id":"023e105f4ecef8ad9ca31a8372d0c353","name":"example.com","content":"198.51.100.4","proxied":false,"ttl":1}],"success":true,"errors":[],"messages":[],"result_info":{"page":1,"per_page":20,"count":1,"total_count":1}}"#,
        )
        .expect("JSON");
        assert_eq!(v.get("success").and_then(Value::as_bool), Some(true));
        let rec = v
            .get("result")
            .map(Value::items)
            .and_then(|r| r.first())
            .expect("a record");
        assert_eq!(
            rec.get("id").and_then(Value::as_str),
            Some("023e105f4ecef8ad9ca31a8372d0c353")
        );
        assert_eq!(
            rec.get("content").and_then(Value::as_str),
            Some("198.51.100.4")
        );
        assert_eq!(rec.get("ttl").and_then(Value::as_number), Some("1"));
        assert_eq!(
            v.get("errors").map(Value::items).map(<[Value]>::len),
            Some(0)
        );
        assert_eq!(v.get("nothing"), None);
    }

    #[test]
    fn strings_resolve_every_escape() {
        let v = parse(r#""a\"b\\c\/d\b\f\n\r\t\u00e9\ud83d\ude00""#).expect("JSON");
        assert_eq!(v.as_str(), Some("a\"b\\c/d\u{8}\u{c}\n\r\té\u{1f600}"));
    }

    #[test]
    fn numbers_are_kept_as_written() {
        for n in [
            "0",
            "-0",
            "12",
            "-3.25",
            "1e10",
            "6.02E+23",
            "1.5e-3",
            "10000000000000000000001",
        ] {
            assert_eq!(parse(n), Ok(Value::Number(n.to_owned())), "{n}");
        }
        for bad in ["01", "-", "1.", ".5", "1e", "+1", "1e+"] {
            assert!(parse(bad).is_err(), "{bad} is not JSON");
        }
    }

    #[test]
    fn what_is_not_json_is_refused() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "{\"a\" 1}",
            "{a:1}",
            "\"unterminated",
            "\"bad \\x escape\"",
            "\"\\ud83d alone\"",
            "\"\\udc00\"",
            "tru",
            "nul",
            "[1] 2",
            "\"tab\there\"",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} is not JSON");
        }
        assert_eq!(
            parse(" [ true , false , null ] "),
            Ok(Value::Array(vec![
                Value::Bool(true),
                Value::Bool(false),
                Value::Null
            ]))
        );
    }

    #[test]
    fn nesting_is_bounded() {
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(parse(&deep).map_err(|e| e.what), Err("nesting this deep"));
        let fine = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&fine).is_ok());
    }
}
