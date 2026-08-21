//! Calendar & Scheduling Widget — system tray popup.
//!
//! Provides a calendar popup for the taskbar clock area, a digital clock
//! display, basic event/reminder management, and rendering to
//! [`RenderCommand`]s that any compositor backend can consume.
//!
//! # Components
//!
//! - [`CalendarView`] — month grid navigation (prev/next, today jump, mini
//!   year view, optional week numbers, configurable first-day-of-week).
//! - [`CalendarEvent`] / [`EventStore`] — in-memory event CRUD with
//!   recurring-event expansion, date-range queries, and text import/export.
//! - [`ReminderManager`] — per-event reminders with snooze & dismiss.
//! - [`ClockDisplay`] — digital clock for the taskbar (12/24h, multi-timezone).
//! - [`CalendarConfig`] — user preferences.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut cal = CalendarView::new(CalendarConfig::default());
//! let mut store = EventStore::new();
//! let mut clock = ClockDisplay::new();
//!
//! // Taskbar renders clock:
//! let clock_cmds = clock.render(x, y);
//!
//! // Click on clock opens the calendar popup:
//! cal.set_visible(true);
//!
//! // Each frame, if visible:
//! let cmds = cal.render(x, y, &store);
//! ```

use guitk::color::Color;
// One calendar for the whole GUI tree. This module used to carry its own
// leap-year rule, month lengths, Sakamoto day-of-week, ISO week number and
// both directions of the timestamp conversion; all of it is now `guitk::date`,
// which derives them from the same `tzrules` era arithmetic the libc's
// `localtime` and the shell's `%(…)T` render through.
use guitk::date::{self, Date, Weekday};
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
// The same zone engine the libc's `localtime` and osh's `printf '%(…)T'` use.
use tzrules::Tz;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT: Color = Color::from_hex(0xA6ADC8);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
}

// ============================================================================
// Layout constants
// ============================================================================

/// Width of the calendar popup.
const POPUP_WIDTH: f32 = 320.0;

/// Cell size (width = height) for each day in the grid.
const CELL_SIZE: f32 = 40.0;

/// Width of the optional week-number column.
const WEEK_NUM_WIDTH: f32 = 28.0;

/// Height of the navigation header (month/year + arrows).
const NAV_HEIGHT: f32 = 44.0;

/// Height of the day-of-week header row (S M T W T F S).
const DOW_HEADER_HEIGHT: f32 = 28.0;

/// Padding inside the popup.
const PADDING: f32 = 12.0;

/// Corner radius for the popup card.
const CARD_RADIUS: f32 = 10.0;

/// Radius for the "today" highlight circle.
const TODAY_RADIUS: f32 = 16.0;

/// Event dot radius.
const DOT_RADIUS: f32 = 3.0;

/// Height of a single event row in the detail popup.
const EVENT_ROW_HEIGHT: f32 = 28.0;

/// Maximum events shown in the detail popup before scrolling.
const MAX_VISIBLE_EVENTS: usize = 6;

/// Mini year-view cell size.
const MINI_CELL: f32 = 12.0;

/// Mini year-view month label height.
const MINI_MONTH_LABEL_HEIGHT: f32 = 18.0;

/// Seconds per minute.
const SECS_PER_MIN: u64 = 60;

/// Seconds per hour.
const SECS_PER_HOUR: u64 = 3600;

/// Seconds per day.
const SECS_PER_DAY: u64 = 86400;

/// The same, signed, for the arithmetic that has to survive a pre-epoch
/// instant without wrapping.
const SECS_PER_DAY_I: i64 = 86_400;

// ============================================================================
// Configuration
// ============================================================================

/// Which day starts the week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FirstDayOfWeek {
    #[default]
    Sunday,
    Monday,
}

/// Calendar user preferences.
#[derive(Clone, Debug)]
pub struct CalendarConfig {
    pub first_day_of_week: FirstDayOfWeek,
    pub show_week_numbers: bool,
    pub default_event_duration_mins: u32,
    pub default_reminder_mins: u32,
}

impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            first_day_of_week: FirstDayOfWeek::Sunday,
            show_week_numbers: false,
            default_event_duration_mins: 60,
            default_reminder_mins: 15,
        }
    }
}

// ============================================================================
// Date arithmetic helpers
// ============================================================================

/// Number of days in the given month (1-indexed).
///
/// A thin alias for [`date::days_in_month`], kept because the argument order
/// here is `(year, month)` and the toolkit's is `(month, year)` — the shared
/// one follows the C convention `tzrules` was extracted from.
fn days_in_month(year: i32, month: u32) -> u32 {
    date::days_in_month(year, month)
}

/// Day-of-week for a given date. 0 = Sunday, 1 = Monday, ..., 6 = Saturday.
fn day_of_week(year: i32, month: u32, day: u32) -> u32 {
    u32::try_from(Date::from_ymd(year, month, day).weekday().index()).unwrap_or(0)
}

/// ISO 8601 week number: `(iso_year, week)`, week 1 being the one containing
/// the year's first Thursday.
fn iso_week_number(year: i32, month: u32, day: u32) -> (i32, u32) {
    Date::from_ymd(year, month, day).iso_week()
}

/// Split a Unix timestamp into the day it falls on and the time within it.
///
/// The time of day is a plain remainder — `rem_euclid`, so an instant before
/// 1970 lands at a positive offset into the day that contains it rather than
/// a negative one into the day after. `Date` handles the day half.
fn timestamp_parts(ts: u64) -> (Date, u32, u32, u32) {
    let secs = i64::try_from(ts).unwrap_or(i64::MAX);
    let within = secs.rem_euclid(SECS_PER_DAY_I);
    let hour = u32::try_from(within.div_euclid(3600)).unwrap_or(0);
    let min = u32::try_from(within.div_euclid(60).rem_euclid(60)).unwrap_or(0);
    let sec = u32::try_from(within.rem_euclid(60)).unwrap_or(0);
    (Date::from_unix_utc(secs), hour, min, sec)
}

/// Decompose a Unix timestamp into `(year, month, day, hour, min, sec)`.
fn timestamp_to_date(ts: u64) -> (i32, u32, u32, u32, u32, u32) {
    let (date, hour, min, sec) = timestamp_parts(ts);
    let (year, month, day) = date.ymd();
    (year, month, day, hour, min, sec)
}

/// The Unix timestamp of `date` at `hour:min:sec`, or `None` before the epoch.
///
/// The `None` is the store's own limit, not the calendar's — event timestamps
/// are `u64` — so it is stated here as one comparison against the epoch rather
/// than as a `year < 1970` test standing in front of a loop that counts up
/// from 1970 one year at a time.
fn timestamp_at(date: Date, hour: u32, min: u32, sec: u32) -> Option<u64> {
    let secs = date
        .unix_secs_utc()
        .checked_add(i64::from(hour).checked_mul(3600)?)?
        .checked_add(i64::from(min).checked_mul(60)?)?
        .checked_add(i64::from(sec))?;
    u64::try_from(secs).ok()
}

/// Convert (year, month, day, hour, min, sec) to a Unix timestamp.
/// Returns `None` for dates before 1970-01-01.
fn date_to_timestamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
) -> Option<u64> {
    timestamp_at(Date::from_ymd(year, month, day), hour, min, sec)
}

/// Name of a month (1-indexed).
fn month_name(month: u32) -> &'static str {
    date::month_name(month)
}

/// Short (3-char) name of a month.
fn month_name_short(month: u32) -> &'static str {
    date::month_short_name(month)
}

/// Day-of-week abbreviations starting from the given first day.
fn dow_headers(first: FirstDayOfWeek) -> [&'static str; 7] {
    match first {
        FirstDayOfWeek::Sunday => ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"],
        FirstDayOfWeek::Monday => ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"],
    }
}

/// Day-of-week name for display, 0 = Sunday.
fn day_of_week_name(dow: u32) -> &'static str {
    Weekday::from_index(i32::try_from(dow).unwrap_or(0)).name()
}

// ============================================================================
// CalendarEvent and recurrence
// ============================================================================

/// Recurrence pattern for a calendar event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recurrence {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// A single calendar event.
#[derive(Clone, Debug)]
pub struct CalendarEvent {
    pub id: u64,
    pub title: String,
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    pub all_day: bool,
    pub repeat: Option<Recurrence>,
    pub color: Color,
    pub description: String,
}

impl CalendarEvent {
    /// Duration of this event in seconds.
    pub fn duration_secs(&self) -> u64 {
        self.end_timestamp.saturating_sub(self.start_timestamp)
    }
}

// ============================================================================
// EventStore
// ============================================================================

/// In-memory storage for calendar events.
pub struct EventStore {
    events: Vec<CalendarEvent>,
    ids: IdSeq,
}

impl EventStore {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            ids: IdSeq::new(),
        }
    }

    /// Add a new event, assigning it a unique ID. Returns the assigned ID,
    /// or `None` if the ID space is exhausted.
    ///
    /// Refusing is the only answer that keeps IDs unique. Wrapping or
    /// saturating the counter would hand out an ID some existing event
    /// already holds, and every lookup in this module -- `remove_event`,
    /// `update_event`, `ReminderManager` -- is by ID, so the two events
    /// would thereafter be one event to every caller.
    pub fn add_event(&mut self, mut event: CalendarEvent) -> Option<u64> {
        let id = self.ids.issue()?;
        event.id = id;
        self.events.push(event);
        Some(id)
    }

    /// Remove an event by ID. Returns `true` if the event was found and removed.
    pub fn remove_event(&mut self, id: u64) -> bool {
        let before = self.events.len();
        self.events.retain(|e| e.id != id);
        self.events.len() < before
    }

    /// Update an event by ID. The closure receives a mutable reference.
    /// Returns `true` if the event was found and updated.
    pub fn update_event<F: FnOnce(&mut CalendarEvent)>(&mut self, id: u64, f: F) -> bool {
        if let Some(e) = self.events.iter_mut().find(|e| e.id == id) {
            f(e);
            true
        } else {
            false
        }
    }

    /// Get a reference to an event by ID.
    pub fn get_event(&self, id: u64) -> Option<&CalendarEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    /// All events (including base recurring events) stored.
    pub fn all_events(&self) -> &[CalendarEvent] {
        &self.events
    }

    /// Number of stored events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get all event occurrences for a specific date, expanding recurring events.
    pub fn events_for_date(&self, year: i32, month: u32, day: u32) -> Vec<CalendarEvent> {
        let day_start = match date_to_timestamp(year, month, day, 0, 0, 0) {
            Some(ts) => ts,
            None => return Vec::new(),
        };
        let day_end = day_start.saturating_add(SECS_PER_DAY);
        self.events_for_range(day_start, day_end)
    }

    /// Get all event occurrences in a timestamp range, expanding recurring events.
    pub fn events_for_range(&self, range_start: u64, range_end: u64) -> Vec<CalendarEvent> {
        let mut result = Vec::new();

        for event in &self.events {
            if event.repeat.is_none() {
                // Non-recurring: simple overlap check.
                if event.start_timestamp < range_end && event.end_timestamp > range_start {
                    result.push(event.clone());
                }
            } else {
                // Recurring: expand occurrences within the range.
                let occurrences = expand_recurrence(event, range_start, range_end);
                result.extend(occurrences);
            }
        }

        result.sort_by_key(|e| e.start_timestamp);
        result
    }

    /// Search events by title or description (case-insensitive substring match).
    pub fn search(&self, query: &str) -> Vec<&CalendarEvent> {
        let q = query.to_lowercase();
        self.events
            .iter()
            .filter(|e| {
                e.title.to_lowercase().contains(&q) || e.description.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Export all events to a simple text format.
    ///
    /// Format per event (lines separated by newlines, events by blank lines):
    /// ```text
    /// EVENT
    /// title: <title>
    /// start: <unix_timestamp>
    /// end: <unix_timestamp>
    /// all_day: <true|false>
    /// repeat: <none|daily|weekly|monthly|yearly>
    /// color: <hex>
    /// description: <description>
    /// ```
    pub fn export_text(&self) -> String {
        let mut out = String::new();
        for (i, event) in self.events.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str("EVENT\n");
            out.push_str(&format!("title: {}\n", event.title));
            out.push_str(&format!("start: {}\n", event.start_timestamp));
            out.push_str(&format!("end: {}\n", event.end_timestamp));
            out.push_str(&format!("all_day: {}\n", event.all_day));
            let repeat_str = match event.repeat {
                None => "none",
                Some(Recurrence::Daily) => "daily",
                Some(Recurrence::Weekly) => "weekly",
                Some(Recurrence::Monthly) => "monthly",
                Some(Recurrence::Yearly) => "yearly",
            };
            out.push_str(&format!("repeat: {repeat_str}\n"));
            out.push_str(&format!(
                "color: {:02X}{:02X}{:02X}\n",
                event.color.r, event.color.g, event.color.b
            ));
            out.push_str(&format!("description: {}\n", event.description));
        }
        out
    }

    /// Import events from the text format produced by [`export_text`].
    /// Returns the number of events successfully imported.
    pub fn import_text(&mut self, text: &str) -> usize {
        // Counting what the store actually gained, rather than incrementing
        // a tally beside each `add_event` call. The two `count += 1` this
        // replaces sat at the ends of two different code paths — the
        // mid-input flush and the final one — and had to be kept in step
        // with the `add_event` calls they were meant to be counting.
        let before = self.events.len();
        let mut title = String::new();
        let mut start: u64 = 0;
        let mut end: u64 = 0;
        let mut all_day = false;
        let mut repeat: Option<Recurrence> = None;
        let mut color = theme::BLUE;
        let mut description = String::new();
        let mut in_event = false;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed == "EVENT" {
                // If we were already in an event, save the previous one.
                if in_event {
                    // A refused event is one the store did not gain, and the
                    // returned count is derived from the store's length, so
                    // ignoring the result here still reports it correctly.
                    let _ = self.add_event(CalendarEvent {
                        id: 0,
                        title: core::mem::take(&mut title),
                        start_timestamp: start,
                        end_timestamp: end,
                        all_day,
                        repeat,
                        color,
                        description: core::mem::take(&mut description),
                    });
                }
                // Reset for new event.
                title = String::new();
                start = 0;
                end = 0;
                all_day = false;
                repeat = None;
                color = theme::BLUE;
                description = String::new();
                in_event = true;
                continue;
            }

            if !in_event {
                continue;
            }

            if let Some(val) = trimmed.strip_prefix("title: ") {
                title = val.to_string();
            } else if let Some(val) = trimmed.strip_prefix("start: ") {
                start = val.parse().unwrap_or(0);
            } else if let Some(val) = trimmed.strip_prefix("end: ") {
                end = val.parse().unwrap_or(0);
            } else if let Some(val) = trimmed.strip_prefix("all_day: ") {
                all_day = val == "true";
            } else if let Some(val) = trimmed.strip_prefix("repeat: ") {
                repeat = match val {
                    "daily" => Some(Recurrence::Daily),
                    "weekly" => Some(Recurrence::Weekly),
                    "monthly" => Some(Recurrence::Monthly),
                    "yearly" => Some(Recurrence::Yearly),
                    _ => None,
                };
            } else if let Some(val) = trimmed.strip_prefix("color: ") {
                color = parse_hex_color(val).unwrap_or(theme::BLUE);
            } else if let Some(val) = trimmed.strip_prefix("description: ") {
                description = val.to_string();
            }
        }

        // Don't forget the last event.
        if in_event {
            // As above: not gaining the event is what the count measures.
            let _ = self.add_event(CalendarEvent {
                id: 0,
                title,
                start_timestamp: start,
                end_timestamp: end,
                all_day,
                repeat,
                color,
                description,
            });
        }

        self.events.len().saturating_sub(before)
    }
}

impl Default for EventStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a 6-digit hex color string (e.g., "89B4FA") into a Color.
fn parse_hex_color(s: &str) -> Option<Color> {
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(s.get(0..2)?, 16).ok()?;
    let g = u8::from_str_radix(s.get(2..4)?, 16).ok()?;
    let b = u8::from_str_radix(s.get(4..6)?, 16).ok()?;
    Some(Color::rgb(r, g, b))
}

/// Expand recurring event occurrences within a time range.
///
/// Generates synthetic `CalendarEvent` copies with adjusted timestamps
/// for each occurrence that overlaps `[range_start, range_end)`.
fn expand_recurrence(
    event: &CalendarEvent,
    range_start: u64,
    range_end: u64,
) -> Vec<CalendarEvent> {
    let recurrence = match event.repeat {
        Some(r) => r,
        None => return Vec::new(),
    };

    let duration = event.duration_secs();
    let (orig_year, orig_month, orig_day, orig_hour, orig_min, orig_sec) =
        timestamp_to_date(event.start_timestamp);

    let mut results = Vec::new();

    // Walk forward from the event's original date, generating occurrences.
    // Limit to a reasonable window to avoid infinite loops.
    let max_iterations = 1000;
    // Every occurrence is a fixed step from the *original* date, not from the
    // one before it. That distinction is the whole reason the old walk needed
    // an unclamped `day` carried alongside a `clamped_day`: stepping
    // 31 January by one month and keeping the result gives 28 February and
    // then 28 March, so "the 31st of every month" decays into "the 28th".
    // Measuring from the anchor each time, `add_months` clamps for display
    // without the series ever losing the day it was anchored on.
    let anchor = Date::from_ymd(orig_year, orig_month, orig_day);

    for step in 0..max_iterations {
        let occurrence = match recurrence {
            Recurrence::Daily => anchor.add_days(step),
            // `saturating_mul`: `step` is bounded by `max_iterations`, but the
            // bound lives in the loop header rather than in this expression,
            // which is exactly the arrangement that goes wrong when one of the
            // two is edited.
            Recurrence::Weekly => anchor.add_days(step.saturating_mul(7)),
            Recurrence::Monthly => anchor.add_months(step),
            Recurrence::Yearly => anchor.add_years(step),
        };
        let Some(occ_start) = timestamp_at(occurrence, orig_hour, orig_min, orig_sec) else {
            break;
        };

        // Stop if we've passed the range.
        if occ_start >= range_end {
            break;
        }

        let occ_end = occ_start.saturating_add(duration);

        // Include if there is overlap.
        if occ_end > range_start {
            results.push(CalendarEvent {
                id: event.id,
                title: event.title.clone(),
                start_timestamp: occ_start,
                end_timestamp: occ_end,
                all_day: event.all_day,
                repeat: event.repeat,
                color: event.color,
                description: event.description.clone(),
            });
        }
    }

    results
}

// ============================================================================
// ReminderManager
// ============================================================================

/// Snooze duration options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnoozeDuration {
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
}

impl SnoozeDuration {
    /// Duration in seconds.
    pub fn secs(self) -> u64 {
        match self {
            Self::FiveMinutes => 5 * SECS_PER_MIN,
            Self::FifteenMinutes => 15 * SECS_PER_MIN,
            Self::ThirtyMinutes => 30 * SECS_PER_MIN,
            Self::OneHour => SECS_PER_HOUR,
        }
    }
}

/// A reminder attached to an event.
#[derive(Clone, Debug)]
pub struct Reminder {
    /// The event ID this reminder is for.
    pub event_id: u64,
    /// The event title (cached for display without needing the store).
    pub event_title: String,
    /// The timestamp when the reminder should fire (event_start - lead_minutes * 60).
    pub fire_at: u64,
    /// Whether this reminder has been dismissed.
    pub dismissed: bool,
}

/// Manages reminders for calendar events.
pub struct ReminderManager {
    reminders: Vec<Reminder>,
}

impl ReminderManager {
    pub fn new() -> Self {
        Self {
            reminders: Vec::new(),
        }
    }

    /// Set a reminder N minutes before an event.
    pub fn set_reminder(
        &mut self,
        event_id: u64,
        event_title: &str,
        event_start: u64,
        lead_minutes: u32,
    ) {
        let fire_at =
            event_start.saturating_sub((lead_minutes as u64).saturating_mul(SECS_PER_MIN));
        self.reminders.push(Reminder {
            event_id,
            event_title: event_title.to_string(),
            fire_at,
            dismissed: false,
        });
    }

    /// Check for reminders that are due at or before `now`.
    /// Returns references to non-dismissed reminders whose fire time has passed.
    pub fn due_reminders(&self, now: u64) -> Vec<&Reminder> {
        self.reminders
            .iter()
            .filter(|r| !r.dismissed && r.fire_at <= now)
            .collect()
    }

    /// Snooze a reminder by pushing its fire time forward.
    pub fn snooze(&mut self, event_id: u64, duration: SnoozeDuration) {
        for r in &mut self.reminders {
            if r.event_id == event_id && !r.dismissed {
                r.fire_at = r.fire_at.saturating_add(duration.secs());
                break;
            }
        }
    }

    /// Dismiss a reminder permanently.
    pub fn dismiss(&mut self, event_id: u64) {
        for r in &mut self.reminders {
            if r.event_id == event_id {
                r.dismissed = true;
                break;
            }
        }
    }

    /// Dismiss all reminders.
    pub fn dismiss_all(&mut self) {
        for r in &mut self.reminders {
            r.dismissed = true;
        }
    }

    /// Number of active (non-dismissed) reminders.
    pub fn active_count(&self) -> usize {
        self.reminders.iter().filter(|r| !r.dismissed).count()
    }

    /// All reminders (including dismissed).
    pub fn all_reminders(&self) -> &[Reminder] {
        &self.reminders
    }

    /// Remove all dismissed reminders from storage.
    pub fn prune_dismissed(&mut self) {
        self.reminders.retain(|r| !r.dismissed);
    }
}

impl Default for ReminderManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ClockDisplay
// ============================================================================

/// Timezone display entry.
#[derive(Clone, Debug)]
pub struct TimezoneEntry {
    /// Display label (e.g., "New York", "London").
    pub label: String,
    /// The zone's **rules**, not a fixed offset.
    ///
    /// This used to be a single `utc_offset_secs: i64`, which cannot be right:
    /// a zone that observes daylight saving has two offsets and a rule saying
    /// which is in force *at a given instant*.  Storing one number meant "New
    /// York" was `-5h` all year, so the clock read an hour early for the ~8
    /// months of EDT — and, worse, was silently wrong rather than visibly
    /// missing.  A [`Tz`] carries the transition rule, so
    /// [`ClockDisplay::format_time`] can ask what the offset is *now*.
    pub tz: Tz,
}

/// Digital clock for the taskbar.
pub struct ClockDisplay {
    /// Whether to use 24-hour format.
    pub use_24h: bool,
    /// Whether to show seconds.
    pub show_seconds: bool,
    /// Additional timezone displays (up to 3).
    pub extra_timezones: Vec<TimezoneEntry>,
}

/// The local seconds a UTC instant names in `tz`.
///
/// Split out so the time and date renderings can never disagree about which
/// day they are on — they must apply the *same* offset to the *same* instant,
/// and a clock that reads `00:30` under one rule and yesterday's date under
/// another is exactly the bug this shape prevents.
fn local_secs(utc_timestamp: u64, tz: &Tz) -> u64 {
    let utc = i64::try_from(utc_timestamp).unwrap_or(i64::MAX);
    let shifted = utc.saturating_add(i64::from(tz.lookup(utc).gmtoff));
    u64::try_from(shifted).unwrap_or(0)
}

impl ClockDisplay {
    pub fn new() -> Self {
        Self {
            use_24h: true,
            show_seconds: false,
            extra_timezones: Vec::new(),
        }
    }

    /// Add an additional timezone display (up to 3), named by a POSIX `TZ`
    /// string (`"EST5EDT,M3.2.0,M11.1.0"`, `"NPT-5:45"`, `"GMT0BST,M3.5.0/1,M10.5.0"`).
    ///
    /// Returns `false` — and adds nothing — if the list is already full or the
    /// string is not a POSIX `TZ` string.  A zoneinfo *name* (`America/New_York`)
    /// is deliberately **not** accepted: it needs a TZif database we do not ship
    /// (known-issues `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), and quietly
    /// falling back to UTC for it would put a wrong time on the taskbar under a
    /// label claiming otherwise.
    pub fn add_timezone(&mut self, label: &str, posix_tz: &str) -> bool {
        if self.extra_timezones.len() >= 3 {
            return false;
        }
        let Some(tz) = Tz::parse(posix_tz.as_bytes()) else {
            return false;
        };
        self.extra_timezones.push(TimezoneEntry {
            label: label.to_string(),
            tz,
        });
        true
    }

    /// Format a UTC timestamp as a wall clock reading in `tz`.
    pub fn format_time(&self, utc_timestamp: u64, tz: &Tz) -> String {
        let (_, _, _, hour, min, sec) = timestamp_to_date(local_secs(utc_timestamp, tz));

        if self.use_24h {
            if self.show_seconds {
                format!("{hour:02}:{min:02}:{sec:02}")
            } else {
                format!("{hour:02}:{min:02}")
            }
        } else {
            // The 12-hour clock is "the hour modulo 12, with 0 written as
            // 12" -- one formula, where this was a four-armed ladder whose
            // last arm subtracted. Three of those arms existed only to keep
            // that subtraction from being reached with `hour < 12`.
            let h12 = match hour % 12 {
                0 => 12,
                h => h,
            };
            let ampm = if hour < 12 { "AM" } else { "PM" };
            if self.show_seconds {
                format!("{h12}:{min:02}:{sec:02} {ampm}")
            } else {
                format!("{h12}:{min:02} {ampm}")
            }
        }
    }

    /// Format a date string: "DayOfWeek, Month DD, YYYY".
    pub fn format_date(&self, utc_timestamp: u64, tz: &Tz) -> String {
        let (year, month, day, _, _, _) = timestamp_to_date(local_secs(utc_timestamp, tz));
        let dow = day_of_week(year, month, day);
        let dow_name = day_of_week_name(dow);
        let month_str = month_name(month);
        format!("{dow_name}, {month_str} {day}, {year}")
    }

    /// Render the clock display for the taskbar.
    ///
    /// Returns render commands positioned at `(x, y)`.
    /// `utc_now` is the current UTC timestamp and `local` is the machine's
    /// zone — the same [`Tz`] the libc's `localtime` and the shell's
    /// `printf '%(…)T'` use, so the taskbar cannot disagree with `date`.
    pub fn render(&self, x: f32, y: f32, utc_now: u64, local: &Tz) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Main time.
        let time_str = self.format_time(utc_now, local);
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: time_str,
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Date below the time.
        let date_str = self.format_date(utc_now, local);
        cmds.push(RenderCommand::Text {
            x,
            y: y + 16.0,
            text: date_str,
            color: theme::SUBTEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Extra timezones.
        let mut tz_y = y + 34.0;
        for tz in &self.extra_timezones {
            let tz_time = self.format_time(utc_now, &tz.tz);
            let label = format!("{}: {}", tz.label, tz_time);
            cmds.push(RenderCommand::Text {
                x,
                y: tz_y,
                text: label,
                color: theme::SUBTEXT,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            tz_y += 14.0;
        }

        cmds
    }
}

impl Default for ClockDisplay {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CalendarView — the popup widget
// ============================================================================

/// Which view the calendar popup is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarViewMode {
    /// Standard month grid.
    Month,
    /// Mini 12-month year overview.
    Year,
}

/// A single cell in the generated month grid.
#[derive(Clone, Copy, Debug)]
pub struct GridCell {
    /// Day number (1-31).
    pub day: u32,
    /// Whether this cell belongs to the currently displayed month.
    pub current_month: bool,
    /// Year of this cell.
    pub year: i32,
    /// Month of this cell (1-12).
    pub month: u32,
}

/// The calendar popup widget.
pub struct CalendarView {
    /// Configuration.
    pub config: CalendarConfig,
    /// Currently displayed year.
    pub view_year: i32,
    /// Currently displayed month (1-12).
    pub view_month: u32,
    /// "Today" — year, month, day.
    pub today: (i32, u32, u32),
    /// Whether the popup is visible.
    pub visible: bool,
    /// Current view mode.
    pub mode: CalendarViewMode,
    /// Selected date (if any) for event detail popup.
    pub selected_date: Option<(i32, u32, u32)>,
}

impl CalendarView {
    pub fn new(config: CalendarConfig) -> Self {
        Self {
            config,
            view_year: 2026,
            view_month: 1,
            today: (2026, 1, 1),
            visible: false,
            mode: CalendarViewMode::Month,
            selected_date: None,
        }
    }

    /// Set today's date and initialize the view to show the current month.
    pub fn set_today(&mut self, year: i32, month: u32, day: u32) {
        self.today = (year, month, day);
        self.view_year = year;
        self.view_month = month;
    }

    /// Set today from a UTC timestamp and local offset.
    pub fn set_today_from_timestamp(&mut self, utc_now: u64, local_offset: i64) {
        let adjusted = (utc_now as i64).saturating_add(local_offset).max(0) as u64;
        let (y, m, d, _, _, _) = timestamp_to_date(adjusted);
        self.set_today(y, m, d);
    }

    /// Show or hide the popup.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if visible {
            // Reset to month view showing today's month.
            self.mode = CalendarViewMode::Month;
            self.view_year = self.today.0;
            self.view_month = self.today.1;
        }
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        self.set_visible(!self.visible);
    }

    /// Navigate to the previous month.
    ///
    /// The year rollover used to be an `if self.view_month == 1` standing
    /// above a `self.view_year -= 1` that was only in range because of it.
    /// Stepping a `Date` carries the rollover inside the step.
    pub fn prev_month(&mut self) {
        self.set_view_to(self.view_anchor().add_months(-1));
    }

    /// Navigate to the next month.
    pub fn next_month(&mut self) {
        self.set_view_to(self.view_anchor().add_months(1));
    }

    /// Jump to today's month.
    pub fn go_to_today(&mut self) {
        self.view_year = self.today.0;
        self.view_month = self.today.1;
        self.mode = CalendarViewMode::Month;
    }

    /// Switch to year view.
    pub fn show_year_view(&mut self) {
        self.mode = CalendarViewMode::Year;
    }

    /// Switch to month view.
    pub fn show_month_view(&mut self) {
        self.mode = CalendarViewMode::Month;
    }

    /// Navigate to previous year (year view).
    pub fn prev_year(&mut self) {
        self.set_view_to(self.view_anchor().add_years(-1));
    }

    /// Navigate to next year (year view).
    pub fn next_year(&mut self) {
        self.set_view_to(self.view_anchor().add_years(1));
    }

    // ========================================================================
    // Grid generation
    // ========================================================================

    /// Generate the 6x7 grid of day cells for the current view month.
    ///
    /// The grid always has 6 rows (42 cells). Cells outside the current
    /// month are filled with days from the previous/next month.
    pub fn generate_grid(&self) -> Vec<GridCell> {
        // One iterator replaces the three loops this used to be: a lead-in
        // computed as `prev_days - offset + 1 + i` (in range only while
        // `offset` was provably no larger than the previous month), the month
        // itself, and a spill-over `while cells.len() < 42`. `month_grid`
        // yields 42 consecutive dates and cannot do otherwise, so
        // `current_month` is a comparison rather than a flag three loops have
        // to agree about.
        self.view_anchor()
            .month_grid(self.week_start())
            .map(|date| {
                let (year, month, day) = date.ymd();
                GridCell {
                    day,
                    current_month: month == self.view_month && year == self.view_year,
                    year,
                    month,
                }
            })
            .collect()
    }

    /// The 1st of the month on display.
    fn view_anchor(&self) -> Date {
        Date::from_ymd(self.view_year, self.view_month, 1)
    }

    /// The weekday the user's week begins on.
    fn week_start(&self) -> Weekday {
        match self.config.first_day_of_week {
            FirstDayOfWeek::Sunday => Weekday::Sunday,
            FirstDayOfWeek::Monday => Weekday::Monday,
        }
    }

    /// Move the view to the month containing `date`.
    fn set_view_to(&mut self, date: Date) {
        let (year, month, _) = date.ymd();
        self.view_year = year;
        self.view_month = month;
    }

    /// The ISO week number of a row of the grid, taken from its first day.
    ///
    /// Takes the row itself rather than the whole grid and an index into it.
    /// The caller had the row already — it was iterating them — and passing
    /// the index instead meant recomputing `row * 7` here and checking the
    /// result against `get`, in a second place from the one that rendered
    /// the cells.
    fn week_number_for(week: &[GridCell]) -> u32 {
        week.first()
            .map_or(0, |cell| iso_week_number(cell.year, cell.month, cell.day).1)
    }

    // ========================================================================
    // Rendering — month view
    // ========================================================================

    /// Render the complete calendar popup at position `(x, y)`.
    ///
    /// `store` is used to show event dots on dates that have events.
    pub fn render(&self, x: f32, y: f32, store: &EventStore) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        match self.mode {
            CalendarViewMode::Month => self.render_month_view(x, y, store),
            CalendarViewMode::Year => self.render_year_view(x, y),
        }
    }

    fn render_month_view(&self, x: f32, y: f32, store: &EventStore) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let wn_extra = if self.config.show_week_numbers {
            WEEK_NUM_WIDTH
        } else {
            0.0
        };
        let total_width = POPUP_WIDTH + wn_extra;
        let grid_rows = 6;
        let total_height =
            PADDING * 2.0 + NAV_HEIGHT + DOW_HEADER_HEIGHT + (grid_rows as f32 * CELL_SIZE);

        // Popup background with shadow.
        cmds.push(RenderCommand::BoxShadow {
            x,
            y,
            width: total_width,
            height: total_height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: total_width,
            height: total_height,
            color: theme::BASE,
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width: total_width,
            height: total_height,
            color: theme::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });

        let content_x = x + PADDING;
        let mut cy = y + PADDING;

        // Navigation header.
        self.render_nav_header(&mut cmds, content_x + wn_extra, cy);
        cy += NAV_HEIGHT;

        // Day-of-week headers.
        self.render_dow_headers(&mut cmds, content_x + wn_extra, cy);
        cy += DOW_HEADER_HEIGHT;

        // Grid.
        // The grid is 42 cells meaning six weeks of seven. `chunks` states
        // that once, where `row * 7` and `row * 7 + col` stated it at each of
        // the two places that indexed back in — and each of those then had to
        // re-check the result it had just computed against `get`.
        let grid = self.generate_grid();
        for (row, week) in grid.chunks(7).enumerate() {
            // Week number column.
            if self.config.show_week_numbers {
                let wn = Self::week_number_for(week);
                cmds.push(RenderCommand::Text {
                    x: content_x,
                    y: cy + row as f32 * CELL_SIZE + 12.0,
                    text: format!("{wn}"),
                    color: theme::SURFACE2,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(WEEK_NUM_WIDTH),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            for (col, cell) in week.iter().enumerate() {
                let cx = content_x + wn_extra + col as f32 * CELL_SIZE;
                let cell_y = cy + row as f32 * CELL_SIZE;
                self.render_day_cell(&mut cmds, cx, cell_y, cell, store);
            }
        }

        // Event detail popup for selected date.
        if let Some((sy, sm, sd)) = self.selected_date {
            let events = store.events_for_date(sy, sm, sd);
            if !events.is_empty() {
                let detail_y = y + total_height + 4.0;
                self.render_event_detail(&mut cmds, x, detail_y, total_width, sy, sm, sd, &events);
            }
        }

        cmds
    }

    fn render_nav_header(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        let label = format!("{} {}", month_name(self.view_month), self.view_year);
        let grid_width = 7.0 * CELL_SIZE;

        // Left arrow.
        cmds.push(RenderCommand::Text {
            x,
            y: y + 10.0,
            text: "<".to_string(),
            color: theme::SUBTEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Right arrow.
        cmds.push(RenderCommand::Text {
            x: x + grid_width - 16.0,
            y: y + 10.0,
            text: ">".to_string(),
            color: theme::SUBTEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Centered month/year label. Month names are localised, so an
        // eight-pixels-per-byte guess put "Februar" and "Fevereiro" visibly
        // off-centre and pushed the longest ones under the > arrow.
        let label_x = text::center_x(&label, x + grid_width / 2.0, 15.0, FontWeightHint::Bold);
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: y + 10.0,
            text: label,
            color: theme::TEXT,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(grid_width - 40.0),
            overflow: TextOverflow::Ellipsis,
        });

        // "Today" button below the label.
        let is_viewing_today = self.view_year == self.today.0 && self.view_month == self.today.1;
        if !is_viewing_today {
            let today_label = "Today";
            let tx = text::center_x(
                today_label,
                x + grid_width / 2.0,
                11.0,
                FontWeightHint::Regular,
            );
            cmds.push(RenderCommand::Text {
                x: tx,
                y: y + 30.0,
                text: today_label.to_string(),
                color: theme::BLUE,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_dow_headers(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        let headers = dow_headers(self.config.first_day_of_week);
        for (i, hdr) in headers.iter().enumerate() {
            let hx = x + i as f32 * CELL_SIZE + (CELL_SIZE - 16.0) / 2.0;
            cmds.push(RenderCommand::Text {
                x: hx,
                y: y + 6.0,
                text: hdr.to_string(),
                color: theme::SUBTEXT,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(CELL_SIZE),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_day_cell(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        cell: &GridCell,
        store: &EventStore,
    ) {
        let is_today =
            cell.year == self.today.0 && cell.month == self.today.1 && cell.day == self.today.2;
        let is_selected = self.selected_date == Some((cell.year, cell.month, cell.day));

        // Today highlight circle.
        if is_today {
            let circle_x = x + (CELL_SIZE - TODAY_RADIUS * 2.0) / 2.0;
            let circle_y = y + (CELL_SIZE - TODAY_RADIUS * 2.0) / 2.0 - 2.0;
            cmds.push(RenderCommand::FillRect {
                x: circle_x,
                y: circle_y,
                width: TODAY_RADIUS * 2.0,
                height: TODAY_RADIUS * 2.0,
                color: theme::BLUE,
                corner_radii: CornerRadii::all(TODAY_RADIUS),
            });
        } else if is_selected {
            let circle_x = x + (CELL_SIZE - TODAY_RADIUS * 2.0) / 2.0;
            let circle_y = y + (CELL_SIZE - TODAY_RADIUS * 2.0) / 2.0 - 2.0;
            cmds.push(RenderCommand::FillRect {
                x: circle_x,
                y: circle_y,
                width: TODAY_RADIUS * 2.0,
                height: TODAY_RADIUS * 2.0,
                color: theme::SURFACE1,
                corner_radii: CornerRadii::all(TODAY_RADIUS),
            });
        }

        // Day number.
        let text_color = if is_today {
            theme::BASE
        } else if !cell.current_month {
            theme::SURFACE2
        } else {
            theme::TEXT
        };

        let day_str = format!("{}", cell.day);
        // Center the number in the cell.
        let char_offset = if cell.day >= 10 { 4.0 } else { 0.0 };
        let tx = x + (CELL_SIZE - 8.0) / 2.0 - char_offset;
        cmds.push(RenderCommand::Text {
            x: tx,
            y: y + (CELL_SIZE - 12.0) / 2.0 - 2.0,
            text: day_str,
            color: text_color,
            font_size: 13.0,
            font_weight: if is_today {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(CELL_SIZE),
            overflow: TextOverflow::Ellipsis,
        });

        // Event dot indicator.
        let has_events = !store
            .events_for_date(cell.year, cell.month, cell.day)
            .is_empty();
        if has_events {
            let dot_x = x + (CELL_SIZE - DOT_RADIUS * 2.0) / 2.0;
            let dot_y = y + CELL_SIZE - DOT_RADIUS * 2.0 - 4.0;
            let dot_color = if is_today {
                theme::BASE
            } else {
                theme::LAVENDER
            };
            cmds.push(RenderCommand::FillRect {
                x: dot_x,
                y: dot_y,
                width: DOT_RADIUS * 2.0,
                height: DOT_RADIUS * 2.0,
                color: dot_color,
                corner_radii: CornerRadii::all(DOT_RADIUS),
            });
        }
    }

    fn render_event_detail(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _year: i32,
        month: u32,
        day: u32,
        events: &[CalendarEvent],
    ) {
        let visible_count = events.len().min(MAX_VISIBLE_EVENTS);
        let header_h = 28.0;
        let detail_height = header_h + visible_count as f32 * EVENT_ROW_HEIGHT + PADDING;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: detail_height,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });

        // Header: "Month DD".
        let header = format!("{} {day}", month_name_short(month));
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + 6.0,
            text: header,
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Event rows.
        for (i, event) in events.iter().take(MAX_VISIBLE_EVENTS).enumerate() {
            let ey = y + header_h + i as f32 * EVENT_ROW_HEIGHT;

            // Color bar.
            cmds.push(RenderCommand::FillRect {
                x: x + PADDING,
                y: ey + 4.0,
                width: 3.0,
                height: EVENT_ROW_HEIGHT - 8.0,
                color: event.color,
                corner_radii: CornerRadii::all(1.5),
            });

            // Time.
            let (_, _, _, h, m, _) = timestamp_to_date(event.start_timestamp);
            let time_str = if event.all_day {
                "All day".to_string()
            } else {
                format!("{h:02}:{m:02}")
            };
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 10.0,
                y: ey + 6.0,
                text: time_str,
                color: theme::SUBTEXT,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(50.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Title.
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 65.0,
                y: ey + 6.0,
                text: event.title.clone(),
                color: theme::TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0 - 75.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // "N more..." if truncated.
        if events.len() > MAX_VISIBLE_EVENTS {
            let more = events.len().saturating_sub(MAX_VISIBLE_EVENTS);
            let my = y + header_h + visible_count as f32 * EVENT_ROW_HEIGHT;
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 10.0,
                y: my,
                text: format!("{more} more..."),
                color: theme::SUBTEXT,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    // ========================================================================
    // Rendering — year view
    // ========================================================================

    fn render_year_view(&self, x: f32, y: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // 4 columns x 3 rows of mini months.
        let mini_month_w = 7.0 * MINI_CELL + 8.0; // 7 cells + padding
        let mini_month_h = 6.0 * MINI_CELL + MINI_MONTH_LABEL_HEIGHT + 8.0;
        let total_width = 4.0 * mini_month_w + PADDING * 2.0 + 3.0 * 8.0; // 3 gaps
        let total_height = NAV_HEIGHT + 3.0 * mini_month_h + PADDING * 2.0 + 2.0 * 8.0;

        // Background.
        cmds.push(RenderCommand::BoxShadow {
            x,
            y,
            width: total_width,
            height: total_height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: total_width,
            height: total_height,
            color: theme::BASE,
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });

        let mut cy = y + PADDING;

        // Year navigation header.
        let year_label = format!("{}", self.view_year);
        let center_x = text::center_x(
            &year_label,
            x + total_width / 2.0,
            16.0,
            FontWeightHint::Bold,
        );

        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: cy + 10.0,
            text: "<".to_string(),
            color: theme::SUBTEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: x + total_width - PADDING - 16.0,
            y: cy + 10.0,
            text: ">".to_string(),
            color: theme::SUBTEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: center_x,
            y: cy + 10.0,
            text: year_label,
            color: theme::TEXT,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += NAV_HEIGHT;

        // Render 12 mini months, four to a row. Iterating the months and
        // deriving the cell keeps "there are twelve of them" the fact being
        // stated, rather than a 3x4 loop that happens to produce 1..=12.
        for (cell, month) in (1..=12u32).enumerate() {
            let col = cell % 4;
            let row = cell / 4;
            let mx = x + PADDING + col as f32 * (mini_month_w + 8.0);
            let my = cy + row as f32 * (mini_month_h + 8.0);
            self.render_mini_month(&mut cmds, mx, my, self.view_year, month);
        }

        cmds
    }

    fn render_mini_month(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        year: i32,
        month: u32,
    ) {
        let is_current = year == self.today.0 && month == self.today.1;
        let label_color = if is_current { theme::BLUE } else { theme::TEXT };

        // Month label.
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: month_name_short(month).to_string(),
            color: label_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(7.0 * MINI_CELL),
            overflow: TextOverflow::Ellipsis,
        });

        let grid_y = y + MINI_MONTH_LABEL_HEIGHT;

        // The same `month_grid` the full month view is built from, rather
        // than the second implementation that used to live here: a
        // `day_of_week` call, its own Sunday/Monday shift, and
        // `offset + d - 1`. Two answers to "which cell does this day fall
        // in" is one too many — this one would have disagreed with the main
        // grid the moment either changed, and the mini months are drawn
        // right beside it.
        for (pos, date) in Date::from_ymd(year, month, 1)
            .month_grid(self.week_start())
            .enumerate()
        {
            let (cell_year, cell_month, d) = date.ymd();
            if cell_year != year || cell_month != month {
                // Lead-in and spill-over cells belong to the neighbouring
                // months; a mini month shows only its own days.
                continue;
            }
            let col = pos % 7;
            let row = pos / 7;
            let cx = x + col as f32 * MINI_CELL;
            let cell_y = grid_y + row as f32 * MINI_CELL;

            let is_today = year == self.today.0 && month == self.today.1 && d == self.today.2;

            if is_today {
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cell_y,
                    width: MINI_CELL,
                    height: MINI_CELL,
                    color: theme::BLUE,
                    corner_radii: CornerRadii::all(MINI_CELL / 2.0),
                });
            }

            let text_color = if is_today {
                theme::BASE
            } else {
                theme::SUBTEXT
            };
            cmds.push(RenderCommand::Text {
                x: cx + 1.0,
                y: cell_y + 1.0,
                text: format!("{d}"),
                color: text_color,
                font_size: 8.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(MINI_CELL),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Render a clock+date string suitable for the taskbar tray area.
    ///
    /// This is a convenience that uses a `ClockDisplay` to render the
    /// tray clock at the given position.
    pub fn render_tray_clock(
        &self,
        clock: &ClockDisplay,
        x: f32,
        y: f32,
        utc_now: u64,
        local: &Tz,
    ) -> Vec<RenderCommand> {
        clock.render(x, y, utc_now, local)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // --- header centring ---

    #[test]
    fn the_month_label_is_centred_in_the_grid() {
        // Month names are localised and vary a lot in length; the old estimate
        // of eight pixels a byte put the long ones under the ">" arrow.
        let grid_width = 280.0;
        for label in ["January 2026", "May 2026", "Fevereiro 2026", "十二月 2026"] {
            let x = guitk::text::center_x(label, grid_width / 2.0, 15.0, FontWeightHint::Bold);
            let w = guitk::text::measure(label, 15.0, FontWeightHint::Bold);
            assert!(
                (x + w / 2.0 - grid_width / 2.0).abs() < 0.01,
                "{label:?} is not centred"
            );
            assert!(x >= 0.0, "{label:?} starts left of the grid");
        }
    }

    // ========================================================================
    // Date arithmetic tests
    // ========================================================================

    #[test]
    fn days_in_month_non_leap() {
        assert_eq!(days_in_month(2023, 1), 31);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 3), 31);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 5), 31);
        assert_eq!(days_in_month(2023, 6), 30);
        assert_eq!(days_in_month(2023, 7), 31);
        assert_eq!(days_in_month(2023, 8), 31);
        assert_eq!(days_in_month(2023, 9), 30);
        assert_eq!(days_in_month(2023, 10), 31);
        assert_eq!(days_in_month(2023, 11), 30);
        assert_eq!(days_in_month(2023, 12), 31);
    }

    #[test]
    fn days_in_month_leap_february() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
    }

    #[test]
    fn an_impossible_month_is_clamped_rather_than_being_zero_days_long() {
        // This used to answer 0, which is not a month length any caller could
        // use: the recurrence walk stepped `while day > days_in_month(..)`,
        // and a zero there is a loop that never advances. Clamping means every
        // month number names a real month.
        assert_eq!(days_in_month(2024, 0), 31, "month 0 reads as January");
        assert_eq!(days_in_month(2024, 13), 31, "month 13 reads as December");
        assert_eq!(days_in_month(2024, u32::MAX), 31);
    }

    #[test]
    fn day_of_week_known_dates() {
        // 2024-01-01 is Monday.
        assert_eq!(day_of_week(2024, 1, 1), 1);
        // 2023-12-25 (Christmas) is Monday.
        assert_eq!(day_of_week(2023, 12, 25), 1);
        // 2026-05-18 (today per system) is Monday.
        assert_eq!(day_of_week(2026, 5, 18), 1);
        // 1970-01-01 (epoch) is Thursday.
        assert_eq!(day_of_week(1970, 1, 1), 4);
        // 2000-02-29 (leap day) is Tuesday.
        assert_eq!(day_of_week(2000, 2, 29), 2);
    }

    #[test]
    fn day_of_week_sunday() {
        // 2024-01-07 is Sunday.
        assert_eq!(day_of_week(2024, 1, 7), 0);
    }

    #[test]
    fn timestamp_roundtrip() {
        // Epoch.
        let (y, m, d, h, mn, s) = timestamp_to_date(0);
        assert_eq!((y, m, d, h, mn, s), (1970, 1, 1, 0, 0, 0));

        // Known timestamp: 2024-01-01 00:00:00 UTC = 1704067200.
        let ts = 1704067200;
        let (y, m, d, h, mn, s) = timestamp_to_date(ts);
        assert_eq!((y, m, d, h, mn, s), (2024, 1, 1, 0, 0, 0));

        // Roundtrip.
        let back = date_to_timestamp(y, m, d, h, mn, s);
        assert_eq!(back, Some(ts));
    }

    #[test]
    fn timestamp_with_time() {
        // 2024-06-15 14:30:45 UTC.
        let ts = date_to_timestamp(2024, 6, 15, 14, 30, 45).expect("valid date");
        let (y, m, d, h, mn, s) = timestamp_to_date(ts);
        assert_eq!((y, m, d, h, mn, s), (2024, 6, 15, 14, 30, 45));
    }

    #[test]
    fn timestamp_before_epoch_returns_none() {
        assert_eq!(date_to_timestamp(1969, 12, 31, 0, 0, 0), None);
    }

    #[test]
    fn timestamp_leap_day() {
        // 2024-02-29 is valid.
        let ts = date_to_timestamp(2024, 2, 29, 12, 0, 0).expect("valid");
        let (y, m, d, _, _, _) = timestamp_to_date(ts);
        assert_eq!((y, m, d), (2024, 2, 29));
    }

    #[test]
    fn iso_week_number_jan1_2024() {
        // 2024-01-01 (Monday) is in ISO week 1 of 2024.
        let (iso_y, wn) = iso_week_number(2024, 1, 1);
        assert_eq!((iso_y, wn), (2024, 1));
    }

    #[test]
    fn iso_week_number_dec31_year_boundary() {
        // 2026-12-31 (Thursday) — ISO week 53 is possible when the year
        // starts on Thursday or the prev year was a leap year starting on Wed.
        // 2026-12-31 is a Thursday. Jan 1, 2026 is Thursday.
        // So week 53 is valid. Let us just verify it doesn't panic.
        let (iso_y, wn) = iso_week_number(2026, 12, 31);
        assert!(wn >= 1 && wn <= 53);
        assert!(iso_y == 2026 || iso_y == 2027);
    }

    #[test]
    fn iso_week_number_jan1_on_friday() {
        // 2021-01-01 is a Friday. It should be ISO week 53 of 2020.
        let (iso_y, wn) = iso_week_number(2021, 1, 1);
        assert_eq!((iso_y, wn), (2020, 53));
    }

    // ========================================================================
    // Calendar grid tests
    // ========================================================================

    #[test]
    fn grid_always_42_cells() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        for month in 1..=12 {
            cal.view_year = 2024;
            cal.view_month = month;
            let grid = cal.generate_grid();
            assert_eq!(
                grid.len(),
                42,
                "Grid for month {month} should have 42 cells"
            );
        }
    }

    #[test]
    fn grid_current_month_days_present() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.view_month = 3; // March: 31 days.
        let grid = cal.generate_grid();

        let current_month_days: Vec<u32> = grid
            .iter()
            .filter(|c| c.current_month)
            .map(|c| c.day)
            .collect();

        assert_eq!(current_month_days.len(), 31);
        assert_eq!(*current_month_days.first().expect("has first"), 1);
        assert_eq!(*current_month_days.last().expect("has last"), 31);
    }

    #[test]
    fn grid_february_leap() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.view_month = 2;
        let grid = cal.generate_grid();

        let feb_days: Vec<u32> = grid
            .iter()
            .filter(|c| c.current_month)
            .map(|c| c.day)
            .collect();

        assert_eq!(feb_days.len(), 29);
        assert!(feb_days.contains(&29));
    }

    #[test]
    fn grid_february_non_leap() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2023;
        cal.view_month = 2;
        let grid = cal.generate_grid();

        let feb_days: Vec<u32> = grid
            .iter()
            .filter(|c| c.current_month)
            .map(|c| c.day)
            .collect();

        assert_eq!(feb_days.len(), 28);
        assert!(!feb_days.contains(&29));
    }

    #[test]
    fn grid_first_day_monday_config() {
        let config = CalendarConfig {
            first_day_of_week: FirstDayOfWeek::Monday,
            ..Default::default()
        };
        let mut cal = CalendarView::new(config);
        // 2024-01-01 is Monday, so with Monday-first the grid should start on day 1.
        cal.view_year = 2024;
        cal.view_month = 1;
        let grid = cal.generate_grid();

        // First cell should be Jan 1 (current month).
        assert!(grid[0].current_month);
        assert_eq!(grid[0].day, 1);
    }

    /// Which grid cell each day of `month` lands in, as the mini month now
    /// computes it: from the shared `month_grid`.
    fn mini_month_cells(cal: &CalendarView, year: i32, month: u32) -> Vec<(u32, usize)> {
        Date::from_ymd(year, month, 1)
            .month_grid(cal.week_start())
            .enumerate()
            .filter_map(|(pos, date)| {
                let (y, m, d) = date.ymd();
                (y == year && m == month).then_some((d, pos))
            })
            .collect()
    }

    #[test]
    fn the_mini_month_puts_each_day_in_the_same_cell_as_the_full_grid() {
        // The mini month used to compute its cells from its own `day_of_week`
        // call, its own Sunday/Monday shift, and `offset + d - 1`. Two
        // answers to "which cell does this day fall in", drawn side by side
        // in the year view. Check they agree, for both week starts and for
        // every month of a leap year and a common one.
        for first_day in [FirstDayOfWeek::Sunday, FirstDayOfWeek::Monday] {
            let config = CalendarConfig {
                first_day_of_week: first_day,
                ..Default::default()
            };
            let mut cal = CalendarView::new(config);
            for year in [2023, 2024] {
                for month in 1..=12 {
                    cal.view_year = year;
                    cal.view_month = month;
                    let full: Vec<(u32, usize)> = cal
                        .generate_grid()
                        .iter()
                        .enumerate()
                        .filter_map(|(pos, c)| c.current_month.then_some((c.day, pos)))
                        .collect();
                    assert_eq!(
                        mini_month_cells(&cal, year, month),
                        full,
                        "{year}-{month:02} with {first_day:?} first"
                    );
                    assert_eq!(
                        full.len() as u32,
                        days_in_month(year, month),
                        "{year}-{month:02} lost a day"
                    );
                }
            }
        }
    }

    #[test]
    fn a_row_of_the_grid_is_seven_consecutive_days() {
        // `chunks(7)` replaced `row * 7 + col`; a row must still be a week.
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.view_month = 3;
        let grid = cal.generate_grid();
        assert_eq!(grid.len(), 42);
        let rows: Vec<&[GridCell]> = grid.chunks(7).collect();
        assert_eq!(rows.len(), 6, "six weeks");
        for week in &rows {
            assert_eq!(week.len(), 7);
            // The week number is read from the row's first cell, so every
            // row must have one.
            assert!(!week.is_empty());
        }
        // And consecutive rows are seven days apart.
        for pair in rows.windows(2) {
            let (Some(a), Some(b)) = (
                pair.first().and_then(|w| w.first()),
                pair.get(1).and_then(|w| w.first()),
            ) else {
                panic!("every row has a first cell");
            };
            let a_date = Date::from_ymd(a.year, a.month, a.day);
            let b_date = Date::from_ymd(b.year, b.month, b.day);
            assert_eq!(a_date.add_days(7), b_date, "rows are one week apart");
        }
    }

    #[test]
    fn the_twelve_hour_clock_reads_the_same_at_every_hour_of_the_day() {
        // The `hour % 12` formula replaced a four-armed ladder that existed
        // to keep its own last arm from subtracting below zero. Walk the day.
        let expected: [(u64, &str, &str); 6] = [
            (0, "12", "AM"),
            (1, "1", "AM"),
            (11, "11", "AM"),
            (12, "12", "PM"),
            (13, "1", "PM"),
            (23, "11", "PM"),
        ];
        let clock = ClockDisplay {
            use_24h: false,
            show_seconds: false,
            extra_timezones: Vec::new(),
        };
        for (hour, h12, ampm) in expected {
            let ts = hour * SECS_PER_HOUR;
            let text = clock.format_time(ts, &Tz::UTC);
            assert_eq!(text, format!("{h12}:00 {ampm}"), "hour {hour}");
        }
    }

    #[test]
    fn grid_leading_trailing_days() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        // March 2024: March 1 is Friday (dow=5).
        // With Sunday-first, offset = 5, so 5 leading days from February.
        cal.view_year = 2024;
        cal.view_month = 3;
        let grid = cal.generate_grid();

        // Leading days should be from Feb.
        let leading: Vec<&GridCell> = grid.iter().take_while(|c| !c.current_month).collect();
        assert_eq!(leading.len(), 5);
        // Feb 2024 has 29 days; leading should be 25, 26, 27, 28, 29.
        assert_eq!(leading[0].day, 25);
        assert_eq!(leading[0].month, 2);
        assert_eq!(leading[4].day, 29);

        // Trailing days should be from April.
        let trailing: Vec<&GridCell> = grid.iter().rev().take_while(|c| !c.current_month).collect();
        assert!(!trailing.is_empty());
        // All trailing should be month 4.
        for t in &trailing {
            assert_eq!(t.month, 4);
        }
    }

    #[test]
    fn grid_january_has_december_leading() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.view_month = 1;
        let grid = cal.generate_grid();

        let leading: Vec<&GridCell> = grid.iter().take_while(|c| !c.current_month).collect();
        // Jan 1, 2024 is Monday (dow=1), Sunday-first offset = 1.
        assert_eq!(leading.len(), 1);
        assert_eq!(leading[0].month, 12);
        assert_eq!(leading[0].year, 2023);
        assert_eq!(leading[0].day, 31);
    }

    #[test]
    fn grid_december_has_january_trailing() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.view_month = 12;
        let grid = cal.generate_grid();

        let trailing: Vec<&GridCell> = grid.iter().rev().take_while(|c| !c.current_month).collect();
        assert!(!trailing.is_empty());
        for t in &trailing {
            assert_eq!(t.month, 1);
            assert_eq!(t.year, 2025);
        }
    }

    // ========================================================================
    // Navigation tests
    // ========================================================================

    #[test]
    fn prev_month_wraps_year() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.view_month = 1;
        cal.prev_month();
        assert_eq!(cal.view_year, 2023);
        assert_eq!(cal.view_month, 12);
    }

    #[test]
    fn next_month_wraps_year() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.view_month = 12;
        cal.next_month();
        assert_eq!(cal.view_year, 2025);
        assert_eq!(cal.view_month, 1);
    }

    #[test]
    fn go_to_today_resets_view() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.today = (2026, 5, 18);
        cal.view_year = 2020;
        cal.view_month = 3;
        cal.mode = CalendarViewMode::Year;
        cal.go_to_today();
        assert_eq!(cal.view_year, 2026);
        assert_eq!(cal.view_month, 5);
        assert_eq!(cal.mode, CalendarViewMode::Month);
    }

    #[test]
    fn prev_next_year() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.view_year = 2024;
        cal.prev_year();
        assert_eq!(cal.view_year, 2023);
        cal.next_year();
        cal.next_year();
        assert_eq!(cal.view_year, 2025);
    }

    // ========================================================================
    // EventStore CRUD tests
    // ========================================================================

    /// Add an event, insisting the store could mint an ID for it.
    ///
    /// `add_event` returns an `Option` now, and a test that wrote
    /// `let _ = store.add_event(..)` at each of its twenty-odd setup sites
    /// would be discarding exactly the signal those sites depend on.
    fn add(store: &mut EventStore, event: CalendarEvent) -> u64 {
        store
            .add_event(event)
            .expect("a store with a handful of events can still mint an ID")
    }

    fn make_event(title: &str, start: u64, end: u64) -> CalendarEvent {
        CalendarEvent {
            id: 0,
            title: title.to_string(),
            start_timestamp: start,
            end_timestamp: end,
            all_day: false,
            repeat: None,
            color: theme::BLUE,
            description: String::new(),
        }
    }

    #[test]
    fn event_store_add_assigns_ids() {
        let mut store = EventStore::new();
        let id1 = add(&mut store, make_event("A", 100, 200));
        let id2 = add(&mut store, make_event("B", 300, 400));
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn event_store_remove() {
        let mut store = EventStore::new();
        let id = add(&mut store, make_event("A", 100, 200));
        assert!(store.remove_event(id));
        assert!(store.is_empty());
        // Removing again should return false.
        assert!(!store.remove_event(id));
    }

    #[test]
    fn event_store_update() {
        let mut store = EventStore::new();
        let id = add(&mut store, make_event("Old Title", 100, 200));
        let updated = store.update_event(id, |e| {
            e.title = "New Title".to_string();
        });
        assert!(updated);
        assert_eq!(store.get_event(id).expect("exists").title, "New Title");
    }

    #[test]
    fn event_store_update_nonexistent() {
        let mut store = EventStore::new();
        assert!(!store.update_event(999, |_| {}));
    }

    #[test]
    fn event_store_get() {
        let mut store = EventStore::new();
        let id = add(&mut store, make_event("Test", 100, 200));
        assert!(store.get_event(id).is_some());
        assert!(store.get_event(999).is_none());
    }

    #[test]
    fn events_for_date_non_recurring() {
        let mut store = EventStore::new();
        // Event on 2024-06-15 at 10:00-11:00 UTC.
        let start = date_to_timestamp(2024, 6, 15, 10, 0, 0).expect("valid");
        let end = start + 3600;
        add(&mut store, make_event("Meeting", start, end));

        let found = store.events_for_date(2024, 6, 15);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Meeting");

        // Different day should find nothing.
        let not_found = store.events_for_date(2024, 6, 16);
        assert!(not_found.is_empty());
    }

    #[test]
    fn events_for_date_spanning_midnight() {
        let mut store = EventStore::new();
        // Event from June 15 23:00 to June 16 01:00.
        let start = date_to_timestamp(2024, 6, 15, 23, 0, 0).expect("valid");
        let end = date_to_timestamp(2024, 6, 16, 1, 0, 0).expect("valid");
        add(&mut store, make_event("Late Night", start, end));

        // Should appear on both days.
        assert_eq!(store.events_for_date(2024, 6, 15).len(), 1);
        assert_eq!(store.events_for_date(2024, 6, 16).len(), 1);
    }

    #[test]
    fn events_for_range() {
        let mut store = EventStore::new();
        let ts1 = date_to_timestamp(2024, 6, 10, 9, 0, 0).expect("valid");
        let ts2 = date_to_timestamp(2024, 6, 15, 9, 0, 0).expect("valid");
        let ts3 = date_to_timestamp(2024, 6, 20, 9, 0, 0).expect("valid");
        add(&mut store, make_event("A", ts1, ts1 + 3600));
        add(&mut store, make_event("B", ts2, ts2 + 3600));
        add(&mut store, make_event("C", ts3, ts3 + 3600));

        let range_start = date_to_timestamp(2024, 6, 12, 0, 0, 0).expect("valid");
        let range_end = date_to_timestamp(2024, 6, 18, 0, 0, 0).expect("valid");
        let found = store.events_for_range(range_start, range_end);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "B");
    }

    #[test]
    fn search_case_insensitive() {
        let mut store = EventStore::new();
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Team Meeting".to_string(),
                start_timestamp: 1000,
                end_timestamp: 2000,
                all_day: false,
                repeat: None,
                color: theme::BLUE,
                description: "Weekly standup with the engineering team".to_string(),
            },
        );
        add(&mut store, make_event("Lunch", 3000, 4000));

        let results = store.search("meeting");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Team Meeting");

        let results2 = store.search("engineering");
        assert_eq!(results2.len(), 1);

        let results3 = store.search("xyz");
        assert!(results3.is_empty());
    }

    // ========================================================================
    // Recurring event expansion tests
    // ========================================================================

    #[test]
    fn recurring_daily() {
        let mut store = EventStore::new();
        let start = date_to_timestamp(2024, 6, 1, 10, 0, 0).expect("valid");
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Daily Standup".to_string(),
                start_timestamp: start,
                end_timestamp: start + 1800, // 30 min
                all_day: false,
                repeat: Some(Recurrence::Daily),
                color: theme::GREEN,
                description: String::new(),
            },
        );

        // Check June 1-5 (5 days).
        let range_start = date_to_timestamp(2024, 6, 1, 0, 0, 0).expect("valid");
        let range_end = date_to_timestamp(2024, 6, 6, 0, 0, 0).expect("valid");
        let events = store.events_for_range(range_start, range_end);
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn recurring_weekly() {
        let mut store = EventStore::new();
        // Starting on a Monday (2024-06-03).
        let start = date_to_timestamp(2024, 6, 3, 14, 0, 0).expect("valid");
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Weekly Review".to_string(),
                start_timestamp: start,
                end_timestamp: start + 3600,
                all_day: false,
                repeat: Some(Recurrence::Weekly),
                color: theme::PEACH,
                description: String::new(),
            },
        );

        // Check entire month of June.
        let range_start = date_to_timestamp(2024, 6, 1, 0, 0, 0).expect("valid");
        let range_end = date_to_timestamp(2024, 7, 1, 0, 0, 0).expect("valid");
        let events = store.events_for_range(range_start, range_end);
        // June 3, 10, 17, 24 = 4 occurrences.
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn recurring_monthly() {
        let mut store = EventStore::new();
        let start = date_to_timestamp(2024, 1, 15, 9, 0, 0).expect("valid");
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Monthly Report".to_string(),
                start_timestamp: start,
                end_timestamp: start + 7200,
                all_day: false,
                repeat: Some(Recurrence::Monthly),
                color: theme::YELLOW,
                description: String::new(),
            },
        );

        // Check Jan-June (6 months).
        let range_start = date_to_timestamp(2024, 1, 1, 0, 0, 0).expect("valid");
        let range_end = date_to_timestamp(2024, 7, 1, 0, 0, 0).expect("valid");
        let events = store.events_for_range(range_start, range_end);
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn recurring_monthly_day31_clamped() {
        let mut store = EventStore::new();
        // Start on Jan 31.
        let start = date_to_timestamp(2024, 1, 31, 10, 0, 0).expect("valid");
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Payday".to_string(),
                start_timestamp: start,
                end_timestamp: start + 3600,
                all_day: false,
                repeat: Some(Recurrence::Monthly),
                color: theme::GREEN,
                description: String::new(),
            },
        );

        // February 2024 has 29 days; the event should appear on Feb 29.
        let feb_events = store.events_for_date(2024, 2, 29);
        assert_eq!(feb_events.len(), 1);

        // Should also appear on March 31.
        let mar_events = store.events_for_date(2024, 3, 31);
        assert_eq!(mar_events.len(), 1);

        // April has 30 days; should appear on April 30.
        let apr_events = store.events_for_date(2024, 4, 30);
        assert_eq!(apr_events.len(), 1);
    }

    #[test]
    fn recurring_yearly() {
        let mut store = EventStore::new();
        let start = date_to_timestamp(2020, 3, 14, 0, 0, 0).expect("valid");
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Pi Day".to_string(),
                start_timestamp: start,
                end_timestamp: start + SECS_PER_DAY,
                all_day: true,
                repeat: Some(Recurrence::Yearly),
                color: theme::LAVENDER,
                description: String::new(),
            },
        );

        // Should appear in 2024.
        let found = store.events_for_date(2024, 3, 14);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Pi Day");

        // Should not appear on other dates.
        assert!(store.events_for_date(2024, 3, 15).is_empty());
    }

    // ========================================================================
    // ReminderManager tests
    // ========================================================================

    #[test]
    fn reminder_set_and_due() {
        let mut rm = ReminderManager::new();
        // Event at t=1000, reminder 15 minutes before = t=100.
        rm.set_reminder(1, "Meeting", 1000, 15);

        // At t=99, not yet due.
        assert!(rm.due_reminders(99).is_empty());

        // At t=100, due.
        let due = rm.due_reminders(100);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].event_id, 1);

        // At t=500, still due (hasn't been dismissed).
        assert_eq!(rm.due_reminders(500).len(), 1);
    }

    #[test]
    fn reminder_dismiss() {
        let mut rm = ReminderManager::new();
        rm.set_reminder(1, "Meeting", 1000, 15);
        rm.dismiss(1);

        // Should no longer appear in due list.
        assert!(rm.due_reminders(2000).is_empty());
        assert_eq!(rm.active_count(), 0);
    }

    #[test]
    fn reminder_snooze() {
        let mut rm = ReminderManager::new();
        rm.set_reminder(1, "Meeting", 1000, 15);
        // fire_at = 1000 - 900 = 100.

        // Snooze 5 minutes (300 seconds).
        rm.snooze(1, SnoozeDuration::FiveMinutes);
        // New fire_at = 100 + 300 = 400.
        assert!(rm.due_reminders(399).is_empty());
        assert_eq!(rm.due_reminders(400).len(), 1);
    }

    #[test]
    fn reminder_snooze_durations() {
        assert_eq!(SnoozeDuration::FiveMinutes.secs(), 300);
        assert_eq!(SnoozeDuration::FifteenMinutes.secs(), 900);
        assert_eq!(SnoozeDuration::ThirtyMinutes.secs(), 1800);
        assert_eq!(SnoozeDuration::OneHour.secs(), 3600);
    }

    #[test]
    fn reminder_dismiss_all() {
        let mut rm = ReminderManager::new();
        rm.set_reminder(1, "A", 1000, 10);
        rm.set_reminder(2, "B", 2000, 10);
        rm.set_reminder(3, "C", 3000, 10);
        assert_eq!(rm.active_count(), 3);

        rm.dismiss_all();
        assert_eq!(rm.active_count(), 0);
        assert!(rm.due_reminders(5000).is_empty());
    }

    #[test]
    fn reminder_prune_dismissed() {
        let mut rm = ReminderManager::new();
        rm.set_reminder(1, "A", 1000, 10);
        rm.set_reminder(2, "B", 2000, 10);
        rm.dismiss(1);
        assert_eq!(rm.all_reminders().len(), 2);

        rm.prune_dismissed();
        assert_eq!(rm.all_reminders().len(), 1);
        assert_eq!(rm.all_reminders()[0].event_id, 2);
    }

    #[test]
    fn multiple_reminders_same_event() {
        let mut rm = ReminderManager::new();
        rm.set_reminder(1, "Meeting", 1000, 15);
        rm.set_reminder(1, "Meeting", 1000, 5);

        // Both should fire at or before t=1000.
        let due = rm.due_reminders(1000);
        assert_eq!(due.len(), 2);
    }

    // ========================================================================
    // ClockDisplay tests
    // ========================================================================

    /// A zone from a POSIX `TZ` string, for the tests below.  Panicking on a
    /// literal the test itself wrote is the right failure mode here: a typo in
    /// the string would otherwise silently become UTC and make the assertion
    /// test nothing.
    fn tz(s: &str) -> Tz {
        Tz::parse(s.as_bytes()).expect("test TZ string should parse")
    }

    #[test]
    fn clock_24h_format() {
        let clock = ClockDisplay {
            use_24h: true,
            show_seconds: false,
            extra_timezones: Vec::new(),
        };
        // Epoch = midnight UTC.
        assert_eq!(clock.format_time(0, &Tz::UTC), "00:00");
        // 13:45 UTC.
        let ts = 13 * 3600 + 45 * 60;
        assert_eq!(clock.format_time(ts, &Tz::UTC), "13:45");
    }

    #[test]
    fn clock_24h_with_seconds() {
        let clock = ClockDisplay {
            use_24h: true,
            show_seconds: true,
            extra_timezones: Vec::new(),
        };
        let ts = 13 * 3600 + 45 * 60 + 30;
        assert_eq!(clock.format_time(ts, &Tz::UTC), "13:45:30");
    }

    #[test]
    fn clock_12h_format() {
        let clock = ClockDisplay {
            use_24h: false,
            show_seconds: false,
            extra_timezones: Vec::new(),
        };
        // Midnight.
        assert_eq!(clock.format_time(0, &Tz::UTC), "12:00 AM");
        // Noon.
        assert_eq!(clock.format_time(12 * 3600, &Tz::UTC), "12:00 PM");
        // 1 PM.
        assert_eq!(clock.format_time(13 * 3600, &Tz::UTC), "1:00 PM");
        // 11 AM.
        assert_eq!(clock.format_time(11 * 3600, &Tz::UTC), "11:00 AM");
    }

    #[test]
    fn clock_timezone_offset() {
        let clock = ClockDisplay {
            use_24h: true,
            show_seconds: false,
            extra_timezones: Vec::new(),
        };
        // India: UTC+5:30, and no DST rule — POSIX writes an *east* offset
        // with a minus sign.  At UTC midnight, local time is 05:30.
        assert_eq!(clock.format_time(0, &tz("IST-5:30")), "05:30");
    }

    /// The bug this API shape exists to prevent: a zone with daylight saving
    /// has two offsets, and which one applies depends on the instant.  A
    /// `utc_offset_secs: i64` field could only ever hold one of them, so the
    /// taskbar read an hour wrong for the ~8 months of EDT.
    #[test]
    fn clock_follows_a_daylight_saving_transition() {
        let clock = ClockDisplay::new();
        let eastern = tz("EST5EDT,M3.2.0,M11.1.0");
        // 2001-09-09 01:46:40 UTC — inside EDT (UTC-4).
        assert_eq!(clock.format_time(1_000_000_000, &eastern), "21:46");
        // 2001-01-01 00:00:00 UTC — inside EST (UTC-5).
        assert_eq!(clock.format_time(978_307_200, &eastern), "19:00");
        // A zone that observes no DST is the same all year, which is the
        // control that keeps the two readings above meaningful.
        let kolkata = tz("IST-5:30");
        assert_eq!(clock.format_time(1_000_000_000, &kolkata), "07:16");
        assert_eq!(clock.format_time(978_307_200, &kolkata), "05:30");
    }

    /// The date must be taken from the same shifted instant as the time, or a
    /// clock reading just after local midnight would show yesterday.
    #[test]
    fn clock_date_moves_with_the_zone() {
        let clock = ClockDisplay::new();
        let ts = 1_704_067_200; // 2024-01-01 00:00:00 UTC — a Monday.
        assert_eq!(clock.format_date(ts, &Tz::UTC), "Monday, January 1, 2024");
        // Five hours behind: still New Year's Eve, and a Sunday.
        assert_eq!(
            clock.format_date(ts, &tz("EST5EDT,M3.2.0,M11.1.0")),
            "Sunday, December 31, 2023"
        );
        assert_eq!(
            clock.format_time(ts, &tz("EST5EDT,M3.2.0,M11.1.0")),
            "19:00"
        );
    }

    #[test]
    fn clock_date_format() {
        let clock = ClockDisplay::new();
        // 2024-01-01 00:00 UTC.
        let ts = 1704067200;
        let date_str = clock.format_date(ts, &Tz::UTC);
        assert_eq!(date_str, "Monday, January 1, 2024");
    }

    #[test]
    fn clock_max_timezones() {
        let mut clock = ClockDisplay::new();
        assert!(clock.add_timezone("New York", "EST5EDT,M3.2.0,M11.1.0"));
        assert!(clock.add_timezone("London", "GMT0BST,M3.5.0/1,M10.5.0"));
        assert!(clock.add_timezone("Tokyo", "JST-9"));
        // Full — refused, and says so rather than silently dropping it.
        assert!(!clock.add_timezone("Sydney", "AEST-10AEDT,M10.1.0,M4.1.0/3"));
        assert_eq!(clock.extra_timezones.len(), 3);
    }

    /// A zoneinfo *name* is not a POSIX `TZ` string.  Accepting it and falling
    /// back to UTC would put a wrong time on the taskbar under a label
    /// claiming otherwise, so it is refused outright.
    #[test]
    fn clock_refuses_a_zone_it_cannot_actually_render() {
        let mut clock = ClockDisplay::new();
        for bad in ["America/New_York", "", "Mars", "EST5EDT,garbage"] {
            assert!(
                !clock.add_timezone("somewhere", bad),
                "{bad:?} should not be accepted as a POSIX TZ string"
            );
        }
        assert!(clock.extra_timezones.is_empty());
    }

    // ========================================================================
    // Import/Export round-trip tests
    // ========================================================================

    #[test]
    fn export_import_roundtrip() {
        let mut store = EventStore::new();
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Team Meeting".to_string(),
                start_timestamp: 1_700_000_000,
                end_timestamp: 1_700_003_600,
                all_day: false,
                repeat: Some(Recurrence::Weekly),
                color: Color::from_hex(0xA6E3A1),
                description: "Weekly sync".to_string(),
            },
        );
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Holiday".to_string(),
                start_timestamp: 1_700_100_000,
                end_timestamp: 1_700_186_400,
                all_day: true,
                repeat: None,
                color: Color::from_hex(0xF9E2AF),
                description: "Day off".to_string(),
            },
        );

        let exported = store.export_text();

        // Import into a fresh store.
        let mut store2 = EventStore::new();
        let count = store2.import_text(&exported);
        assert_eq!(count, 2);
        assert_eq!(store2.len(), 2);

        // Verify content.
        let events = store2.all_events();
        assert_eq!(events[0].title, "Team Meeting");
        assert_eq!(events[0].start_timestamp, 1_700_000_000);
        assert_eq!(events[0].repeat, Some(Recurrence::Weekly));
        assert!(!events[0].all_day);

        assert_eq!(events[1].title, "Holiday");
        assert!(events[1].all_day);
        assert_eq!(events[1].repeat, None);
    }

    #[test]
    fn import_empty_text() {
        let mut store = EventStore::new();
        let count = store.import_text("");
        assert_eq!(count, 0);
        assert!(store.is_empty());
    }

    #[test]
    fn import_single_event() {
        let mut store = EventStore::new();
        let text = "\
EVENT
title: Quick Note
start: 5000
end: 6000
all_day: false
repeat: none
color: 89B4FA
description: Just a test";

        let count = store.import_text(text);
        assert_eq!(count, 1);
        let e = store.get_event(1).expect("event 1");
        assert_eq!(e.title, "Quick Note");
        assert_eq!(e.start_timestamp, 5000);
        assert_eq!(e.end_timestamp, 6000);
        assert_eq!(e.color, Color::from_hex(0x89B4FA));
    }

    #[test]
    fn export_color_hex_format() {
        let mut store = EventStore::new();
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Test".to_string(),
                start_timestamp: 0,
                end_timestamp: 100,
                all_day: false,
                repeat: None,
                color: Color::from_hex(0xF38BA8),
                description: String::new(),
            },
        );

        let text = store.export_text();
        assert!(
            text.contains("color: F38BA8"),
            "Expected hex color in export, got: {text}"
        );
    }

    // ========================================================================
    // Rendering tests (smoke tests: verify non-empty output)
    // ========================================================================

    #[test]
    fn render_hidden_returns_empty() {
        let cal = CalendarView::new(CalendarConfig::default());
        let store = EventStore::new();
        let cmds = cal.render(0.0, 0.0, &store);
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_month_view_produces_commands() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);

        let store = EventStore::new();
        let cmds = cal.render(100.0, 100.0, &store);
        // Should have popup bg, border, nav header, dow headers, and 42 day cells minimum.
        assert!(
            cmds.len() > 50,
            "Expected many render commands, got {}",
            cmds.len()
        );
    }

    #[test]
    fn render_year_view_produces_commands() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);
        cal.mode = CalendarViewMode::Year;

        let store = EventStore::new();
        let cmds = cal.render(0.0, 0.0, &store);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_with_week_numbers() {
        let config = CalendarConfig {
            show_week_numbers: true,
            ..Default::default()
        };
        let mut cal = CalendarView::new(config);
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);

        let store = EventStore::new();
        let cmds = cal.render(0.0, 0.0, &store);
        // Should have extra text commands for week numbers.
        let text_cmds: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { .. }))
            .collect();
        // At least 6 week number texts + 7 dow headers + 42 day numbers + nav.
        assert!(
            text_cmds.len() >= 55,
            "Expected 55+ text commands, got {}",
            text_cmds.len()
        );
    }

    #[test]
    fn render_event_dots_shown() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);

        let mut store = EventStore::new();
        let start = date_to_timestamp(2026, 5, 18, 10, 0, 0).expect("valid");
        add(&mut store, make_event("Test Event", start, start + 3600));

        let cmds = cal.render(0.0, 0.0, &store);
        // Should contain at least one small dot-sized FillRect.
        let has_dot = cmds.iter().any(|c| match c {
            RenderCommand::FillRect { width, height, .. } => {
                (*width - DOT_RADIUS * 2.0).abs() < 0.01
                    && (*height - DOT_RADIUS * 2.0).abs() < 0.01
            }
            _ => false,
        });
        assert!(has_dot, "Expected event dot in render output");
    }

    #[test]
    fn render_selected_date_shows_detail() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);
        cal.selected_date = Some((2026, 5, 18));

        let mut store = EventStore::new();
        let start = date_to_timestamp(2026, 5, 18, 10, 0, 0).expect("valid");
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Visible Event".to_string(),
                start_timestamp: start,
                end_timestamp: start + 3600,
                all_day: false,
                repeat: None,
                color: theme::PEACH,
                description: String::new(),
            },
        );

        let cmds = cal.render(0.0, 0.0, &store);
        // Should contain a text command with the event title.
        let has_event_text = cmds.iter().any(|c| match c {
            RenderCommand::Text { text, .. } => text == "Visible Event",
            _ => false,
        });
        assert!(has_event_text, "Expected event title in render output");
    }

    #[test]
    fn clock_render_produces_commands() {
        let clock = ClockDisplay::new();
        let cmds = clock.render(0.0, 0.0, 1_700_000_000, &Tz::UTC);
        // At minimum: time text + date text.
        assert!(cmds.len() >= 2);
    }

    #[test]
    fn clock_render_with_extra_timezones() {
        let mut clock = ClockDisplay::new();
        assert!(clock.add_timezone("Tokyo", "JST-9"));
        assert!(clock.add_timezone("London", "GMT0BST,M3.5.0/1,M10.5.0"));

        let cmds = clock.render(0.0, 0.0, 1_700_000_000, &Tz::UTC);
        // time + date + 2 timezone lines.
        assert!(cmds.len() >= 4);
    }

    // ========================================================================
    // Miscellaneous edge cases
    // ========================================================================

    #[test]
    fn parse_hex_color_valid() {
        let c = parse_hex_color("89B4FA").expect("valid");
        assert_eq!(c, Color::from_hex(0x89B4FA));
    }

    #[test]
    fn parse_hex_color_invalid_length() {
        assert!(parse_hex_color("FFF").is_none());
        assert!(parse_hex_color("").is_none());
        assert!(parse_hex_color("1234567").is_none());
    }

    #[test]
    fn parse_hex_color_invalid_chars() {
        assert!(parse_hex_color("ZZZZZZ").is_none());
    }

    #[test]
    fn event_duration() {
        let e = make_event("X", 1000, 2000);
        assert_eq!(e.duration_secs(), 1000);

        // Zero-length event.
        let e2 = make_event("Y", 500, 500);
        assert_eq!(e2.duration_secs(), 0);
    }

    #[test]
    fn set_today_from_timestamp() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        // 2024-06-15 at midnight UTC.
        let ts = date_to_timestamp(2024, 6, 15, 0, 0, 0).expect("valid");
        cal.set_today_from_timestamp(ts, 0);
        assert_eq!(cal.today, (2024, 6, 15));
        assert_eq!(cal.view_year, 2024);
        assert_eq!(cal.view_month, 6);
    }

    #[test]
    fn toggle_visibility() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.today = (2026, 5, 18);
        assert!(!cal.visible);
        cal.toggle();
        assert!(cal.visible);
        cal.toggle();
        assert!(!cal.visible);
    }

    #[test]
    fn month_names_correct() {
        assert_eq!(month_name(1), "January");
        assert_eq!(month_name(6), "June");
        assert_eq!(month_name(12), "December");
        assert_eq!(month_name_short(1), "Jan");
        assert_eq!(month_name_short(12), "Dec");
    }

    #[test]
    fn dow_headers_sunday_start() {
        let h = dow_headers(FirstDayOfWeek::Sunday);
        assert_eq!(h[0], "Su");
        assert_eq!(h[6], "Sa");
    }

    #[test]
    fn dow_headers_monday_start() {
        let h = dow_headers(FirstDayOfWeek::Monday);
        assert_eq!(h[0], "Mo");
        assert_eq!(h[6], "Su");
    }

    #[test]
    fn recurring_event_does_not_appear_before_start() {
        let mut store = EventStore::new();
        let start = date_to_timestamp(2024, 6, 15, 10, 0, 0).expect("valid");
        add(
            &mut store,
            CalendarEvent {
                id: 0,
                title: "Future Weekly".to_string(),
                start_timestamp: start,
                end_timestamp: start + 3600,
                all_day: false,
                repeat: Some(Recurrence::Weekly),
                color: theme::BLUE,
                description: String::new(),
            },
        );

        // Query a range entirely before the start date.
        let range_start = date_to_timestamp(2024, 5, 1, 0, 0, 0).expect("valid");
        let range_end = date_to_timestamp(2024, 6, 1, 0, 0, 0).expect("valid");
        let events = store.events_for_range(range_start, range_end);
        assert!(events.is_empty());
    }
}
