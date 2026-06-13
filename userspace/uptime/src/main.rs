//! Slate OS System Uptime Display
//!
//! Displays how long the system has been running, the number of logged-in
//! users, and load averages — matching the output format of Linux `uptime`.
//!
//! Data sources:
//! - `/proc/uptime` — total uptime seconds and idle time
//! - `/proc/loadavg` — 1/5/15-minute load averages and process counts
//! - `/proc/stat` — boot time (btime line) for `--since` mode
//! - `/var/run/utmp` or `/run/user/` — logged-in user count
//!
//! # Usage
//!
//! ```text
//! uptime                Show standard uptime line
//! uptime -p / --pretty  Human-readable uptime
//! uptime -s / --since   Show boot date/time
//! uptime -r / --raw     Show raw uptime seconds
//! uptime --json         JSON output with all fields
//! uptime -V / --version Version display
//! uptime -h / --help    Show help
//! ```

use std::env;
use std::fs;
use std::process;
use std::time::SystemTime;

// ============================================================================
// Data structures
// ============================================================================

/// Parsed uptime information from /proc/uptime.
struct UptimeInfo {
    /// Total system uptime in seconds (fractional).
    total_seconds: f64,
    /// Total idle time in seconds (fractional).
    idle_seconds: f64,
}

/// Parsed load average information from /proc/loadavg.
struct LoadAvg {
    /// 1-minute load average.
    avg_1: f64,
    /// 5-minute load average.
    avg_5: f64,
    /// 15-minute load average.
    avg_15: f64,
    /// Number of currently running processes.
    running: u32,
    /// Total number of processes.
    total: u32,
}

/// Decomposed uptime into days, hours, minutes, seconds.
struct UptimeParts {
    days: u64,
    hours: u64,
    minutes: u64,
    seconds: u64,
}

/// All information needed for display.
struct SystemInfo {
    uptime: UptimeInfo,
    load: Option<LoadAvg>,
    user_count: u32,
    /// Current wall-clock time as seconds since Unix epoch.
    now_epoch: u64,
}

/// Which output mode was selected.
#[derive(PartialEq)]
enum Mode {
    /// Standard one-line display (default).
    Standard,
    /// `-p` / `--pretty`: human-readable uptime only.
    Pretty,
    /// `-s` / `--since`: boot date/time.
    Since,
    /// `-r` / `--raw`: raw seconds.
    Raw,
    /// `--json`: machine-readable JSON.
    Json,
}

// ============================================================================
// File reading helpers
// ============================================================================

/// Read the contents of a file, returning `None` on any I/O error.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

// ============================================================================
// /proc/uptime parser
// ============================================================================

/// Parse `/proc/uptime` which contains two space-separated floats:
/// total uptime seconds and total idle time seconds.
fn read_uptime() -> Option<UptimeInfo> {
    let content = read_file("/proc/uptime")?;
    let mut parts = content.split_whitespace();

    let total_seconds: f64 = parts.next()?.parse().ok()?;
    let idle_seconds: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);

    Some(UptimeInfo {
        total_seconds,
        idle_seconds,
    })
}

// ============================================================================
// /proc/loadavg parser
// ============================================================================

/// Parse `/proc/loadavg` which contains:
/// `0.15 0.10 0.05 2/150 12345`
/// (1min 5min 15min running/total last_pid)
fn read_loadavg() -> Option<LoadAvg> {
    let content = read_file("/proc/loadavg")?;
    let mut parts = content.split_whitespace();

    let avg_1: f64 = parts.next()?.parse().ok()?;
    let avg_5: f64 = parts.next()?.parse().ok()?;
    let avg_15: f64 = parts.next()?.parse().ok()?;

    // Fourth field is "running/total".
    let procs_field = parts.next().unwrap_or("0/0");
    let (running, total) = parse_procs_field(procs_field);

    Some(LoadAvg {
        avg_1,
        avg_5,
        avg_15,
        running,
        total,
    })
}

/// Parse "running/total" process field from /proc/loadavg.
fn parse_procs_field(field: &str) -> (u32, u32) {
    if let Some((r, t)) = field.split_once('/') {
        let running = r.parse().unwrap_or(0);
        let total = t.parse().unwrap_or(0);
        (running, total)
    } else {
        (0, 0)
    }
}

// ============================================================================
// Boot time from /proc/stat
// ============================================================================

/// Read boot time (seconds since epoch) from the `btime` line in `/proc/stat`.
fn read_btime() -> Option<u64> {
    let content = read_file("/proc/stat")?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("btime ") {
            return rest.trim().parse().ok();
        }
    }
    None
}

// ============================================================================
// User count
// ============================================================================

/// Count logged-in users. Tries multiple sources in order:
///
/// 1. `/var/run/utmp` — parse binary utmp records (simplified)
/// 2. Count directories in `/run/user/` (each UID with an active session)
/// 3. Count entries in `/tmp/.users/` (SlateOS-specific fallback)
/// 4. Default to 0 if nothing works.
fn count_users() -> u32 {
    // Try /run/user/ directory listing — each subdirectory is a UID with an
    // active session.
    if let Ok(entries) = fs::read_dir("/run/user") {
        let count = entries.filter(|e| e.is_ok()).count();
        if count > 0 {
            return count as u32;
        }
    }

    // Try /tmp/.users/ as an SlateOS-specific fallback.
    if let Ok(entries) = fs::read_dir("/tmp/.users") {
        let count = entries.filter(|e| e.is_ok()).count();
        if count > 0 {
            return count as u32;
        }
    }

    // Try parsing utmp — each USER_PROCESS record (type 7) is a login session.
    if let Some(data) = read_utmp_user_count("/var/run/utmp") {
        return data;
    }

    0
}

/// Parse utmp file for user session count.
///
/// The utmp record format on Linux x86_64 is 384 bytes per entry. We look for
/// records with ut_type == 7 (USER_PROCESS) to count logged-in users.
///
/// On Slate OS the utmp format may differ; this function degrades gracefully by
/// returning `None` if the file is too small or does not parse.
fn read_utmp_user_count(path: &str) -> Option<u32> {
    let data = fs::read(path).ok()?;

    // Standard Linux utmp record size for x86_64.
    const UTMP_RECORD_SIZE: usize = 384;
    // Offset of ut_type field (i32, little-endian).
    const UT_TYPE_OFFSET: usize = 0;
    // USER_PROCESS type value.
    const USER_PROCESS: i32 = 7;

    if data.len() < UTMP_RECORD_SIZE {
        return None;
    }

    let mut count: u32 = 0;
    let mut offset = 0;

    while offset + UTMP_RECORD_SIZE <= data.len() {
        // Read ut_type as a little-endian i32.
        let type_bytes = data.get(offset + UT_TYPE_OFFSET..offset + UT_TYPE_OFFSET + 4)?;
        let ut_type = i32::from_le_bytes([
            type_bytes[0],
            type_bytes[1],
            type_bytes[2],
            type_bytes[3],
        ]);

        if ut_type == USER_PROCESS {
            count = count.saturating_add(1);
        }

        offset += UTMP_RECORD_SIZE;
    }

    Some(count)
}

// ============================================================================
// Time calculations
// ============================================================================

/// Decompose total seconds into days, hours, minutes, seconds.
fn decompose_uptime(total_seconds: f64) -> UptimeParts {
    let secs = total_seconds as u64;
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    UptimeParts {
        days,
        hours,
        minutes,
        seconds,
    }
}

/// Get the current time as seconds since the Unix epoch.
fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Date/time formatting (no external crate)
// ============================================================================

/// Whether a year is a leap year in the Gregorian calendar.
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days in a given month (1-indexed) of a given year.
fn days_in_month(year: i64, month: u32) -> u32 {
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
        _ => 30, // defensive fallback
    }
}

/// Convert a Unix epoch timestamp to a formatted "YYYY-MM-DD HH:MM:SS" string.
fn epoch_to_datetime(epoch_secs: u64) -> String {
    // Days and remaining seconds since epoch.
    let total_days = (epoch_secs / 86400) as i64;
    let day_seconds = epoch_secs % 86400;

    let hours = day_seconds / 3600;
    let minutes = (day_seconds % 3600) / 60;
    let seconds = day_seconds % 60;

    // Convert days since epoch (1970-01-01) to year/month/day.
    let mut year: i64 = 1970;
    let mut remaining_days = total_days;

    // Fast-forward by 400-year cycles (146097 days each).
    let cycles_400 = remaining_days / 146097;
    remaining_days %= 146097;
    year += cycles_400 * 400;

    loop {
        let days_this_year: i64 = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_this_year {
            break;
        }
        remaining_days -= days_this_year;
        year += 1;
    }

    // Find month and day within the year.
    let mut month: u32 = 1;
    while month <= 12 {
        let dim = days_in_month(year, month) as i64;
        if remaining_days < dim {
            break;
        }
        remaining_days -= dim;
        month += 1;
    }

    let day = remaining_days + 1; // days are 1-indexed

    format!(
        "{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}",
        day = day,
    )
}

/// Format just the time portion (HH:MM:SS) from epoch seconds.
fn epoch_to_time(epoch_secs: u64) -> String {
    let day_seconds = epoch_secs % 86400;
    let hours = day_seconds / 3600;
    let minutes = (day_seconds % 3600) / 60;
    let seconds = day_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

// ============================================================================
// Output formatting
// ============================================================================

/// Format the uptime duration for the standard display line.
///
/// Follows Linux conventions:
/// - "N days, H:MM" if days > 0
/// - "H:MM" if hours > 0 but days == 0
/// - "N min" if only minutes
fn format_uptime_standard(parts: &UptimeParts) -> String {
    if parts.days > 0 {
        let day_word = if parts.days == 1 { "day" } else { "days" };
        format!(
            "{} {day_word}, {:2}:{:02}",
            parts.days, parts.hours, parts.minutes
        )
    } else if parts.hours > 0 {
        format!("{:2}:{:02}", parts.hours, parts.minutes)
    } else {
        format!("{} min", parts.minutes)
    }
}

/// Format the uptime for `--pretty` mode.
///
/// Examples:
/// - "up 3 days, 2 hours, 15 minutes"
/// - "up 45 minutes"
/// - "up 1 day, 0 hours, 30 minutes"
fn format_uptime_pretty(parts: &UptimeParts) -> String {
    let mut segments = Vec::new();

    if parts.days > 0 {
        let word = if parts.days == 1 { "day" } else { "days" };
        segments.push(format!("{} {word}", parts.days));
    }

    if parts.hours > 0 || parts.days > 0 {
        let word = if parts.hours == 1 { "hour" } else { "hours" };
        segments.push(format!("{} {word}", parts.hours));
    }

    let min_word = if parts.minutes == 1 {
        "minute"
    } else {
        "minutes"
    };
    segments.push(format!("{} {min_word}", parts.minutes));

    format!("up {}", segments.join(", "))
}

/// Escape a string for JSON output.
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
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    let _ = std::fmt::Write::write_fmt(
                        &mut out,
                        format_args!("\\u{unit:04x}"),
                    );
                }
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Display functions
// ============================================================================

/// Standard output: ` 14:30:45 up 3 days, 2:15,  2 users,  load average: 0.15, 0.10, 0.05`
fn print_standard(info: &SystemInfo) {
    let time_str = epoch_to_time(info.now_epoch);
    let parts = decompose_uptime(info.uptime.total_seconds);
    let up_str = format_uptime_standard(&parts);

    let user_word = if info.user_count == 1 {
        "user"
    } else {
        "users"
    };

    let load_str = match &info.load {
        Some(load) => format!(
            "  load average: {:.2}, {:.2}, {:.2}",
            load.avg_1, load.avg_5, load.avg_15
        ),
        None => String::new(),
    };

    println!(
        " {time_str} up {up_str},  {users} {user_word},{load_str}",
        users = info.user_count,
    );
}

/// Pretty output: "up 3 days, 2 hours, 15 minutes"
fn print_pretty(info: &SystemInfo) {
    let parts = decompose_uptime(info.uptime.total_seconds);
    println!("{}", format_uptime_pretty(&parts));
}

/// Since output: "YYYY-MM-DD HH:MM:SS"
fn print_since(info: &SystemInfo) {
    // Try btime from /proc/stat first for a more accurate boot timestamp.
    if let Some(btime) = read_btime() {
        println!("{}", epoch_to_datetime(btime));
        return;
    }

    // Fall back to current_time - uptime.
    let boot_epoch = info
        .now_epoch
        .saturating_sub(info.uptime.total_seconds as u64);
    println!("{}", epoch_to_datetime(boot_epoch));
}

/// Raw output: total seconds as a number.
fn print_raw(info: &SystemInfo) {
    println!("{:.2}", info.uptime.total_seconds);
}

/// JSON output with all available fields.
fn print_json(info: &SystemInfo) {
    let parts = decompose_uptime(info.uptime.total_seconds);
    let boot_epoch = info
        .now_epoch
        .saturating_sub(info.uptime.total_seconds as u64);
    let boot_dt = epoch_to_datetime(boot_epoch);
    let current_dt = epoch_to_datetime(info.now_epoch);
    let pretty = format_uptime_pretty(&parts);

    println!("{{");
    println!("  \"uptime_seconds\": {:.2},", info.uptime.total_seconds);
    println!("  \"idle_seconds\": {:.2},", info.uptime.idle_seconds);
    println!("  \"days\": {},", parts.days);
    println!("  \"hours\": {},", parts.hours);
    println!("  \"minutes\": {},", parts.minutes);
    println!("  \"seconds\": {},", parts.seconds);
    println!(
        "  \"pretty\": \"{}\",",
        json_escape(&pretty)
    );
    println!(
        "  \"boot_time\": \"{}\",",
        json_escape(&boot_dt)
    );
    println!(
        "  \"current_time\": \"{}\",",
        json_escape(&current_dt)
    );
    println!("  \"users\": {},", info.user_count);

    match &info.load {
        Some(load) => {
            println!("  \"load_average\": {{");
            println!("    \"1min\": {:.2},", load.avg_1);
            println!("    \"5min\": {:.2},", load.avg_5);
            println!("    \"15min\": {:.2}", load.avg_15);
            println!("  }},");
            println!("  \"processes\": {{");
            println!("    \"running\": {},", load.running);
            println!("    \"total\": {}", load.total);
            println!("  }}");
        }
        None => {
            println!("  \"load_average\": null,");
            println!("  \"processes\": null");
        }
    }

    println!("}}");
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_help() {
    println!("Slate OS System Uptime Display v0.1.0");
    println!();
    println!("Show how long the system has been running.");
    println!();
    println!("USAGE:");
    println!("  uptime [OPTION]");
    println!();
    println!("OPTIONS:");
    println!("  -p, --pretty   Show uptime in human-readable format");
    println!("  -s, --since    Show date/time of last boot (YYYY-MM-DD HH:MM:SS)");
    println!("  -r, --raw      Show raw uptime in seconds");
    println!("      --json     Output all fields in JSON format");
    println!("  -V, --version  Display version and exit");
    println!("  -h, --help     Display this help and exit");
}

fn print_version() {
    println!("uptime (Slate OS) 0.1.0");
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();
    let mut mode = Mode::Standard;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--pretty" => mode = Mode::Pretty,
            "-s" | "--since" => mode = Mode::Since,
            "-r" | "--raw" => mode = Mode::Raw,
            "--json" => mode = Mode::Json,
            "-V" | "--version" => {
                print_version();
                return 0;
            }
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            other => {
                eprintln!("uptime: unknown option: {other}");
                eprintln!("Try 'uptime --help' for usage.");
                return 1;
            }
        }
        i += 1;
    }

    // Read uptime — this is the only mandatory data source.
    let uptime = match read_uptime() {
        Some(u) => u,
        None => {
            eprintln!("uptime: failed to read /proc/uptime");
            return 1;
        }
    };

    let load = read_loadavg();
    let user_count = count_users();
    let now_epoch = current_epoch_seconds();

    let info = SystemInfo {
        uptime,
        load,
        user_count,
        now_epoch,
    };

    match mode {
        Mode::Standard => print_standard(&info),
        Mode::Pretty => print_pretty(&info),
        Mode::Since => print_since(&info),
        Mode::Raw => print_raw(&info),
        Mode::Json => print_json(&info),
    }

    0
}

fn main() {
    process::exit(run());
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Uptime parsing ---

    #[test]
    fn test_decompose_uptime_full() {
        let parts = decompose_uptime(273_915.0); // 3 days, 4 hours, 5 min, 15 sec
        assert_eq!(parts.days, 3);
        assert_eq!(parts.hours, 4);
        assert_eq!(parts.minutes, 5);
        assert_eq!(parts.seconds, 15);
    }

    #[test]
    fn test_decompose_uptime_zero() {
        let parts = decompose_uptime(0.0);
        assert_eq!(parts.days, 0);
        assert_eq!(parts.hours, 0);
        assert_eq!(parts.minutes, 0);
        assert_eq!(parts.seconds, 0);
    }

    #[test]
    fn test_decompose_uptime_fractional() {
        // Fractional seconds are truncated.
        let parts = decompose_uptime(90.7);
        assert_eq!(parts.days, 0);
        assert_eq!(parts.hours, 0);
        assert_eq!(parts.minutes, 1);
        assert_eq!(parts.seconds, 30);
    }

    #[test]
    fn test_decompose_uptime_exactly_one_day() {
        let parts = decompose_uptime(86400.0);
        assert_eq!(parts.days, 1);
        assert_eq!(parts.hours, 0);
        assert_eq!(parts.minutes, 0);
        assert_eq!(parts.seconds, 0);
    }

    // --- Process field parsing ---

    #[test]
    fn test_parse_procs_field_normal() {
        let (r, t) = parse_procs_field("2/150");
        assert_eq!(r, 2);
        assert_eq!(t, 150);
    }

    #[test]
    fn test_parse_procs_field_empty() {
        let (r, t) = parse_procs_field("");
        assert_eq!(r, 0);
        assert_eq!(t, 0);
    }

    #[test]
    fn test_parse_procs_field_no_slash() {
        let (r, t) = parse_procs_field("42");
        assert_eq!(r, 0);
        assert_eq!(t, 0);
    }

    #[test]
    fn test_parse_procs_field_non_numeric() {
        let (r, t) = parse_procs_field("abc/def");
        assert_eq!(r, 0);
        assert_eq!(t, 0);
    }

    // --- Standard uptime formatting ---

    #[test]
    fn test_format_standard_days() {
        let parts = UptimeParts {
            days: 3,
            hours: 2,
            minutes: 15,
            seconds: 0,
        };
        let s = format_uptime_standard(&parts);
        assert!(s.contains("3 days"), "got: {s}");
        assert!(s.contains("2:15"), "got: {s}");
    }

    #[test]
    fn test_format_standard_one_day() {
        let parts = UptimeParts {
            days: 1,
            hours: 0,
            minutes: 5,
            seconds: 0,
        };
        let s = format_uptime_standard(&parts);
        assert!(s.contains("1 day,"), "got: {s}");
        assert!(!s.contains("days"), "got: {s}");
    }

    #[test]
    fn test_format_standard_hours_only() {
        let parts = UptimeParts {
            days: 0,
            hours: 5,
            minutes: 30,
            seconds: 0,
        };
        let s = format_uptime_standard(&parts);
        assert!(s.contains("5:30"), "got: {s}");
        assert!(!s.contains("day"), "got: {s}");
    }

    #[test]
    fn test_format_standard_minutes_only() {
        let parts = UptimeParts {
            days: 0,
            hours: 0,
            minutes: 45,
            seconds: 0,
        };
        let s = format_uptime_standard(&parts);
        assert!(s.contains("45 min"), "got: {s}");
    }

    // --- Pretty formatting ---

    #[test]
    fn test_format_pretty_full() {
        let parts = UptimeParts {
            days: 3,
            hours: 2,
            minutes: 15,
            seconds: 0,
        };
        let s = format_uptime_pretty(&parts);
        assert_eq!(s, "up 3 days, 2 hours, 15 minutes");
    }

    #[test]
    fn test_format_pretty_singular() {
        let parts = UptimeParts {
            days: 1,
            hours: 1,
            minutes: 1,
            seconds: 0,
        };
        let s = format_uptime_pretty(&parts);
        assert_eq!(s, "up 1 day, 1 hour, 1 minute");
    }

    #[test]
    fn test_format_pretty_minutes_only() {
        let parts = UptimeParts {
            days: 0,
            hours: 0,
            minutes: 45,
            seconds: 0,
        };
        let s = format_uptime_pretty(&parts);
        assert_eq!(s, "up 45 minutes");
    }

    #[test]
    fn test_format_pretty_zero() {
        let parts = UptimeParts {
            days: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        };
        let s = format_uptime_pretty(&parts);
        assert_eq!(s, "up 0 minutes");
    }

    // --- Date/time formatting ---

    #[test]
    fn test_epoch_to_datetime_epoch_zero() {
        assert_eq!(epoch_to_datetime(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn test_epoch_to_datetime_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(epoch_to_datetime(1_704_067_200), "2024-01-01 00:00:00");
    }

    #[test]
    fn test_epoch_to_datetime_with_time() {
        // 2024-06-15 14:30:45 UTC = 1718461845
        assert_eq!(epoch_to_datetime(1_718_461_845), "2024-06-15 14:30:45");
    }

    #[test]
    fn test_epoch_to_datetime_leap_year_feb29() {
        // 2024-02-29 12:00:00 UTC = 1709208000
        assert_eq!(epoch_to_datetime(1_709_208_000), "2024-02-29 12:00:00");
    }

    #[test]
    fn test_epoch_to_datetime_year_2000() {
        // 2000-01-01 00:00:00 UTC = 946684800
        assert_eq!(epoch_to_datetime(946_684_800), "2000-01-01 00:00:00");
    }

    #[test]
    fn test_epoch_to_datetime_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        assert_eq!(epoch_to_datetime(1_704_067_199), "2023-12-31 23:59:59");
    }

    #[test]
    fn test_epoch_to_time() {
        let s = epoch_to_time(52245); // 14:30:45
        assert_eq!(s, "14:30:45");
    }

    #[test]
    fn test_epoch_to_time_midnight() {
        assert_eq!(epoch_to_time(0), "00:00:00");
    }

    #[test]
    fn test_epoch_to_time_end_of_day() {
        assert_eq!(epoch_to_time(86399), "23:59:59");
    }

    // --- Leap year ---

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2400));
        assert!(!is_leap_year(2100));
    }

    // --- Days in month ---

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 2), 29); // leap
        assert_eq!(days_in_month(2023, 2), 28); // non-leap
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
    }

    // --- JSON escape ---

    #[test]
    fn test_json_escape_plain() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn test_json_escape_quotes() {
        assert_eq!(json_escape("say \"hi\""), "say \\\"hi\\\"");
    }

    #[test]
    fn test_json_escape_backslash() {
        assert_eq!(json_escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_json_escape_control() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
    }

    // --- utmp parsing ---

    #[test]
    fn test_read_utmp_empty() {
        // Empty data should return None (too small for any record).
        assert_eq!(read_utmp_user_count("/nonexistent/path"), None);
    }
}
