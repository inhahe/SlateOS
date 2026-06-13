//! SlateOS Logged-In Users Display (`who` / `w`)
//!
//! Shows who is currently logged in to the system. Reads session data from
//! multiple sources in priority order:
//!
//! 1. `/var/run/utmp` -- binary utmp records (Linux format for compatibility)
//! 2. `/run/sessions/` -- SlateOS native session directory
//! 3. `/tmp/.users/` -- fallback: files named by username
//!
//! When invoked as `w` (detected via `argv[0]`), displays an extended format
//! including a header line with uptime, user count, and load averages, plus
//! per-user idle time, CPU usage, and current process information.
//!
//! # Usage
//!
//! ```text
//! who                    Show logged-in users
//! who am i / whoami      Show current user only
//! who -a / --all         Show all records (including dead, login, boot)
//! who -b / --boot        Show last boot time
//! who -d / --dead        Show dead processes
//! who -H / --heading     Show column headings
//! who -l / --login       Show system login processes
//! who -q / --count       Show user count and names only
//! who -s / --short       Short format (default)
//! who -T / --mesg        Show message status (+/-/?) for terminal
//! who -u / --users       Show idle time
//! who --json             JSON output
//! w                      Extended format with uptime, idle, CPU, command
//! ```

use std::env;
use std::fs;
use std::process;
use std::time::SystemTime;

// ============================================================================
// Constants
// ============================================================================

/// Standard Linux utmp record size for x86_64.
const UTMP_RECORD_SIZE: usize = 384;

/// Utmp field offsets and sizes (Linux x86_64 layout).
const UT_TYPE_OFFSET: usize = 0;
const UT_PID_OFFSET: usize = 4;
const UT_LINE_OFFSET: usize = 8;
const UT_LINE_SIZE: usize = 32;
const UT_ID_OFFSET: usize = 40;
const UT_ID_SIZE: usize = 4;
const UT_USER_OFFSET: usize = 44;
const UT_USER_SIZE: usize = 32;
const UT_HOST_OFFSET: usize = 76;
const UT_HOST_SIZE: usize = 256;
const UT_TV_SEC_OFFSET: usize = 340;

/// Utmp record type constants.
const EMPTY: i32 = 0;
const RUN_LVL: i32 = 1;
const BOOT_TIME: i32 = 2;
const NEW_TIME: i32 = 3;
const OLD_TIME: i32 = 4;
const INIT_PROCESS: i32 = 5;
const LOGIN_PROCESS: i32 = 6;
const USER_PROCESS: i32 = 7;
const DEAD_PROCESS: i32 = 8;

// ============================================================================
// Data structures
// ============================================================================

/// A single session/login record, normalised from any data source.
struct SessionRecord {
    /// Record type (maps to utmp ut_type values).
    record_type: i32,
    /// Username.
    user: String,
    /// Terminal line (e.g. "tty1", "pts/0").
    tty: String,
    /// Remote host or empty.
    host: String,
    /// Process ID associated with this session.
    pid: i32,
    /// Login time as Unix epoch seconds.
    login_time: u64,
    /// utmp id field (4 bytes, for display in --all mode).
    id: String,
}

/// What the user asked us to do.
#[derive(Debug)]
struct Options {
    /// Show all record types, not just USER_PROCESS.
    show_all: bool,
    /// Show last boot time only.
    show_boot: bool,
    /// Show dead processes.
    show_dead: bool,
    /// Print column headings.
    show_heading: bool,
    /// Show system login processes.
    show_login: bool,
    /// Quick mode: names and count only.
    show_count: bool,
    /// Show message status (+/-/?).
    show_mesg: bool,
    /// Show idle time.
    show_idle: bool,
    /// JSON output.
    json: bool,
    /// Running as `w` (extended format).
    w_mode: bool,
    /// Show only the current user ("who am i" / "whoami").
    am_i: bool,
}

impl Options {
    fn new() -> Self {
        Self {
            show_all: false,
            show_boot: false,
            show_dead: false,
            show_heading: false,
            show_login: false,
            show_count: false,
            show_mesg: false,
            show_idle: false,
            json: false,
            w_mode: false,
            am_i: false,
        }
    }
}

// ============================================================================
// Time helpers (no external crate)
// ============================================================================

/// Get current time as seconds since the Unix epoch.
fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

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

/// Decompose epoch seconds into (year, month, day, hour, minute, second).
fn epoch_to_parts(epoch_secs: u64) -> (i64, u32, u32, u64, u64, u64) {
    let total_days = (epoch_secs / 86400) as i64;
    let day_seconds = epoch_secs % 86400;

    let hours = day_seconds / 3600;
    let minutes = (day_seconds % 3600) / 60;
    let seconds = day_seconds % 60;

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

    let mut month: u32 = 1;
    while month <= 12 {
        let dim = days_in_month(year, month) as i64;
        if remaining_days < dim {
            break;
        }
        remaining_days -= dim;
        month += 1;
    }

    let day = remaining_days as u32 + 1; // 1-indexed
    (year, month, day, hours, minutes, seconds)
}

/// Format epoch seconds as "YYYY-MM-DD HH:MM".
fn format_datetime(epoch_secs: u64) -> String {
    let (year, month, day, hours, minutes, _seconds) = epoch_to_parts(epoch_secs);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
}

/// Format epoch seconds as "YYYY-MM-DD HH:MM:SS" (full precision).
fn format_datetime_full(epoch_secs: u64) -> String {
    let (year, month, day, hours, minutes, seconds) = epoch_to_parts(epoch_secs);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

/// Format just the time portion "HH:MM:SS" from epoch seconds.
fn format_time(epoch_secs: u64) -> String {
    let day_seconds = epoch_secs % 86400;
    let hours = day_seconds / 3600;
    let minutes = (day_seconds % 3600) / 60;
    let seconds = day_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

/// Format just "HH:MM" from epoch seconds (for `w` LOGIN@ column).
fn format_time_short(epoch_secs: u64) -> String {
    let day_seconds = epoch_secs % 86400;
    let hours = day_seconds / 3600;
    let minutes = (day_seconds % 3600) / 60;
    format!("{hours:02}:{minutes:02}")
}

/// Format an idle duration in seconds into a human-readable string.
///
/// Convention: "." means active (< 60 seconds idle), then "MM:SS" or
/// "HH:MMm" or "Xdays" for longer durations.
fn format_idle(idle_secs: u64) -> String {
    if idle_secs < 60 {
        return ".".to_string();
    }
    let minutes = idle_secs / 60;
    let seconds = idle_secs % 60;
    if minutes < 60 {
        return format!("{minutes:2}:{seconds:02}");
    }
    let hours = minutes / 60;
    let remaining_min = minutes % 60;
    if hours < 24 {
        return format!("{hours:2}:{remaining_min:02}m");
    }
    let days = hours / 24;
    format!("{days}days")
}

/// Format a CPU time in hundredths of a second as "N.NNs".
fn format_cpu_time(hundredths: u64) -> String {
    let secs = hundredths / 100;
    let frac = hundredths % 100;
    format!("{secs}.{frac:02}s")
}

// ============================================================================
// Data source: /var/run/utmp (binary utmp records)
// ============================================================================

/// Extract a nul-terminated string from a byte slice.
fn extract_string(data: &[u8], offset: usize, max_len: usize) -> Option<String> {
    let end = offset.checked_add(max_len)?;
    let slice = data.get(offset..end)?;
    // Find the first nul byte; the string is everything before it.
    let nul_pos = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    let text_slice = slice.get(..nul_pos)?;
    Some(String::from_utf8_lossy(text_slice).into_owned())
}

/// Read an i32 (little-endian) from a byte slice at the given offset.
fn read_i32_le(data: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    let bytes = data.get(offset..end)?;
    Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Parse all records from a utmp file.
fn read_utmp(path: &str) -> Option<Vec<SessionRecord>> {
    let data = fs::read(path).ok()?;
    if data.len() < UTMP_RECORD_SIZE {
        return None;
    }

    let mut records = Vec::new();
    let mut offset = 0;

    while offset + UTMP_RECORD_SIZE <= data.len() {
        let record_type = read_i32_le(&data, offset + UT_TYPE_OFFSET)?;
        let pid = read_i32_le(&data, offset + UT_PID_OFFSET)?;
        let tty = extract_string(&data, offset + UT_LINE_OFFSET, UT_LINE_SIZE)?;
        let id = extract_string(&data, offset + UT_ID_OFFSET, UT_ID_SIZE)?;
        let user = extract_string(&data, offset + UT_USER_OFFSET, UT_USER_SIZE)?;
        let host = extract_string(&data, offset + UT_HOST_OFFSET, UT_HOST_SIZE)?;
        let tv_sec = read_i32_le(&data, offset + UT_TV_SEC_OFFSET)?;

        // tv_sec is signed but represents a positive epoch timestamp.
        let login_time = if tv_sec >= 0 { tv_sec as u64 } else { 0 };

        records.push(SessionRecord {
            record_type,
            user,
            tty,
            host,
            pid,
            login_time,
            id,
        });

        offset += UTMP_RECORD_SIZE;
    }

    if records.is_empty() {
        None
    } else {
        Some(records)
    }
}

// ============================================================================
// Data source: /run/sessions/ (SlateOS native)
// ============================================================================

/// Parse session files from /run/sessions/.
///
/// Each file is named by session ID and contains key=value lines:
/// `user=<name>\ntty=<tty>\nhost=<host>\ntime=<epoch>\n`
fn read_sessions_dir(path: &str) -> Option<Vec<SessionRecord>> {
    let entries = fs::read_dir(path).ok()?;
    let mut records = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let session_id = entry.file_name().to_string_lossy().into_owned();
        let content = match fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut user = String::new();
        let mut tty = String::new();
        let mut host = String::new();
        let mut login_time: u64 = 0;

        for line in content.lines() {
            if let Some(val) = line.strip_prefix("user=") {
                user = val.to_string();
            } else if let Some(val) = line.strip_prefix("tty=") {
                tty = val.to_string();
            } else if let Some(val) = line.strip_prefix("host=") {
                host = val.to_string();
            } else if let Some(val) = line.strip_prefix("time=") {
                login_time = val.parse().unwrap_or(0);
            }
        }

        if !user.is_empty() {
            records.push(SessionRecord {
                record_type: USER_PROCESS,
                user,
                tty,
                host,
                pid: 0,
                login_time,
                id: session_id,
            });
        }
    }

    if records.is_empty() {
        None
    } else {
        Some(records)
    }
}

// ============================================================================
// Data source: /tmp/.users/ (fallback)
// ============================================================================

/// Read login records from /tmp/.users/.
///
/// Each file is named by username; its content is the login epoch timestamp.
fn read_users_dir(path: &str) -> Option<Vec<SessionRecord>> {
    let entries = fs::read_dir(path).ok()?;
    let mut records = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let user = entry.file_name().to_string_lossy().into_owned();
        let content = fs::read_to_string(entry.path()).unwrap_or_default();
        let login_time: u64 = content.trim().parse().unwrap_or(0);

        records.push(SessionRecord {
            record_type: USER_PROCESS,
            user,
            tty: "?".to_string(),
            host: String::new(),
            pid: 0,
            login_time,
            id: String::new(),
        });
    }

    if records.is_empty() {
        None
    } else {
        Some(records)
    }
}

// ============================================================================
// Unified data loading
// ============================================================================

/// Load session records from the first available data source.
fn load_records() -> Vec<SessionRecord> {
    // Try sources in priority order.
    if let Some(records) = read_utmp("/var/run/utmp") {
        return records;
    }
    if let Some(records) = read_sessions_dir("/run/sessions") {
        return records;
    }
    if let Some(records) = read_users_dir("/tmp/.users") {
        return records;
    }
    Vec::new()
}

// ============================================================================
// System info for `w` mode
// ============================================================================

/// Parsed uptime from /proc/uptime.
struct UptimeInfo {
    total_seconds: f64,
}

/// Parsed load averages from /proc/loadavg.
struct LoadAvgInfo {
    avg_1: f64,
    avg_5: f64,
    avg_15: f64,
}

/// Per-user process info for `w` display.
struct UserProcessInfo {
    /// Idle time in seconds (from terminal device mtime).
    idle_secs: u64,
    /// Total CPU time for all processes on this tty (in hundredths of a second).
    jcpu: u64,
    /// CPU time for the current foreground process (in hundredths of a second).
    pcpu: u64,
    /// Command name of the current foreground process.
    what: String,
}

/// Read uptime from /proc/uptime.
fn read_uptime() -> Option<UptimeInfo> {
    let content = fs::read_to_string("/proc/uptime").ok()?;
    let total_seconds: f64 = content.split_whitespace().next()?.parse().ok()?;
    Some(UptimeInfo { total_seconds })
}

/// Read load averages from /proc/loadavg.
fn read_loadavg() -> Option<LoadAvgInfo> {
    let content = fs::read_to_string("/proc/loadavg").ok()?;
    let mut parts = content.split_whitespace();
    let avg_1: f64 = parts.next()?.parse().ok()?;
    let avg_5: f64 = parts.next()?.parse().ok()?;
    let avg_15: f64 = parts.next()?.parse().ok()?;
    Some(LoadAvgInfo {
        avg_1,
        avg_5,
        avg_15,
    })
}

/// Format uptime duration for the `w` header line.
fn format_uptime_for_w(total_secs: f64) -> String {
    let secs = total_secs as u64;
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;

    if days > 0 {
        let day_word = if days == 1 { "day" } else { "days" };
        format!("{days} {day_word}, {hours:2}:{minutes:02}")
    } else if hours > 0 {
        format!("{hours:2}:{minutes:02}")
    } else {
        format!("{minutes} min")
    }
}

/// Get idle time for a terminal device by comparing its mtime to now.
fn get_tty_idle(tty: &str, now: u64) -> u64 {
    if tty.is_empty() || tty == "?" {
        return 0;
    }
    // Try /dev/<tty> path.
    let dev_path = if tty.starts_with('/') {
        tty.to_string()
    } else {
        format!("/dev/{tty}")
    };

    let metadata = match fs::metadata(&dev_path) {
        Ok(m) => m,
        Err(_) => return 0,
    };

    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if mtime == 0 || now < mtime {
        0
    } else {
        now - mtime
    }
}

/// Check if a terminal is writable (for message status).
///
/// Returns '+' if writable by group/others, '-' if not, '?' if unknown.
fn get_mesg_status(tty: &str) -> char {
    if tty.is_empty() || tty == "?" {
        return '?';
    }
    let dev_path = if tty.starts_with('/') {
        tty.to_string()
    } else {
        format!("/dev/{tty}")
    };

    // On Unix, check if the file is group-writable by reading permissions.
    // We read /proc/self or use metadata. Since std::fs::Permissions on Unix
    // exposes mode(), but we are on a custom target, try reading permissions
    // via the metadata and checking if the file exists and is writable.
    match fs::metadata(&dev_path) {
        Ok(_meta) => {
            // On our custom target, attempt to open the device for writing
            // as a heuristic.
            match fs::OpenOptions::new().write(true).open(&dev_path) {
                Ok(_) => '+',
                Err(_) => '-',
            }
        }
        Err(_) => '?',
    }
}

/// Gather process info for a user session (for `w` mode).
///
/// Scans /proc to find processes on the given tty and computes JCPU, PCPU,
/// and the WHAT field.
fn get_user_process_info(tty: &str, pid: i32, now: u64) -> UserProcessInfo {
    let idle_secs = get_tty_idle(tty, now);
    let mut jcpu: u64 = 0;
    let mut pcpu: u64 = 0;
    let mut what = String::from("-");

    // Normalise tty name for comparison with /proc/<pid>/stat field 7.
    // The tty field in /proc/stat is a device number, so instead we check
    // /proc/<pid>/fd/0 symlink or /proc/<pid>/stat. For simplicity, scan
    // /proc/<pid>/cmdline for all pids, matching those whose
    // /proc/<pid>/stat contains the tty.

    // If we have a specific PID from utmp, read its info directly.
    if pid > 0
        && let Some((cpu_time, cmd)) = read_proc_stat_brief(pid as u32) {
            pcpu = cpu_time;
            jcpu = cpu_time;
            what = cmd;
        }

    // Try to scan /proc for all processes on this tty to get total JCPU.
    // This is best-effort; if /proc is not available, we use the single-process info.
    if let Ok(entries) = fs::read_dir("/proc") {
        let mut total_cpu: u64 = 0;
        let mut found_any = false;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Only process numeric directory names.
            let proc_pid: u32 = match name_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };

            // Check if this process is on the same tty.
            if let Some(proc_tty) = read_proc_tty(proc_pid)
                && proc_tty == tty
                    && let Some((cpu_time, _cmd)) = read_proc_stat_brief(proc_pid) {
                        total_cpu = total_cpu.saturating_add(cpu_time);
                        found_any = true;
                    }
        }

        if found_any {
            jcpu = total_cpu;
        }
    }

    UserProcessInfo {
        idle_secs,
        jcpu,
        pcpu,
        what,
    }
}

/// Read the tty name from /proc/<pid>/stat (field 7 is tty_nr, but we use
/// /proc/<pid>/fd/0 as a symlink to the controlling terminal).
fn read_proc_tty(pid: u32) -> Option<String> {
    let link = fs::read_link(format!("/proc/{pid}/fd/0")).ok()?;
    let path_str = link.to_string_lossy();
    // /dev/tty1 -> tty1, /dev/pts/0 -> pts/0
    path_str.strip_prefix("/dev/").map(|s| s.to_string())
}

/// Read brief process info from /proc/<pid>/stat:
/// Returns (user_time + sys_time in centiseconds, command name).
fn read_proc_stat_brief(pid: u32) -> Option<(u64, String)> {
    let content = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;

    // Format: pid (comm) state ppid pgrp session tty_nr ... utime stime ...
    // The comm field is in parentheses and may contain spaces, so find the
    // last ')' to locate the end of the command name.
    let comm_start = content.find('(')?;
    let comm_end = content.rfind(')')?;
    let comm = content.get(comm_start + 1..comm_end)?.to_string();

    // Fields after ')' are space-separated; field index 0 after ')' is state.
    let after_comm = content.get(comm_end + 2..)?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();

    // utime is field index 11 (0-indexed after the ')' delimiter),
    // stime is field index 12.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;

    Some((utime.saturating_add(stime), comm))
}

// ============================================================================
// Record filtering
// ============================================================================

/// Get the name of a utmp record type.
fn type_name(record_type: i32) -> &'static str {
    match record_type {
        EMPTY => "EMPTY",
        RUN_LVL => "RUN_LVL",
        BOOT_TIME => "BOOT_TIME",
        NEW_TIME => "NEW_TIME",
        OLD_TIME => "OLD_TIME",
        INIT_PROCESS => "INIT_PROC",
        LOGIN_PROCESS => "LOGIN",
        USER_PROCESS => "USER",
        DEAD_PROCESS => "DEAD",
        _ => "UNKNOWN",
    }
}

/// Filter records based on the selected options.
fn filter_records<'a>(records: &'a [SessionRecord], opts: &Options) -> Vec<&'a SessionRecord> {
    records
        .iter()
        .filter(|r| {
            if opts.show_all {
                // Show all non-empty records.
                return r.record_type != EMPTY;
            }
            if opts.show_boot {
                return r.record_type == BOOT_TIME;
            }
            if opts.show_dead {
                return r.record_type == DEAD_PROCESS;
            }
            if opts.show_login {
                return r.record_type == LOGIN_PROCESS;
            }
            // Default: show only USER_PROCESS records.
            r.record_type == USER_PROCESS
        })
        .collect()
}

/// Get the current user's login name.
fn get_current_user() -> String {
    // Try USER env var, then LOGNAME, then read /proc/self/status for uid
    // and resolve via /etc/passwd.
    if let Ok(user) = env::var("USER")
        && !user.is_empty() {
            return user;
        }
    if let Ok(user) = env::var("LOGNAME")
        && !user.is_empty() {
            return user;
        }
    // Try to read UID from /proc/self/status and map to username.
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                let uid_str = rest.split_whitespace().next().unwrap_or("0");
                let uid: u32 = uid_str.parse().unwrap_or(0);
                return uid_to_name(uid);
            }
        }
    }
    "unknown".to_string()
}

/// Map a UID to a username by scanning /etc/passwd.
fn uid_to_name(uid: u32) -> String {
    if let Ok(content) = fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3
                && let Ok(line_uid) = fields[2].parse::<u32>()
                    && line_uid == uid {
                        return fields[0].to_string();
                    }
        }
    }
    format!("{uid}")
}

/// Get the current user's terminal.
fn get_current_tty() -> String {
    // Try /proc/self/fd/0 symlink.
    if let Ok(link) = fs::read_link("/proc/self/fd/0") {
        let path_str = link.to_string_lossy();
        if let Some(tty) = path_str.strip_prefix("/dev/") {
            return tty.to_string();
        }
    }
    "?".to_string()
}

// ============================================================================
// Output: standard `who` format
// ============================================================================

/// Print the standard `who` output for a set of records.
fn print_who(records: &[&SessionRecord], opts: &Options) {
    let now = current_epoch_secs();

    if opts.show_heading {
        if opts.show_mesg {
            println!(
                "{:<12} M {:<12} {:<16} {:>8}   COMMENT",
                "NAME", "LINE", "TIME", "IDLE"
            );
        } else if opts.show_idle {
            println!(
                "{:<12} {:<12} {:<16} {:>8}   COMMENT",
                "NAME", "LINE", "TIME", "IDLE"
            );
        } else if opts.show_all {
            println!(
                "{:<12} {:<9} {:<12} {:<4} {:<16} {:>6}   COMMENT",
                "NAME", "TYPE", "LINE", "ID", "TIME", "PID"
            );
        } else {
            println!("{:<12} {:<12} TIME", "NAME", "LINE");
        }
    }

    for record in records {
        if opts.show_all {
            print_record_all(record, opts, now);
        } else if opts.show_mesg {
            print_record_mesg(record, now);
        } else if opts.show_idle {
            print_record_idle(record, now);
        } else {
            print_record_short(record);
        }
    }
}

/// Print a record in short format: NAME  LINE  TIME  (HOST)
fn print_record_short(record: &SessionRecord) {
    let time_str = format_datetime(record.login_time);
    if record.host.is_empty() {
        println!("{:<12} {:<12} {}", record.user, record.tty, time_str);
    } else {
        println!(
            "{:<12} {:<12} {} ({})",
            record.user, record.tty, time_str, record.host
        );
    }
}

/// Print a record with message status.
fn print_record_mesg(record: &SessionRecord, now: u64) {
    let time_str = format_datetime(record.login_time);
    let mesg = get_mesg_status(&record.tty);
    let idle = format_idle(get_tty_idle(&record.tty, now));
    let comment = if record.host.is_empty() {
        String::new()
    } else {
        format!("({})", record.host)
    };
    println!(
        "{:<12} {} {:<12} {:<16} {:>8}   {}",
        record.user, mesg, record.tty, time_str, idle, comment
    );
}

/// Print a record with idle time.
fn print_record_idle(record: &SessionRecord, now: u64) {
    let time_str = format_datetime(record.login_time);
    let idle = format_idle(get_tty_idle(&record.tty, now));
    let comment = if record.host.is_empty() {
        String::new()
    } else {
        format!("({})", record.host)
    };
    println!(
        "{:<12} {:<12} {:<16} {:>8}   {}",
        record.user, record.tty, time_str, idle, comment
    );
}

/// Print a record in full --all format with type, id, pid.
fn print_record_all(record: &SessionRecord, _opts: &Options, now: u64) {
    let time_str = format_datetime(record.login_time);
    let tname = type_name(record.record_type);
    let idle = if record.record_type == USER_PROCESS {
        format_idle(get_tty_idle(&record.tty, now))
    } else {
        String::new()
    };
    let comment = if record.host.is_empty() {
        String::new()
    } else {
        format!("({})", record.host)
    };
    let pid_str = if record.pid != 0 {
        format!("{}", record.pid)
    } else {
        String::new()
    };

    println!(
        "{:<12} {:<9} {:<12} {:<4} {:<16} {:>6} {:>8} {}",
        record.user, tname, record.tty, record.id, time_str, pid_str, idle, comment
    );
}

// ============================================================================
// Output: count/quick mode
// ============================================================================

/// Print users in quick/count format: names on one line, then count.
fn print_count(records: &[&SessionRecord]) {
    let names: Vec<&str> = records.iter().map(|r| r.user.as_str()).collect();
    println!("{}", names.join(" "));
    println!("# users={}", records.len());
}

// ============================================================================
// Output: `w` format
// ============================================================================

/// Print the `w`-style extended output.
fn print_w(records: &[&SessionRecord]) {
    let now = current_epoch_secs();
    let user_count = records.len();

    // Header line: current time, uptime, user count, load averages.
    let time_str = format_time(now);
    let uptime_str = match read_uptime() {
        Some(info) => format!("up {}", format_uptime_for_w(info.total_seconds)),
        None => "up ?".to_string(),
    };
    let load_str = match read_loadavg() {
        Some(load) => {
            format!(
                "load average: {:.2}, {:.2}, {:.2}",
                load.avg_1, load.avg_5, load.avg_15
            )
        }
        None => "load average: ?, ?, ?".to_string(),
    };
    let user_word = if user_count == 1 { "user" } else { "users" };

    println!(" {time_str} {uptime_str},  {user_count} {user_word},  {load_str}");

    // Column headings.
    println!(
        "{:<8} {:<8} {:<16} {:<8} {:>6} {:>6} {:>6} WHAT",
        "USER", "TTY", "FROM", "LOGIN@", "IDLE", "JCPU", "PCPU"
    );

    // Per-user rows.
    for record in records {
        let proc_info = get_user_process_info(&record.tty, record.pid, now);
        let login_at = format_time_short(record.login_time);
        let from = if record.host.is_empty() {
            "-".to_string()
        } else {
            record.host.clone()
        };
        let idle_str = format_idle(proc_info.idle_secs);
        let jcpu_str = format_cpu_time(proc_info.jcpu);
        let pcpu_str = format_cpu_time(proc_info.pcpu);

        println!(
            "{:<8} {:<8} {:<16} {:<8} {:>6} {:>6} {:>6} {}",
            record.user, record.tty, from, login_at, idle_str, jcpu_str, pcpu_str, proc_info.what
        );
    }
}

// ============================================================================
// Output: JSON
// ============================================================================

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
                    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Print records in JSON format.
fn print_json(records: &[&SessionRecord], opts: &Options) {
    let now = current_epoch_secs();
    println!("{{");
    println!("  \"timestamp\": {now},");
    println!(
        "  \"timestamp_formatted\": \"{}\",",
        json_escape(&format_datetime_full(now))
    );
    println!("  \"users\": [");

    for (i, record) in records.iter().enumerate() {
        let comma = if i + 1 < records.len() { "," } else { "" };
        let idle = if opts.show_idle || opts.w_mode {
            get_tty_idle(&record.tty, now)
        } else {
            0
        };
        let mesg = if opts.show_mesg {
            format!("{}", get_mesg_status(&record.tty))
        } else {
            String::new()
        };

        println!("    {{");
        println!("      \"user\": \"{}\",", json_escape(&record.user));
        println!("      \"tty\": \"{}\",", json_escape(&record.tty));
        println!("      \"host\": \"{}\",", json_escape(&record.host));
        println!("      \"login_time\": {},", record.login_time);
        println!(
            "      \"login_time_formatted\": \"{}\",",
            json_escape(&format_datetime(record.login_time))
        );
        println!("      \"pid\": {},", record.pid);
        println!("      \"type\": \"{}\",", type_name(record.record_type));

        if opts.show_idle || opts.w_mode {
            println!("      \"idle_seconds\": {idle},");
        }
        if opts.show_mesg {
            println!("      \"mesg\": \"{}\",", json_escape(&mesg));
        }

        // The last field must not have a trailing comma.
        println!("      \"id\": \"{}\"", json_escape(&record.id));
        println!("    }}{comma}");
    }

    println!("  ]");
    println!("}}");
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_help() {
    println!("SlateOS Logged-In Users Display v0.1.0");
    println!();
    println!("Show who is logged in to the system.");
    println!();
    println!("USAGE:");
    println!("  who [OPTION]...");
    println!("  who am i");
    println!("  w");
    println!();
    println!("OPTIONS:");
    println!("  -a, --all       Show all records (dead, login, boot, etc.)");
    println!("  -b, --boot      Show last boot time");
    println!("  -d, --dead      Show dead processes");
    println!("  -H, --heading   Show column headings");
    println!("  -l, --login     Show system login processes");
    println!("  -q, --count     Show user count and names only");
    println!("  -s, --short     Short format (default)");
    println!("  -T, --mesg      Show message status (+/-/?) for terminal");
    println!("  -u, --users     Show idle time");
    println!("      --json      JSON output");
    println!("  -h, --help      Display this help and exit");
    println!("  -V, --version   Display version and exit");
    println!();
    println!("When invoked as 'w', shows extended format with uptime, idle,");
    println!("CPU usage, and current process for each user.");
}

fn print_version() {
    println!("who (SlateOS) 0.1.0");
}

/// Parse command-line arguments into Options.
fn parse_args(args: &[String]) -> Result<Options, i32> {
    let mut opts = Options::new();

    // Check if invoked as `w` via argv[0].
    if let Some(prog) = args.first() {
        let basename = prog
            .rsplit('/')
            .next()
            .unwrap_or(prog)
            .rsplit('\\')
            .next()
            .unwrap_or(prog);
        if basename == "w" || basename == "w.exe" {
            opts.w_mode = true;
            opts.show_heading = true;
        }
    }

    // GNU `who` treats any invocation with exactly two non-option operands
    // ("who am i", "who mom likes", ...) as the -m / current-user form.
    if let (Some(a1), Some(a2)) = (args.get(1), args.get(2))
        && args.len() == 3
        && !a1.starts_with('-')
        && !a2.starts_with('-')
    {
        opts.am_i = true;
        return Ok(opts);
    }

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-a" | "--all" => {
                opts.show_all = true;
                opts.show_heading = true;
            }
            "-b" | "--boot" => opts.show_boot = true,
            "-d" | "--dead" => opts.show_dead = true,
            "-H" | "--heading" => opts.show_heading = true,
            "-l" | "--login" => opts.show_login = true,
            "-q" | "--count" => opts.show_count = true,
            "-s" | "--short" => {} // default, no-op
            "-T" | "--mesg" => opts.show_mesg = true,
            "-u" | "--users" => opts.show_idle = true,
            "--json" => opts.json = true,
            "-h" | "--help" => {
                print_help();
                return Err(0);
            }
            "-V" | "--version" => {
                print_version();
                return Err(0);
            }
            other => {
                eprintln!("who: unknown option: {other}");
                eprintln!("Try 'who --help' for usage.");
                return Err(1);
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();

    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    // Load all session records.
    let records = load_records();

    // If "who am i", filter to current user and tty.
    if opts.am_i {
        let current_user = get_current_user();
        let current_tty = get_current_tty();
        let my_records: Vec<&SessionRecord> = records
            .iter()
            .filter(|r| {
                r.record_type == USER_PROCESS
                    && r.user == current_user
                    && (r.tty == current_tty || current_tty == "?")
            })
            .collect();

        if my_records.is_empty() {
            // Synthesise a record from environment info.
            let now = current_epoch_secs();
            println!(
                "{:<12} {:<12} {}",
                current_user,
                current_tty,
                format_datetime(now)
            );
        } else if opts.json {
            print_json(&my_records, &opts);
        } else {
            print_who(&my_records, &opts);
        }
        return 0;
    }

    // Filter records based on options.
    let filtered = filter_records(&records, &opts);

    // Only show USER_PROCESS records for w and count modes.
    let user_records: Vec<&SessionRecord> = if opts.w_mode || opts.show_count {
        filtered
            .into_iter()
            .filter(|r| r.record_type == USER_PROCESS)
            .collect()
    } else {
        filtered
    };

    if opts.json {
        print_json(&user_records, &opts);
    } else if opts.w_mode {
        print_w(&user_records);
    } else if opts.show_count {
        print_count(&user_records);
    } else {
        print_who(&user_records, &opts);
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

    // --- Time helpers ---

    #[test]
    fn test_epoch_to_parts_epoch_zero() {
        let (y, m, d, h, min, s) = epoch_to_parts(0);
        assert_eq!((y, m, d, h, min, s), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn test_epoch_to_parts_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let (y, m, d, h, min, s) = epoch_to_parts(1_704_067_200);
        assert_eq!((y, m, d), (2024, 1, 1));
        assert_eq!((h, min, s), (0, 0, 0));
    }

    #[test]
    fn test_epoch_to_parts_with_time() {
        // 2024-06-15 14:30:45 UTC = 1718461845
        let (y, m, d, h, min, s) = epoch_to_parts(1_718_461_845);
        assert_eq!((y, m, d), (2024, 6, 15));
        assert_eq!((h, min, s), (14, 30, 45));
    }

    #[test]
    fn test_epoch_to_parts_leap_year_feb29() {
        // 2024-02-29 12:00:00 UTC = 1709208000
        let (y, m, d, h, min, s) = epoch_to_parts(1_709_208_000);
        assert_eq!((y, m, d), (2024, 2, 29));
        assert_eq!((h, min, s), (12, 0, 0));
    }

    #[test]
    fn test_epoch_to_parts_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        let (y, m, d, h, min, s) = epoch_to_parts(1_704_067_199);
        assert_eq!((y, m, d), (2023, 12, 31));
        assert_eq!((h, min, s), (23, 59, 59));
    }

    #[test]
    fn test_epoch_to_parts_y2k() {
        // 2000-01-01 00:00:00 UTC = 946684800
        let (y, m, d, _, _, _) = epoch_to_parts(946_684_800);
        assert_eq!((y, m, d), (2000, 1, 1));
    }

    #[test]
    fn test_format_datetime() {
        assert_eq!(format_datetime(1_704_067_200), "2024-01-01 00:00");
    }

    #[test]
    fn test_format_datetime_full() {
        assert_eq!(format_datetime_full(1_718_461_845), "2024-06-15 14:30:45");
    }

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(52245), "14:30:45");
        assert_eq!(format_time(0), "00:00:00");
        assert_eq!(format_time(86399), "23:59:59");
    }

    #[test]
    fn test_format_time_short() {
        assert_eq!(format_time_short(52200), "14:30");
        assert_eq!(format_time_short(0), "00:00");
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
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 7), 31);
        assert_eq!(days_in_month(2024, 12), 31);
    }

    // --- Idle time formatting ---

    #[test]
    fn test_format_idle_active() {
        assert_eq!(format_idle(0), ".");
        assert_eq!(format_idle(30), ".");
        assert_eq!(format_idle(59), ".");
    }

    #[test]
    fn test_format_idle_minutes() {
        assert_eq!(format_idle(60), " 1:00");
        assert_eq!(format_idle(90), " 1:30");
        assert_eq!(format_idle(3599), "59:59");
    }

    #[test]
    fn test_format_idle_hours() {
        assert_eq!(format_idle(3600), " 1:00m");
        assert_eq!(format_idle(7200), " 2:00m");
    }

    #[test]
    fn test_format_idle_days() {
        assert_eq!(format_idle(86400), "1days");
        assert_eq!(format_idle(172800), "2days");
    }

    // --- CPU time formatting ---

    #[test]
    fn test_format_cpu_time() {
        assert_eq!(format_cpu_time(0), "0.00s");
        assert_eq!(format_cpu_time(100), "1.00s");
        assert_eq!(format_cpu_time(150), "1.50s");
        assert_eq!(format_cpu_time(12345), "123.45s");
    }

    // --- JSON escape ---

    #[test]
    fn test_json_escape_plain() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn test_json_escape_special() {
        assert_eq!(json_escape("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        assert_eq!(json_escape("a\rb"), "a\\rb");
    }

    // --- String extraction from bytes ---

    #[test]
    fn test_extract_string_normal() {
        let data = b"hello\0world";
        assert_eq!(extract_string(data, 0, 11), Some("hello".to_string()));
    }

    #[test]
    fn test_extract_string_no_nul() {
        let data = b"hello";
        assert_eq!(extract_string(data, 0, 5), Some("hello".to_string()));
    }

    #[test]
    fn test_extract_string_empty() {
        let data = b"\0rest";
        assert_eq!(extract_string(data, 0, 4), Some(String::new()));
    }

    #[test]
    fn test_extract_string_offset() {
        let data = b"XXXXhello\0";
        assert_eq!(extract_string(data, 4, 6), Some("hello".to_string()));
    }

    #[test]
    fn test_extract_string_out_of_bounds() {
        let data = b"hi";
        assert_eq!(extract_string(data, 0, 10), None);
    }

    // --- i32 reading ---

    #[test]
    fn test_read_i32_le() {
        let data: &[u8] = &[0x07, 0x00, 0x00, 0x00];
        assert_eq!(read_i32_le(data, 0), Some(7));
    }

    #[test]
    fn test_read_i32_le_negative() {
        let data: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_i32_le(data, 0), Some(-1));
    }

    #[test]
    fn test_read_i32_le_out_of_bounds() {
        let data: &[u8] = &[0x01, 0x02];
        assert_eq!(read_i32_le(data, 0), None);
    }

    // --- Type name ---

    #[test]
    fn test_type_name() {
        assert_eq!(type_name(EMPTY), "EMPTY");
        assert_eq!(type_name(BOOT_TIME), "BOOT_TIME");
        assert_eq!(type_name(USER_PROCESS), "USER");
        assert_eq!(type_name(DEAD_PROCESS), "DEAD");
        assert_eq!(type_name(LOGIN_PROCESS), "LOGIN");
        assert_eq!(type_name(99), "UNKNOWN");
    }

    // --- Record filtering ---

    fn make_record(record_type: i32, user: &str, tty: &str) -> SessionRecord {
        SessionRecord {
            record_type,
            user: user.to_string(),
            tty: tty.to_string(),
            host: String::new(),
            pid: 0,
            login_time: 0,
            id: String::new(),
        }
    }

    #[test]
    fn test_filter_default_only_user_process() {
        let records = vec![
            make_record(USER_PROCESS, "alice", "tty1"),
            make_record(DEAD_PROCESS, "bob", "tty2"),
            make_record(BOOT_TIME, "reboot", "~"),
            make_record(USER_PROCESS, "carol", "pts/0"),
        ];
        let opts = Options::new();
        let filtered = filter_records(&records, &opts);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].user, "alice");
        assert_eq!(filtered[1].user, "carol");
    }

    #[test]
    fn test_filter_all_non_empty() {
        let records = vec![
            make_record(USER_PROCESS, "alice", "tty1"),
            make_record(DEAD_PROCESS, "bob", "tty2"),
            make_record(EMPTY, "", ""),
            make_record(BOOT_TIME, "reboot", "~"),
        ];
        let mut opts = Options::new();
        opts.show_all = true;
        let filtered = filter_records(&records, &opts);
        assert_eq!(filtered.len(), 3); // all except EMPTY
    }

    #[test]
    fn test_filter_boot_only() {
        let records = vec![
            make_record(USER_PROCESS, "alice", "tty1"),
            make_record(BOOT_TIME, "reboot", "~"),
        ];
        let mut opts = Options::new();
        opts.show_boot = true;
        let filtered = filter_records(&records, &opts);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].record_type, BOOT_TIME);
    }

    #[test]
    fn test_filter_dead_only() {
        let records = vec![
            make_record(USER_PROCESS, "alice", "tty1"),
            make_record(DEAD_PROCESS, "bob", "tty2"),
        ];
        let mut opts = Options::new();
        opts.show_dead = true;
        let filtered = filter_records(&records, &opts);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].user, "bob");
    }

    #[test]
    fn test_filter_login_only() {
        let records = vec![
            make_record(USER_PROCESS, "alice", "tty1"),
            make_record(LOGIN_PROCESS, "LOGIN", "tty3"),
        ];
        let mut opts = Options::new();
        opts.show_login = true;
        let filtered = filter_records(&records, &opts);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].record_type, LOGIN_PROCESS);
    }

    // --- Arg parsing ---

    #[test]
    fn test_parse_args_default() {
        let args = vec!["who".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(!opts.show_all);
        assert!(!opts.w_mode);
        assert!(!opts.am_i);
    }

    #[test]
    fn test_parse_args_w_mode() {
        let args = vec!["w".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.w_mode);
        assert!(opts.show_heading);
    }

    #[test]
    fn test_parse_args_am_i() {
        let args = vec!["who".to_string(), "am".to_string(), "i".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.am_i);
    }

    #[test]
    fn test_parse_args_all() {
        let args = vec!["who".to_string(), "-a".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.show_all);
        assert!(opts.show_heading);
    }

    #[test]
    fn test_parse_args_multiple_flags() {
        let args = vec![
            "who".to_string(),
            "-H".to_string(),
            "-T".to_string(),
            "-u".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(opts.show_heading);
        assert!(opts.show_mesg);
        assert!(opts.show_idle);
    }

    #[test]
    fn test_parse_args_json() {
        let args = vec!["who".to_string(), "--json".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.json);
    }

    #[test]
    fn test_parse_args_help_returns_zero_exit() {
        let args = vec!["who".to_string(), "--help".to_string()];
        let result = parse_args(&args);
        assert_eq!(result.unwrap_err(), 0);
    }

    #[test]
    fn test_parse_args_unknown_returns_error() {
        let args = vec!["who".to_string(), "--bogus".to_string()];
        let result = parse_args(&args);
        assert_eq!(result.unwrap_err(), 1);
    }

    // --- utmp record parsing ---

    #[test]
    fn test_read_utmp_nonexistent() {
        assert!(read_utmp("/nonexistent/utmp").is_none());
    }

    #[test]
    fn test_read_utmp_too_small() {
        // Attempting to parse data smaller than one record should return None.
        // We cannot create a temp file on the custom target in tests, so we
        // verify the function handles the nonexistent case.
        assert!(read_utmp("/dev/null").is_none());
    }

    // --- Session dir parsing ---

    #[test]
    fn test_read_sessions_dir_nonexistent() {
        assert!(read_sessions_dir("/nonexistent/sessions").is_none());
    }

    #[test]
    fn test_read_users_dir_nonexistent() {
        assert!(read_users_dir("/nonexistent/users").is_none());
    }

    // --- Uptime formatting for w ---

    #[test]
    fn test_format_uptime_for_w_days() {
        let s = format_uptime_for_w(273_915.0); // 3 days, 4:05
        assert!(s.contains("3 days"), "got: {s}");
    }

    #[test]
    fn test_format_uptime_for_w_hours() {
        let s = format_uptime_for_w(7500.0); // 2:05
        assert!(s.contains("2:05"), "got: {s}");
    }

    #[test]
    fn test_format_uptime_for_w_minutes() {
        let s = format_uptime_for_w(2700.0); // 45 min
        assert!(s.contains("45 min"), "got: {s}");
    }

    #[test]
    fn test_format_uptime_for_w_one_day() {
        let s = format_uptime_for_w(86400.0);
        assert!(s.contains("1 day,"), "got: {s}");
        assert!(!s.contains("days"), "got: {s}");
    }

    // --- Mesg status ---

    #[test]
    fn test_get_mesg_status_empty() {
        assert_eq!(get_mesg_status(""), '?');
        assert_eq!(get_mesg_status("?"), '?');
    }

    // --- load_records fallback ---

    #[test]
    fn test_load_records_returns_vec() {
        // On a test machine without utmp/sessions, this should return
        // an empty Vec rather than panicking.
        let records = load_records();
        // Just verify it doesn't panic; length depends on system state.
        let _ = records.len();
    }
}
