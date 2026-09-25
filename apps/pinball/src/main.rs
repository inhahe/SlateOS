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

//! Slate OS Pinball — classic pinball arcade game.
//!
//! Features a tall vertical playfield with two flippers (left/right),
//! a spring-loaded plunger/launcher, circular bumpers with bounce physics,
//! drop targets, a ramp, and a drain. Ball physics use position/velocity/gravity
//! simulation with collision detection against flippers, bumpers, walls,
//! targets, and ramps. Scoring awards 100 for bumper hits, 500 for target
//! hits, and 1000 for ramp completions, with combo multipliers. Players
//! get 3 balls per game (extra balls from score milestones). Includes
//! nudging and tilt, multi-ball, variable-power ball launching, and a table of
//! high scores kept between games in the settings directory.
//!
//! Played with the keys (F1 lists them) or the pointer: hold on the table's
//! left or right half for that flipper, hold on the plunger lane to pull the
//! plunger and let go to launch, and the sidebar's buttons start, pause and
//! nudge. Nothing answered the pointer before 2026-09-25; "tilt" was pressing
//! the flippers fast, which is playing rather than cheating; the high-score
//! table was five scores nobody had made, forgotten when the window closed; and
//! N threw away a game in progress without asking (`known-issues.md` ->
//! `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).

use appearance::Palette;
use guitk::color::Color;
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use randrange::{RandomSource, SeededRng, seeded_from_system};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
/// How often the table is asked to advance itself.
///
/// 16 ms is one frame at 60 Hz. A pinball table is a physics simulation:
/// `handle_tick` integrates the ball by the elapsed milliseconds, so a longer
/// tick is not merely choppier, it is a coarser integration step and a fast
/// ball can pass through a flipper between two samples.
const TICK: Duration = Duration::from_millis(16);

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
/// How far back nudges are counted towards a tilt.
const TILT_WINDOW_MS: u64 = 5000;
/// How many nudges within the window are warnings. One more tilts the table.
const TILT_WARNINGS: usize = 2;
/// How long a warning's DANGER stays on the sidebar.
const DANGER_MS: u64 = 1500;
/// How hard a nudge shoves each ball up the table (pixels per second).
const NUDGE_IMPULSE: f32 = 160.0;
/// And at most how hard sideways, one way or the other.
const NUDGE_SIDEWAYS: f32 = 60.0;
/// The first line of the high-score file, naming its format.
const SCORES_HEADER: &str = "pinball high scores 1";
/// The largest high-score file read. Five lines are a hundred bytes; this is
/// room for a file somebody has edited and then some.
const MAX_SCORES_BYTES: usize = 64 * 1024;

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

// ── Randomness ─────────────────────────────────────────────────────

/// The seed a table falls back to when the kernel has no entropy to give.
///
/// Unlike a password, a pinball table may be predictable: the worst outcome is
/// that multiball throws its extra balls the same way it did last time. A
/// machine that refused to start because the entropy source was down would be
/// the worse failure, so this is a deliberate exception to the fail-closed rule
/// [`randrange::SecretSource`] exists to enforce. "PINBALL!" in ASCII.
const FALLBACK_SEED: u64 = 0x5049_4E42_414C_4C21;

/// A generator for one session's tables, from the kernel where possible.
///
/// The body of this used to live here; it now lives in
/// [`randrange::seeded_from_system`], because sixteen crates had written it.
fn session_rng() -> SeededRng {
    seeded_from_system(FALLBACK_SEED)
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
        Vec2::new(TABLE_WIDTH - PLUNGER_LANE_WIDTH / 2.0, TABLE_HEIGHT - 40.0)
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
#[derive(Clone, Debug, PartialEq, Eq)]
struct HighScoreEntry {
    score: u32,
    /// The day it was made, `YYYY-MM-DD` in UTC, or empty if the clock was not
    /// set.
    day: String,
}

// ── Tilt tracker ────────────────────────────────────────────────────
/// Shoving the table, and what shoving it too much does.
///
/// A nudge moves the ball; a nudge too many -- more than [`TILT_WARNINGS`]
/// within [`TILT_WINDOW_MS`] -- tilts the table, as a real machine's tilt bob
/// does: the flippers go dead and nothing scores until the ball drains.
///
/// It used to count *flipper presses*, fifteen in a second, and dead the
/// flippers for three seconds. Pressing the flippers quickly is playing the
/// game, not cheating at it, and there was no way to shove the table at all.
#[derive(Clone, Debug)]
struct TiltTracker {
    /// When each recent nudge came, in total ms.
    nudges: Vec<u64>,
    /// Whether the table is tilted, until the ball drains.
    tilted: bool,
    /// When the last warning came, for the sidebar's DANGER.
    warned_ms: Option<u64>,
}

/// What a nudge came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nudge {
    /// The table moved; this many nudges are now counted against it.
    Warned(usize),
    /// That was one too many.
    Tilted,
    /// Already tilted: the table is not listening.
    Ignored,
}

impl TiltTracker {
    fn new() -> Self {
        Self {
            nudges: Vec::new(),
            tilted: false,
            warned_ms: None,
        }
    }

    /// A new ball: a clean slate.
    fn reset(&mut self) {
        self.nudges.clear();
        self.tilted = false;
        self.warned_ms = None;
    }

    /// A nudge at `now`.
    fn nudge(&mut self, now: u64) -> Nudge {
        if self.tilted {
            return Nudge::Ignored;
        }
        self.nudges
            .retain(|&t| now.saturating_sub(t) < TILT_WINDOW_MS);
        self.nudges.push(now);
        if self.nudges.len() > TILT_WARNINGS {
            self.tilted = true;
            Nudge::Tilted
        } else {
            self.warned_ms = Some(now);
            Nudge::Warned(self.nudges.len())
        }
    }

    /// Whether the DANGER warning is showing at `now`.
    fn danger(&self, now: u64) -> bool {
        !self.tilted
            && self
                .warned_ms
                .is_some_and(|at| now.saturating_sub(at) < DANGER_MS)
    }
}

/// What the pointer is holding down, to be let go when the button comes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hold {
    LeftFlipper,
    RightFlipper,
    Plunger,
}

/// What a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The table's left half: the left flipper, while the button is held.
    LeftFlipper,
    /// The table's right half.
    RightFlipper,
    /// The plunger lane before a launch: hold to pull, let go to launch.
    Plunger,
    /// The sidebar's buttons.
    NewGame,
    Pause,
    Nudge,
    Keys,
    /// The pause screen's button.
    Resume,
    /// The game-over screen's button.
    PlayAgain,
    /// The question before a game in progress is thrown away.
    ConfirmNewGame,
    KeepPlaying,
    /// The shortcut card, anywhere on it: closes it.
    HelpCard,
}

/// Every key the game answers, as the card shows them.
///
/// The flippers answer the Shift keys as well; the card names the letters,
/// which every keyboard has in the same place.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1", "This list"),
    ("Z", "Left flipper (or Left Shift)"),
    ("M", "Right flipper (or Right Shift)"),
    ("Space", "Hold to pull the plunger, let go to launch"),
    ("Up", "Nudge the table -- too often and it tilts"),
    ("P", "Pause, and carry on"),
    ("N", "New game (asks first during a game)"),
    ("Esc", "Close this list, or keep playing"),
];

/// The wall clock, in seconds since the epoch, or `None` if it is not set.
fn system_clock() -> Option<i64> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

/// Where the high scores are kept.
fn scores_path() -> Option<PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("pinball").join("high-scores.txt"))
}

/// The high-score table as it is written: a line naming the format, then one
/// line a score -- the score, a tab, and the day.
fn scores_text(scores: &[HighScoreEntry]) -> String {
    let mut text = String::from(SCORES_HEADER);
    text.push('\n');
    for entry in scores {
        text.push_str(&format!("{}\t{}\n", entry.score, entry.day));
    }
    text
}

/// Read a high-score file whole, or say which line is wrong.
///
/// All or nothing: a file with one line not understood is not read at all,
/// because the next game over saves the table -- and a table saved from a
/// partial read deletes the lines that were not understood.
fn parse_scores(text: &str) -> Result<Vec<HighScoreEntry>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(SCORES_HEADER) {
        return Err(String::from("its first line does not name this format"));
    }
    let mut scores = Vec::new();
    for (n, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        let number = n.saturating_add(2);
        let (score, day) = line
            .split_once('\t')
            .ok_or_else(|| format!("line {number} has no tab"))?;
        let score = score
            .parse::<u32>()
            .map_err(|_| format!("line {number}: {score:?} is not a score"))?;
        let day_ok = day.is_empty()
            || (day.len() == 10
                && day.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                }));
        if !day_ok {
            return Err(format!("line {number}: {day:?} is not a day"));
        }
        scores.push(HighScoreEntry {
            score,
            day: day.to_string(),
        });
    }
    if scores.len() > HIGH_SCORE_SLOTS {
        return Err(format!(
            "it holds {} scores, not at most {HIGH_SCORE_SLOTS}",
            scores.len()
        ));
    }
    Ok(scores)
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
    /// Time of last scoring event for combo tracking. `None` means no scoring
    /// event has occurred yet, so the next hit starts a fresh combo (x1).
    last_score_ms: Option<u64>,
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
    rng: SeededRng,
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
    /// Whether the high scores are read from and written to their file.
    /// Off in [`Pinball::new`], so a test's table touches no file;
    /// [`Pinball::from_settings`] turns it on.
    persist: bool,
    /// Why the high scores are not being kept, when they are not.
    scores_note: Option<String>,
    /// Where the day a score was made comes from.
    clock: fn() -> Option<i64>,
    /// Whether the list of keys is up.
    show_help: bool,
    /// Whether N, in the middle of a game, is waiting for a second N.
    confirm_new_game: bool,
    /// What the pointer is holding down.
    held: Option<Hold>,
    /// The size of the window being drawn in.
    window: (f32, f32),
    /// The user's colours, for the card; the table keeps its own.
    palette: Palette,
}

impl Pinball {
    fn new() -> Self {
        // Not a fixed seed: with one, every session's first table threw its
        // multiball the same way, and only the *second* game of a session --
        // which reseeds from the first -- ever differed.
        Self::with_rng(session_rng())
    }

    /// A table driven by `rng`.
    ///
    /// Taking the generator rather than a seed is what lets [`new`](Self::new)
    /// draw from the kernel while a test drives the same table from a seed it
    /// chose. Neither has to know where the other's bits came from.
    fn with_rng(rng: SeededRng) -> Self {
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

        let ramp = Ramp::new(Vec2::new(60.0, 350.0), Vec2::new(40.0, 100.0), 40.0);

        let mut app = Self {
            balls: Vec::new(),
            left_flipper: Flipper::new(FlipperSide::Left, Vec2::new(LEFT_FLIPPER_X, FLIPPER_Y)),
            right_flipper: Flipper::new(FlipperSide::Right, Vec2::new(RIGHT_FLIPPER_X, FLIPPER_Y)),
            bumpers,
            targets,
            ramp,
            phase: GamePhase::ReadyToLaunch,
            score: 0,
            balls_remaining: STARTING_BALLS,
            balls_used: 0,
            // Empty until somebody plays. It was five invented scores --
            // 10000 down to 1000 -- which the table presented as the best
            // games played on this machine.
            high_scores: Vec::new(),
            combo: 1,
            last_score_ms: None,
            total_ms: 0,
            launch_power: 0.0,
            multi_ball_active: false,
            tilt: TiltTracker::new(),
            extra_balls_earned: 0,
            rng,
            ball_lost_ms: 0,
            phase_before_pause: GamePhase::ReadyToLaunch,
            total_bumper_hits: 0,
            total_target_hits: 0,
            total_ramp_completions: 0,
            persist: false,
            scores_note: None,
            clock: system_clock,
            show_help: false,
            confirm_new_game: false,
            held: None,
            window: (WINDOW_WIDTH, WINDOW_HEIGHT),
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
        };
        app.prepare_ball();
        app
    }

    /// The table the window plays: its high scores read from, and kept in,
    /// the settings directory.
    fn from_settings() -> Self {
        let mut app = Self::new();
        app.persist = true;
        match scores_path() {
            Some(path) => app.load_scores(&path),
            None => {
                app.persist = false;
                app.scores_note = Some(String::from("No home directory: scores are not kept."));
            }
        }
        app
    }

    /// Read the high scores at `path`; none there yet is a first game.
    ///
    /// A file that cannot be read whole is left exactly as it is, and the
    /// sidebar says so: saving over it would keep only what was understood.
    fn load_scores(&mut self, path: &Path) {
        let refused =
            |why: String| format!("{} not read ({why}); scores are not kept.", path.display());
        let read = match safeio::read_to_string_capped(path, MAX_SCORES_BYTES) {
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                self.persist = false;
                self.scores_note = Some(refused(err.to_string()));
                return;
            }
        };
        if read.truncated {
            self.persist = false;
            self.scores_note = Some(refused(String::from("it is too large")));
            return;
        }
        match parse_scores(&read.text) {
            Ok(scores) => self.high_scores = scores,
            Err(why) => {
                self.persist = false;
                self.scores_note = Some(refused(why));
            }
        }
    }

    /// Keep the high scores, if they are being kept.
    fn save_scores(&mut self) {
        if !self.persist {
            return;
        }
        let Some(path) = scores_path() else {
            return;
        };
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| safeio::write_str_atomically(&path, &scores_text(&self.high_scores)));
        self.scores_note = match written {
            Ok(()) => None,
            Err(err) => Some(format!("Scores not saved to {}: {err}", path.display())),
        };
    }

    /// Today, `YYYY-MM-DD` in UTC, or empty if the clock is not set.
    fn today(&self) -> String {
        (self.clock)().map_or_else(String::new, |secs| {
            let (y, m, d) = guitk::date::Date::from_unix_utc(secs).ymd();
            format!("{y:04}-{m:02}-{d:02}")
        })
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
        self.balls_used = self.balls_used.saturating_add(1);
        self.phase = GamePhase::Playing;
        self.launch_power = 0.0;
    }

    /// Start a new game, keeping everything that is not the game's: the high
    /// scores and where they are kept, the clock, the window, the colours.
    fn new_game(&mut self) {
        // Carry the generator into the next table rather than reseeding one
        // from the other's output. Same effect on the stream, and it cannot
        // accidentally hand two tables in a session the same seed.
        let mut fresh = Self::with_rng(self.rng.clone());
        fresh.high_scores = std::mem::take(&mut self.high_scores);
        fresh.persist = self.persist;
        fresh.scores_note = self.scores_note.take();
        fresh.clock = self.clock;
        fresh.window = self.window;
        fresh.palette = self.palette;
        *self = fresh;
    }

    /// Whether a game is under way that N would throw away.
    fn game_in_progress(&self) -> bool {
        self.phase != GamePhase::GameOver && (self.score > 0 || self.balls_used > 0)
    }

    /// N: a new game -- at once between games, and after a second N during
    /// one. It started a new game at once whatever was happening, so a stray
    /// key ended a game in progress without a word.
    fn ask_new_game(&mut self) {
        if !self.game_in_progress() || self.confirm_new_game {
            self.confirm_new_game = false;
            self.new_game();
            return;
        }
        self.confirm_new_game = true;
        self.release_all();
    }

    /// Nudge the table: the balls get a shove, and too many shoves tilt it.
    fn nudge(&mut self) -> Nudge {
        if self.phase != GamePhase::Playing {
            return Nudge::Ignored;
        }
        let outcome = self.tilt.nudge(self.total_ms);
        if outcome == Nudge::Ignored {
            return outcome;
        }
        for ball in self.balls.iter_mut().filter(|b| b.active) {
            let sideways = if self.rng.flip() {
                NUDGE_SIDEWAYS
            } else {
                -NUDGE_SIDEWAYS
            };
            ball.vel = ball.vel.add(Vec2::new(sideways, -NUDGE_IMPULSE));
        }
        if outcome == Nudge::Tilted {
            // The flippers go dead at once, held or not.
            self.left_flipper.pressed = false;
            self.right_flipper.pressed = false;
        }
        outcome
    }

    /// Press or let go of a flipper. A tilted table's flippers are dead.
    fn set_flipper(&mut self, side: FlipperSide, pressed: bool) {
        let pressed = pressed && !self.tilt.tilted;
        match side {
            FlipperSide::Left => self.left_flipper.pressed = pressed,
            FlipperSide::Right => self.right_flipper.pressed = pressed,
        }
    }

    /// Start pulling the plunger, if there is a ball to launch.
    fn pull_plunger(&mut self) {
        if self.phase == GamePhase::ReadyToLaunch {
            self.phase = GamePhase::Launching;
            self.launch_power = 0.0;
        }
    }

    /// Let the plunger go.
    fn release_plunger(&mut self) {
        if self.phase == GamePhase::Launching {
            self.launch_ball();
        }
    }

    /// Let go of everything held -- for a window that has lost the keyboard or
    /// the pointer, whose releases will never arrive.
    fn release_all(&mut self) {
        self.left_flipper.pressed = false;
        self.right_flipper.pressed = false;
        if self.phase == GamePhase::Launching {
            // Not launched: a plunger let go because the window went away is
            // not a shot the player took.
            self.phase = GamePhase::ReadyToLaunch;
            self.launch_power = 0.0;
        }
        self.held = None;
    }

    /// P: pause, or carry on.
    fn toggle_pause(&mut self) {
        match self.phase {
            GamePhase::Paused => self.phase = self.phase_before_pause,
            GamePhase::GameOver => {}
            _ => {
                // Let go first: a plunger half pulled goes back to rest, so
                // carrying on does not launch a ball nobody is holding.
                self.release_all();
                self.phase_before_pause = self.phase;
                self.phase = GamePhase::Paused;
            }
        }
    }

    /// Award points with combo multiplier. A tilted table scores nothing.
    fn award_points(&mut self, base_points: u32) {
        if self.tilt.tilted {
            return;
        }
        // Update combo. The first scoring event (or one after the combo window
        // has lapsed) starts a fresh combo at x1; a hit within the window of the
        // previous one increments the multiplier.
        match self.last_score_ms {
            Some(prev) if self.total_ms.saturating_sub(prev) <= COMBO_WINDOW_MS => {
                self.combo = self.combo.saturating_add(1).min(MAX_COMBO);
            }
            _ => {
                self.combo = 1;
            }
        }
        self.last_score_ms = Some(self.total_ms);

        let points = base_points.saturating_mul(self.combo);
        self.score = self.score.saturating_add(points);

        // Check for extra ball milestones.
        let milestone = self
            .extra_balls_earned
            .saturating_add(1)
            .saturating_mul(EXTRA_BALL_SCORE);
        if self.score >= milestone {
            self.extra_balls_earned = self.extra_balls_earned.saturating_add(1);
            self.balls_remaining = self.balls_remaining.saturating_add(1);
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
        let positions = [Vec2::new(100.0, 200.0), Vec2::new(180.0, 250.0)];
        for &pos in &positions {
            let mut ball = Ball::new(pos);
            ball.active = true;
            let vx = if self.rng.flip() { 80.0 } else { -80.0 };
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

    /// Insert the current score into the high score table if it qualifies,
    /// and keep the table.
    ///
    /// A game that scored nothing is not a high score, even on an empty table.
    fn update_high_scores(&mut self) {
        let score = self.score;
        if score == 0 {
            return;
        }
        let entry = HighScoreEntry {
            score,
            day: self.today(),
        };
        let pos = self.high_scores.iter().position(|h| score > h.score);
        if let Some(idx) = pos {
            self.high_scores.insert(idx, entry);
            self.high_scores.truncate(HIGH_SCORE_SLOTS);
        } else if self.high_scores.len() < HIGH_SCORE_SLOTS {
            self.high_scores.push(entry);
        } else {
            return;
        }
        self.save_scores();
    }

    // ── Physics ─────────────────────────────────────────────────────

    /// Run one physics step for all balls.
    fn physics_step(&mut self, dt: f32) {
        // Update flippers.
        self.left_flipper.update(dt);
        self.right_flipper.update(dt);

        let ball_count = self.balls.len();
        for i in 0..ball_count {
            // The integration step belongs to one ball, so it borrows that
            // ball once rather than subscripting it six times. A ball that is
            // not there is skipped for the same reason an inactive one is:
            // `ball_count` was read before the loop and a collision can drain
            // a ball, so the index is a claim about the past.
            let Some(ball) = self.balls.get_mut(i) else {
                continue;
            };
            if !ball.active {
                continue;
            }

            // Apply gravity.
            ball.vel.y += GRAVITY * dt;

            // Apply friction.
            ball.vel = ball.vel.scale(FRICTION);

            // Clamp speed.
            ball.vel = ball.vel.clamp_magnitude(MAX_BALL_SPEED);

            // Update position.
            ball.pos = ball.pos.add(ball.vel.scale(dt));

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
        for (i, ball) in self.balls.iter().enumerate() {
            if ball.pos.y > TABLE_HEIGHT + BALL_RADIUS * 2.0 {
                drained.push(i);
            }
        }
        for &idx in drained.iter().rev() {
            self.drain_ball(idx);
        }
    }

    /// Collide ball with table walls.
    fn collide_walls(&mut self, idx: usize) {
        let Some(ball) = self.balls.get_mut(idx) else {
            return;
        };

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
        let Some((ball_pos, ball_vel)) = self.balls.get(idx).map(|b| (b.pos, b.vel)) else {
            return;
        };

        for bumper in &mut self.bumpers {
            let diff = ball_pos.sub(bumper.pos);
            let dist = diff.length();
            let min_dist = BALL_RADIUS + bumper.radius;

            if dist < min_dist && dist > 0.01 {
                // Push ball out of bumper.
                let normal = diff.normalized();
                if let Some(ball) = self.balls.get_mut(idx) {
                    ball.pos = bumper.pos.add(normal.scale(min_dist + 0.5));
                }

                // Reflect velocity and boost.
                let reflected = ball_vel.reflect(normal);
                if let Some(ball) = self.balls.get_mut(idx) {
                    ball.vel = reflected.scale(BUMPER_RESTITUTION);
                }

                bumper.last_hit_ms = self.total_ms;
                bumper.hit_count = bumper.hit_count.saturating_add(1);
            }
        }

        // Award points for bumper hits (check if any bumper was just hit this frame).
        let current_ms = self.total_ms;
        let bumper_hit_count = self
            .bumpers
            .iter()
            .filter(|b| b.last_hit_ms == current_ms)
            .count();
        for _ in 0..bumper_hit_count {
            self.award_points(BUMPER_POINTS);
            self.total_bumper_hits = self.total_bumper_hits.saturating_add(1);
        }
    }

    /// Collide ball with drop targets.
    fn collide_targets(&mut self, idx: usize) {
        let Some(ball_pos) = self.balls.get(idx).map(|b| b.pos) else {
            return;
        };

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

                // Bounce ball away from target. One lookup, with the side
                // chosen first: the two arms differed only in the sign.
                let above = ball_pos.y < target.pos.y;
                if let Some(ball) = self.balls.get_mut(idx) {
                    let speed = ball.vel.y.abs() * WALL_RESTITUTION;
                    ball.vel.y = if above { -speed } else { speed };
                }
            }
        }

        if hit_any {
            self.award_points(TARGET_POINTS);
            self.total_target_hits = self.total_target_hits.saturating_add(1);

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

        let Some(ball_pos) = self.balls.get(idx).map(|b| b.pos) else {
            return;
        };
        let closest = flipper.closest_point(ball_pos);
        let diff = ball_pos.sub(closest);
        let dist = diff.length();
        let min_dist = BALL_RADIUS + FLIPPER_WIDTH / 2.0;

        if dist < min_dist && dist > 0.01 {
            let normal = diff.normalized();

            // Push ball out of flipper.
            if let Some(ball) = self.balls.get_mut(idx) {
                ball.pos = closest.add(normal.scale(min_dist + 0.5));
            }

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
                if let Some(ball) = self.balls.get_mut(idx) {
                    ball.vel = Vec2::new(base_angle.cos() * speed, base_angle.sin() * speed);
                }
            } else {
                // Passive flipper: just bounce.
                if let Some(ball) = self.balls.get_mut(idx) {
                    ball.vel = ball.vel.reflect(normal).scale(WALL_RESTITUTION);
                }
            }
        }
    }

    /// Check if ball enters the ramp.
    fn check_ramp(&mut self, idx: usize) {
        let Some(ball) = self.balls.get(idx) else {
            return;
        };
        if self.ramp.ball_entering(ball.pos, ball.vel) {
            // Teleport ball to ramp exit with a kick.
            let exit = self.ramp.exit;
            if let Some(ball) = self.balls.get_mut(idx) {
                ball.pos = exit;
                ball.vel = Vec2::new(80.0, 50.0);
            }
            self.award_points(RAMP_POINTS);
            self.total_ramp_completions = self.total_ramp_completions.saturating_add(1);
        }
    }

    /// Check if ball has drained.
    fn check_drain(&mut self, idx: usize) {
        // Drain zone is below the flipper area, between the side gutters.
        let Some(ball) = self.balls.get(idx) else {
            return;
        };
        // The drain is at the very bottom center of the table.
        if ball.pos.y > TABLE_HEIGHT - 20.0
            && ball.pos.x > 30.0
            && ball.pos.x < TABLE_WIDTH - PLUNGER_LANE_WIDTH - 30.0
        {
            // Mark for drain (will be processed after physics loop).
            if let Some(ball) = self.balls.get_mut(idx) {
                ball.pos.y = TABLE_HEIGHT + BALL_RADIUS * 3.0;
            }
        }
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
                EventResult::Consumed
            }
            Event::Resize { width, height } => {
                self.window = (*width as f32, *height as f32);
                EventResult::Consumed
            }
            // A game that goes on while its window is behind another loses
            // the ball with nobody watching; and a flipper held when the
            // window lost the keyboard never hears its key come up.
            Event::FocusOut => {
                if matches!(
                    self.phase,
                    GamePhase::Playing | GamePhase::Launching | GamePhase::ReadyToLaunch
                ) && self.balls_used > 0
                {
                    self.toggle_pause();
                } else {
                    self.release_all();
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, ke: &KeyEvent) -> EventResult {
        // The flippers and the plunger answer the key coming up as well as
        // going down; everything else only its press.
        if ke.key == Key::F1 {
            if ke.pressed {
                self.show_help = !self.show_help;
                self.release_all();
            }
            return EventResult::Consumed;
        }
        if self.show_help {
            // The list is up: a flipper pressed through it would move a
            // flipper nobody can see.
            if ke.pressed && matches!(ke.key, Key::Escape | Key::Enter) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        if self.confirm_new_game {
            if ke.pressed {
                match ke.key {
                    Key::N | Key::Enter => self.ask_new_game(),
                    Key::Escape => self.confirm_new_game = false,
                    _ => {}
                }
            }
            return EventResult::Consumed;
        }
        match ke.key {
            // Left flipper: Left Shift or Z.
            Key::LeftShift | Key::Z => self.set_flipper(FlipperSide::Left, ke.pressed),
            // Right flipper: Right Shift or M.
            Key::RightShift | Key::M => self.set_flipper(FlipperSide::Right, ke.pressed),
            // Space: hold to pull the plunger, let go to launch.
            Key::Space => {
                if ke.pressed {
                    self.pull_plunger();
                } else {
                    self.release_plunger();
                }
            }
            Key::Up if ke.pressed => {
                self.nudge();
            }
            Key::N if ke.pressed => self.ask_new_game(),
            Key::P if ke.pressed => self.toggle_pause(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// The pointer: the table's halves are the flippers, the plunger lane the
    /// plunger, and the buttons are buttons.
    fn handle_mouse(&mut self, me: &MouseEvent) -> EventResult {
        match me.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                match self.frame_at(self.window).hit_test(me.x, me.y) {
                    Some(target) => {
                        self.press(target);
                        EventResult::Consumed
                    }
                    None => EventResult::Ignored,
                }
            }
            MouseEventKind::Release(MouseButton::Left) => match self.held.take() {
                Some(Hold::LeftFlipper) => {
                    self.set_flipper(FlipperSide::Left, false);
                    EventResult::Consumed
                }
                Some(Hold::RightFlipper) => {
                    self.set_flipper(FlipperSide::Right, false);
                    EventResult::Consumed
                }
                Some(Hold::Plunger) => {
                    self.release_plunger();
                    EventResult::Consumed
                }
                None => EventResult::Ignored,
            },
            _ => EventResult::Ignored,
        }
    }

    /// A left press on `target`.
    fn press(&mut self, target: Target) {
        match target {
            Target::LeftFlipper => {
                self.set_flipper(FlipperSide::Left, true);
                self.held = Some(Hold::LeftFlipper);
            }
            Target::RightFlipper => {
                self.set_flipper(FlipperSide::Right, true);
                self.held = Some(Hold::RightFlipper);
            }
            Target::Plunger => {
                self.pull_plunger();
                self.held = Some(Hold::Plunger);
            }
            Target::NewGame | Target::ConfirmNewGame | Target::PlayAgain => self.ask_new_game(),
            Target::KeepPlaying => self.confirm_new_game = false,
            Target::Pause | Target::Resume => self.toggle_pause(),
            Target::Nudge => {
                self.nudge();
            }
            Target::Keys => self.show_help = true,
            Target::HelpCard => self.show_help = false,
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) {
        self.total_ms = self.total_ms.saturating_add(elapsed_ms);
        let dt = elapsed_ms as f32 / 1000.0;

        match self.phase {
            GamePhase::Launching => {
                self.launch_power =
                    (self.launch_power + LAUNCH_POWER_RATE * dt / MAX_LAUNCH_POWER).min(1.0);
                // The flippers work before the ball is in play too; they only
                // moved inside the physics step, so a flipper pressed while
                // the ball sat in the plunger lane did not move.
                self.left_flipper.update(dt);
                self.right_flipper.update(dt);
            }
            GamePhase::ReadyToLaunch => {
                self.left_flipper.update(dt);
                self.right_flipper.update(dt);
            }
            GamePhase::Playing => {
                // Substep for stability.
                let steps = 4;
                let sub_dt = dt / steps as f32;
                for _ in 0..steps {
                    self.physics_step(sub_dt);
                }
            }
            GamePhase::BallLost
                // Wait a moment before preparing the next ball.
                if self.total_ms.saturating_sub(self.ball_lost_ms) > 1500 => {
                    self.prepare_ball();
                }
            _ => {}
        }
    }

    /// Whether anything on the table moves by itself: the ball, a flipper on
    /// its way, the plunger being pulled, the pause after a lost ball, a
    /// warning or a bumper's flash fading.
    fn moving(&self) -> bool {
        let flipper_moving = |f: &Flipper| f.pressed || f.angle > 0.0;
        matches!(
            self.phase,
            GamePhase::Playing | GamePhase::Launching | GamePhase::BallLost
        ) || flipper_moving(&self.left_flipper)
            || flipper_moving(&self.right_flipper)
            || self.tilt.danger(self.total_ms)
            || self.bumpers.iter().any(|b| b.is_flashing(self.total_ms))
            || self.targets.iter().any(|t| t.is_flashing(self.total_ms))
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// The window at `size`, with every control's hit box.
    ///
    /// The table is drawn at its own size -- every collision bound in the
    /// physics is measured in it -- and a larger window puts it in the middle
    /// rather than in the corner.
    fn frame_at(&self, (width, height): (f32, f32)) -> Frame<Target> {
        let window = Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0));
        let mut f = Frame::new(window.w, window.h);
        f.clip(window);
        fill(&mut f, window, BASE, 0.0);
        let dx = ((window.w - WINDOW_WIDTH) / 2.0).max(0.0).floor();
        let dy = ((window.h - WINDOW_HEIGHT) / 2.0).max(0.0).floor();
        f.translate(dx, dy);

        f.draw_with(|cmds| cmds.extend(self.render_commands()));
        self.hit_table(&mut f);
        self.draw_buttons(&mut f);
        self.draw_overlay_buttons(&mut f);
        if self.confirm_new_game {
            self.draw_confirm(&mut f);
        }
        f.untranslate();

        if self.show_help {
            // Modal: nothing behind the card can be clicked.
            f.discard_hits();
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (window.w, window.h),
                0.0,
                SHORTCUTS,
                "F1 or Esc closes this",
            );
            f.hit(Target::HelpCard, window);
        }
        f.unclip();
        f
    }

    /// The table's halves are the flippers while there is a ball to flip, and
    /// the plunger lane is the plunger while there is one to launch.
    fn hit_table(&self, f: &mut Frame<Target>) {
        let flipping = matches!(
            self.phase,
            GamePhase::Playing
                | GamePhase::ReadyToLaunch
                | GamePhase::Launching
                | GamePhase::BallLost
        );
        if !flipping || self.confirm_new_game {
            return;
        }
        let (tx, ty) = (Self::table_origin_x(), Self::table_origin_y());
        let launching = matches!(self.phase, GamePhase::ReadyToLaunch | GamePhase::Launching);
        let playfield = if launching {
            TABLE_WIDTH - PLUNGER_LANE_WIDTH
        } else {
            TABLE_WIDTH
        };
        let half = playfield / 2.0;
        f.hit(Target::LeftFlipper, Rect::new(tx, ty, half, TABLE_HEIGHT));
        f.hit(
            Target::RightFlipper,
            Rect::new(tx + half, ty, playfield - half, TABLE_HEIGHT),
        );
        if launching {
            f.hit(
                Target::Plunger,
                Rect::new(tx + playfield, ty, PLUNGER_LANE_WIDTH, TABLE_HEIGHT),
            );
        }
    }

    /// The sidebar's buttons, along its bottom.
    fn draw_buttons(&self, f: &mut Frame<Target>) {
        let (sx, sy) = (PADDING, PADDING);
        let sw = SIDEBAR_WIDTH - PADDING;
        let w = (sw - 20.0 - 4.0) / 2.0;
        let row1 = sy + TABLE_HEIGHT - 64.0;
        let row2 = sy + TABLE_HEIGHT - 34.0;
        let (left, right) = (sx + 10.0, sx + 10.0 + w + 4.0);
        let paused = self.phase == GamePhase::Paused;
        button(
            f,
            Rect::new(left, row1, w, 26.0),
            "New game",
            Target::NewGame,
            true,
        );
        button(
            f,
            Rect::new(right, row1, w, 26.0),
            if paused { "Resume" } else { "Pause" },
            Target::Pause,
            self.phase != GamePhase::GameOver,
        );
        button(
            f,
            Rect::new(left, row2, w, 26.0),
            "Nudge",
            Target::Nudge,
            self.phase == GamePhase::Playing && !self.tilt.tilted,
        );
        button(
            f,
            Rect::new(right, row2, w, 26.0),
            "Keys (F1)",
            Target::Keys,
            true,
        );
    }

    /// The pause and game-over screens' buttons, under their words.
    fn draw_overlay_buttons(&self, f: &mut Frame<Target>) {
        if self.confirm_new_game {
            return;
        }
        let (tx, ty) = (Self::table_origin_x(), Self::table_origin_y());
        let r = Rect::new(
            tx + TABLE_WIDTH / 2.0 - 55.0,
            ty + TABLE_HEIGHT / 2.0 + 45.0,
            110.0,
            28.0,
        );
        match self.phase {
            GamePhase::Paused => button(f, r, "Resume", Target::Resume, true),
            GamePhase::GameOver => button(f, r, "Play again", Target::PlayAgain, true),
            _ => {}
        }
    }

    /// The question before N throws a game away.
    fn draw_confirm(&self, f: &mut Frame<Target>) {
        let (tx, ty) = (Self::table_origin_x(), Self::table_origin_y());
        fill(
            f,
            Rect::new(tx, ty, TABLE_WIDTH, TABLE_HEIGHT),
            Color::rgba(0, 0, 0, 170),
            8.0,
        );
        // Modal: the table and the sidebar take no press while it is asked.
        f.discard_hits();
        let card = Rect::new(
            tx + 20.0,
            ty + TABLE_HEIGHT / 2.0 - 70.0,
            TABLE_WIDTH - 40.0,
            140.0,
        );
        fill(f, card, SURFACE0, 8.0);
        centred(
            f,
            Rect::new(card.x, card.y + 12.0, card.w, 24.0),
            "New game?",
            OVERLAY_FONT_SIZE,
            TEXT_COLOR,
        );
        centred(
            f,
            Rect::new(card.x, card.y + 42.0, card.w, 18.0),
            &format!("This one ends, at {}.", self.score),
            LABEL_FONT_SIZE,
            SUBTEXT0,
        );
        let w = (card.w - 36.0) / 2.0;
        let y = card.bottom() - 42.0;
        button(
            f,
            Rect::new(card.x + 12.0, y, w, 28.0),
            "New game (N)",
            Target::ConfirmNewGame,
            true,
        );
        button(
            f,
            Rect::new(card.x + 24.0 + w, y, w, 28.0),
            "Keep playing",
            Target::KeepPlaying,
            true,
        );
    }

    /// Named `render_commands` and not `render`, so that a bare
    /// `self.render_commands()` inside the `App` impl cannot resolve to the trait
    /// method -- which it does even at a different arity, reporting a missing
    /// argument rather than calling this.
    fn render_commands(&self) -> Vec<RenderCommand> {
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
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: sy + 68.0,
            text: format!("{}", self.score),
            color: YELLOW,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Clip,
            });
        }

        // Tilt, and the warnings before it.
        if self.tilt.tilted || self.tilt.danger(self.total_ms) {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: sy + 180.0,
                text: if self.tilt.tilted { "TILT!" } else { "DANGER" }.to_string(),
                color: RED,
                font_size: SCORE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Clip,
            });
        }

        // Targets hit indicator.
        let targets_hit = self.targets.iter().filter(|t| !t.active).count();
        let targets_total = self.targets.len();
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: stats_y + 72.0,
            text: format!("Targets: {}/{}", targets_hit, targets_total),
            color: if targets_hit == targets_total {
                GREEN
            } else {
                SUBTEXT0
            },
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
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
                text: score_line(i, entry),
                color: rank_color,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(sw - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        if self.high_scores.is_empty() {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: hs_y + 20.0,
                text: "No scores yet".to_string(),
                color: OVERLAY0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(sw - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        // Why the scores are not kept, whole: a clipped reason is no reason.
        if let Some(note) = &self.scores_note {
            let note_y = hs_y + 20.0 + HIGH_SCORE_SLOTS as f32 * 18.0;
            let lines =
                guitk::text::wrap(note, sw - 20.0, FOOTER_FONT_SIZE, FontWeightHint::Regular);
            for (n, line) in lines.iter().take(3).enumerate() {
                cmds.push(RenderCommand::Text {
                    x: sx + 10.0,
                    y: note_y + n as f32 * 13.0,
                    text: line.clone(),
                    color: PEACH,
                    font_size: FOOTER_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(sw - 20.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // Launch power bar (when launching). Above the buttons.
        if self.phase == GamePhase::Launching || self.phase == GamePhase::ReadyToLaunch {
            let bar_y = sy + sh - 110.0;
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: bar_y,
                text: "POWER".to_string(),
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
        });
    }

    fn render_flippers(&self, cmds: &mut Vec<RenderCommand>, tx: f32, ty: f32) {
        // Draw each flipper as a thick line from pivot to tip.
        self.render_one_flipper(cmds, tx, ty, &self.left_flipper);
        self.render_one_flipper(cmds, tx, ty, &self.right_flipper);
    }

    fn render_one_flipper(
        &self,
        cmds: &mut Vec<RenderCommand>,
        tx: f32,
        ty: f32,
        flipper: &Flipper,
    ) {
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
            if !ball.active
                && self.phase != GamePhase::ReadyToLaunch
                && self.phase != GamePhase::Launching
            {
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
            text: "Z: Left | M: Right | Space: Launch | Up: Nudge | P: Pause | F1: Keys"
                .to_string(),
            color: OVERLAY0,
            font_size: FOOTER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 60.0,
            y: ty + TABLE_HEIGHT / 2.0 + 15.0,
            text: "Press P to resume".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 50.0,
            y: ty + TABLE_HEIGHT / 2.0 - 10.0,
            text: format!("Score: {}", self.score),
            color: YELLOW,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: tx + TABLE_WIDTH / 2.0 - 70.0,
            y: ty + TABLE_HEIGHT / 2.0 + 20.0,
            text: "Press N for new game".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
        });
    }
}

/// Queries the tests read the table through.
#[cfg(test)]
impl Pinball {
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

/// What to do about this program's first command-line argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgVerdict {
    /// No argument: get on with it.
    Run,
    Help,
    Version,
    /// Anything else. This program has no options.
    Refuse,
}

/// Classify the first argument, if there is one.
///
/// Filed by lane B on 2026-09-12: this program accepted
/// `--zzq-not-an-option` and exited 0, which to anything reading an exit
/// status says the option was honoured. It was not -- nothing here had ever
/// looked at `argv`. That is the more common shape of what lane B swept out
/// of `userspace/`: not a parse loop with a careless `_ => {}` arm, but no
/// parse at all.
///
/// Takes `&OsStr`, not `&str`, and that is the point of the signature. An
/// argument can hold any byte; decoding it first would mean an undecodable
/// option name arriving here as replacement characters, which is a string
/// nobody passed. Undecodable is simply not one of the two options this
/// program has, so it is refused like any other.
///
/// `--help` and `--version` are answered rather than refused, so that the
/// refusal is a statement about the interface rather than the absence of one.
fn classify_argument(first: Option<&std::ffi::OsStr>) -> ArgVerdict {
    match first.and_then(std::ffi::OsStr::to_str) {
        None if first.is_none() => ArgVerdict::Run,
        Some("--help" | "-h") => ArgVerdict::Help,
        Some("--version" | "-V") => ArgVerdict::Version,
        _ => ArgVerdict::Refuse,
    }
}

/// Act on [`classify_argument`], exiting for every verdict but `Run`.
/// Takes the first argument that is *not* `--display ADDR`: that one belongs
/// to the strap and every application accepts it, so it is not something this
/// game refuses.
fn refuse_arguments(first: Option<&std::ffi::OsStr>) {
    match classify_argument(first) {
        ArgVerdict::Run => {}
        ArgVerdict::Help => {
            println!("usage: pinball");
            println!();
            println!("A pinball game. Takes no options.");
            std::process::exit(0);
        }
        ArgVerdict::Version => {
            println!("pinball {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        ArgVerdict::Refuse => {
            let bad = first.unwrap_or_default();
            // The raw bytes, escaped -- not `Display` and not `to_string_lossy`.
            // An option name can hold any byte, and a lossy conversion prints
            // U+FFFD where the undecodable ones were: a string nobody passed.
            // `escape_ascii` renders those as \xNN, so the message names
            // exactly what arrived and stays printable.
            eprintln!(
                "pinball: unrecognized option: '{}'",
                bad.as_encoded_bytes().escape_ascii()
            );
            eprintln!("usage: pinball");
            std::process::exit(2);
        }
    }
}

impl App for Pinball {
    fn title(&self) -> String {
        "Pinball".to_string()
    }

    fn app_id(&self) -> String {
        "pinball".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        // The table decides the window. Every coordinate in the renderer and
        // every collision bound in the physics is measured from these two
        // constants, so a different window would draw a table the ball does
        // not agree with.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are a positive constant sum well under u32::MAX"
        )]
        {
            (WINDOW_WIDTH.ceil() as u32, WINDOW_HEIGHT.ceil() as u32)
        }
    }

    fn tick_interval(&self) -> Option<Duration> {
        // Without this the ball does not move: everything on this table is
        // driven by `handle_tick`. But only while something is moving: a
        // paused game, a finished one, or a ball waiting in the plunger lane
        // held the whole desktop awake sixty times a second to redraw the
        // same picture.
        self.moving().then_some(TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        // A tick while anything moves is a new picture; a key or a click the
        // game did not take is not.
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window = (width, height);
        self.frame_at((width, height)).into_tree()
    }

    fn theme_changed(&mut self, palette: &Palette) {
        // The card and its colours follow the user's theme; the table keeps
        // its own, which are the machine's rather than the desktop's.
        self.palette = *palette;
    }
}

impl Probe for Pinball {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame_at(size)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
        self.window = size;
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.window = size;
        self.handle_event(&Event::Key(key.clone()))
    }
}

/// A high score as the sidebar lists it: its place, the score, and the day
/// it was made.
fn score_line(i: usize, entry: &HighScoreEntry) -> String {
    let day = entry
        .day
        .get(5..7)
        .and_then(|m| m.parse::<u32>().ok())
        .zip(entry.day.get(8..10).and_then(|d| d.parse::<u32>().ok()))
        .map(|(m, d)| format!("  {} {d}", guitk::date::month_short_name(m)))
        .unwrap_or_default();
    format!("{}. {}{day}", i.saturating_add(1), entry.score)
}

/// One filled rectangle.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, radius: f32) {
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

/// A line of text centred in `r`.
fn centred(f: &mut Frame<Target>, r: Rect, s: &str, size: f32, color: Color) {
    let w = guitk::text::measure(s, size, FontWeightHint::Bold).min(r.w);
    f.push(RenderCommand::Text {
        x: r.x + (r.w - w) / 2.0,
        y: r.y + (r.h - size) / 2.0 - 1.0,
        text: s.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Bold,
        max_width: Some(r.w),
        overflow: TextOverflow::Ellipsis,
    });
}

/// A button: drawn dim and taking no press when it would do nothing.
fn button(f: &mut Frame<Target>, r: Rect, label: &str, target: Target, enabled: bool) {
    fill(f, r, if enabled { SURFACE1 } else { SURFACE0 }, 5.0);
    centred(
        f,
        r,
        label,
        LABEL_FONT_SIZE,
        if enabled { TEXT_COLOR } else { OVERLAY0 },
    );
    if enabled {
        f.hit(target, r);
    }
}

fn main() -> ExitCode {
    // Parsed rather than indexed, so `--display` reaches the connection
    // instead of being refused as an option this game does not take.
    let args = match app::Args::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("pinball: {e}");
            return ExitCode::from(2);
        }
    };
    refuse_arguments(args.rest.first().map(|a| std::ffi::OsStr::new(a.as_str())));
    let mut game = Pinball::from_settings();
    app::launch_with("pinball", args.display.as_deref(), &mut game)
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {

    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    //
    // The same block `apps/match3` carries, and its absence here is why this
    // crate's test module was contributing to the tree's clippy total while
    // every other app's was not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    #[test]
    fn an_option_this_program_does_not_have_is_not_silently_accepted() {
        use std::ffi::OsStr;
        assert_eq!(classify_argument(None), ArgVerdict::Run);
        assert_eq!(
            classify_argument(Some(OsStr::new("--help"))),
            ArgVerdict::Help
        );
        assert_eq!(classify_argument(Some(OsStr::new("-h"))), ArgVerdict::Help);
        assert_eq!(
            classify_argument(Some(OsStr::new("--version"))),
            ArgVerdict::Version
        );
        assert_eq!(
            classify_argument(Some(OsStr::new("-V"))),
            ArgVerdict::Version
        );
        assert_eq!(
            classify_argument(Some(OsStr::new("--zzq-not-an-option"))),
            ArgVerdict::Refuse,
            "the option lane B's sweep uses"
        );
        // An empty argument is still an argument, and `pinball` takes none.
        assert_eq!(classify_argument(Some(OsStr::new(""))), ArgVerdict::Refuse);
    }

    use super::*;

    /// Helper to create a game with a fixed seed.
    fn test_app() -> Pinball {
        with_seed(12345)
    }

    /// A table whose randomness is reproducible, for tests that need it.
    ///
    /// The production constructor draws its seed from the kernel, which is
    /// exactly what a test must not do; this is the seam that lets the same
    /// table be driven from a seed the test chose.
    fn with_seed(seed: u64) -> Pinball {
        Pinball::with_rng(SeededRng::new(seed))
    }

    /// Helper to create a key press event.
    fn key_press(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// Helper to create a key release event.
    fn key_release(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
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
        assert_eq!(app.targets.len(), 5);
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
    fn a_new_table_invents_no_high_scores() {
        // It began with five scores -- 10000 down to 1000 -- that nobody made.
        let app = test_app();
        assert!(app.high_scores.is_empty());
        assert!(texts_of(&app).iter().any(|t| t == "No scores yet"));
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

    // ── Randomness ──────────────────────────────────────────────────
    //
    // The generator itself belongs to `randrange` and is tested there. What
    // belongs here is what this table asks of it: that multiball can throw a
    // ball either way, and that a seeded table replays exactly.

    /// The one thing pinball draws for: which way multiball's extra balls go.
    ///
    /// A generator that always answered the same way would send every extra
    /// ball right, every game — which is what the local `next_f32() > 0.5`
    /// this replaced would have done had its stream ever been degenerate, and
    /// it is invisible in a single game.
    #[test]
    fn multi_ball_throws_extra_balls_both_ways_across_tables() {
        let mut saw_left = false;
        let mut saw_right = false;
        for seed in 0..40_u64 {
            let mut app = with_seed(seed);
            app.activate_multi_ball();
            for ball in app.balls.iter().filter(|ball| ball.active) {
                if ball.vel.x > 0.0 {
                    saw_right = true;
                } else if ball.vel.x < 0.0 {
                    saw_left = true;
                }
            }
        }
        assert!(
            saw_left && saw_right,
            "multiball never threw a ball one of the two ways"
        );
    }

    /// Two tables built from one seed must play identically — that is what
    /// makes every other test in this module reproducible.
    #[test]
    fn the_same_seed_builds_the_same_table() {
        let mut a = with_seed(2024);
        let mut b = with_seed(2024);
        a.activate_multi_ball();
        b.activate_multi_ball();
        let velocities = |app: &Pinball| -> Vec<(f32, f32)> {
            app.balls
                .iter()
                .map(|ball| (ball.vel.x, ball.vel.y))
                .collect()
        };
        assert_eq!(velocities(&a), velocities(&b));
    }

    /// …and two seeds must give two tables, so that "reproducible" has not
    /// quietly become "identical".
    #[test]
    fn different_seeds_eventually_build_different_tables() {
        let throws = |seed: u64| -> Vec<bool> {
            let mut app = with_seed(seed);
            let mut going_right = Vec::new();
            for _ in 0..8 {
                app.multi_ball_active = false;
                app.balls.clear();
                app.activate_multi_ball();
                going_right.extend(app.balls.iter().map(|ball| ball.vel.x > 0.0));
            }
            going_right
        };
        assert_ne!(throws(1), throws(2));
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
        assert!(tip_rest.sub(tip_active).length() > 1.0);
    }

    #[test]
    fn test_flipper_closest_point_at_pivot() {
        let f = Flipper::new(FlipperSide::Left, Vec2::new(80.0, 500.0));
        let closest = f.closest_point(Vec2::new(80.0, 480.0));
        // Should be near the pivot.
        assert!(closest.sub(f.pivot).length() < FLIPPER_LENGTH);
    }

    // ── Nudging and tilt ────────────────────────────────────────────

    #[test]
    fn a_nudge_or_two_is_a_warning_and_one_more_tilts() {
        let mut t = TiltTracker::new();
        assert_eq!(t.nudge(0), Nudge::Warned(1));
        assert!(t.danger(100));
        assert!(!t.danger(DANGER_MS + 1), "the warning fades");
        assert_eq!(t.nudge(1000), Nudge::Warned(2));
        assert_eq!(t.nudge(2000), Nudge::Tilted);
        assert!(t.tilted);
        assert_eq!(
            t.nudge(2500),
            Nudge::Ignored,
            "a tilted table is not listening"
        );
        t.reset();
        assert!(!t.tilted && t.nudges.is_empty());
    }

    #[test]
    fn nudges_far_apart_never_tilt() {
        let mut t = TiltTracker::new();
        for n in 0..20 {
            assert_ne!(t.nudge(n * TILT_WINDOW_MS), Nudge::Tilted, "nudge {n}");
        }
    }

    #[test]
    fn pressing_the_flippers_fast_is_not_a_tilt() {
        // Fifteen flipper presses in a second tilted the table and killed the
        // flippers: the penalty fell on playing well.
        let mut app = test_app();
        launch_and_play(&mut app);
        for _ in 0..40 {
            app.handle_event(&key_press(Key::Z));
            app.handle_event(&tick(10));
            app.handle_event(&key_release(Key::Z));
        }
        assert!(!app.tilt.tilted);
        app.handle_event(&key_press(Key::Z));
        assert!(app.left_flipper.pressed, "the flipper still answers");
    }

    #[test]
    fn a_nudge_shoves_the_ball_up_the_table() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.balls[0].vel = Vec2::new(0.0, 100.0);
        app.handle_event(&key_press(Key::Up));
        assert!(app.balls[0].vel.y < 100.0 - NUDGE_IMPULSE + 0.01);
        assert!((app.balls[0].vel.x.abs() - NUDGE_SIDEWAYS).abs() < 0.01);
    }

    #[test]
    fn a_nudge_does_nothing_before_the_ball_is_in_play() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::Up));
        assert!(app.tilt.nudges.is_empty());
        assert_eq!(app.balls[0].vel, Vec2::ZERO);
    }

    #[test]
    fn a_tilted_table_has_dead_flippers_and_scores_nothing() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.handle_event(&key_press(Key::Z));
        for _ in 0..=TILT_WARNINGS {
            app.handle_event(&key_press(Key::Up));
        }
        assert!(app.tilt.tilted);
        assert!(!app.left_flipper.pressed, "a held flipper drops");
        app.handle_event(&key_press(Key::M));
        assert!(!app.right_flipper.pressed);
        let score = app.score;
        app.award_points(BUMPER_POINTS);
        assert_eq!(app.score, score);
        let texts = texts_of(&app);
        assert!(texts.iter().any(|t| t == "TILT!"));

        // Until the ball drains: the next one is clean.
        app.drain_ball(0);
        app.handle_event(&tick(1600));
        assert!(app.is_ready_to_launch());
        assert!(!app.tilt.tilted);
    }

    #[test]
    fn a_warning_says_danger() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.handle_event(&key_press(Key::Up));
        assert!(texts_of(&app).iter().any(|t| t == "DANGER"));
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
        assert_eq!(
            app.score, 5000,
            "a game in progress is not thrown away on one key"
        );
        assert!(app.confirm_new_game);
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.score, 0);
        assert!(!app.confirm_new_game);
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
        app.high_scores = vec![entry(900, "2026-09-25"), entry(400, "")];
        let hs = app.high_scores.clone();
        app.handle_event(&key_press(Key::N));
        assert_eq!(app.high_scores, hs);
    }

    #[test]
    fn escape_keeps_the_game_going() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.score = 700;
        app.handle_event(&key_press(Key::N));
        assert!(app.confirm_new_game);
        assert!(
            texts_of(&app).iter().any(|t| t == "This one ends, at 700."),
            "the question says what is lost"
        );
        app.handle_event(&key_press(Key::Z));
        assert!(!app.left_flipper.pressed, "the question has the keyboard");
        app.handle_event(&key_press(Key::Escape));
        assert!(!app.confirm_new_game);
        assert_eq!(app.score, 700);
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
        app.high_scores = (1..=5).map(|n| entry(n * 1000, "")).rev().collect();
        app.score = 50000;
        app.update_high_scores();
        assert_eq!(app.high_scores.len(), HIGH_SCORE_SLOTS);
        assert_eq!(app.high_scores[0].score, 50000);
        assert_eq!(app.high_scores[HIGH_SCORE_SLOTS - 1].score, 2000);
    }

    #[test]
    fn test_low_score_not_inserted() {
        let mut app = test_app();
        app.high_scores = (1..=5).map(|n| entry(n * 1000, "")).rev().collect();
        app.score = 500;
        app.update_high_scores();
        assert_eq!(app.high_scores[HIGH_SCORE_SLOTS - 1].score, 1000);
        let mut app = test_app();
        app.score = 0;
        app.update_high_scores();
        assert!(
            app.high_scores.is_empty(),
            "a game that scored nothing is no high score"
        );
    }

    #[test]
    fn a_high_score_is_dated_by_the_clock() {
        let mut app = test_app();
        app.clock = fixed_clock;
        app.score = 1200;
        app.update_high_scores();
        assert_eq!(app.high_scores[0], entry(1200, "2025-09-25"));
        assert!(texts_of(&app).iter().any(|t| t == "1. 1200  Sep 25"));
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
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut app = test_app();
        app.phase = GamePhase::GameOver;
        let cmds = app.render_commands();
        // Should contain game over text.
        let has_game_over = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("GAME OVER")));
        assert!(has_game_over);
    }

    #[test]
    fn test_render_pause_overlay() {
        let mut app = test_app();
        app.phase = GamePhase::Paused;
        let cmds = app.render_commands();
        let has_paused = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("PAUSED")));
        assert!(has_paused);
    }

    #[test]
    fn test_render_ball_lost_overlay() {
        let mut app = test_app();
        app.phase = GamePhase::BallLost;
        let cmds = app.render_commands();
        let has_ball_lost = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("BALL LOST")));
        assert!(has_ball_lost);
    }

    #[test]
    fn test_render_has_score_text() {
        let app = test_app();
        let cmds = app.render_commands();
        let has_score = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("SCORE")));
        assert!(has_score);
    }

    #[test]
    fn test_render_has_pinball_title() {
        let app = test_app();
        let cmds = app.render_commands();
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("PINBALL")));
        assert!(has_title);
    }

    #[test]
    fn test_render_has_footer() {
        let app = test_app();
        let cmds = app.render_commands();
        let has_footer = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Launch")));
        assert!(has_footer);
    }

    #[test]
    fn test_render_bumpers_visible() {
        let app = test_app();
        let cmds = app.render_commands();
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
        let cmds = app.render_commands();
        let has_multi = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("MULTI-BALL")));
        assert!(has_multi);
    }

    #[test]
    fn test_render_with_tilt_indicator() {
        let mut app = test_app();
        app.tilt.tilted = true;
        let cmds = app.render_commands();
        let has_tilt = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("TILT")));
        assert!(has_tilt);
    }

    #[test]
    fn test_render_high_scores_visible() {
        let app = test_app();
        let cmds = app.render_commands();
        let has_hs = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("HIGH SCORES")));
        assert!(has_hs);
    }

    #[test]
    fn test_render_plunger_when_ready() {
        let app = test_app();
        let cmds = app.render_commands();
        // Plunger renders a FillRect with PEACH color for the head.
        let has_plunger = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == PEACH));
        assert!(has_plunger);
    }

    // ── Helpers for what follows ────────────────────────────────────

    fn entry(score: u32, day: &str) -> HighScoreEntry {
        HighScoreEntry {
            score,
            day: day.to_string(),
        }
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "it stands in for `system_clock`, whose type it must have"
    )]
    fn fixed_clock() -> Option<i64> {
        // 2025-09-25 11:33:20 UTC.
        Some(1_758_800_000)
    }

    /// Every string the window draws.
    fn texts_of(app: &Pinball) -> Vec<String> {
        app.frame_at((WINDOW_WIDTH, WINDOW_HEIGHT))
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn mouse(app: &mut Pinball, target: Target, down: bool) {
        let (x, y) = guitk::probe::rect_of(app, target)
            .unwrap_or_else(|| panic!("{target:?} is not on screen"))
            .centre();
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: if down {
                MouseEventKind::Press(MouseButton::Left)
            } else {
                MouseEventKind::Release(MouseButton::Left)
            },
        }));
    }

    // ── The pointer ─────────────────────────────────────────────────

    #[test]
    fn the_whole_game_can_be_played_with_the_pointer() {
        let mut app = test_app();
        // The plunger: hold, and let go to launch.
        mouse(&mut app, Target::Plunger, true);
        assert!(app.is_launching());
        app.handle_event(&tick(300));
        mouse(&mut app, Target::Plunger, false);
        assert!(app.is_playing(), "letting go launched the ball");

        // The table's halves are the flippers, while the button is held.
        mouse(&mut app, Target::LeftFlipper, true);
        assert!(app.left_flipper_pressed() && !app.right_flipper_pressed());
        mouse(&mut app, Target::LeftFlipper, false);
        assert!(!app.left_flipper_pressed());
        mouse(&mut app, Target::RightFlipper, true);
        assert!(app.right_flipper_pressed());
        mouse(&mut app, Target::RightFlipper, false);
        assert!(!app.right_flipper_pressed());
        assert!(
            guitk::probe::rect_of(&app, Target::Plunger).is_none(),
            "the lane is not a plunger with the ball in play"
        );

        guitk::probe::click(&mut app, Target::Nudge);
        assert_eq!(app.tilt.nudges.len(), 1);

        guitk::probe::click(&mut app, Target::Pause);
        assert!(app.is_paused());
        guitk::probe::click(&mut app, Target::Resume);
        assert!(app.is_playing());

        guitk::probe::click(&mut app, Target::Keys);
        assert!(app.show_help);
        assert!(
            guitk::probe::rect_of(&app, Target::NewGame).is_none(),
            "behind the card, nothing can be clicked"
        );
        guitk::probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);

        app.score = 300;
        guitk::probe::click(&mut app, Target::NewGame);
        assert!(app.confirm_new_game);
        guitk::probe::click(&mut app, Target::KeepPlaying);
        assert!(!app.confirm_new_game && app.score == 300);
        guitk::probe::click(&mut app, Target::NewGame);
        guitk::probe::click(&mut app, Target::ConfirmNewGame);
        assert_eq!(app.score, 0);
        assert!(app.is_ready_to_launch());

        app.phase = GamePhase::GameOver;
        guitk::probe::click(&mut app, Target::PlayAgain);
        assert!(app.is_ready_to_launch(), "play again starts a game");
    }

    #[test]
    fn a_button_that_would_do_nothing_takes_no_press() {
        let mut app = test_app();
        assert!(
            guitk::probe::rect_of(&app, Target::Nudge).is_none(),
            "no nudging a ball in the plunger lane"
        );
        app.phase = GamePhase::GameOver;
        assert!(guitk::probe::rect_of(&app, Target::Pause).is_none());
        assert!(guitk::probe::rect_of(&app, Target::LeftFlipper).is_none());
    }

    #[test]
    fn a_larger_window_puts_the_table_in_the_middle() {
        let app = test_app();
        let small =
            guitk::probe::rect_of_sized(&app, Target::NewGame, (WINDOW_WIDTH, WINDOW_HEIGHT))
                .unwrap();
        let big = guitk::probe::rect_of_sized(
            &app,
            Target::NewGame,
            (WINDOW_WIDTH + 200.0, WINDOW_HEIGHT + 100.0),
        )
        .unwrap();
        assert!((big.x - small.x - 100.0).abs() < 0.01 && (big.y - small.y - 50.0).abs() < 0.01);
        let frame = app.frame_at((WINDOW_WIDTH + 200.0, WINDOW_HEIGHT + 100.0));
        assert!(frame.is_balanced());
    }

    // ── Keys ────────────────────────────────────────────────────────

    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                // Before the ball, with it in play, with the list up, and with
                // the new-game question up: Esc means something only in the
                // last two.
                let answered = (0..4).any(|state| {
                    let mut app = test_app();
                    if state > 0 {
                        launch_and_play(&mut app);
                        app.score = 100;
                    }
                    app.show_help = state == 2;
                    app.confirm_new_game = state == 3;
                    app.handle_key(&stroke) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    #[test]
    fn the_list_of_keys_reaches_the_window_and_holds_the_keyboard() {
        let mut app = test_app();
        app.handle_event(&key_press(Key::F1));
        let missing = guitk::shortcut::missing_rows(&texts_of(&app), SHORTCUTS);
        assert!(missing.is_empty(), "{missing:?}");
        app.handle_event(&key_press(Key::Space));
        assert!(
            app.is_ready_to_launch(),
            "Space pulled the plunger through the card"
        );
        app.handle_event(&key_press(Key::Escape));
        assert!(!app.show_help);
        app.handle_event(&key_press(Key::Space));
        assert!(
            app.is_launching(),
            "control: Space pulls it with the card down"
        );
    }

    #[test]
    fn the_flippers_work_before_the_ball_is_launched() {
        // They moved only inside the physics step, which runs only with the
        // ball in play.
        let mut app = test_app();
        app.handle_event(&key_press(Key::Z));
        app.handle_event(&tick(50));
        assert!(app.left_flipper.angle > 0.0);
        app.handle_event(&key_release(Key::Z));
        app.handle_event(&tick(200));
        assert!(app.left_flipper.angle.abs() < f32::EPSILON);
    }

    // ── The window ──────────────────────────────────────────────────

    #[test]
    fn losing_the_keyboard_pauses_a_game_and_lets_go_of_the_flippers() {
        let mut app = test_app();
        launch_and_play(&mut app);
        app.handle_event(&key_press(Key::Z));
        app.handle_event(&Event::FocusOut);
        assert!(app.is_paused(), "the ball would drain with nobody watching");
        assert!(!app.left_flipper_pressed(), "its key-up will never come");
        app.handle_event(&key_press(Key::P));
        assert!(app.is_playing());

        let mut fresh = test_app();
        fresh.handle_event(&key_press(Key::Space));
        fresh.handle_event(&Event::FocusOut);
        assert!(
            fresh.is_ready_to_launch(),
            "a half-pulled plunger goes back, unlaunched"
        );
        assert!(!fresh.is_paused(), "nothing to pause before the first ball");
    }

    #[test]
    fn the_clock_is_asked_for_only_while_something_moves() {
        let mut app = test_app();
        app.handle_event(&tick(300));
        assert_eq!(
            app.tick_interval(),
            None,
            "a ball waiting to be launched moves nothing"
        );
        app.handle_event(&key_press(Key::Z));
        assert_eq!(app.tick_interval(), Some(TICK), "a flipper on its way");
        app.handle_event(&key_release(Key::Z));
        app.handle_event(&tick(300));
        launch_and_play(&mut app);
        assert_eq!(app.tick_interval(), Some(TICK));
        app.handle_event(&key_press(Key::P));
        app.handle_event(&tick(300));
        assert_eq!(app.tick_interval(), None, "a paused game");
    }

    // ── Kept scores ─────────────────────────────────────────────────

    #[test]
    fn high_scores_are_kept_and_read_back() {
        settingsfile::testing::with_scratch_config("pinball-kept", |_| {
            let mut app = Pinball::from_settings();
            assert!(app.high_scores.is_empty() && app.scores_note.is_none());
            app.clock = fixed_clock;
            app.score = 4200;
            app.drain_ball(0);
            app.balls_remaining = 1;
            app.drain_ball(0);
            assert!(app.is_game_over());
            let path = scores_path().unwrap();
            let written = std::fs::read_to_string(&path).unwrap();
            assert_eq!(written, "pinball high scores 1\n4200\t2025-09-25\n");

            let again = Pinball::from_settings();
            assert_eq!(again.high_scores, vec![entry(4200, "2025-09-25")]);
        });
    }

    #[test]
    fn a_table_that_is_not_opened_from_settings_writes_nothing() {
        settingsfile::testing::with_scratch_config("pinball-quiet", |_| {
            let mut app = test_app();
            app.score = 900;
            app.update_high_scores();
            // Where a kept table would be -- the settings directory is not the
            // scratch root, so this asks the function that decides.
            let path = scores_path().expect("a scratch settings directory");
            assert!(!path.exists(), "{} was written", path.display());
            assert!(
                !path.parent().is_some_and(Path::exists),
                "nor its directory made"
            );
        });
    }

    #[test]
    fn a_scores_file_that_cannot_be_read_whole_is_left_alone() {
        settingsfile::testing::with_scratch_config("pinball-broken", |_| {
            let path = scores_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let broken = "pinball high scores 1\n900\t2025-09-01\nlots\t2025-09-02\n";
            std::fs::write(&path, broken).unwrap();
            let mut app = Pinball::from_settings();
            assert!(app.high_scores.is_empty());
            let note = app.scores_note.clone().expect("the sidebar says why");
            assert!(note.contains("line 3"), "{note}");
            app.score = 5000;
            app.update_high_scores();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                broken,
                "nothing saved over it"
            );
            assert!(texts_of(&app).iter().any(|t| t.contains("not read")));
        });
    }

    #[test]
    fn the_scores_file_is_its_own_format_and_nothing_else() {
        assert_eq!(parse_scores("pinball high scores 1\n"), Ok(Vec::new()));
        assert!(parse_scores("").is_err());
        assert!(
            parse_scores("pinball high scores 2\n").is_err(),
            "a later format"
        );
        assert!(parse_scores("pinball high scores 1\n5\t2025-9-1\n").is_err());
        assert!(parse_scores("pinball high scores 1\n5 2025-09-01\n").is_err());
        let six: String = (0..6).fold(String::new(), |mut s, n| {
            s.push_str(&n.to_string());
            s.push_str("\t\n");
            s
        });
        assert!(parse_scores(&format!("pinball high scores 1\n{six}")).is_err());
        let kept = vec![entry(10, "2025-01-02"), entry(9, "")];
        assert_eq!(parse_scores(&scores_text(&kept)), Ok(kept), "a round trip");
    }
}

/// The wiring itself, driven through the real strap against a stand-in
/// compositor.
///
/// Every other test in this file exercises the game's own model. None of them
/// would notice if the `App` impl were deleted, or if `initial_size` returned
/// `(0, 0)`, or if `render` handed back an empty tree -- the program would
/// compile, launch, and show nothing. That is precisely the failure
/// `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` is about, and it is invisible to a
/// model test by construction.
///
/// So this opens a window *through the trait* -- the title and the size are
/// the ones the game reports, not ones the test chose -- scripts real input,
/// and asserts frames came out and the close request was obeyed.
///
/// **Proved able to fail rather than assumed to be.** Replacing the body of
/// `App::render` with an empty `RenderTree` -- the shape of a wiring that
/// compiles and shows nothing -- makes this report *the game submitted 2
/// frame(s) for this window and every one was empty, so the window would be
/// blank*, while all 120 of the model tests keep passing.
#[cfg(test)]
mod reaches_a_window {
    // A failure here should point at the line that failed, which is what a
    // panicking assertion does. The defensive lints exist to keep panics out
    // of code that runs on a user's data; this is a harness. Same list, and
    // the same reason, as `mod tests` above.
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use oswindow::InputEvent;
    use oswindow::testing;

    #[test]
    fn the_game_opens_a_window_draws_and_closes() {
        let (mut events, desktop) = testing::desktop();
        let mut game = Pinball::new();

        let window = app::open(&mut events, &game).expect("the compositor should grant a window");

        {
            let mut desk = desktop.borrow_mut();
            // A tick, because this game animates without input and a frame
            // that only ever follows a click would look frozen.
            desk.script.push_back(vec![InputEvent::new(
                window,
                Event::Tick { elapsed_ms: 16 },
            )]);
            desk.script
                .push_back(vec![InputEvent::new(window, Event::CloseRequested)]);
        }

        app::drive(&mut events, window, &mut game).expect("the loopback connection cannot fail");

        let desk = desktop.borrow();
        // NEGATIVE CONTROL FIRST. `submitted` being non-empty is what makes
        // the command count below mean "the game drew" rather than "nothing
        // ran and the filter found nothing".
        assert!(
            !desk.submitted.is_empty(),
            "no frame was submitted at all, so this test cannot say anything \
             about what was drawn"
        );
        let commands: usize = desk
            .submitted
            .iter()
            .filter(|(w, _)| *w == window)
            .map(|(_, n)| *n)
            .sum();
        assert!(
            commands > 0,
            "the game submitted {} frame(s) for this window and every one was \
             empty, so the window would be blank",
            desk.submitted.len()
        );
    }

    /// The window the game asks for is the board it draws, not a number.
    ///
    /// `initial_size` is the one part of the `App` impl a compositor obeys
    /// without question, and a wrong answer there is not a crash -- it is a
    /// window with the playfield clipped or floating in a margin. Deriving the
    /// expectation from the same constants the renderer uses is what stops
    /// this from being a copy of the answer.
    #[test]
    fn the_window_it_asks_for_is_the_size_of_the_board() {
        let game = Pinball::new();
        let (w, h) = game.initial_size();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the same positive constants the impl casts"
        )]
        let expected = (WINDOW_WIDTH.ceil() as u32, WINDOW_HEIGHT.ceil() as u32);
        assert_eq!((w, h), expected, "the window does not match the board");
        assert!(w > 0 && h > 0, "a zero dimension is an invisible window");
    }
}
