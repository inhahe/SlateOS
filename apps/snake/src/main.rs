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

//! SlateOS Snake — classic snake arcade game.
//!
//! Features a 20x20 grid-based playfield with arrow-key movement,
//! food and bonus food spawning, collision detection (walls/self),
//! scoring with streak bonuses, three difficulty levels, wrap mode,
//! pause/resume, direction queue for fast input, and a stats panel.
//! Uses an LCG pseudo-random number generator (no external rand crate).

use guitk::color::Color;
use guitk::event::{Event, Key};
#[cfg(test)]
use guitk::event::{KeyEvent, Modifiers};
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
const GRID_COLS: usize = 20;
const GRID_ROWS: usize = 20;
const CELL_SIZE: f32 = 24.0;
const CELL_GAP: f32 = 1.0;
const PADDING: f32 = 12.0;
const HEADER_HEIGHT: f32 = 50.0;
const STATS_PANEL_WIDTH: f32 = 180.0;
const HEADER_FONT_SIZE: f32 = 18.0;
const CELL_FONT_SIZE: f32 = 14.0;
const STATS_FONT_SIZE: f32 = 13.0;
const TITLE_FONT_SIZE: f32 = 24.0;
const OVERLAY_FONT_SIZE: f32 = 16.0;
const CELL_CORNER_RADIUS: f32 = 3.0;

/// Maximum number of queued direction changes.
const MAX_DIR_QUEUE: usize = 2;

/// Number of ticks a bonus food stays alive before disappearing.
const BONUS_FOOD_LIFETIME: u32 = 30;

/// Points awarded for normal food.
const NORMAL_FOOD_POINTS: u32 = 10;

/// Points awarded for bonus food.
const BONUS_FOOD_POINTS: u32 = 50;

/// Streak threshold: after eating N consecutive foods within a time window,
/// a streak multiplier applies.
const STREAK_THRESHOLD: u32 = 3;

/// Multiplier applied when the streak threshold is reached.
const STREAK_MULTIPLIER: u32 = 2;

/// How many ticks between foods still counts as a streak.
const STREAK_WINDOW_TICKS: u32 = 30;

/// Chance (1 in N) that a bonus food spawns after eating normal food.
const BONUS_SPAWN_CHANCE: u64 = 5;

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

// ── Direction ───────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// Returns true if this direction is the opposite of `other`.
    fn is_opposite(self, other: Direction) -> bool {
        matches!(
            (self, other),
            (Direction::Up, Direction::Down)
                | (Direction::Down, Direction::Up)
                | (Direction::Left, Direction::Right)
                | (Direction::Right, Direction::Left)
        )
    }

    /// Returns the (row_delta, col_delta) for movement in this direction.
    fn delta(self) -> (i32, i32) {
        match self {
            Direction::Up => (-1, 0),
            Direction::Down => (1, 0),
            Direction::Left => (0, -1),
            Direction::Right => (0, 1),
        }
    }
}

// ── Grid position ───────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    row: i32,
    col: i32,
}

impl Pos {
    const fn new(row: i32, col: i32) -> Self {
        Self { row, col }
    }

    /// Move this position in the given direction.
    fn moved(self, dir: Direction) -> Self {
        let (dr, dc) = dir.delta();
        Self {
            row: self.row + dr,
            col: self.col + dc,
        }
    }

    /// Wrap position around the grid boundaries.
    fn wrapped(self) -> Self {
        let row = ((self.row % GRID_ROWS as i32) + GRID_ROWS as i32) % GRID_ROWS as i32;
        let col = ((self.col % GRID_COLS as i32) + GRID_COLS as i32) % GRID_COLS as i32;
        Self { row, col }
    }

    /// Check if position is within grid bounds.
    fn in_bounds(self) -> bool {
        self.row >= 0
            && self.row < GRID_ROWS as i32
            && self.col >= 0
            && self.col < GRID_COLS as i32
    }
}

// ── Food types ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FoodKind {
    Normal,
    Bonus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Food {
    pos: Pos,
    kind: FoodKind,
    /// Ticks remaining before this food disappears (only for bonus food).
    ticks_remaining: u32,
}

// ── Difficulty ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// Base tick interval in milliseconds (time between snake moves).
    fn base_interval_ms(self) -> u32 {
        match self {
            Difficulty::Easy => 200,
            Difficulty::Medium => 150,
            Difficulty::Hard => 100,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
        }
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    /// Game is actively running.
    Playing,
    /// Game is paused.
    Paused,
    /// Game over (collision).
    GameOver,
}

// ── Speed level ─────────────────────────────────────────────────────
/// Compute the current speed level from score. Higher score = higher speed.
/// Returns a level from 1..=10.
fn speed_level(score: u32) -> u32 {
    let level = score / 50 + 1;
    if level > 10 { 10 } else { level }
}

/// Compute the tick interval in ms for a given difficulty and speed level.
fn tick_interval_ms(difficulty: Difficulty, level: u32) -> u32 {
    let base = difficulty.base_interval_ms();
    // Each speed level reduces interval by 10%, minimum 50ms.
    let reduction = (level.saturating_sub(1)) * (base / 10);
    let interval = base.saturating_sub(reduction);
    if interval < 50 { 50 } else { interval }
}

// ── Main app struct ─────────────────────────────────────────────────
struct SnakeApp {
    /// Snake body segments, head is at index 0.
    snake: Vec<Pos>,
    /// Current movement direction.
    direction: Direction,
    /// Queued direction changes (buffered for fast input).
    dir_queue: Vec<Direction>,
    /// Normal food currently on the grid.
    food: Food,
    /// Optional bonus food on the grid.
    bonus_food: Option<Food>,
    /// Current game state.
    state: GameState,
    /// Current difficulty setting.
    difficulty: Difficulty,
    /// Whether wrap mode is enabled (snake wraps around edges).
    wrap_mode: bool,
    /// Current score.
    score: u32,
    /// High score across games.
    high_score: u32,
    /// Total foods eaten this game.
    foods_eaten: u32,
    /// Bonus foods eaten this game.
    bonus_eaten: u32,
    /// Current streak counter (consecutive foods within time window).
    streak: u32,
    /// Ticks since last food was eaten (for streak tracking).
    ticks_since_food: u32,
    /// Total game ticks elapsed.
    total_ticks: u32,
    /// Accumulated time in ms (for tick scheduling).
    accumulated_ms: u64,
    /// RNG state.
    rng: Lcg,
    /// Pulsing animation counter (for food rendering).
    pulse_counter: u32,
}

impl SnakeApp {
    /// Create a new Snake game with default settings.
    fn new() -> Self {
        Self::with_seed(42)
    }

    /// Create a new Snake game with a specific RNG seed.
    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            snake: Vec::new(),
            direction: Direction::Right,
            dir_queue: Vec::new(),
            food: Food {
                pos: Pos::new(0, 0),
                kind: FoodKind::Normal,
                ticks_remaining: 0,
            },
            bonus_food: None,
            state: GameState::Playing,
            difficulty: Difficulty::Medium,
            wrap_mode: false,
            score: 0,
            high_score: 0,
            foods_eaten: 0,
            bonus_eaten: 0,
            streak: 0,
            ticks_since_food: 0,
            total_ticks: 0,
            accumulated_ms: 0,
            rng: Lcg::new(seed),
            pulse_counter: 0,
        };
        app.init_snake();
        app.spawn_food();
        app
    }

    /// Initialize the snake at the center of the grid with length 3.
    fn init_snake(&mut self) {
        let center_row = GRID_ROWS as i32 / 2;
        let center_col = GRID_COLS as i32 / 2;
        self.snake.clear();
        // Head at center, body extends to the left.
        for i in 0..3 {
            self.snake.push(Pos::new(center_row, center_col - i));
        }
        self.direction = Direction::Right;
        self.dir_queue.clear();
    }

    /// Restart the game, preserving high score.
    fn restart(&mut self) {
        let high = self.high_score;
        let difficulty = self.difficulty;
        let wrap = self.wrap_mode;
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.high_score = high;
        self.difficulty = difficulty;
        self.wrap_mode = wrap;
    }

    // ── Food spawning ───────────────────────────────────────────────

    /// Spawn normal food at a random empty cell.
    fn spawn_food(&mut self) {
        let pos = self.random_empty_cell();
        self.food = Food {
            pos,
            kind: FoodKind::Normal,
            ticks_remaining: 0,
        };
    }

    /// Spawn bonus food at a random empty cell (not on snake or normal food).
    fn spawn_bonus_food(&mut self) {
        let pos = self.random_empty_cell_excluding(self.food.pos);
        self.bonus_food = Some(Food {
            pos,
            kind: FoodKind::Bonus,
            ticks_remaining: BONUS_FOOD_LIFETIME,
        });
    }

    /// Find a random cell not occupied by the snake.
    fn random_empty_cell(&mut self) -> Pos {
        loop {
            let row = self.rng.next_bounded(GRID_ROWS) as i32;
            let col = self.rng.next_bounded(GRID_COLS) as i32;
            let pos = Pos::new(row, col);
            if !self.snake.contains(&pos) {
                return pos;
            }
        }
    }

    /// Find a random empty cell that is also not at `exclude_pos`.
    fn random_empty_cell_excluding(&mut self, exclude_pos: Pos) -> Pos {
        loop {
            let row = self.rng.next_bounded(GRID_ROWS) as i32;
            let col = self.rng.next_bounded(GRID_COLS) as i32;
            let pos = Pos::new(row, col);
            if !self.snake.contains(&pos) && pos != exclude_pos {
                return pos;
            }
        }
    }

    // ── Input handling ──────────────────────────────────────────────

    /// Handle a key event.
    fn handle_key(&mut self, key: Key, pressed: bool) {
        if !pressed {
            return;
        }

        match self.state {
            GameState::Playing => self.handle_key_playing(key),
            GameState::Paused => self.handle_key_paused(key),
            GameState::GameOver => self.handle_key_game_over(key),
        }
    }

    fn handle_key_playing(&mut self, key: Key) {
        match key {
            Key::Up => self.queue_direction(Direction::Up),
            Key::Down => self.queue_direction(Direction::Down),
            Key::Left => self.queue_direction(Direction::Left),
            Key::Right => self.queue_direction(Direction::Right),
            // WASD alternative controls
            Key::W => self.queue_direction(Direction::Up),
            Key::S => self.queue_direction(Direction::Down),
            Key::A => self.queue_direction(Direction::Left),
            Key::D => self.queue_direction(Direction::Right),
            Key::P => self.state = GameState::Paused,
            Key::Escape => self.state = GameState::Paused,
            _ => {}
        }
    }

    fn handle_key_paused(&mut self, key: Key) {
        match key {
            Key::P | Key::Escape => self.state = GameState::Playing,
            _ => {}
        }
    }

    fn handle_key_game_over(&mut self, key: Key) {
        match key {
            Key::Enter | Key::Space => self.restart(),
            Key::Num1 => {
                self.difficulty = Difficulty::Easy;
                self.restart();
            }
            Key::Num2 => {
                self.difficulty = Difficulty::Medium;
                self.restart();
            }
            Key::Num3 => {
                self.difficulty = Difficulty::Hard;
                self.restart();
            }
            _ => {}
        }
    }

    /// Queue a direction change. Prevents reversals and limits queue depth.
    fn queue_direction(&mut self, new_dir: Direction) {
        if self.dir_queue.len() >= MAX_DIR_QUEUE {
            return;
        }
        // The effective current direction is the last queued one, or the actual direction.
        let effective = self.dir_queue.last().copied().unwrap_or(self.direction);
        if !new_dir.is_opposite(effective) && new_dir != effective {
            self.dir_queue.push(new_dir);
        }
    }

    // ── Game tick ───────────────────────────────────────────────────

    /// Process a tick event. Returns true if the snake moved this tick.
    fn handle_tick(&mut self, elapsed_ms: u64) -> bool {
        if self.state != GameState::Playing {
            return false;
        }

        self.pulse_counter = self.pulse_counter.wrapping_add(1);

        let level = speed_level(self.score);
        let interval = tick_interval_ms(self.difficulty, level) as u64;

        self.accumulated_ms += elapsed_ms;
        if self.accumulated_ms < interval {
            return false;
        }
        self.accumulated_ms -= interval;
        self.total_ticks += 1;
        self.ticks_since_food += 1;

        // Consume one queued direction.
        if let Some(new_dir) = self.dir_queue.first().copied() {
            self.dir_queue.remove(0);
            if !new_dir.is_opposite(self.direction) {
                self.direction = new_dir;
            }
        }

        // Move the snake.
        self.move_snake();

        // Update bonus food lifetime.
        if let Some(bonus) = &mut self.bonus_food {
            bonus.ticks_remaining = bonus.ticks_remaining.saturating_sub(1);
            if bonus.ticks_remaining == 0 {
                self.bonus_food = None;
            }
        }

        true
    }

    /// Move the snake one step in the current direction.
    fn move_snake(&mut self) {
        let head = self.snake[0];
        let mut new_head = head.moved(self.direction);

        if self.wrap_mode {
            new_head = new_head.wrapped();
        } else if !new_head.in_bounds() {
            self.game_over();
            return;
        }

        // Check self-collision (new head hits any body segment except the tail,
        // which will move away unless growing).
        // We check all segments because growth hasn't happened yet.
        if self.snake.contains(&new_head) {
            self.game_over();
            return;
        }

        // Insert new head.
        self.snake.insert(0, new_head);

        // Check if we ate food.
        let ate_normal = new_head == self.food.pos;
        let ate_bonus = self
            .bonus_food
            .as_ref()
            .is_some_and(|b| new_head == b.pos);

        if ate_normal {
            self.eat_normal_food();
        } else if ate_bonus {
            self.eat_bonus_food();
        } else {
            // No food eaten: remove tail to maintain length.
            self.snake.pop();
        }
    }

    /// Handle eating normal food.
    fn eat_normal_food(&mut self) {
        // Update streak.
        if self.ticks_since_food <= STREAK_WINDOW_TICKS {
            self.streak += 1;
        } else {
            self.streak = 1;
        }
        self.ticks_since_food = 0;

        let multiplier = if self.streak >= STREAK_THRESHOLD {
            STREAK_MULTIPLIER
        } else {
            1
        };

        self.score += NORMAL_FOOD_POINTS * multiplier;
        self.foods_eaten += 1;

        // Spawn new food.
        self.spawn_food();

        // Maybe spawn bonus food.
        let chance = self.rng.next_u64() % BONUS_SPAWN_CHANCE;
        if chance == 0 && self.bonus_food.is_none() {
            self.spawn_bonus_food();
        }
    }

    /// Handle eating bonus food.
    fn eat_bonus_food(&mut self) {
        let multiplier = if self.streak >= STREAK_THRESHOLD {
            STREAK_MULTIPLIER
        } else {
            1
        };

        self.score += BONUS_FOOD_POINTS * multiplier;
        self.bonus_eaten += 1;
        self.foods_eaten += 1;
        self.bonus_food = None;
        self.ticks_since_food = 0;
        // Eating bonus food counts as continuing the streak.
        self.streak += 1;
    }

    /// Transition to game over state, updating high score.
    fn game_over(&mut self) {
        self.state = GameState::GameOver;
        if self.score > self.high_score {
            self.high_score = self.score;
        }
    }

    // ── Queries ─────────────────────────────────────────────────────

    /// Current snake length.
    fn snake_length(&self) -> usize {
        self.snake.len()
    }

    /// Current speed level.
    fn current_speed_level(&self) -> u32 {
        speed_level(self.score)
    }

    /// Current tick interval in milliseconds.
    fn current_interval_ms(&self) -> u32 {
        tick_interval_ms(self.difficulty, self.current_speed_level())
    }

    /// Grid width in pixels.
    fn grid_width() -> f32 {
        GRID_COLS as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    /// Grid height in pixels.
    fn grid_height() -> f32 {
        GRID_ROWS as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    /// Total window width.
    fn window_width() -> f32 {
        PADDING * 3.0 + Self::grid_width() + STATS_PANEL_WIDTH
    }

    /// Total window height.
    fn window_height() -> f32 {
        PADDING * 2.0 + HEADER_HEIGHT + Self::grid_height()
    }

    /// Origin X of the grid area.
    fn grid_origin_x() -> f32 {
        PADDING
    }

    /// Origin Y of the grid area.
    fn grid_origin_y() -> f32 {
        PADDING + HEADER_HEIGHT
    }

    /// Origin X of the stats panel.
    fn stats_origin_x() -> f32 {
        PADDING * 2.0 + Self::grid_width()
    }

    /// Origin Y of the stats panel.
    fn stats_origin_y() -> f32 {
        PADDING + HEADER_HEIGHT
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Produce the full render command list for the current frame.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let win_w = Self::window_width();
        let win_h = Self::window_height();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_w,
            height: win_h,
            color: BASE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Header bar.
        self.render_header(&mut cmds);

        // Grid area background.
        cmds.push(RenderCommand::FillRect {
            x: Self::grid_origin_x() - 2.0,
            y: Self::grid_origin_y() - 2.0,
            width: Self::grid_width() + 4.0,
            height: Self::grid_height() + 4.0,
            color: CRUST,
            corner_radii: CornerRadii::all(4.0),
        });

        // Grid cells.
        self.render_grid(&mut cmds);

        // Snake.
        self.render_snake(&mut cmds);

        // Food.
        self.render_food(&mut cmds);

        // Bonus food.
        self.render_bonus_food(&mut cmds);

        // Grid lines.
        self.render_grid_lines(&mut cmds);

        // Stats panel.
        self.render_stats(&mut cmds);

        // Overlay for pause / game over.
        self.render_overlay(&mut cmds);

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let header_w = Self::window_width() - PADDING * 2.0;

        // Header background.
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: PADDING,
            width: header_w,
            height: HEADER_HEIGHT - 4.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: PADDING + 14.0,
            text: String::from("Snake"),
            color: GREEN,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score display.
        let score_text = format!("Score: {}", self.score);
        cmds.push(RenderCommand::Text {
            x: PADDING + 80.0,
            y: PADDING + 14.0,
            text: score_text,
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // High score.
        let high_text = format!("Hi: {}", self.high_score);
        cmds.push(RenderCommand::Text {
            x: PADDING + 200.0,
            y: PADDING + 14.0,
            text: high_text,
            color: YELLOW,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Difficulty / mode indicators.
        let mode_text = format!(
            "{} | {}",
            self.difficulty.label(),
            if self.wrap_mode { "Wrap" } else { "Wall" }
        );
        cmds.push(RenderCommand::Text {
            x: header_w - 80.0,
            y: PADDING + 14.0,
            text: mode_text,
            color: SUBTEXT0,
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();

        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let x = ox + col as f32 * (CELL_SIZE + CELL_GAP);
                let y = oy + row as f32 * (CELL_SIZE + CELL_GAP);

                // Subtle checkerboard pattern.
                let color = if (row + col) % 2 == 0 {
                    SURFACE0
                } else {
                    Color::from_hex(0x2A2A3C)
                };

                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color,
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                });
            }
        }
    }

    fn render_snake(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let len = self.snake.len();

        for (i, pos) in self.snake.iter().enumerate() {
            let x = ox + pos.col as f32 * (CELL_SIZE + CELL_GAP);
            let y = oy + pos.row as f32 * (CELL_SIZE + CELL_GAP);

            // Gradient from bright green (head) to darker green (tail).
            let t = if len > 1 {
                i as f32 / (len - 1) as f32
            } else {
                0.0
            };
            let color = GREEN.lerp(Color::from_hex(0x40B080), t);

            let corner = if i == 0 {
                CornerRadii::all(6.0)
            } else {
                CornerRadii::all(CELL_CORNER_RADIUS)
            };

            cmds.push(RenderCommand::FillRect {
                x: x + 1.0,
                y: y + 1.0,
                width: CELL_SIZE - 2.0,
                height: CELL_SIZE - 2.0,
                color,
                corner_radii: corner,
            });

            // Draw eyes on the head.
            if i == 0 {
                self.render_head_eyes(cmds, x, y);
            }
        }
    }

    fn render_head_eyes(&self, cmds: &mut Vec<RenderCommand>, head_x: f32, head_y: f32) {
        let (eye1_dx, eye1_dy, eye2_dx, eye2_dy) = match self.direction {
            Direction::Up => (5.0, 5.0, 15.0, 5.0),
            Direction::Down => (5.0, 15.0, 15.0, 15.0),
            Direction::Left => (5.0, 5.0, 5.0, 15.0),
            Direction::Right => (15.0, 5.0, 15.0, 15.0),
        };

        let eye_size = 4.0;
        let eye_color = CRUST;

        cmds.push(RenderCommand::FillRect {
            x: head_x + eye1_dx,
            y: head_y + eye1_dy,
            width: eye_size,
            height: eye_size,
            color: eye_color,
            corner_radii: CornerRadii::all(2.0),
        });

        cmds.push(RenderCommand::FillRect {
            x: head_x + eye2_dx,
            y: head_y + eye2_dy,
            width: eye_size,
            height: eye_size,
            color: eye_color,
            corner_radii: CornerRadii::all(2.0),
        });
    }

    fn render_food(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let pos = self.food.pos;

        let x = ox + pos.col as f32 * (CELL_SIZE + CELL_GAP);
        let y = oy + pos.row as f32 * (CELL_SIZE + CELL_GAP);

        // Pulsing effect: oscillate size slightly.
        let pulse = ((self.pulse_counter % 20) as f32 / 20.0 * std::f32::consts::PI * 2.0).sin();
        let inset = 3.0 - pulse * 1.5;

        cmds.push(RenderCommand::FillRect {
            x: x + inset,
            y: y + inset,
            width: CELL_SIZE - inset * 2.0,
            height: CELL_SIZE - inset * 2.0,
            color: RED,
            corner_radii: CornerRadii::all(CELL_SIZE / 2.0),
        });

        // Small highlight to make it look like an apple.
        cmds.push(RenderCommand::FillRect {
            x: x + inset + 3.0,
            y: y + inset + 2.0,
            width: 4.0,
            height: 4.0,
            color: PEACH,
            corner_radii: CornerRadii::all(2.0),
        });
    }

    fn render_bonus_food(&self, cmds: &mut Vec<RenderCommand>) {
        let bonus = match &self.bonus_food {
            Some(b) => b,
            None => return,
        };

        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let pos = bonus.pos;

        let x = ox + pos.col as f32 * (CELL_SIZE + CELL_GAP);
        let y = oy + pos.row as f32 * (CELL_SIZE + CELL_GAP);

        // Faster pulsing for bonus food, also fades as it nears expiry.
        let pulse =
            ((self.pulse_counter % 10) as f32 / 10.0 * std::f32::consts::PI * 2.0).sin();
        let inset = 2.0 - pulse * 1.5;

        // Flicker when close to expiring.
        let visible = if bonus.ticks_remaining <= 5 {
            self.pulse_counter % 4 < 3
        } else {
            true
        };

        if visible {
            cmds.push(RenderCommand::FillRect {
                x: x + inset,
                y: y + inset,
                width: CELL_SIZE - inset * 2.0,
                height: CELL_SIZE - inset * 2.0,
                color: MAUVE,
                corner_radii: CornerRadii::all(4.0),
            });

            // Star-like shape: draw a smaller bright square inside.
            cmds.push(RenderCommand::FillRect {
                x: x + 7.0,
                y: y + 7.0,
                width: 10.0,
                height: 10.0,
                color: LAVENDER,
                corner_radii: CornerRadii::all(2.0),
            });
        }
    }

    fn render_grid_lines(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let gw = Self::grid_width();
        let gh = Self::grid_height();

        let line_color = Color::rgba(100, 100, 140, 30);

        // Vertical lines.
        for col in 1..GRID_COLS {
            let x = ox + col as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP / 2.0;
            cmds.push(RenderCommand::Line {
                x1: x,
                y1: oy,
                x2: x,
                y2: oy + gh,
                color: line_color,
                width: 0.5,
            });
        }

        // Horizontal lines.
        for row in 1..GRID_ROWS {
            let y = oy + row as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP / 2.0;
            cmds.push(RenderCommand::Line {
                x1: ox,
                y1: y,
                x2: ox + gw,
                y2: y,
                color: line_color,
                width: 0.5,
            });
        }
    }

    fn render_stats(&self, cmds: &mut Vec<RenderCommand>) {
        let sx = Self::stats_origin_x();
        let sy = Self::stats_origin_y();

        // Stats panel background.
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: STATS_PANEL_WIDTH,
            height: Self::grid_height(),
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        let mut y_offset = sy + 12.0;
        let line_height = 22.0;

        // Title.
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: y_offset,
            text: String::from("Statistics"),
            color: BLUE,
            font_size: STATS_FONT_SIZE + 2.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y_offset += line_height + 4.0;

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: sx + 10.0,
            y1: y_offset,
            x2: sx + STATS_PANEL_WIDTH - 10.0,
            y2: y_offset,
            color: SURFACE1,
            width: 1.0,
        });
        y_offset += 8.0;

        // Stats rows.
        let stats: Vec<(&str, String, Color)> = vec![
            ("Score", format!("{}", self.score), TEXT_COLOR),
            ("High Score", format!("{}", self.high_score), YELLOW),
            ("Length", format!("{}", self.snake_length()), GREEN),
            (
                "Speed",
                format!("Lv.{}", self.current_speed_level()),
                PEACH,
            ),
            ("Foods Eaten", format!("{}", self.foods_eaten), RED),
            ("Bonus Eaten", format!("{}", self.bonus_eaten), MAUVE),
            ("Streak", format!("x{}", self.streak), TEAL),
            (
                "Interval",
                format!("{}ms", self.current_interval_ms()),
                SUBTEXT0,
            ),
        ];

        for (label, value, val_color) in &stats {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_offset,
                text: String::from(*label),
                color: SUBTEXT0,
                font_size: STATS_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: sx + STATS_PANEL_WIDTH - 60.0,
                y: y_offset,
                text: value.clone(),
                color: *val_color,
                font_size: STATS_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            y_offset += line_height;
        }

        // Controls section.
        y_offset += 12.0;
        cmds.push(RenderCommand::Line {
            x1: sx + 10.0,
            y1: y_offset,
            x2: sx + STATS_PANEL_WIDTH - 10.0,
            y2: y_offset,
            color: SURFACE1,
            width: 1.0,
        });
        y_offset += 8.0;

        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: y_offset,
            text: String::from("Controls"),
            color: BLUE,
            font_size: STATS_FONT_SIZE + 2.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y_offset += line_height;

        let controls = [
            "Arrows / WASD: Move",
            "P / Esc: Pause",
            "W (paused): Wrap mode",
            "1/2/3: Difficulty",
            "Enter: Restart",
        ];

        for line in &controls {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_offset,
                text: String::from(*line),
                color: OVERLAY0,
                font_size: STATS_FONT_SIZE - 1.0,
                font_weight: FontWeightHint::Light,
                max_width: Some(STATS_PANEL_WIDTH - 20.0),
            });
            y_offset += line_height - 4.0;
        }
    }

    fn render_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        match self.state {
            GameState::Paused => self.render_pause_overlay(cmds),
            GameState::GameOver => self.render_game_over_overlay(cmds),
            GameState::Playing => {}
        }
    }

    fn render_pause_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let gw = Self::grid_width();
        let gh = Self::grid_height();
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();

        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: ox,
            y: oy,
            width: gw,
            height: gh,
            color: Color::rgba(17, 17, 27, 180),
            corner_radii: CornerRadii::ZERO,
        });

        // Pause text.
        cmds.push(RenderCommand::Text {
            x: ox + gw / 2.0 - 50.0,
            y: oy + gh / 2.0 - 20.0,
            text: String::from("PAUSED"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: ox + gw / 2.0 - 80.0,
            y: oy + gh / 2.0 + 10.0,
            text: String::from("Press P or Esc to resume"),
            color: SUBTEXT0,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Wrap mode toggle hint.
        let wrap_text = format!(
            "Wrap mode: {} (W to toggle)",
            if self.wrap_mode { "ON" } else { "OFF" }
        );
        cmds.push(RenderCommand::Text {
            x: ox + gw / 2.0 - 100.0,
            y: oy + gh / 2.0 + 35.0,
            text: wrap_text,
            color: TEAL,
            font_size: OVERLAY_FONT_SIZE - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_game_over_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let gw = Self::grid_width();
        let gh = Self::grid_height();
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();

        // Dark overlay.
        cmds.push(RenderCommand::FillRect {
            x: ox,
            y: oy,
            width: gw,
            height: gh,
            color: Color::rgba(17, 17, 27, 200),
            corner_radii: CornerRadii::ZERO,
        });

        // Game over box.
        let box_w = 260.0;
        let box_h = 160.0;
        let box_x = ox + (gw - box_w) / 2.0;
        let box_y = oy + (gh - box_h) / 2.0;

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
            color: RED,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Game Over title.
        cmds.push(RenderCommand::Text {
            x: box_x + 60.0,
            y: box_y + 20.0,
            text: String::from("GAME OVER"),
            color: RED,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score.
        let score_text = format!("Score: {}", self.score);
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 55.0,
            text: score_text,
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // High score.
        let high_text = format!("High Score: {}", self.high_score);
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 78.0,
            text: high_text,
            color: YELLOW,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Length.
        let len_text = format!("Length: {}", self.snake_length());
        cmds.push(RenderCommand::Text {
            x: box_x + 150.0,
            y: box_y + 55.0,
            text: len_text,
            color: GREEN,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Restart hint.
        cmds.push(RenderCommand::Text {
            x: box_x + 30.0,
            y: box_y + 110.0,
            text: String::from("Press Enter to restart"),
            color: SUBTEXT0,
            font_size: OVERLAY_FONT_SIZE - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 30.0,
            y: box_y + 130.0,
            text: String::from("1/2/3: Easy/Medium/Hard"),
            color: OVERLAY0,
            font_size: OVERLAY_FONT_SIZE - 2.0,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });
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
}

fn main() {
    let _app = SnakeApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a game with a fixed seed for deterministic tests.
    fn test_app() -> SnakeApp {
        SnakeApp::with_seed(12345)
    }

    /// Helper to force a tick that causes the snake to move.
    fn force_tick(app: &mut SnakeApp) {
        // Use a large elapsed_ms to guarantee a move happens.
        app.handle_tick(1000);
    }

    // ── Construction & initialization ───────────────────────────────

    #[test]
    fn test_initial_snake_length() {
        let app = test_app();
        assert_eq!(app.snake_length(), 3);
    }

    #[test]
    fn test_initial_snake_position() {
        let app = test_app();
        assert_eq!(app.snake[0], Pos::new(10, 10));
        assert_eq!(app.snake[1], Pos::new(10, 9));
        assert_eq!(app.snake[2], Pos::new(10, 8));
    }

    #[test]
    fn test_initial_direction_is_right() {
        let app = test_app();
        assert_eq!(app.direction, Direction::Right);
    }

    #[test]
    fn test_initial_state_is_playing() {
        let app = test_app();
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_initial_score_is_zero() {
        let app = test_app();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_initial_foods_eaten_is_zero() {
        let app = test_app();
        assert_eq!(app.foods_eaten, 0);
    }

    #[test]
    fn test_initial_food_exists() {
        let app = test_app();
        assert!(app.food.pos.in_bounds());
    }

    #[test]
    fn test_initial_food_not_on_snake() {
        let app = test_app();
        assert!(!app.snake.contains(&app.food.pos));
    }

    #[test]
    fn test_initial_no_bonus_food() {
        let app = test_app();
        assert!(app.bonus_food.is_none());
    }

    #[test]
    fn test_default_difficulty_is_medium() {
        let app = test_app();
        assert_eq!(app.difficulty, Difficulty::Medium);
    }

    #[test]
    fn test_default_wrap_mode_off() {
        let app = test_app();
        assert!(!app.wrap_mode);
    }

    // ── Direction ───────────────────────────────────────────────────

    #[test]
    fn test_direction_opposite_up_down() {
        assert!(Direction::Up.is_opposite(Direction::Down));
        assert!(Direction::Down.is_opposite(Direction::Up));
    }

    #[test]
    fn test_direction_opposite_left_right() {
        assert!(Direction::Left.is_opposite(Direction::Right));
        assert!(Direction::Right.is_opposite(Direction::Left));
    }

    #[test]
    fn test_direction_not_opposite_same() {
        assert!(!Direction::Up.is_opposite(Direction::Up));
        assert!(!Direction::Left.is_opposite(Direction::Left));
    }

    #[test]
    fn test_direction_not_opposite_perpendicular() {
        assert!(!Direction::Up.is_opposite(Direction::Left));
        assert!(!Direction::Down.is_opposite(Direction::Right));
    }

    #[test]
    fn test_direction_delta_up() {
        assert_eq!(Direction::Up.delta(), (-1, 0));
    }

    #[test]
    fn test_direction_delta_down() {
        assert_eq!(Direction::Down.delta(), (1, 0));
    }

    #[test]
    fn test_direction_delta_left() {
        assert_eq!(Direction::Left.delta(), (0, -1));
    }

    #[test]
    fn test_direction_delta_right() {
        assert_eq!(Direction::Right.delta(), (0, 1));
    }

    // ── Pos ─────────────────────────────────────────────────────────

    #[test]
    fn test_pos_in_bounds() {
        assert!(Pos::new(0, 0).in_bounds());
        assert!(Pos::new(19, 19).in_bounds());
        assert!(Pos::new(10, 10).in_bounds());
    }

    #[test]
    fn test_pos_out_of_bounds() {
        assert!(!Pos::new(-1, 0).in_bounds());
        assert!(!Pos::new(0, -1).in_bounds());
        assert!(!Pos::new(20, 0).in_bounds());
        assert!(!Pos::new(0, 20).in_bounds());
    }

    #[test]
    fn test_pos_moved() {
        let p = Pos::new(5, 5);
        assert_eq!(p.moved(Direction::Up), Pos::new(4, 5));
        assert_eq!(p.moved(Direction::Down), Pos::new(6, 5));
        assert_eq!(p.moved(Direction::Left), Pos::new(5, 4));
        assert_eq!(p.moved(Direction::Right), Pos::new(5, 6));
    }

    #[test]
    fn test_pos_wrapped_positive() {
        let p = Pos::new(20, 20);
        let w = p.wrapped();
        assert_eq!(w, Pos::new(0, 0));
    }

    #[test]
    fn test_pos_wrapped_negative() {
        let p = Pos::new(-1, -1);
        let w = p.wrapped();
        assert_eq!(w, Pos::new(19, 19));
    }

    #[test]
    fn test_pos_wrapped_in_bounds_unchanged() {
        let p = Pos::new(10, 10);
        assert_eq!(p.wrapped(), p);
    }

    // ── Snake movement ──────────────────────────────────────────────

    #[test]
    fn test_snake_moves_right() {
        let mut app = test_app();
        let old_head = app.snake[0];
        force_tick(&mut app);
        assert_eq!(app.snake[0], Pos::new(old_head.row, old_head.col + 1));
    }

    #[test]
    fn test_snake_length_preserved_without_food() {
        let mut app = test_app();
        // Move food far away so snake doesn't eat it.
        app.food.pos = Pos::new(0, 0);
        let original_len = app.snake_length();
        force_tick(&mut app);
        assert_eq!(app.snake_length(), original_len);
    }

    #[test]
    fn test_snake_moves_up() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0); // Move food away.
        app.direction = Direction::Up;
        let head = app.snake[0];
        force_tick(&mut app);
        assert_eq!(app.snake[0], Pos::new(head.row - 1, head.col));
    }

    #[test]
    fn test_snake_moves_down() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        app.direction = Direction::Down;
        let head = app.snake[0];
        force_tick(&mut app);
        assert_eq!(app.snake[0], Pos::new(head.row + 1, head.col));
    }

    #[test]
    fn test_snake_moves_left() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        // Turn up first (can't reverse to left from right).
        app.direction = Direction::Up;
        force_tick(&mut app);
        app.direction = Direction::Left;
        let head = app.snake[0];
        force_tick(&mut app);
        assert_eq!(app.snake[0], Pos::new(head.row, head.col - 1));
    }

    // ── Direction change & reversal prevention ──────────────────────

    #[test]
    fn test_cannot_reverse_direction() {
        let mut app = test_app();
        // Initially moving right, try to go left.
        app.queue_direction(Direction::Left);
        assert!(app.dir_queue.is_empty());
    }

    #[test]
    fn test_can_change_to_perpendicular() {
        let mut app = test_app();
        app.queue_direction(Direction::Up);
        assert_eq!(app.dir_queue.len(), 1);
        assert_eq!(app.dir_queue[0], Direction::Up);
    }

    #[test]
    fn test_queue_rejects_same_direction() {
        let mut app = test_app();
        // Moving right, queueing right again should be rejected.
        app.queue_direction(Direction::Right);
        assert!(app.dir_queue.is_empty());
    }

    #[test]
    fn test_direction_queue_max_depth() {
        let mut app = test_app();
        app.queue_direction(Direction::Up);
        app.queue_direction(Direction::Left);
        // Queue is full (MAX_DIR_QUEUE = 2), this should be rejected.
        app.queue_direction(Direction::Down);
        assert_eq!(app.dir_queue.len(), MAX_DIR_QUEUE);
    }

    #[test]
    fn test_direction_queue_consumed_on_tick() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        app.queue_direction(Direction::Up);
        assert_eq!(app.dir_queue.len(), 1);
        force_tick(&mut app);
        assert_eq!(app.direction, Direction::Up);
        assert!(app.dir_queue.is_empty());
    }

    #[test]
    fn test_direction_queue_prevents_reversal_via_chain() {
        let mut app = test_app();
        // Moving right. Queue up then left = valid chain (not a reversal).
        app.queue_direction(Direction::Up);
        app.queue_direction(Direction::Left);
        assert_eq!(app.dir_queue.len(), 2);
    }

    #[test]
    fn test_direction_queue_reversal_via_chain_blocked() {
        let mut app = test_app();
        // Moving right. Queue up, then try right again (opposite of up is down, not right).
        // Up is queued. Then right: last queued is Up, opposite of Up is Down, so Right is ok.
        app.queue_direction(Direction::Up);
        app.queue_direction(Direction::Right);
        assert_eq!(app.dir_queue.len(), 2);
    }

    // ── Collision detection ─────────────────────────────────────────

    #[test]
    fn test_wall_collision_right() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        // Place snake near right wall.
        app.snake = vec![Pos::new(10, 19), Pos::new(10, 18), Pos::new(10, 17)];
        app.direction = Direction::Right;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_wall_collision_left() {
        let mut app = test_app();
        app.food.pos = Pos::new(19, 19);
        app.snake = vec![Pos::new(10, 0), Pos::new(10, 1), Pos::new(10, 2)];
        app.direction = Direction::Left;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_wall_collision_top() {
        let mut app = test_app();
        app.food.pos = Pos::new(19, 19);
        app.snake = vec![Pos::new(0, 10), Pos::new(1, 10), Pos::new(2, 10)];
        app.direction = Direction::Up;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_wall_collision_bottom() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        app.snake = vec![Pos::new(19, 10), Pos::new(18, 10), Pos::new(17, 10)];
        app.direction = Direction::Down;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_self_collision() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        // Create a snake that will hit itself: a U-shape.
        app.snake = vec![
            Pos::new(5, 6),
            Pos::new(5, 7),
            Pos::new(6, 7),
            Pos::new(6, 6),
            Pos::new(6, 5),
        ];
        app.direction = Direction::Down; // Head moves to (6,6), which is occupied.
        force_tick(&mut app);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_no_collision_normal_move() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        force_tick(&mut app);
        assert_eq!(app.state, GameState::Playing);
    }

    // ── Wrap mode ───────────────────────────────────────────────────

    #[test]
    fn test_wrap_mode_right_wall() {
        let mut app = test_app();
        app.wrap_mode = true;
        app.food.pos = Pos::new(0, 0);
        app.snake = vec![Pos::new(10, 19), Pos::new(10, 18), Pos::new(10, 17)];
        app.direction = Direction::Right;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.snake[0], Pos::new(10, 0));
    }

    #[test]
    fn test_wrap_mode_left_wall() {
        let mut app = test_app();
        app.wrap_mode = true;
        app.food.pos = Pos::new(19, 19);
        app.snake = vec![Pos::new(10, 0), Pos::new(10, 1), Pos::new(10, 2)];
        app.direction = Direction::Left;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.snake[0], Pos::new(10, 19));
    }

    #[test]
    fn test_wrap_mode_top_wall() {
        let mut app = test_app();
        app.wrap_mode = true;
        app.food.pos = Pos::new(19, 19);
        app.snake = vec![Pos::new(0, 10), Pos::new(1, 10), Pos::new(2, 10)];
        app.direction = Direction::Up;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.snake[0], Pos::new(19, 10));
    }

    #[test]
    fn test_wrap_mode_bottom_wall() {
        let mut app = test_app();
        app.wrap_mode = true;
        app.food.pos = Pos::new(0, 0);
        app.snake = vec![Pos::new(19, 10), Pos::new(18, 10), Pos::new(17, 10)];
        app.direction = Direction::Down;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.snake[0], Pos::new(0, 10));
    }

    // ── Food eating & growth ────────────────────────────────────────

    #[test]
    fn test_eating_food_grows_snake() {
        let mut app = test_app();
        // Place food directly in front of the snake head.
        let head = app.snake[0];
        app.food.pos = Pos::new(head.row, head.col + 1);
        let len_before = app.snake_length();
        force_tick(&mut app);
        assert_eq!(app.snake_length(), len_before + 1);
    }

    #[test]
    fn test_eating_food_increases_score() {
        let mut app = test_app();
        let head = app.snake[0];
        app.food.pos = Pos::new(head.row, head.col + 1);
        force_tick(&mut app);
        assert!(app.score >= NORMAL_FOOD_POINTS);
    }

    #[test]
    fn test_eating_food_increments_foods_eaten() {
        let mut app = test_app();
        let head = app.snake[0];
        app.food.pos = Pos::new(head.row, head.col + 1);
        force_tick(&mut app);
        assert_eq!(app.foods_eaten, 1);
    }

    #[test]
    fn test_new_food_spawns_after_eating() {
        let mut app = test_app();
        let head = app.snake[0];
        let old_food_pos = Pos::new(head.row, head.col + 1);
        app.food.pos = old_food_pos;
        force_tick(&mut app);
        // New food should be at a different position (with high probability).
        // At minimum, it should be in bounds.
        assert!(app.food.pos.in_bounds());
    }

    #[test]
    fn test_food_not_on_snake_after_spawn() {
        let mut app = test_app();
        let head = app.snake[0];
        app.food.pos = Pos::new(head.row, head.col + 1);
        force_tick(&mut app);
        assert!(!app.snake.contains(&app.food.pos));
    }

    // ── Bonus food ──────────────────────────────────────────────────

    #[test]
    fn test_bonus_food_lifetime_decreases() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        app.bonus_food = Some(Food {
            pos: Pos::new(19, 19),
            kind: FoodKind::Bonus,
            ticks_remaining: 10,
        });
        force_tick(&mut app);
        if let Some(bf) = &app.bonus_food {
            assert_eq!(bf.ticks_remaining, 9);
        }
    }

    #[test]
    fn test_bonus_food_disappears_at_zero() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        app.bonus_food = Some(Food {
            pos: Pos::new(19, 19),
            kind: FoodKind::Bonus,
            ticks_remaining: 1,
        });
        force_tick(&mut app);
        assert!(app.bonus_food.is_none());
    }

    #[test]
    fn test_eating_bonus_food_gives_points() {
        let mut app = test_app();
        let head = app.snake[0];
        app.food.pos = Pos::new(0, 0); // Normal food far away.
        let bonus_pos = Pos::new(head.row, head.col + 1);
        app.bonus_food = Some(Food {
            pos: bonus_pos,
            kind: FoodKind::Bonus,
            ticks_remaining: 10,
        });
        force_tick(&mut app);
        assert!(app.score >= BONUS_FOOD_POINTS);
    }

    #[test]
    fn test_eating_bonus_food_clears_it() {
        let mut app = test_app();
        let head = app.snake[0];
        app.food.pos = Pos::new(0, 0);
        let bonus_pos = Pos::new(head.row, head.col + 1);
        app.bonus_food = Some(Food {
            pos: bonus_pos,
            kind: FoodKind::Bonus,
            ticks_remaining: 10,
        });
        force_tick(&mut app);
        assert!(app.bonus_food.is_none());
    }

    #[test]
    fn test_eating_bonus_food_increments_bonus_eaten() {
        let mut app = test_app();
        let head = app.snake[0];
        app.food.pos = Pos::new(0, 0);
        app.bonus_food = Some(Food {
            pos: Pos::new(head.row, head.col + 1),
            kind: FoodKind::Bonus,
            ticks_remaining: 10,
        });
        force_tick(&mut app);
        assert_eq!(app.bonus_eaten, 1);
    }

    // ── Scoring & streaks ───────────────────────────────────────────

    #[test]
    fn test_streak_increases_on_consecutive_food() {
        let mut app = test_app();
        app.ticks_since_food = 0;
        // Simulate eating: call eat_normal_food directly.
        app.snake.push(Pos::new(19, 19)); // Add dummy tail so no underflow.
        app.eat_normal_food();
        assert_eq!(app.streak, 1);
        app.ticks_since_food = 5; // Still within window.
        app.eat_normal_food();
        assert_eq!(app.streak, 2);
    }

    #[test]
    fn test_streak_resets_on_gap() {
        let mut app = test_app();
        app.streak = 5;
        app.ticks_since_food = STREAK_WINDOW_TICKS + 1;
        app.eat_normal_food();
        assert_eq!(app.streak, 1);
    }

    #[test]
    fn test_streak_multiplier_applied() {
        let mut app = test_app();
        app.streak = STREAK_THRESHOLD - 1;
        app.ticks_since_food = 1; // Within window.
        app.eat_normal_food();
        // Streak should now be >= STREAK_THRESHOLD, and score includes multiplier.
        assert_eq!(app.score, NORMAL_FOOD_POINTS * STREAK_MULTIPLIER);
    }

    #[test]
    fn test_no_multiplier_below_threshold() {
        let mut app = test_app();
        app.streak = 0;
        app.ticks_since_food = STREAK_WINDOW_TICKS + 1;
        app.eat_normal_food();
        assert_eq!(app.score, NORMAL_FOOD_POINTS);
    }

    // ── Speed levels ────────────────────────────────────────────────

    #[test]
    fn test_speed_level_at_zero() {
        assert_eq!(speed_level(0), 1);
    }

    #[test]
    fn test_speed_level_increases_with_score() {
        assert_eq!(speed_level(50), 2);
        assert_eq!(speed_level(100), 3);
        assert_eq!(speed_level(200), 5);
    }

    #[test]
    fn test_speed_level_caps_at_10() {
        assert_eq!(speed_level(500), 10);
        assert_eq!(speed_level(1000), 10);
    }

    #[test]
    fn test_tick_interval_decreases_with_level() {
        let i1 = tick_interval_ms(Difficulty::Medium, 1);
        let i5 = tick_interval_ms(Difficulty::Medium, 5);
        let i10 = tick_interval_ms(Difficulty::Medium, 10);
        assert!(i1 > i5);
        assert!(i5 > i10);
    }

    #[test]
    fn test_tick_interval_minimum() {
        // At maximum level, interval shouldn't go below 50ms.
        let interval = tick_interval_ms(Difficulty::Hard, 10);
        assert!(interval >= 50);
    }

    #[test]
    fn test_easy_slower_than_hard() {
        let easy = tick_interval_ms(Difficulty::Easy, 1);
        let hard = tick_interval_ms(Difficulty::Hard, 1);
        assert!(easy > hard);
    }

    // ── Pause ───────────────────────────────────────────────────────

    #[test]
    fn test_pause_key() {
        let mut app = test_app();
        app.handle_key(Key::P, true);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_unpause_key() {
        let mut app = test_app();
        app.state = GameState::Paused;
        app.handle_key(Key::P, true);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_escape_pauses() {
        let mut app = test_app();
        app.handle_key(Key::Escape, true);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_escape_unpauses() {
        let mut app = test_app();
        app.state = GameState::Paused;
        app.handle_key(Key::Escape, true);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_no_movement_while_paused() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        app.state = GameState::Paused;
        let head = app.snake[0];
        force_tick(&mut app);
        assert_eq!(app.snake[0], head);
    }

    // ── Game over & restart ─────────────────────────────────────────

    #[test]
    fn test_game_over_updates_high_score() {
        let mut app = test_app();
        app.score = 100;
        app.game_over();
        assert_eq!(app.high_score, 100);
    }

    #[test]
    fn test_high_score_only_increases() {
        let mut app = test_app();
        app.score = 100;
        app.game_over();
        app.score = 50;
        app.game_over();
        assert_eq!(app.high_score, 100);
    }

    #[test]
    fn test_restart_resets_score() {
        let mut app = test_app();
        app.score = 100;
        app.game_over();
        app.restart();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_restart_preserves_high_score() {
        let mut app = test_app();
        app.score = 100;
        app.game_over();
        app.restart();
        assert_eq!(app.high_score, 100);
    }

    #[test]
    fn test_restart_resets_snake_length() {
        let mut app = test_app();
        app.score = 100;
        app.restart();
        assert_eq!(app.snake_length(), 3);
    }

    #[test]
    fn test_restart_resets_state_to_playing() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        app.restart();
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_enter_restarts_on_game_over() {
        let mut app = test_app();
        app.score = 50;
        app.game_over();
        app.handle_key(Key::Enter, true);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_restart_preserves_difficulty() {
        let mut app = test_app();
        app.difficulty = Difficulty::Hard;
        app.restart();
        assert_eq!(app.difficulty, Difficulty::Hard);
    }

    #[test]
    fn test_restart_preserves_wrap_mode() {
        let mut app = test_app();
        app.wrap_mode = true;
        app.restart();
        assert!(app.wrap_mode);
    }

    #[test]
    fn test_difficulty_change_on_game_over() {
        let mut app = test_app();
        app.game_over();
        app.handle_key(Key::Num1, true);
        assert_eq!(app.difficulty, Difficulty::Easy);
        assert_eq!(app.state, GameState::Playing);
    }

    // ── Rendering output ────────────────────────────────────────────

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
            RenderCommand::FillRect {
                x,
                y,
                color,
                ..
            } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*color, BASE);
            }
            _ => panic!("first command should be FillRect background"),
        }
    }

    #[test]
    fn test_render_paused_has_overlay() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let cmds = app.render();
        // Should contain text "PAUSED".
        let has_pause_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "PAUSED"));
        assert!(has_pause_text);
    }

    #[test]
    fn test_render_game_over_has_overlay() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let cmds = app.render();
        let has_game_over_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "GAME OVER"));
        assert!(has_game_over_text);
    }

    #[test]
    fn test_render_contains_score_text() {
        let app = test_app();
        let cmds = app.render();
        let has_score = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Score")));
        assert!(has_score);
    }

    #[test]
    fn test_render_contains_statistics() {
        let app = test_app();
        let cmds = app.render();
        let has_stats = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Statistics"));
        assert!(has_stats);
    }

    #[test]
    fn test_render_snake_cells() {
        let app = test_app();
        let cmds = app.render();
        // Count FillRect commands that could be snake segments (green-ish).
        // At minimum we should have at least 3 for the snake body.
        let green_rects = cmds.iter().filter(|c| {
            matches!(c, RenderCommand::FillRect { color, .. } if color.g > 150 && color.r < 200 && color.b < 200)
        }).count();
        // 3 snake segments + 2 eyes = at minimum some green rects.
        assert!(green_rects >= 3, "expected at least 3 snake-colored rects, got {green_rects}");
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
        // Very unlikely to be equal.
        assert_ne!(rng1.next_u64(), rng2.next_u64());
    }

    #[test]
    fn test_lcg_bounded() {
        let mut rng = Lcg::new(42);
        for _ in 0..100 {
            let val = rng.next_bounded(20);
            assert!(val < 20);
        }
    }

    // ── Tick timing ─────────────────────────────────────────────────

    #[test]
    fn test_tick_accumulation() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        // Small elapsed: shouldn't trigger a move.
        let moved = app.handle_tick(10);
        assert!(!moved);
        // Now push enough time to trigger.
        let moved = app.handle_tick(500);
        assert!(moved);
    }

    #[test]
    fn test_tick_does_nothing_when_game_over() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let moved = app.handle_tick(1000);
        assert!(!moved);
    }

    // ── Difficulty settings ─────────────────────────────────────────

    #[test]
    fn test_difficulty_labels() {
        assert_eq!(Difficulty::Easy.label(), "Easy");
        assert_eq!(Difficulty::Medium.label(), "Medium");
        assert_eq!(Difficulty::Hard.label(), "Hard");
    }

    #[test]
    fn test_difficulty_intervals_ordered() {
        assert!(Difficulty::Easy.base_interval_ms() > Difficulty::Medium.base_interval_ms());
        assert!(Difficulty::Medium.base_interval_ms() > Difficulty::Hard.base_interval_ms());
    }

    // ── Event handling ──────────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = test_app();
        let event = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::default(),
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.dir_queue.len(), 1);
    }

    #[test]
    fn test_handle_event_tick() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        let event = Event::Tick { elapsed_ms: 1000 };
        app.handle_event(&event);
        // Snake should have moved.
        assert!(app.total_ticks > 0);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = test_app();
        app.handle_key(Key::Up, false);
        assert!(app.dir_queue.is_empty());
    }

    #[test]
    fn test_wasd_controls() {
        let mut app = test_app();
        app.handle_key(Key::W, true);
        assert_eq!(app.dir_queue.len(), 1);
        assert_eq!(app.dir_queue[0], Direction::Up);
    }

    // ── Window dimensions ───────────────────────────────────────────

    #[test]
    fn test_window_dimensions_positive() {
        assert!(SnakeApp::window_width() > 0.0);
        assert!(SnakeApp::window_height() > 0.0);
    }

    #[test]
    fn test_grid_dimensions_match_constants() {
        let gw = SnakeApp::grid_width();
        let gh = SnakeApp::grid_height();
        let expected_w = GRID_COLS as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        let expected_h = GRID_ROWS as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        assert!((gw - expected_w).abs() < 0.01);
        assert!((gh - expected_h).abs() < 0.01);
    }

    // ── Wrap mode toggle hint ───────────────────────────────────────

    #[test]
    fn test_wrap_mode_self_collision_still_kills() {
        let mut app = test_app();
        app.wrap_mode = true;
        app.food.pos = Pos::new(0, 0);
        // U-shape that causes self-collision.
        app.snake = vec![
            Pos::new(5, 6),
            Pos::new(5, 7),
            Pos::new(6, 7),
            Pos::new(6, 6),
            Pos::new(6, 5),
        ];
        app.direction = Direction::Down;
        force_tick(&mut app);
        assert_eq!(app.state, GameState::GameOver);
    }

    // ── Multiple moves ──────────────────────────────────────────────

    #[test]
    fn test_snake_multiple_moves_no_crash() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        // Move the snake 5 times without crashing.
        for _ in 0..5 {
            force_tick(&mut app);
        }
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_snake_tail_follows_head() {
        let mut app = test_app();
        app.food.pos = Pos::new(0, 0);
        force_tick(&mut app);
        // After one move right, the second segment should be where the head was.
        assert_eq!(app.snake[1], Pos::new(10, 10));
    }
}
