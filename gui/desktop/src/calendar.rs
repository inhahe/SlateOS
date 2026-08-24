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
//! clock.show_date = true;
//!
//! // The taskbar draws the reading itself, into its own themed render tree,
//! // and sizes the tray from `reading_width` so it is never clipped:
//! let slot = clock.reading_width(font_size);
//! let reading = clock.format_taskbar(utc_now, &local_zone);
//!
//! // Click on the clock opens the calendar popup:
//! cal.set_visible(true);
//!
//! // Each frame, if visible. `scale` is the shell's device scale: the popup
//! // is laid out in physical pixels because it is anchored to shell chrome
//! // that already is, and `utc_now` is a parameter so the clock band across
//! // its head reads the current second rather than the second it opened on.
//! let cmds = cal.render(x, y, scale, utc_now, &store);
//!
//! // A click anywhere while it is open, tested against the same layout the
//! // render came from:
//! match cal.hit_test(x, y, scale, click_x, click_y, &store) {
//!     Some(hit) => { cal.apply(hit); }
//!     None => cal.set_visible(false), // outside the popup: dismiss
//! }
//! ```

use appearance::{Palette, readable_on};
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

// The shell's rectangle, not one of this module's own. A second rectangle type
// in the same binary would be a second answer to "is this point inside?" —
// and the answer that matters here is the half-open one `Rect::contains`
// documents, which is what lets two adjacent day cells share an edge without
// both claiming the pixel on it.
use crate::Rect;

// The eight `Color` constants that used to live here are gone; every colour
// below is a role read from the [`Palette`] the renderer is handed. See
// known-issues.md
// `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
//
// Two of them did not survive the move unchanged, because reading a role is
// only half the job — the *right* role still has to be chosen:
//
// - Adjacent-month day numbers were `SURFACE2`, which is a **fill** role being
//   used as an **ink**. Surfaces sit near the background by construction, so
//   that pairing was 2.46:1 in Mocha and 1.91:1 in Latte — below the 4.5:1
//   floor in *both* modes, i.e. broken in the theme the shell ships. `overlay0`
//   does not rescue it either (3.19 / 2.09). It is now `subtext0` (7.37 /
//   4.64), and the recession those days still want comes from being a rung
//   quieter than `text` rather than from being too faint to read.
// - The event-detail body was `SUBTEXT` on the `SURFACE0` panel: 5.65:1 in
//   Mocha but 3.40:1 in Latte, the usual shape of a bug that only the light
//   render can see. It is now `text`, which is what body copy on a panel is.
//
// The selected day's disc moved `surface1` → `surface0` for the same reason:
// `text` on `surface1` is 4.39:1 in Latte, just under the floor. `surface0` is
// the role *further* from `text` in both modes — darker in Mocha, lighter in
// Latte — so it reads 8.69 / 5.17 and clears the floor on both sides.

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

/// Height of the arrows-and-title row at the top of the navigation header.
///
/// The rest of [`NAV_HEIGHT`] is the "Today" button's row. Splitting the header
/// in two is what lets the arrows and the button have hit boxes that do not
/// overlap; a single 44px band would make the whole header one target.
const NAV_ARROW_HEIGHT: f32 = 28.0;

/// Width of the box around each navigation arrow — its hit target, and the
/// box the glyph is centred in.
///
/// The glyph itself is ~10px wide at 18pt, which is a punishing thing to ask
/// anyone to hit. It is drawn centred in this box rather than at its left
/// edge, so what is clickable is what looks clickable.
const NAV_ARROW_WIDTH: f32 = 24.0;

/// Slack around the "Today" label, so the button can be hit either side of the
/// word rather than only on the letters.
const TODAY_BUTTON_PADDING: f32 = 8.0;

/// What the jump-to-this-month button says. Named because the layout measures
/// it and the renderer draws it, and a button sized for one string and
/// labelled with another is a button that is clickable off its own edge.
const TODAY_LABEL: &str = "Today";

/// Height of the day-of-week header row (S M T W T F S).
const DOW_HEADER_HEIGHT: f32 = 28.0;

/// Columns in the month grid — one per day of the week.
const GRID_COLS: usize = 7;

/// Rows in the month grid. Always six, so the popup does not change height
/// between a month that spans five weeks and one that spans six.
const GRID_ROWS: usize = 6;

/// Cells in the month grid.
const GRID_CELLS: usize = GRID_COLS * GRID_ROWS;

/// Mini months per row of the year view.
const YEAR_COLS: usize = 4;

/// Rows of mini months in the year view.
const YEAR_ROWS: usize = 3;

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

/// Height of the detail popup's own "Aug 21" header.
const EVENT_HEADER_HEIGHT: f32 = 28.0;

/// Gap between the calendar card and the event-detail card below it.
const DETAIL_GAP: f32 = 4.0;

/// Maximum events shown in the detail popup before scrolling.
const MAX_VISIBLE_EVENTS: usize = 6;

/// Mini year-view cell size.
const MINI_CELL: f32 = 12.0;

/// Mini year-view month label height.
const MINI_MONTH_LABEL_HEIGHT: f32 = 18.0;

/// Padding around a mini month's grid, inside its own box.
const MINI_MONTH_PADDING: f32 = 8.0;

/// Gap between neighbouring mini months.
const MINI_MONTH_GAP: f32 = 8.0;

// --- Type sizes -------------------------------------------------------------
//
// Named rather than written at the draw call because the layout has to measure
// some of the same strings the renderer draws — the month title and the "Today"
// button are both centred, and a measurement taken at a different size than the
// drawing produces a control that sits off its own hit box.

/// The `<` and `>` navigation glyphs.
const NAV_ARROW_FONT: f32 = 18.0;
/// "September 2026" across the top of the month view.
const NAV_TITLE_FONT: f32 = 15.0;
/// The "Today" button.
const TODAY_FONT: f32 = 11.0;
/// The S M T W T F S column headings.
const DOW_HEADER_FONT: f32 = 11.0;
/// The day numbers in the grid.
const DAY_FONT: f32 = 13.0;
/// The ISO week numbers in the gutter.
const WEEK_NUM_FONT: f32 = 10.0;
/// The event card's date heading.
const EVENT_HEADER_FONT: f32 = 13.0;
/// An event's start time.
const EVENT_TIME_FONT: f32 = 10.0;
/// An event's title.
const EVENT_TITLE_FONT: f32 = 12.0;
/// The year across the top of the year view.
const YEAR_TITLE_FONT: f32 = 16.0;
/// A mini month's name.
const MINI_LABEL_FONT: f32 = 11.0;
/// A day number inside a mini month.
const MINI_DAY_FONT: f32 = 8.0;

// --- Clock band -------------------------------------------------------------

/// Drop from the clock band's time line to the date beneath it.
const CLOCK_DATE_OFFSET: f32 = 16.0;

/// Drop from the clock band's time line to its first extra-zone row.
const CLOCK_ZONES_OFFSET: f32 = 34.0;

/// Height of one extra-zone row in the clock band.
const CLOCK_ZONE_ROW: f32 = 14.0;

/// The clock band's own type sizes: the time, the long date, an extra zone.
const CLOCK_TIME_FONT: f32 = 13.0;
const CLOCK_DATE_FONT: f32 = 11.0;
const CLOCK_ZONE_FONT: f32 = 10.0;

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
    /// The colour the user chose for this event, if they chose one.
    ///
    /// `None` means "unspecified", which is not the same as any colour and
    /// must not be spelled as one — the same reasoning as design-decisions
    /// §526 for the peek popup's sampled colour. The renderer resolves `None`
    /// to a palette role; see [`Self::dot_color`].
    ///
    /// It is deliberately **not** defaulted to a palette role at parse time.
    /// This field is filled in by [`EventStore::import_text`], and a parser
    /// that consulted the palette would make the same file parse to different
    /// data depending on which theme happened to be active — after which
    /// [`EventStore::export_text`] writes that theme-dependent value back, so
    /// merely opening the calendar in light mode would silently rewrite every
    /// event the user never coloured. A display setting must not be able to
    /// edit user data.
    pub color: Option<Color>,
    pub description: String,
}

impl CalendarEvent {
    /// Duration of this event in seconds.
    pub fn duration_secs(&self) -> u64 {
        self.end_timestamp.saturating_sub(self.start_timestamp)
    }

    /// The colour this event's dot is drawn in, given the palette in force.
    ///
    /// An event the user coloured keeps that colour in both modes — it is
    /// their data, not a theme decision, so it is the one thing the calendar
    /// draws that is deliberately not a palette member. An event they did not
    /// colour gets `lavender`, which is what "an event exists here" has always
    /// looked like.
    #[must_use]
    pub fn dot_color(&self, p: &Palette) -> Color {
        self.color.unwrap_or(p.lavender)
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
            // Only when the user actually chose one. Emitting a `color:` line
            // for an uncoloured event would make the file gain a colour it
            // never had the first time it was saved, and from then on the
            // event would be pinned to whatever the renderer's default happened
            // to be on that day.
            if let Some(c) = event.color {
                out.push_str(&format!("color: {:02X}{:02X}{:02X}\n", c.r, c.g, c.b));
            }
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
        let mut color: Option<Color> = None;
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
                color = None;
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
                // A malformed hex is not a colour either, so it stays
                // `None` rather than silently becoming a default the file
                // would then be rewritten with.
                color = parse_hex_color(val);
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
#[derive(Clone, Debug)]
pub struct ClockDisplay {
    /// Whether to use 24-hour format.
    pub use_24h: bool,
    /// Whether to show seconds.
    pub show_seconds: bool,
    /// Whether the taskbar reading is prefixed with the day of the week.
    pub show_day_of_week: bool,
    /// Whether the taskbar reading includes the calendar date.
    pub show_date: bool,
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
    /// A clock showing nothing but the time of day.
    ///
    /// These are the *widget's* defaults, not the desktop's: what the taskbar
    /// actually ships with lives in `DateTimeSettings::default`, and the shell
    /// copies all four switches out of it on every read. A caller that builds a
    /// clock without a settings panel behind it gets a bare `HH:MM`, which is
    /// the reading that needs no explanation.
    pub fn new() -> Self {
        Self {
            use_24h: true,
            show_seconds: false,
            show_day_of_week: false,
            show_date: false,
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

    /// The weekday the instant falls on in `tz`, written out: "Monday".
    pub fn format_day_of_week(&self, utc_timestamp: u64, tz: &Tz) -> &'static str {
        let (year, month, day, _, _, _) = timestamp_to_date(local_secs(utc_timestamp, tz));
        day_of_week_name(day_of_week(year, month, day))
    }

    /// The calendar date **without** the weekday: "January 1, 2024".
    ///
    /// Split from [`format_date`](Self::format_date) because the Date & Time
    /// panel offers the weekday and the date as two independent switches, and a
    /// single function that emits both joined by a comma can answer neither of
    /// them on its own. The joined form is still what the popup wants, so it
    /// stays — as a composition of this and
    /// [`format_day_of_week`](Self::format_day_of_week), not as a third place
    /// that knows how a date is spelled.
    pub fn format_calendar_date(&self, utc_timestamp: u64, tz: &Tz) -> String {
        let (year, month, day, _, _, _) = timestamp_to_date(local_secs(utc_timestamp, tz));
        format!("{} {day}, {year}", month_name(month))
    }

    /// Format a date string: "DayOfWeek, Month DD, YYYY".
    pub fn format_date(&self, utc_timestamp: u64, tz: &Tz) -> String {
        format!(
            "{}, {}",
            self.format_day_of_week(utc_timestamp, tz),
            self.format_calendar_date(utc_timestamp, tz)
        )
    }

    /// The single-line taskbar reading, as the Date & Time panel has set it up.
    ///
    /// The time is always present — a clock with every switch off is still a
    /// clock — and the weekday and date are prepended when asked for, in the
    /// order a person reads them: `"Thu Aug 21 16:30"`.
    ///
    /// # Why the short names here and the long ones in `format_date`
    ///
    /// The taskbar has about a hundred pixels for this, and the popup has a
    /// whole panel. Writing "Thursday, August 21, 2026" into the corner of the
    /// screen would either overrun the display edge or be clipped, and the year
    /// is the field nobody consults at a glance — so the taskbar drops it and
    /// abbreviates the two names. Both spellings come from the same
    /// `guitk::date` tables, so this is a second *presentation* of one calendar
    /// rather than a second calendar; see design-decisions §492.
    pub fn format_taskbar(&self, utc_timestamp: u64, tz: &Tz) -> String {
        let (year, month, day, _, _, _) = timestamp_to_date(local_secs(utc_timestamp, tz));
        let mut out = String::new();
        if self.show_day_of_week {
            let dow = day_of_week(year, month, day);
            out.push_str(Weekday::from_index(i32::try_from(dow).unwrap_or(0)).short_name());
            out.push(' ');
        }
        if self.show_date {
            out.push_str(month_name_short(month));
            out.push(' ');
            out.push_str(&day.to_string());
            out.push(' ');
        }
        out.push_str(&self.format_time(utc_timestamp, tz));
        out
    }

    /// How much horizontal room to reserve for this clock at `font_size`.
    ///
    /// Measured over the **widest value each field can take**, never over the
    /// current instant. A reserve that followed the current reading would change
    /// width as the minute rolled over — `1` and `8` are not the same width in a
    /// proportional face, nor are `Fri` and `Wed` — and every tray item laid out
    /// to the left of the clock would twitch once a minute. The widest reading
    /// is a fixed string for a given set of switches, so the layout is stable.
    pub fn reading_width(&self, font_size: f32) -> f32 {
        text::width(&self.widest_reading(font_size), font_size)
    }

    /// The widest reading [`format_taskbar`](Self::format_taskbar) can produce.
    ///
    /// Assembled from the widest weekday abbreviation, the widest month
    /// abbreviation and the widest digit rather than from a guess, because
    /// which of those is widest is a property of the font face and changes when
    /// the face does.
    fn widest_reading(&self, font_size: f32) -> String {
        let widest = |cands: &[&'static str]| -> &'static str {
            cands.iter().copied().fold("", |best, c| {
                if text::width(c, font_size) > text::width(best, font_size) {
                    c
                } else {
                    best
                }
            })
        };
        let digit = widest(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);
        // Two of the widest digit is at least as wide as any field the clock
        // prints: days of the month, hours, minutes and seconds are all one or
        // two digits.
        let dd = format!("{digit}{digit}");

        let mut out = String::new();
        if self.show_day_of_week {
            let days: [&'static str; 7] = core::array::from_fn(|i| {
                Weekday::from_index(i32::try_from(i).unwrap_or(0)).short_name()
            });
            out.push_str(widest(&days));
            out.push(' ');
        }
        if self.show_date {
            let months: [&'static str; 12] = core::array::from_fn(|i| {
                month_name_short(u32::try_from(i).unwrap_or(0).saturating_add(1))
            });
            out.push_str(widest(&months));
            out.push(' ');
            out.push_str(&dd);
            out.push(' ');
        }
        out.push_str(&dd);
        out.push(':');
        out.push_str(&dd);
        if self.show_seconds {
            out.push(':');
            out.push_str(&dd);
        }
        if !self.use_24h {
            out.push(' ');
            out.push_str(widest(&["AM", "PM"]));
        }
        out
    }

    /// Render the clock as a stacked band: the time, the long date under it,
    /// and one row per extra timezone.
    ///
    /// Returns render commands positioned at `(x, y)`, laid out at `scale`
    /// (the display scaling — the module's constants are logical pixels; see
    /// `guitk::scaling`). `utc_now` is the current UTC timestamp and `local`
    /// is the machine's zone — the same [`Tz`] the libc's `localtime` and the
    /// shell's `printf '%(…)T'` use, so the band cannot disagree with `date`.
    ///
    /// This is **not** the taskbar reading: the taskbar has one line and draws
    /// [`format_taskbar`](Self::format_taskbar) into it. This band is the head
    /// of the calendar popup, which is the only surface with room for the
    /// extra zones — see design-decisions §493.
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        scale: f32,
        utc_now: u64,
        local: &Tz,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let time_ink = p.text;
        let date_ink = p.subtext0;
        let zone_ink = p.subtext0;

        // Main time.
        let time_str = self.format_time(utc_now, local);
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: time_str,
            color: time_ink,
            font_size: CLOCK_TIME_FONT * scale,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Date below the time.
        let date_str = self.format_date(utc_now, local);
        cmds.push(RenderCommand::Text {
            x,
            y: y + CLOCK_DATE_OFFSET * scale,
            text: date_str,
            color: date_ink,
            font_size: CLOCK_DATE_FONT * scale,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Extra timezones.
        let mut tz_y = y + CLOCK_ZONES_OFFSET * scale;
        for tz in &self.extra_timezones {
            let tz_time = self.format_time(utc_now, &tz.tz);
            let label = format!("{}: {}", tz.label, tz_time);
            cmds.push(RenderCommand::Text {
                x,
                y: tz_y,
                text: label,
                color: zone_ink,
                font_size: CLOCK_ZONE_FONT * scale,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            tz_y += CLOCK_ZONE_ROW * scale;
        }

        cmds
    }

    /// How tall [`render`](Self::render) draws, so a host can reserve the room.
    ///
    /// Depends on how many extra zones there are and on nothing else — in
    /// particular not on the instant, which is why the calendar popup can hold
    /// this on its own state and still read the current second at draw time.
    #[must_use]
    pub fn render_height(&self, scale: f32) -> f32 {
        let rows = self.extra_timezones.len();
        let base = if rows == 0 {
            // Time, then date, then the date line's own height.
            CLOCK_DATE_OFFSET + CLOCK_ZONE_ROW
        } else {
            CLOCK_ZONES_OFFSET + rows as f32 * CLOCK_ZONE_ROW
        };
        base * scale
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

/// The clock band drawn across the head of the popup.
///
/// Held on the view rather than passed to `render`, because the band's *height*
/// moves every row below it — the arrows, the day cells, the "Today" button.
/// A caller that supplied the clock only when drawing would have a popup whose
/// cells were clicked one band away from where they appear, which is precisely
/// the class of bug [`MonthLayout`] exists to make impossible.
///
/// The instant is deliberately **not** here: `render` takes it, so the band
/// reads the current second on every frame rather than the second the popup
/// happened to open on. The band's height does not depend on it.
#[derive(Clone, Debug)]
pub struct ClockHeader {
    /// The clock to read with — the same one the taskbar draws, so the two
    /// readings cannot disagree about the hour.
    pub clock: ClockDisplay,
    /// The zone to read in.
    pub zone: Tz,
}

/// What is under a point on the calendar popup.
///
/// Separated from acting on it for the same reason the shell's own `Hit` is:
/// "where is the pointer" becomes something a test can assert directly, and
/// the press path cannot drift from what was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarHit {
    /// The `<` control — the previous month, or the previous year in the year
    /// overview.
    PrevPage,
    /// The `>` control.
    NextPage,
    /// The month-and-year (or year) title, which switches between the month
    /// grid and the year overview.
    Title,
    /// The "Today" button.
    Today,
    /// A day cell, by index into [`CalendarView::generate_grid`].
    Day(usize),
    /// A mini month of the year overview, 1-12.
    Month(u32),
    /// The popup, but none of its controls. A host uses this to know the click
    /// was *on* the popup and so must not dismiss it.
    Panel,
}

// ============================================================================
// Popup geometry
// ============================================================================

/// Where every part of the month popup sits, at a given origin and scaling.
///
/// This exists because the popup is now clicked as well as drawn. Geometry
/// written twice — once to paint an arrow, once to decide whether a click hit
/// it — yields a control that is live somewhere other than where it appears
/// the moment either copy is edited, and nothing about the surviving copy looks
/// wrong. Every rectangle here is read by `render_month_view` *and* by
/// [`CalendarView::hit_test`], so the two cannot disagree.
///
/// `scale` is the display scaling the host draws its chrome at. This module's
/// constants are logical pixels (see `guitk::scaling`), and the popup has to
/// live in the same coordinate space as the taskbar it rises from: laid out at
/// 100% beside a taskbar drawn at 200% it would be half-size and anchored to
/// the wrong pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonthLayout {
    /// The popup card.
    pub frame: Rect,
    /// Display scaling. Every constant in this module is multiplied by it.
    scale: f32,
    /// Top of the clock band, and its height — zero when there is no band.
    clock_y: f32,
    clock_h: f32,
    /// Left edge of the seven-column day grid, past the week-number gutter.
    grid_x: f32,
    /// Top of the arrows-and-title row.
    nav_y: f32,
    /// Top of the S M T W T F S row.
    dow_y: f32,
    /// Top of the first row of day cells.
    grid_y: f32,
    /// Width of the week-number gutter, or zero when the column is off.
    gutter: f32,
    /// Whether the "Today" button is drawn. It is not while the view is
    /// already on today's month, where it would do nothing.
    today_shown: bool,
}

impl MonthLayout {
    /// Lay the month popup out with its top-left corner at `(x, y)`.
    #[must_use]
    pub fn new(view: &CalendarView, x: f32, y: f32, scale: f32) -> Self {
        let px = |v: f32| v * scale;
        let gutter = if view.config.show_week_numbers {
            px(WEEK_NUM_WIDTH)
        } else {
            0.0
        };
        let clock_h = view
            .header
            .as_ref()
            .map_or(0.0, |h| h.clock.render_height(scale));
        let clock_y = y + px(PADDING);
        let nav_y = clock_y + clock_h;
        let dow_y = nav_y + px(NAV_HEIGHT);
        let grid_y = dow_y + px(DOW_HEADER_HEIGHT);
        let height = px(PADDING) * 2.0
            + clock_h
            + px(NAV_HEIGHT)
            + px(DOW_HEADER_HEIGHT)
            + GRID_ROWS as f32 * px(CELL_SIZE);
        Self {
            frame: Rect::new(x, y, px(POPUP_WIDTH) + gutter, height),
            scale,
            clock_y,
            clock_h,
            grid_x: x + px(PADDING) + gutter,
            nav_y,
            dow_y,
            grid_y,
            gutter,
            today_shown: !view.is_viewing_today_month(),
        }
    }

    /// A logical length in drawn pixels.
    fn px(&self, logical: f32) -> f32 {
        logical * self.scale
    }

    /// Width of the seven-column day grid.
    #[must_use]
    pub fn grid_width(&self) -> f32 {
        GRID_COLS as f32 * self.px(CELL_SIZE)
    }

    /// Where the clock band is drawn, and whether there is one.
    #[must_use]
    pub fn clock_band(&self) -> Option<Rect> {
        if self.clock_h <= 0.0 {
            return None;
        }
        Some(Rect::new(
            self.frame.x + self.px(PADDING),
            self.clock_y,
            (self.frame.w - self.px(PADDING) * 2.0).max(0.0),
            self.clock_h,
        ))
    }

    /// The `<` control.
    #[must_use]
    pub fn prev_arrow(&self) -> Rect {
        Rect::new(
            self.grid_x,
            self.nav_y,
            self.px(NAV_ARROW_WIDTH),
            self.px(NAV_ARROW_HEIGHT),
        )
    }

    /// The `>` control.
    #[must_use]
    pub fn next_arrow(&self) -> Rect {
        let w = self.px(NAV_ARROW_WIDTH);
        Rect::new(
            self.grid_x + self.grid_width() - w,
            self.nav_y,
            w,
            self.px(NAV_ARROW_HEIGHT),
        )
    }

    /// The month-and-year title, filling the space between the two arrows.
    #[must_use]
    pub fn title(&self) -> Rect {
        let arrow = self.px(NAV_ARROW_WIDTH);
        Rect::new(
            self.grid_x + arrow,
            self.nav_y,
            (self.grid_width() - arrow * 2.0).max(0.0),
            self.px(NAV_ARROW_HEIGHT),
        )
    }

    /// The "Today" button, or `None` while the view is already on this month.
    #[must_use]
    pub fn today_button(&self) -> Option<Rect> {
        if !self.today_shown {
            return None;
        }
        let pad = self.px(TODAY_BUTTON_PADDING);
        let w =
            text::measure(TODAY_LABEL, self.px(TODAY_FONT), FontWeightHint::Regular) + pad * 2.0;
        let centre = self.grid_x + self.grid_width() / 2.0;
        Some(Rect::new(
            centre - w / 2.0,
            self.nav_y + self.px(NAV_ARROW_HEIGHT),
            w,
            (self.px(NAV_HEIGHT) - self.px(NAV_ARROW_HEIGHT)).max(0.0),
        ))
    }

    /// The day-of-week heading above column `col`.
    #[must_use]
    pub fn dow_header(&self, col: usize) -> Rect {
        let cell = self.px(CELL_SIZE);
        Rect::new(
            self.grid_x + col as f32 * cell,
            self.dow_y,
            cell,
            self.px(DOW_HEADER_HEIGHT),
        )
    }

    /// The week-number gutter cell beside grid row `row`.
    #[must_use]
    pub fn week_number(&self, row: usize) -> Rect {
        let cell = self.px(CELL_SIZE);
        Rect::new(
            self.frame.x + self.px(PADDING),
            self.grid_y + row as f32 * cell,
            self.gutter,
            cell,
        )
    }

    /// The cell at `index` into the 42-cell grid.
    #[must_use]
    pub fn cell(&self, index: usize) -> Rect {
        let size = self.px(CELL_SIZE);
        let col = index % GRID_COLS;
        let row = index / GRID_COLS;
        Rect::new(
            self.grid_x + col as f32 * size,
            self.grid_y + row as f32 * size,
            size,
            size,
        )
    }

    /// Which grid cell a point falls in, if any.
    ///
    /// A scan over [`cell`](Self::cell) rather than arithmetic that inverts it:
    /// dividing the offset by the cell size would be a *second* answer to where
    /// the cells are, and forty-two `contains` calls on a click is not a cost
    /// worth trading correctness for.
    #[must_use]
    pub fn cell_at(&self, px: f32, py: f32) -> Option<usize> {
        (0..GRID_CELLS).find(|&i| self.cell(i).contains(px, py))
    }

    /// The event card that hangs below the popup for a selected date.
    #[must_use]
    pub fn detail(&self, event_count: usize) -> Rect {
        let visible = event_count.min(MAX_VISIBLE_EVENTS);
        let height = self.px(EVENT_HEADER_HEIGHT)
            + visible as f32 * self.px(EVENT_ROW_HEIGHT)
            + self.px(PADDING);
        Rect::new(
            self.frame.x,
            self.frame.y + self.frame.h + self.px(DETAIL_GAP),
            self.frame.w,
            height,
        )
    }
}

/// Where every part of the year overview sits.
///
/// The month view's counterpart, and here for the same reason: the year's
/// arrows and its twelve mini months are all clickable, and their rectangles
/// must be the ones that were drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YearLayout {
    /// The popup card.
    pub frame: Rect,
    scale: f32,
    /// Top of the first row of mini months.
    grid_y: f32,
    /// Size of one mini month.
    mini_w: f32,
    mini_h: f32,
}

impl YearLayout {
    /// Lay the year overview out with its top-left corner at `(x, y)`.
    #[must_use]
    pub fn new(x: f32, y: f32, scale: f32) -> Self {
        let px = |v: f32| v * scale;
        let mini_w = GRID_COLS as f32 * px(MINI_CELL) + px(MINI_MONTH_PADDING);
        let mini_h =
            GRID_ROWS as f32 * px(MINI_CELL) + px(MINI_MONTH_LABEL_HEIGHT) + px(MINI_MONTH_PADDING);
        let width = YEAR_COLS as f32 * mini_w
            + px(PADDING) * 2.0
            + (YEAR_COLS - 1) as f32 * px(MINI_MONTH_GAP);
        let height = px(NAV_HEIGHT)
            + YEAR_ROWS as f32 * mini_h
            + px(PADDING) * 2.0
            + (YEAR_ROWS - 1) as f32 * px(MINI_MONTH_GAP);
        Self {
            frame: Rect::new(x, y, width, height),
            scale,
            grid_y: y + px(PADDING) + px(NAV_HEIGHT),
            mini_w,
            mini_h,
        }
    }

    fn px(&self, logical: f32) -> f32 {
        logical * self.scale
    }

    /// Top of the arrows-and-title row.
    fn nav_y(&self) -> f32 {
        self.frame.y + self.px(PADDING)
    }

    /// The `<` control — the previous year.
    #[must_use]
    pub fn prev_arrow(&self) -> Rect {
        Rect::new(
            self.frame.x + self.px(PADDING),
            self.nav_y(),
            self.px(NAV_ARROW_WIDTH),
            self.px(NAV_ARROW_HEIGHT),
        )
    }

    /// The `>` control — the next year.
    #[must_use]
    pub fn next_arrow(&self) -> Rect {
        let w = self.px(NAV_ARROW_WIDTH);
        Rect::new(
            self.frame.x + self.frame.w - self.px(PADDING) - w,
            self.nav_y(),
            w,
            self.px(NAV_ARROW_HEIGHT),
        )
    }

    /// The year title, which returns to the month grid.
    #[must_use]
    pub fn title(&self) -> Rect {
        let edge = self.px(PADDING) + self.px(NAV_ARROW_WIDTH);
        Rect::new(
            self.frame.x + edge,
            self.nav_y(),
            (self.frame.w - edge * 2.0).max(0.0),
            self.px(NAV_ARROW_HEIGHT),
        )
    }

    /// The box of the `index`-th mini month, `index` counting from January.
    #[must_use]
    pub fn month(&self, index: usize) -> Rect {
        let col = index % YEAR_COLS;
        let row = index / YEAR_COLS;
        Rect::new(
            self.frame.x + self.px(PADDING) + col as f32 * (self.mini_w + self.px(MINI_MONTH_GAP)),
            self.grid_y + row as f32 * (self.mini_h + self.px(MINI_MONTH_GAP)),
            self.mini_w,
            self.mini_h,
        )
    }

    /// Which month (1-12) a point falls on, if any.
    #[must_use]
    pub fn month_at(&self, px: f32, py: f32) -> Option<u32> {
        (0..12usize)
            .find(|&i| self.month(i).contains(px, py))
            .map(|i| u32::try_from(i).unwrap_or(0).saturating_add(1))
    }
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
    /// The clock band across the head of the popup, if the host wants one.
    ///
    /// `None` is a bare month grid. See [`ClockHeader`] for why it lives here
    /// rather than being passed to `render`.
    pub header: Option<ClockHeader>,
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
            header: None,
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
    ///
    /// Opening rewinds it: month view, today's month, nothing selected. A popup
    /// that reopened where it was last left would show September to someone who
    /// closed it there in August and has no idea it moved — and would hang the
    /// event card of a date they have long since stopped looking at underneath
    /// it. This is the same reasoning as the start menu's scroll rewind.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if visible {
            self.mode = CalendarViewMode::Month;
            self.view_year = self.today.0;
            self.view_month = self.today.1;
            self.selected_date = None;
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

    /// Whether the view is already showing the month today falls in.
    ///
    /// Read by the layout (to decide whether the "Today" button exists at all)
    /// and by the renderer (to decide whether to draw it). One answer, so the
    /// button cannot be clickable while invisible or the reverse.
    #[must_use]
    pub fn is_viewing_today_month(&self) -> bool {
        self.view_year == self.today.0 && self.view_month == self.today.1
    }

    /// Set today from a UTC instant and a **zone**.
    ///
    /// Prefer this to [`set_today_from_timestamp`](Self::set_today_from_timestamp),
    /// which takes a fixed offset and so cannot be right across a daylight-saving
    /// transition: a zone that observes DST has two offsets and a rule saying
    /// which is in force at a given instant. This applies the same `local_secs`
    /// the taskbar clock does, so the popup cannot open on a different day than
    /// the reading that opened it.
    pub fn set_today_from_zone(&mut self, utc_now: u64, tz: &Tz) {
        let (y, m, d, _, _, _) = timestamp_to_date(local_secs(utc_now, tz));
        self.set_today(y, m, d);
    }

    // ========================================================================
    // Hit testing
    // ========================================================================

    /// The popup's outline, for a host deciding where to put it.
    #[must_use]
    pub fn popup_rect(&self, x: f32, y: f32, scale: f32) -> Rect {
        match self.mode {
            CalendarViewMode::Month => MonthLayout::new(self, x, y, scale).frame,
            CalendarViewMode::Year => YearLayout::new(x, y, scale).frame,
        }
    }

    /// What is under a point, given where the popup was drawn.
    ///
    /// `None` means the point is not on the popup at all — which is how a host
    /// tells that a click landed outside and should dismiss it. Note that this
    /// is *not* the same as [`CalendarHit::Panel`], which is a click that hit
    /// the popup but none of its controls, and must **not** dismiss it.
    #[must_use]
    pub fn hit_test(
        &self,
        x: f32,
        y: f32,
        scale: f32,
        px: f32,
        py: f32,
        store: &EventStore,
    ) -> Option<CalendarHit> {
        if !self.visible {
            return None;
        }
        match self.mode {
            CalendarViewMode::Month => {
                self.hit_test_month(&MonthLayout::new(self, x, y, scale), px, py, store)
            }
            CalendarViewMode::Year => Self::hit_test_year(&YearLayout::new(x, y, scale), px, py),
        }
    }

    fn hit_test_month(
        &self,
        layout: &MonthLayout,
        px: f32,
        py: f32,
        store: &EventStore,
    ) -> Option<CalendarHit> {
        // The event card is tested first because it is drawn last and hangs
        // *below* the frame: a point inside it is not inside `frame`, and
        // falling through to `None` would dismiss the popup on a click aimed at
        // the very list the click before it opened.
        if let Some(card) = self.detail_rect(layout, store)
            && card.contains(px, py)
        {
            return Some(CalendarHit::Panel);
        }
        if !layout.frame.contains(px, py) {
            return None;
        }
        // The arrows sit inside the title's row and so are tested before it.
        if layout.prev_arrow().contains(px, py) {
            return Some(CalendarHit::PrevPage);
        }
        if layout.next_arrow().contains(px, py) {
            return Some(CalendarHit::NextPage);
        }
        if layout.title().contains(px, py) {
            return Some(CalendarHit::Title);
        }
        if layout.today_button().is_some_and(|r| r.contains(px, py)) {
            return Some(CalendarHit::Today);
        }
        if let Some(index) = layout.cell_at(px, py) {
            return Some(CalendarHit::Day(index));
        }
        Some(CalendarHit::Panel)
    }

    fn hit_test_year(layout: &YearLayout, px: f32, py: f32) -> Option<CalendarHit> {
        if !layout.frame.contains(px, py) {
            return None;
        }
        if layout.prev_arrow().contains(px, py) {
            return Some(CalendarHit::PrevPage);
        }
        if layout.next_arrow().contains(px, py) {
            return Some(CalendarHit::NextPage);
        }
        if layout.title().contains(px, py) {
            return Some(CalendarHit::Title);
        }
        if let Some(month) = layout.month_at(px, py) {
            return Some(CalendarHit::Month(month));
        }
        Some(CalendarHit::Panel)
    }

    /// Where the event card is, if one is showing.
    fn detail_rect(&self, layout: &MonthLayout, store: &EventStore) -> Option<Rect> {
        let (year, month, day) = self.selected_date?;
        let count = store.events_for_date(year, month, day).len();
        if count == 0 {
            return None;
        }
        Some(layout.detail(count))
    }

    /// Act on a hit. Returns whether anything about the view changed.
    pub fn apply(&mut self, hit: CalendarHit) -> bool {
        match hit {
            CalendarHit::PrevPage => {
                match self.mode {
                    CalendarViewMode::Month => self.prev_month(),
                    CalendarViewMode::Year => self.prev_year(),
                }
                true
            }
            CalendarHit::NextPage => {
                match self.mode {
                    CalendarViewMode::Month => self.next_month(),
                    CalendarViewMode::Year => self.next_year(),
                }
                true
            }
            CalendarHit::Title => {
                match self.mode {
                    CalendarViewMode::Month => self.show_year_view(),
                    CalendarViewMode::Year => self.show_month_view(),
                }
                true
            }
            CalendarHit::Today => {
                self.go_to_today();
                true
            }
            CalendarHit::Day(index) => self.select_grid_cell(index),
            CalendarHit::Month(month) => {
                self.view_month = month;
                self.show_month_view();
                true
            }
            CalendarHit::Panel => false,
        }
    }

    /// Select — or deselect — the day in grid cell `index`.
    ///
    /// A cell from a neighbouring month carries the view onto that month as
    /// well. Selecting the trailing "1" of an August grid and staying in August
    /// would leave the highlight on a cell whose date the header contradicts,
    /// and the event card below it listing a September day under an August
    /// title.
    ///
    /// Clicking the selected day again clears it, which is the only way to
    /// dismiss the event card without closing the whole popup.
    fn select_grid_cell(&mut self, index: usize) -> bool {
        let Some(cell) = self.generate_grid().get(index).copied() else {
            return false;
        };
        let date = (cell.year, cell.month, cell.day);
        if self.selected_date == Some(date) {
            self.selected_date = None;
            return true;
        }
        self.selected_date = Some(date);
        if !cell.current_month {
            self.view_year = cell.year;
            self.view_month = cell.month;
        }
        true
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
    /// `scale` is the host's display scaling — see [`MonthLayout`] for why the
    /// popup has to be laid out in the same coordinate space as the chrome it
    /// rises from. `utc_now` feeds the clock band at the head of the popup (and
    /// is unused when there is no band). `store` supplies the event dots and
    /// the card that hangs below a selected date.
    ///
    /// Every rectangle drawn here comes from [`MonthLayout`] or [`YearLayout`],
    /// which is also what [`hit_test`](Self::hit_test) reads — so nothing can be
    /// clickable somewhere other than where it is painted.
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        scale: f32,
        utc_now: u64,
        store: &EventStore,
    ) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        match self.mode {
            CalendarViewMode::Month => self.render_month_view(p, x, y, scale, utc_now, store),
            CalendarViewMode::Year => self.render_year_view(p, x, y, scale),
        }
    }

    fn render_month_view(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        scale: f32,
        utc_now: u64,
        store: &EventStore,
    ) -> Vec<RenderCommand> {
        let layout = MonthLayout::new(self, x, y, scale);
        let frame = layout.frame;
        let radii = CornerRadii::all(layout.px(CARD_RADIUS));
        let mut cmds = Vec::new();
        let popup_bg = p.base;
        let popup_border = p.surface1;

        // Popup background with shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: frame.x,
            y: frame.y,
            width: frame.w,
            height: frame.h,
            offset_x: 0.0,
            offset_y: layout.px(4.0),
            blur: layout.px(16.0),
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: radii,
        });
        cmds.push(RenderCommand::FillRect {
            x: frame.x,
            y: frame.y,
            width: frame.w,
            height: frame.h,
            color: popup_bg,
            corner_radii: radii,
        });
        cmds.push(RenderCommand::StrokeRect {
            x: frame.x,
            y: frame.y,
            width: frame.w,
            height: frame.h,
            color: popup_border,
            line_width: 1.0,
            corner_radii: radii,
        });

        // The clock band, drawn by the very `ClockDisplay` the taskbar reads
        // with — one clock, two presentations, so the popup cannot claim a
        // different hour than the reading that opened it.
        if let (Some(header), Some(band)) = (self.header.as_ref(), layout.clock_band()) {
            cmds.extend(
                header
                    .clock
                    .render(p, band.x, band.y, scale, utc_now, &header.zone),
            );
        }

        self.render_nav_header(p, &mut cmds, &layout);
        self.render_dow_headers(p, &mut cmds, &layout);

        // The grid is 42 cells meaning six weeks of seven. `chunks` states that
        // once, where `row * 7` and `row * 7 + col` stated it at each of the two
        // places that indexed back in.
        let grid = self.generate_grid();
        for (row, week) in grid.chunks(GRID_COLS).enumerate() {
            if self.config.show_week_numbers {
                let gutter = layout.week_number(row);
                cmds.push(RenderCommand::Text {
                    x: gutter.x,
                    y: gutter.y + layout.px(12.0),
                    text: format!("{}", Self::week_number_for(week)),
                    // `surface2` here too, and for the same reason it was
                    // wrong on the adjacent-month days: a fill role read as
                    // ink. The gutter is secondary information, not invisible
                    // information.
                    color: p.subtext0,
                    font_size: layout.px(WEEK_NUM_FONT),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(gutter.w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
        for (index, cell) in grid.iter().enumerate() {
            self.render_day_cell(p, &mut cmds, &layout, index, cell, store);
        }

        // Event card for the selected date.
        if let Some((sy, sm, sd)) = self.selected_date {
            let events = store.events_for_date(sy, sm, sd);
            if !events.is_empty() {
                let card = layout.detail(events.len());
                Self::render_event_detail(p, &mut cmds, &layout, card, sm, sd, &events);
            }
        }

        cmds
    }

    fn render_nav_header(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        layout: &MonthLayout,
    ) {
        let arrow_size = layout.px(NAV_ARROW_FONT);
        let arrow_ink = p.subtext0;
        let month_ink = p.text;
        let today_ink = p.accent;
        // Each glyph is centred in its own hit box, so what is clickable is
        // what looks clickable. Drawing at the box's left edge — which is what
        // this did before the boxes existed — put a 10px glyph at one end of a
        // 24px target and left the other end looking like empty header.
        for (rect, glyph) in [(layout.prev_arrow(), "<"), (layout.next_arrow(), ">")] {
            cmds.push(RenderCommand::Text {
                x: text::center_x(
                    glyph,
                    rect.x + rect.w / 2.0,
                    arrow_size,
                    FontWeightHint::Bold,
                ),
                y: rect.y + layout.px(10.0),
                text: glyph.to_string(),
                color: arrow_ink,
                font_size: arrow_size,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Centred month/year label. Month names are localised, so an
        // eight-pixels-per-byte guess put "Februar" and "Fevereiro" visibly
        // off-centre and pushed the longest ones under the > arrow.
        let title = layout.title();
        let title_size = layout.px(NAV_TITLE_FONT);
        let label = format!("{} {}", month_name(self.view_month), self.view_year);
        cmds.push(RenderCommand::Text {
            x: text::center_x(
                &label,
                title.x + title.w / 2.0,
                title_size,
                FontWeightHint::Bold,
            ),
            y: title.y + layout.px(10.0),
            text: label,
            color: month_ink,
            font_size: title_size,
            font_weight: FontWeightHint::Bold,
            max_width: Some(title.w),
            overflow: TextOverflow::Ellipsis,
        });

        // "Today" button below the label — absent while the view is already on
        // this month, which is the layout's decision, not a second one here.
        if let Some(button) = layout.today_button() {
            let size = layout.px(TODAY_FONT);
            cmds.push(RenderCommand::Text {
                x: text::center_x(
                    TODAY_LABEL,
                    button.x + button.w / 2.0,
                    size,
                    FontWeightHint::Regular,
                ),
                y: button.y + layout.px(2.0),
                text: TODAY_LABEL.to_string(),
                // A control the user can act on, so it wears the accent —
                // the calendar has no branding to protect, which is the
                // other half of design-decisions §527.
                color: today_ink,
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_dow_headers(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        layout: &MonthLayout,
    ) {
        let headers = dow_headers(self.config.first_day_of_week);
        let size = layout.px(DOW_HEADER_FONT);
        let dow_ink = p.subtext0;
        for (col, hdr) in headers.iter().enumerate() {
            let rect = layout.dow_header(col);
            cmds.push(RenderCommand::Text {
                x: text::center_x(hdr, rect.x + rect.w / 2.0, size, FontWeightHint::Bold),
                y: rect.y + layout.px(6.0),
                text: (*hdr).to_string(),
                color: dow_ink,
                font_size: size,
                font_weight: FontWeightHint::Bold,
                max_width: Some(rect.w),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_day_cell(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        layout: &MonthLayout,
        index: usize,
        cell: &GridCell,
        store: &EventStore,
    ) {
        let rect = layout.cell(index);
        let is_today =
            cell.year == self.today.0 && cell.month == self.today.1 && cell.day == self.today.2;
        let is_selected = self.selected_date == Some((cell.year, cell.month, cell.day));

        // Today's highlight, or the selection's.
        // Today wears the accent because it is the one cell the user is
        // being pointed at; the selection is a surface, because it marks where
        // they clicked rather than what matters.
        let today_disc = p.accent;
        let selected_disc = p.surface0;
        let disc = if is_today {
            Some(today_disc)
        } else if is_selected {
            Some(selected_disc)
        } else {
            None
        };
        if let Some(color) = disc {
            let radius = layout.px(TODAY_RADIUS);
            cmds.push(RenderCommand::FillRect {
                x: rect.x + (rect.w - radius * 2.0) / 2.0,
                y: rect.y + (rect.h - radius * 2.0) / 2.0 - layout.px(2.0),
                width: radius * 2.0,
                height: radius * 2.0,
                color,
                corner_radii: CornerRadii::all(radius),
            });
        }

        // Day number, centred by measurement. The old `if day >= 10 { 4.0 }`
        // nudge was a guess at the width of one extra digit in a proportional
        // face, so "11" and "28" were centred differently from each other.
        // Today's disc is whatever accent the user chose, so the ink on it
        // is a function of that fill's brightness rather than a role — the
        // same reasoning as the About dialog's wordmark. `subtext0` for the
        // adjacent-month days replaces a `surface2` that was 2.46:1 in Mocha
        // and 1.91:1 in Latte: a fill role read as an ink is unreadable by
        // construction, because surfaces sit near the background.
        let text_color = if is_today {
            readable_on(today_disc)
        } else if cell.current_month {
            p.text
        } else {
            p.subtext0
        };
        let weight = if is_today {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };
        let day_str = format!("{}", cell.day);
        let size = layout.px(DAY_FONT);
        cmds.push(RenderCommand::Text {
            x: text::center_x(&day_str, rect.x + rect.w / 2.0, size, weight),
            y: rect.y + (rect.h - size) / 2.0 - layout.px(2.0),
            text: day_str,
            color: text_color,
            font_size: size,
            font_weight: weight,
            max_width: Some(rect.w),
            overflow: TextOverflow::Ellipsis,
        });

        // Event dot indicator.
        //
        // The dot takes its colour from the first event of the day, which is
        // the whole reason `CalendarEvent::color` exists. It used to be a
        // fixed LAVENDER that never consulted the event at all, so a colour
        // the user set in the calendar file was parsed, stored and written
        // back out faithfully while changing nothing they could see.
        let todays_events = store.events_for_date(cell.year, cell.month, cell.day);
        if let Some(first) = todays_events.first() {
            let dot = layout.px(DOT_RADIUS);
            // An event the user coloured keeps that colour everywhere, even
            // on today's disc: it is their data and the calendar does not get
            // to overrule it. An *uncoloured* event has no such claim, so on
            // the disc it becomes whatever reads against the accent rather
            // than a lavender that may vanish into it.
            let dot_color = match (first.color, is_today) {
                (Some(chosen), _) => chosen,
                (None, true) => readable_on(today_disc),
                (None, false) => p.lavender,
            };
            cmds.push(RenderCommand::FillRect {
                x: rect.x + (rect.w - dot * 2.0) / 2.0,
                y: rect.y + rect.h - dot * 2.0 - layout.px(4.0),
                width: dot * 2.0,
                height: dot * 2.0,
                color: dot_color,
                corner_radii: CornerRadii::all(dot),
            });
        }
    }

    fn render_event_detail(
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        layout: &MonthLayout,
        card: Rect,
        month: u32,
        day: u32,
        events: &[CalendarEvent],
    ) {
        let pad = layout.px(PADDING);
        let row_h = layout.px(EVENT_ROW_HEIGHT);
        let header_h = layout.px(EVENT_HEADER_HEIGHT);
        let visible_count = events.len().min(MAX_VISIBLE_EVENTS);
        // The card was `surface0` with `subtext0` on it, which is 3.40:1 in
        // Latte. No quiet role clears 4.5:1 against `surface0` there —
        // `subtext1`, the next rung up, still only reaches 4.05 — so the
        // card itself had to move. `mantle` is one step *away* from `base`
        // in both modes, which makes the card a shallow well rather than a
        // raised panel, and buys enough separation for two readable ink
        // tiers: 5.14 for the time and 6.57 for the title in Latte.
        let card_bg = p.mantle;
        let header_ink = p.text;
        let time_ink = p.subtext1;
        let title_ink = p.text;
        let more_ink = p.subtext1;

        cmds.push(RenderCommand::FillRect {
            x: card.x,
            y: card.y,
            width: card.w,
            height: card.h,
            color: card_bg,
            corner_radii: CornerRadii::all(layout.px(CARD_RADIUS)),
        });

        // Header: "Month DD".
        cmds.push(RenderCommand::Text {
            x: card.x + pad,
            y: card.y + layout.px(6.0),
            text: format!("{} {day}", month_name_short(month)),
            color: header_ink,
            font_size: layout.px(EVENT_HEADER_FONT),
            font_weight: FontWeightHint::Bold,
            max_width: Some((card.w - pad * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        for (i, event) in events.iter().take(MAX_VISIBLE_EVENTS).enumerate() {
            let ey = card.y + header_h + i as f32 * row_h;

            // Colour bar.
            cmds.push(RenderCommand::FillRect {
                x: card.x + pad,
                y: ey + layout.px(4.0),
                width: layout.px(3.0),
                height: (row_h - layout.px(8.0)).max(0.0),
                color: event.dot_color(p),
                corner_radii: CornerRadii::all(layout.px(1.5)),
            });

            // Time.
            let (_, _, _, h, m, _) = timestamp_to_date(event.start_timestamp);
            cmds.push(RenderCommand::Text {
                x: card.x + pad + layout.px(10.0),
                y: ey + layout.px(6.0),
                text: if event.all_day {
                    "All day".to_string()
                } else {
                    format!("{h:02}:{m:02}")
                },
                color: time_ink,
                font_size: layout.px(EVENT_TIME_FONT),
                font_weight: FontWeightHint::Regular,
                max_width: Some(layout.px(50.0)),
                overflow: TextOverflow::Ellipsis,
            });

            // Title.
            cmds.push(RenderCommand::Text {
                x: card.x + pad + layout.px(65.0),
                y: ey + layout.px(6.0),
                text: event.title.clone(),
                color: title_ink,
                font_size: layout.px(EVENT_TITLE_FONT),
                font_weight: FontWeightHint::Regular,
                max_width: Some((card.w - pad * 2.0 - layout.px(75.0)).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // "N more..." if truncated.
        if events.len() > MAX_VISIBLE_EVENTS {
            let more = events.len().saturating_sub(MAX_VISIBLE_EVENTS);
            cmds.push(RenderCommand::Text {
                x: card.x + pad + layout.px(10.0),
                y: card.y + header_h + visible_count as f32 * row_h,
                text: format!("{more} more..."),
                color: more_ink,
                font_size: layout.px(TODAY_FONT),
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    // ========================================================================
    // Rendering — year view
    // ========================================================================

    fn render_year_view(&self, p: &Palette, x: f32, y: f32, scale: f32) -> Vec<RenderCommand> {
        let layout = YearLayout::new(x, y, scale);
        let frame = layout.frame;
        let mut cmds = Vec::new();

        // The same three roles the month view names, for the same reasons: the
        // card is the surface the popup floats on, the arrows are chrome the
        // eye should skip, and the year is the one thing being read.
        let card_bg = p.base;
        let arrow_ink = p.subtext0;
        let title_ink = p.text;

        // Background.
        cmds.push(RenderCommand::BoxShadow {
            x: frame.x,
            y: frame.y,
            width: frame.w,
            height: frame.h,
            offset_x: 0.0,
            offset_y: layout.px(4.0),
            blur: layout.px(16.0),
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(layout.px(CARD_RADIUS)),
        });
        cmds.push(RenderCommand::FillRect {
            x: frame.x,
            y: frame.y,
            width: frame.w,
            height: frame.h,
            color: card_bg,
            corner_radii: CornerRadii::all(layout.px(CARD_RADIUS)),
        });

        // Year navigation header. Same shape as the month view's: each glyph
        // centred in the box that is actually clickable, so the two views do
        // not disagree about where an arrow is.
        let arrow_size = layout.px(NAV_ARROW_FONT);
        for (rect, glyph) in [(layout.prev_arrow(), "<"), (layout.next_arrow(), ">")] {
            cmds.push(RenderCommand::Text {
                x: text::center_x(
                    glyph,
                    rect.x + rect.w / 2.0,
                    arrow_size,
                    FontWeightHint::Bold,
                ),
                y: rect.y + layout.px(10.0),
                text: glyph.to_string(),
                color: arrow_ink,
                font_size: arrow_size,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        let title = layout.title();
        let title_size = layout.px(YEAR_TITLE_FONT);
        let year_label = format!("{}", self.view_year);
        cmds.push(RenderCommand::Text {
            x: text::center_x(
                &year_label,
                title.x + title.w / 2.0,
                title_size,
                FontWeightHint::Bold,
            ),
            y: title.y + layout.px(10.0),
            text: year_label,
            color: title_ink,
            font_size: title_size,
            font_weight: FontWeightHint::Bold,
            max_width: Some(title.w),
            overflow: TextOverflow::Ellipsis,
        });

        // Render 12 mini months, four to a row. Iterating the months and
        // deriving the cell keeps "there are twelve of them" the fact being
        // stated, rather than a 3x4 loop that happens to produce 1..=12.
        for (cell, month) in (1..=12u32).enumerate() {
            let box_ = layout.month(cell);
            self.render_mini_month(p, &mut cmds, &layout, box_, month);
        }

        cmds
    }

    /// Draw one mini month inside the box `YearLayout::month` assigned it.
    ///
    /// Takes the box rather than deriving it, so the month a click lands on
    /// (`YearLayout::month_at`) is by construction the month drawn there.
    fn render_mini_month(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        layout: &YearLayout,
        box_: Rect,
        month: u32,
    ) {
        let year = self.view_year;
        let (x, y) = (box_.x, box_.y);
        let cell = layout.px(MINI_CELL);
        let is_current = year == self.today.0 && month == self.today.1;
        // "This is the month you are in" is the same claim the day grid makes
        // with its today disc, so it is drawn in the same role — the accent —
        // and not in a fixed blue that would stop agreeing with the disc the
        // moment the user picked a different one.
        let today_disc = p.accent;
        let label_color = if is_current { today_disc } else { p.text };

        // Month label.
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: month_name_short(month).to_string(),
            color: label_color,
            font_size: layout.px(MINI_LABEL_FONT),
            font_weight: FontWeightHint::Bold,
            max_width: Some(GRID_COLS as f32 * cell),
            overflow: TextOverflow::Ellipsis,
        });

        let grid_y = y + layout.px(MINI_MONTH_LABEL_HEIGHT);

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
            let col = pos % GRID_COLS;
            let row = pos / GRID_COLS;
            let cx = x + col as f32 * cell;
            let cell_y = grid_y + row as f32 * cell;

            let is_today = year == self.today.0 && month == self.today.1 && d == self.today.2;

            if is_today {
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cell_y,
                    width: cell,
                    height: cell,
                    color: today_disc,
                    corner_radii: CornerRadii::all(cell / 2.0),
                });
            }

            // On the disc the ink is a function of the disc's brightness, not
            // a role: the accent is the user's colour and can be anything.
            // Off it, these are secondary digits in a thumbnail, so the quiet
            // role — never `surface*`, which sits too near the card.
            let text_color = if is_today {
                readable_on(today_disc)
            } else {
                p.subtext0
            };
            let size = layout.px(MINI_DAY_FONT);
            cmds.push(RenderCommand::Text {
                x: text::center_x(
                    &format!("{d}"),
                    cx + cell / 2.0,
                    size,
                    FontWeightHint::Regular,
                ),
                y: cell_y + layout.px(1.0),
                text: format!("{d}"),
                color: text_color,
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(cell),
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
        p: &Palette,
        clock: &ClockDisplay,
        x: f32,
        y: f32,
        scale: f32,
        utc_now: u64,
        local: &Tz,
    ) -> Vec<RenderCommand> {
        clock.render(p, x, y, scale, utc_now, local)
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

    /// A fixed instant for render tests: 2023-11-14 22:13:20 UTC.
    ///
    /// Rendering takes the current time as a parameter precisely so tests can
    /// pin it; a render test that read the wall clock would produce different
    /// command counts on the day the clock band gained or lost a digit.
    const NOW: u64 = 1_700_000_000;

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
        assert_eq!(date::days_in_month(2023, 1), 31);
        assert_eq!(date::days_in_month(2023, 2), 28);
        assert_eq!(date::days_in_month(2023, 3), 31);
        assert_eq!(date::days_in_month(2023, 4), 30);
        assert_eq!(date::days_in_month(2023, 5), 31);
        assert_eq!(date::days_in_month(2023, 6), 30);
        assert_eq!(date::days_in_month(2023, 7), 31);
        assert_eq!(date::days_in_month(2023, 8), 31);
        assert_eq!(date::days_in_month(2023, 9), 30);
        assert_eq!(date::days_in_month(2023, 10), 31);
        assert_eq!(date::days_in_month(2023, 11), 30);
        assert_eq!(date::days_in_month(2023, 12), 31);
    }

    #[test]
    fn days_in_month_leap_february() {
        assert_eq!(date::days_in_month(2024, 2), 29);
        assert_eq!(date::days_in_month(2000, 2), 29);
        assert_eq!(date::days_in_month(1900, 2), 28);
    }

    #[test]
    fn an_impossible_month_is_clamped_rather_than_being_zero_days_long() {
        // This used to answer 0, which is not a month length any caller could
        // use: the recurrence walk stepped `while day > date::days_in_month(..)`,
        // and a zero there is a loop that never advances. Clamping means every
        // month number names a real month.
        assert_eq!(date::days_in_month(2024, 0), 31, "month 0 reads as January");
        assert_eq!(
            date::days_in_month(2024, 13),
            31,
            "month 13 reads as December"
        );
        assert_eq!(date::days_in_month(2024, u32::MAX), 31);
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
                        date::days_in_month(year, month),
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
            ..ClockDisplay::new()
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
            // No colour: the overwhelming majority of these tests do not care
            // what an event is drawn in, and `None` is now the honest way to
            // say so. It also makes the default path — the palette's own
            // lavender — the one most of them exercise.
            color: None,
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
                color: None,
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
                color: None,
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
                color: None,
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
                color: None,
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
                color: None,
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
                color: None,
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
            ..ClockDisplay::new()
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
            ..ClockDisplay::new()
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
            ..ClockDisplay::new()
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
            ..ClockDisplay::new()
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

    /// The long form is the two halves joined, not a third spelling of a date.
    #[test]
    fn the_long_date_is_its_two_halves_and_nothing_else() {
        let clock = ClockDisplay::new();
        for ts in [0_u64, 1_704_067_200, 1_787_070_645, 4_000_000_000] {
            for zone in [Tz::UTC, tz("EST5EDT,M3.2.0,M11.1.0"), tz("JST-9")] {
                assert_eq!(
                    clock.format_date(ts, &zone),
                    format!(
                        "{}, {}",
                        clock.format_day_of_week(ts, &zone),
                        clock.format_calendar_date(ts, &zone)
                    )
                );
            }
        }
    }

    /// The two switches the Date & Time panel drew and nothing read.
    #[test]
    fn the_taskbar_reading_follows_the_switches() {
        let mut clock = ClockDisplay::new();
        // 2026-08-18 16:30:45 UTC — a Tuesday.
        let ts = 1_787_070_645;

        assert_eq!(clock.format_taskbar(ts, &Tz::UTC), "16:30");
        clock.show_day_of_week = true;
        assert_eq!(clock.format_taskbar(ts, &Tz::UTC), "Tue 16:30");
        clock.show_date = true;
        assert_eq!(clock.format_taskbar(ts, &Tz::UTC), "Tue Aug 18 16:30");
        clock.show_day_of_week = false;
        assert_eq!(clock.format_taskbar(ts, &Tz::UTC), "Aug 18 16:30");
        clock.show_seconds = true;
        assert_eq!(clock.format_taskbar(ts, &Tz::UTC), "Aug 18 16:30:45");
        clock.use_24h = false;
        assert_eq!(clock.format_taskbar(ts, &Tz::UTC), "Aug 18 4:30:45 PM");
    }

    /// The taskbar's date is taken from the same shifted instant as its time.
    #[test]
    fn the_taskbar_reading_crosses_midnight_with_the_zone() {
        let mut clock = ClockDisplay::new();
        clock.show_day_of_week = true;
        clock.show_date = true;
        let ts = 1_787_070_645; // 2026-08-18 16:30:45 UTC, a Tuesday.
        assert_eq!(clock.format_taskbar(ts, &Tz::UTC), "Tue Aug 18 16:30");
        assert_eq!(clock.format_taskbar(ts, &tz("JST-9")), "Wed Aug 19 01:30");
        assert_eq!(
            clock.format_taskbar(ts, &tz("HST10")),
            "Tue Aug 18 06:30",
            "UTC-10 stays on the same day"
        );
    }

    /// The reserved width has to be an upper bound on every reading, or the
    /// clock is clipped at the display edge for part of the year — and nothing
    /// about a clipped clock says which end was cut.
    #[test]
    fn the_reserved_width_covers_every_reading_the_switches_allow() {
        const SIZE: f32 = 13.0;
        let mut clock = ClockDisplay::new();
        for (dow, date, secs, h24) in [
            (false, false, false, true),
            (true, true, false, true),
            (true, true, true, true),
            (true, true, true, false),
            (false, true, false, false),
        ] {
            clock.show_day_of_week = dow;
            clock.show_date = date;
            clock.show_seconds = secs;
            clock.use_24h = h24;
            let reserved = clock.reading_width(SIZE);
            // Three years sampled every 25 hours: every weekday, every month,
            // every day of the month, and every hour.
            for step in 0..1100_u64 {
                let ts = 1_787_070_645 + step * 25 * 3600;
                let reading = clock.format_taskbar(ts, &Tz::UTC);
                assert!(
                    text::width(&reading, SIZE) <= reserved,
                    "{reading:?} exceeds the reserved {reserved}"
                );
            }
        }
    }

    /// The reserve must also not be wildly generous — a slot much wider than
    /// the reading is dead space taken from the window buttons.
    #[test]
    fn the_reserved_width_is_close_to_what_a_reading_actually_needs() {
        const SIZE: f32 = 13.0;
        let mut clock = ClockDisplay::new();
        clock.show_day_of_week = true;
        clock.show_date = true;
        let reserved = clock.reading_width(SIZE);
        let real = text::width("Mon Sep 28 22:38", SIZE);
        assert!(
            reserved <= real * 1.25,
            "reserved {reserved} against a real reading of {real}"
        );
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
                color: Some(Color::from_hex(0xA6E3A1)),
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
                color: Some(Color::from_hex(0xF9E2AF)),
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

        // The colours are the user's, not the theme's, so they have to come
        // back exactly as they went in.
        assert_eq!(events[0].color, Some(Color::from_hex(0xA6E3A1)));
        assert_eq!(events[1].color, Some(Color::from_hex(0xF9E2AF)));

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
        assert_eq!(e.color, Some(Color::from_hex(0x89B4FA)));
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
                color: Some(Color::from_hex(0xF38BA8)),
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
        let cmds = cal.render(&dark(), 0.0, 0.0, 1.0, NOW, &store);
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_month_view_produces_commands() {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);

        let store = EventStore::new();
        let cmds = cal.render(&dark(), 100.0, 100.0, 1.0, NOW, &store);
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
        let cmds = cal.render(&dark(), 0.0, 0.0, 1.0, NOW, &store);
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
        let cmds = cal.render(&dark(), 0.0, 0.0, 1.0, NOW, &store);
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

        let cmds = cal.render(&dark(), 0.0, 0.0, 1.0, NOW, &store);
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
                color: None,
                description: String::new(),
            },
        );

        let cmds = cal.render(&dark(), 0.0, 0.0, 1.0, NOW, &store);
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
        let cmds = clock.render(&dark(), 0.0, 0.0, 1.0, NOW, &Tz::UTC);
        // At minimum: time text + date text.
        assert!(cmds.len() >= 2);
    }

    #[test]
    fn clock_render_with_extra_timezones() {
        let mut clock = ClockDisplay::new();
        assert!(clock.add_timezone("Tokyo", "JST-9"));
        assert!(clock.add_timezone("London", "GMT0BST,M3.5.0/1,M10.5.0"));

        let cmds = clock.render(&dark(), 0.0, 0.0, 1.0, NOW, &Tz::UTC);
        // time + date + 2 timezone lines.
        assert!(cmds.len() >= 4);
    }

    /// The band's reserved height must cover every row it actually draws.
    ///
    /// The height is what pushes the whole month grid down, so a band that
    /// under-reserves does not merely clip itself — it draws its last zone on
    /// top of the "<" arrow, and the arrow stays clickable underneath.
    #[test]
    fn the_clock_band_reserves_the_height_it_draws_in() {
        let mut clock = ClockDisplay::new();
        for zones in [
            &[][..],
            &[("Tokyo", "JST-9")],
            &[("Tokyo", "JST-9"), ("London", "GMT0BST,M3.5.0/1,M10.5.0")],
            &[
                ("Tokyo", "JST-9"),
                ("London", "GMT0BST,M3.5.0/1,M10.5.0"),
                ("Denver", "MST7MDT,M3.2.0,M11.1.0"),
            ],
        ] {
            clock.extra_timezones.clear();
            for (label, tz) in zones {
                assert!(clock.add_timezone(label, tz));
            }
            for scale in [1.0_f32, 1.5, 2.0] {
                let top = 40.0_f32;
                let height = clock.render_height(scale);
                for cmd in clock.render(&dark(), 0.0, top, scale, NOW, &Tz::UTC) {
                    let RenderCommand::Text { y, font_size, .. } = cmd else {
                        continue;
                    };
                    assert!(
                        y + font_size <= top + height,
                        "{} zone(s) at {scale}x draw down to {} but reserve only {height}",
                        zones.len(),
                        y + font_size - top
                    );
                }
            }
        }
    }

    // ========================================================================
    // Layout and hit testing
    //
    // The point of `MonthLayout`/`YearLayout` is that the renderer and the
    // hit test read the *same* rectangles, so these tests are mostly about
    // that agreement rather than about any particular coordinate.
    // ========================================================================

    /// The palette the older smoke tests render in.
    ///
    /// Mocha, because it is the shipped default. It is deliberately *not*
    /// the palette the colour tests below use: a conversion checked only in
    /// the palette it was converted from hides every failure it causes.
    fn dark() -> Palette {
        Palette::for_mode(false)
    }

    /// A month view sitting at the origin, showing May 2026 with today in it.
    fn open_month() -> CalendarView {
        let mut cal = CalendarView::new(CalendarConfig::default());
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);
        cal
    }

    fn centre(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The selection disc is drawn on the cell the click that made it hits.
    ///
    /// This is the whole reason the layout exists. It closes the loop through
    /// the real render output: select a day, find the disc the renderer drew,
    /// click its centre, and insist the hit test names a cell holding exactly
    /// the date that was selected. When the two derived their geometry
    /// separately, this is the test that would have caught them drifting.
    #[test]
    fn the_selection_disc_is_drawn_on_the_cell_that_is_clicked() {
        let mut cal = open_month();
        let store = EventStore::new();
        cal.selected_date = Some((2026, 5, 7));

        let disc_size = TODAY_RADIUS * 2.0;
        let disc = cal
            .render(&dark(), 0.0, 0.0, 1.0, NOW, &store)
            .into_iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if color == dark().surface0
                    && (width - disc_size).abs() < 0.01
                    && (height - disc_size).abs() < 0.01 =>
                {
                    Some(Rect::new(x, y, width, height))
                }
                _ => None,
            })
            .expect("a selected day draws a disc");

        let (cx, cy) = centre(disc);
        let Some(CalendarHit::Day(index)) = cal.hit_test(0.0, 0.0, 1.0, cx, cy, &store) else {
            panic!("the centre of the selection disc is not on a day cell");
        };
        let cell = cal.generate_grid()[index];
        assert_eq!((cell.year, cell.month, cell.day), (2026, 5, 7));
    }

    /// Each nav control answers at its own centre, and no two overlap there.
    #[test]
    fn every_nav_control_answers_at_its_own_centre() {
        // Off today's month, so the "Today" button exists.
        let mut cal = open_month();
        cal.view_month = 9;
        let store = EventStore::new();
        let layout = MonthLayout::new(&cal, 0.0, 0.0, 1.0);

        for (rect, want) in [
            (layout.prev_arrow(), CalendarHit::PrevPage),
            (layout.next_arrow(), CalendarHit::NextPage),
            (layout.title(), CalendarHit::Title),
            (
                layout.today_button().expect("off-month shows Today"),
                CalendarHit::Today,
            ),
        ] {
            let (px, py) = centre(rect);
            assert_eq!(
                cal.hit_test(0.0, 0.0, 1.0, px, py, &store),
                Some(want),
                "{want:?} is not clickable at the centre of its own box"
            );
        }
    }

    /// Every one of the 42 cells is hit at its own centre, and at its index.
    #[test]
    fn every_grid_cell_answers_at_its_own_index() {
        let cal = open_month();
        let store = EventStore::new();
        let layout = MonthLayout::new(&cal, 17.0, 23.0, 1.0);
        for index in 0..GRID_CELLS {
            let (px, py) = centre(layout.cell(index));
            assert_eq!(
                cal.hit_test(17.0, 23.0, 1.0, px, py, &store),
                Some(CalendarHit::Day(index)),
                "cell {index} is not clickable where it is drawn"
            );
        }
    }

    /// Outside is `None` (dismiss); inside-but-inert is `Panel` (do not).
    ///
    /// Collapsing the two would make the popup close on a click in its own
    /// margin, which is the single most irritating way for a popup to behave.
    #[test]
    fn dead_space_inside_the_popup_is_not_a_dismissal() {
        let cal = open_month();
        let store = EventStore::new();
        let layout = MonthLayout::new(&cal, 0.0, 0.0, 1.0);
        let frame = layout.frame;

        // The strip between the frame's left edge and the padded grid.
        assert_eq!(
            cal.hit_test(0.0, 0.0, 1.0, frame.x + 1.0, layout.grid_y + 1.0, &store),
            Some(CalendarHit::Panel)
        );
        // A pixel past the right edge is off the popup entirely.
        assert_eq!(
            cal.hit_test(0.0, 0.0, 1.0, frame.x + frame.w, frame.y + 1.0, &store),
            None
        );
        assert_eq!(cal.hit_test(0.0, 0.0, 1.0, -1.0, -1.0, &store), None);
    }

    /// The event card hangs below the frame and still belongs to the popup.
    ///
    /// It is drawn outside `frame`, so a naive "is it in the frame?" test
    /// dismisses the popup when the user clicks the very list they just opened.
    #[test]
    fn the_event_card_hangs_below_the_frame_and_still_counts_as_the_popup() {
        let mut cal = open_month();
        let mut store = EventStore::new();
        let start = date_to_timestamp(2026, 5, 18, 10, 0, 0).expect("valid");
        add(&mut store, make_event("Standup", start, start + 3600));
        cal.selected_date = Some((2026, 5, 18));

        let layout = MonthLayout::new(&cal, 0.0, 0.0, 1.0);
        let card = cal
            .detail_rect(&layout, &store)
            .expect("a selected day with events shows a card");
        assert!(
            card.y >= layout.frame.y + layout.frame.h,
            "the card overlaps the grid it describes"
        );

        let (px, py) = centre(card);
        assert_eq!(
            cal.hit_test(0.0, 0.0, 1.0, px, py, &store),
            Some(CalendarHit::Panel)
        );

        // With nothing selected there is no card, and that same point is off
        // the popup — so the host dismisses rather than swallowing the click.
        cal.selected_date = None;
        assert_eq!(cal.hit_test(0.0, 0.0, 1.0, px, py, &store), None);
    }

    /// The popup is measured in the shell's pixels, not its own.
    ///
    /// `guitk::scaling` has widget code work in logical pixels, but this popup
    /// is anchored to taskbar chrome the shell has *already* scaled. Laid out
    /// at 100% beside a 200% taskbar it would be half-size and hung off the
    /// wrong pixel, so the scale is threaded in explicitly.
    #[test]
    fn the_popup_is_measured_in_the_shells_pixels() {
        let cal = open_month();
        let store = EventStore::new();
        let one = cal.popup_rect(0.0, 0.0, 1.0);
        let two = cal.popup_rect(0.0, 0.0, 2.0);
        assert!((two.w - one.w * 2.0).abs() < 0.01, "{two:?} vs {one:?}");
        assert!((two.h - one.h * 2.0).abs() < 0.01, "{two:?} vs {one:?}");

        // And a point that hits a cell at 1x hits the same cell at 2x when it
        // is doubled with the layout.
        for index in [0_usize, 15, 41] {
            let (px, py) = centre(MonthLayout::new(&cal, 0.0, 0.0, 1.0).cell(index));
            assert_eq!(
                cal.hit_test(0.0, 0.0, 2.0, px * 2.0, py * 2.0, &store),
                Some(CalendarHit::Day(index))
            );
        }
    }

    /// The clock band pushes everything below it down by exactly its height.
    #[test]
    fn the_clock_band_moves_the_grid_down_by_its_own_height() {
        let mut cal = open_month();
        let bare = MonthLayout::new(&cal, 0.0, 0.0, 1.0);

        let mut clock = ClockDisplay::new();
        assert!(clock.add_timezone("Tokyo", "JST-9"));
        let band_h = clock.render_height(1.0);
        cal.header = Some(ClockHeader {
            clock,
            zone: Tz::UTC,
        });
        let with = MonthLayout::new(&cal, 0.0, 0.0, 1.0);

        assert!((with.frame.h - bare.frame.h - band_h).abs() < 0.01);
        assert!((with.cell(0).y - bare.cell(0).y - band_h).abs() < 0.01);
        assert_eq!(with.clock_band().map(|b| b.h), Some(band_h));
        assert_eq!(bare.clock_band(), None);
    }

    /// Clicking the selected day again clears it.
    ///
    /// It is the only way to dismiss the event card without closing the popup.
    #[test]
    fn clicking_the_selected_day_again_clears_the_selection() {
        let mut cal = open_month();
        let index = cal
            .generate_grid()
            .iter()
            .position(|c| (c.year, c.month, c.day) == (2026, 5, 18))
            .expect("today is in its own month grid");

        assert!(cal.apply(CalendarHit::Day(index)));
        assert_eq!(cal.selected_date, Some((2026, 5, 18)));
        assert!(cal.apply(CalendarHit::Day(index)));
        assert_eq!(cal.selected_date, None);
    }

    /// A spill-over cell carries the view onto the month it belongs to.
    ///
    /// Otherwise the highlight sits on a cell whose date the header above it
    /// contradicts, and the card below lists a June day under a May title.
    #[test]
    fn a_spill_over_cell_carries_the_view_onto_its_month() {
        let mut cal = open_month();
        let grid = cal.generate_grid();
        let index = grid
            .iter()
            .rposition(|c| !c.current_month)
            .expect("a 42-cell grid always spills past its month");
        let cell = grid[index];

        assert!(cal.apply(CalendarHit::Day(index)));
        assert_eq!(cal.selected_date, Some((cell.year, cell.month, cell.day)));
        assert_eq!((cal.view_year, cal.view_month), (cell.year, cell.month));
    }

    /// The "Today" button is neither drawn nor clickable on today's month.
    #[test]
    fn the_today_button_is_absent_while_the_view_is_already_on_today() {
        let cal = open_month();
        let store = EventStore::new();
        let layout = MonthLayout::new(&cal, 0.0, 0.0, 1.0);
        assert_eq!(layout.today_button(), None);
        assert!(
            !cal.render(&dark(), 0.0, 0.0, 1.0, NOW, &store)
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == TODAY_LABEL))
        );

        // Move off it and the button appears in both.
        let mut off = open_month();
        off.next_month();
        let layout = MonthLayout::new(&off, 0.0, 0.0, 1.0);
        let button = layout.today_button().expect("off-month shows Today");
        let (px, py) = centre(button);
        assert_eq!(
            off.hit_test(0.0, 0.0, 1.0, px, py, &store),
            Some(CalendarHit::Today)
        );
        assert!(
            off.render(&dark(), 0.0, 0.0, 1.0, NOW, &store)
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == TODAY_LABEL))
        );
    }

    /// A hidden calendar is not clickable, however the host places it.
    #[test]
    fn a_hidden_calendar_is_not_clickable() {
        let mut cal = open_month();
        cal.set_visible(false);
        let store = EventStore::new();
        let layout = MonthLayout::new(&cal, 0.0, 0.0, 1.0);
        let (px, py) = centre(layout.cell(0));
        assert_eq!(cal.hit_test(0.0, 0.0, 1.0, px, py, &store), None);
    }

    /// Reopening rewinds to this month with nothing selected.
    ///
    /// A popup that reopens three months from where it was left is a popup
    /// whose state the user cannot see while it is shut.
    #[test]
    fn reopening_rewinds_to_todays_month() {
        let mut cal = open_month();
        cal.next_month();
        cal.show_year_view();
        cal.selected_date = Some((2026, 8, 3));
        cal.set_visible(false);
        cal.set_visible(true);

        assert_eq!((cal.view_year, cal.view_month), (2026, 5));
        assert_eq!(cal.mode, CalendarViewMode::Month);
        assert_eq!(cal.selected_date, None);
    }

    /// Every mini month answers at the box it was drawn in.
    #[test]
    fn every_mini_month_answers_at_its_own_box() {
        let mut cal = open_month();
        cal.show_year_view();
        let store = EventStore::new();
        let layout = YearLayout::new(0.0, 0.0, 1.0);
        for index in 0..12_usize {
            let (px, py) = centre(layout.month(index));
            let month = u32::try_from(index).expect("12 fits") + 1;
            assert_eq!(
                cal.hit_test(0.0, 0.0, 1.0, px, py, &store),
                Some(CalendarHit::Month(month)),
                "mini month {month} is not clickable where it is drawn"
            );
        }
    }

    /// Picking a month from the overview returns to that month's grid.
    #[test]
    fn picking_a_mini_month_opens_it() {
        let mut cal = open_month();
        cal.show_year_view();
        cal.next_year();
        assert!(cal.apply(CalendarHit::Month(11)));
        assert_eq!(cal.mode, CalendarViewMode::Month);
        assert_eq!((cal.view_year, cal.view_month), (2027, 11));
    }

    /// The arrows page months in the grid and years in the overview.
    #[test]
    fn the_arrows_page_by_whatever_the_view_shows() {
        let mut cal = open_month();
        assert!(cal.apply(CalendarHit::NextPage));
        assert_eq!((cal.view_year, cal.view_month), (2026, 6));

        assert!(cal.apply(CalendarHit::Title));
        assert_eq!(cal.mode, CalendarViewMode::Year);
        assert!(cal.apply(CalendarHit::NextPage));
        assert_eq!(cal.view_year, 2027);
        assert_eq!(cal.view_month, 6, "paging years must not move the month");

        assert!(cal.apply(CalendarHit::Title));
        assert_eq!(cal.mode, CalendarViewMode::Month);
    }

    /// A click on inert popup space changes nothing.
    #[test]
    fn a_click_on_the_panel_changes_nothing() {
        let mut cal = open_month();
        let before = (cal.view_year, cal.view_month, cal.mode, cal.selected_date);
        assert!(!cal.apply(CalendarHit::Panel));
        assert_eq!(
            (cal.view_year, cal.view_month, cal.mode, cal.selected_date),
            before
        );
    }

    /// Today comes from a zone's rules, not a fixed offset.
    ///
    /// A shell that opened the popup with a stored `-5h` for New York would
    /// show yesterday's date for the last few hours of every summer evening.
    #[test]
    fn today_is_read_through_the_zones_rules() {
        // 2026-07-04 01:30 UTC. New York is on EDT (-4), so it is still the
        // evening of the 3rd there; a fixed -5 would agree by luck here, so
        // pick an instant where the two rules disagree: 2026-01-01 04:30 UTC
        // is 23:30 on 2025-12-31 in EST (-5) but 00:30 on the 1st in EDT.
        let ny = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("valid POSIX TZ");
        let mut cal = CalendarView::new(CalendarConfig::default());

        let summer = date_to_timestamp(2026, 7, 4, 1, 30, 0).expect("valid");
        cal.set_today_from_zone(summer, &ny);
        assert_eq!(cal.today, (2026, 7, 3), "EDT is -4, not -5");

        let winter = date_to_timestamp(2026, 1, 1, 4, 30, 0).expect("valid");
        cal.set_today_from_zone(winter, &ny);
        assert_eq!(cal.today, (2025, 12, 31), "EST is -5");
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
                color: None,
                description: String::new(),
            },
        );

        // Query a range entirely before the start date.
        let range_start = date_to_timestamp(2024, 5, 1, 0, 0, 0).expect("valid");
        let range_end = date_to_timestamp(2024, 6, 1, 0, 0, 0).expect("valid");
        let events = store.events_for_range(range_start, range_end);
        assert!(events.is_empty());
    }

    // ========================================================================
    // Colour
    //
    // Module 43 of part 2 of
    // TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE. The
    // eight `Color` constants that used to live in this file's `mod theme` are
    // gone, and the tests below are the only thing that says so: a constant
    // the conversion missed still compiles and still draws the colour it
    // always drew, so it is invisible to the eye and to every other test here.
    // ========================================================================

    /// A palette whose accent is a member of no role.
    ///
    /// The stock accent *is* `blue`, and this module has three sites that mean
    /// "today" — the day disc, the mini-month label, the "Today" button —
    /// which must be tellable apart from any role that happens to share the
    /// accent's value. The assertion lives in the fixture rather than in one
    /// test of its own so that it fires in every test that renders.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0x00FF_00FF);
        assert!(
            !p.roles()
                .iter()
                .any(|(n, r)| *n != "accent" && *r == p.accent),
            "the fixture accent collides with a role, so no test using it can \
             tell an accented site from that role"
        );
        p
    }

    /// The colour a user gave one of their own events.
    ///
    /// Deliberately in neither palette. `CalendarEvent::color` is the one
    /// value this module draws that is not the shell's to theme, so a fixture
    /// that used a role could not tell "the user's colour was honoured" from
    /// "the default was drawn" — and telling those apart is the whole point,
    /// since the month-grid dot ignored the field entirely until module 43.
    const USER_ORANGE: Color = Color::from_hex(0xFF7F00);

    /// Every colour a command carries, alpha discarded.
    ///
    /// The drop shadow is included rather than filtered out. It is black at
    /// every alpha in both modes on purpose (§525 decision 3), which makes it
    /// the one entry in the ordered pin below that must *not* move with the
    /// mode — and a site excluded from the pin is a site the pin cannot check.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::BoxShadow { color, .. } => {
                    Some(Color::rgba(color.r, color.g, color.b, 255))
                }
                _ => None,
            })
            .collect()
    }

    /// Opaque black, as [`colors`] reports the drop shadow.
    const SHADOW: Color = Color::rgba(0, 0, 0, 255);

    /// The month view the ordered pin below is a claim about.
    ///
    /// May 2026, open on today (the 18th), the 7th selected, week numbers on,
    /// a clock band carrying one extra zone, and events on three days: one
    /// uncoloured on a plain day, seven on the selected day of which the
    /// earliest carries [`USER_ORANGE`], and one uncoloured on today. Between
    /// them they reach every colour site the month view has except the "Today"
    /// button, which by construction cannot coexist with a visible today cell.
    ///
    /// The grid's shape is a fact about the calendar rather than about this
    /// code: 1 May 2026 is a Friday and weeks start on Sunday, so cell 0 is
    /// 26 April, cell 5 is 1 May, cell 35 is 31 May, and cells 36..=41 are
    /// 1..=6 June. Today is therefore cell 22 and the selection cell 11. Those
    /// are asserted here so that the literal indices the pin uses are anchored
    /// to the dates they claim to be, and cannot quietly come to mean other
    /// cells if the grid's origin ever moves.
    fn scene() -> (CalendarView, EventStore) {
        let config = CalendarConfig {
            show_week_numbers: true,
            ..Default::default()
        };
        let mut cal = CalendarView::new(config);
        cal.set_today(2026, 5, 18);
        cal.set_visible(true);
        cal.selected_date = Some((2026, 5, 7));

        let mut clock = ClockDisplay::new();
        assert!(clock.add_timezone("Tokyo", "JST-9"));
        cal.header = Some(ClockHeader {
            clock,
            zone: Tz::UTC,
        });

        let grid = cal.generate_grid();
        assert_eq!(grid.len(), 42);
        assert_eq!((grid[0].month, grid[0].day), (4, 26));
        assert_eq!((grid[5].month, grid[5].day), (5, 1));
        assert_eq!((grid[11].month, grid[11].day), (5, 7));
        assert_eq!((grid[22].month, grid[22].day), (5, 18));
        assert_eq!((grid[35].month, grid[35].day), (5, 31));
        assert_eq!((grid[41].month, grid[41].day), (6, 6));

        let mut store = EventStore::new();
        let at = |d: u32, h: u32| date_to_timestamp(2026, 5, d, h, 0, 0).expect("valid");
        // A plain current-month day with one uncoloured event: the default dot.
        add(&mut store, make_event("Dentist", at(3, 9), at(3, 10)));
        // Seven on the selected day, so the detail card fills to its six-row
        // maximum and draws the overflow line. `events_for_date` sorts by
        // start time, so the 08:00 one is the `first()` the dot reads — and it
        // is the only one carrying a colour.
        let mut coloured = make_event("Launch", at(7, 8), at(7, 9));
        coloured.color = Some(USER_ORANGE);
        add(&mut store, coloured);
        for i in 1..7u32 {
            add(&mut store, make_event("Slot", at(7, 9 + i), at(7, 10 + i)));
        }
        // One on today, uncoloured, so the dot's on-the-disc rule is drawn.
        add(&mut store, make_event("Standup", at(18, 9), at(18, 10)));
        (cal, store)
    }

    /// Every colour the month view draws, in draw order, written out by hand.
    ///
    /// Written out rather than derived: an expectation built by walking the
    /// same grid the renderer walks is an echo, not a claim, and cannot see
    /// that grid permuted. "Today is cell 22" is something this test asserts.
    #[test]
    fn every_month_view_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let (cal, store) = scene();

            let mut want = vec![
                SHADOW,     // the popup's drop shadow
                p.base,     // the popup itself
                p.surface1, // its one-pixel border
                p.text,     // the clock band's time
                p.subtext0, // its date
                p.subtext0, // its one extra zone
                p.subtext0, // the "<" arrow
                p.subtext0, // the ">" arrow
                p.text,     // "May 2026"
            ];
            // No "Today" button: the view is already on today's month. It has
            // a test of its own below, because it cannot appear here.
            want.extend([p.subtext0; 7]); // day-of-week headers
            want.extend([p.subtext0; 6]); // the week-number gutter, one per row

            for i in 0..42usize {
                match i {
                    // Today: the accent disc, then ink derived from it.
                    22 => want.extend([p.accent, readable_on(p.accent)]),
                    // The selection: a surface disc, and ordinary day ink.
                    11 => want.extend([p.surface0, p.text]),
                    // Lead-in from April and spill-over into June.
                    0..=4 | 36..=41 => want.push(p.subtext0),
                    _ => want.push(p.text),
                }
                match i {
                    7 => want.push(p.lavender),              // 3 May, uncoloured
                    11 => want.push(USER_ORANGE),            // 7 May, the user's
                    22 => want.push(readable_on(p.accent)),  // 18 May, on the disc
                    _ => {}
                }
            }

            // The detail card for the 7th: its fill, its "May 7" header, then
            // six of the day's seven events as bar/time/title, then the line
            // that says one was left out.
            want.extend([p.mantle, p.text]);
            want.extend([USER_ORANGE, p.subtext1, p.text]);
            for _ in 0..5 {
                want.extend([p.lavender, p.subtext1, p.text]);
            }
            want.push(p.subtext1);

            assert_eq!(
                colors(&cal.render(&p, 0.0, 0.0, 1.0, NOW, &store)),
                want,
                "month view, light = {light}"
            );
        }
    }

    /// The "Today" button wears the accent, and only appears off today's month.
    ///
    /// It is a control the user can act on, so it follows the accent rather
    /// than a fixed blue — the other half of design-decisions §527, where the
    /// About dialog's *logo* deliberately does not.
    #[test]
    fn the_today_button_wears_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let (mut cal, store) = scene();
            let on_today = colors(&cal.render(&p, 0.0, 0.0, 1.0, NOW, &store));
            cal.next_month();
            let off_month = colors(&cal.render(&p, 0.0, 0.0, 1.0, NOW, &store));

            // The button is the tenth colour, straight after the month title,
            // and exists only in the second render.
            assert_eq!(on_today[8], p.text, "the month title");
            assert_eq!(off_month[8], p.text, "the month title");
            assert_eq!(off_month[9], p.accent, "the Today button");
            assert_ne!(on_today[9], p.accent, "no Today button on today's month");
        }
    }

    /// The year view's chrome in order, and its one accented cell in 365.
    ///
    /// The twelve mini months are counted rather than pinned in order: 378
    /// literal entries would state nothing the month view's pin does not
    /// already state about which role means what, whereas the counts state the
    /// thing that is actually specific to this view — that exactly one day of
    /// the year is today and exactly one month label is the current one.
    #[test]
    fn every_year_view_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let (mut cal, store) = scene();
            cal.mode = CalendarViewMode::Year;
            let got = colors(&cal.render(&p, 0.0, 0.0, 1.0, NOW, &store));

            assert_eq!(
                &got[..5],
                &[SHADOW, p.base, p.subtext0, p.subtext0, p.text],
                "year-view shadow, card, two arrows and title, light = {light}"
            );

            // Twelve month labels, 365 day numbers (2026 is not a leap year),
            // and one disc under today.
            let rest = &got[5..];
            assert_eq!(rest.len(), 12 + 365 + 1, "light = {light}");
            let n = |want: Color| rest.iter().filter(|c| **c == want).count();
            assert_eq!(n(p.accent), 2, "May's label and today's disc");
            assert_eq!(n(readable_on(p.accent)), 1, "the digit on today's disc");
            assert_eq!(n(p.text), 11, "the other eleven month labels");
            assert_eq!(n(p.subtext0), 364, "every day of 2026 except today");
        }
    }

    /// Every colour either view draws is one its palette can account for.
    ///
    /// The light render is the one that matters: every constant deleted from
    /// this module was a Catppuccin Mocha value, so a missed substitution is a
    /// colour the light palette does not contain, and it names itself.
    #[test]
    fn every_colour_the_calendar_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            let derived = [p.accent, USER_ORANGE];
            for off_month in [false, true] {
                for mode in [CalendarViewMode::Month, CalendarViewMode::Year] {
                    let (mut cal, store) = scene();
                    cal.mode = mode;
                    if off_month {
                        // Reaches the "Today" button, which the scene's own
                        // month cannot draw. A site nothing renders is a site
                        // nothing checks.
                        cal.next_month();
                    }
                    crate::palette_check::assert_drawn_from(
                        &p,
                        &cal.render(&p, 0.0, 0.0, 1.0, NOW, &store),
                        &derived,
                        "calendar",
                    );
                }
            }
        }
    }

    /// Every colour that is a role moves when the mode does.
    ///
    /// This is what the membership sweep cannot see. `assert_drawn_from` runs
    /// against the light palette and is obliged to accept `#EFF1F5` — it is
    /// both Latte `base` and a `readable_on` endpoint — so a leftover constant
    /// that happened to hold a Latte value would pass it. It cannot pass this:
    /// a constant does not move.
    #[test]
    fn every_role_the_calendar_draws_moves_with_the_mode() {
        let dark = accented(false);
        let pale = accented(true);
        for mode in [CalendarViewMode::Month, CalendarViewMode::Year] {
            let (mut cal, store) = scene();
            cal.mode = mode;
            let a = colors(&cal.render(&dark, 0.0, 0.0, 1.0, NOW, &store));
            let b = colors(&cal.render(&pale, 0.0, 0.0, 1.0, NOW, &store));
            assert_eq!(a.len(), b.len());
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                // Three values are the same in both modes on purpose: the drop
                // shadow is an absence of light rather than a colour, the
                // accent is the user's (and this fixture sets one accent for
                // both modes), and so is the event colour. Everything else is
                // a role, and a role that did not move is a constant.
                let fixed = *x == SHADOW
                    || *x == dark.accent
                    || *x == readable_on(dark.accent)
                    || *x == USER_ORANGE;
                if fixed {
                    assert_eq!(x, y, "colour {i} should not have moved");
                } else {
                    assert_ne!(
                        x, y,
                        "colour {i} is #{:02X}{:02X}{:02X} in both modes",
                        x.r, x.g, x.b
                    );
                }
            }
        }
    }

    /// WCAG contrast ratio between two opaque colours.
    fn contrast(a: Color, b: Color) -> f64 {
        fn lum(c: Color) -> f64 {
            fn ch(v: u8) -> f64 {
                let v = f64::from(v) / 255.0;
                if v <= 0.040_45 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        }
        let (x, y) = (lum(a), lum(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Every ink this module puts on a fill clears the 4.5:1 floor.
    ///
    /// Contrast is not a membership property: both halves of an unreadable
    /// pairing can be perfectly good palette members, so the sweep above is
    /// blind to this and always will be. Three of these pairings were below
    /// the floor before the conversion — the adjacent-month days at 1.91:1 in
    /// Latte, the detail card's time at 3.40:1, the week-number gutter — all
    /// because a `surface*` role was being read as an ink, which is
    /// unreadable by construction: surfaces sit near the background, which is
    /// what they are for.
    ///
    /// Run against the *stock* palette, not [`accented`]: an arbitrary accent
    /// has arbitrary contrast, and what to do about that is a design question
    /// logged as
    /// `TD-C-A-USER-CHOSEN-EVENT-COLOUR-CAN-VANISH-INTO-THE-TODAY-DISC`
    /// rather than something this test gets to decide. The pairings are
    /// written out by hand because a pairing is a fact about which fill an ink
    /// lands on, and the command list does not record that.
    #[test]
    fn every_pairing_the_calendar_draws_clears_the_contrast_floor() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let pairings = [
                ("the clock's time", p.base, p.text),
                ("the clock's date and zone rows", p.base, p.subtext0),
                ("the navigation arrows", p.base, p.subtext0),
                ("the month and year title", p.base, p.text),
                ("the day-of-week headers", p.base, p.subtext0),
                ("the week-number gutter", p.base, p.subtext0),
                ("this month's day numbers", p.base, p.text),
                ("the adjacent months' day numbers", p.base, p.subtext0),
                ("a selected day's number", p.surface0, p.text),
                ("today's number", p.accent, readable_on(p.accent)),
                ("the detail card's header", p.mantle, p.text),
                ("an event's time", p.mantle, p.subtext1),
                ("an event's title", p.mantle, p.text),
                ("the overflow line", p.mantle, p.subtext1),
                ("a mini month's day numbers", p.base, p.subtext0),
                ("the current mini month's label", p.base, p.accent),
                ("another mini month's label", p.base, p.text),
                ("the \"Today\" button", p.base, p.accent),
            ];
            for (what, bg, ink) in pairings {
                let ratio = contrast(bg, ink);
                assert!(
                    ratio >= 4.5,
                    "{what} reads at {ratio:.2}:1 in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// The tray-clock delegate hands on the palette it was given.
    ///
    /// `render_tray_clock` is a one-line delegate to `ClockDisplay::render`
    /// and, before this test, was called from nowhere in the tree — every
    /// other test in this module called the inner function directly. A
    /// delegate is a site: it could have dropped the palette, transposed x and
    /// y, or handed the clock a palette of its own, and nothing would have
    /// failed. Same shape as the `window_peek` manager delegate found in
    /// module 41.
    #[test]
    fn the_tray_clock_delegate_draws_the_palette_it_is_handed() {
        for light in [false, true] {
            let p = accented(light);
            let (cal, _) = scene();
            let mut clock = ClockDisplay::new();
            assert!(clock.add_timezone("Tokyo", "JST-9"));
            let via = cal.render_tray_clock(&p, &clock, 12.0, 34.0, 1.0, NOW, &Tz::UTC);
            let direct = clock.render(&p, 12.0, 34.0, 1.0, NOW, &Tz::UTC);
            assert_eq!(
                format!("{via:?}"),
                format!("{direct:?}"),
                "the delegate changed something, light = {light}"
            );
            assert_eq!(colors(&via), vec![p.text, p.subtext0, p.subtext0]);
        }
    }

    /// An event the user never coloured does not gain a colour by being saved.
    ///
    /// `color` was a plain `Color` whose parser default was a hardcoded Mocha
    /// blue, which the serializer then wrote back out — so merely opening the
    /// calendar edited the user's file. `None` is not a colour and must not be
    /// spelled as one; see design-decisions §528.
    #[test]
    fn an_uncoloured_event_round_trips_without_gaining_a_colour() {
        let mut store = EventStore::new();
        add(&mut store, make_event("Plain", 5_000, 6_000));
        let text = store.export_text();
        assert!(
            !text.contains("color:"),
            "an uncoloured event was written out with a colour: {text}"
        );

        let mut back = EventStore::new();
        assert_eq!(back.import_text(&text), 1);
        assert_eq!(back.get_event(1).expect("event 1").color, None);
    }

    /// A colour the user *did* choose is drawn in the month grid.
    ///
    /// The dot used to be a fixed lavender that never looked at the event, so
    /// the field was parsed, stored and written back faithfully while changing
    /// nothing visible outside the detail card. A value that round-trips and a
    /// value that is used are unrelated properties.
    #[test]
    fn a_coloured_event_shows_the_users_colour_in_the_month_grid() {
        for light in [false, true] {
            let p = accented(light);
            let (cal, store) = scene();
            let dots: Vec<Color> = cal
                .render(&p, 0.0, 0.0, 1.0, NOW, &store)
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        width,
                        height,
                        color,
                        ..
                    } if (*width - DOT_RADIUS * 2.0).abs() < 0.01
                        && (*height - DOT_RADIUS * 2.0).abs() < 0.01 =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .collect();
            // 3 May's default, 7 May's chosen colour, 18 May's on-disc ink.
            assert_eq!(
                dots,
                vec![p.lavender, USER_ORANGE, readable_on(p.accent)],
                "light = {light}"
            );
        }
    }

    /// The dot and the detail card's bar cannot disagree about one event.
    ///
    /// Both go through `CalendarEvent::dot_color`, which is the point of it
    /// existing: two sites resolving `None` independently is two answers to
    /// the same question, and they drift.
    #[test]
    fn the_dot_and_the_detail_bar_resolve_a_colour_the_same_way() {
        for light in [false, true] {
            let p = accented(light);
            let coloured = {
                let mut e = make_event("x", 0, 1);
                e.color = Some(USER_ORANGE);
                e
            };
            assert_eq!(coloured.dot_color(&p), USER_ORANGE);
            assert_eq!(make_event("y", 0, 1).dot_color(&p), p.lavender);
        }
    }
}
