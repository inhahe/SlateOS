//! Slate OS Alarm Clock
//!
//! Combined alarm clock, timer, and stopwatch utility with:
//! - **Alarms**: Create/edit/delete alarms with hour, minute, label, repeat days,
//!   sound selection, enable/disable toggle, snooze (5/10/15/30 min), AM/PM and
//!   24h mode, next-alarm indicator.
//! - **Timer**: Quick presets (1/3/5/10/15/30/60 min), custom duration
//!   (hours:minutes:seconds), countdown with progress ring, multiple
//!   simultaneous timers, timer labels.
//! - **Stopwatch**: Start/stop/reset, lap times with splits, best/worst/average
//!   lap statistics, lap history table.
//!
//! Uses the guitk library for UI rendering with a Catppuccin Mocha dark theme.
//!
//! # Drawing and hit testing are one walk
//!
//! Every view draws into a [`Frame`], which records a [`Target`] for each
//! control at the same moment it emits the rectangle for it. There is no second
//! table of "where the buttons are" to drift out of step with the drawing, and
//! a control scrolled out of its pane is not clickable because the frame trims
//! its hit box to the clip in force. See [`guitk::frame`].
//!
//! # This program's whole point is that it is ticked
//!
//! An alarm clock that is not sent [`Event::Tick`] is a clock that does not
//! run: countdowns freeze, snoozes never expire, and the time on screen is
//! whatever it was when the window opened. [`App::tick_interval`] is therefore
//! the single most important method in this file, and it varies — a running
//! stopwatch needs a fast clock to show hundredths, and everything else is
//! content with twice a second.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

#[allow(dead_code)]
const BASE: Color = Color::from_hex(0x1E1E2E);
#[allow(dead_code)]
const MANTLE: Color = Color::from_hex(0x181825);
#[allow(dead_code)]
const CRUST: Color = Color::from_hex(0x11111B);
#[allow(dead_code)]
const SURFACE0: Color = Color::from_hex(0x313244);
#[allow(dead_code)]
const SURFACE1: Color = Color::from_hex(0x45475A);
#[allow(dead_code)]
const SURFACE2: Color = Color::from_hex(0x585B70);
#[allow(dead_code)]
const OVERLAY0: Color = Color::from_hex(0x6C7086);
#[allow(dead_code)]
const OVERLAY1: Color = Color::from_hex(0x7F849C);
#[allow(dead_code)]
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
#[allow(dead_code)]
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
#[allow(dead_code)]
const BLUE: Color = Color::from_hex(0x89B4FA);
#[allow(dead_code)]
const GREEN: Color = Color::from_hex(0xA6E3A1);
#[allow(dead_code)]
const RED: Color = Color::from_hex(0xF38BA8);
#[allow(dead_code)]
const YELLOW: Color = Color::from_hex(0xF9E2AF);
#[allow(dead_code)]
const PEACH: Color = Color::from_hex(0xFAB387);
#[allow(dead_code)]
const MAUVE: Color = Color::from_hex(0xCBA6F7);
#[allow(dead_code)]
const TEAL: Color = Color::from_hex(0x94E2D5);
#[allow(dead_code)]
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
#[allow(dead_code)]
const ROSEWATER: Color = Color::from_hex(0xF5E0DC);
#[allow(dead_code)]
const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
#[allow(dead_code)]
const SAPPHIRE: Color = Color::from_hex(0x74C7EC);
#[allow(dead_code)]
// `0x89DCEB`. Was `0x89DCFE` — a transposed byte pair copied from
// `gui/appearance`. Unused here today, but a wrong constant behind
// `#[allow(dead_code)]` is a wrong constant the next caller inherits.
const SKY: Color = Color::from_hex(0x89DCEB);
#[allow(dead_code)]
const MAROON: Color = Color::from_hex(0xEBA0AC);
#[allow(dead_code)]
const PINK: Color = Color::from_hex(0xF5C2E7);

// ============================================================================
// Constants
// ============================================================================

/// Window dimensions.
const WINDOW_WIDTH: f32 = 480.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// Tab bar height.
const TAB_BAR_HEIGHT: f32 = 48.0;

/// Standard padding.
const PADDING: f32 = 16.0;

/// Number of segments in the progress ring.
const RING_SEGMENTS: usize = 60;

/// Default snooze durations in minutes.
const SNOOZE_OPTIONS: [u32; 4] = [5, 10, 15, 30];

/// Quick timer presets in minutes.
const TIMER_PRESETS: [u32; 7] = [1, 3, 5, 10, 15, 30, 60];

/// Height of one alarm card, and the gap under it.
const ALARM_ROW_H: f32 = 72.0;
const ALARM_ROW_GAP: f32 = 8.0;

/// Height of one timer card, and the gap under it.
const TIMER_ROW_H: f32 = 120.0;
const TIMER_ROW_GAP: f32 = 8.0;

/// Height of one lap row in the stopwatch table.
const LAP_ROW_H: f32 = 22.0;

/// Height of the clock band at the top of the alarm tab.
const CLOCK_H: f32 = 84.0;

/// Height of the `+ Add Alarm` button, of one quick-start preset chip, and of
/// the custom-duration row. Named rather than repeated, so the rectangle that
/// is drawn and the one that is clicked cannot drift apart.
const ADD_BUTTON_H: f32 = 36.0;
const PRESET_H: f32 = 32.0;
const CUSTOM_H: f32 = 36.0;

/// Gap between two chips in a row of them.
const CHIP_GAP: f32 = 8.0;

/// The alarm editor's vertical stack, at the size it would like to be drawn.
///
/// A pad, then six rows — title, the hour/minute spinners, the label field, the
/// repeat-day chips, the sound/snooze pair, and the Save/Cancel strip — with a
/// gap between each and a pad under the last. These add up to
/// [`EDITOR_NATURAL_H`], which is *taller than the content area of a window at
/// this app's own minimum size*: laid out at natural size the Save and Cancel
/// buttons fall below the panel, where the clip hides them and [`Frame::hit`]
/// drops their hit boxes — an editor with no pointer route out.
///
/// So the stack is solved rather than assumed. See [`AlarmClockApp::draw_editor`].
const EDITOR_PAD: f32 = 10.0;
const EDITOR_GAP: f32 = 8.0;
const EDITOR_TITLE_H: f32 = 20.0;
const EDITOR_SPINNER_H: f32 = 102.0;
const EDITOR_LABEL_H: f32 = 32.0;
const EDITOR_CHIP_H: f32 = 30.0;
const EDITOR_ACTION_H: f32 = 34.0;

/// The height the editor's stack wants: two pads, five gaps, six rows.
const EDITOR_NATURAL_H: f32 = EDITOR_PAD * 2.0
    + EDITOR_GAP * 5.0
    + EDITOR_TITLE_H
    + EDITOR_SPINNER_H
    + EDITOR_LABEL_H
    + EDITOR_CHIP_H * 2.0
    + EDITOR_ACTION_H;

/// Presets per row on the timer tab. Seven in rows of four is two rows with a
/// ragged end, which is what a wrapped grid looks like; four across is the
/// widest that keeps `60 min` legible at [`MIN_WIDTH`].
const PRESETS_PER_ROW: usize = 4;

/// Longest label a user may type. Not a styling choice: the label is drawn into
/// a fixed-width card and is stored per alarm, so an unbounded field is an
/// unbounded allocation driven straight from the keyboard.
const MAX_LABEL_LEN: usize = 64;

/// The smallest window this layout is drawn for. Below it rectangles start to
/// overlap rather than merely crowd, so the size handed to [`AlarmClockApp::frame`]
/// is clamped up to this and the view is allowed to run off the bottom edge —
/// a scrolled pane is usable, an overlapping one is not.
const MIN_WIDTH: f32 = 360.0;
const MIN_HEIGHT: f32 = 320.0;

/// How often the app is ticked while the stopwatch is running.
///
/// The stopwatch shows hundredths, so a slower clock would show a display that
/// visibly jumps. It advances by the *measured* `elapsed_ms` of each tick, not
/// by this number, so a late tick costs nothing but a late repaint.
const TICK_FAST: Duration = Duration::from_millis(50);

/// How often the app is ticked the rest of the time.
///
/// Never `None`: the alarm list shows a running clock and alarms fire from it,
/// so an app that stopped its clock whenever the stopwatch was stopped would be
/// an alarm clock that only rings while you are timing something.
const TICK_SLOW: Duration = Duration::from_millis(500);

// ============================================================================
// Targets — every control the pointer can land on
// ============================================================================

/// A control the pointer can land on.
///
/// One variant per thing a user can click. Payloads are stable identifiers
/// (`AlarmId`, `TimerId`) and never indices into a `Vec`, so a hit box recorded
/// while drawing still names the right alarm after a delete reorders the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// One of the three tabs across the top.
    Tab(ActiveTab),
    /// The big clock on the alarm tab. Clicking it swaps 12- and 24-hour.
    ClockFormat,

    // ---- alarm list ----
    /// The `+ Add Alarm` button.
    AddAlarm,
    /// An alarm card, anywhere that is not one of the controls on it.
    AlarmRow(AlarmId),
    /// The enable/disable pill on an alarm card.
    AlarmToggle(AlarmId),
    /// The delete cross on an alarm card.
    AlarmDelete(AlarmId),
    /// `Snooze`, shown only while that alarm is ringing.
    AlarmSnooze(AlarmId),
    /// `Dismiss`, shown while ringing or snoozed.
    AlarmDismiss(AlarmId),

    // ---- alarm editor ----
    /// Step the hour being edited by one, up or down.
    EditHour(Step),
    /// Step the minute being edited by one, up or down.
    EditMinute(Step),
    /// The label entry field.
    EditLabel,
    /// One of the seven repeat-day chips.
    EditDay(Weekday),
    /// Cycle the alarm sound.
    EditSound,
    /// Cycle the snooze duration.
    EditSnooze,
    /// Commit the editor.
    EditSave,
    /// Abandon the editor.
    EditCancel,

    // ---- timer tab ----
    /// A quick-start preset, in minutes.
    Preset(u32),
    /// One of the three custom-duration entry fields.
    CustomField(HmsField),
    /// Start a timer for whatever the custom fields say.
    CustomStart,
    /// A timer card, anywhere that is not one of the controls on it.
    TimerRow(TimerId),
    /// Start/pause on a timer card.
    TimerToggle(TimerId),
    /// Reset on a timer card.
    TimerReset(TimerId),
    /// Delete on a timer card.
    TimerDelete(TimerId),

    // ---- stopwatch tab ----
    /// Start/pause the stopwatch.
    SwToggle,
    /// Record a lap.
    SwLap,
    /// Reset the stopwatch and clear the laps.
    SwReset,
}

/// Which way a stepper points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
}

/// One of the three custom-duration entry fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HmsField {
    Hours,
    Minutes,
    Seconds,
}

impl HmsField {
    /// The three fields left to right.
    pub const ALL: [HmsField; 3] = [HmsField::Hours, HmsField::Minutes, HmsField::Seconds];

    /// The placeholder shown when the field is empty.
    #[must_use]
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Hours => "HH",
            Self::Minutes => "MM",
            Self::Seconds => "SS",
        }
    }

    /// Position in [`Self::ALL`], which is also the index into the entry
    /// buffers.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Hours => 0,
            Self::Minutes => 1,
            Self::Seconds => 2,
        }
    }
}

/// What has the keyboard.
///
/// Exactly one thing can, and `None` is a real state rather than a stand-in for
/// "the label field": with nothing focused, a bare letter is a shortcut. That is
/// the distinction that stops typing a label from resetting the stopwatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// The editor's label field.
    Label,
    /// One of the custom-duration fields.
    Custom(HmsField),
}

/// A frame being drawn. See [`guitk::frame`] for why drawing and hit-testing
/// are the same walk.
pub type Frame = guitk::frame::Frame<Target>;

/// What a handled event asks the window to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing changed; do not repaint.
    None,
    /// State changed; repaint.
    Redraw,
    /// Close the window.
    Quit,
}

// ============================================================================
// Drawing helpers
// ============================================================================

/// Fill a rounded rectangle.
fn fill(f: &mut Frame, rect: Rect, color: Color, radius: f32) {
    f.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

/// Draw a run of text with its top-left corner at `(x, y)`.
fn text(
    f: &mut Frame,
    x: f32,
    y: f32,
    body: impl Into<String>,
    color: Color,
    size: f32,
    weight: FontWeightHint,
    max_width: f32,
) {
    let bound = max_width.max(0.0);
    // Refuse a run the clip in force cannot show, exactly as `guitk::put_text`
    // does. This helper used to push unconditionally, and that is the whole
    // reason an overrunning pass in this app reached the *picture* rather than
    // stopping at the pixels: a clip hides an overrun from the eye, but a
    // `RenderCommand::Text` pushed under it still says a label is on screen, and
    // anything that reads the frame to find out what is displayed is told a lie.
    // `Frame::hit` already drops a hit box with nothing visible, so refusing
    // here is what puts ink and hit boxes back under one rule.
    if !f.is_visible(Rect::new(x, y, bound, size)) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: body.into(),
        color,
        font_size: size,
        font_weight: weight,
        max_width: Some(bound),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Draw a run of text centred horizontally in `[x, x + width)`.
///
/// Centred by measuring, not by a hand-tuned offset. The offsets this file used
/// to carry (`x + width / 2.0 - 80.0`) were correct for one string at one font
/// size and drifted the moment either changed — and the clock string changes
/// width whenever the format is toggled, which is a control the user can reach.
fn text_centred(
    f: &mut Frame,
    x: f32,
    y: f32,
    width: f32,
    body: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    let measured = guitk::text::measure(body, size, weight).min(width);
    let left = x + (width - measured) / 2.0;
    // Bounded to `measured`, not to `width`. The bound is measured from where
    // the run *starts*, and a centred run starts half the slack in -- so
    // passing `width` declares a box that hangs that same half-slack off the
    // right-hand end of the box the run was told to centre in. "Stopwatch" in
    // a 120-point tab was declared to run to 385 in a 360-point window: a
    // quarter of it off the edge of the screen, and no ellipsis until then,
    // because the renderer trims at the bound it is given and this one was a
    // lie. Where the string is too long to centre, `measured` saturates at
    // `width`, `left` lands back at `x`, and the bound is the box exactly.
    text(f, left, y, body, color, size, weight, measured);
}

/// A filled button with a centred label and a hit box, drawn in one call so the
/// two can never name different rectangles.
fn button(f: &mut Frame, rect: Rect, label: &str, bg: Color, fg: Color, target: Target) {
    let size = 13.0;
    fill(f, rect, bg, (rect.h / 2.0).min(8.0));
    text_centred(
        f,
        rect.x,
        rect.y + (rect.h - size) / 2.0 - 1.0,
        rect.w,
        label,
        fg,
        size,
        FontWeightHint::Bold,
    );
    f.hit(target, rect);
}

// ============================================================================
// Active tab
// ============================================================================

/// Which tab is currently selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ActiveTab {
    #[default]
    Alarm,
    Timer,
    Stopwatch,
}

impl ActiveTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Alarm => "Alarm",
            Self::Timer => "Timer",
            Self::Stopwatch => "Stopwatch",
        }
    }

    pub fn all() -> [Self; 3] {
        [Self::Alarm, Self::Timer, Self::Stopwatch]
    }
}

// ============================================================================
// Days of the week
// ============================================================================

/// Day of the week for alarm repeat scheduling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub fn short_name(self) -> &'static str {
        match self {
            Self::Monday => "Mon",
            Self::Tuesday => "Tue",
            Self::Wednesday => "Wed",
            Self::Thursday => "Thu",
            Self::Friday => "Fri",
            Self::Saturday => "Sat",
            Self::Sunday => "Sun",
        }
    }

    pub fn single_letter(self) -> &'static str {
        match self {
            Self::Monday => "M",
            Self::Tuesday => "T",
            Self::Wednesday => "W",
            Self::Thursday => "T",
            Self::Friday => "F",
            Self::Saturday => "S",
            Self::Sunday => "S",
        }
    }

    pub fn all() -> [Self; 7] {
        [
            Self::Monday,
            Self::Tuesday,
            Self::Wednesday,
            Self::Thursday,
            Self::Friday,
            Self::Saturday,
            Self::Sunday,
        ]
    }

    pub fn index(self) -> usize {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }

    pub fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::Monday),
            1 => Some(Self::Tuesday),
            2 => Some(Self::Wednesday),
            3 => Some(Self::Thursday),
            4 => Some(Self::Friday),
            5 => Some(Self::Saturday),
            6 => Some(Self::Sunday),
            _ => None,
        }
    }
}

// ============================================================================
// Time format
// ============================================================================

/// Whether to display time in 12-hour or 24-hour format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TimeFormat {
    #[default]
    TwelveHour,
    TwentyFourHour,
}

impl TimeFormat {
    /// Format an hour (0..=23) for display.
    pub fn format_hour(self, hour: u8) -> (u8, Option<&'static str>) {
        match self {
            Self::TwelveHour => {
                let period = if hour < 12 { "AM" } else { "PM" };
                let display_hour = match hour {
                    0 => 12,
                    1..=12 => hour,
                    _ => hour.saturating_sub(12),
                };
                (display_hour, Some(period))
            }
            Self::TwentyFourHour => (hour, None),
        }
    }
}

// ============================================================================
// Sound selection
// ============================================================================

/// Available alarm/timer sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AlarmSound {
    #[default]
    Default,
    Chime,
    Bell,
    Digital,
    Gentle,
    Loud,
}

impl AlarmSound {
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Chime => "Chime",
            Self::Bell => "Bell",
            Self::Digital => "Digital",
            Self::Gentle => "Gentle",
            Self::Loud => "Loud",
        }
    }

    pub fn all() -> [Self; 6] {
        [
            Self::Default,
            Self::Chime,
            Self::Bell,
            Self::Digital,
            Self::Gentle,
            Self::Loud,
        ]
    }

    pub fn from_index(idx: usize) -> Option<Self> {
        Self::all().get(idx).copied()
    }

    pub fn index(self) -> usize {
        match self {
            Self::Default => 0,
            Self::Chime => 1,
            Self::Bell => 2,
            Self::Digital => 3,
            Self::Gentle => 4,
            Self::Loud => 5,
        }
    }
}

// ============================================================================
// Snooze duration
// ============================================================================

/// Snooze duration in minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnoozeDuration {
    pub minutes: u32,
}

impl SnoozeDuration {
    pub fn new(minutes: u32) -> Self {
        Self { minutes }
    }

    pub fn label(self) -> String {
        format!("{} min", self.minutes)
    }

    pub fn as_seconds(self) -> u64 {
        u64::from(self.minutes) * 60
    }
}

impl Default for SnoozeDuration {
    fn default() -> Self {
        Self {
            minutes: SNOOZE_OPTIONS[0],
        }
    }
}

// ============================================================================
// Alarm
// ============================================================================

/// Unique identifier for an alarm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AlarmId(pub u64);

/// A single alarm entry.
#[derive(Clone, Debug)]
pub struct Alarm {
    pub id: AlarmId,
    pub hour: u8,
    pub minute: u8,
    pub label: String,
    pub enabled: bool,
    pub repeat_days: [bool; 7],
    pub sound: AlarmSound,
    pub snooze_duration: SnoozeDuration,
    /// Whether this alarm is currently snoozed (countdown remaining in seconds).
    pub snoozed_remaining: Option<u64>,
    /// Whether the alarm is currently ringing.
    pub ringing: bool,
}

impl Alarm {
    pub fn new(id: AlarmId, hour: u8, minute: u8) -> Self {
        Self {
            id,
            hour: hour.min(23),
            minute: minute.min(59),
            label: String::new(),
            enabled: true,
            repeat_days: [false; 7],
            sound: AlarmSound::Default,
            snooze_duration: SnoozeDuration::default(),
            snoozed_remaining: None,
            ringing: false,
        }
    }

    /// Whether this alarm repeats on any day.
    pub fn is_repeating(&self) -> bool {
        self.repeat_days.iter().any(|&d| d)
    }

    /// Whether this alarm repeats on a specific weekday.
    pub fn repeats_on(&self, day: Weekday) -> bool {
        self.repeat_days.get(day.index()).copied().unwrap_or(false)
    }

    /// Toggle repeat for a weekday.
    pub fn toggle_day(&mut self, day: Weekday) {
        if let Some(slot) = self.repeat_days.get_mut(day.index()) {
            *slot = !*slot;
        }
    }

    /// Set repeat for a weekday.
    pub fn set_day(&mut self, day: Weekday, enabled: bool) {
        if let Some(slot) = self.repeat_days.get_mut(day.index()) {
            *slot = enabled;
        }
    }

    /// Summary of repeat days (e.g. "Mon, Wed, Fri" or "Every day" or "Once").
    pub fn repeat_summary(&self) -> String {
        if !self.is_repeating() {
            return "Once".to_string();
        }
        let active_count = self.repeat_days.iter().filter(|&&d| d).count();
        if active_count == 7 {
            return "Every day".to_string();
        }
        // Check for weekdays only
        let weekdays_only = self.repeat_days[0..5].iter().all(|&d| d)
            && !self.repeat_days[5]
            && !self.repeat_days[6];
        if weekdays_only {
            return "Weekdays".to_string();
        }
        // Check for weekends only
        let weekends_only = !self.repeat_days[0..5].iter().any(|&d| d)
            && self.repeat_days[5]
            && self.repeat_days[6];
        if weekends_only {
            return "Weekends".to_string();
        }
        // List individual days
        let days: Vec<&str> = Weekday::all()
            .iter()
            .filter(|d| self.repeats_on(**d))
            .map(|d| d.short_name())
            .collect();
        days.join(", ")
    }

    /// Format the alarm time for display.
    pub fn format_time(&self, format: TimeFormat) -> String {
        let (display_hour, period) = format.format_hour(self.hour);
        match period {
            Some(p) => format!("{}:{:02} {}", display_hour, self.minute, p),
            None => format!("{:02}:{:02}", display_hour, self.minute),
        }
    }

    /// Initiate snooze for this alarm.
    pub fn snooze(&mut self) {
        self.ringing = false;
        self.snoozed_remaining = Some(self.snooze_duration.as_seconds());
    }

    /// Dismiss this alarm (stop ringing, clear snooze).
    pub fn dismiss(&mut self) {
        self.ringing = false;
        self.snoozed_remaining = None;
    }

    /// Advance the snooze countdown by one second. Returns true if snooze ended.
    pub fn tick_snooze(&mut self) -> bool {
        if let Some(ref mut remaining) = self.snoozed_remaining {
            if *remaining == 0 {
                self.snoozed_remaining = None;
                self.ringing = true;
                return true;
            }
            *remaining = remaining.saturating_sub(1);
        }
        false
    }

    /// Calculate minutes until this alarm fires, given current hour/minute and
    /// optional current weekday index (0=Mon). Returns `None` if alarm is
    /// disabled or currently ringing/snoozed.
    pub fn minutes_until(
        &self,
        current_hour: u8,
        current_minute: u8,
        current_weekday_idx: Option<usize>,
    ) -> Option<u32> {
        if !self.enabled || self.ringing || self.snoozed_remaining.is_some() {
            return None;
        }
        /// Minutes in a day.
        const DAY: u32 = 24 * 60;

        // Saturating throughout. Every value here is bounded by construction —
        // `hour` is 0..=23 and `minute` 0..=59, both clamped by `Alarm::new` —
        // but the arguments are `u8`s a caller supplies, and a wrapped
        // subtraction would put the next alarm 71 million minutes away rather
        // than report an out-of-range input.
        let minutes_of =
            |h: u8, m: u8| u32::from(h).saturating_mul(60).saturating_add(u32::from(m));
        let alarm_mins = minutes_of(self.hour, self.minute);
        let current_mins = minutes_of(current_hour, current_minute);

        if !self.is_repeating() {
            // One-shot alarm: fires later today, or tomorrow if time passed.
            return Some(if alarm_mins > current_mins {
                alarm_mins.saturating_sub(current_mins)
            } else {
                DAY.saturating_sub(current_mins).saturating_add(alarm_mins)
            });
        }

        // Repeating alarm: find the nearest enabled day.
        let wd = current_weekday_idx.unwrap_or(0);
        for offset in 0u32..8 {
            let day_idx = wd.saturating_add(offset as usize) % 7;
            if !self.repeat_days.get(day_idx).copied().unwrap_or(false) {
                continue;
            }
            if offset == 0 && alarm_mins > current_mins {
                return Some(alarm_mins.saturating_sub(current_mins));
            } else if offset > 0 {
                return Some(
                    offset
                        .saturating_mul(DAY)
                        .saturating_add(alarm_mins)
                        .saturating_sub(current_mins),
                );
            }
            // offset == 0 but alarm_mins <= current_mins: check subsequent days.
        }
        // Fallback — next week same day.
        Some(
            DAY.saturating_mul(7)
                .saturating_sub(current_mins)
                .saturating_add(alarm_mins),
        )
    }

    /// How tall this alarm's card is.
    ///
    /// A ringing or snoozed alarm grows a strip for `Snooze`/`Dismiss`. The list
    /// asks each card its own height rather than assuming a constant, because a
    /// list that steps by a constant while one row is taller draws the rest of
    /// the rows on top of it — and the buttons that appear at exactly the moment
    /// the user needs them would be the ones buried.
    #[must_use]
    pub fn card_height(&self) -> f32 {
        if self.ringing || self.snoozed_remaining.is_some() {
            ALARM_ROW_H + 34.0
        } else {
            ALARM_ROW_H
        }
    }

    /// Draw this alarm's card into `f`, recording its controls.
    pub fn draw(&self, f: &mut Frame, x: f32, y: f32, width: f32, format: TimeFormat) {
        let height = self.card_height();
        let bg_color = if self.ringing {
            Color::rgba(RED.r, RED.g, RED.b, 40)
        } else if self.snoozed_remaining.is_some() {
            Color::rgba(YELLOW.r, YELLOW.g, YELLOW.b, 30)
        } else {
            SURFACE0
        };

        let card = Rect::new(x, y, width, height);
        fill(f, card, bg_color, 8.0);
        // The card itself first, so the controls drawn on top of it below win
        // the hit test — `hit_test` reads back to front.
        f.hit(Target::AlarmRow(self.id), card);

        // Time display.
        let time_color = if self.enabled { TEXT_COLOR } else { OVERLAY0 };
        text(
            f,
            x + PADDING,
            y + 12.0,
            self.format_time(format),
            time_color,
            28.0,
            FontWeightHint::Bold,
            width * 0.6,
        );

        // Label.
        if !self.label.is_empty() {
            text(
                f,
                x + PADDING,
                y + 46.0,
                self.label.clone(),
                SUBTEXT0,
                13.0,
                FontWeightHint::Regular,
                width * 0.5,
            );
        }

        // Repeat summary.
        text(
            f,
            x + PADDING,
            y + 46.0 + if self.label.is_empty() { 0.0 } else { 16.0 },
            self.repeat_summary(),
            OVERLAY1,
            11.0,
            FontWeightHint::Regular,
            width * 0.5,
        );

        // Delete cross, top right.
        let delete = Rect::new(x + width - 30.0, y + 8.0, 22.0, 22.0);
        text_centred(
            f,
            delete.x,
            delete.y + 4.0,
            delete.w,
            "\u{2715}",
            OVERLAY0,
            13.0,
            FontWeightHint::Regular,
        );
        f.hit(Target::AlarmDelete(self.id), delete);

        // Enable/disable pill.
        let toggle = Rect::new(x + width - 62.0, y + 38.0, 44.0, 24.0);
        let toggle_color = if self.enabled { BLUE } else { SURFACE2 };
        fill(f, toggle, toggle_color, toggle.h / 2.0);
        let knob_d = toggle.h - 6.0;
        let knob_x = if self.enabled {
            toggle.x + toggle.w - knob_d - 3.0
        } else {
            toggle.x + 3.0
        };
        fill(
            f,
            Rect::new(knob_x, toggle.y + 3.0, knob_d, knob_d),
            TEXT_COLOR,
            knob_d / 2.0,
        );
        f.hit(Target::AlarmToggle(self.id), toggle);

        // The ringing/snoozed strip.
        if self.ringing || self.snoozed_remaining.is_some() {
            let strip_y = y + ALARM_ROW_H;
            if let Some(remaining) = self.snoozed_remaining {
                // `remaining` is the count still to run, so the user is told how
                // long they have — not how long they asked for, which they
                // already know.
                text(
                    f,
                    x + PADDING,
                    strip_y + 6.0,
                    format!(
                        "Snoozed — {} left",
                        format_duration_hms(clamp_u32(remaining))
                    ),
                    YELLOW,
                    12.0,
                    FontWeightHint::Regular,
                    width * 0.5,
                );
            }
            let btn_w = 84.0f32.min((width - PADDING * 2.0 - 8.0) / 2.0);
            let btn_h = 26.0;
            let mut btn_x = x + width - PADDING - btn_w;
            button(
                f,
                Rect::new(btn_x, strip_y + 2.0, btn_w, btn_h),
                "Dismiss",
                SURFACE1,
                TEXT_COLOR,
                Target::AlarmDismiss(self.id),
            );
            if self.ringing {
                btn_x -= btn_w + 8.0;
                button(
                    f,
                    Rect::new(btn_x, strip_y + 2.0, btn_w, btn_h),
                    "Snooze",
                    BLUE,
                    CRUST,
                    Target::AlarmSnooze(self.id),
                );
            }
        }
    }
}

/// Narrow a `u64` second count to the `u32` the duration formatter takes.
///
/// Saturating rather than `as`: a snooze cannot be 136 years, but a wrapped
/// count would display as a plausible-looking small number, which is worse than
/// an obviously-pegged one.
fn clamp_u32(seconds: u64) -> u32 {
    u32::try_from(seconds).unwrap_or(u32::MAX)
}

// ============================================================================
// Timer
// ============================================================================

/// Unique identifier for a timer instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(pub u64);

/// Timer state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerState {
    /// Not started yet (showing preset/custom input).
    Idle,
    /// Counting down.
    Running,
    /// Paused mid-countdown.
    Paused,
    /// Timer has reached zero.
    Finished,
}

/// A single countdown timer.
#[derive(Clone, Debug)]
pub struct Timer {
    pub id: TimerId,
    pub label: String,
    pub total_seconds: u32,
    pub remaining_seconds: u32,
    pub state: TimerState,
}

impl Timer {
    pub fn new(id: TimerId, total_seconds: u32) -> Self {
        Self {
            id,
            label: String::new(),
            total_seconds,
            remaining_seconds: total_seconds,
            state: TimerState::Idle,
        }
    }

    pub fn with_label(mut self, label: &str) -> Self {
        self.label = label.to_string();
        self
    }

    /// Start or resume the timer.
    pub fn start(&mut self) {
        if self.state == TimerState::Idle || self.state == TimerState::Paused {
            self.state = TimerState::Running;
        }
    }

    /// Pause the timer.
    pub fn pause(&mut self) {
        if self.state == TimerState::Running {
            self.state = TimerState::Paused;
        }
    }

    /// Reset the timer to its original duration.
    pub fn reset(&mut self) {
        self.remaining_seconds = self.total_seconds;
        self.state = TimerState::Idle;
    }

    /// Advance the timer by one second. Returns true if the timer just finished.
    pub fn tick(&mut self) -> bool {
        if self.state != TimerState::Running {
            return false;
        }
        if self.remaining_seconds == 0 {
            self.state = TimerState::Finished;
            return true;
        }
        self.remaining_seconds = self.remaining_seconds.saturating_sub(1);
        if self.remaining_seconds == 0 {
            self.state = TimerState::Finished;
            return true;
        }
        false
    }

    /// Progress fraction (0.0 = just started, 1.0 = finished).
    pub fn progress(&self) -> f32 {
        if self.total_seconds == 0 {
            return 1.0;
        }
        let elapsed = self.total_seconds.saturating_sub(self.remaining_seconds);
        elapsed as f32 / self.total_seconds as f32
    }

    /// Format remaining time as HH:MM:SS or MM:SS.
    pub fn format_remaining(&self) -> String {
        format_duration_hms(self.remaining_seconds)
    }

    /// Format total time as HH:MM:SS or MM:SS.
    pub fn format_total(&self) -> String {
        format_duration_hms(self.total_seconds)
    }

    /// The action the start/pause button on this timer's card performs.
    ///
    /// One button, four meanings — Start, Pause, Resume, Restart — named here so
    /// the label the user reads and the effect a click has come from the same
    /// `match`. Two `match`es over `state` is how a button comes to say "Pause"
    /// and start the timer.
    #[must_use]
    pub fn toggle_label(&self) -> &'static str {
        match self.state {
            TimerState::Idle => "Start",
            TimerState::Running => "Pause",
            TimerState::Paused => "Resume",
            TimerState::Finished => "Restart",
        }
    }

    /// Apply what [`toggle_label`](Self::toggle_label) promises.
    pub fn toggle(&mut self) {
        match self.state {
            TimerState::Idle | TimerState::Paused => self.start(),
            TimerState::Running => self.pause(),
            // A finished timer's button restarts it from the top, because the
            // alternative — start a timer already at zero — finishes again on
            // the next tick and looks like the button did nothing.
            TimerState::Finished => {
                self.reset();
                self.start();
            }
        }
    }

    /// Draw this timer's card into `f`, recording its controls.
    pub fn draw(&self, f: &mut Frame, x: f32, y: f32, width: f32) {
        let bg_color = match self.state {
            TimerState::Finished => Color::rgba(RED.r, RED.g, RED.b, 40),
            TimerState::Paused => Color::rgba(YELLOW.r, YELLOW.g, YELLOW.b, 20),
            TimerState::Running | TimerState::Idle => SURFACE0,
        };

        let card = Rect::new(x, y, width, TIMER_ROW_H);
        fill(f, card, bg_color, 8.0);
        f.hit(Target::TimerRow(self.id), card);

        // Timer label.
        if !self.label.is_empty() {
            text(
                f,
                x + PADDING,
                y + 8.0,
                self.label.clone(),
                SUBTEXT0,
                13.0,
                FontWeightHint::Regular,
                width - PADDING * 2.0,
            );
        }

        let text_y = if self.label.is_empty() {
            y + 16.0
        } else {
            y + 28.0
        };

        // Remaining time.
        let time_color = match self.state {
            TimerState::Finished => RED,
            TimerState::Paused => YELLOW,
            TimerState::Running | TimerState::Idle => TEXT_COLOR,
        };
        text(
            f,
            x + PADDING,
            text_y,
            self.format_remaining(),
            time_color,
            32.0,
            FontWeightHint::Bold,
            width * 0.5,
        );

        // Total time indicator.
        text(
            f,
            x + PADDING,
            text_y + 40.0,
            format!("of {}", self.format_total()),
            OVERLAY0,
            12.0,
            FontWeightHint::Regular,
            width * 0.4,
        );

        // Progress bar.
        let bar = Rect::new(x + PADDING, text_y + 60.0, width - PADDING * 2.0, 6.0);
        fill(f, bar, SURFACE2, 3.0);
        let fill_w = bar.w * self.progress();
        let fill_color = match self.state {
            TimerState::Finished => RED,
            TimerState::Paused => YELLOW,
            TimerState::Running | TimerState::Idle => BLUE,
        };
        if fill_w > 0.0 {
            fill(f, Rect::new(bar.x, bar.y, fill_w, bar.h), fill_color, 3.0);
        }

        // State badge.
        let badge_text = match self.state {
            TimerState::Idle => "READY",
            TimerState::Running => "RUNNING",
            TimerState::Paused => "PAUSED",
            TimerState::Finished => "DONE",
        };
        let badge_color = match self.state {
            TimerState::Idle => OVERLAY0,
            TimerState::Running => GREEN,
            TimerState::Paused => YELLOW,
            TimerState::Finished => RED,
        };
        text(
            f,
            x + width - 84.0,
            y + 10.0,
            badge_text,
            badge_color,
            11.0,
            FontWeightHint::Bold,
            76.0,
        );

        // Controls, right-hand column: Start/Pause over Reset, with the delete
        // cross below both.
        let delete = Rect::new(x + width - 30.0, y + TIMER_ROW_H - 30.0, 22.0, 22.0);
        text_centred(
            f,
            delete.x,
            delete.y + 4.0,
            delete.w,
            "\u{2715}",
            OVERLAY0,
            13.0,
            FontWeightHint::Regular,
        );
        f.hit(Target::TimerDelete(self.id), delete);

        let btn_w = 80.0f32.min(width * 0.35);
        let btn_x = x + width - PADDING - btn_w;
        button(
            f,
            Rect::new(btn_x, y + 30.0, btn_w, 28.0),
            self.toggle_label(),
            BLUE,
            CRUST,
            Target::TimerToggle(self.id),
        );
        button(
            f,
            Rect::new(btn_x, y + 64.0, btn_w, 28.0),
            "Reset",
            SURFACE1,
            TEXT_COLOR,
            Target::TimerReset(self.id),
        );
    }
}

// ============================================================================
// Progress ring rendering
// ============================================================================

/// Generate render commands for a circular progress ring.
/// `progress` ranges from 0.0 (empty) to 1.0 (full).
pub fn render_progress_ring(
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    progress: f32,
    track_color: Color,
    fill_color: Color,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    let progress = progress.clamp(0.0, 1.0);

    // Draw the track (full circle) as line segments.
    for i in 0..RING_SEGMENTS {
        let angle0 = 2.0 * core::f32::consts::PI * (i as f32) / (RING_SEGMENTS as f32);
        let angle1 =
            2.0 * core::f32::consts::PI * (i.saturating_add(1) as f32) / (RING_SEGMENTS as f32);
        cmds.push(RenderCommand::Line {
            x1: cx + radius * angle0.cos(),
            y1: cy + radius * angle0.sin(),
            x2: cx + radius * angle1.cos(),
            y2: cy + radius * angle1.sin(),
            color: track_color,
            width: thickness,
        });
    }

    // Draw the filled portion.
    let filled_segments = (progress * RING_SEGMENTS as f32) as usize;
    // Start from top (-PI/2 offset).
    let offset = -core::f32::consts::FRAC_PI_2;
    for i in 0..filled_segments {
        let angle0 = offset + 2.0 * core::f32::consts::PI * (i as f32) / (RING_SEGMENTS as f32);
        let angle1 = offset
            + 2.0 * core::f32::consts::PI * (i.saturating_add(1) as f32) / (RING_SEGMENTS as f32);
        cmds.push(RenderCommand::Line {
            x1: cx + radius * angle0.cos(),
            y1: cy + radius * angle0.sin(),
            x2: cx + radius * angle1.cos(),
            y2: cy + radius * angle1.sin(),
            color: fill_color,
            width: thickness,
        });
    }

    cmds
}

// ============================================================================
// Stopwatch
// ============================================================================

/// Stopwatch state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopwatchState {
    Stopped,
    Running,
    Paused,
}

/// A single lap record.
#[derive(Clone, Debug, PartialEq)]
pub struct Lap {
    pub number: u32,
    /// Lap split time in milliseconds (time since previous lap or start).
    pub split_ms: u64,
    /// Cumulative elapsed time in milliseconds.
    pub elapsed_ms: u64,
}

impl Lap {
    pub fn format_split(&self) -> String {
        format_duration_ms(self.split_ms)
    }

    pub fn format_elapsed(&self) -> String {
        format_duration_ms(self.elapsed_ms)
    }
}

/// Stopwatch with lap tracking.
#[derive(Clone, Debug)]
pub struct Stopwatch {
    pub state: StopwatchState,
    /// Total elapsed milliseconds.
    pub elapsed_ms: u64,
    /// Milliseconds at the start of the current running segment.
    pub segment_start_ms: u64,
    /// Recorded laps.
    pub laps: Vec<Lap>,
}

impl Stopwatch {
    pub fn new() -> Self {
        Self {
            state: StopwatchState::Stopped,
            elapsed_ms: 0,
            segment_start_ms: 0,
            laps: Vec::new(),
        }
    }

    /// Start or resume.
    pub fn start(&mut self) {
        match self.state {
            StopwatchState::Stopped | StopwatchState::Paused => {
                self.segment_start_ms = self.elapsed_ms;
                self.state = StopwatchState::Running;
            }
            StopwatchState::Running => {}
        }
    }

    /// Pause.
    pub fn pause(&mut self) {
        if self.state == StopwatchState::Running {
            self.state = StopwatchState::Paused;
        }
    }

    /// Reset everything.
    pub fn reset(&mut self) {
        self.state = StopwatchState::Stopped;
        self.elapsed_ms = 0;
        self.segment_start_ms = 0;
        self.laps.clear();
    }

    /// Advance by `delta_ms` milliseconds.
    pub fn tick(&mut self, delta_ms: u64) {
        if self.state == StopwatchState::Running {
            self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
        }
    }

    /// Record a lap.
    pub fn lap(&mut self) {
        if self.state != StopwatchState::Running {
            return;
        }
        let split = self.elapsed_ms.saturating_sub(self.segment_start_ms);
        let number = (self.laps.len() as u32).saturating_add(1);
        self.laps.push(Lap {
            number,
            split_ms: split,
            elapsed_ms: self.elapsed_ms,
        });
        self.segment_start_ms = self.elapsed_ms;
    }

    /// Format the total elapsed time for display.
    pub fn format_elapsed(&self) -> String {
        format_duration_ms(self.elapsed_ms)
    }

    /// Compute lap statistics (best, worst, average split times).
    /// Returns (best_ms, worst_ms, average_ms) or None if no laps.
    pub fn lap_stats(&self) -> Option<LapStats> {
        if self.laps.is_empty() {
            return None;
        }
        let mut best = u64::MAX;
        let mut worst = 0u64;
        let mut total = 0u64;
        for lap in &self.laps {
            if lap.split_ms < best {
                best = lap.split_ms;
            }
            if lap.split_ms > worst {
                worst = lap.split_ms;
            }
            total = total.saturating_add(lap.split_ms);
        }
        let avg = total.checked_div(self.laps.len() as u64).unwrap_or(0);
        Some(LapStats {
            best_ms: best,
            worst_ms: worst,
            average_ms: avg,
            count: self.laps.len(),
        })
    }

    /// The action the start/pause button performs, and the word on it.
    #[must_use]
    pub fn toggle_label(&self) -> &'static str {
        match self.state {
            StopwatchState::Running => "Pause",
            StopwatchState::Paused => "Resume",
            StopwatchState::Stopped => "Start",
        }
    }

    /// Apply what [`toggle_label`](Self::toggle_label) promises.
    pub fn toggle(&mut self) {
        match self.state {
            StopwatchState::Running => self.pause(),
            StopwatchState::Paused | StopwatchState::Stopped => self.start(),
        }
    }

    /// Height of the lap table's content, which is what the pane scrolls over.
    #[must_use]
    pub fn lap_content_height(&self) -> f32 {
        self.laps.len() as f32 * LAP_ROW_H
    }

    /// Where the lap table starts, measured from the top of the tab's content
    /// area. The scroll clamp and the drawing both need it, and a second copy
    /// is a second thing to get wrong.
    pub const LAP_TABLE_TOP: f32 = 190.0;

    /// Draw the whole stopwatch tab into `f`.
    ///
    /// Takes the pane's height as well as its width because the lap table is
    /// scrolled and clipped: a lap that has scrolled out of the pane must not be
    /// drawn over the buttons above it, and — since the frame trims hit boxes to
    /// the clip — must not be clickable either.
    pub fn draw(&self, f: &mut Frame, x: f32, y: f32, width: f32, height: f32, scroll: f32) {
        // Elapsed time — large display.
        let time_color = match self.state {
            StopwatchState::Running => GREEN,
            StopwatchState::Paused => YELLOW,
            StopwatchState::Stopped => TEXT_COLOR,
        };
        text_centred(
            f,
            x,
            y + 16.0,
            width,
            &self.format_elapsed(),
            time_color,
            48.0,
            FontWeightHint::Bold,
        );

        // State indicator.
        let state_text = match self.state {
            StopwatchState::Running => "RUNNING",
            StopwatchState::Paused => "PAUSED",
            StopwatchState::Stopped => "STOPPED",
        };
        text_centred(
            f,
            x,
            y + 78.0,
            width,
            state_text,
            OVERLAY0,
            12.0,
            FontWeightHint::Regular,
        );

        // Controls.
        let gap = 8.0;
        let btn_w = ((width - gap * 2.0) / 3.0).max(48.0);
        let btn_y = y + 100.0;
        let btn_h = 34.0;
        button(
            f,
            Rect::new(x, btn_y, btn_w, btn_h),
            self.toggle_label(),
            BLUE,
            CRUST,
            Target::SwToggle,
        );
        // Lap is only meaningful while running — a lap of a stopped stopwatch
        // would be a zero-length split. Drawn dimmed rather than hidden, so the
        // row of three buttons does not reflow under the pointer.
        let lap_live = self.state == StopwatchState::Running;
        button(
            f,
            Rect::new(x + btn_w + gap, btn_y, btn_w, btn_h),
            "Lap",
            if lap_live { SURFACE1 } else { SURFACE0 },
            if lap_live { TEXT_COLOR } else { OVERLAY0 },
            Target::SwLap,
        );
        button(
            f,
            Rect::new(x + (btn_w + gap) * 2.0, btn_y, btn_w, btn_h),
            "Reset",
            SURFACE1,
            TEXT_COLOR,
            Target::SwReset,
        );

        // Lap stats.
        if let Some(stats) = self.lap_stats() {
            text(
                f,
                x,
                y + 146.0,
                format!(
                    "Best: {}  Worst: {}  Avg: {}  ({} laps)",
                    format_duration_ms(stats.best_ms),
                    format_duration_ms(stats.worst_ms),
                    format_duration_ms(stats.average_ms),
                    stats.count,
                ),
                SUBTEXT0,
                12.0,
                FontWeightHint::Regular,
                width,
            );
        }

        if self.laps.is_empty() {
            return;
        }

        // Lap table header.
        let table_y = y + Self::LAP_TABLE_TOP - 24.0;
        f.push(RenderCommand::Line {
            x1: x,
            y1: table_y,
            x2: x + width,
            y2: table_y,
            color: SURFACE2,
            width: 1.0,
        });

        let col_num_x = x;
        let col_split_x = x + width * 0.28;
        let col_elapsed_x = x + width * 0.62;
        for (cx, title, w) in [
            (col_num_x, "Lap", width * 0.26),
            (col_split_x, "Split", width * 0.32),
            (col_elapsed_x, "Elapsed", width * 0.36),
        ] {
            text(
                f,
                cx,
                table_y + 4.0,
                title,
                OVERLAY1,
                12.0,
                FontWeightHint::Bold,
                w,
            );
        }

        // Lap rows, most recent first, inside a scrolled and clipped pane.
        let pane = Rect::new(
            x,
            y + Self::LAP_TABLE_TOP,
            width,
            (height - Self::LAP_TABLE_TOP).max(0.0),
        );
        f.clip(pane);
        f.translate(0.0, -scroll);
        let stats = self.lap_stats();
        for (i, lap) in self.laps.iter().rev().enumerate() {
            let row_y = pane.y + (i as f32) * LAP_ROW_H;
            // A clip hides an overrun from the eye, but not from the frame.
            // This app draws its runs through its own `text` helper, which
            // pushes a `RenderCommand::Text` whether or not the clip in force
            // could show it -- so without this test a lap four hundred points
            // below the table still enters the picture, claiming to be a label
            // that is on screen, and a reader of the picture is told a lie.
            // A row is drawn whole or not at all, so the comparison is against
            // the row's own edges in the scrolled space; `i` only increases, so
            // the first row past the bottom ends the walk.
            if row_y - scroll >= pane.bottom() {
                break;
            }
            if row_y - scroll + LAP_ROW_H <= pane.y {
                continue;
            }
            // The best and worst laps are only worth colouring once there is
            // something to compare against; with one lap it is both, and
            // painting it green and red at once says nothing.
            let split_color = match stats {
                Some(ref s) if self.laps.len() > 1 && lap.split_ms == s.best_ms => GREEN,
                Some(ref s) if self.laps.len() > 1 && lap.split_ms == s.worst_ms => RED,
                _ => TEXT_COLOR,
            };
            text(
                f,
                col_num_x,
                row_y,
                format!("#{}", lap.number),
                SUBTEXT0,
                12.0,
                FontWeightHint::Regular,
                width * 0.26,
            );
            text(
                f,
                col_split_x,
                row_y,
                lap.format_split(),
                split_color,
                12.0,
                FontWeightHint::Regular,
                width * 0.32,
            );
            text(
                f,
                col_elapsed_x,
                row_y,
                lap.format_elapsed(),
                SUBTEXT0,
                12.0,
                FontWeightHint::Regular,
                width * 0.36,
            );
        }
        f.untranslate();
        f.unclip();
    }
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Lap statistics summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LapStats {
    pub best_ms: u64,
    pub worst_ms: u64,
    pub average_ms: u64,
    pub count: usize,
}

// ============================================================================
// Alarm editor
// ============================================================================

/// The alarm being composed, held apart from the alarm it will become.
///
/// A user who opens an existing alarm, changes the hour and then presses
/// Cancel must get the alarm back unchanged — which is only possible if the
/// edits were never applied to it in the first place. So the editor is a
/// separate value that is copied *out of* an alarm on open and *into* it on
/// save, rather than a set of flags pointing at the live one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlarmEditor {
    pub hour: u8,
    pub minute: u8,
    pub label: String,
    pub repeat_days: [bool; 7],
    pub sound: AlarmSound,
    /// Index into [`SNOOZE_OPTIONS`], cycled by the snooze row.
    pub snooze_index: usize,
    /// The alarm this will overwrite, or `None` for one that does not exist
    /// yet. Held as an `AlarmId` and not an index, so deleting some *other*
    /// alarm while the editor is open cannot redirect the save onto a
    /// neighbour.
    pub editing: Option<AlarmId>,
}

impl AlarmEditor {
    /// An editor for a brand-new alarm at `hour:minute`.
    #[must_use]
    pub fn new_alarm(hour: u8, minute: u8) -> Self {
        Self {
            hour: hour.min(23),
            minute: minute.min(59),
            label: String::new(),
            repeat_days: [false; 7],
            sound: AlarmSound::default(),
            snooze_index: 0,
            editing: None,
        }
    }

    /// An editor loaded from an existing alarm.
    #[must_use]
    pub fn from_alarm(alarm: &Alarm) -> Self {
        Self {
            hour: alarm.hour,
            minute: alarm.minute,
            label: alarm.label.clone(),
            repeat_days: alarm.repeat_days,
            sound: alarm.sound,
            snooze_index: SNOOZE_OPTIONS
                .iter()
                .position(|&m| m == alarm.snooze_duration.minutes)
                .unwrap_or(0),
            editing: Some(alarm.id),
        }
    }

    /// The snooze duration the index names.
    #[must_use]
    pub fn snooze(&self) -> SnoozeDuration {
        SnoozeDuration::new(
            SNOOZE_OPTIONS
                .get(self.snooze_index)
                .copied()
                .unwrap_or(SNOOZE_OPTIONS[0]),
        )
    }

    /// Step the hour, wrapping at both ends.
    pub fn step_hour(&mut self, step: Step) {
        self.hour = wrap_step(self.hour, step, 24);
    }

    /// Step the minute, wrapping at both ends.
    pub fn step_minute(&mut self, step: Step) {
        self.minute = wrap_step(self.minute, step, 60);
    }

    /// Copy the editor's fields onto an alarm, leaving its runtime state
    /// (`enabled`, `ringing`, `snoozed_remaining`) alone — those belong to the
    /// alarm, not to the form.
    pub fn apply_to(&self, alarm: &mut Alarm) {
        alarm.hour = self.hour;
        alarm.minute = self.minute;
        alarm.label.clone_from(&self.label);
        alarm.repeat_days = self.repeat_days;
        alarm.sound = self.sound;
        alarm.snooze_duration = self.snooze();
    }
}

/// Add or subtract one, wrapping within `0..modulus`.
///
/// `rem_euclid` and not `%`: stepping 0 down has to reach `modulus - 1`, and
/// `-1 % 24` is `-1` in Rust, which is not an hour.
fn wrap_step(value: u8, step: Step, modulus: i32) -> u8 {
    let delta = match step {
        Step::Up => 1,
        Step::Down => -1,
    };
    let next = i32::from(value).saturating_add(delta).rem_euclid(modulus);
    u8::try_from(next).unwrap_or(0)
}

// ============================================================================
// Application state
// ============================================================================

/// Top-level application state.
pub struct AlarmClockApp {
    pub active_tab: ActiveTab,
    pub time_format: TimeFormat,
    pub alarms: Vec<Alarm>,
    pub timers: Vec<Timer>,
    pub stopwatch: Stopwatch,
    next_alarm_id: u64,
    next_timer_id: u64,
    /// Current time for display (hour, minute, second).
    pub current_time: (u8, u8, u8),
    /// Current weekday index (0=Mon).
    pub current_weekday: usize,
    /// The size the window was last drawn at.
    ///
    /// Kept because a click arrives without one: [`handle_click`] has to
    /// re-draw the frame to hit-test against it, and drawing it at the default
    /// size would test against a layout the user is not looking at.
    ///
    /// [`handle_click`]: AlarmClockApp::handle_click
    pub window_size: (f32, f32),
    /// Scroll offsets in pixels for the alarm list, the timer list and the lap
    /// table. Pixels rather than row indices because the alarm rows are not all
    /// the same height — a ringing one grows a button strip.
    pub alarm_scroll: f32,
    pub timer_scroll: f32,
    pub lap_scroll: f32,
    /// The open alarm editor, if there is one.
    pub editor: Option<AlarmEditor>,
    /// What has the keyboard.
    pub focus: Option<Focus>,
    /// The three custom-duration entry buffers, indexed by [`HmsField::index`].
    pub custom: [String; 3],
    /// Milliseconds banked toward the next whole second.
    ///
    /// The alarms and the timers advance once per second, but ticks arrive
    /// every 50 ms while the stopwatch runs. Banking the remainder is what
    /// keeps a one-second timer taking one second at both tick rates; a
    /// handler that fired the per-second work on every tick would run a
    /// countdown ten times too fast whenever the stopwatch happened to be on.
    tick_accum_ms: u64,
}

impl AlarmClockApp {
    pub fn new() -> Self {
        Self {
            active_tab: ActiveTab::default(),
            time_format: TimeFormat::default(),
            alarms: Vec::new(),
            timers: Vec::new(),
            stopwatch: Stopwatch::new(),
            next_alarm_id: 1,
            next_timer_id: 1,
            current_time: (0, 0, 0),
            current_weekday: 0,
            window_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            alarm_scroll: 0.0,
            timer_scroll: 0.0,
            lap_scroll: 0.0,
            editor: None,
            focus: None,
            custom: [String::new(), String::new(), String::new()],
            tick_accum_ms: 0,
        }
    }

    // ---- Alarm management ----

    /// Create a new alarm and return its ID.
    pub fn create_alarm(&mut self, hour: u8, minute: u8) -> AlarmId {
        let id = AlarmId(self.next_alarm_id);
        self.next_alarm_id = self.next_alarm_id.saturating_add(1);
        self.alarms.push(Alarm::new(id, hour, minute));
        id
    }

    /// Create a new alarm with a label and return its ID.
    pub fn create_alarm_with_label(&mut self, hour: u8, minute: u8, label: &str) -> AlarmId {
        let id = self.create_alarm(hour, minute);
        if let Some(alarm) = self.find_alarm_mut(id) {
            alarm.label = label.to_string();
        }
        id
    }

    /// Find an alarm by ID.
    pub fn find_alarm(&self, id: AlarmId) -> Option<&Alarm> {
        self.alarms.iter().find(|a| a.id == id)
    }

    /// Find an alarm by ID (mutable).
    pub fn find_alarm_mut(&mut self, id: AlarmId) -> Option<&mut Alarm> {
        self.alarms.iter_mut().find(|a| a.id == id)
    }

    /// Delete an alarm by ID. Returns true if found and removed.
    pub fn delete_alarm(&mut self, id: AlarmId) -> bool {
        let len_before = self.alarms.len();
        self.alarms.retain(|a| a.id != id);
        self.alarms.len() < len_before
    }

    /// Toggle an alarm's enabled state.
    pub fn toggle_alarm(&mut self, id: AlarmId) -> Option<bool> {
        if let Some(alarm) = self.find_alarm_mut(id) {
            alarm.enabled = !alarm.enabled;
            Some(alarm.enabled)
        } else {
            None
        }
    }

    /// Get the next alarm to fire and its minutes-until value.
    pub fn next_alarm(&self) -> Option<(&Alarm, u32)> {
        let (hour, minute, _) = self.current_time;
        let mut best: Option<(&Alarm, u32)> = None;
        for alarm in &self.alarms {
            if let Some(mins) = alarm.minutes_until(hour, minute, Some(self.current_weekday)) {
                match best {
                    None => best = Some((alarm, mins)),
                    Some((_, best_mins)) if mins < best_mins => best = Some((alarm, mins)),
                    _ => {}
                }
            }
        }
        best
    }

    /// Snooze an alarm by ID.
    pub fn snooze_alarm(&mut self, id: AlarmId) {
        if let Some(alarm) = self.find_alarm_mut(id) {
            alarm.snooze();
        }
    }

    /// Dismiss an alarm by ID.
    pub fn dismiss_alarm(&mut self, id: AlarmId) {
        if let Some(alarm) = self.find_alarm_mut(id) {
            alarm.dismiss();
        }
    }

    // ---- Timer management ----

    /// Create a new timer with the given total seconds. Returns the timer ID.
    pub fn create_timer(&mut self, total_seconds: u32) -> TimerId {
        let id = TimerId(self.next_timer_id);
        self.next_timer_id = self.next_timer_id.saturating_add(1);
        self.timers.push(Timer::new(id, total_seconds));
        id
    }

    /// Create a timer from a preset (minutes).
    pub fn create_timer_preset(&mut self, minutes: u32) -> TimerId {
        self.create_timer(minutes.saturating_mul(60))
    }

    /// Create a timer with custom hours:minutes:seconds.
    pub fn create_timer_hms(&mut self, hours: u32, minutes: u32, seconds: u32) -> TimerId {
        let total = hours
            .saturating_mul(3600)
            .saturating_add(minutes.saturating_mul(60))
            .saturating_add(seconds);
        self.create_timer(total)
    }

    /// Find a timer by ID.
    pub fn find_timer(&self, id: TimerId) -> Option<&Timer> {
        self.timers.iter().find(|t| t.id == id)
    }

    /// Find a timer by ID (mutable).
    pub fn find_timer_mut(&mut self, id: TimerId) -> Option<&mut Timer> {
        self.timers.iter_mut().find(|t| t.id == id)
    }

    /// Delete a timer by ID.
    pub fn delete_timer(&mut self, id: TimerId) -> bool {
        let len_before = self.timers.len();
        self.timers.retain(|t| t.id != id);
        self.timers.len() < len_before
    }

    /// Start a timer by ID.
    pub fn start_timer(&mut self, id: TimerId) {
        if let Some(timer) = self.find_timer_mut(id) {
            timer.start();
        }
    }

    /// Pause a timer by ID.
    pub fn pause_timer(&mut self, id: TimerId) {
        if let Some(timer) = self.find_timer_mut(id) {
            timer.pause();
        }
    }

    /// Reset a timer by ID.
    pub fn reset_timer(&mut self, id: TimerId) {
        if let Some(timer) = self.find_timer_mut(id) {
            timer.reset();
        }
    }

    /// Count running timers.
    pub fn running_timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|t| t.state == TimerState::Running)
            .count()
    }

    /// Count finished timers.
    pub fn finished_timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|t| t.state == TimerState::Finished)
            .count()
    }

    // ---- Stopwatch delegation ----

    pub fn stopwatch_start(&mut self) {
        self.stopwatch.start();
    }

    pub fn stopwatch_pause(&mut self) {
        self.stopwatch.pause();
    }

    pub fn stopwatch_reset(&mut self) {
        self.stopwatch.reset();
    }

    pub fn stopwatch_lap(&mut self) {
        self.stopwatch.lap();
    }

    // ---- Time update & ticking ----

    /// Update the current time display.
    pub fn set_current_time(&mut self, hour: u8, minute: u8, second: u8, weekday: usize) {
        self.current_time = (hour.min(23), minute.min(59), second.min(59));
        self.current_weekday = weekday.min(6);
    }

    /// Tick all timers by one second. Returns list of timer IDs that just finished.
    pub fn tick_timers(&mut self) -> Vec<TimerId> {
        let mut finished = Vec::new();
        for timer in &mut self.timers {
            if timer.tick() {
                finished.push(timer.id);
            }
        }
        finished
    }

    /// Tick all alarm snooze countdowns. Returns list of alarm IDs that started ringing.
    pub fn tick_alarm_snoozes(&mut self) -> Vec<AlarmId> {
        let mut ringing = Vec::new();
        for alarm in &mut self.alarms {
            if alarm.tick_snooze() {
                ringing.push(alarm.id);
            }
        }
        ringing
    }

    /// Check if any alarm should trigger at the current time.
    /// Returns IDs of alarms that just started ringing.
    pub fn check_alarm_triggers(&mut self) -> Vec<AlarmId> {
        let (hour, minute, _) = self.current_time;
        let mut triggered = Vec::new();
        for alarm in &mut self.alarms {
            if !alarm.enabled || alarm.ringing || alarm.snoozed_remaining.is_some() {
                continue;
            }
            if alarm.hour == hour && alarm.minute == minute {
                // Check repeat days if applicable.
                if alarm.is_repeating()
                    && !alarm.repeats_on(
                        Weekday::from_index(self.current_weekday).unwrap_or(Weekday::Monday),
                    )
                {
                    continue;
                }
                alarm.ringing = true;
                triggered.push(alarm.id);
            }
        }
        triggered
    }

    // ---- Toggle time format ----

    pub fn toggle_time_format(&mut self) {
        self.time_format = match self.time_format {
            TimeFormat::TwelveHour => TimeFormat::TwentyFourHour,
            TimeFormat::TwentyFourHour => TimeFormat::TwelveHour,
        };
    }

    // ---- Wall clock ----

    /// Set the clock from a UTC instant, in seconds since the epoch.
    ///
    /// Pure, and separated from the wall-clock read on purpose: an alarm that
    /// fires at 07:00 can only be tested if the test can say what time it is.
    /// [`refresh_clock`](Self::refresh_clock) is the one place in this file
    /// that asks the system.
    ///
    /// # Zone
    ///
    /// UTC, said out loud. There is no per-process zone to read yet
    /// (`known-issues.md` -> `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), and every
    /// lane-C surface that renders an instant marks itself with an explicit
    /// `Tz::utc()` so `rg 'Tz::utc' apps/ gui/` finds all of them at once.
    /// Going through `lookup().gmtoff` rather than writing `secs % 86_400` is
    /// what makes this a one-line change when a real zone arrives — and
    /// `% 86_400` is itself the exact bug the taskbar clock shipped: correct
    /// looking, and five hours out.
    pub fn set_time_from_utc(&mut self, utc_secs: i64) {
        let zone = tzrules::Tz::utc();
        let local = utc_secs.saturating_add(i64::from(zone.lookup(utc_secs).gmtoff));

        // `rem_euclid`, not `%`: a pre-1970 instant with `%` gives a negative
        // remainder, which is not a time of day at all.
        let secs_into_day = local.rem_euclid(86_400);
        // Each division is bounded by the line above, so no cast can lose
        // anything: 0..86_400 / 3600 is 0..24 and the remainders are 0..60.
        let (hour, minute, second) = (
            (secs_into_day / 3600) as u8,
            ((secs_into_day / 60) % 60) as u8,
            (secs_into_day % 60) as u8,
        );

        // The civil date comes from `guitk::date`, the toolkit's one calendar,
        // rather than a private day-of-week formula in this file — which is how
        // an alarm set for "Mon" comes to fire on Sunday.
        let day = guitk::date::Date::from_unix_utc(local).weekday();
        self.set_current_time(hour, minute, second, weekday_index(day));
    }

    /// Set the clock from the system's idea of now.
    ///
    /// A clock that cannot be read leaves the previous reading standing rather
    /// than falling back to midnight: a stuck clock is visible, and an alarm
    /// clock that silently reads 00:00 is not.
    pub fn refresh_clock(&mut self) {
        if let Ok(since_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) {
            self.set_time_from_utc(i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX));
        }
    }

    // ---- Editor ----

    /// Open the editor on a new alarm, defaulted to the next whole hour.
    ///
    /// The next hour rather than the current time, because an alarm saved at
    /// the very minute it is set for either fires immediately or waits a whole
    /// day, and neither is what "add an alarm" means.
    pub fn open_new_alarm(&mut self) {
        let (hour, _, _) = self.current_time;
        self.editor = Some(AlarmEditor::new_alarm(hour.wrapping_add(1).min(23), 0));
        self.focus = None;
    }

    /// Open the editor on an existing alarm. No-op if the id is unknown.
    pub fn open_alarm(&mut self, id: AlarmId) {
        if let Some(alarm) = self.find_alarm(id) {
            self.editor = Some(AlarmEditor::from_alarm(alarm));
            self.focus = None;
        }
    }

    /// Commit the open editor, creating or overwriting an alarm.
    ///
    /// Returns the alarm's id, or `None` if no editor was open.
    pub fn save_editor(&mut self) -> Option<AlarmId> {
        let editor = self.editor.take()?;
        self.focus = None;
        let id = match editor.editing {
            Some(id) if self.find_alarm(id).is_some() => id,
            // Either a new alarm, or one that was deleted from under the open
            // editor. Creating it in the second case loses nothing and is far
            // less surprising than a Save that silently does nothing.
            _ => self.create_alarm(editor.hour, editor.minute),
        };
        if let Some(alarm) = self.find_alarm_mut(id) {
            editor.apply_to(alarm);
        }
        Some(id)
    }

    /// Abandon the open editor.
    pub fn cancel_editor(&mut self) {
        self.editor = None;
        self.focus = None;
    }

    // ---- Custom timer entry ----

    /// The duration the three custom fields currently spell, in seconds.
    ///
    /// Empty fields read as zero, so typing only `MM` starts a timer in
    /// minutes. Out-of-range text reads as zero rather than rejecting the whole
    /// entry: the fields are clamped on the way in, so this only fires for a
    /// value programmatically stuffed in.
    #[must_use]
    pub fn custom_seconds(&self) -> u32 {
        let field = |f: HmsField| -> u32 {
            self.custom
                .get(f.index())
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0)
        };
        let hours = field(HmsField::Hours);
        let minutes = field(HmsField::Minutes);
        let seconds = field(HmsField::Seconds);
        hours
            .saturating_mul(3600)
            .saturating_add(minutes.saturating_mul(60))
            .saturating_add(seconds)
    }

    /// Start a timer for whatever the custom fields say, and clear them.
    ///
    /// Returns the new timer's id, or `None` when the fields spell zero — a
    /// zero-second timer would be created already finished, which looks exactly
    /// like the button having malfunctioned.
    pub fn start_custom_timer(&mut self) -> Option<TimerId> {
        let total = self.custom_seconds();
        if total == 0 {
            return None;
        }
        let id = self.create_timer(total);
        self.start_timer(id);
        self.custom = [String::new(), String::new(), String::new()];
        self.focus = None;
        Some(id)
    }

    // ---- Layout ----

    /// The rectangle the active tab draws into.
    #[must_use]
    pub fn content_rect(width: f32, height: f32) -> Rect {
        let top = TAB_BAR_HEIGHT + 8.0;
        Rect::new(
            PADDING,
            top,
            (width - PADDING * 2.0).max(0.0),
            (height - top - PADDING).max(0.0),
        )
    }

    /// The scrolling pane the alarm cards are drawn in.
    fn alarm_list_rect(content: Rect) -> Rect {
        let top = content.y + CLOCK_H + CHIP_GAP + ADD_BUTTON_H + CHIP_GAP;
        Rect::new(
            content.x,
            top,
            content.w,
            (content.y + content.h - top).max(0.0),
        )
    }

    /// The scrolling pane the timer cards are drawn in.
    fn timer_list_rect(content: Rect) -> Rect {
        let top = content.y + preset_block_height() + CHIP_GAP + CUSTOM_H + CHIP_GAP;
        Rect::new(
            content.x,
            top,
            content.w,
            (content.y + content.h - top).max(0.0),
        )
    }

    /// Total height of the alarm cards, gaps included.
    ///
    /// Summed rather than multiplied by a constant because a ringing alarm's
    /// card is taller — see [`Alarm::card_height`].
    #[must_use]
    pub fn alarm_content_height(&self) -> f32 {
        self.alarms
            .iter()
            .map(|a| a.card_height() + ALARM_ROW_GAP)
            .sum()
    }

    /// Total height of the timer cards, gaps included.
    #[must_use]
    pub fn timer_content_height(&self) -> f32 {
        self.timers.len() as f32 * (TIMER_ROW_H + TIMER_ROW_GAP)
    }

    /// Pull every scroll offset back inside its pane.
    ///
    /// Called whenever content is removed as well as when the window resizes:
    /// deleting the last alarm of a scrolled list would otherwise leave the
    /// pane parked past the end, showing nothing, with no way back except the
    /// wheel.
    fn clamp_scrolls(&mut self, width: f32, height: f32) {
        let content = Self::content_rect(width, height);
        let alarms = Self::alarm_list_rect(content);
        self.alarm_scroll = clamp_scroll(self.alarm_scroll, self.alarm_content_height(), alarms.h);
        let timers = Self::timer_list_rect(content);
        self.timer_scroll = clamp_scroll(self.timer_scroll, self.timer_content_height(), timers.h);
        let laps = (content.h - Stopwatch::LAP_TABLE_TOP).max(0.0);
        self.lap_scroll = clamp_scroll(self.lap_scroll, self.stopwatch.lap_content_height(), laps);
    }

    // ---- Rendering ----

    /// Draw the whole window, recording a hit box for every control.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let width = width.max(MIN_WIDTH);
        let height = height.max(MIN_HEIGHT);
        let mut f = Frame::new(width, height);
        fill(&mut f, Rect::new(0.0, 0.0, width, height), BASE, 0.0);
        self.draw_tab_bar(&mut f, width);

        let content = Self::content_rect(width, height);
        match self.active_tab {
            ActiveTab::Alarm => self.draw_alarm_tab(&mut f, content),
            ActiveTab::Timer => self.draw_timer_tab(&mut f, content),
            ActiveTab::Stopwatch => self.stopwatch.draw(
                &mut f,
                content.x,
                content.y,
                content.w,
                content.h,
                self.lap_scroll,
            ),
        }
        f
    }

    /// The three tabs across the top.
    fn draw_tab_bar(&self, f: &mut Frame, width: f32) {
        fill(f, Rect::new(0.0, 0.0, width, TAB_BAR_HEIGHT), MANTLE, 0.0);
        let tab_width = width / 3.0;
        for (i, tab) in ActiveTab::all().into_iter().enumerate() {
            let tx = i as f32 * tab_width;
            let rect = Rect::new(tx, 0.0, tab_width, TAB_BAR_HEIGHT);
            let active = tab == self.active_tab;
            if active {
                fill(f, rect, SURFACE0, 0.0);
                fill(
                    f,
                    Rect::new(tx, TAB_BAR_HEIGHT - 3.0, tab_width, 3.0),
                    BLUE,
                    0.0,
                );
            }
            text_centred(
                f,
                tx,
                TAB_BAR_HEIGHT / 2.0 - 8.0,
                tab_width,
                tab.label(),
                if active { BLUE } else { SUBTEXT0 },
                15.0,
                if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            f.hit(Target::Tab(tab), rect);
        }
    }

    /// The alarm tab: clock, add button, and the scrolling list — or the
    /// editor, which replaces all three while it is open.
    fn draw_alarm_tab(&self, f: &mut Frame, content: Rect) {
        if let Some(editor) = self.editor.as_ref() {
            self.draw_editor(f, editor, content);
            return;
        }

        // The clock. The whole band is the format toggle, which is a large
        // target for a control with no other home; the alternative — a 12/24
        // chip somewhere — is a second thing on screen saying what the clock
        // already says.
        let clock = Rect::new(content.x, content.y, content.w, CLOCK_H);
        fill(f, clock, SURFACE0, 10.0);
        let (hour, minute, second) = self.current_time;
        let (display_hour, period) = self.time_format.format_hour(hour);
        let now = match period {
            Some(p) => format!("{}:{:02}:{:02} {}", display_hour, minute, second, p),
            None => format!("{:02}:{:02}:{:02}", display_hour, minute, second),
        };
        text_centred(
            f,
            clock.x,
            clock.y + 10.0,
            clock.w,
            &now,
            TEXT_COLOR,
            34.0,
            FontWeightHint::Bold,
        );
        let today = Weekday::from_index(self.current_weekday).unwrap_or(Weekday::Monday);
        let sub = match self.next_alarm() {
            Some((alarm, mins)) => {
                let label = if alarm.label.is_empty() {
                    alarm.format_time(self.time_format)
                } else {
                    alarm.label.clone()
                };
                format!(
                    "{} — {} in {}",
                    today.short_name(),
                    label,
                    humanise_minutes(mins)
                )
            }
            None => format!("{} — no alarm set", today.short_name()),
        };
        text_centred(
            f,
            clock.x,
            clock.y + 58.0,
            clock.w,
            &sub,
            SUBTEXT0,
            12.0,
            FontWeightHint::Regular,
        );
        f.hit(Target::ClockFormat, clock);

        button(
            f,
            Rect::new(
                content.x,
                content.y + CLOCK_H + CHIP_GAP,
                content.w,
                ADD_BUTTON_H,
            ),
            "+ Add Alarm",
            BLUE,
            CRUST,
            Target::AddAlarm,
        );

        let list = Self::alarm_list_rect(content);
        if self.alarms.is_empty() {
            text_centred(
                f,
                list.x,
                list.y + 24.0,
                list.w,
                "No alarms yet",
                OVERLAY0,
                14.0,
                FontWeightHint::Regular,
            );
            return;
        }
        f.clip(list);
        f.translate(0.0, -self.alarm_scroll);
        let mut y = list.y;
        for alarm in &self.alarms {
            // See the lap table: the clip stops the pixels, not the commands.
            // Cards differ in height, so each is tested on its own edges rather
            // than on a fixed stride, but `y` still only increases, so the first
            // card past the bottom ends the walk.
            let card_h = alarm.card_height();
            if y - self.alarm_scroll >= list.bottom() {
                break;
            }
            if y - self.alarm_scroll + card_h > list.y {
                alarm.draw(f, list.x, y, list.w, self.time_format);
            }
            y += card_h + ALARM_ROW_GAP;
        }
        f.untranslate();
        f.unclip();
    }

    /// The alarm editor, drawn over the alarm tab's content area.
    ///
    /// The stack is *solved* for the height it was given, not laid out at a
    /// fixed size and allowed to run off the bottom. At this app's own minimum
    /// window the content area is 248 points tall and the natural stack is
    /// [`EDITOR_NATURAL_H`] — so the Save and Cancel buttons used to be painted
    /// below the panel, hidden by the clip in force and with their hit boxes
    /// dropped by `Frame::hit`. That is worse than a cosmetic overrun: the
    /// editor covers the alarm tab, so with no reachable Save and no reachable
    /// Cancel it was a trap the pointer could not get out of.
    ///
    /// Guarding the overrun would have hidden the buttons just as thoroughly.
    /// Instead every vertical metric — row heights, gaps, pads and the font
    /// sizes that go with them — is multiplied by the ratio of the height
    /// available to the height wanted, capped at 1 so a roomy window is laid
    /// out at natural size and the slack is simply left under the last row.
    /// Widths are untouched: the window is clamped to [`MIN_WIDTH`], so the
    /// horizontal direction has the room the rows were written for.
    fn draw_editor(&self, f: &mut Frame, editor: &AlarmEditor, content: Rect) {
        fill(f, content, SURFACE0, 10.0);
        f.clip(content);

        // Not `clamp` alone: `clamp` returns NaN for a NaN input, and a NaN
        // scale would put a NaN into every rectangle below, where it compares
        // false against every edge and so escapes any bound that is checked.
        // A height that is not a number gets the natural layout instead.
        let ratio = content.h / EDITOR_NATURAL_H;
        let s = if ratio.is_finite() {
            ratio.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let pad = EDITOR_PAD * s;
        let gap = EDITOR_GAP * s;
        let spinner_h = EDITOR_SPINNER_H * s;
        let label_h = EDITOR_LABEL_H * s;
        let chip_h = EDITOR_CHIP_H * s;
        let action_h = EDITOR_ACTION_H * s;

        let x = content.x + 10.0;
        let w = (content.w - 20.0).max(0.0);
        let mut row_y = content.y + pad;
        text(
            f,
            x,
            row_y,
            if editor.editing.is_some() {
                "Edit alarm"
            } else {
                "New alarm"
            },
            TEXT_COLOR,
            15.0 * s,
            FontWeightHint::Bold,
            w,
        );
        row_y += EDITOR_TITLE_H * s + gap;

        // Two spinners, hour and minute, each an up button over a big number
        // over a down button. The three share `spinner_h`, so the number and
        // the space around it give way first as the stack is squeezed.
        let col_w = 84.0;
        let col_gap = 16.0;
        let hour_x = content.x + (content.w - col_w * 2.0 - col_gap) / 2.0;
        let minute_x = hour_x + col_w + col_gap;
        let top = row_y;
        let step_h = 26.0 * s;
        let num_size = 32.0 * s;
        let inner = ((spinner_h - step_h * 2.0 - num_size) / 2.0).max(0.0);
        for (col_x, value, up, down) in [
            (
                hour_x,
                u32::from(editor.hour),
                Target::EditHour(Step::Up),
                Target::EditHour(Step::Down),
            ),
            (
                minute_x,
                u32::from(editor.minute),
                Target::EditMinute(Step::Up),
                Target::EditMinute(Step::Down),
            ),
        ] {
            button(
                f,
                Rect::new(col_x, top, col_w, step_h),
                "\u{25B2}",
                SURFACE1,
                TEXT_COLOR,
                up,
            );
            text_centred(
                f,
                col_x,
                top + step_h + inner,
                col_w,
                &format!("{:02}", value),
                TEXT_COLOR,
                num_size,
                FontWeightHint::Bold,
            );
            button(
                f,
                Rect::new(col_x, top + spinner_h - step_h, col_w, step_h),
                "\u{25BC}",
                SURFACE1,
                TEXT_COLOR,
                down,
            );
        }
        let colon_size = 22.0 * s;
        text_centred(
            f,
            hour_x + col_w,
            top + step_h + inner + (num_size - colon_size) / 2.0,
            col_gap,
            ":",
            SUBTEXT0,
            colon_size,
            FontWeightHint::Bold,
        );
        row_y += spinner_h + gap;

        // Label.
        let label_rect = Rect::new(x, row_y, w, label_h);
        let focused = self.focus == Some(Focus::Label);
        fill(
            f,
            label_rect,
            if focused { SURFACE2 } else { SURFACE1 },
            6.0,
        );
        let body = if editor.label.is_empty() && !focused {
            "Label…".to_string()
        } else if focused {
            format!("{}\u{2502}", editor.label)
        } else {
            editor.label.clone()
        };
        let label_size = 13.0 * s;
        text(
            f,
            x + 8.0,
            row_y + (label_h - label_size) / 2.0,
            body,
            if editor.label.is_empty() && !focused {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            label_size,
            FontWeightHint::Regular,
            (w - 16.0).max(0.0),
        );
        f.hit(Target::EditLabel, label_rect);

        // Repeat-day chips.
        row_y += label_h + gap;
        let chip_w = ((w - CHIP_GAP * 6.0) / 7.0).max(1.0);
        for (i, day) in Weekday::all().into_iter().enumerate() {
            let cx = x + i as f32 * (chip_w + CHIP_GAP);
            let on = editor
                .repeat_days
                .get(day.index())
                .copied()
                .unwrap_or(false);
            button(
                f,
                Rect::new(cx, row_y, chip_w, chip_h),
                day.single_letter(),
                if on { BLUE } else { SURFACE1 },
                if on { CRUST } else { SUBTEXT0 },
                Target::EditDay(day),
            );
        }

        // Sound and snooze, each a chip that cycles on click.
        row_y += chip_h + gap;
        let half = ((w - CHIP_GAP) / 2.0).max(1.0);
        button(
            f,
            Rect::new(x, row_y, half, chip_h),
            &format!("Sound: {}", editor.sound.label()),
            SURFACE1,
            SUBTEXT1,
            Target::EditSound,
        );
        button(
            f,
            Rect::new(x + half + CHIP_GAP, row_y, half, chip_h),
            &format!("Snooze: {}", editor.snooze().label()),
            SURFACE1,
            SUBTEXT1,
            Target::EditSnooze,
        );

        // Save and cancel. The stack was solved so that this row lands inside
        // `content` at every window size the app can be given; the test that
        // owns the claim is `the_editor_can_always_be_left_by_pointer`.
        row_y += chip_h + gap;
        button(
            f,
            Rect::new(x, row_y, half, action_h),
            "Save",
            GREEN,
            CRUST,
            Target::EditSave,
        );
        button(
            f,
            Rect::new(x + half + CHIP_GAP, row_y, half, action_h),
            "Cancel",
            SURFACE2,
            TEXT_COLOR,
            Target::EditCancel,
        );
        f.unclip();
    }

    /// The timer tab: quick presets, the custom-duration row, and the list.
    fn draw_timer_tab(&self, f: &mut Frame, content: Rect) {
        let per_row = PRESETS_PER_ROW as f32;
        let chip_w = ((content.w - CHIP_GAP * (per_row - 1.0)) / per_row).max(1.0);
        for (i, minutes) in TIMER_PRESETS.into_iter().enumerate() {
            let (row, col) = (i / PRESETS_PER_ROW, i % PRESETS_PER_ROW);
            let cx = content.x + col as f32 * (chip_w + CHIP_GAP);
            let cy = content.y + row as f32 * (PRESET_H + CHIP_GAP);
            button(
                f,
                Rect::new(cx, cy, chip_w, PRESET_H),
                &format!("{} min", minutes),
                SURFACE1,
                TEXT_COLOR,
                Target::Preset(minutes),
            );
        }

        // Custom duration: HH : MM : SS, then Start.
        let custom_y = content.y + preset_block_height() + CHIP_GAP;
        let start_w = 76.0;
        let field_w = ((content.w - start_w - CHIP_GAP * 3.0) / 3.0).max(1.0);
        for (i, hms) in HmsField::ALL.into_iter().enumerate() {
            let fx = content.x + i as f32 * (field_w + CHIP_GAP);
            let rect = Rect::new(fx, custom_y, field_w, CUSTOM_H);
            let focused = self.focus == Some(Focus::Custom(hms));
            fill(f, rect, if focused { SURFACE2 } else { SURFACE1 }, 6.0);
            let entry = self.custom.get(hms.index()).map_or("", String::as_str);
            let (body, color) = if entry.is_empty() {
                (hms.placeholder().to_string(), OVERLAY0)
            } else if focused {
                (format!("{}\u{2502}", entry), TEXT_COLOR)
            } else {
                (entry.to_string(), TEXT_COLOR)
            };
            text_centred(
                f,
                rect.x,
                rect.y + 10.0,
                rect.w,
                &body,
                color,
                15.0,
                FontWeightHint::Regular,
            );
            f.hit(Target::CustomField(hms), rect);
        }
        button(
            f,
            Rect::new(content.x + content.w - start_w, custom_y, start_w, CUSTOM_H),
            "Start",
            GREEN,
            CRUST,
            Target::CustomStart,
        );

        let list = Self::timer_list_rect(content);
        if self.timers.is_empty() {
            text_centred(
                f,
                list.x,
                list.y + 24.0,
                list.w,
                "No timers running",
                OVERLAY0,
                14.0,
                FontWeightHint::Regular,
            );
            return;
        }
        f.clip(list);
        f.translate(0.0, -self.timer_scroll);
        for (i, timer) in self.timers.iter().enumerate() {
            let y = list.y + i as f32 * (TIMER_ROW_H + TIMER_ROW_GAP);
            // See the lap table: the clip stops the pixels, not the commands.
            if y - self.timer_scroll >= list.bottom() {
                break;
            }
            if y - self.timer_scroll + TIMER_ROW_H <= list.y {
                continue;
            }
            timer.draw(f, list.x, y, list.w);
        }
        f.untranslate();
        f.unclip();
    }

    // ---- Interaction ----

    /// Route a click at window coordinates `(x, y)`.
    ///
    /// The frame is redrawn to hit-test against, which is the whole point of
    /// the frame: the rectangles tested are by construction the ones drawn, so
    /// a control cannot be clickable where it is not visible.
    pub fn handle_click(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        if button != MouseButton::Left {
            return Action::None;
        }
        let Some(target) = self.frame(size.0, size.1).hit_test(x, y) else {
            // A click on nothing drops the keyboard, so clicking away from a
            // field commits nothing and stops swallowing shortcuts.
            return if self.focus.take().is_some() {
                Action::Redraw
            } else {
                Action::None
            };
        };
        self.activate(target, size)
    }

    /// Apply whatever the named control does.
    ///
    /// Split out from [`handle_click`](Self::handle_click) so a test can drive
    /// a control by name, and so a keyboard shortcut and the button it mirrors
    /// run the same code rather than two copies that can disagree.
    pub fn activate(&mut self, target: Target, size: (f32, f32)) -> Action {
        match target {
            Target::Tab(tab) => {
                if self.active_tab == tab {
                    return Action::None;
                }
                self.active_tab = tab;
                self.focus = None;
            }
            Target::ClockFormat => self.toggle_time_format(),

            Target::AddAlarm => self.open_new_alarm(),
            Target::AlarmRow(id) => self.open_alarm(id),
            Target::AlarmToggle(id) => {
                if self.toggle_alarm(id).is_none() {
                    return Action::None;
                }
            }
            Target::AlarmDelete(id) => {
                if !self.delete_alarm(id) {
                    return Action::None;
                }
                // An editor open on the deleted alarm is closed with it: a form
                // whose Save would resurrect the row the user just deleted is
                // worse than no form.
                if self.editor.as_ref().and_then(|e| e.editing) == Some(id) {
                    self.cancel_editor();
                }
                self.clamp_scrolls(size.0, size.1);
            }
            Target::AlarmSnooze(id) => self.snooze_alarm(id),
            Target::AlarmDismiss(id) => {
                self.dismiss_alarm(id);
                self.clamp_scrolls(size.0, size.1);
            }

            Target::EditHour(step) => match self.editor.as_mut() {
                Some(editor) => editor.step_hour(step),
                None => return Action::None,
            },
            Target::EditMinute(step) => match self.editor.as_mut() {
                Some(editor) => editor.step_minute(step),
                None => return Action::None,
            },
            Target::EditLabel => self.focus = Some(Focus::Label),
            Target::EditDay(day) => match self.editor.as_mut() {
                Some(editor) => {
                    if let Some(slot) = editor.repeat_days.get_mut(day.index()) {
                        *slot = !*slot;
                    }
                }
                None => return Action::None,
            },
            Target::EditSound => match self.editor.as_mut() {
                Some(editor) => {
                    let next = editor
                        .sound
                        .index()
                        .saturating_add(1)
                        .checked_rem(AlarmSound::all().len())
                        .unwrap_or(0);
                    editor.sound = AlarmSound::from_index(next).unwrap_or_default();
                }
                None => return Action::None,
            },
            Target::EditSnooze => match self.editor.as_mut() {
                Some(editor) => {
                    editor.snooze_index = editor
                        .snooze_index
                        .saturating_add(1)
                        .checked_rem(SNOOZE_OPTIONS.len())
                        .unwrap_or(0);
                }
                None => return Action::None,
            },
            Target::EditSave => {
                if self.save_editor().is_none() {
                    return Action::None;
                }
                self.clamp_scrolls(size.0, size.1);
            }
            Target::EditCancel => {
                if self.editor.is_none() {
                    return Action::None;
                }
                self.cancel_editor();
            }

            Target::Preset(minutes) => {
                let id = self.create_timer_preset(minutes);
                self.start_timer(id);
            }
            Target::CustomField(field) => self.focus = Some(Focus::Custom(field)),
            Target::CustomStart => {
                if self.start_custom_timer().is_none() {
                    return Action::None;
                }
            }
            Target::TimerRow(_) => return Action::None,
            Target::TimerToggle(id) => match self.find_timer_mut(id) {
                Some(timer) => timer.toggle(),
                None => return Action::None,
            },
            Target::TimerReset(id) => match self.find_timer_mut(id) {
                Some(timer) => timer.reset(),
                None => return Action::None,
            },
            Target::TimerDelete(id) => {
                if !self.delete_timer(id) {
                    return Action::None;
                }
                self.clamp_scrolls(size.0, size.1);
            }

            Target::SwToggle => self.stopwatch.toggle(),
            Target::SwLap => {
                // Only meaningful while running, and the button is drawn dimmed
                // when it is not — `lap` itself already refuses, but returning
                // `None` here keeps a dead click from repainting.
                if self.stopwatch.state != StopwatchState::Running {
                    return Action::None;
                }
                self.stopwatch.lap();
            }
            Target::SwReset => {
                self.stopwatch.reset();
                self.lap_scroll = 0.0;
            }
        }
        Action::Redraw
    }

    /// Route a keystroke.
    pub fn handle_key(&mut self, event: &KeyEvent, size: (f32, f32)) -> Action {
        if !event.pressed {
            return Action::None;
        }
        let m = event.modifiers;

        // Ctrl-Q closes, and is checked before anything else so it works even
        // with a text field focused.
        if m.ctrl && event.key == Key::Q {
            return Action::Quit;
        }

        if event.key == Key::Escape {
            if self.editor.is_some() {
                self.cancel_editor();
                return Action::Redraw;
            }
            return if self.focus.take().is_some() {
                Action::Redraw
            } else {
                Action::None
            };
        }

        // A focused field owns the keyboard: while the user is typing a label,
        // `r` is the letter r and not "reset the stopwatch".
        if let Some(focus) = self.focus {
            return self.type_into(focus, event);
        }

        // Nothing focused, so bare keys are shortcuts — but only bare ones.
        // Alt-Tab belongs to the window manager, and a program that acted on
        // the Tab of it would switch its own tab every time the user switched
        // windows.
        if m.ctrl || m.alt || m.super_key {
            return Action::None;
        }

        match event.key {
            Key::Tab => {
                let step = if m.shift { -1 } else { 1 };
                let tabs = ActiveTab::all();
                let here = tabs.iter().position(|t| *t == self.active_tab).unwrap_or(0);
                let next = (here as isize)
                    .saturating_add(step)
                    .rem_euclid(tabs.len() as isize) as usize;
                self.active_tab = tabs.get(next).copied().unwrap_or_default();
                Action::Redraw
            }
            Key::Num1 => self.activate(Target::Tab(ActiveTab::Alarm), size),
            Key::Num2 => self.activate(Target::Tab(ActiveTab::Timer), size),
            Key::Num3 => self.activate(Target::Tab(ActiveTab::Stopwatch), size),
            Key::N if self.active_tab == ActiveTab::Alarm => self.activate(Target::AddAlarm, size),
            Key::F => self.activate(Target::ClockFormat, size),
            Key::Space if self.active_tab == ActiveTab::Stopwatch => {
                self.activate(Target::SwToggle, size)
            }
            Key::L if self.active_tab == ActiveTab::Stopwatch => self.activate(Target::SwLap, size),
            Key::R if self.active_tab == ActiveTab::Stopwatch => {
                self.activate(Target::SwReset, size)
            }
            _ => Action::None,
        }
    }

    /// Feed a keystroke to the focused field.
    fn type_into(&mut self, focus: Focus, event: &KeyEvent) -> Action {
        match focus {
            Focus::Label => {
                let Some(editor) = self.editor.as_mut() else {
                    // The field cannot be focused without an editor; if it
                    // somehow is, drop the focus rather than swallow keys.
                    self.focus = None;
                    return Action::Redraw;
                };
                match event.key {
                    Key::Enter => {
                        self.save_editor();
                        return Action::Redraw;
                    }
                    Key::Backspace => {
                        if editor.label.pop().is_none() {
                            return Action::None;
                        }
                        return Action::Redraw;
                    }
                    _ => {}
                }
                let mut typed = false;
                for ch in event.typed() {
                    if editor.label.chars().count() >= MAX_LABEL_LEN {
                        break;
                    }
                    editor.label.push(ch);
                    typed = true;
                }
                if typed { Action::Redraw } else { Action::None }
            }
            Focus::Custom(field) => {
                match event.key {
                    Key::Enter => {
                        return if self.start_custom_timer().is_some() {
                            Action::Redraw
                        } else {
                            Action::None
                        };
                    }
                    Key::Tab => {
                        let step = if event.modifiers.shift { -1 } else { 1 };
                        let here = field.index();
                        let next = (here as isize)
                            .saturating_add(step)
                            .rem_euclid(HmsField::ALL.len() as isize)
                            as usize;
                        self.focus = HmsField::ALL.get(next).copied().map(Focus::Custom);
                        return Action::Redraw;
                    }
                    Key::Backspace => {
                        let Some(entry) = self.custom.get_mut(field.index()) else {
                            return Action::None;
                        };
                        return if entry.pop().is_some() {
                            Action::Redraw
                        } else {
                            Action::None
                        };
                    }
                    _ => {}
                }
                let Some(entry) = self.custom.get_mut(field.index()) else {
                    return Action::None;
                };
                let mut typed = false;
                for ch in event.typed() {
                    // Digits only, two of them. A duration field that accepted
                    // letters would parse to zero at the moment Start is
                    // pressed, which reads as the button being broken rather
                    // than as the entry being rejected.
                    if !ch.is_ascii_digit() || entry.len() >= 2 {
                        continue;
                    }
                    entry.push(ch);
                    typed = true;
                }
                if typed { Action::Redraw } else { Action::None }
            }
        }
    }

    /// Route any event that is not a resize.
    pub fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::Scroll { dx: _, dy } => self.scroll(mouse.x, mouse.y, dy, size),
                _ => Action::None,
            },
            Event::Key(key) => self.handle_key(key, size),
            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),
            Event::CloseRequested => Action::Quit,
            _ => Action::None,
        }
    }

    /// Scroll whichever pane the pointer is over.
    fn scroll(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Action {
        let content = Self::content_rect(size.0.max(MIN_WIDTH), size.1.max(MIN_HEIGHT));
        // A continuous pixel offset, so `wheel::pixels` and not the row
        // accumulator: the alarm rows are not a uniform height, so there is no
        // row count for an accumulator to bank.
        let (rect, offset, content_h) = match self.active_tab {
            ActiveTab::Alarm if self.editor.is_none() => (
                Self::alarm_list_rect(content),
                &mut self.alarm_scroll,
                self.alarms
                    .iter()
                    .map(|a| a.card_height() + ALARM_ROW_GAP)
                    .sum::<f32>(),
            ),
            ActiveTab::Timer => (
                Self::timer_list_rect(content),
                &mut self.timer_scroll,
                self.timers.len() as f32 * (TIMER_ROW_H + TIMER_ROW_GAP),
            ),
            ActiveTab::Stopwatch => (
                Rect::new(
                    content.x,
                    content.y + Stopwatch::LAP_TABLE_TOP,
                    content.w,
                    (content.h - Stopwatch::LAP_TABLE_TOP).max(0.0),
                ),
                &mut self.lap_scroll,
                self.stopwatch.lap_content_height(),
            ),
            ActiveTab::Alarm => return Action::None,
        };
        if !rect.contains(x, y) {
            return Action::None;
        }
        let before = *offset;
        *offset = clamp_scroll(
            before + guitk::wheel::pixels(dy, LAP_ROW_H),
            content_h,
            rect.h,
        );
        if (*offset - before).abs() < f32::EPSILON {
            Action::None
        } else {
            Action::Redraw
        }
    }

    /// Advance everything by `elapsed_ms` of real time.
    ///
    /// `elapsed_ms` is what actually elapsed, not what was asked for: a tick
    /// that arrives late because the machine was busy still advances the
    /// stopwatch by the time that passed. The per-second work is driven off a
    /// banked remainder rather than off the tick itself, so it runs once a
    /// second at either tick rate.
    pub fn tick(&mut self, elapsed_ms: u64) -> Action {
        self.stopwatch.tick(elapsed_ms);
        self.tick_accum_ms = self.tick_accum_ms.saturating_add(elapsed_ms);
        let whole_seconds = self.tick_accum_ms / 1000;
        self.tick_accum_ms %= 1000;
        for _ in 0..whole_seconds {
            self.tick_timers();
            self.tick_alarm_snoozes();
            self.check_alarm_triggers();
        }
        if whole_seconds > 0 {
            // The wall clock is read once per second, not once per tick: at
            // `TICK_FAST` that would be twenty `SystemTime::now()` calls a
            // second to move a display that shows whole seconds.
            self.refresh_clock();
            self.check_alarm_triggers();
        }
        // Always a repaint: at `TICK_SLOW` the seconds digit has moved, and at
        // `TICK_FAST` the stopwatch's hundredths have.
        Action::Redraw
    }
}

/// Clamp a scroll offset to the range a pane of `viewport` height can show of
/// `content` height.
fn clamp_scroll(scroll: f32, content: f32, viewport: f32) -> f32 {
    if !scroll.is_finite() {
        return 0.0;
    }
    scroll.clamp(0.0, (content - viewport).max(0.0))
}

/// Height of the preset block on the timer tab, gaps included.
fn preset_block_height() -> f32 {
    let rows = TIMER_PRESETS.len().div_ceil(PRESETS_PER_ROW) as f32;
    (rows * (PRESET_H + CHIP_GAP) - CHIP_GAP).max(0.0)
}

/// This crate's weekday index (0 = Monday) for a `guitk::date` weekday
/// (0 = Sunday).
///
/// The two calendars disagree about where a week starts, and this is the one
/// place that reconciles them — an alarm set for "Mon" that fires on Sunday is
/// what a second, inline copy of this subtraction buys.
fn weekday_index(day: guitk::date::Weekday) -> usize {
    usize::try_from(day.index().saturating_sub(1).rem_euclid(7)).unwrap_or(0)
}

/// "in 3 h 20 min" rather than "in 200 min".
fn humanise_minutes(total: u32) -> String {
    let hours = total / 60;
    let minutes = total % 60;
    match (hours, minutes) {
        (0, 0) => "under a minute".to_string(),
        (0, m) => format!("{} min", m),
        (h, 0) => format!("{} h", h),
        (h, m) => format!("{} h {} min", h, m),
    }
}

impl Default for AlarmClockApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a duration in seconds as `HH:MM:SS` or `MM:SS`.
pub fn format_duration_hms(total_seconds: u32) -> String {
    guitk::duration::clock(u64::from(total_seconds))
}

/// Format a duration in milliseconds as `MM:SS.mmm` or `HH:MM:SS.mmm`.
///
/// The stopwatch reads the same span as [`format_duration_hms`], one field
/// wider. Both defer to `guitk::duration` so they cannot come to disagree
/// about where the hours field appears — which is the bug this crate's
/// sibling `screen_capture` shipped for exactly that reason.
pub fn format_duration_ms(total_ms: u64) -> String {
    guitk::duration::clock_ms(total_ms)
}

/// Parse a `HH:MM:SS` or `MM:SS` string into total seconds.
pub fn parse_duration_hms(input: &str) -> Option<u32> {
    let parts: Vec<&str> = input.split(':').collect();
    match parts.len() {
        2 => {
            let m: u32 = parts.first()?.parse().ok()?;
            let s: u32 = parts.get(1)?.parse().ok()?;
            if s >= 60 {
                return None;
            }
            Some(m.checked_mul(60)?.checked_add(s)?)
        }
        3 => {
            let h: u32 = parts.first()?.parse().ok()?;
            let m: u32 = parts.get(1)?.parse().ok()?;
            let s: u32 = parts.get(2)?.parse().ok()?;
            if m >= 60 || s >= 60 {
                return None;
            }
            Some(
                h.checked_mul(3600)?
                    .checked_add(m.checked_mul(60)?)?
                    .checked_add(s)?,
            )
        }
        _ => None,
    }
}

// ============================================================================
// Window integration
// ============================================================================

impl App for AlarmClockApp {
    fn title(&self) -> String {
        "Clock".to_string()
    }

    fn app_id(&self) -> String {
        "alarmclock".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Ask to be woken, always, and faster while the stopwatch runs.
    ///
    /// **Never `None`.** An alarm clock that stopped its own clock would show
    /// the time the window opened at, and its alarms — which fire from that
    /// clock in [`check_alarm_triggers`] — would never fire at all. The rate
    /// varies because only the stopwatch needs a fast one: it shows hundredths,
    /// and at [`TICK_SLOW`] those digits would visibly jump. Everything else
    /// moves once a second, so twice a second is enough to keep the display
    /// within half a second of the truth while leaving an idle window mostly
    /// asleep.
    ///
    /// This method is consulted after *every* event, so the switch takes effect
    /// on the tick after the one that started the stopwatch.
    ///
    /// [`check_alarm_triggers`]: AlarmClockApp::check_alarm_triggers
    fn tick_interval(&self) -> Option<Duration> {
        Some(if self.stopwatch.state == StopwatchState::Running {
            TICK_FAST
        } else {
            TICK_SLOW
        })
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if let Event::Resize { width, height } = *event {
            let size = (
                (width as f32).max(MIN_WIDTH),
                (height as f32).max(MIN_HEIGHT),
            );
            self.window_size = size;
            // A window made taller shows more of a scrolled list, so an offset
            // that was legal at the old height can be past the end at the new
            // one — leaving the pane blank until the user scrolls back.
            self.clamp_scrolls(size.0, size.1);
            return Response::Redraw;
        }
        let size = self.window_size;
        match self.handle_event(event, size) {
            Action::None => Response::Idle,
            Action::Redraw => Response::Redraw,
            Action::Quit => Response::Exit,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The handed size wins over the recorded one: the first frame is drawn
        // before any `Event::Resize` arrives, so trusting the record would lay
        // the first window out at a size it is not — and every hit box in it
        // would then name the wrong rectangle.
        let size = (width.max(MIN_WIDTH), height.max(MIN_HEIGHT));
        if self.window_size != size {
            self.window_size = size;
            self.clamp_scrolls(size.0, size.1);
        }
        self.frame(width, height).into_tree()
    }
}

impl Probe for AlarmClockApp {
    type Target = Target;
    type Outcome = Action;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.handle_click(x, y, button, size)
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.handle_key(key, size)
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    let mut state = AlarmClockApp::new();
    // Read the clock before the first frame, so the window does not open on
    // 00:00:00 and correct itself half a second later.
    state.refresh_clock();
    app::launch("alarmclock", &mut state)
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
    use guitk::probe;

    // ---- Weekday tests ----

    #[test]
    fn test_weekday_all() {
        let days = Weekday::all();
        assert_eq!(days.len(), 7);
        assert_eq!(days[0], Weekday::Monday);
        assert_eq!(days[6], Weekday::Sunday);
    }

    #[test]
    fn test_weekday_index_roundtrip() {
        for day in Weekday::all() {
            assert_eq!(Weekday::from_index(day.index()), Some(day));
        }
    }

    #[test]
    fn test_weekday_from_index_invalid() {
        assert_eq!(Weekday::from_index(7), None);
        assert_eq!(Weekday::from_index(100), None);
    }

    #[test]
    fn test_weekday_short_name() {
        assert_eq!(Weekday::Monday.short_name(), "Mon");
        assert_eq!(Weekday::Friday.short_name(), "Fri");
        assert_eq!(Weekday::Sunday.short_name(), "Sun");
    }

    #[test]
    fn test_weekday_single_letter() {
        assert_eq!(Weekday::Monday.single_letter(), "M");
        assert_eq!(Weekday::Wednesday.single_letter(), "W");
        assert_eq!(Weekday::Friday.single_letter(), "F");
    }

    // ---- TimeFormat tests ----

    #[test]
    fn test_time_format_12h_am() {
        let fmt = TimeFormat::TwelveHour;
        let (h, p) = fmt.format_hour(0);
        assert_eq!(h, 12);
        assert_eq!(p, Some("AM"));
    }

    #[test]
    fn test_time_format_12h_noon() {
        let fmt = TimeFormat::TwelveHour;
        let (h, p) = fmt.format_hour(12);
        assert_eq!(h, 12);
        assert_eq!(p, Some("PM"));
    }

    #[test]
    fn test_time_format_12h_pm() {
        let fmt = TimeFormat::TwelveHour;
        let (h, p) = fmt.format_hour(15);
        assert_eq!(h, 3);
        assert_eq!(p, Some("PM"));
    }

    #[test]
    fn test_time_format_12h_morning() {
        let fmt = TimeFormat::TwelveHour;
        let (h, p) = fmt.format_hour(9);
        assert_eq!(h, 9);
        assert_eq!(p, Some("AM"));
    }

    #[test]
    fn test_time_format_24h() {
        let fmt = TimeFormat::TwentyFourHour;
        let (h, p) = fmt.format_hour(15);
        assert_eq!(h, 15);
        assert_eq!(p, None);
    }

    #[test]
    fn test_time_format_24h_midnight() {
        let fmt = TimeFormat::TwentyFourHour;
        let (h, p) = fmt.format_hour(0);
        assert_eq!(h, 0);
        assert_eq!(p, None);
    }

    #[test]
    fn test_time_format_default() {
        let fmt = TimeFormat::default();
        assert_eq!(fmt, TimeFormat::TwelveHour);
    }

    // ---- AlarmSound tests ----

    #[test]
    fn test_alarm_sound_all() {
        let sounds = AlarmSound::all();
        assert_eq!(sounds.len(), 6);
    }

    #[test]
    fn test_alarm_sound_index_roundtrip() {
        for sound in AlarmSound::all() {
            assert_eq!(AlarmSound::from_index(sound.index()), Some(sound));
        }
    }

    #[test]
    fn test_alarm_sound_from_index_invalid() {
        assert_eq!(AlarmSound::from_index(6), None);
        assert_eq!(AlarmSound::from_index(100), None);
    }

    #[test]
    fn test_alarm_sound_labels() {
        assert_eq!(AlarmSound::Default.label(), "Default");
        assert_eq!(AlarmSound::Loud.label(), "Loud");
    }

    // ---- SnoozeDuration tests ----

    #[test]
    fn test_snooze_duration_default() {
        let s = SnoozeDuration::default();
        assert_eq!(s.minutes, 5);
    }

    #[test]
    fn test_snooze_duration_as_seconds() {
        let s = SnoozeDuration::new(10);
        assert_eq!(s.as_seconds(), 600);
    }

    #[test]
    fn test_snooze_duration_label() {
        let s = SnoozeDuration::new(15);
        assert_eq!(s.label(), "15 min");
    }

    // ---- Alarm tests ----

    #[test]
    fn test_alarm_new_clamps_hour() {
        let alarm = Alarm::new(AlarmId(1), 25, 30);
        assert_eq!(alarm.hour, 23);
    }

    #[test]
    fn test_alarm_new_clamps_minute() {
        let alarm = Alarm::new(AlarmId(1), 10, 70);
        assert_eq!(alarm.minute, 59);
    }

    #[test]
    fn test_alarm_not_repeating_by_default() {
        let alarm = Alarm::new(AlarmId(1), 7, 0);
        assert!(!alarm.is_repeating());
    }

    #[test]
    fn test_alarm_toggle_day() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.toggle_day(Weekday::Monday);
        assert!(alarm.repeats_on(Weekday::Monday));
        alarm.toggle_day(Weekday::Monday);
        assert!(!alarm.repeats_on(Weekday::Monday));
    }

    #[test]
    fn test_alarm_set_day() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.set_day(Weekday::Friday, true);
        assert!(alarm.repeats_on(Weekday::Friday));
        alarm.set_day(Weekday::Friday, false);
        assert!(!alarm.repeats_on(Weekday::Friday));
    }

    #[test]
    fn test_alarm_repeat_summary_once() {
        let alarm = Alarm::new(AlarmId(1), 7, 0);
        assert_eq!(alarm.repeat_summary(), "Once");
    }

    #[test]
    fn test_alarm_repeat_summary_every_day() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        for day in Weekday::all() {
            alarm.set_day(day, true);
        }
        assert_eq!(alarm.repeat_summary(), "Every day");
    }

    #[test]
    fn test_alarm_repeat_summary_weekdays() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.set_day(Weekday::Monday, true);
        alarm.set_day(Weekday::Tuesday, true);
        alarm.set_day(Weekday::Wednesday, true);
        alarm.set_day(Weekday::Thursday, true);
        alarm.set_day(Weekday::Friday, true);
        assert_eq!(alarm.repeat_summary(), "Weekdays");
    }

    #[test]
    fn test_alarm_repeat_summary_weekends() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.set_day(Weekday::Saturday, true);
        alarm.set_day(Weekday::Sunday, true);
        assert_eq!(alarm.repeat_summary(), "Weekends");
    }

    #[test]
    fn test_alarm_repeat_summary_custom_days() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.set_day(Weekday::Monday, true);
        alarm.set_day(Weekday::Wednesday, true);
        alarm.set_day(Weekday::Friday, true);
        assert_eq!(alarm.repeat_summary(), "Mon, Wed, Fri");
    }

    #[test]
    fn test_alarm_format_time_12h() {
        let alarm = Alarm::new(AlarmId(1), 14, 30);
        assert_eq!(alarm.format_time(TimeFormat::TwelveHour), "2:30 PM");
    }

    #[test]
    fn test_alarm_format_time_24h() {
        let alarm = Alarm::new(AlarmId(1), 14, 5);
        assert_eq!(alarm.format_time(TimeFormat::TwentyFourHour), "14:05");
    }

    #[test]
    fn test_alarm_format_time_midnight_12h() {
        let alarm = Alarm::new(AlarmId(1), 0, 0);
        assert_eq!(alarm.format_time(TimeFormat::TwelveHour), "12:00 AM");
    }

    #[test]
    fn test_alarm_snooze() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.ringing = true;
        alarm.snooze();
        assert!(!alarm.ringing);
        assert!(alarm.snoozed_remaining.is_some());
        assert_eq!(alarm.snoozed_remaining.unwrap(), 300); // 5 min default
    }

    #[test]
    fn test_alarm_dismiss() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.ringing = true;
        alarm.snoozed_remaining = Some(120);
        alarm.dismiss();
        assert!(!alarm.ringing);
        assert!(alarm.snoozed_remaining.is_none());
    }

    #[test]
    fn test_alarm_tick_snooze() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        alarm.snoozed_remaining = Some(2);
        assert!(!alarm.tick_snooze()); // 2 -> 1
        assert!(!alarm.tick_snooze()); // 1 -> 0
        assert!(alarm.tick_snooze()); // 0 -> ringing
        assert!(alarm.ringing);
        assert!(alarm.snoozed_remaining.is_none());
    }

    #[test]
    fn test_alarm_tick_snooze_no_snooze() {
        let mut alarm = Alarm::new(AlarmId(1), 7, 0);
        assert!(!alarm.tick_snooze());
    }

    #[test]
    fn test_alarm_minutes_until_later_today() {
        let alarm = Alarm::new(AlarmId(1), 14, 30);
        let mins = alarm.minutes_until(10, 0, None);
        assert_eq!(mins, Some(270)); // 4h30m = 270 min
    }

    #[test]
    fn test_alarm_minutes_until_tomorrow() {
        let alarm = Alarm::new(AlarmId(1), 6, 0);
        let mins = alarm.minutes_until(10, 0, None);
        // Tomorrow: 24*60 - 600 + 360 = 1200 min
        assert_eq!(mins, Some(1200));
    }

    #[test]
    fn test_alarm_minutes_until_disabled() {
        let mut alarm = Alarm::new(AlarmId(1), 14, 30);
        alarm.enabled = false;
        assert_eq!(alarm.minutes_until(10, 0, None), None);
    }

    #[test]
    fn test_alarm_minutes_until_ringing() {
        let mut alarm = Alarm::new(AlarmId(1), 14, 30);
        alarm.ringing = true;
        assert_eq!(alarm.minutes_until(10, 0, None), None);
    }

    #[test]
    fn test_alarm_minutes_until_repeating() {
        let mut alarm = Alarm::new(AlarmId(1), 8, 0);
        alarm.set_day(Weekday::Wednesday, true); // index 2
        // Current: Monday (0) at 10:00, alarm at 08:00.
        // Next Wednesday = 2 days ahead.
        let mins = alarm.minutes_until(10, 0, Some(0));
        // 2 * 24 * 60 + (8*60 - 10*60) = 2880 - 120 = 2760
        assert_eq!(mins, Some(2760));
    }

    #[test]
    fn test_alarm_enabled_by_default() {
        let alarm = Alarm::new(AlarmId(1), 8, 0);
        assert!(alarm.enabled);
    }

    #[test]
    fn alarm_card_records_its_own_controls() {
        let alarm = Alarm::new(AlarmId(7), 8, 30);
        let mut f: Frame = Frame::new(400.0, 200.0);
        alarm.draw(&mut f, 0.0, 0.0, 400.0, TimeFormat::TwelveHour);
        let targets: Vec<Target> = f.hits().iter().map(|(t, _)| *t).collect();
        assert!(targets.contains(&Target::AlarmRow(AlarmId(7))));
        assert!(targets.contains(&Target::AlarmToggle(AlarmId(7))));
        assert!(targets.contains(&Target::AlarmDelete(AlarmId(7))));
        assert!(f.is_balanced(), "a card must not leave a clip open");
    }

    #[test]
    fn a_ringing_alarm_grows_snooze_and_dismiss() {
        let mut alarm = Alarm::new(AlarmId(7), 8, 30);
        let quiet = alarm.card_height();
        alarm.ringing = true;
        assert!(
            alarm.card_height() > quiet,
            "the strip has to be paid for in height, or the next card covers it"
        );

        let mut f: Frame = Frame::new(400.0, 200.0);
        alarm.draw(&mut f, 0.0, 0.0, 400.0, TimeFormat::TwelveHour);
        let targets: Vec<Target> = f.hits().iter().map(|(t, _)| *t).collect();
        assert!(targets.contains(&Target::AlarmSnooze(AlarmId(7))));
        assert!(targets.contains(&Target::AlarmDismiss(AlarmId(7))));
    }

    // ---- Timer tests ----

    #[test]
    fn test_timer_new() {
        let timer = Timer::new(TimerId(1), 300);
        assert_eq!(timer.total_seconds, 300);
        assert_eq!(timer.remaining_seconds, 300);
        assert_eq!(timer.state, TimerState::Idle);
    }

    #[test]
    fn test_timer_with_label() {
        let timer = Timer::new(TimerId(1), 60).with_label("Tea");
        assert_eq!(timer.label, "Tea");
    }

    #[test]
    fn test_timer_start() {
        let mut timer = Timer::new(TimerId(1), 60);
        timer.start();
        assert_eq!(timer.state, TimerState::Running);
    }

    #[test]
    fn test_timer_pause() {
        let mut timer = Timer::new(TimerId(1), 60);
        timer.start();
        timer.pause();
        assert_eq!(timer.state, TimerState::Paused);
    }

    #[test]
    fn test_timer_pause_when_idle() {
        let mut timer = Timer::new(TimerId(1), 60);
        timer.pause();
        assert_eq!(timer.state, TimerState::Idle);
    }

    #[test]
    fn test_timer_reset() {
        let mut timer = Timer::new(TimerId(1), 300);
        timer.start();
        timer.tick();
        timer.tick();
        timer.reset();
        assert_eq!(timer.remaining_seconds, 300);
        assert_eq!(timer.state, TimerState::Idle);
    }

    #[test]
    fn test_timer_tick() {
        let mut timer = Timer::new(TimerId(1), 3);
        timer.start();
        assert!(!timer.tick()); // 3 -> 2
        assert!(!timer.tick()); // 2 -> 1
        assert!(timer.tick()); // 1 -> 0 (finished!)
        assert_eq!(timer.state, TimerState::Finished);
    }

    #[test]
    fn test_timer_tick_when_idle() {
        let mut timer = Timer::new(TimerId(1), 60);
        assert!(!timer.tick());
        assert_eq!(timer.remaining_seconds, 60);
    }

    #[test]
    fn test_timer_tick_zero_duration() {
        let mut timer = Timer::new(TimerId(1), 0);
        timer.start();
        assert!(timer.tick());
        assert_eq!(timer.state, TimerState::Finished);
    }

    #[test]
    fn test_timer_progress_start() {
        let timer = Timer::new(TimerId(1), 100);
        assert!((timer.progress() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_timer_progress_half() {
        let mut timer = Timer::new(TimerId(1), 100);
        timer.remaining_seconds = 50;
        assert!((timer.progress() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_timer_progress_done() {
        let mut timer = Timer::new(TimerId(1), 100);
        timer.remaining_seconds = 0;
        assert!((timer.progress() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_timer_progress_zero_total() {
        let timer = Timer::new(TimerId(1), 0);
        assert!((timer.progress() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_timer_format_remaining_minutes() {
        let timer = Timer::new(TimerId(1), 125);
        assert_eq!(timer.format_remaining(), "02:05");
    }

    #[test]
    fn test_timer_format_remaining_hours() {
        let timer = Timer::new(TimerId(1), 3661);
        assert_eq!(timer.format_remaining(), "01:01:01");
    }

    #[test]
    fn test_timer_resume_from_paused() {
        let mut timer = Timer::new(TimerId(1), 60);
        timer.start();
        timer.tick(); // 60->59
        timer.pause();
        assert_eq!(timer.remaining_seconds, 59);
        timer.start(); // resume
        assert_eq!(timer.state, TimerState::Running);
        assert_eq!(timer.remaining_seconds, 59);
    }

    #[test]
    fn timer_card_records_its_own_controls() {
        let timer = Timer::new(TimerId(3), 300);
        let mut f: Frame = Frame::new(400.0, 200.0);
        timer.draw(&mut f, 0.0, 0.0, 400.0);
        let targets: Vec<Target> = f.hits().iter().map(|(t, _)| *t).collect();
        assert!(targets.contains(&Target::TimerRow(TimerId(3))));
        assert!(targets.contains(&Target::TimerToggle(TimerId(3))));
        assert!(targets.contains(&Target::TimerReset(TimerId(3))));
        assert!(targets.contains(&Target::TimerDelete(TimerId(3))));
        assert!(f.is_balanced());
    }

    #[test]
    fn the_one_timer_button_says_what_it_will_do() {
        let mut timer = Timer::new(TimerId(1), 300);
        assert_eq!(timer.toggle_label(), "Start");
        timer.toggle();
        assert_eq!(timer.state, TimerState::Running);
        assert_eq!(timer.toggle_label(), "Pause");
        timer.toggle();
        assert_eq!(timer.state, TimerState::Paused);
        assert_eq!(timer.toggle_label(), "Resume");
        timer.toggle();
        assert_eq!(timer.state, TimerState::Running);

        // A finished timer restarts from the top rather than "starting" at
        // zero, which would finish again on the next tick and look like the
        // button did nothing.
        timer.state = TimerState::Finished;
        timer.remaining_seconds = 0;
        assert_eq!(timer.toggle_label(), "Restart");
        timer.toggle();
        assert_eq!(timer.state, TimerState::Running);
        assert_eq!(timer.remaining_seconds, 300);
    }

    // ---- Stopwatch tests ----

    #[test]
    fn test_stopwatch_new() {
        let sw = Stopwatch::new();
        assert_eq!(sw.state, StopwatchState::Stopped);
        assert_eq!(sw.elapsed_ms, 0);
        assert!(sw.laps.is_empty());
    }

    #[test]
    fn test_stopwatch_start() {
        let mut sw = Stopwatch::new();
        sw.start();
        assert_eq!(sw.state, StopwatchState::Running);
    }

    #[test]
    fn test_stopwatch_pause() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.pause();
        assert_eq!(sw.state, StopwatchState::Paused);
    }

    #[test]
    fn test_stopwatch_tick() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(1000);
        assert_eq!(sw.elapsed_ms, 1000);
        sw.tick(500);
        assert_eq!(sw.elapsed_ms, 1500);
    }

    #[test]
    fn test_stopwatch_tick_paused() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(1000);
        sw.pause();
        sw.tick(500);
        assert_eq!(sw.elapsed_ms, 1000); // no change when paused
    }

    #[test]
    fn test_stopwatch_reset() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(5000);
        sw.lap();
        sw.reset();
        assert_eq!(sw.state, StopwatchState::Stopped);
        assert_eq!(sw.elapsed_ms, 0);
        assert!(sw.laps.is_empty());
    }

    #[test]
    fn test_stopwatch_lap() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(3000);
        sw.lap();
        assert_eq!(sw.laps.len(), 1);
        assert_eq!(sw.laps[0].number, 1);
        assert_eq!(sw.laps[0].split_ms, 3000);
        assert_eq!(sw.laps[0].elapsed_ms, 3000);
    }

    #[test]
    fn test_stopwatch_multiple_laps() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(1000);
        sw.lap();
        sw.tick(2000);
        sw.lap();
        sw.tick(500);
        sw.lap();
        assert_eq!(sw.laps.len(), 3);
        assert_eq!(sw.laps[0].split_ms, 1000);
        assert_eq!(sw.laps[1].split_ms, 2000);
        assert_eq!(sw.laps[2].split_ms, 500);
        assert_eq!(sw.laps[2].elapsed_ms, 3500);
    }

    #[test]
    fn test_stopwatch_lap_when_stopped() {
        let mut sw = Stopwatch::new();
        sw.lap();
        assert!(sw.laps.is_empty());
    }

    #[test]
    fn test_stopwatch_lap_stats_none() {
        let sw = Stopwatch::new();
        assert!(sw.lap_stats().is_none());
    }

    #[test]
    fn test_stopwatch_lap_stats() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(1000);
        sw.lap();
        sw.tick(3000);
        sw.lap();
        sw.tick(2000);
        sw.lap();
        let stats = sw.lap_stats().unwrap();
        assert_eq!(stats.best_ms, 1000);
        assert_eq!(stats.worst_ms, 3000);
        assert_eq!(stats.average_ms, 2000);
        assert_eq!(stats.count, 3);
    }

    #[test]
    fn test_stopwatch_lap_stats_single() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(5000);
        sw.lap();
        let stats = sw.lap_stats().unwrap();
        assert_eq!(stats.best_ms, 5000);
        assert_eq!(stats.worst_ms, 5000);
        assert_eq!(stats.average_ms, 5000);
        assert_eq!(stats.count, 1);
    }

    #[test]
    fn test_stopwatch_format_elapsed() {
        let mut sw = Stopwatch::new();
        sw.elapsed_ms = 65123;
        assert_eq!(sw.format_elapsed(), "01:05.123");
    }

    #[test]
    fn test_stopwatch_resume() {
        let mut sw = Stopwatch::new();
        sw.start();
        sw.tick(1000);
        sw.pause();
        sw.start();
        assert_eq!(sw.state, StopwatchState::Running);
        assert_eq!(sw.elapsed_ms, 1000);
    }

    #[test]
    fn stopwatch_records_its_three_buttons_and_closes_its_clip() {
        let sw = Stopwatch::new();
        let mut f: Frame = Frame::new(400.0, 500.0);
        sw.draw(&mut f, 0.0, 0.0, 400.0, 500.0, 0.0);
        let targets: Vec<Target> = f.hits().iter().map(|(t, _)| *t).collect();
        assert!(targets.contains(&Target::SwToggle));
        assert!(targets.contains(&Target::SwLap));
        assert!(targets.contains(&Target::SwReset));
        assert!(
            f.is_balanced(),
            "the lap table's clip and translation must both be closed"
        );
    }

    #[test]
    fn a_lap_scrolled_out_of_the_pane_is_not_clickable() {
        // The lap rows carry no targets of their own, so what this actually
        // proves is the invariant they rely on: the frame trims to the clip, so
        // nothing drawn past the bottom of a scrolled pane can be hit.
        let mut f: Frame = Frame::new(400.0, 300.0);
        f.clip(Rect::new(0.0, 100.0, 400.0, 50.0));
        f.hit(Target::SwLap, Rect::new(0.0, 0.0, 400.0, 400.0));
        f.unclip();
        assert_eq!(f.hit_test(200.0, 10.0), None, "above the clip");
        assert_eq!(f.hit_test(200.0, 120.0), Some(Target::SwLap), "inside it");
        assert_eq!(f.hit_test(200.0, 200.0), None, "below the clip");
    }

    #[test]
    fn test_stopwatch_default() {
        let sw = Stopwatch::default();
        assert_eq!(sw.state, StopwatchState::Stopped);
    }

    // ---- Lap format tests ----

    #[test]
    fn test_lap_format_split() {
        let lap = Lap {
            number: 1,
            split_ms: 62345,
            elapsed_ms: 62345,
        };
        assert_eq!(lap.format_split(), "01:02.345");
    }

    #[test]
    fn test_lap_format_elapsed() {
        let lap = Lap {
            number: 3,
            split_ms: 1000,
            elapsed_ms: 3723456,
        };
        assert_eq!(lap.format_elapsed(), "01:02:03.456");
    }

    // ---- AlarmClockApp tests ----

    #[test]
    fn test_app_create_alarm() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 30);
        assert_eq!(app.alarms.len(), 1);
        assert!(app.find_alarm(id).is_some());
    }

    #[test]
    fn test_app_create_alarm_with_label() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm_with_label(7, 0, "Wake up");
        let alarm = app.find_alarm(id).unwrap();
        assert_eq!(alarm.label, "Wake up");
    }

    #[test]
    fn test_app_delete_alarm() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        assert!(app.delete_alarm(id));
        assert!(app.alarms.is_empty());
    }

    #[test]
    fn test_app_delete_alarm_not_found() {
        let mut app = AlarmClockApp::new();
        assert!(!app.delete_alarm(AlarmId(999)));
    }

    #[test]
    fn test_app_toggle_alarm() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        assert_eq!(app.toggle_alarm(id), Some(false));
        assert_eq!(app.toggle_alarm(id), Some(true));
    }

    #[test]
    fn test_app_toggle_alarm_not_found() {
        let mut app = AlarmClockApp::new();
        assert_eq!(app.toggle_alarm(AlarmId(999)), None);
    }

    #[test]
    fn test_app_next_alarm() {
        let mut app = AlarmClockApp::new();
        app.set_current_time(10, 0, 0, 0);
        app.create_alarm(14, 0);
        app.create_alarm(12, 0);
        let (alarm, mins) = app.next_alarm().unwrap();
        assert_eq!(alarm.hour, 12);
        assert_eq!(mins, 120);
    }

    #[test]
    fn test_app_next_alarm_none() {
        let app = AlarmClockApp::new();
        assert!(app.next_alarm().is_none());
    }

    #[test]
    fn test_app_snooze_alarm() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        app.find_alarm_mut(id).unwrap().ringing = true;
        app.snooze_alarm(id);
        let alarm = app.find_alarm(id).unwrap();
        assert!(!alarm.ringing);
        assert!(alarm.snoozed_remaining.is_some());
    }

    #[test]
    fn test_app_dismiss_alarm() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        app.find_alarm_mut(id).unwrap().ringing = true;
        app.dismiss_alarm(id);
        assert!(!app.find_alarm(id).unwrap().ringing);
    }

    #[test]
    fn test_app_create_timer() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer(300);
        assert_eq!(app.timers.len(), 1);
        assert!(app.find_timer(id).is_some());
    }

    #[test]
    fn test_app_create_timer_preset() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer_preset(5);
        let timer = app.find_timer(id).unwrap();
        assert_eq!(timer.total_seconds, 300);
    }

    #[test]
    fn test_app_create_timer_hms() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer_hms(1, 30, 45);
        let timer = app.find_timer(id).unwrap();
        assert_eq!(timer.total_seconds, 5445);
    }

    #[test]
    fn test_app_delete_timer() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer(60);
        assert!(app.delete_timer(id));
        assert!(app.timers.is_empty());
    }

    #[test]
    fn test_app_start_pause_reset_timer() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer(60);
        app.start_timer(id);
        assert_eq!(app.find_timer(id).unwrap().state, TimerState::Running);
        app.pause_timer(id);
        assert_eq!(app.find_timer(id).unwrap().state, TimerState::Paused);
        app.reset_timer(id);
        assert_eq!(app.find_timer(id).unwrap().state, TimerState::Idle);
    }

    #[test]
    fn test_app_running_timer_count() {
        let mut app = AlarmClockApp::new();
        let id1 = app.create_timer(60);
        let id2 = app.create_timer(120);
        app.create_timer(180);
        app.start_timer(id1);
        app.start_timer(id2);
        assert_eq!(app.running_timer_count(), 2);
    }

    #[test]
    fn test_app_tick_timers() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer(2);
        app.start_timer(id);
        let done = app.tick_timers(); // 2 -> 1
        assert!(done.is_empty());
        let done = app.tick_timers(); // 1 -> 0
        assert_eq!(done.len(), 1);
        assert_eq!(done[0], id);
    }

    #[test]
    fn test_app_tick_alarm_snoozes() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        app.find_alarm_mut(id).unwrap().snoozed_remaining = Some(1);
        let ringing = app.tick_alarm_snoozes(); // 1 -> 0
        assert!(ringing.is_empty());
        let ringing = app.tick_alarm_snoozes(); // 0 -> ringing
        assert_eq!(ringing.len(), 1);
        assert_eq!(ringing[0], id);
    }

    #[test]
    fn test_app_check_alarm_triggers() {
        let mut app = AlarmClockApp::new();
        app.create_alarm(8, 30);
        app.set_current_time(8, 30, 0, 0);
        let triggered = app.check_alarm_triggers();
        assert_eq!(triggered.len(), 1);
    }

    #[test]
    fn test_app_check_alarm_triggers_disabled() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 30);
        app.find_alarm_mut(id).unwrap().enabled = false;
        app.set_current_time(8, 30, 0, 0);
        let triggered = app.check_alarm_triggers();
        assert!(triggered.is_empty());
    }

    #[test]
    fn test_app_check_alarm_triggers_wrong_day() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 30);
        // Only repeat on Tuesday (index 1).
        app.find_alarm_mut(id)
            .unwrap()
            .set_day(Weekday::Tuesday, true);
        // Current day is Monday (index 0).
        app.set_current_time(8, 30, 0, 0);
        let triggered = app.check_alarm_triggers();
        assert!(triggered.is_empty());
    }

    #[test]
    fn test_app_toggle_time_format() {
        let mut app = AlarmClockApp::new();
        assert_eq!(app.time_format, TimeFormat::TwelveHour);
        app.toggle_time_format();
        assert_eq!(app.time_format, TimeFormat::TwentyFourHour);
        app.toggle_time_format();
        assert_eq!(app.time_format, TimeFormat::TwelveHour);
    }

    #[test]
    fn test_app_set_current_time_clamps() {
        let mut app = AlarmClockApp::new();
        app.set_current_time(25, 70, 80, 10);
        assert_eq!(app.current_time, (23, 59, 59));
        assert_eq!(app.current_weekday, 6);
    }

    #[test]
    fn test_app_stopwatch_delegation() {
        let mut app = AlarmClockApp::new();
        app.stopwatch_start();
        assert_eq!(app.stopwatch.state, StopwatchState::Running);
        app.stopwatch.tick(1000);
        app.stopwatch_lap();
        assert_eq!(app.stopwatch.laps.len(), 1);
        app.stopwatch_pause();
        assert_eq!(app.stopwatch.state, StopwatchState::Paused);
        app.stopwatch_reset();
        assert_eq!(app.stopwatch.state, StopwatchState::Stopped);
    }

    // ---- Frame / hit-testing ----

    #[test]
    fn every_tab_draws_and_closes_every_clip_it_opens() {
        for tab in ActiveTab::all() {
            let mut app = AlarmClockApp::new();
            app.active_tab = tab;
            app.create_alarm(8, 0);
            app.create_timer(300);
            app.stopwatch.start();
            app.stopwatch.tick(1500);
            app.stopwatch.lap();

            let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            assert!(!f.hits().is_empty(), "{:?} drew no controls at all", tab);
            assert!(f.is_balanced(), "{:?} left a clip or translation open", tab);
        }
    }

    #[test]
    fn the_tab_bar_is_on_every_tab() {
        for tab in ActiveTab::all() {
            let mut app = AlarmClockApp::new();
            app.active_tab = tab;
            for other in ActiveTab::all() {
                assert!(
                    probe::is_visible(&app, Target::Tab(other)),
                    "{:?} is unreachable from {:?}",
                    other,
                    tab
                );
            }
        }
    }

    #[test]
    fn clicking_a_tab_selects_it() {
        let mut app = AlarmClockApp::new();
        assert_eq!(app.active_tab, ActiveTab::Alarm);
        assert_eq!(
            probe::click(&mut app, Target::Tab(ActiveTab::Timer)),
            Action::Redraw
        );
        assert_eq!(app.active_tab, ActiveTab::Timer);
        // Re-selecting the tab already shown changes nothing, so it must not
        // ask for a repaint.
        assert_eq!(
            probe::click(&mut app, Target::Tab(ActiveTab::Timer)),
            Action::None
        );
    }

    #[test]
    fn clicking_the_clock_swaps_the_time_format() {
        let mut app = AlarmClockApp::new();
        assert_eq!(app.time_format, TimeFormat::TwelveHour);
        assert_eq!(probe::click(&mut app, Target::ClockFormat), Action::Redraw);
        assert_eq!(app.time_format, TimeFormat::TwentyFourHour);
    }

    #[test]
    fn the_window_is_still_usable_at_its_minimum_size() {
        let mut app = AlarmClockApp::new();
        app.create_alarm(8, 0);
        let f = app.frame(MIN_WIDTH, MIN_HEIGHT);
        assert!(f.is_balanced());
        assert!(
            f.hits().iter().any(|(t, _)| *t == Target::AddAlarm),
            "the one control this tab exists for must survive the smallest window"
        );
    }

    #[test]
    fn a_size_below_the_minimum_is_clamped_rather_than_collapsed() {
        // A window driven to 1x1 must not produce negative-width rectangles,
        // which draw as nothing and hit-test as nothing.
        let app = AlarmClockApp::new();
        let f = app.frame(1.0, 1.0);
        assert!(f.is_balanced());
        for (target, rect) in f.hits() {
            assert!(
                rect.w >= 0.0 && rect.h >= 0.0,
                "{:?} has {:?}",
                target,
                rect
            );
        }
    }

    // ---- Alarm list ----

    #[test]
    fn add_alarm_opens_the_editor_rather_than_creating_one_blind() {
        let mut app = AlarmClockApp::new();
        assert_eq!(probe::click(&mut app, Target::AddAlarm), Action::Redraw);
        assert!(app.editor.is_some());
        assert!(
            app.alarms.is_empty(),
            "nothing is created until Save; Cancel must leave no trace"
        );
        assert_eq!(probe::click(&mut app, Target::EditCancel), Action::Redraw);
        assert!(app.alarms.is_empty());
    }

    #[test]
    fn a_new_alarm_defaults_to_the_next_whole_hour() {
        let mut app = AlarmClockApp::new();
        app.set_current_time(7, 43, 12, 0);
        app.open_new_alarm();
        let editor = app.editor.clone().expect("editor open");
        assert_eq!((editor.hour, editor.minute), (8, 0));
    }

    #[test]
    fn saving_the_editor_creates_the_alarm_it_shows() {
        let mut app = AlarmClockApp::new();
        probe::click(&mut app, Target::AddAlarm);
        probe::click(&mut app, Target::EditHour(Step::Up));
        probe::click(&mut app, Target::EditMinute(Step::Down));
        probe::click(&mut app, Target::EditDay(Weekday::Wednesday));
        probe::click(&mut app, Target::EditLabel);
        probe::type_str(&mut app, "Gym");
        let (hour, minute) = {
            let editor = app.editor.as_ref().expect("editor open");
            (editor.hour, editor.minute)
        };

        assert_eq!(probe::click(&mut app, Target::EditSave), Action::Redraw);
        assert!(app.editor.is_none());
        assert_eq!(app.alarms.len(), 1);
        let alarm = app.alarms.first().expect("one alarm");
        assert_eq!((alarm.hour, alarm.minute), (hour, minute));
        assert_eq!(alarm.label, "Gym");
        assert!(alarm.repeats_on(Weekday::Wednesday));
    }

    #[test]
    fn cancelling_an_edit_leaves_the_alarm_untouched() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm_with_label(6, 15, "Work");
        app.open_alarm(id);
        probe::click(&mut app, Target::EditHour(Step::Up));
        probe::click(&mut app, Target::EditLabel);
        probe::type_str(&mut app, "!!!");
        assert_eq!(probe::click(&mut app, Target::EditCancel), Action::Redraw);

        let alarm = app.find_alarm(id).expect("still there");
        assert_eq!((alarm.hour, alarm.minute), (6, 15));
        assert_eq!(alarm.label, "Work");
    }

    #[test]
    fn the_hour_spinner_wraps_at_both_ends() {
        let mut app = AlarmClockApp::new();
        app.editor = Some(AlarmEditor::new_alarm(0, 0));
        probe::click(&mut app, Target::EditHour(Step::Down));
        assert_eq!(app.editor.as_ref().map(|e| e.hour), Some(23));
        probe::click(&mut app, Target::EditHour(Step::Up));
        assert_eq!(app.editor.as_ref().map(|e| e.hour), Some(0));

        app.editor = Some(AlarmEditor::new_alarm(0, 0));
        probe::click(&mut app, Target::EditMinute(Step::Down));
        assert_eq!(app.editor.as_ref().map(|e| e.minute), Some(59));
    }

    #[test]
    fn the_alarm_pill_and_cross_do_what_they_say() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        assert!(app.find_alarm(id).is_some_and(|a| a.enabled));
        assert_eq!(
            probe::click(&mut app, Target::AlarmToggle(id)),
            Action::Redraw
        );
        assert!(app.find_alarm(id).is_some_and(|a| !a.enabled));
        assert_eq!(
            probe::click(&mut app, Target::AlarmDelete(id)),
            Action::Redraw
        );
        assert!(app.find_alarm(id).is_none());
    }

    #[test]
    fn deleting_the_alarm_under_an_open_editor_closes_it() {
        // Otherwise Save would resurrect the row the user just deleted.
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        app.open_alarm(id);
        app.activate(Target::AlarmDelete(id), AlarmClockApp::SIZE);
        assert!(app.editor.is_none());
        assert!(app.alarms.is_empty());
    }

    #[test]
    fn clicking_an_alarm_card_opens_it_for_editing() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm_with_label(9, 45, "Standup");
        assert_eq!(probe::click(&mut app, Target::AlarmRow(id)), Action::Redraw);
        let editor = app.editor.clone().expect("editor open");
        assert_eq!(editor.editing, Some(id));
        assert_eq!((editor.hour, editor.minute), (9, 45));
        assert_eq!(editor.label, "Standup");
    }

    #[test]
    fn snooze_and_dismiss_are_reachable_only_while_ringing() {
        let mut app = AlarmClockApp::new();
        let id = app.create_alarm(8, 0);
        assert!(!probe::is_visible(&app, Target::AlarmSnooze(id)));

        app.set_current_time(8, 0, 0, 0);
        app.check_alarm_triggers();
        assert!(app.find_alarm(id).is_some_and(|a| a.ringing));
        assert!(probe::is_visible(&app, Target::AlarmSnooze(id)));

        assert_eq!(
            probe::click(&mut app, Target::AlarmSnooze(id)),
            Action::Redraw
        );
        let alarm = app.find_alarm(id).expect("still there");
        assert!(!alarm.ringing);
        assert_eq!(alarm.snoozed_remaining, Some(5 * 60));

        assert_eq!(
            probe::click(&mut app, Target::AlarmDismiss(id)),
            Action::Redraw
        );
        let alarm = app.find_alarm(id).expect("still there");
        assert_eq!(alarm.snoozed_remaining, None);
        assert!(!alarm.ringing);
    }

    // ---- Timers ----

    #[test]
    fn every_preset_starts_a_running_timer() {
        for minutes in TIMER_PRESETS {
            let mut app = AlarmClockApp::new();
            app.active_tab = ActiveTab::Timer;
            assert_eq!(
                probe::click(&mut app, Target::Preset(minutes)),
                Action::Redraw
            );
            let timer = app.timers.first().expect("one timer");
            assert_eq!(timer.total_seconds, minutes.saturating_mul(60));
            assert_eq!(
                timer.state,
                TimerState::Running,
                "a preset that had to be started by hand is two clicks for one intention"
            );
        }
    }

    #[test]
    fn the_custom_fields_take_digits_only_and_two_of_them() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Timer;
        probe::click(&mut app, Target::CustomField(HmsField::Minutes));
        probe::type_str(&mut app, "9x99");
        assert_eq!(
            app.custom
                .get(HmsField::Minutes.index())
                .map(String::as_str),
            Some("99")
        );
    }

    #[test]
    fn the_custom_row_starts_the_duration_it_spells() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Timer;
        probe::click(&mut app, Target::CustomField(HmsField::Hours));
        probe::type_str(&mut app, "1");
        probe::click(&mut app, Target::CustomField(HmsField::Seconds));
        probe::type_str(&mut app, "30");
        assert_eq!(app.custom_seconds(), 3600 + 30);

        assert_eq!(probe::click(&mut app, Target::CustomStart), Action::Redraw);
        let timer = app.timers.first().expect("one timer");
        assert_eq!(timer.total_seconds, 3630);
        assert_eq!(timer.state, TimerState::Running);
        assert_eq!(
            app.custom,
            [String::new(), String::new(), String::new()],
            "the fields clear, or the next Start silently repeats the last one"
        );
    }

    #[test]
    fn start_with_empty_fields_does_nothing_at_all() {
        // A zero-second timer is created already finished, which is
        // indistinguishable from the button having malfunctioned.
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Timer;
        assert_eq!(probe::click(&mut app, Target::CustomStart), Action::None);
        assert!(app.timers.is_empty());
    }

    #[test]
    fn timer_card_buttons_route_to_that_timer() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Timer;
        let a = app.create_timer(60);
        let b = app.create_timer(120);

        probe::click(&mut app, Target::TimerToggle(b));
        assert_eq!(app.find_timer(a).map(|t| t.state), Some(TimerState::Idle));
        assert_eq!(
            app.find_timer(b).map(|t| t.state),
            Some(TimerState::Running)
        );

        probe::click(&mut app, Target::TimerDelete(a));
        assert!(app.find_timer(a).is_none());
        assert!(
            app.find_timer(b).is_some(),
            "deleting by id must survive the list reordering under it"
        );
    }

    // ---- Stopwatch ----

    #[test]
    fn the_stopwatch_buttons_drive_the_stopwatch() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Stopwatch;
        assert_eq!(probe::click(&mut app, Target::SwToggle), Action::Redraw);
        assert_eq!(app.stopwatch.state, StopwatchState::Running);

        app.stopwatch.tick(1_500);
        assert_eq!(probe::click(&mut app, Target::SwLap), Action::Redraw);
        assert_eq!(app.stopwatch.laps.len(), 1);

        assert_eq!(probe::click(&mut app, Target::SwToggle), Action::Redraw);
        assert_eq!(app.stopwatch.state, StopwatchState::Paused);
        // Lap is dimmed while paused and must not record one.
        assert_eq!(probe::click(&mut app, Target::SwLap), Action::None);
        assert_eq!(app.stopwatch.laps.len(), 1);

        assert_eq!(probe::click(&mut app, Target::SwReset), Action::Redraw);
        assert_eq!(app.stopwatch.elapsed_ms, 0);
        assert!(app.stopwatch.laps.is_empty());
    }

    // ---- Ticking ----

    #[test]
    fn the_tick_interval_is_never_none() {
        // The bug this file was wired to fix: with no interval the window is
        // never woken, so countdowns freeze and alarms never fire.
        let mut app = AlarmClockApp::new();
        assert_eq!(app.tick_interval(), Some(TICK_SLOW));
        app.stopwatch.start();
        assert_eq!(app.tick_interval(), Some(TICK_FAST));
        app.stopwatch.pause();
        assert_eq!(app.tick_interval(), Some(TICK_SLOW));
    }

    #[test]
    fn a_countdown_takes_the_same_time_at_either_tick_rate() {
        // The per-second work is banked off measured elapsed time, so a fast
        // tick must not run a timer ten times too quickly.
        for (step, count) in [(500_u64, 20_u64), (50, 200)] {
            let mut app = AlarmClockApp::new();
            let id = app.create_timer(60);
            app.start_timer(id);
            for _ in 0..count {
                app.tick(step);
            }
            assert_eq!(
                app.find_timer(id).map(|t| t.remaining_seconds),
                Some(50),
                "ten seconds of ticks at {} ms",
                step
            );
        }
    }

    #[test]
    fn a_late_tick_advances_by_what_actually_elapsed() {
        let mut app = AlarmClockApp::new();
        app.stopwatch.start();
        let id = app.create_timer(60);
        app.start_timer(id);
        // One tick standing in for three seconds of a stalled machine.
        app.tick(3_000);
        assert_eq!(app.stopwatch.elapsed_ms, 3_000);
        assert_eq!(app.find_timer(id).map(|t| t.remaining_seconds), Some(57));
    }

    #[test]
    fn sub_second_ticks_bank_rather_than_round_away() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer(10);
        app.start_timer(id);
        // Nine ticks of 100 ms is under a second: nothing may move yet.
        for _ in 0..9 {
            app.tick(100);
        }
        assert_eq!(app.find_timer(id).map(|t| t.remaining_seconds), Some(10));
        app.tick(100);
        assert_eq!(app.find_timer(id).map(|t| t.remaining_seconds), Some(9));
    }

    // ---- Keyboard ----

    #[test]
    fn digits_select_tabs_and_tab_cycles_them() {
        let mut app = AlarmClockApp::new();
        probe::key(&mut app, &probe::press(Key::Num3));
        assert_eq!(app.active_tab, ActiveTab::Stopwatch);
        probe::key(&mut app, &probe::press(Key::Tab));
        assert_eq!(app.active_tab, ActiveTab::Alarm, "the cycle wraps");
        probe::key(&mut app, &probe::shift(Key::Tab));
        assert_eq!(app.active_tab, ActiveTab::Stopwatch);
    }

    #[test]
    fn alt_tab_is_the_window_managers_and_not_ours() {
        let mut app = AlarmClockApp::new();
        let mut event = probe::press(Key::Tab);
        event.modifiers.alt = true;
        assert_eq!(probe::key(&mut app, &event), Action::None);
        assert_eq!(app.active_tab, ActiveTab::Alarm);
    }

    #[test]
    fn space_drives_the_stopwatch_only_on_its_own_tab() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Alarm;
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Space)),
            Action::None
        );
        assert_eq!(app.stopwatch.state, StopwatchState::Stopped);

        app.active_tab = ActiveTab::Stopwatch;
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Space)),
            Action::Redraw
        );
        assert_eq!(app.stopwatch.state, StopwatchState::Running);
    }

    #[test]
    fn typing_a_label_is_not_a_pile_of_shortcuts() {
        // The whole reason `focus` is an `Option`: with the label focused, `r`
        // is the letter r and must not reset the stopwatch.
        let mut app = AlarmClockApp::new();
        app.stopwatch.start();
        app.stopwatch.tick(5_000);
        probe::click(&mut app, Target::AddAlarm);
        probe::click(&mut app, Target::EditLabel);
        probe::type_str(&mut app, "run");
        assert_eq!(app.editor.as_ref().map(|e| e.label.as_str()), Some("run"));
        assert_eq!(app.stopwatch.elapsed_ms, 5_000);
        assert_eq!(app.stopwatch.state, StopwatchState::Running);
    }

    #[test]
    fn escape_closes_the_editor_then_stops_doing_anything() {
        let mut app = AlarmClockApp::new();
        probe::click(&mut app, Target::AddAlarm);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Escape)),
            Action::Redraw
        );
        assert!(app.editor.is_none());
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Escape)),
            Action::None,
            "Escape with nothing open must not be reported as a change"
        );
    }

    #[test]
    fn ctrl_q_quits_even_with_a_field_focused() {
        let mut app = AlarmClockApp::new();
        probe::click(&mut app, Target::AddAlarm);
        probe::click(&mut app, Target::EditLabel);
        assert_eq!(probe::key(&mut app, &probe::ctrl(Key::Q)), Action::Quit);
    }

    #[test]
    fn the_label_field_is_bounded() {
        let mut app = AlarmClockApp::new();
        probe::click(&mut app, Target::AddAlarm);
        probe::click(&mut app, Target::EditLabel);
        probe::type_str(&mut app, &"x".repeat(MAX_LABEL_LEN + 20));
        assert_eq!(
            app.editor.as_ref().map(|e| e.label.chars().count()),
            Some(MAX_LABEL_LEN)
        );
    }

    #[test]
    fn clicking_away_from_a_field_drops_the_keyboard() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Timer;
        probe::click(&mut app, Target::CustomField(HmsField::Hours));
        assert_eq!(app.focus, Some(Focus::Custom(HmsField::Hours)));
        let (x, y) = probe::bare_point(&app, AlarmClockApp::SIZE).expect("empty space somewhere");
        assert_eq!(
            app.handle_click(x, y, MouseButton::Left, AlarmClockApp::SIZE),
            Action::Redraw
        );
        assert_eq!(app.focus, None);
    }

    // ---- Scrolling ----

    #[test]
    fn the_wheel_scrolls_the_alarm_list_and_stops_at_both_ends() {
        let mut app = AlarmClockApp::new();
        for hour in 0..20 {
            app.create_alarm(hour, 0);
        }
        let size = AlarmClockApp::SIZE;
        let list = AlarmClockApp::alarm_list_rect(AlarmClockApp::content_rect(size.0, size.1));
        let (x, y) = (list.x + list.w / 2.0, list.y + list.h / 2.0);

        // Up at the top is already as far as it goes.
        assert_eq!(app.scroll(x, y, 1.0, size), Action::None);
        assert_eq!(app.scroll(x, y, -1.0, size), Action::Redraw);
        assert!(app.alarm_scroll > 0.0);

        for _ in 0..500 {
            app.scroll(x, y, -1.0, size);
        }
        let max = app.alarm_content_height() - list.h;
        assert!((app.alarm_scroll - max).abs() < 1.0, "clamped to the end");
        assert_eq!(app.scroll(x, y, -1.0, size), Action::None);
    }

    #[test]
    fn deleting_the_content_under_a_scrolled_pane_pulls_it_back() {
        let mut app = AlarmClockApp::new();
        let mut ids = Vec::new();
        for hour in 0..20 {
            ids.push(app.create_alarm(hour, 0));
        }
        let size = AlarmClockApp::SIZE;
        app.alarm_scroll = 5_000.0;
        app.clamp_scrolls(size.0, size.1);
        for id in ids {
            app.activate(Target::AlarmDelete(id), size);
        }
        assert!(
            app.alarm_scroll.abs() < f32::EPSILON,
            "an emptied list parked past its end shows nothing, with no way back"
        );
    }

    #[test]
    fn the_wheel_over_a_pane_that_is_not_there_does_nothing() {
        let mut app = AlarmClockApp::new();
        app.open_new_alarm();
        let size = AlarmClockApp::SIZE;
        assert_eq!(
            app.scroll(size.0 / 2.0, size.1 / 2.0, -1.0, size),
            Action::None,
            "the editor covers the list; scrolling it would move something unseen"
        );
    }

    // ---- Clock ----

    #[test]
    fn the_clock_reads_through_the_zone_and_the_toolkit_calendar() {
        let mut app = AlarmClockApp::new();
        // 2026-08-26T13:45:07Z is a Wednesday.
        app.set_time_from_utc(1_787_751_907);
        assert_eq!(app.current_time, (13, 45, 7));
        assert_eq!(
            Weekday::from_index(app.current_weekday),
            Some(Weekday::Wednesday)
        );
    }

    #[test]
    fn a_pre_epoch_instant_is_still_a_time_of_day() {
        // `%` would give a negative remainder here, which is not a clock
        // reading at all. The argument is not required to come from
        // `SystemTime::now`.
        let mut app = AlarmClockApp::new();
        app.set_time_from_utc(-1);
        assert_eq!(app.current_time, (23, 59, 59));
    }

    #[test]
    fn the_two_weekday_calendars_are_reconciled_in_one_place() {
        // `guitk::date` counts from Sunday and this crate counts from Monday.
        // An alarm set for "Mon" that fires on Sunday is what an inline second
        // copy of this subtraction buys.
        assert_eq!(weekday_index(guitk::date::Weekday::Monday), 0);
        assert_eq!(weekday_index(guitk::date::Weekday::Sunday), 6);
        assert_eq!(weekday_index(guitk::date::Weekday::Saturday), 5);
    }

    #[test]
    fn the_next_alarm_line_reads_in_hours_and_minutes() {
        assert_eq!(humanise_minutes(0), "under a minute");
        assert_eq!(humanise_minutes(45), "45 min");
        assert_eq!(humanise_minutes(120), "2 h");
        assert_eq!(humanise_minutes(200), "3 h 20 min");
    }

    #[test]
    fn every_control_the_frame_records_is_reachable_by_name() {
        // Guards against a hit box recorded for a target the router forgot: a
        // control that is drawn, is clickable, and does nothing.
        let mut app = AlarmClockApp::new();
        app.create_alarm(8, 0);
        app.create_timer(300);
        app.stopwatch.start();
        let mut seen: Vec<String> = Vec::new();
        for tab in ActiveTab::all() {
            app.active_tab = tab;
            seen.extend(probe::control_names(&app));
        }
        app.open_new_alarm();
        app.active_tab = ActiveTab::Alarm;
        seen.extend(probe::control_names(&app));
        for name in [
            "Tab",
            "ClockFormat",
            "AddAlarm",
            "AlarmRow",
            "AlarmToggle",
            "AlarmDelete",
            "EditHour",
            "EditMinute",
            "EditLabel",
            "EditDay",
            "EditSound",
            "EditSnooze",
            "EditSave",
            "EditCancel",
            "Preset",
            "CustomField",
            "CustomStart",
            "TimerRow",
            "TimerToggle",
            "TimerReset",
            "TimerDelete",
            "SwToggle",
            "SwLap",
            "SwReset",
        ] {
            assert!(
                seen.iter().any(|s| s.starts_with(name)),
                "{} is never drawn anywhere",
                name
            );
        }
    }

    #[test]
    fn test_app_multiple_alarms_unique_ids() {
        let mut app = AlarmClockApp::new();
        let id1 = app.create_alarm(8, 0);
        let id2 = app.create_alarm(9, 0);
        let id3 = app.create_alarm(10, 0);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_app_multiple_timers_unique_ids() {
        let mut app = AlarmClockApp::new();
        let id1 = app.create_timer(60);
        let id2 = app.create_timer(120);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_app_default() {
        let app = AlarmClockApp::default();
        assert_eq!(app.active_tab, ActiveTab::Alarm);
        assert!(app.alarms.is_empty());
        assert!(app.timers.is_empty());
    }

    #[test]
    fn test_app_finished_timer_count() {
        let mut app = AlarmClockApp::new();
        let id = app.create_timer(1);
        app.start_timer(id);
        app.tick_timers(); // 1 -> 0
        assert_eq!(app.finished_timer_count(), 1);
    }

    // ---- Utility function tests ----

    #[test]
    fn test_format_duration_hms_seconds_only() {
        assert_eq!(format_duration_hms(45), "00:45");
    }

    #[test]
    fn test_format_duration_hms_minutes() {
        assert_eq!(format_duration_hms(125), "02:05");
    }

    #[test]
    fn test_format_duration_hms_hours() {
        assert_eq!(format_duration_hms(3661), "01:01:01");
    }

    #[test]
    fn test_format_duration_hms_zero() {
        assert_eq!(format_duration_hms(0), "00:00");
    }

    #[test]
    fn test_format_duration_ms_short() {
        assert_eq!(format_duration_ms(1234), "00:01.234");
    }

    #[test]
    fn test_format_duration_ms_minutes() {
        assert_eq!(format_duration_ms(65123), "01:05.123");
    }

    #[test]
    fn test_format_duration_ms_hours() {
        assert_eq!(format_duration_ms(3723456), "01:02:03.456");
    }

    #[test]
    fn test_format_duration_ms_zero() {
        assert_eq!(format_duration_ms(0), "00:00.000");
    }

    #[test]
    fn test_parse_duration_hms_mm_ss() {
        assert_eq!(parse_duration_hms("02:30"), Some(150));
    }

    #[test]
    fn test_parse_duration_hms_hh_mm_ss() {
        assert_eq!(parse_duration_hms("1:30:45"), Some(5445));
    }

    #[test]
    fn test_parse_duration_hms_invalid_seconds() {
        assert_eq!(parse_duration_hms("1:70"), None);
    }

    #[test]
    fn test_parse_duration_hms_invalid_format() {
        assert_eq!(parse_duration_hms("abc"), None);
    }

    #[test]
    fn test_parse_duration_hms_empty() {
        assert_eq!(parse_duration_hms(""), None);
    }

    #[test]
    fn test_parse_duration_hms_too_many_parts() {
        assert_eq!(parse_duration_hms("1:2:3:4"), None);
    }

    #[test]
    fn test_parse_duration_hms_zero() {
        assert_eq!(parse_duration_hms("0:0"), Some(0));
    }

    // ---- Progress ring tests ----

    #[test]
    fn test_progress_ring_empty() {
        let cmds = render_progress_ring(100.0, 100.0, 40.0, 4.0, 0.0, SURFACE2, BLUE);
        // Should have exactly the track segments (RING_SEGMENTS).
        assert_eq!(cmds.len(), RING_SEGMENTS);
    }

    #[test]
    fn test_progress_ring_full() {
        let cmds = render_progress_ring(100.0, 100.0, 40.0, 4.0, 1.0, SURFACE2, BLUE);
        // Track + all filled segments.
        assert_eq!(cmds.len(), RING_SEGMENTS * 2);
    }

    #[test]
    fn test_progress_ring_half() {
        let cmds = render_progress_ring(100.0, 100.0, 40.0, 4.0, 0.5, SURFACE2, BLUE);
        let filled = RING_SEGMENTS / 2;
        assert_eq!(cmds.len(), RING_SEGMENTS + filled);
    }

    #[test]
    fn test_progress_ring_clamp_over() {
        let cmds = render_progress_ring(100.0, 100.0, 40.0, 4.0, 1.5, SURFACE2, BLUE);
        assert_eq!(cmds.len(), RING_SEGMENTS * 2);
    }

    #[test]
    fn test_progress_ring_clamp_negative() {
        let cmds = render_progress_ring(100.0, 100.0, 40.0, 4.0, -0.5, SURFACE2, BLUE);
        assert_eq!(cmds.len(), RING_SEGMENTS);
    }

    // ---- ActiveTab tests ----

    #[test]
    fn test_active_tab_label() {
        assert_eq!(ActiveTab::Alarm.label(), "Alarm");
        assert_eq!(ActiveTab::Timer.label(), "Timer");
        assert_eq!(ActiveTab::Stopwatch.label(), "Stopwatch");
    }

    #[test]
    fn test_active_tab_all() {
        let tabs = ActiveTab::all();
        assert_eq!(tabs.len(), 3);
    }

    #[test]
    fn test_active_tab_default() {
        assert_eq!(ActiveTab::default(), ActiveTab::Alarm);
    }

    // ---- Parse duration edge cases ----

    #[test]
    fn test_parse_duration_hms_hh_invalid_minutes() {
        assert_eq!(parse_duration_hms("1:60:00"), None);
    }

    #[test]
    fn test_parse_duration_hms_hh_invalid_seconds() {
        assert_eq!(parse_duration_hms("1:00:60"), None);
    }

    // ========================================================================
    // Geometry sweeps
    // ========================================================================
    //
    // Three rules held over the whole picture, added after all three scrolling
    // panes were found walking their entire collection under a clip and
    // trusting the clip to tidy up after them. It does not tidy up. A clip
    // stops the *renderer* showing what is outside it, and `Frame::hit` drops
    // a control that lands out there -- so an overrun leaves no trace in the
    // hit boxes, and none in the text either for an app that draws its runs
    // through `guitk::put_text`, which refuses to emit one the clip in force
    // cannot show.
    //
    // This app does not draw through `put_text`. Its own `text` helper, at the
    // top of this file, pushes a `RenderCommand::Text` unconditionally. So a
    // lap row four hundred points below the table went into the picture as a
    // label claiming to be on screen, and every test that read the picture to
    // find out what the user could see was reading a lie. Nothing here caught
    // it across 175 tests, because none of them asked where the paint landed.
    // (known-issues.md, Lesson 107; C-ALARMCLOCK-SCROLLS-BY-CLIP-ALONE.)

    /// Window sizes the sweeps below run at.
    ///
    /// Two of them are below the minimum on one axis or both, because `frame`
    /// clamps up to `MIN_WIDTH`/`MIN_HEIGHT` and a sweep that never asked for a
    /// smaller window would not notice if that clamp went away.
    const GRID: [(f32, f32); 10] = [
        (100.0, 60.0),
        (360.0, 200.0),
        (MIN_WIDTH, MIN_HEIGHT),
        (360.0, 400.0),
        (400.0, 320.0),
        (480.0, 360.0),
        (520.0, 800.0),
        (640.0, 480.0),
        (900.0, 340.0),
        (1280.0, 900.0),
    ];

    /// Apps holding more than any window in `GRID` can show.
    ///
    /// An empty app is worth nothing to these sweeps -- the fault needs
    /// something to overrun with -- so every state below holds more rows than
    /// the tallest window here has room for. Each list appears twice, once at
    /// rest and once parked at the far end of its travel, because the top edge
    /// and the bottom edge fail differently: at rest only the bottom can
    /// overrun, and scrolled, only the top.
    fn states() -> Vec<(&'static str, AlarmClockApp)> {
        let mut out: Vec<(&'static str, AlarmClockApp)> = Vec::new();

        out.push(("nothing at all", AlarmClockApp::new()));

        for (name, scroll) in [("alarms", 0.0_f32), ("alarms scrolled", 10_000.0)] {
            let mut app = AlarmClockApp::new();
            app.active_tab = ActiveTab::Alarm;
            for i in 0..30_u8 {
                app.create_alarm_with_label(i % 24, (i.wrapping_mul(7)) % 60, "get up");
            }
            app.alarm_scroll = scroll;
            out.push((name, app));
        }

        // A ringing alarm grows a strip for Snooze and Dismiss, so the rows in
        // this list are not all the same height. That is the case the alarm
        // pane's guard has to get right on its own: it cannot step by a
        // constant and test the result, it has to ask each card how tall it is.
        let mut mixed = AlarmClockApp::new();
        for i in 0..30_u8 {
            let id = mixed.create_alarm(i % 24, 0);
            if i % 3 == 0 {
                if let Some(alarm) = mixed.find_alarm_mut(id) {
                    alarm.ringing = true;
                }
            }
        }
        out.push(("alarms, every third one ringing", mixed));

        for (name, scroll) in [("timers", 0.0_f32), ("timers scrolled", 10_000.0)] {
            let mut app = AlarmClockApp::new();
            app.active_tab = ActiveTab::Timer;
            for i in 0..30_u32 {
                app.create_timer(60u32.saturating_add(i.saturating_mul(30)));
            }
            app.timer_scroll = scroll;
            out.push((name, app));
        }

        for (name, scroll) in [("laps", 0.0_f32), ("laps scrolled", 10_000.0)] {
            let mut app = AlarmClockApp::new();
            app.active_tab = ActiveTab::Stopwatch;
            app.stopwatch.start();
            for _ in 0..40 {
                app.stopwatch.tick(1_234);
                app.stopwatch.lap();
            }
            app.lap_scroll = scroll;
            out.push((name, app));
        }

        let mut editing = AlarmClockApp::new();
        for i in 0..30_u8 {
            editing.create_alarm(i % 24, 0);
        }
        editing.editor = Some(AlarmEditor::new_alarm(7, 30));
        out.push(("the editor open over a full list", editing));

        out
    }

    /// Every filled box in the frame, in *window* coordinates, paired with the
    /// clip that was in force when it was drawn.
    ///
    /// This began as a copy of the same helper in `contacts`, which reads a
    /// command's coordinates straight out of the tree -- correct there, because
    /// that app never translates. All three panes here draw under a
    /// `PushTranslate`, so a command's own numbers are in the scrolled space
    /// and mean nothing until the offset in force is added back. `Frame::push`
    /// performs exactly this conversion to keep its clip stack in window
    /// coordinates; this is that arithmetic repeated on the reading side.
    fn fills_clipped(frame: &Frame, size: (f32, f32)) -> Vec<Fill> {
        painted(frame, size).fills
    }

    /// A filled box, in window coordinates, and the clip in force when it was
    /// drawn.
    struct Fill {
        rect: Rect,
        clip: Rect,
    }

    /// A run of text, in window coordinates, and the clip in force when it was
    /// drawn.
    struct Run {
        text: String,
        /// The run's box: as wide as the `max_width` it was bounded to, as tall
        /// as its font.
        area: Rect,
        clip: Rect,
        /// Whether the run carried a `max_width` at all. An unbounded run has
        /// an infinitely wide `area`, which would fail the sideways test with a
        /// message about geometry when the fault is that it has no bound.
        bounded: bool,
    }

    /// Everything one walk of the command list found.
    struct Painted {
        fills: Vec<Fill>,
        runs: Vec<Run>,
    }

    /// Whether the fill at `fills[i]`, which lies wholly outside `boundary`, is
    /// excused for it.
    ///
    /// The unit of "do not draw this" is the *item*, and an item is drawn whole
    /// or not at all. A two-point sliver of a timer card at the bottom edge
    /// still paints its progress ring and its delete button in full, ninety
    /// points below the cut, and that is correct: a card that starts inside the
    /// pane is a card the pane is showing. So a fill wholly outside is excused
    /// exactly when some other fill drawn *under the same clip* encloses it and
    /// is itself at least partly inside. Nothing else is -- a row four hundred
    /// points below the pane has no such parent, because the only fill
    /// enclosing it is its own card background, which is just as invisible.
    ///
    /// Shared by both sweeps that ask the question, because a rule stated twice
    /// is a rule that drifts. They differ only in what `boundary` is: the clip
    /// that was in force, or the box the pass was handed.
    fn carried(fills: &[Fill], i: usize, boundary: Rect) -> bool {
        let Some(subject) = fills.get(i) else {
            return false;
        };
        fills.iter().enumerate().any(|(j, outer)| {
            j != i
                && outer.clip == subject.clip
                && boundary.intersect(outer.rect).is_some()
                && outer.rect.intersect(subject.rect) == Some(subject.rect)
        })
    }

    /// Every run of text in the frame, with the clip that was in force.
    fn text_runs_clipped(frame: &Frame, size: (f32, f32)) -> Vec<Run> {
        painted(frame, size).runs
    }

    /// One walk of the command list producing both lists above.
    ///
    /// One walk and not two, because the bookkeeping is the whole difficulty
    /// and a second copy of it is a second chance to get it wrong: the clip
    /// stack has to be converted into window coordinates as it is pushed, the
    /// translation stack has to be unwound on `PopTranslate` and not merely
    /// accumulated, and the two interact -- a clip pushed inside a translation
    /// is itself in the scrolled space.
    fn painted(frame: &Frame, size: (f32, f32)) -> Painted {
        let window = Rect::new(0.0, 0.0, size.0.max(MIN_WIDTH), size.1.max(MIN_HEIGHT));
        let mut clips: Vec<Rect> = Vec::new();
        let mut stack: Vec<(f32, f32)> = Vec::new();
        let mut offset = (0.0_f32, 0.0_f32);
        let mut fills = Vec::new();
        let mut runs = Vec::new();
        for c in frame.commands() {
            match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => {
                    let next = Rect::new(*x, *y, *width, *height).translated(offset.0, offset.1);
                    let merged = clips
                        .last()
                        .map_or(next, |outer| outer.intersect(next).unwrap_or(Rect::EMPTY));
                    clips.push(merged);
                }
                RenderCommand::PopClip => {
                    clips.pop();
                }
                RenderCommand::PushTranslate { dx, dy } => {
                    stack.push((*dx, *dy));
                    offset.0 += *dx;
                    offset.1 += *dy;
                }
                RenderCommand::PopTranslate => {
                    if let Some((dx, dy)) = stack.pop() {
                        offset.0 -= dx;
                        offset.1 -= dy;
                    }
                }
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => fills.push(Fill {
                    rect: Rect::new(*x, *y, *width, *height).translated(offset.0, offset.1),
                    clip: clips.last().copied().unwrap_or(window),
                }),
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    max_width,
                    ..
                } => {
                    let bound = max_width.unwrap_or(f32::INFINITY);
                    runs.push(Run {
                        text: text.clone(),
                        area: Rect::new(*x, *y, bound, *font_size).translated(offset.0, offset.1),
                        clip: clips.last().copied().unwrap_or(window),
                        bounded: max_width.is_some(),
                    });
                }
                _ => {}
            }
        }
        Painted { fills, runs }
    }

    #[test]
    fn nothing_is_painted_entirely_outside_the_clip_in_force() {
        // The rule the three panes broke. A clip makes what is outside it
        // invisible; it does not make it free, and it does not turn a picture
        // claiming to have painted a lap row four hundred points below the
        // table into an honest one.
        //
        // *Entirely* outside, not partly: a row straddling the bottom edge is
        // rightly half drawn and half cut, and so is one straddling the top of
        // a scrolled pane. What nothing may be is wholly invisible.
        //
        // With one exemption, stated once in `carried` and shared with the
        // pass sweep below rather than written out twice.
        for (w, h) in GRID {
            for (name, mut app) in states() {
                app.clamp_scrolls(w, h);
                let frame = app.frame(w, h);
                let fills = fills_clipped(&frame, (w, h));
                for (i, fill) in fills.iter().enumerate() {
                    let (rect, clip) = (fill.rect, fill.clip);
                    if rect.is_empty() || clip.intersect(rect).is_some() {
                        continue;
                    }
                    assert!(
                        carried(&fills, i, clip),
                        "{name} at {w}x{h}: a filled box at {rect:?} is painted entirely \
                         outside the clip {clip:?} that was in force, and no item drawn \
                         under that clip carries it"
                    );
                }
            }
        }
    }

    #[test]
    fn every_run_of_text_is_bounded_and_inside_the_window() {
        // Two rules, and the second is the one the panes broke.
        //
        // Bounded: a run with no `max_width` runs as far as the string is long
        // and over whatever is beside it. Every run in this file goes through
        // the `text` helper, which takes the bound as an argument -- so the way
        // to lose one is to pass `f32::INFINITY` or a constant unrelated to the
        // box, not to forget the field.
        //
        // Inside: sideways a run must fit its clip outright, because nothing
        // here scrolls sideways, so a run hanging over the edge is a run cut in
        // half. Vertically it need only be *partly* inside, because a row at
        // either edge of a scrolled pane is meant to be half drawn. What is
        // forbidden is a run wholly outside -- ink nobody can ever see, and, in
        // this app, ink that goes into the picture regardless and tells anyone
        // reading it that a label is on screen.
        for (w, h) in GRID {
            for (name, mut app) in states() {
                app.clamp_scrolls(w, h);
                let frame = app.frame(w, h);
                for Run {
                    text,
                    area: run,
                    clip,
                    bounded,
                } in text_runs_clipped(&frame, (w, h))
                {
                    assert!(
                        bounded,
                        "{name} at {w}x{h}: {text:?} is drawn with no max_width, so it runs \
                         as far as the string is long and over whatever is beside it"
                    );
                    assert!(
                        run.w.is_finite() && run.w >= 0.0,
                        "{name} at {w}x{h}: {text:?} is bounded to {}",
                        run.w
                    );
                    if clip.is_empty() {
                        // A pane squeezed to nothing clips everything; there is
                        // no box left to be inside of. The rule above still
                        // applies and is what this branch keeps checking.
                        continue;
                    }
                    assert!(
                        run.x >= clip.x - 0.01 && run.right() <= clip.right() + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {}..{} across {clip:?}",
                        run.x,
                        run.right()
                    );
                    assert!(
                        run.bottom() > clip.y - 0.01 && run.y < clip.bottom() + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {}..{} down {clip:?}, which it \
                         misses entirely -- it is drawn where nothing can see it",
                        run.y,
                        run.bottom()
                    );
                }
            }
        }
    }

    /// One drawing pass: the app it belongs to, the frame it writes into, and
    /// the box it is told to stay inside.
    type Pass = fn(&AlarmClockApp, &mut Frame, Rect);

    /// The stopwatch draws from six loose numbers rather than a rect, so it
    /// needs a shim to sit in the table beside the other two.
    fn stopwatch_pass(app: &AlarmClockApp, f: &mut Frame, area: Rect) {
        app.stopwatch
            .draw(f, area.x, area.y, area.w, area.h, app.lap_scroll);
    }

    fn passes() -> Vec<(&'static str, Pass)> {
        vec![
            ("alarm tab", AlarmClockApp::draw_alarm_tab),
            ("timer tab", AlarmClockApp::draw_timer_tab),
            ("stopwatch", stopwatch_pass),
        ]
    }

    #[test]
    fn no_pass_paints_outside_the_box_it_was_given() {
        // Phrased over *fills*, and it has to be: a clip stops the renderer
        // showing text past the edge and makes `Frame::hit` drop the boxes out
        // there, so a pass that overran would look correct from both. The fill
        // is the only witness left. (known-issues.md, Lesson 107.)
        //
        // "Reaches the box", not "lies inside it", which is the stricter form
        // the same sweep takes in `dbviewer`. The difference is that these
        // three panes scroll: an item straddling the pane's top edge is drawn
        // starting above it, on purpose, and demanding containment would fail
        // on correct code. What is forbidden is paint that never touches the
        // box at all -- which is exactly what walking a whole collection with
        // no edge test produces, and is what all three of these did.
        // The boxes are the ones `content_rect` actually produces, not a list
        // of cruel little rectangles. `dbviewer`'s copy of this sweep hands its
        // passes 60x18 to prove they clamp, and that is right there because its
        // passes are handed panes carved out of a sidebar split that really can
        // collapse. Here there is one box, `content_rect`, and `frame` clamps
        // the window to 360x320 before deriving it -- so a 60x18 content area
        // is a state the program cannot enter, and a failure at that size would
        // be a report about arithmetic no user can reach. It does not weaken
        // the sweep: the overrun this test exists for happened at ordinary
        // window sizes with thirty alarms in the list, and every size in `GRID`
        // catches it.
        for (state, mut app) in states() {
            for (w, h) in GRID {
                app.clamp_scrolls(w, h);
                let area = AlarmClockApp::content_rect(w.max(MIN_WIDTH), h.max(MIN_HEIGHT));
                // The frame is far bigger than the box on every side, so an
                // overrun has somewhere to go and is not quietly clamped by the
                // frame's own bounds.
                let size = (area.right() + 400.0, area.bottom() + 400.0);
                for (name, pass) in passes() {
                    let mut f = Frame::new(size.0, size.1);
                    pass(&app, &mut f, area);
                    let fills = fills_clipped(&f, size);
                    for (i, fill) in fills.iter().enumerate() {
                        let filled = fill.rect;
                        if filled.is_empty() || area.intersect(filled).is_some() {
                            continue;
                        }
                        assert!(
                            carried(&fills, i, area),
                            "{state} at {w}x{h}: the {name} pass, given {area:?}, filled \
                             {filled:?}, which does not touch it"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_editor_can_always_be_left_by_pointer() {
        // The editor covers the alarm tab, so Save and Cancel are the only two
        // ways out that a pointer has. Laid out at its natural height the stack
        // is 308 points tall and the content area of a window at this app's own
        // minimum size is 248 -- so both buttons were painted below the panel,
        // where the clip in force hid them and `Frame::hit` dropped their boxes
        // for having nothing visible. The editor was a trap.
        //
        // Asked through `frame`, not through a pass, because the claim is about
        // what a *user* can reach: the whole window, at the sizes the window
        // manager can actually give it, with the hit boxes trimmed exactly as
        // the real click path trims them.
        for (w, h) in GRID {
            let mut app = AlarmClockApp::new();
            for i in 0..30u8 {
                app.create_alarm_with_label(i % 24, (i.wrapping_mul(7)) % 60, "get up");
            }
            probe::click_sized(&mut app, Target::AddAlarm, MouseButton::Left, (w, h));
            assert!(app.editor.is_some(), "the editor did not open at {w}x{h}");

            for target in [Target::EditSave, Target::EditCancel] {
                let rect = probe::rect_of_sized(&app, target, (w, h)).unwrap_or_else(|| {
                    panic!("{target:?} has no hit box at {w}x{h}: the editor cannot be left")
                });
                assert!(
                    !rect.is_empty(),
                    "{target:?} at {w}x{h} has an empty hit box: {rect:?}"
                );
            }

            // And the box does what it names, at that size.
            probe::click_sized(&mut app, Target::EditCancel, MouseButton::Left, (w, h));
            assert!(
                app.editor.is_none(),
                "Cancel did not close the editor at {w}x{h}"
            );
        }
    }

    #[test]
    fn the_editor_is_laid_out_inside_the_panel_it_was_given() {
        // The companion to the test above, phrased over fills rather than hit
        // boxes: every rectangle the editor paints lies inside `content`. Hit
        // boxes are trimmed to the clip, so a button that overran by a point
        // would still answer presses -- the fill is what says whether the stack
        // was *solved* for the height it was given or merely clipped to it.
        for (w, h) in GRID {
            let mut app = AlarmClockApp::new();
            app.editor = Some(AlarmEditor::new_alarm(7, 30));
            let content = AlarmClockApp::content_rect(w.max(MIN_WIDTH), h.max(MIN_HEIGHT));
            let size = (content.right() + 400.0, content.bottom() + 400.0);
            let mut f = Frame::new(size.0, size.1);
            let editor = app.editor.as_ref().unwrap();
            app.draw_editor(&mut f, editor, content);
            for fill in fills_clipped(&f, size) {
                let r = fill.rect;
                // Edge by edge rather than `intersect(r) == Some(r)`, which
                // recomputes `h` as `bottom - y` from inexact floats and so can
                // report a rectangle as unequal to itself.
                assert!(
                    r.x >= content.x
                        && r.y >= content.y
                        && r.right() <= content.right()
                        && r.bottom() <= content.bottom(),
                    "at {w}x{h} the editor filled {r:?}, which leaves its panel {content:?}"
                );
            }
        }
    }
}
