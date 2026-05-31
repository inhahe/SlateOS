//! OurOS Stopwatch & Lap Timer
//!
//! A precision stopwatch with lap timing, split times, countdown timer mode,
//! and session history.

#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]

use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const COL_BASE: u32 = 0x1E1E2E;
const COL_MANTLE: u32 = 0x181825;
const COL_SURFACE0: u32 = 0x313244;
const COL_SURFACE1: u32 = 0x45475A;
const COL_TEXT: u32 = 0xCDD6F4;
const COL_SUBTEXT0: u32 = 0xA6ADC8;
const COL_BLUE: u32 = 0x89B4FA;
const COL_GREEN: u32 = 0xA6E3A1;
const COL_RED: u32 = 0xF38BA8;
const COL_YELLOW: u32 = 0xF9E2AF;
const COL_PEACH: u32 = 0xFAB387;
const COL_LAVENDER: u32 = 0xB4BEFE;
const COL_OVERLAY0: u32 = 0x6C7086;
const COL_TEAL: u32 = 0x94E2D5;
const COL_MAUVE: u32 = 0xCBA6F7;

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
        format!("{hours}:{mins:02}:{secs:02}.{:03}", millis)
    } else {
        format!("{mins:02}:{secs:02}.{:03}", millis)
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
// Stopwatch mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerState {
    Stopped,
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Stopwatch,
    Countdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppView {
    Main,
    History,
    CountdownSetup,
}

// ---------------------------------------------------------------------------
// Lap
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Lap {
    number: u32,
    split_ms: u64, // time since start
    lap_ms: u64,   // time since previous lap
}

// ---------------------------------------------------------------------------
// Session record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SessionRecord {
    mode: AppMode,
    total_ms: u64,
    lap_count: u32,
    best_lap_ms: Option<u64>,
    worst_lap_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Main app
// ---------------------------------------------------------------------------

struct StopwatchApp {
    mode: AppMode,
    view: AppView,
    state: TimerState,
    elapsed_ms: u64,
    laps: Vec<Lap>,
    lap_scroll: usize,
    max_visible_laps: usize,

    // Countdown-specific
    countdown_target_ms: u64,
    countdown_remaining_ms: u64,
    countdown_setup_field: usize,     // 0=hours, 1=mins, 2=secs
    countdown_setup_values: [u32; 3], // hours, mins, secs
    countdown_finished: bool,

    // History
    history: Vec<SessionRecord>,
    history_scroll: usize,

    // Tick tracking
    last_tick_ms: u64,
}

impl StopwatchApp {
    fn new() -> Self {
        Self {
            mode: AppMode::Stopwatch,
            view: AppView::Main,
            state: TimerState::Stopped,
            elapsed_ms: 0,
            laps: Vec::new(),
            lap_scroll: 0,
            max_visible_laps: 8,
            countdown_target_ms: 300_000, // 5 minutes default
            countdown_remaining_ms: 300_000,
            countdown_setup_field: 0,
            countdown_setup_values: [0, 5, 0],
            countdown_finished: false,
            history: Vec::new(),
            history_scroll: 0,
            last_tick_ms: 0,
        }
    }

    fn start(&mut self) {
        match self.state {
            TimerState::Stopped => {
                if self.mode == AppMode::Countdown {
                    let total = self.countdown_setup_values[0] as u64 * 3_600_000
                        + self.countdown_setup_values[1] as u64 * 60_000
                        + self.countdown_setup_values[2] as u64 * 1_000;
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
                self.state = TimerState::Running;
            }
            TimerState::Running => {}
        }
    }

    fn pause(&mut self) {
        if self.state == TimerState::Running {
            self.state = TimerState::Paused;
        }
    }

    fn stop(&mut self) {
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
                lap_count: self.laps.len() as u32,
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

    fn lap(&mut self) {
        if self.state != TimerState::Running || self.mode == AppMode::Countdown {
            return;
        }
        let prev_split = self.laps.last().map_or(0, |l| l.split_ms);
        let lap_ms = self.elapsed_ms.saturating_sub(prev_split);
        let number = self.laps.len() as u32 + 1;
        self.laps.push(Lap {
            number,
            split_ms: self.elapsed_ms,
            lap_ms,
        });
        // Auto-scroll to latest
        if self.laps.len() > self.max_visible_laps {
            self.lap_scroll = self.laps.len() - self.max_visible_laps;
        }
    }

    fn tick(&mut self, current_ms: u64) {
        if self.state != TimerState::Running {
            self.last_tick_ms = current_ms;
            return;
        }
        let delta = current_ms.saturating_sub(self.last_tick_ms);
        self.last_tick_ms = current_ms;

        match self.mode {
            AppMode::Stopwatch => {
                self.elapsed_ms = self.elapsed_ms.saturating_add(delta);
            }
            AppMode::Countdown => {
                if self.countdown_remaining_ms > 0 {
                    if delta >= self.countdown_remaining_ms {
                        self.countdown_remaining_ms = 0;
                        self.countdown_finished = true;
                        self.state = TimerState::Paused;
                    } else {
                        self.countdown_remaining_ms =
                            self.countdown_remaining_ms.saturating_sub(delta);
                    }
                }
            }
        }
    }

    fn display_time(&self) -> u64 {
        match self.mode {
            AppMode::Stopwatch => self.elapsed_ms,
            AppMode::Countdown => self.countdown_remaining_ms,
        }
    }

    fn best_lap(&self) -> Option<&Lap> {
        if self.laps.len() < 2 {
            return None;
        }
        self.laps.iter().min_by_key(|l| l.lap_ms)
    }

    fn worst_lap(&self) -> Option<&Lap> {
        if self.laps.len() < 2 {
            return None;
        }
        self.laps.iter().max_by_key(|l| l.lap_ms)
    }

    fn average_lap_ms(&self) -> Option<u64> {
        if self.laps.is_empty() {
            return None;
        }
        let total: u64 = self.laps.iter().map(|l| l.lap_ms).sum();
        Some(total / self.laps.len() as u64)
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match self.view {
            AppView::Main => self.handle_main(event),
            AppView::History => self.handle_history(event),
            AppView::CountdownSetup => self.handle_countdown_setup(event),
        }
    }

    fn handle_main(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Space => {
                if self.state == TimerState::Running {
                    self.pause();
                } else {
                    self.start();
                }
            }
            Key::L => {
                self.lap();
            }
            Key::R => {
                self.stop();
            }
            Key::M => {
                if self.state == TimerState::Stopped {
                    self.mode = match self.mode {
                        AppMode::Stopwatch => AppMode::Countdown,
                        AppMode::Countdown => AppMode::Stopwatch,
                    };
                }
            }
            Key::T => {
                if self.mode == AppMode::Countdown && self.state == TimerState::Stopped {
                    self.view = AppView::CountdownSetup;
                }
            }
            Key::H => {
                self.view = AppView::History;
            }
            Key::Up => {
                if self.lap_scroll > 0 {
                    self.lap_scroll -= 1;
                }
            }
            Key::Down => {
                let max_scroll = self.laps.len().saturating_sub(self.max_visible_laps);
                if self.lap_scroll < max_scroll {
                    self.lap_scroll += 1;
                }
            }
            _ => {}
        }
    }

    fn handle_history(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Escape | Key::H => {
                self.view = AppView::Main;
            }
            Key::Up => {
                if self.history_scroll > 0 {
                    self.history_scroll -= 1;
                }
            }
            Key::Down => {
                let max = self.history.len().saturating_sub(8);
                if self.history_scroll < max {
                    self.history_scroll += 1;
                }
            }
            _ => {}
        }
    }

    fn handle_countdown_setup(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Escape | Key::Enter => {
                // Apply values
                self.countdown_target_ms = self.countdown_setup_values[0] as u64 * 3_600_000
                    + self.countdown_setup_values[1] as u64 * 60_000
                    + self.countdown_setup_values[2] as u64 * 1_000;
                self.countdown_remaining_ms = self.countdown_target_ms;
                self.view = AppView::Main;
            }
            Key::Left => {
                if self.countdown_setup_field > 0 {
                    self.countdown_setup_field -= 1;
                }
            }
            Key::Right => {
                if self.countdown_setup_field < 2 {
                    self.countdown_setup_field += 1;
                }
            }
            Key::Up => {
                let max = if self.countdown_setup_field == 0 {
                    23
                } else {
                    59
                };
                if self.countdown_setup_values[self.countdown_setup_field] < max {
                    self.countdown_setup_values[self.countdown_setup_field] += 1;
                }
            }
            Key::Down => {
                if self.countdown_setup_values[self.countdown_setup_field] > 0 {
                    self.countdown_setup_values[self.countdown_setup_field] -= 1;
                }
            }
            _ => {}
        }
    }

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(ke) = event {
            self.handle_key(ke);
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::from_hex(COL_BASE),
            corner_radii: CornerRadii::ZERO,
        });

        match self.view {
            AppView::Main => self.render_main(&mut cmds, width),
            AppView::History => self.render_history(&mut cmds, width),
            AppView::CountdownSetup => self.render_countdown_setup(&mut cmds, width),
        }

        cmds
    }

    fn render_main(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        // Mode indicator
        let mode_text = match self.mode {
            AppMode::Stopwatch => "STOPWATCH",
            AppMode::Countdown => "COUNTDOWN",
        };
        let mode_color = match self.mode {
            AppMode::Stopwatch => COL_BLUE,
            AppMode::Countdown => COL_PEACH,
        };
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: String::from(mode_text),
            color: Color::from_hex(mode_color),
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // State indicator
        let (state_text, state_color) = match self.state {
            TimerState::Stopped => ("STOPPED", COL_OVERLAY0),
            TimerState::Running => ("RUNNING", COL_GREEN),
            TimerState::Paused => ("PAUSED", COL_YELLOW),
        };
        cmds.push(RenderCommand::FillRect {
            x: 200.0,
            y: 18.0,
            width: 90.0,
            height: 24.0,
            color: Color::from_hex(COL_SURFACE0),
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::Text {
            x: 212.0,
            y: 21.0,
            text: String::from(state_text),
            color: Color::from_hex(state_color),
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Large time display
        let time_str = format_time_ms(self.display_time());
        let time_color = if self.countdown_finished {
            COL_RED
        } else if self.state == TimerState::Running {
            COL_TEXT
        } else {
            COL_SUBTEXT0
        };
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 70.0,
            text: time_str,
            color: Color::from_hex(time_color),
            font_size: 56.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Countdown finished alert
        if self.countdown_finished {
            cmds.push(RenderCommand::FillRect {
                x: 30.0,
                y: 140.0,
                width: 300.0,
                height: 36.0,
                color: Color::from_hex(COL_RED),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: 50.0,
                y: 148.0,
                text: String::from("TIME'S UP!"),
                color: Color::from_hex(COL_BASE),
                font_size: 20.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Controls
        let controls_y = if self.countdown_finished {
            190.0
        } else {
            145.0
        };
        let controls = match self.mode {
            AppMode::Stopwatch => {
                "Space: Start/Pause  |  L: Lap  |  R: Reset  |  M: Mode  |  H: History"
            }
            AppMode::Countdown => {
                "Space: Start/Pause  |  R: Reset  |  T: Set Time  |  M: Mode  |  H: History"
            }
        };
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: controls_y,
            text: String::from(controls),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Lap list (stopwatch mode only)
        if self.mode == AppMode::Stopwatch && !self.laps.is_empty() {
            let lap_y = controls_y + 35.0;

            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: lap_y,
                text: String::from("Laps"),
                color: Color::from_hex(COL_LAVENDER),
                font_size: 18.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Column headers
            let header_y = lap_y + 28.0;
            let cols = [("Lap", 30.0), ("Lap Time", 120.0), ("Split", 280.0)];
            for (label, x) in cols {
                cmds.push(RenderCommand::Text {
                    x,
                    y: header_y,
                    text: String::from(label),
                    color: Color::from_hex(COL_SUBTEXT0),
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Separator line
            cmds.push(RenderCommand::Line {
                x1: 30.0,
                y1: header_y + 18.0,
                x2: 420.0,
                y2: header_y + 18.0,
                color: Color::from_hex(COL_SURFACE1),
                width: 1.0,
            });

            let best_idx = self.best_lap().map(|l| l.number);
            let worst_idx = self.worst_lap().map(|l| l.number);

            let end = self.laps.len().min(self.lap_scroll + self.max_visible_laps);
            let start = self.lap_scroll.min(self.laps.len());
            for (vis_i, lap) in self.laps[start..end].iter().enumerate() {
                let ly = header_y + 24.0 + vis_i as f32 * 28.0;

                // Highlight best/worst
                let lap_color = if self.laps.len() >= 2 && Some(lap.number) == best_idx {
                    COL_GREEN
                } else if self.laps.len() >= 2 && Some(lap.number) == worst_idx {
                    COL_RED
                } else {
                    COL_TEXT
                };

                cmds.push(RenderCommand::Text {
                    x: 30.0,
                    y: ly,
                    text: format!("#{}", lap.number),
                    color: Color::from_hex(lap_color),
                    font_size: 15.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: 120.0,
                    y: ly,
                    text: format_time_ms(lap.lap_ms),
                    color: Color::from_hex(lap_color),
                    font_size: 15.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: 280.0,
                    y: ly,
                    text: format_time_ms(lap.split_ms),
                    color: Color::from_hex(COL_SUBTEXT0),
                    font_size: 15.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Lap stats
            if let Some(avg) = self.average_lap_ms() {
                let stats_y = header_y + 24.0 + (end - start) as f32 * 28.0 + 15.0;
                cmds.push(RenderCommand::Line {
                    x1: 30.0,
                    y1: stats_y - 5.0,
                    x2: 420.0,
                    y2: stats_y - 5.0,
                    color: Color::from_hex(COL_SURFACE1),
                    width: 1.0,
                });
                cmds.push(RenderCommand::Text {
                    x: 30.0,
                    y: stats_y,
                    text: format!("Avg: {}  |  {} laps", format_time_ms(avg), self.laps.len()),
                    color: Color::from_hex(COL_TEAL),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }
    }

    fn render_history(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: String::from("Session History"),
            color: Color::from_hex(COL_LAVENDER),
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 55.0,
            text: String::from("Esc/H: Back"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        if self.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: 100.0,
                text: String::from("No sessions recorded yet."),
                color: Color::from_hex(COL_SUBTEXT0),
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        let headers = [
            ("Mode", 30.0),
            ("Time", 140.0),
            ("Laps", 280.0),
            ("Best Lap", 340.0),
        ];
        for (label, x) in headers {
            cmds.push(RenderCommand::Text {
                x,
                y: 80.0,
                text: String::from(label),
                color: Color::from_hex(COL_SUBTEXT0),
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        let end = self.history.len().min(self.history_scroll + 8);
        let start = self.history_scroll.min(self.history.len());
        for (i, rec) in self.history[start..end].iter().enumerate() {
            let ry = 102.0 + i as f32 * 28.0;
            let mode_str = match rec.mode {
                AppMode::Stopwatch => "Stopwatch",
                AppMode::Countdown => "Countdown",
            };
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: ry,
                text: String::from(mode_str),
                color: Color::from_hex(COL_TEXT),
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 140.0,
                y: ry,
                text: format_time_short(rec.total_ms),
                color: Color::from_hex(COL_GREEN),
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 280.0,
                y: ry,
                text: rec.lap_count.to_string(),
                color: Color::from_hex(COL_PEACH),
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            let best = rec
                .best_lap_ms
                .map_or(String::from("-"), |ms| format_time_short(ms));
            cmds.push(RenderCommand::Text {
                x: 340.0,
                y: ry,
                text: best,
                color: Color::from_hex(COL_TEAL),
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_countdown_setup(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: String::from("Set Countdown Timer"),
            color: Color::from_hex(COL_PEACH),
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 55.0,
            text: String::from("←/→: Select field  |  ↑/↓: Adjust  |  Enter: Confirm"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let labels = ["Hours", "Minutes", "Seconds"];
        let values = self.countdown_setup_values;

        for (i, (label, val)) in labels.iter().zip(values.iter()).enumerate() {
            let x = 30.0 + i as f32 * 150.0;
            let is_active = i == self.countdown_setup_field;

            cmds.push(RenderCommand::FillRect {
                x,
                y: 100.0,
                width: 130.0,
                height: 100.0,
                color: if is_active {
                    Color::from_hex(COL_SURFACE0)
                } else {
                    Color::from_hex(COL_MANTLE)
                },
                corner_radii: CornerRadii::all(8.0),
            });

            if is_active {
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y: 100.0,
                    width: 130.0,
                    height: 100.0,
                    color: Color::from_hex(COL_BLUE),
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(8.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 15.0,
                y: 110.0,
                text: String::from(*label),
                color: Color::from_hex(COL_SUBTEXT0),
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + 25.0,
                y: 135.0,
                text: format!("{val:02}"),
                color: if is_active {
                    Color::from_hex(COL_BLUE)
                } else {
                    Color::from_hex(COL_TEXT)
                },
                font_size: 40.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Preview
        let total_ms =
            values[0] as u64 * 3_600_000 + values[1] as u64 * 60_000 + values[2] as u64 * 1_000;
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 220.0,
            text: format!("Total: {}", format_time_ms(total_ms)),
            color: Color::from_hex(COL_TEAL),
            font_size: 18.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

fn main() {
    let _app = StopwatchApp::new();
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

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
        let app = StopwatchApp::new();
        assert_eq!(app.mode, AppMode::Stopwatch);
        assert_eq!(app.state, TimerState::Stopped);
        assert_eq!(app.elapsed_ms, 0);
        assert!(app.laps.is_empty());
        assert_eq!(app.view, AppView::Main);
    }

    // --- Start/pause/stop ---

    #[test]
    fn start_stopwatch() {
        let mut app = StopwatchApp::new();
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn pause_stopwatch() {
        let mut app = StopwatchApp::new();
        app.start();
        app.pause();
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn resume_stopwatch() {
        let mut app = StopwatchApp::new();
        app.start();
        app.pause();
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn stop_resets() {
        let mut app = StopwatchApp::new();
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
        let mut app = StopwatchApp::new();
        app.pause();
        assert_eq!(app.state, TimerState::Stopped);
    }

    #[test]
    fn start_when_running_no_effect() {
        let mut app = StopwatchApp::new();
        app.start();
        app.start();
        assert_eq!(app.state, TimerState::Running);
    }

    // --- Tick ---

    #[test]
    fn tick_running() {
        let mut app = StopwatchApp::new();
        app.last_tick_ms = 1000;
        app.start();
        app.tick(1500);
        assert_eq!(app.elapsed_ms, 500);
    }

    #[test]
    fn tick_paused_no_change() {
        let mut app = StopwatchApp::new();
        app.start();
        app.elapsed_ms = 1000;
        app.pause();
        app.last_tick_ms = 5000;
        app.tick(6000);
        assert_eq!(app.elapsed_ms, 1000);
    }

    #[test]
    fn tick_stopped_no_change() {
        let mut app = StopwatchApp::new();
        app.last_tick_ms = 0;
        app.tick(1000);
        assert_eq!(app.elapsed_ms, 0);
    }

    #[test]
    fn tick_accumulates() {
        let mut app = StopwatchApp::new();
        app.last_tick_ms = 0;
        app.start();
        app.tick(100);
        app.tick(300);
        app.tick(500);
        assert_eq!(app.elapsed_ms, 500);
    }

    // --- Laps ---

    #[test]
    fn add_lap() {
        let mut app = StopwatchApp::new();
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
        let mut app = StopwatchApp::new();
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
        let mut app = StopwatchApp::new();
        app.elapsed_ms = 5000;
        app.lap();
        assert!(app.laps.is_empty());
    }

    #[test]
    fn lap_in_countdown_ignored() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.state = TimerState::Running;
        app.lap();
        assert!(app.laps.is_empty());
    }

    // --- Best/worst/average lap ---

    #[test]
    fn best_lap_needs_two() {
        let mut app = StopwatchApp::new();
        app.start();
        app.elapsed_ms = 5000;
        app.lap();
        assert!(app.best_lap().is_none());
    }

    #[test]
    fn best_worst_lap() {
        let mut app = StopwatchApp::new();
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
        let mut app = StopwatchApp::new();
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
        let app = StopwatchApp::new();
        assert_eq!(app.average_lap_ms(), None);
    }

    // --- Display time ---

    #[test]
    fn display_time_stopwatch() {
        let mut app = StopwatchApp::new();
        app.elapsed_ms = 12345;
        assert_eq!(app.display_time(), 12345);
    }

    #[test]
    fn display_time_countdown() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.countdown_remaining_ms = 60000;
        assert_eq!(app.display_time(), 60000);
    }

    // --- Mode switching ---

    #[test]
    fn switch_mode() {
        let mut app = StopwatchApp::new();
        app.handle_key(&make_key(Key::M));
        assert_eq!(app.mode, AppMode::Countdown);
        app.handle_key(&make_key(Key::M));
        assert_eq!(app.mode, AppMode::Stopwatch);
    }

    #[test]
    fn switch_mode_while_running_ignored() {
        let mut app = StopwatchApp::new();
        app.start();
        app.handle_key(&make_key(Key::M));
        assert_eq!(app.mode, AppMode::Stopwatch);
    }

    // --- Countdown ---

    #[test]
    fn countdown_tick() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        // start() recomputes the target from setup_values (h, m, s) in Countdown
        // mode, so configure 10 seconds there rather than poking the derived field.
        app.countdown_setup_values = [0, 0, 10];
        app.last_tick_ms = 0;
        app.start();
        app.tick(3000);
        assert_eq!(app.countdown_remaining_ms, 7000);
    }

    #[test]
    fn countdown_finishes() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        // start() recomputes the target from setup_values in Countdown mode.
        app.countdown_setup_values = [0, 0, 5];
        app.last_tick_ms = 0;
        app.start();
        app.tick(6000);
        assert_eq!(app.countdown_remaining_ms, 0);
        assert!(app.countdown_finished);
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn countdown_zero_target_no_start() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.countdown_setup_values = [0, 0, 0];
        app.start();
        assert_eq!(app.state, TimerState::Stopped);
    }

    #[test]
    fn countdown_setup_open() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.handle_key(&make_key(Key::T));
        assert_eq!(app.view, AppView::CountdownSetup);
    }

    #[test]
    fn countdown_setup_not_in_stopwatch() {
        let mut app = StopwatchApp::new();
        app.handle_key(&make_key(Key::T));
        assert_eq!(app.view, AppView::Main);
    }

    #[test]
    fn countdown_setup_fields() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        assert_eq!(app.countdown_setup_field, 0);
        app.handle_key(&make_key(Key::Right));
        assert_eq!(app.countdown_setup_field, 1);
        app.handle_key(&make_key(Key::Right));
        assert_eq!(app.countdown_setup_field, 2);
        app.handle_key(&make_key(Key::Right));
        assert_eq!(app.countdown_setup_field, 2); // clamped
    }

    #[test]
    fn countdown_setup_adjust() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        app.countdown_setup_values = [0, 0, 0];
        app.handle_key(&make_key(Key::Up));
        assert_eq!(app.countdown_setup_values[0], 1);
        app.handle_key(&make_key(Key::Down));
        assert_eq!(app.countdown_setup_values[0], 0);
        app.handle_key(&make_key(Key::Down));
        assert_eq!(app.countdown_setup_values[0], 0); // clamped at 0
    }

    #[test]
    fn countdown_setup_confirm() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.view = AppView::CountdownSetup;
        app.countdown_setup_values = [0, 10, 30];
        app.handle_key(&make_key(Key::Enter));
        assert_eq!(app.view, AppView::Main);
        assert_eq!(app.countdown_target_ms, 10 * 60_000 + 30 * 1_000);
    }

    // --- History ---

    #[test]
    fn history_recorded_on_stop() {
        let mut app = StopwatchApp::new();
        app.start();
        app.elapsed_ms = 5000;
        app.stop();
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.history[0].total_ms, 5000);
        assert_eq!(app.history[0].mode, AppMode::Stopwatch);
    }

    #[test]
    fn history_not_recorded_when_stopped() {
        let mut app = StopwatchApp::new();
        app.stop();
        assert!(app.history.is_empty());
    }

    #[test]
    fn history_view_toggle() {
        let mut app = StopwatchApp::new();
        app.handle_key(&make_key(Key::H));
        assert_eq!(app.view, AppView::History);
        app.handle_key(&make_key(Key::Escape));
        assert_eq!(app.view, AppView::Main);
    }

    #[test]
    fn history_scroll() {
        let mut app = StopwatchApp::new();
        app.view = AppView::History;
        // Add more than 8 history entries
        for _ in 0..12 {
            app.history.push(SessionRecord {
                mode: AppMode::Stopwatch,
                total_ms: 5000,
                lap_count: 0,
                best_lap_ms: None,
                worst_lap_ms: None,
            });
        }
        app.handle_key(&make_key(Key::Down));
        assert_eq!(app.history_scroll, 1);
    }

    // --- Key handling ---

    #[test]
    fn space_starts() {
        let mut app = StopwatchApp::new();
        app.handle_key(&make_key(Key::Space));
        assert_eq!(app.state, TimerState::Running);
    }

    #[test]
    fn space_pauses() {
        let mut app = StopwatchApp::new();
        app.start();
        app.handle_key(&make_key(Key::Space));
        assert_eq!(app.state, TimerState::Paused);
    }

    #[test]
    fn r_resets() {
        let mut app = StopwatchApp::new();
        app.start();
        app.elapsed_ms = 5000;
        app.handle_key(&make_key(Key::R));
        assert_eq!(app.state, TimerState::Stopped);
        assert_eq!(app.elapsed_ms, 0);
    }

    #[test]
    fn l_adds_lap() {
        let mut app = StopwatchApp::new();
        app.start();
        app.elapsed_ms = 5000;
        app.handle_key(&make_key(Key::L));
        assert_eq!(app.laps.len(), 1);
    }

    #[test]
    fn key_released_ignored() {
        let mut app = StopwatchApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Space,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.state, TimerState::Stopped);
    }

    // --- Event handling ---

    #[test]
    fn handle_event() {
        let mut app = StopwatchApp::new();
        app.handle_event(&Event::Key(make_key(Key::Space)));
        assert_eq!(app.state, TimerState::Running);
    }

    // --- Lap scroll ---

    #[test]
    fn lap_scroll_up() {
        let mut app = StopwatchApp::new();
        app.lap_scroll = 3;
        app.handle_key(&make_key(Key::Up));
        assert_eq!(app.lap_scroll, 2);
    }

    #[test]
    fn lap_scroll_up_at_top() {
        let mut app = StopwatchApp::new();
        app.handle_key(&make_key(Key::Up));
        assert_eq!(app.lap_scroll, 0);
    }

    #[test]
    fn lap_auto_scroll() {
        let mut app = StopwatchApp::new();
        app.max_visible_laps = 3;
        app.start();
        for i in 1..=5 {
            app.elapsed_ms = i * 1000;
            app.lap();
        }
        assert_eq!(app.lap_scroll, 2); // 5 - 3
    }

    // --- Rendering ---

    #[test]
    fn render_main() {
        let app = StopwatchApp::new();
        let cmds = app.render(600.0, 800.0);
        assert!(!cmds.is_empty());
        let has_mode = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "STOPWATCH"));
        assert!(has_mode);
    }

    #[test]
    fn render_running() {
        let mut app = StopwatchApp::new();
        app.start();
        let cmds = app.render(600.0, 800.0);
        let has_running = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "RUNNING"));
        assert!(has_running);
    }

    #[test]
    fn render_countdown_mode() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        let cmds = app.render(600.0, 800.0);
        let has_mode = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "COUNTDOWN"));
        assert!(has_mode);
    }

    #[test]
    fn render_countdown_finished() {
        let mut app = StopwatchApp::new();
        app.mode = AppMode::Countdown;
        app.countdown_finished = true;
        let cmds = app.render(600.0, 800.0);
        let has_alert = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "TIME'S UP!"));
        assert!(has_alert);
    }

    #[test]
    fn render_history_empty() {
        let mut app = StopwatchApp::new();
        app.view = AppView::History;
        let cmds = app.render(600.0, 800.0);
        let has_empty = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("No sessions")));
        assert!(has_empty);
    }

    #[test]
    fn render_history_with_data() {
        let mut app = StopwatchApp::new();
        app.view = AppView::History;
        app.history.push(SessionRecord {
            mode: AppMode::Stopwatch,
            total_ms: 10000,
            lap_count: 3,
            best_lap_ms: Some(2000),
            worst_lap_ms: Some(5000),
        });
        let cmds = app.render(600.0, 800.0);
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Session History"));
        assert!(has_title);
    }

    #[test]
    fn render_countdown_setup() {
        let mut app = StopwatchApp::new();
        app.view = AppView::CountdownSetup;
        let cmds = app.render(600.0, 800.0);
        let has_title = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text == "Set Countdown Timer"),
        );
        assert!(has_title);
    }

    #[test]
    fn render_laps() {
        let mut app = StopwatchApp::new();
        app.start();
        app.elapsed_ms = 5000;
        app.lap();
        app.elapsed_ms = 9000;
        app.lap();
        let cmds = app.render(600.0, 800.0);
        let has_laps = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Laps"));
        assert!(has_laps);
    }

    #[test]
    fn render_has_background() {
        let app = StopwatchApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_bg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
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
}
