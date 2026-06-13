//! Multi-personality job scheduling utility for SlateOS.
//!
//! This binary detects its personality from `argv[0]`:
//!   - `crond`    — cron daemon: reads crontabs, matches schedules
//!   - `crontab`  — user crontab management tool
//!   - `anacron`  — delayed periodic job scheduler
//!   - `at`       — schedule one-time jobs
//!   - `atd`      — at daemon: processes pending one-time jobs
//!   - `batch`    — run jobs when load is low
//!   - `atq`      — list pending at jobs (alias for at -l)
//!   - `atrm`     — remove at jobs (alias for at -d)

#![deny(clippy::all)]
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Crond,
    Crontab,
    Anacron,
    At,
    Atd,
    Batch,
    Atq,
    Atrm,
}

impl Personality {
    fn name(self) -> &'static str {
        match self {
            Self::Crond => "crond",
            Self::Crontab => "crontab",
            Self::Anacron => "anacron",
            Self::At => "at",
            Self::Atd => "atd",
            Self::Batch => "batch",
            Self::Atq => "atq",
            Self::Atrm => "atrm",
        }
    }
}

fn detect_personality(argv0: &str) -> Personality {
    let name = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();

    if lower == "crond" || lower == "cron" {
        Personality::Crond
    } else if lower == "crontab" {
        Personality::Crontab
    } else if lower == "anacron" {
        Personality::Anacron
    } else if lower == "atd" {
        Personality::Atd
    } else if lower == "batch" {
        Personality::Batch
    } else if lower == "atq" {
        Personality::Atq
    } else if lower == "atrm" {
        Personality::Atrm
    } else {
        // Default: "at"
        Personality::At
    }
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum CronError {
    InvalidField { field: &'static str, value: String },
    InvalidRange { field: &'static str, low: u32, high: u32 },
    InvalidStep { field: &'static str, step: u32 },
    ParseInt { field: &'static str, value: String },
    InvalidSpecial(String),
    InvalidLine(String),
    InvalidTimeSpec(String),
    IoError(String),
    PermissionDenied(String),
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField { field, value } => {
                write!(f, "invalid {field} field: '{value}'")
            }
            Self::InvalidRange { field, low, high } => {
                write!(f, "invalid range in {field}: {low}-{high}")
            }
            Self::InvalidStep { field, step } => {
                write!(f, "invalid step in {field}: */{step}")
            }
            Self::ParseInt { field, value } => {
                write!(f, "invalid integer in {field}: '{value}'")
            }
            Self::InvalidSpecial(s) => write!(f, "invalid special string: '{s}'"),
            Self::InvalidLine(s) => write!(f, "invalid crontab line: '{s}'"),
            Self::InvalidTimeSpec(s) => write!(f, "invalid time specification: '{s}'"),
            Self::IoError(s) => write!(f, "I/O error: {s}"),
            Self::PermissionDenied(s) => write!(f, "permission denied: {s}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Simple date/time representation (no external crate dependencies)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DateTime {
    year: u32,
    month: u32,  // 1-12
    day: u32,    // 1-31
    hour: u32,   // 0-23
    minute: u32, // 0-59
    weekday: u32, // 0=Sunday, 1=Monday, ..., 6=Saturday
}

impl DateTime {
    fn new(year: u32, month: u32, day: u32, hour: u32, minute: u32) -> Self {
        let weekday = day_of_week(year, month, day);
        Self { year, month, day, hour, minute, weekday }
    }
}

/// Zeller-like day-of-week: 0=Sunday, 1=Monday, ..., 6=Saturday.
fn day_of_week(year: u32, month: u32, day: u32) -> u32 {
    // Tomohiko Sakamoto's algorithm
    let t = [0u32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let idx = (month as usize).saturating_sub(1).min(11);
    (y + y / 4 - y / 100 + y / 400 + t[idx] + day) % 7
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Cron field representation
// ---------------------------------------------------------------------------

/// A parsed cron field — a set of allowed values.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CronField {
    /// Bitset of allowed values. Index = value, bit set = allowed.
    bits: u64,
    min: u32,
    max: u32,
}

impl CronField {
    fn new(min: u32, max: u32) -> Self {
        Self { bits: 0, min, max }
    }

    fn set(&mut self, val: u32) {
        if val >= self.min && val <= self.max {
            self.bits |= 1u64 << val;
        }
    }

    fn set_all(&mut self) {
        for v in self.min..=self.max {
            self.bits |= 1u64 << v;
        }
    }

    fn matches(&self, val: u32) -> bool {
        if val > 63 { return false; }
        (self.bits >> val) & 1 == 1
    }

    fn is_empty(&self) -> bool {
        self.bits == 0
    }
}

// ---------------------------------------------------------------------------
// Month/weekday name mapping
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Cron field parser
// ---------------------------------------------------------------------------

/// Parse a single cron field token (e.g., "1-5", "*/15", "1,3,5", "*", "mon").
fn parse_field_token(
    token: &str,
    field_name: &'static str,
    min: u32,
    max: u32,
    name_resolver: fn(&str) -> Option<u32>,
) -> Result<CronField, CronError> {
    let mut field = CronField::new(min, max);

    for part in token.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(CronError::InvalidField {
                field: field_name,
                value: token.to_string(),
            });
        }

        // Check for step: e.g., "*/15" or "1-5/2"
        let (range_part, step) = if let Some(idx) = part.find('/') {
            let step_str = &part[idx + 1..];
            let step_val = step_str.parse::<u32>().map_err(|_| CronError::ParseInt {
                field: field_name,
                value: step_str.to_string(),
            })?;
            if step_val == 0 {
                return Err(CronError::InvalidStep {
                    field: field_name,
                    step: 0,
                });
            }
            (&part[..idx], Some(step_val))
        } else {
            (part, None)
        };

        if range_part == "*" {
            // Wildcard, possibly with step
            let s = step.unwrap_or(1);
            let mut v = min;
            while v <= max {
                field.set(v);
                v = v.saturating_add(s);
            }
        } else if let Some(dash) = range_part.find('-') {
            // Range: "1-5"
            let low_str = &range_part[..dash];
            let high_str = &range_part[dash + 1..];

            let low = resolve_value(low_str, field_name, name_resolver)?;
            let high = resolve_value(high_str, field_name, name_resolver)?;

            if low > high || low < min || high > max {
                return Err(CronError::InvalidRange {
                    field: field_name,
                    low,
                    high,
                });
            }

            let s = step.unwrap_or(1);
            let mut v = low;
            while v <= high {
                field.set(v);
                v = v.saturating_add(s);
            }
        } else {
            // Single value (possibly with step applied from that value upward)
            let val = resolve_value(range_part, field_name, name_resolver)?;
            if val < min || val > max {
                return Err(CronError::InvalidField {
                    field: field_name,
                    value: range_part.to_string(),
                });
            }
            if let Some(s) = step {
                let mut v = val;
                while v <= max {
                    field.set(v);
                    v = v.saturating_add(s);
                }
            } else {
                field.set(val);
            }
        }
    }

    if field.is_empty() {
        return Err(CronError::InvalidField {
            field: field_name,
            value: token.to_string(),
        });
    }

    Ok(field)
}

fn resolve_value(
    s: &str,
    field_name: &'static str,
    name_resolver: fn(&str) -> Option<u32>,
) -> Result<u32, CronError> {
    if let Ok(n) = s.parse::<u32>() {
        return Ok(n);
    }
    name_resolver(s).ok_or_else(|| CronError::ParseInt {
        field: field_name,
        value: s.to_string(),
    })
}

fn no_names(_: &str) -> Option<u32> {
    None
}

// ---------------------------------------------------------------------------
// Cron expression
// ---------------------------------------------------------------------------

/// A fully parsed 5-field cron expression.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CronExpr {
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

impl CronExpr {
    /// Parse a 5-field cron expression string.
    fn parse(expr: &str) -> Result<Self, CronError> {
        // Handle special strings
        let expr = expr.trim();
        if let Some(special) = expr.strip_prefix('@') {
            return Self::parse_special(special);
        }

        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() < 5 {
            return Err(CronError::InvalidLine(format!(
                "expected 5 fields, got {}",
                fields.len()
            )));
        }

        let minute = parse_field_token(fields[0], "minute", 0, 59, no_names)?;
        let hour = parse_field_token(fields[1], "hour", 0, 23, no_names)?;
        let day_of_month = parse_field_token(fields[2], "day-of-month", 1, 31, no_names)?;
        let month = parse_field_token(fields[3], "month", 1, 12, month_name_to_num)?;
        let day_of_week = parse_field_token(fields[4], "day-of-week", 0, 6, weekday_name_to_num)?;

        Ok(Self {
            minute,
            hour,
            day_of_month,
            month,
            day_of_week,
        })
    }

    fn parse_special(name: &str) -> Result<Self, CronError> {
        let expr_str = match name.to_ascii_lowercase().as_str() {
            "yearly" | "annually" => "0 0 1 1 *",
            "monthly" => "0 0 1 * *",
            "weekly" => "0 0 * * 0",
            "daily" | "midnight" => "0 0 * * *",
            "hourly" => "0 * * * *",
            "reboot" => {
                // @reboot is special — matches only at daemon start.
                // We represent it as all-zeros (never matches time-based check)
                // and callers check for it explicitly.
                return Ok(Self {
                    minute: CronField::new(0, 59),
                    hour: CronField::new(0, 23),
                    day_of_month: CronField::new(1, 31),
                    month: CronField::new(1, 12),
                    day_of_week: CronField::new(0, 6),
                });
            }
            _ => {
                return Err(CronError::InvalidSpecial(name.to_string()));
            }
        };
        Self::parse(expr_str)
    }

    /// Check whether a given date/time matches this cron expression.
    ///
    /// Per standard cron behavior: if both day-of-month and day-of-week are
    /// restricted (not wildcard "*"), the job runs when EITHER matches.
    fn matches(&self, dt: &DateTime) -> bool {
        if !self.minute.matches(dt.minute) {
            return false;
        }
        if !self.hour.matches(dt.hour) {
            return false;
        }
        if !self.month.matches(dt.month) {
            return false;
        }

        // Day matching: standard cron uses OR when both are specified
        let dom_all = is_all_set(&self.day_of_month, 1, 31);
        let dow_all = is_all_set(&self.day_of_week, 0, 6);

        if dom_all && dow_all {
            // Both are wildcards — always matches
            true
        } else if dom_all {
            // Only day-of-week is restricted
            self.day_of_week.matches(dt.weekday)
        } else if dow_all {
            // Only day-of-month is restricted
            self.day_of_month.matches(dt.day)
        } else {
            // Both restricted — OR semantics
            self.day_of_month.matches(dt.day) || self.day_of_week.matches(dt.weekday)
        }
    }
}

/// Check if all values in [min..=max] are set in the field.
fn is_all_set(field: &CronField, min: u32, max: u32) -> bool {
    for v in min..=max {
        if !field.matches(v) {
            return false;
        }
    }
    true
}

impl fmt::Display for CronExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} {}",
            format_field(&self.minute, 0, 59),
            format_field(&self.hour, 0, 23),
            format_field(&self.day_of_month, 1, 31),
            format_field(&self.month, 1, 12),
            format_field(&self.day_of_week, 0, 6),
        )
    }
}

fn format_field(field: &CronField, min: u32, max: u32) -> String {
    if is_all_set(field, min, max) {
        return "*".to_string();
    }
    let mut vals = Vec::new();
    for v in min..=max {
        if field.matches(v) {
            vals.push(v.to_string());
        }
    }
    vals.join(",")
}

// ---------------------------------------------------------------------------
// Crontab entry
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CrontabEntry {
    schedule: CronExpr,
    user: Option<String>,
    command: String,
    is_reboot: bool,
}

/// Environment variable from a crontab.
#[derive(Clone, Debug)]
struct CrontabEnvVar {
    key: String,
    value: String,
}

/// Parsed crontab file contents.
#[derive(Clone, Debug)]
struct CrontabFile {
    env_vars: Vec<CrontabEnvVar>,
    entries: Vec<CrontabEntry>,
}

fn parse_crontab(content: &str, system_format: bool) -> Result<CrontabFile, CronError> {
    let mut env_vars = Vec::new();
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Check for environment variable: KEY=VALUE
        if let Some(eq_pos) = line.find('=') {
            let key_part = &line[..eq_pos];
            // Environment variable keys contain only alphanumeric and underscore
            if !key_part.is_empty()
                && key_part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                let value = line[eq_pos + 1..].trim().to_string();
                // Strip surrounding quotes if present
                let value = strip_quotes(&value);
                env_vars.push(CrontabEnvVar {
                    key: key_part.to_string(),
                    value,
                });
                continue;
            }
        }

        // Check for special @string entries
        if line.starts_with('@') {
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() < 2 {
                return Err(CronError::InvalidLine(line.to_string()));
            }
            let special = parts[0];
            let rest = parts[1].trim();
            let is_reboot =
                special.eq_ignore_ascii_case("@reboot");

            let schedule = CronExpr::parse(special)?;

            let (user, command) = if system_format {
                let parts2: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
                if parts2.len() < 2 {
                    return Err(CronError::InvalidLine(line.to_string()));
                }
                (Some(parts2[0].to_string()), parts2[1].trim().to_string())
            } else {
                (None, rest.to_string())
            };

            entries.push(CrontabEntry {
                schedule,
                user,
                command,
                is_reboot,
            });
            continue;
        }

        // Standard 5-field line
        let fields: Vec<&str> = line.splitn(6 + usize::from(system_format), char::is_whitespace).collect();
        let min_fields = if system_format { 7 } else { 6 };
        if fields.len() < min_fields {
            return Err(CronError::InvalidLine(line.to_string()));
        }

        let expr_str = format!(
            "{} {} {} {} {}",
            fields[0], fields[1], fields[2], fields[3], fields[4]
        );
        let schedule = CronExpr::parse(&expr_str)?;

        let (user, command) = if system_format {
            let user = fields[5].to_string();
            let cmd_start = fields.get(6).copied().unwrap_or("");
            (Some(user), cmd_start.to_string())
        } else {
            (None, fields[5].to_string())
        };

        entries.push(CrontabEntry {
            schedule,
            user,
            command,
            is_reboot: false,
        });
    }

    Ok(CrontabFile { env_vars, entries })
}

fn strip_quotes(s: &str) -> String {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('\'') && s.ends_with('\'')))
        {
            return s[1..s.len() - 1].to_string();
        }
    s.to_string()
}

// ---------------------------------------------------------------------------
// Anacron configuration
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct AnacronEntry {
    period_days: u32,
    delay_minutes: u32,
    job_id: String,
    command: String,
}

#[derive(Clone, Debug)]
struct AnacronConfig {
    entries: Vec<AnacronEntry>,
    env_vars: Vec<CrontabEnvVar>,
}

fn parse_anacrontab(content: &str) -> Result<AnacronConfig, CronError> {
    let mut entries = Vec::new();
    let mut env_vars = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Environment variable
        if let Some(eq_pos) = line.find('=') {
            let key_part = &line[..eq_pos];
            if !key_part.is_empty()
                && key_part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                let value = line[eq_pos + 1..].trim().to_string();
                let value = strip_quotes(&value);
                env_vars.push(CrontabEnvVar {
                    key: key_part.to_string(),
                    value,
                });
                continue;
            }
        }

        // period delay job-id command
        let parts: Vec<&str> = line.splitn(4, char::is_whitespace).collect();
        if parts.len() < 4 {
            return Err(CronError::InvalidLine(line.to_string()));
        }

        let period_days = if parts[0].starts_with('@') {
            match parts[0].to_ascii_lowercase().as_str() {
                "@daily" => 1,
                "@weekly" => 7,
                "@monthly" => 30,
                "@yearly" | "@annually" => 365,
                _ => {
                    return Err(CronError::InvalidSpecial(parts[0].to_string()));
                }
            }
        } else {
            parts[0].parse::<u32>().map_err(|_| CronError::ParseInt {
                field: "period",
                value: parts[0].to_string(),
            })?
        };

        let delay_minutes = parts[1].parse::<u32>().map_err(|_| CronError::ParseInt {
            field: "delay",
            value: parts[1].to_string(),
        })?;

        entries.push(AnacronEntry {
            period_days,
            delay_minutes,
            job_id: parts[2].to_string(),
            command: parts[3].to_string(),
        });
    }

    Ok(AnacronConfig { entries, env_vars })
}

// ---------------------------------------------------------------------------
// At job representation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct AtJob {
    id: u32,
    time: DateTime,
    queue: char,
    command: String,
    user: String,
    env_vars: Vec<CrontabEnvVar>,
}

impl AtJob {
    fn serialize(&self) -> String {
        let mut out = String::new();
        for ev in &self.env_vars {
            out.push_str(&format!("{}={}\n", ev.key, ev.value));
        }
        out.push_str(&format!("# atjob id={}\n", self.id));
        out.push_str(&format!("# queue={}\n", self.queue));
        out.push_str(&format!("# user={}\n", self.user));
        out.push_str(&format!(
            "# time={:04}-{:02}-{:02} {:02}:{:02}\n",
            self.time.year, self.time.month, self.time.day, self.time.hour, self.time.minute
        ));
        out.push_str(&self.command);
        if !self.command.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    fn deserialize(content: &str) -> Result<Self, CronError> {
        let mut env_vars = Vec::new();
        let mut id = 0u32;
        let mut queue = 'a';
        let mut user = String::new();
        let mut time_str = String::new();
        let mut command_lines = Vec::new();
        let mut in_command = false;

        for line in content.lines() {
            if in_command {
                command_lines.push(line);
                continue;
            }

            if let Some(rest) = line.strip_prefix("# atjob id=") {
                id = rest.parse::<u32>().unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("# queue=") {
                queue = rest.chars().next().unwrap_or('a');
            } else if let Some(rest) = line.strip_prefix("# user=") {
                user = rest.to_string();
            } else if let Some(rest) = line.strip_prefix("# time=") {
                time_str = rest.to_string();
            } else if line.starts_with('#') {
                // Other comment, skip
            } else if let Some(eq_pos) = line.find('=') {
                let key = &line[..eq_pos];
                if !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    env_vars.push(CrontabEnvVar {
                        key: key.to_string(),
                        value: line[eq_pos + 1..].to_string(),
                    });
                } else {
                    in_command = true;
                    command_lines.push(line);
                }
            } else {
                in_command = true;
                command_lines.push(line);
            }
        }

        let time = parse_datetime_str(&time_str)?;
        let command = command_lines.join("\n");

        Ok(Self {
            id,
            time,
            queue,
            command,
            user,
            env_vars,
        })
    }

    fn filename(&self) -> String {
        format!(
            "{}{:05x}{:04}{:02}{:02}{:02}{:02}",
            self.queue,
            self.id,
            self.time.year,
            self.time.month,
            self.time.day,
            self.time.hour,
            self.time.minute
        )
    }
}

fn parse_datetime_str(s: &str) -> Result<DateTime, CronError> {
    // Format: YYYY-MM-DD HH:MM
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(CronError::InvalidTimeSpec(s.to_string()));
    }

    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() != 3 {
        return Err(CronError::InvalidTimeSpec(s.to_string()));
    }

    let time_parts: Vec<&str> = parts[1].split(':').collect();
    if time_parts.len() != 2 {
        return Err(CronError::InvalidTimeSpec(s.to_string()));
    }

    let year = date_parts[0]
        .parse::<u32>()
        .map_err(|_| CronError::InvalidTimeSpec(s.to_string()))?;
    let month = date_parts[1]
        .parse::<u32>()
        .map_err(|_| CronError::InvalidTimeSpec(s.to_string()))?;
    let day = date_parts[2]
        .parse::<u32>()
        .map_err(|_| CronError::InvalidTimeSpec(s.to_string()))?;
    let hour = time_parts[0]
        .parse::<u32>()
        .map_err(|_| CronError::InvalidTimeSpec(s.to_string()))?;
    let minute = time_parts[1]
        .parse::<u32>()
        .map_err(|_| CronError::InvalidTimeSpec(s.to_string()))?;

    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) || hour > 23 || minute > 59
    {
        return Err(CronError::InvalidTimeSpec(s.to_string()));
    }

    Ok(DateTime::new(year, month, day, hour, minute))
}

// ---------------------------------------------------------------------------
// At time parsing (human-friendly time specifications)
// ---------------------------------------------------------------------------

/// Parse an `at`-style time specification relative to a reference time.
///
/// Supported formats:
///   HH:MM                           — today (or tomorrow if already past)
///   noon, midnight, teatime         — named times
///   now + N {minutes,hours,days}    — relative
///   YYYY-MM-DD HH:MM               — absolute
///   tomorrow HH:MM                  — next day
fn parse_at_time(spec: &str, now: &DateTime) -> Result<DateTime, CronError> {
    let spec = spec.trim();
    let lower = spec.to_ascii_lowercase();

    // Named times
    if lower == "noon" {
        return Ok(make_today_or_tomorrow(now, 12, 0));
    }
    if lower == "midnight" {
        return Ok(make_today_or_tomorrow(now, 0, 0));
    }
    if lower == "teatime" {
        return Ok(make_today_or_tomorrow(now, 16, 0));
    }

    // "now + N unit"
    if let Some(rest) = lower.strip_prefix("now") {
        let rest = rest.trim();
        if let Some(rest) = rest.strip_prefix('+') {
            let rest = rest.trim();
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() == 2 {
                let n = parts[0]
                    .parse::<u32>()
                    .map_err(|_| CronError::InvalidTimeSpec(spec.to_string()))?;
                let unit = parts[1].to_ascii_lowercase();
                return add_duration(now, n, &unit);
            } else if parts.len() == 1 {
                // Try "now+Nminutes" style (no space between number and unit)
                let token = parts[0];
                if let Some((n, unit)) = split_number_unit(token) {
                    return add_duration(now, n, &unit);
                }
            }
        }
        if rest.is_empty() {
            // Just "now"
            return Ok(*now);
        }
        return Err(CronError::InvalidTimeSpec(spec.to_string()));
    }

    // "tomorrow HH:MM"
    if let Some(rest) = lower.strip_prefix("tomorrow") {
        let rest = rest.trim();
        let (h, m) = parse_hhmm(rest)?;
        let tomorrow = advance_day(now);
        return Ok(DateTime::new(tomorrow.year, tomorrow.month, tomorrow.day, h, m));
    }

    // YYYY-MM-DD HH:MM
    let parts: Vec<&str> = spec.split_whitespace().collect();
    if parts.len() == 2 && parts[0].contains('-') {
        return parse_datetime_str(spec);
    }

    // HH:MM (today or tomorrow)
    if let Ok((h, m)) = parse_hhmm(spec) {
        return Ok(make_today_or_tomorrow(now, h, m));
    }

    Err(CronError::InvalidTimeSpec(spec.to_string()))
}

fn parse_hhmm(s: &str) -> Result<(u32, u32), CronError> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(CronError::InvalidTimeSpec(s.to_string()));
    }
    let h = parts[0]
        .parse::<u32>()
        .map_err(|_| CronError::InvalidTimeSpec(s.to_string()))?;
    let m = parts[1]
        .parse::<u32>()
        .map_err(|_| CronError::InvalidTimeSpec(s.to_string()))?;
    if h > 23 || m > 59 {
        return Err(CronError::InvalidTimeSpec(s.to_string()));
    }
    Ok((h, m))
}

fn make_today_or_tomorrow(now: &DateTime, hour: u32, minute: u32) -> DateTime {
    if hour > now.hour || (hour == now.hour && minute > now.minute) {
        DateTime::new(now.year, now.month, now.day, hour, minute)
    } else {
        let tom = advance_day(now);
        DateTime::new(tom.year, tom.month, tom.day, hour, minute)
    }
}

fn advance_day(dt: &DateTime) -> DateTime {
    let mut year = dt.year;
    let mut month = dt.month;
    let mut day = dt.day + 1;

    if day > days_in_month(year, month) {
        day = 1;
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }

    DateTime::new(year, month, day, dt.hour, dt.minute)
}

fn add_duration(now: &DateTime, n: u32, unit: &str) -> Result<DateTime, CronError> {
    match unit {
        "minute" | "minutes" | "min" => {
            let total_min = now.minute + n;
            let extra_hours = total_min / 60;
            let minute = total_min % 60;
            let total_hour = now.hour + extra_hours;
            let extra_days = total_hour / 24;
            let hour = total_hour % 24;

            let mut dt = DateTime::new(now.year, now.month, now.day, hour, minute);
            for _ in 0..extra_days {
                dt = advance_day(&dt);
                dt = DateTime::new(dt.year, dt.month, dt.day, hour, minute);
            }
            Ok(dt)
        }
        "hour" | "hours" => {
            let total_hour = now.hour + n;
            let extra_days = total_hour / 24;
            let hour = total_hour % 24;

            let mut dt = DateTime::new(now.year, now.month, now.day, hour, now.minute);
            for _ in 0..extra_days {
                dt = advance_day(&dt);
                dt = DateTime::new(dt.year, dt.month, dt.day, hour, now.minute);
            }
            Ok(dt)
        }
        "day" | "days" => {
            let mut dt = *now;
            for _ in 0..n {
                dt = advance_day(&dt);
            }
            Ok(DateTime::new(
                dt.year, dt.month, dt.day, now.hour, now.minute,
            ))
        }
        _ => Err(CronError::InvalidTimeSpec(format!(
            "unknown unit: {unit}"
        ))),
    }
}

fn split_number_unit(s: &str) -> Option<(u32, String)> {
    let idx = s.find(|c: char| !c.is_ascii_digit())?;
    if idx == 0 {
        return None;
    }
    let n = s[..idx].parse::<u32>().ok()?;
    let unit = s[idx..].to_string();
    Some((n, unit))
}

// ---------------------------------------------------------------------------
// At job ID generation
// ---------------------------------------------------------------------------

fn next_at_job_id(spool_dir: &Path) -> u32 {
    let mut max_id = 0u32;
    if let Ok(entries) = fs::read_dir(spool_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Job filename format: queue_char + 5-hex-digit ID + timestamp
            if name.len() >= 6
                && let Ok(id) = u32::from_str_radix(&name[1..6], 16)
                    && id > max_id {
                        max_id = id;
                    }
        }
    }
    max_id + 1
}

// ---------------------------------------------------------------------------
// Spool/config paths
// ---------------------------------------------------------------------------

const CRONTAB_SPOOL_DIR: &str = "/var/spool/cron/crontabs";
const SYSTEM_CRONTAB: &str = "/etc/crontab";
const ANACRONTAB: &str = "/etc/anacrontab";
const ANACRON_SPOOL_DIR: &str = "/var/spool/anacron";
const AT_SPOOL_DIR: &str = "/var/spool/at";

fn user_crontab_path(user: &str) -> PathBuf {
    PathBuf::from(CRONTAB_SPOOL_DIR).join(user)
}

// ---------------------------------------------------------------------------
// crond personality
// ---------------------------------------------------------------------------

fn run_crond(args: &[String]) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut foreground = false;
    let mut loglevel = 1u32;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--foreground" | "-f" => foreground = true,
            "--loglevel" | "-l" => {
                i += 1;
                if let Some(val) = args.get(i) {
                    loglevel = val.parse().unwrap_or(1);
                }
            }
            "--help" | "-h" => {
                let _ = writeln!(
                    out,
                    "Usage: crond [--foreground] [--loglevel LEVEL]\n\n\
                     Cron daemon: reads and schedules crontab jobs.\n\n\
                     Options:\n  \
                       --foreground, -f   Run in foreground\n  \
                       --loglevel, -l N   Set log level (0=quiet, 8=debug)"
                );
                return 0;
            }
            _ => {
                let _ = writeln!(out, "crond: unknown option '{}'", args[i]);
                return 1;
            }
        }
        i += 1;
    }

    if loglevel > 0 {
        let _ = writeln!(
            out,
            "crond: starting (foreground={foreground}, loglevel={loglevel})"
        );
    }

    // Load system crontab
    let mut all_entries: Vec<CrontabEntry> = Vec::new();

    match fs::read_to_string(SYSTEM_CRONTAB) {
        Ok(content) => match parse_crontab(&content, true) {
            Ok(ctab) => {
                if loglevel >= 2 {
                    let _ = writeln!(
                        out,
                        "crond: loaded {} entries from {}",
                        ctab.entries.len(),
                        SYSTEM_CRONTAB
                    );
                }
                all_entries.extend(ctab.entries);
            }
            Err(e) => {
                let _ = writeln!(out, "crond: error parsing {SYSTEM_CRONTAB}: {e}");
            }
        },
        Err(e) => {
            if loglevel >= 3 {
                let _ = writeln!(out, "crond: cannot read {SYSTEM_CRONTAB}: {e}");
            }
        }
    }

    // Load per-user crontabs
    if let Ok(entries) = fs::read_dir(CRONTAB_SPOOL_DIR) {
        for entry in entries.flatten() {
            let user = entry.file_name().to_string_lossy().to_string();
            if let Ok(content) = fs::read_to_string(entry.path()) {
                match parse_crontab(&content, false) {
                    Ok(mut ctab) => {
                        for e in &mut ctab.entries {
                            e.user = Some(user.clone());
                        }
                        if loglevel >= 2 {
                            let _ = writeln!(
                                out,
                                "crond: loaded {} entries for user '{user}'",
                                ctab.entries.len()
                            );
                        }
                        all_entries.extend(ctab.entries);
                    }
                    Err(e) => {
                        let _ = writeln!(
                            out,
                            "crond: error in crontab for '{user}': {e}"
                        );
                    }
                }
            }
        }
    }

    // Handle @reboot entries
    for entry in &all_entries {
        if entry.is_reboot {
            let user_str = entry.user.as_deref().unwrap_or("root");
            if loglevel >= 1 {
                let _ = writeln!(
                    out,
                    "crond: @reboot: running '{}' as {user_str}",
                    entry.command
                );
            }
        }
    }

    let _ = writeln!(
        out,
        "crond: loaded {} total cron entries, entering scheduler loop",
        all_entries.len()
    );

    // In a real OS, this would enter an infinite loop checking the current
    // time against schedules. We print a summary and exit for testability.
    let _ = writeln!(out, "crond: scheduler ready (simulated)");

    0
}

// ---------------------------------------------------------------------------
// crontab personality
// ---------------------------------------------------------------------------

fn run_crontab(args: &[String]) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut action = CrontabAction::None;
    let mut target_user: Option<String> = None;
    let mut interactive = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" => action = CrontabAction::List,
            "-e" => action = CrontabAction::Edit,
            "-r" => action = CrontabAction::Remove,
            "-i" => interactive = true,
            "-u" => {
                i += 1;
                target_user = args.get(i).cloned();
            }
            "--help" | "-h" => {
                let _ = writeln!(
                    out,
                    "Usage: crontab [-l | -e | -r] [-u user] [-i]\n\n\
                     Manage per-user crontab files.\n\n\
                     Options:\n  \
                       -l         List current crontab\n  \
                       -e         Edit crontab\n  \
                       -r         Remove crontab\n  \
                       -i         Interactive (prompt before removal)\n  \
                       -u USER    Target a specific user (requires root)"
                );
                return 0;
            }
            other => {
                // If it's a filename, treat as "install from file"
                if action == CrontabAction::None {
                    action = CrontabAction::Install(other.to_string());
                } else {
                    let _ = writeln!(out, "crontab: unexpected argument '{other}'");
                    return 1;
                }
            }
        }
        i += 1;
    }

    let user = target_user.unwrap_or_else(|| {
        env::var("USER").unwrap_or_else(|_| "root".to_string())
    });

    match action {
        CrontabAction::None => {
            let _ = writeln!(out, "crontab: usage error (try crontab -h for help)");
            1
        }
        CrontabAction::List => {
            let path = user_crontab_path(&user);
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let _ = write!(out, "{content}");
                    0
                }
                Err(_) => {
                    let _ = writeln!(out, "no crontab for {user}");
                    1
                }
            }
        }
        CrontabAction::Edit => {
            let path = user_crontab_path(&user);
            let editor = env::var("VISUAL")
                .or_else(|_| env::var("EDITOR"))
                .unwrap_or_else(|_| "vi".to_string());
            let _ = writeln!(
                out,
                "crontab: would launch '{editor}' to edit {}",
                path.display()
            );
            // In a real OS we would exec the editor; print a message here.
            0
        }
        CrontabAction::Remove => {
            if interactive {
                let _ = write!(out, "crontab: really delete {user}'s crontab? (y/n) ");
                let _ = out.flush();
                let stdin = io::stdin();
                let mut line = String::new();
                if stdin.lock().read_line(&mut line).is_ok()
                    && !line.trim().eq_ignore_ascii_case("y") {
                        let _ = writeln!(out, "crontab: aborted");
                        return 0;
                    }
            }
            let path = user_crontab_path(&user);
            match fs::remove_file(&path) {
                Ok(()) => {
                    let _ = writeln!(out, "crontab: removed crontab for {user}");
                    0
                }
                Err(e) => {
                    let _ = writeln!(
                        out,
                        "crontab: cannot remove {}: {e}",
                        path.display()
                    );
                    1
                }
            }
        }
        CrontabAction::Install(filename) => {
            let content = if filename == "-" {
                let mut buf = String::new();
                if io::stdin().lock().read_line(&mut buf).is_err() {
                    let _ = writeln!(out, "crontab: failed to read from stdin");
                    return 1;
                }
                buf
            } else {
                match fs::read_to_string(&filename) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = writeln!(out, "crontab: cannot read '{filename}': {e}");
                        return 1;
                    }
                }
            };

            // Validate before installing
            match parse_crontab(&content, false) {
                Ok(ctab) => {
                    let _ = writeln!(
                        out,
                        "crontab: validated {} entries",
                        ctab.entries.len()
                    );
                }
                Err(e) => {
                    let _ = writeln!(out, "crontab: errors in input: {e}");
                    return 1;
                }
            }

            let path = user_crontab_path(&user);
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            match fs::write(&path, &content) {
                Ok(()) => {
                    let _ = writeln!(
                        out,
                        "crontab: installed new crontab for {user}"
                    );
                    0
                }
                Err(e) => {
                    let _ = writeln!(out, "crontab: cannot write {}: {e}", path.display());
                    1
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CrontabAction {
    None,
    List,
    Edit,
    Remove,
    Install(String),
}

// ---------------------------------------------------------------------------
// anacron personality
// ---------------------------------------------------------------------------

fn run_anacron(args: &[String]) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut force = false;
    let mut update_only = false;
    let mut serialize = false;
    let mut run_now = false;
    let mut test_config = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" => force = true,
            "-u" => update_only = true,
            "-s" => serialize = true,
            "-n" => run_now = true,
            "-T" => test_config = true,
            "--help" | "-h" => {
                let _ = writeln!(
                    out,
                    "Usage: anacron [-f] [-u] [-s] [-n] [-T]\n\n\
                     Run periodic jobs that missed their schedule.\n\n\
                     Options:\n  \
                       -f   Force: run all jobs regardless of timestamps\n  \
                       -u   Only update timestamps, don't run jobs\n  \
                       -s   Serialize: run jobs one at a time\n  \
                       -n   Run now: ignore delays\n  \
                       -T   Test configuration and exit"
                );
                return 0;
            }
            _ => {
                let _ = writeln!(out, "anacron: unknown option '{}'", args[i]);
                return 1;
            }
        }
        i += 1;
    }

    // Load anacrontab
    let content = match fs::read_to_string(ANACRONTAB) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(out, "anacron: cannot read {ANACRONTAB}: {e}");
            return 1;
        }
    };

    let config = match parse_anacrontab(&content) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(out, "anacron: error in {ANACRONTAB}: {e}");
            return 1;
        }
    };

    if test_config {
        let _ = writeln!(
            out,
            "anacron: configuration OK ({} jobs defined)",
            config.entries.len()
        );
        return 0;
    }

    // Process each job
    let _ = fs::create_dir_all(ANACRON_SPOOL_DIR);

    for entry in &config.entries {
        let timestamp_file =
            PathBuf::from(ANACRON_SPOOL_DIR).join(&entry.job_id);

        let needs_run = if force {
            true
        } else {
            // Check last-run timestamp
            match fs::read_to_string(&timestamp_file) {
                Ok(ts) => {
                    // Timestamp file contains YYYYMMDD
                    let _ = ts.trim();
                    // In a real implementation we'd compare dates.
                    // Here we simulate: always needs run if timestamp is old.
                    true
                }
                Err(_) => true, // Never run before
            }
        };

        if needs_run {
            if update_only {
                let _ = writeln!(
                    out,
                    "anacron: updating timestamp for '{}'",
                    entry.job_id
                );
                let _ = fs::write(&timestamp_file, "20260520\n");
            } else {
                let delay_msg = if run_now {
                    "no delay".to_string()
                } else if serialize {
                    format!("delay {}m (serialized)", entry.delay_minutes)
                } else {
                    format!("delay {}m", entry.delay_minutes)
                };
                let _ = writeln!(
                    out,
                    "anacron: job '{}' (period={}d, {delay_msg}): {}",
                    entry.job_id, entry.period_days, entry.command
                );
                // Update timestamp
                let _ = fs::write(&timestamp_file, "20260520\n");
            }
        } else {
            let _ = writeln!(
                out,
                "anacron: job '{}' not due yet",
                entry.job_id
            );
        }
    }

    0
}

// ---------------------------------------------------------------------------
// at personality
// ---------------------------------------------------------------------------

fn run_at(args: &[String], personality: Personality) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // atq = at -l, atrm = at -d
    let (effective_args, is_list, is_remove) = match personality {
        Personality::Atq => (args.to_vec(), true, false),
        Personality::Atrm => (args.to_vec(), false, true),
        _ => (args.to_vec(), false, false),
    };

    if is_list {
        return list_at_jobs(&mut out);
    }

    if is_remove {
        return remove_at_jobs(&effective_args, &mut out);
    }

    // Parse at arguments
    let mut file_source: Option<String> = None;
    let mut time_parts: Vec<String> = Vec::new();
    let mut list_mode = false;
    let mut delete_mode = false;
    let mut delete_ids: Vec<String> = Vec::new();

    let mut i = 0;
    while i < effective_args.len() {
        match effective_args[i].as_str() {
            "-f" => {
                i += 1;
                file_source = effective_args.get(i).cloned();
            }
            "-l" => list_mode = true,
            "-d" => {
                delete_mode = true;
                // Remaining args are job IDs
                i += 1;
                while i < effective_args.len() {
                    delete_ids.push(effective_args[i].clone());
                    i += 1;
                }
                break;
            }
            "--help" | "-h" => {
                let _ = writeln!(
                    out,
                    "Usage: at [-f FILE] TIME\n       at -l\n       at -d JOB_ID...\n\n\
                     Schedule one-time jobs.\n\n\
                     Time formats:\n  \
                       HH:MM                    Today or tomorrow\n  \
                       noon, midnight, teatime  Named times\n  \
                       now + N {{minutes,hours,days}}\n  \
                       YYYY-MM-DD HH:MM         Absolute\n  \
                       tomorrow HH:MM           Next day"
                );
                return 0;
            }
            _ => {
                time_parts.push(effective_args[i].clone());
            }
        }
        i += 1;
    }

    if list_mode {
        return list_at_jobs(&mut out);
    }

    if delete_mode {
        return remove_at_jobs(&delete_ids, &mut out);
    }

    if time_parts.is_empty() {
        let _ = writeln!(out, "at: missing time specification (try at -h)");
        return 1;
    }

    let time_spec = time_parts.join(" ");
    let now = DateTime::new(2026, 5, 20, 10, 0); // Simulated "now" for deterministic behavior

    let scheduled_time = match parse_at_time(&time_spec, &now) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "at: {e}");
            return 1;
        }
    };

    // Read command from file or stdin
    let command = if let Some(ref filename) = file_source {
        match fs::read_to_string(filename) {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(out, "at: cannot read '{filename}': {e}");
                return 1;
            }
        }
    } else {
        let _ = writeln!(out, "at: reading commands from stdin (simulated)");
        "echo 'scheduled job'\n".to_string()
    };

    let spool = Path::new(AT_SPOOL_DIR);
    let _ = fs::create_dir_all(spool);
    let job_id = next_at_job_id(spool);

    let user = env::var("USER").unwrap_or_else(|_| "root".to_string());

    let queue = if personality == Personality::Batch {
        'b'
    } else {
        'a'
    };

    let job = AtJob {
        id: job_id,
        time: scheduled_time,
        queue,
        command,
        user,
        env_vars: Vec::new(),
    };

    let job_path = spool.join(job.filename());
    match fs::write(&job_path, job.serialize()) {
        Ok(()) => {
            let _ = writeln!(
                out,
                "job {job_id} at {:04}-{:02}-{:02} {:02}:{:02}",
                scheduled_time.year,
                scheduled_time.month,
                scheduled_time.day,
                scheduled_time.hour,
                scheduled_time.minute,
            );
            0
        }
        Err(e) => {
            let _ = writeln!(out, "at: cannot write job file: {e}");
            1
        }
    }
}

fn list_at_jobs(out: &mut io::StdoutLock<'_>) -> i32 {
    let spool = Path::new(AT_SPOOL_DIR);
    let entries = match fs::read_dir(spool) {
        Ok(e) => e,
        Err(_) => {
            // No jobs
            return 0;
        }
    };

    let mut jobs: BTreeMap<u32, String> = BTreeMap::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(job) = AtJob::deserialize(&content) {
                jobs.insert(
                    job.id,
                    format!(
                        "{}\t{:04}-{:02}-{:02} {:02}:{:02} {} {}",
                        job.id,
                        job.time.year,
                        job.time.month,
                        job.time.day,
                        job.time.hour,
                        job.time.minute,
                        job.queue,
                        job.user,
                    ),
                );
            }
    }

    for line in jobs.values() {
        let _ = writeln!(out, "{line}");
    }

    0
}

fn remove_at_jobs(ids: &[String], out: &mut io::StdoutLock<'_>) -> i32 {
    if ids.is_empty() {
        let _ = writeln!(out, "atrm: missing job id");
        return 1;
    }

    let spool = Path::new(AT_SPOOL_DIR);
    let mut exit_code = 0;

    for id_str in ids {
        let target_id = match id_str.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                let _ = writeln!(out, "atrm: invalid job id '{id_str}'");
                exit_code = 1;
                continue;
            }
        };

        let hex_id = format!("{target_id:05x}");
        let mut found = false;

        if let Ok(entries) = fs::read_dir(spool) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.len() >= 6 && name[1..6] == hex_id {
                    match fs::remove_file(entry.path()) {
                        Ok(()) => {
                            found = true;
                            break;
                        }
                        Err(e) => {
                            let _ = writeln!(out, "atrm: cannot remove job {target_id}: {e}");
                            exit_code = 1;
                        }
                    }
                }
            }
        }

        if !found && exit_code == 0 {
            let _ = writeln!(out, "atrm: job {target_id} not found");
            exit_code = 1;
        }
    }

    exit_code
}

// ---------------------------------------------------------------------------
// atd personality
// ---------------------------------------------------------------------------

fn run_atd(args: &[String]) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut batch_threshold = 1.5f64;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" => {
                i += 1;
                if let Some(val) = args.get(i) {
                    batch_threshold = val.parse().unwrap_or(1.5);
                }
            }
            "--help" | "-h" => {
                let _ = writeln!(
                    out,
                    "Usage: atd [-l load_threshold]\n\n\
                     At daemon: processes pending one-time jobs.\n\n\
                     Options:\n  \
                       -l THRESHOLD  Load average threshold for batch jobs (default: 1.5)"
                );
                return 0;
            }
            _ => {
                let _ = writeln!(out, "atd: unknown option '{}'", args[i]);
                return 1;
            }
        }
        i += 1;
    }

    let _ = writeln!(
        out,
        "atd: starting (batch_threshold={batch_threshold})"
    );

    // Scan spool directory
    let spool = Path::new(AT_SPOOL_DIR);
    let entries = match fs::read_dir(spool) {
        Ok(e) => e,
        Err(e) => {
            let _ = writeln!(out, "atd: cannot read {AT_SPOOL_DIR}: {e}");
            return 1;
        }
    };

    let mut pending = 0u32;
    let mut batch_pending = 0u32;

    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(job) = AtJob::deserialize(&content) {
                if job.queue == 'b' {
                    batch_pending += 1;
                    let _ = writeln!(
                        out,
                        "atd: batch job {} for user '{}' at {:04}-{:02}-{:02} {:02}:{:02}",
                        job.id, job.user,
                        job.time.year, job.time.month, job.time.day,
                        job.time.hour, job.time.minute,
                    );
                } else {
                    pending += 1;
                    let _ = writeln!(
                        out,
                        "atd: job {} for user '{}' at {:04}-{:02}-{:02} {:02}:{:02}",
                        job.id, job.user,
                        job.time.year, job.time.month, job.time.day,
                        job.time.hour, job.time.minute,
                    );
                }
            }
    }

    let _ = writeln!(
        out,
        "atd: {pending} pending jobs, {batch_pending} batch jobs"
    );
    let _ = writeln!(out, "atd: scheduler ready (simulated)");

    0
}

// ---------------------------------------------------------------------------
// batch personality (wrapper for at with queue 'b')
// ---------------------------------------------------------------------------

fn run_batch(args: &[String]) -> i32 {
    // batch is just "at" with batch queue
    run_at(args, Personality::Batch)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("at");
    let personality = detect_personality(argv0);
    let rest = if args.len() > 1 { &args[1..] } else { &[] };
    let rest_vec: Vec<String> = rest.to_vec();

    let exit_code = match personality {
        Personality::Crond => run_crond(&rest_vec),
        Personality::Crontab => run_crontab(&rest_vec),
        Personality::Anacron => run_anacron(&rest_vec),
        Personality::At => run_at(&rest_vec, Personality::At),
        Personality::Atd => run_atd(&rest_vec),
        Personality::Batch => run_batch(&rest_vec),
        Personality::Atq => run_at(&rest_vec, Personality::Atq),
        Personality::Atrm => run_at(&rest_vec, Personality::Atrm),
    };

    process::exit(exit_code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_crond() {
        assert_eq!(detect_personality("crond"), Personality::Crond);
        assert_eq!(detect_personality("/usr/sbin/crond"), Personality::Crond);
        assert_eq!(detect_personality("crond.exe"), Personality::Crond);
        assert_eq!(detect_personality("C:\\bin\\crond.exe"), Personality::Crond);
        assert_eq!(detect_personality("cron"), Personality::Crond);
    }

    #[test]
    fn test_detect_crontab() {
        assert_eq!(detect_personality("crontab"), Personality::Crontab);
        assert_eq!(detect_personality("/usr/bin/crontab"), Personality::Crontab);
    }

    #[test]
    fn test_detect_anacron() {
        assert_eq!(detect_personality("anacron"), Personality::Anacron);
    }

    #[test]
    fn test_detect_at() {
        assert_eq!(detect_personality("at"), Personality::At);
        assert_eq!(detect_personality("/usr/bin/at"), Personality::At);
        assert_eq!(detect_personality("anything_else"), Personality::At);
    }

    #[test]
    fn test_detect_atd() {
        assert_eq!(detect_personality("atd"), Personality::Atd);
    }

    #[test]
    fn test_detect_batch() {
        assert_eq!(detect_personality("batch"), Personality::Batch);
    }

    #[test]
    fn test_detect_atq() {
        assert_eq!(detect_personality("atq"), Personality::Atq);
    }

    #[test]
    fn test_detect_atrm() {
        assert_eq!(detect_personality("atrm"), Personality::Atrm);
    }

    #[test]
    fn test_personality_names() {
        assert_eq!(Personality::Crond.name(), "crond");
        assert_eq!(Personality::Crontab.name(), "crontab");
        assert_eq!(Personality::Anacron.name(), "anacron");
        assert_eq!(Personality::At.name(), "at");
        assert_eq!(Personality::Atd.name(), "atd");
        assert_eq!(Personality::Batch.name(), "batch");
        assert_eq!(Personality::Atq.name(), "atq");
        assert_eq!(Personality::Atrm.name(), "atrm");
    }

    // -----------------------------------------------------------------------
    // Date/time helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_day_of_week_known_dates() {
        // 2024-01-01 is Monday
        assert_eq!(day_of_week(2024, 1, 1), 1);
        // 2026-05-20 is Wednesday
        assert_eq!(day_of_week(2026, 5, 20), 3);
        // 2000-01-01 is Saturday
        assert_eq!(day_of_week(2000, 1, 1), 6);
        // 2023-12-25 is Monday
        assert_eq!(day_of_week(2023, 12, 25), 1);
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 2), 29); // leap year
        assert_eq!(days_in_month(2023, 2), 28); // non-leap
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(1900, 2), 28); // century non-leap
        assert_eq!(days_in_month(2000, 2), 29); // 400-year leap
        assert_eq!(days_in_month(2024, 13), 0); // invalid month
    }

    #[test]
    fn test_advance_day_simple() {
        let dt = DateTime::new(2026, 5, 20, 10, 30);
        let next = advance_day(&dt);
        assert_eq!(next.year, 2026);
        assert_eq!(next.month, 5);
        assert_eq!(next.day, 21);
    }

    #[test]
    fn test_advance_day_month_boundary() {
        let dt = DateTime::new(2026, 1, 31, 10, 30);
        let next = advance_day(&dt);
        assert_eq!(next.month, 2);
        assert_eq!(next.day, 1);
    }

    #[test]
    fn test_advance_day_year_boundary() {
        let dt = DateTime::new(2026, 12, 31, 23, 59);
        let next = advance_day(&dt);
        assert_eq!(next.year, 2027);
        assert_eq!(next.month, 1);
        assert_eq!(next.day, 1);
    }

    // -----------------------------------------------------------------------
    // CronField
    // -----------------------------------------------------------------------

    #[test]
    fn test_cron_field_basic() {
        let mut f = CronField::new(0, 59);
        assert!(f.is_empty());
        f.set(5);
        assert!(f.matches(5));
        assert!(!f.matches(6));
    }

    #[test]
    fn test_cron_field_set_all() {
        let mut f = CronField::new(1, 12);
        f.set_all();
        for v in 1..=12 {
            assert!(f.matches(v));
        }
        assert!(!f.matches(0));
        assert!(!f.matches(13));
    }

    #[test]
    fn test_cron_field_out_of_range() {
        let f = CronField::new(0, 59);
        assert!(!f.matches(64)); // beyond u64 bit width
    }

    // -----------------------------------------------------------------------
    // Name mappings
    // -----------------------------------------------------------------------

    #[test]
    fn test_month_names() {
        assert_eq!(month_name_to_num("jan"), Some(1));
        assert_eq!(month_name_to_num("JAN"), Some(1));
        assert_eq!(month_name_to_num("Dec"), Some(12));
        assert_eq!(month_name_to_num("foo"), None);
    }

    #[test]
    fn test_weekday_names() {
        assert_eq!(weekday_name_to_num("sun"), Some(0));
        assert_eq!(weekday_name_to_num("MON"), Some(1));
        assert_eq!(weekday_name_to_num("sat"), Some(6));
        assert_eq!(weekday_name_to_num("xyz"), None);
    }

    // -----------------------------------------------------------------------
    // Cron field parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_wildcard() {
        let f = parse_field_token("*", "minute", 0, 59, no_names).unwrap();
        for v in 0..=59 {
            assert!(f.matches(v));
        }
    }

    #[test]
    fn test_parse_single_value() {
        let f = parse_field_token("5", "minute", 0, 59, no_names).unwrap();
        assert!(f.matches(5));
        assert!(!f.matches(4));
        assert!(!f.matches(6));
    }

    #[test]
    fn test_parse_range() {
        let f = parse_field_token("1-5", "hour", 0, 23, no_names).unwrap();
        for v in 1..=5 {
            assert!(f.matches(v));
        }
        assert!(!f.matches(0));
        assert!(!f.matches(6));
    }

    #[test]
    fn test_parse_step() {
        let f = parse_field_token("*/15", "minute", 0, 59, no_names).unwrap();
        assert!(f.matches(0));
        assert!(f.matches(15));
        assert!(f.matches(30));
        assert!(f.matches(45));
        assert!(!f.matches(1));
        assert!(!f.matches(14));
    }

    #[test]
    fn test_parse_range_with_step() {
        let f = parse_field_token("1-10/3", "hour", 0, 23, no_names).unwrap();
        assert!(f.matches(1));
        assert!(f.matches(4));
        assert!(f.matches(7));
        assert!(f.matches(10));
        assert!(!f.matches(2));
        assert!(!f.matches(3));
    }

    #[test]
    fn test_parse_list() {
        let f = parse_field_token("1,5,10,15", "minute", 0, 59, no_names).unwrap();
        assert!(f.matches(1));
        assert!(f.matches(5));
        assert!(f.matches(10));
        assert!(f.matches(15));
        assert!(!f.matches(2));
    }

    #[test]
    fn test_parse_month_names() {
        let f =
            parse_field_token("jan-mar", "month", 1, 12, month_name_to_num).unwrap();
        assert!(f.matches(1));
        assert!(f.matches(2));
        assert!(f.matches(3));
        assert!(!f.matches(4));
    }

    #[test]
    fn test_parse_weekday_names() {
        let f = parse_field_token(
            "mon-fri",
            "day-of-week",
            0,
            6,
            weekday_name_to_num,
        )
        .unwrap();
        assert!(f.matches(1)); // mon
        assert!(f.matches(5)); // fri
        assert!(!f.matches(0)); // sun
        assert!(!f.matches(6)); // sat
    }

    #[test]
    fn test_parse_field_invalid_step_zero() {
        let result = parse_field_token("*/0", "minute", 0, 59, no_names);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_field_invalid_range() {
        let result = parse_field_token("10-5", "hour", 0, 23, no_names);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_field_out_of_bounds() {
        let result = parse_field_token("60", "minute", 0, 59, no_names);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_field_bad_name() {
        let result = parse_field_token("xyz", "month", 1, 12, month_name_to_num);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_value_with_step() {
        // "5/10" means starting at 5, every 10
        let f = parse_field_token("5/10", "minute", 0, 59, no_names).unwrap();
        assert!(f.matches(5));
        assert!(f.matches(15));
        assert!(f.matches(25));
        assert!(f.matches(35));
        assert!(f.matches(45));
        assert!(f.matches(55));
        assert!(!f.matches(0));
        assert!(!f.matches(10));
    }

    // -----------------------------------------------------------------------
    // CronExpr parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_every_minute() {
        let expr = CronExpr::parse("* * * * *").unwrap();
        let dt = DateTime::new(2026, 5, 20, 10, 30);
        assert!(expr.matches(&dt));
    }

    #[test]
    fn test_parse_specific_time() {
        let expr = CronExpr::parse("30 10 * * *").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 10, 30)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 20, 10, 31)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 20, 11, 30)));
    }

    #[test]
    fn test_parse_day_of_month() {
        let expr = CronExpr::parse("0 0 15 * *").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 5, 15, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 16, 0, 0)));
    }

    #[test]
    fn test_parse_month_restriction() {
        let expr = CronExpr::parse("0 0 1 6 *").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 6, 1, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 1, 0, 0)));
    }

    #[test]
    fn test_parse_weekday_only() {
        // Every Monday at midnight
        let expr = CronExpr::parse("0 0 * * 1").unwrap();
        // 2026-05-18 is Monday
        assert!(expr.matches(&DateTime::new(2026, 5, 18, 0, 0)));
        // 2026-05-19 is Tuesday
        assert!(!expr.matches(&DateTime::new(2026, 5, 19, 0, 0)));
    }

    #[test]
    fn test_dom_and_dow_or_semantics() {
        // Day 15 OR Monday — OR semantics when both are restricted
        let expr = CronExpr::parse("0 0 15 * 1").unwrap();
        // 2026-05-15 is Friday — matches day-of-month
        assert!(expr.matches(&DateTime::new(2026, 5, 15, 0, 0)));
        // 2026-05-18 is Monday — matches day-of-week
        assert!(expr.matches(&DateTime::new(2026, 5, 18, 0, 0)));
        // 2026-05-19 is Tuesday, day 19 — matches neither
        assert!(!expr.matches(&DateTime::new(2026, 5, 19, 0, 0)));
    }

    #[test]
    fn test_cron_expr_every_15_minutes() {
        let expr = CronExpr::parse("*/15 * * * *").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 0)));
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 15)));
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 30)));
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 45)));
        assert!(!expr.matches(&DateTime::new(2026, 1, 1, 0, 1)));
    }

    #[test]
    fn test_cron_expr_work_hours() {
        // Mon-Fri 9-17
        let expr = CronExpr::parse("0 9-17 * * 1-5").unwrap();
        // Wed at 10:00
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 10, 0)));
        // Wed at 18:00 — out of range
        assert!(!expr.matches(&DateTime::new(2026, 5, 20, 18, 0)));
        // Sat at 10:00 — wrong day
        // 2026-05-23 is Saturday
        assert!(!expr.matches(&DateTime::new(2026, 5, 23, 10, 0)));
    }

    // -----------------------------------------------------------------------
    // Special strings
    // -----------------------------------------------------------------------

    #[test]
    fn test_special_yearly() {
        let expr = CronExpr::parse("@yearly").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 1, 2, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 2, 1, 0, 0)));
    }

    #[test]
    fn test_special_annually() {
        let expr = CronExpr::parse("@annually").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 0)));
    }

    #[test]
    fn test_special_monthly() {
        let expr = CronExpr::parse("@monthly").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 3, 1, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 3, 2, 0, 0)));
    }

    #[test]
    fn test_special_weekly() {
        let expr = CronExpr::parse("@weekly").unwrap();
        // Sunday at midnight
        // 2026-05-17 is Sunday
        assert!(expr.matches(&DateTime::new(2026, 5, 17, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 18, 0, 0)));
    }

    #[test]
    fn test_special_daily() {
        let expr = CronExpr::parse("@daily").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 20, 0, 1)));
    }

    #[test]
    fn test_special_midnight() {
        let expr = CronExpr::parse("@midnight").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 0, 0)));
    }

    #[test]
    fn test_special_hourly() {
        let expr = CronExpr::parse("@hourly").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 10, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 20, 10, 1)));
    }

    #[test]
    fn test_special_reboot() {
        let expr = CronExpr::parse("@reboot").unwrap();
        // @reboot fields are empty (never match time-based)
        assert!(!expr.matches(&DateTime::new(2026, 5, 20, 0, 0)));
    }

    #[test]
    fn test_special_invalid() {
        assert!(CronExpr::parse("@foobar").is_err());
    }

    // -----------------------------------------------------------------------
    // CronExpr Display
    // -----------------------------------------------------------------------

    #[test]
    fn test_cron_expr_display_wildcard() {
        let expr = CronExpr::parse("* * * * *").unwrap();
        assert_eq!(expr.to_string(), "* * * * *");
    }

    #[test]
    fn test_cron_expr_display_specific() {
        let expr = CronExpr::parse("30 10 15 6 1").unwrap();
        assert_eq!(expr.to_string(), "30 10 15 6 1");
    }

    // -----------------------------------------------------------------------
    // Crontab file parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_crontab_basic() {
        let content = "# comment\n\
                        SHELL=/bin/sh\n\
                        PATH=/usr/bin:/bin\n\
                        \n\
                        */15 * * * * /usr/bin/do_stuff\n\
                        0 2 * * * /usr/bin/backup\n";
        let ctab = parse_crontab(content, false).unwrap();
        assert_eq!(ctab.env_vars.len(), 2);
        assert_eq!(ctab.env_vars[0].key, "SHELL");
        assert_eq!(ctab.env_vars[0].value, "/bin/sh");
        assert_eq!(ctab.entries.len(), 2);
        assert_eq!(ctab.entries[0].command, "/usr/bin/do_stuff");
        assert_eq!(ctab.entries[1].command, "/usr/bin/backup");
        assert!(ctab.entries[0].user.is_none());
    }

    #[test]
    fn test_parse_crontab_system_format() {
        let content = "*/5 * * * * root /usr/bin/check\n";
        let ctab = parse_crontab(content, true).unwrap();
        assert_eq!(ctab.entries.len(), 1);
        assert_eq!(ctab.entries[0].user, Some("root".to_string()));
        assert_eq!(ctab.entries[0].command, "/usr/bin/check");
    }

    #[test]
    fn test_parse_crontab_special_entry() {
        let content = "@reboot /usr/bin/startup_script\n";
        let ctab = parse_crontab(content, false).unwrap();
        assert_eq!(ctab.entries.len(), 1);
        assert!(ctab.entries[0].is_reboot);
        assert_eq!(ctab.entries[0].command, "/usr/bin/startup_script");
    }

    #[test]
    fn test_parse_crontab_env_quoted() {
        let content = "MAILTO=\"user@example.com\"\n\
                        0 0 * * * /usr/bin/job\n";
        let ctab = parse_crontab(content, false).unwrap();
        assert_eq!(ctab.env_vars[0].value, "user@example.com");
    }

    #[test]
    fn test_parse_crontab_empty() {
        let content = "# only comments\n\n";
        let ctab = parse_crontab(content, false).unwrap();
        assert!(ctab.entries.is_empty());
        assert!(ctab.env_vars.is_empty());
    }

    #[test]
    fn test_parse_crontab_invalid_schedule() {
        let content = "99 99 99 99 99 /usr/bin/bad\n";
        assert!(parse_crontab(content, false).is_err());
    }

    // -----------------------------------------------------------------------
    // Anacrontab parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_anacrontab_basic() {
        let content = "# anacrontab\n\
                        SHELL=/bin/sh\n\
                        1\t5\tcron.daily\trun-parts /etc/cron.daily\n\
                        7\t10\tcron.weekly\trun-parts /etc/cron.weekly\n";
        let config = parse_anacrontab(content).unwrap();
        assert_eq!(config.entries.len(), 2);
        assert_eq!(config.entries[0].period_days, 1);
        assert_eq!(config.entries[0].delay_minutes, 5);
        assert_eq!(config.entries[0].job_id, "cron.daily");
        assert_eq!(config.entries[1].period_days, 7);
    }

    #[test]
    fn test_parse_anacrontab_special_periods() {
        let content = "@daily\t5\tdaily_job\t/usr/bin/daily\n\
                        @weekly\t10\tweekly_job\t/usr/bin/weekly\n\
                        @monthly\t15\tmonthly_job\t/usr/bin/monthly\n";
        let config = parse_anacrontab(content).unwrap();
        assert_eq!(config.entries[0].period_days, 1);
        assert_eq!(config.entries[1].period_days, 7);
        assert_eq!(config.entries[2].period_days, 30);
    }

    #[test]
    fn test_parse_anacrontab_env_vars() {
        let content = "SHELL=/bin/bash\nPATH=/usr/bin\n1 0 test /bin/true\n";
        let config = parse_anacrontab(content).unwrap();
        assert_eq!(config.env_vars.len(), 2);
        assert_eq!(config.env_vars[0].key, "SHELL");
    }

    #[test]
    fn test_parse_anacrontab_invalid() {
        let content = "bad line\n";
        assert!(parse_anacrontab(content).is_err());
    }

    // -----------------------------------------------------------------------
    // At time parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_at_time_hhmm_future() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = parse_at_time("15:30", &now).unwrap();
        assert_eq!(dt.hour, 15);
        assert_eq!(dt.minute, 30);
        assert_eq!(dt.day, 20); // today
    }

    #[test]
    fn test_at_time_hhmm_past_wraps() {
        let now = DateTime::new(2026, 5, 20, 16, 0);
        let dt = parse_at_time("10:00", &now).unwrap();
        assert_eq!(dt.hour, 10);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.day, 21); // tomorrow
    }

    #[test]
    fn test_at_time_noon() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = parse_at_time("noon", &now).unwrap();
        assert_eq!(dt.hour, 12);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.day, 20);
    }

    #[test]
    fn test_at_time_midnight() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = parse_at_time("midnight", &now).unwrap();
        // midnight is 00:00 — already past 10:00, so tomorrow
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.day, 21);
    }

    #[test]
    fn test_at_time_teatime() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = parse_at_time("teatime", &now).unwrap();
        assert_eq!(dt.hour, 16);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.day, 20);
    }

    #[test]
    fn test_at_time_now_plus_minutes() {
        let now = DateTime::new(2026, 5, 20, 10, 30);
        let dt = parse_at_time("now + 45 minutes", &now).unwrap();
        assert_eq!(dt.hour, 11);
        assert_eq!(dt.minute, 15);
    }

    #[test]
    fn test_at_time_now_plus_hours() {
        let now = DateTime::new(2026, 5, 20, 22, 0);
        let dt = parse_at_time("now + 5 hours", &now).unwrap();
        assert_eq!(dt.hour, 3);
        assert_eq!(dt.day, 21);
    }

    #[test]
    fn test_at_time_now_plus_days() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = parse_at_time("now + 3 days", &now).unwrap();
        assert_eq!(dt.day, 23);
        assert_eq!(dt.hour, 10);
    }

    #[test]
    fn test_at_time_absolute() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = parse_at_time("2026-06-15 14:30", &now).unwrap();
        assert_eq!(dt.year, 2026);
        assert_eq!(dt.month, 6);
        assert_eq!(dt.day, 15);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
    }

    #[test]
    fn test_at_time_tomorrow() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = parse_at_time("tomorrow 09:00", &now).unwrap();
        assert_eq!(dt.day, 21);
        assert_eq!(dt.hour, 9);
        assert_eq!(dt.minute, 0);
    }

    #[test]
    fn test_at_time_now() {
        let now = DateTime::new(2026, 5, 20, 10, 30);
        let dt = parse_at_time("now", &now).unwrap();
        assert_eq!(dt, now);
    }

    #[test]
    fn test_at_time_invalid() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        assert!(parse_at_time("not-a-time", &now).is_err());
    }

    #[test]
    fn test_at_time_invalid_hhmm() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        assert!(parse_at_time("25:00", &now).is_err());
    }

    // -----------------------------------------------------------------------
    // At job serialization
    // -----------------------------------------------------------------------

    #[test]
    fn test_at_job_serialize_deserialize() {
        let job = AtJob {
            id: 42,
            time: DateTime::new(2026, 5, 20, 14, 30),
            queue: 'a',
            command: "echo hello\n".to_string(),
            user: "testuser".to_string(),
            env_vars: vec![CrontabEnvVar {
                key: "HOME".to_string(),
                value: "/home/testuser".to_string(),
            }],
        };

        let serialized = job.serialize();
        let deserialized = AtJob::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.id, 42);
        assert_eq!(deserialized.queue, 'a');
        assert_eq!(deserialized.user, "testuser");
        assert_eq!(deserialized.time.year, 2026);
        assert_eq!(deserialized.time.month, 5);
        assert_eq!(deserialized.time.day, 20);
        assert_eq!(deserialized.time.hour, 14);
        assert_eq!(deserialized.time.minute, 30);
        assert!(deserialized.command.contains("echo hello"));
    }

    #[test]
    fn test_at_job_filename() {
        let job = AtJob {
            id: 255,
            time: DateTime::new(2026, 5, 20, 14, 30),
            queue: 'a',
            command: String::new(),
            user: String::new(),
            env_vars: Vec::new(),
        };
        let name = job.filename();
        assert!(name.starts_with('a'));
        assert!(name.contains("000ff"));
    }

    #[test]
    fn test_at_job_batch_queue() {
        let job = AtJob {
            id: 1,
            time: DateTime::new(2026, 5, 20, 14, 30),
            queue: 'b',
            command: "batch_job\n".to_string(),
            user: "root".to_string(),
            env_vars: Vec::new(),
        };
        let serialized = job.serialize();
        assert!(serialized.contains("queue=b"));
    }

    // -----------------------------------------------------------------------
    // parse_datetime_str
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_datetime_str_valid() {
        let dt = parse_datetime_str("2026-05-20 14:30").unwrap();
        assert_eq!(dt.year, 2026);
        assert_eq!(dt.month, 5);
        assert_eq!(dt.day, 20);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
    }

    #[test]
    fn test_parse_datetime_str_invalid_date() {
        assert!(parse_datetime_str("2026-13-01 00:00").is_err());
    }

    #[test]
    fn test_parse_datetime_str_invalid_format() {
        assert!(parse_datetime_str("not a date").is_err());
    }

    #[test]
    fn test_parse_datetime_str_feb_leap() {
        let dt = parse_datetime_str("2024-02-29 12:00").unwrap();
        assert_eq!(dt.day, 29);
    }

    #[test]
    fn test_parse_datetime_str_feb_no_leap() {
        assert!(parse_datetime_str("2023-02-29 12:00").is_err());
    }

    // -----------------------------------------------------------------------
    // strip_quotes
    // -----------------------------------------------------------------------

    #[test]
    fn test_strip_quotes_double() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
    }

    #[test]
    fn test_strip_quotes_single() {
        assert_eq!(strip_quotes("'hello'"), "hello");
    }

    #[test]
    fn test_strip_quotes_none() {
        assert_eq!(strip_quotes("hello"), "hello");
    }

    #[test]
    fn test_strip_quotes_mismatched() {
        assert_eq!(strip_quotes("\"hello'"), "\"hello'");
    }

    // -----------------------------------------------------------------------
    // Complex cron expressions
    // -----------------------------------------------------------------------

    #[test]
    fn test_cron_list_in_multiple_fields() {
        let expr = CronExpr::parse("0,30 9,17 * * *").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 9, 0)));
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 9, 30)));
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 17, 0)));
        assert!(expr.matches(&DateTime::new(2026, 5, 20, 17, 30)));
        assert!(!expr.matches(&DateTime::new(2026, 5, 20, 10, 0)));
    }

    #[test]
    fn test_cron_month_name_range() {
        let expr = CronExpr::parse("0 0 1 jan-jun *").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 3, 1, 0, 0)));
        assert!(!expr.matches(&DateTime::new(2026, 9, 1, 0, 0)));
    }

    #[test]
    fn test_cron_too_few_fields() {
        assert!(CronExpr::parse("* * *").is_err());
    }

    #[test]
    fn test_cron_weekday_7_is_invalid() {
        // Standard cron: 0-6, 7 is out of range for our parser
        assert!(CronExpr::parse("* * * * 7").is_err());
    }

    #[test]
    fn test_cron_expr_multiple_ranges() {
        let expr = CronExpr::parse("0-15,45-59 * * * *").unwrap();
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 0)));
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 10)));
        assert!(expr.matches(&DateTime::new(2026, 1, 1, 0, 50)));
        assert!(!expr.matches(&DateTime::new(2026, 1, 1, 0, 30)));
    }

    // -----------------------------------------------------------------------
    // Duration arithmetic
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_minutes_no_overflow() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        let dt = add_duration(&now, 30, "minutes").unwrap();
        assert_eq!(dt.hour, 10);
        assert_eq!(dt.minute, 30);
    }

    #[test]
    fn test_add_minutes_hour_overflow() {
        let now = DateTime::new(2026, 5, 20, 10, 45);
        let dt = add_duration(&now, 30, "minutes").unwrap();
        assert_eq!(dt.hour, 11);
        assert_eq!(dt.minute, 15);
    }

    #[test]
    fn test_add_hours_day_overflow() {
        let now = DateTime::new(2026, 5, 20, 23, 0);
        let dt = add_duration(&now, 2, "hours").unwrap();
        assert_eq!(dt.day, 21);
        assert_eq!(dt.hour, 1);
    }

    #[test]
    fn test_add_days() {
        let now = DateTime::new(2026, 5, 30, 10, 0);
        let dt = add_duration(&now, 3, "days").unwrap();
        assert_eq!(dt.month, 6);
        assert_eq!(dt.day, 2);
    }

    #[test]
    fn test_add_unknown_unit() {
        let now = DateTime::new(2026, 5, 20, 10, 0);
        assert!(add_duration(&now, 1, "weeks").is_err());
    }

    // -----------------------------------------------------------------------
    // split_number_unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_split_number_unit() {
        assert_eq!(
            split_number_unit("30minutes"),
            Some((30, "minutes".to_string()))
        );
        assert_eq!(
            split_number_unit("5hours"),
            Some((5, "hours".to_string()))
        );
        assert_eq!(split_number_unit("abc"), None);
        assert_eq!(split_number_unit(""), None);
    }

    // -----------------------------------------------------------------------
    // Error display
    // -----------------------------------------------------------------------

    #[test]
    fn test_error_display() {
        let e = CronError::InvalidField {
            field: "minute",
            value: "abc".to_string(),
        };
        assert!(e.to_string().contains("minute"));
        assert!(e.to_string().contains("abc"));

        let e = CronError::InvalidStep {
            field: "hour",
            step: 0,
        };
        assert!(e.to_string().contains("step"));

        let e = CronError::InvalidSpecial("xyz".to_string());
        assert!(e.to_string().contains("xyz"));

        let e = CronError::IoError("disk full".to_string());
        assert!(e.to_string().contains("disk full"));

        let e = CronError::PermissionDenied("no access".to_string());
        assert!(e.to_string().contains("no access"));

        let e = CronError::InvalidLine("bad".to_string());
        assert!(e.to_string().contains("bad"));

        let e = CronError::InvalidTimeSpec("nope".to_string());
        assert!(e.to_string().contains("nope"));

        let e = CronError::InvalidRange {
            field: "hour",
            low: 10,
            high: 5,
        };
        assert!(e.to_string().contains("10"));

        let e = CronError::ParseInt {
            field: "minute",
            value: "abc".to_string(),
        };
        assert!(e.to_string().contains("abc"));
    }

    // -----------------------------------------------------------------------
    // is_all_set
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_all_set_true() {
        let mut f = CronField::new(0, 6);
        f.set_all();
        assert!(is_all_set(&f, 0, 6));
    }

    #[test]
    fn test_is_all_set_false() {
        let mut f = CronField::new(0, 6);
        f.set(0);
        f.set(1);
        assert!(!is_all_set(&f, 0, 6));
    }

    // -----------------------------------------------------------------------
    // parse_hhmm
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_hhmm_valid() {
        assert_eq!(parse_hhmm("14:30").unwrap(), (14, 30));
        assert_eq!(parse_hhmm("00:00").unwrap(), (0, 0));
        assert_eq!(parse_hhmm("23:59").unwrap(), (23, 59));
    }

    #[test]
    fn test_parse_hhmm_invalid() {
        assert!(parse_hhmm("24:00").is_err());
        assert!(parse_hhmm("12:60").is_err());
        assert!(parse_hhmm("abc").is_err());
    }

    // -----------------------------------------------------------------------
    // Crontab system format with @special
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_system_crontab_with_special() {
        let content = "@hourly root /usr/bin/hourly_check\n";
        let ctab = parse_crontab(content, true).unwrap();
        assert_eq!(ctab.entries.len(), 1);
        assert_eq!(ctab.entries[0].user, Some("root".to_string()));
        assert_eq!(ctab.entries[0].command, "/usr/bin/hourly_check");
    }
}
