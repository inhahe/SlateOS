//! `Slate OS` Reminders & Tasks Application
//!
//! A comprehensive desktop reminders and task management application with:
//! - Task creation, editing, deletion with title, description, due date/time,
//!   priority (low/medium/high/critical), and category assignment
//! - Recurring reminders: daily, weekly, monthly, yearly, custom interval
//! - Categories: work, personal, health, finance, shopping, custom with colors
//! - Multiple views: today, upcoming (7 days), all, by category, overdue, completed
//! - Snooze support: 5min, 15min, 30min, 1hr, custom
//! - Smart sorting: by priority, due date, creation date, alphabetical
//! - Search and filter across titles and descriptions
//! - Visual notification banners when reminders are due
//! - Progress tracking for multi-step tasks with completion percentage
//! - Import/export in JSON format
//! - Multi-panel UI: sidebar (categories + views), main list, detail panel
//!
//! Uses the guitk library for UI rendering with a Catppuccin Mocha dark theme.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::many_single_char_names)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::needless_pass_by_value)]

#[allow(unused_imports)]
use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent};
use guitk::render::RenderTree;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
// The shared civil-date arithmetic. This app used to carry its own copy of
// all of it: a Zeller's congruence for the weekday, a *separate* Julian day
// number for differences, its own leap rule, and month-stepping `while` loops
// in `add_days`. Two unrelated day-numbering schemes in one struct that had
// to agree with each other by coincidence, and five apps besides this one
// with their own incompatible versions. See `known-issues.md`
// C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC.
use guitk::date;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::text;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 720.0;
const SIDEBAR_WIDTH: f32 = 220.0;
const DETAIL_PANEL_WIDTH: f32 = 300.0;
const HEADER_HEIGHT: f32 = 56.0;
const PADDING: f32 = 12.0;
const ITEM_HEIGHT: f32 = 72.0;
const NOTIFICATION_HEIGHT: f32 = 48.0;
const CORNER_RADIUS: f32 = 8.0;
const SMALL_RADIUS: f32 = 4.0;

/// Point size of the prose fields (description, notes) in the detail panel.
const PROSE_FONT_SIZE: f32 = 12.0;
/// Line spacing of those fields.
const PROSE_LINE_HEIGHT: f32 = 18.0;
/// Space left under a prose field before the next one starts. Sized so that a
/// one-line field occupies the 24px it always has.
const PROSE_FIELD_GAP: f32 = 6.0;

/// A prose field of the detail panel — a description or a set of notes.
///
/// Shared by the fields rather than written out at each, so that they cannot
/// drift into laying their text out differently from one another.
fn detail_prose(text: &str, x: f32, y: f32, width: f32) -> text::Paragraph<'_> {
    text::Paragraph::new(text, SUBTEXT0)
        .at(x, y, width)
        .font(PROSE_FONT_SIZE, FontWeightHint::Regular)
        .line_height(PROSE_LINE_HEIGHT)
}

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
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        if !(1..=12).contains(&month) {
            return None;
        }
        let max_day = days_in_month(year, month);
        if !(1..=max_day).contains(&day) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// This date in the shared calendar, for arithmetic.
    ///
    /// Private, and paired with [`from_civil`](Self::from_civil): the struct
    /// itself stays, because its three public fields are read directly all
    /// over the UI, but nothing computes on them any more.
    fn civil(self) -> date::Date {
        date::Date::from_ymd(self.year, self.month, self.day)
    }

    fn from_civil(d: date::Date) -> Self {
        let (year, month, day) = d.ymd();
        Self { year, month, day }
    }

    /// The weekday, as the shared enum.
    pub fn weekday(self) -> date::Weekday {
        self.civil().weekday()
    }

    /// Day of week: 0=Sunday, 1=Monday, ..., 6=Saturday.
    ///
    /// Was a hand-written Zeller's congruence, which is correct for years >= 1
    /// and wrong below that: `y % 100` and `y / 100` truncate toward zero in
    /// Rust, not the flooring the formula assumes, and nothing stopped a
    /// caller building such a date. `Weekday::from_index` uses `rem_euclid`
    /// and has no such range.
    pub fn day_of_week(self) -> u32 {
        u32::try_from(self.weekday().index()).unwrap_or(0)
    }

    /// The weekday's full name.
    ///
    /// The `_ => "Unknown"` arm this replaced could not fire, but it was
    /// there, untestable, in three apps. Matching on the enum removes the arm
    /// rather than leaving a dead branch that reads like a real fallback.
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

    /// Add days (positive or negative).
    ///
    /// Was a pair of `while` loops that stepped one month at a time, so the
    /// cost was proportional to the distance moved and a large `n` walked
    /// thousands of iterations. The shared version converts to a day count,
    /// adds, and converts back.
    pub fn add_days(self, n: i32) -> Self {
        Self::from_civil(self.civil().add_days(n))
    }

    /// Add months, clamping the day into the target month: 31 January plus a
    /// month is 28 February. Not reversible, which is inherent to the clamp.
    pub fn add_months(self, n: i32) -> Self {
        Self::from_civil(self.civil().add_months(n))
    }

    /// Difference in days between two dates (`self - other`).
    ///
    /// No longer "approximate", and no longer computed from a Julian day
    /// number this struct maintained *separately* from the Zeller congruence
    /// it used for weekdays. Both truncated toward zero on negative years,
    /// where the formulas need flooring.
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

    pub fn format_medium(self) -> String {
        format!("{} {} {}", self.day, self.month_short(), self.year)
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

    pub fn to_minutes(self) -> u32 {
        // Saturating rather than wrapping: an hour past 23 is bad data, and
        // a wrapped minute-of-day sorts a task to the wrong end of the list.
        self.hour.saturating_mul(60).saturating_add(self.minute)
    }

    pub fn from_minutes(total: u32) -> Self {
        Self {
            hour: (total / 60).min(23),
            minute: total % 60,
        }
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

    pub fn format_short(self) -> String {
        format!("{} {}", self.date.format_short(), self.time.format_12h())
    }

    pub fn format_medium(self) -> String {
        format!(
            "{} at {}",
            self.date.format_medium(),
            self.time.format_12h()
        )
    }

    /// Difference in minutes (approximate, same-month only for simplicity).
    pub fn minutes_since(self, other: Self) -> i64 {
        let day_diff = self.date.days_since(other.date);
        let minute_diff =
            i64::from(self.time.to_minutes()).saturating_sub(i64::from(other.time.to_minutes()));
        day_diff.saturating_mul(1440).saturating_add(minute_diff)
    }
}

// ============================================================================
// Date helper functions
// ============================================================================

// These delegate to `guitk::date` rather than restating it. One behaviour
// change comes with that, and it is an improvement: an out-of-range month is
// **clamped** into 1..=12 instead of yielding `0` / `"Unknown"` / `"???"`. A
// `0` from `days_in_month` was a live loop-termination hazard — the old
// `while d > days_in_month(y, m)` loop in `add_days` would have spun forever
// on one — and every caller here passes a month that came from a validated
// `Date`, so the clamp is unreachable in practice and merely stops being a
// trap for the next caller.

pub fn is_leap_year(year: i32) -> bool {
    date::is_leap_year(year)
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    date::days_in_month(year, month)
}

pub fn month_name(month: u32) -> &'static str {
    date::month_name(month)
}

pub fn month_short(month: u32) -> &'static str {
    date::month_short_name(month)
}

// ============================================================================
// Priority
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Low => "[-]",
            Self::Medium => "[=]",
            Self::High => "[!]",
            Self::Critical => "[!!]",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Low => OVERLAY0,
            Self::Medium => BLUE,
            Self::High => PEACH,
            Self::Critical => RED,
        }
    }

    /// Numeric weight for sorting (higher = more urgent).
    pub fn weight(self) -> u32 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Low, Self::Medium, Self::High, Self::Critical]
    }

    pub fn from_str_label(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

// ============================================================================
// Task category
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskCategory {
    Work,
    Personal,
    Health,
    Finance,
    Shopping,
    Education,
    Home,
    Social,
}

impl TaskCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Work => "Work",
            Self::Personal => "Personal",
            Self::Health => "Health",
            Self::Finance => "Finance",
            Self::Shopping => "Shopping",
            Self::Education => "Education",
            Self::Home => "Home",
            Self::Social => "Social",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Work => "[W]",
            Self::Personal => "[P]",
            Self::Health => "[H]",
            Self::Finance => "[$]",
            Self::Shopping => "[S]",
            Self::Education => "[E]",
            Self::Home => "[~]",
            Self::Social => "[@]",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Work => BLUE,
            Self::Personal => GREEN,
            Self::Health => RED,
            Self::Finance => YELLOW,
            Self::Shopping => PEACH,
            Self::Education => SKY,
            Self::Home => TEAL,
            Self::Social => MAUVE,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Work,
            Self::Personal,
            Self::Health,
            Self::Finance,
            Self::Shopping,
            Self::Education,
            Self::Home,
            Self::Social,
        ]
    }

    pub fn from_str_label(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "work" => Some(Self::Work),
            "personal" => Some(Self::Personal),
            "health" => Some(Self::Health),
            "finance" => Some(Self::Finance),
            "shopping" => Some(Self::Shopping),
            "education" => Some(Self::Education),
            "home" => Some(Self::Home),
            "social" => Some(Self::Social),
            _ => None,
        }
    }
}

// ============================================================================
// Recurrence rule
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecurrenceRule {
    None,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Custom { interval_days: u32 },
}

impl RecurrenceRule {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "Does not repeat",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Yearly => "Yearly",
            Self::Custom { .. } => "Custom interval",
        }
    }

    /// Generate next occurrence date after `from`.
    pub fn next_occurrence(&self, from: Date) -> Option<Date> {
        match self {
            Self::None => None,
            Self::Daily => Some(from.add_days(1)),
            Self::Weekly => Some(from.add_days(7)),
            Self::Monthly => Some(from.add_months(1)),
            Self::Yearly => Some(from.add_months(12)),
            Self::Custom { interval_days } => {
                if *interval_days == 0 {
                    return None;
                }
                Some(from.add_days(*interval_days as i32))
            }
        }
    }

    /// Check if a recurrence matches a given date from a starting origin date.
    pub fn matches(&self, origin: Date, check: Date) -> bool {
        if check < origin {
            return false;
        }
        match self {
            Self::None => origin == check,
            Self::Daily => true,
            Self::Weekly => {
                let diff = check.days_since(origin);
                diff >= 0 && diff % 7 == 0
            }
            Self::Monthly => origin.day == check.day && check >= origin,
            Self::Yearly => {
                origin.month == check.month && origin.day == check.day && check >= origin
            }
            Self::Custom { interval_days } => {
                if *interval_days == 0 {
                    return origin == check;
                }
                let diff = check.days_since(origin);
                // `checked_rem` although the zero case is handled above:
                // the guard and the division are three lines apart, and this
                // is what keeps them from drifting.
                diff >= 0 && diff.checked_rem(i64::from(*interval_days)) == Some(0)
            }
        }
    }

    pub fn all_presets() -> Vec<Self> {
        vec![
            Self::None,
            Self::Daily,
            Self::Weekly,
            Self::Monthly,
            Self::Yearly,
            Self::Custom { interval_days: 3 },
        ]
    }

    /// Serialize to a simple string for JSON export.
    pub fn to_json_str(&self) -> String {
        match self {
            Self::None => "none".to_string(),
            Self::Daily => "daily".to_string(),
            Self::Weekly => "weekly".to_string(),
            Self::Monthly => "monthly".to_string(),
            Self::Yearly => "yearly".to_string(),
            Self::Custom { interval_days } => format!("custom:{interval_days}"),
        }
    }

    /// Parse from a simple string.
    pub fn from_json_str(s: &str) -> Self {
        match s {
            "none" => Self::None,
            "daily" => Self::Daily,
            "weekly" => Self::Weekly,
            "monthly" => Self::Monthly,
            "yearly" => Self::Yearly,
            other => {
                if let Some(rest) = other.strip_prefix("custom:")
                    && let Ok(days) = rest.parse::<u32>()
                {
                    return Self::Custom {
                        interval_days: days,
                    };
                }
                Self::None
            }
        }
    }
}

// ============================================================================
// Snooze duration
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnoozeDuration {
    Minutes5,
    Minutes15,
    Minutes30,
    Hour1,
    Custom { minutes: u32 },
}

impl SnoozeDuration {
    pub fn label(self) -> String {
        match self {
            Self::Minutes5 => "5 minutes".to_string(),
            Self::Minutes15 => "15 minutes".to_string(),
            Self::Minutes30 => "30 minutes".to_string(),
            Self::Hour1 => "1 hour".to_string(),
            Self::Custom { minutes } => format!("{minutes} minutes"),
        }
    }

    pub fn as_minutes(self) -> u32 {
        match self {
            Self::Minutes5 => 5,
            Self::Minutes15 => 15,
            Self::Minutes30 => 30,
            Self::Hour1 => 60,
            Self::Custom { minutes } => minutes,
        }
    }

    pub fn presets() -> &'static [Self] {
        &[
            Self::Minutes5,
            Self::Minutes15,
            Self::Minutes30,
            Self::Hour1,
        ]
    }
}

// ============================================================================
// Subtask (for progress tracking)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtask {
    pub title: String,
    pub completed: bool,
}

impl Subtask {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            completed: false,
        }
    }
}

// ============================================================================
// Task / Reminder
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub due: Option<DateTime>,
    pub created: DateTime,
    pub priority: Priority,
    pub category: TaskCategory,
    pub recurrence: RecurrenceRule,
    pub completed: bool,
    pub completed_at: Option<DateTime>,
    pub snoozed_until: Option<DateTime>,
    pub subtasks: Vec<Subtask>,
    pub notes: String,
}

impl Task {
    pub fn new(id: u64, title: &str, created: DateTime) -> Self {
        Self {
            id,
            title: title.to_string(),
            description: String::new(),
            due: None,
            created,
            priority: Priority::Medium,
            category: TaskCategory::Personal,
            recurrence: RecurrenceRule::None,
            completed: false,
            completed_at: None,
            snoozed_until: None,
            subtasks: Vec::new(),
            notes: String::new(),
        }
    }

    /// Calculate completion percentage based on subtasks.
    /// Returns 100 if the task itself is completed, or the subtask ratio otherwise.
    pub fn completion_percent(&self) -> u32 {
        if self.completed {
            return 100;
        }
        if self.subtasks.is_empty() {
            return 0;
        }
        let done = self.subtasks.iter().filter(|s| s.completed).count() as u32;
        let total = self.subtasks.len() as u32;
        done.saturating_mul(100).checked_div(total).unwrap_or(0)
    }

    /// Check if this task is overdue relative to `now`.
    pub fn is_overdue(&self, now: DateTime) -> bool {
        if self.completed {
            return false;
        }
        if let Some(due) = self.due {
            due < now
        } else {
            false
        }
    }

    /// Check if this task is due today.
    pub fn is_due_today(&self, today: Date) -> bool {
        if self.completed {
            return false;
        }
        if let Some(due) = self.due {
            due.date == today
        } else {
            false
        }
    }

    /// Check if due within N days from `today`.
    pub fn is_due_within(&self, today: Date, days: i32) -> bool {
        if self.completed {
            return false;
        }
        if let Some(due) = self.due {
            let diff = due.date.days_since(today);
            (0..=i64::from(days)).contains(&diff)
        } else {
            false
        }
    }

    /// Check if the reminder is currently snoozed.
    pub fn is_snoozed(&self, now: DateTime) -> bool {
        if let Some(until) = self.snoozed_until {
            now < until
        } else {
            false
        }
    }

    /// Apply snooze from `now`.
    pub fn snooze(&mut self, now: DateTime, duration: SnoozeDuration) {
        let total_minutes = now.time.to_minutes().saturating_add(duration.as_minutes());
        let extra_days = total_minutes / 1440;
        let remaining = total_minutes % 1440;
        // A snooze that would push the date past `i32` days is not a snooze;
        // clamping keeps it in the far future rather than wrapping to the past.
        let new_date = now
            .date
            .add_days(i32::try_from(extra_days).unwrap_or(i32::MAX));
        let new_time = Time::from_minutes(remaining);
        self.snoozed_until = Some(DateTime::new(new_date, new_time));
    }

    /// Check if the task matches a search query (case-insensitive, searches
    /// title and description).
    pub fn matches_query(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let lower = query.to_lowercase();
        self.title.to_lowercase().contains(&lower)
            || self.description.to_lowercase().contains(&lower)
    }

    /// Produce a human-readable due date label relative to `today`.
    pub fn due_label(&self, today: Date) -> String {
        if let Some(due) = self.due {
            let diff = due.date.days_since(today);
            if diff == 0 {
                format!("Today at {}", due.time.format_12h())
            } else if diff == 1 {
                format!("Tomorrow at {}", due.time.format_12h())
            } else if diff == -1 {
                format!("Yesterday at {}", due.time.format_12h())
            } else if diff < -1 {
                format!("{} days ago", diff.saturating_neg())
            } else if diff <= 7 {
                format!(
                    "{} at {}",
                    due.date.day_of_week_name(),
                    due.time.format_12h()
                )
            } else {
                due.format_medium()
            }
        } else {
            "No due date".to_string()
        }
    }

    /// One task as the object [`TaskStore::export_json`] writes and
    /// [`TaskStore::import_json`] reads back.
    ///
    /// This is a round trip, not a one-way export, so a field missing here is
    /// a field the user loses by exporting and re-importing. Three were:
    /// `created`, `completed_at` and `snoozed_until` were written nowhere, so
    /// an import stamped every task as created at the moment of import — which
    /// silently reorders [`SortMode::CreationDate`] — forgot when anything was
    /// completed, and un-snoozed every snoozed task, so a reminder deliberately
    /// pushed to next week fired on the spot.
    ///
    /// `id` is the one field written but deliberately not restored:
    /// [`TaskStore::add`] assigns from its own counter, because an imported id
    /// may already belong to a task in the store. It is written anyway because
    /// `import_json` finds task objects by scanning for `{"id":`.
    pub fn to_json(&self) -> String {
        // Exhaustive (no `..`), so a new `Task` field stops this compiling
        // until someone decides whether it survives an export. Nothing else
        // would have: the reader builds its `Task` with `Task::new` plus
        // assignments rather than a struct literal, so a new field errors
        // on neither side. See known-issues.md lesson 44.
        let Self {
            id,
            title,
            description,
            due,
            created,
            priority,
            category,
            recurrence,
            completed,
            completed_at,
            snoozed_until,
            subtasks,
            notes,
        } = self;

        // `null` rather than an absent key for the three optional datetimes,
        // so that "no due date" is a recorded fact and not an inference from
        // silence. The reader treats both the same, but a file a human reads
        // should say what it means.
        let opt_dt = |d: &Option<DateTime>| match d {
            Some(d) => format!("\"{}\"", d.format_short()),
            None => "null".to_string(),
        };
        let due_str = opt_dt(due);
        let completed_at_str = opt_dt(completed_at);
        let snoozed_until_str = opt_dt(snoozed_until);
        let subtask_json: Vec<String> = subtasks
            .iter()
            .map(|s| {
                format!(
                    "{{\"title\":\"{}\",\"completed\":{}}}",
                    escape_json(&s.title),
                    s.completed
                )
            })
            .collect();

        format!(
            "{{\"id\":{},\"title\":\"{}\",\"description\":\"{}\",\"due\":{},\
             \"created\":\"{}\",\"priority\":\"{}\",\"category\":\"{}\",\
             \"recurrence\":\"{}\",\"completed\":{},\"completed_at\":{},\
             \"snoozed_until\":{},\"subtasks\":[{}],\"notes\":\"{}\"}}",
            id,
            escape_json(title),
            escape_json(description),
            due_str,
            created.format_short(),
            priority.label(),
            category.label(),
            recurrence.to_json_str(),
            completed,
            completed_at_str,
            snoozed_until_str,
            subtask_json.join(","),
            escape_json(notes),
        )
    }
}

/// Escape a string for the body of a JSON string literal.
///
/// The previous local version was a chain of `str::replace` calls that left
/// every control character except `\n`, `\r` and `\t` raw in the output. A raw
/// control character inside a JSON string is invalid per RFC 8259, so a task
/// whose notes contained one produced a file this app could not reload.
fn escape_json(s: &str) -> String {
    guitk::escape::json_string(s)
}

/// Decode the body of a JSON string literal, reversing [`escape_json`].
///
/// The previous local version was a chain of `str::replace` calls applied in
/// the wrong order: `\\n` was decoded before `\\\\`, so the two-character text
/// `\n` — a literal backslash followed by the letter n — was written as `\\n`
/// and read back as a *newline*. A Windows path in a note (`C:\temp`) came
/// back as `C:\<TAB>emp`, and the damage was re-saved, compounding on every
/// open. The shared decoder is a single left-to-right pass, which structurally
/// cannot re-examine what it has already decoded.
fn unescape_json(s: &str) -> String {
    guitk::escape::unescape_json_string(s)
}

// ============================================================================
// Sort mode
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    DueDate,
    Priority,
    CreationDate,
    Alphabetical,
}

impl SortMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::DueDate => "Due Date",
            Self::Priority => "Priority",
            Self::CreationDate => "Created",
            Self::Alphabetical => "A-Z",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::DueDate,
            Self::Priority,
            Self::CreationDate,
            Self::Alphabetical,
        ]
    }

    pub fn next(self) -> Self {
        match self {
            Self::DueDate => Self::Priority,
            Self::Priority => Self::CreationDate,
            Self::CreationDate => Self::Alphabetical,
            Self::Alphabetical => Self::DueDate,
        }
    }
}

// ============================================================================
// View filter
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewFilter {
    Today,
    Upcoming,
    All,
    Overdue,
    Completed,
    ByCategory(TaskCategory),
}

impl ViewFilter {
    pub fn label(self) -> String {
        match self {
            Self::Today => "Today".to_string(),
            Self::Upcoming => "Upcoming (7 days)".to_string(),
            Self::All => "All Tasks".to_string(),
            Self::Overdue => "Overdue".to_string(),
            Self::Completed => "Completed".to_string(),
            Self::ByCategory(cat) => cat.label().to_string(),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Today => "[*]",
            Self::Upcoming => "[>]",
            Self::All => "[#]",
            Self::Overdue => "[!]",
            Self::Completed => "[v]",
            Self::ByCategory(cat) => cat.icon(),
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Today => BLUE,
            Self::Upcoming => TEAL,
            Self::All => LAVENDER,
            Self::Overdue => RED,
            Self::Completed => GREEN,
            Self::ByCategory(cat) => cat.color(),
        }
    }

    /// Standard views (non-category).
    pub fn standard_views() -> &'static [Self] {
        &[
            Self::Today,
            Self::Upcoming,
            Self::All,
            Self::Overdue,
            Self::Completed,
        ]
    }
}

// ============================================================================
// Notification
// ============================================================================

#[derive(Debug, Clone)]
pub struct Notification {
    pub task_id: u64,
    pub message: String,
    pub triggered_at: DateTime,
    pub dismissed: bool,
}

// ============================================================================
// Task store
// ============================================================================

pub struct TaskStore {
    tasks: Vec<Task>,
    next_id: u64,
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStore {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
        }
    }

    pub fn add(&mut self, mut task: Task) -> u64 {
        let id = self.next_id;
        // Saturating: a wrapped counter hands out an id that already exists,
        // and both selection and completion are by id.
        self.next_id = self.next_id.saturating_add(1);
        task.id = id;
        self.tasks.push(task);
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        self.tasks.len() < before
    }

    pub fn get(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn all(&self) -> &[Task] {
        &self.tasks
    }

    /// Filter tasks by the given view, relative to `now`.
    pub fn filtered(&self, view: ViewFilter, now: DateTime) -> Vec<&Task> {
        let today = now.date;
        match view {
            ViewFilter::Today => self
                .tasks
                .iter()
                .filter(|t| !t.completed && t.is_due_today(today))
                .collect(),
            ViewFilter::Upcoming => self
                .tasks
                .iter()
                .filter(|t| !t.completed && t.is_due_within(today, 7))
                .collect(),
            ViewFilter::All => self.tasks.iter().filter(|t| !t.completed).collect(),
            ViewFilter::Overdue => self.tasks.iter().filter(|t| t.is_overdue(now)).collect(),
            ViewFilter::Completed => self.tasks.iter().filter(|t| t.completed).collect(),
            ViewFilter::ByCategory(cat) => self
                .tasks
                .iter()
                .filter(|t| !t.completed && t.category == cat)
                .collect(),
        }
    }

    /// Sort a list of task references.
    pub fn sorted<'a>(tasks: &[&'a Task], mode: SortMode) -> Vec<&'a Task> {
        let mut result: Vec<&Task> = tasks.to_vec();
        match mode {
            SortMode::DueDate => result.sort_by_key(|t| t.due),
            SortMode::Priority => result.sort_by_key(|t| std::cmp::Reverse(t.priority.weight())),
            SortMode::CreationDate => result.sort_by_key(|t| t.created),
            SortMode::Alphabetical => result.sort_by_key(|t| t.title.to_lowercase()),
        }
        result
    }

    /// Search tasks by title/description.
    pub fn search(&self, query: &str) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.matches_query(query))
            .collect()
    }

    /// Count tasks by category.
    pub fn count_by_category(&self, cat: TaskCategory) -> usize {
        self.tasks
            .iter()
            .filter(|t| !t.completed && t.category == cat)
            .count()
    }

    /// Count overdue tasks.
    pub fn count_overdue(&self, now: DateTime) -> usize {
        self.tasks.iter().filter(|t| t.is_overdue(now)).count()
    }

    /// Count tasks due today.
    pub fn count_today(&self, today: Date) -> usize {
        self.tasks.iter().filter(|t| t.is_due_today(today)).count()
    }

    /// Count completed tasks.
    pub fn count_completed(&self) -> usize {
        self.tasks.iter().filter(|t| t.completed).count()
    }

    /// Get tasks that should trigger a notification now.
    pub fn due_now(&self, now: DateTime) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| {
                if t.completed {
                    return false;
                }
                if t.is_snoozed(now) {
                    return false;
                }
                if let Some(due) = t.due {
                    // Due within the last 5 minutes (notification window)
                    let diff = now.minutes_since(due);
                    (0..=5).contains(&diff)
                } else {
                    false
                }
            })
            .collect()
    }

    /// Export all tasks as JSON.
    pub fn export_json(&self) -> String {
        let items: Vec<String> = self.tasks.iter().map(Task::to_json).collect();
        format!("{{\"tasks\":[{}]}}", items.join(","))
    }

    /// Import tasks from a JSON-like string. Returns number of tasks imported.
    /// This is a simplified parser for our own export format.
    pub fn import_json(&mut self, json: &str, now: DateTime) -> usize {
        let mut count = 0usize;
        // Simple approach: find task objects by splitting on boundaries
        let mut remaining = json;
        while let Some(start) = remaining.find("{\"id\":") {
            remaining = &remaining[start..];
            // Find matching closing brace (simplified — no nested objects except subtasks)
            if let Some(end) = find_matching_brace(remaining) {
                let obj = &remaining[..=end];
                if let Some(task) = parse_task_json(obj, now) {
                    self.add(task);
                    count = count.saturating_add(1);
                }
                // Past the object just consumed. `get` and `checked_add`
                // because `end` comes from scanning a document the user
                // supplied: a brace at the very last byte makes `end + 1`
                // the length, which is a valid empty slice, and anything
                // beyond it is a malformed file rather than a panic.
                let Some(rest) = end.checked_add(1).and_then(|n| remaining.get(n..)) else {
                    break;
                };
                remaining = rest;
            } else {
                break;
            }
        }
        count
    }

    /// Complete a task by ID. Returns true if found.
    pub fn complete_task(&mut self, id: u64, now: DateTime) -> bool {
        if let Some(task) = self.get_mut(id) {
            task.completed = true;
            task.completed_at = Some(now);
            true
        } else {
            false
        }
    }

    /// Uncomplete a task by ID. Returns true if found.
    pub fn uncomplete_task(&mut self, id: u64) -> bool {
        if let Some(task) = self.get_mut(id) {
            task.completed = false;
            task.completed_at = None;
            true
        } else {
            false
        }
    }

    /// Toggle a subtask's completion. Returns the new state or None if not found.
    pub fn toggle_subtask(&mut self, task_id: u64, subtask_idx: usize) -> Option<bool> {
        let task = self.get_mut(task_id)?;
        let subtask = task.subtasks.get_mut(subtask_idx)?;
        subtask.completed = !subtask.completed;
        Some(subtask.completed)
    }

    /// Add a subtask to a task. Returns true if the parent task exists.
    pub fn add_subtask(&mut self, task_id: u64, title: &str) -> bool {
        if let Some(task) = self.get_mut(task_id) {
            task.subtasks.push(Subtask::new(title));
            true
        } else {
            false
        }
    }

    /// Remove a subtask from a task. Returns true if successful.
    pub fn remove_subtask(&mut self, task_id: u64, subtask_idx: usize) -> bool {
        if let Some(task) = self.get_mut(task_id)
            && subtask_idx < task.subtasks.len()
        {
            task.subtasks.remove(subtask_idx);
            return true;
        }
        false
    }
}

// ============================================================================
// JSON parsing helpers
// ============================================================================

/// Find the index of the matching closing brace for a JSON object starting at
/// position 0 in the input.
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '{' {
            depth = depth.saturating_add(1);
        } else if ch == '}' {
            // `checked_sub`, not `-`: a JSON document with more closing braces
            // than opening ones is malformed input, and `?` reports it as the
            // `None` this function already returns for an unbalanced document.
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Scan the JSON string literal starting at `quote`.
///
/// Returns its still-escaped contents and the byte index just past the closing
/// quote. Every scan in this module goes through here so that a `{`, `[` or
/// `"` *inside* a string cannot be mistaken for structure — a note reading
/// `see {the thing}` is text, not an object.
fn scan_json_string(json: &str, quote: usize) -> Option<(&str, usize)> {
    let b = json.as_bytes();
    if b.get(quote) != Some(&b'"') {
        return None;
    }
    let start = quote.checked_add(1)?;
    let mut i = start;
    while let Some(&c) = b.get(i) {
        match c {
            // The escape's second byte is always ASCII (`"\/bfnrtu`), so
            // stepping over it cannot land inside a multi-byte character.
            b'\\' => i = i.checked_add(2)?,
            b'"' => return Some((json.get(start..i)?, i.checked_add(1)?)),
            _ => i = i.checked_add(1)?,
        }
    }
    None
}

/// The raw text of a **top-level** key's value in a JSON object.
///
/// The "top-level" is the entire point. This used to be a pair of helpers that
/// searched the object with `str::find` and `str::contains`, and neither can
/// tell an outer key from one nested inside it. A task carries its subtasks in
/// the same object, and a subtask has its own `"completed"` key, so
/// `"completed":true` on *any* subtask reported the parent task complete:
/// tick one step off a task, export, re-import, and the whole task was done
/// and gone from the active list. `contains` made it worse than a
/// first-match-wins scan would have, because it tested for `true` before
/// `false` and so ignored position entirely — the parent's own
/// `"completed":false` sat earlier in the very same object and lost.
///
/// Depth tracking is what stops a nested key answering for an outer one.
/// Returns the value's source text — `"quoted"` with its quotes, or `null`,
/// `true`, `123` as written — leaving interpretation to the caller.
fn json_top_level_value<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let b = json.as_bytes();
    // Step into the object; everything before its `{` is not ours to read.
    let mut i = json.find('{')?.checked_add(1)?;
    let mut depth = 1usize;
    while let Some(&c) = b.get(i) {
        match c {
            b'"' => {
                let (text, after) = scan_json_string(json, i)?;
                // A string followed by `:` is a key, and only a key at depth 1
                // belongs to this object rather than to something inside it.
                let mut j = after;
                while b.get(j).is_some_and(u8::is_ascii_whitespace) {
                    j = j.checked_add(1)?;
                }
                if depth == 1 && b.get(j) == Some(&b':') {
                    j = j.checked_add(1)?;
                    while b.get(j).is_some_and(u8::is_ascii_whitespace) {
                        j = j.checked_add(1)?;
                    }
                    // Keys in this format are plain identifiers, so comparing
                    // the escaped text is the same as comparing the decoded
                    // text. A key needing an escape would not match, which is
                    // the safe direction: absent, not wrong.
                    if text == key {
                        return json_value_slice(json, j);
                    }
                    i = j;
                } else {
                    i = after;
                }
            }
            b'{' | b'[' => {
                depth = depth.checked_add(1)?;
                i = i.checked_add(1)?;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    // The object ended without the key.
                    return None;
                }
                i = i.checked_add(1)?;
            }
            _ => i = i.checked_add(1)?,
        }
    }
    None
}

/// The source text of one JSON value beginning at `start`.
///
/// Ends at the `,` or closing bracket that belongs to the enclosing object,
/// stepping over nested structures and strings so neither can end it early.
fn json_value_slice(json: &str, start: usize) -> Option<&str> {
    let b = json.as_bytes();
    let mut i = start;
    let mut depth = 0usize;
    while let Some(&c) = b.get(i) {
        match c {
            b'"' => i = scan_json_string(json, i)?.1,
            b'{' | b'[' => {
                depth = depth.checked_add(1)?;
                i = i.checked_add(1)?;
            }
            b'}' | b']' => {
                if depth == 0 {
                    break;
                }
                depth = depth.checked_sub(1)?;
                i = i.checked_add(1)?;
            }
            b',' if depth == 0 => break,
            _ => i = i.checked_add(1)?,
        }
    }
    Some(json.get(start..i)?.trim())
}

/// Extract a JSON string value for a top-level key.
///
/// Returns the still-escaped contents; callers pass it through
/// [`unescape_json`]. A key whose value is `null` (or any non-string) reads as
/// absent, which is what makes `"due":null` and a missing `due` mean the same
/// thing to the reader.
fn json_string_value<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let raw = json_top_level_value(json, key)?;
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner)
}

/// Extract a JSON boolean value for a top-level key.
fn json_bool_value(json: &str, key: &str) -> Option<bool> {
    match json_top_level_value(json, key)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Parse a task from our JSON export format.
fn parse_task_json(json: &str, default_created: DateTime) -> Option<Task> {
    let title = json_string_value(json, "title")?;
    let description = json_string_value(json, "description").unwrap_or("");
    let priority_str = json_string_value(json, "priority").unwrap_or("Medium");
    let category_str = json_string_value(json, "category").unwrap_or("Personal");
    let recurrence_str = json_string_value(json, "recurrence").unwrap_or("none");
    let completed = json_bool_value(json, "completed").unwrap_or(false);
    let notes = json_string_value(json, "notes").unwrap_or("");

    let priority = Priority::from_str_label(priority_str).unwrap_or(Priority::Medium);
    let category = TaskCategory::from_str_label(category_str).unwrap_or(TaskCategory::Personal);
    let recurrence = RecurrenceRule::from_json_str(recurrence_str);

    // Parse due date if present (format: "YYYY-MM-DD H:MM AM/PM")
    let due = json_string_value(json, "due").and_then(parse_datetime_short);

    // Parse subtasks array
    let subtasks = parse_subtasks_json(json);

    // Absent keys fall back rather than failing, which is what lets a file
    // exported before `created`/`completed_at`/`snoozed_until` were written
    // still import: an old task gets the import time for its creation date,
    // exactly as every task did before. `default_created` is the caller's
    // "now" and is only reached for such a file.
    let created = json_string_value(json, "created")
        .and_then(parse_datetime_short)
        .unwrap_or(default_created);
    let completed_at = json_string_value(json, "completed_at").and_then(parse_datetime_short);
    let snoozed_until = json_string_value(json, "snoozed_until").and_then(parse_datetime_short);

    // A struct literal rather than `Task::new` plus a run of assignments, so
    // that a new `Task` field is `E0063` here and cannot be quietly dropped on
    // import -- the counterpart of the destructure in `Task::to_json`, and the
    // reason both sides of this format now fail loudly instead of one.
    Some(Task {
        // Not read from the file: `TaskStore::add` assigns the id, because an
        // imported one may already be in use. See `Task::to_json`.
        id: 0,
        title: unescape_json(title),
        description: unescape_json(description),
        due,
        created,
        priority,
        category,
        recurrence,
        completed,
        completed_at,
        snoozed_until,
        subtasks,
        notes: unescape_json(notes),
    })
}

/// Parse a short datetime string like "2026-05-18 3:00 PM".
fn parse_datetime_short(s: &str) -> Option<DateTime> {
    let parts: Vec<&str> = s.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return None;
    }
    // Every index here is reachable from an imported file, so each one is a
    // `get`: this parses `import_json`'s input, which is a document the user
    // supplies and may well be truncated or hand-edited.
    let date_parts: Vec<&str> = parts.first()?.split('-').collect();
    let year = date_parts.first()?.parse::<i32>().ok()?;
    let month = date_parts.get(1)?.parse::<u32>().ok()?;
    let day = date_parts.get(2)?.parse::<u32>().ok()?;
    let date = Date::new(year, month, day)?;

    let time_str = parts.get(1)?.trim();
    let time = parse_time_12h(time_str)?;

    Some(DateTime::new(date, time))
}

/// Parse a 12-hour time string like "3:00 PM" or "12:30 AM".
fn parse_time_12h(s: &str) -> Option<Time> {
    let s = s.trim();
    let is_pm = s.ends_with("PM");
    let is_am = s.ends_with("AM");
    if !is_pm && !is_am {
        return None;
    }
    // `"PM"` is two bytes and `ends_with` proved they are there, but the
    // subtraction and the slice are separate statements that a later edit can
    // separate further.
    let time_part = s.get(..s.len().saturating_sub(2))?.trim();
    let colon = time_part.find(':')?;
    let hour_raw = time_part.get(..colon)?.parse::<u32>().ok()?;
    let minute = time_part
        .get(colon.checked_add(1)?..)?
        .parse::<u32>()
        .ok()?;

    let hour = if is_am {
        if hour_raw == 12 { 0 } else { hour_raw }
    } else if hour_raw == 12 {
        12
    } else {
        // `Time::new` rejects anything past 23, so an hour like `19 PM` in an
        // imported file is refused rather than wrapped into the small hours.
        hour_raw.saturating_add(12)
    };

    Time::new(hour, minute)
}

/// Parse subtasks from within a task JSON object.
fn parse_subtasks_json(json: &str) -> Vec<Subtask> {
    let mut result = Vec::new();

    // The array's text, located by the same depth-aware scan every other key
    // uses. This was `json.find("\"subtasks\":[")` followed by the first `]`
    // after it, which is two separate ways to get the wrong text: the marker
    // matches inside a note that quotes one, and a `]` in a subtask's own
    // title ended the array early -- a step called "Buy milk [urgent]"
    // silently truncated the list after itself. It only worked at all because
    // `notes` happens to be written after `subtasks`, which is an ordering
    // nothing enforced.
    let Some(raw) = json_top_level_value(json, "subtasks") else {
        return result;
    };
    let Some(inner) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return result;
    };

    let mut remaining = inner.trim_start();
    while !remaining.is_empty() {
        // `find_matching_brace` tracks string state, so a `}` inside a title
        // does not end the object either.
        let Some(brace_end) = find_matching_brace(remaining) else {
            break;
        };
        let Some(obj) = remaining.get(..=brace_end) else {
            break;
        };
        result.push(Subtask {
            title: unescape_json(json_string_value(obj, "title").unwrap_or("")),
            completed: json_bool_value(obj, "completed").unwrap_or(false),
        });
        let Some(rest) = brace_end.checked_add(1).and_then(|n| remaining.get(n..)) else {
            break;
        };
        remaining = rest.trim_start().trim_start_matches(',').trim_start();
    }
    result
}

// ============================================================================
// Reminders application state
// ============================================================================

pub struct RemindersApp {
    pub width: f32,
    pub height: f32,
    pub today: Date,
    pub now: DateTime,
    pub store: TaskStore,
    pub view: ViewFilter,
    pub sort_mode: SortMode,
    pub search_query: String,
    pub selected_task_id: Option<u64>,
    pub notifications: Vec<Notification>,
    pub sidebar_visible: bool,
    pub detail_visible: bool,
    pub show_completed_subtasks: bool,
}

impl RemindersApp {
    pub fn new(width: f32, height: f32, now: DateTime) -> Self {
        Self {
            width,
            height,
            today: now.date,
            now,
            store: TaskStore::new(),
            view: ViewFilter::Today,
            sort_mode: SortMode::DueDate,
            search_query: String::new(),
            selected_task_id: None,
            notifications: Vec::new(),
            sidebar_visible: true,
            detail_visible: true,
            show_completed_subtasks: true,
        }
    }

    /// Check for tasks that should trigger notifications.
    pub fn check_notifications(&mut self) {
        let due_tasks = self.store.due_now(self.now);
        for task in due_tasks {
            let already = self
                .notifications
                .iter()
                .any(|n| n.task_id == task.id && !n.dismissed);
            if !already {
                self.notifications.push(Notification {
                    task_id: task.id,
                    message: format!("Reminder: {}", task.title),
                    triggered_at: self.now,
                    dismissed: false,
                });
            }
        }
    }

    /// Dismiss a notification.
    pub fn dismiss_notification(&mut self, task_id: u64) {
        for notif in &mut self.notifications {
            if notif.task_id == task_id {
                notif.dismissed = true;
            }
        }
    }

    /// Dismiss all notifications.
    pub fn dismiss_all_notifications(&mut self) {
        for notif in &mut self.notifications {
            notif.dismissed = true;
        }
    }

    /// Get active (non-dismissed) notifications.
    pub fn active_notifications(&self) -> Vec<&Notification> {
        self.notifications.iter().filter(|n| !n.dismissed).collect()
    }

    /// Cycle to the next sort mode.
    pub fn cycle_sort(&mut self) {
        self.sort_mode = self.sort_mode.next();
    }

    /// Get the filtered and sorted task list for the current view.
    pub fn current_tasks(&self) -> Vec<&Task> {
        let filtered = if self.search_query.is_empty() {
            self.store.filtered(self.view, self.now)
        } else {
            let search_results = self.store.search(&self.search_query);
            // Apply view filter to search results too
            search_results
                .into_iter()
                .filter(|t| match self.view {
                    ViewFilter::All => !t.completed,
                    ViewFilter::Today => !t.completed && t.is_due_today(self.today),
                    ViewFilter::Upcoming => !t.completed && t.is_due_within(self.today, 7),
                    ViewFilter::Overdue => t.is_overdue(self.now),
                    ViewFilter::Completed => t.completed,
                    ViewFilter::ByCategory(cat) => !t.completed && t.category == cat,
                })
                .collect()
        };
        TaskStore::sorted(&filtered, self.sort_mode)
    }

    /// Select a task by ID for the detail panel.
    pub fn select_task(&mut self, id: u64) {
        if self.store.get(id).is_some() {
            self.selected_task_id = Some(id);
            self.detail_visible = true;
        }
    }

    // ====================================================================
    // Render methods
    // ====================================================================

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Take the system clock's reading, and fire anything it made due.
    ///
    /// Leaves the previous reading standing if the clock cannot be read, for
    /// the reason given at [`system_now`].
    pub fn refresh_now(&mut self) -> EventResult {
        let Some(now) = system_now() else {
            return EventResult::Ignored;
        };
        if now == self.now {
            // The clock has not moved past the resolution this app works in,
            // so there is nothing new to be due and nothing to redraw.
            return EventResult::Ignored;
        }
        self.now = now;
        self.today = now.date;
        let before = self.active_notifications().len();
        self.check_notifications();
        // A minute passing changes the "in 5 minutes" and "overdue by" text on
        // every card, so the frame is stale whether or not anything fired.
        let _ = before;
        EventResult::Consumed
    }

    /// Route a compositor event into the app.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Tick { .. } => self.refresh_now(),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.width = *width as f32;
                    self.height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a key press.
    ///
    /// The app had no input handling before it was wired to the compositor:
    /// every view, sort order and selection it can show was reachable only by a
    /// caller setting the field directly.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        match key.key {
            Key::Num1 => self.set_view(ViewFilter::Today),
            Key::Num2 => self.set_view(ViewFilter::Upcoming),
            Key::Num3 => self.set_view(ViewFilter::All),
            Key::Num4 => self.set_view(ViewFilter::Overdue),
            Key::Num5 => self.set_view(ViewFilter::Completed),
            Key::S => {
                self.cycle_sort();
                EventResult::Consumed
            }
            Key::Up => self.step_selection(-1),
            Key::Down => self.step_selection(1),
            // Space is the near-universal "toggle the selected thing", and
            // completing a task is what this app is for.
            Key::Space | Key::Enter => self.toggle_selected_complete(),
            // Escape clears the banners rather than closing the window: a
            // reminder app that quits when you dismiss a reminder is a trap.
            Key::Escape => {
                if self.active_notifications().is_empty() {
                    return EventResult::Ignored;
                }
                self.dismiss_all_notifications();
                EventResult::Consumed
            }
            Key::B => {
                self.sidebar_visible = !self.sidebar_visible;
                EventResult::Consumed
            }
            Key::D => {
                self.detail_visible = !self.detail_visible;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Switch views, reporting whether anything changed.
    fn set_view(&mut self, view: ViewFilter) -> EventResult {
        if self.view == view {
            return EventResult::Ignored;
        }
        self.view = view;
        // The selection is an id, so it survives the view change — but it can
        // survive into a view that does not show it, where the highlight simply
        // vanishes. Re-anchor to something on screen.
        let visible: Vec<u64> = self.current_tasks().iter().map(|t| t.id).collect();
        if !self
            .selected_task_id
            .is_some_and(|id| visible.contains(&id))
        {
            self.selected_task_id = visible.first().copied();
        }
        EventResult::Consumed
    }

    /// Move the selection through the tasks currently listed.
    ///
    /// Over `current_tasks` and not the whole store, so the selection cannot
    /// step onto a task the current view is hiding.
    fn step_selection(&mut self, delta: isize) -> EventResult {
        let visible: Vec<u64> = self.current_tasks().iter().map(|t| t.id).collect();
        if visible.is_empty() {
            if self.selected_task_id.is_none() {
                return EventResult::Ignored;
            }
            self.selected_task_id = None;
            return EventResult::Consumed;
        }
        let current = self
            .selected_task_id
            .and_then(|id| visible.iter().position(|&v| v == id));
        let next = match current {
            // Not on screen: the first listed task is where the selection
            // belongs, whichever way the user pressed.
            None => 0,
            Some(pos) => {
                let Ok(pos) = isize::try_from(pos) else {
                    return EventResult::Ignored;
                };
                let Some(moved) = pos.checked_add(delta) else {
                    return EventResult::Ignored;
                };
                let Ok(moved) = usize::try_from(moved) else {
                    return EventResult::Ignored; // off the top; stay put
                };
                if moved >= visible.len() {
                    return EventResult::Ignored; // off the bottom; stay put
                }
                moved
            }
        };
        let Some(&id) = visible.get(next) else {
            return EventResult::Ignored;
        };
        if Some(id) == self.selected_task_id {
            return EventResult::Ignored;
        }
        self.selected_task_id = Some(id);
        EventResult::Consumed
    }

    /// Complete or un-complete whatever is selected.
    fn toggle_selected_complete(&mut self) -> EventResult {
        let Some(id) = self.selected_task_id else {
            return EventResult::Ignored;
        };
        let Some(done) = self.store.get(id).map(|t| t.completed) else {
            return EventResult::Ignored;
        };
        let changed = if done {
            self.store.uncomplete_task(id)
        } else {
            self.store.complete_task(id, self.now)
        };
        if !changed {
            return EventResult::Ignored;
        }
        // Completing the selected task can remove it from the view it was
        // listed in, which would leave the highlight nowhere.
        let visible: Vec<u64> = self.current_tasks().iter().map(|t| t.id).collect();
        if !visible.contains(&id) {
            self.selected_task_id = visible.first().copied();
        }
        EventResult::Consumed
    }

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    ///
    /// Renders the full application UI into a list of render commands.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Notification banner (if any active notifications)
        let notification_offset = self.render_notifications(&mut cmds);

        // Header
        self.render_header(&mut cmds, notification_offset);

        let content_y = HEADER_HEIGHT + notification_offset;
        let content_h = self.height - content_y;

        // Sidebar
        let main_x = if self.sidebar_visible {
            self.render_sidebar(&mut cmds, 0.0, content_y, SIDEBAR_WIDTH, content_h);
            SIDEBAR_WIDTH
        } else {
            0.0
        };

        // Detail panel
        let main_w = if self.detail_visible && self.selected_task_id.is_some() {
            let detail_x = self.width - DETAIL_PANEL_WIDTH;
            self.render_detail_panel(
                &mut cmds,
                detail_x,
                content_y,
                DETAIL_PANEL_WIDTH,
                content_h,
            );
            detail_x - main_x
        } else {
            self.width - main_x
        };

        // Main task list
        self.render_task_list(&mut cmds, main_x, content_y, main_w, content_h);

        cmds
    }

    /// Render notification banners at the top. Returns total height consumed.
    fn render_notifications(&self, cmds: &mut Vec<RenderCommand>) -> f32 {
        let active = self.active_notifications();
        if active.is_empty() {
            return 0.0;
        }

        let total_h = active.len() as f32 * NOTIFICATION_HEIGHT;

        // Background for all notifications
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: total_h,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        for (i, notif) in active.iter().enumerate() {
            let y = i as f32 * NOTIFICATION_HEIGHT;

            // Accent bar
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y,
                width: 4.0,
                height: NOTIFICATION_HEIGHT,
                color: PEACH,
                corner_radii: CornerRadii::ZERO,
            });

            // Bell icon placeholder
            cmds.push(RenderCommand::Text {
                x: 16.0,
                y: y + 14.0,
                text: "[!]".to_string(),
                font_size: 16.0,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: Some(30.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Message
            cmds.push(RenderCommand::Text {
                x: 48.0,
                y: y + 14.0,
                text: notif.message.clone(),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(self.width - 200.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Dismiss button
            cmds.push(RenderCommand::FillRect {
                x: self.width - 100.0,
                y: y + 10.0,
                width: 80.0,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: self.width - 88.0,
                y: y + 16.0,
                text: "Dismiss".to_string(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(70.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Snooze button
            cmds.push(RenderCommand::FillRect {
                x: self.width - 200.0,
                y: y + 10.0,
                width: 80.0,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: self.width - 190.0,
                y: y + 16.0,
                text: "Snooze".to_string(),
                font_size: 11.0,
                color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(70.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        total_h
    }

    /// Render the top header bar.
    fn render_header(&self, cmds: &mut Vec<RenderCommand>, y_offset: f32) {
        // Header background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: y_offset,
            width: self.width,
            height: HEADER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Bottom border
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: y_offset + HEADER_HEIGHT - 1.0,
            width: self.width,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y_offset + 8.0,
            text: "Reminders".to_string(),
            font_size: 20.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Current view label
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y_offset + 34.0,
            text: self.view.label(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Search box
        let search_x = 240.0;
        let search_w = 300.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x,
            y: y_offset + 12.0,
            width: search_w,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: search_x,
            y: y_offset + 12.0,
            width: search_w,
            height: 32.0,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        let search_text = if self.search_query.is_empty() {
            "Search tasks...".to_string()
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 12.0,
            y: y_offset + 20.0,
            text: search_text,
            font_size: 13.0,
            color: search_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Sort indicator
        let sort_x = search_x + search_w + 20.0;
        cmds.push(RenderCommand::FillRect {
            x: sort_x,
            y: y_offset + 12.0,
            width: 100.0,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: sort_x + 8.0,
            y: y_offset + 20.0,
            text: format!("Sort: {}", self.sort_mode.label()),
            font_size: 11.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(90.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Task count
        let tasks = self.current_tasks();
        let count_text = format!(
            "{} task{}",
            tasks.len(),
            if tasks.len() == 1 { "" } else { "s" }
        );
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: y_offset + 20.0,
            text: count_text,
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(110.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the left sidebar with views and categories.
    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Right border
        cmds.push(RenderCommand::FillRect {
            x: x + w - 1.0,
            y,
            width: 1.0,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let mut row_y = y + PADDING;

        // Views section
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: row_y,
            text: "VIEWS".to_string(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 20.0;

        for view in ViewFilter::standard_views() {
            let is_active = self.view == *view;
            let bg_color = if is_active { SURFACE0 } else { MANTLE };
            let text_color = if is_active { view.color() } else { SUBTEXT1 };

            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: row_y,
                width: w - 8.0,
                height: 30.0,
                color: bg_color,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            // Icon
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: row_y + 8.0,
                text: view.icon().to_string(),
                font_size: 11.0,
                color: text_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(30.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Label
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 30.0,
                y: row_y + 8.0,
                text: view.label(),
                font_size: 12.0,
                color: text_color,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w - PADDING * 2.0 - 60.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Count badge
            let badge_count = match view {
                ViewFilter::Today => self.store.count_today(self.today),
                ViewFilter::Overdue => self.store.count_overdue(self.now),
                ViewFilter::Completed => self.store.count_completed(),
                _ => 0,
            };
            if badge_count > 0 {
                let badge_text = format!("{badge_count}");
                // A count pill has a radius of half its height, so a single
                // digit measured honestly would render as a squashed oval:
                // floor the width at the height to keep it round.
                let badge_w =
                    text::padded_width(&badge_text, 5.0, 10.0, FontWeightHint::Regular).max(18.0);
                cmds.push(RenderCommand::FillRect {
                    x: x + w - badge_w - 12.0,
                    y: row_y + 6.0,
                    width: badge_w,
                    height: 18.0,
                    color: view.color(),
                    corner_radii: CornerRadii::all(9.0),
                });
                cmds.push(RenderCommand::Text {
                    x: text::center_x(
                        &badge_text,
                        x + w - badge_w / 2.0 - 12.0,
                        10.0,
                        FontWeightHint::Regular,
                    ),
                    y: row_y + 9.0,
                    text: badge_text,
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(badge_w),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            row_y += 34.0;
        }

        // Categories section
        row_y += 12.0;
        cmds.push(RenderCommand::FillRect {
            x: x + PADDING,
            y: row_y,
            width: w - PADDING * 2.0,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        row_y += 12.0;

        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: row_y,
            text: "CATEGORIES".to_string(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 20.0;

        for cat in TaskCategory::all() {
            let is_active = self.view == ViewFilter::ByCategory(*cat);
            let bg_color = if is_active { SURFACE0 } else { MANTLE };
            let text_color = if is_active { cat.color() } else { SUBTEXT1 };

            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: row_y,
                width: w - 8.0,
                height: 28.0,
                color: bg_color,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            // Color dot
            cmds.push(RenderCommand::FillRect {
                x: x + PADDING,
                y: row_y + 9.0,
                width: 10.0,
                height: 10.0,
                color: cat.color(),
                corner_radii: CornerRadii::all(5.0),
            });

            // Label
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 18.0,
                y: row_y + 7.0,
                text: cat.label().to_string(),
                font_size: 12.0,
                color: text_color,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w - PADDING * 2.0 - 50.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Count
            let count = self.store.count_by_category(*cat);
            if count > 0 {
                cmds.push(RenderCommand::Text {
                    x: x + w - 30.0,
                    y: row_y + 7.0,
                    text: format!("{count}"),
                    font_size: 11.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(25.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            row_y += 32.0;
        }
    }

    /// Render the main task list panel.
    fn render_task_list(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Clip region
        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width: w,
            height: h,
        });

        let tasks = self.current_tasks();
        let mut row_y = y + PADDING;

        if tasks.is_empty() {
            // Empty state
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 80.0,
                y: y + h / 2.0 - 30.0,
                text: "No tasks".to_string(),
                font_size: 18.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 100.0,
                y: y + h / 2.0,
                text: "Create a new task to get started".to_string(),
                font_size: 13.0,
                color: SURFACE2,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::PopClip);
            return;
        }

        for task in &tasks {
            if row_y > y + h {
                break;
            }

            let is_selected = self.selected_task_id == Some(task.id);
            self.render_task_item(
                cmds,
                task,
                x + PADDING,
                row_y,
                w - PADDING * 2.0,
                is_selected,
            );
            row_y += ITEM_HEIGHT + 4.0;
        }

        cmds.push(RenderCommand::PopClip);
    }

    /// Render a single task item card.
    fn render_task_item(
        &self,
        cmds: &mut Vec<RenderCommand>,
        task: &Task,
        x: f32,
        y: f32,
        w: f32,
        selected: bool,
    ) {
        let card_color = if selected { SURFACE0 } else { MANTLE };

        // Card shadow (subtle)
        cmds.push(RenderCommand::BoxShadow {
            x,
            y,
            width: w,
            height: ITEM_HEIGHT,
            offset_x: 0.0,
            offset_y: 1.0,
            blur: 4.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 40),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Card background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: ITEM_HEIGHT,
            color: card_color,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Selection highlight
        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x,
                y,
                width: w,
                height: ITEM_HEIGHT,
                color: BLUE,
                line_width: 1.5,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
        }

        // Priority bar on the left
        cmds.push(RenderCommand::FillRect {
            x,
            y: y + 4.0,
            width: 4.0,
            height: ITEM_HEIGHT - 8.0,
            color: task.priority.color(),
            corner_radii: CornerRadii::all(2.0),
        });

        // Checkbox area
        let checkbox_x = x + 14.0;
        let checkbox_y = y + ITEM_HEIGHT / 2.0 - 10.0;
        cmds.push(RenderCommand::StrokeRect {
            x: checkbox_x,
            y: checkbox_y,
            width: 20.0,
            height: 20.0,
            color: if task.completed { GREEN } else { SURFACE2 },
            line_width: 1.5,
            corner_radii: CornerRadii::all(4.0),
        });
        if task.completed {
            cmds.push(RenderCommand::FillRect {
                x: checkbox_x + 3.0,
                y: checkbox_y + 3.0,
                width: 14.0,
                height: 14.0,
                color: GREEN,
                corner_radii: CornerRadii::all(3.0),
            });
            // Checkmark text
            cmds.push(RenderCommand::Text {
                x: checkbox_x + 4.0,
                y: checkbox_y + 3.0,
                text: "v".to_string(),
                font_size: 12.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Title
        let text_x = checkbox_x + 30.0;
        let title_color = if task.completed { OVERLAY0 } else { TEXT };
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: y + 12.0,
            text: task.title.clone(),
            font_size: 14.0,
            color: title_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 100.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Due date label
        let due_text = task.due_label(self.today);
        let due_color = if task.is_overdue(self.now) {
            RED
        } else {
            SUBTEXT0
        };
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: y + 30.0,
            text: due_text,
            font_size: 11.0,
            color: due_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 120.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Category + priority badges
        let badge_y = y + 48.0;
        // Category badge
        cmds.push(RenderCommand::FillRect {
            x: text_x,
            y: badge_y,
            width: 60.0,
            height: 18.0,
            color: Color::rgba(
                task.category.color().r,
                task.category.color().g,
                task.category.color().b,
                40,
            ),
            corner_radii: CornerRadii::all(9.0),
        });
        cmds.push(RenderCommand::Text {
            x: text_x + 8.0,
            y: badge_y + 3.0,
            text: task.category.label().to_string(),
            font_size: 9.0,
            color: task.category.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(55.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Priority badge
        let pri_x = text_x + 68.0;
        cmds.push(RenderCommand::FillRect {
            x: pri_x,
            y: badge_y,
            width: 55.0,
            height: 18.0,
            color: Color::rgba(
                task.priority.color().r,
                task.priority.color().g,
                task.priority.color().b,
                40,
            ),
            corner_radii: CornerRadii::all(9.0),
        });
        cmds.push(RenderCommand::Text {
            x: pri_x + 6.0,
            y: badge_y + 3.0,
            text: task.priority.label().to_string(),
            font_size: 9.0,
            color: task.priority.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(50.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Recurrence indicator
        if task.recurrence != RecurrenceRule::None {
            let rec_x = pri_x + 62.0;
            cmds.push(RenderCommand::Text {
                x: rec_x,
                y: badge_y + 3.0,
                text: format!("[{}]", task.recurrence.label()),
                font_size: 9.0,
                color: TEAL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Progress bar (if subtasks exist)
        if !task.subtasks.is_empty() {
            let pct = task.completion_percent();
            let bar_x = w - 80.0 + x;
            let bar_y = y + 14.0;
            let bar_w = 60.0;
            let bar_h = 6.0;

            // Track
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: bar_w,
                height: bar_h,
                color: SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });

            // Fill
            let fill_w = (bar_w * pct as f32) / 100.0;
            if fill_w > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: fill_w,
                    height: bar_h,
                    color: GREEN,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            // Percentage text
            cmds.push(RenderCommand::Text {
                x: bar_x,
                y: bar_y + 10.0,
                text: format!("{pct}%"),
                font_size: 9.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bar_w),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Render the detail panel for the selected task.
    fn render_detail_panel(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let Some(task_id) = self.selected_task_id else {
            return;
        };
        let Some(task) = self.store.get(task_id) else {
            return;
        };

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 1.0,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let pad = PADDING;
        let mut row_y = y + pad;
        let content_w = w - pad * 2.0;

        // Title
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: row_y,
            text: task.title.clone(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 28.0;

        // Status pill
        let status_text = if task.completed {
            "Completed"
        } else {
            "Active"
        };
        let status_color = if task.completed { GREEN } else { BLUE };
        cmds.push(RenderCommand::FillRect {
            x: x + pad,
            y: row_y,
            width: 80.0,
            height: 22.0,
            color: Color::rgba(status_color.r, status_color.g, status_color.b, 40),
            corner_radii: CornerRadii::all(11.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 10.0,
            y: row_y + 4.0,
            text: status_text.to_string(),
            font_size: 11.0,
            color: status_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 32.0;

        // Separator
        cmds.push(RenderCommand::FillRect {
            x: x + pad,
            y: row_y,
            width: content_w,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        row_y += 12.0;

        // Detail fields
        let field_label_color = OVERLAY0;
        let field_value_color = SUBTEXT1;

        // Due date
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: row_y,
            text: "Due".to_string(),
            font_size: 10.0,
            color: field_label_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 14.0;
        let due_text = if let Some(due) = task.due {
            due.format_medium()
        } else {
            "Not set".to_string()
        };
        let due_color = if task.is_overdue(self.now) {
            RED
        } else {
            field_value_color
        };
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: row_y,
            text: due_text,
            font_size: 13.0,
            color: due_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 22.0;

        // Priority
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: row_y,
            text: "Priority".to_string(),
            font_size: 10.0,
            color: field_label_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 14.0;
        cmds.push(RenderCommand::FillRect {
            x: x + pad,
            y: row_y,
            width: 8.0,
            height: 8.0,
            color: task.priority.color(),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 14.0,
            y: row_y - 2.0,
            text: task.priority.label().to_string(),
            font_size: 13.0,
            color: task.priority.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 22.0;

        // Category
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: row_y,
            text: "Category".to_string(),
            font_size: 10.0,
            color: field_label_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 14.0;
        cmds.push(RenderCommand::FillRect {
            x: x + pad,
            y: row_y,
            width: 8.0,
            height: 8.0,
            color: task.category.color(),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 14.0,
            y: row_y - 2.0,
            text: task.category.label().to_string(),
            font_size: 13.0,
            color: task.category.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 22.0;

        // Recurrence
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: row_y,
            text: "Repeats".to_string(),
            font_size: 10.0,
            color: field_label_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 14.0;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: row_y,
            text: task.recurrence.label().to_string(),
            font_size: 13.0,
            color: field_value_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 22.0;

        // Description
        if !task.description.is_empty() {
            cmds.push(RenderCommand::FillRect {
                x: x + pad,
                y: row_y,
                width: content_w,
                height: 1.0,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            row_y += 12.0;

            cmds.push(RenderCommand::Text {
                x: x + pad,
                y: row_y,
                text: "Description".to_string(),
                font_size: 10.0,
                color: field_label_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(content_w),
                overflow: TextOverflow::Ellipsis,
            });
            row_y += 14.0;
            // `RenderCommand::Text` clips at `max_width` rather than wrapping,
            // so a description longer than the panel is wide used to reach the
            // user as its first line and nothing else. `Paragraph::draw`
            // reports the height it used, so the cursor advances over what was
            // actually drawn and the fields below cannot land on top of it.
            row_y += detail_prose(&task.description, x + pad, row_y, content_w).draw(cmds);
            row_y += PROSE_FIELD_GAP;
        }

        // Notes
        if !task.notes.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + pad,
                y: row_y,
                text: "Notes".to_string(),
                font_size: 10.0,
                color: field_label_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(content_w),
                overflow: TextOverflow::Ellipsis,
            });
            row_y += 14.0;
            row_y += detail_prose(&task.notes, x + pad, row_y, content_w).draw(cmds);
            row_y += PROSE_FIELD_GAP;
        }

        // Subtasks
        if !task.subtasks.is_empty() {
            cmds.push(RenderCommand::FillRect {
                x: x + pad,
                y: row_y,
                width: content_w,
                height: 1.0,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            row_y += 12.0;

            let done_count = task.subtasks.iter().filter(|s| s.completed).count();
            cmds.push(RenderCommand::Text {
                x: x + pad,
                y: row_y,
                text: format!("Subtasks ({}/{})", done_count, task.subtasks.len()),
                font_size: 10.0,
                color: field_label_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(content_w),
                overflow: TextOverflow::Ellipsis,
            });
            row_y += 16.0;

            // Progress bar
            let pct = task.completion_percent();
            cmds.push(RenderCommand::FillRect {
                x: x + pad,
                y: row_y,
                width: content_w,
                height: 6.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            let fill_w = (content_w * pct as f32) / 100.0;
            if fill_w > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: x + pad,
                    y: row_y,
                    width: fill_w,
                    height: 6.0,
                    color: GREEN,
                    corner_radii: CornerRadii::all(3.0),
                });
            }
            row_y += 14.0;

            for st in &task.subtasks {
                if !self.show_completed_subtasks && st.completed {
                    continue;
                }
                let st_color = if st.completed { OVERLAY0 } else { TEXT };

                // Mini checkbox
                cmds.push(RenderCommand::StrokeRect {
                    x: x + pad,
                    y: row_y,
                    width: 14.0,
                    height: 14.0,
                    color: if st.completed { GREEN } else { SURFACE2 },
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(3.0),
                });
                if st.completed {
                    cmds.push(RenderCommand::FillRect {
                        x: x + pad + 2.0,
                        y: row_y + 2.0,
                        width: 10.0,
                        height: 10.0,
                        color: GREEN,
                        corner_radii: CornerRadii::all(2.0),
                    });
                }

                cmds.push(RenderCommand::Text {
                    x: x + pad + 20.0,
                    y: row_y,
                    text: st.title.clone(),
                    font_size: 12.0,
                    color: st_color,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_w - 24.0),
                    overflow: TextOverflow::Ellipsis,
                });
                row_y += 20.0;
            }
        }

        // Snooze options (if task has a due date and is not completed)
        if task.due.is_some() && !task.completed {
            row_y += 8.0;
            cmds.push(RenderCommand::FillRect {
                x: x + pad,
                y: row_y,
                width: content_w,
                height: 1.0,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            row_y += 12.0;

            cmds.push(RenderCommand::Text {
                x: x + pad,
                y: row_y,
                text: "Snooze".to_string(),
                font_size: 10.0,
                color: field_label_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(content_w),
                overflow: TextOverflow::Ellipsis,
            });
            row_y += 16.0;

            let btn_w = (content_w - 8.0) / 2.0;
            for (i, preset) in SnoozeDuration::presets().iter().enumerate() {
                let col = i % 2;
                let row_idx = i / 2;
                let bx = x + pad + col as f32 * (btn_w + 8.0);
                let by = row_y + row_idx as f32 * 30.0;

                cmds.push(RenderCommand::FillRect {
                    x: bx,
                    y: by,
                    width: btn_w,
                    height: 24.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
                cmds.push(RenderCommand::Text {
                    x: bx + 8.0,
                    y: by + 5.0,
                    text: preset.label(),
                    font_size: 11.0,
                    color: SKY,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(btn_w - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // Created date at the bottom
        let created_y = y + h - 24.0;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: created_y,
            text: format!("Created: {}", task.created.format_short()),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Sample data
// ============================================================================

fn sample_tasks(store: &mut TaskStore, now: DateTime) {
    let today = now.date;

    // Overdue task
    let mut t = Task::new(0, "Review quarterly report", now);
    t.description = "Go through Q1 numbers and prepare summary".to_string();
    t.due = Some(DateTime::new(
        today.add_days(-2),
        Time {
            hour: 17,
            minute: 0,
        },
    ));
    t.priority = Priority::High;
    t.category = TaskCategory::Work;
    t.subtasks = vec![
        {
            let mut s = Subtask::new("Read financials");
            s.completed = true;
            s
        },
        Subtask::new("Write summary"),
        Subtask::new("Send to team"),
    ];
    store.add(t);

    // Due today
    let mut t = Task::new(0, "Buy groceries", now);
    t.description = "Milk, eggs, bread, vegetables".to_string();
    t.due = Some(DateTime::new(
        today,
        Time {
            hour: 18,
            minute: 0,
        },
    ));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Shopping;
    store.add(t);

    // Due today, high priority
    let mut t = Task::new(0, "Doctor appointment", now);
    t.description = "Annual checkup at 2pm".to_string();
    t.due = Some(DateTime::new(
        today,
        Time {
            hour: 14,
            minute: 0,
        },
    ));
    t.priority = Priority::High;
    t.category = TaskCategory::Health;
    store.add(t);

    // Due tomorrow
    let mut t = Task::new(0, "Submit tax documents", now);
    t.due = Some(DateTime::new(
        today.add_days(1),
        Time {
            hour: 12,
            minute: 0,
        },
    ));
    t.priority = Priority::Critical;
    t.category = TaskCategory::Finance;
    store.add(t);

    // Recurring daily
    let mut t = Task::new(0, "Morning exercise", now);
    t.description = "30 min cardio + stretching".to_string();
    t.due = Some(DateTime::new(
        today.add_days(1),
        Time { hour: 7, minute: 0 },
    ));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Health;
    t.recurrence = RecurrenceRule::Daily;
    store.add(t);

    // Recurring weekly
    let mut t = Task::new(0, "Team standup meeting", now);
    t.due = Some(DateTime::new(
        today.add_days(2),
        Time { hour: 9, minute: 0 },
    ));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Work;
    t.recurrence = RecurrenceRule::Weekly;
    store.add(t);

    // Due in 5 days
    let mut t = Task::new(0, "Pay electricity bill", now);
    t.due = Some(DateTime::new(
        today.add_days(5),
        Time {
            hour: 23,
            minute: 59,
        },
    ));
    t.priority = Priority::High;
    t.category = TaskCategory::Finance;
    t.notes = "Account #12345, auto-pay not set up yet".to_string();
    store.add(t);

    // Due in a week
    let mut t = Task::new(0, "Plan birthday party", now);
    t.description = "Venue, guest list, catering".to_string();
    t.due = Some(DateTime::new(
        today.add_days(7),
        Time {
            hour: 10,
            minute: 0,
        },
    ));
    t.priority = Priority::Low;
    t.category = TaskCategory::Social;
    t.subtasks = vec![
        {
            let mut s = Subtask::new("Choose venue");
            s.completed = true;
            s
        },
        {
            let mut s = Subtask::new("Send invitations");
            s.completed = true;
            s
        },
        Subtask::new("Order cake"),
        Subtask::new("Buy decorations"),
        Subtask::new("Arrange catering"),
    ];
    store.add(t);

    // Monthly recurring
    let mut t = Task::new(0, "Monthly budget review", now);
    t.due = Some(DateTime::new(
        today.add_days(14),
        Time {
            hour: 20,
            minute: 0,
        },
    ));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Finance;
    t.recurrence = RecurrenceRule::Monthly;
    store.add(t);

    // Education
    let mut t = Task::new(0, "Complete Rust course module 5", now);
    t.description = "Async programming and futures".to_string();
    t.due = Some(DateTime::new(
        today.add_days(3),
        Time {
            hour: 21,
            minute: 0,
        },
    ));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Education;
    t.subtasks = vec![
        {
            let mut s = Subtask::new("Watch lectures");
            s.completed = true;
            s
        },
        Subtask::new("Do exercises"),
        Subtask::new("Submit assignment"),
    ];
    store.add(t);

    // Completed task
    let mut t = Task::new(0, "Clean the garage", now);
    t.due = Some(DateTime::new(
        today.add_days(-1),
        Time {
            hour: 10,
            minute: 0,
        },
    ));
    t.priority = Priority::Low;
    t.category = TaskCategory::Home;
    t.completed = true;
    t.completed_at = Some(DateTime::new(
        today.add_days(-1),
        Time {
            hour: 16,
            minute: 0,
        },
    ));
    store.add(t);

    // No due date
    let mut t = Task::new(0, "Read 'Designing Data-Intensive Applications'", now);
    t.priority = Priority::Low;
    t.category = TaskCategory::Education;
    store.add(t);
}

impl App for RemindersApp {
    fn title(&self) -> String {
        let overdue = self.store.count_overdue(self.now);
        if overdue == 0 {
            "Reminders".to_owned()
        } else {
            // The count belongs in the title because a taskbar entry is all
            // that is visible of a minimised reminder app.
            format!("Reminders ({overdue} overdue)")
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.width as u32, self.height as u32)
        }
    }

    /// Every thirty seconds, and **not** an interval computed from the next due
    /// time.
    ///
    /// Due times are minute-granular, so the worst case for a reminder is the
    /// tick period; thirty seconds keeps that under half a minute, and two
    /// wake-ups a minute is a cost a reminder app has earned.
    ///
    /// The tempting optimisation — return the time until the next due task, so
    /// an idle machine sleeps — is **wrong here**, and quietly. `sync_clock` in
    /// `gui/window/src/app.rs` will not re-arm a wake-up that is already
    /// pending; it re-reads this only once the previous one fires. Return six
    /// hours because that is when the next task is due, and a task the user
    /// adds a minute later fires six hours late. The interval has to be short
    /// enough to be wrong by an acceptable amount.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_secs(30))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.width = width;
        self.height = height;
        RenderTree {
            commands: self.render_commands(),
        }
    }
}

fn main() -> ExitCode {
    // The fallback only applies if the system clock cannot be read at all, and
    // it is a real instant rather than the epoch so that the sample tasks below
    // — which are all relative to it — still make sense.
    let now = system_now().unwrap_or_else(|| {
        DateTime::new(
            Date {
                year: 2026,
                month: 5,
                day: 18,
            },
            Time {
                hour: 10,
                minute: 30,
            },
        )
    });
    let mut app = RemindersApp::new(WINDOW_WIDTH, WINDOW_HEIGHT, now);

    // Until there is a store on disk this is what there is to show, and the
    // dates are all relative to `now`, so "overdue" and "due today" are true
    // statements about the clock rather than about May 2026.
    sample_tasks(&mut app.store, now);
    app.selected_task_id = app.current_tasks().first().map(|t| t.id);
    app.check_notifications();

    app::launch("reminders", &mut app)
}

// ============================================================================
// The clock
// ============================================================================

/// Read the system clock as one of this app's `DateTime`s.
///
/// Returns `None` if the clock cannot be read, so the caller can leave the
/// previous reading standing: a reminder app that silently decides it is
/// midnight on 1 January 1970 marks every task overdue.
///
/// The zone comes from `tzrules` in the same way `apps/alarmclock` gets it, so
/// this picks up a real local zone on the day
/// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ` is fixed, rather than needing to be
/// found and changed again.
fn system_now() -> Option<DateTime> {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let utc = i64::try_from(since_epoch.as_secs()).ok()?;
    let zone = tzrules::Tz::utc();
    let local = utc.saturating_add(i64::from(zone.lookup(utc).gmtoff));

    // `rem_euclid`, not `%`: a pre-1970 instant with `%` gives a negative
    // remainder, which is not a time of day at all.
    let secs_into_day = local.rem_euclid(86_400);
    // Both divisions are bounded by the line above: 0..86_400 / 3600 is 0..24,
    // and the remainder of a minute is 0..60.
    let hour = u32::try_from(secs_into_day / 3600).ok()?;
    let minute = u32::try_from((secs_into_day / 60) % 60).ok()?;

    // The civil date comes from `guitk::date`, the toolkit's one calendar,
    // rather than a private day-number formula in this file.
    let (year, month, day) = guitk::date::Date::from_unix_utc(local).ymd();
    //  already returns exactly the widths Fri Sep  4 02:10:08 EDT 2026 holds, so there is
    // nothing left to convert here.
    Some(DateTime::new(
        Date { year, month, day },
        Time { hour, minute },
    ))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that overflows, indexes out of range or unwraps a `None` should
    // fail loudly and point at the line that did it — that is the diagnosis.
    // The defensive lints exist to keep panics out of code that runs on a
    // user'"'"'s data, which this is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    // ------------------------------------------------------------------
    // Events and the clock
    //
    // The app had no input handling at all until it was wired to the
    // compositor, so every one of these covers a path with no prior coverage.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn populated() -> RemindersApp {
        let now = make_now();
        let mut app = RemindersApp::new(1200.0, 800.0, now);
        sample_tasks(&mut app.store, now);
        app.view = ViewFilter::All;
        app.selected_task_id = app.current_tasks().first().map(|t| t.id);
        app
    }

    #[test]
    fn the_number_row_reaches_every_standard_view() {
        let mut app = populated();
        for (k, view) in [
            (Key::Num1, ViewFilter::Today),
            (Key::Num2, ViewFilter::Upcoming),
            (Key::Num4, ViewFilter::Overdue),
            (Key::Num5, ViewFilter::Completed),
            (Key::Num3, ViewFilter::All),
        ] {
            assert_eq!(app.handle_event(&press(k)), EventResult::Consumed);
            assert_eq!(app.view, view, "{k:?} went to the wrong view");
        }
    }

    #[test]
    fn asking_for_the_view_already_shown_is_not_a_redraw() {
        let mut app = populated();
        assert_eq!(app.view, ViewFilter::All);
        assert_eq!(app.handle_event(&press(Key::Num3)), EventResult::Ignored);
    }

    #[test]
    fn changing_view_leaves_the_selection_on_a_listed_task() {
        // The selection is an id, so it survives a view change — into a view
        // that may not list it, where the highlight simply disappears.
        let mut app = populated();
        for k in [Key::Num1, Key::Num2, Key::Num4, Key::Num5, Key::Num3] {
            app.handle_event(&press(k));
            let listed: Vec<u64> = app.current_tasks().iter().map(|t| t.id).collect();
            match app.selected_task_id {
                Some(id) => assert!(
                    listed.contains(&id),
                    "{:?} left the selection on a task it does not list",
                    app.view
                ),
                None => assert!(listed.is_empty(), "nothing selected while tasks are listed"),
            }
        }
    }

    #[test]
    fn the_arrows_walk_the_listed_tasks_and_stop_at_the_ends() {
        let mut app = populated();
        let listed: Vec<u64> = app.current_tasks().iter().map(|t| t.id).collect();
        assert!(listed.len() >= 3, "the sample data should fill the list");
        assert_eq!(app.selected_task_id, listed.first().copied());
        // Up at the top stays put rather than wrapping to the last task.
        assert_eq!(app.handle_event(&press(Key::Up)), EventResult::Ignored);
        assert_eq!(app.selected_task_id, listed.first().copied());
        for id in listed.iter().skip(1) {
            assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
            assert_eq!(app.selected_task_id, Some(*id));
        }
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Ignored);
        assert_eq!(app.selected_task_id, listed.last().copied());
    }

    #[test]
    fn the_arrows_never_select_a_task_the_view_is_hiding() {
        let mut app = populated();
        app.handle_event(&press(Key::Num4)); // Overdue
        let listed: Vec<u64> = app.current_tasks().iter().map(|t| t.id).collect();
        assert!(
            listed.len() < app.store.len(),
            "the overdue view should hide some tasks"
        );
        for _ in 0..app.store.len() {
            app.handle_event(&press(Key::Down));
            if let Some(id) = app.selected_task_id {
                assert!(listed.contains(&id), "selection left the overdue view");
            }
        }
    }

    #[test]
    fn space_completes_the_selected_task_and_enter_does_the_same() {
        for key in [Key::Space, Key::Enter] {
            let mut app = populated();
            let id = app.selected_task_id.expect("something is selected");
            assert!(!app.store.get(id).expect("the task exists").completed);
            assert_eq!(app.handle_event(&press(key)), EventResult::Consumed);
            assert!(
                app.store.get(id).expect("the task still exists").completed,
                "{key:?} did not complete the task"
            );
        }
    }

    #[test]
    fn completing_a_task_out_of_its_view_moves_the_selection_somewhere_real() {
        let mut app = populated();
        app.handle_event(&press(Key::Num4)); // Overdue: completing leaves it
        let id = app.selected_task_id.expect("something is selected");
        app.handle_event(&press(Key::Space));
        let listed: Vec<u64> = app.current_tasks().iter().map(|t| t.id).collect();
        assert!(!listed.contains(&id), "a completed task is not overdue");
        if let Some(sel) = app.selected_task_id {
            assert!(listed.contains(&sel), "selection points off the list");
        } else {
            assert!(listed.is_empty());
        }
    }

    #[test]
    fn space_with_nothing_selected_does_nothing() {
        let mut app = populated();
        app.selected_task_id = None;
        let done = app.store.all().iter().filter(|t| t.completed).count();
        assert_eq!(app.handle_event(&press(Key::Space)), EventResult::Ignored);
        assert_eq!(app.store.all().iter().filter(|t| t.completed).count(), done);
    }

    #[test]
    fn escape_dismisses_the_banners_rather_than_closing_the_window() {
        // A reminder app that quits when you dismiss a reminder is a trap.
        let mut app = populated();
        app.check_notifications();
        if app.active_notifications().is_empty() {
            // Nothing was due; then Escape has nothing to do and must say so.
            assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Ignored);
            return;
        }
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Consumed);
        assert!(app.active_notifications().is_empty());
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Ignored);
    }

    #[test]
    fn a_key_the_app_has_no_use_for_is_not_consumed() {
        let mut app = populated();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = populated();
        let release = Event::Key(KeyEvent {
            key: Key::Num5,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.view, ViewFilter::All);
    }

    #[test]
    fn a_tick_moves_the_clock_and_a_second_one_in_the_same_minute_does_not() {
        // The whole point of the tick: `now` must advance, or nothing is ever
        // due and the notification banners never appear — lesson 47 exactly.
        let mut app = populated();
        let stale = app.now;
        let first = app.handle_event(&Event::Tick { elapsed_ms: 30_000 });
        assert_eq!(
            first,
            EventResult::Consumed,
            "the first tick should adopt the real clock"
        );
        assert_ne!(app.now, stale, "the clock did not move");
        assert_eq!(app.today, app.now.date, "today drifted from now");
        // Two ticks inside one minute are the same minute, and redrawing for
        // that is a wake-up spent on an identical frame.
        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 30_000 }),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_system_clock_reads_as_a_plausible_date() {
        let now = system_now().expect("the host has a readable clock");
        assert!(
            (2020..2200).contains(&now.date.year),
            "implausible year {}",
            now.date.year
        );
        assert!(
            (1..=12).contains(&now.date.month),
            "month {}",
            now.date.month
        );
        assert!((1..=31).contains(&now.date.day), "day {}", now.date.day);
        assert!(now.time.hour < 24, "hour {}", now.time.hour);
        assert!(now.time.minute < 60, "minute {}", now.time.minute);
    }

    #[test]
    fn the_system_clock_reading_converts_back_to_the_instant_it_came_from() {
        // Ranges alone do not test the conversion: adding a month to the
        // result in September still yields a plausible-looking date and every
        // range assertion above passes. This takes the reading apart and puts
        // it back together through `guitk::date`, which catches a field
        // assigned to the wrong field, an off-by-one, and a month or hour that
        // has drifted -- the transcription errors this function can have.
        let now = system_now().expect("the host has a readable clock");
        let utc = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the host clock is after 1970")
                .as_secs(),
        )
        .expect("seconds since 1970 fit in i64");
        let zone = tzrules::Tz::utc();
        let local = utc.saturating_add(i64::from(zone.lookup(utc).gmtoff));

        let rebuilt = guitk::date::Date::from_ymd(now.date.year, now.date.month, now.date.day)
            .unix_secs_utc()
            + i64::from(now.time.hour) * 3600
            + i64::from(now.time.minute) * 60;

        // `system_now` drops seconds, so the rebuilt instant is up to 59 s
        // behind; a further minute of slack covers the clock advancing between
        // the two readings taken above.
        let skew = local - rebuilt;
        assert!(
            (0..120).contains(&skew),
            "the reading does not convert back to the instant it came from:              off by {skew} s (local {local}, rebuilt {rebuilt}, read {now:?})"
        );
    }

    #[test]
    fn the_title_counts_the_overdue_tasks() {
        // A minimised reminder app is a taskbar entry and nothing else, so the
        // count has to be in the title or it is not visible at all.
        let mut app = populated();
        let overdue = app.store.count_overdue(app.now);
        assert!(
            overdue > 0,
            "the sample data should include an overdue task"
        );
        assert!(
            app.title().contains(&overdue.to_string()),
            "title {:?} does not name the {overdue} overdue tasks",
            app.title()
        );
        for t in app.store.all().iter().map(|t| t.id).collect::<Vec<_>>() {
            app.store.complete_task(t, app.now);
        }
        assert_eq!(app.store.count_overdue(app.now), 0);
        assert_eq!(app.title(), "Reminders", "a clear list should read plainly");
    }

    #[test]
    fn a_resize_is_taken_but_is_not_itself_a_redraw() {
        let mut app = populated();
        assert_eq!(
            app.handle_event(&Event::Resize {
                width: 1280,
                height: 1024
            }),
            EventResult::Ignored
        );
        assert!((app.width - 1280.0).abs() < f32::EPSILON);
        assert!((app.height - 1024.0).abs() < f32::EPSILON);
    }
    use super::*;

    fn make_now() -> DateTime {
        DateTime::new(
            Date {
                year: 2026,
                month: 5,
                day: 18,
            },
            Time {
                hour: 10,
                minute: 30,
            },
        )
    }

    fn make_task(title: &str, now: DateTime) -> Task {
        Task::new(0, title, now)
    }

    // === Date tests ===

    #[test]
    fn test_date_new_valid() {
        assert!(Date::new(2026, 1, 1).is_some());
        assert!(Date::new(2026, 12, 31).is_some());
        assert!(Date::new(2024, 2, 29).is_some()); // Leap year
    }

    #[test]
    fn test_date_new_invalid() {
        assert!(Date::new(2026, 0, 1).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert!(Date::new(2025, 2, 29).is_none()); // Not a leap year
        assert!(Date::new(2026, 1, 32).is_none());
    }

    #[test]
    fn test_leap_year() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2025));
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
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

    /// The weekday *labels*, which the list and detail views draw and which
    /// nothing asserted: swapping `short_name()` for `name()` failed no test
    /// at all, so "Wednesday" could have appeared in a column sized for
    /// "Wed". `day_of_week` was covered; the two functions that turn it into
    /// something a user reads were not.
    #[test]
    fn weekday_labels_are_the_right_day_in_the_right_length() {
        // 2024-01-01 is a Monday, so this walks Monday..Sunday in order.
        let want = [
            ("Monday", "Mon"),
            ("Tuesday", "Tue"),
            ("Wednesday", "Wed"),
            ("Thursday", "Thu"),
            ("Friday", "Fri"),
            ("Saturday", "Sat"),
            ("Sunday", "Sun"),
        ];
        for (i, (long, short)) in want.iter().enumerate() {
            let d = Date {
                year: 2024,
                month: 1,
                day: 1,
            }
            .add_days(i32::try_from(i).unwrap_or(0));
            assert_eq!(d.day_of_week_name(), *long, "day {i}");
            assert_eq!(d.day_of_week_short(), *short, "day {i}");
            // The short form is three characters and is the long form's
            // prefix. A column laid out for the short name is not laid out
            // for the long one.
            assert_eq!(d.day_of_week_short().len(), 3, "day {i}");
            assert!(long.starts_with(short), "day {i}");
        }
    }

    #[test]
    fn test_date_add_days_forward() {
        let d = Date {
            year: 2026,
            month: 1,
            day: 30,
        };
        let next = d.add_days(3);
        assert_eq!(next.month, 2);
        assert_eq!(next.day, 2);
    }

    #[test]
    fn test_date_add_days_backward() {
        let d = Date {
            year: 2026,
            month: 3,
            day: 1,
        };
        let prev = d.add_days(-1);
        assert_eq!(prev.month, 2);
        assert_eq!(prev.day, 28);
    }

    #[test]
    fn test_date_add_days_year_boundary() {
        let d = Date {
            year: 2026,
            month: 12,
            day: 30,
        };
        let next = d.add_days(5);
        assert_eq!(next.year, 2027);
        assert_eq!(next.month, 1);
    }

    #[test]
    fn test_date_add_months() {
        let d = Date {
            year: 2026,
            month: 1,
            day: 31,
        };
        let next = d.add_months(1);
        assert_eq!(next.month, 2);
        assert_eq!(next.day, 28); // Clamped
    }

    #[test]
    fn test_date_add_months_year_wrap() {
        let d = Date {
            year: 2026,
            month: 11,
            day: 15,
        };
        let next = d.add_months(3);
        assert_eq!(next.year, 2027);
        assert_eq!(next.month, 2);
    }

    #[test]
    fn test_date_days_since() {
        let a = Date {
            year: 2026,
            month: 1,
            day: 10,
        };
        let b = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        assert_eq!(a.days_since(b), 9);
    }

    #[test]
    fn test_date_format() {
        let d = Date {
            year: 2026,
            month: 3,
            day: 15,
        };
        assert_eq!(d.format_short(), "2026-03-15");
        assert!(d.format_long().contains("March"));
        assert!(d.format_medium().contains("Mar"));
    }

    // === Time tests ===

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
    fn test_time_format_12h() {
        let t = Time {
            hour: 14,
            minute: 30,
        };
        assert_eq!(t.format_12h(), "2:30 PM");
        let t = Time { hour: 0, minute: 0 };
        assert_eq!(t.format_12h(), "12:00 AM");
        let t = Time {
            hour: 12,
            minute: 0,
        };
        assert_eq!(t.format_12h(), "12:00 PM");
    }

    #[test]
    fn test_time_to_from_minutes() {
        let t = Time {
            hour: 2,
            minute: 30,
        };
        assert_eq!(t.to_minutes(), 150);
        let t2 = Time::from_minutes(150);
        assert_eq!(t2.hour, 2);
        assert_eq!(t2.minute, 30);
    }

    // === Priority tests ===

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Medium);
        assert!(Priority::Medium > Priority::Low);
    }

    #[test]
    fn test_priority_labels() {
        for p in Priority::all() {
            let _ = p.label();
            let _ = p.icon();
            let _ = p.color();
        }
    }

    #[test]
    fn test_priority_from_str() {
        assert_eq!(Priority::from_str_label("low"), Some(Priority::Low));
        assert_eq!(Priority::from_str_label("HIGH"), Some(Priority::High));
        assert_eq!(Priority::from_str_label("nope"), None);
    }

    // === Category tests ===

    #[test]
    fn test_category_labels() {
        for cat in TaskCategory::all() {
            let _ = cat.label();
            let _ = cat.icon();
            let _ = cat.color();
        }
    }

    #[test]
    fn test_category_from_str() {
        assert_eq!(
            TaskCategory::from_str_label("work"),
            Some(TaskCategory::Work)
        );
        assert_eq!(
            TaskCategory::from_str_label("HEALTH"),
            Some(TaskCategory::Health)
        );
        assert_eq!(TaskCategory::from_str_label("xyz"), None);
    }

    // === Recurrence tests ===

    #[test]
    fn test_recurrence_none() {
        let r = RecurrenceRule::None;
        let d = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        assert!(r.matches(d, d));
        assert!(!r.matches(d, d.add_days(1)));
        assert!(r.next_occurrence(d).is_none());
    }

    #[test]
    fn test_recurrence_daily() {
        let r = RecurrenceRule::Daily;
        let origin = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        assert!(r.matches(origin, origin.add_days(5)));
        assert_eq!(r.next_occurrence(origin), Some(origin.add_days(1)));
    }

    #[test]
    fn test_recurrence_weekly() {
        let r = RecurrenceRule::Weekly;
        let origin = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        assert!(r.matches(origin, origin.add_days(7)));
        assert!(r.matches(origin, origin.add_days(14)));
        assert!(!r.matches(origin, origin.add_days(3)));
    }

    #[test]
    fn test_recurrence_monthly() {
        let r = RecurrenceRule::Monthly;
        let origin = Date {
            year: 2026,
            month: 1,
            day: 15,
        };
        assert!(r.matches(
            origin,
            Date {
                year: 2026,
                month: 3,
                day: 15
            }
        ));
        assert!(!r.matches(
            origin,
            Date {
                year: 2026,
                month: 3,
                day: 16
            }
        ));
    }

    #[test]
    fn test_recurrence_yearly() {
        let r = RecurrenceRule::Yearly;
        let origin = Date {
            year: 2026,
            month: 6,
            day: 15,
        };
        assert!(r.matches(
            origin,
            Date {
                year: 2027,
                month: 6,
                day: 15
            }
        ));
        assert!(!r.matches(
            origin,
            Date {
                year: 2027,
                month: 7,
                day: 15
            }
        ));
    }

    #[test]
    fn test_recurrence_custom() {
        let r = RecurrenceRule::Custom { interval_days: 3 };
        let origin = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        assert!(r.matches(origin, origin.add_days(3)));
        assert!(r.matches(origin, origin.add_days(6)));
        assert!(!r.matches(origin, origin.add_days(4)));
    }

    #[test]
    fn test_recurrence_custom_zero() {
        let r = RecurrenceRule::Custom { interval_days: 0 };
        let d = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        assert!(r.matches(d, d));
        assert!(!r.matches(d, d.add_days(1)));
        assert!(r.next_occurrence(d).is_none());
    }

    #[test]
    fn test_recurrence_before_origin() {
        let r = RecurrenceRule::Daily;
        let origin = Date {
            year: 2026,
            month: 5,
            day: 10,
        };
        assert!(!r.matches(
            origin,
            Date {
                year: 2026,
                month: 5,
                day: 9
            }
        ));
    }

    #[test]
    fn test_recurrence_serialization() {
        let rules = RecurrenceRule::all_presets();
        for rule in &rules {
            let s = rule.to_json_str();
            let parsed = RecurrenceRule::from_json_str(&s);
            assert_eq!(rule.label(), parsed.label());
        }
    }

    // === Snooze tests ===

    #[test]
    fn test_snooze_durations() {
        for s in SnoozeDuration::presets() {
            assert!(s.as_minutes() > 0);
            assert!(!s.label().is_empty());
        }
    }

    #[test]
    fn test_snooze_custom() {
        let s = SnoozeDuration::Custom { minutes: 45 };
        assert_eq!(s.as_minutes(), 45);
        assert!(s.label().contains("45"));
    }

    // === Subtask tests ===

    #[test]
    fn test_subtask_creation() {
        let st = Subtask::new("Do the thing");
        assert_eq!(st.title, "Do the thing");
        assert!(!st.completed);
    }

    // === Task tests ===

    #[test]
    fn test_task_completion_percent_empty() {
        let now = make_now();
        let t = make_task("Test", now);
        assert_eq!(t.completion_percent(), 0);
    }

    #[test]
    fn test_task_completion_percent_partial() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.subtasks = vec![
            {
                let mut s = Subtask::new("A");
                s.completed = true;
                s
            },
            Subtask::new("B"),
            Subtask::new("C"),
            Subtask::new("D"),
        ];
        assert_eq!(t.completion_percent(), 25);
    }

    #[test]
    fn test_task_completion_percent_all_done() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.completed = true;
        assert_eq!(t.completion_percent(), 100);
    }

    #[test]
    fn test_task_overdue() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(
            now.date.add_days(-1),
            Time {
                hour: 12,
                minute: 0,
            },
        ));
        assert!(t.is_overdue(now));
    }

    #[test]
    fn test_task_not_overdue_when_completed() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(
            now.date.add_days(-1),
            Time {
                hour: 12,
                minute: 0,
            },
        ));
        t.completed = true;
        assert!(!t.is_overdue(now));
    }

    #[test]
    fn test_task_due_today() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(
            now.date,
            Time {
                hour: 18,
                minute: 0,
            },
        ));
        assert!(t.is_due_today(now.date));
    }

    #[test]
    fn test_task_due_within() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(
            now.date.add_days(3),
            Time {
                hour: 10,
                minute: 0,
            },
        ));
        assert!(t.is_due_within(now.date, 7));
        assert!(!t.is_due_within(now.date, 2));
    }

    #[test]
    fn test_task_snooze() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.snooze(now, SnoozeDuration::Minutes15);
        assert!(t.is_snoozed(now));
        // 20 minutes later
        let later = DateTime::new(
            now.date,
            Time {
                hour: 10,
                minute: 50,
            },
        );
        assert!(!t.is_snoozed(later));
    }

    #[test]
    fn test_task_snooze_hour() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.snooze(now, SnoozeDuration::Hour1);
        assert!(t.is_snoozed(DateTime::new(
            now.date,
            Time {
                hour: 11,
                minute: 0
            }
        )));
        assert!(!t.is_snoozed(DateTime::new(
            now.date,
            Time {
                hour: 11,
                minute: 31
            }
        )));
    }

    #[test]
    fn test_task_matches_query() {
        let now = make_now();
        let mut t = make_task("Buy groceries", now);
        t.description = "Milk and bread".to_string();
        // Substring match: "grocer" is a substring of "groceries" (the title is
        // "Buy groceries"). matches_query does plain case-insensitive substring
        // search, not stemming, so "grocery" would NOT match "groceries".
        assert!(t.matches_query("grocer"));
        assert!(t.matches_query("milk"));
        assert!(t.matches_query("")); // Empty query matches all
        assert!(!t.matches_query("exercise"));
    }

    #[test]
    fn test_task_due_label_today() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(
            now.date,
            Time {
                hour: 14,
                minute: 0,
            },
        ));
        let label = t.due_label(now.date);
        assert!(label.contains("Today"));
    }

    #[test]
    fn test_task_due_label_tomorrow() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(
            now.date.add_days(1),
            Time { hour: 9, minute: 0 },
        ));
        let label = t.due_label(now.date);
        assert!(label.contains("Tomorrow"));
    }

    #[test]
    fn test_task_due_label_no_date() {
        let now = make_now();
        let t = make_task("Test", now);
        assert_eq!(t.due_label(now.date), "No due date");
    }

    // === TaskStore tests ===

    #[test]
    fn test_store_add_remove() {
        let now = make_now();
        let mut store = TaskStore::new();
        let id = store.add(make_task("Test", now));
        assert_eq!(store.len(), 1);
        assert!(store.get(id).is_some());
        assert!(store.remove(id));
        assert!(store.is_empty());
    }

    #[test]
    fn test_store_get_mut() {
        let now = make_now();
        let mut store = TaskStore::new();
        let id = store.add(make_task("Test", now));
        {
            let task = store.get_mut(id).unwrap();
            task.title = "Updated".to_string();
        }
        assert_eq!(store.get(id).unwrap().title, "Updated");
    }

    #[test]
    fn test_store_filter_today() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Due today", now);
        t.due = Some(DateTime::new(
            now.date,
            Time {
                hour: 18,
                minute: 0,
            },
        ));
        store.add(t);

        let mut t2 = make_task("Due tomorrow", now);
        t2.due = Some(DateTime::new(
            now.date.add_days(1),
            Time { hour: 9, minute: 0 },
        ));
        store.add(t2);

        let today = store.filtered(ViewFilter::Today, now);
        assert_eq!(today.len(), 1);
        assert_eq!(today[0].title, "Due today");
    }

    #[test]
    fn test_store_filter_overdue() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Overdue", now);
        t.due = Some(DateTime::new(
            now.date.add_days(-1),
            Time {
                hour: 12,
                minute: 0,
            },
        ));
        store.add(t);

        let mut t2 = make_task("Not overdue", now);
        t2.due = Some(DateTime::new(
            now.date.add_days(1),
            Time {
                hour: 12,
                minute: 0,
            },
        ));
        store.add(t2);

        let overdue = store.filtered(ViewFilter::Overdue, now);
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].title, "Overdue");
    }

    #[test]
    fn test_store_filter_completed() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Done", now);
        t.completed = true;
        store.add(t);
        store.add(make_task("Not done", now));

        let completed = store.filtered(ViewFilter::Completed, now);
        assert_eq!(completed.len(), 1);
    }

    #[test]
    fn test_store_filter_by_category() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t1 = make_task("Work task", now);
        t1.category = TaskCategory::Work;
        store.add(t1);
        let mut t2 = make_task("Health task", now);
        t2.category = TaskCategory::Health;
        store.add(t2);

        let work = store.filtered(ViewFilter::ByCategory(TaskCategory::Work), now);
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].title, "Work task");
    }

    #[test]
    fn test_store_sort_priority() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut low = make_task("Low", now);
        low.priority = Priority::Low;
        store.add(low);
        let mut crit = make_task("Critical", now);
        crit.priority = Priority::Critical;
        store.add(crit);

        let all: Vec<&Task> = store.all().iter().collect();
        let sorted = TaskStore::sorted(&all, SortMode::Priority);
        assert_eq!(sorted[0].title, "Critical");
        assert_eq!(sorted[1].title, "Low");
    }

    #[test]
    fn test_store_sort_alphabetical() {
        let now = make_now();
        let mut store = TaskStore::new();
        store.add(make_task("Banana", now));
        store.add(make_task("Apple", now));
        store.add(make_task("Cherry", now));

        let all: Vec<&Task> = store.all().iter().collect();
        let sorted = TaskStore::sorted(&all, SortMode::Alphabetical);
        assert_eq!(sorted[0].title, "Apple");
        assert_eq!(sorted[1].title, "Banana");
        assert_eq!(sorted[2].title, "Cherry");
    }

    #[test]
    fn test_store_search() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Team Meeting", now);
        t.description = "Weekly sync".to_string();
        store.add(t);
        store.add(make_task("Buy lunch", now));

        assert_eq!(store.search("meeting").len(), 1);
        assert_eq!(store.search("sync").len(), 1);
        assert_eq!(store.search("xyz").len(), 0);
        assert_eq!(store.search("").len(), 2); // Empty matches all
    }

    #[test]
    fn test_store_complete_task() {
        let now = make_now();
        let mut store = TaskStore::new();
        let id = store.add(make_task("Test", now));
        assert!(store.complete_task(id, now));
        assert!(store.get(id).unwrap().completed);
        assert!(store.get(id).unwrap().completed_at.is_some());
    }

    #[test]
    fn test_store_uncomplete_task() {
        let now = make_now();
        let mut store = TaskStore::new();
        let id = store.add(make_task("Test", now));
        store.complete_task(id, now);
        assert!(store.uncomplete_task(id));
        assert!(!store.get(id).unwrap().completed);
    }

    #[test]
    fn test_store_toggle_subtask() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Test", now);
        t.subtasks = vec![Subtask::new("Sub 1"), Subtask::new("Sub 2")];
        let id = store.add(t);

        assert_eq!(store.toggle_subtask(id, 0), Some(true));
        assert!(store.get(id).unwrap().subtasks[0].completed);
        assert_eq!(store.toggle_subtask(id, 0), Some(false));
        assert!(!store.get(id).unwrap().subtasks[0].completed);
    }

    #[test]
    fn test_store_add_remove_subtask() {
        let now = make_now();
        let mut store = TaskStore::new();
        let id = store.add(make_task("Test", now));
        assert!(store.add_subtask(id, "Step 1"));
        assert!(store.add_subtask(id, "Step 2"));
        assert_eq!(store.get(id).unwrap().subtasks.len(), 2);
        assert!(store.remove_subtask(id, 0));
        assert_eq!(store.get(id).unwrap().subtasks.len(), 1);
        assert_eq!(store.get(id).unwrap().subtasks[0].title, "Step 2");
    }

    #[test]
    fn test_store_counts() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t1 = make_task("Today", now);
        t1.due = Some(now);
        t1.category = TaskCategory::Work;
        store.add(t1);

        let mut t2 = make_task("Overdue", now);
        t2.due = Some(DateTime::new(
            now.date.add_days(-2),
            Time {
                hour: 12,
                minute: 0,
            },
        ));
        store.add(t2);

        let mut t3 = make_task("Done", now);
        t3.completed = true;
        store.add(t3);

        assert_eq!(store.count_today(now.date), 1);
        assert_eq!(store.count_overdue(now), 1);
        assert_eq!(store.count_completed(), 1);
        assert_eq!(store.count_by_category(TaskCategory::Work), 1);
    }

    #[test]
    fn test_store_due_now() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Alert!", now);
        t.due = Some(now); // Due right now
        store.add(t);

        let mut t2 = make_task("Not yet", now);
        t2.due = Some(DateTime::new(
            now.date.add_days(1),
            Time {
                hour: 12,
                minute: 0,
            },
        ));
        store.add(t2);

        let due = store.due_now(now);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].title, "Alert!");
    }

    #[test]
    fn test_store_empty() {
        let store = TaskStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        let now = make_now();
        assert!(store.filtered(ViewFilter::All, now).is_empty());
        assert!(store.search("test").is_empty());
    }

    // === JSON import/export tests ===

    #[test]
    fn test_json_export_format() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Test task", now);
        t.priority = Priority::High;
        t.category = TaskCategory::Finance;
        store.add(t);

        let json = store.export_json();
        assert!(json.contains("\"title\":\"Test task\""));
        assert!(json.contains("\"priority\":\"High\""));
        assert!(json.contains("\"category\":\"Finance\""));
    }

    /// Export then import must give back the task that was exported.
    ///
    /// This was a list of five assertions over a thirteen-field struct, built
    /// from a task whose other eight fields were all at their defaults — so it
    /// passed for as long as `created`, `completed_at` and `snoozed_until`
    /// were written nowhere at all. Every field below is deliberately *not*
    /// its default, and the comparison is of whole values, so there is no list
    /// left to fall behind the struct and no default left to hide a drop.
    #[test]
    fn test_json_roundtrip() {
        let now = make_now();
        let mut store = TaskStore::new();

        // A struct literal rather than `make_task` plus assignments: this is
        // the third place a new `Task` field has to be considered, and it
        // should stop compiling here too rather than quietly test a default.
        let t = Task {
            id: 0, // replaced by `add`; see `Task::to_json`
            title: "Roundtrip test".to_string(),
            description: "Some description".to_string(),
            // Struct literals rather than `Date::new(..).expect(..)`, as
            // `make_now` above does: the validating constructor returns an
            // `Option` whose `None` arm is dead weight for a date written out
            // here by hand, and the literal has the property this whole test
            // is about — it names every field, so a new one stops it
            // compiling rather than quietly defaulting.
            due: Some(DateTime::new(
                Date {
                    year: 2026,
                    month: 6,
                    day: 1,
                },
                Time {
                    hour: 9,
                    minute: 30,
                },
            )),
            created: DateTime::new(
                Date {
                    year: 2025,
                    month: 1,
                    day: 2,
                },
                Time {
                    hour: 13,
                    minute: 45,
                },
            ),
            priority: Priority::Critical,
            category: TaskCategory::Health,
            recurrence: RecurrenceRule::Weekly,
            completed: true,
            completed_at: Some(DateTime::new(
                Date {
                    year: 2026,
                    month: 5,
                    day: 17,
                },
                Time { hour: 0, minute: 5 },
            )),
            snoozed_until: Some(DateTime::new(
                Date {
                    year: 2026,
                    month: 12,
                    day: 25,
                },
                Time {
                    hour: 12,
                    minute: 0,
                },
            )),
            subtasks: vec![
                Subtask {
                    title: "Step A".to_string(),
                    completed: true,
                },
                Subtask {
                    title: "Step B".to_string(),
                    completed: false,
                },
            ],
            notes: "Some notes".to_string(),
        };
        store.add(t);

        let json = store.export_json();

        let mut store2 = TaskStore::new();
        let count = store2.import_json(&json, now);
        assert_eq!(count, 1);

        // Both stores number from the same counter, so the ids match without
        // special handling and the comparison can be of the whole store.
        assert_eq!(
            store2.all(),
            store.all(),
            "a task did not survive export and re-import unchanged"
        );

        // `created` falls back to the caller's "now" only when the file does
        // not carry one, so `now` is a value the imported task must *not*
        // have. Without this, a writer that dropped `created` and a test whose
        // task happened to be created at `now` would agree with each other.
        assert_ne!(
            store2.all().first().map(|t| t.created),
            Some(now),
            "`created` came back as the import time, so it was not exported"
        );
    }

    /// The three optional datetimes have to survive as `None` as well.
    ///
    /// `test_json_roundtrip` sets all three, so it walks the `Some` path and
    /// nothing else — and `None` is the side where a key is written as `null`
    /// and a substring-scanning reader can pick up the *next* task's value
    /// instead. A default task also covers empty strings and an empty subtask
    /// list.
    /// A subtask's `completed` must not answer for its parent's.
    ///
    /// The regression test for the reader's worst defect: `json_bool_value`
    /// used `str::contains` over the whole task object, and a task carries its
    /// subtasks inside that object. Ticking off one step of an unfinished task
    /// and re-importing marked the whole task done — the parent's own
    /// `"completed":false` was right there, earlier in the same object, and
    /// lost to a `contains` that asked about `true` first.
    ///
    /// Note this cannot be caught by a round-trip test that sets the parent
    /// and the subtask to the same value, which is why the two states are
    /// deliberately opposed here.
    #[test]
    fn a_completed_subtask_does_not_complete_its_parent() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Not done", now);
        t.completed = false;
        t.subtasks = vec![
            Subtask {
                title: "Step A".to_string(),
                completed: true,
            },
            Subtask {
                title: "Step B".to_string(),
                completed: false,
            },
        ];
        store.add(t);

        let json = store.export_json();
        let mut store2 = TaskStore::new();
        assert_eq!(store2.import_json(&json, now), 1);
        assert_eq!(
            store2.all(),
            store.all(),
            "an unfinished task with a finished step did not survive import"
        );
    }

    /// A bracket in a subtask's title must not truncate the list.
    ///
    /// The array used to be delimited by the first `]` after the `subtasks`
    /// key, so a step whose title contained one ended the array there and
    /// every step after it was dropped on import — a silent partial loss,
    /// which is worse than a refused import because the task still looks
    /// intact. `[urgent]` is not a contrived title.
    #[test]
    fn a_bracket_in_a_step_title_does_not_truncate_the_list() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Shopping", now);
        t.subtasks = vec![
            Subtask {
                title: "Buy milk [urgent]".to_string(),
                completed: false,
            },
            Subtask {
                title: "Buy bread {also}".to_string(),
                completed: true,
            },
            Subtask {
                title: "Pay, then leave".to_string(),
                completed: false,
            },
        ];
        store.add(t);

        let json = store.export_json();
        let mut store2 = TaskStore::new();
        assert_eq!(store2.import_json(&json, now), 1);
        // Checked before the whole-value comparison below because "3 steps
        // became 1" names the bracket bug directly, where a whole-`Task` diff
        // buries it in thirteen fields of otherwise-matching output.
        let imported = store2.all();
        assert_eq!(
            imported.first().map(|t| t.subtasks.len()),
            Some(3),
            "steps were dropped: {:?}",
            imported.first().map(|t| &t.subtasks)
        );
        assert_eq!(
            store2.all(),
            store.all(),
            "a step whose title contains JSON punctuation did not survive"
        );
    }

    /// Text that looks like JSON is text.
    ///
    /// The reader walks the object byte by byte, so every `{`, `[`, `"` and
    /// `,` it meets inside a string has to be stepped over rather than counted
    /// as structure. A note is free-form user text and is exactly where such
    /// characters turn up — including a `"completed":true` a user could
    /// plausibly paste in from an export they were looking at.
    #[test]
    fn a_note_that_looks_like_json_is_still_a_note() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Meta", now);
        t.completed = false;
        t.notes = r#"see {"completed":true, "subtasks":[ }] and a "quote"#.to_string();
        t.description = "ends with a brace }".to_string();
        store.add(t);

        let json = store.export_json();
        let mut store2 = TaskStore::new();
        assert_eq!(store2.import_json(&json, now), 1);
        assert_eq!(
            store2.all(),
            store.all(),
            "a note containing JSON punctuation was read as JSON"
        );
    }

    #[test]
    fn an_untouched_task_survives_the_round_trip_too() {
        let now = make_now();
        let mut store = TaskStore::new();
        store.add(make_task("Bare task", now));

        let json = store.export_json();
        let mut store2 = TaskStore::new();
        assert_eq!(store2.import_json(&json, now), 1);
        assert_eq!(
            store2.all(),
            store.all(),
            "a task with no due date, no completion and no snooze did not \
             survive export and re-import"
        );
    }

    #[test]
    fn test_json_escape_special_chars() {
        let escaped = escape_json("Hello \"world\"\nnew line");
        assert!(escaped.contains("\\\""));
        assert!(escaped.contains("\\n"));
        let unescaped = unescape_json(&escaped);
        assert_eq!(unescaped, "Hello \"world\"\nnew line");
    }

    /// The test above passes against a broken decoder, because none of its
    /// sample text contains a *literal* backslash — the only input that
    /// distinguishes a correct decoder from a `str::replace` chain. These do.
    #[test]
    fn a_literal_backslash_in_a_task_survives_a_save_and_reload() {
        for text in [
            r"a\nb",         // decoded as a newline by a replace-chain
            r"C:\temp",      // decoded as a tab by a replace-chain
            r"C:\new\table", // both, in one path
            r"a\\b",
            r"trailing\",
            r"\u0041 is not an A here",
        ] {
            assert_eq!(
                unescape_json(&escape_json(text)),
                text,
                "round trip corrupted {text:?}"
            );
        }
    }

    /// Corruption of this kind compounds: the file is rewritten on every edit,
    /// so a single round trip can look survivable while five do not.
    #[test]
    fn repeated_saves_do_not_let_a_task_decay() {
        let original = r"C:\new\table and a\nb";
        let mut text = original.to_string();
        for pass in 1..=5 {
            text = unescape_json(&escape_json(&text));
            assert_eq!(text, original, "task text drifted on save {pass}");
        }
    }

    /// A control character has no short escape and must not be emitted raw —
    /// that produces invalid JSON, so the app could not reload its own file.
    #[test]
    fn a_control_character_in_a_note_does_not_produce_invalid_json() {
        let escaped = escape_json("note with a bell \u{7} in it");
        assert!(
            !escaped.chars().any(|c| c < '\u{20}'),
            "raw control character left in JSON output: {escaped:?}"
        );
        assert_eq!(unescape_json(&escaped), "note with a bell \u{7} in it");
    }

    #[test]
    fn test_json_import_empty() {
        let now = make_now();
        let mut store = TaskStore::new();
        assert_eq!(store.import_json("", now), 0);
        assert_eq!(store.import_json("{}", now), 0);
        assert_eq!(store.import_json("garbage", now), 0);
    }

    #[test]
    fn test_find_matching_brace() {
        assert_eq!(find_matching_brace("{\"a\":1}"), Some(6));
        assert_eq!(find_matching_brace("{\"a\":{\"b\":2}}"), Some(12));
        assert_eq!(find_matching_brace("{\"a\":\"}\"}"), Some(8));
        assert!(find_matching_brace("{unclosed").is_none());
    }

    #[test]
    fn test_json_string_value() {
        let json = r#"{"title":"Hello","desc":"World"}"#;
        assert_eq!(json_string_value(json, "title"), Some("Hello"));
        assert_eq!(json_string_value(json, "desc"), Some("World"));
        assert!(json_string_value(json, "missing").is_none());
    }

    #[test]
    fn test_json_bool_value() {
        let json = r#"{"completed":true,"active":false}"#;
        assert_eq!(json_bool_value(json, "completed"), Some(true));
        assert_eq!(json_bool_value(json, "active"), Some(false));
        assert!(json_bool_value(json, "missing").is_none());
    }

    #[test]
    fn test_parse_time_12h() {
        assert_eq!(
            parse_time_12h("3:00 PM"),
            Some(Time {
                hour: 15,
                minute: 0
            })
        );
        assert_eq!(
            parse_time_12h("12:00 AM"),
            Some(Time { hour: 0, minute: 0 })
        );
        assert_eq!(
            parse_time_12h("12:30 PM"),
            Some(Time {
                hour: 12,
                minute: 30
            })
        );
        assert_eq!(
            parse_time_12h("11:59 AM"),
            Some(Time {
                hour: 11,
                minute: 59
            })
        );
        assert!(parse_time_12h("invalid").is_none());
    }

    #[test]
    fn test_parse_datetime_short() {
        let dt = parse_datetime_short("2026-05-18 3:00 PM").unwrap();
        assert_eq!(dt.date.year, 2026);
        assert_eq!(dt.date.month, 5);
        assert_eq!(dt.date.day, 18);
        assert_eq!(dt.time.hour, 15);
        assert_eq!(dt.time.minute, 0);
    }

    // === Notification tests ===

    #[test]
    fn test_app_notifications() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);

        let mut t = make_task("Due now", now);
        t.due = Some(now);
        app.store.add(t);

        app.check_notifications();
        assert_eq!(app.active_notifications().len(), 1);

        // Second check should not duplicate
        app.check_notifications();
        assert_eq!(app.active_notifications().len(), 1);
    }

    #[test]
    fn test_app_dismiss_notification() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);

        let mut t = make_task("Due now", now);
        t.due = Some(now);
        let id = app.store.add(t);

        app.check_notifications();
        assert_eq!(app.active_notifications().len(), 1);

        app.dismiss_notification(id);
        assert_eq!(app.active_notifications().len(), 0);
    }

    #[test]
    fn test_app_dismiss_all() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);

        let mut t1 = make_task("Due 1", now);
        t1.due = Some(now);
        app.store.add(t1);
        let mut t2 = make_task("Due 2", now);
        t2.due = Some(now);
        app.store.add(t2);

        app.check_notifications();
        assert_eq!(app.active_notifications().len(), 2);

        app.dismiss_all_notifications();
        assert_eq!(app.active_notifications().len(), 0);
    }

    // === App rendering tests ===

    #[test]
    fn test_render_all_views() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        sample_tasks(&mut app.store, now);

        for view in ViewFilter::standard_views() {
            app.view = *view;
            let cmds = app.render_commands();
            assert!(!cmds.is_empty(), "View {view:?} produced no commands");
        }
    }

    #[test]
    fn test_render_category_views() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        sample_tasks(&mut app.store, now);

        for cat in TaskCategory::all() {
            app.view = ViewFilter::ByCategory(*cat);
            let cmds = app.render_commands();
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn test_render_with_selection() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        sample_tasks(&mut app.store, now);
        let first_id = app.store.all()[0].id;
        app.select_task(first_id);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    const LONG_NOTES: &str = "Bring the signed copy and the two spare batteries; \
        the office does not stock them and the courier desk closes at four, so \
        anything not handed over by then waits until Monday.";

    /// An app with one selected task carrying a long description and notes.
    fn app_with_prose_task() -> RemindersApp {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        let mut task = Task::new(0, "Collect the parcel", now);
        task.description = LONG_NOTES.to_string();
        task.notes = LONG_NOTES.to_string();
        let id = app.store.add(task);
        app.select_task(id);
        app
    }

    /// The `(y, text)` of every prose line drawn in the detail panel.
    fn prose_lines(app: &RemindersApp) -> Vec<(f32, String)> {
        app.render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    font_size,
                    color,
                    ..
                } if (font_size - PROSE_FONT_SIZE).abs() < 0.01 && color == SUBTEXT0 => {
                    Some((y, text))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_long_description_is_wrapped_not_truncated() {
        // `RenderCommand::Text` clips at `max_width`, so these fields used to
        // reach the user as their first line and nothing else.
        let app = app_with_prose_task();
        let lines = prose_lines(&app);
        assert!(
            lines.len() > 2,
            "two multi-line fields produced only {} line(s)",
            lines.len()
        );
        let drawn: String = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for word in LONG_NOTES.split_whitespace() {
            assert!(drawn.contains(word), "the detail panel lost {word:?}");
        }
    }

    #[test]
    fn detail_panel_fields_do_not_overlap_each_other() {
        // The panel is a running cursor, so a field that wrapped without
        // advancing it would be drawn over by the one below.
        let app = app_with_prose_task();
        let mut lines = prose_lines(&app);
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in lines.windows(2) {
            let (top, bottom) = (&pair[0], &pair[1]);
            assert!(
                bottom.0 - top.0 >= PROSE_LINE_HEIGHT - 0.01,
                "{:?} at {} and {:?} at {} are less than a line apart",
                top.1,
                top.0,
                bottom.1,
                bottom.0
            );
        }
    }

    #[test]
    fn test_render_without_sidebar() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        app.sidebar_visible = false;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_notifications() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        let mut t = make_task("Due now!", now);
        t.due = Some(now);
        app.store.add(t);
        app.check_notifications();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_state() {
        let now = make_now();
        let app = RemindersApp::new(1100.0, 720.0, now);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    // === Sort cycling test ===

    #[test]
    fn test_sort_cycle() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        assert_eq!(app.sort_mode, SortMode::DueDate);
        app.cycle_sort();
        assert_eq!(app.sort_mode, SortMode::Priority);
        app.cycle_sort();
        assert_eq!(app.sort_mode, SortMode::CreationDate);
        app.cycle_sort();
        assert_eq!(app.sort_mode, SortMode::Alphabetical);
        app.cycle_sort();
        assert_eq!(app.sort_mode, SortMode::DueDate);
    }

    // === DateTime tests ===

    #[test]
    fn test_datetime_minutes_since() {
        let a = DateTime::new(
            Date {
                year: 2026,
                month: 5,
                day: 18,
            },
            Time {
                hour: 12,
                minute: 0,
            },
        );
        let b = DateTime::new(
            Date {
                year: 2026,
                month: 5,
                day: 18,
            },
            Time {
                hour: 10,
                minute: 0,
            },
        );
        assert_eq!(a.minutes_since(b), 120);
    }

    #[test]
    fn test_datetime_minutes_since_cross_day() {
        let a = DateTime::new(
            Date {
                year: 2026,
                month: 5,
                day: 19,
            },
            Time { hour: 1, minute: 0 },
        );
        let b = DateTime::new(
            Date {
                year: 2026,
                month: 5,
                day: 18,
            },
            Time {
                hour: 23,
                minute: 0,
            },
        );
        assert_eq!(a.minutes_since(b), 120);
    }

    // === ViewFilter tests ===

    #[test]
    fn test_view_labels() {
        for v in ViewFilter::standard_views() {
            let _ = v.label();
            let _ = v.icon();
            let _ = v.color();
        }
    }

    // === Month name tests ===

    #[test]
    fn test_month_names() {
        assert_eq!(month_name(1), "January");
        assert_eq!(month_name(12), "December");
        assert_eq!(month_short(3), "Mar");
    }

    // === SortMode tests ===

    #[test]
    fn test_sort_mode_labels() {
        for m in SortMode::all() {
            let _ = m.label();
        }
    }
}
