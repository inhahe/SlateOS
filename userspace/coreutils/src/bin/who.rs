//! who — show who is logged in.
//!
//! Usage: who [-a]
//!   -a  show all entries (including system boot, run level, etc.)
//!
//! Attempts to read login records from /var/run/utmp. If that file does not
//! exist (common on early-stage systems), falls back to showing the current
//! user from environment variables and the controlling terminal.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::SystemTime;

/// Attempt to determine the current TTY name from /proc/self/fd/0
/// or fall back to "console".
fn current_tty() -> String {
    // Try reading the symlink target of stdin.
    if let Ok(target) = fs::read_link("/proc/self/fd/0")
        && let Some(s) = target.to_str() {
            // Strip leading /dev/ if present.
            if let Some(stripped) = s.strip_prefix("/dev/") {
                return stripped.to_string();
            }
            return s.to_string();
        }

    // Try the TTY environment variable.
    if let Ok(tty) = env::var("TTY") {
        return tty;
    }

    "console".to_string()
}

/// Format a timestamp for display (YYYY-MM-DD HH:MM).
fn format_time() -> String {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let hours = time_of_day / 3600;
            let minutes = (time_of_day % 3600) / 60;

            let (year, month, day) = days_to_date(days);

            format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
        }
        Err(_) => "????-??-?? ??:??".to_string(),
    }
}

fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = i as u64 + 1;
            break;
        }
        days -= md;
    }
    if month == 0 {
        month = 12;
    }

    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

/// Parse binary utmp records. Each record is 384 bytes on Linux.
/// We attempt to read them but gracefully fall back if the format
/// is different or the file is absent.
fn try_read_utmp(path: &Path) -> Option<Vec<(String, String, String)>> {
    let data = fs::read(path).ok()?;

    // Minimal utmp record parsing: each record is 384 bytes.
    // Offsets (Linux utmp):
    //   0..4:   ut_type (i32)
    //   4..8:   ut_pid (i32)
    //   8..40:  ut_line (32 bytes, NUL-terminated)
    //   40..44: ut_id (4 bytes)
    //   44..76: ut_user (32 bytes, NUL-terminated)
    //   76..332: ut_host (256 bytes, NUL-terminated)
    const RECORD_SIZE: usize = 384;
    const USER_PROCESS: i32 = 7;

    let mut entries = Vec::new();

    let mut offset = 0;
    while offset + RECORD_SIZE <= data.len() {
        let record = &data[offset..offset + RECORD_SIZE];
        let ut_type = i32::from_le_bytes([record[0], record[1], record[2], record[3]]);

        if ut_type == USER_PROCESS {
            let user = extract_string(&record[44..76]);
            let line = extract_string(&record[8..40]);
            let host = extract_string(&record[76..332]);
            if !user.is_empty() {
                entries.push((user, line, host));
            }
        }
        offset += RECORD_SIZE;
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

fn extract_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn main() {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Try reading utmp.
    let utmp_paths = ["/var/run/utmp", "/run/utmp", "/var/log/utmp"];
    for path in &utmp_paths {
        if let Some(entries) = try_read_utmp(Path::new(path)) {
            let time_str = format_time();
            for (user, line, host) in &entries {
                if host.is_empty() {
                    let _ = writeln!(out, "{user:<12} {line:<12} {time_str}");
                } else {
                    let _ = writeln!(out, "{user:<12} {line:<12} {time_str} ({host})");
                }
            }
            return;
        }
    }

    // Fallback: show current user from environment.
    let user = env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "?".to_string());
    let tty = current_tty();
    let time_str = format_time();

    let _ = writeln!(out, "{user:<12} {tty:<12} {time_str}");
}
