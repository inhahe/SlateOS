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
//! syslogd daemon [--socket PATH] [--journal FILE]
//!                             Receive /dev/log's messages into the journal
//! syslogd log <svc> <msg>     Write a log entry (for scripts/services)
//! syslogd query [filters]     Search log entries
//! syslogd tail [n]            Show last N entries (default 20)
//! syslogd follow              Live-tail the log file
//! syslogd stats               Show log statistics
//! syslogd rotate              Force log rotation
//! syslogd clean [days]        Remove logs older than N days (default 30)
//! ```
//!
//! `daemon` is the system log in the POSIX sense: it binds `/dev/log`, where
//! the C library's `syslog()` and `logger` send their messages, and files each
//! as systemd-journald would (design-decisions §1063) -- see [`daemon`],
//! [`frame`] and [`record`].

#[cfg(target_os = "linux")]
mod daemon;
// Built everywhere, so that the host runs their tests; used only by the
// daemon, which exists only where a Unix-domain socket can.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod frame;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod record;
#[cfg(target_os = "linux")]
mod sys;

use journalrec::Value;
use quoting::{os_bytes, quoteaf_os, quotef_os};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const LOG_DIR: &str = "/var/log";
const MAIN_LOG: &str = "syslog.jsonl";
/// Where the system's programs send their log messages.
#[cfg(target_os = "linux")]
const DEV_LOG: &str = "/dev/log";
/// Maximum log file size before rotation (5 MiB).
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;
/// Maximum number of rotated log files to keep.
const MAX_ROTATED_FILES: u32 = 10;

// ============================================================================
// Time helpers
// ============================================================================

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `YYYY-MM-DDTHH:MM:SSZ`: a record's `time` as `log` writes it, and its
/// time as the reading commands show it.
///
/// Through `journalrec::Utc`, in a fixed number of steps: this counted a
/// year at a time from 1970, so one record with a `ts` near `u64::MAX` --
/// which anything able to write a line into the log can put there -- kept
/// `tail`, `query` and `follow` busy for many minutes.
fn format_timestamp(unix_secs: u64) -> String {
    if unix_secs == 0 {
        return "unknown".to_string();
    }
    let t = journalrec::Utc::from_unix(unix_secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

// ============================================================================
// Records: one written by `log`, and one read by the other commands
// ============================================================================

/// A structured log entry, as `syslogd log` writes one.
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
}

/// A journal record as `tail`, `query`, `follow` and `stats` show it.
///
/// Read through `journalrec`'s parser, the one `journalctl` reads with. This
/// program had a parser of its own until 2026-10-07, which `journalctl`'s had
/// been fixed away from on 2026-09-26 and this one never was: it pushed each
/// byte of a string `as char`, so `café` came out as `cafÃ©`; it decoded no
/// `\uXXXX`; and a field that is not text -- an array of its bytes, which is
/// how this program's own daemon files a message that is not text
/// (design-decisions §1063) -- failed the whole parse, so the record was
/// never shown at all.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    /// Seconds since the epoch; 0 when the record has none.
    timestamp: u64,
    /// The level, by its canonical name (`journalrec::PRIORITY_NAMES`)
    /// whichever spelling the record used -- `syslogd log` wrote `error` and
    /// `warn` where every other writer writes `err` and `warning` -- or as
    /// written when it is none of them.
    level: String,
    /// The service and the message, as the bytes they are.
    service: Vec<u8>,
    message: Vec<u8>,
}

impl Shown {
    /// The record on `line`, if it is one: a line that is not UTF-8, or not
    /// a JSON object, is not.
    fn of(line: &[u8]) -> Option<Self> {
        let fields = journalrec::parse_object(std::str::from_utf8(line).ok()?)?;
        let text = |key: &str| fields.get(key).and_then(Value::text);
        let bytes = |key: &str| {
            fields
                .get(key)
                .map(|v| v.bytes().to_vec())
                .unwrap_or_default()
        };
        let level = text("level").map_or_else(String::new, |level| {
            journalrec::priority_name(level).map_or_else(|| level.to_string(), str::to_string)
        });
        Some(Shown {
            timestamp: text("ts").and_then(|t| t.parse().ok()).unwrap_or(0),
            level,
            service: bytes("service"),
            message: bytes("msg"),
        })
    }

    /// `TIME LEVEL [SERVICE] MESSAGE` and a newline, the level padded to the
    /// longest canonical name so the columns line up.
    fn write_short(&self, out: &mut dyn Write) -> io::Result<()> {
        write!(
            out,
            "{} {:<7} [",
            format_timestamp(self.timestamp),
            self.level
        )?;
        out.write_all(&self.service)?;
        out.write_all(b"] ")?;
        out.write_all(&self.message)?;
        out.write_all(b"\n")
    }
}

// ============================================================================
// Log file management
// ============================================================================

fn log_file_path() -> PathBuf {
    PathBuf::from(LOG_DIR).join(MAIN_LOG)
}

fn rotated_path(n: u32) -> PathBuf {
    rotated_path_of(&log_file_path(), n)
}

/// The `n`th rotated copy of the journal at `journal`: its name with `.n`
/// after it.
fn rotated_path_of(journal: &Path, n: u32) -> PathBuf {
    let mut name = journal.as_os_str().to_os_string();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

/// Append `entry` to the journal at `path`. Rotating it when it has grown too
/// big is the caller's: the entry is written either way.
fn write_log_entry(path: &Path, entry: &LogEntry) -> io::Result<()> {
    // A directory that cannot be made is reported by the append, which then
    // cannot make the file.
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }

    let mut line = entry.to_json().into_bytes();
    line.push(b'\n');
    // Under the journal's lock (design-decisions §1037): a vacuum or a
    // rotation holding the file finishes first, and the entry lands in
    // whichever file the path names then.
    journalio::append(path, &line)
}

/// Shift the rotated logs up one and the live log to `.1`, each step under
/// the journal's lock (design-decisions §1037): a `journalctl --vacuum-*`
/// rewriting one of these files would otherwise rename its result over
/// whatever this had just moved there, and a writer waiting on the live
/// log lands in the next one rather than in `.1`.
///
/// Returns each step that failed, and why. A failed step is left undone --
/// the files keep their names, and the log keeps growing until the next
/// rotation tries again -- and the steps after it are still tried. Until
/// 2026-10-07 every result was dropped here, and `syslogd rotate` said
/// "done" whatever had happened.
fn rotate_logs() -> Vec<(PathBuf, io::Error)> {
    rotate_journal(&log_file_path())
}

/// [`rotate_logs`] for the journal at `journal`, whichever file that is:
/// the daemon's `--journal` rotates where it writes.
fn rotate_journal(journal: &Path) -> Vec<(PathBuf, io::Error)> {
    let mut failed = Vec::new();
    let mut step = |path: PathBuf, act: &dyn Fn(journalio::Locked) -> io::Result<()>| {
        match journalio::Locked::open(&path) {
            Ok(Some(held)) => {
                if let Err(e) = act(held) {
                    failed.push((path, e));
                }
            }
            // Nothing there to move.
            Ok(None) => {}
            Err(e) => failed.push((path, e)),
        }
    };

    // Delete the oldest file.
    step(rotated_path_of(journal, MAX_ROTATED_FILES), &|held| {
        held.remove()
    });

    // Shift N-1 → N, N-2 → N-1, etc.
    for i in (1..MAX_ROTATED_FILES).rev() {
        let to = rotated_path_of(journal, i.saturating_add(1));
        step(rotated_path_of(journal, i), &|held| held.rename_to(&to));
    }

    // Move current → .1
    let first = rotated_path_of(journal, 1);
    step(journal.to_path_buf(), &|held| held.rename_to(&first));
    failed
}

/// The journal's bytes, or `None` when there is no journal -- the one
/// absence. Every other failure is an error: "No log file found." once stood
/// in for all of them, and so did one byte anywhere in the file that was not
/// UTF-8, since the file was read as a `String`. A line that is not UTF-8 is
/// now a line that is not a record, and costs only itself.
fn read_log(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The records among the lines of `bytes`, in order.
fn records(bytes: &[u8]) -> impl Iterator<Item = Shown> + '_ {
    journalio::lines(bytes).filter_map(Shown::of)
}

// ============================================================================
// Commands
// ============================================================================

/// `syslogd daemon [--socket PATH] [--journal FILE]`: receive the messages
/// sent to `/dev/log` (or PATH) into the journal (or FILE) until killed. See
/// [`daemon`]. Each option may also be written `--socket=PATH`.
///
/// It writes no PID file and no record of its own starting, which the idle
/// loop this replaced did: nothing read the PID file, and journald, whose
/// part this plays, writes neither.
#[cfg(target_os = "linux")]
fn cmd_daemon(args: &[OsString]) -> process::ExitCode {
    use std::os::unix::ffi::OsStrExt;

    let mut socket = PathBuf::from(DEV_LOG);
    let mut journal = log_file_path();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let bytes = word.as_bytes();
        let (name, inline) = match bytes.iter().position(|&b| b == b'=') {
            Some(at) if bytes.starts_with(b"--") => (
                bytes.get(..at).unwrap_or_default(),
                Some(std::ffi::OsStr::from_bytes(
                    bytes.get(at.saturating_add(1)..).unwrap_or_default(),
                )),
            ),
            _ => (bytes, None),
        };
        let target = match name {
            b"--socket" => &mut socket,
            b"--journal" => &mut journal,
            _ => {
                eprintln_safe(&format!(
                    "syslogd: unknown daemon option {}",
                    quoteaf_os(word)
                ));
                return process::ExitCode::FAILURE;
            }
        };
        match inline.or_else(|| words.next().map(OsString::as_os_str)) {
            Some(value) => *target = PathBuf::from(value),
            None => {
                eprintln_safe(&format!(
                    "syslogd: option {} needs a value",
                    quoteaf_os(word)
                ));
                return process::ExitCode::FAILURE;
            }
        }
    }
    daemon::run(
        &daemon::Options { socket, journal },
        MAX_LOG_SIZE,
        &rotate_journal,
    )
}

/// Elsewhere there are no Unix-domain sockets for `/dev/log` to be.
#[cfg(not(target_os = "linux"))]
fn cmd_daemon(_args: &[OsString]) -> process::ExitCode {
    eprintln_safe("syslogd: the daemon needs Unix-domain sockets, which this system lacks");
    process::ExitCode::FAILURE
}

/// A line on standard error that does not panic when it cannot be written,
/// as `eprintln!` does.
fn eprintln_safe(line: &str) {
    // Nowhere left to report a failure to report.
    let _ = writeln!(io::stderr().lock(), "{line}");
}

/// `syslogd: cannot WHAT PATH: REASON`, the reason in `strerror`'s words.
fn cannot(what: &str, path: &Path, e: &io::Error) {
    eprintln_safe(&format!(
        "syslogd: cannot {what} {}: {}",
        quotef_os(path),
        errmsg::strerror(e)
    ));
}

/// `syslogd: write error: REASON`, for output that could not be written.
fn write_error(e: &io::Error) {
    eprintln_safe(&format!("syslogd: write error: {}", errmsg::strerror(e)));
}

/// Write to standard output through `body`, buffered, and flush, stopping at
/// the first write that fails. `Ok(true)` when it was all written. A reader
/// that has gone (`EPIPE`) is an ordinary end to a pipeline such as
/// `syslogd tail | head`, and is `Ok(false)`, said nothing about.
///
/// Every line the commands print goes through here: `println!`, which they
/// all used until 2026-10-07, panics when the write fails -- a full disk, or
/// that reader gone -- so `syslogd help >/dev/full` ended in a panic and
/// status 101.
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
/// `syslogd: write error: REASON`, when it could not be written.
fn settle(written: io::Result<bool>, status: u8) -> u8 {
    match written {
        Ok(_) => status,
        Err(e) => {
            write_error(&e);
            1
        }
    }
}

/// `log LEVEL SERVICE MESSAGE...`: one record, written under the journal's
/// lock. The level is filed by its canonical name, as every other writer
/// files it: `error` and `warn`, which this used to write as given, are `err`
/// and `warning` in the journal.
fn cmd_log(level: &str, service: &str, message: &str, extra: &[(String, String)]) -> u8 {
    let level = journalrec::priority_name(level).unwrap_or_else(|| {
        eprintln_safe(&format!(
            "syslogd: warning: unknown level {}, using 'info'",
            quoteaf_os(level)
        ));
        "info"
    });

    let entry = LogEntry {
        timestamp: now_secs(),
        level: level.to_string(),
        service: service.to_string(),
        message: message.to_string(),
        extra: extra.to_vec(),
    };

    let path = log_file_path();
    if let Err(e) = write_log_entry(&path, &entry) {
        cannot("write", &path, &e);
        return 1;
    }
    // The record is written; a rotation that could not be done is said, and
    // tried again by the next writer to find the log too big.
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_SIZE) {
        for (file, e) in rotate_logs() {
            cannot("rotate", &file, &e);
        }
    }
    0
}

/// `tail [N]`: the last `count` records. Records, not lines: a line that is
/// not one used to take a record's place.
fn cmd_tail(count: usize) -> u8 {
    let path = log_file_path();
    let bytes = match read_log(&path) {
        Ok(bytes) => bytes.unwrap_or_default(),
        Err(e) => {
            cannot("read", &path, &e);
            return 1;
        }
    };
    let all: Vec<Shown> = records(&bytes).collect();
    let shown = all
        .get(all.len().saturating_sub(count)..)
        .unwrap_or_default();
    let written = to_stdout(|out| {
        if shown.is_empty() {
            return out.write_all(b"No log entries.\n");
        }
        shown.iter().try_for_each(|record| record.write_short(out))
    });
    settle(written, 0)
}

/// `query [FILTERS]`: the records `filters` picks out, up to its limit.
fn cmd_query(filters: &QueryFilters) -> u8 {
    let path = log_file_path();
    let bytes = match read_log(&path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return settle(to_stdout(|out| out.write_all(b"No log file found.\n")), 0),
        Err(e) => {
            cannot("read", &path, &e);
            return 1;
        }
    };
    let matched: Vec<Shown> = records(&bytes)
        .filter(|record| filters.matches(record))
        .take(filters.limit)
        .collect();
    let written = to_stdout(|out| {
        if matched.is_empty() {
            return out.write_all(b"No matching entries.\n");
        }
        for record in &matched {
            record.write_short(out)?;
        }
        writeln!(out, "\n{} entries matched.", matched.len())
    });
    settle(written, 0)
}

/// `follow`: each record as it is appended, until the output cannot take
/// more -- its reader gone (status 0) or a write failing (status 1).
///
/// The live log and its first rotated copy are both followed, each by its
/// identity (journalio's `Follow`): a rotation renames the live log to `.1`,
/// and what was appended to it just before is read there. This used to
/// re-read the whole live log twice a second and slice the `String` at the
/// old length -- which panicked whenever that length fell inside a
/// multi-byte character, skipped a log that was not all UTF-8 while moving
/// on past it, and lost a record caught half-appended.
fn cmd_follow() -> u8 {
    let live = log_file_path();
    let first = rotated_path(1);
    match to_stdout(|out| {
        writeln!(
            out,
            "syslogd: following {} (Ctrl+C to stop)",
            live.display()
        )
    }) {
        Ok(true) => {}
        Ok(false) => return 0,
        Err(e) => {
            write_error(&e);
            return 1;
        }
    }

    let mut follow = journalio::Follow::new();
    for path in [&first, &live] {
        if let Err(e) = follow.skip_to_end(path) {
            cannot("read", path, &e);
        }
    }
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        // The rotated copy first: what is new in it was written before
        // anything in the live log.
        for path in [&first, &live] {
            let bytes = match follow.read(path, true) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    cannot("read", path, &e);
                    continue;
                }
            };
            let fresh: Vec<Shown> = records(&bytes).collect();
            match to_stdout(|out| fresh.iter().try_for_each(|record| record.write_short(out))) {
                Ok(true) => {}
                Ok(false) => return 0,
                Err(e) => {
                    write_error(&e);
                    return 1;
                }
            }
        }
        follow.end_round();
    }
}

/// `stats`: the journal's size, its rotated copies, and its records by level
/// and by service. A file that cannot be read or examined is said, and the
/// status is 1, but what could be counted is still shown.
fn cmd_stats() -> u8 {
    let path = log_file_path();
    let mut failed = false;
    let (bytes, size) = match read_log(&path) {
        Ok(bytes) => {
            let bytes = bytes.unwrap_or_default();
            let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            (bytes, size)
        }
        Err(e) => {
            cannot("read", &path, &e);
            failed = true;
            (Vec::new(), 0)
        }
    };
    let all: Vec<Shown> = records(&bytes).collect();

    // Count rotated files.
    let mut rotated = 0u32;
    let mut rotated_size = 0u64;
    for i in 1..=MAX_ROTATED_FILES {
        let p = rotated_path(i);
        match fs::metadata(&p) {
            Ok(meta) => {
                rotated = rotated.saturating_add(1);
                rotated_size = rotated_size.saturating_add(meta.len());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                cannot("examine", &p, &e);
                failed = true;
            }
        }
    }

    // Records by level -- the canonical names in order of severity, then any
    // other spelling as written -- and by service, most first.
    let mut level_counts: BTreeMap<&str, u64> = BTreeMap::new();
    let mut service_counts: BTreeMap<&[u8], u64> = BTreeMap::new();
    for record in &all {
        let level = level_counts.entry(record.level.as_str()).or_insert(0);
        *level = level.saturating_add(1);
        let service = service_counts.entry(record.service.as_slice()).or_insert(0);
        *service = service.saturating_add(1);
    }
    let mut by_level: Vec<(&str, u64)> = journalrec::PRIORITY_NAMES
        .iter()
        .filter_map(|name| level_counts.remove(name).map(|n| (*name, n)))
        .collect();
    by_level.extend(level_counts);
    let mut by_service: Vec<(&[u8], u64)> = service_counts.into_iter().collect();
    by_service.sort_by_key(|&(_, count)| std::cmp::Reverse(count));

    let written = to_stdout(|out| {
        writeln!(out, "=== Syslog Statistics ===")?;
        writeln!(out, "  Main log:    {}", path.display())?;
        writeln!(out, "  Entries:     {}", all.len())?;
        writeln!(out, "  Size:        {}", format_size(size))?;
        writeln!(
            out,
            "  Rotated:     {rotated} files ({} total)",
            format_size(rotated_size)
        )?;
        writeln!(out, "  Max size:    {} per file", format_size(MAX_LOG_SIZE))?;
        writeln!(out, "  Max files:   {MAX_ROTATED_FILES}")?;
        if all.is_empty() {
            return Ok(());
        }
        writeln!(out, "\n  By level:")?;
        for (level, count) in &by_level {
            let level = if level.is_empty() { "(none)" } else { level };
            writeln!(out, "    {level:<8} {count}")?;
        }
        writeln!(out, "\n  Top services:")?;
        for (service, count) in by_service.iter().take(10) {
            out.write_all(b"    ")?;
            write_padded(out, service, 20)?;
            writeln!(out, " {count}")?;
        }
        Ok(())
    });
    settle(written, u8::from(failed))
}

/// `bytes` and then spaces to fill `width` columns -- a character to a
/// column when the bytes are text, a byte to one when they are not.
fn write_padded(out: &mut dyn Write, bytes: &[u8], width: usize) -> io::Result<()> {
    out.write_all(bytes)?;
    let used = std::str::from_utf8(bytes).map_or(bytes.len(), |s| s.chars().count());
    write!(out, "{:pad$}", "", pad = width.saturating_sub(used))
}

/// `rotate`: the rotation the daemon does when the log grows past its limit,
/// now. Each step that fails is said, and the status is 1.
fn cmd_rotate() -> u8 {
    let header = to_stdout(|out| writeln!(out, "syslogd: forcing log rotation"));
    let failed = rotate_logs();
    for (path, e) in &failed {
        cannot("rotate", path, e);
    }
    let written = header.and_then(|_| {
        if failed.is_empty() {
            to_stdout(|out| writeln!(out, "  done"))
        } else {
            Ok(true)
        }
    });
    settle(written, u8::from(!failed.is_empty()))
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
    let failed = |e: io::Error| format!("{}: {}", quotef_os(path), errmsg::strerror(&e));
    let Some(mut held) = journalio::Locked::open(path).map_err(failed)? else {
        return Ok(Cleaned::NoFile);
    };
    let content = held.read_all().map_err(failed)?;

    let mut kept: Vec<&[u8]> = Vec::new();
    let mut removed = 0usize;
    for line in journalio::lines(&content) {
        // A line this daemon cannot parse is KEPT, not dropped: it is somebody
        // else's record, and a cleaner that deletes what it does not recognise
        // is a cleaner that loses data on a format change.
        if Shown::of(line).is_some_and(|record| record.timestamp < cutoff) {
            removed = removed.saturating_add(1);
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

/// `clean [DAYS]`: every record older than `days` days dropped.
fn cmd_clean(days: u64) -> u8 {
    let cutoff = now_secs().saturating_sub(days.saturating_mul(86400));
    match clean_file(&log_file_path(), cutoff) {
        Ok(Cleaned::NoFile) => settle(to_stdout(|out| out.write_all(b"No log file.\n")), 0),
        Ok(Cleaned::Trimmed { removed, kept }) => {
            let written = to_stdout(|out| {
                writeln!(out, "  removed {removed} entries older than {days} days")?;
                writeln!(out, "  kept {kept} entries")
            });
            settle(written, 0)
        }
        Err(e) => {
            eprintln_safe(&format!("syslogd: {e}"));
            1
        }
    }
}

// ============================================================================
// Query filters
// ============================================================================

#[derive(Debug, Default)]
struct QueryFilters {
    /// A canonical level name, from `journalrec::priority_name`.
    level: Option<&'static str>,
    /// Bytes the service must contain.
    service: Option<Vec<u8>>,
    /// Bytes the message must contain, letters in either case.
    message_contains: Option<Vec<u8>>,
    /// The oldest timestamp shown.
    since: Option<u64>,
    limit: usize,
}

impl QueryFilters {
    fn matches(&self, record: &Shown) -> bool {
        self.level.is_none_or(|level| record.level == level)
            && self
                .service
                .as_ref()
                .is_none_or(|service| contains(&record.service, service))
            && self
                .message_contains
                .as_ref()
                .is_none_or(|text| contains_folded(&record.message, text))
            && self.since.is_none_or(|since| record.timestamp >= since)
    }

    /// `query`'s arguments, or the message refusing them.
    ///
    /// Every option needs its value, and a value that does not parse is
    /// refused. Until 2026-10-07 an option this did not know was skipped --
    /// so a misspelt `--servce net` showed every record -- as was an option
    /// at the end without its value, and a `--since` or `--limit` that did not
    /// parse; and `--level` matched only this program's own spellings, so
    /// `--level error` missed every record the daemon files as `err`.
    fn parse(args: &[OsString]) -> Result<Self, String> {
        let mut filters = Self {
            limit: 100,
            ..Self::default()
        };
        let mut words = args.iter();
        while let Some(option) = words.next() {
            let mut value = || {
                words
                    .next()
                    .ok_or_else(|| format!("option {} needs a value", quoteaf_os(option)))
            };
            match option.as_encoded_bytes() {
                b"--level" | b"-l" => {
                    let level = value()?;
                    filters.level = Some(
                        level
                            .to_str()
                            .and_then(journalrec::priority_name)
                            .ok_or_else(|| format!("unknown level {}", quoteaf_os(level)))?,
                    );
                }
                b"--service" | b"-s" => filters.service = Some(os_bytes(value()?).into_owned()),
                b"--msg" | b"-m" => {
                    filters.message_contains = Some(os_bytes(value()?).into_owned());
                }
                b"--since" => {
                    let hours = value()?;
                    let n: u64 = hours
                        .to_str()
                        .and_then(|h| h.parse().ok())
                        .ok_or_else(|| format!("invalid number of hours {}", quoteaf_os(hours)))?;
                    filters.since = Some(now_secs().saturating_sub(n.saturating_mul(3600)));
                }
                b"--limit" | b"-n" => {
                    let limit = value()?;
                    filters.limit = limit
                        .to_str()
                        .and_then(|n| n.parse().ok())
                        .ok_or_else(|| format!("invalid limit {}", quoteaf_os(limit)))?;
                }
                _ => return Err(format!("unknown query option {}", quoteaf_os(option))),
            }
        }
        Ok(filters)
    }
}

/// Whether `needle` occurs in `haystack`, byte for byte.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

/// Whether `needle` occurs in `haystack`, letters matched in either case:
/// every letter when both are text, as `query --msg` always matched, and
/// ASCII letters when either is not.
fn contains_folded(haystack: &[u8], needle: &[u8]) -> bool {
    match (std::str::from_utf8(haystack), std::str::from_utf8(needle)) {
        (Ok(h), Ok(n)) => h.to_lowercase().contains(&n.to_lowercase()),
        _ => contains(&haystack.to_ascii_lowercase(), &needle.to_ascii_lowercase()),
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

/// `help`'s text, a literal to a line: `scripts/check-help-vs-parser.py`
/// reads each option line on its own.
const USAGE: &[&str] = &[
    "Slate OS System Log Daemon v0.1.0",
    "",
    "JSON-lines structured log aggregation and query service.",
    "Logs are stored at /var/log/syslog.jsonl.",
    "",
    "USAGE:",
    "  syslogd <command> [arguments]",
    "",
    "COMMANDS:",
    "  daemon [--socket PATH] [--journal FILE]",
    "                                 Receive /dev/log's messages into the journal",
    "  log <level> <service> <msg>    Write a log entry",
    "  tail [n]                       Show last N entries (default: 20)",
    "  follow                         Live-tail the log file",
    "  query [filters]                Search log entries",
    "  stats                          Show log statistics",
    "  rotate                         Force log rotation",
    "  clean [days]                   Remove entries older than N days (default: 30)",
    "",
    "QUERY FILTERS:",
    "  --level <level>     Filter by severity (emerg..debug)",
    "  --service <name>    Filter by service name (substring)",
    "  --msg <text>        Filter by message content (case-insensitive)",
    "  --since <hours>     Only entries from last N hours",
    "  --limit <n>         Maximum results (default: 100)",
    "",
    "LOG LEVELS:",
    "  emerg, alert, crit, err, warning, notice, info, debug",
    "  (or 0-7; error, warn, emergency and critical are accepted too)",
    "",
    "EXAMPLES:",
    "  syslogd log info net.dhcp 'lease renewed for 10.0.2.15'",
    "  syslogd log err fs.ext4 'journal replay failed'",
    "  syslogd tail 50",
    "  syslogd query --level err --since 24",
    "  syslogd query --service net --msg timeout",
    "  syslogd follow",
];

fn print_usage(out: &mut dyn Write) -> io::Result<()> {
    for line in USAGE {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// The words of `words` as text, or the message refusing the first that is
/// not: `log`'s level, service and message are filed as text.
fn texts(words: &[OsString]) -> Result<Vec<&str>, String> {
    words
        .iter()
        .map(|word| {
            word.to_str()
                .ok_or_else(|| format!("argument {} is not text", quoteaf_os(word)))
        })
        .collect()
}

/// The one optional number a command takes -- `tail`'s count, `clean`'s
/// days -- `default` when it is not given, or the message refusing it:
/// `what` names it. A word that is not a number used to stand for the
/// default.
fn optional_number<T: std::str::FromStr>(
    words: &[OsString],
    default: T,
    what: &str,
) -> Result<T, String> {
    match words {
        [] => Ok(default),
        [word] => word
            .to_str()
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| format!("invalid {what} {}", quoteaf_os(word))),
        [_, extra, ..] => Err(unexpected(extra)),
    }
}

/// The message refusing a word a command does not take.
fn unexpected(word: &OsString) -> String {
    format!("unexpected argument {}", quoteaf_os(word))
}

/// No words at all, or the message refusing the first.
fn no_words(words: &[OsString]) -> Result<(), String> {
    words.first().map_or(Ok(()), |word| Err(unexpected(word)))
}

fn main() -> process::ExitCode {
    // `args_os`: `env::args` panics on an argument that is not UTF-8. The
    // daemon takes its paths as they are, and `query` its patterns; a word
    // that must be text and is not is refused with the rest.
    let args: Vec<OsString> = env::args_os().collect();
    let rest = args.get(2..).unwrap_or_default();
    let command = args.get(1).map(|word| word.as_encoded_bytes());
    if matches!(command, Some(b"daemon")) {
        return cmd_daemon(rest);
    }
    let status = match command {
        None | Some(b"help" | b"--help" | b"-h") => Ok(settle(to_stdout(print_usage), 0)),
        Some(b"log") => texts(rest).and_then(|words| match words.as_slice() {
            [level, service, message @ ..] if !message.is_empty() => {
                Ok(cmd_log(level, service, &message.join(" "), &[]))
            }
            _ => Err("usage: syslogd log <level> <service> <message>".to_string()),
        }),
        Some(b"tail") => optional_number(rest, 20, "count").map(cmd_tail),
        Some(b"follow" | b"f") => no_words(rest).map(|()| cmd_follow()),
        Some(b"query" | b"search") => QueryFilters::parse(rest).map(|filters| cmd_query(&filters)),
        Some(b"stats") => no_words(rest).map(|()| cmd_stats()),
        Some(b"rotate") => no_words(rest).map(|()| cmd_rotate()),
        Some(b"clean" | b"prune") => optional_number(rest, 30, "number of days").map(cmd_clean),
        Some(_) => {
            let other = args.get(1).map(OsString::as_os_str).unwrap_or_default();
            eprintln_safe(&format!("syslogd: unknown command {}", quoteaf_os(other)));
            eprintln_safe("Run 'syslogd help' for usage.");
            Ok(1)
        }
    };
    match status {
        Ok(status) => process::ExitCode::from(status),
        Err(message) => {
            eprintln_safe(&format!("syslogd: {message}"));
            process::ExitCode::FAILURE
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

    /// A record the daemon filed with a message that is not text -- an array
    /// of its bytes -- is cleaned like any other. The old parser could not
    /// read one, so it was kept for ever.
    #[test]
    fn a_record_whose_message_is_bytes_is_cleaned_too() {
        let dir = ScratchDir::new("syslogd_clean");
        let old = r#"{"ts":50,"level":"info","service":"t","msg":[98,255]}"#;
        let path = log_with(&dir, &[old, &entry(250)]);
        assert_eq!(
            clean_file(&path, 100),
            Ok(Cleaned::Trimmed {
                removed: 1,
                kept: 1
            })
        );
    }

    // --- Reading a record (the shared parser) ---

    fn shown(line: &str) -> Shown {
        Shown::of(line.as_bytes()).expect("a record")
    }

    fn short(record: &Shown) -> Vec<u8> {
        let mut out = Vec::new();
        record.write_short(&mut out).unwrap();
        out
    }

    /// `café` used to come out as `cafÃ©`: each byte pushed `as char`.
    #[test]
    fn a_record_keeps_its_characters() {
        let record = shown(
            "{\"ts\":0,\"level\":\"info\",\"service\":\"d\u{e9}\",\"msg\":\"caf\u{e9} \\u00e9\"}",
        );
        assert_eq!(record.message, "caf\u{e9} \u{e9}".as_bytes());
        assert_eq!(record.service, "d\u{e9}".as_bytes());
        assert_eq!(
            short(&record),
            "unknown info    [d\u{e9}] caf\u{e9} \u{e9}\n".as_bytes()
        );
    }

    /// A message that is not text is an array of its bytes (design-decisions
    /// §1063) -- how this program's own daemon files one. The old parser
    /// failed on the array, and the record was never shown.
    #[test]
    fn a_message_that_is_not_text_is_shown_as_its_bytes() {
        let record = shown(r#"{"ts":0,"level":"err","service":"t","msg":[98,97,100,32,255]}"#);
        assert_eq!(record.message, b"bad \xff");
        assert_eq!(short(&record), b"unknown err     [t] bad \xff\n");
    }

    /// Every spelling of a level is its canonical name: `syslogd log` wrote
    /// `error` and `warn` where the daemon, `logger` and `systemd-cat` write
    /// `err` and `warning`.
    #[test]
    fn levels_are_read_by_their_canonical_names() {
        for (written, read) in [
            ("error", "err"),
            ("warn", "warning"),
            ("3", "err"),
            ("err", "err"),
        ] {
            let record = shown(&format!(r#"{{"ts":1,"level":"{written}","msg":"m"}}"#));
            assert_eq!(record.level, read, "{written}");
        }
        assert_eq!(shown(r#"{"ts":1,"level":"loud","msg":"m"}"#).level, "loud");
        assert_eq!(shown(r#"{"ts":1,"msg":"m"}"#).level, "");
    }

    #[test]
    fn a_line_that_is_not_a_record_is_none() {
        assert_eq!(Shown::of(b"not json"), None);
        assert_eq!(Shown::of(b"{\"msg\":\"\xff\"}"), None);
        assert_eq!(Shown::of(b""), None);
    }

    // --- Query filters ---

    fn words(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn query(list: &[&str]) -> Result<QueryFilters, String> {
        QueryFilters::parse(&words(list))
    }

    /// What used to be skipped -- an unknown option, an option missing its
    /// value, a value that does not parse -- is refused.
    #[test]
    fn a_query_refuses_what_it_used_to_skip() {
        let err = |list: &[&str]| query(list).err().unwrap();
        assert_eq!(err(&["--servce", "net"]), "unknown query option '--servce'");
        assert_eq!(err(&["--level"]), "option '--level' needs a value");
        assert_eq!(err(&["--level", "loud"]), "unknown level 'loud'");
        assert_eq!(
            err(&["--since", "yesterday"]),
            "invalid number of hours 'yesterday'"
        );
        assert_eq!(err(&["--limit", "-1"]), "invalid limit '-1'");
    }

    #[test]
    fn a_query_reads_its_filters() {
        let f = query(&["-l", "error", "-s", "net", "-m", "Lease", "-n", "5"]).unwrap();
        assert_eq!(f.level, Some("err"));
        assert_eq!(f.service.as_deref(), Some(&b"net"[..]));
        assert_eq!(f.message_contains.as_deref(), Some(&b"Lease"[..]));
        assert_eq!(f.limit, 5);
        let f = query(&["--since", "24"]).unwrap();
        assert!(f.since.is_some_and(|since| since <= now_secs()));
        assert_eq!(query(&[]).unwrap().limit, 100);
        // Hours enough to overflow a multiplication saturate instead.
        let most = u64::MAX.to_string();
        assert_eq!(query(&["--since", most.as_str()]).unwrap().since, Some(0));
    }

    #[test]
    fn a_query_matches_by_level_service_and_message() {
        let record =
            shown(r#"{"ts":10,"level":"error","service":"net.dhcp","msg":"Lease RENEWED"}"#);
        let matches = |list: &[&str]| query(list).unwrap().matches(&record);
        assert!(matches(&["--level", "err"]));
        assert!(matches(&["--level", "3"]));
        assert!(!matches(&["--level", "info"]));
        assert!(matches(&["--service", "dhcp"]));
        assert!(!matches(&["--service", "DHCP"]));
        assert!(matches(&["--msg", "lease renewed"]));
        assert!(!matches(&["--msg", "expired"]));
    }

    #[test]
    fn messages_are_matched_in_either_case() {
        assert!(contains_folded(
            "CAF\u{c9}".as_bytes(),
            "caf\u{e9}".as_bytes()
        ));
        assert!(contains_folded(b"BAD \xff", b"bad"));
        assert!(contains_folded(b"bad \xff", b"\xff"));
        assert!(!contains_folded(b"abc", b"abcd"));
        assert!(contains_folded(b"", b""));
    }

    // --- Arguments ---

    #[test]
    fn a_count_is_a_number_or_refused() {
        assert_eq!(optional_number::<usize>(&words(&[]), 20, "count"), Ok(20));
        assert_eq!(optional_number::<usize>(&words(&["5"]), 20, "count"), Ok(5));
        assert_eq!(
            optional_number::<usize>(&words(&["banana"]), 20, "count"),
            Err("invalid count 'banana'".to_string())
        );
        assert_eq!(
            optional_number::<u64>(&words(&["1", "2"]), 30, "number of days"),
            Err("unexpected argument '2'".to_string())
        );
        assert_eq!(
            no_words(&words(&["x"])),
            Err("unexpected argument 'x'".to_string())
        );
        assert_eq!(no_words(&words(&[])), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn a_log_word_that_is_not_text_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let word = std::ffi::OsStr::from_bytes(b"a\xffb").to_os_string();
        let args = [OsString::from("info"), word];
        assert_eq!(
            texts(&args),
            Err("argument 'a'$'\\377''b' is not text".to_string())
        );
    }

    // --- Output ---

    #[test]
    fn a_failed_write_is_status_one() {
        assert_eq!(settle(Ok(true), 0), 0);
        assert_eq!(settle(Ok(false), 1), 1);
        assert_eq!(settle(Err(io::Error::other("no space")), 0), 1);
    }

    #[test]
    fn the_usage_is_written_a_line_at_a_time() {
        let mut out = Vec::new();
        print_usage(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), USAGE.len());
        assert!(text.contains("  tail [n]"));
    }

    /// The value that kept the old year-by-year count busy for minutes.
    #[test]
    fn a_time_is_formatted_at_once() {
        assert_eq!(format_timestamp(0), "unknown");
        assert_eq!(format_timestamp(1_716_000_000), "2024-05-18T02:40:00Z");
        assert_eq!(format_timestamp(u64::MAX), "584554051223-11-09T07:00:15Z");
        let record = shown(r#"{"ts":18446744073709551615,"level":"info","msg":"m"}"#);
        assert!(short(&record).starts_with(b"584554051223-11-09T07:00:15Z info"));
    }

    #[test]
    fn padding_counts_characters_when_it_can() {
        let mut out = Vec::new();
        write_padded(&mut out, "d\u{e9}".as_bytes(), 4).unwrap();
        assert_eq!(out, "d\u{e9}  ".as_bytes());
        let mut out = Vec::new();
        write_padded(&mut out, b"\xff", 3).unwrap();
        assert_eq!(out, b"\xff  ");
        let mut out = Vec::new();
        write_padded(&mut out, b"toolong", 3).unwrap();
        assert_eq!(out, b"toolong");
    }

    // --- Rotation ---

    /// Each file moves up one; nothing is reported when nothing failed.
    #[test]
    fn a_rotation_moves_each_file_up_one() {
        let dir = ScratchDir::new("syslogd_rotate");
        let live = dir.path("syslog.jsonl");
        fs::write(&live, "live\n").unwrap();
        fs::write(rotated_path_of(&live, 1), "one\n").unwrap();
        fs::write(rotated_path_of(&live, MAX_ROTATED_FILES), "oldest\n").unwrap();
        let failed = rotate_journal(&live);
        assert!(failed.is_empty(), "{failed:?}");
        assert!(!live.exists());
        assert_eq!(
            fs::read_to_string(rotated_path_of(&live, 1)).unwrap(),
            "live\n"
        );
        assert_eq!(
            fs::read_to_string(rotated_path_of(&live, 2)).unwrap(),
            "one\n"
        );
        assert!(!rotated_path_of(&live, MAX_ROTATED_FILES + 1).exists());
        assert!(!rotated_path_of(&live, MAX_ROTATED_FILES).exists());
    }
}
