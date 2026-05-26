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
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::unreadable_literal)]

//! OurOS Pinball — classic pinball arcade game.
//!
//! Features a tall vertical playfield with two flippers (left/right),
//! a spring-loaded plunger/launcher, circular bumpers with bounce physics,
//! drop targets, a ramp, and a drain. Ball physics use position/velocity/gravity
//! simulation with collision detection against flippers, bumpers, walls,
//! targets, and ramps. Scoring awards 100 for bumper hits, 500 for target
//! hits, and 1000 for ramp completions, with combo multipliers. Players
//! get 3 balls per game (extra balls from score milestones). Includes
//! tilt detection, multi-ball bonus, high score tracking, and variable-power
//! ball launching.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent};
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Layout constants ────────────────────────────────────────────────
/// Playfield dimensions (the main pinball table area).
const TABLE_WIDTH: f32 = 300.0;
const TABLE_HEIGHT: f32 = 600.0;
/// Left sidebar width for score/info display.
const SIDEBAR_WIDTH: f32 = 160.0;
/// Padding around all elements.
const PADDING: f32 = 10.0;
/// Footer height for controls display.
const FOOTER_HEIGHT: f32 = 40.0;
/// Plunger lane width on the right side of the table.
const PLUNGER_LANE_WIDTH: f32 = 24.0;
/// Full window width.
const WINDOW_WIDTH: f32 = SIDEBAR_WIDTH + TABLE_WIDTH + PADDING * 3.0;
/// Full window height.
const WINDOW_HEIGHT: f32 = TABLE_HEIGHT + PADDING * 2.0 + FOOTER_HEIGHT;

// ── Physics constants ───────────────────────────────────────────────
/// Gravity acceleration (pixels per second^2), pulling the ball down.
const GRAVITY: f32 = 400.0;
/// Ball radius in pixels.
const BALL_RADIUS: f32 = 6.0;
/// Coefficient of restitution for wall bounces.
const WALL_RESTITUTION: f32 = 0.6;
/// Coefficient of restitution for bumper bounces.
const BUMPER_RESTITUTION: f32 = 1.5;
/// Flipper hit speed boost.
const FLIPPER_HIT_SPEED: f32 = 500.0;
/// Maximum ball speed to prevent tunnelling.
const MAX_BALL_SPEED: f32 = 900.0;
/// Friction coefficient (velocity decay per second).
const FRICTION: f32 = 0.995;
/// Maximum plunger launch power.
const MAX_LAUNCH_POWER: f32 = 700.0;
/// Power accumulation rate (per second of holding space).
const LAUNCH_POWER_RATE: f32 = 800.0;
/// Minimum launch power.
const MIN_LAUNCH_POWER: f32 = 200.0;

// ── Game constants ──────────────────────────────────────────────────
/// Starting number of balls.
const STARTING_BALLS: u32 = 3;
/// Score needed for an extra ball.
const EXTRA_BALL_SCORE: u32 = 5000;
/// Points for hitting a bumper.
const BUMPER_POINTS: u32 = 100;
/// Points for hitting a drop target.
const TARGET_POINTS: u32 = 500;
/// Points for completing a ramp.
const RAMP_POINTS: u32 = 1000;
/// Combo multiplier per successive hit within the combo window.
const COMBO_WINDOW_MS: u64 = 2000;
/// Maximum combo multiplier.
const MAX_COMBO: u32 = 5;
/// Number of high score slots.
const HIGH_SCORE_SLOTS: usize = 5;
/// Tilt threshold: max rapid inputs in a window before tilt.
const TILT_THRESHOLD: u32 = 15;
/// Tilt detection window in milliseconds.
const TILT_WINDOW_MS: u64 = 1000;
/// Tilt penalty duration in milliseconds.
const TILT_PENALTY_MS: u64 = 3000;
/// Number of targets that must be hit to activate multi-ball.
const MULTIBALL_TARGET_COUNT: usize = 5;

// ── Flipper geometry ────────────────────────────────────────────────
const FLIPPER_LENGTH: f32 = 60.0;
const FLIPPER_WIDTH: f32 = 12.0;
/// Maximum flipper rotation angle in radians (up from rest).
const FLIPPER_MAX_ANGLE: f32 = 0.5;
/// Flipper rotation speed in radians per second.
const FLIPPER_SPEED: f32 = 12.0;
/// Y position of the flipper pivot points.
const FLIPPER_Y: f32 = TABLE_HEIGHT - 80.0;
/// X position of the left flipper pivot.
const LEFT_FLIPPER_X: f32 = 80.0;
/// X position of the right flipper pivot.
const RIGHT_FLIPPER_X: f32 = TABLE_WIDTH - PLUNGER_LANE_WIDTH - 80.0;

// ── Bumper geometry ─────────────────────────────────────────────────
const BUMPER_RADIUS: f32 = 18.0;
/// Flash duration when a bumper is hit (in ms).
const BUMPER_FLASH_MS: u64 = 200;

// ── Font sizes ──────────────────────────────────────────────────────
const TITLE_FONT_SIZE: f32 = 20.0;
const SCORE_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 12.0;
const FOOTER_FONT_SIZE: f32 = 11.0;
const OVERLAY_FONT_SIZE: f32 = 18.0;

// ── LCG random number generator ────────────────────────────────────
/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Rng {
    state: u64,
}

impl Rng {
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

    /// Returns a float in [0.0, 1.0).
    fn next_f32(&mut self) -> f32 {
        let val = self.next_u64();
        // Use the upper 24 bits for mantissa precision.
        (val >> 40) as f32 / (1u64 << 24) as f32
    }
}

// ── 2D Vector ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    const ZERO: Self = Self { x: 0.0, y: 0.0 };

    fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    fn normalized(self) -> Self {
        let len = self.length();
        if len < 1e-9 {
            Self::ZERO
        } else {
            Self {
                x: self.x / len,
                y: self.y / len,
            }
        }
    }

    fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    fn scale(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }

    /// Reflect this vector off a surface with the given normal.
    fn reflect(self, normal: Self) -> Self {
        let d = self.dot(normal);
        Self {
            x: self.x - 2.0 * d * normal.x,
            y: self.y - 2.0 * d * normal.y,
        }
    }

    /// Clamp magnitude to a maximum value.
    fn clamp_magnitude(self, max_mag: f32) -> Self {
        let mag = self.length();
        if mag > max_mag {
            self.scale(max_mag / mag)
        } else {
            self
        }
    }

    /// Rotate this vector by `angle` radians.
    fn rotate(self, angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            x: self.x * c - self.y * s,
            y: self.x * s + self.y * c,
        }
    }

    /// Distance to another point.
    fn distance_to(self, other: Self) -> f32 {
        self.sub(other).length()
    }
}

// ── Ball ────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Ball {
    pos: Vec2,
    vel: Vec2,
    active: bool,
}

impl Ball {
    fn new(pos: Vec2) -> Self {
        Self {
            pos,
            vel: Vec2::ZERO,
            active: false,
        }
    }

    /// The ball rests in the plunger lane before launch.
    fn plunger_position() -> Vec2 {
        Vec2::new(
            TABLE_WIDTH - PLUNGER_LANE_WIDTH / 2.0,
            TABLE_HEIGHT - 40.0,
        )
    }
}

// ── Bumper ──────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Bumper {
    pos: Vec2,
    radius: f32,
    /// Timestamp (in total elapsed ms) when the bumper was last hit, for flash.
    last_hit_ms: u64,
    hit_count: u32,
}

impl Bumper {
    fn new(x: f32, y: f32, radius: f32) -> Self {
        Self {
            pos: Vec2::new(x, y),
            radius,
            last_hit_ms: 0,
            hit_count: 0,
        }
    }

    fn is_flashing(&self, current_ms: u64) -> bool {
        current_ms.saturating_sub(self.last_hit_ms) < BUMPER_FLASH_MS
    }
}

// ── Drop target ────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct DropTarget {
    pos: Vec2,
    width: f32,
    height: f32,
    active: bool,
    hit_flash_ms: u64,
}

impl DropTarget {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            pos: Vec2::new(x, y),
            width: w,
            height: h,
            active: true,
            hit_flash_ms: 0,
        }
    }

    fn is_flashing(&self, current_ms: u64) -> bool {
        current_ms.saturating_sub(self.hit_flash_ms) < BUMPER_FLASH_MS
    }

    /// Check if a point is inside this target rectangle.
    fn contains(&self, p: Vec2) -> bool {
        p.x >= self.pos.x
            && p.x <= self.pos.x + self.width
            && p.y >= self.pos.y
            && p.y <= self.pos.y + self.height
    }
}

// ── Ramp ────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Ramp {
    /// Entry point (bottom of ramp).
    entry: Vec2,
    /// Exit point (top of ramp).
    exit: Vec2,
    /// Width of the ramp entry zone.
    entry_width: f32,
}

impl Ramp {
    fn new(entry: Vec2, exit: Vec2, entry_width: f32) -> Self {
        Self {
            entry,
            exit,
            entry_width,
        }
    }

    /// Check if the ball is near the ramp entry going upward.
    fn ball_entering(&self, ball_pos: Vec2, ball_vel: Vec2) -> bool {
        let dx = (ball_pos.x - self.entry.x).abs();
        let dy = (ball_pos.y - self.entry.y).abs();
        // Ball must be near entry, moving upward, and with enough speed.
        dx < self.entry_width / 2.0 && dy < 15.0 && ball_vel.y < -100.0
    }
}

// ── Flipper ────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlipperSide {
    Left,
    Right,
}

#[derive(Clone, Debug)]
struct Flipper {
    side: FlipperSide,
    /// Pivot position.
    pivot: Vec2,
    /// Current angle from rest (0 = resting down, positive = raised).
    angle: f32,
    /// Whether the flipper button is currently held.
    pressed: bool,
}

impl Flipper {
    fn new(side: FlipperSide, pivot: Vec2) -> Self {
        Self {
            side,
            pivot,
            angle: 0.0,
            pressed: false,
        }
    }

    /// Rest angle in radians (the angle when not pressed).
    fn rest_angle(&self) -> f32 {
        match self.side {
            FlipperSide::Left => 0.4,
            FlipperSide::Right => core::f32::consts::PI - 0.4,
        }
    }

    /// Active angle in radians (the angle when pressed).
    fn active_angle(&self) -> f32 {
        match self.side {
            FlipperSide::Left => -FLIPPER_MAX_ANGLE,
            FlipperSide::Right => core::f32::consts::PI + FLIPPER_MAX_ANGLE,
        }
    }

    /// Current absolute angle.
    fn current_angle(&self) -> f32 {
        let rest = self.rest_angle();
        let active = self.active_angle();
        rest + (active - rest) * self.angle
    }

    /// Tip position of the flipper.
    fn tip(&self) -> Vec2 {
        let angle = self.current_angle();
        Vec2::new(
            self.pivot.x + FLIPPER_LENGTH * angle.cos(),
            self.pivot.y + FLIPPER_LENGTH * angle.sin(),
        )
    }

    /// Update flipper rotation based on pressed state.
    fn update(&mut self, dt: f32) {
        if self.pressed {
            self.angle = (self.angle + FLIPPER_SPEED * dt).min(1.0);
        } else {
            self.angle = (self.angle - FLIPPER_SPEED * dt).max(0.0);
        }
    }

    /// Get the closest point on the flipper line segment to a given point.
    fn closest_point(&self, p: Vec2) -> Vec2 {
        let a = self.pivot;
        let b = self.tip();
        let ab = b.sub(a);
        let ap = p.sub(a);
        let t = ap.dot(ab) / ab.length_sq();
        let t = t.clamp(0.0, 1.0);
        a.add(ab.scale(t))
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    /// Ball is in the plunger lane, waiting to be launched.
    ReadyToLaunch,
    /// Space is held, accumulating launch power.
    Launching,
    /// Ball is in play on the table.
    Playing,
    /// Ball has drained, brief pause before next ball or game over.
    BallLost,
    /// Game over, showing final score.
    GameOver,
    /// Game is paused.
    Paused,
}

// ── High score entry ────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct HighScoreEntry {
    score: u32,
}

// ── Tilt tracker ────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct TiltTracker {
    /// Timestamps (in total ms) of recent flipper inputs.
    inputs: Vec<u64>,
    /// Whether tilt has been triggered.
    tilted: bool,
    /// When the tilt was triggered (ms).
    tilt_start_ms: u64,
}

impl TiltTracker {
    fn new() -> Self {
        Self {
            inputs: Vec::new(),
            tilted: false,
            tilt_start_ms: 0,
        }
    }

    fn reset(&mut self) {
        self.inputs.clear();
        self.tilted = false;
        self.tilt_start_ms = 0;
    }

    /// Record a flipper input and check if tilt threshold is exceeded.
    fn record_input(&mut self, current_ms: u64) {
        self.inputs.push(current_ms);
        // Remove old inputs outside the window.
        let cutoff = current_ms.saturating_sub(TILT_WINDOW_MS);
        self.inputs.retain(|&t| t >= cutoff);
        if self.inputs.len() >= TILT_THRESHOLD as usize && !self.tilted {
            self.tilted = true;
            self.tilt_start_ms = current_ms;
        }
    }

    /// Check if the tilt penalty has expired.
    fn is_tilt_active(&self, current_ms: u64) -> bool {
        self.tilted && current_ms.saturating_sub(self.tilt_start_ms) < TILT_PENALTY_MS
    }

    /// Clear tilt if the penalty has expired.
    fn update(&mut self, current_ms: u64) {
        if self.tilted && !self.is_tilt_active(current_ms) {
            self.tilted = false;
            self.inputs.clear();
        }
    }
}

// ── Main app struct ─────────────────────────────────────────────────
struct Pinball {
    /// The primary ball.
    balls: Vec<Ball>,
    /// Left flipper.
    left_flipper: Flipper,
    /// Right flipper.
    right_flipper: Flipper,
    /// Circular bumpers on the table.
    bumpers: Vec<Bumper>,
    /// Drop targets.
    targets: Vec<DropTarget>,
    /// Ramp on the table.
    ramp: Ramp,
    /// Current game phase.
    phase: GamePhase,
    /// Current score.
    score: u32,
    /// Balls remaining (including current ball in play).
    balls_remaining: u32,
    /// Total balls used so far this game.
    balls_used: u32,
    /// High score table.
    high_scores: Vec<HighScoreEntry>,
    /// Current combo multiplier.
    combo: u32,
    /// Time of last scoring event for combo tracking.
    last_score_ms: u64,
    /// Total elapsed time in ms.
    total_ms: u64,
    /// Plunger launch power (0.0 to 1.0).
    launch_power: f32,
    /// Whether multi-ball is active.
    multi_ball_active: bool,
    /// Tilt tracking.
    tilt: TiltTracker,
    /// Extra balls earned counter (to track milestones).
    extra_balls_earned: u32,
    /// RNG.
    rng: Rng,
    /// Time of ball lost event for pause before next ball.
    ball_lost_ms: u64,
    /// Phase before pausing (to restore on unpause).
    phase_before_pause: GamePhase,
    /// Total bumper hits this game.
    total_bumper_hits: u32,
    /// Total target hits this game.
    total_target_hits: u32,
    /// Total ramp completions this game.
    total_ramp_completions: u32,
}

impl Pinball {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let bumpers = vec![
            Bumper::new(100.0, 160.0, BUMPER_RADIUS),
            Bumper::new(170.0, 130.0, BUMPER_RADIUS),
            Bumper::new(135.0, 220.0, BUMPER_RADIUS - 4.0),
        ];

        let targets = vec![
            DropTarget::new(40.0, 280.0, 30.0, 10.0),
            DropTarget::new(80.0, 270.0, 30.0, 10.0),
            DropTarget::new(120.0, 265.0, 30.0, 10.0),
            DropTarget::new(160.0, 270.0, 30.0, 10.0),
            DropTarget::new(200.0, 280.0, 30.0, 10.0),
        ];

        let ramp = Ramp::new(
            Vec2::new(60.0, 350.0),
            Vec2::new(40.0, 100.0),
            40.0,
        );

        let mut app = Self {
            balls: Vec::new(),
            left_flipper: Flipper::new(FlipperSide::Left, Vec2::new(LEFT_FLIPPER_X, FLIPPER_Y)),
            right_flipper: Flipper::new(
                FlipperSide::Right,
                Vec2::new(RIGHT_FLIPPER_X, FLIPPER_Y),
            ),
            bumpers,
            targets,
            ramp,
            phase: GamePhase::ReadyToLaunch,
            score: 0,
            balls_remaining: STARTING_BALLS,
            balls_used: 0,
            high_scores: vec![
                HighScoreEntry { score: 10000 },
                HighScoreEntry { score: 7500 },
                HighScoreEntry { score: 5000 },
                HighScoreEntry { score: 2500 },
                HighScoreEntry { score: 1000 },
            ],
            combo: 1,
            last_score_ms: 0,
            total_ms: 0,
            launch_power: 0.0,
            multi_ball_active: false,
            tilt: TiltTracker::new(),
            extra_balls_earned: 0,
            rng: Rng::new(seed),
            ball_lost_ms: 0,
            phase_before_pause: GamePhase::ReadyToLaunch,
            total_bumper_hits: 0,
            total_target_hits: 0,
            total_ramp_completions: 0,
        };
        app.prepare_ball();
        app
    }

    /// Place a new ball in the plunger lane.
    fn prepare_ball(&mut self) {
        self.balls.clear();
        self.balls.push(Ball::new(Ball::plunger_position()));
        self.phase = GamePhase::ReadyToLaunch;
        self.launch_power = 0.0;
        self.tilt.reset();
    }

    /// Launch the ball from the plunger with the accumulated power.
    fn launch_ball(&mut self) {
        let power = MIN_LAUNCH_POWER + (MAX_LAUNCH_POWER - MIN_LAUNCH_POWER) * self.launch_power;
        if let Some(ball) = self.balls.first_mut() {
            ball.vel = Vec2::new(0.0, -power);
            ball.active = true;
        }
        self.balls_used += 1;
        self.phase = GamePhase::Playing;
        self.launch_power = 0.0;
    }

    /// Start a new game, preserving high scores.
    fn new_game(&mut self) {
        let high_scores = self.high_scores.clone();
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.high_scores = high_scores;
    }

    /// Award points with combo multiplier.
    fn award_points(&mut self, base_points: u32) {
        // Update combo.
        if self.total_ms.saturating_sub(self.last_score_ms) > COMBO_WINDOW_MS {
            self.combo = 1;
        } else {
            self.combo = (self.combo + 1).min(MAX_COMBO);
        }
        self.last_score_ms = self.total_ms;

        let points = base_points * self.combo;
        self.score += points;

        // Check for extra ball milestones.
        let milestone = (self.extra_balls_earned + 1) * EXTRA_BALL_SCORE;
        if self.score >= milestone {
            self.extra_balls_earned += 1;
            self.balls_remaining += 1;
        }
    }

    /// Check if all targets have been hit (for multi-ball activation).
    fn all_targets_hit(&self) -> bool {
        self.targets.iter().all(|t| !t.active)
    }

    /// Reset all targets to active.
    fn reset_targets(&mut self) {
        for target in &mut self.targets {
            target.active = true;
        }
    }

    /// Activate multi-ball mode.
    fn activate_multi_ball(&mut self) {
        if self.multi_ball_active {
            return;
        }
        self.multi_ball_active = true;
        // Add two extra balls at various positions.
        let positions = [
            Vec2::new(100.0, 200.0),
            Vec2::new(180.0, 250.0),
        ];
        for &pos in &positions {
            let mut ball = Ball::new(pos);
            ball.active = true;
            let vx = if self.rng.next_f32() > 0.5 { 80.0 } else { -80.0 };
            ball.vel = Vec2::new(vx, -200.0);
            self.balls.push(ball);
        }
        // Reset targets for another round.
        self.reset_targets();
    }

    /// Handle ball draining (falling off the bottom of the table).
    fn drain_ball(&mut self, ball_idx: usize) {
        if self.multi_ball_active && self.balls.len() > 1 {
            // In multi-ball, just remove the drained ball.
            self.balls.remove(ball_idx);
            if self.balls.len() <= 1 {
                self.multi_ball_active = false;
            }
            return;
        }

        // Single ball mode: lose a ball.
        self.balls_remaining = self.balls_remaining.saturating_sub(1);
        if self.balls_remaining == 0 {
            self.phase = GamePhase::GameOver;
            self.update_high_scores();
        } else {
            self.phase = GamePhase::BallLost;
            self.ball_lost_ms = self.total_ms;
        }
    }

    /// Insert the current score into the high score table if it qualifies.
    fn update_high_scores(&mut self) {
        let score = self.score;
        // Find the position to insert.
        let pos = self.high_scores.iter().position(|h| score > h.score);
        if let Some(idx) = pos {
            self.high_scores.insert(idx, HighScoreEntry { score });
            self.high_scores.truncate(HIGH_SCORE_SLOTS);
        } else if self.high_scores.len() < HIGH_SCORE_SLOTS {
            self.high_scores.push(HighScoreEntry { score });
        }
    }

    // ── Physics ─────────────────────────────────────────────────────

    /// Run one physics step for all balls.
    fn physics_step(&mut self, dt: f32) {
        // Update flippers.
        self.left_flipper.update(dt);
        self.right_flipper.update(dt);

        // Update tilt tracker.
        self.tilt.update(self.total_ms);

        // If tilted, disable flippers.
        if self.tilt.is_tilt_active(self.total_ms) {
            self.left_flipper.angle = 0.0;
            self.right_flipper.angle = 0.0;
        }

        let ball_count = self.balls.len();
        for i in 0..ball_count {
            if !self.balls[i].active {
                continue;
            }

            // Apply gravity.
            self.balls[i].vel.y += GRAVITY * dt;

            // Apply friction.
            self.balls[i].vel = self.balls[i].vel.scale(FRICTION);

            // Clamp speed.
            self.balls[i].vel = self.balls[i].vel.clamp_magnitude(MAX_BALL_SPEED);

            // Update position.
            self.balls[i].pos = self.balls[i].pos.add(self.balls[i].vel.scale(dt));

            // Collisions.
            self.collide_walls(i);
            self.collide_bumpers(i);
            self.collide_targets(i);
            self.collide_flipper_left(i);
            self.collide_flipper_right(i);
            self.check_ramp(i);
            self.check_drain(i);
        }

        // Remove drained balls (process in reverse to keep indices valid).
        let mut drained: Vec<usize> = Vec::new();
        for i in 0..self.balls.len() {
            if self.balls[i].pos.y > TABLE_HEIGHT + BALL_RADIUS * 2.0 {
                drained.push(i);
            }
        }
        for &idx in drained.iter().rev() {
            self.drain_ball(idx);
        }
    }

    /// Collide ball with table walls.
    fn collide_walls(&mut self, idx: usize) {
        let ball = &mut self.balls[idx];

        // Left wall.
        if ball.pos.x - BALL_RADIUS < 0.0 {
            ball.pos.x = BALL_RADIUS;
            ball.vel.x = ball.vel.x.abs() * WALL_RESTITUTION;
        }

        // Right wall (account for plunger lane).
        let right_boundary = if ball.pos.y > TABLE_HEIGHT - 100.0 {
            // In the lower area, the plunger lane is open.
            TABLE_WIDTH
        } else {
            TABLE_WIDTH - PLUNGER_LANE_WIDTH
        };
        if ball.pos.x + BALL_RADIUS > right_boundary {
            ball.pos.x = right_boundary - BALL_RADIUS;
            ball.vel.x = -ball.vel.x.abs() * WALL_RESTITUTION;
        }

        // Top wall.
        if ball.pos.y - BALL_RADIUS < 0.0 {
            ball.pos.y = BALL_RADIUS;
            ball.vel.y = ball.vel.y.abs() * WALL_RESTITUTION;
        }

        // Plunger lane walls: a vertical wall separating the plunger from the
        // main table, extending from the top down to near the flippers.
        let lane_wall_x = TABLE_WIDTH - PLUNGER_LANE_WIDTH;
        if ball.pos.x > lane_wall_x - BALL_RADIUS
            && ball.pos.x < lane_wall_x + BALL_RADIUS
            && ball.pos.y < TABLE_HEIGHT - 100.0
        {
            // Ball is near the lane wall. If coming from the plunger lane (right),
            // it goes over the top; if from the left, bounce off.
            if ball.vel.x > 0.0 {
                ball.pos.x = lane_wall_x - BALL_RADIUS;
                ball.vel.x = -ball.vel.x.abs() * WALL_RESTITUTION;
            }
        }
    }

    /// Collide ball with bumpers.
    fn collide_bumpers(&mut self, idx: usize) {
        let ball_pos = self.balls[idx].pos;
        let ball_vel = self.balls[idx].vel;

        for bumper in &mut self.bumpers {
            let diff = ball_pos.sub(bumper.pos);
            let dist = diff.length();
            let min_dist = BALL_RADIUS + bumper.radius;

            if dist < min_dist && dist > 0.01 {
                // Push ball out of bumper.
                let normal = diff.normalized();
                self.balls[idx].pos = bumper.pos.add(normal.scale(min_dist + 0.5));

                // Reflect velocity and boost.
                let reflected = ball_vel.reflect(normal);
                self.balls[idx].vel = reflected.scale(BUMPER_RESTITUTION);

                bumper.last_hit_ms = self.total_ms;
                bumper.hit_count += 1;
            }
        }

        // Award points for bumper hits (check if any bumper was just hit this frame).
        let current_ms = self.total_ms;
        let bumper_hit_count = self.bumpers.iter().filter(|b| b.last_hit_ms == current_ms).count();
        for _ in 0..bumper_hit_count {
            self.award_points(BUMPER_POINTS);
            self.total_bumper_hits += 1;
        }
    }

    /// Collide ball with drop targets.
    fn collide_targets(&mut self, idx: usize) {
        let ball_pos = self.balls[idx].pos;

        let mut hit_any = false;
        for target in &mut self.targets {
            if !target.active {
                continue;
            }

            // Check if ball overlaps the target rectangle.
            let closest_x = ball_pos.x.clamp(target.pos.x, target.pos.x + target.width);
            let closest_y = ball_pos.y.clamp(target.pos.y, target.pos.y + target.height);
            let dist = Vec2::new(ball_pos.x - closest_x, ball_pos.y - closest_y).length();

            if dist < BALL_RADIUS {
                target.active = false;
                target.hit_flash_ms = self.total_ms;
                hit_any = true;

                // Bounce ball away from target.
                if ball_pos.y < target.pos.y {
                    self.balls[idx].vel.y = -self.balls[idx].vel.y.abs() * WALL_RESTITUTION;
                } else {
                    self.balls[idx].vel.y = self.balls[idx].vel.y.abs() * WALL_RESTITUTION;
                }
            }
        }

        if hit_any {
            self.award_points(TARGET_POINTS);
            self.total_target_hits += 1;

            // Check for multi-ball activation.
            if self.all_targets_hit() {
                self.activate_multi_ball();
            }
        }
    }

    /// Collide ball with the left flipper.
    fn collide_flipper_left(&mut self, idx: usize) {
        self.collide_flipper(idx, FlipperSide::Left);
    }

    /// Collide ball with the right flipper.
    fn collide_flipper_right(&mut self, idx: usize) {
        self.collide_flipper(idx, FlipperSide::Right);
    }

    /// Collide ball with a flipper.
    fn collide_flipper(&mut self, idx: usize, side: FlipperSide) {
        let flipper = match side {
            FlipperSide::Left => &self.left_flipper,
            FlipperSide::Right => &self.right_flipper,
        };

        let ball_pos = self.balls[idx].pos;
        let closest = flipper.closest_point(ball_pos);
        let diff = ball_pos.sub(closest);
        let dist = diff.length();
        let min_dist = BALL_RADIUS + FLIPPER_WIDTH / 2.0;

        if dist < min_dist && dist > 0.01 {
            let normal = diff.normalized();

            // Push ball out of flipper.
            self.balls[idx].pos = closest.add(normal.scale(min_dist + 0.5));

            // Calculate deflection angle based on where the ball hits the flipper.
            let pivot = flipper.pivot;
            let tip = flipper.tip();
            let pivot_to_tip = tip.sub(pivot);
            let pivot_to_ball = ball_pos.sub(pivot);
            let t = if pivot_to_tip.length_sq() > 0.01 {
                pivot_to_ball.dot(pivot_to_tip) / pivot_to_tip.length_sq()
            } else {
                0.5
            };
            let t = t.clamp(0.0, 1.0);

            // More speed boost towards the tip of the flipper.
            let speed_factor = 0.5 + t * 0.5;

            if flipper.pressed && flipper.angle > 0.1 {
                // Active flipper hit: launch ball upward with angle-based deflection.
                let base_angle = match side {
                    FlipperSide::Left => -1.2 + t * 0.8,
                    FlipperSide::Right => -(core::f32::consts::PI - 1.2) - t * 0.8,
                };
                let speed = FLIPPER_HIT_SPEED * speed_factor;
                self.balls[idx].vel = Vec2::new(
                    base_angle.cos() * speed,
                    base_angle.sin() * speed,
                );
            } else {
                // Passive flipper: just bounce.
                let reflected = self.balls[idx].vel.reflect(normal);
                self.balls[idx].vel = reflected.scale(WALL_RESTITUTION);
            }
        }
    }

    /// Check if ball enters the ramp.
    fn check_ramp(&mut self, idx: usize) {
        let ball = &self.balls[idx];
        if self.ramp.ball_entering(ball.pos, ball.vel) {
            // Teleport ball to ramp exit with a kick.
            self.balls[idx].pos = self.ramp.exit;
            self.balls[idx].vel = Vec2::new(80.0, 50.0);
            self.award_points(RAMP_POINTS);
            self.total_ramp_completions += 1;
        }
    }

    /// Check if ball has drained.
    fn check_drain(&mut self, idx: usize) {
        // Drain zone is below the flipper area, between the side gutters.
        let ball = &self.balls[idx];
        // The drain is at the very bottom center of the table.
        if ball.pos.y > TABLE_HEIGHT - 20.0
            && ball.pos.x > 30.0
            && ball.pos.x < TABLE_WIDTH - PLUNGER_LANE_WIDTH - 30.0
        {
            // Mark for drain (will be processed after physics loop).
            self.balls[idx].pos.y = TABLE_HEIGHT + BALL_RADIUS * 3.0;
        }
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            _ => {}
        }
    }

    fn handle_key(&mut self, ke: &KeyEvent) {
        match ke.key {
            // Left flipper: Left Shift or Z.
            Key::LeftShift | Key::Z => {
                if ke.pressed {
                    self.left_flipper.pressed = true;
                    if self.phase == GamePhase::Playing {
                        self.tilt.record_input(self.total_ms);
                    }
                } else {
                    self.left_flipper.pressed = false;
                }
            }
            // Right flipper: Right Shift or M.
            Key::RightShift | Key::M => {
                if ke.pressed {
                    self.right_flipper.pressed = true;
                    if self.phase == GamePhase::Playing {
                        self.tilt.record_input(self.total_ms);
                    }
                } else {
                    self.right_flipper.pressed = false;
                }
            }
            // Space: launch ball (hold for power).
            Key::Space => {
                if ke.pressed {
                    if self.phase == GamePhase::ReadyToLaunch {
                        self.phase = GamePhase::Launching;
                        self.launch_power = 0.0;
                    }
                } else if self.phase == GamePhase::Launching {
                    self.launch_ball();
                }
            }
            // N: new game.
            Key::N => {
                if ke.pressed {
                    self.new_game();
                }
            }
            // P: pause/unpause.
            Key::P => {
                if ke.pressed {
                    match self.phase {
                        GamePhase::Paused => {
                            self.phase = self.phase_before_pause;
                        }
                        GamePhase::GameOver => {}
                        other => {
                            self.phase_before_pause = other;
                            self.phase = GamePhase::Paused;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) {
        self.total_ms += elapsed_ms;

        match self.phase {
            GamePhase::Launching => {
                let dt = elapsed_ms as f32 / 1000.0;
                self.launch_power =
                    (self.launch_power + LAUNCH_POWER_RATE * dt / MAX_LAUNCH_POWER).min(1.0);
            }
            GamePhase::Playing => {
                let dt = elapsed_ms as f32 / 1000.0;
                // Substep for stability.
                let steps = 4;
                let sub_dt = dt / steps as f32;
                for _ in 0..steps {
                    self.physics_step(sub_dt);
                }
            }
            GamePhase::BallLost => {
                // Wait a moment before preparing the next ball.
                if self.total_ms.saturating_sub(self.ball_lost_ms) > 1500 {
                    self.prepare_ball();
                }
            }
            _ => {}
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Full window background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Sidebar.
        self.render_sidebar(&mut cmds);

        // Playfield.
        self.render_table(&mut cmds);

        // Footer.
        self.render_footer(&mut cmds);

        // Overlays.
        self.render_overlays(&mut cmds);

        cmds
    }

    /// Origin X of the table within the window.
    fn table_origin_x() -> f32 {
        SIDEBAR_WIDTH + PADDING * 2.0
    }

    /// Origin Y of the table within the window.
    fn table_origin_y() -> f32 {
        PADDING
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let sx = PADDING;
        let sy = PADDING;
        let sw = SIDEBAR_WIDTH - PADDING;
        let sh = TABLE_HEIGHT;

        // Sidebar background.
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: sw,
            height: sh,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: sy + 15.0,
            text: "PINBALL".to_string(),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score.
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: sy + 50.0,
            text: "SCORE".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: sy + 68.0,
            text: format!("{}", self.score),
            color: YELLOW,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Combo.
        if self.combo > 1 {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: sy + 90.0,
                text: format!("COMBO x{}", self.combo),
                color: PEACH,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Balls remaining.
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: sy + 115.0,
            text: "BALLS".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        // Draw ball indicators.
        for i in 0..self.balls_remaining {
            let bx = sx + 15.0 + i as f32 * 18.0;
            let by = sy + 135.0;
            let r = 6.0;
            cmds.push(RenderCommand::FillRect {
                x: bx - r,
                y: by - r,
                width: r * 2.0,
                height: r * 2.0,
                color: TEAL,
                corner_radii: CornerRadii::all(r),
            });
        }

        // Multi-ball indicator.
        if self.multi_ball_active {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: sy + 160.0,
                text: "MULTI-BALL!".to_string(),
                color: GREEN,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Tilt warning.
        if self.tilt.tilted {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: sy + 180.0,
                text: "TILT!".to_string(),
                color: RED,
                font_size: SCORE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Stats.
        let stats_y = sy + 210.0;
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: stats_y,
            text: "STATS".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        let stat_items = [
            format!("Bumpers: {}", self.total_bumper_hits),
            format!("Targets: {}", self.total_target_hits),
            format!("Ramps: {}", self.total_ramp_completions),
        ];
        for (i, item) in stat_items.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: stats_y + 18.0 + i as f32 * 16.0,
                text: item.clone(),
                color: TEXT_COLOR,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Targets hit indicator.
        let targets_hit = self.targets.iter().filter(|t| !t.active).count();
        let targets_total = self.targets.len();
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: stats_y + 72.0,
            text: format!("Targets: {}/{}", targets_hit, targets_total),
            color: if targets_hit == targets_total { GREEN } else { SUBTEXT0 },
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // High scores.
        let hs_y = sy + 340.0;
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: hs_y,
            text: "HIGH SCORES".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Line {
            x1: sx + 10.0,
            y1: hs_y + 14.0,
            x2: sx + sw - 10.0,
            y2: hs_y + 14.0,
            color: OVERLAY0,
            width: 1.0,
        });
        for (i, entry) in self.high_scores.iter().enumerate() {
            let rank_color = match i {
                0 => YELLOW,
                1 => SUBTEXT0,
                2 => PEACH,
                _ => OVERLAY0,
            };
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: hs_y + 20.0 + i as f32 * 18.0,
                text: format!("{}. {}", i + 1, entry.score),
                color: rank_color,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Launch power bar (when launching).
        if self.phase == GamePhase::Launching || self.phase == GamePhase::ReadyToLaunch {
            let bar_y = sy + sh - 80.0;
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: bar_y,
                text: "POWER".to_string(),
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            // Bar background.
            let bar_w = sw - 20.0;
            let bar_h = 12.0;
            cmds.push(RenderCommand::FillRect {
                x: sx + 10.0,
                y: bar_y + 16.0,
                width: bar_w,
                height: bar_h,
                color: SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            // Bar fill.
            let fill_color = if self.launch_power > 0.8 {
                RED
            } else if self.launch_power > 0.5 {
                YELLOW
            } else {
                GREEN
            };
            cmds.push(RenderCommand::FillRect {
                x: sx + 10.0,
                y: bar_y + 16.0,
                width: bar_w * self.launch_power,
                height: bar_h,
                color: fill_color,
                corner_radii: CornerRadii::all(3.0),
            });
        }
    }

    fn render_table(&self, cmds: &mut Vec<RenderCommand>) {
        let tx = Self::table_origin_x();
        let ty = Self::table_origin_y();

        // Table background.
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: ty,
            width: TABLE_WIDTH,
            height: TABLE_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Table inner playing surface.
        cmds.push(RenderCommand::FillRect {
            x: tx + 3.0,
            y: ty + 3.0,
            width: TABLE_WIDTH - 6.0,
            height: TABLE_HEIGHT - 6.0,
            color: Color::from_hex(0x252540),
            corner_radii: CornerRadii::all(6.0),
        });

        // Plunger lane.
        let lane_x = tx + TABLE_WIDTH - PLUNGER_LANE_WIDTH;
        cmds.push(RenderCommand::FillRect {
            x: lane_x,
            y: ty + 3.0,
            width: PLUNGER_LANE_WIDTH - 3.0,
            height: TABLE_HEIGHT - 6.0,
            color: Color::from_hex(0x1A1A30),
            corner_radii: CornerRadii::all(3.0),
        });

        // Lane separator line.
        cmds.push(RenderCommand::Line {
            x1: lane_x,
            y1: ty + 3.0,
            x2: lane_x,
            y2: ty + TABLE_HEIGHT - 100.0,
            color: OVERLAY0,
            width: 2.0,
        });

        // Ramp guide lines.
        self.render_ramp(cmds, tx, ty);

        // Bumpers.
        self.render_bumpers(cmds, tx, ty);

        // Drop targets.
        self.render_targets(cmds, tx, ty);

        // Flippers.
        self.render_flippers(cmds, tx, ty);

        // Drain guides (side gutters).
        self.render_drain_guides(cmds, tx, ty);

        // Balls.
        self.render_balls(cmds, tx, ty);

        // Plunger.
        self.render_plunger(cmds, tx, ty);
    }

    fn render_ramp(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        let entry = self.ramp.entry;
        let exit = self.ramp.exit;

        // Ramp entry indicator.
        cmds.push(RenderCommand::Line {
            x1: tx + entry.x - self.ramp.entry_width / 2.0,
            y1: ty + entry.y,
            x2: tx + entry.x + self.ramp.entry_width / 2.0,
            y2: ty + entry.y,
            color: MAUVE,
            width: 3.0,
        });

        // Ramp guide lines from entry to exit.
        cmds.push(RenderCommand::Line {
            x1: tx + entry.x - 10.0,
            y1: ty + entry.y,
            x2: tx + exit.x - 10.0,
            y2: ty + exit.y,
            color: Color::rgba(203, 166, 247, 80),
            width: 1.5,
        });
        cmds.push(RenderCommand::Line {
            x1: tx + entry.x + 10.0,
            y1: ty + entry.y,
            x2: tx + exit.x + 10.0,
            y2: ty + exit.y,
            color: Color::rgba(203, 166, 247, 80),
            width: 1.5,
        });

        // Ramp exit indicator.
        cmds.push(RenderCommand::FillRect {
            x: tx + exit.x - 8.0,
            y: ty + exit.y - 3.0,
            width: 16.0,
            height: 6.0,
            color: MAUVE,
            corner_radii: CornerRadii::all(2.0),
        });

        // Ramp label.
        cmds.push(RenderCommand::Text {
            x: tx + entry.x - 15.0,
            y: ty + entry.y + 8.0,
            text: "RAMP".to_string(),
            color: MAUVE,
            font_size: 9.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_bumpers(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        for bumper in &self.bumpers {
            let bx = tx + bumper.pos.x;
            let by = ty + bumper.pos.y;
            let r = bumper.radius;
            let flashing = bumper.is_flashing(self.total_ms);

            // Outer glow when flashing.
            if flashing {
                let glow_r = r + 4.0;
                cmds.push(RenderCommand::FillRect {
                    x: bx - glow_r,
                    y: by - glow_r,
                    width: glow_r * 2.0,
                    height: glow_r * 2.0,
                    color: Color::rgba(250, 179, 135, 100),
                    corner_radii: CornerRadii::all(glow_r),
                });
            }

            // Bumper body.
            let body_color = if flashing { PEACH } else { BLUE };
            cmds.push(RenderCommand::FillRect {
                x: bx - r,
                y: by - r,
                width: r * 2.0,
                height: r * 2.0,
                color: body_color,
                corner_radii: CornerRadii::all(r),
            });

            // Inner highlight.
            let inner_r = r * 0.5;
            cmds.push(RenderCommand::FillRect {
                x: bx - inner_r,
                y: by - inner_r,
                width: inner_r * 2.0,
                height: inner_r * 2.0,
                color: if flashing { YELLOW } else { LAVENDER },
                corner_radii: CornerRadii::all(inner_r),
            });

            // Hit count text.
            cmds.push(RenderCommand::Text {
                x: bx - 4.0,
                y: by - 5.0,
                text: format!("{}", bumper.hit_count),
                color: BASE,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_targets(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        for target in &self.targets {
            let color = if target.active {
                RED
            } else if target.is_flashing(self.total_ms) {
                YELLOW
            } else {
                OVERLAY0
            };
            cmds.push(RenderCommand::FillRect {
                x: tx + target.pos.x,
                y: ty + target.pos.y,
                width: target.width,
                height: target.height,
                color,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Label above targets.
        cmds.push(RenderCommand::Text {
            x: tx + 90.0,
            y: ty + 255.0,
            text: "TARGETS".to_string(),
            color: SUBTEXT0,
            font_size: 9.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_flippers(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        // Draw each flipper as a thick line from pivot to tip.
        self.render_one_flipper(cmds, tx, ty, &self.left_flipper);
        self.render_one_flipper(cmds, tx, ty, &self.right_flipper);
    }

    fn render_one_flipper(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32, flipper: &Flipper) {
        let pivot = flipper.pivot;
        let tip = flipper.tip();

        // Draw flipper body as a wide line.
        cmds.push(RenderCommand::Line {
            x1: tx + pivot.x,
            y1: ty + pivot.y,
            x2: tx + tip.x,
            y2: ty + tip.y,
            color: TEAL,
            width: FLIPPER_WIDTH,
        });

        // Pivot circle.
        let pr = 5.0;
        cmds.push(RenderCommand::FillRect {
            x: tx + pivot.x - pr,
            y: ty + pivot.y - pr,
            width: pr * 2.0,
            height: pr * 2.0,
            color: GREEN,
            corner_radii: CornerRadii::all(pr),
        });

        // Tip circle.
        let tr = 3.0;
        cmds.push(RenderCommand::FillRect {
            x: tx + tip.x - tr,
            y: ty + tip.y - tr,
            width: tr * 2.0,
            height: tr * 2.0,
            color: GREEN,
            corner_radii: CornerRadii::all(tr),
        });
    }

    fn render_drain_guides(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        let drain_y = ty + TABLE_HEIGHT - 30.0;

        // Left gutter guide.
        cmds.push(RenderCommand::Line {
            x1: tx + 5.0,
            y1: ty + TABLE_HEIGHT - 120.0,
            x2: tx + 30.0,
            y2: drain_y,
            color: OVERLAY0,
            width: 3.0,
        });

        // Right gutter guide.
        let right_edge = TABLE_WIDTH - PLUNGER_LANE_WIDTH;
        cmds.push(RenderCommand::Line {
            x1: tx + right_edge - 5.0,
            y1: ty + TABLE_HEIGHT - 120.0,
            x2: tx + right_edge - 30.0,
            y2: drain_y,
            color: OVERLAY0,
            width: 3.0,
        });

        // Drain area indicator.
        cmds.push(RenderCommand::FillRect {
            x: tx + 30.0,
            y: drain_y,
            width: right_edge - 60.0,
            height: 4.0,
            color: RED,
            corner_radii: CornerRadii::all(2.0),
        });
    }

    fn render_balls(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        for ball in &self.balls {
            if !ball.active && self.phase != GamePhase::ReadyToLaunch && self.phase != GamePhase::Launching {
                continue;
            }
            let bx = tx + ball.pos.x;
            let by = ty + ball.pos.y;
            let r = BALL_RADIUS;

            // Ball shadow.
            cmds.push(RenderCommand::FillRect {
                x: bx - r + 1.5,
                y: by - r + 1.5,
                width: r * 2.0,
                height: r * 2.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::all(r),
            });

            // Ball body.
            cmds.push(RenderCommand::FillRect {
                x: bx - r,
                y: by - r,
                width: r * 2.0,
                height: r * 2.0,
                color: TEXT_COLOR,
                corner_radii: CornerRadii::all(r),
            });

            // Ball highlight.
            let hr = r * 0.35;
            cmds.push(RenderCommand::FillRect {
                x: bx - hr - 1.0,
                y: by - hr - 1.0,
                width: hr * 2.0,
                height: hr * 2.0,
                color: Color::rgba(255, 255, 255, 150),
                corner_radii: CornerRadii::all(hr),
            });
        }
    }

    fn render_plunger(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        if self.phase != GamePhase::ReadyToLaunch && self.phase != GamePhase::Launching {
            return;
        }

        let lane_x = tx + TABLE_WIDTH - PLUNGER_LANE_WIDTH;
        let plunger_base_y = ty + TABLE_HEIGHT - 20.0;
        let plunger_top_y = plunger_base_y - 30.0 + self.launch_power * 20.0;

        // Plunger rod.
        cmds.push(RenderCommand::FillRect {
            x: lane_x + 6.0,
            y: plunger_top_y,
            width: PLUNGER_LANE_WIDTH - 12.0,
            height: plunger_base_y - plunger_top_y,
            color: OVERLAY0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Plunger head.
        cmds.push(RenderCommand::FillRect {
            x: lane_x + 3.0,
            y: plunger_top_y - 6.0,
            width: PLUNGER_LANE_WIDTH - 6.0,
            height: 10.0,
            color: PEACH,
            corner_radii: CornerRadii::all(3.0),
        });

        // Spring coils.
        let coil_count = 4;
        let coil_spacing = (plunger_base_y - plunger_top_y) / (coil_count + 1) as f32;
        for i in 1..=coil_count {
            let cy = plunger_top_y + coil_spacing * i as f32;
            cmds.push(RenderCommand::Line {
                x1: lane_x + 8.0,
                y1: cy,
                x2: lane_x + PLUNGER_LANE_WIDTH - 8.0,
                y2: cy,
                color: SUBTEXT0,
                width: 1.0,
            });
        }
    }

    fn render_footer(&self, cmds: &mut Vec<RenderCommand>) {
        let fy = WINDOW_HEIGHT - FOOTER_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: fy,
            width: WINDOW_WIDTH,
            height: FOOTER_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: fy + 12.0,
            text: "L-Shift/Z: Left Flip | R-Shift/M: Right Flip | Space: Launch | N: New | P: Pause".to_string(),
            color: OVERLAY0,
            font_size: FOOTER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_overlays(&self, cmds: &mut Vec<RenderCommand>) {
        match self.phase {
            GamePhase::Paused => self.render_pause_overlay(cmds),
            GamePhase::GameOver => self.render_game_over_overlay(cmds),
            GamePhase::BallLost => self.render_ball_lost_overlay(cmds),
            _ => {}
        }
    }

    fn render_pause_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let tx = Self::table_origin_x();
        let ty = Self::table_origin_y();

        // Dim background.
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: ty,
            width: TABLE_WIDTH,
            height: TABLE_HEIGHT,
            color: Color::rgba(0, 0, 0, 150),
            corner_radii: CornerRadii::all(8.0),
        });

        // Pause text.
        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 40.0,
            y: ty + TABLE_HEIGHT / 2.0 - 15.0,
            text: "PAUSED".to_string(),
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 60.0,
            y: ty + TABLE_HEIGHT / 2.0 + 15.0,
            text: "Press P to resume".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_game_over_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let tx = Self::table_origin_x();
        let ty = Self::table_origin_y();

        // Dim background.
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: ty,
            width: TABLE_WIDTH,
            height: TABLE_HEIGHT,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 55.0,
            y: ty + TABLE_HEIGHT / 2.0 - 40.0,
            text: "GAME OVER".to_string(),
            color: RED,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 50.0,
            y: ty + TABLE_HEIGHT / 2.0 - 10.0,
            text: format!("Score: {}", self.score),
            color: YELLOW,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 70.0,
            y: ty + TABLE_HEIGHT / 2.0 + 20.0,
            text: "Press N for new game".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_ball_lost_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let tx = Self::table_origin_x();
        let ty = Self::table_origin_y();

        cmds.push(RenderCommand::FillRect {
            x: tx + TABLE_WIDTH / 2.0 - 60.0,
            y: ty + TABLE_HEIGHT / 2.0 - 20.0,
            width: 120.0,
            height: 30.0,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 40.0,
            y: ty + TABLE_HEIGHT / 2.0 - 8.0,
            text: "BALL LOST".to_string(),
            color: RED,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    // ── Query methods (used by tests) ───────────────────────────────

    fn active_ball_count(&self) -> usize {
        self.balls.iter().filter(|b| b.active).count()
    }

    fn is_paused(&self) -> bool {
        self.phase == GamePhase::Paused
    }

    fn is_game_over(&self) -> bool {
        self.phase == GamePhase::GameOver
    }

    fn is_playing(&self) -> bool {
        self.phase == GamePhase::Playing
    }

    fn is_launching(&self) -> bool {
        self.phase == GamePhase::Launching
    }

    fn is_ready_to_launch(&self) -> bool {
        self.phase == GamePhase::ReadyToLaunch
    }

    fn left_flipper_pressed(&self) -> bool {
        self.left_flipper.pressed
    }

    fn right_flipper_pressed(&self) -> bool {
        self.right_flipper.pressed
    }
}

fn main() {
    let _app = Pinball::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a game with a fixed seed.
    fn test_app() -> Pinball {
        Pinball::with_seed(12345)
    }

    /// Helper to create a key press event.
    fn key_press(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    /// Helper to create a key release event.
    fn key_release(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    /// Helper to create a tick event.
    fn tick(ms: u64) -> Event {
        Event::Tick { elapsed_ms: ms }
    }

    /// Launch the ball and advance time so it's in play.
    fn launch_and_play(app: &mut Pinball) {
        app.handle_event(&key_press(Key::Space));
        // Accumulate some power.
        app.handle_event(&tick(200));
        app.handle_event(&key_release(Key::Space));
        // Let physics run.
        app.handle_event(&tick(16));
    }

    // ── Construction & initialization ───────────────────────────────

    #[test]
    fn test_initial_phase_ready() {
        let app = test_app();
        assert!(app.is_ready_to_launch());
    }

    #[test]
    fn test_initial_score_zero() {
        let app = test_app();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_initial_balls_remaining() {
        let app = test_app();
        assert_eq!(app.balls_remaining, STARTING_BALLS);
    }

    #[test]
    fn test_initial_balls_used_zero() {
        let app = test_app();
        assert_eq!(app.balls_used, 0);
    }

    #[test]
    fn test_initial_has_one_ball() {
        let app = test_app();
        assert_eq!(app.balls.len(), 1);
    }

    #[test]
    fn test_initial_ball_inactive() {
        let app = test_app();
        assert!(!app.balls[0].active);
    }

    #[test]
    fn test_initial_ball_position() {
        let app = test_app();
        let plunger_pos = Ball::plunger_position();
        assert!((app.balls[0].pos.x - plunger_pos.x).abs() < 0.01);
        assert!((app.balls[0].pos.y - plunger_pos.y).abs() < 0.01);
    }

    #[test]
    fn test_initial_ball_velocity_zero() {
        let app = test_app();
        assert!((app.balls[0].vel.x).abs() < 0.01);
        assert!((app.balls[0].vel.y).abs() < 0.01);
    }

    #[test]
    fn test_initial_flippers_not_pressed() {
        let app = test_app();
        assert!(!app.left_flipper_pressed());
        assert!(!app.right_flipper_pressed());
    }

    #[test]
    fn test_initial_flipper_angle_zero() {
        let app = test_app();
        assert!((app.left_flipper.angle).abs() < 0.01);
        assert!((app.right_flipper.angle).abs() < 0.01);
    }

    #[test]
    fn test_initial_bumper_count() {
        let app = test_app();
        assert_eq!(app.bumpers.len(), 3);
    }

    #[test]
    fn test_initial_target_count() {
        let app = test_app();
        assert_eq!(app.targets.len(), MULTIBALL_TARGET_COUNT);
    }

    #[test]
    fn test_initial_all_targets_active() {
        let app = test_app();
        assert!(app.targets.iter().all(|t| t.active));
    }

    #[test]
    fn test_initial_no_multi_ball() {
        let app = test_app();
        assert!(!app.multi_ball_active);
    }

    #[test]
    fn test_initial_combo_is_one() {
        let app = test_app();
        assert_eq!(app.combo, 1);
    }

    #[test]
    fn test_initial_high_scores_populated() {
        let app = test_app();
        assert_eq!(app.high_scores.len(), HIGH_SCORE_SLOTS);
    }

    #[test]
    fn test_initial_no_tilt() {
        let app = test_app();
        assert!(!app.tilt.tilted);
    }

    #[test]
    fn test_initial_launch_power_zero() {
        let app = test_app();
        assert!((app.launch_power).abs() < 0.01);
    }

    #[test]
    fn test_initial_extra_balls_earned_zero() {
        let app = test_app();
        assert_eq!(app.extra_balls_earned, 0);
    }

    #[test]
    fn test_initial_stats_zero() {
        let app = test_app();
        assert_eq!(app.total_bumper_hits, 0);
        assert_eq!(app.total_target_hits, 0);
        assert_eq!(app.total_ramp_completions, 0);
    }

    // ── Vec2 math ───────────────────────────────────────────────────

    #[test]
    fn test_vec2_zero() {
        let v = Vec2::ZERO;
        assert!((v.x).abs() < 0.001);
        assert!((v.y).abs() < 0.001);
    }

    #[test]
    fn test_vec2_length() {
        let v = Vec2::new(3.0, 4.0);
        assert!((v.length() - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_length_sq() {
        let v = Vec2::new(3.0, 4.0);
        assert!((v.length_sq() - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_normalized() {
        let v = Vec2::new(3.0, 4.0).normalized();
        assert!((v.length() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_normalized_zero() {
        let v = Vec2::ZERO.normalized();
        assert!((v.x).abs() < 0.001);
        assert!((v.y).abs() < 0.001);
    }

    #[test]
    fn test_vec2_dot() {
        let a = Vec2::new(1.0, 0.0);
        let b = Vec2::new(0.0, 1.0);
        assert!((a.dot(b)).abs() < 0.001);
    }

    #[test]
    fn test_vec2_dot_parallel() {
        let a = Vec2::new(2.0, 0.0);
        let b = Vec2::new(3.0, 0.0);
        assert!((a.dot(b) - 6.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_add() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(3.0, 4.0);
        let c = a.add(b);
        assert!((c.x - 4.0).abs() < 0.001);
        assert!((c.y - 6.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_sub() {
        let a = Vec2::new(5.0, 7.0);
        let b = Vec2::new(2.0, 3.0);
        let c = a.sub(b);
        assert!((c.x - 3.0).abs() < 0.001);
        assert!((c.y - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_scale() {
        let v = Vec2::new(2.0, 3.0).scale(2.0);
        assert!((v.x - 4.0).abs() < 0.001);
        assert!((v.y - 6.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_reflect() {
        // Reflect (1, -1) off a horizontal surface (normal = (0, 1)).
        let v = Vec2::new(1.0, -1.0);
        let n = Vec2::new(0.0, 1.0);
        let r = v.reflect(n);
        assert!((r.x - 1.0).abs() < 0.001);
        assert!((r.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_clamp_magnitude() {
        let v = Vec2::new(30.0, 40.0); // length = 50
        let c = v.clamp_magnitude(10.0);
        assert!((c.length() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_vec2_clamp_magnitude_no_change() {
        let v = Vec2::new(3.0, 4.0); // length = 5
        let c = v.clamp_magnitude(10.0);
        assert!((c.x - 3.0).abs() < 0.001);
        assert!((c.y - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_rotate_90() {
        let v = Vec2::new(1.0, 0.0);
        let r = v.rotate(core::f32::consts::FRAC_PI_2);
        assert!((r.x).abs() < 0.001);
        assert!((r.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_distance_to() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(3.0, 4.0);
        assert!((a.distance_to(b) - 5.0).abs() < 0.001);
    }

    // ── RNG ─────────────────────────────────────────────────────────

    #[test]
    fn test_rng_deterministic() {
        let mut rng1 = Rng::new(42);
        let mut rng2 = Rng::new(42);
        for _ in 0..10 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut rng1 = Rng::new(42);
        let mut rng2 = Rng::new(99);
        assert_ne!(rng1.next_u64(), rng2.next_u64());
    }

    #[test]
    fn test_rng_bounded() {
        let mut rng = Rng::new(42);
        for _ in 0..100 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_rng_f32_range() {
        let mut rng = Rng::new(42);
        for _ in 0..100 {
            let val = rng.next_f32();
            assert!(val >= 0.0 && val < 1.0);
        }
    }

    // ── Ball ────────────────────────────────────────────────────────

    #[test]
    fn test_ball_plunger_position() {
        let pos = Ball::plunger_position();
        assert!(pos.x > TABLE_WIDTH - PLUNGER_LANE_WIDTH);
        assert!(pos.x < TABLE_WIDTH);
        assert!(pos.y > TABLE_HEIGHT / 2.0);
    }

    #[test]
    fn test_ball_new_inactive() {
        let ball = Ball::new(Vec2::new(100.0, 100.0));
        assert!(!ball.active);
    }

    // ── Bumper ──────────────────────────────────────────────────────

    #[test]
    fn test_bumper_not_initially_flashing() {
        let bumper = Bumper::new(100.0, 100.0, 20.0);
        assert!(!bumper.is_flashing(1000));
    }

    #[test]
    fn test_bumper_flashing_after_hit() {
        let mut bumper = Bumper::new(100.0, 100.0, 20.0);
        bumper.last_hit_ms = 1000;
        assert!(bumper.is_flashing(1100));
    }

    #[test]
    fn test_bumper_flash_expires() {
        let mut bumper = Bumper::new(100.0, 100.0, 20.0);
        bumper.last_hit_ms = 1000;
        assert!(!bumper.is_flashing(1000 + BUMPER_FLASH_MS + 1));
    }

    // ── Drop target ─────────────────────────────────────────────────

    #[test]
    fn test_target_initially_active() {
        let target = DropTarget::new(50.0, 50.0, 30.0, 10.0);
        assert!(target.active);
    }

    #[test]
    fn test_target_contains_inside() {
        let target = DropTarget::new(50.0, 50.0, 30.0, 10.0);
        assert!(target.contains(Vec2::new(60.0, 55.0)));
    }

    #[test]
    fn test_target_contains_outside() {
        let target = DropTarget::new(50.0, 50.0, 30.0, 10.0);
        assert!(!target.contains(Vec2::new(10.0, 10.0)));
    }

    #[test]
    fn test_target_contains_edge() {
        let target = DropTarget::new(50.0, 50.0, 30.0, 10.0);
        assert!(target.contains(Vec2::new(50.0, 50.0)));
        assert!(target.contains(Vec2::new(80.0, 60.0)));
    }

    // ── Ramp ────────────────────────────────────────────────────────

    #[test]
    fn test_ramp_entry_fast_ball() {
        let ramp = Ramp::new(Vec2::new(60.0, 350.0), Vec2::new(40.0, 100.0), 40.0);
        // Ball right at entry, moving up fast.
        assert!(ramp.ball_entering(Vec2::new(60.0, 350.0), Vec2::new(0.0, -200.0)));
    }

    #[test]
    fn test_ramp_entry_slow_ball() {
        let ramp = Ramp::new(Vec2::new(60.0, 350.0), Vec2::new(40.0, 100.0), 40.0);
        // Ball at entry but moving too slowly.
        assert!(!ramp.ball_entering(Vec2::new(60.0, 350.0), Vec2::new(0.0, -50.0)));
    }

    #[test]
    fn test_ramp_entry_wrong_position() {
        let ramp = Ramp::new(Vec2::new(60.0, 350.0), Vec2::new(40.0, 100.0), 40.0);
        // Ball far from entry.
        assert!(!ramp.ball_entering(Vec2::new(200.0, 200.0), Vec2::new(0.0, -200.0)));
    }

    // ── Flipper ─────────────────────────────────────────────────────

    #[test]
    fn test_flipper_initial_angle_zero() {
        let f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        assert!((f.angle).abs() < 0.001);
    }

    #[test]
    fn test_flipper_pressed_raises() {
        let mut f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        f.pressed = true;
        f.update(0.1);
        assert!(f.angle > 0.0);
    }

    #[test]
    fn test_flipper_released_lowers() {
        let mut f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        f.angle = 0.5;
        f.pressed = false;
        f.update(0.1);
        assert!(f.angle < 0.5);
    }

    #[test]
    fn test_flipper_angle_clamped_max() {
        let mut f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        f.pressed = true;
        for _ in 0..100 {
            f.update(0.1);
        }
        assert!((f.angle - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_flipper_angle_clamped_min() {
        let mut f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        f.pressed = false;
        for _ in 0..100 {
            f.update(0.1);
        }
        assert!((f.angle).abs() < 0.01);
    }

    #[test]
    fn test_flipper_tip_moves() {
        let mut f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        let tip_rest = f.tip();
        f.pressed = true;
        f.update(0.5);
        let tip_active = f.tip();
        // Tip should have moved.
        assert!(tip_rest.distance_to(tip_active) > 1.0);
    }

    #[test]
    fn test_flipper_closest_point_at_pivot() {
        let f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        let closest = f.closest_point(Vec2::new(80.0, 480.0));
        // Should be near the pivot.
        assert!(closest.distance_to(f.pivot) < FLIPPER_LENGTH);
    }

    // ── Tilt tracker ────────────────────────────────────────────────

    #[test]
    fn test_tilt_initially_not_tilted() {
        let t = TiltTracker::new();
        assert!(!t.tilted);
    }

    #[test]
    fn test_tilt_not_triggered_few_inputs() {
        let mut t = TiltTracker::new();
        for i in 0..5 {
            t.record_input(i * 50);
        }
        assert!(!t.tilted);
    }

    #[test]
    fn test_tilt_triggered_many_rapid_inputs() {
        let mut t = TiltTracker::new();
        for i in 0..TILT_THRESHOLD {
            t.record_input(i as u64 * 50);
        }
        assert!(t.tilted);
    }

    #[test]
    fn test_tilt_active_during_penalty() {
        let mut t = TiltTracker::new();
        for i in 0..TILT_THRESHOLD {
            t.record_input(i as u64 * 50);
        }
        let trigger_time = (TILT_THRESHOLD - 1) as u64 * 50;
        assert!(t.is_tilt_active(trigger_time + 100));
    }

    #[test]
    fn test_tilt_expires_after_penalty() {
        let mut t = TiltTracker::new();
        for i in 0..TILT_THRESHOLD {
            t.record_input(i as u64 * 50);
        }
        let trigger_time = (TILT_THRESHOLD - 1) as u64 * 50;
        assert!(!t.is_tilt_active(trigger_time + TILT_PENALTY_MS + 1));
    }

    #[test]
    fn test_tilt_reset_clears() {
        let mut t = TiltTracker::new();
        for i in 0..TILT_THRESHOLD {
            t.record_input(i as u64 * 50);
        }
        t.reset();
        assert!(!t.tilted);
        assert!(t.inputs.is_empty());
    }

    // ── Launching ───────────────────────────────────────────────────

    #[test]
    fn test_space_press_starts_launching() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Space));
        assert!(app.is_launching());
    }

    #[test]
    fn test_launch_power_increases() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Space));
        app.handle_event(&tick(200));
        assert!(app.launch_power > 0.0);
    }

    #[test]
    fn test_space_release_launches_ball() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Space));
        app.handle_event(&tick(200));
        app.handle_event(&key_release(Key::Space));
        assert!(app.is_playing());
    }

    #[test]
    fn test_launched_ball_active() {
        let mut app = test_app();
        launch_and_play(&mut app);
        assert!(app.balls[0].active);
    }

    #[test]
    fn test_launched_ball_moves_upward() {
        let mut app = test_app();
        launch_and_play(&mut app);
        assert!(app.balls[0].vel.y < 0.0);
    }

    #[test]
    fn test_launch_increments_balls_used() {
        let mut app = test_app();
        launch_and_play(&mut app);
        assert_eq!(app.balls_used, 1);
    }

    #[test]
    fn test_launch_power_clamped_to_one() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Space));
        // Hold for a very long time.
        for _ in 0..100 {
            app.handle_event(&tick(100));
        }
        assert!((app.launch_power - 1.0).abs() < 0.01);
    }

    // ── Flipper controls ────────────────────────────────────────────

    #[test]
    fn test_left_shift_activates_left_flipper() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::LeftShift));
        assert!(app.left_flipper_pressed());
    }

    #[test]
    fn test_left_shift_release_deactivates() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::LeftShift));
        app.handle_event(&key_release(Key::LeftShift));
        assert!(!app.left_flipper_pressed());
    }

    #[test]
    fn test_z_activates_left_flipper() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Z));
        assert!(app.left_flipper_pressed());
    }

    #[test]
    fn test_right_shift_activates_right_flipper() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::RightShift));
        assert!(app.right_flipper_pressed());
    }

    #[test]
    fn test_m_activates_right_flipper() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::M));
        assert!(app.right_flipper_pressed());
    }

    #[test]
    fn test_m_release_deactivates_right() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::M));
        app.handle_event(&key_release(Key::M));
        assert!(!app.right_flipper_pressed());
    }

    // ── Pause ───────────────────────────────────────────────────────

    #[test]
    fn test_pause_from_ready() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::P));
        assert!(app.is_paused());
    }

    #[test]
    fn test_unpause() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::P));
        app.handle_event(&key_press(Key::P));
        assert!(app.is_ready_to_launch());
    }

    #[test]
    fn test_pause_from_playing() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.handle_event(&key_press(Key::P));
        assert!(app.is_paused());
    }

    #[test]
    fn test_unpause_restores_playing() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.handle_event(&key_press(Key::P));
        app.handle_event(&key_press(Key::P));
        assert!(app.is_playing());
    }

    #[test]
    fn test_pause_does_not_advance_physics() {
        let mut app = test_app();
        launch_and_play(&mut app);
        let pos_before = app.balls[0].pos;
        app.handle_event(&key_press(Key::P));
        app.handle_event(&tick(100));
        // Ball should not move while paused.
        assert!((app.balls[0].pos.x - pos_before.x).abs() < 0.01);
        assert!((app.balls[0].pos.y - pos_before.y).abs() < 0.01);
    }

    // ── New game ────────────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_score() {
        let mut app = test_app();
        app.score = 5000;
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_new_game_resets_balls() {
        let mut app = test_app();
        app.balls_remaining = 1;
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.balls_remaining, STARTING_BALLS);
    }

    #[test]
    fn test_new_game_preserves_high_scores() {
        let mut app = test_app();
        let hs = app.high_scores.clone();
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.high_scores.len(), hs.len());
        for (a, b) in app.high_scores.iter().zip(hs.iter()) {
            assert_eq!(a.score, b.score);
        }
    }

    #[test]
    fn test_new_game_resets_phase() {
        let mut app = test_app();
        app.phase = GamePhase::GameOver;
        app.handle_event(&key_press(Key::N));
        assert!(app.is_ready_to_launch());
    }

    // ── Scoring ─────────────────────────────────────────────────────

    #[test]
    fn test_award_points_basic() {
        let mut app = test_app();
        app.award_points(100);
        assert_eq!(app.score, 100);
    }

    #[test]
    fn test_combo_increments() {
        let mut app = test_app();
        app.total_ms = 1000;
        app.award_points(100);
        app.total_ms = 1500; // Within combo window.
        app.award_points(100);
        assert_eq!(app.combo, 2);
    }

    #[test]
    fn test_combo_multiplies_score() {
        let mut app = test_app();
        app.total_ms = 1000;
        app.award_points(100);
        app.total_ms = 1500;
        app.award_points(100);
        // First: 100 * 1 = 100, Second: 100 * 2 = 200. Total = 300.
        assert_eq!(app.score, 300);
    }

    #[test]
    fn test_combo_resets_after_window() {
        let mut app = test_app();
        app.total_ms = 1000;
        app.award_points(100);
        app.total_ms = 1000 + COMBO_WINDOW_MS + 1;
        app.award_points(100);
        assert_eq!(app.combo, 1);
    }

    #[test]
    fn test_combo_max() {
        let mut app = test_app();
        for i in 0..10 {
            app.total_ms = 1000 + i * 100;
            app.award_points(100);
        }
        assert_eq!(app.combo, MAX_COMBO);
    }

    #[test]
    fn test_extra_ball_milestone() {
        let mut app = test_app();
        app.award_points(EXTRA_BALL_SCORE);
        assert_eq!(app.extra_balls_earned, 1);
        assert_eq!(app.balls_remaining, STARTING_BALLS + 1);
    }

    // ── High scores ─────────────────────────────────────────────────

    #[test]
    fn test_high_score_insertion() {
        let mut app = test_app();
        app.score = 50000;
        app.update_high_scores();
        assert_eq!(app.high_scores[0].score, 50000);
    }

    #[test]
    fn test_high_score_truncated() {
        let mut app = test_app();
        app.score = 50000;
        app.update_high_scores();
        assert_eq!(app.high_scores.len(), HIGH_SCORE_SLOTS);
    }

    #[test]
    fn test_low_score_not_inserted() {
        let mut app = test_app();
        app.score = 0;
        app.update_high_scores();
        // Score of 0 is not higher than any existing score, so no insertion.
        assert_eq!(app.high_scores[HIGH_SCORE_SLOTS - 1].score, 1000);
    }

    // ── Physics ─────────────────────────────────────────────────────

    #[test]
    fn test_gravity_pulls_ball_down() {
        let mut app = test_app();
        launch_and_play(&mut app);
        let initial_vel_y = app.balls[0].vel.y;
        // Run physics.
        app.handle_event(&tick(100));
        // Velocity should be more positive (downward) due to gravity.
        assert!(app.balls[0].vel.y > initial_vel_y);
    }

    #[test]
    fn test_ball_speed_clamped() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.balls[0].vel = Vec2::new(10000.0, 10000.0);
        app.handle_event(&tick(16));
        assert!(app.balls[0].vel.length() <= MAX_BALL_SPEED + 1.0);
    }

    #[test]
    fn test_wall_collision_left() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.balls[0].pos = Vec2::new(2.0, 300.0);
        app.balls[0].vel = Vec2::new(-100.0, 0.0);
        app.physics_step(0.016);
        assert!(app.balls[0].pos.x >= BALL_RADIUS);
    }

    #[test]
    fn test_wall_collision_top() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.balls[0].pos = Vec2::new(150.0, 2.0);
        app.balls[0].vel = Vec2::new(0.0, -100.0);
        app.physics_step(0.016);
        assert!(app.balls[0].pos.y >= BALL_RADIUS);
    }

    // ── Multi-ball ──────────────────────────────────────────────────

    #[test]
    fn test_all_targets_hit_initially_false() {
        let app = test_app();
        assert!(!app.all_targets_hit());
    }

    #[test]
    fn test_all_targets_hit_when_all_deactivated() {
        let mut app = test_app();
        for target in &mut app.targets {
            target.active = false;
        }
        assert!(app.all_targets_hit());
    }

    #[test]
    fn test_multi_ball_activation() {
        let mut app = test_app();
        launch_and_play(&mut app);
        for target in &mut app.targets {
            target.active = false;
        }
        app.activate_multi_ball();
        assert!(app.multi_ball_active);
        assert!(app.balls.len() > 1);
    }

    #[test]
    fn test_multi_ball_does_not_activate_twice() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.activate_multi_ball();
        let ball_count = app.balls.len();
        app.activate_multi_ball();
        assert_eq!(app.balls.len(), ball_count);
    }

    #[test]
    fn test_reset_targets_reactivates_all() {
        let mut app = test_app();
        for target in &mut app.targets {
            target.active = false;
        }
        app.reset_targets();
        assert!(app.targets.iter().all(|t| t.active));
    }

    // ── Game over and ball loss ─────────────────────────────────────

    #[test]
    fn test_drain_decrements_balls() {
        let mut app = test_app();
        launch_and_play(&mut app);
        let before = app.balls_remaining;
        app.drain_ball(0);
        assert_eq!(app.balls_remaining, before - 1);
    }

    #[test]
    fn test_game_over_on_last_ball() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.balls_remaining = 1;
        app.drain_ball(0);
        assert!(app.is_game_over());
    }

    #[test]
    fn test_ball_lost_phase_with_remaining_balls() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.balls_remaining = 2;
        app.drain_ball(0);
        assert_eq!(app.phase, GamePhase::BallLost);
    }

    #[test]
    fn test_ball_lost_prepares_new_ball_after_delay() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.balls_remaining = 2;
        app.drain_ball(0);
        app.ball_lost_ms = app.total_ms;
        // Advance past the delay.
        app.handle_event(&tick(2000));
        assert!(app.is_ready_to_launch());
    }

    // ── Rendering ───────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = test_app();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut app = test_app();
        app.phase = GamePhase::GameOver;
        let cmds = app.render();
        // Should contain game over text.
        let has_game_over = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("GAME OVER")));
        assert!(has_game_over);
    }

    #[test]
    fn test_render_pause_overlay() {
        let mut app = test_app();
        app.phase = GamePhase::Paused;
        let cmds = app.render();
        let has_paused = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("PAUSED")));
        assert!(has_paused);
    }

    #[test]
    fn test_render_ball_lost_overlay() {
        let mut app = test_app();
        app.phase = GamePhase::BallLost;
        let cmds = app.render();
        let has_ball_lost = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("BALL LOST")));
        assert!(has_ball_lost);
    }

    #[test]
    fn test_render_has_score_text() {
        let app = test_app();
        let cmds = app.render();
        let has_score = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("SCORE")));
        assert!(has_score);
    }

    #[test]
    fn test_render_has_pinball_title() {
        let app = test_app();
        let cmds = app.render();
        let has_title = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("PINBALL")));
        assert!(has_title);
    }

    #[test]
    fn test_render_has_footer() {
        let app = test_app();
        let cmds = app.render();
        let has_footer = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Launch")));
        assert!(has_footer);
    }

    #[test]
    fn test_render_bumpers_visible() {
        let app = test_app();
        let cmds = app.render();
        // Bumpers render as FillRect with rounded corners. There should be at least
        // as many rounded rects as there are bumpers (each bumper renders 2+).
        let rounded_count = cmds.iter().filter(|c| {
            matches!(c, RenderCommand::FillRect { corner_radii, .. } if corner_radii != &CornerRadii::ZERO)
        }).count();
        assert!(rounded_count >= app.bumpers.len());
    }

    #[test]
    fn test_render_with_multi_ball_indicator() {
        let mut app = test_app();
        app.multi_ball_active = true;
        let cmds = app.render();
        let has_multi = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("MULTI-BALL")));
        assert!(has_multi);
    }

    #[test]
    fn test_render_with_tilt_indicator() {
        let mut app = test_app();
        app.tilt.tilted = true;
        let cmds = app.render();
        let has_tilt = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("TILT")));
        assert!(has_tilt);
    }

    #[test]
    fn test_render_high_scores_visible() {
        let app = test_app();
        let cmds = app.render();
        let has_hs = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("HIGH SCORES")));
        assert!(has_hs);
    }

    #[test]
    fn test_render_plunger_when_ready() {
        let app = test_app();
        let cmds = app.render();
        // Plunger renders a FillRect with PEACH color for the head.
        let has_plunger = cmds.iter().any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == PEACH));
        assert!(has_plunger);
    }
}
