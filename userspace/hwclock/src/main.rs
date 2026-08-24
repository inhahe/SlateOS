//! hwclock — Hardware clock and time management utility for SlateOS.
//!
//! Reads/writes the hardware RTC via `/proc/rtc` or `/sys/class/rtc/rtc0/`,
//! synchronizes system and hardware clocks, supports timezone offsets,
//! and can query NTP servers for accurate time.

use quoting::quoteaf_os;
use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::net::UdpSocket;
use std::process;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Seconds between NTP epoch (1900-01-01) and Unix epoch (1970-01-01).
const NTP_UNIX_DELTA: u64 = 2_208_988_800;

/// Default NTP server when none is specified.
const DEFAULT_NTP_SERVER: &str = "pool.ntp.org";

/// NTP packet size (simplified SNTPv3).
const NTP_PACKET_LEN: usize = 48;

/// UDP port for NTP.
const NTP_PORT: u16 = 123;

// ---------------------------------------------------------------------------
// Date/Time representation
// ---------------------------------------------------------------------------

/// A simple date-time structure (no external crate dependencies).
#[derive(Clone, Debug, PartialEq, Eq)]
struct DateTime {
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second,
        )
    }
}

// ---------------------------------------------------------------------------
// Date helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `year` is a leap year under the Gregorian calendar.
fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Number of days in a given month (1-based) for a given year.
fn days_in_month(year: u32, month: u32) -> Option<u32> {
    match month {
        1 => Some(31),
        2 => Some(if is_leap_year(year) { 29 } else { 28 }),
        3 => Some(31),
        4 => Some(30),
        5 => Some(31),
        6 => Some(30),
        7 => Some(31),
        8 => Some(31),
        9 => Some(30),
        10 => Some(31),
        11 => Some(30),
        12 => Some(31),
        _ => None,
    }
}

/// Validate that a `DateTime` represents a real calendar date/time.
fn validate_datetime(dt: &DateTime) -> Result<(), String> {
    if dt.month < 1 || dt.month > 12 {
        return Err(format!("month out of range: {}", dt.month));
    }
    let max_day =
        days_in_month(dt.year, dt.month).ok_or_else(|| format!("invalid month: {}", dt.month))?;
    if dt.day < 1 || dt.day > max_day {
        return Err(format!(
            "day out of range for {:04}-{:02}: {} (max {})",
            dt.year, dt.month, dt.day, max_day,
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
    Ok(())
}

/// Convert a `DateTime` (assumed UTC) to Unix timestamp (seconds since
/// 1970-01-01 00:00:00 UTC). Only valid for years >= 1970.
fn datetime_to_unix(dt: &DateTime) -> Result<u64, String> {
    if dt.year < 1970 {
        return Err("year before Unix epoch (1970)".into());
    }
    validate_datetime(dt)?;

    let mut days: u64 = 0;

    // Full years from 1970 up to (but not including) dt.year.
    for y in 1970..dt.year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    // Full months in dt.year up to (but not including) dt.month.
    for m in 1..dt.month {
        days += u64::from(days_in_month(dt.year, m).ok_or_else(|| format!("invalid month: {m}"))?);
    }
    // Days within the month (1-based, so subtract 1).
    days += u64::from(dt.day.saturating_sub(1));

    let secs = days
        .checked_mul(86400)
        .and_then(|s| s.checked_add(u64::from(dt.hour) * 3600))
        .and_then(|s| s.checked_add(u64::from(dt.minute) * 60))
        .and_then(|s| s.checked_add(u64::from(dt.second)))
        .ok_or_else(|| "timestamp overflow".to_string())?;

    Ok(secs)
}

/// Convert a Unix timestamp to a `DateTime` (UTC).
fn unix_to_datetime(mut secs: u64) -> DateTime {
    let second = (secs % 60) as u32;
    secs /= 60;
    let minute = (secs % 60) as u32;
    secs /= 60;
    let hour = (secs % 24) as u32;
    let mut days = secs / 24;

    // Walk years from 1970.
    let mut year: u32 = 1970;
    loop {
        let year_days: u64 = if is_leap_year(year) { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }

    // Walk months within this year.
    let mut month: u32 = 1;
    loop {
        // `days_in_month` will always return Some for month 1..=12.
        let md = days_in_month(year, month).unwrap_or(30);
        if days < u64::from(md) {
            break;
        }
        days -= u64::from(md);
        month += 1;
    }

    let day = days as u32 + 1; // 1-based

    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Apply a signed UTC offset (in seconds) to a Unix timestamp and produce a
/// `DateTime` in the target timezone.
fn unix_to_datetime_offset(secs: u64, offset_secs: i64) -> DateTime {
    let adjusted = if offset_secs >= 0 {
        secs.saturating_add(offset_secs as u64)
    } else {
        secs.saturating_sub(offset_secs.unsigned_abs())
    };
    unix_to_datetime(adjusted)
}

// ---------------------------------------------------------------------------
// Named timezone offsets (hours from UTC)
// ---------------------------------------------------------------------------

/// Map a common timezone abbreviation to the POSIX `TZ` string that *defines*
/// it, so the offset is derived by `tzrules` rather than stored here.
///
/// Every abbreviation below names one fixed offset (`EST` is always UTC-5;
/// `EDT` is the separate name for UTC-4), so these strings carry no DST rule —
/// a user who wants a zone that *switches* passes the full POSIX rule instead
/// (e.g. `EST5EDT,M3.2.0,M11.1.0`), which `parse_tz_offset_secs` accepts
/// directly.
///
/// Note the inverted POSIX sign convention: `EST5` means UTC**-**5.
fn named_tz_posix(name: &str) -> Option<&'static str> {
    // Deliberately small. The full tzdata set belongs in a system zone
    // database, not in a utility's source; what matters is that the entries
    // that *are* here are exact.
    Some(match name.to_ascii_uppercase().as_str() {
        "UTC" | "GMT" | "Z" => "UTC0",
        "EST" => "EST5",
        "EDT" => "EDT4",
        "CST" => "CST6",
        "CDT" => "CDT5",
        "MST" => "MST7",
        "MDT" => "MDT6",
        "PST" => "PST8",
        "PDT" => "PDT7",
        "AKST" => "AKST9",
        "AKDT" => "AKDT8",
        "HST" => "HST10",
        "CET" => "CET-1",
        "CEST" => "CEST-2",
        "EET" => "EET-2",
        "EEST" => "EEST-3",
        "WET" => "WET0",
        "WEST" => "WEST-1",
        // India is +5:30. The old table rounded this to +5 and said so in a
        // comment, which made every Indian `hwclock` reading half an hour
        // early — a comment is not a fix.
        "IST" => "IST-5:30",
        "JST" => "JST-9",
        "KST" => "KST-9",
        "CST_CN" => "<CST>-8", // China Standard Time, distinct from US Central
        "AEST" => "AEST-10",
        "AEDT" => "AEDT-11",
        "ACST" => "ACST-9:30", // also previously rounded, to +9
        "ACDT" => "ACDT-10:30",
        "AWST" => "AWST-8",
        "NZST" => "NZST-12",
        "NZDT" => "NZDT-13",
        _ => return None,
    })
}

/// Parse a user-supplied `--timezone` value into an offset **in seconds** at
/// the given instant.
///
/// Accepts, in order:
///
/// 1. A signed numeric offset in the *user-facing* sign convention, in hours
///    or hours:minutes — `+5`, `-8`, `+5:30`, `-03:30`. `+5` means UTC+5.
/// 2. A known abbreviation from [`named_tz_posix`] — `EST`, `IST`, `NZDT`.
/// 3. Any full POSIX `TZ` string, including one with a DST rule —
///    `EST5EDT,M3.2.0,M11.1.0`, `<+0545>-5:45`.
///
/// Seconds, not hours: half-hour (India, +5:30) and quarter-hour (Nepal,
/// +5:45) zones exist and a whole-hour offset simply cannot express them.
///
/// `at` is the UTC epoch instant the offset is wanted for. It matters for
/// case 3, where the rule may put the zone on either side of a transition.
fn parse_tz_offset_secs(tz: &str, at: i64) -> Result<i32, String> {
    if let Some(secs) = parse_numeric_offset(tz) {
        return Ok(secs);
    }
    let posix = named_tz_posix(tz).unwrap_or(tz);
    tzrules::Tz::parse(posix.as_bytes())
        .map(|rule| rule.lookup(at).gmtoff)
        .ok_or_else(|| format!("unknown timezone: {tz}"))
}

/// Parse `+5`, `-8`, `+5:30`, `-03:30`, `0` into seconds east of UTC.
///
/// Returns `None` if the string is not a bare numeric offset, so the caller
/// can fall through to the named/POSIX paths. A bare `5` (no sign) is accepted
/// as `+5`, matching the old behaviour.
fn parse_numeric_offset(tz: &str) -> Option<i32> {
    let (negative, rest) = match tz.as_bytes().first() {
        Some(b'-') => (true, tz.get(1..)?),
        Some(b'+') => (false, tz.get(1..)?),
        _ => (false, tz),
    };
    let (h_str, m_str) = match rest.split_once(':') {
        Some((h, m)) => (h, m),
        None => (rest, "0"),
    };
    let hours: i32 = h_str.parse().ok()?;
    let minutes: i32 = m_str.parse().ok()?;
    // A minute field is a minute field; `+5:70` is a typo, not 6:10.
    if hours < 0 || !(0..60).contains(&minutes) {
        return None;
    }
    let magnitude = hours
        .checked_mul(3600)?
        .checked_add(minutes.checked_mul(60)?)?;
    Some(if negative { -magnitude } else { magnitude })
}

/// Render an offset in seconds as `UTC+05:30` / `UTC-08:00` / `UTC`.
fn format_offset_label(secs: i32) -> String {
    if secs == 0 {
        return "UTC".to_string();
    }
    let sign = if secs < 0 { '-' } else { '+' };
    let total_minutes = secs.unsigned_abs() / 60;
    let (hours, minutes) = (total_minutes / 60, total_minutes % 60);
    format!("UTC{sign}{hours:02}:{minutes:02}")
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse "YYYY-MM-DD HH:MM:SS" into a `DateTime`.
fn parse_datetime(s: &str) -> Result<DateTime, String> {
    // Accept "YYYY-MM-DD HH:MM:SS" with flexible whitespace.
    let s = s.trim();
    let parts: Vec<&str> = s.splitn(2, [' ', 'T']).collect();
    if parts.len() != 2 {
        return Err(format!("expected 'YYYY-MM-DD HH:MM:SS', got: {s}"));
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    let time_parts: Vec<&str> = parts[1].split(':').collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return Err(format!("expected 'YYYY-MM-DD HH:MM:SS', got: {s}"));
    }
    let year = date_parts[0]
        .parse::<u32>()
        .map_err(|e| format!("bad year: {e}"))?;
    let month = date_parts[1]
        .parse::<u32>()
        .map_err(|e| format!("bad month: {e}"))?;
    let day = date_parts[2]
        .parse::<u32>()
        .map_err(|e| format!("bad day: {e}"))?;
    let hour = time_parts[0]
        .parse::<u32>()
        .map_err(|e| format!("bad hour: {e}"))?;
    let minute = time_parts[1]
        .parse::<u32>()
        .map_err(|e| format!("bad minute: {e}"))?;
    let second = time_parts[2]
        .parse::<u32>()
        .map_err(|e| format!("bad second: {e}"))?;

    let dt = DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    };
    validate_datetime(&dt)?;
    Ok(dt)
}

/// Parse a single unsigned integer from a string, trimming whitespace.
#[allow(dead_code)] // Available for callers parsing individual sysfs values.
fn parse_u32(s: &str) -> Result<u32, String> {
    s.trim()
        .parse::<u32>()
        .map_err(|e| format!("parse error: {e}"))
}

// ---------------------------------------------------------------------------
// RTC reading — /proc/rtc and /sys/class/rtc/rtc0/
// ---------------------------------------------------------------------------

/// Try to read the hardware clock via `/proc/rtc`.
///
/// Expected format (subset):
/// ```text
/// rtc_time    : 14:23:05
/// rtc_date    : 2026-05-17
/// ```
fn read_rtc_proc() -> Result<DateTime, String> {
    let content = fs::read_to_string("/proc/rtc").map_err(|e| format!("/proc/rtc: {e}"))?;

    let mut time_str: Option<&str> = None;
    let mut date_str: Option<&str> = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("rtc_time") {
            let rest = rest.trim_start_matches([' ', ':', '\t']);
            time_str = Some(rest.trim());
        } else if let Some(rest) = line.strip_prefix("rtc_date") {
            let rest = rest.trim_start_matches([' ', ':', '\t']);
            date_str = Some(rest.trim());
        }
    }

    let date = date_str.ok_or("rtc_date not found in /proc/rtc")?;
    let time = time_str.ok_or("rtc_time not found in /proc/rtc")?;

    let combined = format!("{date} {time}");
    parse_datetime(&combined)
}

/// Read a single sysfs RTC attribute file, returning its trimmed content.
fn read_sysfs_attr(attr: &str) -> Result<String, String> {
    let path = format!("/sys/class/rtc/rtc0/{attr}");
    fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("{path}: {e}"))
}

/// Try to read the hardware clock via individual sysfs files under
/// `/sys/class/rtc/rtc0/`.
fn read_rtc_sysfs() -> Result<DateTime, String> {
    // sysfs exposes `date` (YYYY-MM-DD) and `time` (HH:MM:SS).
    let date = read_sysfs_attr("date")?;
    let time = read_sysfs_attr("time")?;
    let combined = format!("{date} {time}");
    parse_datetime(&combined)
}

/// Read the hardware clock, trying `/proc/rtc` first, then sysfs.
fn read_rtc() -> Result<DateTime, String> {
    read_rtc_proc().or_else(|_| read_rtc_sysfs())
}

// ---------------------------------------------------------------------------
// RTC writing — via /sys/class/rtc/rtc0/
// ---------------------------------------------------------------------------

/// Write a `DateTime` to the hardware clock via sysfs.
///
/// This writes the date and time as formatted strings to the wakealarm-style
/// interface or directly to sysfs attribute files. The kernel RTC driver must
/// support writes; otherwise this fails with a permission or I/O error.
fn write_rtc(dt: &DateTime) -> Result<(), String> {
    validate_datetime(dt)?;

    // Some RTC drivers accept writes to /sys/class/rtc/rtc0/date and time,
    // but the more portable interface is /dev/rtc0 with ioctl. Since we do
    // not have libc ioctl wrappers yet, write the formatted timestamp to a
    // kernel-provided control file.
    let timestamp_str = format!("{dt}");

    // Try the combined "set_time" control file first (Slate OS extension).
    let set_path = "/sys/class/rtc/rtc0/set_time";
    if let Ok(mut f) = fs::File::create(set_path) {
        f.write_all(timestamp_str.as_bytes())
            .map_err(|e| format!("{set_path}: {e}"))?;
        return Ok(());
    }

    // Fallback: write date and time separately.
    let date_str = format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day);
    let time_str = format!("{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second);

    let date_path = "/sys/class/rtc/rtc0/date";
    fs::write(date_path, date_str.as_bytes()).map_err(|e| format!("{date_path}: {e}"))?;

    let time_path = "/sys/class/rtc/rtc0/time";
    fs::write(time_path, time_str.as_bytes()).map_err(|e| format!("{time_path}: {e}"))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// System time helpers
// ---------------------------------------------------------------------------

// Native Slate OS clock syscalls (kernel syscall/number.rs is the ABI source of
// truth).  There is NO combined clock_gettime(clock_id, *ts): SYS_CLOCK_REALTIME
// takes no arguments and returns wall-clock nanoseconds-since-epoch in rax, and
// SYS_CLOCK_SETTIME takes the absolute target ns as its only argument.  hwclock
// previously read and wrote the wall clock through the nonexistent, read-only
// /proc/time provider, so it could neither read nor set the system clock.
const SYS_CLOCK_REALTIME: u64 = 14;
const SYS_CLOCK_SETTIME: u64 = 15;
const NS_PER_SEC: i64 = 1_000_000_000;

/// Read the wall clock as nanoseconds since the Unix epoch via
/// `SYS_CLOCK_REALTIME`.
fn read_realtime_ns() -> Result<i64, String> {
    let ret: i64;
    // SAFETY: SYS_CLOCK_REALTIME takes no arguments and writes nothing to
    // userspace; it only reads the kernel clock into rax.  rcx/r11 are
    // clobbered by the SYSCALL instruction per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_CLOCK_REALTIME,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, nomem),
        );
    }
    if ret < 0 {
        return Err(format!("clock_realtime failed with error {ret}"));
    }
    Ok(ret)
}

/// Read the system (kernel) clock as a Unix timestamp (whole seconds).
fn read_system_time() -> Result<u64, String> {
    // Primary: the native kernel wall clock.
    if let Ok(ns) = read_realtime_ns() {
        return Ok((ns / NS_PER_SEC) as u64);
    }

    // Fallback: derive from boot time + uptime if the wall clock is not yet
    // initialised (kernel returns EINVAL until timekeeping is up).
    let uptime_content =
        fs::read_to_string("/proc/uptime").map_err(|e| format!("/proc/uptime: {e}"))?;
    let uptime_secs_str = uptime_content
        .trim()
        .split(|c: char| c.is_whitespace() || c == '.')
        .next()
        .ok_or("empty /proc/uptime")?;
    let uptime_secs: u64 = uptime_secs_str
        .parse()
        .map_err(|e| format!("/proc/uptime parse: {e}"))?;

    // Try to find boot time from /proc/stat (btime field).
    let stat_content = fs::read_to_string("/proc/stat").map_err(|e| format!("/proc/stat: {e}"))?;
    for line in stat_content.lines() {
        if let Some(rest) = line.strip_prefix("btime") {
            let btime: u64 = rest
                .trim()
                .parse()
                .map_err(|e| format!("btime parse: {e}"))?;
            return Ok(btime.saturating_add(uptime_secs));
        }
    }

    Err("could not determine system time: clock_realtime failed and no btime in /proc/stat".into())
}

/// Set the system clock to a given Unix timestamp via `SYS_CLOCK_SETTIME`.
/// Requires `CAP_SYS_TIME`.
fn write_system_time(unix_secs: u64) -> Result<(), String> {
    let target_ns = (unix_secs as i64).saturating_mul(NS_PER_SEC);
    let ret: i64;
    // SAFETY: single scalar argument (absolute target ns); touches no userspace
    // memory.  rcx/r11 are clobbered by SYSCALL per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_CLOCK_SETTIME,
            in("rdi") target_ns as u64,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, nomem),
        );
    }
    if ret < 0 {
        return Err(format!(
            "clock_settime failed with error {ret} (need CAP_SYS_TIME?)"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// NTP (simplified SNTP v3 client)
// ---------------------------------------------------------------------------

/// Query an NTP server and return the current time as a Unix timestamp.
///
/// Sends a minimal 48-byte SNTPv3 client request, reads the server's transmit
/// timestamp from bytes 40..48, and converts from NTP epoch to Unix epoch.
fn ntp_query(server: &str) -> Result<u64, String> {
    let addr = format!("{server}:{NTP_PORT}");

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind UDP: {e}"))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Build SNTP v3 client packet.
    // Byte 0: LI=0 (no warning), VN=3 (version 3), Mode=3 (client)
    //   => 0b00_011_011 = 0x1B
    let mut packet = [0u8; NTP_PACKET_LEN];
    packet[0] = 0x1B;

    socket
        .send_to(&packet, &addr)
        .map_err(|e| format!("send to {addr}: {e}"))?;

    let mut buf = [0u8; NTP_PACKET_LEN];
    let (n, _src) = socket
        .recv_from(&mut buf)
        .map_err(|e| format!("recv from NTP: {e}"))?;

    if n < NTP_PACKET_LEN {
        return Err(format!("NTP response too short: {n} bytes"));
    }

    // Transmit Timestamp: bytes 40..44 are seconds, 44..48 are fraction.
    let ntp_secs = u32::from_be_bytes([buf[40], buf[41], buf[42], buf[43]]);

    if (ntp_secs as u64) < NTP_UNIX_DELTA {
        return Err("NTP timestamp before Unix epoch — server returned invalid data".into());
    }

    Ok(u64::from(ntp_secs) - NTP_UNIX_DELTA)
}

// ---------------------------------------------------------------------------
// Weekday helper (for display)
// ---------------------------------------------------------------------------

/// Compute the day-of-week for a date using Tomohiko Sakamoto's algorithm.
/// Returns 0=Sunday, 1=Monday, ... 6=Saturday.
fn day_of_week(year: u32, month: u32, day: u32) -> u32 {
    static T: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let idx = (month as usize).saturating_sub(1).min(11);
    (y + y / 4 - y / 100 + y / 400 + T[idx] + day) % 7
}

/// Short English weekday name.
fn weekday_name(dow: u32) -> &'static str {
    match dow % 7 {
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

// ---------------------------------------------------------------------------
// Pretty printing
// ---------------------------------------------------------------------------

/// Format a `DateTime` with weekday and timezone label for display.
fn format_display(dt: &DateTime, tz_label: &str) -> String {
    let dow = day_of_week(dt.year, dt.month, dt.day);
    format!("{} {} {tz_label}", weekday_name(dow), dt)
}

// ---------------------------------------------------------------------------
// Command-line parsing
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Action {
    Show,
    HcToSys,
    SysToHc,
    Set { date: String },
    Ntp { server: String },
}

#[derive(Debug)]
struct Options {
    action: Action,
    utc: bool,
    #[allow(dead_code)]
    localtime: bool,
    timezone: Option<String>,
}

fn print_usage() {
    let usage = "\
hwclock - hardware clock management

Usage:
  hwclock [OPTIONS]

Options:
  --show           Show the hardware clock time (default)
  --hctosys        Set system time from hardware clock
  --systohc        Set hardware clock from system time
  --set --date \"YYYY-MM-DD HH:MM:SS\"
                   Set hardware clock to a specific time
  --utc            Interpret RTC as UTC (default)
  --localtime      Interpret RTC as local time
  --timezone <tz>  Display time for a timezone (e.g. EST, +5, PST)
  --ntp [server]   Query NTP server for current time
  --help           Show this help message

Timezone may be a named abbreviation (UTC, EST, PST, CET, JST, ...)
or a signed integer UTC offset in hours (e.g. +5, -8).

Examples:
  hwclock                         Show hardware clock in UTC
  hwclock --show --timezone PST   Show hardware clock in PST
  hwclock --hctosys               Sync system clock from RTC
  hwclock --systohc               Sync RTC from system clock
  hwclock --set --date \"2026-05-17 14:30:00\"
  hwclock --ntp pool.ntp.org      Show NTP time";
    println!("{usage}");
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().collect();

    let mut action: Option<Action> = None;
    let mut utc = true;
    let mut localtime = false;
    let mut timezone: Option<String> = None;
    let mut date_value: Option<String> = None;
    let mut expect_date = false;
    let mut expect_tz = false;
    let mut expect_ntp_server = false;

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();

        if expect_date {
            date_value = Some(arg.to_string());
            expect_date = false;
            i += 1;
            continue;
        }
        if expect_tz {
            timezone = Some(arg.to_string());
            expect_tz = false;
            i += 1;
            continue;
        }
        if expect_ntp_server {
            // The server argument is optional for --ntp; if the next token
            // looks like a flag, treat it as the next option instead.
            if !arg.starts_with('-') {
                action = Some(Action::Ntp {
                    server: arg.to_string(),
                });
                expect_ntp_server = false;
                i += 1;
                continue;
            }
            // Not a server name — fall through to normal flag processing.
            // Action was already set with the default server.
            expect_ntp_server = false;
        }

        match arg {
            "--help" | "-h" => {
                print_usage();
                process::exit(0);
            }
            "--show" | "-r" => {
                action = Some(Action::Show);
            }
            "--hctosys" => {
                action = Some(Action::HcToSys);
            }
            "--systohc" => {
                action = Some(Action::SysToHc);
            }
            "--set" => {
                // The actual date comes via --date; mark that we are in "set" mode.
                if action.is_none() {
                    action = Some(Action::Set {
                        date: String::new(),
                    });
                }
            }
            "--date" => {
                expect_date = true;
            }
            "--utc" | "-u" => {
                utc = true;
                localtime = false;
            }
            "--localtime" => {
                localtime = true;
                utc = false;
            }
            "--timezone" | "--tz" => {
                expect_tz = true;
            }
            "--ntp" => {
                action = Some(Action::Ntp {
                    server: DEFAULT_NTP_SERVER.to_string(),
                });
                expect_ntp_server = true;
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
        i += 1;
    }

    // If --ntp was the last argument, the default server is already stored.
    // Just clear the flag.
    let _ = expect_ntp_server;

    // Resolve --set + --date into the Set action.
    if let Some(Action::Set { .. }) = &action {
        let d = date_value.ok_or("--set requires --date \"YYYY-MM-DD HH:MM:SS\"")?;
        action = Some(Action::Set { date: d });
    } else if date_value.is_some() {
        // --date without --set: treat as --set.
        let d = date_value.ok_or("internal error")?;
        action = Some(Action::Set { date: d });
    }

    let action = action.unwrap_or(Action::Show);

    Ok(Options {
        action,
        utc,
        localtime,
        timezone,
    })
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn cmd_show(opts: &Options) -> Result<(), String> {
    let rtc = read_rtc()?;

    // The RTC reads UTC, so it is the instant the zone rule must be evaluated
    // at — resolve the zone *after* we know what time it is, not before.
    let unix = datetime_to_unix(&rtc)?;
    let (tz_label, offset_secs) = resolve_tz(opts, i64::try_from(unix).unwrap_or(i64::MAX));

    // The RTC value is in UTC (unless --localtime, which we treat as
    // already adjusted). Convert to the display timezone.
    let display = if opts.utc {
        unix_to_datetime_offset(unix, i64::from(offset_secs))
    } else {
        // localtime mode: the RTC already represents wall-clock time in the
        // local timezone. For --timezone display, convert from local to UTC
        // first, then to the target timezone.
        if offset_secs == 0 {
            rtc.clone()
        } else {
            // Assume local = system default (offset 0 if unknown).
            unix_to_datetime_offset(unix, i64::from(offset_secs))
        }
    };

    println!("{}", format_display(&display, &tz_label));
    Ok(())
}

fn cmd_hctosys(_opts: &Options) -> Result<(), String> {
    let rtc = read_rtc()?;
    let unix = datetime_to_unix(&rtc)?;
    write_system_time(unix)?;
    println!("System time set from hardware clock: {rtc} UTC");
    Ok(())
}

fn cmd_systohc(_opts: &Options) -> Result<(), String> {
    let unix = read_system_time()?;
    let dt = unix_to_datetime(unix);
    write_rtc(&dt)?;
    println!("Hardware clock set from system time: {dt} UTC");
    Ok(())
}

fn cmd_set(date_str: &str, _opts: &Options) -> Result<(), String> {
    let dt = parse_datetime(date_str)?;
    write_rtc(&dt)?;
    println!("Hardware clock set to: {dt}");
    Ok(())
}

fn cmd_ntp(server: &str, opts: &Options) -> Result<(), String> {
    println!("Querying NTP server: {server} ...");
    let unix = ntp_query(server)?;
    let utc_dt = unix_to_datetime(unix);

    let (tz_label, offset_secs) = resolve_tz(opts, i64::try_from(unix).unwrap_or(i64::MAX));
    let display = unix_to_datetime_offset(unix, i64::from(offset_secs));

    println!("NTP time: {}", format_display(&display, &tz_label));

    // Also show UTC if a non-UTC timezone was requested.
    if offset_secs != 0 {
        println!("     UTC: {}", format_display(&utc_dt, "UTC"));
    }

    Ok(())
}

/// Determine the timezone label and offset **in seconds** for display.
///
/// `at` is the UTC epoch instant being displayed: a zone given as a full POSIX
/// rule is on one side or the other of a transition depending on it, so the
/// offset cannot be resolved without knowing *when*.
fn resolve_tz(opts: &Options, at: i64) -> (String, i32) {
    let Some(ref tz) = opts.timezone else {
        return if opts.utc {
            ("UTC".to_string(), 0)
        } else {
            ("localtime".to_string(), 0)
        };
    };
    match parse_tz_offset_secs(tz, at) {
        Ok(secs) => {
            // Prefer the abbreviation the *rule* reports at this instant over
            // whatever the user typed: passing `EST5EDT,M3.2.0,M11.1.0` in
            // July should print EDT, not the whole rule string.
            let label = if parse_numeric_offset(tz).is_some() {
                format_offset_label(secs)
            } else {
                let posix = named_tz_posix(tz).unwrap_or(tz);
                tzrules::Tz::parse(posix.as_bytes()).map_or_else(
                    || format_offset_label(secs),
                    |rule| String::from_utf8_lossy(rule.lookup(at).name.as_bytes()).into_owned(),
                )
            };
            (label, secs)
        }
        Err(_) => {
            eprintln!("warning: unknown timezone {}, using UTC", quoteaf_os(tz));
            ("UTC".to_string(), 0)
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("hwclock: {e}");
            eprintln!("Try 'hwclock --help' for more information.");
            process::exit(1);
        }
    };

    let result = match &opts.action {
        Action::Show => cmd_show(&opts),
        Action::HcToSys => cmd_hctosys(&opts),
        Action::SysToHc => cmd_systohc(&opts),
        Action::Set { date } => cmd_set(date, &opts),
        Action::Ntp { server } => cmd_ntp(server, &opts),
    };

    if let Err(e) = result {
        eprintln!("hwclock: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Leap year ----------------------------------------------------------

    #[test]
    fn test_leap_year_common() {
        assert!(!is_leap_year(2023));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2100));
    }

    #[test]
    fn test_leap_year_leap() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(is_leap_year(1600));
    }

    // -- Days in month ------------------------------------------------------

    #[test]
    fn test_days_in_month_feb() {
        assert_eq!(days_in_month(2024, 2), Some(29));
        assert_eq!(days_in_month(2023, 2), Some(28));
    }

    #[test]
    fn test_days_in_month_all() {
        let expected = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (i, &e) in expected.iter().enumerate() {
            assert_eq!(days_in_month(2023, i as u32 + 1), Some(e));
        }
    }

    #[test]
    fn test_days_in_month_invalid() {
        assert_eq!(days_in_month(2023, 0), None);
        assert_eq!(days_in_month(2023, 13), None);
    }

    // -- DateTime validation ------------------------------------------------

    #[test]
    fn test_validate_good() {
        let dt = DateTime {
            year: 2026,
            month: 5,
            day: 17,
            hour: 14,
            minute: 30,
            second: 0,
        };
        assert!(validate_datetime(&dt).is_ok());
    }

    #[test]
    fn test_validate_bad_month() {
        let dt = DateTime {
            year: 2026,
            month: 13,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert!(validate_datetime(&dt).is_err());
    }

    #[test]
    fn test_validate_bad_day() {
        let dt = DateTime {
            year: 2023,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert!(validate_datetime(&dt).is_err());
    }

    #[test]
    fn test_validate_bad_hour() {
        let dt = DateTime {
            year: 2026,
            month: 1,
            day: 1,
            hour: 24,
            minute: 0,
            second: 0,
        };
        assert!(validate_datetime(&dt).is_err());
    }

    // -- Unix timestamp conversion ------------------------------------------

    #[test]
    fn test_unix_epoch() {
        let dt = DateTime {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(datetime_to_unix(&dt).unwrap(), 0);
    }

    #[test]
    fn test_known_timestamp() {
        // 2026-05-17 00:00:00 UTC
        let dt = DateTime {
            year: 2026,
            month: 5,
            day: 17,
            hour: 0,
            minute: 0,
            second: 0,
        };
        // Manually computed: days from 1970-01-01 to 2026-05-17
        // 56 full years (1970..2025) then Jan-Apr 2026 + 17 days in May - 1
        let ts = datetime_to_unix(&dt).unwrap();
        // Verify round-trip.
        let rt = unix_to_datetime(ts);
        assert_eq!(rt, dt);
    }

    #[test]
    fn test_roundtrip_various() {
        let cases = [
            DateTime {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
            },
            DateTime {
                year: 2000,
                month: 2,
                day: 29,
                hour: 23,
                minute: 59,
                second: 59,
            },
            DateTime {
                year: 2024,
                month: 12,
                day: 31,
                hour: 12,
                minute: 0,
                second: 0,
            },
            DateTime {
                year: 2038,
                month: 1,
                day: 19,
                hour: 3,
                minute: 14,
                second: 7,
            },
        ];
        for dt in &cases {
            let ts = datetime_to_unix(dt).unwrap();
            let rt = unix_to_datetime(ts);
            assert_eq!(&rt, dt, "round-trip failed for {dt}");
        }
    }

    #[test]
    fn test_before_epoch() {
        let dt = DateTime {
            year: 1969,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
        };
        assert!(datetime_to_unix(&dt).is_err());
    }

    // -- Timezone offsets ---------------------------------------------------

    #[test]
    fn test_offset_positive() {
        let dt = unix_to_datetime_offset(0, 3600);
        assert_eq!(dt.hour, 1);
    }

    #[test]
    fn test_offset_negative() {
        // 1970-01-02 00:00:00 UTC - 5 hours = 1970-01-01 19:00:00
        let dt = unix_to_datetime_offset(86400, -5 * 3600);
        assert_eq!(dt.hour, 19);
        assert_eq!(dt.day, 1);
    }

    // -- Named timezones ----------------------------------------------------

    /// 2024-07-15 12:00 UTC — northern summer, southern winter.
    const NOON_JUL: i64 = 1_721_044_800;
    /// 2024-01-15 12:00 UTC — the other side of every transition.
    const NOON_JAN: i64 = 1_705_320_000;

    #[test]
    fn test_known_tz() {
        assert_eq!(parse_tz_offset_secs("UTC", NOON_JUL).unwrap(), 0);
        assert_eq!(parse_tz_offset_secs("EST", NOON_JUL).unwrap(), -5 * 3600);
        assert_eq!(parse_tz_offset_secs("JST", NOON_JUL).unwrap(), 9 * 3600);
    }

    #[test]
    fn test_unknown_tz() {
        assert!(named_tz_posix("XYZZY").is_none());
        assert!(parse_tz_offset_secs("XYZZY", NOON_JUL).is_err());
    }

    #[test]
    fn test_parse_tz_numeric() {
        assert_eq!(parse_tz_offset_secs("+5", NOON_JUL).unwrap(), 5 * 3600);
        assert_eq!(parse_tz_offset_secs("-8", NOON_JUL).unwrap(), -8 * 3600);
        assert_eq!(parse_tz_offset_secs("0", NOON_JUL).unwrap(), 0);
    }

    #[test]
    fn test_parse_tz_named() {
        assert_eq!(parse_tz_offset_secs("PST", NOON_JUL).unwrap(), -8 * 3600);
        assert_eq!(parse_tz_offset_secs("cet", NOON_JUL).unwrap(), 3600);
    }

    #[test]
    fn test_a_half_hour_zone_is_not_rounded_to_the_hour() {
        // The whole point of moving off `offset_hours`. India is +5:30, and the
        // old table stored +5 with a comment admitting it — half an hour wrong
        // for every reading.
        assert_eq!(
            parse_tz_offset_secs("IST", NOON_JUL).unwrap(),
            5 * 3600 + 1800
        );
        assert_eq!(
            parse_tz_offset_secs("ACST", NOON_JUL).unwrap(),
            9 * 3600 + 1800
        );
    }

    #[test]
    fn test_a_numeric_offset_may_carry_minutes() {
        assert_eq!(parse_numeric_offset("+5:30"), Some(5 * 3600 + 1800));
        assert_eq!(parse_numeric_offset("-03:30"), Some(-(3 * 3600 + 1800)));
        assert_eq!(parse_numeric_offset("+5:45"), Some(5 * 3600 + 2700));
    }

    #[test]
    fn test_a_bad_minute_field_is_not_silently_carried() {
        // `+5:70` is a typo. Accepting it as 6:10 would invent a zone.
        assert_eq!(parse_numeric_offset("+5:70"), None);
        assert_eq!(parse_numeric_offset("+5:xx"), None);
        assert_eq!(parse_numeric_offset("EST"), None);
    }

    #[test]
    fn test_a_full_posix_rule_is_evaluated_at_the_instant() {
        // US Eastern with its real DST rule reads EDT in July and EST in
        // January — the same string, two different offsets.
        let eastern = "EST5EDT,M3.2.0,M11.1.0";
        assert_eq!(parse_tz_offset_secs(eastern, NOON_JUL).unwrap(), -4 * 3600);
        assert_eq!(parse_tz_offset_secs(eastern, NOON_JAN).unwrap(), -5 * 3600);
    }

    #[test]
    fn test_a_quarter_hour_zone_survives() {
        // Nepal, +5:45, expressible only as a POSIX angle-bracket name.
        assert_eq!(
            parse_tz_offset_secs("<+0545>-5:45", NOON_JUL).unwrap(),
            5 * 3600 + 2700
        );
    }

    #[test]
    fn test_every_named_abbreviation_parses() {
        // A typo in the table above is a typo, and the right place to catch a
        // typo is a failing test rather than a wrong clock on a running system.
        for name in [
            "UTC", "GMT", "Z", "EST", "EDT", "CST", "CDT", "MST", "MDT", "PST", "PDT", "AKST",
            "AKDT", "HST", "CET", "CEST", "EET", "EEST", "WET", "WEST", "IST", "JST", "KST",
            "CST_CN", "AEST", "AEDT", "ACST", "ACDT", "AWST", "NZST", "NZDT",
        ] {
            let posix =
                named_tz_posix(name).unwrap_or_else(|| panic!("{name} missing from the table"));
            let rule = tzrules::Tz::parse(posix.as_bytes())
                .unwrap_or_else(|| panic!("{name} → {posix:?} does not parse"));
            // These are fixed-offset names by construction; none may shift.
            assert_eq!(
                rule.lookup(NOON_JUL).gmtoff,
                rule.lookup(NOON_JAN).gmtoff,
                "{name} should be a fixed offset"
            );
        }
    }

    #[test]
    fn test_offset_label_formatting() {
        assert_eq!(format_offset_label(0), "UTC");
        assert_eq!(format_offset_label(5 * 3600 + 1800), "UTC+05:30");
        assert_eq!(format_offset_label(-8 * 3600), "UTC-08:00");
        assert_eq!(format_offset_label(-(3 * 3600 + 1800)), "UTC-03:30");
    }

    // -- DateTime parsing ---------------------------------------------------

    #[test]
    fn test_parse_datetime_valid() {
        let dt = parse_datetime("2026-05-17 14:30:00").unwrap();
        assert_eq!(dt.year, 2026);
        assert_eq!(dt.month, 5);
        assert_eq!(dt.day, 17);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
        assert_eq!(dt.second, 0);
    }

    #[test]
    fn test_parse_datetime_t_separator() {
        let dt = parse_datetime("2026-05-17T14:30:00").unwrap();
        assert_eq!(dt.hour, 14);
    }

    #[test]
    fn test_parse_datetime_invalid_format() {
        assert!(parse_datetime("not-a-date").is_err());
        assert!(parse_datetime("2026-13-01 00:00:00").is_err());
        assert!(parse_datetime("2023-02-29 00:00:00").is_err());
    }

    // -- Day of week --------------------------------------------------------

    #[test]
    fn test_day_of_week_known() {
        // 2026-05-17 is a Sunday.
        assert_eq!(day_of_week(2026, 5, 17), 0);
        // 2024-01-01 is a Monday.
        assert_eq!(day_of_week(2024, 1, 1), 1);
    }

    // -- Display formatting -------------------------------------------------

    #[test]
    fn test_format_display() {
        let dt = DateTime {
            year: 2026,
            month: 5,
            day: 17,
            hour: 14,
            minute: 30,
            second: 0,
        };
        let s = format_display(&dt, "UTC");
        assert!(s.contains("Sun"));
        assert!(s.contains("2026-05-17"));
        assert!(s.contains("14:30:00"));
        assert!(s.contains("UTC"));
    }

    // -- DateTime Display trait ---------------------------------------------

    #[test]
    fn test_datetime_display() {
        let dt = DateTime {
            year: 2026,
            month: 1,
            day: 5,
            hour: 3,
            minute: 7,
            second: 9,
        };
        assert_eq!(format!("{dt}"), "2026-01-05 03:07:09");
    }
}
