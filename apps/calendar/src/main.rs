//! Calendar and scheduling application for SlateOS.
//!
//! Provides month/week/day/year/agenda views, events added, changed and
//! deleted in a form (with a repeat and a category), kept in the settings
//! folder as they change, ICS import/export, and a mini-calendar sidebar.
//!
//! Opens as a real window, 1280x720 to start with and resizable from there.
//! The whole calendar is drawn as a [`Frame`]: every clickable thing records
//! the box it was painted in, as it is painted, and the hit test reads those
//! boxes back. Nothing here answers "where is that day" twice.

use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
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
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use textfmt::tsv;
use unsaved::{Choice, Question};

// The colours come from the user's palette (822, 838).
//
// The last three -- pink, flamingo and rosewater -- outlasted this
// crate's conversion because the shared palette had no rung for them.
// That was true of five applications at once, so it was the palette's
// gap rather than this file's, and the palette has them now.
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

    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Work => pal.blue,
            Self::Personal => pal.green,
            Self::Health => pal.red,
            Self::Travel => pal.peach,
            Self::Birthday => pal.pink,
            Self::Holiday => pal.yellow,
            Self::Meeting => pal.mauve,
            Self::Deadline => pal.red,
            Self::Social => pal.teal,
            Self::Education => pal.sky,
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
    pub fn effective_color(&self, pal: &Palette) -> Color {
        self.color_override
            .unwrap_or_else(|| self.category.color(pal))
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
    /// How many changes the store has had. The window keeps the events on
    /// disk and writes them when this moves; counted here, where every change
    /// happens, rather than by each of the places that make one (§1206).
    revision: u64,
}

impl EventStore {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_id: 1,
            revision: 0,
        }
    }

    /// A store holding `events`, as read back from the file: new ids go past
    /// every id they use.
    pub fn from_events(events: Vec<CalendarEvent>) -> Self {
        let next_id = events
            .iter()
            .map(|e| e.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Self {
            events,
            next_id,
            revision: 0,
        }
    }

    /// How many changes the store has had.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn add(&mut self, mut event: CalendarEvent) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        event.id = id;
        self.events.push(event);
        self.changed();
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let len_before = self.events.len();
        self.events.retain(|e| e.id != id);
        let removed = self.events.len() < len_before;
        if removed {
            self.changed();
        }
        removed
    }

    pub fn get(&self, id: u64) -> Option<&CalendarEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    /// Event `id`, to change -- counted as changed: nothing borrows an event
    /// mutably but to change it.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut CalendarEvent> {
        let at = self.events.iter().position(|e| e.id == id)?;
        self.changed();
        self.events.get_mut(at)
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

    /// Search events by title, notes and place -- without regard to case,
    /// in any script: `to_ascii_lowercase` left "Été" unfound by "été".
    pub fn search(&self, query: &str) -> Vec<&CalendarEvent> {
        let lower = query.to_lowercase();
        self.events
            .iter()
            .filter(|e| {
                e.title.to_lowercase().contains(&lower)
                    || e.description.to_lowercase().contains(&lower)
                    || e.location
                        .as_deref()
                        .is_some_and(|l| l.to_lowercase().contains(&lower))
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
        if count > 0 {
            self.changed();
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
// The kept calendar
// ============================================================================

/// The first line of the events file, and the format it names.
const CALENDAR_FORMAT: &str = "slateos-calendar\t1";

/// The largest events file this will read. One cut short would be read as a
/// calendar missing its last events, with nothing to say so, and the next
/// change would write the loss back -- so a larger file is refused whole.
const MAX_CALENDAR_BYTES: usize = 16 * 1024 * 1024;

/// Why nothing is kept, when the environment names no home directory.
const NO_HOME: &str = "Nothing is kept: no home directory is set";

/// Where the events are kept, or `None` when the environment names no home
/// directory.
fn events_path() -> Option<std::path::PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("calendar").join("events.txt"))
}

/// A date as written: `YYYY-MM-DD`.
fn date_text(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

/// A date read back from `YYYY-MM-DD`, or `None` if it is not one -- a
/// thirty-first of April included.
fn parse_date_text(text: &str) -> Option<Date> {
    let mut parts = text.trim().splitn(3, '-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    Date::new(year, month, day)
}

/// A time read back from `HH:MM` (or `H:MM`), or `None` if it is not one.
fn parse_time_text(text: &str) -> Option<Time> {
    let (h, m) = text.trim().split_once(':')?;
    if m.len() != 2 {
        return None;
    }
    Time::new(h.parse().ok()?, m.parse().ok()?)
}

/// A category as written: its name, in lower case.
fn category_key(c: EventCategory) -> String {
    c.label().to_lowercase()
}

/// How a repeat is written: `none`, `daily`, `weekly` (the start's weekday),
/// `weekly:1,3` (those weekdays, Sunday 0), `biweekly`, `monthly`, `yearly`,
/// `every:N` (every N days).
fn repeat_key(rule: &RecurrenceRule) -> String {
    match rule {
        RecurrenceRule::None => String::from("none"),
        RecurrenceRule::Daily => String::from("daily"),
        RecurrenceRule::Weekly { days } if days.is_empty() => String::from("weekly"),
        RecurrenceRule::Weekly { days } => format!(
            "weekly:{}",
            days.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        RecurrenceRule::BiWeekly => String::from("biweekly"),
        RecurrenceRule::Monthly => String::from("monthly"),
        RecurrenceRule::Yearly => String::from("yearly"),
        RecurrenceRule::Custom { interval_days } => format!("every:{interval_days}"),
    }
}

/// A repeat read back from [`repeat_key`]'s spelling.
fn parse_repeat_key(text: &str) -> Option<RecurrenceRule> {
    Some(match text {
        "none" => RecurrenceRule::None,
        "daily" => RecurrenceRule::Daily,
        "weekly" => RecurrenceRule::Weekly { days: Vec::new() },
        "biweekly" => RecurrenceRule::BiWeekly,
        "monthly" => RecurrenceRule::Monthly,
        "yearly" => RecurrenceRule::Yearly,
        _ => {
            if let Some(days) = text.strip_prefix("weekly:") {
                let days = days
                    .split(',')
                    .map(|d| d.parse::<u32>().ok().filter(|d| *d < 7))
                    .collect::<Option<Vec<u32>>>()?;
                RecurrenceRule::Weekly { days }
            } else {
                RecurrenceRule::Custom {
                    interval_days: text.strip_prefix("every:")?.parse().ok()?,
                }
            }
        }
    })
}

/// How a reminder is written: `none`, `at`, `minutes:N`, `hours:N`, `day`.
fn reminder_key(r: Reminder) -> String {
    match r {
        Reminder::None => String::from("none"),
        Reminder::AtTime => String::from("at"),
        Reminder::MinutesBefore(n) => format!("minutes:{n}"),
        Reminder::HoursBefore(n) => format!("hours:{n}"),
        Reminder::DayBefore => String::from("day"),
    }
}

/// A reminder read back from [`reminder_key`]'s spelling.
fn parse_reminder_key(text: &str) -> Option<Reminder> {
    Some(match text {
        "none" => Reminder::None,
        "at" => Reminder::AtTime,
        "day" => Reminder::DayBefore,
        _ => {
            if let Some(n) = text.strip_prefix("minutes:") {
                Reminder::MinutesBefore(n.parse().ok()?)
            } else {
                Reminder::HoursBefore(text.strip_prefix("hours:")?.parse().ok()?)
            }
        }
    })
}

/// A colour as written: `#RRGGBB`, or `#RRGGBBAA` when it is not opaque.
fn colour_text(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

/// A colour read back from `#RRGGBB` or `#RRGGBBAA`.
fn parse_colour_text(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    if !hex.is_ascii() || !(hex.len() == 6 || hex.len() == 8) {
        return None;
    }
    let byte = |at: usize| {
        hex.get(at..at.saturating_add(2))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, a))
}

/// The events as the file holds them: the format line, then one line per
/// event, in the order they were added.
///
/// Tab-separated, one event to a line, as the notes library and the address
/// book are kept (design-decisions §1205, §1206); the free text -- title,
/// place, notes -- escaped with `textfmt::tsv`, so a tab or a line break in
/// them cannot start a new field or a new event.
fn calendar_text(store: &EventStore) -> String {
    let mut out = String::from(CALENDAR_FORMAT);
    out.push('\n');
    for e in store.all() {
        let fields = [
            String::from("event"),
            e.id.to_string(),
            date_text(e.start.date),
            e.start.time.format_24h(),
            date_text(e.end.date),
            e.end.time.format_24h(),
            String::from(if e.all_day { "y" } else { "n" }),
            category_key(e.category),
            repeat_key(&e.recurrence),
            reminder_key(e.reminder),
            e.color_override
                .map_or_else(|| String::from("-"), colour_text),
            tsv::escape(&e.title),
            tsv::escape(e.location.as_deref().unwrap_or("")),
            tsv::escape(&e.description),
        ];
        out.push_str(&fields.join("\t"));
        out.push('\n');
    }
    out
}

/// The events read back from [`calendar_text`]'s format, or why they cannot
/// be: read whole or not at all, for the finance ledger's reason
/// (design-decisions §1202) -- a calendar read in part and kept again would
/// lose what was not read. The refusal names the line.
fn parse_calendar(text: &str) -> Result<Vec<CalendarEvent>, String> {
    let mut lines = text.lines();
    match lines.next() {
        Some(first) if first == CALENDAR_FORMAT => {}
        Some(first) if first.starts_with("slateos-calendar\t") => {
            return Err(format!(
                "it is written in a later format ({}) than this version reads",
                first.trim_start_matches("slateos-calendar\t")
            ));
        }
        _ => return Err(String::from("it is not a SlateOS calendar")),
    }
    let mut events: Vec<CalendarEvent> = Vec::new();
    for (i, line) in lines.enumerate() {
        let n = i.saturating_add(2);
        if line.is_empty() {
            continue;
        }
        let bad = |why: &str| format!("line {n}: {why}");
        let fields: Vec<&str> = line.split('\t').collect();
        let [
            kind,
            id,
            start_date,
            start_time,
            end_date,
            end_time,
            all_day,
            category,
            repeats,
            reminder,
            colour,
            title,
            place,
            notes,
        ] = fields.as_slice()
        else {
            return Err(bad(&format!(
                "{} fields where an event has 14",
                fields.len()
            )));
        };
        if *kind != "event" {
            return Err(bad("it is not an event"));
        }
        let id = id
            .parse::<u64>()
            .ok()
            .filter(|&id| id > 0 && id < u64::MAX)
            .ok_or_else(|| bad("its number is not one"))?;
        if events.iter().any(|e| e.id == id) {
            return Err(bad(&format!("another event has its number ({id})")));
        }
        let date = |t: &str, what: &str| {
            parse_date_text(t).ok_or_else(|| bad(&format!("its {what} date is not a date")))
        };
        let time = |t: &str, what: &str| {
            parse_time_text(t).ok_or_else(|| bad(&format!("its {what} time is not a time")))
        };
        let start = DateTime::new(date(start_date, "start")?, time(start_time, "start")?);
        let end = DateTime::new(date(end_date, "end")?, time(end_time, "end")?);
        let all_day = match *all_day {
            "y" => true,
            "n" => false,
            _ => return Err(bad("whether it lasts all day is not said")),
        };
        let category = EventCategory::all()
            .iter()
            .copied()
            .find(|c| category_key(*c) == *category)
            .ok_or_else(|| {
                bad(&format!(
                    "its category ({category}) is not one this version has"
                ))
            })?;
        let recurrence = parse_repeat_key(repeats).ok_or_else(|| {
            bad(&format!(
                "how it repeats ({repeats}) is not one this version reads"
            ))
        })?;
        let reminder = parse_reminder_key(reminder).ok_or_else(|| {
            bad(&format!(
                "its reminder ({reminder}) is not one this version reads"
            ))
        })?;
        let color_override = match *colour {
            "-" => None,
            c => Some(parse_colour_text(c).ok_or_else(|| bad("its colour is not one"))?),
        };
        let text = |t: &str, what: &str| {
            tsv::unescape(t).ok_or_else(|| bad(&format!("its {what} has a broken escape")))
        };
        let place = text(place, "place")?;
        events.push(CalendarEvent {
            id,
            title: text(title, "title")?,
            description: text(notes, "notes")?,
            category,
            start,
            end,
            all_day,
            recurrence,
            reminder,
            location: (!place.is_empty()).then_some(place),
            color_override,
        });
    }
    Ok(events)
}

// ============================================================================
// The event form
// ============================================================================

/// The size a form field's text is drawn at, which moving its caret needs.
const FORM_TEXT_SIZE: f32 = 13.0;
/// A form row's height.
const FORM_ROW_H: f32 = 40.0;

/// A field of the event form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    Title,
    Date,
    AllDay,
    Starts,
    Ends,
    Category,
    Repeats,
    Place,
    Notes,
}

impl FormField {
    /// Whether the field is typed into; the others are chosen, a press or
    /// Left, Right and Space stepping through their values.
    fn is_text(self) -> bool {
        !matches!(self, Self::AllDay | Self::Category | Self::Repeats)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Date => "Date",
            Self::AllDay => "All day",
            Self::Starts => "Starts",
            Self::Ends => "Ends",
            Self::Category => "Category",
            Self::Repeats => "Repeats",
            Self::Place => "Place",
            Self::Notes => "Notes",
        }
    }

    /// What an empty text field says it wants.
    fn placeholder(self) -> &'static str {
        match self {
            Self::Title => "What it is",
            Self::Date => "YYYY-MM-DD",
            Self::Starts | Self::Ends => "HH:MM, 24-hour",
            Self::Place | Self::Notes => "Optional",
            Self::AllDay | Self::Category | Self::Repeats => "",
        }
    }

    /// The most characters the field holds.
    fn capacity(self) -> usize {
        match self {
            Self::Title | Self::Place => 200,
            Self::Notes => 2000,
            Self::Date => 16,
            Self::Starts | Self::Ends => 5,
            Self::AllDay | Self::Category | Self::Repeats => 0,
        }
    }
}

/// The repeats the form offers for any event, in order.
fn standard_repeats() -> [RecurrenceRule; 6] {
    [
        RecurrenceRule::None,
        RecurrenceRule::Daily,
        // No days: on the weekday it starts, whatever the date is changed to.
        RecurrenceRule::Weekly { days: Vec::new() },
        RecurrenceRule::BiWeekly,
        RecurrenceRule::Monthly,
        RecurrenceRule::Yearly,
    ]
}

/// A repeat in words.
fn describe_repeat(rule: &RecurrenceRule) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    match rule {
        RecurrenceRule::Weekly { days } if days.is_empty() => {
            String::from("Weekly, on the day it starts")
        }
        RecurrenceRule::Weekly { days } => format!(
            "Weekly on {}",
            days.iter()
                .filter_map(|d| DAYS.get(usize::try_from(*d).ok()?).copied())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        RecurrenceRule::Custom { interval_days } => format!("Every {interval_days} days"),
        other => other.label().to_owned(),
    }
}

/// The value `step` places after (or before) `at`, round from the last to
/// the first.
fn step_index(at: usize, len: usize, forward: bool) -> usize {
    if forward {
        at.saturating_add(1).checked_rem(len).unwrap_or(0)
    } else {
        at.checked_sub(1).unwrap_or(len.saturating_sub(1))
    }
}

/// A text field holding `value`.
fn field_with(value: &str) -> TextInput {
    let mut input = TextInput::new();
    input.set_text(value);
    input
}

/// The form an event is added or changed in.
///
/// There was no way to add an event to this calendar: `EventStore::add` had
/// no caller but the tests and the `.ics` import, and nothing could change
/// or delete one either.
#[derive(Clone, Debug)]
pub struct EventForm {
    /// The event being changed, or `None` for a new one.
    pub id: Option<u64>,
    title: TextInput,
    date: TextInput,
    starts: TextInput,
    ends: TextInput,
    all_day: bool,
    category: EventCategory,
    repeats: RecurrenceRule,
    /// A repeat the list does not have -- weekly on several days, or every
    /// so many days, as an imported calendar may say -- offered beside the
    /// list, so that changing an event's title does not change its repeat.
    other_repeat: Option<RecurrenceRule>,
    place: TextInput,
    notes: TextInput,
    /// Kept as the event had them: the form does not show them. Nothing in
    /// this program raises a reminder, so offering to set one would promise
    /// an alert that never comes.
    reminder: Reminder,
    color_override: Option<Color>,
}

impl EventForm {
    /// A new event on `date`, from nine to ten.
    pub fn new_on(date: Date) -> Self {
        Self {
            id: None,
            title: TextInput::new(),
            date: field_with(&date_text(date)),
            starts: field_with("09:00"),
            ends: field_with("10:00"),
            all_day: false,
            category: EventCategory::Personal,
            repeats: RecurrenceRule::None,
            other_repeat: None,
            place: TextInput::new(),
            notes: TextInput::new(),
            reminder: Reminder::None,
            color_override: None,
        }
    }

    /// Event `e`, to change.
    pub fn editing(e: &CalendarEvent) -> Self {
        Self {
            id: Some(e.id),
            title: field_with(&e.title),
            date: field_with(&date_text(e.start.date)),
            starts: field_with(&e.start.time.format_24h()),
            ends: field_with(&e.end.time.format_24h()),
            all_day: e.all_day,
            category: e.category,
            repeats: e.recurrence.clone(),
            other_repeat: (!standard_repeats().contains(&e.recurrence))
                .then(|| e.recurrence.clone()),
            place: field_with(e.location.as_deref().unwrap_or("")),
            notes: field_with(&e.description),
            reminder: e.reminder,
            color_override: e.color_override,
        }
    }

    /// The fields, in the order Tab walks them: the times only while the
    /// event is not all day.
    pub fn fields(&self) -> &'static [FormField] {
        if self.all_day {
            &[
                FormField::Title,
                FormField::Date,
                FormField::AllDay,
                FormField::Category,
                FormField::Repeats,
                FormField::Place,
                FormField::Notes,
            ]
        } else {
            &[
                FormField::Title,
                FormField::Date,
                FormField::AllDay,
                FormField::Starts,
                FormField::Ends,
                FormField::Category,
                FormField::Repeats,
                FormField::Place,
                FormField::Notes,
            ]
        }
    }

    /// Text field `which`, to type into.
    fn input(&mut self, which: FormField) -> Option<&mut TextInput> {
        match which {
            FormField::Title => Some(&mut self.title),
            FormField::Date => Some(&mut self.date),
            FormField::Starts => Some(&mut self.starts),
            FormField::Ends => Some(&mut self.ends),
            FormField::Place => Some(&mut self.place),
            FormField::Notes => Some(&mut self.notes),
            FormField::AllDay | FormField::Category | FormField::Repeats => None,
        }
    }

    /// Text field `which`, to read.
    fn input_ref(&self, which: FormField) -> Option<&TextInput> {
        match which {
            FormField::Title => Some(&self.title),
            FormField::Date => Some(&self.date),
            FormField::Starts => Some(&self.starts),
            FormField::Ends => Some(&self.ends),
            FormField::Place => Some(&self.place),
            FormField::Notes => Some(&self.notes),
            FormField::AllDay | FormField::Category | FormField::Repeats => None,
        }
    }

    /// The repeats this form offers.
    fn repeat_choices(&self) -> Vec<RecurrenceRule> {
        let mut out = standard_repeats().to_vec();
        out.extend(self.other_repeat.clone());
        out
    }

    /// Step chosen field `which` on (`forward`) or back. Whether it is one.
    fn step(&mut self, which: FormField, forward: bool) -> bool {
        match which {
            FormField::AllDay => self.all_day = !self.all_day,
            FormField::Category => {
                let all = EventCategory::all();
                let at = all.iter().position(|c| *c == self.category).unwrap_or(0);
                if let Some(next) = all.get(step_index(at, all.len(), forward)) {
                    self.category = *next;
                }
            }
            FormField::Repeats => {
                let choices = self.repeat_choices();
                let at = choices.iter().position(|r| *r == self.repeats).unwrap_or(0);
                if let Some(next) = choices.get(step_index(at, choices.len(), forward)) {
                    self.repeats = next.clone();
                }
            }
            _ => return false,
        }
        true
    }

    /// What chosen field `which` shows.
    fn choice_label(&self, which: FormField) -> String {
        match which {
            FormField::AllDay => String::from(if self.all_day { "Yes" } else { "No" }),
            FormField::Category => {
                format!("{} {}", self.category.icon(), self.category.label())
            }
            FormField::Repeats => describe_repeat(&self.repeats),
            _ => String::new(),
        }
    }

    /// The event the form describes, or what is wrong with it -- said in
    /// the form, which stays up to be put right.
    pub fn to_event(&self) -> Result<CalendarEvent, String> {
        let title = self.title.text().trim();
        if title.is_empty() {
            return Err(String::from("Give it a title"));
        }
        let date = parse_date_text(self.date.text())
            .ok_or("The date is not one -- write it as YYYY-MM-DD, like 2026-09-26")?;
        let (start, end) = if self.all_day {
            (
                Time { hour: 0, minute: 0 },
                Time {
                    hour: 23,
                    minute: 59,
                },
            )
        } else {
            let starts = parse_time_text(self.starts.text())
                .ok_or("The start is not a time -- write it as HH:MM, like 09:30")?;
            let ends = parse_time_text(self.ends.text())
                .ok_or("The end is not a time -- write it as HH:MM, like 17:00")?;
            if ends < starts {
                return Err(String::from("It ends before it starts"));
            }
            (starts, ends)
        };
        let place = self.place.text().trim();
        Ok(CalendarEvent {
            id: self.id.unwrap_or(0),
            title: title.to_owned(),
            description: self.notes.text().to_owned(),
            category: self.category,
            start: DateTime::new(date, start),
            end: DateTime::new(date, end),
            all_day: self.all_day,
            recurrence: self.repeats.clone(),
            reminder: self.reminder,
            location: (!place.is_empty()).then(|| place.to_owned()),
            color_override: self.color_override,
        })
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
    /// The top bar's "New event" button.
    NewEvent,
    /// A field of the event form: a press gives it the keys, and steps a
    /// chosen field on.
    Field(FormField),
    /// The arrows either side of a chosen field.
    StepBack(FormField),
    StepForward(FormField),
    /// The form's buttons.
    Save,
    Cancel,
    DeleteEvent,
    /// Around and behind the form's controls: a press does nothing, since
    /// the form is modal.
    FormBackdrop,
    /// The answers to "Delete this event?", and around them.
    ConfirmDelete,
    KeepEvent,
    ConfirmBackdrop,
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
/// Left edge of the header text, immediately right of the Today button --
/// or of the New event button, when there is one, by as much again.
const HEADER_X: f32 = 160.0;
/// Right edge of the Today button, which nothing may be placed left of.
const CHROME_LEFT: f32 = 152.0;
/// The New event button, after Today, where the bar can spare it.
const NEW_EVENT_X: f32 = 152.0;
const NEW_EVENT_W: f32 = 80.0;
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
    /// "New event", beside Today -- or `None` in a window too narrow to
    /// spare it beside the view tabs, which give way to nothing. N adds an
    /// event either way.
    pub new_event_button: Option<Rect>,
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
    /// The strip under the top bar the empty calendar's two lines are drawn
    /// in, or `None` when there are no lines to draw.
    pub notice: Option<Rect>,
}

/// One line of the notice strip, top to top.
const NOTICE_LINE_H: f32 = 14.0;

impl Layout {
    /// Work out where everything goes in a `width` x `height` window.
    pub fn new(width: f32, height: f32, sidebar_wanted: bool) -> Self {
        Self::with_notice(width, height, sidebar_wanted, 0)
    }

    /// [`new`](Self::new), with room under the top bar for `notice_lines`
    /// lines that nothing else is drawn over.
    ///
    /// The lines were drawn at the top of the window, before the top bar,
    /// which filled the same pixels: in every frame, on no screen. The sidebar
    /// and the views start below the strip now.
    pub fn with_notice(width: f32, height: f32, sidebar_wanted: bool, notice_lines: usize) -> Self {
        let window = Rect::new(0.0, 0.0, width, height);
        let top_bar = Rect::new(0.0, 0.0, width, TOP_BAR_H.min(height));

        // The view selector, from the right edge inwards. It never wraps and
        // never runs off the window: when the room is short the tabs get
        // narrower, because a tab that is off-screen cannot be pressed and
        // there is no other way to change view.
        let count = CalendarView::all().len() as f32;
        // The New event button, where the tabs still have their narrowest
        // pitch beside it.
        let new_right = NEW_EVENT_X + NEW_EVENT_W;
        let new_event_button = (width - 8.0 - MIN_VIEW_TAB_PITCH * count >= new_right + 8.0)
            .then(|| Rect::new(NEW_EVENT_X, CHROME_Y, NEW_EVENT_W, CHROME_H));
        let (chrome_left, header_x) = if new_event_button.is_some() {
            (new_right + 8.0, new_right + 8.0 + (HEADER_X - CHROME_LEFT))
        } else {
            (CHROME_LEFT, HEADER_X)
        };
        let room = (width - 8.0 - chrome_left).max(0.0);
        let view_tab_pitch = (room / count).clamp(MIN_VIEW_TAB_PITCH, VIEW_TAB_PITCH);
        let tabs_run = view_tab_pitch * count;
        let view_tabs_x = (width - 8.0 - tabs_run).max(chrome_left);

        // What is left between the Today button and the tabs, spent on the
        // search box first and the caption second.
        let mut right = view_tabs_x - 8.0;
        let search = if right - header_x - MIN_HEADER_W >= SEARCH_W {
            let rect = Rect::new(right - SEARCH_W, CHROME_Y, SEARCH_W, CHROME_H);
            right = rect.x - 8.0;
            Some(rect)
        } else {
            None
        };
        let header_w = right - header_x;
        let header = if header_w >= MIN_HEADER_W {
            Some(Rect::new(header_x, CHROME_Y, header_w, CHROME_H))
        } else {
            None
        };

        #[allow(clippy::cast_precision_loss, reason = "a handful of lines")]
        let notice_h = if notice_lines == 0 {
            0.0
        } else {
            (notice_lines as f32 * NOTICE_LINE_H + 6.0).min((height - CONTENT_Y).max(0.0))
        };
        let notice = (notice_h > 0.0).then(|| Rect::new(0.0, CONTENT_Y, width, notice_h));
        let content_top = (CONTENT_Y + notice_h).min(height);
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
            new_event_button,
            header,
            search,
            view_tabs_x,
            view_tab_pitch,
            sidebar,
            content,
            notice,
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

/// The most of an `.ics` file one open will read.
///
/// Reported when it bites. `parse_ics` stops at a truncation without
/// complaining -- a cut file simply yields fewer events, and a calendar
/// missing an appointment looks exactly like a calendar that never had one.
pub const MAX_ICS_BYTES: usize = 8 * 1024 * 1024;

/// Leads a message about a file operation that did not happen.
const FILE_FAILED_PREFIX: &str = "Could not";

pub struct CalendarApp {
    /// The open or save picker, while one is up.
    /// The open or save picker. Holds the dialog, the saving flag and the
    /// routing that ten applications used to write out by hand.
    pub picker: FilePicker,
    /// What the last open or save did, for the status line.
    pub last_file_action: Option<String>,
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
    /// Whether the shortcut list is up.
    pub show_help: bool,
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
    /// Whether the events are kept: set by `from_settings`, never by `new`,
    /// so a window a test makes writes nothing.
    persist: bool,
    /// The store's revision when the events were last written or read.
    kept_revision: u64,
    /// Why the events are not being kept, when they are not.
    store_error: Option<String>,
    /// "Your latest changes are not saved", while it is being asked.
    question: Option<Question<()>>,
    /// The event form, while it is up.
    pub form: Option<EventForm>,
    /// Which of the form's fields has the keys.
    pub form_field: FormField,
    /// What the last Save found wrong with the form.
    pub form_error: Option<String>,
    /// The event a delete is waiting on its answer for.
    pub pending_delete: Option<u64>,
    /// What was last copied or cut from a field of the form.
    clipboard: String,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl CalendarApp {
    pub fn new(width: f32, height: f32, today: Date) -> Self {
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            width,
            height,
            view: CalendarView::Month,
            today,
            selected_date: today,
            view_date: today,
            picker: FilePicker::new(),
            last_file_action: None,
            store: EventStore::new(),
            sidebar_visible: true,
            show_help: false,
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
            persist: false,
            kept_revision: 0,
            store_error: None,
            question: None,
            form: None,
            form_field: FormField::Title,
            form_error: None,
            pending_delete: None,
            clipboard: String::new(),
        }
    }

    /// The window's calendar: the events kept last time, and every change
    /// kept from here on.
    pub fn from_settings(width: f32, height: f32, today: Date) -> Self {
        let mut app = Self::new(width, height, today);
        match events_path() {
            Some(path) => {
                app.persist = true;
                app.load_events(&path);
            }
            // Nowhere to keep anything, and never will be while this window
            // is open: said once, and not asked about again at every close.
            None => app.store_error = Some(String::from(NO_HOME)),
        }
        app
    }

    /// Read the events at `path`; with none there yet, this is a first run.
    ///
    /// A file that cannot be read whole is left exactly as it is: nothing is
    /// saved over it, and the window says so for as long as it is open.
    fn load_events(&mut self, path: &std::path::Path) {
        self.load_events_within(path, MAX_CALENDAR_BYTES);
    }

    /// [`load_events`](Self::load_events) with the size limit given, so a
    /// test can reach it without writing sixteen megabytes.
    fn load_events_within(&mut self, path: &std::path::Path, max_bytes: usize) {
        let refused = |why: String| {
            format!(
                "{} was not read ({why}), so nothing is saved over it",
                path.display()
            )
        };
        let read = match safeio::read_to_string_capped(path, max_bytes) {
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                self.persist = false;
                self.store_error = Some(refused(err.to_string()));
                return;
            }
        };
        if read.truncated {
            self.persist = false;
            self.store_error = Some(refused(format!(
                "it is larger than {} MiB",
                max_bytes / (1024 * 1024)
            )));
            return;
        }
        match parse_calendar(&read.text) {
            Ok(events) => {
                self.store = EventStore::from_events(events);
                self.kept_revision = self.store.revision();
                self.selected_event_id = None;
            }
            Err(why) => {
                self.persist = false;
                self.store_error = Some(refused(why));
            }
        }
    }

    /// Write the events, if they have changed since they were last written
    /// and this window keeps anything.
    ///
    /// A failure is kept in `store_error`, drawn under the top bar, and the
    /// next event tries again.
    fn keep(&mut self) {
        if !self.persist || self.store.revision() == self.kept_revision {
            return;
        }
        let Some(path) = events_path() else {
            self.store_error = Some(String::from(NO_HOME));
            return;
        };
        let text = calendar_text(&self.store);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| safeio::write_str_atomically(&path, &text));
        match written {
            Ok(()) => {
                self.kept_revision = self.store.revision();
                self.store_error = None;
            }
            Err(err) => {
                self.store_error = Some(format!(
                    "Your calendar was not saved to {}: {err}",
                    path.display()
                ));
            }
        }
    }

    /// Whether the events hold a change that is not written.
    fn unkept(&self) -> bool {
        self.persist && self.store.revision() != self.kept_revision
    }

    /// Whether the window may close now: at once, unless the events have
    /// changes a save is failing to write, which closing would lose.
    fn request_close(&mut self) -> bool {
        self.keep();
        if !self.unkept() {
            return true;
        }
        // The question replaces whatever is up: the picker would take the keys
        // it needs, and be drawn over it.
        self.picker.close();
        self.show_help = false;
        let detail = self.store_error.clone().unwrap_or_default();
        self.question = Some(Question::new(
            "Your latest changes to your calendar are not saved.",
            &format!("{detail} -- try saving again before closing?"),
            (),
        ));
        false
    }

    /// Act on the close question's answer.
    fn answer(&mut self, choice: Choice) {
        match choice {
            // Leave only if the save now works; if it fails again the error is
            // on screen and the window stays, which is what Save asked for.
            Choice::Save => {
                self.keep();
                self.running = self.unkept();
            }
            Choice::Discard => self.running = false,
            Choice::Cancel => {}
        }
    }

    /// N, or the New event button: a new event on the selected day.
    pub fn open_new_event(&mut self) {
        self.form = Some(EventForm::new_on(self.selected_date));
        self.form_field = FormField::Title;
        self.form_error = None;
        self.search_focused = false;
        self.show_help = false;
    }

    /// Change event `id`, in the form.
    pub fn open_edit_event(&mut self, id: u64) {
        let Some(event) = self.store.get(id) else {
            return;
        };
        self.form = Some(EventForm::editing(event));
        self.form_field = FormField::Title;
        self.form_error = None;
        self.search_focused = false;
        self.show_help = false;
    }

    /// Keep what the form holds, or say in the form what is wrong with it.
    pub fn save_form(&mut self) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        match (form.to_event(), form.id) {
            (Ok(event), Some(id)) => {
                let date = event.start.date;
                if let Some(kept) = self.store.get_mut(id) {
                    *kept = CalendarEvent { id, ..event };
                }
                self.select_date(date);
            }
            (Ok(event), None) => {
                let date = event.start.date;
                let id = self.store.add(event);
                self.selected_event_id = Some(id);
                self.select_date(date);
            }
            (Err(why), _) => {
                self.form_error = Some(why);
                return;
            }
        }
        self.form = None;
        self.form_error = None;
        // A search showing results has results that may have changed.
        self.search();
        self.clamp_scroll();
    }

    /// Close the form, keeping nothing it holds.
    pub fn cancel_form(&mut self) {
        self.form = None;
        self.form_error = None;
    }

    /// Delete event `id` -- after asking, which `pending_delete` holds.
    pub fn delete_event(&mut self, id: u64) {
        self.store.remove(id);
        if self.selected_event_id == Some(id) {
            self.selected_event_id = None;
        }
        self.pending_delete = None;
        self.search();
        self.clamp_scroll();
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
        Layout::with_notice(
            self.width,
            self.height,
            self.sidebar_visible,
            self.notice_lines().len(),
        )
    }

    /// What the window has to say under the top bar, each line with whether
    /// it is a warning: why the events are not being kept, what the last
    /// import or export did, and -- while there are none -- how to add one.
    ///
    /// What the last import or export did was drawn over the top bar, across
    /// its buttons; it has a line of its own here now.
    fn notice_lines(&self) -> Vec<(String, bool)> {
        let mut lines = Vec::new();
        if let Some(error) = &self.store_error {
            lines.push((error.clone(), true));
        }
        if let Some(note) = &self.last_file_action {
            let failed = note.starts_with(FILE_FAILED_PREFIX) || note.starts_with("INCOMPLETE");
            lines.push((note.clone(), failed));
        }
        if self.store.is_empty() {
            lines.push((String::from(NO_EVENTS_LINE), false));
        }
        lines
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
        let layout = Layout::with_notice(
            width,
            height,
            self.sidebar_visible,
            self.notice_lines().len(),
        );

        fill(&mut frame, layout.window, self.palette.base, 0.0);

        self.draw_top_bar(&mut frame, &layout);
        // In the strip under the top bar, which nothing else is drawn in.
        if let Some(strip) = layout.notice {
            for (i, (line, warning)) in self.notice_lines().into_iter().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "three lines at most")]
                let ty = strip.y + 3.0 + i as f32 * NOTICE_LINE_H;
                let avail = (strip.w - 16.0).max(0.0);
                if avail <= 0.0 || ty + NOTICE_LINE_H > strip.y + strip.h {
                    break;
                }
                frame.push(RenderCommand::Text {
                    x: strip.x + 8.0,
                    y: ty,
                    text: line,
                    color: if warning {
                        self.palette.ink(self.palette.red)
                    } else {
                        self.palette.subtext0
                    },
                    font_size: 11.0,
                    font_weight: if warning {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(avail),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

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

        // The form, or the question before a delete, over the calendar they
        // are about. Neither is up with the picker: both take the keys that
        // would open it.
        if let Some(form) = &self.form {
            self.draw_form(&mut frame, form);
        } else if let Some(id) = self.pending_delete {
            self.draw_confirm_delete(&mut frame, id);
        }

        // Last, so it is above everything.
        frame.extend(
            self.picker
                .render(&self.palette, layout.window.w, layout.window.h),
        );

        // And the shortcut list over even that, because it is the one thing a
        // reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut frame,
                &self.palette,
                (layout.window.w, layout.window.h),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
        }

        frame
    }

    // ========================================================================
    // The event form and the question before a delete
    // ========================================================================

    /// Where the form's card is.
    fn form_card(&self, form: &EventForm) -> Rect {
        #[allow(clippy::cast_precision_loss, reason = "nine rows at most")]
        let rows = form.fields().len() as f32;
        let card_w = 560.0_f32.min(self.width - 24.0).max(0.0);
        let card_h = (64.0 + rows * FORM_ROW_H + 96.0)
            .min(self.height - 24.0)
            .max(0.0);
        Rect::new(
            (self.width - card_w) / 2.0,
            (self.height - card_h) / 2.0,
            card_w,
            card_h,
        )
    }

    /// The form over the window: a row per field, what the last Save found
    /// wrong, and the buttons.
    fn draw_form(&self, frame: &mut Frame, form: &EventForm) {
        let window = Rect::new(0.0, 0.0, self.width, self.height);
        fill(frame, window, Color::rgba(0, 0, 0, 150), 0.0);
        frame.hit(Target::FormBackdrop, window);
        let card = self.form_card(form);
        self.palette
            .push_surface(frame, card.x, card.y, card.w, card.h, 12.0, Surface::Card);
        label(
            frame,
            card.x + 20.0,
            card.y + 18.0,
            if form.id.is_some() {
                "Change event"
            } else {
                "New event"
            },
            16.0,
            self.palette.text,
            FontWeightHint::Bold,
            Some((card.w - 40.0).max(0.0)),
        );
        let label_w = 100.0;
        let control_w = (card.w - 40.0 - label_w).max(0.0);
        let mut y = card.y + 56.0;
        for &field in form.fields() {
            label(
                frame,
                card.x + 20.0,
                y + 9.0,
                field.label(),
                12.0,
                self.palette.subtext1,
                FontWeightHint::Regular,
                Some(label_w - 8.0),
            );
            let rect = Rect::new(card.x + 20.0 + label_w, y, control_w, 32.0);
            if field.is_text() {
                self.draw_form_text(frame, form, field, rect);
            } else {
                self.draw_form_choice(frame, form, field, rect);
            }
            y += FORM_ROW_H;
        }
        if let Some(error) = &self.form_error {
            label(
                frame,
                card.x + 20.0,
                y + 4.0,
                error.clone(),
                12.0,
                self.palette.ink(self.palette.red),
                FontWeightHint::Bold,
                Some((card.w - 40.0).max(0.0)),
            );
        }
        let buttons_y = card.bottom() - 46.0;
        let mut right = card.right() - 20.0;
        let mut buttons = vec![
            ("Save", Target::Save, true),
            ("Cancel", Target::Cancel, false),
        ];
        if form.id.is_some() {
            buttons.push(("Delete", Target::DeleteEvent, false));
        }
        for (text, target, primary) in buttons {
            let rect = Rect::new(right - 80.0, buttons_y, 80.0, 32.0);
            self.draw_button(frame, rect, text, target, primary);
            right = rect.x - 8.0;
        }
        label(
            frame,
            card.x + 20.0,
            buttons_y + 9.0,
            "Tab: next field  \u{00B7}  Enter: save  \u{00B7}  Esc: cancel",
            11.0,
            self.palette.subtext0,
            FontWeightHint::Regular,
            Some((right - card.x - 28.0).max(0.0)),
        );
    }

    /// A text field: its box, and what is typed with the caret -- or, while
    /// it is empty and the keys are elsewhere, what it wants.
    fn draw_form_text(&self, frame: &mut Frame, form: &EventForm, field: FormField, rect: Rect) {
        let focused = self.form_field == field;
        self.palette
            .push_surface(frame, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
        stroke(
            frame,
            rect,
            if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            4.0,
            if focused { 2.0 } else { 1.0 },
        );
        if let Some(input) = form.input_ref(field) {
            if input.text().is_empty() && !focused {
                label(
                    frame,
                    rect.x + 8.0,
                    rect.y + 8.0,
                    field.placeholder(),
                    FORM_TEXT_SIZE,
                    self.palette.subtext0,
                    FontWeightHint::Regular,
                    Some((rect.w - 16.0).max(0.0)),
                );
            } else {
                let mut tree = RenderTree::new();
                textedit::draw(
                    &mut tree,
                    &textedit::SingleLine {
                        text: input.text(),
                        cursor: if focused {
                            input.cursor()
                        } else {
                            guitk::text::TextCursor::default()
                        },
                        selection_anchor: if focused {
                            input.selection_anchor()
                        } else {
                            None
                        },
                        focused,
                        x: rect.x + 8.0,
                        y: rect.y + 7.0,
                        width: (rect.w - 16.0).max(0.0),
                        line_height: 18.0,
                        font_size: FORM_TEXT_SIZE,
                        weight: FontWeightHint::Regular,
                        color: self.palette.text,
                        selection_bg: self.palette.blue,
                        selection_fg: self.palette.crust,
                        caret_width: textedit::CARET_WIDTH,
                    },
                );
                frame.extend(tree.commands);
            }
        }
        frame.hit(Target::Field(field), rect);
    }

    /// A chosen field: its value between arrows.
    fn draw_form_choice(&self, frame: &mut Frame, form: &EventForm, field: FormField, rect: Rect) {
        let focused = self.form_field == field;
        let value = Rect::new(rect.x + 36.0, rect.y, (rect.w - 72.0).max(0.0), rect.h);
        self.draw_button(
            frame,
            Rect::new(rect.x, rect.y, 32.0, rect.h),
            "\u{25C0}",
            Target::StepBack(field),
            false,
        );
        self.draw_button(
            frame,
            Rect::new(rect.right() - 32.0, rect.y, 32.0, rect.h),
            "\u{25B6}",
            Target::StepForward(field),
            false,
        );
        self.palette.push_surface(
            frame,
            value.x,
            value.y,
            value.w,
            value.h,
            4.0,
            Surface::Card,
        );
        stroke(
            frame,
            value,
            if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            4.0,
            if focused { 2.0 } else { 1.0 },
        );
        let text = form.choice_label(field);
        label(
            frame,
            guitk::text::center_x(
                &text,
                value.x + value.w / 2.0,
                FORM_TEXT_SIZE,
                FontWeightHint::Regular,
            )
            .max(value.x + 8.0),
            value.y + 8.0,
            text,
            FORM_TEXT_SIZE,
            self.palette.text,
            FontWeightHint::Regular,
            Some((value.w - 16.0).max(0.0)),
        );
        frame.hit(Target::Field(field), value);
    }

    /// A button: its face, its word in the middle, and its hit box.
    fn draw_button(
        &self,
        frame: &mut Frame,
        rect: Rect,
        text: &str,
        target: Target,
        primary: bool,
    ) {
        fill(
            frame,
            rect,
            if primary {
                self.palette.blue
            } else {
                self.palette.surface0
            },
            4.0,
        );
        label(
            frame,
            guitk::text::center_x(text, rect.x + rect.w / 2.0, 12.0, FontWeightHint::Bold)
                .max(rect.x + 4.0),
            rect.y + rect.h / 2.0 - 7.0,
            text,
            12.0,
            if primary {
                self.palette.crust
            } else {
                self.palette.text
            },
            FontWeightHint::Bold,
            Some((rect.w - 8.0).max(0.0)),
        );
        frame.hit(target, rect);
    }

    /// "Delete this event?", with a button for each answer.
    fn draw_confirm_delete(&self, frame: &mut Frame, id: u64) {
        let window = Rect::new(0.0, 0.0, self.width, self.height);
        fill(frame, window, Color::rgba(0, 0, 0, 160), 0.0);
        frame.hit(Target::ConfirmBackdrop, window);
        let card_w = 420.0_f32.min(self.width - 24.0).max(0.0);
        let card = Rect::new(
            (self.width - card_w) / 2.0,
            (self.height - 140.0) / 2.0,
            card_w,
            140.0,
        );
        self.palette
            .push_surface(frame, card.x, card.y, card.w, card.h, 12.0, Surface::Card);
        let (title, repeats) = self.store.get(id).map_or_else(
            || (String::new(), false),
            |e| (e.title.clone(), e.recurrence != RecurrenceRule::None),
        );
        label(
            frame,
            card.x + 20.0,
            card.y + 20.0,
            format!("Delete \u{201C}{title}\u{201D}?"),
            15.0,
            self.palette.text,
            FontWeightHint::Bold,
            Some((card.w - 40.0).max(0.0)),
        );
        label(
            frame,
            card.x + 20.0,
            card.y + 48.0,
            if repeats {
                "Every time it repeats goes with it."
            } else {
                "It cannot be brought back."
            },
            12.0,
            self.palette.subtext0,
            FontWeightHint::Regular,
            Some((card.w - 40.0).max(0.0)),
        );
        let buttons_y = card.bottom() - 48.0;
        self.draw_button(
            frame,
            Rect::new(card.right() - 100.0, buttons_y, 80.0, 32.0),
            "Delete",
            Target::ConfirmDelete,
            true,
        );
        self.draw_button(
            frame,
            Rect::new(card.right() - 188.0, buttons_y, 80.0, 32.0),
            "Keep it",
            Target::KeepEvent,
            false,
        );
    }

    fn draw_top_bar(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.top_bar, self.palette.mantle, 0.0);

        draw_nav_button(frame, &self.palette, layout.nav_back, "<", Target::NavBack);
        draw_nav_button(
            frame,
            &self.palette,
            layout.nav_forward,
            ">",
            Target::NavForward,
        );

        let today = layout.today_button;
        fill(frame, today, self.palette.blue, 4.0);
        label(
            frame,
            today.x + 12.0,
            today.y + 8.0,
            "Today",
            12.0,
            self.palette.crust,
            FontWeightHint::Bold,
            Some(today.w - 8.0),
        );
        frame.hit(Target::TodayButton, today);

        if let Some(new) = layout.new_event_button {
            fill(frame, new, self.palette.surface0, 4.0);
            label(
                frame,
                new.x + 8.0,
                new.y + 8.0,
                "New event",
                12.0,
                self.palette.text,
                FontWeightHint::Regular,
                Some(new.w - 12.0),
            );
            frame.hit(Target::NewEvent, new);
        }

        if let Some(header) = layout.header {
            label(
                frame,
                header.x,
                header.y + 6.0,
                self.header_text(),
                16.0,
                self.palette.text,
                FontWeightHint::Bold,
                Some(header.w),
            );
        }

        if let Some(search) = layout.search {
            fill(frame, search, self.palette.surface0, 4.0);
            if self.search_focused {
                stroke(frame, search, self.palette.blue, 4.0, 1.5);
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
                if empty {
                    self.palette.overlay0
                } else {
                    self.palette.text
                },
                FontWeightHint::Regular,
                Some(search.w - 16.0),
            );
            frame.hit(Target::SearchField, search);
        }

        for (i, view) in CalendarView::all().iter().enumerate() {
            let tab = layout.view_tab(i);
            let active = *view == self.view;
            fill(
                frame,
                tab,
                if active {
                    self.palette.surface0
                } else {
                    self.palette.mantle
                },
                4.0,
            );
            label(
                frame,
                tab.x + 8.0,
                tab.y + 8.0,
                view.label(),
                11.0,
                if active {
                    self.palette.blue
                } else {
                    self.palette.subtext0
                },
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
        line(
            frame,
            0.0,
            sep,
            layout.window.w,
            sep,
            self.palette.surface0,
            1.0,
        );
    }

    fn draw_sidebar(&self, frame: &mut Frame, layout: &Layout, bar: Rect) {
        fill(frame, bar, self.palette.mantle, 0.0);

        if let Some(mini) = layout.mini_calendar() {
            self.draw_mini_calendar(frame, mini);
        }

        label(
            frame,
            bar.x + 12.0,
            bar.y + 210.0,
            "Categories",
            12.0,
            self.palette.subtext0,
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
                if active {
                    cat.color(&self.palette)
                } else {
                    self.palette.surface0
                },
                2.0,
            );
            label(
                frame,
                row.x + 22.0,
                row.y + 4.0,
                cat.label(),
                11.0,
                if active {
                    self.palette.text
                } else {
                    self.palette.overlay0
                },
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
                    self.palette.overlay0,
                    FontWeightHint::Regular,
                    Option::None,
                );
            }
            frame.hit(Target::CategoryFilter(i), row);
        }

        let edge = bar.right() - 0.5;
        line(
            frame,
            edge,
            bar.y,
            edge,
            bar.bottom(),
            self.palette.surface0,
            1.0,
        );
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
            self.palette.subtext0,
            FontWeightHint::Bold,
            Option::None,
        );
        label(
            frame,
            next.x + 5.0,
            area.y,
            ">",
            11.0,
            self.palette.subtext0,
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
            self.palette.text,
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
                self.palette.overlay0,
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
                    if is_today {
                        self.palette.blue
                    } else {
                        self.palette.surface0
                    },
                    3.0,
                );
            }

            let fg = if is_today {
                self.palette.crust
            } else if is_selected {
                self.palette.text
            } else if date.is_weekend() {
                self.palette.subtext0
            } else {
                self.palette.text
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
                    self.palette.peach,
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
                self.palette.subtext0,
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

            stroke(frame, cell, self.palette.surface0, 0.0, 0.5);

            if is_today || is_selected {
                fill(
                    frame,
                    Rect::new(cell.x + 4.0, cell.y + 2.0, 22.0, 18.0),
                    if is_today {
                        self.palette.blue
                    } else {
                        self.palette.surface1
                    },
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
                    self.palette.crust
                } else if date.is_weekend() {
                    self.palette.subtext0
                } else {
                    self.palette.text
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
                fill(frame, chip, ev.effective_color(&self.palette), 2.0);
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
                    self.palette.crust,
                    FontWeightHint::Bold,
                    Some((chip.w - 6.0).max(1.0)),
                );
                if self.selected_event_id == Some(ev.id) {
                    stroke(frame, chip, self.palette.text, 2.0, 1.5);
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
                    self.palette.overlay0,
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
            fill(
                frame,
                header,
                if is_today {
                    self.palette.surface0
                } else {
                    self.palette.mantle
                },
                0.0,
            );
            label(
                frame,
                header.x + 4.0,
                header.y + 4.0,
                date.day_of_week_short(),
                10.0,
                self.palette.subtext0,
                FontWeightHint::Regular,
                Some((day_w - 8.0).max(1.0)),
            );
            label(
                frame,
                header.x + 4.0,
                header.y + 18.0,
                date.day.to_string(),
                16.0,
                if is_today {
                    self.palette.blue
                } else {
                    self.palette.text
                },
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
                self.palette.overlay0,
                FontWeightHint::Regular,
                Some(WEEK_TIME_COL_W - 8.0),
            );
            line(
                frame,
                area.x + WEEK_TIME_COL_W,
                hy,
                area.right(),
                hy,
                self.palette.surface0,
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
                fill(frame, block, ev.effective_color(&self.palette), 3.0);
                label(
                    frame,
                    block.x + 3.0,
                    block.y + 2.0,
                    ev.title.clone(),
                    10.0,
                    self.palette.crust,
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
                        self.palette.crust,
                        FontWeightHint::Regular,
                        Some((block.w - 6.0).max(1.0)),
                    );
                }
                if self.selected_event_id == Some(ev.id) {
                    stroke(frame, block, self.palette.text, 3.0, 1.5);
                }
                frame.hit(Target::Event(ev.id), block);
            }
        }
    }

    fn draw_day_view(&self, frame: &mut Frame, area: Rect) {
        let is_today = self.view_date.is_today(self.today);
        let header = Rect::new(area.x, area.y, area.w, DAY_HEADER_H);
        fill(
            frame,
            header,
            if is_today {
                self.palette.surface0
            } else {
                self.palette.mantle
            },
            0.0,
        );
        label(
            frame,
            header.x + 16.0,
            header.y + 10.0,
            self.view_date.format_long(),
            14.0,
            if is_today {
                self.palette.blue
            } else {
                self.palette.text
            },
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
            fill(frame, block, ev.effective_color(&self.palette), 4.0);
            label(
                frame,
                block.x + 8.0,
                block.y + 5.0,
                format!("All day: {}", ev.title),
                11.0,
                self.palette.crust,
                FontWeightHint::Bold,
                Some((block.w - 16.0).max(1.0)),
            );
            if self.selected_event_id == Some(ev.id) {
                stroke(frame, block, self.palette.text, 4.0, 1.5);
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
                self.palette.overlay0,
                FontWeightHint::Regular,
                Some(DAY_TIME_COL_W - 8.0),
            );
            line(
                frame,
                area.x + DAY_TIME_COL_W,
                hy,
                area.right(),
                hy,
                self.palette.surface0,
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
            fill(frame, block, ev.effective_color(&self.palette), 4.0);
            label(
                frame,
                block.x + 6.0,
                block.y + 4.0,
                ev.title.clone(),
                12.0,
                self.palette.crust,
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
                    self.palette.crust,
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
                    self.palette.crust,
                    FontWeightHint::Regular,
                    Some((block.w - 16.0).max(1.0)),
                );
            }
            if self.selected_event_id == Some(ev.id) {
                stroke(frame, block, self.palette.text, 4.0, 1.5);
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
                if is_current_month {
                    self.palette.blue
                } else {
                    self.palette.text
                },
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
                        self.palette.blue,
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
                        self.palette.crust
                    } else if has_events {
                        self.palette.peach
                    } else if date.is_weekend() {
                        self.palette.overlay0
                    } else {
                        self.palette.subtext0
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
            self.palette.text,
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
                fill(
                    frame,
                    head,
                    if is_today {
                        self.palette.surface0
                    } else {
                        self.palette.mantle
                    },
                    4.0,
                );
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
                    if is_today {
                        self.palette.blue
                    } else {
                        self.palette.text
                    },
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
                ev.effective_color(&self.palette),
                2.0,
            );
            label(
                frame,
                card.x + 12.0,
                card.y + 2.0,
                ev.title.clone(),
                13.0,
                self.palette.text,
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
                self.palette.subtext0,
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
                    self.palette.overlay0,
                    FontWeightHint::Regular,
                    Some((card.w - 44.0).max(1.0)),
                );
            }
            if self.selected_event_id == Some(ev.id) {
                stroke(frame, card, self.palette.text, 4.0, 1.5);
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
                self.palette.overlay0,
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

fn draw_nav_button(frame: &mut Frame, pal: &Palette, rect: Rect, glyph: &str, target: Target) {
    fill(frame, rect, pal.surface0, 4.0);
    label(
        frame,
        rect.x + rect.w / 2.0 - 4.0,
        rect.y + rect.h / 2.0 - 6.0,
        glyph,
        14.0,
        pal.text,
        FontWeightHint::Bold,
        Option::None,
    );
    frame.hit(target, rect);
}

// ============================================================================
// Input
// ============================================================================

/// The one body both the window and the test probe drive the calendar
/// through -- and after every event, the events are kept if they changed.
pub fn handle_event(state: &mut CalendarApp, event: &Event) -> EventResult {
    let result = route_event(state, event);
    state.keep();
    result
}

/// Hand `event` to whatever has it: the close question, the picker, the event
/// form, the question before a delete, or the calendar.
fn route_event(state: &mut CalendarApp, event: &Event) -> EventResult {
    // The close question has every key and click while it is up: a key that
    // reached the calendar under it would be a change made while being asked
    // about the changes.
    if let Some(question) = state.question.as_mut()
        && matches!(event, Event::Key(_) | Event::Mouse(_))
    {
        if let Some(choice) = question.handle(event) {
            state.question = None;
            state.answer(choice);
        }
        return EventResult::Consumed;
    }
    // The picker takes input first while it is up, or a keystroke meant for
    // a filename lands in the search box behind it.
    //
    // A tick comes back as `Ignored` and falls through, which the hand
    // written version got wrong: it returned early for everything that was
    // not a key press or a click, so the midnight rollover below never ran
    // while a dialog was open. Leave a save dialog up across midnight and
    // "today" stayed on yesterday, in blue, in five places.
    match state.picker.handle(event, state.width, state.height) {
        Picked::Chose(path) => {
            let saving = state.picker.is_saving();
            state.last_file_action = Some(if saving {
                state.write_ics(&path)
            } else {
                state.read_ics(&path)
            });
            return EventResult::Consumed;
        }
        // Cancelled grouped with Handled: this caller keeps no dialog
        // state of its own that could go stale.
        Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
        Picked::Ignored => {}
    }
    // The form and the question before a delete are modal: every key and
    // every press is theirs while they are up.
    if state.form.is_some() {
        match event {
            Event::Key(key) if key.pressed => return handle_form_key(state, key),
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Press(_)) => {
                let hit = state.target_at(mouse.x, mouse.y);
                return handle_form_click(state, hit);
            }
            Event::Mouse(_) => return EventResult::Ignored,
            _ => {}
        }
    } else if let Some(id) = state.pending_delete {
        match event {
            Event::Key(key) if key.pressed => return handle_confirm_key(state, id, key),
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Press(_)) => {
                match state.target_at(mouse.x, mouse.y) {
                    Some(Target::ConfirmDelete) => state.delete_event(id),
                    Some(Target::KeepEvent) => state.pending_delete = None,
                    _ => {}
                }
                return EventResult::Consumed;
            }
            Event::Mouse(_) => return EventResult::Ignored,
            _ => {}
        }
    }
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
        // Closes at once unless the latest changes cannot be saved, which
        // closing would lose; then the question is up and the window stays.
        Event::CloseRequested => {
            if state.request_close() {
                state.running = false;
            }
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// Every key this program answers, and what it does.
///
/// Twenty-odd bindings and, until this list existed, no way to learn one but
/// reading the source. `W` is the worst of them: it decides which day a week
/// begins on, which is a question with no universally right answer, and it was
/// answered once at compile time until the key existed and then answerable
/// only by someone who had read the handler.
///
/// **Each row is a key this program actually answers**, which is not a
/// property the list has on its own: `every_advertised_key_does_something`
/// walks it, reads each label with `guitk::shortcut` and presses every key it
/// names. `apps/rssreader` shipped an overlay of twenty-one shortcuts of which
/// about four worked.
const SHORTCUTS: &[(&str, &str)] = &[
    ("1 / 2 / 3", "Month / week / day"),
    ("4 / 5", "Year / agenda"),
    ("Left / Right", "Back / forward one period"),
    ("PageUp / PageDown", "Back / forward one period"),
    ("Home", "Go to today"),
    ("Up / Down", "Scroll the day or week"),
    ("W", "Start the week on Monday or Sunday"),
    ("H", "Show times as 24-hour or 12-hour"),
    ("N", "New event on the selected day"),
    ("Enter", "Change the selected event"),
    ("Delete", "Delete the selected event (asks first)"),
    (
        "Tab / Shift+Tab",
        "In the event form: next / previous field",
    ),
    ("Escape", "Clear the selected event; close the form"),
    ("Ctrl+F", "Search"),
    ("Ctrl+B", "Show or hide the sidebar"),
    ("Ctrl+O / Ctrl+S", "Import / export a calendar file"),
    ("F1 / ?", "This list"),
];

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

impl CalendarApp {
    /// Put the open or save picker up.
    ///
    /// `generate_ics` and `parse_ics` were written, tested and unreachable:
    /// real iCalendar, with BEGIN:VCALENDAR, VERSION:2.0 and a PRODID. The
    /// format was the hard part and it was already finished.
    pub fn open_file_dialog(&mut self, saving: bool) {
        if saving {
            self.picker.open_to_write("calendar.ics");
        } else {
            self.picker.open_to_read();
        }
    }

    /// Write every event to `path` as iCalendar.
    ///
    /// Refuses on an empty calendar rather than writing a file with a header
    /// and no events. An `.ics` holding only BEGIN:VCALENDAR is valid, which
    /// is exactly the problem: it imports silently as nothing, and the user
    /// cannot tell it apart from an export that went wrong.
    pub fn write_ics(&mut self, path: &std::path::Path) -> String {
        if self.store.is_empty() {
            return String::from("No events to write");
        }
        let text = self.store.export_ics("SlateOS Calendar");
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!("Wrote {} event(s) to {}", self.store.len(), path.display()),
            Err(err) => format!("{FILE_FAILED_PREFIX} write {}: {err}", path.display()),
        }
    }

    /// Read `path` and add every event in it.
    ///
    /// Adds rather than replaces: importing a colleague's calendar should not
    /// discard your own.
    pub fn read_ics(&mut self, path: &std::path::Path) -> String {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => return format!("{FILE_FAILED_PREFIX} read {}: {err}", path.display()),
        };
        let whole = text.len();
        let truncated = whole > MAX_ICS_BYTES;
        let body = if truncated {
            let mut cut = MAX_ICS_BYTES;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut = cut.saturating_sub(1);
            }
            text.get(..cut).unwrap_or("").to_string()
        } else {
            text
        };
        let added = self.store.import_ics(&body);
        if truncated {
            format!(
                "INCOMPLETE: {added} event(s) from the first {MAX_ICS_BYTES} bytes of {}, which is {whole} bytes",
                path.display()
            )
        } else {
            format!("Added {added} event(s) from {}", path.display())
        }
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
            Key::S => {
                state.open_file_dialog(true);
                EventResult::Consumed
            }
            Key::O => {
                state.open_file_dialog(false);
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
        // Which day a week begins on. `week_starts_monday` was `true` at
        // construction and had no writer, so every month grid began on Monday
        // for everyone -- a question with no universally right answer, being
        // answered once at compile time.
        Key::W => {
            state.week_starts_monday = !state.week_starts_monday;
            EventResult::Consumed
        }
        // `use_24h` was `false` at construction with no writer anywhere, so
        // every time this program drew was 12-hour for everybody -- the same
        // shape as `week_starts_monday` above, which is why it gets the same
        // kind of answer. A bare letter, next to `W`, because both are
        // questions with no universally right answer that were being settled
        // once at compile time. Found by `scripts/frozen-flag-survey.py`.
        Key::H => {
            state.use_24h = !state.use_24h;
            EventResult::Consumed
        }
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
        // `?`, which is Shift and the slash key. The search branch above
        // returns first when the box has focus, so this cannot swallow a `?`
        // somebody is typing into a query.
        // The shortcut list. `F1` raises it in every app in this tree,
        // including `apps/spreadsheet`, where `?` is a character the
        // program has to be able to type into a cell -- so somebody who
        // has learned one key is never stuck. `?` as well, wherever the
        // program is not obliged to type one.
        Key::F1 => {
            state.show_help = !state.show_help;
            EventResult::Consumed
        }
        Key::Slash if key.modifiers.shift => {
            state.show_help = !state.show_help;
            EventResult::Consumed
        }
        // Before the plain `Escape` arm below, which would otherwise take this
        // and clear the selection while the list stayed up.
        Key::Escape if state.show_help => {
            state.show_help = false;
            EventResult::Consumed
        }
        Key::Escape => {
            state.selected_event_id = Option::None;
            EventResult::Consumed
        }
        Key::N => {
            state.open_new_event();
            EventResult::Consumed
        }
        // The selected event, if it is still there: a search or a filter can
        // take it off the screen, but not out of the calendar.
        Key::Enter => match state.selected_event_id {
            Some(id) if state.store.get(id).is_some() => {
                state.open_edit_event(id);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        },
        Key::Delete => match state.selected_event_id {
            Some(id) if state.store.get(id).is_some() => {
                state.pending_delete = Some(id);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        },
        _ => EventResult::Ignored,
    }
}

/// Keys while the event form is up: Tab walks the fields, Enter saves,
/// Escape leaves, Left, Right and Space step a chosen field, and the rest
/// edit the text field that has the keys.
fn handle_form_key(state: &mut CalendarApp, key: &KeyEvent) -> EventResult {
    let Some(form) = state.form.as_ref() else {
        return EventResult::Ignored;
    };
    let fields = form.fields();
    if !fields.contains(&state.form_field) {
        state.form_field = fields.first().copied().unwrap_or(FormField::Title);
    }
    match key.key {
        Key::Tab => {
            let at = fields
                .iter()
                .position(|f| *f == state.form_field)
                .unwrap_or(0);
            let next = step_index(at, fields.len(), !key.modifiers.shift);
            state.form_field = fields.get(next).copied().unwrap_or(state.form_field);
            EventResult::Consumed
        }
        Key::Enter => {
            state.save_form();
            EventResult::Consumed
        }
        Key::Escape => {
            state.cancel_form();
            EventResult::Consumed
        }
        Key::Left | Key::Right | Key::Space if !state.form_field.is_text() => {
            let (field, forward) = (state.form_field, key.key != Key::Left);
            if let Some(form) = state.form.as_mut()
                && form.step(field, forward)
            {
                state.form_error = None;
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        _ => {
            let field = state.form_field;
            let clipboard = state.clipboard.clone();
            let Some(input) = state.form.as_mut().and_then(|f| f.input(field)) else {
                return EventResult::Ignored;
            };
            let done =
                textline::apply_key(input, key, field.capacity(), &clipboard, FORM_TEXT_SIZE);
            if let Some(copied) = done.copied {
                state.clipboard = copied;
            }
            if done.handled {
                state.form_error = None;
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
    }
}

/// A press while the event form is up. Anywhere but its controls it does
/// nothing: the form is modal, and a press reaching the calendar behind it
/// would change what it is about.
fn handle_form_click(state: &mut CalendarApp, hit: Option<Target>) -> EventResult {
    match hit {
        Some(Target::Field(field)) => {
            state.form_field = field;
            // A press on a chosen value steps it on, as the arrow after it does.
            if !field.is_text()
                && let Some(form) = state.form.as_mut()
            {
                form.step(field, true);
            }
        }
        Some(Target::StepBack(field) | Target::StepForward(field)) => {
            state.form_field = field;
            let forward = matches!(hit, Some(Target::StepForward(_)));
            if let Some(form) = state.form.as_mut() {
                form.step(field, forward);
            }
        }
        Some(Target::Save) => state.save_form(),
        Some(Target::Cancel) => state.cancel_form(),
        Some(Target::DeleteEvent) => {
            if let Some(id) = state.form.as_ref().and_then(|f| f.id) {
                state.cancel_form();
                state.pending_delete = Some(id);
            }
        }
        _ => {}
    }
    EventResult::Consumed
}

/// Keys while "Delete this event?" is up: Enter or Y deletes it, Escape or N
/// keeps it; every other key is swallowed, since a key that reached the
/// calendar would be acted on under a question it has not answered.
fn handle_confirm_key(state: &mut CalendarApp, id: u64, key: &KeyEvent) -> EventResult {
    match key.key {
        Key::Enter | Key::Y => state.delete_event(id),
        Key::Escape | Key::N => state.pending_delete = None,
        _ => {}
    }
    EventResult::Consumed
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
                // A press on the event already selected opens it.
                Some(Target::Event(id)) => {
                    if state.selected_event_id == Some(id) {
                        state.open_edit_event(id);
                    } else {
                        state.selected_event_id = Some(id);
                    }
                }
                Some(Target::NewEvent) => state.open_new_event(),
                // Consumed either way: the click landed on this window. The
                // form's and the delete question's own targets are drawn only
                // while they are up, and are handled before this.
                Some(
                    Target::SearchField
                    | Target::Field(_)
                    | Target::StepBack(_)
                    | Target::StepForward(_)
                    | Target::Save
                    | Target::Cancel
                    | Target::DeleteEvent
                    | Target::FormBackdrop
                    | Target::ConfirmDelete
                    | Target::KeepEvent
                    | Target::ConfirmBackdrop,
                )
                | Option::None => {}
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
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

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

    /// Closing asks first only when the latest changes cannot be saved; the
    /// window then waits for the answer (`KeepOpen`) with the question drawn.
    fn on_event(&mut self, event: &Event) -> Response {
        let result = handle_event(self, event);
        if !self.running {
            return Response::Exit;
        }
        if matches!(event, Event::CloseRequested) {
            return Response::KeepOpen;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        let mut tree = self.frame(width, height).into_tree();
        // Over everything, the picker included: they are never up together.
        let palette = self.palette;
        if let Some(question) = self.question.as_mut() {
            question.render(&palette, width, height, &mut tree);
        }
        tree
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

/// What the window says while it has no events: how to add one.
///
/// It said two other things until 2026-09-26: that the calendar had opened on
/// five invented events until 2026-09-15, which had stopped being news, and
/// that nothing was saved automatically, which stopped being so when the
/// events came to be kept as they change (design-decisions §1209). A banner
/// that denies a capability the program has is the same defect as one that
/// claims a capability it lacks -- and the more expensive direction, since a
/// false denial stops the user trying at all.
const NO_EVENTS_LINE: &str =
    "No events yet -- press N or New event to add one, or Ctrl+O to import an .ics calendar.";

/// A day's worth of events, for tests.
///
/// `#[cfg(test)]` since 2026-09-15. `main` called it, so the window opened on
/// a "Team Standup" at 09:00 **today** and four more like it. An event on a
/// dated day is a claim about what the user has scheduled -- the same shape as
/// `apps/reminders`' overdue task and `apps/habits`' check-ins, and acted on
/// the same way: somebody glances at a calendar to find out whether they are
/// free.
#[cfg(test)]
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
    // The events kept last time. It used to open on `sample_events`, and
    // then empty, and kept nothing.
    let mut app = CalendarApp::from_settings(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
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
    /// The picker is not merely open: it is DRAWN.
    ///
    /// `is_open()` returning true is not the same claim, and assuming it was
    /// is how `apps/flashcards` shipped a dialog that took every keystroke and
    /// painted nothing. Deleting the `picker.render` line in the renderer
    /// leaves `is_open()` true and every other test green; this is the one
    /// that notices.
    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The open test asserts that the KEY HANDLER opened the dialog, which
    /// holds whether or not the picker is ever handed another event. This is
    /// the half routing decides: with a dialog up, a keystroke belongs to the
    /// dialog.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_grid() {
        let today = Date {
            year: 2026,
            month: 9,
            day: 8,
        };
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
        // The week view, because Down scrolls an hour grid and `clamp_scroll`
        // pins the offset back to zero in the month view -- so in the default
        // view the key moves nothing and the assertion below would hold
        // whether or not the dialog took it. Measured, not guessed: the
        // diagnostic printed `view=Month scroll=0` under the routing cut.
        app.view = CalendarView::Week;
        app.content_scroll = 0.0;

        let mut ctrl = guitk::event::Modifiers::NONE;
        ctrl.ctrl = true;
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::O,
                pressed: true,
                modifiers: ctrl,
                text: String::new(),
            }),
        );
        assert!(app.picker.is_open(), "control: the picker must be up");

        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Down,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: String::new(),
            }),
        );
        assert_eq!(
            app.content_scroll, 0.0,
            "Down at the open dialog scrolled the grid behind it"
        );
    }

    #[test]
    fn the_picker_is_drawn_when_it_is_open() {
        let today = Date {
            year: 2026,
            month: 9,
            day: 15,
        };
        let mut app = CalendarApp::new(1024.0, 768.0, today);
        let before = app.frame(1024.0, 768.0).into_tree().commands.len();
        app.open_file_dialog(true);
        assert!(app.picker.is_open(), "no picker came up");
        let after = app.frame(1024.0, 768.0).into_tree().commands.len();
        let own = app.picker.render(&app.palette, 1024.0, 768.0).len();
        assert!(
            own > 0,
            "the picker itself draws nothing, so this proves nothing"
        );
        assert!(
            after >= before + own,
            "the frame does not contain the picker's own {own} command(s) ({before} before, {after} after) -- something else grew instead"
        );
    }

    /// A calendar survives a write and a read.
    ///
    /// `generate_ics` and `parse_ics` were written, tested and unreachable:
    /// real iCalendar, BEGIN:VCALENDAR, VERSION:2.0, a PRODID. The format was
    /// the hard part and it was finished -- found by
    /// `scripts/find-stranded-serialisers.py`, which reports 92 such
    /// functions across 37 crates.
    /// Midnight still arrives while the picker is open.
    ///
    /// The hand-written intercept this replaced returned early for every
    /// event that was not a key press or a click, so `Event::Tick` never
    /// reached the rollover. Leave a save dialog up across midnight and
    /// **"today" stayed on yesterday, in blue, in five different places** --
    /// until something else happened to cause a repaint.
    ///
    /// `FilePicker::handle` returns `Ignored` for a tick precisely so it falls
    /// through to the application.
    #[test]
    fn midnight_still_arrives_while_the_picker_is_open() {
        // A day the real clock cannot be on, so any rollover is visible.
        let long_ago = Date {
            year: 2000,
            month: 1,
            day: 1,
        };
        let mut app = CalendarApp::new(1024.0, 768.0, long_ago);

        app.open_file_dialog(true);
        assert!(app.picker.is_open(), "no picker to test behind");

        handle_event(&mut app, &Event::Tick { elapsed_ms: 60_000 });
        assert_ne!(
            app.today, long_ago,
            "the date stopped advancing because a dialog was open"
        );
        assert!(app.picker.is_open(), "the tick closed the dialog");
    }

    #[test]
    fn a_calendar_survives_a_write_and_a_read() {
        let dir = std::env::temp_dir().join("slateos-calendar-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("cal.ics");

        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
        app.store.add(CalendarEvent {
            id: 0,
            title: "Dentist".to_string(),
            description: String::new(),
            category: EventCategory::Personal,
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
                    minute: 0,
                },
            ),
            all_day: false,
            recurrence: RecurrenceRule::None,
            reminder: Reminder::None,
            location: None,
            color_override: None,
        });

        let said = app.write_ics(&path);
        assert!(said.starts_with("Wrote 1 event"), "{said}");

        let raw = std::fs::read_to_string(&path).expect("written");
        assert!(raw.starts_with("BEGIN:VCALENDAR"), "not iCalendar: {raw:?}");

        let mut reopened = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
        let said = reopened.read_ics(&path);
        assert!(said.starts_with("Added 1 event"), "{said}");
        assert_eq!(reopened.store.len(), 1);

        std::fs::remove_file(&path).ok();
    }

    /// An empty calendar refuses rather than writing a header with no events.
    ///
    /// An .ics holding only BEGIN:VCALENDAR is valid, which is exactly the
    /// problem: it imports silently as nothing, and the user cannot tell it
    /// apart from an export that went wrong.
    #[test]
    fn an_empty_calendar_refuses_to_write() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
        assert!(app.store.is_empty());
        let path = std::env::temp_dir().join("slateos-calendar-should-not-exist.ics");
        std::fs::remove_file(&path).ok();

        let said = app.write_ics(&path);
        assert!(said.contains("No events to write"), "{said}");
        assert!(!path.exists(), "a header-only calendar was written anyway");
    }

    /// A fresh calendar holds no events, and says the emptiness is not yours.
    ///
    /// `main` called `sample_events`, so the window opened on a "Team
    /// Standup" at 09:00 **today** and four more. An event on a dated day is a
    /// claim about what the user has scheduled -- the same shape as
    /// `apps/reminders`' overdue task and `apps/habits`' check-ins, and acted
    /// on the same way: somebody glances at a calendar to find out whether
    /// they are free.
    ///
    /// Two absences, two lines. Nothing here is the user's, and nothing the
    /// user adds will survive the window.
    #[test]
    fn a_fresh_calendar_holds_no_events_and_says_how_to_add_one() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);
        assert!(app.store.is_empty(), "events appeared from nowhere");

        let texts: Vec<String> = app
            .frame(DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|t| t == NO_EVENTS_LINE),
            "the window never said {NO_EVENTS_LINE:?}"
        );
        // The remedy, which is the point of the line: the keys that add one.
        assert!(NO_EVENTS_LINE.contains("press N"));
        assert!(NO_EVENTS_LINE.contains("Ctrl+O"));
        // And no claim that nothing is kept, which is no longer so.
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("gone when the window closes"))
        );
    }

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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        for cat in EventCategory::all() {
            let _ = cat.color(&pal);
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
    fn the_new_event_button_gives_way_to_the_view_tabs() {
        let wide = Layout::new(1280.0, 720.0, true);
        let button = wide.new_event_button.expect("no New event button at 1280");
        assert!(
            button.right() < wide.view_tab(0).x,
            "the button runs into the tabs"
        );
        if let Some(header) = wide.header {
            assert!(
                header.x >= button.right(),
                "the caption is drawn over the button"
            );
        }
        let narrow = Layout::new(320.0, 720.0, true);
        assert!(
            narrow.new_event_button.is_none(),
            "the button crowds the tabs at 320"
        );
        // Where it is drawn, it is pressed.
        let mut app = CalendarApp::new(1280.0, 720.0, Date::new(2026, 9, 26).unwrap());
        probe::click(&mut app, Target::NewEvent);
        assert!(app.form.is_some());
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

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// A list on screen and the handler behind it are two copies of one fact,
    /// and they drift: `apps/rssreader` shipped an overlay of twenty-one
    /// shortcuts of which about four worked. The label is read by
    /// `guitk::shortcut` rather than matched against a table written beside it
    /// here -- that table would be a third copy, drifting from both.
    ///
    /// A fresh calendar per keystroke, because `Ctrl+O` and `Ctrl+S` each put
    /// a file picker up that would take the next key.
    #[test]
    fn every_advertised_key_does_something() {
        // The states between which every key has work: the calendar as it
        // opens, with an event selected (Enter, Delete), and with the event
        // form up (Tab). "Some reachable state answers it", not "it is taken
        // now": Enter with nothing selected declines on purpose.
        let states = || {
            let plain = sample_app(june_2024());
            let mut selected = sample_app(june_2024());
            selected.selected_event_id = selected.store.all().first().map(|e| e.id);
            let mut form = sample_app(june_2024());
            form.open_new_event();
            [plain, selected, form]
        };
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = states().iter_mut().any(|app| {
                    handle_event(app, &Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The shortcut list reaches the window.**
    ///
    /// The guard above reads the list against the handler; this reads it
    /// against the screen. `apps/netscan`'s `wol_note` was written by the
    /// model and drawn by nothing for three commits with every model-level
    /// test passing.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = sample_app(june_2024());
        assert!(
            !help_text(&app).contains("? closes this"),
            "the list is up before anybody asked for it"
        );

        let ask = guitk::shortcut::keystrokes("?").unwrap_or_else(|e| panic!("{e}"));
        for stroke in &ask {
            handle_event(&mut app, &Event::Key(stroke.clone()));
        }

        let shown = help_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Escape,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: String::new(),
            }),
        );
        assert!(
            !help_text(&app).contains("? closes this"),
            "Escape did not close it"
        );
    }

    /// **The clock can be put into 24-hour time.**
    ///
    /// `use_24h` was `false` at construction and written nowhere, so every
    /// time this program drew was 12-hour for every user in every locale. The
    /// guard test only asks whether `H` was consumed; this asks whether the
    /// clock moved, and that the neighbouring preference did not come with it.
    #[test]
    fn the_hour_format_can_be_changed() {
        let mut app = sample_app(june_2024());
        let before = (app.use_24h, app.week_starts_monday);

        probe::key(&mut app, &probe::press(Key::H));

        assert_ne!(app.use_24h, before.0, "H did not change the hour format");
        assert_eq!(
            app.week_starts_monday, before.1,
            "H moved the week-start preference as well"
        );
    }

    /// A `?` typed into the search box stays a `?`.
    #[test]
    fn a_question_mark_in_the_search_box_is_not_the_help_key() {
        let mut app = sample_app(june_2024());
        app.search_focused = true;
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Slash,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: String::from("?"),
            }),
        );
        assert!(
            !help_text(&app).contains("? closes this"),
            "the help key was taken out of somebody's search query"
        );
        assert!(app.search_query.contains('?'), "the `?` was dropped");
    }

    /// Every string the window is drawing, joined.
    fn help_text(app: &CalendarApp) -> String {
        app.frame(DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .into_tree()
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
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

    /// `W` changes which day a week begins on.
    ///
    /// `week_starts_monday` was `true` at construction and had no writer, so
    /// every month grid began on Monday for everyone -- a question with no
    /// universally right answer, answered once at compile time.
    #[test]
    fn w_changes_the_day_a_week_starts_on() {
        let mut app = sample_app(june_2024());
        app.view = CalendarView::Month;
        assert!(app.week_starts_monday, "control: it starts on Monday");

        probe::key(&mut app, &probe::press(Key::W));

        assert!(!app.week_starts_monday, "W did not change the week start");
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

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut CalendarApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let today = Date {
            year: 2026,
            month: 9,
            day: 8,
        };
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, today);

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    /// The warning lines are where they can be seen: nothing drawn after a
    /// line fills the point it is drawn at. The sweep that added them drew
    /// them "after the background, or it would be painted over" -- and in
    /// several apps a bar was then drawn over the same pixels, while a test
    /// that read the frame's texts said they were there. known-issues.md,
    /// `[E] Warnings drawn where the next thing drawn covers them`.
    #[test]
    fn the_warning_lines_are_not_painted_over() {
        let mut app = CalendarApp::new(
            DEFAULT_WIDTH,
            DEFAULT_HEIGHT,
            Date {
                year: 2026,
                month: 5,
                day: 18,
            },
        );
        // Every line the strip can hold at once: a store that is not kept, a
        // file action, and the empty calendar's line. The file action was
        // drawn across the top bar's buttons.
        app.store_error = Some(String::from("Your calendar was not saved to /x: no room"));
        app.last_file_action = Some(String::from("Could not read /y: gone"));
        let lines: Vec<String> = app.notice_lines().into_iter().map(|(l, _)| l).collect();
        assert_eq!(lines.len(), 3, "control: {lines:?}");
        let commands: Vec<RenderCommand> =
            app.frame(DEFAULT_WIDTH, DEFAULT_HEIGHT).commands().to_vec();
        for line in &lines {
            let (at, x, y, reach) = commands
                .iter()
                .enumerate()
                .find_map(|(i, c)| match c {
                    RenderCommand::Text {
                        text,
                        x,
                        y,
                        max_width,
                        ..
                    } if text == line => Some((i, *x, *y, x + max_width.unwrap_or(f32::INFINITY))),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{line:?} is not drawn"));
            let covered = commands.iter().skip(at + 1).any(|c| {
                matches!(c, RenderCommand::FillRect { x: rx, y: ry, width, height, .. }
                    if x >= *rx && x < rx + width && y >= *ry && y < ry + height)
            });
            assert!(!covered, "{line:?} is painted over");
            // Nor drawn on the same row as other text: a header's title over
            // a warning is as unreadable as a fill over it.
            let crowded = commands.iter().any(|c| {
                matches!(c, RenderCommand::Text { text, x: tx, y: ty, max_width: tw, .. }
                    if !lines.contains(text)
                        && (ty - y).abs() < 10.0
                        && *tx < reach
                        && tx + tw.unwrap_or(f32::INFINITY) > x)
            });
            assert!(!crowded, "{line:?} shares its row with other text");
        }
    }

    // ------------------------------------------------------------------
    // Events added, changed and deleted in the window, and kept
    // ------------------------------------------------------------------

    /// Type `text`, one character at a time.
    fn type_in(app: &mut CalendarApp, text: &str) {
        probe::type_str(app, text);
    }

    /// Put `text` in the form's field `field`, over what it held.
    fn fill_field(app: &mut CalendarApp, field: FormField, text: &str) {
        app.form_field = field;
        probe::key(app, &probe::ctrl(Key::A));
        probe::key(app, &probe::press(Key::Backspace));
        type_in(app, text);
    }

    /// Every string the frame draws, joined.
    fn drawn(app: &CalendarApp) -> String {
        render(app)
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// A day in the week of 2026-09-26, the day these tests were written.
    fn a_saturday() -> Date {
        Date::new(2026, 9, 26).unwrap()
    }

    /// An event of every kind the file has to hold: text that needs escaping
    /// in every free field, a colour, a repeat on named days, a reminder.
    fn awkward_event(title: &str) -> CalendarEvent {
        CalendarEvent {
            id: 0,
            title: title.to_owned(),
            description: String::from("two\nlines\tand a tab \\ and a backslash"),
            category: EventCategory::Birthday,
            start: DateTime::new(a_saturday(), Time::new(7, 5).unwrap()),
            end: DateTime::new(a_saturday(), Time::new(23, 59).unwrap()),
            all_day: false,
            recurrence: RecurrenceRule::Weekly {
                days: vec![1, 3, 5],
            },
            reminder: Reminder::MinutesBefore(45),
            location: Some(String::from("Room\t4")),
            color_override: Some(Color::rgba(1, 2, 3, 200)),
        }
    }

    /// The fields of an event a save has to keep, id included.
    fn describe(e: &CalendarEvent) -> String {
        format!(
            "{} {:?} {:?} {:?} {:?} {:?} {} {:?} {:?} {:?} {:?}",
            e.id,
            e.title,
            e.description,
            e.category,
            e.start,
            e.end,
            e.all_day,
            e.recurrence,
            e.reminder,
            e.location,
            e.color_override
        )
    }

    #[test]
    fn the_kept_calendar_reads_back_what_it_wrote_whatever_the_text() {
        let mut store = EventStore::new();
        store.add(awkward_event(
            "Tab\there, and a line\nbreak, and \\t written out",
        ));
        for (i, (category, rule)) in EventCategory::all()
            .iter()
            .zip([
                RecurrenceRule::None,
                RecurrenceRule::Daily,
                RecurrenceRule::Weekly { days: Vec::new() },
                RecurrenceRule::BiWeekly,
                RecurrenceRule::Monthly,
                RecurrenceRule::Yearly,
                RecurrenceRule::Custom { interval_days: 3 },
                RecurrenceRule::Custom { interval_days: 0 },
                RecurrenceRule::Weekly { days: vec![0, 6] },
                RecurrenceRule::None,
            ])
            .enumerate()
        {
            let mut e = awkward_event(&format!("Event {i}"));
            e.category = *category;
            e.recurrence = rule;
            e.all_day = i % 2 == 0;
            e.location = (i % 3 == 0).then(|| String::from("a place"));
            e.color_override = None;
            e.reminder = [
                Reminder::None,
                Reminder::AtTime,
                Reminder::HoursBefore(2),
                Reminder::DayBefore,
            ][i % 4];
            store.add(e);
        }
        let text = calendar_text(&store);
        let back = parse_calendar(&text).expect("it reads back");
        let want: Vec<String> = store.all().iter().map(describe).collect();
        let got: Vec<String> = back.iter().map(describe).collect();
        assert_eq!(got, want);
        // And the same text again: one calendar, one spelling.
        assert_eq!(calendar_text(&EventStore::from_events(back)), text);
    }

    #[test]
    fn a_calendar_that_cannot_be_read_whole_is_refused_and_says_why() {
        let mut store = EventStore::new();
        store.add(awkward_event("One"));
        store.add(awkward_event("Two"));
        let good = calendar_text(&store);
        let cases: [(&str, String, &str); 8] = [
            (
                "later",
                good.replacen("slateos-calendar\t1", "slateos-calendar\t2", 1),
                "later format (2)",
            ),
            (
                "not ours",
                String::from("BEGIN:VCALENDAR\n"),
                "not a SlateOS calendar",
            ),
            (
                "short",
                good.replacen("\tRoom", "", 1),
                "13 fields where an event has 14",
            ),
            (
                "twins",
                good.replacen("event\t2\t", "event\t1\t", 1),
                "another event has its number (1)",
            ),
            (
                "date",
                good.replacen("2026-09-26", "2026-02-30", 1),
                "line 2: its start date is not a date",
            ),
            (
                "time",
                good.replacen("07:05", "7:5", 1),
                "its start time is not a time",
            ),
            (
                "category",
                good.replacen("\tbirthday\t", "\tparty\t", 1),
                "its category (party)",
            ),
            (
                "escape",
                good.replacen("Room\\t4", "Room\\q4", 1),
                "its place has a broken escape",
            ),
        ];
        for (name, text, why) in cases {
            assert_ne!(text, good, "control: case {name} changed nothing");
            let said = parse_calendar(&text).map(|_| ()).unwrap_err();
            assert!(said.contains(why), "{name}: {said}");
        }
        for (key, what) in [
            ("weekly:1,3,5", "weekly:1,9"),
            ("minutes:45", "minutes:x"),
            ("#010203C8", "#0102"),
        ] {
            let text = good.replacen(key, what, 1);
            assert_ne!(text, good, "control: {key} is in the file");
            assert!(parse_calendar(&text).is_err(), "{what} was read");
        }
    }

    #[test]
    fn an_event_is_added_in_the_form_and_is_there_next_time() {
        settingsfile::testing::with_scratch_config("calendar-kept", |_| {
            let mut app = CalendarApp::from_settings(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            assert!(app.store_error.is_none(), "{:?}", app.store_error);
            // An event that changes nothing writes nothing.
            probe::key(&mut app, &probe::press(Key::Right));
            assert!(
                !events_path().unwrap().exists(),
                "a first run wrote a calendar nobody had touched"
            );
            app.select_date(a_saturday());
            probe::key(&mut app, &probe::press(Key::N));
            assert!(app.form.is_some(), "N did not open the form");
            type_in(&mut app, "Dentist");
            fill_field(&mut app, FormField::Starts, "14:30");
            fill_field(&mut app, FormField::Ends, "15:15");
            fill_field(&mut app, FormField::Place, "High St");
            app.form_field = FormField::Category;
            probe::key(&mut app, &probe::press(Key::Right));
            probe::key(&mut app, &probe::press(Key::Enter));
            assert!(app.form.is_none(), "{:?}", app.form_error);
            assert_eq!(app.store.len(), 1, "no event was made");
            let made = app.store.all()[0].clone();
            assert_eq!(made.title, "Dentist");
            assert_eq!(
                made.start,
                DateTime::new(a_saturday(), Time::new(14, 30).unwrap())
            );
            assert_eq!(made.end.time, Time::new(15, 15).unwrap());
            assert_eq!(made.location.as_deref(), Some("High St"));
            assert_eq!(
                made.category,
                EventCategory::Health,
                "the category did not step"
            );
            assert_eq!(
                app.selected_event_id,
                Some(made.id),
                "the new event is not selected"
            );
            assert!(
                drawn(&app).contains("Dentist"),
                "the new event is not drawn"
            );

            let again = CalendarApp::from_settings(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            assert!(again.store_error.is_none(), "{:?}", again.store_error);
            let kept: Vec<String> = again.store.all().iter().map(describe).collect();
            assert_eq!(kept, vec![describe(&made)]);

            // An event added after the restart takes no kept event's number.
            let mut again = again;
            let id = again.store.add(awkward_event("Next"));
            assert_ne!(id, made.id);
        });
    }

    #[test]
    fn a_window_made_by_new_keeps_nothing() {
        settingsfile::testing::with_scratch_config("calendar-quiet", |dir| {
            let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            probe::key(&mut app, &probe::press(Key::N));
            type_in(&mut app, "Scratch");
            probe::key(&mut app, &probe::press(Key::Enter));
            assert_eq!(app.store.len(), 1);
            assert!(!dir.join("slateos").join("calendar").exists());
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::Exit
            ));
        });
    }

    #[test]
    fn the_form_says_what_is_wrong_and_keeps_what_was_typed() {
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
        probe::key(&mut app, &probe::press(Key::N));
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(app.form_error.as_deref(), Some("Give it a title"));
        assert!(app.form.is_some(), "the form went away with nothing kept");
        type_in(&mut app, "Lunch");
        assert!(
            app.form_error.is_none(),
            "typing did not clear the complaint"
        );
        for (field, text, said) in [
            (FormField::Date, "2026-02-30", "The date is not one"),
            (FormField::Starts, "25:00", "The start is not a time"),
            (FormField::Ends, "noon", "The end is not a time"),
        ] {
            fill_field(&mut app, field, text);
            probe::key(&mut app, &probe::press(Key::Enter));
            assert!(
                app.form_error
                    .as_deref()
                    .is_some_and(|e| e.starts_with(said)),
                "{text}: {:?}",
                app.form_error
            );
            assert!(drawn(&app).contains(said), "the complaint is not drawn");
            fill_field(
                &mut app,
                field,
                match field {
                    FormField::Date => "2026-09-26",
                    FormField::Starts => "12:00",
                    _ => "13:00",
                },
            );
        }
        fill_field(&mut app, FormField::Ends, "11:00");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(app.form_error.as_deref(), Some("It ends before it starts"));
        assert_eq!(app.store.len(), 0, "a wrong form was kept");
        // Escape leaves, keeping nothing.
        probe::key(&mut app, &probe::press(Key::Escape));
        assert!(app.form.is_none());
        assert_eq!(app.store.len(), 0);
    }

    #[test]
    fn an_all_day_event_has_no_times_to_fill_in() {
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
        probe::key(&mut app, &probe::press(Key::N));
        type_in(&mut app, "Holiday");
        fill_field(&mut app, FormField::Starts, "garbage");
        app.form_field = FormField::AllDay;
        probe::key(&mut app, &probe::press(Key::Space));
        let fields = app.form.as_ref().unwrap().fields();
        assert!(!fields.contains(&FormField::Starts) && !fields.contains(&FormField::Ends));
        // Tab from the All day row goes past the hidden times.
        probe::key(&mut app, &probe::press(Key::Tab));
        assert_eq!(app.form_field, FormField::Category);
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(app.form.is_none(), "{:?}", app.form_error);
        let made = &app.store.all()[0];
        assert!(made.all_day);
        assert_eq!(made.start.time, Time::new(0, 0).unwrap());
        assert_eq!(made.end.time, Time::new(23, 59).unwrap());
    }

    #[test]
    fn enter_changes_the_selected_event_and_keeps_its_number() {
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
        let id = app.store.add(awkward_event("Old name"));
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Enter)),
            EventResult::Ignored,
            "Enter with nothing selected did something"
        );
        app.selected_event_id = Some(id);
        probe::key(&mut app, &probe::press(Key::Enter));
        let form = app.form.as_ref().expect("Enter did not open the event");
        assert_eq!(form.id, Some(id));
        fill_field(&mut app, FormField::Title, "New name");
        fill_field(&mut app, FormField::Date, "2026-10-01");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(app.form.is_none(), "{:?}", app.form_error);
        assert_eq!(app.store.len(), 1, "a change made a second event");
        let e = app.store.get(id).expect("the event lost its number");
        assert_eq!(e.title, "New name");
        assert_eq!(e.start.date, Date::new(2026, 10, 1).unwrap());
        // What the form does not show is kept as it was.
        assert_eq!(e.reminder, Reminder::MinutesBefore(45));
        assert_eq!(e.color_override, Some(Color::rgba(1, 2, 3, 200)));
        assert_eq!(
            e.recurrence,
            RecurrenceRule::Weekly {
                days: vec![1, 3, 5]
            }
        );
        assert_eq!(
            app.selected_date,
            Date::new(2026, 10, 1).unwrap(),
            "the view did not follow"
        );
    }

    #[test]
    fn repeats_step_through_the_list_and_keep_an_imported_one() {
        let mut e = awkward_event("Pills");
        e.recurrence = RecurrenceRule::Custom { interval_days: 3 };
        let mut form = EventForm::editing(&e);
        assert_eq!(form.choice_label(FormField::Repeats), "Every 3 days");
        let mut seen = vec![form.choice_label(FormField::Repeats)];
        for _ in 0..7 {
            assert!(form.step(FormField::Repeats, true));
            seen.push(form.choice_label(FormField::Repeats));
        }
        assert_eq!(
            seen,
            [
                "Every 3 days",
                "Does not repeat",
                "Daily",
                "Weekly, on the day it starts",
                "Every 2 weeks",
                "Monthly",
                "Yearly",
                "Every 3 days",
            ]
        );
        form.step(FormField::Repeats, false);
        assert_eq!(form.choice_label(FormField::Repeats), "Yearly");
        // A new event's list has no seventh entry.
        let mut fresh = EventForm::new_on(a_saturday());
        fresh.step(FormField::Repeats, false);
        assert_eq!(fresh.choice_label(FormField::Repeats), "Yearly");
    }

    #[test]
    fn delete_asks_first_and_each_answer_does_what_it_says() {
        let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
        let id = app.store.add(awkward_event("Doomed"));
        app.selected_event_id = Some(id);
        probe::key(&mut app, &probe::press(Key::Delete));
        assert_eq!(app.pending_delete, Some(id));
        assert!(
            drawn(&app).contains("Delete \u{201C}Doomed\u{201D}?"),
            "the question is not drawn"
        );
        // A key under the question reaches nothing.
        probe::key(&mut app, &probe::press(Key::Num2));
        assert_eq!(app.view, CalendarView::Month, "a key reached the calendar");
        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(app.pending_delete, None);
        assert_eq!(app.store.len(), 1, "Escape deleted it");
        // Keep it, by the button.
        probe::key(&mut app, &probe::press(Key::Delete));
        probe::click(&mut app, Target::KeepEvent);
        assert_eq!(app.store.len(), 1);
        // Delete, by the button.
        probe::key(&mut app, &probe::press(Key::Delete));
        probe::click(&mut app, Target::ConfirmDelete);
        assert!(app.store.is_empty(), "Delete did not delete");
        assert_eq!(app.selected_event_id, None);
        // From the form, too.
        let id = app.store.add(awkward_event("Also doomed"));
        app.open_edit_event(id);
        probe::click(&mut app, Target::DeleteEvent);
        assert_eq!(app.pending_delete, Some(id));
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(app.store.is_empty());
    }

    #[test]
    fn the_form_takes_every_key_and_press_while_it_is_up() {
        let mut app = sample_app(june_2024());
        probe::click(&mut app, Target::NewEvent);
        assert!(app.form.is_some(), "the New event button did nothing");
        // "2" is the week view's key; in the form it is a character.
        type_in(&mut app, "2");
        assert_eq!(app.view, CalendarView::Month, "a key reached the calendar");
        assert_eq!(app.form.as_ref().unwrap().title.text(), "2");
        // A press beside the card changes nothing behind it.
        let selected = app.selected_date;
        let result = app.click_at(
            5.0,
            DEFAULT_HEIGHT - 5.0,
            MouseButton::Left,
            (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        );
        assert_eq!(result, EventResult::Consumed);
        assert!(app.form.is_some(), "a press beside the form closed it");
        assert_eq!(app.selected_date, selected);
        // Its own controls answer.
        probe::click(&mut app, Target::StepForward(FormField::Category));
        assert_eq!(app.form.as_ref().unwrap().category, EventCategory::Health);
        probe::click(&mut app, Target::Field(FormField::Place));
        assert_eq!(app.form_field, FormField::Place);
        probe::click(&mut app, Target::Cancel);
        assert!(app.form.is_none());
    }

    #[test]
    fn a_second_press_on_an_event_opens_it() {
        let mut app = sample_app(june_2024());
        let id = app
            .store
            .events_on(june_2024())
            .first()
            .map(|e| e.id)
            .expect("control: an event on the day");
        probe::click(&mut app, Target::Event(id));
        assert_eq!(app.selected_event_id, Some(id));
        assert!(app.form.is_none(), "the first press opened it");
        probe::click(&mut app, Target::Event(id));
        assert_eq!(app.form.as_ref().and_then(|f| f.id), Some(id));
    }

    #[test]
    fn a_calendar_file_that_cannot_be_read_is_left_as_it_is() {
        settingsfile::testing::with_scratch_config("calendar-broken", |_| {
            let path = events_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let broken = "slateos-calendar\t1\nevent\t1\tnot a date\n";
            std::fs::write(&path, broken).unwrap();
            let mut app = CalendarApp::from_settings(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            let error = app
                .store_error
                .clone()
                .expect("an unreadable file was taken without a word");
            assert!(error.contains("line 2"), "{error}");
            assert!(drawn(&app).contains(&error), "the refusal is not on screen");
            probe::key(&mut app, &probe::press(Key::N));
            type_in(&mut app, "New");
            probe::key(&mut app, &probe::press(Key::Enter));
            assert_eq!(app.store.len(), 1);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                broken,
                "the unreadable file was saved over"
            );
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::Exit
            ));
        });
    }

    #[test]
    fn a_calendar_file_too_big_to_read_whole_is_refused() {
        settingsfile::testing::with_scratch_config("calendar-big", |_| {
            let mut store = EventStore::new();
            store.add(awkward_event("One"));
            let text = calendar_text(&store);
            let path = events_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
            let mut app = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            app.persist = true;
            app.load_events_within(&path, text.len() - 1);
            assert!(app.store_error.clone().unwrap().contains("larger than"));
            assert!(!app.persist, "a file read in part would be saved over");
            let mut whole = CalendarApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            whole.persist = true;
            whole.load_events_within(&path, text.len());
            assert!(whole.store_error.is_none(), "{:?}", whole.store_error);
            assert_eq!(whole.store.len(), 1, "control: the whole file reads");
        });
    }

    #[test]
    fn closing_while_a_save_fails_asks_first() {
        settingsfile::testing::with_scratch_config("calendar-failing", |_| {
            let mut app = CalendarApp::from_settings(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            let path = events_path().unwrap();
            // A directory where the file goes: every write fails.
            std::fs::create_dir_all(&path).unwrap();
            probe::key(&mut app, &probe::press(Key::N));
            type_in(&mut app, "Unkept");
            probe::key(&mut app, &probe::press(Key::Enter));
            let error = app.store_error.clone().expect("a failed save said nothing");
            assert!(
                error.starts_with("Your calendar was not saved to "),
                "{error}"
            );
            assert!(drawn(&app).contains(&error), "the failure is not on screen");
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            let question: String = app
                .render(DEFAULT_WIDTH, DEFAULT_HEIGHT)
                .commands
                .into_iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                question.contains("not saved"),
                "the question is not drawn: {question}"
            );
            // A key under the question reaches nothing.
            probe::key(&mut app, &probe::press(Key::N));
            assert!(app.form.is_none(), "a key reached the calendar");
            // Save while it still fails: the window stays.
            assert!(matches!(
                app.on_event(&Event::Key(probe::press(Key::S))),
                Response::Redraw
            ));
            assert!(app.running);
            // Put right, then Save: it goes, and the event is there next time.
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            std::fs::remove_dir(&path).unwrap();
            assert!(matches!(
                app.on_event(&Event::Key(probe::press(Key::S))),
                Response::Exit
            ));
            let again = CalendarApp::from_settings(DEFAULT_WIDTH, DEFAULT_HEIGHT, a_saturday());
            assert_eq!(again.store.len(), 1);
            // Don't save lets a failing window go.
            let mut failing = again;
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir_all(&path).unwrap();
            failing.store.add(awkward_event("Lost"));
            failing.keep();
            assert!(matches!(
                failing.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            assert!(matches!(
                failing.on_event(&Event::Key(probe::press(Key::D))),
                Response::Exit
            ));
        });
    }

    #[test]
    fn a_search_finds_accents_in_any_case_and_places() {
        let mut store = EventStore::new();
        store.add(awkward_event("\u{c9}t\u{e9} party"));
        let mut there = awkward_event("Meeting");
        there.location = Some(String::from("Gare du Nord"));
        store.add(there);
        assert_eq!(
            store.search("\u{e9}t\u{e9}").len(),
            1,
            "case folded only ASCII"
        );
        assert_eq!(store.search("gare").len(), 1, "the place is not searched");
    }

    #[test]
    fn the_store_counts_its_changes_and_nothing_else() {
        let mut store = EventStore::new();
        let r0 = store.revision();
        let id = store.add(awkward_event("A"));
        let r1 = store.revision();
        assert_ne!(r1, r0, "an add was not counted");
        let _ = store.get(id);
        let _ = store.search("A");
        assert_eq!(store.revision(), r1, "reading counted as a change");
        assert!(!store.remove(id + 99));
        assert_eq!(store.revision(), r1, "removing nothing counted as a change");
        assert!(store.get_mut(id + 99).is_none());
        assert_eq!(store.revision(), r1);
        store.get_mut(id).unwrap().title = String::from("B");
        let r2 = store.revision();
        assert_ne!(r2, r1, "a change through get_mut was not counted");
        assert!(store.remove(id));
        assert_ne!(store.revision(), r2, "a remove was not counted");
        let r3 = store.revision();
        assert_eq!(store.import_ics("nothing here"), 0);
        assert_eq!(store.revision(), r3, "an import of nothing counted");
        let ics = generate_ics(&[awkward_event("Imported")], "Elsewhere");
        assert_eq!(store.import_ics(&ics), 1, "control: the import reads");
        assert_ne!(store.revision(), r3, "an import was not counted");
    }
}
