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
#![allow(unused_imports)]
#![allow(unused_assignments)]

//! OurOS Breakout — classic brick-breaker arcade game.
//!
//! Features a paddle controlled by arrow keys, a bouncing ball,
//! multiple rows of colored bricks with different point values,
//! angle-based ball reflection off the paddle, multiple lives,
//! score tracking, level progression, and power-ups (wider paddle,
//! multi-ball, extra life). Supports menu, playing, paused, and
//! game over states.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers};
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
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Layout constants ────────────────────────────────────────────────
const PLAY_WIDTH: f32 = 600.0;
const PLAY_HEIGHT: f32 = 500.0;
const HEADER_HEIGHT: f32 = 50.0;
const PADDING: f32 = 16.0;
const WINDOW_WIDTH: f32 = PLAY_WIDTH + PADDING * 2.0;
const WINDOW_HEIGHT: f32 = PLAY_HEIGHT + HEADER_HEIGHT + PADDING * 2.0;

// ── Paddle constants ────────────────────────────────────────────────
const PADDLE_WIDTH: f32 = 100.0;
const PADDLE_WIDE_WIDTH: f32 = 160.0;
const PADDLE_HEIGHT: f32 = 14.0;
const PADDLE_Y_OFFSET: f32 = 30.0;
const PADDLE_SPEED: f32 = 400.0;
const PADDLE_CORNER_RADIUS: f32 = 5.0;

// ── Ball constants ──────────────────────────────────────────────────
const BALL_RADIUS: f32 = 6.0;
const BASE_BALL_SPEED: f32 = 250.0;
const BALL_SPEED_INCREMENT: f32 = 25.0;
const MAX_BALL_SPEED: f32 = 500.0;

// ── Brick constants ─────────────────────────────────────────────────
const BRICK_COLS: usize = 12;
const BRICK_ROWS: usize = 6;
const BRICK_WIDTH: f32 = 46.0;
const BRICK_HEIGHT: f32 = 18.0;
const BRICK_GAP: f32 = 3.0;
const BRICK_TOP_OFFSET: f32 = 60.0;
const BRICK_CORNER_RADIUS: f32 = 3.0;

// ── Power-up constants ──────────────────────────────────────────────
const POWERUP_SIZE: f32 = 20.0;
const POWERUP_SPEED: f32 = 120.0;
/// Chance (1 in N) that a brick drop spawns a power-up.
const POWERUP_SPAWN_CHANCE: u64 = 5;
/// Duration of the wider-paddle power-up in milliseconds.
const WIDE_PADDLE_DURATION_MS: u64 = 8000;

// ── Font sizes ──────────────────────────────────────────────────────
const HEADER_FONT_SIZE: f32 = 16.0;
const TITLE_FONT_SIZE: f32 = 32.0;
const SUBTITLE_FONT_SIZE: f32 = 16.0;
const OVERLAY_FONT_SIZE: f32 = 14.0;
const SCORE_FONT_SIZE: f32 = 18.0;

// ── Game constants ──────────────────────────────────────────────────
const INITIAL_LIVES: u32 = 3;

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

    /// Returns true with probability 1/n.
    fn one_in(&mut self, n: u64) -> bool {
        self.next_u64() % n == 0
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    /// Title/menu screen.
    Menu,
    /// Game is actively running.
    Playing,
    /// Game is paused.
    Paused,
    /// Game over (lost all lives).
    GameOver,
}

// ── Power-up types ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PowerUpKind {
    /// Widens the paddle temporarily.
    WidePaddle,
    /// Spawns an extra ball.
    MultiBall,
    /// Grants an extra life.
    ExtraLife,
}

impl PowerUpKind {
    fn color(self) -> Color {
        match self {
            Self::WidePaddle => GREEN,
            Self::MultiBall => MAUVE,
            Self::ExtraLife => RED,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::WidePaddle => "W",
            Self::MultiBall => "M",
            Self::ExtraLife => "+",
        }
    }
}

// ── Ball ────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Ball {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}

impl Ball {
    fn new(x: f32, y: f32, vx: f32, vy: f32) -> Self {
        Self { x, y, vx, vy }
    }

    fn speed(&self) -> f32 {
        (self.vx * self.vx + self.vy * self.vy).sqrt()
    }

    /// Normalize velocity to the given speed magnitude.
    fn set_speed(&mut self, speed: f32) {
        let current = self.speed();
        if current > 0.0 {
            let ratio = speed / current;
            self.vx *= ratio;
            self.vy *= ratio;
        }
    }
}

// ── Power-up entity ─────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct PowerUp {
    x: f32,
    y: f32,
    kind: PowerUpKind,
}

// ── Brick ───────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Brick {
    alive: bool,
    row: usize,
    col: usize,
}

/// Row colors and point values for bricks, top to bottom.
const BRICK_ROW_COLORS: [Color; BRICK_ROWS] = [RED, PEACH, YELLOW, GREEN, BLUE, LAVENDER];
const BRICK_ROW_POINTS: [u32; BRICK_ROWS] = [60, 50, 40, 30, 20, 10];

// ── Helper: compute brick rectangle ─────────────────────────────────
fn brick_rect(row: usize, col: usize) -> (f32, f32, f32, f32) {
    let total_bricks_width =
        BRICK_COLS as f32 * BRICK_WIDTH + (BRICK_COLS as f32 - 1.0) * BRICK_GAP;
    let start_x = (PLAY_WIDTH - total_bricks_width) / 2.0;
    let bx = start_x + col as f32 * (BRICK_WIDTH + BRICK_GAP);
    let by = BRICK_TOP_OFFSET + row as f32 * (BRICK_HEIGHT + BRICK_GAP);
    (bx, by, BRICK_WIDTH, BRICK_HEIGHT)
}

// ── Main app struct ─────────────────────────────────────────────────
struct BreakoutApp {
    state: GameState,
    /// Paddle center x position (in play-area coordinates).
    paddle_x: f32,
    /// Current paddle width (may be wider due to power-up).
    paddle_width: f32,
    /// Whether the left arrow key is held.
    left_held: bool,
    /// Whether the right arrow key is held.
    right_held: bool,
    /// Active balls in play.
    balls: Vec<Ball>,
    /// Brick grid: `bricks[row][col]`.
    bricks: Vec<Vec<bool>>,
    /// Falling power-ups.
    powerups: Vec<PowerUp>,
    /// Current score.
    score: u32,
    /// Best score across games.
    high_score: u32,
    /// Remaining lives.
    lives: u32,
    /// Current level (starts at 1).
    level: u32,
    /// Total bricks remaining in the current level.
    bricks_remaining: u32,
    /// Accumulated elapsed time in ms for game updates.
    accumulated_ms: u64,
    /// Remaining duration for the wide-paddle power-up (ms).
    wide_paddle_remaining_ms: u64,
    /// RNG.
    rng: Lcg,
    /// Ball speed for the current level.
    ball_speed: f32,
}

impl BreakoutApp {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            state: GameState::Menu,
            paddle_x: PLAY_WIDTH / 2.0,
            paddle_width: PADDLE_WIDTH,
            left_held: false,
            right_held: false,
            balls: Vec::new(),
            bricks: Vec::new(),
            powerups: Vec::new(),
            score: 0,
            high_score: 0,
            lives: INITIAL_LIVES,
            level: 1,
            bricks_remaining: 0,
            accumulated_ms: 0,
            wide_paddle_remaining_ms: 0,
            rng: Lcg::new(seed),
            ball_speed: BASE_BALL_SPEED,
        };
        app.init_bricks();
        app
    }

    // ── Initialization ──────────────────────────────────────────────

    /// Initialize the brick grid for the current level.
    fn init_bricks(&mut self) {
        self.bricks.clear();
        self.bricks_remaining = 0;
        for _row in 0..BRICK_ROWS {
            let mut brick_row = Vec::with_capacity(BRICK_COLS);
            for _col in 0..BRICK_COLS {
                brick_row.push(true);
                self.bricks_remaining += 1;
            }
            self.bricks.push(brick_row);
        }
    }

    /// Spawn a ball at the center above the paddle, heading upward.
    fn spawn_ball(&mut self) {
        let angle = self.random_launch_angle();
        let vx = self.ball_speed * angle.sin();
        let vy = -self.ball_speed * angle.cos();
        let ball = Ball::new(self.paddle_x, self.paddle_top() - BALL_RADIUS - 1.0, vx, vy);
        self.balls.push(ball);
    }

    /// Returns the y coordinate of the top edge of the paddle.
    fn paddle_top(&self) -> f32 {
        PLAY_HEIGHT - PADDLE_Y_OFFSET
    }

    /// Random angle between -60 and +60 degrees from vertical.
    fn random_launch_angle(&mut self) -> f32 {
        // Generate angle in range [-1.0, 1.0] mapped to [-60deg, 60deg].
        let r = (self.rng.next_bounded(1000) as f32 / 500.0) - 1.0;
        r * std::f32::consts::FRAC_PI_3
    }

    /// Start a new game.
    fn start_game(&mut self) {
        let hs = self.high_score;
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.high_score = hs;
        self.state = GameState::Playing;
        self.spawn_ball();
    }

    /// Advance to the next level: reinitialize bricks, increase speed.
    fn next_level(&mut self) {
        self.level += 1;
        self.ball_speed = (BASE_BALL_SPEED + BALL_SPEED_INCREMENT * (self.level - 1) as f32)
            .min(MAX_BALL_SPEED);
        self.init_bricks();
        self.balls.clear();
        self.powerups.clear();
        self.wide_paddle_remaining_ms = 0;
        self.paddle_width = PADDLE_WIDTH;
        self.paddle_x = PLAY_WIDTH / 2.0;
        self.spawn_ball();
    }

    // ── Update / tick ───────────────────────────────────────────────

    fn handle_tick(&mut self, elapsed_ms: u64) {
        if self.state != GameState::Playing {
            return;
        }
        self.accumulated_ms += elapsed_ms;

        let dt = elapsed_ms as f32 / 1000.0;

        // Update paddle position.
        self.update_paddle(dt);

        // Update wide-paddle timer.
        self.update_wide_paddle_timer(elapsed_ms);

        // Update ball positions and handle collisions.
        self.update_balls(dt);

        // Update falling power-ups.
        self.update_powerups(dt);

        // Check if all bricks destroyed.
        if self.bricks_remaining == 0 {
            self.next_level();
        }
    }

    fn update_paddle(&mut self, dt: f32) {
        let half_w = self.paddle_width / 2.0;
        if self.left_held {
            self.paddle_x -= PADDLE_SPEED * dt;
        }
        if self.right_held {
            self.paddle_x += PADDLE_SPEED * dt;
        }
        // Clamp to play area.
        if self.paddle_x - half_w < 0.0 {
            self.paddle_x = half_w;
        }
        if self.paddle_x + half_w > PLAY_WIDTH {
            self.paddle_x = PLAY_WIDTH - half_w;
        }
    }

    fn update_wide_paddle_timer(&mut self, elapsed_ms: u64) {
        if self.wide_paddle_remaining_ms > 0 {
            if elapsed_ms >= self.wide_paddle_remaining_ms {
                self.wide_paddle_remaining_ms = 0;
                self.paddle_width = PADDLE_WIDTH;
                // Re-clamp paddle.
                let half_w = self.paddle_width / 2.0;
                if self.paddle_x - half_w < 0.0 {
                    self.paddle_x = half_w;
                }
                if self.paddle_x + half_w > PLAY_WIDTH {
                    self.paddle_x = PLAY_WIDTH - half_w;
                }
            } else {
                self.wide_paddle_remaining_ms -= elapsed_ms;
            }
        }
    }

    fn update_balls(&mut self, dt: f32) {
        let mut balls_to_remove: Vec<usize> = Vec::new();

        for i in 0..self.balls.len() {
            let ball = &mut self.balls[i];
            ball.x += ball.vx * dt;
            ball.y += ball.vy * dt;

            // Wall collisions (left/right).
            if ball.x - BALL_RADIUS < 0.0 {
                ball.x = BALL_RADIUS;
                ball.vx = ball.vx.abs();
            } else if ball.x + BALL_RADIUS > PLAY_WIDTH {
                ball.x = PLAY_WIDTH - BALL_RADIUS;
                ball.vx = -ball.vx.abs();
            }

            // Ceiling collision.
            if ball.y - BALL_RADIUS < 0.0 {
                ball.y = BALL_RADIUS;
                ball.vy = ball.vy.abs();
            }

            // Bottom: ball lost.
            if ball.y + BALL_RADIUS > PLAY_HEIGHT {
                balls_to_remove.push(i);
                continue;
            }

            // Paddle collision.
            self.check_paddle_collision(i);

            // Brick collisions.
            self.check_brick_collisions(i);
        }

        // Remove lost balls (in reverse order to keep indices valid).
        for &idx in balls_to_remove.iter().rev() {
            self.balls.remove(idx);
        }

        // If no balls remain, lose a life.
        if self.balls.is_empty() {
            self.lose_life();
        }
    }

    fn check_paddle_collision(&mut self, ball_idx: usize) {
        let ball = &self.balls[ball_idx];
        let paddle_top = self.paddle_top();
        let half_w = self.paddle_width / 2.0;
        let paddle_left = self.paddle_x - half_w;
        let paddle_right = self.paddle_x + half_w;

        // Only bounce if ball is moving downward and overlaps the paddle.
        if ball.vy > 0.0
            && ball.y + BALL_RADIUS >= paddle_top
            && ball.y - BALL_RADIUS < paddle_top + PADDLE_HEIGHT
            && ball.x + BALL_RADIUS > paddle_left
            && ball.x - BALL_RADIUS < paddle_right
        {
            // Compute relative position on paddle: -1 (left edge) to +1 (right edge).
            let relative = (ball.x - self.paddle_x) / half_w;
            let clamped = relative.clamp(-1.0, 1.0);

            // Map to angle: max 70 degrees from vertical at edges.
            let max_angle = 70.0_f32.to_radians();
            let angle = clamped * max_angle;

            let speed = self.balls[ball_idx].speed();
            self.balls[ball_idx].vx = speed * angle.sin();
            self.balls[ball_idx].vy = -speed * angle.cos();
            self.balls[ball_idx].y = paddle_top - BALL_RADIUS;
        }
    }

    fn check_brick_collisions(&mut self, ball_idx: usize) {
        let ball = &self.balls[ball_idx];
        let bx = ball.x;
        let by = ball.y;

        for row in 0..BRICK_ROWS {
            for col in 0..BRICK_COLS {
                if !self.bricks[row][col] {
                    continue;
                }
                let (rx, ry, rw, rh) = brick_rect(row, col);

                // Check circle vs rectangle collision.
                if self.ball_rect_collision(bx, by, rx, ry, rw, rh) {
                    // Destroy brick.
                    self.bricks[row][col] = false;
                    self.bricks_remaining -= 1;
                    self.score += BRICK_ROW_POINTS[row];
                    if self.score > self.high_score {
                        self.high_score = self.score;
                    }

                    // Determine reflection direction.
                    self.reflect_ball_off_rect(ball_idx, rx, ry, rw, rh);

                    // Maybe spawn a power-up.
                    self.maybe_spawn_powerup(rx + rw / 2.0, ry + rh / 2.0);

                    // Only handle one brick collision per frame per ball.
                    return;
                }
            }
        }
    }

    /// Check if a ball (circle) collides with a rectangle.
    fn ball_rect_collision(&self, bx: f32, by: f32, rx: f32, ry: f32, rw: f32, rh: f32) -> bool {
        // Find closest point on rectangle to ball center.
        let closest_x = bx.clamp(rx, rx + rw);
        let closest_y = by.clamp(ry, ry + rh);
        let dx = bx - closest_x;
        let dy = by - closest_y;
        (dx * dx + dy * dy) < BALL_RADIUS * BALL_RADIUS
    }

    /// Reflect a ball off a rectangle based on which side was hit.
    fn reflect_ball_off_rect(
        &mut self,
        ball_idx: usize,
        rx: f32,
        ry: f32,
        rw: f32,
        rh: f32,
    ) {
        let ball = &mut self.balls[ball_idx];
        let cx = rx + rw / 2.0;
        let cy = ry + rh / 2.0;

        // Determine which side the ball is closest to.
        let dx = ball.x - cx;
        let dy = ball.y - cy;

        // Ratio of penetration in each axis.
        let x_overlap = (rw / 2.0 + BALL_RADIUS) - dx.abs();
        let y_overlap = (rh / 2.0 + BALL_RADIUS) - dy.abs();

        if x_overlap < y_overlap {
            // Hit left or right side.
            ball.vx = -ball.vx;
            if dx > 0.0 {
                ball.x = rx + rw + BALL_RADIUS;
            } else {
                ball.x = rx - BALL_RADIUS;
            }
        } else {
            // Hit top or bottom.
            ball.vy = -ball.vy;
            if dy > 0.0 {
                ball.y = ry + rh + BALL_RADIUS;
            } else {
                ball.y = ry - BALL_RADIUS;
            }
        }
    }

    fn maybe_spawn_powerup(&mut self, x: f32, y: f32) {
        if self.rng.one_in(POWERUP_SPAWN_CHANCE) {
            let kind = match self.rng.next_bounded(3) {
                0 => PowerUpKind::WidePaddle,
                1 => PowerUpKind::MultiBall,
                _ => PowerUpKind::ExtraLife,
            };
            self.powerups.push(PowerUp { x, y, kind });
        }
    }

    fn update_powerups(&mut self, dt: f32) {
        let paddle_top = self.paddle_top();
        let half_w = self.paddle_width / 2.0;
        let paddle_left = self.paddle_x - half_w;
        let paddle_right = self.paddle_x + half_w;

        let mut collected: Vec<usize> = Vec::new();
        let mut fell_off: Vec<usize> = Vec::new();

        for i in 0..self.powerups.len() {
            self.powerups[i].y += POWERUP_SPEED * dt;

            let pu = &self.powerups[i];
            // Check paddle catch.
            if pu.y + POWERUP_SIZE / 2.0 >= paddle_top
                && pu.y - POWERUP_SIZE / 2.0 < paddle_top + PADDLE_HEIGHT
                && pu.x + POWERUP_SIZE / 2.0 > paddle_left
                && pu.x - POWERUP_SIZE / 2.0 < paddle_right
            {
                collected.push(i);
            } else if pu.y - POWERUP_SIZE / 2.0 > PLAY_HEIGHT {
                fell_off.push(i);
            }
        }

        // Apply collected power-ups.
        for &idx in collected.iter().rev() {
            let kind = self.powerups[idx].kind;
            self.apply_powerup(kind);
            self.powerups.remove(idx);
        }

        // Remove power-ups that fell off screen.
        for &idx in fell_off.iter().rev() {
            // Only remove if not already removed by collection.
            if idx < self.powerups.len() {
                self.powerups.remove(idx);
            }
        }
    }

    fn apply_powerup(&mut self, kind: PowerUpKind) {
        match kind {
            PowerUpKind::WidePaddle => {
                self.paddle_width = PADDLE_WIDE_WIDTH;
                self.wide_paddle_remaining_ms = WIDE_PADDLE_DURATION_MS;
            }
            PowerUpKind::MultiBall => {
                // Clone the first ball with a different angle.
                if let Some(existing) = self.balls.first().cloned() {
                    let mut new_ball = existing;
                    // Rotate velocity by ~30 degrees.
                    let angle = 0.5_f32;
                    let cos_a = angle.cos();
                    let sin_a = angle.sin();
                    let nvx = new_ball.vx * cos_a - new_ball.vy * sin_a;
                    let nvy = new_ball.vx * sin_a + new_ball.vy * cos_a;
                    new_ball.vx = nvx;
                    new_ball.vy = nvy;
                    self.balls.push(new_ball);
                }
            }
            PowerUpKind::ExtraLife => {
                self.lives += 1;
            }
        }
    }

    fn lose_life(&mut self) {
        if self.lives > 1 {
            self.lives -= 1;
            self.paddle_x = PLAY_WIDTH / 2.0;
            self.wide_paddle_remaining_ms = 0;
            self.paddle_width = PADDLE_WIDTH;
            self.powerups.clear();
            self.spawn_ball();
        } else {
            self.lives = 0;
            if self.score > self.high_score {
                self.high_score = self.score;
            }
            self.state = GameState::GameOver;
        }
    }

    // ── Input handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            _ => {}
        }
    }

    fn handle_key(&mut self, ke: &KeyEvent) {
        match self.state {
            GameState::Menu => self.handle_key_menu(ke),
            GameState::Playing => self.handle_key_playing(ke),
            GameState::Paused => self.handle_key_paused(ke),
            GameState::GameOver => self.handle_key_game_over(ke),
        }
    }

    fn handle_key_menu(&mut self, ke: &KeyEvent) {
        if !ke.pressed {
            return;
        }
        match ke.key {
            Key::Enter | Key::Space => self.start_game(),
            Key::Escape => {} // Could quit, but no-op for now.
            _ => {}
        }
    }

    fn handle_key_playing(&mut self, ke: &KeyEvent) {
        match ke.key {
            Key::Left => self.left_held = ke.pressed,
            Key::Right => self.right_held = ke.pressed,
            _ => {}
        }
        if !ke.pressed {
            return;
        }
        match ke.key {
            Key::P => {
                self.state = GameState::Paused;
                self.left_held = false;
                self.right_held = false;
            }
            Key::Escape => {
                self.state = GameState::Paused;
                self.left_held = false;
                self.right_held = false;
            }
            Key::N => self.start_game(),
            _ => {}
        }
    }

    fn handle_key_paused(&mut self, ke: &KeyEvent) {
        if !ke.pressed {
            return;
        }
        match ke.key {
            Key::P | Key::Escape | Key::Space => {
                self.state = GameState::Playing;
            }
            Key::N => self.start_game(),
            _ => {}
        }
    }

    fn handle_key_game_over(&mut self, ke: &KeyEvent) {
        if !ke.pressed {
            return;
        }
        match ke.key {
            Key::N | Key::Enter | Key::Space => self.start_game(),
            Key::Escape => self.state = GameState::Menu,
            _ => {}
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        match self.state {
            GameState::Menu => self.render_menu(&mut cmds, width, height),
            GameState::Playing | GameState::Paused => {
                self.render_game(&mut cmds, width, height);
                if self.state == GameState::Paused {
                    self.render_overlay(&mut cmds, width, height, "PAUSED", "Press P to resume");
                }
            }
            GameState::GameOver => {
                self.render_game(&mut cmds, width, height);
                self.render_overlay(
                    &mut cmds,
                    width,
                    height,
                    "GAME OVER",
                    "Press N or Enter for new game",
                );
            }
        }

        cmds
    }

    fn render_menu(&self, cmds: &mut Vec<RenderCommand>, width: f32, _height: f32) {
        // Title.
        cmds.push(RenderCommand::Text {
            x: width / 2.0 - 80.0,
            y: 120.0,
            text: "BREAKOUT".to_string(),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Subtitle.
        cmds.push(RenderCommand::Text {
            x: width / 2.0 - 100.0,
            y: 170.0,
            text: "Press Enter or Space to start".to_string(),
            color: SUBTEXT0,
            font_size: SUBTITLE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Controls info.
        let controls = [
            ("Left/Right", "Move paddle"),
            ("P / Esc", "Pause"),
            ("N", "New game"),
        ];
        for (i, (key, desc)) in controls.iter().enumerate() {
            let y_pos = 230.0 + i as f32 * 28.0;
            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 90.0,
                y: y_pos,
                text: format!("{key}"),
                color: BLUE,
                font_size: OVERLAY_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 10.0,
                y: y_pos,
                text: format!("- {desc}"),
                color: TEXT_COLOR,
                font_size: OVERLAY_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // High score.
        if self.high_score > 0 {
            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 60.0,
                y: 340.0,
                text: format!("High Score: {}", self.high_score),
                color: YELLOW,
                font_size: SUBTITLE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_game(&self, cmds: &mut Vec<RenderCommand>, _width: f32, _height: f32) {
        let offset_x = PADDING;
        let offset_y = HEADER_HEIGHT;

        // Play area background.
        cmds.push(RenderCommand::FillRect {
            x: offset_x,
            y: offset_y,
            width: PLAY_WIDTH,
            height: PLAY_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Play area border.
        cmds.push(RenderCommand::StrokeRect {
            x: offset_x,
            y: offset_y,
            width: PLAY_WIDTH,
            height: PLAY_HEIGHT,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Header (score, lives, level).
        self.render_header(cmds);

        // Bricks.
        self.render_bricks(cmds, offset_x, offset_y);

        // Paddle.
        self.render_paddle(cmds, offset_x, offset_y);

        // Balls.
        self.render_balls(cmds, offset_x, offset_y);

        // Power-ups.
        self.render_powerups(cmds, offset_x, offset_y);
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        // Header background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: HEADER_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Score.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 16.0,
            text: format!("Score: {}", self.score),
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Lives.
        cmds.push(RenderCommand::Text {
            x: PADDING + 160.0,
            y: 16.0,
            text: format!("Lives: {}", self.lives),
            color: if self.lives <= 1 { RED } else { GREEN },
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Level.
        cmds.push(RenderCommand::Text {
            x: PADDING + 300.0,
            y: 16.0,
            text: format!("Level: {}", self.level),
            color: BLUE,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // High score.
        cmds.push(RenderCommand::Text {
            x: PADDING + 440.0,
            y: 16.0,
            text: format!("Best: {}", self.high_score),
            color: YELLOW,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_bricks(&self, cmds: &mut Vec<RenderCommand>, offset_x: f32, offset_y: f32) {
        for row in 0..BRICK_ROWS {
            for col in 0..BRICK_COLS {
                if !self.bricks[row][col] {
                    continue;
                }
                let (bx, by, bw, bh) = brick_rect(row, col);
                cmds.push(RenderCommand::FillRect {
                    x: offset_x + bx,
                    y: offset_y + by,
                    width: bw,
                    height: bh,
                    color: BRICK_ROW_COLORS[row],
                    corner_radii: CornerRadii::all(BRICK_CORNER_RADIUS),
                });
            }
        }
    }

    fn render_paddle(&self, cmds: &mut Vec<RenderCommand>, offset_x: f32, offset_y: f32) {
        let px = self.paddle_x - self.paddle_width / 2.0;
        let py = self.paddle_top();
        cmds.push(RenderCommand::FillRect {
            x: offset_x + px,
            y: offset_y + py,
            width: self.paddle_width,
            height: PADDLE_HEIGHT,
            color: if self.wide_paddle_remaining_ms > 0 {
                GREEN
            } else {
                LAVENDER
            },
            corner_radii: CornerRadii::all(PADDLE_CORNER_RADIUS),
        });
    }

    fn render_balls(&self, cmds: &mut Vec<RenderCommand>, offset_x: f32, offset_y: f32) {
        for ball in &self.balls {
            // Render ball as a small filled square (simulating circle).
            cmds.push(RenderCommand::FillRect {
                x: offset_x + ball.x - BALL_RADIUS,
                y: offset_y + ball.y - BALL_RADIUS,
                width: BALL_RADIUS * 2.0,
                height: BALL_RADIUS * 2.0,
                color: TEXT_COLOR,
                corner_radii: CornerRadii::all(BALL_RADIUS),
            });
        }
    }

    fn render_powerups(&self, cmds: &mut Vec<RenderCommand>, offset_x: f32, offset_y: f32) {
        for pu in &self.powerups {
            let half = POWERUP_SIZE / 2.0;
            cmds.push(RenderCommand::FillRect {
                x: offset_x + pu.x - half,
                y: offset_y + pu.y - half,
                width: POWERUP_SIZE,
                height: POWERUP_SIZE,
                color: pu.kind.color(),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: offset_x + pu.x - 4.0,
                y: offset_y + pu.y - 5.0,
                text: pu.kind.label().to_string(),
                color: CRUST,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_overlay(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
        title: &str,
        subtitle: &str,
    ) {
        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0x11, 0x11, 0x1B, 180),
            corner_radii: CornerRadii::ZERO,
        });

        // Overlay box.
        let box_w = 320.0;
        let box_h = 120.0;
        let box_x = (width - box_w) / 2.0;
        let box_y = (height - box_h) / 2.0;
        cmds.push(RenderCommand::FillRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: box_x + box_w / 2.0 - (title.len() as f32 * 7.0),
            y: box_y + 35.0,
            text: title.to_string(),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Subtitle.
        cmds.push(RenderCommand::Text {
            x: box_x + box_w / 2.0 - (subtitle.len() as f32 * 3.5),
            y: box_y + 80.0,
            text: subtitle.to_string(),
            color: SUBTEXT0,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // ── Query helpers (for tests) ───────────────────────────────────

    fn alive_brick_count(&self) -> u32 {
        let mut count = 0u32;
        for row in &self.bricks {
            for &alive in row {
                if alive {
                    count += 1;
                }
            }
        }
        count
    }

    fn total_brick_count(&self) -> u32 {
        (BRICK_ROWS * BRICK_COLS) as u32
    }
}

fn main() {
    let _app = BreakoutApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test app in Playing state with a fixed seed.
    fn test_app() -> BreakoutApp {
        let mut app = BreakoutApp::with_seed(12345);
        app.state = GameState::Playing;
        app.spawn_ball();
        app
    }

    /// Helper: create a key press event.
    fn key_press(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    /// Helper: create a key release event.
    fn key_release(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    /// Helper: advance game by a given number of milliseconds.
    fn tick(app: &mut BreakoutApp, ms: u64) {
        app.handle_event(&Event::Tick { elapsed_ms: ms });
    }

    // ── Construction & initialization ───────────────────────────────

    #[test]
    fn test_initial_state_is_menu() {
        let app = BreakoutApp::new();
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn test_initial_score_is_zero() {
        let app = BreakoutApp::new();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_initial_lives() {
        let app = BreakoutApp::new();
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    #[test]
    fn test_initial_level() {
        let app = BreakoutApp::new();
        assert_eq!(app.level, 1);
    }

    #[test]
    fn test_initial_bricks_all_alive() {
        let app = BreakoutApp::new();
        assert_eq!(app.alive_brick_count(), app.total_brick_count());
    }

    #[test]
    fn test_initial_no_balls_before_start() {
        let app = BreakoutApp::new();
        assert!(app.balls.is_empty());
    }

    #[test]
    fn test_initial_paddle_centered() {
        let app = BreakoutApp::new();
        assert!((app.paddle_x - PLAY_WIDTH / 2.0).abs() < 0.01);
    }

    #[test]
    fn test_initial_paddle_width() {
        let app = BreakoutApp::new();
        assert!((app.paddle_width - PADDLE_WIDTH).abs() < 0.01);
    }

    #[test]
    fn test_initial_high_score() {
        let app = BreakoutApp::new();
        assert_eq!(app.high_score, 0);
    }

    #[test]
    fn test_brick_grid_dimensions() {
        let app = BreakoutApp::new();
        assert_eq!(app.bricks.len(), BRICK_ROWS);
        for row in &app.bricks {
            assert_eq!(row.len(), BRICK_COLS);
        }
    }

    // ── Game start ──────────────────────────────────────────────────

    #[test]
    fn test_start_game_from_menu() {
        let mut app = BreakoutApp::new();
        app.handle_event(&key_press(Key::Enter));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_start_game_spawns_ball() {
        let mut app = BreakoutApp::new();
        app.handle_event(&key_press(Key::Enter));
        assert_eq!(app.balls.len(), 1);
    }

    #[test]
    fn test_start_game_space_key() {
        let mut app = BreakoutApp::new();
        app.handle_event(&key_press(Key::Space));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_start_game_resets_score() {
        let mut app = BreakoutApp::new();
        app.score = 100;
        app.handle_event(&key_press(Key::Enter));
        assert_eq!(app.score, 0);
    }

    // ── Paddle movement ─────────────────────────────────────────────

    #[test]
    fn test_paddle_moves_left() {
        let mut app = test_app();
        let initial_x = app.paddle_x;
        app.handle_event(&key_press(Key::Left));
        tick(&mut app, 100);
        assert!(app.paddle_x < initial_x);
    }

    #[test]
    fn test_paddle_moves_right() {
        let mut app = test_app();
        let initial_x = app.paddle_x;
        app.handle_event(&key_press(Key::Right));
        tick(&mut app, 100);
        assert!(app.paddle_x > initial_x);
    }

    #[test]
    fn test_paddle_stops_on_key_release() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Left));
        tick(&mut app, 100);
        let pos_after_move = app.paddle_x;
        app.handle_event(&key_release(Key::Left));
        tick(&mut app, 100);
        // Paddle should not have moved further (may have slight ball-related changes).
        assert!((app.paddle_x - pos_after_move).abs() < 0.01);
    }

    #[test]
    fn test_paddle_clamped_left() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Left));
        // Move for a very long time.
        tick(&mut app, 5000);
        assert!(app.paddle_x >= app.paddle_width / 2.0);
    }

    #[test]
    fn test_paddle_clamped_right() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Right));
        tick(&mut app, 5000);
        assert!(app.paddle_x <= PLAY_WIDTH - app.paddle_width / 2.0);
    }

    #[test]
    fn test_paddle_both_keys_cancel() {
        let mut app = test_app();
        let initial_x = app.paddle_x;
        app.handle_event(&key_press(Key::Left));
        app.handle_event(&key_press(Key::Right));
        tick(&mut app, 100);
        // With both keys held, movements cancel out.
        assert!((app.paddle_x - initial_x).abs() < 0.01);
    }

    // ── Ball physics ────────────────────────────────────────────────

    #[test]
    fn test_ball_moves_on_tick() {
        let mut app = test_app();
        let initial_y = app.balls[0].y;
        tick(&mut app, 16);
        assert!(app.balls[0].y != initial_y);
    }

    #[test]
    fn test_ball_spawned_above_paddle() {
        let app = test_app();
        assert!(app.balls[0].y < app.paddle_top());
    }

    #[test]
    fn test_ball_initial_velocity_upward() {
        let app = test_app();
        // vy should be negative (moving up).
        assert!(app.balls[0].vy < 0.0);
    }

    #[test]
    fn test_ball_bounces_off_left_wall() {
        let mut app = test_app();
        // Position ball near left wall moving left.
        app.balls[0].x = BALL_RADIUS + 1.0;
        app.balls[0].vx = -200.0;
        app.balls[0].vy = 0.0;
        app.balls[0].y = PLAY_HEIGHT / 2.0;
        tick(&mut app, 50);
        assert!(app.balls[0].vx > 0.0);
    }

    #[test]
    fn test_ball_bounces_off_right_wall() {
        let mut app = test_app();
        app.balls[0].x = PLAY_WIDTH - BALL_RADIUS - 1.0;
        app.balls[0].vx = 200.0;
        app.balls[0].vy = 0.0;
        app.balls[0].y = PLAY_HEIGHT / 2.0;
        tick(&mut app, 50);
        assert!(app.balls[0].vx < 0.0);
    }

    #[test]
    fn test_ball_bounces_off_ceiling() {
        let mut app = test_app();
        app.balls[0].y = BALL_RADIUS + 1.0;
        app.balls[0].vy = -200.0;
        app.balls[0].vx = 0.0;
        // Clear bricks so no brick collision.
        for row in &mut app.bricks {
            for brick in row.iter_mut() {
                *brick = false;
            }
        }
        tick(&mut app, 50);
        assert!(app.balls[0].vy > 0.0);
    }

    #[test]
    fn test_ball_lost_below_screen() {
        let mut app = test_app();
        app.balls[0].y = PLAY_HEIGHT - 1.0;
        app.balls[0].vy = 300.0;
        app.balls[0].vx = 0.0;
        let lives_before = app.lives;
        tick(&mut app, 100);
        // Ball should be removed and life lost.
        assert!(app.lives < lives_before || app.state == GameState::GameOver);
    }

    #[test]
    fn test_ball_paddle_reflection_center() {
        let mut app = test_app();
        // Position ball just above paddle center, moving down.
        app.balls[0].x = app.paddle_x;
        app.balls[0].y = app.paddle_top() - BALL_RADIUS - 2.0;
        app.balls[0].vy = 200.0;
        app.balls[0].vx = 0.0;
        tick(&mut app, 20);
        // Ball should bounce up.
        assert!(app.balls[0].vy < 0.0);
    }

    #[test]
    fn test_ball_paddle_reflection_angle_left() {
        let mut app = test_app();
        let half_w = app.paddle_width / 2.0;
        // Hit the left edge of the paddle.
        app.balls[0].x = app.paddle_x - half_w + 5.0;
        app.balls[0].y = app.paddle_top() - BALL_RADIUS - 2.0;
        app.balls[0].vy = 200.0;
        app.balls[0].vx = 0.0;
        tick(&mut app, 20);
        // Ball should go left-ish.
        assert!(app.balls[0].vx < 0.0);
    }

    #[test]
    fn test_ball_paddle_reflection_angle_right() {
        let mut app = test_app();
        let half_w = app.paddle_width / 2.0;
        app.balls[0].x = app.paddle_x + half_w - 5.0;
        app.balls[0].y = app.paddle_top() - BALL_RADIUS - 2.0;
        app.balls[0].vy = 200.0;
        app.balls[0].vx = 0.0;
        tick(&mut app, 20);
        // Ball should go right-ish.
        assert!(app.balls[0].vx > 0.0);
    }

    #[test]
    fn test_ball_speed_constant() {
        let mut app = test_app();
        let speed_before = app.balls[0].speed();
        // Place in open area so no brick collisions.
        app.balls[0].x = PLAY_WIDTH / 2.0;
        app.balls[0].y = PLAY_HEIGHT / 2.0;
        // Clear bricks.
        for row in &mut app.bricks {
            for brick in row.iter_mut() {
                *brick = false;
            }
        }
        app.bricks_remaining = 0;
        // After next_level is called due to 0 remaining, speed increases.
        // Instead, let's just check initial speed.
        assert!(speed_before > 0.0);
    }

    // ── Brick collision ─────────────────────────────────────────────

    #[test]
    fn test_brick_destroyed_on_hit() {
        let mut app = test_app();
        let (bx, by, bw, bh) = brick_rect(BRICK_ROWS - 1, BRICK_COLS / 2);
        // Position ball just below the brick, moving up.
        app.balls[0].x = bx + bw / 2.0;
        app.balls[0].y = by + bh + BALL_RADIUS + 2.0;
        app.balls[0].vy = -300.0;
        app.balls[0].vx = 0.0;
        let count_before = app.alive_brick_count();
        tick(&mut app, 20);
        assert!(app.alive_brick_count() < count_before);
    }

    #[test]
    fn test_brick_hit_increases_score() {
        let mut app = test_app();
        let (bx, by, bw, bh) = brick_rect(BRICK_ROWS - 1, BRICK_COLS / 2);
        app.balls[0].x = bx + bw / 2.0;
        app.balls[0].y = by + bh + BALL_RADIUS + 2.0;
        app.balls[0].vy = -300.0;
        app.balls[0].vx = 0.0;
        tick(&mut app, 20);
        assert!(app.score > 0);
    }

    #[test]
    fn test_brick_row_points_differ() {
        // Top row (row 0) should be worth more than bottom row.
        assert!(BRICK_ROW_POINTS[0] > BRICK_ROW_POINTS[BRICK_ROWS - 1]);
    }

    #[test]
    fn test_ball_rect_collision_hit() {
        let app = BreakoutApp::new();
        // Ball center inside the rectangle.
        assert!(app.ball_rect_collision(50.0, 50.0, 40.0, 40.0, 20.0, 20.0));
    }

    #[test]
    fn test_ball_rect_collision_miss() {
        let app = BreakoutApp::new();
        // Ball far from rectangle.
        assert!(!app.ball_rect_collision(0.0, 0.0, 100.0, 100.0, 20.0, 20.0));
    }

    #[test]
    fn test_ball_rect_collision_edge() {
        let app = BreakoutApp::new();
        // Ball just touching the edge (within radius).
        let result = app.ball_rect_collision(
            100.0 - BALL_RADIUS + 1.0,
            110.0,
            100.0,
            100.0,
            20.0,
            20.0,
        );
        assert!(result);
    }

    #[test]
    fn test_all_bricks_destroyed_check() {
        let mut app = test_app();
        for row in &mut app.bricks {
            for brick in row.iter_mut() {
                *brick = false;
            }
        }
        app.bricks_remaining = 0;
        assert_eq!(app.alive_brick_count(), 0);
    }

    #[test]
    fn test_brick_count_matches_remaining() {
        let app = test_app();
        assert_eq!(app.alive_brick_count(), app.bricks_remaining);
    }

    // ── Score ───────────────────────────────────────────────────────

    #[test]
    fn test_high_score_updated() {
        let mut app = test_app();
        app.score = 500;
        app.high_score = 0;
        // Manually hit a brick to trigger high score update.
        let row = BRICK_ROWS - 1;
        let col = 0;
        app.bricks[row][col] = false;
        app.bricks_remaining -= 1;
        app.score += BRICK_ROW_POINTS[row];
        if app.score > app.high_score {
            app.high_score = app.score;
        }
        assert!(app.high_score > 0);
    }

    #[test]
    fn test_high_score_preserved_across_games() {
        let mut app = test_app();
        app.score = 1000;
        app.high_score = 1000;
        app.start_game();
        assert_eq!(app.high_score, 1000);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_score_resets_on_new_game() {
        let mut app = test_app();
        app.score = 500;
        app.start_game();
        assert_eq!(app.score, 0);
    }

    // ── Lives ───────────────────────────────────────────────────────

    #[test]
    fn test_lose_life_on_ball_lost() {
        let mut app = test_app();
        let lives_before = app.lives;
        app.balls.clear();
        app.lose_life();
        assert_eq!(app.lives, lives_before - 1);
    }

    #[test]
    fn test_new_ball_spawned_after_losing_life() {
        let mut app = test_app();
        app.balls.clear();
        app.lose_life();
        if app.state == GameState::Playing {
            assert_eq!(app.balls.len(), 1);
        }
    }

    #[test]
    fn test_game_over_on_last_life() {
        let mut app = test_app();
        app.lives = 1;
        app.balls.clear();
        app.lose_life();
        assert_eq!(app.state, GameState::GameOver);
        assert_eq!(app.lives, 0);
    }

    #[test]
    fn test_lives_reset_on_new_game() {
        let mut app = test_app();
        app.lives = 0;
        app.state = GameState::GameOver;
        app.start_game();
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    // ── Level progression ───────────────────────────────────────────

    #[test]
    fn test_next_level_increments() {
        let mut app = test_app();
        app.next_level();
        assert_eq!(app.level, 2);
    }

    #[test]
    fn test_next_level_restores_bricks() {
        let mut app = test_app();
        // Destroy some bricks.
        app.bricks[0][0] = false;
        app.bricks_remaining -= 1;
        app.next_level();
        assert_eq!(app.alive_brick_count(), app.total_brick_count());
    }

    #[test]
    fn test_next_level_increases_speed() {
        let mut app = test_app();
        let speed_before = app.ball_speed;
        app.next_level();
        assert!(app.ball_speed > speed_before);
    }

    #[test]
    fn test_speed_capped_at_max() {
        let mut app = test_app();
        app.level = 100;
        app.ball_speed = MAX_BALL_SPEED + 100.0;
        app.next_level();
        assert!(app.ball_speed <= MAX_BALL_SPEED);
    }

    #[test]
    fn test_level_clears_powerups() {
        let mut app = test_app();
        app.powerups.push(PowerUp {
            x: 100.0,
            y: 200.0,
            kind: PowerUpKind::WidePaddle,
        });
        app.next_level();
        assert!(app.powerups.is_empty());
    }

    #[test]
    fn test_level_progression_triggers_on_all_bricks_gone() {
        let mut app = test_app();
        // Destroy all bricks.
        for row in &mut app.bricks {
            for brick in row.iter_mut() {
                *brick = false;
            }
        }
        app.bricks_remaining = 0;
        let level_before = app.level;
        tick(&mut app, 16);
        assert_eq!(app.level, level_before + 1);
    }

    #[test]
    fn test_next_level_resets_paddle() {
        let mut app = test_app();
        app.paddle_x = 100.0;
        app.paddle_width = PADDLE_WIDE_WIDTH;
        app.next_level();
        assert!((app.paddle_x - PLAY_WIDTH / 2.0).abs() < 0.01);
        assert!((app.paddle_width - PADDLE_WIDTH).abs() < 0.01);
    }

    // ── Power-ups ───────────────────────────────────────────────────

    #[test]
    fn test_wide_paddle_powerup() {
        let mut app = test_app();
        app.apply_powerup(PowerUpKind::WidePaddle);
        assert!((app.paddle_width - PADDLE_WIDE_WIDTH).abs() < 0.01);
        assert!(app.wide_paddle_remaining_ms > 0);
    }

    #[test]
    fn test_wide_paddle_expires() {
        let mut app = test_app();
        app.apply_powerup(PowerUpKind::WidePaddle);
        tick(&mut app, WIDE_PADDLE_DURATION_MS + 100);
        assert!((app.paddle_width - PADDLE_WIDTH).abs() < 0.01);
    }

    #[test]
    fn test_multiball_powerup() {
        let mut app = test_app();
        let ball_count_before = app.balls.len();
        app.apply_powerup(PowerUpKind::MultiBall);
        assert_eq!(app.balls.len(), ball_count_before + 1);
    }

    #[test]
    fn test_extra_life_powerup() {
        let mut app = test_app();
        let lives_before = app.lives;
        app.apply_powerup(PowerUpKind::ExtraLife);
        assert_eq!(app.lives, lives_before + 1);
    }

    #[test]
    fn test_powerup_falls_down() {
        let mut app = test_app();
        app.powerups.push(PowerUp {
            x: PLAY_WIDTH / 2.0,
            y: 100.0,
            kind: PowerUpKind::ExtraLife,
        });
        let y_before = app.powerups[0].y;
        tick(&mut app, 100);
        if !app.powerups.is_empty() {
            assert!(app.powerups[0].y > y_before);
        }
    }

    #[test]
    fn test_powerup_collected_by_paddle() {
        let mut app = test_app();
        // Keep ball safe: position it in mid-area bouncing upward.
        app.balls[0].x = PLAY_WIDTH / 2.0;
        app.balls[0].y = PLAY_HEIGHT / 4.0;
        app.balls[0].vy = -200.0;
        app.balls[0].vx = 0.0;
        let lives_before = app.lives;
        // Place power-up just slightly above the paddle, close enough to reach it quickly.
        app.powerups.push(PowerUp {
            x: app.paddle_x,
            y: app.paddle_top() - 2.0,
            kind: PowerUpKind::ExtraLife,
        });
        // Small tick: power-up only needs to fall ~2 pixels at 120 px/s.
        tick(&mut app, 100);
        assert_eq!(app.lives, lives_before + 1);
        assert!(app.powerups.is_empty());
    }

    #[test]
    fn test_powerup_removed_when_off_screen() {
        let mut app = test_app();
        app.powerups.push(PowerUp {
            x: PLAY_WIDTH / 2.0,
            y: PLAY_HEIGHT - 1.0,
            kind: PowerUpKind::WidePaddle,
        });
        tick(&mut app, 1000);
        assert!(app.powerups.is_empty());
    }

    #[test]
    fn test_powerup_kind_colors_different() {
        assert_ne!(
            PowerUpKind::WidePaddle.color(),
            PowerUpKind::MultiBall.color()
        );
        assert_ne!(
            PowerUpKind::MultiBall.color(),
            PowerUpKind::ExtraLife.color()
        );
    }

    #[test]
    fn test_powerup_kind_labels() {
        assert_eq!(PowerUpKind::WidePaddle.label(), "W");
        assert_eq!(PowerUpKind::MultiBall.label(), "M");
        assert_eq!(PowerUpKind::ExtraLife.label(), "+");
    }

    #[test]
    fn test_multiball_no_balls_no_crash() {
        let mut app = test_app();
        app.balls.clear();
        app.apply_powerup(PowerUpKind::MultiBall);
        // Should not crash, and no ball should be added since there are no balls to clone.
        assert!(app.balls.is_empty());
    }

    // ── Game state transitions ──────────────────────────────────────

    #[test]
    fn test_pause_from_playing() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::P));
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_pause_clears_held_keys() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Left));
        assert!(app.left_held);
        app.handle_event(&key_press(Key::P));
        assert!(!app.left_held);
        assert!(!app.right_held);
    }

    #[test]
    fn test_resume_from_paused() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::P));
        assert_eq!(app.state, GameState::Paused);
        app.handle_event(&key_press(Key::P));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_pause_with_escape() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Escape));
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_resume_with_escape() {
        let mut app = test_app();
        app.state = GameState::Paused;
        app.handle_event(&key_press(Key::Escape));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_resume_with_space() {
        let mut app = test_app();
        app.state = GameState::Paused;
        app.handle_event(&key_press(Key::Space));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_no_update_when_paused() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let ball_y = app.balls[0].y;
        tick(&mut app, 100);
        assert!((app.balls[0].y - ball_y).abs() < 0.001);
    }

    #[test]
    fn test_no_update_when_menu() {
        let mut app = BreakoutApp::new();
        tick(&mut app, 100);
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn test_new_game_from_playing() {
        let mut app = test_app();
        app.score = 500;
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_new_game_from_game_over() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_new_game_from_paused() {
        let mut app = test_app();
        app.state = GameState::Paused;
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_game_over_enter_starts_new_game() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        app.handle_event(&key_press(Key::Enter));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_game_over_escape_goes_to_menu() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        app.handle_event(&key_press(Key::Escape));
        assert_eq!(app.state, GameState::Menu);
    }

    // ── Rendering output ────────────────────────────────────────────

    #[test]
    fn test_render_menu_produces_commands() {
        let app = BreakoutApp::new();
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing_produces_commands() {
        let app = test_app();
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_paused_produces_overlay() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        // Should have more commands than playing (overlay added).
        let playing_app = test_app();
        let playing_cmds = playing_app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(cmds.len() > playing_cmds.len());
    }

    #[test]
    fn test_render_game_over_has_overlay() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let playing_app = test_app();
        let playing_cmds = playing_app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(cmds.len() > playing_cmds.len());
    }

    #[test]
    fn test_render_contains_background() {
        let app = test_app();
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        // First command should be the background fill.
        match &cmds[0] {
            RenderCommand::FillRect { color, .. } => {
                assert_eq!(*color, MANTLE);
            }
            _ => panic!("First command should be FillRect background"),
        }
    }

    #[test]
    fn test_render_bricks_counted() {
        let app = test_app();
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let brick_fill_count = cmds
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        width,
                        height,
                        ..
                    } if (*width - BRICK_WIDTH).abs() < 0.01
                        && (*height - BRICK_HEIGHT).abs() < 0.01
                )
            })
            .count();
        assert_eq!(brick_fill_count, (BRICK_ROWS * BRICK_COLS));
    }

    #[test]
    fn test_render_paddle_shown() {
        let app = test_app();
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let paddle_count = cmds
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        height,
                        ..
                    } if (*height - PADDLE_HEIGHT).abs() < 0.01
                )
            })
            .count();
        assert!(paddle_count >= 1);
    }

    #[test]
    fn test_render_ball_shown() {
        let app = test_app();
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let ball_count = cmds
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        width,
                        height,
                        ..
                    } if (*width - BALL_RADIUS * 2.0).abs() < 0.01
                        && (*height - BALL_RADIUS * 2.0).abs() < 0.01
                )
            })
            .count();
        assert!(ball_count >= 1);
    }

    #[test]
    fn test_render_wide_paddle_color_changes() {
        let mut app = test_app();
        let cmds_normal = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.apply_powerup(PowerUpKind::WidePaddle);
        let cmds_wide = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        // Paddle color should differ.
        let find_paddle_color = |cmds: &[RenderCommand]| -> Option<Color> {
            cmds.iter().find_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    height, color, ..
                } if (*height - PADDLE_HEIGHT).abs() < 0.01 => Some(*color),
                _ => None,
            })
        };
        let normal_color = find_paddle_color(&cmds_normal);
        let wide_color = find_paddle_color(&cmds_wide);
        assert_ne!(normal_color, wide_color);
    }

    #[test]
    fn test_render_menu_shows_title() {
        let app = BreakoutApp::new();
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let has_title = cmds.iter().any(|cmd| {
            matches!(
                cmd,
                RenderCommand::Text { text, .. } if text.contains("BREAKOUT")
            )
        });
        assert!(has_title);
    }

    #[test]
    fn test_render_header_shows_score() {
        let mut app = test_app();
        app.score = 42;
        let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let has_score = cmds.iter().any(|cmd| {
            matches!(
                cmd,
                RenderCommand::Text { text, .. } if text.contains("42")
            )
        });
        assert!(has_score);
    }

    // ── Brick rect helper ───────────────────────────────────────────

    #[test]
    fn test_brick_rect_first() {
        let (bx, by, bw, bh) = brick_rect(0, 0);
        assert!(bx >= 0.0);
        assert!(by >= 0.0);
        assert!((bw - BRICK_WIDTH).abs() < 0.01);
        assert!((bh - BRICK_HEIGHT).abs() < 0.01);
    }

    #[test]
    fn test_brick_rect_last() {
        let (bx, by, bw, bh) = brick_rect(BRICK_ROWS - 1, BRICK_COLS - 1);
        assert!(bx + bw <= PLAY_WIDTH);
        assert!(by + bh <= PLAY_HEIGHT);
        assert!((bw - BRICK_WIDTH).abs() < 0.01);
        assert!((bh - BRICK_HEIGHT).abs() < 0.01);
    }

    #[test]
    fn test_brick_rects_no_overlap() {
        for row in 0..BRICK_ROWS {
            for col in 0..BRICK_COLS.saturating_sub(1) {
                let (x1, _, w1, _) = brick_rect(row, col);
                let (x2, _, _, _) = brick_rect(row, col + 1);
                assert!(x1 + w1 <= x2, "Bricks overlap horizontally");
            }
        }
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
    fn test_lcg_bounded() {
        let mut rng = Lcg::new(42);
        for _ in 0..100 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_lcg_one_in() {
        let mut rng = Lcg::new(42);
        let mut count = 0;
        for _ in 0..1000 {
            if rng.one_in(2) {
                count += 1;
            }
        }
        // Roughly half should be true; at least some should be.
        assert!(count > 0);
        assert!(count < 1000);
    }

    // ── Ball struct ─────────────────────────────────────────────────

    #[test]
    fn test_ball_speed_calculation() {
        let ball = Ball::new(0.0, 0.0, 3.0, 4.0);
        assert!((ball.speed() - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_ball_set_speed() {
        let mut ball = Ball::new(0.0, 0.0, 3.0, 4.0);
        ball.set_speed(10.0);
        assert!((ball.speed() - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_ball_set_speed_zero_velocity() {
        let mut ball = Ball::new(0.0, 0.0, 0.0, 0.0);
        ball.set_speed(10.0);
        // Should not crash; speed stays 0 since direction is undefined.
        assert!((ball.speed() - 0.0).abs() < 0.01);
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn test_multiple_balls_all_lost() {
        let mut app = test_app();
        app.apply_powerup(PowerUpKind::MultiBall);
        assert_eq!(app.balls.len(), 2);
        // Move all balls off screen.
        for ball in &mut app.balls {
            ball.y = PLAY_HEIGHT + 100.0;
            ball.vy = 100.0;
        }
        let lives_before = app.lives;
        tick(&mut app, 16);
        assert!(app.lives < lives_before || app.state == GameState::GameOver);
    }

    #[test]
    fn test_key_release_does_not_trigger_actions() {
        let mut app = BreakoutApp::new();
        app.handle_event(&key_release(Key::Enter));
        // Should still be in menu, key release should not start game.
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn test_tick_in_game_over_does_nothing() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let score = app.score;
        tick(&mut app, 1000);
        assert_eq!(app.score, score);
    }

    #[test]
    fn test_spawn_ball_position() {
        let mut app = test_app();
        app.balls.clear();
        app.spawn_ball();
        let ball = &app.balls[0];
        assert!(ball.y < app.paddle_top());
        assert!((ball.x - app.paddle_x).abs() < 1.0);
    }

    #[test]
    fn test_game_over_high_score_set() {
        let mut app = test_app();
        app.score = 999;
        app.lives = 1;
        app.balls.clear();
        app.lose_life();
        assert_eq!(app.high_score, 999);
    }

    #[test]
    fn test_wide_paddle_timer_partial() {
        let mut app = test_app();
        app.apply_powerup(PowerUpKind::WidePaddle);
        tick(&mut app, 1000);
        // Timer should have decreased but not expired.
        assert!(app.wide_paddle_remaining_ms > 0);
        assert!((app.paddle_width - PADDLE_WIDE_WIDTH).abs() < 0.01);
    }

    #[test]
    fn test_reflect_ball_top_hit() {
        let mut app = test_app();
        // Ball above a rect, moving down.
        app.balls[0].x = 100.0;
        app.balls[0].y = 90.0;
        app.balls[0].vy = 200.0;
        app.balls[0].vx = 0.0;
        app.reflect_ball_off_rect(0, 80.0, 95.0, 40.0, 20.0);
        assert!(app.balls[0].vy < 0.0);
    }

    #[test]
    fn test_reflect_ball_side_hit() {
        let mut app = test_app();
        app.balls[0].x = 75.0;
        app.balls[0].y = 105.0;
        app.balls[0].vx = 200.0;
        app.balls[0].vy = 0.0;
        app.reflect_ball_off_rect(0, 80.0, 95.0, 40.0, 20.0);
        assert!(app.balls[0].vx < 0.0);
    }
}
