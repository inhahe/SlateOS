#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::needless_pass_by_value)]

//! Slate OS Habit Tracker --- track daily and weekly habits with streaks,
//! categories, contribution graphs, and completion statistics.
//!
//! Features:
//! - Habits created, archived, restored and deleted (deletion asks first)
//! - Daily or weekly frequency, and a category each
//! - Check-ins for today and the six days before it
//! - Streaks (current and best) and completion rates (7-day, 30-day, all-time)
//! - A contribution graph, a statistics view and an archive
//!
//! "Today" is the clock's, and rolls over at midnight while the window is open
//! (design-decisions 1201). The habits and every check-in are kept in the
//! user's settings (`settingsfile`, `habits.yaml`), one entry per habit.
//! Every control answers the pointer -- the renderer records a hit box where it
//! draws each one (`guitk::frame::Frame`) -- and every key is on the F1 card.

use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::RenderTree;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
// The shared civil-date arithmetic. This app used to carry its own copy: a
// Zeller's congruence for the weekday, a *separate* Rata Die day number for
// differences (offset by -307 rather than the -1 the same formula uses two
// apps over, and documented as "not calendar-accurate, but consistent"), its
// own leap rule, and month-stepping `while` loops in `add_days`. See
// `known-issues.md` C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC.
use guitk::date::{self, Weekday};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};

// ── Catppuccin Mocha palette ────────────────────────────────────────

// Part of the complete Catppuccin Mocha palette, kept whole even though no
// widget currently paints with this one: a named palette with a hole in it is
// not the palette it is named after, and the next widget to want one would
// otherwise re-derive the hex by hand.

// ── Per-habit statistics table ──────────────────────────────────────
//
// The widths live here once. They used to live in a `col_widths` array, in
// each cell's `max_width` as `col_widths[i] - 4.0`, and in a `cx +=` that
// advanced by the *undiscounted* width -- three expressions for one number,
// which is the arrangement that makes "does this cell fit its column?" a
// question with no home. The 4px discount is the gap between columns, so it
// belongs to the table, not to every cell.
/// What the window says before any habit exists.
///
/// Three lines. The third is what makes this different from a reader shipping
/// with a sample book: the invented data here was **a record of what the user
/// did**, on dated days, and every streak and completion rate was computed
/// over it. Their first real check-in would have been averaged in with
/// forty-five days of days they never had.
const NO_HABITS_LINES: [&str; 3] = [
    "No habits yet.",
    "Start one with + New Habit or N. Its check-ins are kept between runs.",
    "This app opened with 45 days of check-ins until 2026-09-15 -- a record of days nobody had.",
];

/// Every key this program answers, and what it does.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`.
const SHORTCUTS: &[(&str, &str)] = &[
    (
        "1 / 2 / 3 / 4",
        "Dashboard / statistics / archive / contribution graph",
    ),
    ("N", "New habit"),
    ("Up / Down", "Choose a habit"),
    ("Left / Right", "Choose a day; another habit, in the graph"),
    (
        "Space / Enter",
        "Check in on the chosen day; restore, in the archive",
    ),
    ("A", "Archive the chosen habit"),
    ("Ctrl+D", "Delete the chosen habit (asks first)"),
    ("C", "Show one category"),
    ("PgUp / PgDn", "Scroll the list"),
    ("F1", "This list"),
];

/// The statistics view's margin, its cards' height, and its table's row
/// pitch: `render_statistics` draws with them and `stats_rect` finds the rows
/// by them, so the two cannot disagree about where a row is.
const STATS_PAD: f32 = 20.0;
const STATS_CARD_H: f32 = 100.0;
const STATS_ROW_H: f32 = 24.0;

/// Room kept under a list's last row when it is scrolled to its end.
const LIST_END_PAD: f32 = 8.0;

/// The delete question's card.
const CONFIRM_W: f32 = 440.0;
const CONFIRM_H: f32 = 150.0;

/// Where the habits are kept (`settingsfile`), under `habits`, one map per
/// habit named by its id.
const CONFIG_NAME: &str = "habits";
const HABITS_KEY: &str = "habits";

/// Today's date from the system clock, or `None` if it cannot be read.
///
/// The zone comes from `tzrules` as in `apps/reminders`, so a real local zone
/// is used on the day `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ` is fixed.
fn today_from_clock() -> Option<Date> {
    let (date, _) = clock_now()?;
    Some(date)
}

/// Today's date, and how many seconds are left of it.
fn clock_now() -> Option<(Date, u64)> {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let utc = i64::try_from(since_epoch.as_secs()).ok()?;
    let zone = tzrules::Tz::utc();
    let local = utc.saturating_add(i64::from(zone.lookup(utc).gmtoff));
    let into_day = u64::try_from(local.rem_euclid(86_400)).ok()?;
    let day_start = local.saturating_sub(local.rem_euclid(86_400));
    let date = Date::from_civil(date::Date::from_unix_utc(day_start));
    Some((date, 86_400_u64.saturating_sub(into_day)))
}

const STATS_GAP: f32 = 4.0;
const STATS_COLUMNS: &[Column] = &[
    Column {
        label: "Habit",
        width: 136.0,
    },
    Column {
        label: "Category",
        width: 86.0,
    },
    Column {
        label: "Streak",
        width: 46.0,
    },
    Column {
        label: "Best",
        width: 46.0,
    },
    Column {
        label: "7d",
        width: 46.0,
    },
    Column {
        label: "30d",
        width: 46.0,
    },
    Column {
        label: "All",
        width: 46.0,
    },
    Column {
        label: "Total",
        width: 46.0,
    },
];
const STATS_HABIT: usize = 0;
const STATS_CATEGORY: usize = 1;
const STATS_STREAK: usize = 2;
const STATS_BEST: usize = 3;
const STATS_7D: usize = 4;
const STATS_30D: usize = 5;
const STATS_ALL: usize = 6;
const STATS_TOTAL: usize = 7;
const STATS_HEADER_FONT: f32 = 10.0;
const STATS_ROW_FONT: f32 = 11.0;

// ── Date ────────────────────────────────────────────────────────────

/// Simple date: year, month (1-12), day (1-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Date {
    year: i32,
    month: u32,
    day: u32,
}

impl Date {
    /// A date, or `None` if there is no such day: what a saved check-in is
    /// read back through.
    ///
    /// Calls `date::days_in_month` rather than a local forwarding wrapper:
    /// there were two such wrappers here whose only caller was this function,
    /// which made three names for one answer.
    fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        if !(1..=12).contains(&month) {
            return None;
        }
        let max_d = date::days_in_month(year, month);
        if day < 1 || day > max_d {
            return None;
        }
        Some(Self { year, month, day })
    }

    fn civil(self) -> date::Date {
        date::Date::from_ymd(self.year, self.month, self.day)
    }

    fn from_civil(d: date::Date) -> Self {
        let (year, month, day) = d.ymd();
        Self { year, month, day }
    }

    fn weekday(self) -> Weekday {
        self.civil().weekday()
    }

    /// Day of week: 0=Sunday ... 6=Saturday.
    ///
    /// Was a hand-written Zeller's congruence, correct for years >= 1 and
    /// wrong below that: `y % 100` and `y / 100` truncate toward zero in
    /// Rust, not the flooring the formula assumes.
    /// `#[cfg(test)]`: its only caller is `load_sample_habits`.
    #[cfg(test)]
    fn day_of_week(self) -> u32 {
        u32::try_from(self.weekday().index()).unwrap_or(0)
    }

    fn day_of_week_short(self) -> &'static str {
        self.weekday().short_name()
    }

    fn month_short(self) -> &'static str {
        month_short(self.month)
    }

    /// Add days (positive or negative).
    ///
    /// Was a pair of `while` loops that stepped one month at a time, so the
    /// cost was proportional to the distance moved — and a streak view that
    /// walks back a year did so one month per iteration. Worse, the loop
    /// condition read `days_in_month`, whose `_ => 30` fallback would have
    /// let a bad month produce a date the loop could not leave.
    fn add_days(self, n: i32) -> Self {
        Self::from_civil(self.civil().add_days(n))
    }

    /// Number of days between self and other (self - other). Positive if self
    /// is later.
    ///
    /// The day number this used to subtract was described as "not
    /// calendar-accurate, but consistent" — true only because the two
    /// subtracted numbers shared the same offset, which is a property of the
    /// call site rather than of the function. It is now a real day count.
    fn days_since(self, other: Self) -> i32 {
        other.civil().days_until(self.civil())
    }

    /// A monotonic day number: days since 1970-01-01, negative before it.
    ///
    /// Kept because the streak walk and the grid both compare dates by it,
    /// but it is no longer a *separate* numbering from the one the weekday
    /// comes from — both are now the same day count, so they cannot disagree.
    /// It was a Rata Die offset by -307, which made it monotonic but not a
    /// count of anything; the old comment said as much.
    fn to_day_number(self) -> i32 {
        self.civil().days_since_epoch()
    }

    /// The Monday of the ISO week containing this date.
    fn week_start_monday(self) -> Self {
        // `days_since` on `Weekday` answers "how far back to the given day",
        // which is exactly the question, rather than reconstructing it from a
        // 0=Sunday index with a special case for Sunday itself.
        let back = self.weekday().days_since(Weekday::Monday);
        self.add_days(i32::try_from(back).unwrap_or(0).saturating_neg())
    }

    fn format_short(self) -> String {
        format!("{} {:02}", self.month_short(), self.day)
    }

    fn format_full(self) -> String {
        format!("{} {:02}, {}", self.month_short(), self.day, self.year)
    }

    /// `YYYY-MM-DD`: how a date is kept.
    fn to_iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// A kept date read back, or `None` for anything that is not one.
    fn from_iso(text: &str) -> Option<Self> {
        let mut parts = text.trim().splitn(3, '-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        Self::new(year, month, day)
    }
}

// These delegate to `guitk::date` rather than restating it. Note what the
// old `days_in_month` returned for an out-of-range month: **30**. The
// calendar's returned **0** and reminders' returned **0**. Three apps, three
// different answers to the same impossible question, none of them reachable
// today and all of them waiting for a caller that does not validate first.
// The shared version clamps into 1..=12, which is the only answer that keeps
// every caller's loop terminating.

fn month_short(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}

// ── Category ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Category {
    Health,
    Fitness,
    Productivity,
    Mindfulness,
    Learning,
    Social,
    Creative,
    Finance,
    Custom,
}

impl Category {
    const ALL: [Self; 9] = [
        Self::Health,
        Self::Fitness,
        Self::Productivity,
        Self::Mindfulness,
        Self::Learning,
        Self::Social,
        Self::Creative,
        Self::Finance,
        Self::Custom,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Health => "Health",
            Self::Fitness => "Fitness",
            Self::Productivity => "Productivity",
            Self::Mindfulness => "Mindfulness",
            Self::Learning => "Learning",
            Self::Social => "Social",
            Self::Creative => "Creative",
            Self::Finance => "Finance",
            Self::Custom => "Custom",
        }
    }

    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Health => pal.green,
            Self::Fitness => pal.peach,
            Self::Productivity => pal.blue,
            Self::Mindfulness => pal.mauve,
            Self::Learning => pal.yellow,
            Self::Social => pal.teal,
            Self::Creative => pal.lavender,
            Self::Finance => pal.red,
            Self::Custom => pal.subtext0,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Health => "\u{2764}",
            Self::Fitness => "\u{1F3CB}",
            Self::Productivity => "\u{26A1}",
            Self::Mindfulness => "\u{1F9D8}",
            Self::Learning => "\u{1F4DA}",
            Self::Social => "\u{1F91D}",
            Self::Creative => "\u{1F3A8}",
            Self::Finance => "\u{1F4B0}",
            Self::Custom => "\u{2B50}",
        }
    }
}

// ── Frequency ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frequency {
    /// Must complete every day
    Daily,
    /// Must complete N times per week (Mon-Sun)
    Weekly(u32),
}

impl Frequency {
    fn label(self) -> String {
        match self {
            Self::Daily => String::from("Daily"),
            Self::Weekly(n) => format!("{n}x / week"),
        }
    }
}

// ── Habit ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Habit {
    /// What the habit is kept under. It was taken out when nothing was kept
    /// ("there is no identity to preserve across a restart"); there is now.
    id: u64,
    name: String,
    category: Category,
    frequency: Frequency,
    /// Dates on which this habit was checked in
    check_ins: Vec<Date>,
    /// Date the habit was created
    created: Date,
    archived: bool,
}

impl Habit {
    /// Took an `id` and a `description` until neither turned out to exist.
    ///
    /// The `id` was handed out by a counter, stored, incremented, and never
    /// read: every lookup here is by position, `active_habits` maps a filtered
    /// position back to a real one, and nothing is persisted, so there is no
    /// identity to preserve across a restart. The `description` came from a
    /// form field with no key handler -- it was always the empty string, and
    /// nothing displayed it.
    fn new(name: &str, category: Category, frequency: Frequency, created: Date) -> Self {
        Self {
            id: 0,
            name: String::from(name),
            category,
            frequency,
            check_ins: Vec::new(),
            created,
            archived: false,
        }
    }

    fn is_checked_on(&self, date: Date) -> bool {
        self.check_ins.contains(&date)
    }

    fn toggle_check_in(&mut self, date: Date) {
        if let Some(pos) = self.check_ins.iter().position(|&d| d == date) {
            self.check_ins.remove(pos);
        } else {
            self.check_ins.push(date);
            self.check_ins.sort();
        }
    }

    /// Current streak ending at `today`.
    fn current_streak(&self, today: Date) -> u32 {
        match self.frequency {
            Frequency::Daily => self.current_streak_daily(today),
            Frequency::Weekly(_n) => self.current_streak_weekly(today),
        }
    }

    fn current_streak_daily(&self, today: Date) -> u32 {
        let mut streak = 0u32;
        let mut d = today;
        loop {
            if self.is_checked_on(d) {
                streak = streak.saturating_add(1);
                d = d.add_days(-1);
            } else if d == today {
                // Today not yet checked in -- check yesterday
                d = d.add_days(-1);
            } else {
                break;
            }
        }
        streak
    }

    fn current_streak_weekly(&self, today: Date) -> u32 {
        let Frequency::Weekly(target) = self.frequency else {
            return 0;
        };
        let mut streak = 0u32;
        let mut week_start = today.week_start_monday();
        loop {
            let count = self.completions_in_week(week_start);
            if count >= target {
                streak = streak.saturating_add(1);
                week_start = week_start.add_days(-7);
            } else if week_start == today.week_start_monday() {
                // Current week is incomplete, check previous
                week_start = week_start.add_days(-7);
            } else {
                break;
            }
        }
        streak
    }

    fn completions_in_week(&self, week_monday: Date) -> u32 {
        let mut count = 0u32;
        for day_offset in 0..7 {
            let d = week_monday.add_days(day_offset);
            if self.is_checked_on(d) {
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Best streak ever.
    fn best_streak(&self, today: Date) -> u32 {
        match self.frequency {
            Frequency::Daily => self.best_streak_daily(today),
            Frequency::Weekly(_) => self.best_streak_weekly(today),
        }
    }

    fn best_streak_daily(&self, _today: Date) -> u32 {
        if self.check_ins.is_empty() {
            return 0;
        }
        let mut sorted = self.check_ins.clone();
        sorted.sort();
        sorted.dedup();
        let mut best = 1u32;
        let mut current = 1u32;
        for i in 1..sorted.len() {
            let (Some(this), Some(prev)) =
                (sorted.get(i), i.checked_sub(1).and_then(|j| sorted.get(j)))
            else {
                continue;
            };
            if this.days_since(*prev) == 1 {
                current = current.saturating_add(1);
                if current > best {
                    best = current;
                }
            } else {
                current = 1;
            }
        }
        best
    }

    fn best_streak_weekly(&self, today: Date) -> u32 {
        let Frequency::Weekly(target) = self.frequency else {
            return 0;
        };
        if self.check_ins.is_empty() {
            return 0;
        }
        let first = self.created;
        let mut week_start = first.week_start_monday();
        let end = today.week_start_monday().add_days(7);
        let mut best = 0u32;
        let mut current = 0u32;
        while week_start.to_day_number() < end.to_day_number() {
            let count = self.completions_in_week(week_start);
            if count >= target {
                current = current.saturating_add(1);
                if current > best {
                    best = current;
                }
            } else {
                current = 0;
            }
            week_start = week_start.add_days(7);
        }
        best
    }

    /// Completion rate over the last N days (0.0..=1.0).
    fn completion_rate(&self, today: Date, days: u32) -> f32 {
        match self.frequency {
            Frequency::Daily => {
                if days == 0 {
                    return 0.0;
                }
                let mut completed = 0u32;
                for i in 0..days {
                    let d = today.add_days((i as i32).saturating_neg());
                    if d.to_day_number() < self.created.to_day_number() {
                        // Don't count days before creation
                        let actual_days = i;
                        return if actual_days == 0 {
                            0.0
                        } else {
                            completed as f32 / actual_days as f32
                        };
                    }
                    if self.is_checked_on(d) {
                        completed = completed.saturating_add(1);
                    }
                }
                completed as f32 / days as f32
            }
            Frequency::Weekly(target) => {
                if days < 7 || target == 0 {
                    return 0.0;
                }
                let weeks = days / 7;
                if weeks == 0 {
                    return 0.0;
                }
                let mut met = 0u32;
                let mut ws = today.week_start_monday();
                for _ in 0..weeks {
                    let count = self.completions_in_week(ws);
                    if count >= target {
                        met = met.saturating_add(1);
                    }
                    ws = ws.add_days(-7);
                }
                met as f32 / weeks as f32
            }
        }
    }

    /// Completion rate over all time since creation.
    fn completion_rate_alltime(&self, today: Date) -> f32 {
        let total_days = today.days_since(self.created).saturating_add(1);
        if total_days <= 0 {
            return 0.0;
        }
        self.completion_rate(today, total_days as u32)
    }

    /// Total check-ins count.
    fn total_check_ins(&self) -> u32 {
        self.check_ins.len() as u32
    }
}

// ── Screens ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Dashboard,
    Statistics,
    Archive,
    HeatMap,
}

impl Screen {
    fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Statistics => "Statistics",
            Self::Archive => "Archive",
            Self::HeatMap => "Heatmap",
        }
    }
}

// ── App ─────────────────────────────────────────────────────────────

struct HabitTrackerApp {
    width: f32,
    height: f32,
    habits: Vec<Habit>,
    today: Date,
    screen: Screen,
    selected_habit: usize,
    scroll_offset: f32,
    category_filter: Option<Category>,
    show_create_form: bool,
    create_name: String,
    create_category_idx: usize,
    create_frequency_daily: bool,
    create_weekly_count: u32,
    /// Which day column is selected for check-in toggling (0 = today, 6 = 6 days ago)
    selected_day_col: usize,
    heatmap_habit_idx: usize,
    status_msg: String,
    /// Whether the shortcut card is up.
    show_help: bool,
    /// The habit a delete is waiting on a `Y` for.
    pending_delete: Option<usize>,
    /// The id the next habit gets.
    next_id: u64,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The wheel's remainder over the list.
    wheel: wheel::Accumulator,
    /// Whether changes are written to the user's settings. Only
    /// [`Self::from_settings`] turns it on, so an app made with `new` -- which
    /// is every test -- is backed by nothing, and a test cannot write the
    /// developer's own `habits.yaml` by forgetting a scratch config
    /// (known-issues: "A test that forgets its scratch settings writes the
    /// developer's own").
    persist: bool,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl HabitTrackerApp {
    const HEADER_H: f32 = 50.0;
    const ROW_H: f32 = 52.0;
    const DAY_COL_W: f32 = 38.0;
    const STATUS_H: f32 = 28.0;
    const NAV_H: f32 = 36.0;
    const HEATMAP_CELL: f32 = 14.0;
    const HEATMAP_GAP: f32 = 3.0;

    fn new() -> Self {
        // The clock's day (design-decisions 1201). It was 18 May 2026 for
        // good, moved only by `+` and `-`, so a check-in made today was filed
        // in May.
        let today =
            today_from_clock().unwrap_or_else(|| Date::from_civil(date::Date::from_unix_utc(0)));
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            width: 1000.0,
            height: 700.0,
            habits: Vec::new(),
            today,
            screen: Screen::Dashboard,
            selected_habit: 0,
            scroll_offset: 0.0,
            category_filter: None,
            show_create_form: false,
            create_name: String::new(),
            create_category_idx: 0,
            create_frequency_daily: true,
            create_weekly_count: 3,
            selected_day_col: 0,
            heatmap_habit_idx: 0,
            status_msg: String::from("Habit Tracker"),
            show_help: false,
            pending_delete: None,
            next_id: 1,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            persist: false,
        }
    }

    /// An app holding the history `new` used to invent, for tests.
    ///
    /// `#[cfg(test)]`. Most of this app's tests are about the streak
    /// arithmetic, the completion rates, the heatmap and the week view -- all
    /// of which need *check-ins*, not specifically invented ones.
    #[cfg(test)]
    fn with_sample_habits() -> Self {
        let mut app = Self::new();
        // The day these tests were written against. `new` reads the clock now,
        // and a history built back from the clock's day would move every
        // weekday-dependent answer (the week view, a weekly habit's rate) with
        // the day the suite happened to run.
        app.today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        app.load_sample_habits();
        app
    }

    /// Four habits with 45 days of check-ins behind them, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. `new` called it, so the window opened
    /// on an exercise habit kept about 70% of the time for a month and a half,
    /// and three more like it -- with streaks, completion rates and a heatmap
    /// computed from them.
    ///
    /// This is not the `apps/ebook` case. A book shipped with a reader claims
    /// nothing; **a check-in on a dated day is a claim about what the user
    /// did**, and it is exactly the claim this app exists to record. It is the
    /// `apps/finance` shape: the first real mark would have been averaged in
    /// with forty-five days of days nobody had.
    #[cfg(test)]
    fn load_sample_habits(&mut self) {
        let base = self.today.add_days(-45);

        // 1) Exercise -- daily, fitness
        let mut h1 = Habit::new("Exercise", Category::Fitness, Frequency::Daily, base);
        // Simulate some check-ins over the last 45 days (about 70% completion)
        for i in 0..45 {
            let d = base.add_days(i);
            if i % 10 != 3 && i % 10 != 7 && i % 7 != 6 {
                h1.check_ins.push(d);
            }
        }
        self.habits.push(h1);

        // 2) Read -- daily, learning
        let mut h2 = Habit::new("Read 30 min", Category::Learning, Frequency::Daily, base);
        for i in 0..45 {
            let d = base.add_days(i);
            if i % 3 != 2 {
                h2.check_ins.push(d);
            }
        }
        self.habits.push(h2);

        // 3) Meditate -- daily, mindfulness
        let mut h3 = Habit::new("Meditate", Category::Mindfulness, Frequency::Daily, base);
        for i in 0..45 {
            let d = base.add_days(i);
            if i % 5 != 4 {
                h3.check_ins.push(d);
            }
        }
        self.habits.push(h3);

        // 4) Meal prep -- weekly 3x, health
        let mut h4 = Habit::new("Meal Prep", Category::Health, Frequency::Weekly(3), base);
        for i in 0..45 {
            let d = base.add_days(i);
            let dow = d.day_of_week();
            if dow == 0 || dow == 3 || dow == 5 {
                h4.check_ins.push(d);
            }
        }
        self.habits.push(h4);

        // 5) Journal -- daily, productivity
        let mut h5 = Habit::new("Journal", Category::Productivity, Frequency::Daily, base);
        for i in 0..45 {
            let d = base.add_days(i);
            if i % 4 != 3 {
                h5.check_ins.push(d);
            }
        }
        self.habits.push(h5);
    }

    // ── Habit management ────────────────────────────────────────────

    fn active_habits(&self) -> Vec<usize> {
        let mut indices = Vec::new();
        for (i, h) in self.habits.iter().enumerate() {
            if h.archived {
                continue;
            }
            if let Some(cat) = self.category_filter
                && h.category != cat
            {
                continue;
            }
            indices.push(i);
        }
        indices
    }

    fn archived_habits(&self) -> Vec<usize> {
        self.habits
            .iter()
            .enumerate()
            .filter(|(_, h)| h.archived)
            .map(|(i, _)| i)
            .collect()
    }

    /// Read the user's habits from their settings.
    ///
    /// An entry with no name, or a date that is not one, is not shown; it is
    /// left in the file, which is the user's, and a later write touches only
    /// the entry it is about.
    fn load_habits(&mut self, doc: &yamldoc::Document) {
        let mut keys: Vec<(u64, String)> = doc
            .keys(&[HABITS_KEY])
            .into_iter()
            .filter_map(|k| k.parse::<u64>().ok().map(|id| (id, k)))
            .collect();
        keys.sort_unstable();
        for (id, key) in keys {
            let get = |field: &str| doc.get_str(&[HABITS_KEY, &key, field]);
            let Some(name) = get("name").filter(|n| !n.trim().is_empty()) else {
                continue;
            };
            let Some(created) = get("created").and_then(|c| Date::from_iso(&c)) else {
                continue;
            };
            let category = get("category")
                .and_then(|c| {
                    Category::ALL
                        .iter()
                        .copied()
                        .find(|cat| cat.label().eq_ignore_ascii_case(&c))
                })
                .unwrap_or(Category::Custom);
            let frequency = match get("frequency").as_deref().map(str::trim) {
                Some(f) if f.starts_with("weekly") => Frequency::Weekly(
                    f.trim_start_matches("weekly")
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(3)
                        .clamp(1, 7),
                ),
                _ => Frequency::Daily,
            };
            let mut habit = Habit::new(&name, category, frequency, created);
            habit.id = id;
            habit.archived = doc
                .get_bool(&[HABITS_KEY, &key, "archived"])
                .unwrap_or(false);
            let mut days: Vec<Date> = doc
                .get_seq(&[HABITS_KEY, &key, "check_ins"])
                .unwrap_or_default()
                .iter()
                .filter_map(|d| Date::from_iso(d))
                .collect();
            days.sort_unstable();
            days.dedup();
            habit.check_ins = days;
            self.next_id = self.next_id.max(id.saturating_add(1));
            self.habits.push(habit);
        }
    }

    /// The tracker the window opens: the user's habits, and every change kept.
    fn from_settings() -> Self {
        let mut app = Self::new();
        app.load_habits(&settingsfile::load(CONFIG_NAME));
        app.persist = true;
        app
    }

    /// Keep habit `index` in the user's settings.
    ///
    /// Nothing was kept: the banner said the app had no filesystem access,
    /// and every habit and check-in went when the window closed -- which for a
    /// program whose whole purpose is a record over days is the purpose.
    fn store_habit(&mut self, index: usize) {
        if !self.persist {
            return;
        }
        let Some(habit) = self.habits.get(index) else {
            return;
        };
        let key = habit.id.to_string();
        let mut doc = settingsfile::load(CONFIG_NAME);
        doc.set_str(&[HABITS_KEY, &key, "name"], &habit.name);
        doc.set_str(
            &[HABITS_KEY, &key, "category"],
            &habit.category.label().to_ascii_lowercase(),
        );
        doc.set_str(
            &[HABITS_KEY, &key, "frequency"],
            &match habit.frequency {
                Frequency::Daily => "daily".to_string(),
                Frequency::Weekly(n) => format!("weekly {n}"),
            },
        );
        doc.set_str(&[HABITS_KEY, &key, "created"], &habit.created.to_iso());
        doc.set_bool(&[HABITS_KEY, &key, "archived"], habit.archived);
        let days: Vec<String> = habit.check_ins.iter().map(|d| d.to_iso()).collect();
        let refs: Vec<&str> = days.iter().map(String::as_str).collect();
        doc.set_seq(&[HABITS_KEY, &key, "check_ins"], &refs);
        self.write(&doc);
    }

    /// Take habit `id` out of the user's settings.
    fn forget_habit(&mut self, id: u64) {
        if !self.persist {
            return;
        }
        let mut doc = settingsfile::load(CONFIG_NAME);
        doc.remove(&[HABITS_KEY, &id.to_string()]);
        self.write(&doc);
    }

    /// Write the settings, saying so if that fails: a mark that silently was
    /// not kept is found out the next day, when it is gone.
    ///
    /// The failure goes in the status line, so every caller writes *after*
    /// setting its own message -- or "Habit created!" would cover the news
    /// that it was not.
    fn write(&mut self, doc: &yamldoc::Document) {
        if let Err(e) = settingsfile::store(CONFIG_NAME, doc) {
            self.status_msg = format!("Could not keep your habits: {e}");
        }
    }

    fn create_habit(&mut self) {
        // Blank, not merely empty: a name of spaces would be kept, and then
        // dropped by `load_habits` the next time the window opened.
        if self.create_name.trim().is_empty() {
            self.status_msg = String::from("Name cannot be empty");
            return;
        }
        let category = Category::ALL
            .get(self.create_category_idx)
            .copied()
            .unwrap_or(Category::Custom);
        let frequency = if self.create_frequency_daily {
            Frequency::Daily
        } else {
            Frequency::Weekly(self.create_weekly_count.clamp(1, 7))
        };
        let mut habit = Habit::new(&self.create_name, category, frequency, self.today);
        habit.id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.habits.push(habit);
        self.create_name.clear();
        self.create_category_idx = 0;
        self.create_frequency_daily = true;
        self.create_weekly_count = 3;
        self.show_create_form = false;
        self.status_msg = String::from("Habit created!");
        self.store_habit(self.habits.len().saturating_sub(1));
    }

    fn delete_habit(&mut self, idx: usize) {
        if idx < self.habits.len() {
            let Some(name) = self.habits.get(idx).map(|h| h.name.clone()) else {
                return;
            };
            let removed = self.habits.remove(idx);
            if self.selected_habit >= self.active_habits().len() && self.selected_habit > 0 {
                self.selected_habit = self.selected_habit.saturating_sub(1);
            }
            self.status_msg = format!("Deleted: {name}");
            self.forget_habit(removed.id);
        }
    }

    fn archive_habit(&mut self, idx: usize) {
        if let Some(h) = self.habits.get_mut(idx) {
            h.archived = true;
            self.status_msg = format!("Archived: {}", h.name);
            self.store_habit(idx);
        }
    }

    fn unarchive_habit(&mut self, idx: usize) {
        if let Some(h) = self.habits.get_mut(idx) {
            h.archived = false;
            self.status_msg = format!("Restored: {}", h.name);
            self.store_habit(idx);
        }
    }

    fn toggle_check_in_selected(&mut self) {
        let active = self.active_habits();
        if let Some(&habit_idx) = active.get(self.selected_habit) {
            let date = self
                .today
                .add_days((self.selected_day_col as i32).saturating_neg());
            if let Some(h) = self.habits.get_mut(habit_idx) {
                h.toggle_check_in(date);
                let checked = h.is_checked_on(date);
                self.status_msg = if checked {
                    format!("Checked in: {} on {}", h.name, date.format_short())
                } else {
                    format!("Unchecked: {} on {}", h.name, date.format_short())
                };
                self.store_habit(habit_idx);
            }
        }
    }

    /// Move "today" on a day. **Tests only**: today is the clock's, and the
    /// `+` key that did this for a user is gone (design-decisions 1201).
    #[cfg(test)]
    fn advance_day(&mut self) {
        self.today = self.today.add_days(1);
        self.status_msg = format!("Date: {}", self.today.format_full());
    }

    /// Move "today" back a day. **Tests only**; see [`Self::advance_day`].
    #[cfg(test)]
    fn go_back_day(&mut self) {
        self.today = self.today.add_days(-1);
        self.status_msg = format!("Date: {}", self.today.format_full());
    }

    // ── Input handling ──────────────────────────────────────────────

    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        // The list of keys, whatever else is up -- except that `?` is a
        // character the new habit's name may need.
        if key == "F1" || (key == "?" && !self.show_create_form) {
            self.show_help = !self.show_help;
            return;
        }
        if self.show_help {
            // Modal: a key behind the card would change a list nobody can see.
            if key == "Escape" || key == "Return" {
                self.show_help = false;
            }
            return;
        }
        if self.show_create_form {
            self.handle_create_form_key(key, ctrl);
            return;
        }
        // A delete waiting on its answer takes the next key, and only `y`
        // deletes: the habit and every check-in go, and there is no undo.
        if let Some(idx) = self.pending_delete.take() {
            if key.eq_ignore_ascii_case("y") {
                self.delete_habit(idx);
            } else {
                self.status_msg = String::from("Kept");
            }
            return;
        }

        match key {
            "1" => {
                self.show_screen(Screen::Dashboard);
            }
            "2" => {
                self.show_screen(Screen::Statistics);
            }
            "3" => {
                self.show_screen(Screen::Archive);
            }
            "4" => {
                self.show_screen(Screen::HeatMap);
            }
            "n" | "N" if !ctrl => {
                self.show_create_form = true;
                self.status_msg = String::from("New habit -- fill in details");
            }
            "Up" if self.selected_habit > 0 => {
                self.selected_habit = self.selected_habit.saturating_sub(1);
            }
            "Down" => {
                let max = match self.screen {
                    Screen::Dashboard => self.active_habits().len(),
                    Screen::Archive => self.archived_habits().len(),
                    _ => 0,
                };
                if self.selected_habit.saturating_add(1) < max {
                    self.selected_habit = self.selected_habit.saturating_add(1);
                }
            }
            "Left" => {
                if self.screen == Screen::Dashboard && self.selected_day_col < 6 {
                    self.selected_day_col = self.selected_day_col.saturating_add(1);
                }
                if self.screen == Screen::HeatMap && self.heatmap_habit_idx > 0 {
                    self.heatmap_habit_idx = self.heatmap_habit_idx.saturating_sub(1);
                }
            }
            "Right" => {
                if self.screen == Screen::Dashboard && self.selected_day_col > 0 {
                    self.selected_day_col = self.selected_day_col.saturating_sub(1);
                }
                if self.screen == Screen::HeatMap {
                    let active = self.active_habits();
                    if self.heatmap_habit_idx.saturating_add(1) < active.len() {
                        self.heatmap_habit_idx = self.heatmap_habit_idx.saturating_add(1);
                    }
                }
            }
            "Space" | "Return" => match self.screen {
                Screen::Dashboard => self.toggle_check_in_selected(),
                Screen::Archive => {
                    let archived = self.archived_habits();
                    if let Some(&idx) = archived.get(self.selected_habit) {
                        self.unarchive_habit(idx);
                    }
                }
                _ => {}
            },
            "d" | "D" if ctrl => {
                // The habit chosen in the list on screen: the archive's own
                // selection indexes the archive, not the dashboard.
                let list = if self.screen == Screen::Archive {
                    self.archived_habits()
                } else {
                    self.active_habits()
                };
                if let Some(&idx) = list.get(self.selected_habit) {
                    self.ask_to_delete(idx);
                }
            }
            "a" | "A" if !ctrl && self.screen == Screen::Dashboard => {
                let active = self.active_habits();
                if let Some(&idx) = active.get(self.selected_habit) {
                    self.archive_habit(idx);
                }
            }
            "c" | "C" => {
                // Cycle category filter
                self.category_filter = match self.category_filter {
                    None => Some(Category::ALL[0]),
                    Some(cat) => {
                        let idx = Category::ALL.iter().position(|&c| c == cat).unwrap_or(0);
                        if let Some(next) = idx.checked_add(1) {
                            Category::ALL.get(next).copied()
                        } else {
                            None
                        }
                    }
                };
                self.selected_habit = 0;
                self.status_msg = match self.category_filter {
                    None => String::from("Filter: All categories"),
                    Some(cat) => format!("Filter: {}", cat.label()),
                };
            }
            "PageDown" => self.page(true),
            "PageUp" => self.page(false),
            _ => {}
        }
        self.keep_selection_visible();
    }

    /// Page Down and Page Up: a page of the list on screen, and the chosen
    /// habit with it, so it stays in view.
    ///
    /// They moved the view by 200 pixels and left the selection behind, which
    /// is what a list's Page Down never does -- and now that the view follows
    /// the selection, it would have pulled the page straight back.
    fn page(&mut self, down: bool) {
        let Some((pane, row_h, rows)) = self.list_pane() else {
            return;
        };
        let per_page = ((pane.h / row_h).floor() as usize).max(1);
        let step = per_page as f32 * row_h;
        let to = if down {
            self.scroll_offset + step
        } else {
            self.scroll_offset - step
        };
        self.scroll_offset = to.clamp(0.0, self.scroll_limit());
        if matches!(self.screen, Screen::Dashboard | Screen::Archive) {
            self.selected_habit = if down {
                self.selected_habit
                    .saturating_add(per_page)
                    .min(rows.saturating_sub(1))
            } else {
                self.selected_habit.saturating_sub(per_page)
            };
        }
    }

    /// Ask before deleting habit `idx`: its whole record goes with it.
    ///
    /// It was deleted at once, on Ctrl+D beside the D that means nothing and
    /// the A that archives. The question is a card with a button for each
    /// answer, so a pointer can give either; `Y` deletes and any other key
    /// keeps.
    fn ask_to_delete(&mut self, idx: usize) {
        if self.habits.get(idx).is_none() {
            return;
        }
        self.pending_delete = Some(idx);
    }

    /// Change screens, starting the new one's list at its top. Returns
    /// whether anything changed.
    ///
    /// The number keys set the screen and left the selection where it was, so
    /// the dashboard's fourth row became a selection past the end of a
    /// one-habit archive; a tab click reset it. One path for both now.
    fn show_screen(&mut self, screen: Screen) -> bool {
        if self.screen == screen {
            return false;
        }
        self.screen = screen;
        self.selected_habit = 0;
        self.scroll_offset = 0.0;
        true
    }

    fn handle_create_form_key(&mut self, key: &str, _ctrl: bool) {
        match key {
            "Escape" => {
                self.show_create_form = false;
                self.status_msg = String::from("Cancelled");
            }
            "Return" => {
                self.create_habit();
            }
            "Tab" => {
                // Cycle category
                self.create_category_idx = self
                    .create_category_idx
                    .saturating_add(1)
                    .checked_rem(Category::ALL.len())
                    .unwrap_or(0);
            }
            // F1 raises the list of keys in every program here, so the
            // frequency moved to F2 and the count to the vertical arrows.
            "F2" => {
                self.create_frequency_daily = !self.create_frequency_daily;
            }
            "Up" => {
                self.create_weekly_count = self.create_weekly_count.saturating_add(1).min(7);
            }
            "Down" => {
                self.create_weekly_count = self.create_weekly_count.saturating_sub(1).max(1);
            }
            "Backspace" => {
                self.create_name.pop();
            }
            // `key_name` names the space bar rather than passing its text, so
            // the arm below never saw one: "Read 20 pages" could not be typed.
            "Space" => self.type_into_name(" "),
            _ => self.type_into_name(key),
        }
    }

    /// Add one typed character to the new habit's name.
    fn type_into_name(&mut self, key: &str) {
        // One character, counted in characters: a limit in bytes cut a name
        // short at a third of its length in some scripts.
        if key.chars().count() == 1 && self.create_name.chars().count() < 40 {
            self.create_name.push_str(key);
        }
    }

    // ── Statistics helpers ───────────────────────────────────────────

    fn overall_completion_today(&self) -> (u32, u32) {
        let active = self.active_habits();
        let total = active.len() as u32;
        let mut done = 0u32;
        for &idx in &active {
            if let Some(h) = self.habits.get(idx)
                && h.is_checked_on(self.today)
            {
                done = done.saturating_add(1);
            }
        }
        (done, total)
    }

    fn best_habit_streak(&self) -> (String, u32) {
        let mut best_name = String::from("--");
        let mut best_val = 0u32;
        for h in &self.habits {
            if h.archived {
                continue;
            }
            let s = h.best_streak(self.today);
            if s > best_val {
                best_val = s;
                best_name = h.name.clone();
            }
        }
        (best_name, best_val)
    }

    fn average_completion_7d(&self) -> f32 {
        let active = self.active_habits();
        if active.is_empty() {
            return 0.0;
        }
        let sum: f32 = active
            .iter()
            .filter_map(|&i| self.habits.get(i))
            .map(|h| h.completion_rate(self.today, 7))
            .sum();
        sum / active.len() as f32
    }

    fn average_completion_30d(&self) -> f32 {
        let active = self.active_habits();
        if active.is_empty() {
            return 0.0;
        }
        let sum: f32 = active
            .iter()
            .filter_map(|&i| self.habits.get(i))
            .map(|h| h.completion_rate(self.today, 30))
            .sum();
        sum / active.len() as f32
    }

    // ── Heatmap data ────────────────────────────────────────────────

    /// Returns up to 365 days of check-in data for a habit.
    /// Each entry is (date, checked_in).
    fn heatmap_data(&self, habit_idx: usize, days: u32) -> Vec<(Date, bool)> {
        let mut result = Vec::with_capacity(days as usize);
        if let Some(h) = self.habits.get(habit_idx) {
            for i in (0..days).rev() {
                let d = self.today.add_days((i as i32).saturating_neg());
                result.push((d, h.is_checked_on(d)));
            }
        }
        result
    }

    /// The colour of one cell of the contribution graph.
    ///
    /// Had no caller until now: the renderer drew every checked day in flat
    /// `self.palette.green`, which makes a grid rather than a heatmap. A contribution graph
    /// says *how much*, and a two-colour one has thrown that away -- the
    /// module doc calls this a "Contribution/heatmap graph" and it was the
    /// half without the heat.
    fn heatmap_color(checked: bool, pal: &Palette, intensity: f32) -> Color {
        if !checked {
            return pal.surface0;
        }
        // Blend pal.green with intensity
        let alpha = (intensity * 255.0).clamp(80.0, 255.0) as u8;
        Color::rgba(166, 227, 161, alpha)
    }

    /// How strongly to colour each day of `data`.
    ///
    /// The run of consecutive checked days a day belongs to, scaled against
    /// the longest run on screen. A boolean habit has no count to shade by, so
    /// the thing worth seeing is momentum: a fortnight kept reads darker than
    /// three separate Tuesdays, which is the question a habit tracker is
    /// looked at to answer.
    ///
    /// Scaled against the longest run *shown* rather than a fixed ceiling, so
    /// the graph uses its whole range whether the best run is five days or
    /// fifty. A judgment call -- see `todo.txt`.
    fn heatmap_intensities(data: &[(Date, bool)]) -> Vec<f32> {
        let mut run = 0u32;
        let mut runs: Vec<u32> = Vec::with_capacity(data.len());
        for (_, checked) in data {
            run = if *checked { run.saturating_add(1) } else { 0 };
            runs.push(run);
        }
        let longest = runs.iter().copied().max().unwrap_or(0);
        if longest == 0 {
            return vec![0.0; data.len()];
        }
        runs.iter()
            .map(|&r| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a run length is bounded by the days on screen"
                )]
                let scaled = r as f32 / longest as f32;
                scaled
            })
            .collect()
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Route one event.
    ///
    /// **This app had no event handling of any kind until 2026-09-03.** Its
    /// `handle_key` takes a `&str` — a third key vocabulary beside
    /// `guitk::event::Key` and the wire's — and nothing in the program produced
    /// one, so every keystroke test handed itself the string it wanted. The
    /// same shape as `apps/paint` and `apps/soundrecorder`.
    ///
    /// Returns whether anything changed, which `App::on_event` turns into a
    /// repaint.
    fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Resize { width, height } => {
                #[allow(clippy::cast_precision_loss)]
                {
                    self.width = *width as f32;
                    self.height = *height as f32;
                }
                true
            }
            Event::Key(key) if key.pressed => {
                let Some(name) = Self::key_name(key) else {
                    return false;
                };
                self.handle_key(&name, key.modifiers.ctrl, key.modifiers.shift);
                true
            }
            // Midnight, or near enough to check.
            Event::Tick { .. } => {
                let Some(today) = today_from_clock() else {
                    return false;
                };
                if today == self.today {
                    return false;
                }
                self.today = today;
                true
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => false,
        }
    }

    /// The string this app's handlers speak, for a toolkit key.
    ///
    /// Translating at the seam rather than converting the handlers, for the
    /// same reason `apps/paint` does: the handlers' meaning is "the user typed
    /// `n`", which is a different question from "the N key went down", and the
    /// two coincide only on a US layout. When `TD-ONLY-ONE-KEYBOARD-LAYOUT` is
    /// closed this is the one function that has to learn about layouts.
    ///
    /// `text` first, so a shifted key and a dead-key sequence arrive correctly
    /// spelled; the named keys are the ones that produce no text at all.
    fn key_name(key: &KeyEvent) -> Option<String> {
        let named = match key.key {
            Key::Up => "Up",
            Key::Down => "Down",
            Key::Left => "Left",
            Key::Right => "Right",
            Key::Space => "Space",
            Key::Escape => "Escape",
            Key::Enter => "Return",
            Key::Tab => "Tab",
            Key::Backspace => "Backspace",
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::F1 => "F1",
            Key::F2 => "F2",
            _ => {
                // Anything else is only interesting as the character it typed.
                let typed = key.text.chars().next()?;
                return Some(typed.to_string());
            }
        };
        Some(named.to_string())
    }

    /// The drawn commands. **Tests only**: the window's `render` takes the
    /// frame itself, because it keeps the frame's boxes for the pointer.
    ///
    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`.
    #[cfg(test)]
    fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame().into_tree().commands
    }

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    fn frame(&self) -> Frame<Target> {
        let mut cmds = Frame::new(self.width, self.height);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds);
        self.render_nav(&mut cmds);

        let content_y = Self::HEADER_H + Self::NAV_H;

        match self.screen {
            Screen::Dashboard => self.render_dashboard(&mut cmds, content_y),
            Screen::Statistics => self.render_statistics(&mut cmds, content_y),
            Screen::Archive => self.render_archive(&mut cmds, content_y),
            Screen::HeatMap => self.render_heatmap_screen(&mut cmds, content_y),
        }

        self.render_status(&mut cmds);

        if self.show_create_form {
            self.render_create_form(&mut cmds);
        }
        if self.pending_delete.is_some() {
            self.render_delete_confirm(&mut cmds);
        }
        // Over everything, because it is the one thing a reader asked for.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut cmds,
                &self.palette,
                (self.width, self.height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
            cmds.hit(
                Target::HelpCard,
                Rect::new(0.0, 0.0, self.width, self.height),
            );
        }

        cmds
    }

    /// A small button, lit while the pointer is on it.
    fn button(&self, cmds: &mut Frame<Target>, rect: Rect, label: &str, target: Target) {
        self.palette.push_surface(
            cmds,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            6.0,
            if self.hover == Some(target) {
                Surface::Selected
            } else {
                Surface::Card
            },
        );
        cmds.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - 11.0) / 2.0,
            text: label.to_string(),
            font_size: 11.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.hit(target, rect);
    }

    fn render_header(&self, cmds: &mut Frame<Target>) {
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            self.width,
            Self::HEADER_H,
            0.0,
            Surface::Card,
        );

        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 14.0,
            text: String::from("\u{1F4CB} Habit Tracker"),
            font_size: 20.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Date display
        cmds.push(RenderCommand::Text {
            x: 240.0,
            y: 18.0,
            // The weekday once; it was written at both ends of the line.
            text: format!(
                "{} {}",
                self.today.day_of_week_short(),
                self.today.format_full()
            ),
            font_size: 14.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(280.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Today's progress
        let (done, total) = self.overall_completion_today();
        let progress_text = format!("Today: {done}/{total}");
        cmds.push(RenderCommand::Text {
            x: self.width - 200.0,
            y: 10.0,
            text: progress_text,
            font_size: 16.0,
            color: self.palette.ink(self.palette.green),
            font_weight: FontWeightHint::Bold,
            max_width: Some(180.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Progress bar
        let bar_x = self.width - 200.0;
        let bar_y = 32.0;
        let bar_w = 160.0;
        let bar_h = 8.0;
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: self.palette.surface0,
            corner_radii: CornerRadii::all(4.0),
        });
        if total > 0 {
            let fill = bar_w * (done as f32 / total as f32);
            if fill > 0.5 {
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: fill,
                    height: bar_h,
                    color: self.palette.green,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        // New habit button
        let btn_x = 560.0;
        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: 10.0,
            width: 90.0,
            height: 30.0,
            color: if self.hover == Some(Target::NewHabit) {
                self.palette.sapphire
            } else {
                self.palette.blue
            },
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.hit(Target::NewHabit, Rect::new(btn_x, 10.0, 90.0, 30.0));
        cmds.push(RenderCommand::Text {
            x: btn_x + 10.0,
            y: 17.0,
            text: String::from("+ New Habit"),
            font_size: 12.0,
            color: self.palette.crust,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_nav(&self, cmds: &mut Frame<Target>) {
        let y = Self::HEADER_H;
        self.palette
            .push_surface(cmds, 0.0, y, self.width, Self::NAV_H, 0.0, Surface::Sidebar);

        let tabs = [
            Screen::Dashboard,
            Screen::Statistics,
            Screen::Archive,
            Screen::HeatMap,
        ];
        let mut tx = 16.0;
        for (i, tab) in tabs.iter().enumerate() {
            let selected = *tab == self.screen;
            let bg = if selected {
                self.palette.blue
            } else {
                self.palette.surface1
            };
            let fg = if selected {
                self.palette.crust
            } else {
                self.palette.text
            };
            let w = 90.0;
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 4.0,
                width: w,
                height: 28.0,
                color: if !selected && self.hover == Some(Target::Tab(*tab)) {
                    self.palette.surface2
                } else {
                    bg
                },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.hit(Target::Tab(*tab), Rect::new(tx, y + 4.0, w, 28.0));
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 10.0,
                text: format!("{} {}", i.saturating_add(1), tab.label()),
                font_size: 11.0,
                color: fg,
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            tx += w + 6.0;
        }

        // The category filter: a chip that steps through them on a press,
        // as C does -- drawn when no filter is set too, or there would be
        // nothing to press to set one.
        let chip = Rect::new(tx + 20.0, y + 6.0, 140.0, 24.0);
        let (fill, label, ink) = match self.category_filter {
            Some(cat) => (
                cat.color(&self.palette),
                format!("{} {}", cat.icon(), cat.label()),
                self.palette.crust,
            ),
            None => (
                if self.hover == Some(Target::FilterChip) {
                    self.palette.surface2
                } else {
                    self.palette.surface1
                },
                String::from("All categories"),
                self.palette.text,
            ),
        };
        cmds.push(RenderCommand::FillRect {
            x: chip.x,
            y: chip.y,
            width: chip.w,
            height: chip.h,
            color: fill,
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::Text {
            x: chip.x + 10.0,
            y: y + 10.0,
            text: label,
            font_size: 11.0,
            color: ink,
            font_weight: FontWeightHint::Bold,
            max_width: Some(chip.w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.hit(Target::FilterChip, chip);
    }

    fn render_dashboard(&self, cmds: &mut Frame<Target>, start_y: f32) {
        let active = self.active_habits();
        if active.is_empty() {
            // Where the habits would be. The notice was drawn at the top of
            // the window before the header, which painted over it.
            let lines: &[&str] = if self.habits.is_empty() {
                &NO_HABITS_LINES
            } else {
                &["No habits in this category."]
            };
            for (i, line) in lines.iter().enumerate() {
                cmds.push(RenderCommand::Text {
                    x: 24.0,
                    y: start_y + 60.0 + i as f32 * 24.0,
                    text: (*line).to_string(),
                    font_size: if i == 0 { 16.0 } else { 12.0 },
                    color: if i == 0 {
                        self.palette.text
                    } else {
                        self.palette.subtext0
                    },
                    font_weight: if i == 0 {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(self.width - 48.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            return;
        }

        // Column headers: day labels
        let name_col_w = 200.0;
        let days_start_x = name_col_w + 80.0;
        for col in 0..7 {
            let d = self.today.add_days((col as i32).saturating_neg());
            let cx = days_start_x + (6i32.saturating_sub(col as i32)) as f32 * Self::DAY_COL_W;
            let label = if col == 0 {
                String::from("Today")
            } else {
                d.day_of_week_short().to_string()
            };
            let label_color = if col == self.selected_day_col {
                self.palette.blue
            } else {
                self.palette.subtext0
            };
            cmds.push(RenderCommand::Text {
                x: cx + 2.0,
                y: start_y + 6.0,
                text: label,
                font_size: 10.0,
                color: label_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(Self::DAY_COL_W),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: cx + 6.0,
                y: start_y + 18.0,
                text: format!("{:02}", d.day),
                font_size: 9.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(Self::DAY_COL_W),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Streak/rate headers
        let stats_x = days_start_x + 7.0 * Self::DAY_COL_W + 12.0;
        cmds.push(RenderCommand::Text {
            x: stats_x,
            y: start_y + 8.0,
            text: String::from("Streak"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(50.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: stats_x + 56.0,
            y: start_y + 8.0,
            text: String::from("7d"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(30.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: stats_x + 90.0,
            y: start_y + 8.0,
            text: String::from("30d"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(30.0),
            overflow: TextOverflow::Ellipsis,
        });

        let row_start_y = start_y + 32.0 - self.scroll_offset;
        // The rows' pane: the wheel scrolls it, and its clip keeps a row
        // scrolled under the column heads from being drawn or pressed there.
        let pane = self.list_rect();
        cmds.hit(Target::HabitList, pane);
        cmds.clip(pane);

        for (vi, &habit_idx) in active.iter().enumerate() {
            let Some(habit) = self.habits.get(habit_idx) else {
                continue;
            };
            let ry = row_start_y + vi as f32 * Self::ROW_H;

            if ry + Self::ROW_H < start_y || ry > self.height - Self::STATUS_H {
                continue; // clipping
            }
            cmds.hit(
                Target::HabitRow(habit_idx),
                Rect::new(8.0, ry, self.width - 16.0, Self::ROW_H - 2.0),
            );

            let is_selected = vi == self.selected_habit;
            let row_bg = if is_selected {
                self.palette.surface1
            } else if vi % 2 == 0 {
                self.palette.surface0
            } else {
                self.palette.base
            };

            cmds.push(RenderCommand::FillRect {
                x: 8.0,
                y: ry,
                width: self.width - 16.0,
                height: Self::ROW_H - 2.0,
                color: row_bg,
                corner_radii: CornerRadii::all(6.0),
            });

            // Category color dot
            cmds.push(RenderCommand::FillRect {
                x: 16.0,
                y: ry + 16.0,
                width: 10.0,
                height: 10.0,
                color: habit.category.color(&self.palette),
                corner_radii: CornerRadii::all(5.0),
            });

            // Habit name
            cmds.push(RenderCommand::Text {
                x: 32.0,
                y: ry + 8.0,
                text: habit.name.clone(),
                font_size: 14.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(160.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Frequency label
            cmds.push(RenderCommand::Text {
                x: 32.0,
                y: ry + 28.0,
                text: habit.frequency.label(),
                font_size: 10.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Check-in dots for last 7 days
            for col in 0..7 {
                let d = self.today.add_days((col as i32).saturating_neg());
                let cx = days_start_x + (6i32.saturating_sub(col as i32)) as f32 * Self::DAY_COL_W;
                let checked = habit.is_checked_on(d);
                let cell_selected = is_selected && col == self.selected_day_col;

                let dot_color = if checked {
                    self.palette.green
                } else {
                    self.palette.surface2
                };
                let dot_size = if cell_selected { 22.0 } else { 18.0 };
                let dot_x = cx + (Self::DAY_COL_W - dot_size) / 2.0;
                let dot_y = ry + (Self::ROW_H - 2.0 - dot_size) / 2.0;
                // The day's cell checks it in, or out.
                cmds.hit(
                    Target::DayCell(habit_idx, col),
                    Rect::new(cx, ry, Self::DAY_COL_W, Self::ROW_H - 2.0),
                );

                cmds.push(RenderCommand::FillRect {
                    x: dot_x,
                    y: dot_y,
                    width: dot_size,
                    height: dot_size,
                    color: dot_color,
                    corner_radii: CornerRadii::all(dot_size / 2.0),
                });

                if checked {
                    cmds.push(RenderCommand::Text {
                        x: dot_x + 3.0,
                        y: dot_y + 2.0,
                        text: String::from("\u{2713}"),
                        font_size: 12.0,
                        color: self.palette.crust,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(dot_size),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                if cell_selected {
                    cmds.push(RenderCommand::StrokeRect {
                        x: dot_x - 2.0,
                        y: dot_y - 2.0,
                        width: dot_size + 4.0,
                        height: dot_size + 4.0,
                        color: self.palette.blue,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(dot_size.midpoint(4.0)),
                    });
                }
            }

            // Streak
            let streak = habit.current_streak(self.today);
            let streak_color = if streak > 0 {
                self.palette.peach
            } else {
                self.palette.overlay0
            };
            cmds.push(RenderCommand::Text {
                x: stats_x,
                y: ry + 16.0,
                text: format!("{streak}"),
                font_size: 14.0,
                color: streak_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // 7-day rate
            let rate_7 = habit.completion_rate(self.today, 7);
            // Inked at the binding: this local is used once, and it is a
            // label. `rate_color` also fills the stat cards, so it cannot be
            // inked in its own body. 837.
            let rate_7_color = self.palette.ink(rate_color(rate_7, &self.palette));
            cmds.push(RenderCommand::Text {
                x: stats_x + 50.0,
                y: ry + 16.0,
                text: format!("{}%", (rate_7 * 100.0) as u32),
                font_size: 12.0,
                color: rate_7_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // 30-day rate
            let rate_30 = habit.completion_rate(self.today, 30);
            let rate_30_color = self.palette.ink(rate_color(rate_30, &self.palette));
            cmds.push(RenderCommand::Text {
                x: stats_x + 86.0,
                y: ry + 16.0,
                text: format!("{}%", (rate_30 * 100.0) as u32),
                font_size: 12.0,
                color: rate_30_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Archiving, from the row it is about. It was `A`, and only `A`.
            let archive = Rect::new(
                (self.width - 16.0 - 80.0).max(stats_x + 130.0),
                ry + 12.0,
                72.0,
                24.0,
            );
            self.button(cmds, archive, "Archive", Target::ArchiveButton(habit_idx));
        }
        cmds.unclip();
    }

    /// Where the dashboard's rows are drawn: under the day heads, over the
    /// status bar.
    fn list_rect(&self) -> Rect {
        let top = Self::HEADER_H + Self::NAV_H + 30.0;
        Rect::new(
            0.0,
            top,
            self.width,
            (self.height - Self::STATUS_H - top).max(0.0),
        )
    }

    /// Where the archive's rows are drawn: under its title, over the status
    /// bar.
    fn archive_rect(&self, start_y: f32) -> Rect {
        let top = start_y + 40.0;
        Rect::new(
            0.0,
            top,
            self.width,
            (self.height - Self::STATUS_H - top).max(0.0),
        )
    }

    /// Where the statistics table's title sits: under the four cards.
    fn stats_table_y(start_y: f32) -> f32 {
        start_y + STATS_PAD * 3.0 + STATS_CARD_H * 2.0 + 10.0
    }

    /// Where the statistics table's rows are drawn: under its column heads,
    /// over the status bar.
    fn stats_rect(&self, start_y: f32) -> Rect {
        let top = Self::stats_table_y(start_y) + 40.0;
        Rect::new(
            0.0,
            top,
            self.width,
            (self.height - Self::STATUS_H - top).max(0.0),
        )
    }

    /// The list the current screen shows: where its rows are drawn, how tall
    /// each is, and how many there are. `None` on the graph, which has none.
    ///
    /// The archive and the statistics table stopped drawing at the window's
    /// bottom edge and had no scrolling, so a habit past it could not be seen
    /// there, nor restored, nor chosen.
    fn list_pane(&self) -> Option<(Rect, f32, usize)> {
        let content_y = Self::HEADER_H + Self::NAV_H;
        match self.screen {
            Screen::Dashboard => Some((self.list_rect(), Self::ROW_H, self.active_habits().len())),
            Screen::Archive => Some((
                self.archive_rect(content_y),
                Self::ROW_H,
                self.archived_habits().len(),
            )),
            Screen::Statistics => Some((
                self.stats_rect(content_y),
                STATS_ROW_H,
                self.active_habits().len(),
            )),
            Screen::HeatMap => None,
        }
    }

    /// How far the list on screen can scroll: its rows past the room they
    /// have. Page Down was capped at 2000 pixels whatever the list, so it
    /// scrolled a list that fitted away into empty space.
    fn scroll_limit(&self) -> f32 {
        self.list_pane().map_or(0.0, |(pane, row_h, rows)| {
            (rows as f32 * row_h + LIST_END_PAD - pane.h).max(0.0)
        })
    }

    /// Keep the chosen habit in its list and on screen, and the scroll in
    /// range.
    ///
    /// The list had an offset only Page Up and Page Down moved, so the arrows
    /// walked the selection off the bottom. And nothing moved the selection
    /// when its list shrank -- archiving the last row left it past the end,
    /// choosing nothing.
    fn keep_selection_visible(&mut self) {
        let active = self.active_habits().len();
        self.heatmap_habit_idx = self.heatmap_habit_idx.min(active.saturating_sub(1));
        if let Some((pane, row_h, rows)) = self.list_pane()
            && matches!(self.screen, Screen::Dashboard | Screen::Archive)
        {
            self.selected_habit = self.selected_habit.min(rows.saturating_sub(1));
            let top = self.selected_habit as f32 * row_h;
            let bottom = top + row_h + LIST_END_PAD;
            if top < self.scroll_offset {
                self.scroll_offset = top;
            } else if bottom > self.scroll_offset + pane.h {
                self.scroll_offset = bottom - pane.h;
            }
        }
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.scroll_limit());
    }

    fn render_statistics(&self, cmds: &mut Frame<Target>, start_y: f32) {
        let pad = STATS_PAD;
        let card_w = (self.width - pad * 3.0) / 2.0;
        let card_h = STATS_CARD_H;

        // Card 1: Overall today
        let (done, total) = self.overall_completion_today();
        self.render_stat_card(
            cmds,
            pad,
            start_y + pad,
            card_w,
            card_h,
            "Today's Progress",
            &format!("{done} / {total}"),
            if total > 0 && done == total {
                self.palette.green
            } else {
                self.palette.blue
            },
        );

        // Card 2: Best streak
        let (best_name, best_val) = self.best_habit_streak();
        self.render_stat_card(
            cmds,
            pad * 2.0 + card_w,
            start_y + pad,
            card_w,
            card_h,
            "Best Streak",
            &format!("{best_val} ({best_name})"),
            self.palette.peach,
        );

        // Card 3: 7-day avg
        let avg_7 = self.average_completion_7d();
        self.render_stat_card(
            cmds,
            pad,
            start_y + pad * 2.0 + card_h,
            card_w,
            card_h,
            "7-Day Average",
            &format!("{}%", (avg_7 * 100.0) as u32),
            rate_color(avg_7, &self.palette),
        );

        // Card 4: 30-day avg
        let avg_30 = self.average_completion_30d();
        self.render_stat_card(
            cmds,
            pad * 2.0 + card_w,
            start_y + pad * 2.0 + card_h,
            card_w,
            card_h,
            "30-Day Average",
            &format!("{}%", (avg_30 * 100.0) as u32),
            rate_color(avg_30, &self.palette),
        );

        // Per-habit stats table
        let table_y = Self::stats_table_y(start_y);
        cmds.push(RenderCommand::Text {
            x: pad,
            y: table_y,
            text: String::from("Per-Habit Statistics"),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        let table = Table::with_gap(STATS_COLUMNS, pad, STATS_GAP);
        cmds.draw_with(|c| {
            table.header(c, table_y + 24.0, self.palette.subtext0, STATS_HEADER_FONT);
        });

        cmds.push(RenderCommand::Line {
            x1: pad,
            y1: table_y + 38.0,
            x2: self.width - pad,
            y2: table_y + 38.0,
            color: self.palette.surface1,
            width: 1.0,
        });

        // The rows scroll under the column heads. They stopped at the bottom
        // edge, so a habit past it had no statistics anywhere.
        let pane = self.stats_rect(start_y);
        cmds.hit(Target::StatsList, pane);
        cmds.clip(pane);
        let active = self.active_habits();
        for (vi, &idx) in active.iter().enumerate() {
            let Some(h) = self.habits.get(idx) else {
                continue;
            };
            let row = Rect::new(
                pad,
                pane.y + vi as f32 * STATS_ROW_H - self.scroll_offset,
                self.width - pad * 2.0,
                STATS_ROW_H,
            );
            if cmds.visible_part(row).is_none() {
                continue;
            }
            let ry = row.y + 4.0;

            // A habit name is user-entered, so it is the one cell here with no
            // bound on its length. It was clipped with no marker, which reads
            // as a complete name that happens to be terse -- the reader had no
            // way to tell "Read" from "Read thirty minutes before bed". Cells
            // are cut at the end (`Fit::Start`): a habit name is a label the
            // user wrote, and a label identifies itself by how it begins.
            let r7 = h.completion_rate(self.today, 7);
            let r30 = h.completion_rate(self.today, 30);
            let ra = h.completion_rate_alltime(self.today);
            let cells: [(usize, String, Color); 8] = [
                (STATS_HABIT, h.name.clone(), self.palette.text),
                (
                    STATS_CATEGORY,
                    h.category.label().to_string(),
                    h.category.color(&self.palette),
                ),
                (
                    STATS_STREAK,
                    format!("{}", h.current_streak(self.today)),
                    self.palette.peach,
                ),
                (
                    STATS_BEST,
                    format!("{}", h.best_streak(self.today)),
                    self.palette.yellow,
                ),
                (
                    STATS_7D,
                    format!("{}%", (r7 * 100.0) as u32),
                    rate_color(r7, &self.palette),
                ),
                (
                    STATS_30D,
                    format!("{}%", (r30 * 100.0) as u32),
                    rate_color(r30, &self.palette),
                ),
                (
                    STATS_ALL,
                    format!("{}%", (ra * 100.0) as u32),
                    rate_color(ra, &self.palette),
                ),
                (
                    STATS_TOTAL,
                    format!("{}", h.total_check_ins()),
                    self.palette.text,
                ),
            ];
            debug_assert_eq!(
                cells.len(),
                table.len(),
                "a cell with no column is positioned past the table and drawn empty",
            );
            cmds.draw_with(|c| {
                for (index, text, color) in &cells {
                    table.cell(c, *index, ry, text, *color, STATS_ROW_FONT, Fit::Start);
                }
            });
            // A row opens its habit's graph.
            cmds.hit(Target::StatsRow(idx), row);
        }
        cmds.unclip();
    }

    // Stat-card render takes self + cmds + rect (x,y,w,h) + 2 labels + accent
    // color. Grouping would not improve clarity.
    #[allow(clippy::too_many_arguments)]
    fn render_stat_card(
        &self,
        cmds: &mut Frame<Target>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        title: &str,
        value: &str,
        accent: Color,
    ) {
        self.palette
            .push_surface(cmds, x, y, w, h, 8.0, Surface::Card);
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 4.0,
            height: h,
            color: accent,
            corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 14.0,
            text: title.to_string(),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 40.0,
            text: value.to_string(),
            font_size: 24.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_archive(&self, cmds: &mut Frame<Target>, start_y: f32) {
        let archived = self.archived_habits();

        if archived.is_empty() {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 120.0,
                y: start_y + 80.0,
                text: String::from("No archived habits. Archive one from its row, or with A."),
                font_size: 16.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: start_y + 12.0,
            text: format!("Archived Habits ({})", archived.len()),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // The rows scroll. They stopped at the bottom edge, so a habit
        // archived past it could not be restored.
        let pane = self.archive_rect(start_y);
        cmds.hit(Target::ArchiveList, pane);
        cmds.clip(pane);
        for (vi, &idx) in archived.iter().enumerate() {
            let Some(h) = self.habits.get(idx) else {
                continue;
            };
            let ry = pane.y + 2.0 + vi as f32 * Self::ROW_H - self.scroll_offset;
            if cmds
                .visible_part(Rect::new(16.0, ry, self.width - 32.0, Self::ROW_H))
                .is_none()
            {
                continue;
            }

            let is_selected = vi == self.selected_habit;
            let bg = if is_selected {
                self.palette.surface1
            } else {
                self.palette.surface0
            };

            cmds.push(RenderCommand::FillRect {
                x: 16.0,
                y: ry,
                width: self.width - 32.0,
                height: Self::ROW_H - 4.0,
                color: bg,
                corner_radii: CornerRadii::all(6.0),
            });

            // Category dot
            cmds.push(RenderCommand::FillRect {
                x: 28.0,
                y: ry + 16.0,
                width: 10.0,
                height: 10.0,
                color: h.category.color(&self.palette),
                corner_radii: CornerRadii::all(5.0),
            });

            cmds.push(RenderCommand::Text {
                x: 46.0,
                y: ry + 8.0,
                text: h.name.clone(),
                font_size: 14.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: 46.0,
                y: ry + 28.0,
                text: format!(
                    "{} -- {} check-ins",
                    h.category.label(),
                    h.total_check_ins()
                ),
                font_size: 10.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.hit(
                Target::ArchiveRow(idx),
                Rect::new(16.0, ry, self.width - 32.0, Self::ROW_H - 4.0),
            );
            // Restoring, from the row: it was Enter on the selected row only.
            let restore = Rect::new(self.width - 32.0 - 180.0, ry + 11.0, 80.0, 24.0);
            self.button(cmds, restore, "Restore", Target::RestoreButton(idx));
            // Deleting, which asks first: a habit's record goes with it.
            let delete = Rect::new(self.width - 32.0 - 90.0, ry + 11.0, 80.0, 24.0);
            self.button(cmds, delete, "Delete", Target::DeleteButton(idx));
        }
        cmds.unclip();
    }

    /// The question before a delete: a card with the habit's name, what goes
    /// with it, and a button for each answer.
    fn render_delete_confirm(&self, cmds: &mut Frame<Target>) {
        let Some(habit) = self.pending_delete.and_then(|i| self.habits.get(i)) else {
            return;
        };
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });
        cmds.hit(
            Target::ConfirmBackdrop,
            Rect::new(0.0, 0.0, self.width, self.height),
        );
        let card = Rect::new(
            (self.width - CONFIRM_W) / 2.0,
            (self.height - CONFIRM_H) / 2.0,
            CONFIRM_W,
            CONFIRM_H,
        );
        let mut paint = self.palette.surface_paint(Surface::Card);
        paint.border = Some(paint.border.unwrap_or(self.palette.surface1));
        self.palette.push_paint_radii(
            cmds,
            card.x,
            card.y,
            card.w,
            card.h,
            CornerRadii::all(12.0),
            paint,
        );
        cmds.hit(Target::ConfirmCard, card);
        cmds.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 20.0,
            text: format!("Delete {}?", habit.name),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(card.w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 52.0,
            text: format!(
                "Its {} check-in(s) go with it, and this cannot be undone.",
                habit.total_check_ins()
            ),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card.w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
        let delete = Rect::new(
            card.right() - 20.0 - 200.0,
            card.bottom() - 52.0,
            110.0,
            32.0,
        );
        cmds.push(RenderCommand::FillRect {
            x: delete.x,
            y: delete.y,
            width: delete.w,
            height: delete.h,
            color: self.palette.red,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: delete.x + 12.0,
            y: delete.y + 9.0,
            text: String::from("Delete (Y)"),
            font_size: 12.0,
            color: self.palette.crust,
            font_weight: FontWeightHint::Bold,
            max_width: Some(delete.w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.hit(Target::ConfirmDelete, delete);
        let keep = Rect::new(card.right() - 20.0 - 80.0, card.bottom() - 52.0, 80.0, 32.0);
        self.button(cmds, keep, "Keep", Target::KeepHabit);
    }

    fn render_heatmap_screen(&self, cmds: &mut Frame<Target>, start_y: f32) {
        let active = self.active_habits();
        if active.is_empty() {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 80.0,
                y: start_y + 80.0,
                text: String::from("No habits to display."),
                font_size: 16.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let habit_idx = active
            .get(self.heatmap_habit_idx.min(active.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        let Some(habit) = self.habits.get(habit_idx) else {
            return;
        };

        // Title
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: start_y + 12.0,
            text: format!("Contribution Graph: {}", habit.name),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Another habit: the arrows, or these.
        let prev = Rect::new(20.0, start_y + 30.0, 96.0, 22.0);
        let next = Rect::new(122.0, start_y + 30.0, 96.0, 22.0);
        self.button(cmds, prev, "< Previous", Target::GraphPrev);
        self.button(cmds, next, "Next >", Target::GraphNext);

        // Heatmap: 52 weeks x 7 days
        let data = self.heatmap_data(habit_idx, 364);
        let hx_start = 60.0;
        let hy_start = start_y + 60.0;
        let cell = Self::HEATMAP_CELL;
        let gap = Self::HEATMAP_GAP;

        // Day labels (Mon, Wed, Fri)
        let day_labels = ["Mon", "", "Wed", "", "Fri", "", "Sun"];
        for (di, label) in day_labels.iter().enumerate() {
            if !label.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: 16.0,
                    y: hy_start + di as f32 * (cell + gap) + 1.0,
                    text: label.to_string(),
                    font_size: 9.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(40.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // Draw cells
        let intensities = Self::heatmap_intensities(&data);
        for (i, (_, checked)) in data.iter().enumerate() {
            let week = i / 7;
            let day = i % 7;
            let cx = hx_start + week as f32 * (cell + gap);
            let cy = hy_start + day as f32 * (cell + gap);

            if cx + cell > self.width - 20.0 {
                break;
            }

            let color = Self::heatmap_color(
                *checked,
                &self.palette,
                intensities.get(i).copied().unwrap_or(1.0),
            );
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: cell,
                height: cell,
                color,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Month labels along the top
        let mut last_month = 0u32;
        for (i, (date, _)) in data.iter().enumerate() {
            let week = i / 7;
            let day = i % 7;
            if day == 0 && date.month != last_month {
                last_month = date.month;
                let mx = hx_start + week as f32 * (cell + gap);
                if mx + 30.0 < self.width - 20.0 {
                    cmds.push(RenderCommand::Text {
                        x: mx,
                        y: hy_start - 14.0,
                        text: date.month_short().to_string(),
                        font_size: 9.0,
                        color: self.palette.subtext0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(30.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
        }

        // Legend
        let ly = hy_start + 7.0 * (cell + gap) + 16.0;
        cmds.push(RenderCommand::Text {
            x: hx_start,
            y: ly,
            text: String::from("Less"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
            overflow: TextOverflow::Ellipsis,
        });
        // Sampled from the same function the cells are drawn with, so the key
        // cannot drift from the thing it is a key to. It used to be four
        // hand-written colours -- and it was the only honest part of the
        // screen, promising a "Less -> More" gradient the graph did not draw.
        let legend_colors = [
            self.palette.surface0,
            Self::heatmap_color(true, &self.palette, 0.0),
            Self::heatmap_color(true, &self.palette, 0.5),
            Self::heatmap_color(true, &self.palette, 1.0),
        ];
        for (li, lc) in legend_colors.iter().enumerate() {
            cmds.push(RenderCommand::FillRect {
                x: hx_start + 36.0 + li as f32 * (cell + 2.0),
                y: ly,
                width: cell,
                height: cell,
                color: *lc,
                corner_radii: CornerRadii::all(2.0),
            });
        }
        cmds.push(RenderCommand::Text {
            x: hx_start + 36.0 + 4.0 * (cell + 2.0) + 4.0,
            y: ly,
            text: String::from("More"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Stats summary below heatmap
        let sy = ly + 36.0;
        let streak = habit.current_streak(self.today);
        let best = habit.best_streak(self.today);
        let r7 = habit.completion_rate(self.today, 7);
        let r30 = habit.completion_rate(self.today, 30);
        let ra = habit.completion_rate_alltime(self.today);

        let stats_items = [
            (format!("Current Streak: {streak}"), self.palette.peach),
            (format!("Best Streak: {best}"), self.palette.yellow),
            (
                format!("7-day: {}%", (r7 * 100.0) as u32),
                rate_color(r7, &self.palette),
            ),
            (
                format!("30-day: {}%", (r30 * 100.0) as u32),
                rate_color(r30, &self.palette),
            ),
            (
                format!("All-time: {}%", (ra * 100.0) as u32),
                rate_color(ra, &self.palette),
            ),
            (
                format!("Total check-ins: {}", habit.total_check_ins()),
                self.palette.text,
            ),
        ];

        for (si, (text, color)) in stats_items.iter().enumerate() {
            let col = si % 3;
            let row = si / 3;
            cmds.push(RenderCommand::Text {
                x: 20.0 + col as f32 * 200.0,
                y: sy + row as f32 * 22.0,
                text: text.clone(),
                font_size: 12.0,
                color: *color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(190.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_create_form(&self, cmds: &mut Frame<Target>) {
        // Modal overlay, which takes every press that misses the form.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });
        cmds.hit(
            Target::FormBackdrop,
            Rect::new(0.0, 0.0, self.width, self.height),
        );

        let fw = 400.0;
        let fh = 300.0;
        let fx = (self.width - fw) / 2.0;
        let fy = (self.height - fh) / 2.0;

        let mut paint = self.palette.surface_paint(Surface::Card);
        paint.border = Some(paint.border.unwrap_or(self.palette.surface1));
        self.palette
            .push_paint_radii(cmds, fx, fy, fw, fh, CornerRadii::all(12.0), paint);
        cmds.hit(Target::FormBackdrop, Rect::new(fx, fy, fw, fh));

        cmds.push(RenderCommand::Text {
            x: fx + 20.0,
            y: fy + 16.0,
            text: String::from("New Habit"),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Name field
        cmds.push(RenderCommand::Text {
            x: fx + 20.0,
            y: fy + 54.0,
            text: String::from("Name:"),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.palette
            .push_surface(cmds, fx + 80.0, fy + 48.0, 290.0, 28.0, 4.0, Surface::Card);
        let display_name = if self.create_name.is_empty() {
            String::from("Type a name...")
        } else {
            self.create_name.clone()
        };
        let name_color = if self.create_name.is_empty() {
            self.palette.overlay0
        } else {
            self.palette.text
        };
        cmds.push(RenderCommand::Text {
            x: fx + 88.0,
            y: fy + 54.0,
            text: display_name,
            font_size: 12.0,
            color: name_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(270.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Category
        let cat = Category::ALL
            .get(self.create_category_idx)
            .copied()
            .unwrap_or(Category::Custom);
        cmds.push(RenderCommand::Text {
            x: fx + 20.0,
            y: fy + 94.0,
            text: String::from("Category:"),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::FillRect {
            x: fx + 110.0,
            y: fy + 88.0,
            width: 140.0,
            height: 24.0,
            color: cat.color(&self.palette),
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::Text {
            x: fx + 120.0,
            y: fy + 92.0,
            text: format!("{} {} (Tab)", cat.icon(), cat.label()),
            font_size: 11.0,
            color: self.palette.crust,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });
        // A press on the category moves to the next, as Tab does.
        cmds.hit(
            Target::FormCategory,
            Rect::new(fx + 110.0, fy + 88.0, 140.0, 24.0),
        );

        // Frequency
        cmds.push(RenderCommand::Text {
            x: fx + 20.0,
            y: fy + 134.0,
            text: String::from("Frequency:"),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
        // Daily or weekly, and how many times a week: buttons, with their
        // keys named on them. The frequency was F1, which is the list of keys
        // in every other program here.
        let daily = Rect::new(fx + 110.0, fy + 128.0, 70.0, 24.0);
        let weekly = Rect::new(fx + 186.0, fy + 128.0, 70.0, 24.0);
        for (rect, label, on, target) in [
            (
                daily,
                "Daily",
                self.create_frequency_daily,
                Target::FormDaily,
            ),
            (
                weekly,
                "Weekly",
                !self.create_frequency_daily,
                Target::FormWeekly,
            ),
        ] {
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: if on {
                    self.palette.blue
                } else if self.hover == Some(target) {
                    self.palette.surface2
                } else {
                    self.palette.surface1
                },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: rect.x + 10.0,
                y: rect.y + 5.0,
                text: label.to_string(),
                font_size: 12.0,
                color: if on {
                    self.palette.crust
                } else {
                    self.palette.text
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(rect.w - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.hit(target, rect);
        }
        cmds.push(RenderCommand::Text {
            x: fx + 264.0,
            y: fy + 134.0,
            text: String::from("F2"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(30.0),
            overflow: TextOverflow::Ellipsis,
        });
        if !self.create_frequency_daily {
            let minus = Rect::new(fx + 110.0, fy + 164.0, 28.0, 24.0);
            let plus = Rect::new(fx + 186.0, fy + 164.0, 28.0, 24.0);
            self.button(cmds, minus, "-", Target::FormFewer);
            self.button(cmds, plus, "+", Target::FormMore);
            cmds.push(RenderCommand::Text {
                x: fx + 146.0,
                y: fy + 169.0,
                text: format!("{}x", self.create_weekly_count),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(36.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: fx + 222.0,
                y: fy + 169.0,
                text: String::from("a week  (Up / Down)"),
                font_size: 10.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Buttons
        cmds.push(RenderCommand::FillRect {
            x: fx + 100.0,
            y: fy + fh - 60.0,
            width: 90.0,
            height: 32.0,
            color: self.palette.green,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.hit(
            Target::FormCreate,
            Rect::new(fx + 100.0, fy + fh - 60.0, 90.0, 32.0),
        );
        cmds.hit(
            Target::FormCancel,
            Rect::new(fx + 210.0, fy + fh - 60.0, 90.0, 32.0),
        );
        cmds.push(RenderCommand::Text {
            x: fx + 118.0,
            y: fy + fh - 52.0,
            text: String::from("Create"),
            font_size: 13.0,
            color: self.palette.crust,
            font_weight: FontWeightHint::Bold,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });

        self.palette.push_surface(
            cmds,
            fx + 210.0,
            fy + fh - 60.0,
            90.0,
            32.0,
            6.0,
            Surface::Card,
        );
        cmds.push(RenderCommand::Text {
            x: fx + 228.0,
            y: fy + fh - 52.0,
            text: String::from("Cancel"),
            font_size: 13.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_status(&self, cmds: &mut Frame<Target>) {
        let y = self.height - Self::STATUS_H;
        self.palette
            .push_surface(cmds, 0.0, y, self.width, Self::STATUS_H, 0.0, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: y + 7.0,
            text: self.status_msg.clone(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: self.width - 320.0,
            y: y + 7.0,
            text: String::from("N:New  A:Archive  C:Filter  Space:Check  F1:Keys"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(310.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

fn rate_color(rate: f32, pal: &Palette) -> Color {
    if rate >= 0.8 {
        pal.green
    } else if rate >= 0.5 {
        pal.yellow
    } else if rate >= 0.3 {
        pal.peach
    } else {
        pal.red
    }
}

impl App for HabitTrackerApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "Habits".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (self.width as u32, self.height as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        if self.handle_event(event) {
            Response::Redraw
        } else {
            Response::Idle
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.width = width;
        self.height = height;
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }

    /// Woken at the next midnight, when "today" moves on (design-decisions
    /// 1201) -- at most an hour away, so a clock that jumped or a machine that
    /// slept is caught up within the hour.
    fn tick_interval(&self) -> Option<Duration> {
        let left = clock_now().map_or(3600, |(_, left)| left.saturating_add(1));
        Some(Duration::from_secs(left.min(3600)))
    }
}

fn main() -> ExitCode {
    app::launch("habits", &mut HabitTrackerApp::from_settings())
}

// ── The pointer ─────────────────────────────────────────────────────

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The tracker drew four tabs, a New Habit button, a row and seven day cells
/// per habit, an archive and a form, and handled no pointer event
/// (`known-issues.md` →
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`). A habit is
/// named by its index in `habits`, not by its row: the category filter
/// renumbers rows and not habits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Tab(Screen),
    NewHabit,
    /// The category filter's chip: a press steps to the next category.
    FilterChip,
    /// The dashboard's list, which scrolls.
    HabitList,
    HabitRow(usize),
    /// A habit's day: 0 is today, 6 six days ago.
    DayCell(usize, usize),
    ArchiveButton(usize),
    /// The statistics table's rows, which scroll.
    StatsList,
    StatsRow(usize),
    /// The archive's list, which scrolls.
    ArchiveList,
    ArchiveRow(usize),
    RestoreButton(usize),
    /// Deleting an archived habit, which asks first.
    DeleteButton(usize),
    /// The question's answers: delete it, or keep it.
    ConfirmDelete,
    KeepHabit,
    /// The question's card, where a press does nothing.
    ConfirmCard,
    /// Everything around the question: a press there keeps the habit, as
    /// Escape does.
    ConfirmBackdrop,
    GraphPrev,
    GraphNext,
    FormCategory,
    FormDaily,
    FormWeekly,
    FormFewer,
    FormMore,
    FormCreate,
    FormCancel,
    /// Everything behind and around the form's controls: a press there does
    /// nothing.
    FormBackdrop,
    HelpCard,
}

impl HabitTrackerApp {
    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self.frame().hit_test(event.x, event.y) else {
                    return false;
                };
                self.activate(target)
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return false;
                }
                self.hover = over;
                true
            }
            MouseEventKind::Leave => self.hover.take().is_some(),
            MouseEventKind::Scroll { dy, .. } => {
                // Over the list on screen, and only there: the wheel over
                // the cards above the statistics table scrolls nothing.
                let over = self.target_at(event.x, event.y);
                if !matches!(
                    over,
                    Some(
                        Target::HabitList
                            | Target::HabitRow(_)
                            | Target::DayCell(..)
                            | Target::ArchiveButton(_)
                            | Target::ArchiveList
                            | Target::ArchiveRow(_)
                            | Target::RestoreButton(_)
                            | Target::DeleteButton(_)
                            | Target::StatsList
                            | Target::StatsRow(_)
                    )
                ) {
                    return false;
                }
                let Some((_, row_h, _)) = self.list_pane() else {
                    return false;
                };
                let rows = self.wheel.rows(dy);
                let before = self.scroll_offset;
                self.scroll_offset =
                    (self.scroll_offset + rows as f32 * row_h).clamp(0.0, self.scroll_limit());
                (self.scroll_offset - before).abs() > f32::EPSILON
            }
            _ => false,
        }
    }

    /// Put the selection on habit `index` (in `habits`), in the list shown.
    fn select_habit(&mut self, index: usize) -> bool {
        let list = if self.screen == Screen::Archive {
            self.archived_habits()
        } else {
            self.active_habits()
        };
        let Some(position) = list.iter().position(|i| *i == index) else {
            return false;
        };
        self.selected_habit = position;
        true
    }

    fn activate(&mut self, target: Target) -> bool {
        match target {
            Target::Tab(screen) => return self.show_screen(screen),
            Target::NewHabit => self.handle_key("n", false, false),
            Target::FilterChip => self.handle_key("c", false, false),
            Target::HabitRow(index) | Target::ArchiveRow(index) => {
                return self.select_habit(index);
            }
            Target::DayCell(index, col) => {
                if !self.select_habit(index) {
                    return false;
                }
                self.selected_day_col = col.min(6);
                self.toggle_check_in_selected();
            }
            Target::ArchiveButton(index) => self.archive_habit(index),
            Target::RestoreButton(index) => self.unarchive_habit(index),
            Target::DeleteButton(index) => self.ask_to_delete(index),
            Target::ConfirmDelete => {
                let Some(index) = self.pending_delete.take() else {
                    return false;
                };
                self.delete_habit(index);
            }
            Target::KeepHabit | Target::ConfirmBackdrop => {
                if self.pending_delete.take().is_none() {
                    return false;
                }
                self.status_msg = String::from("Kept");
            }
            Target::StatsRow(index) => {
                // To this habit's graph.
                let Some(position) = self.active_habits().iter().position(|i| *i == index) else {
                    return false;
                };
                self.heatmap_habit_idx = position;
                self.screen = Screen::HeatMap;
            }
            Target::GraphPrev => self.handle_key("Left", false, false),
            Target::GraphNext => self.handle_key("Right", false, false),
            Target::FormCategory => self.handle_create_form_key("Tab", false),
            Target::FormDaily => self.create_frequency_daily = true,
            Target::FormWeekly => self.create_frequency_daily = false,
            Target::FormFewer => self.handle_create_form_key("Down", false),
            Target::FormMore => self.handle_create_form_key("Up", false),
            Target::FormCreate => self.create_habit(),
            Target::FormCancel => self.handle_create_form_key("Escape", false),
            Target::HelpCard => self.show_help = false,
            Target::HabitList
            | Target::ArchiveList
            | Target::StatsList
            | Target::FormBackdrop
            | Target::ConfirmCard => return false,
        }
        self.keep_selection_visible();
        true
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that overflows or indexes out of range should fail loudly and
    // point at the line that did it — that is the diagnosis. The defensive
    // lints exist to keep panics out of code that runs on a user's data.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    /// An empty tracker says so where the habits would be, and says how to
    /// start one.
    ///
    /// The notice was drawn at the top of the window *before* the header,
    /// which painted over it, so nobody ever read it. And it said "Nothing is
    /// saved between runs", which is no longer true.
    #[test]
    fn an_empty_tracker_says_so_where_the_habits_would_be() {
        let app = HabitTrackerApp::new();
        let drawn: Vec<(String, f32)> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, y, .. } => Some((text.clone(), *y)),
                _ => None,
            })
            .collect();
        let top = HabitTrackerApp::HEADER_H + HabitTrackerApp::NAV_H;
        for line in NO_HABITS_LINES {
            let Some((_, y)) = drawn.iter().find(|(t, _)| t == line) else {
                panic!("the window never said {line:?}");
            };
            assert!(
                *y > top && *y < app.height - HabitTrackerApp::STATUS_H,
                "{line:?} is drawn at y={y}, under the header or the status bar"
            );
        }
        assert!(
            !NO_HABITS_LINES
                .iter()
                .any(|l| l.contains("Nothing is saved")),
            "the notice still says nothing is kept"
        );
        assert!(
            NO_HABITS_LINES.iter().any(|l| l.contains("+ New Habit")),
            "the notice does not say how to start a habit"
        );
    }

    // ── Date tests ──────────────────────────────────────────────────

    #[test]
    fn test_date_creation_valid() {
        let d = Date::new(2026, 5, 18);
        assert!(d.is_some());
        let d = d.unwrap();
        assert_eq!(d.year, 2026);
        assert_eq!(d.month, 5);
        assert_eq!(d.day, 18);
    }

    #[test]
    fn test_date_creation_invalid_month() {
        assert!(Date::new(2026, 0, 1).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
    }

    #[test]
    fn test_date_creation_invalid_day() {
        assert!(Date::new(2026, 2, 30).is_none());
        assert!(Date::new(2026, 4, 31).is_none());
    }

    // The leap-year and February-length tests that stood here asserted
    // 2000/1900/2024 and Feb 2000/1900 -- the same cases, on the same
    // implementation, that `guitk::date`'s own tests already assert. They
    // reached it through two habits-local wrappers that forwarded and did
    // nothing else, so what looked like calendar coverage in this app was a
    // second copy of another crate's tests. Both wrappers and both tests are
    // gone; `date::` is the one place either question is answered.

    #[test]
    fn test_date_add_days_forward() {
        let d = Date {
            year: 2026,
            month: 5,
            day: 30,
        };
        let next = d.add_days(2);
        assert_eq!(next.month, 6);
        assert_eq!(next.day, 1);
    }

    #[test]
    fn test_date_add_days_backward() {
        let d = Date {
            year: 2026,
            month: 6,
            day: 1,
        };
        let prev = d.add_days(-2);
        assert_eq!(prev.month, 5);
        assert_eq!(prev.day, 30);
    }

    #[test]
    fn test_date_add_days_year_boundary() {
        let d = Date {
            year: 2026,
            month: 12,
            day: 31,
        };
        let next = d.add_days(1);
        assert_eq!(next.year, 2027);
        assert_eq!(next.month, 1);
        assert_eq!(next.day, 1);
    }

    #[test]
    fn test_date_add_days_backward_year() {
        let d = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        let prev = d.add_days(-1);
        assert_eq!(prev.year, 2025);
        assert_eq!(prev.month, 12);
        assert_eq!(prev.day, 31);
    }

    #[test]
    fn test_days_since() {
        let d1 = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let d2 = Date {
            year: 2026,
            month: 5,
            day: 15,
        };
        assert_eq!(d1.days_since(d2), 3);
        assert_eq!(d2.days_since(d1), -3);
    }

    #[test]
    fn test_day_of_week() {
        // 2026-05-18 is a Monday
        let d = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        assert_eq!(d.day_of_week(), 1); // 1=Monday
    }

    #[test]
    fn test_day_of_week_sunday() {
        // 2026-05-17 is a Sunday
        let d = Date {
            year: 2026,
            month: 5,
            day: 17,
        };
        assert_eq!(d.day_of_week(), 0); // 0=Sunday
    }

    /// The weekday abbreviations, which are the habit grid's column headers
    /// and which nothing asserted: replacing `short_name()` with `name()`
    /// failed no test, so "Wednesday" could have gone into a column sized for
    /// "Wed". `day_of_week` was covered twice over; the function that turns
    /// it into the text a user reads was not covered at all.
    #[test]
    fn weekday_abbreviations_are_the_right_day_in_three_characters() {
        // 2026-05-17 is a Sunday, so this walks Sunday..Saturday in the same
        // order `day_of_week` numbers them.
        let want = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        for (i, short) in want.iter().enumerate() {
            let d = Date {
                year: 2026,
                month: 5,
                day: 17,
            }
            .add_days(i32::try_from(i).unwrap_or(0));
            assert_eq!(d.day_of_week_short(), *short, "offset {i}");
            assert_eq!(u32::try_from(i).unwrap_or(0), d.day_of_week(), "offset {i}");
            // Three characters, because the grid lays the columns out for
            // three. A full name here is a layout bug, not a wrong day.
            assert_eq!(d.day_of_week_short().len(), 3, "offset {i}");
        }
    }

    #[test]
    fn test_week_start_monday() {
        let d = Date {
            year: 2026,
            month: 5,
            day: 20,
        }; // Wednesday
        let ws = d.week_start_monday();
        assert_eq!(ws.day_of_week(), 1); // Monday
        assert!(ws.to_day_number() <= d.to_day_number());
        assert!(d.to_day_number() - ws.to_day_number() < 7);
    }

    #[test]
    fn test_format_short() {
        let d = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        assert_eq!(d.format_short(), "May 18");
    }

    #[test]
    fn test_format_full() {
        let d = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        assert_eq!(d.format_full(), "May 18, 2026");
    }

    /// This used to assert only that 2026-12-31 numbered higher than
    /// 2026-01-01 — a property every monotone function has, including one
    /// that returned the year. It could not have caught a wrong day *count*,
    /// which is what the streak walk and the grid subtract. Now that
    /// `to_day_number` is days-since-1970 rather than an offset Rata Die, the
    /// values are checkable, so check them.
    #[test]
    fn day_numbers_are_days_since_the_epoch() {
        for (year, month, day, want) in [
            (1970, 1, 1, 0),
            (1969, 12, 31, -1),  // before the epoch: negative, not clamped
            (2000, 3, 1, 11017), // just past a leap day in a century-leap year
            (2026, 1, 1, 20454),
            (2026, 12, 31, 20818),
        ] {
            let d = Date { year, month, day };
            assert_eq!(d.to_day_number(), want, "{year}-{month:02}-{day:02}");
        }

        // And the difference is a day count, which is the only thing callers
        // actually use it for. 2026 is a common year, so 1 Jan to 31 Dec is
        // 364 days.
        let jan1 = Date {
            year: 2026,
            month: 1,
            day: 1,
        };
        let dec31 = Date {
            year: 2026,
            month: 12,
            day: 31,
        };
        assert_eq!(dec31.days_since(jan1), 364);
        assert_eq!(jan1.days_since(dec31), -364);
        assert_eq!(dec31.to_day_number() - jan1.to_day_number(), 364);
    }

    #[test]
    fn test_date_ordering() {
        let d1 = Date {
            year: 2026,
            month: 5,
            day: 1,
        };
        let d2 = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        assert!(d1 < d2);
    }

    // ── Category tests ──────────────────────────────────────────────

    #[test]
    fn test_category_all_count() {
        assert_eq!(Category::ALL.len(), 9);
    }

    #[test]
    fn test_category_labels_non_empty() {
        for cat in &Category::ALL {
            assert!(!cat.label().is_empty());
        }
    }

    #[test]
    fn test_category_icons_non_empty() {
        for cat in &Category::ALL {
            assert!(!cat.icon().is_empty());
        }
    }

    // ── Frequency tests ─────────────────────────────────────────────

    #[test]
    fn test_frequency_daily_label() {
        assert_eq!(Frequency::Daily.label(), "Daily");
    }

    #[test]
    fn test_frequency_weekly_label() {
        assert_eq!(Frequency::Weekly(3).label(), "3x / week");
    }

    // ── Habit tests ─────────────────────────────────────────────────

    #[test]
    fn test_habit_creation() {
        let d = Date {
            year: 2026,
            month: 5,
            day: 1,
        };
        let h = Habit::new("Test", Category::Health, Frequency::Daily, d);
        assert_eq!(h.name, "Test");
        assert!(!h.archived);
        assert!(h.check_ins.is_empty());
    }

    #[test]
    fn test_habit_toggle_check_in() {
        let d = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new("Test", Category::Health, Frequency::Daily, d);
        assert!(!h.is_checked_on(d));
        h.toggle_check_in(d);
        assert!(h.is_checked_on(d));
        h.toggle_check_in(d);
        assert!(!h.is_checked_on(d));
    }

    #[test]
    fn test_habit_multiple_check_ins() {
        let base = Date {
            year: 2026,
            month: 5,
            day: 1,
        };
        let mut h = Habit::new("Test", Category::Health, Frequency::Daily, base);
        for i in 0..5 {
            h.toggle_check_in(base.add_days(i));
        }
        assert_eq!(h.check_ins.len(), 5);
        for i in 0..5 {
            assert!(h.is_checked_on(base.add_days(i)));
        }
    }

    #[test]
    fn test_habit_check_ins_sorted() {
        let base = Date {
            year: 2026,
            month: 5,
            day: 1,
        };
        let mut h = Habit::new("Test", Category::Health, Frequency::Daily, base);
        h.toggle_check_in(base.add_days(5));
        h.toggle_check_in(base.add_days(2));
        h.toggle_check_in(base.add_days(8));
        assert!(h.check_ins.windows(2).all(|w| w[0] <= w[1]));
    }

    // ── Streak tests ────────────────────────────────────────────────

    #[test]
    fn test_streak_daily_consecutive() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new(
            "Test",
            Category::Health,
            Frequency::Daily,
            today.add_days(-10),
        );
        for i in 0..5 {
            h.check_ins.push(today.add_days(-i));
        }
        assert_eq!(h.current_streak(today), 5);
    }

    #[test]
    fn test_streak_daily_with_gap() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new(
            "Test",
            Category::Health,
            Frequency::Daily,
            today.add_days(-10),
        );
        h.check_ins.push(today);
        h.check_ins.push(today.add_days(-1));
        // Gap on -2
        h.check_ins.push(today.add_days(-3));
        assert_eq!(h.current_streak(today), 2);
    }

    #[test]
    fn test_streak_daily_today_not_checked() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new(
            "Test",
            Category::Health,
            Frequency::Daily,
            today.add_days(-10),
        );
        // Checked yesterday and day before
        h.check_ins.push(today.add_days(-1));
        h.check_ins.push(today.add_days(-2));
        // Today not checked -- streak should count from yesterday
        assert_eq!(h.current_streak(today), 2);
    }

    #[test]
    fn test_streak_daily_empty() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let h = Habit::new("Test", Category::Health, Frequency::Daily, today);
        assert_eq!(h.current_streak(today), 0);
    }

    #[test]
    fn test_best_streak_daily() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new(
            "Test",
            Category::Health,
            Frequency::Daily,
            today.add_days(-20),
        );
        // Build a 5-day streak, then gap, then 3-day streak
        for i in 10..15 {
            h.check_ins.push(today.add_days(-i));
        }
        for i in 0..3 {
            h.check_ins.push(today.add_days(-i));
        }
        h.check_ins.sort();
        assert_eq!(h.best_streak(today), 5);
    }

    #[test]
    fn test_streak_weekly() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let start = today.add_days(-28);
        let mut h = Habit::new("Test", Category::Health, Frequency::Weekly(3), start);
        // Fill 3+ days for last 3 weeks
        for week_offset in 0..3 {
            let ws = today.add_days(-(week_offset * 7));
            let monday = ws.week_start_monday();
            for d in 0..3 {
                h.check_ins.push(monday.add_days(d));
            }
        }
        h.check_ins.sort();
        let streak = h.current_streak(today);
        assert!(streak >= 2); // at least 2 full weeks met
    }

    #[test]
    fn test_best_streak_weekly() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let start = today.add_days(-60);
        let mut h = Habit::new("Test", Category::Health, Frequency::Weekly(2), start);
        // 4 consecutive weeks meeting target
        for week in 0..4 {
            let ws = start.add_days(week * 7).week_start_monday();
            h.check_ins.push(ws);
            h.check_ins.push(ws.add_days(2));
        }
        h.check_ins.sort();
        let best = h.best_streak(today);
        assert!(best >= 4);
    }

    // ── Completion rate tests ───────────────────────────────────────

    #[test]
    fn test_completion_rate_daily_perfect() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new(
            "Test",
            Category::Health,
            Frequency::Daily,
            today.add_days(-10),
        );
        for i in 0..7 {
            h.check_ins.push(today.add_days(-i));
        }
        let rate = h.completion_rate(today, 7);
        assert!((rate - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_completion_rate_daily_half() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new(
            "Test",
            Category::Health,
            Frequency::Daily,
            today.add_days(-10),
        );
        // Check in on even days only (0, 2, 4, 6)
        for i in (0..7).filter(|x| x % 2 == 0) {
            h.check_ins.push(today.add_days(-i));
        }
        let rate = h.completion_rate(today, 7);
        // 4 out of 7
        assert!((rate - 4.0 / 7.0).abs() < 0.01);
    }

    #[test]
    fn test_completion_rate_zero_days() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let h = Habit::new("Test", Category::Health, Frequency::Daily, today);
        assert_eq!(h.completion_rate(today, 0), 0.0);
    }

    #[test]
    fn test_completion_rate_weekly() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let start = today.add_days(-28);
        let mut h = Habit::new("Test", Category::Health, Frequency::Weekly(3), start);
        // Meet target 2 out of 4 weeks
        for week in [0, 2] {
            let ws = today.add_days(-(week * 7)).week_start_monday();
            for d in 0..3 {
                h.check_ins.push(ws.add_days(d));
            }
        }
        h.check_ins.sort();
        let rate = h.completion_rate(today, 28);
        assert!(rate > 0.0 && rate <= 1.0);
    }

    #[test]
    fn test_completion_rate_alltime() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let created = today.add_days(-10);
        let mut h = Habit::new("Test", Category::Health, Frequency::Daily, created);
        // 5 out of 11 days (inclusive)
        for i in 0..5 {
            h.check_ins.push(today.add_days(-i));
        }
        let rate = h.completion_rate_alltime(today);
        assert!(rate > 0.0 && rate <= 1.0);
    }

    #[test]
    fn test_total_check_ins() {
        let today = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let mut h = Habit::new(
            "Test",
            Category::Health,
            Frequency::Daily,
            today.add_days(-10),
        );
        for i in 0..5 {
            h.check_ins.push(today.add_days(-i));
        }
        assert_eq!(h.total_check_ins(), 5);
    }

    // ── App creation tests ──────────────────────────────────────────

    #[test]
    fn a_fresh_tracker_has_no_history_and_the_fixture_does() {
        // Was `test_app_new_has_sample_habits`, asserting that a freshly
        // opened tracker already held five habits with forty-five days of
        // check-ins behind them -- a record of what the user had done, on
        // dated days, that they had never done.
        let fresh = HabitTrackerApp::new();
        assert!(fresh.habits.is_empty(), "a history appeared from nowhere");

        // And the fixture still supplies one for everything else to test.
        let seeded = HabitTrackerApp::with_sample_habits();
        assert_eq!(seeded.habits.len(), 5);
    }

    #[test]
    fn test_app_sample_habits_have_check_ins() {
        let app = HabitTrackerApp::with_sample_habits();
        for h in &app.habits {
            assert!(
                !h.check_ins.is_empty(),
                "Habit '{}' should have check-ins",
                h.name
            );
        }
    }

    #[test]
    fn test_app_default_screen() {
        let app = HabitTrackerApp::with_sample_habits();
        assert_eq!(app.screen, Screen::Dashboard);
    }

    /// "Today" is the clock's (design-decisions 1201). It was 18 May 2026 in
    /// every run, so a check-in made today was filed in May.
    #[test]
    fn a_new_tracker_takes_today_from_the_clock() {
        let before = today_from_clock().expect("the clock is readable here");
        let app = HabitTrackerApp::new();
        let after = today_from_clock().expect("the clock is readable here");
        assert!(
            app.today == before || app.today == after,
            "today is {:?}; the clock said {before:?} then {after:?}",
            app.today
        );
    }

    // ── Habit management tests ──────────────────────────────────────

    #[test]
    fn test_create_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        app.create_name = String::from("New Habit");
        app.create_category_idx = 0;
        app.create_frequency_daily = true;
        app.create_habit();
        assert_eq!(app.habits.len(), count + 1);
        assert_eq!(app.habits.last().unwrap().name, "New Habit");
    }

    #[test]
    fn test_create_habit_empty_name_rejected() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        app.create_name.clear();
        app.create_habit();
        assert_eq!(app.habits.len(), count);
    }

    #[test]
    fn test_create_habit_weekly() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.create_name = String::from("Weekly");
        app.create_frequency_daily = false;
        app.create_weekly_count = 4;
        app.create_habit();
        let last = app.habits.last().unwrap();
        assert_eq!(last.frequency, Frequency::Weekly(4));
    }

    #[test]
    fn test_create_habit_clears_form() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.create_name = String::from("Test");
        app.show_create_form = true;
        app.create_habit();
        assert!(app.create_name.is_empty());
        assert!(!app.show_create_form);
    }

    #[test]
    fn test_delete_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        app.delete_habit(0);
        assert_eq!(app.habits.len(), count - 1);
    }

    #[test]
    fn test_delete_habit_out_of_bounds() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        app.delete_habit(100);
        assert_eq!(app.habits.len(), count);
    }

    #[test]
    fn test_archive_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.archive_habit(0);
        assert!(app.habits[0].archived);
    }

    #[test]
    fn test_unarchive_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.habits[0].archived = true;
        app.unarchive_habit(0);
        assert!(!app.habits[0].archived);
    }

    #[test]
    fn test_active_habits_excludes_archived() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let before = app.active_habits().len();
        app.habits[0].archived = true;
        let after = app.active_habits().len();
        assert_eq!(after, before - 1);
    }

    #[test]
    fn test_archived_habits_list() {
        let mut app = HabitTrackerApp::with_sample_habits();
        assert!(app.archived_habits().is_empty());
        app.habits[0].archived = true;
        app.habits[1].archived = true;
        assert_eq!(app.archived_habits().len(), 2);
    }

    // ── Category filter tests ───────────────────────────────────────

    #[test]
    fn test_category_filter_none_shows_all() {
        let app = HabitTrackerApp::with_sample_habits();
        assert!(app.category_filter.is_none());
        assert_eq!(app.active_habits().len(), 5);
    }

    #[test]
    fn test_category_filter_fitness() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.category_filter = Some(Category::Fitness);
        let active = app.active_habits();
        assert_eq!(active.len(), 1);
        assert_eq!(app.habits[active[0]].name, "Exercise");
    }

    #[test]
    fn test_category_filter_no_match() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.category_filter = Some(Category::Finance);
        assert!(app.active_habits().is_empty());
    }

    // ── Check-in toggle tests ───────────────────────────────────────

    #[test]
    fn test_toggle_check_in_selected() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_habit = 0;
        app.selected_day_col = 0; // today
        let idx = app.active_habits()[0];
        let was_checked = app.habits[idx].is_checked_on(app.today);
        app.toggle_check_in_selected();
        let now_checked = app.habits[idx].is_checked_on(app.today);
        assert_ne!(was_checked, now_checked);
    }

    #[test]
    fn test_toggle_check_in_past_day() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_habit = 0;
        app.selected_day_col = 3; // 3 days ago
        let idx = app.active_habits()[0];
        let date = app.today.add_days(-3);
        let was = app.habits[idx].is_checked_on(date);
        app.toggle_check_in_selected();
        assert_ne!(was, app.habits[idx].is_checked_on(date));
    }

    // ── Date navigation tests ───────────────────────────────────────

    #[test]
    fn test_advance_day() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let original = app.today;
        app.advance_day();
        assert_eq!(app.today.days_since(original), 1);
    }

    #[test]
    fn test_go_back_day() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let original = app.today;
        app.go_back_day();
        assert_eq!(original.days_since(app.today), 1);
    }

    // ── Key handling tests ──────────────────────────────────────────

    #[test]
    fn test_key_screen_switch() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.handle_key("2", false, false);
        assert_eq!(app.screen, Screen::Statistics);
        app.handle_key("3", false, false);
        assert_eq!(app.screen, Screen::Archive);
        app.handle_key("4", false, false);
        assert_eq!(app.screen, Screen::HeatMap);
        app.handle_key("1", false, false);
        assert_eq!(app.screen, Screen::Dashboard);
    }

    #[test]
    fn test_key_new_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.handle_key("n", false, false);
        assert!(app.show_create_form);
    }

    #[test]
    fn test_key_up_down() {
        let mut app = HabitTrackerApp::with_sample_habits();
        assert_eq!(app.selected_habit, 0);
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_habit, 1);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_habit, 0);
    }

    #[test]
    fn test_key_up_boundary() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_habit = 0;
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_habit, 0);
    }

    #[test]
    fn test_key_down_boundary() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let max = app.active_habits().len() - 1;
        app.selected_habit = max;
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_habit, max);
    }

    #[test]
    fn test_key_left_right_day_col() {
        let mut app = HabitTrackerApp::with_sample_habits();
        assert_eq!(app.selected_day_col, 0);
        app.handle_key("Left", false, false);
        assert_eq!(app.selected_day_col, 1);
        app.handle_key("Right", false, false);
        assert_eq!(app.selected_day_col, 0);
    }

    #[test]
    fn test_key_left_boundary() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_day_col = 6;
        app.handle_key("Left", false, false);
        assert_eq!(app.selected_day_col, 6);
    }

    #[test]
    fn test_key_right_boundary() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_day_col = 0;
        app.handle_key("Right", false, false);
        assert_eq!(app.selected_day_col, 0);
    }

    #[test]
    fn test_key_space_toggles_check_in() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_habit = 0;
        app.selected_day_col = 0;
        let idx = app.active_habits()[0];
        let before = app.habits[idx].is_checked_on(app.today);
        app.handle_key("Space", false, false);
        let after = app.habits[idx].is_checked_on(app.today);
        assert_ne!(before, after);
    }

    #[test]
    fn test_key_archive() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_habit = 0;
        let idx = app.active_habits()[0];
        app.handle_key("a", false, false);
        assert!(app.habits[idx].archived);
    }

    /// `+` and `-` moved "today" for the user, which filed the next check-in
    /// on whatever day the keys had reached. Today is the clock's now, and a
    /// missed day is filled in from its column.
    #[test]
    fn the_keys_that_moved_today_are_gone() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let day = app.today;
        for k in ["+", "=", "-"] {
            app.handle_key(k, false, false);
            assert_eq!(app.today, day, "{k} still moves today");
        }
    }

    #[test]
    fn test_key_cycle_filter() {
        let mut app = HabitTrackerApp::with_sample_habits();
        assert!(app.category_filter.is_none());
        app.handle_key("c", false, false);
        assert_eq!(app.category_filter, Some(Category::Health));
        app.handle_key("c", false, false);
        assert_eq!(app.category_filter, Some(Category::Fitness));
    }

    #[test]
    fn test_key_cycle_filter_wraps() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.category_filter = Some(*Category::ALL.last().unwrap());
        app.handle_key("c", false, false);
        assert!(app.category_filter.is_none());
    }

    /// Page Down scrolls a list that overflows, and stops where its last row
    /// is in view.
    #[test]
    fn page_down_scrolls_a_long_list_to_its_end_and_no_further() {
        let mut app = app_with_habits(30);
        app.handle_key("PageDown", false, false);
        assert!(app.scroll_offset > 0.0, "Page Down did not scroll");
        for _ in 0..40 {
            app.handle_key("PageDown", false, false);
        }
        assert_eq!(app.scroll_offset, app.scroll_limit());
        let last = *app.active_habits().last().unwrap();
        let row = probe::rect_of(&app, Target::HabitRow(last)).expect("the last row is drawn");
        let pane = app.list_rect();
        assert!(
            row.bottom() <= pane.bottom() + 0.5,
            "at the end of the list its last row is cut off: {row:?} in {pane:?}"
        );
    }

    /// And a list that fits does not scroll at all. The limit was 2000 pixels
    /// whatever the list, so four habits could be paged away into nothing.
    #[test]
    fn page_down_does_not_scroll_a_list_that_fits() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.handle_key("PageDown", false, false);
        assert_eq!(app.scroll_offset, 0.0);
    }

    #[test]
    fn test_key_page_up_at_zero() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.handle_key("PageUp", false, false);
        assert_eq!(app.scroll_offset, 0.0);
    }

    // ── Create form key tests ───────────────────────────────────────

    #[test]
    fn test_create_form_escape() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        app.handle_key("Escape", false, false);
        assert!(!app.show_create_form);
    }

    #[test]
    fn test_create_form_typing() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        app.handle_key("H", false, false);
        app.handle_key("i", false, false);
        assert_eq!(app.create_name, "Hi");
    }

    #[test]
    fn test_create_form_backspace() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        app.create_name = String::from("Hello");
        app.handle_key("Backspace", false, false);
        assert_eq!(app.create_name, "Hell");
    }

    #[test]
    fn test_create_form_tab_cycles_category() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        assert_eq!(app.create_category_idx, 0);
        app.handle_key("Tab", false, false);
        assert_eq!(app.create_category_idx, 1);
    }

    /// F2 switches the new habit between daily and weekly. It was F1, which
    /// is the list of keys in every other program here.
    #[test]
    fn test_create_form_f2_toggles_frequency() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        assert!(app.create_frequency_daily);
        app.handle_key("F2", false, false);
        assert!(!app.create_frequency_daily);
        app.handle_key("F2", false, false);
        assert!(app.create_frequency_daily);
    }

    /// Up and Down set how many times a week, from one to seven. It was F2,
    /// which counted up and wrapped from seven to one.
    #[test]
    fn test_create_form_up_and_down_set_the_weekly_count() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        app.create_weekly_count = 3;
        app.handle_key("Up", false, false);
        assert_eq!(app.create_weekly_count, 4);
        app.handle_key("Down", false, false);
        app.handle_key("Down", false, false);
        assert_eq!(app.create_weekly_count, 2);
        for _ in 0..10 {
            app.handle_key("Down", false, false);
        }
        assert_eq!(app.create_weekly_count, 1);
        for _ in 0..10 {
            app.handle_key("Up", false, false);
        }
        assert_eq!(app.create_weekly_count, 7);
    }

    /// F1 in the form shows the keys, and leaves the form as it was.
    #[test]
    fn f1_in_the_form_shows_the_keys() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        app.create_name = String::from("Walk");
        app.handle_key("F1", false, false);
        assert!(app.show_help);
        assert!(
            app.create_frequency_daily,
            "F1 still switches the frequency"
        );
        app.handle_key("F1", false, false);
        assert!(!app.show_help);
        assert!(app.show_create_form);
        assert_eq!(app.create_name, "Walk");
        // And `?` is a character a name may hold while the form is up.
        app.handle_key("?", false, false);
        assert_eq!(app.create_name, "Walk?");
        assert!(!app.show_help);
    }

    #[test]
    fn test_create_form_enter_creates() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        app.create_name = String::from("From Form");
        let count = app.habits.len();
        app.handle_key("Return", false, false);
        assert_eq!(app.habits.len(), count + 1);
        assert!(!app.show_create_form);
    }

    // ── Statistics helper tests ─────────────────────────────────────

    #[test]
    fn test_overall_completion_today() {
        let app = HabitTrackerApp::with_sample_habits();
        let (done, total) = app.overall_completion_today();
        assert_eq!(total, 5);
        // done depends on sample data, just check it's in range
        assert!(done <= total);
    }

    #[test]
    fn test_best_habit_streak() {
        let app = HabitTrackerApp::with_sample_habits();
        let (name, val) = app.best_habit_streak();
        assert!(!name.is_empty());
        assert!(val > 0);
    }

    #[test]
    fn test_average_completion_7d() {
        let app = HabitTrackerApp::with_sample_habits();
        let avg = app.average_completion_7d();
        assert!((0.0..=1.0).contains(&avg));
    }

    #[test]
    fn test_average_completion_30d() {
        let app = HabitTrackerApp::with_sample_habits();
        let avg = app.average_completion_30d();
        assert!((0.0..=1.0).contains(&avg));
    }

    #[test]
    fn test_average_completion_no_habits() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.habits.clear();
        assert_eq!(app.average_completion_7d(), 0.0);
        assert_eq!(app.average_completion_30d(), 0.0);
    }

    // ── Heatmap tests ───────────────────────────────────────────────

    #[test]
    fn test_heatmap_data_length() {
        let app = HabitTrackerApp::with_sample_habits();
        let data = app.heatmap_data(0, 90);
        assert_eq!(data.len(), 90);
    }

    #[test]
    fn test_heatmap_data_full_year() {
        let app = HabitTrackerApp::with_sample_habits();
        let data = app.heatmap_data(0, 364);
        assert_eq!(data.len(), 364);
    }

    #[test]
    fn test_heatmap_data_invalid_index() {
        let app = HabitTrackerApp::with_sample_habits();
        let data = app.heatmap_data(999, 30);
        assert!(data.is_empty());
    }

    #[test]
    fn test_heatmap_color_checked() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let c = HabitTrackerApp::heatmap_color(true, &pal, 1.0);
        // Should have non-zero alpha
        assert_ne!(c, pal.surface0);
    }

    #[test]
    fn heatmap_color_darkens_with_intensity() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        // The whole point of the function: a stronger day is a stronger cell.
        // The old test only asked that a checked day differ from an unchecked
        // one, which a flat `pal.green` also satisfies -- and flat `pal.green` was in
        // fact what the renderer drew.
        let faint = HabitTrackerApp::heatmap_color(true, &pal, 0.0);
        let strong = HabitTrackerApp::heatmap_color(true, &pal, 1.0);
        assert!(
            strong.a > faint.a,
            "intensity 1.0 ({}) should be more opaque than 0.0 ({})",
            strong.a,
            faint.a
        );
        assert_eq!((strong.r, strong.g, strong.b), (faint.r, faint.g, faint.b));
    }

    #[test]
    fn heatmap_intensities_scale_with_run_length() {
        // A day inside a long run outranks an isolated one, and the longest
        // run on screen reaches the top of the scale.
        let d = Date {
            year: 2026,
            month: 3,
            day: 1,
        };
        let checks = [true, false, true, true, true, true];
        let data: Vec<(Date, bool)> = checks
            .iter()
            .enumerate()
            .map(|(i, &c)| (d.add_days(i as i32), c))
            .collect();
        let got = HabitTrackerApp::heatmap_intensities(&data);
        assert_eq!(got.len(), data.len());
        assert!(got[0] < got[5], "an isolated day is fainter than a run");
        assert_eq!(got[1], 0.0, "an unchecked day has no intensity");
        assert!(
            got[2] < got[3] && got[3] < got[4] && got[4] < got[5],
            "intensity climbs through a run: {got:?}"
        );
        assert_eq!(got[5], 1.0, "the longest run reaches full intensity");
    }

    #[test]
    fn heatmap_intensities_restart_after_a_gap() {
        // Momentum is what is being shown, so a break resets it: the day after
        // a miss is as faint as any first day, however good the week before.
        let d = Date {
            year: 2026,
            month: 3,
            day: 1,
        };
        let checks = [true, true, true, true, false, true];
        let data: Vec<(Date, bool)> = checks
            .iter()
            .enumerate()
            .map(|(i, &c)| (d.add_days(i as i32), c))
            .collect();
        let got = HabitTrackerApp::heatmap_intensities(&data);
        assert_eq!(got[0], got[5], "the day after a gap starts over");
        assert!(got[5] < got[3]);
    }

    #[test]
    fn heatmap_intensities_flat_when_nothing_is_checked() {
        // Every run is zero long, so there is no longest run to divide by --
        // the case that would otherwise divide by zero.
        let d = Date {
            year: 2026,
            month: 3,
            day: 1,
        };
        let data: Vec<(Date, bool)> = (0..5).map(|i| (d.add_days(i), false)).collect();
        let got = HabitTrackerApp::heatmap_intensities(&data);
        assert_eq!(got, vec![0.0; 5]);
    }

    #[test]
    fn heatmap_intensities_of_nothing_is_nothing() {
        assert!(HabitTrackerApp::heatmap_intensities(&[]).is_empty());
    }

    #[test]
    fn render_heatmap_shades_cells_by_run_length() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        // The regression this whole change exists for. The renderer drew
        // `if checked { pal.green } else { pal.surface0 }` -- two colours, whatever
        // the data -- while `heatmap_color` sat unused with only its own test
        // for company.
        //
        // Reads only the graph, not the legend. The legend is drawn from the
        // same `heatmap_color` and is four swatches of the same size, so a
        // test that took every small square passed against the flat renderer
        // on the strength of the legend alone -- which is exactly how the
        // flat graph went unnoticed under a gradient key in the first place.
        // The graph is the seven topmost rows; the legend sits below them.
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::HeatMap;
        app.heatmap_habit_idx = 0;
        let today = app.today;
        let Some(h) = app.habits.get_mut(0) else {
            panic!("the starting library has at least one habit");
        };
        h.check_ins.clear();
        // Days back from today: a lone day, a gap, then a five-day run.
        for back in [1, 8, 9, 10, 11, 12] {
            h.check_ins.push(today.add_days(-back));
        }
        h.check_ins.sort();

        let squares: Vec<(u32, Color)> = app
            .render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (width - height).abs() < f32::EPSILON && width < 20.0 => {
                    Some((y.to_bits(), color))
                }
                _ => None,
            })
            .collect();
        let mut rows: Vec<u32> = squares.iter().map(|(y, _)| *y).collect();
        rows.sort_unstable();
        rows.dedup();
        assert!(rows.len() > 7, "expected 7 graph rows plus a legend row");
        let graph_rows = &rows[..7];

        let mut shades: Vec<u8> = squares
            .iter()
            .filter(|(y, c)| graph_rows.contains(y) && *c != pal.surface0)
            .map(|(_, c)| c.a)
            .collect();
        assert_eq!(shades.len(), 6, "six checked days should be drawn");
        shades.sort_unstable();
        shades.dedup();
        assert!(
            shades.len() > 1,
            "checked days all came out one colour ({shades:?}) -- flat again"
        );
        assert_eq!(
            shades.last().copied(),
            Some(255),
            "the longest run should reach full strength"
        );
    }

    #[test]
    fn test_heatmap_color_unchecked() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let c = HabitTrackerApp::heatmap_color(false, &pal, 0.5);
        assert_eq!(c, pal.surface0);
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn test_render_dashboard() {
        let app = HabitTrackerApp::with_sample_habits();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_statistics() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::Statistics;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_archive_empty() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::Archive;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_archive_with_items() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::Archive;
        app.habits[0].archived = true;
        app.habits[1].archived = true;
        let cmds = app.render_commands();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_heatmap() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::HeatMap;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_create_form() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        let cmds = app.render_commands();
        assert!(cmds.len() > 30);
    }

    #[test]
    fn test_render_empty_dashboard() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.habits.clear();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_heatmap() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::HeatMap;
        app.habits.clear();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_category_filter() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.category_filter = Some(Category::Fitness);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_scroll() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.scroll_offset = 100.0;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    // ── Heatmap navigation tests ────────────────────────────────────

    #[test]
    fn test_heatmap_left_right_navigation() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::HeatMap;
        assert_eq!(app.heatmap_habit_idx, 0);
        app.handle_key("Right", false, false);
        assert_eq!(app.heatmap_habit_idx, 1);
        app.handle_key("Left", false, false);
        assert_eq!(app.heatmap_habit_idx, 0);
    }

    #[test]
    fn test_heatmap_left_boundary() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::HeatMap;
        app.heatmap_habit_idx = 0;
        app.handle_key("Left", false, false);
        assert_eq!(app.heatmap_habit_idx, 0);
    }

    #[test]
    fn test_heatmap_right_boundary() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.screen = Screen::HeatMap;
        let max = app.active_habits().len() - 1;
        app.heatmap_habit_idx = max;
        app.handle_key("Right", false, false);
        assert_eq!(app.heatmap_habit_idx, max);
    }

    // ── Restore from archive test ───────────────────────────────────

    #[test]
    fn test_restore_from_archive_via_enter() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.habits[0].archived = true;
        app.screen = Screen::Archive;
        app.selected_habit = 0;
        app.handle_key("Return", false, false);
        assert!(!app.habits[0].archived);
    }

    // ── Rate color test ─────────────────────────────────────────────

    #[test]
    fn test_rate_color_ranges() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        assert_eq!(rate_color(1.0, &pal), pal.green);
        assert_eq!(rate_color(0.8, &pal), pal.green);
        assert_eq!(rate_color(0.6, &pal), pal.yellow);
        assert_eq!(rate_color(0.4, &pal), pal.peach);
        assert_eq!(rate_color(0.1, &pal), pal.red);
    }

    // ── Ctrl+D delete test ──────────────────────────────────────────

    /// Ctrl+D asks, and only `y` deletes. It deleted at once -- a habit and
    /// every check-in behind it, beside the A that archives.
    #[test]
    fn ctrl_d_asks_and_only_y_deletes() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        app.selected_habit = 0;
        app.handle_key("d", true, false);
        assert_eq!(app.habits.len(), count, "deleted without asking");
        assert!(app.pending_delete.is_some());
        app.handle_key("n", false, false);
        assert_eq!(app.habits.len(), count);
        assert!(app.pending_delete.is_none());
        // Enter is not yes: the default answer to a question with no undo is
        // the one that keeps.
        app.handle_key("d", true, false);
        app.handle_key("Return", false, false);
        assert_eq!(app.habits.len(), count);
        app.handle_key("d", true, false);
        app.handle_key("y", false, false);
        assert_eq!(app.habits.len(), count - 1);
    }

    /// In the archive, Ctrl+D is about the archived habit chosen there. It
    /// looked the archive's selection up in the dashboard's list.
    #[test]
    fn ctrl_d_in_the_archive_deletes_the_archived_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let gone = app.habits[2].name.clone();
        app.archive_habit(2);
        app.show_screen(Screen::Archive);
        app.handle_key("d", true, false);
        app.handle_key("y", false, false);
        assert!(app.habits.iter().all(|h| h.name != gone), "{gone} survived");
        assert!(app.habits.iter().all(|h| !h.archived));
    }

    // ── Completions in week test ────────────────────────────────────

    #[test]
    fn test_completions_in_week() {
        let base = Date {
            year: 2026,
            month: 5,
            day: 18,
        }; // Monday
        let mut h = Habit::new("Test", Category::Health, Frequency::Weekly(3), base);
        h.check_ins.push(base);
        h.check_ins.push(base.add_days(2));
        h.check_ins.push(base.add_days(4));
        assert_eq!(h.completions_in_week(base), 3);
    }

    #[test]
    fn test_completions_in_week_empty() {
        let base = Date {
            year: 2026,
            month: 5,
            day: 18,
        };
        let h = Habit::new("Test", Category::Health, Frequency::Weekly(3), base);
        assert_eq!(h.completions_in_week(base), 0);
    }

    // ── Edge case tests ─────────────────────────────────────────────

    #[test]
    fn test_weekly_count_clamped() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.create_name = String::from("Clamped");
        app.create_frequency_daily = false;
        app.create_weekly_count = 99;
        app.create_habit();
        let last = app.habits.last().unwrap();
        assert_eq!(last.frequency, Frequency::Weekly(7));
    }

    #[test]
    fn test_heatmap_data_has_correct_dates() {
        let app = HabitTrackerApp::with_sample_habits();
        let data = app.heatmap_data(0, 7);
        // Last entry should be today
        assert_eq!(data.last().unwrap().0, app.today);
        // First entry should be 6 days ago
        assert_eq!(data.first().unwrap().0, app.today.add_days(-6));
    }

    #[test]
    fn test_multiple_date_advances() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let start = app.today;
        for _ in 0..10 {
            app.advance_day();
        }
        assert_eq!(app.today.days_since(start), 10);
    }

    #[test]
    fn creating_two_habits_appends_two_habits() {
        // Was `test_next_id_increments`, which watched a counter that nothing
        // ever read. What the test was reaching for -- that creating twice
        // creates two -- is asserted here against the list itself.
        let mut app = HabitTrackerApp::with_sample_habits();
        let before = app.habits.len();
        app.create_name = String::from("A");
        app.create_habit();
        app.create_name = String::from("B");
        app.create_habit();
        assert_eq!(app.habits.len(), before + 2);
        let names: Vec<&str> = app.habits.iter().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"A") && names.contains(&"B"), "{names:?}");
    }

    #[test]
    fn test_selected_habit_adjusts_on_delete() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.selected_habit = 4; // last
        app.delete_habit(4);
        assert!(app.selected_habit < app.active_habits().len());
    }

    // ── Per-habit statistics table ──────────────────────────────────

    /// Two habits whose names share a long prefix and are far too wide for the
    /// 136px name column — the case a silent clip renders identically.
    fn app_with_long_habit_names() -> HabitTrackerApp {
        let mut app = HabitTrackerApp::with_sample_habits();
        for suffix in ["before bed", "after lunch"] {
            app.create_name = format!("Read thirty minutes of a paper book {suffix}");
            app.create_habit();
        }
        app
    }

    /// The text commands of the statistics table, rendered on their own: a full
    /// render puts header and nav text at x values that can coincide with a
    /// column's left edge, and the assertion would then fail on chrome that was
    /// never part of this table.
    fn statistics_commands(app: &HabitTrackerApp) -> Vec<RenderCommand> {
        let mut f = Frame::new(app.width, app.height);
        app.render_statistics(&mut f, 0.0);
        f.into_tree().commands
    }

    #[test]
    fn no_statistics_cell_escapes_its_column() {
        let app = app_with_long_habit_names();
        let cmds = statistics_commands(&app);
        let edges = Table::with_gap(STATS_COLUMNS, 20.0, STATS_GAP).spans();
        let mut checked = 0usize;
        for cmd in &cmds {
            let RenderCommand::Text {
                x: tx,
                text,
                font_size,
                font_weight,
                max_width: Some(_),
                overflow: TextOverflow::Ellipsis,
                ..
            } = cmd
            else {
                continue;
            };
            let Some(&(_, right)) = edges.iter().find(|(left, _)| (left - tx).abs() < 0.01) else {
                continue;
            };
            let drawn = tx + guitk::text::measure(text, *font_size, *font_weight);
            assert!(
                drawn <= right + 0.5,
                "cell {text:?} starting at {tx} runs to {drawn}, \
                 past its column's right edge {right}",
            );
            checked = checked.saturating_add(1);
        }
        // 8 header labels plus 8 cells for each of the two long-named habits.
        assert!(
            checked >= 24,
            "only {checked} statistics cells checked, expected at least 24",
        );
    }

    #[test]
    fn a_clipped_habit_name_says_it_was_clipped() {
        let app = app_with_long_habit_names();
        let cmds = statistics_commands(&app);
        let left = Table::with_gap(STATS_COLUMNS, 20.0, STATS_GAP).left(STATS_HABIT);
        let names: Vec<String> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x,
                    text,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_),
                    overflow: TextOverflow::Ellipsis,
                    ..
                } if (x - left).abs() < 0.01 => Some(text.clone()),
                _ => None,
            })
            .collect();
        let cut: Vec<&String> = names
            .iter()
            .filter(|n| n.starts_with("Read thirty"))
            .collect();
        assert_eq!(
            cut.len(),
            2,
            "both long habit names should be drawn, got {names:?}"
        );
        for name in &cut {
            assert!(
                name.ends_with('…'),
                "a name that did not fit must be marked as cut, got {name:?}"
            );
        }
    }

    // ── Events ──────────────────────────────────────────────────────

    fn key(k: Key, text: &str) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: text.to_string(),
        })
    }

    /// A digit switches screens, through the real event path.
    ///
    /// This app had no event handling until 2026-09-03: `handle_key` takes a
    /// `&str` and nothing produced one, so every keystroke test handed itself
    /// the string it wanted. That proves the match arms and says nothing about
    /// whether a keystroke can reach them.
    #[test]
    fn a_digit_switches_screens_through_the_event_path() {
        let mut app = HabitTrackerApp::with_sample_habits();
        assert!(app.handle_event(&key(Key::Num2, "2")));
        assert_eq!(app.screen, Screen::Statistics);
        assert!(app.handle_event(&key(Key::Num1, "1")));
        assert_eq!(app.screen, Screen::Dashboard);
    }

    /// The named keys arrive under the names the handlers match on.
    ///
    /// The translation is the whole risk here: a key whose name does not match
    /// what `handle_key` expects is silently ignored, and the app looks like it
    /// has no arrow keys rather than like it has a typo.
    #[test]
    fn every_named_key_translates_to_the_string_the_handler_expects() {
        for (k, want) in [
            (Key::Up, "Up"),
            (Key::Down, "Down"),
            (Key::Left, "Left"),
            (Key::Right, "Right"),
            (Key::Space, "Space"),
            (Key::Escape, "Escape"),
            (Key::Enter, "Return"),
            (Key::Tab, "Tab"),
            (Key::Backspace, "Backspace"),
            (Key::PageUp, "PageUp"),
            (Key::PageDown, "PageDown"),
            (Key::F1, "F1"),
            (Key::F2, "F2"),
        ] {
            let ev = KeyEvent {
                key: k,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: String::new(),
            };
            assert_eq!(
                HabitTrackerApp::key_name(&ev).as_deref(),
                Some(want),
                "{k:?} did not translate to {want}"
            );
        }
    }

    /// A printable key arrives as the character it typed, not as its key name.
    ///
    /// `text` first is what makes a shifted key and a dead-key sequence arrive
    /// correctly spelled; asking the `Key` enum for a letter cannot.
    #[test]
    fn a_printable_key_arrives_as_the_character_it_typed() {
        let ev = KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: "n".to_string(),
        };
        assert_eq!(HabitTrackerApp::key_name(&ev).as_deref(), Some("n"));
    }

    /// A key release is not a press.
    #[test]
    fn a_key_release_changes_nothing() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let before = app.screen;
        assert!(!app.handle_event(&Event::Key(KeyEvent {
            key: Key::Num2,
            pressed: false,
            modifiers: guitk::event::Modifiers::NONE,
            text: "2".to_string(),
        })));
        assert_eq!(app.screen, before);
    }

    /// Arrow keys move the selection and stop at both ends.
    ///
    /// The bounds were `self.selected_habit > 0` and
    /// `self.selected_habit < max - 1` — a `- 1` on a `usize` guarded by a
    /// separate `max > 0`. They are `saturating_*` now, and this pins that the
    /// ends still hold.
    #[test]
    fn the_selection_stops_at_both_ends() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.active_habits().len();
        assert!(
            count >= 2,
            "the sample data needs two habits to move between"
        );

        app.selected_habit = 0;
        app.handle_event(&key(Key::Up, ""));
        assert_eq!(app.selected_habit, 0, "Up ran off the top");

        app.selected_habit = count.saturating_sub(1);
        app.handle_event(&key(Key::Down, ""));
        assert_eq!(
            app.selected_habit,
            count.saturating_sub(1),
            "Down ran off the bottom"
        );
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

        fn fills(app: &mut HabitTrackerApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = HabitTrackerApp::with_sample_habits();

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
    // ── The pointer, the questions, the lists, and what is kept ─────

    use guitk::event::MouseEvent;
    use guitk::probe::{self, Probe};
    use std::time::Duration;

    impl Probe for HabitTrackerApp {
        type Target = Target;
        type Outcome = bool;
        const SIZE: (f32, f32) = (1000.0, 700.0);

        /// Drawn at the app's own size, which these tests leave at `SIZE`.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(&mut self, x: f32, y: f32, button: MouseButton, _size: (f32, f32)) -> bool {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> bool {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<bool> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    /// The day the fixtures are built on.
    const FIXTURE_DAY: Date = Date {
        year: 2026,
        month: 5,
        day: 18,
    };

    /// A tracker with `n` daily habits, far more than a window shows when `n`
    /// is large.
    fn app_with_habits(n: usize) -> HabitTrackerApp {
        let mut app = HabitTrackerApp::new();
        app.today = FIXTURE_DAY;
        for i in 0..n {
            app.create_name = format!("Habit {i}");
            app.create_habit();
        }
        app
    }

    /// What the topmost box at `target`'s centre is: whether it can be pressed
    /// or something is over it.
    fn topmost_at_centre_of(app: &HabitTrackerApp, target: Target) -> Option<Target> {
        let r = probe::rect_of(app, target)?;
        app.frame().hit_test(r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn every_tab_is_a_button() {
        let mut app = HabitTrackerApp::with_sample_habits();
        for screen in [
            Screen::Statistics,
            Screen::Archive,
            Screen::HeatMap,
            Screen::Dashboard,
        ] {
            assert!(probe::click(&mut app, Target::Tab(screen)));
            assert_eq!(app.screen, screen);
        }
        // The tab of the screen already shown changes nothing.
        assert!(!probe::click(&mut app, Target::Tab(Screen::Dashboard)));
    }

    #[test]
    fn the_new_habit_button_opens_the_form() {
        let mut app = HabitTrackerApp::with_sample_habits();
        assert!(probe::click(&mut app, Target::NewHabit));
        assert!(app.show_create_form);
    }

    /// The category filter is a chip that is there with no filter set, or
    /// there would be nothing to press to set one.
    #[test]
    fn the_filter_chip_is_there_unset_and_steps_through_categories() {
        let mut app = HabitTrackerApp::with_sample_habits();
        assert!(app.category_filter.is_none());
        assert!(probe::click(&mut app, Target::FilterChip));
        assert_eq!(app.category_filter, Some(Category::Health));
        assert!(probe::click(&mut app, Target::FilterChip));
        assert_eq!(app.category_filter, Some(Category::Fitness));
    }

    /// A day's cell checks that habit in on that day, and out again.
    #[test]
    fn a_day_cell_checks_its_habit_in_on_its_day() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let habit = app.active_habits()[2];
        let day = app.today.add_days(-3);
        let was = app.habits[habit].is_checked_on(day);
        assert!(probe::click(&mut app, Target::DayCell(habit, 3)));
        assert_eq!(app.habits[habit].is_checked_on(day), !was);
        assert_eq!(app.selected_habit, 2, "the press did not choose its habit");
        assert_eq!(app.selected_day_col, 3);
        assert!(probe::click(&mut app, Target::DayCell(habit, 3)));
        assert_eq!(app.habits[habit].is_checked_on(day), was);
    }

    #[test]
    fn a_row_press_chooses_its_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let habit = app.active_habits()[3];
        // On the habit's name, at the row's left: the row's middle is a day
        // cell, which chooses the habit too, so a press there could not tell
        // whether the row itself does.
        let row = probe::rect_of(&app, Target::HabitRow(habit)).unwrap();
        let (x, y) = (row.x + 24.0, row.y + row.h / 2.0);
        assert_eq!(app.frame().hit_test(x, y), Some(Target::HabitRow(habit)));
        let checked = app.habits[habit].check_ins.clone();
        assert!(app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })));
        assert_eq!(app.selected_habit, 3);
        assert_eq!(
            app.habits[habit].check_ins, checked,
            "a row press checked in"
        );
    }

    /// Archiving was `A` and only `A`.
    #[test]
    fn the_archive_button_archives_its_own_row() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let habit = app.active_habits()[1];
        assert!(probe::click(&mut app, Target::ArchiveButton(habit)));
        assert!(app.habits[habit].archived);
        assert!(probe::rect_of(&app, Target::ArchiveButton(habit)).is_none());
    }

    /// Restoring was Enter on the selected row, and deleting was not in the
    /// pointer's reach at all.
    #[test]
    fn an_archived_row_restores_and_deletes() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.archive_habit(0);
        app.archive_habit(1);
        assert!(probe::click(&mut app, Target::Tab(Screen::Archive)));
        assert!(probe::click(&mut app, Target::RestoreButton(0)));
        assert!(!app.habits[0].archived);
        let doomed = app.habits[1].name.clone();
        let count = app.habits.len();
        assert!(probe::click(&mut app, Target::DeleteButton(1)));
        assert_eq!(app.habits.len(), count, "deleted without asking");
        assert!(probe::click(&mut app, Target::ConfirmDelete));
        assert_eq!(app.habits.len(), count - 1);
        assert!(app.habits.iter().all(|h| h.name != doomed));
    }

    /// The question can be answered either way by pointer, and a press on
    /// the card itself answers nothing.
    #[test]
    fn the_delete_question_answers_to_the_pointer() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        for keep in [Target::KeepHabit, Target::ConfirmBackdrop] {
            app.ask_to_delete(0);
            assert!(!probe::click(&mut app, Target::ConfirmCard));
            assert!(app.pending_delete.is_some(), "a press on the card answered");
            if keep == Target::ConfirmBackdrop {
                // Outside the card, over a day cell: a press that reached the
                // cell would check a habit in, where one that reached the New
                // Habit button would only answer the question as a key does
                // -- which is how a missing backdrop hid from this test.
                app.pending_delete = None;
                let cell =
                    probe::rect_of(&app, Target::DayCell(app.active_habits()[0], 0)).unwrap();
                app.ask_to_delete(0);
                let first = app.active_habits()[0];
                let checked = app.habits[first].check_ins.clone();
                app.handle_event(&Event::Mouse(MouseEvent {
                    x: cell.x + cell.w / 2.0,
                    y: cell.y + cell.h / 2.0,
                    kind: MouseEventKind::Press(MouseButton::Left),
                }));
                assert_eq!(
                    app.habits[first].check_ins, checked,
                    "the press reached the day cell"
                );
            } else {
                assert!(probe::click(&mut app, keep));
            }
            assert!(app.pending_delete.is_none());
            assert_eq!(app.habits.len(), count, "{keep:?} deleted");
            assert!(!app.show_create_form, "the press reached the button behind");
        }
    }

    /// While a question or the form is up, nothing behind it can be pressed.
    #[test]
    fn nothing_behind_a_question_or_the_form_can_be_pressed() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let row = app.active_habits()[0];
        app.ask_to_delete(row);
        for behind in [
            Target::NewHabit,
            Target::DayCell(row, 0),
            Target::Tab(Screen::Archive),
        ] {
            assert_eq!(
                topmost_at_centre_of(&app, behind),
                Some(Target::ConfirmBackdrop)
            );
        }
        app.pending_delete = None;
        app.show_create_form = true;
        for behind in [
            Target::NewHabit,
            Target::DayCell(row, 0),
            Target::Tab(Screen::Archive),
        ] {
            assert_eq!(
                topmost_at_centre_of(&app, behind),
                Some(Target::FormBackdrop)
            );
        }
    }

    /// The form can be filled in and sent by pointer, except for the name,
    /// which is typed.
    #[test]
    fn the_form_answers_to_the_pointer() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        probe::click(&mut app, Target::NewHabit);
        probe::type_str(&mut app, "Read");
        assert!(probe::click(&mut app, Target::FormWeekly));
        assert!(!app.create_frequency_daily);
        assert!(probe::click(&mut app, Target::FormMore));
        assert!(probe::click(&mut app, Target::FormMore));
        assert!(probe::click(&mut app, Target::FormFewer));
        assert_eq!(app.create_weekly_count, 4);
        assert!(probe::click(&mut app, Target::FormCategory));
        assert_eq!(app.create_category_idx, 1);
        // Beside the controls, on the card and off it: the backdrop's own
        // centre is the Weekly button, so the points are chosen, not probed.
        let card = probe::rect_of(&app, Target::FormBackdrop).unwrap();
        for (x, y) in [(card.x + 6.0, card.y + 6.0), (4.0, 4.0)] {
            assert_eq!(app.frame().hit_test(x, y), Some(Target::FormBackdrop));
            assert!(!app.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            })));
        }
        assert!(
            app.show_create_form,
            "a press beside the controls closed the form"
        );
        assert!(probe::click(&mut app, Target::FormCreate));
        assert_eq!(app.habits.len(), count + 1);
        let made = app.habits.last().unwrap();
        assert_eq!(made.name, "Read");
        assert_eq!(made.frequency, Frequency::Weekly(4));
        assert_eq!(made.category, Category::ALL[1]);

        probe::click(&mut app, Target::NewHabit);
        assert!(probe::click(&mut app, Target::FormDaily));
        assert!(probe::click(&mut app, Target::FormCancel));
        assert!(!app.show_create_form);
        assert_eq!(app.habits.len(), count + 1);
    }

    /// The count's buttons are there only for a weekly habit.
    #[test]
    fn the_weekly_count_is_offered_only_for_a_weekly_habit() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_create_form = true;
        assert!(probe::rect_of(&app, Target::FormMore).is_none());
        app.create_frequency_daily = false;
        assert!(probe::rect_of(&app, Target::FormMore).is_some());
        assert!(probe::rect_of(&app, Target::FormFewer).is_some());
    }

    /// A name can hold a space. The space bar arrives as the named key
    /// "Space", which the form's catch-all refused as too long.
    #[test]
    fn a_name_can_hold_a_space() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.handle_key("n", false, false);
        for k in [
            key(Key::R, "R"),
            key(Key::E, "e"),
            key(Key::Space, " "),
            key(Key::A, "a"),
        ] {
            app.handle_event(&k);
        }
        assert_eq!(app.create_name, "Re a");
    }

    /// A name of spaces is refused: it would be kept, and then dropped as
    /// blank the next time the window opened.
    #[test]
    fn a_blank_name_is_refused() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let count = app.habits.len();
        app.create_name = String::from("   ");
        app.create_habit();
        assert_eq!(app.habits.len(), count);
        assert_eq!(app.status_msg, "Name cannot be empty");
    }

    /// A statistics row opens its habit's graph.
    #[test]
    fn a_statistics_row_opens_its_graph() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_screen(Screen::Statistics);
        let habit = app.active_habits()[2];
        assert!(probe::click(&mut app, Target::StatsRow(habit)));
        assert_eq!(app.screen, Screen::HeatMap);
        assert_eq!(app.heatmap_habit_idx, 2);
    }

    #[test]
    fn the_graph_buttons_step_through_the_habits() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.show_screen(Screen::HeatMap);
        assert!(probe::click(&mut app, Target::GraphNext));
        assert!(probe::click(&mut app, Target::GraphNext));
        assert_eq!(app.heatmap_habit_idx, 2);
        assert!(probe::click(&mut app, Target::GraphPrev));
        assert_eq!(app.heatmap_habit_idx, 1);
    }

    /// The wheel scrolls the list it is over, and nothing when it is over the
    /// header.
    #[test]
    fn the_wheel_scrolls_the_list_it_is_over() {
        // A negative `dy` is toward the user: toward the end of the list.
        let mut app = app_with_habits(30);
        assert!(!probe::scroll_at_point(&mut app, Target::NewHabit, -3.0));
        assert_eq!(app.scroll_offset, 0.0);
        assert!(probe::scroll_at_point(&mut app, Target::HabitList, -3.0));
        assert!(app.scroll_offset > 0.0);
        let down = app.scroll_offset;
        assert!(probe::scroll_at_point(&mut app, Target::HabitList, 3.0));
        assert!(app.scroll_offset < down);
    }

    /// A row scrolled up under the day heads is not drawn there, nor pressed
    /// there: the list's clip covers its hit boxes too.
    #[test]
    fn a_row_scrolled_under_the_day_heads_cannot_be_pressed() {
        let mut app = app_with_habits(30);
        app.scroll_offset = HabitTrackerApp::ROW_H * 1.5;
        let pane = app.list_rect();
        let first = app.active_habits()[0];
        for target in [
            Target::HabitRow(first),
            Target::DayCell(first, 0),
            Target::ArchiveButton(first),
        ] {
            if let Some(r) = probe::rect_of(&app, target) {
                assert!(
                    r.y >= pane.y - 0.5,
                    "{target:?} reaches above the list: {r:?}"
                );
            }
        }
    }

    /// The arrows keep the chosen habit on screen: the offset moved only with
    /// Page Up and Page Down, so Down walked the selection off the bottom.
    #[test]
    fn the_arrows_keep_the_chosen_habit_on_screen() {
        let mut app = app_with_habits(30);
        for _ in 0..20 {
            app.handle_key("Down", false, false);
        }
        assert_eq!(app.selected_habit, 20);
        let habit = app.active_habits()[20];
        let row = probe::rect_of(&app, Target::HabitRow(habit)).expect("the chosen row is drawn");
        let pane = app.list_rect();
        assert!(
            row.y >= pane.y - 0.5 && row.bottom() <= pane.bottom() + 0.5,
            "{row:?} in {pane:?}"
        );
        for _ in 0..20 {
            app.handle_key("Up", false, false);
        }
        assert_eq!(app.scroll_offset, 0.0);
    }

    /// Archiving the last row leaves a habit chosen. The selection stayed
    /// past the end of the shorter list, choosing nothing.
    #[test]
    fn archiving_the_last_row_leaves_a_habit_chosen() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let last = app.active_habits().len() - 1;
        app.selected_habit = last;
        app.handle_key("a", false, false);
        assert_eq!(app.selected_habit, last - 1);
    }

    /// The number keys start each screen at its top, as a tab click does.
    #[test]
    fn the_number_keys_start_each_screen_at_its_top() {
        let mut app = HabitTrackerApp::with_sample_habits();
        // Three archived, so a selection of 2 is a real row there too: the
        // clamp to the list's length would otherwise hide a missing reset.
        for i in 0..3 {
            app.archive_habit(i);
        }
        app.selected_habit = 2;
        app.handle_key("3", false, false);
        assert_eq!(app.screen, Screen::Archive);
        assert_eq!(app.selected_habit, 0);
    }

    /// The archive scrolls, so a habit archived past the bottom edge can be
    /// reached and restored. It stopped drawing at the edge.
    #[test]
    fn the_archive_scrolls_to_its_last_habit() {
        let mut app = app_with_habits(30);
        for i in 0..30 {
            app.archive_habit(i);
        }
        app.show_screen(Screen::Archive);
        assert!(probe::rect_of(&app, Target::RestoreButton(29)).is_none());
        for _ in 0..60 {
            probe::scroll_at_point(&mut app, Target::ArchiveList, -3.0);
        }
        assert!(probe::click(&mut app, Target::RestoreButton(29)));
        assert!(!app.habits[29].archived);
    }

    /// And the statistics table scrolls, so every habit's numbers can be
    /// read, and its graph opened.
    #[test]
    fn the_statistics_table_scrolls_to_its_last_habit() {
        let mut app = app_with_habits(30);
        app.show_screen(Screen::Statistics);
        assert!(probe::rect_of(&app, Target::StatsRow(29)).is_none());
        // The wheel over the cards above the table scrolls nothing.
        let r = app.stats_rect(HabitTrackerApp::HEADER_H + HabitTrackerApp::NAV_H);
        assert!(!app.handle_event(&Event::Mouse(MouseEvent {
            x: 40.0,
            y: r.y - 150.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
        })));
        for _ in 0..60 {
            probe::scroll_at_point(&mut app, Target::StatsList, -3.0);
        }
        assert!(probe::click(&mut app, Target::StatsRow(29)));
        assert_eq!(app.screen, Screen::HeatMap);
        assert_eq!(app.heatmap_habit_idx, 29);
    }

    /// The pointer lights what it is over.
    #[test]
    fn hovering_a_control_lights_it() {
        let mut app = HabitTrackerApp::with_sample_habits();
        let r = probe::rect_of(&app, Target::NewHabit).unwrap();
        assert!(app.handle_event(&Event::Mouse(MouseEvent {
            x: r.x + 4.0,
            y: r.y + 4.0,
            kind: MouseEventKind::Move,
        })));
        assert_eq!(app.hover, Some(Target::NewHabit));
        assert!(app.handle_event(&Event::Mouse(MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Leave,
        })));
        assert_eq!(app.hover, None);
    }

    /// F1 raises the list of keys, which is modal, and a press or Escape
    /// puts it away.
    #[test]
    fn f1_shows_the_keys_and_the_card_is_modal() {
        let mut app = HabitTrackerApp::with_sample_habits();
        app.handle_key("F1", false, false);
        assert!(app.show_help);
        app.handle_key("n", false, false);
        assert!(
            !app.show_create_form,
            "a key reached the app behind the card"
        );
        app.handle_key("Escape", false, false);
        assert!(!app.show_help);
        app.handle_key("F1", false, false);
        assert!(probe::click(&mut app, Target::HelpCard));
        assert!(!app.show_help);
    }

    /// Every key on the card does something, in a state where it has
    /// something to do.
    #[test]
    fn every_advertised_key_does_something() {
        for (keys, what) in SHORTCUTS {
            match *keys {
                "1 / 2 / 3 / 4" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    for (k, screen) in [
                        ("2", Screen::Statistics),
                        ("3", Screen::Archive),
                        ("4", Screen::HeatMap),
                        ("1", Screen::Dashboard),
                    ] {
                        app.handle_key(k, false, false);
                        assert_eq!(app.screen, screen, "{k}");
                    }
                }
                "N" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    app.handle_key("n", false, false);
                    assert!(app.show_create_form);
                }
                "Up / Down" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    app.handle_key("Down", false, false);
                    assert_eq!(app.selected_habit, 1);
                    app.handle_key("Up", false, false);
                    assert_eq!(app.selected_habit, 0);
                }
                "Left / Right" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    app.handle_key("Left", false, false);
                    assert_eq!(app.selected_day_col, 1);
                    app.handle_key("Right", false, false);
                    assert_eq!(app.selected_day_col, 0);
                    app.show_screen(Screen::HeatMap);
                    app.handle_key("Right", false, false);
                    assert_eq!(app.heatmap_habit_idx, 1);
                    app.handle_key("Left", false, false);
                    assert_eq!(app.heatmap_habit_idx, 0);
                }
                "Space / Enter" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    let habit = app.active_habits()[0];
                    let was = app.habits[habit].is_checked_on(app.today);
                    app.handle_key("Space", false, false);
                    assert_ne!(app.habits[habit].is_checked_on(app.today), was);
                    app.handle_key("Return", false, false);
                    assert_eq!(app.habits[habit].is_checked_on(app.today), was);
                    app.archive_habit(habit);
                    app.show_screen(Screen::Archive);
                    app.handle_key("Return", false, false);
                    assert!(!app.habits[habit].archived);
                }
                "A" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    let habit = app.active_habits()[0];
                    app.handle_key("a", false, false);
                    assert!(app.habits[habit].archived);
                }
                "Ctrl+D" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    app.handle_key("d", true, false);
                    assert!(app.pending_delete.is_some());
                }
                "C" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    app.handle_key("c", false, false);
                    assert_eq!(app.category_filter, Some(Category::Health));
                }
                "PgUp / PgDn" => {
                    let mut app = app_with_habits(30);
                    app.handle_key("PageDown", false, false);
                    assert!(app.scroll_offset > 0.0);
                    app.handle_key("PageUp", false, false);
                    assert_eq!(app.scroll_offset, 0.0);
                }
                "F1" => {
                    let mut app = HabitTrackerApp::with_sample_habits();
                    app.handle_key("F1", false, false);
                    assert!(app.show_help);
                }
                other => panic!(
                    "the card offers {other:?} ({what}) and this test has no case for it -- \
                     add one, or the row advertises a key nothing checks"
                ),
            }
        }
    }

    /// The header names the day once. It read "Mon May 18, 2026 -- Mon".
    #[test]
    fn the_header_names_the_day_once() {
        let app = HabitTrackerApp::with_sample_habits();
        let full = app.today.format_full();
        let header = app
            .render_commands()
            .into_iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, .. } if text.contains(&full) => Some(text),
                _ => None,
            })
            .expect("the header shows the date");
        assert_eq!(
            header.matches(app.today.day_of_week_short()).count(),
            1,
            "{header:?}"
        );
    }

    /// The status bar names F1, and no longer the date keys.
    #[test]
    fn the_status_bar_names_the_help_key() {
        let app = HabitTrackerApp::with_sample_habits();
        let texts: Vec<String> = app
            .render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t.contains("F1:Keys")));
        assert!(!texts.iter().any(|t| t.contains("+/-:Date")));
    }

    // ── The clock ───────────────────────────────────────────────────

    /// A tick after midnight moves today on, and one on the same day does
    /// nothing.
    #[test]
    fn a_tick_after_midnight_moves_today_on() {
        let mut app = HabitTrackerApp::new();
        let clock = today_from_clock().unwrap();
        app.today = clock.add_days(-1);
        assert!(app.handle_event(&Event::Tick { elapsed_ms: 60_000 }));
        let now = app.today;
        assert!(now == clock || now == clock.add_days(1), "today is {now:?}");
        let again = app.handle_event(&Event::Tick { elapsed_ms: 60_000 });
        // Unless midnight fell between the two ticks, the second changes nothing.
        assert_eq!(again, app.today != now);
    }

    /// The window asks to be woken by the next midnight, and within the hour
    /// whatever the clock does.
    #[test]
    fn the_window_asks_to_be_woken_by_midnight() {
        let app = HabitTrackerApp::new();
        let wake = app.tick_interval().expect("the window never wakes");
        assert!(
            wake >= Duration::from_secs(1) && wake <= Duration::from_hours(1),
            "{wake:?}"
        );
    }

    // ── What is kept ────────────────────────────────────────────────

    #[test]
    fn iso_dates_round_trip_and_nonsense_is_refused() {
        let d = Date {
            year: 2026,
            month: 2,
            day: 28,
        };
        assert_eq!(d.to_iso(), "2026-02-28");
        assert_eq!(Date::from_iso("2026-02-28"), Some(d));
        assert_eq!(Date::from_iso(" 2026-02-28 "), Some(d));
        for bad in [
            "",
            "2026-02-30",
            "2026-13-01",
            "yesterday",
            "2026-02",
            "2026-xx-01",
        ] {
            assert_eq!(Date::from_iso(bad), None, "{bad:?}");
        }
    }

    /// Habits, their check-ins, and archiving survive the window closing.
    /// Nothing did: every mark went when the window closed.
    #[test]
    fn habits_and_their_check_ins_survive_a_restart() {
        settingsfile::testing::with_scratch_config("habits-kept", |_| {
            let mut app = HabitTrackerApp::from_settings();
            app.today = FIXTURE_DAY;
            app.create_name = String::from("Read 20 pages");
            app.create_category_idx = 4;
            app.create_frequency_daily = false;
            app.create_weekly_count = 3;
            app.create_habit();
            app.create_name = String::from("Stretch");
            app.create_habit();
            app.selected_habit = 0;
            app.selected_day_col = 0;
            app.toggle_check_in_selected();
            app.selected_day_col = 2;
            app.toggle_check_in_selected();
            app.archive_habit(1);

            let again = HabitTrackerApp::from_settings();
            assert_eq!(again.habits.len(), 2);
            let read = &again.habits[0];
            assert_eq!(read.name, "Read 20 pages");
            assert_eq!(read.category, Category::Learning);
            assert_eq!(read.frequency, Frequency::Weekly(3));
            assert_eq!(read.created, FIXTURE_DAY);
            assert_eq!(read.check_ins, vec![FIXTURE_DAY.add_days(-2), FIXTURE_DAY]);
            assert!(!read.archived);
            assert_eq!(again.habits[1].name, "Stretch");
            assert!(again.habits[1].archived);

            // Restored, unchecked and deleted are kept too.
            let mut app = again;
            app.today = FIXTURE_DAY;
            app.unarchive_habit(1);
            app.selected_habit = 0;
            app.selected_day_col = 0;
            app.toggle_check_in_selected();
            app.ask_to_delete(1);
            app.handle_key("y", false, false);
            let third = HabitTrackerApp::from_settings();
            assert_eq!(third.habits.len(), 1);
            assert_eq!(third.habits[0].check_ins, vec![FIXTURE_DAY.add_days(-2)]);
        });
    }

    /// A new habit's id is past every kept one, so two habits never share an
    /// entry in the file.
    #[test]
    fn a_new_habit_after_a_restart_gets_a_new_id() {
        settingsfile::testing::with_scratch_config("habits-ids", |_| {
            let mut app = HabitTrackerApp::from_settings();
            for name in ["A", "B"] {
                app.create_name = name.to_string();
                app.create_habit();
            }
            let mut app = HabitTrackerApp::from_settings();
            app.create_name = String::from("C");
            app.create_habit();
            let ids: Vec<u64> = app.habits.iter().map(|h| h.id).collect();
            assert_eq!(ids, vec![1, 2, 3]);
            assert_eq!(HabitTrackerApp::from_settings().habits.len(), 3);
        });
    }

    /// An entry that cannot be read is not shown, and is left in the file,
    /// which is the user's: a later write touches only the entry it is about.
    #[test]
    fn an_unreadable_entry_is_skipped_and_left_alone() {
        settingsfile::testing::with_scratch_config("habits-broken", |_| {
            let mut doc = yamldoc::Document::new();
            doc.set_str(&[HABITS_KEY, "1", "created"], "2026-05-01");
            doc.set_str(&[HABITS_KEY, "2", "name"], "No date");
            doc.set_str(&[HABITS_KEY, "2", "created"], "yesterday");
            doc.set_str(&[HABITS_KEY, "7", "name"], "Kept");
            doc.set_str(&[HABITS_KEY, "7", "category"], "gardening");
            doc.set_str(&[HABITS_KEY, "7", "frequency"], "weekly 9");
            doc.set_str(&[HABITS_KEY, "7", "created"], "2026-05-01");
            doc.set_seq(
                &[HABITS_KEY, "7", "check_ins"],
                &["2026-05-02", "not a day", "2026-05-02"],
            );
            settingsfile::store(CONFIG_NAME, &doc).unwrap();

            let mut app = HabitTrackerApp::from_settings();
            assert_eq!(app.habits.len(), 1);
            let kept = &app.habits[0];
            assert_eq!(kept.name, "Kept");
            assert_eq!(kept.id, 7);
            assert_eq!(kept.category, Category::Custom);
            assert_eq!(kept.frequency, Frequency::Weekly(7));
            assert_eq!(
                kept.check_ins,
                vec![Date {
                    year: 2026,
                    month: 5,
                    day: 2
                }]
            );
            app.create_name = String::from("New");
            app.create_habit();
            assert_eq!(app.habits[1].id, 8);
            let file = settingsfile::load(CONFIG_NAME);
            let mut keys = file.keys(&[HABITS_KEY]);
            keys.sort();
            assert_eq!(keys, vec!["1", "2", "7", "8"]);
        });
    }

    /// A tracker made with `new` -- every test's -- writes nothing, so a test
    /// that forgets its scratch settings cannot write the developer's own.
    #[test]
    fn a_tracker_made_with_new_writes_nothing() {
        settingsfile::testing::with_scratch_config("habits-quiet", |_| {
            let mut app = HabitTrackerApp::new();
            app.create_name = String::from("Walk");
            app.create_habit();
            app.toggle_check_in_selected();
            app.archive_habit(0);
            app.delete_habit(0);
            let path = settingsfile::path_for(CONFIG_NAME).unwrap();
            assert!(!path.exists(), "{} was written", path.display());
        });
    }

    /// A write that fails says so, and is not covered by the message of the
    /// change it failed to keep.
    #[test]
    fn a_failed_write_is_reported() {
        settingsfile::testing::with_scratch_config("habits-refused", |dir| {
            // A file where the settings directory must go.
            std::fs::write(dir.join("slateos"), b"not a directory").unwrap();
            let mut app = HabitTrackerApp::from_settings();
            app.create_name = String::from("Walk");
            app.create_habit();
            assert!(
                app.status_msg.starts_with("Could not keep your habits"),
                "{}",
                app.status_msg
            );
            app.status_msg.clear();
            app.delete_habit(0);
            assert!(
                app.status_msg.starts_with("Could not keep your habits"),
                "{}",
                app.status_msg
            );
        });
    }
}
