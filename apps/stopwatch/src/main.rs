//! Slate OS Stopwatch & Lap Timer
//!
//! A precision stopwatch with lap timing, split times, countdown timer mode,
//! and session history.
//!
//! # It now has a window, and the window is what the layout follows
//!
//! Until this was wired, `fn main` built a [`StopwatchApp`], dropped it and
//! exited: the readout, the lap table and the countdown setup existed only for
//! the tests to call. Everything below the [`App`] impl is what turns that into
//! a program.
//!
//! Three consequences, none of them cosmetic:
//!
//! * **Every control is clickable.** Start, Lap, Reset, the mode switch and the
//!   countdown's digit steppers were keys and nothing else, advertised by a line
//!   of grey text. A stopwatch you cannot start with the pointer is not a
//!   stopwatch a pointer user has.
//! * **The lap table is as long as the window is tall.** `max_visible_laps` was
//!   the constant `8`, and the history list carried a *second*, unrelated `8`
//!   split across the scroll clamp and the renderer. Both are now one number
//!   derived from the live window height, so a tall window shows the laps it has
//!   room for and a short one does not draw rows through its own floor.
//! * **The clock runs only while the clock runs.** [`App::tick_interval`] is
//!   consulted after every event, so it answers `Some(16ms)` while running and
//!   `None` otherwise — a stopped stopwatch does not hold the desktop awake.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::process::ExitCode;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

const BASE: Color = Color::from_hex(0x001E_1E2E);
const MANTLE: Color = Color::from_hex(0x0018_1825);
const SURFACE0: Color = Color::from_hex(0x0031_3244);
const SURFACE1: Color = Color::from_hex(0x0045_475A);
const TEXT: Color = Color::from_hex(0x00CD_D6F4);
const SUBTEXT0: Color = Color::from_hex(0x00A6_ADC8);
const BLUE: Color = Color::from_hex(0x0089_B4FA);
const GREEN: Color = Color::from_hex(0x00A6_E3A1);
const RED: Color = Color::from_hex(0x00F3_8BA8);
const YELLOW: Color = Color::from_hex(0x00F9_E2AF);
const PEACH: Color = Color::from_hex(0x00FA_B387);
const LAVENDER: Color = Color::from_hex(0x00B4_BEFE);
const OVERLAY0: Color = Color::from_hex(0x006C_7086);
const TEAL: Color = Color::from_hex(0x0094_E2D5);

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// The size the window opens at.
const DEFAULT_WIDTH: f32 = 640.0;
/// The size the window opens at.
const DEFAULT_HEIGHT: f32 = 560.0;

/// How often to ask for a tick *while running*. A readout with a milliseconds
/// field is only honest at frame rate; a once-a-second clock would show three
/// digits that were always the same three digits.
const TICK_MS: u64 = 16;

const PAD: f32 = 24.0;
const HEADER_H: f32 = 40.0;
const BUTTON_H: f32 = 34.0;
const BUTTON_GAP: f32 = 8.0;
const TIME_H: f32 = 78.0;
const ALERT_H: f32 = 38.0;
const ROW_H: f32 = 28.0;
const LIST_HEAD_H: f32 = 26.0;
const STATS_H: f32 = 26.0;
const CHIP_W: f32 = 88.0;
const CHIP_H: f32 = 24.0;
const STEP_W: f32 = 26.0;

/// The widest the big readout is allowed to be drawn.
const TIME_FONT_MAX: f32 = 56.0;
/// …and the smallest, below which it stops shrinking and starts ellipsing.
const TIME_FONT_MIN: f32 = 15.0;
/// `H:MM:SS.mmm` — the widest string the readout ever holds. The font is sized
/// against this rather than against the current text, so the digits do not
/// change size the moment the hour rolls over.
const TIME_WIDEST: f32 = 11.0;

/// Everything a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Start, pause or resume — whichever the current state makes it.
    StartPause,
    Lap,
    Reset,
    ModeToggle,
    HistoryToggle,
    SetTime,
    /// Leave history or countdown setup for the main view.
    Back,
    /// Select one of the three countdown digit fields (0 = h, 1 = m, 2 = s).
    SetupField(usize),
    SetupUp(usize),
    SetupDown(usize),
    SetupConfirm,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Where everything goes, for one window size.
///
/// Rebuilt on every frame and never stored: a layout kept in a field is a
/// layout that can disagree with the window it claims to describe.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    /// Mode name on the left, run-state chip on the right.
    pub header: Rect,
    /// The toolbar of clickable controls, directly under the header.
    pub buttons: Rect,
    /// The big readout, in the main view.
    pub time: Rect,
    pub time_font: f32,
    /// The "TIME'S UP!" band — present only when there is something to say.
    pub alert: Option<Rect>,
    /// The lap table, in the main view.
    pub list: Rect,
}

impl Layout {
    /// `alert` is passed in rather than read from the app because a layout that
    /// consults application state is one more thing that can disagree with the
    /// frame it laid out.
    #[must_use]
    pub fn new(width: f32, height: f32, alert: bool) -> Self {
        let window = Rect::new(0.0, 0.0, width.max(1.0), height.max(1.0));
        let inner_w = (window.w - PAD * 2.0).max(72.0);
        let x = PAD.min((window.w - inner_w).max(0.0) * 0.5);

        let mut y = 12.0;
        let header = Rect::new(x, y, inner_w, HEADER_H);
        y += HEADER_H;
        let buttons = Rect::new(x, y, inner_w, BUTTON_H);
        y += BUTTON_H + 12.0;
        let time = Rect::new(x, y, inner_w, TIME_H);
        y += TIME_H;
        let alert = alert.then(|| {
            let band = Rect::new(x, y, inner_w.min(300.0), ALERT_H);
            y += ALERT_H + 8.0;
            band
        });
        let list = Rect::new(x, y, inner_w, (window.h - 12.0 - y).max(0.0));

        // Sized against the widest string rather than the current one, and
        // floored so a very narrow window ellipses the readout instead of
        // shrinking it into illegibility.
        let time_font = (inner_w / (TIME_WIDEST * 0.6)).clamp(TIME_FONT_MIN, TIME_FONT_MAX);

        Self {
            window,
            header,
            buttons,
            time,
            time_font,
            alert,
            list,
        }
    }

    /// The whole region below the toolbar, used by the views that do not want a
    /// big readout above their content.
    #[must_use]
    pub fn body(&self) -> Rect {
        Rect::new(
            self.time.x,
            self.time.y,
            self.time.w,
            (self.list.bottom() - self.time.y).max(0.0),
        )
    }

    /// How many table rows fit in `area`, given whether a stats footer has to
    /// fit under them.
    fn rows_in(area: Rect, stats: bool) -> usize {
        let footer = if stats { STATS_H } else { 0.0 };
        let usable = area.h - LIST_HEAD_H - footer;
        if usable < ROW_H {
            return 0;
        }
        (usable / ROW_H).floor().max(0.0) as usize
    }

    /// How many laps the main view can show at this size.
    #[must_use]
    pub fn lap_rows(&self) -> usize {
        Self::rows_in(self.list, true)
    }

    /// How many sessions the history view can show at this size.
    #[must_use]
    pub fn history_rows(&self) -> usize {
        Self::rows_in(self.body(), false)
    }

    /// Lap table column origins: number, lap time, split.
    #[must_use]
    pub fn lap_columns(&self) -> [f32; 3] {
        let w = self.list.w;
        [self.list.x, self.list.x + w * 0.30, self.list.x + w * 0.62]
    }

    /// History table column origins: mode, total, laps, best lap.
    #[must_use]
    pub fn history_columns(&self) -> [f32; 4] {
        let area = self.body();
        [
            area.x,
            area.x + area.w * 0.26,
            area.x + area.w * 0.50,
            area.x + area.w * 0.66,
        ]
    }

    /// The three countdown digit cards, laid across the body.
    #[must_use]
    pub fn setup_cards(&self) -> [Rect; 3] {
        let area = self.body();
        let gap = 12.0;
        let card_w = ((area.w - gap * 2.0) / 3.0).max(48.0);
        let card_h = area.h.clamp(48.0, 112.0);
        [0, 1, 2].map(|i| Rect::new(area.x + (card_w + gap) * i as f32, area.y, card_w, card_h))
    }
}

// ---------------------------------------------------------------------------
// Time formatting helpers
// ---------------------------------------------------------------------------

fn format_time_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let millis = ms % 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}.{millis:03}")
    } else {
        format!("{mins:02}:{secs:02}.{millis:03}")
    }
}

fn format_time_short(ms: u64) -> String {
    let total_secs = ms / 1000;
    let centis = (ms % 1000) / 10;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins:02}:{secs:02}.{centis:02}")
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Stopped,
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Stopwatch,
    Countdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    Main,
    History,
    CountdownSetup,
}

#[derive(Debug, Clone)]
pub struct Lap {
    pub number: u32,
    /// Time since start.
    pub split_ms: u64,
    /// Time since the previous lap.
    pub lap_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub mode: AppMode,
    pub total_ms: u64,
    pub lap_count: u32,
    pub best_lap_ms: Option<u64>,
    pub worst_lap_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct StopwatchApp {
    width: f32,
    height: f32,

    mode: AppMode,
    view: AppView,
    state: TimerState,
    elapsed_ms: u64,
    laps: Vec<Lap>,
    lap_scroll: usize,

    // Countdown-specific
    countdown_target_ms: u64,
    countdown_remaining_ms: u64,
    /// 0 = hours, 1 = minutes, 2 = seconds.
    countdown_setup_field: usize,
    countdown_setup_values: [u32; 3],
    countdown_finished: bool,

    // History
    history: Vec<SessionRecord>,
    history_scroll: usize,

    running: bool,
    // No `last_tick_ms`: `Event::Tick` already carries the interval since
    // this window's previous tick, so there is nothing for the app to
    // subtract.  See `tick`.
    //
    // No `max_visible_laps` either: it was a constant `8` that no window size
    // could move, and the history list carried a second unrelated `8`. Both
    // come from `Layout` now.
}

impl Default for StopwatchApp {
    fn default() -> Self {
        Self::new(DEFAULT_WIDTH, DEFAULT_HEIGHT)
    }
}

impl StopwatchApp {
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            mode: AppMode::Stopwatch,
            view: AppView::Main,
            state: TimerState::Stopped,
            elapsed_ms: 0,
            laps: Vec::new(),
            lap_scroll: 0,
            countdown_target_ms: 300_000, // 5 minutes default
            countdown_remaining_ms: 300_000,
            countdown_setup_field: 0,
            countdown_setup_values: [0, 5, 0],
            countdown_finished: false,
            history: Vec::new(),
            history_scroll: 0,
            running: true,
        }
    }

    /// The layout for the current window size and state.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(
            self.width,
            self.height,
            self.countdown_finished && self.view == AppView::Main,
        )
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        // A taller window shows more rows, which can leave a stored scroll past
        // the new end. The renderer clamps too, but a value only the renderer
        // corrects is a value the next reader gets wrong.
        self.clamp_scrolls();
    }

    fn clamp_scrolls(&mut self) {
        let layout = self.layout();
        self.lap_scroll = self
            .lap_scroll
            .min(self.laps.len().saturating_sub(layout.lap_rows()));
        self.history_scroll = self
            .history_scroll
            .min(self.history.len().saturating_sub(layout.history_rows()));
    }

    #[must_use]
    pub fn max_lap_scroll(&self) -> usize {
        self.laps.len().saturating_sub(self.layout().lap_rows())
    }

    #[must_use]
    pub fn max_history_scroll(&self) -> usize {
        self.history
            .len()
            .saturating_sub(self.layout().history_rows())
    }

    fn setup_total_ms(values: [u32; 3]) -> u64 {
        u64::from(values[0])
            .saturating_mul(3_600_000)
            .saturating_add(u64::from(values[1]).saturating_mul(60_000))
            .saturating_add(u64::from(values[2]).saturating_mul(1_000))
    }

    pub fn start(&mut self) {
        match self.state {
            TimerState::Stopped => {
                if self.mode == AppMode::Countdown {
                    let total = Self::setup_total_ms(self.countdown_setup_values);
                    if total == 0 {
                        return;
                    }
                    self.countdown_target_ms = total;
                    self.countdown_remaining_ms = total;
                    self.countdown_finished = false;
                }
                self.state = TimerState::Running;
            }
            TimerState::Paused => {
                // A countdown that already reached zero has nothing left to
                // resume. Without this it would sit in RUNNING at 00:00 with
                // the alert still up and the tick doing nothing, and only Reset
                // could get it out — a state the window makes reachable in one
                // click, where before it took a deliberate space bar.
                if self.mode == AppMode::Countdown && self.countdown_remaining_ms == 0 {
                    return;
                }
                self.state = TimerState::Running;
            }
            TimerState::Running => {}
        }
    }

    pub fn pause(&mut self) {
        if self.state == TimerState::Running {
            self.state = TimerState::Paused;
        }
    }

    pub fn stop(&mut self) {
        if self.state != TimerState::Stopped {
            // Save session
            let best = self.laps.iter().map(|l| l.lap_ms).min();
            let worst = self.laps.iter().map(|l| l.lap_ms).max();
            let total = if self.mode == AppMode::Countdown {
                self.countdown_target_ms
                    .saturating_sub(self.countdown_remaining_ms)
            } else {
                self.elapsed_ms
            };
            self.history.push(SessionRecord {
                mode: self.mode,
                total_ms: total,
                lap_count: u32::try_from(self.laps.len()).unwrap_or(u32::MAX),
                best_lap_ms: best,
                worst_lap_ms: worst,
            });
        }
        self.state = TimerState::Stopped;
        self.elapsed_ms = 0;
        self.laps.clear();
        self.lap_scroll = 0;
        self.countdown_remaining_ms = self.countdown_target_ms;
        self.countdown_finished = false;
    }

    pub fn lap(&mut self) {
        if self.state != TimerState::Running || self.mode == AppMode::Countdown {
            return;
        }
        let prev_split = self.laps.last().map_or(0, |l| l.split_ms);
        let lap_ms = self.elapsed_ms.saturating_sub(prev_split);
        let number = u32::try_from(self.laps.len().saturating_add(1)).unwrap_or(u32::MAX);
        self.laps.push(Lap {
            number,
            split_ms: self.elapsed_ms,
            lap_ms,
        });
        // Auto-scroll to latest. The window decides how many "latest" is.
        self.lap_scroll = self.max_lap_scroll();
    }

    /// Advance the clock by `delta_ms`, the interval since the previous tick.
    ///
    /// This takes an *interval*, not a timestamp, because that is what
    /// [`Event::Tick`] carries: `oswindow`'s event loop computes
    /// `now - since_last_tick_for_this_window` and puts the result in
    /// `elapsed_ms`.  It used to take an absolute `current_ms` and subtract a
    /// `last_tick_ms` field of its own, which is the same subtraction done
    /// twice -- and, once wired to the real event, would have read
    /// `16 - 16 = 0` on every tick, i.e. a stopwatch that sits at zero while
    /// claiming to run.  The tests passed throughout, because they fed it the
    /// timestamps its own convention wanted.  See known-issues.md lesson 45:
    /// the function had no caller, so nothing had ever disagreed with it.
    ///
    /// A tick that arrives while stopped or paused is dropped, which is the
    /// whole reason the app can ignore wall-clock time: there is no gap to
    /// skip over on resume, because the interval is measured per tick and the
    /// ones we discard were never added.
    pub fn tick(&mut self, delta_ms: u64) {
        if self.state != TimerState::Running {
            return;
        }

        match self.mode {
            AppMode::Stopwatch => {
                self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
            }
            AppMode::Countdown => {
                if self.countdown_remaining_ms > 0 {
                    if delta_ms >= self.countdown_remaining_ms {
                        self.countdown_remaining_ms = 0;
                        self.countdown_finished = true;
                        self.state = TimerState::Paused;
                    } else {
                        self.countdown_remaining_ms =
                            self.countdown_remaining_ms.saturating_sub(delta_ms);
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn display_time(&self) -> u64 {
        match self.mode {
            AppMode::Stopwatch => self.elapsed_ms,
            AppMode::Countdown => self.countdown_remaining_ms,
        }
    }

    #[must_use]
    pub fn best_lap(&self) -> Option<&Lap> {
        if self.laps.len() < 2 {
            return None;
        }
        self.laps.iter().min_by_key(|l| l.lap_ms)
    }

    #[must_use]
    pub fn worst_lap(&self) -> Option<&Lap> {
        if self.laps.len() < 2 {
            return None;
        }
        self.laps.iter().max_by_key(|l| l.lap_ms)
    }

    #[must_use]
    pub fn average_lap_ms(&self) -> Option<u64> {
        if self.laps.is_empty() {
            return None;
        }
        let total: u64 = self.laps.iter().map(|l| l.lap_ms).sum();
        u64::try_from(self.laps.len())
            .ok()
            .and_then(|n| total.checked_div(n))
    }

    pub fn toggle_mode(&mut self) {
        if self.state != TimerState::Stopped {
            return;
        }
        self.mode = match self.mode {
            AppMode::Stopwatch => AppMode::Countdown,
            AppMode::Countdown => AppMode::Stopwatch,
        };
    }

    pub fn open_setup(&mut self) {
        if self.mode == AppMode::Countdown && self.state == TimerState::Stopped {
            self.view = AppView::CountdownSetup;
        }
    }

    pub fn confirm_setup(&mut self) {
        self.countdown_target_ms = Self::setup_total_ms(self.countdown_setup_values);
        self.countdown_remaining_ms = self.countdown_target_ms;
        self.view = AppView::Main;
    }

    fn adjust_setup(&mut self, delta: i32) {
        let max = if self.countdown_setup_field == 0 {
            23
        } else {
            59
        };
        if let Some(v) = self
            .countdown_setup_values
            .get_mut(self.countdown_setup_field)
        {
            let next = i64::from(*v).saturating_add(i64::from(delta)).clamp(0, max);
            *v = u32::try_from(next).unwrap_or(0);
        }
    }

    pub fn scroll_laps(&mut self, rows: isize) {
        let max = self.max_lap_scroll();
        self.lap_scroll = shift(self.lap_scroll, rows, max);
    }

    pub fn scroll_history(&mut self, rows: isize) {
        let max = self.max_history_scroll();
        self.history_scroll = shift(self.history_scroll, rows, max);
    }

    /// What a click at `(x, y)` would land on, asked of the same frame the user
    /// is looking at rather than of a second copy of the geometry.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }
}

/// One PageUp/PageDown step: a screenful of rows, never zero.
fn page(rows: usize) -> isize {
    isize::try_from(rows.max(1)).unwrap_or(isize::MAX)
}

/// Move a row index by a signed number of rows, clamped to `0..=max`.
fn shift(current: usize, rows: isize, max: usize) -> usize {
    if rows >= 0 {
        current.saturating_add(rows.unsigned_abs()).min(max)
    } else {
        current.saturating_sub(rows.unsigned_abs())
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

fn fill(frame: &mut Frame, r: Rect, color: Color, radius: f32) {
    frame.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

fn stroke(frame: &mut Frame, r: Rect, color: Color, line_width: f32, radius: f32) {
    frame.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

fn rule(frame: &mut Frame, x1: f32, y: f32, x2: f32, color: Color) {
    frame.push(RenderCommand::Line {
        x1,
        y1: y,
        x2,
        y2: y,
        color,
        width: 1.0,
    });
}

fn label(
    frame: &mut Frame,
    x: f32,
    y: f32,
    text: impl Into<String>,
    font_size: f32,
    color: Color,
    font_weight: FontWeightHint,
    max_width: Option<f32>,
) {
    frame.push(RenderCommand::Text {
        x,
        y,
        text: text.into(),
        color,
        font_size,
        font_weight,
        max_width,
        // Always elided rather than clipped: a label cut mid-glyph reads as a
        // different label, where one ending in `…` reads as itself, shortened.
        overflow: TextOverflow::Ellipsis,
    });
}

/// Approximate horizontal centring. The toolkit has no text measurement at this
/// layer, so 0.55em per character is the estimate every app here uses.
fn centered_label(
    frame: &mut Frame,
    r: Rect,
    text: &str,
    font_size: f32,
    color: Color,
    font_weight: FontWeightHint,
) {
    let est = text.chars().count() as f32 * font_size * 0.55;
    let x = r.x + (r.w - est).max(0.0) * 0.5;
    let y = r.y + (r.h - font_size * 1.25).max(0.0) * 0.5;
    label(frame, x, y, text, font_size, color, font_weight, Some(r.w));
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl StopwatchApp {
    /// The toolbar for the current view and mode.
    ///
    /// Data rather than a sequence of draw calls, so the hit box and the ink
    /// cannot describe different buttons: `draw_buttons` walks this once and
    /// records both from the same rectangle.
    fn buttons(&self) -> Vec<(&'static str, f32, bool, Target)> {
        match self.view {
            AppView::History => vec![("Back", 66.0, false, Target::Back)],
            AppView::CountdownSetup => vec![
                ("Confirm", 78.0, false, Target::SetupConfirm),
                ("Cancel", 68.0, false, Target::Back),
            ],
            AppView::Main => {
                let start = match self.state {
                    TimerState::Running => "Pause",
                    TimerState::Paused => "Resume",
                    TimerState::Stopped => "Start",
                };
                let mut row = vec![(
                    start,
                    80.0,
                    self.state == TimerState::Running,
                    Target::StartPause,
                )];
                match self.mode {
                    AppMode::Stopwatch => row.push(("Lap", 58.0, false, Target::Lap)),
                    AppMode::Countdown => row.push(("Set Time", 84.0, false, Target::SetTime)),
                }
                row.push(("Reset", 66.0, false, Target::Reset));
                row.push(("Mode", 64.0, false, Target::ModeToggle));
                row.push(("History", 76.0, false, Target::HistoryToggle));
                row
            }
        }
    }

    fn draw_header(&self, frame: &mut Frame, layout: &Layout) {
        let (title, tint) = match self.view {
            AppView::Main => match self.mode {
                AppMode::Stopwatch => ("STOPWATCH", BLUE),
                AppMode::Countdown => ("COUNTDOWN", PEACH),
            },
            AppView::History => ("SESSION HISTORY", LAVENDER),
            AppView::CountdownSetup => ("SET COUNTDOWN", PEACH),
        };

        // The state chip is anchored to the right edge, and the title is given
        // whatever is left. Both matter at every width, so neither is dropped;
        // the title is the one that elides.
        let chip_w = CHIP_W.min(layout.header.w * 0.45);
        let chip = Rect::new(
            layout.header.right() - chip_w,
            layout.header.y + (HEADER_H - CHIP_H) * 0.5,
            chip_w,
            CHIP_H,
        );
        let title_w = (chip.x - layout.header.x - 12.0).max(24.0);

        label(
            frame,
            layout.header.x,
            layout.header.y + 10.0,
            title,
            18.0,
            tint,
            FontWeightHint::Bold,
            Some(title_w),
        );

        let (state_text, state_color) = match self.state {
            TimerState::Stopped => ("STOPPED", OVERLAY0),
            TimerState::Running => ("RUNNING", GREEN),
            TimerState::Paused => ("PAUSED", YELLOW),
        };
        fill(frame, chip, SURFACE0, CHIP_H * 0.5);
        centered_label(
            frame,
            chip,
            state_text,
            13.0,
            state_color,
            FontWeightHint::Bold,
        );
    }

    fn draw_buttons(&self, frame: &mut Frame, layout: &Layout) {
        let buttons = self.buttons();
        if buttons.is_empty() {
            return;
        }
        let widths: f32 = buttons.iter().map(|b| b.1).sum();
        let gaps = BUTTON_GAP * (buttons.len().saturating_sub(1)) as f32;
        // Slide is not enough on its own: the widest toolbar here is 368px and
        // the window can be narrower than that, so the run also scales. It is
        // floored, because a toolbar shrunk past legibility is no more use than
        // one drawn off the edge.
        let scale = (layout.buttons.w / (widths + gaps)).clamp(0.45, 1.0);
        let font = (13.0 * scale).max(9.0);

        let mut x = layout.buttons.x;
        for (text, base_w, active, target) in buttons {
            let w = base_w * scale;
            let r = Rect::new(x, layout.buttons.y, w, layout.buttons.h);
            fill(frame, r, if active { SURFACE1 } else { SURFACE0 }, 6.0);
            if active {
                stroke(frame, r, BLUE, 1.5, 6.0);
            }
            centered_label(
                frame,
                r,
                text,
                font,
                if active { BLUE } else { TEXT },
                FontWeightHint::Bold,
            );
            frame.hit(target, r);
            x += w + BUTTON_GAP * scale;
        }
    }

    fn draw_main(&self, frame: &mut Frame, layout: &Layout) {
        let time_color = if self.countdown_finished {
            RED
        } else if self.state == TimerState::Running {
            TEXT
        } else {
            SUBTEXT0
        };
        label(
            frame,
            layout.time.x,
            layout.time.y + (TIME_H - layout.time_font * 1.25).max(0.0) * 0.5,
            format_time_ms(self.display_time()),
            layout.time_font,
            time_color,
            FontWeightHint::Bold,
            Some(layout.time.w),
        );

        if let Some(band) = layout.alert {
            fill(frame, band, RED, 6.0);
            centered_label(frame, band, "TIME'S UP!", 20.0, BASE, FontWeightHint::Bold);
        }

        if self.mode == AppMode::Stopwatch {
            self.draw_lap_table(frame, layout);
        }
    }

    fn draw_lap_table(&self, frame: &mut Frame, layout: &Layout) {
        let area = layout.list;
        if area.h < LIST_HEAD_H {
            return;
        }
        let cols = layout.lap_columns();
        let (c0, c1, c2) = (cols[0], cols[1], cols[2]);

        if self.laps.is_empty() {
            label(
                frame,
                c0,
                area.y,
                "No laps yet.",
                14.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(area.w),
            );
            return;
        }

        for (text, x, w) in [
            ("Lap", c0, c1 - c0 - 8.0),
            ("Lap Time", c1, c2 - c1 - 8.0),
            ("Split", c2, area.right() - c2),
        ] {
            label(
                frame,
                x,
                area.y,
                text,
                12.0,
                SUBTEXT0,
                FontWeightHint::Bold,
                Some(w.max(8.0)),
            );
        }
        rule(
            frame,
            c0,
            area.y + LIST_HEAD_H - 6.0,
            area.right(),
            SURFACE1,
        );

        let rows = layout.lap_rows();
        let start = self.lap_scroll.min(self.laps.len());
        let end = self.laps.len().min(start.saturating_add(rows));
        let best = self.best_lap().map(|l| l.number);
        let worst = self.worst_lap().map(|l| l.number);

        // Clipped rather than culled by index alone: the clip is what keeps a
        // half-row at the bottom edge from painting through the window floor,
        // and it does the same for hit boxes.
        let body = Rect::new(
            area.x,
            area.y + LIST_HEAD_H,
            area.w,
            (area.h - LIST_HEAD_H - STATS_H).max(0.0),
        );
        frame.clip(body);
        for (vis_i, lap) in self
            .laps
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let ly = body.y + vis_i as f32 * ROW_H;
            let tint = if Some(lap.number) == best {
                GREEN
            } else if Some(lap.number) == worst {
                RED
            } else {
                TEXT
            };
            label(
                frame,
                c0,
                ly,
                format!("#{}", lap.number),
                15.0,
                tint,
                FontWeightHint::Regular,
                Some(c1 - c0 - 8.0),
            );
            label(
                frame,
                c1,
                ly,
                format_time_ms(lap.lap_ms),
                15.0,
                tint,
                FontWeightHint::Bold,
                Some(c2 - c1 - 8.0),
            );
            label(
                frame,
                c2,
                ly,
                format_time_ms(lap.split_ms),
                15.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some(area.right() - c2),
            );
        }
        frame.unclip();

        if let Some(avg) = self.average_lap_ms() {
            // Pinned to the floor of the list rather than trailing the last
            // row, so it does not wander up and down as laps arrive.
            let y = area.bottom() - STATS_H + 4.0;
            rule(frame, c0, y - 6.0, area.right(), SURFACE1);
            label(
                frame,
                c0,
                y,
                format!(
                    "Avg: {}  |  {} laps  |  showing {}-{}",
                    format_time_ms(avg),
                    self.laps.len(),
                    start.saturating_add(1),
                    end
                ),
                14.0,
                TEAL,
                FontWeightHint::Regular,
                Some(area.w),
            );
        }
    }

    fn draw_history(&self, frame: &mut Frame, layout: &Layout) {
        let area = layout.body();
        if self.history.is_empty() {
            label(
                frame,
                area.x,
                area.y,
                "No sessions recorded yet.",
                16.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some(area.w),
            );
            return;
        }

        let cols = layout.history_columns();
        for (text, i) in [("Mode", 0), ("Time", 1), ("Laps", 2), ("Best Lap", 3)] {
            let x = cols.get(i).copied().unwrap_or(area.x);
            let next = cols
                .get(i.saturating_add(1))
                .copied()
                .unwrap_or(area.right());
            label(
                frame,
                x,
                area.y,
                text,
                12.0,
                SUBTEXT0,
                FontWeightHint::Bold,
                Some((next - x - 8.0).max(8.0)),
            );
        }
        rule(
            frame,
            area.x,
            area.y + LIST_HEAD_H - 6.0,
            area.right(),
            SURFACE1,
        );

        let rows = layout.history_rows();
        let start = self.history_scroll.min(self.history.len());
        let end = self.history.len().min(start.saturating_add(rows));

        let body = Rect::new(
            area.x,
            area.y + LIST_HEAD_H,
            area.w,
            (area.h - LIST_HEAD_H).max(0.0),
        );
        frame.clip(body);
        for (i, rec) in self
            .history
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let ry = body.y + i as f32 * ROW_H;
            let mode_str = match rec.mode {
                AppMode::Stopwatch => "Stopwatch",
                AppMode::Countdown => "Countdown",
            };
            let cell = |n: usize| -> (f32, f32) {
                let x = cols.get(n).copied().unwrap_or(area.x);
                let next = cols
                    .get(n.saturating_add(1))
                    .copied()
                    .unwrap_or(area.right());
                (x, (next - x - 8.0).max(8.0))
            };
            let (x0, w0) = cell(0);
            let (x1, w1) = cell(1);
            let (x2, w2) = cell(2);
            let (x3, w3) = cell(3);
            label(
                frame,
                x0,
                ry,
                mode_str,
                14.0,
                TEXT,
                FontWeightHint::Regular,
                Some(w0),
            );
            label(
                frame,
                x1,
                ry,
                format_time_short(rec.total_ms),
                14.0,
                GREEN,
                FontWeightHint::Bold,
                Some(w1),
            );
            label(
                frame,
                x2,
                ry,
                rec.lap_count.to_string(),
                14.0,
                PEACH,
                FontWeightHint::Regular,
                Some(w2),
            );
            let best = rec
                .best_lap_ms
                .map_or_else(|| String::from("-"), format_time_short);
            label(
                frame,
                x3,
                ry,
                best,
                14.0,
                TEAL,
                FontWeightHint::Regular,
                Some(w3),
            );
        }
        frame.unclip();
    }

    fn draw_setup(&self, frame: &mut Frame, layout: &Layout) {
        let labels = ["Hours", "Minutes", "Seconds"];
        let cards = layout.setup_cards();

        for (i, card) in cards.iter().enumerate() {
            let active = i == self.countdown_setup_field;
            fill(frame, *card, if active { SURFACE0 } else { MANTLE }, 8.0);
            if active {
                stroke(frame, *card, BLUE, 2.0, 8.0);
            }
            // Recorded before the steppers, so a click on ▲ is a step and not a
            // selection: `hit_test` walks backwards and the later box wins.
            frame.hit(Target::SetupField(i), *card);

            label(
                frame,
                card.x + 10.0,
                card.y + 8.0,
                labels.get(i).copied().unwrap_or(""),
                12.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some((card.w - 20.0).max(8.0)),
            );

            let value = self.countdown_setup_values.get(i).copied().unwrap_or(0);
            let value_font = ((card.w - STEP_W - 20.0) / 1.4).clamp(14.0, 38.0);
            label(
                frame,
                card.x + 10.0,
                card.y + card.h - value_font * 1.4,
                format!("{value:02}"),
                value_font,
                if active { BLUE } else { TEXT },
                FontWeightHint::Bold,
                Some((card.w - STEP_W - 16.0).max(8.0)),
            );

            let step_h = (card.h * 0.5 - 6.0).max(14.0);
            let up = Rect::new(card.right() - STEP_W - 6.0, card.y + 6.0, STEP_W, step_h);
            let down = Rect::new(
                card.right() - STEP_W - 6.0,
                up.bottom() + 4.0,
                STEP_W,
                step_h,
            );
            for (r, glyph, target) in [
                (up, "\u{25B2}", Target::SetupUp(i)),
                (down, "\u{25BC}", Target::SetupDown(i)),
            ] {
                fill(frame, r, SURFACE1, 4.0);
                centered_label(frame, r, glyph, 11.0, TEXT, FontWeightHint::Bold);
                frame.hit(target, r);
            }
        }

        let total = Self::setup_total_ms(self.countdown_setup_values);
        let y = cards.first().map_or(layout.body().y, |c| c.bottom() + 14.0);
        label(
            frame,
            layout.body().x,
            y,
            format!("Total: {}", format_time_ms(total)),
            18.0,
            if total == 0 { RED } else { TEAL },
            FontWeightHint::Regular,
            Some(layout.body().w),
        );
        if total == 0 {
            label(
                frame,
                layout.body().x,
                y + 26.0,
                "A countdown of zero will not start.",
                13.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(layout.body().w),
            );
        }
    }

    /// The whole window, drawn once, recording where every control ended up.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let mut frame = Frame::new(width, height);
        let layout = Layout::new(
            width,
            height,
            self.countdown_finished && self.view == AppView::Main,
        );

        fill(&mut frame, layout.window, BASE, 0.0);
        self.draw_header(&mut frame, &layout);
        self.draw_buttons(&mut frame, &layout);
        match self.view {
            AppView::Main => self.draw_main(&mut frame, &layout),
            AppView::History => self.draw_history(&mut frame, &layout),
            AppView::CountdownSetup => self.draw_setup(&mut frame, &layout),
        }
        frame
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

impl StopwatchApp {
    /// Leave the setup view without applying the digits.
    ///
    /// The digits are put back to whatever the timer is actually set to, so a
    /// cancelled edit does not leave the fields claiming a duration the
    /// countdown will not run.
    pub fn cancel_setup(&mut self) {
        let secs = self.countdown_target_ms / 1000;
        self.countdown_setup_values = [
            u32::try_from(secs / 3600).unwrap_or(0),
            u32::try_from((secs % 3600) / 60).unwrap_or(0),
            u32::try_from(secs % 60).unwrap_or(0),
        ];
        self.view = AppView::Main;
    }

    /// Do whatever the control named by `target` does.
    ///
    /// One body for the pointer and, where the key means the same thing, for
    /// the keyboard: a button that does something the key does not is a button
    /// whose behaviour nothing tests twice.
    pub fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::StartPause => {
                if self.state == TimerState::Running {
                    self.pause();
                } else {
                    self.start();
                }
            }
            Target::Lap => self.lap(),
            Target::Reset => self.stop(),
            Target::ModeToggle => self.toggle_mode(),
            Target::HistoryToggle => self.view = AppView::History,
            Target::SetTime => self.open_setup(),
            Target::Back => match self.view {
                AppView::CountdownSetup => self.cancel_setup(),
                _ => self.view = AppView::Main,
            },
            Target::SetupField(i) => self.countdown_setup_field = i.min(2),
            Target::SetupUp(i) => {
                self.countdown_setup_field = i.min(2);
                self.adjust_setup(1);
            }
            Target::SetupDown(i) => {
                self.countdown_setup_field = i.min(2);
                self.adjust_setup(-1);
            }
            Target::SetupConfirm => self.confirm_setup(),
        }
        EventResult::Consumed
    }
}

fn handle_main_key(state: &mut StopwatchApp, event: &KeyEvent) -> EventResult {
    match event.key {
        Key::Space => state.activate(Target::StartPause),
        Key::L => state.activate(Target::Lap),
        Key::R => state.activate(Target::Reset),
        Key::M => state.activate(Target::ModeToggle),
        Key::T => state.activate(Target::SetTime),
        Key::H => state.activate(Target::HistoryToggle),
        Key::Up => {
            state.scroll_laps(-1);
            EventResult::Consumed
        }
        Key::Down => {
            state.scroll_laps(1);
            EventResult::Consumed
        }
        Key::PageUp => {
            state.scroll_laps(page(state.layout().lap_rows()).saturating_neg());
            EventResult::Consumed
        }
        Key::PageDown => {
            state.scroll_laps(page(state.layout().lap_rows()));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

fn handle_history_key(state: &mut StopwatchApp, event: &KeyEvent) -> EventResult {
    match event.key {
        Key::Escape | Key::H => state.activate(Target::Back),
        Key::Up => {
            state.scroll_history(-1);
            EventResult::Consumed
        }
        Key::Down => {
            state.scroll_history(1);
            EventResult::Consumed
        }
        Key::PageUp => {
            state.scroll_history(page(state.layout().history_rows()).saturating_neg());
            EventResult::Consumed
        }
        Key::PageDown => {
            state.scroll_history(page(state.layout().history_rows()));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

fn handle_setup_key(state: &mut StopwatchApp, event: &KeyEvent) -> EventResult {
    match event.key {
        // Enter and Escape both apply, as they always have. The toolbar's
        // Cancel is the only way to leave without applying, which is why it
        // exists rather than duplicating Escape.
        Key::Escape | Key::Enter => state.activate(Target::SetupConfirm),
        Key::Left => {
            state.countdown_setup_field = state.countdown_setup_field.saturating_sub(1);
            EventResult::Consumed
        }
        Key::Right => {
            state.countdown_setup_field = state.countdown_setup_field.saturating_add(1).min(2);
            EventResult::Consumed
        }
        Key::Up => state.activate(Target::SetupUp(state.countdown_setup_field)),
        Key::Down => state.activate(Target::SetupDown(state.countdown_setup_field)),
        _ => EventResult::Ignored,
    }
}

fn handle_mouse(state: &mut StopwatchApp, mouse: &MouseEvent) -> EventResult {
    match mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => match state.target_at(mouse.x, mouse.y) {
            Some(target) => state.activate(target),
            None => EventResult::Ignored,
        },
        // `wheel::rows_f` already answers in offset space -- positive means
        // "towards the end of the list" -- so the result is added as it comes.
        // Negating it here would scroll the table backwards.
        MouseEventKind::Scroll { dy, .. } => {
            let rows = wheel::rows_f(dy);
            if rows == 0.0 {
                return EventResult::Ignored;
            }
            let rows = rows as isize;
            match state.view {
                AppView::History => state.scroll_history(rows),
                AppView::Main => state.scroll_laps(rows),
                AppView::CountdownSetup => return EventResult::Ignored,
            }
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// The one event body, shared by the window and the probe.
pub fn handle_event(state: &mut StopwatchApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) if key.pressed => match state.view {
            AppView::Main => handle_main_key(state, key),
            AppView::History => handle_history_key(state, key),
            AppView::CountdownSetup => handle_setup_key(state, key),
        },
        Event::Mouse(mouse) => handle_mouse(state, mouse),
        Event::Resize { width, height } => {
            state.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // The event that makes a stopwatch a stopwatch.  It was not handled
        // here until 2026-08-25: `tick` existed, was correct on its own terms,
        // and had eight tests, but nothing outside them called it, so the
        // running clock never advanced.  That is known-issues.md lesson 45 in
        // its most literal form.
        //
        // Ignored while not running, so a tick that changes nothing does not
        // cost a repaint.
        Event::Tick { elapsed_ms } => {
            if state.state != TimerState::Running {
                return EventResult::Ignored;
            }
            state.tick(*elapsed_ms);
            EventResult::Consumed
        }
        Event::CloseRequested => {
            state.running = false;
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

impl App for StopwatchApp {
    fn title(&self) -> String {
        String::from("Stopwatch")
    }

    fn app_id(&self) -> String {
        String::from("stopwatch")
    }

    fn initial_size(&self) -> (u32, u32) {
        (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32)
    }

    /// A clock only while there is something to advance.
    ///
    /// `tick_interval` is consulted after every event, so this starts and stops
    /// with the timer: a stopped stopwatch asks for nothing and does not hold
    /// the desktop awake, and starting one re-arms the tick on the same event
    /// that started it.
    fn tick_interval(&self) -> Option<Duration> {
        (self.state == TimerState::Running).then(|| Duration::from_millis(TICK_MS))
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

impl Probe for StopwatchApp {
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

fn main() -> ExitCode {
    let mut app = StopwatchApp::default();
    app::launch("stopwatch", &mut app)
}

// ===========================================================================
// Tests
// ===========================================================================

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

    fn sample() -> StopwatchApp {
        StopwatchApp::default()
    }

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// Through the real event path, so a test cannot pass against a key the
    /// window would never deliver.
    fn press(app: &mut StopwatchApp, key: Key) -> EventResult {
        probe::key(app, &probe::press(key))
    }

    fn render(app: &StopwatchApp) -> Vec<RenderCommand> {
        app.frame(DEFAULT_WIDTH, DEFAULT_HEIGHT).commands().to_vec()
    }

    fn drawn_text(cmds: &[RenderCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn shows(app: &StopwatchApp, needle: &str) -> bool {
        drawn_text(&render(app)).iter().any(|t| t == needle)
    }

    /// Sizes to hold the layout to: the default, a big screen, a wide-and-short
    /// window, a small one, and two narrower than any toolbar was drawn for.
    const SIZES: [(f32, f32); 6] = [
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        (1600.0, 1000.0),
        (900.0, 320.0),
        (640.0, 480.0),
        (420.0, 300.0),
        (320.0, 240.0),
    ];

    // --- Time formatting ---

    #[test]
    fn format_time_zero() {
        assert_eq!(format_time_ms(0), "00:00.000");
    }

    #[test]
    fn format_time_seconds() {
        assert_eq!(format_time_ms(5_123), "00:05.123");
    }

    #[test]
    fn format_time_minutes() {
        assert_eq!(format_time_ms(125_456), "02:05.456");
    }

    #[test]
    fn format_time_hours() {
        assert_eq!(format_time_ms(3_661_789), "1:01:01.789");
    }

    #[test]
    fn format_time_short_basic() {
        assert_eq!(format_time_short(65_120), "01:05.12");
    }

    // --- App creation ---

    #[test]
    fn new_app() {
        let app = sample();
        assert_eq!(app.mode, AppMode::Stopwatch);
        assert_eq!(app.state, TimerState::Stopped);
        assert_eq!(app.elapsed_ms, 0);
        assert!(app.laps.is_empty());
        assert_eq!(app.view, AppView::Main);
    }

    // --- Start/pause/stop ---

    #[test]
    fn start_stopwatch() {
        let mut app = sample();
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn pause_stopwatch() {
        let mut app = sample();
        app.start();
        app.pause();
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn resume_stopwatch() {
        let mut app = sample();
        app.start();
        app.pause();
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn stop_resets() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 5000;
        app.lap();
        app.stop();
        assert_eq!(app.state, TimerState::Stopped);
        assert_eq!(app.elapsed_ms, 0);
        assert!(app.laps.is_empty());
    }

    #[test]
    fn pause_when_stopped_no_effect() {
        let mut app = sample();
        app.pause();
        assert_eq!(app.state, TimerState::Stopped);
    }

    #[test]
    fn start_when_running_no_effect() {
        let mut app = sample();
        app.start();
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    /// A countdown that has run out has nothing to resume.
    ///
    /// Without the guard in `start`, Space (or the Resume button, which the
    /// window now puts one click away) moved it to RUNNING at 00:00 with the
    /// alert still up and the tick advancing nothing -- a state only Reset
    /// could leave.
    #[test]
    fn a_finished_countdown_does_not_resume_into_nothing() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.countdown_setup_values = [0, 0, 1];
        app.start();
        app.tick(2000);
        assert!(app.countdown_finished);
        assert_eq!(app.state, TimerState::Paused);

        app.start();
        assert_eq!(
            app.state,
            TimerState::Paused,
            "a spent countdown was resumed into a clock with nothing to count"
        );
    }

    // --- Tick ---

    /// The clock advances when a real `Event::Tick` arrives.
    ///
    /// Through `handle_event`, not `tick`, deliberately: `tick` was correct
    /// and thoroughly tested for months while `handle_event` dropped the
    /// event, so a test that calls `tick` directly cannot tell a wired
    /// stopwatch from an unwired one.  This is the one that can -- checked by
    /// deleting the `Event::Tick` arm and confirming it, and only it, fails.
    #[test]
    fn a_tick_event_advances_the_clock() {
        let mut app = sample();
        app.start();
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: 500 }),
            EventResult::Consumed
        );
        assert_eq!(app.elapsed_ms, 500, "Event::Tick did not reach the clock");
    }

    /// …and a tick that changes nothing does not cost a repaint.
    #[test]
    fn a_tick_while_stopped_is_ignored_rather_than_redrawn() {
        let mut app = sample();
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: 500 }),
            EventResult::Ignored
        );
        assert_eq!(app.elapsed_ms, 0);
    }

    /// The window is only asked for a clock while there is something to move.
    ///
    /// `tick_interval` is consulted after every event, so this is not merely an
    /// optimisation: it is what stops a stopped stopwatch holding the desktop's
    /// event loop awake at 60Hz forever.
    #[test]
    fn the_clock_is_asked_for_only_while_it_runs() {
        let mut app = sample();
        assert_eq!(
            app.tick_interval(),
            None,
            "a stopped stopwatch asked for a clock"
        );
        app.start();
        assert_eq!(app.tick_interval(), Some(Duration::from_millis(TICK_MS)));
        app.pause();
        assert_eq!(
            app.tick_interval(),
            None,
            "a paused stopwatch asked for a clock"
        );
    }

    #[test]
    fn tick_running() {
        let mut app = sample();
        app.start();
        app.tick(500);
        assert_eq!(app.elapsed_ms, 500);
    }

    #[test]
    fn tick_paused_no_change() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 1000;
        app.pause();
        app.tick(1000);
        assert_eq!(app.elapsed_ms, 1000);
    }

    #[test]
    fn tick_stopped_no_change() {
        let mut app = sample();
        app.tick(1000);
        assert_eq!(app.elapsed_ms, 0);
    }

    #[test]
    fn tick_accumulates() {
        let mut app = sample();
        app.start();
        app.tick(100);
        app.tick(200);
        app.tick(200);
        assert_eq!(app.elapsed_ms, 500);
    }

    /// Time that passes while paused is not silently credited on resume.
    ///
    /// The old timestamp-taking `tick` needed a `last_tick_ms` field kept up
    /// to date on every dropped tick to get this right; with intervals it is
    /// automatic, because a dropped tick's interval is simply never added.
    #[test]
    fn a_pause_does_not_bank_time() {
        let mut app = sample();
        app.start();
        app.tick(1000);
        app.pause();
        app.tick(60_000);
        app.start(); // resume
        app.tick(1000);
        assert_eq!(app.elapsed_ms, 2000, "the paused minute was credited");
    }

    // --- Laps ---

    #[test]
    fn add_lap() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 5000;
        app.lap();
        assert_eq!(app.laps.len(), 1);
        assert_eq!(app.laps[0].number, 1);
        assert_eq!(app.laps[0].split_ms, 5000);
        assert_eq!(app.laps[0].lap_ms, 5000);
    }

    #[test]
    fn second_lap() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 3000;
        app.lap();
        app.elapsed_ms = 7000;
        app.lap();
        assert_eq!(app.laps.len(), 2);
        assert_eq!(app.laps[1].number, 2);
        assert_eq!(app.laps[1].split_ms, 7000);
        assert_eq!(app.laps[1].lap_ms, 4000);
    }

    #[test]
    fn lap_when_stopped_ignored() {
        let mut app = sample();
        app.elapsed_ms = 5000;
        app.lap();
        assert!(app.laps.is_empty());
    }

    #[test]
    fn lap_in_countdown_ignored() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.state = TimerState::Running;
        app.lap();
        assert!(app.laps.is_empty());
    }

    // --- Best/worst/average lap ---

    #[test]
    fn best_lap_needs_two() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 5000;
        app.lap();
        assert!(app.best_lap().is_none());
    }

    #[test]
    fn best_worst_lap() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 3000;
        app.lap();
        app.elapsed_ms = 5000;
        app.lap();
        app.elapsed_ms = 10000;
        app.lap();
        assert_eq!(app.best_lap().map(|l| l.lap_ms), Some(2000));
        assert_eq!(app.worst_lap().map(|l| l.lap_ms), Some(5000));
    }

    #[test]
    fn average_lap() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 2000;
        app.lap();
        app.elapsed_ms = 6000;
        app.lap();
        // Lap 1: 2000, Lap 2: 4000, avg = 3000
        assert_eq!(app.average_lap_ms(), Some(3000));
    }

    #[test]
    fn average_lap_empty() {
        let app = sample();
        assert_eq!(app.average_lap_ms(), None);
    }

    // --- Display time ---

    #[test]
    fn display_time_stopwatch() {
        let mut app = sample();
        app.elapsed_ms = 12345;
        assert_eq!(app.display_time(), 12345);
    }

    #[test]
    fn display_time_countdown() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.countdown_remaining_ms = 60000;
        assert_eq!(app.display_time(), 60000);
    }

    // --- Mode switching ---

    #[test]
    fn switch_mode() {
        let mut app = sample();
        press(&mut app, Key::M);
        assert_eq!(app.mode, AppMode::Countdown);
        press(&mut app, Key::M);
        assert_eq!(app.mode, AppMode::Stopwatch);
    }

    #[test]
    fn switch_mode_while_running_ignored() {
        let mut app = sample();
        app.start();
        press(&mut app, Key::M);
        assert_eq!(app.mode, AppMode::Stopwatch);
    }

    // --- Countdown ---

    #[test]
    fn countdown_tick() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        // start() recomputes the target from setup_values (h, m, s) in Countdown
        // mode, so configure 10 seconds there rather than poking the derived field.
        app.countdown_setup_values = [0, 0, 10];
        app.start();
        app.tick(3000);
        assert_eq!(app.countdown_remaining_ms, 7000);
    }

    #[test]
    fn countdown_finishes() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.countdown_setup_values = [0, 0, 5];
        app.start();
        app.tick(6000);
        assert_eq!(app.countdown_remaining_ms, 0);
        assert!(app.countdown_finished);
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn countdown_zero_target_no_start() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.countdown_setup_values = [0, 0, 0];
        app.start();
        assert_eq!(app.state, TimerState::Stopped);
    }

    #[test]
    fn countdown_setup_open() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        press(&mut app, Key::T);
        assert_eq!(app.view, AppView::CountdownSetup);
    }

    #[test]
    fn countdown_setup_not_in_stopwatch() {
        let mut app = sample();
        press(&mut app, Key::T);
        assert_eq!(app.view, AppView::Main);
    }

    #[test]
    fn countdown_setup_fields() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        assert_eq!(app.countdown_setup_field, 0);
        press(&mut app, Key::Right);
        assert_eq!(app.countdown_setup_field, 1);
        press(&mut app, Key::Right);
        assert_eq!(app.countdown_setup_field, 2);
        press(&mut app, Key::Right);
        assert_eq!(app.countdown_setup_field, 2); // clamped
    }

    #[test]
    fn countdown_setup_adjust() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        app.countdown_setup_values = [0, 0, 0];
        press(&mut app, Key::Up);
        assert_eq!(app.countdown_setup_values[0], 1);
        press(&mut app, Key::Down);
        assert_eq!(app.countdown_setup_values[0], 0);
        press(&mut app, Key::Down);
        assert_eq!(app.countdown_setup_values[0], 0); // clamped at 0
    }

    #[test]
    fn countdown_setup_confirm() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        app.countdown_setup_values = [0, 10, 30];
        press(&mut app, Key::Enter);
        assert_eq!(app.view, AppView::Main);
        assert_eq!(app.countdown_target_ms, 10 * 60_000 + 30 * 1_000);
    }

    /// Hours cap at 23 and minutes/seconds at 59, from the pointer as from the
    /// keyboard -- both go through `adjust_setup`, which is the point of
    /// routing the buttons through `activate` rather than their own bodies.
    #[test]
    fn the_setup_steppers_clamp_where_the_keys_do() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        app.countdown_setup_values = [23, 59, 59];
        for i in 0..3 {
            probe::click(&mut app, Target::SetupUp(i));
        }
        assert_eq!(
            app.countdown_setup_values,
            [23, 59, 59],
            "a stepper ran past its ceiling"
        );

        app.countdown_setup_values = [0, 0, 0];
        for i in 0..3 {
            probe::click(&mut app, Target::SetupDown(i));
        }
        assert_eq!(
            app.countdown_setup_values,
            [0, 0, 0],
            "a stepper ran below zero"
        );
    }

    /// Leaving setup without confirming puts the digits back.
    ///
    /// Otherwise the fields keep showing an edit the countdown never took, and
    /// the next Start silently runs the *old* duration while the setup view
    /// claims the new one.
    #[test]
    fn cancelling_setup_puts_the_digits_back() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.countdown_target_ms = 90_000; // 1m30s
        app.view = AppView::CountdownSetup;
        app.countdown_setup_values = [3, 7, 9];

        probe::click(&mut app, Target::Back);
        assert_eq!(app.view, AppView::Main);
        assert_eq!(
            app.countdown_setup_values,
            [0, 1, 30],
            "the fields kept an edit the timer never took"
        );
    }

    // --- History ---

    #[test]
    fn history_recorded_on_stop() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 5000;
        app.stop();
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.history[0].total_ms, 5000);
        assert_eq!(app.history[0].mode, AppMode::Stopwatch);
    }

    #[test]
    fn history_not_recorded_when_stopped() {
        let mut app = sample();
        app.stop();
        assert!(app.history.is_empty());
    }

    #[test]
    fn history_view_toggle() {
        let mut app = sample();
        press(&mut app, Key::H);
        assert_eq!(app.view, AppView::History);
        press(&mut app, Key::Escape);
        assert_eq!(app.view, AppView::Main);
    }

    #[test]
    fn history_scroll() {
        let mut app = sample();
        app.view = AppView::History;
        for _ in 0..40 {
            app.history.push(SessionRecord {
                mode: AppMode::Stopwatch,
                total_ms: 5000,
                lap_count: 0,
                best_lap_ms: None,
                worst_lap_ms: None,
            });
        }
        press(&mut app, Key::Down);
        assert_eq!(app.history_scroll, 1);
    }

    // --- Key handling ---

    #[test]
    fn space_starts() {
        let mut app = sample();
        press(&mut app, Key::Space);
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn space_pauses() {
        let mut app = sample();
        app.start();
        press(&mut app, Key::Space);
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn r_resets() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 5000;
        press(&mut app, Key::R);
        assert_eq!(app.state, TimerState::Stopped);
        assert_eq!(app.elapsed_ms, 0);
    }

    #[test]
    fn l_adds_lap() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 5000;
        press(&mut app, Key::L);
        assert_eq!(app.laps.len(), 1);
    }

    #[test]
    fn key_released_ignored() {
        let mut app = sample();
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Space,
                pressed: false,
                modifiers: Modifiers::NONE,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, TimerState::Stopped);
    }

    #[test]
    fn a_key_nothing_is_bound_to_is_ignored() {
        let mut app = sample();
        assert_eq!(
            handle_event(&mut app, &Event::Key(make_key(Key::F7))),
            EventResult::Ignored
        );
    }

    #[test]
    fn handle_event_routes_keys() {
        let mut app = sample();
        handle_event(&mut app, &Event::Key(make_key(Key::Space)));
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn closing_the_window_stops_the_app() {
        let mut app = sample();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    // --- Enum equality ---

    #[test]
    fn timer_state_eq() {
        assert_eq!(TimerState::Running, TimerState::Running);
        assert_ne!(TimerState::Running, TimerState::Paused);
    }

    #[test]
    fn app_mode_eq() {
        assert_eq!(AppMode::Stopwatch, AppMode::Stopwatch);
        assert_ne!(AppMode::Stopwatch, AppMode::Countdown);
    }

    // --- Rendering ---

    #[test]
    fn render_main() {
        assert!(shows(&sample(), "STOPWATCH"));
    }

    #[test]
    fn render_running() {
        let mut app = sample();
        app.start();
        assert!(shows(&app, "RUNNING"));
    }

    #[test]
    fn render_countdown_mode() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        assert!(shows(&app, "COUNTDOWN"));
    }

    #[test]
    fn render_countdown_finished() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.countdown_finished = true;
        assert!(shows(&app, "TIME'S UP!"));
    }

    #[test]
    fn render_history_empty() {
        let mut app = sample();
        app.view = AppView::History;
        assert!(shows(&app, "No sessions recorded yet."));
    }

    #[test]
    fn render_history_with_data() {
        let mut app = sample();
        app.view = AppView::History;
        app.history.push(SessionRecord {
            mode: AppMode::Stopwatch,
            total_ms: 10_000,
            lap_count: 3,
            best_lap_ms: Some(2000),
            worst_lap_ms: Some(5000),
        });
        assert!(shows(&app, "SESSION HISTORY"));
        assert!(shows(&app, "Stopwatch"));
    }

    #[test]
    fn render_countdown_setup() {
        let mut app = sample();
        app.view = AppView::CountdownSetup;
        assert!(shows(&app, "SET COUNTDOWN"));
        assert!(shows(&app, "Hours"));
    }

    #[test]
    fn render_laps() {
        let mut app = sample();
        app.start();
        app.elapsed_ms = 5000;
        app.lap();
        app.elapsed_ms = 9000;
        app.lap();
        assert!(shows(&app, "#1"));
        assert!(shows(&app, "#2"));
    }

    #[test]
    fn render_has_background() {
        let cmds = render(&sample());
        assert!(
            cmds.iter().any(
                |c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0)
            )
        );
    }

    // --- The window ---

    /// The probe draws at the size the window opens at, so a hit box a test
    /// finds is a hit box the user can hit.
    #[test]
    fn the_window_declares_the_size_the_probe_draws_at() {
        let app = sample();
        assert_eq!(
            app.initial_size(),
            (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32)
        );
        assert_eq!(
            <StopwatchApp as Probe>::SIZE,
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        );
        assert_eq!(app.title(), "Stopwatch");
        assert_eq!(app.app_id(), "stopwatch");
    }

    /// Every clip and translate is closed, in every view, at every size.
    #[test]
    fn every_view_draws_a_balanced_frame_at_every_reasonable_size() {
        for (w, h) in SIZES {
            for view in [AppView::Main, AppView::History, AppView::CountdownSetup] {
                for mode in [AppMode::Stopwatch, AppMode::Countdown] {
                    let mut app = StopwatchApp::new(w, h);
                    app.view = view;
                    app.mode = mode;
                    app.start();
                    for i in 1..=20 {
                        app.elapsed_ms = i * 1000;
                        app.lap();
                    }
                    for _ in 0..20 {
                        app.history.push(SessionRecord {
                            mode,
                            total_ms: 1234,
                            lap_count: 2,
                            best_lap_ms: Some(500),
                            worst_lap_ms: Some(700),
                        });
                    }
                    let frame = app.frame(w, h);
                    assert!(
                        frame.is_balanced(),
                        "unbalanced frame at {w}x{h} in {view:?}/{mode:?}"
                    );
                }
            }
        }
    }

    /// The layout is derived from the window, not from a constant.
    #[test]
    fn the_layout_follows_the_window_instead_of_a_constant() {
        let small = Layout::new(400.0, 400.0, false);
        let large = Layout::new(1200.0, 900.0, false);
        assert!(
            large.header.w > small.header.w,
            "the header ignored the width"
        );
        assert!(large.list.h > small.list.h, "the list ignored the height");
        assert!(
            large.time_font > small.time_font,
            "the readout ignored the width"
        );
        assert!(
            small.list.bottom() <= 400.0,
            "the list ran through the window floor"
        );
        assert!(large.list.bottom() <= 900.0);
    }

    /// The lap table is as long as the window is tall.
    ///
    /// This was `max_visible_laps: 8` -- a field no window size could move, so
    /// a 1000px-tall window showed eight laps and half a screen of nothing,
    /// and a 300px one drew eight rows through its own floor.
    #[test]
    fn the_lap_table_is_as_long_as_the_window_is_tall() {
        let short = StopwatchApp::new(DEFAULT_WIDTH, 340.0).layout().lap_rows();
        let tall = StopwatchApp::new(DEFAULT_WIDTH, 1000.0).layout().lap_rows();
        assert!(
            tall > short,
            "a taller window showed no more laps ({tall} vs {short})"
        );
        assert!(short < 8, "a 340px window still claimed eight rows");
    }

    /// …and the history list is the same number, derived the same way.
    ///
    /// It used to carry its own `8`, written twice: once in the scroll clamp
    /// and once in the renderer, with nothing tying them together.
    #[test]
    fn the_two_lists_no_longer_share_a_magic_eight() {
        for (w, h) in SIZES {
            let app = StopwatchApp::new(w, h);
            let layout = app.layout();
            // The history view has no big readout above it and no stats footer
            // below, so it fits strictly more rows than the lap table at the
            // same size -- which is the proof they are both measured rather
            // than both hardcoded.
            assert!(
                layout.history_rows() >= layout.lap_rows(),
                "history showed fewer rows than laps at {w}x{h}"
            );
        }
    }

    /// No row is painted below the floor of the region that holds it.
    #[test]
    fn the_table_never_paints_through_its_own_floor() {
        for (w, h) in SIZES {
            let mut app = StopwatchApp::new(w, h);
            app.start();
            for i in 1..=60 {
                app.elapsed_ms = i * 1000;
                app.lap();
            }
            let layout = app.layout();
            let floor = layout.list.bottom() - STATS_H;
            let rows: Vec<f32> = app
                .frame(w, h)
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, y, .. } if text.starts_with('#') => Some(*y),
                    _ => None,
                })
                .collect();
            assert!(
                rows.len() <= layout.lap_rows(),
                "{} rows drawn into room for {} at {w}x{h}",
                rows.len(),
                layout.lap_rows()
            );
            for y in rows {
                assert!(
                    y + ROW_H <= floor + 0.01,
                    "a lap row was drawn at {y}, past the {floor} floor at {w}x{h}"
                );
            }
        }
    }

    /// Every button in every toolbar is inside the window it was drawn for.
    #[test]
    fn the_toolbar_never_leaves_the_window() {
        for (w, h) in SIZES {
            for view in [AppView::Main, AppView::History, AppView::CountdownSetup] {
                for mode in [AppMode::Stopwatch, AppMode::Countdown] {
                    let mut app = StopwatchApp::new(w, h);
                    app.view = view;
                    app.mode = mode;
                    let frame = app.frame(w, h);
                    for (target, rect) in frame.hits() {
                        assert!(
                            rect.x >= 0.0 && rect.right() <= w + 0.01,
                            "{target:?} at {rect:?} left the {w}x{h} window in {view:?}"
                        );
                        assert!(
                            rect.y >= 0.0 && rect.bottom() <= h + 0.01,
                            "{target:?} at {rect:?} left the {w}x{h} window in {view:?}"
                        );
                    }
                }
            }
        }
    }

    /// Every toolbar button does what its label says, from the pointer.
    ///
    /// None of these were reachable with a mouse before: the whole program was
    /// keys and a line of grey text telling you which.
    #[test]
    fn every_toolbar_button_does_what_it_says() {
        let mut app = sample();
        probe::click(&mut app, Target::StartPause);
        assert_eq!(app.state, TimerState::Running, "Start did not start");

        app.elapsed_ms = 4000;
        probe::click(&mut app, Target::Lap);
        assert_eq!(app.laps.len(), 1, "Lap did not record a lap");

        probe::click(&mut app, Target::StartPause);
        assert_eq!(app.state, TimerState::Paused, "Pause did not pause");

        probe::click(&mut app, Target::Reset);
        assert_eq!(app.state, TimerState::Stopped, "Reset did not stop");
        assert_eq!(app.history.len(), 1, "Reset did not record the session");

        probe::click(&mut app, Target::ModeToggle);
        assert_eq!(app.mode, AppMode::Countdown, "Mode did not switch");

        probe::click(&mut app, Target::SetTime);
        assert_eq!(
            app.view,
            AppView::CountdownSetup,
            "Set Time did not open setup"
        );

        probe::click(&mut app, Target::SetupConfirm);
        assert_eq!(app.view, AppView::Main, "Confirm did not close setup");

        probe::click(&mut app, Target::HistoryToggle);
        assert_eq!(app.view, AppView::History, "History did not open");

        probe::click(&mut app, Target::Back);
        assert_eq!(app.view, AppView::Main, "Back did not return");
    }

    /// A click on a stepper is a step, not merely a field selection.
    ///
    /// The stepper boxes sit inside the card that selects the field, and
    /// `hit_test` walks backwards, so this only holds because the card's box is
    /// recorded first.
    #[test]
    fn a_stepper_takes_the_click_off_the_card_it_sits_on() {
        let mut app = sample();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        app.countdown_setup_values = [0, 0, 0];

        let up = probe::rect_of(&app, Target::SetupUp(1)).expect("no stepper drawn");
        let card = probe::rect_of(&app, Target::SetupField(1)).expect("no card drawn");
        assert!(
            card.intersect(up) == Some(up),
            "the stepper is not inside the card it must win against"
        );

        let (cx, cy) = up.centre();
        app.click_at(cx, cy, MouseButton::Left, <StopwatchApp as Probe>::SIZE);
        assert_eq!(
            app.countdown_setup_values[1], 1,
            "the click selected the field instead of stepping it"
        );
    }

    fn scroll(dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x: DEFAULT_WIDTH * 0.5,
            y: DEFAULT_HEIGHT * 0.75,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    /// The wheel scrolls the lap table, and stops at both ends.
    #[test]
    fn the_wheel_scrolls_the_lap_table_and_stops_at_both_ends() {
        let mut app = sample();
        app.start();
        for i in 1..=60 {
            app.elapsed_ms = i * 1000;
            app.lap();
        }
        app.lap_scroll = 0;

        handle_event(&mut app, &scroll(-1.0));
        assert!(app.lap_scroll > 0, "the wheel moved nothing");

        for _ in 0..80 {
            handle_event(&mut app, &scroll(-1.0));
        }
        assert_eq!(
            app.lap_scroll,
            app.max_lap_scroll(),
            "the wheel ran past the end"
        );

        for _ in 0..200 {
            handle_event(&mut app, &scroll(1.0));
        }
        assert_eq!(app.lap_scroll, 0, "the wheel ran past the start");
    }

    /// Growing the window hands back the scroll it no longer needs.
    #[test]
    fn growing_the_window_gives_back_the_scroll_it_no_longer_needs() {
        let mut app = StopwatchApp::new(DEFAULT_WIDTH, 340.0);
        app.start();
        for i in 1..=30 {
            app.elapsed_ms = i * 1000;
            app.lap();
        }
        app.lap_scroll = app.max_lap_scroll();
        assert!(app.lap_scroll > 0);

        handle_event(
            &mut app,
            &Event::Resize {
                width: DEFAULT_WIDTH as u32,
                height: 2000,
            },
        );
        assert_eq!(
            app.lap_scroll,
            app.max_lap_scroll(),
            "a window with room for every lap kept scrolling past them"
        );
        assert_eq!(app.lap_scroll, 0, "thirty laps did not fit a 2000px window");
    }

    #[test]
    fn a_resize_event_is_what_moves_the_layout() {
        let mut app = sample();
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Resize {
                    width: 800,
                    height: 600
                }
            ),
            EventResult::Consumed
        );
        assert_eq!(app.layout().window.w, 800.0);
        assert_eq!(app.layout().window.h, 600.0);
    }

    /// The alert band costs space only when there is an alert to put in it.
    #[test]
    fn the_alert_band_only_costs_space_when_there_is_an_alert() {
        let quiet = Layout::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, false);
        let loud = Layout::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, true);
        assert!(quiet.alert.is_none());
        assert!(loud.alert.is_some());
        assert!(
            loud.list.h < quiet.list.h,
            "the alert appeared without taking the room it needs"
        );
    }
}
