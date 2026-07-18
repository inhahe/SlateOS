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
//!   → locale::timezone_offset_minutes() → i16
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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
    Slash,  // /
    Dash,   // -
    Dot,    // .
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
#[derive(Debug, Clone)]
pub struct Timezone {
    /// IANA timezone identifier (e.g., "America/New_York").
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// UTC offset in minutes (e.g., -300 for UTC-5).
    pub utc_offset_min: i16,
    /// Whether DST is observed.
    pub observes_dst: bool,
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

pub fn config() -> LocaleConfig { STATE.lock().config.clone() }
pub fn language() -> String { STATE.lock().config.language.clone() }
pub fn timezone_id() -> String { STATE.lock().config.timezone.clone() }

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

/// Get UTC offset in minutes for current timezone.
pub fn timezone_offset_minutes() -> i16 {
    let state = STATE.lock();
    state.timezones.iter()
        .find(|t| t.id == state.config.timezone)
        .map(|t| t.utc_offset_min)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Language management
// ---------------------------------------------------------------------------

pub fn add_language(tag: &str, native: &str, english: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.languages.len() >= MAX_LANGUAGES { return Err(KernelError::ResourceExhausted); }
    if state.languages.iter().any(|l| l.tag == tag) { return Err(KernelError::AlreadyExists); }
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
    if state.languages.len() == len { return Err(KernelError::NotFound); }
    Ok(())
}

pub fn list_languages() -> Vec<Language> { STATE.lock().languages.clone() }

// ---------------------------------------------------------------------------
// Timezone management
// ---------------------------------------------------------------------------

pub fn add_timezone(id: &str, name: &str, offset_min: i16, dst: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.timezones.len() >= MAX_TIMEZONES { return Err(KernelError::ResourceExhausted); }
    if state.timezones.iter().any(|t| t.id == id) { return Err(KernelError::AlreadyExists); }
    state.timezones.push(Timezone {
        id: String::from(id),
        display_name: String::from(name),
        utc_offset_min: offset_min,
        observes_dst: dst,
    });
    Ok(())
}

pub fn list_timezones() -> Vec<Timezone> { STATE.lock().timezones.clone() }

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.languages.is_empty() { return; }

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
        ("zh-CN", "\u{4e2d}\u{6587}(\u{7b80}\u{4f53})", "Chinese (Simplified)"),
        ("ru-RU", "\u{0420}\u{0443}\u{0441}\u{0441}\u{043a}\u{0438}\u{0439}", "Russian"),
        ("ar-SA", "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}", "Arabic"),
        ("hi-IN", "\u{0939}\u{093f}\u{0928}\u{094d}\u{0926}\u{0940}", "Hindi"),
    ];
    for &(tag, native, english) in &langs {
        state.languages.push(Language {
            tag: String::from(tag),
            native_name: String::from(native),
            english_name: String::from(english),
        });
    }

    // Common timezones.
    let tzs = [
        ("UTC", "UTC", 0, false),
        ("America/New_York", "Eastern Time (US)", -300, true),
        ("America/Chicago", "Central Time (US)", -360, true),
        ("America/Denver", "Mountain Time (US)", -420, true),
        ("America/Los_Angeles", "Pacific Time (US)", -480, true),
        ("Europe/London", "GMT / BST", 0, true),
        ("Europe/Berlin", "Central European Time", 60, true),
        ("Europe/Moscow", "Moscow Time", 180, false),
        ("Asia/Tokyo", "Japan Standard Time", 540, false),
        ("Asia/Shanghai", "China Standard Time", 480, false),
        ("Asia/Kolkata", "India Standard Time", 330, false),
        ("Australia/Sydney", "Australian Eastern Time", 600, true),
    ];
    for &(id, name, offset, dst) in &tzs {
        state.timezones.push(Timezone {
            id: String::from(id),
            display_name: String::from(name),
            utc_offset_min: offset,
            observes_dst: dst,
        });
    }

    state.config = LocaleConfig::default();
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

pub fn stats() -> (usize, usize, u64) {
    let state = STATE.lock();
    (state.languages.len(), state.timezones.len(), CHANGE_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() { CHANGE_COUNT.store(0, Ordering::Relaxed); }

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

    // Test 5: Timezone.
    serial_println!("  locale::self_test 5: timezone");
    set_timezone("Asia/Tokyo")?;
    assert_eq!(timezone_offset_minutes(), 540);
    set_timezone("America/New_York")?;
    assert_eq!(timezone_offset_minutes(), -300);

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
