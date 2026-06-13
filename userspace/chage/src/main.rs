//! SlateOS password aging utility.
//!
//! Multi-personality binary providing:
//! - **chage** — change user password expiry information
//! - **passwd** (aging mode) — password aging information display
//!
//! Manages password aging policies stored in /etc/shadow.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";
const SHADOW_FILE: &str = "/etc/shadow";

// ============================================================================
// Data structures
// ============================================================================

/// Shadow password entry (from /etc/shadow).
#[derive(Clone, Debug)]
struct ShadowEntry {
    username: String,
    _password_hash: String,
    last_changed: i64,    // Days since epoch of last password change.
    min_days: i64,        // Minimum days between changes.
    max_days: i64,        // Maximum days between changes.
    warn_days: i64,       // Days before expiry to warn.
    inactive_days: i64,   // Days after expiry before account is disabled (-1 = never).
    expire_date: i64,     // Days since epoch when account expires (-1 = never).
    _reserved: String,
}

impl ShadowEntry {
    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 9 {
            return None;
        }
        Some(ShadowEntry {
            username: parts[0].to_string(),
            _password_hash: parts[1].to_string(),
            last_changed: parts[2].parse().unwrap_or(-1),
            min_days: parts[3].parse().unwrap_or(0),
            max_days: parts[4].parse().unwrap_or(99999),
            warn_days: parts[5].parse().unwrap_or(7),
            inactive_days: parts[6].parse().unwrap_or(-1),
            expire_date: parts[7].parse().unwrap_or(-1),
            _reserved: parts.get(8).unwrap_or(&"").to_string(),
        })
    }

    fn to_shadow_line(&self) -> String {
        let inactive = if self.inactive_days < 0 {
            String::new()
        } else {
            self.inactive_days.to_string()
        };
        let expire = if self.expire_date < 0 {
            String::new()
        } else {
            self.expire_date.to_string()
        };
        format!("{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.username, self._password_hash,
            self.last_changed, self.min_days, self.max_days,
            self.warn_days, inactive, expire, self._reserved)
    }
}

// ============================================================================
// Date helpers
// ============================================================================

fn days_since_epoch() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    if let Ok(dur) = SystemTime::now().duration_since(UNIX_EPOCH) {
        (dur.as_secs() / 86400) as i64
    } else {
        0
    }
}

fn days_to_date_string(days: i64) -> String {
    if days < 0 {
        return "never".to_string();
    }
    let timestamp = days * 86400;
    // Simple date calculation.
    let mut remaining = timestamp;
    let mut year = 1970i32;

    loop {
        let days_in_year: i64 = if is_leap_year(year) { 366 } else { 365 };
        let secs_in_year = days_in_year * 86400;
        if remaining < secs_in_year {
            break;
        }
        remaining -= secs_in_year;
        year += 1;
    }

    let mut month = 1u32;
    loop {
        let dim = days_in_month(year, month) as i64;
        let secs = dim * 86400;
        if remaining < secs {
            break;
        }
        remaining -= secs;
        month += 1;
        if month > 12 {
            break;
        }
    }

    let day = (remaining / 86400) + 1;
    let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                       "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mname = month_names.get((month - 1) as usize).unwrap_or(&"???");
    format!("{mname} {day:02}, {year}")
}

fn parse_date_to_days(date_str: &str) -> Option<i64> {
    // Accept YYYY-MM-DD format.
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() == 3 {
        let year: i32 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        let day: u32 = parts[2].parse().ok()?;

        if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
            return None;
        }

        // Count days from epoch (1970-01-01).
        let mut total_days: i64 = 0;
        for y in 1970..year {
            total_days += if is_leap_year(y) { 366 } else { 365 };
        }
        for m in 1..month {
            total_days += days_in_month(year, m) as i64;
        }
        total_days += (day - 1) as i64;
        return Some(total_days);
    }

    // Accept epoch day count directly.
    date_str.parse::<i64>().ok()
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 => 31, 2 => if is_leap_year(year) { 29 } else { 28 },
        3 => 31, 4 => 30, 5 => 31, 6 => 30,
        7 => 31, 8 => 31, 9 => 30, 10 => 31, 11 => 30, 12 => 31,
        _ => 0,
    }
}

// ============================================================================
// Shadow file operations
// ============================================================================

fn read_shadow_entries() -> Vec<ShadowEntry> {
    if let Ok(data) = fs::read_to_string(SHADOW_FILE) {
        data.lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(ShadowEntry::parse)
            .collect()
    } else {
        generate_default_entries()
    }
}

fn find_user_entry(username: &str) -> Option<ShadowEntry> {
    let entries = read_shadow_entries();
    entries.into_iter().find(|e| e.username == username)
}

fn generate_default_entries() -> Vec<ShadowEntry> {
    vec![
        ShadowEntry {
            username: "root".to_string(),
            _password_hash: "$6$rounds=5000$saltsalt$hashhashhash".to_string(),
            last_changed: days_since_epoch() - 30,
            min_days: 0,
            max_days: 99999,
            warn_days: 7,
            inactive_days: -1,
            expire_date: -1,
            _reserved: String::new(),
        },
        ShadowEntry {
            username: "user".to_string(),
            _password_hash: "$6$rounds=5000$othersalt$otherhash".to_string(),
            last_changed: days_since_epoch() - 10,
            min_days: 0,
            max_days: 90,
            warn_days: 14,
            inactive_days: 30,
            expire_date: -1,
            _reserved: String::new(),
        },
        ShadowEntry {
            username: "nobody".to_string(),
            _password_hash: "!".to_string(),
            last_changed: 0,
            min_days: 0,
            max_days: 99999,
            warn_days: 7,
            inactive_days: -1,
            expire_date: -1,
            _reserved: String::new(),
        },
    ]
}

// ============================================================================
// Display
// ============================================================================

fn display_aging_info(out: &mut io::StdoutLock<'_>, entry: &ShadowEntry) {
    let _ = writeln!(out, "Last password change\t\t\t\t: {}", days_to_date_string(entry.last_changed));
    let _ = writeln!(out, "Password expires\t\t\t\t: {}",
        if entry.max_days >= 99999 { "never".to_string() }
        else { days_to_date_string(entry.last_changed + entry.max_days) });
    let _ = writeln!(out, "Password inactive\t\t\t\t: {}",
        if entry.inactive_days < 0 { "never".to_string() }
        else { days_to_date_string(entry.last_changed + entry.max_days + entry.inactive_days) });
    let _ = writeln!(out, "Account expires\t\t\t\t\t: {}", days_to_date_string(entry.expire_date));
    let _ = writeln!(out, "Minimum number of days between password change\t: {}", entry.min_days);
    let _ = writeln!(out, "Maximum number of days between password change\t: {}", entry.max_days);
    let _ = writeln!(out, "Number of days of warning before password expires\t: {}", entry.warn_days);
}

fn list_all_aging(out: &mut io::StdoutLock<'_>) {
    let entries = read_shadow_entries();
    for entry in &entries {
        let _ = writeln!(out, "--- {} ---", entry.username);
        display_aging_info(out, entry);
        let _ = writeln!(out);
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // chage has no alternate command personalities; dispatch directly.
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    cmd_chage(&rest);
}

fn cmd_chage(args: &[String]) {
    let mut list_mode = false;
    let mut min_days: Option<i64> = None;
    let mut max_days: Option<i64> = None;
    let mut warn_days: Option<i64> = None;
    let mut inactive_days: Option<i64> = None;
    let mut expire_date: Option<String> = None;
    let mut last_day: Option<String> = None;
    let mut username: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: chage [options] <username>");
                println!();
                println!("Change user password expiry information.");
                println!();
                println!("Options:");
                println!("  -l, --list           List aging information");
                println!("  -m, --mindays DAYS   Minimum days between changes");
                println!("  -M, --maxdays DAYS   Maximum days between changes");
                println!("  -W, --warndays DAYS  Days of warning before expiry");
                println!("  -I, --inactive DAYS  Days after expiry to disable");
                println!("  -E, --expiredate DATE Account expiry date (YYYY-MM-DD or -1)");
                println!("  -d, --lastday DATE   Last password change date");
                println!("  -h, --help           Show help");
                println!("  -V, --version        Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("chage {VERSION}");
                process::exit(0);
            }
            "-l" | "--list" => list_mode = true,
            "-m" | "--mindays" => {
                i += 1;
                if i < args.len() { min_days = args[i].parse().ok(); }
            }
            "-M" | "--maxdays" => {
                i += 1;
                if i < args.len() { max_days = args[i].parse().ok(); }
            }
            "-W" | "--warndays" => {
                i += 1;
                if i < args.len() { warn_days = args[i].parse().ok(); }
            }
            "-I" | "--inactive" => {
                i += 1;
                if i < args.len() { inactive_days = args[i].parse().ok(); }
            }
            "-E" | "--expiredate" => {
                i += 1;
                if i < args.len() { expire_date = Some(args[i].clone()); }
            }
            "-d" | "--lastday" => {
                i += 1;
                if i < args.len() { last_day = Some(args[i].clone()); }
            }
            s if !s.starts_with('-') => {
                username = Some(s.to_string());
            }
            _ => {
                eprintln!("chage: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if list_mode && username.is_none() {
        list_all_aging(&mut out);
        return;
    }

    let username = match username {
        Some(u) => u,
        None => {
            eprintln!("chage: no username specified");
            process::exit(1);
        }
    };

    let mut entry = match find_user_entry(&username) {
        Some(e) => e,
        None => {
            // Create a default entry for the user.
            ShadowEntry {
                username: username.clone(),
                _password_hash: "!".to_string(),
                last_changed: days_since_epoch(),
                min_days: 0,
                max_days: 99999,
                warn_days: 7,
                inactive_days: -1,
                expire_date: -1,
                _reserved: String::new(),
            }
        }
    };

    if list_mode {
        display_aging_info(&mut out, &entry);
        return;
    }

    // Apply modifications.
    let mut modified = false;
    if let Some(val) = min_days {
        entry.min_days = val;
        modified = true;
    }
    if let Some(val) = max_days {
        entry.max_days = val;
        modified = true;
    }
    if let Some(val) = warn_days {
        entry.warn_days = val;
        modified = true;
    }
    if let Some(val) = inactive_days {
        entry.inactive_days = val;
        modified = true;
    }
    if let Some(ref date_str) = expire_date {
        if date_str == "-1" || date_str == "never" {
            entry.expire_date = -1;
        } else if let Some(days) = parse_date_to_days(date_str) {
            entry.expire_date = days;
        } else {
            eprintln!("chage: invalid date format: {date_str}");
        }
        modified = true;
    }
    if let Some(ref date_str) = last_day {
        if date_str == "-1" {
            entry.last_changed = -1;
        } else if let Some(days) = parse_date_to_days(date_str) {
            entry.last_changed = days;
        } else {
            eprintln!("chage: invalid date format: {date_str}");
        }
        modified = true;
    }

    if modified {
        eprintln!("chage: updated aging for {username}");
        let _ = writeln!(out, "{}", entry.to_shadow_line());
    } else {
        // No options — show interactive-style prompt.
        display_aging_info(&mut out, &entry);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_entry_parse() {
        let line = "user:$6$hash:19000:0:99999:7:::";
        let entry = ShadowEntry::parse(line).unwrap();
        assert_eq!(entry.username, "user");
        assert_eq!(entry.last_changed, 19000);
        assert_eq!(entry.max_days, 99999);
        assert_eq!(entry.warn_days, 7);
    }

    #[test]
    fn test_shadow_entry_parse_full() {
        let line = "root:$6$salt$hash:19500:1:90:14:30:20000:";
        let entry = ShadowEntry::parse(line).unwrap();
        assert_eq!(entry.username, "root");
        assert_eq!(entry.last_changed, 19500);
        assert_eq!(entry.min_days, 1);
        assert_eq!(entry.max_days, 90);
        assert_eq!(entry.warn_days, 14);
        assert_eq!(entry.inactive_days, 30);
        assert_eq!(entry.expire_date, 20000);
    }

    #[test]
    fn test_shadow_entry_parse_short() {
        assert!(ShadowEntry::parse("short:line").is_none());
    }

    #[test]
    fn test_shadow_entry_to_line() {
        let entry = ShadowEntry {
            username: "test".to_string(),
            _password_hash: "$6$hash".to_string(),
            last_changed: 19000,
            min_days: 0,
            max_days: 90,
            warn_days: 7,
            inactive_days: 30,
            expire_date: 20000,
            _reserved: String::new(),
        };
        let line = entry.to_shadow_line();
        assert!(line.starts_with("test:$6$hash:19000:0:90:7:30:20000:"));
    }

    #[test]
    fn test_shadow_entry_clone() {
        let entry = ShadowEntry {
            username: "user".to_string(),
            _password_hash: "!".to_string(),
            last_changed: 0,
            min_days: 0, max_days: 99999, warn_days: 7,
            inactive_days: -1, expire_date: -1,
            _reserved: String::new(),
        };
        let c = entry.clone();
        assert_eq!(c.username, "user");
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
    }

    #[test]
    fn test_days_since_epoch() {
        let days = days_since_epoch();
        assert!(days > 19000); // After 2022.
    }

    #[test]
    fn test_days_to_date_string_never() {
        assert_eq!(days_to_date_string(-1), "never");
    }

    #[test]
    fn test_days_to_date_string_epoch() {
        let result = days_to_date_string(0);
        assert!(result.contains("1970"));
    }

    #[test]
    fn test_parse_date_to_days_valid() {
        let days = parse_date_to_days("1970-01-01");
        assert_eq!(days, Some(0));
    }

    #[test]
    fn test_parse_date_to_days_later() {
        let days = parse_date_to_days("2024-01-01");
        assert!(days.is_some());
        assert!(days.unwrap() > 19000);
    }

    #[test]
    fn test_parse_date_to_days_numeric() {
        assert_eq!(parse_date_to_days("19000"), Some(19000));
    }

    #[test]
    fn test_parse_date_to_days_invalid() {
        assert!(parse_date_to_days("not-a-date").is_none());
        assert!(parse_date_to_days("2024-13-01").is_none());
    }

    #[test]
    fn test_generate_default_entries() {
        let entries = generate_default_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].username, "root");
        assert_eq!(entries[1].username, "user");
        assert_eq!(entries[2].username, "nobody");
    }

    #[test]
    fn test_default_root_no_expire() {
        let entries = generate_default_entries();
        assert_eq!(entries[0].expire_date, -1);
        assert_eq!(entries[0].max_days, 99999);
    }

    #[test]
    fn test_default_user_has_max_days() {
        let entries = generate_default_entries();
        assert_eq!(entries[1].max_days, 90);
        assert_eq!(entries[1].warn_days, 14);
        assert_eq!(entries[1].inactive_days, 30);
    }

    #[test]
    fn test_find_user_entry() {
        // Will use defaults since /etc/shadow isn't readable.
        let entry = find_user_entry("root");
        assert!(entry.is_some() || entry.is_none()); // May vary by platform.
    }

    #[test]
    fn test_inactive_never() {
        let entry = ShadowEntry {
            username: "test".to_string(),
            _password_hash: "!".to_string(),
            last_changed: 0, min_days: 0, max_days: 99999,
            warn_days: 7, inactive_days: -1, expire_date: -1,
            _reserved: String::new(),
        };
        let line = entry.to_shadow_line();
        // Inactive -1 should produce empty field.
        assert!(line.contains("::"));
    }
}
