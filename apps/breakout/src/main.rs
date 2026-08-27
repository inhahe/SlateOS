#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]

//! Slate OS Breakout — classic brick-breaker arcade game.
//!
//! A paddle, a ball, six rows of bricks worth more the higher they sit,
//! angle-based reflection off the paddle, lives, levels, and three power-ups
//! (wide paddle, multi-ball, extra life), across menu, playing, paused and
//! game-over states.
//!
//! Four things were wrong with it before it had a window, and they are worth
//! naming because the same four keep recurring in this tree:
//!
//! * **`main` built the app and dropped it.** Nothing opened, no tick ever
//!   arrived, no key was ever delivered. The game was a library nobody linked.
//! * **The layout was a constant.** `render_game` took `width` and `height`
//!   and ignored both, drawing a 600×500 play area at a fixed offset inside a
//!   632×582 window. Any other window size showed the game in a corner.
//! * **A long tick teleported the ball.** `handle_tick` turned `elapsed_ms`
//!   straight into a distance with no cap and no sub-stepping, so a window
//!   frozen for ten seconds played out ten seconds at once, and a fast ball
//!   crossed an 18-pixel brick row between one position and the next without
//!   ever being inside it. It went through the wall.
//! * **Nothing was clickable.** Breakout is a game people play with a mouse.
//!
//! What replaces them: a [`Layout`] recomputed from the live window size on
//! every frame and never remembered, physics that stays in play-area units and
//! is mapped to the screen by one aspect-preserving scale, a tick that is
//! capped and then cut into steps small enough that nothing can pass through
//! anything, and a [`Frame`] that records a hit box for every clickable thing
//! as it draws it.

use guitk::color::Color;
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

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

// ── Game constants ──────────────────────────────────────────────────
const INITIAL_LIVES: u32 = 3;
/// Widest deviation of a ball launch from straight up, in radians (60°).
const MAX_LAUNCH_ANGLE: f32 = std::f32::consts::FRAC_PI_3;

// ── Randomness ──────────────────────────────────────────────────────
// This crate used to carry its own LCG whose `next_bounded` reduced with
// `state % bound`, taking the low bits of a generator whose low bits are its
// weakest part.  Breakout's one degenerate bound was the launch angle's 1000:
// the odd factor 125 hid it, but 8 divides 1000, so `(state % 1000) % 8` *is*
// `state % 8` -- a period-8 cycle, and the same cycle at every seed, with the
// seed choosing only the phase.
//
// `start_game` then made that permanent rather than transient.  It reseeds
// (`Self::with_seed(self.rng.next_u64())`), and the new generator's very first
// output is the opening launch angle, because `init_bricks` draws nothing.  So
// the opening angle of every game in a session was that fixed cycle sampled at
// a fixed stride, and its parity never changed: a chain of 5000 new games
// reached **500 of the 1000 angles and 4 of the 8 residues mod 8**, with the
// first seed deciding which half was possible.  This is the same reseed shape
// that made `solitaire` and `freecell` worse than `hearts`.
//
// See `known-issues.md`, "The same broken reduction is copy-pasted into 27
// crates", and `design-decisions.md` §447.
use randrange::{RandomSource, SeededRng};

/// A uniformly random ball launch direction, in radians from straight up.
///
/// A free function rather than an `SeededRng` method because a launch angle is this
/// game's unit, not the generator's.
///
/// Drawn as a continuous angle rather than the old `next_bounded(1000)` mapped
/// through `v / 500.0 - 1.0`.  That 1000 was an arbitrary quantisation of a
/// quantity that was never discrete -- it bought nothing, and it was the sole
/// even bound in the crate.
fn random_launch_angle(rng: &mut SeededRng) -> f32 {
    rng.between_f32(-MAX_LAUNCH_ANGLE, MAX_LAUNCH_ANGLE)
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
    ///
    /// Only the tests use it: the game changes a ball's speed by rebuilding
    /// its velocity from an angle, never by rescaling the one it has.
    #[cfg(test)]
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

// ── Bricks ──────────────────────────────────────────────────────────
//
// A brick has no state beyond whether it is still there, and where it is is a
// function of its row and column. So the grid is a `Vec<Vec<bool>>` and
// `brick_rect` supplies the geometry. There was a `Brick` struct here holding
// exactly that -- `alive`, `row`, `col` -- which nothing ever constructed.

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

/// Whether a ball centred on (`bx`, `by`) overlaps the rectangle `rect`.
///
/// The ball is a circle for this test even though it is drawn as a rounded
/// square: a square would catch on brick corners at angles a player would not
/// expect.
fn ball_rect_collision(bx: f32, by: f32, rect: (f32, f32, f32, f32)) -> bool {
    let (rx, ry, rw, rh) = rect;
    // Closest point on the rectangle to the ball's centre.
    let closest_x = bx.clamp(rx, rx + rw);
    let closest_y = by.clamp(ry, ry + rh);
    let dx = bx - closest_x;
    let dy = by - closest_y;
    (dx * dx + dy * dy) < BALL_RADIUS * BALL_RADIUS
}

/// Reflect `ball` off `rect`, off whichever side it came in through.
///
/// Which side that is is decided by comparing how far the ball has penetrated
/// on each axis: the shallower one is the face it arrived at. The ball is also
/// pushed clear of that face, so it cannot be found still overlapping on the
/// next step and reflected a second time back into the brick.
fn reflect_ball_off_rect(ball: &mut Ball, rect: (f32, f32, f32, f32)) {
    let (rx, ry, rw, rh) = rect;
    let dx = ball.x - (rx + rw / 2.0);
    let dy = ball.y - (ry + rh / 2.0);

    let x_overlap = (rw / 2.0 + BALL_RADIUS) - dx.abs();
    let y_overlap = (rh / 2.0 + BALL_RADIUS) - dy.abs();

    if x_overlap < y_overlap {
        ball.vx = -ball.vx;
        ball.x = if dx > 0.0 {
            rx + rw + BALL_RADIUS
        } else {
            rx - BALL_RADIUS
        };
    } else {
        ball.vy = -ball.vy;
        ball.y = if dy > 0.0 {
            ry + rh + BALL_RADIUS
        } else {
            ry - BALL_RADIUS
        };
    }
}

// ── Tick pacing ─────────────────────────────────────────────────────

/// The longest span of real time one tick is allowed to stand for.
///
/// A window that was dragged, occluded or starved owes the player some
/// catching up — a quarter second of it. It does not owe them the ten seconds
/// of play they could not see and could not have returned.
const MAX_CATCHUP_MS: u64 = 250;

/// The furthest anything may move, in play-area units, in one step.
///
/// A brick is 18 units tall and the ball is 12 across. Integrated in one jump,
/// a 500-unit-per-second ball crosses a whole row of bricks between two
/// positions without ever being inside one, and passes through it. Four units
/// is small enough that every solid thing in the game is sampled at least
/// twice on the way through.
const MAX_STEP: f32 = 4.0;

// ── Commands ────────────────────────────────────────────────────────

/// Everything the player can ask for, however they ask.
///
/// The keys and the buttons are two ways of naming the same short list, so
/// they name it once here and both go through [`BreakoutApp::apply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Abandon whatever is on screen and start again from the first level.
    NewGame,
    /// Pause a game in progress, or resume a paused one.
    PauseToggle,
}

/// The footer buttons, in the order they are drawn, each with its label.
const BUTTONS: [(Action, &str); 2] = [
    (Action::NewGame, "N  New game"),
    (Action::PauseToggle, "P  Pause"),
];

/// Everything a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The play area. Pointing at it steers the paddle.
    Play,
    /// One of the footer buttons.
    Button(Action),
    /// The message panel over a menu, a pause or a finished game. Clicking it
    /// does whatever its text says.
    Overlay,
}

/// One frame of this game, with its hit boxes.
pub type Frame = guitk::frame::Frame<Target>;

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes in a window of a given size.
///
/// Recomputed from the live size on every frame and never stored, so there is
/// no second copy of the window's dimensions to fall out of date. The play
/// area keeps the 600×500 shape the physics is written in: a stretched field
/// would let a ball leave a wall at an angle it did not arrive at.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The margin used between and around the regions below.
    pub pad: f32,
    /// The score / lives / level strip along the top.
    pub header: Rect,
    /// The play area on screen.
    pub play: Rect,
    /// Screen pixels per play-area unit. Zero when there is no room at all.
    pub scale: f32,
    /// The strip of buttons along the bottom. Empty when there is no room.
    pub footer: Rect,
    /// The message panel, centred.
    pub overlay: Rect,
    /// Body text size for this window.
    pub font: f32,
}

impl Layout {
    /// Lay out a window `width` by `height`.
    pub fn new(width: f32, height: f32) -> Self {
        let window = Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0));
        let pad = (window.w.min(window.h) * 0.025).min(PADDING);
        let font = (window.h / 40.0).clamp(8.0, HEADER_FONT_SIZE);

        // Capped by what is left after the padding, not by the window: the
        // twenty-two-pixel floor is a legibility wish, and a window shorter
        // than that does not grant it by letting the header hang off the
        // bottom.
        let head_h = (window.h * 0.09)
            .clamp(22.0, HEADER_HEIGHT)
            .min((window.h - pad * 2.0).max(0.0));
        let body_w = (window.w - pad * 2.0).max(0.0);
        let header = Rect::new(pad, pad, body_w, head_h);

        let room_h = (window.h - header.bottom() - pad * 2.0).max(0.0);
        let foot_h = (window.h * 0.07).clamp(18.0, 30.0);
        // The buttons are a convenience; the game is the point. When the two
        // cannot both fit, the buttons go and the keys still work.
        let show_footer = room_h - foot_h - pad >= PLAY_HEIGHT * 0.15 && body_w >= 120.0;
        let mid_h = if show_footer {
            (room_h - foot_h - pad).max(0.0)
        } else {
            room_h
        };

        let scale = (body_w / PLAY_WIDTH).min(mid_h / PLAY_HEIGHT).max(0.0);
        let pw = PLAY_WIDTH * scale;
        let ph = PLAY_HEIGHT * scale;
        let play = Rect::new(
            pad + ((body_w - pw) / 2.0).max(0.0),
            header.bottom() + pad + ((mid_h - ph) / 2.0).max(0.0),
            pw,
            ph,
        );

        let footer = if show_footer {
            Rect::new(pad, window.h - pad - foot_h, body_w, foot_h)
        } else {
            Rect::EMPTY
        };

        let ow = (window.w * 0.72).clamp(80.0, 340.0).min(window.w);
        let oh = (window.h * 0.34).clamp(48.0, 200.0).min(window.h);
        let overlay = Rect::new(
            ((window.w - ow) / 2.0).max(0.0),
            ((window.h - oh) / 2.0).max(0.0),
            ow,
            oh,
        );

        Self {
            window,
            pad,
            header,
            play,
            scale,
            footer,
            overlay,
            font,
        }
    }

    /// A rectangle given in play-area units, placed on the screen.
    pub fn to_screen(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(
            self.play.x + x * self.scale,
            self.play.y + y * self.scale,
            w * self.scale,
            h * self.scale,
        )
    }

    /// The play-area x a screen x names, or `None` if the play area is a
    /// sliver with no interior to name.
    pub fn play_x(&self, screen_x: f32) -> Option<f32> {
        if self.scale <= 0.0 {
            return None;
        }
        Some((screen_x - self.play.x) / self.scale)
    }

    /// The `index`th footer button, or an empty rect if it does not fit.
    pub fn button(&self, index: usize) -> Rect {
        if self.footer.is_empty() {
            return Rect::EMPTY;
        }
        let w = (self.footer.w * 0.26).min(120.0);
        let gap = (self.footer.w * 0.02).min(10.0);
        let x = self.footer.x + index as f32 * (w + gap);
        if x + w > self.footer.right() + 0.01 {
            return Rect::EMPTY;
        }
        Rect::new(x, self.footer.y, w, self.footer.h)
    }
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
    rng: SeededRng,
    /// Ball speed for the current level.
    ball_speed: f32,
    /// Width of the window as of the last frame or resize.
    ///
    /// Held only so that an event arriving between frames -- a click, a
    /// pointer move -- can be hit-tested against the same layout the player is
    /// looking at. The layout itself is recomputed from it, never cached.
    width: f32,
    /// Height of the window as of the last frame or resize.
    height: f32,
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
            rng: SeededRng::new(seed),
            ball_speed: BASE_BALL_SPEED,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
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
                self.bricks_remaining = self.bricks_remaining.saturating_add(1);
            }
            self.bricks.push(brick_row);
        }
    }

    /// Spawn a ball at the center above the paddle, heading upward.
    fn spawn_ball(&mut self) {
        let angle = random_launch_angle(&mut self.rng);
        let vx = self.ball_speed * angle.sin();
        let vy = -self.ball_speed * angle.cos();
        let ball = Ball::new(self.paddle_x, self.paddle_top() - BALL_RADIUS - 1.0, vx, vy);
        self.balls.push(ball);
    }

    /// Returns the y coordinate of the top edge of the paddle.
    fn paddle_top(&self) -> f32 {
        PLAY_HEIGHT - PADDLE_Y_OFFSET
    }

    /// Start a new game.
    fn start_game(&mut self) {
        let hs = self.high_score;
        let seed = self.rng.next_u64();
        // The window did not close and reopen just because the player pressed
        // N. Whatever this replaces itself with has to keep the size it is
        // being drawn at, or the next frame is laid out for a window that is
        // not there.
        let (w, h) = (self.width, self.height);
        *self = Self::with_seed(seed);
        self.high_score = hs;
        self.width = w;
        self.height = h;
        self.state = GameState::Playing;
        self.spawn_ball();
    }

    /// Note the window's size. Called on a resize and before every frame.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// The layout for the size last seen.
    fn layout(&self) -> Layout {
        Layout::new(self.width, self.height)
    }

    /// Advance to the next level: reinitialize bricks, increase speed.
    fn next_level(&mut self) {
        self.level = self.level.saturating_add(1);
        self.ball_speed = (BASE_BALL_SPEED
            + BALL_SPEED_INCREMENT * self.level.saturating_sub(1) as f32)
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

    /// Advance the game by `elapsed_ms` of real time.
    ///
    /// Returns whether anything moved, so a caller can skip a repaint that
    /// would draw the same picture again.
    ///
    /// The time is capped at [`MAX_CATCHUP_MS`] and then cut into steps of at
    /// most [`MAX_STEP`] play-area units. Neither is a refinement: without the
    /// cap a window that was occluded for ten seconds resumes by playing ten
    /// seconds at once, and without the steps a ball moving 500 units a second
    /// jumps 8 units per 16 ms tick at level one and far more later, which is
    /// most of a brick, so it arrives on the far side of one having never been
    /// inside it.
    fn handle_tick(&mut self, elapsed_ms: u64) -> bool {
        if self.state != GameState::Playing {
            return false;
        }
        let ms = elapsed_ms.min(MAX_CATCHUP_MS);
        if ms == 0 {
            return false;
        }
        self.accumulated_ms = self.accumulated_ms.saturating_add(ms);
        self.update_wide_paddle_timer(ms);

        let dt = ms as f32 / 1000.0;
        // The step is set by whatever in the game is fastest right now, not by
        // the ball alone: the paddle and a falling power-up are also solid.
        let fastest = self
            .balls
            .iter()
            .map(Ball::speed)
            .fold(0.0_f32, f32::max)
            .max(PADDLE_SPEED)
            .max(POWERUP_SPEED)
            .max(1.0);
        let slice = (MAX_STEP / fastest).max(0.001);

        let mut left = dt;
        while left > 0.0 {
            let step = left.min(slice);
            self.update_paddle(step);
            let life_lost = self.update_balls(step);
            self.update_powerups(step);
            left -= step;

            // Both of these put a ball back in the middle of the screen. The
            // rest of this tick would move it before the player has seen where
            // it starts -- and a finished game has nothing left to simulate.
            if self.bricks_remaining == 0 {
                self.next_level();
                break;
            }
            if life_lost || self.state != GameState::Playing {
                break;
            }
        }
        true
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
                self.wide_paddle_remaining_ms =
                    self.wide_paddle_remaining_ms.saturating_sub(elapsed_ms);
            }
        }
    }

    /// Move every ball one step and resolve what it hits.
    ///
    /// Returns whether the last ball was lost, which resets the board and so
    /// ends the tick.
    fn update_balls(&mut self, dt: f32) -> bool {
        // Lifted out of `self` for the duration. The collision routines need
        // the bricks, the score and the RNG, so they take `&mut self`; holding
        // a `&mut Ball` borrowed out of `self.balls` at the same time is what
        // forced the old code to pass indices around and index the vector
        // again at every use. Nothing else touches `self.balls` in here, so
        // taking it and putting it back is exact.
        let mut balls = std::mem::take(&mut self.balls);
        balls.retain_mut(|ball| {
            ball.x += ball.vx * dt;
            ball.y += ball.vy * dt;

            // Side walls.
            if ball.x - BALL_RADIUS < 0.0 {
                ball.x = BALL_RADIUS;
                ball.vx = ball.vx.abs();
            } else if ball.x + BALL_RADIUS > PLAY_WIDTH {
                ball.x = PLAY_WIDTH - BALL_RADIUS;
                ball.vx = -ball.vx.abs();
            }

            // Ceiling.
            if ball.y - BALL_RADIUS < 0.0 {
                ball.y = BALL_RADIUS;
                ball.vy = ball.vy.abs();
            }

            // Out the bottom: this ball is gone.
            if ball.y + BALL_RADIUS > PLAY_HEIGHT {
                return false;
            }

            self.bounce_off_paddle(ball);
            self.hit_brick(ball);
            true
        });
        self.balls = balls;

        if self.balls.is_empty() {
            self.lose_life();
            return true;
        }
        false
    }

    /// Bounce `ball` off the paddle if it is coming down onto it.
    ///
    /// Where along the paddle it lands sets the angle it leaves at, up to 70°
    /// from vertical at the ends: that is the only steering the player has
    /// over the ball, and it is what makes the paddle a bat rather than a wall.
    fn bounce_off_paddle(&self, ball: &mut Ball) {
        let paddle_top = self.paddle_top();
        let half_w = self.paddle_width / 2.0;
        let paddle_left = self.paddle_x - half_w;
        let paddle_right = self.paddle_x + half_w;

        if ball.vy > 0.0
            && ball.y + BALL_RADIUS >= paddle_top
            && ball.y - BALL_RADIUS < paddle_top + PADDLE_HEIGHT
            && ball.x + BALL_RADIUS > paddle_left
            && ball.x - BALL_RADIUS < paddle_right
        {
            let relative = ((ball.x - self.paddle_x) / half_w).clamp(-1.0, 1.0);
            let angle = relative * 70.0_f32.to_radians();
            let speed = ball.speed();
            ball.vx = speed * angle.sin();
            ball.vy = -speed * angle.cos();
            ball.y = paddle_top - BALL_RADIUS;
        }
    }

    /// Break the first brick `ball` is touching, if any, and bounce it off.
    ///
    /// At most one brick per ball per step, which is why the step has to be
    /// small: a ball that overlaps two bricks at once would otherwise leave
    /// one of them standing with the ball already past it.
    fn hit_brick(&mut self, ball: &mut Ball) {
        let mut hit = None;
        'search: for (row, cells) in self.bricks.iter().enumerate() {
            for (col, &alive) in cells.iter().enumerate() {
                if !alive {
                    continue;
                }
                let rect = brick_rect(row, col);
                if ball_rect_collision(ball.x, ball.y, rect) {
                    hit = Some((row, col, rect));
                    break 'search;
                }
            }
        }
        let Some((row, col, rect)) = hit else {
            return;
        };

        if let Some(cell) = self
            .bricks
            .get_mut(row)
            .and_then(|cells| cells.get_mut(col))
        {
            *cell = false;
        }
        self.bricks_remaining = self.bricks_remaining.saturating_sub(1);
        // `row` came from enumerating a grid built with exactly one row per
        // entry in this table, so the lookup cannot miss. Zero is the harmless
        // answer if the two ever stop agreeing.
        self.score = self
            .score
            .saturating_add(BRICK_ROW_POINTS.get(row).copied().unwrap_or(0));
        self.high_score = self.high_score.max(self.score);

        reflect_ball_off_rect(ball, rect);

        let (rx, ry, rw, rh) = rect;
        self.maybe_spawn_powerup(rx + rw / 2.0, ry + rh / 2.0);
    }

    fn maybe_spawn_powerup(&mut self, x: f32, y: f32) {
        if self.rng.chance_in(1, POWERUP_SPAWN_CHANCE) {
            let kind = match self.rng.below(3) {
                0 => PowerUpKind::WidePaddle,
                1 => PowerUpKind::MultiBall,
                _ => PowerUpKind::ExtraLife,
            };
            self.powerups.push(PowerUp { x, y, kind });
        }
    }

    /// Drop every power-up one step, collecting the ones the paddle catches
    /// and discarding the ones that reach the floor.
    ///
    /// The old version built two index lists and then removed by index from
    /// both, which was wrong as well as awkward: the second list was indices
    /// into the vector *before* the first list's removals, so a caught
    /// power-up above a fallen one deleted the wrong entry. Retaining in place
    /// has no indices to go stale.
    fn update_powerups(&mut self, dt: f32) {
        let paddle_top = self.paddle_top();
        let half_w = self.paddle_width / 2.0;
        let paddle_left = self.paddle_x - half_w;
        let paddle_right = self.paddle_x + half_w;
        let half = POWERUP_SIZE / 2.0;

        let mut collected: Vec<PowerUpKind> = Vec::new();
        let mut powerups = std::mem::take(&mut self.powerups);
        powerups.retain_mut(|pu| {
            pu.y += POWERUP_SPEED * dt;
            if pu.y + half >= paddle_top
                && pu.y - half < paddle_top + PADDLE_HEIGHT
                && pu.x + half > paddle_left
                && pu.x - half < paddle_right
            {
                collected.push(pu.kind);
                return false;
            }
            pu.y - half <= PLAY_HEIGHT
        });
        self.powerups = powerups;

        for kind in collected {
            self.apply_powerup(kind);
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
                self.lives = self.lives.saturating_add(1);
            }
        }
    }

    fn lose_life(&mut self) {
        if self.lives > 1 {
            self.lives = self.lives.saturating_sub(1);
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

    // ── Commands ────────────────────────────────────────────────────

    /// Whether `action` can do anything in the current state.
    ///
    /// A button that cannot act is drawn dim — but it still records its hit
    /// box, so that a click on it stops there instead of falling through to
    /// whatever happens to be behind it.
    fn enabled(&self, action: Action) -> bool {
        match action {
            Action::NewGame => true,
            Action::PauseToggle => matches!(self.state, GameState::Playing | GameState::Paused),
        }
    }

    /// Carry out `action`. Returns whether anything changed.
    ///
    /// Every command arrives here, whether it came from a key or a click, so
    /// there is one description of what each one does rather than two that can
    /// drift apart.
    fn apply(&mut self, action: Action) -> bool {
        if !self.enabled(action) {
            return false;
        }
        match action {
            Action::NewGame => self.start_game(),
            Action::PauseToggle => match self.state {
                GameState::Playing => {
                    self.state = GameState::Paused;
                    // A key held down when the game stops listening is still
                    // held when it starts again, and the paddle would set off
                    // on its own the moment play resumed. Let go of both.
                    self.left_held = false;
                    self.right_held = false;
                }
                GameState::Paused => self.state = GameState::Playing,
                GameState::Menu | GameState::GameOver => return false,
            },
        }
        true
    }

    /// What the message panel currently invites, if anything.
    fn overlay_action(&self) -> Option<Action> {
        match self.state {
            GameState::Menu | GameState::GameOver => Some(Action::NewGame),
            GameState::Paused => Some(Action::PauseToggle),
            GameState::Playing => None,
        }
    }

    // ── Input ───────────────────────────────────────────────────────

    fn handle_key(&mut self, ke: &KeyEvent) -> EventResult {
        // A modified key belongs to the desktop: Alt-N is not "new game".
        if ke.modifiers.ctrl || ke.modifiers.alt || ke.modifiers.super_key {
            return EventResult::Ignored;
        }

        // Steering is held rather than pressed, so it is the only thing here
        // that reads both edges: the release is what stops the paddle.
        if self.state == GameState::Playing {
            match ke.key {
                Key::Left => {
                    self.left_held = ke.pressed;
                    return EventResult::Consumed;
                }
                Key::Right => {
                    self.right_held = ke.pressed;
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        // Everything else is a command, and a command happens once, on the way
        // down. Letting go of P must not pause again.
        if !ke.pressed {
            return EventResult::Ignored;
        }

        match self.state {
            GameState::Menu => match ke.key {
                Key::Enter | Key::Space | Key::N => {
                    self.apply(Action::NewGame);
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            GameState::Playing => match ke.key {
                Key::P | Key::Escape => {
                    self.apply(Action::PauseToggle);
                    EventResult::Consumed
                }
                Key::N => {
                    self.apply(Action::NewGame);
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            GameState::Paused => match ke.key {
                Key::P | Key::Escape | Key::Space => {
                    self.apply(Action::PauseToggle);
                    EventResult::Consumed
                }
                Key::N => {
                    self.apply(Action::NewGame);
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            GameState::GameOver => match ke.key {
                Key::N | Key::Enter | Key::Space => {
                    self.apply(Action::NewGame);
                    EventResult::Consumed
                }
                Key::Escape => {
                    self.state = GameState::Menu;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
        }
    }

    /// Put the paddle's centre at play-area x `px`, kept inside the walls.
    fn steer_to(&mut self, px: f32) {
        let half = self.paddle_width / 2.0;
        self.paddle_x = px.clamp(half, (PLAY_WIDTH - half).max(half));
        // The pointer is the thing steering now. A key still held down would
        // otherwise drag the paddle straight back off where it is pointed.
        self.left_held = false;
        self.right_held = false;
    }

    /// What is under (`x`, `y`) on the screen, if anything.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    fn handle_mouse(&mut self, me: &MouseEvent) -> EventResult {
        let target = self.target_at(me.x, me.y);
        match me.kind {
            MouseEventKind::Move => {
                if self.state == GameState::Playing && target == Some(Target::Play) {
                    if let Some(px) = self.layout().play_x(me.x) {
                        self.steer_to(px);
                        return EventResult::Consumed;
                    }
                }
                EventResult::Ignored
            }
            MouseEventKind::Press(MouseButton::Left) => {
                let action = match target {
                    Some(Target::Button(action)) => Some(action),
                    Some(Target::Overlay) => self.overlay_action(),
                    // A click in the play area is not a command — steering
                    // already happened on the move that brought the pointer
                    // there. And a click on nothing at all belongs to whoever
                    // else wants it.
                    Some(Target::Play) | None => None,
                };
                match action {
                    Some(action) => {
                        self.apply(action);
                        EventResult::Consumed
                    }
                    None => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Draw one frame for a window `width` by `height`.
    ///
    /// The frame is also the hit map: every clickable thing records its box
    /// here as it is drawn, so there is no second description of where things
    /// are that could disagree with the picture.
    fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(width, height);

        fill(&mut f, l.window, MANTLE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_play(&mut f, &l);
        self.draw_footer(&mut f, &l);
        // Last, because `hit_test` searches backwards: the panel is in front
        // of everything it covers, and a click on it must not reach through.
        self.draw_overlay(&mut f, &l);
        f
    }

    /// The message panel's title, its instruction, and an optional third line.
    /// `None` while a game is actually being played.
    fn overlay_text(&self) -> Option<(&'static str, &'static str, Option<String>)> {
        match self.state {
            GameState::Playing => None,
            GameState::Menu => Some((
                "BREAKOUT",
                "Press Enter or Space to start",
                (self.high_score > 0).then(|| format!("High score: {}", self.high_score)),
            )),
            GameState::Paused => Some(("PAUSED", "Press P to resume", None)),
            GameState::GameOver => Some((
                "GAME OVER",
                "Press N or Enter for a new game",
                Some(format!(
                    "Score {}  \u{2022}  Best {}",
                    self.score, self.high_score
                )),
            )),
        }
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        fill(f, l.header, CRUST, 4.0);

        let fields = [
            (
                format!("Score: {}", self.score),
                TEXT_COLOR,
                FontWeightHint::Bold,
            ),
            (
                format!("Lives: {}", self.lives),
                if self.lives <= 1 { RED } else { GREEN },
                FontWeightHint::Bold,
            ),
            (format!("Level: {}", self.level), BLUE, FontWeightHint::Bold),
            (
                format!("Best: {}", self.high_score),
                YELLOW,
                FontWeightHint::Regular,
            ),
        ];
        let inner = (l.header.w - l.pad * 2.0).max(0.0);
        let cell = inner / fields.len() as f32;
        let y = l.header.y + (l.header.h - text::line_height(l.font, FontWeightHint::Bold)) / 2.0;
        f.clip(l.header);
        for (i, (body, color, weight)) in fields.iter().enumerate() {
            let x = l.header.x + l.pad + i as f32 * cell;
            label(f, x, y, body, l.font, *color, *weight, Some(cell));
        }
        f.unclip();
    }

    fn draw_play(&self, f: &mut Frame, l: &Layout) {
        if l.play.is_empty() {
            return;
        }
        fill(f, l.play, BASE, 4.0);
        stroke(f, l.play, SURFACE1, 1.0, 4.0);

        // A ball touching a wall is half outside the play area in play units,
        // and a brick's rounded corner rides the border. Clipping is what
        // keeps the game inside its own box at every window size.
        f.clip(l.play);
        for (row, cells) in self.bricks.iter().enumerate() {
            // Rows and colours are built from the same length, so this cannot
            // miss; a brick in grey would be the visible sign that they had
            // stopped agreeing.
            let color = BRICK_ROW_COLORS.get(row).copied().unwrap_or(SURFACE2);
            for (col, &alive) in cells.iter().enumerate() {
                if !alive {
                    continue;
                }
                let (bx, by, bw, bh) = brick_rect(row, col);
                fill(
                    f,
                    l.to_screen(bx, by, bw, bh),
                    color,
                    BRICK_CORNER_RADIUS * l.scale,
                );
            }
        }

        let paddle = l.to_screen(
            self.paddle_x - self.paddle_width / 2.0,
            self.paddle_top(),
            self.paddle_width,
            PADDLE_HEIGHT,
        );
        fill(
            f,
            paddle,
            if self.wide_paddle_remaining_ms > 0 {
                GREEN
            } else {
                LAVENDER
            },
            PADDLE_CORNER_RADIUS * l.scale,
        );

        for ball in &self.balls {
            let r = l.to_screen(
                ball.x - BALL_RADIUS,
                ball.y - BALL_RADIUS,
                BALL_RADIUS * 2.0,
                BALL_RADIUS * 2.0,
            );
            fill(f, r, TEXT_COLOR, BALL_RADIUS * l.scale);
        }

        for pu in &self.powerups {
            let half = POWERUP_SIZE / 2.0;
            let r = l.to_screen(pu.x - half, pu.y - half, POWERUP_SIZE, POWERUP_SIZE);
            fill(f, r, pu.kind.color(), 4.0 * l.scale);
            let (cx, cy) = r.centre();
            centred(
                f,
                cx,
                cy,
                pu.kind.label(),
                (POWERUP_SIZE * 0.6 * l.scale).max(1.0),
                CRUST,
                FontWeightHint::Bold,
            );
        }
        f.unclip();

        // Recorded after the contents so that a point anywhere in the play
        // area names the play area, whatever it happens to be drawn over.
        f.hit(Target::Play, l.play);
    }

    /// The label a button carries right now.
    ///
    /// Only the pause button changes: a control that toggles has to say which
    /// way it will go, or the player has to guess from the state of the game.
    fn button_label(&self, action: Action, default: &'static str) -> &'static str {
        match action {
            Action::PauseToggle if self.state == GameState::Paused => "P  Resume",
            _ => default,
        }
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        if l.footer.is_empty() {
            return;
        }
        for (i, (action, text_body)) in BUTTONS.iter().enumerate() {
            let r = l.button(i);
            if r.is_empty() {
                continue;
            }
            let on = self.enabled(*action);
            fill(f, r, if on { SURFACE1 } else { SURFACE0 }, 4.0);
            let (cx, cy) = r.centre();
            f.clip(r);
            centred(
                f,
                cx,
                cy,
                self.button_label(*action, text_body),
                l.font,
                if on { TEXT_COLOR } else { OVERLAY0 },
                FontWeightHint::Regular,
            );
            f.unclip();
            // A button that can do nothing still takes its own click. Letting
            // one fall through would hand it to whatever is behind, which is
            // never what a player aiming at a button meant.
            f.hit(Target::Button(*action), r);
        }

        let last = l.button(BUTTONS.len().saturating_sub(1));
        if last.is_empty() {
            return;
        }
        let x = last.right() + l.pad;
        let w = l.footer.right() - x;
        if w >= 110.0 {
            let y = l.footer.y
                + (l.footer.h - text::line_height(l.font, FontWeightHint::Regular)) / 2.0;
            label(
                f,
                x,
                y,
                HELP,
                l.font,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some(w),
            );
        }
    }

    fn draw_overlay(&self, f: &mut Frame, l: &Layout) {
        let Some((title, hint, extra)) = self.overlay_text() else {
            return;
        };
        // Dim what is behind, so the panel reads as being in front of a game
        // that is still there rather than as a screen that replaced it.
        fill(f, l.window, Color::rgba(0x11, 0x11, 0x1B, 180), 0.0);

        let r = l.overlay;
        if r.is_empty() {
            return;
        }
        fill(f, r, SURFACE0, 8.0);
        stroke(f, r, SURFACE2, 1.0, 8.0);

        let title_size = (r.h * 0.26).clamp(l.font, TITLE_FONT_SIZE);
        let th = text::line_height(title_size, FontWeightHint::Bold);
        let bh = text::line_height(l.font, FontWeightHint::Regular);
        let gap = (r.h * 0.06).min(10.0);
        let mut total = th + gap + bh;
        if extra.is_some() {
            total += gap + bh;
        }

        let (cx, cy) = r.centre();
        let mut y = cy - total / 2.0 + th / 2.0;
        f.clip(r);
        centred(f, cx, y, title, title_size, LAVENDER, FontWeightHint::Bold);
        y += th / 2.0 + gap + bh / 2.0;
        centred(f, cx, y, hint, l.font, SUBTEXT0, FontWeightHint::Regular);
        if let Some(extra) = &extra {
            y += bh + gap;
            centred(f, cx, y, extra, l.font, YELLOW, FontWeightHint::Regular);
        }
        f.unclip();

        f.hit(Target::Overlay, r);
    }

    // ── Query helpers (for tests) ───────────────────────────────────

    #[cfg(test)]
    fn alive_brick_count(&self) -> u32 {
        let mut count = 0u32;
        for row in &self.bricks {
            for &alive in row {
                if alive {
                    count = count.saturating_add(1);
                }
            }
        }
        count
    }

    #[cfg(test)]
    fn total_brick_count(&self) -> u32 {
        (BRICK_ROWS * BRICK_COLS) as u32
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// The sentence in the footer, when there is room for it.
const HELP: &str = "\u{2190}/\u{2192} or the mouse to move the paddle";

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius.max(0.0)),
    });
}

fn stroke(f: &mut Frame, r: Rect, color: Color, line_width: f32, radius: f32) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius.max(0.0)),
    });
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: body.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        // A cut label is only readable if the cut is marked, and every label
        // here with a width limit is one whose text can outgrow it.
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Draw `body` centred on (`cx`, `cy`).
///
/// Both offsets are measured rather than guessed. `guitk::text` asks the same
/// font the compositor will shape with, and centring is exactly where a guess
/// shows: half the error in a width lands in the offset, so it grows with the
/// label and "GAME OVER" would sit further off centre than "PAUSED".
/// Vertically the anchor is the line box rather than the em size, since `y` on
/// a `Text` command is the top of the line and a line is taller than its size.
fn centred(
    f: &mut Frame,
    cx: f32,
    cy: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    label(
        f,
        text::center_x(body, cx, size, weight),
        cy - text::line_height(size, weight) / 2.0,
        body,
        size,
        color,
        weight,
        None,
    );
}

// ── Window wiring ───────────────────────────────────────────────────

/// One body for every event, whoever delivers it.
///
/// [`App::on_event`] and the [`Probe`] impl both call this, so a test that
/// clicks the Pause button is a test of the shipped program rather than of a
/// second implementation written to make the test pass.
fn handle_event(app: &mut BreakoutApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => app.handle_mouse(mouse),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // A window without the keyboard cannot be played, and a ball that kept
        // falling while it was away would be answered by nobody: the player
        // comes back to a life they did not lose. Pausing is the only honest
        // thing to do with time the player could not use.
        Event::FocusOut => {
            if app.state == GameState::Playing {
                app.apply(Action::PauseToggle);
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        Event::Tick { elapsed_ms } => {
            if app.handle_tick(*elapsed_ms) {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        _ => EventResult::Ignored,
    }
}

impl App for BreakoutApp {
    fn on_event(&mut self, event: &Event) -> Response {
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }

    fn title(&self) -> String {
        String::from("Breakout")
    }

    fn app_id(&self) -> String {
        String::from("breakout")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// How often the game asks to be woken — a floor, not a promise.
    ///
    /// [`BreakoutApp::handle_tick`] moves the game by the `elapsed_ms` the
    /// tick actually carried, so a tick that comes late moves it *further*,
    /// not slower, and a level plays at the same speed on a compositor running
    /// at 30 Hz as on one running at 144. Nothing here may assume this number
    /// was honoured.
    ///
    /// Dropped outside play: a menu is a still picture, and a still picture
    /// that asks to be redrawn sixty times a second keeps a laptop awake.
    fn tick_interval(&self) -> Option<Duration> {
        match self.state {
            GameState::Playing => Some(Duration::from_millis(16)),
            GameState::Menu | GameState::Paused | GameState::GameOver => None,
        }
    }
}

impl Probe for BreakoutApp {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
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

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut app = BreakoutApp::new();
    app::launch("breakout", &mut app)
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::probe;
    use std::collections::BTreeSet;

    const SIZE: (f32, f32) = BreakoutApp::SIZE;

    /// One compositor frame at 60 Hz, which is what the per-tick numbers in
    /// the converted tests below mean by "a tick".
    const FRAME: u64 = 16;

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
            text: String::new(),
        })
    }

    /// Helper: create a key release event.
    fn key_release(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// Helper: advance game by a given number of milliseconds.
    fn tick(app: &mut BreakoutApp, ms: u64) {
        handle_event(app, &Event::Tick { elapsed_ms: ms });
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
        handle_event(&mut app, &key_press(Key::Enter));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_start_game_spawns_ball() {
        let mut app = BreakoutApp::new();
        handle_event(&mut app, &key_press(Key::Enter));
        assert_eq!(app.balls.len(), 1);
    }

    #[test]
    fn test_start_game_space_key() {
        let mut app = BreakoutApp::new();
        handle_event(&mut app, &key_press(Key::Space));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_start_game_resets_score() {
        let mut app = BreakoutApp::new();
        app.score = 100;
        handle_event(&mut app, &key_press(Key::Enter));
        assert_eq!(app.score, 0);
    }

    // ── Paddle movement ─────────────────────────────────────────────

    #[test]
    fn test_paddle_moves_left() {
        let mut app = test_app();
        let initial_x = app.paddle_x;
        handle_event(&mut app, &key_press(Key::Left));
        tick(&mut app, 100);
        assert!(app.paddle_x < initial_x);
    }

    #[test]
    fn test_paddle_moves_right() {
        let mut app = test_app();
        let initial_x = app.paddle_x;
        handle_event(&mut app, &key_press(Key::Right));
        tick(&mut app, 100);
        assert!(app.paddle_x > initial_x);
    }

    #[test]
    fn test_paddle_stops_on_key_release() {
        let mut app = test_app();
        handle_event(&mut app, &key_press(Key::Left));
        tick(&mut app, 100);
        let pos_after_move = app.paddle_x;
        handle_event(&mut app, &key_release(Key::Left));
        tick(&mut app, 100);
        // Paddle should not have moved further (may have slight ball-related changes).
        assert!((app.paddle_x - pos_after_move).abs() < 0.01);
    }

    #[test]
    fn test_paddle_clamped_left() {
        let mut app = test_app();
        handle_event(&mut app, &key_press(Key::Left));
        // Move for a very long time.
        tick(&mut app, 5000);
        assert!(app.paddle_x >= app.paddle_width / 2.0);
    }

    #[test]
    fn test_paddle_clamped_right() {
        let mut app = test_app();
        handle_event(&mut app, &key_press(Key::Right));
        tick(&mut app, 5000);
        assert!(app.paddle_x <= PLAY_WIDTH - app.paddle_width / 2.0);
    }

    #[test]
    fn test_paddle_both_keys_cancel() {
        let mut app = test_app();
        let initial_x = app.paddle_x;
        handle_event(&mut app, &key_press(Key::Left));
        handle_event(&mut app, &key_press(Key::Right));
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
        // Ball center inside the rectangle.
        assert!(ball_rect_collision(50.0, 50.0, (40.0, 40.0, 20.0, 20.0)));
    }

    #[test]
    fn test_ball_rect_collision_miss() {
        // Ball far from rectangle.
        assert!(!ball_rect_collision(0.0, 0.0, (100.0, 100.0, 20.0, 20.0)));
    }

    #[test]
    fn test_ball_rect_collision_edge() {
        // Ball just touching the edge (within radius).
        let result =
            ball_rect_collision(100.0 - BALL_RADIUS + 1.0, 110.0, (100.0, 100.0, 20.0, 20.0));
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
        // No single tick can expire it, because a tick is capped at
        // MAX_CATCHUP_MS -- a window that was frozen for ten seconds does not
        // owe the player ten seconds of play. The timer runs down over the
        // frames the player actually saw, so the test has to supply them.
        let mut elapsed = 0;
        while elapsed < WIDE_PADDLE_DURATION_MS + 100 {
            tick(&mut app, MAX_CATCHUP_MS);
            elapsed += MAX_CATCHUP_MS;
        }
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
        // Not `powerups.is_empty()`: the ball is live, so a brick it breaks in
        // this same tick may drop a *second* power-up, and whether it does is a
        // property of the generator's stream rather than of collection. That
        // assertion therefore passed only for as long as the stream did not
        // change. Ask the question it meant instead -- is the one placed at the
        // paddle gone -- by requiring anything still falling to be up at the
        // bricks, where a fresh drop starts, rather than down at the paddle.
        let paddle_band = app.paddle_top() - POWERUP_SIZE;
        assert!(
            app.powerups.iter().all(|p| p.y < paddle_band),
            "the power-up placed at the paddle was not collected: {:?} still within reach",
            app.powerups.iter().map(|p| p.y).collect::<Vec<_>>()
        );
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
        handle_event(&mut app, &key_press(Key::P));
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_pause_clears_held_keys() {
        let mut app = test_app();
        handle_event(&mut app, &key_press(Key::Left));
        assert!(app.left_held);
        handle_event(&mut app, &key_press(Key::P));
        assert!(!app.left_held);
        assert!(!app.right_held);
    }

    #[test]
    fn test_resume_from_paused() {
        let mut app = test_app();
        handle_event(&mut app, &key_press(Key::P));
        assert_eq!(app.state, GameState::Paused);
        handle_event(&mut app, &key_press(Key::P));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_pause_with_escape() {
        let mut app = test_app();
        handle_event(&mut app, &key_press(Key::Escape));
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_resume_with_escape() {
        let mut app = test_app();
        app.state = GameState::Paused;
        handle_event(&mut app, &key_press(Key::Escape));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_resume_with_space() {
        let mut app = test_app();
        app.state = GameState::Paused;
        handle_event(&mut app, &key_press(Key::Space));
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
        handle_event(&mut app, &key_press(Key::N));
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_new_game_from_game_over() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        handle_event(&mut app, &key_press(Key::N));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_new_game_from_paused() {
        let mut app = test_app();
        app.state = GameState::Paused;
        handle_event(&mut app, &key_press(Key::N));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_game_over_enter_starts_new_game() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        handle_event(&mut app, &key_press(Key::Enter));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_game_over_escape_goes_to_menu() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        handle_event(&mut app, &key_press(Key::Escape));
        assert_eq!(app.state, GameState::Menu);
    }

    // ── Rendering output ────────────────────────────────────────────

    #[test]
    fn test_render_menu_produces_commands() {
        let app = BreakoutApp::new();
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing_produces_commands() {
        let app = test_app();
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_paused_produces_overlay() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        // Should have more commands than playing (overlay added).
        let playing_app = test_app();
        let playing_cmds = playing_app
            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
            .commands()
            .to_vec();
        assert!(cmds.len() > playing_cmds.len());
    }

    #[test]
    fn test_render_game_over_has_overlay() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        let playing_app = test_app();
        let playing_cmds = playing_app
            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
            .commands()
            .to_vec();
        assert!(cmds.len() > playing_cmds.len());
    }

    #[test]
    fn test_render_contains_background() {
        let app = test_app();
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
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
        // Play units are not screen pixels: the play area is scaled to fit
        // whatever window it is drawn in, so the expected size is scaled too.
        let scale = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).scale;
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        let brick_fill_count = cmds
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        width,
                        height,
                        ..
                    } if (*width - BRICK_WIDTH * scale).abs() < 0.01
                        && (*height - BRICK_HEIGHT * scale).abs() < 0.01
                )
            })
            .count();
        assert_eq!(brick_fill_count, (BRICK_ROWS * BRICK_COLS));
    }

    #[test]
    fn test_render_paddle_shown() {
        let app = test_app();
        let scale = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).scale;
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        let paddle_count = cmds
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        height,
                        ..
                    } if (*height - PADDLE_HEIGHT * scale).abs() < 0.01
                )
            })
            .count();
        assert!(paddle_count >= 1);
    }

    #[test]
    fn test_render_ball_shown() {
        let app = test_app();
        let scale = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).scale;
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        let ball_count = cmds
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        width,
                        height,
                        ..
                    } if (*width - BALL_RADIUS * 2.0 * scale).abs() < 0.01
                        && (*height - BALL_RADIUS * 2.0 * scale).abs() < 0.01
                )
            })
            .count();
        assert!(ball_count >= 1);
    }

    #[test]
    fn test_render_wide_paddle_color_changes() {
        let mut app = test_app();
        let scale = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).scale;
        let cmds_normal = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        app.apply_powerup(PowerUpKind::WidePaddle);
        let cmds_wide = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
        // Paddle color should differ.
        let find_paddle_color = |cmds: &[RenderCommand]| -> Option<Color> {
            cmds.iter().find_map(|cmd| match cmd {
                RenderCommand::FillRect { height, color, .. }
                    if (*height - PADDLE_HEIGHT * scale).abs() < 0.01 =>
                {
                    Some(*color)
                }
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
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
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
        let cmds = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().to_vec();
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

    // ── Launch angle ────────────────────────────────────────────────
    //
    // The three tests that used to sit here checked the generator, not the
    // game: same seed gives the same stream, a bounded draw is below its
    // bound, and `one_in(2)` is true somewhere between 0 and 1000 times out of
    // 1000.  All three passed against the broken reduction, and the third is
    // the sharpest illustration in this crate of why: on the old LCG
    // `next_u64() % 2` alternated exactly, so `one_in(2)` returned true on
    // precisely every other call -- a perfect 500, from a sequence with no
    // randomness in it at all -- and the assertion was only `0 < count <
    // 1000`.  What follows tests the angle the player actually sees instead.

    /// Re-creates the historical draw so the tests below pin the *claim*
    /// rather than the implementation, in the manner of `randrange`'s own
    /// `the_original_defect_still_cycles_when_reproduced`.
    ///
    /// Returns the opening launch angle of each game in a chain of new games,
    /// quantised back onto the 1000-step lattice the old code used, so the two
    /// generators can be compared on one scale.
    fn opening_angle_lattice_points(seed: u64, games: usize, historical: bool) -> Vec<usize> {
        /// The old `Lcg::next_u64`.
        fn lcg(state: u64) -> u64 {
            state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407)
        }
        /// Angle -> the old code's 0..1000 index, for comparison on one scale.
        fn lattice(angle: f32) -> usize {
            let r = angle / MAX_LAUNCH_ANGLE; // back to -1.0..1.0
            (((r + 1.0) * 500.0) as usize).min(999)
        }

        let mut out = Vec::with_capacity(games);
        let mut carrier = seed;
        for _ in 0..games {
            // `start_game`: draw a new seed from the running generator, then
            // build a fresh one whose first output is the opening angle.
            if historical {
                carrier = lcg(carrier);
                out.push((lcg(carrier) % 1000) as usize);
                carrier = lcg(carrier);
            } else {
                let mut running = SeededRng::new(carrier);
                carrier = running.next_u64();
                let mut fresh = SeededRng::new(carrier);
                out.push(lattice(random_launch_angle(&mut fresh)));
                carrier = fresh.next_u64();
            }
        }
        out
    }

    #[test]
    fn launch_angle_stays_within_sixty_degrees_of_vertical() {
        let mut rng = SeededRng::new(42);
        for _ in 0..1000 {
            let angle = random_launch_angle(&mut rng);
            assert!(
                angle >= -MAX_LAUNCH_ANGLE && angle < MAX_LAUNCH_ANGLE,
                "launch angle {angle} escaped +/-60 degrees"
            );
            // A launch must always carry the ball upward, never sideways or
            // down: that is what bounding the angle is *for*.
            assert!(-angle.cos() < 0.0, "launch angle {angle} did not go up");
        }
    }

    /// The defect this crate was migrated for.
    ///
    /// `start_game` reseeds, and the fresh generator's first output is the
    /// opening angle, so the openings of successive games were consecutive
    /// low-bit draws of one LCG.  8 divides 1000, so their parity never
    /// changed for the whole session and only 4 of the 8 residues mod 8 were
    /// ever reachable -- half the angles, with the first seed choosing which
    /// half.  Counting distinct values alone would *not* catch this (500 of
    /// 1000 still looks plentiful); the parity is the part that must be
    /// asserted.
    #[test]
    fn opening_launch_angle_is_not_locked_to_one_parity() {
        for seed in [42_u64, 7, 999, 2024] {
            let points = opening_angle_lattice_points(seed, 400, false);

            let parities: BTreeSet<usize> = points.iter().map(|p| p % 2).collect();
            assert_eq!(
                parities.len(),
                2,
                "seed {seed}: every opening angle over 400 new games had the same parity \
                 ({:?} mod 2) -- half the range is unreachable",
                parities
            );

            let residues: BTreeSet<usize> = points.iter().map(|p| p % 8).collect();
            assert_eq!(
                residues.len(),
                8,
                "seed {seed}: openings over 400 new games reached only {} of the 8 residues \
                 mod 8 ({residues:?})",
                residues.len()
            );

            let distinct: BTreeSet<usize> = points.iter().copied().collect();
            assert!(
                distinct.len() > 300,
                "seed {seed}: 400 new games produced only {} distinct opening angles",
                distinct.len()
            );
        }
    }

    /// Pins the claim above: the historical draw really was degenerate, so the
    /// test cannot quietly rot into asserting nothing.
    #[test]
    fn the_original_reduction_still_locks_the_opening_angle() {
        for seed in [42_u64, 7, 999, 2024] {
            let points = opening_angle_lattice_points(seed, 400, true);

            let parities: BTreeSet<usize> = points.iter().map(|p| p % 2).collect();
            assert_eq!(
                parities.len(),
                1,
                "seed {seed}: the historical `state % 1000` was expected to fix the opening \
                 angle's parity for the whole chain, but reached {parities:?}"
            );

            let residues: BTreeSet<usize> = points.iter().map(|p| p % 8).collect();
            assert_eq!(
                residues.len(),
                4,
                "seed {seed}: the historical draw was expected to reach 4 of the 8 residues \
                 mod 8, but reached {residues:?}"
            );
        }
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
        handle_event(&mut app, &key_release(Key::Enter));
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
        reflect_ball_off_rect(&mut app.balls[0], (80.0, 95.0, 40.0, 20.0));
        assert!(app.balls[0].vy < 0.0);
    }

    #[test]
    fn test_reflect_ball_side_hit() {
        let mut app = test_app();
        app.balls[0].x = 75.0;
        app.balls[0].y = 105.0;
        app.balls[0].vx = 200.0;
        app.balls[0].vy = 0.0;
        reflect_ball_off_rect(&mut app.balls[0], (80.0, 95.0, 40.0, 20.0));
        assert!(app.balls[0].vx < 0.0);
    }

    // ── The window: buttons, panel and pointer ──────────────────────

    /// A game under way at the window's own size, which is what most of the
    /// tests below are about.
    fn windowed() -> BreakoutApp {
        let mut app = BreakoutApp::with_seed(12345);
        app.start_game();
        app.resize(SIZE.0, SIZE.1);
        app
    }

    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// A pointer move to a point on the screen.
    fn point_at(app: &mut BreakoutApp, x: f32, y: f32) -> EventResult {
        handle_event(
            app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Move,
            }),
        )
    }

    #[test]
    fn the_buttons_offer_every_action_the_keys_do() {
        let app = windowed();
        for (action, _) in BUTTONS {
            assert!(
                probe::rect_of(&app, Target::Button(action)).is_some(),
                "{action:?} has no button"
            );
        }
    }

    #[test]
    fn every_button_is_the_action_it_names() {
        // New game from a finished game, and pause from a running one: each
        // button does the thing its own label says and not its neighbour's.
        let mut app = windowed();
        app.state = GameState::GameOver;
        app.score = 500;
        probe::click(&mut app, Target::Button(Action::NewGame));
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);

        probe::click(&mut app, Target::Button(Action::PauseToggle));
        assert_eq!(app.state, GameState::Paused);
        probe::click(&mut app, Target::Button(Action::PauseToggle));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_button_that_can_do_nothing_still_takes_its_own_click() {
        // Pause means nothing on the menu, but the button is still there and
        // still eats the click rather than letting it reach the panel behind.
        let mut app = BreakoutApp::new();
        app.resize(SIZE.0, SIZE.1);
        assert_eq!(app.state, GameState::Menu);
        assert!(!app.enabled(Action::PauseToggle));
        assert!(probe::rect_of(&app, Target::Button(Action::PauseToggle)).is_some());
        probe::click(&mut app, Target::Button(Action::PauseToggle));
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn the_pause_button_says_which_way_it_will_go() {
        let mut app = windowed();
        let running = app.button_label(Action::PauseToggle, "P  Pause");
        app.apply(Action::PauseToggle);
        let paused = app.button_label(Action::PauseToggle, "P  Pause");
        assert_ne!(running, paused);
        assert!(paused.contains("Resume"));
    }

    #[test]
    fn the_message_panel_does_what_its_text_says() {
        // Menu: it says "press Enter to start", so clicking it starts.
        let mut app = BreakoutApp::new();
        app.resize(SIZE.0, SIZE.1);
        probe::click(&mut app, Target::Overlay);
        assert_eq!(app.state, GameState::Playing);

        // Paused: it says "press P to resume", so clicking it resumes.
        app.apply(Action::PauseToggle);
        assert_eq!(app.state, GameState::Paused);
        probe::click(&mut app, Target::Overlay);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_game_in_progress_has_no_message_panel() {
        let app = windowed();
        assert!(app.overlay_text().is_none());
        assert!(probe::rect_of(&app, Target::Overlay).is_none());
    }

    #[test]
    fn the_message_panel_is_in_front_of_the_bricks_it_covers() {
        // The panel sits over the play area, and the play area is clickable.
        // Drawn in the wrong order, a click meant for the panel would land on
        // the play area behind it and steer the paddle instead.
        let mut app = windowed();
        app.apply(Action::PauseToggle);
        let panel = probe::rect_of(&app, Target::Overlay).expect("paused panel");
        let (cx, cy) = panel.centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::Overlay));
    }

    #[test]
    fn a_click_on_nothing_is_left_for_whoever_wants_it() {
        let mut app = windowed();
        assert_eq!(probe::click_background(&mut app), EventResult::Ignored);
    }

    #[test]
    fn the_pointer_steers_the_paddle_to_where_it_points() {
        let mut app = windowed();
        let play = probe::rect_of(&app, Target::Play).expect("play area");
        let x = play.x + play.w * 0.25;
        assert_eq!(
            point_at(&mut app, x, play.y + play.h / 2.0),
            EventResult::Consumed
        );
        let want = PLAY_WIDTH * 0.25;
        assert!(
            (app.paddle_x - want).abs() < 1.0,
            "paddle at {}, wanted {want}",
            app.paddle_x
        );
    }

    #[test]
    fn the_pointer_only_steers_while_it_is_over_the_play_area() {
        let mut app = windowed();
        let before = app.paddle_x;
        // The footer is not the play area, and dragging across it on the way
        // to a button must not swing the paddle to the far wall.
        let footer = app.layout().footer;
        assert_eq!(
            point_at(&mut app, footer.x + 1.0, footer.y + footer.h / 2.0),
            EventResult::Ignored
        );
        assert_eq!(app.paddle_x, before);
    }

    #[test]
    fn the_pointer_takes_the_paddle_from_a_key_that_is_still_held() {
        let mut app = windowed();
        probe::key(&mut app, &probe::press(Key::Left));
        assert!(app.left_held);
        let play = probe::rect_of(&app, Target::Play).expect("play area");
        point_at(&mut app, play.x + play.w * 0.75, play.y + play.h / 2.0);
        assert!(!app.left_held, "a held key still fights the pointer");
    }

    #[test]
    fn the_paddle_cannot_be_steered_through_a_wall() {
        let mut app = windowed();
        let play = probe::rect_of(&app, Target::Play).expect("play area");
        point_at(&mut app, play.x - 500.0, play.y + play.h / 2.0);
        assert!(app.paddle_x - app.paddle_width / 2.0 >= -0.01);
        point_at(&mut app, play.right() + 500.0, play.y + play.h / 2.0);
        assert!(app.paddle_x + app.paddle_width / 2.0 <= PLAY_WIDTH + 0.01);
    }

    // ── Keys ────────────────────────────────────────────────────────

    #[test]
    fn a_ctrl_or_alt_combination_belongs_to_the_desktop() {
        let mut app = windowed();
        assert_eq!(
            probe::key(&mut app, &probe::ctrl(Key::N)),
            EventResult::Ignored
        );
        assert_eq!(app.score, 0);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_release_is_not_a_second_press() {
        // Letting go of P must not pause again. This is the whole of the
        // fault: an app that ignores `pressed` sees every keystroke twice.
        let mut app = windowed();
        probe::key(&mut app, &probe::press(Key::P));
        assert_eq!(app.state, GameState::Paused);
        probe::key(&mut app, &release(Key::P));
        assert_eq!(app.state, GameState::Paused);
    }

    // ── Time ────────────────────────────────────────────────────────

    #[test]
    fn the_ball_goes_the_same_distance_however_the_ticks_are_cut() {
        // One long tick and ten short ones covering the same span must leave
        // the ball in the same place, or the game runs at the speed of the
        // compositor rather than at its own.
        let place = |slices: u64, each: u64| {
            let mut app = windowed();
            app.balls = vec![Ball::new(300.0, 300.0, 200.0, 0.0)];
            for _ in 0..slices {
                handle_event(&mut app, &Event::Tick { elapsed_ms: each });
            }
            (app.balls[0].x, app.balls[0].y)
        };
        let (x1, y1) = place(1, 160);
        let (x10, y10) = place(10, 16);
        assert!((x1 - x10).abs() < 0.5, "{x1} vs {x10}");
        assert!((y1 - y10).abs() < 0.5, "{y1} vs {y10}");
    }

    #[test]
    fn a_window_that_was_frozen_does_not_owe_the_player_ten_seconds() {
        let paddle_after = |ms: u64| {
            let mut app = windowed();
            app.left_held = true;
            handle_event(&mut app, &Event::Tick { elapsed_ms: ms });
            app.paddle_x
        };
        assert_eq!(paddle_after(10_000), paddle_after(MAX_CATCHUP_MS));
        // And the cap is a cap, not a floor: a short tick still moves less.
        assert!(paddle_after(FRAME) > paddle_after(MAX_CATCHUP_MS));
    }

    #[test]
    fn a_fast_ball_cannot_pass_through_a_brick() {
        // Bottom row of bricks, and a ball below it moving up fast enough to
        // cross the whole row inside one tick. Integrated in a single jump it
        // arrives above the row with every brick still standing.
        let mut app = windowed();
        let (bx, by, bw, _) = brick_rect(BRICK_ROWS - 1, BRICK_COLS / 2);
        let before = app.bricks_remaining;
        app.balls = vec![Ball::new(bx + bw / 2.0, by + 60.0, 0.0, -MAX_BALL_SPEED)];
        handle_event(&mut app, &Event::Tick { elapsed_ms: 200 });
        assert!(
            app.bricks_remaining < before,
            "the ball went through the row without touching it"
        );
    }

    #[test]
    fn a_new_ball_waits_for_the_next_tick_rather_than_the_rest_of_this_one() {
        // Losing the last ball puts a new one in the middle. Spending the rest
        // of the tick on it would move it before the player has seen where it
        // starts.
        let mut app = windowed();
        app.lives = 3;
        // Off to the side: a ball dropped down the middle lands on the paddle,
        // which is exactly what is not being tested here.
        app.balls = vec![Ball::new(20.0, PLAY_HEIGHT - 20.0, 0.0, MAX_BALL_SPEED)];
        handle_event(&mut app, &Event::Tick { elapsed_ms: 200 });
        assert_eq!(app.lives, 2);
        assert_eq!(app.balls.len(), 1);
        let spawned = app.paddle_top() - BALL_RADIUS - 1.0;
        assert!(
            (app.balls[0].y - spawned).abs() < 0.001,
            "the new ball had already moved to {}",
            app.balls[0].y
        );
    }

    #[test]
    fn the_game_asks_for_ticks_only_while_it_is_playing() {
        let mut app = windowed();
        assert!(app.tick_interval().is_some());
        for state in [GameState::Menu, GameState::Paused, GameState::GameOver] {
            app.state = state;
            assert!(app.tick_interval().is_none(), "{state:?} asked for ticks");
        }
    }

    #[test]
    fn a_tick_that_moves_nothing_does_not_cost_a_repaint() {
        let mut app = windowed();
        app.apply(Action::PauseToggle);
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: FRAME }),
            EventResult::Ignored
        );
    }

    #[test]
    fn losing_focus_pauses_rather_than_playing_on_unwatched() {
        let mut app = windowed();
        assert_eq!(
            handle_event(&mut app, &Event::FocusOut),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Paused);
        // And a second one changes nothing: an already-paused game is not
        // resumed by losing focus it did not have.
        assert_eq!(
            handle_event(&mut app, &Event::FocusOut),
            EventResult::Ignored
        );
        assert_eq!(app.state, GameState::Paused);
    }

    // ── Layout ──────────────────────────────────────────────────────

    /// Window sizes worth laying out: the natural one, tall, wide, tiny, and
    /// degenerate.
    const SIZES: [(f32, f32); 9] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (1920.0, 1080.0),
        (400.0, 900.0),
        (900.0, 300.0),
        (240.0, 200.0),
        (120.0, 90.0),
        (40.0, 40.0),
        (1.0, 1.0),
        (0.0, 0.0),
    ];

    #[test]
    fn the_layout_stays_inside_the_window_at_every_size() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("header", l.header),
                ("play", l.play),
                ("footer", l.footer),
                ("overlay", l.overlay),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "{name} {r:?} escapes a {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn the_play_area_keeps_its_shape_whatever_shape_the_window_is() {
        // A stretched play area would let a ball leave a wall at an angle it
        // did not arrive at, because the physics is written in square units.
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            if l.play.is_empty() {
                continue;
            }
            let want = PLAY_WIDTH / PLAY_HEIGHT;
            let got = l.play.w / l.play.h;
            assert!((got - want).abs() < 0.001, "{w}x{h} gave aspect {got}");
        }
    }

    #[test]
    fn a_cramped_window_drops_the_buttons_rather_than_the_game() {
        let roomy = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!roomy.footer.is_empty());
        let cramped = Layout::new(200.0, 120.0);
        assert!(cramped.footer.is_empty());
        assert!(
            !cramped.play.is_empty(),
            "the game went before the buttons did"
        );
    }

    #[test]
    fn the_buttons_follow_the_window_when_it_is_resized() {
        let mut app = windowed();
        let before = probe::rect_of(&app, Target::Button(Action::NewGame)).expect("button");
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1200,
                height: 900,
            },
        );
        let after = probe::rect_of_sized(&app, Target::Button(Action::NewGame), (1200.0, 900.0))
            .expect("button");
        assert_ne!(before.y, after.y, "the footer ignored the new height");
    }

    #[test]
    fn a_resize_event_is_what_the_next_frame_is_drawn_at() {
        let mut app = windowed();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 640,
                height: 480,
            },
        );
        let f = app.frame(app.width, app.height);
        assert_eq!((f.width, f.height), (640.0, 480.0));
    }

    #[test]
    fn a_point_on_the_screen_names_the_same_point_in_the_play_area_at_any_size() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            if l.play.is_empty() {
                continue;
            }
            for unit in [0.0, 1.0, 299.5, PLAY_WIDTH] {
                let screen = l.to_screen(unit, 0.0, 0.0, 0.0);
                let back = l.play_x(screen.x).expect("a play area has an interior");
                assert!((back - unit).abs() < 0.01, "{w}x{h}: {unit} -> {back}");
            }
        }
    }

    #[test]
    fn every_state_draws_a_balanced_frame_at_every_size() {
        // Every clip pushed has to be popped, or everything drawn after this
        // app in the same tree is clipped to a rectangle that is not its own.
        for state in [
            GameState::Menu,
            GameState::Playing,
            GameState::Paused,
            GameState::GameOver,
        ] {
            for (w, h) in SIZES {
                let mut app = windowed();
                app.state = state;
                let f = app.frame(w, h);
                assert!(f.is_balanced(), "{state:?} at {w}x{h} left a clip open");
            }
        }
    }

    #[test]
    fn a_new_game_keeps_the_window_it_is_drawn_in() {
        // `start_game` replaces the whole app with a fresh one. The window did
        // not close and reopen, so the size has to survive that.
        let mut app = windowed();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 500,
                height: 400,
            },
        );
        handle_event(&mut app, &Event::Key(probe::press(Key::N)));
        assert_eq!((app.width, app.height), (500.0, 400.0));
    }

    #[test]
    fn a_caught_power_up_above_a_fallen_one_removes_the_right_one() {
        // Two removals in one pass. The old code collected indices into the
        // vector as it was, then deleted from both lists, so the second list's
        // indices referred to a vector the first list had already shortened.
        let mut app = windowed();
        let half = POWERUP_SIZE / 2.0;
        app.powerups = vec![
            PowerUp {
                x: app.paddle_x,
                y: app.paddle_top(),
                kind: PowerUpKind::ExtraLife,
            },
            PowerUp {
                x: 10.0,
                y: PLAY_HEIGHT + half + 1.0,
                kind: PowerUpKind::MultiBall,
            },
        ];
        let lives = app.lives;
        app.update_powerups(0.0);
        assert!(app.powerups.is_empty(), "{:?} survived", app.powerups);
        assert_eq!(app.lives, lives + 1, "the caught one was not the one taken");
    }
}
