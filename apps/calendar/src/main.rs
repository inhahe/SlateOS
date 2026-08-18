//! Calendar and scheduling application for SlateOS.
//!
//! Provides month/week/day/year views, event creation with recurrence,
//! categories, reminders, ICS import/export, and a mini-calendar sidebar.

use guitk::color::Color;
// The shared civil-date arithmetic. This app used to carry its own: a Zeller's
// congruence for the weekday, a *separate* Julian day number for differences,
// its own leap rule, and an ISO week number its own comment admitted was "a
// simple approximation". The approximation disagreed with the real ISO week on
// 38.5% of the days between 1900 and 2100. See `known-issues.md`
// C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC.
use guitk::date::{self, Weekday};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

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
        self.hour * 60 + self.minute
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
            (self.hour - 12, "PM")
        };
        format!("{h}:{:02} {ampm}", self.minute)
    }

    /// Minutes between two times (self - other).
    pub fn minutes_since(self, other: Self) -> i32 {
        self.to_minutes() as i32 - other.to_minutes() as i32
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
                diff >= 0 && diff % (*interval_days as i64) == 0
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
            end_min - start_min
        } else {
            (24 * 60 - start_min) + end_min
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
                lines.push(format!("RRULE:FREQ=DAILY;INTERVAL={interval_days}"))
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
                next_id += 1;
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
        self.next_id += 1;
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
            self.next_id += 1;
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
    // Rendering
    // ========================================================================

    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top bar
        self.render_top_bar(&mut cmds);

        let content_y = 50.0;
        let sidebar_w = if self.sidebar_visible { 220.0 } else { 0.0 };
        let main_x = sidebar_w;
        let main_w = self.width - sidebar_w;

        // Sidebar (mini calendar + categories)
        if self.sidebar_visible {
            self.render_sidebar(&mut cmds, content_y);
        }

        // Main content area
        cmds.push(RenderCommand::PushClip {
            x: main_x,
            y: content_y,
            width: main_w,
            height: self.height - content_y,
        });

        match self.view {
            CalendarView::Month => self.render_month_view(&mut cmds, main_x, content_y, main_w),
            CalendarView::Week => self.render_week_view(&mut cmds, main_x, content_y, main_w),
            CalendarView::Day => self.render_day_view(&mut cmds, main_x, content_y, main_w),
            CalendarView::Year => self.render_year_view(&mut cmds, main_x, content_y, main_w),
            CalendarView::Agenda => self.render_agenda_view(&mut cmds, main_x, content_y, main_w),
        }

        cmds.push(RenderCommand::PopClip);

        cmds
    }

    fn render_top_bar(&self, cmds: &mut Vec<RenderCommand>) {
        // Bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: 48.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Navigation buttons
        self.render_nav_button(cmds, 8.0, 8.0, 32.0, 32.0, "<");
        self.render_nav_button(cmds, 44.0, 8.0, 32.0, 32.0, ">");

        // Today button
        cmds.push(RenderCommand::FillRect {
            x: 84.0,
            y: 10.0,
            width: 60.0,
            height: 28.0,
            color: BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: 96.0,
            y: 18.0,
            text: "Today".to_string(),
            font_size: 12.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(52.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Current view date header
        let header = match self.view {
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
            CalendarView::Agenda => format!("Agenda from {}", self.view_date.format_short()),
        };

        cmds.push(RenderCommand::Text {
            x: 160.0,
            y: 16.0,
            text: header,
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // View selector buttons
        let views = CalendarView::all();
        let view_x = self.width - (views.len() as f32 * 68.0) - 8.0;
        for (i, v) in views.iter().enumerate() {
            let bx = view_x + i as f32 * 68.0;
            let active = *v == self.view;
            let bg = if active { SURFACE0 } else { MANTLE };
            let fg = if active { BLUE } else { SUBTEXT0 };

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: 10.0,
                width: 64.0,
                height: 28.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: 18.0,
                text: v.label().to_string(),
                font_size: 11.0,
                color: fg,
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(52.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: 48.0,
            x2: self.width,
            y2: 48.0,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_nav_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + w / 2.0 - 4.0,
            y: y + h / 2.0 - 6.0,
            text: label.to_string(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, top_y: f32) {
        let sidebar_w = 220.0;

        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top_y,
            width: sidebar_w,
            height: self.height - top_y,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Mini calendar
        self.render_mini_calendar(cmds, 10.0, top_y + 10.0, 200.0);

        // Category filters
        let cat_y = top_y + 210.0;
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: cat_y,
            text: "Categories".to_string(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        for (i, cat) in EventCategory::all().iter().enumerate() {
            let cy = cat_y + 22.0 + i as f32 * 24.0;
            let active = self.category_filter.is_none() || self.category_filter == Some(*cat);

            cmds.push(RenderCommand::FillRect {
                x: 12.0,
                y: cy,
                width: 12.0,
                height: 12.0,
                color: if active { cat.color() } else { SURFACE0 },
                corner_radii: CornerRadii::all(2.0),
            });
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: cy,
                text: cat.label().to_string(),
                font_size: 11.0,
                color: if active { TEXT } else { OVERLAY0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(160.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Event count
            let count = self.store.events_by_category(*cat).len();
            if count > 0 {
                cmds.push(RenderCommand::Text {
                    x: 180.0,
                    y: cy,
                    text: count.to_string(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sidebar_w,
            y1: top_y,
            x2: sidebar_w,
            y2: self.height,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_mini_calendar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32) {
        let cell_w = w / 7.0;
        let cell_h = 18.0;

        // Month/year header
        let header = format!(
            "{} {}",
            month_short(self.mini_cal_month),
            self.mini_cal_year
        );
        cmds.push(RenderCommand::Text {
            x: x + w / 2.0 - 30.0,
            y,
            text: header,
            font_size: 11.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w),
            overflow: TextOverflow::Ellipsis,
        });

        // Day-of-week headers
        let day_headers = if self.week_starts_monday {
            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
        } else {
            ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
        };

        let header_y = y + 18.0;
        for (i, dh) in day_headers.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: x + i as f32 * cell_w + 2.0,
                y: header_y,
                text: dh.to_string(),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(cell_w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Days grid
        let first_dow = first_dow_of_month(self.mini_cal_year, self.mini_cal_month);
        let start_offset = if self.week_starts_monday {
            if first_dow == 0 { 6 } else { first_dow - 1 }
        } else {
            first_dow
        };
        let total_days = days_in_month(self.mini_cal_year, self.mini_cal_month);

        let grid_y = header_y + 16.0;
        for day in 1..=total_days {
            let pos = (day - 1 + start_offset) as usize;
            let row = pos / 7;
            let col = pos % 7;
            let dx = x + col as f32 * cell_w;
            let dy = grid_y + row as f32 * cell_h;

            let date = Date {
                year: self.mini_cal_year,
                month: self.mini_cal_month,
                day,
            };
            let is_today = date.is_today(self.today);
            let is_selected = date == self.selected_date;
            let has_events = !self.store.events_on(date).is_empty();

            if is_today {
                cmds.push(RenderCommand::FillRect {
                    x: dx,
                    y: dy - 1.0,
                    width: cell_w - 1.0,
                    height: cell_h - 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(3.0),
                });
            } else if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: dx,
                    y: dy - 1.0,
                    width: cell_w - 1.0,
                    height: cell_h - 2.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(3.0),
                });
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

            cmds.push(RenderCommand::Text {
                x: dx + 4.0,
                y: dy + 1.0,
                text: day.to_string(),
                font_size: 10.0,
                color: fg,
                font_weight: if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(cell_w - 4.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Event dot
            if has_events && !is_today {
                cmds.push(RenderCommand::FillRect {
                    x: dx + cell_w / 2.0 - 2.0,
                    y: dy + cell_h - 5.0,
                    width: 4.0,
                    height: 3.0,
                    color: PEACH,
                    corner_radii: CornerRadii::all(1.5),
                });
            }
        }
    }

    fn week_start(&self, date: Date) -> Date {
        let dow = date.day_of_week();
        let offset = if self.week_starts_monday {
            if dow == 0 { 6 } else { dow - 1 }
        } else {
            dow
        };
        date.add_days(-(offset as i32))
    }

    fn render_month_view(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32) {
        let col_w = w / 7.0;
        let header_h = 24.0;

        // Day-of-week headers
        let day_headers = if self.week_starts_monday {
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
        };

        for (i, dh) in day_headers.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: x + i as f32 * col_w + 8.0,
                y: y + 6.0,
                text: dh.to_string(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_w - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Grid
        let grid_y = y + header_h;
        let available_h = self.height - grid_y;
        let first_dow = first_dow_of_month(self.view_date.year, self.view_date.month);
        let start_offset = if self.week_starts_monday {
            if first_dow == 0 { 6 } else { first_dow - 1 }
        } else {
            first_dow
        };
        let total_days = days_in_month(self.view_date.year, self.view_date.month);
        let total_cells = start_offset + total_days;
        let num_rows = total_cells.div_ceil(7).max(5);
        let row_h = available_h / num_rows as f32;

        for day in 1..=total_days {
            let pos = (day - 1 + start_offset) as usize;
            let row = pos / 7;
            let col = pos % 7;
            let cx = x + col as f32 * col_w;
            let cy = grid_y + row as f32 * row_h;

            let date = Date {
                year: self.view_date.year,
                month: self.view_date.month,
                day,
            };
            let is_today = date.is_today(self.today);
            let is_selected = date == self.selected_date;

            // Cell border
            cmds.push(RenderCommand::StrokeRect {
                x: cx,
                y: cy,
                width: col_w,
                height: row_h,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
                line_width: 0.5,
            });

            // Day number
            let day_bg_color = if is_today {
                BLUE
            } else if is_selected {
                SURFACE1
            } else {
                Color::rgba(0, 0, 0, 0)
            };
            if is_today || is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: cx + 4.0,
                    y: cy + 2.0,
                    width: 22.0,
                    height: 18.0,
                    color: day_bg_color,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let day_fg = if is_today {
                CRUST
            } else if date.is_weekend() {
                SUBTEXT0
            } else {
                TEXT
            };
            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: cy + 4.0,
                text: day.to_string(),
                font_size: 12.0,
                color: day_fg,
                font_weight: if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(20.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Events for this day
            let events = self.store.events_on(date);
            let max_visible = ((row_h - 24.0) / 16.0) as usize;
            for (ei, ev) in events.iter().enumerate().take(max_visible) {
                let ey = cy + 22.0 + ei as f32 * 16.0;

                cmds.push(RenderCommand::FillRect {
                    x: cx + 4.0,
                    y: ey,
                    width: col_w - 8.0,
                    height: 14.0,
                    color: ev.effective_color(),
                    corner_radii: CornerRadii::all(2.0),
                });

                let time_prefix = if ev.all_day {
                    String::new()
                } else {
                    format!("{} ", ev.start.time.format_12h())
                };

                cmds.push(RenderCommand::Text {
                    x: cx + 7.0,
                    y: ey + 2.0,
                    text: format!("{time_prefix}{}", ev.title),
                    font_size: 9.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(col_w - 14.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            if events.len() > max_visible {
                cmds.push(RenderCommand::Text {
                    x: cx + 8.0,
                    y: cy + 22.0 + max_visible as f32 * 16.0,
                    text: format!("+{} more", events.len() - max_visible),
                    font_size: 9.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(col_w - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_week_view(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32) {
        let week_start = self.week_start(self.view_date);
        let time_col_w = 50.0;
        let day_w = (w - time_col_w) / 7.0;
        let hour_h = 48.0;
        let header_h = 40.0;

        // Day headers
        for i in 0..7 {
            let date = week_start.add_days(i);
            let dx = x + time_col_w + i as f32 * day_w;
            let is_today = date.is_today(self.today);

            cmds.push(RenderCommand::FillRect {
                x: dx,
                y,
                width: day_w,
                height: header_h,
                color: if is_today { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::ZERO,
            });

            cmds.push(RenderCommand::Text {
                x: dx + 4.0,
                y: y + 4.0,
                text: date.day_of_week_short().to_string(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(day_w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });

            let day_color = if is_today { BLUE } else { TEXT };
            cmds.push(RenderCommand::Text {
                x: dx + 4.0,
                y: y + 18.0,
                text: date.day.to_string(),
                font_size: 16.0,
                color: day_color,
                font_weight: if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(day_w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Time grid
        let grid_y = y + header_h;
        for hour in 0..24 {
            let hy = grid_y + hour as f32 * hour_h;

            // Time label
            let time = Time { hour, minute: 0 };
            cmds.push(RenderCommand::Text {
                x: x + 4.0,
                y: hy + 2.0,
                text: if self.use_24h {
                    time.format_24h()
                } else {
                    time.format_12h()
                },
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(time_col_w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Hour line
            cmds.push(RenderCommand::Line {
                x1: x + time_col_w,
                y1: hy,
                x2: x + w,
                y2: hy,
                color: SURFACE0,
                width: 0.5,
            });

            // Half-hour line
            cmds.push(RenderCommand::Line {
                x1: x + time_col_w,
                y1: hy + hour_h / 2.0,
                x2: x + w,
                y2: hy + hour_h / 2.0,
                color: Color::rgba(49, 50, 68, 128),
                width: 0.5,
            });
        }

        // Events on the grid
        for i in 0..7 {
            let date = week_start.add_days(i);
            let dx = x + time_col_w + i as f32 * day_w;
            let events = self.store.events_on(date);

            for ev in &events {
                if ev.all_day {
                    continue;
                }
                let start_min = ev.start.time.to_minutes() as f32;
                let end_min = ev.end.time.to_minutes() as f32;
                let ey = grid_y + (start_min / 60.0) * hour_h;
                let eh = ((end_min - start_min) / 60.0) * hour_h;

                cmds.push(RenderCommand::FillRect {
                    x: dx + 2.0,
                    y: ey,
                    width: day_w - 4.0,
                    height: eh.max(16.0),
                    color: ev.effective_color(),
                    corner_radii: CornerRadii::all(3.0),
                });

                cmds.push(RenderCommand::Text {
                    x: dx + 5.0,
                    y: ey + 2.0,
                    text: ev.title.clone(),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(day_w - 10.0),
                    overflow: TextOverflow::Ellipsis,
                });

                if eh > 20.0 {
                    cmds.push(RenderCommand::Text {
                        x: dx + 5.0,
                        y: ey + 14.0,
                        text: ev.time_range_label(),
                        font_size: 9.0,
                        color: CRUST,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(day_w - 10.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
        }
    }

    fn render_day_view(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32) {
        let time_col_w = 60.0;
        let hour_h = 60.0;
        let header_h = 36.0;

        // Day header
        let is_today = self.view_date.is_today(self.today);
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: header_h,
            color: if is_today { SURFACE0 } else { MANTLE },
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 10.0,
            text: self.view_date.format_long(),
            font_size: 14.0,
            color: if is_today { BLUE } else { TEXT },
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // All-day events
        let all_day_events: Vec<_> = self
            .store
            .events_on(self.view_date)
            .into_iter()
            .filter(|e| e.all_day)
            .collect();
        let all_day_h = if all_day_events.is_empty() {
            0.0
        } else {
            28.0 * all_day_events.len() as f32 + 8.0
        };

        for (i, ev) in all_day_events.iter().enumerate() {
            let ey = y + header_h + 4.0 + i as f32 * 28.0;
            cmds.push(RenderCommand::FillRect {
                x: x + time_col_w,
                y: ey,
                width: w - time_col_w - 16.0,
                height: 24.0,
                color: ev.effective_color(),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + time_col_w + 8.0,
                y: ey + 5.0,
                text: format!("All day: {}", ev.title),
                font_size: 11.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w - time_col_w - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Time grid
        let grid_y = y + header_h + all_day_h;
        for hour in 0..24 {
            let hy = grid_y + hour as f32 * hour_h;

            let time = Time { hour, minute: 0 };
            cmds.push(RenderCommand::Text {
                x: x + 4.0,
                y: hy + 2.0,
                text: if self.use_24h {
                    time.format_24h()
                } else {
                    time.format_12h()
                },
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(time_col_w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Line {
                x1: x + time_col_w,
                y1: hy,
                x2: x + w,
                y2: hy,
                color: SURFACE0,
                width: 0.5,
            });
        }

        // Timed events
        let timed_events: Vec<_> = self
            .store
            .events_on(self.view_date)
            .into_iter()
            .filter(|e| !e.all_day)
            .collect();
        let event_w = w - time_col_w - 16.0;

        for ev in &timed_events {
            let start_min = ev.start.time.to_minutes() as f32;
            let end_min = ev.end.time.to_minutes() as f32;
            let ey = grid_y + (start_min / 60.0) * hour_h;
            let eh = ((end_min - start_min) / 60.0) * hour_h;

            cmds.push(RenderCommand::FillRect {
                x: x + time_col_w + 4.0,
                y: ey,
                width: event_w,
                height: eh.max(20.0),
                color: ev.effective_color(),
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + time_col_w + 10.0,
                y: ey + 4.0,
                text: ev.title.clone(),
                font_size: 12.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(event_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            if eh > 24.0 {
                cmds.push(RenderCommand::Text {
                    x: x + time_col_w + 10.0,
                    y: ey + 18.0,
                    text: ev.time_range_label(),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(event_w - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            if eh > 40.0
                && let Some(loc) = &ev.location {
                    cmds.push(RenderCommand::Text {
                        x: x + time_col_w + 10.0,
                        y: ey + 32.0,
                        text: loc.clone(),
                        font_size: 10.0,
                        color: CRUST,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(event_w - 16.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
        }
    }

    fn render_year_view(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32) {
        let cols = 4;
        let rows = 3;
        let month_w = w / cols as f32;
        let month_h = (self.height - y - 10.0) / rows as f32;

        for month in 1..=12u32 {
            let col = ((month - 1) % cols as u32) as f32;
            let row = ((month - 1) / cols as u32) as f32;
            let mx = x + col * month_w;
            let my = y + row * month_h;

            // Month name
            let is_current_month =
                self.view_date.year == self.today.year && month == self.today.month;

            cmds.push(RenderCommand::Text {
                x: mx + 8.0,
                y: my + 4.0,
                text: month_name(month).to_string(),
                font_size: 12.0,
                color: if is_current_month { BLUE } else { TEXT },
                font_weight: FontWeightHint::Bold,
                max_width: Some(month_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Mini day grid
            let cell_w = (month_w - 16.0) / 7.0;
            let cell_h = 14.0;
            let grid_y = my + 22.0;

            let first_dow = first_dow_of_month(self.view_date.year, month);
            let start_offset = if self.week_starts_monday {
                if first_dow == 0 { 6 } else { first_dow - 1 }
            } else {
                first_dow
            };
            let total = days_in_month(self.view_date.year, month);

            for day in 1..=total {
                let pos = (day - 1 + start_offset) as usize;
                let r = pos / 7;
                let c = pos % 7;
                let dx = mx + 8.0 + c as f32 * cell_w;
                let dy = grid_y + r as f32 * cell_h;

                let date = Date {
                    year: self.view_date.year,
                    month,
                    day,
                };
                let is_today = date.is_today(self.today);
                let has_events = !self.store.events_on(date).is_empty();

                if is_today {
                    cmds.push(RenderCommand::FillRect {
                        x: dx - 1.0,
                        y: dy - 1.0,
                        width: cell_w,
                        height: cell_h - 1.0,
                        color: BLUE,
                        corner_radii: CornerRadii::all(2.0),
                    });
                }

                let fg = if is_today {
                    CRUST
                } else if has_events {
                    PEACH
                } else if date.is_weekend() {
                    OVERLAY0
                } else {
                    SUBTEXT0
                };

                cmds.push(RenderCommand::Text {
                    x: dx,
                    y: dy,
                    text: day.to_string(),
                    font_size: 8.0,
                    color: fg,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(cell_w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_agenda_view(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32) {
        let upcoming = self.store.upcoming(self.view_date, 30);

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 8.0,
            text: format!("Upcoming Events ({})", upcoming.len()),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        let mut row_y = y + 36.0;
        let mut last_date: Option<Date> = None;

        for ev in &upcoming {
            // Date header if different from last
            if last_date != Some(ev.start.date) {
                if last_date.is_some() {
                    row_y += 8.0;
                }

                let is_today = ev.start.date.is_today(self.today);
                cmds.push(RenderCommand::FillRect {
                    x: x + 8.0,
                    y: row_y,
                    width: w - 16.0,
                    height: 22.0,
                    color: if is_today { SURFACE0 } else { MANTLE },
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 4.0,
                    text: if is_today {
                        format!("Today - {}", ev.start.date.format_long())
                    } else {
                        ev.start.date.format_long()
                    },
                    font_size: 12.0,
                    color: if is_today { BLUE } else { TEXT },
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(w - 40.0),
                    overflow: TextOverflow::Ellipsis,
                });
                row_y += 26.0;
                last_date = Some(ev.start.date);
            }

            // Event card
            cmds.push(RenderCommand::FillRect {
                x: x + 16.0,
                y: row_y,
                width: 4.0,
                height: 40.0,
                color: ev.effective_color(),
                corner_radii: CornerRadii::all(2.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: row_y + 2.0,
                text: ev.title.clone(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w - 100.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: row_y + 18.0,
                text: format!(
                    "{} | {} | {}",
                    ev.time_range_label(),
                    ev.duration_label(),
                    ev.category.label()
                ),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 60.0),
                overflow: TextOverflow::Ellipsis,
            });

            if let Some(loc) = &ev.location {
                cmds.push(RenderCommand::Text {
                    x: x + 28.0,
                    y: row_y + 30.0,
                    text: loc.clone(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 60.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            row_y += 46.0;
        }

        if upcoming.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 60.0,
                y: y + 100.0,
                text: "No upcoming events".to_string(),
                font_size: 14.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
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

fn main() {
    let today = Date {
        year: 2026,
        month: 5,
        day: 18,
    };
    let mut app = CalendarApp::new(1280.0, 720.0, today);

    sample_events(&mut app.store, today);

    // Verify all views render
    for view in CalendarView::all() {
        app.view = *view;
        let cmds = app.render();
        let _ = cmds.len();
    }

    // Test navigation
    app.navigate_forward();
    app.navigate_backward();
    app.go_to_today();

    // Test ICS export
    let ics = app.store.export_ics("My Calendar");
    let _ = ics.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
            let cmds = app.render();
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
        let cmds = app.render();
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
}
