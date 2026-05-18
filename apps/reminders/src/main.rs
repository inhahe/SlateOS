//! `OurOS` Reminders & Tasks Application
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
#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

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

    /// Day of week: 0=Sunday, 1=Monday, ..., 6=Saturday (Zeller's congruence).
    pub fn day_of_week(self) -> u32 {
        let mut y = self.year;
        let mut m = self.month as i32;
        if m < 3 {
            m += 12;
            y -= 1;
        }
        let q = self.day as i32;
        let k = y % 100;
        let j = y / 100;
        let h = (q + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 - 2 * j) % 7;
        ((h + 6) % 7) as u32
    }

    pub fn day_of_week_name(self) -> &'static str {
        match self.day_of_week() {
            0 => "Sunday",
            1 => "Monday",
            2 => "Tuesday",
            3 => "Wednesday",
            4 => "Thursday",
            5 => "Friday",
            6 => "Saturday",
            _ => "Unknown",
        }
    }

    pub fn day_of_week_short(self) -> &'static str {
        match self.day_of_week() {
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

    pub fn month_name(self) -> &'static str {
        month_name(self.month)
    }

    pub fn month_short(self) -> &'static str {
        month_short(self.month)
    }

    /// Add days (positive or negative).
    pub fn add_days(self, n: i32) -> Self {
        let mut y = self.year;
        let mut m = self.month;
        let mut d = self.day as i32 + n;

        while d > days_in_month(y, m) as i32 {
            d -= days_in_month(y, m) as i32;
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
        }
        while d < 1 {
            m = if m == 1 { 12 } else { m - 1 };
            if m == 12 {
                y -= 1;
            }
            d += days_in_month(y, m) as i32;
        }

        Self { year: y, month: m, day: d as u32 }
    }

    /// Add months (clamping day to valid range).
    pub fn add_months(self, n: i32) -> Self {
        let total_months = (self.year * 12 + self.month as i32 - 1) + n;
        let new_year = total_months.div_euclid(12);
        let new_month = (total_months.rem_euclid(12) + 1) as u32;
        let max_d = days_in_month(new_year, new_month);
        Self { year: new_year, month: new_month, day: self.day.min(max_d) }
    }

    /// Difference in days (self - other), approximate via Julian day number.
    pub fn days_since(self, other: Self) -> i64 {
        self.to_day_number() - other.to_day_number()
    }

    fn to_day_number(self) -> i64 {
        let mut y = i64::from(self.year);
        let mut m = i64::from(self.month);
        if m <= 2 {
            y -= 1;
            m += 12;
        }
        let d = i64::from(self.day);
        365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + d - 1
    }

    pub fn format_short(self) -> String {
        format!("{}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn format_long(self) -> String {
        format!("{}, {} {}, {}", self.day_of_week_name(), self.month_name(), self.day, self.year)
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
        self.hour * 60 + self.minute
    }

    pub fn from_minutes(total: u32) -> Self {
        Self { hour: (total / 60).min(23), minute: total % 60 }
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
        format!("{} at {}", self.date.format_medium(), self.time.format_12h())
    }

    /// Difference in minutes (approximate, same-month only for simplicity).
    pub fn minutes_since(self, other: Self) -> i64 {
        let day_diff = self.date.days_since(other.date);
        let minute_diff = i64::from(self.time.to_minutes()) - i64::from(other.time.to_minutes());
        day_diff * 1440 + minute_diff
    }
}

// ============================================================================
// Date helper functions
// ============================================================================

pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap_year(year) { 29 } else { 28 },
        _ => 0,
    }
}

pub fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January", 2 => "February", 3 => "March",
        4 => "April", 5 => "May", 6 => "June",
        7 => "July", 8 => "August", 9 => "September",
        10 => "October", 11 => "November", 12 => "December",
        _ => "Unknown",
    }
}

pub fn month_short(month: u32) -> &'static str {
    match month {
        1 => "Jan", 2 => "Feb", 3 => "Mar",
        4 => "Apr", 5 => "May", 6 => "Jun",
        7 => "Jul", 8 => "Aug", 9 => "Sep",
        10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "???",
    }
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
            Self::Work, Self::Personal, Self::Health, Self::Finance,
            Self::Shopping, Self::Education, Self::Home, Self::Social,
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
            Self::Monthly => {
                origin.day == check.day && check >= origin
            }
            Self::Yearly => {
                origin.month == check.month && origin.day == check.day && check >= origin
            }
            Self::Custom { interval_days } => {
                if *interval_days == 0 {
                    return origin == check;
                }
                let diff = check.days_since(origin);
                diff >= 0 && diff % i64::from(*interval_days) == 0
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
                    return Self::Custom { interval_days: days };
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
        &[Self::Minutes5, Self::Minutes15, Self::Minutes30, Self::Hour1]
    }
}

// ============================================================================
// Subtask (for progress tracking)
// ============================================================================

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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
        (done * 100).checked_div(total).unwrap_or(0)
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
        let total_minutes = now.time.to_minutes() + duration.as_minutes();
        let extra_days = total_minutes / 1440;
        let remaining = total_minutes % 1440;
        let new_date = now.date.add_days(extra_days as i32);
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
                format!("{} days ago", -diff)
            } else if diff <= 7 {
                format!("{} at {}", due.date.day_of_week_name(), due.time.format_12h())
            } else {
                due.format_medium()
            }
        } else {
            "No due date".to_string()
        }
    }

    /// Simple JSON export for a single task.
    pub fn to_json(&self) -> String {
        let due_str = if let Some(d) = self.due {
            format!("\"{}\"", d.format_short())
        } else {
            "null".to_string()
        };
        let subtask_json: Vec<String> = self.subtasks.iter().map(|s| {
            format!("{{\"title\":\"{}\",\"completed\":{}}}", escape_json(&s.title), s.completed)
        }).collect();

        format!(
            "{{\"id\":{},\"title\":\"{}\",\"description\":\"{}\",\"due\":{},\
             \"priority\":\"{}\",\"category\":\"{}\",\"recurrence\":\"{}\",\
             \"completed\":{},\"subtasks\":[{}],\"notes\":\"{}\"}}",
            self.id,
            escape_json(&self.title),
            escape_json(&self.description),
            due_str,
            self.priority.label(),
            self.category.label(),
            self.recurrence.to_json_str(),
            self.completed,
            subtask_json.join(","),
            escape_json(&self.notes),
        )
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn unescape_json(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
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
        &[Self::DueDate, Self::Priority, Self::CreationDate, Self::Alphabetical]
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
        &[Self::Today, Self::Upcoming, Self::All, Self::Overdue, Self::Completed]
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
        self.next_id += 1;
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
            ViewFilter::Today => self.tasks.iter()
                .filter(|t| !t.completed && t.is_due_today(today))
                .collect(),
            ViewFilter::Upcoming => self.tasks.iter()
                .filter(|t| !t.completed && t.is_due_within(today, 7))
                .collect(),
            ViewFilter::All => self.tasks.iter()
                .filter(|t| !t.completed)
                .collect(),
            ViewFilter::Overdue => self.tasks.iter()
                .filter(|t| t.is_overdue(now))
                .collect(),
            ViewFilter::Completed => self.tasks.iter()
                .filter(|t| t.completed)
                .collect(),
            ViewFilter::ByCategory(cat) => self.tasks.iter()
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
        self.tasks.iter().filter(|t| t.matches_query(query)).collect()
    }

    /// Count tasks by category.
    pub fn count_by_category(&self, cat: TaskCategory) -> usize {
        self.tasks.iter().filter(|t| !t.completed && t.category == cat).count()
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
        self.tasks.iter().filter(|t| {
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
        }).collect()
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
                    count += 1;
                }
                remaining = &remaining[end + 1..];
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
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Extract a JSON string value for a given key from a flat JSON object string.
fn json_string_value<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("\"{key}\":\"");
    let start = json.find(&pattern)?;
    let after_key = start + pattern.len();
    let rest = &json[after_key..];
    // Find unescaped closing quote
    let mut escaped = false;
    for (i, ch) in rest.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Some(&rest[..i]);
        }
    }
    None
}

/// Extract a JSON boolean value for a given key.
fn json_bool_value(json: &str, key: &str) -> Option<bool> {
    let pattern_true = format!("\"{key}\":true");
    let pattern_false = format!("\"{key}\":false");
    if json.contains(&pattern_true) {
        Some(true)
    } else if json.contains(&pattern_false) {
        Some(false)
    } else {
        None
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

    let mut task = Task::new(0, &unescape_json(title), default_created);
    task.description = unescape_json(description);
    task.due = due;
    task.priority = priority;
    task.category = category;
    task.recurrence = recurrence;
    task.completed = completed;
    task.subtasks = subtasks;
    task.notes = unescape_json(notes);

    Some(task)
}

/// Parse a short datetime string like "2026-05-18 3:00 PM".
fn parse_datetime_short(s: &str) -> Option<DateTime> {
    let parts: Vec<&str> = s.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return None;
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() < 3 {
        return None;
    }
    let year = date_parts[0].parse::<i32>().ok()?;
    let month = date_parts[1].parse::<u32>().ok()?;
    let day = date_parts[2].parse::<u32>().ok()?;
    let date = Date::new(year, month, day)?;

    let time_str = parts[1].trim();
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
    let time_part = s[..s.len() - 2].trim();
    let colon = time_part.find(':')?;
    let hour_raw = time_part[..colon].parse::<u32>().ok()?;
    let minute = time_part[colon + 1..].parse::<u32>().ok()?;

    let hour = if is_am {
        if hour_raw == 12 { 0 } else { hour_raw }
    } else if hour_raw == 12 {
        12
    } else {
        hour_raw + 12
    };

    Time::new(hour, minute)
}

/// Parse subtasks from within a task JSON object.
fn parse_subtasks_json(json: &str) -> Vec<Subtask> {
    let mut result = Vec::new();
    let marker = "\"subtasks\":[";
    let Some(pos) = json.find(marker) else { return result };
    let start = pos + marker.len();
    let rest = &json[start..];
    // Find the matching ]
    let Some(end) = rest.find(']') else { return result };
    let array_content = &rest[..end];
    if array_content.trim().is_empty() {
        return result;
    }

    // Split on },{ boundaries
    let mut remaining = array_content;
    while !remaining.is_empty() {
        if let Some(brace_end) = find_matching_brace(remaining.trim_start()) {
            let trimmed = remaining.trim_start();
            let obj = &trimmed[..=brace_end];
            let title = json_string_value(obj, "title").unwrap_or("");
            let completed = json_bool_value(obj, "completed").unwrap_or(false);
            let mut subtask = Subtask::new(&unescape_json(title));
            subtask.completed = completed;
            result.push(subtask);
            remaining = &trimmed[brace_end + 1..];
            // Skip comma
            remaining = remaining.trim_start_matches(',');
        } else {
            break;
        }
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
            let already = self.notifications.iter().any(|n| n.task_id == task.id && !n.dismissed);
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
            search_results.into_iter().filter(|t| {
                match self.view {
                    ViewFilter::All => !t.completed,
                    ViewFilter::Today => !t.completed && t.is_due_today(self.today),
                    ViewFilter::Upcoming => !t.completed && t.is_due_within(self.today, 7),
                    ViewFilter::Overdue => t.is_overdue(self.now),
                    ViewFilter::Completed => t.completed,
                    ViewFilter::ByCategory(cat) => !t.completed && t.category == cat,
                }
            }).collect()
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

    /// Render the full application UI into a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: BASE, corner_radii: CornerRadii::ZERO,
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
            self.render_detail_panel(&mut cmds, detail_x, content_y, DETAIL_PANEL_WIDTH, content_h);
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
            x: 0.0, y: 0.0, width: self.width, height: total_h,
            color: CRUST, corner_radii: CornerRadii::ZERO,
        });

        for (i, notif) in active.iter().enumerate() {
            let y = i as f32 * NOTIFICATION_HEIGHT;

            // Accent bar
            cmds.push(RenderCommand::FillRect {
                x: 0.0, y, width: 4.0, height: NOTIFICATION_HEIGHT,
                color: PEACH, corner_radii: CornerRadii::ZERO,
            });

            // Bell icon placeholder
            cmds.push(RenderCommand::Text {
                x: 16.0, y: y + 14.0,
                text: "[!]".to_string(),
                font_size: 16.0, color: PEACH,
                font_weight: FontWeightHint::Bold, max_width: Some(30.0),
            });

            // Message
            cmds.push(RenderCommand::Text {
                x: 48.0, y: y + 14.0,
                text: notif.message.clone(),
                font_size: 14.0, color: TEXT,
                font_weight: FontWeightHint::Bold, max_width: Some(self.width - 200.0),
            });

            // Dismiss button
            cmds.push(RenderCommand::FillRect {
                x: self.width - 100.0, y: y + 10.0, width: 80.0, height: 28.0,
                color: SURFACE0, corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: self.width - 88.0, y: y + 16.0,
                text: "Dismiss".to_string(),
                font_size: 11.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: Some(70.0),
            });

            // Snooze button
            cmds.push(RenderCommand::FillRect {
                x: self.width - 200.0, y: y + 10.0, width: 80.0, height: 28.0,
                color: SURFACE0, corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: self.width - 190.0, y: y + 16.0,
                text: "Snooze".to_string(),
                font_size: 11.0, color: BLUE,
                font_weight: FontWeightHint::Regular, max_width: Some(70.0),
            });
        }

        total_h
    }

    /// Render the top header bar.
    fn render_header(&self, cmds: &mut Vec<RenderCommand>, y_offset: f32) {
        // Header background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: y_offset, width: self.width, height: HEADER_HEIGHT,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        // Bottom border
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: y_offset + HEADER_HEIGHT - 1.0,
            width: self.width, height: 1.0,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: PADDING, y: y_offset + 8.0,
            text: "Reminders".to_string(),
            font_size: 20.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold, max_width: Some(150.0),
        });

        // Current view label
        cmds.push(RenderCommand::Text {
            x: PADDING, y: y_offset + 34.0,
            text: self.view.label(),
            font_size: 12.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: Some(200.0),
        });

        // Search box
        let search_x = 240.0;
        let search_w = 300.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x, y: y_offset + 12.0, width: search_w, height: 32.0,
            color: SURFACE0, corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: search_x, y: y_offset + 12.0, width: search_w, height: 32.0,
            color: SURFACE1, line_width: 1.0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        let search_text = if self.search_query.is_empty() {
            "Search tasks...".to_string()
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() { OVERLAY0 } else { TEXT };
        cmds.push(RenderCommand::Text {
            x: search_x + 12.0, y: y_offset + 20.0,
            text: search_text, font_size: 13.0, color: search_color,
            font_weight: FontWeightHint::Regular, max_width: Some(search_w - 24.0),
        });

        // Sort indicator
        let sort_x = search_x + search_w + 20.0;
        cmds.push(RenderCommand::FillRect {
            x: sort_x, y: y_offset + 12.0, width: 100.0, height: 32.0,
            color: SURFACE0, corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: sort_x + 8.0, y: y_offset + 20.0,
            text: format!("Sort: {}", self.sort_mode.label()),
            font_size: 11.0, color: SUBTEXT1,
            font_weight: FontWeightHint::Regular, max_width: Some(90.0),
        });

        // Task count
        let tasks = self.current_tasks();
        let count_text = format!("{} task{}", tasks.len(), if tasks.len() == 1 { "" } else { "s" });
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0, y: y_offset + 20.0,
            text: count_text, font_size: 13.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: Some(110.0),
        });
    }

    /// Render the left sidebar with views and categories.
    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        // Right border
        cmds.push(RenderCommand::FillRect {
            x: x + w - 1.0, y, width: 1.0, height: h,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });

        let mut row_y = y + PADDING;

        // Views section
        cmds.push(RenderCommand::Text {
            x: x + PADDING, y: row_y,
            text: "VIEWS".to_string(),
            font_size: 10.0, color: OVERLAY0,
            font_weight: FontWeightHint::Bold, max_width: Some(w - PADDING * 2.0),
        });
        row_y += 20.0;

        for view in ViewFilter::standard_views() {
            let is_active = self.view == *view;
            let bg_color = if is_active { SURFACE0 } else { MANTLE };
            let text_color = if is_active { view.color() } else { SUBTEXT1 };

            cmds.push(RenderCommand::FillRect {
                x: x + 4.0, y: row_y, width: w - 8.0, height: 30.0,
                color: bg_color, corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            // Icon
            cmds.push(RenderCommand::Text {
                x: x + PADDING, y: row_y + 8.0,
                text: view.icon().to_string(),
                font_size: 11.0, color: text_color,
                font_weight: FontWeightHint::Bold, max_width: Some(30.0),
            });

            // Label
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 30.0, y: row_y + 8.0,
                text: view.label(),
                font_size: 12.0, color: text_color,
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(w - PADDING * 2.0 - 60.0),
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
                let badge_w = 10.0 + badge_text.len() as f32 * 7.0;
                cmds.push(RenderCommand::FillRect {
                    x: x + w - badge_w - 12.0, y: row_y + 6.0,
                    width: badge_w, height: 18.0,
                    color: view.color(),
                    corner_radii: CornerRadii::all(9.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + w - badge_w - 7.0, y: row_y + 9.0,
                    text: badge_text, font_size: 10.0, color: CRUST,
                    font_weight: FontWeightHint::Bold, max_width: Some(badge_w),
                });
            }

            row_y += 34.0;
        }

        // Categories section
        row_y += 12.0;
        cmds.push(RenderCommand::FillRect {
            x: x + PADDING, y: row_y, width: w - PADDING * 2.0, height: 1.0,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });
        row_y += 12.0;

        cmds.push(RenderCommand::Text {
            x: x + PADDING, y: row_y,
            text: "CATEGORIES".to_string(),
            font_size: 10.0, color: OVERLAY0,
            font_weight: FontWeightHint::Bold, max_width: Some(w - PADDING * 2.0),
        });
        row_y += 20.0;

        for cat in TaskCategory::all() {
            let is_active = self.view == ViewFilter::ByCategory(*cat);
            let bg_color = if is_active { SURFACE0 } else { MANTLE };
            let text_color = if is_active { cat.color() } else { SUBTEXT1 };

            cmds.push(RenderCommand::FillRect {
                x: x + 4.0, y: row_y, width: w - 8.0, height: 28.0,
                color: bg_color, corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            // Color dot
            cmds.push(RenderCommand::FillRect {
                x: x + PADDING, y: row_y + 9.0, width: 10.0, height: 10.0,
                color: cat.color(), corner_radii: CornerRadii::all(5.0),
            });

            // Label
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 18.0, y: row_y + 7.0,
                text: cat.label().to_string(),
                font_size: 12.0, color: text_color,
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(w - PADDING * 2.0 - 50.0),
            });

            // Count
            let count = self.store.count_by_category(*cat);
            if count > 0 {
                cmds.push(RenderCommand::Text {
                    x: x + w - 30.0, y: row_y + 7.0,
                    text: format!("{count}"),
                    font_size: 11.0, color: OVERLAY0,
                    font_weight: FontWeightHint::Regular, max_width: Some(25.0),
                });
            }

            row_y += 32.0;
        }
    }

    /// Render the main task list panel.
    fn render_task_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32, y: f32, w: f32, h: f32,
    ) {
        // Clip region
        cmds.push(RenderCommand::PushClip { x, y, width: w, height: h });

        let tasks = self.current_tasks();
        let mut row_y = y + PADDING;

        if tasks.is_empty() {
            // Empty state
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 80.0, y: y + h / 2.0 - 30.0,
                text: "No tasks".to_string(),
                font_size: 18.0, color: OVERLAY0,
                font_weight: FontWeightHint::Bold, max_width: Some(200.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 100.0, y: y + h / 2.0,
                text: "Create a new task to get started".to_string(),
                font_size: 13.0, color: SURFACE2,
                font_weight: FontWeightHint::Regular, max_width: Some(250.0),
            });
            cmds.push(RenderCommand::PopClip);
            return;
        }

        for task in &tasks {
            if row_y > y + h {
                break;
            }

            let is_selected = self.selected_task_id == Some(task.id);
            self.render_task_item(cmds, task, x + PADDING, row_y, w - PADDING * 2.0, is_selected);
            row_y += ITEM_HEIGHT + 4.0;
        }

        cmds.push(RenderCommand::PopClip);
    }

    /// Render a single task item card.
    fn render_task_item(
        &self,
        cmds: &mut Vec<RenderCommand>,
        task: &Task,
        x: f32, y: f32, w: f32,
        selected: bool,
    ) {
        let card_color = if selected { SURFACE0 } else { MANTLE };

        // Card shadow (subtle)
        cmds.push(RenderCommand::BoxShadow {
            x, y, width: w, height: ITEM_HEIGHT,
            offset_x: 0.0, offset_y: 1.0, blur: 4.0, spread: 0.0,
            color: Color::rgba(0, 0, 0, 40),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Card background
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: ITEM_HEIGHT,
            color: card_color, corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Selection highlight
        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x, y, width: w, height: ITEM_HEIGHT,
                color: BLUE, line_width: 1.5,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
        }

        // Priority bar on the left
        cmds.push(RenderCommand::FillRect {
            x, y: y + 4.0, width: 4.0, height: ITEM_HEIGHT - 8.0,
            color: task.priority.color(),
            corner_radii: CornerRadii::all(2.0),
        });

        // Checkbox area
        let checkbox_x = x + 14.0;
        let checkbox_y = y + ITEM_HEIGHT / 2.0 - 10.0;
        cmds.push(RenderCommand::StrokeRect {
            x: checkbox_x, y: checkbox_y, width: 20.0, height: 20.0,
            color: if task.completed { GREEN } else { SURFACE2 },
            line_width: 1.5,
            corner_radii: CornerRadii::all(4.0),
        });
        if task.completed {
            cmds.push(RenderCommand::FillRect {
                x: checkbox_x + 3.0, y: checkbox_y + 3.0,
                width: 14.0, height: 14.0,
                color: GREEN, corner_radii: CornerRadii::all(3.0),
            });
            // Checkmark text
            cmds.push(RenderCommand::Text {
                x: checkbox_x + 4.0, y: checkbox_y + 3.0,
                text: "v".to_string(),
                font_size: 12.0, color: CRUST,
                font_weight: FontWeightHint::Bold, max_width: Some(16.0),
            });
        }

        // Title
        let text_x = checkbox_x + 30.0;
        let title_color = if task.completed { OVERLAY0 } else { TEXT };
        cmds.push(RenderCommand::Text {
            x: text_x, y: y + 12.0,
            text: task.title.clone(),
            font_size: 14.0, color: title_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 100.0),
        });

        // Due date label
        let due_text = task.due_label(self.today);
        let due_color = if task.is_overdue(self.now) { RED } else { SUBTEXT0 };
        cmds.push(RenderCommand::Text {
            x: text_x, y: y + 30.0,
            text: due_text, font_size: 11.0, color: due_color,
            font_weight: FontWeightHint::Regular, max_width: Some(w - 120.0),
        });

        // Category + priority badges
        let badge_y = y + 48.0;
        // Category badge
        cmds.push(RenderCommand::FillRect {
            x: text_x, y: badge_y, width: 60.0, height: 18.0,
            color: Color::rgba(
                task.category.color().r,
                task.category.color().g,
                task.category.color().b,
                40,
            ),
            corner_radii: CornerRadii::all(9.0),
        });
        cmds.push(RenderCommand::Text {
            x: text_x + 8.0, y: badge_y + 3.0,
            text: task.category.label().to_string(),
            font_size: 9.0, color: task.category.color(),
            font_weight: FontWeightHint::Bold, max_width: Some(55.0),
        });

        // Priority badge
        let pri_x = text_x + 68.0;
        cmds.push(RenderCommand::FillRect {
            x: pri_x, y: badge_y, width: 55.0, height: 18.0,
            color: Color::rgba(
                task.priority.color().r,
                task.priority.color().g,
                task.priority.color().b,
                40,
            ),
            corner_radii: CornerRadii::all(9.0),
        });
        cmds.push(RenderCommand::Text {
            x: pri_x + 6.0, y: badge_y + 3.0,
            text: task.priority.label().to_string(),
            font_size: 9.0, color: task.priority.color(),
            font_weight: FontWeightHint::Bold, max_width: Some(50.0),
        });

        // Recurrence indicator
        if task.recurrence != RecurrenceRule::None {
            let rec_x = pri_x + 62.0;
            cmds.push(RenderCommand::Text {
                x: rec_x, y: badge_y + 3.0,
                text: format!("[{}]", task.recurrence.label()),
                font_size: 9.0, color: TEAL,
                font_weight: FontWeightHint::Regular, max_width: Some(100.0),
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
                x: bar_x, y: bar_y, width: bar_w, height: bar_h,
                color: SURFACE1, corner_radii: CornerRadii::all(3.0),
            });

            // Fill
            let fill_w = (bar_w * pct as f32) / 100.0;
            if fill_w > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: bar_x, y: bar_y, width: fill_w, height: bar_h,
                    color: GREEN, corner_radii: CornerRadii::all(3.0),
                });
            }

            // Percentage text
            cmds.push(RenderCommand::Text {
                x: bar_x, y: bar_y + 10.0,
                text: format!("{pct}%"),
                font_size: 9.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: Some(bar_w),
            });
        }
    }

    /// Render the detail panel for the selected task.
    fn render_detail_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32, y: f32, w: f32, h: f32,
    ) {
        let Some(task_id) = self.selected_task_id else { return };
        let Some(task) = self.store.get(task_id) else { return };

        // Background
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::FillRect {
            x, y, width: 1.0, height: h,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });

        let pad = PADDING;
        let mut row_y = y + pad;
        let content_w = w - pad * 2.0;

        // Title
        cmds.push(RenderCommand::Text {
            x: x + pad, y: row_y,
            text: task.title.clone(),
            font_size: 18.0, color: TEXT,
            font_weight: FontWeightHint::Bold, max_width: Some(content_w),
        });
        row_y += 28.0;

        // Status pill
        let status_text = if task.completed { "Completed" } else { "Active" };
        let status_color = if task.completed { GREEN } else { BLUE };
        cmds.push(RenderCommand::FillRect {
            x: x + pad, y: row_y, width: 80.0, height: 22.0,
            color: Color::rgba(status_color.r, status_color.g, status_color.b, 40),
            corner_radii: CornerRadii::all(11.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 10.0, y: row_y + 4.0,
            text: status_text.to_string(),
            font_size: 11.0, color: status_color,
            font_weight: FontWeightHint::Bold, max_width: Some(70.0),
        });
        row_y += 32.0;

        // Separator
        cmds.push(RenderCommand::FillRect {
            x: x + pad, y: row_y, width: content_w, height: 1.0,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });
        row_y += 12.0;

        // Detail fields
        let field_label_color = OVERLAY0;
        let field_value_color = SUBTEXT1;

        // Due date
        cmds.push(RenderCommand::Text {
            x: x + pad, y: row_y,
            text: "Due".to_string(),
            font_size: 10.0, color: field_label_color,
            font_weight: FontWeightHint::Bold, max_width: Some(content_w),
        });
        row_y += 14.0;
        let due_text = if let Some(due) = task.due {
            due.format_medium()
        } else {
            "Not set".to_string()
        };
        let due_color = if task.is_overdue(self.now) { RED } else { field_value_color };
        cmds.push(RenderCommand::Text {
            x: x + pad, y: row_y,
            text: due_text,
            font_size: 13.0, color: due_color,
            font_weight: FontWeightHint::Regular, max_width: Some(content_w),
        });
        row_y += 22.0;

        // Priority
        cmds.push(RenderCommand::Text {
            x: x + pad, y: row_y,
            text: "Priority".to_string(),
            font_size: 10.0, color: field_label_color,
            font_weight: FontWeightHint::Bold, max_width: Some(content_w),
        });
        row_y += 14.0;
        cmds.push(RenderCommand::FillRect {
            x: x + pad, y: row_y, width: 8.0, height: 8.0,
            color: task.priority.color(), corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 14.0, y: row_y - 2.0,
            text: task.priority.label().to_string(),
            font_size: 13.0, color: task.priority.color(),
            font_weight: FontWeightHint::Bold, max_width: Some(content_w - 20.0),
        });
        row_y += 22.0;

        // Category
        cmds.push(RenderCommand::Text {
            x: x + pad, y: row_y,
            text: "Category".to_string(),
            font_size: 10.0, color: field_label_color,
            font_weight: FontWeightHint::Bold, max_width: Some(content_w),
        });
        row_y += 14.0;
        cmds.push(RenderCommand::FillRect {
            x: x + pad, y: row_y, width: 8.0, height: 8.0,
            color: task.category.color(), corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 14.0, y: row_y - 2.0,
            text: task.category.label().to_string(),
            font_size: 13.0, color: task.category.color(),
            font_weight: FontWeightHint::Bold, max_width: Some(content_w - 20.0),
        });
        row_y += 22.0;

        // Recurrence
        cmds.push(RenderCommand::Text {
            x: x + pad, y: row_y,
            text: "Repeats".to_string(),
            font_size: 10.0, color: field_label_color,
            font_weight: FontWeightHint::Bold, max_width: Some(content_w),
        });
        row_y += 14.0;
        cmds.push(RenderCommand::Text {
            x: x + pad, y: row_y,
            text: task.recurrence.label().to_string(),
            font_size: 13.0, color: field_value_color,
            font_weight: FontWeightHint::Regular, max_width: Some(content_w),
        });
        row_y += 22.0;

        // Description
        if !task.description.is_empty() {
            cmds.push(RenderCommand::FillRect {
                x: x + pad, y: row_y, width: content_w, height: 1.0,
                color: SURFACE0, corner_radii: CornerRadii::ZERO,
            });
            row_y += 12.0;

            cmds.push(RenderCommand::Text {
                x: x + pad, y: row_y,
                text: "Description".to_string(),
                font_size: 10.0, color: field_label_color,
                font_weight: FontWeightHint::Bold, max_width: Some(content_w),
            });
            row_y += 14.0;
            cmds.push(RenderCommand::Text {
                x: x + pad, y: row_y,
                text: task.description.clone(),
                font_size: 12.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: Some(content_w),
            });
            row_y += 24.0;
        }

        // Notes
        if !task.notes.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + pad, y: row_y,
                text: "Notes".to_string(),
                font_size: 10.0, color: field_label_color,
                font_weight: FontWeightHint::Bold, max_width: Some(content_w),
            });
            row_y += 14.0;
            cmds.push(RenderCommand::Text {
                x: x + pad, y: row_y,
                text: task.notes.clone(),
                font_size: 12.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: Some(content_w),
            });
            row_y += 24.0;
        }

        // Subtasks
        if !task.subtasks.is_empty() {
            cmds.push(RenderCommand::FillRect {
                x: x + pad, y: row_y, width: content_w, height: 1.0,
                color: SURFACE0, corner_radii: CornerRadii::ZERO,
            });
            row_y += 12.0;

            let done_count = task.subtasks.iter().filter(|s| s.completed).count();
            cmds.push(RenderCommand::Text {
                x: x + pad, y: row_y,
                text: format!("Subtasks ({}/{})", done_count, task.subtasks.len()),
                font_size: 10.0, color: field_label_color,
                font_weight: FontWeightHint::Bold, max_width: Some(content_w),
            });
            row_y += 16.0;

            // Progress bar
            let pct = task.completion_percent();
            cmds.push(RenderCommand::FillRect {
                x: x + pad, y: row_y, width: content_w, height: 6.0,
                color: SURFACE1, corner_radii: CornerRadii::all(3.0),
            });
            let fill_w = (content_w * pct as f32) / 100.0;
            if fill_w > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: x + pad, y: row_y, width: fill_w, height: 6.0,
                    color: GREEN, corner_radii: CornerRadii::all(3.0),
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
                    x: x + pad, y: row_y, width: 14.0, height: 14.0,
                    color: if st.completed { GREEN } else { SURFACE2 },
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(3.0),
                });
                if st.completed {
                    cmds.push(RenderCommand::FillRect {
                        x: x + pad + 2.0, y: row_y + 2.0,
                        width: 10.0, height: 10.0,
                        color: GREEN, corner_radii: CornerRadii::all(2.0),
                    });
                }

                cmds.push(RenderCommand::Text {
                    x: x + pad + 20.0, y: row_y,
                    text: st.title.clone(),
                    font_size: 12.0, color: st_color,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_w - 24.0),
                });
                row_y += 20.0;
            }
        }

        // Snooze options (if task has a due date and is not completed)
        if task.due.is_some() && !task.completed {
            row_y += 8.0;
            cmds.push(RenderCommand::FillRect {
                x: x + pad, y: row_y, width: content_w, height: 1.0,
                color: SURFACE0, corner_radii: CornerRadii::ZERO,
            });
            row_y += 12.0;

            cmds.push(RenderCommand::Text {
                x: x + pad, y: row_y,
                text: "Snooze".to_string(),
                font_size: 10.0, color: field_label_color,
                font_weight: FontWeightHint::Bold, max_width: Some(content_w),
            });
            row_y += 16.0;

            let btn_w = (content_w - 8.0) / 2.0;
            for (i, preset) in SnoozeDuration::presets().iter().enumerate() {
                let col = i % 2;
                let row_idx = i / 2;
                let bx = x + pad + col as f32 * (btn_w + 8.0);
                let by = row_y + row_idx as f32 * 30.0;

                cmds.push(RenderCommand::FillRect {
                    x: bx, y: by, width: btn_w, height: 24.0,
                    color: SURFACE0, corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
                cmds.push(RenderCommand::Text {
                    x: bx + 8.0, y: by + 5.0,
                    text: preset.label(),
                    font_size: 11.0, color: SKY,
                    font_weight: FontWeightHint::Regular, max_width: Some(btn_w - 16.0),
                });
            }
        }

        // Created date at the bottom
        let created_y = y + h - 24.0;
        cmds.push(RenderCommand::Text {
            x: x + pad, y: created_y,
            text: format!("Created: {}", task.created.format_short()),
            font_size: 10.0, color: OVERLAY0,
            font_weight: FontWeightHint::Regular, max_width: Some(content_w),
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
    t.due = Some(DateTime::new(today.add_days(-2), Time { hour: 17, minute: 0 }));
    t.priority = Priority::High;
    t.category = TaskCategory::Work;
    t.subtasks = vec![
        { let mut s = Subtask::new("Read financials"); s.completed = true; s },
        Subtask::new("Write summary"),
        Subtask::new("Send to team"),
    ];
    store.add(t);

    // Due today
    let mut t = Task::new(0, "Buy groceries", now);
    t.description = "Milk, eggs, bread, vegetables".to_string();
    t.due = Some(DateTime::new(today, Time { hour: 18, minute: 0 }));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Shopping;
    store.add(t);

    // Due today, high priority
    let mut t = Task::new(0, "Doctor appointment", now);
    t.description = "Annual checkup at 2pm".to_string();
    t.due = Some(DateTime::new(today, Time { hour: 14, minute: 0 }));
    t.priority = Priority::High;
    t.category = TaskCategory::Health;
    store.add(t);

    // Due tomorrow
    let mut t = Task::new(0, "Submit tax documents", now);
    t.due = Some(DateTime::new(today.add_days(1), Time { hour: 12, minute: 0 }));
    t.priority = Priority::Critical;
    t.category = TaskCategory::Finance;
    store.add(t);

    // Recurring daily
    let mut t = Task::new(0, "Morning exercise", now);
    t.description = "30 min cardio + stretching".to_string();
    t.due = Some(DateTime::new(today.add_days(1), Time { hour: 7, minute: 0 }));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Health;
    t.recurrence = RecurrenceRule::Daily;
    store.add(t);

    // Recurring weekly
    let mut t = Task::new(0, "Team standup meeting", now);
    t.due = Some(DateTime::new(today.add_days(2), Time { hour: 9, minute: 0 }));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Work;
    t.recurrence = RecurrenceRule::Weekly;
    store.add(t);

    // Due in 5 days
    let mut t = Task::new(0, "Pay electricity bill", now);
    t.due = Some(DateTime::new(today.add_days(5), Time { hour: 23, minute: 59 }));
    t.priority = Priority::High;
    t.category = TaskCategory::Finance;
    t.notes = "Account #12345, auto-pay not set up yet".to_string();
    store.add(t);

    // Due in a week
    let mut t = Task::new(0, "Plan birthday party", now);
    t.description = "Venue, guest list, catering".to_string();
    t.due = Some(DateTime::new(today.add_days(7), Time { hour: 10, minute: 0 }));
    t.priority = Priority::Low;
    t.category = TaskCategory::Social;
    t.subtasks = vec![
        { let mut s = Subtask::new("Choose venue"); s.completed = true; s },
        { let mut s = Subtask::new("Send invitations"); s.completed = true; s },
        Subtask::new("Order cake"),
        Subtask::new("Buy decorations"),
        Subtask::new("Arrange catering"),
    ];
    store.add(t);

    // Monthly recurring
    let mut t = Task::new(0, "Monthly budget review", now);
    t.due = Some(DateTime::new(today.add_days(14), Time { hour: 20, minute: 0 }));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Finance;
    t.recurrence = RecurrenceRule::Monthly;
    store.add(t);

    // Education
    let mut t = Task::new(0, "Complete Rust course module 5", now);
    t.description = "Async programming and futures".to_string();
    t.due = Some(DateTime::new(today.add_days(3), Time { hour: 21, minute: 0 }));
    t.priority = Priority::Medium;
    t.category = TaskCategory::Education;
    t.subtasks = vec![
        { let mut s = Subtask::new("Watch lectures"); s.completed = true; s },
        Subtask::new("Do exercises"),
        Subtask::new("Submit assignment"),
    ];
    store.add(t);

    // Completed task
    let mut t = Task::new(0, "Clean the garage", now);
    t.due = Some(DateTime::new(today.add_days(-1), Time { hour: 10, minute: 0 }));
    t.priority = Priority::Low;
    t.category = TaskCategory::Home;
    t.completed = true;
    t.completed_at = Some(DateTime::new(today.add_days(-1), Time { hour: 16, minute: 0 }));
    store.add(t);

    // No due date
    let mut t = Task::new(0, "Read 'Designing Data-Intensive Applications'", now);
    t.priority = Priority::Low;
    t.category = TaskCategory::Education;
    store.add(t);
}

fn main() {
    let now = DateTime::new(
        Date { year: 2026, month: 5, day: 18 },
        Time { hour: 10, minute: 30 },
    );
    let mut app = RemindersApp::new(WINDOW_WIDTH, WINDOW_HEIGHT, now);

    sample_tasks(&mut app.store, now);

    // Verify all views render
    for view in ViewFilter::standard_views() {
        app.view = *view;
        let cmds = app.render();
        let _ = cmds.len();
    }
    // Verify category views
    for cat in TaskCategory::all() {
        app.view = ViewFilter::ByCategory(*cat);
        let cmds = app.render();
        let _ = cmds.len();
    }

    // Test with selected task
    app.view = ViewFilter::All;
    if let Some(first) = app.store.all().first() {
        app.select_task(first.id);
    }
    let cmds = app.render();
    let _ = cmds.len();

    // Test notifications
    app.check_notifications();

    // Test export
    let json = app.store.export_json();
    let _ = json.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_now() -> DateTime {
        DateTime::new(
            Date { year: 2026, month: 5, day: 18 },
            Time { hour: 10, minute: 30 },
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
        let d = Date { year: 2024, month: 1, day: 1 };
        assert_eq!(d.day_of_week(), 1);
        // 2024-01-07 is Sunday
        let d = Date { year: 2024, month: 1, day: 7 };
        assert_eq!(d.day_of_week(), 0);
    }

    #[test]
    fn test_date_add_days_forward() {
        let d = Date { year: 2026, month: 1, day: 30 };
        let next = d.add_days(3);
        assert_eq!(next.month, 2);
        assert_eq!(next.day, 2);
    }

    #[test]
    fn test_date_add_days_backward() {
        let d = Date { year: 2026, month: 3, day: 1 };
        let prev = d.add_days(-1);
        assert_eq!(prev.month, 2);
        assert_eq!(prev.day, 28);
    }

    #[test]
    fn test_date_add_days_year_boundary() {
        let d = Date { year: 2026, month: 12, day: 30 };
        let next = d.add_days(5);
        assert_eq!(next.year, 2027);
        assert_eq!(next.month, 1);
    }

    #[test]
    fn test_date_add_months() {
        let d = Date { year: 2026, month: 1, day: 31 };
        let next = d.add_months(1);
        assert_eq!(next.month, 2);
        assert_eq!(next.day, 28); // Clamped
    }

    #[test]
    fn test_date_add_months_year_wrap() {
        let d = Date { year: 2026, month: 11, day: 15 };
        let next = d.add_months(3);
        assert_eq!(next.year, 2027);
        assert_eq!(next.month, 2);
    }

    #[test]
    fn test_date_days_since() {
        let a = Date { year: 2026, month: 1, day: 10 };
        let b = Date { year: 2026, month: 1, day: 1 };
        assert_eq!(a.days_since(b), 9);
    }

    #[test]
    fn test_date_format() {
        let d = Date { year: 2026, month: 3, day: 15 };
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
        let t = Time { hour: 14, minute: 30 };
        assert_eq!(t.format_12h(), "2:30 PM");
        let t = Time { hour: 0, minute: 0 };
        assert_eq!(t.format_12h(), "12:00 AM");
        let t = Time { hour: 12, minute: 0 };
        assert_eq!(t.format_12h(), "12:00 PM");
    }

    #[test]
    fn test_time_to_from_minutes() {
        let t = Time { hour: 2, minute: 30 };
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
        assert_eq!(TaskCategory::from_str_label("work"), Some(TaskCategory::Work));
        assert_eq!(TaskCategory::from_str_label("HEALTH"), Some(TaskCategory::Health));
        assert_eq!(TaskCategory::from_str_label("xyz"), None);
    }

    // === Recurrence tests ===

    #[test]
    fn test_recurrence_none() {
        let r = RecurrenceRule::None;
        let d = Date { year: 2026, month: 1, day: 1 };
        assert!(r.matches(d, d));
        assert!(!r.matches(d, d.add_days(1)));
        assert!(r.next_occurrence(d).is_none());
    }

    #[test]
    fn test_recurrence_daily() {
        let r = RecurrenceRule::Daily;
        let origin = Date { year: 2026, month: 1, day: 1 };
        assert!(r.matches(origin, origin.add_days(5)));
        assert_eq!(r.next_occurrence(origin), Some(origin.add_days(1)));
    }

    #[test]
    fn test_recurrence_weekly() {
        let r = RecurrenceRule::Weekly;
        let origin = Date { year: 2026, month: 1, day: 1 };
        assert!(r.matches(origin, origin.add_days(7)));
        assert!(r.matches(origin, origin.add_days(14)));
        assert!(!r.matches(origin, origin.add_days(3)));
    }

    #[test]
    fn test_recurrence_monthly() {
        let r = RecurrenceRule::Monthly;
        let origin = Date { year: 2026, month: 1, day: 15 };
        assert!(r.matches(origin, Date { year: 2026, month: 3, day: 15 }));
        assert!(!r.matches(origin, Date { year: 2026, month: 3, day: 16 }));
    }

    #[test]
    fn test_recurrence_yearly() {
        let r = RecurrenceRule::Yearly;
        let origin = Date { year: 2026, month: 6, day: 15 };
        assert!(r.matches(origin, Date { year: 2027, month: 6, day: 15 }));
        assert!(!r.matches(origin, Date { year: 2027, month: 7, day: 15 }));
    }

    #[test]
    fn test_recurrence_custom() {
        let r = RecurrenceRule::Custom { interval_days: 3 };
        let origin = Date { year: 2026, month: 1, day: 1 };
        assert!(r.matches(origin, origin.add_days(3)));
        assert!(r.matches(origin, origin.add_days(6)));
        assert!(!r.matches(origin, origin.add_days(4)));
    }

    #[test]
    fn test_recurrence_custom_zero() {
        let r = RecurrenceRule::Custom { interval_days: 0 };
        let d = Date { year: 2026, month: 1, day: 1 };
        assert!(r.matches(d, d));
        assert!(!r.matches(d, d.add_days(1)));
        assert!(r.next_occurrence(d).is_none());
    }

    #[test]
    fn test_recurrence_before_origin() {
        let r = RecurrenceRule::Daily;
        let origin = Date { year: 2026, month: 5, day: 10 };
        assert!(!r.matches(origin, Date { year: 2026, month: 5, day: 9 }));
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
            { let mut s = Subtask::new("A"); s.completed = true; s },
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
        t.due = Some(DateTime::new(now.date.add_days(-1), Time { hour: 12, minute: 0 }));
        assert!(t.is_overdue(now));
    }

    #[test]
    fn test_task_not_overdue_when_completed() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(now.date.add_days(-1), Time { hour: 12, minute: 0 }));
        t.completed = true;
        assert!(!t.is_overdue(now));
    }

    #[test]
    fn test_task_due_today() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(now.date, Time { hour: 18, minute: 0 }));
        assert!(t.is_due_today(now.date));
    }

    #[test]
    fn test_task_due_within() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(now.date.add_days(3), Time { hour: 10, minute: 0 }));
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
        let later = DateTime::new(now.date, Time { hour: 10, minute: 50 });
        assert!(!t.is_snoozed(later));
    }

    #[test]
    fn test_task_snooze_hour() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.snooze(now, SnoozeDuration::Hour1);
        assert!(t.is_snoozed(DateTime::new(now.date, Time { hour: 11, minute: 0 })));
        assert!(!t.is_snoozed(DateTime::new(now.date, Time { hour: 11, minute: 31 })));
    }

    #[test]
    fn test_task_matches_query() {
        let now = make_now();
        let mut t = make_task("Buy groceries", now);
        t.description = "Milk and bread".to_string();
        assert!(t.matches_query("grocery"));
        assert!(t.matches_query("milk"));
        assert!(t.matches_query("")); // Empty query matches all
        assert!(!t.matches_query("exercise"));
    }

    #[test]
    fn test_task_due_label_today() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(now.date, Time { hour: 14, minute: 0 }));
        let label = t.due_label(now.date);
        assert!(label.contains("Today"));
    }

    #[test]
    fn test_task_due_label_tomorrow() {
        let now = make_now();
        let mut t = make_task("Test", now);
        t.due = Some(DateTime::new(now.date.add_days(1), Time { hour: 9, minute: 0 }));
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
        t.due = Some(DateTime::new(now.date, Time { hour: 18, minute: 0 }));
        store.add(t);

        let mut t2 = make_task("Due tomorrow", now);
        t2.due = Some(DateTime::new(now.date.add_days(1), Time { hour: 9, minute: 0 }));
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
        t.due = Some(DateTime::new(now.date.add_days(-1), Time { hour: 12, minute: 0 }));
        store.add(t);

        let mut t2 = make_task("Not overdue", now);
        t2.due = Some(DateTime::new(now.date.add_days(1), Time { hour: 12, minute: 0 }));
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
        t2.due = Some(DateTime::new(now.date.add_days(-2), Time { hour: 12, minute: 0 }));
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
        t2.due = Some(DateTime::new(now.date.add_days(1), Time { hour: 12, minute: 0 }));
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

    #[test]
    fn test_json_roundtrip() {
        let now = make_now();
        let mut store = TaskStore::new();
        let mut t = make_task("Roundtrip test", now);
        t.description = "Some description".to_string();
        t.priority = Priority::Critical;
        t.category = TaskCategory::Health;
        t.recurrence = RecurrenceRule::Weekly;
        t.subtasks = vec![
            { let mut s = Subtask::new("Step A"); s.completed = true; s },
            Subtask::new("Step B"),
        ];
        store.add(t);

        let json = store.export_json();

        let mut store2 = TaskStore::new();
        let count = store2.import_json(&json, now);
        assert_eq!(count, 1);
        let imported = &store2.all()[0];
        assert_eq!(imported.title, "Roundtrip test");
        assert_eq!(imported.priority, Priority::Critical);
        assert_eq!(imported.category, TaskCategory::Health);
        assert_eq!(imported.subtasks.len(), 2);
        assert!(imported.subtasks[0].completed);
    }

    #[test]
    fn test_json_escape_special_chars() {
        let escaped = escape_json("Hello \"world\"\nnew line");
        assert!(escaped.contains("\\\""));
        assert!(escaped.contains("\\n"));
        let unescaped = unescape_json(&escaped);
        assert_eq!(unescaped, "Hello \"world\"\nnew line");
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
        assert_eq!(parse_time_12h("3:00 PM"), Some(Time { hour: 15, minute: 0 }));
        assert_eq!(parse_time_12h("12:00 AM"), Some(Time { hour: 0, minute: 0 }));
        assert_eq!(parse_time_12h("12:30 PM"), Some(Time { hour: 12, minute: 30 }));
        assert_eq!(parse_time_12h("11:59 AM"), Some(Time { hour: 11, minute: 59 }));
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
            let cmds = app.render();
            assert!(!cmds.is_empty(), "View {:?} produced no commands", view);
        }
    }

    #[test]
    fn test_render_category_views() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        sample_tasks(&mut app.store, now);

        for cat in TaskCategory::all() {
            app.view = ViewFilter::ByCategory(*cat);
            let cmds = app.render();
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_without_sidebar() {
        let now = make_now();
        let mut app = RemindersApp::new(1100.0, 720.0, now);
        app.sidebar_visible = false;
        let cmds = app.render();
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_state() {
        let now = make_now();
        let app = RemindersApp::new(1100.0, 720.0, now);
        let cmds = app.render();
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
        let a = DateTime::new(Date { year: 2026, month: 5, day: 18 }, Time { hour: 12, minute: 0 });
        let b = DateTime::new(Date { year: 2026, month: 5, day: 18 }, Time { hour: 10, minute: 0 });
        assert_eq!(a.minutes_since(b), 120);
    }

    #[test]
    fn test_datetime_minutes_since_cross_day() {
        let a = DateTime::new(Date { year: 2026, month: 5, day: 19 }, Time { hour: 1, minute: 0 });
        let b = DateTime::new(Date { year: 2026, month: 5, day: 18 }, Time { hour: 23, minute: 0 });
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
