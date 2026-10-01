//! Slate OS System Log Daemon (`syslogd`)
//!
//! A JSON-lines log aggregation service that collects structured log messages
//! from all system services, stores them in rotating log files, and provides
//! query/filter capabilities. Follows the design spec's requirement for
//! text-based (not binary) structured logging.
//!
//! # Log Format
//!
//! Each line is a JSON object:
//! ```json
//! {"ts":1716000000,"level":"info","service":"net.dhcp","msg":"lease renewed","ip":"10.0.2.15"}
//! ```
//!
//! # Commands
//!
//! ```text
//! syslogd daemon              Run as log collection daemon
//! syslogd log <svc> <msg>     Write a log entry (for scripts/services)
//! syslogd query [filters]     Search log entries
//! syslogd tail [n]            Show last N entries (default 20)
//! syslogd follow              Live-tail the log file
//! syslogd stats               Show log statistics
//! syslogd rotate              Force log rotation
//! syslogd clean [days]        Remove logs older than N days (default 30)
//! ```

use quoting::{quoteaf_os, quotef_os};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const LOG_DIR: &str = "/var/log";
const MAIN_LOG: &str = "syslog.jsonl";
const PID_PATH: &str = "/var/run/syslogd.pid";
/// Maximum log file size before rotation (5 MiB).
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;
/// Maximum number of rotated log files to keep.
const MAX_ROTATED_FILES: u32 = 10;
/// Severity levels (RFC 5424 compatible).
const LEVELS: &[&str] = &[
    "emerg", "alert", "crit", "error", "warn", "notice", "info", "debug",
];

// ============================================================================
// Time helpers
// ============================================================================

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_timestamp(unix_secs: u64) -> String {
    if unix_secs == 0 {
        return "unknown".to_string();
    }

    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let mut y = 1970i64;
    let mut remaining_days = days as i64;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let month_days: [i64; 12] = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0u32;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            month = i as u32 + 1;
            break;
        }
        remaining_days -= md;
    }
    if month == 0 {
        month = 12;
    }
    let day = remaining_days + 1;

    format!("{y:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ============================================================================
// JSON helpers (minimal, no dependency)
// ============================================================================

/// A structured log entry.
#[derive(Debug, Clone)]
struct LogEntry {
    /// Unix timestamp.
    timestamp: u64,
    /// Severity level.
    level: String,
    /// Service/source name.
    service: String,
    /// Human-readable message.
    message: String,
    /// Optional extra key-value pairs.
    extra: Vec<(String, String)>,
}

impl LogEntry {
    fn to_json(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("\"ts\":{}", self.timestamp));
        parts.push(format!(
            "\"time\":\"{}\"",
            journalrec::escape(&format_timestamp(self.timestamp))
        ));
        parts.push(format!("\"level\":\"{}\"", journalrec::escape(&self.level)));
        parts.push(format!(
            "\"service\":\"{}\"",
            journalrec::escape(&self.service)
        ));
        parts.push(format!("\"msg\":\"{}\"", journalrec::escape(&self.message)));

        for (k, v) in &self.extra {
            parts.push(format!(
                "\"{}\":\"{}\"",
                journalrec::escape(k),
                journalrec::escape(v)
            ));
        }

        format!("{{{}}}", parts.join(","))
    }

    /// Parse from a JSON-lines entry (minimal parser).
    fn from_json(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
            return None;
        }

        let inner = &trimmed[1..trimmed.len() - 1];
        let mut entry = LogEntry {
            timestamp: 0,
            level: String::new(),
            service: String::new(),
            message: String::new(),
            extra: Vec::new(),
        };

        // Simple key-value extraction (handles escaped quotes minimally).
        let mut pos = 0;
        let bytes = inner.as_bytes();

        while pos < bytes.len() {
            // Skip whitespace and commas.
            while pos < bytes.len()
                && (bytes[pos] == b' ' || bytes[pos] == b',' || bytes[pos] == b'\t')
            {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }

            // Parse key.
            let key = parse_json_string(inner, &mut pos)?;

            // Skip colon.
            while pos < bytes.len() && bytes[pos] != b':' {
                pos += 1;
            }
            pos += 1; // skip ':'

            // Skip whitespace.
            while pos < bytes.len() && bytes[pos] == b' ' {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }

            // Parse value.
            let value = if bytes[pos] == b'"' {
                parse_json_string(inner, &mut pos)?
            } else {
                // Numeric value.
                let start = pos;
                while pos < bytes.len() && bytes[pos] != b',' && bytes[pos] != b'}' {
                    pos += 1;
                }
                inner[start..pos].trim().to_string()
            };

            match key.as_str() {
                "ts" => entry.timestamp = value.parse().unwrap_or(0),
                "level" => entry.level = value,
                "service" => entry.service = value,
                "msg" => entry.message = value,
                "time" => {} // Derived from ts, skip.
                _ => entry.extra.push((key, value)),
            }
        }

        Some(entry)
    }

    fn display_short(&self) -> String {
        let time_str = format_timestamp(self.timestamp);
        let level_padded = format!("{:<6}", self.level);
        format!(
            "{} {} [{}] {}",
            time_str, level_padded, self.service, self.message
        )
    }
}

/// Parse a JSON string starting with `"` at `pos`.
fn parse_json_string(s: &str, pos: &mut usize) -> Option<String> {
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
                _ => {
                    result.push(bytes[*pos] as char);
                    *pos += 1;
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

    Some(result)
}

// ============================================================================
// Log file management
// ============================================================================

fn log_file_path() -> PathBuf {
    PathBuf::from(LOG_DIR).join(MAIN_LOG)
}

fn rotated_path(n: u32) -> PathBuf {
    PathBuf::from(LOG_DIR).join(format!("{MAIN_LOG}.{n}"))
}

fn write_log_entry(entry: &LogEntry) -> io::Result<()> {
    let _ = fs::create_dir_all(LOG_DIR);

    let path = log_file_path();
    let mut line = entry.to_json().into_bytes();
    line.push(b'\n');
    // Under the journal's lock (design-decisions §1037): a vacuum or a
    // rotation holding the file finishes first, and the entry lands in
    // whichever file the path names then.
    journalio::append(&path, &line)?;

    // Check if rotation is needed.
    let meta = fs::metadata(&path)?;
    if meta.len() > MAX_LOG_SIZE {
        rotate_logs();
    }

    Ok(())
}

/// Shift the rotated logs up one and the live log to `.1`, each step under
/// the journal's lock (design-decisions §1037): a `journalctl --vacuum-*`
/// rewriting one of these files would otherwise rename its result over
/// whatever this had just moved there, and a writer waiting on the live
/// log lands in the next one rather than in `.1`.
///
/// A step that fails is left undone -- the files keep their names and the
/// log keeps growing until the next rotation tries again -- which is why
/// each result is not looked at.
fn rotate_logs() {
    // Delete the oldest file.
    if let Ok(Some(oldest)) = journalio::Locked::open(&rotated_path(MAX_ROTATED_FILES)) {
        let _ = oldest.remove();
    }

    // Shift N-1 → N, N-2 → N-1, etc.
    for i in (1..MAX_ROTATED_FILES).rev() {
        if let Ok(Some(held)) = journalio::Locked::open(&rotated_path(i)) {
            let _ = held.rename_to(&rotated_path(i + 1));
        }
    }

    // Move current → .1
    if let Ok(Some(held)) = journalio::Locked::open(&log_file_path()) {
        let _ = held.rename_to(&rotated_path(1));
    }
}

fn read_log_entries(max_entries: usize) -> Vec<LogEntry> {
    let path = log_file_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(max_entries);

    all_lines[start..]
        .iter()
        .filter_map(|line| LogEntry::from_json(line))
        .collect()
}

fn count_all_entries() -> (usize, u64) {
    let path = log_file_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

    let count = content.lines().count();
    let size = content.len() as u64;
    (count, size)
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_daemon() {
    println!("syslogd: starting daemon");

    // Write PID file.
    if let Some(parent) = Path::new(PID_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(PID_PATH, format!("{}", process::id()));

    // Log startup.
    let startup = LogEntry {
        timestamp: now_secs(),
        level: "info".to_string(),
        service: "syslogd".to_string(),
        message: "daemon started".to_string(),
        extra: vec![("pid".to_string(), process::id().to_string())],
    };
    let _ = write_log_entry(&startup);

    // In a real implementation, we'd listen on a socket or pipe for log
    // messages from other services. For now, the daemon sits idle and
    // services use `syslogd log` to write entries directly.
    //
    // Future: listen on a Unix domain socket or IPC channel for structured
    // log ingestion from all services.
    println!("syslogd: listening for log messages (write via 'syslogd log')");

    loop {
        // Periodic maintenance: check file size, rotate if needed.
        if let Ok(meta) = fs::metadata(log_file_path())
            && meta.len() > MAX_LOG_SIZE
        {
            println!("syslogd: rotating logs (size {})", meta.len());
            rotate_logs();
        }

        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

fn cmd_log(level: &str, service: &str, message: &str, extra: &[(String, String)]) {
    // Validate level.
    let level_lower = level.to_lowercase();
    if !LEVELS.contains(&level_lower.as_str()) {
        eprintln!("warning: unknown level {}, using 'info'", quoteaf_os(level));
    }

    let entry = LogEntry {
        timestamp: now_secs(),
        level: level_lower,
        service: service.to_string(),
        message: message.to_string(),
        extra: extra.to_vec(),
    };

    if let Err(e) = write_log_entry(&entry) {
        eprintln!("error writing log: {e}");
        process::exit(1);
    }
}

fn cmd_tail(count: usize) {
    let entries = read_log_entries(count);
    if entries.is_empty() {
        println!("No log entries.");
        return;
    }

    for entry in &entries {
        println!("{}", entry.display_short());
    }
}

fn cmd_query(filters: &QueryFilters) {
    let path = log_file_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            println!("No log file found.");
            return;
        }
    };

    let mut matches = 0usize;
    for line in content.lines() {
        if let Some(entry) = LogEntry::from_json(line)
            && filters.matches(&entry)
        {
            println!("{}", entry.display_short());
            matches += 1;
            if matches >= filters.limit {
                break;
            }
        }
    }

    if matches == 0 {
        println!("No matching entries.");
    } else {
        println!("\n{matches} entries matched.");
    }
}

fn cmd_follow() {
    println!(
        "syslogd: following {} (Ctrl+C to stop)",
        log_file_path().display()
    );

    let mut last_size = fs::metadata(log_file_path()).map(|m| m.len()).unwrap_or(0);

    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        let current_size = match fs::metadata(log_file_path()) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };

        if current_size > last_size {
            // Read new content.
            if let Ok(content) = fs::read_to_string(log_file_path()) {
                let bytes = content.as_bytes();
                if (last_size as usize) < bytes.len() {
                    let new_data = &content[last_size as usize..];
                    for line in new_data.lines() {
                        if let Some(entry) = LogEntry::from_json(line) {
                            println!("{}", entry.display_short());
                        }
                    }
                }
            }
            last_size = current_size;
        } else if current_size < last_size {
            // File was rotated/truncated.
            last_size = 0;
        }
    }
}

fn cmd_stats() {
    println!("=== Syslog Statistics ===");

    let (total_entries, total_size) = count_all_entries();
    println!("  Main log:    {}", log_file_path().display());
    println!("  Entries:     {total_entries}");
    println!("  Size:        {}", format_size(total_size));

    // Count rotated files.
    let mut rotated = 0u32;
    let mut rotated_size = 0u64;
    for i in 1..=MAX_ROTATED_FILES {
        let p = rotated_path(i);
        if let Ok(meta) = fs::metadata(&p) {
            rotated += 1;
            rotated_size += meta.len();
        }
    }
    println!(
        "  Rotated:     {rotated} files ({} total)",
        format_size(rotated_size)
    );
    println!("  Max size:    {} per file", format_size(MAX_LOG_SIZE));
    println!("  Max files:   {MAX_ROTATED_FILES}");

    // Level breakdown.
    if total_entries > 0 {
        let entries = read_log_entries(total_entries);
        let mut level_counts = std::collections::BTreeMap::new();
        let mut service_counts = std::collections::BTreeMap::new();

        for entry in &entries {
            *level_counts.entry(entry.level.clone()).or_insert(0u64) += 1;
            *service_counts.entry(entry.service.clone()).or_insert(0u64) += 1;
        }

        println!("\n  By level:");
        for level in LEVELS {
            if let Some(count) = level_counts.get(*level) {
                println!("    {:<8} {count}", level);
            }
        }

        println!("\n  Top services:");
        let mut sorted: Vec<_> = service_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (svc, count) in sorted.iter().take(10) {
            println!("    {:<20} {count}", svc);
        }
    }
}

fn cmd_rotate() {
    println!("syslogd: forcing log rotation");
    rotate_logs();
    println!("  done");
}

/// What [`clean_file`] did, or that there was nothing to do.
#[derive(Debug, PartialEq, Eq)]
enum Cleaned {
    /// The log does not exist. An absence, and the only one.
    NoFile,
    /// `removed` entries dropped, `kept` retained.
    Trimmed { removed: usize, kept: usize },
}

/// Drop entries older than `cutoff` from the log at `path`, rewriting it.
///
/// A parameter rather than [`log_file_path`] so this is testable at all --
/// this daemon had no tests before 2026-09-12, and the behaviour below is not
/// something to ship on a reading.
///
/// # Errors
///
/// The log exists and could not be locked, read or written. **That is not
/// "no log file", which is what this used to print for every failure.** The
/// operator would then believe there was nothing to clean while the file went
/// on growing. Only `NotFound` is an absence.
///
/// The file is held under the journal's lock from the read to the rewrite
/// (design-decisions §1037), and the rewrite is a whole new file renamed over
/// it: an entry another program appends meanwhile waits, then lands in the
/// cleaned file, where the old in-place rewrite lost it. Lines are bytes,
/// kept exactly as they were -- a line that is not UTF-8 is a line this
/// daemon cannot parse, and kept like any other, where the whole file used
/// to be refused for it.
fn clean_file(path: &Path, cutoff: u64) -> Result<Cleaned, String> {
    let failed = |e: io::Error| format!("{}: {e}", quotef_os(path));
    let Some(mut held) = journalio::Locked::open(path).map_err(failed)? else {
        return Ok(Cleaned::NoFile);
    };
    let content = held.read_all().map_err(failed)?;

    let mut kept: Vec<&[u8]> = Vec::new();
    let mut removed = 0usize;
    // Split on newlines, a final newline ending the last line rather than
    // starting an empty one.
    let body = content.strip_suffix(b"\n").unwrap_or(&content);
    for line in body.split(|&b| b == b'\n').filter(|_| !content.is_empty()) {
        // A line this daemon cannot parse is KEPT, not dropped: it is somebody
        // else's record, and a cleaner that deletes what it does not recognise
        // is a cleaner that loses data on a format change.
        let old = std::str::from_utf8(line)
            .ok()
            .and_then(LogEntry::from_json)
            .is_some_and(|entry| entry.timestamp < cutoff);
        if old {
            removed += 1;
        } else {
            kept.push(line);
        }
    }

    if removed > 0 {
        let mut new_content = Vec::with_capacity(content.len());
        for line in &kept {
            new_content.extend_from_slice(line);
            new_content.push(b'\n');
        }
        held.replace(&new_content).map_err(failed)?;
    }
    Ok(Cleaned::Trimmed {
        removed,
        kept: kept.len(),
    })
}

fn cmd_clean(days: u64) {
    let cutoff = now_secs().saturating_sub(days * 86400);
    match clean_file(&log_file_path(), cutoff) {
        Ok(Cleaned::NoFile) => println!("No log file."),
        Ok(Cleaned::Trimmed { removed, kept }) => {
            println!("  removed {removed} entries older than {days} days");
            println!("  kept {kept} entries");
        }
        Err(e) => {
            eprintln!("syslogd: {e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Query filters
// ============================================================================

struct QueryFilters {
    level: Option<String>,
    service: Option<String>,
    message_contains: Option<String>,
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
}

impl QueryFilters {
    fn new() -> Self {
        QueryFilters {
            level: None,
            service: None,
            message_contains: None,
            since: None,
            until: None,
            limit: 100,
        }
    }

    fn matches(&self, entry: &LogEntry) -> bool {
        if let Some(ref level) = self.level
            && entry.level != *level
        {
            return false;
        }

        if let Some(ref svc) = self.service
            && !entry.service.contains(svc.as_str())
        {
            return false;
        }

        if let Some(ref substr) = self.message_contains {
            let msg_lower = entry.message.to_lowercase();
            let sub_lower = substr.to_lowercase();
            if !msg_lower.contains(&sub_lower) {
                return false;
            }
        }

        if let Some(since) = self.since
            && entry.timestamp < since
        {
            return false;
        }

        if let Some(until) = self.until
            && entry.timestamp > until
        {
            return false;
        }

        true
    }

    fn parse_args(args: &[String]) -> Self {
        let mut filters = Self::new();
        let mut i = 0;

        while i < args.len() {
            match args[i].as_str() {
                "--level" | "-l" if i + 1 < args.len() => {
                    filters.level = Some(args[i + 1].to_lowercase());
                    i += 2;
                }
                "--service" | "-s" if i + 1 < args.len() => {
                    filters.service = Some(args[i + 1].clone());
                    i += 2;
                }
                "--msg" | "-m" if i + 1 < args.len() => {
                    filters.message_contains = Some(args[i + 1].clone());
                    i += 2;
                }
                "--since" if i + 1 < args.len() => {
                    // Parse as hours ago.
                    if let Ok(hours) = args[i + 1].parse::<u64>() {
                        filters.since = Some(now_secs().saturating_sub(hours * 3600));
                    }
                    i += 2;
                }
                "--limit" | "-n" if i + 1 < args.len() => {
                    filters.limit = args[i + 1].parse().unwrap_or(100);
                    i += 2;
                }
                _ => {
                    i += 1;
                }
            }
        }

        filters
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("Slate OS System Log Daemon v0.1.0");
    println!();
    println!("JSON-lines structured log aggregation and query service.");
    println!("Logs are stored at {LOG_DIR}/{MAIN_LOG}.");
    println!();
    println!("USAGE:");
    println!("  syslogd <command> [arguments]");
    println!();
    println!("COMMANDS:");
    println!("  daemon                         Run as log collection daemon");
    println!("  log <level> <service> <msg>    Write a log entry");
    println!("  tail [n]                       Show last N entries (default: 20)");
    println!("  follow                         Live-tail the log file");
    println!("  query [filters]                Search log entries");
    println!("  stats                          Show log statistics");
    println!("  rotate                         Force log rotation");
    println!("  clean [days]                   Remove entries older than N days (default: 30)");
    println!();
    println!("QUERY FILTERS:");
    println!("  --level <level>     Filter by severity (emerg..debug)");
    println!("  --service <name>    Filter by service name (substring)");
    println!("  --msg <text>        Filter by message content (case-insensitive)");
    println!("  --since <hours>     Only entries from last N hours");
    println!("  --limit <n>         Maximum results (default: 100)");
    println!();
    println!("LOG LEVELS:");
    println!("  emerg, alert, crit, error, warn, notice, info, debug");
    println!();
    println!("EXAMPLES:");
    println!("  syslogd log info net.dhcp 'lease renewed for 10.0.2.15'");
    println!("  syslogd log error fs.ext4 'journal replay failed'");
    println!("  syslogd tail 50");
    println!("  syslogd query --level error --since 24");
    println!("  syslogd query --service net --msg timeout");
    println!("  syslogd follow");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    match args[1].as_str() {
        "daemon" => cmd_daemon(),
        "log" => {
            if args.len() < 5 {
                eprintln!("usage: syslogd log <level> <service> <message>");
                process::exit(1);
            }
            let level = &args[2];
            let service = &args[3];
            let message = args[4..].join(" ");

            // Parse extra key=value pairs from environment (future).
            cmd_log(level, service, &message, &[]);
        }
        "tail" => {
            let count = if args.len() >= 3 {
                args[2].parse().unwrap_or(20)
            } else {
                20
            };
            cmd_tail(count);
        }
        "follow" | "f" => cmd_follow(),
        "query" | "search" => {
            let filters = QueryFilters::parse_args(&args[2..]);
            cmd_query(&filters);
        }
        "stats" => cmd_stats(),
        "rotate" => cmd_rotate(),
        "clean" | "prune" => {
            let days = if args.len() >= 3 {
                args[2].parse().unwrap_or(30)
            } else {
                30
            };
            cmd_clean(days);
        }
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'syslogd help' for usage.");
            process::exit(1);
        }
    }
}

#[cfg(test)]
// CLAUDE.md allows the defensive lints inside `#[cfg(test)]`, where panicking
// on bad data is the point.
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    /// The first tests this daemon has ever had. They exist because
    /// `clean_file` REWRITES the file it reads, which is the shape that
    /// destroyed data in `useradd` on the same day -- so "it looks right" is
    /// not a standard it should be held to.
    fn log_with(dir: &ScratchDir, lines: &[&str]) -> std::path::PathBuf {
        let path = dir.path("messages.json");
        fs::write(&path, lines.join("\n") + "\n").expect("write fixture");
        path
    }

    /// One record in the shape `LogEntry::to_json` writes: the keys are `ts`
    /// and `msg`, not `timestamp` and `message`. My first fixture used the
    /// long names, which the parser files under `extra` while leaving
    /// `timestamp` at 0 -- so every entry looked older than every cutoff and
    /// the test reported 3 removed instead of 1. A fixture in a format the
    /// program does not write tests nothing; this one is taken from `to_json`.
    fn entry(ts: u64) -> String {
        format!(r#"{{"ts":{ts},"level":"info","service":"t","msg":"m"}}"#)
    }

    #[test]
    fn an_absent_log_is_the_one_absence() {
        let dir = ScratchDir::new("syslogd_clean");
        let got = clean_file(&dir.path("nothing-here.json"), 100);
        assert_eq!(got, Ok(Cleaned::NoFile));
    }

    #[test]
    fn a_log_that_cannot_be_read_is_an_error_not_an_absence() {
        // A directory where the file should be: it exists, so this is not
        // NotFound, and it cannot be read as text. Before 2026-09-12 this
        // printed "No log file." and returned 0, so the operator believed
        // there was nothing to clean.
        let dir = ScratchDir::new("syslogd_clean");
        let path = dir.path("messages.json");
        fs::create_dir(&path).expect("mkdir");
        assert!(clean_file(&path, 100).is_err());
    }

    #[test]
    fn a_line_that_is_not_utf8_is_kept_and_the_rest_cleaned() {
        // It used to cost the whole file: `read_to_string` refused it, and the
        // log went uncleaned. Now it is a line this daemon cannot parse, kept
        // byte for byte like any other.
        let dir = ScratchDir::new("syslogd_clean");
        let path = dir.path("messages.json");
        let mut bytes = vec![0x7B, 0xFF, 0x7D, 0x0A];
        bytes.extend_from_slice(format!("{}\n{}\n", entry(50), entry(250)).as_bytes());
        fs::write(&path, &bytes).expect("write");
        assert!(
            core::str::from_utf8(&bytes).is_err(),
            "fixture must not be utf-8, or this test proves nothing"
        );
        assert_eq!(
            clean_file(&path, 100),
            Ok(Cleaned::Trimmed {
                removed: 1,
                kept: 2
            })
        );
        let mut want = vec![0x7B, 0xFF, 0x7D, 0x0A];
        want.extend_from_slice(format!("{}\n", entry(250)).as_bytes());
        assert_eq!(fs::read(&path).unwrap(), want);
    }

    #[test]
    fn cleaning_keeps_carriage_returns_and_blank_lines_and_skips_a_useless_rewrite() {
        let dir = ScratchDir::new("syslogd_clean");
        let path = dir.path("messages.json");
        let text = format!("x\r\n\n{}\n", entry(250));
        fs::write(&path, &text).expect("write");
        assert_eq!(
            clean_file(&path, 100),
            Ok(Cleaned::Trimmed {
                removed: 0,
                kept: 3
            })
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn entries_older_than_the_cutoff_go_and_the_rest_stay() {
        let dir = ScratchDir::new("syslogd_clean");
        let path = log_with(&dir, &[&entry(50), &entry(150), &entry(250)]);
        let got = clean_file(&path, 100).expect("clean");
        assert_eq!(
            got,
            Cleaned::Trimmed {
                removed: 1,
                kept: 2
            }
        );

        let after = fs::read_to_string(&path).expect("read back");
        assert!(!after.contains("\"ts\":50"), "the old one is gone");
        assert!(after.contains("\"ts\":150"));
        assert!(after.contains("\"ts\":250"));
    }

    #[test]
    fn a_line_this_daemon_cannot_parse_is_kept_rather_than_dropped() {
        // It is somebody else's record. A cleaner that deletes what it does
        // not recognise loses data the first time the format changes.
        let dir = ScratchDir::new("syslogd_clean");
        let path = log_with(&dir, &["not json at all", &entry(50), &entry(250)]);
        let got = clean_file(&path, 100).expect("clean");
        assert_eq!(
            got,
            Cleaned::Trimmed {
                removed: 1,
                kept: 2
            }
        );
        let after = fs::read_to_string(&path).expect("read back");
        assert!(after.contains("not json at all"));
    }
}
