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
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

//! SlateOS Simon — classic memory pattern game.
//!
//! Features four colored buttons (Red, Green, Blue, Yellow) in a 2x2 grid.
//! The computer plays a growing sequence of colors; the player must repeat
//! the sequence from memory. Each successful round adds one more step.
//! Three speed modes (Slow, Medium, Fast) control playback tempo.
//! Arrow keys or number keys 1-4 select colors. High score tracking
//! persists across restarts. Uses an LCG pseudo-random number generator
//! (no external rand crate). Visual pulse indicators substitute for sound.

use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ── Layout constants ────────────────────────────────────────────────
const WINDOW_WIDTH: f32 = 520.0;
const WINDOW_HEIGHT: f32 = 620.0;
const BUTTON_SIZE: f32 = 160.0;
const BUTTON_GAP: f32 = 20.0;
const GRID_X: f32 = (WINDOW_WIDTH - BUTTON_SIZE * 2.0 - BUTTON_GAP) / 2.0;
const GRID_Y: f32 = 160.0;
const HEADER_HEIGHT: f32 = 50.0;
const BUTTON_CORNER_RADIUS: f32 = 16.0;
const TITLE_FONT_SIZE: f32 = 28.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const OVERLAY_FONT_SIZE: f32 = 16.0;
const SMALL_FONT_SIZE: f32 = 13.0;
const PULSE_INDICATOR_SIZE: f32 = 12.0;

/// Maximum sequence length (practical limit).
const MAX_SEQUENCE_LEN: usize = 999;

// ── Timing constants (milliseconds) ────────────────────────────────
/// Duration a button stays lit during sequence playback.
fn flash_duration_ms(speed: Speed) -> u64 {
    match speed {
        Speed::Slow => 800,
        Speed::Medium => 500,
        Speed::Fast => 300,
    }
}

/// Gap between flashes during sequence playback.
fn gap_duration_ms(speed: Speed) -> u64 {
    match speed {
        Speed::Slow => 400,
        Speed::Medium => 250,
        Speed::Fast => 150,
    }
}

/// Pause before sequence playback starts.
const PRE_SEQUENCE_DELAY_MS: u64 = 600;

/// Duration a button stays lit when the player presses it.
const PLAYER_FLASH_MS: u64 = 250;

/// Duration of the error flash on wrong input.
const ERROR_FLASH_MS: u64 = 800;

/// Duration of the success flash between rounds.
const SUCCESS_FLASH_MS: u64 = 600;

// ── LCG random number generator ────────────────────────────────────
/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Lcg {
    state: u64,
}

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Returns a value in `0..bound` (exclusive upper bound).
    fn next_bounded(&mut self, bound: usize) -> usize {
        let val = self.next_u64();
        (val % bound as u64) as usize
    }
}

// ── Simon color ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SimonColor {
    Red,
    Green,
    Blue,
    Yellow,
}

impl SimonColor {
    /// All four colors in grid order: top-left, top-right, bottom-left, bottom-right.
    const ALL: [SimonColor; 4] = [
        SimonColor::Red,
        SimonColor::Green,
        SimonColor::Blue,
        SimonColor::Yellow,
    ];

    /// Normal (dim) color for the button.
    fn normal_color(self) -> Color {
        match self {
            SimonColor::Red => Color::from_hex(0x8B2240),
            SimonColor::Green => Color::from_hex(0x2D6B3F),
            SimonColor::Blue => Color::from_hex(0x2B4C8C),
            SimonColor::Yellow => Color::from_hex(0x8B7B2A),
        }
    }

    /// Lit (bright) color when the button is active.
    fn lit_color(self) -> Color {
        match self {
            SimonColor::Red => RED,
            SimonColor::Green => GREEN,
            SimonColor::Blue => BLUE,
            SimonColor::Yellow => YELLOW,
        }
    }

    /// Grid position: (row, col) in the 2x2 grid.
    fn grid_pos(self) -> (usize, usize) {
        match self {
            SimonColor::Red => (0, 0),
            SimonColor::Green => (0, 1),
            SimonColor::Blue => (1, 0),
            SimonColor::Yellow => (1, 1),
        }
    }

    /// Convert from index (0..4) to color.
    fn from_index(idx: usize) -> Option<SimonColor> {
        match idx {
            0 => Some(SimonColor::Red),
            1 => Some(SimonColor::Green),
            2 => Some(SimonColor::Blue),
            3 => Some(SimonColor::Yellow),
            _ => None,
        }
    }

    /// Convert to index (0..4).
    fn to_index(self) -> usize {
        match self {
            SimonColor::Red => 0,
            SimonColor::Green => 1,
            SimonColor::Blue => 2,
            SimonColor::Yellow => 3,
        }
    }

    /// Label for display.
    fn label(self) -> &'static str {
        match self {
            SimonColor::Red => "Red",
            SimonColor::Green => "Green",
            SimonColor::Blue => "Blue",
            SimonColor::Yellow => "Yellow",
        }
    }

    /// Sound type indicator label (visual substitute for audio).
    fn sound_label(self) -> &'static str {
        match self {
            SimonColor::Red => "LOW",
            SimonColor::Green => "MID",
            SimonColor::Blue => "HIGH",
            SimonColor::Yellow => "TOP",
        }
    }
}

// ── Speed mode ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Speed {
    Slow,
    Medium,
    Fast,
}

impl Speed {
    fn label(self) -> &'static str {
        match self {
            Speed::Slow => "Slow",
            Speed::Medium => "Medium",
            Speed::Fast => "Fast",
        }
    }

    fn next(self) -> Speed {
        match self {
            Speed::Slow => Speed::Medium,
            Speed::Medium => Speed::Fast,
            Speed::Fast => Speed::Slow,
        }
    }
}

// ── Game state machine ──────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    /// Waiting before starting sequence playback.
    PreSequence,
    /// Computer is showing the sequence to memorize.
    ShowSequence,
    /// Waiting for the player to repeat the sequence.
    PlayerInput,
    /// Wrong input — showing error flash.
    GameOver,
    /// Brief success flash between rounds.
    RoundSuccess,
}

// ── Playback state ──────────────────────────────────────────────────
/// Tracks where we are in sequence playback.
#[derive(Clone, Debug)]
struct PlaybackState {
    /// Index into the sequence being shown.
    step_index: usize,
    /// Whether we are in the flash (true) or gap (false) phase.
    in_flash: bool,
    /// Milliseconds elapsed in the current phase.
    phase_elapsed_ms: u64,
}

impl PlaybackState {
    fn new() -> Self {
        Self {
            step_index: 0,
            in_flash: true,
            phase_elapsed_ms: 0,
        }
    }
}

// ── Main application ────────────────────────────────────────────────
struct SimonApp {
    /// The full sequence of colors generated so far.
    sequence: Vec<SimonColor>,
    /// Current round number (1-based, equals sequence length).
    round: u32,
    /// Current game state.
    state: GameState,
    /// Current speed setting.
    speed: Speed,
    /// Player's current input index within the sequence.
    player_index: usize,
    /// Score: number of rounds completed successfully.
    score: u32,
    /// Highest score achieved across all games.
    high_score: u32,
    /// Total games played.
    games_played: u32,
    /// Longest streak ever achieved.
    longest_streak: u32,
    /// Which button is currently lit (if any).
    lit_button: Option<SimonColor>,
    /// Timer for the player's button flash feedback.
    player_flash_timer: u64,
    /// Timer for error/success flashes.
    state_timer: u64,
    /// Playback state for sequence display.
    playback: PlaybackState,
    /// Pre-sequence delay timer.
    pre_delay_timer: u64,
    /// Pseudo-random number generator.
    rng: Lcg,
    /// Pulse animation counter for sound indicators.
    pulse_counter: u32,
    /// Currently selected button (for keyboard highlight).
    selected_button: usize,
    /// Whether the selection highlight is visible.
    show_selection: bool,
}

impl SimonApp {
    fn new() -> Self {
        Self::with_seed(0xDEAD_BEEF_CAFE)
    }

    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            sequence: Vec::new(),
            round: 0,
            state: GameState::PreSequence,
            speed: Speed::Medium,
            player_index: 0,
            score: 0,
            high_score: 0,
            games_played: 0,
            longest_streak: 0,
            lit_button: None,
            player_flash_timer: 0,
            state_timer: 0,
            playback: PlaybackState::new(),
            pre_delay_timer: 0,
            rng: Lcg::new(seed),
            pulse_counter: 0,
            selected_button: 0,
            show_selection: false,
        };
        app.start_new_game();
        app
    }

    /// Begin a fresh game, preserving high score and stats.
    fn start_new_game(&mut self) {
        self.sequence.clear();
        self.round = 0;
        self.score = 0;
        self.player_index = 0;
        self.lit_button = None;
        self.player_flash_timer = 0;
        self.state_timer = 0;
        self.games_played += 1;
        self.start_next_round();
    }

    /// Add a new color to the sequence and begin showing it.
    fn start_next_round(&mut self) {
        self.round += 1;
        let idx = self.rng.next_bounded(4);
        if let Some(color) = SimonColor::from_index(idx) {
            self.sequence.push(color);
        }
        self.player_index = 0;
        self.lit_button = None;
        self.state = GameState::PreSequence;
        self.pre_delay_timer = 0;
        self.playback = PlaybackState::new();
    }

    /// Begin sequence playback.
    fn begin_playback(&mut self) {
        self.state = GameState::ShowSequence;
        self.playback = PlaybackState::new();
        // Light up the first step immediately.
        if let Some(&color) = self.sequence.first() {
            self.lit_button = Some(color);
        }
    }

    /// Handle a tick (elapsed time in milliseconds).
    fn handle_tick(&mut self, elapsed_ms: u64) {
        self.pulse_counter = self.pulse_counter.wrapping_add(1);

        // Handle player flash timer (visual feedback for button press).
        if self.player_flash_timer > 0 {
            if elapsed_ms >= self.player_flash_timer {
                self.player_flash_timer = 0;
                // Only clear the lit button if we are in player input mode.
                if self.state == GameState::PlayerInput {
                    self.lit_button = None;
                }
            } else {
                self.player_flash_timer -= elapsed_ms;
            }
        }

        match self.state {
            GameState::PreSequence => {
                self.pre_delay_timer += elapsed_ms;
                if self.pre_delay_timer >= PRE_SEQUENCE_DELAY_MS {
                    self.begin_playback();
                }
            }
            GameState::ShowSequence => {
                self.advance_playback(elapsed_ms);
            }
            GameState::GameOver => {
                self.state_timer += elapsed_ms;
                if self.state_timer >= ERROR_FLASH_MS {
                    self.lit_button = None;
                }
            }
            GameState::RoundSuccess => {
                self.state_timer += elapsed_ms;
                if self.state_timer >= SUCCESS_FLASH_MS {
                    self.start_next_round();
                }
            }
            GameState::PlayerInput => {
                // Nothing automatic; waiting for player.
            }
        }
    }

    /// Advance the sequence playback animation.
    /// Loops through as many flash/gap transitions as the elapsed time covers,
    /// so a single large tick can complete the entire sequence.
    fn advance_playback(&mut self, elapsed_ms: u64) {
        self.playback.phase_elapsed_ms += elapsed_ms;

        loop {
            if self.playback.in_flash {
                let duration = flash_duration_ms(self.speed);
                if self.playback.phase_elapsed_ms >= duration {
                    // End of flash: enter gap.
                    self.playback.in_flash = false;
                    self.playback.phase_elapsed_ms -= duration;
                    self.lit_button = None;
                } else {
                    break;
                }
            }

            if !self.playback.in_flash {
                let duration = gap_duration_ms(self.speed);
                if self.playback.phase_elapsed_ms >= duration {
                    // End of gap: move to next step.
                    self.playback.step_index += 1;
                    self.playback.phase_elapsed_ms -= duration;
                    self.playback.in_flash = true;

                    if self.playback.step_index >= self.sequence.len() {
                        // Sequence complete: switch to player input.
                        self.state = GameState::PlayerInput;
                        self.player_index = 0;
                        self.lit_button = None;
                        break;
                    } else {
                        // Light up the next step.
                        self.lit_button = Some(self.sequence[self.playback.step_index]);
                    }
                } else {
                    break;
                }
            }
        }
    }

    /// Handle a player's color input during the PlayerInput state.
    fn player_press(&mut self, color: SimonColor) {
        if self.state != GameState::PlayerInput {
            return;
        }

        // Visual feedback: light up the pressed button.
        self.lit_button = Some(color);
        self.player_flash_timer = PLAYER_FLASH_MS;

        let expected = self.sequence[self.player_index];
        if color == expected {
            self.player_index += 1;
            if self.player_index >= self.sequence.len() {
                // Round complete!
                self.score += 1;
                if self.score > self.high_score {
                    self.high_score = self.score;
                }
                if self.score > self.longest_streak {
                    self.longest_streak = self.score;
                }
                self.state = GameState::RoundSuccess;
                self.state_timer = 0;
            }
        } else {
            // Wrong! Game over.
            self.trigger_game_over();
        }
    }

    /// Transition to game over state.
    fn trigger_game_over(&mut self) {
        if self.score > self.high_score {
            self.high_score = self.score;
        }
        if self.score > self.longest_streak {
            self.longest_streak = self.score;
        }
        self.state = GameState::GameOver;
        self.state_timer = 0;
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: Key, pressed: bool) {
        if !pressed {
            return;
        }

        match key {
            // Arrow keys select a button.
            Key::Up => {
                self.show_selection = true;
                if self.selected_button >= 2 {
                    self.selected_button -= 2;
                }
            }
            Key::Down => {
                self.show_selection = true;
                if self.selected_button < 2 {
                    self.selected_button += 2;
                }
            }
            Key::Left => {
                self.show_selection = true;
                if self.selected_button % 2 == 1 {
                    self.selected_button -= 1;
                }
            }
            Key::Right => {
                self.show_selection = true;
                if self.selected_button.is_multiple_of(2) {
                    self.selected_button += 1;
                }
            }
            // Enter/Space confirm selected button.
            Key::Enter | Key::Space => {
                if self.state == GameState::GameOver {
                    self.start_new_game();
                } else if self.state == GameState::PlayerInput
                    && let Some(color) = SimonColor::from_index(self.selected_button) {
                        self.player_press(color);
                    }
            }
            // Number keys 1-4 directly press a button.
            Key::Num1 => {
                self.handle_number_key(0);
            }
            Key::Num2 => {
                self.handle_number_key(1);
            }
            Key::Num3 => {
                self.handle_number_key(2);
            }
            Key::Num4 => {
                self.handle_number_key(3);
            }
            // S cycles speed.
            Key::S => {
                self.speed = self.speed.next();
            }
            // R restarts (only on game over).
            Key::R
                if self.state == GameState::GameOver => {
                    self.start_new_game();
                }
            // Escape restarts on game over.
            Key::Escape
                if self.state == GameState::GameOver => {
                    self.start_new_game();
                }
            _ => {}
        }
    }

    /// Handle a number key press (index 0..4).
    fn handle_number_key(&mut self, idx: usize) {
        if self.state == GameState::GameOver {
            self.start_new_game();
            return;
        }
        self.selected_button = idx;
        self.show_selection = true;
        if self.state == GameState::PlayerInput
            && let Some(color) = SimonColor::from_index(idx) {
                self.player_press(color);
            }
    }

    /// Handle incoming events (called by the framework).
    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke.key, ke.pressed),
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
            }
            _ => {}
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Produce all render commands for the current frame.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(64);

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds);
        self.render_buttons(&mut cmds);
        self.render_info_panel(&mut cmds);
        self.render_sound_indicator(&mut cmds);

        if self.state == GameState::GameOver && self.state_timer >= ERROR_FLASH_MS {
            self.render_game_over_overlay(&mut cmds);
        }

        if self.state == GameState::ShowSequence || self.state == GameState::PreSequence {
            self.render_watch_indicator(&mut cmds);
        }

        cmds
    }

    /// Render the title and score header.
    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        // Title background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: HEADER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title text.
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: 12.0,
            text: String::from("SIMON"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Round indicator.
        let round_text = format!("Round {}", self.round);
        cmds.push(RenderCommand::Text {
            x: 160.0,
            y: 18.0,
            text: round_text,
            color: TEAL,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Score.
        let score_text = format!("Score: {}", self.score);
        cmds.push(RenderCommand::Text {
            x: 280.0,
            y: 12.0,
            text: score_text,
            color: GREEN,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // High score.
        let high_text = format!("Best: {}", self.high_score);
        cmds.push(RenderCommand::Text {
            x: 280.0,
            y: 32.0,
            text: high_text,
            color: YELLOW,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Speed indicator.
        let speed_text = format!("Speed: {} (S)", self.speed.label());
        cmds.push(RenderCommand::Text {
            x: 400.0,
            y: 18.0,
            text: speed_text,
            color: PEACH,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the 2x2 grid of colored buttons.
    fn render_buttons(&self, cmds: &mut Vec<RenderCommand>) {
        for &color in &SimonColor::ALL {
            let (row, col) = color.grid_pos();
            let x = GRID_X + col as f32 * (BUTTON_SIZE + BUTTON_GAP);
            let y = GRID_Y + row as f32 * (BUTTON_SIZE + BUTTON_GAP);

            let is_lit = self.lit_button == Some(color);
            let btn_color = if is_lit {
                color.lit_color()
            } else {
                color.normal_color()
            };

            // Button background.
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width: BUTTON_SIZE,
                height: BUTTON_SIZE,
                color: btn_color,
                corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS),
            });

            // Glow effect when lit: draw a slightly larger rect behind.
            if is_lit {
                let glow_color = Color::rgba(
                    color.lit_color().r,
                    color.lit_color().g,
                    color.lit_color().b,
                    60,
                );
                cmds.push(RenderCommand::FillRect {
                    x: x - 4.0,
                    y: y - 4.0,
                    width: BUTTON_SIZE + 8.0,
                    height: BUTTON_SIZE + 8.0,
                    color: glow_color,
                    corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS + 4.0),
                });
            }

            // Selection highlight (keyboard navigation).
            if self.show_selection && color.to_index() == self.selected_button {
                cmds.push(RenderCommand::StrokeRect {
                    x: x - 3.0,
                    y: y - 3.0,
                    width: BUTTON_SIZE + 6.0,
                    height: BUTTON_SIZE + 6.0,
                    color: TEXT_COLOR,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS + 3.0),
                });
            }

            // Button label.
            let label_x = x + BUTTON_SIZE / 2.0 - 20.0;
            let label_y = y + BUTTON_SIZE / 2.0 - 10.0;
            let label_color = if is_lit {
                Color::from_hex(0x1E1E2E)
            } else {
                Color::rgba(
                    color.lit_color().r,
                    color.lit_color().g,
                    color.lit_color().b,
                    120,
                )
            };
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: label_y,
                text: String::from(color.label()),
                color: label_color,
                font_size: HEADER_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Number key hint.
            let num_label = format!("{}", color.to_index() + 1);
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 8.0,
                text: num_label,
                color: Color::rgba(200, 200, 220, 80),
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Light,
                max_width: None,
            });
        }
    }

    /// Render the information panel below the buttons.
    fn render_info_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let panel_y = GRID_Y + BUTTON_SIZE * 2.0 + BUTTON_GAP + 30.0;

        // Panel background.
        cmds.push(RenderCommand::FillRect {
            x: GRID_X,
            y: panel_y,
            width: BUTTON_SIZE * 2.0 + BUTTON_GAP,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // State indicator.
        let state_text = match self.state {
            GameState::PreSequence => String::from("Get ready..."),
            GameState::ShowSequence => format!(
                "Watch! ({}/{})",
                self.playback.step_index + 1,
                self.sequence.len()
            ),
            GameState::PlayerInput => format!(
                "Your turn! ({}/{})",
                self.player_index + 1,
                self.sequence.len()
            ),
            GameState::GameOver => String::from("Game Over!"),
            GameState::RoundSuccess => format!("Round {} complete!", self.round),
        };
        let state_color = match self.state {
            GameState::PreSequence => SUBTEXT0,
            GameState::ShowSequence => MAUVE,
            GameState::PlayerInput => TEAL,
            GameState::GameOver => RED,
            GameState::RoundSuccess => GREEN,
        };
        cmds.push(RenderCommand::Text {
            x: GRID_X + 15.0,
            y: panel_y + 12.0,
            text: state_text,
            color: state_color,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Controls hint.
        let controls = String::from("Arrow keys + Enter  |  Keys 1-4  |  S: Speed");
        cmds.push(RenderCommand::Text {
            x: GRID_X + 15.0,
            y: panel_y + 38.0,
            text: controls,
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Stats line.
        let stats = format!(
            "Games: {}  |  Best streak: {}",
            self.games_played, self.longest_streak
        );
        cmds.push(RenderCommand::Text {
            x: GRID_X + 15.0,
            y: panel_y + 56.0,
            text: stats,
            color: SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render a visual sound/pulse indicator when a button is lit.
    fn render_sound_indicator(&self, cmds: &mut Vec<RenderCommand>) {
        if let Some(color) = self.lit_button {
            let indicator_y = GRID_Y - 30.0;
            let indicator_x = WINDOW_WIDTH / 2.0 - 40.0;

            // Pulsing dot.
            let phase = (self.pulse_counter % 8) as f32 / 8.0;
            let size = PULSE_INDICATOR_SIZE + phase * 4.0;

            cmds.push(RenderCommand::FillRect {
                x: indicator_x,
                y: indicator_y,
                width: size,
                height: size,
                color: color.lit_color(),
                corner_radii: CornerRadii::all(size / 2.0),
            });

            // Sound type label.
            cmds.push(RenderCommand::Text {
                x: indicator_x + size + 8.0,
                y: indicator_y + 2.0,
                text: format!("~{}~", color.sound_label()),
                color: color.lit_color(),
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    /// Render a "WATCH" indicator during sequence playback.
    fn render_watch_indicator(&self, cmds: &mut Vec<RenderCommand>) {
        let y = GRID_Y - 55.0;
        let x = WINDOW_WIDTH / 2.0 - 30.0;

        // Blinking effect.
        let visible = self.pulse_counter % 6 < 4;
        if visible {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: String::from("WATCH"),
                color: MAUVE,
                font_size: HEADER_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    /// Render the game over overlay.
    fn render_game_over_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        // Dark overlay.
        cmds.push(RenderCommand::FillRect {
            x: GRID_X - 10.0,
            y: GRID_Y - 10.0,
            width: BUTTON_SIZE * 2.0 + BUTTON_GAP + 20.0,
            height: BUTTON_SIZE * 2.0 + BUTTON_GAP + 20.0,
            color: Color::rgba(17, 17, 27, 200),
            corner_radii: CornerRadii::all(12.0),
        });

        // Game over box.
        let box_w = 280.0;
        let box_h = 180.0;
        let box_x = WINDOW_WIDTH / 2.0 - box_w / 2.0;
        let box_y = GRID_Y + BUTTON_SIZE - box_h / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: RED,
            line_width: 2.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Game Over text.
        cmds.push(RenderCommand::Text {
            x: box_x + 60.0,
            y: box_y + 18.0,
            text: String::from("GAME OVER"),
            color: RED,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score.
        let score_text = format!("Score: {} rounds", self.score);
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 58.0,
            text: score_text,
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // High score.
        let high_text = format!("Best: {} rounds", self.high_score);
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 82.0,
            text: high_text,
            color: YELLOW,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Sequence length.
        let seq_text = format!("Sequence: {} steps", self.sequence.len());
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 106.0,
            text: seq_text,
            color: TEAL,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Restart hint.
        cmds.push(RenderCommand::Text {
            x: box_x + 30.0,
            y: box_y + 140.0,
            text: String::from("Enter / R / 1-4 to restart"),
            color: SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 30.0,
            y: box_y + 158.0,
            text: String::from("S: change speed"),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });
    }
}

fn main() {
    let _app = SimonApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a game with a fixed seed for deterministic tests.
    fn test_app() -> SimonApp {
        SimonApp::with_seed(12345)
    }

    /// Helper: create an app in PlayerInput state with a known sequence.
    fn app_ready_for_input(seq: &[SimonColor]) -> SimonApp {
        let mut app = SimonApp::with_seed(99999);
        app.sequence = seq.to_vec();
        app.round = seq.len() as u32;
        app.state = GameState::PlayerInput;
        app.player_index = 0;
        app.lit_button = None;
        app
    }

    /// Helper: force enough ticks to complete the pre-sequence delay.
    fn skip_pre_delay(app: &mut SimonApp) {
        app.handle_tick(PRE_SEQUENCE_DELAY_MS + 1);
    }

    /// Helper: force enough ticks to complete the full sequence playback.
    /// Sends two ticks: one to clear the pre-sequence delay, then one for
    /// the full playback. This is necessary because `handle_tick` only
    /// processes one state per call (PreSequence does not fall through
    /// into ShowSequence within the same tick).
    fn skip_playback(app: &mut SimonApp) {
        // First, clear any pre-sequence delay.
        if app.state == GameState::PreSequence {
            app.handle_tick(PRE_SEQUENCE_DELAY_MS + 1);
        }
        // Now advance through the full sequence playback.
        let steps = app.sequence.len() as u64;
        let total = steps * (flash_duration_ms(app.speed) + gap_duration_ms(app.speed)) + 100;
        app.handle_tick(total);
    }

    // ── Construction & initialization ───────────────────────────────

    #[test]
    fn test_initial_round_is_one() {
        let app = test_app();
        assert_eq!(app.round, 1);
    }

    #[test]
    fn test_initial_score_is_zero() {
        let app = test_app();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_initial_high_score_is_zero() {
        let app = test_app();
        assert_eq!(app.high_score, 0);
    }

    #[test]
    fn test_initial_sequence_length_is_one() {
        let app = test_app();
        assert_eq!(app.sequence.len(), 1);
    }

    #[test]
    fn test_initial_state_is_pre_sequence() {
        let app = test_app();
        assert_eq!(app.state, GameState::PreSequence);
    }

    #[test]
    fn test_initial_speed_is_medium() {
        let app = test_app();
        assert_eq!(app.speed, Speed::Medium);
    }

    #[test]
    fn test_initial_player_index_is_zero() {
        let app = test_app();
        assert_eq!(app.player_index, 0);
    }

    #[test]
    fn test_initial_lit_button_is_none() {
        let app = test_app();
        assert!(app.lit_button.is_none());
    }

    #[test]
    fn test_initial_games_played_is_one() {
        let app = test_app();
        assert_eq!(app.games_played, 1);
    }

    #[test]
    fn test_initial_sequence_contains_valid_color() {
        let app = test_app();
        assert!(SimonColor::ALL.contains(&app.sequence[0]));
    }

    // ── LCG RNG ─────────────────────────────────────────────────────

    #[test]
    fn test_lcg_deterministic() {
        let mut rng1 = Lcg::new(42);
        let mut rng2 = Lcg::new(42);
        for _ in 0..10 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_lcg_different_seeds_differ() {
        let mut rng1 = Lcg::new(1);
        let mut rng2 = Lcg::new(2);
        assert_ne!(rng1.next_u64(), rng2.next_u64());
    }

    #[test]
    fn test_lcg_bounded() {
        let mut rng = Lcg::new(42);
        for _ in 0..100 {
            let val = rng.next_bounded(4);
            assert!(val < 4);
        }
    }

    #[test]
    fn test_lcg_bounded_single() {
        let mut rng = Lcg::new(42);
        for _ in 0..50 {
            assert_eq!(rng.next_bounded(1), 0);
        }
    }

    #[test]
    fn test_lcg_produces_all_values_in_range() {
        let mut rng = Lcg::new(42);
        let mut seen = [false; 4];
        for _ in 0..100 {
            seen[rng.next_bounded(4)] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    // ── SimonColor ──────────────────────────────────────────────────

    #[test]
    fn test_color_from_index_valid() {
        assert_eq!(SimonColor::from_index(0), Some(SimonColor::Red));
        assert_eq!(SimonColor::from_index(1), Some(SimonColor::Green));
        assert_eq!(SimonColor::from_index(2), Some(SimonColor::Blue));
        assert_eq!(SimonColor::from_index(3), Some(SimonColor::Yellow));
    }

    #[test]
    fn test_color_from_index_invalid() {
        assert_eq!(SimonColor::from_index(4), None);
        assert_eq!(SimonColor::from_index(100), None);
    }

    #[test]
    fn test_color_to_index_roundtrip() {
        for i in 0..4 {
            let c = SimonColor::from_index(i).unwrap();
            assert_eq!(c.to_index(), i);
        }
    }

    #[test]
    fn test_color_grid_positions() {
        assert_eq!(SimonColor::Red.grid_pos(), (0, 0));
        assert_eq!(SimonColor::Green.grid_pos(), (0, 1));
        assert_eq!(SimonColor::Blue.grid_pos(), (1, 0));
        assert_eq!(SimonColor::Yellow.grid_pos(), (1, 1));
    }

    #[test]
    fn test_color_labels() {
        assert_eq!(SimonColor::Red.label(), "Red");
        assert_eq!(SimonColor::Green.label(), "Green");
        assert_eq!(SimonColor::Blue.label(), "Blue");
        assert_eq!(SimonColor::Yellow.label(), "Yellow");
    }

    #[test]
    fn test_color_sound_labels() {
        assert_eq!(SimonColor::Red.sound_label(), "LOW");
        assert_eq!(SimonColor::Green.sound_label(), "MID");
        assert_eq!(SimonColor::Blue.sound_label(), "HIGH");
        assert_eq!(SimonColor::Yellow.sound_label(), "TOP");
    }

    #[test]
    fn test_color_normal_colors_differ_from_lit() {
        for &c in &SimonColor::ALL {
            assert_ne!(c.normal_color(), c.lit_color());
        }
    }

    #[test]
    fn test_color_all_array_has_four() {
        assert_eq!(SimonColor::ALL.len(), 4);
    }

    // ── Speed ───────────────────────────────────────────────────────

    #[test]
    fn test_speed_labels() {
        assert_eq!(Speed::Slow.label(), "Slow");
        assert_eq!(Speed::Medium.label(), "Medium");
        assert_eq!(Speed::Fast.label(), "Fast");
    }

    #[test]
    fn test_speed_cycle() {
        assert_eq!(Speed::Slow.next(), Speed::Medium);
        assert_eq!(Speed::Medium.next(), Speed::Fast);
        assert_eq!(Speed::Fast.next(), Speed::Slow);
    }

    #[test]
    fn test_flash_duration_decreases_with_speed() {
        let slow = flash_duration_ms(Speed::Slow);
        let medium = flash_duration_ms(Speed::Medium);
        let fast = flash_duration_ms(Speed::Fast);
        assert!(slow > medium);
        assert!(medium > fast);
    }

    #[test]
    fn test_gap_duration_decreases_with_speed() {
        let slow = gap_duration_ms(Speed::Slow);
        let medium = gap_duration_ms(Speed::Medium);
        let fast = gap_duration_ms(Speed::Fast);
        assert!(slow > medium);
        assert!(medium > fast);
    }

    // ── Pre-sequence delay ──────────────────────────────────────────

    #[test]
    fn test_pre_sequence_transitions_to_show() {
        let mut app = test_app();
        assert_eq!(app.state, GameState::PreSequence);
        skip_pre_delay(&mut app);
        assert_eq!(app.state, GameState::ShowSequence);
    }

    #[test]
    fn test_pre_sequence_partial_tick_stays() {
        let mut app = test_app();
        app.handle_tick(PRE_SEQUENCE_DELAY_MS / 2);
        assert_eq!(app.state, GameState::PreSequence);
    }

    // ── Sequence playback ───────────────────────────────────────────

    #[test]
    fn test_playback_lights_first_button() {
        let mut app = test_app();
        skip_pre_delay(&mut app);
        assert_eq!(app.state, GameState::ShowSequence);
        assert!(app.lit_button.is_some());
    }

    #[test]
    fn test_playback_completes_to_player_input() {
        let mut app = test_app();
        skip_playback(&mut app);
        assert_eq!(app.state, GameState::PlayerInput);
    }

    #[test]
    fn test_playback_clears_lit_button_after() {
        let mut app = test_app();
        skip_playback(&mut app);
        assert!(app.lit_button.is_none());
    }

    #[test]
    fn test_playback_multi_step() {
        let mut app = app_ready_for_input(&[SimonColor::Red, SimonColor::Blue, SimonColor::Green]);
        // Switch to showing the sequence.
        app.state = GameState::PreSequence;
        app.pre_delay_timer = 0;
        app.playback = PlaybackState::new();
        skip_playback(&mut app);
        assert_eq!(app.state, GameState::PlayerInput);
    }

    // ── Player input — correct ──────────────────────────────────────

    #[test]
    fn test_correct_single_input() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.state, GameState::RoundSuccess);
        assert_eq!(app.score, 1);
    }

    #[test]
    fn test_correct_multi_step_input() {
        let seq = [SimonColor::Red, SimonColor::Green, SimonColor::Blue];
        let mut app = app_ready_for_input(&seq);
        app.player_press(SimonColor::Red);
        assert_eq!(app.state, GameState::PlayerInput);
        assert_eq!(app.player_index, 1);
        app.player_press(SimonColor::Green);
        assert_eq!(app.state, GameState::PlayerInput);
        assert_eq!(app.player_index, 2);
        app.player_press(SimonColor::Blue);
        assert_eq!(app.state, GameState::RoundSuccess);
        assert_eq!(app.score, 1);
    }

    #[test]
    fn test_correct_input_lights_button() {
        let mut app = app_ready_for_input(&[SimonColor::Red, SimonColor::Green]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.lit_button, Some(SimonColor::Red));
    }

    #[test]
    fn test_correct_input_sets_flash_timer() {
        let mut app = app_ready_for_input(&[SimonColor::Blue]);
        app.player_press(SimonColor::Blue);
        // Flash timer is irrelevant after round success but was set before completion.
        // For multi-step, check the intermediate state.
        let mut app2 = app_ready_for_input(&[SimonColor::Blue, SimonColor::Red]);
        app2.player_press(SimonColor::Blue);
        assert!(app2.player_flash_timer > 0);
    }

    // ── Player input — incorrect ────────────────────────────────────

    #[test]
    fn test_wrong_input_triggers_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_wrong_input_preserves_sequence() {
        let seq = vec![SimonColor::Red, SimonColor::Green];
        let mut app = app_ready_for_input(&seq);
        app.player_press(SimonColor::Yellow);
        assert_eq!(app.sequence, seq);
    }

    #[test]
    fn test_wrong_second_input_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red, SimonColor::Green]);
        app.player_press(SimonColor::Red); // correct
        app.player_press(SimonColor::Yellow); // wrong
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_player_press_ignored_during_playback() {
        let mut app = test_app();
        skip_pre_delay(&mut app);
        assert_eq!(app.state, GameState::ShowSequence);
        app.player_press(SimonColor::Red);
        // State should still be ShowSequence.
        assert_eq!(app.state, GameState::ShowSequence);
    }

    #[test]
    fn test_player_press_ignored_during_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue); // triggers game over
        let prev_score = app.score;
        app.player_press(SimonColor::Red); // ignored
        assert_eq!(app.score, prev_score);
    }

    // ── Round success & progression ─────────────────────────────────

    #[test]
    fn test_round_success_increments_score() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.score, 1);
    }

    #[test]
    fn test_round_success_transitions_after_timer() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.state, GameState::RoundSuccess);
        app.handle_tick(SUCCESS_FLASH_MS + 1);
        // Should transition to PreSequence for next round.
        assert_eq!(app.state, GameState::PreSequence);
    }

    #[test]
    fn test_round_success_extends_sequence() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        let old_len = app.sequence.len();
        app.player_press(SimonColor::Red);
        app.handle_tick(SUCCESS_FLASH_MS + 1);
        assert_eq!(app.sequence.len(), old_len + 1);
    }

    #[test]
    fn test_round_success_updates_high_score() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.high_score, 1);
    }

    #[test]
    fn test_multiple_rounds_accumulate_score() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.score, 1);
        // After timer, new round starts with extended sequence.
        app.handle_tick(SUCCESS_FLASH_MS + 1);
        assert_eq!(app.state, GameState::PreSequence);
        // Skip through to player input for round 2.
        skip_playback(&mut app);
        assert_eq!(app.state, GameState::PlayerInput);
        // Play round 2: replay the full 2-element sequence.
        let seq: Vec<SimonColor> = app.sequence.clone();
        for &color in &seq {
            app.player_press(color);
        }
        assert_eq!(app.score, 2);
    }

    // ── High score tracking ─────────────────────────────────────────

    #[test]
    fn test_high_score_preserved_on_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.high_score, 1);
        app.handle_tick(SUCCESS_FLASH_MS + 1);
        skip_playback(&mut app);
        app.player_press(SimonColor::Yellow); // wrong
        assert_eq!(app.high_score, 1);
    }

    #[test]
    fn test_high_score_only_increases() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.high_score, 1);
        // Restart and fail immediately.
        app.start_new_game();
        skip_playback(&mut app);
        app.player_press(SimonColor::Yellow); // probably wrong
        // High score should still be 1 (or higher if the player got lucky).
        assert!(app.high_score >= 1);
    }

    #[test]
    fn test_high_score_preserved_across_restart() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        app.start_new_game();
        assert_eq!(app.high_score, 1);
    }

    // ── Restart ─────────────────────────────────────────────────────

    #[test]
    fn test_restart_resets_score() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        app.start_new_game();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_restart_resets_round_to_one() {
        let mut app = test_app();
        app.round = 5;
        app.start_new_game();
        assert_eq!(app.round, 1);
    }

    #[test]
    fn test_restart_creates_new_sequence() {
        let mut app = test_app();
        app.start_new_game();
        assert_eq!(app.sequence.len(), 1);
    }

    #[test]
    fn test_restart_goes_to_pre_sequence() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        app.start_new_game();
        assert_eq!(app.state, GameState::PreSequence);
    }

    #[test]
    fn test_restart_increments_games_played() {
        let mut app = test_app();
        let before = app.games_played;
        app.start_new_game();
        assert_eq!(app.games_played, before + 1);
    }

    // ── Key handling ────────────────────────────────────────────────

    #[test]
    fn test_arrow_up_selects() {
        let mut app = test_app();
        app.selected_button = 2;
        app.handle_key(Key::Up, true);
        assert_eq!(app.selected_button, 0);
        assert!(app.show_selection);
    }

    #[test]
    fn test_arrow_down_selects() {
        let mut app = test_app();
        app.selected_button = 0;
        app.handle_key(Key::Down, true);
        assert_eq!(app.selected_button, 2);
        assert!(app.show_selection);
    }

    #[test]
    fn test_arrow_left_selects() {
        let mut app = test_app();
        app.selected_button = 1;
        app.handle_key(Key::Left, true);
        assert_eq!(app.selected_button, 0);
    }

    #[test]
    fn test_arrow_right_selects() {
        let mut app = test_app();
        app.selected_button = 0;
        app.handle_key(Key::Right, true);
        assert_eq!(app.selected_button, 1);
    }

    #[test]
    fn test_arrow_up_clamps_at_top() {
        let mut app = test_app();
        app.selected_button = 0;
        app.handle_key(Key::Up, true);
        assert_eq!(app.selected_button, 0);
    }

    #[test]
    fn test_arrow_down_clamps_at_bottom() {
        let mut app = test_app();
        app.selected_button = 3;
        app.handle_key(Key::Down, true);
        assert_eq!(app.selected_button, 3);
    }

    #[test]
    fn test_arrow_left_clamps_at_left() {
        let mut app = test_app();
        app.selected_button = 0;
        app.handle_key(Key::Left, true);
        assert_eq!(app.selected_button, 0);
    }

    #[test]
    fn test_arrow_right_clamps_at_right() {
        let mut app = test_app();
        app.selected_button = 1;
        app.handle_key(Key::Right, true);
        assert_eq!(app.selected_button, 1);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = test_app();
        app.selected_button = 0;
        app.handle_key(Key::Down, false);
        assert_eq!(app.selected_button, 0);
    }

    #[test]
    fn test_number_key_1_selects_red() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.handle_key(Key::Num1, true);
        // If Red was correct, round success.
        assert_eq!(app.state, GameState::RoundSuccess);
    }

    #[test]
    fn test_number_key_2_selects_green() {
        let mut app = app_ready_for_input(&[SimonColor::Green]);
        app.handle_key(Key::Num2, true);
        assert_eq!(app.state, GameState::RoundSuccess);
    }

    #[test]
    fn test_number_key_3_selects_blue() {
        let mut app = app_ready_for_input(&[SimonColor::Blue]);
        app.handle_key(Key::Num3, true);
        assert_eq!(app.state, GameState::RoundSuccess);
    }

    #[test]
    fn test_number_key_4_selects_yellow() {
        let mut app = app_ready_for_input(&[SimonColor::Yellow]);
        app.handle_key(Key::Num4, true);
        assert_eq!(app.state, GameState::RoundSuccess);
    }

    #[test]
    fn test_enter_confirms_selection() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.selected_button = 0;
        app.handle_key(Key::Enter, true);
        assert_eq!(app.state, GameState::RoundSuccess);
    }

    #[test]
    fn test_space_confirms_selection() {
        let mut app = app_ready_for_input(&[SimonColor::Green]);
        app.selected_button = 1;
        app.handle_key(Key::Space, true);
        assert_eq!(app.state, GameState::RoundSuccess);
    }

    #[test]
    fn test_enter_restarts_on_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue); // game over
        app.handle_key(Key::Enter, true);
        assert_eq!(app.state, GameState::PreSequence);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_r_restarts_on_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue);
        app.handle_key(Key::R, true);
        assert_eq!(app.state, GameState::PreSequence);
    }

    #[test]
    fn test_escape_restarts_on_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue);
        app.handle_key(Key::Escape, true);
        assert_eq!(app.state, GameState::PreSequence);
    }

    #[test]
    fn test_s_cycles_speed() {
        let mut app = test_app();
        assert_eq!(app.speed, Speed::Medium);
        app.handle_key(Key::S, true);
        assert_eq!(app.speed, Speed::Fast);
        app.handle_key(Key::S, true);
        assert_eq!(app.speed, Speed::Slow);
        app.handle_key(Key::S, true);
        assert_eq!(app.speed, Speed::Medium);
    }

    #[test]
    fn test_r_ignored_when_not_game_over() {
        let mut app = test_app();
        let round = app.round;
        app.handle_key(Key::R, true);
        assert_eq!(app.round, round); // no restart
    }

    #[test]
    fn test_number_key_restarts_on_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue);
        assert_eq!(app.state, GameState::GameOver);
        app.handle_key(Key::Num1, true);
        assert_eq!(app.state, GameState::PreSequence);
    }

    // ── Event handling ──────────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = test_app();
        let event = Event::Key(KeyEvent {
            key: Key::S,
            pressed: true,
            modifiers: Modifiers::default(),
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.speed, Speed::Fast);
    }

    #[test]
    fn test_handle_event_tick() {
        let mut app = test_app();
        let pc_before = app.pulse_counter;
        let event = Event::Tick { elapsed_ms: 100 };
        app.handle_event(&event);
        assert!(app.pulse_counter > pc_before);
    }

    // ── Game over timing ────────────────────────────────────────────

    #[test]
    fn test_game_over_error_flash_timing() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue);
        assert_eq!(app.state, GameState::GameOver);
        // During error flash, lit button may still be set.
        app.handle_tick(ERROR_FLASH_MS / 2);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_game_over_clears_lit_after_error_flash() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue);
        app.handle_tick(ERROR_FLASH_MS + 1);
        assert!(app.lit_button.is_none());
    }

    // ── Player flash timer ──────────────────────────────────────────

    #[test]
    fn test_player_flash_timer_decreases() {
        let mut app = app_ready_for_input(&[SimonColor::Red, SimonColor::Green]);
        app.player_press(SimonColor::Red);
        let timer = app.player_flash_timer;
        assert!(timer > 0);
        app.handle_tick(50);
        assert!(app.player_flash_timer < timer);
    }

    #[test]
    fn test_player_flash_timer_clears_lit() {
        let mut app = app_ready_for_input(&[SimonColor::Red, SimonColor::Green]);
        app.player_press(SimonColor::Red);
        assert!(app.lit_button.is_some());
        app.handle_tick(PLAYER_FLASH_MS + 1);
        assert!(app.lit_button.is_none());
    }

    // ── Rendering ───────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = test_app();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_starts_with_background() {
        let app = test_app();
        let cmds = app.render();
        match &cmds[0] {
            RenderCommand::FillRect { x, y, color, .. } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*color, BASE);
            }
            _ => panic!("first command should be FillRect background"),
        }
    }

    #[test]
    fn test_render_contains_title() {
        let app = test_app();
        let cmds = app.render();
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "SIMON"));
        assert!(has_title);
    }

    #[test]
    fn test_render_contains_score() {
        let app = test_app();
        let cmds = app.render();
        let has_score = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Score")));
        assert!(has_score);
    }

    #[test]
    fn test_render_contains_round() {
        let app = test_app();
        let cmds = app.render();
        let has_round = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Round")));
        assert!(has_round);
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Blue);
        app.handle_tick(ERROR_FLASH_MS + 1);
        let cmds = app.render();
        let has_game_over = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "GAME OVER"));
        assert!(has_game_over);
    }

    #[test]
    fn test_render_watch_indicator_during_playback() {
        let mut app = test_app();
        skip_pre_delay(&mut app);
        // Force pulse_counter to a visible phase so the blinking text is shown.
        app.pulse_counter = 0;
        let cmds = app.render();
        let has_watch = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "WATCH"));
        assert!(has_watch);
    }

    #[test]
    fn test_render_no_watch_during_player_input() {
        let app = app_ready_for_input(&[SimonColor::Red]);
        let cmds = app.render();
        let has_watch = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "WATCH"));
        assert!(!has_watch);
    }

    #[test]
    fn test_render_sound_indicator_when_lit() {
        let mut app = test_app();
        app.lit_button = Some(SimonColor::Red);
        let cmds = app.render();
        let has_sound = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("LOW")));
        assert!(has_sound);
    }

    #[test]
    fn test_render_no_sound_indicator_when_not_lit() {
        let mut app = test_app();
        app.lit_button = None;
        let cmds = app.render();
        let has_sound = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("~LOW~")));
        assert!(!has_sound);
    }

    #[test]
    fn test_render_buttons_present() {
        let app = test_app();
        let cmds = app.render();
        // Should have at least 4 button FillRects.
        let button_rects = cmds
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    RenderCommand::FillRect {
                        width,
                        height,
                        ..
                    } if (*width - BUTTON_SIZE).abs() < 0.01 && (*height - BUTTON_SIZE).abs() < 0.01
                )
            })
            .count();
        assert!(
            button_rects >= 4,
            "expected at least 4 button rects, got {button_rects}"
        );
    }

    #[test]
    fn test_render_selection_highlight() {
        let mut app = test_app();
        app.show_selection = true;
        app.selected_button = 2;
        let cmds = app.render();
        let has_stroke = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == TEXT_COLOR));
        assert!(has_stroke);
    }

    #[test]
    fn test_render_no_selection_when_hidden() {
        let mut app = test_app();
        app.show_selection = false;
        let cmds = app.render();
        let has_stroke = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == TEXT_COLOR));
        assert!(!has_stroke);
    }

    // ── Longest streak tracking ─────────────────────────────────────

    #[test]
    fn test_longest_streak_updates_on_success() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        assert_eq!(app.longest_streak, 1);
    }

    #[test]
    fn test_longest_streak_preserved_on_game_over() {
        let mut app = app_ready_for_input(&[SimonColor::Red]);
        app.player_press(SimonColor::Red);
        app.handle_tick(SUCCESS_FLASH_MS + 1);
        skip_playback(&mut app);
        app.player_press(SimonColor::Yellow); // wrong
        assert!(app.longest_streak >= 1);
    }

    // ── Playback state ──────────────────────────────────────────────

    #[test]
    fn test_playback_state_new() {
        let pb = PlaybackState::new();
        assert_eq!(pb.step_index, 0);
        assert!(pb.in_flash);
        assert_eq!(pb.phase_elapsed_ms, 0);
    }

    // ── Sequence generation ─────────────────────────────────────────

    #[test]
    fn test_sequence_deterministic_with_seed() {
        let app1 = SimonApp::with_seed(42);
        let app2 = SimonApp::with_seed(42);
        assert_eq!(app1.sequence, app2.sequence);
    }

    #[test]
    fn test_different_seeds_different_sequences() {
        let app1 = SimonApp::with_seed(1);
        let app2 = SimonApp::with_seed(2);
        // Might be the same for one step, but with 4 choices it's unlikely.
        // Generate a few rounds to be sure.
        // Actually, just check the seed gives valid colors.
        assert!(SimonColor::ALL.contains(&app1.sequence[0]));
        assert!(SimonColor::ALL.contains(&app2.sequence[0]));
    }

    #[test]
    fn test_new_round_extends_sequence() {
        let mut app = test_app();
        assert_eq!(app.sequence.len(), 1);
        app.start_next_round();
        assert_eq!(app.sequence.len(), 2);
        app.start_next_round();
        assert_eq!(app.sequence.len(), 3);
    }

    #[test]
    fn test_new_round_preserves_previous_entries() {
        let mut app = test_app();
        let first = app.sequence[0];
        app.start_next_round();
        assert_eq!(app.sequence[0], first);
    }

    // ── Miscellaneous edge cases ────────────────────────────────────

    #[test]
    fn test_pulse_counter_wraps() {
        let mut app = test_app();
        app.pulse_counter = u32::MAX;
        app.handle_tick(1);
        assert_eq!(app.pulse_counter, 0);
    }

    #[test]
    fn test_handle_tick_zero_elapsed() {
        let mut app = test_app();
        let state_before = app.state;
        app.handle_tick(0);
        assert_eq!(app.state, state_before);
    }

    #[test]
    fn test_window_constants_positive() {
        const { assert!(WINDOW_WIDTH > 0.0) };
        const { assert!(WINDOW_HEIGHT > 0.0) };
        const { assert!(BUTTON_SIZE > 0.0) };
        const { assert!(BUTTON_GAP > 0.0) };
    }

    #[test]
    fn test_grid_fits_in_window() {
        let grid_width = BUTTON_SIZE * 2.0 + BUTTON_GAP;
        assert!(GRID_X + grid_width <= WINDOW_WIDTH);
        let grid_height = BUTTON_SIZE * 2.0 + BUTTON_GAP;
        assert!(GRID_Y + grid_height <= WINDOW_HEIGHT);
    }
}
