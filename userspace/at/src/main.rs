//! Slate OS `at` -- schedule one-time command execution at a specified time
//!
//! Manages job files in `/var/spool/at/`. Each job is a text file containing
//! a header (time, queue, user, creation timestamp) followed by shell commands.
//! Actual execution is handled by the `atd` daemon; this tool only manages the
//! queue.
//!
//! # Usage
//!
//! ```text
//! at <timespec>                 Schedule commands from stdin
//! at -f <file> <timespec>       Schedule commands from file
//! at -l / atq                   List pending jobs
//! at -d <jobid> / atrm <jobid>  Remove a scheduled job
//! at -c <jobid>                 Display job contents
//! at -v <timespec>              Display scheduled time without creating job
//! at -b / batch                 Schedule for when load drops below threshold
//! at -q <queue> <timespec>      Use specified queue letter (a-z)
//! at -m <timespec>              Mail output to user when job completes
//! at --json -l                  List jobs in JSON format
//! ```
//!
//! # Symlink Aliases
//!
//! When invoked as `atq`, behaves as `at -l`.
//! When invoked as `atrm`, behaves as `at -d`.
//! When invoked as `batch`, behaves as `at -b`.
//!
//! # Time Specifications
//!
//! - `HH:MM` -- today if time hasn't passed, tomorrow otherwise
//! - `HH:MM YYYY-MM-DD` -- specific date and time
//! - `now + N minutes/hours/days/weeks` -- relative offset from now
//! - `noon`, `midnight`, `teatime` (16:00) -- named times
//! - `tomorrow` -- next day, same time as now
//! - `next week`, `next month` -- simple relative
//!
//! # Syscall Interface
//!
//! Uses `clock_gettime` (syscall 40) for current time via inline x86_64
//! assembly.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// Spool directory for at job files.
// Spelled in `cronspool` so the daemons and the editor cannot drift apart.
const SPOOL_DIR: &str = cronspool::AT_SPOOL;

/// Default queue letter.
const DEFAULT_QUEUE: char = 'a';

/// Native Slate OS wall-clock syscall (kernel syscall/number.rs); no-arg,
/// returns nanoseconds-since-epoch in rax.  The kernel has no combined
/// clock_gettime(clock_id, *ts) form.  (Syscall 40 is SYS_PORT_READ; the old
/// SYS_CLOCK_GETTIME=40 was wrong.)
#[cfg(target_vendor = "slateos")]
const SYS_CLOCK_REALTIME: u64 = 14;

/// Nanoseconds per second, to convert the kernel's ns clock value to seconds.
#[cfg(target_vendor = "slateos")]
const NSEC_PER_SEC: i64 = 1_000_000_000;

/// Seconds per day.
const SECS_PER_DAY: i64 = 86400;

/// Seconds per hour.
const SECS_PER_HOUR: i64 = 3600;

/// Seconds per minute.
const SECS_PER_MINUTE: i64 = 60;

// ============================================================================
// Timespec / syscall
// ============================================================================

/// Read the current wall-clock time.
///
/// Returns epoch seconds on success.  On SlateOS this uses the native
/// no-argument `SYS_CLOCK_REALTIME` syscall, which returns
/// nanoseconds-since-epoch in `rax`, and falls back to `std` if the kernel
/// reports an error.
///
/// The raw `syscall` is gated on `target_vendor = "slateos"` (see
/// `toolchain/x86_64-slateos.json`), which is true only when compiling for the
/// real OS.  A `syscall` instruction on a development host does not fail
/// cleanly: it enters whatever kernel is actually running with our call number
/// in `rax`, and SlateOS number 14 is `rt_sigprocmask` on Linux.  On a host we
/// therefore skip it entirely and use `std`, which is the *correct* answer
/// there anyway — this function only reads a clock, so there is nothing the
/// syscall would have told us that `std` cannot.
fn get_current_time() -> Result<i64, String> {
    #[cfg(target_vendor = "slateos")]
    {
        let ret: i64;

        // SAFETY: SYS_CLOCK_REALTIME takes no arguments and writes nothing to
        // userspace; it only reads the kernel clock into rax.  rcx/r11 are
        // clobbered by the SYSCALL instruction.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") SYS_CLOCK_REALTIME,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack, nomem),
            );
        }

        if ret >= 0 {
            // `ret` is nanoseconds since the epoch; return whole seconds.
            return Ok(ret / NSEC_PER_SEC);
        }
    }

    // Fallback (and the only path on a development host): std SystemTime.
    match std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH) {
        Ok(dur) => Ok(dur.as_secs() as i64),
        Err(e) => Err(format!("cannot get current time: {e}")),
    }
}

// ============================================================================
// Error type
// ============================================================================

/// All errors the tool can produce.
#[derive(Debug)]
enum Error {
    /// Bad command-line usage.
    Usage(String),
    /// I/O or filesystem error.
    Io(String),
    /// Time parsing error.
    TimeParse(String),
    /// Job not found.
    NotFound(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Usage(msg) | Error::Io(msg) | Error::TimeParse(msg) | Error::NotFound(msg) => {
                write!(f, "{msg}")
            }
        }
    }
}

// ============================================================================
// Calendar math (from scratch, no chrono)
// ============================================================================

/// Broken-down date and time.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DateTime {
    year: i64,
    month: u32,  // 1-12
    day: u32,    // 1-31
    hour: u32,   // 0-23
    minute: u32, // 0-59
    second: u32, // 0-59
}

const WEEKDAY_ABBR: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTH_ABBR: [&str; 13] = [
    "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Whether `year` is a leap year.
///
/// Delegates to `civildate`. Kept as a wrapper rather than deleted because
/// four tests call it -- and clippy reported it "never used" from the non-test
/// build, which is exactly the reading that would have removed it and broken
/// them. `--all-targets` and a plain build disagree about dead code, and the
/// plain build is the one that sounds authoritative.
/// Only the tests call this now, and `#[cfg(test)]` says so rather than
/// `#[allow(dead_code)]` -- the first is a fact about who uses it, the second
/// is a request to stop being told.
#[cfg(test)]
fn is_leap_year(year: i64) -> bool {
    i32::try_from(year).is_ok_and(civildate::is_leap_year)
}

/// Days in a given month, or 0 for a month outside 1..=12.
///
/// Delegates to `civildate`, which answers with a `match` rather than by
/// indexing a table. The version here read `DAYS_IN_MONTH[month as usize]`
/// behind a range guard -- correct, but one of two copies of the same table in
/// this tree, and the copy without the guard is right below.
fn days_in_month(year: i64, month: u32) -> u32 {
    let Ok(y) = i32::try_from(year) else {
        // Not a truncating cast. A year outside `i32` is not a date, and
        // `as i32` would silently answer about a different year entirely --
        // 4_294_967_298 would become 2. 0 is what this function already
        // returns for an impossible month.
        return 0;
    };
    civildate::days_in_month(y, month)
}

/// Day of week: 0=Mon, 1=Tue, ..., 6=Sun (ISO 8601).
///
/// # What this replaced, and the bug in it
///
/// Sakamoto's algorithm over a twelve-element table, indexed as
/// `T[(m - 1) as usize]` with NO range guard. For `month == 0` that is
/// `(-1) as usize`, which wraps to `usize::MAX` and panics -- and month is
/// `u32` straight off a parsed timespec, so the guard was the caller's job and
/// no caller did it. `days_in_month` immediately above DID guard its range,
/// which is what makes this the kind of defect a reader skims past.
///
/// `civildate::day_of_week` computes from the day count and indexes nothing.
fn day_of_week(year: i64, month: u32, day: u32) -> u32 {
    let Ok(y) = i32::try_from(year) else {
        return 0;
    };
    // `civildate` counts 0=Sunday; this returns ISO, 0=Monday.
    (civildate::day_of_week(y, month, day) + 6) % 7
}

/// Convert Unix epoch seconds to broken-down DateTime.
///
/// Uses Howard Hinnant's civil_from_days algorithm.
fn epoch_to_datetime(epoch_sec: i64) -> DateTime {
    let total_days = epoch_sec.div_euclid(SECS_PER_DAY);
    let day_seconds = epoch_sec.rem_euclid(SECS_PER_DAY);

    let hour = (day_seconds / 3600) as u32;
    let minute = ((day_seconds % 3600) / 60) as u32;
    let second = (day_seconds % 60) as u32;

    // Shift epoch from 1970-01-01 to 0000-03-01.
    let z = total_days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy_from_mar = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy_from_mar + 2) / 153;
    let d = doy_from_mar - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    DateTime {
        year,
        month: m as u32,
        day: d as u32,
        hour,
        minute,
        second,
    }
}

/// Convert broken-down DateTime to Unix epoch seconds.
///
/// Uses Howard Hinnant's days_from_civil algorithm.
fn datetime_to_epoch(dt: &DateTime) -> Result<i64, String> {
    if dt.month < 1 || dt.month > 12 {
        return Err(format!("month out of range: {}", dt.month));
    }
    let max_day = days_in_month(dt.year, dt.month);
    if dt.day < 1 || dt.day > max_day {
        return Err(format!(
            "day {} out of range for {:04}-{:02} (max {max_day})",
            dt.day, dt.year, dt.month
        ));
    }
    if dt.hour > 23 {
        return Err(format!("hour out of range: {}", dt.hour));
    }
    if dt.minute > 59 {
        return Err(format!("minute out of range: {}", dt.minute));
    }
    if dt.second > 59 {
        return Err(format!("second out of range: {}", dt.second));
    }

    let y = if dt.month <= 2 { dt.year - 1 } else { dt.year };
    let m = if dt.month <= 2 {
        dt.month as i64 + 9
    } else {
        dt.month as i64 - 3
    };
    let d = dt.day as i64;

    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let total_days = era * 146_097 + doe - 719_468;

    let epoch = total_days * SECS_PER_DAY
        + dt.hour as i64 * SECS_PER_HOUR
        + dt.minute as i64 * SECS_PER_MINUTE
        + dt.second as i64;

    Ok(epoch)
}

// ============================================================================
// Time formatting
// ============================================================================

/// Format a DateTime for display: "Sat May 17 18:30:00 2025".
fn format_datetime(dt: &DateTime) -> String {
    let dow = day_of_week(dt.year, dt.month, dt.day) as usize;
    let wday = if dow < WEEKDAY_ABBR.len() {
        WEEKDAY_ABBR[dow]
    } else {
        "???"
    };
    let m = dt.month as usize;
    let mon = if (1..=12).contains(&m) {
        MONTH_ABBR[m]
    } else {
        "???"
    };
    format!(
        "{wday} {mon} {:2} {:02}:{:02}:{:02} {:04}",
        dt.day, dt.hour, dt.minute, dt.second, dt.year
    )
}

// ============================================================================
// Time specification parsing
// ============================================================================

/// Parse a time specification string into a Unix epoch timestamp.
///
/// `now_epoch` is the current time used to resolve relative specs.
fn parse_timespec(spec: &str, now_epoch: i64) -> Result<i64, Error> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(Error::TimeParse("empty time specification".into()));
    }

    let lower = spec.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();

    if tokens.is_empty() {
        return Err(Error::TimeParse("empty time specification".into()));
    }

    // Named times: "noon", "midnight", "teatime"
    if tokens.len() == 1 {
        match tokens[0] {
            "noon" => return resolve_named_time(now_epoch, 12, 0),
            "midnight" => return resolve_named_time(now_epoch, 0, 0),
            "teatime" => return resolve_named_time(now_epoch, 16, 0),
            "tomorrow" => {
                return Ok(now_epoch + SECS_PER_DAY);
            }
            _ => {}
        }
    }

    // "now + N unit"
    if tokens.len() >= 4 && tokens[0] == "now" && tokens[1] == "+" {
        return parse_relative_offset(now_epoch, &tokens[2..]);
    }

    // "next week" / "next month"
    if tokens.len() == 2 && tokens[0] == "next" {
        return parse_next(now_epoch, tokens[1]);
    }

    // "noon tomorrow", "midnight tomorrow", "teatime tomorrow"
    if tokens.len() == 2 {
        let named_hour = match tokens[0] {
            "noon" => Some(12u32),
            "midnight" => Some(0u32),
            "teatime" => Some(16u32),
            _ => None,
        };
        if let Some(hour) = named_hour
            && tokens[1] == "tomorrow"
        {
            let now_dt = epoch_to_datetime(now_epoch);
            let mut dt = DateTime {
                year: now_dt.year,
                month: now_dt.month,
                day: now_dt.day,
                hour,
                minute: 0,
                second: 0,
            };
            advance_day(&mut dt);
            return datetime_to_epoch(&dt).map_err(Error::TimeParse);
        }
    }

    // "HH:MM YYYY-MM-DD"
    if tokens.len() == 2
        && let Some(epoch) = try_parse_hhmm_date(&tokens)
    {
        return epoch;
    }

    // "HH:MM" alone (today or tomorrow)
    if tokens.len() == 1
        && let Some(epoch) = try_parse_hhmm(tokens[0], now_epoch)
    {
        return epoch;
    }

    // "HH:MM tomorrow"
    if tokens.len() == 2
        && tokens[1] == "tomorrow"
        && let Some((hour, minute)) = parse_hhmm_pair(tokens[0])
    {
        let now_dt = epoch_to_datetime(now_epoch);
        let mut dt = DateTime {
            year: now_dt.year,
            month: now_dt.month,
            day: now_dt.day,
            hour,
            minute,
            second: 0,
        };
        advance_day(&mut dt);
        return datetime_to_epoch(&dt).map_err(Error::TimeParse);
    }

    Err(Error::TimeParse(format!(
        "cannot parse time specification: '{spec}'"
    )))
}

/// Parse "HH:MM" from a single token. Returns (hour, minute) or None.
fn parse_hhmm_pair(token: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let hour: u32 = parts[0].parse().ok()?;
    let minute: u32 = parts[1].parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some((hour, minute))
}

/// Try to parse a single "HH:MM" token. Resolves to today if the time hasn't
/// passed, or tomorrow if it has.
fn try_parse_hhmm(token: &str, now_epoch: i64) -> Option<Result<i64, Error>> {
    let (hour, minute) = parse_hhmm_pair(token)?;
    let now_dt = epoch_to_datetime(now_epoch);

    let mut dt = DateTime {
        year: now_dt.year,
        month: now_dt.month,
        day: now_dt.day,
        hour,
        minute,
        second: 0,
    };

    let target = match datetime_to_epoch(&dt) {
        Ok(t) => t,
        Err(e) => return Some(Err(Error::TimeParse(e))),
    };

    // If the time has already passed today, schedule for tomorrow.
    if target <= now_epoch {
        advance_day(&mut dt);
        Some(datetime_to_epoch(&dt).map_err(Error::TimeParse))
    } else {
        Some(Ok(target))
    }
}

/// Try to parse "HH:MM YYYY-MM-DD" from two tokens.
fn try_parse_hhmm_date(tokens: &[&str]) -> Option<Result<i64, Error>> {
    if tokens.len() != 2 {
        return None;
    }

    let (hour, minute) = parse_hhmm_pair(tokens[0])?;

    // Parse date: YYYY-MM-DD
    let date_parts: Vec<&str> = tokens[1].split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let year: i64 = date_parts[0].parse().ok()?;
    let month: u32 = date_parts[1].parse().ok()?;
    let day: u32 = date_parts[2].parse().ok()?;

    let dt = DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second: 0,
    };

    Some(datetime_to_epoch(&dt).map_err(Error::TimeParse))
}

/// Resolve a named time (noon, midnight, teatime) to today or tomorrow.
fn resolve_named_time(now_epoch: i64, hour: u32, minute: u32) -> Result<i64, Error> {
    let now_dt = epoch_to_datetime(now_epoch);
    let mut dt = DateTime {
        year: now_dt.year,
        month: now_dt.month,
        day: now_dt.day,
        hour,
        minute,
        second: 0,
    };

    let target = datetime_to_epoch(&dt).map_err(Error::TimeParse)?;
    if target <= now_epoch {
        advance_day(&mut dt);
        datetime_to_epoch(&dt).map_err(Error::TimeParse)
    } else {
        Ok(target)
    }
}

/// Parse "now + N unit" where unit is minutes/hours/days/weeks.
fn parse_relative_offset(now_epoch: i64, tokens: &[&str]) -> Result<i64, Error> {
    if tokens.len() < 2 {
        return Err(Error::TimeParse(
            "expected 'now + N unit' (e.g. 'now + 30 minutes')".into(),
        ));
    }

    let n: i64 = tokens[0].parse().map_err(|_| {
        Error::TimeParse(format!("invalid number in relative time: '{}'", tokens[0]))
    })?;

    if n < 0 {
        return Err(Error::TimeParse("relative offset must be positive".into()));
    }

    let unit = tokens[1].trim_end_matches('s'); // "minutes" -> "minute"
    let seconds = match unit {
        "minute" | "min" => n * SECS_PER_MINUTE,
        "hour" | "hr" => n * SECS_PER_HOUR,
        "day" => n * SECS_PER_DAY,
        "week" | "wk" => n * SECS_PER_DAY * 7,
        _ => {
            return Err(Error::TimeParse(format!(
                "unknown time unit: '{}' (expected minutes, hours, days, or weeks)",
                tokens[1]
            )));
        }
    };

    Ok(now_epoch + seconds)
}

/// Parse "next week" / "next month".
fn parse_next(now_epoch: i64, unit: &str) -> Result<i64, Error> {
    match unit {
        "week" => Ok(now_epoch + SECS_PER_DAY * 7),
        "month" => {
            let mut dt = epoch_to_datetime(now_epoch);
            if dt.month == 12 {
                dt.month = 1;
                dt.year += 1;
            } else {
                dt.month += 1;
            }
            // Clamp day if the target month is shorter (e.g. Jan 31 -> Feb 28).
            let max_day = days_in_month(dt.year, dt.month);
            if dt.day > max_day {
                dt.day = max_day;
            }
            datetime_to_epoch(&dt).map_err(Error::TimeParse)
        }
        _ => Err(Error::TimeParse(format!(
            "unknown unit after 'next': '{unit}' (expected 'week' or 'month')"
        ))),
    }
}

/// Advance a DateTime by one day, handling month/year rollover.
fn advance_day(dt: &mut DateTime) {
    let max_day = days_in_month(dt.year, dt.month);
    if dt.day < max_day {
        dt.day += 1;
    } else if dt.month < 12 {
        dt.month += 1;
        dt.day = 1;
    } else {
        dt.year += 1;
        dt.month = 1;
        dt.day = 1;
    }
}

// ============================================================================
// User / UID helpers
// ============================================================================

/// The caller's login name, or `None` if this build cannot determine it.
///
/// # Why not `$USER`
///
/// It read `$USER`, then `$LOGNAME`, then a pid. Both variables are the
/// caller's to set, and this name is written into the job file as the
/// SUBMITTER, then compared by [`atd_may_run`] against the daemon's own name
/// before a job runs. Both sides of that comparison came from here, so the
/// guard weighed two strings the caller could influence rather than two
/// identities.
///
/// # And the fallback was a different bug
///
/// `format!("uid{}", process::id())` is the PROCESS ID, not the uid, under a
/// doc comment saying "from environment". A pid is unique per invocation, so
/// it is not an identity at all: a job queued by a process with pid 4123 is
/// recorded as `uid4123`, and the daemon comparing its own `uid5678` refuses
/// it forever. With `$USER` unset -- which is how a daemon started by init
/// runs -- `at` therefore refused every job it had itself accepted.
///
/// `userspace/crontab` had this exact pair, wording and all, and its repair
/// note is what made this one recognisable. The uid fallback here is the REAL
/// uid, which is stable across invocations and is the property the original
/// was reaching for.
fn current_username() -> Option<String> {
    let uid = authlib::identity::caller_uid()?;
    let named = userdb::UserDb::load(userdb::DEFAULT_PATH)
        .ok()
        .and_then(|db| db.find_uid(uid).and_then(userdb::Record::username));
    Some(named.unwrap_or_else(|| format!("uid{uid}")))
}

// ============================================================================
// Job file management
// ============================================================================

/// A parsed at-job read from the spool directory.
#[derive(Debug)]
struct Job {
    id: u32,
    queue: char,
    epoch: i64,
    user: String,
    created: i64,
    commands: String,
}

/// The file name format for jobs: `a<queue><5-digit-id>`.
///
/// Examples: `aa00001`, `ab00042`, `aa99999`.
fn job_filename(queue: char, id: u32) -> String {
    format!("a{queue}{id:05}")
}

/// Parse a job filename into (queue, id). Returns None if the name doesn't
/// match the expected pattern.
fn parse_job_filename(name: &str) -> Option<(char, u32)> {
    let bytes = name.as_bytes();
    // Must be "a" + queue char + 5 digits = 7 bytes.
    if bytes.len() != 7 || bytes[0] != b'a' {
        return None;
    }
    let queue = bytes[1] as char;
    if !queue.is_ascii_lowercase() {
        return None;
    }
    let id_str = &name[2..];
    let id: u32 = id_str.parse().ok()?;
    Some((queue, id))
}

/// Build the header for a job file.
fn build_job_header(epoch: i64, queue: char, user: &str, created: i64) -> String {
    format!(
        "# at job\n\
         # time: {epoch}\n\
         # queue: {queue}\n\
         # user: {user}\n\
         # created: {created}\n"
    )
}

/// Parse a job file's header. Returns (epoch, queue, user, created, commands)
/// or None if the header is malformed.
fn parse_job_file(content: &str) -> Option<(i64, char, String, i64, String)> {
    let mut epoch: Option<i64> = None;
    let mut queue: Option<char> = None;
    let mut user: Option<String> = None;
    let mut created: Option<i64> = None;
    // `None` until we encounter the first non-header line. Leaving it unset
    // distinguishes "header-only file (no commands)" from "commands begin at
    // byte 0" — without this, a job file with no command body would echo the
    // entire header back as its command text.
    let mut command_start: Option<usize> = None;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "# at job" {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# time: ") {
            epoch = rest.trim().parse().ok();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# queue: ") {
            let q = rest.trim();
            if q.len() == 1 {
                queue = q.chars().next();
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# user: ") {
            user = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# created: ") {
            created = rest.trim().parse().ok();
            continue;
        }
        // First non-header line marks the start of commands.
        // Calculate byte offset to this line.
        command_start = Some(
            content
                .lines()
                .take(idx)
                .map(|l| l.len() + 1) // +1 for newline
                .sum(),
        );
        break;
    }

    let commands = match command_start {
        Some(start) if start < content.len() => content[start..].to_string(),
        _ => String::new(),
    };

    Some((
        epoch.unwrap_or(0),
        queue.unwrap_or(DEFAULT_QUEUE),
        user.unwrap_or_default(),
        created.unwrap_or(0),
        commands,
    ))
}

/// Read all jobs from the spool directory.
fn read_all_jobs() -> Result<Vec<Job>, Error> {
    let entries = match fs::read_dir(SPOOL_DIR) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(Error::Io(format!(
                "cannot read spool directory {SPOOL_DIR}: {e}"
            )));
        }
    };

    let mut jobs = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };

        let (queue, id) = match parse_job_filename(&name_str) {
            Some(pair) => pair,
            None => continue,
        };

        let content = match fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some((epoch, _q, user, created, commands)) = parse_job_file(&content) {
            jobs.push(Job {
                id,
                queue,
                epoch,
                user,
                created,
                commands,
            });
        }
    }

    jobs.sort_by_key(|j| j.id);
    Ok(jobs)
}

/// Find the next available job ID by scanning the spool directory.
fn next_job_id() -> Result<u32, Error> {
    let jobs = read_all_jobs()?;
    let max_id = jobs.iter().map(|j| j.id).max().unwrap_or(0);
    Ok(max_id + 1)
}

/// Ensure the spool directory exists.
fn ensure_spool_dir() -> Result<(), Error> {
    fs::create_dir_all(SPOOL_DIR)
        .map_err(|e| Error::Io(format!("cannot create spool directory {SPOOL_DIR}: {e}")))
}

// ============================================================================
// Commands
// ============================================================================

/// Schedule a new job: read commands from the given source, write to spool.
fn cmd_schedule(
    commands: &str,
    target_epoch: i64,
    queue: char,
    mail: bool,
    username: &str,
    now_epoch: i64,
) -> Result<(), Error> {
    ensure_spool_dir()?;

    let id = next_job_id()?;
    let filename = job_filename(queue, id);
    let path = PathBuf::from(SPOOL_DIR).join(&filename);

    let mut header = build_job_header(target_epoch, queue, username, now_epoch);
    if mail {
        header.push_str("# mail: yes\n");
    }
    let content = format!("{header}{commands}");

    fs::write(&path, &content)
        .map_err(|e| Error::Io(format!("cannot write job file {}: {e}", path.display())))?;

    let dt = epoch_to_datetime(target_epoch);
    eprintln!("job {id} at {}", format_datetime(&dt));

    Ok(())
}

/// Schedule a batch job (runs when load drops below threshold).
fn cmd_batch(
    commands: &str,
    queue: char,
    mail: bool,
    username: &str,
    now_epoch: i64,
) -> Result<(), Error> {
    ensure_spool_dir()?;

    let id = next_job_id()?;
    let filename = job_filename(queue, id);
    let path = PathBuf::from(SPOOL_DIR).join(&filename);

    let mut header = build_job_header(now_epoch, queue, username, now_epoch);
    header.push_str("# batch: yes\n");
    if mail {
        header.push_str("# mail: yes\n");
    }
    let content = format!("{header}{commands}");

    fs::write(&path, &content)
        .map_err(|e| Error::Io(format!("cannot write job file {}: {e}", path.display())))?;

    eprintln!("job {id} (batch) scheduled");

    Ok(())
}

/// List all pending jobs.
fn cmd_list(json: bool) -> Result<(), Error> {
    let jobs = read_all_jobs()?;

    if json {
        print_jobs_json(&jobs);
    } else {
        print_jobs_table(&jobs);
    }

    Ok(())
}

/// Print jobs as a human-readable table.
fn print_jobs_table(jobs: &[Job]) {
    if jobs.is_empty() {
        println!("no pending jobs");
        return;
    }
    println!("{:<8}{:<7}{:<25}User", "Job ID", "Queue", "When");
    for job in jobs {
        let dt = epoch_to_datetime(job.epoch);
        println!(
            "{:<8}{:<7}{:<25}{}",
            job.id,
            job.queue,
            format_datetime(&dt),
            job.user
        );
    }
}

/// Print jobs as JSON.
fn print_jobs_json(jobs: &[Job]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = out.write_all(b"[\n");
    for (i, job) in jobs.iter().enumerate() {
        let dt = epoch_to_datetime(job.epoch);
        let when = format_datetime(&dt);
        // Escape user and when strings for JSON (basic: replace " and \).
        let user_esc = json_escape(&job.user);
        let when_esc = json_escape(&when);

        let _ = write!(
            out,
            "  {{\"id\":{},\"queue\":\"{}\",\"when\":\"{}\",\"epoch\":{},\"created\":{},\"user\":\"{}\"}}",
            job.id, job.queue, when_esc, job.epoch, job.created, user_esc
        );
        if i + 1 < jobs.len() {
            let _ = out.write_all(b",\n");
        } else {
            let _ = out.write_all(b"\n");
        }
    }
    let _ = out.write_all(b"]\n");
}

/// Minimal JSON string escaping.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                // Unicode escape for other control chars.
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Remove a job by ID.
fn cmd_remove(id: u32) -> Result<(), Error> {
    let jobs = read_all_jobs()?;
    let job = jobs.iter().find(|j| j.id == id);

    match job {
        Some(j) => {
            let filename = job_filename(j.queue, j.id);
            let path = PathBuf::from(SPOOL_DIR).join(&filename);
            fs::remove_file(&path).map_err(|e| {
                Error::Io(format!("cannot remove job file {}: {e}", path.display()))
            })?;
            eprintln!("job {id} removed");
            Ok(())
        }
        None => Err(Error::NotFound(format!("job {id} not found"))),
    }
}

/// Display the commands stored in a job.
fn cmd_cat(id: u32) -> Result<(), Error> {
    let jobs = read_all_jobs()?;
    let job = jobs.iter().find(|j| j.id == id);

    match job {
        Some(j) => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            let _ = out.write_all(j.commands.as_bytes());
            if !j.commands.ends_with('\n') && !j.commands.is_empty() {
                let _ = out.write_all(b"\n");
            }
            Ok(())
        }
        None => Err(Error::NotFound(format!("job {id} not found"))),
    }
}

/// Display the time a job would run without actually scheduling it.
fn cmd_verify(target_epoch: i64) {
    let dt = epoch_to_datetime(target_epoch);
    println!("{}", format_datetime(&dt));
}

/// Read commands from stdin until EOF.
fn read_commands_stdin() -> Result<String, Error> {
    eprintln!("at> (type commands, press Ctrl+D when done)");
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| Error::Io(format!("cannot read from stdin: {e}")))?;
    if buf.is_empty() {
        return Err(Error::Usage("no commands provided".into()));
    }
    Ok(buf)
}

/// Read commands from a file.
fn read_commands_file(path: &str) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|e| Error::Io(format!("cannot read '{}': {e}", path)))
}

// ============================================================================
// Argument parsing
// ============================================================================

/// The mode of invocation determined from argv[0].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvokedAs {
    At,
    Atq,
    Atrm,
    Batch,
    /// The daemon that runs what the others spooled.
    ///
    /// It lives in this crate because this crate owns the spool format. A
    /// drain living elsewhere would be a second reader of a format with one
    /// writer, which is how `crond`'s /etc/cron.d parser came to disagree with
    /// the files it was reading.
    Atd,
}

/// Parsed command-line arguments.
struct Args {
    action: Action,
    queue: char,
    mail: bool,
    json: bool,
}

/// What the user wants to do.
enum Action {
    /// Schedule commands from stdin at the given timespec.
    Schedule(String),
    /// Schedule commands from a file.
    ScheduleFile(String, String),
    /// List pending jobs.
    List,
    /// Remove a job by ID.
    Remove(u32),
    /// Display job contents.
    Cat(u32),
    /// Display the time without scheduling (verify mode).
    Verify(String),
    /// Batch: schedule for low-load execution, commands from stdin.
    Batch,
    /// Show help.
    Help,
    /// Run the daemon. `true` drains once and returns, for tests and for a
    /// caller that wants a single sweep rather than a resident process.
    Daemon(bool),
}

/// Detect how the binary was invoked by examining argv[0].
fn detect_invocation(argv0: &str) -> InvokedAs {
    let basename = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(argv0);

    match basename {
        "atq" => InvokedAs::Atq,
        "atrm" => InvokedAs::Atrm,
        "batch" => InvokedAs::Batch,
        "atd" => InvokedAs::Atd,
        _ => InvokedAs::At,
    }
}

fn parse_args() -> Result<Args, Error> {
    let argv: Vec<String> = env::args().collect();
    let invoked = if argv.is_empty() {
        InvokedAs::At
    } else {
        detect_invocation(&argv[0])
    };

    let argc = argv.len();
    let mut queue = DEFAULT_QUEUE;
    let mut mail = false;
    let mut json = false;

    match invoked {
        InvokedAs::Atd => {
            // atd [--once]
            let once = argv[1..].iter().any(|a| a == "--once");
            return Ok(Args {
                action: Action::Daemon(once),
                queue,
                mail,
                json,
            });
        }
        InvokedAs::Atq => {
            // atq [--json]
            for arg in &argv[1..] {
                if arg == "--json" {
                    json = true;
                }
            }
            return Ok(Args {
                action: Action::List,
                queue,
                mail,
                json,
            });
        }
        InvokedAs::Atrm => {
            // atrm <jobid> [<jobid>...]
            if argc < 2 {
                return Err(Error::Usage("atrm: missing job ID".into()));
            }
            let id: u32 = argv[1]
                .parse()
                .map_err(|_| Error::Usage(format!("atrm: invalid job ID: '{}'", argv[1])))?;
            return Ok(Args {
                action: Action::Remove(id),
                queue,
                mail,
                json,
            });
        }
        InvokedAs::Batch => {
            return Ok(Args {
                action: Action::Batch,
                queue,
                mail,
                json,
            });
        }
        InvokedAs::At => {} // fall through to full parsing
    }

    // No arguments at all: help.
    if argc < 2 {
        return Ok(Args {
            action: Action::Help,
            queue,
            mail,
            json,
        });
    }

    let mut action: Option<Action> = None;
    let mut file_path: Option<String> = None;
    let mut verify = false;
    let mut timespec_parts: Vec<String> = Vec::new();
    let mut i = 1;

    while i < argc {
        match argv[i].as_str() {
            "-h" | "--help" | "help" => {
                return Ok(Args {
                    action: Action::Help,
                    queue,
                    mail,
                    json,
                });
            }
            "-l" | "--list" => {
                action = Some(Action::List);
                i += 1;
            }
            "-d" | "--delete" => {
                if i + 1 >= argc {
                    return Err(Error::Usage("-d requires a job ID".into()));
                }
                let id: u32 = argv[i + 1]
                    .parse()
                    .map_err(|_| Error::Usage(format!("invalid job ID: '{}'", argv[i + 1])))?;
                action = Some(Action::Remove(id));
                i += 2;
            }
            "-c" | "--cat" => {
                if i + 1 >= argc {
                    return Err(Error::Usage("-c requires a job ID".into()));
                }
                let id: u32 = argv[i + 1]
                    .parse()
                    .map_err(|_| Error::Usage(format!("invalid job ID: '{}'", argv[i + 1])))?;
                action = Some(Action::Cat(id));
                i += 2;
            }
            "-f" | "--file" => {
                if i + 1 >= argc {
                    return Err(Error::Usage("-f requires a file path".into()));
                }
                file_path = Some(argv[i + 1].clone());
                i += 2;
            }
            "-v" | "--verify" => {
                verify = true;
                i += 1;
            }
            "-b" | "--batch" => {
                action = Some(Action::Batch);
                i += 1;
            }
            "-q" | "--queue" => {
                if i + 1 >= argc {
                    return Err(Error::Usage("-q requires a queue letter (a-z)".into()));
                }
                let q = argv[i + 1].trim();
                if q.len() != 1 || !q.as_bytes()[0].is_ascii_lowercase() {
                    return Err(Error::Usage(format!(
                        "invalid queue letter: '{}' (must be a-z)",
                        argv[i + 1]
                    )));
                }
                queue = q.as_bytes()[0] as char;
                i += 2;
            }
            "-m" | "--mail" => {
                mail = true;
                i += 1;
            }
            "--json" => {
                json = true;
                i += 1;
            }
            _ => {
                // Collect remaining arguments as timespec.
                timespec_parts.push(argv[i].clone());
                i += 1;
            }
        }
    }

    // If an explicit action was set (list, remove, cat, batch), return it.
    if let Some(a) = action {
        return Ok(Args {
            action: a,
            queue,
            mail,
            json,
        });
    }

    // Otherwise, we need a timespec for scheduling or verification.
    if timespec_parts.is_empty() {
        return Ok(Args {
            action: Action::Help,
            queue,
            mail,
            json,
        });
    }

    let timespec = timespec_parts.join(" ");

    if verify {
        return Ok(Args {
            action: Action::Verify(timespec),
            queue,
            mail,
            json,
        });
    }

    if let Some(fp) = file_path {
        return Ok(Args {
            action: Action::ScheduleFile(fp, timespec),
            queue,
            mail,
            json,
        });
    }

    Ok(Args {
        action: Action::Schedule(timespec),
        queue,
        mail,
        json,
    })
}

// ============================================================================
// Help
// ============================================================================

fn print_usage() {
    eprintln!("at (Slate OS) v0.1.0 -- schedule one-time commands");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  at <timespec>                Schedule commands from stdin");
    eprintln!("  at -f <file> <timespec>      Schedule commands from file");
    eprintln!("  at -l [--json]               List pending jobs (atq)");
    eprintln!("  at -d <jobid>                Remove a job (atrm)");
    eprintln!("  at -c <jobid>                Display job commands");
    eprintln!("  at -v <timespec>             Show when job would run");
    eprintln!("  at -b                        Schedule for low load (batch)");
    eprintln!("  at -q <queue> <timespec>     Use queue letter (a-z)");
    eprintln!("  at -m <timespec>             Mail output to user");
    eprintln!();
    eprintln!("SYMLINK ALIASES:");
    eprintln!("  atq                          Same as 'at -l'");
    eprintln!("  atrm <jobid>                 Same as 'at -d <jobid>'");
    eprintln!("  batch                        Same as 'at -b'");
    eprintln!();
    eprintln!("TIME SPECIFICATIONS:");
    eprintln!("  HH:MM                        Today (or tomorrow if past)");
    eprintln!("  HH:MM YYYY-MM-DD             Specific date");
    eprintln!("  HH:MM tomorrow               Tomorrow at given time");
    eprintln!("  now + N minutes/hours/days/weeks");
    eprintln!("  noon, midnight, teatime      Named times (today/tomorrow)");
    eprintln!("  noon tomorrow                Named time + tomorrow");
    eprintln!("  tomorrow                     24 hours from now");
    eprintln!("  next week, next month        Relative");
    eprintln!();
    eprintln!("JOB STORAGE:");
    eprintln!("  Jobs stored in {SPOOL_DIR}/");
    eprintln!("  Execution handled by atd daemon.");
}

// ============================================================================
// Entry point
// ============================================================================

// ============================================================================
// atd -- the daemon that runs what `at` spooled
// ============================================================================

/// How often the daemon looks at the spool.
///
/// at(1) resolves to the minute, so a shorter period buys nothing and a longer
/// one makes a job late by up to its own length.
const ATD_POLL_SECS: u64 = 60;

/// Whether a job submitted by `want` may be run by a process that is
/// `running_as`.
///
/// # Why a job can be refused rather than run
///
/// A job records the user who submitted it, and this daemon cannot change
/// user: there is no `setuid`, `setgid` or `setgroups` in this crate or in any
/// of the other four cron crates. Running one user's job as another gives it
/// authority its submitter never had -- and if this daemon runs as root, that
/// is every authority on the machine.
///
/// `design-decisions.md` 1019: ask what the caller loses if we proceed. Here
/// they lose the containment implied by having submitted the job as
/// themselves, so it is the refuse case. `crond` was given the
/// same rule the same day, deliberately worded alike.
///
/// # The unknown case fails closed
///
/// If we cannot tell who we are, we cannot tell whether we are the submitter.
/// "I do not know" must not be worth the same as "yes".
fn atd_may_run(want: &str, running_as: Option<&str>) -> Result<(), String> {
    match running_as {
        Some(me) if me == want => Ok(()),
        Some(me) => Err(format!(
            "submitted by {want:?}, atd is running as {me:?}, and it cannot \
             change user -- running the job would give it authority its \
             submitter did not have"
        )),
        None => Err(format!(
            "submitted by {want:?} and the current user is unknown, so it \
             cannot be confirmed -- refusing rather than guessing"
        )),
    }
}

/// The jobs whose time has come, oldest first.
///
/// Split out so the selection can be tested without a clock, a spool or a
/// shell. Ordering matters: two jobs due in the same tick should run in the
/// order they were scheduled for, and `at` guarantees no more than that.
fn atd_due(jobs: &[Job], now: i64) -> Vec<&Job> {
    let mut due: Vec<&Job> = jobs.iter().filter(|j| j.epoch <= now).collect();
    due.sort_by_key(|j| (j.epoch, j.id));
    due
}

/// Run one job, and say whether its spool file should now be removed.
///
/// `false` for a refusal, which is the one case that must NOT delete: the user
/// was told the job was scheduled, and discarding it because we could not run
/// it destroys their work to tidy up after our own limitation. It stays in the
/// spool where `atq` shows it and `atrm` can remove it.
fn atd_run_one(job: &Job, running_as: Option<&str>) -> bool {
    if let Err(why) = atd_may_run(&job.user, running_as) {
        eprintln!("atd: REFUSING job {}: {why}", job.id);
        return false;
    }

    eprintln!("atd: running job {} for {}", job.id, job.user);
    let status = process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&job.commands)
        .status();

    match status {
        Ok(st) => {
            eprintln!("atd: job {} exited {}", job.id, st.code().unwrap_or(-1));
            // Removed whatever the exit status: at(1) runs a job once. A job
            // that failed has still run, and keeping it would run it again
            // every minute forever.
            true
        }
        Err(e) => {
            // The shell could not be started at all, so the job did NOT run.
            // Keeping it is right for the same reason a refusal keeps it.
            eprintln!("atd: job {} could not be started: {e}", job.id);
            false
        }
    }
}

/// The daemon loop.
fn cmd_atd(once: bool) -> Result<(), Error> {
    ensure_spool_dir()?;
    // `None` here is already the fail-closed case in `atd_may_run`: a daemon
    // that cannot tell who it is cannot tell whether it is a job's submitter.
    let me = current_username();
    eprintln!(
        "atd: watching {SPOOL_DIR} every {ATD_POLL_SECS}s as {}",
        me.as_deref().unwrap_or("<unknown user>")
    );

    loop {
        let now = get_current_time().map_err(Error::Io)?;
        let jobs = read_all_jobs()?;
        for job in atd_due(&jobs, now) {
            if atd_run_one(job, me.as_deref()) {
                let path = Path::new(SPOOL_DIR).join(job_filename(job.queue, job.id));
                if let Err(e) = fs::remove_file(&path) {
                    // Named rather than swallowed: a job that ran and whose
                    // file survives will run again next tick.
                    eprintln!(
                        "atd: job {} ran but {} remains: {e}",
                        job.id,
                        path.display()
                    );
                }
            }
        }
        if once {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(ATD_POLL_SECS));
    }
}

/// The caller's name, or a refusal.
///
/// Queueing a job writes this into the job file as the submitter, and
/// `atd_may_run` later refuses to run a job whose submitter is not the
/// daemon's own user. A job filed under a name we had to guess is one that
/// either never runs or runs for the wrong person, so not knowing is a
/// refusal at submission time rather than a surprise at run time.
fn require_username() -> Result<String, Error> {
    current_username().ok_or_else(|| {
        Error::Io(
            "cannot determine who you are, and a job records its submitter; refusing".to_string(),
        )
    })
}

fn run() -> Result<(), Error> {
    let args = parse_args()?;

    match args.action {
        Action::Help => {
            print_usage();
            Ok(())
        }
        Action::Daemon(once) => cmd_atd(once),
        Action::List => cmd_list(args.json),
        Action::Remove(id) => cmd_remove(id),
        Action::Cat(id) => cmd_cat(id),
        Action::Verify(ref timespec) => {
            let now = get_current_time().map_err(Error::Io)?;
            let target = parse_timespec(timespec, now)?;
            cmd_verify(target);
            Ok(())
        }
        Action::Schedule(ref timespec) => {
            let now = get_current_time().map_err(Error::Io)?;
            let target = parse_timespec(timespec, now)?;
            let commands = read_commands_stdin()?;
            cmd_schedule(
                &commands,
                target,
                args.queue,
                args.mail,
                &require_username()?,
                now,
            )
        }
        Action::ScheduleFile(ref path, ref timespec) => {
            let now = get_current_time().map_err(Error::Io)?;
            let target = parse_timespec(timespec, now)?;
            let commands = read_commands_file(path)?;
            cmd_schedule(
                &commands,
                target,
                args.queue,
                args.mail,
                &require_username()?,
                now,
            )
        }
        Action::Batch => {
            let now = get_current_time().map_err(Error::Io)?;
            let commands = read_commands_stdin()?;
            cmd_batch(&commands, args.queue, args.mail, &require_username()?, now)
        }
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("at: {e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
// Panicking on bad data is what a test is for; CLAUDE.md allows these
// inside `#[cfg(test)]` and requires them everywhere else.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn day_of_week_survives_a_month_outside_the_calendar() {
        // The version this replaced indexed `T[(m - 1) as usize]` with no
        // range guard, so month 0 became `(-1) as usize` -- usize::MAX -- and
        // panicked. `month` is a u32 straight off a parsed timespec, so the
        // guard was the caller's job and no caller did it. The function
        // immediately above it DID guard its range, which is what makes this
        // the kind of defect a reader skims past.
        //
        // Found by `clippy::indexing_slicing`, which this crate was not
        // subject to until today: it is one of the 134 that inherited neither
        // `[workspace.lints]` nor the inner attribute.
        for m in [0_u32, 13, 99, u32::MAX] {
            let _ = day_of_week(2026, m, 1);
        }
        for m in [0_u32, 13, 99, u32::MAX] {
            assert_eq!(days_in_month(2026, m), 0);
        }
    }

    #[test]
    fn day_of_week_still_agrees_with_the_calendar() {
        // 2026-09-10 is a Thursday; ISO counts Monday as 0, so Thursday is 3.
        assert_eq!(day_of_week(2026, 9, 10), 3);
        // 1970-01-01 was a Thursday.
        assert_eq!(day_of_week(1970, 1, 1), 3);
        // A year outside i32 is not a date; answering 0 beats answering about
        // a different year, which `as i32` would have done silently.
        assert_eq!(day_of_week(i64::MAX, 9, 10), 0);
    }

    // -- Calendar math tests --

    #[test]
    fn leap_year() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn days_in_month_standard() {
        assert_eq!(days_in_month(2023, 1), 31);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 12), 31);
    }

    #[test]
    fn days_in_month_invalid() {
        assert_eq!(days_in_month(2023, 0), 0);
        assert_eq!(days_in_month(2023, 13), 0);
    }

    #[test]
    fn epoch_roundtrip() {
        let test_epochs: &[i64] = &[
            0,
            86400,
            1_704_067_200, // 2024-01-01
            1_747_491_045, // 2025-05-17 14:10:45 UTC
            946_684_800,   // 2000-01-01
        ];
        for &epoch in test_epochs {
            let dt = epoch_to_datetime(epoch);
            let back = datetime_to_epoch(&dt).unwrap();
            assert_eq!(epoch, back, "round-trip failed for epoch {epoch}");
        }
    }

    #[test]
    fn epoch_to_datetime_unix_epoch() {
        let dt = epoch_to_datetime(0);
        assert_eq!(dt.year, 1970);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 1);
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.second, 0);
    }

    #[test]
    fn epoch_to_datetime_known() {
        // 2025-05-17 14:30:45 UTC
        let dt = epoch_to_datetime(1_747_492_245);
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 5);
        assert_eq!(dt.day, 17);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
        assert_eq!(dt.second, 45);
    }

    #[test]
    fn datetime_to_epoch_validation() {
        // Bad month.
        let dt = DateTime {
            year: 2025,
            month: 13,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert!(datetime_to_epoch(&dt).is_err());

        // Bad day.
        let dt = DateTime {
            year: 2023,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert!(datetime_to_epoch(&dt).is_err());

        // Bad hour.
        let dt = DateTime {
            year: 2025,
            month: 1,
            day: 1,
            hour: 24,
            minute: 0,
            second: 0,
        };
        assert!(datetime_to_epoch(&dt).is_err());
    }

    #[test]
    fn day_of_week_known_dates() {
        // 2024-01-01 was Monday.
        assert_eq!(day_of_week(2024, 1, 1), 0);
        // 1970-01-01 was Thursday.
        assert_eq!(day_of_week(1970, 1, 1), 3);
        // 2025-05-17 is Saturday.
        assert_eq!(day_of_week(2025, 5, 17), 5);
    }

    // -- advance_day tests --

    #[test]
    fn advance_day_normal() {
        let mut dt = DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        };
        advance_day(&mut dt);
        assert_eq!((dt.year, dt.month, dt.day), (2025, 5, 18));
    }

    #[test]
    fn advance_day_month_boundary() {
        let mut dt = DateTime {
            year: 2025,
            month: 5,
            day: 31,
            hour: 10,
            minute: 0,
            second: 0,
        };
        advance_day(&mut dt);
        assert_eq!((dt.year, dt.month, dt.day), (2025, 6, 1));
    }

    #[test]
    fn advance_day_year_boundary() {
        let mut dt = DateTime {
            year: 2025,
            month: 12,
            day: 31,
            hour: 10,
            minute: 0,
            second: 0,
        };
        advance_day(&mut dt);
        assert_eq!((dt.year, dt.month, dt.day), (2026, 1, 1));
    }

    #[test]
    fn advance_day_leap_february() {
        let mut dt = DateTime {
            year: 2024,
            month: 2,
            day: 28,
            hour: 10,
            minute: 0,
            second: 0,
        };
        advance_day(&mut dt);
        assert_eq!((dt.year, dt.month, dt.day), (2024, 2, 29));

        advance_day(&mut dt);
        assert_eq!((dt.year, dt.month, dt.day), (2024, 3, 1));
    }

    // -- format_datetime tests --

    #[test]
    fn format_datetime_display() {
        let dt = epoch_to_datetime(1_747_492_245);
        let formatted = format_datetime(&dt);
        assert_eq!(formatted, "Sat May 17 14:30:45 2025");
    }

    // -- parse_hhmm_pair tests --

    #[test]
    fn parse_hhmm_pair_valid() {
        assert_eq!(parse_hhmm_pair("14:30"), Some((14, 30)));
        assert_eq!(parse_hhmm_pair("00:00"), Some((0, 0)));
        assert_eq!(parse_hhmm_pair("23:59"), Some((23, 59)));
    }

    #[test]
    fn parse_hhmm_pair_invalid() {
        assert_eq!(parse_hhmm_pair("24:00"), None);
        assert_eq!(parse_hhmm_pair("12:60"), None);
        assert_eq!(parse_hhmm_pair("abc"), None);
        assert_eq!(parse_hhmm_pair("12:"), None);
        assert_eq!(parse_hhmm_pair(":30"), None);
    }

    // -- parse_timespec tests --

    #[test]
    fn timespec_hhmm_future_today() {
        // "now" is 2025-05-17 10:00:00; schedule for 14:30 should be same day.
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("14:30", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 5);
        assert_eq!(dt.day, 17);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
    }

    #[test]
    fn timespec_hhmm_past_rolls_to_tomorrow() {
        // "now" is 2025-05-17 16:00:00; schedule for 14:30 should roll to 18th.
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 16,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("14:30", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 5);
        assert_eq!(dt.day, 18);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
    }

    #[test]
    fn timespec_hhmm_date() {
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("09:00 2025-06-01", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 6);
        assert_eq!(dt.day, 1);
        assert_eq!(dt.hour, 9);
        assert_eq!(dt.minute, 0);
    }

    #[test]
    fn timespec_noon() {
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("noon", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.hour, 12);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.day, 17);
    }

    #[test]
    fn timespec_midnight_rolls() {
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        })
        .unwrap();
        // Midnight at 00:00 has already passed today, should roll to 18th.
        let target = parse_timespec("midnight", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.day, 18);
    }

    #[test]
    fn timespec_teatime() {
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("teatime", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.hour, 16);
        assert_eq!(dt.minute, 0);
    }

    #[test]
    fn timespec_tomorrow() {
        let now = 1_747_491_045_i64; // 2025-05-17 14:10:45 UTC
        let target = parse_timespec("tomorrow", now).unwrap();
        assert_eq!(target, now + SECS_PER_DAY);
    }

    #[test]
    fn timespec_relative_minutes() {
        let now = 1_747_491_045_i64;
        let target = parse_timespec("now + 30 minutes", now).unwrap();
        assert_eq!(target, now + 30 * 60);
    }

    #[test]
    fn timespec_relative_hours() {
        let now = 1_747_491_045_i64;
        let target = parse_timespec("now + 2 hours", now).unwrap();
        assert_eq!(target, now + 2 * 3600);
    }

    #[test]
    fn timespec_relative_days() {
        let now = 1_747_491_045_i64;
        let target = parse_timespec("now + 3 days", now).unwrap();
        assert_eq!(target, now + 3 * SECS_PER_DAY);
    }

    #[test]
    fn timespec_relative_weeks() {
        let now = 1_747_491_045_i64;
        let target = parse_timespec("now + 1 week", now).unwrap();
        assert_eq!(target, now + 7 * SECS_PER_DAY);
    }

    #[test]
    fn timespec_next_week() {
        let now = 1_747_491_045_i64;
        let target = parse_timespec("next week", now).unwrap();
        assert_eq!(target, now + 7 * SECS_PER_DAY);
    }

    #[test]
    fn timespec_next_month() {
        // 2025-05-17 14:30:45 -> 2025-06-17 14:30:45
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 14,
            minute: 30,
            second: 45,
        })
        .unwrap();
        let target = parse_timespec("next month", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 6);
        assert_eq!(dt.day, 17);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
    }

    #[test]
    fn timespec_next_month_clamping() {
        // 2025-01-31 -> next month should clamp to Feb 28.
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 1,
            day: 31,
            hour: 12,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("next month", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.month, 2);
        assert_eq!(dt.day, 28); // 2025 is not a leap year
    }

    #[test]
    fn timespec_hhmm_tomorrow() {
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("14:30 tomorrow", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.day, 18);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
    }

    #[test]
    fn timespec_noon_tomorrow() {
        let now = datetime_to_epoch(&DateTime {
            year: 2025,
            month: 5,
            day: 17,
            hour: 10,
            minute: 0,
            second: 0,
        })
        .unwrap();
        let target = parse_timespec("noon tomorrow", now).unwrap();
        let dt = epoch_to_datetime(target);
        assert_eq!(dt.day, 18);
        assert_eq!(dt.hour, 12);
        assert_eq!(dt.minute, 0);
    }

    #[test]
    fn timespec_invalid() {
        assert!(parse_timespec("", 0).is_err());
        assert!(parse_timespec("gibberish nonsense", 0).is_err());
        assert!(parse_timespec("now + -5 minutes", 0).is_err());
    }

    #[test]
    fn timespec_relative_singular() {
        // "now + 1 minute" (singular form)
        let now = 1_000_000_i64;
        let target = parse_timespec("now + 1 minute", now).unwrap();
        assert_eq!(target, now + 60);
    }

    // -- Job filename tests --

    #[test]
    fn job_filename_format() {
        assert_eq!(job_filename('a', 1), "aa00001");
        assert_eq!(job_filename('b', 42), "ab00042");
        assert_eq!(job_filename('z', 99999), "az99999");
    }

    #[test]
    fn parse_job_filename_valid() {
        assert_eq!(parse_job_filename("aa00001"), Some(('a', 1)));
        assert_eq!(parse_job_filename("ab00042"), Some(('b', 42)));
        assert_eq!(parse_job_filename("az99999"), Some(('z', 99999)));
    }

    #[test]
    fn parse_job_filename_invalid() {
        assert_eq!(parse_job_filename(""), None);
        assert_eq!(parse_job_filename("b00001"), None); // no leading 'a'
        assert_eq!(parse_job_filename("aA00001"), None); // uppercase queue
        assert_eq!(parse_job_filename("aa0001"), None); // too short
        assert_eq!(parse_job_filename("aa000001"), None); // too long
        assert_eq!(parse_job_filename("aahello"), None); // non-numeric
    }

    // -- Job file header tests --

    #[test]
    fn build_and_parse_job_header() {
        let header = build_job_header(1_747_491_045, 'a', "root", 1_747_400_000);
        let content = format!("{header}/bin/echo hello\n/bin/cleanup\n");

        let (epoch, queue, user, created, commands) = parse_job_file(&content).unwrap();
        assert_eq!(epoch, 1_747_491_045);
        assert_eq!(queue, 'a');
        assert_eq!(user, "root");
        assert_eq!(created, 1_747_400_000);
        assert_eq!(commands, "/bin/echo hello\n/bin/cleanup\n");
    }

    #[test]
    fn parse_job_file_empty_commands() {
        let header = build_job_header(100, 'b', "user", 50);
        let (epoch, queue, user, created, commands) = parse_job_file(&header).unwrap();
        assert_eq!(epoch, 100);
        assert_eq!(queue, 'b');
        assert_eq!(user, "user");
        assert_eq!(created, 50);
        assert!(commands.is_empty());
    }

    // -- JSON escaping tests --

    #[test]
    fn json_escape_plain() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn json_escape_special() {
        assert_eq!(json_escape("he\"llo"), "he\\\"llo");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
    }

    // -- Invocation detection tests --

    #[test]
    fn detect_invocation_at() {
        assert_eq!(detect_invocation("at"), InvokedAs::At);
        assert_eq!(detect_invocation("/usr/bin/at"), InvokedAs::At);
    }

    #[test]
    fn detect_invocation_atq() {
        assert_eq!(detect_invocation("atq"), InvokedAs::Atq);
        assert_eq!(detect_invocation("/usr/bin/atq"), InvokedAs::Atq);
    }

    #[test]
    fn detect_invocation_atrm() {
        assert_eq!(detect_invocation("atrm"), InvokedAs::Atrm);
        assert_eq!(detect_invocation("/bin/atrm"), InvokedAs::Atrm);
    }

    #[test]
    fn detect_invocation_batch() {
        assert_eq!(detect_invocation("batch"), InvokedAs::Batch);
        assert_eq!(detect_invocation("atd"), InvokedAs::Atd);
        assert_eq!(detect_invocation("/usr/sbin/atd"), InvokedAs::Atd);
    }

    // ---- atd: the spool now has a drain ----------------------------------

    /// `$USER` must not name the submitter a job is filed under.
    ///
    /// The name is written into the job file and `atd_may_run` compares it
    /// against the daemon's own before running anything -- and BOTH sides came
    /// from this lookup, so the guard weighed two strings the caller could
    /// set rather than two identities.
    #[test]
    fn the_environment_cannot_name_the_submitter() {
        // SAFETY: single-threaded test; both variables are removed again
        // below. `set_var` is unsafe in edition 2024 only because a concurrent
        // reader would be UB.
        unsafe {
            std::env::set_var("USER", "attacker-chosen");
            std::env::set_var("LOGNAME", "attacker-chosen-too");
        }
        let answer = current_username();
        unsafe {
            std::env::remove_var("USER");
            std::env::remove_var("LOGNAME");
        }
        assert_ne!(answer.as_deref(), Some("attacker-chosen"));
        assert_ne!(answer.as_deref(), Some("attacker-chosen-too"));
    }

    /// The name must be the same on two calls within one run, and -- the part
    /// the old pid fallback broke -- must not be derived from the pid.
    ///
    /// `format!("uid{}", process::id())` gave a different name to every
    /// invocation, so a job queued as `uid4123` could never match a daemon
    /// calling itself `uid5678`: with `$USER` unset, `at` refused every job it
    /// had accepted.
    #[test]
    fn the_name_is_not_the_process_id() {
        let pid_shaped = format!("uid{}", std::process::id());
        assert_ne!(current_username().as_deref(), Some(pid_shaped.as_str()));
        // Stable within a run, which a pid-derived name also is -- this pins
        // the weaker property too so a future fallback cannot reintroduce a
        // per-call value.
        assert_eq!(current_username(), current_username());
    }

    fn job(id: u32, epoch: i64, user: &str) -> Job {
        Job {
            id,
            queue: 'a',
            epoch,
            user: user.to_string(),
            created: 0,
            commands: "/bin/true".to_string(),
        }
    }

    #[test]
    fn atd_runs_only_jobs_whose_time_has_come() {
        let jobs = vec![job(1, 100, "u"), job(2, 200, "u"), job(3, 300, "u")];
        let due = atd_due(&jobs, 200);
        assert_eq!(due.iter().map(|j| j.id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn atd_runs_a_job_due_exactly_now() {
        // `<=`, not `<`. A job scheduled for 12:00 that waits until 12:01
        // because the comparison excluded its own second is a minute late for
        // no reason anybody could find in the output.
        let jobs = vec![job(1, 500, "u")];
        assert_eq!(atd_due(&jobs, 500).len(), 1);
    }

    #[test]
    fn atd_runs_due_jobs_in_scheduled_order() {
        let jobs = vec![job(9, 300, "u"), job(4, 100, "u"), job(7, 200, "u")];
        let due = atd_due(&jobs, 999);
        assert_eq!(due.iter().map(|j| j.id).collect::<Vec<_>>(), vec![4, 7, 9]);
    }

    #[test]
    fn atd_leaves_the_future_alone() {
        let jobs = vec![job(1, 1_000, "u")];
        assert!(atd_due(&jobs, 999).is_empty());
    }

    #[test]
    fn atd_refuses_a_job_submitted_by_another_user() {
        let err = atd_may_run("alice", Some("root")).expect_err("must refuse");
        assert!(err.contains("alice"), "message names neither user: {err}");
        assert!(err.contains("root"), "message names neither user: {err}");
    }

    #[test]
    fn atd_runs_a_job_submitted_by_us() {
        assert!(atd_may_run("alice", Some("alice")).is_ok());
    }

    #[test]
    fn atd_refuses_when_it_cannot_tell_who_it_is() {
        // The arm that matters. If this returns Ok, every queued job runs as
        // whoever started the daemon, which for a system daemon is root.
        let err = atd_may_run("alice", None).expect_err("unknown must refuse");
        assert!(err.contains("unknown"), "unhelpful message: {err}");
    }

    #[test]
    fn a_refused_job_is_kept_not_discarded() {
        // The one case that must not delete. The user was told the job was
        // scheduled; throwing it away because we could not run it destroys
        // their work to tidy up after our own limitation. It stays in the
        // spool where atq shows it and atrm can remove it.
        assert!(!atd_run_one(&job(1, 0, "alice"), Some("root")));
    }

    #[test]
    fn batch_is_detected_by_path_too() {
        assert_eq!(detect_invocation("/usr/bin/batch"), InvokedAs::Batch);
    }
}
