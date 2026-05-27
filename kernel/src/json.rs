//! Minimal JSON parser for kernel use (`no_std` compatible).
//!
//! Provides a simple recursive-descent parser sufficient for OCI image
//! manifests, configuration blobs, and other structured JSON data used
//! by the container runtime.
//!
//! ## Supported types
//!
//! - Null
//! - Boolean (`true` / `false`)
//! - Number (signed integers only — no floats, sufficient for OCI)
//! - String (with `\n`, `\t`, `\\`, `\"`, `\/`, `\uXXXX` escapes)
//! - Array
//! - Object (ordered by insertion — not sorted)
//!
//! ## Limitations
//!
//! - No floating-point support (all numbers are `i64`)
//! - Maximum nesting depth of 32 to prevent stack overflow
//! - Maximum input size of 4 MiB (configurable via `MAX_INPUT_SIZE`)
//! - No streaming/incremental parsing — entire input must be in memory
//!
//! ## References
//!
//! - RFC 8259 (The JavaScript Object Notation Data Interchange Format)
//! - ECMA-404 (The JSON Data Interchange Syntax)

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum nesting depth for JSON objects/arrays.
const MAX_DEPTH: usize = 32;

/// Maximum input size (4 MiB).
const MAX_INPUT_SIZE: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// JSON value type
// ---------------------------------------------------------------------------

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValue {
    /// JSON `null`.
    Null,
    /// JSON boolean (`true` or `false`).
    Bool(bool),
    /// JSON number (integer only — sufficient for OCI manifests).
    Number(i64),
    /// JSON string (unescaped).
    Str(String),
    /// JSON array.
    Array(Vec<JsonValue>),
    /// JSON object (key-value pairs, insertion-ordered).
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Returns the string value if this is a `Str`, or `None`.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns the integer value if this is a `Number`, or `None`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Returns the boolean value if this is a `Bool`, or `None`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns a reference to the array if this is an `Array`, or `None`.
    #[must_use]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            Self::Array(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// Returns a reference to the object entries if this is an `Object`.
    #[must_use]
    pub fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            Self::Object(entries) => Some(entries.as_slice()),
            _ => None,
        }
    }

    /// Look up a field by name in a JSON object.
    ///
    /// Returns `None` if `self` is not an object or the key is absent.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            Self::Object(entries) => {
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

    /// Look up a string field by name.  Convenience for `get(key)?.as_str()`.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    /// Look up an integer field by name.
    #[must_use]
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key)?.as_i64()
    }

    /// Look up an array field by name.
    #[must_use]
    pub fn get_array(&self, key: &str) -> Option<&[JsonValue]> {
        self.get(key)?.as_array()
    }

    /// Check if this value is null.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Recursive-descent JSON parser.
struct Parser<'a> {
    /// Raw input bytes (must be valid UTF-8).
    input: &'a [u8],
    /// Current position in the input.
    pos: usize,
    /// Current nesting depth (objects + arrays).
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            depth: 0,
        }
    }

    /// Peek at the current byte without consuming it.
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Consume and return the current byte.
    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos = self.pos.saturating_add(1);
        Some(b)
    }

    /// Skip whitespace (space, tab, newline, carriage return).
    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.pos = self.pos.saturating_add(1);
                }
                _ => break,
            }
        }
    }

    /// Expect a specific byte, consuming it.  Error if mismatch.
    fn expect(&mut self, expected: u8) -> KernelResult<()> {
        match self.advance() {
            Some(b) if b == expected => Ok(()),
            _ => Err(KernelError::InvalidArgument),
        }
    }

    /// Parse a JSON value.
    fn parse_value(&mut self) -> KernelResult<JsonValue> {
        self.skip_ws();
        match self.peek() {
            None => Err(KernelError::InvalidArgument),
            Some(b'"') => self.parse_string().map(JsonValue::Str),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') => self.parse_true(),
            Some(b'f') => self.parse_false(),
            Some(b'n') => self.parse_null(),
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            Some(_) => Err(KernelError::InvalidArgument),
        }
    }

    /// Parse a JSON string (the opening `"` has not been consumed).
    #[allow(clippy::arithmetic_side_effects)]
    fn parse_string(&mut self) -> KernelResult<String> {
        self.expect(b'"')?;
        let mut s = String::with_capacity(32);

        loop {
            match self.advance() {
                None => return Err(KernelError::InvalidArgument),
                Some(b'"') => return Ok(s),
                Some(b'\\') => {
                    match self.advance() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b'r') => s.push('\r'),
                        Some(b't') => s.push('\t'),
                        Some(b'b') => s.push('\u{0008}'),
                        Some(b'f') => s.push('\u{000C}'),
                        Some(b'u') => {
                            let cp = self.parse_hex4()?;
                            if let Some(c) = char::from_u32(cp) {
                                s.push(c);
                            } else {
                                // Surrogate pair first half — try to read second half.
                                if (0xD800..=0xDBFF).contains(&cp) {
                                    self.expect(b'\\')?;
                                    self.expect(b'u')?;
                                    let cp2 = self.parse_hex4()?;
                                    if (0xDC00..=0xDFFF).contains(&cp2) {
                                        let full = 0x10000
                                            + ((cp - 0xD800) << 10)
                                            + (cp2 - 0xDC00);
                                        if let Some(c) = char::from_u32(full) {
                                            s.push(c);
                                        } else {
                                            s.push('\u{FFFD}');
                                        }
                                    } else {
                                        s.push('\u{FFFD}');
                                    }
                                } else {
                                    s.push('\u{FFFD}');
                                }
                            }
                        }
                        _ => return Err(KernelError::InvalidArgument),
                    }
                }
                Some(b) => {
                    // Direct UTF-8 byte — push as char if valid ASCII,
                    // otherwise collect a full UTF-8 sequence.
                    if b < 0x80 {
                        s.push(b as char);
                    } else {
                        // Multi-byte UTF-8: read continuation bytes.
                        let (needed, mut cp) = if b & 0xE0 == 0xC0 {
                            (1u8, u32::from(b & 0x1F))
                        } else if b & 0xF0 == 0xE0 {
                            (2, u32::from(b & 0x0F))
                        } else if b & 0xF8 == 0xF0 {
                            (3, u32::from(b & 0x07))
                        } else {
                            return Err(KernelError::InvalidArgument);
                        };
                        for _ in 0..needed {
                            let cont = self.advance()
                                .ok_or(KernelError::InvalidArgument)?;
                            if cont & 0xC0 != 0x80 {
                                return Err(KernelError::InvalidArgument);
                            }
                            cp = (cp << 6) | u32::from(cont & 0x3F);
                        }
                        if let Some(c) = char::from_u32(cp) {
                            s.push(c);
                        } else {
                            return Err(KernelError::InvalidArgument);
                        }
                    }
                }
            }
        }
    }

    /// Parse 4 hex digits into a u32 code point.
    #[allow(clippy::arithmetic_side_effects)]
    fn parse_hex4(&mut self) -> KernelResult<u32> {
        let mut val: u32 = 0;
        for _ in 0..4 {
            let b = self.advance().ok_or(KernelError::InvalidArgument)?;
            let digit = match b {
                b'0'..=b'9' => u32::from(b - b'0'),
                b'a'..=b'f' => u32::from(b - b'a') + 10,
                b'A'..=b'F' => u32::from(b - b'A') + 10,
                _ => return Err(KernelError::InvalidArgument),
            };
            val = (val << 4) | digit;
        }
        Ok(val)
    }

    /// Parse a JSON number (integer only).
    #[allow(clippy::arithmetic_side_effects)]
    fn parse_number(&mut self) -> KernelResult<JsonValue> {
        let negative = if self.peek() == Some(b'-') {
            self.advance();
            true
        } else {
            false
        };

        let mut val: i64 = 0;
        let mut digits = 0u32;

        while let Some(b) = self.peek() {
            if b >= b'0' && b <= b'9' {
                let d = i64::from(b - b'0');
                val = val.checked_mul(10)
                    .and_then(|v| v.checked_add(d))
                    .ok_or(KernelError::InvalidArgument)?;
                self.advance();
                digits = digits.saturating_add(1);
            } else {
                break;
            }
        }

        if digits == 0 {
            return Err(KernelError::InvalidArgument);
        }

        // Skip fractional part (if any) — we discard it.
        if self.peek() == Some(b'.') {
            self.advance();
            while let Some(b) = self.peek() {
                if b >= b'0' && b <= b'9' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Skip exponent (if any) — we discard it.
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.advance();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            while let Some(b) = self.peek() {
                if b >= b'0' && b <= b'9' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        if negative {
            val = val.checked_neg().ok_or(KernelError::InvalidArgument)?;
        }

        Ok(JsonValue::Number(val))
    }

    /// Parse `true`.
    fn parse_true(&mut self) -> KernelResult<JsonValue> {
        self.expect(b't')?;
        self.expect(b'r')?;
        self.expect(b'u')?;
        self.expect(b'e')?;
        Ok(JsonValue::Bool(true))
    }

    /// Parse `false`.
    fn parse_false(&mut self) -> KernelResult<JsonValue> {
        self.expect(b'f')?;
        self.expect(b'a')?;
        self.expect(b'l')?;
        self.expect(b's')?;
        self.expect(b'e')?;
        Ok(JsonValue::Bool(false))
    }

    /// Parse `null`.
    fn parse_null(&mut self) -> KernelResult<JsonValue> {
        self.expect(b'n')?;
        self.expect(b'u')?;
        self.expect(b'l')?;
        self.expect(b'l')?;
        Ok(JsonValue::Null)
    }

    /// Parse a JSON object: `{ "key": value, ... }`.
    fn parse_object(&mut self) -> KernelResult<JsonValue> {
        self.expect(b'{')?;
        self.depth = self.depth.saturating_add(1);
        if self.depth > MAX_DEPTH {
            return Err(KernelError::InvalidArgument);
        }

        let mut entries: Vec<(String, JsonValue)> = Vec::new();

        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.advance();
            self.depth = self.depth.saturating_sub(1);
            return Ok(JsonValue::Object(entries));
        }

        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            entries.push((key, value));

            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                }
                Some(b'}') => {
                    self.advance();
                    self.depth = self.depth.saturating_sub(1);
                    return Ok(JsonValue::Object(entries));
                }
                _ => return Err(KernelError::InvalidArgument),
            }
        }
    }

    /// Parse a JSON array: `[ value, ... ]`.
    fn parse_array(&mut self) -> KernelResult<JsonValue> {
        self.expect(b'[')?;
        self.depth = self.depth.saturating_add(1);
        if self.depth > MAX_DEPTH {
            return Err(KernelError::InvalidArgument);
        }

        let mut items: Vec<JsonValue> = Vec::new();

        self.skip_ws();
        if self.peek() == Some(b']') {
            self.advance();
            self.depth = self.depth.saturating_sub(1);
            return Ok(JsonValue::Array(items));
        }

        loop {
            let value = self.parse_value()?;
            items.push(value);

            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                }
                Some(b']') => {
                    self.advance();
                    self.depth = self.depth.saturating_sub(1);
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(KernelError::InvalidArgument),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a JSON string into a [`JsonValue`].
///
/// # Errors
///
/// Returns `InvalidArgument` if the input is not valid JSON, exceeds
/// `MAX_INPUT_SIZE`, or has nesting deeper than `MAX_DEPTH`.
pub fn parse(input: &[u8]) -> KernelResult<JsonValue> {
    if input.len() > MAX_INPUT_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;

    // Ensure there's no trailing non-whitespace.
    parser.skip_ws();
    if parser.pos < parser.input.len() {
        return Err(KernelError::InvalidArgument);
    }

    Ok(value)
}

/// Parse a UTF-8 string as JSON.
pub fn parse_str(input: &str) -> KernelResult<JsonValue> {
    parse(input.as_bytes())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the JSON parser against known inputs.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[json] Running self-test...");

    // Test 1: null, true, false.
    {
        assert_eq!(parse_str("null")?, JsonValue::Null);
        assert_eq!(parse_str("true")?, JsonValue::Bool(true));
        assert_eq!(parse_str("false")?, JsonValue::Bool(false));
        serial_println!("[json]   primitives: OK");
    }

    // Test 2: numbers.
    {
        assert_eq!(parse_str("0")?, JsonValue::Number(0));
        assert_eq!(parse_str("42")?, JsonValue::Number(42));
        assert_eq!(parse_str("-1")?, JsonValue::Number(-1));
        assert_eq!(parse_str("1000000")?, JsonValue::Number(1_000_000));
        serial_println!("[json]   numbers: OK");
    }

    // Test 3: strings.
    {
        assert_eq!(
            parse_str(r#""hello""#)?,
            JsonValue::Str(String::from("hello"))
        );
        assert_eq!(
            parse_str(r#""a\nb""#)?,
            JsonValue::Str(String::from("a\nb"))
        );
        assert_eq!(
            parse_str(r#""a\\b""#)?,
            JsonValue::Str(String::from("a\\b"))
        );
        assert_eq!(
            parse_str(r#""a\"b""#)?,
            JsonValue::Str(String::from("a\"b"))
        );
        // Unicode escape.
        assert_eq!(
            parse_str(r#""\u0041""#)?,
            JsonValue::Str(String::from("A"))
        );
        serial_println!("[json]   strings: OK");
    }

    // Test 4: arrays.
    {
        let v = parse_str("[1, 2, 3]")?;
        let arr = v.as_array().expect("should be array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_i64(), Some(1));
        assert_eq!(arr[2].as_i64(), Some(3));

        // Empty array.
        let v = parse_str("[]")?;
        assert_eq!(v.as_array().expect("array").len(), 0);
        serial_println!("[json]   arrays: OK");
    }

    // Test 5: objects.
    {
        let v = parse_str(r#"{"name": "test", "value": 42}"#)?;
        assert_eq!(v.get_str("name"), Some("test"));
        assert_eq!(v.get_i64("value"), Some(42));

        // Empty object.
        let v = parse_str("{}")?;
        assert_eq!(v.as_object().expect("object").len(), 0);
        serial_println!("[json]   objects: OK");
    }

    // Test 6: nested structure (OCI-manifest-like).
    {
        let input = r#"{
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": "sha256:abc123",
                "size": 1024
            },
            "layers": [
                {
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": "sha256:def456",
                    "size": 2048
                }
            ]
        }"#;
        let v = parse_str(input)?;
        assert_eq!(v.get_i64("schemaVersion"), Some(2));
        let config = v.get("config").expect("config");
        assert_eq!(config.get_str("digest"), Some("sha256:abc123"));
        assert_eq!(config.get_i64("size"), Some(1024));
        let layers = v.get_array("layers").expect("layers");
        assert_eq!(layers.len(), 1);
        assert_eq!(
            layers[0].get_str("digest"),
            Some("sha256:def456")
        );
        serial_println!("[json]   nested (OCI-like): OK");
    }

    // Test 7: error cases.
    {
        assert!(parse_str("").is_err());
        assert!(parse_str("{").is_err());
        assert!(parse_str("[1,]").is_err()); // trailing comma
        assert!(parse_str(r#"{"a": }"#).is_err()); // missing value
        serial_println!("[json]   error cases: OK");
    }

    // Test 8: whitespace tolerance.
    {
        let v = parse_str("  { \"a\" : 1 }  ")?;
        assert_eq!(v.get_i64("a"), Some(1));
        serial_println!("[json]   whitespace: OK");
    }

    serial_println!("[json] Self-test PASSED (8 tests)");
    Ok(())
}
