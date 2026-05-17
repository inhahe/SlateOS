//! OurOS Cron Daemon (`crond`)
//!
//! A time-based task scheduler that runs commands at specified times and
//! intervals. Supports standard crontab syntax plus extensions.
//!
//! # Architecture
//!
//! - **crond daemon**: background process that wakes every 60 seconds,
//!   checks which jobs match the current time, and spawns them.
//! - **crontab files**: per-user job definitions in `/var/spool/cron/<user>`
//!   and system-wide jobs in `/etc/cron.d/`.
//! - **crond** binary doubles as both daemon and CLI tool for managing jobs.
//!
//! # Crontab Syntax
//!
//! ```text
//! # min  hour  day  month  weekday  command
//!   */5   *     *    *      *       /bin/cleanup --temp
//!   0     3     *    *      *       /bin/backup start /home
//!   30    8     1    *      *       /bin/report --monthly
//! ```
//!
//! Special time strings: `@reboot`, `@hourly`, `@daily`, `@weekly`, `@monthly`, `@yearly`.
//!
//! # Commands
//!
//! ```text
//! crond daemon           Run as daemon (background scheduler)
//! crond list             List current user's crontab entries
//! crond add <spec>       Add a crontab entry (quoted cron line)
//! crond remove <n>       Remove entry by line number
//! crond edit             Show crontab file path for manual editing
//! crond run-pending      Single pass: run all due jobs and exit
//! crond next [n]         Show next N upcoming job executions
//! crond log [n]          Show last N execution log entries
//! crond status           Show daemon status
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const SPOOL_DIR: &str = "/var/spool/cron";
const SYSTEM_CRON_DIR: &str = "/etc/cron.d";
const LOG_PATH: &str = "/var/log/cron.log";
const PID_PATH: &str = "/var/run/crond.pid";
const DEFAULT_USER: &str = "root";
// Max log entries kept (for future log rotation).
#[allow(dead_code)]
const MAX_LOG_ENTRIES: usize = 1000;

// ============================================================================
// Time helpers
// ============================================================================

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Broken-down time from Unix seconds (UTC).
#[derive(Debug, Clone, Copy)]
struct BrokenTime {
    year: i64,
    month: u32,   // 1-12
    day: u32,     // 1-31
    hour: u32,    // 0-23
    minute: u32,  // 0-59
    weekday: u32, // 0=Sunday, 1=Monday, ..., 6=Saturday
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[allow(dead_code)] // Used by future next-execution prediction with month rollover.
fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap_year(y) { 29 } else { 28 },
        _ => 30,
    }
}

fn unix_to_broken(unix_secs: u64) -> BrokenTime {
    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hour = (time_secs / 3600) as u32;
    let minute = ((time_secs % 3600) / 60) as u32;

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
    }
    let day = remaining_days as u32 + 1;

    BrokenTime { year: y, month, day, hour, minute, weekday }
}

fn format_time(bt: &BrokenTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        bt.year, bt.month, bt.day, bt.hour, bt.minute
    )
}

fn weekday_name(wd: u32) -> &'static str {
    match wd {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        6 => "Sat",
        _ => "???",
    }
}

// ============================================================================
// Cron schedule field
// ============================================================================

/// A single cron schedule field (minute, hour, day, month, weekday).
#[derive(Debug, Clone)]
enum CronField {
    /// Match any value.
    Any,
    /// Match specific values.
    Values(Vec<u32>),
    /// Match a range (inclusive).
    Range(u32, u32),
    /// Match every N-th value starting from base.
    Step(u32, u32), // (base, step)
}

impl CronField {
    fn matches(&self, value: u32) -> bool {
        match self {
            CronField::Any => true,
            CronField::Values(vals) => vals.contains(&value),
            CronField::Range(lo, hi) => value >= *lo && value <= *hi,
            CronField::Step(base, step) => {
                if *step == 0 {
                    return value == *base;
                }
                value >= *base && (value - *base) % *step == 0
            }
        }
    }

    /// Parse a single cron field from text.
    ///
    /// Supports: `*`, `N`, `N-M`, `*/N`, `N,M,O`, `N-M/S`.
    fn parse(s: &str, min: u32, max: u32) -> Result<Self, String> {
        // Wildcard.
        if s == "*" {
            return Ok(CronField::Any);
        }

        // Step: */N or N-M/S
        if let Some((base_part, step_str)) = s.split_once('/') {
            let step: u32 = step_str.parse().map_err(|_| format!("bad step: {step_str}"))?;
            if base_part == "*" {
                return Ok(CronField::Step(min, step));
            }
            if let Some((lo_str, hi_str)) = base_part.split_once('-') {
                let lo: u32 = lo_str.parse().map_err(|_| format!("bad range start: {lo_str}"))?;
                let hi: u32 = hi_str.parse().map_err(|_| format!("bad range end: {hi_str}"))?;
                // Expand range with step into values.
                let mut vals = Vec::new();
                let mut v = lo;
                while v <= hi {
                    vals.push(v);
                    v += step;
                }
                return Ok(CronField::Values(vals));
            }
            let base: u32 = base_part.parse().map_err(|_| format!("bad step base: {base_part}"))?;
            return Ok(CronField::Step(base, step));
        }

        // Comma-separated list.
        if s.contains(',') {
            let mut vals = Vec::new();
            for part in s.split(',') {
                let v: u32 = part.trim().parse().map_err(|_| format!("bad value: {part}"))?;
                if v < min || v > max {
                    return Err(format!("value {v} out of range [{min}-{max}]"));
                }
                vals.push(v);
            }
            vals.sort();
            vals.dedup();
            return Ok(CronField::Values(vals));
        }

        // Range: N-M
        if let Some((lo_str, hi_str)) = s.split_once('-') {
            let lo: u32 = lo_str.parse().map_err(|_| format!("bad range start: {lo_str}"))?;
            let hi: u32 = hi_str.parse().map_err(|_| format!("bad range end: {hi_str}"))?;
            return Ok(CronField::Range(lo, hi));
        }

        // Single value.
        let v: u32 = s.parse().map_err(|_| format!("bad value: {s}"))?;
        if v < min || v > max {
            return Err(format!("value {v} out of range [{min}-{max}]"));
        }
        Ok(CronField::Values(vec![v]))
    }
}

// ============================================================================
// Cron job
// ============================================================================

/// A parsed cron job entry.
#[derive(Debug, Clone)]
struct CronJob {
    /// Original line from crontab.
    raw_line: String,
    /// Minute field (0-59).
    minute: CronField,
    /// Hour field (0-23).
    hour: CronField,
    /// Day of month field (1-31).
    day: CronField,
    /// Month field (1-12).
    month: CronField,
    /// Day of week field (0-6, 0=Sunday).
    weekday: CronField,
    /// Command to execute.
    command: String,
    /// Whether this is a @reboot job.
    at_reboot: bool,
}

impl CronJob {
    /// Check if this job should run at the given time.
    fn matches_time(&self, bt: &BrokenTime) -> bool {
        if self.at_reboot {
            return false; // @reboot jobs run once at startup, not on schedule.
        }

        self.minute.matches(bt.minute)
            && self.hour.matches(bt.hour)
            && self.day.matches(bt.day)
            && self.month.matches(bt.month)
            && self.weekday.matches(bt.weekday)
    }

    /// Parse a crontab line.
    fn parse(line: &str) -> Result<Self, String> {
        let trimmed = line.trim();

        // Special time strings.
        if trimmed.starts_with('@') {
            return Self::parse_special(trimmed);
        }

        let parts: Vec<&str> = trimmed.splitn(6, char::is_whitespace).collect();
        if parts.len() < 6 {
            return Err("need 5 schedule fields + command".to_string());
        }

        let minute = CronField::parse(parts[0], 0, 59)?;
        let hour = CronField::parse(parts[1], 0, 23)?;
        let day = CronField::parse(parts[2], 1, 31)?;
        let month = CronField::parse(parts[3], 1, 12)?;
        let weekday = CronField::parse(parts[4], 0, 6)?;
        let command = parts[5].to_string();

        Ok(CronJob {
            raw_line: line.to_string(),
            minute,
            hour,
            day,
            month,
            weekday,
            command,
            at_reboot: false,
        })
    }

    fn parse_special(line: &str) -> Result<Self, String> {
        let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
        if parts.len() < 2 {
            return Err("need @keyword command".to_string());
        }

        let keyword = parts[0];
        let command = parts[1].to_string();

        let (minute, hour, day, month, weekday, at_reboot) = match keyword {
            "@reboot" => (
                CronField::Any, CronField::Any, CronField::Any,
                CronField::Any, CronField::Any, true,
            ),
            "@yearly" | "@annually" => (
                CronField::Values(vec![0]), CronField::Values(vec![0]),
                CronField::Values(vec![1]), CronField::Values(vec![1]),
                CronField::Any, false,
            ),
            "@monthly" => (
                CronField::Values(vec![0]), CronField::Values(vec![0]),
                CronField::Values(vec![1]), CronField::Any,
                CronField::Any, false,
            ),
            "@weekly" => (
                CronField::Values(vec![0]), CronField::Values(vec![0]),
                CronField::Any, CronField::Any,
                CronField::Values(vec![0]), false,
            ),
            "@daily" | "@midnight" => (
                CronField::Values(vec![0]), CronField::Values(vec![0]),
                CronField::Any, CronField::Any,
                CronField::Any, false,
            ),
            "@hourly" => (
                CronField::Values(vec![0]), CronField::Any,
                CronField::Any, CronField::Any,
                CronField::Any, false,
            ),
            other => return Err(format!("unknown special: {other}")),
        };

        Ok(CronJob {
            raw_line: line.to_string(),
            minute,
            hour,
            day,
            month,
            weekday,
            command,
            at_reboot,
        })
    }

    fn describe(&self) -> String {
        if self.at_reboot {
            return format!("@reboot  {}", self.command);
        }
        self.raw_line.clone()
    }
}

// ============================================================================
// Crontab file
// ============================================================================

/// A crontab (collection of jobs).
struct Crontab {
    jobs: Vec<CronJob>,
    path: PathBuf,
}

impl Crontab {
    fn load(path: &Path) -> Self {
        let mut jobs = Vec::new();

        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                match CronJob::parse(trimmed) {
                    Ok(job) => jobs.push(job),
                    Err(e) => eprintln!("  skip bad line: {e}: {trimmed}"),
                }
            }
        }

        Crontab { jobs, path: path.to_path_buf() }
    }

    fn save(&self) -> Result<(), String> {
        let mut out = String::new();
        out.push_str("# OurOS crontab — managed by crond\n");
        out.push_str("# min hour day month weekday command\n\n");

        for job in &self.jobs {
            out.push_str(&job.raw_line);
            out.push('\n');
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        fs::write(&self.path, &out).map_err(|e| format!("write: {e}"))
    }
}

// ============================================================================
// Execution log
// ============================================================================

/// A log entry recording a job execution.
struct LogEntry {
    timestamp: u64,
    command: String,
    exit_code: i32,
    duration_ms: u64,
}

impl LogEntry {
    fn format(&self) -> String {
        let bt = unix_to_broken(self.timestamp);
        format!(
            "[{}] exit={} {}ms  {}",
            format_time(&bt),
            self.exit_code,
            self.duration_ms,
            self.command
        )
    }
}

fn append_log(entry: &LogEntry) {
    let line = format!("{}\n", entry.format());
    // Append to log file, creating it if needed.
    if let Some(parent) = Path::new(LOG_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });
}

fn read_log(max_entries: usize) -> Vec<String> {
    let content = match fs::read_to_string(LOG_PATH) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let start = lines.len().saturating_sub(max_entries);
    lines[start..].to_vec()
}

// ============================================================================
// Job execution
// ============================================================================

fn execute_job(job: &CronJob) -> LogEntry {
    let start = now_secs();
    let start_instant = std::time::Instant::now();

    println!("  exec: {}", job.command);

    // On our OS, spawn the command via the shell.
    // For now, use std::process::Command.
    let result = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&job.command)
        .status();

    let duration = start_instant.elapsed();
    let exit_code = match result {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => {
            eprintln!("    spawn error: {e}");
            -1
        }
    };

    LogEntry {
        timestamp: start,
        command: job.command.clone(),
        exit_code,
        duration_ms: duration.as_millis() as u64,
    }
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_daemon() {
    println!("crond: starting daemon");

    // Write PID file.
    if let Some(parent) = Path::new(PID_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(PID_PATH, format!("{}", std::process::id()));

    // Run @reboot jobs on first pass.
    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    let tab = Crontab::load(&user_tab_path);
    for job in &tab.jobs {
        if job.at_reboot {
            println!("  @reboot: {}", job.command);
            let entry = execute_job(job);
            append_log(&entry);
        }
    }

    // Main loop: wake every 60 seconds, check jobs.
    let mut last_minute = u64::MAX;

    loop {
        let now = now_secs();
        let bt = unix_to_broken(now);
        let current_minute = now / 60;

        // Only fire once per minute.
        if current_minute != last_minute {
            last_minute = current_minute;

            // Reload crontab each cycle (allows live edits).
            let tab = Crontab::load(&user_tab_path);

            // Also load system cron.d files.
            let system_jobs = load_system_jobs();

            let mut ran = 0u32;
            for job in tab.jobs.iter().chain(system_jobs.iter()) {
                if job.matches_time(&bt) {
                    let entry = execute_job(job);
                    append_log(&entry);
                    ran += 1;
                }
            }

            if ran > 0 {
                println!("crond: ran {ran} job(s) at {}", format_time(&bt));
            }
        }

        // Sleep until next minute boundary (plus small buffer).
        let next_minute_start = (current_minute + 1) * 60;
        let sleep_secs = next_minute_start.saturating_sub(now_secs()).max(1);
        std::thread::sleep(std::time::Duration::from_secs(sleep_secs));
    }
}

fn load_system_jobs() -> Vec<CronJob> {
    let mut jobs = Vec::new();

    let read_dir = match fs::read_dir(SYSTEM_CRON_DIR) {
        Ok(rd) => rd,
        Err(_) => return jobs,
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Ok(job) = CronJob::parse(trimmed) {
                    jobs.push(job);
                }
            }
        }
    }

    jobs
}

fn cmd_list() {
    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    let tab = Crontab::load(&user_tab_path);

    if tab.jobs.is_empty() {
        println!("No crontab entries.");
        return;
    }

    println!("Crontab for {DEFAULT_USER} ({}):", user_tab_path.display());
    for (i, job) in tab.jobs.iter().enumerate() {
        println!("  {:3}  {}", i + 1, job.describe());
    }
}

fn cmd_add(spec: &str) {
    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    let mut tab = Crontab::load(&user_tab_path);

    // Validate the spec first.
    match CronJob::parse(spec) {
        Ok(job) => {
            println!("  added: {}", job.describe());
            tab.jobs.push(job);
        }
        Err(e) => {
            eprintln!("error: invalid crontab entry: {e}");
            eprintln!("  format: MIN HOUR DAY MONTH WEEKDAY COMMAND");
            eprintln!("  or: @reboot|@hourly|@daily|@weekly|@monthly|@yearly COMMAND");
            process::exit(1);
        }
    }

    if let Err(e) = tab.save() {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

fn cmd_remove(num: usize) {
    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    let mut tab = Crontab::load(&user_tab_path);

    if num == 0 || num > tab.jobs.len() {
        eprintln!("error: entry {num} does not exist (have {} entries)", tab.jobs.len());
        process::exit(1);
    }

    let removed = tab.jobs.remove(num - 1);
    println!("  removed: {}", removed.describe());

    if let Err(e) = tab.save() {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

fn cmd_edit() {
    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    println!("Crontab file: {}", user_tab_path.display());
    println!("Edit directly, then crond will pick up changes on next cycle.");
    println!();
    println!("Format: MIN HOUR DAY MONTH WEEKDAY COMMAND");
    println!("  Fields: * (any), N (exact), N-M (range), */N (step), N,M (list)");
    println!("  Special: @reboot @hourly @daily @weekly @monthly @yearly");
}

fn cmd_run_pending() {
    let now = now_secs();
    let bt = unix_to_broken(now);

    println!("crond: single pass at {}", format_time(&bt));

    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    let tab = Crontab::load(&user_tab_path);
    let system_jobs = load_system_jobs();

    let mut ran = 0u32;
    for job in tab.jobs.iter().chain(system_jobs.iter()) {
        if job.matches_time(&bt) {
            let entry = execute_job(job);
            append_log(&entry);
            ran += 1;
        }
    }

    println!("crond: ran {ran} job(s)");
}

fn cmd_next(count: usize) {
    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    let tab = Crontab::load(&user_tab_path);
    let system_jobs = load_system_jobs();

    let all_jobs: Vec<&CronJob> = tab.jobs.iter().chain(system_jobs.iter())
        .filter(|j| !j.at_reboot)
        .collect();

    if all_jobs.is_empty() {
        println!("No scheduled jobs.");
        return;
    }

    println!("Next {count} upcoming executions:");

    // Walk forward minute by minute from now.
    let now = now_secs();
    let mut found = 0usize;
    let max_check = 60 * 24 * 31; // Check up to 31 days ahead.

    for offset_min in 1..=max_check {
        let future = now + offset_min * 60;
        let bt = unix_to_broken(future);

        for job in &all_jobs {
            if job.matches_time(&bt) {
                println!(
                    "  {} {} — {}",
                    format_time(&bt),
                    weekday_name(bt.weekday),
                    job.command
                );
                found += 1;
                if found >= count {
                    return;
                }
            }
        }
    }

    if found == 0 {
        println!("  (no jobs found within 31 days)");
    }
}

fn cmd_log(count: usize) {
    let entries = read_log(count);
    if entries.is_empty() {
        println!("No log entries.");
        return;
    }

    println!("Last {} log entries:", entries.len());
    for entry in &entries {
        println!("  {entry}");
    }
}

fn cmd_status() {
    println!("=== Cron Daemon Status ===");

    // Check PID file.
    let running = if let Ok(pid_str) = fs::read_to_string(PID_PATH) {
        let pid = pid_str.trim();
        // Check if process is alive (on our OS, would check /proc/<pid>).
        println!("  PID file:  {} (pid {})", PID_PATH, pid);
        true
    } else {
        println!("  PID file:  not found");
        false
    };

    println!("  Status:    {}", if running { "running (PID file exists)" } else { "not running" });

    // Count jobs.
    let user_tab_path = PathBuf::from(SPOOL_DIR).join(DEFAULT_USER);
    let tab = Crontab::load(&user_tab_path);
    let system_jobs = load_system_jobs();

    println!("  User jobs: {}", tab.jobs.len());
    println!("  System:    {}", system_jobs.len());
    println!("  Total:     {}", tab.jobs.len() + system_jobs.len());

    // Last log entry.
    let entries = read_log(1);
    if let Some(last) = entries.last() {
        println!("  Last exec: {last}");
    } else {
        println!("  Last exec: (none)");
    }
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("OurOS Cron Daemon v0.1.0");
    println!();
    println!("Time-based task scheduler. Runs commands at specified times.");
    println!();
    println!("USAGE:");
    println!("  crond <command> [arguments]");
    println!();
    println!("COMMANDS:");
    println!("  daemon           Run as background scheduler daemon");
    println!("  list             List crontab entries for current user");
    println!("  add <spec>       Add a crontab entry");
    println!("  remove <n>       Remove entry by line number");
    println!("  edit             Show crontab file path and format help");
    println!("  run-pending      Single pass: execute due jobs and exit");
    println!("  next [n]         Show next N upcoming executions (default: 10)");
    println!("  log [n]          Show last N execution log entries (default: 20)");
    println!("  status           Show daemon status");
    println!();
    println!("CRONTAB FORMAT:");
    println!("  MIN HOUR DAY MONTH WEEKDAY COMMAND");
    println!();
    println!("  Field values:");
    println!("    *       any value");
    println!("    N       exact value");
    println!("    N-M     range (inclusive)");
    println!("    N,M,O   list of values");
    println!("    */N     every N-th value");
    println!("    N-M/S   range with step");
    println!();
    println!("  Special keywords:");
    println!("    @reboot   — run once at daemon startup");
    println!("    @hourly   — 0 * * * *");
    println!("    @daily    — 0 0 * * *");
    println!("    @weekly   — 0 0 * * 0");
    println!("    @monthly  — 0 0 1 * *");
    println!("    @yearly   — 0 0 1 1 *");
    println!();
    println!("EXAMPLES:");
    println!("  crond add '*/5 * * * * /bin/cleanup --temp'");
    println!("  crond add '0 3 * * * /bin/backup start /home'");
    println!("  crond add '@daily /bin/report --summary'");
    println!("  crond add '@reboot /bin/indexer daemon'");
    println!("  crond next 5");
    println!("  crond daemon");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    match args[1].as_str() {
        "daemon" => cmd_daemon(),
        "list" | "ls" => cmd_list(),
        "add" => {
            if args.len() < 3 {
                eprintln!("usage: crond add <crontab-line>");
                process::exit(1);
            }
            // Join remaining args as the spec (in case it wasn't quoted).
            let spec = args[2..].join(" ");
            cmd_add(&spec);
        }
        "remove" | "rm" | "del" => {
            if args.len() < 3 {
                eprintln!("usage: crond remove <line-number>");
                process::exit(1);
            }
            match args[2].parse::<usize>() {
                Ok(n) => cmd_remove(n),
                Err(_) => {
                    eprintln!("error: invalid line number: {}", args[2]);
                    process::exit(1);
                }
            }
        }
        "edit" => cmd_edit(),
        "run-pending" | "run" => cmd_run_pending(),
        "next" => {
            let count = if args.len() >= 3 {
                args[2].parse::<usize>().unwrap_or(10)
            } else {
                10
            };
            cmd_next(count);
        }
        "log" | "logs" => {
            let count = if args.len() >= 3 {
                args[2].parse::<usize>().unwrap_or(20)
            } else {
                20
            };
            cmd_log(count);
        }
        "status" => cmd_status(),
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'crond help' for usage.");
            process::exit(1);
        }
    }
}
