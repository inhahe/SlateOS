#![allow(dead_code)]
//! Pomodoro Focus Timer — productivity timer for OurOS.
//!
//! Features:
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
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
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
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Timer Phase ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Work,
    ShortBreak,
    LongBreak,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Work => "Focus",
            Self::ShortBreak => "Short Break",
            Self::LongBreak => "Long Break",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Work => RED,
            Self::ShortBreak => GREEN,
            Self::LongBreak => BLUE,
        }
    }
}

// ── Timer State ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerState {
    Idle,
    Running,
    Paused,
}

// ── Ambient Sound ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AmbientSound {
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

    fn label(self) -> &'static str {
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

    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        let next_idx = (idx.wrapping_add(1)) % Self::ALL.len();
        Self::ALL.get(next_idx).copied().unwrap_or(Self::None)
    }
}

// ── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Settings {
    work_minutes: u32,
    short_break_minutes: u32,
    long_break_minutes: u32,
    rounds_per_set: u32,
    auto_start_breaks: bool,
    auto_start_work: bool,
    daily_goal: u32,
    notification_sound: bool,
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
    fn duration_secs(&self, phase: Phase) -> u32 {
        match phase {
            Phase::Work => self.work_minutes.saturating_mul(60),
            Phase::ShortBreak => self.short_break_minutes.saturating_mul(60),
            Phase::LongBreak => self.long_break_minutes.saturating_mul(60),
        }
    }
}

// ── Focus Log Entry ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct LogEntry {
    phase: Phase,
    task_label: String,
    started_at_ms: u64,
    duration_secs: u32,
    completed: bool,
}

// ── Daily Statistics ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct DayStats {
    date_str: String, // "YYYY-MM-DD" format
    pomodoros_completed: u32,
    total_focus_minutes: u32,
    total_break_minutes: u32,
    goal_met: bool,
}

// ── Application State ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Timer,
    Stats,
    Settings,
    Log,
}

struct PomodoroApp {
    // Timer core
    phase: Phase,
    state: TimerState,
    remaining_secs: u32,
    current_round: u32, // 1-based within the set

    // Settings
    settings: Settings,

    // Task labeling
    current_task: String,
    task_input_active: bool,

    // Sound
    ambient_sound: AmbientSound,

    // Focus log
    log_entries: Vec<LogEntry>,
    log_scroll: usize,

    // Statistics
    daily_stats: Vec<DayStats>,
    current_day: String,
    streak_days: u32,
    total_pomodoros: u32,
    total_focus_minutes: u32,

    // Session tracking
    session_start_ms: u64,
    current_time_ms: u64,

    // UI
    screen: Screen,
    status_message: String,
    width: f32,
    height: f32,

    // Notifications queue
    pending_notification: Option<String>,

    // Settings editing
    settings_cursor: usize,
}

impl PomodoroApp {
    fn new() -> Self {
        let settings = Settings::default();
        let remaining = settings.duration_secs(Phase::Work);
        Self {
            phase: Phase::Work,
            state: TimerState::Idle,
            remaining_secs: remaining,
            current_round: 1,
            settings,
            current_task: String::new(),
            task_input_active: false,
            ambient_sound: AmbientSound::None,
            log_entries: Vec::new(),
            log_scroll: 0,
            daily_stats: Vec::new(),
            current_day: "2026-05-18".into(),
            streak_days: 0,
            total_pomodoros: 0,
            total_focus_minutes: 0,
            session_start_ms: 0,
            current_time_ms: 0,
            screen: Screen::Timer,
            status_message: "Press Space to start".into(),
            width: 600.0,
            height: 500.0,
            pending_notification: None,
            settings_cursor: 0,
        }
    }

    // ── Timer control ──────────────────────────────────────────────────

    fn start(&mut self) {
        if self.state == TimerState::Idle || self.state == TimerState::Paused {
            self.state = TimerState::Running;
            if self.session_start_ms == 0 {
                self.session_start_ms = self.current_time_ms;
            }
            self.status_message = format!("{} — Running", self.phase.label());
        }
    }

    fn pause(&mut self) {
        if self.state == TimerState::Running {
            self.state = TimerState::Paused;
            self.status_message = format!("{} — Paused", self.phase.label());
        }
    }

    fn toggle(&mut self) {
        match self.state {
            TimerState::Idle | TimerState::Paused => self.start(),
            TimerState::Running => self.pause(),
        }
    }

    fn reset_timer(&mut self) {
        self.remaining_secs = self.settings.duration_secs(self.phase);
        self.state = TimerState::Idle;
        self.session_start_ms = 0;
        self.status_message = "Timer reset".into();
    }

    fn skip_phase(&mut self) {
        self.complete_phase(false);
    }

    /// Called each second when running.
    fn tick(&mut self) {
        if self.state != TimerState::Running {
            return;
        }

        if self.remaining_secs > 0 {
            self.remaining_secs = self.remaining_secs.saturating_sub(1);
        }

        if self.remaining_secs == 0 {
            self.complete_phase(true);
        }
    }

    /// Complete the current phase and transition.
    fn complete_phase(&mut self, completed: bool) {
        let phase = self.phase;
        let duration = self
            .settings
            .duration_secs(phase)
            .saturating_sub(self.remaining_secs);

        // Log the entry
        self.log_entries.push(LogEntry {
            phase,
            task_label: self.current_task.clone(),
            started_at_ms: self.session_start_ms,
            duration_secs: duration,
            completed,
        });

        // Update statistics
        if phase == Phase::Work && completed {
            self.total_pomodoros = self.total_pomodoros.saturating_add(1);
            let minutes = duration / 60;
            self.total_focus_minutes = self.total_focus_minutes.saturating_add(minutes);
            self.update_daily_stats(minutes, 0);
        } else if completed {
            let minutes = duration / 60;
            self.update_daily_stats(0, minutes);
        }

        // Determine next phase
        let (next_phase, next_round) = self.next_phase();
        self.phase = next_phase;
        self.current_round = next_round;
        self.remaining_secs = self.settings.duration_secs(next_phase);
        self.session_start_ms = self.current_time_ms;

        // Notification
        let msg = if completed {
            format!("{} complete! Next: {}", phase.label(), next_phase.label())
        } else {
            format!("Skipped {}. Next: {}", phase.label(), next_phase.label())
        };
        self.pending_notification = Some(msg.clone());
        self.status_message = msg;

        // Auto-start next phase?
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
    fn next_phase(&self) -> (Phase, u32) {
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

    /// Update daily stats.
    fn update_daily_stats(&mut self, focus_mins: u32, break_mins: u32) {
        let day = self.current_day.clone();
        let goal = self.settings.daily_goal;

        if let Some(stats) = self.daily_stats.iter_mut().find(|s| s.date_str == day) {
            stats.total_focus_minutes = stats.total_focus_minutes.saturating_add(focus_mins);
            stats.total_break_minutes = stats.total_break_minutes.saturating_add(break_mins);
            if focus_mins > 0 {
                stats.pomodoros_completed = stats.pomodoros_completed.saturating_add(1);
            }
            stats.goal_met = stats.pomodoros_completed >= goal;
        } else {
            let pomodoros = if focus_mins > 0 { 1u32 } else { 0u32 };
            self.daily_stats.push(DayStats {
                date_str: day,
                pomodoros_completed: pomodoros,
                total_focus_minutes: focus_mins,
                total_break_minutes: break_mins,
                goal_met: pomodoros >= goal,
            });
        }

        self.update_streak();
    }

    /// Calculate current streak.
    fn update_streak(&mut self) {
        let mut streak: u32 = 0;
        // Count consecutive days with goal_met from most recent
        for stats in self.daily_stats.iter().rev() {
            if stats.goal_met {
                streak = streak.saturating_add(1);
            } else {
                break;
            }
        }
        self.streak_days = streak;
    }

    /// Get today's stats.
    fn today_stats(&self) -> Option<&DayStats> {
        self.daily_stats.iter().find(|s| s.date_str == self.current_day)
    }

    /// Today's pomodoro count.
    fn today_pomodoros(&self) -> u32 {
        self.today_stats().map_or(0, |s| s.pomodoros_completed)
    }

    // ── Format helpers ─────────────────────────────────────────────────

    fn format_time(secs: u32) -> String {
        let m = secs / 60;
        let s = secs % 60;
        format!("{m:02}:{s:02}")
    }

    fn format_time_long(secs: u32) -> String {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        if h > 0 {
            format!("{h}h {m:02}m {s:02}s")
        } else {
            format!("{m}m {s:02}s")
        }
    }

    fn progress_fraction(&self) -> f32 {
        let total = self.settings.duration_secs(self.phase);
        if total == 0 {
            return 0.0;
        }
        let elapsed = total.saturating_sub(self.remaining_secs);
        (elapsed as f32) / (total as f32)
    }

    // ── Keyboard handling ──────────────────────────────────────────────

    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        // Task input mode
        if self.task_input_active {
            match key {
                "Return" | "Enter" | "Escape" => {
                    self.task_input_active = false;
                    if self.current_task.is_empty() {
                        self.status_message = "No task label set".into();
                    } else {
                        self.status_message =
                            format!("Task: {}", self.current_task);
                    }
                }
                "BackSpace" => {
                    self.current_task.pop();
                }
                _ if key.len() == 1 && !ctrl => {
                    self.current_task.push_str(key);
                }
                _ => {}
            }
            return;
        }

        // Settings screen
        if self.screen == Screen::Settings {
            self.handle_settings_key(key, ctrl);
            return;
        }

        match key {
            // Space: toggle timer
            " " => self.toggle(),
            // R: reset
            "r" if !ctrl => self.reset_timer(),
            // S: skip phase
            "s" if !ctrl => self.skip_phase(),
            // T: set task label
            "t" if !ctrl => {
                self.task_input_active = true;
                self.current_task.clear();
                self.status_message = "Type task label, Enter to confirm".into();
            }
            // A: cycle ambient sound
            "a" if !ctrl => {
                self.ambient_sound = self.ambient_sound.next();
                self.status_message = format!("Sound: {}", self.ambient_sound.label());
            }
            // 1-4: switch screens
            "1" => self.screen = Screen::Timer,
            "2" => self.screen = Screen::Stats,
            "3" => self.screen = Screen::Log,
            "4" => self.screen = Screen::Settings,
            // N: dismiss notification
            "n" if !ctrl => {
                self.pending_notification = None;
            }
            // Page keys for log scroll
            "PageUp" | "Prior" if self.screen == Screen::Log => {
                self.log_scroll = self.log_scroll.saturating_sub(10);
            }
            "PageDown" | "Next" if self.screen == Screen::Log => {
                let max_scroll = self.log_entries.len().saturating_sub(1);
                self.log_scroll = self.log_scroll.saturating_add(10).min(max_scroll);
            }
            _ => {}
        }
    }

    fn handle_settings_key(&mut self, key: &str, _ctrl: bool) {
        let max_cursor: usize = 7;
        match key {
            "Up" => {
                self.settings_cursor = self.settings_cursor.saturating_sub(1);
            }
            "Down" => {
                let next = self.settings_cursor.saturating_add(1);
                if next <= max_cursor {
                    self.settings_cursor = next;
                }
            }
            "Left" => {
                self.adjust_setting(false);
            }
            "Right" => {
                self.adjust_setting(true);
            }
            "1" => self.screen = Screen::Timer,
            "2" => self.screen = Screen::Stats,
            "3" => self.screen = Screen::Log,
            _ => {}
        }
    }

    fn adjust_setting(&mut self, increase: bool) {
        match self.settings_cursor {
            0 => {
                // Work minutes
                if increase {
                    self.settings.work_minutes =
                        self.settings.work_minutes.saturating_add(5).min(120);
                } else {
                    self.settings.work_minutes =
                        self.settings.work_minutes.saturating_sub(5).max(5);
                }
            }
            1 => {
                // Short break
                if increase {
                    self.settings.short_break_minutes =
                        self.settings.short_break_minutes.saturating_add(1).min(30);
                } else {
                    self.settings.short_break_minutes =
                        self.settings.short_break_minutes.saturating_sub(1).max(1);
                }
            }
            2 => {
                // Long break
                if increase {
                    self.settings.long_break_minutes =
                        self.settings.long_break_minutes.saturating_add(5).min(60);
                } else {
                    self.settings.long_break_minutes =
                        self.settings.long_break_minutes.saturating_sub(5).max(5);
                }
            }
            3 => {
                // Rounds per set
                if increase {
                    self.settings.rounds_per_set =
                        self.settings.rounds_per_set.saturating_add(1).min(10);
                } else {
                    self.settings.rounds_per_set =
                        self.settings.rounds_per_set.saturating_sub(1).max(2);
                }
            }
            4 => {
                // Auto-start breaks
                self.settings.auto_start_breaks = !self.settings.auto_start_breaks;
            }
            5 => {
                // Auto-start work
                self.settings.auto_start_work = !self.settings.auto_start_work;
            }
            6 => {
                // Daily goal
                if increase {
                    self.settings.daily_goal =
                        self.settings.daily_goal.saturating_add(1).min(20);
                } else {
                    self.settings.daily_goal =
                        self.settings.daily_goal.saturating_sub(1).max(1);
                }
            }
            7 => {
                // Notification sound
                self.settings.notification_sound = !self.settings.notification_sound;
            }
            _ => {}
        }

        // If timer is idle, update remaining to reflect new settings
        if self.state == TimerState::Idle {
            self.remaining_secs = self.settings.duration_secs(self.phase);
        }
    }

    // ── Rendering ──────────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Tab bar
        self.render_tabs(&mut cmds);

        let content_y: f32 = 36.0;
        let content_h = self.height - 36.0 - 28.0; // tabs + status bar

        match self.screen {
            Screen::Timer => self.render_timer(&mut cmds, content_y, content_h),
            Screen::Stats => self.render_stats(&mut cmds, content_y, content_h),
            Screen::Log => self.render_log(&mut cmds, content_y, content_h),
            Screen::Settings => self.render_settings(&mut cmds, content_y, content_h),
        }

        // Status bar
        self.render_status_bar(&mut cmds);

        // Notification overlay
        if let Some(ref notif) = self.pending_notification {
            self.render_notification(&mut cmds, notif);
        }

        cmds
    }

    fn render_tabs(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: 36.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let tabs = [
            (Screen::Timer, "1: Timer"),
            (Screen::Stats, "2: Stats"),
            (Screen::Log, "3: Log"),
            (Screen::Settings, "4: Settings"),
        ];
        let tab_w: f32 = 120.0;
        for (i, (screen, label)) in tabs.iter().enumerate() {
            let tx = 8.0 + (i as f32) * (tab_w + 4.0);
            let is_active = self.screen == *screen;
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: 4.0,
                    width: tab_w,
                    height: 28.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: tx + 12.0,
                y: 10.0,
                text: label.to_string(),
                font_size: 12.0,
                color: if is_active { TEXT_COLOR } else { OVERLAY0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 24.0),
            });
        }
    }

    fn render_timer(&self, cmds: &mut Vec<RenderCommand>, y: f32, h: f32) {
        let cx = self.width / 2.0;
        let cy = y + h * 0.35;

        // Phase label
        cmds.push(RenderCommand::Text {
            x: cx - 80.0,
            y: y + 20.0,
            text: self.phase.label().to_string(),
            font_size: 20.0,
            color: self.phase.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(160.0),
        });

        // Round indicator
        cmds.push(RenderCommand::Text {
            x: cx - 60.0,
            y: y + 46.0,
            text: format!(
                "Round {}/{}",
                self.current_round, self.settings.rounds_per_set
            ),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });

        // Progress ring (simulated as arc using filled rects)
        let ring_r: f32 = 100.0;
        let ring_w: f32 = 8.0;
        // Outer circle background
        cmds.push(RenderCommand::FillRect {
            x: cx - ring_r,
            y: cy - ring_r,
            width: ring_r * 2.0,
            height: ring_r * 2.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(ring_r),
        });
        // Inner circle (creates ring effect)
        let inner_r = ring_r - ring_w;
        cmds.push(RenderCommand::FillRect {
            x: cx - inner_r,
            y: cy - inner_r,
            width: inner_r * 2.0,
            height: inner_r * 2.0,
            color: BASE,
            corner_radii: CornerRadii::all(inner_r),
        });

        // Progress indicator (simple bar at bottom of ring area)
        let progress = self.progress_fraction();
        let bar_w = ring_r * 2.0 - 20.0;
        let bar_h: f32 = 6.0;
        let bar_x = cx - bar_w / 2.0;
        let bar_y = cy + ring_r + 16.0;
        // Bar background
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        // Bar fill
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w * progress,
            height: bar_h,
            color: self.phase.color(),
            corner_radii: CornerRadii::all(3.0),
        });

        // Timer display (large)
        cmds.push(RenderCommand::Text {
            x: cx - 60.0,
            y: cy - 24.0,
            text: Self::format_time(self.remaining_secs),
            font_size: 48.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });

        // State indicator
        let state_label = match self.state {
            TimerState::Idle => "Ready",
            TimerState::Running => "Running",
            TimerState::Paused => "Paused",
        };
        cmds.push(RenderCommand::Text {
            x: cx - 30.0,
            y: cy + 28.0,
            text: state_label.to_string(),
            font_size: 12.0,
            color: match self.state {
                TimerState::Idle => OVERLAY0,
                TimerState::Running => GREEN,
                TimerState::Paused => YELLOW,
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });

        // Controls help
        let controls_y = bar_y + 24.0;
        cmds.push(RenderCommand::Text {
            x: cx - 140.0,
            y: controls_y,
            text: "[Space] Start/Pause  [R] Reset  [S] Skip  [T] Task  [A] Sound".into(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(280.0),
        });

        // Task label
        if !self.current_task.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx - 100.0,
                y: controls_y + 18.0,
                text: format!("Task: {}", self.current_task),
                font_size: 11.0,
                color: LAVENDER,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }

        // Task input overlay
        if self.task_input_active {
            cmds.push(RenderCommand::FillRect {
                x: cx - 150.0,
                y: controls_y + 36.0,
                width: 300.0,
                height: 30.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx - 140.0,
                y: controls_y + 44.0,
                text: if self.current_task.is_empty() {
                    "Type task label...".into()
                } else {
                    format!("{}|", self.current_task)
                },
                font_size: 12.0,
                color: if self.current_task.is_empty() {
                    OVERLAY0
                } else {
                    TEXT_COLOR
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(280.0),
            });
        }

        // Today's progress
        let bottom_y = y + h - 60.0;
        let today_count = self.today_pomodoros();
        let goal = self.settings.daily_goal;
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: bottom_y,
            text: format!("Today: {today_count}/{goal} pomodoros"),
            font_size: 12.0,
            color: if today_count >= goal { GREEN } else { SUBTEXT1 },
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Streak
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: bottom_y + 18.0,
            text: format!("Streak: {} days", self.streak_days),
            font_size: 11.0,
            color: if self.streak_days > 0 { PEACH } else { OVERLAY0 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Ambient sound
        cmds.push(RenderCommand::Text {
            x: self.width - 180.0,
            y: bottom_y,
            text: format!("Sound: {}", self.ambient_sound.label()),
            font_size: 11.0,
            color: TEAL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(160.0),
        });

        // Total pomodoros
        cmds.push(RenderCommand::Text {
            x: self.width - 180.0,
            y: bottom_y + 18.0,
            text: format!(
                "Total: {} ({}h {}m)",
                self.total_pomodoros,
                self.total_focus_minutes / 60,
                self.total_focus_minutes % 60
            ),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(160.0),
        });
    }

    fn render_stats(&self, cmds: &mut Vec<RenderCommand>, y: f32, _h: f32) {
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: y + 10.0,
            text: "Statistics".into(),
            font_size: 18.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        let mut sy = y + 40.0;

        // Summary cards
        let cards: [(&str, String, Color); 4] = [
            ("Total Pomodoros", format!("{}", self.total_pomodoros), RED),
            (
                "Total Focus Time",
                Self::format_time_long(self.total_focus_minutes.saturating_mul(60)),
                GREEN,
            ),
            (
                "Current Streak",
                format!("{} days", self.streak_days),
                PEACH,
            ),
            (
                "Daily Goal",
                format!(
                    "{}/{}",
                    self.today_pomodoros(),
                    self.settings.daily_goal
                ),
                BLUE,
            ),
        ];

        let card_w: f32 = 130.0;
        for (i, (label, value, color)) in cards.iter().enumerate() {
            let cx = 20.0 + (i as f32) * (card_w + 12.0);
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: sy,
                width: card_w,
                height: 60.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 10.0,
                y: sy + 8.0,
                text: label.to_string(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(card_w - 20.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 10.0,
                y: sy + 26.0,
                text: value.clone(),
                font_size: 18.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(card_w - 20.0),
            });
        }

        sy += 80.0;

        // Daily history
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: sy,
            text: "Daily History".into(),
            font_size: 14.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });
        sy += 22.0;

        if self.daily_stats.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: sy,
                text: "No data yet. Complete your first pomodoro!".into(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 40.0),
            });
        } else {
            // Table header
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: sy,
                text: "Date           Pomos  Focus    Break    Goal".into(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(self.width - 40.0),
            });
            sy += 16.0;

            for stats in self.daily_stats.iter().rev().take(10) {
                let goal_icon = if stats.goal_met { "OK" } else { "--" };
                cmds.push(RenderCommand::Text {
                    x: 20.0,
                    y: sy,
                    text: format!(
                        "{}  {:>5}  {:>4}m  {:>5}m  {}",
                        stats.date_str,
                        stats.pomodoros_completed,
                        stats.total_focus_minutes,
                        stats.total_break_minutes,
                        goal_icon
                    ),
                    font_size: 10.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(self.width - 40.0),
                });
                sy += 14.0;
            }
        }
    }

    fn render_log(&self, cmds: &mut Vec<RenderCommand>, y: f32, h: f32) {
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: y + 10.0,
            text: format!("Focus Log ({} entries)", self.log_entries.len()),
            font_size: 18.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        let start_y = y + 40.0;
        let row_h: f32 = 20.0;
        let visible = ((h - 50.0) / row_h) as usize;

        if self.log_entries.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: start_y,
                text: "No log entries yet".into(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 40.0),
            });
            return;
        }

        // Header
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: start_y,
            text: "Phase        Duration   Task                    Status".into(),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - 40.0),
        });

        let entries_rev: Vec<&LogEntry> = self.log_entries.iter().rev().collect();
        for (i, entry) in entries_rev
            .iter()
            .skip(self.log_scroll)
            .take(visible)
            .enumerate()
        {
            let ey = start_y + 18.0 + (i as f32) * row_h;
            let status = if entry.completed { "Done" } else { "Skip" };
            let task = if entry.task_label.is_empty() {
                "-"
            } else {
                entry.task_label.as_str()
            };
            // Truncate task to 20 chars
            let task_display: String = task.chars().take(20).collect();

            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: ey,
                text: format!(
                    "{:<12} {:>6}   {:<24} {}",
                    entry.phase.label(),
                    Self::format_time(entry.duration_secs),
                    task_display,
                    status
                ),
                font_size: 10.0,
                color: if entry.completed {
                    TEXT_COLOR
                } else {
                    OVERLAY0
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 40.0),
            });
        }
    }

    fn render_settings(&self, cmds: &mut Vec<RenderCommand>, y: f32, _h: f32) {
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: y + 10.0,
            text: "Settings".into(),
            font_size: 18.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: y + 34.0,
            text: "Use Up/Down to navigate, Left/Right to adjust".into(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - 40.0),
        });

        let items: Vec<(&str, String)> = vec![
            (
                "Work Duration",
                format!("{} min", self.settings.work_minutes),
            ),
            (
                "Short Break",
                format!("{} min", self.settings.short_break_minutes),
            ),
            (
                "Long Break",
                format!("{} min", self.settings.long_break_minutes),
            ),
            (
                "Rounds per Set",
                format!("{}", self.settings.rounds_per_set),
            ),
            (
                "Auto-start Breaks",
                if self.settings.auto_start_breaks {
                    "Yes"
                } else {
                    "No"
                }
                .into(),
            ),
            (
                "Auto-start Work",
                if self.settings.auto_start_work {
                    "Yes"
                } else {
                    "No"
                }
                .into(),
            ),
            (
                "Daily Goal",
                format!("{} pomodoros", self.settings.daily_goal),
            ),
            (
                "Notification Sound",
                if self.settings.notification_sound {
                    "On"
                } else {
                    "Off"
                }
                .into(),
            ),
        ];

        let start_y = y + 52.0;
        let row_h: f32 = 32.0;
        for (i, (label, value)) in items.iter().enumerate() {
            let ry = start_y + (i as f32) * row_h;
            let is_selected = i == self.settings_cursor;

            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 16.0,
                    y: ry - 2.0,
                    width: self.width - 32.0,
                    height: row_h - 4.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: 28.0,
                y: ry + 6.0,
                text: label.to_string(),
                font_size: 12.0,
                color: if is_selected { TEXT_COLOR } else { SUBTEXT1 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });

            cmds.push(RenderCommand::Text {
                x: self.width - 200.0,
                y: ry + 6.0,
                text: if is_selected {
                    format!("< {value} >")
                } else {
                    value.clone()
                },
                font_size: 12.0,
                color: if is_selected { BLUE } else { SUBTEXT0 },
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(180.0),
            });
        }
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - 28.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: 28.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: y + 8.0,
            text: self.status_message.clone(),
            font_size: 10.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.6),
        });

        // Timer mini display on right
        let time_str = Self::format_time(self.remaining_secs);
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: y + 8.0,
            text: format!("{} {time_str}", self.phase.label()),
            font_size: 10.0,
            color: self.phase.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(110.0),
        });
    }

    fn render_notification(&self, cmds: &mut Vec<RenderCommand>, text: &str) {
        let nw: f32 = 320.0;
        let nh: f32 = 60.0;
        let nx = (self.width - nw) / 2.0;
        let ny: f32 = 50.0;

        // Shadow
        cmds.push(RenderCommand::BoxShadow {
            x: nx,
            y: ny,
            width: nw,
            height: nh,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 2.0,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(10.0),
        });

        cmds.push(RenderCommand::FillRect {
            x: nx,
            y: ny,
            width: nw,
            height: nh,
            color: SURFACE1,
            corner_radii: CornerRadii::all(10.0),
        });

        cmds.push(RenderCommand::Text {
            x: nx + 16.0,
            y: ny + 12.0,
            text: text.to_string(),
            font_size: 12.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(nw - 32.0),
        });

        cmds.push(RenderCommand::Text {
            x: nx + 16.0,
            y: ny + 34.0,
            text: "Press [N] to dismiss".into(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(nw - 32.0),
        });
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

fn main() {
    let _app = PomodoroApp::new();
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Phase tests ────────────────────────────────────────────────────

    #[test]
    fn test_phase_labels() {
        assert_eq!(Phase::Work.label(), "Focus");
        assert_eq!(Phase::ShortBreak.label(), "Short Break");
        assert_eq!(Phase::LongBreak.label(), "Long Break");
    }

    #[test]
    fn test_phase_colors_distinct() {
        let c1 = Phase::Work.color();
        let c2 = Phase::ShortBreak.color();
        let c3 = Phase::LongBreak.color();
        // At least they shouldn't all be the same
        assert!(c1 != c2 || c2 != c3);
    }

    // ── Settings tests ─────────────────────────────────────────────────

    #[test]
    fn test_default_settings() {
        let s = Settings::default();
        assert_eq!(s.work_minutes, 25);
        assert_eq!(s.short_break_minutes, 5);
        assert_eq!(s.long_break_minutes, 15);
        assert_eq!(s.rounds_per_set, 4);
        assert_eq!(s.daily_goal, 8);
    }

    #[test]
    fn test_duration_secs() {
        let s = Settings::default();
        assert_eq!(s.duration_secs(Phase::Work), 1500);
        assert_eq!(s.duration_secs(Phase::ShortBreak), 300);
        assert_eq!(s.duration_secs(Phase::LongBreak), 900);
    }

    // ── Ambient sound tests ────────────────────────────────────────────

    #[test]
    fn test_ambient_sound_cycle() {
        let mut s = AmbientSound::None;
        s = s.next();
        assert_eq!(s, AmbientSound::Rain);
        s = s.next();
        assert_eq!(s, AmbientSound::Cafe);
    }

    #[test]
    fn test_ambient_sound_wraps() {
        let mut s = AmbientSound::Fireplace;
        s = s.next();
        assert_eq!(s, AmbientSound::None);
    }

    // ── App creation tests ─────────────────────────────────────────────

    #[test]
    fn test_app_creation() {
        let app = PomodoroApp::new();
        assert_eq!(app.phase, Phase::Work);
        assert_eq!(app.state, TimerState::Idle);
        assert_eq!(app.remaining_secs, 1500);
        assert_eq!(app.current_round, 1);
    }

    // ── Timer control tests ────────────────────────────────────────────

    #[test]
    fn test_start() {
        let mut app = PomodoroApp::new();
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn test_pause() {
        let mut app = PomodoroApp::new();
        app.start();
        app.pause();
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn test_toggle() {
        let mut app = PomodoroApp::new();
        app.toggle();
        assert_eq!(app.state, TimerState::Running);
        app.toggle();
        assert_eq!(app.state, TimerState::Paused);
        app.toggle();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn test_reset() {
        let mut app = PomodoroApp::new();
        app.start();
        app.remaining_secs = 100;
        app.reset_timer();
        assert_eq!(app.state, TimerState::Idle);
        assert_eq!(app.remaining_secs, 1500);
    }

    // ── Tick tests ─────────────────────────────────────────────────────

    #[test]
    fn test_tick_decrements() {
        let mut app = PomodoroApp::new();
        app.start();
        let before = app.remaining_secs;
        app.tick();
        assert_eq!(app.remaining_secs, before - 1);
    }

    #[test]
    fn test_tick_does_nothing_when_paused() {
        let mut app = PomodoroApp::new();
        app.state = TimerState::Paused;
        app.remaining_secs = 100;
        app.tick();
        assert_eq!(app.remaining_secs, 100);
    }

    #[test]
    fn test_tick_does_nothing_when_idle() {
        let mut app = PomodoroApp::new();
        app.remaining_secs = 100;
        app.tick();
        assert_eq!(app.remaining_secs, 100);
    }

    #[test]
    fn test_tick_completes_phase() {
        let mut app = PomodoroApp::new();
        app.start();
        app.remaining_secs = 1;
        app.tick();
        // Should have transitioned to ShortBreak
        assert_eq!(app.phase, Phase::ShortBreak);
    }

    // ── Phase transition tests ─────────────────────────────────────────

    #[test]
    fn test_work_to_short_break() {
        let app = PomodoroApp::new();
        let (next, round) = app.next_phase();
        assert_eq!(next, Phase::ShortBreak);
        assert_eq!(round, 1);
    }

    #[test]
    fn test_short_break_to_work() {
        let mut app = PomodoroApp::new();
        app.phase = Phase::ShortBreak;
        app.current_round = 1;
        let (next, round) = app.next_phase();
        assert_eq!(next, Phase::Work);
        assert_eq!(round, 2);
    }

    #[test]
    fn test_work_to_long_break_at_end_of_set() {
        let mut app = PomodoroApp::new();
        app.current_round = 4; // last round
        let (next, round) = app.next_phase();
        assert_eq!(next, Phase::LongBreak);
        assert_eq!(round, 1);
    }

    #[test]
    fn test_long_break_to_work() {
        let mut app = PomodoroApp::new();
        app.phase = Phase::LongBreak;
        let (next, round) = app.next_phase();
        assert_eq!(next, Phase::Work);
        assert_eq!(round, 1);
    }

    #[test]
    fn test_complete_phase_logs_entry() {
        let mut app = PomodoroApp::new();
        app.start();
        app.remaining_secs = 0;
        app.complete_phase(true);
        assert_eq!(app.log_entries.len(), 1);
        assert!(app.log_entries.first().is_some_and(|e| e.completed));
    }

    #[test]
    fn test_skip_phase() {
        let mut app = PomodoroApp::new();
        app.start();
        app.skip_phase();
        assert_eq!(app.phase, Phase::ShortBreak);
        assert_eq!(app.log_entries.len(), 1);
        assert!(app.log_entries.first().is_some_and(|e| !e.completed));
    }

    // ── Statistics tests ───────────────────────────────────────────────

    #[test]
    fn test_update_daily_stats() {
        let mut app = PomodoroApp::new();
        app.update_daily_stats(25, 0);
        assert_eq!(app.daily_stats.len(), 1);
        let stats = app.daily_stats.first().unwrap();
        assert_eq!(stats.pomodoros_completed, 1);
        assert_eq!(stats.total_focus_minutes, 25);
    }

    #[test]
    fn test_update_daily_stats_accumulates() {
        let mut app = PomodoroApp::new();
        app.update_daily_stats(25, 0);
        app.update_daily_stats(25, 0);
        let stats = app.daily_stats.first().unwrap();
        assert_eq!(stats.pomodoros_completed, 2);
        assert_eq!(stats.total_focus_minutes, 50);
    }

    #[test]
    fn test_daily_goal_met() {
        let mut app = PomodoroApp::new();
        app.settings.daily_goal = 2;
        app.update_daily_stats(25, 0);
        assert!(!app.daily_stats.first().unwrap().goal_met);
        app.update_daily_stats(25, 0);
        assert!(app.daily_stats.first().unwrap().goal_met);
    }

    #[test]
    fn test_streak_counting() {
        let mut app = PomodoroApp::new();
        app.settings.daily_goal = 1;
        app.current_day = "2026-05-16".into();
        app.update_daily_stats(25, 0);
        app.current_day = "2026-05-17".into();
        app.update_daily_stats(25, 0);
        app.current_day = "2026-05-18".into();
        app.update_daily_stats(25, 0);
        assert_eq!(app.streak_days, 3);
    }

    #[test]
    fn test_streak_broken() {
        let mut app = PomodoroApp::new();
        app.settings.daily_goal = 1;
        app.current_day = "2026-05-16".into();
        app.update_daily_stats(25, 0);
        // Day with no goal met
        app.current_day = "2026-05-17".into();
        app.update_daily_stats(0, 5); // only break
        app.current_day = "2026-05-18".into();
        app.update_daily_stats(25, 0);
        assert_eq!(app.streak_days, 1); // only today
    }

    #[test]
    fn test_today_pomodoros() {
        let mut app = PomodoroApp::new();
        assert_eq!(app.today_pomodoros(), 0);
        app.update_daily_stats(25, 0);
        assert_eq!(app.today_pomodoros(), 1);
    }

    // ── Format tests ───────────────────────────────────────────────────

    #[test]
    fn test_format_time() {
        assert_eq!(PomodoroApp::format_time(0), "00:00");
        assert_eq!(PomodoroApp::format_time(61), "01:01");
        assert_eq!(PomodoroApp::format_time(1500), "25:00");
        assert_eq!(PomodoroApp::format_time(3599), "59:59");
    }

    #[test]
    fn test_format_time_long() {
        assert_eq!(PomodoroApp::format_time_long(0), "0m 00s");
        assert_eq!(PomodoroApp::format_time_long(3661), "1h 01m 01s");
    }

    #[test]
    fn test_progress_fraction() {
        let mut app = PomodoroApp::new();
        assert_eq!(app.progress_fraction(), 0.0);
        app.remaining_secs = 750; // half of 1500
        assert!((app.progress_fraction() - 0.5).abs() < 0.01);
        app.remaining_secs = 0;
        assert!((app.progress_fraction() - 1.0).abs() < 0.01);
    }

    // ── Key handling tests ─────────────────────────────────────────────

    #[test]
    fn test_key_space_starts() {
        let mut app = PomodoroApp::new();
        app.handle_key(" ", false, false);
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn test_key_r_resets() {
        let mut app = PomodoroApp::new();
        app.start();
        app.remaining_secs = 100;
        app.handle_key("r", false, false);
        assert_eq!(app.state, TimerState::Idle);
        assert_eq!(app.remaining_secs, 1500);
    }

    #[test]
    fn test_key_s_skips() {
        let mut app = PomodoroApp::new();
        app.handle_key("s", false, false);
        assert_eq!(app.phase, Phase::ShortBreak);
    }

    #[test]
    fn test_key_a_cycles_sound() {
        let mut app = PomodoroApp::new();
        assert_eq!(app.ambient_sound, AmbientSound::None);
        app.handle_key("a", false, false);
        assert_eq!(app.ambient_sound, AmbientSound::Rain);
    }

    #[test]
    fn test_key_number_switches_screen() {
        let mut app = PomodoroApp::new();
        app.handle_key("2", false, false);
        assert_eq!(app.screen, Screen::Stats);
        app.handle_key("3", false, false);
        assert_eq!(app.screen, Screen::Log);
        app.handle_key("4", false, false);
        assert_eq!(app.screen, Screen::Settings);
        app.handle_key("1", false, false);
        assert_eq!(app.screen, Screen::Timer);
    }

    #[test]
    fn test_key_n_dismisses_notification() {
        let mut app = PomodoroApp::new();
        app.pending_notification = Some("Test".into());
        app.handle_key("n", false, false);
        assert!(app.pending_notification.is_none());
    }

    // ── Task input tests ───────────────────────────────────────────────

    #[test]
    fn test_task_input_mode() {
        let mut app = PomodoroApp::new();
        app.handle_key("t", false, false);
        assert!(app.task_input_active);
        assert!(app.current_task.is_empty());
    }

    #[test]
    fn test_task_typing() {
        let mut app = PomodoroApp::new();
        app.task_input_active = true;
        app.handle_key("H", false, false);
        app.handle_key("i", false, false);
        assert_eq!(app.current_task, "Hi");
    }

    #[test]
    fn test_task_backspace() {
        let mut app = PomodoroApp::new();
        app.task_input_active = true;
        app.current_task = "Hello".into();
        app.handle_key("BackSpace", false, false);
        assert_eq!(app.current_task, "Hell");
    }

    #[test]
    fn test_task_enter_confirms() {
        let mut app = PomodoroApp::new();
        app.task_input_active = true;
        app.current_task = "My Task".into();
        app.handle_key("Return", false, false);
        assert!(!app.task_input_active);
        assert_eq!(app.current_task, "My Task");
    }

    // ── Settings adjustment tests ──────────────────────────────────────

    #[test]
    fn test_settings_adjust_work() {
        let mut app = PomodoroApp::new();
        app.settings_cursor = 0;
        app.adjust_setting(true);
        assert_eq!(app.settings.work_minutes, 30);
        app.adjust_setting(false);
        assert_eq!(app.settings.work_minutes, 25);
    }

    #[test]
    fn test_settings_adjust_short_break() {
        let mut app = PomodoroApp::new();
        app.settings_cursor = 1;
        app.adjust_setting(true);
        assert_eq!(app.settings.short_break_minutes, 6);
        app.adjust_setting(false);
        assert_eq!(app.settings.short_break_minutes, 5);
    }

    #[test]
    fn test_settings_bounds() {
        let mut app = PomodoroApp::new();
        app.settings_cursor = 0;
        app.settings.work_minutes = 120;
        app.adjust_setting(true);
        assert_eq!(app.settings.work_minutes, 120); // capped

        app.settings.work_minutes = 5;
        app.adjust_setting(false);
        assert_eq!(app.settings.work_minutes, 5); // floored
    }

    #[test]
    fn test_settings_toggle_auto_start() {
        let mut app = PomodoroApp::new();
        app.settings_cursor = 4;
        assert!(!app.settings.auto_start_breaks);
        app.adjust_setting(true);
        assert!(app.settings.auto_start_breaks);
    }

    #[test]
    fn test_settings_updates_timer_when_idle() {
        let mut app = PomodoroApp::new();
        app.settings_cursor = 0;
        app.adjust_setting(true); // 25 -> 30
        assert_eq!(app.remaining_secs, 1800); // 30 * 60
    }

    // ── Auto-start tests ───────────────────────────────────────────────

    #[test]
    fn test_auto_start_break() {
        let mut app = PomodoroApp::new();
        app.settings.auto_start_breaks = true;
        app.start();
        app.remaining_secs = 1;
        app.tick(); // completes work, transitions to short break
        assert_eq!(app.phase, Phase::ShortBreak);
        assert_eq!(app.state, TimerState::Running); // auto-started
    }

    #[test]
    fn test_no_auto_start_break() {
        let mut app = PomodoroApp::new();
        app.settings.auto_start_breaks = false;
        app.start();
        app.remaining_secs = 1;
        app.tick();
        assert_eq!(app.phase, Phase::ShortBreak);
        assert_eq!(app.state, TimerState::Idle); // waiting for user
    }

    // ── Full cycle test ────────────────────────────────────────────────

    #[test]
    fn test_full_pomodoro_set() {
        let mut app = PomodoroApp::new();
        app.settings.rounds_per_set = 2;

        // Round 1: Work
        assert_eq!(app.phase, Phase::Work);
        assert_eq!(app.current_round, 1);
        app.start();
        app.remaining_secs = 0;
        app.tick(); // Work -> ShortBreak

        assert_eq!(app.phase, Phase::ShortBreak);
        app.start();
        app.remaining_secs = 0;
        app.tick(); // ShortBreak -> Work (round 2)

        assert_eq!(app.phase, Phase::Work);
        assert_eq!(app.current_round, 2);
        app.start();
        app.remaining_secs = 0;
        app.tick(); // Work -> LongBreak (end of set)

        assert_eq!(app.phase, Phase::LongBreak);
        app.start();
        app.remaining_secs = 0;
        app.tick(); // LongBreak -> Work (round 1 again)

        assert_eq!(app.phase, Phase::Work);
        assert_eq!(app.current_round, 1);
    }

    // ── Render tests ───────────────────────────────────────────────────

    #[test]
    fn test_render_timer_screen() {
        let app = PomodoroApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_stats_screen() {
        let mut app = PomodoroApp::new();
        app.screen = Screen::Stats;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_log_screen() {
        let mut app = PomodoroApp::new();
        app.screen = Screen::Log;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_settings_screen() {
        let mut app = PomodoroApp::new();
        app.screen = Screen::Settings;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_notification() {
        let mut app = PomodoroApp::new();
        app.pending_notification = Some("Test notification".into());
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_task() {
        let mut app = PomodoroApp::new();
        app.current_task = "Writing code".into();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_log_entries() {
        let mut app = PomodoroApp::new();
        app.screen = Screen::Log;
        app.log_entries.push(LogEntry {
            phase: Phase::Work,
            task_label: "Test".into(),
            started_at_ms: 0,
            duration_secs: 1500,
            completed: true,
        });
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_stats() {
        let mut app = PomodoroApp::new();
        app.screen = Screen::Stats;
        app.update_daily_stats(25, 0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // ── Log scroll tests ───────────────────────────────────────────────

    #[test]
    fn test_log_scroll() {
        let mut app = PomodoroApp::new();
        app.screen = Screen::Log;
        for i in 0..20 {
            app.log_entries.push(LogEntry {
                phase: Phase::Work,
                task_label: format!("Task {i}"),
                started_at_ms: 0,
                duration_secs: 1500,
                completed: true,
            });
        }
        app.handle_key("PageDown", false, false);
        assert!(app.log_scroll > 0);
        app.handle_key("PageUp", false, false);
        assert_eq!(app.log_scroll, 0);
    }
}
