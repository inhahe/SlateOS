//! SlateOS Cron Daemon (`crond2`) with anacron support
//!
//! A multi-personality daemon for scheduled task execution:
//!
//! - **crond mode** (default): classic cron — wakes every minute, checks all
//!   crontab entries against the current time, and spawns matching commands.
//! - **anacron mode** (when `argv[0]` is `"anacron"`): for tasks that must run
//!   at certain intervals even if the system was powered off. Reads
//!   `/etc/anacrontab` and uses timestamp files to track last-run times.
//!
//! # Crontab file locations
//!
//! - Per-user: `/var/spool/cron/crontabs/<user>`
//! - System-wide: `/etc/crontab` (has an extra user field after the five
//!   time fields)
//!
//! # Cron expression syntax
//!
//! ```text
//! # min  hour  dom  month  dow  command
//!   */5   *     *    *      *   /bin/cleanup --temp
//!   0     3     1-15 *      1-5 /bin/weekday-report
//!   30    8     1    Jan    *   /bin/monthly
//! ```
//!
//! Named months (Jan-Dec) and weekdays (Sun-Sat) are supported, as are
//! special strings: `@reboot`, `@yearly`/`@annually`, `@monthly`, `@weekly`,
//! `@daily`/`@midnight`, `@hourly`.
//!
//! # Anacrontab format
//!
//! ```text
//! # period  delay  job-id       command
//!   1        5     daily-backup  /bin/backup --daily
//!   7        10    weekly-clean  /bin/cleanup --weekly
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

/// Per-user crontab spool directory.
const USER_CRONTAB_DIR: &str = "/var/spool/cron/crontabs";

/// System-wide crontab.
const SYSTEM_CRONTAB: &str = "/etc/crontab";

/// Anacron configuration file.
const ANACRONTAB_PATH: &str = "/etc/anacrontab";

/// Anacron timestamp spool directory.
const ANACRON_SPOOL: &str = "/var/spool/anacron";

/// PID file for daemon mode.
const PID_PATH: &str = "/var/run/crond2.pid";

/// Default log level.
const DEFAULT_LOG_LEVEL: u32 = 1;

/// Seconds per minute (for the main loop sleep).
const SECS_PER_MINUTE: u64 = 60;

/// Default shell used to execute commands.
const DEFAULT_SHELL: &str = "/bin/sh";

// ============================================================================
// Time helpers
// ============================================================================

/// Current time as Unix seconds (UTC).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Broken-down time from Unix seconds (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrokenTime {
    year: i64,
    month: u32,  // 1-12
    day: u32,    // 1-31
    hour: u32,   // 0-23
    minute: u32, // 0-59
    #[allow(dead_code)]
    second: u32, // 0-59
    weekday: u32, // 0=Sunday .. 6=Saturday
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Convert Unix seconds to broken-down time (UTC).
fn unix_to_broken(unix_secs: u64) -> BrokenTime {
    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hour = (time_secs / 3600) as u32;
    let minute = ((time_secs % 3600) / 60) as u32;
    let second = (time_secs % 60) as u32;

    // Weekday: Jan 1 1970 was Thursday (4).
    let weekday = ((days + 4) % 7) as u32;

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

    let mut month = 1u32;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            month = i as u32 + 1;
            break;
        }
        remaining_days -= md;
        // If we exhaust all months, it is December.
        if i == 11 {
            month = 12;
        }
    }
    let day = remaining_days as u32 + 1;

    BrokenTime {
        year: y,
        month,
        day,
        hour,
        minute,
        second,
        weekday,
    }
}

/// Convert broken-down time back to Unix seconds (UTC).
/// Needed for next-run-time calculation.
fn broken_to_unix(bt: &BrokenTime) -> u64 {
    let mut days: i64 = 0;

    // Years from 1970 to bt.year.
    for y in 1970..bt.year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    // Handle years before 1970 (unlikely for cron, but correct).
    for y in bt.year..1970 {
        days -= if is_leap_year(y) { 366 } else { 365 };
    }

    // Months within the year.
    for m in 1..bt.month {
        days += i64::from(days_in_month(bt.year, m));
    }

    // Days within the month (1-based).
    days += i64::from(bt.day) - 1;

    let total_secs =
        days * 86400 + i64::from(bt.hour) * 3600 + i64::from(bt.minute) * 60 + i64::from(bt.second);

    if total_secs < 0 { 0 } else { total_secs as u64 }
}

// ============================================================================
// Cron field bitset — fast matching via u64 bitmask
// ============================================================================

/// A cron schedule field stored as a bitmask for O(1) matching.
///
/// Bit N is set if value N is included in the match set.
/// Maximum supported value is 63 (fits all cron fields: minute 0-59,
/// hour 0-23, day 1-31, month 1-12, weekday 0-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FieldBitset {
    bits: u64,
    /// True means "match everything" (the `*` wildcard).
    wildcard: bool,
}

impl FieldBitset {
    /// A wildcard field that matches any value.
    const fn wildcard() -> Self {
        Self {
            bits: 0,
            wildcard: true,
        }
    }

    /// An empty field that matches nothing.
    const fn empty() -> Self {
        Self {
            bits: 0,
            wildcard: false,
        }
    }

    /// Create from a single value.
    fn single(v: u32) -> Self {
        Self {
            bits: 1u64 << v,
            wildcard: false,
        }
    }

    /// Set a bit for value `v`.
    fn set(&mut self, v: u32) {
        if v < 64 {
            self.bits |= 1u64 << v;
        }
    }

    /// Test if value `v` matches this field.
    fn matches(self, v: u32) -> bool {
        if self.wildcard {
            return true;
        }
        if v >= 64 {
            return false;
        }
        (self.bits >> v) & 1 == 1
    }

    /// Return the union of two bitsets.
    fn union(self, other: Self) -> Self {
        if self.wildcard || other.wildcard {
            return Self::wildcard();
        }
        Self {
            bits: self.bits | other.bits,
            wildcard: false,
        }
    }

    /// True if no bits are set and not a wildcard.
    // Held-for-future API: part of the coherent FieldBitset interface; useful
    // for validating that a parsed cron field actually matches something.
    #[allow(dead_code)]
    fn is_empty(self) -> bool {
        !self.wildcard && self.bits == 0
    }

    /// Smallest value >= `from` that is set, or None.
    // Held-for-future API: needed by an alternate next-match algorithm that
    // walks fields directly instead of probing minute-by-minute.
    #[allow(dead_code)]
    fn next_set_from(self, from: u32, max: u32) -> Option<u32> {
        if self.wildcard {
            if from <= max {
                return Some(from);
            }
            return None;
        }
        (from..=max).find(|&v| v < 64 && (self.bits >> v) & 1 == 1)
    }

    /// Smallest value that is set, or None.
    // Held-for-future API: companion to next_set_from for the same algorithm.
    #[allow(dead_code)]
    fn first_set(self, min: u32, max: u32) -> Option<u32> {
        self.next_set_from(min, max)
    }
}

// ============================================================================
// Name-to-number mapping for months and weekdays
// ============================================================================

/// Map a three-letter month abbreviation to its number (1-12).
fn month_name_to_num(name: &str) -> Option<u32> {
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

/// Map a three-letter weekday abbreviation to its number (0=Sun .. 6=Sat).
fn weekday_name_to_num(name: &str) -> Option<u32> {
    match name.to_ascii_lowercase().as_str() {
        "sun" => Some(0),
        "mon" => Some(1),
        "tue" => Some(2),
        "wed" => Some(3),
        "thu" => Some(4),
        "fri" => Some(5),
        "sat" => Some(6),
        _ => None,
    }
}

/// Parse a token that could be a number or a named abbreviation.
fn parse_token(s: &str, name_fn: fn(&str) -> Option<u32>) -> Result<u32, String> {
    if let Ok(n) = s.parse::<u32>() {
        return Ok(n);
    }
    name_fn(s).ok_or_else(|| format!("unknown name: {s}"))
}

// ============================================================================
// Cron field parser
// ============================================================================

/// Parse a single cron field (e.g. `"*/5"`, `"1-5"`, `"Mon,Wed,Fri"`)
/// into a `FieldBitset`.
///
/// `min` and `max` define the valid range for this field type.
/// `name_fn` optionally maps names to numbers (for months/weekdays).
fn parse_field(
    s: &str,
    min: u32,
    max: u32,
    name_fn: fn(&str) -> Option<u32>,
) -> Result<FieldBitset, String> {
    // Wildcard.
    if s == "*" {
        return Ok(FieldBitset::wildcard());
    }

    // Comma-separated list: each element can be a range, step, or value.
    if s.contains(',') {
        let mut result = FieldBitset::empty();
        for part in s.split(',') {
            let sub = parse_field_atom(part.trim(), min, max, name_fn)?;
            result = result.union(sub);
        }
        return Ok(result);
    }

    parse_field_atom(s, min, max, name_fn)
}

/// Parse a single atom within a cron field (no commas).
fn parse_field_atom(
    s: &str,
    min: u32,
    max: u32,
    name_fn: fn(&str) -> Option<u32>,
) -> Result<FieldBitset, String> {
    // Step: */N or range/N
    if let Some((base_part, step_str)) = s.split_once('/') {
        let step: u32 = step_str
            .parse()
            .map_err(|_| format!("bad step value: {step_str}"))?;
        if step == 0 {
            return Err("step cannot be zero".to_string());
        }

        let (range_min, range_max) = if base_part == "*" {
            (min, max)
        } else if let Some((lo_str, hi_str)) = base_part.split_once('-') {
            let lo = parse_token(lo_str, name_fn)?;
            let hi = parse_token(hi_str, name_fn)?;
            (lo, hi)
        } else {
            let base = parse_token(base_part, name_fn)?;
            (base, max)
        };

        let mut result = FieldBitset::empty();
        let mut v = range_min;
        while v <= range_max {
            result.set(v);
            // Guard against overflow when v + step > u32::MAX.
            v = match v.checked_add(step) {
                Some(next) => next,
                None => break,
            };
        }
        return Ok(result);
    }

    // Range: N-M
    if let Some((lo_str, hi_str)) = s.split_once('-') {
        let lo = parse_token(lo_str, name_fn)?;
        let hi = parse_token(hi_str, name_fn)?;
        if lo > hi {
            return Err(format!("range start {lo} > end {hi}"));
        }
        let mut result = FieldBitset::empty();
        for v in lo..=hi {
            result.set(v);
        }
        return Ok(result);
    }

    // Single value (number or name).
    let v = parse_token(s, name_fn)?;
    if v < min || v > max {
        return Err(format!("value {v} out of range [{min}-{max}]"));
    }
    Ok(FieldBitset::single(v))
}

/// Stub name function for fields that do not support names.
fn no_names(_s: &str) -> Option<u32> {
    None
}

// ============================================================================
// Cron expression (five fields)
// ============================================================================

/// A fully parsed cron time expression (five fields).
#[derive(Debug, Clone, Copy)]
struct CronExpr {
    minute: FieldBitset,
    hour: FieldBitset,
    dom: FieldBitset, // day of month
    month: FieldBitset,
    dow: FieldBitset, // day of week
    /// Both dom and dow were explicitly specified (not wildcards).
    /// Per POSIX, if both are restricted, match is a union (OR), not
    /// intersection (AND).
    dom_and_dow_restricted: bool,
}

impl CronExpr {
    /// Parse a five-field cron expression string.
    fn parse(fields: &[&str]) -> Result<Self, String> {
        if fields.len() < 5 {
            return Err("need 5 schedule fields".to_string());
        }

        let minute = parse_field(fields[0], 0, 59, no_names)?;
        let hour = parse_field(fields[1], 0, 23, no_names)?;
        let dom = parse_field(fields[2], 1, 31, no_names)?;
        let month = parse_field(fields[3], 1, 12, month_name_to_num)?;
        let mut dow = parse_field(fields[4], 0, 7, weekday_name_to_num)?;

        // Normalize: day-of-week 7 == 0 (both mean Sunday).
        if dow.matches(7) && !dow.wildcard {
            dow.set(0);
        }

        let dom_and_dow_restricted = !dom.wildcard && !dow.wildcard;

        Ok(CronExpr {
            minute,
            hour,
            dom,
            month,
            dow,
            dom_and_dow_restricted,
        })
    }

    /// Check if this expression matches the given time.
    fn matches_time(&self, bt: &BrokenTime) -> bool {
        if !self.minute.matches(bt.minute) {
            return false;
        }
        if !self.hour.matches(bt.hour) {
            return false;
        }
        if !self.month.matches(bt.month) {
            return false;
        }

        // Day matching: if both dom and dow are restricted, use union (OR).
        if self.dom_and_dow_restricted {
            self.dom.matches(bt.day) || self.dow.matches(bt.weekday)
        } else {
            self.dom.matches(bt.day) && self.dow.matches(bt.weekday)
        }
    }

    /// Calculate the next time (as Unix seconds) at or after `from` when this
    /// expression matches. Returns `None` if no match is found within a
    /// reasonable horizon (4 years).
    // Held-for-future API: scheduling currently uses per-tick matches() probing;
    // this helper enables a future "sleep until next match" optimisation.
    #[allow(dead_code)]
    fn next_match_after(&self, from: u64) -> Option<u64> {
        let mut bt = unix_to_broken(from);
        // Start at the beginning of the current minute.  If `from` is not
        // exactly on a minute boundary, that truncated instant lies in the
        // past; advance to the next minute boundary.  Cron fires on minute
        // boundaries and the next match must never be earlier than `from`.
        bt.second = 0;
        if broken_to_unix(&bt) < from {
            let next_minute = broken_to_unix(&bt) + 60;
            bt = unix_to_broken(next_minute);
            bt.second = 0;
        }

        // Search up to ~4 years to cover leap year cycles.
        let limit = from + 4 * 366 * 86400;

        loop {
            let current = broken_to_unix(&bt);
            if current > limit {
                return None;
            }

            // Month check.
            if !self.month.matches(bt.month) {
                // Advance to next matching month.
                if let Some(next_m) = self.month.next_set_from(bt.month + 1, 12) {
                    bt.month = next_m;
                    bt.day = 1;
                    bt.hour = 0;
                    bt.minute = 0;
                    continue;
                }
                // Wrap to next year.
                bt.year += 1;
                bt.month = self.month.first_set(1, 12).unwrap_or(1);
                bt.day = 1;
                bt.hour = 0;
                bt.minute = 0;
                continue;
            }

            // Day check (dom/dow union logic).
            let day_ok = if self.dom_and_dow_restricted {
                self.dom.matches(bt.day) || self.dow.matches(bt.weekday)
            } else {
                self.dom.matches(bt.day) && self.dow.matches(bt.weekday)
            };

            if !day_ok {
                // Advance one day.
                bt.day += 1;
                bt.hour = 0;
                bt.minute = 0;
                let max_day = days_in_month(bt.year, bt.month);
                if bt.day > max_day {
                    bt.day = 1;
                    bt.month += 1;
                    if bt.month > 12 {
                        bt.month = 1;
                        bt.year += 1;
                    }
                }
                // Recalculate weekday.
                let new_secs = broken_to_unix(&bt);
                bt = unix_to_broken(new_secs);
                bt.second = 0;
                continue;
            }

            // Hour check.
            if !self.hour.matches(bt.hour) {
                if let Some(next_h) = self.hour.next_set_from(bt.hour + 1, 23) {
                    bt.hour = next_h;
                    bt.minute = 0;
                    continue;
                }
                // Advance to next day.
                bt.day += 1;
                bt.hour = 0;
                bt.minute = 0;
                let max_day = days_in_month(bt.year, bt.month);
                if bt.day > max_day {
                    bt.day = 1;
                    bt.month += 1;
                    if bt.month > 12 {
                        bt.month = 1;
                        bt.year += 1;
                    }
                }
                let new_secs = broken_to_unix(&bt);
                bt = unix_to_broken(new_secs);
                bt.second = 0;
                continue;
            }

            // Minute check.
            if !self.minute.matches(bt.minute) {
                if let Some(next_min) = self.minute.next_set_from(bt.minute + 1, 59) {
                    bt.minute = next_min;
                    // Check full match.
                    let candidate = broken_to_unix(&bt);
                    let check = unix_to_broken(candidate);
                    if self.matches_time(&check) {
                        return Some(candidate);
                    }
                    // Move forward.
                    bt.minute = next_min + 1;
                    if bt.minute > 59 {
                        bt.hour += 1;
                        bt.minute = 0;
                        if bt.hour > 23 {
                            bt.day += 1;
                            bt.hour = 0;
                            let max_day = days_in_month(bt.year, bt.month);
                            if bt.day > max_day {
                                bt.day = 1;
                                bt.month += 1;
                                if bt.month > 12 {
                                    bt.month = 1;
                                    bt.year += 1;
                                }
                            }
                        }
                        let new_secs = broken_to_unix(&bt);
                        bt = unix_to_broken(new_secs);
                        bt.second = 0;
                    }
                    continue;
                }
                // No more minutes this hour; advance hour.
                bt.hour += 1;
                bt.minute = 0;
                if bt.hour > 23 {
                    bt.day += 1;
                    bt.hour = 0;
                    let max_day = days_in_month(bt.year, bt.month);
                    if bt.day > max_day {
                        bt.day = 1;
                        bt.month += 1;
                        if bt.month > 12 {
                            bt.month = 1;
                            bt.year += 1;
                        }
                    }
                }
                let new_secs = broken_to_unix(&bt);
                bt = unix_to_broken(new_secs);
                bt.second = 0;
                continue;
            }

            // All fields match.
            return Some(broken_to_unix(&bt));
        }
    }
}

// ============================================================================
// Special cron strings
// ============================================================================

/// Sentinel for `@reboot` jobs (no time expression).
// Held-for-future use: @reboot jobs are currently tracked via the explicit
// `is_reboot: bool` field on CronJob; this sentinel is reserved for a future
// representation that stores reboot jobs in the same "next-fire" priority
// queue as scheduled jobs.
#[allow(dead_code)]
const REBOOT_SENTINEL: u64 = u64::MAX;

/// Parse `@keyword` into either a `CronExpr` or the reboot sentinel.
fn parse_special_string(keyword: &str) -> Result<(Option<CronExpr>, bool), String> {
    match keyword.to_ascii_lowercase().as_str() {
        "@reboot" => Ok((None, true)),
        "@yearly" | "@annually" => {
            // 0 0 1 1 *
            let fields = ["0", "0", "1", "1", "*"];
            Ok((Some(CronExpr::parse(&fields)?), false))
        }
        "@monthly" => {
            // 0 0 1 * *
            let fields = ["0", "0", "1", "*", "*"];
            Ok((Some(CronExpr::parse(&fields)?), false))
        }
        "@weekly" => {
            // 0 0 * * 0
            let fields = ["0", "0", "*", "*", "0"];
            Ok((Some(CronExpr::parse(&fields)?), false))
        }
        "@daily" | "@midnight" => {
            // 0 0 * * *
            let fields = ["0", "0", "*", "*", "*"];
            Ok((Some(CronExpr::parse(&fields)?), false))
        }
        "@hourly" => {
            // 0 * * * *
            let fields = ["0", "*", "*", "*", "*"];
            Ok((Some(CronExpr::parse(&fields)?), false))
        }
        other => Err(format!("unknown special string: {other}")),
    }
}

// ============================================================================
// Cron entry (a line from a crontab file)
// ============================================================================

/// A single cron job entry loaded from a crontab file.
#[derive(Debug, Clone)]
struct CronEntry {
    /// The cron time expression (None for @reboot jobs).
    expr: Option<CronExpr>,
    /// The command to execute.
    command: String,
    /// The user to run as (from /etc/crontab or the file's owner).
    user: String,
    /// Whether this is a @reboot job.
    is_reboot: bool,
    /// Accumulated environment variables from preceding lines.
    env_vars: HashMap<String, String>,
}

/// A collection of cron entries from one or more crontab files.
struct CronTab {
    entries: Vec<CronEntry>,
}

impl CronTab {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Load a user crontab (no user field in the time spec).
    fn load_user_crontab(&mut self, path: &Path, user: &str) {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log_msg(2, &format!("cannot read {}: {e}", path.display()));
                return;
            }
        };

        let mut env_vars: HashMap<String, String> = HashMap::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Environment variable assignment: NAME=value (no spaces around =).
            if let Some(entry) = try_parse_env_line(trimmed) {
                env_vars.insert(entry.0, entry.1);
                continue;
            }

            match parse_crontab_line(trimmed, false) {
                Ok((expr, is_reboot, cmd)) => {
                    self.entries.push(CronEntry {
                        expr,
                        command: cmd,
                        user: user.to_string(),
                        is_reboot,
                        env_vars: env_vars.clone(),
                    });
                }
                Err(e) => {
                    log_msg(
                        2,
                        &format!("bad line in {}: {e}: {trimmed}", path.display()),
                    );
                }
            }
        }
    }

    /// Load the system crontab (/etc/crontab) which has an extra user field.
    fn load_system_crontab(&mut self, path: &Path) {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log_msg(2, &format!("cannot read {}: {e}", path.display()));
                return;
            }
        };

        let mut env_vars: HashMap<String, String> = HashMap::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(entry) = try_parse_env_line(trimmed) {
                env_vars.insert(entry.0, entry.1);
                continue;
            }

            match parse_crontab_line(trimmed, true) {
                Ok((expr, is_reboot, _cmd)) => {
                    // For system crontab, the user field is the 6th token (index 5),
                    // and the command follows. We need to re-parse to extract the user.
                    let user_and_cmd = extract_user_from_system_line(trimmed);
                    self.entries.push(CronEntry {
                        expr,
                        command: user_and_cmd.1,
                        user: user_and_cmd.0,
                        is_reboot,
                        env_vars: env_vars.clone(),
                    });
                }
                Err(e) => {
                    log_msg(
                        2,
                        &format!("bad line in {}: {e}: {trimmed}", path.display()),
                    );
                }
            }
        }
    }

    /// Return all entries that match the given time.
    fn matching_entries(&self, bt: &BrokenTime) -> Vec<&CronEntry> {
        self.entries
            .iter()
            .filter(|e| {
                if e.is_reboot {
                    return false;
                }
                if let Some(ref expr) = e.expr {
                    expr.matches_time(bt)
                } else {
                    false
                }
            })
            .collect()
    }

    /// Return all @reboot entries.
    fn reboot_entries(&self) -> Vec<&CronEntry> {
        self.entries.iter().filter(|e| e.is_reboot).collect()
    }
}

/// Try to parse an environment variable assignment line.
/// Format: NAME=value (NAME must be alphanumeric + underscore, starts with letter).
fn try_parse_env_line(line: &str) -> Option<(String, String)> {
    // Must contain '=' and not start with a special char or digit.
    let eq_pos = line.find('=')?;
    let name = &line[..eq_pos];
    let value = &line[eq_pos + 1..];

    // Name must be a valid identifier-like string.
    if name.is_empty() {
        return None;
    }
    let first = name.as_bytes()[0];
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }

    // Strip optional surrounding quotes from value.
    let val = value.trim();
    let val = if (val.starts_with('"') && val.ends_with('"'))
        || (val.starts_with('\'') && val.ends_with('\''))
    {
        if val.len() >= 2 {
            &val[1..val.len() - 1]
        } else {
            val
        }
    } else {
        val
    };

    Some((name.to_string(), val.to_string()))
}

/// Parse a crontab line into (expression, is_reboot, command).
/// `has_user_field`: true for /etc/crontab (6th field is user, 7th+ is command),
/// false for user crontabs (6th+ field is command).
fn parse_crontab_line(
    line: &str,
    has_user_field: bool,
) -> Result<(Option<CronExpr>, bool, String), String> {
    let trimmed = line.trim();

    // Special @keyword lines.
    if trimmed.starts_with('@') {
        let extra_fields = if has_user_field { 2 } else { 1 };
        let parts: Vec<&str> = trimmed
            .splitn(1 + extra_fields, char::is_whitespace)
            .collect();
        if parts.len() < 1 + extra_fields {
            return Err("need @keyword and command".to_string());
        }

        let (expr, is_reboot) = parse_special_string(parts[0])?;
        let cmd = if has_user_field {
            // parts[1] is user, parts[2..] is command.
            if parts.len() > 2 {
                parts[2].to_string()
            } else {
                return Err("need command after user field".to_string());
            }
        } else {
            parts[1].to_string()
        };

        return Ok((expr, is_reboot, cmd));
    }

    // Standard five-field line.
    let num_pre_cmd = if has_user_field { 7 } else { 6 };
    let parts: Vec<&str> = trimmed.splitn(num_pre_cmd, char::is_whitespace).collect();
    if parts.len() < num_pre_cmd {
        return Err(format!("need {} fields", num_pre_cmd));
    }

    let fields = &parts[0..5];
    let expr = CronExpr::parse(fields)?;
    let cmd = if has_user_field {
        // parts[5] is user, parts[6] is command.
        parts[6].to_string()
    } else {
        parts[5].to_string()
    };

    Ok((Some(expr), false, cmd))
}

/// Extract (user, command) from a system crontab line.
fn extract_user_from_system_line(line: &str) -> (String, String) {
    let trimmed = line.trim();

    if trimmed.starts_with('@') {
        // @keyword user command
        let parts: Vec<&str> = trimmed.splitn(3, char::is_whitespace).collect();
        if parts.len() >= 3 {
            return (parts[1].to_string(), parts[2].to_string());
        }
        return ("root".to_string(), trimmed.to_string());
    }

    // min hour dom month dow user command
    let parts: Vec<&str> = trimmed.splitn(7, char::is_whitespace).collect();
    if parts.len() >= 7 {
        (parts[5].to_string(), parts[6].to_string())
    } else if parts.len() >= 6 {
        (parts[5].to_string(), String::new())
    } else {
        ("root".to_string(), trimmed.to_string())
    }
}

// ============================================================================
// Anacron entry and table
// ============================================================================

/// An anacron job entry.
#[derive(Debug, Clone)]
struct AnacronEntry {
    /// Period in days between runs.
    period_days: u32,
    /// Delay in minutes after anacron starts before running this job.
    delay_minutes: u32,
    /// Unique identifier for timestamp tracking.
    job_id: String,
    /// Command to execute.
    command: String,
}

/// The anacron job table.
struct AnacronTab {
    entries: Vec<AnacronEntry>,
    /// Environment variables defined in the anacrontab.
    env_vars: HashMap<String, String>,
}

impl AnacronTab {
    fn load(path: &Path) -> Result<Self, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;

        let mut entries = Vec::new();
        let mut env_vars = HashMap::new();

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Environment variable.
            if let Some(entry) = try_parse_env_line(trimmed) {
                env_vars.insert(entry.0, entry.1);
                continue;
            }

            // Anacron entry: period delay job-id command
            let parts: Vec<&str> = trimmed.splitn(4, char::is_whitespace).collect();
            if parts.len() < 4 {
                return Err(format!(
                    "line {}: need period, delay, job-id, and command",
                    line_num + 1
                ));
            }

            let period_days: u32 = parts[0]
                .parse()
                .map_err(|_| format!("line {}: bad period: {}", line_num + 1, parts[0]))?;

            let delay_minutes: u32 = parts[1]
                .parse()
                .map_err(|_| format!("line {}: bad delay: {}", line_num + 1, parts[1]))?;

            let job_id = parts[2].to_string();
            let command = parts[3].to_string();

            entries.push(AnacronEntry {
                period_days,
                delay_minutes,
                job_id,
                command,
            });
        }

        Ok(AnacronTab { entries, env_vars })
    }

    /// Validate syntax only (for -T flag).
    fn validate(path: &Path) -> Result<(), String> {
        let _ = Self::load(path)?;
        Ok(())
    }
}

/// Read the timestamp for an anacron job (Unix seconds of last run).
fn read_anacron_timestamp(job_id: &str) -> u64 {
    let path = PathBuf::from(ANACRON_SPOOL).join(job_id);
    match fs::read_to_string(&path) {
        Ok(s) => {
            // Timestamp file contains a date string like "20260518" or Unix secs.
            // We support both: try Unix seconds first, then YYYYMMDD.
            let trimmed = s.trim();
            if let Ok(secs) = trimmed.parse::<u64>() {
                return secs;
            }
            // Try YYYYMMDD format.
            if trimmed.len() == 8
                && let (Ok(y), Ok(m), Ok(d)) = (
                    trimmed[0..4].parse::<i64>(),
                    trimmed[4..6].parse::<u32>(),
                    trimmed[6..8].parse::<u32>(),
                ) {
                    let bt = BrokenTime {
                        year: y,
                        month: m,
                        day: d,
                        hour: 0,
                        minute: 0,
                        second: 0,
                        weekday: 0, // Not needed for conversion.
                    };
                    return broken_to_unix(&bt);
                }
            0
        }
        Err(_) => 0,
    }
}

/// Write the current timestamp for an anacron job.
fn write_anacron_timestamp(job_id: &str, secs: u64) {
    let dir = Path::new(ANACRON_SPOOL);
    if let Err(e) = fs::create_dir_all(dir) {
        log_msg(1, &format!("cannot create {}: {e}", dir.display()));
        return;
    }

    let bt = unix_to_broken(secs);
    let date_str = format!("{:04}{:02}{:02}", bt.year, bt.month, bt.day);
    let path = dir.join(job_id);

    if let Err(e) = fs::write(&path, date_str.as_bytes()) {
        log_msg(
            1,
            &format!("cannot write timestamp {}: {e}", path.display()),
        );
    }
}

// ============================================================================
// Logging
// ============================================================================

/// Global log level (set from -L flag). Higher = more verbose.
static mut LOG_LEVEL: u32 = DEFAULT_LOG_LEVEL;
/// If true, log to stdout instead of syslog.
static mut LOG_TO_STDOUT: bool = false;

/// Log a message if the current log level is >= `level`.
fn log_msg(level: u32, msg: &str) {
    // SAFETY: These statics are only written during argument parsing (single-
    // threaded startup), then read-only during the main loop. No data race.
    let (log_level, to_stdout) = unsafe { (LOG_LEVEL, LOG_TO_STDOUT) };

    if level > log_level {
        return;
    }

    if to_stdout {
        let bt = unix_to_broken(now_secs());
        println!(
            "[{:04}-{:02}-{:02} {:02}:{:02}] {}",
            bt.year, bt.month, bt.day, bt.hour, bt.minute, msg
        );
    } else {
        // Syslog: on SlateOS, write to /dev/log or use the syslog utility.
        // For now, write to stderr as a fallback.
        let bt = unix_to_broken(now_secs());
        eprintln!(
            "crond2[{}]: [{:04}-{:02}-{:02} {:02}:{:02}] {}",
            std::process::id(),
            bt.year,
            bt.month,
            bt.day,
            bt.hour,
            bt.minute,
            msg
        );
    }
}

// ============================================================================
// Job execution
// ============================================================================

/// Execute a cron command with the given environment.
fn execute_command(command: &str, user: &str, env_vars: &HashMap<String, String>) {
    log_msg(1, &format!("({user}) CMD ({command})"));

    let shell = env_vars
        .get("SHELL")
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SHELL);

    let mut cmd = process::Command::new(shell);
    cmd.arg("-c").arg(command);

    // Set environment variables from the crontab.
    for (key, val) in env_vars {
        cmd.env(key, val);
    }

    // Set USER and LOGNAME.
    cmd.env("USER", user);
    cmd.env("LOGNAME", user);

    // Set HOME if not already set.
    if !env_vars.contains_key("HOME") {
        cmd.env("HOME", format!("/home/{user}"));
    }

    match cmd.output() {
        Ok(output) => {
            // If the command produced output, log it or send to mail.
            if !output.stdout.is_empty()
                && let Ok(text) = String::from_utf8(output.stdout) {
                    // Send to user's mail spool.
                    send_mail(user, command, &text);
                }
            if !output.stderr.is_empty()
                && let Ok(text) = String::from_utf8(output.stderr) {
                    log_msg(1, &format!("({user}) STDERR: {text}"));
                }
            if !output.status.success() {
                let code = output.status.code().unwrap_or(-1);
                log_msg(
                    1,
                    &format!("({user}) CMD ({command}) exited with status {code}"),
                );
            }
        }
        Err(e) => {
            log_msg(1, &format!("({user}) EXEC FAILED ({command}): {e}"));
        }
    }
}

/// Send command output to the user's mail spool.
fn send_mail(user: &str, command: &str, body: &str) {
    let mail_dir = format!("/var/mail/{user}");
    let _ = fs::create_dir_all(
        Path::new(&mail_dir)
            .parent()
            .unwrap_or(Path::new("/var/mail")),
    );

    let bt = unix_to_broken(now_secs());
    let header = format!(
        "From: Cron Daemon <root>\nTo: {user}\nSubject: Cron <{user}> {command}\nDate: {:04}-{:02}-{:02} {:02}:{:02}\n\n",
        bt.year, bt.month, bt.day, bt.hour, bt.minute
    );

    let mail_path = format!("/var/mail/{user}");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&mail_path)
    {
        Ok(mut f) => {
            let _ = f.write_all(header.as_bytes());
            let _ = f.write_all(body.as_bytes());
            let _ = f.write_all(b"\n");
        }
        Err(e) => {
            log_msg(2, &format!("cannot write mail for {user}: {e}"));
        }
    }
}

// ============================================================================
// Crond main loop
// ============================================================================

/// Load all crontab files and return a combined table.
fn load_all_crontabs() -> CronTab {
    let mut tab = CronTab::new();

    // Load system crontab.
    let sys_path = Path::new(SYSTEM_CRONTAB);
    if sys_path.exists() {
        log_msg(2, "loading /etc/crontab");
        tab.load_system_crontab(sys_path);
    }

    // Load per-user crontabs.
    let spool = Path::new(USER_CRONTAB_DIR);
    if let Ok(entries) = fs::read_dir(spool) {
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.is_file() {
                let user = entry.file_name().to_string_lossy().to_string();
                log_msg(2, &format!("loading crontab for user {user}"));
                tab.load_user_crontab(&path, &user);
            }
        }
    }

    log_msg(1, &format!("loaded {} cron entries", tab.entries.len()));
    tab
}

/// The main crond daemon loop.
fn run_crond(foreground: bool) {
    if !foreground {
        // Write PID file.
        let pid_dir = Path::new(PID_PATH)
            .parent()
            .unwrap_or(Path::new("/var/run"));
        let _ = fs::create_dir_all(pid_dir);
        let _ = fs::write(PID_PATH, format!("{}", process::id()));
    }

    log_msg(0, "crond2 starting");

    let mut tab = load_all_crontabs();

    // Run @reboot jobs.
    let reboot_jobs: Vec<CronEntry> = tab.reboot_entries().iter().map(|e| (*e).clone()).collect();
    for entry in &reboot_jobs {
        execute_command(&entry.command, &entry.user, &entry.env_vars);
    }

    loop {
        // Sleep until the next minute boundary.
        let now = now_secs();
        let next_minute = ((now / SECS_PER_MINUTE) + 1) * SECS_PER_MINUTE;
        let sleep_secs = next_minute.saturating_sub(now);
        if sleep_secs > 0 {
            std::thread::sleep(std::time::Duration::from_secs(sleep_secs));
        }

        // Reload crontabs every iteration (checks for changes).
        tab = load_all_crontabs();

        let bt = unix_to_broken(now_secs());
        log_msg(
            2,
            &format!(
                "tick: {:04}-{:02}-{:02} {:02}:{:02}",
                bt.year, bt.month, bt.day, bt.hour, bt.minute
            ),
        );

        // Find and execute matching entries.
        let matching: Vec<CronEntry> = tab
            .matching_entries(&bt)
            .iter()
            .map(|e| (*e).clone())
            .collect();

        for entry in &matching {
            // Spawn each job in a separate thread to avoid blocking.
            let cmd = entry.command.clone();
            let user = entry.user.clone();
            let env = entry.env_vars.clone();
            std::thread::spawn(move || {
                execute_command(&cmd, &user, &env);
            });
        }
    }
}

// ============================================================================
// Anacron main logic
// ============================================================================

/// Run anacron mode.
fn run_anacron(
    force: bool,
    update_only: bool,
    no_delay: bool,
    serialize: bool,
    foreground: bool,
    test_only: bool,
) {
    if test_only {
        match AnacronTab::validate(Path::new(ANACRONTAB_PATH)) {
            Ok(()) => {
                println!("anacrontab syntax OK");
                process::exit(0);
            }
            Err(e) => {
                eprintln!("anacrontab error: {e}");
                process::exit(1);
            }
        }
    }

    let anacrontab = match AnacronTab::load(Path::new(ANACRONTAB_PATH)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error loading anacrontab: {e}");
            process::exit(1);
        }
    };

    if foreground {
        log_msg(0, "anacron starting in foreground");
    } else {
        log_msg(0, "anacron starting");
    }

    let now = now_secs();
    let secs_per_day: u64 = 86400;

    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();

    for entry in &anacrontab.entries {
        let last_run = read_anacron_timestamp(&entry.job_id);
        let elapsed_days = if last_run > 0 {
            (now.saturating_sub(last_run)) / secs_per_day
        } else {
            // Never run before — treat as overdue.
            u64::from(entry.period_days) + 1
        };

        let should_run = force || elapsed_days >= u64::from(entry.period_days);

        if !should_run {
            log_msg(
                1,
                &format!(
                    "job `{}` not due (last run {} days ago, period {} days)",
                    entry.job_id, elapsed_days, entry.period_days
                ),
            );
            continue;
        }

        if update_only {
            log_msg(1, &format!("updating timestamp for `{}`", entry.job_id));
            write_anacron_timestamp(&entry.job_id, now);
            continue;
        }

        let delay_secs = if no_delay {
            0
        } else {
            u64::from(entry.delay_minutes) * 60
        };

        let cmd = entry.command.clone();
        let job_id = entry.job_id.clone();
        let env_vars = anacrontab.env_vars.clone();
        let run_now = now;

        let handle = std::thread::spawn(move || {
            if delay_secs > 0 {
                log_msg(1, &format!("job `{job_id}`: delaying {delay_secs}s"));
                std::thread::sleep(std::time::Duration::from_secs(delay_secs));
            }

            log_msg(1, &format!("job `{job_id}`: running `{cmd}`"));
            execute_command(&cmd, "root", &env_vars);
            write_anacron_timestamp(&job_id, run_now);
            log_msg(1, &format!("job `{job_id}`: done"));
        });

        if serialize {
            // Wait for each job before starting the next.
            let _ = handle.join();
        } else {
            handles.push(handle);
        }
    }

    // Wait for all parallel jobs to finish.
    for h in handles {
        let _ = h.join();
    }

    log_msg(0, "anacron: all jobs processed");
}

// ============================================================================
// CLI argument parsing
// ============================================================================

struct CrondArgs {
    foreground: bool,
    log_level: u32,
    log_stdout: bool,
}

struct AnacronArgs {
    force: bool,
    update_only: bool,
    no_delay: bool,
    serialize: bool,
    foreground: bool,
    test_only: bool,
}

fn print_crond_usage() {
    eprintln!("Usage: crond2 [-f] [-L loglevel] [-l]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -f          Run in foreground (don't daemonize)");
    eprintln!("  -L level    Set log level (0=minimal, 1=normal, 2=verbose)");
    eprintln!("  -l          Log to stdout instead of syslog");
}

fn print_anacron_usage() {
    eprintln!("Usage: anacron [-f] [-u] [-n] [-s] [-d] [-T]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -f    Force run all jobs regardless of timestamps");
    eprintln!("  -u    Only update timestamps, don't run jobs");
    eprintln!("  -n    Run jobs without delay");
    eprintln!("  -s    Serialize jobs (run one at a time)");
    eprintln!("  -d    Run in foreground, log to stderr");
    eprintln!("  -T    Test anacrontab syntax and exit");
}

fn parse_crond_args(args: &[String]) -> Result<CrondArgs, String> {
    let mut result = CrondArgs {
        foreground: false,
        log_level: DEFAULT_LOG_LEVEL,
        log_stdout: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" => result.foreground = true,
            "-l" => result.log_stdout = true,
            "-L" => {
                i += 1;
                if i >= args.len() {
                    return Err("-L requires a log level argument".to_string());
                }
                result.log_level = args[i]
                    .parse()
                    .map_err(|_| format!("bad log level: {}", args[i]))?;
            }
            "-h" | "--help" => {
                print_crond_usage();
                process::exit(0);
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
        i += 1;
    }

    Ok(result)
}

fn parse_anacron_args(args: &[String]) -> Result<AnacronArgs, String> {
    let mut result = AnacronArgs {
        force: false,
        update_only: false,
        no_delay: false,
        serialize: false,
        foreground: false,
        test_only: false,
    };

    for arg in args {
        match arg.as_str() {
            "-f" => result.force = true,
            "-u" => result.update_only = true,
            "-n" => result.no_delay = true,
            "-s" => result.serialize = true,
            "-d" => result.foreground = true,
            "-T" => result.test_only = true,
            "-h" | "--help" => {
                print_anacron_usage();
                process::exit(0);
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
    }

    Ok(result)
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Determine personality from argv[0].
    let prog_name = args
        .first()
        .map(|s| {
            Path::new(s)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_default();

    let is_anacron = prog_name == "anacron";

    if is_anacron {
        let anacron_args = match parse_anacron_args(&args[1..]) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("anacron: {e}");
                print_anacron_usage();
                process::exit(1);
            }
        };

        if anacron_args.foreground {
            // SAFETY: Single-threaded at this point (before spawning any threads).
            unsafe {
                LOG_TO_STDOUT = true;
            }
        }

        run_anacron(
            anacron_args.force,
            anacron_args.update_only,
            anacron_args.no_delay,
            anacron_args.serialize,
            anacron_args.foreground,
            anacron_args.test_only,
        );
    } else {
        let crond_args = match parse_crond_args(&args[1..]) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("crond2: {e}");
                print_crond_usage();
                process::exit(1);
            }
        };

        // SAFETY: Single-threaded at this point (before spawning any threads).
        unsafe {
            LOG_LEVEL = crond_args.log_level;
            LOG_TO_STDOUT = crond_args.log_stdout;
        }

        run_crond(crond_args.foreground);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- FieldBitset basics ----

    #[test]
    fn bitset_wildcard_matches_anything() {
        let f = FieldBitset::wildcard();
        assert!(f.matches(0));
        assert!(f.matches(59));
        assert!(f.matches(31));
    }

    #[test]
    fn bitset_empty_matches_nothing() {
        let f = FieldBitset::empty();
        assert!(!f.matches(0));
        assert!(!f.matches(1));
    }

    #[test]
    fn bitset_single_value() {
        let f = FieldBitset::single(5);
        assert!(f.matches(5));
        assert!(!f.matches(4));
        assert!(!f.matches(6));
    }

    #[test]
    fn bitset_set_and_match() {
        let mut f = FieldBitset::empty();
        f.set(10);
        f.set(20);
        f.set(30);
        assert!(f.matches(10));
        assert!(f.matches(20));
        assert!(f.matches(30));
        assert!(!f.matches(15));
    }

    #[test]
    fn bitset_union() {
        let a = FieldBitset::single(3);
        let b = FieldBitset::single(7);
        let u = a.union(b);
        assert!(u.matches(3));
        assert!(u.matches(7));
        assert!(!u.matches(5));
    }

    #[test]
    fn bitset_union_with_wildcard() {
        let a = FieldBitset::single(3);
        let w = FieldBitset::wildcard();
        let u = a.union(w);
        assert!(u.wildcard);
        assert!(u.matches(0));
        assert!(u.matches(59));
    }

    #[test]
    fn bitset_is_empty() {
        assert!(FieldBitset::empty().is_empty());
        assert!(!FieldBitset::single(0).is_empty());
        assert!(!FieldBitset::wildcard().is_empty());
    }

    #[test]
    fn bitset_next_set_from() {
        let mut f = FieldBitset::empty();
        f.set(5);
        f.set(15);
        f.set(30);
        assert_eq!(f.next_set_from(0, 59), Some(5));
        assert_eq!(f.next_set_from(6, 59), Some(15));
        assert_eq!(f.next_set_from(16, 59), Some(30));
        assert_eq!(f.next_set_from(31, 59), None);
    }

    #[test]
    fn bitset_next_set_from_wildcard() {
        let f = FieldBitset::wildcard();
        assert_eq!(f.next_set_from(10, 59), Some(10));
        assert_eq!(f.next_set_from(60, 59), None);
    }

    #[test]
    fn bitset_first_set() {
        let mut f = FieldBitset::empty();
        f.set(3);
        f.set(7);
        assert_eq!(f.first_set(0, 59), Some(3));
        assert_eq!(f.first_set(5, 59), Some(7));
    }

    // ---- Cron field parsing ----

    #[test]
    fn parse_wildcard() {
        let f = parse_field("*", 0, 59, no_names).unwrap();
        assert!(f.wildcard);
        assert!(f.matches(0));
        assert!(f.matches(59));
    }

    #[test]
    fn parse_single_value() {
        let f = parse_field("30", 0, 59, no_names).unwrap();
        assert!(f.matches(30));
        assert!(!f.matches(29));
    }

    #[test]
    fn parse_range() {
        let f = parse_field("10-15", 0, 59, no_names).unwrap();
        assert!(!f.matches(9));
        assert!(f.matches(10));
        assert!(f.matches(12));
        assert!(f.matches(15));
        assert!(!f.matches(16));
    }

    #[test]
    fn parse_list() {
        let f = parse_field("1,5,10,20", 0, 59, no_names).unwrap();
        assert!(f.matches(1));
        assert!(f.matches(5));
        assert!(f.matches(10));
        assert!(f.matches(20));
        assert!(!f.matches(2));
        assert!(!f.matches(15));
    }

    #[test]
    fn parse_step_from_wildcard() {
        // */5 in minute field (0-59) should match 0, 5, 10, ..., 55.
        let f = parse_field("*/5", 0, 59, no_names).unwrap();
        assert!(f.matches(0));
        assert!(f.matches(5));
        assert!(f.matches(10));
        assert!(f.matches(55));
        assert!(!f.matches(1));
        assert!(!f.matches(59));
    }

    #[test]
    fn parse_step_from_range() {
        // 1-10/2 should match 1, 3, 5, 7, 9
        let f = parse_field("1-10/2", 0, 59, no_names).unwrap();
        assert!(f.matches(1));
        assert!(f.matches(3));
        assert!(f.matches(5));
        assert!(f.matches(7));
        assert!(f.matches(9));
        assert!(!f.matches(0));
        assert!(!f.matches(2));
        assert!(!f.matches(10));
    }

    #[test]
    fn parse_step_from_value() {
        // 5/10 should match 5, 15, 25, 35, 45, 55
        let f = parse_field("5/10", 0, 59, no_names).unwrap();
        assert!(f.matches(5));
        assert!(f.matches(15));
        assert!(f.matches(25));
        assert!(!f.matches(0));
        assert!(!f.matches(10));
    }

    #[test]
    fn parse_named_months() {
        let f = parse_field("Jan", 1, 12, month_name_to_num).unwrap();
        assert!(f.matches(1));
        assert!(!f.matches(2));

        let f = parse_field("Dec", 1, 12, month_name_to_num).unwrap();
        assert!(f.matches(12));
    }

    #[test]
    fn parse_named_weekdays() {
        let f = parse_field("Mon", 0, 7, weekday_name_to_num).unwrap();
        assert!(f.matches(1));
        assert!(!f.matches(0));

        let f = parse_field("Sun", 0, 7, weekday_name_to_num).unwrap();
        assert!(f.matches(0));
    }

    #[test]
    fn parse_named_range() {
        // Mon-Fri
        let f = parse_field("Mon-Fri", 0, 7, weekday_name_to_num).unwrap();
        assert!(f.matches(1)); // Mon
        assert!(f.matches(3)); // Wed
        assert!(f.matches(5)); // Fri
        assert!(!f.matches(0)); // Sun
        assert!(!f.matches(6)); // Sat
    }

    #[test]
    fn parse_named_list() {
        let f = parse_field("Mon,Wed,Fri", 0, 7, weekday_name_to_num).unwrap();
        assert!(f.matches(1));
        assert!(f.matches(3));
        assert!(f.matches(5));
        assert!(!f.matches(2));
    }

    #[test]
    fn parse_zero_step_rejected() {
        assert!(parse_field("*/0", 0, 59, no_names).is_err());
    }

    #[test]
    fn parse_out_of_range_value() {
        assert!(parse_field("60", 0, 59, no_names).is_err());
    }

    #[test]
    fn parse_invalid_range_order() {
        assert!(parse_field("10-5", 0, 59, no_names).is_err());
    }

    // ---- CronExpr parsing and matching ----

    #[test]
    fn cron_expr_parse_simple() {
        let fields = ["0", "0", "1", "1", "*"];
        let expr = CronExpr::parse(&fields).unwrap();
        assert!(expr.minute.matches(0));
        assert!(!expr.minute.matches(1));
        assert!(expr.hour.matches(0));
        assert!(expr.dom.matches(1));
        assert!(expr.month.matches(1));
        assert!(expr.dow.wildcard);
    }

    #[test]
    fn cron_expr_matches_specific_time() {
        // 30 8 * * * => every day at 8:30
        let fields = ["30", "8", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        let t = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 8,
            minute: 30,
            second: 0,
            weekday: 1, // Monday
        };
        assert!(expr.matches_time(&t));

        let t2 = BrokenTime { minute: 31, ..t };
        assert!(!expr.matches_time(&t2));
    }

    #[test]
    fn cron_expr_wildcard_matches_all() {
        let fields = ["*", "*", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        let t = BrokenTime {
            year: 2026,
            month: 3,
            day: 15,
            hour: 14,
            minute: 42,
            second: 0,
            weekday: 0,
        };
        assert!(expr.matches_time(&t));
    }

    #[test]
    fn cron_expr_step_minutes() {
        // */15 * * * * => every 15 minutes
        let fields = ["*/15", "*", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        let mk = |min: u32| BrokenTime {
            year: 2026,
            month: 1,
            day: 1,
            hour: 12,
            minute: min,
            second: 0,
            weekday: 4,
        };

        assert!(expr.matches_time(&mk(0)));
        assert!(expr.matches_time(&mk(15)));
        assert!(expr.matches_time(&mk(30)));
        assert!(expr.matches_time(&mk(45)));
        assert!(!expr.matches_time(&mk(10)));
    }

    #[test]
    fn cron_expr_range_hours() {
        // 0 9-17 * * * => on the hour, 9am to 5pm
        let fields = ["0", "9-17", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        let mk = |hour: u32| BrokenTime {
            year: 2026,
            month: 6,
            day: 10,
            hour,
            minute: 0,
            second: 0,
            weekday: 3,
        };

        assert!(!expr.matches_time(&mk(8)));
        assert!(expr.matches_time(&mk(9)));
        assert!(expr.matches_time(&mk(12)));
        assert!(expr.matches_time(&mk(17)));
        assert!(!expr.matches_time(&mk(18)));
    }

    #[test]
    fn cron_expr_dom_dow_union() {
        // 0 0 15 * Fri => runs on the 15th OR on Fridays (union)
        let fields = ["0", "0", "15", "*", "Fri"];
        let expr = CronExpr::parse(&fields).unwrap();
        assert!(expr.dom_and_dow_restricted);

        // A Friday that is not the 15th.
        let fri_not_15 = BrokenTime {
            year: 2026,
            month: 5,
            day: 22, // Not the 15th
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 5, // Friday
        };
        assert!(expr.matches_time(&fri_not_15));

        // The 15th on a non-Friday.
        let the_15th = BrokenTime {
            year: 2026,
            month: 5,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 4, // Thursday (not Friday)
        };
        assert!(expr.matches_time(&the_15th));

        // Neither the 15th nor a Friday.
        let neither = BrokenTime {
            year: 2026,
            month: 5,
            day: 20,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 3, // Wednesday
        };
        assert!(!expr.matches_time(&neither));
    }

    #[test]
    fn cron_expr_only_dom_specified() {
        // 0 0 15 * * => only dom restricted, dow is wildcard
        let fields = ["0", "0", "15", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();
        assert!(!expr.dom_and_dow_restricted);

        let any_day = BrokenTime {
            year: 2026,
            month: 5,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 4,
        };
        assert!(expr.matches_time(&any_day));
    }

    // ---- Special strings ----

    #[test]
    fn special_reboot() {
        let (expr, is_reboot) = parse_special_string("@reboot").unwrap();
        assert!(is_reboot);
        assert!(expr.is_none());
    }

    #[test]
    fn special_yearly() {
        let (expr, is_reboot) = parse_special_string("@yearly").unwrap();
        assert!(!is_reboot);
        let expr = expr.unwrap();
        // Should match Jan 1 00:00.
        let jan1 = BrokenTime {
            year: 2026,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 4,
        };
        assert!(expr.matches_time(&jan1));

        let feb1 = BrokenTime { month: 2, ..jan1 };
        assert!(!expr.matches_time(&feb1));
    }

    #[test]
    fn special_annually() {
        let (expr, _) = parse_special_string("@annually").unwrap();
        let expr = expr.unwrap();
        let jan1 = BrokenTime {
            year: 2026,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 4,
        };
        assert!(expr.matches_time(&jan1));
    }

    #[test]
    fn special_monthly() {
        let (expr, _) = parse_special_string("@monthly").unwrap();
        let expr = expr.unwrap();
        let first = BrokenTime {
            year: 2026,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 1,
        };
        assert!(expr.matches_time(&first));

        let second = BrokenTime { day: 2, ..first };
        assert!(!expr.matches_time(&second));
    }

    #[test]
    fn special_weekly() {
        let (expr, _) = parse_special_string("@weekly").unwrap();
        let expr = expr.unwrap();
        let sunday = BrokenTime {
            year: 2026,
            month: 5,
            day: 3,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        assert!(expr.matches_time(&sunday));

        let monday = BrokenTime {
            weekday: 1,
            ..sunday
        };
        assert!(!expr.matches_time(&monday));
    }

    #[test]
    fn special_daily() {
        let (expr, _) = parse_special_string("@daily").unwrap();
        let expr = expr.unwrap();
        let midnight = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 1,
        };
        assert!(expr.matches_time(&midnight));

        let noon = BrokenTime {
            hour: 12,
            ..midnight
        };
        assert!(!expr.matches_time(&noon));
    }

    #[test]
    fn special_midnight() {
        let (expr, _) = parse_special_string("@midnight").unwrap();
        assert!(expr.is_some());
    }

    #[test]
    fn special_hourly() {
        let (expr, _) = parse_special_string("@hourly").unwrap();
        let expr = expr.unwrap();
        let on_hour = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 14,
            minute: 0,
            second: 0,
            weekday: 1,
        };
        assert!(expr.matches_time(&on_hour));

        let off_hour = BrokenTime {
            minute: 30,
            ..on_hour
        };
        assert!(!expr.matches_time(&off_hour));
    }

    #[test]
    fn special_unknown_rejected() {
        assert!(parse_special_string("@unknown").is_err());
    }

    // ---- Time conversion ----

    #[test]
    fn unix_to_broken_epoch() {
        let bt = unix_to_broken(0);
        assert_eq!(bt.year, 1970);
        assert_eq!(bt.month, 1);
        assert_eq!(bt.day, 1);
        assert_eq!(bt.hour, 0);
        assert_eq!(bt.minute, 0);
        assert_eq!(bt.weekday, 4); // Thursday
    }

    #[test]
    fn unix_to_broken_known_date() {
        // 2026-05-18 12:30:00 UTC
        // Calculate: from 1970 to 2026 is 56 years.
        // We test round-trip instead of exact seconds.
        let bt = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 12,
            minute: 30,
            second: 0,
            weekday: 0, // placeholder
        };
        let secs = broken_to_unix(&bt);
        let bt2 = unix_to_broken(secs);
        assert_eq!(bt2.year, 2026);
        assert_eq!(bt2.month, 5);
        assert_eq!(bt2.day, 18);
        assert_eq!(bt2.hour, 12);
        assert_eq!(bt2.minute, 30);
    }

    #[test]
    fn broken_to_unix_roundtrip() {
        let original = 1_700_000_000u64; // Nov 2023 approx
        let bt = unix_to_broken(original);
        let result = broken_to_unix(&bt);
        assert_eq!(original, result);
    }

    #[test]
    fn leap_year_checks() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn days_in_month_feb_leap() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 4), 30);
    }

    #[test]
    fn unix_to_broken_leap_day() {
        // Feb 29, 2024 00:00:00 UTC
        let bt = BrokenTime {
            year: 2024,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        let secs = broken_to_unix(&bt);
        let bt2 = unix_to_broken(secs);
        assert_eq!(bt2.year, 2024);
        assert_eq!(bt2.month, 2);
        assert_eq!(bt2.day, 29);
    }

    // ---- Environment variable parsing ----

    #[test]
    fn env_line_simple() {
        let result = try_parse_env_line("SHELL=/bin/bash");
        assert_eq!(result, Some(("SHELL".to_string(), "/bin/bash".to_string())));
    }

    #[test]
    fn env_line_with_quotes() {
        let result = try_parse_env_line("PATH=\"/usr/bin:/bin\"");
        assert_eq!(
            result,
            Some(("PATH".to_string(), "/usr/bin:/bin".to_string()))
        );
    }

    #[test]
    fn env_line_single_quotes() {
        let result = try_parse_env_line("MAILTO='user@example.com'");
        assert_eq!(
            result,
            Some(("MAILTO".to_string(), "user@example.com".to_string()))
        );
    }

    #[test]
    fn env_line_not_assignment() {
        // A cron line should not be parsed as env.
        assert!(try_parse_env_line("*/5 * * * * /bin/cmd").is_none());
    }

    #[test]
    fn env_line_empty_value() {
        let result = try_parse_env_line("EMPTY=");
        assert_eq!(result, Some(("EMPTY".to_string(), String::new())));
    }

    #[test]
    fn env_line_underscore_name() {
        let result = try_parse_env_line("_VAR=test");
        assert_eq!(result, Some(("_VAR".to_string(), "test".to_string())));
    }

    // ---- Crontab line parsing ----

    #[test]
    fn parse_user_crontab_line() {
        let (expr, is_reboot, cmd) = parse_crontab_line("30 8 * * * /bin/backup", false).unwrap();
        assert!(!is_reboot);
        assert!(expr.is_some());
        assert_eq!(cmd, "/bin/backup");
    }

    #[test]
    fn parse_system_crontab_line() {
        let (expr, is_reboot, _cmd) =
            parse_crontab_line("30 8 * * * root /bin/backup", true).unwrap();
        assert!(!is_reboot);
        assert!(expr.is_some());
    }

    #[test]
    fn parse_crontab_reboot_line() {
        let (expr, is_reboot, cmd) = parse_crontab_line("@reboot /bin/startup", false).unwrap();
        assert!(is_reboot);
        assert!(expr.is_none());
        assert_eq!(cmd, "/bin/startup");
    }

    #[test]
    fn parse_crontab_daily_line() {
        let (expr, is_reboot, cmd) = parse_crontab_line("@daily /bin/cleanup", false).unwrap();
        assert!(!is_reboot);
        assert!(expr.is_some());
        assert_eq!(cmd, "/bin/cleanup");
    }

    #[test]
    fn extract_user_system_line() {
        let (user, cmd) = extract_user_from_system_line("30 8 * * * root /bin/backup --all");
        assert_eq!(user, "root");
        assert_eq!(cmd, "/bin/backup --all");
    }

    #[test]
    fn extract_user_system_reboot() {
        let (user, cmd) = extract_user_from_system_line("@reboot root /bin/startup");
        assert_eq!(user, "root");
        assert_eq!(cmd, "/bin/startup");
    }

    // ---- Anacron entry parsing ----

    #[test]
    fn anacron_parse_valid() {
        let content = "1 5 daily-job /bin/daily-task\n7 10 weekly-job /bin/weekly-task\n";
        let tmpdir = std::env::temp_dir().join("crond2_test_anacron");
        let _ = fs::create_dir_all(&tmpdir);
        let path = tmpdir.join("anacrontab_test");
        fs::write(&path, content).unwrap();

        let tab = AnacronTab::load(&path).unwrap();
        assert_eq!(tab.entries.len(), 2);
        assert_eq!(tab.entries[0].period_days, 1);
        assert_eq!(tab.entries[0].delay_minutes, 5);
        assert_eq!(tab.entries[0].job_id, "daily-job");
        assert_eq!(tab.entries[0].command, "/bin/daily-task");
        assert_eq!(tab.entries[1].period_days, 7);

        let _ = fs::remove_dir_all(&tmpdir);
    }

    #[test]
    fn anacron_parse_with_comments_and_env() {
        let content = "# Comment line\nSHELL=/bin/bash\n\n1 5 test-job /bin/test\n";
        let tmpdir = std::env::temp_dir().join("crond2_test_anacron2");
        let _ = fs::create_dir_all(&tmpdir);
        let path = tmpdir.join("anacrontab_test2");
        fs::write(&path, content).unwrap();

        let tab = AnacronTab::load(&path).unwrap();
        assert_eq!(tab.entries.len(), 1);
        assert_eq!(tab.env_vars.get("SHELL"), Some(&"/bin/bash".to_string()));

        let _ = fs::remove_dir_all(&tmpdir);
    }

    #[test]
    fn anacron_parse_bad_period() {
        let content = "abc 5 test-job /bin/test\n";
        let tmpdir = std::env::temp_dir().join("crond2_test_anacron3");
        let _ = fs::create_dir_all(&tmpdir);
        let path = tmpdir.join("anacrontab_test3");
        fs::write(&path, content).unwrap();

        assert!(AnacronTab::load(&path).is_err());

        let _ = fs::remove_dir_all(&tmpdir);
    }

    // ---- Anacron timestamps ----

    #[test]
    fn anacron_timestamp_roundtrip() {
        let tmpdir = std::env::temp_dir().join("crond2_test_ts");
        let _ = fs::create_dir_all(&tmpdir);
        let ts_path = tmpdir.join("test-job-ts");

        // Write a YYYYMMDD timestamp file manually.
        fs::write(&ts_path, "20260518").unwrap();

        // Read it back via our parser.
        let content = fs::read_to_string(&ts_path).unwrap();
        let trimmed = content.trim();
        // Verify YYYYMMDD parsing logic.
        assert_eq!(trimmed.len(), 8);
        let y: i64 = trimmed[0..4].parse().unwrap();
        let m: u32 = trimmed[4..6].parse().unwrap();
        let d: u32 = trimmed[6..8].parse().unwrap();
        assert_eq!(y, 2026);
        assert_eq!(m, 5);
        assert_eq!(d, 18);

        let _ = fs::remove_dir_all(&tmpdir);
    }

    // ---- Next run time calculation ----

    #[test]
    fn next_match_every_minute() {
        let fields = ["*", "*", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        // From any time, the next match should be within one minute.
        let from = 1_700_000_000u64;
        let result = expr.next_match_after(from);
        assert!(result.is_some());
        let next = result.unwrap();
        assert!(next >= from);
        assert!(next <= from + 60);
    }

    #[test]
    fn next_match_specific_minute() {
        // 30 * * * * => at minute 30 of every hour
        let fields = ["30", "*", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        // Start from a time where minute = 0.
        let bt = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 10,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        let from = broken_to_unix(&bt);
        let next = expr.next_match_after(from).unwrap();
        let next_bt = unix_to_broken(next);
        assert_eq!(next_bt.minute, 30);
        assert_eq!(next_bt.hour, 10);
    }

    #[test]
    fn next_match_wraps_hour() {
        // 15 * * * * => at minute 15
        let fields = ["15", "*", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        // Start at minute 20, should wrap to next hour.
        let bt = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 10,
            minute: 20,
            second: 0,
            weekday: 0,
        };
        let from = broken_to_unix(&bt);
        let next = expr.next_match_after(from).unwrap();
        let next_bt = unix_to_broken(next);
        assert_eq!(next_bt.minute, 15);
        assert_eq!(next_bt.hour, 11);
    }

    #[test]
    fn next_match_specific_hour_and_minute() {
        // 0 3 * * * => at 3:00 every day
        let fields = ["0", "3", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        // Start at 10:00, should jump to 3:00 next day.
        let bt = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 10,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        let from = broken_to_unix(&bt);
        let next = expr.next_match_after(from).unwrap();
        let next_bt = unix_to_broken(next);
        assert_eq!(next_bt.hour, 3);
        assert_eq!(next_bt.minute, 0);
        assert_eq!(next_bt.day, 19);
    }

    #[test]
    fn next_match_monthly() {
        // 0 0 1 * * => midnight on the 1st of each month
        let fields = ["0", "0", "1", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        // Start mid-month.
        let bt = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        let from = broken_to_unix(&bt);
        let next = expr.next_match_after(from).unwrap();
        let next_bt = unix_to_broken(next);
        assert_eq!(next_bt.month, 6);
        assert_eq!(next_bt.day, 1);
    }

    #[test]
    fn next_match_exact_time() {
        // If "from" is exactly a matching time, it should return that time.
        let fields = ["0", "12", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        let bt = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 12,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        let from = broken_to_unix(&bt);
        let next = expr.next_match_after(from).unwrap();
        assert_eq!(next, from);
    }

    // ---- Edge cases ----

    #[test]
    fn midnight_boundary() {
        // 0 0 * * * => midnight every day
        let fields = ["0", "0", "*", "*", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        let bt = BrokenTime {
            year: 2026,
            month: 5,
            day: 18,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 1,
        };
        assert!(expr.matches_time(&bt));

        let bt2 = BrokenTime {
            hour: 0,
            minute: 1,
            ..bt
        };
        assert!(!expr.matches_time(&bt2));
    }

    #[test]
    fn month_boundary_december_to_january() {
        // Test that next_match wraps from Dec to Jan correctly.
        let fields = ["0", "0", "1", "1", "*"];
        let expr = CronExpr::parse(&fields).unwrap();

        // Start in December.
        let bt = BrokenTime {
            year: 2025,
            month: 12,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        let from = broken_to_unix(&bt);
        let next = expr.next_match_after(from).unwrap();
        let next_bt = unix_to_broken(next);
        assert_eq!(next_bt.year, 2026);
        assert_eq!(next_bt.month, 1);
        assert_eq!(next_bt.day, 1);
    }

    #[test]
    fn dow_7_equals_0() {
        // Day 7 should be treated same as day 0 (Sunday).
        let fields = ["0", "0", "*", "*", "7"];
        let expr = CronExpr::parse(&fields).unwrap();

        let sunday = BrokenTime {
            year: 2026,
            month: 5,
            day: 3,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 0, // Sunday
        };
        assert!(expr.matches_time(&sunday));
    }

    // ---- Argument parsing ----

    #[test]
    fn parse_crond_args_defaults() {
        let args: Vec<String> = vec![];
        let result = parse_crond_args(&args).unwrap();
        assert!(!result.foreground);
        assert_eq!(result.log_level, DEFAULT_LOG_LEVEL);
        assert!(!result.log_stdout);
    }

    #[test]
    fn parse_crond_args_all_flags() {
        let args: Vec<String> = vec![
            "-f".to_string(),
            "-l".to_string(),
            "-L".to_string(),
            "2".to_string(),
        ];
        let result = parse_crond_args(&args).unwrap();
        assert!(result.foreground);
        assert!(result.log_stdout);
        assert_eq!(result.log_level, 2);
    }

    #[test]
    fn parse_crond_args_bad_option() {
        let args: Vec<String> = vec!["--bogus".to_string()];
        assert!(parse_crond_args(&args).is_err());
    }

    #[test]
    fn parse_anacron_args_defaults() {
        let args: Vec<String> = vec![];
        let result = parse_anacron_args(&args).unwrap();
        assert!(!result.force);
        assert!(!result.update_only);
        assert!(!result.no_delay);
        assert!(!result.serialize);
        assert!(!result.foreground);
        assert!(!result.test_only);
    }

    #[test]
    fn parse_anacron_args_all_flags() {
        let args: Vec<String> = vec![
            "-f".to_string(),
            "-u".to_string(),
            "-n".to_string(),
            "-s".to_string(),
            "-d".to_string(),
            "-T".to_string(),
        ];
        let result = parse_anacron_args(&args).unwrap();
        assert!(result.force);
        assert!(result.update_only);
        assert!(result.no_delay);
        assert!(result.serialize);
        assert!(result.foreground);
        assert!(result.test_only);
    }

    #[test]
    fn parse_anacron_args_bad_option() {
        let args: Vec<String> = vec!["--bogus".to_string()];
        assert!(parse_anacron_args(&args).is_err());
    }
}
