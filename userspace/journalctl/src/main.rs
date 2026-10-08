//! Slate OS Journal Log Viewer (`journalctl`)
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

use journalrec::Value;
use quoting::{os_bytes, quotef_os};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const JOURNAL_DIR: &str = "/var/log/journal";
/// Fallback log paths when journal dir does not exist, and whether
/// `syslogd` rotates each: `syslog.jsonl` grows `syslog.jsonl.1`, `.2`, ...
/// as it passes 5 MiB, and those are the journal's older records.
const FALLBACK_PATHS: &[(&str, bool)] =
    &[("/var/log/syslog.jsonl", true), ("/var/log/syslog", false)];

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
            Self::Alert => "\x1b[1;31m",        // bold red
            Self::Critical => "\x1b[31m",       // red
            Self::Error => "\x1b[91m",          // bright red
            Self::Warning => "\x1b[33m",        // yellow
            Self::Notice => "\x1b[1;37m",       // bold white
            Self::Info => "",                   // default
            Self::Debug => "\x1b[90m",          // dim gray
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
    /// Service or unit name (e.g. "net.dhcp", "kernel"). Bytes, as the
    /// record gave them: a sender's name need not be text.
    unit: Vec<u8>,
    /// Log message, as the record gave it -- any bytes (design-decisions
    /// §1063), shown as they are.
    message: Vec<u8>,
    /// Boot identifier string.
    boot_id: String,
    /// Process ID (0 if unknown).
    pid: u64,
    /// All key-value fields from the original JSON (preserves extras).
    fields: BTreeMap<String, Value>,
}

impl JournalEntry {
    /// Parse from a single JSON-lines record, through `journalrec`'s reader
    /// -- the one `syslogd`'s commands read with too.
    fn from_json_line(line: &str) -> Option<Self> {
        let fields = journalrec::parse_object(line)?;
        // A field read as text: a string or a scalar, not bytes.
        let text = |key: &str| fields.get(key).and_then(Value::text);

        let timestamp = text("ts")
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| {
                text("__REALTIME_TIMESTAMP")
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|us| us / 1_000_000)
            })
            .unwrap_or(0);

        let timestamp_usec = text("ts_usec")
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| {
                text("__REALTIME_TIMESTAMP")
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|us| us % 1_000_000)
            })
            .unwrap_or(0);

        let priority_str = text("level").or_else(|| text("PRIORITY"));
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
            .map(|v| v.bytes().to_vec())
            .unwrap_or_default();

        let message = fields
            .get("msg")
            .or_else(|| fields.get("MESSAGE"))
            .map(|v| v.bytes().to_vec())
            .unwrap_or_default();

        let boot_id = text("boot_id")
            .or_else(|| text("_BOOT_ID"))
            .map(str::to_string)
            .unwrap_or_default();

        let pid = text("pid")
            .or_else(|| text("_PID"))
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
        parts.push(format!(
            "\"level\":\"{}\"",
            journalrec::escape(self.priority.name())
        ));
        if !self.unit.is_empty() {
            parts.push(format!(
                "\"service\":{}",
                journalrec::json_value(&self.unit)
            ));
        }
        parts.push(format!("\"msg\":{}", journalrec::json_value(&self.message)));
        if !self.boot_id.is_empty() {
            parts.push(format!(
                "\"boot_id\":\"{}\"",
                journalrec::escape(&self.boot_id)
            ));
        }
        if self.pid != 0 {
            parts.push(format!("\"pid\":{}", self.pid));
        }
        // Include extra fields not already serialized.
        let known_keys: &[&str] = &[
            "ts",
            "ts_usec",
            "level",
            "service",
            "msg",
            "boot_id",
            "pid",
            "time",
            "__REALTIME_TIMESTAMP",
            "PRIORITY",
            "_SYSTEMD_UNIT",
            "SYSLOG_IDENTIFIER",
            "unit",
            "MESSAGE",
            "_BOOT_ID",
            "_PID",
        ];
        for (k, v) in &self.fields {
            if !known_keys.contains(&k.as_str()) {
                parts.push(format!("\"{}\":{}", journalrec::escape(k), v.to_json()));
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
            journalrec::escape(self.priority.name())
        ));
        if !self.unit.is_empty() {
            lines.push(format!(
                "    \"service\": {},",
                journalrec::json_value(&self.unit)
            ));
        }
        lines.push(format!(
            "    \"msg\": {},",
            journalrec::json_value(&self.message)
        ));
        if !self.boot_id.is_empty() {
            lines.push(format!(
                "    \"boot_id\": \"{}\",",
                journalrec::escape(&self.boot_id)
            ));
        }
        if self.pid != 0 {
            lines.push(format!("    \"pid\": {},", self.pid));
        }
        let known_keys: &[&str] = &[
            "ts",
            "ts_usec",
            "level",
            "service",
            "msg",
            "boot_id",
            "pid",
            "time",
            "__REALTIME_TIMESTAMP",
            "PRIORITY",
            "_SYSTEMD_UNIT",
            "SYSLOG_IDENTIFIER",
            "unit",
            "MESSAGE",
            "_BOOT_ID",
            "_PID",
        ];
        let extras: Vec<_> = self
            .fields
            .iter()
            .filter(|(k, _)| !known_keys.contains(&k.as_str()))
            .collect();
        for (k, v) in &extras {
            lines.push(format!(
                "    \"{}\": {},",
                journalrec::escape(k),
                v.to_json()
            ));
        }
        // Remove trailing comma from last field line.
        if let Some(last) = lines.last_mut()
            && last.ends_with(',')
        {
            last.pop();
        }
        lines.push("}".to_string());
        lines.join("\n")
    }
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

/// Convert unix timestamp to "YYYY-MM-DD HH:MM:SS" string.
///
/// Through `journalrec::Utc`, in a fixed number of steps: this counted a
/// year at a time from 1970, so one record with a `ts` near `u64::MAX` --
/// which anything able to write a line into the log can put there -- kept
/// every listing busy for many minutes.
fn format_timestamp(unix_secs: u64) -> String {
    if unix_secs == 0 {
        return "0000-00-00 00:00:00".to_string();
    }
    let t = journalrec::Utc::from_unix(unix_secs);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// Format with microsecond precision: "YYYY-MM-DD HH:MM:SS.UUUUUU".
fn format_timestamp_precise(unix_secs: u64, usec: u64) -> String {
    let base = format_timestamp(unix_secs);
    format!("{base}.{usec:06}")
}

/// Parse a datetime string into a unix timestamp.
///
/// Supported formats:
/// - "YYYY-MM-DD HH:MM:SS", with "HH" or "HH:MM" also allowed
/// - "YYYY-MM-DD"
/// - "now", "today", "yesterday"
/// - "-Nd" / "-Nh" / "-Nm" / "-Ns" (relative: N days/hours/minutes/seconds ago)
///
/// A date that is not a real one -- February 30, an hour of 25 -- is
/// refused, and so is one before 1970, where no record can be. Until
/// 2026-10-08 such a date rolled over into the next month or hour, a date
/// before 1970 was taken as the same day of 1970, a time field that did not
/// parse was taken as 0, a year was counted to one step at a time (without
/// end, for a large enough year), a large relative count overflowed, and a
/// relative form ending in a multi-byte character panicked.
fn parse_datetime(s: &str) -> Option<u64> {
    let s = s.trim();

    if s.eq_ignore_ascii_case("now") {
        return Some(now_secs());
    }
    if s.eq_ignore_ascii_case("today") {
        let now = now_secs();
        // Round down to midnight.
        return Some(now.saturating_sub(now % 86400));
    }
    if s.eq_ignore_ascii_case("yesterday") {
        let now = now_secs();
        return Some(now.saturating_sub(now % 86400).saturating_sub(86400));
    }

    // Relative: -Nd, -Nh, -Nm, -Ns
    if let Some(rest) = s.strip_prefix('-') {
        let per: u64 = match rest.as_bytes().last() {
            Some(b'd') => 86400,
            Some(b'h') => 3600,
            Some(b'm') => 60,
            Some(b's') => 1,
            _ => return None,
        };
        // The unit is one ASCII byte, so this cut is between characters.
        let n: u64 = rest.get(..rest.len().saturating_sub(1))?.parse().ok()?;
        return Some(now_secs().saturating_sub(n.checked_mul(per)?));
    }

    // "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DD"
    let (date_part, time_part) = match s.split_once(' ') {
        Some((date, time)) => (date, Some(time)),
        None => (s, None),
    };
    let date_fields: Vec<&str> = date_part.split('-').collect();
    let [year, month, day] = date_fields.as_slice() else {
        return None;
    };
    let (hour, minute, second) = match time_part {
        None => (0, 0, 0),
        Some(time) => {
            let fields: Vec<&str> = time.split(':').collect();
            if fields.len() > 3 {
                return None;
            }
            // A field that is there must be a number; one left off is 0.
            let field = |i: usize| fields.get(i).map_or(Some(0), |f| f.parse().ok());
            (field(0)?, field(1)?, field(2)?)
        }
    };
    journalrec::Utc {
        year: year.parse().ok()?,
        month: month.parse().ok()?,
        day: day.parse().ok()?,
        hour,
        minute,
        second,
    }
    .to_unix()
}

// ============================================================================
// Duration / size parsing (for --vacuum-time, --vacuum-size)
// ============================================================================

/// Parse a duration string like "2d", "1w", "3h", "30m", "7200s", "1M", "1y"
/// into seconds; a bare number is seconds.
///
/// `None` for a duration too long to count in seconds. It used to be
/// multiplied out unchecked, wrapping round to a short one, so a vast enough
/// `--vacuum-time` removed nearly every record; and a unit that was a
/// multi-byte character was cut through, which panicked.
fn parse_duration_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    let per: u64 = match s.as_bytes().last()? {
        b's' => 1,
        b'm' => 60,
        b'h' => 3_600,
        b'd' => 86_400,
        b'w' => 604_800,
        b'M' => 2_592_000,
        b'y' => 31_536_000,
        // Maybe the whole string is just a number (seconds).
        _ => return s.parse().ok(),
    };
    // The unit is one ASCII byte, so this cut is between characters.
    let n: u64 = s.get(..s.len().saturating_sub(1))?.parse().ok()?;
    n.checked_mul(per)
}

/// Parse a size string like "100M", "1G", "500K" into bytes; a bare number
/// is bytes.
///
/// `None` for a size too large to count in bytes. It used to be multiplied
/// out unchecked: `--vacuum-size 17179869184G` is 2^64 bytes, which wrapped
/// round to 0 and so removed the whole journal.
fn parse_size_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    let per: u64 = match s.as_bytes().last()? {
        b'0'..=b'9' => return s.parse().ok(),
        b'B' | b'b' => 1,
        b'K' | b'k' => 1_024,
        b'M' => 1_048_576,
        b'G' | b'g' => 1_073_741_824,
        _ => return None,
    };
    // The unit is one ASCII byte, so this cut is between characters.
    let n: u64 = s.get(..s.len().saturating_sub(1))?.parse().ok()?;
    n.checked_mul(per)
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
    discover().0
}

/// The log files `journalctl` reads, and the directories under
/// `/var/log/journal/` it could not list.
///
/// The second half is what [`discover_journal_files`] throws away. A reader
/// that cannot say "I could not look there" reports an empty journal and a
/// journal it was not allowed to read in the same words.
fn discover() -> (Vec<PathBuf>, Vec<(PathBuf, io::Error)>) {
    let journal_path = Path::new(JOURNAL_DIR);
    let mut files = Vec::new();
    let mut unreadable = Vec::new();

    if journal_path.is_dir() {
        collect_jsonl_files(journal_path, &mut files, &mut unreadable);
    }
    files.sort();

    // If no journal files found, try fallback paths: each one's rotated
    // copies first, oldest first, then the file itself -- the order the
    // records were written in, which is the order `--vacuum-size` trims.
    if files.is_empty() {
        for &(path_str, rotated) in FALLBACK_PATHS {
            let p = Path::new(path_str);
            if rotated {
                files.extend(rotated_siblings(p));
            }
            if p.is_file() {
                files.push(p.to_path_buf());
            }
        }
    }

    (files, unreadable)
}

/// `syslogd`'s rotated copies of `live` -- `NAME.1`, `NAME.2`, ..., the
/// higher the older -- oldest first. Until 2026-09-26 nothing read these,
/// so every record older than the last rotation was invisible here.
fn rotated_siblings(live: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (live.parent(), live.file_name()) else {
        return Vec::new();
    };
    let mut prefix = name.as_encoded_bytes().to_vec();
    prefix.push(b'.');
    // A directory that cannot be listed has no rotated copies to offer; the
    // live file, if there, is still read -- and reported if unreadable.
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let file_name = e.file_name();
            let n = rotation_number(file_name.as_encoded_bytes(), &prefix)?;
            let path = e.path();
            path.is_file().then_some((n, path))
        })
        .collect();
    found.sort_by_key(|&(n, _)| std::cmp::Reverse(n));
    found.into_iter().map(|(_, p)| p).collect()
}

/// `N` of a name that is `prefix` then the digits of `N` (1 or more).
fn rotation_number(name: &[u8], prefix: &[u8]) -> Option<u64> {
    let digits = name.strip_prefix(prefix)?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits)
        .ok()?
        .parse()
        .ok()
        .filter(|&n| n > 0)
}

/// Whether `path` is one of `syslogd`'s rotated copies: an archive, which
/// a vacuum that empties it removes, where the live file is kept.
fn is_rotated(path: &Path) -> bool {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    FALLBACK_PATHS.iter().any(|&(live, rotated)| {
        let live = Path::new(live);
        rotated
            && live.parent() == Some(dir)
            && live.file_name().is_some_and(|l| {
                let mut prefix = l.as_encoded_bytes().to_vec();
                prefix.push(b'.');
                rotation_number(name.as_encoded_bytes(), &prefix).is_some()
            })
    })
}

/// Recursively collect .jsonl, .log and .journal files from a directory,
/// recording each directory that could not be listed.
///
/// The suffix is matched on the name's BYTES: a log file whose name is not
/// valid UTF-8 is still a log file, and `to_str().unwrap_or("")` -- what this
/// did before -- quietly left it out.
fn collect_jsonl_files(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    unreadable: &mut Vec<(PathBuf, io::Error)>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            unreadable.push((dir.to_path_buf(), e));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                unreadable.push((dir.to_path_buf(), e));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out, unreadable);
        } else if path.is_file() {
            let name = path
                .file_name()
                .map(OsStr::as_encoded_bytes)
                .unwrap_or_default();
            if name.ends_with(b".jsonl") || name.ends_with(b".log") || name.ends_with(b".journal") {
                out.push(path);
            }
        }
    }
}

/// A line of a log file as a journal record, if it is one.
///
/// A record is JSON, and JSON is UTF-8, so a line that is not UTF-8 is not a
/// record -- but it is only that LINE. Reading the file as one `String`, as
/// this program did, made one such byte cost every record in the file.
fn record_of(line: &[u8]) -> Option<JournalEntry> {
    std::str::from_utf8(line)
        .ok()
        .and_then(JournalEntry::from_json_line)
}

/// Whether a line is blank, which is not worth a warning.
fn is_blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}

/// Every log file's records, and an account of what could not be shown.
struct Journal {
    /// All records, oldest first.
    entries: Vec<JournalEntry>,
    /// What has been read of each file -- known by its identity, so that a
    /// rotation renaming the files does not lose the place -- and any
    /// unterminated last line held back (see [`read_journal`]): where `-f`
    /// goes on from.
    follow: journalio::Follow,
    /// Files with lines that are not records, and how many.
    not_records: Vec<(PathBuf, usize)>,
    /// Files and directories that could not be read, and why.
    unreadable: Vec<(PathBuf, io::Error)>,
}

/// Read every log file, as bytes, line by line.
///
/// `hold_back_unterminated` is for `-f`: a last line with no newline may be a
/// record still being written, so it is neither shown nor counted, but held
/// in [`Journal::follow`] for the follow loop to complete. A plain listing
/// shows it if it parses -- a writer that never ends its last record is still
/// a writer whose record should be seen.
fn read_journal(hold_back_unterminated: bool) -> Journal {
    let (files, unreadable) = discover();
    read_files(&files, unreadable, hold_back_unterminated)
}

/// [`read_journal`] over a given list of files -- everything but where the
/// journal lives, so a test can hand it its own.
fn read_files(
    files: &[PathBuf],
    mut unreadable: Vec<(PathBuf, io::Error)>,
    hold_back_unterminated: bool,
) -> Journal {
    let mut entries = Vec::new();
    let mut follow = journalio::Follow::new();
    let mut not_records = Vec::new();

    for file in files {
        let bytes = match follow.read(file, hold_back_unterminated) {
            Ok(Some(b)) => b,
            // Gone between discovery and reading -- rotated away. Not an
            // error: there is nothing left that could have been shown.
            Ok(None) => continue,
            Err(e) => {
                unreadable.push((file.clone(), e));
                continue;
            }
        };
        let mut bad = 0usize;
        for line in journalio::lines(&bytes) {
            match record_of(line) {
                Some(entry) => entries.push(entry),
                None if is_blank(line) => {}
                None => bad = bad.saturating_add(1),
            }
        }
        if bad > 0 {
            not_records.push((file.clone(), bad));
        }
    }

    // Sort by timestamp.
    entries.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then(a.timestamp_usec.cmp(&b.timestamp_usec))
    });

    Journal {
        entries,
        follow,
        not_records,
        unreadable,
    }
}

fn len_u64(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// Say on stderr what [`read_journal`] could not show. Returns whether
/// anything could not be READ, which is an error; lines that are not records
/// are reported but are not one -- `/var/log/syslog` holds text lines, and
/// saying so each time is the point, not a failure.
fn report_journal(journal: &Journal) -> bool {
    for (path, n) in &journal.not_records {
        let what = if *n == 1 { "line is" } else { "lines are" };
        say(&format!(
            "journalctl: {}: {n} {what} not a journal record and not shown",
            quotef_os(path)
        ));
    }
    for (path, e) in &journal.unreadable {
        cannot("read", path, e);
    }
    !journal.unreadable.is_empty()
}

/// Compute total disk usage of all journal files.
fn journal_disk_usage() -> (usize, u64) {
    let files = discover_journal_files();
    let mut total_bytes: u64 = 0;
    let mut count = 0usize;

    for file in &files {
        if let Ok(meta) = fs::metadata(file) {
            total_bytes = total_bytes.saturating_add(meta.len());
            count = count.saturating_add(1);
        }
    }

    (count, total_bytes)
}

// ============================================================================
// Substring matching (simple pattern -- not full regex, but handles basic cases)
// ============================================================================

/// Whether `pattern` occurs in `haystack`, ASCII letters matched in either
/// case. Both are bytes, so a message that is not text is searched as it is,
/// for a pattern that need not be text either.
fn pattern_matches(haystack: &[u8], pattern: impl AsRef<[u8]>) -> bool {
    let h = haystack.to_ascii_lowercase();
    let p = pattern.as_ref().to_ascii_lowercase();
    p.is_empty() || h.windows(p.len()).any(|w| w == p.as_slice())
}

/// Whether `unit` names a unit `wanted` picks out: a substring, ASCII
/// letters in either case.
fn unit_matches(unit: &[u8], wanted: impl AsRef<[u8]>) -> bool {
    pattern_matches(unit, wanted)
}

/// Whether `unit` is the kernel's, for `-k`.
fn is_kernel_unit(unit: &[u8]) -> bool {
    let u = unit.to_ascii_lowercase();
    u == b"kernel" || u == b"kern" || u == b"dmesg"
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
    /// Filter by unit/service name (substring match), as the bytes given.
    unit_filter: Option<Vec<u8>>,
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
    /// Filter by boot ID, as the bytes given; empty for the latest boot.
    boot_filter: Option<Vec<u8>>,
    /// Show only kernel messages.
    dmesg: bool,
    /// Number of entries to show (None = all).
    num_entries: Option<usize>,
    /// Grep / pattern filter on message content, as the bytes given.
    grep_pattern: Option<Vec<u8>>,
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
///
/// The words are taken as they are, not as text: a unit name, a boot ID and a
/// `--grep` pattern are matched against a record's bytes, so they may be any
/// bytes as well, and a value that has to be text -- a priority, a date, a
/// count -- and is not is refused like any other value that does not parse.
/// `std::env::args()`, which this was handed until 2026-10-07, panicked on
/// such a word instead.
fn parse_args<S: AsRef<OsStr>>(args: &[S]) -> Result<Config, String> {
    let mut cfg = Config::new();
    let word = |i: usize| args.get(i).map(AsRef::as_ref);
    let mut i = 1; // skip argv[0]

    while let Some(arg) = word(i) {
        match arg.as_encoded_bytes() {
            b"-u" | b"--unit" => {
                let unit = word(i.saturating_add(1)).ok_or("-u requires a unit name")?;
                cfg.unit_filter = Some(os_bytes(unit).into_owned());
                i = i.saturating_add(2);
            }
            b"-p" | b"--priority" => {
                let level = word(i.saturating_add(1)).ok_or("-p requires a priority level")?;
                let prio = level
                    .to_str()
                    .and_then(Priority::from_name)
                    .ok_or_else(|| format!("unknown priority: {}", quotef_os(level)))?;
                cfg.priority_filter = Some(prio);
                i = i.saturating_add(2);
            }
            b"--since" => {
                let (ts, taken) = datetime_at(args, i.saturating_add(1), "--since")?;
                cfg.since = Some(ts);
                i = i.saturating_add(1).saturating_add(taken);
            }
            b"--until" => {
                let (ts, taken) = datetime_at(args, i.saturating_add(1), "--until")?;
                cfg.until = Some(ts);
                i = i.saturating_add(1).saturating_add(taken);
            }
            b"-f" | b"--follow" => {
                cfg.follow = true;
                i = i.saturating_add(1);
            }
            b"-r" | b"--reverse" => {
                cfg.reverse = true;
                i = i.saturating_add(1);
            }
            b"-o" | b"--output" => {
                let name = word(i.saturating_add(1)).ok_or("-o requires a format name")?;
                cfg.output_format = name
                    .to_str()
                    .and_then(OutputFormat::from_name)
                    .ok_or_else(|| format!("unknown output format: {}", quotef_os(name)))?;
                i = i.saturating_add(2);
            }
            b"-b" | b"--boot" => {
                if let Some(id) =
                    word(i.saturating_add(1)).filter(|w| !w.as_encoded_bytes().starts_with(b"-"))
                {
                    cfg.boot_filter = Some(os_bytes(id).into_owned());
                    i = i.saturating_add(2);
                } else {
                    // Current boot: empty means "latest boot_id"
                    cfg.boot_filter = Some(Vec::new());
                    i = i.saturating_add(1);
                }
            }
            b"-k" | b"--dmesg" => {
                cfg.dmesg = true;
                i = i.saturating_add(1);
            }
            b"-n" | b"--lines" => {
                let count = word(i.saturating_add(1)).ok_or("-n requires a count")?;
                let n: usize = count
                    .to_str()
                    .and_then(|c| c.parse().ok())
                    .ok_or_else(|| format!("invalid count: {}", quotef_os(count)))?;
                cfg.num_entries = Some(n);
                i = i.saturating_add(2);
            }
            b"--grep" => {
                let pattern = word(i.saturating_add(1)).ok_or("--grep requires a pattern")?;
                cfg.grep_pattern = Some(os_bytes(pattern).into_owned());
                i = i.saturating_add(2);
            }
            b"--no-color" | b"--nocolor" => {
                cfg.color = false;
                i = i.saturating_add(1);
            }
            b"--no-pager" => {
                // We don't implement a pager; accept and ignore.
                i = i.saturating_add(1);
            }
            b"--list-fields" => {
                cfg.list_fields = true;
                i = i.saturating_add(1);
            }
            b"--disk-usage" => {
                cfg.disk_usage = true;
                i = i.saturating_add(1);
            }
            b"--vacuum-time" => {
                let duration = word(i.saturating_add(1))
                    .ok_or("--vacuum-time requires a duration (e.g. 2d, 1w)")?;
                let secs = duration
                    .to_str()
                    .and_then(parse_duration_secs)
                    .ok_or_else(|| format!("invalid duration: {}", quotef_os(duration)))?;
                cfg.vacuum_time = Some(secs);
                i = i.saturating_add(2);
            }
            b"--vacuum-size" => {
                let size = word(i.saturating_add(1))
                    .ok_or("--vacuum-size requires a size (e.g. 100M, 1G)")?;
                let bytes = size
                    .to_str()
                    .and_then(parse_size_bytes)
                    .ok_or_else(|| format!("invalid size: {}", quotef_os(size)))?;
                cfg.vacuum_size = Some(bytes);
                i = i.saturating_add(2);
            }
            b"-h" | b"--help" | b"help" => {
                cfg.show_help = true;
                i = i.saturating_add(1);
            }
            _ => {
                return Err(format!("unknown option: {}", quotef_os(arg)));
            }
        }
    }

    // JSON/json-pretty output disables color.
    if cfg.output_format == OutputFormat::Json || cfg.output_format == OutputFormat::JsonPretty {
        cfg.color = false;
    }

    Ok(cfg)
}

/// The datetime of `--since`/`--until` (named `option`), starting at
/// `args[i]`: one word, or two -- `YYYY-MM-DD HH:MM:SS` as the shell splits
/// it, the second word a time with a colon -- and how many words it took.
fn datetime_at<S: AsRef<OsStr>>(
    args: &[S],
    i: usize,
    option: &str,
) -> Result<(u64, usize), String> {
    let word = |i: usize| args.get(i).map(AsRef::as_ref);
    let first = word(i).ok_or_else(|| format!("{option} requires a datetime"))?;
    let Some(date) = first.to_str() else {
        return Err(format!("cannot parse datetime: {}", quotef_os(first)));
    };
    let time = word(i.saturating_add(1))
        .and_then(OsStr::to_str)
        .filter(|t| t.contains(':') && !t.starts_with('-'));
    let (text, taken) = match time {
        Some(time) => (format!("{date} {time}"), 2),
        None => (date.to_string(), 1),
    };
    let ts = parse_datetime(&text)
        .ok_or_else(|| format!("cannot parse datetime: {}", quotef_os(&text)))?;
    Ok((ts, taken))
}

// ============================================================================
// Filtering
// ============================================================================

fn apply_filters(entries: &[JournalEntry], cfg: &Config) -> Vec<JournalEntry> {
    let mut result: Vec<JournalEntry> = entries
        .iter()
        .filter(|e| {
            // Unit filter.
            if let Some(ref unit) = cfg.unit_filter
                && !unit_matches(&e.unit, unit)
            {
                return false;
            }

            // Priority filter: show entries at this level or more severe.
            if let Some(max_prio) = cfg.priority_filter
                && (e.priority as u8) > (max_prio as u8)
            {
                return false;
            }

            // Since filter.
            if let Some(since) = cfg.since
                && e.timestamp < since
            {
                return false;
            }

            // Until filter.
            if let Some(until) = cfg.until
                && e.timestamp > until
            {
                return false;
            }

            // Boot filter.
            if let Some(ref boot) = cfg.boot_filter
                && !boot.is_empty()
                && e.boot_id.as_bytes() != boot.as_slice()
            {
                return false;
            }
            // Empty boot_filter means "current boot" -- handled after
            // collecting entries (we pick the most recent boot_id).

            // Dmesg: only kernel messages.
            if cfg.dmesg && !is_kernel_unit(&e.unit) {
                return false;
            }

            // Grep filter.
            if let Some(ref pattern) = cfg.grep_pattern
                && !pattern_matches(&e.message, pattern)
            {
                return false;
            }

            true
        })
        .cloned()
        .collect();

    // Handle empty boot_filter (current boot = most recent boot_id).
    if let Some(ref boot) = cfg.boot_filter
        && boot.is_empty()
    {
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
        && result.len() > n
    {
        if cfg.reverse {
            // Already reversed: take the first n.
            result.truncate(n);
        } else {
            // Take the last n entries.
            let start = result.len().saturating_sub(n);
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

/// Write one entry to `out` in the configured format. The unit and the
/// message go out as their bytes, whatever they are.
///
/// # Errors
///
/// The write's: the caller stops there.
fn render_entry(out: &mut dyn Write, entry: &JournalEntry, cfg: &Config) -> io::Result<()> {
    match cfg.output_format {
        OutputFormat::Short => render_short(out, entry, cfg.color, false),
        OutputFormat::ShortPrecise => render_short(out, entry, cfg.color, true),
        OutputFormat::Json => writeln!(out, "{}", entry.to_json()),
        OutputFormat::JsonPretty => writeln!(out, "{}", entry.to_json_pretty()),
        OutputFormat::Cat => {
            out.write_all(&entry.message)?;
            out.write_all(b"\n")
        }
        OutputFormat::Verbose => render_verbose(out, entry, cfg.color),
    }
}

/// `short` and `short-precise`: `TIME UNIT[PID]: MESSAGE`.
fn render_short(
    out: &mut dyn Write,
    entry: &JournalEntry,
    color: bool,
    precise: bool,
) -> io::Result<()> {
    let ts = if precise {
        format_timestamp_precise(entry.timestamp, entry.timestamp_usec)
    } else {
        format_timestamp(entry.timestamp)
    };
    write!(out, "{ts} ")?;
    if entry.unit.is_empty() {
        out.write_all(b"unknown")?;
    } else {
        out.write_all(&entry.unit)?;
    }
    if entry.pid != 0 {
        write!(out, "[{}]", entry.pid)?;
    }
    out.write_all(b": ")?;
    let c = if color {
        entry.priority.ansi_color()
    } else {
        ""
    };
    out.write_all(c.as_bytes())?;
    out.write_all(&entry.message)?;
    if !c.is_empty() {
        out.write_all(b"\x1b[0m")?;
    }
    out.write_all(b"\n")
}

fn render_verbose(out: &mut dyn Write, entry: &JournalEntry, color: bool) -> io::Result<()> {
    let ts = format_timestamp_precise(entry.timestamp, entry.timestamp_usec);
    if color {
        let c = entry.priority.ansi_color();
        let reset = if c.is_empty() { "" } else { "\x1b[0m" };
        writeln!(out, "{c}{ts} [{:<8}] {}{reset}", entry.priority.label(), ts)?;
    } else {
        writeln!(out, "{ts} [{:<8}]", entry.priority.label())?;
    }
    writeln!(out, "    _PRIORITY={}", entry.priority as u8)?;
    if !entry.unit.is_empty() {
        out.write_all(b"    _UNIT=")?;
        out.write_all(&entry.unit)?;
        out.write_all(b"\n")?;
    }
    if entry.pid != 0 {
        writeln!(out, "    _PID={}", entry.pid)?;
    }
    if !entry.boot_id.is_empty() {
        writeln!(out, "    _BOOT_ID={}", entry.boot_id)?;
    }
    out.write_all(b"    MESSAGE=")?;
    out.write_all(&entry.message)?;
    out.write_all(b"\n")?;

    let known_keys: &[&str] = &[
        "ts",
        "ts_usec",
        "level",
        "service",
        "msg",
        "boot_id",
        "pid",
        "time",
        "__REALTIME_TIMESTAMP",
        "PRIORITY",
        "_SYSTEMD_UNIT",
        "SYSLOG_IDENTIFIER",
        "unit",
        "MESSAGE",
        "_BOOT_ID",
        "_PID",
    ];
    for (k, v) in &entry.fields {
        if !known_keys.contains(&k.as_str()) {
            write!(out, "    {k}=")?;
            out.write_all(v.bytes())?;
            out.write_all(b"\n")?;
        }
    }
    out.write_all(b"\n")
}

/// Write `entries` to standard output through [`to_stdout`]. In follow mode
/// each record is flushed as it is written.
///
/// # Errors
///
/// A failed write other than `EPIPE`.
fn render_all<'a>(
    entries: impl IntoIterator<Item = &'a JournalEntry>,
    cfg: &Config,
) -> io::Result<bool> {
    to_stdout(|out| {
        for entry in entries {
            render_entry(out, entry, cfg)?;
            // In follow mode a record must not wait in the buffer for the
            // next one; flushing per record costs a short listing nothing
            // that matters.
            if cfg.follow {
                out.flush()?;
            }
        }
        Ok(())
    })
}

/// Write to standard output through `body`, buffered, and flush, stopping at
/// the first write that fails. `Ok(true)` when it was all written. A reader
/// that has gone (`EPIPE`) is an ordinary end to a pipeline such as
/// `journalctl | head`, and is `Ok(false)`, said nothing about.
///
/// Every line this program prints goes through here: `println!`, which most
/// of them used until 2026-10-07, panics when the write fails -- a full disk,
/// or that reader gone -- so `journalctl --list-fields | head -1` ended in a
/// panic and status 101.
///
/// # Errors
///
/// A failed write other than `EPIPE`, for the caller to report.
fn to_stdout(body: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> io::Result<bool> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    match body(&mut out).and_then(|()| out.flush()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e),
    }
}

/// The status of a command whose output went through [`to_stdout`]: its own
/// `status` when the output was written, or its reader went away; 1, after
/// `journalctl: write error: REASON`, when it could not be written.
fn settle(written: io::Result<bool>, status: i32) -> i32 {
    match written {
        Ok(_) => status,
        Err(e) => {
            write_error(&e);
            1
        }
    }
}

/// A line on standard error. `eprintln!` panics when standard error cannot
/// be written (`2>/dev/full`); there is nowhere left to say so, so it is not
/// said.
fn say(line: &str) {
    let _ = writeln!(io::stderr().lock(), "{line}");
}

/// `journalctl: cannot WHAT PATH: REASON`, the reason in `strerror`'s words.
fn cannot(what: &str, path: &Path, e: &io::Error) {
    say(&format!(
        "journalctl: cannot {what} {}: {}",
        quotef_os(path),
        errmsg::strerror(e)
    ));
}

// ============================================================================
// Action commands
// ============================================================================

fn cmd_list_fields() -> i32 {
    let journal = read_journal(false);
    let failed = report_journal(&journal);
    let mut field_names: BTreeSet<String> = BTreeSet::new();

    for entry in &journal.entries {
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

    let written = to_stdout(|out| {
        if field_names.is_empty() {
            return out.write_all(b"No journal entries found.\n");
        }
        writeln!(out, "Known journal fields ({} total):", field_names.len())?;
        for name in &field_names {
            writeln!(out, "  {name}")?;
        }
        Ok(())
    });
    settle(written, i32::from(failed))
}

fn cmd_disk_usage() -> i32 {
    let (file_count, total_bytes) = journal_disk_usage();
    let written = to_stdout(|out| {
        writeln!(
            out,
            "Archived and active journals take up {} in {} file(s).",
            format_size(total_bytes),
            file_count,
        )
    });
    settle(written, 0)
}

fn cmd_vacuum_time(max_age_secs: u64) -> i32 {
    let cutoff = now_secs().saturating_sub(max_age_secs);
    let (files, unreadable) = discover();
    let vacuumed = vacuum_time(&files, cutoff, &is_rotated);

    let written = to_stdout(|out| {
        writeln!(
            out,
            "Vacuumed by time: removed {} entries, kept {} entries.",
            vacuumed.removed, vacuumed.kept
        )
    });
    for (path, e) in &unreadable {
        cannot("read", path, e);
    }
    vacuumed.report();
    settle(
        written,
        i32::from(!vacuumed.failures.is_empty() || !unreadable.is_empty()),
    )
}

/// Why one file of a vacuum was left as it was.
#[derive(Debug)]
enum VacuumFailure {
    /// It could not be opened and locked.
    Open(io::Error),
    /// It could not be read.
    Read(io::Error),
    /// Its new contents could not be written in its place.
    Rewrite(io::Error),
}

/// What a vacuum did, over every file it looked at.
#[derive(Debug, Default)]
struct Vacuumed {
    /// Lines removed, and kept, in the files it rewrote or left alone.
    removed: usize,
    kept: usize,
    /// Each file left as it was, and why.
    failures: Vec<(PathBuf, VacuumFailure)>,
}

impl Vacuumed {
    /// Say on stderr which files were left as they were, and why.
    fn report(&self) {
        for (path, failure) in &self.failures {
            let (what, e) = match failure {
                VacuumFailure::Open(e) => ("open", e),
                VacuumFailure::Read(e) => ("read", e),
                VacuumFailure::Rewrite(e) => ("rewrite", e),
            };
            cannot(what, path, e);
        }
    }
}

/// One file of a vacuum, under the journal's lock (design-decisions §1037):
/// the file as it is once locked, each line kept or dropped by `keep`, and --
/// if any is dropped -- the rest written to a new file renamed over it, or,
/// for an `archive` (a rotated copy) left empty, the file removed. The live
/// file is kept even when empty: its writers would only make it again.
///
/// A record another program appends meanwhile waits for the lock, then lands
/// in the new file. The old way -- read, filter, write back over the same
/// path -- lost it: `B-JOURNALCTL-VACUUM-LOSES-RECORDS-APPENDED-DURING-ITS-
/// REWRITE`.
///
/// Returns the lines removed and kept; a file that has gone -- rotated away
/// since it was listed -- is (0, 0).
fn vacuum_file(
    path: &Path,
    archive: bool,
    keep: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<(usize, usize), VacuumFailure> {
    let Some(mut held) = journalio::Locked::open(path).map_err(VacuumFailure::Open)? else {
        return Ok((0, 0));
    };
    let content = held.read_all().map_err(VacuumFailure::Read)?;
    let mut kept: Vec<&[u8]> = Vec::new();
    let mut removed = 0usize;
    for line in journalio::lines(&content) {
        if keep(line) {
            kept.push(line);
        } else {
            removed = removed.saturating_add(1);
        }
    }
    if removed > 0 {
        let written = if kept.is_empty() && archive {
            held.remove()
        } else {
            held.replace(&joined_lines(&kept))
        };
        written.map_err(VacuumFailure::Rewrite)?;
    }
    Ok((removed, kept.len()))
}

/// `--vacuum-time`: every record older than `cutoff` dropped from `files`.
/// A record young enough, and anything that is not a record, is kept exactly
/// as it was. `archive` says which files are rotated copies.
fn vacuum_time(files: &[PathBuf], cutoff: u64, archive: &dyn Fn(&Path) -> bool) -> Vacuumed {
    let mut vacuumed = Vacuumed::default();
    for file in files {
        let mut keep = |line: &[u8]| record_of(line).is_none_or(|e| e.timestamp >= cutoff);
        match vacuum_file(file, archive(file), &mut keep) {
            Ok((removed, kept)) => {
                vacuumed.removed = vacuumed.removed.saturating_add(removed);
                vacuumed.kept = vacuumed.kept.saturating_add(kept);
            }
            Err(failure) => vacuumed.failures.push((file.clone(), failure)),
        }
    }
    vacuumed
}

/// Lines back into a file's bytes, each ended by a newline.
fn joined_lines(lines: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out
}

fn cmd_vacuum_size(max_bytes: u64) -> i32 {
    let files = discover_journal_files();
    let (_, current_total) = journal_disk_usage();

    if current_total <= max_bytes {
        let written = to_stdout(|out| {
            writeln!(
                out,
                "Journal size ({}) is already within limit ({}).",
                format_size(current_total),
                format_size(max_bytes)
            )
        });
        return settle(written, 0);
    }

    let vacuumed = vacuum_size(&files, current_total.saturating_sub(max_bytes), &is_rotated);

    let (_, new_total) = journal_disk_usage();
    let written = to_stdout(|out| {
        writeln!(
            out,
            "Vacuumed by size: removed {} entries. Journal now uses {}.",
            vacuumed.removed,
            format_size(new_total)
        )
    });
    vacuumed.report();
    if !vacuumed.failures.is_empty() {
        say(&format!(
            "journalctl: {} journal file(s) could not be read or truncated",
            vacuumed.failures.len()
        ));
    }
    settle(written, i32::from(!vacuumed.failures.is_empty()))
}

/// `--vacuum-size`: lines dropped from the front of `files` -- the oldest
/// first, as [`discover`] lists them -- until `bytes_to_remove` bytes are
/// gone. `archive` says which files are rotated copies, removed when
/// emptied.
fn vacuum_size(
    files: &[PathBuf],
    bytes_to_remove: u64,
    archive: &dyn Fn(&Path) -> bool,
) -> Vacuumed {
    let mut vacuumed = Vacuumed::default();
    let mut bytes_removed: u64 = 0;
    for file in files {
        if bytes_removed >= bytes_to_remove {
            break;
        }
        let before = bytes_removed;
        let mut keep = |line: &[u8]| {
            if bytes_removed >= bytes_to_remove {
                return true;
            }
            // The line and its newline.
            bytes_removed = bytes_removed.saturating_add(len_u64(line).saturating_add(1));
            false
        };
        match vacuum_file(file, archive(file), &mut keep) {
            Ok((removed, _)) => vacuumed.removed = vacuumed.removed.saturating_add(removed),
            Err(failure) => {
                // Nothing of this file was removed after all.
                bytes_removed = before;
                vacuumed.failures.push((file.clone(), failure));
            }
        }
    }
    vacuumed
}

// ============================================================================
// Follow mode
// ============================================================================

/// `-f`: the last records, then each new one as it is written, until the
/// output cannot take more -- its reader gone (status 0) or a write failing
/// (status 1).
fn cmd_follow(cfg: &Config) -> i32 {
    // Print existing entries first (last N if -n specified, else last 10).
    // The offsets `-f` continues from come out of the SAME read: taking file
    // sizes afterwards, as this did, lost every record appended in between.
    let journal = read_journal(true);
    report_journal(&journal);
    let entries = &journal.entries;
    let num = cfg.num_entries.unwrap_or(10);
    let filtered = apply_filters(entries, cfg);

    let display_entries = if filtered.len() > num {
        filtered
            .get(filtered.len().saturating_sub(num)..)
            .unwrap_or_default()
    } else {
        &filtered
    };

    match render_all(display_entries, cfg) {
        Ok(true) => {}
        // The reader has gone: nothing left to follow for.
        Ok(false) => return 0,
        Err(e) => {
            write_error(&e);
            return 1;
        }
    }

    let mut follow = journal.follow;

    // Poll for new content. Only the bytes past each file's offset are read
    // -- this used to re-read every file whole, twice a second, and then
    // slice the `String` at the old length, which panicked whenever that
    // length fell inside a multi-byte character. Each file is known by its
    // identity, not its name: a rotation renames them all, and the place
    // reached in each goes with it (journalio's `Follow`).
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        let (current_files, _) = discover();
        for file in &current_files {
            // A file first seen now is read from its start.
            let bytes = match follow.read(file, true) {
                Ok(Some(bytes)) => bytes,
                // Rotated away between discovery and now; it is found under
                // its new name next round.
                Ok(None) => continue,
                Err(e) => {
                    cannot("read", file, &e);
                    continue;
                }
            };
            // Apply filters except reverse and num_entries.
            let fresh: Vec<JournalEntry> = journalio::lines(&bytes)
                .filter_map(record_of)
                .filter(|entry| entry_passes_filters(entry, cfg))
                .collect();
            match render_all(&fresh, cfg) {
                Ok(true) => {}
                Ok(false) => return 0,
                Err(e) => {
                    write_error(&e);
                    return 1;
                }
            }
        }
        // A file not found this round -- vacuumed away, or past the last
        // rotation -- is forgotten, so a new file given its identity is read
        // from its start.
        follow.end_round();
    }
}

/// Check if a single entry passes the configured filters (for follow mode).
fn entry_passes_filters(entry: &JournalEntry, cfg: &Config) -> bool {
    if let Some(ref unit) = cfg.unit_filter
        && !unit_matches(&entry.unit, unit)
    {
        return false;
    }
    if let Some(max_prio) = cfg.priority_filter
        && (entry.priority as u8) > (max_prio as u8)
    {
        return false;
    }
    if let Some(since) = cfg.since
        && entry.timestamp < since
    {
        return false;
    }
    if let Some(until) = cfg.until
        && entry.timestamp > until
    {
        return false;
    }
    if cfg.dmesg && !is_kernel_unit(&entry.unit) {
        return false;
    }
    if let Some(ref pattern) = cfg.grep_pattern
        && !pattern_matches(&entry.message, pattern)
    {
        return false;
    }
    true
}

// ============================================================================
// Help
// ============================================================================

/// `--help`'s text, a literal to a line: `scripts/check-help-vs-parser.py`
/// reads each option line on its own.
const USAGE: &[&str] = &[
    "Slate OS Journal Log Viewer v0.1.0",
    "",
    "Query and display messages from the journal.",
    "",
    "USAGE:",
    "  journalctl [OPTIONS]",
    "",
    "OPTIONS:",
    "  -u, --unit <UNIT>           Show entries from this unit/service",
    "  -p, --priority <LEVEL>      Show entries at this priority or higher",
    "      --since <DATETIME>      Show entries since datetime",
    "      --until <DATETIME>      Show entries until datetime",
    "  -f, --follow                Follow/tail mode",
    "  -r, --reverse               Show newest entries first",
    "  -o, --output <FORMAT>       Output format:",
    "                                short, short-precise, json,",
    "                                json-pretty, cat, verbose",
    "  -b, --boot [ID]             Show entries from boot (latest if no ID)",
    "  -k, --dmesg                 Show kernel messages only",
    "  -n, --lines <N>             Show last N entries",
    "      --grep <PATTERN>        Filter messages by substring",
    "      --no-color              Disable colored output",
    "      --no-pager              Do not pipe through pager",
    "",
    "INFORMATIONAL:",
    "      --list-fields           List all known field names",
    "      --disk-usage            Show disk usage of journal files",
    "",
    "MAINTENANCE:",
    "      --vacuum-time <DUR>     Remove entries older than duration",
    "                                (e.g. 2d, 1w, 3h, 1M, 1y)",
    "      --vacuum-size <SIZE>    Shrink journal to at most size",
    "                                (e.g. 100M, 1G, 500K)",
    "",
    "DATETIME FORMATS:",
    "  YYYY-MM-DD HH:MM:SS        Absolute datetime",
    "  YYYY-MM-DD                  Date only (midnight)",
    "  today / yesterday / now     Relative names",
    "  -Nd / -Nh / -Nm / -Ns      Relative offset",
    "",
    "PRIORITY LEVELS (highest to lowest):",
    "  emerg(0) alert(1) crit(2) err(3) warning(4) notice(5) info(6) debug(7)",
    "",
    "EXAMPLES:",
    "  journalctl -u net.dhcp              Show DHCP service logs",
    "  journalctl -p err                   Show errors and above",
    "  journalctl --since yesterday        Show last 24h",
    "  journalctl -k -b                    Kernel messages, current boot",
    "  journalctl -f -u kernel             Follow kernel messages",
    "  journalctl -o json-pretty -n 5      Last 5 entries as JSON",
    "  journalctl --vacuum-time 2w         Remove entries older than 2 weeks",
];

fn print_usage(out: &mut dyn Write) -> io::Result<()> {
    for line in USAGE {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

// ============================================================================
// Application logic
// ============================================================================

fn run<S: AsRef<OsStr>>(args: &[S]) -> i32 {
    let cfg = match parse_args(args) {
        Ok(c) => c,
        Err(e) => {
            say(&format!("journalctl: {e}"));
            say("Try 'journalctl --help' for usage.");
            return 1;
        }
    };

    if cfg.show_help {
        return settle(to_stdout(print_usage), 0);
    }

    // Dispatch action commands.
    if cfg.list_fields {
        return cmd_list_fields();
    }
    if cfg.disk_usage {
        return cmd_disk_usage();
    }
    if let Some(secs) = cfg.vacuum_time {
        return cmd_vacuum_time(secs);
    }
    if let Some(bytes) = cfg.vacuum_size {
        return cmd_vacuum_size(bytes);
    }

    // Follow mode: returns only once its output cannot take more.
    if cfg.follow {
        return cmd_follow(&cfg);
    }

    // Normal display mode.
    let journal = read_journal(false);
    let failed = report_journal(&journal);
    let entries = &journal.entries;
    if entries.is_empty() {
        say("No journal entries found.");
        say(&format!("(Looked in {JOURNAL_DIR} and fallback paths)"));
        return 1;
    }

    let filtered = apply_filters(entries, &cfg);
    if filtered.is_empty() {
        say("No entries match the specified filters.");
        return i32::from(failed);
    }

    settle(render_all(&filtered, &cfg), i32::from(failed))
}

/// `journalctl: write error: REASON`, for output that could not be written.
fn write_error(e: &io::Error) {
    say(&format!("journalctl: write error: {}", errmsg::strerror(e)));
}

// ============================================================================
// Entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // `args_os`: `std::env::args()` panics on an argument that is not UTF-8,
    // which is a word this program can be handed (a unit name, a pattern).
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    run(&args)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
fn main() {}

#[cfg(test)]
// CLAUDE.md allows the defensive lints inside `#[cfg(test)]`, where panicking
// on bad data is the point.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
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
        fields.insert("ts".to_string(), Value::Scalar(ts.to_string()));
        fields.insert("level".to_string(), Value::Text(level.to_string()));
        fields.insert("service".to_string(), Value::Text(unit.to_string()));
        fields.insert("msg".to_string(), Value::Text(msg.to_string()));
        if !boot_id.is_empty() {
            fields.insert("boot_id".to_string(), Value::Text(boot_id.to_string()));
        }
        if pid != 0 {
            fields.insert("pid".to_string(), Value::Scalar(pid.to_string()));
        }
        JournalEntry {
            timestamp: ts,
            timestamp_usec: 0,
            priority,
            unit: unit.as_bytes().to_vec(),
            message: msg.as_bytes().to_vec(),
            boot_id: boot_id.to_string(),
            pid,
            fields,
        }
    }

    /// A record whose message is not text: read as its bytes, shown as
    /// them, and written back as the same array (design-decisions §1063).
    #[test]
    fn a_message_that_is_not_text_is_its_bytes_both_ways() {
        let line =
            r#"{"ts":5,"level":"notice","service":"t","msg":[98,97,100,32,255],"facility":"user"}"#;
        let entry = JournalEntry::from_json_line(line).unwrap();
        assert_eq!(entry.message, b"bad \xff");
        assert_eq!(entry.unit, b"t");
        assert!(entry.to_json().contains(r#""msg":[98,97,100,32,255]"#));
        let mut out = Vec::new();
        let mut cfg = Config::new();
        cfg.color = false;
        render_entry(&mut out, &entry, &cfg).unwrap();
        assert!(out.ends_with(b": bad \xff\n"), "{out:?}");
        // An array that is not bytes is kept as it was, not half-read.
        let odd = r#"{"ts":1,"msg":"m","list":[1,2,300]}"#;
        let entry = JournalEntry::from_json_line(odd).unwrap();
        assert_eq!(
            entry.fields.get("list"),
            Some(&Value::Scalar("[1,2,300]".into()))
        );
        assert_eq!(entry.message, b"m");
    }

    /// A grep or a unit filter looks at the bytes, letters matched in
    /// either case.
    #[test]
    fn filters_match_bytes() {
        assert!(pattern_matches(b"bad \xff byte", "BYTE"));
        assert!(pattern_matches(b"\xff", ""));
        assert!(!pattern_matches(b"abc", "abcd"));
        assert!(is_kernel_unit(b"Kernel"));
        assert!(unit_matches(b"net.DHCP", "dhcp"));
    }

    fn sample_entries() -> Vec<JournalEntry> {
        vec![
            make_entry(1000, "emerg", "kernel", "panic: out of memory", "boot1", 0),
            make_entry(1001, "alert", "kernel", "watchdog timeout", "boot1", 0),
            make_entry(
                1002,
                "crit",
                "fs.ext4",
                "journal checksum error",
                "boot1",
                100,
            ),
            make_entry(1003, "err", "net.dhcp", "lease expired", "boot1", 200),
            make_entry(
                1004,
                "warning",
                "sshd",
                "auth failure from 10.0.0.1",
                "boot1",
                300,
            ),
            make_entry(1005, "notice", "init", "service started: crond", "boot1", 1),
            make_entry(
                1006,
                "info",
                "net.dhcp",
                "lease renewed for 10.0.2.15",
                "boot1",
                200,
            ),
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

    // The parser's own tests -- JSON objects, strings, escapes, byte arrays
    // -- are journalrec's, with the parser.

    // =========================================================================
    // JournalEntry parsing tests
    // =========================================================================

    #[test]
    fn test_entry_from_json_line_basic() {
        let line = make_json_line(1716000000, "info", "net.dhcp", "lease renewed", "abc", 42);
        let entry = JournalEntry::from_json_line(&line).unwrap();
        assert_eq!(entry.timestamp, 1716000000);
        assert_eq!(entry.priority, Priority::Info);
        assert_eq!(entry.unit, b"net.dhcp");
        assert_eq!(entry.message, b"lease renewed");
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
        assert_eq!(
            entry.fields.get("custom_key").and_then(Value::text),
            Some("custom_val")
        );
    }

    #[test]
    fn test_entry_from_json_line_systemd_compat() {
        // Simulates systemd journal JSON export fields.
        let line = r#"{"__REALTIME_TIMESTAMP":"1716000000000000","PRIORITY":"3","_SYSTEMD_UNIT":"sshd","MESSAGE":"auth failed","_BOOT_ID":"xyz","_PID":"123"}"#;
        let entry = JournalEntry::from_json_line(line).unwrap();
        assert_eq!(entry.timestamp, 1716000000);
        assert_eq!(entry.priority, Priority::Error);
        assert_eq!(entry.unit, b"sshd");
        assert_eq!(entry.message, b"auth failed");
        assert_eq!(entry.boot_id, "xyz");
        assert_eq!(entry.pid, 123);
    }

    #[test]
    fn test_entry_to_json_roundtrip() {
        let original_line = make_json_line(5000, "warning", "net.tcp", "retransmit", "boot99", 777);
        let entry = JournalEntry::from_json_line(&original_line).unwrap();
        let serialized = entry.to_json();
        let reparsed = JournalEntry::from_json_line(&serialized).unwrap();
        assert_eq!(reparsed.timestamp, 5000);
        assert_eq!(reparsed.priority, Priority::Warning);
        assert_eq!(reparsed.unit, b"net.tcp");
        assert_eq!(reparsed.message, b"retransmit");
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
        cfg.unit_filter = Some("net.dhcp".into());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| unit_matches(&e.unit, "net.dhcp")));
    }

    #[test]
    fn test_filter_by_unit_case_insensitive() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.unit_filter = Some("NET.DHCP".into());
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
        cfg.boot_filter = Some("boot2".into());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| e.boot_id == "boot2"));
    }

    #[test]
    fn test_filter_by_boot_latest() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.boot_filter = Some(Vec::new()); // empty = latest
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
            u == b"kernel" || u == b"kern" || u == b"dmesg"
        }));
    }

    #[test]
    fn test_filter_grep() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.grep_pattern = Some("lease".into());
        let filtered = apply_filters(&entries, &cfg);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_grep_case_insensitive() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.grep_pattern = Some("LEASE".into());
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
        cfg.boot_filter = Some("boot1".into());
        let filtered = apply_filters(&entries, &cfg);
        // boot1 entries with priority <= Error: emerg, alert, crit, err.
        assert_eq!(filtered.len(), 4);
    }

    #[test]
    fn test_filter_no_matches() {
        let entries = sample_entries();
        let mut cfg = Config::new();
        cfg.unit_filter = Some("nonexistent.service".into());
        let filtered = apply_filters(&entries, &cfg);
        assert!(filtered.is_empty());
    }

    // =========================================================================
    // Pattern matching tests
    // =========================================================================

    #[test]
    fn test_pattern_matches_basic() {
        assert!(pattern_matches(b"hello world", "hello"));
        assert!(pattern_matches(b"hello world", "world"));
        assert!(!pattern_matches(b"hello world", "xyz"));
    }

    #[test]
    fn test_pattern_matches_case_insensitive() {
        assert!(pattern_matches(b"Hello World", "hello"));
        assert!(pattern_matches(b"hello", "HELLO"));
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
        assert_eq!(cfg.unit_filter.as_deref(), Some(&b"sshd"[..]));
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
        let args = vec!["journalctl".to_string(), "-n".to_string(), "25".to_string()];
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
        assert_eq!(cfg.grep_pattern.as_deref(), Some(&b"error"[..]));
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
        assert_eq!(cfg.boot_filter, Some(b"boot42".to_vec()));
    }

    #[test]
    fn test_parse_args_boot_no_id() {
        let args = vec!["journalctl".to_string(), "-b".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.boot_filter, Some(Vec::new()));
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

    /// A value is quoted in the message when it needs to be -- a newline in
    /// it would otherwise forge a line of the diagnostic -- and left bare
    /// when it does not.
    #[test]
    fn a_rejected_value_is_quoted_when_it_must_be() {
        let err = |words: &[&str]| parse_args(words).err().unwrap();
        assert_eq!(err(&["j", "--bogus"]), "unknown option: --bogus");
        assert_eq!(err(&["j", "a\nb"]), "unknown option: 'a'$'\\n''b'");
        assert_eq!(err(&["j", "-p", "x y"]), "unknown priority: 'x y'");
        assert_eq!(err(&["j", "-n", "-1"]), "invalid count: -1");
        assert_eq!(
            err(&["j", "--since", "2024-13-01", "10:00"]),
            "cannot parse datetime: '2024-13-01 10:00'"
        );
        assert_eq!(err(&["j", "--until"]), "--until requires a datetime");
    }

    /// Words that are not text, which `std::env::args()` panicked on: taken
    /// as bytes where a record's bytes are matched, refused where a value
    /// must be text.
    #[cfg(unix)]
    #[test]
    fn words_that_are_not_text_are_bytes_or_refused() {
        use std::os::unix::ffi::OsStrExt;
        let os = |b: &[u8]| OsStr::from_bytes(b).to_os_string();
        let args = [
            os(b"j"),
            os(b"-u"),
            os(b"u\xff"),
            os(b"--grep"),
            os(b"\xfe"),
            os(b"-b"),
            os(b"\xfd"),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.unit_filter.as_deref(), Some(&b"u\xff"[..]));
        assert_eq!(cfg.grep_pattern.as_deref(), Some(&b"\xfe"[..]));
        assert_eq!(cfg.boot_filter.as_deref(), Some(&b"\xfd"[..]));
        for option in [
            &b"-p"[..],
            b"-o",
            b"-n",
            b"--since",
            b"--vacuum-time",
            b"--vacuum-size",
        ] {
            let args = [os(b"j"), os(option), os(b"\xff")];
            let e = parse_args(&args).err().unwrap();
            // GNU's shell-escape quoting of a lone byte that is not text.
            assert!(e.ends_with(": ''$'\\377'"), "{e}");
        }
        assert!(parse_args(&[os(b"j"), os(b"--\xff")]).is_err());
    }

    /// A command's status after its output: its own when the output was
    /// written or its reader went away, 1 when a write failed.
    #[test]
    fn a_failed_write_is_status_one() {
        assert_eq!(settle(Ok(true), 0), 0);
        assert_eq!(settle(Ok(false), 3), 3);
        let full = io::Error::other("no space");
        assert_eq!(settle(Err(full), 0), 1);
    }

    #[test]
    fn the_usage_is_written_a_line_at_a_time() {
        let mut out = Vec::new();
        print_usage(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("Slate OS Journal Log Viewer"));
        assert_eq!(text.lines().count(), USAGE.len());
        assert!(text.contains("  -u, --unit <UNIT>"));
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
        cfg.unit_filter = Some("sshd".into());
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
        cfg.grep_pattern = Some("foobar".into());
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
    // Dates (the calendar itself is journalrec::Utc's, tested there)
    // =========================================================================

    /// A date that is not a real one, or is before any record, is refused
    /// rather than rolled over; a time field that does not parse is refused
    /// rather than taken as 0.
    #[test]
    fn a_date_that_is_not_one_is_refused() {
        for bad in [
            "2024-02-30",
            "2023-02-29",
            "1969-12-31",
            "1960-06-01",
            "2024-01-01 25:00",
            "2024-01-01 10:60",
            "2024-01-01 10:00:60",
            "2024-01-01 10:xx",
            "2024-01-01 1:2:3:4",
            "2024-01",
            "2024-01-01-01",
        ] {
            assert_eq!(parse_datetime(bad), None, "{bad}");
        }
        assert_eq!(parse_datetime("2024-02-29"), Some(1_709_164_800));
        assert_eq!(
            parse_datetime("2024-02-29 12"),
            Some(1_709_164_800 + 12 * 3600)
        );
        assert_eq!(
            parse_datetime("2024-02-29 12:30"),
            Some(1_709_164_800 + 12 * 3600 + 1800)
        );
    }

    /// A size or a duration too large to count is refused. Multiplied out
    /// unchecked, `--vacuum-size 17179869184G` was 2^64 bytes, wrapped round
    /// to 0, and removed the whole journal; a multi-byte unit panicked.
    #[test]
    fn a_vacuum_limit_too_large_to_count_is_refused() {
        assert_eq!(parse_size_bytes("17179869184G"), None);
        assert_eq!(
            parse_size_bytes("17179869183G"),
            Some(17_179_869_183 * 1_073_741_824)
        );
        assert_eq!(parse_duration_secs("600000000000y"), None);
        assert_eq!(parse_duration_secs("2d"), Some(172_800));
        assert_eq!(parse_duration_secs("1w"), Some(604_800));
        assert_eq!(parse_duration_secs("5\u{e9}"), None);
        assert_eq!(parse_size_bytes("5\u{e9}"), None);
        assert_eq!(parse_duration_secs(""), None);
        assert_eq!(parse_size_bytes(""), None);
        assert_eq!(parse_size_bytes("100"), Some(100));
        assert_eq!(parse_duration_secs("120"), Some(120));
        assert_eq!(parse_size_bytes("1x"), None);
    }

    /// What used to hang, overflow or panic is answered at once.
    #[test]
    fn extreme_dates_are_answered_at_once() {
        assert_eq!(parse_datetime("9223372036854775807-01-01"), None);
        assert_eq!(parse_datetime("-18446744073709551615d"), None);
        assert_eq!(parse_datetime("-99999999999999999999d"), None);
        assert_eq!(parse_datetime("-5\u{e9}"), None);
        assert_eq!(parse_datetime("-d"), None);
        assert_eq!(format_timestamp(u64::MAX), "584554051223-11-09 07:00:15");
        assert_eq!(format_timestamp(1_716_000_000), "2024-05-18 02:40:00");
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
        assert_eq!(cfg.unit_filter.as_deref(), Some(&b"sshd"[..]));
        assert_eq!(cfg.priority_filter, Some(Priority::Error));
        assert_eq!(cfg.num_entries, Some(50));
        assert!(cfg.reverse);
    }

    // --- Decoding (the cases themselves are journalrec's) ---

    /// What the shared reader decodes reaches the record: a message in UTF-8,
    /// or escaped, keeps its characters. The private parser this replaced
    /// was fixed for that on 2026-09-26; `syslogd`'s copy never was.
    #[test]
    fn a_record_keeps_its_characters() {
        let entry =
            JournalEntry::from_json_line("{\"ts\":1,\"msg\":\"caf\u{e9} \\u00e9 \\ud83d\\ude00\"}")
                .unwrap();
        assert_eq!(entry.message, "caf\u{e9} \u{e9} \u{1f600}".as_bytes());
        assert_eq!(entry.timestamp, 1);
    }

    // --- Reading as bytes (B-JOURNALCTL-SKIPS-A-WHOLE-LOG-FILE-...) ---

    use scratchdir::ScratchDir;

    fn rec(ts: u64, msg: &str) -> String {
        format!("{{\"ts\":{ts},\"level\":\"info\",\"service\":\"t\",\"msg\":\"{msg}\"}}")
    }

    /// One byte that is not UTF-8 costs its own line, not the file.
    #[test]
    fn a_bad_byte_costs_its_line_not_the_file() {
        let dir = ScratchDir::new("journalctl_bad_byte");
        let file = dir.path("syslog.jsonl");
        let mut bytes = format!("{}\n", rec(1, "a")).into_bytes();
        bytes.extend_from_slice(b"\xff\xfe not a record\n");
        bytes.extend_from_slice(format!("{}\n", rec(2, "b")).as_bytes());
        fs::write(&file, &bytes).unwrap();

        let j = read_files(std::slice::from_ref(&file), Vec::new(), false);
        let msgs: Vec<&[u8]> = j.entries.iter().map(|e| e.message.as_slice()).collect();
        assert_eq!(msgs, [&b"a"[..], &b"b"[..]]);
        assert_eq!(j.not_records, [(file, 1)]);
        assert!(j.unreadable.is_empty());
    }

    /// Text lines -- `logger`'s, in `/var/log/syslog` -- are counted, so
    /// they can be reported, rather than vanishing.
    #[test]
    fn lines_that_are_not_records_are_counted_and_blank_ones_are_not() {
        let dir = ScratchDir::new("journalctl_text_lines");
        let file = dir.path("syslog");
        fs::write(
            &file,
            "<13>Sep 26 06:37:16 t: hi\n\n<13>Sep 26 06:37:17 t: ho\n",
        )
        .unwrap();
        let j = read_files(std::slice::from_ref(&file), Vec::new(), false);
        assert!(j.entries.is_empty());
        assert_eq!(j.not_records, [(file, 2)]);
    }

    /// A file that cannot be read is reported, not skipped.
    #[test]
    fn an_unreadable_file_is_reported() {
        let dir = ScratchDir::new("journalctl_unreadable");
        // A directory where a file is expected: `fs::read` fails, and not
        // with NotFound.
        let not_a_file = dir.path("syslog.jsonl");
        fs::create_dir(&not_a_file).unwrap();
        let j = read_files(std::slice::from_ref(&not_a_file), Vec::new(), false);
        assert_eq!(j.unreadable.len(), 1);
        assert_eq!(j.unreadable[0].0, not_a_file);
    }

    /// A file rotated away between discovery and reading is simply absent.
    #[test]
    fn a_file_that_vanished_is_not_an_error() {
        let dir = ScratchDir::new("journalctl_vanished");
        let j = read_files(&[dir.path("gone.jsonl")], Vec::new(), false);
        assert!(j.unreadable.is_empty() && j.entries.is_empty());
    }

    /// A listing shows an unterminated last record; `-f` holds it back and
    /// shows it once, when its newline arrives.
    #[test]
    fn an_unterminated_last_record_is_shown_by_a_listing_and_held_by_follow() {
        let dir = ScratchDir::new("journalctl_unterminated");
        let file = dir.path("syslog.jsonl");
        fs::write(&file, format!("{}\n{}", rec(1, "a"), rec(2, "b"))).unwrap();

        let listing = read_files(std::slice::from_ref(&file), Vec::new(), false);
        assert_eq!(listing.entries.len(), 2);

        let mut held = read_files(std::slice::from_ref(&file), Vec::new(), true);
        assert_eq!(held.entries.len(), 1);
        assert_eq!(held.follow.read(&file, true).unwrap(), Some(Vec::new()));

        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        io::Write::write_all(&mut f, b"\n").unwrap();
        drop(f);
        let bytes = held.follow.read(&file, true).unwrap().unwrap();
        assert_eq!(bytes, format!("{}\n", rec(2, "b")).into_bytes());
    }

    /// Only the bytes past the offset are read, and a record written in two
    /// pieces -- split inside a multi-byte character -- arrives whole. The
    /// `String` slicing this replaced panicked on exactly that split.
    #[test]
    fn follow_joins_a_record_written_in_two_pieces_across_a_character() {
        let dir = ScratchDir::new("journalctl_torn");
        let file = dir.path("syslog.jsonl");
        fs::write(&file, b"").unwrap();
        let mut follow = journalio::Follow::new();

        let whole = format!("{}\n", rec(3, "caf\u{e9}"));
        let bytes = whole.as_bytes();
        // Split inside the two bytes of U+00E9.
        let cut = whole.find('\u{e9}').unwrap() + 1;
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();

        io::Write::write_all(&mut f, &bytes[..cut]).unwrap();
        assert_eq!(follow.read(&file, true).unwrap(), Some(Vec::new()));

        io::Write::write_all(&mut f, &bytes[cut..]).unwrap();
        let read = follow.read(&file, true).unwrap().unwrap();
        let lines: Vec<&[u8]> = journalio::lines(&read).collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(record_of(lines[0]).unwrap().message, "caf\u{e9}".as_bytes());
    }

    /// A file that got shorter was truncated, and is read again from its
    /// start rather than from the old offset.
    #[test]
    fn follow_rereads_a_truncated_file_from_the_start() {
        let dir = ScratchDir::new("journalctl_truncated");
        let file = dir.path("syslog.jsonl");
        fs::write(&file, format!("{}\n{}\n", rec(1, "old"), rec(2, "older"))).unwrap();
        let mut follow = journalio::Follow::new();
        let read = follow.read(&file, true).unwrap().unwrap();
        assert_eq!(journalio::lines(&read).count(), 2);

        // `fs::write` truncates the same file rather than replacing it.
        fs::write(&file, format!("{}\n", rec(3, "new"))).unwrap();
        let read = follow.read(&file, true).unwrap().unwrap();
        assert_eq!(read, format!("{}\n", rec(3, "new")).into_bytes());
    }

    // --- Rotated files, and vacuums under the journal's lock ---

    #[test]
    fn rotated_copies_are_found_oldest_first() {
        let dir = ScratchDir::new("journalctl_rotated");
        let live = dir.path("syslog.jsonl");
        for name in [
            "syslog.jsonl",
            "syslog.jsonl.1",
            "syslog.jsonl.10",
            "syslog.jsonl.2",
            "syslog.jsonl.0",
            "syslog.jsonl.x",
            "syslog.jsonl.",
            ".syslog.jsonl.12.tmp",
            "other.1",
        ] {
            fs::write(dir.path(name), b"").unwrap();
        }
        let found = rotated_siblings(&live);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["syslog.jsonl.10", "syslog.jsonl.2", "syslog.jsonl.1"]
        );
    }

    #[test]
    fn only_syslogds_live_log_has_rotated_copies() {
        assert!(is_rotated(Path::new("/var/log/syslog.jsonl.3")));
        assert!(!is_rotated(Path::new("/var/log/syslog.jsonl")));
        assert!(!is_rotated(Path::new("/var/log/syslog.1")));
        assert!(!is_rotated(Path::new("/tmp/syslog.jsonl.3")));
    }

    /// A vacuum by time keeps what is young and what is not a record, and
    /// removes a rotated copy it empties -- never the live file.
    #[test]
    fn vacuum_by_time_trims_and_removes_an_emptied_archive() {
        let dir = ScratchDir::new("journalctl_vacuum_time");
        let old = dir.path("syslog.jsonl.1");
        let live = dir.path("syslog.jsonl");
        fs::write(&old, format!("{}\n{}\n", rec(10, "a"), rec(20, "b"))).unwrap();
        fs::write(
            &live,
            format!("{}\nnot a record\n{}\n", rec(30, "c"), rec(300, "d")),
        )
        .unwrap();
        let archive = |p: &Path| p == old.as_path();
        let v = vacuum_time(&[old.clone(), live.clone()], 100, &archive);
        assert_eq!((v.removed, v.kept), (3, 2));
        assert!(v.failures.is_empty());
        assert!(!old.exists(), "an emptied archive is removed");
        assert_eq!(
            fs::read_to_string(&live).unwrap(),
            format!("not a record\n{}\n", rec(300, "d"))
        );
        // The live file stays, even when nothing is left in it.
        let v = vacuum_time(std::slice::from_ref(&live), 1000, &archive);
        assert_eq!(v.removed, 1);
        assert_eq!(fs::read_to_string(&live).unwrap(), "not a record\n");
    }

    /// A vacuum by size trims the oldest file first, and stops as soon as
    /// enough is gone.
    #[test]
    fn vacuum_by_size_trims_oldest_first() {
        let dir = ScratchDir::new("journalctl_vacuum_size");
        let old = dir.path("syslog.jsonl.1");
        let live = dir.path("syslog.jsonl");
        let (a, b, c) = (rec(1, "a"), rec(2, "b"), rec(3, "c"));
        fs::write(&old, format!("{a}\n{b}\n")).unwrap();
        fs::write(&live, format!("{c}\n")).unwrap();
        let archive = |p: &Path| p == old.as_path();
        // One byte more than the first line: the first two go.
        let want = u64::try_from(a.len()).unwrap() + 2;
        let v = vacuum_size(&[old.clone(), live.clone()], want, &archive);
        assert_eq!(v.removed, 2);
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(&live).unwrap(), format!("{c}\n"));
    }

    #[test]
    fn a_file_that_cannot_be_rewritten_is_left_whole_and_reported() {
        let dir = ScratchDir::new("journalctl_vacuum_fail");
        // A directory: it opens, and cannot be read as a file.
        let not_a_file = dir.path("syslog.jsonl");
        fs::create_dir(&not_a_file).unwrap();
        let v = vacuum_time(std::slice::from_ref(&not_a_file), 100, &|_| false);
        assert_eq!(v.failures.len(), 1);
        assert!(not_a_file.is_dir());
    }

    #[test]
    fn lines_round_trip_through_vacuums_split_and_join() {
        for text in ["", "a\n", "a\nb\n", "a\nb", "\n", "a\n\nb\n"] {
            let lines: Vec<&[u8]> = journalio::lines(text.as_bytes()).collect();
            let back = joined_lines(&lines);
            let expected = if text.is_empty() || text.ends_with('\n') {
                text.to_string()
            } else {
                format!("{text}\n")
            };
            assert_eq!(back, expected.as_bytes(), "{text:?}");
        }
    }
}
