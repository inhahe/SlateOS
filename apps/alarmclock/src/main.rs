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

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

#[allow(unused_imports)]
use std::collections::VecDeque;

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
const SKY: Color = Color::from_hex(0x89DCFE);
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

// ============================================================================
// Active tab
// ============================================================================

/// Which tab is currently selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
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
                    _ => hour - 12,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(Default)]
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
        Self { minutes: SNOOZE_OPTIONS[0] }
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
        let alarm_mins = u32::from(self.hour) * 60 + u32::from(self.minute);
        let current_mins = u32::from(current_hour) * 60 + u32::from(current_minute);

        if !self.is_repeating() {
            // One-shot alarm: fires later today, or tomorrow if time passed.
            return if alarm_mins > current_mins {
                Some(alarm_mins - current_mins)
            } else {
                Some(24 * 60 - current_mins + alarm_mins)
            };
        }

        // Repeating alarm: find the nearest enabled day.
        let wd = current_weekday_idx.unwrap_or(0);
        for offset in 0u32..8 {
            let day_idx = (wd + offset as usize) % 7;
            if !self.repeat_days.get(day_idx).copied().unwrap_or(false) {
                continue;
            }
            if offset == 0 && alarm_mins > current_mins {
                return Some(alarm_mins - current_mins);
            } else if offset > 0 {
                return Some(
                    offset * 24 * 60 + alarm_mins - current_mins,
                );
            }
            // offset == 0 but alarm_mins <= current_mins: check subsequent days.
        }
        // Fallback — next week same day.
        Some(7 * 24 * 60 - current_mins + alarm_mins)
    }

    /// Produce render commands for this alarm entry in the alarm list.
    pub fn render(&self, x: f32, y: f32, width: f32, format: TimeFormat) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let item_height = 72.0;
        let bg_color = if self.ringing {
            Color::rgba(RED.r, RED.g, RED.b, 40)
        } else if self.snoozed_remaining.is_some() {
            Color::rgba(YELLOW.r, YELLOW.g, YELLOW.b, 30)
        } else {
            SURFACE0
        };

        // Background card.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: item_height,
            color: bg_color,
            corner_radii: CornerRadii::all(8.0),
        });

        // Time display.
        let time_str = self.format_time(format);
        let time_color = if self.enabled { TEXT_COLOR } else { OVERLAY0 };
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + 12.0,
            text: time_str,
            color: time_color,
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width * 0.6),
        });

        // Label.
        if !self.label.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + 46.0,
                text: self.label.clone(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.5),
            });
        }

        // Repeat summary.
        let summary = self.repeat_summary();
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + 46.0 + if self.label.is_empty() { 0.0 } else { 16.0 },
            text: summary,
            color: OVERLAY1,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });

        // Enable/disable toggle indicator.
        let toggle_x = x + width - 56.0;
        let toggle_y = y + 24.0;
        let toggle_w = 40.0;
        let toggle_h = 22.0;
        let toggle_color = if self.enabled { BLUE } else { SURFACE2 };
        cmds.push(RenderCommand::FillRect {
            x: toggle_x,
            y: toggle_y,
            width: toggle_w,
            height: toggle_h,
            color: toggle_color,
            corner_radii: CornerRadii::all(toggle_h / 2.0),
        });
        let knob_x = if self.enabled {
            toggle_x + toggle_w - toggle_h + 2.0
        } else {
            toggle_x + 2.0
        };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: toggle_y + 2.0,
            width: toggle_h - 4.0,
            height: toggle_h - 4.0,
            color: TEXT_COLOR,
            corner_radii: CornerRadii::all((toggle_h - 4.0) / 2.0),
        });

        cmds
    }
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

    /// Produce render commands for this timer entry in the timer list.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let item_height = 120.0;
        let bg_color = match self.state {
            TimerState::Finished => Color::rgba(RED.r, RED.g, RED.b, 40),
            TimerState::Running => SURFACE0,
            TimerState::Paused => Color::rgba(YELLOW.r, YELLOW.g, YELLOW.b, 20),
            TimerState::Idle => SURFACE0,
        };

        // Background card.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: item_height,
            color: bg_color,
            corner_radii: CornerRadii::all(8.0),
        });

        // Timer label.
        if !self.label.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + 8.0,
                text: self.label.clone(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
            });
        }

        let text_y = if self.label.is_empty() { y + 16.0 } else { y + 28.0 };

        // Remaining time.
        let time_str = self.format_remaining();
        let time_color = match self.state {
            TimerState::Finished => RED,
            TimerState::Paused => YELLOW,
            _ => TEXT_COLOR,
        };
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: text_y,
            text: time_str,
            color: time_color,
            font_size: 32.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width * 0.6),
        });

        // Total time indicator.
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: text_y + 40.0,
            text: format!("of {}", self.format_total()),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });

        // Progress bar.
        let bar_x = x + PADDING;
        let bar_y = text_y + 60.0;
        let bar_w = width - PADDING * 2.0;
        let bar_h = 6.0;
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE2,
            corner_radii: CornerRadii::all(3.0),
        });
        let fill_w = bar_w * self.progress();
        let fill_color = match self.state {
            TimerState::Finished => RED,
            TimerState::Paused => YELLOW,
            _ => BLUE,
        };
        if fill_w > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: fill_w,
                height: bar_h,
                color: fill_color,
                corner_radii: CornerRadii::all(3.0),
            });
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
        cmds.push(RenderCommand::Text {
            x: x + width - 80.0,
            y: y + 12.0,
            text: badge_text.to_string(),
            color: badge_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(72.0),
        });

        cmds
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
        let angle1 = 2.0 * core::f32::consts::PI * ((i + 1) as f32) / (RING_SEGMENTS as f32);
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
        let angle1 =
            offset + 2.0 * core::f32::consts::PI * ((i + 1) as f32) / (RING_SEGMENTS as f32);
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
        let number = self.laps.len() as u32 + 1;
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
        let avg = total / self.laps.len() as u64;
        Some(LapStats {
            best_ms: best,
            worst_ms: worst,
            average_ms: avg,
            count: self.laps.len(),
        })
    }

    /// Produce render commands for the stopwatch display.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Elapsed time — large display.
        let time_str = self.format_elapsed();
        let time_color = match self.state {
            StopwatchState::Running => GREEN,
            StopwatchState::Paused => YELLOW,
            StopwatchState::Stopped => TEXT_COLOR,
        };
        cmds.push(RenderCommand::Text {
            x: x + width / 2.0 - 100.0,
            y: y + 20.0,
            text: time_str,
            color: time_color,
            font_size: 48.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
        });

        // State indicator.
        let state_text = match self.state {
            StopwatchState::Running => "RUNNING",
            StopwatchState::Paused => "PAUSED",
            StopwatchState::Stopped => "STOPPED",
        };
        cmds.push(RenderCommand::Text {
            x: x + width / 2.0 - 30.0,
            y: y + 76.0,
            text: state_text.to_string(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });

        // Lap stats.
        if let Some(stats) = self.lap_stats() {
            let stats_y = y + 100.0;
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: stats_y,
                text: format!(
                    "Best: {}  Worst: {}  Avg: {}  ({} laps)",
                    format_duration_ms(stats.best_ms),
                    format_duration_ms(stats.worst_ms),
                    format_duration_ms(stats.average_ms),
                    stats.count,
                ),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
            });
        }

        // Lap table header.
        let table_y = y + 130.0;
        if !self.laps.is_empty() {
            // Header line.
            cmds.push(RenderCommand::Line {
                x1: x + PADDING,
                y1: table_y,
                x2: x + width - PADDING,
                y2: table_y,
                color: SURFACE2,
                width: 1.0,
            });

            let col_num_x = x + PADDING;
            let col_split_x = x + 80.0;
            let col_elapsed_x = x + 220.0;

            cmds.push(RenderCommand::Text {
                x: col_num_x,
                y: table_y + 4.0,
                text: "Lap".to_string(),
                color: OVERLAY1,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(60.0),
            });
            cmds.push(RenderCommand::Text {
                x: col_split_x,
                y: table_y + 4.0,
                text: "Split".to_string(),
                color: OVERLAY1,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
            });
            cmds.push(RenderCommand::Text {
                x: col_elapsed_x,
                y: table_y + 4.0,
                text: "Elapsed".to_string(),
                color: OVERLAY1,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
            });

            // Lap rows (most recent first, limited display).
            let stats = self.lap_stats();
            let row_height = 22.0;
            let max_visible = 10;
            let start = if self.laps.len() > max_visible {
                self.laps.len() - max_visible
            } else {
                0
            };
            for (i, lap) in self.laps[start..].iter().rev().enumerate() {
                let row_y = table_y + 24.0 + (i as f32) * row_height;

                // Highlight best/worst laps.
                let split_color = if let Some(ref s) = stats {
                    if self.laps.len() > 1 && lap.split_ms == s.best_ms {
                        GREEN
                    } else if self.laps.len() > 1 && lap.split_ms == s.worst_ms {
                        RED
                    } else {
                        TEXT_COLOR
                    }
                } else {
                    TEXT_COLOR
                };

                cmds.push(RenderCommand::Text {
                    x: col_num_x,
                    y: row_y,
                    text: format!("#{}", lap.number),
                    color: SUBTEXT0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(50.0),
                });
                cmds.push(RenderCommand::Text {
                    x: col_split_x,
                    y: row_y,
                    text: lap.format_split(),
                    color: split_color,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                });
                cmds.push(RenderCommand::Text {
                    x: col_elapsed_x,
                    y: row_y,
                    text: lap.format_elapsed(),
                    color: SUBTEXT0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                });
            }
        }

        cmds
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
        self.create_timer(minutes * 60)
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
        self.timers.iter().filter(|t| t.state == TimerState::Running).count()
    }

    /// Count finished timers.
    pub fn finished_timer_count(&self) -> usize {
        self.timers.iter().filter(|t| t.state == TimerState::Finished).count()
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
                if alarm.is_repeating() && !alarm.repeats_on(
                    Weekday::from_index(self.current_weekday).unwrap_or(Weekday::Monday),
                ) {
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

    // ---- Rendering ----

    /// Produce all render commands for the application window.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Window background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title bar area.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TAB_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Tab buttons.
        let tab_width = WINDOW_WIDTH / 3.0;
        for (i, tab) in ActiveTab::all().iter().enumerate() {
            let tx = i as f32 * tab_width;
            let is_active = *tab == self.active_tab;
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: 0.0,
                    width: tab_width,
                    height: TAB_BAR_HEIGHT,
                    color: SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                });
                // Active indicator line.
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: TAB_BAR_HEIGHT - 3.0,
                    width: tab_width,
                    height: 3.0,
                    color: BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            cmds.push(RenderCommand::Text {
                x: tx + tab_width / 2.0 - 24.0,
                y: TAB_BAR_HEIGHT / 2.0 - 8.0,
                text: tab.label().to_string(),
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_size: 15.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 8.0),
            });
        }

        // Content area.
        let content_y = TAB_BAR_HEIGHT + 8.0;
        let content_width = WINDOW_WIDTH - PADDING * 2.0;
        match self.active_tab {
            ActiveTab::Alarm => {
                cmds.extend(self.render_alarms(PADDING, content_y, content_width));
            }
            ActiveTab::Timer => {
                cmds.extend(self.render_timers(PADDING, content_y, content_width));
            }
            ActiveTab::Stopwatch => {
                cmds.extend(self.stopwatch.render(PADDING, content_y, content_width));
            }
        }

        cmds
    }

    /// Render the alarm list view.
    fn render_alarms(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Current time display.
        let (hour, minute, second) = self.current_time;
        let (display_hour, period) = self.time_format.format_hour(hour);
        let time_str = match period {
            Some(p) => format!("{}:{:02}:{:02} {}", display_hour, minute, second, p),
            None => format!("{:02}:{:02}:{:02}", display_hour, minute, second),
        };
        cmds.push(RenderCommand::Text {
            x: x + width / 2.0 - 80.0,
            y,
            text: time_str,
            color: TEXT_COLOR,
            font_size: 36.0,
            font_weight: FontWeightHint::Light,
            max_width: Some(width),
        });

        // Next alarm indicator.
        let indicator_y = y + 46.0;
        if let Some((alarm, mins)) = self.next_alarm() {
            let hours_until = mins / 60;
            let mins_until = mins % 60;
            let until_str = if hours_until > 0 {
                format!(
                    "Next: {} in {}h {}m",
                    alarm.format_time(self.time_format),
                    hours_until,
                    mins_until,
                )
            } else {
                format!(
                    "Next: {} in {}m",
                    alarm.format_time(self.time_format),
                    mins_until,
                )
            };
            cmds.push(RenderCommand::Text {
                x,
                y: indicator_y,
                text: until_str,
                color: BLUE,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
        } else {
            cmds.push(RenderCommand::Text {
                x,
                y: indicator_y,
                text: "No upcoming alarms".to_string(),
                color: OVERLAY0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
        }

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: indicator_y + 24.0,
            x2: x + width,
            y2: indicator_y + 24.0,
            color: SURFACE1,
            width: 1.0,
        });

        // Alarm entries.
        let mut entry_y = indicator_y + 32.0;
        if self.alarms.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 60.0,
                y: entry_y + 40.0,
                text: "No alarms set".to_string(),
                color: OVERLAY0,
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
        } else {
            for alarm in &self.alarms {
                cmds.extend(alarm.render(x, entry_y, width, self.time_format));
                entry_y += 80.0;
            }
        }

        // "Add Alarm" button.
        let btn_y = entry_y + 8.0;
        let btn_w = 140.0;
        let btn_h = 40.0;
        let btn_x = x + (width - btn_w) / 2.0;
        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: btn_y,
            width: btn_w,
            height: btn_h,
            color: BLUE,
            corner_radii: CornerRadii::all(btn_h / 2.0),
        });
        cmds.push(RenderCommand::Text {
            x: btn_x + btn_w / 2.0 - 36.0,
            y: btn_y + 10.0,
            text: "+ Add Alarm".to_string(),
            color: CRUST,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(btn_w - 16.0),
        });

        cmds
    }

    /// Render the timer list view.
    fn render_timers(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Quick preset buttons.
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Quick Start".to_string(),
            color: SUBTEXT1,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        let preset_y = y + 24.0;
        let preset_btn_w = 56.0;
        let preset_btn_h = 32.0;
        let gap = 8.0;
        for (i, &preset_min) in TIMER_PRESETS.iter().enumerate() {
            let px = x + (i as f32) * (preset_btn_w + gap);
            cmds.push(RenderCommand::FillRect {
                x: px,
                y: preset_y,
                width: preset_btn_w,
                height: preset_btn_h,
                color: SURFACE1,
                corner_radii: CornerRadii::all(preset_btn_h / 2.0),
            });
            cmds.push(RenderCommand::Text {
                x: px + preset_btn_w / 2.0 - 12.0,
                y: preset_y + 7.0,
                text: format!("{}m", preset_min),
                color: TEXT_COLOR,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(preset_btn_w - 8.0),
            });
        }

        // Custom timer input area.
        let custom_y = preset_y + preset_btn_h + 16.0;
        cmds.push(RenderCommand::Text {
            x,
            y: custom_y,
            text: "Custom Timer".to_string(),
            color: SUBTEXT1,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        // HH:MM:SS input fields (placeholder display).
        let input_y = custom_y + 24.0;
        let field_w = 60.0;
        let colon_w = 16.0;
        let fields_total_w = field_w * 3.0 + colon_w * 2.0;
        let start_x = x + (width - fields_total_w) / 2.0;

        for (i, label) in ["HH", "MM", "SS"].iter().enumerate() {
            let fx = start_x + (i as f32) * (field_w + colon_w);
            cmds.push(RenderCommand::FillRect {
                x: fx,
                y: input_y,
                width: field_w,
                height: 40.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: fx + field_w / 2.0 - 10.0,
                y: input_y + 10.0,
                text: label.to_string(),
                color: OVERLAY0,
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(field_w - 8.0),
            });
            if i < 2 {
                let cx = fx + field_w + 2.0;
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: input_y + 8.0,
                    text: ":".to_string(),
                    color: OVERLAY1,
                    font_size: 18.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(colon_w),
                });
            }
        }

        // Separator.
        let sep_y = input_y + 56.0;
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: sep_y,
            x2: x + width,
            y2: sep_y,
            color: SURFACE1,
            width: 1.0,
        });

        // Active timers list.
        let mut timer_y = sep_y + 8.0;
        if self.timers.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 50.0,
                y: timer_y + 20.0,
                text: "No active timers".to_string(),
                color: OVERLAY0,
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
        } else {
            let running = self.running_timer_count();
            let finished = self.finished_timer_count();
            cmds.push(RenderCommand::Text {
                x,
                y: timer_y,
                text: format!(
                    "{} timer{} ({} running, {} finished)",
                    self.timers.len(),
                    if self.timers.len() == 1 { "" } else { "s" },
                    running,
                    finished,
                ),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            timer_y += 20.0;
            for timer in &self.timers {
                cmds.extend(timer.render(x, timer_y, width));
                timer_y += 128.0;
            }
        }

        cmds
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
    let h = total_seconds / 3600;
    let m = (total_seconds % 3600) / 60;
    let s = total_seconds % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

/// Format a duration in milliseconds as `MM:SS.mmm` or `HH:MM:SS.mmm`.
pub fn format_duration_ms(total_ms: u64) -> String {
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let s = total_secs % 60;
    let total_mins = total_secs / 60;
    let m = total_mins % 60;
    let h = total_mins / 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
    } else {
        format!("{:02}:{:02}.{:03}", m, s, ms)
    }
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
// Main entry point (placeholder — real entry wires into OS event loop)
// ============================================================================

fn main() {
    // Placeholder: the real application will be launched by the OS window manager
    // and will integrate with the compositor event loop.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(alarm.tick_snooze());  // 0 -> ringing
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
    fn test_alarm_render_not_empty() {
        let alarm = Alarm::new(AlarmId(1), 8, 30);
        let cmds = alarm.render(0.0, 0.0, 400.0, TimeFormat::TwelveHour);
        assert!(!cmds.is_empty());
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
        assert!(timer.tick());  // 1 -> 0 (finished!)
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
    fn test_timer_render_not_empty() {
        let timer = Timer::new(TimerId(1), 300);
        let cmds = timer.render(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
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
    fn test_stopwatch_render_not_empty() {
        let sw = Stopwatch::new();
        let cmds = sw.render(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
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
        app.find_alarm_mut(id).unwrap().set_day(Weekday::Tuesday, true);
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

    #[test]
    fn test_app_render_not_empty() {
        let app = AlarmClockApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_alarm_tab() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Alarm;
        app.create_alarm(8, 0);
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_app_render_timer_tab() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Timer;
        app.create_timer(300);
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_app_render_stopwatch_tab() {
        let mut app = AlarmClockApp::new();
        app.active_tab = ActiveTab::Stopwatch;
        let cmds = app.render();
        assert!(cmds.len() > 5);
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
}
