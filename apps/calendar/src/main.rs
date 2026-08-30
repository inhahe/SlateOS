//! Calendar and scheduling application for SlateOS.
//!
//! Provides month/week/day/year views, event creation with recurrence,
//! categories, reminders, ICS import/export, and a mini-calendar sidebar.
//!
//! Opens as a real window, 1280x720 to start with and resizable from there.
//! The whole calendar is drawn as a [`Frame`]: every clickable thing records
//! the box it was painted in, as it is painted, and the hit test reads those
//! boxes back. Nothing here answers "where is that day" twice.

use guitk::color::Color;
// The shared civil-date arithmetic. This app used to carry its own: a Zeller's
// congruence for the weekday, a *separate* Julian day number for differences,
// its own leap rule, and an ISO week number its own comment admitted was "a
// simple approximation". The approximation disagreed with the real ISO week on
// 38.5% of the days between 1900 and 2100. See `known-issues.md`
// C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC.
use guitk::date::{self, Weekday};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
// Catppuccin Mocha palette — kept complete even though a few entries are
// not currently referenced; future event-category styling will pick them up.
#[allow(dead_code)]
const MANTLE: Color = Color::from_hex(0x181825);
#[allow(dead_code)]
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
#[allow(dead_code)]
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
#[allow(dead_code)]
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const PINK: Color = Color::from_hex(0xF5C2E7);
#[allow(dead_code)]
const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
#[allow(dead_code)]
const ROSEWATER: Color = Color::from_hex(0xF5E0DC);

// ============================================================================
// Date and time types
// ============================================================================

/// A simple date (year, month 1-12, day 1-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// This date as a [`guitk::date::Date`], the one civil-date implementation.
    ///
    /// The app keeps its own `{year, month, day}` struct because a hundred and
    /// forty call sites read those fields directly, but every *calculation*
    /// goes through here and back. Two representations of a date are only a
    /// hazard when they are two implementations of the arithmetic as well.
    fn civil(self) -> date::Date {
        date::Date::from_ymd(self.year, self.month, self.day)
    }

    /// The inverse of [`civil`](Self::civil).
    fn from_civil(d: date::Date) -> Self {
        let (year, month, day) = d.ymd();
        Self { year, month, day }
    }

    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        if !(1..=12).contains(&month) {
            return None;
        }
        let max_day = days_in_month(year, month);
        if day < 1 || day > max_day {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// The weekday.
    pub fn weekday(self) -> Weekday {
        self.civil().weekday()
    }

    /// Day of week: 0=Sunday, 1=Monday, ..., 6=Saturday.
    ///
    /// Was a hand-written Zeller's congruence. Zeller is correct for years
    /// ≥ 1 and wrong below that — `y % 100` and `y / 100` truncate toward
    /// zero in Rust, which is not the flooring the formula assumes — and
    /// nothing stopped a caller building a year 0. It also gave this struct a
    /// *second* day-numbering scheme beside `to_day_number`'s Julian one, two
    /// unrelated formulas that had to agree with each other by coincidence.
    pub fn day_of_week(self) -> u32 {
        u32::try_from(self.weekday().index()).unwrap_or(0)
    }

    pub fn day_of_week_name(self) -> &'static str {
        self.weekday().name()
    }

    pub fn day_of_week_short(self) -> &'static str {
        self.weekday().short_name()
    }

    pub fn month_name(self) -> &'static str {
        month_name(self.month)
    }

    pub fn month_short(self) -> &'static str {
        month_short(self.month)
    }

    /// The ISO 8601 week number, 1..=53.
    ///
    /// This was `day_of_year / 7 + 1`, adjusted by the weekday of 1 January
    /// and clamped to 53 — described in its own comment as "a simple
    /// approximation", which is what it was. Measured against the real ISO
    /// week over 1900-01-01..2100-01-01, it was wrong on **28 144 of 73 049
    /// days — 38.5%**, typically by one week, and the clamp meant it silently
    /// reported 53 for anything that overshot.
    ///
    /// ISO week 1 is the week containing the year's first Thursday, which is
    /// not in general the week containing 1 January; a formula that starts
    /// counting at 1 January cannot express that.
    pub fn week_number(self) -> u32 {
        self.civil().iso_week().1
    }

    /// The ISO 8601 week-numbering year, which is not always the calendar
    /// year: 2027-01-01 falls in week 53 of 2026.
    ///
    /// A week number without its year is ambiguous at exactly the boundary
    /// where it is most likely to be wrong, so the pair is available even
    /// though the current views only draw the number.
    pub fn iso_week(self) -> (i32, u32) {
        self.civil().iso_week()
    }

    pub fn day_of_year(self) -> u32 {
        self.civil().day_of_year()
    }

    pub fn is_today(self, today: Date) -> bool {
        self == today
    }

    pub fn is_weekend(self) -> bool {
        let dow = self.day_of_week();
        dow == 0 || dow == 6
    }

    /// Add days (positive or negative).
    ///
    /// Was a pair of `while` loops that stepped one month at a time, so
    /// `add_days(3650)` walked a hundred and twenty iterations to move ten
    /// years, and the backward loop's `if m == 12 { y -= 1 }` was correct only
    /// because "the new month is December" happens to imply "we just wrapped"
    /// — a proof that lived in the reader's head. It is now one addition on a
    /// day number.
    pub fn add_days(self, n: i32) -> Self {
        Self::from_civil(self.civil().add_days(n))
    }

    /// Next month, same day, clamped into the target month: 31 January plus a
    /// month is 28 February. Not reversible, which is inherent to the clamp.
    pub fn next_month(self) -> Self {
        Self::from_civil(self.civil().add_months(1))
    }

    /// Previous month, same day, clamped as [`next_month`](Self::next_month).
    pub fn prev_month(self) -> Self {
        Self::from_civil(self.civil().add_months(-1))
    }

    /// Next year, with 29 February clamped to the 28th in a common year.
    pub fn next_year(self) -> Self {
        Self::from_civil(self.civil().add_years(1))
    }

    /// Previous year, clamped as [`next_year`](Self::next_year).
    pub fn prev_year(self) -> Self {
        Self::from_civil(self.civil().add_years(-1))
    }

    /// Difference in days between two dates (`self - other`).
    ///
    /// No longer "approximate", and no longer computed from a Julian day
    /// number that this struct maintained *separately* from the Zeller
    /// congruence it used for weekdays. Both truncated toward zero on
    /// negative years, where the formulas need flooring.
    pub fn days_since(self, other: Self) -> i64 {
        i64::from(other.civil().days_until(self.civil()))
    }

    pub fn format_short(self) -> String {
        format!("{}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn format_long(self) -> String {
        format!(
            "{}, {} {}, {}",
            self.day_of_week_name(),
            self.month_name(),
            self.day,
            self.year
        )
    }

    pub fn format_header(self) -> String {
        format!("{} {}", self.month_name(), self.year)
    }
}

/// Time of day (hour 0-23, minute 0-59).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time {
    pub hour: u32,
    pub minute: u32,
}

impl Time {
    pub fn new(hour: u32, minute: u32) -> Option<Self> {
        if hour > 23 || minute > 59 {
            return None;
        }
        Some(Self { hour, minute })
    }

    pub fn from_minutes(total: u32) -> Self {
        Self {
            hour: (total / 60).min(23),
            minute: total % 60,
        }
    }

    pub fn to_minutes(self) -> u32 {
        self.hour.saturating_mul(60).saturating_add(self.minute)
    }

    pub fn format_24h(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    pub fn format_12h(self) -> String {
        let (h, ampm) = if self.hour == 0 {
            (12, "AM")
        } else if self.hour < 12 {
            (self.hour, "AM")
        } else if self.hour == 12 {
            (12, "PM")
        } else {
            (self.hour.saturating_sub(12), "PM")
        };
        format!("{h}:{:02} {ampm}", self.minute)
    }

    /// Minutes between two times (self - other).
    pub fn minutes_since(self, other: Self) -> i32 {
        (self.to_minutes() as i32).saturating_sub(other.to_minutes() as i32)
    }
}

/// Combined date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTime {
    pub date: Date,
    pub time: Time,
}

impl DateTime {
    pub fn new(date: Date, time: Time) -> Self {
        Self { date, time }
    }

    pub fn format(self) -> String {
        format!("{} {}", self.date.format_short(), self.time.format_24h())
    }

    pub fn format_ics(self) -> String {
        format!(
            "{}{:02}{:02}T{:02}{:02}00",
            self.date.year, self.date.month, self.date.day, self.time.hour, self.time.minute
        )
    }
}

// ============================================================================
// Date helper functions
// ============================================================================

// These four were each a local copy of a calculation `guitk::date` already
// owns. They stay as free functions because the app's own call sites read
// better that way, but they no longer *decide* anything.
//
// One behaviour change worth naming: `days_in_month` used to answer **0** for
// a month outside 1..=12, and `month_name` / `month_short` answered "Unknown"
// / "???". A zero-length month is not a safer answer than a clamped one — the
// old `add_days` walked `while d > days_in_month(y, m)`, a loop whose
// termination depended on the month never leaving range, proved somewhere else
// entirely. `guitk::date::days_in_month` clamps instead, so the loop that
// depended on it could not have spun even if the proof had failed. (That loop
// is gone as well; this is about what the *next* one would inherit.)

pub fn is_leap_year(year: i32) -> bool {
    date::is_leap_year(year)
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    date::days_in_month(year, month)
}

pub fn days_in_year(year: i32) -> u32 {
    if is_leap_year(year) { 366 } else { 365 }
}

pub fn month_name(month: u32) -> &'static str {
    date::month_name(month)
}

pub fn month_short(month: u32) -> &'static str {
    date::month_short_name(month)
}

/// First day-of-week for a given month (0=Sunday).
pub fn first_dow_of_month(year: i32, month: u32) -> u32 {
    Date {
        year,
        month,
        day: 1,
    }
    .day_of_week()
}

// ============================================================================
// Event categories
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCategory {
    Work,
    Personal,
    Health,
    Travel,
    Birthday,
    Holiday,
    Meeting,
    Deadline,
    Social,
    Education,
}

impl EventCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Work => "Work",
            Self::Personal => "Personal",
            Self::Health => "Health",
            Self::Travel => "Travel",
            Self::Birthday => "Birthday",
            Self::Holiday => "Holiday",
            Self::Meeting => "Meeting",
            Self::Deadline => "Deadline",
            Self::Social => "Social",
            Self::Education => "Education",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Work => BLUE,
            Self::Personal => GREEN,
            Self::Health => RED,
            Self::Travel => PEACH,
            Self::Birthday => PINK,
            Self::Holiday => YELLOW,
            Self::Meeting => MAUVE,
            Self::Deadline => RED,
            Self::Social => TEAL,
            Self::Education => SKY,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Work => "[W]",
            Self::Personal => "[P]",
            Self::Health => "[H]",
            Self::Travel => "[T]",
            Self::Birthday => "[B]",
            Self::Holiday => "[!]",
            Self::Meeting => "[M]",
            Self::Deadline => "[D]",
            Self::Social => "[S]",
            Self::Education => "[E]",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Work,
            Self::Personal,
            Self::Health,
            Self::Travel,
            Self::Birthday,
            Self::Holiday,
            Self::Meeting,
            Self::Deadline,
            Self::Social,
            Self::Education,
        ]
    }
}

// ============================================================================
// Recurrence
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecurrenceRule {
    None,
    Daily,
    Weekly { days: Vec<u32> },
    BiWeekly,
    Monthly,
    Yearly,
    Custom { interval_days: u32 },
}

impl RecurrenceRule {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "Does not repeat",
            Self::Daily => "Daily",
            Self::Weekly { .. } => "Weekly",
            Self::BiWeekly => "Every 2 weeks",
            Self::Monthly => "Monthly",
            Self::Yearly => "Yearly",
            Self::Custom { .. } => "Custom interval",
        }
    }

    /// Generate next occurrence after `from` date.
    pub fn next_occurrence(&self, from: Date) -> Option<Date> {
        match self {
            Self::None => None,
            Self::Daily => Some(from.add_days(1)),
            Self::Weekly { days } => {
                if days.is_empty() {
                    return Some(from.add_days(7));
                }
                let _current_dow = from.day_of_week();
                // Find next matching day
                for offset in 1..=7 {
                    let next = from.add_days(offset);
                    if days.contains(&next.day_of_week()) {
                        return Some(next);
                    }
                }
                // Fallback: one week
                Some(from.add_days(7))
            }
            Self::BiWeekly => Some(from.add_days(14)),
            Self::Monthly => Some(from.next_month()),
            Self::Yearly => Some(from.next_year()),
            Self::Custom { interval_days } => Some(from.add_days(*interval_days as i32)),
        }
    }

    /// Check if date matches rule relative to origin.
    pub fn matches(&self, origin: Date, check: Date) -> bool {
        if origin == check {
            return true;
        }
        if check < origin {
            return false;
        }

        match self {
            Self::None => false,
            Self::Daily => true,
            Self::Weekly { days } => {
                if days.is_empty() {
                    let diff = check.days_since(origin);
                    diff >= 0 && diff % 7 == 0
                } else {
                    days.contains(&check.day_of_week())
                }
            }
            Self::BiWeekly => {
                let diff = check.days_since(origin);
                diff >= 0 && diff % 14 == 0
            }
            Self::Monthly => check.day == origin.day && check >= origin,
            Self::Yearly => {
                check.month == origin.month && check.day == origin.day && check >= origin
            }
            Self::Custom { interval_days } => {
                if *interval_days == 0 {
                    return false;
                }
                let diff = check.days_since(origin);
                diff >= 0 && diff.checked_rem(i64::from(*interval_days)) == Some(0)
            }
        }
    }
}

// ============================================================================
// Reminders
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reminder {
    None,
    AtTime,
    MinutesBefore(u32),
    HoursBefore(u32),
    DayBefore,
}

impl Reminder {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No reminder",
            Self::AtTime => "At time of event",
            Self::MinutesBefore(5) => "5 minutes before",
            Self::MinutesBefore(10) => "10 minutes before",
            Self::MinutesBefore(15) => "15 minutes before",
            Self::MinutesBefore(30) => "30 minutes before",
            Self::MinutesBefore(_) => "Minutes before",
            Self::HoursBefore(1) => "1 hour before",
            Self::HoursBefore(_) => "Hours before",
            Self::DayBefore => "1 day before",
        }
    }

    pub fn presets() -> Vec<Self> {
        vec![
            Self::None,
            Self::AtTime,
            Self::MinutesBefore(5),
            Self::MinutesBefore(10),
            Self::MinutesBefore(15),
            Self::MinutesBefore(30),
            Self::HoursBefore(1),
            Self::DayBefore,
        ]
    }
}

// ============================================================================
// Calendar event
// ============================================================================

#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub category: EventCategory,
    pub start: DateTime,
    pub end: DateTime,
    pub all_day: bool,
    pub recurrence: RecurrenceRule,
    pub reminder: Reminder,
    pub location: Option<String>,
    pub color_override: Option<Color>,
}

impl CalendarEvent {
    pub fn effective_color(&self) -> Color {
        self.color_override.unwrap_or_else(|| self.category.color())
    }

    pub fn duration_minutes(&self) -> u32 {
        if self.all_day {
            return 24 * 60;
        }
        let start_min = self.start.time.to_minutes();
        let end_min = self.end.time.to_minutes();
        if end_min >= start_min {
            end_min.saturating_sub(start_min)
        } else {
            (24 * 60u32)
                .saturating_sub(start_min)
                .saturating_add(end_min)
        }
    }

    pub fn duration_label(&self) -> String {
        if self.all_day {
            return "All day".to_string();
        }
        let mins = self.duration_minutes();
        if mins >= 60 {
            let h = mins / 60;
            let m = mins % 60;
            if m == 0 {
                format!("{h}h")
            } else {
                format!("{h}h {m}m")
            }
        } else {
            format!("{mins}m")
        }
    }

    pub fn time_range_label(&self) -> String {
        if self.all_day {
            "All day".to_string()
        } else {
            format!(
                "{} - {}",
                self.start.time.format_12h(),
                self.end.time.format_12h()
            )
        }
    }

    pub fn occurs_on(&self, date: Date) -> bool {
        if self.all_day && self.start.date == date {
            return true;
        }
        if self.start.date == date {
            return true;
        }
        self.recurrence.matches(self.start.date, date)
    }

    /// Format as ICS VEVENT.
    pub fn to_ics(&self) -> String {
        let mut lines = Vec::new();
        lines.push("BEGIN:VEVENT".to_string());
        lines.push(format!("UID:{}-slateos@calendar", self.id));
        lines.push(format!("DTSTART:{}", self.start.format_ics()));
        lines.push(format!("DTEND:{}", self.end.format_ics()));
        lines.push(format!("SUMMARY:{}", ics_escape(&self.title)));
        if !self.description.is_empty() {
            lines.push(format!("DESCRIPTION:{}", ics_escape(&self.description)));
        }
        if let Some(loc) = &self.location {
            lines.push(format!("LOCATION:{}", ics_escape(loc)));
        }
        lines.push(format!("CATEGORIES:{}", self.category.label()));
        match &self.recurrence {
            RecurrenceRule::Daily => lines.push("RRULE:FREQ=DAILY".to_string()),
            RecurrenceRule::Weekly { days } => {
                let day_strs: Vec<&str> = days
                    .iter()
                    .filter_map(|d| match d {
                        0 => Some("SU"),
                        1 => Some("MO"),
                        2 => Some("TU"),
                        3 => Some("WE"),
                        4 => Some("TH"),
                        5 => Some("FR"),
                        6 => Some("SA"),
                        _ => None,
                    })
                    .collect();
                if day_strs.is_empty() {
                    lines.push("RRULE:FREQ=WEEKLY".to_string());
                } else {
                    lines.push(format!("RRULE:FREQ=WEEKLY;BYDAY={}", day_strs.join(",")));
                }
            }
            RecurrenceRule::Monthly => lines.push("RRULE:FREQ=MONTHLY".to_string()),
            RecurrenceRule::Yearly => lines.push("RRULE:FREQ=YEARLY".to_string()),
            RecurrenceRule::BiWeekly => lines.push("RRULE:FREQ=WEEKLY;INTERVAL=2".to_string()),
            RecurrenceRule::Custom { interval_days } => {
                lines.push(format!("RRULE:FREQ=DAILY;INTERVAL={interval_days}"));
            }
            RecurrenceRule::None => {}
        }
        lines.push("END:VEVENT".to_string());
        lines.join("\r\n")
    }
}

fn ics_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

// ============================================================================
// ICS parser (basic)
// ============================================================================

pub fn parse_ics(content: &str) -> Vec<CalendarEvent> {
    let mut events = Vec::new();
    let mut in_event = false;
    let mut title = String::new();
    let mut description = String::new();
    let mut location: Option<String> = None;
    let mut dtstart: Option<DateTime> = None;
    let mut dtend: Option<DateTime> = None;
    let mut category = EventCategory::Personal;
    let mut next_id: u64 = 1000;

    for line in content.lines() {
        let line = line.trim();
        if line == "BEGIN:VEVENT" {
            in_event = true;
            title.clear();
            description.clear();
            location = None;
            dtstart = None;
            dtend = None;
            category = EventCategory::Personal;
        } else if line == "END:VEVENT" && in_event {
            if let (Some(start), Some(end)) = (dtstart, dtend) {
                events.push(CalendarEvent {
                    id: next_id,
                    title: ics_unescape(&title),
                    description: ics_unescape(&description),
                    category,
                    start,
                    end,
                    all_day: false,
                    recurrence: RecurrenceRule::None,
                    reminder: Reminder::None,
                    location: location.as_deref().map(ics_unescape),
                    color_override: None,
                });
                next_id = next_id.saturating_add(1);
            }
            in_event = false;
        } else if in_event {
            if let Some(val) = line.strip_prefix("SUMMARY:") {
                title = val.to_string();
            } else if let Some(val) = line.strip_prefix("DESCRIPTION:") {
                description = val.to_string();
            } else if let Some(val) = line.strip_prefix("LOCATION:") {
                location = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("DTSTART:") {
                dtstart = parse_ics_datetime(val);
            } else if let Some(val) = line.strip_prefix("DTEND:") {
                dtend = parse_ics_datetime(val);
            } else if let Some(val) = line.strip_prefix("CATEGORIES:") {
                category = match val.to_ascii_lowercase().as_str() {
                    "work" => EventCategory::Work,
                    "health" => EventCategory::Health,
                    "travel" => EventCategory::Travel,
                    "birthday" => EventCategory::Birthday,
                    "holiday" => EventCategory::Holiday,
                    "meeting" => EventCategory::Meeting,
                    "deadline" => EventCategory::Deadline,
                    "social" => EventCategory::Social,
                    "education" => EventCategory::Education,
                    _ => EventCategory::Personal,
                };
            }
        }
    }

    events
}

fn parse_ics_datetime(s: &str) -> Option<DateTime> {
    // Format: YYYYMMDDTHHMMSS or YYYYMMDD
    let s = s.trim();
    if s.len() < 8 {
        return None;
    }
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(4..6)?.parse().ok()?;
    let day: u32 = s.get(6..8)?.parse().ok()?;

    let date = Date::new(year, month, day)?;

    let time = if s.len() >= 15 && s.as_bytes().get(8) == Some(&b'T') {
        let hour: u32 = s.get(9..11)?.parse().ok()?;
        let minute: u32 = s.get(11..13)?.parse().ok()?;
        Time::new(hour, minute)?
    } else {
        Time { hour: 0, minute: 0 }
    };

    Some(DateTime { date, time })
}

fn ics_unescape(s: &str) -> String {
    // Single left-to-right pass. Chained `.replace()` is incorrect here: e.g.
    // an escaped backslash followed by a literal 'n' ("\\n") would be matched as
    // a "\n" newline escape by an earlier pass, corrupting the round-trip. RFC
    // 5545 defines the escapes \\, \;, \, and \n/\N (both mean newline).
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => result.push('\n'),
                Some(';') => result.push(';'),
                Some(',') => result.push(','),
                Some('\\') => result.push('\\'),
                // Unknown escape: malformed input — keep the following char as-is.
                Some(other) => result.push(other),
                // Trailing backslash with nothing after it: keep it literally.
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Generate ICS calendar file from events.
pub fn generate_ics(events: &[CalendarEvent], calendar_name: &str) -> String {
    let mut lines = Vec::new();
    lines.push("BEGIN:VCALENDAR".to_string());
    lines.push("VERSION:2.0".to_string());
    lines.push("PRODID:-//SlateOS//Calendar//EN".to_string());
    lines.push(format!("X-WR-CALNAME:{calendar_name}"));

    for event in events {
        lines.push(event.to_ics());
    }

    lines.push("END:VCALENDAR".to_string());
    lines.join("\r\n")
}

// ============================================================================
// Calendar store
// ============================================================================

/// The event store with CRUD operations and querying.
pub struct EventStore {
    events: Vec<CalendarEvent>,
    next_id: u64,
}

impl EventStore {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_id: 1,
        }
    }

    pub fn add(&mut self, mut event: CalendarEvent) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        event.id = id;
        self.events.push(event);
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let len_before = self.events.len();
        self.events.retain(|e| e.id != id);
        self.events.len() < len_before
    }

    pub fn get(&self, id: u64) -> Option<&CalendarEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut CalendarEvent> {
        self.events.iter_mut().find(|e| e.id == id)
    }

    pub fn all(&self) -> &[CalendarEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get all events occurring on a given date.
    pub fn events_on(&self, date: Date) -> Vec<&CalendarEvent> {
        self.events.iter().filter(|e| e.occurs_on(date)).collect()
    }

    /// Events in a date range (inclusive).
    pub fn events_in_range(&self, start: Date, end: Date) -> Vec<&CalendarEvent> {
        let mut result = Vec::new();
        let mut d = start;
        while d <= end {
            for ev in &self.events {
                if ev.occurs_on(d) && !result.iter().any(|e: &&CalendarEvent| e.id == ev.id) {
                    result.push(ev);
                }
            }
            d = d.add_days(1);
        }
        result
    }

    /// Events filtered by category.
    pub fn events_by_category(&self, cat: EventCategory) -> Vec<&CalendarEvent> {
        self.events.iter().filter(|e| e.category == cat).collect()
    }

    /// Search events by title/description.
    pub fn search(&self, query: &str) -> Vec<&CalendarEvent> {
        let lower = query.to_ascii_lowercase();
        self.events
            .iter()
            .filter(|e| {
                e.title.to_ascii_lowercase().contains(&lower)
                    || e.description.to_ascii_lowercase().contains(&lower)
            })
            .collect()
    }

    /// Upcoming events from a date, sorted.
    pub fn upcoming(&self, from: Date, limit: usize) -> Vec<&CalendarEvent> {
        let mut upcoming: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|e| e.start.date >= from)
            .collect();
        upcoming.sort_by_key(|a| a.start);
        upcoming.truncate(limit);
        upcoming
    }

    /// Import events from ICS content.
    pub fn import_ics(&mut self, content: &str) -> usize {
        let imported = parse_ics(content);
        let count = imported.len();
        for mut event in imported {
            event.id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            self.events.push(event);
        }
        count
    }

    /// Export all events as ICS.
    pub fn export_ics(&self, calendar_name: &str) -> String {
        generate_ics(&self.events, calendar_name)
    }
}

impl Default for EventStore {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Calendar views
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarView {
    Month,
    Week,
    Day,
    Year,
    Agenda,
}

impl CalendarView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Month => "Month",
            Self::Week => "Week",
            Self::Day => "Day",
            Self::Year => "Year",
            Self::Agenda => "Agenda",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Month, Self::Week, Self::Day, Self::Year, Self::Agenda]
    }
}

// ============================================================================
// Layout
// ============================================================================
//
// Every number deciding *where* something is drawn is worked out here, once
// per frame, from the size the window actually has. Nothing remembers it: a
// `Layout` is built, drawn from, hit-tested through, and dropped.
//
// The chrome is allocated right to left, because the view selector is the only
// way to change view and so must survive any width, while the header text is
// merely a caption for a view that is already on screen. So the tabs are
// placed first (shrinking their pitch rather than running off the edge), then
// the search box if what is left can spare it, and the header gets the
// remainder or is dropped.

/// A clickable thing in the calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Step the view one month/week/day/year backwards.
    NavBack,
    /// Step the view one month/week/day/year forwards.
    NavForward,
    /// Jump back to the real today.
    TodayButton,
    /// One of the five view selector tabs, indexing [`CalendarView::all`].
    ViewTab(usize),
    /// The search box in the top bar.
    SearchField,
    /// The mini calendar's month header: back a month, forward a month.
    MiniPrevMonth,
    MiniNextMonth,
    /// A day cell in the sidebar's mini calendar.
    MiniDay(Date),
    /// One of the sidebar's category swatches, indexing [`EventCategory::all`].
    CategoryFilter(usize),
    /// A day cell in the month, week or year view.
    Day(Date),
    /// A painted event, by its store id.
    Event(u64),
}

/// One frame of this app's drawing, carrying the boxes it recorded.
pub type Frame = guitk::frame::Frame<Target>;

/// Height of the top chrome bar.
const TOP_BAR_H: f32 = 48.0;
/// Where the content area begins: the bar, plus the separator line under it.
const CONTENT_Y: f32 = 50.0;
/// Width of the sidebar when it is shown.
const SIDEBAR_W: f32 = 220.0;
/// The narrowest content area worth keeping. Below this the sidebar goes: a
/// month grid squeezed into 200px is seven columns of nothing.
const MIN_CONTENT_W: f32 = 320.0;
/// Full pitch of a view selector tab, and the gap inside it.
const VIEW_TAB_PITCH: f32 = 68.0;
const VIEW_TAB_GAP: f32 = 4.0;
/// The narrowest a tab may be squeezed to before it stops being a target.
const MIN_VIEW_TAB_PITCH: f32 = 26.0;
/// Width of the search box, and the least header worth painting.
const SEARCH_W: f32 = 160.0;
const MIN_HEADER_W: f32 = 60.0;
/// Left edge of the header text, immediately right of the Today button.
const HEADER_X: f32 = 160.0;
/// Right edge of the Today button, which nothing may be placed left of.
const CHROME_LEFT: f32 = 152.0;
/// Height of a control in the top bar, and its top edge.
const CHROME_Y: f32 = 10.0;
const CHROME_H: f32 = 28.0;

/// The size the window opens at.
const DEFAULT_WIDTH: f32 = 1280.0;
const DEFAULT_HEIGHT: f32 = 720.0;

// Geometry of the five views. Each view's content has an intrinsic height,
// which is what decides whether it scrolls: a month grid stretches to fill the
// area it is given and normally does not, while a day view is twenty-four
// hours of grid and in a 720px window always does.
/// The day-of-week strip above the month grid.
const MONTH_HEADER_H: f32 = 24.0;
/// The shortest a month-grid row may be before the grid starts to scroll
/// instead. Below this a day number and one event stop fitting together.
const MONTH_MIN_ROW_H: f32 = 56.0;
/// Week view: the day header strip, one hour of grid, the time gutter.
const WEEK_HEADER_H: f32 = 40.0;
const WEEK_HOUR_H: f32 = 48.0;
const WEEK_TIME_COL_W: f32 = 50.0;
/// Day view: the date header, one hour of grid, the time gutter, and the
/// height of one all-day event's row in the band under the header.
const DAY_HEADER_H: f32 = 36.0;
const DAY_HOUR_H: f32 = 60.0;
const DAY_TIME_COL_W: f32 = 60.0;
const DAY_ALL_DAY_ROW_H: f32 = 28.0;
/// Year view: twelve months in a fixed 4x3 arrangement, each no shorter than
/// six rows of day numbers plus its name.
const YEAR_COLS: usize = 4;
const YEAR_ROWS: usize = 3;
const YEAR_MIN_MONTH_H: f32 = 120.0;
/// Agenda: where the list starts under its own caption, and the vertical
/// advance of a date header, an event card, and the gap between date groups.
const AGENDA_TOP: f32 = 36.0;
const AGENDA_HEADER_H: f32 = 26.0;
const AGENDA_EVENT_H: f32 = 46.0;
const AGENDA_GROUP_GAP: f32 = 8.0;

/// Where everything in the window is, for one particular window size.
pub struct Layout {
    pub window: Rect,
    pub top_bar: Rect,
    pub nav_back: Rect,
    pub nav_forward: Rect,
    pub today_button: Rect,
    /// The header caption, or `None` in a window too narrow to spare the room.
    pub header: Option<Rect>,
    /// The search box, or `None` in a window too narrow to spare the room.
    pub search: Option<Rect>,
    /// Left edge of the view selector, and the pitch its tabs are spaced at.
    pub view_tabs_x: f32,
    pub view_tab_pitch: f32,
    /// The sidebar, or `None` when hidden or when the content could not spare it.
    pub sidebar: Option<Rect>,
    /// Everything below the top bar and right of the sidebar.
    pub content: Rect,
}

impl Layout {
    /// Work out where everything goes in a `width` x `height` window.
    pub fn new(width: f32, height: f32, sidebar_wanted: bool) -> Self {
        let window = Rect::new(0.0, 0.0, width, height);
        let top_bar = Rect::new(0.0, 0.0, width, TOP_BAR_H.min(height));

        // The view selector, from the right edge inwards. It never wraps and
        // never runs off the window: when the room is short the tabs get
        // narrower, because a tab that is off-screen cannot be pressed and
        // there is no other way to change view.
        let count = CalendarView::all().len() as f32;
        let room = (width - 8.0 - CHROME_LEFT).max(0.0);
        let view_tab_pitch = (room / count).clamp(MIN_VIEW_TAB_PITCH, VIEW_TAB_PITCH);
        let tabs_run = view_tab_pitch * count;
        let view_tabs_x = (width - 8.0 - tabs_run).max(CHROME_LEFT);

        // What is left between the Today button and the tabs, spent on the
        // search box first and the caption second.
        let mut right = view_tabs_x - 8.0;
        let search = if right - HEADER_X - MIN_HEADER_W >= SEARCH_W {
            let rect = Rect::new(right - SEARCH_W, CHROME_Y, SEARCH_W, CHROME_H);
            right = rect.x - 8.0;
            Some(rect)
        } else {
            None
        };
        let header_w = right - HEADER_X;
        let header = if header_w >= MIN_HEADER_W {
            Some(Rect::new(HEADER_X, CHROME_Y, header_w, CHROME_H))
        } else {
            None
        };

        let content_top = CONTENT_Y.min(height);
        let sidebar = if sidebar_wanted && width - SIDEBAR_W >= MIN_CONTENT_W {
            Some(Rect::new(
                0.0,
                content_top,
                SIDEBAR_W,
                (height - content_top).max(0.0),
            ))
        } else {
            None
        };
        let content_x = sidebar.map_or(0.0, Rect::right);
        let content = Rect::new(
            content_x,
            content_top,
            (width - content_x).max(0.0),
            (height - content_top).max(0.0),
        );

        Self {
            window,
            top_bar,
            nav_back: Rect::new(8.0, 8.0, 32.0, 32.0),
            nav_forward: Rect::new(44.0, 8.0, 32.0, 32.0),
            today_button: Rect::new(84.0, CHROME_Y, 60.0, CHROME_H),
            header,
            search,
            view_tabs_x,
            view_tab_pitch,
            sidebar,
            content,
        }
    }

    /// The box for view selector tab `index`.
    pub fn view_tab(&self, index: usize) -> Rect {
        Rect::new(
            self.view_tabs_x + index as f32 * self.view_tab_pitch,
            CHROME_Y,
            (self.view_tab_pitch - VIEW_TAB_GAP).max(1.0),
            CHROME_H,
        )
    }

    /// The mini calendar's box inside the sidebar, if there is a sidebar.
    ///
    /// 142 tall: an 18px month header, a 16px day-of-week strip, and six 18px
    /// rows of days — the most any month needs.
    pub fn mini_calendar(&self) -> Option<Rect> {
        self.sidebar
            .map(|bar| Rect::new(bar.x + 10.0, bar.y + 10.0, 200.0, 142.0))
    }

    /// The box for category swatch `index` in the sidebar, if there is one.
    ///
    /// A short window leaves the last rows hanging below the sidebar. They are
    /// not special-cased here: the sidebar is drawn inside a clip, which drops
    /// both the ink and the hit box, so the rule lives in exactly one place.
    pub fn category_row(&self, index: usize) -> Option<Rect> {
        let bar = self.sidebar?;
        Some(Rect::new(
            bar.x + 8.0,
            bar.y + 210.0 + 22.0 + index as f32 * 24.0 - 2.0,
            bar.w - 16.0,
            20.0,
        ))
    }
}

// ============================================================================
// Main calendar application
// ============================================================================

pub struct CalendarApp {
    pub width: f32,
    pub height: f32,

    // Current view
    pub view: CalendarView,
    pub today: Date,
    pub selected_date: Date,
    pub view_date: Date,

    // Events
    pub store: EventStore,

    // UI state
    pub sidebar_visible: bool,
    pub search_query: String,
    pub search_results: Vec<u64>,
    pub selected_event_id: Option<u64>,
    pub category_filter: Option<EventCategory>,

    // Mini calendar
    pub mini_cal_month: u32,
    pub mini_cal_year: i32,

    // Time format
    pub use_24h: bool,
    pub week_starts_monday: bool,

    /// How far the content area is scrolled down, in pixels.
    ///
    /// The week and day views draw a full twenty-four hours — 1152px and
    /// 1440px of grid. In a 720px window that put everything after early
    /// afternoon below the bottom edge with no way to reach it, which nobody
    /// noticed because there was no window and no wheel.
    pub content_scroll: f32,
    /// Whether typing goes to the search box.
    pub search_focused: bool,
    /// Cleared when the window is closed, which is what stops the loop.
    pub running: bool,
}

impl CalendarApp {
    pub fn new(width: f32, height: f32, today: Date) -> Self {
        Self {
            width,
            height,
            view: CalendarView::Month,
            today,
            selected_date: today,
            view_date: today,
            store: EventStore::new(),
            sidebar_visible: true,
            search_query: String::new(),
            search_results: Vec::new(),
            selected_event_id: None,
            category_filter: None,
            mini_cal_month: today.month,
            mini_cal_year: today.year,
            use_24h: false,
            week_starts_monday: true,
            content_scroll: 0.0,
            search_focused: false,
            running: true,
        }
    }

    // Navigation
    pub fn go_to_today(&mut self) {
        self.selected_date = self.today;
        self.view_date = self.today;
        self.mini_cal_month = self.today.month;
        self.mini_cal_year = self.today.year;
    }

    pub fn navigate_forward(&mut self) {
        match self.view {
            CalendarView::Month => {
                self.view_date = self.view_date.next_month();
            }
            CalendarView::Week => {
                self.view_date = self.view_date.add_days(7);
            }
            CalendarView::Day => {
                self.view_date = self.view_date.add_days(1);
            }
            CalendarView::Year => {
                self.view_date = self.view_date.next_year();
            }
            CalendarView::Agenda => {
                self.view_date = self.view_date.add_days(30);
            }
        }
    }

    pub fn navigate_backward(&mut self) {
        match self.view {
            CalendarView::Month => {
                self.view_date = self.view_date.prev_month();
            }
            CalendarView::Week => {
                self.view_date = self.view_date.add_days(-7);
            }
            CalendarView::Day => {
                self.view_date = self.view_date.add_days(-1);
            }
            CalendarView::Year => {
                self.view_date = self.view_date.prev_year();
            }
            CalendarView::Agenda => {
                self.view_date = self.view_date.add_days(-30);
            }
        }
    }

    pub fn select_date(&mut self, date: Date) {
        self.selected_date = date;
        self.view_date = date;
        // The mini calendar follows the selection. Without this, clicking into
        // next month from the month grid left the sidebar showing the old one,
        // with the selection highlight nowhere in it.
        self.mini_cal_month = date.month;
        self.mini_cal_year = date.year;
    }

    pub fn search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
        } else {
            self.search_results = self
                .store
                .search(&self.search_query)
                .iter()
                .map(|e| e.id)
                .collect();
        }
    }

    // ========================================================================
    // Geometry
    // ========================================================================

    /// Adopt a new window size.
    ///
    /// The scroll offset is re-clamped here rather than at the next wheel
    /// event: growing the window can make the content fit, and a stale offset
    /// would leave a gap at the bottom that nothing could scroll back up.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        self.clamp_scroll();
    }

    /// Where everything goes at the current window size.
    pub fn layout(&self) -> Layout {
        Layout::new(self.width, self.height, self.sidebar_visible)
    }

    fn week_start(&self, date: Date) -> Date {
        let dow = date.day_of_week();
        let offset = if self.week_starts_monday {
            dow.checked_sub(1).unwrap_or(6)
        } else {
            dow
        };
        // `offset` is 0..=6, so the negation is exact; spelled `wrapping_neg`
        // because a bare `-` on a signed value is an overflow the lint counts.
        date.add_days((offset as i32).wrapping_neg())
    }

    /// How many blank cells precede the 1st of a month in a day grid.
    ///
    /// This used to be written out three times — mini calendar, month view,
    /// year view — as the same `if first_dow == 0 { 6 } else { first_dow - 1 }`.
    fn start_offset(&self, first_dow: u32) -> u32 {
        if self.week_starts_monday {
            first_dow.checked_sub(1).unwrap_or(6)
        } else {
            first_dow
        }
    }

    /// Step the sidebar's mini calendar one month forwards or backwards.
    pub fn step_mini_month(&mut self, delta: i32) {
        // Counted as months since year zero, so a step across December is
        // a division rather than a loop that has to be got right twice.
        let months = i64::from(self.mini_cal_year)
            .saturating_mul(12)
            .saturating_add(i64::from(self.mini_cal_month))
            .saturating_sub(1)
            .saturating_add(i64::from(delta));
        self.mini_cal_year = i32::try_from(months.div_euclid(12)).unwrap_or(self.mini_cal_year);
        self.mini_cal_month = u32::try_from(months.rem_euclid(12).saturating_add(1)).unwrap_or(1);
    }

    /// The events on `date` that the category filter lets through.
    ///
    /// Every view used to call `store.events_on` directly, so picking a
    /// category greyed out the other swatches in the sidebar and changed
    /// nothing else on screen. Nobody noticed because nothing could set the
    /// filter: there was no click handling at all.
    pub fn visible_events_on(&self, date: Date) -> Vec<&CalendarEvent> {
        self.store
            .events_on(date)
            .into_iter()
            .filter(|e| self.category_filter.is_none_or(|c| e.category == c))
            .collect()
    }

    /// What the agenda lists: the search results when a query is typed, the
    /// next thirty upcoming events otherwise, category-filtered either way.
    ///
    /// `search_results` was computed and then never read by anything.
    pub fn agenda_events(&self) -> Vec<&CalendarEvent> {
        let base: Vec<&CalendarEvent> = if self.search_query.is_empty() {
            self.store.upcoming(self.view_date, 30)
        } else {
            self.search_results
                .iter()
                .filter_map(|id| self.store.get(*id))
                .collect()
        };
        base.into_iter()
            .filter(|e| self.category_filter.is_none_or(|c| e.category == c))
            .collect()
    }

    /// How tall the agenda list is, walked exactly the way it is drawn so the
    /// scroll extent and the ink cannot drift apart.
    fn agenda_height(&self, events: &[&CalendarEvent]) -> f32 {
        let mut y = AGENDA_TOP;
        let mut last: Option<Date> = Option::None;
        for ev in events {
            if last != Some(ev.start.date) {
                if last.is_some() {
                    y += AGENDA_GROUP_GAP;
                }
                y += AGENDA_HEADER_H;
                last = Some(ev.start.date);
            }
            y += AGENDA_EVENT_H;
        }
        y + 8.0
    }

    /// Rows in the month grid: five at minimum, six or seven when the month
    /// spills.
    fn month_rows(&self) -> u32 {
        let first_dow = first_dow_of_month(self.view_date.year, self.view_date.month);
        let cells = self
            .start_offset(first_dow)
            .saturating_add(days_in_month(self.view_date.year, self.view_date.month));
        cells.div_ceil(7).max(5)
    }

    /// Height of one month-grid row: the area shared out, but never squeezed
    /// below the point where a day number and one event stop fitting.
    fn month_row_h(&self, area: Rect) -> f32 {
        ((area.h - MONTH_HEADER_H) / self.month_rows() as f32).max(MONTH_MIN_ROW_H)
    }

    /// Height of one month block in the year view, floored the same way.
    fn year_month_h(&self, area: Rect) -> f32 {
        ((area.h - 10.0) / YEAR_ROWS as f32).max(YEAR_MIN_MONTH_H)
    }

    /// Height of the day view's all-day band, which is zero when empty.
    fn all_day_band_h(&self) -> f32 {
        let count = self
            .visible_events_on(self.view_date)
            .iter()
            .filter(|e| e.all_day)
            .count();
        if count == 0 {
            0.0
        } else {
            DAY_ALL_DAY_ROW_H * count as f32 + 8.0
        }
    }

    /// How tall the current view's content is, which is what makes it
    /// scrollable when it exceeds `area.h`.
    fn content_height(&self, area: Rect) -> f32 {
        match self.view {
            CalendarView::Month => {
                MONTH_HEADER_H + self.month_row_h(area) * self.month_rows() as f32
            }
            CalendarView::Week => WEEK_HEADER_H + 24.0 * WEEK_HOUR_H,
            CalendarView::Day => DAY_HEADER_H + self.all_day_band_h() + 24.0 * DAY_HOUR_H,
            CalendarView::Year => 10.0 + self.year_month_h(area) * YEAR_ROWS as f32,
            CalendarView::Agenda => self.agenda_height(&self.agenda_events()),
        }
    }

    /// The furthest the content can be scrolled: zero when it already fits.
    pub fn max_content_scroll(&self) -> f32 {
        let area = self.layout().content;
        (self.content_height(area) - area.h).max(0.0)
    }

    fn clamp_scroll(&mut self) {
        self.content_scroll = self.content_scroll.clamp(0.0, self.max_content_scroll());
    }

    /// What is under the pointer, according to the boxes the last frame
    /// recorded as it painted them.
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    /// The caption in the top bar, naming whatever the current view shows.
    fn header_text(&self) -> String {
        match self.view {
            CalendarView::Month | CalendarView::Year => self.view_date.format_header(),
            CalendarView::Week => {
                let start = self.week_start(self.view_date);
                let end = start.add_days(6);
                if start.month == end.month {
                    format!(
                        "{} {}-{}, {}",
                        start.month_name(),
                        start.day,
                        end.day,
                        start.year
                    )
                } else {
                    format!(
                        "{} {} - {} {}, {}",
                        start.month_short(),
                        start.day,
                        end.month_short(),
                        end.day,
                        start.year
                    )
                }
            }
            CalendarView::Day => self.view_date.format_long(),
            CalendarView::Agenda => {
                if self.search_query.is_empty() {
                    format!("Agenda from {}", self.view_date.format_short())
                } else {
                    format!("Search: {}", self.search_query)
                }
            }
        }
    }

    fn day_headers_short(&self) -> [&'static str; 7] {
        if self.week_starts_monday {
            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
        } else {
            ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
        }
    }

    fn day_headers_long(&self) -> [&'static str; 7] {
        if self.week_starts_monday {
            [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ]
        } else {
            [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ]
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Draw the whole calendar, recording a hit box for everything clickable
    /// at the moment it is painted.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let mut frame = Frame::new(width, height);
        let layout = Layout::new(width, height, self.sidebar_visible);

        fill(&mut frame, layout.window, BASE, 0.0);
        self.draw_top_bar(&mut frame, &layout);

        if let Some(bar) = layout.sidebar {
            // Clipped, so a category row pushed below a short sidebar is
            // neither painted nor clickable. The rule lives here once instead
            // of being re-derived by whatever wants to know.
            frame.clip(bar);
            self.draw_sidebar(&mut frame, &layout, bar);
            frame.unclip();
        }

        let area = layout.content;
        let scroll = self
            .content_scroll
            .clamp(0.0, (self.content_height(area) - area.h).max(0.0));
        frame.clip(area);
        frame.translate(0.0, -scroll);
        match self.view {
            CalendarView::Month => self.draw_month_view(&mut frame, area),
            CalendarView::Week => self.draw_week_view(&mut frame, area),
            CalendarView::Day => self.draw_day_view(&mut frame, area),
            CalendarView::Year => self.draw_year_view(&mut frame, area),
            CalendarView::Agenda => self.draw_agenda_view(&mut frame, area),
        }
        frame.untranslate();
        frame.unclip();

        frame
    }

    fn draw_top_bar(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.top_bar, MANTLE, 0.0);

        draw_nav_button(frame, layout.nav_back, "<", Target::NavBack);
        draw_nav_button(frame, layout.nav_forward, ">", Target::NavForward);

        let today = layout.today_button;
        fill(frame, today, BLUE, 4.0);
        label(
            frame,
            today.x + 12.0,
            today.y + 8.0,
            "Today",
            12.0,
            CRUST,
            FontWeightHint::Bold,
            Some(today.w - 8.0),
        );
        frame.hit(Target::TodayButton, today);

        if let Some(header) = layout.header {
            label(
                frame,
                header.x,
                header.y + 6.0,
                self.header_text(),
                16.0,
                TEXT,
                FontWeightHint::Bold,
                Some(header.w),
            );
        }

        if let Some(search) = layout.search {
            fill(frame, search, SURFACE0, 4.0);
            if self.search_focused {
                stroke(frame, search, BLUE, 4.0, 1.5);
            }
            let empty = self.search_query.is_empty();
            label(
                frame,
                search.x + 8.0,
                search.y + 8.0,
                if empty {
                    String::from("Search events")
                } else {
                    self.search_query.clone()
                },
                11.0,
                if empty { OVERLAY0 } else { TEXT },
                FontWeightHint::Regular,
                Some(search.w - 16.0),
            );
            frame.hit(Target::SearchField, search);
        }

        for (i, view) in CalendarView::all().iter().enumerate() {
            let tab = layout.view_tab(i);
            let active = *view == self.view;
            fill(frame, tab, if active { SURFACE0 } else { MANTLE }, 4.0);
            label(
                frame,
                tab.x + 8.0,
                tab.y + 8.0,
                view.label(),
                11.0,
                if active { BLUE } else { SUBTEXT0 },
                if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                Some((tab.w - 12.0).max(1.0)),
            );
            frame.hit(Target::ViewTab(i), tab);
        }

        let sep = layout.top_bar.bottom();
        line(frame, 0.0, sep, layout.window.w, sep, SURFACE0, 1.0);
    }

    fn draw_sidebar(&self, frame: &mut Frame, layout: &Layout, bar: Rect) {
        fill(frame, bar, MANTLE, 0.0);

        if let Some(mini) = layout.mini_calendar() {
            self.draw_mini_calendar(frame, mini);
        }

        label(
            frame,
            bar.x + 12.0,
            bar.y + 210.0,
            "Categories",
            12.0,
            SUBTEXT0,
            FontWeightHint::Bold,
            Some(bar.w - 20.0),
        );

        for (i, cat) in EventCategory::all().iter().enumerate() {
            let Some(row) = layout.category_row(i) else {
                continue;
            };
            let active = self.category_filter.is_none() || self.category_filter == Some(*cat);
            fill(
                frame,
                Rect::new(row.x + 4.0, row.y + 4.0, 12.0, 12.0),
                if active { cat.color() } else { SURFACE0 },
                2.0,
            );
            label(
                frame,
                row.x + 22.0,
                row.y + 4.0,
                cat.label(),
                11.0,
                if active { TEXT } else { OVERLAY0 },
                FontWeightHint::Regular,
                Some((row.w - 60.0).max(1.0)),
            );
            let count = self.store.events_by_category(*cat).len();
            if count > 0 {
                label(
                    frame,
                    row.right() - 28.0,
                    row.y + 4.0,
                    count.to_string(),
                    10.0,
                    OVERLAY0,
                    FontWeightHint::Regular,
                    Option::None,
                );
            }
            frame.hit(Target::CategoryFilter(i), row);
        }

        let edge = bar.right() - 0.5;
        line(frame, edge, bar.y, edge, bar.bottom(), SURFACE0, 1.0);
    }

    fn draw_mini_calendar(&self, frame: &mut Frame, area: Rect) {
        let cell_w = area.w / 7.0;
        let cell_h = 18.0;

        // The two month-step arrows, which is what makes the mini calendar a
        // way to reach another month rather than a picture of this one.
        let prev = Rect::new(area.x, area.y - 2.0, 18.0, 18.0);
        let next = Rect::new(area.right() - 18.0, area.y - 2.0, 18.0, 18.0);
        label(
            frame,
            prev.x + 5.0,
            area.y,
            "<",
            11.0,
            SUBTEXT0,
            FontWeightHint::Bold,
            Option::None,
        );
        label(
            frame,
            next.x + 5.0,
            area.y,
            ">",
            11.0,
            SUBTEXT0,
            FontWeightHint::Bold,
            Option::None,
        );
        frame.hit(Target::MiniPrevMonth, prev);
        frame.hit(Target::MiniNextMonth, next);

        label(
            frame,
            area.x + area.w / 2.0 - 30.0,
            area.y,
            format!(
                "{} {}",
                month_short(self.mini_cal_month),
                self.mini_cal_year
            ),
            11.0,
            TEXT,
            FontWeightHint::Bold,
            Some(area.w - 40.0),
        );

        let header_y = area.y + 18.0;
        for (i, dh) in self.day_headers_short().iter().enumerate() {
            label(
                frame,
                area.x + i as f32 * cell_w + 2.0,
                header_y,
                *dh,
                9.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(cell_w),
            );
        }

        let first_dow = first_dow_of_month(self.mini_cal_year, self.mini_cal_month);
        let start_offset = self.start_offset(first_dow);
        let total_days = days_in_month(self.mini_cal_year, self.mini_cal_month);
        let grid_y = header_y + 16.0;

        for day in 1..=total_days {
            let pos = day.saturating_sub(1).saturating_add(start_offset) as usize;
            let cell = Rect::new(
                area.x + (pos % 7) as f32 * cell_w,
                grid_y + (pos / 7) as f32 * cell_h,
                cell_w,
                cell_h,
            );

            let date = Date {
                year: self.mini_cal_year,
                month: self.mini_cal_month,
                day,
            };
            let is_today = date.is_today(self.today);
            let is_selected = date == self.selected_date;
            let has_events = !self.visible_events_on(date).is_empty();

            if is_today || is_selected {
                fill(
                    frame,
                    Rect::new(cell.x, cell.y - 1.0, cell_w - 1.0, cell_h - 2.0),
                    if is_today { BLUE } else { SURFACE0 },
                    3.0,
                );
            }

            let fg = if is_today {
                CRUST
            } else if is_selected {
                TEXT
            } else if date.is_weekend() {
                SUBTEXT0
            } else {
                TEXT
            };
            label(
                frame,
                cell.x + 4.0,
                cell.y + 1.0,
                day.to_string(),
                10.0,
                fg,
                if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                Some(cell_w - 4.0),
            );

            if has_events && !is_today {
                fill(
                    frame,
                    Rect::new(cell.x + cell_w / 2.0 - 2.0, cell.bottom() - 5.0, 4.0, 3.0),
                    PEACH,
                    1.5,
                );
            }

            frame.hit(Target::MiniDay(date), cell);
        }
    }

    fn draw_month_view(&self, frame: &mut Frame, area: Rect) {
        let col_w = area.w / 7.0;

        for (i, dh) in self.day_headers_long().iter().enumerate() {
            label(
                frame,
                area.x + i as f32 * col_w + 8.0,
                area.y + 6.0,
                *dh,
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some((col_w - 12.0).max(1.0)),
            );
        }

        let grid_y = area.y + MONTH_HEADER_H;
        let row_h = self.month_row_h(area);
        let first_dow = first_dow_of_month(self.view_date.year, self.view_date.month);
        let start_offset = self.start_offset(first_dow);
        let total_days = days_in_month(self.view_date.year, self.view_date.month);

        for day in 1..=total_days {
            let pos = day.saturating_sub(1).saturating_add(start_offset) as usize;
            let cell = Rect::new(
                area.x + (pos % 7) as f32 * col_w,
                grid_y + (pos / 7) as f32 * row_h,
                col_w,
                row_h,
            );

            let date = Date {
                year: self.view_date.year,
                month: self.view_date.month,
                day,
            };
            let is_today = date.is_today(self.today);
            let is_selected = date == self.selected_date;

            stroke(frame, cell, SURFACE0, 0.0, 0.5);

            if is_today || is_selected {
                fill(
                    frame,
                    Rect::new(cell.x + 4.0, cell.y + 2.0, 22.0, 18.0),
                    if is_today { BLUE } else { SURFACE1 },
                    4.0,
                );
            }

            label(
                frame,
                cell.x + 8.0,
                cell.y + 4.0,
                day.to_string(),
                12.0,
                if is_today {
                    CRUST
                } else if date.is_weekend() {
                    SUBTEXT0
                } else {
                    TEXT
                },
                if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                Some(20.0),
            );

            // The day cell first, so an event drawn on top of it also wins the
            // hit test: `hit_test` reads the recorded boxes back to front.
            frame.hit(Target::Day(date), cell);

            let events = self.visible_events_on(date);
            let max_visible = ((row_h - 24.0) / 16.0).max(0.0) as usize;
            for (ei, ev) in events.iter().enumerate().take(max_visible) {
                let chip = Rect::new(
                    cell.x + 4.0,
                    cell.y + 22.0 + ei as f32 * 16.0,
                    (col_w - 8.0).max(1.0),
                    14.0,
                );
                fill(frame, chip, ev.effective_color(), 2.0);
                let prefix = if ev.all_day {
                    String::new()
                } else {
                    format!("{} ", ev.start.time.format_12h())
                };
                label(
                    frame,
                    chip.x + 3.0,
                    chip.y + 2.0,
                    format!("{prefix}{}", ev.title),
                    9.0,
                    CRUST,
                    FontWeightHint::Bold,
                    Some((chip.w - 6.0).max(1.0)),
                );
                if self.selected_event_id == Some(ev.id) {
                    stroke(frame, chip, TEXT, 2.0, 1.5);
                }
                frame.hit(Target::Event(ev.id), chip);
            }

            if events.len() > max_visible {
                label(
                    frame,
                    cell.x + 8.0,
                    cell.y + 22.0 + max_visible as f32 * 16.0,
                    format!("+{} more", events.len().saturating_sub(max_visible)),
                    9.0,
                    OVERLAY0,
                    FontWeightHint::Regular,
                    Some((col_w - 16.0).max(1.0)),
                );
            }
        }
    }

    fn draw_week_view(&self, frame: &mut Frame, area: Rect) {
        let week_start = self.week_start(self.view_date);
        let day_w = (area.w - WEEK_TIME_COL_W) / 7.0;

        for i in 0..7 {
            let date = week_start.add_days(i);
            let header = Rect::new(
                area.x + WEEK_TIME_COL_W + i as f32 * day_w,
                area.y,
                day_w,
                WEEK_HEADER_H,
            );
            let is_today = date.is_today(self.today);
            fill(frame, header, if is_today { SURFACE0 } else { MANTLE }, 0.0);
            label(
                frame,
                header.x + 4.0,
                header.y + 4.0,
                date.day_of_week_short(),
                10.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some((day_w - 8.0).max(1.0)),
            );
            label(
                frame,
                header.x + 4.0,
                header.y + 18.0,
                date.day.to_string(),
                16.0,
                if is_today { BLUE } else { TEXT },
                if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                Some((day_w - 8.0).max(1.0)),
            );
            frame.hit(Target::Day(date), header);
        }

        let grid_y = area.y + WEEK_HEADER_H;
        for hour in 0..24 {
            let hy = grid_y + hour as f32 * WEEK_HOUR_H;
            let time = Time { hour, minute: 0 };
            label(
                frame,
                area.x + 4.0,
                hy + 2.0,
                if self.use_24h {
                    time.format_24h()
                } else {
                    time.format_12h()
                },
                10.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(WEEK_TIME_COL_W - 8.0),
            );
            line(
                frame,
                area.x + WEEK_TIME_COL_W,
                hy,
                area.right(),
                hy,
                SURFACE0,
                0.5,
            );
            line(
                frame,
                area.x + WEEK_TIME_COL_W,
                hy + WEEK_HOUR_H / 2.0,
                area.right(),
                hy + WEEK_HOUR_H / 2.0,
                Color::rgba(49, 50, 68, 128),
                0.5,
            );
        }

        for i in 0..7 {
            let date = week_start.add_days(i);
            let dx = area.x + WEEK_TIME_COL_W + i as f32 * day_w;
            for ev in &self.visible_events_on(date) {
                if ev.all_day {
                    continue;
                }
                let start_min = ev.start.time.to_minutes() as f32;
                let end_min = ev.end.time.to_minutes() as f32;
                let block = Rect::new(
                    dx + 2.0,
                    grid_y + (start_min / 60.0) * WEEK_HOUR_H,
                    (day_w - 4.0).max(1.0),
                    (((end_min - start_min) / 60.0) * WEEK_HOUR_H).max(16.0),
                );
                fill(frame, block, ev.effective_color(), 3.0);
                label(
                    frame,
                    block.x + 3.0,
                    block.y + 2.0,
                    ev.title.clone(),
                    10.0,
                    CRUST,
                    FontWeightHint::Bold,
                    Some((block.w - 6.0).max(1.0)),
                );
                if block.h > 20.0 {
                    label(
                        frame,
                        block.x + 3.0,
                        block.y + 14.0,
                        ev.time_range_label(),
                        9.0,
                        CRUST,
                        FontWeightHint::Regular,
                        Some((block.w - 6.0).max(1.0)),
                    );
                }
                if self.selected_event_id == Some(ev.id) {
                    stroke(frame, block, TEXT, 3.0, 1.5);
                }
                frame.hit(Target::Event(ev.id), block);
            }
        }
    }

    fn draw_day_view(&self, frame: &mut Frame, area: Rect) {
        let is_today = self.view_date.is_today(self.today);
        let header = Rect::new(area.x, area.y, area.w, DAY_HEADER_H);
        fill(frame, header, if is_today { SURFACE0 } else { MANTLE }, 0.0);
        label(
            frame,
            header.x + 16.0,
            header.y + 10.0,
            self.view_date.format_long(),
            14.0,
            if is_today { BLUE } else { TEXT },
            FontWeightHint::Bold,
            Some((area.w - 32.0).max(1.0)),
        );
        frame.hit(Target::Day(self.view_date), header);

        let day_events = self.visible_events_on(self.view_date);
        let event_w = (area.w - DAY_TIME_COL_W - 16.0).max(1.0);

        for (i, ev) in day_events.iter().filter(|e| e.all_day).enumerate() {
            let block = Rect::new(
                area.x + DAY_TIME_COL_W,
                area.y + DAY_HEADER_H + 4.0 + i as f32 * DAY_ALL_DAY_ROW_H,
                event_w,
                24.0,
            );
            fill(frame, block, ev.effective_color(), 4.0);
            label(
                frame,
                block.x + 8.0,
                block.y + 5.0,
                format!("All day: {}", ev.title),
                11.0,
                CRUST,
                FontWeightHint::Bold,
                Some((block.w - 16.0).max(1.0)),
            );
            if self.selected_event_id == Some(ev.id) {
                stroke(frame, block, TEXT, 4.0, 1.5);
            }
            frame.hit(Target::Event(ev.id), block);
        }

        let grid_y = area.y + DAY_HEADER_H + self.all_day_band_h();
        for hour in 0..24 {
            let hy = grid_y + hour as f32 * DAY_HOUR_H;
            let time = Time { hour, minute: 0 };
            label(
                frame,
                area.x + 4.0,
                hy + 2.0,
                if self.use_24h {
                    time.format_24h()
                } else {
                    time.format_12h()
                },
                11.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(DAY_TIME_COL_W - 8.0),
            );
            line(
                frame,
                area.x + DAY_TIME_COL_W,
                hy,
                area.right(),
                hy,
                SURFACE0,
                0.5,
            );
        }

        for ev in day_events.iter().filter(|e| !e.all_day) {
            let start_min = ev.start.time.to_minutes() as f32;
            let end_min = ev.end.time.to_minutes() as f32;
            let block = Rect::new(
                area.x + DAY_TIME_COL_W + 4.0,
                grid_y + (start_min / 60.0) * DAY_HOUR_H,
                event_w,
                (((end_min - start_min) / 60.0) * DAY_HOUR_H).max(20.0),
            );
            fill(frame, block, ev.effective_color(), 4.0);
            label(
                frame,
                block.x + 6.0,
                block.y + 4.0,
                ev.title.clone(),
                12.0,
                CRUST,
                FontWeightHint::Bold,
                Some((block.w - 16.0).max(1.0)),
            );
            if block.h > 24.0 {
                label(
                    frame,
                    block.x + 6.0,
                    block.y + 18.0,
                    ev.time_range_label(),
                    10.0,
                    CRUST,
                    FontWeightHint::Regular,
                    Some((block.w - 16.0).max(1.0)),
                );
            }
            if block.h > 40.0
                && let Some(loc) = &ev.location
            {
                label(
                    frame,
                    block.x + 6.0,
                    block.y + 32.0,
                    loc.clone(),
                    10.0,
                    CRUST,
                    FontWeightHint::Regular,
                    Some((block.w - 16.0).max(1.0)),
                );
            }
            if self.selected_event_id == Some(ev.id) {
                stroke(frame, block, TEXT, 4.0, 1.5);
            }
            frame.hit(Target::Event(ev.id), block);
        }
    }

    fn draw_year_view(&self, frame: &mut Frame, area: Rect) {
        let month_w = area.w / YEAR_COLS as f32;
        let month_h = self.year_month_h(area);

        for month in 1..=12u32 {
            let index = month.saturating_sub(1) as usize;
            let mx = area.x + (index % YEAR_COLS) as f32 * month_w;
            let my = area.y + (index / YEAR_COLS) as f32 * month_h;

            let is_current_month =
                self.view_date.year == self.today.year && month == self.today.month;
            label(
                frame,
                mx + 8.0,
                my + 4.0,
                month_name(month),
                12.0,
                if is_current_month { BLUE } else { TEXT },
                FontWeightHint::Bold,
                Some((month_w - 16.0).max(1.0)),
            );

            let cell_w = (month_w - 16.0) / 7.0;
            let cell_h = 14.0;
            let grid_y = my + 22.0;
            let first_dow = first_dow_of_month(self.view_date.year, month);
            let start_offset = self.start_offset(first_dow);
            let total = days_in_month(self.view_date.year, month);

            for day in 1..=total {
                let pos = day.saturating_sub(1).saturating_add(start_offset) as usize;
                let cell = Rect::new(
                    mx + 8.0 + (pos % 7) as f32 * cell_w,
                    grid_y + (pos / 7) as f32 * cell_h,
                    cell_w,
                    cell_h,
                );

                let date = Date {
                    year: self.view_date.year,
                    month,
                    day,
                };
                let is_today = date.is_today(self.today);
                let has_events = !self.visible_events_on(date).is_empty();

                if is_today {
                    fill(
                        frame,
                        Rect::new(cell.x - 1.0, cell.y - 1.0, cell_w, cell_h - 1.0),
                        BLUE,
                        2.0,
                    );
                }

                label(
                    frame,
                    cell.x,
                    cell.y,
                    day.to_string(),
                    8.0,
                    if is_today {
                        CRUST
                    } else if has_events {
                        PEACH
                    } else if date.is_weekend() {
                        OVERLAY0
                    } else {
                        SUBTEXT0
                    },
                    FontWeightHint::Regular,
                    Some(cell_w.max(1.0)),
                );

                frame.hit(Target::Day(date), cell);
            }
        }
    }

    fn draw_agenda_view(&self, frame: &mut Frame, area: Rect) {
        let events = self.agenda_events();

        label(
            frame,
            area.x + 16.0,
            area.y + 8.0,
            if self.search_query.is_empty() {
                format!("Upcoming Events ({})", events.len())
            } else {
                format!("{} matching \"{}\"", events.len(), self.search_query)
            },
            14.0,
            TEXT,
            FontWeightHint::Bold,
            Some((area.w - 32.0).max(1.0)),
        );

        let mut row_y = area.y + AGENDA_TOP;
        let mut last_date: Option<Date> = Option::None;

        for ev in &events {
            if last_date != Some(ev.start.date) {
                if last_date.is_some() {
                    row_y += AGENDA_GROUP_GAP;
                }
                let is_today = ev.start.date.is_today(self.today);
                let head = Rect::new(area.x + 8.0, row_y, (area.w - 16.0).max(1.0), 22.0);
                fill(frame, head, if is_today { SURFACE0 } else { MANTLE }, 4.0);
                label(
                    frame,
                    head.x + 8.0,
                    head.y + 4.0,
                    if is_today {
                        format!("Today - {}", ev.start.date.format_long())
                    } else {
                        ev.start.date.format_long()
                    },
                    12.0,
                    if is_today { BLUE } else { TEXT },
                    FontWeightHint::Bold,
                    Some((head.w - 24.0).max(1.0)),
                );
                frame.hit(Target::Day(ev.start.date), head);
                row_y += AGENDA_HEADER_H;
                last_date = Some(ev.start.date);
            }

            let card = Rect::new(area.x + 16.0, row_y, (area.w - 32.0).max(1.0), 40.0);
            fill(
                frame,
                Rect::new(card.x, card.y, 4.0, card.h),
                ev.effective_color(),
                2.0,
            );
            label(
                frame,
                card.x + 12.0,
                card.y + 2.0,
                ev.title.clone(),
                13.0,
                TEXT,
                FontWeightHint::Bold,
                Some((card.w - 84.0).max(1.0)),
            );
            label(
                frame,
                card.x + 12.0,
                card.y + 18.0,
                format!(
                    "{} | {} | {}",
                    ev.time_range_label(),
                    ev.duration_label(),
                    ev.category.label()
                ),
                10.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some((card.w - 44.0).max(1.0)),
            );
            if let Some(loc) = &ev.location {
                label(
                    frame,
                    card.x + 12.0,
                    card.y + 30.0,
                    loc.clone(),
                    10.0,
                    OVERLAY0,
                    FontWeightHint::Regular,
                    Some((card.w - 44.0).max(1.0)),
                );
            }
            if self.selected_event_id == Some(ev.id) {
                stroke(frame, card, TEXT, 4.0, 1.5);
            }
            frame.hit(Target::Event(ev.id), card);

            row_y += AGENDA_EVENT_H;
        }

        if events.is_empty() {
            label(
                frame,
                area.x + area.w / 2.0 - 60.0,
                area.y + 100.0,
                if self.search_query.is_empty() {
                    "No upcoming events"
                } else {
                    "Nothing matches that search"
                },
                14.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(220.0),
            );
        }
    }
}

// ============================================================================
// Drawing helpers
// ============================================================================
//
// Four shapes cover everything this app paints. They exist so the drawing code
// reads as a description of the calendar rather than as a wall of struct
// literals with eight fields each.

fn fill(frame: &mut Frame, rect: Rect, color: Color, radius: f32) {
    frame.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

fn stroke(frame: &mut Frame, rect: Rect, color: Color, radius: f32, line_width: f32) {
    frame.push(RenderCommand::StrokeRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
        line_width,
    });
}

fn label(
    frame: &mut Frame,
    x: f32,
    y: f32,
    text: impl Into<String>,
    font_size: f32,
    color: Color,
    font_weight: FontWeightHint,
    max_width: Option<f32>,
) {
    frame.push(RenderCommand::Text {
        x,
        y,
        text: text.into(),
        font_size,
        color,
        font_weight,
        max_width,
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

fn line(frame: &mut Frame, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
    frame.push(RenderCommand::Line {
        x1,
        y1,
        x2,
        y2,
        color,
        width,
    });
}

fn draw_nav_button(frame: &mut Frame, rect: Rect, glyph: &str, target: Target) {
    fill(frame, rect, SURFACE0, 4.0);
    label(
        frame,
        rect.x + rect.w / 2.0 - 4.0,
        rect.y + rect.h / 2.0 - 6.0,
        glyph,
        14.0,
        TEXT,
        FontWeightHint::Bold,
        Option::None,
    );
    frame.hit(target, rect);
}

// ============================================================================
// Input
// ============================================================================

/// The one body both the window and the test probe drive the calendar through.
pub fn handle_event(state: &mut CalendarApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) if key.pressed => handle_key(state, key),
        Event::Mouse(mouse) => handle_mouse(state, mouse),
        Event::Resize { width, height } => {
            state.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        Event::Tick { .. } => {
            // Midnight. "Today" is drawn in blue in five different places, and
            // without this it would stay on yesterday until something else
            // happened to cause a repaint.
            match today_from_clock() {
                Some(now) if now != state.today => {
                    state.today = now;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        }
        Event::CloseRequested => {
            state.running = false;
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// The view a digit key selects, by its index in [`CalendarView::all`].
fn view_for_digit(key: Key) -> Option<usize> {
    match key {
        Key::Num1 => Some(0),
        Key::Num2 => Some(1),
        Key::Num3 => Some(2),
        Key::Num4 => Some(3),
        Key::Num5 => Some(4),
        _ => Option::None,
    }
}

fn handle_key(state: &mut CalendarApp, key: &KeyEvent) -> EventResult {
    if key.modifiers.ctrl {
        return match key.key {
            Key::F => {
                state.search_focused = true;
                EventResult::Consumed
            }
            Key::B => {
                state.sidebar_visible = !state.sidebar_visible;
                state.clamp_scroll();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        };
    }

    if state.search_focused {
        match key.key {
            Key::Escape => {
                state.search_query.clear();
                state.search();
                state.search_focused = false;
                state.clamp_scroll();
                return EventResult::Consumed;
            }
            Key::Backspace => {
                state.search_query.pop();
                state.search();
                state.content_scroll = 0.0;
                return EventResult::Consumed;
            }
            Key::Enter => {
                state.search_focused = false;
                return EventResult::Consumed;
            }
            _ if key.types_text() => {
                state.search_query.extend(key.typed());
                state.search();
                // The agenda is the only view that shows results, so a search
                // that leaves you looking at a month grid has found nothing as
                // far as the user can tell.
                state.view = CalendarView::Agenda;
                state.content_scroll = 0.0;
                return EventResult::Consumed;
            }
            _ => {}
        }
    }

    if let Some(index) = view_for_digit(key.key)
        && let Some(view) = CalendarView::all().get(index)
    {
        state.view = *view;
        state.content_scroll = 0.0;
        return EventResult::Consumed;
    }

    match key.key {
        Key::Left | Key::PageUp => {
            state.navigate_backward();
            state.content_scroll = 0.0;
            EventResult::Consumed
        }
        Key::Right | Key::PageDown => {
            state.navigate_forward();
            state.content_scroll = 0.0;
            EventResult::Consumed
        }
        Key::Home => {
            state.go_to_today();
            state.content_scroll = 0.0;
            EventResult::Consumed
        }
        Key::Up => {
            state.content_scroll -= WEEK_HOUR_H;
            state.clamp_scroll();
            EventResult::Consumed
        }
        Key::Down => {
            state.content_scroll += WEEK_HOUR_H;
            state.clamp_scroll();
            EventResult::Consumed
        }
        Key::Escape => {
            state.selected_event_id = Option::None;
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

fn handle_mouse(state: &mut CalendarApp, mouse: &MouseEvent) -> EventResult {
    let (x, y) = (mouse.x, mouse.y);

    match &mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => {
            let hit = state.target_at(x, y);
            state.search_focused = matches!(hit, Some(Target::SearchField));

            match hit {
                Some(Target::NavBack) => {
                    state.navigate_backward();
                    state.content_scroll = 0.0;
                }
                Some(Target::NavForward) => {
                    state.navigate_forward();
                    state.content_scroll = 0.0;
                }
                Some(Target::TodayButton) => {
                    state.go_to_today();
                    state.content_scroll = 0.0;
                }
                Some(Target::ViewTab(index)) => {
                    if let Some(view) = CalendarView::all().get(index) {
                        state.view = *view;
                        state.content_scroll = 0.0;
                    }
                }
                Some(Target::MiniPrevMonth) => state.step_mini_month(-1),
                Some(Target::MiniNextMonth) => state.step_mini_month(1),
                Some(Target::MiniDay(date)) => state.select_date(date),
                Some(Target::Day(date)) => {
                    state.select_date(date);
                    // In the year view a day is eight pixels of nothing much.
                    // Clicking one means "show me that month".
                    if state.view == CalendarView::Year {
                        state.view = CalendarView::Month;
                        state.content_scroll = 0.0;
                    }
                }
                Some(Target::CategoryFilter(index)) => {
                    if let Some(cat) = EventCategory::all().get(index) {
                        state.category_filter = if state.category_filter == Some(*cat) {
                            Option::None
                        } else {
                            Some(*cat)
                        };
                    }
                }
                Some(Target::Event(id)) => state.selected_event_id = Some(id),
                // Consumed either way: the click landed on this window.
                Some(Target::SearchField) | Option::None => {}
            }

            state.clamp_scroll();
            EventResult::Consumed
        }

        MouseEventKind::Scroll { dy, .. } => {
            // `dy` is a notch count, not a distance.
            let step = wheel::pixels(*dy, WEEK_HOUR_H);
            let layout = state.layout();

            if let Some(bar) = layout.sidebar
                && bar.contains(x, y)
            {
                // Nothing in the sidebar scrolls, so the wheel does there what
                // the two arrows above the mini calendar do.
                if step > 0.0 {
                    state.step_mini_month(1);
                } else if step < 0.0 {
                    state.step_mini_month(-1);
                }
                return EventResult::Consumed;
            }

            if layout.content.contains(x, y) {
                state.content_scroll += step;
                state.clamp_scroll();
                return EventResult::Consumed;
            }

            EventResult::Ignored
        }

        _ => EventResult::Ignored,
    }
}

// ============================================================================
// Window
// ============================================================================

impl App for CalendarApp {
    fn title(&self) -> String {
        String::from("Calendar")
    }

    fn app_id(&self) -> String {
        String::from("calendar")
    }

    fn initial_size(&self) -> (u32, u32) {
        (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32)
    }

    /// A minute. Long enough to be free, short enough that "today" moves to the
    /// new day within a minute of midnight rather than whenever the user next
    /// happens to click something.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_mins(1))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        let result = handle_event(self, event);
        if !self.running {
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for CalendarApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (DEFAULT_WIDTH, DEFAULT_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

/// Today's civil date from the system clock, or `None` before the epoch.
///
/// `main` used to open on a hardcoded 2026-05-18, so every "today" highlight in
/// the app pointed at a day in the past.
fn today_from_clock() -> Option<Date> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(Date::from_civil(date::Date::from_unix_utc(
        i64::try_from(secs).ok()?,
    )))
}

// ============================================================================
// Sample data
// ============================================================================

fn sample_events(store: &mut EventStore, today: Date) {
    store.add(CalendarEvent {
        id: 0,
        title: "Team Standup".to_string(),
        description: "Daily sync meeting".to_string(),
        category: EventCategory::Meeting,
        start: DateTime::new(today, Time { hour: 9, minute: 0 }),
        end: DateTime::new(
            today,
            Time {
                hour: 9,
                minute: 30,
            },
        ),
        all_day: false,
        recurrence: RecurrenceRule::Weekly {
            days: vec![1, 2, 3, 4, 5],
        },
        reminder: Reminder::MinutesBefore(5),
        location: Some("Conference Room A".to_string()),
        color_override: None,
    });

    store.add(CalendarEvent {
        id: 0,
        title: "Lunch with Sarah".to_string(),
        description: String::new(),
        category: EventCategory::Social,
        start: DateTime::new(
            today,
            Time {
                hour: 12,
                minute: 0,
            },
        ),
        end: DateTime::new(
            today,
            Time {
                hour: 13,
                minute: 0,
            },
        ),
        all_day: false,
        recurrence: RecurrenceRule::None,
        reminder: Reminder::MinutesBefore(30),
        location: Some("Downtown Cafe".to_string()),
        color_override: None,
    });

    store.add(CalendarEvent {
        id: 0,
        title: "Project Deadline".to_string(),
        description: "Q2 deliverables due".to_string(),
        category: EventCategory::Deadline,
        start: DateTime::new(
            today.add_days(3),
            Time {
                hour: 17,
                minute: 0,
            },
        ),
        end: DateTime::new(
            today.add_days(3),
            Time {
                hour: 17,
                minute: 0,
            },
        ),
        all_day: false,
        recurrence: RecurrenceRule::None,
        reminder: Reminder::DayBefore,
        location: None,
        color_override: None,
    });

    store.add(CalendarEvent {
        id: 0,
        title: "Mom's Birthday".to_string(),
        description: String::new(),
        category: EventCategory::Birthday,
        start: DateTime::new(today.add_days(7), Time { hour: 0, minute: 0 }),
        end: DateTime::new(
            today.add_days(7),
            Time {
                hour: 23,
                minute: 59,
            },
        ),
        all_day: true,
        recurrence: RecurrenceRule::Yearly,
        reminder: Reminder::DayBefore,
        location: None,
        color_override: None,
    });

    store.add(CalendarEvent {
        id: 0,
        title: "Gym Session".to_string(),
        description: "Upper body workout".to_string(),
        category: EventCategory::Health,
        start: DateTime::new(today.add_days(1), Time { hour: 7, minute: 0 }),
        end: DateTime::new(today.add_days(1), Time { hour: 8, minute: 0 }),
        all_day: false,
        recurrence: RecurrenceRule::Weekly {
            days: vec![1, 3, 5],
        },
        reminder: Reminder::MinutesBefore(15),
        location: Some("FitLife Gym".to_string()),
        color_override: None,
    });

    store.add(CalendarEvent {
        id: 0,
        title: "Vacation".to_string(),
        description: "Summer holiday".to_string(),
        category: EventCategory::Travel,
        start: DateTime::new(today.add_days(14), Time { hour: 0, minute: 0 }),
        end: DateTime::new(
            today.add_days(21),
            Time {
                hour: 23,
                minute: 59,
            },
        ),
        all_day: true,
        recurrence: RecurrenceRule::None,
        reminder: Reminder::DayBefore,
        location: Some("Barcelona, Spain".to_string()),
        color_override: None,
    });

    store.add(CalendarEvent {
        id: 0,
        title: "Code Review".to_string(),
        description: "Review PR #42".to_string(),
        category: EventCategory::Work,
        start: DateTime::new(
            today,
            Time {
                hour: 14,
                minute: 0,
            },
        ),
        end: DateTime::new(
            today,
            Time {
                hour: 15,
                minute: 30,
            },
        ),
        all_day: false,
        recurrence: RecurrenceRule::None,
        reminder: Reminder::MinutesBefore(10),
        location: None,
        color_override: None,
    });

    store.add(CalendarEvent {
        id: 0,
        title: "Online Course: Rust".to_string(),
        description: "Advanced async programming".to_string(),
        category: EventCategory::Education,
        start: DateTime::new(
            today.add_days(2),
            Time {
                hour: 19,
                minute: 0,
            },
        ),
        end: DateTime::new(
            today.add_days(2),
            Time {
                hour: 21,
                minute: 0,
            },
        ),
        all_day: false,
        recurrence: RecurrenceRule::Weekly { days: vec![2, 4] },
        reminder: Reminder::MinutesBefore(15),
        location: None,
        color_override: None,
    });
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    // A clock that cannot be read at all is a broken machine, not a reason to
    // refuse to open a calendar; fall back to the epoch's own new year so the
    // failure is visible rather than plausible.
    let today = today_from_clock().unwrap_or(Date {
        year: 1970,
        month: 1,
        day: 1,
    });
    let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
    sample_events(&mut app.store, today);
    app::launch("calendar", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use guitk::probe;

    /// The draw commands of one frame at the app's current size.
    fn render(app: &CalendarApp) -> Vec<RenderCommand> {
        app.frame(app.width, app.height).commands().to_vec()
    }

    /// A calendar with the sample events, opened at the default window size.
    fn sample_app(today: Date) -> CalendarApp {
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
        sample_events(&mut app.store, today);
        app
    }

    fn june_2024() -> Date {
        Date {
            year: 2024,
            month: 6,
            day: 15,
        }
    }

    // Date tests
    #[test]
    fn test_date_new_valid() {
        assert!(Date::new(2024, 1, 1).is_some());
        assert!(Date::new(2024, 12, 31).is_some());
        assert!(Date::new(2024, 2, 29).is_some());
    }

    #[test]
    fn test_date_new_invalid() {
        assert!(Date::new(2024, 0, 1).is_none());
        assert!(Date::new(2024, 13, 1).is_none());
        assert!(Date::new(2023, 2, 29).is_none());
        assert!(Date::new(2024, 1, 32).is_none());
    }

    #[test]
    fn test_leap_year() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 4), 30);
    }

    #[test]
    fn test_day_of_week() {
        // 2024-01-01 is Monday
        let d = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        assert_eq!(d.day_of_week(), 1);

        // 2024-01-07 is Sunday
        let d = Date {
            year: 2024,
            month: 1,
            day: 7,
        };
        assert_eq!(d.day_of_week(), 0);
    }

    #[test]
    fn test_date_format() {
        let d = Date {
            year: 2024,
            month: 3,
            day: 15,
        };
        assert_eq!(d.format_short(), "2024-03-15");
        assert!(d.format_long().contains("March"));
        assert!(d.format_long().contains("15"));
    }

    #[test]
    fn test_date_add_days() {
        let d = Date {
            year: 2024,
            month: 1,
            day: 30,
        };
        let next = d.add_days(3);
        assert_eq!(next.month, 2);
        assert_eq!(next.day, 2);
    }

    #[test]
    fn test_date_add_days_negative() {
        let d = Date {
            year: 2024,
            month: 3,
            day: 1,
        };
        let prev = d.add_days(-1);
        assert_eq!(prev.month, 2);
        assert_eq!(prev.day, 29); // 2024 is leap year
    }

    #[test]
    fn test_date_next_prev_month() {
        let d = Date {
            year: 2024,
            month: 1,
            day: 31,
        };
        let next = d.next_month();
        assert_eq!(next.month, 2);
        assert_eq!(next.day, 29); // Clamped to max day in Feb

        let d2 = Date {
            year: 2024,
            month: 1,
            day: 15,
        };
        let prev = d2.prev_month();
        assert_eq!(prev.month, 12);
        assert_eq!(prev.year, 2023);
    }

    #[test]
    fn test_date_weekend() {
        let sat = Date {
            year: 2024,
            month: 1,
            day: 6,
        };
        assert!(sat.is_weekend());
        let mon = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        assert!(!mon.is_weekend());
    }

    #[test]
    fn test_date_days_since() {
        let a = Date {
            year: 2024,
            month: 1,
            day: 10,
        };
        let b = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        assert_eq!(a.days_since(b), 9);
    }

    #[test]
    fn test_day_of_year() {
        let d = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        assert_eq!(d.day_of_year(), 1);
        let d2 = Date {
            year: 2024,
            month: 12,
            day: 31,
        };
        assert_eq!(d2.day_of_year(), 366); // Leap year
    }

    // Time tests
    #[test]
    fn test_time_new_valid() {
        assert!(Time::new(0, 0).is_some());
        assert!(Time::new(23, 59).is_some());
    }

    #[test]
    fn test_time_new_invalid() {
        assert!(Time::new(24, 0).is_none());
        assert!(Time::new(0, 60).is_none());
    }

    #[test]
    fn test_time_format() {
        let t = Time {
            hour: 14,
            minute: 30,
        };
        assert_eq!(t.format_24h(), "14:30");
        assert_eq!(t.format_12h(), "2:30 PM");

        let t2 = Time { hour: 0, minute: 0 };
        assert_eq!(t2.format_12h(), "12:00 AM");
    }

    #[test]
    fn test_time_to_minutes() {
        let t = Time {
            hour: 2,
            minute: 30,
        };
        assert_eq!(t.to_minutes(), 150);
    }

    #[test]
    fn test_time_from_minutes() {
        let t = Time::from_minutes(150);
        assert_eq!(t.hour, 2);
        assert_eq!(t.minute, 30);
    }

    // DateTime tests
    #[test]
    fn test_datetime_format_ics() {
        let dt = DateTime {
            date: Date {
                year: 2024,
                month: 3,
                day: 15,
            },
            time: Time {
                hour: 14,
                minute: 30,
            },
        };
        assert_eq!(dt.format_ics(), "20240315T143000");
    }

    // Event category tests
    #[test]
    fn test_category_colors() {
        for cat in EventCategory::all() {
            let _ = cat.color();
            let _ = cat.label();
            let _ = cat.icon();
        }
    }

    // Recurrence tests
    #[test]
    fn test_recurrence_daily() {
        let rule = RecurrenceRule::Daily;
        let origin = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        assert!(rule.matches(
            origin,
            Date {
                year: 2024,
                month: 1,
                day: 5
            }
        ));
        assert!(rule.matches(origin, origin));
    }

    #[test]
    fn test_recurrence_weekly() {
        let rule = RecurrenceRule::Weekly {
            days: vec![1, 3, 5],
        }; // Mon, Wed, Fri
        let origin = Date {
            year: 2024,
            month: 1,
            day: 1,
        }; // Monday
        // Jan 3 2024 is Wednesday
        assert!(rule.matches(
            origin,
            Date {
                year: 2024,
                month: 1,
                day: 3
            }
        ));
    }

    #[test]
    fn test_recurrence_monthly() {
        let rule = RecurrenceRule::Monthly;
        let origin = Date {
            year: 2024,
            month: 1,
            day: 15,
        };
        assert!(rule.matches(
            origin,
            Date {
                year: 2024,
                month: 3,
                day: 15
            }
        ));
        assert!(!rule.matches(
            origin,
            Date {
                year: 2024,
                month: 3,
                day: 16
            }
        ));
    }

    #[test]
    fn test_recurrence_yearly() {
        let rule = RecurrenceRule::Yearly;
        let origin = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        assert!(rule.matches(
            origin,
            Date {
                year: 2025,
                month: 6,
                day: 15
            }
        ));
        assert!(!rule.matches(
            origin,
            Date {
                year: 2025,
                month: 7,
                day: 15
            }
        ));
    }

    #[test]
    fn test_recurrence_next_occurrence() {
        let rule = RecurrenceRule::Daily;
        let from = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        let next = rule.next_occurrence(from).unwrap();
        assert_eq!(
            next,
            Date {
                year: 2024,
                month: 1,
                day: 2
            }
        );
    }

    #[test]
    fn test_recurrence_none() {
        let rule = RecurrenceRule::None;
        assert!(!rule.matches(
            Date {
                year: 2024,
                month: 1,
                day: 1
            },
            Date {
                year: 2024,
                month: 1,
                day: 2
            },
        ));
        assert!(
            rule.next_occurrence(Date {
                year: 2024,
                month: 1,
                day: 1
            })
            .is_none()
        );
    }

    // Reminder tests
    #[test]
    fn test_reminder_presets() {
        let presets = Reminder::presets();
        assert!(presets.len() >= 6);
    }

    // Event tests
    #[test]
    fn test_event_duration() {
        let ev = CalendarEvent {
            id: 1,
            title: "Test".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time { hour: 9, minute: 0 },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time {
                    hour: 10,
                    minute: 30,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        };
        assert_eq!(ev.duration_minutes(), 90);
        assert_eq!(ev.duration_label(), "1h 30m");
    }

    #[test]
    fn test_event_all_day_duration() {
        let ev = CalendarEvent {
            id: 1,
            title: "Holiday".to_string(),
            description: String::new(),
            category: EventCategory::Holiday,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 12,
                    day: 25,
                },
                Time { hour: 0, minute: 0 },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 12,
                    day: 25,
                },
                Time {
                    hour: 23,
                    minute: 59,
                },
            ),
            all_day: true,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        };
        assert_eq!(ev.duration_label(), "All day");
    }

    #[test]
    fn test_event_occurs_on() {
        let ev = CalendarEvent {
            id: 1,
            title: "Test".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time { hour: 9, minute: 0 },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::Weekly { days: vec![1] }, // Mondays
            reminder: Reminder::None,
            location: None,
            color_override: None,
        };
        // Jan 1 2024 is Monday
        assert!(ev.occurs_on(Date {
            year: 2024,
            month: 1,
            day: 1
        }));
        assert!(ev.occurs_on(Date {
            year: 2024,
            month: 1,
            day: 8
        })); // Next Monday
        assert!(!ev.occurs_on(Date {
            year: 2024,
            month: 1,
            day: 2
        })); // Tuesday
    }

    // ICS tests
    #[test]
    fn test_ics_roundtrip() {
        let ev = CalendarEvent {
            id: 42,
            title: "Meeting".to_string(),
            description: "Important meeting".to_string(),
            category: EventCategory::Meeting,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 6,
                    day: 15,
                },
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 6,
                    day: 15,
                },
                Time {
                    hour: 11,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: Some("Room 101".to_string()),
            color_override: None,
        };

        let ics = generate_ics(&[ev], "Test");
        assert!(ics.contains("BEGIN:VEVENT"));
        assert!(ics.contains("SUMMARY:Meeting"));
        assert!(ics.contains("LOCATION:Room 101"));

        let parsed = parse_ics(&ics);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "Meeting");
        assert_eq!(parsed[0].start.date.year, 2024);
    }

    #[test]
    fn test_ics_escape_unescape() {
        let original = "Hello; World, Test\\n";
        let escaped = ics_escape(original);
        assert!(escaped.contains("\\;"));
        assert!(escaped.contains("\\,"));
        let unescaped = ics_unescape(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn test_parse_ics_datetime() {
        let dt = parse_ics_datetime("20240315T143000").unwrap();
        assert_eq!(dt.date.year, 2024);
        assert_eq!(dt.date.month, 3);
        assert_eq!(dt.date.day, 15);
        assert_eq!(dt.time.hour, 14);
        assert_eq!(dt.time.minute, 30);
    }

    // EventStore tests
    #[test]
    fn test_store_add_remove() {
        let mut store = EventStore::new();
        let id = store.add(CalendarEvent {
            id: 0,
            title: "Test".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time { hour: 9, minute: 0 },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });
        assert_eq!(store.len(), 1);
        assert!(store.get(id).is_some());
        assert!(store.remove(id));
        assert!(store.is_empty());
    }

    #[test]
    fn test_store_events_on() {
        let mut store = EventStore::new();
        let date = Date {
            year: 2024,
            month: 3,
            day: 15,
        };
        store.add(CalendarEvent {
            id: 0,
            title: "A".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(date, Time { hour: 9, minute: 0 }),
            end: DateTime::new(
                date,
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });
        store.add(CalendarEvent {
            id: 0,
            title: "B".to_string(),
            description: String::new(),
            category: EventCategory::Personal,
            start: DateTime::new(
                date.add_days(1),
                Time {
                    hour: 12,
                    minute: 0,
                },
            ),
            end: DateTime::new(
                date.add_days(1),
                Time {
                    hour: 13,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });

        assert_eq!(store.events_on(date).len(), 1);
        assert_eq!(store.events_on(date.add_days(1)).len(), 1);
        assert_eq!(store.events_on(date.add_days(2)).len(), 0);
    }

    #[test]
    fn test_store_search() {
        let mut store = EventStore::new();
        let date = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        store.add(CalendarEvent {
            id: 0,
            title: "Team Meeting".to_string(),
            description: "Weekly sync".to_string(),
            category: EventCategory::Meeting,
            start: DateTime::new(date, Time { hour: 9, minute: 0 }),
            end: DateTime::new(
                date,
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });
        store.add(CalendarEvent {
            id: 0,
            title: "Lunch".to_string(),
            description: String::new(),
            category: EventCategory::Social,
            start: DateTime::new(
                date,
                Time {
                    hour: 12,
                    minute: 0,
                },
            ),
            end: DateTime::new(
                date,
                Time {
                    hour: 13,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });

        assert_eq!(store.search("meeting").len(), 1);
        assert_eq!(store.search("sync").len(), 1);
        assert_eq!(store.search("xyz").len(), 0);
    }

    #[test]
    fn test_store_by_category() {
        let mut store = EventStore::new();
        let date = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        store.add(CalendarEvent {
            id: 0,
            title: "Work".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(date, Time { hour: 9, minute: 0 }),
            end: DateTime::new(
                date,
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });
        store.add(CalendarEvent {
            id: 0,
            title: "Gym".to_string(),
            description: String::new(),
            category: EventCategory::Health,
            start: DateTime::new(date, Time { hour: 7, minute: 0 }),
            end: DateTime::new(date, Time { hour: 8, minute: 0 }),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });

        assert_eq!(store.events_by_category(EventCategory::Work).len(), 1);
        assert_eq!(store.events_by_category(EventCategory::Health).len(), 1);
        assert_eq!(store.events_by_category(EventCategory::Travel).len(), 0);
    }

    #[test]
    fn test_store_import_ics() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20240615T100000\r\nDTEND:20240615T110000\r\nSUMMARY:Imported Event\r\nEND:VEVENT\r\nEND:VCALENDAR";
        let mut store = EventStore::new();
        let count = store.import_ics(ics);
        assert_eq!(count, 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.all()[0].title, "Imported Event");
    }

    // CalendarApp tests
    #[test]
    fn test_app_navigation() {
        let today = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let mut app = CalendarApp::new(800.0, 600.0, today);

        app.view = CalendarView::Month;
        app.navigate_forward();
        assert_eq!(app.view_date.month, 7);
        app.navigate_backward();
        assert_eq!(app.view_date.month, 6);
        app.go_to_today();
        assert_eq!(app.view_date, today);
    }

    #[test]
    fn test_app_navigation_week() {
        let today = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let mut app = CalendarApp::new(800.0, 600.0, today);
        app.view = CalendarView::Week;
        app.navigate_forward();
        assert_eq!(app.view_date.day, 22);
    }

    #[test]
    fn test_app_render_all_views() {
        let today = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let mut app = CalendarApp::new(1280.0, 720.0, today);
        sample_events(&mut app.store, today);

        for view in CalendarView::all() {
            app.view = *view;
            let cmds = render(&app);
            assert!(!cmds.is_empty(), "View {:?} produced no commands", view);
        }
    }

    #[test]
    fn test_app_render_without_sidebar() {
        let today = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let mut app = CalendarApp::new(800.0, 600.0, today);
        app.sidebar_visible = false;
        let cmds = render(&app);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_search() {
        let today = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let mut app = CalendarApp::new(800.0, 600.0, today);
        sample_events(&mut app.store, today);

        app.search_query = "Lunch".to_string();
        app.search();
        assert!(!app.search_results.is_empty());

        app.search_query = "zzzzz".to_string();
        app.search();
        assert!(app.search_results.is_empty());
    }

    // View label tests
    #[test]
    fn test_view_labels() {
        for v in CalendarView::all() {
            let _ = v.label();
        }
    }

    // Month name tests
    #[test]
    fn test_month_names() {
        assert_eq!(month_name(1), "January");
        assert_eq!(month_name(12), "December");
        assert_eq!(month_short(3), "Mar");
    }

    // First DOW tests
    #[test]
    fn test_first_dow() {
        // Jan 2024 starts on Monday
        assert_eq!(first_dow_of_month(2024, 1), 1);
    }

    // Week number test
    //
    // This used to be the whole of it:
    //
    //     let wn = Date { year: 2024, month: 1, day: 8 }.week_number();
    //     assert!((1..=53).contains(&wn));
    //
    // and it passed for two years over an implementation that was wrong on
    // 38.5% of all dates — because the old `week_number` ended in
    // `week.min(53)` and could not return anything outside 1..=53 whatever it
    // computed. The assertion restated the implementation's clamp rather than
    // the caller's requirement, so no arithmetic error could reach it.
    #[test]
    fn week_numbers_match_the_iso_standards_worked_examples() {
        // From ISO 8601 itself and the usual worked examples. Each is a case
        // where week 1 is *not* the week containing 1 January, which is the
        // whole content of the rule and the thing a "day-of-year / 7" formula
        // structurally cannot express.
        for (y, m, d, want) in [
            (2026, 12, 31, (2026, 53)),
            (2027, 1, 1, (2026, 53)), // a Friday: still last year's week 53
            (2027, 1, 4, (2027, 1)),
            (2025, 1, 1, (2025, 1)),
            (2024, 12, 30, (2025, 1)), // a Monday: already next year's week 1
            (2021, 1, 1, (2020, 53)),
            (2020, 12, 31, (2020, 53)),
            (1977, 1, 1, (1976, 53)),
            (1977, 1, 3, (1977, 1)),
            (2024, 1, 8, (2024, 2)),
        ] {
            let date = Date {
                year: y,
                month: m,
                day: d,
            };
            assert_eq!(date.iso_week(), want, "{y}-{m:02}-{d:02}");
            assert_eq!(date.week_number(), want.1, "{y}-{m:02}-{d:02}");
        }
    }

    #[test]
    fn a_week_number_is_constant_across_its_own_monday_to_sunday() {
        // The property that makes a week number a week number, and the one the
        // old formula broke: counting from 1 January means the boundary lands
        // wherever that day happens to fall, not on a Monday.
        //
        // Asserted over `week_number` and not only over `iso_week`, because
        // `week_number` is what the month and week views actually draw. A
        // property test that exercises the accessor nobody calls would go
        // green over a broken one, which is the failure this whole change is
        // about.
        let mut date = Date {
            year: 2023,
            month: 12,
            day: 25,
        };
        for _ in 0..800 {
            // 0..=6 by construction, so the negation below is exact; asserted
            // rather than papered over with a silent `unwrap_or(0)`, which
            // would turn an impossible failure into a wrong Monday.
            let back = date.weekday().days_since(Weekday::Monday);
            assert!(back <= 6, "{date:?}: {back} days back to Monday");
            let monday = date.add_days(-(back as i32));

            assert_eq!(
                date.week_number(),
                monday.week_number(),
                "{date:?} disagrees with the Monday of its own week, {monday:?}"
            );
            assert_eq!(
                date.iso_week(),
                monday.iso_week(),
                "{date:?} disagrees with the Monday of its own week, {monday:?}"
            );
            // The two accessors are separate entry points onto the same fact;
            // a caller reading one and a caller reading the other must not be
            // able to disagree.
            assert_eq!(date.week_number(), date.iso_week().1, "{date:?}");
            date = date.add_days(1);
        }
    }

    // Event time range label
    #[test]
    fn test_time_range_label() {
        let ev = CalendarEvent {
            id: 1,
            title: "T".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time { hour: 9, minute: 0 },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        };
        let label = ev.time_range_label();
        assert!(label.contains("9:00 AM"));
        assert!(label.contains("10:00 AM"));
    }

    // ICS recurrence output
    #[test]
    fn test_ics_weekly_recurrence() {
        let ev = CalendarEvent {
            id: 1,
            title: "Weekly".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time { hour: 9, minute: 0 },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 1,
                    day: 1,
                },
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::Weekly {
                days: vec![1, 3, 5],
            },
            reminder: Reminder::None,
            location: None,
            color_override: None,
        };
        let ics = ev.to_ics();
        assert!(ics.contains("RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR"));
    }

    #[test]
    fn test_ics_yearly_recurrence() {
        let ev = CalendarEvent {
            id: 1,
            title: "Birthday".to_string(),
            description: String::new(),
            category: EventCategory::Birthday,
            start: DateTime::new(
                Date {
                    year: 2024,
                    month: 6,
                    day: 15,
                },
                Time { hour: 0, minute: 0 },
            ),
            end: DateTime::new(
                Date {
                    year: 2024,
                    month: 6,
                    day: 15,
                },
                Time {
                    hour: 23,
                    minute: 59,
                },
            ),
            all_day: true,
            recurrence: RecurrenceRule::Yearly,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        };
        let ics = ev.to_ics();
        assert!(ics.contains("RRULE:FREQ=YEARLY"));
    }

    // Edge cases
    #[test]
    fn test_date_add_days_year_boundary() {
        let d = Date {
            year: 2024,
            month: 12,
            day: 30,
        };
        let next = d.add_days(5);
        assert_eq!(next.year, 2025);
        assert_eq!(next.month, 1);
    }

    #[test]
    fn test_empty_store() {
        let store = EventStore::new();
        assert!(store.is_empty());
        let date = Date {
            year: 2024,
            month: 1,
            day: 1,
        };
        assert!(store.events_on(date).is_empty());
        assert!(store.search("test").is_empty());
    }

    #[test]
    fn test_upcoming_sorted() {
        let mut store = EventStore::new();
        let base = Date {
            year: 2024,
            month: 6,
            day: 1,
        };

        store.add(CalendarEvent {
            id: 0,
            title: "Later".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(base.add_days(5), Time { hour: 9, minute: 0 }),
            end: DateTime::new(
                base.add_days(5),
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });
        store.add(CalendarEvent {
            id: 0,
            title: "Sooner".to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(base.add_days(2), Time { hour: 9, minute: 0 }),
            end: DateTime::new(
                base.add_days(2),
                Time {
                    hour: 10,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });

        let upcoming = store.upcoming(base, 10);
        assert_eq!(upcoming.len(), 2);
        assert_eq!(upcoming[0].title, "Sooner");
        assert_eq!(upcoming[1].title, "Later");
    }

    // ========================================================================
    // Window, layout and input
    //
    // Everything below exercises the app through the same two doors the
    // compositor uses: a frame that records where it painted, and an event.
    // ========================================================================

    /// An event at `hour` on `date`, added to `app`, returning its id.
    fn add_event_at(app: &mut CalendarApp, date: Date, hour: u32, title: &str) -> u64 {
        app.store.add(CalendarEvent {
            id: 0,
            title: title.to_string(),
            description: String::new(),
            category: EventCategory::Personal,
            start: DateTime::new(date, Time { hour, minute: 0 }),
            end: DateTime::new(
                date,
                Time {
                    hour: hour + 1,
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        })
    }

    #[test]
    fn the_window_declares_the_size_the_probe_draws_at() {
        let app = sample_app(june_2024());
        assert_eq!(
            app.initial_size(),
            (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32)
        );
        assert_eq!(CalendarApp::SIZE, (DEFAULT_WIDTH, DEFAULT_HEIGHT));
        assert_eq!(app.title(), "Calendar");
        assert!(app.tick_interval().is_some());
    }

    #[test]
    fn every_view_draws_a_balanced_frame_at_every_reasonable_size() {
        for (w, h) in [
            (DEFAULT_WIDTH, DEFAULT_HEIGHT),
            (1920.0, 1080.0),
            (800.0, 600.0),
            (480.0, 360.0),
            (320.0, 240.0),
        ] {
            let mut app = sample_app(june_2024());
            app.resize(w, h);
            for view in CalendarView::all() {
                app.view = *view;
                let frame = app.frame(w, h);
                assert!(
                    frame.is_balanced(),
                    "{view:?} at {w}x{h} left a clip or translate open"
                );
                assert!(
                    !frame.commands().is_empty(),
                    "{view:?} at {w}x{h} painted nothing"
                );
            }
        }
    }

    #[test]
    fn the_layout_follows_the_window_instead_of_a_constant() {
        // The old renderer hardcoded a 220px sidebar and took the rest, at
        // whatever size the window happened to be. The content area is the
        // thing that has to grow.
        let wide = Layout::new(1920.0, 1080.0, true);
        let narrow = Layout::new(900.0, 600.0, true);
        assert!(wide.content.w > narrow.content.w);
        assert_eq!(wide.content.right(), 1920.0);
        assert_eq!(narrow.content.right(), 900.0);
        assert_eq!(wide.content.bottom(), 1080.0);
    }

    #[test]
    fn the_view_tabs_never_run_off_the_right_edge() {
        // They are the only way to change view, so they shrink rather than
        // leave the window. Everything else in the bar gives way to them.
        for width in [320.0, 400.0, 520.0, 640.0, 900.0, 1280.0, 1920.0] {
            let layout = Layout::new(width, 720.0, true);
            let last = layout.view_tab(CalendarView::all().len() - 1);
            assert!(
                last.right() <= width + 0.01,
                "at width {width} the last tab ended at {}",
                last.right()
            );
            assert!(layout.view_tab(0).x >= CHROME_LEFT - 0.01);
            assert!(last.w >= 1.0);
        }
    }

    #[test]
    fn a_narrow_window_drops_the_caption_before_the_search_box() {
        // Both are droppable; the caption goes first because it only names a
        // view that is already on screen, while the search box is the only way
        // to reach an event by name.
        let mut caption_only_widths = 0;
        for width in [320.0, 500.0, 700.0, 900.0, 1100.0, 1400.0] {
            let layout = Layout::new(width, 720.0, true);
            if layout.search.is_some() {
                // Whenever there is room for the search box there is room for
                // the caption too, by construction.
                assert!(
                    layout.header.is_some(),
                    "at width {width} the search box survived but the caption did not"
                );
            } else if layout.header.is_some() {
                caption_only_widths += 1;
            }
        }
        assert!(
            caption_only_widths > 0,
            "no width dropped the search box while keeping the caption"
        );
    }

    #[test]
    fn the_sidebar_goes_when_the_content_cannot_spare_it() {
        assert!(Layout::new(1280.0, 720.0, true).sidebar.is_some());
        assert!(Layout::new(400.0, 720.0, true).sidebar.is_none());
        // …and the content then starts at the left edge rather than at 220.
        assert_eq!(Layout::new(400.0, 720.0, true).content.x, 0.0);
        // Asking for no sidebar is honoured at any width.
        assert!(Layout::new(1920.0, 1080.0, false).sidebar.is_none());
    }

    #[test]
    fn a_category_row_below_a_short_sidebar_is_not_clickable() {
        // It is drawn inside the sidebar's clip, so both the ink and the box
        // are dropped. The renderer decides this once; nothing re-derives it.
        let mut app = sample_app(june_2024());
        app.resize(1280.0, 280.0);
        let last = EventCategory::all().len() - 1;
        assert!(
            probe::rect_of_sized(&app, Target::CategoryFilter(last), (1280.0, 280.0)).is_none(),
            "the bottom category row survived a 280px window"
        );
        app.resize(1280.0, 720.0);
        assert!(probe::rect_of(&app, Target::CategoryFilter(last)).is_some());
    }

    #[test]
    fn clicking_a_day_selects_it_and_the_mini_calendar_follows() {
        let mut app = sample_app(june_2024());
        let target = Date {
            year: 2024,
            month: 6,
            day: 20,
        };
        assert_eq!(
            probe::click(&mut app, Target::Day(target)),
            EventResult::Consumed
        );
        assert_eq!(app.selected_date, target);
        assert_eq!(app.mini_cal_month, 6);
        assert_eq!(app.mini_cal_year, 2024);

        // A day in another month drags the sidebar with it.
        app.view_date = Date {
            year: 2024,
            month: 7,
            day: 1,
        };
        app.select_date(Date {
            year: 2024,
            month: 7,
            day: 4,
        });
        assert_eq!(app.mini_cal_month, 7);
    }

    #[test]
    fn the_day_a_click_lands_on_is_the_day_that_was_drawn() {
        // This is the whole point of recording hit boxes while painting: there
        // is no second expression to keep in step with the first.
        let app = sample_app(june_2024());
        for day in [1u32, 9, 17, 30] {
            let date = Date {
                year: 2024,
                month: 6,
                day,
            };
            let rect = probe::rect_of(&app, Target::Day(date))
                .unwrap_or_else(|| panic!("June {day} was not drawn"));
            // The lower part of the cell, below where event chips stack: those
            // deliberately take the click off the cell they sit on.
            let (cx, _) = rect.centre();
            assert_eq!(
                app.target_at(cx, rect.bottom() - 4.0),
                Some(Target::Day(date))
            );
        }
    }

    #[test]
    fn an_event_chip_takes_the_click_off_the_day_it_sits_on() {
        // Both boxes cover the point; the one recorded later wins, and the
        // event is drawn on top of the cell.
        let today = june_2024();
        let mut app = sample_app(today);
        let id = add_event_at(&mut app, today, 9, "Standup");
        let chip = probe::rect_of(&app, Target::Event(id)).expect("the chip was drawn");
        let cell = probe::rect_of(&app, Target::Day(today)).expect("the cell was drawn");
        let (cx, cy) = chip.centre();
        assert!(cell.contains(cx, cy), "the chip is not inside its own cell");
        assert_eq!(app.target_at(cx, cy), Some(Target::Event(id)));
    }

    #[test]
    fn the_view_tabs_and_nav_buttons_do_what_they_say() {
        let mut app = sample_app(june_2024());
        for (i, view) in CalendarView::all().iter().enumerate() {
            probe::click(&mut app, Target::ViewTab(i));
            assert_eq!(app.view, *view);
        }

        app.view = CalendarView::Month;
        app.view_date = june_2024();
        probe::click(&mut app, Target::NavForward);
        assert_eq!(app.view_date.month, 7);
        probe::click(&mut app, Target::NavBack);
        assert_eq!(app.view_date.month, 6);

        app.view_date = Date {
            year: 2020,
            month: 1,
            day: 1,
        };
        probe::click(&mut app, Target::TodayButton);
        assert_eq!(app.view_date, app.today);
    }

    #[test]
    fn the_category_filter_actually_filters() {
        // It used to change the colour of its own swatch and nothing else:
        // every view called `store.events_on` directly.
        let today = june_2024();
        let mut app = sample_app(today);
        let unfiltered = app.visible_events_on(today).len();
        assert!(unfiltered > 0, "the sample data has nothing on the day");

        let index = EventCategory::all()
            .iter()
            .position(|c| *c == EventCategory::Meeting)
            .expect("Meeting is a category");
        probe::click(&mut app, Target::CategoryFilter(index));
        assert_eq!(app.category_filter, Some(EventCategory::Meeting));
        assert!(
            app.visible_events_on(today)
                .iter()
                .all(|e| e.category == EventCategory::Meeting)
        );

        // Clicking the same swatch again clears the filter.
        probe::click(&mut app, Target::CategoryFilter(index));
        assert_eq!(app.category_filter, None);
        assert_eq!(app.visible_events_on(today).len(), unfiltered);
    }

    #[test]
    fn the_late_evening_is_reachable_in_the_day_view() {
        // 24 hours at 60px is 1440px of grid in a 720px window. Before there
        // was a window there was no wheel, and everything after early
        // afternoon was simply unreachable.
        let today = june_2024();
        let mut app = sample_app(today);
        app.view = CalendarView::Day;
        app.view_date = today;
        let id = add_event_at(&mut app, today, 22, "Late film");

        assert!(
            app.max_content_scroll() > 0.0,
            "the day view did not scroll"
        );
        assert!(
            probe::rect_of(&app, Target::Event(id)).is_none(),
            "a 22:00 event was clickable before scrolling to it"
        );

        app.content_scroll = app.max_content_scroll();
        let rect = probe::rect_of(&app, Target::Event(id))
            .expect("scrolling to the bottom should reach a 22:00 event");
        let content = app.layout().content;
        assert!(rect.y >= content.y - 0.01 && rect.bottom() <= content.bottom() + 0.01);

        // And the click resolves to it where it is now drawn.
        let (cx, cy) = rect.centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::Event(id)));
    }

    #[test]
    fn the_wheel_scrolls_the_week_view_and_stops_at_both_ends() {
        let mut app = sample_app(june_2024());
        app.view = CalendarView::Week;
        let content = app.layout().content;
        let (x, y) = content.centre();

        let scroll = |app: &mut CalendarApp, dy: f32| {
            handle_event(
                app,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy },
                }),
            )
        };

        for _ in 0..100 {
            scroll(&mut app, -1.0);
        }
        let bottom = app.content_scroll;
        assert!(bottom > 0.0, "the wheel did not move the week view");
        assert_eq!(bottom, app.max_content_scroll());

        for _ in 0..200 {
            scroll(&mut app, 1.0);
        }
        assert_eq!(app.content_scroll, 0.0);
    }

    #[test]
    fn the_wheel_over_the_sidebar_steps_the_mini_calendar() {
        let mut app = sample_app(june_2024());
        let bar = app.layout().sidebar.expect("the sidebar fits at 1280x720");
        let (x, y) = bar.centre();
        let start = app.mini_cal_month;

        handle_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            }),
        );
        assert_ne!(app.mini_cal_month, start);
        assert_eq!(app.content_scroll, 0.0, "the content moved too");
    }

    #[test]
    fn a_search_puts_its_results_where_they_can_be_seen() {
        // `search_results` was computed by `search()` and then read by nothing
        // at all — there was no search UI.
        let mut app = sample_app(june_2024());
        probe::click(&mut app, Target::SearchField);
        assert!(app.search_focused);

        probe::type_str(&mut app, "Lunch");
        assert_eq!(app.search_query, "Lunch");
        assert_eq!(app.view, CalendarView::Agenda);
        assert!(!app.search_results.is_empty());
        let listed = app.agenda_events();
        assert!(!listed.is_empty());
        assert!(listed.iter().all(|e| e.title.contains("Lunch")));

        // Escape empties the box and hands the keyboard back.
        probe::key(&mut app, &probe::press(Key::Escape));
        assert!(app.search_query.is_empty());
        assert!(!app.search_focused);
    }

    #[test]
    fn a_selected_event_is_outlined_where_it_is_drawn() {
        let today = june_2024();
        let mut app = sample_app(today);
        app.view = CalendarView::Day;
        app.view_date = today;
        let id = add_event_at(&mut app, today, 9, "Standup");

        let before = render(&app)
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        probe::click(&mut app, Target::Event(id));
        assert_eq!(app.selected_event_id, Some(id));
        let after = render(&app)
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert!(
            after > before,
            "selecting an event drew no outline ({before} -> {after})"
        );

        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(app.selected_event_id, None);
    }

    #[test]
    fn a_year_view_day_click_opens_that_month() {
        let mut app = sample_app(june_2024());
        app.view = CalendarView::Year;
        let target = Date {
            year: 2024,
            month: 11,
            day: 5,
        };
        probe::click(&mut app, Target::Day(target));
        assert_eq!(app.view, CalendarView::Month);
        assert_eq!(app.view_date.month, 11);
        assert_eq!(app.selected_date, target);
    }

    #[test]
    fn the_keyboard_navigates_switches_view_and_scrolls() {
        let mut app = sample_app(june_2024());
        app.view = CalendarView::Month;

        probe::key(&mut app, &probe::press(Key::Right));
        assert_eq!(app.view_date.month, 7);
        probe::key(&mut app, &probe::press(Key::Left));
        assert_eq!(app.view_date.month, 6);
        probe::key(&mut app, &probe::press(Key::Home));
        assert_eq!(app.view_date, app.today);

        probe::key(&mut app, &probe::press(Key::Num3));
        assert_eq!(app.view, CalendarView::Day);
        probe::key(&mut app, &probe::press(Key::Down));
        assert!(app.content_scroll > 0.0);
        probe::key(&mut app, &probe::press(Key::Up));
        assert_eq!(app.content_scroll, 0.0);

        probe::key(&mut app, &probe::ctrl(Key::B));
        assert!(!app.sidebar_visible);
        assert_eq!(app.layout().content.x, 0.0);
        probe::key(&mut app, &probe::ctrl(Key::F));
        assert!(app.search_focused);
    }

    #[test]
    fn growing_the_window_gives_back_the_scroll_it_no_longer_needs() {
        let mut app = sample_app(june_2024());
        app.view = CalendarView::Week;
        app.content_scroll = app.max_content_scroll();
        assert!(app.content_scroll > 0.0);

        // Tall enough that the whole 24-hour grid fits with room to spare.
        app.resize(1280.0, 2400.0);
        assert_eq!(app.max_content_scroll(), 0.0);
        assert_eq!(
            app.content_scroll, 0.0,
            "a stale offset left a gap nothing could scroll back"
        );
    }

    #[test]
    fn closing_the_window_stops_the_app() {
        let mut app = sample_app(june_2024());
        assert!(app.running);
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
        assert!(!app.running);
    }

    #[test]
    fn a_resize_event_is_what_moves_the_layout() {
        let mut app = sample_app(june_2024());
        assert!(matches!(
            app.on_event(&Event::Resize {
                width: 900,
                height: 500
            }),
            Response::Redraw
        ));
        assert_eq!(app.width, 900.0);
        assert_eq!(app.layout().window.h, 500.0);
    }

    #[test]
    fn today_comes_from_the_clock_rather_than_a_literal() {
        // `main` opened on a hardcoded 2026-05-18, so every "today" highlight
        // in the app pointed at a fixed day.
        let today = today_from_clock().expect("the system clock is after 1970");
        assert!(
            today.year >= 2024 && today.year < 2200,
            "the clock read {today:?}"
        );
        assert!(today.month >= 1 && today.month <= 12);
        assert!(today.day >= 1 && today.day <= 31);
    }
}
