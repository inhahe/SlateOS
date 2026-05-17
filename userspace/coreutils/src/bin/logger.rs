//! logger — log messages to the system log.
//!
//! Usage: logger [-t TAG] [-p PRIORITY] [MESSAGE...]
//!   -t TAG       mark the message with the specified tag (default: "user")
//!   -p PRIORITY  specify priority in facility.level format (default: "user.notice")
//!
//! Writes a syslog-style text line to stdout (our OS uses text-based logs,
//! not binary syslog). If no MESSAGE arguments are given, reads from stdin.
//!
//! Output format:
//!   <priority> YYYY-MM-DDTHH:MM:SS TAG: MESSAGE

use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::process;
use std::time::SystemTime;

/// Map facility names to numeric codes (RFC 5424 compatible).
fn facility_code(name: &str) -> Option<u32> {
    match name {
        "kern" => Some(0),
        "user" => Some(1),
        "mail" => Some(2),
        "daemon" => Some(3),
        "auth" => Some(4),
        "syslog" => Some(5),
        "lpr" => Some(6),
        "news" => Some(7),
        "uucp" => Some(8),
        "cron" => Some(9),
        "local0" => Some(16),
        "local1" => Some(17),
        "local2" => Some(18),
        "local3" => Some(19),
        "local4" => Some(20),
        "local5" => Some(21),
        "local6" => Some(22),
        "local7" => Some(23),
        _ => None,
    }
}

/// Map severity names to numeric codes (RFC 5424 compatible).
fn severity_code(name: &str) -> Option<u32> {
    match name {
        "emerg" | "panic" => Some(0),
        "alert" => Some(1),
        "crit" => Some(2),
        "err" | "error" => Some(3),
        "warning" | "warn" => Some(4),
        "notice" => Some(5),
        "info" => Some(6),
        "debug" => Some(7),
        _ => None,
    }
}

/// Parse a "facility.severity" string into the PRI value.
fn parse_priority(s: &str) -> Option<u32> {
    if let Some((fac_str, sev_str)) = s.split_once('.') {
        let fac = facility_code(fac_str)?;
        let sev = severity_code(sev_str)?;
        Some(fac * 8 + sev)
    } else {
        // Could be severity alone (assume user facility).
        let sev = severity_code(s)?;
        Some(1 * 8 + sev)
    }
}

/// Format a timestamp from SystemTime as ISO 8601 (approximate, no TZ library).
fn format_timestamp() -> String {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            // Simple date/time computation (no leap seconds, approximate).
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let hours = time_of_day / 3600;
            let minutes = (time_of_day % 3600) / 60;
            let seconds = time_of_day % 60;

            // Calculate year/month/day from days since epoch (1970-01-01).
            let (year, month, day) = days_to_date(days);

            format!(
                "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}"
            )
        }
        Err(_) => "1970-01-01T00:00:00".to_string(),
    }
}

/// Convert days since Unix epoch to (year, month, day).
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
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut tag = "user".to_string();
    let mut priority_str = "user.notice".to_string();
    let mut message_parts: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                if i < args.len() {
                    tag = args[i].clone();
                } else {
                    eprintln!("logger: option -t requires an argument");
                    process::exit(1);
                }
            }
            "-p" => {
                i += 1;
                if i < args.len() {
                    priority_str = args[i].clone();
                } else {
                    eprintln!("logger: option -p requires an argument");
                    process::exit(1);
                }
            }
            _ => {
                message_parts.push(args[i].clone());
            }
        }
        i += 1;
    }

    let pri = match parse_priority(&priority_str) {
        Some(p) => p,
        None => {
            eprintln!("logger: unknown priority: {priority_str}");
            process::exit(1);
        }
    };

    let timestamp = format_timestamp();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if message_parts.is_empty() {
        // Read messages from stdin, one per line.
        let reader = BufReader::new(io::stdin());
        for line in reader.lines() {
            match line {
                Ok(msg) => {
                    let _ = writeln!(out, "<{pri}> {timestamp} {tag}: {msg}");
                }
                Err(e) => {
                    eprintln!("logger: {e}");
                    process::exit(1);
                }
            }
        }
    } else {
        let msg = message_parts.join(" ");
        let _ = writeln!(out, "<{pri}> {timestamp} {tag}: {msg}");
    }
}
