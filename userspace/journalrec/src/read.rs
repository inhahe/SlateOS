//! Reading a record: the other half of the one spelling.
//!
//! Until 2026-10-07 `journalctl` and `syslogd`'s reading commands each had a
//! parser of their own, and they had drifted apart. `journalctl`'s was fixed
//! on 2026-09-26 to decode UTF-8 and every JSON escape, and to read a field
//! that is not text as its bytes (design-decisions §1063); `syslogd`'s still
//! turned each non-ASCII character into mojibake (`café` came out as
//! `cafÃ©`), and dropped every record holding a byte array -- the way its own
//! daemon writes a message that is not text. Now there is one, here, beside
//! the writer whose output it reads.
//!
//! It reads the flat objects the writers here produce: string, scalar and
//! byte-array values, nothing nested. A line that is not such an object is
//! not a record, and what that costs is the caller's to decide -- one line,
//! never a whole file.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::{escape, json_value};

/// One field's value, as the record wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A JSON string, decoded.
    Text(String),
    /// A JSON array of byte values: a field that is not text, as `syslogd`
    /// writes one and as `journalctl -o json` prints one on Linux
    /// (design-decisions §1063).
    Bytes(Vec<u8>),
    /// Anything else -- a number, `true`, `false`, `null`, or an array that
    /// is not bytes -- as it was written.
    Scalar(String),
}

impl Value {
    /// The value's bytes: a string's UTF-8, an array's bytes, a scalar's
    /// text.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Value::Text(s) | Value::Scalar(s) => s.as_bytes(),
            Value::Bytes(b) => b,
        }
    }

    /// The value as text, when it is: a string or a scalar.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(s) | Value::Scalar(s) => Some(s),
            Value::Bytes(_) => None,
        }
    }

    /// The value as JSON: a string escaped by [`escape`], bytes as
    /// [`json_value`] spells them, a scalar as it was written.
    #[must_use]
    pub fn to_json(&self) -> String {
        match self {
            Value::Text(s) => format!("\"{}\"", escape(s)),
            Value::Bytes(b) => json_value(b),
            Value::Scalar(s) => s.clone(),
        }
    }
}

/// A record line's fields, by name.
///
/// The line is a flat JSON object, whitespace around it allowed, whose
/// values are strings, byte arrays and scalars. `None` when it is not an
/// object at all. An object that goes wrong part of the way through gives the
/// fields before the fault: a record whose writer died mid-field still shows
/// what it got to.
#[must_use]
pub fn parse_object(line: &str) -> Option<BTreeMap<String, Value>> {
    let inner = line.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut at = Cursor {
        text: inner,
        pos: 0,
    };
    let mut fields = BTreeMap::new();
    loop {
        at.skip(|b| matches!(b, b' ' | b',' | b'\t' | b'\n' | b'\r'));
        if at.done() {
            break;
        }
        let Some(key) = at.string() else {
            break;
        };
        at.skip(|b| b != b':');
        if at.done() {
            break;
        }
        at.bump(); // the ':'
        at.skip(|b| matches!(b, b' ' | b'\t'));
        let value = match at.peek() {
            None => break,
            Some(b'"') => match at.string() {
                Some(text) => Value::Text(text),
                None => break,
            },
            Some(b'[') => {
                // To the closing bracket: a flat array, as every writer here
                // spells one, so no bracket is nested inside it.
                let start = at.pos;
                at.skip(|b| b != b']');
                at.bump(); // the ']', when there is one
                let text = at.since(start);
                byte_array(text).map_or_else(|| Value::Scalar(text.to_string()), Value::Bytes)
            }
            Some(_) => {
                let start = at.pos;
                at.skip(|b| b != b',' && b != b'}');
                Value::Scalar(at.since(start).trim().to_string())
            }
        };
        fields.insert(key, value);
    }
    Some(fields)
}

/// Where a parse has got to in a record's text. Every step is checked, so a
/// malformed line ends the parse instead of reading past its end.
struct Cursor<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn done(&self) -> bool {
        self.pos >= self.text.len()
    }

    /// One byte on, never past the end.
    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1).min(self.text.len());
    }

    /// On past every byte `pass` accepts.
    fn skip(&mut self, pass: impl Fn(u8) -> bool) {
        while self.peek().is_some_and(&pass) {
            self.bump();
        }
    }

    /// The text from `start` to the cursor. Both are always at character
    /// boundaries -- every stop is an ASCII byte or the end -- so this is
    /// never cut short; `get` makes that a promise rather than a panic.
    fn since(&self, start: usize) -> &'a str {
        self.text.get(start..self.pos).unwrap_or_default()
    }

    /// The JSON string at the cursor, decoded, with the cursor moved past
    /// it; `None` when the cursor is not at one. An unterminated string is
    /// what it holds up to the end.
    fn string(&mut self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.bump();
        let mut out = String::new();
        // Where the current run of bytes needing no decoding began. A run
        // always ends at an ASCII byte -- a quote, a backslash -- or at the
        // end, so it is copied as the UTF-8 it already is. Pushing it byte by
        // byte `as char` turned every non-ASCII character into mojibake: a
        // record saying `café` was shown as `cafÃ©`.
        let mut run = self.pos;
        while let Some(b) = self.peek() {
            match b {
                b'"' => {
                    out.push_str(self.since(run));
                    self.bump();
                    return Some(out);
                }
                b'\\' => {
                    out.push_str(self.since(run));
                    let rest = self.text.as_bytes().get(self.pos..).unwrap_or_default();
                    let span = decode_escape(rest, &mut out);
                    self.pos = self.pos.saturating_add(span).min(self.text.len());
                    run = self.pos;
                }
                _ => self.bump(),
            }
        }
        out.push_str(self.since(run));
        Some(out)
    }
}

/// `[98,97,255]` as the bytes it lists, or `None` when it is not an array of
/// numbers each 0 to 255 -- kept then as written rather than half-read.
fn byte_array(text: &str) -> Option<Vec<u8>> {
    let body = text.strip_prefix('[')?.strip_suffix(']')?.trim();
    if body.is_empty() {
        return Some(Vec::new());
    }
    body.split(',')
        .map(|n| n.trim().parse::<u8>().ok())
        .collect()
}

/// Decode the escape at the start of `rest`, whose first byte is its
/// backslash, into `out`, and return how many bytes it spans.
///
/// Every escape JSON defines is decoded, `\uXXXX` included and a surrogate
/// pair as the one character it encodes. What cannot be decoded -- a lone
/// surrogate, a malformed `\u`, an escape JSON does not define -- is kept as
/// it was written, backslash and all: a log viewer that drops text it cannot
/// interpret shows a record that was never written.
fn decode_escape(rest: &[u8], out: &mut String) -> usize {
    let simple = match rest.get(1) {
        Some(b'"') => Some('"'),
        Some(b'\\') => Some('\\'),
        Some(b'/') => Some('/'),
        Some(b'b') => Some('\u{8}'),
        Some(b'f') => Some('\u{c}'),
        Some(b'n') => Some('\n'),
        Some(b'r') => Some('\r'),
        Some(b't') => Some('\t'),
        _ => None,
    };
    if let Some(c) = simple {
        out.push(c);
        return 2;
    }
    if rest.get(1) == Some(&b'u')
        && let Some(unit) = hex4(rest.get(2..6))
    {
        if (0xD800..0xDC00).contains(&unit) {
            if rest.get(6) == Some(&b'\\')
                && rest.get(7) == Some(&b'u')
                && let Some(low) = hex4(rest.get(8..12))
                && let Some(ch) = surrogate_pair(unit, low)
            {
                out.push(ch);
                return 12;
            }
        } else if let Some(ch) = char::from_u32(unit) {
            out.push(ch);
            return 6;
        }
    }
    // Kept as written: the backslash here, and whatever follows it copied by
    // the caller as ordinary text.
    out.push('\\');
    1
}

/// The character the UTF-16 surrogate pair `high`, `low` encodes, when they
/// are one.
fn surrogate_pair(high: u32, low: u32) -> Option<char> {
    let high = high.checked_sub(0xD800).filter(|&h| h < 0x400)?;
    let low = low.checked_sub(0xDC00).filter(|&l| l < 0x400)?;
    char::from_u32(
        0x1_0000_u32
            .checked_add(high.checked_shl(10)?)?
            .checked_add(low)?,
    )
}

/// Four hex digits as the number they spell; `None` for anything else,
/// including too few.
fn hex4(digits: Option<&[u8]>) -> Option<u32> {
    digits
        .filter(|d| d.len() == 4)?
        .iter()
        .try_fold(0u32, |acc, &d| {
            acc.checked_mul(16)?
                .checked_add(char::from(d).to_digit(16)?)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn fields(json: &str) -> BTreeMap<String, Value> {
        let parsed = parse_object(json);
        assert!(parsed.is_some(), "not an object: {json}");
        parsed.unwrap_or_default()
    }

    fn text<'a>(map: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
        map.get(key).and_then(Value::text)
    }

    /// One string decoded on its own, as a record's value would be.
    fn decoded(json: &str) -> Option<String> {
        Cursor { text: json, pos: 0 }.string()
    }

    #[test]
    fn an_object_gives_its_fields() {
        let map = fields(r#"{"ts":1000,"level":"info","msg":"hello"}"#);
        assert_eq!(map.get("ts"), Some(&Value::Scalar("1000".to_string())));
        assert_eq!(text(&map, "level"), Some("info"));
        assert_eq!(text(&map, "msg"), Some("hello"));
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn an_empty_object_has_no_fields() {
        assert!(fields("{}").is_empty());
        assert!(fields("  { }  ").is_empty());
    }

    #[test]
    fn what_is_not_an_object_is_not_a_record() {
        for line in ["not json", "[1,2,3]", "", "{", "}", "{\"a\":1"] {
            assert_eq!(parse_object(line), None, "{line:?}");
        }
    }

    #[test]
    fn escaped_strings_are_decoded() {
        let map = fields(r#"{"msg":"line1\nline2","path":"c:\\temp"}"#);
        assert_eq!(text(&map, "msg"), Some("line1\nline2"));
        assert_eq!(text(&map, "path"), Some("c:\\temp"));
        let map = fields(r#"{"msg":"hello \u0041 world"}"#);
        assert_eq!(text(&map, "msg"), Some("hello A world"));
    }

    /// A field that is not text is an array of its bytes, read as them; an
    /// array that is not bytes is kept as written, not half-read.
    #[test]
    fn a_byte_array_is_its_bytes() {
        let map = fields(r#"{"msg":[98,97,100,32,255],"list":[1,2,300],"none":[]}"#);
        assert_eq!(map.get("msg"), Some(&Value::Bytes(b"bad \xff".to_vec())));
        assert_eq!(map.get("msg").map(Value::text), Some(None));
        assert_eq!(
            map.get("list"),
            Some(&Value::Scalar("[1,2,300]".to_string()))
        );
        assert_eq!(map.get("none"), Some(&Value::Bytes(vec![])));
    }

    /// The array a field that is not text is written as reads back as the
    /// same bytes, and writes out the same again.
    #[test]
    fn bytes_round_trip_through_the_writer() {
        let raw = b"\x00a\xff\"";
        let line = format!("{{\"msg\":{}}}", json_value(raw));
        let map = fields(&line);
        assert_eq!(map.get("msg").map(Value::bytes), Some(&raw[..]));
        assert_eq!(map.get("msg").map(Value::to_json), Some(json_value(raw)));
        let text = Value::Text("a\"b\n".to_string());
        assert_eq!(text.to_json(), "\"a\\\"b\\n\"");
        assert_eq!(
            fields(&format!("{{\"m\":{}}}", text.to_json())).get("m"),
            Some(&text)
        );
    }

    /// Whitespace between the parts of an object, as a pretty-printer
    /// writes it, is no obstacle.
    #[test]
    fn whitespace_around_the_parts_is_allowed() {
        let map = fields("{\n  \"a\": \"x\",\n  \"b\":\t7 ,\r\n \"c\": [104,105]\n}");
        assert_eq!(text(&map, "a"), Some("x"));
        assert_eq!(text(&map, "b"), Some("7"));
        assert_eq!(map.get("c").map(Value::bytes), Some(&b"hi"[..]));
    }

    /// An object that goes wrong part of the way through keeps the fields
    /// before the fault.
    #[test]
    fn a_broken_object_keeps_the_fields_before_the_fault() {
        // The last brace is taken as the object's, so the string runs to the
        // end unterminated, and is kept as far as it goes.
        let map = fields(r#"{"ts":5,"msg":"he}"#);
        assert_eq!(map.get("ts"), Some(&Value::Scalar("5".to_string())));
        assert_eq!(text(&map, "msg"), Some("he"));
        // A key with no value, a key with no colon, and something other than
        // a string where a key belongs each end the fields there.
        for line in [
            r#"{"ts":5,"msg":}"#,
            r#"{"ts":5,"msg"}"#,
            r#"{"ts":5,7:"x"}"#,
        ] {
            let map = fields(line);
            assert_eq!(map.len(), 1, "{line}");
            assert_eq!(
                map.get("ts"),
                Some(&Value::Scalar("5".to_string())),
                "{line}"
            );
        }
    }

    /// Non-ASCII text used to come back as mojibake, one Latin-1 character
    /// per UTF-8 byte: `café` as `cafÃ©`.
    #[test]
    fn strings_decode_as_utf8() {
        assert_eq!(decoded("\"caf\u{e9}\"").as_deref(), Some("caf\u{e9}"));
        assert_eq!(
            decoded("\"\u{65e5}\u{672c}\"").as_deref(),
            Some("\u{65e5}\u{672c}")
        );
        let map = fields("{\"msg\":\"caf\u{e9}\",\"service\":\"\u{65e5}\"}");
        assert_eq!(text(&map, "msg"), Some("caf\u{e9}"));
        assert_eq!(text(&map, "service"), Some("\u{65e5}"));
    }

    #[test]
    fn every_escape_json_defines_is_decoded() {
        let cases = [
            (r#""\u00e9""#, "\u{e9}"),
            (r#""\ud83d\ude00""#, "\u{1f600}"),
            (r#""\b\f\n\r\t\/\\\"""#, "\u{8}\u{c}\n\r\t/\\\""),
        ];
        for (json, want) in cases {
            assert_eq!(decoded(json).as_deref(), Some(want), "{json}");
        }
    }

    /// What cannot be decoded is kept as written, not dropped.
    #[test]
    fn undecodable_escapes_are_kept_as_written() {
        let cases = [
            (r#""a\ud83dz""#, "a\\ud83dz"),
            (r#""a\ude00z""#, "a\\ude00z"),
            (r#""\ud83d\u0041""#, "\\ud83dA"),
            (r#""\x41""#, "\\x41"),
            (r#""\u12G4""#, "\\u12G4"),
            (r#""\u00e""#, "\\u00e"),
            (r#""\u+0e9""#, "\\u+0e9"),
            (r#""end\"#, "end\\"),
        ];
        for (json, want) in cases {
            assert_eq!(decoded(json).as_deref(), Some(want), "{json}");
        }
    }

    /// `\u` followed by multi-byte characters once sliced the `&str` inside
    /// one of them, which panics.
    #[test]
    fn a_multibyte_character_after_a_short_u_escape_does_not_panic() {
        assert_eq!(
            decoded("\"\\u\u{e9}\u{e9}\u{e9}\"").as_deref(),
            Some("\\u\u{e9}\u{e9}\u{e9}")
        );
        assert_eq!(decoded("\"\\\u{e9}\"").as_deref(), Some("\\\u{e9}"));
    }

    /// An unterminated string is what it holds up to the end.
    #[test]
    fn an_unterminated_string_is_kept() {
        assert_eq!(decoded("\"abc").as_deref(), Some("abc"));
        assert_eq!(decoded("abc"), None);
        assert_eq!(decoded(""), None);
    }

    #[test]
    fn surrogate_pairs_need_both_halves_in_range() {
        assert_eq!(surrogate_pair(0xD83D, 0xDE00), Some('\u{1f600}'));
        assert_eq!(surrogate_pair(0xD800, 0xDC00), Some('\u{10000}'));
        assert_eq!(surrogate_pair(0xDBFF, 0xDFFF), Some('\u{10ffff}'));
        assert_eq!(surrogate_pair(0xDC00, 0xDC00), None);
        assert_eq!(surrogate_pair(0xD800, 0xE000), None);
        assert_eq!(surrogate_pair(0x41, 0xDC00), None);
    }

    #[test]
    fn hex4_wants_exactly_four_digits() {
        let hex = |digits: &[u8]| hex4(Some(digits));
        assert_eq!(hex(b"00e9"), Some(0xe9));
        assert_eq!(hex(b"FFFF"), Some(0xffff));
        assert_eq!(hex(b"00e"), None);
        assert_eq!(hex(b"00e9a"), None);
        assert_eq!(hex(b"+0e9"), None);
        assert_eq!(hex(b"\xc3\xa9ab"), None);
        assert_eq!(hex4(None), None);
    }
}
