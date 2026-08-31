//! Snake — drive a growing snake around a 20x20 board, eat the food, do not
//! run into a wall or into yourself. Three difficulties, an optional wrap
//! mode that turns the walls into doorways, bonus food that expires, a streak
//! bonus for eating without dawdling, and a stats panel.
//!
//! ## What wiring it found
//!
//! `main` was `let _app = SnakeApp::new();` — it built the board, placed the
//! first food and dropped the lot, so no frame ever reached a screen and no
//! key ever arrived.
//!
//! **The program chose its own window and then drew in absolute pixels inside
//! it.** `render` took no size; `window_width()` returned
//! `PADDING * 3 + grid_width() + STATS_PANEL_WIDTH` off a `CELL_SIZE` of 24,
//! and every draw call was placed against those constants. A window the user
//! made smaller did not shrink the board, it cut it off. Everything is solved
//! from the live window size every frame now.
//!
//! **The pointer did nothing whatsoever.** `handle_event` matched `Event::Key`
//! and `Event::Tick` and had no mouse arm at all, so pause, restart, the wrap
//! switch and the three difficulties were keyboard-only — and on the game-over
//! screen, which is where a new player arrives within a minute, there was no
//! way to start again except to know that Enter does it. The footer carries
//! those five switches as controls now, and clicking a square of the board
//! steers the snake towards it.
//!
//! **A stall made the snake teleport.** `handle_tick` banked `elapsed_ms` and
//! subtracted *one* interval per call, so a window that was hidden for ten
//! seconds came back with 9.85 seconds of credit and the snake then moved
//! once per frame until it had spent them — usually into a wall, always
//! without the player having seen it. Catch-up is bounded now, and what
//! cannot be caught up on is dropped.
//!
//! **Filling the board hung the game.** `random_empty_cell` drew a cell at
//! random and retried until it found one the snake was not on; with the snake
//! covering all 400 squares — which is winning — there is no such cell, and
//! the loop never ends. It is a scan of the free squares now, and a board with
//! none of them is a win rather than a hang.
//!
//! **Bonus food revived a streak that had lapsed.** `eat_normal_food` reset
//! the streak to 1 when more than `STREAK_WINDOW_TICKS` had gone by;
//! `eat_bonus_food` incremented it unconditionally, so a bonus eaten after a
//! minute of wandering counted as though it had come straight after the last
//! meal.
//!
//! Ten blanket `#![allow(...)]` sat at the top of the file, `dead_code` among
//! them — which is what let a program whose `main` discarded its own app
//! compile without a word of complaint.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use randrange::{RandomSource, SeededRng, seeded_from_system};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
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

// ── The board ───────────────────────────────────────────────────────
const GRID_COLS: usize = 20;
const GRID_ROWS: usize = 20;

/// The gap between two neighbouring squares, per unit of square size, so that
/// the board looks the same at every scale rather than growing a hairline gap
/// on a big window and a chasm on a small one.
const GAP_PER_CELL: f32 = 0.08;

/// The share of the body's width the stats panel may take before the board
/// starts losing squares to it. The panel is as wide as its own widest line
/// when there is room; this is the ceiling, not the width.
const STATS_SHARE: f32 = 0.34;

/// The size the window opens at: the board square at a comfortable 24 pixels
/// a square, with room beside it for the panel and above and below for the
/// two bands.
const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 660.0;

/// How often the window is asked for a tick. This is the *frame* rate, not
/// the snake's speed: the snake moves on a whole number of these, so the
/// interval has to divide the fastest difficulty (100ms) evenly or the game
/// would run slower than it claims.
const TICK: Duration = Duration::from_millis(25);

/// The most moves one tick may make up for after a stall.
///
/// `handle_tick` banks the time it is given and spends it a move at a time.
/// Without a ceiling a window that was hidden for ten seconds comes back with
/// four hundred moves of credit and plays them out in one frame — the player
/// sees the snake vanish and the game end. Two is enough to absorb a late
/// frame and small enough that nothing happens off-screen.
const MAX_CATCH_UP_MOVES: u32 = 2;

/// Maximum number of queued direction changes.
const MAX_DIR_QUEUE: usize = 2;

/// Number of moves a bonus food stays on the board before it disappears.
const BONUS_FOOD_LIFETIME: u32 = 30;

/// Points awarded for normal food.
const NORMAL_FOOD_POINTS: u32 = 10;

/// Points awarded for bonus food.
const BONUS_FOOD_POINTS: u32 = 50;

/// Eat this many in a row without dawdling and the multiplier applies.
const STREAK_THRESHOLD: u32 = 3;

/// The multiplier a streak is worth.
const STREAK_MULTIPLIER: u32 = 2;

/// How many moves between two meals still counts as a streak.
const STREAK_WINDOW_TICKS: u32 = 30;

/// Chance (1 in N) that a bonus food appears after a normal one is eaten.
const BONUS_SPAWN_CHANCE: u64 = 5;

/// Every 50 points is a speed level, up to this many.
const MAX_SPEED_LEVEL: u32 = 10;

/// The snake never moves slower than one square per this many milliseconds.
const MIN_INTERVAL_MS: u32 = 50;

/// The snake's starting length.
const START_LENGTH: i32 = 3;

/// The heading over the statistics column.
///
/// Named, because `stats_width` measures it to decide how wide the column
/// wants to be and `draw_stats` is what draws it, forty lines and one function
/// away. A string measured in one place and drawn in another is a column sized
/// for a line it does not contain (known-issues lesson 93).
const STATS_HEADING: &str = "Stats";

// ── Direction ───────────────────────────────────────────────────────

/// One of the four ways the snake can be pointing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// Whether this direction is the other one turned around.
    ///
    /// A snake may not reverse into itself, which is what this is for.
    ///
    /// Answered from [`delta`](Self::delta) rather than by a second table of
    /// pairs, so that which way a direction points and which direction is its
    /// reverse cannot come to disagree.
    #[must_use]
    pub fn is_opposite(self, other: Self) -> bool {
        let (dr, dc) = self.delta();
        let (odr, odc) = other.delta();
        dr == odr.saturating_neg() && dc == odc.saturating_neg()
    }

    /// The (row, column) step one move in this direction takes.
    #[must_use]
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Self::Up => (-1, 0),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
            Self::Right => (0, 1),
        }
    }

    /// The direction an arrow or WASD key asks for, if it asks for one.
    #[must_use]
    pub const fn from_key(key: Key) -> Option<Self> {
        match key {
            Key::Up | Key::W => Some(Self::Up),
            Key::Down | Key::S => Some(Self::Down),
            Key::Left | Key::A => Some(Self::Left),
            Key::Right | Key::D => Some(Self::Right),
            _ => None,
        }
    }
}

// ── Grid position ───────────────────────────────────────────────────

/// A square of the board, by row and column.
///
/// Signed, because a snake walking off the top of the board is at row `-1`
/// until something decides what that means — a wall, or the bottom row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub row: i32,
    pub col: i32,
}

impl Pos {
    #[must_use]
    pub const fn new(row: i32, col: i32) -> Self {
        Self { row, col }
    }

    /// This square, one step in the given direction.
    #[must_use]
    pub fn moved(self, dir: Direction) -> Self {
        let (dr, dc) = dir.delta();
        Self {
            row: self.row.saturating_add(dr),
            col: self.col.saturating_add(dc),
        }
    }

    /// This square brought back onto the board, off whichever edge it left.
    #[must_use]
    pub fn wrapped(self) -> Self {
        Self {
            row: wrap_index(self.row, GRID_ROWS),
            col: wrap_index(self.col, GRID_COLS),
        }
    }

    /// Whether this square is on the board.
    #[must_use]
    pub fn in_bounds(self) -> bool {
        let rows = i32::try_from(GRID_ROWS).unwrap_or(i32::MAX);
        let cols = i32::try_from(GRID_COLS).unwrap_or(i32::MAX);
        self.row >= 0 && self.row < rows && self.col >= 0 && self.col < cols
    }
}

/// `v` brought into `0..len`, off either end.
///
/// Rust's `%` keeps the sign of the left operand, so `-1 % 20` is `-1` and not
/// the 19 a wrap wants; adding `len` back to a negative remainder is what
/// fixes that.
fn wrap_index(v: i32, len: usize) -> i32 {
    let len = i32::try_from(len).unwrap_or(i32::MAX);
    // A board with no rows has no square to bring anything back onto, so the
    // answer is the only index that could not be out of range.
    let Some(m) = v.checked_rem(len) else {
        return 0;
    };
    if m < 0 { m.saturating_add(len) } else { m }
}

// ── Food ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoodKind {
    Normal,
    Bonus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Food {
    pub pos: Pos,
    pub kind: FoodKind,
    /// Moves left before this food disappears. Only bonus food expires.
    pub ticks_remaining: u32,
}

// ── Difficulty ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

/// The three difficulties, in the order the footer offers them.
pub const DIFFICULTIES: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

impl Difficulty {
    /// Milliseconds between moves at speed level 1.
    #[must_use]
    pub const fn base_interval_ms(self) -> u32 {
        match self {
            Self::Easy => 200,
            Self::Medium => 150,
            Self::Hard => 100,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }

    /// The number key that picks this difficulty on the game-over screen.
    #[must_use]
    pub const fn key(self) -> Key {
        match self {
            Self::Easy => Key::Num1,
            Self::Medium => Key::Num2,
            Self::Hard => Key::Num3,
        }
    }
}

// ── Game state ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Playing,
    Paused,
    /// Ran into a wall or into itself.
    GameOver,
    /// Filled the board. There is nowhere left to put food, which is the
    /// condition the old `random_empty_cell` looped forever on.
    Won,
}

// ── What a click can land on ────────────────────────────────────────

/// Everything on the screen a pointer can hit.
///
/// The drawing pass records these as it draws, so a control is clickable
/// exactly where its ink is and nowhere else — the geometry is written down
/// once (`known-issues.md` lesson 63).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A square of the board, by row and column. Clicking one steers the snake
    /// towards it.
    Cell(usize, usize),
    /// Pause, or unpause.
    Pause,
    /// Start again.
    Restart,
    /// Turn the walls into doorways, or back into walls.
    Wrap,
    /// Pick a difficulty.
    Level(Difficulty),
}

/// Whether an event changed anything the window would need to redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    Consumed,
    Ignored,
}

// ── Speed ───────────────────────────────────────────────────────────

/// The speed level a score has earned: one per 50 points, capped.
#[must_use]
pub fn speed_level(score: u32) -> u32 {
    (score / 50).saturating_add(1).min(MAX_SPEED_LEVEL)
}

/// Milliseconds between moves at a given difficulty and speed level.
///
/// Each level above the first takes a tenth of the base interval off, so Hard
/// at level 10 is the floor and Easy at level 1 is the ceiling.
#[must_use]
pub fn tick_interval_ms(difficulty: Difficulty, level: u32) -> u32 {
    let base = difficulty.base_interval_ms();
    let reduction = level.saturating_sub(1).saturating_mul(base / 10);
    base.saturating_sub(reduction).max(MIN_INTERVAL_MS)
}

// ── Layout ──────────────────────────────────────────────────────────

/// The bands a window of a given size is divided into.
///
/// Built fresh every frame from the live window size and never stored on the
/// model, because a remembered layout is one that can disagree with the window
/// it is drawn in — which is exactly what the absolute `window_width()` and
/// `grid_origin_x()` this replaces did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Score, high score, and the state of play.
    pub header: Rect,
    /// The board and the stats panel.
    pub body: Rect,
    /// The five switches.
    pub footer: Rect,
    /// The header's size.
    pub big: f32,
    /// Body text.
    pub font: f32,
    /// The stats panel and the footer.
    pub small: f32,
    /// The margin between a band and what is inside it.
    pub pad: f32,
}

impl Layout {
    /// Solve the bands for a window of this size.
    ///
    /// The footer gives up its height first and the header second, so that a
    /// window squashed from below loses its switches before it loses the score
    /// — and a window squashed to nothing is left with bands of no height
    /// rather than bands of negative height, which would draw inside out.
    #[must_use]
    pub fn new(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0);
        let big = (h / 26.0).clamp(10.0, 24.0);
        let font = (h / 40.0).clamp(8.0, 16.0);
        let small = (font - 1.0).max(7.0);

        let mut hdr = (h * 0.09).clamp(22.0, 54.0);
        let mut ftr = (h * 0.08).clamp(20.0, 46.0);
        // The board is what the game is; the bands round it are what is spent
        // to have it. `spare` is all the two of them may spend between them.
        //
        // Written as one share of `h` rather than as `h` less another share:
        // `spare` is then non-negative by construction and the header's own
        // share is non-negative because it only reaches the first arm when it
        // is the larger of the two — so neither needs flooring at nought, and
        // there is no floor here that no test could reach
        // (`known-issues.md` lesson 51).
        let spare = h * 0.45;
        if hdr + ftr > spare {
            if hdr > spare {
                hdr = spare;
                ftr = 0.0;
            } else {
                ftr = spare - hdr;
            }
        }

        let header = Rect::new(0.0, 0.0, w, hdr);
        let footer = Rect::new(0.0, h - ftr, w, ftr);
        let body = Rect::new(
            pad,
            hdr + pad,
            (w - pad * 2.0).max(0.0),
            (footer.y - hdr - pad * 2.0).max(0.0),
        );

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            body,
            footer,
            big,
            font,
            small,
            pad,
        }
    }

    /// Split the body into the board's half and the stats panel's.
    ///
    /// `wanted` is how wide the panel's own widest line is. The panel gets it
    /// when there is room and less when there is not: it may never take more
    /// than [`STATS_SHARE`] of the body, because the board losing squares to a
    /// panel of numbers is the wrong trade. A body too narrow for both leaves
    /// the panel a strip of no width, which draws nothing.
    ///
    /// No emptiness guard in front of this: every width below is floored at
    /// nought, so a body of no size yields two strips of no size and both draw
    /// nothing, which is what a guard would have returned. A guard in front of
    /// a rule that already holds is a line no test can own (`known-issues.md`
    /// lesson 51).
    #[must_use]
    pub fn split(&self, wanted: f32) -> (Rect, Rect) {
        let ceiling = (self.body.w * STATS_SHARE - self.pad).max(0.0);
        let stats_w = wanted.max(0.0).min(ceiling);
        let board_w = (self.body.w - stats_w - self.pad).max(0.0);
        let board = Rect::new(self.body.x, self.body.y, board_w, self.body.h);
        let stats = Rect::new(
            self.body.right() - stats_w,
            self.body.y,
            stats_w,
            self.body.h,
        );
        (board, stats)
    }
}

// ── The board's geometry ────────────────────────────────────────────

/// Where every square of the board goes inside the space it was given.
///
/// One number is solved — the size of a square — and everything else follows
/// from it, so the drawing pass and the hit test cannot disagree about where a
/// square is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Board {
    /// The space the board was given, which is usually bigger than the board.
    pub area: Rect,
    pub cols: usize,
    pub rows: usize,
    /// The side of one square.
    pub cell: f32,
    /// The space between two neighbouring squares.
    pub gap: f32,
    /// The squares themselves, centred in `area`.
    pub cells: Rect,
}

impl Board {
    /// Fit a `cols` x `rows` board of square squares into `area`, centred.
    ///
    /// A board with no squares is turned away at the door because the fit below
    /// divides by `n - 1` gaps, which is negative for `n == 0`. A board with no
    /// *room* is not: the fit already answers a square of no size for it, and a
    /// guard in front of a rule that already holds is a line no test can own
    /// (`known-issues.md` lesson 51).
    #[must_use]
    pub fn new(area: Rect, cols: usize, rows: usize) -> Self {
        if cols == 0 || rows == 0 {
            return Self {
                area,
                cols,
                rows,
                cell: 0.0,
                gap: 0.0,
                cells: Rect::EMPTY,
            };
        }
        let across = usize_f32(cols);
        let down = usize_f32(rows);
        // A row of `n` squares is `n` squares and `n - 1` gaps wide, each gap
        // being `GAP_PER_CELL` of a square. Solve for the square that makes
        // that fit both ways and take the smaller, so the squares stay square.
        let per_w = across + (across - 1.0) * GAP_PER_CELL;
        let per_h = down + (down - 1.0) * GAP_PER_CELL;
        let cell = (area.w / per_w).min(area.h / per_h).max(0.0);
        let gap = cell * GAP_PER_CELL;
        let span_w = across * cell + (across - 1.0) * gap;
        let span_h = down * cell + (down - 1.0) * gap;
        let cells = Rect::new(
            area.x + (area.w - span_w) / 2.0,
            area.y + (area.h - span_h) / 2.0,
            span_w,
            span_h,
        );
        Self {
            area,
            cols,
            rows,
            cell,
            gap,
            cells,
        }
    }

    /// The distance from one square's left edge to the next one's.
    #[must_use]
    pub fn step(&self) -> f32 {
        self.cell + self.gap
    }

    /// The ink of one square, or [`Rect::EMPTY`] for a square off the board.
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if row >= self.rows || col >= self.cols || self.cell <= 0.0 {
            return Rect::EMPTY;
        }
        Rect::new(
            self.cells.x + usize_f32(col) * self.step(),
            self.cells.y + usize_f32(row) * self.step(),
            self.cell,
            self.cell,
        )
    }

    /// The clickable box of one square: its ink grown by half the gap on every
    /// side, so the gaps belong to the square nearer them and a click between
    /// two squares lands on one of them rather than falling through.
    #[must_use]
    pub fn cell_hit(&self, row: usize, col: usize) -> Rect {
        let r = self.cell_rect(row, col);
        if r.is_empty() {
            return Rect::EMPTY;
        }
        let half = self.gap / 2.0;
        Rect::new(r.x - half, r.y - half, r.w + self.gap, r.h + self.gap)
    }
}

/// `usize` as `f32`, saturating rather than wrapping.
///
/// Every count here is a board dimension or an index into one, all far below
/// `f32`'s exact-integer range; the cast is written out so the lint does not
/// have to be turned off across the file to allow it.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts are board dimensions, orders of magnitude below 2^24"
)]
fn usize_f32(v: usize) -> f32 {
    v as f32
}

// ── The game ────────────────────────────────────────────────────────

/// The whole of the game: the snake, what it is eating, and the score.
pub struct SnakeApp {
    /// The snake's squares, head first.
    pub snake: Vec<Pos>,
    /// Which way the snake is going.
    pub direction: Direction,
    /// Turns asked for but not yet taken, so a fast double-turn is not lost
    /// between two moves.
    pub dir_queue: Vec<Direction>,
    pub food: Food,
    pub bonus_food: Option<Food>,
    pub state: GameState,
    pub difficulty: Difficulty,
    /// Whether the walls are doorways.
    pub wrap_mode: bool,
    pub score: u32,
    pub high_score: u32,
    pub foods_eaten: u32,
    pub bonus_eaten: u32,
    /// Meals in a row taken without dawdling.
    pub streak: u32,
    /// Moves since the last meal, which is what "dawdling" is measured in.
    pub ticks_since_food: u32,
    /// Moves this game.
    pub total_ticks: u32,
    /// Time banked towards the next move.
    pub accumulated_ms: u64,
    rng: SeededRng,
    /// Advanced once per move, so the food's pulse is tied to the game's clock
    /// rather than to how often the window happens to redraw.
    pub pulse_counter: u32,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against. Stored for that reason and no other.
    size: (f32, f32),
}

impl SnakeApp {
    /// A new game, seeded from the system so two players do not get the same
    /// board.
    #[must_use]
    pub fn new() -> Self {
        Self::from_rng(seeded_from_system(0x5EED_5EED))
    }

    /// A new game from a known seed, which is what the tests use.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        Self::from_rng(SeededRng::new(seed))
    }

    fn from_rng(rng: SeededRng) -> Self {
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
            rng,
            pulse_counter: 0,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        app.init_snake();
        app.spawn_food();
        app
    }

    /// Put a three-square snake in the middle of the board.
    ///
    /// Nothing is cleared or defaulted first. This is called once, from
    /// `from_rng`, on a game whose fields were set two lines earlier: the
    /// snake is already empty, the queue is already empty and the
    /// direction is already `Right`. Setting them a second time would be
    /// three lines no test could own — take any of them away and the
    /// whole suite still passes (`known-issues.md` lesson 51). Which way
    /// a new snake faces is the struct literal's to say, and the test
    /// `a_new_game_has_a_snake_of_three_in_the_middle_facing_right`
    /// holds it to it there.
    fn init_snake(&mut self) {
        let rows = i32::try_from(GRID_ROWS).unwrap_or(1);
        let cols = i32::try_from(GRID_COLS).unwrap_or(1);
        for i in 0..START_LENGTH {
            self.snake
                .push(Pos::new(rows / 2, (cols / 2).saturating_sub(i)));
        }
    }

    /// Start again, keeping what a restart should keep: the high score, the
    /// difficulty, the wrap switch, and the window size the frame is read
    /// against.
    pub fn restart(&mut self) {
        let seed = self.rng.next_u64();
        let high = self.high_score;
        let difficulty = self.difficulty;
        let wrap = self.wrap_mode;
        let size = self.size;
        *self = Self::with_seed(seed);
        self.high_score = high;
        self.difficulty = difficulty;
        self.wrap_mode = wrap;
        self.size = size;
    }

    /// The size the next click will be read against.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// The size the last frame was drawn at.
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    // ── Food ────────────────────────────────────────────────────────

    /// Every square the snake is not on.
    ///
    /// The old code drew a square at random and retried until it found a free
    /// one, which never ends once the snake covers the board — and covering
    /// the board is winning, so the game hung at the moment it was won. A scan
    /// answers "there are none" as readily as it answers "here is one".
    #[must_use]
    pub fn free_cells(&self) -> Vec<Pos> {
        let mut free = Vec::new();
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let pos = Pos::new(
                    i32::try_from(row).unwrap_or(0),
                    i32::try_from(col).unwrap_or(0),
                );
                if !self.snake.contains(&pos) {
                    free.push(pos);
                }
            }
        }
        free
    }

    /// Put a normal food somewhere free, or declare the game won if there is
    /// nowhere free left.
    fn spawn_food(&mut self) {
        let free = self.free_cells();
        let Some(pos) = self.pick(&free) else {
            self.win();
            return;
        };
        self.food = Food {
            pos,
            kind: FoodKind::Normal,
            ticks_remaining: 0,
        };
    }

    /// Put a bonus food somewhere free that is not where the normal food is.
    fn spawn_bonus_food(&mut self) {
        let food_pos = self.food.pos;
        let free: Vec<Pos> = self
            .free_cells()
            .into_iter()
            .filter(|p| *p != food_pos)
            .collect();
        if let Some(pos) = self.pick(&free) {
            self.bonus_food = Some(Food {
                pos,
                kind: FoodKind::Bonus,
                ticks_remaining: BONUS_FOOD_LIFETIME,
            });
        }
    }

    /// One of `cells`, uniformly, or `None` if there are none.
    ///
    /// No emptiness guard in front of this: `below` answers nought for a bound
    /// of nought without drawing at all, and the nought-th of nothing is
    /// `None`, so the empty case is already the answer. A guard in front of a
    /// rule that already holds is a line no test can own (`known-issues.md`
    /// lesson 51).
    fn pick(&mut self, cells: &[Pos]) -> Option<Pos> {
        cells.get(self.rng.below(cells.len())).copied()
    }

    // ── Keyboard ────────────────────────────────────────────────────

    /// Answer a key press. A key that changed nothing is left for whoever else
    /// is listening rather than swallowed.
    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        if !ev.pressed {
            return EventResult::Ignored;
        }
        // A shortcut belongs to whoever is listening for shortcuts, not to the
        // game: Ctrl-R is the browser's reload, not a restart.
        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {
            return EventResult::Ignored;
        }

        // These four work on every screen, so the difficulty and the restart
        // can be chosen separately. Picking a difficulty used to restart the
        // game as a side effect, which meant there was no way to say "same
        // game, different speed" and no way to say "again, same speed".
        match ev.key {
            Key::R => {
                self.restart();
                return EventResult::Consumed;
            }
            Key::B => {
                self.wrap_mode = !self.wrap_mode;
                return EventResult::Consumed;
            }
            Key::Num1 | Key::Num2 | Key::Num3 => {
                if let Some(d) = DIFFICULTIES.iter().find(|d| d.key() == ev.key) {
                    self.difficulty = *d;
                    return EventResult::Consumed;
                }
                return EventResult::Ignored;
            }
            _ => {}
        }

        match self.state {
            GameState::Playing => self.key_playing(ev.key),
            GameState::Paused => self.key_paused(ev.key),
            GameState::GameOver | GameState::Won => self.key_finished(ev.key),
        }
    }

    fn key_playing(&mut self, key: Key) -> EventResult {
        if let Some(dir) = Direction::from_key(key) {
            return self.queue_direction(dir);
        }
        match key {
            Key::P | Key::Escape => {
                self.state = GameState::Paused;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn key_paused(&mut self, key: Key) -> EventResult {
        match key {
            Key::P | Key::Escape => {
                self.state = GameState::Playing;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn key_finished(&mut self, key: Key) -> EventResult {
        match key {
            Key::Enter | Key::Space => {
                self.restart();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Ask for a turn.
    ///
    /// Refused if the queue is full, if it is the way the snake is already
    /// going, or if it is a reversal — measured against the *last* turn asked
    /// for rather than the current direction, so that two quick turns that are
    /// each legal in sequence both land.
    pub fn queue_direction(&mut self, new_dir: Direction) -> EventResult {
        if self.dir_queue.len() >= MAX_DIR_QUEUE {
            return EventResult::Ignored;
        }
        let effective = self.dir_queue.last().copied().unwrap_or(self.direction);
        if new_dir == effective || new_dir.is_opposite(effective) {
            return EventResult::Ignored;
        }
        self.dir_queue.push(new_dir);
        EventResult::Consumed
    }

    // ── The clock ───────────────────────────────────────────────────

    /// Spend `elapsed_ms` of game time.
    ///
    /// The time is banked and spent a move at a time, but no more than
    /// [`MAX_CATCH_UP_MOVES`] of it per call: a window that was hidden for ten
    /// seconds used to come back with four hundred moves of credit and play
    /// them out in a single frame. What cannot be caught up on is dropped, not
    /// carried, because carrying it only defers the same stampede.
    pub fn handle_tick(&mut self, elapsed_ms: u64) -> EventResult {
        if self.state != GameState::Playing {
            return EventResult::Ignored;
        }
        let interval = u64::from(self.current_interval_ms()).max(1);
        self.accumulated_ms = self.accumulated_ms.saturating_add(elapsed_ms);

        // The type is written out rather than inferred: the comparison
        // against `MAX_CATCH_UP_MOVES` below was the only thing pinning
        // it, so touching the loop's condition turned `moves` into an
        // ambiguous integer and the file stopped compiling.
        let mut moves: u32 = 0;
        while self.accumulated_ms >= interval
            && moves < MAX_CATCH_UP_MOVES
            && self.state == GameState::Playing
        {
            self.accumulated_ms = self.accumulated_ms.saturating_sub(interval);
            self.step_once();
            moves = moves.saturating_add(1);
        }
        // Drop the rest. Keeping it would move the stampede to the next tick.
        self.accumulated_ms = self.accumulated_ms.checked_rem(interval).unwrap_or(0);

        if moves == 0 {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// One move of the snake, and everything that hangs off one move.
    fn step_once(&mut self) {
        self.total_ticks = self.total_ticks.saturating_add(1);
        self.ticks_since_food = self.ticks_since_food.saturating_add(1);
        self.pulse_counter = self.pulse_counter.wrapping_add(1);

        // No reversal check here: `queue_direction` refuses a reversal against
        // the last turn already queued, so the queue is a legal chain from the
        // current direction and nothing in between can change it. A second
        // check would be a guard in front of a rule that already holds, which
        // is a line no test can own (`known-issues.md` lesson 51).
        if !self.dir_queue.is_empty() {
            self.direction = self.dir_queue.remove(0);
        }

        self.move_snake();

        if let Some(bonus) = &mut self.bonus_food {
            bonus.ticks_remaining = bonus.ticks_remaining.saturating_sub(1);
            if bonus.ticks_remaining == 0 {
                self.bonus_food = None;
            }
        }
    }

    /// Move the head one square and bring the rest along.
    fn move_snake(&mut self) {
        let Some(head) = self.snake.first().copied() else {
            return;
        };
        let mut new_head = head.moved(self.direction);

        if self.wrap_mode {
            new_head = new_head.wrapped();
        } else if !new_head.in_bounds() {
            self.game_over();
            return;
        }

        // Every segment counts, including the tail: it only moves away if the
        // snake is not growing, and whether it is growing is not known until
        // after the head has landed.
        if self.snake.contains(&new_head) {
            self.game_over();
            return;
        }

        self.snake.insert(0, new_head);

        let ate_normal = new_head == self.food.pos;
        let ate_bonus = self.bonus_food.is_some_and(|b| new_head == b.pos);

        if ate_normal {
            self.eat_normal_food();
        } else if ate_bonus {
            self.eat_bonus_food();
        } else {
            self.snake.pop();
        }
    }

    /// Whether the last meal was recent enough to keep a streak alive.
    fn streak_is_alive(&self) -> bool {
        self.ticks_since_food <= STREAK_WINDOW_TICKS
    }

    /// The multiplier the current streak is worth.
    fn multiplier(&self) -> u32 {
        if self.streak >= STREAK_THRESHOLD {
            STREAK_MULTIPLIER
        } else {
            1
        }
    }

    fn eat_normal_food(&mut self) {
        self.extend_streak();
        self.score = self
            .score
            .saturating_add(NORMAL_FOOD_POINTS.saturating_mul(self.multiplier()));
        self.foods_eaten = self.foods_eaten.saturating_add(1);

        self.spawn_food();

        if self.bonus_food.is_none()
            && self.state == GameState::Playing
            && self.rng.chance_in(1, BONUS_SPAWN_CHANCE)
        {
            self.spawn_bonus_food();
        }
    }

    fn eat_bonus_food(&mut self) {
        // Same rule as a normal meal. It used to be `streak += 1` with no
        // window check at all, so a bonus eaten after a minute of wandering
        // revived a streak that had long since lapsed.
        self.extend_streak();
        self.score = self
            .score
            .saturating_add(BONUS_FOOD_POINTS.saturating_mul(self.multiplier()));
        self.bonus_eaten = self.bonus_eaten.saturating_add(1);
        self.foods_eaten = self.foods_eaten.saturating_add(1);
        self.bonus_food = None;
    }

    /// Count this meal towards the streak, or start a new streak if the last
    /// one had lapsed. Written once because both meals obey it.
    fn extend_streak(&mut self) {
        self.streak = if self.streak_is_alive() {
            self.streak.saturating_add(1)
        } else {
            1
        };
        self.ticks_since_food = 0;
    }

    fn game_over(&mut self) {
        self.state = GameState::GameOver;
        self.high_score = self.high_score.max(self.score);
    }

    fn win(&mut self) {
        self.state = GameState::Won;
        self.high_score = self.high_score.max(self.score);
    }

    // ── Queries ─────────────────────────────────────────────────────

    #[must_use]
    pub fn snake_length(&self) -> usize {
        self.snake.len()
    }

    #[must_use]
    pub fn current_speed_level(&self) -> u32 {
        speed_level(self.score)
    }

    #[must_use]
    pub fn current_interval_ms(&self) -> u32 {
        tick_interval_ms(self.difficulty, self.current_speed_level())
    }

    /// Whether the game is over, either way round.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(self.state, GameState::GameOver | GameState::Won)
    }
}

impl Default for SnakeApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── The pointer ─────────────────────────────────────────────────────

impl SnakeApp {
    /// Answer a click.
    ///
    /// `handle_event` used to match `Event::Key` and `Event::Tick` and nothing
    /// else, so every switch in this program was keyboard-only — and on the
    /// game-over screen, which a new player reaches inside a minute, the only
    /// way on was to already know that Enter does it.
    ///
    /// What a click lands on is read out of the frame the drawing pass built,
    /// so a control is clickable exactly where its ink is.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let (w, h) = self.size;
        let Some(target) = self.frame(w, h).hit_test(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        self.activate(target)
    }

    /// Do what the thing under the pointer does.
    pub fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Cell(row, col) => self.steer_towards(row, col),
            Target::Pause => self.toggle_pause(),
            Target::Restart => {
                self.restart();
                EventResult::Consumed
            }
            Target::Wrap => {
                self.wrap_mode = !self.wrap_mode;
                EventResult::Consumed
            }
            Target::Level(level) => {
                self.difficulty = level;
                EventResult::Consumed
            }
        }
    }

    /// Pause, or unpause. A finished game has nothing to pause, so the switch
    /// is refused there rather than quietly putting a dead game into a state it
    /// cannot leave.
    fn toggle_pause(&mut self) -> EventResult {
        match self.state {
            GameState::Playing => {
                self.state = GameState::Paused;
                EventResult::Consumed
            }
            GameState::Paused => {
                self.state = GameState::Playing;
                EventResult::Consumed
            }
            GameState::GameOver | GameState::Won => EventResult::Ignored,
        }
    }

    /// Turn towards a square of the board.
    ///
    /// A click asks for up to two turns — one for each axis it is away from
    /// the head — and gets whichever of them the snake may take.
    ///
    /// Which one that is is not a choice to be made here, and deliberately is
    /// not made here: the snake is travelling along one axis, so a turn along
    /// *that* axis is either the way it is already going or a reversal, and
    /// both are refused. The perpendicular axis is the only one that can be
    /// taken, and the sign of the distance along it says which way. Sorting the
    /// two by distance first would be a tie-break for a tie that cannot happen
    /// — a line no test could own (`known-issues.md` lesson 51).
    ///
    /// A click on the square the head is on, or straight along the way it is
    /// going, asks for nothing and is left alone.
    pub fn steer_towards(&mut self, row: usize, col: usize) -> EventResult {
        if self.state != GameState::Playing {
            return EventResult::Ignored;
        }
        let Some(head) = self.snake.first().copied() else {
            return EventResult::Ignored;
        };
        let (Ok(row), Ok(col)) = (i32::try_from(row), i32::try_from(col)) else {
            return EventResult::Ignored;
        };
        let dr = row.saturating_sub(head.row);
        let dc = col.saturating_sub(head.col);
        let mut wanted = Vec::new();
        if dr != 0 {
            wanted.push(if dr < 0 {
                Direction::Up
            } else {
                Direction::Down
            });
        }
        if dc != 0 {
            wanted.push(if dc < 0 {
                Direction::Left
            } else {
                Direction::Right
            });
        }
        for dir in wanted {
            if self.queue_direction(dir) == EventResult::Consumed {
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }
}

// ── Drawing ─────────────────────────────────────────────────────────

/// The size of a bonus food, as a share of its square, as it pulses.
///
/// A table rather than a sine, because the pulse advances one step per *move*
/// and there are only eight steps to it — spelling them out is shorter than the
/// arithmetic that would produce them, and cannot produce a scale outside the
/// square.
const PULSE: [f32; 8] = [0.72, 0.78, 0.84, 0.90, 0.96, 0.90, 0.84, 0.78];

impl SnakeApp {
    /// Draw one frame, recording where every control went as it goes.
    ///
    /// Everything is solved from `width` and `height`, which is the whole
    /// difference from the program this replaces: that one worked out a window
    /// size of its own from a fixed 24-pixel square and drew against those
    /// numbers, so a window the user made smaller did not shrink the board, it
    /// cut it off.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let l = Layout::new(width, height);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        self.draw_header(&mut f, &l);
        let (_, stats) = l.split(self.stats_width(&l));
        let board = self.board(&l);
        self.draw_board(&mut f, &board);
        self.draw_stats(&mut f, &l, stats);
        self.draw_footer(&mut f, &l);
        self.draw_overlay(&mut f, &l, &board);
        f
    }

    /// Where the board goes in a window laid out like `l`.
    ///
    /// The one place the answer is worked out. The drawing pass and the hit
    /// test both come here, so they cannot disagree about which square a point
    /// is in (`known-issues.md` lesson 63).
    #[must_use]
    pub fn board(&self, l: &Layout) -> Board {
        let (area, _) = l.split(self.stats_width(l));
        Board::new(area, GRID_COLS, GRID_ROWS)
    }

    /// The stats rows, as they are written.
    #[must_use]
    pub fn stats_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Length", self.snake_length().to_string()),
            ("Eaten", self.foods_eaten.to_string()),
            ("Bonus", self.bonus_eaten.to_string()),
            ("Streak", self.streak.to_string()),
            ("Speed", self.current_speed_level().to_string()),
            ("Moves", self.total_ticks.to_string()),
        ]
    }

    /// How wide the stats panel would like to be: its own widest row, measured.
    ///
    /// The panel used to be `STATS_PANEL_WIDTH = 180.0` whatever was in it, so
    /// a small window gave 180 pixels to six short numbers and the board went
    /// without.
    #[must_use]
    pub fn stats_width(&self, l: &Layout) -> f32 {
        let heading = text::measure(STATS_HEADING, l.small, FontWeightHint::Bold);
        let widest = self
            .stats_rows()
            .iter()
            .map(|(name, value)| {
                text::measure(name, l.small, FontWeightHint::Regular)
                    + l.pad * 2.0
                    + text::measure(value, l.small, FontWeightHint::Bold)
            })
            .fold(heading, f32::max);
        widest + l.pad * 2.0
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, CornerRadii::ZERO);
        let mut rest = inset_x(l.header, l.pad * 2.0);

        // The right-hand items take what they measure and the score gets what
        // is left — and is told to stop there, which is what `max_width` is for
        // and what every string in this program was missing.
        let best = format!("Best {}", self.high_score);
        let best_w = text::measure(&best, l.font, FontWeightHint::Bold);
        let best_rect = take_right(&mut rest, best_w, l.pad * 2.0);
        label_left(
            f,
            &Label {
                text: &best,
                size: l.font,
                weight: FontWeightHint::Bold,
                color: YELLOW,
            },
            best_rect,
        );

        let state = self.state_label();
        let state_w = text::measure(state, l.font, FontWeightHint::Bold);
        let state_rect = take_right(&mut rest, state_w, l.pad * 2.0);
        label_left(
            f,
            &Label {
                text: state,
                size: l.font,
                weight: FontWeightHint::Bold,
                color: self.state_color(),
            },
            state_rect,
        );

        let score = format!("Score {}", self.score);
        label_left(
            f,
            &Label {
                text: &score,
                size: l.big,
                weight: FontWeightHint::Bold,
                color: TEXT_COLOR,
            },
            rest,
        );
    }

    /// The word for the state of play.
    #[must_use]
    pub fn state_label(&self) -> &'static str {
        match self.state {
            GameState::Playing => "Playing",
            GameState::Paused => "Paused",
            GameState::GameOver => "Game over",
            GameState::Won => "You win",
        }
    }

    fn state_color(&self) -> Color {
        match self.state {
            GameState::Playing => GREEN,
            GameState::Paused => PEACH,
            GameState::GameOver => RED,
            GameState::Won => MAUVE,
        }
    }

    /// The squares, then the food, then the snake over both.
    ///
    /// No guard for a board with no room in it: every rectangle below comes
    /// from `cell_rect`, which answers nothing for a square of no size, and
    /// both `fill` and `Frame::hit` drop a rectangle of no size. A guard in
    /// front of a rule that already holds is a line no test can own
    /// (`known-issues.md` lesson 51).
    fn draw_board(&self, f: &mut Frame<Target>, b: &Board) {
        let radius = CornerRadii::all(b.cell * 0.18);
        for row in 0..b.rows {
            for col in 0..b.cols {
                let r = b.cell_rect(row, col);
                fill(f, r, SURFACE0, radius);
                f.hit(Target::Cell(row, col), b.cell_hit(row, col));
            }
        }

        self.draw_food(f, b);

        // The snake last, so it is drawn over the food it is about to eat
        // rather than under it, and head last of all.
        for (i, seg) in self.snake.iter().enumerate().rev() {
            let (Ok(row), Ok(col)) = (usize::try_from(seg.row), usize::try_from(seg.col)) else {
                continue;
            };
            let r = b.cell_rect(row, col);
            if r.is_empty() {
                continue;
            }
            let head = i == 0;
            fill(f, r, if head { GREEN } else { TEAL }, radius);
            if head {
                // Two eyes, so which way the snake is pointing is visible on
                // the board and not only in the direction it moves next.
                self.draw_eyes(f, r, b.cell);
            }
        }
    }

    fn draw_food(&self, f: &mut Frame<Target>, b: &Board) {
        let normal = self.food.pos;
        if let (Ok(row), Ok(col)) = (usize::try_from(normal.row), usize::try_from(normal.col)) {
            let r = shrink(b.cell_rect(row, col), b.cell * 0.15);
            fill(f, r, RED, CornerRadii::all(r.w / 2.0));
        }
        let Some(bonus) = self.bonus_food else {
            return;
        };
        let (Ok(row), Ok(col)) = (
            usize::try_from(bonus.pos.row),
            usize::try_from(bonus.pos.col),
        ) else {
            return;
        };
        let cell = b.cell_rect(row, col);
        // The pulse shrinks the bonus rather than the square it is in, so a
        // bonus about to expire is visibly smaller and never overlaps its
        // neighbours.
        let scale = self.pulse_scale();
        let r = shrink(cell, cell.w * (1.0 - scale) / 2.0);
        fill(f, r, YELLOW, CornerRadii::all(r.w / 2.0));
        stroke(
            f,
            r,
            PEACH,
            (b.cell * 0.07).max(1.0),
            CornerRadii::all(r.w / 2.0),
        );
    }

    /// How big the bonus food is drawn this move, as a share of its square.
    #[must_use]
    pub fn pulse_scale(&self) -> f32 {
        let i = usize::try_from(self.pulse_counter % 8).unwrap_or(0);
        PULSE.get(i).copied().unwrap_or(0.8)
    }

    fn draw_eyes(&self, f: &mut Frame<Target>, r: Rect, cell: f32) {
        let size = (cell * 0.16).max(1.0);
        let (dr, dc) = self.direction.delta();
        let (fx, fy) = (dc_f32(dc), dc_f32(dr));
        let (cx, cy) = r.centre();
        // Forward towards the nose, and one eye out to each side of it.
        let nose_x = cx + fx * cell * 0.22;
        let nose_y = cy + fy * cell * 0.22;
        for side in [-1.0f32, 1.0] {
            let ex = nose_x + fy * side * cell * 0.20;
            let ey = nose_y - fx * side * cell * 0.20;
            fill(
                f,
                Rect::new(ex - size / 2.0, ey - size / 2.0, size, size),
                CRUST,
                CornerRadii::all(size / 2.0),
            );
        }
    }

    /// The panel of counts beside the board.
    ///
    /// A panel of no width writes nothing without being told not to: every
    /// label is placed in a rectangle cut from `area`, and `label_left` drops
    /// one of no size (`known-issues.md` lesson 51).
    fn draw_stats(&self, f: &mut Frame<Target>, l: &Layout, area: Rect) {
        fill(f, area, MANTLE, CornerRadii::all(l.pad));
        let inner = inset_x(area, l.pad);
        let line = text::line_height(l.small, FontWeightHint::Regular) + l.pad;
        let mut y = inner.y + l.pad;

        label_left(
            f,
            &Label {
                text: STATS_HEADING,
                size: l.small,
                weight: FontWeightHint::Bold,
                color: LAVENDER,
            },
            Rect::new(inner.x, y, inner.w, line),
        );
        y += line;

        for (name, value) in self.stats_rows() {
            if y + line > inner.bottom() {
                break;
            }
            let row = Rect::new(inner.x, y, inner.w, line);
            let value_w = text::measure(&value, l.small, FontWeightHint::Bold).min(inner.w);
            let mut left = row;
            let right = take_right(&mut left, value_w, l.pad);
            label_left(
                f,
                &Label {
                    text: name,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: SUBTEXT0,
                },
                left,
            );
            label_left(
                f,
                &Label {
                    text: &value,
                    size: l.small,
                    weight: FontWeightHint::Bold,
                    color: TEXT_COLOR,
                },
                right,
            );
            y += line;
        }
    }

    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, MANTLE, CornerRadii::ZERO);
        let mut rest = inset_x(l.footer, l.pad * 2.0);

        for (target, text, on) in self.switches() {
            let w = text::measure(text, l.small, FontWeightHint::Bold) + l.pad * 2.0;
            let box_rect = take_left(&mut rest, w, l.pad);
            if box_rect.is_empty() {
                continue;
            }
            let inner = inset_y(box_rect, box_rect.h * 0.18);
            fill(
                f,
                inner,
                if on { BLUE } else { SURFACE0 },
                CornerRadii::all(inner.h * 0.25),
            );
            label_centred(
                f,
                &Label {
                    text,
                    size: l.small,
                    weight: FontWeightHint::Bold,
                    color: if on { BASE } else { TEXT_COLOR },
                },
                inner,
            );
            f.hit(target, box_rect);
        }

        label_left(
            f,
            &Label {
                text: "Arrows/WASD: steer   P: pause   R: again   B: wrap",
                size: l.small,
                weight: FontWeightHint::Regular,
                color: OVERLAY0,
            },
            rest,
        );
    }

    /// The five switches the footer carries, in the order it lays them out,
    /// each with the word on it and whether it is lit.
    ///
    /// One list, read by the drawing pass. A test that wants to know what the
    /// footer offers asks the frame, not this — but the switch that is lit and
    /// the switch that is drawn cannot differ, because there is one of them.
    #[must_use]
    pub fn switches(&self) -> Vec<(Target, &'static str, bool)> {
        let mut out = vec![
            (
                Target::Pause,
                if self.state == GameState::Paused {
                    "Resume"
                } else {
                    "Pause"
                },
                self.state == GameState::Paused,
            ),
            (Target::Restart, "Restart", false),
            (Target::Wrap, "Wrap", self.wrap_mode),
        ];
        for level in DIFFICULTIES {
            out.push((
                Target::Level(level),
                level.label(),
                level == self.difficulty,
            ));
        }
        out
    }

    /// The word across the board when the game is not being played.
    ///
    /// The panel is cut from the board's own squares, so a board with no room
    /// in it gives a panel with no room in it and every label is dropped
    /// unwritten; no guard says so twice (`known-issues.md` lesson 51).
    fn draw_overlay(&self, f: &mut Frame<Target>, l: &Layout, b: &Board) {
        let (headline, hint, color) = match self.state {
            // A game in play has nothing across it: the board is the thing.
            GameState::Playing => return,
            GameState::Paused => ("Paused", "P or the Pause switch to carry on", PEACH),
            GameState::GameOver => ("Game over", "Enter or Restart to play again", RED),
            GameState::Won => ("You win", "The board is full. Enter to play again", MAUVE),
        };
        let panel = Rect::new(
            b.cells.x,
            b.cells.y + (b.cells.h - l.big * 4.0) / 2.0,
            b.cells.w,
            l.big * 4.0,
        );
        fill(f, panel, CRUST, CornerRadii::all(l.pad));
        let line = text::line_height(l.big, FontWeightHint::Bold);
        label_centred(
            f,
            &Label {
                text: headline,
                size: l.big,
                weight: FontWeightHint::Bold,
                color,
            },
            Rect::new(panel.x, panel.y + l.pad, panel.w, line),
        );
        label_centred(
            f,
            &Label {
                text: hint,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: SUBTEXT0,
            },
            Rect::new(
                panel.x,
                panel.y + l.pad + line,
                panel.w,
                (panel.h - l.pad - line).max(0.0),
            ),
        );
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

fn stroke(
    f: &mut Frame<Target>,
    r: Rect,
    color: Color,
    line_width: f32,
    corner_radii: CornerRadii,
) {
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
        corner_radii,
    });
}

/// One string and everything about how it looks, minus where it goes.
struct Label<'a> {
    text: &'a str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// The one place a `Text` command is built.
///
/// `limit` is passed straight through as `max_width`, so a caller that worked
/// out a width limit gets one the renderer will actually stop at, and the
/// overflow rule follows from it rather than being a second choice that could
/// disagree with it.
fn push_text(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, limit: f32) {
    if l.text.is_empty() || limit <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: l.text.to_string(),
        color: l.color,
        font_size: l.size,
        font_weight: l.weight,
        max_width: Some(limit),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Against the left edge of `r`, centred down it.
fn label_left(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x, r.y + (r.h - lh) / 2.0, r.w);
}

/// Centred in `r` — across from the measured width, down from the line height
/// — **and limited to `r`**.
///
/// The width that decides the centre is the width the renderer is told to stop
/// at, so the two cannot disagree; and because that width is never more than
/// `r.w`, `(r.w - w) / 2.0` is never negative, which is what keeps a string too
/// wide for its box starting at the box rather than to the left of it.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x + (r.w - w) / 2.0, r.y + (r.h - lh) / 2.0, r.w);
}

/// Take `w` off the right-hand end of `area`, leaving `gap` between what was
/// taken and what is left.
///
/// Returns [`Rect::EMPTY`] and takes nothing if there is not room, so a row
/// that runs out of space drops its right-hand items rather than drawing them
/// on top of its left-hand ones.
fn take_right(area: &mut Rect, w: f32, gap: f32) -> Rect {
    if w <= 0.0 || area.w < w {
        return Rect::EMPTY;
    }
    let taken = Rect::new(area.right() - w, area.y, w, area.h);
    area.w = (area.w - w - gap).max(0.0);
    taken
}

/// Take `w` off the left-hand end of `area`. See [`take_right`].
fn take_left(area: &mut Rect, w: f32, gap: f32) -> Rect {
    if w <= 0.0 || area.w < w {
        return Rect::EMPTY;
    }
    let taken = Rect::new(area.x, area.y, w, area.h);
    area.x += w + gap;
    area.w = (area.w - w - gap).max(0.0);
    taken
}

/// `r` with `dx` taken off each of its left and right edges.
fn inset_x(r: Rect, dx: f32) -> Rect {
    Rect::new(r.x + dx, r.y, (r.w - dx * 2.0).max(0.0), r.h)
}

/// `r` with `dy` taken off each of its top and bottom edges.
fn inset_y(r: Rect, dy: f32) -> Rect {
    Rect::new(r.x, r.y + dy, r.w, (r.h - dy * 2.0).max(0.0))
}

/// `r` with `d` taken off all four edges.
fn shrink(r: Rect, d: f32) -> Rect {
    inset_y(inset_x(r, d), d)
}

/// A one-square step as a number to multiply a distance by.
///
/// The steps are only ever `-1`, `0` or `1`, so this is exact.
fn dc_f32(v: i32) -> f32 {
    match v {
        d if d < 0 => -1.0,
        0 => 0.0,
        _ => 1.0,
    }
}

// ── Window plumbing ─────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(app: &mut SnakeApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Tick { elapsed_ms } => app.handle_tick(*elapsed_ms),
        Event::Resize { width, height } => {
            app.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// A window dimension as a length.
///
/// Window sizes are pixel counts in the thousands; the cast is written out so
/// the lint does not have to be turned off across the file to allow it.
#[expect(
    clippy::cast_precision_loss,
    reason = "a window dimension is orders of magnitude below 2^24"
)]
fn f32_from_u32(v: u32) -> f32 {
    v as f32
}

impl App for SnakeApp {
    fn title(&self) -> String {
        "Snake".to_string()
    }

    fn app_id(&self) -> String {
        "snake".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// The window is asked for a tick at the frame rate, not at the snake's
    /// speed: the snake's speed changes with the score and the difficulty, and
    /// a window that had to be re-armed every time it changed would be a second
    /// place the speed is written down.
    fn tick_interval(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against, which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for SnakeApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
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

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut game = SnakeApp::new();
    app::launch("snake", &mut game)
}

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

    /// The size the probe reads a click against, spelled once.
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    /// A game from a known seed, so a test that says "the food is here" is
    /// saying something that stays true.
    fn game() -> SnakeApp {
        SnakeApp::with_seed(7)
    }

    /// A game with the snake put where the test wants it and the food put
    /// somewhere it will not be walked into by accident.
    ///
    /// `cells` is head first, as the snake itself is.
    fn placed(cells: &[(i32, i32)], dir: Direction) -> SnakeApp {
        let mut app = game();
        app.snake = cells.iter().map(|&(r, c)| Pos::new(r, c)).collect();
        app.direction = dir;
        app.dir_queue.clear();
        app.food.pos = Pos::new(0, 0);
        assert!(
            !app.snake.contains(&app.food.pos),
            "the fixture put the food under the snake"
        );
        app
    }

    /// The head of the snake.
    fn head(app: &SnakeApp) -> Pos {
        *app.snake.first().expect("the snake has no head")
    }

    /// One move of the snake, by handing the clock exactly one interval.
    fn step(app: &mut SnakeApp) -> EventResult {
        let interval = u64::from(app.current_interval_ms());
        app.handle_tick(interval)
    }

    /// `n` moves of the snake, one call each, so the catch-up ceiling never
    /// comes into it.
    fn steps(app: &mut SnakeApp, n: u32) {
        for _ in 0..n {
            step(app);
        }
    }

    /// The layout of a window of this size.
    fn layout(size: (f32, f32)) -> Layout {
        Layout::new(size.0, size.1)
    }

    /// The board as it is drawn in a window of this size.
    fn board_at(app: &SnakeApp, size: (f32, f32)) -> Board {
        app.board(&layout(size))
    }

    /// Click the middle of a square of the board, by geometry.
    ///
    /// Deliberately not `probe::click(app, Target::Cell(row, col))`: a hit box
    /// filed under its own transpose would send that helper to the transposed
    /// box and then read the transposed target back out of it, agreeing with
    /// itself the whole way round (`known-issues.md` lesson 65).
    fn click_cell(app: &mut SnakeApp, row: usize, col: usize) -> EventResult {
        let r = board_at(app, SIZE).cell_rect(row, col);
        assert!(!r.is_empty(), "square {row},{col} is not on the board");
        let (x, y) = r.centre();
        app.click_at(x, y, MouseButton::Left, SIZE)
    }

    /// Every string the frame draws.
    fn texts(f: &Frame<Target>) -> Vec<String> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The topmost rectangle filled in `color` that covers `(x, y)`.
    fn fill_covering(f: &Frame<Target>, x: f32, y: f32, color: Color) -> Option<Rect> {
        f.commands().iter().rev().find_map(|c| match c {
            RenderCommand::FillRect {
                x: rx,
                y: ry,
                width,
                height,
                color: found,
                ..
            } if *found == color => {
                let r = Rect::new(*rx, *ry, *width, *height);
                if r.contains(x, y) { Some(r) } else { None }
            }
            _ => None,
        })
    }

    // ── The rules ───────────────────────────────────────────────────

    #[test]
    fn a_new_game_has_a_snake_of_three_in_the_middle_facing_right() {
        let app = game();
        assert_eq!(app.snake_length(), 3);
        assert_eq!(head(&app), Pos::new(10, 10));
        assert_eq!(app.snake[1], Pos::new(10, 9));
        assert_eq!(app.snake[2], Pos::new(10, 8));
        assert_eq!(app.direction, Direction::Right);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn a_move_takes_the_head_one_square_and_brings_the_tail_along() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8)], Direction::Right);
        step(&mut app);
        assert_eq!(head(&app), Pos::new(10, 11), "the head did not move");
        assert_eq!(
            app.snake.last().copied(),
            Some(Pos::new(10, 9)),
            "the tail did not follow"
        );
        assert_eq!(app.snake_length(), 3, "the snake changed length on a move");
    }

    #[test]
    fn every_direction_moves_the_snake_its_own_way() {
        // Each direction is pressed from the middle of the board, where it is
        // free to move, and asserted against the square it should reach --
        // never at the edge it stops at, where every direction looks alike
        // (`known-issues.md` lesson 70).
        for (dir, want) in [
            (Direction::Up, Pos::new(9, 10)),
            (Direction::Down, Pos::new(11, 10)),
            (Direction::Left, Pos::new(10, 9)),
            (Direction::Right, Pos::new(10, 11)),
        ] {
            let mut app = placed(&[(10, 10)], dir);
            step(&mut app);
            assert_eq!(head(&app), want, "{dir:?} went the wrong way");
        }
    }

    #[test]
    fn eating_lengthens_the_snake_and_scores() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8)], Direction::Right);
        app.food.pos = Pos::new(10, 11);
        step(&mut app);
        assert_eq!(app.snake_length(), 4, "the snake did not grow");
        assert_eq!(app.score, NORMAL_FOOD_POINTS);
        assert_eq!(app.foods_eaten, 1);
        assert_ne!(
            app.food.pos,
            Pos::new(10, 11),
            "the food that was eaten is still there"
        );
    }

    #[test]
    fn the_food_is_never_put_under_the_snake() {
        for seed in 0..200u64 {
            let app = SnakeApp::with_seed(seed);
            assert!(
                !app.snake.contains(&app.food.pos),
                "seed {seed} put the food under the snake"
            );
        }
    }

    #[test]
    fn running_into_a_wall_ends_the_game() {
        for (dir, cell) in [
            (Direction::Up, (0, 10)),
            (Direction::Down, (19, 10)),
            (Direction::Left, (10, 0)),
            (Direction::Right, (10, 19)),
        ] {
            let mut app = placed(&[cell], dir);
            step(&mut app);
            assert_eq!(
                app.state,
                GameState::GameOver,
                "walking {dir:?} out of {cell:?} did not end the game"
            );
        }
    }

    #[test]
    fn running_into_itself_ends_the_game() {
        // A ring the head is about to close: (10,10) facing down, with (11,10)
        // its own third segment.
        let mut app = placed(
            &[(10, 10), (10, 11), (11, 11), (11, 10), (12, 10)],
            Direction::Down,
        );
        step(&mut app);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn the_last_square_of_the_snake_is_as_solid_as_the_rest() {
        // The tail counts, and it has to: it only moves away if the snake
        // is not growing, and whether it is growing is not known until the
        // head has landed. A ring closed onto the middle of the snake
        // passes just as well against a check that skips the last square,
        // so this one closes onto the last square itself
        // (`known-issues.md` lesson 70).
        let mut app = placed(&[(10, 10), (10, 11), (11, 11), (11, 10)], Direction::Down);
        assert_eq!(
            app.snake.last().copied(),
            Some(Pos::new(11, 10)),
            "the square the head is about to enter is not the tail"
        );
        step(&mut app);
        assert_eq!(
            app.state,
            GameState::GameOver,
            "the head walked onto the tail and the game carried on"
        );
    }

    #[test]
    fn a_wall_is_a_doorway_in_wrap_mode() {
        let mut app = placed(&[(10, 19)], Direction::Right);
        app.wrap_mode = true;
        step(&mut app);
        assert_eq!(
            app.state,
            GameState::Playing,
            "the doorway was still a wall"
        );
        assert_eq!(
            head(&app),
            Pos::new(10, 0),
            "the snake came out somewhere else"
        );
    }

    #[test]
    fn wrapping_works_off_every_edge() {
        for (dir, from, to) in [
            (Direction::Up, (0, 4), (19, 4)),
            (Direction::Down, (19, 4), (0, 4)),
            (Direction::Left, (4, 0), (4, 19)),
            (Direction::Right, (4, 19), (4, 0)),
        ] {
            let mut app = placed(&[from], dir);
            app.wrap_mode = true;
            step(&mut app);
            assert_eq!(head(&app), Pos::new(to.0, to.1), "{dir:?} came out wrong");
        }
    }

    #[test]
    fn filling_the_board_is_a_win_and_not_a_hang() {
        // The old `random_empty_cell` drew a square at random and retried until
        // it found one the snake was not on. With the snake covering all 400
        // squares there is no such square, so the loop never ended -- the game
        // hung at the exact moment it was won. If that comes back, this test
        // does not fail, it never returns, which is what the suite's timeout is
        // for.
        let mut cells = vec![Pos::new(0, 1)];
        for row in 0..20i32 {
            for col in 0..20i32 {
                let p = Pos::new(row, col);
                if p != Pos::new(0, 0) && p != Pos::new(0, 1) {
                    cells.push(p);
                }
            }
        }
        let mut app = game();
        app.snake = cells;
        app.direction = Direction::Left;
        app.dir_queue.clear();
        app.food.pos = Pos::new(0, 0);
        assert_eq!(
            app.free_cells().len(),
            1,
            "the fixture left more than one square"
        );

        step(&mut app);

        assert_eq!(app.state, GameState::Won, "a full board is not a win");
        assert!(app.is_finished());
        assert_eq!(app.snake_length(), 400);
    }

    #[test]
    fn a_bonus_food_disappears_when_its_time_is_up() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.bonus_food = Some(Food {
            pos: Pos::new(0, 19),
            kind: FoodKind::Bonus,
            ticks_remaining: 3,
        });
        steps(&mut app, 2);
        assert!(app.bonus_food.is_some(), "the bonus went early");
        step(&mut app);
        assert!(app.bonus_food.is_none(), "the bonus outstayed its lifetime");
    }

    #[test]
    fn a_spawned_bonus_is_given_the_lifetime_it_is_meant_to_have() {
        // The test above builds its bonus by hand, so the number
        // `spawn_bonus_food` writes into a real one is not held by it: put
        // `u32::MAX` there and a hand-built bonus still expires on time.
        // This one takes the bonus the game itself placed and only moves
        // it out of the snake's way.
        let mut app = placed(&[(10, 10)], Direction::Right);
        // Wrapping, so a snake left running for a whole lifetime goes
        // round row ten for ever instead of into the right-hand wall.
        app.wrap_mode = true;
        app.spawn_bonus_food();
        let mut bonus = app.bonus_food.expect("no bonus was placed at all");
        assert_eq!(
            bonus.ticks_remaining, BONUS_FOOD_LIFETIME,
            "a fresh bonus was not given the stated lifetime"
        );
        bonus.pos = Pos::new(0, 5);
        app.bonus_food = Some(bonus);
        steps(&mut app, BONUS_FOOD_LIFETIME.saturating_sub(1));
        assert!(
            app.bonus_food.is_some(),
            "the bonus went a move short of its lifetime"
        );
        step(&mut app);
        assert!(
            app.bonus_food.is_none(),
            "the bonus outlived the lifetime it was given"
        );
    }

    #[test]
    fn a_bonus_is_worth_more_than_a_meal() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.bonus_food = Some(Food {
            pos: Pos::new(10, 11),
            kind: FoodKind::Bonus,
            ticks_remaining: BONUS_FOOD_LIFETIME,
        });
        step(&mut app);
        assert_eq!(app.score, BONUS_FOOD_POINTS);
        assert_eq!(app.bonus_eaten, 1);
        assert_eq!(app.foods_eaten, 1);
        assert!(app.bonus_food.is_none(), "the bonus was eaten and stayed");
    }

    #[test]
    fn a_bonus_eaten_after_dawdling_does_not_revive_a_lapsed_streak() {
        // `eat_bonus_food` used to do `streak += 1` with no window check at
        // all, while `eat_normal_food` reset the streak to 1 -- so a bonus
        // eaten after a minute of wandering counted as though it had come
        // straight after the last meal.
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.streak = 5;
        app.ticks_since_food = STREAK_WINDOW_TICKS + 1;
        app.bonus_food = Some(Food {
            pos: Pos::new(10, 11),
            kind: FoodKind::Bonus,
            ticks_remaining: BONUS_FOOD_LIFETIME,
        });
        step(&mut app);
        assert_eq!(app.streak, 1, "a lapsed streak was revived by a bonus");
    }

    #[test]
    fn a_meal_eaten_promptly_carries_the_streak_on() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.streak = 2;
        app.ticks_since_food = 1;
        app.food.pos = Pos::new(10, 11);
        step(&mut app);
        assert_eq!(app.streak, 3);
        assert_eq!(
            app.ticks_since_food, 0,
            "the clock since the meal was not reset"
        );
    }

    #[test]
    fn a_meal_on_the_last_move_of_the_window_carries_the_streak_on() {
        // The window is `<=`, so a meal eaten on the move the window runs
        // out still counts. `ticks_since_food` is stepped at the top of a
        // move and read when the meal lands, so the fixture is one short
        // of the window. Testing further in would pass just as well
        // against a window one move shorter than it says
        // (`known-issues.md` lesson 70).
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.streak = 2;
        app.ticks_since_food = STREAK_WINDOW_TICKS.saturating_sub(1);
        app.food.pos = Pos::new(10, 11);
        step(&mut app);
        assert_eq!(
            app.streak, 3,
            "a meal on the last move of the window lapsed the streak"
        );
    }

    #[test]
    fn a_meal_eaten_late_starts_the_streak_again() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.streak = 4;
        app.ticks_since_food = STREAK_WINDOW_TICKS + 1;
        app.food.pos = Pos::new(10, 11);
        step(&mut app);
        assert_eq!(app.streak, 1);
    }

    #[test]
    fn a_streak_doubles_what_a_meal_is_worth() {
        // One short of the threshold, so the meal being scored is the one that
        // reaches it exactly. Starting further along would pass just as well
        // against a threshold set one too high (`known-issues.md` lesson 70).
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.streak = STREAK_THRESHOLD - 1;
        app.ticks_since_food = 0;
        app.food.pos = Pos::new(10, 11);
        step(&mut app);
        assert_eq!(
            app.score,
            NORMAL_FOOD_POINTS * STREAK_MULTIPLIER,
            "the streak was not counted"
        );
    }

    #[test]
    fn a_meal_short_of_the_threshold_is_worth_its_face_value() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.streak = STREAK_THRESHOLD - 2;
        app.ticks_since_food = 0;
        app.food.pos = Pos::new(10, 11);
        step(&mut app);
        assert_eq!(app.score, NORMAL_FOOD_POINTS);
    }

    #[test]
    fn speed_climbs_a_level_every_fifty_points_and_then_stops() {
        assert_eq!(speed_level(0), 1);
        assert_eq!(speed_level(49), 1);
        assert_eq!(speed_level(50), 2);
        assert_eq!(speed_level(149), 3);
        assert_eq!(speed_level(u32::MAX), MAX_SPEED_LEVEL);
    }

    #[test]
    fn a_harder_difficulty_moves_the_snake_sooner() {
        assert!(
            tick_interval_ms(Difficulty::Hard, 1) < tick_interval_ms(Difficulty::Medium, 1),
            "Hard is not faster than Medium"
        );
        assert!(
            tick_interval_ms(Difficulty::Medium, 1) < tick_interval_ms(Difficulty::Easy, 1),
            "Medium is not faster than Easy"
        );
    }

    #[test]
    fn a_higher_level_moves_the_snake_sooner_but_never_below_the_floor() {
        assert!(
            tick_interval_ms(Difficulty::Easy, 3) < tick_interval_ms(Difficulty::Easy, 1),
            "levelling up did not speed the snake"
        );
        for level in 1..=MAX_SPEED_LEVEL {
            for difficulty in DIFFICULTIES {
                assert!(
                    tick_interval_ms(difficulty, level) >= MIN_INTERVAL_MS,
                    "{difficulty:?} at level {level} moves faster than the floor"
                );
            }
        }
    }

    #[test]
    fn the_high_score_survives_a_restart_and_the_score_does_not() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.score = 120;
        app.high_score = 120;
        app.restart();
        assert_eq!(app.high_score, 120, "the best score was forgotten");
        assert_eq!(app.score, 0, "the new game started with the old score");
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.snake_length(), 3);
    }

    #[test]
    fn ending_the_game_banks_the_score_as_the_best_one() {
        let mut app = placed(&[(10, 19)], Direction::Right);
        app.score = 70;
        step(&mut app);
        assert_eq!(app.state, GameState::GameOver);
        assert_eq!(app.high_score, 70);
    }

    #[test]
    fn a_worse_game_does_not_lower_the_best_score() {
        let mut app = placed(&[(10, 19)], Direction::Right);
        app.score = 10;
        app.high_score = 500;
        step(&mut app);
        assert_eq!(app.high_score, 500);
    }

    #[test]
    fn a_restart_keeps_the_difficulty_and_the_wrap_switch() {
        let mut app = game();
        app.difficulty = Difficulty::Hard;
        app.wrap_mode = true;
        app.restart();
        assert_eq!(
            app.difficulty,
            Difficulty::Hard,
            "the difficulty was thrown away by the restart"
        );
        assert!(
            app.wrap_mode,
            "the wrap switch was thrown away by the restart"
        );
    }

    #[test]
    fn a_restart_deals_a_different_board() {
        let mut app = game();
        let first = app.food.pos;
        let mut differed = false;
        for _ in 0..8 {
            app.restart();
            if app.food.pos != first {
                differed = true;
                break;
            }
        }
        assert!(differed, "every restart placed the food in the same square");
    }

    // ── The clock ───────────────────────────────────────────────────

    #[test]
    fn time_short_of_a_move_is_banked_rather_than_lost() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        let interval = u64::from(app.current_interval_ms());
        assert_eq!(
            app.handle_tick(interval / 2),
            EventResult::Ignored,
            "half an interval moved the snake"
        );
        assert_eq!(app.total_ticks, 0);
        assert_eq!(
            app.handle_tick(interval - interval / 2),
            EventResult::Consumed,
            "the banked half was thrown away"
        );
        assert_eq!(app.total_ticks, 1);
    }

    #[test]
    fn a_stall_moves_the_snake_at_most_twice() {
        // `handle_tick` used to bank `elapsed_ms` and subtract *one* interval
        // per call, so a window hidden for ten seconds came back with 9.85
        // seconds of credit and the snake then moved once per frame until it
        // had spent them -- usually into a wall, always without the player
        // having seen it.
        let mut app = placed(&[(10, 2)], Direction::Right);
        let interval = u64::from(app.current_interval_ms());
        app.handle_tick(interval * 400);
        assert_eq!(
            app.total_ticks, MAX_CATCH_UP_MOVES,
            "a stall was played out in one frame"
        );
        assert_eq!(head(&app), Pos::new(10, 4));
        assert_eq!(app.state, GameState::Playing, "the stall drove into a wall");
    }

    #[test]
    fn a_game_that_ends_part_way_through_a_catch_up_stops_there() {
        // The catch-up loop asks after every move whether there is still a
        // game to move. Without that it would walk a dead snake into the
        // wall a second time, counting a move the player never had.
        let mut app = placed(&[(10, 19)], Direction::Right);
        let interval = u64::from(app.current_interval_ms());
        app.handle_tick(interval.saturating_mul(10));
        assert_eq!(
            app.state,
            GameState::GameOver,
            "the snake was against the right-hand wall and survived"
        );
        assert_eq!(
            app.total_ticks, 1,
            "the catch-up went on moving a snake that had already died"
        );
    }

    #[test]
    fn what_could_not_be_caught_up_on_is_dropped_rather_than_carried() {
        let mut app = placed(&[(10, 2)], Direction::Right);
        let interval = u64::from(app.current_interval_ms());
        app.handle_tick(interval * 400);
        let after_stall = app.total_ticks;
        assert_eq!(
            app.handle_tick(0),
            EventResult::Ignored,
            "the leftover credit was carried into the next frame"
        );
        assert_eq!(app.total_ticks, after_stall);
    }

    #[test]
    fn the_clock_runs_only_while_the_game_is_being_played() {
        for state in [GameState::Paused, GameState::GameOver, GameState::Won] {
            let mut app = placed(&[(10, 10)], Direction::Right);
            app.state = state;
            let interval = u64::from(app.current_interval_ms());
            assert_eq!(
                app.handle_tick(interval * 10),
                EventResult::Ignored,
                "the clock ran while {state:?}"
            );
            assert_eq!(app.total_ticks, 0, "the snake moved while {state:?}");
            assert_eq!(head(&app), Pos::new(10, 10));
            // And banked nothing while it waited, which is a separate rule
            // from the one above and needs its own check. The catch-up loop
            // inside `handle_tick` asks the state as well, so the count above
            // stays at nought whether or not the guard at the top of the
            // function is there; what only that guard does is keep the time
            // out of the bank. Ten seconds of it would be dropped as
            // uncatchable anyway — it is the *nearly a move* below that shows
            // the difference, because that is the part which survives to be
            // spent.
            app.handle_tick(interval.saturating_sub(1));
            app.state = GameState::Playing;
            app.handle_tick(1);
            assert_eq!(
                app.total_ticks, 0,
                "the time that passed while {state:?} was banked and spent \
                 on the first millisecond of play after it"
            );
        }
    }

    #[test]
    fn the_clock_advances_the_count_rather_than_setting_it() {
        let mut app = placed(&[(10, 4)], Direction::Right);
        steps(&mut app, 3);
        assert_eq!(app.total_ticks, 3, "the moves were not counted up");
        assert_eq!(head(&app), Pos::new(10, 7));
    }

    #[test]
    fn the_bonus_pulse_follows_the_moves_and_not_the_frames() {
        // The pulse used to advance once per *frame*, so it beat at whatever
        // rate the window happened to redraw at rather than at the game's own
        // speed.
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert_eq!(app.pulse_counter, 0);
        app.handle_tick(1);
        assert_eq!(
            app.pulse_counter, 0,
            "a frame with no move advanced the pulse"
        );
        step(&mut app);
        assert_eq!(app.pulse_counter, 1, "a move did not advance the pulse");
    }

    // ── The keyboard ────────────────────────────────────────────────

    #[test]
    fn the_arrows_steer_the_snake() {
        // Each arrow is pressed while the snake is going a way it may legally
        // turn from, so a refusal cannot be mistaken for a turn.
        for (key, want) in [
            (Key::Up, Direction::Up),
            (Key::Down, Direction::Down),
            (Key::Left, Direction::Left),
            (Key::Right, Direction::Right),
        ] {
            let start = if matches!(want, Direction::Up | Direction::Down) {
                Direction::Right
            } else {
                Direction::Up
            };
            let mut app = placed(&[(10, 10)], start);
            assert_eq!(
                probe::key(&mut app, &probe::press(key)),
                EventResult::Consumed
            );
            assert_eq!(
                app.dir_queue,
                vec![want],
                "{key:?} asked for the wrong turn"
            );
        }
    }

    #[test]
    fn wasd_steers_the_snake_as_the_arrows_do() {
        for (key, want) in [
            (Key::W, Direction::Up),
            (Key::S, Direction::Down),
            (Key::A, Direction::Left),
            (Key::D, Direction::Right),
        ] {
            let start = if matches!(want, Direction::Up | Direction::Down) {
                Direction::Right
            } else {
                Direction::Up
            };
            let mut app = placed(&[(10, 10)], start);
            assert_eq!(
                probe::key(&mut app, &probe::press(key)),
                EventResult::Consumed
            );
            assert_eq!(
                app.dir_queue,
                vec![want],
                "{key:?} asked for the wrong turn"
            );
        }
    }

    #[test]
    fn a_turn_reaches_the_snake_on_its_next_move() {
        let mut app = placed(&[(10, 10), (10, 9)], Direction::Right);
        probe::key(&mut app, &probe::press(Key::Up));
        step(&mut app);
        assert_eq!(
            app.direction,
            Direction::Up,
            "the queued turn was not taken"
        );
        assert_eq!(head(&app), Pos::new(9, 10));
        assert!(app.dir_queue.is_empty(), "the turn stayed in the queue");
    }

    #[test]
    fn the_snake_may_not_turn_back_on_itself() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8)], Direction::Right);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Left)),
            EventResult::Ignored,
            "the snake was allowed to reverse into itself"
        );
        assert!(app.dir_queue.is_empty());
        step(&mut app);
        assert_eq!(app.direction, Direction::Right);
    }

    #[test]
    fn the_way_the_snake_is_already_going_is_not_a_turn() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Right)),
            EventResult::Ignored
        );
        assert!(app.dir_queue.is_empty(), "a turn to nowhere was queued");
    }

    #[test]
    fn two_quick_turns_both_land() {
        // Measured against the *last* turn asked for rather than the current
        // direction: right-then-up-then-left is legal in sequence, and a check
        // against the current direction would have refused the second.
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Up)),
            EventResult::Consumed
        );
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Left)),
            EventResult::Consumed,
            "the second turn was measured against the direction rather than the first"
        );
        assert_eq!(app.dir_queue, vec![Direction::Up, Direction::Left]);
        step(&mut app);
        assert_eq!(head(&app), Pos::new(9, 10));
        step(&mut app);
        assert_eq!(head(&app), Pos::new(9, 9));
    }

    #[test]
    fn the_turn_queue_has_a_bottom() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        probe::key(&mut app, &probe::press(Key::Up));
        probe::key(&mut app, &probe::press(Key::Left));
        assert_eq!(app.dir_queue.len(), MAX_DIR_QUEUE);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Down)),
            EventResult::Ignored,
            "a third turn was taken on top of a full queue"
        );
        assert_eq!(app.dir_queue.len(), MAX_DIR_QUEUE);
    }

    #[test]
    fn p_pauses_and_unpauses() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::P)),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Paused);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::P)),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn escape_pauses_and_unpauses_too() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(app.state, GameState::Paused);
        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn an_arrow_does_nothing_while_the_game_is_paused() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.state = GameState::Paused;
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Up)),
            EventResult::Ignored,
            "a paused game was steered"
        );
        assert!(app.dir_queue.is_empty());
    }

    #[test]
    fn enter_starts_again_once_the_game_is_over() {
        for state in [GameState::GameOver, GameState::Won] {
            let mut app = placed(&[(10, 10)], Direction::Right);
            app.state = state;
            app.score = 90;
            assert_eq!(
                probe::key(&mut app, &probe::press(Key::Enter)),
                EventResult::Consumed
            );
            assert_eq!(
                app.state,
                GameState::Playing,
                "Enter did not start again from {state:?}"
            );
            assert_eq!(app.score, 0, "the new game started with the old score");
            assert_eq!(app.snake_length(), 3, "the new game kept the old snake");
        }
    }

    #[test]
    fn enter_does_nothing_while_the_game_is_still_being_played() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8)], Direction::Right);
        app.score = 40;
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Enter)),
            EventResult::Ignored,
            "Enter threw away a game that was still going"
        );
        assert_eq!(app.score, 40);
    }

    #[test]
    fn r_starts_again_at_any_time() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
        app.score = 40;
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::R)),
            EventResult::Consumed
        );
        assert_eq!(app.score, 0);
        assert_eq!(app.snake_length(), 3);
    }

    #[test]
    fn b_turns_the_walls_into_doorways_and_back() {
        // `wrap_mode` was a documented feature with no key bound to it at all,
        // so the only way to reach it was to edit the source.
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert!(!app.wrap_mode);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::B)),
            EventResult::Consumed
        );
        assert!(app.wrap_mode, "B did not turn the walls into doorways");
        probe::key(&mut app, &probe::press(Key::B));
        assert!(!app.wrap_mode, "B did not turn them back");
    }

    #[test]
    fn a_difficulty_key_changes_the_speed_without_starting_a_new_game() {
        // The number keys used to set the difficulty *and* restart, so there
        // was no way to say "same game, different speed" and no way to say
        // "again, same speed".
        for (key, want) in [
            (Key::Num1, Difficulty::Easy),
            (Key::Num2, Difficulty::Medium),
            (Key::Num3, Difficulty::Hard),
        ] {
            let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
            app.score = 60;
            assert_eq!(
                probe::key(&mut app, &probe::press(key)),
                EventResult::Consumed
            );
            assert_eq!(app.difficulty, want, "{key:?} picked the wrong difficulty");
            assert_eq!(app.score, 60, "{key:?} threw the game away");
            assert_eq!(app.snake_length(), 4, "{key:?} threw the snake away");
        }
    }

    #[test]
    fn a_difficulty_key_works_on_the_game_over_screen_too() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.state = GameState::GameOver;
        probe::key(&mut app, &probe::press(Key::Num3));
        assert_eq!(app.difficulty, Difficulty::Hard);
        assert_eq!(
            app.state,
            GameState::GameOver,
            "picking a difficulty restarted the game by itself"
        );
    }

    #[test]
    fn the_chosen_difficulty_is_what_the_snake_moves_at() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        probe::key(&mut app, &probe::press(Key::Num3));
        assert_eq!(
            app.current_interval_ms(),
            tick_interval_ms(Difficulty::Hard, 1),
            "the difficulty was recorded but not used"
        );
    }

    #[test]
    fn a_shortcut_belongs_to_whoever_is_listening_for_shortcuts() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
        assert_eq!(
            probe::key(&mut app, &probe::ctrl(Key::R)),
            EventResult::Ignored,
            "Ctrl-R restarted the game instead of reloading whatever owns the window"
        );
        assert_eq!(app.snake_length(), 4);
    }

    #[test]
    fn letting_a_key_go_is_not_pressing_it() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        let mut release = probe::press(Key::Up);
        release.pressed = false;
        assert_eq!(probe::key(&mut app, &release), EventResult::Ignored);
        assert!(app.dir_queue.is_empty(), "a key release steered the snake");
    }

    #[test]
    fn a_key_that_changes_nothing_is_left_for_whoever_wants_it() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Q)),
            EventResult::Ignored
        );
    }

    #[test]
    fn space_starts_again_once_the_game_is_over() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
        app.state = GameState::GameOver;
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Space)),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.snake_length(), 3);
    }

    // ── The pointer ─────────────────────────────────────────────────

    #[test]
    fn clicking_a_square_off_to_one_side_turns_the_snake_towards_it() {
        // Each click is made from a snake that is free to turn that way, and
        // asserted against the turn it asked for -- never at a square the snake
        // could not turn towards anyway, where a refusal and a turn look alike
        // (`known-issues.md` lesson 70).
        for (facing, cell, want) in [
            (Direction::Right, (5, 10), Direction::Up),
            (Direction::Right, (15, 10), Direction::Down),
            (Direction::Up, (10, 5), Direction::Left),
            (Direction::Up, (10, 15), Direction::Right),
        ] {
            let mut app = placed(&[(10, 10)], facing);
            assert_eq!(
                click_cell(&mut app, cell.0, cell.1),
                EventResult::Consumed,
                "going {facing:?}, the click on {cell:?} was refused"
            );
            assert_eq!(
                app.dir_queue,
                vec![want],
                "going {facing:?}, the click on {cell:?} asked for the wrong turn"
            );
        }
    }

    #[test]
    fn a_click_steers_the_snake_on_its_next_move() {
        let mut app = placed(&[(10, 10), (10, 9)], Direction::Right);
        click_cell(&mut app, 4, 10);
        step(&mut app);
        assert_eq!(
            head(&app),
            Pos::new(9, 10),
            "the click did not reach the snake"
        );
    }

    #[test]
    fn a_click_behind_the_snake_still_turns_it() {
        // Straight backwards is a reversal and refused; the click is still an
        // instruction to go that way, and the axis it *can* be taken on is.
        //
        // Both facings, because the two wanted directions are tried in a
        // fixed order — up or down first, left or right second. A snake
        // travelling sideways has its *first* choice taken, so it never
        // reaches the second, and on its own it would pass just as well
        // against code that gave up after one refusal.
        for (facing, behind, taken) in [
            (Direction::Left, (14_usize, 18_usize), Direction::Down),
            (Direction::Down, (4, 18), Direction::Right),
        ] {
            let body = match facing {
                Direction::Left => (10, 11),
                _ => (9, 10),
            };
            let mut app = placed(&[(10, 10), body], facing);
            assert_eq!(
                click_cell(&mut app, behind.0, behind.1),
                EventResult::Consumed,
                "a click behind a snake facing {facing:?} did nothing at all"
            );
            assert_eq!(
                app.dir_queue,
                vec![taken],
                "a snake facing {facing:?} did not fall back to the other axis"
            );
        }
    }

    #[test]
    fn clicking_straight_ahead_asks_for_nothing() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert_eq!(
            click_cell(&mut app, 10, 15),
            EventResult::Ignored,
            "a click along the way the snake is already going was taken as a turn"
        );
        assert!(app.dir_queue.is_empty());
    }

    #[test]
    fn clicking_the_square_the_head_is_on_asks_for_nothing() {
        // Both facings, because the square under the head is nought away along
        // *both* axes, and one facing only exercises one of the two arms that
        // have to notice that.
        //
        // The snakes are bent, and their tails are a square away on both
        // axes from their heads. A one-square snake is its own tail, so a
        // click measured from the wrong end of the snake would look exactly
        // like a click measured from the right one (`known-issues.md`
        // lesson 70).
        for (facing, body) in [
            (Direction::Right, [(10, 9), (11, 9)]),
            (Direction::Up, [(11, 10), (11, 9)]),
        ] {
            let mut app = placed(&[(10, 10), body[0], body[1]], facing);
            assert_ne!(
                app.snake.last().copied(),
                Some(Pos::new(10, 10)),
                "the tail is on the square being clicked"
            );
            assert_eq!(
                click_cell(&mut app, 10, 10),
                EventResult::Ignored,
                "a snake facing {facing:?} was steered onto its own square"
            );
            assert!(app.dir_queue.is_empty());
        }
    }

    #[test]
    fn a_finished_game_cannot_be_steered() {
        for state in [GameState::Paused, GameState::GameOver, GameState::Won] {
            let mut app = placed(&[(10, 10)], Direction::Right);
            app.state = state;
            assert_eq!(
                click_cell(&mut app, 5, 10),
                EventResult::Ignored,
                "a {state:?} game was steered"
            );
            assert!(app.dir_queue.is_empty());
        }
    }

    #[test]
    fn the_pause_switch_pauses_and_then_offers_to_carry_on() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert!(
            texts(&app.draw(SIZE)).iter().any(|t| t == "Pause"),
            "the footer does not offer to pause"
        );
        assert_eq!(probe::click(&mut app, Target::Pause), EventResult::Consumed);
        assert_eq!(app.state, GameState::Paused);
        assert!(
            texts(&app.draw(SIZE)).iter().any(|t| t == "Resume"),
            "the paused footer still says Pause"
        );
        probe::click(&mut app, Target::Pause);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn the_pause_switch_is_refused_once_the_game_is_over() {
        for state in [GameState::GameOver, GameState::Won] {
            let mut app = placed(&[(10, 10)], Direction::Right);
            app.state = state;
            assert_eq!(
                probe::click(&mut app, Target::Pause),
                EventResult::Ignored,
                "a {state:?} game was put into a pause it could not leave"
            );
            assert_eq!(app.state, state);
        }
    }

    #[test]
    fn the_restart_switch_starts_a_new_game() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
        app.score = 200;
        app.state = GameState::GameOver;
        assert_eq!(
            probe::click(&mut app, Target::Restart),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
        assert_eq!(app.snake_length(), 3);
    }

    #[test]
    fn the_wrap_switch_turns_the_walls_into_doorways() {
        let lit = |app: &SnakeApp| {
            app.switches()
                .into_iter()
                .find(|(t, _, _)| *t == Target::Wrap)
                .map(|(_, _, on)| on)
                .expect("there is no wrap switch")
        };
        let mut app = placed(&[(10, 19)], Direction::Right);
        assert!(!app.wrap_mode);
        assert!(!lit(&app), "the wrap switch was lit with the walls up");
        assert_eq!(probe::click(&mut app, Target::Wrap), EventResult::Consumed);
        assert!(app.wrap_mode, "the switch did not change the walls");
        assert!(lit(&app), "the switch does not say it is on");
        step(&mut app);
        assert_eq!(
            head(&app),
            Pos::new(10, 0),
            "the switch changed the label and not the walls"
        );
    }

    #[test]
    fn each_difficulty_switch_picks_its_own_difficulty() {
        for want in DIFFICULTIES {
            let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
            assert_eq!(
                probe::click(&mut app, Target::Level(want)),
                EventResult::Consumed
            );
            assert_eq!(
                app.difficulty, want,
                "the {want:?} switch picked something else"
            );
            assert_eq!(
                app.snake_length(),
                4,
                "the {want:?} switch threw the game away"
            );
        }
    }

    #[test]
    fn the_difficulty_switch_that_is_chosen_is_the_one_lit_up() {
        let mut app = game();
        probe::click(&mut app, Target::Level(Difficulty::Hard));
        let lit: Vec<&'static str> = app
            .switches()
            .into_iter()
            .filter(|(_, _, on)| *on)
            .map(|(_, text, _)| text)
            .collect();
        assert_eq!(lit, vec!["Hard"], "the wrong switch is lit");
    }

    #[test]
    fn the_screen_offers_every_control_the_program_has() {
        let app = game();
        let names = probe::control_names(&app);
        for wanted in ["Cell", "Pause", "Restart", "Wrap", "Level"] {
            assert!(
                names.iter().any(|n| n == wanted),
                "nothing on screen for {wanted}; there is {names:?}"
            );
        }
    }

    #[test]
    fn a_right_click_is_not_a_click() {
        let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
        assert_eq!(
            probe::click_with(&mut app, Target::Restart, MouseButton::Right),
            EventResult::Ignored
        );
        assert_eq!(app.snake_length(), 4, "a right click restarted the game");
    }

    #[test]
    fn a_click_on_nothing_is_left_for_whoever_wants_it() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert_eq!(probe::click_background(&mut app), EventResult::Ignored);
    }

    #[test]
    fn a_square_is_clickable_where_its_ink_is() {
        let app = game();
        let b = board_at(&app, SIZE);
        for (row, col) in [(0, 0), (7, 12), (19, 19)] {
            let ink = b.cell_rect(row, col);
            let hit = b.cell_hit(row, col);
            let (x, y) = ink.centre();
            assert_eq!(
                app.draw(SIZE).hit_test(x, y),
                Some(Target::Cell(row, col)),
                "the middle of square {row},{col} is not that square"
            );
            assert!(
                hit.contains(ink.x, ink.y) && hit.contains(ink.right() - 0.01, ink.bottom() - 0.01),
                "the hit box of {row},{col} does not cover its ink"
            );
        }
    }

    #[test]
    fn a_click_in_a_gap_lands_on_the_square_it_is_nearest() {
        // Nearest, not merely one of the two. A click box grown by a whole gap
        // on one side and nothing on the other is the right size and in the
        // wrong place — it swallows the gap after its square and gives away
        // the gap before it — and it still leaves no gap belonging to nobody,
        // so a test that only asked whether the middle of a gap landed
        // somewhere would pass against a board whose ink and whose click boxes
        // are a whole gap apart (`known-issues.md` lesson 65).
        //
        // A quarter of a gap either side of the ink, so the point is
        // unambiguously in one square's half of the gap.
        let app = game();
        let b = board_at(&app, SIZE);
        assert!(b.gap > 0.0, "there is no gap to click in");
        let a = b.cell_rect(5, 5);
        let (cx, cy) = a.centre();
        let quarter = b.gap / 4.0;
        let f = app.draw(SIZE);
        for (x, y, want, whereabouts) in [
            (
                a.x - quarter,
                cy,
                Target::Cell(5, 5),
                "in the gap to its left",
            ),
            (
                a.right() + quarter,
                cy,
                Target::Cell(5, 5),
                "in the gap to its right",
            ),
            (
                a.right() + b.gap - quarter,
                cy,
                Target::Cell(5, 6),
                "in the far half of the gap to its right",
            ),
            (cx, a.y - quarter, Target::Cell(5, 5), "in the gap above it"),
            (
                cx,
                a.bottom() + b.gap - quarter,
                Target::Cell(6, 5),
                "in the far half of the gap below it",
            ),
        ] {
            assert_eq!(
                f.hit_test(x, y),
                Some(want),
                "a click {whereabouts} did not land on the square it is nearest"
            );
        }
    }

    // ── Geometry ────────────────────────────────────────────────────

    #[test]
    fn the_board_fits_whatever_window_it_is_given() {
        for size in [
            (760.0, 660.0),
            (1200.0, 900.0),
            (420.0, 380.0),
            (300.0, 260.0),
            // Taller than it is wide, so the width is what runs out first and
            // the other of the two fits decides the square.
            (300.0, 1000.0),
        ] {
            let app = game();
            let b = board_at(&app, size);
            assert!(b.cell > 0.0, "at {size:?} the squares have no size");
            assert!(
                b.cells.x >= b.area.x - 0.01 && b.cells.right() <= b.area.right() + 0.01,
                "at {size:?} the board is wider than the space it was given"
            );
            assert!(
                b.cells.y >= b.area.y - 0.01 && b.cells.bottom() <= b.area.bottom() + 0.01,
                "at {size:?} the board is taller than the space it was given"
            );
            let last = b.cell_rect(19, 19);
            assert!(
                last.right() <= size.0 + 0.01 && last.bottom() <= size.1 + 0.01,
                "at {size:?} the last square is off the window"
            );
            // `cells` is where the squares are said to be and `last` is where
            // the last of them is actually put; the two are reached by
            // different sums -- one span, nineteen steps -- so agreeing is a
            // real check rather than a rule repeated to itself
            // (`known-issues.md` lesson 65).
            assert!(
                (last.right() - b.cells.right()).abs() < 0.01,
                "at {size:?} the squares stop {} short of the board's edge",
                b.cells.right() - last.right()
            );
            assert!(
                (last.bottom() - b.cells.bottom()).abs() < 0.01,
                "at {size:?} the rows stop {} short of the board's foot",
                b.cells.bottom() - last.bottom()
            );
        }
    }

    #[test]
    fn a_bigger_window_gets_bigger_squares() {
        let app = game();
        let small = board_at(&app, (400.0, 360.0));
        let large = board_at(&app, (900.0, 800.0));
        assert!(
            large.cell > small.cell,
            "the squares did not grow with the window: {} then {}",
            small.cell,
            large.cell
        );
    }

    #[test]
    fn the_squares_are_square_and_evenly_spaced() {
        for size in [(760.0, 660.0), (420.0, 380.0), (1100.0, 700.0)] {
            let app = game();
            let b = board_at(&app, size);
            let first = b.cell_rect(0, 0);
            assert!(
                (first.w - first.h).abs() < 0.001,
                "at {size:?} a square is {first:?}"
            );
            let next = b.cell_rect(0, 1);
            // Spelled out as `cell + gap` rather than as `b.step()`: `cell_rect`
            // is laid out *by* `step`, so an assertion phrased in terms of
            // `step` agrees with whatever value it takes, including one that had
            // dropped the gap (`known-issues.md` lesson 65).
            assert!(b.gap > 0.0, "at {size:?} there is no gap to count");
            assert!(
                (next.x - first.x - (b.cell + b.gap)).abs() < 0.001,
                "at {size:?} the step is not {} + {}",
                b.cell,
                b.gap
            );
            let below = b.cell_rect(1, 0);
            assert!(
                (below.y - first.y - (b.cell + b.gap)).abs() < 0.001,
                "at {size:?} the rows are spaced differently from the columns"
            );
        }
    }

    #[test]
    fn the_board_sits_in_the_middle_of_the_space_it_was_given() {
        // A board is fitted by the tighter of its two axes, so exactly one axis
        // has slack to centre in. Each fixture is checked for slack on the axis
        // it is asserting about, so neither assertion can pass vacuously
        // (`known-issues.md` lesson 70).
        let app = game();
        for size in [(1100.0, 500.0), (500.0, 900.0)] {
            let b = board_at(&app, size);
            let slack_x = b.area.w - b.cells.w;
            let slack_y = b.area.h - b.cells.h;
            if slack_x > 0.5 {
                assert!(
                    ((b.cells.x - b.area.x) - (b.area.right() - b.cells.right())).abs() < 0.01,
                    "at {size:?} the board is not centred across"
                );
            }
            if slack_y > 0.5 {
                assert!(
                    ((b.cells.y - b.area.y) - (b.area.bottom() - b.cells.bottom())).abs() < 0.01,
                    "at {size:?} the board is not centred down"
                );
            }
            assert!(
                slack_x > 0.5 || slack_y > 0.5,
                "at {size:?} neither axis had slack, so nothing was tested"
            );
        }
    }

    #[test]
    fn the_stats_panel_is_as_wide_as_what_is_written_in_it() {
        // The panel was `STATS_PANEL_WIDTH = 180.0` whatever was in it, so a
        // small window gave 180 pixels to six short numbers and the board went
        // without. A wider number should cost the board width; a constant
        // cannot do that.
        let l = layout(SIZE);
        let short = game();
        let mut long = game();
        long.total_ticks = 999_999_999;
        assert!(
            long.stats_width(&l) > short.stats_width(&l),
            "a longer number did not make the panel wider"
        );
        assert!(
            long.board(&l).area.w < short.board(&l).area.w,
            "the wider panel did not take its width from the board"
        );
    }

    #[test]
    fn the_stats_panel_never_takes_more_than_its_share_of_the_body() {
        // Narrow windows as well as roomy ones, and at least one narrow
        // enough that the panel actually wants more than its share: a
        // ceiling is only a ceiling where something reaches it, so a test
        // that only ever asked at 420 across would pass against no ceiling
        // at all (`known-issues.md` lesson 70).
        let mut bit = false;
        for size in [(420.0_f32, 380.0_f32), (300.0, 340.0), (230.0, 320.0)] {
            let l = layout(size);
            let mut app = game();
            app.total_ticks = u32::MAX;
            app.score = u32::MAX;
            let wanted = app.stats_width(&l);
            let (board_area, stats) = l.split(wanted);
            if wanted > l.body.w * STATS_SHARE {
                bit = true;
            }
            assert!(
                stats.w <= l.body.w * STATS_SHARE + 0.01,
                "at {size:?} the panel took {} of a body {} wide",
                stats.w,
                l.body.w
            );
            assert!(
                board_area.w > 0.0,
                "at {size:?} the panel left the board nothing"
            );
            assert!(
                app.board(&l).cell > 0.0,
                "at {size:?} the panel squeezed the squares out of existence"
            );
        }
        assert!(
            bit,
            "the panel never wanted more than its share, so the ceiling was never tested"
        );
    }

    #[test]
    fn a_window_squashed_from_below_loses_its_switches_before_its_score() {
        let tall = layout((300.0, 600.0));
        assert!(tall.footer.h > 0.0 && tall.header.h > 0.0);
        let squat = layout((300.0, 40.0));
        assert_eq!(squat.footer.h, 0.0, "the footer kept its height");
        assert!(
            squat.header.h > 0.0,
            "the header gave up its height before the footer"
        );
    }

    #[test]
    fn no_band_is_ever_drawn_inside_out() {
        for h in [0.0, 1.0, 20.0, 40.0, 80.0, 200.0, 660.0] {
            let l = layout((300.0, h));
            for (name, r) in [("header", l.header), ("body", l.body), ("footer", l.footer)] {
                assert!(
                    r.w >= 0.0 && r.h >= 0.0,
                    "at height {h} the {name} is {r:?}"
                );
            }
        }
    }

    #[test]
    fn the_body_sits_between_the_bands_and_inside_the_window() {
        // The board is drawn in the body, so a body that reached into a band
        // would put squares under the score or under the switches, and a click
        // that landed on both would go to whichever was filed last.
        for size in [
            (760.0, 660.0),
            (1200.0, 900.0),
            (420.0, 380.0),
            (300.0, 120.0),
        ] {
            let l = layout(size);
            assert!(l.body.h > 0.0, "at {size:?} there is no body to check");
            assert!(
                l.body.y >= l.header.bottom() - 0.01,
                "at {size:?} the body starts inside the header"
            );
            assert!(
                l.body.bottom() <= l.footer.y + 0.01,
                "at {size:?} the body runs into the footer"
            );
            assert!(
                l.body.x >= -0.01 && l.body.right() <= size.0 + 0.01,
                "at {size:?} the body hangs off the window: {:?}",
                l.body
            );
        }
    }

    #[test]
    fn a_window_with_no_room_in_it_has_nothing_to_click() {
        let app = game();
        let f = app.draw((0.0, 0.0));
        assert_eq!(
            f.hit_test(0.0, 0.0),
            None,
            "a window of no size still had a control in it"
        );
    }

    // ── Drawing ─────────────────────────────────────────────────────

    #[test]
    fn every_string_is_told_where_to_stop() {
        // Every string in the program this replaces was drawn with
        // `max_width: None`, so a label longer than its box ran across
        // whatever was beside it.
        for size in [SIZE, (420.0, 380.0), (1200.0, 900.0)] {
            let app = game();
            for c in app.draw(size).commands() {
                if let RenderCommand::Text {
                    text, max_width, ..
                } = c
                {
                    assert!(
                        max_width.is_some_and(|w| w > 0.0),
                        "at {size:?} {text:?} was drawn with no width to stop at"
                    );
                }
            }
        }
    }

    #[test]
    fn every_square_of_the_board_is_drawn() {
        // The squares under everything else. Without them the board is an
        // invisible grid of hit boxes on the background, and the snake is
        // three lozenges floating in the dark.
        let app = placed(&[(10, 10)], Direction::Right);
        let b = board_at(&app, SIZE);
        let f = app.draw(SIZE);
        for (row, col) in [(0, 0), (7, 12), (19, 19)] {
            let (x, y) = b.cell_rect(row, col).centre();
            assert!(
                fill_covering(&f, x, y, SURFACE0).is_some(),
                "square {row},{col} was not drawn"
            );
        }
        // Counted inside the board's own squares, because the unlit switches
        // in the footer are drawn in the same colour.
        let drawn = f
            .commands()
            .iter()
            .filter(|c| {
                matches!(c, RenderCommand::FillRect { color, x, y, .. }
                    if *color == SURFACE0 && b.cells.contains(*x, *y))
            })
            .count();
        assert_eq!(
            drawn,
            GRID_ROWS * GRID_COLS,
            "the board drew {drawn} squares and there are {}",
            GRID_ROWS * GRID_COLS
        );
    }

    #[test]
    fn the_snake_is_drawn_where_the_snake_is() {
        let app = placed(&[(10, 10), (10, 9), (10, 8)], Direction::Right);
        let b = board_at(&app, SIZE);
        let f = app.draw(SIZE);

        let (hx, hy) = b.cell_rect(10, 10).centre();
        assert!(
            fill_covering(&f, hx, hy, GREEN).is_some(),
            "there is no head where the head is"
        );
        let (bx, by) = b.cell_rect(10, 9).centre();
        assert!(
            fill_covering(&f, bx, by, TEAL).is_some(),
            "there is no body where the body is"
        );
        let (ex, ey) = b.cell_rect(3, 3).centre();
        assert!(
            fill_covering(&f, ex, ey, GREEN).is_none() && fill_covering(&f, ex, ey, TEAL).is_none(),
            "there is snake on a square the snake is not on"
        );
    }

    #[test]
    fn the_food_is_drawn_where_the_food_is() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.food.pos = Pos::new(4, 15);
        let b = board_at(&app, SIZE);
        let f = app.draw(SIZE);
        let (x, y) = b.cell_rect(4, 15).centre();
        assert!(
            fill_covering(&f, x, y, RED).is_some(),
            "there is no food where the food is"
        );
        let (ox, oy) = b.cell_rect(4, 14).centre();
        assert!(
            fill_covering(&f, ox, oy, RED).is_none(),
            "there is food on the square beside it too"
        );
    }

    #[test]
    fn the_bonus_food_pulses_as_the_snake_moves() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.bonus_food = Some(Food {
            pos: Pos::new(4, 15),
            kind: FoodKind::Bonus,
            ticks_remaining: BONUS_FOOD_LIFETIME,
        });
        let b = board_at(&app, SIZE);
        let (x, y) = b.cell_rect(4, 15).centre();

        app.pulse_counter = 0;
        let small = fill_covering(&app.draw(SIZE), x, y, YELLOW)
            .expect("there is no bonus where the bonus is");
        app.pulse_counter = 4;
        let large = fill_covering(&app.draw(SIZE), x, y, YELLOW)
            .expect("the bonus stopped being drawn part way through its pulse");
        assert!(
            large.w > small.w,
            "the bonus is the same size all the way round its pulse: {} then {}",
            small.w,
            large.w
        );
        assert!(
            large.w <= b.cell + 0.01,
            "the bonus grew past the square it is in"
        );
    }

    #[test]
    fn the_screen_says_what_state_the_game_is_in() {
        for (state, word) in [
            (GameState::Playing, "Playing"),
            (GameState::Paused, "Paused"),
            (GameState::GameOver, "Game over"),
            (GameState::Won, "You win"),
        ] {
            let mut app = placed(&[(10, 10)], Direction::Right);
            app.state = state;
            assert!(
                texts(&app.draw(SIZE)).iter().any(|t| t == word),
                "a {state:?} game does not say {word:?} anywhere"
            );
        }
    }

    #[test]
    fn the_header_says_what_state_the_game_is_in() {
        // In the header, not merely somewhere on the screen: the overlay
        // writes the same word across the board, so a header carrying the
        // wrong one would still look right to a test that only asked whether
        // the word appeared at all.
        for (state, word) in [
            (GameState::Playing, "Playing"),
            (GameState::Paused, "Paused"),
            (GameState::GameOver, "Game over"),
            (GameState::Won, "You win"),
        ] {
            let mut app = placed(&[(10, 10)], Direction::Right);
            app.state = state;
            let l = layout(SIZE);
            let f = app.draw(SIZE);
            let banner: Vec<String> = f
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, y, .. } if *y < l.header.bottom() => {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect();
            assert!(
                banner.iter().any(|t| t == word),
                "a {state:?} game's header does not say {word:?}; it says {banner:?}"
            );
        }
    }

    #[test]
    fn a_game_that_has_stopped_says_how_to_carry_on() {
        for (state, hint) in [
            (GameState::Paused, "P or the Pause switch to carry on"),
            (GameState::GameOver, "Enter or Restart to play again"),
            (GameState::Won, "The board is full. Enter to play again"),
        ] {
            let mut app = placed(&[(10, 10)], Direction::Right);
            app.state = state;
            let words = texts(&app.draw(SIZE));
            assert!(
                words.iter().any(|t| t == hint),
                "a {state:?} game does not say how to carry on; it says {words:?}"
            );
        }
    }

    #[test]
    fn a_game_that_is_still_going_has_nothing_across_the_board() {
        let app = placed(&[(10, 10)], Direction::Right);
        let words = texts(&app.draw(SIZE));
        for word in ["Paused", "Game over", "You win"] {
            assert!(
                !words.iter().any(|t| t == word),
                "a game in play says {word:?}"
            );
        }
    }

    #[test]
    fn the_header_shows_the_score_and_the_best_one() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.score = 130;
        app.high_score = 470;
        let words = texts(&app.draw(SIZE));
        assert!(
            words.iter().any(|t| t == "Score 130"),
            "the score is not on the screen; there is {words:?}"
        );
        assert!(
            words.iter().any(|t| t == "Best 470"),
            "the best score is not on the screen; there is {words:?}"
        );
    }

    #[test]
    fn the_stats_panel_shows_what_the_game_has_counted() {
        let mut app = placed(&[(10, 10), (10, 9)], Direction::Right);
        app.foods_eaten = 7;
        app.bonus_eaten = 3;
        app.streak = 4;
        app.total_ticks = 88;
        let words = texts(&app.draw(SIZE));
        for name in [
            "Stats", "Length", "Eaten", "Bonus", "Streak", "Speed", "Moves",
        ] {
            assert!(
                words.iter().any(|t| t == name),
                "the panel has no {name} row; there is {words:?}"
            );
        }
        for value in ["2", "7", "3", "4", "88"] {
            assert!(
                words.iter().any(|t| t == value),
                "the panel does not show {value}; there is {words:?}"
            );
        }
    }

    // ── The window ──────────────────────────────────────────────────

    #[test]
    fn the_window_is_named_and_sized() {
        let app = game();
        assert_eq!(app.title(), "Snake");
        assert_eq!(app.app_id(), "snake");
        assert_eq!(
            app.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
    }

    #[test]
    fn the_window_asks_for_a_tick_often_enough_to_run_the_fastest_game() {
        let app = game();
        let tick = app
            .tick_interval()
            .expect("the window asks for no ticks at all");
        let fastest = u64::from(tick_interval_ms(Difficulty::Hard, 1));
        assert!(
            tick.as_millis() > 0 && fastest % tick.as_millis() as u64 == 0,
            "a {tick:?} tick cannot make a move every {fastest}ms on the nose"
        );
    }

    #[test]
    fn closing_the_window_ends_the_program() {
        let mut app = game();
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn an_event_that_changed_something_asks_for_a_redraw() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Up))),
            Response::Redraw
        ));
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle
        ));
    }

    #[test]
    fn the_size_a_frame_is_drawn_at_is_the_size_the_next_click_is_read_against() {
        let mut app = game();
        let tree = app.render(500.0, 420.0);
        assert!(
            !tree.commands.is_empty(),
            "the window was handed nothing to draw"
        );
        assert_eq!(app.size(), (500.0, 420.0));
    }

    #[test]
    fn a_resize_moves_the_switches_and_the_clicks_follow_them() {
        // The stored size is what a click is read against, so a click aimed at
        // the small window's Restart switch must restart the game -- which it
        // cannot do if the click is still being read against the old size.
        let small = (360.0, 320.0);
        let mut app = placed(&[(10, 10), (10, 9), (10, 8), (10, 7)], Direction::Right);
        let big_rect = probe::rect_of_sized(&app, Target::Restart, SIZE)
            .expect("no Restart switch in a full-sized window");
        let small_rect = probe::rect_of_sized(&app, Target::Restart, small)
            .expect("no Restart switch in a small window");
        assert_ne!(
            big_rect, small_rect,
            "the switch did not move with the window"
        );

        handle_event(
            &mut app,
            &Event::Resize {
                width: small.0 as u32,
                height: small.1 as u32,
            },
        );
        assert_eq!(app.size(), small);

        let (x, y) = small_rect.centre();
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Press(MouseButton::Left),
                })
            ),
            EventResult::Consumed,
            "the click was read against the size the window used to be"
        );
        assert_eq!(app.snake_length(), 3, "the Restart switch did not restart");
        assert_eq!(
            app.size(),
            small,
            "the restart threw away the size the window had been given"
        );
    }

    #[test]
    fn an_event_the_game_has_no_use_for_is_left_alone() {
        let mut app = game();
        assert_eq!(
            handle_event(&mut app, &Event::FocusIn),
            EventResult::Ignored
        );
    }

    #[test]
    fn level_one_moves_at_the_difficultys_own_pace() {
        // A difficulty's base interval is what it means: at the first level,
        // before any speed has been earned, the snake moves at exactly it.
        for d in DIFFICULTIES {
            assert_eq!(
                tick_interval_ms(d, 1),
                d.base_interval_ms(),
                "{d:?} at the first level does not move at its own pace"
            );
        }
    }

    #[test]
    fn the_footer_names_every_difficulty() {
        let app = game();
        let drawn = texts(&app.frame(SIZE.0, SIZE.1));
        for d in DIFFICULTIES {
            assert!(
                drawn.iter().any(|t| t == d.label()),
                "{d:?} is not named anywhere on the screen"
            );
        }
        let mut names: Vec<&str> = DIFFICULTIES.iter().map(|d| d.label()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            DIFFICULTIES.len(),
            "two difficulties are offered under the same name"
        );
    }

    #[test]
    fn the_board_keeps_most_of_the_window() {
        // The bands round the board are what is spent to have it, and below a
        // point they start paying: however short the window, they never take
        // more than the part of it the board is not entitled to.
        // Down to 20 high, where the bands are already smaller than their own
        // smallest size and are paying everything they have.
        for h in [20.0_f32, 40.0, 60.0, 100.0, 200.0, 400.0, 660.0, 1000.0] {
            let l = layout((760.0, h));
            assert!(
                l.header.h + l.footer.h <= h * 0.45 + 0.01,
                "at {h} high the bands eat into the board's share"
            );
        }
        // In the middle of the range, where neither the smallest nor the
        // largest band is in play, these are the depths. They are the numbers
        // a 400-high window is drawn with, not a second copy of the sum that
        // produces them.
        let l = layout((760.0, 400.0));
        assert!(
            (l.header.h - 36.0).abs() < 0.01,
            "the header is {} deep and should be 36",
            l.header.h
        );
        assert!(
            (l.footer.h - 32.0).abs() < 0.01,
            "the footer is {} deep and should be 32",
            l.footer.h
        );
    }

    #[test]
    fn the_panel_sits_beside_the_board_and_not_on_it() {
        for size in [(760.0, 660.0), (1200.0, 900.0), (420.0, 380.0)] {
            let app = game();
            let l = layout(size);
            let (board, stats) = l.split(app.stats_width(&l));
            assert!(!stats.is_empty(), "at {size:?} there is no stats panel");
            assert!(
                stats.x >= board.right() - 0.01,
                "at {size:?} the panel is drawn over the board"
            );
            assert!(
                stats.right() <= l.body.right() + 0.01,
                "at {size:?} the panel hangs off the end of the body"
            );
        }
    }

    #[test]
    fn a_bonus_never_appears_on_top_of_the_food() {
        // A bonus under the meal is a bonus that cannot be eaten as one: the
        // meal is checked first, so the square would score ten and the bonus
        // would sit there unreachable until its time ran out.
        let mut seen = 0_u32;
        for seed in 0..400_u64 {
            let mut app = SnakeApp::with_seed(seed);
            app.snake = vec![Pos::new(10, 10)];
            app.direction = Direction::Right;
            app.dir_queue.clear();
            app.bonus_food = None;
            app.food.pos = Pos::new(10, 11);
            step(&mut app);
            if let Some(bonus) = app.bonus_food {
                seen = seen.saturating_add(1);
                assert_ne!(
                    bonus.pos, app.food.pos,
                    "seed {seed} put the bonus under the meal"
                );
                assert!(
                    !app.snake.contains(&bonus.pos),
                    "seed {seed} put the bonus under the snake"
                );
            }
        }
        assert!(seen > 0, "no seed in four hundred produced a bonus at all");
    }

    #[test]
    fn a_bonus_never_appears_under_the_snake() {
        // A bonus under the snake cannot be eaten: the square is already
        // the snake's, and running the head back over it is running into
        // itself. The test above would not show it — a three-square snake
        // covers three squares of four hundred, so a bonus missing it is a
        // coincidence rather than a check (`known-issues.md` lesson 70).
        // This snake has nine tenths of the board.
        let mut long = Vec::new();
        for row in 0..18_i32 {
            for col in 0..20_i32 {
                long.push(Pos::new(row, col));
            }
        }
        let mut seen = 0_u32;
        for seed in 0..200_u64 {
            let mut app = SnakeApp::with_seed(seed);
            app.snake.clone_from(&long);
            app.bonus_food = None;
            app.food.pos = Pos::new(19, 0);
            app.spawn_bonus_food();
            if let Some(bonus) = app.bonus_food {
                seen = seen.saturating_add(1);
                assert!(
                    !app.snake.contains(&bonus.pos),
                    "seed {seed} put the bonus at {:?}, under the snake",
                    bonus.pos
                );
            }
        }
        assert!(seen > 0, "no seed in two hundred placed a bonus at all");
    }

    #[test]
    fn dawdling_between_meals_lets_the_streak_lapse() {
        // Nothing sets the clock since the last meal but the moves themselves,
        // so a snake going round in circles has to lose its streak by moving.
        let mut app = placed(&[(10, 10)], Direction::Right);
        app.wrap_mode = true;
        app.streak = 4;
        app.ticks_since_food = 0;
        app.food.pos = Pos::new(0, 0);
        steps(&mut app, STREAK_WINDOW_TICKS + 2);
        assert_eq!(
            app.state,
            GameState::Playing,
            "the wandering ended the game instead of the streak"
        );
        assert!(
            app.ticks_since_food > STREAK_WINDOW_TICKS,
            "the moves were not counted against the streak"
        );

        let head = head(&app);
        app.food.pos = head.moved(app.direction).wrapped();
        step(&mut app);
        assert_eq!(app.foods_eaten, 1, "the meal was not eaten");
        assert_eq!(app.streak, 1, "the streak outlived its window");
    }

    #[test]
    fn a_tick_from_the_window_moves_the_snake() {
        // The window hands time in on `Event::Tick`; without that arm the
        // clock never reaches the game and the snake stands still on screen
        // however long it is left running.
        let mut app = placed(&[(10, 10), (10, 9), (10, 8)], Direction::Right);
        let before = head(&app);
        let interval = u64::from(app.current_interval_ms());
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Tick {
                    elapsed_ms: interval
                }
            ),
            EventResult::Consumed,
            "a tick long enough to move the snake was turned away"
        );
        assert_eq!(
            head(&app),
            Pos::new(before.row, before.col + 1),
            "the tick reached the game without moving the snake"
        );
    }

    #[test]
    fn a_win_banks_the_score_as_the_best_one() {
        // The board is filled but for the square the food is in, so the meal
        // that is about to be eaten is also the move that wins.
        let mut cells = vec![Pos::new(0, 1)];
        for row in 0..20i32 {
            for col in 0..20i32 {
                let p = Pos::new(row, col);
                if p != Pos::new(0, 0) && p != Pos::new(0, 1) {
                    cells.push(p);
                }
            }
        }
        let mut app = game();
        app.snake = cells;
        app.direction = Direction::Left;
        app.dir_queue.clear();
        app.food.pos = Pos::new(0, 0);
        app.score = 310;
        step(&mut app);
        assert_eq!(app.state, GameState::Won);
        assert_eq!(
            app.high_score,
            310 + NORMAL_FOOD_POINTS,
            "a game that was won was not banked as the best one"
        );
    }

    #[test]
    fn a_bigger_score_moves_the_snake_sooner() {
        let mut app = placed(&[(10, 10)], Direction::Right);
        let slow = app.current_interval_ms();
        app.score = 500;
        assert!(
            app.current_interval_ms() < slow,
            "five hundred points bought no speed at all: {slow} either way"
        );
    }
}
