//! Timezone and system clock configuration.
//!
//! Manages timezone selection, NTP synchronisation settings, date/time
//! format preferences, and GPS-based timezone detection.
//!
//! ## Design Reference
//!
//! design.txt line 1354: "timezone (try to detect by gps)"
//! design.txt line 711: "clock" on taskbar
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Date & Time
//!   → timezone::set_timezone("America/New_York")
//!   → timezone::set_ntp(true)
//!   → timezone::set_format(TimeFormat::H24)
//!
//! Taskbar clock widget
//!   → timezone::current_time() → formatted string
//!   → timezone::timezone_info() → TzInfo
//!
//! GPS subsystem
//!   → timezone::detect_from_location(lat, lon)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Time display format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeFormat {
    /// 12-hour with AM/PM.
    H12,
    /// 24-hour (default).
    H24,
}

/// Date display format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateFormat {
    /// YYYY-MM-DD (ISO 8601, default).
    Iso,
    /// MM/DD/YYYY (US).
    Mdy,
    /// DD/MM/YYYY (European).
    Dmy,
    /// DD.MM.YYYY (German/Central European).
    DmyDot,
}

/// Day the week starts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekStart {
    Sunday,
    Monday,
    Saturday,
}

/// NTP synchronisation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtpStatus {
    Disabled,
    Syncing,
    Synced,
    Failed,
}

/// Timezone information.
#[derive(Debug, Clone)]
pub struct TzInfo {
    /// IANA timezone name (e.g., "America/New_York").
    pub name: String,
    /// Display label (e.g., "Eastern Time (US & Canada)").
    pub display_name: String,
    /// UTC offset in minutes (e.g., -300 for UTC-5).
    pub utc_offset_min: i32,
    /// Whether DST is currently active.
    pub dst_active: bool,
    /// DST abbreviation (e.g., "EDT") or standard (e.g., "EST").
    pub abbreviation: String,
}

/// Registered timezone entry in the database.
#[derive(Debug, Clone)]
pub struct TzEntry {
    /// IANA timezone name.
    pub name: String,
    /// Display label.
    pub display_name: String,
    /// Standard UTC offset in minutes.
    pub std_offset_min: i32,
    /// DST UTC offset in minutes (same as std if no DST).
    pub dst_offset_min: i32,
    /// Standard abbreviation (e.g., "EST").
    pub std_abbrev: String,
    /// DST abbreviation (e.g., "EDT").
    pub dst_abbrev: String,
    /// Region for grouping (e.g., "Americas", "Europe").
    pub region: String,
    /// Approximate latitude (for GPS matching).
    pub lat: f32,
    /// Approximate longitude.
    pub lon: f32,
}

/// NTP server configuration.
#[derive(Debug, Clone)]
pub struct NtpServer {
    pub hostname: String,
    pub port: u16,
    pub enabled: bool,
    pub last_sync_ns: u64,
    pub offset_us: i64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    /// Current timezone name.
    current_tz: String,
    /// Timezone database.
    tz_database: Vec<TzEntry>,
    /// NTP servers.
    ntp_servers: Vec<NtpServer>,
    /// NTP enabled.
    ntp_enabled: bool,
    /// NTP status.
    ntp_status: NtpStatus,
    /// Time format.
    time_format: TimeFormat,
    /// Date format.
    date_format: DateFormat,
    /// Week start.
    week_start: WeekStart,
    /// Show seconds in clock.
    show_seconds: bool,
    /// Show date in taskbar clock.
    show_date: bool,
    /// Manual time offset applied (ns, for when NTP is off).
    manual_offset_ns: i64,
    /// Last GPS detection result.
    last_gps_tz: String,
    /// Last GPS detection coordinates.
    last_gps_lat: f32,
    last_gps_lon: f32,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    current_tz: String::new(), // set in init_defaults
    tz_database: Vec::new(),
    ntp_servers: Vec::new(),
    ntp_enabled: true,
    ntp_status: NtpStatus::Disabled,
    time_format: TimeFormat::H24,
    date_format: DateFormat::Iso,
    week_start: WeekStart::Monday,
    show_seconds: false,
    show_date: true,
    manual_offset_ns: 0,
    last_gps_tz: String::new(),
    last_gps_lat: 0.0,
    last_gps_lon: 0.0,
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Timezone selection
// ---------------------------------------------------------------------------

/// Set the current timezone by IANA name.
pub fn set_timezone(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Verify timezone exists in database.
    if !state.tz_database.is_empty() && !state.tz_database.iter().any(|t| t.name == name) {
        return Err(KernelError::NotFound);
    }
    state.current_tz = String::from(name);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get current timezone name.
pub fn current_timezone() -> String {
    STATE.lock().current_tz.clone()
}

/// Get full timezone info for the current timezone.
pub fn timezone_info() -> KernelResult<TzInfo> {
    let state = STATE.lock();
    let entry = state.tz_database.iter().find(|t| t.name == state.current_tz)
        .ok_or(KernelError::NotFound)?;
    // Simplified: DST detection would need actual date logic.
    Ok(TzInfo {
        name: entry.name.clone(),
        display_name: entry.display_name.clone(),
        utc_offset_min: entry.std_offset_min,
        dst_active: false,
        abbreviation: entry.std_abbrev.clone(),
    })
}

/// List all timezones, optionally filtered by region.
pub fn list_timezones(region_filter: &str) -> Vec<TzEntry> {
    let state = STATE.lock();
    if region_filter.is_empty() {
        state.tz_database.clone()
    } else {
        state.tz_database.iter()
            .filter(|t| t.region == region_filter)
            .cloned()
            .collect()
    }
}

/// List unique regions.
pub fn list_regions() -> Vec<String> {
    let state = STATE.lock();
    let mut regions: Vec<String> = Vec::new();
    for tz in &state.tz_database {
        if !regions.contains(&tz.region) {
            regions.push(tz.region.clone());
        }
    }
    regions
}

/// Detect timezone from GPS coordinates (nearest match).
pub fn detect_from_location(lat: f32, lon: f32) -> KernelResult<String> {
    let mut state = STATE.lock();
    state.last_gps_lat = lat;
    state.last_gps_lon = lon;

    // Find nearest timezone by simple Euclidean distance (approximation).
    let mut best_name = String::new();
    let mut best_dist = f32::MAX;
    for tz in &state.tz_database {
        let dlat = tz.lat - lat;
        let dlon = tz.lon - lon;
        let dist = dlat * dlat + dlon * dlon;
        if dist < best_dist {
            best_dist = dist;
            best_name = tz.name.clone();
        }
    }

    if best_name.is_empty() {
        return Err(KernelError::NotFound);
    }

    state.last_gps_tz = best_name.clone();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(best_name)
}

// ---------------------------------------------------------------------------
// NTP configuration
// ---------------------------------------------------------------------------

/// Enable or disable NTP synchronisation.
pub fn set_ntp_enabled(enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.ntp_enabled = enabled;
    state.ntp_status = if enabled { NtpStatus::Syncing } else { NtpStatus::Disabled };
    state.changes += 1;
    Ok(())
}

/// Get NTP status.
pub fn ntp_status() -> NtpStatus {
    STATE.lock().ntp_status
}

/// Add an NTP server.
pub fn add_ntp_server(hostname: &str, port: u16) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.ntp_servers.len() >= 16 {
        return Err(KernelError::ResourceExhausted);
    }
    if state.ntp_servers.iter().any(|s| s.hostname == hostname) {
        return Err(KernelError::AlreadyExists);
    }
    state.ntp_servers.push(NtpServer {
        hostname: String::from(hostname),
        port,
        enabled: true,
        last_sync_ns: 0,
        offset_us: 0,
    });
    state.changes += 1;
    Ok(())
}

/// Remove an NTP server.
pub fn remove_ntp_server(hostname: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let before = state.ntp_servers.len();
    state.ntp_servers.retain(|s| s.hostname != hostname);
    if state.ntp_servers.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    Ok(())
}

/// List NTP servers.
pub fn list_ntp_servers() -> Vec<NtpServer> {
    STATE.lock().ntp_servers.clone()
}

/// Simulate NTP sync (record an offset).
pub fn simulate_sync(hostname: &str, offset_us: i64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let server = state.ntp_servers.iter_mut().find(|s| s.hostname == hostname)
        .ok_or(KernelError::NotFound)?;
    server.last_sync_ns = crate::hpet::elapsed_ns();
    server.offset_us = offset_us;
    state.ntp_status = NtpStatus::Synced;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Display preferences
// ---------------------------------------------------------------------------

/// Set time display format.
pub fn set_time_format(fmt: TimeFormat) {
    let mut state = STATE.lock();
    state.time_format = fmt;
    state.changes += 1;
}

/// Set date display format.
pub fn set_date_format(fmt: DateFormat) {
    let mut state = STATE.lock();
    state.date_format = fmt;
    state.changes += 1;
}

/// Set first day of the week.
pub fn set_week_start(day: WeekStart) {
    let mut state = STATE.lock();
    state.week_start = day;
    state.changes += 1;
}

/// Set whether seconds are shown in the clock.
pub fn set_show_seconds(show: bool) {
    let mut state = STATE.lock();
    state.show_seconds = show;
    state.changes += 1;
}

/// Set whether date is shown in the taskbar clock.
pub fn set_show_date(show: bool) {
    let mut state = STATE.lock();
    state.show_date = show;
    state.changes += 1;
}

/// Get current format settings.
pub fn format_settings() -> (TimeFormat, DateFormat, WeekStart, bool, bool) {
    let state = STATE.lock();
    (state.time_format, state.date_format, state.week_start, state.show_seconds, state.show_date)
}

/// Set manual time offset (when NTP is disabled).
pub fn set_manual_offset(offset_ns: i64) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.manual_offset_ns = offset_ns;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

fn add_tz(db: &mut Vec<TzEntry>, name: &str, display: &str, std_off: i32, dst_off: i32,
    std_ab: &str, dst_ab: &str, region: &str, lat: f32, lon: f32)
{
    db.push(TzEntry {
        name: String::from(name),
        display_name: String::from(display),
        std_offset_min: std_off,
        dst_offset_min: dst_off,
        std_abbrev: String::from(std_ab),
        dst_abbrev: String::from(dst_ab),
        region: String::from(region),
        lat,
        lon,
    });
}

/// Initialise with common timezone database and NTP servers.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.tz_database.is_empty() {
        return;
    }

    // Populate timezone database with common entries.
    let db = &mut state.tz_database;
    add_tz(db, "America/New_York", "Eastern Time (US)", -300, -240, "EST", "EDT", "Americas", 40.7, -74.0);
    add_tz(db, "America/Chicago", "Central Time (US)", -360, -300, "CST", "CDT", "Americas", 41.9, -87.6);
    add_tz(db, "America/Denver", "Mountain Time (US)", -420, -360, "MST", "MDT", "Americas", 39.7, -105.0);
    add_tz(db, "America/Los_Angeles", "Pacific Time (US)", -480, -420, "PST", "PDT", "Americas", 34.1, -118.2);
    add_tz(db, "America/Anchorage", "Alaska Time", -540, -480, "AKST", "AKDT", "Americas", 61.2, -149.9);
    add_tz(db, "Pacific/Honolulu", "Hawaii Time", -600, -600, "HST", "HST", "Pacific", 21.3, -157.8);
    add_tz(db, "America/Sao_Paulo", "Brasilia Time", -180, -180, "BRT", "BRT", "Americas", -23.5, -46.6);
    add_tz(db, "Europe/London", "Greenwich Mean Time", 0, 60, "GMT", "BST", "Europe", 51.5, -0.1);
    add_tz(db, "Europe/Berlin", "Central European Time", 60, 120, "CET", "CEST", "Europe", 52.5, 13.4);
    add_tz(db, "Europe/Moscow", "Moscow Time", 180, 180, "MSK", "MSK", "Europe", 55.8, 37.6);
    add_tz(db, "Asia/Tokyo", "Japan Standard Time", 540, 540, "JST", "JST", "Asia", 35.7, 139.7);
    add_tz(db, "Asia/Shanghai", "China Standard Time", 480, 480, "CST", "CST", "Asia", 31.2, 121.5);
    add_tz(db, "Asia/Kolkata", "India Standard Time", 330, 330, "IST", "IST", "Asia", 28.6, 77.2);
    add_tz(db, "Asia/Dubai", "Gulf Standard Time", 240, 240, "GST", "GST", "Asia", 25.3, 55.3);
    add_tz(db, "Australia/Sydney", "Australian Eastern Time", 600, 660, "AEST", "AEDT", "Australia", -33.9, 151.2);
    add_tz(db, "Pacific/Auckland", "New Zealand Time", 720, 780, "NZST", "NZDT", "Pacific", -36.8, 174.8);
    add_tz(db, "UTC", "Coordinated Universal Time", 0, 0, "UTC", "UTC", "UTC", 0.0, 0.0);

    // Default timezone.
    state.current_tz = String::from("UTC");

    // Default NTP servers.
    state.ntp_servers.push(NtpServer {
        hostname: String::from("pool.ntp.org"),
        port: 123,
        enabled: true,
        last_sync_ns: 0,
        offset_us: 0,
    });
    state.ntp_servers.push(NtpServer {
        hostname: String::from("time.google.com"),
        port: 123,
        enabled: true,
        last_sync_ns: 0,
        offset_us: 0,
    });
    state.ntp_servers.push(NtpServer {
        hostname: String::from("time.cloudflare.com"),
        port: 123,
        enabled: true,
        last_sync_ns: 0,
        offset_us: 0,
    });

    state.ntp_enabled = true;
    state.ntp_status = NtpStatus::Disabled;
    state.changes += 1;
}

/// Return (tz_count, ntp_server_count, ntp_enabled, ops).
pub fn stats() -> (usize, usize, bool, u64) {
    let state = STATE.lock();
    let tzs = state.tz_database.len();
    let ntps = state.ntp_servers.len();
    let ntp_on = state.ntp_enabled;
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (tzs, ntps, ntp_on, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.current_tz = String::new();
    state.tz_database.clear();
    state.ntp_servers.clear();
    state.ntp_enabled = true;
    state.ntp_status = NtpStatus::Disabled;
    state.time_format = TimeFormat::H24;
    state.date_format = DateFormat::Iso;
    state.week_start = WeekStart::Monday;
    state.show_seconds = false;
    state.show_date = true;
    state.manual_offset_ns = 0;
    state.last_gps_tz = String::new();
    state.last_gps_lat = 0.0;
    state.last_gps_lon = 0.0;
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    init_defaults();

    // Test 1: timezone selection.
    serial_println!("timezone::self_test 1: timezone selection");
    set_timezone("America/New_York")?;
    assert_eq!(current_timezone(), "America/New_York");
    let info = timezone_info()?;
    assert_eq!(info.utc_offset_min, -300);
    // Invalid timezone rejected.
    assert!(set_timezone("Invalid/Zone").is_err());

    // Test 2: list and filter.
    serial_println!("timezone::self_test 2: list and filter");
    let all = list_timezones("");
    assert_eq!(all.len(), 17);
    let europe = list_timezones("Europe");
    assert_eq!(europe.len(), 3);
    let regions = list_regions();
    assert!(regions.len() >= 5); // Americas, Europe, Asia, Pacific, Australia, UTC

    // Test 3: GPS detection.
    serial_println!("timezone::self_test 3: GPS detection");
    let detected = detect_from_location(51.5, -0.1)?; // London
    assert_eq!(detected, "Europe/London");
    let detected = detect_from_location(35.7, 139.7)?; // Tokyo
    assert_eq!(detected, "Asia/Tokyo");

    // Test 4: NTP servers.
    serial_println!("timezone::self_test 4: NTP servers");
    let servers = list_ntp_servers();
    assert_eq!(servers.len(), 3);
    add_ntp_server("custom.ntp.example", 123)?;
    assert_eq!(list_ntp_servers().len(), 4);
    assert!(add_ntp_server("pool.ntp.org", 123).is_err()); // duplicate
    remove_ntp_server("custom.ntp.example")?;
    assert_eq!(list_ntp_servers().len(), 3);

    // Test 5: NTP sync.
    serial_println!("timezone::self_test 5: NTP sync");
    set_ntp_enabled(true)?;
    simulate_sync("pool.ntp.org", -1500)?;
    assert_eq!(ntp_status(), NtpStatus::Synced);
    set_ntp_enabled(false)?;
    assert_eq!(ntp_status(), NtpStatus::Disabled);

    // Test 6: display format settings.
    serial_println!("timezone::self_test 6: format settings");
    set_time_format(TimeFormat::H12);
    set_date_format(DateFormat::Dmy);
    set_week_start(WeekStart::Sunday);
    set_show_seconds(true);
    set_show_date(false);
    let (tf, df, ws, sec, date) = format_settings();
    assert_eq!(tf, TimeFormat::H12);
    assert_eq!(df, DateFormat::Dmy);
    assert_eq!(ws, WeekStart::Sunday);
    assert!(sec);
    assert!(!date);

    // Test 7: manual offset.
    serial_println!("timezone::self_test 7: manual offset");
    set_manual_offset(3_600_000_000_000)?; // +1 hour
    let state = STATE.lock();
    assert_eq!(state.manual_offset_ns, 3_600_000_000_000);
    drop(state);

    clear_all();
    serial_println!("timezone::self_test: all 7 tests passed");
    Ok(())
}
