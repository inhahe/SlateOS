//! taskscheduler -- OurOS Task Scheduler
//!
//! A cron-like task scheduling application with a GUI built on guitk.
//! Supports one-shot and recurring schedules including daily, weekly,
//! monthly, hourly, every-N-minutes, and full cron expressions.
//!
//! # Architecture
//!
//! ```text
//! CronExpr          -- parsed cron expression (minute, hour, dom, month, dow)
//!       |
//!       v
//! ScheduleFrequency -- enum of all schedule types (Once, Daily, Cron, etc.)
//!       |
//!       v
//! ScheduledTask     -- a single task with schedule, retry policy, run history
//!       |
//!       v
//! TaskScheduler     -- manages collection of tasks, checks due, calculates next run
//!       |
//!       v
//! TaskHistory       -- log of past executions
//!       |
//!       v
//! TaskSchedulerConfig -- persistence in simple text format
//!       |
//!       v
//! SchedulerUI       -- guitk-based GUI with task list, add/edit, history
//! ```

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::BTreeMap;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COLOR_TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 820.0;
const WINDOW_HEIGHT: f32 = 600.0;
const HEADER_HEIGHT: f32 = 48.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
const ROW_HEIGHT: f32 = 32.0;
const PADDING: f32 = 12.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const BUTTON_WIDTH: f32 = 90.0;
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;
const CHECKBOX_SIZE: f32 = 16.0;

// ============================================================================
// DayOfWeek
// ============================================================================

/// Day of the week (0 = Sunday through 6 = Saturday), matching cron convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DayOfWeek {
    Sunday = 0,
    Monday = 1,
    Tuesday = 2,
    Wednesday = 3,
    Thursday = 4,
    Friday = 5,
    Saturday = 6,
}

impl DayOfWeek {
    /// Parse a numeric day-of-week value (0..=6).
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Sunday),
            1 => Some(Self::Monday),
            2 => Some(Self::Tuesday),
            3 => Some(Self::Wednesday),
            4 => Some(Self::Thursday),
            5 => Some(Self::Friday),
            6 => Some(Self::Saturday),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Sunday => "Sunday",
            Self::Monday => "Monday",
            Self::Tuesday => "Tuesday",
            Self::Wednesday => "Wednesday",
            Self::Thursday => "Thursday",
            Self::Friday => "Friday",
            Self::Saturday => "Saturday",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Sunday => "Sun",
            Self::Monday => "Mon",
            Self::Tuesday => "Tue",
            Self::Wednesday => "Wed",
            Self::Thursday => "Thu",
            Self::Friday => "Fri",
            Self::Saturday => "Sat",
        }
    }
}

// ============================================================================
// CronExpr — simple cron expression parser
// ============================================================================

/// A single cron field that can match specific values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CronField {
    /// Wildcard: matches any value.
    Any,
    /// Matches a single specific value.
    Value(u8),
    /// Matches any value in the list.
    List(Vec<u8>),
    /// Matches values in a range (inclusive).
    Range(u8, u8),
    /// Matches every Nth value starting from the base (base/step).
    Step(u8, u8),
}

impl CronField {
    /// Check whether this field matches a given value.
    pub fn matches(&self, val: u8) -> bool {
        match self {
            Self::Any => true,
            Self::Value(v) => *v == val,
            Self::List(vs) => vs.contains(&val),
            Self::Range(lo, hi) => val >= *lo && val <= *hi,
            Self::Step(base, step) => {
                if *step == 0 {
                    return val == *base;
                }
                if val < *base {
                    return false;
                }
                (val - *base) % *step == 0
            }
        }
    }

    /// Parse a single cron field string.
    ///
    /// Supported formats:
    /// - `*` — wildcard
    /// - `5` — single value
    /// - `1,3,5` — list
    /// - `1-5` — range
    /// - `*/15` — step from 0
    /// - `5/10` — step from base
    pub fn parse(s: &str) -> Result<Self, CronParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(CronParseError::EmptyField);
        }

        // Wildcard
        if s == "*" {
            return Ok(Self::Any);
        }

        // Step: */N or base/step
        if let Some(slash_pos) = s.find('/') {
            let base_part = &s[..slash_pos];
            let step_part = &s[slash_pos + 1..];
            let step: u8 = step_part
                .parse()
                .map_err(|_| CronParseError::InvalidNumber(step_part.to_string()))?;
            let base: u8 = if base_part == "*" {
                0
            } else {
                base_part
                    .parse()
                    .map_err(|_| CronParseError::InvalidNumber(base_part.to_string()))?
            };
            return Ok(Self::Step(base, step));
        }

        // Range: lo-hi
        if let Some(dash_pos) = s.find('-') {
            let lo_part = &s[..dash_pos];
            let hi_part = &s[dash_pos + 1..];
            let lo: u8 = lo_part
                .parse()
                .map_err(|_| CronParseError::InvalidNumber(lo_part.to_string()))?;
            let hi: u8 = hi_part
                .parse()
                .map_err(|_| CronParseError::InvalidNumber(hi_part.to_string()))?;
            if lo > hi {
                return Err(CronParseError::InvalidRange(lo, hi));
            }
            return Ok(Self::Range(lo, hi));
        }

        // List: a,b,c
        if s.contains(',') {
            let mut vals = Vec::new();
            for part in s.split(',') {
                let v: u8 = part
                    .trim()
                    .parse()
                    .map_err(|_| CronParseError::InvalidNumber(part.to_string()))?;
                vals.push(v);
            }
            vals.sort();
            vals.dedup();
            return Ok(Self::List(vals));
        }

        // Single value
        let v: u8 = s
            .parse()
            .map_err(|_| CronParseError::InvalidNumber(s.to_string()))?;
        Ok(Self::Value(v))
    }
}

/// Errors from parsing cron expressions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CronParseError {
    /// Not enough fields (expected 5).
    WrongFieldCount(usize),
    /// An empty field was encountered.
    EmptyField,
    /// A numeric value could not be parsed.
    InvalidNumber(String),
    /// Range lo > hi.
    InvalidRange(u8, u8),
    /// A field value is out of the allowed range.
    OutOfRange {
        field: &'static str,
        value: u8,
        max: u8,
    },
}

impl core::fmt::Display for CronParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WrongFieldCount(n) => write!(f, "expected 5 fields, got {n}"),
            Self::EmptyField => write!(f, "empty cron field"),
            Self::InvalidNumber(s) => write!(f, "invalid number: {s}"),
            Self::InvalidRange(lo, hi) => write!(f, "invalid range: {lo}-{hi}"),
            Self::OutOfRange { field, value, max } => {
                write!(f, "{field} value {value} out of range 0-{max}")
            }
        }
    }
}

/// A parsed cron expression with five fields: minute, hour, day-of-month, month,
/// day-of-week.
///
/// Format: `minute hour day_of_month month day_of_week`
///
/// Ranges:
/// - minute: 0-59
/// - hour: 0-23
/// - day_of_month: 1-31
/// - month: 1-12
/// - day_of_week: 0-6 (0 = Sunday)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronExpr {
    pub minute: CronField,
    pub hour: CronField,
    pub day_of_month: CronField,
    pub month: CronField,
    pub day_of_week: CronField,
}

impl CronExpr {
    /// Parse a cron expression string (5 space-separated fields).
    pub fn parse(expr: &str) -> Result<Self, CronParseError> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(CronParseError::WrongFieldCount(fields.len()));
        }

        let minute = CronField::parse(fields[0])?;
        let hour = CronField::parse(fields[1])?;
        let day_of_month = CronField::parse(fields[2])?;
        let month = CronField::parse(fields[3])?;
        let day_of_week = CronField::parse(fields[4])?;

        let cron = Self {
            minute,
            hour,
            day_of_month,
            month,
            day_of_week,
        };
        cron.validate()?;
        Ok(cron)
    }

    /// Validate that field values are within the allowed ranges.
    fn validate(&self) -> Result<(), CronParseError> {
        validate_field_range(&self.minute, "minute", 59)?;
        validate_field_range(&self.hour, "hour", 23)?;
        validate_field_range_min(&self.day_of_month, "day_of_month", 1, 31)?;
        validate_field_range_min(&self.month, "month", 1, 12)?;
        validate_field_range(&self.day_of_week, "day_of_week", 6)?;
        Ok(())
    }

    /// Check whether a given time matches this cron expression.
    ///
    /// Arguments are decomposed time fields (not a timestamp) so this stays
    /// pure and testable without a clock.
    pub fn matches(&self, minute: u8, hour: u8, day_of_month: u8, month: u8, day_of_week: u8) -> bool {
        self.minute.matches(minute)
            && self.hour.matches(hour)
            && self.day_of_month.matches(day_of_month)
            && self.month.matches(month)
            && self.day_of_week.matches(day_of_week)
    }

    /// Format this cron expression back to string form.
    pub fn to_string_repr(&self) -> String {
        format!(
            "{} {} {} {} {}",
            format_cron_field(&self.minute),
            format_cron_field(&self.hour),
            format_cron_field(&self.day_of_month),
            format_cron_field(&self.month),
            format_cron_field(&self.day_of_week),
        )
    }
}

/// Validate that all concrete values in a field are within 0..=max.
fn validate_field_range(field: &CronField, name: &'static str, max: u8) -> Result<(), CronParseError> {
    validate_field_range_min(field, name, 0, max)
}

/// Validate that all concrete values in a field are within min..=max.
fn validate_field_range_min(
    field: &CronField,
    name: &'static str,
    min: u8,
    max: u8,
) -> Result<(), CronParseError> {
    match field {
        CronField::Any => Ok(()),
        CronField::Value(v) => {
            if *v < min || *v > max {
                Err(CronParseError::OutOfRange {
                    field: name,
                    value: *v,
                    max,
                })
            } else {
                Ok(())
            }
        }
        CronField::List(vs) => {
            for v in vs {
                if *v < min || *v > max {
                    return Err(CronParseError::OutOfRange {
                        field: name,
                        value: *v,
                        max,
                    });
                }
            }
            Ok(())
        }
        CronField::Range(lo, hi) => {
            if *lo < min || *hi > max {
                return Err(CronParseError::OutOfRange {
                    field: name,
                    value: if *lo < min { *lo } else { *hi },
                    max,
                });
            }
            Ok(())
        }
        CronField::Step(base, _step) => {
            if *base < min || *base > max {
                Err(CronParseError::OutOfRange {
                    field: name,
                    value: *base,
                    max,
                })
            } else {
                Ok(())
            }
        }
    }
}

/// Format a CronField back to string.
fn format_cron_field(field: &CronField) -> String {
    match field {
        CronField::Any => String::from("*"),
        CronField::Value(v) => format!("{v}"),
        CronField::List(vs) => vs
            .iter()
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(","),
        CronField::Range(lo, hi) => format!("{lo}-{hi}"),
        CronField::Step(base, step) => {
            if *base == 0 {
                format!("*/{step}")
            } else {
                format!("{base}/{step}")
            }
        }
    }
}

// ============================================================================
// ScheduleFrequency
// ============================================================================

/// How often a task should run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleFrequency {
    /// Run exactly once at the specified timestamp.
    Once,
    /// Run every day at the same time.
    Daily,
    /// Run on a specific day of the week.
    Weekly(DayOfWeek),
    /// Run on a specific day of the month (1-31).
    Monthly(u8),
    /// Run every hour.
    Hourly,
    /// Run every N minutes.
    EveryNMinutes(u32),
    /// Run according to a cron expression.
    Cron(CronExpr),
}

impl ScheduleFrequency {
    /// Human-readable description of this frequency.
    pub fn display_name(&self) -> String {
        match self {
            Self::Once => String::from("Once"),
            Self::Daily => String::from("Daily"),
            Self::Weekly(day) => format!("Weekly ({})", day.display_name()),
            Self::Monthly(day) => format!("Monthly (day {day})"),
            Self::Hourly => String::from("Hourly"),
            Self::EveryNMinutes(n) => format!("Every {n} min"),
            Self::Cron(expr) => format!("Cron: {}", expr.to_string_repr()),
        }
    }
}

// ============================================================================
// TaskResult
// ============================================================================

/// Outcome of a single task execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Task failed with an error message.
    Error(String),
}

impl TaskResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    pub fn display_str(&self) -> &str {
        match self {
            Self::Ok => "OK",
            Self::Error(msg) => msg.as_str(),
        }
    }
}

// ============================================================================
// ScheduledTask
// ============================================================================

/// A single scheduled task.
#[derive(Clone, Debug)]
pub struct ScheduledTask {
    /// Unique identifier.
    pub id: u64,
    /// Human-readable name.
    pub name: String,
    /// Command to execute.
    pub command: String,
    /// How often to run.
    pub frequency: ScheduleFrequency,
    /// Whether the task is active.
    pub enabled: bool,
    /// Timestamp (unix epoch seconds) of the next scheduled run.
    pub next_run_timestamp: u64,
    /// Timestamp of the last run (0 if never run).
    pub last_run_timestamp: u64,
    /// Result of the last execution.
    pub last_result: Option<TaskResult>,
    /// Whether to retry on failure.
    pub retry_on_failure: bool,
    /// Maximum number of retries (0 = no retries).
    pub max_retries: u32,
    /// Current retry count for the current attempt.
    pub current_retries: u32,
    /// Timestamp when this task was created.
    pub created_at: u64,
}

impl ScheduledTask {
    /// Create a new task with sensible defaults.
    pub fn new(id: u64, name: &str, command: &str, frequency: ScheduleFrequency, now: u64) -> Self {
        Self {
            id,
            name: name.to_string(),
            command: command.to_string(),
            frequency,
            enabled: true,
            next_run_timestamp: now,
            last_run_timestamp: 0,
            last_result: None,
            retry_on_failure: false,
            max_retries: 0,
            current_retries: 0,
            created_at: now,
        }
    }

    /// Whether this task has ever been executed.
    pub fn has_run(&self) -> bool {
        self.last_run_timestamp > 0
    }

    /// Whether the last execution succeeded.
    pub fn last_succeeded(&self) -> bool {
        self.last_result.as_ref().is_some_and(|r| r.is_ok())
    }

    /// Whether the last execution failed.
    pub fn last_failed(&self) -> bool {
        self.last_result.as_ref().is_some_and(|r| !r.is_ok())
    }

    /// Whether this task can retry after its current failure.
    pub fn can_retry(&self) -> bool {
        self.retry_on_failure && self.current_retries < self.max_retries
    }

    /// Display text for the last result column.
    pub fn result_display(&self) -> &str {
        match &self.last_result {
            None => "Never run",
            Some(TaskResult::Ok) => "OK",
            Some(TaskResult::Error(msg)) => msg.as_str(),
        }
    }
}

// ============================================================================
// TaskHistory
// ============================================================================

/// A single entry in the execution history log.
#[derive(Clone, Debug)]
pub struct TaskHistoryEntry {
    /// ID of the task that was executed.
    pub task_id: u64,
    /// Name of the task (snapshot at time of execution).
    pub task_name: String,
    /// Unix timestamp when execution started.
    pub timestamp: u64,
    /// Whether execution succeeded.
    pub success: bool,
    /// Duration of execution in milliseconds.
    pub duration_ms: u64,
    /// Error message if execution failed.
    pub error: Option<String>,
}

/// Persistent log of task executions.
#[derive(Clone, Debug, Default)]
pub struct TaskHistory {
    entries: Vec<TaskHistoryEntry>,
    /// Maximum number of entries to retain.
    max_entries: usize,
}

impl TaskHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 1000,
        }
    }

    #[must_use]
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Record a successful execution.
    pub fn record_success(&mut self, task_id: u64, task_name: &str, timestamp: u64, duration_ms: u64) {
        self.add_entry(TaskHistoryEntry {
            task_id,
            task_name: task_name.to_string(),
            timestamp,
            success: true,
            duration_ms,
            error: None,
        });
    }

    /// Record a failed execution.
    pub fn record_failure(
        &mut self,
        task_id: u64,
        task_name: &str,
        timestamp: u64,
        duration_ms: u64,
        error: &str,
    ) {
        self.add_entry(TaskHistoryEntry {
            task_id,
            task_name: task_name.to_string(),
            timestamp,
            success: false,
            duration_ms,
            error: Some(error.to_string()),
        });
    }

    fn add_entry(&mut self, entry: TaskHistoryEntry) {
        self.entries.push(entry);
        // Trim to max_entries if needed.
        if self.entries.len() > self.max_entries {
            let excess = self.entries.len() - self.max_entries;
            self.entries.drain(..excess);
        }
    }

    /// All entries, oldest first.
    pub fn entries(&self) -> &[TaskHistoryEntry] {
        &self.entries
    }

    /// Entries for a specific task, oldest first.
    pub fn entries_for_task(&self, task_id: u64) -> Vec<&TaskHistoryEntry> {
        self.entries.iter().filter(|e| e.task_id == task_id).collect()
    }

    /// Most recent entries, newest first, up to `limit`.
    pub fn recent(&self, limit: usize) -> Vec<&TaskHistoryEntry> {
        self.entries.iter().rev().take(limit).collect()
    }

    /// Total number of recorded executions.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Number of successful executions.
    pub fn success_count(&self) -> usize {
        self.entries.iter().filter(|e| e.success).count()
    }

    /// Number of failed executions.
    pub fn failure_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.success).count()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================================
// TaskScheduler
// ============================================================================

/// Manages a collection of scheduled tasks.
pub struct TaskScheduler {
    /// All tasks, keyed by ID.
    tasks: BTreeMap<u64, ScheduledTask>,
    /// Next ID to assign.
    next_id: u64,
    /// Execution history.
    pub history: TaskHistory,
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            next_id: 1,
            history: TaskHistory::new(),
        }
    }

    /// Add a new task. Returns the assigned task ID.
    pub fn add_task(
        &mut self,
        name: &str,
        command: &str,
        frequency: ScheduleFrequency,
        now: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let mut task = ScheduledTask::new(id, name, command, frequency, now);
        task.next_run_timestamp = calculate_next_run(&task.frequency, now);
        self.tasks.insert(id, task);
        id
    }

    /// Remove a task by ID. Returns true if it existed.
    pub fn remove_task(&mut self, id: u64) -> bool {
        self.tasks.remove(&id).is_some()
    }

    /// Enable a task.
    pub fn enable_task(&mut self, id: u64) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable a task.
    pub fn disable_task(&mut self, id: u64) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.enabled = false;
            true
        } else {
            false
        }
    }

    /// Get a task by ID.
    pub fn get_task(&self, id: u64) -> Option<&ScheduledTask> {
        self.tasks.get(&id)
    }

    /// Get a mutable reference to a task by ID.
    pub fn get_task_mut(&mut self, id: u64) -> Option<&mut ScheduledTask> {
        self.tasks.get_mut(&id)
    }

    /// List all tasks sorted by next run time.
    pub fn list_tasks(&self) -> Vec<&ScheduledTask> {
        let mut tasks: Vec<&ScheduledTask> = self.tasks.values().collect();
        tasks.sort_by_key(|t| t.next_run_timestamp);
        tasks
    }

    /// List only enabled tasks sorted by next run time.
    pub fn list_enabled_tasks(&self) -> Vec<&ScheduledTask> {
        let mut tasks: Vec<&ScheduledTask> = self
            .tasks
            .values()
            .filter(|t| t.enabled)
            .collect();
        tasks.sort_by_key(|t| t.next_run_timestamp);
        tasks
    }

    /// Check which tasks are due to run at or before the given timestamp.
    pub fn check_due(&self, now_timestamp: u64) -> Vec<&ScheduledTask> {
        let mut due: Vec<&ScheduledTask> = self
            .tasks
            .values()
            .filter(|t| t.enabled && t.next_run_timestamp <= now_timestamp)
            .collect();
        due.sort_by_key(|t| t.next_run_timestamp);
        due
    }

    /// Mark a task as completed successfully.
    pub fn mark_completed(&mut self, id: u64, now: u64, duration_ms: u64) {
        if let Some(task) = self.tasks.get_mut(&id) {
            let task_name = task.name.clone();
            task.last_run_timestamp = now;
            task.last_result = Some(TaskResult::Ok);
            task.current_retries = 0;

            // For one-shot tasks, disable after completion.
            if task.frequency == ScheduleFrequency::Once {
                task.enabled = false;
                task.next_run_timestamp = u64::MAX;
            } else {
                task.next_run_timestamp = calculate_next_run(&task.frequency, now);
            }

            self.history.record_success(id, &task_name, now, duration_ms);
        }
    }

    /// Mark a task as failed.
    pub fn mark_failed(&mut self, id: u64, error_msg: &str, now: u64, duration_ms: u64) {
        if let Some(task) = self.tasks.get_mut(&id) {
            let task_name = task.name.clone();
            task.last_run_timestamp = now;
            task.last_result = Some(TaskResult::Error(error_msg.to_string()));

            if task.can_retry() {
                task.current_retries = task.current_retries.saturating_add(1);
                // Schedule retry in 60 seconds.
                task.next_run_timestamp = now.saturating_add(60);
            } else {
                task.current_retries = 0;
                if task.frequency == ScheduleFrequency::Once {
                    task.enabled = false;
                    task.next_run_timestamp = u64::MAX;
                } else {
                    task.next_run_timestamp = calculate_next_run(&task.frequency, now);
                }
            }

            self.history
                .record_failure(id, &task_name, now, duration_ms, error_msg);
        }
    }

    /// Total number of tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Number of enabled tasks.
    pub fn enabled_count(&self) -> usize {
        self.tasks.values().filter(|t| t.enabled).count()
    }

    /// Update a task's name, command, and frequency.
    pub fn update_task(
        &mut self,
        id: u64,
        name: &str,
        command: &str,
        frequency: ScheduleFrequency,
        now: u64,
    ) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.name = name.to_string();
            task.command = command.to_string();
            task.frequency = frequency;
            task.next_run_timestamp = calculate_next_run(&task.frequency, now);
            true
        } else {
            false
        }
    }

    /// Set retry policy on a task.
    pub fn set_retry_policy(&mut self, id: u64, retry_on_failure: bool, max_retries: u32) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.retry_on_failure = retry_on_failure;
            task.max_retries = max_retries;
            true
        } else {
            false
        }
    }
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// calculate_next_run — schedule calculation
// ============================================================================

/// Calculate the next run timestamp for a given frequency, based on the
/// current time.
///
/// This uses a simplified model where timestamps are unix epoch seconds.
/// For daily/weekly/monthly, it adds the appropriate number of seconds.
/// For cron expressions, it scans forward minute-by-minute (up to a
/// bounded limit) to find the next matching time.
pub fn calculate_next_run(frequency: &ScheduleFrequency, now: u64) -> u64 {
    const SECS_PER_MINUTE: u64 = 60;
    const SECS_PER_HOUR: u64 = 3600;
    const SECS_PER_DAY: u64 = 86400;

    match frequency {
        ScheduleFrequency::Once => now,
        ScheduleFrequency::Daily => now.saturating_add(SECS_PER_DAY),
        ScheduleFrequency::Weekly(_day) => now.saturating_add(SECS_PER_DAY * 7),
        ScheduleFrequency::Monthly(_day) => {
            // Approximate month as 30 days.
            now.saturating_add(SECS_PER_DAY * 30)
        }
        ScheduleFrequency::Hourly => now.saturating_add(SECS_PER_HOUR),
        ScheduleFrequency::EveryNMinutes(n) => {
            now.saturating_add((*n as u64).saturating_mul(SECS_PER_MINUTE))
        }
        ScheduleFrequency::Cron(expr) => {
            // Scan forward minute by minute, up to ~2 years, to find the next
            // matching minute.
            let max_scan = SECS_PER_MINUTE * 60 * 24 * 366 * 2; // ~2 years
            let mut candidate = now.saturating_add(SECS_PER_MINUTE);
            // Align to the start of the minute.
            candidate = candidate - (candidate % SECS_PER_MINUTE);
            let end = now.saturating_add(max_scan);

            while candidate <= end {
                let time = decompose_timestamp(candidate);
                if expr.matches(time.minute, time.hour, time.day, time.month, time.weekday) {
                    return candidate;
                }
                candidate = candidate.saturating_add(SECS_PER_MINUTE);
            }
            // If no match found within scan window, return far future.
            u64::MAX
        }
    }
}

/// Decomposed time fields from a unix timestamp.
/// Uses a simple algorithm (no timezone support -- assumes UTC).
struct DecomposedTime {
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    weekday: u8,
}

/// Decompose a unix epoch timestamp into calendar fields (UTC).
fn decompose_timestamp(ts: u64) -> DecomposedTime {
    let secs = ts;
    let minute = ((secs % 3600) / 60) as u8;
    let hour = ((secs % 86400) / 3600) as u8;

    // Days since epoch (1970-01-01, which was a Thursday = weekday 4).
    let days = (secs / 86400) as u64;
    let weekday = ((days + 4) % 7) as u8; // 0 = Sunday

    // Compute year, month, day from days since epoch.
    let (year, month, day) = days_to_ymd(days);
    let _ = year; // We only need month and day for cron matching.

    DecomposedTime {
        minute,
        hour,
        day: day as u8,
        month: month as u8,
        weekday,
    }
}

/// Convert days since epoch to (year, month, day).
/// All values are 1-based for month and day.
fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm adapted from Howard Hinnant's `civil_from_days`.
    let z = days_since_epoch as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year
    let mp = (5 * doy + 2) / 153; // month progress
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y as u64, m, d)
}

// ============================================================================
// TaskSchedulerConfig — simple text-based persistence
// ============================================================================

/// Serialization/deserialization for task scheduler state using a simple
/// line-based text format.
///
/// Format:
/// ```text
/// TASK|id|name|command|frequency_type|frequency_param|enabled|next_run|last_run|retry|max_retries|created_at
/// ```
pub struct TaskSchedulerConfig;

impl TaskSchedulerConfig {
    /// Serialize all tasks to a text config string.
    pub fn serialize(scheduler: &TaskScheduler) -> String {
        let mut lines = Vec::new();
        lines.push(String::from("# OurOS Task Scheduler Config"));
        lines.push(format!("VERSION|1"));

        for task in scheduler.tasks.values() {
            let freq_str = serialize_frequency(&task.frequency);
            let enabled_str = if task.enabled { "1" } else { "0" };
            let retry_str = if task.retry_on_failure { "1" } else { "0" };
            let last_result_str = match &task.last_result {
                None => String::from("none"),
                Some(TaskResult::Ok) => String::from("ok"),
                Some(TaskResult::Error(msg)) => format!("error:{}", msg.replace('|', "\\|")),
            };

            lines.push(format!(
                "TASK|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                task.id,
                task.name.replace('|', "\\|"),
                task.command.replace('|', "\\|"),
                freq_str,
                enabled_str,
                task.next_run_timestamp,
                task.last_run_timestamp,
                last_result_str,
                retry_str,
                task.max_retries,
                task.created_at,
            ));
        }

        lines.join("\n")
    }

    /// Deserialize tasks from a config string into a scheduler.
    pub fn deserialize(text: &str) -> Result<TaskScheduler, ConfigError> {
        let mut scheduler = TaskScheduler::new();
        let mut max_id: u64 = 0;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("VERSION|") {
                continue;
            }
            if line.starts_with("TASK|") {
                let task = parse_task_line(line)?;
                if task.id >= max_id {
                    max_id = task.id.saturating_add(1);
                }
                scheduler.tasks.insert(task.id, task);
            }
        }

        scheduler.next_id = max_id;
        Ok(scheduler)
    }
}

/// Errors from config parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// Wrong number of fields in a TASK line.
    InvalidFieldCount(usize),
    /// A numeric field could not be parsed.
    InvalidNumber(String),
    /// An invalid frequency type was encountered.
    InvalidFrequency(String),
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidFieldCount(n) => write!(f, "expected 12 fields in TASK line, got {n}"),
            Self::InvalidNumber(s) => write!(f, "invalid number in config: {s}"),
            Self::InvalidFrequency(s) => write!(f, "invalid frequency: {s}"),
        }
    }
}

/// Serialize a ScheduleFrequency to string.
fn serialize_frequency(freq: &ScheduleFrequency) -> String {
    match freq {
        ScheduleFrequency::Once => String::from("once"),
        ScheduleFrequency::Daily => String::from("daily"),
        ScheduleFrequency::Weekly(day) => format!("weekly:{}", *day as u8),
        ScheduleFrequency::Monthly(day) => format!("monthly:{day}"),
        ScheduleFrequency::Hourly => String::from("hourly"),
        ScheduleFrequency::EveryNMinutes(n) => format!("every_n_min:{n}"),
        ScheduleFrequency::Cron(expr) => format!("cron:{}", expr.to_string_repr()),
    }
}

/// Deserialize a ScheduleFrequency from string.
fn deserialize_frequency(s: &str) -> Result<ScheduleFrequency, ConfigError> {
    if s == "once" {
        return Ok(ScheduleFrequency::Once);
    }
    if s == "daily" {
        return Ok(ScheduleFrequency::Daily);
    }
    if s == "hourly" {
        return Ok(ScheduleFrequency::Hourly);
    }
    if let Some(rest) = s.strip_prefix("weekly:") {
        let day_num: u8 = rest
            .parse()
            .map_err(|_| ConfigError::InvalidNumber(rest.to_string()))?;
        let day = DayOfWeek::from_u8(day_num)
            .ok_or_else(|| ConfigError::InvalidFrequency(s.to_string()))?;
        return Ok(ScheduleFrequency::Weekly(day));
    }
    if let Some(rest) = s.strip_prefix("monthly:") {
        let day: u8 = rest
            .parse()
            .map_err(|_| ConfigError::InvalidNumber(rest.to_string()))?;
        return Ok(ScheduleFrequency::Monthly(day));
    }
    if let Some(rest) = s.strip_prefix("every_n_min:") {
        let n: u32 = rest
            .parse()
            .map_err(|_| ConfigError::InvalidNumber(rest.to_string()))?;
        return Ok(ScheduleFrequency::EveryNMinutes(n));
    }
    if let Some(rest) = s.strip_prefix("cron:") {
        let expr = CronExpr::parse(rest)
            .map_err(|_| ConfigError::InvalidFrequency(s.to_string()))?;
        return Ok(ScheduleFrequency::Cron(expr));
    }
    Err(ConfigError::InvalidFrequency(s.to_string()))
}

/// Parse a single TASK line from the config file.
fn parse_task_line(line: &str) -> Result<ScheduledTask, ConfigError> {
    let parts: Vec<&str> = line.splitn(12, '|').collect();
    if parts.len() < 12 {
        return Err(ConfigError::InvalidFieldCount(parts.len()));
    }

    // parts[0] is "TASK"
    let id: u64 = parts[1]
        .parse()
        .map_err(|_| ConfigError::InvalidNumber(parts[1].to_string()))?;
    let name = parts[2].replace("\\|", "|");
    let command = parts[3].replace("\\|", "|");
    let frequency = deserialize_frequency(parts[4])?;
    let enabled = parts[5] == "1";
    let next_run: u64 = parts[6]
        .parse()
        .map_err(|_| ConfigError::InvalidNumber(parts[6].to_string()))?;
    let last_run: u64 = parts[7]
        .parse()
        .map_err(|_| ConfigError::InvalidNumber(parts[7].to_string()))?;

    let last_result = match parts[8] {
        "none" => None,
        "ok" => Some(TaskResult::Ok),
        s if s.starts_with("error:") => {
            Some(TaskResult::Error(s[6..].replace("\\|", "|").to_string()))
        }
        _ => None,
    };

    let retry = parts[9] == "1";
    let max_retries: u32 = parts[10]
        .parse()
        .map_err(|_| ConfigError::InvalidNumber(parts[10].to_string()))?;
    let created_at: u64 = parts[11]
        .trim()
        .parse()
        .map_err(|_| ConfigError::InvalidNumber(parts[11].to_string()))?;

    Ok(ScheduledTask {
        id,
        name,
        command,
        frequency,
        enabled,
        next_run_timestamp: next_run,
        last_run_timestamp: last_run,
        last_result,
        retry_on_failure: retry,
        max_retries,
        current_retries: 0,
        created_at,
    })
}

// ============================================================================
// SchedulerUI — GUI view state
// ============================================================================

/// Which tab the UI is currently showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiTab {
    /// Main task list.
    Tasks,
    /// Execution history.
    History,
}

/// Which dialog is open (if any).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiDialog {
    None,
    /// Add task dialog.
    AddTask,
    /// Edit task dialog (contains task ID).
    EditTask(u64),
    /// Confirm delete dialog (contains task ID).
    ConfirmDelete(u64),
}

/// Form state for the add/edit task dialog.
#[derive(Clone, Debug)]
pub struct TaskFormState {
    pub name: String,
    pub command: String,
    pub frequency_index: usize,
    pub enabled: bool,
    pub cron_expr: String,
    pub weekly_day: u8,
    pub monthly_day: u8,
    pub interval_minutes: u32,
}

impl TaskFormState {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            frequency_index: 0,
            enabled: true,
            cron_expr: String::from("0 * * * *"),
            weekly_day: 1,
            monthly_day: 1,
            interval_minutes: 30,
        }
    }

    /// Populate from an existing task.
    pub fn from_task(task: &ScheduledTask) -> Self {
        let (freq_index, cron_expr, weekly_day, monthly_day, interval_minutes) =
            match &task.frequency {
                ScheduleFrequency::Once => (0, String::new(), 1u8, 1u8, 30u32),
                ScheduleFrequency::Daily => (1, String::new(), 1, 1, 30),
                ScheduleFrequency::Weekly(day) => (2, String::new(), *day as u8, 1, 30),
                ScheduleFrequency::Monthly(d) => (3, String::new(), 1, *d, 30),
                ScheduleFrequency::Hourly => (4, String::new(), 1, 1, 30),
                ScheduleFrequency::EveryNMinutes(n) => (5, String::new(), 1, 1, *n),
                ScheduleFrequency::Cron(expr) => (6, expr.to_string_repr(), 1, 1, 30),
            };

        Self {
            name: task.name.clone(),
            command: task.command.clone(),
            frequency_index: freq_index,
            enabled: task.enabled,
            cron_expr,
            weekly_day,
            monthly_day,
            interval_minutes,
        }
    }

    /// Build the ScheduleFrequency from the current form state.
    pub fn build_frequency(&self) -> Option<ScheduleFrequency> {
        match self.frequency_index {
            0 => Some(ScheduleFrequency::Once),
            1 => Some(ScheduleFrequency::Daily),
            2 => {
                DayOfWeek::from_u8(self.weekly_day)
                    .map(ScheduleFrequency::Weekly)
            }
            3 => Some(ScheduleFrequency::Monthly(self.monthly_day)),
            4 => Some(ScheduleFrequency::Hourly),
            5 => Some(ScheduleFrequency::EveryNMinutes(self.interval_minutes)),
            6 => CronExpr::parse(&self.cron_expr).ok().map(ScheduleFrequency::Cron),
            _ => None,
        }
    }
}

impl Default for TaskFormState {
    fn default() -> Self {
        Self::new()
    }
}

/// Frequency type labels for the UI dropdown.
const FREQUENCY_LABELS: &[&str] = &[
    "Once",
    "Daily",
    "Weekly",
    "Monthly",
    "Hourly",
    "Every N Minutes",
    "Cron Expression",
];

/// Complete UI state for the task scheduler application.
pub struct SchedulerUI {
    /// Current tab.
    pub tab: UiTab,
    /// Current dialog.
    pub dialog: UiDialog,
    /// The scheduler engine.
    pub scheduler: TaskScheduler,
    /// Selected task ID (if any).
    pub selected_task_id: Option<u64>,
    /// Form state for add/edit dialog.
    pub form: TaskFormState,
    /// Scroll offset for the task list.
    pub task_list_scroll: f32,
    /// Scroll offset for the history list.
    pub history_scroll: f32,
    /// Status message displayed temporarily.
    pub status_message: Option<String>,
}

impl SchedulerUI {
    pub fn new() -> Self {
        Self {
            tab: UiTab::Tasks,
            dialog: UiDialog::None,
            scheduler: TaskScheduler::new(),
            selected_task_id: None,
            form: TaskFormState::new(),
            task_list_scroll: 0.0,
            history_scroll: 0.0,
            status_message: None,
        }
    }

    // -- tab navigation -------------------------------------------------------

    pub fn switch_to_tasks(&mut self) {
        self.tab = UiTab::Tasks;
    }

    pub fn switch_to_history(&mut self) {
        self.tab = UiTab::History;
    }

    // -- task selection -------------------------------------------------------

    pub fn select_task(&mut self, id: u64) {
        self.selected_task_id = Some(id);
    }

    pub fn deselect_task(&mut self) {
        self.selected_task_id = None;
    }

    // -- dialog management ----------------------------------------------------

    pub fn open_add_dialog(&mut self) {
        self.form = TaskFormState::new();
        self.dialog = UiDialog::AddTask;
    }

    pub fn open_edit_dialog(&mut self, id: u64) {
        if let Some(task) = self.scheduler.get_task(id) {
            self.form = TaskFormState::from_task(task);
            self.dialog = UiDialog::EditTask(id);
        }
    }

    pub fn open_delete_dialog(&mut self, id: u64) {
        self.dialog = UiDialog::ConfirmDelete(id);
    }

    pub fn close_dialog(&mut self) {
        self.dialog = UiDialog::None;
    }

    // -- actions --------------------------------------------------------------

    /// Commit the add-task form: create a new task.
    pub fn commit_add_task(&mut self, now: u64) -> Option<u64> {
        if self.form.name.is_empty() || self.form.command.is_empty() {
            self.status_message = Some(String::from("Name and command are required"));
            return None;
        }
        let freq = match self.form.build_frequency() {
            Some(f) => f,
            None => {
                self.status_message = Some(String::from("Invalid frequency"));
                return None;
            }
        };
        let id = self.scheduler.add_task(&self.form.name, &self.form.command, freq, now);
        if self.form.enabled {
            self.scheduler.enable_task(id);
        } else {
            self.scheduler.disable_task(id);
        }
        self.dialog = UiDialog::None;
        self.status_message = Some(format!("Task '{}' added", self.form.name));
        Some(id)
    }

    /// Commit the edit-task form: update the existing task.
    pub fn commit_edit_task(&mut self, id: u64, now: u64) -> bool {
        if self.form.name.is_empty() || self.form.command.is_empty() {
            self.status_message = Some(String::from("Name and command are required"));
            return false;
        }
        let freq = match self.form.build_frequency() {
            Some(f) => f,
            None => {
                self.status_message = Some(String::from("Invalid frequency"));
                return false;
            }
        };
        let updated = self.scheduler.update_task(id, &self.form.name, &self.form.command, freq, now);
        if updated {
            if self.form.enabled {
                self.scheduler.enable_task(id);
            } else {
                self.scheduler.disable_task(id);
            }
            self.dialog = UiDialog::None;
            self.status_message = Some(format!("Task '{}' updated", self.form.name));
        }
        updated
    }

    /// Delete the selected task.
    pub fn confirm_delete_task(&mut self, id: u64) -> bool {
        let removed = self.scheduler.remove_task(id);
        if removed {
            if self.selected_task_id == Some(id) {
                self.selected_task_id = None;
            }
            self.status_message = Some(String::from("Task deleted"));
        }
        self.dialog = UiDialog::None;
        removed
    }

    /// Toggle enabled/disabled on the selected task.
    pub fn toggle_selected_task(&mut self) {
        if let Some(id) = self.selected_task_id {
            if let Some(task) = self.scheduler.get_task(id) {
                if task.enabled {
                    self.scheduler.disable_task(id);
                } else {
                    self.scheduler.enable_task(id);
                }
            }
        }
    }

    // -- rendering ------------------------------------------------------------

    /// Render the entire UI.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Window background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Header.
        self.render_header(&mut cmds, width);

        // Toolbar.
        self.render_toolbar(&mut cmds, width);

        // Tab bar.
        self.render_tab_bar(&mut cmds, width);

        // Content area.
        let content_top = HEADER_HEIGHT + TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        let content_height = height - content_top;

        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_top,
            width,
            height: content_height,
        });

        match self.tab {
            UiTab::Tasks => self.render_task_list(&mut cmds, width, content_top, content_height),
            UiTab::History => self.render_history(&mut cmds, width, content_top, content_height),
        }

        cmds.push(RenderCommand::PopClip);

        // Status bar at the very bottom.
        if let Some(msg) = &self.status_message {
            self.render_status_bar(&mut cmds, width, height, msg);
        }

        // Dialog overlay (if any).
        match &self.dialog {
            UiDialog::None => {}
            UiDialog::AddTask => self.render_add_edit_dialog(&mut cmds, width, height, "Add Task", None),
            UiDialog::EditTask(id) => {
                self.render_add_edit_dialog(&mut cmds, width, height, "Edit Task", Some(*id));
            }
            UiDialog::ConfirmDelete(id) => {
                self.render_confirm_delete_dialog(&mut cmds, width, height, *id);
            }
        }

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: HEADER_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii {
                top_left: CORNER_RADIUS,
                top_right: CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: (HEADER_HEIGHT - FONT_SIZE_HEADING) / 2.0,
            text: String::from("Task Scheduler"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
        });

        // Task count in header.
        let count_text = format!(
            "{} tasks ({} enabled)",
            self.scheduler.task_count(),
            self.scheduler.enabled_count()
        );
        cmds.push(RenderCommand::Text {
            x: width - 200.0,
            y: (HEADER_HEIGHT - FONT_SIZE_SMALL) / 2.0,
            text: count_text,
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        let y = HEADER_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: TOOLBAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let btn_y = y + (TOOLBAR_HEIGHT - BUTTON_HEIGHT) / 2.0;
        let mut bx = PADDING;

        // Add button.
        self.render_button(cmds, bx, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Add", COLOR_GREEN);
        bx += BUTTON_WIDTH + 8.0;

        // Edit button.
        let edit_color = if self.selected_task_id.is_some() { COLOR_BLUE } else { COLOR_SURFACE2 };
        self.render_button(cmds, bx, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Edit", edit_color);
        bx += BUTTON_WIDTH + 8.0;

        // Remove button.
        let remove_color = if self.selected_task_id.is_some() { COLOR_RED } else { COLOR_SURFACE2 };
        self.render_button(cmds, bx, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Remove", remove_color);
        bx += BUTTON_WIDTH + 8.0;

        // Enable/Disable toggle button.
        let toggle_label = if let Some(id) = self.selected_task_id {
            if self.scheduler.get_task(id).is_some_and(|t| t.enabled) {
                "Disable"
            } else {
                "Enable"
            }
        } else {
            "Enable"
        };
        let toggle_color = if self.selected_task_id.is_some() { COLOR_PEACH } else { COLOR_SURFACE2 };
        self.render_button(cmds, bx, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, toggle_label, toggle_color);
        let _ = bx;
    }

    fn render_tab_bar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        let y = HEADER_HEIGHT + TOOLBAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: TAB_BAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_BAR_HEIGHT - 1.0,
            x2: width,
            y2: y + TAB_BAR_HEIGHT - 1.0,
            color: COLOR_SURFACE1,
            width: 1.0,
        });

        // Tasks tab.
        let tasks_selected = self.tab == UiTab::Tasks;
        let tasks_color = if tasks_selected { COLOR_BLUE } else { COLOR_SUBTEXT };
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + (TAB_BAR_HEIGHT - FONT_SIZE) / 2.0,
            text: String::from("Tasks"),
            color: tasks_color,
            font_size: FONT_SIZE,
            font_weight: if tasks_selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
            max_width: Some(80.0),
        });

        if tasks_selected {
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: y + TAB_BAR_HEIGHT - 3.0,
                width: 40.0,
                height: 3.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::all(1.5),
            });
        }

        // History tab.
        let hist_selected = self.tab == UiTab::History;
        let hist_color = if hist_selected { COLOR_BLUE } else { COLOR_SUBTEXT };
        cmds.push(RenderCommand::Text {
            x: PADDING + 80.0,
            y: y + (TAB_BAR_HEIGHT - FONT_SIZE) / 2.0,
            text: String::from("History"),
            color: hist_color,
            font_size: FONT_SIZE,
            font_weight: if hist_selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
            max_width: Some(80.0),
        });

        if hist_selected {
            cmds.push(RenderCommand::FillRect {
                x: PADDING + 80.0,
                y: y + TAB_BAR_HEIGHT - 3.0,
                width: 50.0,
                height: 3.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::all(1.5),
            });
        }
    }

    fn render_task_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        top: f32,
        _height: f32,
    ) {
        // Column headers.
        let header_y = top;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: header_y,
            width,
            height: ROW_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        let col_enabled_x = PADDING;
        let col_name_x: f32 = 50.0;
        let col_command_x: f32 = 200.0;
        let col_freq_x: f32 = 420.0;
        let col_next_x: f32 = 560.0;
        let col_result_x: f32 = 700.0;

        let header_text_y = header_y + (ROW_HEIGHT - FONT_SIZE_SMALL) / 2.0;

        for (label, x) in [
            ("On", col_enabled_x),
            ("Name", col_name_x),
            ("Command", col_command_x),
            ("Frequency", col_freq_x),
            ("Next Run", col_next_x),
            ("Last Result", col_result_x),
        ] {
            cmds.push(RenderCommand::Text {
                x,
                y: header_text_y,
                text: label.to_string(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(140.0),
            });
        }

        // Task rows.
        let tasks = self.scheduler.list_tasks();
        for (i, task) in tasks.iter().enumerate() {
            let row_y = top + ROW_HEIGHT + (i as f32) * ROW_HEIGHT;
            let is_selected = self.selected_task_id == Some(task.id);

            // Row background.
            let row_bg = if is_selected {
                COLOR_SURFACE1
            } else if i % 2 == 0 {
                COLOR_BASE
            } else {
                COLOR_SURFACE0
            };
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: row_y,
                width,
                height: ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });

            let text_y = row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0;

            // Enabled checkbox.
            let cb_y = row_y + (ROW_HEIGHT - CHECKBOX_SIZE) / 2.0;
            cmds.push(RenderCommand::StrokeRect {
                x: col_enabled_x,
                y: cb_y,
                width: CHECKBOX_SIZE,
                height: CHECKBOX_SIZE,
                color: COLOR_SUBTEXT,
                line_width: 1.0,
                corner_radii: CornerRadii::all(3.0),
            });
            if task.enabled {
                cmds.push(RenderCommand::FillRect {
                    x: col_enabled_x + 3.0,
                    y: cb_y + 3.0,
                    width: CHECKBOX_SIZE - 6.0,
                    height: CHECKBOX_SIZE - 6.0,
                    color: COLOR_GREEN,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            // Name.
            let name_color = if task.enabled { COLOR_TEXT } else { COLOR_SUBTEXT };
            cmds.push(RenderCommand::Text {
                x: col_name_x,
                y: text_y,
                text: task.name.clone(),
                color: name_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(145.0),
            });

            // Command.
            cmds.push(RenderCommand::Text {
                x: col_command_x,
                y: text_y,
                text: task.command.clone(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(215.0),
            });

            // Frequency.
            cmds.push(RenderCommand::Text {
                x: col_freq_x,
                y: text_y,
                text: task.frequency.display_name(),
                color: COLOR_MAUVE,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(135.0),
            });

            // Next run.
            let next_run_text = if task.next_run_timestamp == u64::MAX {
                String::from("--")
            } else {
                format_timestamp(task.next_run_timestamp)
            };
            cmds.push(RenderCommand::Text {
                x: col_next_x,
                y: text_y,
                text: next_run_text,
                color: COLOR_TEAL,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(135.0),
            });

            // Last result.
            let result_color = match &task.last_result {
                None => COLOR_SUBTEXT,
                Some(TaskResult::Ok) => COLOR_GREEN,
                Some(TaskResult::Error(_)) => COLOR_RED,
            };
            cmds.push(RenderCommand::Text {
                x: col_result_x,
                y: text_y,
                text: task.result_display().to_string(),
                color: result_color,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(115.0),
            });
        }

        // Empty state.
        if tasks.is_empty() {
            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 80.0,
                y: top + ROW_HEIGHT + 40.0,
                text: String::from("No tasks scheduled"),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }
    }

    fn render_history(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        top: f32,
        _height: f32,
    ) {
        // Column headers.
        let header_y = top;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: header_y,
            width,
            height: ROW_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        let col_time_x = PADDING;
        let col_name_x: f32 = 180.0;
        let col_status_x: f32 = 380.0;
        let col_duration_x: f32 = 500.0;
        let col_error_x: f32 = 620.0;

        let header_text_y = header_y + (ROW_HEIGHT - FONT_SIZE_SMALL) / 2.0;
        for (label, x) in [
            ("Time", col_time_x),
            ("Task", col_name_x),
            ("Status", col_status_x),
            ("Duration", col_duration_x),
            ("Error", col_error_x),
        ] {
            cmds.push(RenderCommand::Text {
                x,
                y: header_text_y,
                text: label.to_string(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(180.0),
            });
        }

        // History rows (newest first).
        let entries = self.scheduler.history.recent(100);
        for (i, entry) in entries.iter().enumerate() {
            let row_y = top + ROW_HEIGHT + (i as f32) * ROW_HEIGHT;
            let row_bg = if i % 2 == 0 { COLOR_BASE } else { COLOR_SURFACE0 };
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: row_y,
                width,
                height: ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });

            let text_y = row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0;

            // Timestamp.
            cmds.push(RenderCommand::Text {
                x: col_time_x,
                y: text_y,
                text: format_timestamp(entry.timestamp),
                color: COLOR_TEAL,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(165.0),
            });

            // Task name.
            cmds.push(RenderCommand::Text {
                x: col_name_x,
                y: text_y,
                text: entry.task_name.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(195.0),
            });

            // Status.
            let (status_text, status_color) = if entry.success {
                ("OK", COLOR_GREEN)
            } else {
                ("Failed", COLOR_RED)
            };
            cmds.push(RenderCommand::Text {
                x: col_status_x,
                y: text_y,
                text: status_text.to_string(),
                color: status_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(110.0),
            });

            // Duration.
            cmds.push(RenderCommand::Text {
                x: col_duration_x,
                y: text_y,
                text: format_duration_ms(entry.duration_ms),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(115.0),
            });

            // Error.
            if let Some(err) = &entry.error {
                cmds.push(RenderCommand::Text {
                    x: col_error_x,
                    y: text_y,
                    text: err.clone(),
                    color: COLOR_RED,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(195.0),
                });
            }
        }

        // Empty state.
        if entries.is_empty() {
            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 60.0,
                y: top + ROW_HEIGHT + 40.0,
                text: String::from("No history yet"),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }
    }

    fn render_status_bar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
        message: &str,
    ) {
        let bar_h = 24.0;
        let y = height - bar_h;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: bar_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        });

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + (bar_h - FONT_SIZE_SMALL) / 2.0,
            text: message.to_string(),
            color: COLOR_YELLOW,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
        });
    }

    fn render_add_edit_dialog(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
        title: &str,
        _task_id: Option<u64>,
    ) {
        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        let dialog_w: f32 = 440.0;
        let dialog_h: f32 = 380.0;
        let dx = (width - dialog_w) / 2.0;
        let dy = (height - dialog_h) / 2.0;

        // Dialog background.
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: title.to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - PADDING * 2.0),
        });

        let mut field_y = dy + 44.0;
        let label_x = dx + PADDING;
        let value_x = dx + 130.0;
        let field_spacing = 36.0;

        // Name field.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: field_y,
            text: String::from("Name:"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });
        self.render_text_field(cmds, value_x, field_y - 2.0, 280.0, &self.form.name);
        field_y += field_spacing;

        // Command field.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: field_y,
            text: String::from("Command:"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });
        self.render_text_field(cmds, value_x, field_y - 2.0, 280.0, &self.form.command);
        field_y += field_spacing;

        // Frequency selector.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: field_y,
            text: String::from("Frequency:"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });
        let freq_label = FREQUENCY_LABELS
            .get(self.form.frequency_index)
            .unwrap_or(&"Unknown");
        self.render_text_field(cmds, value_x, field_y - 2.0, 280.0, freq_label);
        field_y += field_spacing;

        // Frequency-specific parameter.
        match self.form.frequency_index {
            2 => {
                // Weekly: show day selector.
                let day = DayOfWeek::from_u8(self.form.weekly_day)
                    .map(|d| d.display_name())
                    .unwrap_or("Monday");
                cmds.push(RenderCommand::Text {
                    x: label_x,
                    y: field_y,
                    text: String::from("Day of week:"),
                    color: COLOR_SUBTEXT,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(100.0),
                });
                self.render_text_field(cmds, value_x, field_y - 2.0, 280.0, day);
                field_y += field_spacing;
            }
            3 => {
                // Monthly: show day selector.
                cmds.push(RenderCommand::Text {
                    x: label_x,
                    y: field_y,
                    text: String::from("Day of month:"),
                    color: COLOR_SUBTEXT,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(100.0),
                });
                let day_text = format!("{}", self.form.monthly_day);
                self.render_text_field(cmds, value_x, field_y - 2.0, 280.0, &day_text);
                field_y += field_spacing;
            }
            5 => {
                // Every N minutes.
                cmds.push(RenderCommand::Text {
                    x: label_x,
                    y: field_y,
                    text: String::from("Minutes:"),
                    color: COLOR_SUBTEXT,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(100.0),
                });
                let min_text = format!("{}", self.form.interval_minutes);
                self.render_text_field(cmds, value_x, field_y - 2.0, 280.0, &min_text);
                field_y += field_spacing;
            }
            6 => {
                // Cron expression.
                cmds.push(RenderCommand::Text {
                    x: label_x,
                    y: field_y,
                    text: String::from("Cron:"),
                    color: COLOR_SUBTEXT,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(100.0),
                });
                self.render_text_field(cmds, value_x, field_y - 2.0, 280.0, &self.form.cron_expr);
                field_y += field_spacing;
            }
            _ => {}
        }

        // Enabled checkbox.
        let cb_y = field_y;
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cb_y,
            text: String::from("Enabled:"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: value_x,
            y: cb_y - 1.0,
            width: CHECKBOX_SIZE,
            height: CHECKBOX_SIZE,
            color: COLOR_SUBTEXT,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });
        if self.form.enabled {
            cmds.push(RenderCommand::FillRect {
                x: value_x + 3.0,
                y: cb_y + 2.0,
                width: CHECKBOX_SIZE - 6.0,
                height: CHECKBOX_SIZE - 6.0,
                color: COLOR_GREEN,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Dialog buttons.
        let btn_y = dy + dialog_h - BUTTON_HEIGHT - PADDING;
        let cancel_x = dx + dialog_w - PADDING - BUTTON_WIDTH;
        let save_x = cancel_x - 8.0 - BUTTON_WIDTH;

        self.render_button(cmds, save_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Save", COLOR_GREEN);
        self.render_button(cmds, cancel_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Cancel", COLOR_SURFACE2);
    }

    fn render_confirm_delete_dialog(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
        task_id: u64,
    ) {
        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        let dialog_w: f32 = 360.0;
        let dialog_h: f32 = 160.0;
        let dx = (width - dialog_w) / 2.0;
        let dy = (height - dialog_h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: String::from("Confirm Delete"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - PADDING * 2.0),
        });

        // Message.
        let task_name = self
            .scheduler
            .get_task(task_id)
            .map(|t| t.name.as_str())
            .unwrap_or("this task");
        let msg = format!("Delete task '{task_name}'? This cannot be undone.");
        cmds.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 52.0,
            text: msg,
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - PADDING * 2.0),
        });

        // Buttons.
        let btn_y = dy + dialog_h - BUTTON_HEIGHT - PADDING;
        let cancel_x = dx + dialog_w - PADDING - BUTTON_WIDTH;
        let delete_x = cancel_x - 8.0 - BUTTON_WIDTH;

        self.render_button(cmds, delete_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Delete", COLOR_RED);
        self.render_button(cmds, cancel_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Cancel", COLOR_SURFACE2);
    }

    fn render_text_field(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        value: &str,
    ) {
        let field_h = 24.0;

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: field_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height: field_h,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 6.0,
            y: y + (field_h - FONT_SIZE) / 2.0,
            text: value.to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 12.0),
        });
    }

    fn render_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        bg: Color,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let text_width = label.len() as f32 * FONT_SIZE * 0.6;
        let text_x = x + (w - text_width) / 2.0;
        let text_y = y + (h - FONT_SIZE) / 2.0;

        cmds.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: label.to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w),
        });
    }
}

impl Default for SchedulerUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a unix timestamp into a human-readable UTC date/time string.
fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return String::from("Never");
    }
    if ts == u64::MAX {
        return String::from("--");
    }

    let dt = decompose_timestamp(ts);
    let (year, month, day) = days_to_ymd(ts / 86400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        dt.hour, dt.minute
    )
}

/// Format milliseconds into a readable duration.
fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{mins}m {secs}s")
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- CronField tests ----------------------------------------------------

    #[test]
    fn test_cron_field_any_matches_everything() {
        let f = CronField::Any;
        for i in 0..60 {
            assert!(f.matches(i));
        }
    }

    #[test]
    fn test_cron_field_value_matches_exact() {
        let f = CronField::Value(5);
        assert!(f.matches(5));
        assert!(!f.matches(4));
        assert!(!f.matches(6));
    }

    #[test]
    fn test_cron_field_list_matches_members() {
        let f = CronField::List(vec![1, 3, 5, 7]);
        assert!(f.matches(1));
        assert!(f.matches(5));
        assert!(!f.matches(2));
        assert!(!f.matches(8));
    }

    #[test]
    fn test_cron_field_range_matches_inclusive() {
        let f = CronField::Range(3, 7);
        assert!(!f.matches(2));
        assert!(f.matches(3));
        assert!(f.matches(5));
        assert!(f.matches(7));
        assert!(!f.matches(8));
    }

    #[test]
    fn test_cron_field_step_matches_multiples() {
        let f = CronField::Step(0, 15);
        assert!(f.matches(0));
        assert!(f.matches(15));
        assert!(f.matches(30));
        assert!(f.matches(45));
        assert!(!f.matches(10));
        assert!(!f.matches(1));
    }

    #[test]
    fn test_cron_field_step_with_base() {
        let f = CronField::Step(5, 10);
        assert!(f.matches(5));
        assert!(f.matches(15));
        assert!(f.matches(25));
        assert!(!f.matches(0));
        assert!(!f.matches(10));
    }

    #[test]
    fn test_cron_field_step_zero_step() {
        let f = CronField::Step(5, 0);
        assert!(f.matches(5));
        assert!(!f.matches(0));
        assert!(!f.matches(10));
    }

    // -- CronField parsing tests --------------------------------------------

    #[test]
    fn test_parse_cron_field_wildcard() {
        assert_eq!(CronField::parse("*"), Ok(CronField::Any));
    }

    #[test]
    fn test_parse_cron_field_single_value() {
        assert_eq!(CronField::parse("42"), Ok(CronField::Value(42)));
    }

    #[test]
    fn test_parse_cron_field_list() {
        assert_eq!(CronField::parse("1,3,5"), Ok(CronField::List(vec![1, 3, 5])));
    }

    #[test]
    fn test_parse_cron_field_list_deduplicates() {
        assert_eq!(CronField::parse("5,3,5,1"), Ok(CronField::List(vec![1, 3, 5])));
    }

    #[test]
    fn test_parse_cron_field_range() {
        assert_eq!(CronField::parse("1-5"), Ok(CronField::Range(1, 5)));
    }

    #[test]
    fn test_parse_cron_field_invalid_range() {
        assert_eq!(CronField::parse("5-1"), Err(CronParseError::InvalidRange(5, 1)));
    }

    #[test]
    fn test_parse_cron_field_step_from_zero() {
        assert_eq!(CronField::parse("*/15"), Ok(CronField::Step(0, 15)));
    }

    #[test]
    fn test_parse_cron_field_step_from_base() {
        assert_eq!(CronField::parse("5/10"), Ok(CronField::Step(5, 10)));
    }

    #[test]
    fn test_parse_cron_field_empty_is_error() {
        assert_eq!(CronField::parse(""), Err(CronParseError::EmptyField));
    }

    #[test]
    fn test_parse_cron_field_invalid_number() {
        assert!(matches!(CronField::parse("abc"), Err(CronParseError::InvalidNumber(_))));
    }

    // -- CronExpr parsing tests ---------------------------------------------

    #[test]
    fn test_parse_cron_expr_all_wildcards() {
        let expr = CronExpr::parse("* * * * *").expect("should parse");
        assert_eq!(expr.minute, CronField::Any);
        assert_eq!(expr.hour, CronField::Any);
        assert_eq!(expr.day_of_month, CronField::Any);
        assert_eq!(expr.month, CronField::Any);
        assert_eq!(expr.day_of_week, CronField::Any);
    }

    #[test]
    fn test_parse_cron_expr_specific_time() {
        let expr = CronExpr::parse("30 2 * * *").expect("should parse");
        assert_eq!(expr.minute, CronField::Value(30));
        assert_eq!(expr.hour, CronField::Value(2));
    }

    #[test]
    fn test_parse_cron_expr_wrong_field_count() {
        assert_eq!(
            CronExpr::parse("* * *"),
            Err(CronParseError::WrongFieldCount(3))
        );
    }

    #[test]
    fn test_parse_cron_expr_too_many_fields() {
        assert_eq!(
            CronExpr::parse("* * * * * *"),
            Err(CronParseError::WrongFieldCount(6))
        );
    }

    #[test]
    fn test_cron_expr_matches() {
        let expr = CronExpr::parse("30 2 15 6 *").expect("should parse");
        assert!(expr.matches(30, 2, 15, 6, 3));
        assert!(!expr.matches(0, 2, 15, 6, 3));
        assert!(!expr.matches(30, 3, 15, 6, 3));
    }

    #[test]
    fn test_cron_expr_every_15_minutes() {
        let expr = CronExpr::parse("*/15 * * * *").expect("should parse");
        assert!(expr.matches(0, 10, 1, 1, 0));
        assert!(expr.matches(15, 10, 1, 1, 0));
        assert!(expr.matches(30, 10, 1, 1, 0));
        assert!(expr.matches(45, 10, 1, 1, 0));
        assert!(!expr.matches(10, 10, 1, 1, 0));
    }

    #[test]
    fn test_cron_expr_weekdays_only() {
        let expr = CronExpr::parse("0 9 * * 1-5").expect("should parse");
        assert!(expr.matches(0, 9, 1, 1, 1)); // Monday
        assert!(expr.matches(0, 9, 1, 1, 5)); // Friday
        assert!(!expr.matches(0, 9, 1, 1, 0)); // Sunday
        assert!(!expr.matches(0, 9, 1, 1, 6)); // Saturday
    }

    #[test]
    fn test_cron_expr_to_string_repr_roundtrip() {
        let original = "30 2 15 6 1,3,5";
        let expr = CronExpr::parse(original).expect("should parse");
        let repr = expr.to_string_repr();
        let reparsed = CronExpr::parse(&repr).expect("should reparse");
        assert_eq!(expr, reparsed);
    }

    #[test]
    fn test_cron_expr_validation_minute_out_of_range() {
        assert!(matches!(
            CronExpr::parse("60 * * * *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    #[test]
    fn test_cron_expr_validation_hour_out_of_range() {
        assert!(matches!(
            CronExpr::parse("0 24 * * *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    #[test]
    fn test_cron_expr_validation_day_zero() {
        // Day of month must be 1-31.
        assert!(matches!(
            CronExpr::parse("0 0 0 * *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    #[test]
    fn test_cron_expr_validation_month_zero() {
        // Month must be 1-12.
        assert!(matches!(
            CronExpr::parse("0 0 * 0 *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    // -- DayOfWeek tests ----------------------------------------------------

    #[test]
    fn test_day_of_week_from_u8() {
        assert_eq!(DayOfWeek::from_u8(0), Some(DayOfWeek::Sunday));
        assert_eq!(DayOfWeek::from_u8(6), Some(DayOfWeek::Saturday));
        assert_eq!(DayOfWeek::from_u8(7), None);
    }

    #[test]
    fn test_day_of_week_display_names() {
        assert_eq!(DayOfWeek::Monday.display_name(), "Monday");
        assert_eq!(DayOfWeek::Friday.short_name(), "Fri");
    }

    // -- ScheduleFrequency tests --------------------------------------------

    #[test]
    fn test_frequency_display_names() {
        assert_eq!(ScheduleFrequency::Once.display_name(), "Once");
        assert_eq!(ScheduleFrequency::Daily.display_name(), "Daily");
        assert_eq!(ScheduleFrequency::Hourly.display_name(), "Hourly");
        assert_eq!(
            ScheduleFrequency::Weekly(DayOfWeek::Monday).display_name(),
            "Weekly (Monday)"
        );
        assert_eq!(
            ScheduleFrequency::Monthly(15).display_name(),
            "Monthly (day 15)"
        );
        assert_eq!(
            ScheduleFrequency::EveryNMinutes(30).display_name(),
            "Every 30 min"
        );
    }

    // -- TaskResult tests ---------------------------------------------------

    #[test]
    fn test_task_result_ok() {
        let r = TaskResult::Ok;
        assert!(r.is_ok());
        assert_eq!(r.display_str(), "OK");
    }

    #[test]
    fn test_task_result_error() {
        let r = TaskResult::Error(String::from("timeout"));
        assert!(!r.is_ok());
        assert_eq!(r.display_str(), "timeout");
    }

    // -- ScheduledTask tests ------------------------------------------------

    #[test]
    fn test_scheduled_task_new() {
        let task = ScheduledTask::new(1, "backup", "/usr/bin/backup", ScheduleFrequency::Daily, 1000);
        assert_eq!(task.id, 1);
        assert_eq!(task.name, "backup");
        assert!(task.enabled);
        assert!(!task.has_run());
        assert_eq!(task.result_display(), "Never run");
    }

    #[test]
    fn test_scheduled_task_last_succeeded() {
        let mut task = ScheduledTask::new(1, "t", "cmd", ScheduleFrequency::Once, 0);
        assert!(!task.last_succeeded());
        task.last_result = Some(TaskResult::Ok);
        assert!(task.last_succeeded());
    }

    #[test]
    fn test_scheduled_task_last_failed() {
        let mut task = ScheduledTask::new(1, "t", "cmd", ScheduleFrequency::Once, 0);
        assert!(!task.last_failed());
        task.last_result = Some(TaskResult::Error(String::from("err")));
        assert!(task.last_failed());
    }

    #[test]
    fn test_scheduled_task_can_retry() {
        let mut task = ScheduledTask::new(1, "t", "cmd", ScheduleFrequency::Once, 0);
        assert!(!task.can_retry());
        task.retry_on_failure = true;
        task.max_retries = 3;
        assert!(task.can_retry());
        task.current_retries = 3;
        assert!(!task.can_retry());
    }

    // -- TaskHistory tests --------------------------------------------------

    #[test]
    fn test_history_initially_empty() {
        let h = TaskHistory::new();
        assert_eq!(h.count(), 0);
        assert_eq!(h.success_count(), 0);
        assert_eq!(h.failure_count(), 0);
    }

    #[test]
    fn test_history_record_success() {
        let mut h = TaskHistory::new();
        h.record_success(1, "task1", 1000, 50);
        assert_eq!(h.count(), 1);
        assert_eq!(h.success_count(), 1);
        assert_eq!(h.failure_count(), 0);
    }

    #[test]
    fn test_history_record_failure() {
        let mut h = TaskHistory::new();
        h.record_failure(1, "task1", 2000, 100, "timeout");
        assert_eq!(h.count(), 1);
        assert_eq!(h.success_count(), 0);
        assert_eq!(h.failure_count(), 1);
        let entry = &h.entries()[0];
        assert_eq!(entry.error.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_history_entries_for_task() {
        let mut h = TaskHistory::new();
        h.record_success(1, "task1", 1000, 50);
        h.record_success(2, "task2", 2000, 60);
        h.record_success(1, "task1", 3000, 70);

        let t1 = h.entries_for_task(1);
        assert_eq!(t1.len(), 2);
        let t2 = h.entries_for_task(2);
        assert_eq!(t2.len(), 1);
    }

    #[test]
    fn test_history_recent_ordering() {
        let mut h = TaskHistory::new();
        h.record_success(1, "a", 100, 10);
        h.record_success(2, "b", 200, 20);
        h.record_success(3, "c", 300, 30);

        let recent = h.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].timestamp, 300);
        assert_eq!(recent[1].timestamp, 200);
    }

    #[test]
    fn test_history_max_entries_trim() {
        let mut h = TaskHistory::new().with_max_entries(3);
        for i in 0..5 {
            h.record_success(i, &format!("t{i}"), i * 100, 10);
        }
        assert_eq!(h.count(), 3);
        // Oldest entries should have been trimmed.
        assert_eq!(h.entries()[0].task_id, 2);
    }

    #[test]
    fn test_history_clear() {
        let mut h = TaskHistory::new();
        h.record_success(1, "t", 100, 10);
        h.clear();
        assert_eq!(h.count(), 0);
    }

    // -- TaskScheduler CRUD tests -------------------------------------------

    #[test]
    fn test_scheduler_add_task() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("backup", "/bin/backup", ScheduleFrequency::Daily, 1000);
        assert_eq!(id, 1);
        assert_eq!(s.task_count(), 1);
        assert!(s.get_task(id).is_some());
    }

    #[test]
    fn test_scheduler_add_multiple_tasks() {
        let mut s = TaskScheduler::new();
        let id1 = s.add_task("a", "cmd_a", ScheduleFrequency::Daily, 100);
        let id2 = s.add_task("b", "cmd_b", ScheduleFrequency::Hourly, 100);
        assert_ne!(id1, id2);
        assert_eq!(s.task_count(), 2);
    }

    #[test]
    fn test_scheduler_remove_task() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Once, 0);
        assert!(s.remove_task(id));
        assert_eq!(s.task_count(), 0);
        assert!(!s.remove_task(id)); // Already removed.
    }

    #[test]
    fn test_scheduler_enable_disable() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 0);
        assert!(s.get_task(id).is_some_and(|t| t.enabled));

        s.disable_task(id);
        assert!(s.get_task(id).is_some_and(|t| !t.enabled));

        s.enable_task(id);
        assert!(s.get_task(id).is_some_and(|t| t.enabled));
    }

    #[test]
    fn test_scheduler_list_tasks_sorted_by_next_run() {
        let mut s = TaskScheduler::new();
        // EveryNMinutes(60) from now=1000 -> next_run = 1000 + 3600 = 4600
        s.add_task("later", "cmd", ScheduleFrequency::EveryNMinutes(60), 1000);
        // EveryNMinutes(5) from now=1000 -> next_run = 1000 + 300 = 1300
        s.add_task("sooner", "cmd", ScheduleFrequency::EveryNMinutes(5), 1000);

        let tasks = s.list_tasks();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "sooner");
        assert_eq!(tasks[1].name, "later");
    }

    #[test]
    fn test_scheduler_check_due() {
        let mut s = TaskScheduler::new();
        // This task is due at next_run = 100 + 300 = 400
        s.add_task("t1", "cmd1", ScheduleFrequency::EveryNMinutes(5), 100);
        // This task is due at next_run = 100 + 86400 = 86500
        s.add_task("t2", "cmd2", ScheduleFrequency::Daily, 100);

        let due = s.check_due(500);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "t1");

        let due_all = s.check_due(100_000);
        assert_eq!(due_all.len(), 2);
    }

    #[test]
    fn test_scheduler_check_due_excludes_disabled() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::EveryNMinutes(1), 100);
        s.disable_task(id);

        let due = s.check_due(10_000);
        assert!(due.is_empty());
    }

    #[test]
    fn test_scheduler_mark_completed() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 1000);
        s.mark_completed(id, 2000, 50);

        let task = s.get_task(id).expect("task should exist");
        assert_eq!(task.last_run_timestamp, 2000);
        assert!(task.last_succeeded());
        assert!(task.next_run_timestamp > 2000);
        assert_eq!(s.history.count(), 1);
    }

    #[test]
    fn test_scheduler_mark_completed_once_disables() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Once, 1000);
        s.mark_completed(id, 2000, 50);

        let task = s.get_task(id).expect("task should exist");
        assert!(!task.enabled);
        assert_eq!(task.next_run_timestamp, u64::MAX);
    }

    #[test]
    fn test_scheduler_mark_failed_with_retry() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 1000);
        s.set_retry_policy(id, true, 3);

        s.mark_failed(id, "connection refused", 2000, 100);

        let task = s.get_task(id).expect("task should exist");
        assert!(task.last_failed());
        assert_eq!(task.current_retries, 1);
        // Should be scheduled for retry at now + 60.
        assert_eq!(task.next_run_timestamp, 2060);
    }

    #[test]
    fn test_scheduler_mark_failed_exhausted_retries() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 1000);
        s.set_retry_policy(id, true, 1);

        s.mark_failed(id, "err", 2000, 10);
        assert_eq!(s.get_task(id).map(|t| t.current_retries), Some(1));

        // Second failure exhausts retries.
        s.mark_failed(id, "err2", 2060, 10);
        let task = s.get_task(id).expect("task should exist");
        assert_eq!(task.current_retries, 0);
        // Should schedule for next normal run, not retry.
        assert!(task.next_run_timestamp > 2060 + 60);
    }

    #[test]
    fn test_scheduler_update_task() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("old_name", "old_cmd", ScheduleFrequency::Daily, 1000);
        let updated = s.update_task(id, "new_name", "new_cmd", ScheduleFrequency::Hourly, 2000);
        assert!(updated);

        let task = s.get_task(id).expect("task should exist");
        assert_eq!(task.name, "new_name");
        assert_eq!(task.command, "new_cmd");
        assert_eq!(task.frequency, ScheduleFrequency::Hourly);
    }

    #[test]
    fn test_scheduler_update_nonexistent_returns_false() {
        let mut s = TaskScheduler::new();
        assert!(!s.update_task(999, "n", "c", ScheduleFrequency::Once, 0));
    }

    // -- calculate_next_run tests -------------------------------------------

    #[test]
    fn test_calculate_next_run_once() {
        let next = calculate_next_run(&ScheduleFrequency::Once, 1000);
        assert_eq!(next, 1000);
    }

    #[test]
    fn test_calculate_next_run_daily() {
        let next = calculate_next_run(&ScheduleFrequency::Daily, 1000);
        assert_eq!(next, 1000 + 86400);
    }

    #[test]
    fn test_calculate_next_run_hourly() {
        let next = calculate_next_run(&ScheduleFrequency::Hourly, 1000);
        assert_eq!(next, 1000 + 3600);
    }

    #[test]
    fn test_calculate_next_run_every_n_minutes() {
        let next = calculate_next_run(&ScheduleFrequency::EveryNMinutes(15), 1000);
        assert_eq!(next, 1000 + 15 * 60);
    }

    #[test]
    fn test_calculate_next_run_weekly() {
        let next = calculate_next_run(&ScheduleFrequency::Weekly(DayOfWeek::Monday), 1000);
        assert_eq!(next, 1000 + 7 * 86400);
    }

    #[test]
    fn test_calculate_next_run_monthly() {
        let next = calculate_next_run(&ScheduleFrequency::Monthly(15), 1000);
        assert_eq!(next, 1000 + 30 * 86400);
    }

    #[test]
    fn test_calculate_next_run_cron_every_minute() {
        let expr = CronExpr::parse("* * * * *").expect("should parse");
        let now = 1_700_000_000u64; // Some reasonable timestamp.
        let next = calculate_next_run(&ScheduleFrequency::Cron(expr), now);
        // Should be next minute boundary.
        assert!(next > now);
        assert!(next <= now + 120);
    }

    // -- Config serialization tests -----------------------------------------

    #[test]
    fn test_config_serialize_deserialize_roundtrip() {
        let mut s = TaskScheduler::new();
        s.add_task("backup", "/bin/backup", ScheduleFrequency::Daily, 1000);
        s.add_task("cleanup", "/bin/clean", ScheduleFrequency::Weekly(DayOfWeek::Monday), 2000);
        s.add_task("report", "/bin/report", ScheduleFrequency::EveryNMinutes(30), 3000);

        let text = TaskSchedulerConfig::serialize(&s);
        let restored = TaskSchedulerConfig::deserialize(&text).expect("should deserialize");

        assert_eq!(restored.task_count(), 3);
        assert!(restored.get_task(1).is_some_and(|t| t.name == "backup"));
        assert!(restored.get_task(2).is_some_and(|t| t.name == "cleanup"));
    }

    #[test]
    fn test_config_serialize_cron_task() {
        let mut s = TaskScheduler::new();
        s.add_task(
            "cron_task",
            "/bin/job",
            ScheduleFrequency::Cron(CronExpr::parse("*/15 * * * *").expect("parse")),
            5000,
        );

        let text = TaskSchedulerConfig::serialize(&s);
        let restored = TaskSchedulerConfig::deserialize(&text).expect("should deserialize");
        let task = restored.get_task(1).expect("should exist");
        assert!(matches!(task.frequency, ScheduleFrequency::Cron(_)));
    }

    #[test]
    fn test_config_deserialize_empty() {
        let s = TaskSchedulerConfig::deserialize("").expect("should handle empty");
        assert_eq!(s.task_count(), 0);
    }

    #[test]
    fn test_config_deserialize_comments_and_blanks() {
        let text = "# comment\n\n# another comment\nVERSION|1\n";
        let s = TaskSchedulerConfig::deserialize(text).expect("should handle");
        assert_eq!(s.task_count(), 0);
    }

    // -- Frequency serialization tests --------------------------------------

    #[test]
    fn test_serialize_frequency_once() {
        assert_eq!(serialize_frequency(&ScheduleFrequency::Once), "once");
    }

    #[test]
    fn test_serialize_frequency_daily() {
        assert_eq!(serialize_frequency(&ScheduleFrequency::Daily), "daily");
    }

    #[test]
    fn test_serialize_frequency_hourly() {
        assert_eq!(serialize_frequency(&ScheduleFrequency::Hourly), "hourly");
    }

    #[test]
    fn test_deserialize_frequency_roundtrip() {
        let freqs = vec![
            ScheduleFrequency::Once,
            ScheduleFrequency::Daily,
            ScheduleFrequency::Hourly,
            ScheduleFrequency::Weekly(DayOfWeek::Wednesday),
            ScheduleFrequency::Monthly(15),
            ScheduleFrequency::EveryNMinutes(45),
        ];
        for freq in freqs {
            let s = serialize_frequency(&freq);
            let restored = deserialize_frequency(&s).expect("should roundtrip");
            assert_eq!(restored, freq);
        }
    }

    // -- decompose_timestamp / days_to_ymd tests ----------------------------

    #[test]
    fn test_decompose_epoch_zero() {
        let dt = decompose_timestamp(0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.hour, 0);
        // 1970-01-01 is Thursday = weekday 4.
        assert_eq!(dt.weekday, 4);
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2000-01-01 is day 10957 since epoch.
        let (y, m, d) = days_to_ymd(10957);
        assert_eq!((y, m, d), (2000, 1, 1));
    }

    // -- SchedulerUI tests --------------------------------------------------

    #[test]
    fn test_ui_initial_state() {
        let ui = SchedulerUI::new();
        assert_eq!(ui.tab, UiTab::Tasks);
        assert_eq!(ui.dialog, UiDialog::None);
        assert!(ui.selected_task_id.is_none());
    }

    #[test]
    fn test_ui_tab_switching() {
        let mut ui = SchedulerUI::new();
        ui.switch_to_history();
        assert_eq!(ui.tab, UiTab::History);
        ui.switch_to_tasks();
        assert_eq!(ui.tab, UiTab::Tasks);
    }

    #[test]
    fn test_ui_add_task_flow() {
        let mut ui = SchedulerUI::new();
        ui.open_add_dialog();
        assert_eq!(ui.dialog, UiDialog::AddTask);

        ui.form.name = String::from("test_task");
        ui.form.command = String::from("/bin/test");
        ui.form.frequency_index = 1; // Daily

        let id = ui.commit_add_task(1000);
        assert!(id.is_some());
        assert_eq!(ui.scheduler.task_count(), 1);
        assert_eq!(ui.dialog, UiDialog::None);
    }

    #[test]
    fn test_ui_add_task_requires_name() {
        let mut ui = SchedulerUI::new();
        ui.open_add_dialog();
        ui.form.command = String::from("/bin/test");
        // name is empty
        let id = ui.commit_add_task(1000);
        assert!(id.is_none());
    }

    #[test]
    fn test_ui_edit_task_flow() {
        let mut ui = SchedulerUI::new();
        let id = ui.scheduler.add_task("original", "cmd", ScheduleFrequency::Daily, 1000);

        ui.open_edit_dialog(id);
        assert!(matches!(ui.dialog, UiDialog::EditTask(_)));

        ui.form.name = String::from("updated");
        let ok = ui.commit_edit_task(id, 2000);
        assert!(ok);
        assert_eq!(ui.scheduler.get_task(id).map(|t| t.name.as_str()), Some("updated"));
    }

    #[test]
    fn test_ui_delete_task_flow() {
        let mut ui = SchedulerUI::new();
        let id = ui.scheduler.add_task("to_delete", "cmd", ScheduleFrequency::Once, 0);
        ui.select_task(id);

        ui.open_delete_dialog(id);
        assert!(matches!(ui.dialog, UiDialog::ConfirmDelete(_)));

        let removed = ui.confirm_delete_task(id);
        assert!(removed);
        assert_eq!(ui.scheduler.task_count(), 0);
        assert!(ui.selected_task_id.is_none());
    }

    #[test]
    fn test_ui_toggle_selected_task() {
        let mut ui = SchedulerUI::new();
        let id = ui.scheduler.add_task("t", "cmd", ScheduleFrequency::Daily, 0);
        ui.select_task(id);
        assert!(ui.scheduler.get_task(id).is_some_and(|t| t.enabled));

        ui.toggle_selected_task();
        assert!(ui.scheduler.get_task(id).is_some_and(|t| !t.enabled));

        ui.toggle_selected_task();
        assert!(ui.scheduler.get_task(id).is_some_and(|t| t.enabled));
    }

    // -- TaskFormState tests ------------------------------------------------

    #[test]
    fn test_form_state_from_task() {
        let task = ScheduledTask::new(
            1,
            "weekly_backup",
            "/bin/backup",
            ScheduleFrequency::Weekly(DayOfWeek::Friday),
            1000,
        );
        let form = TaskFormState::from_task(&task);
        assert_eq!(form.name, "weekly_backup");
        assert_eq!(form.frequency_index, 2);
        assert_eq!(form.weekly_day, 5); // Friday
    }

    #[test]
    fn test_form_state_build_frequency() {
        let mut form = TaskFormState::new();
        form.frequency_index = 1;
        assert_eq!(form.build_frequency(), Some(ScheduleFrequency::Daily));

        form.frequency_index = 4;
        assert_eq!(form.build_frequency(), Some(ScheduleFrequency::Hourly));

        form.frequency_index = 5;
        form.interval_minutes = 10;
        assert_eq!(
            form.build_frequency(),
            Some(ScheduleFrequency::EveryNMinutes(10))
        );
    }

    #[test]
    fn test_form_state_build_cron_frequency() {
        let mut form = TaskFormState::new();
        form.frequency_index = 6;
        form.cron_expr = String::from("*/5 * * * *");
        let freq = form.build_frequency();
        assert!(matches!(freq, Some(ScheduleFrequency::Cron(_))));
    }

    #[test]
    fn test_form_state_invalid_cron_returns_none() {
        let mut form = TaskFormState::new();
        form.frequency_index = 6;
        form.cron_expr = String::from("invalid");
        assert!(form.build_frequency().is_none());
    }

    // -- Render tests -------------------------------------------------------

    #[test]
    fn test_render_tasks_tab() {
        let ui = SchedulerUI::new();
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_history_tab() {
        let mut ui = SchedulerUI::new();
        ui.switch_to_history();
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_tasks() {
        let mut ui = SchedulerUI::new();
        ui.scheduler.add_task("test", "cmd", ScheduleFrequency::Daily, 1000);
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_add_dialog() {
        let mut ui = SchedulerUI::new();
        ui.open_add_dialog();
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_confirm_delete_dialog() {
        let mut ui = SchedulerUI::new();
        let id = ui.scheduler.add_task("t", "c", ScheduleFrequency::Once, 0);
        ui.open_delete_dialog(id);
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_status_message() {
        let mut ui = SchedulerUI::new();
        ui.status_message = Some(String::from("Task added"));
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    // -- Utility function tests ---------------------------------------------

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "Never");
    }

    #[test]
    fn test_format_timestamp_max() {
        assert_eq!(format_timestamp(u64::MAX), "--");
    }

    #[test]
    fn test_format_timestamp_known() {
        // 1700000000 = 2023-11-14 22:13 UTC
        let s = format_timestamp(1_700_000_000);
        assert!(s.starts_with("2023-"));
    }

    #[test]
    fn test_format_duration_ms_millis() {
        assert_eq!(format_duration_ms(500), "500ms");
    }

    #[test]
    fn test_format_duration_ms_seconds() {
        assert_eq!(format_duration_ms(2500), "2.5s");
    }

    #[test]
    fn test_format_duration_ms_minutes() {
        assert_eq!(format_duration_ms(125000), "2m 5s");
    }
}
