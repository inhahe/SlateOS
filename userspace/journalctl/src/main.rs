//! SlateOS Journal Log Viewer (`journalctl`)
//!
//! Reads, filters, and manages structured JSON-lines log files stored under
//! `/var/log/journal/`. Compatible with our syslogd's output format.
//!
//! Each journal entry is a single JSON-lines record:
//! ```json
//! {"ts":1716000000,"level":"info","service":"net.dhcp","msg":"lease renewed",
//!  "boot_id":"abc123","pid":42}
//! ```
//!
//! # Usage
//!
//! ```text
//! journalctl                         Show all journal entries
//! journalctl -u <unit>               Filter by unit/service name
//! journalctl -p <priority>           Filter by priority (0-7 or name)
//! journalctl --since <datetime>      Show entries since datetime
//! journalctl --until <datetime>      Show entries until datetime
//! journalctl -f                      Follow mode (live tail)
//! journalctl -r                      Reverse output (newest first)
//! journalctl -o <format>             Output format: short, short-precise, json,
//!                                      json-pretty, cat, verbose
//! journalctl -b [id]                 Show entries from boot id
//! journalctl -k / --dmesg            Show kernel messages only
//! journalctl -n <count>              Show last N entries (default 10)
//! journalctl --grep <pattern>        Filter by regex/substring in message
//! journalctl --list-fields           List all known field names
//! journalctl --disk-usage            Show journal disk usage
//! journalctl --vacuum-time <time>    Remove entries older than time (e.g. 2d, 1w)
//! journalctl --vacuum-size <size>    Shrink journal to at most size (e.g. 100M)
//! journalctl --no-pager              Do not pipe through pager
//! journalctl --no-color              Disable colored output
//! ```

#![cfg_attr(not(test), no_main)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const JOURNAL_DIR: &str = "/var/log/journal";
/// Fallback log paths when journal dir does not exist.
const FALLBACK_PATHS: &[&str] = &["/var/log/syslog.jsonl", "/var/log/syslog"];

// ============================================================================
// Priority levels (RFC 5424 / syslog compatible)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Priority {
    Emergency = 0,
    Alert = 1,
    Critical = 2,
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl Priority {
    fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "emerg" | "emergency" | "0" => Some(Self::Emergency),
            "alert" | "1" => Some(Self::Alert),
            "crit" | "critical" | "2" => Some(Self::Critical),
            "err" | "error" | "3" => Some(Self::Error),
            "warning" | "warn" | "4" => Some(Self::Warning),
            "notice" | "5" => Some(Self::Notice),
            "info" | "6" => Some(Self::Info),
            "debug" | "7" => Some(Self::Debug),
            _ => None,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Emergency,
            1 => Self::Alert,
            2 => Self::Critical,
            3 => Self::Error,
            4 => Self::Warning,
            5 => Self::Notice,
            6 => Self::Info,
            7 => Self::Debug,
            _ => Self::Debug,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Emergency => "emerg",
            Self::Alert => "alert",
            Self::Critical => "crit",
            Self::Error => "err",
            Self::Warning => "warning",
            Self::Notice => "notice",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Emergency => "EMERGENCY",
            Self::Alert => "ALERT",
            Self::Critical => "CRITICAL",
            Self::Error => "ERROR",
            Self::Warning => "WARNING",
            Self::Notice => "NOTICE",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }

    fn ansi_color(self) -> &'static str {
        match self {
            Self::Emergency => "\x1b[1;41;37m", // bold white on red bg
            Self::Alert => "\x1b[1;31m",         // bold red
            Self::Critical => "\x1b[31m",        // red
            Self::Error => "\x1b[91m",           // bright red
            Self::Warning => "\x1b[33m",         // yellow
            Self::Notice => "\x1b[1;37m",        // bold white
            Self::Info => "",                     // default
            Self::Debug => "\x1b[90m",           // dim gray
        }
    }
}

// ============================================================================
// Journal entry
// ============================================================================

/// A single structured journal log entry parsed from JSON-lines.
#[derive(Debug, Clone)]
struct JournalEntry {
    /// Unix timestamp in seconds.
    timestamp: u64,
    /// Microsecond component (for short-precise output).
    timestamp_usec: u64,
    /// Severity / priority level.
    priority: Priority,
    /// Service or unit name (e.g. "net.dhcp", "kernel").
    unit: String,
    /// Log message text.
    message: String,
    /// Boot identifier string.
    boot_id: String,
    /// Process ID (0 if unknown).
    pid: u64,
    /// All key-value fields from the original JSON (preserves extras).
    fields: BTreeMap<String, String>,
}

impl JournalEntry {
    /// Parse from a single JSON-lines record.
    fn from_json_line(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
            return None;
        }

        let fields = parse_json_object(trimmed)?;

        let timestamp = fields
            .get("ts")
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| {
                fields
                    .get("__REALTIME_TIMESTAMP")
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|us| us / 1_000_000)
            })
            .unwrap_or(0);

        let timestamp_usec = fields
            .get("ts_usec")
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| {
                fields
                    .get("__REALTIME_TIMESTAMP")
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|us| us % 1_000_000)
            })
            .unwrap_or(0);

        let priority_str = fields.get("level").or_else(|| fields.get("PRIORITY"));
        let priority = priority_str
            .and_then(|s| {
                Priority::from_name(s).or_else(|| s.parse::<u8>().ok().map(Priority::from_u8))
            })
            .unwrap_or(Priority::Info);

        let unit = fields
            .get("service")
            .or_else(|| fields.get("_SYSTEMD_UNIT"))
            .or_else(|| fields.get("SYSLOG_IDENTIFIER"))
            .or_else(|| fields.get("unit"))
            .cloned()
            .unwrap_or_default();

        let message = fields
            .get("msg")
            .or_else(|| fields.get("MESSAGE"))
            .cloned()
            .unwrap_or_default();

        let boot_id = fields
            .get("boot_id")
            .or_else(|| fields.get("_BOOT_ID"))
            .cloned()
            .unwrap_or_default();

        let pid = fields
            .get("pid")
            .or_else(|| fields.get("_PID"))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        Some(JournalEntry {
            timestamp,
            timestamp_usec,
            priority,
            unit,
            message,
            boot_id,
            pid,
            fields,
        })
    }

    /// Serialize back to JSON-lines format.
    fn to_json(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("\"ts\":{}", self.timestamp));
        if self.timestamp_usec != 0 {
            parts.push(format!("\"ts_usec\":{}", self.timestamp_usec));
        }
        parts.push(format!("\"level\":\"{}\"", json_escape(self.priority.name())));
        if !self.unit.is_empty() {
            parts.push(format!("\"service\":\"{}\"", json_escape(&self.unit)));
        }
        parts.push(format!("\"msg\":\"{}\"", json_escape(&self.message)));
        if !self.boot_id.is_empty() {
            parts.push(format!("\"boot_id\":\"{}\"", json_escape(&self.boot_id)));
        }
        if self.pid != 0 {
            parts.push(format!("\"pid\":{}", self.pid));
        }
        // Include extra fields not already serialized.
        let known_keys: &[&str] = &[
            "ts", "ts_usec", "level", "service", "msg", "boot_id", "pid", "time",
            "__REALTIME_TIMESTAMP", "PRIORITY", "_SYSTEMD_UNIT", "SYSLOG_IDENTIFIER",
            "unit", "MESSAGE", "_BOOT_ID", "_PID",
        ];
        for (k, v) in &self.fields {
            if !known_keys.contains(&k.as_str()) {
                parts.push(format!("\"{}\":\"{}\"", json_escape(k), json_escape(v)));
            }
        }
        format!("{{{}}}", parts.join(","))
    }

    /// Serialize to pretty-printed JSON.
    fn to_json_pretty(&self) -> String {
        let mut lines = Vec::new();
        lines.push("{".to_string());
        lines.push(format!("    \"ts\": {},", self.timestamp));
        if self.timestamp_usec != 0 {
            lines.push(format!("    \"ts_usec\": {},", self.timestamp_usec));
        }
        lines.push(format!(
            "    \"level\": \"{}\",",
            json_escape(self.priority.name())
        ));
        if !self.unit.is_empty() {
            lines.push(format!(
                "    \"service\": \"{}\",",
                json_escape(&self.unit)
            ));
        }
        lines.push(format!("    \"msg\": \"{}\",", json_escape(&self.message)));
        if !self.boot_id.is_empty() {
            lines.push(format!(
                "    \"boot_id\": \"{}\",",
                json_escape(&self.boot_id)
            ));
        }
        if self.pid != 0 {
            lines.push(format!("    \"pid\": {},", self.pid));
        }
        let known_keys: &[&str] = &[
            "ts", "ts_usec", "level", "service", "msg", "boot_id", "pid", "time",
            "__REALTIME_TIMESTAMP", "PRIORITY", "_SYSTEMD_UNIT", "SYSLOG_IDENTIFIER",
            "unit", "MESSAGE", "_BOOT_ID", "_PID",
        ];
        let extras: Vec<_> = self
            .fields
            .iter()
            .filter(|(k, _)| !known_keys.contains(&k.as_str()))
            .collect();
        for (k, v) in &extras {
            lines.push(format!(
                "    \"{}\": \"{}\",",
                json_escape(k),
                json_escape(v)
            ));
        }
        // Remove trailing comma from last field line.
        if let Some(last) = lines.last_mut()
            && last.ends_with(',') {
                last.pop();
            }
        lines.push("}".to_string());
        lines.join("\n")
    }
}

// ============================================================================
// Minimal JSON parser (no external deps)
// ============================================================================

/// Parse a flat JSON object into key-value pairs.
/// Handles string and numeric values. Does not handle nested objects or arrays.
fn parse_json_object(json: &str) -> Option<BTreeMap<String, String>> {
    let trimmed = json.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut map = BTreeMap::new();
    let mut pos = 0;
    let bytes = inner.as_bytes();

    while pos < bytes.len() {
        // Skip whitespace and commas.
        while pos < bytes.len()
            && (bytes[pos] == b' '
                || bytes[pos] == b','
                || bytes[pos] == b'\t'
                || bytes[pos] == b'\n'
                || bytes[pos] == b'\r')
        {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // Parse key (must be a string).
        let key = match parse_json_string_value(inner, &mut pos) {
            Some(k) => k,
            None => break,
        };

        // Skip to colon.
        while pos < bytes.len() && bytes[pos] != b':' {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        pos += 1; // skip ':'

        // Skip whitespace.
        while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // Parse value (string or number/bool/null).
        let value = if bytes[pos] == b'"' {
            match parse_json_string_value(inner, &mut pos) {
                Some(v) => v,
                None => break,
            }
        } else {
            let start = pos;
            while pos < bytes.len() && bytes[pos] != b',' && bytes[pos] != b'}' {
                pos += 1;
            }
            inner[start..pos].trim().to_string()
        };

        map.insert(key, value);
    }

    Some(map)
}

/// Parse a JSON string starting at `pos` (which should point to the opening `"`).
fn parse_json_string_value(s: &str, pos: &mut usize) -> Option<String> {
    let bytes = s.as_bytes();
    if *pos >= bytes.len() || bytes[*pos] != b'"' {
        return None;
    }
    *pos += 1; // skip opening "

    let mut result = String::new();
    while *pos < bytes.len() {
        if bytes[*pos] == b'\\' && *pos + 1 < bytes.len() {
            match bytes[*pos + 1] {
                b'"' => {
                    result.push('"');
                    *pos += 2;
                }
                b'\\' => {
                    result.push('\\');
                    *pos += 2;
                }
                b'n' => {
                    result.push('\n');
                    *pos += 2;
                }
                b'r' => {
                    result.push('\r');
                    *pos += 2;
                }
                b't' => {
                    result.push('\t');
                    *pos += 2;
                }
                b'/' => {
                    result.push('/');
                    *pos += 2;
                }
                b'u' => {
                    // \uXXXX — parse 4 hex digits.
                    if *pos + 5 < bytes.len() {
                        let hex = &s[*pos + 2..*pos + 6];
                        if let Ok(code) = u32::from_str_radix(hex, 16)
                            && let Some(ch) = char::from_u32(code) {
                                result.push(ch);
                            }
                        *pos += 6;
                    } else {
                        *pos += 2;
                    }
                }
                _ => {
                    result.push(bytes[*pos + 1] as char);
                    *pos += 2;
                }
            }
        } else if bytes[*pos] == b'"' {
            *pos += 1; // skip closing "
            return Some(result);
        } else {
            result.push(bytes[*pos] as char);
            *pos += 1;
        }
    }
    // Unterminated string -- return what we have.
    Some(result)
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                // Use write! to a String -- infallible, no error to handle.
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Timestamp formatting and parsing
// ============================================================================

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> i64 {
    match month {
        1 => 31,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 30,
    }
}

/// Convert unix timestamp to "YYYY-MM-DD HH:MM:SS" string.
fn format_timestamp(unix_secs: u64) -> String {
    if unix_secs == 0 {
        return "0000-00-00 00:00:00".to_string();
    }

    let secs = unix_secs;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let mut remaining_days = (secs / 86400) as i64;
    let mut year: i64 = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let mut month = 1u32;
    loop {
        let dim = days_in_month(year, month);
        if remaining_days < dim {
            break;
        }
        remaining_days -= dim;
        month += 1;
    }
    let day = remaining_days + 1;

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

/// Format with microsecond precision: "YYYY-MM-DD HH:MM:SS.UUUUUU".
fn format_timestamp_precise(unix_secs: u64, usec: u64) -> String {
    let base = format_timestamp(unix_secs);
    format!("{base}.{usec:06}")
}

/// Parse a datetime string into a unix timestamp.
///
/// Supported formats:
/// - "YYYY-MM-DD HH:MM:SS"
/// - "YYYY-MM-DD"
/// - "today"
/// - "yesterday"
/// - "-Nd" / "-Nh" / "-Nm" (relative: N days/hours/minutes ago)
fn parse_datetime(s: &str) -> Option<u64> {
    let s = s.trim();

    if s.eq_ignore_ascii_case("now") {
        return Some(now_secs());
    }
    if s.eq_ignore_ascii_case("today") {
        let now = now_secs();
        // Round down to midnight.
        return Some(now - (now % 86400));
    }
    if s.eq_ignore_ascii_case("yesterday") {
        let now = now_secs();
        return Some(now - (now % 86400) - 86400);
    }

    // Relative: -Nd, -Nh, -Nm, -Ns
    if s.starts_with('-') && s.len() >= 3 {
        let suffix = s.as_bytes()[s.len() - 1];
        let num_str = &s[1..s.len() - 1];
        if let Ok(n) = num_str.parse::<u64>() {
            let secs = match suffix {
                b'd' => n * 86400,
                b'h' => n * 3600,
                b'm' => n * 60,
                b's' => n,
                _ => return None,
            };
            return Some(now_secs().saturating_sub(secs));
        }
    }

    // "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DD"
    let parts: Vec<&str> = s.splitn(2, ' ').collect();
    let date_part = parts.first()?;
    let time_part = if parts.len() > 1 {
        Some(parts[1])
    } else {
        None
    };

    let date_fields: Vec<&str> = date_part.split('-').collect();
    if date_fields.len() != 3 {
        return None;
    }
    let year: i64 = date_fields[0].parse().ok()?;
    let month: u32 = date_fields[1].parse().ok()?;
    let day: u32 = date_fields[2].parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (hours, minutes, seconds) = if let Some(t) = time_part {
        let tf: Vec<&str> = t.split(':').collect();
        let h: u64 = tf.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        let m: u64 = tf.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let s: u64 = tf.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
        (h, m, s)
    } else {
        (0, 0, 0)
    };

    // Convert to unix timestamp (UTC).
    let mut total_days: i64 = 0;
    for y in 1970..year {
        total_days += if is_leap_year(y) { 366 } else { 365 };
    }
    for m in 1..month {
        total_days += days_in_month(year, m);
    }
    total_days += (day as i64) - 1;

    let ts = (total_days as u64) * 86400 + hours * 3600 + minutes * 60 + seconds;
    Some(ts)
}

// ============================================================================
// Duration / size parsing (for --vacuum-time, --vacuum-size)
// ============================================================================

/// Parse a duration string like "2d", "1w", "3h", "30m", "7200s", "1M", "1y"
/// into seconds.
fn parse_duration_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let last = s.as_bytes()[s.len() - 1];
    let num_str = &s[..s.len() - 1];
    let n: u64 = num_str.parse().ok()?;

    match last {
        b's' => Some(n),
        b'm' => Some(n * 60),
        b'h' => Some(n * 3600),
        b'd' => Some(n * 86400),
        b'w' => Some(n * 7 * 86400),
        b'M' => Some(n * 30 * 86400),
        b'y' => Some(n * 365 * 86400),
        _ => {
            // Maybe the whole string is just a number (seconds).
            s.parse::<u64>().ok()
        }
    }
}

/// Parse a size string like "100M", "1G", "500K" into bytes.
fn parse_size_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let last = s.as_bytes()[s.len() - 1];
    if last.is_ascii_digit() {
        return s.parse::<u64>().ok();
    }
    let num_str = &s[..s.len() - 1];
    let n: u64 = num_str.parse().ok()?;

    match last {
        b'B' | b'b' => Some(n),
        b'K' | b'k' => Some(n * 1024),
        b'M' => Some(n * 1024 * 1024),
        b'G' | b'g' => Some(n * 1024 * 1024 * 1024),
        _ => None,
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ============================================================================
// Journal file discovery and reading
// ============================================================================

/// Discover all journal files under the journal directory.
fn discover_journal_files() -> Vec<PathBuf> {
    let journal_path = Path::new(JOURNAL_DIR);
    let mut files = Vec::new();

    if journal_path.is_dir() {
        collect_jsonl_files(journal_path, &mut files);
    }

    // If no journal files found, try fallback paths.
    if files.is_empty() {
        for path_str in FALLBACK_PATHS {
            let p = Path::new(path_str);
            if p.is_file() {
                files.push(p.to_path_buf());
            }
        }
    }

    files.sort();
    files
}

/// Recursively collect .jsonl and .log files from a directory.
fn collect_jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.is_file() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if name.ends_with(".jsonl")
                || name.ends_with(".log")
                || name.ends_with(".journal")
            {
                out.push(path);
            }
        }
    }
}

/// Read all journal entries from all discovered files.
fn read_all_entries() -> Vec<JournalEntry> {
    let files = discover_journal_files();
    let mut entries = Vec::new();

    for file in &files {
        if let Ok(content) = fs::read_to_string(file) {
            for line in content.lines() {
                if let Some(entry) = JournalEntry::from_json_line(line) {
                    entries.push(entry);
                }
            }
        }
    }

    // Sort by timestamp.
    entries.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then(a.timestamp_usec.cmp(&b.timestamp_usec))
    });

    entries
}

/// Compute total disk usage of all journal files.
fn journal_disk_usage() -> (usize, u64) {
    let files = discover_journal_files();
    let mut total_bytes: u64 = 0;
    let mut count = 0usize;

    for file in &files {
        if let Ok(meta) = fs::metadata(file) {
            total_bytes += meta.len();
            count += 1;
        }
    }

    (count, total_bytes)
}

// ============================================================================
// Substring matching (simple pattern -- not full regex, but handles basic cases)
// ============================================================================

/// Check if `haystack` contains `pattern` (case-insensitive simple substring match).
/// Supports basic patterns: literal substring matching.
fn pattern_matches(haystack: &str, pattern: &str) -> bool {
    let h = haystack.to_ascii_lowercase();
    let p = pattern.to_ascii_lowercase();
    h.contains(&p)
}

// ============================================================================
// Output format
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Short,
    ShortPrecise,
    Json,
    JsonPretty,
    Cat,
    Verbose,
}

impl OutputFormat {
    fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "short" => Some(Self::Short),
            "short-precise" => Some(Self::ShortPrecise),
            "json" => Some(Self::Json),
            "json-pretty" => Some(Self::JsonPretty),
            "cat" => Some(Self::Cat),
            "verbose" => Some(Self::Verbose),
            _ => None,
        }
    }
}

// ============================================================================
// Configuration / command-line state
// ============================================================================

struct Config {
    /// Filter by unit/service name (substring match).
    unit_filter: Option<String>,
    /// Filter by max priority level (show this level and more severe).
    priority_filter: Option<Priority>,
    /// Show entries since this timestamp.
    since: Option<u64>,
    /// Show entries until this timestamp.
    until: Option<u64>,
    /// Follow mode (like tail -f).
    follow: bool,
    /// Reverse output (newest first).
    reverse: bool,
    /// Output format.
    output_format: OutputFormat,
    /// Filter by boot ID.
    boot_filter: Option<String>,
    /// Show only kernel messages.
    dmesg: bool,
    /// Number of entries to show (None = all).
    num_entries: Option<usize>,
    /// Grep / pattern filter on message content.
    grep_pattern: Option<String>,
    /// Use colored output.
    color: bool,

    // Action flags (mutually exclusive with normal display).
    list_fields: bool,
    disk_usage: bool,
    vacuum_time: Option<u64>,
    vacuum_size: Option<u64>,
    show_help: bool,
}

impl Config {
    fn new() -> Self {
        Config {
            unit_filter: None,
            priority_filter: None,
            since: None,
            until: None,
            follow: false,
            reverse: false,
            output_format: OutputFormat::Short,
            boot_filter: None,
            dmesg: false,
            num_entries: None,
            grep_pattern: None,
            color: true,
            list_fields: false,
            disk_usage: false,
            vacuum_time: None,
            vacuum_size: None,
            show_help: false,
        }
    }
}

/// Parse command-line arguments into a Config.
/// Returns Err(message) on invalid arguments.
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::new();
    let mut i = 1; // skip argv[0]

    while i < args.len() {
        match args[i].as_str() {
            "-u" | "--unit" => {
                if i + 1 >= args.len() {
                    return Err("-u requires a unit name".to_string());
                }
                cfg.unit_filter = Some(args[i + 1].clone());
                i += 2;
            }
            "-p" | "--priority" => {
                if i + 1 >= args.len() {
                    return Err("-p requires a priority level".to_string());
                }
                let prio = Priority::from_name(&args[i + 1])
                    .ok_or_else(|| format!("unknown priority: {}", args[i + 1]))?;
                cfg.priority_filter = Some(prio);
                i += 2;
            }
            "--since" => {
                if i + 1 >= args.len() {
                    return Err("--since requires a datetime".to_string());
                }
                // Peek: might be two-part datetime "YYYY-MM-DD HH:MM:SS"
                let datetime_str = if i + 2 < args.len()
                    && args[i + 2].contains(':')
                    && !args[i + 2].starts_with('-')
                {
                    let combined = format!("{} {}", args[i + 1], args[i + 2]);
                    i += 3;
                    combined
                } else {
                    i += 2;
                    args[i - 1].clone()
                };
                let ts = parse_datetime(&datetime_str)
                    .ok_or_else(|| format!("cannot parse datetime: {datetime_str}"))?;
                cfg.since = Some(ts);
            }
            "--until" => {
                if i + 1 >= args.len() {
                    return Err("--until requires a datetime".to_string());
                }
                let datetime_str = if i + 2 < args.len()
                    && args[i + 2].contains(':')
                    && !args[i + 2].starts_with('-')
                {
                    let combined = format!("{} {}", args[i + 1], args[i + 2]);
                    i += 3;
                    combined
                } else {
                    i += 2;
                    args[i - 1].clone()
                };
                let ts = parse_datetime(&datetime_str)
                    .ok_or_else(|| format!("cannot parse datetime: {datetime_str}"))?;
                cfg.until = Some(ts);
            }
            "-f" | "--follow" => {
                cfg.follow = true;
                i += 1;
            }
            "-r" | "--reverse" => {
                cfg.reverse = true;
                i += 1;
            }
            "-o" | "--output" => {
                if i + 1 >= args.len() {
                    return Err("-o requires a format name".to_string());
                }
                cfg.output_format = OutputFormat::from_name(&args[i + 1])
                    .ok_or_else(|| format!("unknown output format: {}", args[i + 1]))?;
                i += 2;
            }
            "-b" | "--boot" => {
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    cfg.boot_filter = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    // Current boot: empty string means "latest boot_id"
                    cfg.boot_filter = Some(String::new());
                    i += 1;
                }
            }
            "-k" | "--dmesg" => {
                cfg.dmesg = true;
                i += 1;
            }
            "-n" | "--lines" => {
                if i + 1 >= args.len() {
                    return Err("-n requires a count".to_string());
                }
                let n: usize = args[i + 1]
                    .parse()
                    .map_err(|_| format!("invalid count: {}", args[i + 1]))?;
                cfg.num_entries = Some(n);
                i += 2;
            }
            "--grep" => {
                if i + 1 >= args.len() {
                    return Err("--grep requires a pattern".to_string());
                }
                cfg.grep_pattern = Some(args[i + 1].clone());
                i += 2;
            }
            "--no-color" | "--nocolor" => {
                cfg.color = false;
                i += 1;
            }
            "--no-pager" => {
                // We don't implement a pager; accept and ignore.
                i += 1;
            }
            "--list-fields" => {
                cfg.list_fields = true;
                i += 1;
            }
            "--disk-usage" => {
                cfg.disk_usage = true;
                i += 1;
            }
            "--vacuum-time" => {
                if i + 1 >= args.len() {
                    return Err("--vacuum-time requires a duration (e.g. 2d, 1w)".to_string());
                }
                let secs = parse_duration_secs(&args[i + 1])
                    .ok_or_else(|| format!("invalid duration: {}", args[i + 1]))?;
                cfg.vacuum_time = Some(secs);
                i += 2;
            }
            "--vacuum-size" => {
                if i + 1 >= args.len() {
                    return Err("--vacuum-size requires a size (e.g. 100M, 1G)".to_string());
                }
                let bytes = parse_size_bytes(&args[i + 1])
                    .ok_or_else(|| format!("invalid size: {}", args[i + 1]))?;
                cfg.vacuum_size = Some(bytes);
                i += 2;
            }
            "-h" | "--help" | "help" => {
                cfg.show_help = true;
                i += 1;
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
    }

    // JSON/json-pretty output disables color.
    if cfg.output_format == OutputFormat::Json || cfg.output_format == OutputFormat::JsonPretty {
        cfg.color = false;
    }

    Ok(cfg)
}

// ============================================================================
// Filtering
// ============================================================================

fn apply_filters(entries: &[JournalEntry], cfg: &Config) -> Vec<JournalEntry> {
    let mut result: Vec<JournalEntry> = entries
        .iter()
        .filter(|e| {
            // Unit filter.
            if let Some(ref unit) = cfg.unit_filter {
                let u_lower = unit.to_ascii_lowercase();
                let entry_unit = e.unit.to_ascii_lowercase();
                if !entry_unit.contains(&u_lower) {
                    return false;
                }
            }

            // Priority filter: show entries at this level or more severe.
            if let Some(max_prio) = cfg.priority_filter
                && (e.priority as u8) > (max_prio as u8) {
                    return false;
                }

            // Since filter.
            if let Some(since) = cfg.since
                && e.timestamp < since {
                    return false;
                }

            // Until filter.
            if let Some(until) = cfg.until
                && e.timestamp > until {
                    return false;
                }

            // Boot filter.
            if let Some(ref boot) = cfg.boot_filter
                && !boot.is_empty() && e.boot_id != *boot {
                    return false;
                }
                // Empty boot_filter means "current boot" -- handled after
                // collecting entries (we pick the most recent boot_id).

            // Dmesg: only kernel messages.
            if cfg.dmesg {
                let u = e.unit.to_ascii_lowercase();
                if u != "kernel" && u != "kern" && u != "dmesg" {
                    return false;
                }
            }

            // Grep filter.
            if let Some(ref pattern) = cfg.grep_pattern
                && !pattern_matches(&e.message, pattern) {
                    return false;
                }

            true
        })
        .cloned()
        .collect();

    // Handle empty boot_filter (current boot = most recent boot_id).
    if let Some(ref boot) = cfg.boot_filter
        && boot.is_empty() {
            // Find the most recent boot_id.
            if let Some(latest_boot) = find_latest_boot_id(&result) {
                result.retain(|e| e.boot_id == latest_boot);
            }
        }

    // Reverse if requested.
    if cfg.reverse {
        result.reverse();
    }

    // Limit number of entries.
    if let Some(n) = cfg.num_entries
        && result.len() > n {
            if cfg.reverse {
                // Already reversed: take the first n.
                result.truncate(n);
            } else {
                // Take the last n entries.
                let start = result.len() - n;
                result = result.split_off(start);
            }
        }

    result
}

fn find_latest_boot_id(entries: &[JournalEntry]) -> Option<String> {
    entries
        .iter()
        .rev()
        .find(|e| !e.boot_id.is_empty())
        .map(|e| e.boot_id.clone())
}

// ============================================================================
// Output rendering
// ============================================================================

fn render_entry(entry: &JournalEntry, cfg: &Config) {
    match cfg.output_format {
        OutputFormat::Short => render_short(entry, cfg.color),
        OutputFormat::ShortPrecise => render_short_precise(entry, cfg.color),
        OutputFormat::Json => println!("{}", entry.to_json()),
        OutputFormat::JsonPretty => println!("{}", entry.to_json_pretty()),
        OutputFormat::Cat => println!("{}", entry.message),
        OutputFormat::Verbose => render_verbose(entry, cfg.color),
    }
}

fn render_short(entry: &JournalEntry, color: bool) {
    let ts = format_timestamp(entry.timestamp);
    let unit_str = if entry.unit.is_empty() {
        "unknown".to_string()
    } else {
        entry.unit.clone()
    };
    let pid_str = if entry.pid != 0 {
        format!("[{}]", entry.pid)
    } else {
        String::new()
    };

    if color {
        let c = entry.priority.ansi_color();
        let reset = if c.is_empty() { "" } else { "\x1b[0m" };
        println!("{ts} {unit_str}{pid_str}: {c}{}{reset}", entry.message);
    } else {
        println!("{ts} {unit_str}{pid_str}: {}", entry.message);
    }
}

fn render_short_precise(entry: &JournalEntry, color: bool) {
    let ts = format_timestamp_precise(entry.timestamp, entry.timestamp_usec);
    let unit_str = if entry.unit.is_empty() {
        "unknown".to_string()
    } else {
        entry.unit.clone()
    };
    let pid_str = if entry.pid != 0 {
        format!("[{}]", entry.pid)
    } else {
        String::new()
    };

    if color {
        let c = entry.priority.ansi_color();
        let reset = if c.is_empty() { "" } else { "\x1b[0m" };
        println!("{ts} {unit_str}{pid_str}: {c}{}{reset}", entry.message);
    } else {
        println!("{ts} {unit_str}{pid_str}: {}", entry.message);
    }
}

fn render_verbose(entry: &JournalEntry, color: bool) {
    let ts = format_timestamp_precise(entry.timestamp, entry.timestamp_usec);
    if color {
        let c = entry.priority.ansi_color();
        let reset = if c.is_empty() { "" } else { "\x1b[0m" };
        println!("{c}{ts} [{:<8}] {}{reset}", entry.priority.label(), ts);
    } else {
        println!("{ts} [{:<8}]", entry.priority.label());
    }
    println!("    _PRIORITY={}", entry.priority as u8);
    if !entry.unit.is_empty() {
        println!("    _UNIT={}", entry.unit);
    }
    if entry.pid != 0 {
        println!("    _PID={}", entry.pid);
    }
    if !entry.boot_id.is_empty() {
        println!("    _BOOT_ID={}", entry.boot_id);
    }
    println!("    MESSAGE={}", entry.message);

    let known_keys: &[&str] = &[
        "ts", "ts_usec", "level", "service", "msg", "boot_id", "pid", "time",
        "__REALTIME_TIMESTAMP", "PRIORITY", "_SYSTEMD_UNIT", "SYSLOG_IDENTIFIER",
        "unit", "MESSAGE", "_BOOT_ID", "_PID",
    ];
    for (k, v) in &entry.fields {
        if !known_keys.contains(&k.as_str()) {
            println!("    {k}={v}");
        }
    }
    println!();
}

// ============================================================================
// Action commands
// ============================================================================

fn cmd_list_fields() {
    let entries = read_all_entries();
    let mut field_names: BTreeSet<String> = BTreeSet::new();

    for entry in &entries {
        for key in entry.fields.keys() {
            field_names.insert(key.clone());
        }
        // Also include synthesized field names.
        field_names.insert("ts".to_string());
        field_names.insert("level".to_string());
        field_names.insert("service".to_string());
        field_names.insert("msg".to_string());
        field_names.insert("boot_id".to_string());
        field_names.insert("pid".to_string());
    }

    if field_names.is_empty() {
        println!("No journal entries found.");
        return;
    }

    println!("Known journal fields ({} total):", field_names.len());
    for name in &field_names {
        println!("  {name}");
    }
}

fn cmd_disk_usage() {
    let (file_count, total_bytes) = journal_disk_usage();
    println!(
        "Archived and active journals take up {} in {} file(s).",
        format_size(total_bytes),
        file_count,
    );
}

fn cmd_vacuum_time(max_age_secs: u64) {
    let cutoff = now_secs().saturating_sub(max_age_secs);
    let files = discover_journal_files();
    let mut total_removed = 0usize;
    let mut total_kept = 0usize;

    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut kept_lines = Vec::new();
        let mut removed = 0usize;

        for line in content.lines() {
            if let Some(entry) = JournalEntry::from_json_line(line) {
                if entry.timestamp >= cutoff {
                    kept_lines.push(line);
                } else {
                    removed += 1;
                }
            } else {
                // Preserve non-parseable lines.
                kept_lines.push(line);
            }
        }

        if removed > 0 {
            let new_content = if kept_lines.is_empty() {
                String::new()
            } else {
                let mut s = kept_lines.join("\n");
                s.push('\n');
                s
            };
            if fs::write(file, new_content).is_ok() {
                total_removed += removed;
                total_kept += kept_lines.len();
            }
        } else {
            total_kept += kept_lines.len();
        }
    }

    println!(
        "Vacuumed by time: removed {} entries, kept {} entries.",
        total_removed, total_kept
    );
}

fn cmd_vacuum_size(max_bytes: u64) {
    let files = discover_journal_files();
    let (_, current_total) = journal_disk_usage();

    if current_total <= max_bytes {
        println!(
            "Journal size ({}) is already within limit ({}).",
            format_size(current_total),
            format_size(max_bytes)
        );
        return;
    }

    let mut total_removed = 0usize;
    let bytes_to_remove = current_total - max_bytes;
    let mut bytes_removed: u64 = 0;

    // Process files oldest-first, removing oldest entries.
    for file in &files {
        if bytes_removed >= bytes_to_remove {
            break;
        }

        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        let mut keep_from = 0;

        for (idx, line) in lines.iter().enumerate() {
            if bytes_removed >= bytes_to_remove {
                break;
            }
            // Count bytes of this line plus the newline.
            bytes_removed += (line.len() as u64) + 1;
            total_removed += 1;
            keep_from = idx + 1;
        }

        let kept = &lines[keep_from..];
        let new_content = if kept.is_empty() {
            String::new()
        } else {
            let mut s = kept.join("\n");
            s.push('\n');
            s
        };
        let _ = fs::write(file, new_content);
    }

    let (_, new_total) = journal_disk_usage();
    println!(
        "Vacuumed by size: removed {} entries. Journal now uses {}.",
        total_removed,
        format_size(new_total)
    );
}

// ============================================================================
// Follow mode
// ============================================================================

fn cmd_follow(cfg: &Config) {
    // Print existing entries first (last N if -n specified, else last 10).
    let entries = read_all_entries();
    let num = cfg.num_entries.unwrap_or(10);
    let filtered = apply_filters(&entries, cfg);

    let display_entries = if filtered.len() > num {
        &filtered[filtered.len() - num..]
    } else {
        &filtered
    };

    for entry in display_entries {
        render_entry(entry, cfg);
    }

    // Track file sizes for change detection.
    let mut file_sizes: BTreeMap<PathBuf, u64> = BTreeMap::new();
    for file in &discover_journal_files() {
        let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        file_sizes.insert(file.clone(), size);
    }

    // Poll for new content.
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        let current_files = discover_journal_files();
        for file in &current_files {
            let current_size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
            let prev_size = file_sizes.get(file).copied().unwrap_or(0);

            if current_size > prev_size {
                if let Ok(content) = fs::read_to_string(file) {
                    let new_data = if (prev_size as usize) < content.len() {
                        &content[prev_size as usize..]
                    } else {
                        ""
                    };
                    for line in new_data.lines() {
                        if let Some(entry) = JournalEntry::from_json_line(line) {
                            // Apply filters except reverse and num_entries.
                            let passes = entry_passes_filters(&entry, cfg);
                            if passes {
                                render_entry(&entry, cfg);
                            }
                        }
                    }
                }
            } else if current_size < prev_size {
                // File was truncated or rotated.
            }
            file_sizes.insert(file.clone(), current_size);
        }
    }
}

/// Check if a single entry passes the configured filters (for follow mode).
fn entry_passes_filters(entry: &JournalEntry, cfg: &Config) -> bool {
    if let Some(ref unit) = cfg.unit_filter {
        let u_lower = unit.to_ascii_lowercase();
        if !entry.unit.to_ascii_lowercase().contains(&u_lower) {
            return false;
        }
    }
    if let Some(max_prio) = cfg.priority_filter
        && (entry.priority as u8) > (max_prio as u8) {
            return false;
        }
    if let Some(since) = cfg.since
        && entry.timestamp < since {
            return false;
        }
    if let Some(until) = cfg.until
        && entry.timestamp > until {
            return false;
        }
    if cfg.dmesg {
        let u = entry.unit.to_ascii_lowercase();
        if u != "kernel" && u != "kern" && u != "dmesg" {
            return false;
        }
    }
    if let Some(ref pattern) = cfg.grep_pattern
        && !pattern_matches(&entry.message, pattern) {
            return false;
        }
    true
}

// ============================================================================
// Help
// ============================================================================

fn print_usage() {
    println!("Slate OS Journal Log Viewer v0.1.0");
    println!();
    println!("Query and display messages from the journal.");
    println!();
    println!("USAGE:");
    println!("  journalctl [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("  -u, --unit <UNIT>           Show entries from this unit/service");
    println!("  -p, --priority <LEVEL>      Show entries at this priority or higher");
    println!("      --since <DATETIME>      Show entries since datetime");
    println!("      --until <DATETIME>      Show entries until datetime");
    println!("  -f, --follow                Follow/tail mode");
    println!("  -r, --reverse               Show newest entries first");
    println!("  -o, --output <FORMAT>       Output format:");
    println!("                                short, short-precise, json,");
    println!("                                json-pretty, cat, verbose");
    println!("  -b, --boot [ID]             Show entries from boot (latest if no ID)");
    println!("  -k, --dmesg                 Show kernel messages only");
    println!("  -n, --lines <N>             Show last N entries");
    println!("      --grep <PATTERN>        Filter messages by substring");
    println!("      --no-color              Disable colored output");
    println!("      --no-pager              Do not pipe through pager");
    println!();
    println!("INFORMATIONAL:");
    println!("      --list-fields           List all known field names");
    println!("      --disk-usage            Show disk usage of journal files");
    println!();
    println!("MAINTENANCE:");
    println!("      --vacuum-time <DUR>     Remove entries older than duration");
    println!("                                (e.g. 2d, 1w, 3h, 1M, 1y)");
    println!("      --vacuum-size <SIZE>    Shrink journal to at most size");
    println!("                                (e.g. 100M, 1G, 500K)");
    println!();
    println!("DATETIME FORMATS:");
    println!("  YYYY-MM-DD HH:MM:SS        Absolute datetime");
    println!("  YYYY-MM-DD                  Date only (midnight)");
    println!("  today / yesterday / now     Relative names");
    println!("  -Nd / -Nh / -Nm / -Ns      Relative offset");
    println!();
    println!("PRIORITY LEVELS (highest to lowest):");
    println!("  emerg(0) alert(1) crit(2) err(3) warning(4) notice(5) info(6) debug(7)");
    println!();
    println!("EXAMPLES:");
    println!("  journalctl -u net.dhcp              Show DHCP service logs");
    println!("  journalctl -p err                   Show errors and above");
    println!("  journalctl --since yesterday        Show last 24h");
    println!("  journalctl -k -b                    Kernel messages, current boot");
    println!("  journalctl -f -u kernel             Follow kernel messages");
    println!("  journalctl -o json-pretty -n 5      Last 5 entries as JSON");
    println!("  journalctl --vacuum-time 2w         Remove entries older than 2 weeks");
}

// ============================================================================
// Application logic
// ============================================================================

fn run(args: &[String]) -> i32 {
    let cfg = match parse_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("journalctl: {e}");
            eprintln!("Try 'journalctl --help' for usage.");
            return 1;
        }
    };

    if cfg.show_help {
        print_usage();
        return 0;
    }

    // Dispatch action commands.
    if cfg.list_fields {
        cmd_list_fields();
        return 0;
    }
    if cfg.disk_usage {
        cmd_disk_usage();
        return 0;
    }
    if let Some(secs) = cfg.vacuum_time {
        cmd_vacuum_time(secs);
        return 0;
    }
    if let Some(bytes) = cfg.vacuum_size {
        cmd_vacuum_size(bytes);
        return 0;
    }

    // Follow mode is special (never returns).
    if cfg.follow {
        cmd_follow(&cfg);
        return 0; // unreachable, but satisfies the type
    }

    // Normal display mode.
    let entries = read_all_entries();
    if entries.is_empty() {
        eprintln!("No journal entries found.");
        eprintln!("(Looked in {} and fallback paths)", JOURNAL_DIR);
        return 1;
    }

    let filtered = apply_filters(&entries, &cfg);
    if filtered.is_empty() {
        eprintln!("No entries match the specified filters.");
        return 0;
    }

    for entry in &filtered {
        render_entry(entry, &cfg);
    }

    0
}

// ============================================================================
// Entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = std::env::args().collect();
    run(&args)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Helper: create a JournalEntry for testing ---

    fn make_entry(
        ts: u64,
        level: &str,
        unit: &str,
        msg: &str,
        boot_id: &str,
        pid: u64,
    ) -> JournalEntry {
        let priority = Priority::from_name(level).unwrap_or(Priority::Info);
        let mut fields = BTreeMap::new();
        fields.insert("ts".to_string(), ts.to_string());
        fields.insert("level".to_string(), level.to_string());
        fields.insert("service".to_string(), unit.to_string());
        fields.insert("msg".to_string(), msg.to_string());
        if !boot_id.is_empty() {
            fields.insert("boot_id".to_string(), boot_id.to_string());
        }
        if pid != 0 {
            fields.insert("pid".to_string(), pid.to_string());
        }
        JournalEntry {
            timestamp: ts,
            timestamp_usec: 0,
            priority,
            unit: unit.to_string(),
            message: msg.to_string(),
            boot_id: boot_id.to_string(),
            pid,
            fields,
        }
    }

    fn sample_entries() -> Vec<JournalEntry> {
        vec![
            make_entry(1000, "emerg", "kernel", "panic: out of memory", "boot1", 0),
            make_entry(1001, "alert", "kernel", "watchdog timeout", "boot1", 0),
            make_entry(1002, "crit", "fs.ext4", "journal checksum error", "boot1", 100),
            make_entry(1003, "err", "net.dhcp", "lease expired", "boot1", 200),
            make_entry(1004, "warning", "sshd", "auth failure from 10.0.0.1", "boot1", 300),
            make_entry(1005, "notice", "init", "service started: crond", "boot1", 1),
            make_entry(1006, "info", "net.dhcp", "lease renewed for 10.0.2.15", "boot1", 200),
            make_entry(1007, "debug", "scheduler", "rebalance cpus", "boot1", 0),
            make_entry(2000, "info", "kernel", "boot complete", "boot2", 0),
            make_entry(2001, "err", "net.tcp", "connection reset", "boot2", 500),
        ]
    }

    fn make_json_line(
        ts: u64,
        level: &str,
        service: &str,
        msg: &str,
        boot_id: &str,
        pid: u64,
    ) -> String {
        let mut parts = vec![
            format!("\"ts\":{ts}"),
            format!("\"level\":\"{level}\""),
            format!("\"service\":\"{service}\""),
            format!("\"msg\":\"{msg}\""),
        ];
        if !boot_id.is_empty() {
            parts.push(format!("\"boot_id\":\"{boot_id}\""));
        }
        if pid != 0 {
            parts.push(format!("\"pid\":{pid}"));
        }
        format!("{{{}}}", parts.join(","))
    }

    // =========================================================================
    // Priority tests
    // =========================================================================

    #[test]
    fn test_priority_from_name_numeric() {
        assert_eq!(Priority::from_name("0"), Some(Priority::Emergency));
        assert_eq!(Priority::from_name("3"), Some(Priority::Error));
        assert_eq!(Priority::from_name("7"), Some(Priority::Debug));
    }

    #[test]
    fn test_priority_from_name_strings() {
        assert_eq!(Priority::from_name("emerg"), Some(Priority::Emergency));
        assert_eq!(Priority::from_name("ALERT"), Some(Priority::Alert));
        assert_eq!(Priority::from_name("crit"), Some(Priority::Critical));
        assert_eq!(Priority::from_name("err"), Some(Priority::Error));
        assert_eq!(Priority::from_name("warning"), Some(Priority::Warning));
        assert_eq!(Priority::from_name("warn"), Some(Priority::Warning));
        assert_eq!(Priority::from_name("notice"), Some(Priority::Notice));
        assert_eq!(Priority::from_name("info"), Some(Priority::Info));
        assert_eq!(Priority::from_name("debug"), Some(Priority::Debug));
    }

    #[test]
    fn test_priority_from_name_unknown() {
        assert_eq!(Priority::from_name("bogus"), None);
        assert_eq!(Priority::from_name(""), None);
        assert_eq!(Priority::from_name("99"), None);
    }

    #[test]
    fn test_priority_from_u8() {
        assert_eq!(Priority::from_u8(0), Priority::Emergency);
        assert_eq!(Priority::from_u8(7), Priority::Debug);
        assert_eq!(Priority::from_u8(255), Priority::Debug);
    }

    #[test]
    fn test_priority_name_roundtrip() {
        for val in 0..=7u8 {
            let p = Priority::from_u8(val);
            let name = p.name();
            let p2 = Priority::from_name(name).unwrap();
            assert_eq!(p, p2);
        }
    }

    #[test]
    fn test_priority_label() {
        assert_eq!(Priority::Emergency.label(), "EMERGENCY");
        assert_eq!(Priority::Error.label(), "ERROR");
        assert_eq!(Priority::Debug.label(), "DEBUG");
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Emergency < Priority::Alert);
        assert!(Priority::Alert < Priority::Critical);
        assert!(Priority::Info < Priority::Debug);
    }

    #[test]
    fn test_priority_ansi_color_nonempty() {
        // Emergency through Warning should have non-empty ANSI codes.
        assert!(!Priority::Emergency.ansi_color().is_empty());
        assert!(!Priority::Alert.ansi_color().is_empty());
        assert!(!Priority::Warning.ansi_color().is_empty());
        assert!(!Priority::Debug.ansi_color().is_empty());
        // Info uses default (empty).
        assert!(Priority::Info.ansi_color().is_empty());
    }

    // =========================================================================
    // JSON parsing tests
    // =========================================================================

    #[test]
    fn test_parse_json_object_basic() {
        let json = r#"{"ts":1000,"level":"info","msg":"hello"}"#;
        let map = parse_json_object(json).unwrap();
        assert_eq!(map.get("ts").unwrap(), "1000");
        assert_eq!(map.get("level").unwrap(), "info");
        assert_eq!(map.get("msg").unwrap(), "hello");
    }

    #[test]
    fn test_parse_json_object_empty() {
        let map = parse_json_object("{}").unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_json_object_not_json() {
        assert!(parse_json_object("not json").is_none());
        assert!(parse_json_object("[1,2,3]").is_none());
        assert!(parse_json_object("").is_none());
    }

    #[test]
    fn test_parse_json_object_escaped_strings() {
        let json = r#"{"msg":"line1\nline2","path":"c:\\temp"}"#;
        let map = parse_json_object(json).unwrap();
        assert_eq!(map.get("msg").unwrap(), "line1\nline2");
        assert_eq!(map.get("path").unwrap(), "c:\\temp");
    }

    #[test]
    fn test_parse_json_string_with_unicode() {
        let json = r#"{"msg":"hello \u0041 world"}"#;
        let map = parse_json_object(json).unwrap();
        assert_eq!(map.get("msg").unwrap(), "hello A world");
    }

    #[test]
    fn test_json_escape_basic() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
    }

    #[test]
    fn test_json_escape_control_chars() {
        assert_eq!(json_escape("\x01"), "\\u0001");
        assert_eq!(json_escape("\x1f"), "\\u001f");
    }

    // =========================================================================
    // JournalEntry parsing tests
    // =========================================================================

    #[test]
    fn test_entry_from_json_line_basic() {
        let line = make_json_line(1716000000, "info", "net.dhcp", "lease renewed", "abc", 42);
        let entry = JournalEntry::from_json_line(&line).unwrap();
        assert_eq!(entry.timestamp, 1716000000);
        assert_eq!(entry.priority, Priority::Info);
        assert_eq!(entry.unit, "net.dhcp");
        assert_eq!(entry.message, "lease renewed");
        assert_eq!(entry.boot_id, "abc");
        assert_eq!(entry.pid, 42);
    }

    #[test]
    fn test_entry_from_json_line_no_boot() {
        let line = r#"{"ts":100,"level":"err","service":"fs","msg":"io error"}"#;
        let entry = JournalEntry::from_json_line(line).unwrap();
        assert_eq!(entry.timestamp, 100);
        assert_eq!(entry.priority, Priority::Error);
        assert_eq!(entry.boot_id, "");
        assert_eq!(entry.pid, 0);
    }

    #[test]
    fn test_entry_from_json_line_invalid() {
        assert!(JournalEntry::from_json_line("not json").is_none());
        assert!(JournalEntry::from_json_line("").is_none());
        assert!(JournalEntry::from_json_line("[]").is_none());
    }

    #[test]
    fn test_entry_from_json_line_extra_fields() {
        let line = r#"{"ts":100,"level":"info","service":"x","msg":"m","custom_key":"custom_val"}"#;
        let entry = JournalEntry::from_json_line(line).unwrap();
        assert_eq!(entry.fields.get("custom_key").unwrap(), "custom_val");
    }

    #[test]
    fn test_entry_from_json_line_systemd_compat() {
        // Simulates systemd journal JSON export fields.
        let line = r#"{"__REALTIME_TIMESTAMP":"1716000000000000","PRIORITY":"3","_SYSTEMD_UNIT":"sshd","MESSAGE":"auth failed","_BOOT_ID":"xyz","_PID":"123"}"#;
        let entry = JournalEntry::from_json_line(line).unwrap();
        assert_eq!(entry.timestamp, 1716000000);
        assert_eq!(entry.priority, Priority::Error);
        assert_eq!(entry.unit, "sshd");
        assert_eq!(entry.message, "auth failed");
        assert_eq!(entry.boot_id, "xyz");
        assert_eq!(entry.pid, 123);
    }

    #[test]
    fn test_entry_to_json_roundtrip() {
        let original_line =
            make_json_line(5000, "warning", "net.tcp", "retransmit", "boot99", 777);
        let entry = JournalEntry::from_json_line(&original_line).unwrap();
        let serialized = entry.to_json();
        let reparsed = JournalEntry::from_json_line(&serialized).unwrap();
        assert_eq!(reparsed.timestamp, 5000);
        assert_eq!(reparsed.priority, Priority::Warning);
        assert_eq!(reparsed.unit, "net.tcp");
        assert_eq!(reparsed.message, "retransmit");
        assert_eq!(reparsed.boot_id, "boot99");
        assert_eq!(reparsed.pid, 777);
    }

    #[test]
    fn test_entry_to_json_pretty_contains_fields() {
        let entry = make_entry(1000, "err", "fs", "disk full", "b1", 42);
        let pretty = entry.to_json_pretty();
        assert!(pretty.contains("\"ts\": 1000"));
        assert!(pretty.contains("\"level\": \"err\""));
        assert!(pretty.contains("\"msg\": \"disk full\""));
        assert!(pretty.contains("\"pid\": 42"));
    }

    // =========================================================================
    // Timestamp formatting tests
    // =========================================================================

    #[test]
    fn test_format_timestamp_epoch() {
        assert_eq!(format_timestamp(0), "0000-00-00 00:00:00");
    }

    #[test]
    fn test_format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let ts = 1704067200;
        let formatted = format_timestamp(ts);
        assert_eq!(formatted, "2024-01-01 00:00:00");
    }

    #[test]
    fn test_format_timestamp_with_time() {
        // 1970-01-01 01:00:00 UTC = 3600
        let formatted = format_timestamp(3600);
        assert_eq!(formatted, "1970-01-01 01:00:00");
    }

    #[test]
    fn test_format_timestamp_precise() {
        let s = format_timestamp_precise(3600, 123456);
        assert_eq!(s, "1970-01-01 01:00:00.123456");
    }

    // =========================================================================
    // Datetime parsing tests
    // =========================================================================

    #[test]
    fn test_parse_datetime_absolute() {
        let ts = parse_datetime("2024-01-01 00:00:00").unwrap();
        assert_eq!(ts, 1704067200);
    }

    #[test]
    fn test_parse_datetime_date_only() {
        let ts = parse_datetime("2024-01-01").unwrap();
        assert_eq!(ts, 1704067200);
    }

    #[test]
    fn test_parse_datetime_now() {
        let ts = parse_datetime("now").unwrap();
        // Should be within a few seconds of actual now.
        let actual = now_secs();
        assert!(ts <= actual + 2);
        assert!(ts >= actual.saturating_sub(2));
    }

    #[test]
    fn test_parse_datetime_today() {
        let ts = parse_datetime("today").unwrap();
        let now = now_secs();
        // Today should be midnight of current day.
        let midnight = now - (now % 86400);
        assert_eq!(ts, midnight);
    }

    #[test]
    fn test_parse_datetime_yesterday() {
        let ts = parse_datetime("yesterday").unwrap();
        let now = now_secs();
        let yesterday_midnight = now - (now % 86400) - 86400;
        assert_eq!(ts, yesterday_midnight);
    }

    #[test]
    fn test_parse_datetime_relative_days() {
        let ts = parse_datetime("-3d").unwrap();
        let expected = now_secs() - 3 * 86400;
        assert!((ts as i64 - expected as i64).unsigned_abs() <= 2);
    }

    #[test]
    fn test_parse_datetime_relative_hours() {
        let ts = parse_datetime("-2h").unwrap();
        let expected = now_secs() - 2 * 3600;
        assert!((ts as i64 - expected as i64).unsigned_abs() <= 2);
    }

    #[test]
    fn test_parse_datetime_relative_minutes() {
        let ts = parse_datetime("-30m").unwrap();
        let expected = now_secs() - 30 * 60;
        assert!((ts as i64 - expected as i64).unsigned_abs() <= 2);
    }

    #[test]
    fn test_parse_datetime_invalid() {
        assert!(parse_datetime("not-a-date").is_none());
        assert!(parse_datetime("2024-13-01").is_none());
        assert!(parse_datetime("").is_none());
    }

    // =========================================================================
    // Duration/size parsing tests
    // =========================================================================

    #[test]
    fn test_parse_duration_secs_units() {
        assert_eq!(parse_duration_secs("10s"), Some(10));
        assert_eq!(parse_duration_secs("5m"), Some(300));
        assert_eq!(parse_duration_secs("2h"), Some(7200));
        assert_eq!(parse_duration_secs("3d"), Some(259200));
        assert_eq!(parse_duration_secs("1w"), Some(604800));
        assert_eq!(parse_duration_secs("1M"), Some(2592000));
        assert_eq!(parse_duration_secs("1y"), Some(31536000));
    }

    #[test]
    fn test_parse_duration_secs_plain_number() {
        assert_eq!(parse_duration_secs("3600"), Some(3600));
    }

    #[test]
    fn test_parse_duration_secs_invalid() {
        assert_eq!(parse_duration_secs(""), None);
    }

    #[test]
    fn test_parse_size_bytes_units() {
        assert_eq!(parse_size_bytes("100B"), Some(100));
        assert_eq!(parse_size_bytes("10K"), Some(10240));
        assert_eq!(parse_size_bytes("5M"), Some(5242880));
        assert_eq!(parse_size_bytes("1G"), Some(1073741824));
    }

    #[test]
    fn test_parse_size_bytes_plain_number() {
        assert_eq!(parse_size_bytes("4096"), Some(4096));
    }

    #[test]
    fn test_parse_size_bytes_invalid() {
        assert_eq!(parse_size_bytes(""), None);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(100), "100 B");
        assert_eq!(format_size(2048), "2.0 KiB");
        assert_eq!(format_size(1048576), "1.0 MiB");
        assert_eq!(format_size(1073741824), "1.00 GiB");
    }

    // =========================================================================
    // Filtering tests
    // =========================================================================

    #[test]
    fn test_filter_by_unit() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.unit_filter = Some("net.dhcp".to_string());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| e.unit.contains("net.dhcp")));
    }

    #[test]
    fn test_filter_by_unit_case_insensitive() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.unit_filter = Some("NET.DHCP".to_string());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_priority() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.priority_filter = Some(Priority::Error);
        let filtered = apply_filters(&entries, &cfg);
        // Should include emerg, alert, crit, err (0-3).
        assert!(filtered.iter().all(|e| (e.priority as u8) <= 3));
        assert_eq!(filtered.len(), 5); // emerg, alert, crit, err (boot1), err (boot2)
    }

    #[test]
    fn test_filter_by_since() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.since = Some(2000);
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| e.timestamp >= 2000));
    }

    #[test]
    fn test_filter_by_until() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.until = Some(1003);
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 4);
        assert!(filtered.iter().all(|e| e.timestamp <= 1003));
    }

    #[test]
    fn test_filter_by_since_and_until() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.since = Some(1003);
        cfg.until = Some(1006);
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 4);
    }

    #[test]
    fn test_filter_by_boot() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.boot_filter = Some("boot2".to_string());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| e.boot_id == "boot2"));
    }

    #[test]
    fn test_filter_by_boot_latest() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.boot_filter = Some(String::new()); // empty = latest
        let filtered = apply_filters(&entries, &cfg);
        // Latest boot is "boot2".
        assert!(filtered.iter().all(|e| e.boot_id == "boot2"));
    }

    #[test]
    fn test_filter_dmesg() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.dmesg = true;
        let filtered = apply_filters(&entries, &cfg);
        assert!(filtered.iter().all(|e| {
            let u = e.unit.to_ascii_lowercase();
            u == "kernel" || u == "kern" || u == "dmesg"
        }));
    }

    #[test]
    fn test_filter_grep() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.grep_pattern = Some("lease".to_string());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_grep_case_insensitive() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.grep_pattern = Some("LEASE".to_string());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_num_entries() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.num_entries = Some(3);
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 3);
        // Should be the last 3 entries.
        assert_eq!(filtered[0].timestamp, 1007);
        assert_eq!(filtered[1].timestamp, 2000);
        assert_eq!(filtered[2].timestamp, 2001);
    }

    #[test]
    fn test_filter_reverse() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.reverse = true;
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), entries.len());
        // First entry should now be the one with highest timestamp.
        assert_eq!(filtered[0].timestamp, 2001);
        assert_eq!(filtered[filtered.len() - 1].timestamp, 1000);
    }

    #[test]
    fn test_filter_reverse_with_num() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.reverse = true;
        cfg.num_entries = Some(2);
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
        // Reversed, then take first 2 = two most recent.
        assert_eq!(filtered[0].timestamp, 2001);
        assert_eq!(filtered[1].timestamp, 2000);
    }

    #[test]
    fn test_filter_combined() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.priority_filter = Some(Priority::Error);
        cfg.boot_filter = Some("boot1".to_string());
        let filtered = apply_filters(&entries, &cfg);
        // boot1 entries with priority <= Error: emerg, alert, crit, err.
        assert_eq!(filtered.len(), 4);
    }

    #[test]
    fn test_filter_no_matches() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.unit_filter = Some("nonexistent.service".to_string());
        let filtered = apply_filters(&entries, &cfg);
        assert!(filtered.is_empty());
    }

    // =========================================================================
    // Pattern matching tests
    // =========================================================================

    #[test]
    fn test_pattern_matches_basic() {
        assert!(pattern_matches("hello world", "hello"));
        assert!(pattern_matches("hello world", "world"));
        assert!(!pattern_matches("hello world", "xyz"));
    }

    #[test]
    fn test_pattern_matches_case_insensitive() {
        assert!(pattern_matches("Hello World", "hello"));
        assert!(pattern_matches("hello", "HELLO"));
    }

    // =========================================================================
    // Output format tests
    // =========================================================================

    #[test]
    fn test_output_format_from_name() {
        assert_eq!(OutputFormat::from_name("short"), Some(OutputFormat::Short));
        assert_eq!(
            OutputFormat::from_name("short-precise"),
            Some(OutputFormat::ShortPrecise)
        );
        assert_eq!(OutputFormat::from_name("json"), Some(OutputFormat::Json));
        assert_eq!(
            OutputFormat::from_name("json-pretty"),
            Some(OutputFormat::JsonPretty)
        );
        assert_eq!(OutputFormat::from_name("cat"), Some(OutputFormat::Cat));
        assert_eq!(
            OutputFormat::from_name("verbose"),
            Some(OutputFormat::Verbose)
        );
        assert_eq!(OutputFormat::from_name("unknown"), None);
    }

    #[test]
    fn test_output_format_case_insensitive() {
        assert_eq!(OutputFormat::from_name("JSON"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::from_name("Cat"), Some(OutputFormat::Cat));
    }

    // =========================================================================
    // Argument parsing tests
    // =========================================================================

    #[test]
    fn test_parse_args_empty() {
        let args = vec!["journalctl".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.unit_filter.is_none());
        assert!(cfg.priority_filter.is_none());
        assert!(!cfg.follow);
        assert!(!cfg.reverse);
        assert_eq!(cfg.output_format, OutputFormat::Short);
    }

    #[test]
    fn test_parse_args_unit() {
        let args = vec![
            "journalctl".to_string(),
            "-u".to_string(),
            "sshd".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.unit_filter.as_deref(), Some("sshd"));
    }

    #[test]
    fn test_parse_args_priority() {
        let args = vec![
            "journalctl".to_string(),
            "-p".to_string(),
            "err".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.priority_filter, Some(Priority::Error));
    }

    #[test]
    fn test_parse_args_follow() {
        let args = vec!["journalctl".to_string(), "-f".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.follow);
    }

    #[test]
    fn test_parse_args_reverse() {
        let args = vec!["journalctl".to_string(), "-r".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.reverse);
    }

    #[test]
    fn test_parse_args_output_format() {
        let args = vec![
            "journalctl".to_string(),
            "-o".to_string(),
            "json-pretty".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.output_format, OutputFormat::JsonPretty);
        assert!(!cfg.color); // JSON disables color.
    }

    #[test]
    fn test_parse_args_num_entries() {
        let args = vec![
            "journalctl".to_string(),
            "-n".to_string(),
            "25".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.num_entries, Some(25));
    }

    #[test]
    fn test_parse_args_grep() {
        let args = vec![
            "journalctl".to_string(),
            "--grep".to_string(),
            "error".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.grep_pattern.as_deref(), Some("error"));
    }

    #[test]
    fn test_parse_args_dmesg() {
        let args = vec!["journalctl".to_string(), "-k".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.dmesg);

        let args2 = vec!["journalctl".to_string(), "--dmesg".to_string()];
        let cfg2 = parse_args(&args2).unwrap();
        assert!(cfg2.dmesg);
    }

    #[test]
    fn test_parse_args_boot_with_id() {
        let args = vec![
            "journalctl".to_string(),
            "-b".to_string(),
            "boot42".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.boot_filter, Some("boot42".to_string()));
    }

    #[test]
    fn test_parse_args_boot_no_id() {
        let args = vec!["journalctl".to_string(), "-b".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.boot_filter, Some(String::new()));
    }

    #[test]
    fn test_parse_args_help() {
        let args = vec!["journalctl".to_string(), "--help".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_help);
    }

    #[test]
    fn test_parse_args_list_fields() {
        let args = vec!["journalctl".to_string(), "--list-fields".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.list_fields);
    }

    #[test]
    fn test_parse_args_disk_usage() {
        let args = vec!["journalctl".to_string(), "--disk-usage".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.disk_usage);
    }

    #[test]
    fn test_parse_args_vacuum_time() {
        let args = vec![
            "journalctl".to_string(),
            "--vacuum-time".to_string(),
            "7d".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.vacuum_time, Some(604800));
    }

    #[test]
    fn test_parse_args_vacuum_size() {
        let args = vec![
            "journalctl".to_string(),
            "--vacuum-size".to_string(),
            "100M".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.vacuum_size, Some(104857600));
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let args = vec!["journalctl".to_string(), "--bogus".to_string()];
        let result = parse_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_args_missing_value() {
        let args = vec!["journalctl".to_string(), "-u".to_string()];
        assert!(parse_args(&args).is_err());

        let args2 = vec!["journalctl".to_string(), "-p".to_string()];
        assert!(parse_args(&args2).is_err());

        let args3 = vec!["journalctl".to_string(), "-n".to_string()];
        assert!(parse_args(&args3).is_err());
    }

    #[test]
    fn test_parse_args_invalid_priority() {
        let args = vec![
            "journalctl".to_string(),
            "-p".to_string(),
            "bogus".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_invalid_format() {
        let args = vec![
            "journalctl".to_string(),
            "-o".to_string(),
            "xml".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }

    // =========================================================================
    // Entry passes filters tests (follow mode helper)
    // =========================================================================

    #[test]
    fn test_entry_passes_filters_all_pass() {
        let entry = make_entry(1000, "info", "net.dhcp", "lease renewed", "b1", 100);
        let cfg = Config::new();
        assert!(entry_passes_filters(&entry, &cfg));
    }

    #[test]
    fn test_entry_passes_filters_unit_fail() {
        let entry = make_entry(1000, "info", "net.dhcp", "lease renewed", "b1", 100);
        let mut cfg = Config::new();
        cfg.unit_filter = Some("sshd".to_string());
        assert!(!entry_passes_filters(&entry, &cfg));
    }

    #[test]
    fn test_entry_passes_filters_priority_fail() {
        let entry = make_entry(1000, "debug", "net", "trace data", "b1", 0);
        let mut cfg = Config::new();
        cfg.priority_filter = Some(Priority::Warning);
        assert!(!entry_passes_filters(&entry, &cfg));
    }

    #[test]
    fn test_entry_passes_filters_since_fail() {
        let entry = make_entry(500, "info", "net", "old msg", "b1", 0);
        let mut cfg = Config::new();
        cfg.since = Some(1000);
        assert!(!entry_passes_filters(&entry, &cfg));
    }

    #[test]
    fn test_entry_passes_filters_dmesg_pass() {
        let entry = make_entry(1000, "info", "kernel", "boot ok", "b1", 0);
        let mut cfg = Config::new();
        cfg.dmesg = true;
        assert!(entry_passes_filters(&entry, &cfg));
    }

    #[test]
    fn test_entry_passes_filters_dmesg_fail() {
        let entry = make_entry(1000, "info", "net.dhcp", "lease", "b1", 0);
        let mut cfg = Config::new();
        cfg.dmesg = true;
        assert!(!entry_passes_filters(&entry, &cfg));
    }

    #[test]
    fn test_entry_passes_filters_grep_fail() {
        let entry = make_entry(1000, "info", "net", "hello world", "b1", 0);
        let mut cfg = Config::new();
        cfg.grep_pattern = Some("foobar".to_string());
        assert!(!entry_passes_filters(&entry, &cfg));
    }

    // =========================================================================
    // find_latest_boot_id tests
    // =========================================================================

    #[test]
    fn test_find_latest_boot_id() {
        let entries = sample_entries();
        let latest = find_latest_boot_id(&entries);
        assert_eq!(latest, Some("boot2".to_string()));
    }

    #[test]
    fn test_find_latest_boot_id_empty() {
        let entries: Vec<JournalEntry> = Vec::new();
        assert_eq!(find_latest_boot_id(&entries), None);
    }

    #[test]
    fn test_find_latest_boot_id_no_boot() {
        let entries = vec![make_entry(1000, "info", "x", "msg", "", 0)];
        assert_eq!(find_latest_boot_id(&entries), None);
    }

    // =========================================================================
    // Leap year / date helpers
    // =========================================================================

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }

    // =========================================================================
    // run() integration-level tests
    // =========================================================================

    #[test]
    fn test_run_help() {
        let args = vec!["journalctl".to_string(), "--help".to_string()];
        let code = run(&args);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_disk_usage() {
        // Won't find journal files in test env, but should not crash.
        let args = vec!["journalctl".to_string(), "--disk-usage".to_string()];
        let code = run(&args);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_unknown_option() {
        let args = vec!["journalctl".to_string(), "--bogus".to_string()];
        let code = run(&args);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_parse_args_no_color() {
        let args = vec!["journalctl".to_string(), "--no-color".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(!cfg.color);
    }

    #[test]
    fn test_parse_args_no_pager() {
        let args = vec!["journalctl".to_string(), "--no-pager".to_string()];
        let cfg = parse_args(&args).unwrap();
        // Should parse without error (accepted and ignored).
        assert!(!cfg.show_help);
    }

    #[test]
    fn test_parse_args_since_date() {
        let args = vec![
            "journalctl".to_string(),
            "--since".to_string(),
            "2024-01-01".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.since, Some(1704067200));
    }

    #[test]
    fn test_parse_args_combined_flags() {
        let args = vec![
            "journalctl".to_string(),
            "-u".to_string(),
            "sshd".to_string(),
            "-p".to_string(),
            "err".to_string(),
            "-n".to_string(),
            "50".to_string(),
            "-r".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.unit_filter.as_deref(), Some("sshd"));
        assert_eq!(cfg.priority_filter, Some(Priority::Error));
        assert_eq!(cfg.num_entries, Some(50));
        assert!(cfg.reverse);
    }
}
