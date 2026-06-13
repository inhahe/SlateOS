//! SlateOS Login Records Viewer (`last` / `lastb` / `lastlog`)
//!
//! Multi-personality binary that shows historical login information. The
//! personality is detected via `argv[0]`:
//!
//! - **last**: show listing of last logged-in users from `/var/log/wtmp`
//! - **lastb**: show listing of failed login attempts from `/var/log/btmp`
//! - **lastlog**: show last login time for each user from `/var/log/lastlog`
//!
//! # wtmp/btmp record format (384 bytes)
//!
//! | Offset | Size | Field         |
//! |--------|------|---------------|
//! | 0      | 2    | ut_type (u16) |
//! | 2      | 2    | padding       |
//! | 4      | 4    | ut_pid (u32)  |
//! | 8      | 32   | ut_line       |
//! | 40     | 4    | ut_id         |
//! | 44     | 32   | ut_user       |
//! | 76     | 256  | ut_host       |
//! | 332    | 4    | ut_exit       |
//! | 336    | 4    | ut_session    |
//! | 340    | 4    | ut_tv_sec     |
//! | 344    | 4    | ut_tv_usec    |
//! | 348    | 16   | ut_addr_v6    |
//! | 364    | 20   | unused        |
//! | = 384 bytes total              |

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// Standard wtmp/btmp record size in bytes.
const WTMP_RECORD_SIZE: usize = 384;

// Field offsets within a wtmp record.
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
const UT_EXIT_OFFSET: usize = 332;
const UT_SESSION_OFFSET: usize = 336;
const UT_TV_SEC_OFFSET: usize = 340;
const UT_TV_USEC_OFFSET: usize = 344;
const UT_ADDR_OFFSET: usize = 348;

/// Record type constants.
/// All types are defined for completeness even if not all are referenced in
/// non-test code paths (e.g. EMPTY is only checked implicitly via "not matched").
#[allow(dead_code)]
const EMPTY: u16 = 0;
const RUN_LVL: u16 = 1;
const BOOT_TIME: u16 = 2;
#[allow(dead_code)]
const NEW_TIME: u16 = 3;
#[allow(dead_code)]
const OLD_TIME: u16 = 4;
#[allow(dead_code)]
const INIT_PROCESS: u16 = 5;
#[allow(dead_code)]
const LOGIN_PROCESS: u16 = 6;
const USER_PROCESS: u16 = 7;
const DEAD_PROCESS: u16 = 8;

/// Default file paths.
const DEFAULT_WTMP: &str = "/var/log/wtmp";
const DEFAULT_BTMP: &str = "/var/log/btmp";
const DEFAULT_LASTLOG: &str = "/var/log/lastlog";

/// lastlog record size: 4 (tv_sec) + 32 (line) + 256 (host) = 292 bytes.
const LASTLOG_RECORD_SIZE: usize = 292;
#[allow(dead_code)] // Defined for documentation of the binary format.
const LASTLOG_TIME_SIZE: usize = 4;
const LASTLOG_LINE_OFFSET: usize = 4;
const LASTLOG_LINE_SIZE: usize = 32;
const LASTLOG_HOST_OFFSET: usize = 36;
const LASTLOG_HOST_SIZE: usize = 256;

// ============================================================================
// Personality detection
// ============================================================================

/// Which personality this binary is running as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Last,
    Lastb,
    Lastlog,
}

/// Detect personality from argv[0].
fn detect_personality(argv0: &str) -> Personality {
    let basename = argv0
        .rsplit('/')
        .next()
        .unwrap_or(argv0)
        .rsplit('\\')
        .next()
        .unwrap_or(argv0);
    // Strip common extensions.
    let name = basename
        .strip_suffix(".exe")
        .unwrap_or(basename);
    match name {
        "lastb" => Personality::Lastb,
        "lastlog" => Personality::Lastlog,
        _ => Personality::Last,
    }
}

// ============================================================================
// Data structures
// ============================================================================

/// A single wtmp/btmp record parsed from binary data.
///
/// All fields are parsed from the binary format for completeness. Some fields
/// (id, exit_status, session, time_usec) are not directly used in display
/// output but are preserved for Debug output and potential future use.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct WtmpRecord {
    record_type: u16,
    pid: u32,
    terminal: String,
    id: String,
    user: String,
    host: String,
    exit_status: u32,
    session: u32,
    time_sec: u32,
    time_usec: u32,
    addr: [u32; 4],
}

/// A lastlog record parsed from binary data.
#[derive(Debug, Clone)]
struct LastlogRecord {
    uid: u32,
    username: String,
    time_sec: u32,
    terminal: String,
    host: String,
}

/// Resolved login entry pairing a login with its logout.
#[derive(Debug, Clone)]
struct LoginEntry {
    user: String,
    terminal: String,
    host: String,
    login_time: u32,
    logout_time: Option<u32>,
    record_type: u16,
    addr: [u32; 4],
}

// ============================================================================
// Options for last/lastb
// ============================================================================

#[derive(Debug)]
struct LastOptions {
    /// Maximum number of entries to display (0 = unlimited).
    count: usize,
    /// File to read instead of the default.
    file: String,
    /// Do not show hostname field.
    no_host: bool,
    /// Show hostname in the last column.
    host_last: bool,
    /// Translate IP addresses to hostnames (stub: just shows IP).
    dns_lookup: bool,
    /// Show full login and logout timestamps.
    full_times: bool,
    /// Show IP address instead of hostname.
    show_ip: bool,
    /// Show shutdown and runlevel entries.
    show_system: bool,
    /// Show full user and domain names (no truncation).
    wide: bool,
    /// Filter by username or terminal name.
    filters: Vec<String>,
}

impl LastOptions {
    fn new(default_file: &str) -> Self {
        Self {
            count: 0,
            file: default_file.to_string(),
            no_host: false,
            host_last: false,
            dns_lookup: false,
            full_times: false,
            show_ip: false,
            show_system: false,
            wide: false,
            filters: Vec::new(),
        }
    }
}

// ============================================================================
// Options for lastlog
// ============================================================================

#[derive(Debug)]
struct LastlogOptions {
    /// Show only for this user.
    user_filter: Option<String>,
    /// Show only entries older than this many days.
    before_days: Option<u64>,
    /// Show only entries newer than this many days.
    time_days: Option<u64>,
    /// Clear last login record for user (requires root).
    clear: bool,
    /// File to read.
    file: String,
}

impl LastlogOptions {
    fn new() -> Self {
        Self {
            user_filter: None,
            before_days: None,
            time_days: None,
            clear: false,
            file: DEFAULT_LASTLOG.to_string(),
        }
    }
}

// ============================================================================
// Time helpers
// ============================================================================

/// Whether a year is a leap year in the Gregorian calendar.
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days in a given month (1-indexed).
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
        _ => 30,
    }
}

/// Day-of-week from epoch seconds (0=Thu for epoch, returns 0=Sun..6=Sat).
fn day_of_week(epoch_secs: u64) -> u32 {
    // Jan 1, 1970 was a Thursday (index 4 if 0=Sun).
    let days_since_epoch = epoch_secs / 86400;
    // (days + 4) % 7 gives 0=Sun.
    ((days_since_epoch + 4) % 7) as u32
}

/// Short day-of-week name.
fn dow_name(dow: u32) -> &'static str {
    match dow {
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

/// Short month name (1-indexed).
fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
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

    let day = remaining_days as u32 + 1;
    (year, month, day, hours, minutes, seconds)
}

/// Get current time as seconds since the Unix epoch.
fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format as "Day Mon DD HH:MM" (standard `last` short format).
fn format_short_time(epoch_secs: u32) -> String {
    let t = epoch_secs as u64;
    let (year, month, day, hours, minutes, _seconds) = epoch_to_parts(t);
    let dow = day_of_week(t);
    let _ = year; // Not shown in short format.
    format!(
        "{} {} {:>2} {:02}:{:02}",
        dow_name(dow),
        month_name(month),
        day,
        hours,
        minutes
    )
}

/// Format as "Day Mon DD HH:MM:SS YYYY" (full `last -F` format).
fn format_full_time(epoch_secs: u32) -> String {
    let t = epoch_secs as u64;
    let (year, month, day, hours, minutes, seconds) = epoch_to_parts(t);
    let dow = day_of_week(t);
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {}",
        dow_name(dow),
        month_name(month),
        day,
        hours,
        minutes,
        seconds,
        year
    )
}

/// Format duration in seconds as "(HH:MM)" or "(D+HH:MM)".
fn format_duration(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        format!("({}+{:02}:{:02})", days, hours, minutes)
    } else {
        format!("({:02}:{:02})", hours, minutes)
    }
}

/// Format an IPv4 address from a u32 (network byte order / little-endian stored).
fn format_ipv4(addr: u32) -> String {
    let b0 = addr & 0xFF;
    let b1 = (addr >> 8) & 0xFF;
    let b2 = (addr >> 16) & 0xFF;
    let b3 = (addr >> 24) & 0xFF;
    format!("{}.{}.{}.{}", b0, b1, b2, b3)
}

/// Format an IPv6 address from four u32s.
fn format_ipv6(addr: &[u32; 4]) -> String {
    // If only addr[0] is set and the rest are zero, it is an IPv4 address.
    if addr[1] == 0 && addr[2] == 0 && addr[3] == 0 {
        if addr[0] == 0 {
            return String::new();
        }
        return format_ipv4(addr[0]);
    }
    // Full IPv6 display.
    let mut parts = [0u16; 8];
    for i in 0..4 {
        parts[i * 2] = (addr[i] & 0xFFFF) as u16;
        parts[i * 2 + 1] = ((addr[i] >> 16) & 0xFFFF) as u16;
    }
    let strs: Vec<String> = parts.iter().map(|p| format!("{:x}", p)).collect();
    strs.join(":")
}

/// Get the host display string for a login entry.
fn get_host_display(entry: &LoginEntry, opts: &LastOptions) -> String {
    if opts.no_host {
        return String::new();
    }
    if opts.show_ip {
        let ip = format_ipv6(&entry.addr);
        if ip.is_empty() {
            return entry.host.clone();
        }
        return ip;
    }
    if opts.dns_lookup {
        // Stub: in a real system we would do reverse DNS. For now, show the
        // host field or the IP address if the host is empty.
        if entry.host.is_empty() {
            let ip = format_ipv6(&entry.addr);
            if !ip.is_empty() {
                return ip;
            }
        }
    }
    entry.host.clone()
}

// ============================================================================
// Binary record parsing
// ============================================================================

/// Extract a nul-terminated string from a byte slice.
fn extract_string(data: &[u8], offset: usize, max_len: usize) -> Option<String> {
    let end = offset.checked_add(max_len)?;
    let slice = data.get(offset..end)?;
    let nul_pos = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    let text_slice = slice.get(..nul_pos)?;
    Some(String::from_utf8_lossy(text_slice).into_owned())
}

/// Read a u16 (little-endian) from a byte slice.
fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes = data.get(offset..end)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Read a u32 (little-endian) from a byte slice.
fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes = data.get(offset..end)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Parse all wtmp/btmp records from a file.
fn parse_wtmp_records(data: &[u8]) -> Vec<WtmpRecord> {
    let mut records = Vec::new();
    let mut offset = 0;

    while offset + WTMP_RECORD_SIZE <= data.len() {
        let record_type = match read_u16_le(data, offset + UT_TYPE_OFFSET) {
            Some(v) => v,
            None => {
                offset += WTMP_RECORD_SIZE;
                continue;
            }
        };
        let pid = read_u32_le(data, offset + UT_PID_OFFSET).unwrap_or(0);
        let terminal = extract_string(data, offset + UT_LINE_OFFSET, UT_LINE_SIZE)
            .unwrap_or_default();
        let id = extract_string(data, offset + UT_ID_OFFSET, UT_ID_SIZE)
            .unwrap_or_default();
        let user = extract_string(data, offset + UT_USER_OFFSET, UT_USER_SIZE)
            .unwrap_or_default();
        let host = extract_string(data, offset + UT_HOST_OFFSET, UT_HOST_SIZE)
            .unwrap_or_default();
        let exit_status = read_u32_le(data, offset + UT_EXIT_OFFSET).unwrap_or(0);
        let session = read_u32_le(data, offset + UT_SESSION_OFFSET).unwrap_or(0);
        let time_sec = read_u32_le(data, offset + UT_TV_SEC_OFFSET).unwrap_or(0);
        let time_usec = read_u32_le(data, offset + UT_TV_USEC_OFFSET).unwrap_or(0);
        let mut addr = [0u32; 4];
        for (i, slot) in addr.iter_mut().enumerate() {
            *slot = read_u32_le(data, offset + UT_ADDR_OFFSET + i * 4).unwrap_or(0);
        }

        records.push(WtmpRecord {
            record_type,
            pid,
            terminal,
            id,
            user,
            host,
            exit_status,
            session,
            time_sec,
            time_usec,
            addr,
        });

        offset += WTMP_RECORD_SIZE;
    }

    records
}

/// Build a synthetic wtmp record from raw parts (used in tests).
//
// This helper mirrors the on-disk wtmp record layout and needs all of these
// independent fields to build a faithful test fixture; collapsing them into
// a struct would just spread the arity across additional types.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn build_wtmp_record_bytes(
    record_type: u16,
    pid: u32,
    terminal: &str,
    id: &str,
    user: &str,
    host: &str,
    time_sec: u32,
    addr: [u32; 4],
) -> Vec<u8> {
    let mut buf = vec![0u8; WTMP_RECORD_SIZE];
    // type (u16 LE)
    buf[UT_TYPE_OFFSET] = (record_type & 0xFF) as u8;
    buf[UT_TYPE_OFFSET + 1] = ((record_type >> 8) & 0xFF) as u8;
    // pid (u32 LE)
    let pid_bytes = pid.to_le_bytes();
    buf[UT_PID_OFFSET..UT_PID_OFFSET + 4].copy_from_slice(&pid_bytes);
    // terminal
    let term_bytes = terminal.as_bytes();
    let copy_len = term_bytes.len().min(UT_LINE_SIZE);
    buf[UT_LINE_OFFSET..UT_LINE_OFFSET + copy_len].copy_from_slice(&term_bytes[..copy_len]);
    // id
    let id_bytes = id.as_bytes();
    let copy_len = id_bytes.len().min(UT_ID_SIZE);
    buf[UT_ID_OFFSET..UT_ID_OFFSET + copy_len].copy_from_slice(&id_bytes[..copy_len]);
    // user
    let user_bytes = user.as_bytes();
    let copy_len = user_bytes.len().min(UT_USER_SIZE);
    buf[UT_USER_OFFSET..UT_USER_OFFSET + copy_len].copy_from_slice(&user_bytes[..copy_len]);
    // host
    let host_bytes = host.as_bytes();
    let copy_len = host_bytes.len().min(UT_HOST_SIZE);
    buf[UT_HOST_OFFSET..UT_HOST_OFFSET + copy_len].copy_from_slice(&host_bytes[..copy_len]);
    // time_sec (u32 LE)
    let ts_bytes = time_sec.to_le_bytes();
    buf[UT_TV_SEC_OFFSET..UT_TV_SEC_OFFSET + 4].copy_from_slice(&ts_bytes);
    // addr
    for i in 0..4 {
        let ab = addr[i].to_le_bytes();
        buf[UT_ADDR_OFFSET + i * 4..UT_ADDR_OFFSET + i * 4 + 4].copy_from_slice(&ab);
    }
    buf
}

// ============================================================================
// Login entry resolution (pairing logins with logouts)
// ============================================================================

/// Resolve wtmp records into login entries by matching USER_PROCESS with
/// DEAD_PROCESS on the same terminal, and handling BOOT_TIME/SHUTDOWN events.
fn resolve_login_entries(records: &[WtmpRecord], show_system: bool) -> Vec<LoginEntry> {
    let mut entries = Vec::new();

    // We process records in reverse chronological order (newest first, since
    // wtmp is appended chronologically). For each terminal, we track the most
    // recent logout time.
    let mut logout_times: Vec<(String, u32)> = Vec::new();
    let mut last_shutdown: Option<u32> = None;

    // Process in reverse order to pair logouts with logins.
    for record in records.iter().rev() {
        match record.record_type {
            DEAD_PROCESS
                // Record logout time for this terminal.
                if !record.terminal.is_empty() => {
                    // Remove any existing entry for this terminal, then add new one.
                    logout_times.retain(|(t, _)| t != &record.terminal);
                    logout_times.push((record.terminal.clone(), record.time_sec));
                }
            USER_PROCESS => {
                // Find matching logout.
                let logout = find_and_remove_logout(&mut logout_times, &record.terminal);
                entries.push(LoginEntry {
                    user: record.user.clone(),
                    terminal: record.terminal.clone(),
                    host: record.host.clone(),
                    login_time: record.time_sec,
                    logout_time: logout.or(last_shutdown),
                    record_type: record.record_type,
                    addr: record.addr,
                });
            }
            BOOT_TIME => {
                // A boot means all previous sessions ended.
                last_shutdown = Some(record.time_sec);
                logout_times.clear();
                if show_system {
                    entries.push(LoginEntry {
                        user: "reboot".to_string(),
                        terminal: "system boot".to_string(),
                        host: String::new(),
                        login_time: record.time_sec,
                        logout_time: None,
                        record_type: record.record_type,
                        addr: [0; 4],
                    });
                }
            }
            RUN_LVL if show_system => {
                entries.push(LoginEntry {
                    user: "runlevel".to_string(),
                    terminal: format!("(run-lvl {})", record.pid),
                    host: String::new(),
                    login_time: record.time_sec,
                    logout_time: None,
                    record_type: record.record_type,
                    addr: [0; 4],
                });
            }
            _ => {}
        }
    }

    // Reverse so newest entries are first (standard `last` output order).
    entries.reverse();
    entries
}

/// Find and remove a logout time for a terminal.
fn find_and_remove_logout(logout_times: &mut Vec<(String, u32)>, terminal: &str) -> Option<u32> {
    if let Some(pos) = logout_times.iter().position(|(t, _)| t == terminal) {
        let (_, time) = logout_times.remove(pos);
        Some(time)
    } else {
        None
    }
}

// ============================================================================
// Filtering
// ============================================================================

/// Apply user/tty filters to login entries.
fn filter_entries(entries: &[LoginEntry], filters: &[String]) -> Vec<LoginEntry> {
    if filters.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|e| {
            filters.iter().any(|f| {
                e.user == *f || e.terminal == *f
            })
        })
        .cloned()
        .collect()
}

// ============================================================================
// last/lastb output
// ============================================================================

/// Print login entries in `last` format.
fn print_last_entries(entries: &[LoginEntry], opts: &LastOptions) {
    let now = current_epoch_secs() as u32;

    for (printed, entry) in entries.iter().enumerate() {
        if opts.count > 0 && printed >= opts.count {
            break;
        }

        let user_width = if opts.wide { 32 } else { 8 };
        let user_display = if opts.wide {
            entry.user.clone()
        } else {
            truncate_str(&entry.user, 8)
        };

        let term_display = if opts.wide {
            entry.terminal.clone()
        } else {
            truncate_str(&entry.terminal, 12)
        };

        let host_display = get_host_display(entry, opts);
        let host_width = if opts.wide { 46 } else { 16 };

        let login_str = if opts.full_times {
            format_full_time(entry.login_time)
        } else {
            format_short_time(entry.login_time)
        };

        let status_str = match entry.logout_time {
            Some(logout_time) => {
                if logout_time <= entry.login_time {
                    // Edge case: logout before or at login (corrupt record).
                    "  gone - no logout".to_string()
                } else {
                    let duration_secs = (logout_time - entry.login_time) as u64;
                    if opts.full_times {
                        format!(
                            "- {} {}",
                            format_full_time(logout_time),
                            format_duration(duration_secs)
                        )
                    } else {
                        format!(
                            "- {} {}",
                            format_short_time(logout_time),
                            format_duration(duration_secs)
                        )
                    }
                }
            }
            None => {
                if entry.record_type == BOOT_TIME || entry.record_type == RUN_LVL {
                    // System entries: show as still running.
                    let duration_secs = now.saturating_sub(entry.login_time) as u64;
                    format!("  still running {}", format_duration(duration_secs))
                } else {
                    let duration_secs = now.saturating_sub(entry.login_time) as u64;
                    format!("  still logged in {}", format_duration(duration_secs))
                }
            }
        };

        if opts.host_last {
            // -a: hostname in last column.
            println!(
                "{:<user_width$} {:<12} {} {}   {}",
                user_display, term_display, login_str, status_str, host_display,
                user_width = user_width
            );
        } else if opts.no_host {
            println!(
                "{:<user_width$} {:<12} {} {}",
                user_display, term_display, login_str, status_str,
                user_width = user_width
            );
        } else {
            println!(
                "{:<user_width$} {:<12} {:<host_width$} {} {}",
                user_display, term_display, host_display, login_str, status_str,
                user_width = user_width,
                host_width = host_width
            );
        }
    }
}

/// Print the "wtmp begins" or "btmp begins" footer.
fn print_file_footer(records: &[WtmpRecord], personality: Personality) {
    let label = match personality {
        Personality::Lastb => "btmp",
        _ => "wtmp",
    };
    if let Some(first) = records.first() {
        println!();
        println!("{} begins {}", label, format_full_time(first.time_sec));
    }
}

/// Truncate a string to `max_len` characters.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        s[..max_len].to_string()
    }
}

// ============================================================================
// lastlog output
// ============================================================================

/// Parse lastlog records from binary data.
fn parse_lastlog_records(data: &[u8]) -> Vec<LastlogRecord> {
    let mut records = Vec::new();
    let mut uid: u32 = 0;
    let mut offset = 0;

    while offset + LASTLOG_RECORD_SIZE <= data.len() {
        let time_sec = read_u32_le(data, offset).unwrap_or(0);
        let terminal = extract_string(data, offset + LASTLOG_LINE_OFFSET, LASTLOG_LINE_SIZE)
            .unwrap_or_default();
        let host = extract_string(data, offset + LASTLOG_HOST_OFFSET, LASTLOG_HOST_SIZE)
            .unwrap_or_default();

        records.push(LastlogRecord {
            uid,
            username: String::new(), // Resolved later from /etc/passwd.
            time_sec,
            terminal,
            host,
        });

        uid += 1;
        offset += LASTLOG_RECORD_SIZE;
    }

    records
}

/// Resolve UIDs to usernames using /etc/passwd.
fn resolve_usernames(records: &mut [LastlogRecord]) {
    let passwd = match fs::read_to_string("/etc/passwd") {
        Ok(content) => content,
        Err(_) => {
            // Fall back to numeric UIDs.
            for record in records.iter_mut() {
                record.username = format!("{}", record.uid);
            }
            return;
        }
    };

    let mut uid_map: Vec<(u32, String)> = Vec::new();
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3
            && let Ok(uid) = fields[2].parse::<u32>() {
                uid_map.push((uid, fields[0].to_string()));
            }
    }

    for record in records.iter_mut() {
        if let Some((_, name)) = uid_map.iter().find(|(u, _)| *u == record.uid) {
            record.username = name.clone();
        } else {
            record.username = format!("{}", record.uid);
        }
    }
}

/// Look up a UID by username from /etc/passwd.
fn lookup_uid(username: &str) -> Option<u32> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == username {
            return fields[2].parse().ok();
        }
    }
    None
}

/// Print lastlog entries.
fn print_lastlog(records: &[LastlogRecord], opts: &LastlogOptions) {
    let now = current_epoch_secs();

    println!(
        "{:<16} {:<8} {:<16} Latest",
        "Username", "Port", "From"
    );

    for record in records {
        // Apply user filter.
        if let Some(ref filter) = opts.user_filter
            && record.username != *filter {
                continue;
            }

        // Skip entries with no login time (never logged in).
        if record.time_sec == 0 {
            if opts.user_filter.is_some() {
                println!(
                    "{:<16} {:<8} {:<16} **Never logged in**",
                    record.username, "", ""
                );
            }
            continue;
        }

        // Apply before/time filters.
        let login_age_days = now.saturating_sub(record.time_sec as u64) / 86400;
        if let Some(before) = opts.before_days
            && login_age_days < before {
                continue;
            }
        if let Some(time) = opts.time_days
            && login_age_days > time {
                continue;
            }

        let time_str = format_full_time(record.time_sec);
        println!(
            "{:<16} {:<8} {:<16} {}",
            record.username, record.terminal, record.host, time_str
        );
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parse arguments for `last`/`lastb`.
fn parse_last_args(args: &[String], personality: Personality) -> Result<LastOptions, i32> {
    let default_file = match personality {
        Personality::Lastb => DEFAULT_BTMP,
        _ => DEFAULT_WTMP,
    };
    let mut opts = LastOptions::new(default_file);

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();

        // Check for -NUM shorthand (e.g. -5).
        if arg.starts_with('-') && arg.len() > 1 {
            let maybe_num = &arg[1..];
            if maybe_num.chars().all(|c| c.is_ascii_digit())
                && let Ok(n) = maybe_num.parse::<usize>() {
                    opts.count = n;
                    i += 1;
                    continue;
                }
        }

        match arg {
            "-n" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("last: -n requires an argument");
                    return Err(1);
                }
                match args[i].parse::<usize>() {
                    Ok(n) => opts.count = n,
                    Err(_) => {
                        eprintln!("last: invalid count: {}", args[i]);
                        return Err(1);
                    }
                }
            }
            "-f" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("last: -f requires an argument");
                    return Err(1);
                }
                opts.file = args[i].clone();
            }
            "-R" => opts.no_host = true,
            "-a" => opts.host_last = true,
            "-d" => opts.dns_lookup = true,
            "-F" => opts.full_times = true,
            "-i" => opts.show_ip = true,
            "-x" => opts.show_system = true,
            "-w" => opts.wide = true,
            "-h" | "--help" => {
                print_last_help(personality);
                return Err(0);
            }
            "-V" | "--version" => {
                print_version(personality);
                return Err(0);
            }
            other => {
                if other.starts_with('-') {
                    eprintln!("last: unknown option: {other}");
                    eprintln!("Try 'last --help' for usage.");
                    return Err(1);
                }
                // Positional argument: user or tty filter.
                opts.filters.push(other.to_string());
            }
        }
        i += 1;
    }

    Ok(opts)
}

/// Parse arguments for `lastlog`.
fn parse_lastlog_args(args: &[String]) -> Result<LastlogOptions, i32> {
    let mut opts = LastlogOptions::new();

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-u" | "--user" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lastlog: -u requires an argument");
                    return Err(1);
                }
                opts.user_filter = Some(args[i].clone());
            }
            "-b" | "--before" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lastlog: -b requires an argument");
                    return Err(1);
                }
                match args[i].parse::<u64>() {
                    Ok(d) => opts.before_days = Some(d),
                    Err(_) => {
                        eprintln!("lastlog: invalid days: {}", args[i]);
                        return Err(1);
                    }
                }
            }
            "-t" | "--time" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lastlog: -t requires an argument");
                    return Err(1);
                }
                match args[i].parse::<u64>() {
                    Ok(d) => opts.time_days = Some(d),
                    Err(_) => {
                        eprintln!("lastlog: invalid days: {}", args[i]);
                        return Err(1);
                    }
                }
            }
            "-C" | "--clear" => opts.clear = true,
            "-h" | "--help" => {
                print_lastlog_help();
                return Err(0);
            }
            "-V" | "--version" => {
                print_version(Personality::Lastlog);
                return Err(0);
            }
            other => {
                // Handle --user=VALUE style.
                if let Some(val) = other.strip_prefix("--user=") {
                    opts.user_filter = Some(val.to_string());
                } else if let Some(val) = other.strip_prefix("--before=") {
                    match val.parse::<u64>() {
                        Ok(d) => opts.before_days = Some(d),
                        Err(_) => {
                            eprintln!("lastlog: invalid days: {val}");
                            return Err(1);
                        }
                    }
                } else if let Some(val) = other.strip_prefix("--time=") {
                    match val.parse::<u64>() {
                        Ok(d) => opts.time_days = Some(d),
                        Err(_) => {
                            eprintln!("lastlog: invalid days: {val}");
                            return Err(1);
                        }
                    }
                } else {
                    eprintln!("lastlog: unknown option: {other}");
                    eprintln!("Try 'lastlog --help' for usage.");
                    return Err(1);
                }
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ============================================================================
// Help and version
// ============================================================================

fn print_last_help(personality: Personality) {
    let name = match personality {
        Personality::Last => "last",
        Personality::Lastb => "lastb",
        Personality::Lastlog => "lastlog",
    };
    println!("SlateOS Login Records Viewer v0.1.0");
    println!();
    println!("Show listing of last logged-in users.");
    println!();
    println!("USAGE:");
    println!("  {name} [OPTIONS] [USER|TTY]...");
    println!();
    println!("OPTIONS:");
    println!("  -n NUM, -NUM    Show only last NUM entries");
    println!("  -f FILE         Use FILE instead of default");
    println!("  -R              Don't show hostname");
    println!("  -a              Show hostname in last column");
    println!("  -d              Translate IPs to hostnames");
    println!("  -F              Show full login and logout times");
    println!("  -i              Show IP address instead of hostname");
    println!("  -x              Show shutdown and runlevel entries");
    println!("  -w              Show full user and domain names");
    println!("  -h, --help      Display this help");
    println!("  -V, --version   Display version");
    if personality == Personality::Lastb {
        println!();
        println!("NOTE: lastb reads /var/log/btmp (failed logins),");
        println!("      which requires root privileges.");
    }
}

fn print_lastlog_help() {
    println!("SlateOS Last Login Viewer v0.1.0");
    println!();
    println!("Show last login information for all users.");
    println!();
    println!("USAGE:");
    println!("  lastlog [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("  -u, --user USER    Show only for USER");
    println!("  -b, --before DAYS  Show only entries older than DAYS");
    println!("  -t, --time DAYS    Show only entries newer than DAYS");
    println!("  -C, --clear        Clear last login record for user");
    println!("  -h, --help         Display this help");
    println!("  -V, --version      Display version");
}

fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Last => "last",
        Personality::Lastb => "lastb",
        Personality::Lastlog => "lastlog",
    };
    println!("{name} (SlateOS) 0.1.0");
}

// ============================================================================
// lastlog --clear
// ============================================================================

/// Clear the lastlog entry for a specific user.
fn clear_lastlog_entry(opts: &LastlogOptions) -> i32 {
    let username = match &opts.user_filter {
        Some(u) => u,
        None => {
            eprintln!("lastlog: --clear requires --user");
            return 1;
        }
    };
    let uid = match lookup_uid(username) {
        Some(u) => u,
        None => {
            eprintln!("lastlog: unknown user: {username}");
            return 1;
        }
    };

    let mut data = match fs::read(&opts.file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("lastlog: cannot read {}: {e}", opts.file);
            return 1;
        }
    };

    let offset = (uid as usize) * LASTLOG_RECORD_SIZE;
    let end = offset + LASTLOG_RECORD_SIZE;
    if end > data.len() {
        eprintln!("lastlog: no record for user {username} (UID {uid})");
        return 1;
    }

    // Zero out the record.
    for byte in &mut data[offset..end] {
        *byte = 0;
    }

    if let Err(e) = fs::write(&opts.file, &data) {
        eprintln!("lastlog: cannot write {}: {e}", opts.file);
        return 1;
    }

    println!("lastlog: cleared login record for {username}");
    0
}

// ============================================================================
// Entry points for each personality
// ============================================================================

/// Run the `last` or `lastb` personality.
fn run_last(args: &[String], personality: Personality) -> i32 {
    let opts = match parse_last_args(args, personality) {
        Ok(o) => o,
        Err(code) => return code,
    };

    let data = match fs::read(&opts.file) {
        Ok(d) => d,
        Err(e) => {
            let name = match personality {
                Personality::Lastb => "lastb",
                _ => "last",
            };
            eprintln!("{name}: cannot open {}: {e}", opts.file);
            return 1;
        }
    };

    let records = parse_wtmp_records(&data);
    if records.is_empty() {
        let label = match personality {
            Personality::Lastb => "btmp",
            _ => "wtmp",
        };
        println!();
        println!("{label} begins (empty)");
        return 0;
    }

    let entries = resolve_login_entries(&records, opts.show_system);
    let filtered = filter_entries(&entries, &opts.filters);

    print_last_entries(&filtered, &opts);
    print_file_footer(&records, personality);

    0
}

/// Run the `lastlog` personality.
fn run_lastlog(args: &[String]) -> i32 {
    let opts = match parse_lastlog_args(args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    if opts.clear {
        return clear_lastlog_entry(&opts);
    }

    let data = match fs::read(&opts.file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("lastlog: cannot open {}: {e}", opts.file);
            return 1;
        }
    };

    let mut records = parse_lastlog_records(&data);
    resolve_usernames(&mut records);
    print_lastlog(&records, &opts);

    0
}

// ============================================================================
// Main
// ============================================================================

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();
    let personality = if let Some(argv0) = args.first() {
        detect_personality(argv0)
    } else {
        Personality::Last
    };

    match personality {
        Personality::Last | Personality::Lastb => run_last(&args, personality),
        Personality::Lastlog => run_lastlog(&args),
    }
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

    // --- Personality detection ---

    #[test]
    fn test_personality_last() {
        assert_eq!(detect_personality("last"), Personality::Last);
        assert_eq!(detect_personality("/usr/bin/last"), Personality::Last);
        assert_eq!(detect_personality("C:\\bin\\last.exe"), Personality::Last);
    }

    #[test]
    fn test_personality_lastb() {
        assert_eq!(detect_personality("lastb"), Personality::Lastb);
        assert_eq!(detect_personality("/usr/bin/lastb"), Personality::Lastb);
        assert_eq!(detect_personality("lastb.exe"), Personality::Lastb);
    }

    #[test]
    fn test_personality_lastlog() {
        assert_eq!(detect_personality("lastlog"), Personality::Lastlog);
        assert_eq!(detect_personality("/usr/bin/lastlog"), Personality::Lastlog);
    }

    #[test]
    fn test_personality_unknown_defaults_to_last() {
        assert_eq!(detect_personality("something"), Personality::Last);
        assert_eq!(detect_personality(""), Personality::Last);
    }

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
    fn test_epoch_to_parts_leap_year() {
        // 2024-02-29 12:00:00 UTC = 1709208000
        let (y, m, d, h, _, _) = epoch_to_parts(1_709_208_000);
        assert_eq!((y, m, d), (2024, 2, 29));
        assert_eq!(h, 12);
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2400));
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
    }

    #[test]
    fn test_day_of_week() {
        // Jan 1, 1970 was Thursday (4 in 0=Sun scheme).
        assert_eq!(day_of_week(0), 4);
        // Jan 2, 1970 was Friday.
        assert_eq!(day_of_week(86400), 5);
        // Jan 4, 1970 was Sunday.
        assert_eq!(day_of_week(86400 * 3), 0);
    }

    #[test]
    fn test_dow_name() {
        assert_eq!(dow_name(0), "Sun");
        assert_eq!(dow_name(4), "Thu");
        assert_eq!(dow_name(6), "Sat");
        assert_eq!(dow_name(99), "???");
    }

    #[test]
    fn test_month_name() {
        assert_eq!(month_name(1), "Jan");
        assert_eq!(month_name(6), "Jun");
        assert_eq!(month_name(12), "Dec");
        assert_eq!(month_name(0), "???");
    }

    #[test]
    fn test_format_short_time() {
        // Epoch 0 = Thu Jan 1 00:00 1970
        let s = format_short_time(0);
        assert!(s.starts_with("Thu Jan  1 00:00"), "got: {s}");
    }

    #[test]
    fn test_format_full_time() {
        let s = format_full_time(0);
        assert!(s.contains("1970"), "got: {s}");
        assert!(s.starts_with("Thu Jan  1 00:00:00 1970"), "got: {s}");
    }

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration(3661), "(01:01)");
    }

    #[test]
    fn test_format_duration_with_days() {
        // 1 day + 2 hours + 30 minutes = 95400 seconds.
        assert_eq!(format_duration(95400), "(1+02:30)");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "(00:00)");
    }

    // --- IPv4/IPv6 formatting ---

    #[test]
    fn test_format_ipv4() {
        // 192.168.1.1 in little-endian: 0x0101A8C0
        assert_eq!(format_ipv4(0xC0A80101), "1.1.168.192");
        // 127.0.0.1 stored as LE bytes: bytes [127, 0, 0, 1] -> u32 LE = 0x0100007F
        assert_eq!(format_ipv4(0x0100007F), "127.0.0.1");
    }

    #[test]
    fn test_format_ipv6_empty() {
        assert_eq!(format_ipv6(&[0, 0, 0, 0]), "");
    }

    #[test]
    fn test_format_ipv6_v4_mapped() {
        // Only addr[0] set, rest zero -> IPv4.
        let ip = format_ipv6(&[0x0100007F, 0, 0, 0]);
        assert_eq!(ip, "127.0.0.1");
    }

    #[test]
    fn test_format_ipv6_full() {
        let ip = format_ipv6(&[1, 2, 3, 4]);
        assert!(!ip.is_empty());
        // Should produce 8 colon-separated hex groups.
        assert_eq!(ip.split(':').count(), 8);
    }

    // --- String extraction ---

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
    fn test_extract_string_out_of_bounds() {
        let data = b"hi";
        assert_eq!(extract_string(data, 0, 10), None);
    }

    // --- Binary reading ---

    #[test]
    fn test_read_u16_le() {
        let data: &[u8] = &[0x07, 0x00, 0xFF, 0xFF];
        assert_eq!(read_u16_le(data, 0), Some(7));
        assert_eq!(read_u16_le(data, 2), Some(0xFFFF));
    }

    #[test]
    fn test_read_u32_le() {
        let data: &[u8] = &[0x01, 0x00, 0x00, 0x00];
        assert_eq!(read_u32_le(data, 0), Some(1));
    }

    #[test]
    fn test_read_u32_le_out_of_bounds() {
        let data: &[u8] = &[0x01, 0x02];
        assert_eq!(read_u32_le(data, 0), None);
    }

    // --- wtmp record parsing ---

    #[test]
    fn test_parse_single_wtmp_record() {
        let data = build_wtmp_record_bytes(
            USER_PROCESS, 1234, "pts/0", "s0", "alice", "192.168.1.1", 1700000000, [0x0101A8C0, 0, 0, 0],
        );
        let records = parse_wtmp_records(&data);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_type, USER_PROCESS);
        assert_eq!(records[0].pid, 1234);
        assert_eq!(records[0].terminal, "pts/0");
        assert_eq!(records[0].user, "alice");
        assert_eq!(records[0].host, "192.168.1.1");
        assert_eq!(records[0].time_sec, 1700000000);
    }

    #[test]
    fn test_parse_multiple_wtmp_records() {
        let mut data = build_wtmp_record_bytes(
            USER_PROCESS, 100, "tty1", "t1", "bob", "", 1700000000, [0; 4],
        );
        data.extend(build_wtmp_record_bytes(
            DEAD_PROCESS, 100, "tty1", "t1", "", "", 1700003600, [0; 4],
        ));
        let records = parse_wtmp_records(&data);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].record_type, USER_PROCESS);
        assert_eq!(records[1].record_type, DEAD_PROCESS);
    }

    #[test]
    fn test_parse_empty_data() {
        let records = parse_wtmp_records(&[]);
        assert!(records.is_empty());
    }

    #[test]
    fn test_parse_truncated_data() {
        // Less than one record.
        let data = vec![0u8; 100];
        let records = parse_wtmp_records(&data);
        assert!(records.is_empty());
    }

    #[test]
    fn test_parse_boot_time_record() {
        let data = build_wtmp_record_bytes(
            BOOT_TIME, 0, "~", "~~", "reboot", "", 1700000000, [0; 4],
        );
        let records = parse_wtmp_records(&data);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_type, BOOT_TIME);
        assert_eq!(records[0].user, "reboot");
    }

    // --- Login entry resolution ---

    #[test]
    fn test_resolve_simple_login_logout() {
        let records = vec![
            WtmpRecord {
                record_type: USER_PROCESS, pid: 100, terminal: "tty1".into(),
                id: "t1".into(), user: "alice".into(), host: "".into(),
                exit_status: 0, session: 0, time_sec: 1000, time_usec: 0, addr: [0; 4],
            },
            WtmpRecord {
                record_type: DEAD_PROCESS, pid: 100, terminal: "tty1".into(),
                id: "t1".into(), user: "".into(), host: "".into(),
                exit_status: 0, session: 0, time_sec: 2000, time_usec: 0, addr: [0; 4],
            },
        ];
        let entries = resolve_login_entries(&records, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].user, "alice");
        assert_eq!(entries[0].login_time, 1000);
        assert_eq!(entries[0].logout_time, Some(2000));
    }

    #[test]
    fn test_resolve_still_logged_in() {
        let records = vec![
            WtmpRecord {
                record_type: USER_PROCESS, pid: 200, terminal: "pts/0".into(),
                id: "s0".into(), user: "bob".into(), host: "example.com".into(),
                exit_status: 0, session: 0, time_sec: 5000, time_usec: 0, addr: [0; 4],
            },
        ];
        let entries = resolve_login_entries(&records, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].user, "bob");
        assert!(entries[0].logout_time.is_none());
    }

    #[test]
    fn test_resolve_boot_clears_sessions() {
        let records = vec![
            WtmpRecord {
                record_type: USER_PROCESS, pid: 100, terminal: "tty1".into(),
                id: "t1".into(), user: "alice".into(), host: "".into(),
                exit_status: 0, session: 0, time_sec: 1000, time_usec: 0, addr: [0; 4],
            },
            WtmpRecord {
                record_type: BOOT_TIME, pid: 0, terminal: "~".into(),
                id: "~~".into(), user: "reboot".into(), host: "".into(),
                exit_status: 0, session: 0, time_sec: 3000, time_usec: 0, addr: [0; 4],
            },
        ];
        let entries = resolve_login_entries(&records, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].user, "alice");
        // Boot time should serve as the logout time.
        assert_eq!(entries[0].logout_time, Some(3000));
    }

    #[test]
    fn test_resolve_with_system_entries() {
        let records = vec![
            WtmpRecord {
                record_type: BOOT_TIME, pid: 0, terminal: "~".into(),
                id: "~~".into(), user: "reboot".into(), host: "".into(),
                exit_status: 0, session: 0, time_sec: 1000, time_usec: 0, addr: [0; 4],
            },
            WtmpRecord {
                record_type: RUN_LVL, pid: 3, terminal: "~".into(),
                id: "~~".into(), user: "runlevel".into(), host: "".into(),
                exit_status: 0, session: 0, time_sec: 1001, time_usec: 0, addr: [0; 4],
            },
        ];
        let entries_no_sys = resolve_login_entries(&records, false);
        assert!(entries_no_sys.is_empty());

        let entries_with_sys = resolve_login_entries(&records, true);
        assert_eq!(entries_with_sys.len(), 2);
    }

    // --- Filtering ---

    #[test]
    fn test_filter_by_user() {
        let entries = vec![
            LoginEntry {
                user: "alice".into(), terminal: "tty1".into(), host: "".into(),
                login_time: 1000, logout_time: Some(2000), record_type: USER_PROCESS,
                addr: [0; 4],
            },
            LoginEntry {
                user: "bob".into(), terminal: "pts/0".into(), host: "".into(),
                login_time: 3000, logout_time: None, record_type: USER_PROCESS,
                addr: [0; 4],
            },
        ];
        let filtered = filter_entries(&entries, &["alice".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].user, "alice");
    }

    #[test]
    fn test_filter_by_tty() {
        let entries = vec![
            LoginEntry {
                user: "alice".into(), terminal: "tty1".into(), host: "".into(),
                login_time: 1000, logout_time: None, record_type: USER_PROCESS,
                addr: [0; 4],
            },
            LoginEntry {
                user: "bob".into(), terminal: "pts/0".into(), host: "".into(),
                login_time: 2000, logout_time: None, record_type: USER_PROCESS,
                addr: [0; 4],
            },
        ];
        let filtered = filter_entries(&entries, &["pts/0".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].user, "bob");
    }

    #[test]
    fn test_filter_no_filters_returns_all() {
        let entries = vec![
            LoginEntry {
                user: "alice".into(), terminal: "tty1".into(), host: "".into(),
                login_time: 1000, logout_time: None, record_type: USER_PROCESS,
                addr: [0; 4],
            },
        ];
        let filtered = filter_entries(&entries, &[]);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_no_match() {
        let entries = vec![
            LoginEntry {
                user: "alice".into(), terminal: "tty1".into(), host: "".into(),
                login_time: 1000, logout_time: None, record_type: USER_PROCESS,
                addr: [0; 4],
            },
        ];
        let filtered = filter_entries(&entries, &["nonexistent".to_string()]);
        assert!(filtered.is_empty());
    }

    // --- Argument parsing (last/lastb) ---

    #[test]
    fn test_parse_last_args_default() {
        let args = vec!["last".to_string()];
        let opts = parse_last_args(&args, Personality::Last).unwrap();
        assert_eq!(opts.count, 0);
        assert_eq!(opts.file, DEFAULT_WTMP);
        assert!(!opts.no_host);
        assert!(!opts.host_last);
        assert!(opts.filters.is_empty());
    }

    #[test]
    fn test_parse_last_args_count_n() {
        let args = vec!["last".to_string(), "-n".to_string(), "5".to_string()];
        let opts = parse_last_args(&args, Personality::Last).unwrap();
        assert_eq!(opts.count, 5);
    }

    #[test]
    fn test_parse_last_args_count_shorthand() {
        let args = vec!["last".to_string(), "-10".to_string()];
        let opts = parse_last_args(&args, Personality::Last).unwrap();
        assert_eq!(opts.count, 10);
    }

    #[test]
    fn test_parse_last_args_file() {
        let args = vec!["last".to_string(), "-f".to_string(), "/tmp/wtmp".to_string()];
        let opts = parse_last_args(&args, Personality::Last).unwrap();
        assert_eq!(opts.file, "/tmp/wtmp");
    }

    #[test]
    fn test_parse_last_args_flags() {
        let args = vec![
            "last".to_string(), "-R".to_string(), "-a".to_string(),
            "-F".to_string(), "-i".to_string(), "-x".to_string(),
            "-w".to_string(), "-d".to_string(),
        ];
        let opts = parse_last_args(&args, Personality::Last).unwrap();
        assert!(opts.no_host);
        assert!(opts.host_last);
        assert!(opts.full_times);
        assert!(opts.show_ip);
        assert!(opts.show_system);
        assert!(opts.wide);
        assert!(opts.dns_lookup);
    }

    #[test]
    fn test_parse_last_args_positional_filter() {
        let args = vec!["last".to_string(), "root".to_string(), "pts/0".to_string()];
        let opts = parse_last_args(&args, Personality::Last).unwrap();
        assert_eq!(opts.filters, vec!["root".to_string(), "pts/0".to_string()]);
    }

    #[test]
    fn test_parse_last_args_unknown_flag() {
        let args = vec!["last".to_string(), "--bogus".to_string()];
        let result = parse_last_args(&args, Personality::Last);
        assert_eq!(result.unwrap_err(), 1);
    }

    #[test]
    fn test_parse_last_args_help_returns_zero() {
        let args = vec!["last".to_string(), "--help".to_string()];
        let result = parse_last_args(&args, Personality::Last);
        assert_eq!(result.unwrap_err(), 0);
    }

    #[test]
    fn test_parse_lastb_default_file() {
        let args = vec!["lastb".to_string()];
        let opts = parse_last_args(&args, Personality::Lastb).unwrap();
        assert_eq!(opts.file, DEFAULT_BTMP);
    }

    // --- Argument parsing (lastlog) ---

    #[test]
    fn test_parse_lastlog_args_default() {
        let args = vec!["lastlog".to_string()];
        let opts = parse_lastlog_args(&args).unwrap();
        assert!(opts.user_filter.is_none());
        assert!(opts.before_days.is_none());
        assert!(opts.time_days.is_none());
        assert!(!opts.clear);
    }

    #[test]
    fn test_parse_lastlog_args_user() {
        let args = vec!["lastlog".to_string(), "-u".to_string(), "alice".to_string()];
        let opts = parse_lastlog_args(&args).unwrap();
        assert_eq!(opts.user_filter, Some("alice".to_string()));
    }

    #[test]
    fn test_parse_lastlog_args_user_equals() {
        let args = vec!["lastlog".to_string(), "--user=bob".to_string()];
        let opts = parse_lastlog_args(&args).unwrap();
        assert_eq!(opts.user_filter, Some("bob".to_string()));
    }

    #[test]
    fn test_parse_lastlog_args_before() {
        let args = vec!["lastlog".to_string(), "-b".to_string(), "30".to_string()];
        let opts = parse_lastlog_args(&args).unwrap();
        assert_eq!(opts.before_days, Some(30));
    }

    #[test]
    fn test_parse_lastlog_args_time() {
        let args = vec!["lastlog".to_string(), "-t".to_string(), "7".to_string()];
        let opts = parse_lastlog_args(&args).unwrap();
        assert_eq!(opts.time_days, Some(7));
    }

    #[test]
    fn test_parse_lastlog_args_clear() {
        let args = vec!["lastlog".to_string(), "-C".to_string()];
        let opts = parse_lastlog_args(&args).unwrap();
        assert!(opts.clear);
    }

    #[test]
    fn test_parse_lastlog_args_unknown() {
        let args = vec!["lastlog".to_string(), "--garbage".to_string()];
        let result = parse_lastlog_args(&args);
        assert_eq!(result.unwrap_err(), 1);
    }

    // --- lastlog record parsing ---

    #[test]
    fn test_parse_lastlog_records() {
        // Build two records: UID 0 with login, UID 1 with no login.
        let mut data = vec![0u8; LASTLOG_RECORD_SIZE * 2];
        // UID 0: time=1700000000, line="pts/0", host="myhost"
        let time_bytes = 1_700_000_000u32.to_le_bytes();
        data[0..4].copy_from_slice(&time_bytes);
        let line = b"pts/0";
        data[LASTLOG_LINE_OFFSET..LASTLOG_LINE_OFFSET + line.len()].copy_from_slice(line);
        let host = b"myhost";
        data[LASTLOG_HOST_OFFSET..LASTLOG_HOST_OFFSET + host.len()].copy_from_slice(host);
        // UID 1: all zeros (never logged in).

        let records = parse_lastlog_records(&data);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].uid, 0);
        assert_eq!(records[0].time_sec, 1_700_000_000);
        assert_eq!(records[0].terminal, "pts/0");
        assert_eq!(records[0].host, "myhost");
        assert_eq!(records[1].uid, 1);
        assert_eq!(records[1].time_sec, 0);
    }

    // --- truncate_str ---

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hi", 8), "hi");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("12345678", 8), "12345678");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("longusername", 8), "longuser");
    }

    // --- get_host_display ---

    #[test]
    fn test_host_display_no_host_flag() {
        let entry = LoginEntry {
            user: "alice".into(), terminal: "tty1".into(), host: "myhost".into(),
            login_time: 1000, logout_time: None, record_type: USER_PROCESS,
            addr: [0; 4],
        };
        let mut opts = LastOptions::new(DEFAULT_WTMP);
        opts.no_host = true;
        assert_eq!(get_host_display(&entry, &opts), "");
    }

    #[test]
    fn test_host_display_show_ip() {
        let entry = LoginEntry {
            user: "alice".into(), terminal: "tty1".into(), host: "myhost".into(),
            login_time: 1000, logout_time: None, record_type: USER_PROCESS,
            addr: [0x0100007F, 0, 0, 0],
        };
        let mut opts = LastOptions::new(DEFAULT_WTMP);
        opts.show_ip = true;
        assert_eq!(get_host_display(&entry, &opts), "127.0.0.1");
    }

    #[test]
    fn test_host_display_normal() {
        let entry = LoginEntry {
            user: "alice".into(), terminal: "tty1".into(), host: "remotehost".into(),
            login_time: 1000, logout_time: None, record_type: USER_PROCESS,
            addr: [0; 4],
        };
        let opts = LastOptions::new(DEFAULT_WTMP);
        assert_eq!(get_host_display(&entry, &opts), "remotehost");
    }

    // --- build_wtmp_record_bytes round-trip ---

    #[test]
    fn test_build_and_parse_round_trip() {
        let mut data = Vec::new();
        data.extend(build_wtmp_record_bytes(
            BOOT_TIME, 0, "~", "~~", "reboot", "", 1000, [0; 4],
        ));
        data.extend(build_wtmp_record_bytes(
            USER_PROCESS, 500, "tty1", "t1", "root", "10.0.0.1", 2000, [0x0100000A, 0, 0, 0],
        ));
        data.extend(build_wtmp_record_bytes(
            DEAD_PROCESS, 500, "tty1", "t1", "", "", 3000, [0; 4],
        ));

        let records = parse_wtmp_records(&data);
        assert_eq!(records.len(), 3);

        let entries = resolve_login_entries(&records, true);
        // Should have: boot entry + root login.
        assert_eq!(entries.len(), 2);

        let boot_entry = entries.iter().find(|e| e.record_type == BOOT_TIME).unwrap();
        assert_eq!(boot_entry.user, "reboot");

        let user_entry = entries.iter().find(|e| e.user == "root").unwrap();
        assert_eq!(user_entry.login_time, 2000);
        assert_eq!(user_entry.logout_time, Some(3000));
    }
}
