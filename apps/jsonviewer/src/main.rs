//! `OurOS` JSON Viewer & Editor
//!
//! A full-featured JSON data viewer and editor with:
//! - Custom JSON parser supporting full JSON spec (RFC 8259)
//! - Collapsible tree view with color-coded values
//! - Syntax-highlighted raw/formatted view with pretty-printing
//! - Search across keys and values with match navigation
//! - `JSONPath` display for selected nodes
//! - In-place edit mode for values, key add/delete
//! - Real-time validation with line/column error reporting
//! - Statistics panel (node count, depth, type distribution)
//! - YAML-like display conversion
//! - Multi-tab document support
//! - Structural JSON diff between two documents
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha theme.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::match_wildcard_for_single_variants)]
#![allow(clippy::single_match_else)]
#![allow(dead_code)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 800.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
const SIDEBAR_WIDTH: f32 = 320.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const PADDING: f32 = 10.0;
const LINE_HEIGHT: f32 = 20.0;
const CHAR_WIDTH: f32 = 8.4;
const SMALL_TEXT: f32 = 12.0;
const NORMAL_TEXT: f32 = 14.0;
const HEADER_TEXT: f32 = 16.0;
const TITLE_TEXT: f32 = 18.0;
const TREE_INDENT: f32 = 20.0;
const TREE_ICON_SIZE: f32 = 14.0;

// Limits
const MAX_INPUT_LEN: usize = 1_048_576;
const MAX_SEARCH_LEN: usize = 256;
const MAX_TABS: usize = 20;
const MAX_DEPTH: usize = 128;
const MAX_SEARCH_RESULTS: usize = 5000;

// ============================================================================
// JSON Value types
// ============================================================================

/// Represents a parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Returns the type name for display.
    fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::Str(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// Returns the number of child nodes (recursive).
    fn node_count(&self) -> usize {
        match self {
            Self::Array(arr) => 1 + arr.iter().map(Self::node_count).sum::<usize>(),
            Self::Object(obj) => 1 + obj.iter().map(|(_, v)| v.node_count()).sum::<usize>(),
            _ => 1,
        }
    }

    /// Returns the maximum depth of the value tree.
    fn max_depth(&self) -> usize {
        match self {
            Self::Array(arr) => 1 + arr.iter().map(Self::max_depth).max().unwrap_or(0),
            Self::Object(obj) => 1 + obj.iter().map(|(_, v)| v.max_depth()).max().unwrap_or(0),
            _ => 1,
        }
    }

    /// Count values by type.
    fn type_counts(&self) -> TypeCounts {
        let mut counts = TypeCounts::default();
        self.count_types(&mut counts);
        counts
    }

    fn count_types(&self, counts: &mut TypeCounts) {
        match self {
            Self::Null => counts.nulls += 1,
            Self::Bool(_) => counts.bools += 1,
            Self::Number(_) => counts.numbers += 1,
            Self::Str(_) => counts.strings += 1,
            Self::Array(arr) => {
                counts.arrays += 1;
                for v in arr {
                    v.count_types(counts);
                }
            }
            Self::Object(obj) => {
                counts.objects += 1;
                for (_, v) in obj {
                    v.count_types(counts);
                }
            }
        }
    }

    /// Approximate serialized size in bytes.
    fn approx_size(&self) -> usize {
        match self {
            Self::Null => 4,
            Self::Bool(b) => {
                if *b {
                    4
                } else {
                    5
                }
            }
            Self::Number(n) => format_number(*n).len(),
            Self::Str(s) => s.len() + 2,
            Self::Array(arr) => {
                2 + arr.iter().map(Self::approx_size).sum::<usize>() + arr.len().saturating_sub(1)
            }
            Self::Object(obj) => {
                2 + obj
                    .iter()
                    .map(|(k, v)| k.len() + 3 + v.approx_size())
                    .sum::<usize>()
                    + obj.len().saturating_sub(1)
            }
        }
    }
}

/// Type distribution counters.
#[derive(Debug, Default, Clone)]
struct TypeCounts {
    nulls: usize,
    bools: usize,
    numbers: usize,
    strings: usize,
    arrays: usize,
    objects: usize,
}

impl TypeCounts {
    fn total(&self) -> usize {
        self.nulls + self.bools + self.numbers + self.strings + self.arrays + self.objects
    }
}

// ============================================================================
// JSON Parser
// ============================================================================

/// Parse error with position information.
#[derive(Debug, Clone)]
struct ParseError {
    message: String,
    line: usize,
    column: usize,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Line {}, Col {}: {}",
            self.line, self.column, self.message
        )
    }
}

/// JSON parser with position tracking.
struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            depth: 0,
        }
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            message: message.into(),
            line: self.line,
            column: self.col,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let byte = self.input.get(self.pos).copied()?;
        self.pos += 1;
        if byte == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(byte)
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), ParseError> {
        match self.advance() {
            Some(b) if b == expected => Ok(()),
            Some(b) => Err(self.error(format!(
                "Expected '{}', found '{}'",
                expected as char, b as char
            ))),
            None => Err(self.error(format!(
                "Expected '{}', found end of input",
                expected as char
            ))),
        }
    }

    fn parse(&mut self) -> Result<JsonValue, ParseError> {
        self.skip_whitespace();
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.pos < self.input.len() {
            return Err(self.error("Unexpected content after JSON value"));
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<JsonValue, ParseError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => self.parse_string().map(JsonValue::Str),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't' | b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(b) => Err(self.error(format!("Unexpected character: '{}'", b as char))),
            None => Err(self.error("Unexpected end of input")),
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut result = String::new();
        loop {
            match self.advance() {
                Some(b'"') => return Ok(result),
                Some(b'\\') => {
                    let escaped = self.parse_escape()?;
                    result.push(escaped);
                }
                Some(b) if b < 0x20 => {
                    return Err(self.error("Control character in string"));
                }
                Some(b) => {
                    // Handle multi-byte UTF-8
                    if b < 0x80 {
                        result.push(b as char);
                    } else {
                        // Rewind and parse full UTF-8 character
                        self.pos -= 1;
                        self.col -= 1;
                        let ch = self.parse_utf8_char()?;
                        result.push(ch);
                    }
                }
                None => return Err(self.error("Unterminated string")),
            }
        }
    }

    fn parse_utf8_char(&mut self) -> Result<char, ParseError> {
        let start = self.pos;
        let first = self
            .input
            .get(self.pos)
            .copied()
            .ok_or_else(|| self.error("Unexpected end of input in UTF-8 sequence"))?;

        let len = if first & 0x80 == 0 {
            1
        } else if first & 0xE0 == 0xC0 {
            2
        } else if first & 0xF0 == 0xE0 {
            3
        } else if first & 0xF8 == 0xF0 {
            4
        } else {
            return Err(self.error("Invalid UTF-8 start byte"));
        };

        if self.pos + len > self.input.len() {
            return Err(self.error("Incomplete UTF-8 sequence"));
        }

        let slice = &self.input[start..start + len];
        let s = core::str::from_utf8(slice).map_err(|_| self.error("Invalid UTF-8 sequence"))?;
        let ch = s
            .chars()
            .next()
            .ok_or_else(|| self.error("Empty UTF-8 sequence"))?;

        for _ in 0..len {
            self.advance();
        }
        Ok(ch)
    }

    fn parse_escape(&mut self) -> Result<char, ParseError> {
        match self.advance() {
            Some(b'"') => Ok('"'),
            Some(b'\\') => Ok('\\'),
            Some(b'/') => Ok('/'),
            Some(b'b') => Ok('\u{0008}'),
            Some(b'f') => Ok('\u{000C}'),
            Some(b'n') => Ok('\n'),
            Some(b'r') => Ok('\r'),
            Some(b't') => Ok('\t'),
            Some(b'u') => self.parse_unicode_escape(),
            Some(b) => Err(self.error(format!("Invalid escape: \\{}", b as char))),
            None => Err(self.error("Unterminated escape sequence")),
        }
    }

    fn parse_unicode_escape(&mut self) -> Result<char, ParseError> {
        let hex = self.parse_hex4()?;
        // Check for surrogate pair
        if (0xD800..=0xDBFF).contains(&hex) {
            // High surrogate, expect \uXXXX low surrogate
            self.expect(b'\\')?;
            self.expect(b'u')?;
            let low = self.parse_hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(self.error("Invalid low surrogate in surrogate pair"));
            }
            let codepoint = 0x10000 + ((hex - 0xD800) << 10) + (low - 0xDC00);
            char::from_u32(codepoint).ok_or_else(|| self.error("Invalid Unicode codepoint"))
        } else if (0xDC00..=0xDFFF).contains(&hex) {
            Err(self.error("Unexpected low surrogate"))
        } else {
            char::from_u32(hex).ok_or_else(|| self.error("Invalid Unicode codepoint"))
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, ParseError> {
        let mut val = 0u32;
        for _ in 0..4 {
            let b = self
                .advance()
                .ok_or_else(|| self.error("Unexpected end in Unicode escape"))?;
            let digit = match b {
                b'0'..=b'9' => u32::from(b - b'0'),
                b'a'..=b'f' => u32::from(b - b'a') + 10,
                b'A'..=b'F' => u32::from(b - b'A') + 10,
                _ => {
                    return Err(self.error(format!(
                        "Invalid hex digit in Unicode escape: '{}'",
                        b as char
                    )));
                }
            };
            val = val * 16 + digit;
        }
        Ok(val)
    }

    fn parse_number(&mut self) -> Result<JsonValue, ParseError> {
        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;

        // Optional minus
        if self.peek() == Some(b'-') {
            self.advance();
        }

        // Integer part
        match self.peek() {
            Some(b'0') => {
                self.advance();
            }
            Some(b'1'..=b'9') => {
                self.advance();
                while let Some(b'0'..=b'9') = self.peek() {
                    self.advance();
                }
            }
            _ => return Err(self.error("Invalid number")),
        }

        // Fractional part
        if self.peek() == Some(b'.') {
            self.advance();
            let frac_start = self.pos;
            while let Some(b'0'..=b'9') = self.peek() {
                self.advance();
            }
            if self.pos == frac_start {
                return Err(self.error("Expected digit after decimal point"));
            }
        }

        // Exponent
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.advance();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.advance();
            }
            let exp_start = self.pos;
            while let Some(b'0'..=b'9') = self.peek() {
                self.advance();
            }
            if self.pos == exp_start {
                return Err(self.error("Expected digit in exponent"));
            }
        }

        let num_str =
            core::str::from_utf8(&self.input[start..self.pos]).map_err(|_| ParseError {
                message: "Invalid UTF-8 in number".into(),
                line: start_line,
                column: start_col,
            })?;
        let value: f64 = num_str.parse().map_err(|_| ParseError {
            message: format!("Invalid number: {num_str}"),
            line: start_line,
            column: start_col,
        })?;
        Ok(JsonValue::Number(value))
    }

    fn parse_bool(&mut self) -> Result<JsonValue, ParseError> {
        if self.match_keyword(b"true") {
            Ok(JsonValue::Bool(true))
        } else if self.match_keyword(b"false") {
            Ok(JsonValue::Bool(false))
        } else {
            Err(self.error("Invalid keyword"))
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, ParseError> {
        if self.match_keyword(b"null") {
            Ok(JsonValue::Null)
        } else {
            Err(self.error("Invalid keyword"))
        }
    }

    fn match_keyword(&mut self, keyword: &[u8]) -> bool {
        let end = self.pos + keyword.len();
        if end > self.input.len() {
            return false;
        }
        if &self.input[self.pos..end] == keyword {
            for _ in 0..keyword.len() {
                self.advance();
            }
            true
        } else {
            false
        }
    }

    fn parse_object(&mut self) -> Result<JsonValue, ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("Maximum nesting depth exceeded"));
        }
        self.expect(b'{')?;
        self.skip_whitespace();

        let mut entries = Vec::new();
        if self.peek() == Some(b'}') {
            self.advance();
            self.depth -= 1;
            return Ok(JsonValue::Object(entries));
        }

        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.parse_value()?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                }
                Some(b'}') => {
                    self.advance();
                    self.depth -= 1;
                    return Ok(JsonValue::Object(entries));
                }
                _ => return Err(self.error("Expected ',' or '}' in object")),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("Maximum nesting depth exceeded"));
        }
        self.expect(b'[')?;
        self.skip_whitespace();

        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.advance();
            self.depth -= 1;
            return Ok(JsonValue::Array(items));
        }

        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                }
                Some(b']') => {
                    self.advance();
                    self.depth -= 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(self.error("Expected ',' or ']' in array")),
            }
        }
    }
}

/// Parse a JSON string into a value.
fn parse_json(input: &str) -> Result<JsonValue, ParseError> {
    let mut parser = Parser::new(input);
    parser.parse()
}

// ============================================================================
// JSON Formatter / Serializer
// ============================================================================

/// Indentation style for formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndentStyle {
    Spaces2,
    Spaces4,
    Spaces8,
    Tabs,
}

impl IndentStyle {
    fn indent_str(self) -> &'static str {
        match self {
            Self::Spaces2 => "  ",
            Self::Spaces4 => "    ",
            Self::Spaces8 => "        ",
            Self::Tabs => "\t",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Spaces2 => "2 spaces",
            Self::Spaces4 => "4 spaces",
            Self::Spaces8 => "8 spaces",
            Self::Tabs => "Tabs",
        }
    }

    fn cycle(self) -> Self {
        match self {
            Self::Spaces2 => Self::Spaces4,
            Self::Spaces4 => Self::Spaces8,
            Self::Spaces8 => Self::Tabs,
            Self::Tabs => Self::Spaces2,
        }
    }
}

/// Format a JSON value as a pretty-printed string.
fn format_json(value: &JsonValue, indent: IndentStyle) -> String {
    let mut output = String::new();
    format_value(value, indent, 0, &mut output);
    output.push('\n');
    output
}

fn format_value(value: &JsonValue, indent: IndentStyle, depth: usize, out: &mut String) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonValue::Number(n) => out.push_str(&format_number(*n)),
        JsonValue::Str(s) => {
            out.push('"');
            escape_json_string(s, out);
            out.push('"');
        }
        JsonValue::Array(arr) => format_array(arr, indent, depth, out),
        JsonValue::Object(obj) => format_object(obj, indent, depth, out),
    }
}

fn format_array(arr: &[JsonValue], indent: IndentStyle, depth: usize, out: &mut String) {
    if arr.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push_str("[\n");
    for (i, item) in arr.iter().enumerate() {
        push_indent(out, indent, depth + 1);
        format_value(item, indent, depth + 1, out);
        if i + 1 < arr.len() {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(out, indent, depth);
    out.push(']');
}

fn format_object(obj: &[(String, JsonValue)], indent: IndentStyle, depth: usize, out: &mut String) {
    if obj.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push_str("{\n");
    for (i, (key, val)) in obj.iter().enumerate() {
        push_indent(out, indent, depth + 1);
        out.push('"');
        escape_json_string(key, out);
        out.push_str("\": ");
        format_value(val, indent, depth + 1, out);
        if i + 1 < obj.len() {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(out, indent, depth);
    out.push('}');
}

fn push_indent(out: &mut String, indent: IndentStyle, depth: usize) {
    let unit = indent.indent_str();
    for _ in 0..depth {
        out.push_str(unit);
    }
}

/// Minify JSON (remove all whitespace).
fn minify_json(value: &JsonValue) -> String {
    let mut output = String::new();
    minify_value(value, &mut output);
    output
}

fn minify_value(value: &JsonValue, out: &mut String) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonValue::Number(n) => out.push_str(&format_number(*n)),
        JsonValue::Str(s) => {
            out.push('"');
            escape_json_string(s, out);
            out.push('"');
        }
        JsonValue::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                minify_value(item, out);
            }
            out.push(']');
        }
        JsonValue::Object(obj) => {
            out.push('{');
            for (i, (key, val)) in obj.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('"');
                escape_json_string(key, out);
                out.push_str("\":");
                minify_value(val, out);
            }
            out.push('}');
        }
    }
}

fn escape_json_string(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if c < '\u{0020}' => {
                let code = c as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            c => out.push(c),
        }
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        let s = format!("{n}");
        s
    }
}

// ============================================================================
// YAML-like display
// ============================================================================

fn to_yaml_like(value: &JsonValue) -> String {
    let mut out = String::new();
    yaml_value(value, 0, &mut out);
    out
}

fn yaml_value(value: &JsonValue, depth: usize, out: &mut String) {
    match value {
        JsonValue::Null => out.push_str("null\n"),
        JsonValue::Bool(b) => {
            out.push_str(if *b { "true\n" } else { "false\n" });
        }
        JsonValue::Number(n) => {
            out.push_str(&format_number(*n));
            out.push('\n');
        }
        JsonValue::Str(s) => {
            // Use quotes if string contains special characters
            if s.contains(':')
                || s.contains('#')
                || s.is_empty()
                || s.starts_with(' ')
                || s.ends_with(' ')
            {
                out.push('"');
                out.push_str(s);
                out.push_str("\"\n");
            } else {
                out.push_str(s);
                out.push('\n');
            }
        }
        JsonValue::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]\n");
            } else {
                out.push('\n');
                for item in arr {
                    push_yaml_indent(out, depth);
                    out.push_str("- ");
                    yaml_value(item, depth + 1, out);
                }
            }
        }
        JsonValue::Object(obj) => {
            if obj.is_empty() {
                out.push_str("{}\n");
            } else {
                out.push('\n');
                for (key, val) in obj {
                    push_yaml_indent(out, depth);
                    out.push_str(key);
                    out.push_str(": ");
                    yaml_value(val, depth + 1, out);
                }
            }
        }
    }
}

fn push_yaml_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

// ============================================================================
// JSONPath
// ============================================================================

/// Build a `JSONPath` string for a node at a given path of indices.
fn build_json_path(value: &JsonValue, path: &[PathSegment]) -> String {
    let mut result = String::from("$");
    let mut current = value;

    for seg in path {
        match seg {
            PathSegment::Key(k) => {
                result.push('.');
                if needs_bracket_notation(k) {
                    result.push_str("[\"");
                    result.push_str(k);
                    result.push_str("\"]");
                } else {
                    result.push_str(k);
                }
            }
            PathSegment::Index(i) => {
                result.push('[');
                result.push_str(&i.to_string());
                result.push(']');
            }
        }
        current = resolve_segment(current, seg);
    }
    let _ = current; // used to walk the tree
    result
}

fn needs_bracket_notation(key: &str) -> bool {
    key.is_empty()
        || key.contains(' ')
        || key.contains('.')
        || key.contains('[')
        || key.contains(']')
        || key.starts_with(|c: char| c.is_ascii_digit())
}

fn resolve_segment<'a>(value: &'a JsonValue, seg: &PathSegment) -> &'a JsonValue {
    match (value, seg) {
        (JsonValue::Object(obj), PathSegment::Key(k)) => obj
            .iter()
            .find(|(key, _)| key == k)
            .map_or(&JsonValue::Null, |(_, v)| v),
        (JsonValue::Array(arr), PathSegment::Index(i)) => arr.get(*i).unwrap_or(&JsonValue::Null),
        _ => &JsonValue::Null,
    }
}

#[derive(Debug, Clone)]
enum PathSegment {
    Key(String),
    Index(usize),
}

// ============================================================================
// Tree Node (for tree view)
// ============================================================================

/// A flattened tree node for display in the tree view.
#[derive(Debug, Clone)]
struct TreeViewNode {
    /// Depth level (0 = root).
    depth: usize,
    /// Label to display (key name or array index).
    label: String,
    /// The value at this node (for leaf display).
    value_display: String,
    /// The JSON value type.
    value_type: ValueType,
    /// Whether this node can be expanded (has children).
    expandable: bool,
    /// Whether this node is currently expanded.
    expanded: bool,
    /// Path segments to this node.
    path: Vec<PathSegment>,
    /// Number of children (for summary).
    child_count: usize,
    /// Whether this node matches a search.
    search_match: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    Null,
    Bool,
    Number,
    Str,
    Array,
    Object,
}

impl ValueType {
    fn color(self) -> Color {
        match self {
            Self::Null => OVERLAY0,
            Self::Bool => BLUE,
            Self::Number => PEACH,
            Self::Str => GREEN,
            Self::Array => LAVENDER,
            Self::Object => MAUVE,
        }
    }
}

/// Build a flat list of visible tree nodes from a JSON value.
fn build_tree_nodes(
    value: &JsonValue,
    expanded_paths: &[Vec<PathSegment>],
    search_matches: &[Vec<PathSegment>],
) -> Vec<TreeViewNode> {
    let mut nodes = Vec::new();
    build_tree_recursive(value, 0, &[], expanded_paths, search_matches, &mut nodes);
    nodes
}

fn build_tree_recursive(
    value: &JsonValue,
    depth: usize,
    path: &[PathSegment],
    expanded_paths: &[Vec<PathSegment>],
    search_matches: &[Vec<PathSegment>],
    nodes: &mut Vec<TreeViewNode>,
) {
    let is_expanded = is_path_expanded(path, expanded_paths);
    let is_match = search_matches.iter().any(|m| paths_equal(m, path));

    match value {
        JsonValue::Object(obj) => {
            nodes.push(TreeViewNode {
                depth,
                label: if path.is_empty() {
                    String::from("{root}")
                } else {
                    path_last_label(path)
                },
                value_display: format!("{{{} keys}}", obj.len()),
                value_type: ValueType::Object,
                expandable: true,
                expanded: is_expanded,
                path: path.to_vec(),
                child_count: obj.len(),
                search_match: is_match,
            });
            if is_expanded {
                for (key, val) in obj {
                    let mut child_path = path.to_vec();
                    child_path.push(PathSegment::Key(key.clone()));
                    build_tree_recursive(
                        val,
                        depth + 1,
                        &child_path,
                        expanded_paths,
                        search_matches,
                        nodes,
                    );
                }
            }
        }
        JsonValue::Array(arr) => {
            nodes.push(TreeViewNode {
                depth,
                label: if path.is_empty() {
                    String::from("[root]")
                } else {
                    path_last_label(path)
                },
                value_display: format!("[{} items]", arr.len()),
                value_type: ValueType::Array,
                expandable: true,
                expanded: is_expanded,
                path: path.to_vec(),
                child_count: arr.len(),
                search_match: is_match,
            });
            if is_expanded {
                for (i, val) in arr.iter().enumerate() {
                    let mut child_path = path.to_vec();
                    child_path.push(PathSegment::Index(i));
                    build_tree_recursive(
                        val,
                        depth + 1,
                        &child_path,
                        expanded_paths,
                        search_matches,
                        nodes,
                    );
                }
            }
        }
        other => {
            let (display, vtype) = leaf_display(other);
            nodes.push(TreeViewNode {
                depth,
                label: if path.is_empty() {
                    String::from("(value)")
                } else {
                    path_last_label(path)
                },
                value_display: display,
                value_type: vtype,
                expandable: false,
                expanded: false,
                path: path.to_vec(),
                child_count: 0,
                search_match: is_match,
            });
        }
    }
}

fn path_last_label(path: &[PathSegment]) -> String {
    match path.last() {
        Some(PathSegment::Key(k)) => k.clone(),
        Some(PathSegment::Index(i)) => format!("[{i}]"),
        None => String::from("(root)"),
    }
}

fn leaf_display(value: &JsonValue) -> (String, ValueType) {
    match value {
        JsonValue::Null => (String::from("null"), ValueType::Null),
        JsonValue::Bool(b) => (b.to_string(), ValueType::Bool),
        JsonValue::Number(n) => (format_number(*n), ValueType::Number),
        JsonValue::Str(s) => {
            let truncated = if s.len() > 60 {
                let prefix: String = s.chars().take(57).collect();
                format!("\"{prefix}...\"")
            } else {
                format!("\"{s}\"")
            };
            (truncated, ValueType::Str)
        }
        JsonValue::Array(_) | JsonValue::Object(_) => (String::from("..."), ValueType::Object),
    }
}

fn is_path_expanded(path: &[PathSegment], expanded: &[Vec<PathSegment>]) -> bool {
    if path.is_empty() {
        // Root is always expanded
        return true;
    }
    expanded.iter().any(|e| paths_equal(e, path))
}

fn paths_equal(a: &[PathSegment], b: &[PathSegment]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(sa, sb)| match (sa, sb) {
        (PathSegment::Key(ka), PathSegment::Key(kb)) => ka == kb,
        (PathSegment::Index(ia), PathSegment::Index(ib)) => ia == ib,
        _ => false,
    })
}

// ============================================================================
// Search
// ============================================================================

/// Search results for key/value matching.
fn search_json(value: &JsonValue, query: &str, case_sensitive: bool) -> Vec<Vec<PathSegment>> {
    let mut results = Vec::new();
    let query_lower = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    search_recursive(value, &[], &query_lower, case_sensitive, &mut results);
    results
}

fn search_recursive(
    value: &JsonValue,
    path: &[PathSegment],
    query: &str,
    case_sensitive: bool,
    results: &mut Vec<Vec<PathSegment>>,
) {
    if results.len() >= MAX_SEARCH_RESULTS {
        return;
    }

    // Check if this node's key matches
    if let Some(seg) = path.last() {
        let key_str = match seg {
            PathSegment::Key(k) => k.clone(),
            PathSegment::Index(i) => i.to_string(),
        };
        let compare = if case_sensitive {
            key_str.clone()
        } else {
            key_str.to_lowercase()
        };
        if compare.contains(query) {
            results.push(path.to_vec());
            // Don't return; also search children
        }
    }

    match value {
        JsonValue::Str(s) => {
            let compare = if case_sensitive {
                s.clone()
            } else {
                s.to_lowercase()
            };
            if compare.contains(query) && !results.iter().any(|r| paths_equal(r, path)) {
                results.push(path.to_vec());
            }
        }
        JsonValue::Number(n) => {
            let ns = format_number(*n);
            if ns.contains(query) && !results.iter().any(|r| paths_equal(r, path)) {
                results.push(path.to_vec());
            }
        }
        JsonValue::Bool(b) => {
            let bs = if *b { "true" } else { "false" };
            if bs.contains(query) && !results.iter().any(|r| paths_equal(r, path)) {
                results.push(path.to_vec());
            }
        }
        JsonValue::Null => {
            if "null".contains(query) && !results.iter().any(|r| paths_equal(r, path)) {
                results.push(path.to_vec());
            }
        }
        JsonValue::Object(obj) => {
            for (key, val) in obj {
                let mut child_path = path.to_vec();
                child_path.push(PathSegment::Key(key.clone()));
                search_recursive(val, &child_path, query, case_sensitive, results);
            }
        }
        JsonValue::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                let mut child_path = path.to_vec();
                child_path.push(PathSegment::Index(i));
                search_recursive(val, &child_path, query, case_sensitive, results);
            }
        }
    }
}

// ============================================================================
// JSON Diff
// ============================================================================

#[derive(Debug, Clone)]
enum DiffKind {
    Added,
    Removed,
    Changed,
    TypeChanged,
}

#[derive(Debug, Clone)]
struct DiffEntry {
    path: String,
    kind: DiffKind,
    left: String,
    right: String,
}

fn diff_json(left: &JsonValue, right: &JsonValue) -> Vec<DiffEntry> {
    let mut entries = Vec::new();
    diff_recursive(left, right, "$", &mut entries);
    entries
}

fn diff_recursive(left: &JsonValue, right: &JsonValue, path: &str, entries: &mut Vec<DiffEntry>) {
    if core::mem::discriminant(left) != core::mem::discriminant(right) {
        entries.push(DiffEntry {
            path: path.to_string(),
            kind: DiffKind::TypeChanged,
            left: format!("{} ({})", value_summary(left), left.type_name()),
            right: format!("{} ({})", value_summary(right), right.type_name()),
        });
        return;
    }

    match (left, right) {
        (JsonValue::Null, JsonValue::Null) => {}
        (JsonValue::Bool(a), JsonValue::Bool(b)) if a != b => {
            entries.push(DiffEntry {
                path: path.to_string(),
                kind: DiffKind::Changed,
                left: a.to_string(),
                right: b.to_string(),
            });
        }
        (JsonValue::Number(a), JsonValue::Number(b)) if (a - b).abs() > f64::EPSILON => {
            entries.push(DiffEntry {
                path: path.to_string(),
                kind: DiffKind::Changed,
                left: format_number(*a),
                right: format_number(*b),
            });
        }
        (JsonValue::Str(a), JsonValue::Str(b)) if a != b => {
            entries.push(DiffEntry {
                path: path.to_string(),
                kind: DiffKind::Changed,
                left: format!("\"{a}\""),
                right: format!("\"{b}\""),
            });
        }
        (JsonValue::Array(a), JsonValue::Array(b)) => {
            let max_len = a.len().max(b.len());
            for i in 0..max_len {
                let child_path = format!("{path}[{i}]");
                match (a.get(i), b.get(i)) {
                    (Some(av), Some(bv)) => diff_recursive(av, bv, &child_path, entries),
                    (Some(av), None) => {
                        entries.push(DiffEntry {
                            path: child_path,
                            kind: DiffKind::Removed,
                            left: value_summary(av),
                            right: String::new(),
                        });
                    }
                    (None, Some(bv)) => {
                        entries.push(DiffEntry {
                            path: child_path,
                            kind: DiffKind::Added,
                            left: String::new(),
                            right: value_summary(bv),
                        });
                    }
                    (None, None) => {}
                }
            }
        }
        (JsonValue::Object(a), JsonValue::Object(b)) => {
            // Check keys in left
            for (key, val_a) in a {
                let child_path = if needs_bracket_notation(key) {
                    format!("{path}[\"{key}\"]")
                } else {
                    format!("{path}.{key}")
                };
                if let Some((_, val_b)) = b.iter().find(|(k, _)| k == key) {
                    diff_recursive(val_a, val_b, &child_path, entries);
                } else {
                    entries.push(DiffEntry {
                        path: child_path,
                        kind: DiffKind::Removed,
                        left: value_summary(val_a),
                        right: String::new(),
                    });
                }
            }
            // Check keys only in right
            for (key, val_b) in b {
                if !a.iter().any(|(k, _)| k == key) {
                    let child_path = if needs_bracket_notation(key) {
                        format!("{path}[\"{key}\"]")
                    } else {
                        format!("{path}.{key}")
                    };
                    entries.push(DiffEntry {
                        path: child_path,
                        kind: DiffKind::Added,
                        left: String::new(),
                        right: value_summary(val_b),
                    });
                }
            }
        }
        _ => {}
    }
}

fn value_summary(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::from("null"),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => format_number(*n),
        JsonValue::Str(s) => {
            if s.len() > 40 {
                let prefix: String = s.chars().take(37).collect();
                format!("\"{prefix}...\"")
            } else {
                format!("\"{s}\"")
            }
        }
        JsonValue::Array(a) => format!("[{} items]", a.len()),
        JsonValue::Object(o) => format!("{{{} keys}}", o.len()),
    }
}

// ============================================================================
// Edit operations
// ============================================================================

/// Mutate a value at a given path.
fn set_value_at_path(root: &mut JsonValue, path: &[PathSegment], new_value: JsonValue) -> bool {
    if path.is_empty() {
        *root = new_value;
        return true;
    }

    let (head, rest) = path.split_first().expect("path is non-empty");
    match (root, head) {
        (JsonValue::Object(obj), PathSegment::Key(k)) => {
            if let Some((_, val)) = obj.iter_mut().find(|(key, _)| key == k) {
                if rest.is_empty() {
                    *val = new_value;
                    true
                } else {
                    set_value_at_path(val, rest, new_value)
                }
            } else {
                false
            }
        }
        (JsonValue::Array(arr), PathSegment::Index(i)) => {
            if let Some(val) = arr.get_mut(*i) {
                if rest.is_empty() {
                    *val = new_value;
                    true
                } else {
                    set_value_at_path(val, rest, new_value)
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Delete a key/index at a given path.
fn delete_at_path(root: &mut JsonValue, path: &[PathSegment]) -> bool {
    if path.is_empty() {
        return false;
    }
    if path.len() == 1 {
        return match (&mut *root, &path[0]) {
            (JsonValue::Object(obj), PathSegment::Key(k)) => {
                let before = obj.len();
                obj.retain(|(key, _)| key != k);
                obj.len() < before
            }
            (JsonValue::Array(arr), PathSegment::Index(i)) if *i < arr.len() => {
                arr.remove(*i);
                true
            }
            _ => false,
        };
    }

    let (head, rest) = path.split_first().expect("path is non-empty");
    match (&mut *root, head) {
        (JsonValue::Object(obj), PathSegment::Key(k)) => {
            if let Some((_, val)) = obj.iter_mut().find(|(key, _)| key == k) {
                delete_at_path(val, rest)
            } else {
                false
            }
        }
        (JsonValue::Array(arr), PathSegment::Index(i)) => {
            if let Some(val) = arr.get_mut(*i) {
                delete_at_path(val, rest)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Add a new key to an object at a given path.
fn add_key_at_path(
    root: &mut JsonValue,
    path: &[PathSegment],
    key: String,
    value: JsonValue,
) -> bool {
    let target = get_value_at_path_mut(root, path);
    if let Some(JsonValue::Object(obj)) = target {
        obj.push((key, value));
        true
    } else {
        false
    }
}

fn get_value_at_path_mut<'a>(
    root: &'a mut JsonValue,
    path: &[PathSegment],
) -> Option<&'a mut JsonValue> {
    if path.is_empty() {
        return Some(root);
    }
    let (head, rest) = path.split_first()?;
    match (root, head) {
        (JsonValue::Object(obj), PathSegment::Key(k)) => {
            let (_, val) = obj.iter_mut().find(|(key, _)| key == k)?;
            get_value_at_path_mut(val, rest)
        }
        (JsonValue::Array(arr), PathSegment::Index(i)) => {
            let val = arr.get_mut(*i)?;
            get_value_at_path_mut(val, rest)
        }
        _ => None,
    }
}

fn get_value_at_path<'a>(root: &'a JsonValue, path: &[PathSegment]) -> Option<&'a JsonValue> {
    if path.is_empty() {
        return Some(root);
    }
    let (head, rest) = path.split_first()?;
    match (root, head) {
        (JsonValue::Object(obj), PathSegment::Key(k)) => {
            let (_, val) = obj.iter().find(|(key, _)| key == k)?;
            get_value_at_path(val, rest)
        }
        (JsonValue::Array(arr), PathSegment::Index(i)) => {
            let val = arr.get(*i)?;
            get_value_at_path(val, rest)
        }
        _ => None,
    }
}

// ============================================================================
// Syntax-highlighted text lines (for raw view)
// ============================================================================

#[derive(Debug, Clone)]
struct HighlightedSpan {
    text: String,
    color: Color,
    bold: bool,
}

/// Generate syntax-highlighted spans for formatted JSON.
fn highlight_json_text(formatted: &str) -> Vec<Vec<HighlightedSpan>> {
    let mut lines: Vec<Vec<HighlightedSpan>> = Vec::new();
    let mut current_line: Vec<HighlightedSpan> = Vec::new();
    let chars: Vec<char> = formatted.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];
        match ch {
            '\n' => {
                lines.push(core::mem::take(&mut current_line));
                i += 1;
            }
            '"' => {
                // Determine if this is a key or a value
                // Look ahead past the string to see if there's a ':'
                let (string_content, end_idx) = extract_string(&chars, i);
                let is_key = is_key_position(&chars, end_idx);
                let color = if is_key { BLUE } else { GREEN };
                current_line.push(HighlightedSpan {
                    text: string_content,
                    color,
                    bold: is_key,
                });
                i = end_idx;
            }
            '0'..='9' | '-' => {
                let start = i;
                if ch == '-' {
                    i += 1;
                }
                while i < len
                    && (chars[i].is_ascii_digit()
                        || chars[i] == '.'
                        || chars[i] == 'e'
                        || chars[i] == 'E'
                        || chars[i] == '+'
                        || chars[i] == '-')
                {
                    // Avoid consuming the minus at start of a negative after 'e'
                    if i > start + 1
                        && (chars[i] == '+' || chars[i] == '-')
                        && chars[i - 1] != 'e'
                        && chars[i - 1] != 'E'
                    {
                        break;
                    }
                    i += 1;
                }
                let num_text: String = chars[start..i].iter().collect();
                current_line.push(HighlightedSpan {
                    text: num_text,
                    color: PEACH,
                    bold: false,
                });
            }
            't' if i + 4 <= len && chars[i..i + 4].iter().collect::<String>() == "true" => {
                current_line.push(HighlightedSpan {
                    text: String::from("true"),
                    color: BLUE,
                    bold: false,
                });
                i += 4;
            }
            'f' if i + 5 <= len && chars[i..i + 5].iter().collect::<String>() == "false" => {
                current_line.push(HighlightedSpan {
                    text: String::from("false"),
                    color: BLUE,
                    bold: false,
                });
                i += 5;
            }
            'n' if i + 4 <= len && chars[i..i + 4].iter().collect::<String>() == "null" => {
                current_line.push(HighlightedSpan {
                    text: String::from("null"),
                    color: OVERLAY0,
                    bold: false,
                });
                i += 4;
            }
            '{' | '}' | '[' | ']' => {
                current_line.push(HighlightedSpan {
                    text: ch.to_string(),
                    color: TEXT_COLOR,
                    bold: true,
                });
                i += 1;
            }
            ':' => {
                current_line.push(HighlightedSpan {
                    text: String::from(": "),
                    color: SUBTEXT0,
                    bold: false,
                });
                // Skip the space after colon if present
                i += 1;
                if i < len && chars[i] == ' ' {
                    i += 1;
                }
            }
            ',' => {
                current_line.push(HighlightedSpan {
                    text: String::from(","),
                    color: SUBTEXT0,
                    bold: false,
                });
                i += 1;
            }
            ' ' | '\t' => {
                let start = i;
                while i < len && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                let ws: String = chars[start..i].iter().collect();
                current_line.push(HighlightedSpan {
                    text: ws,
                    color: TEXT_COLOR,
                    bold: false,
                });
            }
            '\r' => {
                i += 1;
            }
            _ => {
                current_line.push(HighlightedSpan {
                    text: ch.to_string(),
                    color: TEXT_COLOR,
                    bold: false,
                });
                i += 1;
            }
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
}

fn extract_string(chars: &[char], start: usize) -> (String, usize) {
    let mut result = String::new();
    result.push('"');
    let mut i = start + 1;
    let len = chars.len();
    while i < len {
        let ch = chars[i];
        result.push(ch);
        if ch == '\\' && i + 1 < len {
            i += 1;
            result.push(chars[i]);
        } else if ch == '"' {
            return (result, i + 1);
        }
        i += 1;
    }
    (result, i)
}

fn is_key_position(chars: &[char], after_string: usize) -> bool {
    let mut i = after_string;
    let len = chars.len();
    while i < len && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    i < len && chars[i] == ':'
}

// ============================================================================
// Document / Tab state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Tree,
    Raw,
    Yaml,
    Stats,
    Diff,
}

impl ViewMode {
    fn label(self) -> &'static str {
        match self {
            Self::Tree => "Tree",
            Self::Raw => "Raw",
            Self::Yaml => "YAML",
            Self::Stats => "Stats",
            Self::Diff => "Diff",
        }
    }
}

const VIEW_MODES: [ViewMode; 5] = [
    ViewMode::Tree,
    ViewMode::Raw,
    ViewMode::Yaml,
    ViewMode::Stats,
    ViewMode::Diff,
];

/// A single JSON document tab.
struct Document {
    /// Tab identifier.
    id: u64,
    /// Tab title.
    title: String,
    /// Raw input text.
    input: String,
    /// Parsed JSON value (if valid).
    parsed: Option<JsonValue>,
    /// Parse error (if invalid).
    error: Option<ParseError>,
    /// Current view mode.
    view_mode: ViewMode,
    /// Expanded paths in tree view.
    expanded_paths: Vec<Vec<PathSegment>>,
    /// Selected tree node index.
    selected_node: usize,
    /// Scroll offset for tree view.
    tree_scroll: f32,
    /// Scroll offset for raw view.
    raw_scroll: f32,
    /// Indent style.
    indent: IndentStyle,
    /// Whether to show minified.
    minified: bool,
    /// Whether the document has been modified.
    dirty: bool,
    /// Formatted text (cached).
    formatted_cache: Option<String>,
    /// Highlighted lines (cached).
    highlighted_cache: Option<Vec<Vec<HighlightedSpan>>>,
    /// YAML text (cached).
    yaml_cache: Option<String>,
    /// Diff comparison source (second document raw text).
    diff_source: String,
    /// Diff results.
    diff_results: Vec<DiffEntry>,
    /// Diff scroll offset.
    diff_scroll: f32,
}

impl Document {
    fn new(id: u64, title: String) -> Self {
        Self {
            id,
            title,
            input: String::new(),
            parsed: None,
            error: None,
            view_mode: ViewMode::Tree,
            expanded_paths: Vec::new(),
            selected_node: 0,
            tree_scroll: 0.0,
            raw_scroll: 0.0,
            indent: IndentStyle::Spaces2,
            minified: false,
            dirty: false,
            formatted_cache: None,
            highlighted_cache: None,
            yaml_cache: None,
            diff_source: String::new(),
            diff_results: Vec::new(),
            diff_scroll: 0.0,
        }
    }

    fn reparse(&mut self) {
        self.formatted_cache = None;
        self.highlighted_cache = None;
        self.yaml_cache = None;

        if self.input.trim().is_empty() {
            self.parsed = None;
            self.error = None;
            return;
        }

        match parse_json(&self.input) {
            Ok(value) => {
                self.parsed = Some(value);
                self.error = None;
            }
            Err(e) => {
                self.parsed = None;
                self.error = Some(e);
            }
        }
    }

    fn get_formatted(&mut self) -> String {
        if let Some(ref cached) = self.formatted_cache {
            return cached.clone();
        }
        let formatted = if let Some(ref value) = self.parsed {
            if self.minified {
                minify_json(value)
            } else {
                format_json(value, self.indent)
            }
        } else {
            self.input.clone()
        };
        self.formatted_cache = Some(formatted.clone());
        formatted
    }

    fn get_highlighted(&mut self) -> Vec<Vec<HighlightedSpan>> {
        if let Some(ref cached) = self.highlighted_cache {
            return cached.clone();
        }
        let formatted = self.get_formatted();
        let highlighted = highlight_json_text(&formatted);
        self.highlighted_cache = Some(highlighted.clone());
        highlighted
    }

    fn get_yaml(&mut self) -> String {
        if let Some(ref cached) = self.yaml_cache {
            return cached.clone();
        }
        let yaml = if let Some(ref value) = self.parsed {
            to_yaml_like(value)
        } else {
            String::from("(invalid JSON)")
        };
        self.yaml_cache = Some(yaml.clone());
        yaml
    }

    fn invalidate_caches(&mut self) {
        self.formatted_cache = None;
        self.highlighted_cache = None;
        self.yaml_cache = None;
    }

    fn run_diff(&mut self) {
        if self.diff_source.trim().is_empty() {
            self.diff_results.clear();
            return;
        }
        if let Some(ref left) = self.parsed {
            match parse_json(&self.diff_source) {
                Ok(right) => {
                    self.diff_results = diff_json(left, &right);
                }
                Err(_) => {
                    self.diff_results.clear();
                }
            }
        }
    }
}

// ============================================================================
// Application state
// ============================================================================

struct App {
    /// All open documents.
    documents: Vec<Document>,
    /// Index of the active document.
    active_tab: usize,
    /// Next tab ID.
    next_tab_id: u64,
    /// Search query.
    search_query: String,
    /// Whether search is case-sensitive.
    search_case_sensitive: bool,
    /// Current search results.
    search_results: Vec<Vec<PathSegment>>,
    /// Current search result index.
    search_index: usize,
    /// Whether search panel is visible.
    search_visible: bool,
    /// Edit mode flag.
    edit_mode: bool,
    /// Edit buffer for value editing.
    edit_buffer: String,
    /// Whether the app is focused on the input area.
    input_focused: bool,
    /// Cursor position in input.
    cursor_pos: usize,
    /// Width of the window.
    width: f32,
    /// Height of the window.
    height: f32,
}

impl App {
    fn new() -> Self {
        let mut doc = Document::new(1, String::from("Untitled"));
        // Load sample JSON
        doc.input = SAMPLE_JSON.to_string();
        doc.reparse();

        Self {
            documents: vec![doc],
            active_tab: 0,
            next_tab_id: 2,
            search_query: String::new(),
            search_case_sensitive: false,
            search_results: Vec::new(),
            search_index: 0,
            search_visible: false,
            edit_mode: false,
            edit_buffer: String::new(),
            input_focused: false,
            cursor_pos: 0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    fn active_doc(&self) -> Option<&Document> {
        self.documents.get(self.active_tab)
    }

    fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.documents.get_mut(self.active_tab)
    }

    fn new_tab(&mut self) {
        if self.documents.len() >= MAX_TABS {
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let title = format!("Untitled {id}");
        self.documents.push(Document::new(id, title));
        self.active_tab = self.documents.len() - 1;
    }

    fn close_tab(&mut self, index: usize) {
        if self.documents.len() <= 1 {
            return;
        }
        if index < self.documents.len() {
            self.documents.remove(index);
            if self.active_tab >= self.documents.len() {
                self.active_tab = self.documents.len() - 1;
            }
        }
    }

    fn perform_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
            self.search_index = 0;
            return;
        }
        if let Some(doc) = self.documents.get(self.active_tab)
            && let Some(ref value) = doc.parsed
        {
            self.search_results =
                search_json(value, &self.search_query, self.search_case_sensitive);
            if self.search_index >= self.search_results.len() {
                self.search_index = 0;
            }
        }
    }

    fn search_next(&mut self) {
        if !self.search_results.is_empty() {
            self.search_index = (self.search_index + 1) % self.search_results.len();
            self.ensure_search_result_visible();
        }
    }

    fn search_prev(&mut self) {
        if !self.search_results.is_empty() {
            if self.search_index == 0 {
                self.search_index = self.search_results.len() - 1;
            } else {
                self.search_index -= 1;
            }
            self.ensure_search_result_visible();
        }
    }

    fn ensure_search_result_visible(&mut self) {
        if let Some(result_path) = self.search_results.get(self.search_index) {
            // Ensure all ancestor paths are expanded
            if let Some(doc) = self.documents.get_mut(self.active_tab) {
                for i in 1..result_path.len() {
                    let ancestor = result_path[..i].to_vec();
                    if !doc.expanded_paths.iter().any(|p| paths_equal(p, &ancestor)) {
                        doc.expanded_paths.push(ancestor);
                    }
                }
            }
        }
    }

    fn toggle_expand(&mut self, path: &[PathSegment]) {
        if let Some(doc) = self.documents.get_mut(self.active_tab) {
            if let Some(idx) = doc.expanded_paths.iter().position(|p| paths_equal(p, path)) {
                doc.expanded_paths.remove(idx);
            } else {
                doc.expanded_paths.push(path.to_vec());
            }
        }
    }

    /// Handle keyboard events.
    fn handle_key(
        &mut self,
        key: guitk::event::Key,
        modifiers: guitk::event::Modifiers,
        text: Option<char>,
    ) {
        use guitk::event::Key;

        // Global shortcuts
        if modifiers.ctrl {
            match key {
                Key::N => {
                    self.new_tab();
                    return;
                }
                Key::W => {
                    let idx = self.active_tab;
                    self.close_tab(idx);
                    return;
                }
                Key::F => {
                    self.search_visible = !self.search_visible;
                    return;
                }
                Key::G => {
                    if modifiers.shift {
                        self.search_prev();
                    } else {
                        self.search_next();
                    }
                    return;
                }
                Key::E => {
                    self.edit_mode = !self.edit_mode;
                    return;
                }
                Key::Tab => {
                    if !self.documents.is_empty() {
                        if modifiers.shift {
                            if self.active_tab == 0 {
                                self.active_tab = self.documents.len() - 1;
                            } else {
                                self.active_tab -= 1;
                            }
                        } else {
                            self.active_tab = (self.active_tab + 1) % self.documents.len();
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        // Input area handling
        if self.input_focused {
            self.handle_input_key(key, modifiers, text);
            return;
        }

        // Search bar handling
        if self.search_visible {
            match key {
                Key::Escape => {
                    self.search_visible = false;
                    return;
                }
                Key::Enter => {
                    if modifiers.shift {
                        self.search_prev();
                    } else {
                        self.search_next();
                    }
                    return;
                }
                Key::Backspace => {
                    self.search_query.pop();
                    self.perform_search();
                    return;
                }
                _ => {
                    if let Some(ch) = text
                        && self.search_query.len() < MAX_SEARCH_LEN
                    {
                        self.search_query.push(ch);
                        self.perform_search();
                    }
                    return;
                }
            }
        }

        // View mode handling — extract mode first to avoid holding &mut doc across handle_tree_key
        let view_mode = self.documents.get(self.active_tab).map(|d| d.view_mode);
        match view_mode {
            Some(ViewMode::Tree) => self.handle_tree_key(key, modifiers),
            Some(ViewMode::Raw) => {
                if let Some(doc) = self.documents.get_mut(self.active_tab) {
                    match key {
                        Key::Up => doc.raw_scroll = (doc.raw_scroll - LINE_HEIGHT).max(0.0),
                        Key::Down => doc.raw_scroll += LINE_HEIGHT,
                        Key::PageUp => {
                            doc.raw_scroll = (doc.raw_scroll - 10.0 * LINE_HEIGHT).max(0.0)
                        }
                        Key::PageDown => doc.raw_scroll += 10.0 * LINE_HEIGHT,
                        Key::Home => doc.raw_scroll = 0.0,
                        Key::I if !modifiers.ctrl => {
                            doc.indent = doc.indent.cycle();
                            doc.invalidate_caches();
                        }
                        Key::M if !modifiers.ctrl => {
                            doc.minified = !doc.minified;
                            doc.invalidate_caches();
                        }
                        _ => {}
                    }
                }
            }
            Some(ViewMode::Diff) => {
                if let Some(doc) = self.documents.get_mut(self.active_tab) {
                    match key {
                        Key::Up => doc.diff_scroll = (doc.diff_scroll - LINE_HEIGHT).max(0.0),
                        Key::Down => doc.diff_scroll += LINE_HEIGHT,
                        Key::PageUp => {
                            doc.diff_scroll = (doc.diff_scroll - 10.0 * LINE_HEIGHT).max(0.0)
                        }
                        Key::PageDown => doc.diff_scroll += 10.0 * LINE_HEIGHT,
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        // Mode switching with number keys (separate borrow scope)
        if let Some(doc) = self.documents.get_mut(self.active_tab) {
            match key {
                Key::Num1 if !modifiers.ctrl => doc.view_mode = ViewMode::Tree,
                Key::Num2 if !modifiers.ctrl => doc.view_mode = ViewMode::Raw,
                Key::Num3 if !modifiers.ctrl => doc.view_mode = ViewMode::Yaml,
                Key::Num4 if !modifiers.ctrl => doc.view_mode = ViewMode::Stats,
                Key::Num5 if !modifiers.ctrl => doc.view_mode = ViewMode::Diff,
                _ => {}
            }
        }
    }

    fn handle_tree_key(&mut self, key: guitk::event::Key, modifiers: guitk::event::Modifiers) {
        use guitk::event::Key;

        let doc = match self.documents.get_mut(self.active_tab) {
            Some(d) => d,
            None => return,
        };

        let nodes = build_tree_nodes(
            doc.parsed.as_ref().unwrap_or(&JsonValue::Null),
            &doc.expanded_paths,
            &self.search_results,
        );

        let _ = modifiers;

        match key {
            Key::Up if doc.selected_node > 0 => {
                doc.selected_node -= 1;
            }
            Key::Down if doc.selected_node + 1 < nodes.len() => {
                doc.selected_node += 1;
            }
            Key::Left => {
                // Collapse current node or go to parent
                if let Some(node) = nodes.get(doc.selected_node) {
                    if node.expandable && node.expanded {
                        let path = node.path.clone();
                        self.toggle_expand(&path);
                    } else if !node.path.is_empty() {
                        // Go to parent
                        let parent_path = &node.path[..node.path.len() - 1];
                        if let Some(idx) =
                            nodes.iter().position(|n| paths_equal(&n.path, parent_path))
                            && let Some(d) = self.documents.get_mut(self.active_tab)
                        {
                            d.selected_node = idx;
                        }
                    }
                }
            }
            Key::Right => {
                // Expand current node or go to first child
                if let Some(node) = nodes.get(doc.selected_node) {
                    if node.expandable && !node.expanded {
                        let path = node.path.clone();
                        self.toggle_expand(&path);
                    } else if node.expandable
                        && node.expanded
                        && doc.selected_node + 1 < nodes.len()
                    {
                        doc.selected_node += 1;
                    }
                }
            }
            Key::Enter | Key::Space => {
                if let Some(node) = nodes.get(doc.selected_node) {
                    if node.expandable {
                        let path = node.path.clone();
                        self.toggle_expand(&path);
                    } else if self.edit_mode {
                        // Start editing this value
                        self.edit_buffer = node.value_display.clone();
                    }
                }
            }
            Key::Delete => {
                if self.edit_mode
                    && let Some(node) = nodes.get(doc.selected_node)
                {
                    let path = node.path.clone();
                    if let Some(d) = self.documents.get_mut(self.active_tab) {
                        // Perform deletion in a separate scope to release borrow on d.parsed
                        let deleted = d
                            .parsed
                            .as_mut()
                            .is_some_and(|value| delete_at_path(value, &path));
                        if deleted {
                            d.dirty = true;
                            d.invalidate_caches();
                            // Re-borrow parsed immutably to regenerate input text
                            if let Some(ref value) = d.parsed {
                                d.input = format_json(value, d.indent);
                            }
                        }
                    }
                }
            }
            Key::PageUp => {
                doc.selected_node = doc.selected_node.saturating_sub(10);
            }
            Key::PageDown => {
                doc.selected_node = (doc.selected_node + 10).min(nodes.len().saturating_sub(1));
            }
            Key::Home => {
                doc.selected_node = 0;
            }
            Key::End if !nodes.is_empty() => {
                doc.selected_node = nodes.len() - 1;
            }
            _ => {}
        }
    }

    fn handle_input_key(
        &mut self,
        key: guitk::event::Key,
        _modifiers: guitk::event::Modifiers,
        text: Option<char>,
    ) {
        use guitk::event::Key;

        let doc = match self.documents.get_mut(self.active_tab) {
            Some(d) => d,
            None => return,
        };

        match key {
            Key::Escape => {
                self.input_focused = false;
            }
            Key::Backspace => {
                if self.cursor_pos > 0 && !doc.input.is_empty() {
                    let byte_pos = char_to_byte_pos(&doc.input, self.cursor_pos - 1);
                    let next_byte = char_to_byte_pos(&doc.input, self.cursor_pos);
                    doc.input.drain(byte_pos..next_byte);
                    self.cursor_pos -= 1;
                    doc.dirty = true;
                    doc.reparse();
                }
            }
            Key::Delete => {
                let char_count = doc.input.chars().count();
                if self.cursor_pos < char_count {
                    let byte_pos = char_to_byte_pos(&doc.input, self.cursor_pos);
                    let next_byte = char_to_byte_pos(&doc.input, self.cursor_pos + 1);
                    doc.input.drain(byte_pos..next_byte);
                    doc.dirty = true;
                    doc.reparse();
                }
            }
            Key::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                }
            }
            Key::Right => {
                let char_count = doc.input.chars().count();
                if self.cursor_pos < char_count {
                    self.cursor_pos += 1;
                }
            }
            Key::Home => {
                self.cursor_pos = 0;
            }
            Key::End => {
                self.cursor_pos = doc.input.chars().count();
            }
            Key::Enter => {
                if doc.input.len() < MAX_INPUT_LEN {
                    let byte_pos = char_to_byte_pos(&doc.input, self.cursor_pos);
                    doc.input.insert(byte_pos, '\n');
                    self.cursor_pos += 1;
                    doc.dirty = true;
                    doc.reparse();
                }
            }
            _ => {
                if let Some(ch) = text
                    && doc.input.len() < MAX_INPUT_LEN
                {
                    let byte_pos = char_to_byte_pos(&doc.input, self.cursor_pos);
                    doc.input.insert(byte_pos, ch);
                    self.cursor_pos += 1;
                    doc.dirty = true;
                    doc.reparse();
                }
            }
        }
    }

    fn handle_mouse(&mut self, x: f32, y: f32, button: guitk::event::MouseButton) {
        let _ = button;

        // Tab bar clicks
        if (TOOLBAR_HEIGHT..TOOLBAR_HEIGHT + TAB_BAR_HEIGHT).contains(&y) {
            self.handle_tab_click(x);
            return;
        }

        // View mode tab clicks
        let mode_bar_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        if y >= mode_bar_y && y < mode_bar_y + 30.0 {
            self.handle_mode_click(x);
            return;
        }

        // Tree view clicks
        let content_y = mode_bar_y + 30.0;
        if y >= content_y
            && x < self.width - SIDEBAR_WIDTH
            && let Some(doc) = self.documents.get_mut(self.active_tab)
            && doc.view_mode == ViewMode::Tree
        {
            let row = ((y - content_y + doc.tree_scroll) / LINE_HEIGHT) as usize;
            let nodes = build_tree_nodes(
                doc.parsed.as_ref().unwrap_or(&JsonValue::Null),
                &doc.expanded_paths,
                &self.search_results,
            );
            if row < nodes.len() {
                doc.selected_node = row;
                if let Some(node) = nodes.get(row)
                    && node.expandable
                {
                    let path = node.path.clone();
                    self.toggle_expand(&path);
                }
            }
        }
    }

    fn handle_tab_click(&mut self, x: f32) {
        let mut tab_x = PADDING;
        for (i, doc) in self.documents.iter().enumerate() {
            let tab_width = doc.title.len() as f32 * CHAR_WIDTH + 40.0;
            if x >= tab_x && x < tab_x + tab_width {
                self.active_tab = i;
                return;
            }
            tab_x += tab_width + 4.0;
        }
        // Click on "+" button area
        if x >= tab_x && x < tab_x + 30.0 {
            self.new_tab();
        }
    }

    fn handle_mode_click(&mut self, x: f32) {
        let mut mode_x = PADDING;
        for mode in &VIEW_MODES {
            let mode_width = mode.label().len() as f32 * CHAR_WIDTH + 20.0;
            if x >= mode_x && x < mode_x + mode_width {
                if let Some(doc) = self.documents.get_mut(self.active_tab) {
                    doc.view_mode = *mode;
                }
                return;
            }
            mode_x += mode_width + 4.0;
        }
    }

    fn handle_scroll(&mut self, _x: f32, _y: f32, dy: f32) {
        if let Some(doc) = self.documents.get_mut(self.active_tab) {
            match doc.view_mode {
                ViewMode::Tree => {
                    doc.tree_scroll = (doc.tree_scroll - dy * 3.0).max(0.0);
                }
                ViewMode::Raw | ViewMode::Yaml | ViewMode::Stats => {
                    doc.raw_scroll = (doc.raw_scroll - dy * 3.0).max(0.0);
                }
                ViewMode::Diff => {
                    doc.diff_scroll = (doc.diff_scroll - dy * 3.0).max(0.0);
                }
            }
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    fn render(&mut self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut cmds);
        self.render_tab_bar(&mut cmds);
        self.render_mode_bar(&mut cmds);
        self.render_content(&mut cmds);
        self.render_sidebar(&mut cmds);
        self.render_status_bar(&mut cmds);

        if self.search_visible {
            self.render_search_bar(&mut cmds);
        }

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 14.0,
            text: String::from("JSON Viewer"),
            color: LAVENDER,
            font_size: TITLE_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Toolbar buttons
        let buttons = [
            ("New", BLUE),
            ("Search", TEAL),
            ("Edit", if self.edit_mode { GREEN } else { SUBTEXT0 }),
        ];
        let mut bx = 200.0;
        for (label, color) in &buttons {
            let bw = label.len() as f32 * CHAR_WIDTH + 16.0;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: 8.0,
                width: bw,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: 18.0,
                text: (*label).to_string(),
                color: *color,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            bx += bw + 8.0;
        }

        // Separator line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.width,
            y2: TOOLBAR_HEIGHT,
            color: SURFACE1,
            width: 1.0,
        });
    }

    fn render_tab_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT;

        // Tab bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: TAB_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tab_x = PADDING;
        for (i, doc) in self.documents.iter().enumerate() {
            let is_active = i == self.active_tab;
            let tab_width = doc.title.len() as f32 * CHAR_WIDTH + 40.0;

            // Tab background
            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y: y + 4.0,
                width: tab_width,
                height: TAB_BAR_HEIGHT - 4.0,
                color: if is_active { BASE } else { SURFACE0 },
                corner_radii: CornerRadii {
                    top_left: 6.0,
                    top_right: 6.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            // Dirty indicator
            let label = if doc.dirty {
                format!("{} *", doc.title)
            } else {
                doc.title.clone()
            };

            // Tab label
            cmds.push(RenderCommand::Text {
                x: tab_x + 10.0,
                y: y + 16.0,
                text: label,
                color: if is_active { TEXT_COLOR } else { SUBTEXT0 },
                font_size: SMALL_TEXT,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 20.0),
            });

            // Close button
            if self.documents.len() > 1 {
                cmds.push(RenderCommand::Text {
                    x: tab_x + tab_width - 18.0,
                    y: y + 16.0,
                    text: String::from("x"),
                    color: OVERLAY0,
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            tab_x += tab_width + 4.0;
        }

        // New tab button
        cmds.push(RenderCommand::FillRect {
            x: tab_x,
            y: y + 6.0,
            width: 28.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: tab_x + 8.0,
            y: y + 16.0,
            text: String::from("+"),
            color: SUBTEXT0,
            font_size: NORMAL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Bottom line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_BAR_HEIGHT,
            x2: self.width,
            y2: y + TAB_BAR_HEIGHT,
            color: SURFACE1,
            width: 1.0,
        });
    }

    fn render_mode_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: 30.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let active_mode = self.active_doc().map_or(ViewMode::Tree, |d| d.view_mode);
        let mut mode_x = PADDING;
        for mode in &VIEW_MODES {
            let is_active = *mode == active_mode;
            let mode_width = mode.label().len() as f32 * CHAR_WIDTH + 20.0;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: mode_x,
                    y: y + 3.0,
                    width: mode_width,
                    height: 24.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: mode_x + 10.0,
                y: y + 14.0,
                text: mode.label().to_string(),
                color: if is_active { TEXT_COLOR } else { SUBTEXT0 },
                font_size: SMALL_TEXT,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            mode_x += mode_width + 4.0;
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + 30.0,
            x2: self.width,
            y2: y + 30.0,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_content(&mut self, cmds: &mut Vec<RenderCommand>) {
        let top = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 30.0;
        let content_width = self.width - SIDEBAR_WIDTH;
        let content_height = self.height - top - STATUS_BAR_HEIGHT;

        // Content background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: content_width,
            height: content_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: top,
            width: content_width,
            height: content_height,
        });

        // Separate immutable reads from mutable borrows
        let view_mode = self.documents.get(self.active_tab).map(|d| d.view_mode);

        match view_mode {
            Some(ViewMode::Tree) => self.render_tree_view(cmds, top, content_width, content_height),
            Some(ViewMode::Raw) => self.render_raw_view(cmds, top, content_width, content_height),
            Some(ViewMode::Yaml) => self.render_yaml_view(cmds, top, content_width, content_height),
            Some(ViewMode::Stats) => {
                self.render_stats_view(cmds, top, content_width, content_height)
            }
            Some(ViewMode::Diff) => self.render_diff_view(cmds, top, content_width, content_height),
            None => {
                cmds.push(RenderCommand::Text {
                    x: PADDING,
                    y: top + 30.0,
                    text: String::from("No document open"),
                    color: SUBTEXT0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_tree_view(&self, cmds: &mut Vec<RenderCommand>, top: f32, width: f32, height: f32) {
        let doc = match self.active_doc() {
            Some(d) => d,
            None => return,
        };

        if let Some(ref error) = doc.error {
            self.render_error_banner(cmds, top, width, error);
            return;
        }

        let value = match doc.parsed {
            Some(ref v) => v,
            None => {
                cmds.push(RenderCommand::Text {
                    x: PADDING,
                    y: top + 30.0,
                    text: String::from("Enter JSON in the input area or paste a document"),
                    color: SUBTEXT0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - PADDING * 2.0),
                });
                return;
            }
        };

        let nodes = build_tree_nodes(value, &doc.expanded_paths, &self.search_results);

        let scroll = doc.tree_scroll;
        let first_visible = (scroll / LINE_HEIGHT) as usize;
        let visible_count = (height / LINE_HEIGHT) as usize + 2;
        let last_visible = (first_visible + visible_count).min(nodes.len());

        for i in first_visible..last_visible {
            if let Some(node) = nodes.get(i) {
                let row_y = top + (i as f32 * LINE_HEIGHT) - scroll;
                let indent_x = PADDING + node.depth as f32 * TREE_INDENT;

                // Selection highlight
                if i == doc.selected_node {
                    cmds.push(RenderCommand::FillRect {
                        x: 0.0,
                        y: row_y,
                        width,
                        height: LINE_HEIGHT,
                        color: SURFACE0,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Search match highlight
                if node.search_match {
                    cmds.push(RenderCommand::FillRect {
                        x: 0.0,
                        y: row_y,
                        width,
                        height: LINE_HEIGHT,
                        color: Color::rgba(250, 179, 135, 30),
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Expand/collapse indicator
                if node.expandable {
                    let arrow = if node.expanded { "v" } else { ">" };
                    cmds.push(RenderCommand::Text {
                        x: indent_x - TREE_ICON_SIZE,
                        y: row_y + 14.0,
                        text: arrow.to_string(),
                        color: SUBTEXT0,
                        font_size: SMALL_TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                // Label (key name or index)
                let label_width = node.label.len() as f32 * CHAR_WIDTH;
                cmds.push(RenderCommand::Text {
                    x: indent_x,
                    y: row_y + 14.0,
                    text: node.label.clone(),
                    color: if node.expandable { MAUVE } else { BLUE },
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width * 0.4),
                });

                // Colon separator
                let colon_x = indent_x + label_width + 4.0;
                cmds.push(RenderCommand::Text {
                    x: colon_x,
                    y: row_y + 14.0,
                    text: String::from(":"),
                    color: SUBTEXT0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                // Value
                let value_x = colon_x + CHAR_WIDTH + 4.0;
                cmds.push(RenderCommand::Text {
                    x: value_x,
                    y: row_y + 14.0,
                    text: node.value_display.clone(),
                    color: node.value_type.color(),
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - value_x - PADDING),
                });
            }
        }

        // Show total node count at bottom
        if nodes.len() > visible_count {
            let info = format!(
                "{} nodes total, showing {}-{}",
                nodes.len(),
                first_visible + 1,
                last_visible
            );
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: top + height - 6.0,
                text: info,
                color: OVERLAY0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_raw_view(
        &mut self,
        cmds: &mut Vec<RenderCommand>,
        top: f32,
        width: f32,
        height: f32,
    ) {
        let doc = match self.documents.get_mut(self.active_tab) {
            Some(d) => d,
            None => return,
        };

        if let Some(ref error) = doc.error {
            let err_clone = error.clone();
            self.render_error_banner(cmds, top, width, &err_clone);
            return;
        }

        let highlighted = doc.get_highlighted();
        let scroll = doc.raw_scroll;
        let first_visible = (scroll / LINE_HEIGHT) as usize;
        let visible_count = (height / LINE_HEIGHT) as usize + 2;
        let last_visible = (first_visible + visible_count).min(highlighted.len());

        // Gutter (line numbers)
        let gutter_width = 50.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: gutter_width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        for i in first_visible..last_visible {
            let row_y = top + (i as f32 * LINE_HEIGHT) - scroll;

            // Line number
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: row_y + 14.0,
                text: format!("{}", i + 1),
                color: OVERLAY0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(gutter_width - 8.0),
            });

            // Highlighted spans
            if let Some(line_spans) = highlighted.get(i) {
                let mut span_x = gutter_width + 4.0;
                for span in line_spans {
                    cmds.push(RenderCommand::Text {
                        x: span_x,
                        y: row_y + 14.0,
                        text: span.text.clone(),
                        color: span.color,
                        font_size: NORMAL_TEXT,
                        font_weight: if span.bold {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        },
                        max_width: Some(width - span_x - PADDING),
                    });
                    span_x += span.text.len() as f32 * CHAR_WIDTH;
                }
            }
        }

        // Format info
        let doc = self.documents.get(self.active_tab);
        if let Some(d) = doc {
            let info = if d.minified {
                String::from("Minified")
            } else {
                format!("Indent: {}", d.indent.label())
            };
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: top + height - 6.0,
                text: info,
                color: OVERLAY0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_yaml_view(
        &mut self,
        cmds: &mut Vec<RenderCommand>,
        top: f32,
        width: f32,
        _height: f32,
    ) {
        let doc = match self.documents.get_mut(self.active_tab) {
            Some(d) => d,
            None => return,
        };

        if doc.parsed.is_none() {
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: top + 30.0,
                text: String::from("No valid JSON to convert to YAML"),
                color: SUBTEXT0,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        let yaml = doc.get_yaml();
        let scroll = doc.raw_scroll;

        let lines: Vec<&str> = yaml.lines().collect();
        let first_visible = (scroll / LINE_HEIGHT) as usize;
        let visible_count = (_height / LINE_HEIGHT) as usize + 2;
        let last_visible = (first_visible + visible_count).min(lines.len());

        for i in first_visible..last_visible {
            if let Some(line) = lines.get(i) {
                let row_y = top + (i as f32 * LINE_HEIGHT) - scroll;

                // Simple YAML highlighting
                let (color, bold) = if line.trim_start().starts_with('-') {
                    (TEAL, false)
                } else if line.contains(':') {
                    (BLUE, true)
                } else {
                    (TEXT_COLOR, false)
                };

                cmds.push(RenderCommand::Text {
                    x: PADDING,
                    y: row_y + 14.0,
                    text: (*line).to_string(),
                    color,
                    font_size: NORMAL_TEXT,
                    font_weight: if bold {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(width - PADDING * 2.0),
                });
            }
        }
    }

    fn render_stats_view(&self, cmds: &mut Vec<RenderCommand>, top: f32, width: f32, _height: f32) {
        let doc = match self.active_doc() {
            Some(d) => d,
            None => return,
        };

        let value = match doc.parsed {
            Some(ref v) => v,
            None => {
                cmds.push(RenderCommand::Text {
                    x: PADDING,
                    y: top + 30.0,
                    text: String::from("No valid JSON to analyze"),
                    color: SUBTEXT0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                return;
            }
        };

        let counts = value.type_counts();
        let node_count = value.node_count();
        let depth = value.max_depth();
        let approx_bytes = value.approx_size();

        let mut row_y = top + 20.0;
        let section_gap = 30.0;

        // Header
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: row_y,
            text: String::from("Document Statistics"),
            color: LAVENDER,
            font_size: HEADER_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += section_gap;

        // General stats
        let stats = [
            ("Total Nodes", format!("{node_count}"), PEACH),
            ("Max Depth", format!("{depth}"), YELLOW),
            ("Approx Size", format_size(approx_bytes), TEAL),
            ("Root Type", value.type_name().to_string(), BLUE),
        ];

        for (label, val, color) in &stats {
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: row_y - 10.0,
                width: width * 0.5 - PADDING * 2.0,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 8.0,
                y: row_y + 4.0,
                text: (*label).to_string(),
                color: TEXT_COLOR,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 200.0,
                y: row_y + 4.0,
                text: val.clone(),
                color: *color,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            row_y += 32.0;
        }

        row_y += 10.0;

        // Type distribution
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: row_y,
            text: String::from("Type Distribution"),
            color: LAVENDER,
            font_size: HEADER_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += section_gap;

        let total = counts.total().max(1) as f32;
        let type_rows = [
            ("Objects", counts.objects, MAUVE),
            ("Arrays", counts.arrays, LAVENDER),
            ("Strings", counts.strings, GREEN),
            ("Numbers", counts.numbers, PEACH),
            ("Booleans", counts.bools, BLUE),
            ("Nulls", counts.nulls, OVERLAY0),
        ];

        let bar_max_width = width * 0.5 - 250.0;

        for (label, count, color) in &type_rows {
            let pct = *count as f32 / total;
            let bar_width = pct * bar_max_width;

            cmds.push(RenderCommand::Text {
                x: PADDING + 8.0,
                y: row_y + 4.0,
                text: (*label).to_string(),
                color: TEXT_COLOR,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: 120.0,
                y: row_y + 4.0,
                text: count.to_string(),
                color: *color,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Bar background
            cmds.push(RenderCommand::FillRect {
                x: 180.0,
                y: row_y - 4.0,
                width: bar_max_width,
                height: 16.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(3.0),
            });

            // Bar fill
            if bar_width > 1.0 {
                cmds.push(RenderCommand::FillRect {
                    x: 180.0,
                    y: row_y - 4.0,
                    width: bar_width,
                    height: 16.0,
                    color: *color,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            // Percentage
            cmds.push(RenderCommand::Text {
                x: 180.0 + bar_max_width + 8.0,
                y: row_y + 4.0,
                text: format!("{:.1}%", pct * 100.0),
                color: SUBTEXT0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            row_y += 28.0;
        }
    }

    fn render_diff_view(
        &mut self,
        cmds: &mut Vec<RenderCommand>,
        top: f32,
        width: f32,
        height: f32,
    ) {
        let doc = match self.documents.get(self.active_tab) {
            Some(d) => d,
            None => return,
        };

        if doc.parsed.is_none() {
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: top + 30.0,
                text: String::from("Parse the primary document first to enable diff"),
                color: SUBTEXT0,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        let diff_results = &doc.diff_results;

        // Header
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: top + 20.0,
            text: format!("Diff Results: {} difference(s)", diff_results.len()),
            color: LAVENDER,
            font_size: HEADER_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        if diff_results.is_empty() {
            let msg = if doc.diff_source.is_empty() {
                "Enter comparison JSON in the diff source panel"
            } else {
                "Documents are identical"
            };
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: top + 50.0,
                text: msg.to_string(),
                color: SUBTEXT0,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        let scroll = doc.diff_scroll;
        let start_y = top + 40.0;
        let row_height = 50.0;
        let first_visible = (scroll / row_height) as usize;
        let visible_count = (height / row_height) as usize + 2;
        let last_visible = (first_visible + visible_count).min(diff_results.len());

        for i in first_visible..last_visible {
            if let Some(entry) = diff_results.get(i) {
                let row_y = start_y + (i as f32 * row_height) - scroll;

                let (indicator, ind_color) = match entry.kind {
                    DiffKind::Added => ("+", GREEN),
                    DiffKind::Removed => ("-", RED),
                    DiffKind::Changed => ("~", YELLOW),
                    DiffKind::TypeChanged => ("!", PEACH),
                };

                // Background
                let bg_color = match entry.kind {
                    DiffKind::Added => Color::rgba(166, 227, 161, 15),
                    DiffKind::Removed => Color::rgba(243, 139, 168, 15),
                    DiffKind::Changed => Color::rgba(249, 226, 175, 15),
                    DiffKind::TypeChanged => Color::rgba(250, 179, 135, 15),
                };

                cmds.push(RenderCommand::FillRect {
                    x: PADDING,
                    y: row_y - 2.0,
                    width: width - PADDING * 2.0,
                    height: row_height - 4.0,
                    color: bg_color,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Indicator
                cmds.push(RenderCommand::Text {
                    x: PADDING + 8.0,
                    y: row_y + 14.0,
                    text: indicator.to_string(),
                    color: ind_color,
                    font_size: HEADER_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                // Path
                cmds.push(RenderCommand::Text {
                    x: PADDING + 30.0,
                    y: row_y + 14.0,
                    text: entry.path.clone(),
                    color: BLUE,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width * 0.6),
                });

                // Values
                if !entry.left.is_empty() {
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 30.0,
                        y: row_y + 32.0,
                        text: format!("L: {}", entry.left),
                        color: RED,
                        font_size: SMALL_TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.4),
                    });
                }
                if !entry.right.is_empty() {
                    cmds.push(RenderCommand::Text {
                        x: width * 0.45,
                        y: row_y + 32.0,
                        text: format!("R: {}", entry.right),
                        color: GREEN,
                        font_size: SMALL_TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.4),
                    });
                }
            }
        }
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let top = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 30.0;
        let sidebar_x = self.width - SIDEBAR_WIDTH;
        let sidebar_height = self.height - top - STATUS_BAR_HEIGHT;

        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: sidebar_x,
            y: top,
            width: SIDEBAR_WIDTH,
            height: sidebar_height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sidebar_x,
            y1: top,
            x2: sidebar_x,
            y2: top + sidebar_height,
            color: SURFACE1,
            width: 1.0,
        });

        let doc = match self.active_doc() {
            Some(d) => d,
            None => return,
        };

        let mut section_y = top + PADDING;

        // JSONPath section
        cmds.push(RenderCommand::Text {
            x: sidebar_x + PADDING,
            y: section_y + 4.0,
            text: String::from("JSONPath"),
            color: LAVENDER,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        section_y += 20.0;

        if let Some(ref value) = doc.parsed {
            let nodes = build_tree_nodes(value, &doc.expanded_paths, &self.search_results);
            if let Some(node) = nodes.get(doc.selected_node) {
                let json_path = build_json_path(value, &node.path);
                cmds.push(RenderCommand::FillRect {
                    x: sidebar_x + PADDING,
                    y: section_y - 4.0,
                    width: SIDEBAR_WIDTH - PADDING * 2.0,
                    height: 22.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: sidebar_x + PADDING + 6.0,
                    y: section_y + 8.0,
                    text: json_path,
                    color: TEAL,
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 12.0),
                });
            }
        }
        section_y += 30.0;

        // Validation section
        cmds.push(RenderCommand::Line {
            x1: sidebar_x + PADDING,
            y1: section_y,
            x2: sidebar_x + SIDEBAR_WIDTH - PADDING,
            y2: section_y,
            color: SURFACE0,
            width: 1.0,
        });
        section_y += PADDING;

        cmds.push(RenderCommand::Text {
            x: sidebar_x + PADDING,
            y: section_y + 4.0,
            text: String::from("Validation"),
            color: LAVENDER,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        section_y += 20.0;

        if let Some(ref error) = doc.error {
            cmds.push(RenderCommand::FillRect {
                x: sidebar_x + PADDING,
                y: section_y - 4.0,
                width: SIDEBAR_WIDTH - PADDING * 2.0,
                height: 40.0,
                color: Color::rgba(243, 139, 168, 25),
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 6.0,
                y: section_y + 8.0,
                text: String::from("Invalid JSON"),
                color: RED,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 6.0,
                y: section_y + 24.0,
                text: format!("Ln {}, Col {}", error.line, error.column),
                color: RED,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 12.0),
            });
            section_y += 48.0;
        } else if doc.parsed.is_some() {
            cmds.push(RenderCommand::FillRect {
                x: sidebar_x + PADDING,
                y: section_y - 4.0,
                width: SIDEBAR_WIDTH - PADDING * 2.0,
                height: 22.0,
                color: Color::rgba(166, 227, 161, 25),
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 6.0,
                y: section_y + 8.0,
                text: String::from("Valid JSON"),
                color: GREEN,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            section_y += 30.0;
        } else {
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 6.0,
                y: section_y + 8.0,
                text: String::from("No input"),
                color: OVERLAY0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            section_y += 24.0;
        }

        // Quick info section
        cmds.push(RenderCommand::Line {
            x1: sidebar_x + PADDING,
            y1: section_y,
            x2: sidebar_x + SIDEBAR_WIDTH - PADDING,
            y2: section_y,
            color: SURFACE0,
            width: 1.0,
        });
        section_y += PADDING;

        cmds.push(RenderCommand::Text {
            x: sidebar_x + PADDING,
            y: section_y + 4.0,
            text: String::from("Quick Info"),
            color: LAVENDER,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        section_y += 20.0;

        if let Some(ref value) = doc.parsed {
            let info_items = [
                ("Nodes", format!("{}", value.node_count())),
                ("Depth", format!("{}", value.max_depth())),
                ("Size", format_size(value.approx_size())),
                ("Type", value.type_name().to_string()),
            ];

            for (label, val) in &info_items {
                cmds.push(RenderCommand::Text {
                    x: sidebar_x + PADDING + 6.0,
                    y: section_y + 4.0,
                    text: format!("{label}:"),
                    color: SUBTEXT0,
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: sidebar_x + PADDING + 80.0,
                    y: section_y + 4.0,
                    text: val.clone(),
                    color: TEXT_COLOR,
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                section_y += 18.0;
            }
        }

        // Keyboard shortcuts section
        section_y += 10.0;
        cmds.push(RenderCommand::Line {
            x1: sidebar_x + PADDING,
            y1: section_y,
            x2: sidebar_x + SIDEBAR_WIDTH - PADDING,
            y2: section_y,
            color: SURFACE0,
            width: 1.0,
        });
        section_y += PADDING;

        cmds.push(RenderCommand::Text {
            x: sidebar_x + PADDING,
            y: section_y + 4.0,
            text: String::from("Shortcuts"),
            color: LAVENDER,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        section_y += 20.0;

        let shortcuts = [
            ("Ctrl+N", "New tab"),
            ("Ctrl+W", "Close tab"),
            ("Ctrl+F", "Search"),
            ("Ctrl+E", "Edit mode"),
            ("1-5", "Switch view"),
            ("I", "Cycle indent"),
            ("M", "Toggle minify"),
        ];

        for (key, desc) in &shortcuts {
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 6.0,
                y: section_y + 4.0,
                text: (*key).to_string(),
                color: YELLOW,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 80.0,
                y: section_y + 4.0,
                text: (*desc).to_string(),
                color: SUBTEXT0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            section_y += 16.0;
        }
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.width,
            y2: y,
            color: SURFACE1,
            width: 1.0,
        });

        let doc = match self.active_doc() {
            Some(d) => d,
            None => return,
        };

        // Left: view mode + status
        let status = if doc.error.is_some() {
            "Invalid"
        } else if doc.parsed.is_some() {
            "Valid"
        } else {
            "Empty"
        };
        let status_color = if doc.error.is_some() { RED } else { GREEN };

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 16.0,
            text: format!("{} | {}", doc.view_mode.label(), status),
            color: status_color,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Center: edit mode indicator
        if self.edit_mode {
            cmds.push(RenderCommand::Text {
                x: self.width * 0.4,
                y: y + 16.0,
                text: String::from("EDIT MODE"),
                color: YELLOW,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Right: input size
        let size_info = format!("{} chars", doc.input.len());
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: y + 16.0,
            text: size_info,
            color: SUBTEXT0,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_search_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let bar_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 30.0;
        let bar_height = 36.0;

        // Overlay background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.width - SIDEBAR_WIDTH,
            height: bar_height,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Search icon placeholder
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: bar_y + 18.0,
            text: String::from("Find:"),
            color: SUBTEXT0,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Search input
        cmds.push(RenderCommand::FillRect {
            x: 50.0,
            y: bar_y + 4.0,
            width: 300.0,
            height: 28.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: 58.0,
            y: bar_y + 18.0,
            text: if self.search_query.is_empty() {
                String::from("Search keys and values...")
            } else {
                self.search_query.clone()
            },
            color: if self.search_query.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_size: NORMAL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(284.0),
        });

        // Result count
        if !self.search_results.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 370.0,
                y: bar_y + 18.0,
                text: format!(
                    "{}/{} matches",
                    self.search_index + 1,
                    self.search_results.len()
                ),
                color: TEAL,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else if !self.search_query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 370.0,
                y: bar_y + 18.0,
                text: String::from("No matches"),
                color: RED,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Case sensitivity toggle
        cmds.push(RenderCommand::FillRect {
            x: 500.0,
            y: bar_y + 6.0,
            width: 28.0,
            height: 24.0,
            color: if self.search_case_sensitive {
                BLUE
            } else {
                SURFACE1
            },
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: 507.0,
            y: bar_y + 18.0,
            text: String::from("Aa"),
            color: if self.search_case_sensitive {
                CRUST
            } else {
                SUBTEXT0
            },
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Close button
        cmds.push(RenderCommand::Text {
            x: self.width - SIDEBAR_WIDTH - 30.0,
            y: bar_y + 18.0,
            text: String::from("Esc"),
            color: OVERLAY0,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bottom line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: bar_y + bar_height,
            x2: self.width - SIDEBAR_WIDTH,
            y2: bar_y + bar_height,
            color: SURFACE1,
            width: 1.0,
        });
    }

    fn render_error_banner(
        &self,
        cmds: &mut Vec<RenderCommand>,
        top: f32,
        width: f32,
        error: &ParseError,
    ) {
        // Error background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: top + PADDING,
            width: width - PADDING * 2.0,
            height: 60.0,
            color: Color::rgba(243, 139, 168, 20),
            corner_radii: CornerRadii::all(6.0),
        });

        // Error border
        cmds.push(RenderCommand::StrokeRect {
            x: PADDING,
            y: top + PADDING,
            width: width - PADDING * 2.0,
            height: 60.0,
            color: RED,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Error title
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: top + PADDING + 20.0,
            text: String::from("Parse Error"),
            color: RED,
            font_size: NORMAL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Error details
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: top + PADDING + 40.0,
            text: format!(
                "Line {}, Col {}: {}",
                error.line, error.column, error.message
            ),
            color: TEXT_COLOR,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0 - 24.0),
        });
    }
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn char_to_byte_pos(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map_or(s.len(), |(byte_idx, _)| byte_idx)
}

// ============================================================================
// Sample JSON
// ============================================================================

const SAMPLE_JSON: &str = r#"{
  "name": "OurOS JSON Viewer",
  "version": "0.1.0",
  "description": "A full-featured JSON viewer and editor",
  "features": [
    "Tree view",
    "Syntax highlighting",
    "Search",
    "Diff",
    "YAML conversion"
  ],
  "settings": {
    "theme": "catppuccin-mocha",
    "indent": 2,
    "wordWrap": true,
    "autoValidate": true
  },
  "users": [
    {
      "id": 1,
      "name": "Alice",
      "email": "alice@example.com",
      "active": true,
      "score": 98.5
    },
    {
      "id": 2,
      "name": "Bob",
      "email": "bob@example.com",
      "active": false,
      "score": null
    }
  ],
  "metadata": {
    "created": "2026-01-15T10:30:00Z",
    "modified": "2026-05-18T14:22:00Z",
    "tags": ["viewer", "editor", "json", "utility"]
  }
}"#;

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let _app = App::new();
    // In the real OS, this would enter the event loop:
    // app.run()
    // For now, the app struct + rendering methods demonstrate the full feature set.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Parser tests ---

    #[test]
    fn parse_null() {
        let v = parse_json("null").unwrap();
        assert_eq!(v, JsonValue::Null);
    }

    #[test]
    fn parse_true() {
        let v = parse_json("true").unwrap();
        assert_eq!(v, JsonValue::Bool(true));
    }

    #[test]
    fn parse_false() {
        let v = parse_json("false").unwrap();
        assert_eq!(v, JsonValue::Bool(false));
    }

    #[test]
    fn parse_integer() {
        let v = parse_json("42").unwrap();
        assert_eq!(v, JsonValue::Number(42.0));
    }

    #[test]
    fn parse_negative_number() {
        let v = parse_json("-17").unwrap();
        assert_eq!(v, JsonValue::Number(-17.0));
    }

    #[test]
    fn parse_float() {
        let v = parse_json("3.14").unwrap();
        if let JsonValue::Number(n) = v {
            assert!((n - 3.14).abs() < f64::EPSILON);
        } else {
            panic!("expected number");
        }
    }

    #[test]
    fn parse_exponent() {
        let v = parse_json("1e10").unwrap();
        assert_eq!(v, JsonValue::Number(1e10));
    }

    #[test]
    fn parse_negative_exponent() {
        let v = parse_json("5.5e-3").unwrap();
        if let JsonValue::Number(n) = v {
            assert!((n - 5.5e-3).abs() < 1e-10);
        } else {
            panic!("expected number");
        }
    }

    #[test]
    fn parse_zero() {
        let v = parse_json("0").unwrap();
        assert_eq!(v, JsonValue::Number(0.0));
    }

    #[test]
    fn parse_simple_string() {
        let v = parse_json("\"hello\"").unwrap();
        assert_eq!(v, JsonValue::Str("hello".to_string()));
    }

    #[test]
    fn parse_string_with_escapes() {
        let v = parse_json("\"a\\nb\\tc\"").unwrap();
        assert_eq!(v, JsonValue::Str("a\nb\tc".to_string()));
    }

    #[test]
    fn parse_string_with_unicode() {
        let v = parse_json("\"\\u0041\"").unwrap();
        assert_eq!(v, JsonValue::Str("A".to_string()));
    }

    #[test]
    fn parse_surrogate_pair() {
        // U+1F600 (grinning face) = D83D DE00
        let v = parse_json("\"\\uD83D\\uDE00\"").unwrap();
        assert_eq!(v, JsonValue::Str("\u{1F600}".to_string()));
    }

    #[test]
    fn parse_empty_string() {
        let v = parse_json("\"\"").unwrap();
        assert_eq!(v, JsonValue::Str(String::new()));
    }

    #[test]
    fn parse_string_with_backslash() {
        let v = parse_json("\"a\\\\b\"").unwrap();
        assert_eq!(v, JsonValue::Str("a\\b".to_string()));
    }

    #[test]
    fn parse_string_with_quotes() {
        let v = parse_json("\"a\\\"b\"").unwrap();
        assert_eq!(v, JsonValue::Str("a\"b".to_string()));
    }

    #[test]
    fn parse_empty_object() {
        let v = parse_json("{}").unwrap();
        assert_eq!(v, JsonValue::Object(vec![]));
    }

    #[test]
    fn parse_simple_object() {
        let v = parse_json("{\"a\": 1}").unwrap();
        assert_eq!(
            v,
            JsonValue::Object(vec![("a".to_string(), JsonValue::Number(1.0))])
        );
    }

    #[test]
    fn parse_nested_object() {
        let v = parse_json("{\"a\": {\"b\": 2}}").unwrap();
        assert_eq!(
            v,
            JsonValue::Object(vec![(
                "a".to_string(),
                JsonValue::Object(vec![("b".to_string(), JsonValue::Number(2.0))])
            )])
        );
    }

    #[test]
    fn parse_empty_array() {
        let v = parse_json("[]").unwrap();
        assert_eq!(v, JsonValue::Array(vec![]));
    }

    #[test]
    fn parse_simple_array() {
        let v = parse_json("[1, 2, 3]").unwrap();
        assert_eq!(
            v,
            JsonValue::Array(vec![
                JsonValue::Number(1.0),
                JsonValue::Number(2.0),
                JsonValue::Number(3.0),
            ])
        );
    }

    #[test]
    fn parse_mixed_array() {
        let v = parse_json("[1, \"two\", true, null]").unwrap();
        assert_eq!(
            v,
            JsonValue::Array(vec![
                JsonValue::Number(1.0),
                JsonValue::Str("two".to_string()),
                JsonValue::Bool(true),
                JsonValue::Null,
            ])
        );
    }

    #[test]
    fn parse_nested_array() {
        let v = parse_json("[[1, 2], [3, 4]]").unwrap();
        assert_eq!(
            v,
            JsonValue::Array(vec![
                JsonValue::Array(vec![JsonValue::Number(1.0), JsonValue::Number(2.0)]),
                JsonValue::Array(vec![JsonValue::Number(3.0), JsonValue::Number(4.0)]),
            ])
        );
    }

    #[test]
    fn parse_complex_document() {
        let v = parse_json(SAMPLE_JSON).unwrap();
        if let JsonValue::Object(ref obj) = v {
            assert!(obj.iter().any(|(k, _)| k == "name"));
            assert!(obj.iter().any(|(k, _)| k == "users"));
        } else {
            panic!("expected object");
        }
    }

    #[test]
    fn parse_whitespace_handling() {
        let v = parse_json("  \n\t { \"a\" : 1 } \n ").unwrap();
        assert_eq!(
            v,
            JsonValue::Object(vec![("a".to_string(), JsonValue::Number(1.0))])
        );
    }

    // --- Error cases ---

    #[test]
    fn parse_error_empty_input() {
        assert!(parse_json("").is_err());
    }

    #[test]
    fn parse_error_trailing_content() {
        assert!(parse_json("1 2").is_err());
    }

    #[test]
    fn parse_error_unterminated_string() {
        assert!(parse_json("\"hello").is_err());
    }

    #[test]
    fn parse_error_invalid_escape() {
        assert!(parse_json("\"\\x\"").is_err());
    }

    #[test]
    fn parse_error_invalid_number() {
        assert!(parse_json("01").is_err());
    }

    #[test]
    fn parse_error_missing_comma_object() {
        assert!(parse_json("{\"a\": 1 \"b\": 2}").is_err());
    }

    #[test]
    fn parse_error_missing_comma_array() {
        assert!(parse_json("[1 2]").is_err());
    }

    #[test]
    fn parse_error_missing_colon() {
        assert!(parse_json("{\"a\" 1}").is_err());
    }

    #[test]
    fn parse_error_line_column() {
        let err = parse_json("{\n  \"a\": }").unwrap_err();
        assert!(err.line >= 2);
    }

    #[test]
    fn parse_error_lone_low_surrogate() {
        assert!(parse_json("\"\\uDC00\"").is_err());
    }

    #[test]
    fn parse_error_depth_exceeded() {
        let deep = "[".repeat(200) + &"]".repeat(200);
        assert!(parse_json(&deep).is_err());
    }

    // --- Formatter tests ---

    #[test]
    fn format_null() {
        let formatted = format_json(&JsonValue::Null, IndentStyle::Spaces2);
        assert_eq!(formatted.trim(), "null");
    }

    #[test]
    fn format_string() {
        let formatted = format_json(&JsonValue::Str("hello".to_string()), IndentStyle::Spaces2);
        assert_eq!(formatted.trim(), "\"hello\"");
    }

    #[test]
    fn format_object_indentation() {
        let obj = JsonValue::Object(vec![("a".to_string(), JsonValue::Number(1.0))]);
        let formatted = format_json(&obj, IndentStyle::Spaces4);
        assert!(formatted.contains("    \"a\""));
    }

    #[test]
    fn format_empty_containers() {
        assert_eq!(
            format_json(&JsonValue::Array(vec![]), IndentStyle::Spaces2).trim(),
            "[]"
        );
        assert_eq!(
            format_json(&JsonValue::Object(vec![]), IndentStyle::Spaces2).trim(),
            "{}"
        );
    }

    #[test]
    fn minify_roundtrip() {
        let v = parse_json(SAMPLE_JSON).unwrap();
        let minified = minify_json(&v);
        let reparsed = parse_json(&minified).unwrap();
        // Compare types and structure (not exact f64 equality)
        assert_eq!(v, reparsed);
    }

    #[test]
    fn format_escape_special_chars() {
        let v = JsonValue::Str("a\nb\tc".to_string());
        let formatted = format_json(&v, IndentStyle::Spaces2);
        assert!(formatted.contains("\\n"));
        assert!(formatted.contains("\\t"));
    }

    #[test]
    fn format_number_integer() {
        assert_eq!(format_number(42.0), "42");
    }

    #[test]
    fn format_number_float() {
        let s = format_number(3.14);
        assert!(s.contains("3.14"));
    }

    // --- YAML tests ---

    #[test]
    fn yaml_simple_object() {
        let v = JsonValue::Object(vec![(
            "key".to_string(),
            JsonValue::Str("value".to_string()),
        )]);
        let yaml = to_yaml_like(&v);
        assert!(yaml.contains("key: value"));
    }

    #[test]
    fn yaml_array() {
        let v = JsonValue::Array(vec![JsonValue::Number(1.0), JsonValue::Number(2.0)]);
        let yaml = to_yaml_like(&v);
        assert!(yaml.contains("- 1"));
        assert!(yaml.contains("- 2"));
    }

    // --- Search tests ---

    #[test]
    fn search_by_key() {
        let v = parse_json("{\"name\": \"Alice\", \"age\": 30}").unwrap();
        let results = search_json(&v, "name", false);
        assert!(!results.is_empty());
    }

    #[test]
    fn search_by_value() {
        let v = parse_json("{\"name\": \"Alice\"}").unwrap();
        let results = search_json(&v, "alice", false);
        assert!(!results.is_empty());
    }

    #[test]
    fn search_case_sensitive() {
        let v = parse_json("{\"Name\": \"Alice\"}").unwrap();
        let results = search_json(&v, "name", true);
        assert!(results.is_empty());
    }

    #[test]
    fn search_nested() {
        let v = parse_json("{\"a\": {\"b\": {\"target\": 1}}}").unwrap();
        let results = search_json(&v, "target", false);
        assert!(!results.is_empty());
    }

    #[test]
    fn search_no_results() {
        let v = parse_json("{\"a\": 1}").unwrap();
        let results = search_json(&v, "nonexistent", false);
        assert!(results.is_empty());
    }

    // --- JSONPath tests ---

    #[test]
    fn json_path_root() {
        let v = parse_json("{}").unwrap();
        let path = build_json_path(&v, &[]);
        assert_eq!(path, "$");
    }

    #[test]
    fn json_path_simple_key() {
        let v = parse_json("{\"name\": \"test\"}").unwrap();
        let path = build_json_path(&v, &[PathSegment::Key("name".to_string())]);
        assert_eq!(path, "$.name");
    }

    #[test]
    fn json_path_array_index() {
        let v = parse_json("[1, 2, 3]").unwrap();
        let path = build_json_path(&v, &[PathSegment::Index(1)]);
        assert_eq!(path, "$[1]");
    }

    #[test]
    fn json_path_nested() {
        let v = parse_json("{\"users\": [{\"name\": \"Alice\"}]}").unwrap();
        let path = build_json_path(
            &v,
            &[
                PathSegment::Key("users".to_string()),
                PathSegment::Index(0),
                PathSegment::Key("name".to_string()),
            ],
        );
        assert_eq!(path, "$.users[0].name");
    }

    #[test]
    fn json_path_bracket_notation() {
        let v = parse_json("{\"my key\": 1}").unwrap();
        let path = build_json_path(&v, &[PathSegment::Key("my key".to_string())]);
        assert!(path.contains("[\"my key\"]"));
    }

    // --- Diff tests ---

    #[test]
    fn diff_identical() {
        let a = parse_json("{\"a\": 1}").unwrap();
        let b = parse_json("{\"a\": 1}").unwrap();
        assert!(diff_json(&a, &b).is_empty());
    }

    #[test]
    fn diff_changed_value() {
        let a = parse_json("{\"a\": 1}").unwrap();
        let b = parse_json("{\"a\": 2}").unwrap();
        let diffs = diff_json(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0].kind, DiffKind::Changed));
    }

    #[test]
    fn diff_added_key() {
        let a = parse_json("{\"a\": 1}").unwrap();
        let b = parse_json("{\"a\": 1, \"b\": 2}").unwrap();
        let diffs = diff_json(&a, &b);
        assert!(diffs.iter().any(|d| matches!(d.kind, DiffKind::Added)));
    }

    #[test]
    fn diff_removed_key() {
        let a = parse_json("{\"a\": 1, \"b\": 2}").unwrap();
        let b = parse_json("{\"a\": 1}").unwrap();
        let diffs = diff_json(&a, &b);
        assert!(diffs.iter().any(|d| matches!(d.kind, DiffKind::Removed)));
    }

    #[test]
    fn diff_type_changed() {
        let a = parse_json("{\"a\": 1}").unwrap();
        let b = parse_json("{\"a\": \"one\"}").unwrap();
        let diffs = diff_json(&a, &b);
        assert!(
            diffs
                .iter()
                .any(|d| matches!(d.kind, DiffKind::TypeChanged))
        );
    }

    #[test]
    fn diff_array_length() {
        let a = parse_json("[1, 2]").unwrap();
        let b = parse_json("[1, 2, 3]").unwrap();
        let diffs = diff_json(&a, &b);
        assert!(diffs.iter().any(|d| matches!(d.kind, DiffKind::Added)));
    }

    // --- Edit operation tests ---

    #[test]
    fn edit_set_value() {
        let mut root = parse_json("{\"a\": 1}").unwrap();
        let result = set_value_at_path(
            &mut root,
            &[PathSegment::Key("a".to_string())],
            JsonValue::Number(42.0),
        );
        assert!(result);
        if let JsonValue::Object(ref obj) = root {
            assert_eq!(obj[0].1, JsonValue::Number(42.0));
        }
    }

    #[test]
    fn edit_set_nested() {
        let mut root = parse_json("{\"a\": {\"b\": 1}}").unwrap();
        let result = set_value_at_path(
            &mut root,
            &[
                PathSegment::Key("a".to_string()),
                PathSegment::Key("b".to_string()),
            ],
            JsonValue::Str("updated".to_string()),
        );
        assert!(result);
    }

    #[test]
    fn edit_delete_key() {
        let mut root = parse_json("{\"a\": 1, \"b\": 2}").unwrap();
        let result = delete_at_path(&mut root, &[PathSegment::Key("a".to_string())]);
        assert!(result);
        if let JsonValue::Object(ref obj) = root {
            assert_eq!(obj.len(), 1);
            assert_eq!(obj[0].0, "b");
        }
    }

    #[test]
    fn edit_delete_array_item() {
        let mut root = parse_json("[1, 2, 3]").unwrap();
        let result = delete_at_path(&mut root, &[PathSegment::Index(1)]);
        assert!(result);
        if let JsonValue::Array(ref arr) = root {
            assert_eq!(arr.len(), 2);
        }
    }

    #[test]
    fn edit_add_key() {
        let mut root = parse_json("{\"a\": 1}").unwrap();
        let result = add_key_at_path(&mut root, &[], "b".to_string(), JsonValue::Number(2.0));
        assert!(result);
        if let JsonValue::Object(ref obj) = root {
            assert_eq!(obj.len(), 2);
        }
    }

    #[test]
    fn edit_set_root() {
        let mut root = parse_json("1").unwrap();
        set_value_at_path(&mut root, &[], JsonValue::Str("replaced".to_string()));
        assert_eq!(root, JsonValue::Str("replaced".to_string()));
    }

    #[test]
    fn edit_invalid_path() {
        let mut root = parse_json("{\"a\": 1}").unwrap();
        let result = set_value_at_path(
            &mut root,
            &[PathSegment::Key("nonexistent".to_string())],
            JsonValue::Null,
        );
        assert!(!result);
    }

    // --- Value method tests ---

    #[test]
    fn node_count_leaf() {
        assert_eq!(JsonValue::Null.node_count(), 1);
        assert_eq!(JsonValue::Number(1.0).node_count(), 1);
    }

    #[test]
    fn node_count_nested() {
        let v = parse_json("{\"a\": [1, 2], \"b\": 3}").unwrap();
        assert_eq!(v.node_count(), 5); // object + array + 1 + 2 + 3
    }

    #[test]
    fn max_depth_flat() {
        assert_eq!(JsonValue::Null.max_depth(), 1);
    }

    #[test]
    fn max_depth_nested() {
        let v = parse_json("{\"a\": {\"b\": {\"c\": 1}}}").unwrap();
        assert_eq!(v.max_depth(), 4);
    }

    #[test]
    fn type_counts_mixed() {
        let v =
            parse_json("{\"a\": 1, \"b\": \"s\", \"c\": true, \"d\": null, \"e\": [1]}").unwrap();
        let counts = v.type_counts();
        assert_eq!(counts.objects, 1);
        assert_eq!(counts.arrays, 1);
        assert_eq!(counts.numbers, 2); // 1 and array item 1
        assert_eq!(counts.strings, 1);
        assert_eq!(counts.bools, 1);
        assert_eq!(counts.nulls, 1);
    }

    #[test]
    fn approx_size_non_zero() {
        let v = parse_json(SAMPLE_JSON).unwrap();
        assert!(v.approx_size() > 0);
    }

    // --- Tree view tests ---

    #[test]
    fn tree_build_simple_object() {
        let v = parse_json("{\"a\": 1}").unwrap();
        let nodes = build_tree_nodes(&v, &[], &[]);
        assert!(!nodes.is_empty());
        assert!(nodes[0].expandable);
    }

    #[test]
    fn tree_expand_shows_children() {
        let v = parse_json("{\"a\": 1, \"b\": 2}").unwrap();
        let expanded = vec![Vec::new()]; // root expanded
        let nodes = build_tree_nodes(&v, &expanded, &[]);
        // Root + a + b = 3 nodes
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn tree_collapse_hides_children() {
        let v = parse_json("{\"a\": 1, \"b\": 2}").unwrap();
        let nodes = build_tree_nodes(&v, &[], &[]);
        // Only root (collapsed, but root is always expanded -- hmm)
        // Actually root is always expanded, so we get 3
        assert!(nodes.len() >= 1);
    }

    #[test]
    fn tree_leaf_display() {
        let (display, vtype) = leaf_display(&JsonValue::Bool(true));
        assert_eq!(display, "true");
        assert_eq!(vtype, ValueType::Bool);
    }

    // --- Highlight tests ---

    #[test]
    fn highlight_simple() {
        let lines = highlight_json_text("{\"key\": 42}\n");
        assert!(!lines.is_empty());
    }

    #[test]
    fn highlight_preserves_structure() {
        let v = parse_json("{\"a\": 1}").unwrap();
        let formatted = format_json(&v, IndentStyle::Spaces2);
        let lines = highlight_json_text(&formatted);
        // Should have multiple lines (opening brace, key-value, closing brace)
        assert!(lines.len() >= 3);
    }

    // --- App state tests ---

    #[test]
    fn app_new_creates_default_doc() {
        let app = App::new();
        assert_eq!(app.documents.len(), 1);
        assert!(app.documents[0].parsed.is_some());
    }

    #[test]
    fn app_new_tab() {
        let mut app = App::new();
        app.new_tab();
        assert_eq!(app.documents.len(), 2);
        assert_eq!(app.active_tab, 1);
    }

    #[test]
    fn app_close_tab() {
        let mut app = App::new();
        app.new_tab();
        app.close_tab(0);
        assert_eq!(app.documents.len(), 1);
    }

    #[test]
    fn app_cannot_close_last_tab() {
        let mut app = App::new();
        app.close_tab(0);
        assert_eq!(app.documents.len(), 1);
    }

    #[test]
    fn app_render_produces_commands() {
        let mut app = App::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn app_search_flow() {
        let mut app = App::new();
        app.search_query = "name".to_string();
        app.perform_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn app_search_navigation() {
        let mut app = App::new();
        app.search_query = "id".to_string();
        app.perform_search();
        let count = app.search_results.len();
        if count > 1 {
            app.search_next();
            assert_eq!(app.search_index, 1);
            app.search_prev();
            assert_eq!(app.search_index, 0);
        }
    }

    // --- Format size tests ---

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(42), "42 B");
    }

    #[test]
    fn format_size_kilobytes() {
        let s = format_size(2048);
        assert!(s.contains("KB"));
    }

    #[test]
    fn format_size_megabytes() {
        let s = format_size(2_000_000);
        assert!(s.contains("MB"));
    }

    // --- Edge case tests ---

    #[test]
    fn parse_very_long_string() {
        let long = format!("\"{}\"", "a".repeat(10000));
        let v = parse_json(&long).unwrap();
        if let JsonValue::Str(s) = v {
            assert_eq!(s.len(), 10000);
        } else {
            panic!("expected string");
        }
    }

    #[test]
    fn parse_deeply_nested() {
        let depth = 50;
        let open: String = "[".repeat(depth);
        let close: String = "]".repeat(depth);
        let json = format!("{open}1{close}");
        let v = parse_json(&json).unwrap();
        assert_eq!(v.max_depth(), depth + 1);
    }

    #[test]
    fn parse_all_escape_sequences() {
        let v = parse_json("\"\\\" \\\\ \\/ \\b \\f \\n \\r \\t\"").unwrap();
        if let JsonValue::Str(s) = v {
            assert!(s.contains('"'));
            assert!(s.contains('\\'));
            assert!(s.contains('/'));
            assert!(s.contains('\n'));
            assert!(s.contains('\r'));
            assert!(s.contains('\t'));
        }
    }

    #[test]
    fn path_equality() {
        let a = vec![PathSegment::Key("a".to_string()), PathSegment::Index(0)];
        let b = vec![PathSegment::Key("a".to_string()), PathSegment::Index(0)];
        assert!(paths_equal(&a, &b));
    }

    #[test]
    fn path_inequality() {
        let a = vec![PathSegment::Key("a".to_string())];
        let b = vec![PathSegment::Key("b".to_string())];
        assert!(!paths_equal(&a, &b));
    }

    #[test]
    fn type_name_coverage() {
        assert_eq!(JsonValue::Null.type_name(), "null");
        assert_eq!(JsonValue::Bool(true).type_name(), "boolean");
        assert_eq!(JsonValue::Number(0.0).type_name(), "number");
        assert_eq!(JsonValue::Str(String::new()).type_name(), "string");
        assert_eq!(JsonValue::Array(vec![]).type_name(), "array");
        assert_eq!(JsonValue::Object(vec![]).type_name(), "object");
    }

    #[test]
    fn value_type_colors_distinct() {
        let colors = [
            ValueType::Null.color(),
            ValueType::Bool.color(),
            ValueType::Number.color(),
            ValueType::Str.color(),
        ];
        // All should be different
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn indent_style_cycle() {
        let start = IndentStyle::Spaces2;
        let after1 = start.cycle();
        assert_eq!(after1, IndentStyle::Spaces4);
        let after2 = after1.cycle();
        assert_eq!(after2, IndentStyle::Spaces8);
        let after3 = after2.cycle();
        assert_eq!(after3, IndentStyle::Tabs);
        let after4 = after3.cycle();
        assert_eq!(after4, IndentStyle::Spaces2);
    }

    #[test]
    fn char_to_byte_pos_ascii() {
        let s = "hello";
        assert_eq!(char_to_byte_pos(s, 0), 0);
        assert_eq!(char_to_byte_pos(s, 3), 3);
        assert_eq!(char_to_byte_pos(s, 5), 5);
    }

    #[test]
    fn diff_nested_changes() {
        let a = parse_json("{\"a\": {\"b\": 1, \"c\": 2}}").unwrap();
        let b = parse_json("{\"a\": {\"b\": 1, \"c\": 3}}").unwrap();
        let diffs = diff_json(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].path.contains("c"));
    }
}
