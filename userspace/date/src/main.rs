//! OurOS Date and Time Utility
//!
//! Displays or sets the system date and time. Supports strftime-like format
//! strings, multiple output formats (RFC 5322, RFC 3339, ISO 8601, JSON),
//! and date parsing.
//!
//! All date math (epoch conversion, leap years, day-of-week) is implemented
//! from scratch with no external crate dependencies.
//!
//! # Usage
//!
//! ```text
//! date                         Display current date/time
//! date +FORMAT                 Display with custom format
//! date -u                      Display in UTC
//! date -d "2025-01-15 10:30"   Display given date
//! date -s "2025-01-15 10:30"   Set system date/time (requires root)
//! date -R                      RFC 5322 format
//! date -Iseconds               ISO 8601 format
//! date --rfc-3339=seconds      RFC 3339 format
//! date --json                  JSON output
//! date -r <file>               Show file modification time
//! ```
//!
//! # Syscall Interface
//!
//! Uses `clock_gettime` (syscall 40) and `clock_settime` (syscall 41) via
//! inline x86_64 assembly.

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall constants
// ============================================================================

const SYS_CLOCK_GETTIME: u64 = 40;
const SYS_CLOCK_SETTIME: u64 = 41;
const CLOCK_REALTIME: u64 = 0;

// ============================================================================
// Timespec
// ============================================================================

/// Kernel timespec structure for clock_gettime/clock_settime.
#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Read the current time from the specified clock.
///
/// Returns (seconds_since_epoch, nanoseconds) on success.
fn clock_gettime(clock_id: u64) -> Result<(i64, i64), String> {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let ret: i64;

    // SAFETY: We pass a valid pointer to a stack-allocated Timespec struct.
    // The kernel writes tv_sec and tv_nsec into it. The struct is repr(C)
    // with the expected layout. The pointer is valid for the duration of the
    // syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_CLOCK_GETTIME,
            in("rdi") clock_id,
            in("rsi") &mut ts as *mut Timespec,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }

    if ret < 0 {
        return Err(format!("clock_gettime failed with error {ret}"));
    }

    Ok((ts.tv_sec, ts.tv_nsec))
}

/// Set the specified clock to the given time.
///
/// Requires root privileges.
fn clock_settime(clock_id: u64, sec: i64, nsec: i64) -> Result<(), String> {
    let ts = Timespec {
        tv_sec: sec,
        tv_nsec: nsec,
    };
    let ret: i64;

    // SAFETY: We pass a valid pointer to a stack-allocated Timespec struct.
    // The kernel reads tv_sec and tv_nsec from it. The struct is repr(C)
    // with the expected layout. The pointer is valid for the duration of the
    // syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_CLOCK_SETTIME,
            in("rdi") clock_id,
            in("rsi") &ts as *const Timespec,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }

    if ret < 0 {
        return Err(format!(
            "clock_settime failed with error {ret} (are you root?)"
        ));
    }

    Ok(())
}

// ============================================================================
// Date/time representation
// ============================================================================

/// Broken-down date and time with nanosecond precision.
#[derive(Clone, Debug)]
struct DateTime {
    year: i64,
    month: u32,  // 1-12
    day: u32,    // 1-31
    hour: u32,   // 0-23
    minute: u32, // 0-59
    second: u32, // 0-59
    nsec: i64,   // 0-999_999_999
}

// ============================================================================
// Calendar math — all from scratch, no chrono
// ============================================================================

/// Cumulative days before each month in a non-leap year (index 0 = before Jan).
const DAYS_BEFORE_MONTH: [u32; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

/// Days in each month for a non-leap year (index 0 unused, 1=Jan..12=Dec).
const DAYS_IN_MONTH: [u32; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

const WEEKDAY_ABBR: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAY_FULL: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

const MONTH_ABBR: [&str; 13] = [
    "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_FULL: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Returns true if `year` is a leap year under the Gregorian calendar.
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Number of days in a given month (1-based) for a given year.
fn days_in_month(year: i64, month: u32) -> u32 {
    if month == 2 && is_leap_year(year) {
        29
    } else if month >= 1 && month <= 12 {
        DAYS_IN_MONTH[month as usize]
    } else {
        0
    }
}

/// Day of year (1-366) for the given date.
fn day_of_year(year: i64, month: u32, day: u32) -> u32 {
    if month < 1 || month > 12 {
        return 0;
    }
    let mut doy = DAYS_BEFORE_MONTH[month as usize] + day;
    if month > 2 && is_leap_year(year) {
        doy += 1;
    }
    doy
}

/// Day of week using Tomohiko Sakamoto's algorithm.
///
/// Returns 0=Monday, 1=Tuesday, ..., 6=Sunday (ISO 8601 convention).
fn day_of_week(year: i64, month: u32, day: u32) -> u32 {
    // Sakamoto's algorithm works with 0=Sunday, so we adjust at the end.
    static T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];

    let mut y = year;
    if month < 3 {
        y -= 1;
    }
    let m = month as i64;
    let d = day as i64;

    // The algorithm gives 0=Sunday, 1=Monday, ..., 6=Saturday.
    let dow = ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d) % 7 + 7) % 7;

    // Convert to ISO: 0=Monday..6=Sunday.
    // Sakamoto: 0=Sun, 1=Mon, 2=Tue, 3=Wed, 4=Thu, 5=Fri, 6=Sat
    // ISO:      0=Mon, 1=Tue, 2=Wed, 3=Thu, 4=Fri, 5=Sat, 6=Sun
    ((dow + 6) % 7) as u32
}

/// Convert Unix epoch seconds to broken-down DateTime.
///
/// Handles both positive and negative epoch values (dates before 1970).
fn epoch_to_datetime(epoch_sec: i64, nsec: i64) -> DateTime {
    // Algorithm based on Howard Hinnant's civil_from_days.
    // Reference: http://howardhinnant.github.io/date_algorithms.html

    let secs_per_day: i64 = 86400;

    // Split into days and time-of-day. Use Euclidean division so remainder
    // is always non-negative.
    let total_days = epoch_sec.div_euclid(secs_per_day);
    let day_seconds = epoch_sec.rem_euclid(secs_per_day);

    let hour = (day_seconds / 3600) as u32;
    let minute = ((day_seconds % 3600) / 60) as u32;
    let second = (day_seconds % 60) as u32;

    // Shift epoch from 1970-01-01 to 0000-03-01 for easier month math.
    // Days from 0000-03-01 to 1970-01-01 = 719468.
    let z = total_days + 719_468;

    // Era (400-year cycle).
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // year of era [0, 399]
    let y = yoe + era * 400;
    let doy_from_mar = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year from March 1 [0, 365]
    let mp = (5 * doy_from_mar + 2) / 153; // month index from March [0, 11]
    let d = doy_from_mar - (153 * mp + 2) / 5 + 1; // day of month [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    DateTime {
        year,
        month: m as u32,
        day: d as u32,
        hour,
        minute,
        second,
        nsec,
    }
}

/// Convert broken-down DateTime to Unix epoch seconds.
fn datetime_to_epoch(dt: &DateTime) -> Result<i64, String> {
    // Validate fields.
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

    // Howard Hinnant's days_from_civil.
    let y = if dt.month <= 2 {
        dt.year - 1
    } else {
        dt.year
    };
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

    let epoch = total_days * 86400
        + dt.hour as i64 * 3600
        + dt.minute as i64 * 60
        + dt.second as i64;

    Ok(epoch)
}

// ============================================================================
// Format string processing
// ============================================================================

/// Format a DateTime according to a strftime-like format string.
///
/// Supports: %Y %m %d %H %M %S %a %A %b %B %j %u %Z %z %s %N %n %t %%
///           %F %T %R %c %x %X %D %r %p %P %I %e %k
fn format_datetime(dt: &DateTime, epoch_sec: i64, fmt: &str) -> String {
    let mut result = String::with_capacity(fmt.len() * 2);
    let mut chars = fmt.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }

        match chars.next() {
            None => result.push('%'),
            Some(spec) => match spec {
                // Year, 4-digit.
                'Y' => result.push_str(&format!("{:04}", dt.year)),
                // Year, 2-digit.
                'y' => {
                    let y2 = dt.year.rem_euclid(100);
                    result.push_str(&format!("{y2:02}"));
                }
                // Month, zero-padded.
                'm' => result.push_str(&format!("{:02}", dt.month)),
                // Day, zero-padded.
                'd' => result.push_str(&format!("{:02}", dt.day)),
                // Day, space-padded.
                'e' => result.push_str(&format!("{:2}", dt.day)),
                // Hour (00-23), zero-padded.
                'H' => result.push_str(&format!("{:02}", dt.hour)),
                // Hour (0-23), space-padded.
                'k' => result.push_str(&format!("{:2}", dt.hour)),
                // Hour (01-12), zero-padded.
                'I' => {
                    let h12 = match dt.hour % 12 {
                        0 => 12,
                        h => h,
                    };
                    result.push_str(&format!("{h12:02}"));
                }
                // Minute, zero-padded.
                'M' => result.push_str(&format!("{:02}", dt.minute)),
                // Second, zero-padded.
                'S' => result.push_str(&format!("{:02}", dt.second)),
                // Abbreviated weekday name.
                'a' => {
                    let dow = day_of_week(dt.year, dt.month, dt.day) as usize;
                    if dow < WEEKDAY_ABBR.len() {
                        result.push_str(WEEKDAY_ABBR[dow]);
                    }
                }
                // Full weekday name.
                'A' => {
                    let dow = day_of_week(dt.year, dt.month, dt.day) as usize;
                    if dow < WEEKDAY_FULL.len() {
                        result.push_str(WEEKDAY_FULL[dow]);
                    }
                }
                // Abbreviated month name.
                'b' | 'h' => {
                    let m = dt.month as usize;
                    if m >= 1 && m <= 12 {
                        result.push_str(MONTH_ABBR[m]);
                    }
                }
                // Full month name.
                'B' => {
                    let m = dt.month as usize;
                    if m >= 1 && m <= 12 {
                        result.push_str(MONTH_FULL[m]);
                    }
                }
                // Day of year (001-366).
                'j' => {
                    let doy = day_of_year(dt.year, dt.month, dt.day);
                    result.push_str(&format!("{doy:03}"));
                }
                // Weekday number (1=Mon, 7=Sun), ISO 8601.
                'u' => {
                    let dow = day_of_week(dt.year, dt.month, dt.day);
                    result.push_str(&format!("{}", dow + 1));
                }
                // Weekday number (0=Sun, 6=Sat).
                'w' => {
                    let dow = day_of_week(dt.year, dt.month, dt.day);
                    // dow: 0=Mon..6=Sun -> we need 0=Sun, 1=Mon..6=Sat
                    let w = if dow == 6 { 0 } else { dow + 1 };
                    result.push_str(&format!("{w}"));
                }
                // Timezone name (we only support UTC for now).
                'Z' => result.push_str("UTC"),
                // Timezone offset.
                'z' => result.push_str("+0000"),
                // Unix epoch seconds.
                's' => result.push_str(&format!("{epoch_sec}")),
                // Nanoseconds.
                'N' => result.push_str(&format!("{:09}", dt.nsec)),
                // Newline.
                'n' => result.push('\n'),
                // Tab.
                't' => result.push('\t'),
                // Literal percent.
                '%' => result.push('%'),

                // Composite formats.
                // %F = %Y-%m-%d
                'F' => {
                    result.push_str(&format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day));
                }
                // %T = %H:%M:%S
                'T' => {
                    result.push_str(&format!(
                        "{:02}:{:02}:{:02}",
                        dt.hour, dt.minute, dt.second
                    ));
                }
                // %R = %H:%M
                'R' => {
                    result.push_str(&format!("{:02}:{:02}", dt.hour, dt.minute));
                }
                // %D = %m/%d/%y
                'D' => {
                    let y2 = dt.year.rem_euclid(100);
                    result.push_str(&format!("{:02}/{:02}/{y2:02}", dt.month, dt.day));
                }
                // %c = locale date/time (use default format).
                'c' => {
                    let dow = day_of_week(dt.year, dt.month, dt.day) as usize;
                    let wday = if dow < WEEKDAY_ABBR.len() {
                        WEEKDAY_ABBR[dow]
                    } else {
                        "???"
                    };
                    let m = dt.month as usize;
                    let mon = if m >= 1 && m <= 12 {
                        MONTH_ABBR[m]
                    } else {
                        "???"
                    };
                    result.push_str(&format!(
                        "{wday} {mon} {:2} {:02}:{:02}:{:02} {:04}",
                        dt.day, dt.hour, dt.minute, dt.second, dt.year
                    ));
                }
                // %x = locale date.
                'x' => {
                    let y2 = dt.year.rem_euclid(100);
                    result.push_str(&format!("{:02}/{:02}/{y2:02}", dt.month, dt.day));
                }
                // %X = locale time.
                'X' => {
                    result.push_str(&format!(
                        "{:02}:{:02}:{:02}",
                        dt.hour, dt.minute, dt.second
                    ));
                }
                // %r = 12-hour time with AM/PM.
                'r' => {
                    let h12 = match dt.hour % 12 {
                        0 => 12,
                        h => h,
                    };
                    let ampm = if dt.hour < 12 { "AM" } else { "PM" };
                    result.push_str(&format!(
                        "{h12:02}:{:02}:{:02} {ampm}",
                        dt.minute, dt.second
                    ));
                }
                // %p = AM/PM uppercase.
                'p' => {
                    result.push_str(if dt.hour < 12 { "AM" } else { "PM" });
                }
                // %P = am/pm lowercase.
                'P' => {
                    result.push_str(if dt.hour < 12 { "am" } else { "pm" });
                }
                // Unknown specifier: output as-is.
                other => {
                    result.push('%');
                    result.push(other);
                }
            },
        }
    }

    result
}

// ============================================================================
// Date string parsing
// ============================================================================

/// Parse a date string in common formats.
///
/// Supported formats:
/// - "YYYY-MM-DD HH:MM:SS"
/// - "YYYY-MM-DD HH:MM"
/// - "YYYY-MM-DD"
/// - "YYYY-MM-DDTHH:MM:SS" (ISO 8601 with T separator)
/// - "YYYY-MM-DDTHH:MM"
/// - "MM/DD/YYYY HH:MM:SS"
/// - "MM/DD/YYYY"
/// - "@EPOCH" (Unix timestamp)
fn parse_date_string(s: &str) -> Result<(i64, i64), String> {
    let s = s.trim();

    // Unix timestamp: @<seconds>
    if let Some(rest) = s.strip_prefix('@') {
        let sec: i64 = rest
            .parse()
            .map_err(|_| format!("invalid epoch timestamp: {rest}"))?;
        return Ok((sec, 0));
    }

    // Try ISO 8601 with T separator: replace T with space.
    let normalized = s.replace('T', " ");

    // Try YYYY-MM-DD based formats.
    if let Some(result) = try_parse_ymd(&normalized) {
        return result;
    }

    // Try MM/DD/YYYY format.
    if let Some(result) = try_parse_mdy(&normalized) {
        return result;
    }

    Err(format!(
        "cannot parse date string: '{s}'\n\
         Expected formats: YYYY-MM-DD [HH:MM[:SS]], MM/DD/YYYY [HH:MM[:SS]], or @EPOCH"
    ))
}

/// Attempt to parse "YYYY-MM-DD" with optional " HH:MM[:SS]" suffix.
fn try_parse_ymd(s: &str) -> Option<Result<(i64, i64), String>> {
    // Must have at least "YYYY-MM-DD" = 10 chars, with dashes at positions 4 and 7.
    if s.len() < 10 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes.get(4).copied() != Some(b'-') || bytes.get(7).copied() != Some(b'-') {
        return None;
    }

    let year: i64 = match s.get(0..4).and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return Some(Err("invalid year".to_string())),
    };
    let month: u32 = match s.get(5..7).and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return Some(Err("invalid month".to_string())),
    };
    let day: u32 = match s.get(8..10).and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return Some(Err("invalid day".to_string())),
    };

    let (hour, minute, second) = if s.len() > 10 {
        match parse_time_suffix(s.get(11..).unwrap_or("")) {
            Ok(hms) => hms,
            Err(e) => return Some(Err(e)),
        }
    } else {
        (0, 0, 0)
    };

    let dt = DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        nsec: 0,
    };

    Some(datetime_to_epoch(&dt).map(|sec| (sec, 0)))
}

/// Attempt to parse "MM/DD/YYYY" with optional " HH:MM[:SS]" suffix.
fn try_parse_mdy(s: &str) -> Option<Result<(i64, i64), String>> {
    // Must have at least "MM/DD/YYYY" = 10 chars.
    if s.len() < 10 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes.get(2).copied() != Some(b'/') || bytes.get(5).copied() != Some(b'/') {
        return None;
    }

    let month: u32 = match s.get(0..2).and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return Some(Err("invalid month".to_string())),
    };
    let day: u32 = match s.get(3..5).and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return Some(Err("invalid day".to_string())),
    };
    let year: i64 = match s.get(6..10).and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return Some(Err("invalid year".to_string())),
    };

    let (hour, minute, second) = if s.len() > 10 {
        match parse_time_suffix(s.get(11..).unwrap_or("")) {
            Ok(hms) => hms,
            Err(e) => return Some(Err(e)),
        }
    } else {
        (0, 0, 0)
    };

    let dt = DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        nsec: 0,
    };

    Some(datetime_to_epoch(&dt).map(|sec| (sec, 0)))
}

/// Parse a time suffix like "HH:MM" or "HH:MM:SS" (with optional leading space).
fn parse_time_suffix(s: &str) -> Result<(u32, u32, u32), String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok((0, 0, 0));
    }

    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        2 => {
            let hour: u32 = parts[0]
                .parse()
                .map_err(|_| format!("invalid hour: {}", parts[0]))?;
            let minute: u32 = parts[1]
                .parse()
                .map_err(|_| format!("invalid minute: {}", parts[1]))?;
            Ok((hour, minute, 0))
        }
        3 => {
            let hour: u32 = parts[0]
                .parse()
                .map_err(|_| format!("invalid hour: {}", parts[0]))?;
            let minute: u32 = parts[1]
                .parse()
                .map_err(|_| format!("invalid minute: {}", parts[1]))?;
            let second: u32 = parts[2]
                .parse()
                .map_err(|_| format!("invalid second: {}", parts[2]))?;
            Ok((hour, minute, second))
        }
        _ => Err(format!("invalid time format: '{s}' (expected HH:MM or HH:MM:SS)")),
    }
}

// ============================================================================
// Output format helpers
// ============================================================================

/// Default display format: "Sat May 17 14:30:45 UTC 2025".
fn format_default(dt: &DateTime, epoch_sec: i64) -> String {
    format_datetime(dt, epoch_sec, "%a %b %e %H:%M:%S %Z %Y")
}

/// RFC 5322 format: "Sat, 17 May 2025 14:30:45 +0000".
fn format_rfc5322(dt: &DateTime, epoch_sec: i64) -> String {
    format_datetime(dt, epoch_sec, "%a, %d %b %Y %H:%M:%S %z")
}

/// RFC 3339 format with the specified precision.
fn format_rfc3339(dt: &DateTime, precision: &str) -> String {
    match precision {
        "date" => format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day),
        "seconds" => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}+00:00",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
        ),
        "ns" => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:09}+00:00",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, dt.nsec
        ),
        _ => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}+00:00",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
        ),
    }
}

/// ISO 8601 format with the specified precision.
fn format_iso8601(dt: &DateTime, precision: &str) -> String {
    match precision {
        "date" => format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day),
        "hours" => format!(
            "{:04}-{:02}-{:02}T{:02}+00:00",
            dt.year, dt.month, dt.day, dt.hour
        ),
        "minutes" => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}+00:00",
            dt.year, dt.month, dt.day, dt.hour, dt.minute
        ),
        "seconds" => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
        ),
        "ns" => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}+00:00",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, dt.nsec
        ),
        _ => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
        ),
    }
}

/// JSON output with all fields.
fn format_json(dt: &DateTime, epoch_sec: i64) -> String {
    let dow = day_of_week(dt.year, dt.month, dt.day);
    let doy = day_of_year(dt.year, dt.month, dt.day);

    let dow_usize = dow as usize;
    let wday_abbr = if dow_usize < WEEKDAY_ABBR.len() {
        WEEKDAY_ABBR[dow_usize]
    } else {
        "???"
    };
    let wday_full = if dow_usize < WEEKDAY_FULL.len() {
        WEEKDAY_FULL[dow_usize]
    } else {
        "???"
    };
    let m = dt.month as usize;
    let mon_abbr = if m >= 1 && m <= 12 {
        MONTH_ABBR[m]
    } else {
        "???"
    };
    let mon_full = if m >= 1 && m <= 12 {
        MONTH_FULL[m]
    } else {
        "???"
    };

    format!(
        "{{\
         \"epoch\":{epoch_sec},\
         \"year\":{},\
         \"month\":{},\
         \"day\":{},\
         \"hour\":{},\
         \"minute\":{},\
         \"second\":{},\
         \"nanosecond\":{},\
         \"day_of_year\":{doy},\
         \"day_of_week\":{},\
         \"day_of_week_name\":\"{wday_full}\",\
         \"day_of_week_abbr\":\"{wday_abbr}\",\
         \"month_name\":\"{mon_full}\",\
         \"month_abbr\":\"{mon_abbr}\",\
         \"timezone\":\"UTC\",\
         \"timezone_offset\":\"+0000\",\
         \"iso8601\":\"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00\"\
         }}",
        dt.year,
        dt.month,
        dt.day,
        dt.hour,
        dt.minute,
        dt.second,
        dt.nsec,
        dow + 1,
        dt.year,
        dt.month,
        dt.day,
        dt.hour,
        dt.minute,
        dt.second,
    )
}

// ============================================================================
// File modification time
// ============================================================================

/// Read the modification time of a file as an epoch timestamp.
///
/// Falls back to reading from a proc-style metadata file if std metadata
/// is not available (which may happen on our custom target).
fn file_mtime(path: &str) -> Result<(i64, i64), String> {
    let meta = fs::metadata(path).map_err(|e| format!("cannot stat '{path}': {e}"))?;

    // Try std::time::SystemTime.
    match meta.modified() {
        Ok(mtime) => {
            match mtime.duration_since(std::time::SystemTime::UNIX_EPOCH) {
                Ok(dur) => Ok((dur.as_secs() as i64, dur.subsec_nanos() as i64)),
                Err(e) => {
                    // Time before epoch.
                    let dur = e.duration();
                    Ok((-(dur.as_secs() as i64), dur.subsec_nanos() as i64))
                }
            }
        }
        Err(e) => Err(format!("cannot get modification time of '{path}': {e}")),
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line action.
enum Action {
    /// Display current time (with optional custom format).
    Display { format: Option<String> },
    /// Display given date string (with optional custom format).
    DisplayDate {
        date_str: String,
        format: Option<String>,
    },
    /// Set system time.
    SetTime { date_str: String },
    /// RFC 5322 format.
    Rfc5322,
    /// ISO 8601 format.
    Iso8601 { precision: String },
    /// RFC 3339 format.
    Rfc3339 { precision: String },
    /// Display file modification time (with optional custom format).
    Reference {
        path: String,
        format: Option<String>,
    },
    /// JSON output.
    Json,
    /// Show help.
    Help,
    /// Show version.
    Version,
}

/// Whether to force UTC display.
struct Options {
    utc: bool,
    action: Action,
}

const VERSION: &str = "0.1.0";

fn print_usage() {
    println!("date (OurOS) {VERSION}");
    println!();
    println!("Display or set the system date and time.");
    println!();
    println!("USAGE:");
    println!("  date [OPTION]... [+FORMAT]");
    println!();
    println!("OPTIONS:");
    println!("  -u, --utc, --universal    Display/set time in UTC");
    println!("  -d, --date <STRING>       Display given date instead of current time");
    println!("  -s, --set <STRING>        Set system date/time (requires root)");
    println!("  -R, --rfc-email           RFC 5322 format output");
    println!("  -I[FMT], --iso-8601[=FMT] ISO 8601 format (date,hours,minutes,seconds,ns)");
    println!("  --rfc-3339=FMT            RFC 3339 format (date,seconds,ns)");
    println!("  -r, --reference <FILE>    Display last modification time of FILE");
    println!("  --json                    JSON output with all fields");
    println!("  -h, --help                Show this help");
    println!("  -V, --version             Show version");
    println!();
    println!("FORMAT SPECIFIERS:");
    println!("  %Y  4-digit year          %m  month (01-12)      %d  day (01-31)");
    println!("  %H  hour (00-23)          %M  minute (00-59)     %S  second (00-59)");
    println!("  %I  12-hour (01-12)       %p  AM/PM              %P  am/pm");
    println!("  %a  abbreviated weekday   %A  full weekday       %u  weekday (1=Mon)");
    println!("  %b  abbreviated month     %B  full month         %j  day of year");
    println!("  %e  day space-padded      %k  hour space-padded  %y  2-digit year");
    println!("  %F  %Y-%m-%d              %T  %H:%M:%S           %R  %H:%M");
    println!("  %D  %m/%d/%y              %r  12-hour time       %c  locale date/time");
    println!("  %x  locale date           %X  locale time        %Z  timezone name");
    println!("  %z  timezone offset       %s  epoch seconds      %N  nanoseconds");
    println!("  %n  newline               %t  tab                %%  literal %");
    println!();
    println!("DATE STRING FORMATS:");
    println!("  YYYY-MM-DD [HH:MM[:SS]]   2025-05-17 14:30:00");
    println!("  YYYY-MM-DDTHH:MM[:SS]     2025-05-17T14:30:00");
    println!("  MM/DD/YYYY [HH:MM[:SS]]   05/17/2025 14:30:00");
    println!("  @SECONDS                  @1747498200 (Unix epoch)");
}

/// Parse command-line arguments into Options.
fn parse_args(args: &[String]) -> Options {
    let mut utc = false;
    let mut action: Option<Action> = None;
    let mut format_str: Option<String> = None;
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];

        // Format string starts with '+'.
        if let Some(rest) = arg.strip_prefix('+') {
            format_str = Some(rest.to_string());
            i += 1;
            continue;
        }

        match arg.as_str() {
            "-h" | "--help" => {
                return Options {
                    utc,
                    action: Action::Help,
                };
            }
            "-V" | "--version" => {
                return Options {
                    utc,
                    action: Action::Version,
                };
            }
            "-u" | "--utc" | "--universal" => {
                utc = true;
            }
            "-R" | "--rfc-email" => {
                action = Some(Action::Rfc5322);
            }
            "--json" => {
                action = Some(Action::Json);
            }
            "-d" | "--date" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("date: option '{arg}' requires an argument");
                    process::exit(1);
                }
                action = Some(Action::DisplayDate {
                    date_str: args[i].clone(),
                    format: None, // filled in later
                });
            }
            "-s" | "--set" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("date: option '{arg}' requires an argument");
                    process::exit(1);
                }
                action = Some(Action::SetTime {
                    date_str: args[i].clone(),
                });
            }
            "-r" | "--reference" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("date: option '{arg}' requires an argument");
                    process::exit(1);
                }
                action = Some(Action::Reference {
                    path: args[i].clone(),
                    format: None, // filled in later
                });
            }
            _ if arg.starts_with("-I") => {
                // -I, -Idate, -Ihours, -Iminutes, -Iseconds, -Ins
                let precision = arg.strip_prefix("-I").unwrap_or("date");
                let precision = if precision.is_empty() {
                    "date"
                } else {
                    precision
                };
                action = Some(Action::Iso8601 {
                    precision: precision.to_string(),
                });
            }
            _ if arg.starts_with("--iso-8601") => {
                let precision = if let Some(rest) = arg.strip_prefix("--iso-8601=") {
                    rest
                } else {
                    "date"
                };
                action = Some(Action::Iso8601 {
                    precision: precision.to_string(),
                });
            }
            _ if arg.starts_with("--rfc-3339=") => {
                let precision = arg
                    .strip_prefix("--rfc-3339=")
                    .unwrap_or("seconds");
                action = Some(Action::Rfc3339 {
                    precision: precision.to_string(),
                });
            }
            _ => {
                eprintln!("date: unrecognized option: '{arg}'");
                eprintln!("Try 'date --help' for more information.");
                process::exit(1);
            }
        }

        i += 1;
    }

    // Merge format string into actions that accept it.
    let action = match action {
        Some(Action::DisplayDate { date_str, .. }) => Action::DisplayDate {
            date_str,
            format: format_str,
        },
        Some(Action::Reference { path, .. }) => Action::Reference {
            path,
            format: format_str,
        },
        Some(other) => other,
        None => Action::Display {
            format: format_str,
        },
    };

    Options { utc, action }
}

// ============================================================================
// Time source
// ============================================================================

/// Get the current time as (epoch_sec, nsec).
///
/// Tries the kernel syscall first, falls back to std::time::SystemTime.
fn get_current_time() -> Result<(i64, i64), String> {
    // Try the direct syscall for OurOS.
    match clock_gettime(CLOCK_REALTIME) {
        Ok(time) => return Ok(time),
        Err(_) => {}
    }

    // Fallback: std::time::SystemTime (may work if the runtime is functional).
    match std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH) {
        Ok(dur) => Ok((dur.as_secs() as i64, dur.subsec_nanos() as i64)),
        Err(e) => Err(format!("cannot get current time: {e}")),
    }
}

// ============================================================================
// Main execution
// ============================================================================

fn run(opts: Options) -> Result<(), String> {
    // Currently all display is UTC. When timezone support is added, this flag
    // will select between local time and UTC. For now, acknowledge the field
    // so the compiler does not warn about an unread field.
    let _utc = opts.utc;

    match opts.action {
        Action::Help => {
            print_usage();
            Ok(())
        }
        Action::Version => {
            println!("date (OurOS) {VERSION}");
            Ok(())
        }
        Action::Display { format } => {
            let (sec, nsec) = get_current_time()?;
            let dt = epoch_to_datetime(sec, nsec);
            let output = match format {
                Some(fmt) => format_datetime(&dt, sec, &fmt),
                None => format_default(&dt, sec),
            };
            println!("{output}");
            Ok(())
        }
        Action::DisplayDate { date_str, format } => {
            let (sec, nsec) = parse_date_string(&date_str)?;
            let dt = epoch_to_datetime(sec, nsec);
            let output = match format {
                Some(fmt) => format_datetime(&dt, sec, &fmt),
                None => format_default(&dt, sec),
            };
            println!("{output}");
            Ok(())
        }
        Action::SetTime { date_str } => {
            let (sec, nsec) = parse_date_string(&date_str)?;
            clock_settime(CLOCK_REALTIME, sec, nsec)?;
            // Display the newly set time.
            let dt = epoch_to_datetime(sec, nsec);
            println!("{}", format_default(&dt, sec));
            Ok(())
        }
        Action::Rfc5322 => {
            let (sec, nsec) = get_current_time()?;
            let dt = epoch_to_datetime(sec, nsec);
            println!("{}", format_rfc5322(&dt, sec));
            Ok(())
        }
        Action::Iso8601 { precision } => {
            let (sec, nsec) = get_current_time()?;
            let dt = epoch_to_datetime(sec, nsec);
            println!("{}", format_iso8601(&dt, &precision));
            Ok(())
        }
        Action::Rfc3339 { precision } => {
            let (sec, nsec) = get_current_time()?;
            let dt = epoch_to_datetime(sec, nsec);
            println!("{}", format_rfc3339(&dt, &precision));
            Ok(())
        }
        Action::Reference { path, format } => {
            let (sec, nsec) = file_mtime(&path)?;
            let dt = epoch_to_datetime(sec, nsec);
            let output = match format {
                Some(fmt) => format_datetime(&dt, sec, &fmt),
                None => format_default(&dt, sec),
            };
            println!("{output}");
            Ok(())
        }
        Action::Json => {
            let (sec, nsec) = get_current_time()?;
            let dt = epoch_to_datetime(sec, nsec);
            println!("{}", format_json(&dt, sec));
            Ok(())
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let opts = parse_args(&args);

    if let Err(e) = run(opts) {
        eprintln!("date: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Leap year tests ---

    #[test]
    fn test_leap_year_common() {
        assert!(!is_leap_year(2023));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2100));
    }

    #[test]
    fn test_leap_year_leap() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(is_leap_year(1600));
        assert!(is_leap_year(2400));
    }

    // --- Days in month ---

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 1), 31);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 12), 31);
    }

    #[test]
    fn test_days_in_month_invalid() {
        assert_eq!(days_in_month(2023, 0), 0);
        assert_eq!(days_in_month(2023, 13), 0);
    }

    // --- Day of year ---

    #[test]
    fn test_day_of_year() {
        // Jan 1
        assert_eq!(day_of_year(2023, 1, 1), 1);
        // Dec 31 non-leap
        assert_eq!(day_of_year(2023, 12, 31), 365);
        // Dec 31 leap
        assert_eq!(day_of_year(2024, 12, 31), 366);
        // Mar 1 non-leap
        assert_eq!(day_of_year(2023, 3, 1), 60);
        // Mar 1 leap
        assert_eq!(day_of_year(2024, 3, 1), 61);
    }

    // --- Day of week ---

    #[test]
    fn test_day_of_week_known_dates() {
        // 2024-01-01 was Monday.
        assert_eq!(day_of_week(2024, 1, 1), 0); // Monday
        // 2024-02-29 was Thursday (leap day).
        assert_eq!(day_of_week(2024, 2, 29), 3); // Thursday
        // 1970-01-01 was Thursday.
        assert_eq!(day_of_week(1970, 1, 1), 3); // Thursday
        // 2000-01-01 was Saturday.
        assert_eq!(day_of_week(2000, 1, 1), 5); // Saturday
        // 2025-05-17 is Saturday.
        assert_eq!(day_of_week(2025, 5, 17), 5); // Saturday
    }

    // --- Epoch conversion round-trip ---

    #[test]
    fn test_epoch_to_datetime_unix_epoch() {
        let dt = epoch_to_datetime(0, 0);
        assert_eq!(dt.year, 1970);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 1);
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.second, 0);
    }

    #[test]
    fn test_epoch_to_datetime_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let dt = epoch_to_datetime(1_704_067_200, 0);
        assert_eq!(dt.year, 2024);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 1);
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.second, 0);
    }

    #[test]
    fn test_epoch_to_datetime_with_time() {
        // 2025-05-17 14:30:45 UTC = 1747491045
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 5);
        assert_eq!(dt.day, 17);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
        assert_eq!(dt.second, 45);
    }

    #[test]
    fn test_epoch_roundtrip() {
        let test_epochs: Vec<i64> = vec![
            0,
            1_704_067_200,
            1_747_491_045,
            86400,
            -86400,
            946_684_800, // 2000-01-01
        ];
        for epoch in test_epochs {
            let dt = epoch_to_datetime(epoch, 0);
            let back = datetime_to_epoch(&dt).unwrap();
            assert_eq!(epoch, back, "round-trip failed for epoch {epoch}");
        }
    }

    #[test]
    fn test_epoch_negative() {
        // 1969-12-31 23:59:59 = -1
        let dt = epoch_to_datetime(-1, 0);
        assert_eq!(dt.year, 1969);
        assert_eq!(dt.month, 12);
        assert_eq!(dt.day, 31);
        assert_eq!(dt.hour, 23);
        assert_eq!(dt.minute, 59);
        assert_eq!(dt.second, 59);
    }

    // --- datetime_to_epoch validation ---

    #[test]
    fn test_datetime_to_epoch_invalid_month() {
        let dt = DateTime {
            year: 2025,
            month: 13,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            nsec: 0,
        };
        assert!(datetime_to_epoch(&dt).is_err());
    }

    #[test]
    fn test_datetime_to_epoch_invalid_day() {
        let dt = DateTime {
            year: 2023,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
            nsec: 0,
        };
        assert!(datetime_to_epoch(&dt).is_err());
    }

    #[test]
    fn test_datetime_to_epoch_leap_day() {
        let dt = DateTime {
            year: 2024,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
            nsec: 0,
        };
        assert!(datetime_to_epoch(&dt).is_ok());
    }

    // --- Format specifiers ---

    #[test]
    fn test_format_year() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%Y"), "2025");
    }

    #[test]
    fn test_format_2digit_year() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%y"), "25");
    }

    #[test]
    fn test_format_month_day() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%m-%d"), "05-17");
    }

    #[test]
    fn test_format_hms() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(
            format_datetime(&dt, 1_747_491_045, "%H:%M:%S"),
            "14:30:45"
        );
    }

    #[test]
    fn test_format_12hour() {
        // 14:30 -> 02:30 PM
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%I:%M %p"), "02:30 PM");

        // 00:00 -> 12:00 AM
        let dt_midnight = epoch_to_datetime(1_747_440_000, 0);
        assert_eq!(
            format_datetime(&dt_midnight, 1_747_440_000, "%I:%M %p"),
            "12:00 AM"
        );
    }

    #[test]
    fn test_format_weekday() {
        // 2025-05-17 is Saturday.
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%a"), "Sat");
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%A"), "Saturday");
    }

    #[test]
    fn test_format_month_name() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%b"), "May");
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%B"), "May");
    }

    #[test]
    fn test_format_composite_f() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(
            format_datetime(&dt, 1_747_491_045, "%F"),
            "2025-05-17"
        );
    }

    #[test]
    fn test_format_composite_t() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(
            format_datetime(&dt, 1_747_491_045, "%T"),
            "14:30:45"
        );
    }

    #[test]
    fn test_format_epoch_seconds() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(
            format_datetime(&dt, 1_747_491_045, "%s"),
            "1747491045"
        );
    }

    #[test]
    fn test_format_nanoseconds() {
        let dt = epoch_to_datetime(0, 123_456_789);
        assert_eq!(format_datetime(&dt, 0, "%N"), "123456789");
    }

    #[test]
    fn test_format_escape() {
        let dt = epoch_to_datetime(0, 0);
        assert_eq!(format_datetime(&dt, 0, "%%"), "%");
        assert_eq!(format_datetime(&dt, 0, "%n"), "\n");
        assert_eq!(format_datetime(&dt, 0, "%t"), "\t");
    }

    #[test]
    fn test_format_space_padded() {
        // Day 3 -> " 3"
        let dt = DateTime {
            year: 2025,
            month: 1,
            day: 3,
            hour: 5,
            minute: 0,
            second: 0,
            nsec: 0,
        };
        let epoch = datetime_to_epoch(&dt).unwrap();
        assert_eq!(format_datetime(&dt, epoch, "%e"), " 3");
        assert_eq!(format_datetime(&dt, epoch, "%k"), " 5");
    }

    #[test]
    fn test_format_day_of_year() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        // May 17 = 31+28+31+30+17 = 137
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%j"), "137");
    }

    #[test]
    fn test_format_weekday_number() {
        // Saturday = 6 in ISO (u)
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%u"), "6");
    }

    // --- Date parsing ---

    #[test]
    fn test_parse_ymd() {
        let (sec, _) = parse_date_string("2025-05-17 14:30:45").unwrap();
        assert_eq!(sec, 1_747_491_045);
    }

    #[test]
    fn test_parse_ymd_date_only() {
        let (sec, _) = parse_date_string("2025-05-17").unwrap();
        let dt = epoch_to_datetime(sec, 0);
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 5);
        assert_eq!(dt.day, 17);
        assert_eq!(dt.hour, 0);
    }

    #[test]
    fn test_parse_iso_with_t() {
        let (sec, _) = parse_date_string("2025-05-17T14:30:45").unwrap();
        assert_eq!(sec, 1_747_491_045);
    }

    #[test]
    fn test_parse_epoch() {
        let (sec, _) = parse_date_string("@1747491045").unwrap();
        assert_eq!(sec, 1_747_491_045);
    }

    #[test]
    fn test_parse_mdy() {
        let (sec, _) = parse_date_string("05/17/2025 14:30:45").unwrap();
        assert_eq!(sec, 1_747_491_045);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_date_string("not-a-date").is_err());
        assert!(parse_date_string("2025-13-01").is_err());
        assert!(parse_date_string("2023-02-29").is_err());
    }

    // --- RFC / ISO format outputs ---

    #[test]
    fn test_rfc5322_format() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        let out = format_rfc5322(&dt, 1_747_491_045);
        assert_eq!(out, "Sat, 17 May 2025 14:30:45 +0000");
    }

    #[test]
    fn test_iso8601_date() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_iso8601(&dt, "date"), "2025-05-17");
    }

    #[test]
    fn test_iso8601_seconds() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(
            format_iso8601(&dt, "seconds"),
            "2025-05-17T14:30:45+00:00"
        );
    }

    #[test]
    fn test_rfc3339_seconds() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(
            format_rfc3339(&dt, "seconds"),
            "2025-05-17 14:30:45+00:00"
        );
    }

    #[test]
    fn test_rfc3339_ns() {
        let dt = epoch_to_datetime(1_747_491_045, 123_456_789);
        assert_eq!(
            format_rfc3339(&dt, "ns"),
            "2025-05-17 14:30:45.123456789+00:00"
        );
    }

    // --- JSON output ---

    #[test]
    fn test_json_output() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        let json = format_json(&dt, 1_747_491_045);
        assert!(json.contains("\"epoch\":1747491045"));
        assert!(json.contains("\"year\":2025"));
        assert!(json.contains("\"month\":5"));
        assert!(json.contains("\"day\":17"));
        assert!(json.contains("\"timezone\":\"UTC\""));
        assert!(json.contains("\"day_of_week_name\":\"Saturday\""));
    }

    // --- Default format ---

    #[test]
    fn test_default_format() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        let out = format_default(&dt, 1_747_491_045);
        assert_eq!(out, "Sat May 17 14:30:45 UTC 2025");
    }

    // --- Edge cases ---

    #[test]
    fn test_year_2000() {
        let dt = epoch_to_datetime(946_684_800, 0);
        assert_eq!(dt.year, 2000);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 1);
    }

    #[test]
    fn test_far_future() {
        // 2100-01-01 00:00:00 UTC = 4102444800
        let dt = epoch_to_datetime(4_102_444_800, 0);
        assert_eq!(dt.year, 2100);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 1);
    }

    #[test]
    fn test_12hour_edge_cases() {
        // Noon: 12:00 PM
        let dt = DateTime {
            year: 2025,
            month: 1,
            day: 1,
            hour: 12,
            minute: 0,
            second: 0,
            nsec: 0,
        };
        let epoch = datetime_to_epoch(&dt).unwrap();
        assert_eq!(format_datetime(&dt, epoch, "%I %p"), "12 PM");

        // 1 PM
        let dt_1pm = DateTime {
            year: 2025,
            month: 1,
            day: 1,
            hour: 13,
            minute: 0,
            second: 0,
            nsec: 0,
        };
        let epoch_1pm = datetime_to_epoch(&dt_1pm).unwrap();
        assert_eq!(format_datetime(&dt_1pm, epoch_1pm, "%I %p"), "01 PM");
    }

    #[test]
    fn test_format_r_12hour() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(
            format_datetime(&dt, 1_747_491_045, "%r"),
            "02:30:45 PM"
        );
    }

    #[test]
    fn test_format_d_us_date() {
        let dt = epoch_to_datetime(1_747_491_045, 0);
        assert_eq!(format_datetime(&dt, 1_747_491_045, "%D"), "05/17/25");
    }
}
