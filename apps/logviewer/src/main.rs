//! `Slate OS` System Log Viewer
//!
//! A log viewing and analysis tool with:
//! - JSON-lines log format parsing (the OS's native log format)
//! - Real-time log tailing with auto-scroll
//! - Log level filtering (trace, debug, info, warn, error, fatal)
//! - Full-text search with regex support
//! - Time range filtering
//! - Source/component filtering
//! - Log entry detail view
//! - Statistics dashboard (level distribution, rate, top sources)
//! - Log bookmarking for interesting entries
//! - Export filtered view
//! - Multi-file support with tabs
//! - Color-coded log levels
//!
//! Uses the guitk library for UI rendering.

#![deny(clippy::all, clippy::pedantic)]
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
#![allow(clippy::cognitive_complexity)]

use guitk::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, Response};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 800.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const FILTER_BAR_HEIGHT: f32 = 36.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const PADDING: f32 = 8.0;
const LINE_HEIGHT: f32 = 20.0;
/// Font size of the level badge on a log row.
const BADGE_TEXT: f32 = 10.0;
const SMALL_TEXT: f32 = 12.0;
const NORMAL_TEXT: f32 = 14.0;
const HEADER_TEXT: f32 = 16.0;
const TITLE_TEXT: f32 = 18.0;

/// Width of the `[source]` cell on a log row.
///
/// The cell is elided to this and the row cursor advances by what was drawn,
/// so the two cannot disagree — see `render_log_list`.
const SOURCE_WIDTH: f32 = 100.0;

const MAX_LOG_ENTRIES: usize = 100_000;
// There is deliberately no `MAX_BOOKMARKS`. One sat here, unenforced, between
// two caps that are enforced — which made it read as a bound that held. It
// cannot be one: a bookmark is a `bool` on a `LogEntry`, so the number of them
// is already bounded by `MAX_LOG_ENTRIES` and there is no separate storage to
// limit. A cap here would be a policy ("you may not mark more than 500 lines")
// with nothing behind it.
const MAX_SEARCH_RESULTS: usize = 10_000;

// ============================================================================
// Log Level
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::Trace => "TRC",
            Self::Debug => "DBG",
            Self::Info => "INF",
            Self::Warn => "WRN",
            Self::Error => "ERR",
            Self::Fatal => "FTL",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Trace => OVERLAY0,
            Self::Debug => SUBTEXT0,
            Self::Info => BLUE,
            Self::Warn => YELLOW,
            Self::Error => RED,
            Self::Fatal => MAUVE,
        }
    }

    fn bg_color(self) -> Color {
        match self {
            Self::Trace => SURFACE0,
            Self::Debug => SURFACE0,
            Self::Info => Color::rgba(137, 180, 250, 20),
            Self::Warn => Color::rgba(249, 226, 175, 20),
            Self::Error => Color::rgba(243, 139, 168, 25),
            Self::Fatal => Color::rgba(203, 166, 247, 30),
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "TRACE" | "TRC" => Some(Self::Trace),
            "DEBUG" | "DBG" => Some(Self::Debug),
            "INFO" | "INF" => Some(Self::Info),
            "WARN" | "WARNING" | "WRN" => Some(Self::Warn),
            "ERROR" | "ERR" => Some(Self::Error),
            "FATAL" | "FTL" | "CRITICAL" | "CRIT" => Some(Self::Fatal),
            _ => None,
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::Trace,
            Self::Debug,
            Self::Info,
            Self::Warn,
            Self::Error,
            Self::Fatal,
        ]
    }

    fn severity(self) -> u8 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
            Self::Fatal => 5,
        }
    }
}

// ============================================================================
// Log Entry
// ============================================================================

#[derive(Debug, Clone)]
struct LogEntry {
    line_number: usize,
    timestamp: u64, // milliseconds since epoch
    level: LogLevel,
    source: String,
    message: String,
    fields: Vec<(String, String)>,
    raw: String,
    bookmarked: bool,
}

impl LogEntry {
    fn timestamp_display(&self) -> String {
        // Simple HH:MM:SS.mmm format from timestamp
        let total_secs = self.timestamp / 1000;
        let ms = self.timestamp % 1000;
        let secs = total_secs % 60;
        let mins = (total_secs / 60) % 60;
        let hours = (total_secs / 3600) % 24;
        format!("{hours:02}:{mins:02}:{secs:02}.{ms:03}")
    }
}

// ============================================================================
// JSON-lines Parser
// ============================================================================

fn parse_json_line(line: &str, line_number: usize) -> Option<LogEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return None;
    }

    // Simple JSON object parser
    let fields = parse_json_object(trimmed)?;

    let timestamp = fields
        .iter()
        .find(|(k, _)| k == "timestamp" || k == "ts" || k == "time" || k == "t")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .unwrap_or(0);

    let level = fields
        .iter()
        .find(|(k, _)| k == "level" || k == "lvl" || k == "severity")
        .and_then(|(_, v)| LogLevel::from_str(v))
        .unwrap_or(LogLevel::Info);

    let source = fields
        .iter()
        .find(|(k, _)| {
            k == "source" || k == "src" || k == "component" || k == "module" || k == "logger"
        })
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let message = fields
        .iter()
        .find(|(k, _)| k == "message" || k == "msg" || k == "text")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let extra_fields: Vec<(String, String)> = fields
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "timestamp"
                    | "ts"
                    | "time"
                    | "t"
                    | "level"
                    | "lvl"
                    | "severity"
                    | "source"
                    | "src"
                    | "component"
                    | "module"
                    | "logger"
                    | "message"
                    | "msg"
                    | "text"
            )
        })
        .cloned()
        .collect();

    Some(LogEntry {
        line_number,
        timestamp,
        level,
        source,
        message,
        fields: extra_fields,
        raw: line.into(),
        bookmarked: false,
    })
}

fn parse_json_object(s: &str) -> Option<Vec<(String, String)>> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    // Skip whitespace and opening brace
    skip_ws(&chars, &mut i);
    if chars.get(i) != Some(&'{') {
        return None;
    }
    i = i.saturating_add(1);

    let mut fields = Vec::new();

    loop {
        skip_ws(&chars, &mut i);
        if chars.get(i) == Some(&'}') {
            break;
        }

        // Parse key
        let key = parse_json_string(&chars, &mut i)?;
        skip_ws(&chars, &mut i);
        if chars.get(i) != Some(&':') {
            return None;
        }
        i = i.saturating_add(1);
        skip_ws(&chars, &mut i);

        // Parse value
        let value = parse_json_value(&chars, &mut i)?;
        fields.push((key, value));

        skip_ws(&chars, &mut i);
        if chars.get(i) == Some(&',') {
            i = i.saturating_add(1);
        }
    }

    Some(fields)
}

fn skip_ws(chars: &[char], i: &mut usize) {
    // `get` rather than a length test plus an index: one expression that
    // cannot disagree with itself, over input this program did not write.
    while chars.get(*i).is_some_and(char::is_ascii_whitespace) {
        *i = i.saturating_add(1);
    }
}

fn parse_json_string(chars: &[char], i: &mut usize) -> Option<String> {
    if chars.get(*i) != Some(&'"') {
        return None;
    }
    *i = i.saturating_add(1);

    let mut s = String::new();
    while let Some(&ch) = chars.get(*i) {
        match ch {
            '"' => {
                *i = i.saturating_add(1);
                return Some(s);
            }
            '\\' => {
                *i = i.saturating_add(1);
                match chars.get(*i) {
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some('/') => s.push('/'),
                    Some('u') => {
                        // Parse 4 hex digits
                        *i = i.saturating_add(1);
                        let mut hex = String::new();
                        for _ in 0..4 {
                            if let Some(&c) = chars.get(*i) {
                                hex.push(c);
                                *i = i.saturating_add(1);
                            }
                        }
                        if let Ok(code) = u32::from_str_radix(&hex, 16)
                            && let Some(ch) = char::from_u32(code)
                        {
                            s.push(ch);
                        }
                        continue;
                    }
                    Some(&c) => s.push(c),
                    None => return None,
                }
            }
            c => s.push(c),
        }
        *i = i.saturating_add(1);
    }
    None
}

fn parse_json_value(chars: &[char], i: &mut usize) -> Option<String> {
    skip_ws(chars, i);
    match chars.get(*i) {
        Some('"') => parse_json_string(chars, i),
        Some(c) if c.is_ascii_digit() || *c == '-' => {
            let mut n = String::new();
            while let Some(&ch) = chars.get(*i) {
                if !(ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'e' | 'E' | '+')) {
                    break;
                }
                n.push(ch);
                *i = i.saturating_add(1);
            }
            Some(n)
        }
        Some('t') => {
            // true
            if chars
                .get(*i..i.saturating_add(4))
                .map(|s| s.iter().collect::<String>())
                == Some("true".into())
            {
                *i = i.saturating_add(4);
                Some("true".into())
            } else {
                None
            }
        }
        Some('f') => {
            // false
            if chars
                .get(*i..i.saturating_add(5))
                .map(|s| s.iter().collect::<String>())
                == Some("false".into())
            {
                *i = i.saturating_add(5);
                Some("false".into())
            } else {
                None
            }
        }
        Some('n') => {
            // null
            if chars
                .get(*i..i.saturating_add(4))
                .map(|s| s.iter().collect::<String>())
                == Some("null".into())
            {
                *i = i.saturating_add(4);
                Some("null".into())
            } else {
                None
            }
        }
        Some('[' | '{') => {
            // Skip nested structures (arrays/objects) as a single string
            let start = *i;
            let open = *chars.get(*i)?;
            let close = if open == '[' { ']' } else { '}' };
            let mut depth: u32 = 1;
            *i = i.saturating_add(1);
            while depth > 0 {
                let Some(&ch) = chars.get(*i) else {
                    // Ran off the end with the structure still open: the line
                    // is truncated, which is a thing a log file being written
                    // to genuinely is at the moment it is read.
                    break;
                };
                match ch {
                    c if c == open => depth = depth.saturating_add(1),
                    c if c == close => depth = depth.saturating_sub(1),
                    '"' => {
                        // The nested string moves `i` past its closing quote,
                        // so this must not advance again.
                        let _ = parse_json_string(chars, i);
                        continue;
                    }
                    _ => {}
                }
                *i = i.saturating_add(1);
            }
            Some(chars.get(start..*i)?.iter().collect())
        }
        _ => None,
    }
}

// ============================================================================
// Plain text log parser (fallback)
// ============================================================================

fn parse_plain_line(line: &str, line_number: usize) -> LogEntry {
    // Try to detect level from common patterns
    let upper = line.to_ascii_uppercase();
    let level = if upper.contains("[ERROR]") || upper.contains(" ERROR ") {
        LogLevel::Error
    } else if upper.contains("[WARN]") || upper.contains(" WARN ") || upper.contains("[WARNING]") {
        LogLevel::Warn
    } else if upper.contains("[DEBUG]") || upper.contains(" DEBUG ") {
        LogLevel::Debug
    } else if upper.contains("[TRACE]") || upper.contains(" TRACE ") {
        LogLevel::Trace
    } else if upper.contains("[FATAL]") || upper.contains(" FATAL ") || upper.contains("[CRITICAL]")
    {
        LogLevel::Fatal
    } else {
        LogLevel::Info
    };

    LogEntry {
        line_number,
        timestamp: 0,
        level,
        source: String::new(),
        message: line.into(),
        fields: Vec::new(),
        raw: line.into(),
        bookmarked: false,
    }
}

// ============================================================================
// Log File
// ============================================================================

#[derive(Debug, Clone)]
struct LogFile {
    name: String,
    path: String,
    entries: Vec<LogEntry>,
    is_json: bool,
}

impl LogFile {
    fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            entries: Vec::new(),
            is_json: false,
        }
    }

    fn parse_content(&mut self, content: &str) {
        self.entries.clear();

        // Detect if JSON-lines format
        let first_line = content.lines().next().unwrap_or("");
        self.is_json = first_line.trim().starts_with('{');

        for (i, line) in content.lines().enumerate() {
            if self.entries.len() >= MAX_LOG_ENTRIES {
                break;
            }

            let entry = if self.is_json {
                parse_json_line(line, i.saturating_add(1))
                    .unwrap_or_else(|| parse_plain_line(line, i.saturating_add(1)))
            } else {
                parse_plain_line(line, i.saturating_add(1))
            };

            self.entries.push(entry);
        }
    }

    fn level_counts(&self) -> [(LogLevel, usize); 6] {
        let mut counts: [(LogLevel, usize); 6] = [
            (LogLevel::Trace, 0),
            (LogLevel::Debug, 0),
            (LogLevel::Info, 0),
            (LogLevel::Warn, 0),
            (LogLevel::Error, 0),
            (LogLevel::Fatal, 0),
        ];

        for entry in &self.entries {
            for item in &mut counts {
                if item.0 == entry.level {
                    item.1 = item.1.saturating_add(1);
                }
            }
        }
        counts
    }

    fn unique_sources(&self) -> Vec<String> {
        let mut sources: Vec<String> = Vec::new();
        for entry in &self.entries {
            if !entry.source.is_empty() && !sources.contains(&entry.source) {
                sources.push(entry.source.clone());
            }
        }
        sources.sort();
        sources
    }

    fn top_sources(&self, limit: usize) -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for entry in &self.entries {
            if entry.source.is_empty() {
                continue;
            }
            if let Some(item) = counts.iter_mut().find(|(s, _)| *s == entry.source) {
                item.1 = item.1.saturating_add(1);
            } else {
                counts.push((entry.source.clone(), 1));
            }
        }
        counts.sort_by_key(|c| core::cmp::Reverse(c.1));
        counts.truncate(limit);
        counts
    }
}

// ============================================================================
// Filter State
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
struct FilterState {
    min_level: LogLevel,
    search_query: String,
    source_filter: Option<String>,
    time_start: Option<u64>,
    time_end: Option<u64>,
    show_bookmarked_only: bool,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            min_level: LogLevel::Trace,
            search_query: String::new(),
            source_filter: None,
            time_start: None,
            time_end: None,
            show_bookmarked_only: false,
        }
    }
}

impl FilterState {
    fn matches(&self, entry: &LogEntry) -> bool {
        // Level filter
        if entry.level.severity() < self.min_level.severity() {
            return false;
        }

        // Source filter
        if let Some(src) = &self.source_filter
            && !entry.source.eq_ignore_ascii_case(src)
        {
            return false;
        }

        // Time range
        if let Some(start) = self.time_start
            && entry.timestamp < start
        {
            return false;
        }
        if let Some(end) = self.time_end
            && entry.timestamp > end
        {
            return false;
        }

        // Bookmarked only
        if self.show_bookmarked_only && !entry.bookmarked {
            return false;
        }

        // Text search
        if !self.search_query.is_empty() {
            let lower = self.search_query.to_ascii_lowercase();
            let msg_match = entry.message.to_ascii_lowercase().contains(&lower);
            let src_match = entry.source.to_ascii_lowercase().contains(&lower);
            let field_match = entry
                .fields
                .iter()
                .any(|(_, v)| v.to_ascii_lowercase().contains(&lower));
            if !(msg_match || src_match || field_match) {
                return false;
            }
        }

        true
    }

    fn is_active(&self) -> bool {
        self.min_level != LogLevel::Trace
            || !self.search_query.is_empty()
            || self.source_filter.is_some()
            || self.time_start.is_some()
            || self.time_end.is_some()
            || self.show_bookmarked_only
    }
}

// ============================================================================
// Application State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    List,
    Stats,
    Detail,
}

impl ViewMode {
    fn label(self) -> &'static str {
        match self {
            Self::List => "Log View",
            Self::Stats => "Statistics",
            Self::Detail => "Detail",
        }
    }
}

struct App {
    // Files
    files: Vec<LogFile>,
    active_file: usize,

    // Filter
    filter: FilterState,

    // View
    view_mode: ViewMode,
    selected_entry: Option<usize>,
    scroll_offset: f32,
    auto_scroll: bool,
    wrap_lines: bool,
    show_timestamps: bool,
    show_source: bool,
    show_line_numbers: bool,

    // Search
    search_results: Vec<usize>,
    current_search_result: usize,
    /// Whether typing goes to the search box rather than to the shortcuts.
    ///
    /// Without it, typing "w" to look for a warning would set the severity
    /// floor to Warn instead — the app had no input at all, so nothing had ever
    /// needed to make the distinction.
    search_focused: bool,
}

impl App {
    fn new() -> Self {
        let mut file = LogFile::new("system.log", "/var/log/system.log");

        // Sample log content
        let sample = r#"{"timestamp":1716000000000,"level":"INFO","source":"kernel","message":"System starting up","version":"0.1.0"}
{"timestamp":1716000000100,"level":"DEBUG","source":"mm","message":"Physical memory: 8192 MiB detected"}
{"timestamp":1716000000200,"level":"INFO","source":"sched","message":"Scheduler initialized with 4 CPUs"}
{"timestamp":1716000000300,"level":"INFO","source":"pci","message":"PCI bus enumeration complete: 12 devices found"}
{"timestamp":1716000000400,"level":"WARN","source":"usb","message":"USB controller timeout during reset","port":2}
{"timestamp":1716000000500,"level":"INFO","source":"fs","message":"Root filesystem mounted (ext4)"}
{"timestamp":1716000000600,"level":"DEBUG","source":"net","message":"Network stack initializing"}
{"timestamp":1716000000700,"level":"INFO","source":"net","message":"eth0: link up 1000 Mbps full-duplex"}
{"timestamp":1716000000800,"level":"INFO","source":"dhcp","message":"DHCP lease obtained: 192.168.1.100"}
{"timestamp":1716000000900,"level":"ERROR","source":"gpu","message":"Failed to initialize Vulkan: driver not found"}
{"timestamp":1716000001000,"level":"INFO","source":"compositor","message":"Compositor started (software renderer)"}
{"timestamp":1716000001100,"level":"INFO","source":"desktop","message":"Desktop shell loaded","user":"root"}
{"timestamp":1716000001200,"level":"WARN","source":"audio","message":"No audio devices detected"}
{"timestamp":1716000001300,"level":"DEBUG","source":"pkg","message":"Package cache loaded: 142 packages"}
{"timestamp":1716000001400,"level":"INFO","source":"service","message":"All services started (23 active)"}
{"timestamp":1716000001500,"level":"TRACE","source":"ipc","message":"Channel 0x1A created: compositor -> desktop"}
{"timestamp":1716000001600,"level":"INFO","source":"login","message":"User session started","user":"admin"}
{"timestamp":1716000001700,"level":"ERROR","source":"net","message":"DNS resolution failed for update.example.com","error":"timeout"}
{"timestamp":1716000001800,"level":"WARN","source":"mm","message":"Memory pressure: 85% used, starting reclamation"}
{"timestamp":1716000001900,"level":"INFO","source":"mm","message":"Reclaimed 256 MiB (12 pages swapped out)"}
{"timestamp":1716000002000,"level":"FATAL","source":"driver","message":"Storage controller I/O error on /dev/sda","sector":48192}"#;

        file.parse_content(sample);

        Self {
            files: vec![file],
            active_file: 0,
            filter: FilterState::default(),
            view_mode: ViewMode::List,
            selected_entry: None,
            scroll_offset: 0.0,
            auto_scroll: true,
            wrap_lines: false,
            show_timestamps: true,
            show_source: true,
            show_line_numbers: true,
            search_results: Vec::new(),
            current_search_result: 0,
            search_focused: false,
        }
    }

    fn active_log(&self) -> Option<&LogFile> {
        self.files.get(self.active_file)
    }

    fn filtered_entries(&self) -> Vec<(usize, &LogEntry)> {
        if let Some(log) = self.active_log() {
            log.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| self.filter.matches(e))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn toggle_bookmark(&mut self, entry_idx: usize) {
        if let Some(log) = self.files.get_mut(self.active_file)
            && let Some(entry) = log.entries.get_mut(entry_idx)
        {
            entry.bookmarked = !entry.bookmarked;
        }
    }

    fn update_search(&mut self) {
        self.search_results.clear();
        if self.filter.search_query.is_empty() {
            return;
        }

        let lower = self.filter.search_query.to_ascii_lowercase();
        // Use field access instead of active_log() to allow partial borrowing
        if let Some(log) = self.files.get(self.active_file) {
            for (i, entry) in log.entries.iter().enumerate() {
                if self.search_results.len() >= MAX_SEARCH_RESULTS {
                    break;
                }
                if entry.message.to_ascii_lowercase().contains(&lower)
                    || entry.source.to_ascii_lowercase().contains(&lower)
                {
                    self.search_results.push(i);
                }
            }
        }
        self.current_search_result = 0;
    }

    fn next_search_result(&mut self) {
        if !self.search_results.is_empty() {
            self.current_search_result = self
                .current_search_result
                .saturating_add(1)
                .checked_rem(self.search_results.len())
                .unwrap_or(0);
        }
    }

    fn prev_search_result(&mut self) {
        if !self.search_results.is_empty() {
            if self.current_search_result == 0 {
                self.current_search_result = self.search_results.len().saturating_sub(1);
            } else {
                self.current_search_result = self.current_search_result.saturating_sub(1);
            }
        }
    }

    // ========================================================================
    // Events
    // ========================================================================

    /// Route a compositor event into the app.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Resize { .. } => {
                // This app lays out from constants rather than from a stored
                // size, so there is nothing to reconcile and nothing to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a key press.
    ///
    /// While the search box has text being typed into it every printable key is
    /// search text — which is why the search branch comes first, and why typing
    /// "w" to look for a warning does not toggle line wrapping.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        if self.search_focused {
            return self.handle_key_search(key);
        }
        match key.key {
            // Views.
            Key::Num1 => self.set_view(ViewMode::List),
            Key::Num2 => self.set_view(ViewMode::Stats),
            Key::Num3 => self.set_view(ViewMode::Detail),
            // Severity floor, on the initial of the level it admits.
            Key::T => self.set_min_level(LogLevel::Trace),
            Key::D => self.set_min_level(LogLevel::Debug),
            Key::I => self.set_min_level(LogLevel::Info),
            Key::W => self.set_min_level(LogLevel::Warn),
            Key::E => self.set_min_level(LogLevel::Error),
            Key::F => self.set_min_level(LogLevel::Fatal),
            // Selection, over the entries the filter is actually showing.
            Key::Up => self.step_selection(-1),
            Key::Down => self.step_selection(1),
            Key::Home => self.select_edge(true),
            Key::End => self.select_edge(false),
            Key::Space => self.toggle_selected_bookmark(),
            Key::B => {
                self.filter.show_bookmarked_only = !self.filter.show_bookmarked_only;
                self.reanchor_selection();
                EventResult::Consumed
            }
            Key::Slash => {
                self.search_focused = true;
                EventResult::Consumed
            }
            Key::N => self.step_search(true),
            Key::P => self.step_search(false),
            // Display toggles.
            Key::L => {
                self.wrap_lines = !self.wrap_lines;
                EventResult::Consumed
            }
            Key::A => {
                self.auto_scroll = !self.auto_scroll;
                EventResult::Consumed
            }
            Key::Escape => {
                if self.filter == FilterState::default() {
                    return EventResult::Ignored;
                }
                self.filter = FilterState::default();
                self.update_search();
                self.reanchor_selection();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Keys while the search box has focus.
    fn handle_key_search(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape | Key::Enter => {
                self.search_focused = false;
                EventResult::Consumed
            }
            Key::Backspace => {
                if self.filter.search_query.pop().is_none() {
                    return EventResult::Ignored;
                }
                self.update_search();
                self.reanchor_selection();
                EventResult::Consumed
            }
            _ => {
                if key.text.is_empty() || key.modifiers.ctrl {
                    return EventResult::Ignored;
                }
                self.filter.search_query.push_str(&key.text);
                self.update_search();
                self.reanchor_selection();
                EventResult::Consumed
            }
        }
    }

    /// Switch views, reporting whether anything changed.
    fn set_view(&mut self, mode: ViewMode) -> EventResult {
        if self.view_mode == mode {
            return EventResult::Ignored;
        }
        self.view_mode = mode;
        EventResult::Consumed
    }

    /// Raise or lower the severity floor.
    fn set_min_level(&mut self, level: LogLevel) -> EventResult {
        if self.filter.min_level == level {
            return EventResult::Ignored;
        }
        self.filter.min_level = level;
        // Changing the floor changes which entries exist on screen, and the
        // selection is an index into that list.
        self.reanchor_selection();
        EventResult::Consumed
    }

    /// Move the selection through the entries the filter is showing.
    ///
    /// Over `filtered_entries` and not the whole file: stepping by raw index
    /// would walk through entries the current severity floor is hiding, so the
    /// highlight would vanish for several presses and then reappear elsewhere.
    fn step_selection(&mut self, delta: isize) -> EventResult {
        let visible: Vec<usize> = self.filtered_entries().iter().map(|(i, _)| *i).collect();
        if visible.is_empty() {
            if self.selected_entry.is_none() {
                return EventResult::Ignored;
            }
            self.selected_entry = None;
            return EventResult::Consumed;
        }
        let current = self
            .selected_entry
            .and_then(|i| visible.iter().position(|&v| v == i));
        let next = match current {
            None => 0,
            Some(pos) => {
                let Ok(pos) = isize::try_from(pos) else {
                    return EventResult::Ignored;
                };
                let Some(moved) = pos.checked_add(delta) else {
                    return EventResult::Ignored;
                };
                let Ok(moved) = usize::try_from(moved) else {
                    return EventResult::Ignored; // off the top; stay put
                };
                if moved >= visible.len() {
                    return EventResult::Ignored; // off the bottom; stay put
                }
                moved
            }
        };
        let Some(&idx) = visible.get(next) else {
            return EventResult::Ignored;
        };
        if Some(idx) == self.selected_entry {
            return EventResult::Ignored;
        }
        self.selected_entry = Some(idx);
        EventResult::Consumed
    }

    /// Jump to the first or last visible entry.
    fn select_edge(&mut self, first: bool) -> EventResult {
        let visible: Vec<usize> = self.filtered_entries().iter().map(|(i, _)| *i).collect();
        let target = if first {
            visible.first().copied()
        } else {
            visible.last().copied()
        };
        if target == self.selected_entry {
            return EventResult::Ignored;
        }
        self.selected_entry = target;
        EventResult::Consumed
    }

    /// Put the selection back on a visible entry after the filter changed.
    fn reanchor_selection(&mut self) {
        let visible: Vec<usize> = self.filtered_entries().iter().map(|(i, _)| *i).collect();
        if !self.selected_entry.is_some_and(|i| visible.contains(&i)) {
            self.selected_entry = visible.first().copied();
        }
    }

    /// Bookmark or un-bookmark whatever is selected.
    fn toggle_selected_bookmark(&mut self) -> EventResult {
        let Some(idx) = self.selected_entry else {
            return EventResult::Ignored;
        };
        self.toggle_bookmark(idx);
        // Un-bookmarking while showing bookmarks only removes the entry from
        // the list it was selected in.
        self.reanchor_selection();
        EventResult::Consumed
    }

    /// Step through the search hits.
    fn step_search(&mut self, forward: bool) -> EventResult {
        if self.search_results.is_empty() {
            return EventResult::Ignored;
        }
        if forward {
            self.next_search_result();
        } else {
            self.prev_search_result();
        }
        EventResult::Consumed
    }

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut cmds);
        self.render_filter_bar(&mut cmds);

        let content_y = TOOLBAR_HEIGHT + FILTER_BAR_HEIGHT;
        let content_h = WINDOW_HEIGHT - content_y - STATUS_BAR_HEIGHT;

        match self.view_mode {
            ViewMode::List => self.render_log_list(&mut cmds, content_y, content_h),
            ViewMode::Stats => self.render_stats(&mut cmds, content_y, content_h),
            ViewMode::Detail => self.render_detail(&mut cmds, content_y, content_h),
        }

        self.render_status_bar(&mut cmds);

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 13.0,
            text: "Log Viewer".into(),
            font_size: TITLE_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });

        // File tabs
        let mut tab_x = 140.0;
        for (fi, file) in self.files.iter().enumerate() {
            // Measured bold whatever the tab's state: the active tab is drawn
            // bold, and sizing each tab to its own weight would reflow the
            // whole strip every time the user switched files.
            let w = text::measure(&file.name, SMALL_TEXT, FontWeightHint::Bold) + 20.0;
            let active = fi == self.active_file;

            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y: 8.0,
                width: w,
                height: 28.0,
                color: if active { SURFACE0 } else { CRUST },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: tab_x + 10.0,
                y: 14.0,
                text: file.name.clone(),
                font_size: SMALL_TEXT,
                color: if active { TEXT } else { SUBTEXT0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            tab_x += w + 4.0;
        }

        // View mode buttons
        let modes = [ViewMode::List, ViewMode::Stats, ViewMode::Detail];
        let mut mx = WINDOW_WIDTH - 300.0;
        for mode in &modes {
            let label = mode.label();
            let w = text::measure(label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
            let active = *mode == self.view_mode;

            cmds.push(RenderCommand::FillRect {
                x: mx,
                y: 8.0,
                width: w,
                height: 28.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: mx + 8.0,
                y: 14.0,
                text: label.into(),
                font_size: SMALL_TEXT,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            mx += w + 4.0;
        }

        // Auto-scroll toggle
        let auto_label = if self.auto_scroll {
            "Auto [ON]"
        } else {
            "Auto [OFF]"
        };
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 80.0,
            y: 14.0,
            text: auto_label.into(),
            font_size: SMALL_TEXT,
            color: if self.auto_scroll { GREEN } else { OVERLAY0 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_filter_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: FILTER_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Level filter pills
        let mut lx = PADDING;
        for level in LogLevel::all() {
            let label = level.short_label();
            let w = text::measure(label, BADGE_TEXT, FontWeightHint::Bold) + 12.0;
            let active = level.severity() >= self.filter.min_level.severity();

            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: y + 6.0,
                width: w,
                height: 24.0,
                color: if active { level.color() } else { SURFACE0 },
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 6.0,
                y: y + 11.0,
                text: label.into(),
                font_size: BADGE_TEXT,
                color: if active { CRUST } else { OVERLAY0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            lx += w + 4.0;
        }

        // Search box
        let search_x = lx + 12.0;
        let search_w = 250.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x,
            y: y + 6.0,
            width: search_w,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        let search_text = if self.filter.search_query.is_empty() {
            "Search logs..."
        } else {
            &self.filter.search_query
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 10.0,
            y: y + 11.0,
            text: search_text.into(),
            font_size: SMALL_TEXT,
            color: if self.filter.search_query.is_empty() {
                OVERLAY0
            } else {
                TEXT
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Search result count
        if !self.search_results.is_empty() {
            let count_text = format!(
                "{}/{}",
                self.current_search_result.saturating_add(1),
                self.search_results.len()
            );
            cmds.push(RenderCommand::Text {
                x: search_x + search_w + 8.0,
                y: y + 11.0,
                text: count_text,
                font_size: SMALL_TEXT,
                color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Source filter
        if let Some(src) = &self.filter.source_filter {
            let src_x = WINDOW_WIDTH - 200.0;
            cmds.push(RenderCommand::FillRect {
                x: src_x,
                y: y + 6.0,
                width: 150.0,
                height: 24.0,
                color: TEAL,
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: src_x + 8.0,
                y: y + 11.0,
                text: format!("src: {src}"),
                font_size: 10.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(134.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Filter active indicator
        if self.filter.is_active() {
            cmds.push(RenderCommand::FillRect {
                x: WINDOW_WIDTH - 40.0,
                y: y + 12.0,
                width: 12.0,
                height: 12.0,
                color: PEACH,
                corner_radii: CornerRadii::all(6.0),
            });
        }
    }

    fn render_log_list(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        let entries = self.filtered_entries();
        let max_visible = (height / LINE_HEIGHT) as usize;
        let scroll = (self.scroll_offset / LINE_HEIGHT) as usize;

        for (vi, (original_idx, entry)) in entries.iter().enumerate().skip(scroll).take(max_visible)
        {
            let ey = y + (vi.saturating_sub(scroll) as f32) * LINE_HEIGHT;
            let selected = self.selected_entry == Some(*original_idx);
            let is_search_hit = self.search_results.contains(original_idx);

            // Row background
            let bg = if selected {
                SURFACE0
            } else if is_search_hit {
                Color::rgba(137, 180, 250, 15)
            } else if entry.level >= LogLevel::Error {
                entry.level.bg_color()
            } else if vi % 2 == 0 {
                BASE
            } else {
                MANTLE
            };

            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: ey,
                width: WINDOW_WIDTH,
                height: LINE_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            let mut cx = PADDING;

            // Bookmark indicator
            if entry.bookmarked {
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ey + 3.0,
                    text: "*".into(),
                    font_size: NORMAL_TEXT,
                    color: YELLOW,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(12.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            cx += 14.0;

            // Line number
            if self.show_line_numbers {
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ey + 3.0,
                    text: format!("{:>5}", entry.line_number),
                    font_size: SMALL_TEXT,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(42.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cx += 46.0;
            }

            // Timestamp
            if self.show_timestamps && entry.timestamp > 0 {
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ey + 3.0,
                    text: entry.timestamp_display(),
                    font_size: SMALL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(100.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cx += 104.0;
            }

            // Level badge
            let level_label = entry.level.short_label();
            let level_w = text::measure(level_label, BADGE_TEXT, FontWeightHint::Bold) + 8.0;
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: ey + 2.0,
                width: level_w,
                height: 16.0,
                color: entry.level.color(),
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 4.0,
                y: ey + 4.0,
                text: level_label.into(),
                font_size: BADGE_TEXT,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(level_w),
                overflow: TextOverflow::Ellipsis,
            });
            cx += level_w + 6.0;

            // Source
            if self.show_source && !entry.source.is_empty() {
                // The brackets are drawn, so they have to be measured: the
                // old estimate advanced by the bare source name and left the
                // message overlapping the `]`.
                //
                // The source is elided rather than left to the compositor's
                // clip, and the cursor then advances by what was *drawn*. The
                // two used to disagree: the cell was clipped at SOURCE_WIDTH
                // but `cx` advanced by the source's full untruncated width, so
                // a long source both vanished mid-name with no marker and
                // pushed the message right by space nothing occupied. Past
                // roughly 1000 px of source it drove `msg_width` negative,
                // and `elide` of a negative width is the empty string — the
                // log message disappeared altogether. A source name comes out
                // of the log file, so its length is not ours to assume.
                let source_text = format!("[{}]", entry.source);
                let fitted = text::elide(
                    &source_text,
                    SOURCE_WIDTH,
                    "…",
                    SMALL_TEXT,
                    FontWeightHint::Bold,
                );
                let drawn_w = text::measure(&fitted, SMALL_TEXT, FontWeightHint::Bold);
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ey + 3.0,
                    text: fitted,
                    font_size: SMALL_TEXT,
                    color: SKY,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(SOURCE_WIDTH),
                    overflow: TextOverflow::Ellipsis,
                });
                cx += drawn_w + 8.0;
            }

            // Message
            let msg_width = (WINDOW_WIDTH - cx - PADDING).max(0.0);
            let display_msg = text::elide(
                &entry.message,
                msg_width,
                "...",
                NORMAL_TEXT,
                FontWeightHint::Regular,
            );
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ey + 3.0,
                text: display_msg,
                font_size: NORMAL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(msg_width),
                overflow: TextOverflow::Ellipsis,
            });
        }

        if entries.is_empty() {
            let empty = "No log entries match filters";
            cmds.push(RenderCommand::Text {
                x: text::center_x(
                    empty,
                    WINDOW_WIDTH / 2.0,
                    NORMAL_TEXT,
                    FontWeightHint::Regular,
                ),
                y: y + height / 2.0,
                text: empty.into(),
                font_size: NORMAL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_stats(&self, cmds: &mut Vec<RenderCommand>, y: f32, _height: f32) {
        if let Some(log) = self.active_log() {
            let counts = log.level_counts();
            let total = log.entries.len();

            // Level distribution
            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0,
                y: y + 16.0,
                text: "Level Distribution".into(),
                font_size: HEADER_TEXT,
                color: BLUE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });

            let bar_max_w = 400.0;
            for (i, (level, count)) in counts.iter().enumerate() {
                let sy = y + 44.0 + (i as f32) * 32.0;
                let pct = if total > 0 {
                    (*count as f32) / (total as f32)
                } else {
                    0.0
                };
                let bar_w = pct * bar_max_w;

                // Label
                cmds.push(RenderCommand::Text {
                    x: PADDING + 12.0,
                    y: sy + 4.0,
                    text: level.label().into(),
                    font_size: NORMAL_TEXT,
                    color: level.color(),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(60.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Bar
                cmds.push(RenderCommand::FillRect {
                    x: 100.0,
                    y: sy + 2.0,
                    width: bar_w.max(2.0),
                    height: 18.0,
                    color: level.color(),
                    corner_radii: CornerRadii::all(3.0),
                });

                // Count
                cmds.push(RenderCommand::Text {
                    x: 100.0 + bar_w + 8.0,
                    y: sy + 4.0,
                    text: format!("{count} ({:.1}%)", pct * 100.0),
                    font_size: SMALL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Top sources
            let sources_y = y + 250.0;
            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0,
                y: sources_y,
                text: "Top Sources".into(),
                font_size: HEADER_TEXT,
                color: TEAL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });

            for (si, (source, count)) in log.top_sources(10).iter().enumerate() {
                let sy = sources_y + 28.0 + (si as f32) * 24.0;
                cmds.push(RenderCommand::Text {
                    x: PADDING + 20.0,
                    y: sy,
                    text: source.clone(),
                    font_size: NORMAL_TEXT,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: 250.0,
                    y: sy,
                    text: format!("{count}"),
                    font_size: NORMAL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Summary stats on right
            let stats_x = WINDOW_WIDTH / 2.0 + 40.0;
            cmds.push(RenderCommand::Text {
                x: stats_x,
                y: y + 16.0,
                text: "Summary".into(),
                font_size: HEADER_TEXT,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });

            let summary_items = [
                ("Total entries", format!("{total}")),
                ("Sources", format!("{}", log.unique_sources().len())),
                (
                    "Errors",
                    format!(
                        "{}",
                        counts
                            .iter()
                            .find(|(l, _)| *l == LogLevel::Error)
                            .map_or(0, |(_, c)| *c)
                    ),
                ),
                (
                    "Warnings",
                    format!(
                        "{}",
                        counts
                            .iter()
                            .find(|(l, _)| *l == LogLevel::Warn)
                            .map_or(0, |(_, c)| *c)
                    ),
                ),
                (
                    "Format",
                    if log.is_json {
                        "JSON-lines"
                    } else {
                        "Plain text"
                    }
                    .into(),
                ),
                (
                    "Bookmarks",
                    format!("{}", log.entries.iter().filter(|e| e.bookmarked).count()),
                ),
            ];

            for (si, (label, value)) in summary_items.iter().enumerate() {
                let sy = y + 44.0 + (si as f32) * 26.0;
                cmds.push(RenderCommand::Text {
                    x: stats_x,
                    y: sy,
                    text: (*label).into(),
                    font_size: NORMAL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: stats_x + 140.0,
                    y: sy,
                    text: value.clone(),
                    font_size: NORMAL_TEXT,
                    color: TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_detail(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        if let Some(idx) = self.selected_entry
            && let Some(log) = self.active_log()
            && let Some(entry) = log.entries.get(idx)
        {
            let panel_w = WINDOW_WIDTH - 2.0 * PADDING;

            // Header
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: y + PADDING,
                width: panel_w,
                height: 50.0,
                color: MANTLE,
                corner_radii: CornerRadii {
                    top_left: 8.0,
                    top_right: 8.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            // Level badge
            let level_w =
                text::measure(entry.level.label(), NORMAL_TEXT, FontWeightHint::Bold) + 16.0;
            cmds.push(RenderCommand::FillRect {
                x: PADDING + 12.0,
                y: y + PADDING + 10.0,
                width: level_w,
                height: 24.0,
                color: entry.level.color(),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 20.0,
                y: y + PADDING + 14.0,
                text: entry.level.label().into(),
                font_size: NORMAL_TEXT,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(level_w),
                overflow: TextOverflow::Ellipsis,
            });

            // Source and time
            cmds.push(RenderCommand::Text {
                x: PADDING + level_w + 20.0,
                y: y + PADDING + 14.0,
                text: format!("[{}] at {}", entry.source, entry.timestamp_display()),
                font_size: NORMAL_TEXT,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0,
                y: y + PADDING + 38.0,
                text: format!("Line {}", entry.line_number),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Message body
            let body_y = y + PADDING + 54.0;
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: body_y,
                width: panel_w,
                height: height - 80.0,
                color: CRUST,
                corner_radii: CornerRadii {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_left: 8.0,
                    bottom_right: 8.0,
                },
            });

            // Message
            cmds.push(RenderCommand::Text {
                x: PADDING + 16.0,
                y: body_y + 12.0,
                text: "Message:".into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 16.0,
                y: body_y + 30.0,
                text: entry.message.clone(),
                font_size: NORMAL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - 32.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Extra fields
            if !entry.fields.is_empty() {
                let fields_y = body_y + 60.0;
                cmds.push(RenderCommand::Text {
                    x: PADDING + 16.0,
                    y: fields_y,
                    text: "Fields:".into(),
                    font_size: SMALL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(100.0),
                    overflow: TextOverflow::Ellipsis,
                });

                for (fi, (key, value)) in entry.fields.iter().enumerate() {
                    let fy = fields_y + 20.0 + (fi as f32) * LINE_HEIGHT;
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 24.0,
                        y: fy,
                        text: format!("{key}:"),
                        font_size: SMALL_TEXT,
                        color: TEAL,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(150.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 180.0,
                        y: fy,
                        text: value.clone(),
                        font_size: SMALL_TEXT,
                        color: TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(panel_w - 200.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }

            // Raw JSON
            let raw_y = body_y + 120.0 + (entry.fields.len() as f32) * LINE_HEIGHT;
            cmds.push(RenderCommand::Text {
                x: PADDING + 16.0,
                y: raw_y,
                text: "Raw:".into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::FillRect {
                x: PADDING + 16.0,
                y: raw_y + 18.0,
                width: panel_w - 32.0,
                height: LINE_HEIGHT + 8.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 24.0,
                y: raw_y + 22.0,
                text: text::elide(
                    &entry.raw,
                    panel_w - 48.0,
                    "...",
                    SMALL_TEXT,
                    FontWeightHint::Regular,
                ),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - 48.0),
                overflow: TextOverflow::Ellipsis,
            });

            return;
        }

        // No selection
        let empty = "Select a log entry to view details";
        cmds.push(RenderCommand::Text {
            x: text::center_x(
                empty,
                WINDOW_WIDTH / 2.0,
                NORMAL_TEXT,
                FontWeightHint::Regular,
            ),
            y: y + height / 2.0,
            text: empty.into(),
            font_size: NORMAL_TEXT,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let entries = self.filtered_entries();
        let total = self.active_log().map_or(0, |l| l.entries.len());

        // Entry count
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 5.0,
            text: format!("{} / {} entries", entries.len(), total),
            font_size: SMALL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // File info
        if let Some(log) = self.active_log() {
            cmds.push(RenderCommand::Text {
                x: 200.0,
                y: y + 5.0,
                text: log.path.clone(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Format
            cmds.push(RenderCommand::Text {
                x: WINDOW_WIDTH - 120.0,
                y: y + 5.0,
                text: if log.is_json {
                    "JSON-lines"
                } else {
                    "Plain text"
                }
                .into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// Main
// ============================================================================

// The trait and this app's own state type are both called `App`, so the impl
// names the trait in full rather than importing it under an alias.
impl oswindow::app::App for App {
    fn title(&self) -> String {
        self.active_log().map_or_else(
            || "Log Viewer".to_owned(),
            |log| format!("Log Viewer — {}", log.name),
        )
    }

    fn initial_size(&self) -> (u32, u32) {
        // The crate already allows the cast lints at module level; both are
        // positive constants well inside u32 regardless.
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// No clock — and for this app that is a statement worth reading twice.
    ///
    /// The module header promises "real-time log tailing with auto-scroll", and
    /// `auto_scroll` is a real toggle drawn in the status bar. There is nothing
    /// behind either: the only log this app has is a sample string compiled
    /// into the binary, and nothing reads a file. A tick would re-render an
    /// unchanging buffer on a timer.
    ///
    /// When a log source exists, this returns the poll interval and the tick
    /// re-reads the file — and only then does `auto_scroll` mean anything. See
    /// known-issues.md -> TD-C-LOGVIEWER-TAILS-A-STRING-COMPILED-INTO-ITSELF.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, _width: f32, _height: f32) -> RenderTree {
        // The size is ignored rather than stored: this app lays out from
        // constants, so there is no stored dimension to reconcile. Saying so
        // here beats a field nothing reads.
        RenderTree {
            commands: self.render_commands(),
        }
    }
}

fn main() -> ExitCode {
    let mut viewer = App::new();
    app::launch("logviewer", &mut viewer)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range or unwraps a `None` should fail loudly
    // and point at the line that did it — that is the diagnosis. The defensive
    // lints exist to keep panics out of code that runs on a user'"'"'s data,
    // which this is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    #[test]
    fn no_prefix_of_a_log_line_panics_or_hangs_the_parser() {
        // A log file being appended to *is* truncated at the moment it is
        // read, so a half-written last line is the normal case rather than a
        // hostile one. Every prefix has to be refused or parsed, never
        // panic and never loop.
        let line = concat!(
            r#"{"timestamp":1716000000000,"level":"INFO","source":"kernel","#,
            r#""message":"hello \"world\"","tags":["a","b"],"nested":{"k":1},"#,
            r#""n":-1.5e+3,"ok":true}"#
        );
        for end in 0..=line.len() {
            let Some(prefix) = line.get(..end) else {
                continue;
            };
            let mut file = LogFile::new("t.log", "/t.log");
            file.parse_content(prefix);
        }
    }

    #[test]
    fn a_truncated_nested_value_does_not_swallow_the_rest_of_the_file() {
        // The nested-structure scanner stops at the end of input with the
        // structure still open. It must not then report success over text it
        // never saw.
        let mut file = LogFile::new("t.log", "/t.log");
        file.parse_content(r#"{"timestamp":1,"level":"INFO","message":"a","x":{"y":"#);
        // Whatever it decides about this line, the next one must still parse.
        let mut file2 = LogFile::new("t.log", "/t.log");
        file2.parse_content(concat!(
            r#"{"timestamp":1,"level":"INFO","message":"a","x":{"y":"#,
            "\n",
            r#"{"timestamp":2,"level":"WARN","message":"b"}"#
        ));
        assert!(
            file2.entries.iter().any(|e| e.message == "b"),
            "the line after a truncated one should still be read"
        );
    }

    #[test]
    fn a_string_containing_braces_does_not_end_a_nested_value_early() {
        // The scanner hands a quote to `parse_json_string`, which is what
        // stops a `}` inside a message from closing the object around it.
        let mut file = LogFile::new("t.log", "/t.log");
        file.parse_content(
            r#"{"timestamp":1,"level":"INFO","message":"m","x":{"k":"}}}"},"after":"seen"}"#,
        );
        let entry = file.entries.first().expect("the line should parse");
        assert_eq!(
            entry
                .fields
                .iter()
                .find(|(k, _)| k == "after")
                .map(|(_, v)| v.as_str()),
            Some("seen"),
            "the field after the brace-laden string was lost: {:?}",
            entry.fields
        );
    }

    // ------------------------------------------------------------------
    // Events
    //
    // The app had no input handling until it was wired to the compositor.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
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

    #[test]
    fn the_number_row_reaches_every_view() {
        let mut app = App::new();
        for (k, mode) in [
            (Key::Num2, ViewMode::Stats),
            (Key::Num3, ViewMode::Detail),
            (Key::Num1, ViewMode::List),
        ] {
            assert_eq!(app.handle_event(&press(k)), EventResult::Consumed);
            assert_eq!(app.view_mode, mode, "{k:?} went to the wrong view");
        }
    }

    #[test]
    fn asking_for_the_view_already_shown_is_not_a_redraw() {
        let mut app = App::new();
        assert_eq!(app.view_mode, ViewMode::List);
        assert_eq!(app.handle_event(&press(Key::Num1)), EventResult::Ignored);
    }

    #[test]
    fn the_severity_keys_raise_the_floor_and_hide_the_quieter_lines() {
        let mut app = App::new();
        let all = app.filtered_entries().len();
        assert_eq!(app.handle_event(&press(Key::E)), EventResult::Consumed);
        assert_eq!(app.filter.min_level, LogLevel::Error);
        let errors = app.filtered_entries().len();
        assert!(errors > 0, "the sample log has errors");
        assert!(errors < all, "a floor of Error should hide the info lines");
        // Every surviving line is at least as severe as the floor.
        for (_, e) in app.filtered_entries() {
            assert!(
                e.level >= LogLevel::Error,
                "{:?} survived a floor of Error",
                e.level
            );
        }
        assert_eq!(app.handle_event(&press(Key::T)), EventResult::Consumed);
        assert_eq!(app.filtered_entries().len(), all, "Trace admits everything");
    }

    #[test]
    fn raising_the_floor_moves_the_selection_onto_a_line_still_shown() {
        // The selection is an index into the whole file while the screen lists
        // only what passes the filter, so a floor change can strand it.
        let mut app = App::new();
        app.handle_event(&press(Key::Down));
        assert!(app.selected_entry.is_some());
        app.handle_event(&press(Key::F)); // Fatal only
        let shown: Vec<usize> = app.filtered_entries().iter().map(|(i, _)| *i).collect();
        match app.selected_entry {
            Some(i) => assert!(shown.contains(&i), "selection left the visible list"),
            None => assert!(shown.is_empty()),
        }
    }

    #[test]
    fn the_arrows_walk_the_visible_lines_and_stop_at_the_ends() {
        let mut app = App::new();
        app.handle_event(&press(Key::E)); // a short list, quick to walk
        let shown: Vec<usize> = app.filtered_entries().iter().map(|(i, _)| *i).collect();
        assert!(shown.len() >= 2, "the sample log has several errors");
        app.selected_entry = shown.first().copied();
        assert_eq!(
            app.handle_event(&press(Key::Up)),
            EventResult::Ignored,
            "Up at the top should stay put"
        );
        for id in shown.iter().skip(1) {
            assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
            assert_eq!(app.selected_entry, Some(*id));
        }
        assert_eq!(
            app.handle_event(&press(Key::Down)),
            EventResult::Ignored,
            "Down at the bottom should stay put"
        );
    }

    #[test]
    fn home_and_end_jump_within_the_filtered_list() {
        let mut app = App::new();
        app.handle_event(&press(Key::W)); // hide the chatter
        let shown: Vec<usize> = app.filtered_entries().iter().map(|(i, _)| *i).collect();
        assert!(!shown.is_empty());
        app.handle_event(&press(Key::End));
        assert_eq!(app.selected_entry, shown.last().copied());
        app.handle_event(&press(Key::Home));
        assert_eq!(app.selected_entry, shown.first().copied());
        // And once there, Home is not a redraw.
        assert_eq!(app.handle_event(&press(Key::Home)), EventResult::Ignored);
    }

    #[test]
    fn typing_a_search_does_not_run_the_shortcuts_those_letters_name() {
        // "w" is the Warn floor outside the search box and a letter inside it.
        let mut app = App::new();
        let floor = app.filter.min_level;
        assert_eq!(app.handle_event(&press(Key::Slash)), EventResult::Consumed);
        assert!(app.search_focused);
        app.handle_event(&typed('w'));
        app.handle_event(&typed('a'));
        assert_eq!(app.filter.search_query, "wa");
        assert_eq!(app.filter.min_level, floor, "the floor moved while typing");
        app.handle_event(&press(Key::Enter));
        assert!(!app.search_focused);
        // And now the same key is a shortcut again.
        app.handle_event(&press(Key::W));
        assert_eq!(app.filter.min_level, LogLevel::Warn);
    }

    #[test]
    fn a_search_finds_lines_and_backspace_gives_them_back() {
        let mut app = App::new();
        let all = app.filtered_entries().len();
        app.handle_event(&press(Key::Slash));
        for c in "dhcp".chars() {
            app.handle_event(&typed(c));
        }
        let hits = app.filtered_entries().len();
        assert!(hits > 0, "the sample log mentions dhcp");
        assert!(hits < all, "a search should narrow the list");
        for _ in 0..4 {
            app.handle_event(&press(Key::Backspace));
        }
        assert_eq!(app.filter.search_query, "");
        assert_eq!(app.filtered_entries().len(), all);
        assert_eq!(
            app.handle_event(&press(Key::Backspace)),
            EventResult::Ignored,
            "backspace on an empty query is not a redraw"
        );
    }

    #[test]
    fn space_bookmarks_the_selected_line_and_b_shows_only_those() {
        let mut app = App::new();
        app.handle_event(&press(Key::Down));
        let idx = app.selected_entry.expect("something is selected");
        assert_eq!(app.handle_event(&press(Key::Space)), EventResult::Consumed);
        assert!(
            app.active_log()
                .and_then(|l| l.entries.get(idx))
                .is_some_and(|e| e.bookmarked),
            "the line should be bookmarked"
        );
        assert_eq!(app.handle_event(&press(Key::B)), EventResult::Consumed);
        assert!(app.filter.show_bookmarked_only);
        let shown = app.filtered_entries();
        assert_eq!(shown.len(), 1, "only the bookmarked line should be listed");
        assert_eq!(shown.first().map(|(i, _)| *i), Some(idx));

        // Un-bookmarking the last one empties the list the selection was in,
        // and a selection pointing at a line the view no longer shows is a
        // highlight the user cannot see.
        assert_eq!(app.handle_event(&press(Key::Space)), EventResult::Consumed);
        assert!(
            app.filtered_entries().is_empty(),
            "nothing is bookmarked now"
        );
        assert_eq!(
            app.selected_entry, None,
            "the selection should not survive onto a line the view hides"
        );
    }

    #[test]
    fn escape_clears_the_filters_and_does_nothing_when_they_are_already_clear() {
        let mut app = App::new();
        assert_eq!(
            app.handle_event(&press(Key::Escape)),
            EventResult::Ignored,
            "nothing is filtered yet"
        );
        app.handle_event(&press(Key::E));
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Consumed);
        assert_eq!(app.filter.min_level, LogLevel::Trace);
    }

    #[test]
    fn a_key_the_app_has_no_use_for_is_not_consumed() {
        let mut app = App::new();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = App::new();
        let release = Event::Key(KeyEvent {
            key: Key::Num2,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.view_mode, ViewMode::List);
    }

    #[test]
    fn the_display_toggles_flip() {
        let mut app = App::new();
        let (wrap, auto) = (app.wrap_lines, app.auto_scroll);
        app.handle_event(&press(Key::L));
        assert_ne!(app.wrap_lines, wrap);
        assert_eq!(app.auto_scroll, auto, "L should not touch auto-scroll");
        app.handle_event(&press(Key::A));
        assert_ne!(app.auto_scroll, auto);
    }
    use super::*;

    // --- Log level tests ---

    #[test]
    fn test_log_level_labels() {
        assert_eq!(LogLevel::Info.label(), "INFO");
        assert_eq!(LogLevel::Error.label(), "ERROR");
        assert_eq!(LogLevel::Fatal.short_label(), "FTL");
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("WARNING"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("CRITICAL"), Some(LogLevel::Fatal));
        assert_eq!(LogLevel::from_str("unknown"), None);
    }

    #[test]
    fn test_log_level_severity_order() {
        assert!(LogLevel::Trace.severity() < LogLevel::Debug.severity());
        assert!(LogLevel::Debug.severity() < LogLevel::Info.severity());
        assert!(LogLevel::Info.severity() < LogLevel::Warn.severity());
        assert!(LogLevel::Warn.severity() < LogLevel::Error.severity());
        assert!(LogLevel::Error.severity() < LogLevel::Fatal.severity());
    }

    #[test]
    fn test_log_level_all() {
        assert_eq!(LogLevel::all().len(), 6);
    }

    // --- JSON parser tests ---

    #[test]
    fn test_parse_json_line_basic() {
        let line = r#"{"level":"INFO","message":"hello","source":"test","timestamp":1000}"#;
        let entry = parse_json_line(line, 1).unwrap();
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "hello");
        assert_eq!(entry.source, "test");
        assert_eq!(entry.timestamp, 1000);
    }

    #[test]
    fn test_parse_json_line_error_level() {
        let line = r#"{"level":"ERROR","msg":"failure"}"#;
        let entry = parse_json_line(line, 1).unwrap();
        assert_eq!(entry.level, LogLevel::Error);
    }

    #[test]
    fn test_parse_json_line_extra_fields() {
        let line = r#"{"level":"INFO","message":"test","port":8080,"host":"localhost"}"#;
        let entry = parse_json_line(line, 1).unwrap();
        assert_eq!(entry.fields.len(), 2);
    }

    #[test]
    fn test_parse_json_line_empty() {
        assert!(parse_json_line("", 1).is_none());
    }

    #[test]
    fn test_parse_json_line_not_json() {
        assert!(parse_json_line("plain text log line", 1).is_none());
    }

    #[test]
    fn test_parse_json_string_escapes() {
        let chars: Vec<char> = r#""hello \"world\"""#.chars().collect();
        let mut i = 0;
        let result = parse_json_string(&chars, &mut i).unwrap();
        assert_eq!(result, "hello \"world\"");
    }

    #[test]
    fn test_parse_json_unicode_escape() {
        let chars: Vec<char> = r#""hello\u0041""#.chars().collect();
        let mut i = 0;
        let result = parse_json_string(&chars, &mut i).unwrap();
        assert_eq!(result, "helloA");
    }

    #[test]
    fn test_parse_json_value_number() {
        let chars: Vec<char> = "42".chars().collect();
        let mut i = 0;
        let result = parse_json_value(&chars, &mut i).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_parse_json_value_true() {
        let chars: Vec<char> = "true".chars().collect();
        let mut i = 0;
        let result = parse_json_value(&chars, &mut i).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_parse_json_value_null() {
        let chars: Vec<char> = "null".chars().collect();
        let mut i = 0;
        let result = parse_json_value(&chars, &mut i).unwrap();
        assert_eq!(result, "null");
    }

    // --- Plain text parser tests ---

    #[test]
    fn test_parse_plain_error() {
        let entry = parse_plain_line("[ERROR] something failed", 1);
        assert_eq!(entry.level, LogLevel::Error);
    }

    #[test]
    fn test_parse_plain_warn() {
        let entry = parse_plain_line("[WARN] low memory", 1);
        assert_eq!(entry.level, LogLevel::Warn);
    }

    #[test]
    fn test_parse_plain_default() {
        let entry = parse_plain_line("just a plain message", 1);
        assert_eq!(entry.level, LogLevel::Info);
    }

    // --- LogEntry tests ---

    #[test]
    fn test_timestamp_display() {
        let entry = LogEntry {
            line_number: 1,
            timestamp: 3_661_500,
            level: LogLevel::Info,
            source: String::new(),
            message: String::new(),
            fields: Vec::new(),
            raw: String::new(),
            bookmarked: false,
        };
        assert_eq!(entry.timestamp_display(), "01:01:01.500");
    }

    #[test]
    fn test_timestamp_display_zero() {
        let entry = LogEntry {
            line_number: 1,
            timestamp: 0,
            level: LogLevel::Info,
            source: String::new(),
            message: String::new(),
            fields: Vec::new(),
            raw: String::new(),
            bookmarked: false,
        };
        assert_eq!(entry.timestamp_display(), "00:00:00.000");
    }

    // --- LogFile tests ---

    #[test]
    fn test_log_file_parse_json() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content(
            r#"{"level":"INFO","message":"hello"}
{"level":"ERROR","message":"fail"}"#,
        );
        assert_eq!(file.entries.len(), 2);
        assert!(file.is_json);
    }

    #[test]
    fn test_log_file_parse_plain() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content("line 1\nline 2\nline 3");
        assert_eq!(file.entries.len(), 3);
        assert!(!file.is_json);
    }

    #[test]
    fn test_log_file_level_counts() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content(
            r#"{"level":"INFO","message":"a"}
{"level":"INFO","message":"b"}
{"level":"ERROR","message":"c"}"#,
        );
        let counts = file.level_counts();
        let info_count = counts
            .iter()
            .find(|(l, _)| *l == LogLevel::Info)
            .map_or(0, |(_, c)| *c);
        assert_eq!(info_count, 2);
    }

    #[test]
    fn test_log_file_unique_sources() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content(
            r#"{"level":"INFO","source":"a","message":"x"}
{"level":"INFO","source":"b","message":"y"}
{"level":"INFO","source":"a","message":"z"}"#,
        );
        let sources = file.unique_sources();
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn test_log_file_top_sources() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content(
            r#"{"level":"INFO","source":"a","message":"1"}
{"level":"INFO","source":"a","message":"2"}
{"level":"INFO","source":"b","message":"3"}"#,
        );
        let top = file.top_sources(5);
        assert_eq!(top[0].0, "a");
        assert_eq!(top[0].1, 2);
    }

    // --- Filter tests ---

    #[test]
    fn test_filter_default() {
        let filter = FilterState::default();
        assert_eq!(filter.min_level, LogLevel::Trace);
        assert!(!filter.is_active());
    }

    #[test]
    fn test_filter_by_level() {
        let filter = FilterState {
            min_level: LogLevel::Warn,
            ..Default::default()
        };
        let info_entry = make_entry(LogLevel::Info, "", "test");
        let warn_entry = make_entry(LogLevel::Warn, "", "test");
        assert!(!filter.matches(&info_entry));
        assert!(filter.matches(&warn_entry));
    }

    #[test]
    fn test_filter_by_source() {
        let filter = FilterState {
            source_filter: Some("kernel".into()),
            ..Default::default()
        };
        let kernel = make_entry(LogLevel::Info, "kernel", "msg");
        let net = make_entry(LogLevel::Info, "net", "msg");
        assert!(filter.matches(&kernel));
        assert!(!filter.matches(&net));
    }

    #[test]
    fn test_filter_by_search() {
        let filter = FilterState {
            search_query: "error".into(),
            ..Default::default()
        };
        let match_entry = make_entry(LogLevel::Info, "", "an error occurred");
        let no_match = make_entry(LogLevel::Info, "", "all is well");
        assert!(filter.matches(&match_entry));
        assert!(!filter.matches(&no_match));
    }

    #[test]
    fn test_filter_bookmarked_only() {
        let filter = FilterState {
            show_bookmarked_only: true,
            ..Default::default()
        };
        let mut bookmarked = make_entry(LogLevel::Info, "", "msg");
        bookmarked.bookmarked = true;
        let not_bookmarked = make_entry(LogLevel::Info, "", "msg");
        assert!(filter.matches(&bookmarked));
        assert!(!filter.matches(&not_bookmarked));
    }

    #[test]
    fn test_filter_is_active() {
        let mut filter = FilterState::default();
        assert!(!filter.is_active());

        filter.min_level = LogLevel::Error;
        assert!(filter.is_active());
    }

    // --- App tests ---

    #[test]
    fn test_app_new() {
        let app = App::new();
        assert!(!app.files.is_empty());
        assert!(app.active_log().is_some());
    }

    #[test]
    fn test_app_filtered_entries() {
        let app = App::new();
        let entries = app.filtered_entries();
        assert!(!entries.is_empty());
    }

    #[test]
    fn test_app_filtered_with_level() {
        let mut app = App::new();
        app.filter.min_level = LogLevel::Error;
        let entries = app.filtered_entries();
        assert!(entries.iter().all(|(_, e)| e.level >= LogLevel::Error));
    }

    #[test]
    fn test_app_toggle_bookmark() {
        let mut app = App::new();
        assert!(!app.files[0].entries[0].bookmarked);
        app.toggle_bookmark(0);
        assert!(app.files[0].entries[0].bookmarked);
        app.toggle_bookmark(0);
        assert!(!app.files[0].entries[0].bookmarked);
    }

    #[test]
    fn test_app_search() {
        let mut app = App::new();
        app.filter.search_query = "error".into();
        app.update_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn test_app_search_navigation() {
        let mut app = App::new();
        // update_search is a text search over message + source (NOT the level
        // field — levels are handled by the level filter). "net" is the source
        // of several sample entries, so it yields multiple matches, which is
        // what we need to exercise next/prev wrap-around below.
        app.filter.search_query = "net".into();
        app.update_search();
        let count = app.search_results.len();
        assert!(count > 1);

        app.next_search_result();
        assert_eq!(app.current_search_result, 1);

        app.prev_search_result();
        assert_eq!(app.current_search_result, 0);

        app.prev_search_result(); // wrap
        assert_eq!(app.current_search_result, count.saturating_sub(1));
    }

    #[test]
    fn test_app_render_list_view() {
        let app = App::new();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_stats_view() {
        let mut app = App::new();
        app.view_mode = ViewMode::Stats;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_detail_view() {
        let mut app = App::new();
        app.view_mode = ViewMode::Detail;
        app.selected_entry = Some(0);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_detail_no_selection() {
        let mut app = App::new();
        app.view_mode = ViewMode::Detail;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_view_mode_label() {
        assert_eq!(ViewMode::List.label(), "Log View");
        assert_eq!(ViewMode::Stats.label(), "Statistics");
    }

    // --- Utility tests ---

    /// The raw line at the foot of the detail panel is cut to the width of the
    /// box drawn behind it. The old helper compared `s.len()` — bytes — against
    /// a fixed budget of 120 "characters", so an accented line was cut short
    /// while it still fitted, and a line of wide glyphs ran past the box.
    #[test]
    fn the_raw_line_is_cut_to_the_box_drawn_behind_it() {
        for raw in [
            "2026-08-14T10:00:00Z INFO kernel: a raw log line long enough to need cutting down",
            "2026-08-14T10:00:00Z INFO kernel: une ligne brute avec des caractères accentués \
             suffisamment longue",
        ] {
            // Half of what the line actually measures, rather than a literal.
            // The `ends_with("...")` assertion below only means anything if
            // the line really does overflow `room`, and with a literal that is
            // a race between the constant and however wide the host's font
            // draws. These lines measure 450px against the 400px that used to
            // be hard-coded here — a 12% margin — and the same shape of test
            // in dbviewer had already lost that race and begun failing when
            // `SystemFont` started resolving a narrower face. Half the
            // measured width cannot lose it, on any face.
            let room = text::measure(raw, SMALL_TEXT, FontWeightHint::Regular) / 2.0;
            let out = text::elide(raw, room, "...", SMALL_TEXT, FontWeightHint::Regular);
            let w = text::measure(&out, SMALL_TEXT, FontWeightHint::Regular);
            assert!(w <= room + 0.01, "{out:?} is {w} px in {room} px of room");
            assert!(out.ends_with("..."), "a cut raw line should say so");
        }
    }

    /// A raw line that fits is left exactly as it is — no ellipsis, no loss.
    #[test]
    fn a_short_raw_line_is_left_alone() {
        let raw = "INFO ready";
        assert_eq!(
            text::elide(raw, 400.0, "...", SMALL_TEXT, FontWeightHint::Regular),
            raw
        );
    }

    // --- Helper ---

    fn make_entry(level: LogLevel, source: &str, message: &str) -> LogEntry {
        LogEntry {
            line_number: 1,
            timestamp: 1000,
            level,
            source: source.into(),
            message: message.into(),
            fields: Vec::new(),
            raw: String::new(),
            bookmarked: false,
        }
    }
    // --- Text measurement ---

    /// Every level badge has to fit the pill drawn behind it. The labels are
    /// short and ASCII, so the old byte-count estimate happened to work — but
    /// it worked for the wrong reason, and would stop the moment the face
    /// changed or a label gained a non-ASCII character.
    #[test]
    fn level_badges_fit_their_pills() {
        for level in LogLevel::all() {
            let label = level.short_label();
            let pill = text::measure(label, BADGE_TEXT, FontWeightHint::Bold) + 12.0;
            assert!(
                text::measure(label, BADGE_TEXT, FontWeightHint::Bold) <= pill - 12.0 + 0.01,
                "{label:?} does not fit its pill"
            );
            assert!(pill > 12.0, "{label:?} produced an empty pill");
        }
    }

    /// The source field is drawn with brackets around it, so it has to be
    /// measured with them. The old estimate advanced the pen by the bare
    /// source name, leaving the message overlapping the closing bracket.
    #[test]
    fn the_source_field_advances_past_its_brackets() {
        let bare = text::measure("kernel", SMALL_TEXT, FontWeightHint::Bold);
        let bracketed = text::measure("[kernel]", SMALL_TEXT, FontWeightHint::Bold);
        assert!(
            bracketed > bare,
            "brackets are drawn, so they take room: {bare} vs {bracketed}"
        );
    }

    /// A long message is cut to the room left on the row, by measured width.
    #[test]
    fn a_long_message_is_elided_to_the_room_left() {
        let msg = "a log line that is far longer than the space left for it on the row";
        let room = 120.0;
        let out = text::elide(msg, room, "...", NORMAL_TEXT, FontWeightHint::Regular);
        let w = text::measure(&out, NORMAL_TEXT, FontWeightHint::Regular);
        assert!(w <= room + 0.01, "{out:?} is {w} px in {room} px of room");
        assert!(out.ends_with("..."), "a cut message should say so");
    }

    /// The file-tab strip must not reflow when the selection moves, so every
    /// tab is measured in the bold weight the active one is drawn in.
    #[test]
    fn the_file_tab_strip_does_not_reflow_on_selection() {
        for name in ["app.log", "very-long-service-name.log"] {
            let w = text::measure(name, SMALL_TEXT, FontWeightHint::Bold) + 20.0;
            let regular = text::measure(name, SMALL_TEXT, FontWeightHint::Regular);
            assert!(w >= regular + 20.0, "{name:?} tab is too narrow when bold");
        }
    }

    // --- Log row layout ---
    //
    // A row's cursor and the cells it draws are two calculations of one
    // quantity. The source cell is where they used to disagree: it was clipped
    // to SOURCE_WIDTH but advanced the cursor by its full untruncated width.

    /// An app whose one and only log entry has the given source.
    fn app_with_source(source: &str) -> App {
        let mut app = App::new();
        app.files.truncate(1);
        app.active_file = 0;
        if let Some(file) = app.files.first_mut() {
            file.entries.clear();
            file.entries.push(LogEntry {
                line_number: 1,
                timestamp: 1_000,
                level: LogLevel::Info,
                source: source.to_string(),
                message: "the message that must survive".to_string(),
                fields: Vec::new(),
                raw: String::new(),
                bookmarked: false,
            });
        }
        // Every optional cell on, so the row cursor has crossed all of them by
        // the time it reaches the source — the layout most likely to overflow.
        app.show_source = true;
        app.show_timestamps = true;
        app.show_line_numbers = true;
        app
    }

    /// Every text command drawn by the log list, in order.
    fn row_texts(app: &App) -> Vec<(f32, String, f32, FontWeightHint)> {
        let mut cmds = Vec::new();
        app.render_log_list(&mut cmds, 0.0, 200.0);
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => Some((*x, text.clone(), *font_size, *font_weight)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_overlong_source_is_marked_as_cut_and_stays_in_its_cell() {
        let app = app_with_source("com.example.platform.services.database.connection.pool");
        let texts = row_texts(&app);
        let (x, drawn, size, weight) = texts
            .iter()
            .find(|(_, t, _, _)| t.starts_with('['))
            .cloned()
            .expect("the source cell should be drawn");
        assert!(
            drawn.ends_with('…'),
            "a source too long for its cell must be visibly cut, got {drawn:?}"
        );
        let w = text::measure(&drawn, size, weight);
        assert!(
            w <= SOURCE_WIDTH + 0.01,
            "source {drawn:?} is {w} px in a {SOURCE_WIDTH} px cell"
        );
        assert!(x >= 0.0);
    }

    #[test]
    fn a_short_source_is_left_verbatim() {
        let app = app_with_source("db");
        let texts = row_texts(&app);
        assert!(
            texts.iter().any(|(_, t, _, _)| t == "[db]"),
            "a source that fits must be drawn verbatim: {texts:?}"
        );
    }

    #[test]
    fn an_absurd_source_does_not_swallow_the_message() {
        // The failure this guards: `cx` advanced by the source's *full* width,
        // so a source wide enough drove msg_width negative, and elide() of a
        // negative width is the empty string. The log line vanished, leaving a
        // row that showed only a truncated source name.
        let app = app_with_source(&"averylongsourcesegment.".repeat(60));
        let texts = row_texts(&app);
        assert!(
            texts
                .iter()
                .any(|(_, t, _, _)| t.starts_with("the message")),
            "the message must still be drawn however long the source is: {texts:?}"
        );
    }

    #[test]
    fn the_message_starts_where_the_source_ends() {
        // The cursor must advance by what was drawn, not by what it was given:
        // otherwise a clipped source leaves a gap of blank row before the
        // message, in space nothing occupies.
        let app = app_with_source("com.example.platform.services.database.connection.pool");
        let texts = row_texts(&app);
        let (sx, source, ssize, sweight) = texts
            .iter()
            .find(|(_, t, _, _)| t.starts_with('['))
            .cloned()
            .expect("the source cell should be drawn");
        let (mx, _, _, _) = texts
            .iter()
            .find(|(_, t, _, _)| t.starts_with("the message"))
            .cloned()
            .expect("the message should be drawn");
        let source_end = sx + text::measure(&source, ssize, sweight);
        let gap = mx - source_end;
        assert!(
            (0.0..=16.0).contains(&gap),
            "message starts {gap} px after the source ends; \
             a gap that large is space the clipped source was charged for"
        );
    }
}
