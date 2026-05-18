//! OurOS System Log Viewer
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
const CHAR_WIDTH: f32 = 8.0;
const SMALL_TEXT: f32 = 12.0;
const NORMAL_TEXT: f32 = 14.0;
const HEADER_TEXT: f32 = 16.0;
const TITLE_TEXT: f32 = 18.0;

const MAX_LOG_ENTRIES: usize = 100_000;
const MAX_BOOKMARKS: usize = 500;
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
        &[Self::Trace, Self::Debug, Self::Info, Self::Warn, Self::Error, Self::Fatal]
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

    let timestamp = fields.iter()
        .find(|(k, _)| k == "timestamp" || k == "ts" || k == "time" || k == "t")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .unwrap_or(0);

    let level = fields.iter()
        .find(|(k, _)| k == "level" || k == "lvl" || k == "severity")
        .and_then(|(_, v)| LogLevel::from_str(v))
        .unwrap_or(LogLevel::Info);

    let source = fields.iter()
        .find(|(k, _)| k == "source" || k == "src" || k == "component" || k == "module" || k == "logger")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let message = fields.iter()
        .find(|(k, _)| k == "message" || k == "msg" || k == "text")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let extra_fields: Vec<(String, String)> = fields.iter()
        .filter(|(k, _)| {
            !matches!(k.as_str(), "timestamp" | "ts" | "time" | "t" | "level" | "lvl" | "severity"
                | "source" | "src" | "component" | "module" | "logger" | "message" | "msg" | "text")
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
    if chars.get(i) != Some(&'{') { return None; }
    i = i.saturating_add(1);

    let mut fields = Vec::new();

    loop {
        skip_ws(&chars, &mut i);
        if chars.get(i) == Some(&'}') { break; }

        // Parse key
        let key = parse_json_string(&chars, &mut i)?;
        skip_ws(&chars, &mut i);
        if chars.get(i) != Some(&':') { return None; }
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
    while *i < chars.len() && chars[*i].is_ascii_whitespace() {
        *i = i.saturating_add(1);
    }
}

fn parse_json_string(chars: &[char], i: &mut usize) -> Option<String> {
    if chars.get(*i) != Some(&'"') { return None; }
    *i = i.saturating_add(1);

    let mut s = String::new();
    while *i < chars.len() {
        match chars[*i] {
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
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                s.push(ch);
                            }
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
            while *i < chars.len() && (chars[*i].is_ascii_digit() || chars[*i] == '.' || chars[*i] == '-' || chars[*i] == 'e' || chars[*i] == 'E' || chars[*i] == '+') {
                n.push(chars[*i]);
                *i = i.saturating_add(1);
            }
            Some(n)
        }
        Some('t') => {
            // true
            if chars.get(*i..i.saturating_add(4)).map(|s| s.iter().collect::<String>()) == Some("true".into()) {
                *i = i.saturating_add(4);
                Some("true".into())
            } else { None }
        }
        Some('f') => {
            // false
            if chars.get(*i..i.saturating_add(5)).map(|s| s.iter().collect::<String>()) == Some("false".into()) {
                *i = i.saturating_add(5);
                Some("false".into())
            } else { None }
        }
        Some('n') => {
            // null
            if chars.get(*i..i.saturating_add(4)).map(|s| s.iter().collect::<String>()) == Some("null".into()) {
                *i = i.saturating_add(4);
                Some("null".into())
            } else { None }
        }
        Some('[') | Some('{') => {
            // Skip nested structures (arrays/objects) as a single string
            let start = *i;
            let open = chars[*i];
            let close = if open == '[' { ']' } else { '}' };
            let mut depth: u32 = 1;
            *i = i.saturating_add(1);
            while *i < chars.len() && depth > 0 {
                match chars[*i] {
                    c if c == open => depth = depth.saturating_add(1),
                    c if c == close => depth = depth.saturating_sub(1),
                    '"' => { let _ = parse_json_string(chars, i); continue; }
                    _ => {}
                }
                *i = i.saturating_add(1);
            }
            Some(chars[start..*i].iter().collect())
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
    } else if upper.contains("[FATAL]") || upper.contains(" FATAL ") || upper.contains("[CRITICAL]") {
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
            if self.entries.len() >= MAX_LOG_ENTRIES { break; }

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
            if entry.source.is_empty() { continue; }
            if let Some(item) = counts.iter_mut().find(|(s, _)| *s == entry.source) {
                item.1 = item.1.saturating_add(1);
            } else {
                counts.push((entry.source.clone(), 1));
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        counts.truncate(limit);
        counts
    }
}

// ============================================================================
// Filter State
// ============================================================================

#[derive(Debug, Clone)]
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
        if let Some(src) = &self.source_filter {
            if !entry.source.eq_ignore_ascii_case(src) {
                return false;
            }
        }

        // Time range
        if let Some(start) = self.time_start {
            if entry.timestamp < start { return false; }
        }
        if let Some(end) = self.time_end {
            if entry.timestamp > end { return false; }
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
            let field_match = entry.fields.iter().any(|(_, v)| v.to_ascii_lowercase().contains(&lower));
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
        }
    }

    fn active_log(&self) -> Option<&LogFile> {
        self.files.get(self.active_file)
    }

    fn filtered_entries(&self) -> Vec<(usize, &LogEntry)> {
        if let Some(log) = self.active_log() {
            log.entries.iter()
                .enumerate()
                .filter(|(_, e)| self.filter.matches(e))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn toggle_bookmark(&mut self, entry_idx: usize) {
        if let Some(log) = self.files.get_mut(self.active_file) {
            if let Some(entry) = log.entries.get_mut(entry_idx) {
                entry.bookmarked = !entry.bookmarked;
            }
        }
    }

    fn update_search(&mut self) {
        self.search_results.clear();
        if self.filter.search_query.is_empty() { return; }

        let lower = self.filter.search_query.to_ascii_lowercase();
        // Use field access instead of active_log() to allow partial borrowing
        if let Some(log) = self.files.get(self.active_file) {
            for (i, entry) in log.entries.iter().enumerate() {
                if self.search_results.len() >= MAX_SEARCH_RESULTS { break; }
                if entry.message.to_ascii_lowercase().contains(&lower)
                    || entry.source.to_ascii_lowercase().contains(&lower) {
                    self.search_results.push(i);
                }
            }
        }
        self.current_search_result = 0;
    }

    fn next_search_result(&mut self) {
        if !self.search_results.is_empty() {
            self.current_search_result = (self.current_search_result.saturating_add(1)) % self.search_results.len();
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

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0,
            width: WINDOW_WIDTH, height: WINDOW_HEIGHT,
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
            x: 0.0, y: 0.0,
            width: WINDOW_WIDTH, height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING, y: 13.0,
            text: "Log Viewer".into(),
            font_size: TITLE_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });

        // File tabs
        let mut tab_x = 140.0;
        for (fi, file) in self.files.iter().enumerate() {
            let w = (file.name.len() as f32) * CHAR_WIDTH + 20.0;
            let active = fi == self.active_file;

            cmds.push(RenderCommand::FillRect {
                x: tab_x, y: 8.0,
                width: w, height: 28.0,
                color: if active { SURFACE0 } else { CRUST },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: tab_x + 10.0, y: 14.0,
                text: file.name.clone(),
                font_size: SMALL_TEXT,
                color: if active { TEXT } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(w),
            });
            tab_x += w + 4.0;
        }

        // View mode buttons
        let modes = [ViewMode::List, ViewMode::Stats, ViewMode::Detail];
        let mut mx = WINDOW_WIDTH - 300.0;
        for mode in &modes {
            let label = mode.label();
            let w = (label.len() as f32) * CHAR_WIDTH + 16.0;
            let active = *mode == self.view_mode;

            cmds.push(RenderCommand::FillRect {
                x: mx, y: 8.0,
                width: w, height: 28.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: mx + 8.0, y: 14.0,
                text: label.into(),
                font_size: SMALL_TEXT,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
            });
            mx += w + 4.0;
        }

        // Auto-scroll toggle
        let auto_label = if self.auto_scroll { "Auto [ON]" } else { "Auto [OFF]" };
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 80.0, y: 14.0,
            text: auto_label.into(),
            font_size: SMALL_TEXT,
            color: if self.auto_scroll { GREEN } else { OVERLAY0 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });
    }

    fn render_filter_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0, y,
            width: WINDOW_WIDTH, height: FILTER_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Level filter pills
        let mut lx = PADDING;
        for level in LogLevel::all() {
            let label = level.short_label();
            let w = (label.len() as f32) * CHAR_WIDTH + 12.0;
            let active = level.severity() >= self.filter.min_level.severity();

            cmds.push(RenderCommand::FillRect {
                x: lx, y: y + 6.0,
                width: w, height: 24.0,
                color: if active { level.color() } else { SURFACE0 },
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 6.0, y: y + 11.0,
                text: label.into(),
                font_size: 10.0,
                color: if active { CRUST } else { OVERLAY0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
            });
            lx += w + 4.0;
        }

        // Search box
        let search_x = lx + 12.0;
        let search_w = 250.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x, y: y + 6.0,
            width: search_w, height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        let search_text = if self.filter.search_query.is_empty() {
            "Search logs..."
        } else {
            &self.filter.search_query
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 10.0, y: y + 11.0,
            text: search_text.into(),
            font_size: SMALL_TEXT,
            color: if self.filter.search_query.is_empty() { OVERLAY0 } else { TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 20.0),
        });

        // Search result count
        if !self.search_results.is_empty() {
            let count_text = format!("{}/{}", self.current_search_result.saturating_add(1), self.search_results.len());
            cmds.push(RenderCommand::Text {
                x: search_x + search_w + 8.0, y: y + 11.0,
                text: count_text,
                font_size: SMALL_TEXT,
                color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
            });
        }

        // Source filter
        if let Some(src) = &self.filter.source_filter {
            let src_x = WINDOW_WIDTH - 200.0;
            cmds.push(RenderCommand::FillRect {
                x: src_x, y: y + 6.0,
                width: 150.0, height: 24.0,
                color: TEAL,
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: src_x + 8.0, y: y + 11.0,
                text: format!("src: {src}"),
                font_size: 10.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(134.0),
            });
        }

        // Filter active indicator
        if self.filter.is_active() {
            cmds.push(RenderCommand::FillRect {
                x: WINDOW_WIDTH - 40.0, y: y + 12.0,
                width: 12.0, height: 12.0,
                color: PEACH,
                corner_radii: CornerRadii::all(6.0),
            });
        }
    }

    fn render_log_list(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        let entries = self.filtered_entries();
        let max_visible = (height / LINE_HEIGHT) as usize;
        let scroll = (self.scroll_offset / LINE_HEIGHT) as usize;

        for (vi, (original_idx, entry)) in entries.iter().enumerate().skip(scroll).take(max_visible) {
            let ey = y + ((vi - scroll) as f32) * LINE_HEIGHT;
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
                x: 0.0, y: ey,
                width: WINDOW_WIDTH, height: LINE_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            let mut cx = PADDING;

            // Bookmark indicator
            if entry.bookmarked {
                cmds.push(RenderCommand::Text {
                    x: cx, y: ey + 3.0,
                    text: "*".into(),
                    font_size: NORMAL_TEXT,
                    color: YELLOW,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(12.0),
                });
            }
            cx += 14.0;

            // Line number
            if self.show_line_numbers {
                cmds.push(RenderCommand::Text {
                    x: cx, y: ey + 3.0,
                    text: format!("{:>5}", entry.line_number),
                    font_size: SMALL_TEXT,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(42.0),
                });
                cx += 46.0;
            }

            // Timestamp
            if self.show_timestamps && entry.timestamp > 0 {
                cmds.push(RenderCommand::Text {
                    x: cx, y: ey + 3.0,
                    text: entry.timestamp_display(),
                    font_size: SMALL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(100.0),
                });
                cx += 104.0;
            }

            // Level badge
            let level_label = entry.level.short_label();
            let level_w = (level_label.len() as f32) * 7.0 + 8.0;
            cmds.push(RenderCommand::FillRect {
                x: cx, y: ey + 2.0,
                width: level_w, height: 16.0,
                color: entry.level.color(),
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 4.0, y: ey + 4.0,
                text: level_label.into(),
                font_size: 10.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(level_w),
            });
            cx += level_w + 6.0;

            // Source
            if self.show_source && !entry.source.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: cx, y: ey + 3.0,
                    text: format!("[{}]", entry.source),
                    font_size: SMALL_TEXT,
                    color: SKY,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(100.0),
                });
                cx += (entry.source.len() as f32) * 7.0 + 20.0;
            }

            // Message
            let msg_width = WINDOW_WIDTH - cx - PADDING;
            let display_msg: String = entry.message.chars().take((msg_width / CHAR_WIDTH) as usize).collect();
            cmds.push(RenderCommand::Text {
                x: cx, y: ey + 3.0,
                text: display_msg,
                font_size: NORMAL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(msg_width),
            });
        }

        if entries.is_empty() {
            cmds.push(RenderCommand::Text {
                x: WINDOW_WIDTH / 2.0 - 80.0, y: y + height / 2.0,
                text: "No log entries match filters".into(),
                font_size: NORMAL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
            });
        }
    }

    fn render_stats(&self, cmds: &mut Vec<RenderCommand>, y: f32, _height: f32) {
        if let Some(log) = self.active_log() {
            let counts = log.level_counts();
            let total = log.entries.len();

            // Level distribution
            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0, y: y + 16.0,
                text: "Level Distribution".into(),
                font_size: HEADER_TEXT,
                color: BLUE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
            });

            let bar_max_w = 400.0;
            for (i, (level, count)) in counts.iter().enumerate() {
                let sy = y + 44.0 + (i as f32) * 32.0;
                let pct = if total > 0 { (*count as f32) / (total as f32) } else { 0.0 };
                let bar_w = pct * bar_max_w;

                // Label
                cmds.push(RenderCommand::Text {
                    x: PADDING + 12.0, y: sy + 4.0,
                    text: level.label().into(),
                    font_size: NORMAL_TEXT,
                    color: level.color(),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(60.0),
                });

                // Bar
                cmds.push(RenderCommand::FillRect {
                    x: 100.0, y: sy + 2.0,
                    width: bar_w.max(2.0), height: 18.0,
                    color: level.color(),
                    corner_radii: CornerRadii::all(3.0),
                });

                // Count
                cmds.push(RenderCommand::Text {
                    x: 100.0 + bar_w + 8.0, y: sy + 4.0,
                    text: format!("{count} ({:.1}%)", pct * 100.0),
                    font_size: SMALL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                });
            }

            // Top sources
            let sources_y = y + 250.0;
            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0, y: sources_y,
                text: "Top Sources".into(),
                font_size: HEADER_TEXT,
                color: TEAL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
            });

            for (si, (source, count)) in log.top_sources(10).iter().enumerate() {
                let sy = sources_y + 28.0 + (si as f32) * 24.0;
                cmds.push(RenderCommand::Text {
                    x: PADDING + 20.0, y: sy,
                    text: source.clone(),
                    font_size: NORMAL_TEXT,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                });
                cmds.push(RenderCommand::Text {
                    x: 250.0, y: sy,
                    text: format!("{count}"),
                    font_size: NORMAL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
                });
            }

            // Summary stats on right
            let stats_x = WINDOW_WIDTH / 2.0 + 40.0;
            cmds.push(RenderCommand::Text {
                x: stats_x, y: y + 16.0,
                text: "Summary".into(),
                font_size: HEADER_TEXT,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
            });

            let summary_items = [
                ("Total entries", format!("{total}")),
                ("Sources", format!("{}", log.unique_sources().len())),
                ("Errors", format!("{}", counts.iter().find(|(l, _)| *l == LogLevel::Error).map_or(0, |(_, c)| *c))),
                ("Warnings", format!("{}", counts.iter().find(|(l, _)| *l == LogLevel::Warn).map_or(0, |(_, c)| *c))),
                ("Format", if log.is_json { "JSON-lines" } else { "Plain text" }.into()),
                ("Bookmarks", format!("{}", log.entries.iter().filter(|e| e.bookmarked).count())),
            ];

            for (si, (label, value)) in summary_items.iter().enumerate() {
                let sy = y + 44.0 + (si as f32) * 26.0;
                cmds.push(RenderCommand::Text {
                    x: stats_x, y: sy,
                    text: (*label).into(),
                    font_size: NORMAL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                });
                cmds.push(RenderCommand::Text {
                    x: stats_x + 140.0, y: sy,
                    text: value.clone(),
                    font_size: NORMAL_TEXT,
                    color: TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                });
            }
        }
    }

    fn render_detail(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        if let Some(idx) = self.selected_entry {
            if let Some(log) = self.active_log() {
                if let Some(entry) = log.entries.get(idx) {
                    let panel_w = WINDOW_WIDTH - 2.0 * PADDING;

                    // Header
                    cmds.push(RenderCommand::FillRect {
                        x: PADDING, y: y + PADDING,
                        width: panel_w, height: 50.0,
                        color: MANTLE,
                        corner_radii: CornerRadii { top_left: 8.0, top_right: 8.0, bottom_left: 0.0, bottom_right: 0.0 },
                    });

                    // Level badge
                    let level_w = (entry.level.label().len() as f32) * 8.0 + 16.0;
                    cmds.push(RenderCommand::FillRect {
                        x: PADDING + 12.0, y: y + PADDING + 10.0,
                        width: level_w, height: 24.0,
                        color: entry.level.color(),
                        corner_radii: CornerRadii::all(4.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 20.0, y: y + PADDING + 14.0,
                        text: entry.level.label().into(),
                        font_size: NORMAL_TEXT,
                        color: CRUST,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(level_w),
                    });

                    // Source and time
                    cmds.push(RenderCommand::Text {
                        x: PADDING + level_w + 20.0, y: y + PADDING + 14.0,
                        text: format!("[{}] at {}", entry.source, entry.timestamp_display()),
                        font_size: NORMAL_TEXT,
                        color: SUBTEXT1,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(400.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: PADDING + 12.0, y: y + PADDING + 38.0,
                        text: format!("Line {}", entry.line_number),
                        font_size: SMALL_TEXT,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(100.0),
                    });

                    // Message body
                    let body_y = y + PADDING + 54.0;
                    cmds.push(RenderCommand::FillRect {
                        x: PADDING, y: body_y,
                        width: panel_w, height: height - 80.0,
                        color: CRUST,
                        corner_radii: CornerRadii { top_left: 0.0, top_right: 0.0, bottom_left: 8.0, bottom_right: 8.0 },
                    });

                    // Message
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 16.0, y: body_y + 12.0,
                        text: "Message:".into(),
                        font_size: SMALL_TEXT,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(100.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 16.0, y: body_y + 30.0,
                        text: entry.message.clone(),
                        font_size: NORMAL_TEXT,
                        color: TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(panel_w - 32.0),
                    });

                    // Extra fields
                    if !entry.fields.is_empty() {
                        let fields_y = body_y + 60.0;
                        cmds.push(RenderCommand::Text {
                            x: PADDING + 16.0, y: fields_y,
                            text: "Fields:".into(),
                            font_size: SMALL_TEXT,
                            color: SUBTEXT0,
                            font_weight: FontWeightHint::Bold,
                            max_width: Some(100.0),
                        });

                        for (fi, (key, value)) in entry.fields.iter().enumerate() {
                            let fy = fields_y + 20.0 + (fi as f32) * LINE_HEIGHT;
                            cmds.push(RenderCommand::Text {
                                x: PADDING + 24.0, y: fy,
                                text: format!("{key}:"),
                                font_size: SMALL_TEXT,
                                color: TEAL,
                                font_weight: FontWeightHint::Bold,
                                max_width: Some(150.0),
                            });
                            cmds.push(RenderCommand::Text {
                                x: PADDING + 180.0, y: fy,
                                text: value.clone(),
                                font_size: SMALL_TEXT,
                                color: TEXT,
                                font_weight: FontWeightHint::Regular,
                                max_width: Some(panel_w - 200.0),
                            });
                        }
                    }

                    // Raw JSON
                    let raw_y = body_y + 120.0 + (entry.fields.len() as f32) * LINE_HEIGHT;
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 16.0, y: raw_y,
                        text: "Raw:".into(),
                        font_size: SMALL_TEXT,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(100.0),
                    });
                    cmds.push(RenderCommand::FillRect {
                        x: PADDING + 16.0, y: raw_y + 18.0,
                        width: panel_w - 32.0, height: LINE_HEIGHT + 8.0,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: PADDING + 24.0, y: raw_y + 22.0,
                        text: truncate_str(&entry.raw, 120),
                        font_size: SMALL_TEXT,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(panel_w - 48.0),
                    });

                    return;
                }
            }
        }

        // No selection
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH / 2.0 - 100.0, y: y + height / 2.0,
            text: "Select a log entry to view details".into(),
            font_size: NORMAL_TEXT,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0, y,
            width: WINDOW_WIDTH, height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let entries = self.filtered_entries();
        let total = self.active_log().map_or(0, |l| l.entries.len());

        // Entry count
        cmds.push(RenderCommand::Text {
            x: PADDING, y: y + 5.0,
            text: format!("{} / {} entries", entries.len(), total),
            font_size: SMALL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // File info
        if let Some(log) = self.active_log() {
            cmds.push(RenderCommand::Text {
                x: 200.0, y: y + 5.0,
                text: log.path.clone(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
            });

            // Format
            cmds.push(RenderCommand::Text {
                x: WINDOW_WIDTH - 120.0, y: y + 5.0,
                text: if log.is_json { "JSON-lines" } else { "Plain text" }.into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
        }
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max { s.into() }
    else {
        let t: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{t}...")
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let app = App::new();
    let _cmds = app.render();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
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
            line_number: 1, timestamp: 3_661_500, level: LogLevel::Info,
            source: String::new(), message: String::new(), fields: Vec::new(),
            raw: String::new(), bookmarked: false,
        };
        assert_eq!(entry.timestamp_display(), "01:01:01.500");
    }

    #[test]
    fn test_timestamp_display_zero() {
        let entry = LogEntry {
            line_number: 1, timestamp: 0, level: LogLevel::Info,
            source: String::new(), message: String::new(), fields: Vec::new(),
            raw: String::new(), bookmarked: false,
        };
        assert_eq!(entry.timestamp_display(), "00:00:00.000");
    }

    // --- LogFile tests ---

    #[test]
    fn test_log_file_parse_json() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content(r#"{"level":"INFO","message":"hello"}
{"level":"ERROR","message":"fail"}"#);
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
        file.parse_content(r#"{"level":"INFO","message":"a"}
{"level":"INFO","message":"b"}
{"level":"ERROR","message":"c"}"#);
        let counts = file.level_counts();
        let info_count = counts.iter().find(|(l, _)| *l == LogLevel::Info).map_or(0, |(_, c)| *c);
        assert_eq!(info_count, 2);
    }

    #[test]
    fn test_log_file_unique_sources() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content(r#"{"level":"INFO","source":"a","message":"x"}
{"level":"INFO","source":"b","message":"y"}
{"level":"INFO","source":"a","message":"z"}"#);
        let sources = file.unique_sources();
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn test_log_file_top_sources() {
        let mut file = LogFile::new("test", "/test");
        file.parse_content(r#"{"level":"INFO","source":"a","message":"1"}
{"level":"INFO","source":"a","message":"2"}
{"level":"INFO","source":"b","message":"3"}"#);
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
        let filter = FilterState { min_level: LogLevel::Warn, ..Default::default() };
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
        app.filter.search_query = "info".into();
        app.update_search();
        let count = app.search_results.len();
        assert!(count > 0);

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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_stats_view() {
        let mut app = App::new();
        app.view_mode = ViewMode::Stats;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_detail_view() {
        let mut app = App::new();
        app.view_mode = ViewMode::Detail;
        app.selected_entry = Some(0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_detail_no_selection() {
        let mut app = App::new();
        app.view_mode = ViewMode::Detail;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_view_mode_label() {
        assert_eq!(ViewMode::List.label(), "Log View");
        assert_eq!(ViewMode::Stats.label(), "Statistics");
    }

    // --- Utility tests ---

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate_str("a very long string indeed", 10);
        assert!(result.ends_with("..."));
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
}
