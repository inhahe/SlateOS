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

/// Parse a buffer of binary utmp records.  Each record is 384 bytes on Linux,
/// and we extract `(user, line, host)` for every USER_PROCESS entry whose
/// `user` field is non-empty.  Returns an empty Vec if no usable records are
/// found.  Pure function; suitable for unit tests using synthetic byte
/// buffers.
fn parse_utmp_records(data: &[u8]) -> Vec<(String, String, String)> {
    const RECORD_SIZE: usize = 384;
    const USER_PROCESS: i32 = 7;

    let mut entries = Vec::new();
    let mut offset: usize = 0;
    while offset.saturating_add(RECORD_SIZE) <= data.len() {
        let Some(record) = data.get(offset..offset.saturating_add(RECORD_SIZE)) else {
            break;
        };
        let Some(type_bytes) = record.get(0..4) else { break };
        let ut_type = i32::from_le_bytes([type_bytes[0], type_bytes[1], type_bytes[2], type_bytes[3]]);

        if ut_type == USER_PROCESS {
            let user = record.get(44..76).map(extract_string).unwrap_or_default();
            let line = record.get(8..40).map(extract_string).unwrap_or_default();
            let host = record.get(76..332).map(extract_string).unwrap_or_default();
            if !user.is_empty() {
                entries.push((user, line, host));
            }
        }
        offset = offset.saturating_add(RECORD_SIZE);
    }
    entries
}

/// Read a utmp file and parse its records.  Returns `None` if the file is
/// missing/unreadable or if it contained no usable records.
fn try_read_utmp(path: &Path) -> Option<Vec<(String, String, String)>> {
    let data = fs::read(path).ok()?;
    let entries = parse_utmp_records(&data);
    if entries.is_empty() { None } else { Some(entries) }
}

/// Read a NUL-terminated (or full-buffer) UTF-8 string from a byte slice.
fn extract_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(&[])).to_string()
}

/// Format one `who` output line.  Empty `host` is omitted (no trailing
/// "()"); otherwise it appears at the end in parentheses.
fn format_who_line(user: &str, line: &str, time_str: &str, host: &str) -> String {
    if host.is_empty() {
        format!("{user:<12} {line:<12} {time_str}")
    } else {
        format!("{user:<12} {line:<12} {time_str} ({host})")
    }
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
                let _ = writeln!(out, "{}", format_who_line(user, line, &time_str, host));
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

    let _ = writeln!(out, "{}", format_who_line(&user, &tty, &time_str, ""));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- extract_string ----------------

    #[test]
    fn extract_empty_slice() {
        assert_eq!(extract_string(&[]), "");
    }

    #[test]
    fn extract_no_null_takes_full() {
        assert_eq!(extract_string(b"hello"), "hello");
    }

    #[test]
    fn extract_truncates_at_null() {
        let buf = b"alice\0\0\0\0";
        assert_eq!(extract_string(buf), "alice");
    }

    #[test]
    fn extract_invalid_utf8_lossy() {
        // 0xff is invalid UTF-8; from_utf8_lossy keeps the replacement char.
        let buf = [0xffu8, b'X', 0];
        let result = extract_string(&buf);
        assert!(result.ends_with('X'));
        assert!(result.contains('\u{FFFD}'));
    }

    // ---------------- is_leap ----------------

    #[test]
    fn leap_basics() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
        assert!(!is_leap(2100));
    }

    // ---------------- days_to_date ----------------

    #[test]
    fn date_epoch() {
        assert_eq!(days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn date_one_year() {
        assert_eq!(days_to_date(365), (1971, 1, 1));
    }

    #[test]
    fn date_leap_day_2000() {
        assert_eq!(days_to_date(11016), (2000, 2, 29));
    }

    // ---------------- format_who_line ----------------

    #[test]
    fn format_no_host() {
        let line = format_who_line("alice", "pts/0", "2024-01-01 12:34", "");
        assert_eq!(line, "alice        pts/0        2024-01-01 12:34");
    }

    #[test]
    fn format_with_host() {
        let line = format_who_line("bob", "pts/1", "2024-01-01 12:34", "10.0.0.1");
        assert_eq!(line, "bob          pts/1        2024-01-01 12:34 (10.0.0.1)");
    }

    #[test]
    fn format_long_user_no_padding() {
        // Padding is `<12` so users longer than 12 chars aren't truncated.
        let line = format_who_line("verylongusername", "tty1", "2024-01-01 00:00", "");
        assert!(line.starts_with("verylongusername"));
    }

    // ---------------- parse_utmp_records ----------------

    /// Build a synthetic 384-byte utmp record.
    fn build_record(ut_type: i32, user: &str, line: &str, host: &str) -> Vec<u8> {
        let mut rec = vec![0u8; 384];
        rec[0..4].copy_from_slice(&ut_type.to_le_bytes());
        // ut_line at 8..40 (32 bytes).
        let line_bytes = line.as_bytes();
        let n = line_bytes.len().min(31);
        rec[8..8 + n].copy_from_slice(&line_bytes[..n]);
        // ut_user at 44..76 (32 bytes).
        let user_bytes = user.as_bytes();
        let n = user_bytes.len().min(31);
        rec[44..44 + n].copy_from_slice(&user_bytes[..n]);
        // ut_host at 76..332 (256 bytes).
        let host_bytes = host.as_bytes();
        let n = host_bytes.len().min(255);
        rec[76..76 + n].copy_from_slice(&host_bytes[..n]);
        rec
    }

    #[test]
    fn parse_empty_data() {
        assert!(parse_utmp_records(&[]).is_empty());
    }

    #[test]
    fn parse_one_user_process() {
        let rec = build_record(7, "alice", "pts/0", "localhost");
        let parsed = parse_utmp_records(&rec);
        assert_eq!(parsed, vec![("alice".to_string(), "pts/0".to_string(), "localhost".to_string())]);
    }

    #[test]
    fn parse_skips_non_user_process() {
        // type 1 = RUN_LVL, type 2 = BOOT_TIME — neither is USER_PROCESS.
        let mut data = build_record(1, "runlevel", "~", "");
        data.extend_from_slice(&build_record(2, "boot", "~", ""));
        assert!(parse_utmp_records(&data).is_empty());
    }

    #[test]
    fn parse_skips_user_process_with_empty_user() {
        let rec = build_record(7, "", "pts/0", "host");
        assert!(parse_utmp_records(&rec).is_empty());
    }

    #[test]
    fn parse_multiple_records() {
        let mut data = build_record(7, "alice", "pts/0", "");
        data.extend_from_slice(&build_record(1, "runlevel", "~", ""));
        data.extend_from_slice(&build_record(7, "bob", "pts/1", "10.0.0.1"));
        let parsed = parse_utmp_records(&data);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "alice");
        assert_eq!(parsed[1].0, "bob");
        assert_eq!(parsed[1].2, "10.0.0.1");
    }

    #[test]
    fn parse_ignores_trailing_partial_record() {
        let mut data = build_record(7, "alice", "pts/0", "");
        data.extend_from_slice(&[0u8; 200]); // < 384 bytes
        let parsed = parse_utmp_records(&data);
        assert_eq!(parsed.len(), 1);
    }
}
