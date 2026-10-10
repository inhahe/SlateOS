//! The applications' JSON: a value tree ([`JsonValue`]), a parser
//! ([`json_parse`]) and a printer ([`json_pretty`], and `Display`).
//!
//! It began as the backup tool's, moved into `apps/snapstore` on 2026-09-27 --
//! the JSON a snapshot store's manifests are written in -- and out of it on
//! 2026-10-09, when the weather app needed to read a forecast service's
//! replies. A second hand-written reader for that would have been the fifth in
//! `apps/` (the snapshot store's, the JSON viewer's, the kanban board's and the
//! log viewer's flat one were the four); two readers of one format disagree
//! somewhere, and each one's tests can only see it agreeing with itself.
//! `snapstore::json` re-exports this crate, so the stores read exactly as they
//! did.
//!
//! # Untrusted input
//!
//! A reply from the network is written by whoever answers -- over plain HTTP,
//! by anyone on the path. So the parser is bounded: nesting deeper than
//! [`MAX_DEPTH`] is an error, not a recursion until the stack runs out, and
//! every slice is taken with `get`, so no input panics it. What it accepts
//! beyond RFC 8259 it accepts on purpose, being a reader of files people
//! edit: an unknown escape is kept as written rather than dropped, and a lone
//! surrogate escape reads as U+FFFD.

use std::fmt;

/// How deeply arrays and objects may nest. Deeper is refused: the parser is
/// recursive, and nesting is the one thing in a document that costs stack
/// rather than heap. No document a program here reads comes near it.
pub const MAX_DEPTH: usize = 128;

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as an `f64`, which every JSON number is read as.
    Number(f64),
    /// A string.
    Str(String),
    /// An array.
    Array(Vec<JsonValue>),
    /// An object, its members in the order written.
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// The string, if this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The number, if this is a whole one a `u64` holds exactly.
    ///
    /// It was `*n as u64`, which reads -5 as 0 and 420.7 as 420: a corrupt
    /// size came back as a plausible one, and a permission that is not a
    /// whole number was applied as the nearest one below. `None` instead,
    /// so the reader can refuse what nobody wrote.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        // 2^53: past it, not every whole number is an f64, so a value read
        // there may not be the one written.
        const EXACT: f64 = 9_007_199_254_740_992.0;
        match self {
            #[allow(
                clippy::float_cmp,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "whether the value is exactly whole is the question asked; the cast is of a whole number in 0..=2^53, which a u64 holds"
            )]
            JsonValue::Number(n)
                if n.is_finite() && *n >= 0.0 && n.trunc() == *n && *n <= EXACT =>
            {
                Some(*n as u64)
            }
            _ => None,
        }
    }

    /// The number, if this is one -- whole or not, finite as JSON's are.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) if n.is_finite() => Some(*n),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The array's elements, if this is one.
    #[must_use]
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The member named `key`, if this is an object that has one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(entries) => {
                for (k, v) in entries {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            JsonValue::Number(n) => {
                // The exact comparison is the point, and clippy's suggested
                // epsilon would be a bug: this asks "is this float an integer,
                // so I may print it without a decimal point?", and only an
                // exact round-trip answers it. Within a margin of error,
                // 3.0000001 would print as `3` and the manifest would claim a
                // file size, mtime or block count that was never measured.
                // `as u64` saturates rather than wrapping, so a value too large
                // for u64 round-trips to u64::MAX-as-f64, compares unequal, and
                // correctly takes the float branch; NaN fails every comparison
                // and does the same.
                #[allow(
                    clippy::float_cmp,
                    reason = "exactness is the question being asked; see comment"
                )]
                let integral = *n == (*n as u64) as f64 && *n >= 0.0;
                if integral {
                    write!(f, "{}", *n as u64)
                } else {
                    write!(f, "{n}")
                }
            }
            JsonValue::Str(s) => write_string(f, s),
            JsonValue::Array(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            JsonValue::Object(entries) => {
                write!(f, "{{")?;
                for (i, (key, val)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write_string(f, key)?;
                    write!(f, ":{val}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// `s` as a JSON string literal: quoted, with `"`, `\` and the control
/// characters escaped. Object keys go through here as well as string values:
/// a key was written bare between quotes, so one holding a `"` made a
/// document no reader could read back.
fn write_string(f: &mut impl fmt::Write, s: &str) -> fmt::Result {
    f.write_char('"')?;
    for ch in s.chars() {
        match ch {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if c < '\x20' => write!(f, "\\u{:04x}", u32::from(c))?,
            c => f.write_char(c)?,
        }
    }
    f.write_char('"')
}

/// Pretty-print JSON with indentation.
#[must_use]
pub fn json_pretty(value: &JsonValue, indent: usize) -> String {
    let mut out = String::new();
    json_pretty_inner(value, indent, 0, &mut out);
    out
}

fn json_pretty_inner(value: &JsonValue, indent: usize, depth: usize, out: &mut String) {
    // Saturating because `depth` is recursion depth over attacker-shaped input:
    // a manifest nested a few thousand deep would overflow the multiplication
    // long before it produced a line anyone would read.
    let prefix = " ".repeat(indent.saturating_mul(depth));
    let inner_depth = depth.saturating_add(1);
    let inner_prefix = " ".repeat(indent.saturating_mul(inner_depth));

    match value {
        // The separator is written *before* every entry but the first rather
        // than after every entry but the last. Both produce the same bytes, but
        // the leading form needs no lookahead to the container's length, so
        // there is no `i + 1` to compare against it.
        JsonValue::Object(entries) if !entries.is_empty() => {
            out.push_str("{\n");
            for (i, (key, val)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&inner_prefix);
                // Writing to a `String` cannot fail.
                let _infallible = write_string(out, key);
                out.push_str(": ");
                json_pretty_inner(val, indent, inner_depth, out);
            }
            out.push('\n');
            out.push_str(&prefix);
            out.push('}');
        }
        JsonValue::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&inner_prefix);
                json_pretty_inner(item, indent, inner_depth, out);
            }
            out.push('\n');
            out.push_str(&prefix);
            out.push(']');
        }
        _ => {
            out.push_str(&value.to_string());
        }
    }
}

/// Parse a JSON string into a JsonValue.
///
/// # Errors
///
/// Says what is wrong with text that is not JSON.
pub fn json_parse(input: &str) -> Result<JsonValue, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty input".to_string());
    }
    let (val, rest) = parse_value(trimmed, 0)?;
    if !rest.trim().is_empty() {
        // Take twenty *characters*, not twenty bytes. `&rest[..20]` panics if
        // byte 20 lands inside a multi-byte character, and trailing garbage is
        // precisely where a stray non-ASCII byte turns up — so the diagnostic
        // for a corrupt manifest would itself have been the crash.
        let shown: String = rest.chars().take(20).collect();
        return Err(format!("trailing characters: {shown:?}"));
    }
    Ok(val)
}

/// A value, `depth` arrays and objects deep.
fn parse_value(input: &str, depth: usize) -> Result<(JsonValue, &str), String> {
    let s = input.trim_start();
    let Some(&lead) = s.as_bytes().first() else {
        return Err("unexpected end of input".to_string());
    };
    match lead {
        b'"' => parse_string(s),
        b'{' => parse_object(s, depth),
        b'[' => parse_array(s, depth),
        b't' | b'f' => parse_bool(s),
        b'n' => parse_null(s),
        b'-' | b'0'..=b'9' => parse_number(s),
        c => Err(format!("unexpected character: {}", c as char)),
    }
}

/// Parse a JSON string literal, returning the decoded value and the remaining
/// input.
///
/// Literal stretches are copied out as whole `&str` slices rather than a byte
/// at a time. This matters more here than in most JSON readers: the strings in
/// a manifest are **file paths**, and pushing `byte as char` reads each UTF-8
/// byte as the Latin-1 scalar of that value — so a manifest listing
/// `写真/2024.jpg` read back as a mojibake path that no longer names any file,
/// and restore and verify both reported it missing.
fn parse_string(input: &str) -> Result<(JsonValue, &str), String> {
    if !input.starts_with('"') {
        return Err("expected '\"'".to_string());
    }
    let bytes = input.as_bytes();
    let mut result = String::new();
    let mut i = 1;
    // Start of the current run of literal (unescaped) text.
    let mut run_start = i;
    while let Some(&b) = bytes.get(i) {
        // `"` and `\` are ASCII, and an ASCII byte can never occur inside a
        // multi-byte UTF-8 sequence, so `i` is always on a character boundary
        // where a run is cut.
        if b == b'"' {
            result.push_str(input.get(run_start..i).ok_or("string cut mid-character")?);
            let rest = input
                .get(i.saturating_add(1)..)
                .ok_or("string cut mid-character")?;
            return Ok((JsonValue::Str(result), rest));
        }
        if b != b'\\' {
            i = i.saturating_add(1);
            continue;
        }
        result.push_str(input.get(run_start..i).ok_or("string cut mid-character")?);
        let after = i.saturating_add(1);
        // Take a whole character: an unknown escape may be followed by a
        // multi-byte one, and consuming a single byte of it would both corrupt
        // it and strand the scan inside a UTF-8 sequence.
        let esc = input
            .get(after..)
            .and_then(|s| s.chars().next())
            .ok_or("unexpected end in string escape")?;
        let mut next = after.saturating_add(esc.len_utf8());
        match esc {
            '"' => result.push('"'),
            '\\' => result.push('\\'),
            '/' => result.push('/'),
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            'b' => result.push('\u{08}'),
            'f' => result.push('\u{0c}'),
            'u' => {
                let (c, after_escape) = parse_unicode_escape(input, next)?;
                result.push(c);
                next = after_escape;
            }
            other => {
                // Unknown escape: keep it verbatim rather than silently
                // dropping the backslash out of a path.
                result.push('\\');
                result.push(other);
            }
        }
        i = next;
        run_start = i;
    }
    Err("unterminated string".to_string())
}

/// Decode a `\u` escape whose four hex digits begin at `start` (just past the
/// `u`), returning the character and the offset just past the escape.
///
/// A leading surrogate is combined with a following `\uXXXX` trailing
/// surrogate, which is how JSON spells anything outside the BMP.
fn parse_unicode_escape(input: &str, start: usize) -> Result<(char, usize), String> {
    let (hi, after_hi) = parse_hex4(input, start)?;
    let bytes = input.as_bytes();
    if (0xD800..0xDC00).contains(&hi)
        && bytes.get(after_hi).copied() == Some(b'\\')
        && bytes.get(after_hi.saturating_add(1)).copied() == Some(b'u')
        && let Ok((lo, after_lo)) = parse_hex4(input, after_hi.saturating_add(2))
        && (0xDC00..0xE000).contains(&lo)
        // Bounded by the two range checks above: at most 0x10000 + 0xFFC00 +
        // 0x3FF = 0x10FFFF, so neither the shift nor the sums can overflow.
        && let Some(c) = char::from_u32(
            0x1_0000_u32
                .saturating_add(hi.saturating_sub(0xD800) << 10)
                .saturating_add(lo.saturating_sub(0xDC00)),
        )
    {
        return Ok((c, after_lo));
    }
    // A lone surrogate has no scalar value. The old code dropped it silently,
    // so an escaped emoji in a path simply vanished from the manifest; U+FFFD
    // at least leaves the loss visible.
    Ok((char::from_u32(hi).unwrap_or('\u{FFFD}'), after_hi))
}

/// Read exactly four ASCII hex digits at `start`.
fn parse_hex4(input: &str, start: usize) -> Result<(u32, usize), String> {
    let end = start.saturating_add(4);
    // `get`, not a slice: the old code sliced these four bytes blindly, so a
    // `\u` followed by multi-byte text (`"\u日本"` cuts at byte 7, inside 本)
    // panicked while merely reading a manifest off disk.
    let hex = input.get(start..end).ok_or("incomplete unicode escape")?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid unicode escape".to_string());
    }
    u32::from_str_radix(hex, 16)
        .map(|v| (v, end))
        .map_err(|_| "invalid unicode escape".to_string())
}

/// One container deeper than `depth`, or an error past [`MAX_DEPTH`].
fn deeper(depth: usize) -> Result<usize, String> {
    let depth = depth.saturating_add(1);
    if depth > MAX_DEPTH {
        Err(format!("nested more than {MAX_DEPTH} deep"))
    } else {
        Ok(depth)
    }
}

/// A member of an object as the document wrote it ([`object_members`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Member<'a> {
    /// Its name.
    pub key: String,
    /// Its value, as read.
    pub value: JsonValue,
    /// Its value exactly as the document wrote it: `1.50`,
    /// `1700000000123456789`, `{"a": [1, 2]}`, `"aA"` with its quotes
    /// and escapes.
    pub text: &'a str,
}

/// The members of the one object `input` is, in the order written, each with
/// its value's own text.
///
/// For a reader that shows what was written rather than what it reads as.
/// A number is read as an `f64`, so `1.50` reads back as `1.5`, and a 64-bit
/// id or a nanosecond time past 2^53 reads back rounded -- a log viewer
/// showing `1700000000123456800` for `1700000000123456789` would show a
/// different id. [`Member::text`] is the value as written; the grammar is
/// [`json_parse`]'s, so what one accepts the other does.
///
/// # Errors
///
/// As [`json_parse`]; and a document that is JSON but not an object.
pub fn object_members(input: &str) -> Result<Vec<Member<'_>>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty input".to_string());
    }
    if !trimmed.starts_with('{') {
        return Err("not an object".to_string());
    }
    let (members, rest) = parse_members(trimmed, 0)?;
    if !rest.trim().is_empty() {
        let shown: String = rest.chars().take(20).collect();
        return Err(format!("trailing characters: {shown:?}"));
    }
    Ok(members
        .into_iter()
        .map(|(key, value, text)| Member { key, value, text })
        .collect())
}

/// An object's members, each with its value's text, and what follows the
/// object. `input` starts with its `{`.
type Members<'a> = (Vec<(String, JsonValue, &'a str)>, &'a str);

fn parse_members(input: &str, depth: usize) -> Result<Members<'_>, String> {
    let depth = deeper(depth)?;
    let mut s = input.get(1..).unwrap_or_default(); // skip '{'
    let mut members = Vec::new();

    s = s.trim_start();
    if let Some(rest) = s.strip_prefix('}') {
        return Ok((members, rest));
    }

    loop {
        s = s.trim_start();
        let (key_val, rest) = parse_string(s)?;
        let key = match key_val {
            JsonValue::Str(k) => k,
            _ => return Err("object key must be string".to_string()),
        };
        s = rest.trim_start();
        let Some(after_colon) = s.strip_prefix(':') else {
            return Err("expected ':'".to_string());
        };
        let start = after_colon.trim_start();
        let (val, rest) = parse_value(start, depth)?;
        // `rest` is what `start` had left after the value, so the value is
        // the part before it -- ending on a character boundary, since `rest`
        // begins on one.
        let text = start
            .get(..start.len().saturating_sub(rest.len()))
            .unwrap_or_default();
        members.push((key, val, text));
        s = rest.trim_start();
        if let Some(rest) = s.strip_prefix('}') {
            return Ok((members, rest));
        }
        let Some(after_comma) = s.strip_prefix(',') else {
            return Err("expected ',' or '}'".to_string());
        };
        s = after_comma;
    }
}

fn parse_object(input: &str, depth: usize) -> Result<(JsonValue, &str), String> {
    let (members, rest) = parse_members(input, depth)?;
    let entries = members
        .into_iter()
        .map(|(key, value, _)| (key, value))
        .collect();
    Ok((JsonValue::Object(entries), rest))
}

fn parse_array(input: &str, depth: usize) -> Result<(JsonValue, &str), String> {
    let depth = deeper(depth)?;
    let mut s = &input[1..]; // skip '['
    let mut items = Vec::new();

    s = s.trim_start();
    if let Some(rest) = s.strip_prefix(']') {
        return Ok((JsonValue::Array(items), rest));
    }

    loop {
        let (val, rest) = parse_value(s, depth)?;
        items.push(val);
        s = rest.trim_start();
        if let Some(rest) = s.strip_prefix(']') {
            return Ok((JsonValue::Array(items), rest));
        }
        let Some(after_comma) = s.strip_prefix(',') else {
            return Err("expected ',' or ']'".to_string());
        };
        s = after_comma;
    }
}

fn parse_bool(input: &str) -> Result<(JsonValue, &str), String> {
    if let Some(rest) = input.strip_prefix("true") {
        Ok((JsonValue::Bool(true), rest))
    } else if let Some(rest) = input.strip_prefix("false") {
        Ok((JsonValue::Bool(false), rest))
    } else {
        Err("expected 'true' or 'false'".to_string())
    }
}

fn parse_null(input: &str) -> Result<(JsonValue, &str), String> {
    if let Some(rest) = input.strip_prefix("null") {
        Ok((JsonValue::Null, rest))
    } else {
        Err("expected 'null'".to_string())
    }
}

fn parse_number(input: &str) -> Result<(JsonValue, &str), String> {
    let bytes = input.as_bytes();
    let mut end = 0usize;

    // Reading through `get` carries the bound with the byte, so the "am I still
    // inside the string?" test cannot drift apart from the byte it guards.
    if bytes.get(end) == Some(&b'-') {
        end = end.saturating_add(1);
    }
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end = end.saturating_add(1);
    }
    if bytes.get(end) == Some(&b'.') {
        end = end.saturating_add(1);
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end = end.saturating_add(1);
        }
    }
    if matches!(bytes.get(end), Some(&b'e' | &b'E')) {
        end = end.saturating_add(1);
        if matches!(bytes.get(end), Some(&b'+' | &b'-')) {
            end = end.saturating_add(1);
        }
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end = end.saturating_add(1);
        }
    }

    // Every byte the scan accepted is ASCII, so `end` is always on a character
    // boundary and neither split can panic — but `get` states that rather than
    // relying on the reader to re-derive it.
    let num_str = input.get(..end).unwrap_or_default();
    let rest = input.get(end..).unwrap_or_default();
    let num: f64 = num_str
        .parse()
        .map_err(|_| format!("invalid number: {num_str}"))?;
    Ok((JsonValue::Number(num), rest))
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did it
    // -- that is the diagnosis. The defensive lints keep panics out of code
    // that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

    // --- JSON Parser Tests ---

    #[test]
    fn test_json_parse_string() {
        let val = json_parse(r#""hello""#).unwrap();
        assert_eq!(val.as_str(), Some("hello"));
    }

    /// The strings in a manifest are file paths. A byte-at-a-time `as char`
    /// read turns every non-ASCII path into one that names no file, so restore
    /// and verify both report it missing.
    #[test]
    fn a_non_ascii_string_survives_the_json_round_trip() {
        for text in [
            "写真/2024.jpg",
            "Musique/Café/piste.flac",
            "Ωμέγα.txt",
            "🚀/launch.log",
            "D:/Δοκιμή/日本語/файл.bin",
        ] {
            let encoded = JsonValue::Str(text.to_string()).to_string();
            let val =
                json_parse(&encoded).unwrap_or_else(|e| panic!("failed to parse {encoded}: {e}"));
            assert_eq!(val.as_str(), Some(text), "path changed: {encoded}");
        }
    }

    /// `\uXXXX` is what our own writer emits for control characters, and what
    /// any other JSON writer may emit for anything at all. The old reader
    /// dropped an astral character silently rather than pairing surrogates.
    #[test]
    fn unicode_escapes_including_surrogate_pairs_are_decoded() {
        for (encoded, want) in [
            (r#""\u0041""#, "A"),
            (r#""\u00e9""#, "é"),
            (r#""\u5199\u771f""#, "写真"),
            (r#""\ud83d\ude80/launch.log""#, "🚀/launch.log"),
            (r#""a\u0001b""#, "a\u{01}b"),
        ] {
            let val = json_parse(encoded).unwrap_or_else(|e| panic!("{encoded}: {e}"));
            assert_eq!(val.as_str(), Some(want), "wrong decode of {encoded}");
        }
    }

    /// A `\u` whose four bytes ran into a multi-byte character used to be
    /// sliced blindly, so reading a manifest could panic on a char boundary.
    #[test]
    fn a_malformed_unicode_escape_is_an_error_not_a_panic() {
        for bad in [r#""\u日本""#, r#""\u12""#, r#""\uzzzz""#, r#""\u""#] {
            assert!(
                json_parse(bad).is_err(),
                "{bad} should be rejected, not accepted or panic"
            );
        }
    }

    /// Control: the ASCII path is byte-for-byte what it always was.
    #[test]
    fn ascii_json_strings_are_unchanged() {
        for (encoded, want) in [
            (r#""hello""#, "hello"),
            (r#""a\"b""#, "a\"b"),
            (r#""a\\b""#, "a\\b"),
            (r#""a\nb\tc\/d""#, "a\nb\tc/d"),
            (r#""""#, ""),
        ] {
            let val = json_parse(encoded).unwrap_or_else(|e| panic!("{encoded}: {e}"));
            assert_eq!(val.as_str(), Some(want), "wrong parse of {encoded}");
        }
    }

    #[test]
    fn test_json_parse_number() {
        let val = json_parse("42").unwrap();
        assert_eq!(val.as_u64(), Some(42));
    }

    #[test]
    fn test_json_parse_object() {
        let val = json_parse(r#"{"key": "value", "num": 123}"#).unwrap();
        assert_eq!(val.get("key").unwrap().as_str(), Some("value"));
        assert_eq!(val.get("num").unwrap().as_u64(), Some(123));
    }

    #[test]
    fn test_json_parse_array() {
        let val = json_parse(r"[1, 2, 3]").unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_u64(), Some(1));
    }

    #[test]
    fn test_json_parse_nested() {
        let val = json_parse(r#"{"files": [{"path": "a.txt", "size": 100}]}"#).unwrap();
        let files = val.get("files").unwrap().as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].get("path").unwrap().as_str(), Some("a.txt"));
        assert_eq!(files[0].get("size").unwrap().as_u64(), Some(100));
    }

    #[test]
    fn test_json_escape_roundtrip() {
        let val = JsonValue::Str("hello\nworld\t\"quoted\"".to_string());
        let serialized = format!("{}", val);
        let parsed = json_parse(&serialized).unwrap();
        assert_eq!(parsed.as_str(), Some("hello\nworld\t\"quoted\""));
    }

    // --- JSON pretty-printing ---

    /// The manifest round-trip tests cannot see this: the parser discards
    /// whitespace, so a printer that lost an indent, doubled a comma or put the
    /// closing brace in the wrong column would still parse back to an equal
    /// value. A manifest is a file a person opens when a backup goes wrong, so
    /// its shape is a feature. Pinned exactly.
    #[test]
    fn the_pretty_printer_lays_a_manifest_out_readably() {
        let value = JsonValue::Object(vec![
            ("name".to_string(), JsonValue::Str("backup-42".to_string())),
            (
                "files".to_string(),
                JsonValue::Array(vec![
                    JsonValue::Str("a.txt".to_string()),
                    JsonValue::Str("b.txt".to_string()),
                ]),
            ),
            ("count".to_string(), JsonValue::Number(2.0)),
        ]);
        assert_eq!(
            json_pretty(&value, 2),
            "{\n  \"name\": \"backup-42\",\n  \"files\": [\n    \"a.txt\",\n    \"b.txt\"\n  ],\n  \"count\": 2\n}"
        );
    }

    /// An empty container has no entries to separate, so it never reaches the
    /// indenting arms at all — it must still print as valid JSON rather than as
    /// an open brace waiting for a newline.
    #[test]
    fn empty_containers_print_flat() {
        assert_eq!(json_pretty(&JsonValue::Object(vec![]), 2), "{}");
        assert_eq!(json_pretty(&JsonValue::Array(vec![]), 2), "[]");
    }

    /// The byte-offset truncation this replaced would panic here: the twentieth
    /// byte of the trailing text lands inside a two-byte character.
    #[test]
    fn trailing_garbage_is_reported_without_splitting_a_character() {
        let err = json_parse("{} ééééééééééééééééééé").unwrap_err();
        assert!(
            err.starts_with("trailing characters:"),
            "unexpected message: {err}"
        );
    }

    /// **Nesting is bounded**: a reply of a hundred thousand `[` -- which a
    /// recursive reader without a limit follows until the stack runs out, and
    /// the program with it -- is an error. Nesting up to the limit reads.
    #[test]
    fn nesting_past_the_limit_is_an_error_not_a_stack_overflow() {
        let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert!(
            json_parse(&deep(MAX_DEPTH)).is_ok(),
            "the limit itself is refused"
        );
        let err = json_parse(&deep(MAX_DEPTH + 1)).unwrap_err();
        assert!(err.contains("nested more than"), "{err}");
        let hostile = "[".repeat(100_000);
        assert!(json_parse(&hostile).is_err());
        let objects = format!(
            "{}1{}",
            "{\"a\":".repeat(MAX_DEPTH + 1),
            "}".repeat(MAX_DEPTH + 1)
        );
        assert!(json_parse(&objects).is_err(), "objects are not counted");
    }

    /// **A key is escaped as a string is.** It was written bare between
    /// quotes, so a key holding a `"` or a `\` made a document that does not
    /// read back -- by either printer.
    #[test]
    fn an_object_key_with_a_quote_or_a_backslash_round_trips() {
        let value = JsonValue::Object(vec![(String::from("a\"b\\c\nd"), JsonValue::Number(1.0))]);
        for text in [value.to_string(), json_pretty(&value, 2)] {
            let back = json_parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(back, value, "{text}");
        }
    }

    /// A number of any kind, for the readings a forecast is made of.
    #[test]
    fn a_number_reads_as_an_f64_whole_or_not() {
        assert_eq!(json_parse("21.4").unwrap().as_f64(), Some(21.4));
        assert_eq!(json_parse("-3").unwrap().as_f64(), Some(-3.0));
        assert_eq!(json_parse("1e3").unwrap().as_f64(), Some(1000.0));
        assert_eq!(json_parse("\"7\"").unwrap().as_f64(), None);
        assert_eq!(JsonValue::Number(f64::NAN).as_f64(), None);
    }

    /// A number is a `u64` only when it is one exactly: not negative, not a
    /// fraction, not past where an f64 stops holding every whole number.
    #[test]
    fn only_a_whole_number_is_a_u64() {
        let n = |v: f64| JsonValue::Number(v).as_u64();
        assert_eq!(n(0.0), Some(0));
        assert_eq!(n(420.0), Some(420));
        assert_eq!(n(9_007_199_254_740_992.0), Some(9_007_199_254_740_992));
        assert_eq!(n(-5.0), None);
        assert_eq!(n(420.7), None);
        assert_eq!(n(f64::NAN), None);
        assert_eq!(n(f64::INFINITY), None);
        assert_eq!(n(1e20), None);
        assert_eq!(JsonValue::Str("5".to_string()).as_u64(), None);
    }

    /// **Each member's value comes with its own text, as written**: a number
    /// past an `f64`'s exact range, or with a trailing zero, is there as the
    /// document has it, and a nested value is its whole text.
    #[test]
    fn an_objects_members_come_with_their_text_as_written() {
        let members = object_members(
            r#" {"id": 1700000000123456789, "ratio":1.50, "tags":[1, 2], "s":"aA", "n":null} "#,
        )
        .unwrap();
        let texts: Vec<(&str, &str)> = members.iter().map(|m| (m.key.as_str(), m.text)).collect();
        assert_eq!(
            texts,
            [
                ("id", "1700000000123456789"),
                ("ratio", "1.50"),
                ("tags", "[1, 2]"),
                ("s", r#""aA""#),
                ("n", "null"),
            ]
        );
        assert_eq!(members[3].value, JsonValue::Str("aA".to_string()));
        assert_eq!(members[1].value.as_f64(), Some(1.5));
        // The same document through `json_parse` reads the same.
        let JsonValue::Object(entries) = json_parse(r#"{"a":[1,{"b":2}]}"#).unwrap() else {
            panic!("not an object");
        };
        let members = object_members(r#"{"a":[1,{"b":2}]}"#).unwrap();
        assert_eq!(entries[0].1, members[0].value);
    }

    /// What `json_parse` refuses this does, and a document that is not an
    /// object.
    #[test]
    fn object_members_refuses_what_is_not_one_object() {
        assert!(object_members("").is_err());
        assert!(object_members("[1]").is_err());
        assert!(
            object_members(r#"["a":1}"#).is_err(),
            "read as an object for its members, the first character unread"
        );
        assert!(
            object_members(r#"{"a":1 "b":2}"#).is_err(),
            "a comma left out"
        );
        assert!(object_members(r#"{"a":1} and more"#).is_err());
        assert!(object_members(r#"{"a":"#).is_err(), "cut short");
        assert_eq!(object_members(" {} ").unwrap(), Vec::new());
    }
}
