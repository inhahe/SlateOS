//! A calendar's events, and the forms they are kept and exchanged in.
//!
//! Split out of `apps/calendar` on 2026-09-27 so a second reader can use the
//! same events without depending on the calendar program: the desktop's
//! calendar popup drew from a store of its own that nothing ever filled, and
//! reads the calendar's file through this crate instead (lane C's request,
//! C-Q19). The code moved as it was; what is here is what the calendar
//! program already did.
//!
//! - **The model**: [`Date`], [`Time`], [`DateTime`], [`EventCategory`],
//!   [`RecurrenceRule`], [`Reminder`] and [`CalendarEvent`], with
//!   [`CalendarEvent::occurs_on`] deciding which days an event is on --
//!   repeats and several-day all-day events included.
//! - **The kept calendar**: `<config>/calendar/events.txt`, one event a line,
//!   tab-separated ([`calendar_text`], [`parse_calendar`], [`events_path`]);
//!   read whole or not at all. [`load`] reads it for a program that only
//!   shows the events.
//! - **iCalendar** import and export ([`parse_ics_report`],
//!   [`generate_ics`]).

use appearance::Palette;
use guitk::color::Color;
use guitk::date::{self, Weekday};
use textfmt::tsv;

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
    pub fn civil(self) -> date::Date {
        date::Date::from_ymd(self.year, self.month, self.day)
    }

    /// The inverse of [`civil`](Self::civil).
    pub fn from_civil(d: date::Date) -> Self {
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

    /// Whether the event is on `date`: the day it starts, a day it repeats
    /// on, or -- for an all-day event of several days -- any day it covers.
    ///
    /// A timed event that runs past midnight is on the day it starts only:
    /// the day and week views place an event by its times, which would be
    /// wrong on the days after.
    pub fn occurs_on(&self, date: Date) -> bool {
        let span = if self.all_day {
            i32::try_from(self.end.date.days_since(self.start.date).clamp(0, 366)).unwrap_or(0)
        } else {
            0
        };
        (0..=span).any(|back| {
            let origin = date.add_days(back.wrapping_neg());
            origin == self.start.date || self.recurrence.matches(self.start.date, origin)
        })
    }

    /// The event as an iCalendar `VEVENT`, each line folded at 75 octets as
    /// RFC 5545 asks.
    ///
    /// An all-day event is written as dates (`VALUE=DATE`, the end the day
    /// after its last), as other calendars write one, rather than as a
    /// midnight-to-23:59 appointment; a reminder as an alarm, so a calendar
    /// that can raise one does.
    pub fn to_ics(&self) -> String {
        let date = |d: Date| format!("{:04}{:02}{:02}", d.year, d.month, d.day);
        let mut lines = vec![
            String::from("BEGIN:VEVENT"),
            format!("UID:{}-slateos@calendar", self.id),
        ];
        if self.all_day {
            lines.push(format!("DTSTART;VALUE=DATE:{}", date(self.start.date)));
            let last = self.end.date.max(self.start.date);
            lines.push(format!("DTEND;VALUE=DATE:{}", date(last.add_days(1))));
        } else {
            lines.push(format!("DTSTART:{}", self.start.format_ics()));
            lines.push(format!("DTEND:{}", self.end.format_ics()));
        }
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
                    .filter_map(|d| ICS_DAYS.get(usize::try_from(*d).ok()?).copied())
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
            // Every no days is a repeat that never happens, and `INTERVAL=0`
            // is not one the standard allows.
            RecurrenceRule::Custom { interval_days: 0 } | RecurrenceRule::None => {}
            RecurrenceRule::Custom { interval_days } => {
                lines.push(format!("RRULE:FREQ=DAILY;INTERVAL={interval_days}"));
            }
        }
        let trigger = match self.reminder {
            Reminder::None => None,
            Reminder::AtTime => Some(String::from("PT0M")),
            Reminder::MinutesBefore(n) => Some(format!("-PT{n}M")),
            Reminder::HoursBefore(n) => Some(format!("-PT{n}H")),
            Reminder::DayBefore => Some(String::from("-P1D")),
        };
        if let Some(trigger) = trigger {
            lines.push(String::from("BEGIN:VALARM"));
            lines.push(String::from("ACTION:DISPLAY"));
            lines.push(format!("DESCRIPTION:{}", ics_escape(&self.title)));
            lines.push(format!("TRIGGER:{trigger}"));
            lines.push(String::from("END:VALARM"));
        }
        lines.push("END:VEVENT".to_string());
        lines
            .iter()
            .map(|l| fold_ics_line(l))
            .collect::<Vec<_>>()
            .join("\r\n")
    }
}

/// The weekdays as iCalendar names them, Sunday first, as the model counts.
const ICS_DAYS: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

/// A content line folded as RFC 5545 asks: no line longer than 75 octets,
/// each continuation starting with a space, and never inside a character.
fn fold_ics_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut width = 0_usize;
    for c in line.chars() {
        let len = c.len_utf8();
        if width.saturating_add(len) > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width = width.saturating_add(len);
    }
    out
}

/// Text as an iCalendar value: backslash, semicolon, comma and line break
/// escaped -- a carriage return, alone or before a line feed, as one break.
fn ics_escape(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

// ============================================================================
// ICS parser (basic)
// ============================================================================

/// What an `.ics` import found: the events it could read, and what it had
/// to leave out or keep in a simpler form -- so the import can say so, since
/// a calendar missing an appointment looks exactly like one that never had
/// it.
#[derive(Debug, Default)]
pub struct IcsImport {
    pub events: Vec<CalendarEvent>,
    /// Events with no start this could read, left out.
    pub unreadable: usize,
    /// Events whose times were written for a time zone, or in UTC, kept as
    /// the clock time written: this calendar has no time zones.
    pub zoned: usize,
    /// Events whose repeat this calendar cannot keep as written -- one with
    /// an end, or a rule it has no name for -- kept as the nearest it has.
    pub simplified: usize,
}

/// The events in an iCalendar text, as [`import_ics`](parse_ics_report)
/// reads them.
pub fn parse_ics(content: &str) -> Vec<CalendarEvent> {
    parse_ics_report(content).events
}

/// The logical lines of an iCalendar text: a line beginning with a space or
/// a tab continues the one before it (RFC 5545's folding), and a line may
/// end in a carriage return or not.
fn unfold_ics(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in content.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t'))
            && let Some(last) = out.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        out.push(line.to_owned());
    }
    out
}

/// A content line split into its name (upper case), its parameters (names
/// upper case, values unquoted) and its value:
/// `DTSTART;TZID="Europe/Paris":20260926T090000`. The value starts after the
/// first colon outside a quoted parameter.
fn split_ics_line(line: &str) -> Option<(String, IcsParams, &str)> {
    let mut in_quotes = false;
    let mut cuts = Vec::new();
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => cuts.push(i),
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let value = line.get(colon.saturating_add(1)..)?;
    let name_end = cuts.first().copied().unwrap_or(colon);
    let name = line.get(..name_end)?.trim().to_ascii_uppercase();
    let mut params = Vec::new();
    for (k, &at) in cuts.iter().enumerate() {
        let until = cuts.get(k.saturating_add(1)).copied().unwrap_or(colon);
        let Some(part) = line.get(at.saturating_add(1)..until) else {
            continue;
        };
        if let Some((key, val)) = part.split_once('=') {
            params.push((
                key.trim().to_ascii_uppercase(),
                val.trim().trim_matches('"').to_owned(),
            ));
        }
    }
    Some((name, params, value))
}

/// A content line's parameters: names in upper case, values unquoted.
type IcsParams = Vec<(String, String)>;

/// Parameter `key` of a content line.
fn ics_param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// When a start or an end is: the date and time, whether only a date was
/// written, and whether the time was written for a zone -- a `TZID`, or `Z`
/// for UTC.
fn parse_ics_when(value: &str, params: &[(String, String)]) -> Option<(DateTime, bool, bool)> {
    let value = value.trim();
    let date_only = ics_param(params, "VALUE") == Some("DATE")
        || (value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit()));
    let zoned = !date_only && (ics_param(params, "TZID").is_some() || value.ends_with('Z'));
    Some((parse_ics_datetime(value)?, date_only, zoned))
}

/// An iCalendar duration in minutes -- `PT1H30M`, `P1D`, `-P2W` -- with
/// seconds dropped.
fn parse_ics_duration(value: &str) -> Option<i64> {
    let v = value.trim();
    let (negative, v) = match v.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let v = v.strip_prefix('P')?;
    let mut minutes: i64 = 0;
    let mut number = String::new();
    let mut in_time = false;
    for c in v.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        if c == 'T' && number.is_empty() {
            in_time = true;
            continue;
        }
        let n: i64 = number.parse().ok()?;
        number.clear();
        let add = match (c, in_time) {
            ('W', false) => n.checked_mul(7 * 24 * 60)?,
            ('D', false) => n.checked_mul(24 * 60)?,
            ('H', true) => n.checked_mul(60)?,
            ('M', true) => n,
            ('S', true) => n.checked_div(60)?,
            _ => return None,
        };
        minutes = minutes.checked_add(add)?;
    }
    if !number.is_empty() {
        return None;
    }
    Some(if negative {
        minutes.saturating_neg()
    } else {
        minutes
    })
}

/// `at` moved by `minutes`, across midnight as far as it goes.
fn add_minutes(at: DateTime, minutes: i64) -> DateTime {
    let total = i64::from(at.time.to_minutes()).saturating_add(minutes);
    let days = i32::try_from(total.div_euclid(24 * 60)).unwrap_or(0);
    let minute = u32::try_from(total.rem_euclid(24 * 60)).unwrap_or(0);
    DateTime::new(at.date.add_days(days), Time::from_minutes(minute))
}

/// A repeat read from an `RRULE`, and whether it had to be simplified to
/// fit: this calendar has no repeat with an end (`COUNT`, `UNTIL`), none on
/// "the second Tuesday", none every three months.
fn parse_rrule(value: &str) -> (RecurrenceRule, bool) {
    let mut freq = String::new();
    let mut interval: u32 = 1;
    let mut days: Vec<u32> = Vec::new();
    let mut simplified = false;
    for part in value.split(';') {
        let Some((key, val)) = part.split_once('=') else {
            simplified = true;
            continue;
        };
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => freq = val.trim().to_ascii_uppercase(),
            "INTERVAL" => match val.trim().parse::<u32>() {
                Ok(n) if n > 0 => interval = n,
                _ => simplified = true,
            },
            "BYDAY" => {
                for day in val.split(',') {
                    let day = day.trim().to_ascii_uppercase();
                    match ICS_DAYS.iter().position(|d| *d == day) {
                        Some(i) => days.extend(u32::try_from(i).ok()),
                        // "2TU", the second Tuesday: no such repeat here.
                        None => simplified = true,
                    }
                }
            }
            "WKST" => {}
            _ => simplified = true,
        }
    }
    let rule = match (freq.as_str(), interval) {
        ("DAILY", 1) => RecurrenceRule::Daily,
        ("DAILY", n) => RecurrenceRule::Custom { interval_days: n },
        ("WEEKLY", 1) => RecurrenceRule::Weekly { days },
        ("WEEKLY", 2) if days.is_empty() => RecurrenceRule::BiWeekly,
        ("WEEKLY", n) if days.is_empty() => RecurrenceRule::Custom {
            interval_days: n.saturating_mul(7),
        },
        ("WEEKLY", _) => {
            simplified = true;
            RecurrenceRule::Weekly { days }
        }
        ("MONTHLY", n) => {
            simplified |= n != 1 || !days.is_empty();
            RecurrenceRule::Monthly
        }
        ("YEARLY", n) => {
            simplified |= n != 1 || !days.is_empty();
            RecurrenceRule::Yearly
        }
        _ => {
            simplified = true;
            RecurrenceRule::None
        }
    };
    (rule, simplified)
}

/// A reminder read from an alarm's `TRIGGER`: how long before the start.
fn reminder_from_trigger(value: &str) -> Option<Reminder> {
    let before = parse_ics_duration(value)?.saturating_neg();
    Some(match before {
        ..=0 => Reminder::AtTime,
        1440 => Reminder::DayBefore,
        m if m % 60 == 0 && m < 1440 => Reminder::HoursBefore(u32::try_from(m / 60).ok()?),
        m => Reminder::MinutesBefore(u32::try_from(m).ok()?),
    })
}

/// One `VEVENT`, as its lines are read.
#[derive(Default)]
struct IcsEventDraft {
    title: String,
    description: String,
    location: Option<String>,
    start: Option<(DateTime, bool, bool)>,
    end: Option<(DateTime, bool, bool)>,
    duration: Option<i64>,
    category: Option<EventCategory>,
    rule: Option<(RecurrenceRule, bool)>,
    reminder: Option<Reminder>,
}

/// The events in an iCalendar text, and what could not be read or kept as
/// written.
///
/// What other calendars write, not only what this one does: folded lines,
/// parameters (`DTSTART;TZID=...:`, `DTSTART;VALUE=DATE:`), an all-day event
/// as dates with the day after it as its end, a `DURATION` instead of an end
/// or neither, a repeat (`RRULE`), several categories, and an alarm -- whose
/// own `DESCRIPTION` used to overwrite the event's. Before, every event whose
/// start had a parameter, or had no `DTEND`, was left out without a word:
/// most of what a phone's calendar exports.
pub fn parse_ics_report(content: &str) -> IcsImport {
    let mut report = IcsImport::default();
    let mut stack: Vec<String> = Vec::new();
    let mut draft: Option<IcsEventDraft> = None;
    for line in unfold_ics(content) {
        let Some((name, params, value)) = split_ics_line(&line) else {
            continue;
        };
        match name.as_str() {
            "BEGIN" => {
                let component = value.trim().to_ascii_uppercase();
                if component == "VEVENT" {
                    draft = Some(IcsEventDraft::default());
                }
                stack.push(component);
                continue;
            }
            "END" => {
                let component = value.trim().to_ascii_uppercase();
                if stack.last() == Some(&component) {
                    stack.pop();
                }
                if component == "VEVENT"
                    && let Some(done) = draft.take()
                {
                    match finish_ics_event(done) {
                        Some((event, zoned, simplified)) => {
                            report.zoned = report.zoned.saturating_add(usize::from(zoned));
                            report.simplified =
                                report.simplified.saturating_add(usize::from(simplified));
                            report.events.push(event);
                        }
                        None => report.unreadable = report.unreadable.saturating_add(1),
                    }
                }
                continue;
            }
            _ => {}
        }
        let Some(ev) = draft.as_mut() else {
            continue;
        };
        match stack.last().map(String::as_str) {
            Some("VEVENT") => match name.as_str() {
                "SUMMARY" => ev.title = ics_unescape(value),
                "DESCRIPTION" => ev.description = ics_unescape(value),
                "LOCATION" => ev.location = Some(ics_unescape(value)),
                "DTSTART" => ev.start = parse_ics_when(value, &params),
                "DTEND" => ev.end = parse_ics_when(value, &params),
                "DURATION" => ev.duration = parse_ics_duration(value),
                "RRULE" => ev.rule = Some(parse_rrule(value)),
                "CATEGORIES" if ev.category.is_none() => {
                    ev.category = value.split(',').find_map(|c| {
                        let c = ics_unescape(c.trim()).to_lowercase();
                        EventCategory::all()
                            .iter()
                            .copied()
                            .find(|k| k.label().to_lowercase() == c)
                    });
                }
                _ => {}
            },
            Some("VALARM") if name == "TRIGGER" && ev.reminder.is_none() => {
                // An alarm at a moment rather than before the start has no
                // reminder here to be.
                if ics_param(&params, "VALUE") != Some("DATE-TIME") {
                    ev.reminder = reminder_from_trigger(value);
                }
            }
            _ => {}
        }
    }
    report
}

/// The event a `VEVENT`'s lines describe -- with whether its times were
/// zoned and its repeat simplified -- or `None` if it has no start.
fn finish_ics_event(d: IcsEventDraft) -> Option<(CalendarEvent, bool, bool)> {
    let (start, all_day, zoned) = d.start?;
    let end = if all_day {
        // An all-day event ends the day before its end date: DTEND is the
        // first day it is not on.
        let last = match (d.end, d.duration) {
            (Some((end, _, _)), _) if end.date > start.date => end.date.add_days(-1),
            (None, Some(minutes)) if minutes > 24 * 60 => {
                add_minutes(start, minutes.saturating_sub(1)).date
            }
            _ => start.date,
        };
        DateTime::new(
            last,
            Time {
                hour: 23,
                minute: 59,
            },
        )
    } else {
        let end = match (d.end, d.duration) {
            (Some((end, _, _)), _) => end,
            (None, Some(minutes)) => add_minutes(start, minutes),
            (None, None) => start,
        };
        end.max(start)
    };
    let start = if all_day {
        DateTime::new(start.date, Time { hour: 0, minute: 0 })
    } else {
        start
    };
    let (recurrence, simplified) = d.rule.unwrap_or((RecurrenceRule::None, false));
    Some((
        CalendarEvent {
            id: 0,
            title: d.title,
            description: d.description,
            category: d.category.unwrap_or(EventCategory::Personal),
            start,
            end,
            all_day,
            recurrence,
            reminder: d.reminder.unwrap_or(Reminder::None),
            location: d.location.filter(|l| !l.is_empty()),
            color_override: None,
        },
        zoned,
        simplified,
    ))
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
    lines.push(fold_ics_line(&format!(
        "X-WR-CALNAME:{}",
        ics_escape(calendar_name)
    )));

    for event in events {
        lines.push(event.to_ics());
    }

    lines.push("END:VCALENDAR".to_string());
    lines.join("\r\n")
}

// ============================================================================
// The kept calendar
// ============================================================================

/// The first line of the events file, and the format it names.
pub const CALENDAR_FORMAT: &str = "slateos-calendar\t1";

/// The largest events file this will read. One cut short would be read as a
/// calendar missing its last events, with nothing to say so, and the next
/// change would write the loss back -- so a larger file is refused whole.
pub const MAX_CALENDAR_BYTES: usize = 16 * 1024 * 1024;

/// Where the events are kept, or `None` when the environment names no home
/// directory.
pub fn events_path() -> Option<std::path::PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("calendar").join("events.txt"))
}

/// A date as written: `YYYY-MM-DD`.
pub fn date_text(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

/// A date read back from `YYYY-MM-DD`, or `None` if it is not one -- a
/// thirty-first of April included.
pub fn parse_date_text(text: &str) -> Option<Date> {
    let mut parts = text.trim().splitn(3, '-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    Date::new(year, month, day)
}

/// A time read back from `HH:MM` (or `H:MM`), or `None` if it is not one.
pub fn parse_time_text(text: &str) -> Option<Time> {
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
pub fn calendar_text(events: &[CalendarEvent]) -> String {
    let mut out = String::from(CALENDAR_FORMAT);
    out.push('\n');
    for e in events {
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
pub fn parse_calendar(text: &str) -> Result<Vec<CalendarEvent>, String> {
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

/// The events kept at `path`, for a program that shows them and does not
/// change them -- the desktop's calendar popup.
///
/// No file there is no events: nothing has been added yet. A file past
/// [`MAX_CALENDAR_BYTES`] is refused whole, as the calendar refuses it,
/// rather than read in part.
///
/// # Errors
///
/// The file cannot be read, is too large, or is not a calendar this reads --
/// the reason [`parse_calendar`] gives.
pub fn load(path: &std::path::Path) -> Result<Vec<CalendarEvent>, String> {
    match safeio::read_to_string_capped(path, MAX_CALENDAR_BYTES) {
        Ok(read) if read.truncated => Err(format!(
            "the calendar is larger than {} MiB, and is not read in part",
            MAX_CALENDAR_BYTES >> 20
        )),
        Ok(read) => parse_calendar(&read.text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn event(id: u64, title: &str, start: Date) -> CalendarEvent {
        CalendarEvent {
            id,
            title: title.to_string(),
            description: String::new(),
            category: EventCategory::Work,
            start: DateTime::new(start, Time { hour: 9, minute: 0 }),
            end: DateTime::new(
                start,
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
        }
    }

    fn day(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).unwrap()
    }

    #[test]
    fn events_are_kept_and_read_back_as_they_were() {
        let mut with_everything = event(2, "Tab\there, line\nbreak", day(2026, 9, 26));
        with_everything.location = Some("Room \\ 4".to_string());
        with_everything.description = "notes".to_string();
        with_everything.recurrence = RecurrenceRule::Weekly {
            days: vec![1, 3, 5],
        };
        with_everything.reminder = Reminder::MinutesBefore(45);
        with_everything.color_override = Some(Color::rgba(1, 2, 3, 200));
        with_everything.category = EventCategory::Birthday;
        let events = vec![event(1, "Standup", day(2026, 9, 25)), with_everything];
        let text = calendar_text(&events);
        assert!(text.starts_with(CALENDAR_FORMAT));
        let back = parse_calendar(&text).unwrap();
        assert_eq!(calendar_text(&back), text, "one calendar, one spelling");
        assert_eq!(back[1].title, "Tab\there, line\nbreak");
        assert_eq!(back[1].location.as_deref(), Some("Room \\ 4"));
    }

    #[test]
    fn a_calendar_not_understood_is_refused_whole() {
        let good = calendar_text(&[event(1, "One", day(2026, 9, 25))]);
        assert!(parse_calendar(&good).is_ok());
        for bad in [
            String::new(),
            "BEGIN:VCALENDAR\n".to_string(),
            good.replacen("slateos-calendar\t1", "slateos-calendar\t2", 1),
            good.replacen("2026-09-25", "2026-02-30", 1),
            good.replacen("\twork\t", "\tparty\t", 1),
            format!("{good}{}", good.lines().nth(1).unwrap()),
        ] {
            assert!(parse_calendar(&bad).is_err(), "read: {bad:?}");
        }
    }

    #[test]
    fn load_reads_what_the_calendar_keeps_and_nothing_there_is_no_events() {
        let scratch = scratchdir::ScratchDir::new("calendarstore_load");
        let path = scratch.dir().join("events.txt");
        assert!(load(&path).unwrap().is_empty(), "no file is no events");
        let events = vec![event(1, "Dentist", day(2026, 10, 1))];
        std::fs::write(&path, calendar_text(&events)).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].title, "Dentist");
        std::fs::write(&path, "not a calendar").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn an_event_is_on_the_days_it_starts_and_repeats() {
        let mut e = event(1, "Gym", day(2026, 9, 7));
        assert!(e.occurs_on(day(2026, 9, 7)));
        assert!(!e.occurs_on(day(2026, 9, 14)));
        e.recurrence = RecurrenceRule::Weekly { days: Vec::new() };
        assert!(e.occurs_on(day(2026, 9, 14)), "a weekly event, a week on");
        assert!(!e.occurs_on(day(2026, 9, 15)));
        assert!(!e.occurs_on(day(2026, 8, 31)), "not before it starts");
        let mut trip = event(2, "Trip", day(2026, 9, 10));
        trip.all_day = true;
        trip.end = DateTime::new(day(2026, 9, 12), Time { hour: 0, minute: 0 });
        assert!(
            trip.occurs_on(day(2026, 9, 11)),
            "an all-day event covers its days"
        );
        assert!(!trip.occurs_on(day(2026, 9, 13)));
    }

    // ---- moved from apps/calendar with the iCalendar code they test ----

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

    #[test]
    fn durations_and_repeats_are_read_as_the_standard_writes_them() {
        assert_eq!(parse_ics_duration("PT1H30M"), Some(90));
        assert_eq!(parse_ics_duration("P1D"), Some(1440));
        assert_eq!(parse_ics_duration("-P2W"), Some(-20160));
        assert_eq!(parse_ics_duration("PT45S"), Some(0));
        assert_eq!(parse_ics_duration("P1DT2H"), Some(1560));
        assert_eq!(parse_ics_duration("1H"), None);
        assert_eq!(parse_ics_duration("PT1X"), None);
        assert_eq!(parse_ics_duration("P1H"), None, "an hour needs its T");
        assert_eq!(parse_rrule("FREQ=DAILY"), (RecurrenceRule::Daily, false));
        assert_eq!(
            parse_rrule("FREQ=WEEKLY;INTERVAL=2"),
            (RecurrenceRule::BiWeekly, false)
        );
        assert_eq!(
            parse_rrule("FREQ=WEEKLY;INTERVAL=3"),
            (RecurrenceRule::Custom { interval_days: 21 }, false)
        );
        assert_eq!(
            parse_rrule("FREQ=MONTHLY;BYDAY=2TU"),
            (RecurrenceRule::Monthly, true)
        );
        assert_eq!(
            parse_rrule("FREQ=YEARLY;UNTIL=20300101"),
            (RecurrenceRule::Yearly, true)
        );
        assert_eq!(parse_rrule("FREQ=HOURLY"), (RecurrenceRule::None, true));
        assert_eq!(
            reminder_from_trigger("-PT30M"),
            Some(Reminder::MinutesBefore(30))
        );
        assert_eq!(
            reminder_from_trigger("-PT2H"),
            Some(Reminder::HoursBefore(2))
        );
        assert_eq!(reminder_from_trigger("-P1D"), Some(Reminder::DayBefore));
        assert_eq!(reminder_from_trigger("PT0S"), Some(Reminder::AtTime));
    }

    #[test]
    fn a_quoted_parameter_may_hold_a_colon() {
        let (name, params, value) =
            split_ics_line("DTSTART;TZID=\"America/New_York\";X-NOTE=\"a:b;c\":20260101T090000")
                .unwrap();
        assert_eq!(name, "DTSTART");
        assert_eq!(ics_param(&params, "TZID"), Some("America/New_York"));
        assert_eq!(ics_param(&params, "X-NOTE"), Some("a:b;c"));
        assert_eq!(value, "20260101T090000");
    }
}
