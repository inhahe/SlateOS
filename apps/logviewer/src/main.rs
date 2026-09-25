//! `Slate OS` System Log Viewer
//!
//! Reads log files and shows them as a list that can be filtered, searched
//! and followed. The system journal (`journalrec::MAIN_LOG_PATH`,
//! `/var/log/syslog.jsonl`) is opened at start; any other log opens in a tab
//! of its own with Open or Ctrl+O.
//!
//! - JSON lines (the OS's own format, written by `journalrec`) and plain text.
//! - Following a log as it grows: while a log with a file is open, the window
//!   reads what the file has grown by once a second. A line caught
//!   half-written becomes one entry when it is finished, and a log truncated
//!   or replaced underneath is read again from the start.
//! - A log too large to read whole is read from its end (the last
//!   [`MAX_READ_BYTES`]); at most the newest [`MAX_LOG_ENTRIES`] are kept.
//! - Filtering by level, source, time range and bookmarks; searching by text
//!   or by POSIX extended regular expression (`ere`, the engine `grep -E`
//!   uses here).
//! - A detail view that shows an entry whole, a statistics view, and several
//!   logs in tabs.
//! - Export of the entries shown, as the file's own bytes.
//!
//! Every control answers the pointer -- the renderer records a hit box where
//! it draws each one (`guitk::frame::Frame`) -- and every key is on the F1
//! card.

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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::dialog::{FileDialog, FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, Response};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

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
/// The most of a log file read at once; past it, only the end is read.
const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;
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

    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Trace => pal.overlay0,
            Self::Debug => pal.subtext0,
            Self::Info => pal.blue,
            Self::Warn => pal.yellow,
            Self::Error => pal.red,
            Self::Fatal => pal.mauve,
        }
    }

    fn bg_color(self, pal: &Palette) -> Color {
        match self {
            Self::Trace => pal.surface0,
            Self::Debug => pal.surface0,
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
            // The journal's own words (`journalrec::PRIORITY_NAMES`, syslog's
            // eight priorities). `notice` read as nothing and every notice was
            // filed as info by the fallback; `emerg` and `alert`, the two most
            // urgent things a log can say, fell to the same fallback.
            "NOTICE" => Some(Self::Info),
            "EMERG" | "ALERT" => Some(Self::Fatal),
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
        clock_of(self.timestamp)
    }
}

/// A time as the list's timestamp column shows it: `HH:MM:SS.mmm`, in UTC --
/// the one spelling, so a time-range chip names its bound the way the row it
/// came from did.
fn clock_of(ms: u64) -> String {
    let total_secs = ms / 1000;
    let millis = ms % 1000;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = (total_secs / 3600) % 24;
    format!("{hours:02}:{mins:02}:{secs:02}.{millis:03}")
}

// ============================================================================
// JSON-lines Parser
// ============================================================================

/// Whether a log is JSON lines, going by its first line.
fn looks_like_json(bytes: &[u8]) -> bool {
    let first = bytes.split(|b| *b == b'\n').next().unwrap_or_default();
    first.trim_ascii_start().first() == Some(&b'{')
}

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
        .map_or(0, as_millis);

    let level = fields
        .iter()
        .find(|(k, _)| k == "level" || k == "lvl" || k == "severity")
        .and_then(|(_, v)| LogLevel::from_str(v))
        .unwrap_or(LogLevel::Info);

    let source = fields
        .iter()
        .find(|(k, _)| {
            k == "source"
                || k == "src"
                || k == "service"
                || k == "component"
                || k == "module"
                || k == "logger"
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
                    | "service"
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

/// A timestamp in milliseconds, whether it was written in seconds or in
/// milliseconds.
///
/// The system journal writes seconds (`journalrec::Record::ts`); other tools
/// write milliseconds. A reading below 100 000 000 000 is taken as seconds --
/// that is the year 5138 in seconds and 1973 in milliseconds, so no plausible
/// log line is on the wrong side of it. Taken as milliseconds, every journal
/// entry was dated within a few hours of the epoch and the time column read
/// 07:00 for a whole day's log.
fn as_millis(t: u64) -> u64 {
    if t < 100_000_000_000 {
        t.saturating_mul(1000)
    } else {
        t
    }
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

/// What reading more of a log did to its entries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Growth {
    /// How many more entries there are.
    added: usize,
    /// How many of the oldest were let go to stay within [`MAX_LOG_ENTRIES`]:
    /// every index into the entries moves down by this much.
    dropped: usize,
    /// Whether the last entry, read while its line was still being written,
    /// was read again whole. The count does not move, but the entry did.
    revised: bool,
    /// Whether the file was read again from the start -- it was truncated or
    /// replaced -- so every index into the old entries is void.
    reread: bool,
}

impl Growth {
    /// Whether the entries are any different.
    fn changed(self) -> bool {
        self.added > 0 || self.dropped > 0 || self.revised || self.reread
    }
}

/// FNV-1a over `bytes`, continuing from `hash`: a fingerprint of what has been
/// read of a file, so an export can tell the file it re-reads is still the
/// one whose lines are on screen.
fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Where [`fnv1a`] starts.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Where [`LogFile::id`]s come from.
static NEXT_LOG_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Debug)]
struct LogFile {
    /// Which log this is, for as long as the program runs: what a cache of
    /// something worked out from its entries is keyed by. Not its place in
    /// the tab strip, which closing a tab renumbers.
    id: u64,
    /// Bumped by every change to the entries -- lines read, a line finished,
    /// a bookmark -- so a cache keyed by it cannot outlive what it describes.
    revision: u64,
    name: String,
    path: String,
    entries: Vec<LogEntry>,
    is_json: bool,
    /// The file this was read from, when it was read from one: what the tail
    /// re-reads.
    source: Option<std::path::PathBuf>,
    /// How many bytes of `source` have been read.
    read_len: u64,
    /// [`fnv1a`] of the bytes of `source` read so far, from `start_offset`.
    read_hash: u64,
    /// The bytes after the last newline read, when what was read did not end
    /// in one: the last entry, shown -- a file that simply ends without a
    /// newline is complete as far as anyone can know -- but kept aside as
    /// well, so that when more arrives it is read again with it and a line
    /// caught half-written becomes one entry rather than two.
    ///
    /// Bytes, not text, so a character cut in half by the read is whole again
    /// when the rest of it arrives.
    unfinished: Vec<u8>,
    /// The number the next line read will be given.
    next_line: usize,
    /// Where in the file line 1 starts: 0, unless the file was too large to
    /// read whole and only its end was read -- which is also what an export
    /// counts lines from, so the lines it writes are the ones shown.
    start_offset: u64,
    /// How many of the oldest entries were let go to stay within
    /// [`MAX_LOG_ENTRIES`], so the view can say it is not the whole log.
    dropped: usize,
}

impl LogFile {
    fn new(name: &str, path: &str) -> Self {
        Self {
            id: NEXT_LOG_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            revision: 0,
            name: name.into(),
            path: path.into(),
            entries: Vec::new(),
            is_json: false,
            source: None,
            read_len: 0,
            read_hash: FNV_OFFSET,
            unfinished: Vec::new(),
            next_line: 1,
            start_offset: 0,
            dropped: 0,
        }
    }

    /// Replace the entries with those of `content`, read as a whole log.
    /// **Tests only**: a file is read as bytes ([`parse_bytes`](Self::parse_bytes)).
    #[cfg(test)]
    fn parse_content(&mut self, content: &str) {
        self.parse_bytes(content.as_bytes());
    }

    /// Replace the entries with those of `bytes`, read as a whole log.
    fn parse_bytes(&mut self, bytes: &[u8]) {
        self.touch();
        self.entries.clear();
        self.unfinished.clear();
        self.next_line = 1;
        self.dropped = 0;
        self.is_json = looks_like_json(bytes);
        // A fresh read has no view to adjust, so what it did is not wanted.
        self.append_bytes(bytes);
    }

    /// Parse `bytes` onto the end of the log. Returns what that did.
    ///
    /// A last line with no newline after it is an entry like any other, and
    /// is also kept in [`unfinished`](Self::unfinished): when more arrives,
    /// that entry is taken back and its line read again with what followed.
    ///
    /// The *newest* entries are kept. This kept the first [`MAX_LOG_ENTRIES`]
    /// and ignored the rest, which for a log -- where what happened last is
    /// what somebody opened it to see -- is the wrong end.
    fn append_bytes(&mut self, bytes: &[u8]) -> Growth {
        let mut growth = Growth::default();
        if bytes.is_empty() {
            return growth;
        }
        self.touch();
        let mut joined = std::mem::take(&mut self.unfinished);
        let mut bookmarked = false;
        if !joined.is_empty() {
            // The unfinished line's entry is the newest one, so letting the
            // oldest go cannot have taken it.
            if let Some(last) = self.entries.pop() {
                bookmarked = last.bookmarked;
                self.next_line = last.line_number;
                growth.revised = true;
            }
        }
        joined.extend_from_slice(bytes);
        let mut lines: Vec<&[u8]> = joined.split(|b| *b == b'\n').collect();
        // What follows the last newline: nothing when the text ended in one,
        // otherwise a line still open.
        let open = lines.pop().unwrap_or_default();
        if !open.is_empty() {
            lines.push(open);
        }
        // Lines that would be let go the moment they were added -- a file with
        // more lines than are kept -- are counted and not parsed, so reading a
        // large log costs the entries kept rather than every line in it.
        let excess = self
            .entries
            .len()
            .saturating_add(lines.len())
            .saturating_sub(MAX_LOG_ENTRIES);
        let skip = excess.saturating_sub(self.entries.len()).min(lines.len());
        let mut pushed = 0usize;
        for (n, line) in lines.into_iter().enumerate() {
            let number = self.next_line;
            self.next_line = self.next_line.saturating_add(1);
            if n < skip {
                continue;
            }
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            // A log is text, but a line of it may not be: decoding lossily is
            // for *display* only -- an export writes the file's own bytes
            // ([`exact_lines`](Self::exact_lines)).
            let text = String::from_utf8_lossy(line);
            let mut entry = if self.is_json {
                parse_json_line(&text, number).unwrap_or_else(|| parse_plain_line(&text, number))
            } else {
                parse_plain_line(&text, number)
            };
            // The line read again is the first, and keeps its bookmark.
            if n == 0 {
                entry.bookmarked = bookmarked;
            }
            self.entries.push(entry);
            pushed = pushed.saturating_add(1);
        }
        self.unfinished = open.to_vec();
        // The entry read again is not a new one, unless it was skipped.
        growth.added = pushed.saturating_sub(usize::from(growth.revised && skip == 0));
        let old = self.entries.len().saturating_sub(MAX_LOG_ENTRIES);
        if old > 0 {
            self.entries.drain(..old);
        }
        self.dropped = self.dropped.saturating_add(excess);
        growth.dropped = old;
        growth
    }

    /// Note that the entries changed.
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Bookmark entry `index`, or take its bookmark away.
    fn toggle_bookmark(&mut self, index: usize) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.bookmarked = !entry.bookmarked;
            self.touch();
        }
    }

    /// Read the file at `path` as a log.
    ///
    /// # Errors
    /// Whatever reading it does.
    fn open(path: &std::path::Path) -> std::io::Result<Self> {
        Self::open_within(path, MAX_READ_BYTES)
    }

    /// [`open`](Self::open), reading at most the last `limit` bytes.
    ///
    /// A log can be larger than anyone would read whole. Past the limit only
    /// its end is read -- the recent part, which is what a log is opened for
    /// -- starting at the first whole line after the cut.
    fn open_within(path: &std::path::Path, limit: u64) -> std::io::Result<Self> {
        let len = std::fs::metadata(path)?.len();
        let cut_at = len.saturating_sub(limit);
        // From one byte before the cut, when there is a cut: that byte says
        // whether the cut fell inside a line, which is dropped, or at the
        // start of one, which is whole and kept.
        let mut start = cut_at.saturating_sub(1);
        let mut file = std::fs::File::open(path)?;
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes)?;
        if cut_at > 0 {
            let cut = bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |i| i.saturating_add(1));
            bytes.drain(..cut);
            start = start.saturating_add(u64::try_from(cut).unwrap_or(0));
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut log = Self::new(&name, &path.display().to_string());
        log.parse_bytes(&bytes);
        log.start_offset = start;
        log.read_len = start.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        log.read_hash = fnv1a(FNV_OFFSET, &bytes);
        log.source = Some(path.to_path_buf());
        Ok(log)
    }

    /// The exact bytes of the lines numbered in `keep`, from the file, each
    /// ending in a newline: what an export writes.
    ///
    /// From the file and not from the entries, whose text is decoded for
    /// display: a log line that is not valid UTF-8 would otherwise be written
    /// out with replacement characters where its bytes were.
    fn exact_lines(&self, keep: &std::collections::BTreeSet<usize>) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let Some(path) = &self.source else {
            for entry in &self.entries {
                if keep.contains(&entry.line_number) {
                    out.extend_from_slice(entry.raw.as_bytes());
                    out.push(b'\n');
                }
            }
            return Ok(out);
        };
        let mut file = std::fs::File::open(path)?;
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(self.start_offset))?;
        // Only as far as was read: what has been written since is not on
        // screen, so it is not what "export" means.
        let span = self.read_len.saturating_sub(self.start_offset);
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(&mut file, span), &mut bytes)?;
        // The same bytes, or the file was replaced under the viewer and its
        // lines are not the ones shown -- which an export must not pretend.
        if u64::try_from(bytes.len()).ok() != Some(span)
            || fnv1a(FNV_OFFSET, &bytes) != self.read_hash
        {
            return Err(std::io::Error::other(
                "the log changed on disk since it was read -- open it again to export it",
            ));
        }
        for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
            if keep.contains(&i.saturating_add(1)) {
                out.extend_from_slice(line);
                out.push(b'\n');
            }
        }
        Ok(out)
    }

    /// Read whatever has been added to the file since it was last read, or
    /// `None` if it could not be read.
    ///
    /// A file shorter than what has been read was truncated or replaced (a
    /// log rotated under the viewer), and is read again from the start.
    fn refresh(&mut self) -> Option<Growth> {
        let path = self.source.clone()?;
        let len = std::fs::metadata(&path).ok()?.len();
        if len == self.read_len {
            return Some(Growth::default());
        }
        if len < self.read_len {
            let fresh = Self::open(&path).ok()?;
            // Opened as it was: the tab keeps its name, its place and its
            // identity, and its revision moves on.
            let name = std::mem::take(&mut self.name);
            let (id, revision) = (self.id, self.revision);
            *self = fresh;
            self.name = name;
            self.id = id;
            self.revision = revision.wrapping_add(1);
            return Some(Growth {
                added: self.entries.len(),
                reread: true,
                ..Growth::default()
            });
        }
        let mut file = std::fs::File::open(&path).ok()?;
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(self.read_len)).ok()?;
        let mut added = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut added).ok()?;
        self.read_len = self
            .read_len
            .saturating_add(u64::try_from(added.len()).unwrap_or(u64::MAX));
        self.read_hash = fnv1a(self.read_hash, &added);
        if self.entries.is_empty() {
            self.is_json = looks_like_json(&added);
        }
        Some(self.append_bytes(&added))
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

/// Whether `hay` contains `needle`, ignoring ASCII case.
///
/// The plain search made a lowercased copy of every entry's message, source
/// and fields to compare against, on every keystroke. Bytes compare the same
/// way without the copies; and a needle that is valid UTF-8 can only match a
/// haystack that is at the boundaries of its characters, so comparing bytes
/// finds exactly what comparing text would.
fn contains_ignore_ascii_case(hay: &str, needle: &str) -> bool {
    let (hay, needle) = (hay.as_bytes(), needle.as_bytes());
    needle.is_empty()
        || hay
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle))
}

#[derive(Debug, Clone, PartialEq)]
struct FilterState {
    min_level: LogLevel,
    search_query: String,
    source_filter: Option<String>,
    time_start: Option<u64>,
    time_end: Option<u64>,
    show_bookmarked_only: bool,
    /// Whether the search is a POSIX extended regular expression (through
    /// `ere`, the engine `grep -E` uses here) rather than plain text. The
    /// module doc promised "full-text search with regex support" and the
    /// search was only ever a substring.
    regex: bool,
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
            regex: false,
        }
    }
}

impl FilterState {
    /// The search as a compiled pattern, when it is a regular expression.
    ///
    /// `Err` carries why the pattern is not one; a search that does not
    /// compile matches nothing rather than quietly becoming a text search.
    fn pattern(&self) -> Option<Result<ere::Regex, &'static str>> {
        if !self.regex || self.search_query.is_empty() {
            return None;
        }
        Some(ere::Regex::new_flags(self.search_query.as_bytes(), true).map_err(|e| e.message()))
    }

    /// Whether `entry` passes, compiling the search if it is a pattern. For
    /// one entry at a time; a list is filtered with [`matches_with`], which
    /// compiles once. **Tests only.**
    ///
    /// [`matches_with`]: Self::matches_with
    #[cfg(test)]
    fn matches(&self, entry: &LogEntry) -> bool {
        let pattern = self.pattern();
        self.matches_with(entry, pattern.as_ref())
    }

    /// Whether the text of `entry` -- its message, its source or any field --
    /// contains the search.
    fn text_matches(
        &self,
        entry: &LogEntry,
        pattern: Option<&Result<ere::Regex, &'static str>>,
    ) -> bool {
        match pattern {
            Some(Ok(re)) => {
                let hit = |t: &str| re.is_match(t.as_bytes()).unwrap_or(false);
                hit(&entry.message)
                    || hit(&entry.source)
                    || entry.fields.iter().any(|(_, v)| hit(v))
            }
            Some(Err(_)) => false,
            None => {
                let needle = self.search_query.as_str();
                contains_ignore_ascii_case(&entry.message, needle)
                    || contains_ignore_ascii_case(&entry.source, needle)
                    || entry
                        .fields
                        .iter()
                        .any(|(_, v)| contains_ignore_ascii_case(v, needle))
            }
        }
    }

    fn matches_with(
        &self,
        entry: &LogEntry,
        pattern: Option<&Result<ere::Regex, &'static str>>,
    ) -> bool {
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
        if !self.search_query.is_empty() && !self.text_matches(entry, pattern) {
            return false;
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
            || self.regex
    }
}

// ============================================================================
// Application State
// ============================================================================

/// What a list of the entries the filter passes was worked out for.
#[derive(Debug, Clone, PartialEq)]
struct VisibleKey {
    log: u64,
    revision: u64,
    filter: FilterState,
}

/// The detail body of one entry, laid out at the origin: the commands, and
/// how tall they are.
///
/// A long entry is thousands of characters to wrap, and the frame, the wheel
/// and the scroll limit all need the layout -- several times a frame, before
/// this, and each time from scratch.
#[derive(Debug)]
struct DetailLayout {
    key: DetailKey,
    commands: Vec<RenderCommand>,
    height: f32,
}

/// What a [`DetailLayout`] was laid out for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DetailKey {
    log: u64,
    revision: u64,
    entry: usize,
    width: f32,
    palette: Palette,
}

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
    /// The first entry the list shows, as a position in the filtered list.
    ///
    /// Was `scroll_offset: f32`, which nothing wrote: the list showed its
    /// first screenful and the keyboard walked the selection off the bottom.
    list_scroll: usize,
    /// The wheel's remainder over the list.
    list_wheel: wheel::Accumulator,
    /// The size the window was last drawn at; the layout reads it.
    win_w: f32,
    win_h: f32,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The file picker, for opening a log and for exporting one.
    picker: FilePicker,
    /// Whether the picker is up to open a log or to write an export.
    exporting: bool,
    /// What the last open, export or tail said.
    status: String,
    /// How far the detail view's body is scrolled, and for which entry: a
    /// different entry starts at its top.
    detail_scroll: (Option<usize>, f32),
    /// Which entries the filter passes, and what that was worked out for.
    visible: std::cell::RefCell<Option<(VisibleKey, std::rc::Rc<[usize]>)>>,
    /// The detail body, laid out once for the entry and width it is for.
    detail_kept: std::cell::RefCell<Option<DetailLayout>>,
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
    /// Whether the shortcut list is up.
    show_help: bool,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

/// What the window says about what it cannot do.
///
/// Nothing in this crate is invented, which is why the fixture scanner
/// never looked at it. This is the other half of the same discipline,
/// found by `scripts/find-silent-incapacity.py`: a program that reaches
/// nothing outside its own process and never says so.
const NO_LOGS_LINES: [&str; 2] = [
    "No log is open.",
    "Open one with the Open button or Ctrl+O -- the system journal is /var/log/syslog.jsonl.",
];

/// Every key this program answers, and what it does.
///
/// Thirty-odd bindings, and until the list there was nothing on screen
/// naming one. `F1` rather than
/// `?`, because this match reads `key.key` and ignores modifiers, so a shifted
/// slash is a slash and would open the search box.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`.
const SHORTCUTS: &[(&str, &str)] = &[
    ("1 / 2 / 3", "List / statistics / detail"),
    ("T / D / I", "Show trace / debug / info and above"),
    ("W / E / F", "Show warnings / errors / fatal and above"),
    ("Up / Down", "Move through the entries"),
    ("Home / End", "First / last entry"),
    ("Space", "Bookmark this entry"),
    ("B", "Show only the bookmarked entries"),
    ("/", "Search"),
    ("N / P", "Next / previous match"),
    ("L", "Wrap long lines"),
    ("A", "Follow the tail of the log"),
    ("Ctrl+L", "Show or hide the line numbers"),
    ("Ctrl+T", "Show or hide the timestamps"),
    ("Ctrl+S", "Show or hide the source column"),
    ("PgUp / PgDn", "A screen up / down"),
    ("O", "Only this entry's source, or every source"),
    ("[ / ]", "Show from / up to this entry's time"),
    ("Ctrl+R", "Search with a regular expression"),
    ("Ctrl+O", "Open a log"),
    ("Ctrl+E", "Export the entries shown"),
    ("Ctrl+W", "Close this log"),
    ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous log"),
    ("Escape", "Clear every filter"),
    ("F1", "This list"),
];

impl App {
    /// A viewer with no log open. `main` opens the system journal; a test
    /// that wants entries uses [`App::with_sample`].
    ///
    /// This used to open on twenty invented entries under the name
    /// `/var/log/system.log` -- a kernel starting up, a USB controller timing
    /// out, a storage controller failing -- that described no machine at all,
    /// while `NO_LOGS_LINES` said at the top of the same window that no log
    /// was being read. `TD-C-LOGVIEWER-TAILS-A-STRING-COMPILED-INTO-ITSELF`.
    fn new() -> Self {
        Self {
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            files: Vec::new(),
            active_file: 0,
            filter: FilterState::default(),
            view_mode: ViewMode::List,
            selected_entry: None,
            list_scroll: 0,
            list_wheel: wheel::Accumulator::default(),
            win_w: WINDOW_WIDTH,
            win_h: WINDOW_HEIGHT,
            hover: None,
            last_hits: Vec::new(),
            picker: FilePicker::new(),
            exporting: false,
            status: String::new(),
            detail_scroll: (None, 0.0),
            visible: std::cell::RefCell::new(None),
            detail_kept: std::cell::RefCell::new(None),
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

    /// A viewer holding a sample log, for the tests of everything that acts on
    /// entries. **Tests only**: see [`App::new`].
    #[cfg(test)]
    fn with_sample() -> Self {
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
        let mut app = Self::new();
        app.files.push(file);
        app
    }

    fn active_log(&self) -> Option<&LogFile> {
        self.files.get(self.active_file)
    }

    /// The entries the filter shows, with their indices in the file.
    ///
    /// Asked for several times by every event and every frame -- the list,
    /// the status bar, each move of the selection -- and a regular expression
    /// over a hundred thousand entries is not free. So which entries pass is
    /// kept until the filter, the log in front or its entries change.
    fn filtered_entries(&self) -> Vec<(usize, &LogEntry)> {
        let Some(log) = self.active_log() else {
            return Vec::new();
        };
        let key = VisibleKey {
            log: log.id,
            revision: log.revision,
            filter: self.filter.clone(),
        };
        let kept = self
            .visible
            .borrow()
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| std::rc::Rc::clone(v));
        let indices = kept.unwrap_or_else(|| {
            let pattern = self.filter.pattern();
            let fresh: std::rc::Rc<[usize]> = log
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| self.filter.matches_with(e, pattern.as_ref()))
                .map(|(i, _)| i)
                .collect();
            *self.visible.borrow_mut() = Some((key, std::rc::Rc::clone(&fresh)));
            fresh
        });
        indices
            .iter()
            .filter_map(|&i| log.entries.get(i).map(|e| (i, e)))
            .collect()
    }

    fn toggle_bookmark(&mut self, entry_idx: usize) {
        if let Some(log) = self.files.get_mut(self.active_file) {
            log.toggle_bookmark(entry_idx);
        }
    }

    fn update_search(&mut self) {
        self.search_results.clear();
        self.current_search_result = 0;
        if self.filter.search_query.is_empty() {
            return;
        }
        // The same test the filter applies, so the hit count and the list
        // cannot disagree about what matches.
        let pattern = self.filter.pattern();
        if let Some(log) = self.files.get(self.active_file) {
            for (i, entry) in log.entries.iter().enumerate() {
                if self.search_results.len() >= MAX_SEARCH_RESULTS {
                    break;
                }
                if self.filter.text_matches(entry, pattern.as_ref()) {
                    self.search_results.push(i);
                }
            }
        }
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
        let before = self.selected_entry;
        let result = self.dispatch_event(event);
        // Following keeps the selection on the newest entry. Someone who moves
        // it anywhere else has gone to read something, and on a busy log the
        // next line would drag the selection away from it before it could be
        // read -- so that is where following stops, and the toolbar says so.
        if self.auto_scroll
            && matches!(event, Event::Key(_) | Event::Mouse(_))
            && self.selected_entry != before
            && self.selected_entry.is_some()
            && self.selected_entry != self.last_shown()
        {
            self.auto_scroll = false;
        }
        result
    }

    /// The last entry the filter shows, as an index into the file.
    fn last_shown(&self) -> Option<usize> {
        self.filtered_entries().last().map(|(i, _)| *i)
    }

    /// Where the list, the statistics or the detail go: between the filter
    /// bar and the status bar.
    fn content_rect(&self) -> Rect {
        let top = TOOLBAR_HEIGHT + FILTER_BAR_HEIGHT;
        Rect::new(0.0, top, self.win_w, self.list_height())
    }

    /// Start or stop following the end of the log. Starting goes there.
    fn toggle_follow(&mut self) -> EventResult {
        self.auto_scroll = !self.auto_scroll;
        if self.auto_scroll {
            self.select_edge(false);
        }
        EventResult::Consumed
    }

    fn dispatch_event(&mut self, event: &Event) -> EventResult {
        // The picker takes input first while it is up.
        match self.picker.handle(event, self.win_w, self.win_h) {
            Picked::Chose(path) => {
                if std::mem::take(&mut self.exporting) {
                    self.export_to(&path);
                } else {
                    self.open_log(&path);
                }
                return EventResult::Consumed;
            }
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse) => {
                // The card is modal: a press anywhere puts it away, and nothing
                // under it hears one.
                if self.show_help {
                    if matches!(mouse.kind, MouseEventKind::Press(_)) {
                        self.show_help = false;
                        return EventResult::Consumed;
                    }
                    return EventResult::Ignored;
                }
                self.handle_mouse(mouse)
            }
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.win_w = *width as f32;
                    self.win_h = *height as f32;
                }
                self.keep_selection_visible();
                EventResult::Consumed
            }
            Event::Tick { .. } => {
                if self.refresh_tails() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
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
        // The shortcut list, whatever has the keyboard.
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if key.key == Key::Escape && self.show_help {
            self.show_help = false;
            return EventResult::Consumed;
        }
        // Chords first, and apart. This match reads `key.key` and nothing
        // else, so a bare `Key::D` arm takes `Ctrl+D` as readily as `D`: a
        // chord nobody bound quietly raised the level floor. A chord means
        // something here or nothing at all, and the same thing while the
        // search box has the keyboard -- where `Ctrl+R` is exactly what
        // someone typing a pattern reaches for.
        if key.modifiers.ctrl {
            return self.handle_chord(key);
        }
        if self.search_focused {
            return self.handle_key_search(key);
        }
        match key.key {
            // Only this entry's source, or every source again.
            Key::O => self.toggle_source_filter(),
            // The time range: from, or up to, the selected entry.
            Key::LeftBracket => self.limit_time(true),
            Key::RightBracket => self.limit_time(false),
            Key::PageUp => self.step_selection(self.page_len().saturating_neg()),
            Key::PageDown => self.step_selection(self.page_len()),
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
            Key::A => self.toggle_follow(),
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

    /// A key pressed with Ctrl.
    ///
    /// `show_line_numbers`, `show_timestamps` and `show_source` were `true` at
    /// construction with no writer anywhere -- three members missing from the
    /// group the author had already labelled "Display toggles". Found by
    /// `scripts/frozen-flag-survey.py`.
    fn handle_chord(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::L => {
                self.show_line_numbers = !self.show_line_numbers;
                EventResult::Consumed
            }
            Key::T => {
                self.show_timestamps = !self.show_timestamps;
                EventResult::Consumed
            }
            Key::S => {
                self.show_source = !self.show_source;
                EventResult::Consumed
            }
            Key::O => {
                self.ask_to_open();
                EventResult::Consumed
            }
            Key::E => {
                self.ask_to_export();
                EventResult::Consumed
            }
            Key::R => {
                self.filter.regex = !self.filter.regex;
                self.update_search();
                self.reanchor_selection();
                EventResult::Consumed
            }
            Key::W => self.close_tab(self.active_file),
            Key::Tab => self.cycle_tab(!key.modifiers.shift),
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
                // As far as the list goes: a page up from near the top is
                // the top, not nothing. A single step at an end is the same
                // place, so it still reports that nothing moved.
                let moved = pos.saturating_add(delta).max(0);
                usize::try_from(moved)
                    .unwrap_or(0)
                    .min(visible.len().saturating_sub(1))
            }
        };
        let Some(&idx) = visible.get(next) else {
            return EventResult::Ignored;
        };
        if Some(idx) == self.selected_entry {
            return EventResult::Ignored;
        }
        self.selected_entry = Some(idx);
        self.keep_selection_visible();
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
        self.keep_selection_visible();
        EventResult::Consumed
    }

    /// Put the selection back on a visible entry after the filter changed.
    fn reanchor_selection(&mut self) {
        let visible: Vec<usize> = self.filtered_entries().iter().map(|(i, _)| *i).collect();
        if self.auto_scroll {
            // Following: the end of whatever is shown now.
            self.selected_entry = visible.last().copied();
        } else if !self.selected_entry.is_some_and(|i| visible.contains(&i)) {
            self.selected_entry = visible.first().copied();
        }
        self.list_scroll = self.list_scroll.min(visible.len().saturating_sub(1));
        self.keep_selection_visible();
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
        // To the hit, as well as counting it: N and P moved a number in the
        // filter bar and nothing else, so "next match" never showed one.
        if let Some(&hit) = self.search_results.get(self.current_search_result) {
            self.selected_entry = Some(hit);
            self.keep_selection_visible();
        }
        EventResult::Consumed
    }

    /// The drawn commands. **Tests only**: the window's `render` takes the
    /// frame itself, because it keeps the frame's boxes for the pointer.
    ///
    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    #[cfg(test)]
    fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame().into_tree().commands
    }

    /// Draw the window into `cmds`, recording every control where it is drawn.
    fn draw(&self, cmds: &mut Frame<Target>) {
        // Background: the window as it is, not the size it opened at.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.win_w,
            height: self.win_h,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(cmds);
        self.render_filter_bar(cmds);

        let content = self.content_rect();
        let (content_y, content_h) = (content.y, content.h);

        if self.files.is_empty() {
            // Where the entries would be, and only when there are none to
            // show: this was drawn in tiny type at the top of the window over
            // a list of invented entries that contradicted it.
            for (i, line) in NO_LOGS_LINES.iter().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "two lines; index is 0 or 1")]
                let ty = content_y + content_h / 2.0 - 20.0 + i as f32 * 22.0;
                cmds.push(RenderCommand::Text {
                    x: text::center_x(line, self.win_w / 2.0, NORMAL_TEXT, FontWeightHint::Regular),
                    y: ty,
                    text: (*line).to_string(),
                    color: if i == 0 {
                        self.palette.ink(self.palette.yellow)
                    } else {
                        self.palette.subtext0
                    },
                    font_size: NORMAL_TEXT,
                    font_weight: if i == 0 {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some((self.win_w - 32.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        } else {
            match self.view_mode {
                ViewMode::List => self.render_log_list(cmds, content_y, content_h),
                ViewMode::Stats => self.render_stats(cmds, content_y, content_h),
                ViewMode::Detail => self.render_detail(cmds, content_y, content_h),
            }
        }

        self.render_status_bar(cmds);

        // Over everything, because it is the one thing a reader asked for.
        if self.show_help {
            guitk::shortcut::render_card(
                cmds,
                &self.palette,
                (self.win_w, self.win_h),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
            cmds.hit(
                Target::HelpCard,
                Rect::new(0.0, 0.0, self.win_w, self.win_h),
            );
        }
    }

    /// A compact button, lit while the pointer is on it.
    fn button(&self, cmds: &mut Frame<Target>, rect: Rect, label: &str, lit: bool, target: Target) {
        let hot = self.hover == Some(target);
        self.palette.push_surface(
            cmds,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            4.0,
            if lit || hot {
                Surface::Selected
            } else {
                Surface::Card
            },
        );
        cmds.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - SMALL_TEXT) / 2.0,
            text: label.into(),
            font_size: SMALL_TEXT,
            color: if lit {
                self.palette.text
            } else {
                self.palette.subtext0
            },
            font_weight: FontWeightHint::Bold,
            max_width: Some((rect.w - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.hit(target, rect);
    }

    /// The toolbar: the title, a tab for each open log (with its close box),
    /// Open, and on the right the three views, Export and Follow.
    fn render_toolbar(&self, cmds: &mut Frame<Target>) {
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            self.win_w,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 13.0,
            text: "Log Viewer".into(),
            font_size: TITLE_TEXT,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });

        // The right-hand group first, so the tabs know where to stop.
        let follow_label = if self.auto_scroll {
            "Follow: on  A"
        } else {
            "Follow: off  A"
        };
        let mut right = self.win_w - PADDING;
        let follow_w = text::measure(follow_label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
        right -= follow_w;
        self.button(
            cmds,
            Rect::new(right, 8.0, follow_w, 28.0),
            follow_label,
            self.auto_scroll,
            Target::Follow,
        );
        let export_label = "Export…";
        let export_w = text::measure(export_label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
        right -= export_w + 6.0;
        self.button(
            cmds,
            Rect::new(right, 8.0, export_w, 28.0),
            export_label,
            false,
            Target::Export,
        );
        let modes = [ViewMode::List, ViewMode::Stats, ViewMode::Detail];
        let widths: Vec<f32> = modes
            .iter()
            .map(|m| text::measure(m.label(), SMALL_TEXT, FontWeightHint::Bold) + 16.0)
            .collect();
        right -= widths.iter().map(|w| w + 4.0).sum::<f32>() + 8.0;
        let mut mx = right;
        for (mode, w) in modes.into_iter().zip(widths) {
            let active = mode == self.view_mode;
            let rect = Rect::new(mx, 8.0, w, 28.0);
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: if active {
                    self.palette.blue
                } else if self.hover == Some(Target::View(mode)) {
                    self.palette.surface1
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: rect.x + 8.0,
                y: 14.0,
                text: mode.label().into(),
                font_size: SMALL_TEXT,
                color: if active {
                    self.palette.crust
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.hit(Target::View(mode), rect);
            mx += w + 4.0;
        }
        let tabs_end = right - 8.0;

        // The file tabs, then Open. A tab closes from its own box.
        let mut tab_x = 140.0;
        for (fi, file) in self.files.iter().enumerate() {
            // Measured bold whatever the tab's state: the active tab is drawn
            // bold, and sizing each tab to its own weight would reflow the
            // whole strip every time the user switched files.
            let w = text::measure(&file.name, SMALL_TEXT, FontWeightHint::Bold) + 20.0 + 18.0;
            if tab_x + w > tabs_end {
                break;
            }
            let active = fi == self.active_file;
            let tab = Rect::new(tab_x, 8.0, w, 28.0);
            self.palette.push_surface(
                cmds,
                tab.x,
                tab.y,
                tab.w,
                tab.h,
                4.0,
                if active || self.hover == Some(Target::Tab(fi)) {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            cmds.push(RenderCommand::Text {
                x: tab.x + 10.0,
                y: 14.0,
                text: file.name.clone(),
                font_size: SMALL_TEXT,
                color: if active {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w - 28.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.hit(Target::Tab(fi), tab);
            let close = Rect::new(tab.right() - 20.0, tab.y + 5.0, 16.0, 18.0);
            if self.hover == Some(Target::CloseTab(fi)) {
                cmds.push(RenderCommand::FillRect {
                    x: close.x,
                    y: close.y,
                    width: close.w,
                    height: close.h,
                    color: self.palette.surface1,
                    corner_radii: CornerRadii::all(3.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: close.x + 4.0,
                y: close.y + 2.0,
                text: "x".into(),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.hit(Target::CloseTab(fi), close);
            tab_x += w + 4.0;
        }
        let open_label = "Open…";
        let open_w = text::measure(open_label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
        if tab_x + open_w <= tabs_end {
            self.button(
                cmds,
                Rect::new(tab_x, 8.0, open_w, 28.0),
                open_label,
                false,
                Target::OpenLog,
            );
        }
    }

    /// The filter bar: the six level pills, the search box and its regular-
    /// expression switch, the bookmarked-only switch, and a chip for each of
    /// the source and time filters while one is set -- a press on a chip
    /// clears it.
    fn render_filter_bar(&self, cmds: &mut Frame<Target>) {
        let y = TOOLBAR_HEIGHT;

        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.win_w,
            FILTER_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Level filter pills: a press shows that level and everything above.
        let mut lx = PADDING;
        for level in LogLevel::all() {
            let label = level.short_label();
            let w = text::measure(label, BADGE_TEXT, FontWeightHint::Bold) + 12.0;
            let active = level.severity() >= self.filter.min_level.severity();
            let pill = Rect::new(lx, y + 6.0, w, 24.0);
            cmds.push(RenderCommand::FillRect {
                x: pill.x,
                y: pill.y,
                width: pill.w,
                height: pill.h,
                color: if active {
                    level.color(&self.palette)
                } else if self.hover == Some(Target::Level(*level)) {
                    self.palette.surface1
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 6.0,
                y: y + 11.0,
                text: label.into(),
                font_size: BADGE_TEXT,
                color: if active {
                    self.palette.crust
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.hit(Target::Level(*level), pill);
            lx += w + 4.0;
        }

        // Search box
        let search = Rect::new(lx + 12.0, y + 6.0, 250.0, 24.0);
        self.palette.push_surface(
            cmds,
            search.x,
            search.y,
            search.w,
            search.h,
            12.0,
            Surface::Card,
        );
        let broken = matches!(self.filter.pattern(), Some(Err(_)));
        if self.search_focused || broken {
            cmds.push(RenderCommand::StrokeRect {
                x: search.x,
                y: search.y,
                width: search.w,
                height: search.h,
                color: if broken {
                    self.palette.red
                } else {
                    self.palette.blue
                },
                line_width: 1.0,
                corner_radii: CornerRadii::all(12.0),
            });
        }
        let search_text = if self.filter.search_query.is_empty() && !self.search_focused {
            "Search logs...  /"
        } else {
            &self.filter.search_query
        };
        cmds.push(RenderCommand::Text {
            x: search.x + 10.0,
            y: y + 11.0,
            text: search_text.into(),
            font_size: SMALL_TEXT,
            color: if self.filter.search_query.is_empty() {
                self.palette.subtext0
            } else {
                self.palette.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(search.w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        if self.search_focused {
            let caret = (search.x
                + 10.0
                + text::measure(
                    &self.filter.search_query,
                    SMALL_TEXT,
                    FontWeightHint::Regular,
                ))
            .min(search.right() - 10.0);
            cmds.push(RenderCommand::FillRect {
                x: caret,
                y: search.y + 5.0,
                width: 1.0,
                height: search.h - 10.0,
                color: self.palette.text,
                corner_radii: CornerRadii::ZERO,
            });
        }
        cmds.hit(Target::SearchBox, search);

        // The regular-expression switch, and the bookmarked-only switch.
        let mut cx = search.right() + 6.0;
        for (label, lit, target) in [
            (".*", self.filter.regex, Target::RegexSwitch),
            (
                "Bookmarked",
                self.filter.show_bookmarked_only,
                Target::BookmarkedOnly,
            ),
        ] {
            let w = text::measure(label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
            self.button(cmds, Rect::new(cx, y + 6.0, w, 24.0), label, lit, target);
            cx += w + 6.0;
        }

        // Search result count
        if !self.search_results.is_empty() {
            let count_text = format!(
                "{}/{}",
                self.current_search_result.saturating_add(1),
                self.search_results.len()
            );
            cmds.push(RenderCommand::Text {
                x: cx + 2.0,
                y: y + 11.0,
                text: count_text,
                font_size: SMALL_TEXT,
                color: self.palette.ink(self.palette.green),
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            cx += 70.0;
        }

        // The source and time filters, each a chip that clears itself. The
        // source chip was drawn with no way to clear it but Escape, which
        // clears everything; the time range had no way to be set at all.
        let time_label = match (self.filter.time_start, self.filter.time_end) {
            (None, None) => None,
            (Some(a), None) => Some(format!("from {}  ×", clock_of(a))),
            (None, Some(b)) => Some(format!("up to {}  ×", clock_of(b))),
            (Some(a), Some(b)) => Some(format!("{} - {}  ×", clock_of(a), clock_of(b))),
        };
        for (label, target) in [
            (
                self.filter
                    .source_filter
                    .as_ref()
                    .map(|src| format!("src: {src}  ×")),
                Target::SourceChip,
            ),
            (time_label, Target::TimeChip),
        ] {
            let Some(label) = label else {
                continue;
            };
            let w = (text::measure(&label, 10.0, FontWeightHint::Bold) + 16.0).min(220.0);
            let chip = Rect::new(cx, y + 6.0, w, 24.0);
            if chip.right() > self.win_w - 48.0 {
                break;
            }
            cmds.push(RenderCommand::FillRect {
                x: chip.x,
                y: chip.y,
                width: chip.w,
                height: chip.h,
                color: if self.hover == Some(target) {
                    self.palette.sky
                } else {
                    self.palette.teal
                },
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: chip.x + 8.0,
                y: y + 11.0,
                text: label,
                font_size: 10.0,
                color: self.palette.crust,
                font_weight: FontWeightHint::Bold,
                max_width: Some(chip.w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.hit(target, chip);
            cx = chip.right() + 6.0;
        }

        // Filter active indicator
        if self.filter.is_active() {
            cmds.push(RenderCommand::FillRect {
                x: self.win_w - 40.0,
                y: y + 12.0,
                width: 12.0,
                height: 12.0,
                color: self.palette.peach,
                corner_radii: CornerRadii::all(6.0),
            });
        }
    }

    /// Where a row's message column starts.
    ///
    /// The columns before it are not a constant width: the line numbers and
    /// the timestamp are optional, the level badge is as wide as its word,
    /// and the source cell advances by its *elided* width -- a distinction
    /// this file has already had wrong once, when the cell was clipped at
    /// `SOURCE_WIDTH` and `cx` advanced by the untruncated width. So the sum
    /// lives here, and the renderer's running `cx` is checked against it by
    /// `the_message_starts_where_the_columns_end`.
    fn message_column_x(&self, entry: &LogEntry) -> f32 {
        let mut cx = PADDING + 14.0;
        if self.show_line_numbers {
            cx += 46.0;
        }
        if self.show_timestamps && entry.timestamp > 0 {
            cx += 104.0;
        }
        let level_w =
            text::measure(entry.level.short_label(), BADGE_TEXT, FontWeightHint::Bold) + 8.0;
        cx += level_w + 6.0;
        if self.show_source && !entry.source.is_empty() {
            // The one-character ellipsis the renderer uses, not three dots:
            // they are different widths, and the agreement test below caught
            // the 1.2 pixels between them the first time it ran.
            let fitted = text::elide(
                &format!("[{}]", entry.source),
                SOURCE_WIDTH,
                "\u{2026}",
                SMALL_TEXT,
                FontWeightHint::Bold,
            );
            cx += text::measure(&fitted, SMALL_TEXT, FontWeightHint::Bold) + 8.0;
        }
        cx
    }

    /// The message, as the lines it will be drawn on.
    ///
    /// One elided line normally. With `wrap_lines` on -- `L`, and the card
    /// says "Wrap long lines" -- as many lines as the width takes, which is
    /// what makes that row true. Until 2026-09-22 `wrap_lines` was declared,
    /// initialised and inverted by `L` and read by nothing at all, so the
    /// window advertised wrapping and elided every line exactly as before.
    fn message_lines(&self, message: &str, width: f32) -> Vec<String> {
        if self.wrap_lines {
            // Hard, so a message with no room to break at a space -- a path, a
            // URL, an escaped blob -- still wraps rather than running off.
            text::wrap_hard(message, width, NORMAL_TEXT, FontWeightHint::Regular)
        } else {
            vec![text::elide(
                message,
                width,
                "...",
                NORMAL_TEXT,
                FontWeightHint::Regular,
            )]
        }
    }

    fn render_log_list(&self, cmds: &mut Frame<Target>, y: f32, height: f32) {
        let entries = self.filtered_entries();
        let max_visible = (height / LINE_HEIGHT) as usize;
        let scroll = self.list_scroll;
        let list = Rect::new(0.0, y, self.win_w, height);
        cmds.hit(Target::List, list);
        // Clipped, so the last row -- which may be a wrapped entry taller than
        // what is left -- is not painted over the status bar.
        cmds.clip(list);

        // `ey` is carried rather than computed from the row index, because a
        // wrapped entry is taller than one row and the next one has to start
        // under it. With wrapping off every row is `LINE_HEIGHT` and this is
        // the same arithmetic the index form did.
        let mut ey = y;
        for (vi, (original_idx, entry)) in entries.iter().enumerate().skip(scroll).take(max_visible)
        {
            if ey >= y + height {
                break;
            }
            let selected = self.selected_entry == Some(*original_idx);
            let is_search_hit = self.search_results.contains(original_idx);

            // How tall this row is, measured before anything is drawn in it
            // because the background has to cover it. `message_column_x` is
            // the one place the columns before the message are added up, and
            // both this and the drawing below read it -- the alternative is
            // two copies of an arithmetic this file has already had wrong.
            let row_h = self.row_height(entry);

            // Row background
            let bg = if selected {
                self.palette.surface0
            } else if is_search_hit {
                Color::rgba(137, 180, 250, 15)
            } else if entry.level >= LogLevel::Error {
                entry.level.bg_color(&self.palette)
            } else if vi % 2 == 0 {
                self.palette.base
            } else {
                self.palette.mantle
            };

            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: ey,
                width: self.win_w,
                height: row_h,
                color: if self.hover == Some(Target::Entry(*original_idx)) && !selected {
                    self.palette.surface0
                } else {
                    bg
                },
                corner_radii: CornerRadii::ZERO,
            });
            cmds.hit(
                Target::Entry(*original_idx),
                Rect::new(0.0, ey, self.win_w, row_h),
            );

            let mut cx = PADDING;

            // Bookmark indicator, which is also where a press bookmarks.
            cmds.hit(
                Target::Star(*original_idx),
                Rect::new(cx - 4.0, ey, 16.0, LINE_HEIGHT),
            );
            if entry.bookmarked {
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ey + 3.0,
                    text: "*".into(),
                    font_size: NORMAL_TEXT,
                    color: self.palette.ink(self.palette.yellow),
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
                    color: self.palette.subtext0,
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
                    color: self.palette.subtext0,
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
                color: entry.level.color(&self.palette),
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 4.0,
                y: ey + 4.0,
                text: level_label.into(),
                font_size: BADGE_TEXT,
                color: self.palette.crust,
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
                    color: self.palette.ink(self.palette.sky),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(SOURCE_WIDTH),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.hit(
                    Target::Source(*original_idx),
                    Rect::new(cx, ey, drawn_w, LINE_HEIGHT),
                );
                cx += drawn_w + 8.0;
            }

            // Message. One elided line, or as many wrapped ones as it takes.
            //
            // `cx` is where the columns before it happened to end, and it is
            // not a constant: the line-number and timestamp columns are
            // optional, the level badge is as wide as its word, and the source
            // cell advances by its *elided* width -- which this file has
            // already had wrong once. So the wrap is measured here, against
            // the width this row really has, rather than against a second
            // copy of that arithmetic computed somewhere else.
            let msg_width = (self.win_w - cx - PADDING).max(0.0);
            let lines = self.message_lines(&entry.message, msg_width);
            for (li, line) in lines.iter().enumerate() {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a wrapped log line is tens of rows, not millions"
                )]
                let ly = ey + 3.0 + li as f32 * LINE_HEIGHT;
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ly,
                    text: line.clone(),
                    font_size: NORMAL_TEXT,
                    color: self.palette.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(msg_width),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            ey += row_h;
        }
        cmds.unclip();

        if entries.is_empty() {
            let empty = "No log entries match filters";
            cmds.push(RenderCommand::Text {
                x: text::center_x(
                    empty,
                    self.win_w / 2.0,
                    NORMAL_TEXT,
                    FontWeightHint::Regular,
                ),
                y: y + height / 2.0,
                text: empty.into(),
                font_size: NORMAL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_stats(&self, cmds: &mut Frame<Target>, y: f32, height: f32) {
        // Clipped, so a window too short for ten sources does not draw them
        // over the status bar.
        cmds.clip(Rect::new(0.0, y, self.win_w, height));
        if let Some(log) = self.active_log() {
            let counts = log.level_counts();
            let total = log.entries.len();
            let stats_x = self.win_w / 2.0 + 40.0;

            // Level distribution
            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0,
                y: y + 16.0,
                text: "Level Distribution".into(),
                font_size: HEADER_TEXT,
                color: self.palette.ink(self.palette.blue),
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
                // The row is a way to the list, at this level and above --
                // up to the summary column, which is not part of it.
                let row = Rect::new(
                    PADDING + 4.0,
                    sy,
                    (bar_max_w + 200.0)
                        .min(stats_x - 20.0 - (PADDING + 4.0))
                        .max(0.0),
                    26.0,
                );
                if self.hover == Some(Target::StatLevel(*level)) {
                    cmds.push(RenderCommand::FillRect {
                        x: row.x,
                        y: row.y,
                        width: row.w,
                        height: row.h,
                        color: self.palette.surface0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }
                cmds.hit(Target::StatLevel(*level), row);

                // Label
                cmds.push(RenderCommand::Text {
                    x: PADDING + 12.0,
                    y: sy + 4.0,
                    text: level.label().into(),
                    font_size: NORMAL_TEXT,
                    color: self.palette.ink(level.color(&self.palette)),
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
                    color: level.color(&self.palette),
                    corner_radii: CornerRadii::all(3.0),
                });

                // Count
                cmds.push(RenderCommand::Text {
                    x: 100.0 + bar_w + 8.0,
                    y: sy + 4.0,
                    text: format!("{count} ({:.1}%)", pct * 100.0),
                    font_size: SMALL_TEXT,
                    color: self.palette.subtext0,
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
                color: self.palette.ink(self.palette.teal),
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });

            for (si, (source, count)) in log.top_sources(10).iter().enumerate() {
                let sy = sources_y + 28.0 + (si as f32) * 24.0;
                // Only this source, in the list.
                let row = Rect::new(PADDING + 12.0, sy - 3.0, 330.0, 22.0);
                if self.hover == Some(Target::StatSource(si)) {
                    cmds.push(RenderCommand::FillRect {
                        x: row.x,
                        y: row.y,
                        width: row.w,
                        height: row.h,
                        color: self.palette.surface0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }
                cmds.hit(Target::StatSource(si), row);
                cmds.push(RenderCommand::Text {
                    x: PADDING + 20.0,
                    y: sy,
                    text: source.clone(),
                    font_size: NORMAL_TEXT,
                    color: self.palette.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: 250.0,
                    y: sy,
                    text: format!("{count}"),
                    font_size: NORMAL_TEXT,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Summary stats on right
            cmds.push(RenderCommand::Text {
                x: stats_x,
                y: y + 16.0,
                text: "Summary".into(),
                font_size: HEADER_TEXT,
                color: self.palette.ink(self.palette.peach),
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
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: stats_x + 140.0,
                    y: sy,
                    text: value.clone(),
                    font_size: NORMAL_TEXT,
                    color: self.palette.text,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
        cmds.unclip();
    }

    /// The detail view: the selected entry whole, and what can be done with
    /// it.
    ///
    /// The message and the raw line are wrapped, not cut. This is the one
    /// view whose purpose is to show an entry entirely, and it drew each on
    /// one elided line -- so the end of a long message, and all of a long
    /// JSON line, could be read nowhere in the program. The body scrolls under
    /// the wheel when an entry is longer than the window, and the buttons are
    /// pinned below it, where a long entry cannot push them out of reach.
    fn render_detail(&self, cmds: &mut Frame<Target>, y: f32, height: f32) {
        let Some(entry) = self
            .selected_entry
            .and_then(|i| self.active_log()?.entries.get(i))
        else {
            let empty = "Select a log entry to view details";
            cmds.push(RenderCommand::Text {
                x: text::center_x(
                    empty,
                    self.win_w / 2.0,
                    NORMAL_TEXT,
                    FontWeightHint::Regular,
                ),
                y: y + height / 2.0,
                text: empty.into(),
                font_size: NORMAL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        };
        let panel_w = (self.win_w - 2.0 * PADDING).max(0.0);

        // Header
        self.palette.push_surface_radii(
            cmds,
            PADDING,
            y + PADDING,
            panel_w,
            50.0,
            CornerRadii {
                top_left: 8.0,
                top_right: 8.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
            Surface::Card,
        );

        // Level badge
        let level_w = text::measure(entry.level.label(), NORMAL_TEXT, FontWeightHint::Bold) + 16.0;
        cmds.push(RenderCommand::FillRect {
            x: PADDING + 12.0,
            y: y + PADDING + 10.0,
            width: level_w,
            height: 24.0,
            color: entry.level.color(&self.palette),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: PADDING + 20.0,
            y: y + PADDING + 14.0,
            text: entry.level.label().into(),
            font_size: NORMAL_TEXT,
            color: self.palette.crust,
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
            color: self.palette.subtext1,
            font_weight: FontWeightHint::Regular,
            max_width: Some((panel_w - level_w - 40.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: y + PADDING + 38.0,
            text: format!("Line {}", entry.line_number),
            font_size: SMALL_TEXT,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });

        // The body: the message, the fields and the raw line, scrolled.
        let body = Self::detail_body_rect(y, height, self.win_w);
        self.palette.push_surface_radii(
            cmds,
            body.x,
            body.y,
            body.w,
            body.h,
            CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: 8.0,
                bottom_right: 8.0,
            },
            Surface::Card,
        );
        cmds.hit(Target::DetailBody, body);
        let offset = self.detail_offset();
        if let Some(layout) = self.detail_layout((body.w - 32.0).max(0.0)) {
            cmds.clip(body);
            cmds.translate(body.x + 16.0, body.y + 12.0 - offset);
            cmds.extend(layout.commands.iter().cloned());
            cmds.untranslate();
            cmds.unclip();
        }

        // What can be done from here. The time range, the source filter
        // and stepping between entries had keys at best and no way to be
        // found from the entry they act on.
        let mut bx = PADDING + 16.0;
        let by = body.bottom() + 8.0;
        for (label, target, lit) in [
            (
                "Show from here  [",
                Target::DetailFrom,
                self.filter.time_start == Some(entry.timestamp),
            ),
            (
                "Show up to here  ]",
                Target::DetailUntil,
                self.filter.time_end == Some(entry.timestamp),
            ),
            (
                "Only this source  O",
                Target::DetailSource,
                self.filter.source_filter.as_deref() == Some(entry.source.as_str()),
            ),
            (
                if entry.bookmarked {
                    "Bookmarked  Space"
                } else {
                    "Bookmark  Space"
                },
                Target::DetailBookmark,
                entry.bookmarked,
            ),
            ("Previous  Up", Target::DetailPrev, false),
            ("Next  Down", Target::DetailNext, false),
        ] {
            let w = text::measure(label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
            self.button(cmds, Rect::new(bx, by, w, 26.0), label, lit, target);
            bx += w + 6.0;
        }
    }

    /// Where the detail view's body goes, in a content area at `y` of
    /// `height`: under the 50-pixel header, over the row of buttons.
    fn detail_body_rect(y: f32, height: f32, width: f32) -> Rect {
        let top = y + PADDING + 54.0;
        let buttons = y + height - PADDING - 26.0;
        Rect::new(
            PADDING,
            top,
            (width - 2.0 * PADDING).max(0.0),
            (buttons - 8.0 - top).max(0.0),
        )
    }

    /// Lay out the detail body -- the message, the fields and the raw line,
    /// each wrapped to `width` -- at (`x`, `y`) into `out`, and say how tall it
    /// is. The drawing and the scrolling both read this, so they cannot
    /// disagree about where the end is.
    fn detail_body(
        &self,
        entry: &LogEntry,
        x: f32,
        y: f32,
        width: f32,
        out: &mut Vec<RenderCommand>,
    ) -> f32 {
        let heading = |out: &mut Vec<RenderCommand>, label: &str, at: f32| {
            out.push(RenderCommand::Text {
                x,
                y: at,
                text: label.into(),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
        };
        let mut cy = y;
        heading(out, "Message:", cy);
        cy += 18.0;
        cy += wrapped_block(
            &entry.message,
            (x, cy, width),
            NORMAL_TEXT,
            self.palette.text,
            out,
        );
        if !entry.fields.is_empty() {
            cy += 12.0;
            heading(out, "Fields:", cy);
            cy += 20.0;
            for (key, value) in &entry.fields {
                out.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: cy,
                    text: format!("{key}:"),
                    font_size: SMALL_TEXT,
                    color: self.palette.ink(self.palette.teal),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(150.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cy += wrapped_block(
                    value,
                    (x + 164.0, cy, (width - 164.0).max(0.0)),
                    SMALL_TEXT,
                    self.palette.text,
                    out,
                );
            }
        }
        cy += 12.0;
        heading(out, "Raw:", cy);
        cy += 18.0;
        // The box is measured from the lines drawn in it.
        let mut raw = Vec::new();
        let raw_h = wrapped_block(
            &entry.raw,
            (x + 8.0, cy + 4.0, (width - 16.0).max(0.0)),
            SMALL_TEXT,
            self.palette.subtext0,
            &mut raw,
        );
        let box_h = raw_h + 8.0;
        self.palette
            .push_surface(out, x, cy, width, box_h, 4.0, Surface::Card);
        out.extend(raw);
        cy += box_h;
        cy - y
    }

    /// The selected entry's detail body laid out at the origin for `width`,
    /// from the one kept when nothing it depends on has changed.
    fn detail_layout(&self, width: f32) -> Option<std::cell::Ref<'_, DetailLayout>> {
        let log = self.active_log()?;
        let index = self.selected_entry?;
        let entry = log.entries.get(index)?;
        let key = DetailKey {
            log: log.id,
            revision: log.revision,
            entry: index,
            width,
            palette: self.palette,
        };
        let kept = self
            .detail_kept
            .borrow()
            .as_ref()
            .is_some_and(|l| l.key == key);
        if !kept {
            let mut commands = Vec::new();
            let height = self.detail_body(entry, 0.0, 0.0, width, &mut commands);
            *self.detail_kept.borrow_mut() = Some(DetailLayout {
                key,
                commands,
                height,
            });
        }
        std::cell::Ref::filter_map(self.detail_kept.borrow(), Option::as_ref).ok()
    }

    /// How far the detail body can scroll: its content's height past the
    /// room it has.
    fn detail_scroll_max(&self) -> f32 {
        let area = self.content_rect();
        let body = Self::detail_body_rect(area.y, area.h, self.win_w);
        self.detail_layout((body.w - 32.0).max(0.0))
            .map_or(0.0, |layout| (layout.height + 24.0 - body.h).max(0.0))
    }

    /// The detail body's scroll, for the entry now selected: another entry
    /// starts at its top, and a window grown since shows what now fits.
    fn detail_offset(&self) -> f32 {
        let (entry, offset) = self.detail_scroll;
        if entry == self.selected_entry {
            offset.min(self.detail_scroll_max())
        } else {
            0.0
        }
    }

    fn render_status_bar(&self, cmds: &mut Frame<Target>) {
        let y = self.win_h - STATUS_BAR_HEIGHT;

        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.win_w,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        let entries = self.filtered_entries();
        let total = self.active_log().map_or(0, |l| l.entries.len());

        // Entry count
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 5.0,
            text: format!("{} / {} entries", entries.len(), total),
            font_size: SMALL_TEXT,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // What the pointer is over, or what the last open or export said,
        // or the file.
        // A search that is not a pattern matches nothing; the outline says
        // so, and this says why.
        let broken = match self.filter.pattern() {
            Some(Err(why)) => Some(format!("The pattern does not compile: {why}")),
            _ => None,
        };
        let middle = match (
            self.hover.and_then(Target::tip),
            broken,
            self.status.is_empty(),
        ) {
            (Some(tip), _, _) => Some(tip.to_string()),
            (None, Some(why), _) => Some(why),
            (None, None, false) => Some(self.status.clone()),
            (None, None, true) => None,
        };
        if let Some(text) = middle {
            cmds.push(RenderCommand::Text {
                x: 200.0,
                y: y + 5.0,
                text,
                font_size: SMALL_TEXT,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some((self.win_w - 340.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        } else if let Some(log) = self.active_log() {
            cmds.push(RenderCommand::Text {
                x: 200.0,
                y: y + 5.0,
                text: log.path.clone(),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        if let Some(log) = self.active_log() {
            // Format
            cmds.push(RenderCommand::Text {
                x: self.win_w - 120.0,
                y: y + 5.0,
                text: if log.is_json {
                    "JSON-lines"
                } else {
                    "Plain text"
                }
                .into(),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

/// Draw `text` wrapped to the width of `(x, y, width)`, one command per
/// line [`LINE_HEIGHT`] apart, and return the height it took: at least one
/// line, so an empty value still has its row.
///
/// Breaking inside a word where one does not fit (`text::wrap_hard`), because
/// the text here is a log's: a JSON line has no spaces in it at all, and a
/// path or a URL is one long word. `text::wrap` would leave either on a single
/// line, cut off by the box it was meant to fill.
fn wrapped_block(
    text_in: &str,
    (x, y, width): (f32, f32, f32),
    size: f32,
    color: Color,
    out: &mut Vec<RenderCommand>,
) -> f32 {
    let lines = text::wrap_hard(text_in, width, size, FontWeightHint::Regular);
    let mut ly = y;
    for line in &lines {
        out.push(RenderCommand::Text {
            x,
            y: ly,
            text: line.clone(),
            font_size: size,
            color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        ly += LINE_HEIGHT;
    }
    (ly - y).max(LINE_HEIGHT)
}

// ============================================================================
// Pointer targets
// ============================================================================

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The viewer drew file tabs, three view buttons, a follow switch, six level
/// pills, a search box and a list of entries, and handled no pointer event
/// (`known-issues.md` →
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`). An entry
/// is named by its index in the open file, not by its row: rows are a
/// filtered, scrolled view of the file, and the file's own order is the one
/// thing a filter does not renumber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Tab(usize),
    CloseTab(usize),
    OpenLog,
    Export,
    View(ViewMode),
    Follow,
    Level(LogLevel),
    SearchBox,
    RegexSwitch,
    BookmarkedOnly,
    /// The source filter's chip; a press clears it.
    SourceChip,
    /// The time range's chip; a press clears it.
    TimeChip,
    /// The list itself, which scrolls under the wheel.
    List,
    Entry(usize),
    /// An entry's bookmark mark.
    Star(usize),
    /// An entry's source: a press shows only that source.
    Source(usize),
    /// A level's row in the statistics: a press shows that level and above.
    StatLevel(LogLevel),
    /// A source's row in the statistics, by its place in the top ten.
    StatSource(usize),
    DetailFrom,
    DetailUntil,
    DetailSource,
    DetailBookmark,
    DetailPrev,
    DetailNext,
    /// The detail view's body, which scrolls under the wheel.
    DetailBody,
    HelpCard,
}

impl Target {
    /// What pressing this does, with its key, for the status bar.
    fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::OpenLog => "Open a log file (Ctrl+O)",
            Self::Export => "Write the entries shown to a file (Ctrl+E)",
            Self::Follow => "Follow the end of the log as it grows (A)",
            Self::CloseTab(_) => "Close this log (Ctrl+W)",
            Self::RegexSwitch => "Search with a regular expression (Ctrl+R)",
            Self::BookmarkedOnly => "Show only the bookmarked entries (B)",
            Self::SourceChip | Self::TimeChip => "Clear this filter",
            Self::Star(_) => "Bookmark this entry (Space)",
            Self::Source(_) | Self::DetailSource => "Show only this source (O)",
            Self::DetailFrom => "Show from this entry on ([)",
            Self::DetailUntil => "Show up to this entry (])",
            _ => return None,
        })
    }
}

/// How often the viewer looks at its files for new lines.
const TAIL_POLL: Duration = Duration::from_secs(1);

/// What an export of the log at `log` is offered as: named for the log, in
/// its own format. A plain-text log exported as `.jsonl` -- every export was
/// -- is a file that says it is something it is not.
///
/// From the path's own bytes, so a name that is not UTF-8 is offered as it
/// is rather than as its rendering.
fn export_name(log: &std::path::Path) -> std::ffi::OsString {
    let log_name = log;
    let mut name = log_name
        .file_stem()
        .map_or_else(|| "log".into(), std::ffi::OsStr::to_os_string);
    name.push("-filtered.");
    name.push(log_name.extension().unwrap_or(std::ffi::OsStr::new("log")));
    name
}

/// Whether `a` and `b` name the same file: the same path once resolved, or
/// the same spelling when either cannot be resolved (it does not exist yet).
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

impl App {
    /// Open the log at `path` in a tab of its own, and say what happened.
    fn open_log(&mut self, path: &std::path::Path) {
        // A file already open is brought forward rather than opened twice,
        // where the two tabs would follow the same file side by side.
        if let Some(open) = self
            .files
            .iter()
            .position(|f| f.source.as_deref().is_some_and(|src| same_file(src, path)))
        {
            self.active_file = open;
            self.show_tab();
            self.status = format!("{} is already open", path.display());
            return;
        }
        match LogFile::open(path) {
            Ok(log) => {
                let count = log.entries.len();
                let note = if log.start_offset > 0 {
                    format!(
                        "; the file is large, so only its last {} MiB",
                        MAX_READ_BYTES / (1024 * 1024)
                    )
                } else if log.dropped > 0 {
                    format!("; the newest {MAX_LOG_ENTRIES}")
                } else {
                    String::new()
                };
                self.status = format!("Opened {} -- {count} entries{note}", path.display());
                self.files.push(log);
                self.active_file = self.files.len().saturating_sub(1);
                self.selected_entry = None;
                self.list_scroll = 0;
                self.update_search();
                // A log is opened to see what happened last.
                if self.auto_scroll {
                    self.select_edge(false);
                }
            }
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    fn ask_to_open(&mut self) {
        self.exporting = false;
        self.picker.put_up(
            FileDialog::open().with_initial_path(
                std::path::Path::new(journalrec::MAIN_LOG_PATH)
                    .parent()
                    .unwrap_or(std::path::Path::new("/")),
            ),
            false,
        );
    }

    fn ask_to_export(&mut self) {
        let Some(log) = self.active_log() else {
            self.status = "Nothing to export: no log is open".to_string();
            return;
        };
        // From the file's own name where there is a file: `name` is the
        // tab's label, decoded to be drawn.
        let name = export_name(
            log.source
                .as_deref()
                .unwrap_or_else(|| std::path::Path::new(&log.name)),
        );
        self.exporting = true;
        self.picker.open_to_write(name);
    }

    /// Write the entries the filter shows to `path`: the exact lines of the
    /// log, in its order. "Export filtered view" was in the module doc and on
    /// no key and no button.
    fn export_to(&mut self, path: &std::path::Path) {
        let keep: std::collections::BTreeSet<usize> = self
            .filtered_entries()
            .iter()
            .map(|(_, e)| e.line_number)
            .collect();
        let Some(log) = self.active_log() else {
            return;
        };
        // Written atomically, so over the log itself it would replace the
        // whole file with the part of it the filter shows.
        if log
            .source
            .as_deref()
            .is_some_and(|src| same_file(src, path))
        {
            self.status = format!(
                "{} is the log being exported -- choose another name",
                path.display()
            );
            return;
        }
        let written = log
            .exact_lines(&keep)
            .and_then(|bytes| safeio::write_atomically(path, &bytes));
        self.status = match written {
            Ok(()) => format!("Wrote {} entries to {}", keep.len(), path.display()),
            Err(e) => format!("Could not write {}: {e}", path.display()),
        };
    }

    /// Read whatever the open files have grown by. Returns whether anything
    /// arrived.
    ///
    /// "Real-time log tailing with auto-scroll" was the module doc's second
    /// line, and `auto_scroll` a switch drawn in the toolbar, with no file
    /// behind either (`TD-C-LOGVIEWER-TAILS-A-STRING-COMPILED-INTO-ITSELF`).
    fn refresh_tails(&mut self) -> bool {
        // The entry at the top of the list, so the rows under the reader stay
        // put when the oldest entries are let go from above them.
        let top = self
            .filtered_entries()
            .get(self.list_scroll)
            .map(|(i, _)| *i);
        let mut arrived = false;
        let mut top_moved = None;
        for (fi, log) in self.files.iter_mut().enumerate() {
            let Some(growth) = log.refresh() else {
                continue;
            };
            if !growth.changed() {
                continue;
            }
            arrived = true;
            if fi == self.active_file {
                self.selected_entry = if growth.reread {
                    None
                } else {
                    self.selected_entry
                        .and_then(|i| i.checked_sub(growth.dropped))
                };
                top_moved = Some(if growth.reread {
                    None
                } else {
                    top.map(|i| i.saturating_sub(growth.dropped))
                });
            }
        }
        if let Some(top) = top_moved {
            self.list_scroll = top.map_or(0, |top| {
                self.filtered_entries()
                    .iter()
                    .position(|(i, _)| *i >= top)
                    .unwrap_or(0)
            });
        }
        if arrived {
            self.update_search();
            if self.auto_scroll {
                self.select_edge(false);
            } else {
                self.reanchor_selection();
            }
        }
        arrived
    }

    /// Close tab `index`.
    fn close_tab(&mut self, index: usize) -> EventResult {
        if index >= self.files.len() {
            return EventResult::Ignored;
        }
        self.files.remove(index);
        if self.active_file >= self.files.len() {
            self.active_file = self.files.len().saturating_sub(1);
        }
        self.selected_entry = None;
        self.list_scroll = 0;
        self.update_search();
        self.reanchor_selection();
        EventResult::Consumed
    }

    /// Bring the next or the previous tab forward, wrapping.
    fn cycle_tab(&mut self, forward: bool) -> EventResult {
        let count = self.files.len();
        if count < 2 {
            return EventResult::Ignored;
        }
        let last = count.saturating_sub(1);
        self.active_file = if forward {
            if self.active_file >= last {
                0
            } else {
                self.active_file.saturating_add(1)
            }
        } else {
            self.active_file.checked_sub(1).unwrap_or(last)
        };
        self.show_tab();
        EventResult::Consumed
    }

    /// Settle the view on the tab now in front.
    fn show_tab(&mut self) {
        self.selected_entry = None;
        self.list_scroll = 0;
        self.update_search();
        self.reanchor_selection();
    }

    /// Show only the selected entry's source, or every source again.
    fn toggle_source_filter(&mut self) -> EventResult {
        if self.filter.source_filter.take().is_some() {
            self.reanchor_selection();
            return EventResult::Consumed;
        }
        let source = self
            .selected_entry
            .and_then(|i| self.active_log()?.entries.get(i))
            .map(|e| e.source.clone())
            .filter(|s| !s.is_empty());
        let Some(source) = source else {
            return EventResult::Ignored;
        };
        self.filter.source_filter = Some(source);
        self.reanchor_selection();
        EventResult::Consumed
    }

    /// Show only entries from (`from`) or up to the selected entry's time.
    fn limit_time(&mut self, from: bool) -> EventResult {
        let Some(ts) = self
            .selected_entry
            .and_then(|i| self.active_log()?.entries.get(i))
            .map(|e| e.timestamp)
            .filter(|t| *t > 0)
        else {
            return EventResult::Ignored;
        };
        if from {
            self.filter.time_start = Some(ts);
        } else {
            self.filter.time_end = Some(ts);
        }
        self.reanchor_selection();
        EventResult::Consumed
    }

    /// How tall `entry`'s row is: one line, or as many as its message wraps
    /// to. The one statement of it, read by the drawing and by scrolling.
    fn row_height(&self, entry: &LogEntry) -> f32 {
        // One line unless wrapping is on, whatever the message: nothing to
        // measure, and this is asked of every row the list might show.
        if !self.wrap_lines {
            return LINE_HEIGHT;
        }
        let row_width = (self.win_w - self.message_column_x(entry) - PADDING).max(0.0);
        #[allow(
            clippy::cast_precision_loss,
            reason = "a wrapped log line is tens of rows, not millions"
        )]
        let lines = self.message_lines(&entry.message, row_width).len() as f32;
        lines.max(1.0) * LINE_HEIGHT
    }

    /// The height the list has to draw in.
    fn list_height(&self) -> f32 {
        (self.win_h - TOOLBAR_HEIGHT - FILTER_BAR_HEIGHT - STATUS_BAR_HEIGHT).max(0.0)
    }

    /// Roughly a screenful of entries, for `PageUp` and `PageDown`.
    fn page_len(&self) -> isize {
        let rows = (self.list_height() / LINE_HEIGHT) as usize;
        isize::try_from(rows.saturating_sub(1).max(1)).unwrap_or(1)
    }

    /// Scroll the list so the selected entry is on screen.
    fn keep_selection_visible(&mut self) {
        let entries = self.filtered_entries();
        let Some(pos) = self
            .selected_entry
            .and_then(|sel| entries.iter().position(|(i, _)| *i == sel))
        else {
            return;
        };
        if pos < self.list_scroll {
            self.list_scroll = pos;
            return;
        }
        // The earliest first row from which everything through the selected
        // one fits -- the rows are not all one height -- found by walking back
        // from the selection, so only rows that could be on screen are
        // measured. This measured every entry in the log on every keystroke.
        let room = self.list_height();
        let mut used = 0.0;
        let mut first = pos;
        for (i, (_, entry)) in entries
            .iter()
            .enumerate()
            .take(pos.saturating_add(1))
            .skip(self.list_scroll)
            .rev()
        {
            let h = self.row_height(entry);
            if i < pos && used + h > room {
                break;
            }
            used += h;
            first = i;
        }
        self.list_scroll = first;
    }

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    /// Route a pointer event.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let target = self.frame().hit_test(event.x, event.y);
                // A press anywhere but the search box takes the keyboard
                // from it, including a press on nothing.
                let unfocused =
                    target != Some(Target::SearchBox) && std::mem::take(&mut self.search_focused);
                let result = target.map_or(EventResult::Ignored, |t| self.activate(t));
                if unfocused {
                    EventResult::Consumed
                } else {
                    result
                }
            }
            // A double press on an entry opens its detail. (Nothing produces
            // this event until `oswindow` synthesises it; see known-issues.md.)
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                match self.frame().hit_test(event.x, event.y) {
                    Some(Target::Entry(i)) => {
                        self.selected_entry = Some(i);
                        self.set_view(ViewMode::Detail);
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => {
                let over = self.target_at(event.x, event.y);
                if over == Some(Target::DetailBody) && self.view_mode == ViewMode::Detail {
                    let before = self.detail_offset();
                    let after = (before + wheel::pixels(dy, LINE_HEIGHT))
                        .clamp(0.0, self.detail_scroll_max());
                    self.detail_scroll = (self.selected_entry, after);
                    return if (after - before).abs() < f32::EPSILON {
                        EventResult::Ignored
                    } else {
                        EventResult::Consumed
                    };
                }
                if !matches!(
                    over,
                    Some(Target::List | Target::Entry(_) | Target::Star(_) | Target::Source(_))
                ) || self.view_mode != ViewMode::List
                {
                    return EventResult::Ignored;
                }
                let rows = self.list_wheel.rows(dy);
                let last = self.filtered_entries().len().saturating_sub(1);
                let before = self.list_scroll;
                self.list_scroll = self.list_scroll.saturating_add_signed(rows).min(last);
                if self.list_scroll == before {
                    EventResult::Ignored
                } else {
                    // Scrolled away from the end by hand: stop following it,
                    // or the next line to arrive would drag the view back.
                    if rows < 0 {
                        self.auto_scroll = false;
                    }
                    EventResult::Consumed
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Do what pressing `target` means.
    fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Tab(i) => {
                if i == self.active_file || i >= self.files.len() {
                    return EventResult::Ignored;
                }
                self.active_file = i;
                self.show_tab();
                EventResult::Consumed
            }
            Target::CloseTab(i) => self.close_tab(i),
            Target::OpenLog => {
                self.ask_to_open();
                EventResult::Consumed
            }
            Target::Export => {
                self.ask_to_export();
                EventResult::Consumed
            }
            Target::View(mode) => self.set_view(mode),
            Target::Follow => self.toggle_follow(),
            Target::Level(level) => {
                self.set_min_level(level);
                EventResult::Consumed
            }
            // A level's row in the statistics is a way to the list, at that
            // level and above -- as a source's row is.
            Target::StatLevel(level) => {
                self.set_min_level(level);
                self.view_mode = ViewMode::List;
                EventResult::Consumed
            }
            Target::SearchBox => {
                self.search_focused = true;
                EventResult::Consumed
            }
            Target::RegexSwitch => {
                self.filter.regex = !self.filter.regex;
                self.update_search();
                self.reanchor_selection();
                EventResult::Consumed
            }
            Target::BookmarkedOnly => {
                self.filter.show_bookmarked_only = !self.filter.show_bookmarked_only;
                self.reanchor_selection();
                EventResult::Consumed
            }
            Target::SourceChip => {
                self.filter.source_filter = None;
                self.reanchor_selection();
                EventResult::Consumed
            }
            Target::TimeChip => {
                self.filter.time_start = None;
                self.filter.time_end = None;
                self.reanchor_selection();
                EventResult::Consumed
            }
            Target::Entry(i) => {
                self.selected_entry = Some(i);
                EventResult::Consumed
            }
            Target::Star(i) => {
                self.toggle_bookmark(i);
                self.reanchor_selection();
                EventResult::Consumed
            }
            Target::Source(i) => {
                self.selected_entry = Some(i);
                self.filter.source_filter = None;
                self.toggle_source_filter()
            }
            Target::StatSource(rank) => {
                let Some((source, _)) = self
                    .active_log()
                    .and_then(|log| log.top_sources(10).into_iter().nth(rank))
                else {
                    return EventResult::Ignored;
                };
                self.filter.source_filter = Some(source);
                self.view_mode = ViewMode::List;
                self.reanchor_selection();
                EventResult::Consumed
            }
            Target::DetailFrom => self.limit_time(true),
            Target::DetailUntil => self.limit_time(false),
            // Drawn lit while it is the filter, so a second press takes it
            // off again; while another source is the filter, a press moves
            // it to this one.
            Target::DetailSource => {
                let own = self
                    .selected_entry
                    .and_then(|i| self.active_log()?.entries.get(i))
                    .map(|e| e.source.clone());
                if own.is_some() && self.filter.source_filter == own {
                    self.filter.source_filter = None;
                    self.reanchor_selection();
                    return EventResult::Consumed;
                }
                self.filter.source_filter = None;
                self.toggle_source_filter()
            }
            Target::DetailBookmark => self.toggle_selected_bookmark(),
            Target::DetailPrev => self.step_selection(-1),
            Target::DetailNext => self.step_selection(1),
            Target::HelpCard => {
                self.show_help = false;
                EventResult::Consumed
            }
            // Room around the rows and the body of an entry: nothing to do
            // but scroll, which the wheel does.
            Target::List | Target::DetailBody => EventResult::Ignored,
        }
    }

    /// Draw the window, recording every control where it is drawn.
    fn frame(&self) -> Frame<Target> {
        let mut f = Frame::new(self.win_w, self.win_h);
        self.draw(&mut f);
        f
    }
}

// ============================================================================
// Main
// ============================================================================

// The trait and this app's own state type are both called `App`, so the impl
// names the trait in full rather than importing it under an alias.
impl oswindow::app::App for App {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

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

    /// A clock while a log with a file behind it is open: each tick reads what
    /// the file has grown by. Following decides only whether the selection
    /// goes to what arrives; the lines arrive either way, so scrolling back to
    /// read something does not stop the log being read.
    fn tick_interval(&self) -> Option<Duration> {
        self.files
            .iter()
            .any(|f| f.source.is_some())
            .then_some(TAIL_POLL)
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

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size is believed: this app laid itself out from the constants
        // it opens at, whatever the window had been resized to.
        self.win_w = width;
        self.win_h = height;
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        let mut tree = frame.into_tree();
        // Last, so the picker draws over the list it is choosing for.
        tree.commands
            .extend(self.picker.render(&self.palette, width, height));
        tree
    }
}

fn main() -> ExitCode {
    let mut viewer = App::new();
    // The system journal, where the programs that log write
    // (`journalrec::MAIN_LOG_PATH`). If it is not there the window says so and
    // offers to open another.
    viewer.open_log(std::path::Path::new(journalrec::MAIN_LOG_PATH));
    if viewer.files.is_empty() {
        viewer.status.push_str(" -- Ctrl+O opens another log");
    }
    app::launch("logviewer", &mut viewer)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range or unwraps a `None` should fail loudly
    // and point at the line that did it — that is the diagnosis. The defensive
    // lints exist to keep panics out of code that runs on a user's data,
    // which this is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use std::fmt::Write as _;

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

    /// A key with Ctrl held.
    fn ctrl(k: Key) -> Event {
        let mut modifiers = Modifiers::NONE;
        modifiers.ctrl = true;
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    /// **Every key the shortcut list advertises is one this program answers.**
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

    /// Viewers chosen so that between them every advertised key has work.
    fn help_states() -> Vec<App> {
        let plain = App::with_sample();

        // A filter set, so `Escape` has something to clear; and the selection
        // moved, so the keys that go back have somewhere to go.
        let mut filtered = App::with_sample();
        filtered.handle_event(&press(Key::W));
        filtered.handle_event(&press(Key::Down));

        // A search with matches, the one state `N` and `P` can step through.
        let mut searching = App::with_sample();
        searching.handle_event(&press(Key::Slash));
        for c in "e".chars() {
            searching.handle_event(&typed(c));
        }
        searching.handle_event(&press(Key::Escape));

        // In another view, so `1` has one to return from: setting the view
        // already in force changes nothing, and nothing changing is how this
        // app reports `Ignored`.
        let mut elsewhere = App::with_sample();
        elsewhere.handle_event(&press(Key::Num2));

        // Two logs, so `Ctrl+Tab` has another to go to.
        let two = two_logs();

        vec![plain, filtered, searching, elsewhere, two]
    }

    /// **The three display toggles change the display.**
    ///
    /// They were `true` at construction with no writer anywhere -- three
    /// members missing from the group the source already called "Display
    /// toggles". The chords are guarded and sit above every bare letter,
    /// because this match reads `key.key` alone: an unguarded `Key::T` takes
    /// `Ctrl+T` as readily as `T`. So this asserts the *effect*, which is the
    /// only thing that can tell the two apart -- both are `Consumed`.
    #[test]
    fn the_display_toggles_move_and_do_not_change_the_level() {
        let mut app = App::with_sample();
        let level = app.filter.min_level;
        let before = (app.show_line_numbers, app.show_timestamps, app.show_source);

        app.handle_event(&ctrl(Key::L));
        app.handle_event(&ctrl(Key::T));
        app.handle_event(&ctrl(Key::S));

        assert_ne!(app.show_line_numbers, before.0, "Ctrl+L did nothing");
        assert_ne!(app.show_timestamps, before.1, "Ctrl+T did nothing");
        assert_ne!(app.show_source, before.2, "Ctrl+S did nothing");
        assert_eq!(
            app.filter.min_level, level,
            "a display chord fell through to the level filter"
        );
    }

    /// **The shortcut list reaches the window.**
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = App::with_sample();
        assert!(
            !drawn_help_text(&app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_event(&press(Key::F1));
        let shown = drawn_help_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(*keys), "{keys:?} never reached the window");
            assert!(shown.contains(*what), "{what:?} never reached the window");
        }

        app.handle_event(&press(Key::Escape));
        assert!(
            !drawn_help_text(&app).contains("F1 closes this"),
            "Escape did not close it"
        );
    }

    /// Every string the window is drawing, joined.
    fn drawn_help_text(app: &App) -> String {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
        assert_eq!(app.view_mode, ViewMode::List);
        assert_eq!(app.handle_event(&press(Key::Num1)), EventResult::Ignored);
    }

    #[test]
    fn the_severity_keys_raise_the_floor_and_hide_the_quieter_lines() {
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = App::with_sample();
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
        let mut app = App::with_sample();
        let (wrap, auto) = (app.wrap_lines, app.auto_scroll);
        app.handle_event(&press(Key::L));
        assert_ne!(app.wrap_lines, wrap);
        assert_eq!(app.auto_scroll, auto, "L should not touch auto-scroll");
        app.handle_event(&press(Key::A));
        assert_ne!(app.auto_scroll, auto);
    }

    /// Every piece of text the window draws.
    fn texts(app: &App) -> Vec<String> {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// With no log open the window says so, and how to open one, where the
    /// entries would be -- and says nothing of the kind over entries.
    ///
    /// This began as `find-silent-incapacity.py`'s finding that the program
    /// read nothing outside itself and never said so. It reads files now; what
    /// is left to say is the empty state, and the journal it names is the one
    /// `main` opens.
    #[test]
    fn with_no_log_open_the_window_says_how_to_open_one() {
        let empty = texts(&App::new());
        for line in NO_LOGS_LINES {
            assert!(
                empty.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        assert!(
            NO_LOGS_LINES
                .iter()
                .any(|l| l.contains(journalrec::MAIN_LOG_PATH)),
            "the window should name the journal main opens"
        );
        let open = texts(&App::with_sample());
        for line in NO_LOGS_LINES {
            assert!(
                !open.iter().any(|t| t == line),
                "{line:?} was drawn over an open log"
            );
        }
    }

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
        let line =
            r#"{"level":"INFO","message":"hello","source":"test","timestamp":1716000000123}"#;
        let entry = parse_json_line(line, 1).unwrap();
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "hello");
        assert_eq!(entry.source, "test");
        assert_eq!(entry.timestamp, 1_716_000_000_123);
    }

    /// The system journal stamps its records in whole seconds
    /// (`journalrec::Record::ts`); the list keeps milliseconds. A time in
    /// seconds read as milliseconds is a date in January 1970.
    #[test]
    fn a_journal_record_is_read_with_its_time_in_seconds() {
        let record = journalrec::Record {
            ts: 1_716_000_000,
            level: "warning".into(),
            service: "net.dhcp".into(),
            msg: "lease renewed".into(),
            pid: Some(42),
        };
        let entry = parse_json_line(&record.to_json_line(), 1).unwrap();
        assert_eq!(entry.timestamp, 1_716_000_000_000);
        assert_eq!(entry.level, LogLevel::Warn);
        assert_eq!(entry.source, "net.dhcp");
        assert_eq!(entry.message, "lease renewed");
        assert_eq!(entry.fields, vec![("pid".to_string(), "42".to_string())]);
        // And one already in milliseconds is left as it is.
        assert_eq!(as_millis(1_716_000_000_123), 1_716_000_000_123);
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
        let app = App::with_sample();
        assert!(!app.files.is_empty());
        assert!(app.active_log().is_some());
    }

    #[test]
    fn test_app_filtered_entries() {
        let app = App::with_sample();
        let entries = app.filtered_entries();
        assert!(!entries.is_empty());
    }

    #[test]
    fn test_app_filtered_with_level() {
        let mut app = App::with_sample();
        app.filter.min_level = LogLevel::Error;
        let entries = app.filtered_entries();
        assert!(entries.iter().all(|(_, e)| e.level >= LogLevel::Error));
    }

    #[test]
    fn test_app_toggle_bookmark() {
        let mut app = App::with_sample();
        assert!(!app.files[0].entries[0].bookmarked);
        app.toggle_bookmark(0);
        assert!(app.files[0].entries[0].bookmarked);
        app.toggle_bookmark(0);
        assert!(!app.files[0].entries[0].bookmarked);
    }

    #[test]
    fn test_app_search() {
        let mut app = App::with_sample();
        app.filter.search_query = "error".into();
        app.update_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn test_app_search_navigation() {
        let mut app = App::with_sample();
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
        let app = App::with_sample();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_stats_view() {
        let mut app = App::with_sample();
        app.view_mode = ViewMode::Stats;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_detail_view() {
        let mut app = App::with_sample();
        app.view_mode = ViewMode::Detail;
        app.selected_entry = Some(0);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_detail_no_selection() {
        let mut app = App::with_sample();
        app.view_mode = ViewMode::Detail;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_view_mode_label() {
        assert_eq!(ViewMode::List.label(), "Log View");
        assert_eq!(ViewMode::Stats.label(), "Statistics");
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
    /// **L wraps a long line, which the card has always said it does.**
    ///
    /// `wrap_lines` was declared, initialised and inverted by `L`, and read by
    /// nothing -- while `("L", "Wrap long lines")` sat on this window's
    /// shortcut card. An unnamed working key is a discoverability defect; a
    /// card row for a key with no effect is a false claim, and the card made
    /// the second out of the first.
    #[test]
    fn wrapping_gives_a_long_message_more_than_one_line() {
        let long = "the quick brown fox jumps over the lazy dog and keeps going                     well past the width of any sensible log window so that there                     is certainly something to wrap here";
        let mut app = app_with_source("svc");
        if let Some(file) = app.files.first_mut()
            && let Some(entry) = file.entries.first_mut()
        {
            entry.message = long.to_string();
        }

        let width = (WINDOW_WIDTH
            - app.message_column_x(
                app.files
                    .first()
                    .and_then(|f| f.entries.first())
                    .expect("one entry"),
            )
            - PADDING)
            .max(0.0);

        assert_eq!(
            app.message_lines(long, width).len(),
            1,
            "control: with wrapping off the message is one elided line"
        );

        app.wrap_lines = true;
        let lines = app.message_lines(long, width);
        assert!(
            lines.len() > 1,
            "L is on the card as \"Wrap long lines\" and the message is still one line"
        );
        assert!(
            lines.iter().all(|l| !l.contains("...")),
            "a wrapped line was elided as well: {lines:?}"
        );
    }

    /// **The message is drawn where the columns actually end.**
    ///
    /// `message_column_x` adds the columns up and the renderer advances a
    /// running `cx` through them; both have to reach the same number or the
    /// row's height is measured against one width and drawn at another. The
    /// source cell is why this is a test rather than an assumption -- it
    /// advances by its *elided* width, and this file has had that wrong
    /// before.
    #[test]
    fn the_message_starts_where_the_columns_end() {
        for source in ["", "svc", "a-very-long-source-name-that-will-be-elided"] {
            let app = app_with_source(source);
            let entry = app
                .files
                .first()
                .and_then(|f| f.entries.first())
                .expect("one entry");
            let expected = app.message_column_x(entry);
            let drawn = app.render_commands().iter().find_map(|c| match c {
                RenderCommand::Text { x, text, .. }
                    if text.starts_with("the message that must survive") =>
                {
                    Some(*x)
                }
                _ => None,
            });
            assert_eq!(
                drawn,
                Some(expected),
                "with source {source:?} the message is drawn somewhere else"
            );
        }
    }

    fn app_with_source(source: &str) -> App {
        let mut app = App::with_sample();
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
            file.touch();
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
        let mut frame = Frame::new(app.win_w, app.win_h);
        app.render_log_list(&mut frame, 0.0, 200.0);
        frame
            .commands()
            .iter()
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

        let mut app = App::with_sample();

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
    // Files: opening, following, exporting
    //
    // `TD-C-LOGVIEWER-TAILS-A-STRING-COMPILED-INTO-ITSELF`: the viewer opened
    // on twenty invented entries and read no file at all.
    // ------------------------------------------------------------------

    /// A directory of its own under the system's temporary directory, gone
    /// again when the test is.
    struct Scratch {
        dir: std::path::PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("slateos-logviewer-{tag}-{}", std::process::id()));
            // Left over from a run that died before its drop, or (usually)
            // not there at all; either way the create below is what counts.
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        fn write(&self, name: &str, bytes: &[u8]) -> std::path::PathBuf {
            let path = self.dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }

        fn append(&self, name: &str, bytes: &[u8]) {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(self.dir.join(name))
                .unwrap();
            file.write_all(bytes).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: it is under the temporary directory, which the
            // system clears anyway.
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn tick() -> Event {
        Event::Tick { elapsed_ms: 1000 }
    }

    /// The raw lines of the log in front.
    fn raws(app: &App) -> Vec<String> {
        app.active_log()
            .map(|l| l.entries.iter().map(|e| e.raw.clone()).collect())
            .unwrap_or_default()
    }

    /// The entries the filter shows, as indices into the file.
    fn shown(app: &App) -> Vec<usize> {
        app.filtered_entries().iter().map(|(i, _)| *i).collect()
    }

    /// A viewer with `path` open, the way `main` opens the journal.
    fn opened(path: &std::path::Path) -> App {
        let mut app = App::new();
        app.open_log(path);
        assert_eq!(app.files.len(), 1, "{}", app.status);
        app
    }

    #[test]
    fn opening_a_file_reads_it_and_goes_to_its_last_entry() {
        let dir = Scratch::new("open");
        let path = dir.write("app.log", b"one\ntwo\nthree\n");
        let app = opened(&path);
        assert_eq!(raws(&app), ["one", "two", "three"]);
        assert_eq!(
            app.selected_entry,
            Some(2),
            "a log is opened to see what happened last"
        );
        assert!(app.status.contains("3 entries"), "{}", app.status);
        assert_eq!(oswindow::app::App::title(&app), "Log Viewer — app.log");
    }

    #[test]
    fn a_file_that_cannot_be_read_says_why_and_opens_nothing() {
        let dir = Scratch::new("missing");
        let mut app = App::new();
        app.open_log(&dir.dir.join("absent.log"));
        assert!(app.files.is_empty());
        assert!(app.status.starts_with("Could not open"), "{}", app.status);
    }

    #[test]
    fn a_file_already_open_is_brought_forward_rather_than_opened_twice() {
        let dir = Scratch::new("twice");
        let a = dir.write("a.log", b"a\n");
        let b = dir.write("b.log", b"b\n");
        let mut app = opened(&a);
        app.open_log(&b);
        assert_eq!(app.active_file, 1);
        app.open_log(&a);
        assert_eq!(
            app.files.len(),
            2,
            "the same file was opened in a second tab"
        );
        assert_eq!(app.active_file, 0);
        assert!(app.status.contains("already open"), "{}", app.status);
    }

    #[test]
    fn the_tail_reads_what_is_written_and_following_goes_to_it() {
        let dir = Scratch::new("tail");
        let path = dir.write("t.log", b"one\n");
        let mut app = opened(&path);
        assert_eq!(
            app.handle_event(&tick()),
            EventResult::Ignored,
            "nothing new is nothing to draw"
        );
        dir.append("t.log", b"two\nthree\n");
        assert_eq!(app.handle_event(&tick()), EventResult::Consumed);
        assert_eq!(raws(&app), ["one", "two", "three"]);
        assert_eq!(
            app.selected_entry,
            Some(2),
            "following goes to what arrived"
        );
        assert_eq!(app.files[0].entries[2].line_number, 3);
    }

    #[test]
    fn a_reader_who_selects_an_earlier_entry_is_not_dragged_away_from_it() {
        let dir = Scratch::new("reader");
        let path = dir.write("r.log", b"one\ntwo\nthree\n");
        let mut app = opened(&path);
        assert!(app.auto_scroll);
        app.handle_event(&press(Key::Home));
        assert_eq!(app.selected_entry, Some(0));
        assert!(
            !app.auto_scroll,
            "going back to an earlier entry should stop following"
        );
        dir.append("r.log", b"four\n");
        assert_eq!(app.handle_event(&tick()), EventResult::Consumed);
        assert_eq!(
            raws(&app).len(),
            4,
            "the lines arrive whether or not they are followed"
        );
        assert_eq!(
            app.selected_entry,
            Some(0),
            "the selection was dragged away"
        );
        // A starts following again, from the end.
        app.handle_event(&press(Key::A));
        assert!(app.auto_scroll);
        assert_eq!(app.selected_entry, Some(3));
    }

    #[test]
    fn the_clock_runs_while_a_file_is_open_following_or_not() {
        use oswindow::app::App as _;
        let dir = Scratch::new("clock");
        let path = dir.write("c.log", b"one\n");
        let mut app = opened(&path);
        assert_eq!(app.tick_interval(), Some(TAIL_POLL));
        app.auto_scroll = false;
        assert_eq!(
            app.tick_interval(),
            Some(TAIL_POLL),
            "not following is not the same as not reading"
        );
        assert_eq!(
            App::with_sample().tick_interval(),
            None,
            "a log with no file behind it has nothing to read again"
        );
    }

    #[test]
    fn a_line_caught_half_written_becomes_one_entry_when_it_is_finished() {
        let dir = Scratch::new("half");
        let path = dir.write("h.log", b"first\nhal");
        let mut app = opened(&path);
        assert_eq!(
            raws(&app),
            ["first", "hal"],
            "a file that ends without a newline still shows its last line"
        );
        dir.append("h.log", b"f done\nnext\n");
        app.handle_event(&tick());
        assert_eq!(raws(&app), ["first", "half done", "next"]);
        let numbers: Vec<usize> = app.files[0].entries.iter().map(|e| e.line_number).collect();
        assert_eq!(numbers, [1, 2, 3]);
    }

    #[test]
    fn finishing_a_line_keeps_the_bookmark_it_was_given_half_written() {
        let dir = Scratch::new("halfmark");
        let path = dir.write("h.log", b"first\nhal");
        let mut app = opened(&path);
        app.toggle_bookmark(1);
        dir.append("h.log", b"f\n");
        assert_eq!(app.handle_event(&tick()), EventResult::Consumed);
        assert_eq!(raws(&app), ["first", "half"]);
        assert!(app.files[0].entries[1].bookmarked);
    }

    #[test]
    fn a_character_cut_in_half_by_a_read_is_whole_once_the_rest_arrives() {
        let dir = Scratch::new("utf8");
        let path = dir.write("u.log", b"x\ncaf\xc3");
        let mut app = opened(&path);
        dir.append("u.log", b"\xa9\n");
        app.handle_event(&tick());
        assert_eq!(raws(&app), ["x", "café"]);
    }

    #[test]
    fn a_log_rotated_under_the_viewer_is_read_again_from_its_start() {
        let dir = Scratch::new("rotate");
        let path = dir.write("r.log", b"old one\nold two\nold three\n");
        let mut app = opened(&path);
        std::fs::write(&path, b"new\n").unwrap();
        assert_eq!(app.handle_event(&tick()), EventResult::Consumed);
        assert_eq!(raws(&app), ["new"]);
        assert_eq!(app.selected_entry, Some(0));
        assert_eq!(app.files[0].name, "r.log");
    }

    #[test]
    fn a_log_too_large_to_read_whole_is_read_from_its_end_at_a_whole_line() {
        let dir = Scratch::new("large");
        // Lines start at 0, 9, 18 and 29; the file is 39 bytes.
        let path = dir.write("big.log", b"line one\nline two\nline three\nline four\n");
        // Twenty bytes from the end is inside "line three", which goes.
        let log = LogFile::open_within(&path, 20).unwrap();
        let raw: Vec<&str> = log.entries.iter().map(|e| e.raw.as_str()).collect();
        assert_eq!(raw, ["line four"]);
        assert_eq!(log.start_offset, 29);
        assert_eq!(log.entries[0].line_number, 1);
        // What an export writes is that line, counted the same way.
        let keep = std::collections::BTreeSet::from([1]);
        assert_eq!(log.exact_lines(&keep).unwrap(), b"line four\n");
        // Twenty-one is exactly the start of "line three", which is whole.
        let log = LogFile::open_within(&path, 21).unwrap();
        let raw: Vec<&str> = log.entries.iter().map(|e| e.raw.as_str()).collect();
        assert_eq!(raw, ["line three", "line four"]);
        assert_eq!(log.start_offset, 18);
    }

    #[test]
    fn the_newest_entries_are_kept_when_there_are_too_many() {
        let mut text = String::new();
        for i in 0..MAX_LOG_ENTRIES + 5 {
            text.push_str(&i.to_string());
            text.push('\n');
        }
        let mut log = LogFile::new("many.log", "/many.log");
        log.parse_content(&text);
        assert_eq!(log.entries.len(), MAX_LOG_ENTRIES);
        assert_eq!(log.entries[0].raw, "5", "the oldest are the ones let go");
        assert_eq!(log.dropped, 5);
        assert_eq!(
            log.entries.last().unwrap().line_number,
            MAX_LOG_ENTRIES + 5,
            "the lines let go still count"
        );
        // And more arriving lets the oldest go, reporting how many moved.
        let growth = log.append_bytes(b"new one\nnew two\n");
        assert_eq!(growth.dropped, 2);
        assert_eq!(growth.added, 2);
        assert_eq!(log.entries[0].raw, "7");
        assert_eq!(log.dropped, 7);
    }

    #[test]
    fn an_export_writes_the_exact_bytes_of_the_entries_shown() {
        let dir = Scratch::new("export");
        let path = dir.write(
            "e.log",
            b"[INFO] keep one\n[DEBUG] drop this\n[ERROR] keep \xff two\n",
        );
        let mut app = opened(&path);
        app.handle_event(&press(Key::I)); // info and above
        let out = dir.dir.join("out.log");
        app.export_to(&out);
        assert_eq!(
            std::fs::read(&out).unwrap(),
            b"[INFO] keep one\n[ERROR] keep \xff two\n",
            "{}",
            app.status
        );
        assert!(app.status.starts_with("Wrote 2 entries"), "{}", app.status);
    }

    #[test]
    fn an_export_will_not_write_over_the_log_it_is_exporting() {
        let dir = Scratch::new("self");
        let path = dir.write("s.log", b"[INFO] one\n[DEBUG] two\n");
        let mut app = opened(&path);
        app.handle_event(&press(Key::I));
        app.export_to(&path);
        assert!(
            app.status.contains("is the log being exported"),
            "{}",
            app.status
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"[INFO] one\n[DEBUG] two\n");
    }

    #[test]
    fn an_export_refuses_a_log_that_changed_on_disk_since_it_was_read() {
        let dir = Scratch::new("changed");
        let path = dir.write("c.log", b"alpha\nbravo\n");
        let mut app = opened(&path);
        // The same length, so nothing about its size gives it away.
        std::fs::write(&path, b"ALPHA\nBRAVO\n").unwrap();
        let out = dir.dir.join("out.log");
        app.export_to(&out);
        assert!(app.status.contains("changed on disk"), "{}", app.status);
        assert!(
            !out.exists(),
            "an export of lines other than the ones shown was written"
        );
    }

    #[test]
    fn an_export_is_offered_under_the_logs_name_in_its_own_format() {
        let name = |p: &str| export_name(std::path::Path::new(p));
        assert_eq!(name("/var/log/system.log"), "system-filtered.log");
        assert_eq!(name("syslog.jsonl"), "syslog-filtered.jsonl");
        assert_eq!(name("/var/log/messages"), "messages-filtered.log");
    }

    #[test]
    fn with_nothing_open_export_says_there_is_nothing_to_export() {
        let mut app = App::new();
        app.handle_event(&ctrl(Key::E));
        assert!(!app.picker.is_open());
        assert!(
            app.status.starts_with("Nothing to export"),
            "{}",
            app.status
        );
    }

    // ------------------------------------------------------------------
    // The pointer
    //
    // `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`: tabs,
    // view buttons, a follow switch, level pills, a search box and a list,
    // and no pointer event handled at all.
    // ------------------------------------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for App {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

        /// Drawn at the viewer's own size, which these tests leave at `SIZE`
        /// unless they resize it, and then read the frame directly.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    /// The sample and a second, short log, for the tab strip.
    fn two_logs() -> App {
        let mut app = App::with_sample();
        let mut second = LogFile::new("second.log", "/second.log");
        second.parse_content("[INFO] one\n[WARN] two\n");
        app.files.push(second);
        app
    }

    /// A log of `n` numbered lines, not being followed: one to scroll.
    fn long_log(n: usize) -> App {
        let mut app = App::new();
        let mut log = LogFile::new("long.log", "/long.log");
        let mut text = String::new();
        for i in 0..n {
            writeln!(text, "entry {i}").unwrap();
        }
        log.parse_content(&text);
        app.files.push(log);
        app.auto_scroll = false;
        app
    }

    /// States that between them draw every control there is.
    fn pointer_states() -> Vec<(&'static str, App)> {
        let stats = {
            let mut a = App::with_sample();
            a.view_mode = ViewMode::Stats;
            a
        };
        let detail = {
            let mut a = App::with_sample();
            a.auto_scroll = false;
            a.selected_entry = Some(9);
            a.view_mode = ViewMode::Detail;
            a
        };
        let filtered = {
            let mut a = App::with_sample();
            a.auto_scroll = false;
            a.filter.source_filter = Some("net".into());
            a.filter.time_start = Some(1);
            a
        };
        vec![
            ("the list", two_logs()),
            ("the statistics", stats),
            ("an entry in detail", detail),
            ("every filter set", filtered),
            ("nothing open", App::new()),
        ]
    }

    /// **Every control drawn is the one a press on it reaches** -- none is
    /// covered by another -- and between them the states draw every kind of
    /// control there is. What each one does is the business of the tests
    /// after this.
    #[test]
    fn every_control_drawn_is_the_one_a_press_on_it_reaches() {
        let mut kinds = std::collections::BTreeSet::new();
        for (what, app) in pointer_states() {
            let frame = app.frame();
            for (target, rect) in frame.hits() {
                kinds.insert(probe::variant_name(*target));
                // The two that hold other controls, and are there for the
                // wheel rather than the press.
                if matches!(target, Target::List | Target::DetailBody) {
                    continue;
                }
                let (x, y) = rect.centre();
                assert_eq!(
                    frame.hit_test(x, y),
                    Some(*target),
                    "{target:?} in {what} is covered by something else"
                );
            }
        }
        let mut help = App::with_sample();
        help.handle_event(&press(Key::F1));
        for (target, _) in help.frame().hits() {
            kinds.insert(probe::variant_name(*target));
        }
        for kind in [
            "Tab",
            "CloseTab",
            "OpenLog",
            "Export",
            "View",
            "Follow",
            "Level",
            "SearchBox",
            "RegexSwitch",
            "BookmarkedOnly",
            "SourceChip",
            "TimeChip",
            "List",
            "Entry",
            "Star",
            "Source",
            "StatLevel",
            "StatSource",
            "DetailFrom",
            "DetailUntil",
            "DetailSource",
            "DetailBookmark",
            "DetailPrev",
            "DetailNext",
            "DetailBody",
            "HelpCard",
        ] {
            assert!(kinds.contains(kind), "no state draws a {kind}: {kinds:?}");
        }
    }

    #[test]
    fn a_tab_brings_its_log_forward_and_its_box_closes_it() {
        let mut app = two_logs();
        assert_eq!(
            probe::click(&mut app, Target::Tab(1)),
            EventResult::Consumed
        );
        assert_eq!(app.active_file, 1);
        assert_eq!(
            probe::click(&mut app, Target::Tab(1)),
            EventResult::Ignored,
            "a tab already in front"
        );
        probe::click(&mut app, Target::CloseTab(0));
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.files[0].name, "second.log");
        assert_eq!(app.active_file, 0);
    }

    #[test]
    fn ctrl_tab_goes_round_the_logs_and_ctrl_w_closes_them() {
        let mut app = two_logs();
        app.handle_event(&ctrl(Key::Tab));
        assert_eq!(app.active_file, 1);
        app.handle_event(&ctrl(Key::Tab));
        assert_eq!(app.active_file, 0, "past the last it wraps");
        let mut back = Modifiers::NONE;
        back.ctrl = true;
        back.shift = true;
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: back,
            text: String::new(),
        }));
        assert_eq!(app.active_file, 1, "Ctrl+Shift+Tab goes the other way");
        app.handle_event(&ctrl(Key::W));
        app.handle_event(&ctrl(Key::W));
        assert!(app.files.is_empty());
        assert!(
            texts(&app).iter().any(|t| t == NO_LOGS_LINES[0]),
            "closing the last log should leave the window saying how to open one"
        );
        assert_eq!(app.handle_event(&ctrl(Key::W)), EventResult::Ignored);
    }

    #[test]
    fn open_and_export_put_up_the_picker_and_escape_takes_it_down() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::OpenLog);
        assert!(app.picker.is_open());
        assert!(!app.exporting);
        app.handle_event(&press(Key::Escape));
        assert!(!app.picker.is_open());
        probe::click(&mut app, Target::Export);
        assert!(app.picker.is_open());
        assert!(app.exporting);
        // The keys do the same.
        let mut keyed = App::with_sample();
        keyed.handle_event(&ctrl(Key::O));
        assert!(keyed.picker.is_open());
    }

    #[test]
    fn the_view_buttons_switch_views() {
        let mut app = App::with_sample();
        for mode in [ViewMode::Stats, ViewMode::Detail, ViewMode::List] {
            assert_eq!(
                probe::click(&mut app, Target::View(mode)),
                EventResult::Consumed
            );
            assert_eq!(app.view_mode, mode);
        }
    }

    #[test]
    fn the_follow_button_starts_following_from_the_end_and_stops_it() {
        let mut app = App::with_sample();
        app.auto_scroll = false;
        app.selected_entry = Some(0);
        probe::click(&mut app, Target::Follow);
        assert!(app.auto_scroll);
        assert_eq!(app.selected_entry, app.last_shown());
        probe::click(&mut app, Target::Follow);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn a_level_pill_sets_the_floor() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::Level(LogLevel::Error));
        assert_eq!(app.filter.min_level, LogLevel::Error);
        assert!(
            app.filtered_entries()
                .iter()
                .all(|(_, e)| e.level >= LogLevel::Error)
        );
    }

    #[test]
    fn the_search_box_takes_the_keyboard_and_a_press_elsewhere_takes_it_back() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::SearchBox);
        assert!(app.search_focused);
        for c in "usb".chars() {
            app.handle_event(&typed(c));
        }
        assert_eq!(app.filter.search_query, "usb");
        assert_eq!(shown(&app), [4]);
        // A press on nothing at all takes it back too, and is a redraw: the
        // caret goes.
        assert_eq!(probe::click_background(&mut app), EventResult::Consumed);
        assert!(!app.search_focused);
    }

    #[test]
    fn the_pattern_switch_makes_the_search_a_regular_expression() {
        let mut app = App::with_sample();
        app.filter.search_query = "fail(ed|ure)".into();
        app.update_search();
        assert!(
            shown(&app).is_empty(),
            "as text, the parentheses are literal"
        );
        probe::click(&mut app, Target::RegexSwitch);
        assert!(app.filter.regex);
        let hits: Vec<String> = app
            .filtered_entries()
            .iter()
            .map(|(_, e)| e.message.clone())
            .collect();
        assert!(
            hits.iter()
                .any(|m| m.starts_with("Failed to initialize Vulkan")),
            "the pattern should ignore case as the text search does: {hits:?}"
        );
        assert!(
            hits.iter().any(|m| m.contains("resolution failed")),
            "{hits:?}"
        );
        assert_eq!(
            app.search_results.len(),
            hits.len(),
            "the match count and the list disagree"
        );
        // Ctrl+R turns it back into text.
        app.handle_event(&ctrl(Key::R));
        assert!(!app.filter.regex);
        assert!(shown(&app).is_empty());
    }

    #[test]
    fn a_pattern_that_does_not_compile_matches_nothing_and_says_why() {
        let mut app = App::with_sample();
        app.filter.regex = true;
        app.filter.search_query = "fail(".into();
        app.update_search();
        assert!(shown(&app).is_empty());
        let red = app.palette.red;
        assert!(
            app.render_commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == red)),
            "a broken pattern should be outlined"
        );
        assert!(
            texts(&app)
                .iter()
                .any(|t| t.starts_with("The pattern does not compile: ")),
            "the window should say why the pattern matches nothing"
        );
    }

    #[test]
    fn the_star_bookmarks_its_entry_and_the_switch_shows_only_those() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::Star(3));
        assert!(app.files[0].entries[3].bookmarked);
        probe::click(&mut app, Target::BookmarkedOnly);
        assert_eq!(shown(&app), [3]);
        probe::click(&mut app, Target::Star(3));
        assert!(
            !app.files[0].entries[3].bookmarked,
            "a second press takes the bookmark away"
        );
        assert!(
            shown(&app).is_empty(),
            "the only bookmark is gone and the list still shows its entry"
        );
    }

    #[test]
    fn a_press_on_a_source_shows_only_that_source_and_its_chip_clears_it() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::Source(6));
        assert_eq!(app.filter.source_filter.as_deref(), Some("net"));
        assert_eq!(shown(&app), [6, 7, 17]);
        assert!(probe::is_visible(&app, Target::SourceChip));
        probe::click(&mut app, Target::SourceChip);
        assert_eq!(app.filter.source_filter, None);
        assert!(
            !probe::is_visible(&app, Target::SourceChip),
            "a chip for a filter that is not set"
        );
    }

    #[test]
    fn a_press_on_a_row_selects_that_entry_whatever_row_it_is_drawn_on() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::Level(LogLevel::Warn));
        // The third row now, and the thirteenth entry in the file.
        assert_eq!(shown(&app).get(2), Some(&12));
        probe::click(&mut app, Target::Entry(12));
        assert_eq!(app.selected_entry, Some(12));
    }

    #[test]
    fn a_double_press_on_an_entry_opens_it() {
        let mut app = App::with_sample();
        let (x, y) = probe::rect_of(&app, Target::Entry(4)).unwrap().centre();
        app.handle_event(&mouse(x, y, MouseEventKind::DoubleClick(MouseButton::Left)));
        assert_eq!(app.view_mode, ViewMode::Detail);
        assert_eq!(app.selected_entry, Some(4));
    }

    #[test]
    fn the_detail_buttons_set_the_time_range_and_its_chip_clears_it() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::Entry(5));
        assert!(
            !app.auto_scroll,
            "selecting an earlier entry stops following"
        );
        probe::click(&mut app, Target::View(ViewMode::Detail));
        let t5 = app.files[0].entries[5].timestamp;
        probe::click(&mut app, Target::DetailFrom);
        assert_eq!(app.filter.time_start, Some(t5));
        assert_eq!(
            app.selected_entry,
            Some(5),
            "the entry acted on is still shown"
        );
        probe::click(&mut app, Target::DetailNext);
        probe::click(&mut app, Target::DetailNext);
        assert_eq!(app.selected_entry, Some(7));
        let t7 = app.files[0].entries[7].timestamp;
        probe::click(&mut app, Target::DetailUntil);
        assert_eq!(app.filter.time_end, Some(t7));
        assert_eq!(shown(&app), [5, 6, 7]);
        // One chip for the range; a press on it clears both ends.
        probe::click(&mut app, Target::TimeChip);
        assert_eq!((app.filter.time_start, app.filter.time_end), (None, None));
    }

    #[test]
    fn the_detail_buttons_step_bookmark_and_toggle_the_source() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::Entry(6));
        probe::click(&mut app, Target::View(ViewMode::Detail));
        probe::click(&mut app, Target::DetailBookmark);
        assert!(app.files[0].entries[6].bookmarked);
        let message = |app: &App, i: usize| app.files[0].entries[i].message.clone();
        probe::click(&mut app, Target::DetailPrev);
        assert_eq!(app.selected_entry, Some(5));
        assert!(
            texts(&app).contains(&message(&app, 5)),
            "the detail still shows the entry it showed before"
        );
        probe::click(&mut app, Target::DetailNext);
        assert!(texts(&app).contains(&message(&app, 6)));
        probe::click(&mut app, Target::DetailSource);
        assert_eq!(app.filter.source_filter.as_deref(), Some("net"));
        probe::click(&mut app, Target::DetailSource);
        assert_eq!(
            app.filter.source_filter, None,
            "the button is drawn lit, so a second press takes the filter off"
        );
    }

    #[test]
    fn the_statistics_lead_to_the_list() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::View(ViewMode::Stats));
        probe::click(&mut app, Target::StatLevel(LogLevel::Warn));
        assert_eq!(app.filter.min_level, LogLevel::Warn);
        assert_eq!(app.view_mode, ViewMode::List);
        probe::click(&mut app, Target::View(ViewMode::Stats));
        let top = app.files[0].top_sources(10)[0].0.clone();
        probe::click(&mut app, Target::StatSource(0));
        assert_eq!(app.filter.source_filter.as_deref(), Some(top.as_str()));
        assert_eq!(app.view_mode, ViewMode::List);
    }

    #[test]
    fn a_level_row_in_the_statistics_stops_short_of_the_summary() {
        let mut app = App::with_sample();
        app.view_mode = ViewMode::Stats;
        app.handle_event(&Event::Resize {
            width: 800,
            height: 800,
        });
        let row = app
            .frame()
            .rect_of(|t| *t == Target::StatLevel(LogLevel::Info))
            .unwrap();
        assert!(
            row.right() < 800.0 / 2.0 + 40.0,
            "{row:?} runs under the summary column"
        );
    }

    #[test]
    fn the_wheel_scrolls_the_list_and_scrolling_back_stops_following() {
        let mut app = long_log(200);
        app.auto_scroll = true;
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::List, -1.0),
            EventResult::Consumed
        );
        let down = app.list_scroll;
        assert!(down > 0, "a notch toward the end moved nothing");
        assert!(
            app.auto_scroll,
            "scrolling toward the end is not leaving it"
        );
        assert!(probe::is_visible(&app, Target::Entry(down)));
        assert!(!probe::is_visible(&app, Target::Entry(0)));
        probe::scroll_at_point(&mut app, Target::List, 1.0);
        assert!(app.list_scroll < down);
        assert!(!app.auto_scroll, "scrolling back should stop following");
    }

    #[test]
    fn walking_past_the_bottom_of_the_screen_scrolls_the_list_to_the_selection() {
        let mut app = long_log(200);
        app.handle_event(&press(Key::Home));
        for _ in 0..60 {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(app.selected_entry, Some(60));
        assert!(
            probe::is_visible(&app, Target::Entry(60)),
            "the selection walked off the screen"
        );
        app.handle_event(&press(Key::PageDown));
        let paged = app.selected_entry.unwrap();
        assert!(paged > 60);
        assert!(probe::is_visible(&app, Target::Entry(paged)));
        app.handle_event(&press(Key::Home));
        assert!(probe::is_visible(&app, Target::Entry(0)));
        // A page up from near the top is the top, not nothing.
        app.handle_event(&press(Key::Down));
        assert_eq!(app.handle_event(&press(Key::PageUp)), EventResult::Consumed);
        assert_eq!(app.selected_entry, Some(0));
        assert_eq!(app.handle_event(&press(Key::PageUp)), EventResult::Ignored);
    }

    #[test]
    fn a_long_entry_scrolls_in_the_detail_and_its_buttons_stay_where_they_are() {
        let mut app = App::new();
        let mut log = LogFile::new("long.log", "/long.log");
        let mut long = String::new();
        for i in 0..600 {
            write!(long, "word{i} ").unwrap();
        }
        log.parse_content(&format!("short\n{long}\n"));
        app.files.push(log);
        app.auto_scroll = false;
        probe::click(&mut app, Target::Entry(1));
        probe::click(&mut app, Target::View(ViewMode::Detail));
        assert!(
            app.detail_scroll_max() > 0.0,
            "the entry should be longer than the window"
        );
        let next = probe::rect_of(&app, Target::DetailNext).unwrap();
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::DetailBody, -1.0),
            EventResult::Consumed
        );
        assert!(app.detail_offset() > 0.0);
        assert_eq!(
            probe::rect_of(&app, Target::DetailNext),
            Some(next),
            "the buttons moved with the text"
        );
        // To the end, and no further.
        for _ in 0..50 {
            probe::scroll_at_point(&mut app, Target::DetailBody, -1.0);
        }
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::DetailBody, -1.0),
            EventResult::Ignored
        );
        assert!((app.detail_offset() - app.detail_scroll_max()).abs() < 0.01);
        // Another entry starts at its top.
        probe::click(&mut app, Target::DetailPrev);
        assert_eq!(app.detail_offset(), 0.0);
    }

    #[test]
    fn the_detail_shows_all_of_a_raw_line_with_no_spaces_in_it() {
        let record = format!(
            r#"{{"ts":1716000000,"level":"info","service":"svc","msg":"m","blob":"{}"}}"#,
            "x".repeat(600)
        );
        let mut app = App::new();
        let mut log = LogFile::new("j.jsonl", "/j.jsonl");
        log.parse_content(&record);
        app.files.push(log);
        app.selected_entry = Some(0);
        app.view_mode = ViewMode::Detail;
        let area = app.content_rect();
        let body = App::detail_body_rect(area.y, area.h, app.win_w);
        let width = body.w - 32.0;
        let mut cmds = Vec::new();
        app.detail_body(&app.files[0].entries[0], 0.0, 0.0, width, &mut cmds);
        let drawn: Vec<String> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let raw_at = drawn.iter().position(|t| t == "Raw:").unwrap();
        let raw_lines = &drawn[raw_at + 1..];
        assert!(
            raw_lines.len() > 1,
            "a long line with no spaces was left on one line"
        );
        assert_eq!(
            raw_lines.concat(),
            record,
            "part of the raw line is missing"
        );
        for line in raw_lines {
            let w = text::measure(line, SMALL_TEXT, FontWeightHint::Regular);
            assert!(w <= width - 16.0 + 0.5, "{line:?} is {w} px in {width} px");
        }
    }

    #[test]
    fn the_detail_is_drawn_again_in_a_new_theme() {
        use oswindow::app::App as _;
        let theme = |mode| {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                ..appearance::AppearanceSettings::default()
            })
        };
        let mut app = App::with_sample();
        app.auto_scroll = false;
        app.selected_entry = Some(9);
        app.view_mode = ViewMode::Detail;
        let message = app.files[0].entries[9].message.clone();
        let colour = |app: &App| {
            app.render_commands().into_iter().find_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == message => Some(color),
                _ => None,
            })
        };
        app.theme_changed(&theme(appearance::ThemeMode::Dark));
        let dark = colour(&app);
        app.theme_changed(&theme(appearance::ThemeMode::Light));
        let light = colour(&app);
        assert!(dark.is_some(), "the entry's message is not drawn");
        assert_ne!(
            dark, light,
            "the entry kept the colours of the theme it was first drawn in"
        );
    }

    #[test]
    fn wrapping_breaks_a_word_too_long_for_the_row() {
        let mut app = app_with_source("svc");
        app.wrap_lines = true;
        let word = "x".repeat(400);
        let lines = app.message_lines(&word, 300.0);
        assert!(
            lines.len() > 1,
            "a message with no spaces was left on one line"
        );
        assert_eq!(lines.concat(), word);
    }

    #[test]
    fn a_filter_changed_while_following_stays_at_the_end() {
        let mut app = App::with_sample();
        app.handle_event(&press(Key::End));
        assert!(app.auto_scroll);
        assert_eq!(app.selected_entry, Some(20));
        app.handle_event(&press(Key::Slash));
        for c in "net".chars() {
            app.handle_event(&typed(c));
        }
        assert_eq!(app.selected_entry, Some(17));
        assert_eq!(app.selected_entry, app.last_shown());
        assert!(app.auto_scroll, "narrowing the list is not leaving its end");
    }

    #[test]
    fn the_rows_under_a_reader_stay_put_when_old_entries_are_let_go() {
        let dir = Scratch::new("letgo");
        let mut text = String::new();
        for i in 0..MAX_LOG_ENTRIES {
            writeln!(text, "{i}").unwrap();
        }
        let path = dir.write("full.log", text.as_bytes());
        let mut app = opened(&path);
        app.handle_event(&press(Key::Home));
        assert!(!app.auto_scroll);
        app.selected_entry = Some(60);
        app.list_scroll = 50;
        dir.append("full.log", b"new one\nnew two\n");
        assert_eq!(app.handle_event(&tick()), EventResult::Consumed);
        let rows = app.filtered_entries();
        assert_eq!(
            rows[app.list_scroll].1.raw, "50",
            "the rows moved under the reader"
        );
        assert_eq!(
            app.selected_entry
                .map(|i| app.files[0].entries[i].raw.clone()),
            Some("60".to_string())
        );
    }

    #[test]
    fn the_pointer_lights_what_it_is_over_and_the_status_bar_says_what_it_does() {
        let mut app = App::with_sample();
        let (x, y) = probe::rect_of(&app, Target::Export).unwrap().centre();
        assert_eq!(
            app.handle_event(&mouse(x, y, MouseEventKind::Move)),
            EventResult::Consumed
        );
        assert_eq!(app.hover, Some(Target::Export));
        assert!(
            texts(&app)
                .iter()
                .any(|t| t == "Write the entries shown to a file (Ctrl+E)")
        );
        assert_eq!(
            app.handle_event(&mouse(x, y, MouseEventKind::Move)),
            EventResult::Ignored,
            "moving within the same control is not a redraw"
        );
        app.handle_event(&mouse(x, y, MouseEventKind::Leave));
        assert_eq!(app.hover, None);
    }

    #[test]
    fn a_press_puts_the_shortcut_card_away_and_reaches_nothing_under_it() {
        let mut app = App::with_sample();
        let (x, y) = probe::rect_of(&app, Target::Export).unwrap().centre();
        app.handle_event(&press(Key::F1));
        assert!(app.show_help);
        assert_eq!(app.frame().hit_test(x, y), Some(Target::HelpCard));
        app.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)));
        assert!(!app.show_help);
        assert!(
            !app.picker.is_open(),
            "the press went through the card to the button under it"
        );
    }

    #[test]
    fn the_window_is_laid_out_at_the_size_it_is_given() {
        let mut app = App::with_sample();
        app.handle_event(&Event::Resize {
            width: 1600,
            height: 900,
        });
        let frame = app.frame();
        let follow = frame.rect_of(|t| *t == Target::Follow).unwrap();
        assert!(
            (follow.right() - (1600.0 - PADDING)).abs() < 0.5,
            "{follow:?} is not at the right edge"
        );
        let list = frame.rect_of(|t| *t == Target::List).unwrap();
        assert!(
            (list.bottom() - (900.0 - STATUS_BAR_HEIGHT)).abs() < 0.5,
            "{list:?}"
        );
        let background = app.render_commands().into_iter().find_map(|c| match c {
            RenderCommand::FillRect { width, height, .. } => Some((width, height)),
            _ => None,
        });
        assert_eq!(
            background,
            Some((1600.0, 900.0)),
            "the background stopped at the size the window opened at"
        );
    }

    // ------------------------------------------------------------------
    // Keys
    // ------------------------------------------------------------------

    #[test]
    fn a_chord_nobody_bound_does_not_run_the_key_under_it() {
        let mut app = App::with_sample();
        assert_eq!(app.handle_event(&ctrl(Key::D)), EventResult::Ignored);
        assert_eq!(
            app.filter.min_level,
            LogLevel::Trace,
            "Ctrl+D raised the floor to Debug"
        );
        assert_eq!(app.handle_event(&ctrl(Key::B)), EventResult::Ignored);
        assert!(!app.filter.show_bookmarked_only);
    }

    #[test]
    fn the_chords_and_f1_work_while_a_search_is_being_typed() {
        let mut app = App::with_sample();
        app.handle_event(&press(Key::Slash));
        app.handle_event(&ctrl(Key::R));
        assert!(app.filter.regex);
        assert!(
            app.search_focused,
            "a chord should not take the keyboard from the box"
        );
        app.handle_event(&press(Key::F1));
        assert!(
            app.show_help,
            "F1 reaches the list whatever has the keyboard"
        );
    }

    #[test]
    fn o_and_the_brackets_filter_by_the_selected_entry() {
        let mut app = App::with_sample();
        probe::click(&mut app, Target::Entry(7));
        app.handle_event(&press(Key::O));
        assert_eq!(app.filter.source_filter.as_deref(), Some("net"));
        app.handle_event(&press(Key::O));
        assert_eq!(app.filter.source_filter, None);
        let t7 = app.files[0].entries[7].timestamp;
        app.handle_event(&press(Key::LeftBracket));
        assert_eq!(app.filter.time_start, Some(t7));
        app.handle_event(&press(Key::RightBracket));
        assert_eq!(app.filter.time_end, Some(t7));
        assert_eq!(shown(&app), [7]);
    }

    #[test]
    fn n_and_p_go_to_the_match_as_well_as_counting_it() {
        let mut app = App::with_sample();
        app.auto_scroll = false;
        app.filter.search_query = "mm".into();
        app.update_search();
        assert!(app.search_results.len() >= 2);
        app.handle_event(&press(Key::N));
        assert_eq!(
            app.selected_entry,
            app.search_results.get(app.current_search_result).copied()
        );
        app.handle_event(&press(Key::P));
        assert_eq!(
            app.selected_entry,
            app.search_results.get(app.current_search_result).copied()
        );
    }
}
