//! Pomodoro Focus Timer — productivity timer for SlateOS.
//!
//! # What having a window changed
//!
//! `main` used to build a `PomodoroApp` and drop it, so none of the below was
//! reachable: the app had four screens, eight settings and a focus log, and no
//! way to see any of them. Wiring it to `oswindow` surfaced four more faults
//! that only a running clock and a pointer can expose.
//!
//! **It counted its own ticks instead of the time.** `tick()` took no argument
//! and subtracted exactly one second per call, so the countdown measured
//! *frames the loop happened to deliver*, not minutes. `Event::Tick` carries
//! the interval that actually elapsed precisely because that number is not the
//! one you asked for; a 25-minute focus block on a busy desktop ran long by
//! however much the loop was late, silently. It now advances by `elapsed_ms`
//! and carries the sub-second remainder forward.
//!
//! **It did not know what day it was.** `current_day` was the string literal
//! `"2026-05-18"`, so every session anyone would ever run landed in one bucket.
//! The daily goal was really a goal-since-launch, and the streak — the whole
//! point of which is *consecutive days* — could not exceed one. The day now
//! comes from the machine's clock and rolls over at midnight while the app is
//! open.
//!
//! **The streak counted rows, not days.** `update_streak` walked `daily_stats`
//! backwards counting `goal_met`, but a day with no pomodoros leaves no row, so
//! Monday and Wednesday looked adjacent and a skipped Tuesday was invisible.
//! It now requires each row to be the calendar day before the last.
//!
//! **Nothing was clickable and nothing fit.** Four tabs at a fixed 120px each
//! needed 496px of a 600px window and ran off anything narrower; the progress
//! ring was a constant 100px radius regardless of how much room was left; the
//! settings column was pinned at `width - 200`. Every control was keyboard-only
//! — including the eight settings rows, which could only be reached by counting
//! Down presses. Layout is now derived from the live window size on every
//! frame, the log scrolls under the wheel, and every control is a hit box.
//!
//! # Features
//!
//! - Classic Pomodoro technique: 25-min work / 5-min short break / 15-min long break
//! - Customizable durations (work, short break, long break, rounds per set)
//! - Session tracking with statistics (daily/weekly/total)
//! - Task tagging: label each pomodoro with what you're working on
//! - Auto-start next phase or pause between phases
//! - Notification on phase transition
//! - Focus log: timestamped record of completed sessions
//! - Streak tracking: consecutive days of meeting daily goal
//! - Daily goal: target number of pomodoros per day
//! - Ambient sound selection (simulated: rain, cafe, forest, white noise)
//! - Minimal distraction UI with large timer display

use guitk::color::Color;
use guitk::date::Date;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x001E_1E2E);
const CRUST: Color = Color::from_hex(0x0011_111B);
const SURFACE0: Color = Color::from_hex(0x0031_3244);
const SURFACE1: Color = Color::from_hex(0x0045_475A);
const TEXT_COLOR: Color = Color::from_hex(0x00CD_D6F4);
const SUBTEXT0: Color = Color::from_hex(0x00A6_ADC8);
const SUBTEXT1: Color = Color::from_hex(0x00BA_C2DE);
const BLUE: Color = Color::from_hex(0x0089_B4FA);
const GREEN: Color = Color::from_hex(0x00A6_E3A1);
const RED: Color = Color::from_hex(0x00F3_8BA8);
const YELLOW: Color = Color::from_hex(0x00F9_E2AF);
const PEACH: Color = Color::from_hex(0x00FA_B387);
const LAVENDER: Color = Color::from_hex(0x00B4_BEFE);
const TEAL: Color = Color::from_hex(0x0094_E2D5);
const MAUVE: Color = Color::from_hex(0x00CB_A6F7);
const OVERLAY0: Color = Color::from_hex(0x006C_7086);

// ── Window geometry ────────────────────────────────────────────────────────

const DEFAULT_WIDTH: f32 = 640.0;
const DEFAULT_HEIGHT: f32 = 540.0;

/// A quarter-second heartbeat, not a one-second one.
///
/// The countdown only ever *shows* whole seconds, so a one-second interval
/// looks like the obvious choice — but the interval is a floor rather than a
/// promise, and a tick that lands 1.2 s after the last one would make the
/// display skip a number. Asking four times a second means the seconds digit
/// changes within a quarter-second of when it should, while the arithmetic
/// stays exact either way because it works from `elapsed_ms`.
const TICK_MS: u64 = 250;

const PAD: f32 = 16.0;
const ROW_GAP: f32 = 4.0;

// ── Hit targets ────────────────────────────────────────────────────────────

/// Everything the pointer can land on.
///
/// Recorded by the renderer as it paints, so a control's ink and its hit box
/// cannot disagree about where it is — which is what stopped the tab strip
/// from being clickable in the places it had scaled away from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A tab in the top strip, indexed into [`Screen::ALL`].
    Tab(usize),
    StartPause,
    Reset,
    Skip,
    Task,
    Sound,
    /// A settings row, selecting it — indexed into the settings list.
    Setting(usize),
    /// The `‹` on a settings row: the same step `Left` takes.
    SettingLess(usize),
    /// The `›` on a settings row: the same step `Right` takes.
    SettingMore(usize),
    /// The transition banner, which dismisses on a click anywhere in it.
    Notification,
}

pub type Frame = guitk::frame::Frame<Target>;

// ── Layout ─────────────────────────────────────────────────────────────────

/// Where everything is, for one window size.
///
/// Derived on every frame and never stored on the app: a remembered layout is
/// a layout that can disagree with the window, which is how the old fixed
/// `tab_w = 120.0` survived being wrong at every size but 600px wide.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub window: Rect,
    pub tabs: Rect,
    pub tab_w: f32,
    pub content: Rect,
    pub status: Rect,
    /// The progress ring's bounding square on the timer screen.
    pub ring: Rect,
    pub time_font: f32,
    pub bar: Rect,
    pub buttons: Rect,
    pub task: Rect,
    pub notification: Option<Rect>,
}

impl Layout {
    pub fn new(width: f32, height: f32, notification: bool) -> Self {
        let window = Rect::new(0.0, 0.0, width.max(1.0), height.max(1.0));

        let tab_h = (window.h * 0.09).clamp(24.0, 36.0);
        let status_h = (window.h * 0.07).clamp(20.0, 28.0);
        let tabs = Rect::new(0.0, 0.0, window.w, tab_h);
        let status = Rect::new(0.0, window.bottom() - status_h, window.w, status_h);
        let content = Rect::new(
            0.0,
            tabs.bottom(),
            window.w,
            (window.h - tab_h - status_h).max(0.0),
        );

        // Four tabs share the strip. They shrink together rather than the
        // fourth one falling off the edge, which is what a fixed width did at
        // any window narrower than 500px.
        let tab_w = ((window.w - 16.0) / 4.0 - ROW_GAP).clamp(28.0, 150.0);

        // The timer screen's bands are measured from the bottom up, because
        // the buttons and the task line have a size they need and the ring is
        // whatever is left. Sizing the ring first is how it used to end up
        // larger than the window it was drawn in.
        let button_h = (content.h * 0.12).clamp(24.0, 34.0);
        let buttons = Rect::new(
            content.x + PAD,
            content.bottom() - PAD - button_h,
            (content.w - PAD * 2.0).max(0.0),
            button_h,
        );
        let task_h = (content.h * 0.06).clamp(12.0, 18.0);
        let task = Rect::new(buttons.x, buttons.y - ROW_GAP - task_h, buttons.w, task_h);
        let bar_h = 6.0;
        let bar = Rect::new(
            buttons.x + buttons.w * 0.1,
            task.y - 10.0 - bar_h,
            (buttons.w * 0.8).max(0.0),
            bar_h,
        );

        let head_h = (content.h * 0.18).clamp(30.0, 66.0);
        let ring_top = content.y + head_h;
        let ring_room = Rect::new(
            content.x,
            ring_top,
            content.w,
            (bar.y - 8.0 - ring_top).max(0.0),
        );
        let ring_d = ring_room.w.min(ring_room.h).max(0.0);
        let (rcx, rcy) = ring_room.centre();
        let ring = Rect::new(rcx - ring_d / 2.0, rcy - ring_d / 2.0, ring_d, ring_d);

        // "88:88" is five characters at roughly 0.6em apiece, and the ring's
        // inner circle is the box it has to sit inside.
        let time_font = (ring_d * 0.72 / (5.0 * 0.6)).clamp(12.0, 56.0);

        let notification = notification.then(|| {
            let nw = (window.w * 0.7).clamp(180.0, 360.0);
            let nh = (content.h * 0.2).clamp(40.0, 60.0);
            Rect::new(
                (window.w - nw) / 2.0,
                content.y + ROW_GAP,
                nw,
                nh.min(content.h),
            )
        });

        Self {
            window,
            tabs,
            tab_w,
            content,
            status,
            ring,
            time_font,
            bar,
            buttons,
            task,
            notification,
        }
    }

    /// The x of tab `i`, whatever the strip has been scaled to.
    pub fn tab_rect(&self, index: usize) -> Rect {
        let i = index as f32;
        Rect::new(
            8.0 + i * (self.tab_w + ROW_GAP),
            self.tabs.y + 4.0,
            self.tab_w,
            (self.tabs.h - 8.0).max(1.0),
        )
    }

    /// The height of one settings row, chosen so all eight fit the window.
    ///
    /// The eighth row used to be drawn at a fixed 32px stride from a fixed
    /// origin, which put it below the status bar on anything under 400px tall
    /// — reachable with `Down`, invisible once reached.
    pub fn settings_row_h(&self) -> f32 {
        let rows = SETTING_COUNT as f32;
        ((self.content.h - 60.0) / rows).clamp(18.0, 34.0)
    }

    pub fn settings_row(&self, index: usize) -> Rect {
        let h = self.settings_row_h();
        Rect::new(
            self.content.x + PAD,
            self.content.y + 52.0 + index as f32 * h,
            (self.content.w - PAD * 2.0).max(0.0),
            (h - ROW_GAP).max(1.0),
        )
    }

    /// How many log rows the window has room for — at least one, so an empty
    /// list still has somewhere to put its "no entries" line.
    pub fn log_rows(&self) -> usize {
        let room = (self.content.h - 62.0).max(0.0);
        ((room / LOG_ROW_H) as usize).max(1)
    }

    /// The four log columns, as fractions of the width rather than the
    /// constants 20/120/200/380 that used to run off a narrow window.
    pub fn log_columns(&self) -> [f32; 4] {
        let x = self.content.x + PAD;
        let w = (self.content.w - PAD * 2.0).max(1.0);
        [x, x + w * 0.24, x + w * 0.40, x + w * 0.80]
    }
}

const LOG_ROW_H: f32 = 20.0;
const SETTING_COUNT: usize = 8;

// ── Timer Phase ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Work,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Work => "Focus",
            Self::ShortBreak => "Short Break",
            Self::LongBreak => "Long Break",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Work => RED,
            Self::ShortBreak => GREEN,
            Self::LongBreak => BLUE,
        }
    }
}

// ── Timer State ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Idle,
    Running,
    Paused,
}

// ── Ambient Sound ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientSound {
    None,
    Rain,
    Cafe,
    Forest,
    WhiteNoise,
    Ocean,
    Fireplace,
}

impl AmbientSound {
    const ALL: [Self; 7] = [
        Self::None,
        Self::Rain,
        Self::Cafe,
        Self::Forest,
        Self::WhiteNoise,
        Self::Ocean,
        Self::Fireplace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Rain => "Rain",
            Self::Cafe => "Cafe",
            Self::Forest => "Forest",
            Self::WhiteNoise => "White Noise",
            Self::Ocean => "Ocean",
            Self::Fireplace => "Fireplace",
        }
    }

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        // `None` is the first entry, so falling off the end and falling back
        // to it *is* the wrap — no modulus, and no way to be off by one.
        Self::ALL
            .get(idx.wrapping_add(1))
            .copied()
            .unwrap_or(Self::None)
    }
}

// ── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Settings {
    pub work_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    pub rounds_per_set: u32,
    pub auto_start_breaks: bool,
    pub auto_start_work: bool,
    pub daily_goal: u32,
    pub notification_sound: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            work_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            rounds_per_set: 4,
            auto_start_breaks: false,
            auto_start_work: false,
            daily_goal: 8,
            notification_sound: true,
        }
    }
}

impl Settings {
    pub fn duration_secs(&self, phase: Phase) -> u32 {
        match phase {
            Phase::Work => self.work_minutes.saturating_mul(60),
            Phase::ShortBreak => self.short_break_minutes.saturating_mul(60),
            Phase::LongBreak => self.long_break_minutes.saturating_mul(60),
        }
    }

    /// The label and current value of every settings row, in cursor order.
    ///
    /// One table, read by the renderer *and* by the row count the cursor
    /// clamps against, so a row cannot exist for the keyboard and not for the
    /// pointer.
    pub fn rows(&self) -> [(&'static str, String); SETTING_COUNT] {
        [
            ("Work Duration", format!("{} min", self.work_minutes)),
            ("Short Break", format!("{} min", self.short_break_minutes)),
            ("Long Break", format!("{} min", self.long_break_minutes)),
            ("Rounds per Set", format!("{}", self.rounds_per_set)),
            ("Auto-start Breaks", yes_no(self.auto_start_breaks).into()),
            ("Auto-start Work", yes_no(self.auto_start_work).into()),
            ("Daily Goal", format!("{} pomodoros", self.daily_goal)),
            ("Notification Sound", on_off(self.notification_sound).into()),
        ]
    }
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "Yes" } else { "No" }
}

fn on_off(flag: bool) -> &'static str {
    if flag { "On" } else { "Off" }
}

// ── Focus Log Entry ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub phase: Phase,
    pub task_label: String,
    pub started_at_ms: u64,
    pub duration_secs: u32,
    pub completed: bool,
}

// ── Daily Statistics ───────────────────────────────────────────────────────

/// One row per calendar day the user actually worked.
///
/// The date is a [`Date`] rather than a `"YYYY-MM-DD"` string because the
/// streak has to know whether two rows are *adjacent days*, and string
/// comparison cannot answer that. Rendering derives the string; nothing
/// derives the date from the string.
#[derive(Debug, Clone)]
pub struct DayStats {
    pub date: Date,
    pub pomodoros_completed: u32,
    pub total_focus_minutes: u32,
    pub total_break_minutes: u32,
    pub goal_met: bool,
}

impl DayStats {
    pub fn date_str(&self) -> String {
        let (y, m, d) = self.date.ymd();
        format!("{y:04}-{m:02}-{d:02}")
    }
}

// ── Screens ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Timer,
    Stats,
    Log,
    Settings,
}

impl Screen {
    pub const ALL: [Self; 4] = [Self::Timer, Self::Stats, Self::Log, Self::Settings];

    pub fn label(self) -> &'static str {
        match self {
            Self::Timer => "1: Timer",
            Self::Stats => "2: Stats",
            Self::Log => "3: Log",
            Self::Settings => "4: Settings",
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

// ── Application State ──────────────────────────────────────────────────────

pub struct PomodoroApp {
    // Timer core
    pub phase: Phase,
    pub state: TimerState,
    pub remaining_secs: u32,
    /// Sub-second remainder carried between ticks.
    ///
    /// Without it, a 250 ms heartbeat that ran a little late would round its
    /// leftover away four times a second and the timer would drift long.
    pub carry_ms: u64,
    pub current_round: u32,

    pub settings: Settings,

    pub current_task: String,
    pub task_input_active: bool,

    pub ambient_sound: AmbientSound,

    pub log_entries: Vec<LogEntry>,
    pub log_scroll: usize,

    pub daily_stats: Vec<DayStats>,
    /// The day the clock says it is — recomputed on every tick, so a session
    /// running past midnight starts a new row instead of padding yesterday's.
    pub today: Date,
    pub streak_days: u32,
    pub total_pomodoros: u32,
    pub total_focus_minutes: u32,

    pub session_start_ms: u64,
    pub now_ms: u64,

    pub screen: Screen,
    pub status_message: String,
    pub width: f32,
    pub height: f32,

    pub pending_notification: Option<String>,

    pub settings_cursor: usize,

    /// Cleared by `CloseRequested`, which is what makes the window shut.
    pub running: bool,
}

impl Default for PomodoroApp {
    fn default() -> Self {
        Self::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, 0)
    }
}

impl PomodoroApp {
    /// A new timer, sized to `width` × `height`, with the clock at `now_ms`.
    ///
    /// The instant is a parameter rather than a read of `SystemTime` so the
    /// tests can put the app on a known day; `main` passes the machine's.
    pub fn new(width: f32, height: f32, now_ms: u64) -> Self {
        let settings = Settings::default();
        let remaining = settings.duration_secs(Phase::Work);
        Self {
            phase: Phase::Work,
            state: TimerState::Idle,
            remaining_secs: remaining,
            carry_ms: 0,
            current_round: 1,
            settings,
            current_task: String::new(),
            task_input_active: false,
            ambient_sound: AmbientSound::None,
            log_entries: Vec::new(),
            log_scroll: 0,
            daily_stats: Vec::new(),
            today: day_of(now_ms),
            streak_days: 0,
            total_pomodoros: 0,
            total_focus_minutes: 0,
            session_start_ms: 0,
            now_ms,
            screen: Screen::Timer,
            status_message: "Press Space to start".into(),
            width: width.max(1.0),
            height: height.max(1.0),
            pending_notification: None,
            settings_cursor: 0,
            running: true,
        }
    }

    pub fn layout(&self) -> Layout {
        Layout::new(self.width, self.height, self.pending_notification.is_some())
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self.clamp_log_scroll();
    }

    // ── Timer control ──────────────────────────────────────────────────

    pub fn start(&mut self) {
        if self.state == TimerState::Idle || self.state == TimerState::Paused {
            self.state = TimerState::Running;
            self.carry_ms = 0;
            if self.session_start_ms == 0 {
                self.session_start_ms = self.now_ms;
            }
            self.status_message = format!("{} — Running", self.phase.label());
        }
    }

    pub fn pause(&mut self) {
        if self.state == TimerState::Running {
            self.state = TimerState::Paused;
            self.status_message = format!("{} — Paused", self.phase.label());
        }
    }

    pub fn toggle(&mut self) {
        match self.state {
            TimerState::Idle | TimerState::Paused => self.start(),
            TimerState::Running => self.pause(),
        }
    }

    pub fn reset_timer(&mut self) {
        self.remaining_secs = self.settings.duration_secs(self.phase);
        self.state = TimerState::Idle;
        self.carry_ms = 0;
        self.session_start_ms = 0;
        self.status_message = "Timer reset".into();
    }

    pub fn skip_phase(&mut self) {
        self.complete_phase(false);
    }

    /// Advance by the time that actually elapsed.
    ///
    /// The old signature took nothing and subtracted one second per call, so
    /// the countdown measured *ticks delivered* rather than time passed. The
    /// interval an app asks for is a floor, not a promise — `Event::Tick`
    /// carries the real figure for exactly this reason — so a loop running a
    /// little late made every focus block run a little long, invisibly, and
    /// the error accumulated across a set.
    /// Returns whether anything the user can see moved, so an idle heartbeat
    /// that changed nothing does not cost a repaint.
    pub fn tick(&mut self, elapsed_ms: u64) -> bool {
        // The wall clock advances even while the timer is paused: it is what
        // the day rollover and the log's start times are read from, and
        // neither of those has anything to do with whether a pomodoro is
        // currently running.
        self.now_ms = self.now_ms.saturating_add(elapsed_ms);
        let rolled = self.roll_day();

        if self.state != TimerState::Running {
            return rolled;
        }

        let total = self.carry_ms.saturating_add(elapsed_ms);
        let whole = total / 1000;
        self.carry_ms = total % 1000;
        if whole == 0 {
            return rolled;
        }

        let step = u32::try_from(whole).unwrap_or(u32::MAX);
        self.remaining_secs = self.remaining_secs.saturating_sub(step);

        if self.remaining_secs == 0 {
            self.complete_phase(true);
        }
        true
    }

    /// Move `today` on if the clock has crossed midnight.
    ///
    /// A pomodoro session left open overnight used to keep filing into the
    /// day it started, because the day was a string literal set at compile
    /// time and never touched again.
    fn roll_day(&mut self) -> bool {
        let today = day_of(self.now_ms);
        if today == self.today {
            return false;
        }
        self.today = today;
        self.update_streak();
        true
    }

    /// Complete the current phase and transition.
    pub fn complete_phase(&mut self, completed: bool) {
        let phase = self.phase;
        let duration = self
            .settings
            .duration_secs(phase)
            .saturating_sub(self.remaining_secs);

        self.log_entries.push(LogEntry {
            phase,
            task_label: self.current_task.clone(),
            started_at_ms: self.session_start_ms,
            duration_secs: duration,
            completed,
        });

        if phase == Phase::Work && completed {
            self.total_pomodoros = self.total_pomodoros.saturating_add(1);
            let minutes = duration / 60;
            self.total_focus_minutes = self.total_focus_minutes.saturating_add(minutes);
            self.update_daily_stats(minutes, 0);
        } else if completed {
            let minutes = duration / 60;
            self.update_daily_stats(0, minutes);
        }

        let (next_phase, next_round) = self.next_phase();
        self.phase = next_phase;
        self.current_round = next_round;
        self.remaining_secs = self.settings.duration_secs(next_phase);
        self.session_start_ms = self.now_ms;
        self.carry_ms = 0;

        let msg = if completed {
            format!("{} complete! Next: {}", phase.label(), next_phase.label())
        } else {
            format!("Skipped {}. Next: {}", phase.label(), next_phase.label())
        };
        self.pending_notification = Some(msg.clone());
        self.status_message = msg;

        let should_auto = match next_phase {
            Phase::Work => self.settings.auto_start_work,
            Phase::ShortBreak | Phase::LongBreak => self.settings.auto_start_breaks,
        };
        self.state = if should_auto {
            TimerState::Running
        } else {
            TimerState::Idle
        };
    }

    /// Determine the next phase based on current phase and round.
    pub fn next_phase(&self) -> (Phase, u32) {
        match self.phase {
            Phase::Work => {
                if self.current_round >= self.settings.rounds_per_set {
                    (Phase::LongBreak, 1)
                } else {
                    (Phase::ShortBreak, self.current_round)
                }
            }
            Phase::ShortBreak => (Phase::Work, self.current_round.saturating_add(1)),
            Phase::LongBreak => (Phase::Work, 1),
        }
    }

    /// Update today's row, creating it if this is the day's first session.
    pub fn update_daily_stats(&mut self, focus_mins: u32, break_mins: u32) {
        let day = self.today;
        let goal = self.settings.daily_goal;

        if let Some(stats) = self.daily_stats.iter_mut().find(|s| s.date == day) {
            stats.total_focus_minutes = stats.total_focus_minutes.saturating_add(focus_mins);
            stats.total_break_minutes = stats.total_break_minutes.saturating_add(break_mins);
            if focus_mins > 0 {
                stats.pomodoros_completed = stats.pomodoros_completed.saturating_add(1);
            }
            stats.goal_met = stats.pomodoros_completed >= goal;
        } else {
            let pomodoros = u32::from(focus_mins > 0);
            self.daily_stats.push(DayStats {
                date: day,
                pomodoros_completed: pomodoros,
                total_focus_minutes: focus_mins,
                total_break_minutes: break_mins,
                goal_met: pomodoros >= goal,
            });
        }

        self.update_streak();
    }

    /// Consecutive *days* meeting the goal, counting back from today.
    ///
    /// The old version walked `daily_stats` backwards counting `goal_met`
    /// rows, which is only the same thing if the user works every single day:
    /// a day with no sessions leaves no row at all, so a Monday and a
    /// Wednesday sat next to each other in the vector and a skipped Tuesday
    /// was invisible. Each row now has to be the calendar day before the one
    /// after it, or the run stops there.
    pub fn update_streak(&mut self) {
        // A day still in progress does not break a streak. If today's goal is
        // not met *yet*, the run is counted from yesterday — otherwise every
        // streak would read zero until the day's last pomodoro landed.
        let today_met = self
            .daily_stats
            .iter()
            .any(|s| s.date == self.today && s.goal_met);
        let mut expected = if today_met {
            self.today
        } else {
            self.today.add_days(-1)
        };

        let mut streak: u32 = 0;
        for stats in self.daily_stats.iter().rev() {
            if stats.date > expected {
                // A row for today that did not meet the goal, skipped over.
                continue;
            }
            if stats.date != expected || !stats.goal_met {
                break;
            }
            streak = streak.saturating_add(1);
            expected = expected.add_days(-1);
        }
        self.streak_days = streak;
    }

    pub fn today_stats(&self) -> Option<&DayStats> {
        self.daily_stats.iter().find(|s| s.date == self.today)
    }

    pub fn today_pomodoros(&self) -> u32 {
        self.today_stats().map_or(0, |s| s.pomodoros_completed)
    }

    // ── Format helpers ─────────────────────────────────────────────────

    /// The countdown on the dial: `mm:ss`, widening past an hour.
    ///
    /// The stock intervals are all under an hour, but the settings screen
    /// does not cap them, and a 90-minute focus block used to read `90:00`.
    pub fn format_time(secs: u32) -> String {
        guitk::duration::clock(u64::from(secs))
    }

    /// Total focus time across the day's sessions, which is routinely hours.
    pub fn format_time_long(secs: u32) -> String {
        guitk::duration::units(u64::from(secs))
    }

    pub fn progress_fraction(&self) -> f32 {
        let total = self.settings.duration_secs(self.phase);
        if total == 0 {
            return 0.0;
        }
        let elapsed = total.saturating_sub(self.remaining_secs);
        (elapsed as f32) / (total as f32)
    }

    // ── Scrolling ──────────────────────────────────────────────────────

    /// The furthest the log can scroll: the point where its last row sits on
    /// the bottom line, not the point where its last row sits alone at the
    /// top. The old clamp was `len - 1`, which let the user scroll a full
    /// table into a nearly empty one.
    pub fn max_log_scroll(&self) -> usize {
        self.log_entries
            .len()
            .saturating_sub(self.layout().log_rows())
    }

    pub fn clamp_log_scroll(&mut self) {
        self.log_scroll = self.log_scroll.min(self.max_log_scroll());
    }

    pub fn scroll_log(&mut self, rows: isize) {
        let max = self.max_log_scroll();
        self.log_scroll = shift(self.log_scroll, rows, max);
    }
}

/// The civil day an instant falls on, in UTC.
///
/// UTC rather than local because nothing else in this app knows a zone, and a
/// day boundary that disagreed with the one the log's clock times are printed
/// in would be worse than one that is merely offset.
fn day_of(now_ms: u64) -> Date {
    Date::from_unix_utc(i64::try_from(now_ms / 1000).unwrap_or(0))
}

/// `hh:mm` of the day an instant falls on, or `-` for "never started".
fn hhmm(ms: u64) -> String {
    if ms == 0 {
        return "-".into();
    }
    let secs_of_day = (ms / 1000) % 86_400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    format!("{h:02}:{m:02}")
}

/// Move `current` by `rows`, staying inside `0 ..= max`.
fn shift(current: usize, rows: isize, max: usize) -> usize {
    if rows >= 0 {
        current.saturating_add(rows.unsigned_abs()).min(max)
    } else {
        current.saturating_sub(rows.unsigned_abs())
    }
}

/// One page of rows as a signed step, for the wheel and the page keys.
fn page(rows: usize) -> isize {
    isize::try_from(rows.max(1)).unwrap_or(isize::MAX)
}

// ── Input ──────────────────────────────────────────────────────────────────

impl PomodoroApp {
    /// Do what a control does, whether a click or a key reached it.
    ///
    /// One body for both routes, so the pointer cannot end up able to do
    /// something the keyboard cannot or the other way round — which is where
    /// this app started, with eight settings rows reachable only by counting
    /// `Down` presses.
    pub fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Tab(i) => {
                if let Some(screen) = Screen::from_index(i) {
                    self.screen = screen;
                }
            }
            Target::StartPause => self.toggle(),
            Target::Reset => self.reset_timer(),
            Target::Skip => self.skip_phase(),
            Target::Task => self.begin_task_input(),
            Target::Sound => self.cycle_sound(),
            Target::Setting(i) => {
                self.settings_cursor = i.min(SETTING_COUNT.saturating_sub(1));
            }
            Target::SettingLess(i) => {
                self.settings_cursor = i.min(SETTING_COUNT.saturating_sub(1));
                self.adjust_setting(false);
            }
            Target::SettingMore(i) => {
                self.settings_cursor = i.min(SETTING_COUNT.saturating_sub(1));
                self.adjust_setting(true);
            }
            Target::Notification => self.pending_notification = None,
        }
        EventResult::Consumed
    }

    fn begin_task_input(&mut self) {
        self.task_input_active = true;
        self.current_task.clear();
        self.status_message = "Type task label, Enter to confirm".into();
    }

    fn end_task_input(&mut self) {
        self.task_input_active = false;
        self.status_message = if self.current_task.is_empty() {
            "No task label set".into()
        } else {
            format!("Task: {}", self.current_task)
        };
    }

    fn cycle_sound(&mut self) {
        self.ambient_sound = self.ambient_sound.next();
        self.status_message = format!("Sound: {}", self.ambient_sound.label());
    }

    /// What the pointer would hit at `(x, y)`.
    ///
    /// Asks the renderer, rather than re-deriving the geometry: a second copy
    /// of the layout is a second thing to keep in step with the first.
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    // ── Keyboard ───────────────────────────────────────────────────────

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if self.task_input_active {
            return self.handle_task_key(key);
        }
        if self.screen == Screen::Settings {
            return self.handle_settings_key(key);
        }

        // A digit switches screens from anywhere; the tab strip is the same
        // four targets under the pointer.
        if let Some(result) = self.handle_screen_key(key) {
            return result;
        }

        let ctrl = key.modifiers.ctrl;
        match key.key {
            Key::Space => self.activate(Target::StartPause),
            Key::R if !ctrl => self.activate(Target::Reset),
            Key::S if !ctrl => self.activate(Target::Skip),
            Key::T if !ctrl => self.activate(Target::Task),
            Key::A if !ctrl => self.activate(Target::Sound),
            Key::N if !ctrl => self.activate(Target::Notification),
            Key::PageUp if self.screen == Screen::Log => {
                self.scroll_log(page(self.layout().log_rows()).saturating_neg());
                EventResult::Consumed
            }
            Key::PageDown if self.screen == Screen::Log => {
                self.scroll_log(page(self.layout().log_rows()));
                EventResult::Consumed
            }
            Key::Up if self.screen == Screen::Log => {
                self.scroll_log(-1);
                EventResult::Consumed
            }
            Key::Down if self.screen == Screen::Log => {
                self.scroll_log(1);
                EventResult::Consumed
            }
            Key::Home if self.screen == Screen::Log => {
                self.log_scroll = 0;
                EventResult::Consumed
            }
            Key::End if self.screen == Screen::Log => {
                self.log_scroll = self.max_log_scroll();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// The digits 1–4, which reach every screen from every screen.
    fn handle_screen_key(&mut self, key: &KeyEvent) -> Option<EventResult> {
        let index = match key.key {
            Key::Num1 => 0,
            Key::Num2 => 1,
            Key::Num3 => 2,
            Key::Num4 => 3,
            _ => return None,
        };
        Some(self.activate(Target::Tab(index)))
    }

    fn handle_task_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter | Key::Escape => {
                self.end_task_input();
                EventResult::Consumed
            }
            Key::Backspace => {
                self.current_task.pop();
                EventResult::Consumed
            }
            _ => {
                // The typed text, not the key name: a label is whatever the
                // keyboard produced, including characters no `Key` names.
                // `typed` has already dropped the control characters that
                // Enter, Tab and Escape produce on most layouts.
                if key.modifiers.ctrl || !key.types_text() {
                    return EventResult::Ignored;
                }
                self.current_task.extend(key.typed());
                EventResult::Consumed
            }
        }
    }

    fn handle_settings_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up => {
                self.settings_cursor = self.settings_cursor.saturating_sub(1);
                EventResult::Consumed
            }
            Key::Down => {
                let next = self.settings_cursor.saturating_add(1);
                if next < SETTING_COUNT {
                    self.settings_cursor = next;
                }
                EventResult::Consumed
            }
            Key::Left => {
                self.adjust_setting(false);
                EventResult::Consumed
            }
            Key::Right => {
                self.adjust_setting(true);
                EventResult::Consumed
            }
            _ => self.handle_screen_key(key).unwrap_or(EventResult::Ignored),
        }
    }

    pub fn adjust_setting(&mut self, increase: bool) {
        let s = &mut self.settings;
        match self.settings_cursor {
            0 => {
                s.work_minutes = if increase {
                    s.work_minutes.saturating_add(5).min(120)
                } else {
                    s.work_minutes.saturating_sub(5).max(5)
                };
            }
            1 => {
                s.short_break_minutes = if increase {
                    s.short_break_minutes.saturating_add(1).min(30)
                } else {
                    s.short_break_minutes.saturating_sub(1).max(1)
                };
            }
            2 => {
                s.long_break_minutes = if increase {
                    s.long_break_minutes.saturating_add(5).min(60)
                } else {
                    s.long_break_minutes.saturating_sub(5).max(5)
                };
            }
            3 => {
                s.rounds_per_set = if increase {
                    s.rounds_per_set.saturating_add(1).min(10)
                } else {
                    s.rounds_per_set.saturating_sub(1).max(2)
                };
            }
            4 => s.auto_start_breaks = !s.auto_start_breaks,
            5 => s.auto_start_work = !s.auto_start_work,
            6 => {
                s.daily_goal = if increase {
                    s.daily_goal.saturating_add(1).min(20)
                } else {
                    s.daily_goal.saturating_sub(1).max(1)
                };
            }
            7 => s.notification_sound = !s.notification_sound,
            _ => {}
        }

        // An idle timer shows the new duration at once. A running one keeps
        // counting the interval it started with, because changing the length
        // of a block already under way is not a thing the user asked for.
        if self.state == TimerState::Idle {
            self.remaining_secs = self.settings.duration_secs(self.phase);
        }

        // The goal moved, so which days met it may have moved with it.
        let goal = self.settings.daily_goal;
        for stats in &mut self.daily_stats {
            stats.goal_met = stats.pomodoros_completed >= goal;
        }
        self.update_streak();
    }

    // ── Pointer ────────────────────────────────────────────────────────

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => match self.target_at(mouse.x, mouse.y) {
                Some(target) => self.activate(target),
                None => EventResult::Ignored,
            },
            MouseEventKind::Scroll { dy, .. } => {
                if self.screen != Screen::Log {
                    return EventResult::Ignored;
                }
                // `wheel` answers in offset space already — one notch down is
                // a *larger* offset — so the result is added as it comes.
                // Negating it here is the trap its own docs warn about.
                let rows = wheel::rows_f(dy);
                if rows == 0.0 {
                    return EventResult::Ignored;
                }
                self.scroll_log(rows as isize);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// The one event body, shared by the window and the probe.
pub fn handle_event(state: &mut PomodoroApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) if key.pressed => state.handle_key(key),
        Event::Mouse(mouse) => state.handle_mouse(mouse),
        Event::Resize { width, height } => {
            state.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        Event::Tick { elapsed_ms } => {
            // An idle heartbeat exists only to notice midnight. When it does
            // not, saying so is what keeps a stopped timer from repainting
            // the window once a minute for nothing.
            if state.tick(*elapsed_ms) {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        Event::CloseRequested => {
            state.running = false;
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

// ── Drawing helpers ────────────────────────────────────────────────────────

fn fill(frame: &mut Frame, rect: Rect, color: Color, radius: f32) {
    frame.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

fn label(frame: &mut Frame, x: f32, y: f32, text: String, size: f32, color: Color, max_w: f32) {
    frame.push(RenderCommand::Text {
        x,
        y,
        text,
        font_size: size,
        color,
        font_weight: FontWeightHint::Regular,
        max_width: Some(max_w.max(1.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

fn bold(frame: &mut Frame, x: f32, y: f32, text: String, size: f32, color: Color, max_w: f32) {
    frame.push(RenderCommand::Text {
        x,
        y,
        text,
        font_size: size,
        color,
        font_weight: FontWeightHint::Bold,
        max_width: Some(max_w.max(1.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Text centred on `rect`, estimating the run's width at 0.55em per character.
///
/// An estimate rather than a measurement because the frame does not carry a
/// font; being a few pixels out is invisible, whereas the old fixed
/// `cx - 60.0` was visibly wrong the moment the string changed length.
fn centred(frame: &mut Frame, rect: Rect, text: &str, size: f32, color: Color, is_bold: bool) {
    let est = text.chars().count() as f32 * size * 0.55;
    let x = rect.x + (rect.w - est) / 2.0;
    let y = rect.y + (rect.h - size) / 2.0;
    let draw = if is_bold { bold } else { label };
    draw(frame, x, y, text.to_string(), size, color, rect.w);
}

// ── Rendering ──────────────────────────────────────────────────────────────

impl PomodoroApp {
    /// The whole window, and every hit box in it, for one size.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let mut frame = Frame::new(width, height);
        let layout = Layout::new(width, height, self.pending_notification.is_some());

        fill(&mut frame, layout.window, BASE, 0.0);
        self.draw_tabs(&mut frame, &layout);

        match self.screen {
            Screen::Timer => self.draw_timer(&mut frame, &layout),
            Screen::Stats => self.draw_stats(&mut frame, &layout),
            Screen::Log => self.draw_log(&mut frame, &layout),
            Screen::Settings => self.draw_settings(&mut frame, &layout),
        }

        self.draw_status_bar(&mut frame, &layout);

        // Last, so the banner's hit box beats whatever it covers: `hit_test`
        // walks backwards and the topmost thing should take the click.
        if let (Some(rect), Some(text)) = (layout.notification, self.pending_notification.as_ref())
        {
            self.draw_notification(&mut frame, rect, text);
        }

        frame
    }

    fn draw_tabs(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.tabs, CRUST, 0.0);
        for (i, screen) in Screen::ALL.iter().enumerate() {
            let rect = layout.tab_rect(i);
            let active = self.screen == *screen;
            if active {
                fill(frame, rect, SURFACE0, 6.0);
            }
            let color = if active { TEXT_COLOR } else { OVERLAY0 };
            centred(
                frame,
                rect,
                screen.label(),
                (rect.h * 0.42).clamp(9.0, 13.0),
                color,
                active,
            );
            frame.hit(Target::Tab(i), rect);
        }
    }

    fn draw_timer(&self, frame: &mut Frame, layout: &Layout) {
        let head = Rect::new(
            layout.content.x,
            layout.content.y + 6.0,
            layout.content.w,
            22.0,
        );
        centred(
            frame,
            head,
            self.phase.label(),
            18.0,
            self.phase.color(),
            true,
        );
        let round = Rect::new(head.x, head.bottom() + 2.0, head.w, 16.0);
        centred(
            frame,
            round,
            &format!(
                "Round {}/{}",
                self.current_round, self.settings.rounds_per_set
            ),
            11.0,
            SUBTEXT0,
            false,
        );

        // The ring: an outer disc with the background punched back out of it.
        if layout.ring.w > 8.0 {
            let ring_w = (layout.ring.w * 0.08).clamp(4.0, 10.0);
            fill(frame, layout.ring, SURFACE0, layout.ring.w / 2.0);
            let inner = Rect::new(
                layout.ring.x + ring_w,
                layout.ring.y + ring_w,
                (layout.ring.w - ring_w * 2.0).max(0.0),
                (layout.ring.h - ring_w * 2.0).max(0.0),
            );
            fill(frame, inner, BASE, inner.w / 2.0);

            let time = Self::format_time(self.remaining_secs);
            let time_box = Rect::new(inner.x, inner.y + inner.h * 0.28, inner.w, layout.time_font);
            centred(frame, time_box, &time, layout.time_font, TEXT_COLOR, true);

            let (state_label, state_color) = match self.state {
                TimerState::Idle => ("Ready", OVERLAY0),
                TimerState::Running => ("Running", GREEN),
                TimerState::Paused => ("Paused", YELLOW),
            };
            let state_box = Rect::new(inner.x, time_box.bottom() + 4.0, inner.w, 14.0);
            if state_box.bottom() < inner.bottom() {
                centred(frame, state_box, state_label, 11.0, state_color, false);
            }
        }

        // Progress bar.
        fill(frame, layout.bar, SURFACE0, 3.0);
        let filled = Rect::new(
            layout.bar.x,
            layout.bar.y,
            layout.bar.w * self.progress_fraction(),
            layout.bar.h,
        );
        fill(frame, filled, self.phase.color(), 3.0);

        // Task line, or the input box while a label is being typed.
        if self.task_input_active {
            fill(frame, layout.task, SURFACE1, 5.0);
            let text = format!("Task: {}_", self.current_task);
            label(
                frame,
                layout.task.x + 6.0,
                layout.task.y + 2.0,
                text,
                (layout.task.h * 0.7).clamp(9.0, 12.0),
                TEXT_COLOR,
                layout.task.w - 12.0,
            );
        } else if self.current_task.is_empty() {
            centred(
                frame,
                layout.task,
                &format!("Sound: {}", self.ambient_sound.label()),
                (layout.task.h * 0.7).clamp(9.0, 12.0),
                OVERLAY0,
                false,
            );
        } else {
            centred(
                frame,
                layout.task,
                &format!("Task: {}", self.current_task),
                (layout.task.h * 0.7).clamp(9.0, 12.0),
                LAVENDER,
                false,
            );
        }
        frame.hit(Target::Task, layout.task);

        self.draw_buttons(frame, layout);
    }

    /// The control row, as a table rather than five hand-placed rectangles.
    ///
    /// It slides before it scales: the buttons keep their natural widths
    /// while there is room, and shrink together once there is not, so the
    /// fifth one never falls off the edge the way the fourth tab used to.
    fn draw_buttons(&self, frame: &mut Frame, layout: &Layout) {
        let start_label = match self.state {
            TimerState::Running => "Pause",
            TimerState::Paused => "Resume",
            TimerState::Idle => "Start",
        };
        let buttons: [(&str, f32, Target); 5] = [
            (start_label, 78.0, Target::StartPause),
            ("Reset", 62.0, Target::Reset),
            ("Skip", 56.0, Target::Skip),
            ("Task", 56.0, Target::Task),
            ("Sound", 62.0, Target::Sound),
        ];

        let natural: f32 = buttons.iter().map(|(_, w, _)| *w).sum();
        let gaps = ROW_GAP * (buttons.len() as f32 - 1.0);
        let scale = ((layout.buttons.w - gaps) / natural).clamp(0.35, 1.0);
        let total = natural * scale + gaps;
        let mut x = layout.buttons.x + (layout.buttons.w - total).max(0.0) / 2.0;

        for (text, width, target) in buttons {
            let w = width * scale;
            let rect = Rect::new(x, layout.buttons.y, w, layout.buttons.h);
            let active = matches!(target, Target::StartPause) && self.state == TimerState::Running;
            fill(frame, rect, if active { SURFACE1 } else { SURFACE0 }, 6.0);
            centred(
                frame,
                rect,
                text,
                (rect.h * 0.38).clamp(8.0, 13.0),
                TEXT_COLOR,
                false,
            );
            frame.hit(target, rect);
            x += w + ROW_GAP;
        }
    }

    fn draw_stats(&self, frame: &mut Frame, layout: &Layout) {
        let x = layout.content.x + PAD;
        let w = (layout.content.w - PAD * 2.0).max(1.0);
        bold(
            frame,
            x,
            layout.content.y + 8.0,
            "Statistics".into(),
            16.0,
            BLUE,
            w,
        );

        let today = self.today_pomodoros();
        let goal = self.settings.daily_goal;
        let today_focus = self.today_stats().map_or(0, |s| s.total_focus_minutes);
        let today_break = self.today_stats().map_or(0, |s| s.total_break_minutes);

        let cards: [(&str, String, Color); 6] = [
            (
                "Today",
                format!("{today} / {goal}"),
                if today >= goal { GREEN } else { PEACH },
            ),
            ("Streak", format!("{} days", self.streak_days), MAUVE),
            ("Total Pomodoros", format!("{}", self.total_pomodoros), TEAL),
            (
                "Total Focus",
                Self::format_time_long(self.total_focus_minutes.saturating_mul(60)),
                BLUE,
            ),
            (
                "Focus Today",
                Self::format_time_long(today_focus.saturating_mul(60)),
                YELLOW,
            ),
            (
                "Breaks Today",
                Self::format_time_long(today_break.saturating_mul(60)),
                GREEN,
            ),
        ];

        // Two columns wherever there is room for two, one when there is not.
        let cols = if w >= 320.0 { 2usize } else { 1 };
        let card_w = (w - ROW_GAP * (cols as f32 - 1.0)) / cols as f32;
        let card_h = ((layout.content.h - 40.0) / 3.0 - ROW_GAP).clamp(28.0, 64.0);
        for (i, (name, value, color)) in cards.iter().enumerate() {
            let col = i.checked_rem(cols).unwrap_or(0);
            let row = i.checked_div(cols).unwrap_or(0);
            let rect = Rect::new(
                x + col as f32 * (card_w + ROW_GAP),
                layout.content.y + 32.0 + row as f32 * (card_h + ROW_GAP),
                card_w,
                card_h,
            );
            if rect.bottom() > layout.content.bottom() {
                break;
            }
            fill(frame, rect, SURFACE0, 8.0);
            label(
                frame,
                rect.x + 10.0,
                rect.y + 6.0,
                (*name).to_string(),
                10.0,
                SUBTEXT0,
                rect.w - 20.0,
            );
            bold(
                frame,
                rect.x + 10.0,
                rect.y + rect.h - 22.0,
                value.clone(),
                (rect.h * 0.32).clamp(11.0, 20.0),
                *color,
                rect.w - 20.0,
            );
        }
    }

    fn draw_log(&self, frame: &mut Frame, layout: &Layout) {
        let cols = layout.log_columns();
        let x = cols[0];
        let w = (layout.content.w - PAD * 2.0).max(1.0);
        bold(
            frame,
            x,
            layout.content.y + 8.0,
            format!("Focus Log ({} entries)", self.log_entries.len()),
            16.0,
            BLUE,
            w,
        );

        if self.log_entries.is_empty() {
            label(
                frame,
                x,
                layout.content.y + 40.0,
                "No log entries yet".into(),
                11.0,
                OVERLAY0,
                w,
            );
            return;
        }

        let head_y = layout.content.y + 34.0;
        for (i, name) in ["Started", "Phase", "Task", "Result"].iter().enumerate() {
            let cx = cols.get(i).copied().unwrap_or(x);
            bold(frame, cx, head_y, (*name).to_string(), 10.0, SUBTEXT0, w);
        }

        // Newest first, which is what a log is read as.
        let rows = layout.log_rows();
        let body = Rect::new(
            layout.content.x,
            head_y + 16.0,
            layout.content.w,
            (layout.content.bottom() - head_y - 16.0).max(0.0),
        );
        frame.clip(body);
        for (i, entry) in self
            .log_entries
            .iter()
            .rev()
            .skip(self.log_scroll)
            .take(rows)
            .enumerate()
        {
            let ey = body.y + i as f32 * LOG_ROW_H;
            let color = if entry.completed {
                TEXT_COLOR
            } else {
                OVERLAY0
            };
            let task = if entry.task_label.is_empty() {
                "-"
            } else {
                entry.task_label.as_str()
            };
            let cells = [
                hhmm(entry.started_at_ms),
                format!(
                    "{} {}",
                    entry.phase.label(),
                    Self::format_time(entry.duration_secs)
                ),
                task.to_string(),
                if entry.completed { "Done" } else { "Skip" }.to_string(),
            ];
            for (c, text) in cells.into_iter().enumerate() {
                let cx = cols.get(c).copied().unwrap_or(x);
                let next = cols.get(c.saturating_add(1)).copied().unwrap_or(x + w);
                label(frame, cx, ey, text, 10.0, color, (next - cx - 6.0).max(1.0));
            }
        }
        frame.unclip();
    }

    fn draw_settings(&self, frame: &mut Frame, layout: &Layout) {
        let x = layout.content.x + PAD;
        let w = (layout.content.w - PAD * 2.0).max(1.0);
        bold(
            frame,
            x,
            layout.content.y + 8.0,
            "Settings".into(),
            16.0,
            BLUE,
            w,
        );
        label(
            frame,
            x,
            layout.content.y + 30.0,
            "Click ‹ › or use Left/Right to adjust".into(),
            10.0,
            OVERLAY0,
            w,
        );

        for (i, (name, value)) in self.settings.rows().into_iter().enumerate() {
            let rect = layout.settings_row(i);
            if rect.bottom() > layout.content.bottom() {
                break;
            }
            let selected = i == self.settings_cursor;
            if selected {
                fill(frame, rect, SURFACE0, 6.0);
            }
            // The row's own box goes down first so the steppers drawn over it
            // win the click: `hit_test` walks backwards.
            frame.hit(Target::Setting(i), rect);

            let font = (rect.h * 0.42).clamp(9.0, 12.0);
            label(
                frame,
                rect.x + 10.0,
                rect.y + (rect.h - font) / 2.0,
                name.to_string(),
                font,
                if selected { TEXT_COLOR } else { SUBTEXT1 },
                rect.w * 0.45,
            );

            let step_w = (rect.h * 0.9).clamp(16.0, 26.0);
            let more = Rect::new(rect.right() - step_w - 4.0, rect.y, step_w, rect.h);
            let value_w = (rect.w * 0.3).clamp(40.0, 150.0);
            let less = Rect::new(more.x - value_w - step_w, rect.y, step_w, rect.h);
            let value_box = Rect::new(less.right(), rect.y, value_w, rect.h);

            centred(
                frame,
                value_box,
                &value,
                font,
                if selected { BLUE } else { SUBTEXT0 },
                selected,
            );
            for (glyph, box_rect, target) in [
                ("‹", less, Target::SettingLess(i)),
                ("›", more, Target::SettingMore(i)),
            ] {
                fill(frame, box_rect, SURFACE1, 4.0);
                centred(frame, box_rect, glyph, font, TEXT_COLOR, true);
                frame.hit(target, box_rect);
            }
        }
    }

    fn draw_status_bar(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.status, CRUST, 0.0);
        let font = (layout.status.h * 0.4).clamp(8.0, 11.0);
        label(
            frame,
            layout.status.x + 8.0,
            layout.status.y + (layout.status.h - font) / 2.0,
            self.status_message.clone(),
            font,
            SUBTEXT1,
            layout.status.w * 0.6,
        );

        let right = format!(
            "{} {}",
            self.phase.label(),
            Self::format_time(self.remaining_secs)
        );
        let est = right.chars().count() as f32 * font * 0.55;
        bold(
            frame,
            (layout.status.right() - 8.0 - est).max(layout.status.w * 0.6),
            layout.status.y + (layout.status.h - font) / 2.0,
            right,
            font,
            self.phase.color(),
            layout.status.w * 0.4,
        );
    }

    fn draw_notification(&self, frame: &mut Frame, rect: Rect, text: &str) {
        frame.push(RenderCommand::BoxShadow {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 2.0,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(10.0),
        });
        fill(frame, rect, SURFACE1, 10.0);
        bold(
            frame,
            rect.x + 14.0,
            rect.y + 10.0,
            text.to_string(),
            11.0,
            TEXT_COLOR,
            rect.w - 28.0,
        );
        label(
            frame,
            rect.x + 14.0,
            rect.bottom() - 20.0,
            "Click here or press [N] to dismiss".into(),
            9.0,
            OVERLAY0,
            rect.w - 28.0,
        );
        frame.hit(Target::Notification, rect);
    }
}

// ── Window ─────────────────────────────────────────────────────────────────

impl App for PomodoroApp {
    fn title(&self) -> String {
        "Pomodoro".into()
    }

    fn app_id(&self) -> String {
        "pomodoro".into()
    }

    fn initial_size(&self) -> (u32, u32) {
        (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32)
    }

    /// Fast while a block is running, slow while one is not, never off.
    ///
    /// A running timer needs the seconds digit to change on time, so it asks
    /// four times a second. A stopped one still has one thing that moves — the
    /// calendar day, which is what the daily goal and the streak are counted
    /// against — so it asks once a minute rather than not at all. The idle
    /// heartbeat costs nothing to repaint, because `handle_event` reports a
    /// tick that changed nothing as ignored.
    fn tick_interval(&self) -> Option<Duration> {
        Some(if self.state == TimerState::Running {
            Duration::from_millis(TICK_MS)
        } else {
            Duration::from_mins(1)
        })
    }

    fn on_event(&mut self, event: &Event) -> Response {
        let result = handle_event(self, event);
        if !self.running {
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.width = width;
        self.height = height;
        self.frame(width, height).into_tree()
    }
}

// ── Probe ──────────────────────────────────────────────────────────────────

impl Probe for PomodoroApp {
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

// ── Entry point ────────────────────────────────────────────────────────────

/// The machine's clock in milliseconds, or `None` if it is before 1970 or
/// past the end of `u64` — neither of which a caller can do anything about.
fn now_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis())
        .and_then(|ms| u64::try_from(ms).ok())
}

/// 2026-05-18 00:00:00 UTC — the day `current_day` used to be hard-coded to.
///
/// Kept only as the fallback for a machine whose clock will not answer, and
/// pinned by a test so it cannot quietly stop being the date it claims to be.
const FROZEN_DAY_MS: u64 = 1_779_062_400_000;

fn main() -> ExitCode {
    // The fallback is the day this app used to be frozen on, 2026-05-18,
    // which is now reached only by a machine with no clock at all.
    let start = now_ms().unwrap_or(FROZEN_DAY_MS);
    let mut app = PomodoroApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, start);
    app::launch("pomodoro", &mut app)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    const DAY_MS: u64 = 86_400_000;

    fn sample() -> PomodoroApp {
        PomodoroApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, FROZEN_DAY_MS)
    }

    fn press(app: &mut PomodoroApp, key: Key) -> EventResult {
        probe::key(app, &probe::press(key))
    }

    fn typing(app: &mut PomodoroApp, text: &str) {
        for ch in text.chars() {
            let event = KeyEvent {
                key: Key::Unknown(0),
                pressed: true,
                modifiers: Modifiers::NONE,
                text: ch.to_string(),
            };
            probe::key(app, &event);
        }
    }

    fn render(app: &PomodoroApp) -> Vec<RenderCommand> {
        app.frame(app.width, app.height).commands().to_vec()
    }

    fn drawn_text(cmds: &[RenderCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn shows(app: &PomodoroApp, needle: &str) -> bool {
        drawn_text(&render(app)).iter().any(|t| t.contains(needle))
    }

    /// Sizes the layout has to survive: the default, a big desktop window, a
    /// wide short one, a tall narrow one, and two that are barely windows.
    const SIZES: [(f32, f32); 6] = [
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        (1600.0, 1000.0),
        (960.0, 320.0),
        (380.0, 720.0),
        (420.0, 300.0),
        (320.0, 240.0),
    ];

    // ── Phase and settings ─────────────────────────────────────────────

    #[test]
    fn phase_labels_are_distinct() {
        let labels = [
            Phase::Work.label(),
            Phase::ShortBreak.label(),
            Phase::LongBreak.label(),
        ];
        assert_eq!(labels[0], "Focus");
        for (i, a) in labels.iter().enumerate() {
            for b in labels.iter().skip(i + 1) {
                assert_ne!(a, b, "two phases share a label");
            }
        }
    }

    #[test]
    fn phase_colors_are_distinct() {
        assert_ne!(Phase::Work.color(), Phase::ShortBreak.color());
        assert_ne!(Phase::Work.color(), Phase::LongBreak.color());
        assert_ne!(Phase::ShortBreak.color(), Phase::LongBreak.color());
    }

    #[test]
    fn default_settings_are_the_classic_technique() {
        let s = Settings::default();
        assert_eq!(s.work_minutes, 25);
        assert_eq!(s.short_break_minutes, 5);
        assert_eq!(s.long_break_minutes, 15);
        assert_eq!(s.rounds_per_set, 4);
        assert_eq!(s.daily_goal, 8);
    }

    #[test]
    fn durations_are_minutes_in_seconds() {
        let s = Settings::default();
        assert_eq!(s.duration_secs(Phase::Work), 1500);
        assert_eq!(s.duration_secs(Phase::ShortBreak), 300);
        assert_eq!(s.duration_secs(Phase::LongBreak), 900);
    }

    #[test]
    fn the_settings_table_has_a_row_for_every_cursor_position() {
        let app = sample();
        let rows = app.settings.rows();
        assert_eq!(
            rows.len(),
            SETTING_COUNT,
            "the renderer and the cursor clamp disagree about how many rows there are"
        );
    }

    #[test]
    fn ambient_sound_cycles_and_wraps() {
        let mut sound = AmbientSound::None;
        for expected in AmbientSound::ALL.iter().skip(1) {
            sound = sound.next();
            assert_eq!(sound, *expected);
        }
        assert_eq!(
            sound.next(),
            AmbientSound::None,
            "the last sound did not wrap to the first"
        );
    }

    // ── Timer control ──────────────────────────────────────────────────

    #[test]
    fn a_new_timer_is_idle_at_a_full_focus_block() {
        let app = sample();
        assert_eq!(app.phase, Phase::Work);
        assert_eq!(app.state, TimerState::Idle);
        assert_eq!(app.remaining_secs, 1500);
        assert_eq!(app.current_round, 1);
        assert!(app.log_entries.is_empty());
    }

    #[test]
    fn start_pause_and_resume() {
        let mut app = sample();
        app.start();
        assert_eq!(app.state, TimerState::Running);
        app.pause();
        assert_eq!(app.state, TimerState::Paused);
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn toggle_is_start_then_pause() {
        let mut app = sample();
        app.toggle();
        assert_eq!(app.state, TimerState::Running);
        app.toggle();
        assert_eq!(app.state, TimerState::Paused);
        app.toggle();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn reset_puts_the_full_block_back() {
        let mut app = sample();
        app.start();
        app.tick(60_000);
        assert_eq!(app.remaining_secs, 1440);
        app.reset_timer();
        assert_eq!(app.remaining_secs, 1500);
        assert_eq!(app.state, TimerState::Idle);
        assert_eq!(app.session_start_ms, 0);
    }

    // ── The tick, which is the whole point ─────────────────────────────

    /// The bug this app shipped with: `tick()` took no argument and dropped
    /// exactly one second per call, so it counted frames rather than time.
    #[test]
    fn the_timer_advances_by_the_time_that_passed_not_by_the_tick_count() {
        let mut app = sample();
        app.start();

        // One late tick, worth two and a half seconds.
        app.tick(2500);
        assert_eq!(
            app.remaining_secs, 1498,
            "a 2.5 s tick has to cost 2 s, not 1"
        );
        assert_eq!(app.carry_ms, 500, "the half second was thrown away");

        // The remainder is carried, not rounded off: two halves make a whole
        // second between them.
        app.tick(500);
        assert_eq!(app.remaining_secs, 1497);
        assert_eq!(app.carry_ms, 0);
    }

    #[test]
    fn a_quarter_second_of_ticks_makes_exactly_one_second() {
        let mut app = sample();
        app.start();
        for _ in 0..4 {
            app.tick(250);
        }
        assert_eq!(app.remaining_secs, 1499);
    }

    /// Forty minutes of 250 ms heartbeats has to land on the same number the
    /// arithmetic says, with no drift accumulated over 9600 of them.
    #[test]
    fn a_long_run_of_ticks_does_not_drift() {
        let mut app = sample();
        app.settings.work_minutes = 40;
        app.remaining_secs = app.settings.duration_secs(Phase::Work);
        app.start();
        for _ in 0..2400 {
            app.tick(250);
        }
        assert_eq!(
            app.remaining_secs,
            2400 - 600,
            "600 seconds of ticks did not cost 600 seconds"
        );
    }

    #[test]
    fn a_tick_while_paused_or_idle_leaves_the_countdown_alone() {
        for state in [TimerState::Idle, TimerState::Paused] {
            let mut app = sample();
            app.state = state;
            app.tick(5000);
            assert_eq!(app.remaining_secs, 1500, "{state:?} counted down");
        }
    }

    #[test]
    fn a_tick_that_changes_nothing_reports_so() {
        let mut app = sample();
        assert!(!app.tick(1000), "an idle tick claimed the window moved");
        app.start();
        assert!(app.tick(1000), "a running tick claimed nothing moved");
    }

    #[test]
    fn reaching_zero_completes_the_phase() {
        let mut app = sample();
        app.start();
        app.remaining_secs = 2;
        app.tick(2000);
        assert_eq!(app.phase, Phase::ShortBreak);
        assert_eq!(app.log_entries.len(), 1);
        assert!(app.log_entries[0].completed);
    }

    // ── Phase transitions ──────────────────────────────────────────────

    #[test]
    fn work_goes_to_a_short_break_mid_set() {
        let mut app = sample();
        app.current_round = 2;
        app.skip_phase();
        assert_eq!(app.phase, Phase::ShortBreak);
        assert_eq!(app.current_round, 2);
    }

    #[test]
    fn a_short_break_goes_back_to_work_and_counts_a_round() {
        let mut app = sample();
        app.phase = Phase::ShortBreak;
        app.current_round = 2;
        app.skip_phase();
        assert_eq!(app.phase, Phase::Work);
        assert_eq!(app.current_round, 3);
    }

    #[test]
    fn the_last_round_of_a_set_earns_a_long_break() {
        let mut app = sample();
        app.current_round = 4;
        app.skip_phase();
        assert_eq!(app.phase, Phase::LongBreak);
        assert_eq!(app.current_round, 1);
    }

    #[test]
    fn a_long_break_starts_the_next_set() {
        let mut app = sample();
        app.phase = Phase::LongBreak;
        app.skip_phase();
        assert_eq!(app.phase, Phase::Work);
        assert_eq!(app.current_round, 1);
    }

    #[test]
    fn a_skipped_phase_is_logged_as_skipped() {
        let mut app = sample();
        app.skip_phase();
        assert_eq!(app.log_entries.len(), 1);
        assert!(!app.log_entries[0].completed);
        assert_eq!(app.total_pomodoros, 0, "a skip is not a pomodoro");
    }

    #[test]
    fn auto_start_carries_straight_on() {
        let mut app = sample();
        app.settings.auto_start_breaks = true;
        app.start();
        app.remaining_secs = 1;
        app.tick(1000);
        assert_eq!(app.phase, Phase::ShortBreak);
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn without_auto_start_the_next_phase_waits() {
        let mut app = sample();
        app.start();
        app.remaining_secs = 1;
        app.tick(1000);
        assert_eq!(app.state, TimerState::Idle);
    }

    #[test]
    fn a_full_set_ends_on_a_long_break() {
        let mut app = sample();
        app.settings.auto_start_breaks = true;
        app.settings.auto_start_work = true;
        app.start();

        // Four work blocks and the three short breaks between them.
        for _ in 0..7 {
            app.remaining_secs = 1;
            app.tick(1000);
        }
        assert_eq!(app.phase, Phase::LongBreak);
        assert_eq!(app.total_pomodoros, 4);
    }

    // ── The calendar, which used to be a string literal ────────────────

    #[test]
    fn the_frozen_fallback_day_is_the_day_it_claims_to_be() {
        let (y, m, d) = day_of(FROZEN_DAY_MS).ymd();
        assert_eq!(
            (y, m, d),
            (2026, 5, 18),
            "the fallback constant has drifted off the date its comment names"
        );
    }

    #[test]
    fn the_day_comes_from_the_clock_rather_than_a_literal() {
        let a = PomodoroApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, FROZEN_DAY_MS);
        let b = PomodoroApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, FROZEN_DAY_MS + DAY_MS * 40);
        assert_ne!(
            a.today, b.today,
            "two apps started forty days apart agree about what day it is"
        );
    }

    /// A session left open past midnight files into the new day.
    #[test]
    fn the_day_rolls_over_while_the_app_is_open() {
        let mut app = sample();
        let first = app.today;
        // Start at midnight, run for a day and a minute.
        app.tick(DAY_MS + 60_000);
        assert_eq!(
            app.today,
            first.add_days(1),
            "the app is still filing into yesterday"
        );
    }

    #[test]
    fn a_rollover_is_worth_a_repaint_even_when_the_timer_is_stopped() {
        let mut app = sample();
        assert_eq!(app.state, TimerState::Idle);
        assert!(
            app.tick(DAY_MS),
            "midnight passed and the window was not told"
        );
    }

    #[test]
    fn todays_pomodoros_are_counted_against_today() {
        let mut app = sample();
        app.update_daily_stats(25, 0);
        assert_eq!(app.today_pomodoros(), 1);

        // Tomorrow starts from zero rather than inheriting the count.
        app.tick(DAY_MS);
        assert_eq!(
            app.today_pomodoros(),
            0,
            "yesterday's work is still counting towards today's goal"
        );
        assert_eq!(app.total_pomodoros, 0, "the running total is separate");
    }

    #[test]
    fn a_days_stats_accumulate_within_the_day() {
        let mut app = sample();
        app.update_daily_stats(25, 0);
        app.update_daily_stats(25, 0);
        let today = app.today_stats().expect("no row for today");
        assert_eq!(today.pomodoros_completed, 2);
        assert_eq!(today.total_focus_minutes, 50);
        assert_eq!(app.daily_stats.len(), 1, "one day, two rows");
    }

    #[test]
    fn the_goal_is_met_at_the_goal() {
        let mut app = sample();
        app.settings.daily_goal = 3;
        for _ in 0..2 {
            app.update_daily_stats(25, 0);
        }
        assert!(!app.today_stats().expect("no row").goal_met);
        app.update_daily_stats(25, 0);
        assert!(app.today_stats().expect("no row").goal_met);
    }

    #[test]
    fn moving_the_goal_moves_which_days_met_it() {
        let mut app = sample();
        app.settings.daily_goal = 8;
        for _ in 0..3 {
            app.update_daily_stats(25, 0);
        }
        assert!(!app.today_stats().expect("no row").goal_met);

        // Drop the goal to three and today qualifies retroactively — the
        // alternative is a stats screen that contradicts its own settings.
        app.screen = Screen::Settings;
        app.settings_cursor = 6;
        for _ in 0..5 {
            app.adjust_setting(false);
        }
        assert_eq!(app.settings.daily_goal, 3);
        assert!(app.today_stats().expect("no row").goal_met);
    }

    // ── The streak, which used to count rows ───────────────────────────

    /// A day off leaves no row at all, so walking the vector backwards made
    /// Monday and Wednesday look adjacent. They are not.
    #[test]
    fn a_missed_day_breaks_the_streak_even_though_it_leaves_no_row() {
        let mut app = sample();
        app.settings.daily_goal = 1;
        let today = app.today;
        app.daily_stats = vec![
            met_day(today.add_days(-4)),
            met_day(today.add_days(-3)),
            // -2 missing entirely: no session, so no row.
            met_day(today.add_days(-1)),
            met_day(today),
        ];
        app.update_streak();
        assert_eq!(
            app.streak_days, 2,
            "the gap at -2 was counted through, so a day off is invisible"
        );
    }

    #[test]
    fn an_unbroken_run_counts_every_day_of_it() {
        let mut app = sample();
        app.settings.daily_goal = 1;
        let today = app.today;
        app.daily_stats = (0..5).rev().map(|n| met_day(today.add_days(-n))).collect();
        app.update_streak();
        assert_eq!(app.streak_days, 5);
    }

    /// The day is not over yet, so failing to meet today's goal *so far* must
    /// not zero a streak the user has not actually broken.
    #[test]
    fn a_day_still_in_progress_does_not_break_the_streak() {
        let mut app = sample();
        app.settings.daily_goal = 4;
        let today = app.today;
        app.daily_stats = vec![
            met_day(today.add_days(-2)),
            met_day(today.add_days(-1)),
            DayStats {
                date: today,
                pomodoros_completed: 1,
                total_focus_minutes: 25,
                total_break_minutes: 0,
                goal_met: false,
            },
        ];
        app.update_streak();
        assert_eq!(
            app.streak_days, 2,
            "one pomodoro into today wiped out two finished days"
        );
    }

    #[test]
    fn a_missed_yesterday_ends_the_streak_at_zero() {
        let mut app = sample();
        app.settings.daily_goal = 1;
        let today = app.today;
        app.daily_stats = vec![met_day(today.add_days(-3)), met_day(today.add_days(-2))];
        app.update_streak();
        assert_eq!(app.streak_days, 0);
    }

    fn met_day(date: Date) -> DayStats {
        DayStats {
            date,
            pomodoros_completed: 8,
            total_focus_minutes: 200,
            total_break_minutes: 40,
            goal_met: true,
        }
    }

    // ── Keyboard ───────────────────────────────────────────────────────

    #[test]
    fn space_starts_and_pauses() {
        let mut app = sample();
        assert_eq!(press(&mut app, Key::Space), EventResult::Consumed);
        assert_eq!(app.state, TimerState::Running);
        press(&mut app, Key::Space);
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn r_resets_and_s_skips() {
        let mut app = sample();
        app.start();
        app.tick(10_000);
        press(&mut app, Key::R);
        assert_eq!(app.remaining_secs, 1500);

        press(&mut app, Key::S);
        assert_eq!(app.phase, Phase::ShortBreak);
    }

    #[test]
    fn a_cycles_the_ambient_sound() {
        let mut app = sample();
        press(&mut app, Key::A);
        assert_eq!(app.ambient_sound, AmbientSound::Rain);
    }

    #[test]
    fn the_digits_reach_every_screen_from_every_screen() {
        let mut app = sample();
        for (key, screen) in [
            (Key::Num2, Screen::Stats),
            (Key::Num3, Screen::Log),
            (Key::Num4, Screen::Settings),
            (Key::Num1, Screen::Timer),
        ] {
            press(&mut app, key);
            assert_eq!(app.screen, screen, "{key:?} did not switch screens");
        }

        // Including out of the settings screen, which has its own key handler
        // and used to swallow `4` and offer no way back to it.
        app.screen = Screen::Settings;
        press(&mut app, Key::Num1);
        assert_eq!(app.screen, Screen::Timer);
    }

    #[test]
    fn n_dismisses_the_notification() {
        let mut app = sample();
        app.skip_phase();
        assert!(app.pending_notification.is_some());
        press(&mut app, Key::N);
        assert!(app.pending_notification.is_none());
    }

    #[test]
    fn a_key_nothing_is_bound_to_is_ignored() {
        let mut app = sample();
        assert_eq!(press(&mut app, Key::F7), EventResult::Ignored);
        assert_eq!(app.state, TimerState::Idle);
    }

    // ── Task labels ────────────────────────────────────────────────────

    #[test]
    fn typing_a_task_label() {
        let mut app = sample();
        press(&mut app, Key::T);
        assert!(app.task_input_active);
        typing(&mut app, "write tests");
        assert_eq!(app.current_task, "write tests");

        press(&mut app, Key::Backspace);
        assert_eq!(app.current_task, "write test");

        press(&mut app, Key::Enter);
        assert!(!app.task_input_active);
        assert!(app.status_message.contains("write test"));
    }

    /// Escape and Enter *produce text* on most layouts, so a field that
    /// appends whatever arrives fills with control characters.
    #[test]
    fn the_task_field_does_not_swallow_control_characters() {
        let mut app = sample();
        press(&mut app, Key::T);
        let escape = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "\u{1b}".into(),
        };
        probe::key(&mut app, &escape);
        assert!(!app.task_input_active, "Escape did not close the field");
        assert!(
            app.current_task.is_empty(),
            "the escape character went into the label: {:?}",
            app.current_task
        );
    }

    #[test]
    fn a_task_label_rides_along_on_the_log_entry() {
        let mut app = sample();
        press(&mut app, Key::T);
        typing(&mut app, "thesis");
        press(&mut app, Key::Enter);
        app.skip_phase();
        assert_eq!(app.log_entries[0].task_label, "thesis");
    }

    // ── Settings ───────────────────────────────────────────────────────

    #[test]
    fn the_settings_cursor_stops_at_both_ends() {
        let mut app = sample();
        app.screen = Screen::Settings;
        for _ in 0..20 {
            press(&mut app, Key::Down);
        }
        assert_eq!(app.settings_cursor, SETTING_COUNT - 1);
        for _ in 0..20 {
            press(&mut app, Key::Up);
        }
        assert_eq!(app.settings_cursor, 0);
    }

    #[test]
    fn adjusting_a_duration_moves_it_and_stops_at_the_bounds() {
        let mut app = sample();
        app.screen = Screen::Settings;
        app.settings_cursor = 0;
        press(&mut app, Key::Right);
        assert_eq!(app.settings.work_minutes, 30);
        for _ in 0..50 {
            press(&mut app, Key::Right);
        }
        assert_eq!(app.settings.work_minutes, 120);
        for _ in 0..50 {
            press(&mut app, Key::Left);
        }
        assert_eq!(app.settings.work_minutes, 5);
    }

    #[test]
    fn a_toggle_setting_flips_either_way() {
        let mut app = sample();
        app.screen = Screen::Settings;
        app.settings_cursor = 4;
        assert!(!app.settings.auto_start_breaks);
        press(&mut app, Key::Right);
        assert!(app.settings.auto_start_breaks);
        press(&mut app, Key::Left);
        assert!(!app.settings.auto_start_breaks);
    }

    #[test]
    fn a_new_duration_shows_at_once_when_idle_and_waits_when_running() {
        let mut app = sample();
        app.screen = Screen::Settings;
        app.settings_cursor = 0;
        press(&mut app, Key::Right);
        assert_eq!(app.remaining_secs, 1800, "an idle timer did not take it up");

        app.state = TimerState::Running;
        press(&mut app, Key::Right);
        assert_eq!(
            app.remaining_secs, 1800,
            "the block already under way changed length mid-flight"
        );
    }

    // ── The window ─────────────────────────────────────────────────────

    #[test]
    fn the_window_declares_the_size_the_probe_draws_at() {
        let app = sample();
        let (w, h) = app.initial_size();
        assert_eq!(
            (w as f32, h as f32),
            <PomodoroApp as Probe>::SIZE,
            "the probe tests a window the app never opens"
        );
        assert_eq!(app.title(), "Pomodoro");
        assert_eq!(app.app_id(), "pomodoro");
    }

    #[test]
    fn a_running_timer_asks_for_a_faster_clock_than_a_stopped_one() {
        let mut app = sample();
        let idle = app
            .tick_interval()
            .expect("a stopped app still tracks the day");
        app.start();
        let running = app.tick_interval().expect("a running timer has no clock");
        assert!(
            running < idle,
            "the running interval {running:?} is not faster than the idle {idle:?}"
        );
        assert_eq!(running, Duration::from_millis(TICK_MS));
    }

    #[test]
    fn closing_the_window_stops_the_app() {
        let mut app = sample();
        assert!(app.running);
        let response = app.on_event(&Event::CloseRequested);
        assert!(matches!(response, Response::Exit));
    }

    #[test]
    fn a_consumed_event_repaints_and_an_ignored_one_does_not() {
        let mut app = sample();
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Space))),
            Response::Redraw
        ));
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::F7))),
            Response::Idle
        ));
    }

    #[test]
    fn an_idle_heartbeat_that_changed_nothing_costs_no_repaint() {
        let mut app = sample();
        assert!(
            matches!(
                app.on_event(&Event::Tick { elapsed_ms: 60_000 }),
                Response::Idle
            ),
            "a stopped timer repainted the window for a tick that moved nothing"
        );
    }

    #[test]
    fn render_produces_a_tree() {
        let mut app = sample();
        let tree = app.render(800.0, 600.0);
        assert!(!tree.commands.is_empty());
        assert_eq!(app.width, 800.0);
        assert_eq!(app.height, 600.0);
    }

    // ── Layout ─────────────────────────────────────────────────────────

    #[test]
    fn every_screen_draws_a_balanced_frame_at_every_reasonable_size() {
        for screen in Screen::ALL {
            for (w, h) in SIZES {
                let mut app = sample();
                app.screen = screen;
                app.skip_phase(); // gives it a log entry and a notification
                app.resize(w, h);
                let frame = app.frame(w, h);
                assert!(
                    frame.is_balanced(),
                    "{screen:?} at {w}x{h} left a clip or a translate open"
                );
                assert!(
                    !frame.commands().is_empty(),
                    "{screen:?} at {w}x{h} drew nothing"
                );
            }
        }
    }

    /// The old layout was a set of constants: `tab_w = 120.0` four times over
    /// needed 496px of a 600px window and simply ran off anything narrower.
    #[test]
    fn the_tab_strip_never_leaves_the_window() {
        for (w, h) in SIZES {
            let layout = Layout::new(w, h, false);
            let last = layout.tab_rect(Screen::ALL.len() - 1);
            assert!(
                last.right() <= w + 0.5,
                "the fourth tab ends at {} in a {w}px window",
                last.right()
            );
        }
    }

    #[test]
    fn every_tab_is_clickable_at_every_size() {
        for (w, h) in SIZES {
            for (i, screen) in Screen::ALL.iter().enumerate() {
                let mut app = sample();
                app.resize(w, h);
                let rect = probe::rect_of_sized(&app, Target::Tab(i), (w, h))
                    .unwrap_or_else(|| panic!("tab {i} is not drawn at {w}x{h}"));
                let (cx, cy) = rect.centre();
                app.click_at(cx, cy, MouseButton::Left, (w, h));
                assert_eq!(app.screen, *screen, "tab {i} at {w}x{h} went elsewhere");
            }
        }
    }

    /// The progress ring was a constant 100px radius, so in a 240px-tall
    /// window it was larger than the space it was drawn in.
    #[test]
    fn the_ring_fits_the_room_that_is_left_for_it() {
        for (w, h) in SIZES {
            let layout = Layout::new(w, h, false);
            assert!(
                layout.ring.w <= layout.content.w + 0.5 && layout.ring.h <= layout.content.h + 0.5,
                "a {}x{} ring in a {}x{} content area",
                layout.ring.w,
                layout.ring.h,
                layout.content.w,
                layout.content.h
            );
            assert!(
                layout.ring.bottom() <= layout.bar.y + 0.5,
                "the ring overlaps the progress bar at {w}x{h}"
            );
        }
    }

    /// Eight rows at a fixed 32px stride put the last one under the status
    /// bar on anything shorter than about 400px — reachable with `Down`,
    /// invisible once reached.
    #[test]
    fn every_settings_row_stays_above_the_status_bar() {
        for (w, h) in SIZES {
            let layout = Layout::new(w, h, false);
            let last = layout.settings_row(SETTING_COUNT - 1);
            assert!(
                last.bottom() <= layout.content.bottom() + 0.5,
                "row 8 ends at {} with content ending at {} ({w}x{h})",
                last.bottom(),
                layout.content.bottom()
            );
        }
    }

    #[test]
    fn the_layout_follows_the_window_instead_of_a_constant() {
        let small = Layout::new(400.0, 320.0, false);
        let large = Layout::new(1400.0, 900.0, false);
        assert!(large.tab_w > small.tab_w, "the tabs did not grow");
        assert!(large.ring.w > small.ring.w, "the ring did not grow");
        assert!(large.time_font > small.time_font, "the clock did not grow");
        assert!(
            large.log_rows() > small.log_rows(),
            "the log did not get longer"
        );
    }

    #[test]
    fn the_notification_band_only_costs_room_when_there_is_one() {
        let bare = Layout::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, false);
        let alert = Layout::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, true);
        assert!(bare.notification.is_none());
        let band = alert.notification.expect("no band drawn");
        assert!(band.w <= DEFAULT_WIDTH, "the band is wider than the window");
        assert_eq!(bare.content, alert.content, "the band moved the content");
    }

    // ── The pointer ────────────────────────────────────────────────────

    #[test]
    fn every_timer_button_does_what_it_says() {
        let mut app = sample();
        probe::click(&mut app, Target::StartPause);
        assert_eq!(app.state, TimerState::Running);
        probe::click(&mut app, Target::StartPause);
        assert_eq!(app.state, TimerState::Paused);

        app.tick(5000);
        probe::click(&mut app, Target::Reset);
        assert_eq!(app.state, TimerState::Idle);
        assert_eq!(app.remaining_secs, 1500);

        probe::click(&mut app, Target::Skip);
        assert_eq!(app.phase, Phase::ShortBreak);

        app.pending_notification = None;
        probe::click(&mut app, Target::Sound);
        assert_eq!(app.ambient_sound, AmbientSound::Rain);

        probe::click(&mut app, Target::Task);
        assert!(app.task_input_active);
    }

    #[test]
    fn the_notification_dismisses_under_a_click() {
        let mut app = sample();
        app.skip_phase();
        assert!(app.pending_notification.is_some());
        probe::click(&mut app, Target::Notification);
        assert!(app.pending_notification.is_none());
    }

    /// The banner is drawn last, so it takes the click off whatever it covers
    /// rather than the other way round.
    #[test]
    fn the_notification_wins_the_click_over_what_it_covers() {
        let mut app = sample();
        app.skip_phase();
        let band = probe::rect_of(&app, Target::Notification).expect("no banner");
        let (cx, cy) = band.centre();
        assert_eq!(
            app.target_at(cx, cy),
            Some(Target::Notification),
            "something under the banner is taking its clicks"
        );
    }

    #[test]
    fn a_settings_row_selects_and_its_steppers_step() {
        let mut app = sample();
        app.screen = Screen::Settings;

        probe::click(&mut app, Target::Setting(3));
        assert_eq!(app.settings_cursor, 3);
        assert_eq!(app.settings.rounds_per_set, 4, "selecting also adjusted");

        probe::click(&mut app, Target::SettingMore(3));
        assert_eq!(app.settings.rounds_per_set, 5);
        probe::click(&mut app, Target::SettingLess(3));
        assert_eq!(app.settings.rounds_per_set, 4);
    }

    /// The steppers sit inside the row that selects the field, and `hit_test`
    /// walks backwards, so this only holds because the row is recorded first.
    #[test]
    fn a_stepper_takes_the_click_off_the_row_it_sits_on() {
        let mut app = sample();
        app.screen = Screen::Settings;
        let row = probe::rect_of(&app, Target::Setting(0)).expect("no row");
        let more = probe::rect_of(&app, Target::SettingMore(0)).expect("no stepper");
        assert_eq!(
            row.intersect(more),
            Some(more),
            "the stepper is not inside the row it must win against"
        );
        let (cx, cy) = more.centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::SettingMore(0)));
    }

    #[test]
    fn a_click_on_the_background_is_ignored() {
        let mut app = sample();
        let before = app.state;
        assert_eq!(probe::click_background(&mut app), EventResult::Ignored);
        assert_eq!(app.state, before);
    }

    // ── The log and its wheel ──────────────────────────────────────────

    fn with_log(entries: usize) -> PomodoroApp {
        let mut app = sample();
        app.screen = Screen::Log;
        for i in 0..entries {
            app.current_task = format!("task {i}");
            app.skip_phase();
        }
        app.pending_notification = None;
        app.current_task.clear();
        app
    }

    fn scroll(dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x: DEFAULT_WIDTH / 2.0,
            y: DEFAULT_HEIGHT / 2.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    #[test]
    fn the_wheel_scrolls_the_log_and_stops_at_both_ends() {
        let mut app = with_log(40);
        assert_eq!(app.log_scroll, 0);

        handle_event(&mut app, &scroll(-1.0));
        assert!(app.log_scroll > 0, "the wheel moved nothing");

        for _ in 0..40 {
            handle_event(&mut app, &scroll(-1.0));
        }
        assert_eq!(
            app.log_scroll,
            app.max_log_scroll(),
            "the wheel ran past the end of the log"
        );

        for _ in 0..80 {
            handle_event(&mut app, &scroll(1.0));
        }
        assert_eq!(app.log_scroll, 0, "the wheel ran past the top");
    }

    /// The old clamp was `len - 1`, which let a full table be scrolled into a
    /// nearly empty one with a single row stranded at the top.
    #[test]
    fn the_log_stops_where_its_last_row_reaches_the_bottom_line() {
        let app = with_log(40);
        let rows = app.layout().log_rows();
        assert_eq!(app.max_log_scroll(), 40 - rows);
        assert!(rows > 1, "the test window has room for only one row");
    }

    #[test]
    fn a_log_shorter_than_the_window_does_not_scroll_at_all() {
        let mut app = with_log(2);
        assert_eq!(app.max_log_scroll(), 0);
        handle_event(&mut app, &scroll(-1.0));
        assert_eq!(app.log_scroll, 0);
    }

    #[test]
    fn growing_the_window_gives_back_the_scroll_it_no_longer_needs() {
        let mut app = with_log(40);
        app.log_scroll = app.max_log_scroll();
        let tight = app.log_scroll;

        app.resize(1200.0, 1200.0);
        assert!(
            app.log_scroll < tight,
            "a taller window kept a scroll offset it has no rows for"
        );
        assert_eq!(app.log_scroll, app.max_log_scroll());
    }

    #[test]
    fn the_page_keys_move_a_windowful() {
        let mut app = with_log(60);
        let rows = app.layout().log_rows();
        press(&mut app, Key::PageDown);
        assert_eq!(app.log_scroll, rows);
        press(&mut app, Key::PageUp);
        assert_eq!(app.log_scroll, 0);
        press(&mut app, Key::End);
        assert_eq!(app.log_scroll, app.max_log_scroll());
        press(&mut app, Key::Home);
        assert_eq!(app.log_scroll, 0);
    }

    #[test]
    fn the_wheel_does_nothing_on_a_screen_with_no_list() {
        let mut app = sample();
        app.screen = Screen::Timer;
        assert_eq!(handle_event(&mut app, &scroll(-1.0)), EventResult::Ignored);
    }

    #[test]
    fn a_resize_event_is_what_moves_the_layout() {
        let mut app = sample();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1024,
                height: 768,
            },
        );
        assert_eq!((app.width, app.height), (1024.0, 768.0));
    }

    // ── What the screens actually say ──────────────────────────────────

    #[test]
    fn the_timer_screen_shows_the_countdown_and_the_round() {
        let app = sample();
        assert!(shows(&app, "25:00"), "no countdown");
        assert!(shows(&app, "Focus"), "no phase name");
        assert!(shows(&app, "Round 1/4"), "no round");
        assert!(shows(&app, "Ready"), "no state");
        assert!(shows(&app, "Start"), "no start button");
    }

    #[test]
    fn a_running_timer_says_so_on_the_button_and_the_dial() {
        let mut app = sample();
        app.start();
        assert!(shows(&app, "Running"));
        assert!(shows(&app, "Pause"), "the button still offers to start");
    }

    #[test]
    fn the_stats_screen_shows_the_streak_and_the_goal() {
        let mut app = sample();
        app.screen = Screen::Stats;
        app.settings.daily_goal = 6;
        app.update_daily_stats(25, 0);
        assert!(shows(&app, "1 / 6"), "no goal progress");
        assert!(shows(&app, "Streak"), "no streak card");
    }

    #[test]
    fn an_empty_log_says_it_is_empty() {
        let mut app = sample();
        app.screen = Screen::Log;
        assert!(shows(&app, "No log entries yet"));
    }

    #[test]
    fn a_log_entry_shows_its_task_and_its_result() {
        let mut app = with_log(1);
        app.log_entries[0].task_label = "thesis".into();
        assert!(shows(&app, "thesis"), "no task label");
        assert!(shows(&app, "Skip"), "no result");
        assert!(shows(&app, "Focus Log (1 entries)"));
    }

    #[test]
    fn the_settings_screen_shows_every_row_it_has() {
        let mut app = sample();
        app.screen = Screen::Settings;
        app.resize(900.0, 800.0);
        for (name, _) in app.settings.rows() {
            assert!(shows(&app, name), "settings row {name:?} is not drawn");
        }
    }

    #[test]
    fn a_never_started_log_entry_shows_a_dash_rather_than_the_epoch() {
        assert_eq!(hhmm(0), "-");
        assert_eq!(hhmm(FROZEN_DAY_MS), "00:00");
        assert_eq!(hhmm(FROZEN_DAY_MS + 3_600_000 * 9 + 60_000 * 30), "09:30");
    }

    #[test]
    fn the_countdown_widens_past_an_hour() {
        assert_eq!(PomodoroApp::format_time(1500), "25:00");
        assert_eq!(PomodoroApp::format_time(0), "00:00");
        assert_eq!(PomodoroApp::format_time(5400), "01:30:00");
    }

    #[test]
    fn progress_runs_from_nothing_to_all_of_it() {
        let mut app = sample();
        assert_eq!(app.progress_fraction(), 0.0);
        app.remaining_secs = 750;
        assert_eq!(app.progress_fraction(), 0.5);
        app.remaining_secs = 0;
        assert_eq!(app.progress_fraction(), 1.0);
    }
}
