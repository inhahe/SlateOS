//! `Slate OS` JSON Viewer & Editor
//!
//! A full-featured JSON data viewer and editor with:
//! - Custom JSON parser supporting full JSON spec (RFC 8259)
//! - Collapsible tree view with color-coded values
//! - Syntax-highlighted raw/formatted view with pretty-printing
//! - Search across keys and values with match navigation
//! - `JSONPath` display for selected nodes
//! - In-place edit mode for values, key add/delete
//! - Real-time validation with line/column error reporting
//! - The document's own text, edited as it is in the raw view (Enter or a
//!   click), parsed again as it changes -- how a file that does not parse
//!   is repaired
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
// The defensive lints below fire heavily in this crate's hand-written JSON
// parser/lexer (RFC 8259) where the algorithm itself enforces bounds and
// non-overflow: every index/slice is gated by the parser's position vs the
// source length, and arithmetic operates on small parser offsets (well below
// usize::MAX). Replacing every `+ 1` with `.saturating_add(1)` here would
// only obscure the parser without changing observable behaviour.
#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::indexing_slicing)]

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::dialog::{FileDialog, FilePicker, Picked};
use guitk::event::{Event, EventResult, KeyEvent, MouseButton, MouseEventKind};
use guitk::render::{FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, Response};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use textarea::{Edited, TextArea};
use unsaved::{Choice, Question};

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

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
const SMALL_TEXT: f32 = 12.0;
const NORMAL_TEXT: f32 = 14.0;
const HEADER_TEXT: f32 = 16.0;
const TITLE_TEXT: f32 = 18.0;
const TREE_INDENT: f32 = 20.0;
const TREE_ICON_SIZE: f32 = 14.0;
/// The find bar's height, and its "Aa" (match case) button's left edge and
/// width -- read by the drawing and the click alike.
const SEARCH_BAR_HEIGHT: f32 = 36.0;
const CASE_BUTTON_X: f32 = 500.0;
const CASE_BUTTON_W: f32 = 28.0;

/// The text drawn on `doc`'s tab, dirty marker included.
fn tab_label(doc: &Document) -> String {
    if doc.dirty {
        format!("{} *", doc.title)
    } else {
        doc.title.clone()
    }
}

/// Width of the tab drawn for `doc`.
///
/// Hit-testing and rendering share this, because when each computed its own
/// the two drifted: the click test sized the tab from `doc.title` while the
/// render sized it from the title *plus* its dirty marker, so a modified
/// document's tab was drawn wider than the region that would select it.
///
/// Measured bold whatever the tab's state, since the active tab is drawn
/// bold — sizing each tab to its current weight would reflow the whole strip
/// every time the user switched tabs.
fn tab_width(doc: &Document) -> f32 {
    text::measure(&tab_label(doc), SMALL_TEXT, FontWeightHint::Bold) + 40.0
}

/// Width of the view-mode chip for `mode`, likewise shared with its hit test
/// and measured bold so the strip does not reflow on selection.
fn mode_width(mode: ViewMode) -> f32 {
    text::measure(mode.label(), SMALL_TEXT, FontWeightHint::Bold) + 20.0
}

// Limits
/// The character bound handed to [`TextArea`] while the text is edited as it
/// is: none. The real bound is [`MAX_OPEN_BYTES`] of UTF-8, checked after
/// each key, because text this program writes must be text it can read back
/// whole. A character bound at that number would stop an ASCII text at the
/// same place -- first, and without a word -- since a text never has more
/// characters than bytes.
const SOURCE_CHARS_UNBOUNDED: usize = usize::MAX;
/// The raw view's line-number gutter.
const RAW_GUTTER: f32 = 50.0;
/// Where the raw view's text begins.
const SOURCE_TEXT_X: f32 = RAW_GUTTER + 4.0;
/// Room kept beside the caret when a long line scrolls sideways to show it.
const SOURCE_MARGIN: f32 = 40.0;
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
        } else if byte & 0xC0 != 0x80 {
            // `col` is shown to the user as "Ln 3, Col 17" and is meant to be a
            // character position. A UTF-8 continuation byte (0b10xx_xxxx) is
            // the tail of a character whose leading byte already counted, so
            // only leading bytes advance the column — otherwise an error after
            // a run of CJK is reported up to three columns per character too
            // far right, pointing at text that isn't there.
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

    /// The indent `text` is written with, judged by its first indented line,
    /// or `None` when no line is indented or the step is not one of these.
    ///
    /// The first indented line is enough because JSON has no multi-line
    /// strings: every line break is structural, so the first line that starts
    /// with whitespace is one level in, and its whitespace is one step.
    fn detect(text: &str) -> Option<Self> {
        let lead = text.lines().find_map(|line| {
            let rest = line.trim_start_matches([' ', '\t']);
            let lead = line.len() - rest.len();
            (lead > 0 && !rest.is_empty()).then(|| line.get(..lead).unwrap_or(""))
        })?;
        if lead.starts_with('\t') {
            return Some(Self::Tabs);
        }
        match lead.len() {
            2 => Some(Self::Spaces2),
            4 => Some(Self::Spaces4),
            8 => Some(Self::Spaces8),
            _ => None,
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
    /// The colour a JSON value's type is written in.
    ///
    /// Inked per arm rather than around the `match`: `overlay0` is the
    /// faintest legible mark and is deliberately below the text floor, so
    /// raising it would erase the distinction it exists to make. Every other
    /// arm is a dual-use hue drawn as text and asks for the legible version.
    /// 837, and `gui/appearance/colour-methods.py` for why this method may be
    /// inked inside at all: every one of its callers draws text.
    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Null => pal.overlay0,
            Self::Bool => pal.ink(pal.blue),
            Self::Number => pal.ink(pal.peach),
            Self::Str => pal.ink(pal.green),
            Self::Array => pal.ink(pal.lavender),
            Self::Object => pal.ink(pal.mauve),
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
    let Some((head, rest)) = path.split_first() else {
        *root = new_value;
        return true;
    };
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

    let Some((head, rest)) = path.split_first() else {
        return false;
    };
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
/// Add a key to an object at `path`.
///
/// Nothing calls this yet, and that is a statement about the UI rather than
/// about the function: adding a key needs somewhere for the user to type the
/// name, and this program has no such field. Editing an *existing* value works
/// (`set_value_at_path`, wired in the same commit that wrote this comment);
/// creating a new one does not. It keeps its test, which is what makes wiring
/// it later a small job rather than a rewrite.
/// See known-issues.md -> TD-C-JSONVIEWER-CAN-EDIT-A-VALUE-BUT-NOT-ADD-ONE.
#[allow(dead_code, reason = "the editor's add-a-key half has no UI yet")]
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

/// The value at `path`, mutably.
///
/// Used only by [`add_key_at_path`], and therefore reachable exactly when that
/// is.
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
fn highlight_json_text(formatted: &str, pal: &Palette) -> Vec<Vec<HighlightedSpan>> {
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
                let color = if is_key { pal.blue } else { pal.green };
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
                    color: pal.peach,
                    bold: false,
                });
            }
            't' if i + 4 <= len && chars[i..i + 4].iter().collect::<String>() == "true" => {
                current_line.push(HighlightedSpan {
                    text: String::from("true"),
                    color: pal.blue,
                    bold: false,
                });
                i += 4;
            }
            'f' if i + 5 <= len && chars[i..i + 5].iter().collect::<String>() == "false" => {
                current_line.push(HighlightedSpan {
                    text: String::from("false"),
                    color: pal.blue,
                    bold: false,
                });
                i += 5;
            }
            'n' if i + 4 <= len && chars[i..i + 4].iter().collect::<String>() == "null" => {
                current_line.push(HighlightedSpan {
                    text: String::from("null"),
                    color: pal.overlay0,
                    bold: false,
                });
                i += 4;
            }
            '{' | '}' | '[' | ']' => {
                current_line.push(HighlightedSpan {
                    text: ch.to_string(),
                    color: pal.text,
                    bold: true,
                });
                i += 1;
            }
            ':' => {
                current_line.push(HighlightedSpan {
                    text: String::from(": "),
                    color: pal.subtext0,
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
                    color: pal.subtext0,
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
                    color: pal.text,
                    bold: false,
                });
            }
            '\r' => {
                i += 1;
            }
            _ => {
                current_line.push(HighlightedSpan {
                    text: ch.to_string(),
                    color: pal.text,
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

/// The raw view's text being edited as it is.
///
/// The raw view otherwise draws a *formatted copy* -- re-indented, or
/// minified, as `I` and `M` choose -- which is not what is in the file and so
/// cannot be edited in place. This is the file's own text, verbatim, with a
/// caret. Every change is written straight back to [`Document::input`] and
/// parsed again, so the tree, the statistics and a save see it at once, and a
/// document that does not parse can be repaired where it stands.
///
/// Before this, the only text entry was an "input area" whose keys were
/// handled and which nothing could focus: `input_focused` had one writer,
/// which set it to `false`. A new document could never be given content, and
/// an opened one that did not parse could be looked at but not fixed.
#[derive(Debug, Clone)]
struct SourceEdit {
    /// The text, its caret and its selection.
    area: TextArea,
    /// The first line on screen.
    scroll: usize,
    /// How far the lines are scrolled sideways, in pixels: a minified
    /// document is one line, and the caret must stay on screen along it.
    hscroll: f32,
    /// The wheel's fractions of a line, until they make a whole one.
    wheel: wheel::Accumulator,
}

impl SourceEdit {
    /// `text`, with the caret at byte `at` and the first line on screen at
    /// `scroll`.
    fn new(text: &str, at: usize, scroll: usize) -> Self {
        let mut area = TextArea::new(NORMAL_TEXT).with_family(FontFamily::Mono);
        area.set_text(text);
        area.move_to(at, false);
        Self {
            area,
            scroll,
            hscroll: 0.0,
            wheel: wheel::Accumulator::default(),
        }
    }

    /// How far into its line the caret is drawn, in pixels.
    fn caret_x(&self) -> f32 {
        let caret = self.area.caret();
        let start = self.area.line_start(caret);
        text::measure_in(
            self.area.text().get(start..caret).unwrap_or(""),
            NORMAL_TEXT,
            FontWeightHint::Regular,
            FontFamily::Mono,
        )
    }

    /// Scroll so the caret is among the `rows` lines on screen, and within
    /// the `width` pixels the text is drawn in.
    fn keep_caret_in_view(&mut self, rows: usize, width: f32) {
        let rows = rows.max(1);
        let line = self.area.line_index(self.area.caret());
        if line < self.scroll {
            self.scroll = line;
        } else if line >= self.scroll.saturating_add(rows) {
            self.scroll = line.saturating_add(1).saturating_sub(rows);
        }
        let x = self.caret_x();
        // The margin shrinks with a narrow window, or the caret could never
        // be both inside it and on screen.
        let margin = SOURCE_MARGIN.min(width / 4.0).max(0.0);
        if x < self.hscroll {
            self.hscroll = (x - margin).max(0.0);
        } else if x + guitk::textedit::CARET_WIDTH > self.hscroll + width {
            self.hscroll = (x + guitk::textedit::CARET_WIDTH + margin - width).max(0.0);
        }
    }
}

/// Byte offset of 1-based `line` and `column` (a column counts characters,
/// as [`ParseError`] reports them) in `text`, clamped to the line's end and
/// to the text's.
fn byte_at(text: &str, line: usize, column: usize) -> usize {
    let mut start = 0;
    for _ in 1..line {
        match text.get(start..).and_then(|rest| rest.find('\n')) {
            Some(nl) => start = start.saturating_add(nl).saturating_add(1),
            None => return text.len(),
        }
    }
    let rest = text.get(start..).unwrap_or("");
    let end = rest.find('\n').unwrap_or(rest.len());
    let within = rest.get(..end).unwrap_or("");
    start.saturating_add(char_to_byte_pos(within, column.saturating_sub(1)))
}

/// A single JSON document tab.
struct Document {
    /// Tab identifier, unique for the life of the window.
    ///
    /// What anything that outlives one event names a tab by -- the close
    /// question, and the picker choosing where to save -- because a position
    /// in `documents` moves when a tab before it closes. Most of the program
    /// still addresses the *active* tab by index, which is safe within one
    /// event; see known-issues.md -> TD-C-JSONVIEWER-CAN-EDIT-A-VALUE-BUT-NOT-ADD-ONE.
    id: u64,
    /// Tab title.
    title: String,
    /// The file this document was read from or last saved to, or `None` for
    /// one never saved.
    ///
    /// The path itself and not the title: a name that is not valid UTF-8 is
    /// shown with replacement characters, and saving to *that* would write a
    /// different file from the one that was opened.
    path: Option<PathBuf>,
    /// `Some(whole)` when only the first part of a `whole`-byte file was read.
    ///
    /// Such a document is never written back over its file, which would cut
    /// the file off where the reading stopped. Save As may write it elsewhere,
    /// and the new file then holds all of it.
    whole_len: Option<usize>,
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
    /// Counts content changes, so an edit reads as a redraw. See
    /// [`invalidate_caches`](Self::invalidate_caches).
    revision: u64,
    /// Selected tree node index.
    selected_node: usize,
    /// Scroll offset for tree view.
    tree_scroll: f32,
    /// Scroll offset for raw view.
    raw_scroll: f32,
    /// The text being edited as it is, in the raw view; `None` while the
    /// view shows the formatted copy.
    source: Option<SourceEdit>,
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
            path: None,
            whole_len: None,
            input: String::new(),
            parsed: None,
            error: None,
            view_mode: ViewMode::Tree,
            expanded_paths: Vec::new(),
            selected_node: 0,
            revision: 0,
            tree_scroll: 0.0,
            raw_scroll: 0.0,
            source: None,
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

    fn get_highlighted(&mut self, pal: &Palette) -> Vec<Vec<HighlightedSpan>> {
        if let Some(ref cached) = self.highlighted_cache {
            return cached.clone();
        }
        let formatted = self.get_formatted();
        let highlighted = highlight_json_text(&formatted, pal);
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

    /// The document's content changed: drop the derived text and count it.
    ///
    /// `revision` is what makes an *edit* read as a redraw. Deleting a node or
    /// committing a new value rewrites `input` and `parsed`, and neither is in
    /// the redraw fingerprint -- a `1` edited to a `2` is the same length, so
    /// even a cheap proxy like `input.len()` would miss it and the change
    /// would sit on screen undrawn. Every content change already comes through
    /// here, so one counter here is exact and costs nothing.
    fn invalidate_caches(&mut self) {
        self.formatted_cache = None;
        self.highlighted_cache = None;
        self.yaml_cache = None;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Rewrite `input` from `parsed` after a change made in the tree, in the
    /// layout the text already had.
    ///
    /// Saving writes `input`, so this is what a tree edit does to the file. It
    /// was `format_json(value, self.indent)` whatever the file looked like, so
    /// changing one number in a one-line file saved it pretty-printed, and a
    /// file indented by four spaces came back at two -- `indent` being the raw
    /// view's setting, not the file's. Now a one-line text stays one line, an
    /// indented one keeps the indent it has ([`IndentStyle::detect`]), and a
    /// final newline is kept or left off as it was. The spacing *inside* a line
    /// is not kept: the text is regenerated from the parsed value, which does
    /// not remember it.
    fn regenerate_input(&mut self) {
        let Some(value) = self.parsed.as_ref() else {
            return;
        };
        let one_line = !self.input.trim().contains('\n');
        let final_newline = self.input.ends_with('\n');
        let mut text = if one_line {
            minify_json(value)
        } else {
            format_json(
                value,
                IndentStyle::detect(&self.input).unwrap_or(IndentStyle::Spaces2),
            )
        };
        match (final_newline, text.ends_with('\n')) {
            (true, false) => text.push('\n'),
            (false, true) => {
                text.pop();
            }
            _ => {}
        }
        self.input = text;
    }

    /// The name to offer when saving this document somewhere new.
    fn save_name(&self) -> OsString {
        self.path.as_deref().and_then(Path::file_name).map_or_else(
            || OsString::from(format!("{}.json", self.title)),
            OsString::from,
        )
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
    /// Whether the shortcut list is up.
    show_help: bool,
    /// Edit mode flag.
    edit_mode: bool,
    /// Edit buffer for value editing.
    edit_buffer: String,
    /// The node the edit buffer belongs to, if an edit is in progress.
    ///
    /// `edit_mode` says the user has *enabled* editing; this says a particular
    /// value is being typed over. Without it a commit has no address to write
    /// to, which is why `set_value_at_path` — written and covered by four
    /// tests — had no caller at all: Enter loaded a value into the buffer and
    /// nothing ever wrote it back.
    editing_path: Option<Vec<PathSegment>>,
    /// What Ctrl+C or Ctrl+X last took from the text being edited, for
    /// Ctrl+V: the program's own, as in the other editors here.
    clipboard: String,
    /// The open or save picker. Holds the dialog, the saving flag and
    /// the routing eleven applications used to write out by hand.
    pub picker: FilePicker,
    /// What the picker is choosing a path for, since one picker serves
    /// opening and saving.
    picker_purpose: PickerPurpose,
    /// "Unsaved changes -- save them?", while it is being asked, and what
    /// it would close. The toolkit's own dialog, asked the one way every
    /// editor here asks it (`apps/unsaved`).
    question: Option<Question<CloseScope>>,
    /// Set once the window may close; the next answer to the loop is `Exit`.
    quit: bool,
    /// What the last open or save did, for the status line.
    note: Option<String>,
    /// Width of the window.
    width: f32,
    /// Height of the window.
    height: f32,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

// ============================================================================
// Statistics-view geometry
// ============================================================================
//
// Every column here is a fraction of the room actually available. The panel is
// the window minus the sidebar, and the user can make the window narrow, so a
// literal constant subtracted from a fraction of it is a bug waiting for a
// small window: the type-distribution bar used to be sized
// `width * 0.5 - 250.0`, which is negative for any panel under 500 px. That
// produced a negative-width `FillRect`, and the percentage label — positioned
// at `bar_x + bar_width + 8.0` — walked *left* of its own bar and landed on
// top of the count cell. Deriving each column from the width that is really
// there, and clamping at zero, makes both impossible by construction.

/// Gap between statistics columns.
const STATS_GAP: f32 = 8.0;
/// Left edge of a statistics row: the panel padding plus the card's own inset.
const STATS_INSET: f32 = PADDING + 8.0;
/// Interior padding a general-statistics card spends on each side.
const STATS_CARD_PAD: f32 = 8.0;

/// Column indices for a general-statistics row.
const STATS_LABEL: usize = 0;
const STATS_VALUE: usize = 1;

/// Column indices for a type-distribution row.
const TYPE_LABEL: usize = 0;
const TYPE_COUNT: usize = 1;
const TYPE_BAR: usize = 2;
const TYPE_PCT: usize = 3;

/// Share of a type-distribution row taken by each column. Fractions, not
/// pixels, so the row scales with the panel instead of overflowing it; they
/// sum to 1.0 so the row exactly fills the space between the inset and the
/// right margin (see `the_type_rows_fill_the_panel`).
const TYPE_FRACTIONS: [f32; 4] = [0.20, 0.10, 0.55, 0.15];

/// Width of a general-statistics card.
///
/// Clamped at zero: this is a rect width, and a negative one is not a small
/// rect but an ill-formed drawing command.
fn stats_card_width(panel_width: f32) -> f32 {
    (panel_width * 0.5 - PADDING * 2.0).max(0.0)
}

/// The label/value columns inside a general-statistics card.
fn stats_columns(panel_width: f32) -> [Column; 2] {
    let interior = (stats_card_width(panel_width) - STATS_CARD_PAD * 2.0 - STATS_GAP).max(0.0);
    [
        Column {
            label: "",
            width: interior * 0.5,
        },
        Column {
            label: "",
            width: interior * 0.5,
        },
    ]
}

/// The label/count/bar/percentage columns of a type-distribution row.
fn type_columns(panel_width: f32) -> [Column; 4] {
    // The row runs from the inset to the panel's right margin; the gaps come
    // out of it before the fractions are applied, so the fractions describe
    // drawable width rather than width-plus-gaps.
    let row = (panel_width - STATS_INSET - PADDING).max(0.0);
    let usable = (row - STATS_GAP * 3.0).max(0.0);
    let mut columns = [Column {
        label: "",
        width: 0.0,
    }; 4];
    for (column, fraction) in columns.iter_mut().zip(TYPE_FRACTIONS) {
        column.width = usable * fraction;
    }
    columns
}

/// A statistics table whose first column starts at the shared left inset.
///
/// [`Table`]'s origin sits *before* the leading gap — `left(0)` is `x + gap` —
/// so the inset has to be handed over less one gap. Both tables go through
/// here rather than each writing that adjustment out, because getting it wrong
/// is silent: every column shifts by the same 8 px, so the columns still line
/// up with each other and only the margin at the far end is wrong.
fn stats_table(columns: &[Column]) -> Table<'_> {
    Table::with_gap(columns, STATS_INSET - STATS_GAP, STATS_GAP)
}

/// Every key this program answers, and what it does.
///
/// Thirty-odd bindings across five views and nothing on screen naming one.
/// `I` and `M` are the worst: they cycle the indent and toggle minification,
/// they work only in the raw view, and a reader looking at pretty-printed JSON
/// has no way to learn that the program can reformat it at all.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`, which reads each label with
/// `guitk::shortcut` and presses every key it names.
const SHORTCUTS: &[(&str, &str)] = &[
    ("1 / 2 / 3", "Tree / raw text / YAML"),
    ("4 / 5", "Statistics / diff"),
    ("Up / Down", "Move through the tree"),
    ("Left", "Collapse this node, or go to its parent"),
    ("Right", "Expand this node, or go to its first child"),
    ("Enter / Space", "Expand or collapse"),
    ("PageUp / PageDown", "A screen at a time"),
    ("Home / End", "First / last node"),
    ("Delete", "Remove this node"),
    ("I", "Cycle the indent, in the raw view"),
    ("M", "Minify or pretty-print, in the raw view"),
    ("Enter", "Edit the text itself, in the raw view (Esc stops)"),
    ("Ctrl+O", "Open a file"),
    ("Ctrl+S", "Save"),
    ("Ctrl+Shift+S", "Save as a new file"),
    ("Ctrl+N / Ctrl+W", "New tab / close this tab"),
    ("Ctrl+Tab", "Next tab"),
    ("Ctrl+F", "Find"),
    ("Ctrl+I", "Match case while searching"),
    ("Ctrl+G", "Find the next match"),
    ("Ctrl+E", "Editing on or off"),
    ("F1 / ?", "This list"),
];

/// What the file picker is choosing a path for. Each names its document by
/// [`Document::id`], since the picker outlives the event that put it up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerPurpose {
    /// A file to read into a new tab.
    Open,
    /// Where to write this document, which then belongs to that file.
    SaveAs(u64),
    /// Where to write this document before its tab closes.
    SaveThenClose(u64),
    /// Where to write this document before the window goes on closing.
    SaveThenQuit(u64),
}

/// What a pending close would close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseScope {
    /// One document's tab, by [`Document::id`].
    Tab(u64),
    /// The whole window.
    Window,
}

/// What a toolbar button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolbarAction {
    New,
    Open,
    Save,
    Search,
    Edit,
}

/// The toolbar's buttons, in the order they are drawn.
///
/// New, Search and Edit were drawn and answered nothing -- the toolbar sat
/// above a click handler that began at the tab bar -- and there was no Save to
/// draw, because nothing could save.
const TOOLBAR: [(ToolbarAction, &str); 5] = [
    (ToolbarAction::New, "New"),
    (ToolbarAction::Open, "Open"),
    (ToolbarAction::Save, "Save"),
    (ToolbarAction::Search, "Search"),
    (ToolbarAction::Edit, "Edit"),
];

/// Where the first toolbar button starts, clear of the title.
const TOOLBAR_FIRST_X: f32 = 200.0;
/// A toolbar button's top, and its height.
const TOOLBAR_BUTTON_Y: f32 = 8.0;
const TOOLBAR_BUTTON_H: f32 = 28.0;

/// Each toolbar button's action, label, left edge and width.
///
/// The one walk the drawing and the click both take, so a button answers
/// exactly where it is drawn.
fn toolbar_buttons() -> Vec<(ToolbarAction, &'static str, f32, f32)> {
    let mut x = TOOLBAR_FIRST_X;
    TOOLBAR
        .iter()
        .map(|&(action, label)| {
            let w = text::width(label, SMALL_TEXT) + 16.0;
            let at = (action, label, x, w);
            x += w + 8.0;
            at
        })
        .collect()
}

/// How much of a tab's right end is its close mark, which is drawn 18 px in.
const TAB_CLOSE_ZONE: f32 = 22.0;

/// Everything that decides whether a frame is worth drawing.
///
/// `handle_key` reports nothing about whether it did anything, so this is
/// compared around every event and the answer *is* `EventResult`. A field
/// missing from here is a change the user cannot see: the picker once opened
/// invisibly for that reason, and so -- until the shortcut list's guard test
/// pressed `Enter` and found nothing happened -- did expanding a node.
///
/// A struct rather than the tuple this was. Sixteen fields is past the twelve
/// Rust implements `PartialEq` for, and it was past readable long before that:
/// `(usize, String, usize, bool, bool, bool, String, usize, ...)` gives a
/// reader adding a field no way to check they put it in the right place.
#[derive(Clone, PartialEq)]
struct Fingerprint {
    active_tab: usize,
    search_query: String,
    search_index: usize,
    search_visible: bool,
    /// Whether the search is case-sensitive, which the "Aa" button draws.
    search_case_sensitive: bool,
    edit_mode: bool,
    edit_buffer: String,
    /// The text being edited as it is: its caret, selection and scroll.
    /// What it holds is counted by `revision`.
    source: Option<(usize, Option<usize>, usize, f32)>,
    /// Document state: the selection, and the view it is shown in.
    selected_node: usize,
    view_mode: ViewMode,
    /// Opening or closing the picker is a redraw.
    picker_open: bool,
    /// ...and so is raising or dismissing the shortcut list.
    show_help: bool,
    /// The *count* of expanded paths, not the paths: one event toggles at most
    /// one, so the count always moves, and cloning a `Vec<Vec<PathSegment>>`
    /// every keystroke to learn that costs real time for no more information.
    /// An operation that expanded one path and collapsed another in a single
    /// event would defeat it, and the clone is then the honest replacement.
    expanded: usize,
    /// Content changes -- a deleted node, a committed value -- which nothing
    /// else here can see: `parsed` and `input` are not fields of this, and a
    /// same-length edit moves no other number.
    revision: u64,
    /// Scrolling moved the view, which the old tuple's comment claimed was
    /// covered and was not.
    tree_scroll: f32,
    raw_scroll: f32,
    diff_scroll: f32,
    /// Every tab's title and unsaved mark: a save clears the mark and Save As
    /// renames the tab without touching the content, so no number above moves.
    /// Closing a tab shows here too -- closing the active tab of two fresh
    /// ones left every other field equal, and the closed tab stayed drawn.
    tabs: Vec<(String, bool)>,
    /// What the status line says about the last open or save.
    note: Option<String>,
    /// The close question, raised or answered. What happens *inside* it --
    /// focus moving between its buttons, the pointer over one -- is not
    /// seen here; `handle_event` answers every event it takes as a redraw.
    question: Option<CloseScope>,
    quit: bool,
}

impl App {
    fn new() -> Self {
        // Opens empty. It used to load `SAMPLE_JSON` -- a document describing
        // this program, its version and its feature list -- which was the only
        // content the viewer could ever hold, because nothing here could read
        // a file. Ctrl+O reads one now.
        let doc = Document::new(1, String::from("Untitled"));

        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            documents: vec![doc],
            picker: FilePicker::new(),
            picker_purpose: PickerPurpose::Open,
            question: None,
            quit: false,
            note: Some(String::from("Press Ctrl+O to open a JSON file")),
            active_tab: 0,
            next_tab_id: 2,
            search_query: String::new(),
            search_case_sensitive: false,
            search_results: Vec::new(),
            search_index: 0,
            search_visible: false,
            show_help: false,
            edit_mode: false,
            edit_buffer: String::new(),
            editing_path: None,
            clipboard: String::new(),
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    fn active_doc(&self) -> Option<&Document> {
        self.documents.get(self.active_tab)
    }

    fn new_tab(&mut self) {
        if self.documents.len() >= MAX_TABS {
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        let title = format!("Untitled {id}");
        self.documents.push(Document::new(id, title));
        self.switch_to(self.documents.len() - 1);
    }

    /// Make tab `index` the active one.
    ///
    /// A value being typed over belongs to the tab it was started in, but
    /// `editing_path` is only a path: it names a node in *whichever* document
    /// is active. Switching tabs without settling it left the path pointing
    /// into the next tab, where Enter wrote the typed value into a different
    /// document at the same path. So it is committed first, the way clicking
    /// another cell commits the one being typed in a spreadsheet. The search
    /// is re-run for the same reason: its matches are paths into one document.
    fn switch_to(&mut self, index: usize) {
        if index == self.active_tab {
            return;
        }
        if self.editing_path.is_some() {
            self.commit_edit();
        }
        self.active_tab = index;
        self.perform_search();
    }

    /// Where tab `id` is now, if it is still open.
    fn index_of(&self, id: u64) -> Option<usize> {
        self.documents.iter().position(|d| d.id == id)
    }

    fn close_tab(&mut self, index: usize) {
        if self.documents.len() <= 1 || index >= self.documents.len() {
            return;
        }
        if index == self.active_tab {
            // The edit's document is going; the edit goes with it.
            self.cancel_edit();
        }
        self.documents.remove(index);
        // A tab closed before the active one moves it one place left; the
        // active tab closed hands over to the one after it, or the last.
        if index < self.active_tab {
            self.active_tab -= 1;
        } else if self.active_tab >= self.documents.len() {
            self.active_tab = self.documents.len() - 1;
        }
        self.perform_search();
    }

    fn perform_search(&mut self) {
        let value = self
            .documents
            .get(self.active_tab)
            .and_then(|doc| doc.parsed.as_ref());
        match value {
            Some(value) if !self.search_query.is_empty() => {
                self.search_results =
                    search_json(value, &self.search_query, self.search_case_sensitive);
                if self.search_index >= self.search_results.len() {
                    self.search_index = 0;
                }
            }
            // Nothing to search, or nothing to search for: no matches. This
            // kept the last document's matches when the active one had no
            // parsed value, drawing them against a document they are not in.
            _ => {
                self.search_results.clear();
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
    fn handle_key(&mut self, ev: &KeyEvent) {
        use guitk::event::Key;

        let (key, modifiers) = (ev.key, ev.modifiers);
        let text = ev.text.chars().next();
        let editing_source = self.source_is_open();

        // The shortcut list, before anything else -- but **not** while the
        // find bar or a value edit is open, because both of those take typed
        // characters and `?` belongs in somebody's query or their string.
        //
        // That gate is here because the first survey of this app said it had
        // no text entry at all: it takes characters as an `Option<char>`
        // argument to this function rather than through `key.text` or
        // `typed()`, so a search for either found nothing. The keystroke does
        // become text; it just arrives by a road the search did not cover.
        let typing = self.search_visible || self.editing_path.is_some() || editing_source;
        if !typing {
            if key == Key::F1 || (key == Key::Slash && modifiers.shift) {
                self.show_help = !self.show_help;
                return;
            }
            if key == Key::Escape && self.show_help {
                self.show_help = false;
                return;
            }
        }

        // Global shortcuts
        if modifiers.ctrl {
            match key {
                Key::O => {
                    self.open_file_dialog();
                    return;
                }
                Key::N => {
                    self.new_tab();
                    return;
                }
                Key::S => {
                    if modifiers.shift {
                        self.save_active_as();
                    } else {
                        self.save_active();
                    }
                    return;
                }
                Key::W => {
                    self.request_close_tab(self.active_tab);
                    return;
                }
                Key::F => {
                    // The find bar takes the keys while it is open, so the
                    // text stops being edited first: otherwise each would
                    // think a typed key was its own.
                    self.close_source();
                    self.search_visible = !self.search_visible;
                    return;
                }
                // The "Aa" button the search bar draws, whose colour has
                // tracked this flag since it was written and whose flag had
                // no writer: `search_case_sensitive` was `false` at
                // construction, passed to `search_json`, drawn twice, and
                // changeable only from a test. `Ctrl+I` is what
                // `apps/hexeditor` uses for the same question.
                // Found by `scripts/frozen-flag-survey.py`.
                Key::I => {
                    self.search_case_sensitive = !self.search_case_sensitive;
                    self.perform_search();
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
                        let next = if modifiers.shift {
                            self.active_tab
                                .checked_sub(1)
                                .unwrap_or(self.documents.len() - 1)
                        } else {
                            (self.active_tab + 1) % self.documents.len()
                        };
                        self.switch_to(next);
                    }
                    return;
                }
                _ => {}
            }
        }

        // The text being edited as it is takes every other key.
        if editing_source {
            self.source_key(ev);
            return;
        }

        // An edit in progress takes every printable key, for the same reason
        // the search bar below does: while a value is being typed over, "5" is
        // the character five and not the shortcut for the diff view.
        if self.editing_path.is_some() {
            match key {
                Key::Escape => {
                    self.cancel_edit();
                    return;
                }
                Key::Enter => {
                    self.commit_edit();
                    return;
                }
                Key::Backspace => {
                    self.edit_buffer.pop();
                    return;
                }
                _ => {
                    if let Some(ch) = text
                        && self.edit_buffer.len() < MAX_SEARCH_LEN
                    {
                        self.edit_buffer.push(ch);
                    }
                    return;
                }
            }
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

        // Enter edits the text itself: in the raw view, and in a tree with
        // nothing in it -- a new document, or one that does not parse, which
        // is the one that most needs it. The caret starts where the parse
        // failed, when it did.
        if key == Key::Enter
            && let Some(doc) = self.documents.get(self.active_tab)
            && (doc.view_mode == ViewMode::Raw
                || (doc.view_mode == ViewMode::Tree && doc.parsed.is_none()))
        {
            let at = doc
                .error
                .as_ref()
                .map_or(0, |e| byte_at(&doc.input, e.line, e.column));
            self.open_source(at, None);
            return;
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
                            doc.raw_scroll = (doc.raw_scroll - 10.0 * LINE_HEIGHT).max(0.0);
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
                            doc.diff_scroll = (doc.diff_scroll - 10.0 * LINE_HEIGHT).max(0.0);
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
        // Entering the diff view is the moment the comparison is wanted, and
        // until this call existed nothing in the program ever ran one: the
        // differ, its four kinds, `Document::run_diff` and a whole
        // `render_diff_view` were written and tested, and the *one* thing
        // missing was anything that filled `diff_source`. The view drew
        // "0 difference(s)" for every document, always.
        if matches!(key, Key::Num5) && !modifiers.ctrl {
            self.prepare_diff_against_next_tab();
        }
    }

    /// Point the active document's diff at the next open tab, and run it.
    ///
    /// The next tab rather than a file chooser, because tabs are what this
    /// program already has: comparing the document you are looking at with the
    /// one beside it is the comparison a two-tab window is asking for. With
    /// only one document open there is nothing to compare against, and the
    /// diff view already says so.
    fn prepare_diff_against_next_tab(&mut self) {
        if self.documents.len() < 2 {
            return;
        }
        let next = self
            .active_tab
            .checked_add(1)
            .map_or(0, |n| if n < self.documents.len() { n } else { 0 });
        let Some(source) = self.documents.get(next).map(|d| d.input.clone()) else {
            return;
        };
        if let Some(doc) = self.documents.get_mut(self.active_tab) {
            doc.diff_source = source;
            doc.run_diff();
        }
    }

    /// Write the edit buffer back into the document.
    ///
    /// The buffer is parsed as JSON first, so `123` becomes a number and
    /// `"123"` a string — which is the distinction this program exists to show,
    /// and would be lost by storing every edit as text. A buffer that is not
    /// valid JSON on its own is taken as a string, because that is what a user
    /// typing `hello` into a string field means.
    fn commit_edit(&mut self) {
        let Some(path) = self.editing_path.take() else {
            return;
        };
        let text = self.edit_buffer.clone();
        let new_value =
            parse_json(&text).unwrap_or_else(|_| JsonValue::Str(text.trim_matches('"').to_owned()));
        let Some(doc) = self.documents.get_mut(self.active_tab) else {
            return;
        };
        let written = doc
            .parsed
            .as_mut()
            .is_some_and(|root| set_value_at_path(root, &path, new_value));
        if written {
            doc.dirty = true;
            doc.invalidate_caches();
            doc.regenerate_input();
        }
        self.edit_buffer.clear();
    }

    /// Abandon an edit in progress, leaving the document alone.
    fn cancel_edit(&mut self) {
        self.editing_path = None;
        self.edit_buffer.clear();
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
                // A commit first: with an edit in progress, Enter means "keep
                // what I typed", not "start again".
                if self.editing_path.is_some() {
                    self.commit_edit();
                } else if let Some(node) = nodes.get(doc.selected_node) {
                    if node.expandable {
                        let path = node.path.clone();
                        self.toggle_expand(&path);
                    } else if self.edit_mode {
                        // Start editing this value.
                        self.edit_buffer = node.value_display.clone();
                        self.editing_path = Some(node.path.clone());
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
                            d.regenerate_input();
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

    /// Whether the active document's text is being edited as it is.
    fn source_is_open(&self) -> bool {
        self.active_doc().is_some_and(|d| d.source.is_some())
    }

    /// Edit the active document's own text, with the caret at byte `at` and,
    /// when given, line `scroll` first on screen -- in the raw view, which
    /// this switches to.
    ///
    /// A value being typed over in the tree is committed first, as switching
    /// tabs commits it, and the find bar closes: both take typed keys, and
    /// only one thing can.
    fn open_source(&mut self, at: usize, scroll: Option<usize>) {
        if self.editing_path.is_some() {
            self.commit_edit();
        }
        self.search_visible = false;
        let (rows, width) = (self.source_rows(), self.source_width());
        let Some(doc) = self.documents.get_mut(self.active_tab) else {
            return;
        };
        doc.view_mode = ViewMode::Raw;
        let mut source = SourceEdit::new(&doc.input, at, scroll.unwrap_or(0));
        let lines = source.area.line_count();
        source.scroll = source.scroll.min(lines.saturating_sub(1));
        source.keep_caret_in_view(rows, width);
        doc.source = Some(source);
    }

    /// Stop editing the active document's text; the raw view shows the
    /// formatted copy again. The text keeps every change.
    fn close_source(&mut self) {
        if let Some(doc) = self.documents.get_mut(self.active_tab) {
            doc.source = None;
        }
    }

    /// How many lines of text the raw view shows: its height, less the line
    /// at its foot that says whether the text parses.
    fn source_rows(&self) -> usize {
        let top = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 30.0;
        let height = self.height - top - STATUS_BAR_HEIGHT - LINE_HEIGHT;
        if height.is_finite() && height > 0.0 {
            ((height / LINE_HEIGHT) as usize).max(1)
        } else {
            1
        }
    }

    /// How wide the raw view's text is drawn, in pixels.
    fn source_width(&self) -> f32 {
        (self.width - SIDEBAR_WIDTH - SOURCE_TEXT_X - PADDING).max(1.0)
    }

    /// A key while the text is being edited as it is.
    ///
    /// Escape stops; Tab puts in a step of the indent the text is written
    /// with, since in this view a Tab has no field to move to; every other
    /// key is [`TextArea::apply_key`]'s. A change is written back to the
    /// document at once and parsed again -- unless it would take the text past
    /// [`MAX_OPEN_BYTES`], which this program would then fail to read back
    /// whole, in which case the text is left as it was and the status line
    /// says why.
    fn source_key(&mut self, ev: &KeyEvent) {
        use guitk::event::Key;
        let (rows, width) = (self.source_rows(), self.source_width());
        let Some(doc) = self.documents.get_mut(self.active_tab) else {
            return;
        };
        let Some(source) = doc.source.as_mut() else {
            return;
        };
        if ev.key == Key::Escape {
            doc.source = None;
            return;
        }
        let before = source.area.clone();
        let edited = if ev.key == Key::Tab && !ev.modifiers.ctrl && !ev.modifiers.shift {
            let step = IndentStyle::detect(source.area.text())
                .unwrap_or(IndentStyle::Spaces2)
                .indent_str();
            Edited {
                handled: true,
                changed: source.area.insert(step, SOURCE_CHARS_UNBOUNDED),
                copied: None,
            }
        } else {
            source
                .area
                .apply_key(ev, SOURCE_CHARS_UNBOUNDED, &self.clipboard, rows)
        };
        if let Some(copied) = edited.copied {
            self.clipboard = copied;
        }
        if source.area.text().len() > MAX_OPEN_BYTES {
            source.area = before;
            self.note = Some(format!(
                "Not added: the text would be over {MAX_OPEN_BYTES} bytes, more than this program reads of a file"
            ));
            return;
        }
        source.keep_caret_in_view(rows, width);
        if edited.changed {
            source.area.text().clone_into(&mut doc.input);
            doc.dirty = true;
            doc.reparse();
            doc.invalidate_caches();
        }
    }

    /// A press in the raw view, `dy` pixels below its top: the text is edited
    /// from there.
    ///
    /// When the view was showing the formatted copy, the text itself takes
    /// its place first, from the same line, and the caret goes where the
    /// pointer is in *that* -- what is under the pointer can change, since
    /// the formatted copy is not the file's text. When it was showing a parse
    /// error, there is no text on screen to point at, so the caret goes where
    /// the parse failed.
    fn source_click(&mut self, x: f32, dy: f32) {
        let (rows, width) = (self.source_rows(), self.source_width());
        let Some(doc) = self.documents.get(self.active_tab) else {
            return;
        };
        if doc.source.is_none() {
            if let Some(error) = &doc.error {
                let at = byte_at(&doc.input, error.line, error.column);
                self.open_source(at, None);
                return;
            }
            // The caret starts on the first line shown, or keeping it in
            // view would scroll back to the top before the press is placed.
            let first = (doc.raw_scroll / LINE_HEIGHT) as usize;
            let at = byte_at(&doc.input, first.saturating_add(1), 1);
            self.open_source(at, Some(first));
        }
        let Some(source) = self
            .documents
            .get_mut(self.active_tab)
            .and_then(|d| d.source.as_mut())
        else {
            return;
        };
        let row = if dy.is_finite() && dy > 0.0 {
            (dy / LINE_HEIGHT) as usize
        } else {
            0
        };
        let line = source.scroll.saturating_add(row);
        source
            .area
            .click(line, x - SOURCE_TEXT_X + source.hscroll, false);
        source.keep_caret_in_view(rows, width);
    }

    fn handle_mouse(&mut self, x: f32, y: f32, button: MouseButton) {
        // The toolbar. Its buttons were drawn above a handler that began at
        // the tab bar, so none of them could be clicked.
        if y < TOOLBAR_HEIGHT {
            if button == MouseButton::Left
                && (TOOLBAR_BUTTON_Y..TOOLBAR_BUTTON_Y + TOOLBAR_BUTTON_H).contains(&y)
                && let Some((action, ..)) = toolbar_buttons()
                    .into_iter()
                    .find(|&(_, _, bx, bw)| x >= bx && x < bx + bw)
            {
                self.toolbar(action);
            }
            return;
        }

        // Tab bar clicks
        if (TOOLBAR_HEIGHT..TOOLBAR_HEIGHT + TAB_BAR_HEIGHT).contains(&y) {
            self.handle_tab_click(x, button);
            return;
        }

        // View mode tab clicks
        let mode_bar_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        if y >= mode_bar_y && y < mode_bar_y + 30.0 {
            self.handle_mode_click(x);
            return;
        }

        // The find bar lies over the top of the content. A click on it went
        // through to the tree, selecting whatever row was under the bar.
        let content_y = mode_bar_y + 30.0;
        if self.search_visible
            && (content_y..content_y + SEARCH_BAR_HEIGHT).contains(&y)
            && x < self.width - SIDEBAR_WIDTH
        {
            if button == MouseButton::Left {
                if (CASE_BUTTON_X..CASE_BUTTON_X + CASE_BUTTON_W).contains(&x) {
                    self.search_case_sensitive = !self.search_case_sensitive;
                    self.perform_search();
                } else if x >= self.search_close_x() {
                    self.search_visible = false;
                }
            }
            return;
        }

        // The raw view: a press edits the text itself, from where it lands.
        if y >= content_y
            && x < self.width - SIDEBAR_WIDTH
            && button == MouseButton::Left
            && self
                .active_doc()
                .is_some_and(|d| d.view_mode == ViewMode::Raw)
        {
            self.source_click(x, y - content_y);
            return;
        }

        // Tree view clicks
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

    /// Where each tab is drawn, as (index, left edge, width), and then where
    /// the "+" button starts: the one walk the drawing and the click share.
    fn tab_rects(&self) -> (Vec<(usize, f32, f32)>, f32) {
        let mut x = PADDING;
        let tabs = self
            .documents
            .iter()
            .enumerate()
            .map(|(i, doc)| {
                let w = tab_width(doc);
                let at = (i, x, w);
                x += w + 4.0;
                at
            })
            .collect();
        (tabs, x)
    }

    fn handle_tab_click(&mut self, x: f32, button: MouseButton) {
        let (tabs, plus_x) = self.tab_rects();
        if let Some(&(i, tab_x, w)) = tabs.iter().find(|&&(_, tx, w)| x >= tx && x < tx + w) {
            // The "x" drawn at a tab's right end, drawn only while there is
            // more than one tab -- and until now a click on it only selected
            // the tab it was meant to close.
            if button == MouseButton::Left
                && self.documents.len() > 1
                && x >= tab_x + w - TAB_CLOSE_ZONE
            {
                self.request_close_tab(i);
            } else {
                self.switch_to(i);
            }
            return;
        }
        // Click on "+" button area
        if x >= plus_x && x < plus_x + 30.0 {
            self.new_tab();
        }
    }

    /// Where the find bar's "Esc" -- its close button -- begins.
    fn search_close_x(&self) -> f32 {
        self.width - SIDEBAR_WIDTH - 34.0
    }

    fn handle_mode_click(&mut self, x: f32) {
        let mut mode_x = PADDING;
        for mode in &VIEW_MODES {
            let mode_width = mode_width(*mode);
            if x >= mode_x && x < mode_x + mode_width {
                if let Some(doc) = self.documents.get_mut(self.active_tab) {
                    doc.view_mode = *mode;
                    // The text is edited in the raw view only; leaving it
                    // stops, as Escape does, and the changes stay.
                    if *mode != ViewMode::Raw {
                        doc.source = None;
                    }
                }
                return;
            }
            mode_x += mode_width + 4.0;
        }
    }

    /// The wheel, whose `dy` is in notches (`guitk::event::MouseEventKind::Scroll`).
    ///
    /// It was treated as pixels, three to a notch: a notch moved the tree a
    /// seventh of a line. `wheel::pixels` moves the same distance a notch
    /// moves any list here. The text being edited scrolls by whole lines,
    /// through its own accumulator.
    fn handle_scroll(&mut self, _x: f32, _y: f32, dy: f32) {
        if let Some(doc) = self.documents.get_mut(self.active_tab) {
            let step = wheel::pixels(dy, LINE_HEIGHT);
            match doc.view_mode {
                ViewMode::Tree => {
                    doc.tree_scroll = (doc.tree_scroll + step).max(0.0);
                }
                ViewMode::Raw if doc.source.is_some() => {
                    if let Some(source) = doc.source.as_mut() {
                        let rows = source.wheel.rows(dy);
                        let last = source.area.line_count().saturating_sub(1);
                        source.scroll = source.scroll.saturating_add_signed(rows).min(last);
                    }
                }
                ViewMode::Raw | ViewMode::Yaml | ViewMode::Stats => {
                    doc.raw_scroll = (doc.raw_scroll + step).max(0.0);
                }
                ViewMode::Diff => {
                    doc.diff_scroll = (doc.diff_scroll + step).max(0.0);
                }
            }
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Route a compositor event into the app.
    ///
    /// `handle_key` below already existed and was already tested; nothing
    /// dispatched to it, because nothing delivered an event. This is that
    /// dispatch, and the translation of a `KeyEvent` into the three arguments
    /// `handle_key` takes.
    /// Put the file picker up, listing the directory it starts in.
    pub fn open_file_dialog(&mut self) {
        self.picker_purpose = PickerPurpose::Open;
        self.picker.open_to_read();
    }

    /// Document `id`, if it is still open.
    fn doc_mut(&mut self, id: u64) -> Option<&mut Document> {
        self.documents.iter_mut().find(|d| d.id == id)
    }

    /// Save the active document: over its own file, or through the picker
    /// when it has none.
    ///
    /// It could not be saved at all. The tree edited values and deleted nodes
    /// and marked the tab modified, and every one of those changes was lost
    /// when the window closed.
    fn save_active(&mut self) {
        // A value half typed is part of what the user means to save.
        if self.editing_path.is_some() {
            self.commit_edit();
        }
        let Some(doc) = self.documents.get(self.active_tab) else {
            return;
        };
        let id = doc.id;
        if doc.path.is_none() {
            self.ask_where_to_save(PickerPurpose::SaveAs(id));
        } else {
            self.note = Some(self.save_to_own_file(id));
        }
    }

    /// Save As: the picker, starting beside the document's own file.
    fn save_active_as(&mut self) {
        if self.editing_path.is_some() {
            self.commit_edit();
        }
        if let Some(id) = self.documents.get(self.active_tab).map(|d| d.id) {
            self.ask_where_to_save(PickerPurpose::SaveAs(id));
        }
    }

    /// Put the picker up to choose where the document `purpose` names goes.
    fn ask_where_to_save(&mut self, purpose: PickerPurpose) {
        let (PickerPurpose::SaveAs(id)
        | PickerPurpose::SaveThenClose(id)
        | PickerPurpose::SaveThenQuit(id)) = purpose
        else {
            return;
        };
        let Some(doc) = self.documents.iter().find(|d| d.id == id) else {
            return;
        };
        // Beside the file it came from, when it came from one: a copy of a
        // file is nearly always wanted near the original, not in `$HOME`.
        let start = doc
            .path
            .as_deref()
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(FilePicker::default_start, Path::to_path_buf);
        let dialog = FileDialog::save()
            .with_initial_path(start)
            .with_filename(doc.save_name());
        self.picker_purpose = purpose;
        self.picker.put_up(dialog, true);
    }

    /// Write document `id` over its own file. What to say about it.
    ///
    /// Refused for a document holding only part of its file: writing it back
    /// would cut the file off where the reading stopped.
    fn save_to_own_file(&mut self, id: u64) -> String {
        let Some(doc) = self.doc_mut(id) else {
            return String::from("Nothing to save");
        };
        let Some(path) = doc.path.clone() else {
            return String::from("Not saved: it has no file yet -- use Save As");
        };
        if let Some(whole) = doc.whole_len {
            return format!(
                "Not saved: only part of {} was read ({whole} bytes is over the {MAX_OPEN_BYTES} read), and writing it back would cut the file short -- save as a new file instead",
                path.display()
            );
        }
        match safeio::write_atomically(&path, doc.input.as_bytes()) {
            Ok(()) => {
                doc.dirty = false;
                format!("Saved {}", path.display())
            }
            Err(err) => format!("Not saved: could not write {}: {err}", path.display()),
        }
    }

    /// Write document `id` to `path`, which becomes its file. What to say.
    fn save_to(&mut self, id: u64, path: &Path) -> Result<String, String> {
        let Some(doc) = self.doc_mut(id) else {
            return Err(String::from("Nothing to save"));
        };
        match safeio::write_atomically(path, doc.input.as_bytes()) {
            Ok(()) => {
                doc.path = Some(path.to_path_buf());
                // All of the document is in the new file: nothing left to cut.
                doc.whole_len = None;
                doc.dirty = false;
                if let Some(name) = path.file_name() {
                    // The tab's label only; the real name is `path`.
                    doc.title = name.to_string_lossy().into_owned();
                }
                Ok(format!("Saved {}", path.display()))
            }
            Err(err) => Err(format!(
                "Not saved: could not write {}: {err}",
                path.display()
            )),
        }
    }

    /// The picker chose `path`: do what it was put up for.
    fn picked(&mut self, path: &Path) {
        match self.picker_purpose {
            PickerPurpose::Open => self.note = Some(self.open_path(path)),
            PickerPurpose::SaveAs(id) => {
                self.note = Some(match self.save_to(id, path) {
                    Ok(said) | Err(said) => said,
                });
            }
            PickerPurpose::SaveThenClose(id) => {
                let said = match self.save_to(id, path) {
                    Ok(said) => {
                        if let Some(index) = self.index_of(id) {
                            self.close_tab(index);
                        }
                        said
                    }
                    Err(said) => said,
                };
                self.note = Some(said);
            }
            PickerPurpose::SaveThenQuit(id) => match self.save_to(id, path) {
                Ok(_) => self.continue_quitting(),
                Err(said) => self.note = Some(said),
            },
        }
    }

    /// Close tab `index`, asking first if it has unsaved changes.
    fn request_close_tab(&mut self, index: usize) {
        // The last tab is never closed -- `close_tab` keeps one -- so there
        // is no close to ask about.
        if self.documents.len() <= 1 {
            return;
        }
        if index == self.active_tab && self.editing_path.is_some() {
            // A value half typed is a change too.
            self.commit_edit();
        }
        match self.documents.get(index) {
            Some(doc) if doc.dirty => {
                let (id, message) = (doc.id, unsaved::message_for(&[&doc.title]));
                self.switch_to(index);
                self.question = Some(Question::new(
                    &message,
                    "Save them before the tab closes?",
                    CloseScope::Tab(id),
                ));
            }
            Some(_) => self.close_tab(index),
            None => {}
        }
    }

    /// The window has been asked to close. Whether it may go now; if not,
    /// the question is up.
    fn request_quit(&mut self) -> bool {
        if self.editing_path.is_some() {
            self.commit_edit();
        }
        if self.documents.iter().any(|d| d.dirty) {
            // The question replaces whatever was up: a picker left open would
            // take the keys the question needs, and be drawn over it.
            self.picker.close();
            self.show_help = false;
            let names: Vec<&str> = self
                .documents
                .iter()
                .filter(|d| d.dirty)
                .map(|d| d.title.as_str())
                .collect();
            self.question = Some(Question::new(
                &unsaved::message_for(&names),
                "Save them before closing?",
                CloseScope::Window,
            ));
            false
        } else {
            self.quit = true;
            true
        }
    }

    /// Answer the close question put before `scope`.
    fn answer_close(&mut self, scope: CloseScope, choice: Choice) {
        match (scope, choice) {
            (_, Choice::Cancel) => {}
            (CloseScope::Tab(id), Choice::Discard) => {
                if let Some(index) = self.index_of(id) {
                    self.close_tab(index);
                }
            }
            (CloseScope::Tab(id), Choice::Save) => {
                let Some(index) = self.index_of(id) else {
                    return;
                };
                self.switch_to(index);
                let own_file = self
                    .documents
                    .get(index)
                    .is_some_and(|d| d.path.is_some() && d.whole_len.is_none());
                if own_file {
                    let said = self.save_to_own_file(id);
                    if self.documents.get(index).is_some_and(|d| !d.dirty) {
                        self.close_tab(index);
                    }
                    self.note = Some(said);
                } else {
                    self.ask_where_to_save(PickerPurpose::SaveThenClose(id));
                }
            }
            (CloseScope::Window, Choice::Discard) => self.quit = true,
            (CloseScope::Window, Choice::Save) => self.continue_quitting(),
        }
    }

    /// Carry on closing the window: save everything that can go back to its
    /// own file, ask where to put the first thing that cannot, and quit once
    /// nothing is left unsaved. A failed save stops it, with the window open
    /// and the failure on the status line.
    fn continue_quitting(&mut self) {
        let own_files: Vec<u64> = self
            .documents
            .iter()
            .filter(|d| d.dirty && d.path.is_some() && d.whole_len.is_none())
            .map(|d| d.id)
            .collect();
        for id in own_files {
            let said = self.save_to_own_file(id);
            if self.documents.iter().any(|d| d.id == id && d.dirty) {
                self.note = Some(format!("{said} -- so the window stays open"));
                return;
            }
        }
        match self.documents.iter().find(|d| d.dirty).map(|d| d.id) {
            Some(id) => {
                if let Some(index) = self.index_of(id) {
                    self.switch_to(index);
                }
                self.ask_where_to_save(PickerPurpose::SaveThenQuit(id));
            }
            None => self.quit = true,
        }
    }

    /// Do what a toolbar button does -- the same as its key.
    fn toolbar(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::New => self.new_tab(),
            ToolbarAction::Open => self.open_file_dialog(),
            ToolbarAction::Save => self.save_active(),
            ToolbarAction::Search => self.search_visible = !self.search_visible,
            ToolbarAction::Edit => self.edit_mode = !self.edit_mode,
        }
    }

    /// Read `path` into a new tab. Returns what to say about it.
    ///
    /// A file that is not JSON is still *opened*: the text goes in and the
    /// parse error is what the viewer is for. What is reported here is the
    /// read, not the parse -- those are different failures, and conflating
    /// them would tell someone their disk was unreadable when their braces
    /// were unbalanced.
    ///
    /// Bounded at [`MAX_OPEN_BYTES`], and it says so when it cuts. A JSON
    /// document truncated in the middle is not valid JSON, so a silent cut
    /// would show a parse error that is about this cap rather than about the
    /// file -- the worst kind, because it accuses the user's data.
    pub fn open_path(&mut self, path: &std::path::Path) -> String {
        let shown = path.display().to_string();
        // Bounded before it is read, not after: `read_to_string` then a cut
        // held the whole file first, so the cap stopped nothing a cap is for.
        let read = match safeio::read_to_string_capped(path, MAX_OPEN_BYTES) {
            Ok(read) => read,
            Err(err) => return format!("Could not read {shown}: {err}"),
        };
        let (input, whole, truncated) = (read.text, read.whole, read.truncated);

        let name = path
            .file_name()
            .map_or_else(|| shown.clone(), |n| n.to_string_lossy().into_owned());
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        let mut doc = Document::new(id, name);
        doc.path = Some(path.to_path_buf());
        doc.whole_len = truncated.then_some(whole);
        // The raw view starts in the file's own indent rather than always in
        // two spaces, so what it shows is how the file is written.
        doc.indent = IndentStyle::detect(&input).unwrap_or(IndentStyle::Spaces2);
        doc.input = input;
        doc.reparse();
        self.documents.push(doc);
        self.switch_to(self.documents.len().saturating_sub(1));

        if truncated {
            // Front-loaded on purpose. This lands in a status bar with a
            // bounded width and `TextOverflow::Ellipsis`, so whatever is at
            // the end may not survive. The clause that must survive is the one
            // saying the document is incomplete, so it goes first and the file
            // name -- which the user just chose and already knows -- goes last.
            format!(
                "INCOMPLETE: only the first {MAX_OPEN_BYTES} bytes are shown, of {whole} in {shown}"
            )
        } else {
            format!("Opened {shown}")
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        // The close question has every key and click while it is up: a key
        // that reached the document under it would be a change made while
        // being asked whether to keep the changes. Each one it takes is a
        // redraw, since focus and hover inside it are its own business.
        if let Some(question) = self.question.as_mut()
            && matches!(event, Event::Key(_) | Event::Mouse(_))
        {
            if let Some(choice) = question.handle(event) {
                let scope = question.pending();
                self.question = None;
                self.answer_close(scope, choice);
            }
            return EventResult::Consumed;
        }
        // The picker takes the event first while it is up, or a click meant
        // for a filename lands on the tree behind it.
        match self.picker.handle(event, self.width, self.height) {
            Picked::Chose(path) => {
                self.picked(&path);
                return EventResult::Consumed;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key_ev) => {
                if !key_ev.pressed {
                    return EventResult::Ignored;
                }
                // `handle_key` reports nothing about whether it did anything,
                // and an app that answers `Consumed` to every key redraws on
                // the ones it ignored. Comparing the state around the call
                // beats making every arm of a long match remember to report.
                let before = self.state_fingerprint();
                self.handle_key(key_ev);
                if self.state_fingerprint() == before {
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                }
            }
            Event::Mouse(mouse_ev) => {
                // The click and scroll handlers below were in the same position
                // as `handle_key`: written, and called by nothing.
                let before = self.state_fingerprint();
                match mouse_ev.kind {
                    MouseEventKind::Press(button) => {
                        self.handle_mouse(mouse_ev.x, mouse_ev.y, button);
                    }
                    MouseEventKind::Scroll { dy, .. } => {
                        self.handle_scroll(mouse_ev.x, mouse_ev.y, dy);
                    }
                    _ => return EventResult::Ignored,
                }
                if self.state_fingerprint() == before {
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                }
            }
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.width = *width as f32;
                    self.height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// A cheap summary of everything a keystroke can change.
    ///
    /// A tuple of small copies rather than a hash: a hash could collide and
    /// silently drop a redraw.
    fn state_fingerprint(&self) -> Fingerprint {
        let doc = self.documents.get(self.active_tab);
        Fingerprint {
            active_tab: self.active_tab,
            search_query: self.search_query.clone(),
            search_index: self.search_index,
            search_visible: self.search_visible,
            search_case_sensitive: self.search_case_sensitive,
            edit_mode: self.edit_mode,
            edit_buffer: self.edit_buffer.clone(),
            source: doc
                .and_then(|d| d.source.as_ref())
                .map(|s| (s.area.caret(), s.area.anchor(), s.scroll, s.hscroll)),
            selected_node: doc.map_or(0, |d| d.selected_node),
            view_mode: doc.map_or(ViewMode::Tree, |d| d.view_mode),
            picker_open: self.picker.is_open(),
            show_help: self.show_help,
            expanded: doc.map_or(0, |d| d.expanded_paths.len()),
            revision: doc.map_or(0, |d| d.revision),
            tree_scroll: doc.map_or(0.0, |d| d.tree_scroll),
            raw_scroll: doc.map_or(0.0, |d| d.raw_scroll),
            diff_scroll: doc.map_or(0.0, |d| d.diff_scroll),
            tabs: self
                .documents
                .iter()
                .map(|d| (d.title.clone(), d.dirty))
                .collect(),
            note: self.note.clone(),
            question: self.question.as_ref().map(Question::pending),
            quit: self.quit,
        }
    }

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    fn render_commands(&mut self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: self.palette.base,
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
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            self.width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 14.0,
            text: String::from("JSON Viewer"),
            color: self.palette.ink(self.palette.lavender),
            font_size: TITLE_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Toolbar buttons
        for (action, label, bx, bw) in toolbar_buttons() {
            let color = match action {
                ToolbarAction::New | ToolbarAction::Open | ToolbarAction::Save => self.palette.blue,
                ToolbarAction::Search => self.palette.teal,
                ToolbarAction::Edit if self.edit_mode => self.palette.green,
                ToolbarAction::Edit => self.palette.subtext0,
            };
            self.palette.push_surface(
                cmds,
                bx,
                TOOLBAR_BUTTON_Y,
                bw,
                TOOLBAR_BUTTON_H,
                4.0,
                Surface::Card,
            );
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: 18.0,
                text: label.to_string(),
                color,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Separator line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.width,
            y2: TOOLBAR_HEIGHT,
            color: self.palette.surface1,
            width: 1.0,
        });
    }

    fn render_tab_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT;

        // Tab bar background
        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.width,
            TAB_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        let (tabs, plus_x) = self.tab_rects();
        for (i, tab_x, tab_width) in tabs {
            let Some(doc) = self.documents.get(i) else {
                continue;
            };
            let is_active = i == self.active_tab;

            // Tab background
            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y: y + 4.0,
                width: tab_width,
                height: TAB_BAR_HEIGHT - 4.0,
                color: if is_active {
                    self.palette.base
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii {
                    top_left: 6.0,
                    top_right: 6.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            // Tab label (dirty marker included)
            let label = tab_label(doc);
            cmds.push(RenderCommand::Text {
                x: tab_x + 10.0,
                y: y + 16.0,
                text: label,
                color: if is_active {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_size: SMALL_TEXT,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 20.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Close button
            if self.documents.len() > 1 {
                cmds.push(RenderCommand::Text {
                    x: tab_x + tab_width - 18.0,
                    y: y + 16.0,
                    text: String::from("x"),
                    color: self.palette.subtext0,
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // New tab button
        self.palette
            .push_surface(cmds, plus_x, y + 6.0, 28.0, 24.0, 4.0, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: plus_x + 8.0,
            y: y + 16.0,
            text: String::from("+"),
            color: self.palette.subtext0,
            font_size: NORMAL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Bottom line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_BAR_HEIGHT,
            x2: self.width,
            y2: y + TAB_BAR_HEIGHT,
            color: self.palette.surface1,
            width: 1.0,
        });
    }

    fn render_mode_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;

        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.width,
            30.0,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        let active_mode = self.active_doc().map_or(ViewMode::Tree, |d| d.view_mode);
        let mut mode_x = PADDING;
        for mode in &VIEW_MODES {
            let is_active = *mode == active_mode;
            let mode_width = mode_width(*mode);

            if is_active {
                self.palette.push_surface(
                    cmds,
                    mode_x,
                    y + 3.0,
                    mode_width,
                    24.0,
                    4.0,
                    Surface::Selected,
                );
            }

            cmds.push(RenderCommand::Text {
                x: mode_x + 10.0,
                y: y + 14.0,
                text: mode.label().to_string(),
                color: if is_active {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_size: SMALL_TEXT,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            mode_x += mode_width + 4.0;
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + 30.0,
            x2: self.width,
            y2: y + 30.0,
            color: self.palette.surface0,
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
            color: self.palette.base,
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
                self.render_stats_view(cmds, top, content_width, content_height);
            }
            Some(ViewMode::Diff) => self.render_diff_view(cmds, top, content_width, content_height),
            None => {
                cmds.push(RenderCommand::Text {
                    x: PADDING,
                    y: top + 30.0,
                    text: String::from("No document open"),
                    color: self.palette.subtext0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
                    text: String::from(
                        "Nothing here yet -- press Ctrl+O to open a JSON file, or Enter to type one",
                    ),
                    color: self.palette.subtext0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - PADDING * 2.0),
                    overflow: TextOverflow::Ellipsis,
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
                    self.palette.push_surface(
                        cmds,
                        0.0,
                        row_y,
                        width,
                        LINE_HEIGHT,
                        0.0,
                        Surface::Selected,
                    );
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
                        color: self.palette.subtext0,
                        font_size: SMALL_TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                // Label (key name or index)
                let label_width = text::measure(&node.label, NORMAL_TEXT, FontWeightHint::Bold);
                cmds.push(RenderCommand::Text {
                    x: indent_x,
                    y: row_y + 14.0,
                    text: node.label.clone(),
                    color: if node.expandable {
                        self.palette.ink(self.palette.mauve)
                    } else {
                        self.palette.ink(self.palette.blue)
                    },
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width * 0.4),
                    overflow: TextOverflow::Ellipsis,
                });

                // Colon separator
                let colon_x = indent_x + label_width + 4.0;
                cmds.push(RenderCommand::Text {
                    x: colon_x,
                    y: row_y + 14.0,
                    text: String::from(":"),
                    color: self.palette.subtext0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Value
                let value_x = colon_x + text::width(":", NORMAL_TEXT) + 4.0;
                cmds.push(RenderCommand::Text {
                    x: value_x,
                    y: row_y + 14.0,
                    text: node.value_display.clone(),
                    color: node.value_type.color(&self.palette),
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - value_x - PADDING),
                    overflow: TextOverflow::Ellipsis,
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
                color: self.palette.subtext0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
        if self.source_is_open() {
            self.render_source(cmds, top, width, height);
            return;
        }
        let doc = match self.documents.get_mut(self.active_tab) {
            Some(d) => d,
            None => return,
        };

        if let Some(ref error) = doc.error {
            let err_clone = error.clone();
            self.render_error_banner(cmds, top, width, &err_clone);
            return;
        }

        let highlighted = doc.get_highlighted(&self.palette);
        let scroll = doc.raw_scroll;
        let first_visible = (scroll / LINE_HEIGHT) as usize;
        let visible_count = (height / LINE_HEIGHT) as usize + 2;
        let last_visible = (first_visible + visible_count).min(highlighted.len());

        // Gutter (line numbers)
        let gutter_width = RAW_GUTTER;
        self.palette
            .push_surface(cmds, 0.0, top, gutter_width, height, 0.0, Surface::Sidebar);

        for i in first_visible..last_visible {
            let row_y = top + (i as f32 * LINE_HEIGHT) - scroll;

            // Line number
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: row_y + 14.0,
                text: format!("{}", i + 1),
                color: self.palette.subtext0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(gutter_width - 8.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Highlighted spans
            if let Some(line_spans) = highlighted.get(i) {
                let mut span_x = gutter_width + 4.0;
                for span in line_spans {
                    let span_weight = if span.bold {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    };
                    cmds.push(RenderCommand::Text {
                        x: span_x,
                        y: row_y + 14.0,
                        text: span.text.clone(),
                        color: span.color,
                        font_size: NORMAL_TEXT,
                        font_weight: span_weight,
                        max_width: Some(width - span_x - PADDING),
                        overflow: TextOverflow::Ellipsis,
                    });
                    // A key is drawn bold and its value is not, so each run
                    // has to advance the pen by its own width in its own
                    // weight; one nominal cell per byte put the value halfway
                    // through the key on any line with non-ASCII text.
                    span_x += text::measure(&span.text, NORMAL_TEXT, span_weight);
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
                color: self.palette.subtext0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    /// The text being edited as it is: its lines verbatim in the fixed-pitch
    /// face, the selection under them, the caret, and the line the parse
    /// failed on marked in the gutter -- with what the parse says at the foot.
    ///
    /// Only the part of a line that is on screen is sent: a minified document
    /// is one line, and drawing all of it each frame would send the whole file
    /// to the compositor for every key.
    fn render_source(&self, cmds: &mut Vec<RenderCommand>, top: f32, width: f32, height: f32) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        let Some(source) = doc.source.as_ref() else {
            return;
        };
        let rows = self.source_rows();
        let text_width = self.source_width();
        let bad_line = doc.error.as_ref().map(|e| e.line.saturating_sub(1));

        self.palette
            .push_surface(cmds, 0.0, top, RAW_GUTTER, height, 0.0, Surface::Sidebar);
        let selection = source.area.selection();
        let caret = source.area.caret();
        let mut start = 0_usize;
        let lines = source.area.text().split('\n');
        for (i, line) in lines.enumerate() {
            let end = start.saturating_add(line.len());
            if i >= source.scroll.saturating_add(rows) {
                break;
            }
            if i >= source.scroll {
                let row_y = top + (i - source.scroll) as f32 * LINE_HEIGHT;
                let bad = bad_line == Some(i);
                if bad {
                    cmds.push(RenderCommand::FillRect {
                        x: RAW_GUTTER,
                        y: row_y,
                        width: width - RAW_GUTTER,
                        height: LINE_HEIGHT,
                        color: Color::rgba(243, 139, 168, 24),
                        corner_radii: CornerRadii::ZERO,
                    });
                }
                cmds.push(RenderCommand::Text {
                    x: 4.0,
                    y: row_y + 14.0,
                    text: format!("{}", i + 1),
                    color: if bad {
                        self.palette.ink(self.palette.red)
                    } else {
                        self.palette.subtext0
                    },
                    font_size: SMALL_TEXT,
                    font_weight: if bad {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(RAW_GUTTER - 8.0),
                    overflow: TextOverflow::Ellipsis,
                });
                self.render_source_line(
                    cmds, source, line, start, row_y, text_width, selection, caret,
                );
            }
            start = end.saturating_add(1);
        }

        let (said, color) = match &doc.error {
            Some(error) => (
                format!("{error} -- Esc shows the formatted view"),
                self.palette.ink(self.palette.red),
            ),
            None if doc.parsed.is_some() => (
                String::from("Valid JSON -- Esc shows it formatted"),
                self.palette.subtext0,
            ),
            None => (
                String::from("Type or paste JSON -- Esc stops editing"),
                self.palette.subtext0,
            ),
        };
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: top + height - 6.0,
            text: said,
            color,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// One line of the text being edited, `line` starting at byte `start` of
    /// it, drawn at `row_y` within `text_width` pixels, scrolled sideways by
    /// the editor's `hscroll`.
    #[allow(
        clippy::too_many_arguments,
        reason = "one drawing step, split from its loop; bundling these into a struct would only rename them"
    )]
    fn render_source_line(
        &self,
        cmds: &mut Vec<RenderCommand>,
        source: &SourceEdit,
        line: &str,
        start: usize,
        row_y: f32,
        text_width: f32,
        selection: Option<(usize, usize)>,
        caret: usize,
    ) {
        let measure =
            |s: &str| text::measure_in(s, NORMAL_TEXT, FontWeightHint::Regular, FontFamily::Mono);
        let end = start.saturating_add(line.len());
        cmds.push(RenderCommand::PushClip {
            x: SOURCE_TEXT_X,
            y: row_y,
            width: text_width,
            height: LINE_HEIGHT,
        });
        if let Some((from, to)) = selection {
            let a = from.clamp(start, end).saturating_sub(start);
            let b = to.clamp(start, end).saturating_sub(start);
            if a < b {
                for (left, w) in text::selection_boxes_in(
                    line,
                    a,
                    b,
                    NORMAL_TEXT,
                    FontWeightHint::Regular,
                    FontFamily::Mono,
                ) {
                    cmds.push(RenderCommand::FillRect {
                        x: SOURCE_TEXT_X + left - source.hscroll,
                        y: row_y,
                        width: w,
                        height: LINE_HEIGHT,
                        color: self.palette.surface2,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }
        // The part on screen: from the character at the left edge to the one
        // past the right. A carriage return at the end is not drawn.
        let shown = line.strip_suffix('\r').unwrap_or(line);
        let from = text::cursor_at_in(
            shown,
            source.hscroll,
            NORMAL_TEXT,
            FontWeightHint::Regular,
            FontFamily::Mono,
        )
        .byte;
        let from = shown
            .get(..from)
            .and_then(|head| head.char_indices().next_back())
            .map_or(0, |(i, _)| i);
        let to = text::cursor_at_in(
            shown,
            source.hscroll + text_width,
            NORMAL_TEXT,
            FontWeightHint::Regular,
            FontFamily::Mono,
        )
        .byte;
        let to = shown
            .get(to..)
            .and_then(|tail| tail.chars().next())
            .map_or(shown.len(), |c| to.saturating_add(c.len_utf8()));
        if let Some(piece) = shown.get(from..to)
            && !piece.is_empty()
        {
            cmds.push(RenderCommand::PushFont {
                family: FontFamily::Mono,
            });
            cmds.push(RenderCommand::Text {
                x: SOURCE_TEXT_X + measure(shown.get(..from).unwrap_or("")) - source.hscroll,
                y: row_y + 14.0,
                text: piece.to_owned(),
                color: self.palette.text,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::PopFont);
        }
        if (start..=end).contains(&caret) {
            let x = measure(line.get(..caret.saturating_sub(start)).unwrap_or(""));
            cmds.push(RenderCommand::FillRect {
                x: SOURCE_TEXT_X + x - source.hscroll,
                y: row_y,
                width: guitk::textedit::CARET_WIDTH,
                height: LINE_HEIGHT,
                color: self.palette.text,
                corner_radii: CornerRadii::ZERO,
            });
        }
        cmds.push(RenderCommand::PopClip);
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
                color: self.palette.subtext0,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                    (self.palette.ink(self.palette.teal), false)
                } else if line.contains(':') {
                    (self.palette.ink(self.palette.blue), true)
                } else {
                    (self.palette.text, false)
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
                    overflow: TextOverflow::Ellipsis,
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
                    color: self.palette.subtext0,
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
        Table::fitted(
            cmds,
            PADDING,
            width - PADDING * 2.0,
            row_y,
            "Document Statistics",
            self.palette.lavender,
            HEADER_TEXT,
            Fit::Start,
            FontWeightHint::Bold,
        );
        row_y += section_gap;

        // General stats
        let stats = [
            ("Total Nodes", format!("{node_count}"), self.palette.peach),
            ("Max Depth", format!("{depth}"), self.palette.yellow),
            ("Approx Size", format_size(approx_bytes), self.palette.teal),
            (
                "Root Type",
                value.type_name().to_string(),
                self.palette.blue,
            ),
        ];

        let stats_cols = stats_columns(width);
        let general = stats_table(&stats_cols);

        for (label, val, color) in &stats {
            self.palette.push_surface(
                cmds,
                PADDING,
                row_y - 10.0,
                stats_card_width(width),
                28.0,
                4.0,
                Surface::Card,
            );
            general.cell_weighted(
                cmds,
                STATS_LABEL,
                row_y + 4.0,
                label,
                self.palette.text,
                NORMAL_TEXT,
                Fit::Start,
                FontWeightHint::Bold,
            );
            // The value is a formatted number, a size or a type name — read
            // left-to-right, so a cut belongs at the end.
            general.cell(
                cmds,
                STATS_VALUE,
                row_y + 4.0,
                val,
                *color,
                NORMAL_TEXT,
                Fit::Start,
            );
            row_y += 32.0;
        }

        row_y += 10.0;

        // Type distribution
        Table::fitted(
            cmds,
            PADDING,
            width - PADDING * 2.0,
            row_y,
            "Type Distribution",
            self.palette.lavender,
            HEADER_TEXT,
            Fit::Start,
            FontWeightHint::Bold,
        );
        row_y += section_gap;

        let total = counts.total().max(1) as f32;
        let type_rows = [
            ("Objects", counts.objects, self.palette.mauve),
            ("Arrays", counts.arrays, self.palette.lavender),
            ("Strings", counts.strings, self.palette.green),
            ("Numbers", counts.numbers, self.palette.peach),
            ("Booleans", counts.bools, self.palette.blue),
            ("Nulls", counts.nulls, self.palette.overlay0),
        ];

        let type_cols = type_columns(width);
        let type_table = stats_table(&type_cols);
        let bar_max_width = type_table.width(TYPE_BAR);
        let bar_x = type_table.left(TYPE_BAR);

        for (label, count, color) in &type_rows {
            let pct = *count as f32 / total;
            let bar_width = pct * bar_max_width;

            type_table.cell(
                cmds,
                TYPE_LABEL,
                row_y + 4.0,
                label,
                self.palette.text,
                NORMAL_TEXT,
                Fit::Start,
            );

            type_table.cell_weighted(
                cmds,
                TYPE_COUNT,
                row_y + 4.0,
                &count.to_string(),
                *color,
                NORMAL_TEXT,
                Fit::Start,
                FontWeightHint::Bold,
            );

            // Bar background. `bar_max_width` is a column width, so it is
            // already clamped at zero — a narrow panel shrinks the bar rather
            // than inverting it.
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: row_y - 4.0,
                width: bar_max_width,
                height: 16.0,
                color: self.palette.surface0,
                corner_radii: CornerRadii::all(3.0),
            });

            // Bar fill
            if bar_width > 1.0 {
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: row_y - 4.0,
                    width: bar_width,
                    height: 16.0,
                    color: *color,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            // Percentage. Its own column, so it sits right of the bar whatever
            // the panel width — it used to be positioned at `bar_x + bar_width`
            // and so followed a negative bar back across the count cell.
            type_table.cell(
                cmds,
                TYPE_PCT,
                row_y + 4.0,
                &format!("{:.1}%", pct * 100.0),
                self.palette.subtext0,
                SMALL_TEXT,
                Fit::Start,
            );

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
                color: self.palette.subtext0,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            return;
        }

        let diff_results = &doc.diff_results;

        // Header
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: top + 20.0,
            text: format!("Diff Results: {} difference(s)", diff_results.len()),
            color: self.palette.ink(self.palette.lavender),
            font_size: HEADER_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: self.palette.subtext0,
                font_size: NORMAL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                    DiffKind::Added => ("+", self.palette.green),
                    DiffKind::Removed => ("-", self.palette.red),
                    DiffKind::Changed => ("~", self.palette.yellow),
                    DiffKind::TypeChanged => ("!", self.palette.peach),
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
                    overflow: TextOverflow::Clip,
                });

                // Path
                cmds.push(RenderCommand::Text {
                    x: PADDING + 30.0,
                    y: row_y + 14.0,
                    text: entry.path.clone(),
                    color: self.palette.ink(self.palette.blue),
                    font_size: NORMAL_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width * 0.6),
                    overflow: TextOverflow::Ellipsis,
                });

                // Values
                if !entry.left.is_empty() {
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 30.0,
                        y: row_y + 32.0,
                        text: format!("L: {}", entry.left),
                        color: self.palette.ink(self.palette.red),
                        font_size: SMALL_TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.4),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
                if !entry.right.is_empty() {
                    cmds.push(RenderCommand::Text {
                        x: width * 0.45,
                        y: row_y + 32.0,
                        text: format!("R: {}", entry.right),
                        color: self.palette.ink(self.palette.green),
                        font_size: SMALL_TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.4),
                        overflow: TextOverflow::Ellipsis,
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
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sidebar_x,
            y1: top,
            x2: sidebar_x,
            y2: top + sidebar_height,
            color: self.palette.surface1,
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
            color: self.palette.ink(self.palette.lavender),
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                    color: self.palette.surface0,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: sidebar_x + PADDING + 6.0,
                    y: section_y + 8.0,
                    text: json_path,
                    color: self.palette.ink(self.palette.teal),
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 12.0),
                    overflow: TextOverflow::Ellipsis,
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
            color: self.palette.surface0,
            width: 1.0,
        });
        section_y += PADDING;

        cmds.push(RenderCommand::Text {
            x: sidebar_x + PADDING,
            y: section_y + 4.0,
            text: String::from("Validation"),
            color: self.palette.ink(self.palette.lavender),
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: self.palette.ink(self.palette.red),
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 6.0,
                y: section_y + 24.0,
                text: format!("Ln {}, Col {}", error.line, error.column),
                color: self.palette.ink(self.palette.red),
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 12.0),
                overflow: TextOverflow::Ellipsis,
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
                color: self.palette.ink(self.palette.green),
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            section_y += 30.0;
        } else {
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 6.0,
                y: section_y + 8.0,
                text: String::from("No input"),
                color: self.palette.subtext0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            section_y += 24.0;
        }

        // Quick info section
        cmds.push(RenderCommand::Line {
            x1: sidebar_x + PADDING,
            y1: section_y,
            x2: sidebar_x + SIDEBAR_WIDTH - PADDING,
            y2: section_y,
            color: self.palette.surface0,
            width: 1.0,
        });
        section_y += PADDING;

        cmds.push(RenderCommand::Text {
            x: sidebar_x + PADDING,
            y: section_y + 4.0,
            text: String::from("Quick Info"),
            color: self.palette.ink(self.palette.lavender),
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                    color: self.palette.subtext0,
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                cmds.push(RenderCommand::Text {
                    x: sidebar_x + PADDING + 80.0,
                    y: section_y + 4.0,
                    text: val.clone(),
                    color: self.palette.text,
                    font_size: SMALL_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
            color: self.palette.surface0,
            width: 1.0,
        });
        section_y += PADDING;

        cmds.push(RenderCommand::Text {
            x: sidebar_x + PADDING,
            y: section_y + 4.0,
            text: String::from("Shortcuts"),
            color: self.palette.ink(self.palette.lavender),
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: self.palette.ink(self.palette.yellow),
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: sidebar_x + PADDING + 80.0,
                y: section_y + 4.0,
                text: (*desc).to_string(),
                color: self.palette.subtext0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            section_y += 16.0;
        }
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - STATUS_BAR_HEIGHT;

        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.width,
            y2: y,
            color: self.palette.surface1,
            width: 1.0,
        });

        // What the last open or save did.
        //
        // Drawn before the document lookup below, which returns early when
        // there is no document -- and "there is no document" is exactly when a
        // failed open has the most to say.
        //
        // This field was computed and never rendered between 2026-09-15 and
        // the same evening, when lane A's `check-fields-written-never-read`
        // gate caught it on `main` and blocked their boot test. The gate was
        // right to refuse: `open_path` returns the truncation warning through
        // this field, so a document cut at 8 MiB was displayed with nothing
        // saying it was incomplete -- a JSON file cut in half is invalid JSON,
        // so the user would have seen a parse error about their file that was
        // really about our cap.
        if let Some(note) = &self.note {
            let x = PADDING + 110.0;
            let right = if self.edit_mode {
                self.width * 0.4
            } else {
                self.width - 130.0
            };
            cmds.push(RenderCommand::Text {
                x,
                y: y + 16.0,
                text: note.clone(),
                color: self.palette.subtext0,
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some((right - x - 8.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

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
        let status_color = if doc.error.is_some() {
            self.palette.red
        } else {
            self.palette.green
        };

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 16.0,
            text: format!("{} | {}", doc.view_mode.label(), status),
            color: status_color,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Center: edit mode indicator
        if self.edit_mode {
            cmds.push(RenderCommand::Text {
                x: self.width * 0.4,
                y: y + 16.0,
                text: String::from("EDIT MODE"),
                color: self.palette.ink(self.palette.yellow),
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Right: input size
        let size_info = format!("{} chars", doc.input.len());
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: y + 16.0,
            text: size_info,
            color: self.palette.subtext0,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_search_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let bar_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 30.0;
        let bar_height = SEARCH_BAR_HEIGHT;

        // Overlay background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.width - SIDEBAR_WIDTH,
            height: bar_height,
            color: self.palette.surface0,
            corner_radii: CornerRadii::ZERO,
        });

        // Search icon placeholder
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: bar_y + 18.0,
            text: String::from("Find:"),
            color: self.palette.subtext0,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Search input
        self.palette
            .push_surface(cmds, 50.0, bar_y + 4.0, 300.0, 28.0, 4.0, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: 58.0,
            y: bar_y + 18.0,
            text: if self.search_query.is_empty() {
                String::from("Search keys and values...")
            } else {
                self.search_query.clone()
            },
            color: if self.search_query.is_empty() {
                self.palette.subtext0
            } else {
                self.palette.text
            },
            font_size: NORMAL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(284.0),
            overflow: TextOverflow::Ellipsis,
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
                color: self.palette.ink(self.palette.teal),
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else if !self.search_query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 370.0,
                y: bar_y + 18.0,
                text: String::from("No matches"),
                color: self.palette.ink(self.palette.red),
                font_size: SMALL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Case sensitivity toggle
        cmds.push(RenderCommand::FillRect {
            x: CASE_BUTTON_X,
            y: bar_y + 6.0,
            width: CASE_BUTTON_W,
            height: 24.0,
            color: if self.search_case_sensitive {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: CASE_BUTTON_X + 7.0,
            y: bar_y + 18.0,
            text: String::from("Aa"),
            color: if self.search_case_sensitive {
                self.palette.crust
            } else {
                self.palette.subtext0
            },
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Close button
        cmds.push(RenderCommand::Text {
            x: self.search_close_x() + 4.0,
            y: bar_y + 18.0,
            text: String::from("Esc"),
            color: self.palette.subtext0,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Bottom line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: bar_y + bar_height,
            x2: self.width - SIDEBAR_WIDTH,
            y2: bar_y + bar_height,
            color: self.palette.surface1,
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
            color: self.palette.red,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Error title
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: top + PADDING + 20.0,
            text: String::from("Parse Error"),
            color: self.palette.ink(self.palette.red),
            font_size: NORMAL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Error details
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: top + PADDING + 40.0,
            text: format!(
                "Line {}, Col {}: {}",
                error.line, error.column, error.message
            ),
            color: self.palette.text,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0 - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // What to do about it: the one thing a reader of this banner wants.
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: top + PADDING + 60.0 + 22.0,
            text: String::from("Press Enter to edit the text where it goes wrong"),
            color: self.palette.subtext0,
            font_size: SMALL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0 - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

fn format_size(bytes: usize) -> String {
    guitk::bytes::iec(u64::try_from(bytes).unwrap_or(u64::MAX))
}

fn char_to_byte_pos(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map_or(s.len(), |(byte_idx, _)| byte_idx)
}

// ============================================================================
// Sample JSON
// ============================================================================

/// The most of a file one open will read.
///
/// Reported when it bites: a JSON document cut in half is not valid JSON, so a
/// silent truncation would show a parse error about this cap while appearing
/// to be about the file.
pub const MAX_OPEN_BYTES: usize = 8 * 1024 * 1024;

/// A sample document, for tests.
///
/// `#[cfg(test)]` since 2026-09-15. `App::new` loaded it, so a viewer that
/// could not read a file showed a description of itself instead. A fixture
/// production can reach is a fixture that ships.
#[cfg(test)]
const SAMPLE_JSON: &str = r#"{
  "name": "Slate OS JSON Viewer",
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

impl oswindow::app::App for App {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "JSON Viewer".to_owned()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.width as u32, self.height as u32)
        }
    }

    /// No clock.
    ///
    /// Nothing here advances on its own: the tree expands when it is expanded
    /// and the view scrolls when it is scrolled. There is no animation and no
    /// data that ages, so a tick would redraw an identical frame.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // Closing over unsaved work asks first, and the window waits for the
        // answer: `KeepOpen` declines the close and draws the question.
        if matches!(event, Event::CloseRequested) {
            return if self.request_quit() {
                Response::Exit
            } else {
                Response::KeepOpen
            };
        }
        let result = self.handle_event(event);
        if self.quit {
            // The question was answered, or the last save it asked for was
            // made: nothing is left unsaved.
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.width = width;
        self.height = height;
        let mut commands = self.render_commands();
        // The close question over the document it is asking about. It and the
        // picker are never up together: answering the question takes it down
        // before any picker goes up, and raising it takes the picker down.
        let palette = self.palette;
        if let Some(question) = self.question.as_mut() {
            commands.extend(question.commands(&palette, width, height));
        }
        // Last, so it is above everything -- the same order in which
        // `handle_event` gives it the click.
        commands.extend(self.picker.render(&self.palette, width, height));

        // And the shortcut list over even that, because it is the one thing a
        // reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut commands,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
        }
        RenderTree { commands }
    }
}

fn main() -> ExitCode {
    let mut app = App::new();
    app::launch("jsonviewer", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::single_char_pattern
)]
mod tests {

    use super::*;

    /// What an open did reaches the window.
    ///
    /// `note` (then `last_open`) was assigned by `open_path` and rendered by nothing between
    /// 2026-09-15 and the same evening, when lane A's
    /// `check-fields-written-never-read` gate caught it on `main` and blocked
    /// their boot test. The gate was right to refuse rather than be
    /// baselined: the field carries the truncation warning, so a document cut
    /// at the read cap was shown with nothing saying it was incomplete -- and
    /// a JSON file cut in half is invalid JSON, so the user would have met a
    /// parse error about their own file that was really about our cap.
    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The open test asserts that the KEY HANDLER opened the dialog, which
    /// holds whether or not the picker is ever handed another event -- and in
    /// this crate that test called `handle_key` directly, one layer below the
    /// routing. This is the half routing decides: with a dialog up, a
    /// keystroke belongs to the dialog.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_document() {
        let mut app = app_with_sample();
        let before = app.documents.len();

        let mut ctrl = Modifiers::NONE;
        ctrl.ctrl = true;
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::O,
            pressed: true,
            modifiers: ctrl,
            text: String::new(),
        }));
        assert!(app.picker.is_open(), "control: the picker must be up");

        // Ctrl+N opens a new tab; at the dialog it is part of a filename.
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: ctrl,
            text: String::new(),
        }));
        assert_eq!(
            app.documents.len(),
            before,
            "Ctrl+N at the open dialog opened a tab behind it"
        );
    }

    #[test]
    fn the_status_bar_says_what_the_last_open_did() {
        let mut app = App::new();
        app.note = Some(String::from("INCOMPLETE: only the first 4 bytes are shown"));
        let tree = app.render(900.0, 700.0);
        assert!(
            tree.commands.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text.starts_with("INCOMPLETE:")
            )),
            "the open result never reached the screen",
        );
    }

    /// The warning leads with the part that must survive an ellipsis.
    ///
    /// It goes into a status bar with a bounded width and
    /// `TextOverflow::Ellipsis`. The clause that matters is that the document
    /// is incomplete, so it is first; the file name, which the user just
    /// picked, is last.
    #[test]
    fn the_truncation_warning_leads_with_the_word_that_matters() {
        let dir = std::env::temp_dir().join("slateos-jsonviewer-truncation");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("big.json");
        let body = vec![b' '; MAX_OPEN_BYTES + 64];
        std::fs::write(&path, &body).expect("write fixture");

        let mut app = App::new();
        let note = app.open_path(&path);
        std::fs::remove_file(&path).ok();

        assert!(
            note.starts_with("INCOMPLETE"),
            "the warning buries its point: {note}"
        );
        assert!(note.contains(&MAX_OPEN_BYTES.to_string()), "{note}");
    }

    // ------------------------------------------------------------------
    // Compositor routing, and the two features it made reachable
    // ------------------------------------------------------------------

    use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent};
    // The trait, so `app.render(w, h)` resolves: this app's state type is also
    // called `App`, which is why the impl names the trait in full.
    use oswindow::app::App as _;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// `Consumed` means something here that it does not mean in every app:
    /// `handle_event` compares a `state_fingerprint` around the call, so a key
    /// only reads as consumed if it actually changed the program. That makes
    /// this a stronger check than the same test elsewhere -- and it is why
    /// `show_help` had to join that fingerprint, or `F1` would have flipped it,
    /// the tuple would have compared equal, and the card would have been open
    /// and undrawn.
    ///
    /// The label is read by `guitk::shortcut` rather than matched against a
    /// table beside it here, which would be a third copy of the same fact.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = help_states().iter_mut().any(|app| {
                    app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Documents chosen so that between them every advertised key has work.
    fn help_states() -> Vec<App> {
        // Two tabs, tree view, selection at the top.
        let plain = two_tabs();

        // ...with the selection moved down, so `Up`, `Home` and the keys that
        // go back have somewhere to go.
        let mut moved = two_tabs();
        moved.handle_event(&press(Key::Down));

        // ...in the raw view, the one place `I` and `M` mean anything.
        let mut raw = two_tabs();
        raw.handle_event(&press(Key::Num2));

        // ...with editing on *and the selection on a child*, which is the one
        // state `Delete` can remove a node in: the root has no path, and
        // `delete_at_path` declines to delete the document itself.
        let mut editing = two_tabs();
        editing.handle_event(&press_ctrl(Key::E));
        editing.handle_event(&press(Key::Right));
        editing.handle_event(&press(Key::Down));

        // ...with a search that has matches, the one state `Ctrl+G` can step
        // through: with no matches `search_next` moves nothing, and nothing
        // moving is how this app reports `Ignored`.
        //
        // Its own document, with *three* matches: `two_tabs` holds one `a`
        // each, and stepping from the only match wraps straight back to it, so
        // nothing moves and the app rightly says nothing happened.
        let mut searching = App::new();
        searching.documents.clear();
        let text = r#"{"aa":1,"ab":2,"ac":3}"#;
        let mut doc = Document::new(0, "search".to_owned());
        doc.input = text.to_owned();
        doc.parsed = parse_json(text).ok();
        searching.documents.push(doc);
        searching.active_tab = 0;
        searching.handle_event(&press_ctrl(Key::F));
        searching.handle_event(&typed('a'));

        // ...in a view other than the tree, so `1` has one to return from:
        // setting the view already in force changes nothing, and nothing
        // changing is how this app reports `Ignored`.
        let mut elsewhere = two_tabs();
        elsewhere.handle_event(&press(Key::Num4));

        vec![plain, moved, raw, editing, searching, elsewhere]
    }

    /// **The "Aa" button can be turned on, and the search follows it.**
    ///
    /// `search_case_sensitive` was `false` at construction, passed to
    /// `search_json`, drawn twice as the colour of a button, and written only
    /// by a test. The toolbar showed a case-sensitivity control that could not
    /// be operated.
    ///
    /// Asserts the search result, not the flag: a toggle that flips a boolean
    /// the matcher ignores is the defect `apps/regextester`'s `multiline` had,
    /// and only a changed answer tells the two apart.
    #[test]
    fn matching_case_can_be_turned_on_and_changes_the_answer() {
        let mut app = App::new();
        app.documents.clear();
        let text = r#"{"Name":"x","name":"y"}"#;
        let mut doc = Document::new(0, "case".to_owned());
        doc.input = text.to_owned();
        doc.parsed = parse_json(text).ok();
        app.documents.push(doc);
        app.active_tab = 0;

        app.handle_event(&press_ctrl(Key::F));
        for c in "Name".chars() {
            app.handle_event(&typed(c));
        }
        let insensitive = app.search_results.len();
        assert!(insensitive >= 2, "the fixture should match both spellings");

        assert_eq!(
            app.handle_event(&press_ctrl(Key::I)),
            EventResult::Consumed,
            "Ctrl+I did not read as a redraw, so the button would not repaint"
        );
        assert!(app.search_case_sensitive, "the flag did not move");
        assert!(
            app.search_results.len() < insensitive,
            "the search returned the same answer with matching on: {} then {}",
            insensitive,
            app.search_results.len()
        );
    }

    /// **The shortcut list reaches the window.**
    ///
    /// The guard above reads the list against the handler; this reads it
    /// against the screen. `apps/rssreader`'s overlay drew twenty of its
    /// twenty-one rows for weeks, because its box was a third quantity
    /// agreeing with neither the list nor the handler.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = two_tabs();
        assert!(
            !help_text(&mut app).contains("F1 or ? closes this"),
            "the list is up before anybody asked for it"
        );

        assert_eq!(
            app.handle_event(&press(Key::F1)),
            EventResult::Consumed,
            "raising the list did not read as a redraw, so it would be invisible"
        );
        let shown = help_text(&mut app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        app.handle_event(&press(Key::Escape));
        assert!(
            !help_text(&mut app).contains("F1 or ? closes this"),
            "Escape did not close it"
        );
    }

    /// A `?` typed into the find bar stays a `?`.
    #[test]
    fn the_help_key_does_not_take_a_character_out_of_a_search() {
        let mut app = two_tabs();
        app.handle_event(&press_ctrl(Key::F));
        assert!(app.search_visible, "Ctrl+F did not open the find bar");
        app.handle_event(&typed('?'));
        assert!(
            !help_text(&mut app).contains("F1 or ? closes this"),
            "the help key was taken out of somebody's query"
        );
    }

    /// Every string the window is drawing, joined.
    fn help_text(app: &mut App) -> String {
        let (w, h) = (app.width, app.height);
        app.render(w, h)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// A key with Ctrl held.
    fn press_ctrl(k: Key) -> Event {
        let mut modifiers = Modifiers::NONE;
        modifiers.ctrl = true;
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    /// Two tabs holding documents that differ in one field.
    ///
    /// Each with its own id, as `App` gives them: the close question and the
    /// picker find their document by id, and two tabs sharing one would send
    /// a save meant for the second to the first.
    fn two_tabs() -> App {
        let mut app = App::new();
        app.documents.clear();
        for (title, text) in [
            ("left", r#"{"a":1,"b":"x"}"#),
            ("right", r#"{"a":2,"b":"x"}"#),
        ] {
            let mut doc = Document::new(app.next_tab_id, title.to_owned());
            app.next_tab_id += 1;
            doc.input = text.to_owned();
            doc.parsed = parse_json(text).ok();
            app.documents.push(doc);
        }
        app.active_tab = 0;
        app
    }

    #[test]
    fn a_key_event_reaches_the_key_handler() {
        // The dispatch is what was missing; `handle_key` itself was tested.
        let mut app = two_tabs();
        assert_eq!(app.handle_event(&press(Key::Num2)), EventResult::Consumed);
        assert_eq!(
            app.documents.first().map(|d| d.view_mode),
            Some(ViewMode::Raw)
        );
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = two_tabs();
        let release = Event::Key(KeyEvent {
            key: Key::Num2,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(
            app.documents.first().map(|d| d.view_mode),
            Some(ViewMode::Tree)
        );
    }

    #[test]
    fn the_diff_view_actually_diffs_something() {
        // `diff_json`, its four kinds, `run_diff` and a whole `render_diff_view`
        // were written and tested, and nothing ever filled `diff_source` — so
        // the view reported "0 difference(s)" for every document, always.
        let mut app = two_tabs();
        assert!(
            app.documents
                .first()
                .is_some_and(|d| d.diff_results.is_empty()),
            "nothing is diffed until the view is opened"
        );
        assert_eq!(app.handle_event(&press(Key::Num5)), EventResult::Consumed);
        assert_eq!(
            app.documents.first().map(|d| d.view_mode),
            Some(ViewMode::Diff)
        );
        let results = app
            .documents
            .first()
            .map(|d| d.diff_results.len())
            .unwrap_or_default();
        assert_eq!(results, 1, "the two tabs differ in exactly one field");
    }

    #[test]
    fn one_tab_alone_has_nothing_to_diff_against() {
        let mut app = two_tabs();
        app.documents.truncate(1);
        let _ = app.handle_event(&press(Key::Num5));
        // The count would be zero either way — a document compared with
        // itself has no differences. What the guard actually decides is
        // whether `diff_source` is filled at all, and that is what the diff
        // view reads to choose between "no source" and "0 difference(s)".
        // The second is a lie when there was nothing to compare against.
        assert!(
            app.documents
                .first()
                .is_some_and(|d| d.diff_source.is_empty()),
            "a single document should not be pointed at itself as a source"
        );
        assert!(
            app.documents
                .first()
                .is_some_and(|d| d.diff_results.is_empty()),
            "and so it has no differences to show"
        );
    }

    #[test]
    fn an_edited_value_is_written_back_into_the_document() {
        // `set_value_at_path` had four tests and no caller: Enter loaded a value
        // into the buffer and nothing ever wrote it back, so every edit was
        // discarded silently.
        let mut app = two_tabs();
        app.edit_mode = true;
        app.editing_path = Some(vec![PathSegment::Key("a".to_owned())]);
        app.edit_buffer = "42".to_owned();
        app.commit_edit();
        let value = app
            .documents
            .first()
            .and_then(|d| d.parsed.as_ref())
            .and_then(|v| match v {
                JsonValue::Object(o) => o.iter().find(|(k, _)| k == "a").map(|(_, v)| v.clone()),
                _ => None,
            });
        assert!(
            matches!(value, Some(JsonValue::Number(n)) if (n - 42.0).abs() < f64::EPSILON),
            "expected the number 42, got {value:?}"
        );
        assert!(
            app.documents.first().is_some_and(|d| d.dirty),
            "an edit should mark the document dirty"
        );
    }

    #[test]
    fn an_edit_keeps_the_difference_between_a_number_and_a_string() {
        // Which is the distinction this program exists to show, and would be
        // lost by storing every edit as text.
        let mut app = two_tabs();
        app.editing_path = Some(vec![PathSegment::Key("a".to_owned())]);
        app.edit_buffer = "\"42\"".to_owned();
        app.commit_edit();
        let value = app
            .documents
            .first()
            .and_then(|d| d.parsed.as_ref())
            .and_then(|v| match v {
                JsonValue::Object(o) => o.iter().find(|(k, _)| k == "a").map(|(_, v)| v.clone()),
                _ => None,
            });
        assert!(
            matches!(value, Some(JsonValue::Str(ref s)) if s == "42"),
            "expected the string \"42\", got {value:?}"
        );
    }

    #[test]
    fn typing_during_an_edit_is_text_and_not_a_shortcut() {
        // "5" is the diff view outside an edit and the character five inside
        // one.
        let mut app = two_tabs();
        app.editing_path = Some(vec![PathSegment::Key("a".to_owned())]);
        app.edit_buffer.clear();
        let _ = app.handle_event(&typed('5'));
        assert_eq!(app.edit_buffer, "5");
        assert_eq!(
            app.documents.first().map(|d| d.view_mode),
            Some(ViewMode::Tree),
            "typing during an edit switched the view"
        );
    }

    #[test]
    fn escape_abandons_an_edit_and_leaves_the_document_alone() {
        let mut app = two_tabs();
        let before = app.documents.first().and_then(|d| d.parsed.clone());
        app.editing_path = Some(vec![PathSegment::Key("a".to_owned())]);
        app.edit_buffer = "999".to_owned();
        let _ = app.handle_event(&press(Key::Escape));
        assert!(app.editing_path.is_none(), "Escape should end the edit");
        assert!(app.edit_buffer.is_empty());
        assert_eq!(
            app.documents.first().and_then(|d| d.parsed.clone()),
            before,
            "an abandoned edit changed the document"
        );
    }

    #[test]
    fn a_click_on_the_tab_bar_switches_tabs() {
        // The mouse layer — tab clicks, view-mode clicks, scrolling — was in
        // the same position as the keys: written, and called by nothing. The
        // tab bar is the part with geometry a test can aim at exactly.
        let mut app = two_tabs();
        assert_eq!(app.active_tab, 0);
        let first_width = app.documents.first().map(tab_width).unwrap_or(0.0);
        let second_tab_x = PADDING + first_width + 4.0 + 2.0;
        let click = Event::Mouse(MouseEvent {
            x: second_tab_x,
            y: TOOLBAR_HEIGHT + 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert_eq!(app.handle_event(&click), EventResult::Consumed);
        assert_eq!(
            app.active_tab, 1,
            "the click did not reach handle_tab_click"
        );
    }

    #[test]
    fn a_resize_is_taken_but_is_not_itself_a_redraw() {
        let mut app = two_tabs();
        assert_eq!(
            app.handle_event(&Event::Resize {
                width: 1600,
                height: 900
            }),
            EventResult::Ignored
        );
        assert!((app.width - 1600.0).abs() < f32::EPSILON);
        assert!((app.height - 900.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rendering_draws_something_in_every_view_at_an_awkward_size() {
        let mut app = two_tabs();
        for k in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5] {
            let _ = app.handle_event(&press(k));
            for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
                assert!(
                    !app.render(w, h).commands.is_empty(),
                    "drew nothing at {w}x{h}"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Saving, and asking before unsaved work is lost
    // ------------------------------------------------------------------

    /// A file in the temporary directory, removed when dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn with(tag: &str, text: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "slateos-jsonviewer-{tag}-{}-{unique}.json",
                std::process::id()
            ));
            std::fs::write(&path, text).expect("scratch file");
            Self(path)
        }

        fn read(&self) -> String {
            std::fs::read_to_string(&self.0).expect("read the file back")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_file(&self.0));
        }
    }

    /// `scratch` opened into a fresh window, with editing on.
    fn opened(scratch: &Scratch) -> App {
        let mut app = App::new();
        let said = app.open_path(&scratch.0);
        assert!(said.starts_with("Opened"), "{said}");
        app.edit_mode = true;
        app
    }

    /// Set top-level key `key` of the active document to `value`, through the
    /// same commit a typed edit ends in.
    fn set_key(app: &mut App, key: &str, value: &str) {
        app.editing_path = Some(vec![PathSegment::Key(key.to_owned())]);
        app.edit_buffer = value.to_owned();
        app.commit_edit();
    }

    fn press_ctrl_shift(k: Key) -> Event {
        let mut modifiers = Modifiers::NONE;
        modifiers.ctrl = true;
        modifiers.shift = true;
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    /// The picker chose `path`, as it does in a window: it takes itself down,
    /// then says what was chosen. Calling `picked` with the dialog still up
    /// would leave it to swallow the next keystroke.
    fn choose(app: &mut App, path: &Path) {
        assert!(app.picker.is_open(), "nothing was asking for a file");
        app.picker.close();
        app.picked(path);
    }

    /// What the close question is asking about, while it is up.
    fn asking(app: &App) -> Option<CloseScope> {
        app.question.as_ref().map(Question::pending)
    }

    fn click(app: &mut App, x: f32, y: f32) -> EventResult {
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }))
    }

    /// **An edited file is saved back to itself, in its own layout.**
    ///
    /// It could not be saved at all: the tree changed values and deleted nodes
    /// and marked the tab modified, and the changes went when the window did.
    /// And the text a tree edit leaves is the file's own indent, not the raw
    /// view's two spaces.
    #[test]
    fn an_edited_file_is_saved_back_to_itself() {
        let file = Scratch::with("save", "{\n    \"a\": 1,\n    \"b\": \"x\"\n}\n");
        let mut app = opened(&file);

        // Saving a file that has not changed is still a save, and says so.
        assert_eq!(
            app.handle_event(&press_ctrl(Key::S)),
            EventResult::Consumed,
            "the status line's news of a save was not a redraw"
        );
        assert!(
            app.note.as_deref().is_some_and(|n| n.starts_with("Saved")),
            "{:?}",
            app.note
        );

        set_key(&mut app, "a", "2");
        assert!(
            app.documents[app.active_tab].dirty,
            "control: an edit marks the tab"
        );
        assert_eq!(app.handle_event(&press_ctrl(Key::S)), EventResult::Consumed);
        assert_eq!(
            file.read(),
            "{\n    \"a\": 2,\n    \"b\": \"x\"\n}\n",
            "saved, in the file's own four-space indent"
        );
        assert!(
            !app.documents[app.active_tab].dirty,
            "a save clears the mark"
        );
    }

    /// A one-line file stays one line, and gains no final newline it lacked.
    #[test]
    fn a_one_line_file_is_saved_on_one_line() {
        let file = Scratch::with("oneline", r#"{"a":1,"b":[true,null]}"#);
        let mut app = opened(&file);
        set_key(&mut app, "a", "2");
        app.handle_event(&press_ctrl(Key::S));
        assert_eq!(file.read(), r#"{"a":2,"b":[true,null]}"#);
    }

    #[test]
    fn the_indent_is_read_from_the_first_indented_line() {
        for (text, expected) in [
            ("{\n  \"a\": 1\n}", Some(IndentStyle::Spaces2)),
            (
                "{\n    \"a\": {\n        \"b\": 1\n    }\n}",
                Some(IndentStyle::Spaces4),
            ),
            ("{\n        \"a\": 1\n}", Some(IndentStyle::Spaces8)),
            ("{\n\t\"a\": 1\n}", Some(IndentStyle::Tabs)),
            // Three is not a step this program writes.
            ("{\n   \"a\": 1\n}", None),
            // Nothing is indented.
            (r#"{"a":1}"#, None),
            // A blank line, or one of only spaces, says nothing.
            ("{\n\n  \n    \"a\": 1\n}", Some(IndentStyle::Spaces4)),
        ] {
            assert_eq!(IndentStyle::detect(text), expected, "{text:?}");
        }
    }

    /// **A file read only in part is never saved over.**
    ///
    /// At most `MAX_OPEN_BYTES` is read, and writing that back would cut the
    /// file off where the reading stopped. Save As writes it to a new file,
    /// which then holds all of the document and may be saved over.
    #[test]
    fn a_file_read_only_in_part_is_never_saved_over() {
        let mut text = String::from(r#"{"a":1}"#);
        text.push_str(&" ".repeat(MAX_OPEN_BYTES));
        let file = Scratch::with("partial", &text);
        let mut app = App::new();
        let said = app.open_path(&file.0);
        assert!(said.starts_with("INCOMPLETE"), "{said}");
        set_key(&mut app, "a", "2");
        assert!(
            app.documents[app.active_tab].dirty,
            "control: the cut text still parses, so it can be edited"
        );

        app.handle_event(&press_ctrl(Key::S));
        assert_eq!(
            std::fs::metadata(&file.0).map(|m| m.len()).ok(),
            u64::try_from(text.len()).ok(),
            "the file was cut short"
        );
        assert!(
            app.note
                .as_deref()
                .is_some_and(|n| n.starts_with("Not saved")),
            "{:?}",
            app.note
        );
        assert!(
            app.documents[app.active_tab].dirty,
            "a refused save is not a save"
        );

        let copy = Scratch::with("partial-copy", "");
        app.handle_event(&press_ctrl_shift(Key::S));
        assert!(app.picker.is_saving(), "Save As asks where");
        choose(&mut app, &copy.0);
        assert_eq!(copy.read(), r#"{"a":2}"#);
        assert_eq!(
            app.documents[app.active_tab].path.as_deref(),
            Some(copy.0.as_path())
        );

        // The copy holds all of the document, so it may be saved over.
        set_key(&mut app, "a", "3");
        app.handle_event(&press_ctrl(Key::S));
        assert_eq!(copy.read(), r#"{"a":3}"#, "{:?}", app.note);
    }

    #[test]
    fn an_untitled_document_is_saved_where_the_picker_says() {
        let out = Scratch::with("untitled-out", "");
        let mut app = two_tabs();
        set_key(&mut app, "a", "5");
        app.handle_event(&press_ctrl(Key::S));
        assert!(app.picker.is_saving(), "no file yet, so it asks where");
        choose(&mut app, &out.0);
        assert_eq!(out.read(), r#"{"a":5,"b":"x"}"#);
        let doc = &app.documents[0];
        assert_eq!(doc.path.as_deref(), Some(out.0.as_path()));
        assert!(!doc.dirty);
        assert_eq!(
            Some(doc.title.as_str()),
            out.0.file_name().and_then(|n| n.to_str()),
            "the tab is named for its new file"
        );
    }

    /// Save As starts beside the file, not in `$HOME`.
    ///
    /// In a directory of its own, which is neither `$HOME` nor the temporary
    /// directory the picker falls back to without one -- a file in either
    /// would pass whether or not the picker looked at it.
    #[test]
    fn save_as_starts_beside_the_documents_own_file() {
        let dir =
            std::env::temp_dir().join(format!("slateos-jsonviewer-beside-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory of its own");
        let path = dir.join("doc.json");
        std::fs::write(&path, "{}").expect("the file");
        let mut app = App::new();
        app.open_path(&path);
        app.handle_event(&press_ctrl_shift(Key::S));
        let start = app.picker.dialog().map(|d| d.current_path().to_path_buf());
        drop(std::fs::remove_dir_all(&dir));
        assert_eq!(start, Some(dir));
    }

    /// **Closing the window over unsaved work asks, and each answer is kept.**
    ///
    /// The window closed on any close request, answered `Exit` without
    /// looking, and the edits went with it.
    #[test]
    fn closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept() {
        let mut clean = App::new();
        assert_eq!(
            clean.on_event(&Event::CloseRequested),
            Response::Exit,
            "nothing unsaved: the window just goes"
        );

        let original = "{\n  \"a\": 1\n}\n";
        let file = Scratch::with("close", original);
        let mut app = opened(&file);
        set_key(&mut app, "a", "2");
        assert_eq!(
            app.on_event(&Event::CloseRequested),
            Response::KeepOpen,
            "the window must wait for the answer"
        );
        assert_eq!(asking(&app), Some(CloseScope::Window));
        assert!(
            help_text(&mut app).contains("Unsaved changes"),
            "the question is not drawn"
        );

        // A key meant as a shortcut goes to the question, not the document.
        app.on_event(&press(Key::Num2));
        assert_eq!(app.documents[app.active_tab].view_mode, ViewMode::Tree);

        // Cancel: the window stays, and so do the changes.
        assert_eq!(app.on_event(&press(Key::Escape)), Response::Redraw);
        assert_eq!(asking(&app), None);
        assert!(app.documents[app.active_tab].dirty);
        assert_eq!(file.read(), original);

        // Save: the file is written, and the window goes.
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(app.on_event(&press(Key::S)), Response::Exit);
        assert_eq!(file.read(), "{\n  \"a\": 2\n}\n");

        // Don't save, clicked: the window goes and the file is untouched.
        let other = Scratch::with("discard", original);
        let mut app = opened(&other);
        set_key(&mut app, "a", "3");
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        // Drawn first: a dialog's buttons are where it last drew them.
        help_text(&mut app);
        let (x, y) = app
            .question
            .as_ref()
            .and_then(|q| q.button_centre(Choice::Discard))
            .expect("the question is drawn");
        let discard = Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert_eq!(app.on_event(&discard), Response::Exit);
        assert_eq!(other.read(), original);
    }

    #[test]
    fn saving_on_close_writes_the_files_and_asks_where_for_the_untitled() {
        let file = Scratch::with("quit-own", "{\n  \"a\": 1\n}\n");
        let out = Scratch::with("quit-untitled", "");
        let mut app = opened(&file);
        set_key(&mut app, "a", "2");
        // The untitled tab App::new opens with, given something unsaved.
        app.switch_to(0);
        if let Some(doc) = app.documents.first_mut() {
            doc.input = String::from("[1]");
            doc.reparse();
        }
        app.editing_path = Some(vec![PathSegment::Index(0)]);
        app.edit_buffer = String::from("7");
        app.commit_edit();
        assert!(app.documents[0].dirty, "control: both tabs have changes");

        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(
            app.on_event(&press(Key::S)),
            Response::Redraw,
            "one document has no file, so the window waits while it asks where"
        );
        assert_eq!(
            file.read(),
            "{\n  \"a\": 2\n}\n",
            "the one with a file is saved without asking"
        );
        assert!(app.picker.is_saving());
        choose(&mut app, &out.0);
        assert_eq!(out.read(), "[7]");
        assert!(app.quit, "nothing is left unsaved, so the window may go");
    }

    #[test]
    fn a_tab_with_unsaved_changes_asks_before_it_closes() {
        let mut app = two_tabs();
        set_key(&mut app, "a", "9");
        app.handle_event(&press_ctrl(Key::W));
        assert_eq!(app.documents.len(), 2, "closed without asking");
        let id = app.documents[0].id;
        assert_eq!(asking(&app), Some(CloseScope::Tab(id)));

        app.handle_event(&typed('d'));
        assert_eq!(app.documents.len(), 1, "Don't save did not close it");
        assert_eq!(app.documents[0].title, "right");
    }

    /// A tab's "x" closes *that* tab, and the active tab stays the same
    /// document -- closing a tab before it shifts every index after it.
    #[test]
    fn a_tabs_close_mark_closes_that_tab_and_the_active_one_stays() {
        // Three tabs with the middle one active: were it the last, closing an
        // earlier tab would leave the index past the end, and the clamp back
        // onto the last tab would land on the right document by accident.
        let mut app = two_tabs();
        app.new_tab();
        app.switch_to(1);
        let active = app.documents[app.active_tab].id;
        let (tabs, _) = app.tab_rects();
        let (_, x, w) = tabs[0];
        assert_eq!(
            click(&mut app, x + w - 8.0, TOOLBAR_HEIGHT + 16.0),
            EventResult::Consumed
        );
        assert_eq!(app.documents.len(), 2, "the close mark did not close it");
        assert!(app.documents.iter().all(|d| d.title != "left"));
        assert_eq!(
            app.documents[app.active_tab].id, active,
            "the selection moved to a different document"
        );

        // The rest of the tab still only selects it.
        let (tabs, _) = app.tab_rects();
        let (_, x, _) = tabs[0];
        click(&mut app, x + 4.0, TOOLBAR_HEIGHT + 16.0);
        assert_eq!(app.documents.len(), 2);
        assert_eq!(app.active_tab, 0);
    }

    #[test]
    fn every_toolbar_button_does_what_it_says() {
        let centre = |action| {
            toolbar_buttons()
                .into_iter()
                .find(|b| b.0 == action)
                .map(|(_, _, x, w)| (x + w / 2.0, TOOLBAR_BUTTON_Y + TOOLBAR_BUTTON_H / 2.0))
                .expect("a button")
        };
        let mut app = two_tabs();

        let (x, y) = centre(ToolbarAction::New);
        assert_eq!(click(&mut app, x, y), EventResult::Consumed);
        assert_eq!(app.documents.len(), 3);

        let (x, y) = centre(ToolbarAction::Open);
        click(&mut app, x, y);
        assert!(app.picker.is_open() && !app.picker.is_saving());
        app.picker.close();

        let (x, y) = centre(ToolbarAction::Save);
        click(&mut app, x, y);
        assert!(app.picker.is_saving(), "a document with no file asks where");
        app.picker.close();

        let (x, y) = centre(ToolbarAction::Search);
        click(&mut app, x, y);
        assert!(app.search_visible);

        let (x, y) = centre(ToolbarAction::Edit);
        click(&mut app, x, y);
        assert!(app.edit_mode);
    }

    /// The find bar lies over the tree; a click on it is the bar's.
    #[test]
    fn the_find_bar_takes_its_own_clicks() {
        // The root is always expanded, so row 1 is key "a".
        let mut app = two_tabs();
        app.handle_event(&press_ctrl(Key::F));
        let bar_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 30.0;

        assert_eq!(
            click(&mut app, CASE_BUTTON_X + 4.0, bar_y + 12.0),
            EventResult::Consumed
        );
        assert!(
            app.search_case_sensitive,
            "Aa did not turn matching case on"
        );

        // Row 1 of the tree lies under the bar here.
        click(&mut app, 200.0, bar_y + LINE_HEIGHT + 4.0);
        assert_eq!(
            app.documents[0].selected_node, 0,
            "a click on the find bar went through to the tree"
        );

        let close_x = app.search_close_x();
        click(&mut app, close_x + 6.0, bar_y + 12.0);
        assert!(!app.search_visible, "Esc on the bar did not close it");
    }

    /// A value being typed is committed to its own tab when the tab changes,
    /// not carried into the next one.
    #[test]
    fn a_value_being_typed_stays_with_its_own_tab() {
        let mut app = two_tabs();
        app.editing_path = Some(vec![PathSegment::Key("a".to_owned())]);
        app.edit_buffer = "7".to_owned();
        app.handle_event(&press_ctrl(Key::Tab));
        assert_eq!(app.active_tab, 1);
        assert!(
            app.editing_path.is_none(),
            "the edit followed into the next tab"
        );
        assert_eq!(app.documents[0].input, r#"{"a":7,"b":"x"}"#);
        assert_eq!(
            app.documents[1].input, r#"{"a":2,"b":"x"}"#,
            "the other tab was changed"
        );
    }

    /// Closing a tab redraws even when the tab that takes its place looks the
    /// same in every other respect -- two fresh tabs did, and the closed one
    /// stayed on screen.
    #[test]
    fn closing_a_tab_is_a_redraw_even_when_the_next_looks_the_same() {
        let mut app = App::new();
        app.new_tab();
        app.switch_to(0);
        assert_eq!(app.handle_event(&press_ctrl(Key::W)), EventResult::Consumed);
        assert_eq!(app.documents.len(), 1);
    }

    /// A tree edit leaves the text in the layout it had: one line or indented,
    /// with or without a final newline.
    #[test]
    fn a_tree_edit_keeps_the_texts_layout() {
        for (before, after) in [
            (r#"{"a":1,"b":[true,null]}"#, r#"{"a":2,"b":[true,null]}"#),
            ("{\"a\":1}\n", "{\"a\":2}\n"),
            ("{\n  \"a\": 1\n}", "{\n  \"a\": 2\n}"),
            ("{\n\t\"a\": 1\n}\n", "{\n\t\"a\": 2\n}\n"),
        ] {
            let mut doc = Document::new(1, String::from("layout"));
            doc.input = before.to_owned();
            doc.reparse();
            let root = doc.parsed.as_mut().expect("the fixture parses");
            assert!(set_value_at_path(
                root,
                &[PathSegment::Key(String::from("a"))],
                JsonValue::Number(2.0)
            ));
            doc.regenerate_input();
            assert_eq!(doc.input, after, "from {before:?}");
        }
    }

    /// The find bar's matches belong to the document on screen. Moving to a
    /// tab with nothing parsed kept the last tab's matches, highlighted against
    /// a document they are not in.
    #[test]
    fn the_matches_belong_to_the_document_on_screen() {
        let mut app = two_tabs();
        app.handle_event(&press_ctrl(Key::F));
        app.handle_event(&typed('a'));
        assert!(
            !app.search_results.is_empty(),
            "control: the search matches"
        );
        app.new_tab();
        assert!(
            app.search_results.is_empty(),
            "an empty tab shows the last tab's matches: {:?}",
            app.search_results
        );
    }

    /// A value half typed when the window closes is a change like any other:
    /// it is asked about, not dropped.
    #[test]
    fn a_value_being_typed_when_the_window_closes_is_asked_about() {
        let mut app = two_tabs();
        app.editing_path = Some(vec![PathSegment::Key("a".to_owned())]);
        app.edit_buffer = "8".to_owned();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(app.documents[0].input, r#"{"a":8,"b":"x"}"#);
    }

    /// Ctrl+S in the middle of typing a value saves the value.
    #[test]
    fn saving_keeps_the_value_being_typed() {
        let file = Scratch::with("save-mid-edit", r#"{"a":1}"#);
        let mut app = opened(&file);
        app.editing_path = Some(vec![PathSegment::Key("a".to_owned())]);
        app.edit_buffer = "6".to_owned();
        app.handle_event(&press_ctrl(Key::S));
        assert_eq!(file.read(), r#"{"a":6}"#);
        assert!(app.editing_path.is_none(), "the edit was left open");
    }

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
        // Use 3.25 (exactly representable in IEEE-754 binary64) so we
        // don't trip clippy::approx_constant — any literal close to PI
        // is flagged regardless of context.
        let v = parse_json("3.25").unwrap();
        if let JsonValue::Number(n) = v {
            assert!((n - 3.25).abs() < f64::EPSILON);
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

    /// The reported column is shown to the user as "Ln n, Col m" and is meant
    /// to be a character position. Counting UTF-8 bytes would push the column
    /// two or three places right for every non-ASCII character before it.
    #[test]
    fn error_column_counts_characters_not_bytes() {
        // Same document three times: the string value differs only in the
        // width of its characters, so the reported column must not differ.
        let ascii = parse_json("{\"a\": \"xxx\" }}").unwrap_err();
        for value in ["\"日本語\"", "\"ΩΩΩ\"", "\"😀😀😀\""] {
            let doc = format!("{{\"a\": {value} }}}}");
            let err = parse_json(&doc).unwrap_err();
            assert_eq!(
                err.column, ascii.column,
                "column shifted for {value}: got {} want {}",
                err.column, ascii.column
            );
        }
    }

    /// Control: an all-ASCII document reports exactly the column it always did.
    #[test]
    fn error_column_on_ascii_is_unchanged() {
        let err = parse_json("{\"a\": \"xxx\" }}").unwrap_err();
        assert_eq!(err.line, 1);
        // `}}` — the first `}` closes the object, the second is trailing junk
        // at 1-based character 14.
        assert_eq!(err.column, 14);
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
        // 3.25 is exactly representable — avoids clippy::approx_constant
        // (any literal near PI is flagged regardless of context).
        let s = format_number(3.25);
        assert!(s.contains("3.25"));
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
        assert!(!nodes.is_empty());
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let lines = highlight_json_text("{\"key\": 42}\n", &pal);
        assert!(!lines.is_empty());
    }

    #[test]
    fn highlight_preserves_structure() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let v = parse_json("{\"a\": 1}").unwrap();
        let formatted = format_json(&v, IndentStyle::Spaces2);
        let lines = highlight_json_text(&formatted, &pal);
        // Should have multiple lines (opening brace, key-value, closing brace)
        assert!(lines.len() >= 3);
    }

    // --- App state tests ---

    /// An app showing the sample document.
    ///
    /// `App::new` opens empty since 2026-09-15. The sample was production data
    /// until then -- a JSON document describing this program -- and six tests
    /// were resting on it without saying so, which is why they all went red at
    /// once. They build their own now.
    fn app_with_sample() -> App {
        let mut app = App::new();
        if let Some(doc) = app.documents.get_mut(0) {
            doc.input = SAMPLE_JSON.to_string();
            doc.reparse();
        }
        app
    }

    #[test]
    fn app_new_creates_default_doc() {
        let app = app_with_sample();
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
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn app_search_flow() {
        let mut app = app_with_sample();
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
        assert!(s.contains("KiB"));
    }

    #[test]
    fn format_size_megabytes() {
        let s = format_size(2_000_000);
        assert!(s.contains("MiB"));
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let colors = [
            ValueType::Null.color(&pal),
            ValueType::Bool.color(&pal),
            ValueType::Number.color(&pal),
            ValueType::Str.color(&pal),
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
    // --- Tab and chip geometry ---

    /// Clicking a tab has to select the tab the user sees. Hit-testing and
    /// rendering used to size the tab separately, and the two disagreed: the
    /// click test measured `doc.title` while the render measured the title
    /// plus its dirty marker, so a modified document's tab was drawn wider
    /// than the region that selected it and the strip drifted out of step
    /// from that tab onwards.
    #[test]
    fn a_dirty_tab_is_as_wide_as_it_looks() {
        let mut doc = Document::new(1, "notes.json".to_string());
        let clean = tab_width(&doc);
        doc.dirty = true;
        let dirty = tab_width(&doc);
        assert!(
            dirty > clean,
            "the dirty marker takes room, so the tab has to grow for it"
        );
        assert!(
            text::measure(&tab_label(&doc), SMALL_TEXT, FontWeightHint::Bold) <= dirty - 20.0,
            "the label does not fit inside the tab drawn for it"
        );
    }

    /// The active tab is drawn bold. If each tab were measured in its own
    /// current weight the whole strip would reflow every time the user
    /// switched tabs, so every tab is measured bold.
    #[test]
    fn the_tab_strip_does_not_reflow_on_selection() {
        let doc = Document::new(1, "a-reasonably-long-name.json".to_string());
        let w = tab_width(&doc);
        let bold = text::measure(&tab_label(&doc), SMALL_TEXT, FontWeightHint::Bold);
        let regular = text::measure(&tab_label(&doc), SMALL_TEXT, FontWeightHint::Regular);
        assert!(bold >= regular);
        assert!(w >= bold + 20.0, "no room for the close button");
    }

    /// Same rule for the view-mode chips, and every mode's label must fit.
    #[test]
    fn every_view_mode_chip_fits_its_label() {
        for mode in &VIEW_MODES {
            let w = mode_width(*mode);
            let label = text::measure(mode.label(), SMALL_TEXT, FontWeightHint::Bold);
            assert!(
                label <= w - 20.0 + 0.01,
                "{:?} does not fit its chip",
                mode.label()
            );
        }
    }

    // --- Statistics-view layout ---
    //
    // The statistics panel is the window minus the sidebar, so its width is
    // whatever the user leaves it. Its geometry used to be written as
    // constants subtracted from fractions of that width, which is only correct
    // for a wide window; these tests pin the behaviour at narrow ones.

    const STATS_TOP: f32 = 110.0;

    /// A statistics text cell: where it starts, what it says, and how it is
    /// drawn — the weight is part of the geometry, because it decides how wide
    /// the glyphs are and therefore where the text must be cut.
    type StatCell = (f32, String, f32, FontWeightHint);

    fn stats_commands(width: f32) -> Vec<RenderCommand> {
        // The sample document, because a statistics panel over an empty one
        // has no types to distribute and nothing to draw.
        let app = app_with_sample();
        let mut cmds = Vec::new();
        app.render_stats_view(&mut cmds, STATS_TOP, width, 600.0);
        cmds
    }

    /// The panel's text cells, split into the general-statistics rows and the
    /// type-distribution rows.
    ///
    /// The split is by the section header between them rather than by x or y:
    /// the two tables deliberately share a left inset, so x cannot tell a
    /// general label from a type label, and keying off row heights would
    /// re-derive the layout the test is supposed to be checking.
    fn stats_cells(width: f32) -> (Vec<StatCell>, Vec<StatCell>) {
        let (mut general, mut types) = (Vec::new(), Vec::new());
        let mut in_types = false;
        for cmd in stats_commands(width) {
            if let RenderCommand::Text {
                x,
                ref text,
                font_size,
                font_weight,
                ..
            } = cmd
            {
                if text == "Type Distribution" {
                    in_types = true;
                    continue;
                }
                if text == "Document Statistics" {
                    continue;
                }
                let cell = (x, text.clone(), font_size, font_weight);
                if in_types {
                    types.push(cell);
                } else {
                    general.push(cell);
                }
            }
        }
        assert!(
            in_types,
            "the type-distribution header was not drawn at width {width}"
        );
        (general, types)
    }

    /// Assert every cell fits the column it starts in; returns how many were
    /// actually checked, so the caller can prove the test was not vacuous.
    fn cells_fit_columns(cells: &[StatCell], columns: &[Column], what: &str) -> usize {
        let table = stats_table(columns);
        let spans = table.spans();
        let mut checked = 0;
        for (x, text, size, weight) in cells {
            let Some(&(left, right)) = spans.iter().find(|(l, _)| (l - x).abs() < 0.01) else {
                panic!("{what} cell {text:?} starts at {x}, which is no column's left edge");
            };
            let drawn = x + text::measure(text, *size, *weight);
            assert!(
                drawn <= right + 0.01,
                "{what} cell {text:?} at {left} draws to {drawn}, past its column edge {right}"
            );
            checked += 1;
        }
        checked
    }

    /// The row's columns are shares of the room there is, so they add up to
    /// exactly it. Fractions that did not sum to one would leave a ragged
    /// right edge visible only at one particular window width.
    #[test]
    fn the_type_rows_fill_the_panel() {
        let sum: f32 = TYPE_FRACTIONS.iter().sum();
        assert!((sum - 1.0).abs() < 0.001, "fractions sum to {sum}, not 1");

        for width in [240.0_f32, 320.0, 480.0, 880.0, 1600.0] {
            let columns = type_columns(width);
            let table = stats_table(&columns);
            let right = table.right(TYPE_PCT);
            assert!(
                (right - (width - PADDING)).abs() < 0.01,
                "at width {width} the type rows end at {right}, not at the margin {}",
                width - PADDING
            );
        }
    }

    /// The general-statistics cells live inside the card drawn behind them.
    #[test]
    fn the_general_stats_stay_inside_their_card() {
        for width in [240.0_f32, 320.0, 480.0, 880.0, 1600.0] {
            let columns = stats_columns(width);
            let table = stats_table(&columns);
            let card_right = PADDING + stats_card_width(width);
            let right = table.right(STATS_VALUE);
            assert!(
                right <= card_right + 0.01,
                "at width {width} the value column ends at {right}, past the card edge {card_right}"
            );
        }
    }

    /// No cell may draw past the column it starts in, at any window width.
    #[test]
    fn no_statistics_cell_escapes_its_column() {
        let mut checked = 0;
        for width in [240.0_f32, 320.0, 480.0, 880.0, 1600.0] {
            let (general, types) = stats_cells(width);
            checked += cells_fit_columns(&general, &stats_columns(width), "general");
            checked += cells_fit_columns(&types, &type_columns(width), "type");
        }
        // Four general rows of two cells and six type rows of three, per width.
        assert!(
            checked >= 5 * (4 * 2 + 6 * 3),
            "only {checked} cells checked"
        );
    }

    /// The regression test for the original bug: the bar was sized
    /// `width * 0.5 - 250.0`, negative for any panel under 500 px, and the
    /// percentage label was placed at `bar_x + bar_width + 8.0` — so on a
    /// narrow panel it walked left of its own bar and onto the count cell.
    #[test]
    fn the_percentage_is_drawn_right_of_its_bar() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        for width in [120.0_f32, 240.0, 320.0, 480.0, 499.0, 880.0] {
            let cmds = stats_commands(width);
            let bars: Vec<(f32, f32)> = cmds
                .iter()
                .filter_map(|c| match *c {
                    RenderCommand::FillRect {
                        x,
                        width: w,
                        height,
                        color,
                        ..
                    } if (height - 16.0).abs() < 0.01 && color == pal.surface0 => Some((x, w)),
                    _ => None,
                })
                .collect();
            // Found by position, not by a trailing '%': on a very narrow panel
            // the percentage column is too small for "12.5%" and the text is
            // legitimately cut away to nothing. Where it lands is the property
            // under test, and that is true of the cut cell too.
            let columns = type_columns(width);
            let pct_x = stats_table(&columns).left(TYPE_PCT);
            let percentages: Vec<&str> = cmds
                .iter()
                .filter_map(|c| match *c {
                    RenderCommand::Text { x, ref text, .. } if (x - pct_x).abs() < 0.01 => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(bars.len(), 6, "at width {width}: one bar per JSON type");
            assert_eq!(percentages.len(), 6, "at width {width}");

            for (&(bar_x, bar_w), text) in bars.iter().zip(&percentages) {
                assert!(
                    bar_w >= 0.0,
                    "at width {width} a bar has negative width {bar_w}"
                );
                assert!(
                    pct_x >= bar_x + bar_w - 0.01,
                    "at width {width} the percentage at {pct_x} is left of its bar's right edge {}",
                    bar_x + bar_w
                );
                // Once there is room for it, the number itself must survive —
                // a percentage column that always elides to nothing would
                // satisfy the placement check vacuously.
                assert!(
                    width < 320.0 || text.ends_with('%'),
                    "at width {width} the percentage reads {text:?}"
                );
            }
        }
    }

    /// Nothing in the panel may ask for a negative-width rect — not the bars,
    /// not the cards — however narrow, or absent, the panel is.
    #[test]
    fn no_statistics_rect_has_a_negative_width() {
        let mut checked = 0;
        for width in [0.0_f32, 1.0, 20.0, 60.0, 120.0, 240.0, 499.0, 880.0] {
            for cmd in stats_commands(width) {
                // Both kinds. The property here is geometry, not fill, and
                // since the surface conversion a card is an outline under the
                // default theme -- so counting only `FillRect` stopped seeing
                // the rectangles this was written to check.
                let rect = match cmd {
                    RenderCommand::FillRect { x, width: w, .. }
                    | RenderCommand::StrokeRect { x, width: w, .. } => Some((x, w)),
                    _ => None,
                };
                if let Some((x, w)) = rect {
                    assert!(w >= 0.0, "at width {width} a rect at {x} is {w} wide");
                    checked += 1;
                }
            }
        }
        assert!(checked >= 8 * (4 + 6), "only {checked} rects checked");
    }

    /// A statistic that fits is shown whole — the fitting must not leave a cut
    /// marker on text that was never cut.
    #[test]
    fn a_short_statistic_is_drawn_verbatim() {
        let (general, types) = stats_cells(880.0);
        for (_, text, _, _) in general.iter().chain(&types) {
            assert!(!text.contains('…'), "{text:?} was cut but had room");
        }
        assert!(
            general.iter().any(|(_, t, _, _)| t == "Total Nodes"),
            "the general statistics were not drawn"
        );
        assert!(
            types.iter().any(|(_, t, _, _)| t == "Objects"),
            "the type distribution was not drawn"
        );
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        use oswindow::app::App as _;
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut App) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = App::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ------------------------------------------------------------------
    // The text itself, edited in the raw view
    // ------------------------------------------------------------------

    /// A key as a keyboard sends it: the key, and the text it types.
    fn keyed(k: Key, text: &str) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_owned(),
        })
    }

    /// The top of the view's text: below the toolbar, the tabs and the chips.
    const CONTENT_Y: f32 = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 30.0;

    /// A window whose one document holds `text`, parsed.
    fn holding(text: &str) -> App {
        let mut app = App::new();
        app.documents[0].input = text.to_owned();
        app.documents[0].reparse();
        app
    }

    fn source(app: &App) -> &SourceEdit {
        app.documents[app.active_tab]
            .source
            .as_ref()
            .expect("the text is being edited")
    }

    /// **Enter in the raw view edits the text itself**, and what is typed is
    /// the document at once: parsed, marked unsaved, and seen as a change.
    #[test]
    fn enter_in_the_raw_view_edits_the_text_itself() {
        let mut app = two_tabs();
        app.handle_event(&press(Key::Num2));
        assert_eq!(app.handle_event(&press(Key::Enter)), EventResult::Consumed);
        assert_eq!(
            source(&app).area.caret(),
            0,
            "a document that parses starts at its start"
        );
        app.handle_event(&press_ctrl(Key::End));
        app.handle_event(&press(Key::Left));
        for c in r#","c":true"#.chars() {
            assert_eq!(app.handle_event(&typed(c)), EventResult::Consumed, "{c:?}");
        }
        let doc = &app.documents[0];
        assert_eq!(doc.input, r#"{"a":1,"b":"x","c":true}"#);
        assert!(doc.dirty, "an edited text was not marked unsaved");
        assert!(
            matches!(&doc.parsed, Some(JsonValue::Object(entries))
                if entries.iter().any(|(k, v)| k == "c" && *v == JsonValue::Bool(true))),
            "the tree does not see what was typed"
        );
        assert!(help_text(&mut app).contains(r#"{"a":1,"b":"x","c":true}"#));
    }

    /// **A document that does not parse is repaired where it goes wrong.**
    /// Enter in its tree -- which has only the error to show -- edits the
    /// text with the caret at the place the parse failed.
    #[test]
    fn a_document_that_does_not_parse_is_repaired_where_it_goes_wrong() {
        let mut app = holding(r#"{"a":1 "b":2}"#);
        assert!(app.documents[0].error.is_some());
        assert!(
            help_text(&mut app).contains("Press Enter to edit the text where it goes wrong"),
            "the banner does not say what to do"
        );
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.documents[0].view_mode, ViewMode::Raw);
        assert_eq!(
            source(&app).area.caret(),
            7,
            "the caret is not at the failure"
        );
        let (w, h) = (app.width, app.height);
        let red = app.palette.ink(app.palette.red);
        assert!(
            app.render(w, h).commands.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, color, .. } if text == "1" && *color == red
            )),
            "the line that fails is not marked"
        );
        let drawn = help_text(&mut app);
        assert!(
            drawn.contains("Line 1, Col 8"),
            "the parse's verdict is not drawn: {drawn}"
        );
        app.handle_event(&typed(','));
        let doc = &app.documents[0];
        assert_eq!(doc.input, r#"{"a":1 ,"b":2}"#);
        assert!(
            doc.error.is_none() && doc.parsed.is_some(),
            "the repair did not parse"
        );
        assert!(help_text(&mut app).contains("Valid JSON"));
    }

    /// A new document says how to give it content, and Enter does.
    #[test]
    fn a_new_document_can_be_typed_into() {
        let mut app = App::new();
        assert!(help_text(&mut app).contains("or Enter to type one"));
        app.handle_event(&press(Key::Enter));
        assert!(
            app.source_is_open(),
            "Enter on an empty tree did not edit the text"
        );
        assert!(help_text(&mut app).contains("Type or paste JSON"));
        for c in "[1, 2]".chars() {
            app.handle_event(&typed(c));
        }
        assert_eq!(app.documents[0].input, "[1, 2]");
        assert!(app.documents[0].parsed.is_some());
    }

    /// Escape stops editing, and keeps every change; the keys are the view's
    /// again.
    #[test]
    fn escape_stops_editing_and_keeps_the_changes() {
        let mut app = two_tabs();
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        // Twice: the first also marks the tab unsaved, which is a change of
        // its own; the second changes nothing but the text.
        for _ in 0..2 {
            assert_eq!(
                app.handle_event(&press(Key::Delete)),
                EventResult::Consumed,
                "a deletion moves no caret, and was taken for nothing happening"
            );
        }
        assert!(
            app.documents[0].error.is_some(),
            "a deleted brace still parsed"
        );
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Consumed);
        assert!(!app.source_is_open());
        assert_eq!(app.documents[0].input, r#"a":1,"b":"x"}"#);
        app.handle_event(&press(Key::Num1));
        assert_eq!(app.documents[0].view_mode, ViewMode::Tree);
    }

    /// While the text is edited, a key that types is typing: `5` is not the
    /// diff view, `?` not the shortcut list, `i` and `m` not the indent.
    #[test]
    fn a_typed_key_is_the_texts_while_it_is_edited() {
        let mut app = holding("[]");
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press(Key::Right));
        let (indent, minified) = (app.documents[0].indent, app.documents[0].minified);
        app.handle_event(&keyed(Key::Num5, "5"));
        app.handle_event(&keyed(Key::I, "i"));
        app.handle_event(&keyed(Key::M, "m"));
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Slash,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: String::from("?"),
        }));
        let doc = &app.documents[0];
        assert_eq!(doc.input, "[5im?]");
        assert_eq!(doc.view_mode, ViewMode::Raw);
        assert_eq!((doc.indent, doc.minified), (indent, minified));
        assert!(!app.show_help, "the shortcut list took a typed ?");
    }

    /// A press in the raw view edits the text, with the caret where it lands;
    /// a second press moves it.
    #[test]
    fn a_press_in_the_raw_view_puts_the_caret_where_it_lands() {
        let mut app = holding("[\n  1,\n  2\n]");
        app.handle_event(&press(Key::Num2));
        click(
            &mut app,
            SOURCE_TEXT_X + 1.0,
            CONTENT_Y + LINE_HEIGHT * 2.0 + 5.0,
        );
        let edit = source(&app);
        assert_eq!(
            edit.area.caret(),
            edit.area.start_of_line(2),
            "not the third line's start"
        );
        click(&mut app, SOURCE_TEXT_X + 500.0, CONTENT_Y + 5.0);
        assert_eq!(source(&app).area.caret(), 1, "past a line's end is its end");
    }

    /// A press on a parse error's banner goes to the error: there is no text
    /// on screen to point at.
    #[test]
    fn a_press_on_a_parse_error_goes_to_it() {
        let mut app = holding("[1,\n 2,,\n 3]");
        app.handle_event(&press(Key::Num2));
        click(&mut app, 300.0, CONTENT_Y + 300.0);
        let edit = source(&app);
        assert_eq!(
            edit.area.line_index(edit.area.caret()),
            1,
            "not on the line that fails"
        );
    }

    /// Leaving the raw view stops editing, as Escape does.
    #[test]
    fn leaving_the_raw_view_stops_editing() {
        let mut app = two_tabs();
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        click(
            &mut app,
            PADDING + 5.0,
            TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 15.0,
        );
        assert_eq!(app.documents[0].view_mode, ViewMode::Tree);
        assert!(
            !app.source_is_open(),
            "the tree view was left editing the text"
        );
    }

    /// The find bar and the text both take typed keys, so opening one closes
    /// the other.
    #[test]
    fn the_find_bar_takes_the_keys_from_the_text() {
        let mut app = two_tabs();
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press_ctrl(Key::F));
        assert!(app.search_visible && !app.source_is_open());
        app.handle_event(&typed('a'));
        assert_eq!(app.search_query, "a");
        assert_eq!(app.documents[0].input, r#"{"a":1,"b":"x"}"#);
        // And editing the text again closes the find bar.
        click(
            &mut app,
            SOURCE_TEXT_X + 1.0,
            CONTENT_Y + SEARCH_BAR_HEIGHT + 5.0,
        );
        assert!(
            app.source_is_open(),
            "a press below the find bar did not edit the text"
        );
        assert!(
            !app.search_visible,
            "the find bar stayed open over the text"
        );
    }

    /// The caret stays on screen: down a long document and along a long line.
    /// Only the part of a line that shows is drawn.
    #[test]
    fn the_caret_stays_on_screen() {
        let long = format!("[{}1]", "1,".repeat(400));
        let mut app = holding(&format!("{}{long}", "[1,\n".repeat(300)));
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press_ctrl(Key::End));
        let (rows, edit) = (app.source_rows(), source(&app));
        let line = edit.area.line_index(edit.area.caret());
        assert_eq!(line, 300);
        assert!(
            edit.scroll <= line && line < edit.scroll + rows,
            "the last line is off screen"
        );
        assert!(edit.hscroll > 0.0, "the end of a long line is off screen");
        let (w, h) = (app.width, app.height);
        let sent = app
            .render(w, h)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } if long.contains(text.as_str()) => {
                    Some(text.chars().count())
                }
                _ => None,
            })
            .max()
            .unwrap_or(0);
        assert!(
            (1..200).contains(&sent),
            "{sent} characters of a long line were sent to be drawn"
        );
        app.handle_event(&press(Key::Home));
        assert!(
            source(&app).hscroll.abs() < f32::EPSILON,
            "the start of the line is off screen"
        );
        app.handle_event(&press_ctrl(Key::Home));
        assert_eq!(source(&app).scroll, 0);
    }

    /// Tab puts in a step of the indent the text is written with.
    #[test]
    fn tab_indents_in_the_texts_own_step() {
        for (text, step) in [
            ("{\n    \"a\": 1\n}", "    "),
            ("[1]", "  "),
            ("[\n\t1\n]", "\t"),
        ] {
            let mut app = holding(text);
            app.handle_event(&press(Key::Num2));
            app.handle_event(&press(Key::Enter));
            app.handle_event(&press(Key::Tab));
            assert_eq!(app.documents[0].input, format!("{step}{text}"), "{text:?}");
        }
    }

    /// Ctrl+A, X and V: cut and paste within the program.
    #[test]
    fn cut_and_paste_within_the_text() {
        let mut app = two_tabs();
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press_ctrl(Key::A));
        app.handle_event(&press_ctrl(Key::X));
        assert_eq!(app.documents[0].input, "");
        assert_eq!(app.clipboard, r#"{"a":1,"b":"x"}"#);
        app.handle_event(&press_ctrl(Key::V));
        app.handle_event(&press_ctrl(Key::V));
        assert_eq!(app.documents[0].input, r#"{"a":1,"b":"x"}{"a":1,"b":"x"}"#);
        assert!(app.documents[0].error.is_some());
    }

    /// The text cannot grow past what opening the file again would read
    /// whole; the status line says why a key did nothing.
    #[test]
    fn the_text_stops_where_a_reopened_file_would_be_cut() {
        // The caret stays at the start of a line, so no key has to measure
        // the long one.
        let full = format!("\n{}", "a".repeat(MAX_OPEN_BYTES - 1));
        let mut app = holding(&full);
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press_ctrl(Key::Home));
        app.handle_event(&typed(' '));
        assert!(app.documents[0].input == full, "the text grew past the cap");
        assert!(
            source(&app).area.text() == full,
            "the refused key is still on screen"
        );
        assert!(
            app.note
                .as_deref()
                .is_some_and(|n| n.starts_with("Not added")),
            "{:?}",
            app.note
        );
        app.handle_event(&press(Key::Delete));
        app.handle_event(&typed(' '));
        let doc = &app.documents[0];
        assert_eq!(
            doc.input.len(),
            MAX_OPEN_BYTES,
            "the cap is not the last byte"
        );
        assert!(doc.input.starts_with(" a"));
    }

    /// Ctrl+S saves what was typed, over the file it came from.
    #[test]
    fn what_is_typed_is_what_is_saved() {
        let file = Scratch::with("source-save", "[1]");
        let mut app = opened(&file);
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press_ctrl(Key::End));
        app.handle_event(&press(Key::Left));
        app.handle_event(&typed(','));
        app.handle_event(&typed('2'));
        app.handle_event(&press_ctrl(Key::S));
        assert_eq!(file.read(), "[1,2]");
        assert!(!app.documents[app.active_tab].dirty);
        assert!(app.source_is_open(), "saving stopped the editing");
    }

    /// A notch of the wheel moves a view the distance it moves every list
    /// here -- not three pixels, as it did -- and the text by whole lines.
    #[test]
    fn a_notch_of_the_wheel_moves_a_view_its_rows() {
        let mut app = holding(&"[1,\n".repeat(100));
        app.handle_scroll(0.0, 0.0, -1.0);
        let notch = wheel::pixels(-1.0, LINE_HEIGHT);
        assert!((app.documents[0].tree_scroll - notch).abs() < 0.01);
        assert!(
            app.documents[0].tree_scroll >= LINE_HEIGHT,
            "a notch moved less than a line"
        );
        app.handle_event(&press(Key::Num2));
        app.handle_scroll(0.0, 0.0, -1.0);
        assert!((app.documents[0].raw_scroll - notch).abs() < 0.01);
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press_ctrl(Key::Home));
        assert_eq!(source(&app).scroll, 0);
        app.handle_scroll(0.0, 0.0, -1.0);
        let moved = source(&app).scroll;
        assert!(moved >= 1, "a notch did not move the text");
        app.handle_scroll(0.0, 0.0, 1.0);
        assert_eq!(source(&app).scroll, 0);
        app.handle_scroll(0.0, 0.0, -1000.0);
        assert_eq!(source(&app).scroll, 100, "scrolled past the last line");
    }

    /// A press in a raw view scrolled down edits the line under the pointer,
    /// with the text opened where the view was.
    #[test]
    fn a_press_after_scrolling_lands_on_the_line_under_it() {
        let text = format!("[\n{}  0\n]", "  0,\n".repeat(60));
        let mut app = holding(&text);
        app.handle_event(&press(Key::Num2));
        app.documents[0].raw_scroll = 10.0 * LINE_HEIGHT;
        click(
            &mut app,
            SOURCE_TEXT_X + 1.0,
            CONTENT_Y + LINE_HEIGHT * 2.0 + 5.0,
        );
        let edit = source(&app);
        assert_eq!(edit.scroll, 10, "the text did not open where the view was");
        assert_eq!(
            edit.area.line_index(edit.area.caret()),
            12,
            "not the line under the pointer"
        );
    }

    /// A value being typed over in the tree is committed before the text is
    /// edited, so the text holds it and nothing is left pending.
    #[test]
    fn a_value_being_typed_over_is_committed_before_the_text_is_edited() {
        let mut app = two_tabs();
        app.edit_mode = true;
        app.editing_path = Some(vec![PathSegment::Key(String::from("a"))]);
        app.edit_buffer = String::from("5");
        app.documents[0].view_mode = ViewMode::Raw;
        click(&mut app, SOURCE_TEXT_X + 1.0, CONTENT_Y + 5.0);
        assert!(
            app.editing_path.is_none(),
            "the value edit was left pending"
        );
        assert!(
            source(&app).area.text().contains(r#""a":5"#),
            "the text does not hold the committed value: {}",
            source(&app).area.text()
        );
    }

    /// Line and column as the parser counts them, to a byte.
    #[test]
    fn a_line_and_column_become_a_byte() {
        let text = "ab\n\u{e9}cd\nx";
        assert_eq!(byte_at(text, 1, 1), 0);
        assert_eq!(byte_at(text, 1, 3), 2);
        assert_eq!(byte_at(text, 2, 1), 3);
        assert_eq!(
            byte_at(text, 2, 2),
            5,
            "a column counts characters, not bytes"
        );
        assert_eq!(
            byte_at(text, 2, 99),
            7,
            "past the end of the line is its end"
        );
        assert_eq!(byte_at(text, 3, 1), 8);
        assert_eq!(
            byte_at(text, 9, 1),
            text.len(),
            "past the last line is the end"
        );
    }
}
