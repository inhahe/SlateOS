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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use tzrules::Tz;

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
///
/// The entry stores a **rule**, not a pair of offsets. It used to carry
/// `std_offset_min`/`dst_offset_min`/`std_abbrev`/`dst_abbrev` — which knew a
/// zone had two states but had nothing that could *choose* between them, so
/// [`timezone_info`] hardcoded `dst_active: false` and every DST zone read as
/// standard time all year. The POSIX `TZ` rule carries the transition dates as
/// well as the offsets, so the choice is made by evaluating it at an instant.
#[derive(Debug, Clone)]
pub struct TzEntry {
    /// IANA timezone name.
    pub name: String,
    /// Display label.
    pub display_name: String,
    /// The POSIX `TZ` rule defining this zone, e.g.
    /// `"EST5EDT,M3.2.0,M11.1.0"`. Evaluated by `tzrules`, the same engine the
    /// libc, the shell, the desktop clock and `hwclock` use.
    pub posix_tz: String,
    /// Region for grouping (e.g., "Americas", "Europe").
    pub region: String,
    /// Approximate latitude (for GPS matching).
    pub lat: f32,
    /// Approximate longitude.
    pub lon: f32,
}

impl TzEntry {
    /// The parsed rule, or `None` if `posix_tz` is malformed. Entries are
    /// validated at insertion, so this is `Some` for anything in the database.
    #[must_use]
    pub fn rule(&self) -> Option<Tz> {
        Tz::parse(self.posix_tz.as_bytes())
    }

    /// UTC offset in minutes **at a given instant** — -300 for New York in
    /// January, -240 for the same zone in July.
    #[must_use]
    pub fn offset_minutes_at(&self, utc_secs: i64) -> i32 {
        self.rule()
            .map_or(0, |r| r.lookup(utc_secs).gmtoff.div_euclid(60))
    }

    /// Whether the zone is shifted at `utc_secs`.
    #[must_use]
    pub fn is_dst_at(&self, utc_secs: i64) -> bool {
        self.rule().is_some_and(|r| r.lookup(utc_secs).is_dst)
    }

    /// The abbreviation in effect at `utc_secs` (`EST` or `EDT`).
    #[must_use]
    pub fn abbrev_at(&self, utc_secs: i64) -> String {
        self.rule().map_or_else(
            || String::from("UTC"),
            |r| name_to_string(r.lookup(utc_secs).name),
        )
    }

    /// The zone's standard (non-DST) offset in minutes. Unlike
    /// [`Self::offset_minutes_at`] this is a property of the zone rather than
    /// of an instant, so it is the right thing to sort or tabulate by.
    #[must_use]
    pub fn std_offset_min(&self) -> i32 {
        self.rule().map_or(0, |r| r.std_gmtoff.div_euclid(60))
    }

    /// The zone's DST offset in minutes, equal to the standard offset for a
    /// zone that does not observe DST.
    #[must_use]
    pub fn dst_offset_min(&self) -> i32 {
        self.rule().map_or(0, |r| {
            r.dst.map_or(r.std_gmtoff, |d| d.gmtoff).div_euclid(60)
        })
    }

    /// The standard abbreviation (`EST`).
    #[must_use]
    pub fn std_abbrev(&self) -> String {
        self.rule()
            .map_or_else(|| String::from("UTC"), |r| name_to_string(r.std_name))
    }

    /// The DST abbreviation (`EDT`), equal to the standard one when the zone
    /// does not observe DST.
    #[must_use]
    pub fn dst_abbrev(&self) -> String {
        self.rule().map_or_else(
            || String::from("UTC"),
            |r| name_to_string(r.dst.map_or(r.std_name, |d| d.name)),
        )
    }

    /// Whether the zone observes DST at all — a property of the rule, as
    /// distinct from [`Self::is_dst_at`], which is a property of an instant.
    #[must_use]
    pub fn observes_dst(&self) -> bool {
        self.rule().is_some_and(|r| r.has_dst())
    }
}

/// A `TzName` as an owned `String`. Abbreviations are ASCII by construction,
/// but a non-UTF-8 one reads as `UTC` rather than panicking in a kernel path.
fn name_to_string(name: tzrules::TzName) -> String {
    core::str::from_utf8(name.as_bytes()).map_or_else(|_| String::from("UTC"), String::from)
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

impl State {
    /// The state a fresh boot starts with.
    ///
    /// Extracted from the initialiser of `STATE` so that the self-test can
    /// be handed a pristine table without disturbing the live one; see
    /// `crate::fs::selftest`.
    const fn new() -> Self {
        Self {
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
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

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

/// Get full timezone info for the current timezone **at a given instant**.
///
/// This used to return `dst_active: false` unconditionally with the comment
/// "Simplified: DST detection would need actual date logic". It does now: the
/// rule carries the transition dates, so the answer is computed rather than
/// assumed.
pub fn timezone_info_at(utc_secs: i64) -> KernelResult<TzInfo> {
    let state = STATE.lock();
    let entry = state
        .tz_database
        .iter()
        .find(|t| t.name == state.current_tz)
        .ok_or(KernelError::NotFound)?;
    Ok(TzInfo {
        name: entry.name.clone(),
        display_name: entry.display_name.clone(),
        utc_offset_min: entry.offset_minutes_at(utc_secs),
        dst_active: entry.is_dst_at(utc_secs),
        abbreviation: entry.abbrev_at(utc_secs),
    })
}

/// Get full timezone info for the current timezone right now.
pub fn timezone_info() -> KernelResult<TzInfo> {
    timezone_info_at(super::locale::now_utc_secs())
}

/// List all timezones, optionally filtered by region.
pub fn list_timezones(region_filter: &str) -> Vec<TzEntry> {
    let state = STATE.lock();
    if region_filter.is_empty() {
        state.tz_database.clone()
    } else {
        state
            .tz_database
            .iter()
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
    state.ntp_status = if enabled {
        NtpStatus::Syncing
    } else {
        NtpStatus::Disabled
    };
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
    let server = state
        .ntp_servers
        .iter_mut()
        .find(|s| s.hostname == hostname)
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
    (
        state.time_format,
        state.date_format,
        state.week_start,
        state.show_seconds,
        state.show_date,
    )
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

/// Insert a zone. A rule that does not parse is dropped rather than stored,
/// because an unparseable entry would silently read as UTC forever after —
/// the table is a compile-time constant, so a dropped row is a bug caught by
/// the self-test's count assertion.
fn add_tz(
    db: &mut Vec<TzEntry>,
    name: &str,
    display: &str,
    posix_tz: &str,
    region: &str,
    lat: f32,
    lon: f32,
) {
    if Tz::parse(posix_tz.as_bytes()).is_none() {
        return;
    }
    db.push(TzEntry {
        name: String::from(name),
        display_name: String::from(display),
        posix_tz: String::from(posix_tz),
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
    // POSIX `TZ` rules. Note the inverted sign convention: `EST5` is UTC-5 and
    // `JST-9` is UTC+9. Angle brackets are needed for abbreviations that are
    // not purely alphabetic (`<-03>`, `<+04>`), which is also what tzdata does.
    add_tz(
        db,
        "America/New_York",
        "Eastern Time (US)",
        "EST5EDT,M3.2.0,M11.1.0",
        "Americas",
        40.7,
        -74.0,
    );
    add_tz(
        db,
        "America/Chicago",
        "Central Time (US)",
        "CST6CDT,M3.2.0,M11.1.0",
        "Americas",
        41.9,
        -87.6,
    );
    add_tz(
        db,
        "America/Denver",
        "Mountain Time (US)",
        "MST7MDT,M3.2.0,M11.1.0",
        "Americas",
        39.7,
        -105.0,
    );
    add_tz(
        db,
        "America/Los_Angeles",
        "Pacific Time (US)",
        "PST8PDT,M3.2.0,M11.1.0",
        "Americas",
        34.1,
        -118.2,
    );
    add_tz(
        db,
        "America/Anchorage",
        "Alaska Time",
        "AKST9AKDT,M3.2.0,M11.1.0",
        "Americas",
        61.2,
        -149.9,
    );
    add_tz(
        db,
        "Pacific/Honolulu",
        "Hawaii Time",
        "HST10",
        "Pacific",
        21.3,
        -157.8,
    );
    // Brazil abolished DST in 2019, and tzdata now prints `-03` rather than
    // `BRT`. The old row's `dst_offset_min == std_offset_min` said as much,
    // but nothing could act on it.
    add_tz(
        db,
        "America/Sao_Paulo",
        "Brasilia Time",
        "<-03>3",
        "Americas",
        -23.5,
        -46.6,
    );
    add_tz(
        db,
        "Europe/London",
        "Greenwich Mean Time",
        "GMT0BST,M3.5.0/1,M10.5.0",
        "Europe",
        51.5,
        -0.1,
    );
    add_tz(
        db,
        "Europe/Berlin",
        "Central European Time",
        "CET-1CEST,M3.5.0,M10.5.0/3",
        "Europe",
        52.5,
        13.4,
    );
    // Russia abolished DST in 2011 and settled on permanent UTC+3 in 2014.
    add_tz(
        db,
        "Europe/Moscow",
        "Moscow Time",
        "MSK-3",
        "Europe",
        55.8,
        37.6,
    );
    add_tz(
        db,
        "Asia/Tokyo",
        "Japan Standard Time",
        "JST-9",
        "Asia",
        35.7,
        139.7,
    );
    add_tz(
        db,
        "Asia/Shanghai",
        "China Standard Time",
        "CST-8",
        "Asia",
        31.2,
        121.5,
    );
    add_tz(
        db,
        "Asia/Kolkata",
        "India Standard Time",
        "IST-5:30",
        "Asia",
        28.6,
        77.2,
    );
    add_tz(
        db,
        "Asia/Dubai",
        "Gulf Standard Time",
        "<+04>-4",
        "Asia",
        25.3,
        55.3,
    );
    add_tz(
        db,
        "Australia/Sydney",
        "Australian Eastern Time",
        "AEST-10AEDT,M10.1.0,M4.1.0/3",
        "Australia",
        -33.9,
        151.2,
    );
    add_tz(
        db,
        "Pacific/Auckland",
        "New Zealand Time",
        "NZST-12NZDT,M9.5.0,M4.1.0/3",
        "Pacific",
        -36.8,
        174.8,
    );
    add_tz(
        db,
        "UTC",
        "Coordinated Universal Time",
        "UTC0",
        "UTC",
        0.0,
        0.0,
    );

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

/// The suite asserts exact table contents, so it needs a table of its own.
/// It used to get one by calling `clear_all()`, which — since this suite is
/// reachable from the shell — deleted whatever the user had stored here and
/// then reported success.  The live state is moved aside for the duration and
/// put back afterwards; `crate::fs::selftest` records why this shape rather
/// than the alternatives.
pub fn self_test() -> KernelResult<()> {
    // These counters live outside the table, so `with_pristine` cannot
    // see them; save and restore them here so a run leaves no trace.
    let saved_op_count = OP_COUNT.load(Ordering::Relaxed);
    let result = crate::fs::selftest::with_pristine(&STATE, State::new(), self_test_inner);
    OP_COUNT.store(saved_op_count, Ordering::Relaxed);
    result
}

fn self_test_inner() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    init_defaults();

    /// 2024-01-15 12:00:00 UTC — northern winter.
    const NOON_JAN: i64 = 1_705_320_000;
    /// 2024-07-15 12:00:00 UTC — northern summer.
    const NOON_JUL: i64 = 1_721_044_800;

    // Test 1: timezone selection. The offset is asserted at both halves of the
    // year, because the old table could only ever have got one of them right —
    // `timezone_info` returned the winter offset and `dst_active: false` in
    // July as well.
    serial_println!("timezone::self_test 1: timezone selection");
    set_timezone("America/New_York")?;
    assert_eq!(current_timezone(), "America/New_York");
    let winter = timezone_info_at(NOON_JAN)?;
    assert_eq!(winter.utc_offset_min, -300);
    assert!(!winter.dst_active);
    assert_eq!(winter.abbreviation, "EST");
    let summer = timezone_info_at(NOON_JUL)?;
    assert_eq!(summer.utc_offset_min, -240);
    assert!(summer.dst_active);
    assert_eq!(summer.abbreviation, "EDT");
    // Invalid timezone rejected.
    assert!(set_timezone("Invalid/Zone").is_err());

    // Southern hemisphere: DST covers the months the northern zones are on
    // standard time, so a northern-only check would not catch a season sign
    // error.
    set_timezone("Pacific/Auckland")?;
    assert_eq!(timezone_info_at(NOON_JAN)?.utc_offset_min, 780); // NZDT
    assert_eq!(timezone_info_at(NOON_JUL)?.utc_offset_min, 720); // NZST

    // Every shipped rule must parse — `add_tz` drops the ones that do not, so
    // the count assertion in test 2 is the other half of this check — and the
    // standard/DST offsets must be *derived* rather than stored.
    for tz in &list_timezones("") {
        assert!(tz.rule().is_some());
    }
    let all1 = list_timezones("");
    let kolkata = all1
        .iter()
        .find(|t| t.name == "Asia/Kolkata")
        .ok_or(KernelError::NotFound)?;
    assert_eq!(kolkata.std_offset_min(), 330); // the half-hour zone
    assert!(!kolkata.observes_dst());
    let ny = all1
        .iter()
        .find(|t| t.name == "America/New_York")
        .ok_or(KernelError::NotFound)?;
    assert_eq!(ny.std_offset_min(), -300);
    assert_eq!(ny.dst_offset_min(), -240);
    assert_eq!(ny.std_abbrev(), "EST");
    assert_eq!(ny.dst_abbrev(), "EDT");
    assert!(ny.observes_dst());
    set_timezone("America/New_York")?;

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
