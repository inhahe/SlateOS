//! Locale and regional settings — language, number/date formats, timezone.
//!
//! System-wide locale configuration that applications query for formatting
//! numbers, dates, currencies, sort order, and UI language. Also manages
//! timezone selection.
//!
//! ## Design Reference
//!
//! Implied by design.txt line 288 (locale-dependent case folding), line 1339
//! (auto-detect DPI/scaling), and general OS requirements for multi-language
//! support and regional format differences.
//!
//! ## Architecture
//!
//! ```text
//! Application / GUI toolkit
//!   → locale::language() → "en-US"
//!   → locale::number_format() → NumberFormat
//!   → locale::date_format() → DateFormat
//!   → locale::timezone_offset_minutes() → i16   (the offset *now*)
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use tzrules::Tz;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum installed languages.
const MAX_LANGUAGES: usize = 64;

/// Maximum installed timezones.
const MAX_TIMEZONES: usize = 512;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Number format style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberSeparator {
    /// 1,000.50 (US/UK)
    CommaDot,
    /// 1.000,50 (Germany/Brazil)
    DotComma,
    /// 1 000,50 (France/Sweden)
    SpaceComma,
    /// 1'000.50 (Switzerland)
    ApostropheDot,
}

impl NumberSeparator {
    pub fn label(self) -> &'static str {
        match self {
            Self::CommaDot => "1,000.50",
            Self::DotComma => "1.000,50",
            Self::SpaceComma => "1 000,50",
            Self::ApostropheDot => "1'000.50",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "commadot" | "us" | "en" => Some(Self::CommaDot),
            "dotcomma" | "de" | "eu" => Some(Self::DotComma),
            "spacecomma" | "fr" => Some(Self::SpaceComma),
            "apostrophedot" | "ch" => Some(Self::ApostropheDot),
            _ => None,
        }
    }

    pub fn thousands(self) -> char {
        match self {
            Self::CommaDot => ',',
            Self::DotComma => '.',
            Self::SpaceComma => ' ',
            Self::ApostropheDot => '\'',
        }
    }

    pub fn decimal(self) -> char {
        match self {
            Self::CommaDot | Self::ApostropheDot => '.',
            Self::DotComma | Self::SpaceComma => ',',
        }
    }
}

/// Date format order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    /// MM/DD/YYYY (US)
    MonthDayYear,
    /// DD/MM/YYYY (most of world)
    DayMonthYear,
    /// YYYY-MM-DD (ISO 8601)
    YearMonthDay,
}

impl DateOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::MonthDayYear => "MM/DD/YYYY",
            Self::DayMonthYear => "DD/MM/YYYY",
            Self::YearMonthDay => "YYYY-MM-DD",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "mdy" | "us" => Some(Self::MonthDayYear),
            "dmy" | "eu" | "uk" => Some(Self::DayMonthYear),
            "ymd" | "iso" => Some(Self::YearMonthDay),
            _ => None,
        }
    }
}

/// Date separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSeparator {
    Slash, // /
    Dash,  // -
    Dot,   // .
}

impl DateSeparator {
    pub fn label(self) -> &'static str {
        match self {
            Self::Slash => "/",
            Self::Dash => "-",
            Self::Dot => ".",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "/" | "slash" => Some(Self::Slash),
            "-" | "dash" => Some(Self::Dash),
            "." | "dot" => Some(Self::Dot),
            _ => None,
        }
    }
}

/// Time format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeFormat {
    /// 12-hour with AM/PM.
    Hour12,
    /// 24-hour.
    Hour24,
}

impl TimeFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hour12 => "12h",
            Self::Hour24 => "24h",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "12" | "12h" | "ampm" => Some(Self::Hour12),
            "24" | "24h" => Some(Self::Hour24),
            _ => None,
        }
    }
}

/// First day of week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstDay {
    Sunday,
    Monday,
    Saturday,
}

impl FirstDay {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sunday => "Sunday",
            Self::Monday => "Monday",
            Self::Saturday => "Saturday",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sun" | "sunday" => Some(Self::Sunday),
            "mon" | "monday" => Some(Self::Monday),
            "sat" | "saturday" => Some(Self::Saturday),
            _ => None,
        }
    }
}

/// Measurement system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurementSystem {
    Metric,
    Imperial,
}

impl MeasurementSystem {
    pub fn label(self) -> &'static str {
        match self {
            Self::Metric => "metric",
            Self::Imperial => "imperial",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "metric" | "si" => Some(Self::Metric),
            "imperial" | "us" => Some(Self::Imperial),
            _ => None,
        }
    }
}

/// An installed language.
#[derive(Debug, Clone)]
pub struct Language {
    /// BCP 47 language tag (e.g., "en-US", "de-DE").
    pub tag: String,
    /// Native display name.
    pub native_name: String,
    /// English display name.
    pub english_name: String,
}

/// A timezone entry.
///
/// The entry stores a **rule**, not an offset. It used to store
/// `utc_offset_min: i16` beside an `observes_dst: bool`, which is a shape that
/// cannot be right: a zone has *two* offsets and a rule choosing between them
/// at a given instant, so one number is an hour wrong for whichever half of
/// the year it is not describing — and the `bool` merely recorded that the
/// entry *knew* it was incomplete without doing anything about it. The IANA
/// `id` is a label that selects a rule; nothing computes local time from it.
#[derive(Debug, Clone)]
pub struct Timezone {
    /// IANA timezone identifier (e.g., "America/New_York").
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// The POSIX `TZ` rule that defines this zone (e.g.
    /// `"EST5EDT,M3.2.0,M11.1.0"`). Evaluated by `tzrules`, the same engine
    /// the libc, the shell, the desktop clock and the world clock use — so
    /// nothing on the machine can disagree about what time it is.
    pub posix_tz: String,
}

impl Timezone {
    /// The parsed rule, or `None` if `posix_tz` is malformed.
    #[must_use]
    pub fn rule(&self) -> Option<Tz> {
        Tz::parse(self.posix_tz.as_bytes())
    }

    /// UTC offset in minutes **at a given instant** (e.g. -300 for EST,
    /// -240 for the same zone on EDT). Unparseable rules read as UTC.
    #[must_use]
    pub fn offset_minutes_at(&self, utc_secs: i64) -> i16 {
        let secs = self.rule().map_or(0, |r| r.lookup(utc_secs).gmtoff);
        i16::try_from(secs.div_euclid(60)).unwrap_or(0)
    }

    /// Whether the zone is *currently* shifted — a live property, unlike
    /// [`Self::observes_dst`].
    #[must_use]
    pub fn is_dst_at(&self, utc_secs: i64) -> bool {
        self.rule().is_some_and(|r| r.lookup(utc_secs).is_dst)
    }

    /// Whether the zone shifts at some point in the year — a standing
    /// property. Use [`Self::is_dst_at`] to badge a zone as on DST *now*.
    #[must_use]
    pub fn observes_dst(&self) -> bool {
        self.rule().is_some_and(|r| r.has_dst())
    }

    /// The abbreviation in force at an instant (`EST` in winter, `EDT` in
    /// summer).
    #[must_use]
    pub fn abbrev_at(&self, utc_secs: i64) -> String {
        self.rule().map_or_else(
            || String::from("UTC"),
            |r| {
                let info = r.lookup(utc_secs);
                core::str::from_utf8(info.name.as_bytes())
                    .map_or_else(|_| String::from("UTC"), String::from)
            },
        )
    }
}

/// Render a UTC offset in minutes as `+05:30` / `-08:00` / `+00:00`.
///
/// Deliberately integer-only. This used to be formatted as
/// `offset_minutes as f32 / 60.0`, which both printed nonsense for a
/// half-hour zone (`UTC+5.5`) and dragged floating point — and therefore SSE
/// register state — into kernel print paths.
///
/// Takes `impl Into<i32>` rather than `i16` so the sibling [`super::timezone`] table,
/// whose offsets are `i32`, can share the one formatter instead of growing a
/// near-copy that drifts.
#[must_use]
pub fn format_utc_offset(minutes: impl Into<i32>) -> String {
    let total: i32 = minutes.into();
    let sign = if total < 0 { '-' } else { '+' };
    let abs = total.unsigned_abs();
    alloc::format!("{}{:02}:{:02}", sign, abs / 60, abs % 60)
}

/// Full locale configuration.
#[derive(Debug, Clone)]
pub struct LocaleConfig {
    /// Display language (BCP 47 tag).
    pub language: String,
    /// Fallback language.
    pub fallback_language: String,
    /// Regional format language (for numbers/dates, may differ from display).
    pub region_format: String,
    /// Number format.
    pub number_format: NumberSeparator,
    /// Currency symbol.
    pub currency_symbol: String,
    /// Currency position: true = before number ($100), false = after (100€).
    pub currency_before: bool,
    /// Date order.
    pub date_order: DateOrder,
    /// Date separator.
    pub date_separator: DateSeparator,
    /// Time format.
    pub time_format: TimeFormat,
    /// First day of week.
    pub first_day: FirstDay,
    /// Measurement system.
    pub measurement: MeasurementSystem,
    /// Active timezone ID.
    pub timezone: String,
    /// Paper size: true = A4, false = Letter.
    pub paper_a4: bool,
}

impl Default for LocaleConfig {
    fn default() -> Self {
        Self {
            language: String::from("en-US"),
            fallback_language: String::from("en"),
            region_format: String::from("en-US"),
            number_format: NumberSeparator::CommaDot,
            currency_symbol: String::from("$"),
            currency_before: true,
            date_order: DateOrder::MonthDayYear,
            date_separator: DateSeparator::Slash,
            time_format: TimeFormat::Hour12,
            first_day: FirstDay::Sunday,
            measurement: MeasurementSystem::Imperial,
            timezone: String::from("America/New_York"),
            paper_a4: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    config: LocaleConfig,
    languages: Vec<Language>,
    timezones: Vec<Timezone>,
}

impl State {
    const fn new() -> Self {
        Self {
            config: LocaleConfig {
                language: String::new(),
                fallback_language: String::new(),
                region_format: String::new(),
                number_format: NumberSeparator::CommaDot,
                currency_symbol: String::new(),
                currency_before: true,
                date_order: DateOrder::MonthDayYear,
                date_separator: DateSeparator::Slash,
                time_format: TimeFormat::Hour12,
                first_day: FirstDay::Sunday,
                measurement: MeasurementSystem::Imperial,
                timezone: String::new(),
                paper_a4: false,
            },
            languages: Vec::new(),
            timezones: Vec::new(),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Configuration getters/setters
// ---------------------------------------------------------------------------

pub fn config() -> LocaleConfig {
    STATE.lock().config.clone()
}
pub fn language() -> String {
    STATE.lock().config.language.clone()
}
pub fn timezone_id() -> String {
    STATE.lock().config.timezone.clone()
}

pub fn set_language(tag: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.languages.iter().any(|l| l.tag == tag) {
        return Err(KernelError::NotFound);
    }
    state.config.language = String::from(tag);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn set_fallback_language(tag: &str) {
    STATE.lock().config.fallback_language = String::from(tag);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_region_format(tag: &str) {
    STATE.lock().config.region_format = String::from(tag);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_number_format(fmt: NumberSeparator) {
    STATE.lock().config.number_format = fmt;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_currency(symbol: &str, before: bool) {
    let mut state = STATE.lock();
    state.config.currency_symbol = String::from(symbol);
    state.config.currency_before = before;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_date_order(order: DateOrder) {
    STATE.lock().config.date_order = order;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_date_separator(sep: DateSeparator) {
    STATE.lock().config.date_separator = sep;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_time_format(fmt: TimeFormat) {
    STATE.lock().config.time_format = fmt;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_first_day(day: FirstDay) {
    STATE.lock().config.first_day = day;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_measurement(sys: MeasurementSystem) {
    STATE.lock().config.measurement = sys;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_timezone(tz_id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.timezones.iter().any(|t| t.id == tz_id) {
        return Err(KernelError::NotFound);
    }
    state.config.timezone = String::from(tz_id);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn set_paper_a4(v: bool) {
    STATE.lock().config.paper_a4 = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// UTC offset in minutes for the current timezone **at `utc_secs`**.
///
/// There is no such thing as "the" offset of a zone: `America/New_York` is
/// -300 in January and -240 in July. The instant is therefore a parameter, not
/// something this function is entitled to assume.
pub fn timezone_offset_minutes_at(utc_secs: i64) -> i16 {
    let state = STATE.lock();
    state
        .timezones
        .iter()
        .find(|t| t.id == state.config.timezone)
        .map_or(0, |t| t.offset_minutes_at(utc_secs))
}

/// UTC offset in minutes for the current timezone right now.
///
/// Convenience wrapper over [`timezone_offset_minutes_at`] that reads the
/// realtime clock. Before timekeeping is initialised `clock_realtime()` returns
/// 0, which evaluates the rule at the epoch — standard time for every zone,
/// which is the only defensible answer when the machine does not yet know what
/// time it is.
pub fn timezone_offset_minutes() -> i16 {
    timezone_offset_minutes_at(now_utc_secs())
}

/// Current wall-clock time in whole seconds since the Unix epoch.
///
/// Public because every caller that wants to *display* a zone needs the same
/// instant the offset was resolved at — a listing that resolved each row at its
/// own "now" could straddle a DST transition mid-table.
#[must_use]
pub fn now_utc_secs() -> i64 {
    let ns = crate::timekeeping::clock_realtime();
    i64::try_from(ns / 1_000_000_000).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Language management
// ---------------------------------------------------------------------------

pub fn add_language(tag: &str, native: &str, english: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.languages.len() >= MAX_LANGUAGES {
        return Err(KernelError::ResourceExhausted);
    }
    if state.languages.iter().any(|l| l.tag == tag) {
        return Err(KernelError::AlreadyExists);
    }
    state.languages.push(Language {
        tag: String::from(tag),
        native_name: String::from(native),
        english_name: String::from(english),
    });
    Ok(())
}

pub fn remove_language(tag: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.languages.len();
    state.languages.retain(|l| l.tag != tag);
    if state.languages.len() == len {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

pub fn list_languages() -> Vec<Language> {
    STATE.lock().languages.clone()
}

// ---------------------------------------------------------------------------
// Timezone management
// ---------------------------------------------------------------------------

/// Install a timezone.
///
/// `posix_tz` is a POSIX `TZ` rule (`"EST5EDT,M3.2.0,M11.1.0"`), not an offset.
/// It is parsed here rather than at first use, so a malformed rule is rejected
/// at the point it can still be reported instead of silently reading as UTC
/// every time anyone asks for the time.
pub fn add_timezone(id: &str, name: &str, posix_tz: &str) -> KernelResult<()> {
    if Tz::parse(posix_tz.as_bytes()).is_none() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    if state.timezones.len() >= MAX_TIMEZONES {
        return Err(KernelError::ResourceExhausted);
    }
    if state.timezones.iter().any(|t| t.id == id) {
        return Err(KernelError::AlreadyExists);
    }
    state.timezones.push(Timezone {
        id: String::from(id),
        display_name: String::from(name),
        posix_tz: String::from(posix_tz),
    });
    Ok(())
}

pub fn list_timezones() -> Vec<Timezone> {
    STATE.lock().timezones.clone()
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.languages.is_empty() {
        return;
    }

    // Common languages.
    let langs = [
        ("en-US", "English (US)", "English (United States)"),
        ("en-GB", "English (UK)", "English (United Kingdom)"),
        ("de-DE", "Deutsch", "German"),
        ("fr-FR", "Fran\u{00e7}ais", "French"),
        ("es-ES", "Espa\u{00f1}ol", "Spanish"),
        ("pt-BR", "Portugu\u{00ea}s (BR)", "Portuguese (Brazil)"),
        ("ja-JP", "\u{65e5}\u{672c}\u{8a9e}", "Japanese"),
        ("ko-KR", "\u{d55c}\u{ad6d}\u{c5b4}", "Korean"),
        (
            "zh-CN",
            "\u{4e2d}\u{6587}(\u{7b80}\u{4f53})",
            "Chinese (Simplified)",
        ),
        (
            "ru-RU",
            "\u{0420}\u{0443}\u{0441}\u{0441}\u{043a}\u{0438}\u{0439}",
            "Russian",
        ),
        (
            "ar-SA",
            "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}",
            "Arabic",
        ),
        (
            "hi-IN",
            "\u{0939}\u{093f}\u{0928}\u{094d}\u{0926}\u{0940}",
            "Hindi",
        ),
    ];
    for &(tag, native, english) in &langs {
        state.languages.push(Language {
            tag: String::from(tag),
            native_name: String::from(native),
            english_name: String::from(english),
        });
    }

    // Common timezones. Each carries the POSIX `TZ` rule that *defines* it —
    // the transition dates included — so the offset is derived at the instant
    // it is asked for. The old table stored a single winter offset and a
    // `dst: bool` that nothing consulted, so every DST zone in it was an hour
    // wrong for half of every year.
    //
    // Note the inverted POSIX sign convention: `EST5` is UTC-5 and `JST-9` is
    // UTC+9. `<+0530>`-style angle brackets are unnecessary here because every
    // abbreviation below is alphabetic.
    let tzs = [
        ("UTC", "UTC", "UTC0"),
        (
            "America/New_York",
            "Eastern Time (US)",
            "EST5EDT,M3.2.0,M11.1.0",
        ),
        (
            "America/Chicago",
            "Central Time (US)",
            "CST6CDT,M3.2.0,M11.1.0",
        ),
        (
            "America/Denver",
            "Mountain Time (US)",
            "MST7MDT,M3.2.0,M11.1.0",
        ),
        (
            "America/Los_Angeles",
            "Pacific Time (US)",
            "PST8PDT,M3.2.0,M11.1.0",
        ),
        ("Europe/London", "GMT / BST", "GMT0BST,M3.5.0/1,M10.5.0"),
        (
            "Europe/Berlin",
            "Central European Time",
            "CET-1CEST,M3.5.0,M10.5.0/3",
        ),
        // Russia abolished DST in 2011 and settled on permanent UTC+3 in 2014.
        ("Europe/Moscow", "Moscow Time", "MSK-3"),
        ("Asia/Tokyo", "Japan Standard Time", "JST-9"),
        ("Asia/Shanghai", "China Standard Time", "CST-8"),
        // India is UTC+5:30 — the half-hour a whole-hour offset cannot express.
        ("Asia/Kolkata", "India Standard Time", "IST-5:30"),
        // Southern hemisphere: DST starts in October and ends in April.
        (
            "Australia/Sydney",
            "Australian Eastern Time",
            "AEST-10AEDT,M10.1.0,M4.1.0/3",
        ),
    ];
    for &(id, name, posix_tz) in &tzs {
        state.timezones.push(Timezone {
            id: String::from(id),
            display_name: String::from(name),
            posix_tz: String::from(posix_tz),
        });
    }

    state.config = LocaleConfig::default();
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

pub fn stats() -> (usize, usize, u64) {
    let state = STATE.lock();
    (
        state.languages.len(),
        state.timezones.len(),
        CHANGE_COUNT.load(Ordering::Relaxed),
    )
}

pub fn reset_stats() {
    CHANGE_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.config = LocaleConfig::default();
    state.languages.clear();
    state.timezones.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Init defaults.
    serial_println!("  locale::self_test 1: init defaults");
    init_defaults();
    let langs = list_languages();
    assert!(langs.len() >= 12);
    let tzs = list_timezones();
    assert!(tzs.len() >= 12);

    // Test 2: Language selection.
    serial_println!("  locale::self_test 2: language");
    set_language("de-DE")?;
    assert_eq!(language(), "de-DE");
    assert!(set_language("xx-XX").is_err()); // Not installed.

    // Test 3: Number format.
    serial_println!("  locale::self_test 3: number format");
    set_number_format(NumberSeparator::DotComma);
    let cfg = config();
    assert_eq!(cfg.number_format, NumberSeparator::DotComma);
    assert_eq!(cfg.number_format.thousands(), '.');
    assert_eq!(cfg.number_format.decimal(), ',');

    // Test 4: Date/time format.
    serial_println!("  locale::self_test 4: date/time");
    set_date_order(DateOrder::YearMonthDay);
    set_date_separator(DateSeparator::Dash);
    set_time_format(TimeFormat::Hour24);
    let cfg2 = config();
    assert_eq!(cfg2.date_order, DateOrder::YearMonthDay);
    assert_eq!(cfg2.time_format, TimeFormat::Hour24);

    // Test 5: Timezone. Every assertion is made at a *named instant*, because
    // the whole point of the rule-carrying table is that a zone does not have
    // one offset. New York is checked in both halves of the year precisely
    // because the old fixed-offset table could only ever have got one of them
    // right.
    serial_println!("  locale::self_test 5: timezone");
    /// 2024-01-15 12:00:00 UTC — northern winter.
    const NOON_JAN: i64 = 1_705_320_000;
    /// 2024-07-15 12:00:00 UTC — northern summer.
    const NOON_JUL: i64 = 1_721_044_800;

    set_timezone("Asia/Tokyo")?;
    // Japan has not observed DST since 1951, so the offset is the same all year.
    assert_eq!(timezone_offset_minutes_at(NOON_JAN), 540);
    assert_eq!(timezone_offset_minutes_at(NOON_JUL), 540);

    set_timezone("America/New_York")?;
    assert_eq!(timezone_offset_minutes_at(NOON_JAN), -300); // EST
    assert_eq!(timezone_offset_minutes_at(NOON_JUL), -240); // EDT

    // Sydney is the southern-hemisphere check: its DST covers the months the
    // northern zones are on standard time, so a table that got the sign of the
    // season wrong would still pass a northern-only test.
    set_timezone("Australia/Sydney")?;
    assert_eq!(timezone_offset_minutes_at(NOON_JAN), 660); // AEDT
    assert_eq!(timezone_offset_minutes_at(NOON_JUL), 600); // AEST

    // The half-hour zone a whole-hour offset cannot represent at all.
    set_timezone("Asia/Kolkata")?;
    assert_eq!(timezone_offset_minutes_at(NOON_JAN), 330);
    assert_eq!(
        format_utc_offset(timezone_offset_minutes_at(NOON_JAN)),
        "+05:30"
    );

    // Formatting is integer-only in both directions.
    assert_eq!(format_utc_offset(0), "+00:00");
    assert_eq!(format_utc_offset(-480), "-08:00");
    assert_eq!(format_utc_offset(345), "+05:45"); // Nepal

    // Every shipped rule must parse, and the DST flag must be *derived*.
    for tz in &list_timezones() {
        assert!(tz.rule().is_some());
    }
    let tzs2 = list_timezones();
    let ny = tzs2
        .iter()
        .find(|t| t.id == "America/New_York")
        .ok_or(KernelError::NotFound)?;
    assert!(ny.observes_dst());
    assert!(!ny.is_dst_at(NOON_JAN));
    assert!(ny.is_dst_at(NOON_JUL));
    assert_eq!(ny.abbrev_at(NOON_JAN), "EST");
    assert_eq!(ny.abbrev_at(NOON_JUL), "EDT");
    let tokyo = tzs2
        .iter()
        .find(|t| t.id == "Asia/Tokyo")
        .ok_or(KernelError::NotFound)?;
    assert!(!tokyo.observes_dst());

    // A malformed rule is rejected at install time rather than silently
    // reading as UTC forever after.
    assert!(add_timezone("Bad/Zone", "Nonsense", "").is_err());
    set_timezone("America/New_York")?;

    // Test 6: Measurement and currency.
    serial_println!("  locale::self_test 6: measurement/currency");
    set_measurement(MeasurementSystem::Metric);
    assert_eq!(config().measurement, MeasurementSystem::Metric);
    set_currency("\u{20ac}", false);
    let cfg3 = config();
    assert_eq!(cfg3.currency_symbol, "\u{20ac}");
    assert!(!cfg3.currency_before);

    // Test 7: First day and paper.
    serial_println!("  locale::self_test 7: first day and paper");
    set_first_day(FirstDay::Monday);
    assert_eq!(config().first_day, FirstDay::Monday);
    set_paper_a4(true);
    assert!(config().paper_a4);

    let (lc, tc, changes) = stats();
    assert!(lc >= 12);
    assert!(tc >= 12);
    assert!(changes > 0);

    clear_all();
    reset_stats();
    serial_println!("  locale: all tests passed");
    Ok(())
}
