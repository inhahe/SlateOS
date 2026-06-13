//! `SlateOS` Clipboard Service
//!
//! System-wide clipboard manager providing multi-format copy/paste with history.
//! All applications communicate with this service via IPC messages to share
//! clipboard data. The service maintains a ring buffer of recent clips, supports
//! format negotiation, and enforces security policies for sensitive data.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of entries retained in clipboard history.
const HISTORY_CAPACITY: usize = 50;

/// Duration after which sensitive entries are automatically cleared.
const SENSITIVE_EXPIRY: Duration = Duration::from_secs(30);

/// Maximum length of a text preview for UI display.
const PREVIEW_MAX_CHARS: usize = 100;

// ---------------------------------------------------------------------------
// Clipboard Formats
// ---------------------------------------------------------------------------

/// Represents the data format of a clipboard entry.
///
/// Applications provide data in one or more formats when copying. Consumers
/// request their preferred format and the service returns the best available
/// match, falling back to `PlainText` when possible.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClipboardFormat {
    /// UTF-8 plain text.
    PlainText,
    /// Rich text with formatting (RTF-style).
    RichText,
    /// HTML fragment.
    Html,
    /// PNG image data.
    ImagePng,
    /// BMP image data.
    ImageBmp,
    /// List of filesystem paths (newline-separated internally).
    FilePaths,
    /// Application-defined custom format identified by a MIME-like string.
    Custom(String),
}

impl ClipboardFormat {
    /// Returns the category tag for UI display purposes.
    #[must_use]
    pub fn category(&self) -> EntryCategory {
        match self {
            Self::PlainText | Self::RichText | Self::Html => EntryCategory::Text,
            Self::ImagePng | Self::ImageBmp => EntryCategory::Image,
            Self::FilePaths => EntryCategory::Files,
            Self::Custom(_) => EntryCategory::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry Category
// ---------------------------------------------------------------------------

/// High-level category for clipboard manager UI grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCategory {
    /// Text-based content (plain, rich, HTML).
    Text,
    /// Image content (PNG, BMP).
    Image,
    /// File path lists.
    Files,
    /// Custom or unrecognized formats.
    Other,
}

// ---------------------------------------------------------------------------
// Source Application Info
// ---------------------------------------------------------------------------

/// Identifies the application that placed data on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceApp {
    /// Human-readable application name.
    pub name: String,
    /// Process ID of the source application.
    pub pid: u64,
}

// ---------------------------------------------------------------------------
// Clipboard Entry
// ---------------------------------------------------------------------------

/// A single clipboard entry containing data in one or more formats.
///
/// When an application copies data, it provides all formats it can render.
/// The clipboard service stores them together so that consumers can pick
/// the richest format they support.
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    /// When this entry was placed on the clipboard.
    pub timestamp: SystemTime,
    /// Data in each available format. Order represents the source app's
    /// preference (richest format first).
    pub formats: Vec<(ClipboardFormat, Vec<u8>)>,
    /// The application that created this entry.
    pub source: SourceApp,
    /// Whether this entry is pinned (immune to ring-buffer eviction).
    pub pinned: bool,
    /// Whether this entry contains sensitive data (e.g., passwords).
    /// Sensitive entries are excluded from history and auto-cleared.
    pub sensitive: bool,
    /// Total size of all format data in bytes.
    pub total_bytes: usize,
}

impl ClipboardEntry {
    /// Creates a new clipboard entry with the given formats and source.
    #[must_use]
    pub fn new(
        formats: Vec<(ClipboardFormat, Vec<u8>)>,
        source: SourceApp,
        sensitive: bool,
    ) -> Self {
        let total_bytes = formats.iter().map(|(_, data)| data.len()).sum();
        Self {
            timestamp: SystemTime::now(),
            formats,
            source,
            pinned: false,
            sensitive,
            total_bytes,
        }
    }

    /// Returns the data for the requested format, performing basic conversion
    /// if the exact format is not available.
    #[must_use]
    pub fn get_format(&self, requested: &ClipboardFormat) -> Option<Vec<u8>> {
        // Direct match first.
        for (fmt, data) in &self.formats {
            if fmt == requested {
                return Some(data.clone());
            }
        }

        // Attempt conversion to PlainText if that was requested.
        if *requested == ClipboardFormat::PlainText {
            return self.convert_to_plain_text();
        }

        None
    }

    /// Returns a list of all formats available in this entry.
    #[must_use]
    pub fn available_formats(&self) -> Vec<&ClipboardFormat> {
        self.formats.iter().map(|(fmt, _)| fmt).collect()
    }

    /// Generates a short preview string suitable for clipboard manager UI.
    #[must_use]
    pub fn preview(&self) -> String {
        // Try text-based formats first.
        for (fmt, data) in &self.formats {
            match fmt {
                ClipboardFormat::PlainText | ClipboardFormat::RichText => {
                    if let Ok(text) = std::str::from_utf8(data) {
                        let trimmed = text.trim();
                        if trimmed.len() <= PREVIEW_MAX_CHARS {
                            return trimmed.to_string();
                        }
                        // Safe to slice: we find a char boundary at or before PREVIEW_MAX_CHARS.
                        let boundary = find_char_boundary(trimmed, PREVIEW_MAX_CHARS);
                        let mut preview = trimmed[..boundary].to_string();
                        preview.push_str("...");
                        return preview;
                    }
                }
                ClipboardFormat::Html => {
                    if let Ok(html) = std::str::from_utf8(data) {
                        let plain = strip_html_tags(html);
                        let trimmed = plain.trim();
                        if trimmed.len() <= PREVIEW_MAX_CHARS {
                            return trimmed.to_string();
                        }
                        let boundary = find_char_boundary(trimmed, PREVIEW_MAX_CHARS);
                        let mut preview = trimmed[..boundary].to_string();
                        preview.push_str("...");
                        return preview;
                    }
                }
                ClipboardFormat::FilePaths => {
                    if let Ok(paths) = std::str::from_utf8(data) {
                        let count = paths.lines().count();
                        return format!("{count} file(s)");
                    }
                }
                ClipboardFormat::ImagePng | ClipboardFormat::ImageBmp => {
                    return format!("Image ({} bytes)", data.len());
                }
                ClipboardFormat::Custom(mime) => {
                    return format!("Custom: {mime} ({} bytes)", data.len());
                }
            }
        }

        String::from("[empty]")
    }

    /// Returns the primary category of this entry based on its first format.
    #[must_use]
    pub fn category(&self) -> EntryCategory {
        self.formats
            .first()
            .map_or(EntryCategory::Other, |(fmt, _)| fmt.category())
    }

    /// Attempts to convert available data to plain text.
    fn convert_to_plain_text(&self) -> Option<Vec<u8>> {
        for (fmt, data) in &self.formats {
            match fmt {
                ClipboardFormat::Html => {
                    if let Ok(html) = std::str::from_utf8(data) {
                        let plain = strip_html_tags(html);
                        return Some(plain.into_bytes());
                    }
                }
                ClipboardFormat::RichText => {
                    if let Ok(rich) = std::str::from_utf8(data) {
                        let plain = strip_rich_text_formatting(rich);
                        return Some(plain.into_bytes());
                    }
                }
                ClipboardFormat::FilePaths => {
                    // Already newline-separated paths — return as-is.
                    return Some(data.clone());
                }
                _ => {}
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Entry Metadata (for history listing without full data)
// ---------------------------------------------------------------------------

/// Lightweight metadata about a history entry, used for browsing without
/// transferring full clipboard data over IPC.
#[derive(Debug, Clone)]
pub struct EntryMetadata {
    /// Index in the history ring buffer.
    pub index: usize,
    /// When the entry was created.
    pub timestamp: SystemTime,
    /// Short preview string.
    pub preview: String,
    /// Source application name.
    pub source_name: String,
    /// Category tag.
    pub category: EntryCategory,
    /// Total data size in bytes.
    pub total_bytes: usize,
    /// Whether the entry is pinned.
    pub pinned: bool,
}

// ---------------------------------------------------------------------------
// Service Requests & Responses
// ---------------------------------------------------------------------------

/// Requests that applications send to the clipboard service.
#[derive(Debug)]
pub enum ClipboardRequest {
    /// Place data on the clipboard in one or more formats.
    Copy {
        formats: Vec<(ClipboardFormat, Vec<u8>)>,
        source: SourceApp,
        sensitive: bool,
    },
    /// Request clipboard data in a preferred format.
    Paste {
        preferred_format: ClipboardFormat,
    },
    /// Query which formats are available in the current clipboard.
    GetFormats,
    /// List recent history entries (metadata only).
    GetHistory,
    /// Retrieve full data for a specific history entry.
    GetHistoryEntry {
        index: usize,
    },
    /// Pin a history entry to prevent eviction.
    PinEntry {
        index: usize,
    },
    /// Unpin a previously pinned history entry.
    UnpinEntry {
        index: usize,
    },
    /// Remove all non-pinned entries from history.
    ClearHistory,
    /// Search history entries by text content.
    SearchHistory {
        query: String,
    },
    /// Subscribe to clipboard change notifications.
    Subscribe {
        subscriber_pid: u64,
    },
    /// Unsubscribe from clipboard change notifications.
    Unsubscribe {
        subscriber_pid: u64,
    },
}

/// Responses sent back to applications from the clipboard service.
#[derive(Debug)]
pub enum ClipboardResponse {
    /// Copy succeeded.
    CopyOk,
    /// Paste result with data in the requested (or converted) format.
    PasteOk {
        format: ClipboardFormat,
        data: Vec<u8>,
    },
    /// No data available for the requested format.
    PasteEmpty,
    /// List of formats in the current clipboard entry.
    Formats(Vec<ClipboardFormat>),
    /// History listing with metadata.
    History(Vec<EntryMetadata>),
    /// Full data for a single history entry.
    HistoryEntry(ClipboardEntry),
    /// Search results as metadata list.
    SearchResults(Vec<EntryMetadata>),
    /// Generic success acknowledgment.
    Ok,
    /// Error response with description.
    Error(ClipboardError),
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that the clipboard service can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    /// Requested history index is out of range.
    IndexOutOfRange { index: usize, max: usize },
    /// No clipboard content is available.
    ClipboardEmpty,
    /// The requested format is not available and cannot be converted.
    FormatUnavailable(ClipboardFormat),
    /// An internal error occurred.
    Internal(String),
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IndexOutOfRange { index, max } => {
                write!(f, "history index {index} out of range (max {max})")
            }
            Self::ClipboardEmpty => write!(f, "clipboard is empty"),
            Self::FormatUnavailable(fmt) => {
                write!(f, "format {fmt:?} not available and cannot be converted")
            }
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Clipboard Service
// ---------------------------------------------------------------------------

/// The core clipboard service managing state, history, and subscriptions.
///
/// This service runs as a system daemon. Applications communicate with it
/// via IPC (channels). The service maintains the current clipboard content,
/// a history ring buffer, and a subscriber list for change notifications.
pub struct ClipboardService {
    /// The current clipboard entry (most recent copy).
    current: Option<ClipboardEntry>,
    /// History ring buffer of past clipboard entries.
    history: VecDeque<ClipboardEntry>,
    /// PIDs of applications subscribed to change notifications.
    subscribers: Vec<u64>,
}

impl Default for ClipboardService {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardService {
    /// Creates a new clipboard service with empty state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: None,
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
            subscribers: Vec::new(),
        }
    }

    /// Handles an incoming request and produces a response.
    pub fn handle_request(&mut self, request: ClipboardRequest) -> ClipboardResponse {
        match request {
            ClipboardRequest::Copy {
                formats,
                source,
                sensitive,
            } => self.handle_copy(formats, source, sensitive),
            ClipboardRequest::Paste { preferred_format } => self.handle_paste(&preferred_format),
            ClipboardRequest::GetFormats => self.handle_get_formats(),
            ClipboardRequest::GetHistory => self.handle_get_history(),
            ClipboardRequest::GetHistoryEntry { index } => self.handle_get_history_entry(index),
            ClipboardRequest::PinEntry { index } => self.handle_pin(index, true),
            ClipboardRequest::UnpinEntry { index } => self.handle_pin(index, false),
            ClipboardRequest::ClearHistory => self.handle_clear_history(),
            ClipboardRequest::SearchHistory { query } => self.handle_search(&query),
            ClipboardRequest::Subscribe { subscriber_pid } => {
                self.handle_subscribe(subscriber_pid)
            }
            ClipboardRequest::Unsubscribe { subscriber_pid } => {
                self.handle_unsubscribe(subscriber_pid)
            }
        }
    }

    /// Performs periodic maintenance: expires sensitive entries, enforces capacity.
    pub fn tick(&mut self) {
        self.expire_sensitive_entries();
    }

    /// Returns the list of subscriber PIDs that should be notified of changes.
    #[must_use]
    pub fn pending_notifications(&self) -> &[u64] {
        &self.subscribers
    }

    // -----------------------------------------------------------------------
    // Request handlers
    // -----------------------------------------------------------------------

    fn handle_copy(
        &mut self,
        formats: Vec<(ClipboardFormat, Vec<u8>)>,
        source: SourceApp,
        sensitive: bool,
    ) -> ClipboardResponse {
        let entry = ClipboardEntry::new(formats, source, sensitive);

        // Add to history only if not sensitive.
        if !sensitive {
            self.push_history(entry.clone());
        }

        self.current = Some(entry);
        ClipboardResponse::CopyOk
    }

    fn handle_paste(&self, preferred: &ClipboardFormat) -> ClipboardResponse {
        let Some(entry) = &self.current else {
            return ClipboardResponse::PasteEmpty;
        };

        match entry.get_format(preferred) {
            Some(data) => ClipboardResponse::PasteOk {
                format: preferred.clone(),
                data,
            },
            None => ClipboardResponse::PasteEmpty,
        }
    }

    fn handle_get_formats(&self) -> ClipboardResponse {
        let Some(entry) = &self.current else {
            return ClipboardResponse::Formats(Vec::new());
        };

        let formats = entry
            .available_formats()
            .into_iter()
            .cloned()
            .collect();
        ClipboardResponse::Formats(formats)
    }

    fn handle_get_history(&self) -> ClipboardResponse {
        let metadata: Vec<EntryMetadata> = self
            .history
            .iter()
            .enumerate()
            .map(|(index, entry)| EntryMetadata {
                index,
                timestamp: entry.timestamp,
                preview: entry.preview(),
                source_name: entry.source.name.clone(),
                category: entry.category(),
                total_bytes: entry.total_bytes,
                pinned: entry.pinned,
            })
            .collect();
        ClipboardResponse::History(metadata)
    }

    fn handle_get_history_entry(&self, index: usize) -> ClipboardResponse {
        if let Some(entry) = self.history.get(index) {
            ClipboardResponse::HistoryEntry(entry.clone())
        } else {
            ClipboardResponse::Error(self.index_error(index))
        }
    }

    fn handle_pin(&mut self, index: usize, pin: bool) -> ClipboardResponse {
        if let Some(entry) = self.history.get_mut(index) {
            entry.pinned = pin;
            ClipboardResponse::Ok
        } else {
            ClipboardResponse::Error(self.index_error(index))
        }
    }

    /// Constructs an `IndexOutOfRange` error for the current history size.
    fn index_error(&self, index: usize) -> ClipboardError {
        let max = if self.history.is_empty() {
            0
        } else {
            self.history.len().saturating_sub(1)
        };
        ClipboardError::IndexOutOfRange { index, max }
    }

    fn handle_clear_history(&mut self) -> ClipboardResponse {
        // Retain only pinned entries.
        self.history.retain(|entry| entry.pinned);
        ClipboardResponse::Ok
    }

    fn handle_search(&self, query: &str) -> ClipboardResponse {
        let query_lower = query.to_lowercase();
        let results: Vec<EntryMetadata> = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                // Search through text-based formats.
                entry.formats.iter().any(|(fmt, data)| {
                    matches!(
                        fmt,
                        ClipboardFormat::PlainText
                            | ClipboardFormat::RichText
                            | ClipboardFormat::Html
                            | ClipboardFormat::FilePaths
                    ) && std::str::from_utf8(data)
                        .is_ok_and(|text| text.to_lowercase().contains(&query_lower))
                })
            })
            .map(|(index, entry)| EntryMetadata {
                index,
                timestamp: entry.timestamp,
                preview: entry.preview(),
                source_name: entry.source.name.clone(),
                category: entry.category(),
                total_bytes: entry.total_bytes,
                pinned: entry.pinned,
            })
            .collect();
        ClipboardResponse::SearchResults(results)
    }

    fn handle_subscribe(&mut self, pid: u64) -> ClipboardResponse {
        if !self.subscribers.contains(&pid) {
            self.subscribers.push(pid);
        }
        ClipboardResponse::Ok
    }

    fn handle_unsubscribe(&mut self, pid: u64) -> ClipboardResponse {
        self.subscribers.retain(|&p| p != pid);
        ClipboardResponse::Ok
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Pushes an entry to the history ring buffer, evicting the oldest
    /// non-pinned entry if at capacity.
    fn push_history(&mut self, entry: ClipboardEntry) {
        if self.history.len() >= HISTORY_CAPACITY {
            // Find and remove the oldest non-pinned entry.
            let evict_idx = self
                .history
                .iter()
                .position(|e| !e.pinned);

            if let Some(idx) = evict_idx {
                self.history.remove(idx);
            } else {
                // All entries are pinned; drop the oldest pinned entry as last resort.
                self.history.pop_front();
            }
        }
        self.history.push_back(entry);
    }

    /// Removes sensitive entries that have exceeded their expiry duration.
    fn expire_sensitive_entries(&mut self) {
        let now = SystemTime::now();

        // Check and clear the current entry if it is sensitive and expired.
        if let Some(entry) = &self.current
            && entry.sensitive
        {
            let elapsed = now.duration_since(entry.timestamp).unwrap_or(Duration::ZERO);
            if elapsed >= SENSITIVE_EXPIRY {
                self.current = None;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Format Conversion Utilities
// ---------------------------------------------------------------------------

/// Strips HTML tags from a string, returning only the text content.
///
/// This is a basic implementation that handles common cases. It does not
/// parse malformed HTML or handle CDATA sections.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity_buf = String::new();

    for ch in html.chars() {
        if in_entity {
            entity_buf.push(ch);
            if ch == ';' {
                // Resolve common HTML entities.
                let resolved = resolve_html_entity(&entity_buf);
                result.push_str(&resolved);
                entity_buf.clear();
                in_entity = false;
            }
            // Abandon entity parsing if it gets too long (malformed).
            if entity_buf.len() > 10 {
                result.push('&');
                result.push_str(&entity_buf);
                entity_buf.clear();
                in_entity = false;
            }
        } else if in_tag {
            if ch == '>' {
                in_tag = false;
            }
        } else if ch == '<' {
            in_tag = true;
        } else if ch == '&' {
            in_entity = true;
            entity_buf.clear();
        } else {
            result.push(ch);
        }
    }

    // If we ended mid-entity, flush it.
    if in_entity {
        result.push('&');
        result.push_str(&entity_buf);
    }

    result
}

/// Resolves common HTML character entities to their text equivalents.
fn resolve_html_entity(entity: &str) -> String {
    match entity {
        "amp;" => "&".to_string(),
        "lt;" => "<".to_string(),
        "gt;" => ">".to_string(),
        "quot;" => "\"".to_string(),
        "apos;" => "'".to_string(),
        "nbsp;" => " ".to_string(),
        _ => {
            // Numeric entities: &#123; or &#x1F;
            if let Some(rest) = entity.strip_prefix('#') {
                let code_str = rest.strip_suffix(';').unwrap_or(rest);
                let code = if let Some(hex_str) = code_str.strip_prefix('x') {
                    u32::from_str_radix(hex_str, 16).ok()
                } else {
                    code_str.parse::<u32>().ok()
                };
                if let Some(c) = code.and_then(char::from_u32) {
                    return c.to_string();
                }
            }
            // Unknown entity — return as-is with ampersand.
            let mut s = String::from("&");
            s.push_str(entity);
            s
        }
    }
}

/// Strips basic rich text formatting markers, returning plain text.
///
/// Handles a simplified RTF-like format: removes `{\rtf...}` wrappers
/// and common control words. For full RTF parsing a dedicated library
/// would be needed; this covers the common case of lightly formatted text.
fn strip_rich_text_formatting(rich: &str) -> String {
    let mut result = String::with_capacity(rich.len());
    let mut chars = rich.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                // Skip RTF control words (e.g., \b, \i, \par).
                // Consume alphanumeric characters after the backslash.
                let mut control_word = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() {
                        control_word.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // \par maps to newline.
                if control_word == "par" || control_word == "line" {
                    result.push('\n');
                }
                // Skip optional trailing space after control word.
                if let Some(&' ') = chars.peek() {
                    chars.next();
                }
            }
            '{' | '}' => {
                // Skip RTF group delimiters.
            }
            _ => {
                result.push(ch);
            }
        }
    }

    result
}

/// Finds the last valid UTF-8 character boundary at or before `max_bytes`.
fn find_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    boundary
}

// ---------------------------------------------------------------------------
// Service Event Loop (main entry point)
// ---------------------------------------------------------------------------

fn main() {
    // Initialize the clipboard service.
    let mut service = ClipboardService::new();

    // In the real system, this would register with the service manager,
    // open an IPC endpoint, and enter an event loop dispatching requests.
    // For now, we perform a basic self-test to verify the service works.
    run_self_test(&mut service);

    // TODO: Register with service manager via IPC.
    // TODO: Open clipboard service channel endpoint.
    // TODO: Enter async event loop: select on IPC messages + timer ticks.
    //
    // Pseudocode for the real event loop:
    //
    // loop {
    //     match select(ipc_channel, tick_timer) {
    //         Event::IpcMessage(msg) => {
    //             let request = deserialize_request(msg);
    //             let response = service.handle_request(request);
    //             send_response(response);
    //             if matches!(response, ClipboardResponse::CopyOk) {
    //                 notify_subscribers(&service);
    //             }
    //         }
    //         Event::Tick => {
    //             service.tick();
    //         }
    //     }
    // }
}

/// Performs a basic self-test of clipboard operations during early boot.
/// This validates the service logic before entering the IPC event loop.
fn run_self_test(service: &mut ClipboardService) {
    // Test basic copy/paste cycle.
    let copy_request = ClipboardRequest::Copy {
        formats: vec![
            (
                ClipboardFormat::PlainText,
                b"Hello, SlateOS!".to_vec(),
            ),
            (
                ClipboardFormat::Html,
                b"<b>Hello</b>, SlateOS!".to_vec(),
            ),
        ],
        source: SourceApp {
            name: String::from("self-test"),
            pid: 0,
        },
        sensitive: false,
    };

    let response = service.handle_request(copy_request);
    assert!(matches!(response, ClipboardResponse::CopyOk));

    let paste_request = ClipboardRequest::Paste {
        preferred_format: ClipboardFormat::PlainText,
    };
    let response = service.handle_request(paste_request);
    if let ClipboardResponse::PasteOk { data, .. } = response {
        assert_eq!(data, b"Hello, SlateOS!");
    }
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn make_source(name: &str) -> SourceApp {
        SourceApp {
            name: name.to_string(),
            pid: 1000,
        }
    }

    fn text_entry(text: &str) -> Vec<(ClipboardFormat, Vec<u8>)> {
        vec![(ClipboardFormat::PlainText, text.as_bytes().to_vec())]
    }

    #[test]
    fn test_copy_and_paste_plain_text() {
        let mut svc = ClipboardService::new();
        let resp = svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("test data"),
            source: make_source("editor"),
            sensitive: false,
        });
        assert!(matches!(resp, ClipboardResponse::CopyOk));

        let resp = svc.handle_request(ClipboardRequest::Paste {
            preferred_format: ClipboardFormat::PlainText,
        });
        match resp {
            ClipboardResponse::PasteOk { format, data } => {
                assert_eq!(format, ClipboardFormat::PlainText);
                assert_eq!(data, b"test data");
            }
            _ => panic!("Expected PasteOk"),
        }
    }

    #[test]
    fn test_paste_empty_clipboard() {
        let mut svc = ClipboardService::new();
        let resp = svc.handle_request(ClipboardRequest::Paste {
            preferred_format: ClipboardFormat::PlainText,
        });
        assert!(matches!(resp, ClipboardResponse::PasteEmpty));
    }

    #[test]
    fn test_multi_format_copy() {
        let mut svc = ClipboardService::new();
        let formats = vec![
            (ClipboardFormat::Html, b"<b>bold</b>".to_vec()),
            (ClipboardFormat::PlainText, b"bold".to_vec()),
        ];
        svc.handle_request(ClipboardRequest::Copy {
            formats,
            source: make_source("browser"),
            sensitive: false,
        });

        // Request HTML — should get it directly.
        let resp = svc.handle_request(ClipboardRequest::Paste {
            preferred_format: ClipboardFormat::Html,
        });
        match resp {
            ClipboardResponse::PasteOk { data, .. } => {
                assert_eq!(data, b"<b>bold</b>");
            }
            _ => panic!("Expected PasteOk for HTML"),
        }

        // Request PlainText — should get it directly.
        let resp = svc.handle_request(ClipboardRequest::Paste {
            preferred_format: ClipboardFormat::PlainText,
        });
        match resp {
            ClipboardResponse::PasteOk { data, .. } => {
                assert_eq!(data, b"bold");
            }
            _ => panic!("Expected PasteOk for PlainText"),
        }
    }

    #[test]
    fn test_format_conversion_html_to_plain() {
        let mut svc = ClipboardService::new();
        let formats = vec![(
            ClipboardFormat::Html,
            b"<p>Hello &amp; <b>world</b></p>".to_vec(),
        )];
        svc.handle_request(ClipboardRequest::Copy {
            formats,
            source: make_source("web"),
            sensitive: false,
        });

        let resp = svc.handle_request(ClipboardRequest::Paste {
            preferred_format: ClipboardFormat::PlainText,
        });
        match resp {
            ClipboardResponse::PasteOk { data, .. } => {
                let text = String::from_utf8(data).unwrap();
                assert_eq!(text, "Hello & world");
            }
            _ => panic!("Expected conversion to PlainText"),
        }
    }

    #[test]
    fn test_format_conversion_file_paths_to_plain() {
        let mut svc = ClipboardService::new();
        let paths = b"/home/user/file1.txt\n/home/user/file2.txt".to_vec();
        let formats = vec![(ClipboardFormat::FilePaths, paths.clone())];
        svc.handle_request(ClipboardRequest::Copy {
            formats,
            source: make_source("explorer"),
            sensitive: false,
        });

        let resp = svc.handle_request(ClipboardRequest::Paste {
            preferred_format: ClipboardFormat::PlainText,
        });
        match resp {
            ClipboardResponse::PasteOk { data, .. } => {
                assert_eq!(data, paths);
            }
            _ => panic!("Expected FilePaths to PlainText conversion"),
        }
    }

    #[test]
    fn test_get_formats() {
        let mut svc = ClipboardService::new();
        let formats = vec![
            (ClipboardFormat::Html, b"<b>hi</b>".to_vec()),
            (ClipboardFormat::PlainText, b"hi".to_vec()),
            (ClipboardFormat::ImagePng, vec![0x89, 0x50, 0x4E, 0x47]),
        ];
        svc.handle_request(ClipboardRequest::Copy {
            formats,
            source: make_source("app"),
            sensitive: false,
        });

        let resp = svc.handle_request(ClipboardRequest::GetFormats);
        match resp {
            ClipboardResponse::Formats(fmts) => {
                assert_eq!(fmts.len(), 3);
                assert!(fmts.contains(&ClipboardFormat::Html));
                assert!(fmts.contains(&ClipboardFormat::PlainText));
                assert!(fmts.contains(&ClipboardFormat::ImagePng));
            }
            _ => panic!("Expected Formats response"),
        }
    }

    #[test]
    fn test_history_records_non_sensitive() {
        let mut svc = ClipboardService::new();
        for i in 0..5 {
            svc.handle_request(ClipboardRequest::Copy {
                formats: text_entry(&format!("entry {i}")),
                source: make_source("app"),
                sensitive: false,
            });
        }

        let resp = svc.handle_request(ClipboardRequest::GetHistory);
        match resp {
            ClipboardResponse::History(entries) => {
                assert_eq!(entries.len(), 5);
            }
            _ => panic!("Expected History response"),
        }
    }

    #[test]
    fn test_sensitive_entries_excluded_from_history() {
        let mut svc = ClipboardService::new();
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("password123"),
            source: make_source("password-manager"),
            sensitive: true,
        });

        let resp = svc.handle_request(ClipboardRequest::GetHistory);
        match resp {
            ClipboardResponse::History(entries) => {
                assert!(entries.is_empty());
            }
            _ => panic!("Expected empty History"),
        }

        // But it should still be pasteable.
        let resp = svc.handle_request(ClipboardRequest::Paste {
            preferred_format: ClipboardFormat::PlainText,
        });
        assert!(matches!(resp, ClipboardResponse::PasteOk { .. }));
    }

    #[test]
    fn test_history_capacity_eviction() {
        let mut svc = ClipboardService::new();
        for i in 0..HISTORY_CAPACITY + 10 {
            svc.handle_request(ClipboardRequest::Copy {
                formats: text_entry(&format!("item {i}")),
                source: make_source("app"),
                sensitive: false,
            });
        }

        let resp = svc.handle_request(ClipboardRequest::GetHistory);
        match resp {
            ClipboardResponse::History(entries) => {
                assert_eq!(entries.len(), HISTORY_CAPACITY);
            }
            _ => panic!("Expected History response"),
        }
    }

    #[test]
    fn test_pin_prevents_eviction() {
        let mut svc = ClipboardService::new();

        // Add one entry and pin it.
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("pinned item"),
            source: make_source("app"),
            sensitive: false,
        });
        svc.handle_request(ClipboardRequest::PinEntry { index: 0 });

        // Fill history to capacity.
        for i in 0..HISTORY_CAPACITY + 5 {
            svc.handle_request(ClipboardRequest::Copy {
                formats: text_entry(&format!("filler {i}")),
                source: make_source("app"),
                sensitive: false,
            });
        }

        // The pinned entry should still be in history.
        let resp = svc.handle_request(ClipboardRequest::GetHistory);
        match resp {
            ClipboardResponse::History(entries) => {
                let has_pinned = entries.iter().any(|e| e.preview == "pinned item");
                assert!(has_pinned, "Pinned entry should survive eviction");
            }
            _ => panic!("Expected History response"),
        }
    }

    #[test]
    fn test_clear_history_retains_pinned() {
        let mut svc = ClipboardService::new();
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("keep me"),
            source: make_source("app"),
            sensitive: false,
        });
        svc.handle_request(ClipboardRequest::PinEntry { index: 0 });

        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("remove me"),
            source: make_source("app"),
            sensitive: false,
        });

        svc.handle_request(ClipboardRequest::ClearHistory);

        let resp = svc.handle_request(ClipboardRequest::GetHistory);
        match resp {
            ClipboardResponse::History(entries) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].preview, "keep me");
            }
            _ => panic!("Expected History"),
        }
    }

    #[test]
    fn test_search_history() {
        let mut svc = ClipboardService::new();
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("apple pie recipe"),
            source: make_source("notes"),
            sensitive: false,
        });
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("banana bread recipe"),
            source: make_source("notes"),
            sensitive: false,
        });
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("cherry tart"),
            source: make_source("notes"),
            sensitive: false,
        });

        let resp = svc.handle_request(ClipboardRequest::SearchHistory {
            query: String::from("recipe"),
        });
        match resp {
            ClipboardResponse::SearchResults(results) => {
                assert_eq!(results.len(), 2);
            }
            _ => panic!("Expected SearchResults"),
        }
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut svc = ClipboardService::new();
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("Hello World"),
            source: make_source("app"),
            sensitive: false,
        });

        let resp = svc.handle_request(ClipboardRequest::SearchHistory {
            query: String::from("hello"),
        });
        match resp {
            ClipboardResponse::SearchResults(results) => {
                assert_eq!(results.len(), 1);
            }
            _ => panic!("Expected SearchResults"),
        }
    }

    #[test]
    fn test_subscribe_and_unsubscribe() {
        let mut svc = ClipboardService::new();
        svc.handle_request(ClipboardRequest::Subscribe {
            subscriber_pid: 42,
        });
        svc.handle_request(ClipboardRequest::Subscribe {
            subscriber_pid: 43,
        });
        assert_eq!(svc.pending_notifications().len(), 2);

        // Duplicate subscribe should not add twice.
        svc.handle_request(ClipboardRequest::Subscribe {
            subscriber_pid: 42,
        });
        assert_eq!(svc.pending_notifications().len(), 2);

        svc.handle_request(ClipboardRequest::Unsubscribe {
            subscriber_pid: 42,
        });
        assert_eq!(svc.pending_notifications().len(), 1);
        assert_eq!(svc.pending_notifications()[0], 43);
    }

    #[test]
    fn test_get_history_entry_valid_index() {
        let mut svc = ClipboardService::new();
        svc.handle_request(ClipboardRequest::Copy {
            formats: text_entry("specific entry"),
            source: make_source("editor"),
            sensitive: false,
        });

        let resp = svc.handle_request(ClipboardRequest::GetHistoryEntry { index: 0 });
        match resp {
            ClipboardResponse::HistoryEntry(entry) => {
                assert_eq!(entry.source.name, "editor");
            }
            _ => panic!("Expected HistoryEntry"),
        }
    }

    #[test]
    fn test_get_history_entry_invalid_index() {
        let mut svc = ClipboardService::new();
        let resp = svc.handle_request(ClipboardRequest::GetHistoryEntry { index: 99 });
        match resp {
            ClipboardResponse::Error(ClipboardError::IndexOutOfRange { index, .. }) => {
                assert_eq!(index, 99);
            }
            _ => panic!("Expected IndexOutOfRange error"),
        }
    }

    #[test]
    fn test_preview_plain_text_short() {
        let entry = ClipboardEntry::new(
            text_entry("short text"),
            make_source("app"),
            false,
        );
        assert_eq!(entry.preview(), "short text");
    }

    #[test]
    fn test_preview_plain_text_long() {
        let long_text = "a".repeat(200);
        let entry = ClipboardEntry::new(
            text_entry(&long_text),
            make_source("app"),
            false,
        );
        let preview = entry.preview();
        assert!(preview.ends_with("..."));
        assert!(preview.len() <= PREVIEW_MAX_CHARS + 3);
    }

    #[test]
    fn test_preview_file_paths() {
        let paths = "/a/b/c\n/d/e/f\n/g/h/i";
        let entry = ClipboardEntry::new(
            vec![(ClipboardFormat::FilePaths, paths.as_bytes().to_vec())],
            make_source("explorer"),
            false,
        );
        assert_eq!(entry.preview(), "3 file(s)");
    }

    #[test]
    fn test_preview_image() {
        let entry = ClipboardEntry::new(
            vec![(ClipboardFormat::ImagePng, vec![0; 1024])],
            make_source("paint"),
            false,
        );
        assert_eq!(entry.preview(), "Image (1024 bytes)");
    }

    #[test]
    fn test_strip_html_tags_basic() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
        assert_eq!(strip_html_tags("<b>bold</b> text"), "bold text");
        assert_eq!(strip_html_tags("no tags here"), "no tags here");
    }

    #[test]
    fn test_strip_html_entities() {
        assert_eq!(strip_html_tags("&amp;"), "&");
        assert_eq!(strip_html_tags("&lt;tag&gt;"), "<tag>");
        assert_eq!(strip_html_tags("&#65;"), "A");
        assert_eq!(strip_html_tags("&#x41;"), "A");
    }

    #[test]
    fn test_strip_rich_text() {
        assert_eq!(strip_rich_text_formatting("plain text"), "plain text");
        assert_eq!(
            strip_rich_text_formatting("\\b bold\\b0 normal"),
            "boldnormal"
        );
        assert_eq!(
            strip_rich_text_formatting("line one\\par line two"),
            "line one\nline two"
        );
    }

    #[test]
    fn test_entry_category() {
        let text_entry_val = ClipboardEntry::new(
            text_entry("hi"),
            make_source("app"),
            false,
        );
        assert_eq!(text_entry_val.category(), EntryCategory::Text);

        let img_entry = ClipboardEntry::new(
            vec![(ClipboardFormat::ImagePng, vec![0])],
            make_source("app"),
            false,
        );
        assert_eq!(img_entry.category(), EntryCategory::Image);

        let file_entry = ClipboardEntry::new(
            vec![(ClipboardFormat::FilePaths, b"/tmp/x".to_vec())],
            make_source("app"),
            false,
        );
        assert_eq!(file_entry.category(), EntryCategory::Files);
    }

    #[test]
    fn test_total_bytes_tracking() {
        let formats = vec![
            (ClipboardFormat::PlainText, vec![0u8; 100]),
            (ClipboardFormat::Html, vec![0u8; 200]),
        ];
        let entry = ClipboardEntry::new(formats, make_source("app"), false);
        assert_eq!(entry.total_bytes, 300);
    }

    #[test]
    fn test_find_char_boundary() {
        let s = "hello";
        assert_eq!(find_char_boundary(s, 3), 3);
        assert_eq!(find_char_boundary(s, 100), 5);

        // Multi-byte character: e-acute is 2 bytes in UTF-8.
        let s = "caf\u{00e9}!";
        // "caf" = 3 bytes, e-acute = 2 bytes, "!" = 1 byte → total 6 bytes.
        // Asking for boundary at 4 should land at 3 (before the multi-byte char)
        // since byte 4 is in the middle of the e-acute.
        let boundary = find_char_boundary(s, 4);
        assert!(s.is_char_boundary(boundary));
    }
}
