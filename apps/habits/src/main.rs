#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::needless_pass_by_value)]

//! OurOS Habit Tracker --- track daily and weekly habits with streaks,
//! categories, contribution graphs, and completion statistics.
//!
//! Features:
//! - Habit CRUD (create, edit, delete, archive/unarchive)
//! - Daily or weekly frequency
//! - Categories with color coding
//! - Check-in toggles per day
//! - Streak tracking (current and best)
//! - Completion rates (7-day, 30-day, all-time)
//! - Contribution/heatmap graph
//! - Archive view for retired habits
//! - Statistics dashboard
//! - Simulated date system for testing
//! - 5 sample habits pre-loaded

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Date ────────────────────────────────────────────────────────────

/// Simple date: year, month (1-12), day (1-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Date {
    year: i32,
    month: u32,
    day: u32,
}

impl Date {
    fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        if month < 1 || month > 12 {
            return None;
        }
        let max_d = days_in_month(year, month);
        if day < 1 || day > max_d {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// Day of week: 0=Sunday ... 6=Saturday (Zeller's congruence).
    fn day_of_week(self) -> u32 {
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

    fn day_of_week_short(self) -> &'static str {
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

    fn month_short(self) -> &'static str {
        month_short(self.month)
    }

    fn add_days(self, n: i32) -> Self {
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

    /// Number of days between self and other (self - other). Positive if self is later.
    fn days_since(self, other: Self) -> i32 {
        self.to_day_number() - other.to_day_number()
    }

    /// Monotonic day number for comparison (not calendar-accurate, but consistent).
    fn to_day_number(self) -> i32 {
        let mut y = self.year;
        let mut m = self.month as i32;
        if m <= 2 {
            y -= 1;
            m += 12;
        }
        // Rata Die approximation
        365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + self.day as i32 - 307
    }

    /// The Monday of the ISO week containing this date.
    fn week_start_monday(self) -> Self {
        let dow = self.day_of_week(); // 0=Sun..6=Sat
        let days_back = if dow == 0 { 6 } else { dow as i32 - 1 };
        self.add_days(-days_back)
    }

    fn format_short(self) -> String {
        format!("{} {:02}", self.month_short(), self.day)
    }

    fn format_full(self) -> String {
        format!("{} {:02}, {}", self.month_short(), self.day, self.year)
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 => 31,
        2 => if is_leap_year(year) { 29 } else { 28 },
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 30,
    }
}

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

    fn color(self) -> Color {
        match self {
            Self::Health => GREEN,
            Self::Fitness => PEACH,
            Self::Productivity => BLUE,
            Self::Mindfulness => MAUVE,
            Self::Learning => YELLOW,
            Self::Social => TEAL,
            Self::Creative => LAVENDER,
            Self::Finance => RED,
            Self::Custom => SUBTEXT0,
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
    id: u32,
    name: String,
    description: String,
    category: Category,
    frequency: Frequency,
    /// Dates on which this habit was checked in
    check_ins: Vec<Date>,
    /// Date the habit was created
    created: Date,
    archived: bool,
}

impl Habit {
    fn new(id: u32, name: &str, description: &str, category: Category, frequency: Frequency, created: Date) -> Self {
        Self {
            id,
            name: String::from(name),
            description: String::from(description),
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
                streak += 1;
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
                streak += 1;
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
                count += 1;
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
            if sorted[i].days_since(sorted[i - 1]) == 1 {
                current += 1;
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
                current += 1;
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
                    let d = today.add_days(-(i as i32));
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
                        completed += 1;
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
                        met += 1;
                    }
                    ws = ws.add_days(-7);
                }
                met as f32 / weeks as f32
            }
        }
    }

    /// Completion rate over all time since creation.
    fn completion_rate_alltime(&self, today: Date) -> f32 {
        let total_days = today.days_since(self.created) + 1;
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
    next_id: u32,
    today: Date,
    screen: Screen,
    selected_habit: usize,
    scroll_offset: f32,
    category_filter: Option<Category>,
    show_create_form: bool,
    create_name: String,
    create_description: String,
    create_category_idx: usize,
    create_frequency_daily: bool,
    create_weekly_count: u32,
    /// Which day column is selected for check-in toggling (0 = today, 6 = 6 days ago)
    selected_day_col: usize,
    heatmap_habit_idx: usize,
    status_msg: String,
}

impl HabitTrackerApp {
    const HEADER_H: f32 = 50.0;
    const ROW_H: f32 = 52.0;
    const SIDEBAR_W: f32 = 200.0;
    const DAY_COL_W: f32 = 38.0;
    const STATUS_H: f32 = 28.0;
    const NAV_H: f32 = 36.0;
    const HEATMAP_CELL: f32 = 14.0;
    const HEATMAP_GAP: f32 = 3.0;

    fn new() -> Self {
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut app = Self {
            width: 1000.0,
            height: 700.0,
            habits: Vec::new(),
            next_id: 1,
            today,
            screen: Screen::Dashboard,
            selected_habit: 0,
            scroll_offset: 0.0,
            category_filter: None,
            show_create_form: false,
            create_name: String::new(),
            create_description: String::new(),
            create_category_idx: 0,
            create_frequency_daily: true,
            create_weekly_count: 3,
            selected_day_col: 0,
            heatmap_habit_idx: 0,
            status_msg: String::from("Habit Tracker"),
        };
        app.load_sample_habits();
        app
    }

    fn load_sample_habits(&mut self) {
        let base = self.today.add_days(-45);

        // 1) Exercise -- daily, fitness
        let mut h1 = Habit::new(
            self.next_id, "Exercise", "30 minutes of physical activity",
            Category::Fitness, Frequency::Daily, base,
        );
        self.next_id += 1;
        // Simulate some check-ins over the last 45 days (about 70% completion)
        for i in 0..45 {
            let d = base.add_days(i);
            if i % 10 != 3 && i % 10 != 7 && i % 7 != 6 {
                h1.check_ins.push(d);
            }
        }
        self.habits.push(h1);

        // 2) Read -- daily, learning
        let mut h2 = Habit::new(
            self.next_id, "Read 30 min", "Read for at least 30 minutes",
            Category::Learning, Frequency::Daily, base,
        );
        self.next_id += 1;
        for i in 0..45 {
            let d = base.add_days(i);
            if i % 3 != 2 {
                h2.check_ins.push(d);
            }
        }
        self.habits.push(h2);

        // 3) Meditate -- daily, mindfulness
        let mut h3 = Habit::new(
            self.next_id, "Meditate", "10 minutes of mindfulness meditation",
            Category::Mindfulness, Frequency::Daily, base,
        );
        self.next_id += 1;
        for i in 0..45 {
            let d = base.add_days(i);
            if i % 5 != 4 {
                h3.check_ins.push(d);
            }
        }
        self.habits.push(h3);

        // 4) Meal prep -- weekly 3x, health
        let mut h4 = Habit::new(
            self.next_id, "Meal Prep", "Prepare healthy meals for the week",
            Category::Health, Frequency::Weekly(3), base,
        );
        self.next_id += 1;
        for i in 0..45 {
            let d = base.add_days(i);
            let dow = d.day_of_week();
            if dow == 0 || dow == 3 || dow == 5 {
                h4.check_ins.push(d);
            }
        }
        self.habits.push(h4);

        // 5) Journal -- daily, productivity
        let mut h5 = Habit::new(
            self.next_id, "Journal", "Write in daily journal",
            Category::Productivity, Frequency::Daily, base,
        );
        self.next_id += 1;
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
            if let Some(cat) = self.category_filter {
                if h.category != cat {
                    continue;
                }
            }
            indices.push(i);
        }
        indices
    }

    fn archived_habits(&self) -> Vec<usize> {
        self.habits.iter().enumerate()
            .filter(|(_, h)| h.archived)
            .map(|(i, _)| i)
            .collect()
    }

    fn create_habit(&mut self) {
        if self.create_name.is_empty() {
            self.status_msg = String::from("Name cannot be empty");
            return;
        }
        let category = Category::ALL.get(self.create_category_idx)
            .copied()
            .unwrap_or(Category::Custom);
        let frequency = if self.create_frequency_daily {
            Frequency::Daily
        } else {
            Frequency::Weekly(self.create_weekly_count.clamp(1, 7))
        };
        let habit = Habit::new(
            self.next_id,
            &self.create_name,
            &self.create_description,
            category,
            frequency,
            self.today,
        );
        self.next_id += 1;
        self.habits.push(habit);
        self.create_name.clear();
        self.create_description.clear();
        self.create_category_idx = 0;
        self.create_frequency_daily = true;
        self.create_weekly_count = 3;
        self.show_create_form = false;
        self.status_msg = String::from("Habit created!");
    }

    fn delete_habit(&mut self, idx: usize) {
        if idx < self.habits.len() {
            let name = self.habits[idx].name.clone();
            self.habits.remove(idx);
            if self.selected_habit >= self.active_habits().len() && self.selected_habit > 0 {
                self.selected_habit -= 1;
            }
            self.status_msg = format!("Deleted: {name}");
        }
    }

    fn archive_habit(&mut self, idx: usize) {
        if let Some(h) = self.habits.get_mut(idx) {
            h.archived = true;
            self.status_msg = format!("Archived: {}", h.name);
        }
    }

    fn unarchive_habit(&mut self, idx: usize) {
        if let Some(h) = self.habits.get_mut(idx) {
            h.archived = false;
            self.status_msg = format!("Restored: {}", h.name);
        }
    }

    fn toggle_check_in_selected(&mut self) {
        let active = self.active_habits();
        if let Some(&habit_idx) = active.get(self.selected_habit) {
            let date = self.today.add_days(-(self.selected_day_col as i32));
            if let Some(h) = self.habits.get_mut(habit_idx) {
                h.toggle_check_in(date);
                let checked = h.is_checked_on(date);
                self.status_msg = if checked {
                    format!("Checked in: {} on {}", h.name, date.format_short())
                } else {
                    format!("Unchecked: {} on {}", h.name, date.format_short())
                };
            }
        }
    }

    fn advance_day(&mut self) {
        self.today = self.today.add_days(1);
        self.status_msg = format!("Date: {}", self.today.format_full());
    }

    fn go_back_day(&mut self) {
        self.today = self.today.add_days(-1);
        self.status_msg = format!("Date: {}", self.today.format_full());
    }

    // ── Input handling ──────────────────────────────────────────────

    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        if self.show_create_form {
            self.handle_create_form_key(key, ctrl);
            return;
        }

        match key {
            "1" => self.screen = Screen::Dashboard,
            "2" => self.screen = Screen::Statistics,
            "3" => self.screen = Screen::Archive,
            "4" => self.screen = Screen::HeatMap,
            "n" | "N" if !ctrl => {
                self.show_create_form = true;
                self.status_msg = String::from("New habit -- fill in details");
            }
            "Up" => {
                if self.selected_habit > 0 {
                    self.selected_habit -= 1;
                }
            }
            "Down" => {
                let max = match self.screen {
                    Screen::Dashboard => self.active_habits().len(),
                    Screen::Archive => self.archived_habits().len(),
                    _ => 0,
                };
                if max > 0 && self.selected_habit < max - 1 {
                    self.selected_habit += 1;
                }
            }
            "Left" => {
                if self.screen == Screen::Dashboard && self.selected_day_col < 6 {
                    self.selected_day_col += 1;
                }
                if self.screen == Screen::HeatMap && self.heatmap_habit_idx > 0 {
                    self.heatmap_habit_idx -= 1;
                }
            }
            "Right" => {
                if self.screen == Screen::Dashboard && self.selected_day_col > 0 {
                    self.selected_day_col -= 1;
                }
                if self.screen == Screen::HeatMap {
                    let active = self.active_habits();
                    if !active.is_empty() && self.heatmap_habit_idx < active.len() - 1 {
                        self.heatmap_habit_idx += 1;
                    }
                }
            }
            "Space" | "Return" => {
                match self.screen {
                    Screen::Dashboard => self.toggle_check_in_selected(),
                    Screen::Archive => {
                        let archived = self.archived_habits();
                        if let Some(&idx) = archived.get(self.selected_habit) {
                            self.unarchive_habit(idx);
                        }
                    }
                    _ => {}
                }
            }
            "d" | "D" if ctrl => {
                let active = self.active_habits();
                if let Some(&idx) = active.get(self.selected_habit) {
                    self.delete_habit(idx);
                }
            }
            "a" | "A" if !ctrl => {
                if self.screen == Screen::Dashboard {
                    let active = self.active_habits();
                    if let Some(&idx) = active.get(self.selected_habit) {
                        self.archive_habit(idx);
                    }
                }
            }
            "+" | "=" => self.advance_day(),
            "-" => self.go_back_day(),
            "c" | "C" => {
                // Cycle category filter
                self.category_filter = match self.category_filter {
                    None => Some(Category::ALL[0]),
                    Some(cat) => {
                        let idx = Category::ALL.iter().position(|&c| c == cat).unwrap_or(0);
                        if idx + 1 < Category::ALL.len() {
                            Some(Category::ALL[idx + 1])
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
            "PageDown" => {
                self.scroll_offset = (self.scroll_offset + 200.0).min(2000.0);
            }
            "PageUp" => {
                self.scroll_offset = (self.scroll_offset - 200.0).max(0.0);
            }
            _ => {}
        }
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
                self.create_category_idx = (self.create_category_idx + 1) % Category::ALL.len();
            }
            "F1" => {
                self.create_frequency_daily = !self.create_frequency_daily;
            }
            "F2" => {
                self.create_weekly_count = (self.create_weekly_count % 7) + 1;
            }
            "Backspace" => {
                self.create_name.pop();
            }
            _ => {
                if key.len() == 1 && self.create_name.len() < 40 {
                    self.create_name.push_str(key);
                }
            }
        }
    }

    // ── Statistics helpers ───────────────────────────────────────────

    fn overall_completion_today(&self) -> (u32, u32) {
        let active = self.active_habits();
        let total = active.len() as u32;
        let mut done = 0u32;
        for &idx in &active {
            if let Some(h) = self.habits.get(idx) {
                if h.is_checked_on(self.today) {
                    done += 1;
                }
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
        let sum: f32 = active.iter()
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
        let sum: f32 = active.iter()
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
                let d = self.today.add_days(-(i as i32));
                result.push((d, h.is_checked_on(d)));
            }
        }
        result
    }

    fn heatmap_color(checked: bool, intensity: f32) -> Color {
        if !checked {
            return SURFACE0;
        }
        // Blend GREEN with intensity
        let alpha = (intensity * 255.0).clamp(80.0, 255.0) as u8;
        Color::rgba(166, 227, 161, alpha)
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: BASE, corner_radii: CornerRadii::ZERO,
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

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: Self::HEADER_H,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 16.0, y: 14.0,
            text: String::from("\u{1F4CB} Habit Tracker"),
            font_size: 20.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Date display
        cmds.push(RenderCommand::Text {
            x: 240.0, y: 18.0,
            text: format!("{} {} -- {}", self.today.day_of_week_short(), self.today.format_full(), self.today.day_of_week_short()),
            font_size: 14.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(280.0),
        });

        // Today's progress
        let (done, total) = self.overall_completion_today();
        let progress_text = format!("Today: {done}/{total}");
        cmds.push(RenderCommand::Text {
            x: self.width - 200.0, y: 10.0,
            text: progress_text,
            font_size: 16.0, color: GREEN,
            font_weight: FontWeightHint::Bold,
            max_width: Some(180.0),
        });

        // Progress bar
        let bar_x = self.width - 200.0;
        let bar_y = 32.0;
        let bar_w = 160.0;
        let bar_h = 8.0;
        cmds.push(RenderCommand::FillRect {
            x: bar_x, y: bar_y, width: bar_w, height: bar_h,
            color: SURFACE0, corner_radii: CornerRadii::all(4.0),
        });
        if total > 0 {
            let fill = bar_w * (done as f32 / total as f32);
            if fill > 0.5 {
                cmds.push(RenderCommand::FillRect {
                    x: bar_x, y: bar_y, width: fill, height: bar_h,
                    color: GREEN, corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        // New habit button
        let btn_x = 560.0;
        cmds.push(RenderCommand::FillRect {
            x: btn_x, y: 10.0, width: 90.0, height: 30.0,
            color: BLUE, corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: btn_x + 10.0, y: 17.0,
            text: String::from("+ New Habit"),
            font_size: 12.0, color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
        });
    }

    fn render_nav(&self, cmds: &mut Vec<RenderCommand>) {
        let y = Self::HEADER_H;
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y, width: self.width, height: Self::NAV_H,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });

        let tabs = [Screen::Dashboard, Screen::Statistics, Screen::Archive, Screen::HeatMap];
        let mut tx = 16.0;
        for (i, tab) in tabs.iter().enumerate() {
            let selected = *tab == self.screen;
            let bg = if selected { BLUE } else { SURFACE1 };
            let fg = if selected { CRUST } else { TEXT_COLOR };
            let w = 90.0;
            cmds.push(RenderCommand::FillRect {
                x: tx, y: y + 4.0, width: w, height: 28.0,
                color: bg, corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0, y: y + 10.0,
                text: format!("{} {}", i + 1, tab.label()),
                font_size: 11.0, color: fg,
                font_weight: if selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(w - 16.0),
            });
            tx += w + 6.0;
        }

        // Category filter indicator
        if let Some(cat) = self.category_filter {
            cmds.push(RenderCommand::FillRect {
                x: tx + 20.0, y: y + 6.0, width: 120.0, height: 24.0,
                color: cat.color(), corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 30.0, y: y + 10.0,
                text: format!("{} {}", cat.icon(), cat.label()),
                font_size: 11.0, color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
            });
        }
    }

    fn render_dashboard(&self, cmds: &mut Vec<RenderCommand>, start_y: f32) {
        let active = self.active_habits();
        if active.is_empty() {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 100.0, y: start_y + 80.0,
                text: String::from("No habits yet. Press N to create one."),
                font_size: 16.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
            return;
        }

        // Column headers: day labels
        let name_col_w = 200.0;
        let days_start_x = name_col_w + 80.0;
        for col in 0..7 {
            let d = self.today.add_days(-(col as i32));
            let cx = days_start_x + (6 - col) as f32 * Self::DAY_COL_W;
            let label = if col == 0 {
                String::from("Today")
            } else {
                d.day_of_week_short().to_string()
            };
            let label_color = if col == self.selected_day_col { BLUE } else { SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: cx + 2.0, y: start_y + 6.0,
                text: label,
                font_size: 10.0, color: label_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(Self::DAY_COL_W),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 6.0, y: start_y + 18.0,
                text: format!("{:02}", d.day),
                font_size: 9.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(Self::DAY_COL_W),
            });
        }

        // Streak/rate headers
        let stats_x = days_start_x + 7.0 * Self::DAY_COL_W + 12.0;
        cmds.push(RenderCommand::Text {
            x: stats_x, y: start_y + 8.0,
            text: String::from("Streak"),
            font_size: 10.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(50.0),
        });
        cmds.push(RenderCommand::Text {
            x: stats_x + 56.0, y: start_y + 8.0,
            text: String::from("7d"),
            font_size: 10.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(30.0),
        });
        cmds.push(RenderCommand::Text {
            x: stats_x + 90.0, y: start_y + 8.0,
            text: String::from("30d"),
            font_size: 10.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(30.0),
        });

        let row_start_y = start_y + 32.0 - self.scroll_offset;

        for (vi, &habit_idx) in active.iter().enumerate() {
            let Some(habit) = self.habits.get(habit_idx) else { continue };
            let ry = row_start_y + vi as f32 * Self::ROW_H;

            if ry + Self::ROW_H < start_y || ry > self.height - Self::STATUS_H {
                continue; // clipping
            }

            let is_selected = vi == self.selected_habit;
            let row_bg = if is_selected { SURFACE1 } else if vi % 2 == 0 { SURFACE0 } else { BASE };

            cmds.push(RenderCommand::FillRect {
                x: 8.0, y: ry, width: self.width - 16.0, height: Self::ROW_H - 2.0,
                color: row_bg, corner_radii: CornerRadii::all(6.0),
            });

            // Category color dot
            cmds.push(RenderCommand::FillRect {
                x: 16.0, y: ry + 16.0, width: 10.0, height: 10.0,
                color: habit.category.color(), corner_radii: CornerRadii::all(5.0),
            });

            // Habit name
            cmds.push(RenderCommand::Text {
                x: 32.0, y: ry + 8.0,
                text: habit.name.clone(),
                font_size: 14.0, color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(160.0),
            });

            // Frequency label
            cmds.push(RenderCommand::Text {
                x: 32.0, y: ry + 28.0,
                text: habit.frequency.label(),
                font_size: 10.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });

            // Check-in dots for last 7 days
            for col in 0..7 {
                let d = self.today.add_days(-(col as i32));
                let cx = days_start_x + (6 - col) as f32 * Self::DAY_COL_W;
                let checked = habit.is_checked_on(d);
                let cell_selected = is_selected && col == self.selected_day_col;

                let dot_color = if checked { GREEN } else { SURFACE2 };
                let dot_size = if cell_selected { 22.0 } else { 18.0 };
                let dot_x = cx + (Self::DAY_COL_W - dot_size) / 2.0;
                let dot_y = ry + (Self::ROW_H - 2.0 - dot_size) / 2.0;

                cmds.push(RenderCommand::FillRect {
                    x: dot_x, y: dot_y, width: dot_size, height: dot_size,
                    color: dot_color, corner_radii: CornerRadii::all(dot_size / 2.0),
                });

                if checked {
                    cmds.push(RenderCommand::Text {
                        x: dot_x + 3.0, y: dot_y + 2.0,
                        text: String::from("\u{2713}"),
                        font_size: 12.0, color: CRUST,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(dot_size),
                    });
                }

                if cell_selected {
                    cmds.push(RenderCommand::StrokeRect {
                        x: dot_x - 2.0, y: dot_y - 2.0,
                        width: dot_size + 4.0, height: dot_size + 4.0,
                        color: BLUE, line_width: 2.0,
                        corner_radii: CornerRadii::all((dot_size + 4.0) / 2.0),
                    });
                }
            }

            // Streak
            let streak = habit.current_streak(self.today);
            let streak_color = if streak > 0 { PEACH } else { OVERLAY0 };
            cmds.push(RenderCommand::Text {
                x: stats_x, y: ry + 16.0,
                text: format!("{streak}"),
                font_size: 14.0, color: streak_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(40.0),
            });

            // 7-day rate
            let rate_7 = habit.completion_rate(self.today, 7);
            let rate_7_color = rate_color(rate_7);
            cmds.push(RenderCommand::Text {
                x: stats_x + 50.0, y: ry + 16.0,
                text: format!("{}%", (rate_7 * 100.0) as u32),
                font_size: 12.0, color: rate_7_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
            });

            // 30-day rate
            let rate_30 = habit.completion_rate(self.today, 30);
            let rate_30_color = rate_color(rate_30);
            cmds.push(RenderCommand::Text {
                x: stats_x + 86.0, y: ry + 16.0,
                text: format!("{}%", (rate_30 * 100.0) as u32),
                font_size: 12.0, color: rate_30_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
            });
        }
    }

    fn render_statistics(&self, cmds: &mut Vec<RenderCommand>, start_y: f32) {
        let pad = 20.0;
        let card_w = (self.width - pad * 3.0) / 2.0;
        let card_h = 100.0;

        // Card 1: Overall today
        let (done, total) = self.overall_completion_today();
        self.render_stat_card(
            cmds, pad, start_y + pad, card_w, card_h,
            "Today's Progress",
            &format!("{done} / {total}"),
            if total > 0 && done == total { GREEN } else { BLUE },
        );

        // Card 2: Best streak
        let (best_name, best_val) = self.best_habit_streak();
        self.render_stat_card(
            cmds, pad * 2.0 + card_w, start_y + pad, card_w, card_h,
            "Best Streak",
            &format!("{best_val} ({best_name})"),
            PEACH,
        );

        // Card 3: 7-day avg
        let avg_7 = self.average_completion_7d();
        self.render_stat_card(
            cmds, pad, start_y + pad * 2.0 + card_h, card_w, card_h,
            "7-Day Average",
            &format!("{}%", (avg_7 * 100.0) as u32),
            rate_color(avg_7),
        );

        // Card 4: 30-day avg
        let avg_30 = self.average_completion_30d();
        self.render_stat_card(
            cmds, pad * 2.0 + card_w, start_y + pad * 2.0 + card_h, card_w, card_h,
            "30-Day Average",
            &format!("{}%", (avg_30 * 100.0) as u32),
            rate_color(avg_30),
        );

        // Per-habit stats table
        let table_y = start_y + pad * 3.0 + card_h * 2.0 + 10.0;
        cmds.push(RenderCommand::Text {
            x: pad, y: table_y,
            text: String::from("Per-Habit Statistics"),
            font_size: 16.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        let headers = ["Habit", "Category", "Streak", "Best", "7d", "30d", "All", "Total"];
        let col_widths: [f32; 8] = [140.0, 90.0, 50.0, 50.0, 50.0, 50.0, 50.0, 50.0];
        let mut hx = pad;
        for (i, header) in headers.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: hx, y: table_y + 24.0,
                text: header.to_string(),
                font_size: 10.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_widths[i]),
            });
            hx += col_widths[i];
        }

        cmds.push(RenderCommand::Line {
            x1: pad, y1: table_y + 38.0,
            x2: self.width - pad, y2: table_y + 38.0,
            color: SURFACE1, width: 1.0,
        });

        let active = self.active_habits();
        for (vi, &idx) in active.iter().enumerate() {
            let Some(h) = self.habits.get(idx) else { continue };
            let ry = table_y + 44.0 + vi as f32 * 24.0;
            if ry > self.height - Self::STATUS_H {
                break;
            }

            let mut cx = pad;
            // Name
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: h.name.clone(),
                font_size: 11.0, color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[0] - 4.0),
            });
            cx += col_widths[0];

            // Category
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: h.category.label().to_string(),
                font_size: 11.0, color: h.category.color(),
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[1] - 4.0),
            });
            cx += col_widths[1];

            // Current streak
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: format!("{}", h.current_streak(self.today)),
                font_size: 11.0, color: PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[2] - 4.0),
            });
            cx += col_widths[2];

            // Best streak
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: format!("{}", h.best_streak(self.today)),
                font_size: 11.0, color: YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[3] - 4.0),
            });
            cx += col_widths[3];

            // 7d rate
            let r7 = h.completion_rate(self.today, 7);
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: format!("{}%", (r7 * 100.0) as u32),
                font_size: 11.0, color: rate_color(r7),
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[4] - 4.0),
            });
            cx += col_widths[4];

            // 30d rate
            let r30 = h.completion_rate(self.today, 30);
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: format!("{}%", (r30 * 100.0) as u32),
                font_size: 11.0, color: rate_color(r30),
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[5] - 4.0),
            });
            cx += col_widths[5];

            // All-time rate
            let ra = h.completion_rate_alltime(self.today);
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: format!("{}%", (ra * 100.0) as u32),
                font_size: 11.0, color: rate_color(ra),
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[6] - 4.0),
            });
            cx += col_widths[6];

            // Total check-ins
            cmds.push(RenderCommand::Text {
                x: cx, y: ry,
                text: format!("{}", h.total_check_ins()),
                font_size: 11.0, color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[7] - 4.0),
            });
        }
    }

    fn render_stat_card(
        &self, cmds: &mut Vec<RenderCommand>,
        x: f32, y: f32, w: f32, h: f32,
        title: &str, value: &str, accent: Color,
    ) {
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: SURFACE0, corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::FillRect {
            x, y, width: 4.0, height: h,
            color: accent, corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0, y: y + 14.0,
            text: title.to_string(),
            font_size: 12.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 32.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0, y: y + 40.0,
            text: value.to_string(),
            font_size: 24.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 32.0),
        });
    }

    fn render_archive(&self, cmds: &mut Vec<RenderCommand>, start_y: f32) {
        let archived = self.archived_habits();

        if archived.is_empty() {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 120.0, y: start_y + 80.0,
                text: String::from("No archived habits. Press A to archive one."),
                font_size: 16.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
            return;
        }

        cmds.push(RenderCommand::Text {
            x: 20.0, y: start_y + 12.0,
            text: format!("Archived Habits ({})", archived.len()),
            font_size: 16.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        for (vi, &idx) in archived.iter().enumerate() {
            let Some(h) = self.habits.get(idx) else { continue };
            let ry = start_y + 42.0 + vi as f32 * Self::ROW_H;
            if ry > self.height - Self::STATUS_H {
                break;
            }

            let is_selected = vi == self.selected_habit;
            let bg = if is_selected { SURFACE1 } else { SURFACE0 };

            cmds.push(RenderCommand::FillRect {
                x: 16.0, y: ry, width: self.width - 32.0, height: Self::ROW_H - 4.0,
                color: bg, corner_radii: CornerRadii::all(6.0),
            });

            // Category dot
            cmds.push(RenderCommand::FillRect {
                x: 28.0, y: ry + 16.0, width: 10.0, height: 10.0,
                color: h.category.color(), corner_radii: CornerRadii::all(5.0),
            });

            cmds.push(RenderCommand::Text {
                x: 46.0, y: ry + 8.0,
                text: h.name.clone(),
                font_size: 14.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });

            cmds.push(RenderCommand::Text {
                x: 46.0, y: ry + 28.0,
                text: format!("{} -- {} check-ins", h.category.label(), h.total_check_ins()),
                font_size: 10.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });

            // Restore hint
            if is_selected {
                cmds.push(RenderCommand::Text {
                    x: self.width - 180.0, y: ry + 14.0,
                    text: String::from("Enter to restore"),
                    font_size: 11.0, color: BLUE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(140.0),
                });
            }
        }
    }

    fn render_heatmap_screen(&self, cmds: &mut Vec<RenderCommand>, start_y: f32) {
        let active = self.active_habits();
        if active.is_empty() {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 80.0, y: start_y + 80.0,
                text: String::from("No habits to display."),
                font_size: 16.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
            return;
        }

        let habit_idx = active.get(self.heatmap_habit_idx.min(active.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        let Some(habit) = self.habits.get(habit_idx) else { return };

        // Title
        cmds.push(RenderCommand::Text {
            x: 20.0, y: start_y + 12.0,
            text: format!("Contribution Graph: {}", habit.name),
            font_size: 16.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
        });

        // Navigation hint
        cmds.push(RenderCommand::Text {
            x: 20.0, y: start_y + 34.0,
            text: String::from("Left/Right to switch habits"),
            font_size: 11.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

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
                    x: 16.0, y: hy_start + di as f32 * (cell + gap) + 1.0,
                    text: label.to_string(),
                    font_size: 9.0, color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(40.0),
                });
            }
        }

        // Draw cells
        for (i, (_, checked)) in data.iter().enumerate() {
            let week = i / 7;
            let day = i % 7;
            let cx = hx_start + week as f32 * (cell + gap);
            let cy = hy_start + day as f32 * (cell + gap);

            if cx + cell > self.width - 20.0 {
                break;
            }

            let color = if *checked { GREEN } else { SURFACE0 };
            cmds.push(RenderCommand::FillRect {
                x: cx, y: cy, width: cell, height: cell,
                color, corner_radii: CornerRadii::all(2.0),
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
                        x: mx, y: hy_start - 14.0,
                        text: date.month_short().to_string(),
                        font_size: 9.0, color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(30.0),
                    });
                }
            }
        }

        // Legend
        let ly = hy_start + 7.0 * (cell + gap) + 16.0;
        cmds.push(RenderCommand::Text {
            x: hx_start, y: ly,
            text: String::from("Less"),
            font_size: 10.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
        });
        let legend_colors = [SURFACE0, Color::rgba(166, 227, 161, 80), Color::rgba(166, 227, 161, 160), GREEN];
        for (li, lc) in legend_colors.iter().enumerate() {
            cmds.push(RenderCommand::FillRect {
                x: hx_start + 36.0 + li as f32 * (cell + 2.0), y: ly,
                width: cell, height: cell,
                color: *lc, corner_radii: CornerRadii::all(2.0),
            });
        }
        cmds.push(RenderCommand::Text {
            x: hx_start + 36.0 + 4.0 * (cell + 2.0) + 4.0, y: ly,
            text: String::from("More"),
            font_size: 10.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
        });

        // Stats summary below heatmap
        let sy = ly + 36.0;
        let streak = habit.current_streak(self.today);
        let best = habit.best_streak(self.today);
        let r7 = habit.completion_rate(self.today, 7);
        let r30 = habit.completion_rate(self.today, 30);
        let ra = habit.completion_rate_alltime(self.today);

        let stats_items = [
            (format!("Current Streak: {streak}"), PEACH),
            (format!("Best Streak: {best}"), YELLOW),
            (format!("7-day: {}%", (r7 * 100.0) as u32), rate_color(r7)),
            (format!("30-day: {}%", (r30 * 100.0) as u32), rate_color(r30)),
            (format!("All-time: {}%", (ra * 100.0) as u32), rate_color(ra)),
            (format!("Total check-ins: {}", habit.total_check_ins()), TEXT_COLOR),
        ];

        for (si, (text, color)) in stats_items.iter().enumerate() {
            let col = si % 3;
            let row = si / 3;
            cmds.push(RenderCommand::Text {
                x: 20.0 + col as f32 * 200.0, y: sy + row as f32 * 22.0,
                text: text.clone(),
                font_size: 12.0, color: *color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(190.0),
            });
        }
    }

    fn render_create_form(&self, cmds: &mut Vec<RenderCommand>) {
        // Modal overlay
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: Color::rgba(0, 0, 0, 160), corner_radii: CornerRadii::ZERO,
        });

        let fw = 400.0;
        let fh = 300.0;
        let fx = (self.width - fw) / 2.0;
        let fy = (self.height - fh) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: fx, y: fy, width: fw, height: fh,
            color: MANTLE, corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: fx, y: fy, width: fw, height: fh,
            color: SURFACE1, line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: fx + 20.0, y: fy + 16.0,
            text: String::from("New Habit"),
            font_size: 18.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Name field
        cmds.push(RenderCommand::Text {
            x: fx + 20.0, y: fy + 54.0,
            text: String::from("Name:"),
            font_size: 12.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: fx + 80.0, y: fy + 48.0, width: 290.0, height: 28.0,
            color: SURFACE0, corner_radii: CornerRadii::all(4.0),
        });
        let display_name = if self.create_name.is_empty() {
            String::from("Type a name...")
        } else {
            self.create_name.clone()
        };
        let name_color = if self.create_name.is_empty() { OVERLAY0 } else { TEXT_COLOR };
        cmds.push(RenderCommand::Text {
            x: fx + 88.0, y: fy + 54.0,
            text: display_name,
            font_size: 12.0, color: name_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(270.0),
        });

        // Category
        let cat = Category::ALL.get(self.create_category_idx).copied().unwrap_or(Category::Custom);
        cmds.push(RenderCommand::Text {
            x: fx + 20.0, y: fy + 94.0,
            text: String::from("Category:"),
            font_size: 12.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: fx + 110.0, y: fy + 88.0, width: 140.0, height: 24.0,
            color: cat.color(), corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::Text {
            x: fx + 120.0, y: fy + 92.0,
            text: format!("{} {} (Tab)", cat.icon(), cat.label()),
            font_size: 11.0, color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });

        // Frequency
        cmds.push(RenderCommand::Text {
            x: fx + 20.0, y: fy + 134.0,
            text: String::from("Frequency:"),
            font_size: 12.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });
        let freq_text = if self.create_frequency_daily {
            String::from("Daily (F1 to toggle)")
        } else {
            format!("{}x/week (F1 toggle, F2 count)", self.create_weekly_count)
        };
        cmds.push(RenderCommand::Text {
            x: fx + 110.0, y: fy + 134.0,
            text: freq_text,
            font_size: 12.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(250.0),
        });

        // Buttons
        cmds.push(RenderCommand::FillRect {
            x: fx + 100.0, y: fy + fh - 60.0, width: 90.0, height: 32.0,
            color: GREEN, corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: fx + 118.0, y: fy + fh - 52.0,
            text: String::from("Create"),
            font_size: 13.0, color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(70.0),
        });

        cmds.push(RenderCommand::FillRect {
            x: fx + 210.0, y: fy + fh - 60.0, width: 90.0, height: 32.0,
            color: SURFACE1, corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: fx + 228.0, y: fy + fh - 52.0,
            text: String::from("Cancel"),
            font_size: 13.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(70.0),
        });
    }

    fn render_status(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - Self::STATUS_H;
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y, width: self.width, height: Self::STATUS_H,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 12.0, y: y + 7.0,
            text: self.status_msg.clone(),
            font_size: 11.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: self.width - 320.0, y: y + 7.0,
            text: String::from("N:New  A:Archive  C:Filter  Space:Check  +/-:Date"),
            font_size: 10.0, color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(310.0),
        });
    }
}

fn rate_color(rate: f32) -> Color {
    if rate >= 0.8 {
        GREEN
    } else if rate >= 0.5 {
        YELLOW
    } else if rate >= 0.3 {
        PEACH
    } else {
        RED
    }
}

fn main() {
    let _app = HabitTrackerApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_leap_year() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn test_days_in_february() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
    }

    #[test]
    fn test_date_add_days_forward() {
        let d = Date { year: 2026, month: 5, day: 30 };
        let next = d.add_days(2);
        assert_eq!(next.month, 6);
        assert_eq!(next.day, 1);
    }

    #[test]
    fn test_date_add_days_backward() {
        let d = Date { year: 2026, month: 6, day: 1 };
        let prev = d.add_days(-2);
        assert_eq!(prev.month, 5);
        assert_eq!(prev.day, 30);
    }

    #[test]
    fn test_date_add_days_year_boundary() {
        let d = Date { year: 2026, month: 12, day: 31 };
        let next = d.add_days(1);
        assert_eq!(next.year, 2027);
        assert_eq!(next.month, 1);
        assert_eq!(next.day, 1);
    }

    #[test]
    fn test_date_add_days_backward_year() {
        let d = Date { year: 2026, month: 1, day: 1 };
        let prev = d.add_days(-1);
        assert_eq!(prev.year, 2025);
        assert_eq!(prev.month, 12);
        assert_eq!(prev.day, 31);
    }

    #[test]
    fn test_days_since() {
        let d1 = Date { year: 2026, month: 5, day: 18 };
        let d2 = Date { year: 2026, month: 5, day: 15 };
        assert_eq!(d1.days_since(d2), 3);
        assert_eq!(d2.days_since(d1), -3);
    }

    #[test]
    fn test_day_of_week() {
        // 2026-05-18 is a Monday
        let d = Date { year: 2026, month: 5, day: 18 };
        assert_eq!(d.day_of_week(), 1); // 1=Monday
    }

    #[test]
    fn test_day_of_week_sunday() {
        // 2026-05-17 is a Sunday
        let d = Date { year: 2026, month: 5, day: 17 };
        assert_eq!(d.day_of_week(), 0); // 0=Sunday
    }

    #[test]
    fn test_week_start_monday() {
        let d = Date { year: 2026, month: 5, day: 20 }; // Wednesday
        let ws = d.week_start_monday();
        assert_eq!(ws.day_of_week(), 1); // Monday
        assert!(ws.to_day_number() <= d.to_day_number());
        assert!(d.to_day_number() - ws.to_day_number() < 7);
    }

    #[test]
    fn test_format_short() {
        let d = Date { year: 2026, month: 5, day: 18 };
        assert_eq!(d.format_short(), "May 18");
    }

    #[test]
    fn test_format_full() {
        let d = Date { year: 2026, month: 5, day: 18 };
        assert_eq!(d.format_full(), "May 18, 2026");
    }

    #[test]
    fn test_to_day_number_monotonic() {
        let d1 = Date { year: 2026, month: 1, day: 1 };
        let d2 = Date { year: 2026, month: 12, day: 31 };
        assert!(d2.to_day_number() > d1.to_day_number());
    }

    #[test]
    fn test_date_ordering() {
        let d1 = Date { year: 2026, month: 5, day: 1 };
        let d2 = Date { year: 2026, month: 5, day: 18 };
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
        let d = Date { year: 2026, month: 5, day: 1 };
        let h = Habit::new(1, "Test", "Desc", Category::Health, Frequency::Daily, d);
        assert_eq!(h.id, 1);
        assert_eq!(h.name, "Test");
        assert!(!h.archived);
        assert!(h.check_ins.is_empty());
    }

    #[test]
    fn test_habit_toggle_check_in() {
        let d = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, d);
        assert!(!h.is_checked_on(d));
        h.toggle_check_in(d);
        assert!(h.is_checked_on(d));
        h.toggle_check_in(d);
        assert!(!h.is_checked_on(d));
    }

    #[test]
    fn test_habit_multiple_check_ins() {
        let base = Date { year: 2026, month: 5, day: 1 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, base);
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
        let base = Date { year: 2026, month: 5, day: 1 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, base);
        h.toggle_check_in(base.add_days(5));
        h.toggle_check_in(base.add_days(2));
        h.toggle_check_in(base.add_days(8));
        assert!(h.check_ins.windows(2).all(|w| w[0] <= w[1]));
    }

    // ── Streak tests ────────────────────────────────────────────────

    #[test]
    fn test_streak_daily_consecutive() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today.add_days(-10));
        for i in 0..5 {
            h.check_ins.push(today.add_days(-i));
        }
        assert_eq!(h.current_streak(today), 5);
    }

    #[test]
    fn test_streak_daily_with_gap() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today.add_days(-10));
        h.check_ins.push(today);
        h.check_ins.push(today.add_days(-1));
        // Gap on -2
        h.check_ins.push(today.add_days(-3));
        assert_eq!(h.current_streak(today), 2);
    }

    #[test]
    fn test_streak_daily_today_not_checked() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today.add_days(-10));
        // Checked yesterday and day before
        h.check_ins.push(today.add_days(-1));
        h.check_ins.push(today.add_days(-2));
        // Today not checked -- streak should count from yesterday
        assert_eq!(h.current_streak(today), 2);
    }

    #[test]
    fn test_streak_daily_empty() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today);
        assert_eq!(h.current_streak(today), 0);
    }

    #[test]
    fn test_best_streak_daily() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today.add_days(-20));
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
        let today = Date { year: 2026, month: 5, day: 18 };
        let start = today.add_days(-28);
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Weekly(3), start);
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
        let today = Date { year: 2026, month: 5, day: 18 };
        let start = today.add_days(-60);
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Weekly(2), start);
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
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today.add_days(-10));
        for i in 0..7 {
            h.check_ins.push(today.add_days(-i));
        }
        let rate = h.completion_rate(today, 7);
        assert!((rate - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_completion_rate_daily_half() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today.add_days(-10));
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
        let today = Date { year: 2026, month: 5, day: 18 };
        let h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today);
        assert_eq!(h.completion_rate(today, 0), 0.0);
    }

    #[test]
    fn test_completion_rate_weekly() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let start = today.add_days(-28);
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Weekly(3), start);
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
        let today = Date { year: 2026, month: 5, day: 18 };
        let created = today.add_days(-10);
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, created);
        // 5 out of 11 days (inclusive)
        for i in 0..5 {
            h.check_ins.push(today.add_days(-i));
        }
        let rate = h.completion_rate_alltime(today);
        assert!(rate > 0.0 && rate <= 1.0);
    }

    #[test]
    fn test_total_check_ins() {
        let today = Date { year: 2026, month: 5, day: 18 };
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Daily, today.add_days(-10));
        for i in 0..5 {
            h.check_ins.push(today.add_days(-i));
        }
        assert_eq!(h.total_check_ins(), 5);
    }

    // ── App creation tests ──────────────────────────────────────────

    #[test]
    fn test_app_new_has_sample_habits() {
        let app = HabitTrackerApp::new();
        assert_eq!(app.habits.len(), 5);
    }

    #[test]
    fn test_app_sample_habits_have_check_ins() {
        let app = HabitTrackerApp::new();
        for h in &app.habits {
            assert!(!h.check_ins.is_empty(), "Habit '{}' should have check-ins", h.name);
        }
    }

    #[test]
    fn test_app_default_screen() {
        let app = HabitTrackerApp::new();
        assert_eq!(app.screen, Screen::Dashboard);
    }

    #[test]
    fn test_app_date() {
        let app = HabitTrackerApp::new();
        assert_eq!(app.today.year, 2026);
        assert_eq!(app.today.month, 5);
        assert_eq!(app.today.day, 18);
    }

    // ── Habit management tests ──────────────────────────────────────

    #[test]
    fn test_create_habit() {
        let mut app = HabitTrackerApp::new();
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
        let mut app = HabitTrackerApp::new();
        let count = app.habits.len();
        app.create_name.clear();
        app.create_habit();
        assert_eq!(app.habits.len(), count);
    }

    #[test]
    fn test_create_habit_weekly() {
        let mut app = HabitTrackerApp::new();
        app.create_name = String::from("Weekly");
        app.create_frequency_daily = false;
        app.create_weekly_count = 4;
        app.create_habit();
        let last = app.habits.last().unwrap();
        assert_eq!(last.frequency, Frequency::Weekly(4));
    }

    #[test]
    fn test_create_habit_clears_form() {
        let mut app = HabitTrackerApp::new();
        app.create_name = String::from("Test");
        app.show_create_form = true;
        app.create_habit();
        assert!(app.create_name.is_empty());
        assert!(!app.show_create_form);
    }

    #[test]
    fn test_delete_habit() {
        let mut app = HabitTrackerApp::new();
        let count = app.habits.len();
        app.delete_habit(0);
        assert_eq!(app.habits.len(), count - 1);
    }

    #[test]
    fn test_delete_habit_out_of_bounds() {
        let mut app = HabitTrackerApp::new();
        let count = app.habits.len();
        app.delete_habit(100);
        assert_eq!(app.habits.len(), count);
    }

    #[test]
    fn test_archive_habit() {
        let mut app = HabitTrackerApp::new();
        app.archive_habit(0);
        assert!(app.habits[0].archived);
    }

    #[test]
    fn test_unarchive_habit() {
        let mut app = HabitTrackerApp::new();
        app.habits[0].archived = true;
        app.unarchive_habit(0);
        assert!(!app.habits[0].archived);
    }

    #[test]
    fn test_active_habits_excludes_archived() {
        let mut app = HabitTrackerApp::new();
        let before = app.active_habits().len();
        app.habits[0].archived = true;
        let after = app.active_habits().len();
        assert_eq!(after, before - 1);
    }

    #[test]
    fn test_archived_habits_list() {
        let mut app = HabitTrackerApp::new();
        assert!(app.archived_habits().is_empty());
        app.habits[0].archived = true;
        app.habits[1].archived = true;
        assert_eq!(app.archived_habits().len(), 2);
    }

    // ── Category filter tests ───────────────────────────────────────

    #[test]
    fn test_category_filter_none_shows_all() {
        let app = HabitTrackerApp::new();
        assert!(app.category_filter.is_none());
        assert_eq!(app.active_habits().len(), 5);
    }

    #[test]
    fn test_category_filter_fitness() {
        let mut app = HabitTrackerApp::new();
        app.category_filter = Some(Category::Fitness);
        let active = app.active_habits();
        assert_eq!(active.len(), 1);
        assert_eq!(app.habits[active[0]].name, "Exercise");
    }

    #[test]
    fn test_category_filter_no_match() {
        let mut app = HabitTrackerApp::new();
        app.category_filter = Some(Category::Finance);
        assert!(app.active_habits().is_empty());
    }

    // ── Check-in toggle tests ───────────────────────────────────────

    #[test]
    fn test_toggle_check_in_selected() {
        let mut app = HabitTrackerApp::new();
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
        let mut app = HabitTrackerApp::new();
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
        let mut app = HabitTrackerApp::new();
        let original = app.today;
        app.advance_day();
        assert_eq!(app.today.days_since(original), 1);
    }

    #[test]
    fn test_go_back_day() {
        let mut app = HabitTrackerApp::new();
        let original = app.today;
        app.go_back_day();
        assert_eq!(original.days_since(app.today), 1);
    }

    // ── Key handling tests ──────────────────────────────────────────

    #[test]
    fn test_key_screen_switch() {
        let mut app = HabitTrackerApp::new();
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
        let mut app = HabitTrackerApp::new();
        app.handle_key("n", false, false);
        assert!(app.show_create_form);
    }

    #[test]
    fn test_key_up_down() {
        let mut app = HabitTrackerApp::new();
        assert_eq!(app.selected_habit, 0);
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_habit, 1);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_habit, 0);
    }

    #[test]
    fn test_key_up_boundary() {
        let mut app = HabitTrackerApp::new();
        app.selected_habit = 0;
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_habit, 0);
    }

    #[test]
    fn test_key_down_boundary() {
        let mut app = HabitTrackerApp::new();
        let max = app.active_habits().len() - 1;
        app.selected_habit = max;
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_habit, max);
    }

    #[test]
    fn test_key_left_right_day_col() {
        let mut app = HabitTrackerApp::new();
        assert_eq!(app.selected_day_col, 0);
        app.handle_key("Left", false, false);
        assert_eq!(app.selected_day_col, 1);
        app.handle_key("Right", false, false);
        assert_eq!(app.selected_day_col, 0);
    }

    #[test]
    fn test_key_left_boundary() {
        let mut app = HabitTrackerApp::new();
        app.selected_day_col = 6;
        app.handle_key("Left", false, false);
        assert_eq!(app.selected_day_col, 6);
    }

    #[test]
    fn test_key_right_boundary() {
        let mut app = HabitTrackerApp::new();
        app.selected_day_col = 0;
        app.handle_key("Right", false, false);
        assert_eq!(app.selected_day_col, 0);
    }

    #[test]
    fn test_key_space_toggles_check_in() {
        let mut app = HabitTrackerApp::new();
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
        let mut app = HabitTrackerApp::new();
        app.selected_habit = 0;
        let idx = app.active_habits()[0];
        app.handle_key("a", false, false);
        assert!(app.habits[idx].archived);
    }

    #[test]
    fn test_key_advance_date() {
        let mut app = HabitTrackerApp::new();
        let d = app.today;
        app.handle_key("+", false, false);
        assert_eq!(app.today.days_since(d), 1);
    }

    #[test]
    fn test_key_go_back_date() {
        let mut app = HabitTrackerApp::new();
        let d = app.today;
        app.handle_key("-", false, false);
        assert_eq!(d.days_since(app.today), 1);
    }

    #[test]
    fn test_key_cycle_filter() {
        let mut app = HabitTrackerApp::new();
        assert!(app.category_filter.is_none());
        app.handle_key("c", false, false);
        assert_eq!(app.category_filter, Some(Category::Health));
        app.handle_key("c", false, false);
        assert_eq!(app.category_filter, Some(Category::Fitness));
    }

    #[test]
    fn test_key_cycle_filter_wraps() {
        let mut app = HabitTrackerApp::new();
        app.category_filter = Some(*Category::ALL.last().unwrap());
        app.handle_key("c", false, false);
        assert!(app.category_filter.is_none());
    }

    #[test]
    fn test_key_page_down() {
        let mut app = HabitTrackerApp::new();
        app.handle_key("PageDown", false, false);
        assert!(app.scroll_offset > 0.0);
    }

    #[test]
    fn test_key_page_up_at_zero() {
        let mut app = HabitTrackerApp::new();
        app.handle_key("PageUp", false, false);
        assert_eq!(app.scroll_offset, 0.0);
    }

    // ── Create form key tests ───────────────────────────────────────

    #[test]
    fn test_create_form_escape() {
        let mut app = HabitTrackerApp::new();
        app.show_create_form = true;
        app.handle_key("Escape", false, false);
        assert!(!app.show_create_form);
    }

    #[test]
    fn test_create_form_typing() {
        let mut app = HabitTrackerApp::new();
        app.show_create_form = true;
        app.handle_key("H", false, false);
        app.handle_key("i", false, false);
        assert_eq!(app.create_name, "Hi");
    }

    #[test]
    fn test_create_form_backspace() {
        let mut app = HabitTrackerApp::new();
        app.show_create_form = true;
        app.create_name = String::from("Hello");
        app.handle_key("Backspace", false, false);
        assert_eq!(app.create_name, "Hell");
    }

    #[test]
    fn test_create_form_tab_cycles_category() {
        let mut app = HabitTrackerApp::new();
        app.show_create_form = true;
        assert_eq!(app.create_category_idx, 0);
        app.handle_key("Tab", false, false);
        assert_eq!(app.create_category_idx, 1);
    }

    #[test]
    fn test_create_form_f1_toggles_frequency() {
        let mut app = HabitTrackerApp::new();
        app.show_create_form = true;
        assert!(app.create_frequency_daily);
        app.handle_key("F1", false, false);
        assert!(!app.create_frequency_daily);
        app.handle_key("F1", false, false);
        assert!(app.create_frequency_daily);
    }

    #[test]
    fn test_create_form_f2_cycles_weekly_count() {
        let mut app = HabitTrackerApp::new();
        app.show_create_form = true;
        app.create_weekly_count = 3;
        app.handle_key("F2", false, false);
        assert_eq!(app.create_weekly_count, 4);
    }

    #[test]
    fn test_create_form_enter_creates() {
        let mut app = HabitTrackerApp::new();
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
        let app = HabitTrackerApp::new();
        let (done, total) = app.overall_completion_today();
        assert_eq!(total, 5);
        // done depends on sample data, just check it's in range
        assert!(done <= total);
    }

    #[test]
    fn test_best_habit_streak() {
        let app = HabitTrackerApp::new();
        let (name, val) = app.best_habit_streak();
        assert!(!name.is_empty());
        assert!(val > 0);
    }

    #[test]
    fn test_average_completion_7d() {
        let app = HabitTrackerApp::new();
        let avg = app.average_completion_7d();
        assert!(avg >= 0.0 && avg <= 1.0);
    }

    #[test]
    fn test_average_completion_30d() {
        let app = HabitTrackerApp::new();
        let avg = app.average_completion_30d();
        assert!(avg >= 0.0 && avg <= 1.0);
    }

    #[test]
    fn test_average_completion_no_habits() {
        let mut app = HabitTrackerApp::new();
        app.habits.clear();
        assert_eq!(app.average_completion_7d(), 0.0);
        assert_eq!(app.average_completion_30d(), 0.0);
    }

    // ── Heatmap tests ───────────────────────────────────────────────

    #[test]
    fn test_heatmap_data_length() {
        let app = HabitTrackerApp::new();
        let data = app.heatmap_data(0, 90);
        assert_eq!(data.len(), 90);
    }

    #[test]
    fn test_heatmap_data_full_year() {
        let app = HabitTrackerApp::new();
        let data = app.heatmap_data(0, 364);
        assert_eq!(data.len(), 364);
    }

    #[test]
    fn test_heatmap_data_invalid_index() {
        let app = HabitTrackerApp::new();
        let data = app.heatmap_data(999, 30);
        assert!(data.is_empty());
    }

    #[test]
    fn test_heatmap_color_checked() {
        let c = HabitTrackerApp::heatmap_color(true, 1.0);
        // Should have non-zero alpha
        assert_ne!(c, SURFACE0);
    }

    #[test]
    fn test_heatmap_color_unchecked() {
        let c = HabitTrackerApp::heatmap_color(false, 0.5);
        assert_eq!(c, SURFACE0);
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn test_render_dashboard() {
        let app = HabitTrackerApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_statistics() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::Statistics;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_archive_empty() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::Archive;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_archive_with_items() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::Archive;
        app.habits[0].archived = true;
        app.habits[1].archived = true;
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_heatmap() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::HeatMap;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_create_form() {
        let mut app = HabitTrackerApp::new();
        app.show_create_form = true;
        let cmds = app.render();
        assert!(cmds.len() > 30);
    }

    #[test]
    fn test_render_empty_dashboard() {
        let mut app = HabitTrackerApp::new();
        app.habits.clear();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_heatmap() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::HeatMap;
        app.habits.clear();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_category_filter() {
        let mut app = HabitTrackerApp::new();
        app.category_filter = Some(Category::Fitness);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_scroll() {
        let mut app = HabitTrackerApp::new();
        app.scroll_offset = 100.0;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // ── Heatmap navigation tests ────────────────────────────────────

    #[test]
    fn test_heatmap_left_right_navigation() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::HeatMap;
        assert_eq!(app.heatmap_habit_idx, 0);
        app.handle_key("Right", false, false);
        assert_eq!(app.heatmap_habit_idx, 1);
        app.handle_key("Left", false, false);
        assert_eq!(app.heatmap_habit_idx, 0);
    }

    #[test]
    fn test_heatmap_left_boundary() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::HeatMap;
        app.heatmap_habit_idx = 0;
        app.handle_key("Left", false, false);
        assert_eq!(app.heatmap_habit_idx, 0);
    }

    #[test]
    fn test_heatmap_right_boundary() {
        let mut app = HabitTrackerApp::new();
        app.screen = Screen::HeatMap;
        let max = app.active_habits().len() - 1;
        app.heatmap_habit_idx = max;
        app.handle_key("Right", false, false);
        assert_eq!(app.heatmap_habit_idx, max);
    }

    // ── Restore from archive test ───────────────────────────────────

    #[test]
    fn test_restore_from_archive_via_enter() {
        let mut app = HabitTrackerApp::new();
        app.habits[0].archived = true;
        app.screen = Screen::Archive;
        app.selected_habit = 0;
        app.handle_key("Return", false, false);
        assert!(!app.habits[0].archived);
    }

    // ── Rate color test ─────────────────────────────────────────────

    #[test]
    fn test_rate_color_ranges() {
        assert_eq!(rate_color(1.0), GREEN);
        assert_eq!(rate_color(0.8), GREEN);
        assert_eq!(rate_color(0.6), YELLOW);
        assert_eq!(rate_color(0.4), PEACH);
        assert_eq!(rate_color(0.1), RED);
    }

    // ── Ctrl+D delete test ──────────────────────────────────────────

    #[test]
    fn test_ctrl_d_deletes() {
        let mut app = HabitTrackerApp::new();
        let count = app.habits.len();
        app.selected_habit = 0;
        app.handle_key("d", true, false);
        assert_eq!(app.habits.len(), count - 1);
    }

    // ── Completions in week test ────────────────────────────────────

    #[test]
    fn test_completions_in_week() {
        let base = Date { year: 2026, month: 5, day: 18 }; // Monday
        let mut h = Habit::new(1, "Test", "", Category::Health, Frequency::Weekly(3), base);
        h.check_ins.push(base);
        h.check_ins.push(base.add_days(2));
        h.check_ins.push(base.add_days(4));
        assert_eq!(h.completions_in_week(base), 3);
    }

    #[test]
    fn test_completions_in_week_empty() {
        let base = Date { year: 2026, month: 5, day: 18 };
        let h = Habit::new(1, "Test", "", Category::Health, Frequency::Weekly(3), base);
        assert_eq!(h.completions_in_week(base), 0);
    }

    // ── Edge case tests ─────────────────────────────────────────────

    #[test]
    fn test_weekly_count_clamped() {
        let mut app = HabitTrackerApp::new();
        app.create_name = String::from("Clamped");
        app.create_frequency_daily = false;
        app.create_weekly_count = 99;
        app.create_habit();
        let last = app.habits.last().unwrap();
        assert_eq!(last.frequency, Frequency::Weekly(7));
    }

    #[test]
    fn test_heatmap_data_has_correct_dates() {
        let app = HabitTrackerApp::new();
        let data = app.heatmap_data(0, 7);
        // Last entry should be today
        assert_eq!(data.last().unwrap().0, app.today);
        // First entry should be 6 days ago
        assert_eq!(data.first().unwrap().0, app.today.add_days(-6));
    }

    #[test]
    fn test_multiple_date_advances() {
        let mut app = HabitTrackerApp::new();
        let start = app.today;
        for _ in 0..10 {
            app.advance_day();
        }
        assert_eq!(app.today.days_since(start), 10);
    }

    #[test]
    fn test_next_id_increments() {
        let mut app = HabitTrackerApp::new();
        let id_before = app.next_id;
        app.create_name = String::from("A");
        app.create_habit();
        app.create_name = String::from("B");
        app.create_habit();
        assert_eq!(app.next_id, id_before + 2);
    }

    #[test]
    fn test_selected_habit_adjusts_on_delete() {
        let mut app = HabitTrackerApp::new();
        app.selected_habit = 4; // last
        app.delete_habit(4);
        assert!(app.selected_habit < app.active_habits().len());
    }
}
