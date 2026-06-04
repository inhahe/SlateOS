//! OurOS RSS/Atom Feed Reader Application
//!
//! A full-featured feed reader providing:
//! - RSS 2.0 and Atom 1.0 XML parsing (built-in, no external crates)
//! - Feed management: add/remove/rename feeds, organize into folders
//! - Three-pane layout: sidebar (folders/feeds), article list, content view
//! - Article states: read/unread, starred/favorited with counters
//! - Refresh: per-feed and refresh-all, configurable auto-refresh interval
//! - Article list: title, date, source feed, read/unread indicator, star toggle
//! - Content view: rendered article with title, date, author, body text
//! - Search across all articles
//! - Sort by date, title, or feed name
//! - Filter: all, unread only, starred only
//! - Feed health status (last successful refresh, error tracking)
//! - OPML import/export for feed lists
//! - Keyboard shortcuts
//! - Offline reading cache
//! - Feed auto-discovery from URL
//! - Dark theme (Catppuccin Mocha) throughout
//!
//! All data is simulated locally (no network required).
//! Uses the guitk library for rendering.

#![allow(dead_code, clippy::too_many_arguments)]

use std::collections::HashMap;

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const CRUST: Color = Color::from_hex(0x11111B);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// XML Parser — minimal, built-in, no external crates
// ============================================================================

/// A parsed XML element.
#[derive(Clone, Debug, PartialEq)]
pub struct XmlElement {
    /// Tag name (e.g. "rss", "channel", "item", "feed", "entry").
    pub tag: String,
    /// Attribute map (key=value pairs from the opening tag).
    pub attributes: HashMap<String, String>,
    /// Child elements.
    pub children: Vec<XmlNode>,
}

/// An XML node — either an element or text content.
#[derive(Clone, Debug, PartialEq)]
pub enum XmlNode {
    Element(XmlElement),
    Text(String),
}

impl XmlElement {
    /// Create a new element with the given tag name.
    pub fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
        }
    }

    /// Get the text content of this element (concatenation of all direct Text children).
    pub fn text_content(&self) -> String {
        let mut result = String::new();
        for child in &self.children {
            if let XmlNode::Text(t) = child {
                result.push_str(t);
            }
        }
        result
    }

    /// Find the first child element with the given tag name.
    pub fn find_child(&self, tag: &str) -> Option<&XmlElement> {
        for child in &self.children {
            if let XmlNode::Element(elem) = child
                && elem.tag == tag {
                    return Some(elem);
                }
        }
        None
    }

    /// Find all child elements with the given tag name.
    pub fn find_children(&self, tag: &str) -> Vec<&XmlElement> {
        let mut result = Vec::new();
        for child in &self.children {
            if let XmlNode::Element(elem) = child
                && elem.tag == tag {
                    result.push(elem);
                }
        }
        result
    }

    /// Get text content of a named child element.
    pub fn child_text(&self, tag: &str) -> Option<String> {
        self.find_child(tag).map(|c| c.text_content())
    }

    /// Get an attribute value by name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(|s| s.as_str())
    }

    /// Find all descendant elements matching a tag, recursively.
    pub fn find_all(&self, tag: &str) -> Vec<&XmlElement> {
        let mut result = Vec::new();
        self.find_all_recursive(tag, &mut result);
        result
    }

    fn find_all_recursive<'a>(&'a self, tag: &str, out: &mut Vec<&'a XmlElement>) {
        for child in &self.children {
            if let XmlNode::Element(elem) = child {
                if elem.tag == tag {
                    out.push(elem);
                }
                elem.find_all_recursive(tag, out);
            }
        }
    }
}

/// XML parser state.
struct XmlParser<'a> {
    input: &'a [u8],
    pos: usize,
}

/// Errors that can occur during XML parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum XmlError {
    UnexpectedEof,
    MalformedTag(String),
    MismatchedClose { expected: String, found: String },
    InvalidEntity(String),
    InvalidAttribute(String),
}

impl core::fmt::Display for XmlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of input"),
            Self::MalformedTag(s) => write!(f, "malformed tag: {s}"),
            Self::MismatchedClose { expected, found } => {
                write!(f, "mismatched close tag: expected </{expected}>, found </{found}>")
            }
            Self::InvalidEntity(s) => write!(f, "invalid entity: {s}"),
            Self::InvalidAttribute(s) => write!(f, "invalid attribute: {s}"),
        }
    }
}

impl<'a> XmlParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    /// Peek at the current byte without consuming it.
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Advance position by one byte and return it.
    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    /// Skip whitespace characters.
    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Check if we're at the end of input.
    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Read bytes until a specific byte is encountered (not consumed).
    fn read_until(&mut self, stop: u8) -> String {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b == stop {
                break;
            }
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    /// Read a name (tag name or attribute name): [a-zA-Z0-9_:.-]+
    fn read_name(&mut self) -> String {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b':' || b == b'.' || b == b'-' {
                self.pos += 1;
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    /// Decode an XML entity reference.
    fn decode_entity(entity: &str) -> Result<char, XmlError> {
        match entity {
            "amp" => Ok('&'),
            "lt" => Ok('<'),
            "gt" => Ok('>'),
            "quot" => Ok('"'),
            "apos" => Ok('\''),
            s if s.starts_with('#') => {
                let numeric = &s[1..];
                let codepoint = if let Some(hex) = numeric.strip_prefix('x') {
                    u32::from_str_radix(hex, 16)
                        .map_err(|_| XmlError::InvalidEntity(entity.to_string()))?
                } else {
                    numeric
                        .parse::<u32>()
                        .map_err(|_| XmlError::InvalidEntity(entity.to_string()))?
                };
                char::from_u32(codepoint)
                    .ok_or_else(|| XmlError::InvalidEntity(entity.to_string()))
            }
            _ => Err(XmlError::InvalidEntity(entity.to_string())),
        }
    }

    /// Decode XML entities in a string.
    fn decode_entities(text: &str) -> Result<String, XmlError> {
        let mut result = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            if ch == '&' {
                let mut entity = String::new();
                for ec in chars.by_ref() {
                    if ec == ';' {
                        break;
                    }
                    entity.push(ec);
                }
                let decoded = Self::decode_entity(&entity)?;
                result.push(decoded);
            } else {
                result.push(ch);
            }
        }
        Ok(result)
    }

    /// Read a quoted attribute value.
    fn read_attribute_value(&mut self) -> Result<String, XmlError> {
        self.skip_whitespace();
        let quote = self.advance().ok_or(XmlError::UnexpectedEof)?;
        if quote != b'"' && quote != b'\'' {
            return Err(XmlError::InvalidAttribute(
                "expected quote for attribute value".to_string(),
            ));
        }
        let mut value = String::new();
        while let Some(b) = self.advance() {
            if b == quote {
                return Self::decode_entities(&value);
            }
            value.push(b as char);
        }
        Err(XmlError::UnexpectedEof)
    }

    /// Skip the XML declaration (<?xml ... ?>).
    fn skip_xml_declaration(&mut self) {
        if self.pos + 1 < self.input.len()
            && self.input[self.pos] == b'<'
            && self.input[self.pos + 1] == b'?'
        {
            while self.pos + 1 < self.input.len() {
                if self.input[self.pos] == b'?' && self.input[self.pos + 1] == b'>' {
                    self.pos += 2;
                    return;
                }
                self.pos += 1;
            }
        }
    }

    /// Skip a comment (<!-- ... -->).
    fn skip_comment(&mut self) -> bool {
        if self.pos + 3 < self.input.len()
            && self.input[self.pos] == b'<'
            && self.input[self.pos + 1] == b'!'
            && self.input[self.pos + 2] == b'-'
            && self.input[self.pos + 3] == b'-'
        {
            self.pos += 4;
            while self.pos + 2 < self.input.len() {
                if self.input[self.pos] == b'-'
                    && self.input[self.pos + 1] == b'-'
                    && self.input[self.pos + 2] == b'>'
                {
                    self.pos += 3;
                    return true;
                }
                self.pos += 1;
            }
            // Unterminated comment: skip to end
            self.pos = self.input.len();
            return true;
        }
        false
    }

    /// Skip a CDATA section and return its content.
    fn try_read_cdata(&mut self) -> Option<String> {
        if self.pos + 8 < self.input.len()
            && &self.input[self.pos..self.pos + 9] == b"<![CDATA["
        {
            self.pos += 9;
            let start = self.pos;
            while self.pos + 2 < self.input.len() {
                if self.input[self.pos] == b']'
                    && self.input[self.pos + 1] == b']'
                    && self.input[self.pos + 2] == b'>'
                {
                    let content =
                        String::from_utf8_lossy(&self.input[start..self.pos]).to_string();
                    self.pos += 3;
                    return Some(content);
                }
                self.pos += 1;
            }
            let content =
                String::from_utf8_lossy(&self.input[start..self.input.len()]).to_string();
            self.pos = self.input.len();
            return Some(content);
        }
        None
    }

    /// Skip a DOCTYPE declaration.
    fn skip_doctype(&mut self) -> bool {
        if self.pos + 8 < self.input.len() {
            let slice = &self.input[self.pos..self.pos + 9];
            if slice.eq_ignore_ascii_case(b"<!DOCTYPE") || slice.eq_ignore_ascii_case(b"<!doctype")
            {
                let mut depth: u32 = 1;
                self.pos += 9;
                while let Some(b) = self.advance() {
                    if b == b'<' {
                        depth = depth.saturating_add(1);
                    } else if b == b'>' {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return true;
                        }
                    }
                }
                return true;
            }
        }
        false
    }

    /// Parse a single element (and its children recursively).
    fn parse_element(&mut self) -> Result<XmlElement, XmlError> {
        self.skip_whitespace();

        // Expect '<'
        if self.advance() != Some(b'<') {
            return Err(XmlError::MalformedTag("expected '<'".to_string()));
        }

        // Read tag name
        let tag = self.read_name();
        if tag.is_empty() {
            return Err(XmlError::MalformedTag("empty tag name".to_string()));
        }

        let mut elem = XmlElement::new(&tag);

        // Parse attributes
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'>') => {
                    self.pos += 1;
                    break;
                }
                Some(b'/') => {
                    self.pos += 1;
                    if self.advance() != Some(b'>') {
                        return Err(XmlError::MalformedTag(
                            "expected '>' after '/'".to_string(),
                        ));
                    }
                    // Self-closing tag
                    return Ok(elem);
                }
                Some(_) => {
                    let attr_name = self.read_name();
                    if attr_name.is_empty() {
                        // Skip unknown byte
                        self.pos += 1;
                        continue;
                    }
                    self.skip_whitespace();
                    if self.peek() == Some(b'=') {
                        self.pos += 1; // skip '='
                        let value = self.read_attribute_value()?;
                        elem.attributes.insert(attr_name, value);
                    } else {
                        // Boolean attribute (no value)
                        elem.attributes.insert(attr_name, String::new());
                    }
                }
                None => return Err(XmlError::UnexpectedEof),
            }
        }

        // Parse children until closing tag
        loop {
            // Check for CDATA
            if let Some(cdata) = self.try_read_cdata() {
                if !cdata.trim().is_empty() {
                    elem.children.push(XmlNode::Text(cdata));
                }
                continue;
            }

            // Check for comment
            if self.skip_comment() {
                continue;
            }

            // Check for closing tag
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'<'
                && self.input[self.pos + 1] == b'/'
            {
                self.pos += 2;
                let close_tag = self.read_name();
                // Skip to '>'
                while let Some(b) = self.advance() {
                    if b == b'>' {
                        break;
                    }
                }
                if close_tag != tag {
                    return Err(XmlError::MismatchedClose {
                        expected: tag,
                        found: close_tag,
                    });
                }
                return Ok(elem);
            }

            // Check for child element
            if self.peek() == Some(b'<') {
                // Could be a child element, a processing instruction, or a comment
                if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'?' {
                    self.skip_xml_declaration();
                    continue;
                }
                if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'!' {
                    if self.skip_comment() {
                        continue;
                    }
                    if self.skip_doctype() {
                        continue;
                    }
                    if let Some(cdata) = self.try_read_cdata() {
                        if !cdata.trim().is_empty() {
                            elem.children.push(XmlNode::Text(cdata));
                        }
                        continue;
                    }
                    // Unknown <! construct, skip it
                    self.pos += 1;
                    continue;
                }
                let child = self.parse_element()?;
                elem.children.push(XmlNode::Element(child));
                continue;
            }

            if self.at_end() {
                // Implicit close at EOF
                return Ok(elem);
            }

            // Text content
            let text_start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'<' {
                    break;
                }
                self.pos += 1;
            }
            let raw = String::from_utf8_lossy(&self.input[text_start..self.pos]).to_string();
            if let Ok(decoded) = Self::decode_entities(&raw) {
                if !decoded.trim().is_empty() {
                    elem.children.push(XmlNode::Text(decoded));
                }
            } else if !raw.trim().is_empty() {
                elem.children.push(XmlNode::Text(raw));
            }
        }
    }

    /// Parse the full document, returning the root element.
    fn parse_document(&mut self) -> Result<XmlElement, XmlError> {
        self.skip_whitespace();
        self.skip_xml_declaration();
        self.skip_whitespace();
        while self.skip_comment() || self.skip_doctype() {
            self.skip_whitespace();
        }
        self.skip_whitespace();
        if self.at_end() {
            return Err(XmlError::UnexpectedEof);
        }
        self.parse_element()
    }
}

/// Parse an XML string into a root element.
pub fn parse_xml(input: &str) -> Result<XmlElement, XmlError> {
    let mut parser = XmlParser::new(input);
    parser.parse_document()
}

// ============================================================================
// Feed data model
// ============================================================================

/// Unique identifier for feeds.
pub type FeedId = u64;
/// Unique identifier for articles.
pub type ArticleId = u64;
/// Unique identifier for folders.
pub type FolderId = u64;

/// Feed format type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedFormat {
    Rss2,
    Atom1,
    Unknown,
}

impl FeedFormat {
    /// Detect feed format from the root element tag.
    pub fn detect(root_tag: &str) -> Self {
        match root_tag {
            "rss" => Self::Rss2,
            "feed" => Self::Atom1,
            _ => Self::Unknown,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rss2 => "RSS 2.0",
            Self::Atom1 => "Atom 1.0",
            Self::Unknown => "Unknown",
        }
    }
}

/// Health status of a feed, tracking refresh success/failure.
#[derive(Clone, Debug)]
pub struct FeedHealth {
    /// Timestamp of the last successful refresh (seconds since epoch).
    pub last_success: Option<u64>,
    /// Timestamp of the last attempted refresh.
    pub last_attempt: Option<u64>,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
    /// Last error message, if any.
    pub last_error: Option<String>,
}

impl Default for FeedHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedHealth {
    pub fn new() -> Self {
        Self {
            last_success: None,
            last_attempt: None,
            consecutive_failures: 0,
            last_error: None,
        }
    }

    /// Record a successful refresh.
    pub fn record_success(&mut self, timestamp: u64) {
        self.last_success = Some(timestamp);
        self.last_attempt = Some(timestamp);
        self.consecutive_failures = 0;
        self.last_error = None;
    }

    /// Record a failed refresh.
    pub fn record_failure(&mut self, timestamp: u64, error: &str) {
        self.last_attempt = Some(timestamp);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_error = Some(error.to_string());
    }

    /// Whether this feed is considered healthy.
    pub fn is_healthy(&self) -> bool {
        self.consecutive_failures == 0
    }

    /// Status text for display.
    pub fn status_text(&self) -> String {
        if self.is_healthy() {
            if let Some(ts) = self.last_success {
                format!("OK (last: {})", format_timestamp(ts))
            } else {
                "Never refreshed".to_string()
            }
        } else {
            let err = self.last_error.as_deref().unwrap_or("unknown error");
            format!("Error ({} failures): {}", self.consecutive_failures, err)
        }
    }
}

/// A feed subscription.
#[derive(Clone, Debug)]
pub struct Feed {
    pub id: FeedId,
    pub title: String,
    pub url: String,
    pub description: String,
    pub folder_id: Option<FolderId>,
    pub format: FeedFormat,
    pub health: FeedHealth,
    /// Auto-refresh interval in seconds (0 = disabled).
    pub auto_refresh_seconds: u64,
    /// Website link associated with the feed.
    pub link: String,
}

impl Feed {
    pub fn new(id: FeedId, title: &str, url: &str) -> Self {
        Self {
            id,
            title: title.to_string(),
            url: url.to_string(),
            description: String::new(),
            folder_id: None,
            format: FeedFormat::Unknown,
            health: FeedHealth::new(),
            auto_refresh_seconds: 3600,
            link: String::new(),
        }
    }
}

/// An article from a feed.
#[derive(Clone, Debug)]
pub struct Article {
    pub id: ArticleId,
    pub feed_id: FeedId,
    pub title: String,
    pub link: String,
    pub author: String,
    pub published: u64,
    pub summary: String,
    pub content: String,
    pub is_read: bool,
    pub is_starred: bool,
    /// Cached offline content (HTML stripped to plain text for rendering).
    pub cached_text: String,
}

impl Article {
    pub fn new(id: ArticleId, feed_id: FeedId, title: &str) -> Self {
        Self {
            id,
            feed_id,
            title: title.to_string(),
            link: String::new(),
            author: String::new(),
            published: 0,
            summary: String::new(),
            content: String::new(),
            is_read: false,
            is_starred: false,
            cached_text: String::new(),
        }
    }

    /// Get display-ready text for the content pane.
    pub fn display_content(&self) -> &str {
        if !self.cached_text.is_empty() {
            &self.cached_text
        } else if !self.content.is_empty() {
            &self.content
        } else {
            &self.summary
        }
    }
}

/// A folder/category for organizing feeds.
#[derive(Clone, Debug)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    pub is_expanded: bool,
}

impl Folder {
    pub fn new(id: FolderId, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            is_expanded: true,
        }
    }
}

// ============================================================================
// Date/time formatting helpers
// ============================================================================

/// Format a Unix timestamp into a human-readable date string.
pub fn format_timestamp(ts: u64) -> String {
    // Simple epoch-based formatting (no chrono dependency)
    let secs_per_minute: u64 = 60;
    let secs_per_hour: u64 = 3600;
    let secs_per_day: u64 = 86400;

    let days = ts / secs_per_day;
    let remaining = ts % secs_per_day;
    let hours = remaining / secs_per_hour;
    let remaining = remaining % secs_per_hour;
    let minutes = remaining / secs_per_minute;

    // Simple year/month/day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}"
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Adjusted algorithm based on civil calendar
    let mut year: u64 = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month: u64 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    (year, month, days + 1)
}

/// Check if a year is a leap year.
fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Parse a simple date string into a Unix timestamp.
/// Supports formats like "2024-01-15", "2024-01-15T10:30:00", RFC-822 dates.
pub fn parse_date_string(date_str: &str) -> Option<u64> {
    let trimmed = date_str.trim();

    // Try ISO 8601: "2024-01-15T10:30:00Z" or "2024-01-15T10:30:00+00:00"
    if let Some(result) = try_parse_iso8601(trimmed) {
        return Some(result);
    }

    // Try RFC 822: "Mon, 15 Jan 2024 10:30:00 GMT"
    if let Some(result) = try_parse_rfc822(trimmed) {
        return Some(result);
    }

    // Try simple date: "2024-01-15"
    if let Some(result) = try_parse_simple_date(trimmed) {
        return Some(result);
    }

    None
}

/// Try to parse an ISO 8601 date string.
fn try_parse_iso8601(s: &str) -> Option<u64> {
    // Strip timezone suffix
    let base = s
        .trim_end_matches('Z')
        .split('+')
        .next()?;
    let base = base.split('-').collect::<Vec<_>>();
    // Need at least year-month-day
    if base.len() < 3 {
        return None;
    }

    // The third part might contain "dayTtime"
    let year: u64 = base.first()?.parse().ok()?;
    let month: u64 = base.get(1)?.parse().ok()?;

    let day_part = *base.get(2)?;
    let (day_str, time_str) = if let Some(t_pos) = day_part.find('T') {
        (&day_part[..t_pos], Some(&day_part[t_pos + 1..]))
    } else {
        (day_part, None)
    };

    let day: u64 = day_str.parse().ok()?;
    let (hours, minutes, seconds) = if let Some(time) = time_str {
        parse_time_components(time)
    } else {
        (0, 0, 0)
    };

    Some(ymd_hms_to_epoch(year, month, day, hours, minutes, seconds))
}

/// Parse time components from "HH:MM:SS" or "HH:MM" string.
fn parse_time_components(time: &str) -> (u64, u64, u64) {
    let parts: Vec<&str> = time.split(':').collect();
    let hours: u64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minutes: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let seconds: u64 = parts.get(2).and_then(|s| {
        // Handle fractional seconds like "30.123"
        s.split('.').next().and_then(|w| w.parse().ok())
    }).unwrap_or(0);
    (hours, minutes, seconds)
}

/// Try to parse an RFC 822 date string.
fn try_parse_rfc822(s: &str) -> Option<u64> {
    // "Mon, 15 Jan 2024 10:30:00 GMT"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    // Skip day name if present (e.g. "Mon,")
    let offset = if parts.first()?.ends_with(',') { 1 } else { 0 };

    let day: u64 = parts.get(offset)?.parse().ok()?;
    let month = month_name_to_number(parts.get(offset + 1)?)?;
    let year: u64 = parts.get(offset + 2)?.parse().ok()?;

    let (hours, minutes, seconds) = if let Some(time_str) = parts.get(offset + 3) {
        parse_time_components(time_str)
    } else {
        (0, 0, 0)
    };

    Some(ymd_hms_to_epoch(year, month, day, hours, minutes, seconds))
}

/// Convert month name abbreviation to number (1-12).
fn month_name_to_number(name: &str) -> Option<u64> {
    match name.to_ascii_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

/// Try to parse a simple "YYYY-MM-DD" date.
fn try_parse_simple_date(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: u64 = parts.first()?.parse().ok()?;
    let month: u64 = parts.get(1)?.parse().ok()?;
    let day: u64 = parts.get(2)?.parse().ok()?;
    Some(ymd_hms_to_epoch(year, month, day, 0, 0, 0))
}

/// Convert year/month/day/hour/minute/second to Unix epoch seconds.
fn ymd_hms_to_epoch(year: u64, month: u64, day: u64, h: u64, m: u64, s: u64) -> u64 {
    let mut total_days: u64 = 0;

    // Days from years
    for y in 1970..year {
        total_days += if is_leap_year(y) { 366 } else { 365 };
    }

    // Days from months in the current year
    let month_days: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    for i in 0..(month.saturating_sub(1) as usize) {
        if let Some(&md) = month_days.get(i) {
            total_days += md;
        }
    }

    total_days += day.saturating_sub(1);

    total_days * 86400 + h * 3600 + m * 60 + s
}

// ============================================================================
// HTML to plain text converter (for article content)
// ============================================================================

/// Strip HTML tags and convert common entities to produce plain text.
pub fn html_to_text(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity_buf = String::new();
    let mut last_was_space = false;

    for ch in html.chars() {
        if in_entity {
            if ch == ';' {
                let decoded = match entity_buf.as_str() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => ' ',
                    "mdash" | "#8212" => '\u{2014}',
                    "ndash" | "#8211" => '\u{2013}',
                    "hellip" | "#8230" => '\u{2026}',
                    "laquo" | "#171" => '\u{00AB}',
                    "raquo" | "#187" => '\u{00BB}',
                    _ => '?',
                };
                result.push(decoded);
                last_was_space = false;
                in_entity = false;
                entity_buf.clear();
            } else {
                entity_buf.push(ch);
                if entity_buf.len() > 10 {
                    // Malformed entity, dump as-is
                    result.push('&');
                    result.push_str(&entity_buf);
                    in_entity = false;
                    entity_buf.clear();
                }
            }
            continue;
        }

        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            continue;
        }

        match ch {
            '<' => {
                in_tag = true;
            }
            '&' => {
                in_entity = true;
                entity_buf.clear();
            }
            '\n' | '\r' => {
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            _ => {
                if ch == ' ' || ch == '\t' {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(ch);
                    last_was_space = false;
                }
            }
        }
    }

    result.trim().to_string()
}

// ============================================================================
// RSS 2.0 Parser
// ============================================================================

/// Parsed data from an RSS 2.0 feed.
#[derive(Clone, Debug)]
pub struct ParsedFeed {
    pub title: String,
    pub link: String,
    pub description: String,
    pub format: FeedFormat,
    pub articles: Vec<ParsedArticle>,
}

/// A single parsed article/entry from a feed.
#[derive(Clone, Debug)]
pub struct ParsedArticle {
    pub title: String,
    pub link: String,
    pub author: String,
    pub published: u64,
    pub summary: String,
    pub content: String,
}

/// Parse an RSS 2.0 XML document into a `ParsedFeed`.
pub fn parse_rss2(root: &XmlElement) -> Option<ParsedFeed> {
    let channel = root.find_child("channel")?;
    let title = channel.child_text("title").unwrap_or_default();
    let link = channel.child_text("link").unwrap_or_default();
    let description = channel.child_text("description").unwrap_or_default();

    let mut articles = Vec::new();
    for item in channel.find_children("item") {
        let item_title = item.child_text("title").unwrap_or_default();
        let item_link = item.child_text("link").unwrap_or_default();
        let item_author = item
            .child_text("author")
            .or_else(|| item.child_text("dc:creator"))
            .unwrap_or_default();
        let item_pub_date = item.child_text("pubDate").unwrap_or_default();
        let item_description = item.child_text("description").unwrap_or_default();
        let item_content = item
            .child_text("content:encoded")
            .unwrap_or_default();

        let published = parse_date_string(&item_pub_date).unwrap_or(0);

        articles.push(ParsedArticle {
            title: item_title,
            link: item_link,
            author: item_author,
            published,
            summary: html_to_text(&item_description),
            content: html_to_text(&item_content),
        });
    }

    Some(ParsedFeed {
        title,
        link,
        description,
        format: FeedFormat::Rss2,
        articles,
    })
}

/// Parse an Atom 1.0 XML document into a `ParsedFeed`.
pub fn parse_atom1(root: &XmlElement) -> Option<ParsedFeed> {
    let title = root.child_text("title").unwrap_or_default();

    // Atom uses <link> elements with attributes
    let link = root
        .find_children("link")
        .iter()
        .find(|l| {
            l.attr("rel").unwrap_or("alternate") == "alternate"
        })
        .and_then(|l| l.attr("href"))
        .unwrap_or("")
        .to_string();

    let description = root.child_text("subtitle").unwrap_or_default();

    let mut articles = Vec::new();
    for entry in root.find_children("entry") {
        let entry_title = entry.child_text("title").unwrap_or_default();
        let entry_link = entry
            .find_children("link")
            .iter()
            .find(|l| {
                l.attr("rel").unwrap_or("alternate") == "alternate"
            })
            .and_then(|l| l.attr("href"))
            .unwrap_or("")
            .to_string();
        let entry_author = entry
            .find_child("author")
            .and_then(|a| a.child_text("name"))
            .unwrap_or_default();
        let entry_updated = entry
            .child_text("updated")
            .or_else(|| entry.child_text("published"))
            .unwrap_or_default();
        let entry_summary = entry.child_text("summary").unwrap_or_default();
        let entry_content = entry.child_text("content").unwrap_or_default();

        let published = parse_date_string(&entry_updated).unwrap_or(0);

        articles.push(ParsedArticle {
            title: entry_title,
            link: entry_link,
            author: entry_author,
            published,
            summary: html_to_text(&entry_summary),
            content: html_to_text(&entry_content),
        });
    }

    Some(ParsedFeed {
        title,
        link,
        description,
        format: FeedFormat::Atom1,
        articles,
    })
}

/// Auto-detect feed format and parse accordingly.
pub fn parse_feed(xml_str: &str) -> Result<ParsedFeed, String> {
    let root = parse_xml(xml_str).map_err(|e| format!("XML parse error: {e}"))?;
    let format = FeedFormat::detect(&root.tag);
    match format {
        FeedFormat::Rss2 => parse_rss2(&root).ok_or_else(|| "failed to parse RSS 2.0".to_string()),
        FeedFormat::Atom1 => {
            parse_atom1(&root).ok_or_else(|| "failed to parse Atom 1.0".to_string())
        }
        FeedFormat::Unknown => Err(format!("unknown feed format: root tag '{}'", root.tag)),
    }
}

// ============================================================================
// OPML Import/Export
// ============================================================================

/// An OPML outline entry (represents one feed or folder).
#[derive(Clone, Debug)]
pub struct OpmlOutline {
    pub text: String,
    pub xml_url: Option<String>,
    pub html_url: Option<String>,
    pub feed_type: Option<String>,
    pub children: Vec<OpmlOutline>,
}

/// Parse an OPML document into a list of outlines.
pub fn parse_opml(xml_str: &str) -> Result<Vec<OpmlOutline>, String> {
    let root = parse_xml(xml_str).map_err(|e| format!("OPML parse error: {e}"))?;

    if root.tag != "opml" {
        return Err(format!("expected <opml> root, found <{}>", root.tag));
    }

    let body = root
        .find_child("body")
        .ok_or_else(|| "missing <body> element".to_string())?;

    let mut outlines = Vec::new();
    for outline_elem in body.find_children("outline") {
        outlines.push(parse_opml_outline(outline_elem));
    }

    Ok(outlines)
}

/// Parse a single OPML outline element (recursive for folders).
fn parse_opml_outline(elem: &XmlElement) -> OpmlOutline {
    let text = elem.attr("text").unwrap_or("").to_string();
    let xml_url = elem.attr("xmlUrl").map(|s| s.to_string());
    let html_url = elem.attr("htmlUrl").map(|s| s.to_string());
    let feed_type = elem.attr("type").map(|s| s.to_string());

    let children: Vec<OpmlOutline> = elem
        .find_children("outline")
        .iter()
        .map(|child| parse_opml_outline(child))
        .collect();

    OpmlOutline {
        text,
        xml_url,
        html_url,
        feed_type,
        children,
    }
}

/// Generate OPML XML from a list of feeds and folders.
pub fn generate_opml(title: &str, feeds: &[Feed], folders: &[Folder]) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<opml version=\"2.0\">\n");
    xml.push_str("  <head>\n");
    xml.push_str(&format!("    <title>{}</title>\n", escape_xml(title)));
    xml.push_str("  </head>\n");
    xml.push_str("  <body>\n");

    // Feeds in folders
    for folder in folders {
        xml.push_str(&format!(
            "    <outline text=\"{}\">\n",
            escape_xml(&folder.name)
        ));
        for feed in feeds {
            if feed.folder_id == Some(folder.id) {
                xml.push_str(&format!(
                    "      <outline text=\"{}\" type=\"rss\" xmlUrl=\"{}\" htmlUrl=\"{}\"/>\n",
                    escape_xml(&feed.title),
                    escape_xml(&feed.url),
                    escape_xml(&feed.link),
                ));
            }
        }
        xml.push_str("    </outline>\n");
    }

    // Feeds without a folder
    for feed in feeds {
        if feed.folder_id.is_none() {
            xml.push_str(&format!(
                "    <outline text=\"{}\" type=\"rss\" xmlUrl=\"{}\" htmlUrl=\"{}\"/>\n",
                escape_xml(&feed.title),
                escape_xml(&feed.url),
                escape_xml(&feed.link),
            ));
        }
    }

    xml.push_str("  </body>\n");
    xml.push_str("</opml>\n");
    xml
}

/// Escape special XML characters in a string.
pub fn escape_xml(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            _ => result.push(ch),
        }
    }
    result
}

// ============================================================================
// Feed auto-discovery
// ============================================================================

/// Discovered feed link from an HTML page.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscoveredFeed {
    pub url: String,
    pub title: String,
    pub feed_type: String,
}

/// Scan HTML content for <link> tags that reference RSS/Atom feeds.
pub fn discover_feeds(html: &str) -> Vec<DiscoveredFeed> {
    let mut results = Vec::new();
    let lower = html.to_lowercase();

    // Look for <link rel="alternate" type="application/rss+xml" ...>
    // and <link rel="alternate" type="application/atom+xml" ...>
    let feed_types = [
        ("application/rss+xml", "RSS"),
        ("application/atom+xml", "Atom"),
    ];

    // Simple regex-free scanner for <link> tags
    let mut pos = 0;
    while let Some(link_start) = lower[pos..].find("<link") {
        let abs_start = pos + link_start;
        let tag_end = match lower[abs_start..].find('>') {
            Some(end) => abs_start + end + 1,
            None => break,
        };
        let tag_content = &html[abs_start..tag_end];

        // Check if it's a feed link
        for &(mime_type, label) in &feed_types {
            if tag_content.to_lowercase().contains(mime_type)
                && let Some(href) = extract_attribute(tag_content, "href") {
                    let title = extract_attribute(tag_content, "title")
                        .unwrap_or_else(|| label.to_string());
                    results.push(DiscoveredFeed {
                        url: href,
                        title,
                        feed_type: label.to_string(),
                    });
                }
        }

        pos = tag_end;
    }

    results
}

/// Extract the value of an HTML attribute from a tag string.
fn extract_attribute(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let search = format!("{attr_name}=");
    let attr_pos = lower.find(&search)?;
    let value_start = attr_pos + search.len();
    let rest = &tag[value_start..];

    let rest_trimmed = rest.trim_start();
    if let Some(inner) = rest_trimmed.strip_prefix('"') {
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else if let Some(inner) = rest_trimmed.strip_prefix('\'') {
        let end = inner.find('\'')?;
        Some(inner[..end].to_string())
    } else {
        // Unquoted value (until whitespace or >)
        let end = rest_trimmed
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest_trimmed.len());
        Some(rest_trimmed[..end].to_string())
    }
}

// ============================================================================
// Sort and Filter types
// ============================================================================

/// Sort order for the article list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    DateDesc,
    DateAsc,
    TitleAsc,
    TitleDesc,
    FeedNameAsc,
    FeedNameDesc,
}

impl SortOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::DateDesc => "Date (newest first)",
            Self::DateAsc => "Date (oldest first)",
            Self::TitleAsc => "Title (A-Z)",
            Self::TitleDesc => "Title (Z-A)",
            Self::FeedNameAsc => "Feed (A-Z)",
            Self::FeedNameDesc => "Feed (Z-A)",
        }
    }

    /// Cycle to the next sort order.
    pub fn next(self) -> Self {
        match self {
            Self::DateDesc => Self::DateAsc,
            Self::DateAsc => Self::TitleAsc,
            Self::TitleAsc => Self::TitleDesc,
            Self::TitleDesc => Self::FeedNameAsc,
            Self::FeedNameAsc => Self::FeedNameDesc,
            Self::FeedNameDesc => Self::DateDesc,
        }
    }
}

/// Filter mode for the article list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    All,
    Unread,
    Starred,
}

impl FilterMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All Articles",
            Self::Unread => "Unread Only",
            Self::Starred => "Starred Only",
        }
    }

    /// Cycle to the next filter mode.
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Unread,
            Self::Unread => Self::Starred,
            Self::Starred => Self::All,
        }
    }
}

// ============================================================================
// Keyboard shortcuts
// ============================================================================

/// Keyboard shortcut action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    NextArticle,
    PrevArticle,
    NextFeed,
    PrevFeed,
    ToggleRead,
    ToggleStar,
    RefreshCurrent,
    RefreshAll,
    Search,
    CycleSortOrder,
    CycleFilter,
    ToggleSidebar,
    MarkAllRead,
    OpenInBrowser,
    AddFeed,
    RemoveFeed,
    RenameFeed,
    NewFolder,
    ToggleFolderExpand,
    ShowHelp,
    Quit,
}

impl KeyAction {
    /// Human-readable description of the action.
    pub fn description(self) -> &'static str {
        match self {
            Self::NextArticle => "Next article",
            Self::PrevArticle => "Previous article",
            Self::NextFeed => "Next feed",
            Self::PrevFeed => "Previous feed",
            Self::ToggleRead => "Toggle read/unread",
            Self::ToggleStar => "Toggle star",
            Self::RefreshCurrent => "Refresh current feed",
            Self::RefreshAll => "Refresh all feeds",
            Self::Search => "Search articles",
            Self::CycleSortOrder => "Cycle sort order",
            Self::CycleFilter => "Cycle filter mode",
            Self::ToggleSidebar => "Toggle sidebar",
            Self::MarkAllRead => "Mark all as read",
            Self::OpenInBrowser => "Open in browser",
            Self::AddFeed => "Add new feed",
            Self::RemoveFeed => "Remove feed",
            Self::RenameFeed => "Rename feed",
            Self::NewFolder => "New folder",
            Self::ToggleFolderExpand => "Toggle folder expand",
            Self::ShowHelp => "Show keyboard shortcuts",
            Self::Quit => "Quit",
        }
    }

    /// Keybinding display string.
    pub fn key_hint(self) -> &'static str {
        match self {
            Self::NextArticle => "J / Down",
            Self::PrevArticle => "K / Up",
            Self::NextFeed => "Shift+J",
            Self::PrevFeed => "Shift+K",
            Self::ToggleRead => "R",
            Self::ToggleStar => "S",
            Self::RefreshCurrent => "F5",
            Self::RefreshAll => "Shift+F5",
            Self::Search => "Ctrl+F / /",
            Self::CycleSortOrder => "O",
            Self::CycleFilter => "F",
            Self::ToggleSidebar => "B",
            Self::MarkAllRead => "Shift+R",
            Self::OpenInBrowser => "Enter / O",
            Self::AddFeed => "A",
            Self::RemoveFeed => "D",
            Self::RenameFeed => "Ctrl+R",
            Self::NewFolder => "Ctrl+N",
            Self::ToggleFolderExpand => "Space",
            Self::ShowHelp => "?",
            Self::Quit => "Q",
        }
    }
}

/// All available keyboard shortcuts.
pub const ALL_KEY_ACTIONS: &[KeyAction] = &[
    KeyAction::NextArticle,
    KeyAction::PrevArticle,
    KeyAction::NextFeed,
    KeyAction::PrevFeed,
    KeyAction::ToggleRead,
    KeyAction::ToggleStar,
    KeyAction::RefreshCurrent,
    KeyAction::RefreshAll,
    KeyAction::Search,
    KeyAction::CycleSortOrder,
    KeyAction::CycleFilter,
    KeyAction::ToggleSidebar,
    KeyAction::MarkAllRead,
    KeyAction::OpenInBrowser,
    KeyAction::AddFeed,
    KeyAction::RemoveFeed,
    KeyAction::RenameFeed,
    KeyAction::NewFolder,
    KeyAction::ToggleFolderExpand,
    KeyAction::ShowHelp,
    KeyAction::Quit,
];

// ============================================================================
// Active pane tracking
// ============================================================================

/// Which pane currently has keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivePane {
    Sidebar,
    ArticleList,
    ContentView,
}

impl ActivePane {
    /// Cycle to the next pane (left to right, wrapping).
    pub fn next(self) -> Self {
        match self {
            Self::Sidebar => Self::ArticleList,
            Self::ArticleList => Self::ContentView,
            Self::ContentView => Self::Sidebar,
        }
    }

    /// Cycle to the previous pane.
    pub fn prev(self) -> Self {
        match self {
            Self::Sidebar => Self::ContentView,
            Self::ArticleList => Self::Sidebar,
            Self::ContentView => Self::ArticleList,
        }
    }
}

// ============================================================================
// Sidebar selection model
// ============================================================================

/// What is selected in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSelection {
    /// "All Feeds" virtual entry.
    AllFeeds,
    /// A specific folder.
    Folder(FolderId),
    /// A specific feed.
    Feed(FeedId),
    /// "Starred" virtual entry.
    Starred,
}

// ============================================================================
// Offline reading cache
// ============================================================================

/// Offline cache entry for an article.
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub article_id: ArticleId,
    pub cached_at: u64,
    pub text_content: String,
    pub size_bytes: usize,
}

/// Offline reading cache manager.
#[derive(Clone, Debug)]
pub struct OfflineCache {
    pub entries: HashMap<ArticleId, CacheEntry>,
    /// Maximum cache size in bytes.
    pub max_size: usize,
    /// Current total size.
    pub current_size: usize,
}

impl OfflineCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_size,
            current_size: 0,
        }
    }

    /// Cache an article's content for offline reading.
    pub fn cache_article(&mut self, article_id: ArticleId, content: &str, timestamp: u64) {
        let size = content.len();

        // Evict old entries if we'd exceed the limit
        while self.current_size + size > self.max_size && !self.entries.is_empty() {
            self.evict_oldest();
        }

        if size > self.max_size {
            // Single article exceeds cache — skip it
            return;
        }

        // Remove existing entry if present
        if let Some(old) = self.entries.remove(&article_id) {
            self.current_size = self.current_size.saturating_sub(old.size_bytes);
        }

        let entry = CacheEntry {
            article_id,
            cached_at: timestamp,
            text_content: content.to_string(),
            size_bytes: size,
        };

        self.current_size += size;
        self.entries.insert(article_id, entry);
    }

    /// Get cached content for an article.
    pub fn get_cached(&self, article_id: ArticleId) -> Option<&str> {
        self.entries.get(&article_id).map(|e| e.text_content.as_str())
    }

    /// Remove the oldest cached entry.
    fn evict_oldest(&mut self) {
        let oldest_id = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.cached_at)
            .map(|(id, _)| *id);

        if let Some(id) = oldest_id
            && let Some(entry) = self.entries.remove(&id) {
                self.current_size = self.current_size.saturating_sub(entry.size_bytes);
            }
    }

    /// Check if an article is cached.
    pub fn is_cached(&self, article_id: ArticleId) -> bool {
        self.entries.contains_key(&article_id)
    }

    /// Clear the entire cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_size = 0;
    }

    /// Number of cached articles.
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

// ============================================================================
// Search engine
// ============================================================================

/// Search result with match context.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub article_id: ArticleId,
    pub feed_id: FeedId,
    pub title_match: bool,
    pub content_match: bool,
    pub author_match: bool,
    /// Snippet of content around the match.
    pub snippet: String,
}

/// Search across all articles for a query string.
pub fn search_articles(articles: &[Article], query: &str) -> Vec<SearchResult> {
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for article in articles {
        let title_lower = article.title.to_lowercase();
        let content_lower = article.display_content().to_lowercase();
        let author_lower = article.author.to_lowercase();

        let title_match = title_lower.contains(&query_lower);
        let content_match = content_lower.contains(&query_lower);
        let author_match = author_lower.contains(&query_lower);

        if title_match || content_match || author_match {
            let snippet = if content_match {
                extract_snippet(article.display_content(), &query_lower, 80)
            } else if title_match {
                article.title.clone()
            } else {
                format!("by {}", article.author)
            };

            results.push(SearchResult {
                article_id: article.id,
                feed_id: article.feed_id,
                title_match,
                content_match,
                author_match,
                snippet,
            });
        }
    }

    results
}

/// Extract a snippet of text around a search match.
fn extract_snippet(text: &str, query: &str, context_chars: usize) -> String {
    let lower = text.to_lowercase();
    if let Some(pos) = lower.find(query) {
        let start = pos.saturating_sub(context_chars);
        let end = (pos + query.len() + context_chars).min(text.len());
        let mut snippet = String::new();
        if start > 0 {
            snippet.push_str("...");
        }
        snippet.push_str(&text[start..end]);
        if end < text.len() {
            snippet.push_str("...");
        }
        snippet
    } else {
        // Shouldn't happen if the caller verified the match, but be safe
        text.chars().take(context_chars * 2).collect()
    }
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state for the RSS reader.
pub struct RssReaderApp {
    pub width: f32,
    pub height: f32,

    // Data
    pub feeds: Vec<Feed>,
    pub articles: Vec<Article>,
    pub folders: Vec<Folder>,
    pub next_feed_id: FeedId,
    pub next_article_id: ArticleId,
    pub next_folder_id: FolderId,

    // UI state
    pub active_pane: ActivePane,
    pub sidebar_selection: SidebarSelection,
    pub selected_article_index: usize,
    pub article_scroll_offset: f32,
    pub content_scroll_offset: f32,
    pub sidebar_scroll_offset: f32,
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub article_list_width: f32,

    // Filtering/sorting
    pub sort_order: SortOrder,
    pub filter_mode: FilterMode,
    pub search_query: String,
    pub search_active: bool,
    pub search_results: Vec<SearchResult>,

    // Display state
    pub show_help: bool,
    pub show_add_feed_dialog: bool,
    pub show_feed_health: bool,
    pub status_message: String,
    pub status_timestamp: u64,

    // Offline cache
    pub cache: OfflineCache,

    // Auto-refresh
    pub global_auto_refresh_seconds: u64,
    pub last_global_refresh: u64,
}

impl RssReaderApp {
    /// Create a new RSS reader app with default dimensions and sample data.
    pub fn new(width: f32, height: f32) -> Self {
        let mut app = Self {
            width,
            height,
            feeds: Vec::new(),
            articles: Vec::new(),
            folders: Vec::new(),
            next_feed_id: 1,
            next_article_id: 1,
            next_folder_id: 1,
            active_pane: ActivePane::ArticleList,
            sidebar_selection: SidebarSelection::AllFeeds,
            selected_article_index: 0,
            article_scroll_offset: 0.0,
            content_scroll_offset: 0.0,
            sidebar_scroll_offset: 0.0,
            sidebar_visible: true,
            sidebar_width: 220.0,
            article_list_width: 340.0,
            sort_order: SortOrder::DateDesc,
            filter_mode: FilterMode::All,
            search_query: String::new(),
            search_active: false,
            search_results: Vec::new(),
            show_help: false,
            show_add_feed_dialog: false,
            show_feed_health: false,
            status_message: String::new(),
            status_timestamp: 0,
            cache: OfflineCache::new(10 * 1024 * 1024), // 10 MB
            global_auto_refresh_seconds: 1800,
            last_global_refresh: 0,
        };
        app.populate_sample_data();
        app
    }

    /// Add a new folder and return its ID.
    pub fn add_folder(&mut self, name: &str) -> FolderId {
        let id = self.next_folder_id;
        self.next_folder_id += 1;
        self.folders.push(Folder::new(id, name));
        id
    }

    /// Add a new feed and return its ID.
    pub fn add_feed(&mut self, title: &str, url: &str, folder_id: Option<FolderId>) -> FeedId {
        let id = self.next_feed_id;
        self.next_feed_id += 1;
        let mut feed = Feed::new(id, title, url);
        feed.folder_id = folder_id;
        self.feeds.push(feed);
        id
    }

    /// Remove a feed by ID and all its articles.
    pub fn remove_feed(&mut self, feed_id: FeedId) {
        self.feeds.retain(|f| f.id != feed_id);
        self.articles.retain(|a| a.feed_id != feed_id);
    }

    /// Rename a feed.
    pub fn rename_feed(&mut self, feed_id: FeedId, new_name: &str) {
        if let Some(feed) = self.feeds.iter_mut().find(|f| f.id == feed_id) {
            feed.title = new_name.to_string();
        }
    }

    /// Remove a folder and optionally its feeds.
    pub fn remove_folder(&mut self, folder_id: FolderId, remove_feeds: bool) {
        self.folders.retain(|f| f.id != folder_id);
        if remove_feeds {
            let feed_ids: Vec<FeedId> = self
                .feeds
                .iter()
                .filter(|f| f.folder_id == Some(folder_id))
                .map(|f| f.id)
                .collect();
            for fid in feed_ids {
                self.remove_feed(fid);
            }
        } else {
            // Unparent feeds from the deleted folder
            for feed in &mut self.feeds {
                if feed.folder_id == Some(folder_id) {
                    feed.folder_id = None;
                }
            }
        }
    }

    /// Move a feed to a different folder.
    pub fn move_feed_to_folder(&mut self, feed_id: FeedId, folder_id: Option<FolderId>) {
        if let Some(feed) = self.feeds.iter_mut().find(|f| f.id == feed_id) {
            feed.folder_id = folder_id;
        }
    }

    /// Add an article to the store and cache it.
    pub fn add_article(&mut self, feed_id: FeedId, title: &str, content: &str) -> ArticleId {
        let id = self.next_article_id;
        self.next_article_id += 1;
        let mut article = Article::new(id, feed_id, title);
        article.content = content.to_string();
        article.cached_text = html_to_text(content);
        self.cache
            .cache_article(id, &article.cached_text, article.published);
        self.articles.push(article);
        id
    }

    /// Toggle read state for the selected article.
    pub fn toggle_read(&mut self) {
        let filtered = self.filtered_article_indices();
        if let Some(&idx) = filtered.get(self.selected_article_index)
            && let Some(article) = self.articles.get_mut(idx) {
                article.is_read = !article.is_read;
            }
    }

    /// Toggle star state for the selected article.
    pub fn toggle_star(&mut self) {
        let filtered = self.filtered_article_indices();
        if let Some(&idx) = filtered.get(self.selected_article_index)
            && let Some(article) = self.articles.get_mut(idx) {
                article.is_starred = !article.is_starred;
            }
    }

    /// Mark all visible articles as read.
    pub fn mark_all_read(&mut self) {
        let indices = self.filtered_article_indices();
        for idx in indices {
            if let Some(article) = self.articles.get_mut(idx) {
                article.is_read = true;
            }
        }
    }

    /// Count unread articles for a specific feed.
    pub fn unread_count_for_feed(&self, feed_id: FeedId) -> usize {
        self.articles
            .iter()
            .filter(|a| a.feed_id == feed_id && !a.is_read)
            .count()
    }

    /// Count total unread articles.
    pub fn total_unread(&self) -> usize {
        self.articles.iter().filter(|a| !a.is_read).count()
    }

    /// Count starred articles.
    pub fn total_starred(&self) -> usize {
        self.articles.iter().filter(|a| a.is_starred).count()
    }

    /// Count unread articles in a folder (across all feeds in that folder).
    pub fn unread_count_for_folder(&self, folder_id: FolderId) -> usize {
        let feed_ids: Vec<FeedId> = self
            .feeds
            .iter()
            .filter(|f| f.folder_id == Some(folder_id))
            .map(|f| f.id)
            .collect();
        self.articles
            .iter()
            .filter(|a| feed_ids.contains(&a.feed_id) && !a.is_read)
            .count()
    }

    /// Get the indices of articles matching current filter, sort, and sidebar selection.
    pub fn filtered_article_indices(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .articles
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                // Filter by sidebar selection
                let sidebar_ok = match self.sidebar_selection {
                    SidebarSelection::AllFeeds => true,
                    SidebarSelection::Feed(fid) => a.feed_id == fid,
                    SidebarSelection::Folder(folder_id) => self
                        .feeds
                        .iter()
                        .any(|f| f.id == a.feed_id && f.folder_id == Some(folder_id)),
                    SidebarSelection::Starred => a.is_starred,
                };
                if !sidebar_ok {
                    return false;
                }

                // Filter by filter mode
                match self.filter_mode {
                    FilterMode::All => true,
                    FilterMode::Unread => !a.is_read,
                    FilterMode::Starred => a.is_starred,
                }
            })
            .filter(|(_, a)| {
                // Filter by search query
                if self.search_query.is_empty() {
                    return true;
                }
                let q = self.search_query.to_lowercase();
                a.title.to_lowercase().contains(&q)
                    || a.author.to_lowercase().contains(&q)
                    || a.display_content().to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        // Sort
        indices.sort_by(|&a_idx, &b_idx| {
            let a = &self.articles[a_idx];
            let b = &self.articles[b_idx];
            match self.sort_order {
                SortOrder::DateDesc => b.published.cmp(&a.published),
                SortOrder::DateAsc => a.published.cmp(&b.published),
                SortOrder::TitleAsc => a.title.cmp(&b.title),
                SortOrder::TitleDesc => b.title.cmp(&a.title),
                SortOrder::FeedNameAsc => {
                    let fa = self.feed_name(a.feed_id);
                    let fb = self.feed_name(b.feed_id);
                    fa.cmp(&fb)
                }
                SortOrder::FeedNameDesc => {
                    let fa = self.feed_name(a.feed_id);
                    let fb = self.feed_name(b.feed_id);
                    fb.cmp(&fa)
                }
            }
        });

        indices
    }

    /// Get the display name of a feed by ID.
    pub fn feed_name(&self, feed_id: FeedId) -> String {
        self.feeds
            .iter()
            .find(|f| f.id == feed_id)
            .map(|f| f.title.clone())
            .unwrap_or_else(|| "Unknown Feed".to_string())
    }

    /// Navigate to the next article.
    pub fn next_article(&mut self) {
        let count = self.filtered_article_indices().len();
        if count > 0 && self.selected_article_index + 1 < count {
            self.selected_article_index += 1;
            self.content_scroll_offset = 0.0;
        }
    }

    /// Navigate to the previous article.
    pub fn prev_article(&mut self) {
        if self.selected_article_index > 0 {
            self.selected_article_index -= 1;
            self.content_scroll_offset = 0.0;
        }
    }

    /// Perform a search and store results.
    pub fn perform_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
        } else {
            self.search_results = search_articles(&self.articles, &self.search_query);
        }
    }

    /// Import feeds from OPML data.
    pub fn import_opml(&mut self, opml_xml: &str) -> Result<usize, String> {
        let outlines = parse_opml(opml_xml)?;
        let mut count = 0;
        for outline in &outlines {
            count += self.import_opml_outline(outline, None);
        }
        Ok(count)
    }

    /// Recursively import OPML outlines.
    fn import_opml_outline(
        &mut self,
        outline: &OpmlOutline,
        parent_folder: Option<FolderId>,
    ) -> usize {
        let mut count = 0;

        if let Some(ref url) = outline.xml_url {
            // This is a feed
            let feed_id = self.add_feed(&outline.text, url, parent_folder);
            if let Some(ref html_url) = outline.html_url
                && let Some(feed) = self.feeds.iter_mut().find(|f| f.id == feed_id) {
                    feed.link = html_url.clone();
                }
            count += 1;
        } else if !outline.children.is_empty() {
            // This is a folder
            let folder_id = self.add_folder(&outline.text);
            for child in &outline.children {
                count += self.import_opml_outline(child, Some(folder_id));
            }
        }

        count
    }

    /// Export current feeds and folders as OPML.
    pub fn export_opml(&self) -> String {
        generate_opml("RSS Reader Feeds", &self.feeds, &self.folders)
    }

    /// Simulate refreshing a feed by ingesting parsed feed data.
    pub fn ingest_parsed_feed(&mut self, feed_id: FeedId, parsed: &ParsedFeed, timestamp: u64) {
        // Update feed metadata
        if let Some(feed) = self.feeds.iter_mut().find(|f| f.id == feed_id) {
            if feed.title.is_empty() || feed.title == feed.url {
                feed.title = parsed.title.clone();
            }
            feed.description = parsed.description.clone();
            feed.link = parsed.link.clone();
            feed.format = parsed.format;
            feed.health.record_success(timestamp);
        }

        // Add new articles (dedup by title+link)
        for pa in &parsed.articles {
            let already_exists = self.articles.iter().any(|a| {
                a.feed_id == feed_id && a.title == pa.title && a.link == pa.link
            });
            if !already_exists {
                let id = self.next_article_id;
                self.next_article_id += 1;
                let mut article = Article::new(id, feed_id, &pa.title);
                article.link = pa.link.clone();
                article.author = pa.author.clone();
                article.published = pa.published;
                article.summary = pa.summary.clone();
                article.content = pa.content.clone();
                article.cached_text = if !pa.content.is_empty() {
                    pa.content.clone()
                } else {
                    pa.summary.clone()
                };
                self.cache
                    .cache_article(id, &article.cached_text, timestamp);
                self.articles.push(article);
            }
        }
    }

    /// Populate sample data for demonstration.
    fn populate_sample_data(&mut self) {
        // Create folders
        let tech_folder = self.add_folder("Technology");
        let news_folder = self.add_folder("News");
        let science_folder = self.add_folder("Science");
        let dev_folder = self.add_folder("Development");

        // Create feeds
        let rust_feed = self.add_feed(
            "This Week in Rust",
            "https://this-week-in-rust.org/atom.xml",
            Some(dev_folder),
        );
        let hn_feed = self.add_feed(
            "Hacker News",
            "https://hnrss.org/frontpage",
            Some(tech_folder),
        );
        let bbc_feed = self.add_feed(
            "BBC World News",
            "https://feeds.bbci.co.uk/news/world/rss.xml",
            Some(news_folder),
        );
        let arxiv_feed = self.add_feed(
            "arXiv CS",
            "https://arxiv.org/rss/cs.AI",
            Some(science_folder),
        );
        let lobsters_feed = self.add_feed(
            "Lobsters",
            "https://lobste.rs/rss",
            Some(tech_folder),
        );
        let planet_feed = self.add_feed(
            "Planet Rust",
            "https://planet.rust-lang.org/atom.xml",
            Some(dev_folder),
        );

        // Set feed formats
        for feed in &mut self.feeds {
            if feed.url.contains("atom.xml") {
                feed.format = FeedFormat::Atom1;
            } else {
                feed.format = FeedFormat::Rss2;
            }
            feed.health.record_success(1_700_000_000);
        }

        // Simulate an error on one feed
        if let Some(arxiv) = self.feeds.iter_mut().find(|f| f.id == arxiv_feed) {
            arxiv.health.record_failure(1_700_100_000, "Connection timeout");
            arxiv.health.record_failure(1_700_200_000, "Connection timeout");
        }

        // Create sample articles
        let base_ts: u64 = 1_700_000_000;

        self.create_sample_article(
            rust_feed,
            "This Week in Rust 520",
            "TWiR Team",
            base_ts + 86400 * 7,
            "This week's crate is ratatui, a library for building terminal UIs. \
             Rust 1.74 was released with return-position impl Trait in traits, \
             better async diagnostics, and improved const generics.",
            false,
            true,
        );
        self.create_sample_article(
            rust_feed,
            "This Week in Rust 519",
            "TWiR Team",
            base_ts + 86400 * 6,
            "Highlights include the new async working group roadmap, \
             improvements to cargo's dependency resolution, and a new RFC \
             for pattern types. Community spotlight on axum web framework.",
            true,
            false,
        );
        self.create_sample_article(
            rust_feed,
            "This Week in Rust 518",
            "TWiR Team",
            base_ts + 86400 * 5,
            "Feature spotlight: edition 2024 planning, proc-macro improvements, \
             and the Rust Foundation's engineering update. Notable crate: \
             polars for high-performance DataFrames.",
            true,
            false,
        );

        self.create_sample_article(
            hn_feed,
            "Show HN: A Microkernel OS Written Entirely by AI",
            "rustdev42",
            base_ts + 86400 * 7 + 3600,
            "An ambitious project to build a complete desktop OS using AI pair programming. \
             Features include a custom GUI toolkit, package manager, and full POSIX compatibility. \
             The kernel uses 16KiB pages and a capability-based security model.",
            false,
            true,
        );
        self.create_sample_article(
            hn_feed,
            "SQLite Considers Adding a Native Vector Search Extension",
            "databasenews",
            base_ts + 86400 * 6 + 7200,
            "The SQLite team is exploring built-in vector similarity search, \
             potentially making it the simplest path to vector database functionality. \
             Discussion around performance characteristics and API design.",
            false,
            false,
        );
        self.create_sample_article(
            hn_feed,
            "Why WebAssembly Is the Future of Server-Side Rendering",
            "webdevtimes",
            base_ts + 86400 * 5 + 1800,
            "A deep dive into using WebAssembly for server-side rendering, \
             with benchmarks showing 3x improvement over V8 for certain workloads. \
             Covers WASI, component model, and toolchain maturity.",
            true,
            false,
        );
        self.create_sample_article(
            hn_feed,
            "The Hidden Costs of Microservices Nobody Talks About",
            "archdigest",
            base_ts + 86400 * 4 + 5400,
            "An honest retrospective on microservices at scale: observability overhead, \
             distributed transaction complexity, cold start latency, and the cognitive \
             load on developers. Includes cost analysis from a Fortune 500 migration.",
            true,
            false,
        );

        self.create_sample_article(
            bbc_feed,
            "Major Climate Agreement Reached at Summit",
            "BBC Correspondents",
            base_ts + 86400 * 7 + 7200,
            "World leaders have agreed on a landmark climate package that includes \
             binding emissions targets for developing nations. The agreement covers \
             carbon markets, deforestation limits, and a $100 billion adaptation fund.",
            false,
            false,
        );
        self.create_sample_article(
            bbc_feed,
            "Space Agency Announces New Mars Mission Timeline",
            "Science Desk",
            base_ts + 86400 * 6 + 3600,
            "The European Space Agency has revealed an accelerated timeline for its \
             Mars sample return mission, with launch now planned for 2028. The mission \
             will work in conjunction with NASA's Perseverance rover samples.",
            false,
            true,
        );
        self.create_sample_article(
            bbc_feed,
            "Global Chip Shortage Shows Signs of Easing",
            "Tech Editor",
            base_ts + 86400 * 3 + 9000,
            "Semiconductor supply chains are stabilizing as new fabrication plants \
             come online in Arizona and Dresden. Lead times have dropped significantly \
             for automotive and consumer electronics chipsets.",
            true,
            false,
        );

        self.create_sample_article(
            arxiv_feed,
            "Scaling Laws for Neural Architecture Search",
            "Chen, Li, et al.",
            base_ts + 86400 * 7 + 1200,
            "We present empirical scaling laws for neural architecture search (NAS) \
             that predict search cost and final model performance as functions of \
             search space size and compute budget. Our findings suggest that current \
             NAS methods leave significant performance on the table.",
            false,
            false,
        );
        self.create_sample_article(
            arxiv_feed,
            "Efficient Attention Mechanisms for Long Sequences",
            "Wang, Johnson, et al.",
            base_ts + 86400 * 5 + 4800,
            "We propose a novel attention mechanism that achieves O(n log n) complexity \
             while maintaining comparable performance to standard O(n^2) attention. \
             Evaluations on document understanding and genomics tasks show strong results.",
            false,
            true,
        );

        self.create_sample_article(
            lobsters_feed,
            "Writing a Tree-Walking Interpreter in Rust",
            "compiler_nerd",
            base_ts + 86400 * 6 + 5400,
            "A tutorial series on building a complete interpreter for a small language, \
             covering lexing, parsing, type checking, and evaluation. Uses Rust's enums \
             and pattern matching for clean AST representation.",
            false,
            false,
        );
        self.create_sample_article(
            lobsters_feed,
            "The State of Linux Desktop in 2024",
            "pinguin",
            base_ts + 86400 * 4 + 2700,
            "A comprehensive review of the Linux desktop ecosystem: Wayland adoption, \
             Flatpak maturity, gaming via Proton, and the ongoing fragmentation debate. \
             Includes user satisfaction survey results from 50,000 respondents.",
            true,
            false,
        );

        self.create_sample_article(
            planet_feed,
            "Async Rust: A Practical Guide to Pitfalls",
            "Alice Ryhl",
            base_ts + 86400 * 7 + 2400,
            "Common mistakes in async Rust code and how to avoid them: accidental blocking, \
             cancellation safety, select! gotchas, and async drop. Includes real-world \
             examples from the Tokio maintainer team.",
            false,
            false,
        );
        self.create_sample_article(
            planet_feed,
            "Introducing Bevy 0.14: A Game Engine Update",
            "Bevy Contributors",
            base_ts + 86400 * 5 + 6000,
            "Bevy 0.14 brings deferred rendering, screen-space ambient occlusion, \
             a revamped asset system, and significantly improved compile times. \
             The ECS system now supports component hooks and observers.",
            false,
            true,
        );
    }

    /// Helper to create a sample article with all fields populated.
    fn create_sample_article(
        &mut self,
        feed_id: FeedId,
        title: &str,
        author: &str,
        published: u64,
        content: &str,
        is_read: bool,
        is_starred: bool,
    ) {
        let id = self.next_article_id;
        self.next_article_id += 1;
        let mut article = Article::new(id, feed_id, title);
        article.author = author.to_string();
        article.published = published;
        article.content = content.to_string();
        article.cached_text = content.to_string();
        article.is_read = is_read;
        article.is_starred = is_starred;
        self.cache.cache_article(id, content, published);
        self.articles.push(article);
    }

    /// Get the currently selected article, if any.
    pub fn selected_article(&self) -> Option<&Article> {
        let filtered = self.filtered_article_indices();
        filtered
            .get(self.selected_article_index)
            .and_then(|&idx| self.articles.get(idx))
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire application frame, producing a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background fill
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title bar
        let title_bar_height = 40.0;
        self.render_title_bar(&mut cmds, title_bar_height);

        // Toolbar (filter/sort/search)
        let toolbar_y = title_bar_height;
        let toolbar_height = 36.0;
        self.render_toolbar(&mut cmds, toolbar_y, toolbar_height);

        // Main content area
        let content_y = toolbar_y + toolbar_height;
        let content_height = self.height - content_y - 28.0; // leave room for status bar

        if self.sidebar_visible {
            // Three-pane layout
            let sidebar_x = 0.0;
            let list_x = self.sidebar_width;
            let content_pane_x = self.sidebar_width + self.article_list_width;
            let content_pane_width = self.width - content_pane_x;

            self.render_sidebar(&mut cmds, sidebar_x, content_y, self.sidebar_width, content_height);

            // Sidebar/list separator
            cmds.push(RenderCommand::Line {
                x1: list_x,
                y1: content_y,
                x2: list_x,
                y2: content_y + content_height,
                color: SURFACE1,
                width: 1.0,
            });

            self.render_article_list(
                &mut cmds,
                list_x,
                content_y,
                self.article_list_width,
                content_height,
            );

            // List/content separator
            cmds.push(RenderCommand::Line {
                x1: content_pane_x,
                y1: content_y,
                x2: content_pane_x,
                y2: content_y + content_height,
                color: SURFACE1,
                width: 1.0,
            });

            self.render_content_view(
                &mut cmds,
                content_pane_x,
                content_y,
                content_pane_width,
                content_height,
            );
        } else {
            // Two-pane layout (no sidebar)
            let list_x = 0.0;
            let content_pane_x = self.article_list_width;
            let content_pane_width = self.width - content_pane_x;

            self.render_article_list(
                &mut cmds,
                list_x,
                content_y,
                self.article_list_width,
                content_height,
            );

            cmds.push(RenderCommand::Line {
                x1: content_pane_x,
                y1: content_y,
                x2: content_pane_x,
                y2: content_y + content_height,
                color: SURFACE1,
                width: 1.0,
            });

            self.render_content_view(
                &mut cmds,
                content_pane_x,
                content_y,
                content_pane_width,
                content_height,
            );
        }

        // Status bar
        let status_y = self.height - 28.0;
        self.render_status_bar(&mut cmds, status_y, 28.0);

        // Overlays
        if self.show_help {
            self.render_help_overlay(&mut cmds);
        }

        if self.show_add_feed_dialog {
            self.render_add_feed_dialog(&mut cmds);
        }

        if self.show_feed_health {
            self.render_feed_health_overlay(&mut cmds);
        }

        cmds
    }

    /// Render the title bar with app name and quick actions.
    fn render_title_bar(&self, cmds: &mut Vec<RenderCommand>, height: f32) {
        // Title bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // RSS icon (simplified as text)
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "RSS".to_string(),
            font_size: 13.0,
            color: PEACH,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: 48.0,
            y: 10.0,
            text: "Feed Reader".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Feed count badge
        let feed_count = self.feeds.len();
        let article_count = self.articles.len();
        let unread = self.total_unread();
        let info = format!("{feed_count} feeds | {article_count} articles | {unread} unread");
        cmds.push(RenderCommand::Text {
            x: 200.0,
            y: 13.0,
            text: info,
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Right side: refresh all button area
        let refresh_x = self.width - 120.0;
        cmds.push(RenderCommand::FillRect {
            x: refresh_x,
            y: 8.0,
            width: 100.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: refresh_x + 10.0,
            y: 12.0,
            text: "Refresh All".to_string(),
            font_size: 12.0,
            color: BLUE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: height,
            x2: self.width,
            y2: height,
            color: SURFACE1,
            width: 1.0,
        });
    }

    /// Render the toolbar with filter, sort, and search controls.
    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Filter button
        let filter_x = 12.0;
        let filter_color = match self.filter_mode {
            FilterMode::All => SUBTEXT0,
            FilterMode::Unread => BLUE,
            FilterMode::Starred => YELLOW,
        };
        cmds.push(RenderCommand::FillRect {
            x: filter_x,
            y: y + 6.0,
            width: 110.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: filter_x + 8.0,
            y: y + 10.0,
            text: format!("Filter: {}", self.filter_mode.label()),
            font_size: 11.0,
            color: filter_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(94.0),
        });

        // Sort button
        let sort_x = 132.0;
        cmds.push(RenderCommand::FillRect {
            x: sort_x,
            y: y + 6.0,
            width: 160.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: sort_x + 8.0,
            y: y + 10.0,
            text: format!("Sort: {}", self.sort_order.label()),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(144.0),
        });

        // Search box
        let search_x = self.width - 260.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x,
            y: y + 6.0,
            width: 240.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        if self.search_active {
            cmds.push(RenderCommand::StrokeRect {
                x: search_x,
                y: y + 6.0,
                width: 240.0,
                height: 24.0,
                color: BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        let search_display = if self.search_query.is_empty() {
            "Search articles... (Ctrl+F)".to_string()
        } else {
            self.search_query.clone()
        };
        let search_text_color = if self.search_query.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 8.0,
            y: y + 10.0,
            text: search_display,
            font_size: 11.0,
            color: search_text_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(224.0),
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + height,
            x2: self.width,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });
    }

    /// Render the sidebar with folders and feeds.
    fn render_sidebar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        panel_width: f32,
        panel_height: f32,
    ) {
        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: panel_width,
            height: panel_height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Clip to sidebar area
        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width: panel_width,
            height: panel_height,
        });

        let item_height: f32 = 28.0;
        let mut cy = y + 8.0 - self.sidebar_scroll_offset;

        // "All Feeds" entry
        let is_selected = self.sidebar_selection == SidebarSelection::AllFeeds;
        let active_highlight = is_selected && self.active_pane == ActivePane::Sidebar;
        if is_selected {
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: cy,
                width: panel_width - 8.0,
                height: item_height,
                color: if active_highlight { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
        }
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 6.0,
            text: "All Feeds".to_string(),
            font_size: 13.0,
            color: if is_selected { TEXT } else { SUBTEXT0 },
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width - 60.0),
        });
        // Unread count badge
        let total_unread = self.total_unread();
        if total_unread > 0 {
            let badge_text = format!("{total_unread}");
            cmds.push(RenderCommand::FillRect {
                x: x + panel_width - 44.0,
                y: cy + 4.0,
                width: 32.0,
                height: 20.0,
                color: BLUE,
                corner_radii: CornerRadii::all(10.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + panel_width - 38.0,
                y: cy + 7.0,
                text: badge_text,
                font_size: 11.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(24.0),
            });
        }
        cy += item_height + 2.0;

        // "Starred" entry
        let is_starred_selected = self.sidebar_selection == SidebarSelection::Starred;
        let starred_highlight = is_starred_selected && self.active_pane == ActivePane::Sidebar;
        if is_starred_selected {
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: cy,
                width: panel_width - 8.0,
                height: item_height,
                color: if starred_highlight { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
        }
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 6.0,
            text: "Starred".to_string(),
            font_size: 13.0,
            color: if is_starred_selected { YELLOW } else { SUBTEXT0 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_width - 60.0),
        });
        let starred_count = self.total_starred();
        if starred_count > 0 {
            cmds.push(RenderCommand::FillRect {
                x: x + panel_width - 44.0,
                y: cy + 4.0,
                width: 32.0,
                height: 20.0,
                color: YELLOW,
                corner_radii: CornerRadii::all(10.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + panel_width - 38.0,
                y: cy + 7.0,
                text: format!("{starred_count}"),
                font_size: 11.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(24.0),
            });
        }
        cy += item_height + 8.0;

        // Separator
        cmds.push(RenderCommand::Line {
            x1: x + 12.0,
            y1: cy,
            x2: x + panel_width - 12.0,
            y2: cy,
            color: SURFACE0,
            width: 1.0,
        });
        cy += 8.0;

        // Folders and feeds
        for folder in &self.folders {
            // Folder header
            let is_folder_selected =
                self.sidebar_selection == SidebarSelection::Folder(folder.id);
            let folder_highlight = is_folder_selected && self.active_pane == ActivePane::Sidebar;
            if is_folder_selected {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: cy,
                    width: panel_width - 8.0,
                    height: item_height,
                    color: if folder_highlight { SURFACE1 } else { SURFACE0 },
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Expand/collapse indicator
            let indicator = if folder.is_expanded { "v" } else { ">" };
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 6.0,
                text: indicator.to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 6.0,
                text: folder.name.clone(),
                font_size: 12.0,
                color: if is_folder_selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(panel_width - 72.0),
            });

            // Folder unread count
            let folder_unread = self.unread_count_for_folder(folder.id);
            if folder_unread > 0 {
                cmds.push(RenderCommand::Text {
                    x: x + panel_width - 40.0,
                    y: cy + 7.0,
                    text: format!("{folder_unread}"),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
            cy += item_height;

            // Feeds inside folder (if expanded)
            if folder.is_expanded {
                for feed in &self.feeds {
                    if feed.folder_id != Some(folder.id) {
                        continue;
                    }
                    let is_feed_selected =
                        self.sidebar_selection == SidebarSelection::Feed(feed.id);
                    let feed_highlight =
                        is_feed_selected && self.active_pane == ActivePane::Sidebar;
                    if is_feed_selected {
                        cmds.push(RenderCommand::FillRect {
                            x: x + 4.0,
                            y: cy,
                            width: panel_width - 8.0,
                            height: item_height,
                            color: if feed_highlight { SURFACE1 } else { SURFACE0 },
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }

                    // Health indicator dot
                    let health_color = if feed.health.is_healthy() { GREEN } else { RED };
                    cmds.push(RenderCommand::FillRect {
                        x: x + 28.0,
                        y: cy + 10.0,
                        width: 6.0,
                        height: 6.0,
                        color: health_color,
                        corner_radii: CornerRadii::all(3.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: x + 40.0,
                        y: cy + 6.0,
                        text: feed.title.clone(),
                        font_size: 12.0,
                        color: if is_feed_selected { TEXT } else { SUBTEXT0 },
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(panel_width - 88.0),
                    });

                    // Feed unread count
                    let feed_unread = self.unread_count_for_feed(feed.id);
                    if feed_unread > 0 {
                        cmds.push(RenderCommand::Text {
                            x: x + panel_width - 40.0,
                            y: cy + 7.0,
                            text: format!("{feed_unread}"),
                            font_size: 10.0,
                            color: BLUE,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                        });
                    }
                    cy += item_height;
                }
            }

            cy += 2.0; // spacing between folders
        }

        // Ungrouped feeds (no folder)
        let ungrouped: Vec<&Feed> = self
            .feeds
            .iter()
            .filter(|f| f.folder_id.is_none())
            .collect();
        if !ungrouped.is_empty() {
            cy += 4.0;
            cmds.push(RenderCommand::Line {
                x1: x + 12.0,
                y1: cy,
                x2: x + panel_width - 12.0,
                y2: cy,
                color: SURFACE0,
                width: 1.0,
            });
            cy += 8.0;

            for feed in ungrouped {
                let is_feed_selected =
                    self.sidebar_selection == SidebarSelection::Feed(feed.id);
                let feed_highlight =
                    is_feed_selected && self.active_pane == ActivePane::Sidebar;
                if is_feed_selected {
                    cmds.push(RenderCommand::FillRect {
                        x: x + 4.0,
                        y: cy,
                        width: panel_width - 8.0,
                        height: item_height,
                        color: if feed_highlight { SURFACE1 } else { SURFACE0 },
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                let health_color = if feed.health.is_healthy() { GREEN } else { RED };
                cmds.push(RenderCommand::FillRect {
                    x: x + 12.0,
                    y: cy + 10.0,
                    width: 6.0,
                    height: 6.0,
                    color: health_color,
                    corner_radii: CornerRadii::all(3.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 24.0,
                    y: cy + 6.0,
                    text: feed.title.clone(),
                    font_size: 12.0,
                    color: if is_feed_selected { TEXT } else { SUBTEXT0 },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width - 72.0),
                });

                let feed_unread = self.unread_count_for_feed(feed.id);
                if feed_unread > 0 {
                    cmds.push(RenderCommand::Text {
                        x: x + panel_width - 40.0,
                        y: cy + 7.0,
                        text: format!("{feed_unread}"),
                        font_size: 10.0,
                        color: BLUE,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
                cy += item_height;
            }
        }

        cmds.push(RenderCommand::PopClip);
    }

    /// Render the article list pane.
    fn render_article_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        panel_width: f32,
        panel_height: f32,
    ) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: panel_width,
            height: panel_height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width: panel_width,
            height: panel_height,
        });

        let filtered = self.filtered_article_indices();
        let item_height: f32 = 72.0;
        let mut cy = y + 4.0 - self.article_scroll_offset;

        if filtered.is_empty() {
            // Empty state
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + panel_height / 2.0 - 20.0,
                text: "No articles match the current filter".to_string(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - 32.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + panel_height / 2.0 + 4.0,
                text: "Try changing the filter or selecting a different feed".to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Light,
                max_width: Some(panel_width - 32.0),
            });
        }

        for (list_idx, &article_idx) in filtered.iter().enumerate() {
            if cy + item_height < y {
                cy += item_height;
                continue; // Above visible area
            }
            if cy > y + panel_height {
                break; // Below visible area
            }

            let article = &self.articles[article_idx];
            let is_selected = list_idx == self.selected_article_index;
            let is_active = is_selected && self.active_pane == ActivePane::ArticleList;

            // Selection highlight
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: cy,
                    width: panel_width - 8.0,
                    height: item_height - 4.0,
                    color: if is_active { SURFACE1 } else { SURFACE0 },
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            // Unread indicator dot
            if !article.is_read {
                cmds.push(RenderCommand::FillRect {
                    x: x + 10.0,
                    y: cy + 10.0,
                    width: 8.0,
                    height: 8.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Star indicator
            if article.is_starred {
                cmds.push(RenderCommand::Text {
                    x: x + panel_width - 24.0,
                    y: cy + 6.0,
                    text: "*".to_string(),
                    font_size: 16.0,
                    color: YELLOW,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Title
            let title_color = if article.is_read { SUBTEXT0 } else { TEXT };
            let title_weight = if article.is_read {
                FontWeightHint::Regular
            } else {
                FontWeightHint::Bold
            };
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 6.0,
                text: article.title.clone(),
                font_size: 13.0,
                color: title_color,
                font_weight: title_weight,
                max_width: Some(panel_width - 56.0),
            });

            // Feed name and date
            let feed_name = self.feed_name(article.feed_id);
            let date_str = format_timestamp(article.published);
            let meta_text = format!("{feed_name} | {date_str}");
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 26.0,
                text: meta_text,
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - 40.0),
            });

            // Summary preview
            let preview = if article.summary.len() > 100 {
                format!("{}...", &article.summary[..100])
            } else if article.summary.is_empty() {
                let content = article.display_content();
                if content.len() > 100 {
                    format!("{}...", &content[..100])
                } else {
                    content.to_string()
                }
            } else {
                article.summary.clone()
            };
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 44.0,
                text: preview,
                font_size: 11.0,
                color: SURFACE2,
                font_weight: FontWeightHint::Light,
                max_width: Some(panel_width - 40.0),
            });

            cy += item_height;
        }

        cmds.push(RenderCommand::PopClip);
    }

    /// Render the article content view pane.
    fn render_content_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        panel_width: f32,
        panel_height: f32,
    ) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: panel_width,
            height: panel_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width: panel_width,
            height: panel_height,
        });

        let article = self.selected_article();

        if let Some(article) = article {
            let padding = 20.0;
            let content_width = panel_width - padding * 2.0;
            let mut cy = y + padding - self.content_scroll_offset;

            // Article title
            cmds.push(RenderCommand::Text {
                x: x + padding,
                y: cy,
                text: article.title.clone(),
                font_size: 20.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(content_width),
            });
            cy += 32.0;

            // Author and date line
            let author_text = if article.author.is_empty() {
                String::new()
            } else {
                format!("by {} | ", article.author)
            };
            let date_text = format_timestamp(article.published);
            let feed_name = self.feed_name(article.feed_id);
            let meta_line = format!("{author_text}{date_text} | {feed_name}");
            cmds.push(RenderCommand::Text {
                x: x + padding,
                y: cy,
                text: meta_line,
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_width),
            });
            cy += 24.0;

            // Status badges (read/unread, starred)
            let status_text = if article.is_read { "Read" } else { "Unread" };
            let status_color = if article.is_read { OVERLAY0 } else { BLUE };
            cmds.push(RenderCommand::FillRect {
                x: x + padding,
                y: cy,
                width: 60.0,
                height: 22.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + padding + 8.0,
                y: cy + 4.0,
                text: status_text.to_string(),
                font_size: 11.0,
                color: status_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            if article.is_starred {
                cmds.push(RenderCommand::FillRect {
                    x: x + padding + 68.0,
                    y: cy,
                    width: 70.0,
                    height: 22.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + padding + 76.0,
                    y: cy + 4.0,
                    text: "* Starred".to_string(),
                    font_size: 11.0,
                    color: YELLOW,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Cached indicator
            if self.cache.is_cached(article.id) {
                let cache_x = if article.is_starred {
                    x + padding + 146.0
                } else {
                    x + padding + 68.0
                };
                cmds.push(RenderCommand::FillRect {
                    x: cache_x,
                    y: cy,
                    width: 62.0,
                    height: 22.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: cache_x + 6.0,
                    y: cy + 4.0,
                    text: "Cached".to_string(),
                    font_size: 11.0,
                    color: GREEN,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            cy += 32.0;

            // Separator line
            cmds.push(RenderCommand::Line {
                x1: x + padding,
                y1: cy,
                x2: x + padding + content_width,
                y2: cy,
                color: SURFACE0,
                width: 1.0,
            });
            cy += 16.0;

            // Article link
            if !article.link.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + padding,
                    y: cy,
                    text: article.link.clone(),
                    font_size: 11.0,
                    color: BLUE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_width),
                });
                cy += 20.0;
            }

            // Article body text (wrapped into lines)
            let body = article.display_content();
            let lines = wrap_text(body, content_width, 14.0);
            let line_height: f32 = 22.0;

            for line in &lines {
                if cy + line_height < y {
                    cy += line_height;
                    continue;
                }
                if cy > y + panel_height {
                    break;
                }
                cmds.push(RenderCommand::Text {
                    x: x + padding,
                    y: cy,
                    text: line.clone(),
                    font_size: 14.0,
                    color: SUBTEXT1,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_width),
                });
                cy += line_height;
            }
        } else {
            // No article selected
            cmds.push(RenderCommand::Text {
                x: x + panel_width / 2.0 - 80.0,
                y: y + panel_height / 2.0 - 20.0,
                text: "Select an article to read".to_string(),
                font_size: 15.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - 40.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + panel_width / 2.0 - 110.0,
                y: y + panel_height / 2.0 + 10.0,
                text: "Use J/K or arrow keys to navigate".to_string(),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Light,
                max_width: Some(panel_width - 40.0),
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    /// Render the status bar at the bottom of the window.
    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.width,
            y2: y,
            color: SURFACE0,
            width: 1.0,
        });

        // Active pane indicator
        let pane_label = match self.active_pane {
            ActivePane::Sidebar => "Sidebar",
            ActivePane::ArticleList => "Article List",
            ActivePane::ContentView => "Content View",
        };
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: y + 7.0,
            text: pane_label.to_string(),
            font_size: 11.0,
            color: BLUE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Filter and sort status
        cmds.push(RenderCommand::Text {
            x: 120.0,
            y: y + 7.0,
            text: format!(
                "{} | {}",
                self.filter_mode.label(),
                self.sort_order.label()
            ),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        // Status message
        if !self.status_message.is_empty() {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0,
                y: y + 7.0,
                text: self.status_message.clone(),
                font_size: 11.0,
                color: PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width / 2.0 - 120.0),
            });
        }

        // Cache status
        let cache_info = format!(
            "Cache: {} articles ({:.1} KB)",
            self.cache.count(),
            self.cache.current_size as f64 / 1024.0
        );
        cmds.push(RenderCommand::Text {
            x: self.width - 200.0,
            y: y + 7.0,
            text: cache_info,
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });

        // Help hint
        cmds.push(RenderCommand::Text {
            x: self.width - 40.0,
            y: y + 7.0,
            text: "? Help".to_string(),
            font_size: 10.0,
            color: SURFACE2,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the keyboard shortcuts help overlay.
    fn render_help_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        // Dimmed background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let dialog_width: f32 = 450.0;
        let dialog_height: f32 = 520.0;
        let dx = (self.width - dialog_width) / 2.0;
        let dy = (self.height - dialog_height) / 2.0;

        // Dialog background
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_width,
            height: dialog_height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Dialog border
        cmds.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_width,
            height: dialog_height,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 16.0,
            text: "Keyboard Shortcuts".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Close hint
        cmds.push(RenderCommand::Text {
            x: dx + dialog_width - 80.0,
            y: dy + 18.0,
            text: "Press ? to close".to_string(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: dx + 20.0,
            y1: dy + 44.0,
            x2: dx + dialog_width - 20.0,
            y2: dy + 44.0,
            color: SURFACE1,
            width: 1.0,
        });

        // Shortcut list
        let mut row_y = dy + 56.0;
        let row_height: f32 = 22.0;

        for action in ALL_KEY_ACTIONS {
            if row_y + row_height > dy + dialog_height - 10.0 {
                break;
            }

            // Key hint
            cmds.push(RenderCommand::FillRect {
                x: dx + 20.0,
                y: row_y,
                width: 120.0,
                height: 18.0,
                color: CRUST,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: dx + 26.0,
                y: row_y + 2.0,
                text: action.key_hint().to_string(),
                font_size: 11.0,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: Some(110.0),
            });

            // Description
            cmds.push(RenderCommand::Text {
                x: dx + 152.0,
                y: row_y + 2.0,
                text: action.description().to_string(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_width - 180.0),
            });

            row_y += row_height;
        }
    }

    /// Render the "Add Feed" dialog overlay.
    fn render_add_feed_dialog(&self, cmds: &mut Vec<RenderCommand>) {
        // Dimmed background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let dialog_width: f32 = 400.0;
        let dialog_height: f32 = 260.0;
        let dx = (self.width - dialog_width) / 2.0;
        let dy = (self.height - dialog_height) / 2.0;

        // Dialog background
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_width,
            height: dialog_height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_width,
            height: dialog_height,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 16.0,
            text: "Add New Feed".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // URL input label
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 56.0,
            text: "Feed URL:".to_string(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // URL input field
        cmds.push(RenderCommand::FillRect {
            x: dx + 20.0,
            y: dy + 76.0,
            width: dialog_width - 40.0,
            height: 32.0,
            color: CRUST,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: dx + 20.0,
            y: dy + 76.0,
            width: dialog_width - 40.0,
            height: 32.0,
            color: BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + 28.0,
            y: dy + 84.0,
            text: "https://example.com/feed.xml".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_width - 56.0),
        });

        // Folder selection label
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 124.0,
            text: "Folder (optional):".to_string(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Folder dropdown
        cmds.push(RenderCommand::FillRect {
            x: dx + 20.0,
            y: dy + 144.0,
            width: dialog_width - 40.0,
            height: 32.0,
            color: CRUST,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + 28.0,
            y: dy + 152.0,
            text: "None (ungrouped)".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_width - 56.0),
        });

        // Buttons
        let button_y = dy + dialog_height - 52.0;

        // Cancel button
        cmds.push(RenderCommand::FillRect {
            x: dx + dialog_width - 200.0,
            y: button_y,
            width: 80.0,
            height: 32.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + dialog_width - 182.0,
            y: button_y + 8.0,
            text: "Cancel".to_string(),
            font_size: 12.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Add button
        cmds.push(RenderCommand::FillRect {
            x: dx + dialog_width - 108.0,
            y: button_y,
            width: 88.0,
            height: 32.0,
            color: BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + dialog_width - 88.0,
            y: button_y + 8.0,
            text: "Add Feed".to_string(),
            font_size: 12.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render the feed health status overlay.
    fn render_feed_health_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        // Dimmed background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let dialog_width: f32 = 500.0;
        let row_height: f32 = 36.0;
        let header_height: f32 = 60.0;
        let dialog_height = header_height + (self.feeds.len() as f32) * row_height + 20.0;
        let dx = (self.width - dialog_width) / 2.0;
        let dy = (self.height - dialog_height) / 2.0;

        // Dialog background
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_width,
            height: dialog_height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_width,
            height: dialog_height,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 16.0,
            text: "Feed Health Status".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Column headers
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 44.0,
            text: "Feed".to_string(),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: dx + 200.0,
            y: dy + 44.0,
            text: "Format".to_string(),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: dx + 280.0,
            y: dy + 44.0,
            text: "Status".to_string(),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: dx + 20.0,
            y1: dy + header_height,
            x2: dx + dialog_width - 20.0,
            y2: dy + header_height,
            color: SURFACE1,
            width: 1.0,
        });

        // Feed rows
        let mut row_y = dy + header_height + 8.0;
        for feed in &self.feeds {
            // Health dot
            let health_color = if feed.health.is_healthy() { GREEN } else { RED };
            cmds.push(RenderCommand::FillRect {
                x: dx + 20.0,
                y: row_y + 6.0,
                width: 8.0,
                height: 8.0,
                color: health_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Feed name
            cmds.push(RenderCommand::Text {
                x: dx + 36.0,
                y: row_y + 2.0,
                text: feed.title.clone(),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(156.0),
            });

            // Format
            cmds.push(RenderCommand::Text {
                x: dx + 200.0,
                y: row_y + 2.0,
                text: feed.format.label().to_string(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Status text
            let status = feed.health.status_text();
            let status_color = if feed.health.is_healthy() {
                GREEN
            } else {
                RED
            };
            cmds.push(RenderCommand::Text {
                x: dx + 280.0,
                y: row_y + 2.0,
                text: status,
                font_size: 11.0,
                color: status_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_width - 300.0),
            });

            // URL underneath
            cmds.push(RenderCommand::Text {
                x: dx + 36.0,
                y: row_y + 18.0,
                text: feed.url.clone(),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Light,
                max_width: Some(dialog_width - 60.0),
            });

            row_y += row_height;
        }
    }
}

// ============================================================================
// Text wrapping utility
// ============================================================================

/// Simple word-wrap that breaks text into lines fitting within `max_width`.
///
/// Assumes an approximate character width based on font size.
pub fn wrap_text(text: &str, max_width: f32, font_size: f32) -> Vec<String> {
    let char_width = font_size * 0.55; // Approximate average character width
    let max_chars = (max_width / char_width) as usize;

    if max_chars == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            if word.len() > max_chars {
                // Word is longer than the line — break it
                let mut remaining = word;
                while remaining.len() > max_chars {
                    let (chunk, rest) = remaining.split_at(max_chars);
                    lines.push(chunk.to_string());
                    remaining = rest;
                }
                current_line = remaining.to_string();
            } else {
                current_line = word.to_string();
            }
        } else if current_line.len() + 1 + word.len() <= max_chars {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let app = RssReaderApp::new(1200.0, 800.0);
    let cmds = app.render();

    // In the actual OS, these commands would be submitted to the compositor.
    // For now, we verify the app produces valid render output.
    let _cmd_count = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // XML Parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_simple_element() {
        let xml = "<root>hello</root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.tag, "root");
        assert_eq!(elem.text_content(), "hello");
    }

    #[test]
    fn test_parse_nested_elements() {
        let xml = "<a><b>inner</b></a>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.tag, "a");
        let b = elem.find_child("b").unwrap();
        assert_eq!(b.text_content(), "inner");
    }

    #[test]
    fn test_parse_attributes() {
        let xml = r#"<link rel="alternate" href="http://example.com"/>"#;
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.attr("rel"), Some("alternate"));
        assert_eq!(elem.attr("href"), Some("http://example.com"));
    }

    #[test]
    fn test_parse_self_closing_tag() {
        let xml = "<img src=\"test.png\"/>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.tag, "img");
        assert_eq!(elem.attr("src"), Some("test.png"));
        assert!(elem.children.is_empty());
    }

    #[test]
    fn test_parse_xml_declaration_skipped() {
        let xml = "<?xml version=\"1.0\"?><root>text</root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.tag, "root");
        assert_eq!(elem.text_content(), "text");
    }

    #[test]
    fn test_parse_comment_skipped() {
        let xml = "<root><!-- comment -->content</root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.text_content(), "content");
    }

    #[test]
    fn test_parse_cdata() {
        let xml = "<root><![CDATA[<b>not a tag</b>]]></root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.text_content(), "<b>not a tag</b>");
    }

    #[test]
    fn test_parse_entities() {
        let xml = "<root>&amp; &lt; &gt; &quot; &apos;</root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.text_content(), "& < > \" '");
    }

    #[test]
    fn test_parse_numeric_entity() {
        let xml = "<root>&#65;&#x42;</root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.text_content(), "AB");
    }

    #[test]
    fn test_parse_multiple_children() {
        let xml = "<root><a>1</a><b>2</b><c>3</c></root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.find_children("a").len(), 1);
        assert_eq!(elem.find_children("b").len(), 1);
        assert_eq!(elem.find_children("c").len(), 1);
    }

    #[test]
    fn test_parse_empty_element() {
        let xml = "<root></root>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.text_content(), "");
        assert!(elem.children.is_empty());
    }

    #[test]
    fn test_parse_deeply_nested() {
        let xml = "<a><b><c><d>deep</d></c></b></a>";
        let elem = parse_xml(xml).unwrap();
        let d = elem
            .find_child("b")
            .unwrap()
            .find_child("c")
            .unwrap()
            .find_child("d")
            .unwrap();
        assert_eq!(d.text_content(), "deep");
    }

    #[test]
    fn test_parse_mismatched_close_tag() {
        let xml = "<a></b>";
        let result = parse_xml(xml);
        assert!(result.is_err());
        if let Err(XmlError::MismatchedClose { expected, found }) = result {
            assert_eq!(expected, "a");
            assert_eq!(found, "b");
        }
    }

    #[test]
    fn test_parse_empty_input() {
        let result = parse_xml("");
        assert!(result.is_err());
    }

    #[test]
    fn test_find_all_recursive() {
        let xml = "<root><item>1</item><sub><item>2</item></sub></root>";
        let elem = parse_xml(xml).unwrap();
        let items = elem.find_all("item");
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_child_text_returns_none_for_missing() {
        let xml = "<root><a>text</a></root>";
        let elem = parse_xml(xml).unwrap();
        assert!(elem.child_text("missing").is_none());
    }

    #[test]
    fn test_parse_attribute_with_single_quotes() {
        let xml = "<tag attr='value'>text</tag>";
        let elem = parse_xml(xml).unwrap();
        assert_eq!(elem.attr("attr"), Some("value"));
    }

    #[test]
    fn test_xml_element_new() {
        let elem = XmlElement::new("test");
        assert_eq!(elem.tag, "test");
        assert!(elem.attributes.is_empty());
        assert!(elem.children.is_empty());
    }

    // -----------------------------------------------------------------------
    // RSS 2.0 parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_rss2_basic() {
        let xml = r#"<?xml version="1.0"?>
        <rss version="2.0">
          <channel>
            <title>Test Feed</title>
            <link>http://example.com</link>
            <description>A test feed</description>
            <item>
              <title>Article 1</title>
              <link>http://example.com/1</link>
              <description>First article</description>
              <pubDate>Mon, 15 Jan 2024 10:30:00 GMT</pubDate>
            </item>
          </channel>
        </rss>"#;

        let parsed = parse_feed(xml).unwrap();
        assert_eq!(parsed.title, "Test Feed");
        assert_eq!(parsed.link, "http://example.com");
        assert_eq!(parsed.format, FeedFormat::Rss2);
        assert_eq!(parsed.articles.len(), 1);
        assert_eq!(parsed.articles[0].title, "Article 1");
    }

    #[test]
    fn test_parse_rss2_multiple_items() {
        let xml = r#"<rss version="2.0">
          <channel>
            <title>Multi</title>
            <link>http://example.com</link>
            <description>desc</description>
            <item><title>A</title></item>
            <item><title>B</title></item>
            <item><title>C</title></item>
          </channel>
        </rss>"#;

        let parsed = parse_feed(xml).unwrap();
        assert_eq!(parsed.articles.len(), 3);
        assert_eq!(parsed.articles[0].title, "A");
        assert_eq!(parsed.articles[1].title, "B");
        assert_eq!(parsed.articles[2].title, "C");
    }

    #[test]
    fn test_parse_rss2_with_author() {
        let xml = r#"<rss version="2.0">
          <channel>
            <title>Feed</title>
            <item>
              <title>Post</title>
              <author>john@example.com</author>
            </item>
          </channel>
        </rss>"#;

        let parsed = parse_feed(xml).unwrap();
        assert_eq!(parsed.articles[0].author, "john@example.com");
    }

    // -----------------------------------------------------------------------
    // Atom 1.0 parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_atom1_basic() {
        let xml = r#"<?xml version="1.0"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <title>Atom Feed</title>
          <link rel="alternate" href="http://example.com"/>
          <subtitle>An atom feed</subtitle>
          <entry>
            <title>Entry 1</title>
            <link rel="alternate" href="http://example.com/1"/>
            <updated>2024-01-15T10:30:00Z</updated>
            <author><name>Jane</name></author>
            <summary>First entry summary</summary>
          </entry>
        </feed>"#;

        let parsed = parse_feed(xml).unwrap();
        assert_eq!(parsed.title, "Atom Feed");
        assert_eq!(parsed.format, FeedFormat::Atom1);
        assert_eq!(parsed.articles.len(), 1);
        assert_eq!(parsed.articles[0].title, "Entry 1");
        assert_eq!(parsed.articles[0].author, "Jane");
    }

    #[test]
    fn test_parse_atom1_multiple_entries() {
        let xml = r#"<feed>
          <title>Multi Atom</title>
          <entry><title>X</title></entry>
          <entry><title>Y</title></entry>
        </feed>"#;

        let parsed = parse_feed(xml).unwrap();
        assert_eq!(parsed.articles.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Feed format detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_feed_format_detect_rss() {
        assert_eq!(FeedFormat::detect("rss"), FeedFormat::Rss2);
    }

    #[test]
    fn test_feed_format_detect_atom() {
        assert_eq!(FeedFormat::detect("feed"), FeedFormat::Atom1);
    }

    #[test]
    fn test_feed_format_detect_unknown() {
        assert_eq!(FeedFormat::detect("html"), FeedFormat::Unknown);
    }

    #[test]
    fn test_feed_format_labels() {
        assert_eq!(FeedFormat::Rss2.label(), "RSS 2.0");
        assert_eq!(FeedFormat::Atom1.label(), "Atom 1.0");
        assert_eq!(FeedFormat::Unknown.label(), "Unknown");
    }

    #[test]
    fn test_parse_unknown_format_error() {
        let xml = "<html><body>not a feed</body></html>";
        let result = parse_feed(xml);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Date parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_iso8601_date() {
        let ts = parse_date_string("2024-01-15T10:30:00Z").unwrap();
        assert!(ts > 0);
        let formatted = format_timestamp(ts);
        assert!(formatted.contains("2024"));
        assert!(formatted.contains("01"));
        assert!(formatted.contains("15"));
    }

    #[test]
    fn test_parse_rfc822_date() {
        let ts = parse_date_string("Mon, 15 Jan 2024 10:30:00 GMT").unwrap();
        assert!(ts > 0);
    }

    #[test]
    fn test_parse_simple_date() {
        let ts = parse_date_string("2024-01-15").unwrap();
        assert!(ts > 0);
    }

    #[test]
    fn test_parse_invalid_date_returns_none() {
        assert!(parse_date_string("not a date").is_none());
        assert!(parse_date_string("").is_none());
    }

    #[test]
    fn test_format_timestamp_epoch() {
        let formatted = format_timestamp(0);
        assert_eq!(formatted, "1970-01-01 00:00");
    }

    #[test]
    fn test_format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC
        let ts = ymd_hms_to_epoch(2024, 1, 1, 0, 0, 0);
        let formatted = format_timestamp(ts);
        assert!(formatted.starts_with("2024-01-01"));
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-01-01 is day 19723 since epoch
        let ts = ymd_hms_to_epoch(2024, 1, 1, 0, 0, 0);
        let days = ts / 86400;
        let (y, m, d) = days_to_ymd(days);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn test_month_name_to_number() {
        assert_eq!(month_name_to_number("Jan"), Some(1));
        assert_eq!(month_name_to_number("dec"), Some(12));
        assert_eq!(month_name_to_number("xyz"), None);
    }

    #[test]
    fn test_parse_time_components() {
        assert_eq!(parse_time_components("10:30:45"), (10, 30, 45));
        assert_eq!(parse_time_components("23:59"), (23, 59, 0));
        assert_eq!(parse_time_components("12:00:30.123"), (12, 0, 30));
    }

    // -----------------------------------------------------------------------
    // HTML to text conversion tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_html_to_text_strips_tags() {
        assert_eq!(html_to_text("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn test_html_to_text_decodes_entities() {
        assert_eq!(html_to_text("a &amp; b &lt; c"), "a & b < c");
    }

    #[test]
    fn test_html_to_text_collapses_whitespace() {
        assert_eq!(html_to_text("hello    world\n\nfoo"), "hello world foo");
    }

    #[test]
    fn test_html_to_text_empty_input() {
        assert_eq!(html_to_text(""), "");
    }

    #[test]
    fn test_html_to_text_no_html() {
        assert_eq!(html_to_text("plain text"), "plain text");
    }

    #[test]
    fn test_html_to_text_nbsp() {
        assert_eq!(html_to_text("a&nbsp;b"), "a b");
    }

    // -----------------------------------------------------------------------
    // OPML import/export tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_opml_basic() {
        let xml = r#"<?xml version="1.0"?>
        <opml version="2.0">
          <head><title>My Feeds</title></head>
          <body>
            <outline text="Tech">
              <outline text="HN" type="rss" xmlUrl="http://hn.rss" htmlUrl="http://hn.com"/>
            </outline>
            <outline text="Blog" type="rss" xmlUrl="http://blog.rss"/>
          </body>
        </opml>"#;

        let outlines = parse_opml(xml).unwrap();
        assert_eq!(outlines.len(), 2);
        assert_eq!(outlines[0].text, "Tech");
        assert_eq!(outlines[0].children.len(), 1);
        assert_eq!(outlines[0].children[0].text, "HN");
        assert_eq!(
            outlines[0].children[0].xml_url,
            Some("http://hn.rss".to_string())
        );
        assert_eq!(outlines[1].text, "Blog");
    }

    #[test]
    fn test_parse_opml_invalid_root() {
        let xml = "<html><body></body></html>";
        let result = parse_opml(xml);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_opml_round_trip() {
        let feeds = vec![
            Feed::new(1, "Feed A", "http://a.rss"),
            Feed::new(2, "Feed B", "http://b.rss"),
        ];
        let folders: Vec<Folder> = Vec::new();
        let xml = generate_opml("Test", &feeds, &folders);
        assert!(xml.contains("Feed A"));
        assert!(xml.contains("http://a.rss"));

        // Parse it back
        let outlines = parse_opml(&xml).unwrap();
        assert_eq!(outlines.len(), 2);
    }

    #[test]
    fn test_generate_opml_with_folders() {
        let mut feeds = vec![Feed::new(1, "Inside", "http://in.rss")];
        feeds[0].folder_id = Some(10);
        let folders = vec![Folder::new(10, "MyFolder")];
        let xml = generate_opml("Test", &feeds, &folders);
        assert!(xml.contains("MyFolder"));
        assert!(xml.contains("Inside"));
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }

    #[test]
    fn test_escape_xml_no_special_chars() {
        assert_eq!(escape_xml("hello world"), "hello world");
    }

    // -----------------------------------------------------------------------
    // Feed discovery tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_discover_feeds_rss() {
        let html = r#"<html><head>
            <link rel="alternate" type="application/rss+xml" href="/feed.xml" title="My RSS"/>
        </head></html>"#;
        let feeds = discover_feeds(html);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].url, "/feed.xml");
        assert_eq!(feeds[0].title, "My RSS");
    }

    #[test]
    fn test_discover_feeds_atom() {
        let html = r#"<head>
            <link rel="alternate" type="application/atom+xml" href="/atom.xml" title="Atom Feed"/>
        </head>"#;
        let feeds = discover_feeds(html);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].feed_type, "Atom");
    }

    #[test]
    fn test_discover_feeds_none() {
        let html = "<html><head><title>No feeds</title></head></html>";
        let feeds = discover_feeds(html);
        assert!(feeds.is_empty());
    }

    #[test]
    fn test_discover_feeds_multiple() {
        let html = r#"<head>
            <link rel="alternate" type="application/rss+xml" href="/rss"/>
            <link rel="alternate" type="application/atom+xml" href="/atom"/>
        </head>"#;
        let feeds = discover_feeds(html);
        assert_eq!(feeds.len(), 2);
    }

    #[test]
    fn test_extract_attribute() {
        let tag = r#"<link href="http://example.com" rel="alternate">"#;
        assert_eq!(
            extract_attribute(tag, "href"),
            Some("http://example.com".to_string())
        );
        assert_eq!(
            extract_attribute(tag, "rel"),
            Some("alternate".to_string())
        );
    }

    #[test]
    fn test_extract_attribute_missing() {
        let tag = "<link rel=\"alternate\">";
        assert_eq!(extract_attribute(tag, "href"), None);
    }

    // -----------------------------------------------------------------------
    // Search tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_search_articles_by_title() {
        let articles = vec![
            Article::new(1, 1, "Rust Programming"),
            Article::new(2, 1, "Python Basics"),
        ];
        let results = search_articles(&articles, "Rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].article_id, 1);
        assert!(results[0].title_match);
    }

    #[test]
    fn test_search_articles_by_content() {
        let mut article = Article::new(1, 1, "Title");
        article.content = "This discusses Rust programming".to_string();
        let results = search_articles(&[article], "Rust");
        assert_eq!(results.len(), 1);
        assert!(results[0].content_match);
    }

    #[test]
    fn test_search_articles_by_author() {
        let mut article = Article::new(1, 1, "Title");
        article.author = "John Smith".to_string();
        let results = search_articles(&[article], "Smith");
        assert_eq!(results.len(), 1);
        assert!(results[0].author_match);
    }

    #[test]
    fn test_search_articles_empty_query() {
        let articles = vec![Article::new(1, 1, "Test")];
        let results = search_articles(&articles, "");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_articles_no_match() {
        let articles = vec![Article::new(1, 1, "Hello World")];
        let results = search_articles(&articles, "xyz123");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_case_insensitive() {
        let articles = vec![Article::new(1, 1, "RUST Programming")];
        let results = search_articles(&articles, "rust");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_extract_snippet() {
        let text = "The quick brown fox jumps over the lazy dog";
        let snippet = extract_snippet(text, "fox", 10);
        assert!(snippet.contains("fox"));
    }

    #[test]
    fn test_extract_snippet_at_start() {
        let text = "fox jumps over the lazy dog";
        let snippet = extract_snippet(text, "fox", 5);
        assert!(snippet.starts_with("fox"));
    }

    // -----------------------------------------------------------------------
    // Sort order tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sort_order_labels() {
        assert_eq!(SortOrder::DateDesc.label(), "Date (newest first)");
        assert_eq!(SortOrder::TitleAsc.label(), "Title (A-Z)");
    }

    #[test]
    fn test_sort_order_cycle() {
        let initial = SortOrder::DateDesc;
        let next = initial.next();
        assert_eq!(next, SortOrder::DateAsc);
        assert_eq!(SortOrder::FeedNameDesc.next(), SortOrder::DateDesc);
    }

    #[test]
    fn test_sort_order_full_cycle() {
        let start = SortOrder::DateDesc;
        let mut current = start;
        for _ in 0..6 {
            current = current.next();
        }
        assert_eq!(current, start);
    }

    // -----------------------------------------------------------------------
    // Filter mode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_filter_mode_labels() {
        assert_eq!(FilterMode::All.label(), "All Articles");
        assert_eq!(FilterMode::Unread.label(), "Unread Only");
        assert_eq!(FilterMode::Starred.label(), "Starred Only");
    }

    #[test]
    fn test_filter_mode_cycle() {
        assert_eq!(FilterMode::All.next(), FilterMode::Unread);
        assert_eq!(FilterMode::Unread.next(), FilterMode::Starred);
        assert_eq!(FilterMode::Starred.next(), FilterMode::All);
    }

    // -----------------------------------------------------------------------
    // Active pane tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_active_pane_next() {
        assert_eq!(ActivePane::Sidebar.next(), ActivePane::ArticleList);
        assert_eq!(ActivePane::ArticleList.next(), ActivePane::ContentView);
        assert_eq!(ActivePane::ContentView.next(), ActivePane::Sidebar);
    }

    #[test]
    fn test_active_pane_prev() {
        assert_eq!(ActivePane::Sidebar.prev(), ActivePane::ContentView);
        assert_eq!(ActivePane::ArticleList.prev(), ActivePane::Sidebar);
        assert_eq!(ActivePane::ContentView.prev(), ActivePane::ArticleList);
    }

    // -----------------------------------------------------------------------
    // Key action tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_action_descriptions_not_empty() {
        for action in ALL_KEY_ACTIONS {
            assert!(!action.description().is_empty());
        }
    }

    #[test]
    fn test_key_action_hints_not_empty() {
        for action in ALL_KEY_ACTIONS {
            assert!(!action.key_hint().is_empty());
        }
    }

    #[test]
    fn test_all_key_actions_count() {
        assert_eq!(ALL_KEY_ACTIONS.len(), 21);
    }

    // -----------------------------------------------------------------------
    // Offline cache tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cache_basic_operations() {
        let mut cache = OfflineCache::new(1024);
        cache.cache_article(1, "hello", 100);
        assert!(cache.is_cached(1));
        assert_eq!(cache.get_cached(1), Some("hello"));
        assert_eq!(cache.count(), 1);
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = OfflineCache::new(20);
        cache.cache_article(1, "0123456789", 100); // 10 bytes
        cache.cache_article(2, "0123456789", 200); // 10 bytes
        // Should still fit
        assert_eq!(cache.count(), 2);

        cache.cache_article(3, "0123456789", 300); // needs eviction
        assert_eq!(cache.count(), 2); // one was evicted
        assert!(!cache.is_cached(1)); // oldest evicted
        assert!(cache.is_cached(2));
        assert!(cache.is_cached(3));
    }

    #[test]
    fn test_cache_oversized_item_skipped() {
        let mut cache = OfflineCache::new(5);
        cache.cache_article(1, "too long content", 100);
        assert!(!cache.is_cached(1));
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn test_cache_update_existing() {
        let mut cache = OfflineCache::new(1024);
        cache.cache_article(1, "old", 100);
        cache.cache_article(1, "new content", 200);
        assert_eq!(cache.get_cached(1), Some("new content"));
        assert_eq!(cache.count(), 1);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = OfflineCache::new(1024);
        cache.cache_article(1, "a", 100);
        cache.cache_article(2, "b", 200);
        cache.clear();
        assert_eq!(cache.count(), 0);
        assert_eq!(cache.current_size, 0);
    }

    #[test]
    fn test_cache_get_uncached_returns_none() {
        let cache = OfflineCache::new(1024);
        assert!(cache.get_cached(999).is_none());
    }

    // -----------------------------------------------------------------------
    // Feed health tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_feed_health_new_is_not_healthy() {
        // New feeds have no successes, but also no failures
        let health = FeedHealth::new();
        assert!(health.is_healthy()); // zero failures = healthy
    }

    #[test]
    fn test_feed_health_after_success() {
        let mut health = FeedHealth::new();
        health.record_success(1000);
        assert!(health.is_healthy());
        assert_eq!(health.last_success, Some(1000));
        assert_eq!(health.consecutive_failures, 0);
    }

    #[test]
    fn test_feed_health_after_failure() {
        let mut health = FeedHealth::new();
        health.record_failure(1000, "timeout");
        assert!(!health.is_healthy());
        assert_eq!(health.consecutive_failures, 1);
        assert_eq!(health.last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_feed_health_success_clears_failures() {
        let mut health = FeedHealth::new();
        health.record_failure(1000, "error");
        health.record_failure(2000, "error");
        health.record_success(3000);
        assert!(health.is_healthy());
        assert_eq!(health.consecutive_failures, 0);
        assert!(health.last_error.is_none());
    }

    #[test]
    fn test_feed_health_status_text() {
        let mut health = FeedHealth::new();
        assert_eq!(health.status_text(), "Never refreshed");

        health.record_success(1000);
        assert!(health.status_text().starts_with("OK"));

        health.record_failure(2000, "timeout");
        assert!(health.status_text().contains("Error"));
        assert!(health.status_text().contains("timeout"));
    }

    // -----------------------------------------------------------------------
    // Application state tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_app_creation() {
        let app = RssReaderApp::new(1200.0, 800.0);
        assert_eq!(app.width, 1200.0);
        assert_eq!(app.height, 800.0);
        assert!(!app.feeds.is_empty());
        assert!(!app.articles.is_empty());
        assert!(!app.folders.is_empty());
    }

    #[test]
    fn test_app_add_folder() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let initial_count = app.folders.len();
        let id = app.add_folder("Test Folder");
        assert_eq!(app.folders.len(), initial_count + 1);
        assert!(app.folders.iter().any(|f| f.id == id && f.name == "Test Folder"));
    }

    #[test]
    fn test_app_add_feed() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let initial_count = app.feeds.len();
        let id = app.add_feed("New Feed", "http://new.rss", None);
        assert_eq!(app.feeds.len(), initial_count + 1);
        assert!(app.feeds.iter().any(|f| f.id == id));
    }

    #[test]
    fn test_app_remove_feed() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let feed_id = app.feeds[0].id;
        let articles_before = app.articles.len();
        app.remove_feed(feed_id);
        assert!(!app.feeds.iter().any(|f| f.id == feed_id));
        assert!(app.articles.len() < articles_before);
    }

    #[test]
    fn test_app_rename_feed() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let feed_id = app.feeds[0].id;
        app.rename_feed(feed_id, "Renamed Feed");
        let feed = app.feeds.iter().find(|f| f.id == feed_id).unwrap();
        assert_eq!(feed.title, "Renamed Feed");
    }

    #[test]
    fn test_app_remove_folder_with_feeds() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let folder_id = app.folders[0].id;
        let feeds_in_folder = app
            .feeds
            .iter()
            .filter(|f| f.folder_id == Some(folder_id))
            .count();
        assert!(feeds_in_folder > 0);
        app.remove_folder(folder_id, true);
        assert!(!app.folders.iter().any(|f| f.id == folder_id));
        let remaining = app
            .feeds
            .iter()
            .filter(|f| f.folder_id == Some(folder_id))
            .count();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_app_remove_folder_keep_feeds() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let folder_id = app.folders[0].id;
        let feed_ids: Vec<FeedId> = app
            .feeds
            .iter()
            .filter(|f| f.folder_id == Some(folder_id))
            .map(|f| f.id)
            .collect();
        app.remove_folder(folder_id, false);
        // Feeds should still exist but unparented
        for fid in &feed_ids {
            let feed = app.feeds.iter().find(|f| f.id == *fid).unwrap();
            assert_eq!(feed.folder_id, None);
        }
    }

    #[test]
    fn test_app_move_feed_to_folder() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let feed_id = app.feeds[0].id;
        let new_folder = app.add_folder("NewDir");
        app.move_feed_to_folder(feed_id, Some(new_folder));
        let feed = app.feeds.iter().find(|f| f.id == feed_id).unwrap();
        assert_eq!(feed.folder_id, Some(new_folder));
    }

    #[test]
    fn test_app_toggle_read() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let filtered = app.filtered_article_indices();
        let idx = filtered[0];
        let was_read = app.articles[idx].is_read;
        app.toggle_read();
        assert_eq!(app.articles[idx].is_read, !was_read);
    }

    #[test]
    fn test_app_toggle_star() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let filtered = app.filtered_article_indices();
        let idx = filtered[0];
        let was_starred = app.articles[idx].is_starred;
        app.toggle_star();
        assert_eq!(app.articles[idx].is_starred, !was_starred);
    }

    #[test]
    fn test_app_mark_all_read() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        assert!(app.total_unread() > 0);
        app.mark_all_read();
        let filtered = app.filtered_article_indices();
        for &idx in &filtered {
            assert!(app.articles[idx].is_read);
        }
    }

    #[test]
    fn test_app_unread_counts() {
        let app = RssReaderApp::new(800.0, 600.0);
        let total = app.total_unread();
        assert!(total > 0);

        let feed_id = app.feeds[0].id;
        let feed_unread = app.unread_count_for_feed(feed_id);
        assert!(feed_unread <= total);
    }

    #[test]
    fn test_app_starred_count() {
        let app = RssReaderApp::new(800.0, 600.0);
        let starred = app.total_starred();
        assert!(starred > 0);
    }

    #[test]
    fn test_app_folder_unread_count() {
        let app = RssReaderApp::new(800.0, 600.0);
        let folder_id = app.folders[0].id;
        let count = app.unread_count_for_folder(folder_id);
        // Should be non-negative (some folders may have unread articles)
        assert!(count <= app.total_unread());
    }

    #[test]
    fn test_app_next_prev_article() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        assert_eq!(app.selected_article_index, 0);
        app.next_article();
        assert_eq!(app.selected_article_index, 1);
        app.prev_article();
        assert_eq!(app.selected_article_index, 0);
    }

    #[test]
    fn test_app_prev_article_at_zero_stays() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.prev_article();
        assert_eq!(app.selected_article_index, 0);
    }

    #[test]
    fn test_app_selected_article() {
        let app = RssReaderApp::new(800.0, 600.0);
        let article = app.selected_article();
        assert!(article.is_some());
    }

    #[test]
    fn test_app_feed_name() {
        let app = RssReaderApp::new(800.0, 600.0);
        let feed_id = app.feeds[0].id;
        let name = app.feed_name(feed_id);
        assert!(!name.is_empty());
    }

    #[test]
    fn test_app_feed_name_unknown() {
        let app = RssReaderApp::new(800.0, 600.0);
        let name = app.feed_name(99999);
        assert_eq!(name, "Unknown Feed");
    }

    // -----------------------------------------------------------------------
    // Filtering and sorting tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_filter_all_returns_all() {
        let app = RssReaderApp::new(800.0, 600.0);
        let indices = app.filtered_article_indices();
        assert_eq!(indices.len(), app.articles.len());
    }

    #[test]
    fn test_filter_unread() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.filter_mode = FilterMode::Unread;
        let indices = app.filtered_article_indices();
        for &idx in &indices {
            assert!(!app.articles[idx].is_read);
        }
    }

    #[test]
    fn test_filter_starred() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.filter_mode = FilterMode::Starred;
        let indices = app.filtered_article_indices();
        for &idx in &indices {
            assert!(app.articles[idx].is_starred);
        }
    }

    #[test]
    fn test_filter_by_feed() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let feed_id = app.feeds[0].id;
        app.sidebar_selection = SidebarSelection::Feed(feed_id);
        let indices = app.filtered_article_indices();
        for &idx in &indices {
            assert_eq!(app.articles[idx].feed_id, feed_id);
        }
    }

    #[test]
    fn test_filter_by_folder() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let folder_id = app.folders[0].id;
        app.sidebar_selection = SidebarSelection::Folder(folder_id);
        let indices = app.filtered_article_indices();
        let folder_feed_ids: Vec<FeedId> = app
            .feeds
            .iter()
            .filter(|f| f.folder_id == Some(folder_id))
            .map(|f| f.id)
            .collect();
        for &idx in &indices {
            assert!(folder_feed_ids.contains(&app.articles[idx].feed_id));
        }
    }

    #[test]
    fn test_sort_by_date_desc() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.sort_order = SortOrder::DateDesc;
        let indices = app.filtered_article_indices();
        for win in indices.windows(2) {
            assert!(app.articles[win[0]].published >= app.articles[win[1]].published);
        }
    }

    #[test]
    fn test_sort_by_date_asc() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.sort_order = SortOrder::DateAsc;
        let indices = app.filtered_article_indices();
        for win in indices.windows(2) {
            assert!(app.articles[win[0]].published <= app.articles[win[1]].published);
        }
    }

    #[test]
    fn test_sort_by_title_asc() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.sort_order = SortOrder::TitleAsc;
        let indices = app.filtered_article_indices();
        for win in indices.windows(2) {
            assert!(app.articles[win[0]].title <= app.articles[win[1]].title);
        }
    }

    #[test]
    fn test_search_filtering() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.search_query = "Rust".to_string();
        let indices = app.filtered_article_indices();
        for &idx in &indices {
            let a = &app.articles[idx];
            let found = a.title.to_lowercase().contains("rust")
                || a.display_content().to_lowercase().contains("rust")
                || a.author.to_lowercase().contains("rust");
            assert!(found);
        }
    }

    // -----------------------------------------------------------------------
    // OPML import/export integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_app_import_opml() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let opml = r#"<?xml version="1.0"?>
        <opml version="2.0">
          <head><title>Import</title></head>
          <body>
            <outline text="Imported" type="rss" xmlUrl="http://imported.rss"/>
          </body>
        </opml>"#;
        let count = app.import_opml(opml).unwrap();
        assert_eq!(count, 1);
        assert!(app.feeds.iter().any(|f| f.title == "Imported"));
    }

    #[test]
    fn test_app_import_opml_with_folders() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let initial_folders = app.folders.len();
        let opml = r#"<opml version="2.0">
          <head><title>Test</title></head>
          <body>
            <outline text="MyFolder">
              <outline text="Feed1" type="rss" xmlUrl="http://f1.rss"/>
              <outline text="Feed2" type="rss" xmlUrl="http://f2.rss"/>
            </outline>
          </body>
        </opml>"#;
        let count = app.import_opml(opml).unwrap();
        assert_eq!(count, 2);
        assert!(app.folders.len() > initial_folders);
    }

    #[test]
    fn test_app_export_opml() {
        let app = RssReaderApp::new(800.0, 600.0);
        let xml = app.export_opml();
        assert!(xml.contains("<opml"));
        assert!(xml.contains("</opml>"));
        for feed in &app.feeds {
            assert!(xml.contains(&escape_xml(&feed.title)));
        }
    }

    // -----------------------------------------------------------------------
    // Feed ingestion tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ingest_parsed_feed() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let feed_id = app.add_feed("Test", "http://test.rss", None);
        let initial_articles = app.articles.len();

        let parsed = ParsedFeed {
            title: "Updated Title".to_string(),
            link: "http://test.com".to_string(),
            description: "desc".to_string(),
            format: FeedFormat::Rss2,
            articles: vec![ParsedArticle {
                title: "New Article".to_string(),
                link: "http://test.com/1".to_string(),
                author: "Author".to_string(),
                published: 1_700_000_000,
                summary: "summary".to_string(),
                content: "full content".to_string(),
            }],
        };

        app.ingest_parsed_feed(feed_id, &parsed, 1_700_100_000);
        assert_eq!(app.articles.len(), initial_articles + 1);
        let feed = app.feeds.iter().find(|f| f.id == feed_id).unwrap();
        assert!(feed.health.is_healthy());
    }

    #[test]
    fn test_ingest_dedup() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        let feed_id = app.add_feed("Dup", "http://dup.rss", None);

        let parsed = ParsedFeed {
            title: "Dup Feed".to_string(),
            link: "".to_string(),
            description: "".to_string(),
            format: FeedFormat::Rss2,
            articles: vec![ParsedArticle {
                title: "Same Article".to_string(),
                link: "http://dup.com/1".to_string(),
                author: "".to_string(),
                published: 0,
                summary: "".to_string(),
                content: "".to_string(),
            }],
        };

        app.ingest_parsed_feed(feed_id, &parsed, 100);
        let count1 = app.articles.len();
        app.ingest_parsed_feed(feed_id, &parsed, 200);
        assert_eq!(app.articles.len(), count1); // no duplicate added
    }

    // -----------------------------------------------------------------------
    // Rendering tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let app = RssReaderApp::new(1200.0, 800.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
        // Should produce a significant number of commands
        assert!(cmds.len() > 50);
    }

    #[test]
    fn test_render_with_sidebar_hidden() {
        let mut app = RssReaderApp::new(1200.0, 800.0);
        app.sidebar_visible = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_help_overlay() {
        let mut app = RssReaderApp::new(1200.0, 800.0);
        app.show_help = true;
        let cmds = app.render();
        // Should have more commands due to overlay
        let normal = {
            let mut a2 = RssReaderApp::new(1200.0, 800.0);
            a2.show_help = false;
            a2.render().len()
        };
        assert!(cmds.len() > normal);
    }

    #[test]
    fn test_render_with_add_feed_dialog() {
        let mut app = RssReaderApp::new(1200.0, 800.0);
        app.show_add_feed_dialog = true;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_feed_health_overlay() {
        let mut app = RssReaderApp::new(1200.0, 800.0);
        app.show_feed_health = true;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_article_list() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.search_query = "zzzzzz_no_match_ever".to_string();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_no_selected_article() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.articles.clear();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_small_window() {
        let app = RssReaderApp::new(400.0, 300.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Text wrapping tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_wrap_text_short_string() {
        let lines = wrap_text("hello", 200.0, 14.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "hello");
    }

    #[test]
    fn test_wrap_text_wraps_long_string() {
        let long = "word ".repeat(50);
        let lines = wrap_text(long.trim(), 100.0, 14.0);
        assert!(lines.len() > 1);
    }

    #[test]
    fn test_wrap_text_empty_string() {
        let lines = wrap_text("", 200.0, 14.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "");
    }

    #[test]
    fn test_wrap_text_single_long_word() {
        let word = "a".repeat(200);
        let lines = wrap_text(&word, 50.0, 14.0);
        assert!(lines.len() > 1);
    }

    // -----------------------------------------------------------------------
    // Article tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_article_new() {
        let article = Article::new(1, 2, "Test Title");
        assert_eq!(article.id, 1);
        assert_eq!(article.feed_id, 2);
        assert_eq!(article.title, "Test Title");
        assert!(!article.is_read);
        assert!(!article.is_starred);
    }

    #[test]
    fn test_article_display_content_priority() {
        let mut article = Article::new(1, 1, "Test");
        // When cached_text is set, it should be preferred
        article.cached_text = "cached".to_string();
        article.content = "content".to_string();
        article.summary = "summary".to_string();
        assert_eq!(article.display_content(), "cached");

        // When cached_text is empty, content is used
        article.cached_text.clear();
        assert_eq!(article.display_content(), "content");

        // When content is also empty, summary is used
        article.content.clear();
        assert_eq!(article.display_content(), "summary");
    }

    // -----------------------------------------------------------------------
    // Feed tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_feed_new() {
        let feed = Feed::new(1, "My Feed", "http://example.com/feed");
        assert_eq!(feed.id, 1);
        assert_eq!(feed.title, "My Feed");
        assert_eq!(feed.url, "http://example.com/feed");
        assert_eq!(feed.folder_id, None);
        assert_eq!(feed.auto_refresh_seconds, 3600);
    }

    // -----------------------------------------------------------------------
    // Folder tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_folder_new() {
        let folder = Folder::new(1, "My Folder");
        assert_eq!(folder.id, 1);
        assert_eq!(folder.name, "My Folder");
        assert!(folder.is_expanded);
    }

    // -----------------------------------------------------------------------
    // Sidebar selection test
    // -----------------------------------------------------------------------

    #[test]
    fn test_sidebar_selection_starred_filter() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.sidebar_selection = SidebarSelection::Starred;
        let indices = app.filtered_article_indices();
        for &idx in &indices {
            assert!(app.articles[idx].is_starred);
        }
    }

    // -----------------------------------------------------------------------
    // Search integration test
    // -----------------------------------------------------------------------

    #[test]
    fn test_perform_search() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.search_query = "Rust".to_string();
        app.perform_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn test_perform_search_empty() {
        let mut app = RssReaderApp::new(800.0, 600.0);
        app.search_query.clear();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    // -----------------------------------------------------------------------
    // XmlError display test
    // -----------------------------------------------------------------------

    #[test]
    fn test_xml_error_display() {
        let err = XmlError::UnexpectedEof;
        assert_eq!(format!("{err}"), "unexpected end of input");

        let err = XmlError::MalformedTag("bad".to_string());
        assert!(format!("{err}").contains("bad"));

        let err = XmlError::MismatchedClose {
            expected: "a".to_string(),
            found: "b".to_string(),
        };
        assert!(format!("{err}").contains("a"));
        assert!(format!("{err}").contains("b"));
    }
}
