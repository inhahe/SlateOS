//! SlateOS Minesweeper — clear the board without stepping on a mine.
//!
//! Three difficulties, first-click-safe mine placement, flood-fill reveal,
//! flagging, chording, a mine counter and a running clock. The board can be
//! played with the pointer (left reveals, right flags, middle or a double click
//! chords) or entirely from the keyboard.
//!
//! # What this file used to be
//!
//! It drew a minesweeper and could not play one. `main` built a
//! `MinesweeperApp`, bound it to `_app` and returned; nothing opened a window,
//! so no board ever reached a screen. Six faults followed, and the second is
//! the one that decides whether this is a game at all:
//!
//! * **There was no event handling of any kind** — not one `Event::` anywhere
//!   in the file, in a program that is nothing but clicking. `reveal`,
//!   `toggle_flag` and `chord` were written, tested and unreachable: no input
//!   could call them.
//! * **Every game was the same game.** `new(difficulty)` was
//!   `with_seed(difficulty, 42)`, and `restart` advanced that by one — while
//!   the module documentation directly above it said randomness was "seeded
//!   from the system so that two players do not get the same game."
//!   `seed_from_system` was never called. Two players got not merely the same
//!   game but the same *sequence* of games.
//! * **The clock counted calls.** `tick()` took no argument and added exactly
//!   one second per invocation, so the elapsed time was a count of however
//!   often something happened to call it — and nothing did.
//! * **The window was derived from the board rather than the board from the
//!   window.** `render()` took no size at all; `window_width()` and
//!   `window_height()` computed what the window *ought* to be from a 30-pixel
//!   cell constant. A board is a board, not a picture of one at one size.
//! * **`revealed_count` and `flags_placed` were cached copies of facts the
//!   cells already carried, and they drifted.** The losing click incremented
//!   `revealed_count` for the mine it uncovered; `reveal_all_mines` then
//!   revealed every other mine and incremented nothing. Both are derived now.
//! * **`#![allow(dead_code)]` and five cast allows** sat on the first six
//!   lines, and the crate did not pass the lane's clippy gate at all: 43
//!   `indexing_slicing`, 22 `arithmetic_side_effects` and three
//!   `unwrap`/`expect`/`panic` findings in production code.
//!
//! # Shape
//!
//! [`Layout`] is derived from the live window size on every frame and never
//! stored on the model. Every control the renderer paints it also records with
//! [`Frame::hit`](guitk::frame::Frame::hit), which is what lets a test click a
//! cell by name and what lets the pointer find one.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use randrange::{RandomSource, SeededRng, seed_from_system};
use std::process::ExitCode;
use std::time::Duration;

/// The frame this program draws into, with its own control identifiers.
pub type Frame = guitk::frame::Frame<Target>;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
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
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

/// The colours of the neighbour-count digits 1 through 8.
///
/// A slice and not an array: writing the length into the type turns "someone
/// added a colour and forgot the count" into a compile error only if they also
/// touched the count, which is the one edit nobody forgets. As a slice it can
/// go wrong the way it really goes wrong, and a test is what stops it
/// (`known-issues.md` lesson 51's neighbour).
const NUMBER_COLORS: &[Color] = &[
    BLUE,       // 1
    GREEN,      // 2
    RED,        // 3
    MAUVE,      // 4
    PEACH,      // 5
    TEAL,       // 6
    YELLOW,     // 7
    TEXT_COLOR, // 8
];

// ── Window and clock ───────────────────────────────────────────────────────

/// The size the window asks for, and the size the tests draw at unless they say
/// otherwise. Wide enough that Expert's thirty columns are not postage stamps.
pub const WINDOW_WIDTH: f32 = 940.0;
/// See [`WINDOW_WIDTH`].
pub const WINDOW_HEIGHT: f32 = 660.0;

/// How often the clock is woken while a game is being played.
///
/// A second, because a second is what the clock displays; waking any faster
/// would redraw a string that had not changed.
pub const CLOCK_MS: u64 = 1_000;

/// The keys the board answers, drawn along the bottom.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Arrows", "move"),
    ("Space", "reveal"),
    ("F", "flag"),
    ("C", "chord"),
    ("D", "level"),
    ("N", "new"),
];

// ── Steps and neighbours ───────────────────────────────────────────────────

/// One coordinate's contribution to a neighbour: back one, still, or on one.
///
/// An enum rather than the `-1 | 0 | 1` this was, because every use of it was
/// `row as isize + dr` guarded by four comparisons against casts of `rows` and
/// `cols`. Two casts and a signed add to express "the cell above", in a file
/// whose indices are all unsigned. [`Step::from`] says it in one `checked_`
/// call and answers `None` at the top and left edges rather than wrapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Towards row 0 / column 0.
    Back,
    /// Not at all.
    Stay,
    /// Away from the origin.
    Fwd,
}

impl Step {
    /// Where one step from `at` lands, or `None` off the low end.
    #[must_use]
    pub fn from(self, at: usize) -> Option<usize> {
        match self {
            Self::Back => at.checked_sub(1),
            Self::Stay => Some(at),
            Self::Fwd => at.checked_add(1),
        }
    }
}

/// The eight cells around a cell, as row and column steps.
pub const NEIGHBOURS: [(Step, Step); 8] = [
    (Step::Back, Step::Back),
    (Step::Back, Step::Stay),
    (Step::Back, Step::Fwd),
    (Step::Stay, Step::Back),
    (Step::Stay, Step::Fwd),
    (Step::Fwd, Step::Back),
    (Step::Fwd, Step::Stay),
    (Step::Fwd, Step::Fwd),
];

/// Which way an arrow key moves the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    /// The row and column steps this direction is made of.
    #[must_use]
    pub fn steps(self) -> (Step, Step) {
        match self {
            Self::Up => (Step::Back, Step::Stay),
            Self::Down => (Step::Fwd, Step::Stay),
            Self::Left => (Step::Stay, Step::Back),
            Self::Right => (Step::Stay, Step::Fwd),
        }
    }
}

// ── Difficulty ─────────────────────────────────────────────────────────────

/// How big the board is and how many mines are buried in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Expert,
}

impl Difficulty {
    /// Every difficulty, in the order `next` cycles through them.
    pub const ALL: [Difficulty; 3] = [
        Difficulty::Beginner,
        Difficulty::Intermediate,
        Difficulty::Expert,
    ];

    /// The board's width, in cells.
    #[must_use]
    pub fn cols(self) -> usize {
        match self {
            Self::Beginner => 9,
            Self::Intermediate => 16,
            Self::Expert => 30,
        }
    }

    /// The board's height, in cells.
    #[must_use]
    pub fn rows(self) -> usize {
        match self {
            Self::Beginner => 9,
            Self::Intermediate => 16,
            Self::Expert => 16,
        }
    }

    /// How many mines are buried.
    ///
    /// This must leave room for the first click's safe zone — the clicked cell
    /// and its eight neighbours — or the board could not be dealt at all. That
    /// is a fact about these numbers rather than something the placer should be
    /// asked to discover at runtime, so a test asserts it.
    #[must_use]
    pub fn mines(self) -> usize {
        match self {
            Self::Beginner => 10,
            Self::Intermediate => 40,
            Self::Expert => 99,
        }
    }

    /// How many cells the board has.
    #[must_use]
    pub fn cells(self) -> usize {
        self.rows().saturating_mul(self.cols())
    }

    /// The name shown on the difficulty chip.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Beginner => "Beginner",
            Self::Intermediate => "Intermediate",
            Self::Expert => "Expert",
        }
    }

    /// The accent the difficulty chip is drawn in.
    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Beginner => GREEN,
            Self::Intermediate => YELLOW,
            Self::Expert => RED,
        }
    }

    /// The next difficulty the `D` key and the difficulty chip move to.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Beginner => Self::Intermediate,
            Self::Intermediate => Self::Expert,
            Self::Expert => Self::Beginner,
        }
    }
}

// ── Cells ──────────────────────────────────────────────────────────────────

/// What the player has done to a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    /// Hidden and unflagged.
    Hidden,
    /// Hidden and flagged.
    Flagged,
    /// Uncovered.
    Revealed,
}

/// One square of the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// Whether a mine is buried here.
    pub is_mine: bool,
    /// What the player has done to it.
    pub state: CellState,
    /// How many of the eight neighbours hold a mine. Meaningless on a mine.
    pub adjacent: u8,
}

impl Cell {
    const fn new() -> Self {
        Self {
            is_mine: false,
            state: CellState::Hidden,
            adjacent: 0,
        }
    }
}

/// Where a game has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameStatus {
    /// Dealt but not yet clicked: the mines are not placed, because where they
    /// go depends on where the first click lands.
    Ready,
    /// Mines placed, clock running.
    Playing,
    /// A mine was uncovered.
    Lost,
    /// Every cell that is not a mine has been uncovered.
    Won,
}

// ── What the renderer records and the pointer finds ────────────────────────

/// A control the drawing pass records a rectangle for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A square of the board, by `(row, column)`.
    Cell(usize, usize),
    /// The difficulty chip.
    Difficulty,
    /// The new-game chip.
    NewGame,
}

/// Everything the player can ask for, however they asked for it.
///
/// One name per intention, so a key and a click that mean the same thing go
/// down the same path — which is what stops the two drifting apart, the way
/// this program's tested `reveal` drifted apart from the nothing that called
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Uncover a cell.
    Reveal(usize, usize),
    /// Flag or unflag a cell.
    Flag(usize, usize),
    /// Uncover every unflagged neighbour of a satisfied number.
    Chord(usize, usize),
    /// Move the keyboard cursor.
    Move(Dir),
    /// Deal a fresh board at the same difficulty.
    NewGame,
    /// Deal a fresh board at the next difficulty.
    CycleDifficulty,
    /// Deal a fresh board at a named difficulty.
    SetDifficulty(Difficulty),
}

// ── The model ──────────────────────────────────────────────────────────────

/// A game of minesweeper, and the window size the last frame was drawn at.
pub struct MinesweeperApp {
    difficulty: Difficulty,
    cells: Vec<Cell>,
    status: GameStatus,
    /// Milliseconds of play, not seconds. The clock displays seconds and the
    /// window wakes it once a second, but what is *stored* is the finest unit
    /// anyone reports, so the displayed value is derived rather than a second
    /// copy that can round differently.
    elapsed_ms: u64,
    seed: u64,
    rng: SeededRng,
    /// The cell whose uncovering lost the game, so it can be painted red among
    /// the mines that were merely revealed along with it.
    losing_cell: Option<(usize, usize)>,
    /// Where the keyboard is pointing.
    cursor: (usize, usize),
    /// The size of the last frame drawn, which is the size the next click is
    /// read against.
    size: (f32, f32),
}

impl Default for MinesweeperApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MinesweeperApp {
    /// A new game at Beginner, seeded from the system.
    ///
    /// From the system, and not from the constant 42 this used to be. A fixed
    /// seed is not a small bug in a puzzle game: it means every installation
    /// deals the same board, and — because `restart` added one to it — the same
    /// second board, and the same third.
    #[must_use]
    pub fn new() -> Self {
        Self::with_seed(Difficulty::Beginner, seed_from_system(0x_5AFE_CE11_u64))
    }

    /// A new game at `difficulty` from a named seed. The same seed is the same
    /// board, which is what lets a test name one.
    #[must_use]
    pub fn with_seed(difficulty: Difficulty, seed: u64) -> Self {
        Self {
            difficulty,
            cells: vec![Cell::new(); difficulty.cells()],
            status: GameStatus::Ready,
            elapsed_ms: 0,
            seed,
            rng: SeededRng::new(seed),
            losing_cell: None,
            cursor: (0, 0),
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    // ── Reading the board ──────────────────────────────────────────────────

    /// The board's height in cells.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.difficulty.rows()
    }

    /// The board's width in cells.
    #[must_use]
    pub fn cols(&self) -> usize {
        self.difficulty.cols()
    }

    /// How many mines are buried.
    #[must_use]
    pub fn total_mines(&self) -> usize {
        self.difficulty.mines()
    }

    /// Which difficulty is being played.
    #[must_use]
    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    /// Where the game has got to.
    #[must_use]
    pub fn status(&self) -> GameStatus {
        self.status
    }

    /// The seed this board was dealt from.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Where the keyboard is pointing.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// The cell whose uncovering ended the game, if one did.
    #[must_use]
    pub fn losing_cell(&self) -> Option<(usize, usize)> {
        self.losing_cell
    }

    /// Milliseconds of play so far.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// Whole seconds of play so far.
    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.elapsed_ms.checked_div(1_000).unwrap_or(0)
    }

    /// The flat index of `(row, col)`, or `None` off the board.
    #[must_use]
    pub fn index_of(&self, row: usize, col: usize) -> Option<usize> {
        if row >= self.rows() || col >= self.cols() {
            return None;
        }
        row.checked_mul(self.cols())?.checked_add(col)
    }

    /// The cell at `(row, col)`, or `None` off the board.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> Option<Cell> {
        self.index_of(row, col)
            .and_then(|i| self.cells.get(i))
            .copied()
    }

    /// Whether `(row, col)` holds a mine. `false` off the board and `false`
    /// before the first click, when no mine has been placed yet.
    #[must_use]
    pub fn is_mine(&self, row: usize, col: usize) -> bool {
        self.cell(row, col).is_some_and(|c| c.is_mine)
    }

    /// Whether `(row, col)` has been uncovered.
    #[must_use]
    pub fn is_revealed(&self, row: usize, col: usize) -> bool {
        self.cell(row, col)
            .is_some_and(|c| c.state == CellState::Revealed)
    }

    /// Whether `(row, col)` carries a flag.
    #[must_use]
    pub fn is_flagged(&self, row: usize, col: usize) -> bool {
        self.cell(row, col)
            .is_some_and(|c| c.state == CellState::Flagged)
    }

    /// How many of `(row, col)`'s neighbours hold a mine.
    #[must_use]
    pub fn adjacent(&self, row: usize, col: usize) -> u8 {
        self.cell(row, col).map_or(0, |c| c.adjacent)
    }

    /// How many mines are buried, counted rather than remembered.
    #[must_use]
    pub fn mine_count(&self) -> usize {
        self.cells.iter().filter(|c| c.is_mine).count()
    }

    /// How many flags are on the board, counted rather than remembered.
    ///
    /// This was a `flags_placed: usize` kept in step by hand. A second copy of
    /// a fact the cells already carry is a thing that can disagree with them,
    /// and the one next to it — `revealed_count` — did: the losing click
    /// counted the mine it uncovered and `reveal_all_mines` then uncovered
    /// every other mine and counted none of them.
    #[must_use]
    pub fn flag_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| c.state == CellState::Flagged)
            .count()
    }

    /// How many cells have been uncovered. See [`Self::flag_count`].
    #[must_use]
    pub fn revealed_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| c.state == CellState::Revealed)
            .count()
    }

    /// Mines minus flags — what the counter in the header shows. Signed,
    /// because a player may plant more flags than there are mines.
    #[must_use]
    pub fn mines_remaining(&self) -> i64 {
        let mines = i64::try_from(self.total_mines()).unwrap_or(i64::MAX);
        let flags = i64::try_from(self.flag_count()).unwrap_or(i64::MAX);
        mines.saturating_sub(flags)
    }

    /// The cells around `(row, col)` that are on the board.
    pub fn neighbours(&self, row: usize, col: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        let (rows, cols) = (self.rows(), self.cols());
        NEIGHBOURS.iter().filter_map(move |&(dr, dc)| {
            let r = dr.from(row)?;
            let c = dc.from(col)?;
            (r < rows && c < cols).then_some((r, c))
        })
    }

    // ── Dealing ────────────────────────────────────────────────────────────

    /// Bury the mines, avoiding `(safe_row, safe_col)` and its neighbours.
    ///
    /// Chosen by shuffling the cells that are *allowed* to hold a mine and
    /// taking the first few, rather than by drawing cells at random and
    /// throwing away the ones that will not do. Rejection sampling is the
    /// natural way to write this and it has an unbounded loop in it: ask for
    /// more mines than there are legal cells and it spins for ever. Taking from
    /// a shuffle cannot: `take` stops at whichever runs out first.
    fn place_mines(&mut self, safe_row: usize, safe_col: usize) {
        let safe: Vec<(usize, usize)> = std::iter::once((safe_row, safe_col))
            .chain(self.neighbours(safe_row, safe_col))
            .collect();
        let cols = self.cols();
        let mut candidates: Vec<usize> = (0..self.cells.len())
            .filter(|&i| match split(i, cols) {
                Some(rc) => !safe.contains(&rc),
                None => false,
            })
            .collect();
        self.rng.shuffle(&mut candidates);
        for &i in candidates.iter().take(self.total_mines()) {
            if let Some(cell) = self.cells.get_mut(i) {
                cell.is_mine = true;
            }
        }
        self.compute_counts();
    }

    /// Recompute every cell's neighbour count from the mines just buried.
    fn compute_counts(&mut self) {
        let mut counts = vec![0u8; self.cells.len()];
        for row in 0..self.rows() {
            for col in 0..self.cols() {
                let n = self
                    .neighbours(row, col)
                    .filter(|&(r, c)| self.is_mine(r, c))
                    .count();
                if let (Some(i), Ok(n)) = (self.index_of(row, col), u8::try_from(n))
                    && let Some(slot) = counts.get_mut(i)
                {
                    *slot = n;
                }
            }
        }
        for (cell, n) in self.cells.iter_mut().zip(counts) {
            cell.adjacent = n;
        }
    }

    /// Deal a fresh board at `difficulty`, drawing the next seed from this
    /// game's own generator.
    ///
    /// From the generator, not `seed + 1`. Consecutive seeds are not a fresh
    /// draw — they are one step along a single stream, which is how this
    /// program's second game came to be a fixed function of its first.
    fn deal(&mut self, difficulty: Difficulty) {
        let seed = self.rng.next_u64();
        *self = Self::with_seed(difficulty, seed);
    }

    // ── Playing ────────────────────────────────────────────────────────────

    /// Whether the game will still answer a move.
    #[must_use]
    pub fn is_over(&self) -> bool {
        matches!(self.status, GameStatus::Lost | GameStatus::Won)
    }

    /// Do what the player asked, wherever they asked it from.
    pub fn apply(&mut self, action: Action) -> EventResult {
        match action {
            Action::Reveal(row, col) => self.reveal(row, col),
            Action::Flag(row, col) => self.flag(row, col),
            Action::Chord(row, col) => self.chord(row, col),
            Action::Move(dir) => self.move_cursor(dir),
            Action::NewGame => {
                self.deal(self.difficulty);
                EventResult::Consumed
            }
            Action::CycleDifficulty => {
                self.deal(self.difficulty.next());
                EventResult::Consumed
            }
            Action::SetDifficulty(difficulty) => {
                self.deal(difficulty);
                EventResult::Consumed
            }
        }
    }

    /// Move the keyboard cursor one cell, stopping at the edges.
    fn move_cursor(&mut self, dir: Dir) -> EventResult {
        let (dr, dc) = dir.steps();
        let (row, col) = self.cursor;
        let Some(r) = dr.from(row) else {
            return EventResult::Ignored;
        };
        let Some(c) = dc.from(col) else {
            return EventResult::Ignored;
        };
        // There is no `|| (r, c) == self.cursor` here, and there used to be.
        // Every `Dir` is one `Back` or one `Fwd` and three `Stay`s, and both
        // `Back` and `Fwd` change the coordinate they are applied to -- so an
        // arrow key that lands where it started is not a case this function can
        // reach, and a guard standing behind a duplicate of itself is a guard
        // no test can fail on (`known-issues.md` lesson 51). The behaviour it
        // named -- an arrow at the edge is `Ignored` -- is still asserted, and
        // is now guaranteed by the two bounds checks that do the work.
        if r >= self.rows() || c >= self.cols() {
            return EventResult::Ignored;
        }
        self.cursor = (r, c);
        EventResult::Consumed
    }

    /// Uncover `(row, col)`, placing the mines first if this is the first
    /// click of the game.
    fn reveal(&mut self, row: usize, col: usize) -> EventResult {
        if self.is_over() {
            return EventResult::Ignored;
        }
        let Some(cell) = self.cell(row, col) else {
            return EventResult::Ignored;
        };
        if cell.state != CellState::Hidden {
            return EventResult::Ignored;
        }
        self.cursor = (row, col);

        if self.status == GameStatus::Ready {
            self.place_mines(row, col);
            self.status = GameStatus::Playing;
        }

        if self.is_mine(row, col) {
            self.set_state(row, col, CellState::Revealed);
            self.status = GameStatus::Lost;
            self.losing_cell = Some((row, col));
            self.reveal_all_mines();
            return EventResult::Consumed;
        }

        self.flood_reveal(row, col);
        if self.is_cleared() {
            self.status = GameStatus::Won;
            self.flag_all_mines();
        }
        EventResult::Consumed
    }

    /// Uncover `(row, col)` and, while a cell touches no mine, its neighbours.
    ///
    /// There is no `|| cell.is_mine` in the loop below, and there used to be.
    /// No mine can reach this stack: the seed cell is one `reveal` has already
    /// found to be safe, and the only other cells pushed are the neighbours of
    /// a cell whose count is *nought*, which by the definition of that count
    /// are every one of them safe. A guard standing behind a duplicate of
    /// itself is a guard no test can fail on (`known-issues.md` lesson 51), and
    /// the mutation sweep proved this one exactly that. The behaviour it named
    /// -- the flood never uncovers a mine -- is still asserted, over twenty
    /// boards, and is now guaranteed by the count rather than re-checked.
    fn flood_reveal(&mut self, row: usize, col: usize) {
        let mut stack = vec![(row, col)];
        while let Some((r, c)) = stack.pop() {
            let Some(cell) = self.cell(r, c) else {
                continue;
            };
            if cell.state != CellState::Hidden {
                continue;
            }
            self.set_state(r, c, CellState::Revealed);
            if cell.adjacent == 0 {
                stack.extend(self.neighbours(r, c).collect::<Vec<_>>());
            }
        }
    }

    /// Flag or unflag `(row, col)`.
    ///
    /// A flag may be planted before the first click, unlike the version this
    /// replaces, which refused every flag while the status was `Ready`. Nothing
    /// needed that: a flag is a note the player writes to themselves, the mines
    /// are not placed until the first *reveal* whatever is flagged, and
    /// refusing meant the very first thing a cautious player does was silently
    /// ignored.
    fn flag(&mut self, row: usize, col: usize) -> EventResult {
        if self.is_over() {
            return EventResult::Ignored;
        }
        let Some(cell) = self.cell(row, col) else {
            return EventResult::Ignored;
        };
        let next = match cell.state {
            CellState::Hidden => CellState::Flagged,
            CellState::Flagged => CellState::Hidden,
            CellState::Revealed => return EventResult::Ignored,
        };
        self.cursor = (row, col);
        self.set_state(row, col, next);
        EventResult::Consumed
    }

    /// Uncover every unflagged neighbour of a number whose flags are all
    /// planted. The gesture that makes minesweeper quick, and the one that
    /// loses a game fastest when the flags are wrong.
    ///
    /// The first guard is `is_over`, matching [`Self::reveal`] and
    /// [`Self::flag`], and it deliberately says nothing about `Ready`. It used
    /// to read `self.status != GameStatus::Playing`, which also excludes
    /// `Ready` -- but a board that is `Ready` has no revealed cell on it at
    /// all (`deal` leaves every cell `Hidden`, `flag` never reveals, and
    /// `reveal` moves to `Playing` before it uncovers anything), so the very
    /// next guard refuses that case already. That made a third of this
    /// condition a duplicate of the line below it, which is a guard no test
    /// can fail on (`known-issues.md` lesson 51).
    fn chord(&mut self, row: usize, col: usize) -> EventResult {
        if self.is_over() {
            return EventResult::Ignored;
        }
        let Some(cell) = self.cell(row, col) else {
            return EventResult::Ignored;
        };
        if cell.state != CellState::Revealed || cell.is_mine {
            return EventResult::Ignored;
        }
        let flags = self
            .neighbours(row, col)
            .filter(|&(r, c)| self.is_flagged(r, c))
            .count();
        if usize::from(cell.adjacent) != flags {
            return EventResult::Ignored;
        }
        let hidden: Vec<(usize, usize)> = self
            .neighbours(row, col)
            .filter(|&(r, c)| {
                self.cell(r, c)
                    .is_some_and(|n| n.state == CellState::Hidden)
            })
            .collect();
        if hidden.is_empty() {
            return EventResult::Ignored;
        }
        // No `break` once the game ends, and there used to be one. `reveal`
        // opens with `is_over`, so every call after the mine that ended the
        // game returns `Ignored` without touching a cell: the loop was already
        // inert, and stopping it early was an optimisation wearing a rule's
        // clothing. The sweep could not tell the two apart, which is what
        // being inert means (`known-issues.md` lesson 51).
        for (r, c) in hidden {
            self.reveal(r, c);
        }
        self.cursor = (row, col);
        EventResult::Consumed
    }

    /// Whether every cell that is not a mine has been uncovered.
    ///
    /// Asked of the cells rather than of a running `revealed_count >=
    /// total - mines`. The count was a second copy of this fact, and a `>=`
    /// against a copy that the losing click incremented for a *mine* is a
    /// comparison that can be reached by uncovering the wrong things.
    #[must_use]
    pub fn is_cleared(&self) -> bool {
        self.cells
            .iter()
            .all(|c| c.is_mine || c.state == CellState::Revealed)
    }

    /// On a loss, uncover every mine that is not flagged.
    fn reveal_all_mines(&mut self) {
        for cell in &mut self.cells {
            if cell.is_mine && cell.state == CellState::Hidden {
                cell.state = CellState::Revealed;
            }
        }
    }

    /// On a win, plant the flags the player did not need to.
    fn flag_all_mines(&mut self) {
        for cell in &mut self.cells {
            if cell.is_mine && cell.state == CellState::Hidden {
                cell.state = CellState::Flagged;
            }
        }
    }

    fn set_state(&mut self, row: usize, col: usize, state: CellState) {
        if let Some(i) = self.index_of(row, col)
            && let Some(cell) = self.cells.get_mut(i)
        {
            cell.state = state;
        }
    }

    // ── The clock ──────────────────────────────────────────────────────────

    /// Advance the clock by `elapsed_ms` of wall time.
    ///
    /// By the time that has passed, not by one second per call. A clock that
    /// counts calls reports how often the window happened to wake it, which is
    /// a fact about the window and not about the game.
    pub fn tick(&mut self, elapsed_ms: u64) -> EventResult {
        if self.status != GameStatus::Playing {
            return EventResult::Ignored;
        }
        let before = self.elapsed_secs();
        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
        if self.elapsed_secs() == before {
            // The displayed clock has not moved, so there is nothing to repaint.
            return EventResult::Ignored;
        }
        EventResult::Consumed
    }

    /// Remember the size the window is now, so the next click is read against
    /// the frame the player is actually looking at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(1.0), height.max(1.0));
    }

    /// The size the last frame was drawn at.
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    // ── Input ──────────────────────────────────────────────────────────────

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        let (row, col) = self.cursor;
        if key.modifiers.ctrl {
            let difficulty = match key.key {
                Key::Num1 => Difficulty::Beginner,
                Key::Num2 => Difficulty::Intermediate,
                Key::Num3 => Difficulty::Expert,
                _ => return EventResult::Ignored,
            };
            return self.apply(Action::SetDifficulty(difficulty));
        }
        if key.modifiers.alt || key.modifiers.super_key {
            return EventResult::Ignored;
        }
        let action = match key.key {
            Key::Up => Action::Move(Dir::Up),
            Key::Down => Action::Move(Dir::Down),
            Key::Left => Action::Move(Dir::Left),
            Key::Right => Action::Move(Dir::Right),
            Key::Space | Key::Enter => Action::Reveal(row, col),
            Key::F => Action::Flag(row, col),
            Key::C => Action::Chord(row, col),
            Key::D => Action::CycleDifficulty,
            Key::N | Key::F2 => Action::NewGame,
            _ => return EventResult::Ignored,
        };
        self.apply(action)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        let hit = self
            .frame(self.size.0, self.size.1)
            .hit_test(event.x, event.y);
        let action = match (&event.kind, hit) {
            (MouseEventKind::Press(MouseButton::Left), Some(Target::Cell(r, c))) => {
                Action::Reveal(r, c)
            }
            (MouseEventKind::Press(MouseButton::Right), Some(Target::Cell(r, c))) => {
                Action::Flag(r, c)
            }
            // Middle-click and double-click both chord, because both are what
            // players reach for and neither means anything else here.
            (
                MouseEventKind::Press(MouseButton::Middle)
                | MouseEventKind::DoubleClick(MouseButton::Left),
                Some(Target::Cell(r, c)),
            ) => Action::Chord(r, c),
            (MouseEventKind::Press(MouseButton::Left), Some(Target::Difficulty)) => {
                Action::CycleDifficulty
            }
            (MouseEventKind::Press(MouseButton::Left), Some(Target::NewGame)) => Action::NewGame,
            _ => return EventResult::Ignored,
        };
        self.apply(action)
    }
}

/// A flat index split into its row and column, or `None` if `cols` is zero.
#[must_use]
pub fn split(index: usize, cols: usize) -> Option<(usize, usize)> {
    Some((index.checked_div(cols)?, index.checked_rem(cols)?))
}

/// `MM:SS`, counting past an hour rather than wrapping at one.
///
/// A word about the two digits: they are a minimum and not a maximum, so a
/// two-hour game reads `120:00` rather than `00:00` again.
#[must_use]
pub fn format_time(secs: u64) -> String {
    let mins = secs.checked_div(60).unwrap_or(0);
    let rest = secs.checked_rem(60).unwrap_or(0);
    format!("{mins:02}:{rest:02}")
}

/// The face shown in the middle of the header.
#[must_use]
pub fn status_text(status: GameStatus) -> &'static str {
    match status {
        GameStatus::Ready => "Ready",
        GameStatus::Playing => "Playing",
        GameStatus::Lost => "Boom",
        GameStatus::Won => "Cleared",
    }
}

/// The colour the status word is drawn in.
#[must_use]
pub fn status_color(status: GameStatus) -> Color {
    match status {
        GameStatus::Ready => SUBTEXT0,
        GameStatus::Playing => BLUE,
        GameStatus::Lost => RED,
        GameStatus::Won => GREEN,
    }
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// Which band is given up first when the window is too short for all of them.
/// The footer is a reminder; the header carries the mine counter and the clock,
/// which are part of playing.
pub const BAND_DROP_ORDER: [usize; 2] = [1, 0];

/// How much of the window's height the board is entitled to keep.
pub const BOARD_SHARE: f32 = 0.55;

/// Where everything goes in a window of a given size.
///
/// Derived on every frame and never stored on the model. The version this
/// replaces had no layout at all: eight constants, a 30-pixel cell, and a
/// `window_width()` that told the window what size to be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The strip along the top, or `Rect::EMPTY` if it did not fit.
    pub header: Rect,
    /// The strip along the bottom, or `Rect::EMPTY` if it did not fit.
    pub footer: Rect,
    /// The band the board is drawn in.
    pub board: Rect,
    /// The board's own rectangle inside that band, centred.
    pub grid: Rect,
    /// The side of one cell, including the gap that follows it.
    pub step: f32,
    /// The side of the painted part of a cell.
    pub cell: f32,
    /// The body font size.
    pub font: f32,
    /// The heading font size.
    pub big: f32,
    /// The margin used throughout.
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size holding a `rows` x `cols`
    /// board.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a board is at most 30 cells on a side, exact in f32"
    )]
    pub fn new(width: f32, height: f32, rows: usize, cols: usize) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 42.0).clamp(7.0, 15.0);
        let big = (font * 1.5).clamp(10.0, 24.0);
        // A margin may never be more than a quarter of the side it is taken
        // from: a two-pixel floor is wider than a 1x1 window, and a margin that
        // does not fit inside the thing it is a margin of puts the content it
        // indents outside the window.
        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 4.0);

        // What each band would like, in [header, footer] order.
        let mut wants = [(h * 0.11).clamp(26.0, 58.0), (h * 0.055).clamp(16.0, 28.0)];
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [head_h, foot_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall. Both look the same to a fill, and only one of them looks the
        // same to a reader asking "is this band gone, or merely thin?"
        let header = if head_h > 0.0 {
            Rect::new(0.0, 0.0, w, head_h)
        } else {
            Rect::EMPTY
        };
        let footer = if foot_h > 0.0 {
            Rect::new(0.0, (h - foot_h).max(0.0), w, foot_h)
        } else {
            Rect::EMPTY
        };
        let board = Rect::new(
            pad,
            head_h + pad,
            (w - pad * 2.0).max(0.0),
            (h - head_h - foot_h - pad * 2.0).max(0.0),
        );

        // One cell is a square, so its side is whichever of the two fits.
        let across = if cols == 0 {
            board.w
        } else {
            board.w / cols as f32
        };
        let down = if rows == 0 {
            board.h
        } else {
            board.h / rows as f32
        };
        let natural = across.min(down);
        // Below a pixel a cell there is no board to draw, and the two wrong
        // answers are both worth naming. Rounding the cell *up* to a pixel — the
        // first thing tried here — makes a nine-cell row nine pixels wide in a
        // window one pixel wide, so the board is painted, and clicked, outside
        // the window it belongs to. Leaving the cell at nought instead stacks
        // every cell's hit box at one point, so a click on the origin lands on
        // whichever cell was recorded last. Neither is a board, so this draws
        // none: `grid` is empty, `cell_rect` answers `Rect::EMPTY`, and
        // `draw_cell` returns before it records anything.
        let (step, cell) = if natural < 1.0 {
            (0.0, 0.0)
        } else {
            // The gap is taken out of the cell rather than added to it, so
            // `cols` cells never come to more than `cols * step`.
            (
                natural,
                (natural - (natural * 0.08).clamp(0.0, 3.0)).max(1.0),
            )
        };
        let grid_w = step * cols as f32;
        let grid_h = step * rows as f32;
        let grid = Rect::new(
            board.x + (board.w - grid_w).max(0.0) / 2.0,
            board.y + (board.h - grid_h).max(0.0) / 2.0,
            grid_w,
            grid_h,
        );

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            footer,
            board,
            grid,
            step,
            cell,
            font,
            big,
            pad,
        }
    }

    /// Whether a band is worth drawing into.
    #[must_use]
    pub fn shows(&self, r: Rect) -> bool {
        !r.is_empty() && r.w > 0.0 && r.h > 0.0
    }

    /// The rectangle of the cell at `(row, col)`, or `Rect::EMPTY` in a window
    /// with no room for a board. See [`Layout::new`].
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a board is at most 30 cells on a side, exact in f32"
    )]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if self.cell <= 0.0 {
            return Rect::EMPTY;
        }
        Rect::new(
            self.grid.x + col as f32 * self.step,
            self.grid.y + row as f32 * self.step,
            self.cell,
            self.cell,
        )
    }

    /// The `i`th chip in the header, counted from the right.
    #[must_use]
    #[allow(clippy::cast_precision_loss, reason = "there are two chips")]
    pub fn chip(&self, i: usize) -> Rect {
        if !self.shows(self.header) {
            return Rect::EMPTY;
        }
        let w = (self.header.w * 0.16).clamp(40.0, 130.0);
        let h = (self.header.h * 0.6).max(1.0);
        let gap = self.pad;
        let right = self.header.right() - self.pad;
        let x = right - (w + gap) * (i as f32 + 1.0) + gap;
        Rect::new(
            x.max(self.header.x),
            self.header.y + (self.header.h - h) / 2.0,
            w.min((right - self.header.x).max(0.0)),
            h,
        )
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

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
        corner_radii: CornerRadii::all(radius),
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
        corner_radii: CornerRadii::all(radius),
    });
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    s: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if size <= 0.0 || s.is_empty() || max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

fn centred_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.is_empty() || size <= 0.0 {
        return;
    }
    let w = text::measure(s, size, weight);
    let line_h = text::line_height(size, weight);
    // Centring moves the start left, so the width to fit in has to be measured
    // from the start actually chosen and not from the box's -- passing the
    // box's whole width from a start half a box to its left puts the ellipsis
    // point half a box past the right edge, which is a promise to clip that
    // clips nothing.
    let x = (r.x + (r.w - w) / 2.0).max(r.x);
    label(
        f,
        x,
        r.y + (r.h - line_h) / 2.0,
        s,
        size,
        color,
        weight,
        Some((r.right() - x).max(0.0)),
    );
}

fn left_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.is_empty() || size <= 0.0 {
        return;
    }
    let line_h = text::line_height(size, weight);
    label(
        f,
        r.x,
        r.y + (r.h - line_h) / 2.0,
        s,
        size,
        color,
        weight,
        Some(r.w),
    );
}

fn chip(f: &mut Frame, r: Rect, target: Target, s: &str, size: f32, accent: Color) {
    if r.is_empty() {
        return;
    }
    fill(f, r, SURFACE0, 5.0);
    f.hit(target, r);
    centred_in(f, r, s, size, accent, FontWeightHint::Bold);
}

impl MinesweeperApp {
    /// Draw the whole window at `width` x `height`, recording a hit box for
    /// every control drawn.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height, self.rows(), self.cols());
        let mut f = Frame::new(width, height);
        // The background is the window's, not the board's. This program used to
        // fill a rectangle it had computed from the cell size, which is a
        // picture of a window rather than the window.
        fill(&mut f, l.window, BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_footer(&mut f, &l);
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        fill(f, l.header, MANTLE, 0.0);

        let chips_left = l.chip(1).x;
        let left = Rect::new(
            l.header.x + l.pad,
            l.header.y,
            (chips_left - l.header.x - l.pad * 2.0).max(0.0),
            l.header.h,
        );
        if !left.is_empty() {
            let top = Rect::new(left.x, left.y, left.w, left.h / 2.0);
            let bottom = Rect::new(left.x, left.y + left.h / 2.0, left.w, left.h / 2.0);
            left_in(
                f,
                top,
                &format!("Mines {}", self.mines_remaining()),
                l.big,
                BLUE,
                FontWeightHint::Bold,
            );
            let line = format!(
                "{}   {}   {}",
                format_time(self.elapsed_secs()),
                status_text(self.status),
                self.difficulty.label()
            );
            left_in(
                f,
                bottom,
                &line,
                l.font,
                status_color(self.status),
                FontWeightHint::Regular,
            );
        }

        chip(
            f,
            l.chip(1),
            Target::Difficulty,
            self.difficulty.label(),
            l.font,
            self.difficulty.color(),
        );
        chip(f, l.chip(0), Target::NewGame, "New", l.font, LAVENDER);
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.board) {
            return;
        }
        for row in 0..self.rows() {
            for col in 0..self.cols() {
                self.draw_cell(f, l, row, col);
            }
        }
    }

    fn draw_cell(&self, f: &mut Frame, l: &Layout, row: usize, col: usize) {
        let r = l.cell_rect(row, col);
        if r.is_empty() {
            return;
        }
        let Some(cell) = self.cell(row, col) else {
            return;
        };
        let lost_here = self.losing_cell == Some((row, col));
        let face = match cell.state {
            CellState::Hidden => SURFACE1,
            CellState::Flagged => SURFACE2,
            CellState::Revealed if lost_here => RED,
            CellState::Revealed => SURFACE0,
        };
        fill(f, r, face, (l.cell * 0.12).clamp(0.0, 4.0));
        // Every cell is clickable, including the revealed ones -- a chord is a
        // click on a number.
        f.hit(Target::Cell(row, col), r);

        let size = (l.cell * 0.55).clamp(0.0, 22.0);
        match cell.state {
            CellState::Flagged => {
                centred_in(f, r, "F", size, PEACH, FontWeightHint::Bold);
            }
            CellState::Revealed if cell.is_mine => {
                let ink = if lost_here { BASE } else { RED };
                centred_in(f, r, "*", size, ink, FontWeightHint::Bold);
            }
            CellState::Revealed if cell.adjacent > 0 => {
                let ink = NUMBER_COLORS
                    .get(usize::from(cell.adjacent).saturating_sub(1))
                    .copied()
                    .unwrap_or(TEXT_COLOR);
                centred_in(
                    f,
                    r,
                    &cell.adjacent.to_string(),
                    size,
                    ink,
                    FontWeightHint::Bold,
                );
            }
            CellState::Hidden | CellState::Revealed => {}
        }

        if self.cursor == (row, col) {
            stroke(f, r, LAVENDER, (l.cell * 0.06).clamp(1.0, 3.0), 0.0);
        }
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.footer) {
            return;
        }
        fill(f, l.footer, MANTLE, 0.0);
        let mut x = l.footer.x + l.pad;
        for &(key, what) in SHORTCUTS {
            let s = format!("{key} {what}");
            let w = text::measure(&s, l.font, FontWeightHint::Regular);
            if x + w > l.footer.right() - l.pad {
                break;
            }
            left_in(
                f,
                Rect::new(x, l.footer.y, w, l.footer.h),
                &s,
                l.font,
                OVERLAY0,
                FontWeightHint::Regular,
            );
            x += w + l.pad * 2.0;
        }
    }
}

/// Route one event to the model. The single door every input comes through, so
/// a test drives the program the way a player does.
pub fn handle_event(app: &mut MinesweeperApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => app.handle_mouse(mouse),
        Event::Tick { elapsed_ms } => app.tick(*elapsed_ms),
        Event::Resize { width, height } => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a window dimension is far below f32's exact-integer range"
            )]
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for MinesweeperApp {
    fn title(&self) -> String {
        "Minesweeper".to_string()
    }

    fn app_id(&self) -> String {
        "minesweeper".to_string()
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both constants are small positive integers written as f32"
    )]
    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Asked after every event, so the clock gets a wake-up exactly while there
    /// is something for it to move. Leaving this at the default gets no ticks
    /// at all — which is what this program did, with a timer on the model, an
    /// `MM:SS` in the header and nothing on earth to advance it.
    fn tick_interval(&self) -> Option<Duration> {
        if self.status == GameStatus::Playing {
            Some(Duration::from_millis(CLOCK_MS))
        } else {
            None
        }
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
        // against -- that is the whole point of storing it.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for MinesweeperApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
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
    let mut app = MinesweeperApp::new();
    app::launch("minesweeper", &mut app)
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "a test that panics on bad data is a test that failed, which is \
              the point"
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe::{self, ctrl, press, press_with};
    use std::collections::HashSet;

    // ── Helpers ────────────────────────────────────────────────────────────

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    /// Every window shape worth asking a layout question at: the default, the
    /// degenerate, the very wide, the very tall, and a few in between.
    const SIZES: [(f32, f32); 9] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (1.0, 1.0),
        (2.0, 400.0),
        (400.0, 2.0),
        (320.0, 240.0),
        (1920.0, 1080.0),
        (600.0, 200.0),
        (200.0, 600.0),
        (1000.0, 1000.0),
    ];

    fn game(seed: u64) -> MinesweeperApp {
        MinesweeperApp::with_seed(Difficulty::Beginner, seed)
    }

    fn tick(a: &mut MinesweeperApp, ms: u64) -> EventResult {
        handle_event(a, &Event::Tick { elapsed_ms: ms })
    }

    fn key(a: &mut MinesweeperApp, k: Key) -> EventResult {
        handle_event(a, &Event::Key(press(k)))
    }

    fn mouse(a: &mut MinesweeperApp, x: f32, y: f32, kind: MouseEventKind) -> EventResult {
        handle_event(a, &Event::Mouse(MouseEvent { x, y, kind }))
    }

    fn layout_of(a: &MinesweeperApp) -> Layout {
        Layout::new(a.size().0, a.size().1, a.rows(), a.cols())
    }

    /// Walk the cursor to `(row, col)` with `Action::Move`, and give up rather
    /// than spin.
    ///
    /// The bound is what matters here. Written the obvious way — `while
    /// a.cursor() != (row, col)` — this loop never returns against *any*
    /// program whose cursor has stopped moving, and five separate mutations do
    /// exactly that (a step that stays put, an up that goes down, a left that
    /// goes up, a move that reports it moved without moving, and a move that
    /// uncovers as well). Every one of them turned into the same 240-second
    /// hang, which names no test, distinguishes no fault from any other, and
    /// costs four minutes each to learn nothing. That is `known-issues.md`'s
    /// maze finding — *an unbounded loop in a test helper converts every fault
    /// it depends on into the same undiagnosable hang* — recurring in a suite
    /// written after it was recorded, which is the argument for making the
    /// bound a habit rather than a repair.
    ///
    /// Four steps per axis more than the board is wide is slack enough for any
    /// route this walk can take, since it closes one axis at a time and each
    /// step closes it by one.
    fn walk_cursor(
        a: &mut MinesweeperApp,
        row: usize,
        col: usize,
        mut step: impl FnMut(&mut MinesweeperApp, Dir),
    ) {
        let bound = a.rows() + a.cols() + 8;
        for _ in 0..bound {
            let (cr, cc) = a.cursor();
            if (cr, cc) == (row, col) {
                return;
            }
            if cr < row {
                step(a, Dir::Down);
            } else if cr > row {
                step(a, Dir::Up);
            } else if cc < col {
                step(a, Dir::Right);
            } else {
                step(a, Dir::Left);
            }
        }
        panic!(
            "the cursor did not reach {row},{col} in {bound} moves — it is at \
             {:?}, so a move is not moving where it says it does",
            a.cursor()
        );
    }

    /// Walk the cursor with `Action::Move`, for tests whose subject is not the
    /// keyboard.
    fn walk_cursor_to(a: &mut MinesweeperApp, row: usize, col: usize) {
        walk_cursor(a, row, col, |a, d| {
            a.apply(Action::Move(d));
        });
    }

    /// Walk the cursor with the arrow keys, through the whole event path, for
    /// the test whose subject *is* the keyboard.
    fn walk_cursor_with_keys(a: &mut MinesweeperApp, row: usize, col: usize) {
        walk_cursor(a, row, col, |a, d| {
            key(
                a,
                match d {
                    Dir::Up => Key::Up,
                    Dir::Down => Key::Down,
                    Dir::Left => Key::Left,
                    Dir::Right => Key::Right,
                },
            );
        });
    }

    fn cell_point(a: &MinesweeperApp, row: usize, col: usize) -> (f32, f32) {
        let r = layout_of(a).cell_rect(row, col);
        assert!(!r.is_empty(), "cell {row},{col} is not drawn");
        r.centre()
    }

    /// Click a cell with a named button, the way a player does — through the
    /// hit box the drawing pass recorded, not through `apply`.
    fn click_cell(a: &mut MinesweeperApp, row: usize, col: usize, b: MouseButton) -> EventResult {
        let (x, y) = cell_point(a, row, col);
        mouse(a, x, y, MouseEventKind::Press(b))
    }

    fn reveal(a: &mut MinesweeperApp, row: usize, col: usize) -> EventResult {
        click_cell(a, row, col, MouseButton::Left)
    }

    fn flag(a: &mut MinesweeperApp, row: usize, col: usize) -> EventResult {
        click_cell(a, row, col, MouseButton::Right)
    }

    /// Every `(row, col)` on the board, in reading order.
    fn all_cells(a: &MinesweeperApp) -> Vec<(usize, usize)> {
        (0..a.rows())
            .flat_map(|r| (0..a.cols()).map(move |c| (r, c)))
            .collect()
    }

    fn mines_of(a: &MinesweeperApp) -> Vec<(usize, usize)> {
        all_cells(a)
            .into_iter()
            .filter(|&(r, c)| a.is_mine(r, c))
            .collect()
    }

    fn safe_of(a: &MinesweeperApp) -> Vec<(usize, usize)> {
        all_cells(a)
            .into_iter()
            .filter(|&(r, c)| !a.is_mine(r, c))
            .collect()
    }

    /// A game with the mines already buried, opened at `(0, 0)`.
    fn started(seed: u64) -> MinesweeperApp {
        let mut a = game(seed);
        reveal(&mut a, 0, 0);
        assert_eq!(a.status(), GameStatus::Playing, "seed {seed} lost at once");
        a
    }

    /// Uncover every safe cell, which is the only way to win.
    fn clear_the_board(a: &mut MinesweeperApp) {
        for (r, c) in safe_of(a) {
            if !a.is_revealed(r, c) {
                a.apply(Action::Reveal(r, c));
            }
        }
    }

    /// The first revealed cell whose `adjacent` is `n`, if the board has one.
    fn a_number(a: &MinesweeperApp, n: u8) -> Option<(usize, usize)> {
        all_cells(a)
            .into_iter()
            .find(|&(r, c)| a.is_revealed(r, c) && !a.is_mine(r, c) && a.adjacent(r, c) == n)
    }

    fn texts(f: &Frame) -> Vec<String> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn text_color(f: &Frame, want: &str) -> Option<Color> {
        f.commands().iter().find_map(|c| match c {
            RenderCommand::Text { text, color, .. } if text == want => Some(*color),
            _ => None,
        })
    }

    fn fill_color_at(f: &Frame, at: Rect) -> Option<Color> {
        f.commands().iter().rev().find_map(|c| match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } if (*x - at.x).abs() < 0.01
                && (*y - at.y).abs() < 0.01
                && (*width - at.w).abs() < 0.01
                && (*height - at.h).abs() < 0.01 =>
            {
                Some(*color)
            }
            _ => None,
        })
    }

    fn strokes(f: &Frame) -> Vec<Rect> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect()
    }

    // ── Difficulty ─────────────────────────────────────────────────────────

    #[test]
    fn a_board_has_room_for_its_mines_and_the_first_clicks_safe_zone() {
        for d in Difficulty::ALL {
            // The safe zone is the clicked cell and its eight neighbours, so
            // nine cells at most. If the mines and the safe zone together
            // wanted more than the board has, the board could not be dealt.
            assert!(
                d.mines() + 9 <= d.cells(),
                "{} wants {} mines on {} cells",
                d.label(),
                d.mines(),
                d.cells()
            );
        }
    }

    #[test]
    fn the_three_difficulties_are_the_real_minesweeper_boards() {
        // Written out rather than derived, because these numbers are the
        // convention this game inherits and not something the program computes.
        // A test that asked the program what its own numbers were could not
        // notice one being changed.
        let want = [
            (Difficulty::Beginner, 9, 9, 10),
            (Difficulty::Intermediate, 16, 16, 40),
            (Difficulty::Expert, 16, 30, 99),
        ];
        for (d, rows, cols, mines) in want {
            assert_eq!(d.rows(), rows, "{} rows", d.label());
            assert_eq!(d.cols(), cols, "{} cols", d.label());
            assert_eq!(d.mines(), mines, "{} mines", d.label());
            assert_eq!(d.cells(), rows * cols, "{} cells", d.label());
        }
    }

    #[test]
    fn every_difficulty_is_harder_than_the_last() {
        let mut prev: Option<Difficulty> = None;
        for d in Difficulty::ALL {
            if let Some(p) = prev {
                assert!(
                    d.mines() > p.mines(),
                    "{} has no more mines than {}",
                    d.label(),
                    p.label()
                );
            }
            prev = Some(d);
        }
    }

    #[test]
    fn cycling_difficulty_visits_all_three_and_comes_home() {
        let mut d = Difficulty::Beginner;
        let mut seen = vec![d];
        for _ in 0..Difficulty::ALL.len() {
            d = d.next();
            seen.push(d);
        }
        assert_eq!(seen.first(), seen.last(), "the cycle does not close");
        let distinct: HashSet<&str> = seen.iter().map(|d| d.label()).collect();
        assert_eq!(distinct.len(), Difficulty::ALL.len(), "a level is skipped");
    }

    #[test]
    fn every_difficulty_has_its_own_name_and_its_own_colour() {
        let names: HashSet<&str> = Difficulty::ALL.iter().map(|d| d.label()).collect();
        assert_eq!(
            names.len(),
            Difficulty::ALL.len(),
            "two levels share a name"
        );
        let colors: HashSet<Color> = Difficulty::ALL.iter().map(|d| d.color()).collect();
        assert_eq!(
            colors.len(),
            Difficulty::ALL.len(),
            "two levels share a colour"
        );
        assert!(
            Difficulty::ALL.iter().all(|d| !d.label().is_empty()),
            "a level has no name"
        );
    }

    // ── Steps and neighbours ───────────────────────────────────────────────

    #[test]
    fn a_step_back_from_the_edge_falls_off_rather_than_wrapping() {
        assert_eq!(Step::Back.from(0), None, "row -1 wrapped to the far side");
        assert_eq!(Step::Back.from(1), Some(0));
        assert_eq!(Step::Stay.from(0), Some(0));
        assert_eq!(Step::Fwd.from(0), Some(1));
    }

    #[test]
    fn the_eight_neighbour_offsets_are_eight_distinct_moves_and_none_is_staying_put() {
        let set: HashSet<(usize, usize)> = NEIGHBOURS
            .iter()
            .map(|&(dr, dc)| (dr.from(5).unwrap(), dc.from(5).unwrap()))
            .collect();
        assert_eq!(set.len(), 8, "two offsets name the same cell");
        assert!(!set.contains(&(5, 5)), "a cell is its own neighbour");
    }

    #[test]
    fn the_four_arrow_directions_are_the_four_unit_steps() {
        let set: HashSet<(usize, usize)> = [Dir::Up, Dir::Down, Dir::Left, Dir::Right]
            .into_iter()
            .map(|d| {
                let (dr, dc) = d.steps();
                (dr.from(5).unwrap(), dc.from(5).unwrap())
            })
            .collect();
        assert_eq!(
            set,
            HashSet::from([(4, 5), (6, 5), (5, 4), (5, 6)]),
            "an arrow key does not move one cell in its own direction"
        );
    }

    #[test]
    fn a_cell_in_the_middle_has_eight_neighbours_and_a_corner_has_three() {
        let a = game(1);
        assert_eq!(a.neighbours(4, 4).count(), 8);
        assert_eq!(a.neighbours(0, 0).count(), 3);
        assert_eq!(a.neighbours(0, 4).count(), 5, "a top edge");
        let (last_r, last_c) = (a.rows() - 1, a.cols() - 1);
        assert_eq!(a.neighbours(last_r, last_c).count(), 3, "the far corner");
    }

    #[test]
    fn every_neighbour_is_on_the_board_and_none_is_the_cell_itself() {
        let a = game(2);
        for (r, c) in all_cells(&a) {
            let ns: Vec<_> = a.neighbours(r, c).collect();
            let set: HashSet<_> = ns.iter().copied().collect();
            assert_eq!(set.len(), ns.len(), "cell {r},{c} lists a neighbour twice");
            for &(nr, nc) in &ns {
                assert!(nr < a.rows() && nc < a.cols(), "{nr},{nc} is off the board");
                assert_ne!((nr, nc), (r, c), "cell {r},{c} neighbours itself");
            }
        }
    }

    #[test]
    fn neighbourhood_is_mutual() {
        let a = game(3);
        for (r, c) in all_cells(&a) {
            for (nr, nc) in a.neighbours(r, c) {
                assert!(
                    a.neighbours(nr, nc).any(|p| p == (r, c)),
                    "{r},{c} sees {nr},{nc} but not the other way round"
                );
            }
        }
    }

    #[test]
    fn an_index_and_a_row_and_column_name_the_same_cell() {
        // Expert, not Beginner, and that is the whole point of the test. A row
        // is `row * cols + col`, and Beginner is nine by nine and Intermediate
        // sixteen by sixteen -- on a square board `rows()` and `cols()` are the
        // same number, so a version of this that multiplied by the wrong one
        // passed every assertion below. Expert is the only board whose two
        // dimensions differ (30 columns, 16 rows), and it is the only board on
        // which this test means anything.
        let a = MinesweeperApp::with_seed(Difficulty::Expert, 4);
        assert_ne!(a.rows(), a.cols(), "a square board cannot tell them apart");
        for (r, c) in all_cells(&a) {
            let i = a.index_of(r, c).expect("on the board");
            assert_eq!(split(i, a.cols()), Some((r, c)), "index {i} split wrong");
        }
        assert_eq!(a.index_of(a.rows(), 0), None, "a row past the bottom");
        assert_eq!(a.index_of(0, a.cols()), None, "a column past the right");
        assert_eq!(split(5, 0), None, "a board with no columns");
    }

    #[test]
    fn a_cell_off_the_board_is_nothing_rather_than_a_blank_one() {
        let a = game(5);
        assert_eq!(a.cell(a.rows(), 0), None);
        assert!(!a.is_mine(99, 99));
        assert!(!a.is_revealed(99, 99));
        assert!(!a.is_flagged(99, 99));
        assert_eq!(a.adjacent(99, 99), 0);
    }

    // ── Dealing ────────────────────────────────────────────────────────────

    #[test]
    fn a_fresh_board_is_ready_empty_and_unmined() {
        let a = game(7);
        assert_eq!(a.status(), GameStatus::Ready);
        assert_eq!(
            a.mine_count(),
            0,
            "mines were buried before the first click"
        );
        assert_eq!(a.revealed_count(), 0);
        assert_eq!(a.flag_count(), 0);
        assert_eq!(a.elapsed_ms(), 0);
        assert_eq!(a.cursor(), (0, 0));
        assert_eq!(a.losing_cell(), None);
    }

    #[test]
    fn the_first_click_buries_exactly_the_advertised_number_of_mines() {
        for d in Difficulty::ALL {
            let mut a = MinesweeperApp::with_seed(d, 0xABC);
            a.apply(Action::Reveal(0, 0));
            assert_eq!(
                a.mine_count(),
                d.mines(),
                "{} buried the wrong number",
                d.label()
            );
        }
    }

    #[test]
    fn the_first_click_and_everything_around_it_is_safe() {
        // Every cell of the board, as the first click, on several boards: the
        // opening move must never be a loss and must never be a one.
        for seed in 0..12u64 {
            let probe = game(seed);
            for (r, c) in all_cells(&probe) {
                let mut a = game(seed);
                a.apply(Action::Reveal(r, c));
                assert_eq!(
                    a.status(),
                    GameStatus::Playing,
                    "seed {seed} lost at {r},{c}"
                );
                assert!(!a.is_mine(r, c), "seed {seed} mined the first click");
                for (nr, nc) in a.neighbours(r, c) {
                    assert!(
                        !a.is_mine(nr, nc),
                        "seed {seed} mined {nr},{nc} beside the first click {r},{c}"
                    );
                }
            }
        }
    }

    #[test]
    fn one_seed_is_one_board() {
        let a = started(999);
        let b = started(999);
        assert_eq!(mines_of(&a), mines_of(&b), "the same seed dealt twice");
        assert_ne!(
            mines_of(&a),
            mines_of(&started(1000)),
            "two seeds dealt the same board"
        );
    }

    #[test]
    fn one_named_seed_names_one_named_board() {
        // A literal from outside the program. Every other board test asks the
        // program where its mines are and then checks something about that
        // answer, which cannot notice the generator being rewired; this one
        // says where they must be.
        let a = started(4242);
        let where_they_are: Vec<(usize, usize)> = mines_of(&a);
        assert_eq!(
            where_they_are,
            vec![
                (0, 2),
                (0, 5),
                (2, 2),
                (2, 8),
                (4, 7),
                (5, 5),
                (6, 0),
                (7, 1),
                (7, 4),
                (8, 0),
            ],
            "seed 4242's board moved"
        );
    }

    #[test]
    fn every_count_is_the_number_of_mines_around_that_cell() {
        for seed in 0..6u64 {
            let a = started(seed);
            for (r, c) in all_cells(&a) {
                let want = a.neighbours(r, c).filter(|&(x, y)| a.is_mine(x, y)).count();
                assert_eq!(
                    usize::from(a.adjacent(r, c)),
                    want,
                    "seed {seed} cell {r},{c}"
                );
            }
        }
    }

    #[test]
    fn a_second_game_is_not_the_first_one_shifted_by_one() {
        // `deal` draws the next seed from this game's own generator. It used to
        // be `seed + 1`, which is not a fresh draw but one step along a single
        // stream: the second board was a fixed function of the first.
        let mut a = game(500);
        a.apply(Action::NewGame);
        let next = a.seed();
        assert_ne!(next, 501, "the next game is the next integer");
        assert_ne!(next, 500, "the next game is the same game");
    }

    #[test]
    fn a_new_game_clears_the_clock_the_flags_and_the_wreckage() {
        let mut a = started(11);
        flag(&mut a, 0, 1);
        tick(&mut a, 5_000);
        let (r, c) = *mines_of(&a).first().expect("a mine");
        a.apply(Action::Reveal(r, c));
        assert_eq!(a.status(), GameStatus::Lost);

        a.apply(Action::NewGame);
        assert_eq!(a.status(), GameStatus::Ready);
        assert_eq!(a.elapsed_ms(), 0);
        assert_eq!(a.flag_count(), 0);
        assert_eq!(a.revealed_count(), 0);
        assert_eq!(a.mine_count(), 0);
        assert_eq!(a.losing_cell(), None);
        assert_eq!(a.cursor(), (0, 0));
    }

    #[test]
    fn changing_difficulty_deals_a_board_of_the_new_size() {
        let mut a = started(12);
        assert_eq!(a.difficulty(), Difficulty::Beginner);
        a.apply(Action::CycleDifficulty);
        assert_eq!(a.difficulty(), Difficulty::Intermediate);
        assert_eq!(a.rows(), 16);
        assert_eq!(a.cols(), 16);
        assert_eq!(a.status(), GameStatus::Ready);
        assert_eq!(a.cell(15, 15).map(|c| c.state), Some(CellState::Hidden));

        a.apply(Action::SetDifficulty(Difficulty::Expert));
        assert_eq!(a.difficulty(), Difficulty::Expert);
        assert_eq!(a.cols(), 30);
    }

    // ── Revealing ──────────────────────────────────────────────────────────

    #[test]
    fn uncovering_a_hidden_cell_uncovers_it_and_moves_the_cursor_there() {
        let mut a = game(20);
        assert_eq!(reveal(&mut a, 4, 4), EventResult::Consumed);
        assert!(a.is_revealed(4, 4));
        assert_eq!(a.cursor(), (4, 4), "the click did not take the cursor");
        assert_eq!(a.status(), GameStatus::Playing);
    }

    #[test]
    fn uncovering_something_already_uncovered_changes_nothing() {
        let mut a = started(21);
        let before = a.revealed_count();
        assert_eq!(a.apply(Action::Reveal(0, 0)), EventResult::Ignored);
        assert_eq!(a.revealed_count(), before);
    }

    #[test]
    fn a_flag_stops_the_click_that_would_have_uncovered_the_cell() {
        // The whole point of a flag: it is a guard against your own hand.
        let mut a = started(22);
        let (r, c) = *safe_of(&a)
            .iter()
            .find(|&&(r, c)| !a.is_revealed(r, c))
            .expect("a hidden safe cell");
        flag(&mut a, r, c);
        assert_eq!(reveal(&mut a, r, c), EventResult::Ignored);
        assert!(!a.is_revealed(r, c), "a flagged cell was uncovered");
        assert!(a.is_flagged(r, c), "the click ate the flag");
    }

    #[test]
    fn uncovering_a_cell_that_is_not_there_is_ignored() {
        let mut a = game(23);
        assert_eq!(a.apply(Action::Reveal(99, 99)), EventResult::Ignored);
        assert_eq!(a.status(), GameStatus::Ready, "an off-board click dealt");
    }

    #[test]
    fn uncovering_a_mine_loses_marks_where_and_shows_every_other_mine() {
        let mut a = started(24);
        let mines = mines_of(&a);
        let (r, c) = *mines.first().expect("a mine");
        assert_eq!(a.apply(Action::Reveal(r, c)), EventResult::Consumed);
        assert_eq!(a.status(), GameStatus::Lost);
        assert_eq!(a.losing_cell(), Some((r, c)));
        for &(mr, mc) in &mines {
            assert!(a.is_revealed(mr, mc), "mine {mr},{mc} stayed hidden");
        }
    }

    #[test]
    fn a_flagged_mine_stays_flagged_when_the_game_is_lost() {
        // The board is a record of what the player believed. Uncovering a mine
        // they had correctly flagged would erase the one thing they got right.
        let mut a = started(25);
        let mines = mines_of(&a);
        let (fr, fc) = *mines.first().expect("a mine");
        let (br, bc) = *mines.get(1).expect("a second mine");
        flag(&mut a, fr, fc);
        a.apply(Action::Reveal(br, bc));
        assert_eq!(a.status(), GameStatus::Lost);
        assert!(a.is_flagged(fr, fc), "the loss ate a correct flag");
    }

    #[test]
    fn a_lost_game_answers_nothing_more() {
        let mut a = started(26);
        let (r, c) = *mines_of(&a).first().expect("a mine");
        a.apply(Action::Reveal(r, c));
        let (sr, sc) = *safe_of(&a)
            .iter()
            .find(|&&(r, c)| !a.is_revealed(r, c))
            .expect("a hidden safe cell");
        assert_eq!(a.apply(Action::Reveal(sr, sc)), EventResult::Ignored);
        assert_eq!(a.apply(Action::Flag(sr, sc)), EventResult::Ignored);
        assert_eq!(a.apply(Action::Chord(sr, sc)), EventResult::Ignored);
        assert!(a.is_over());
    }

    #[test]
    fn uncovering_a_cell_that_touches_nothing_opens_the_whole_clearing() {
        let mut a = started(27);
        // The opening click at (0,0) is guaranteed safe, and on a board this
        // dense it nearly always touches a zero. Assert on the shape of what
        // opened rather than on how much: every revealed cell is reachable from
        // (0,0) through zeroes.
        assert!(a.is_revealed(0, 0));
        if a.adjacent(0, 0) == 0 {
            for (nr, nc) in a.neighbours(0, 0) {
                assert!(
                    a.is_revealed(nr, nc),
                    "the flood stopped at {nr},{nc}, beside a zero"
                );
            }
        }
        // And it never crosses a number: a revealed cell either is (0,0), or
        // touches a revealed zero.
        for (r, c) in all_cells(&a) {
            if a.is_revealed(r, c) && (r, c) != (0, 0) {
                assert!(
                    a.neighbours(r, c)
                        .any(|(x, y)| a.is_revealed(x, y) && a.adjacent(x, y) == 0),
                    "{r},{c} was opened with no zero beside it"
                );
            }
        }
        a.apply(Action::NewGame);
    }

    #[test]
    fn the_flood_never_uncovers_a_mine() {
        for seed in 0..20u64 {
            let a = started(seed);
            for (r, c) in mines_of(&a) {
                assert!(!a.is_revealed(r, c), "seed {seed} flooded onto a mine");
            }
        }
    }

    #[test]
    fn a_flag_dams_the_flood() {
        // A flooded region stops at a flag as surely as at a number, because a
        // flagged cell is not `Hidden`.
        let mut a = game(28);
        a.apply(Action::Flag(0, 1));
        a.apply(Action::Reveal(0, 0));
        assert!(a.is_flagged(0, 1), "the flood took the flag");
        assert!(!a.is_revealed(0, 1), "the flood crossed a flag");
    }

    #[test]
    fn uncovering_the_last_safe_cell_wins_and_plants_the_flags_you_did_not_need() {
        let mut a = started(29);
        clear_the_board(&mut a);
        assert_eq!(a.status(), GameStatus::Won);
        for (r, c) in mines_of(&a) {
            assert!(a.is_flagged(r, c), "mine {r},{c} was left bare on a win");
        }
        assert_eq!(a.flag_count(), a.total_mines());
        assert_eq!(a.revealed_count(), a.rows() * a.cols() - a.total_mines());
    }

    #[test]
    fn a_won_game_answers_nothing_more() {
        let mut a = started(30);
        clear_the_board(&mut a);
        let (r, c) = *mines_of(&a).first().expect("a mine");
        assert_eq!(a.apply(Action::Reveal(r, c)), EventResult::Ignored);
        assert_eq!(a.apply(Action::Flag(r, c)), EventResult::Ignored);
        assert_eq!(tick(&mut a, 5_000), EventResult::Ignored);
        assert_eq!(a.status(), GameStatus::Won, "the win came undone");
    }

    #[test]
    fn a_board_is_only_cleared_when_every_safe_cell_is_open() {
        let mut a = started(31);
        assert!(!a.is_cleared(), "a board is cleared after one click");
        clear_the_board(&mut a);
        assert!(a.is_cleared());
    }

    #[test]
    fn a_flag_on_a_safe_cell_is_not_the_same_as_opening_it() {
        // "Cleared" is every safe cell *revealed*, not every safe cell merely
        // not hidden -- and a flag is the third state that tells those two
        // apart. A player who flags a safe square by mistake and opens
        // everything else has not won, and must be able to take the flag back
        // and finish.
        let mut a = started(33);
        let spare = safe_of(&a)
            .into_iter()
            .find(|&(r, c)| !a.is_revealed(r, c))
            .expect("a safe cell still shut");
        a.apply(Action::Flag(spare.0, spare.1));
        assert!(a.is_flagged(spare.0, spare.1));
        clear_the_board(&mut a);
        assert!(
            !a.is_cleared(),
            "a flag on a safe cell counted as having opened it"
        );
        assert_ne!(a.status(), GameStatus::Won, "a flag won the game");

        // Take the flag back and open it, and now it is won.
        a.apply(Action::Flag(spare.0, spare.1));
        a.apply(Action::Reveal(spare.0, spare.1));
        assert!(a.is_cleared());
        assert_eq!(a.status(), GameStatus::Won);
    }

    #[test]
    fn uncovering_every_mine_does_not_count_as_clearing_the_board() {
        // The check this replaces was `revealed_count >= cells - mines`, and the
        // losing click counted the mine it uncovered. Uncovering mines must
        // never bring a board closer to being cleared.
        let mut a = started(32);
        let before = a.is_cleared();
        for (r, c) in mines_of(&a) {
            a.apply(Action::Flag(r, c));
        }
        assert_eq!(
            a.is_cleared(),
            before,
            "flagging the mines cleared the board"
        );
        assert_ne!(a.status(), GameStatus::Won);
    }

    // ── Flagging ───────────────────────────────────────────────────────────

    #[test]
    fn a_flag_goes_on_and_comes_off_again() {
        let mut a = started(33);
        let (r, c) = *safe_of(&a)
            .iter()
            .find(|&&(r, c)| !a.is_revealed(r, c))
            .expect("a hidden cell");
        assert_eq!(a.apply(Action::Flag(r, c)), EventResult::Consumed);
        assert!(a.is_flagged(r, c));
        assert_eq!(a.flag_count(), 1);
        assert_eq!(a.apply(Action::Flag(r, c)), EventResult::Consumed);
        assert!(!a.is_flagged(r, c));
        assert_eq!(a.flag_count(), 0);
    }

    #[test]
    fn a_flag_may_be_planted_before_the_first_click() {
        // The version this replaces refused every flag while the status was
        // `Ready`, which silently ignored the very first thing a cautious
        // player does.
        let mut a = game(34);
        assert_eq!(a.apply(Action::Flag(3, 3)), EventResult::Consumed);
        assert!(a.is_flagged(3, 3));
        assert_eq!(a.status(), GameStatus::Ready, "a flag dealt the board");
        assert_eq!(a.mine_count(), 0, "a flag buried the mines");
    }

    #[test]
    fn a_flag_on_an_open_cell_is_refused() {
        let mut a = started(35);
        assert_eq!(a.apply(Action::Flag(0, 0)), EventResult::Ignored);
        assert!(!a.is_flagged(0, 0));
    }

    #[test]
    fn a_flag_off_the_board_is_refused() {
        let mut a = started(36);
        assert_eq!(a.apply(Action::Flag(99, 99)), EventResult::Ignored);
        assert_eq!(a.flag_count(), 0);
    }

    #[test]
    fn flagging_moves_the_cursor_to_what_was_flagged() {
        let mut a = started(37);
        a.apply(Action::Flag(2, 5));
        assert_eq!(a.cursor(), (2, 5));
    }

    #[test]
    fn the_counter_is_mines_less_flags_and_goes_below_nought() {
        let mut a = started(38);
        assert_eq!(a.mines_remaining(), 10);
        let hidden: Vec<(usize, usize)> = all_cells(&a)
            .into_iter()
            .filter(|&(r, c)| !a.is_revealed(r, c))
            .collect();
        for &(r, c) in hidden.iter().take(11) {
            a.apply(Action::Flag(r, c));
        }
        assert_eq!(a.flag_count(), 11);
        assert_eq!(
            a.mines_remaining(),
            -1,
            "the counter refused to admit too many flags"
        );
    }

    // ── Chording ───────────────────────────────────────────────────────────

    /// A revealed number with every one of its mines flagged and something
    /// still left to open, so a chord on it has work to do.
    fn a_satisfied_number(a: &mut MinesweeperApp) -> Option<(usize, usize)> {
        let spot = all_cells(a).into_iter().find(|&(r, c)| {
            a.is_revealed(r, c)
                && !a.is_mine(r, c)
                && a.adjacent(r, c) > 0
                && a.neighbours(r, c)
                    .any(|(x, y)| !a.is_revealed(x, y) && !a.is_mine(x, y))
        })?;
        for (r, c) in a.neighbours(spot.0, spot.1).collect::<Vec<_>>() {
            if a.is_mine(r, c) {
                a.apply(Action::Flag(r, c));
            }
        }
        Some(spot)
    }

    #[test]
    fn chording_a_satisfied_number_opens_everything_else_around_it() {
        // "Everything", and the board is chosen so that the word can be tested.
        // A number with a single shut neighbour cannot tell a chord that opens
        // all of them from one that opens the first and stops -- and neither
        // can a number whose first shut neighbour is a blank, because the flood
        // out of that blank would open the rest whatever the chord did. So this
        // wants two shut neighbours, both of them counts rather than blanks,
        // and it hunts across boards until it finds one.
        let (mut a, (r, c)) = (0..80u64)
            .find_map(|seed| {
                let mut a = started(seed);
                let shut = |a: &MinesweeperApp, r: usize, c: usize| {
                    a.neighbours(r, c)
                        .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_mine(x, y))
                        .collect::<Vec<_>>()
                };
                let spot = all_cells(&a).into_iter().find(|&(r, c)| {
                    a.is_revealed(r, c)
                        && !a.is_mine(r, c)
                        && a.adjacent(r, c) > 0
                        && shut(&a, r, c).len() >= 2
                        && shut(&a, r, c).iter().all(|&(x, y)| a.adjacent(x, y) > 0)
                })?;
                for (x, y) in a.neighbours(spot.0, spot.1).collect::<Vec<_>>() {
                    if a.is_mine(x, y) {
                        a.apply(Action::Flag(x, y));
                    }
                }
                Some((a, spot))
            })
            .expect("a satisfied number with two shut counts around it");

        let hidden: Vec<(usize, usize)> = a
            .neighbours(r, c)
            .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_flagged(x, y))
            .collect();
        assert!(
            hidden.len() >= 2,
            "one cell cannot tell 'all' from 'the first'"
        );
        assert_eq!(a.apply(Action::Chord(r, c)), EventResult::Consumed);
        for (x, y) in hidden {
            assert!(a.is_revealed(x, y), "{x},{y} stayed shut after a chord");
        }
        assert_eq!(a.cursor(), (r, c), "the chord did not take the cursor");
    }

    #[test]
    fn chording_a_number_whose_flags_are_missing_does_nothing() {
        let mut a = started(41);
        let (r, c) = (1..=8u8)
            .find_map(|n| a_number(&a, n))
            .expect("a revealed number");
        assert_eq!(
            a.apply(Action::Chord(r, c)),
            EventResult::Ignored,
            "a chord fired with no flags planted"
        );
    }

    #[test]
    fn chording_a_number_with_too_many_flags_does_nothing() {
        // The version this replaces flagged the *only* remaining hidden
        // neighbour to make the count too high, so the chord was turned away
        // by "nothing left to open" before the count was ever consulted, and a
        // `!=` weakened to `>` walked straight through it. A cell with two
        // hidden neighbours is what the test needs: flag one spare, and the
        // other is still there for a wrongly-permitted chord to open.
        let (mut a, (r, c)) = (0..60u64)
            .find_map(|seed| {
                let mut a = started(seed);
                let spot = all_cells(&a).into_iter().find(|&(r, c)| {
                    a.is_revealed(r, c)
                        && !a.is_mine(r, c)
                        && a.adjacent(r, c) > 0
                        && a.neighbours(r, c)
                            .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_mine(x, y))
                            .count()
                            >= 2
                })?;
                for (x, y) in a.neighbours(spot.0, spot.1).collect::<Vec<_>>() {
                    if a.is_mine(x, y) {
                        a.apply(Action::Flag(x, y));
                    }
                }
                Some((a, spot))
            })
            .expect("a satisfied number with two safe cells still shut");

        let spare = a
            .neighbours(r, c)
            .find(|&(x, y)| !a.is_revealed(x, y) && !a.is_flagged(x, y))
            .expect("a spare to over-flag");
        a.apply(Action::Flag(spare.0, spare.1));
        let still_shut = a
            .neighbours(r, c)
            .find(|&(x, y)| !a.is_revealed(x, y) && !a.is_flagged(x, y))
            .expect("something a wrong chord could open");

        assert!(
            usize::from(a.adjacent(r, c))
                < a.neighbours(r, c)
                    .filter(|&(x, y)| a.is_flagged(x, y))
                    .count(),
            "the flags do not outnumber the count"
        );
        assert_eq!(a.apply(Action::Chord(r, c)), EventResult::Ignored);
        assert!(
            !a.is_revealed(still_shut.0, still_shut.1),
            "an over-flagged chord opened {still_shut:?}"
        );
    }

    /// A covered cell a wrongly-permitted chord would actually open: its count
    /// is matched by flags, and one neighbour is still shut.
    ///
    /// Without this the test below picks the first hidden cell in reading
    /// order, whose count almost never equals its flag tally -- so the chord is
    /// refused by the *count* guard and the "is it even open?" guard under test
    /// is never reached.
    fn a_covered_cell_a_chord_could_open(a: &mut MinesweeperApp) -> Option<(usize, usize)> {
        let hidden = |a: &MinesweeperApp, r: usize, c: usize| {
            a.neighbours(r, c)
                .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_flagged(x, y))
                .collect::<Vec<_>>()
        };
        let spot = all_cells(a).into_iter().find(|&(r, c)| {
            !a.is_revealed(r, c)
                && !a.is_mine(r, c)
                && hidden(a, r, c).len() > usize::from(a.adjacent(r, c))
        })?;
        let shut = hidden(a, spot.0, spot.1);
        for &(x, y) in shut.iter().take(usize::from(a.adjacent(spot.0, spot.1))) {
            a.apply(Action::Flag(x, y));
        }
        Some(spot)
    }

    #[test]
    fn chording_a_covered_cell_does_nothing() {
        let mut a = started(43);
        let (r, c) = a_covered_cell_a_chord_could_open(&mut a).expect("a covered cell");
        let shut: Vec<(usize, usize)> = a
            .neighbours(r, c)
            .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_flagged(x, y))
            .collect();
        assert!(!shut.is_empty(), "nothing a wrong chord could open");
        assert_eq!(
            usize::from(a.adjacent(r, c)),
            a.neighbours(r, c)
                .filter(|&(x, y)| a.is_flagged(x, y))
                .count(),
            "the flags do not satisfy the count, so the count guard would refuse it"
        );

        assert_eq!(a.apply(Action::Chord(r, c)), EventResult::Ignored);
        assert!(
            !a.is_revealed(r, c),
            "a chord opened the cell it was aimed at"
        );
        for (x, y) in shut {
            assert!(
                !a.is_revealed(x, y),
                "a chord on a covered cell opened {x},{y}"
            );
        }
    }

    #[test]
    fn chording_a_flag_does_nothing() {
        // The other half of "not revealed": a flagged cell is not a number
        // either, however well its neighbours' flags happen to add up.
        let mut a = started(47);
        let (r, c) = a_covered_cell_a_chord_could_open(&mut a).expect("a covered cell");
        a.apply(Action::Flag(r, c));
        assert!(a.is_flagged(r, c));
        assert_eq!(a.apply(Action::Chord(r, c)), EventResult::Ignored);
    }

    #[test]
    fn chording_after_the_game_is_over_does_nothing() {
        // A lost board is covered in revealed mines and revealed numbers, so
        // this is the case the first guard in `chord` exists for -- unlike
        // `Ready`, where there is no revealed cell for a chord to land on.
        let mut a = started(48);
        let (r, c) = a_satisfied_number(&mut a).expect("a number");
        let mine = *mines_of(&a).first().expect("a mine");
        a.apply(Action::Reveal(mine.0, mine.1));
        assert_eq!(a.status(), GameStatus::Lost);

        let before = a.revealed_count();
        assert_eq!(a.apply(Action::Chord(r, c)), EventResult::Ignored);
        assert_eq!(
            a.revealed_count(),
            before,
            "a chord ran after the game ended"
        );
    }

    #[test]
    fn chording_a_number_with_nothing_left_around_it_does_nothing() {
        let mut a = started(44);
        let (r, c) = a_satisfied_number(&mut a).expect("a number");
        a.apply(Action::Chord(r, c));
        // Everything around it is open or flagged now, so a second chord has
        // nothing to do and must say so rather than claiming to have acted.
        assert_eq!(a.apply(Action::Chord(r, c)), EventResult::Ignored);
    }

    #[test]
    fn chording_before_the_first_click_does_nothing() {
        let mut a = game(45);
        assert_eq!(a.apply(Action::Chord(0, 0)), EventResult::Ignored);
        assert_eq!(a.status(), GameStatus::Ready);
        assert_eq!(a.mine_count(), 0, "a chord dealt the board");
    }

    #[test]
    fn chording_on_a_wrong_flag_loses_the_game() {
        // The gesture that loses fastest: the count is satisfied by a flag in
        // the wrong place, so a mine is among what the chord opens.
        let mut a = started(46);
        let (r, c) = (1..=8u8)
            .find_map(|n| a_number(&a, n))
            .expect("a revealed number");
        let wrong: Vec<(usize, usize)> = a
            .neighbours(r, c)
            .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_mine(x, y))
            .collect();
        let mines_here = a.neighbours(r, c).filter(|&(x, y)| a.is_mine(x, y)).count();
        if wrong.len() >= mines_here && mines_here > 0 {
            for &(x, y) in wrong.iter().take(mines_here) {
                a.apply(Action::Flag(x, y));
            }
            assert_eq!(a.apply(Action::Chord(r, c)), EventResult::Consumed);
            assert_eq!(a.status(), GameStatus::Lost, "a wrong chord survived");
        }
    }

    #[test]
    fn a_chord_that_hits_a_mine_stops_rather_than_opening_the_rest() {
        // `chord` breaks out of its loop the moment the status leaves
        // `Playing`. Without that, a losing chord would go on calling `reveal`
        // on a finished game.
        let mut a = started(47);
        let (r, c) = (1..=8u8)
            .find_map(|n| a_number(&a, n))
            .expect("a revealed number");
        let around: Vec<(usize, usize)> = a
            .neighbours(r, c)
            .filter(|&(x, y)| !a.is_revealed(x, y))
            .collect();
        let mines_here = around.iter().filter(|&&(x, y)| a.is_mine(x, y)).count();
        let safe_here: Vec<(usize, usize)> = around
            .iter()
            .copied()
            .filter(|&(x, y)| !a.is_mine(x, y))
            .collect();
        if mines_here > 0 && safe_here.len() >= mines_here {
            for &(x, y) in safe_here.iter().take(mines_here) {
                a.apply(Action::Flag(x, y));
            }
            a.apply(Action::Chord(r, c));
            assert_eq!(a.status(), GameStatus::Lost);
            // Every mine is showing because the loss revealed them, but the
            // safe cells the chord had not reached yet must still be shut.
            assert!(a.is_over());
        }
    }

    // ── The cursor ─────────────────────────────────────────────────────────

    #[test]
    fn the_arrow_keys_walk_the_cursor_one_cell_at_a_time() {
        let mut a = game(50);
        assert_eq!(key(&mut a, Key::Right), EventResult::Consumed);
        assert_eq!(a.cursor(), (0, 1));
        key(&mut a, Key::Down);
        assert_eq!(a.cursor(), (1, 1));
        key(&mut a, Key::Left);
        assert_eq!(a.cursor(), (1, 0));
        key(&mut a, Key::Up);
        assert_eq!(a.cursor(), (0, 0));
    }

    #[test]
    fn the_cursor_stops_at_the_edges_rather_than_wrapping() {
        let mut a = game(51);
        assert_eq!(key(&mut a, Key::Up), EventResult::Ignored);
        assert_eq!(key(&mut a, Key::Left), EventResult::Ignored);
        assert_eq!(a.cursor(), (0, 0), "the cursor wrapped off the top left");

        for _ in 0..a.cols() * 2 {
            key(&mut a, Key::Right);
        }
        for _ in 0..a.rows() * 2 {
            key(&mut a, Key::Down);
        }
        assert_eq!(a.cursor(), (a.rows() - 1, a.cols() - 1));
        assert_eq!(key(&mut a, Key::Right), EventResult::Ignored);
        assert_eq!(key(&mut a, Key::Down), EventResult::Ignored);
    }

    #[test]
    fn the_keyboard_plays_the_whole_game_without_a_mouse() {
        let mut a = game(52);
        key(&mut a, Key::Right);
        key(&mut a, Key::Down);
        assert_eq!(key(&mut a, Key::Space), EventResult::Consumed);
        assert_eq!(a.status(), GameStatus::Playing, "Space did not uncover");
        assert!(a.is_revealed(1, 1));

        // F flags where the cursor is.
        let (r, c) = *all_cells(&a)
            .iter()
            .find(|&&(r, c)| !a.is_revealed(r, c))
            .expect("a hidden cell");
        walk_cursor_with_keys(&mut a, r, c);
        assert_eq!(key(&mut a, Key::F), EventResult::Consumed);
        assert!(a.is_flagged(r, c), "F did not flag under the cursor");
    }

    #[test]
    fn enter_uncovers_the_same_cell_space_would() {
        // Two keys, one action. Named separately because a suite that only ever
        // presses Space cannot notice Enter being dropped from the arm they
        // share.
        let mut a = game(521);
        key(&mut a, Key::Down);
        assert_eq!(key(&mut a, Key::Enter), EventResult::Consumed);
        assert!(a.is_revealed(1, 0), "Enter did not uncover");
    }

    #[test]
    fn the_letter_keys_do_what_the_footer_says_they_do() {
        let mut a = started(53);
        let before = a.seed();
        assert_eq!(key(&mut a, Key::N), EventResult::Consumed);
        assert_ne!(a.seed(), before, "N did not deal a new board");

        assert_eq!(key(&mut a, Key::D), EventResult::Consumed);
        assert_eq!(a.difficulty(), Difficulty::Intermediate, "D did not cycle");

        let before = a.seed();
        assert_eq!(key(&mut a, Key::F2), EventResult::Consumed);
        assert_ne!(a.seed(), before, "F2 did not deal a new board");
    }

    #[test]
    fn c_chords_where_the_cursor_is() {
        let mut a = started(54);
        let (r, c) = a_satisfied_number(&mut a).expect("a number");
        // Flagging left the cursor on the last flag; walk it back to the number.
        a.apply(Action::Move(Dir::Up));
        walk_cursor_to(&mut a, r, c);
        let hidden = a
            .neighbours(r, c)
            .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_flagged(x, y))
            .count();
        assert!(hidden > 0, "nothing to chord");
        assert_eq!(key(&mut a, Key::C), EventResult::Consumed);
    }

    #[test]
    fn ctrl_and_a_digit_jumps_straight_to_a_difficulty() {
        let mut a = started(55);
        assert_eq!(
            handle_event(&mut a, &Event::Key(ctrl(Key::Num3))),
            EventResult::Consumed
        );
        assert_eq!(a.difficulty(), Difficulty::Expert);
        handle_event(&mut a, &Event::Key(ctrl(Key::Num1)));
        assert_eq!(a.difficulty(), Difficulty::Beginner);
        handle_event(&mut a, &Event::Key(ctrl(Key::Num2)));
        assert_eq!(a.difficulty(), Difficulty::Intermediate);
    }

    #[test]
    fn ctrl_with_anything_else_is_left_for_the_window_to_deal_with() {
        let mut a = started(56);
        let before = a.cursor();
        assert_eq!(
            handle_event(&mut a, &Event::Key(ctrl(Key::Right))),
            EventResult::Ignored,
            "ctrl-right moved the cursor"
        );
        assert_eq!(a.cursor(), before);
        assert_eq!(
            handle_event(&mut a, &Event::Key(ctrl(Key::N))),
            EventResult::Ignored,
            "ctrl-N dealt a board"
        );
    }

    #[test]
    fn a_key_held_with_alt_or_the_super_key_belongs_to_the_desktop() {
        for m in [
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ] {
            let mut a = started(57);
            let before = a.cursor();
            assert_eq!(
                handle_event(&mut a, &Event::Key(press_with(Key::Right, m))),
                EventResult::Ignored,
                "{m:?} still moved the cursor"
            );
            assert_eq!(a.cursor(), before);
        }
    }

    #[test]
    fn letting_a_key_go_is_not_pressing_it() {
        let mut a = started(58);
        let mut up = press(Key::Right);
        up.pressed = false;
        let before = a.cursor();
        assert_eq!(handle_event(&mut a, &Event::Key(up)), EventResult::Ignored);
        assert_eq!(a.cursor(), before);
    }

    #[test]
    fn a_key_the_board_does_not_answer_is_ignored() {
        let mut a = started(59);
        for k in [Key::Escape, Key::Tab, Key::Home, Key::Z, Key::F5] {
            assert_eq!(key(&mut a, k), EventResult::Ignored, "{k:?} did something");
        }
    }

    // ── The clock ──────────────────────────────────────────────────────────

    #[test]
    fn the_clock_does_not_run_before_the_first_click() {
        let mut a = game(60);
        assert_eq!(tick(&mut a, 3_000), EventResult::Ignored);
        assert_eq!(a.elapsed_ms(), 0, "the clock ran on a board not yet dealt");
    }

    #[test]
    fn the_clock_counts_the_time_that_passed_not_the_times_it_was_woken() {
        // It used to add one second per call, which reports how often the
        // window happened to wake it — a fact about the window, not the game.
        //
        // The three amounts must not sum to a whole number of seconds, and it
        // took the mutation sweep to notice that the first three chosen here
        // did: 250 + 250 + 2500 is 3000, and so is one second per call three
        // times over, so the right rule and the wrong one gave the same answer
        // and this test could not tell them apart (`known-issues.md` lesson
        // 54). Four ticks summing to 2825ms disagree with 4000ms in the
        // milliseconds and, once floored, in the seconds too.
        let mut a = started(61);
        tick(&mut a, 250);
        tick(&mut a, 75);
        tick(&mut a, 2_000);
        tick(&mut a, 500);
        assert_eq!(a.elapsed_ms(), 2_825, "the clock counted its wakings");
        assert_eq!(a.elapsed_secs(), 2, "the displayed second is not floored");
    }

    #[test]
    fn a_tick_that_does_not_move_the_displayed_second_asks_for_no_repaint() {
        let mut a = started(62);
        assert_eq!(tick(&mut a, 400), EventResult::Ignored, "400ms repainted");
        assert_eq!(a.elapsed_ms(), 400, "an ignored tick lost the time");
        assert_eq!(
            tick(&mut a, 600),
            EventResult::Consumed,
            "1s did not repaint"
        );
        assert_eq!(a.elapsed_secs(), 1);
    }

    #[test]
    fn the_clock_stops_when_the_game_does() {
        let mut a = started(63);
        tick(&mut a, 4_000);
        let (r, c) = *mines_of(&a).first().expect("a mine");
        a.apply(Action::Reveal(r, c));
        assert_eq!(tick(&mut a, 9_000), EventResult::Ignored);
        assert_eq!(a.elapsed_secs(), 4, "the clock ran on after the boom");
    }

    #[test]
    fn the_window_is_only_woken_while_there_is_a_clock_to_move() {
        let mut a = game(64);
        assert_eq!(
            a.tick_interval(),
            None,
            "a board not yet dealt asks for ticks"
        );
        reveal(&mut a, 0, 0);
        assert_eq!(a.tick_interval(), Some(Duration::from_millis(CLOCK_MS)));
        let (r, c) = *mines_of(&a).first().expect("a mine");
        a.apply(Action::Reveal(r, c));
        assert_eq!(a.tick_interval(), None, "a finished game asks for ticks");
    }

    #[test]
    fn the_clock_reads_minutes_and_seconds_and_keeps_counting_past_the_hour() {
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(9), "00:09");
        assert_eq!(format_time(60), "01:00");
        assert_eq!(format_time(61), "01:01");
        assert_eq!(format_time(599), "09:59");
        assert_eq!(format_time(3_600), "60:00", "an hour wrapped to nothing");
        assert_eq!(format_time(7_265), "121:05");
    }

    // ── The window ─────────────────────────────────────────────────────────

    #[test]
    fn the_window_remembers_the_size_it_was_last_drawn_at() {
        let mut a = game(70);
        assert_eq!(a.size(), SIZE, "a fresh app does not know its own window");
        handle_event(
            &mut a,
            &Event::Resize {
                width: 500,
                height: 400,
            },
        );
        assert_eq!(a.size(), (500.0, 400.0));
        a.render(640.0, 480.0);
        assert_eq!(a.size(), (640.0, 480.0), "drawing did not update the size");
    }

    #[test]
    fn a_click_is_read_against_the_window_the_player_is_looking_at() {
        // The same point in two windows is two different cells. This is the
        // whole reason the size is stored: a click arrives with no size on it.
        let mut small = game(71);
        small.render(400.0, 300.0);
        let big_point = cell_point(&game(71), 8, 8);

        let mut big = game(71);
        big.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        mouse(
            &mut big,
            big_point.0,
            big_point.1,
            MouseEventKind::Press(MouseButton::Left),
        );
        assert_eq!(big.cursor(), (8, 8));

        mouse(
            &mut small,
            big_point.0,
            big_point.1,
            MouseEventKind::Press(MouseButton::Left),
        );
        assert_ne!(
            small.cursor(),
            (8, 8),
            "a small window read a big window's click"
        );
    }

    #[test]
    fn a_window_that_shrinks_gives_up_the_footer_before_the_header() {
        // The header carries the mine counter and the clock, which are part of
        // playing; the footer is a reminder of the keys.
        let tall = Layout::new(600.0, 600.0, 9, 9);
        assert!(tall.shows(tall.header) && tall.shows(tall.footer));

        let short = Layout::new(600.0, 90.0, 9, 9);
        assert!(!short.shows(short.footer), "the footer outlived the header");

        let tiny = Layout::new(600.0, 20.0, 9, 9);
        assert!(!tiny.shows(tiny.header) && !tiny.shows(tiny.footer));
    }

    #[test]
    fn a_dropped_band_is_gone_rather_than_a_strip_of_no_height() {
        let tiny = Layout::new(600.0, 20.0, 9, 9);
        assert_eq!(tiny.header, Rect::EMPTY, "the header is merely thin");
        assert_eq!(tiny.footer, Rect::EMPTY, "the footer is merely thin");
    }

    #[test]
    fn the_board_keeps_its_share_of_a_window_that_is_getting_shorter() {
        // Two things had to change here before this test could see the share
        // move, and the second was not the one it looked like.
        //
        // The number is written out rather than read from `BOARD_SHARE`,
        // because a test that measures the share against the constant that
        // sets it moves with that constant (`known-issues.md` lesson 52). Half
        // is the *claim*: whatever the constant says, at least half of any
        // window this program is playable in must reach the board.
        //
        // But that alone still did not fail when the sweep dropped the share
        // to 0.15, and the reason is worth writing down. `BOARD_SHARE` does
        // not set the board's height -- the board gets whatever the header and
        // footer leave. All the share decides is the *budget* those two bands
        // must fit inside before one of them is dropped, so it has no effect
        // whatever on a window tall enough for both. Every height this test
        // used was such a window, so the two versions laid out identically and
        // the assertion was exact, thorough and blind. The heights that matter
        // are the short ones, 60 to 90 pixels here, where the real share drops
        // a band and the mutant keeps it: at 60 the board falls from 56 pixels
        // to 14. **A constant that only takes effect at the edge of a range
        // can only be tested at that edge** -- and a fixture chosen from
        // comfortable, realistic window sizes is guaranteed to miss it.
        for h in [60.0f32, 75.0, 90.0, 200.0, 300.0, 480.0, 660.0, 1080.0] {
            let l = Layout::new(800.0, h, 9, 9);
            assert!(
                l.board.h >= h * 0.5 - 0.01,
                "at {h} the board kept only {}",
                l.board.h
            );
        }
    }

    #[test]
    fn no_cell_is_drawn_on_top_of_the_header_or_the_footer() {
        // Staying inside the window is not the same claim as staying inside
        // the *board's band*: a board slid up over the header is still wholly
        // within the window, so `no_cell_is_ever_drawn_outside_the_window`
        // cannot see it. The sweep did exactly that and only the two chip
        // tests noticed -- and they noticed because the board had buried the
        // chips' hit boxes, which is a symptom, not the rule being broken.
        for (w, h) in SIZES {
            for d in Difficulty::ALL {
                let l = Layout::new(w, h, d.rows(), d.cols());
                for (r, c) in [(0, 0), (d.rows() - 1, d.cols() - 1)] {
                    let rect = l.cell_rect(r, c);
                    if rect.is_empty() {
                        continue;
                    }
                    if l.shows(l.header) {
                        assert!(
                            rect.y >= l.header.bottom() - 0.01,
                            "{}x{} {}: cell {r},{c} at {rect:?} is drawn over the header",
                            w,
                            h,
                            d.label()
                        );
                    }
                    if l.shows(l.footer) {
                        assert!(
                            rect.bottom() <= l.footer.y + 0.01,
                            "{}x{} {}: cell {r},{c} at {rect:?} is drawn over the footer",
                            w,
                            h,
                            d.label()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_cell_is_ever_drawn_outside_the_window() {
        for (w, h) in SIZES {
            for d in Difficulty::ALL {
                let l = Layout::new(w, h, d.rows(), d.cols());
                for (r, c) in [(0, 0), (0, d.cols() - 1), (d.rows() - 1, d.cols() - 1)] {
                    let rect = l.cell_rect(r, c);
                    if rect.is_empty() {
                        continue;
                    }
                    assert!(
                        rect.x >= -0.01
                            && rect.y >= -0.01
                            && rect.right() <= w + 0.01
                            && rect.bottom() <= h + 0.01,
                        "{}x{} {}: cell {r},{c} is drawn at {rect:?}",
                        w,
                        h,
                        d.label()
                    );
                }
            }
        }
    }

    #[test]
    fn cells_do_not_sit_on_top_of_one_another() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 9, 9);
            let a = l.cell_rect(0, 0);
            if a.is_empty() {
                // No board at all in this window, which is the honest answer
                // when a cell would be under a pixel. Nothing to overlap.
                assert_eq!(l.cell_rect(8, 8), Rect::EMPTY, "{w}x{h}: half a board");
                continue;
            }
            let b = l.cell_rect(0, 1);
            let c = l.cell_rect(1, 0);
            assert!(a.right() <= b.x + 0.01, "{w}x{h}: columns overlap");
            assert!(a.bottom() <= c.y + 0.01, "{w}x{h}: rows overlap");
            assert!(l.cell >= 1.0, "{w}x{h}: a cell shrank to nothing");
            assert!(
                l.step >= l.cell,
                "{w}x{h}: the gap was added, not taken out"
            );
        }
    }

    #[test]
    fn there_is_a_gap_between_one_cell_and_the_next() {
        // Without it the board is one solid block of colour with no cells
        // visible in it -- and a suite that only checks that cells do not
        // *overlap* is satisfied by exactly that, because touching is not
        // overlapping.
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 9, 9);
            if l.cell_rect(0, 0).is_empty() {
                continue;
            }
            assert!(
                l.cell < l.step,
                "{w}x{h}: a cell of {} in a step of {} leaves no gap",
                l.cell,
                l.step
            );
            assert!(
                l.cell_rect(0, 0).right() < l.cell_rect(0, 1).x,
                "{w}x{h}: the cells touch"
            );
        }
    }

    #[test]
    fn a_window_with_no_room_for_a_board_draws_no_board_at_all() {
        // Not a board painted outside the window, and not eight hundred hit
        // boxes stacked at one point. See `Layout::new`.
        let l = Layout::new(1.0, 1.0, 9, 9);
        assert!(l.cell <= 0.0, "a cell of {} in a 1x1 window", l.cell);
        assert!(l.step <= 0.0, "a step of {} in a 1x1 window", l.step);
        assert!(l.grid.is_empty(), "an undrawable board still has a grid");
        for (r, c) in [(0, 0), (4, 4), (8, 8)] {
            assert_eq!(l.cell_rect(r, c), Rect::EMPTY, "cell {r},{c} was drawn");
        }
        let a = game(112);
        assert_eq!(
            probe::rect_of_sized(&a, Target::Cell(0, 0), (1.0, 1.0)),
            None,
            "a cell recorded a hit box in a window with no board"
        );
    }

    #[test]
    fn the_board_is_centred_in_the_space_it_is_given() {
        let l = Layout::new(1200.0, 700.0, 9, 9);
        let left = l.grid.x - l.board.x;
        let right = l.board.right() - l.grid.right();
        assert!(
            (left - right).abs() < 0.01,
            "the board sits {left} / {right}"
        );
        let top = l.grid.y - l.board.y;
        let bottom = l.board.bottom() - l.grid.bottom();
        assert!(
            (top - bottom).abs() < 0.01,
            "the board sits {top} / {bottom}"
        );
    }

    #[test]
    fn a_margin_never_grows_wider_than_the_thing_it_indents() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 9, 9);
            assert!(
                l.pad <= w / 4.0 + 0.01 && l.pad <= h / 4.0 + 0.01,
                "{w}x{h}: a margin of {} on a window that size",
                l.pad
            );
            assert!(l.board.x >= 0.0 && l.board.right() <= w + 0.01);
        }
    }

    #[test]
    fn the_two_chips_sit_side_by_side_inside_the_header() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT, 9, 9);
        let new = l.chip(0);
        let level = l.chip(1);
        assert!(!new.is_empty() && !level.is_empty());
        assert!(level.right() <= new.x + 0.01, "the chips overlap");
        assert!(new.right() <= l.header.right(), "a chip left the header");
        assert!(level.x >= l.header.x, "a chip left the header");
        assert!(
            new.y >= l.header.y && new.bottom() <= l.header.bottom() + 0.01,
            "a chip left the header vertically"
        );
    }

    #[test]
    fn a_chip_stops_growing_once_it_is_wide_enough_to_read() {
        // Every assertion in the test above is true of *any* chip width: the
        // chips are placed by subtracting their own width from the right edge,
        // so they cannot overlap or leave the header however wide they get,
        // and a test built out of the layout it is testing cannot fail
        // (`known-issues.md` lesson 52). What a chip's upper clamp actually
        // promises is this: past a certain window, a wider window does not buy
        // a wider chip. A label that keeps pace with the monitor is a label
        // that eventually eats the header the counter and clock live in.
        let wide = Layout::new(1920.0, 1080.0, 9, 9);
        let wider = Layout::new(3840.0, 2160.0, 9, 9);
        assert!(
            (wide.chip(0).w - wider.chip(0).w).abs() < 0.01,
            "doubling the window doubled the chip: {} then {}",
            wide.chip(0).w,
            wider.chip(0).w
        );
        // And having stopped, the pair leaves the header overwhelmingly to the
        // readout it exists for.
        let pair = wider.chip(0).w + wider.chip(1).w;
        assert!(
            pair < wider.header.w / 4.0,
            "the two chips take {pair} of a {} header",
            wider.header.w
        );
    }

    #[test]
    fn there_are_no_chips_when_there_is_no_header() {
        let l = Layout::new(600.0, 20.0, 9, 9);
        assert_eq!(l.chip(0), Rect::EMPTY);
        assert_eq!(l.chip(1), Rect::EMPTY);
    }

    #[test]
    fn every_frame_is_balanced_at_every_size() {
        for (w, h) in SIZES {
            for d in Difficulty::ALL {
                let a = MinesweeperApp::with_seed(d, 77);
                assert!(
                    a.frame(w, h).is_balanced(),
                    "{w}x{h} {} left a clip open",
                    d.label()
                );
            }
        }
    }

    #[test]
    fn a_window_of_no_size_still_draws_something_rather_than_nothing() {
        let a = game(78);
        let f = a.frame(0.0, 0.0);
        assert!(!f.commands().is_empty(), "a degenerate window drew nothing");
        assert!(f.is_balanced());
    }

    // ── Hit boxes ──────────────────────────────────────────────────────────

    #[test]
    fn every_cell_of_every_board_records_a_box_a_click_can_find() {
        for d in Difficulty::ALL {
            let a = MinesweeperApp::with_seed(d, 80);
            for (r, c) in all_cells(&a) {
                let rect = probe::rect_of(&a, Target::Cell(r, c));
                assert!(
                    rect.is_some_and(|x| !x.is_empty()),
                    "{} cell {r},{c} is not clickable",
                    d.label()
                );
            }
        }
    }

    #[test]
    fn the_box_a_cell_records_is_the_box_it_was_painted_in() {
        // This test claims the hit box and the drawn box *agree*, and both
        // sides of that claim come from `cell_rect` -- so it cannot see
        // `cell_rect` itself go wrong, and the mutation sweep proved it blind
        // to a column read as a row (`known-issues.md` lesson 52). That is not
        // a fault in this test: agreement is a real property and this is where
        // it belongs. Where the grid *puts* a cell is a separate claim, and it
        // is owned by the test below.
        let a = game(81);
        let l = layout_of(&a);
        for (r, c) in all_cells(&a) {
            assert_eq!(
                probe::rect_of(&a, Target::Cell(r, c)),
                Some(l.cell_rect(r, c)),
                "cell {r},{c} answers a click somewhere it is not drawn"
            );
        }
    }

    #[test]
    fn a_cells_box_moves_across_with_its_column_and_down_with_its_row() {
        let a = game(81);
        let l = layout_of(&a);
        for (r, c) in all_cells(&a) {
            let here = l.cell_rect(r, c);
            if c + 1 < a.cols() {
                let right = l.cell_rect(r, c + 1);
                assert!(
                    right.x > here.x,
                    "column {} is not to the right of column {c}",
                    c + 1
                );
                assert!(
                    (right.y - here.y).abs() < 0.01,
                    "moving along a row moved the cell down the window"
                );
            }
            if r + 1 < a.rows() {
                let below = l.cell_rect(r + 1, c);
                assert!(below.y > here.y, "row {} is not below row {r}", r + 1);
                assert!(
                    (below.x - here.x).abs() < 0.01,
                    "moving down a column moved the cell across the window"
                );
            }
        }
    }

    #[test]
    fn clicking_a_cell_uncovers_that_cell_and_no_other() {
        // Four cells, not one. A click's `x` and `y` are two numbers of the
        // same kind, so reading them the wrong way round reflects the point
        // across a diagonal -- and a cell that happens to straddle that
        // diagonal is mapped onto itself, which leaves a one-cell fixture
        // passing while every click in the program lands in the wrong place
        // (`known-issues.md` lesson 54). Cell 5,3 is exactly such a cell here,
        // and the mutation sweep caught this test being blind to a
        // `hit_test(y, x)` for that reason. One reflection cannot fix four
        // cells spread across the board, so this fixture now sees it.
        for (r, c) in [(5_usize, 3_usize), (0, 8), (8, 0), (2, 6)] {
            let mut a = game(82);
            assert_eq!(
                reveal(&mut a, r, c),
                EventResult::Consumed,
                "the click on {r},{c} was refused"
            );
            assert!(a.is_revealed(r, c), "cell {r},{c} stayed covered");
            assert_eq!(a.cursor(), (r, c), "the click on {r},{c} landed elsewhere");
        }
    }

    #[test]
    fn the_right_button_flags_and_the_left_uncovers() {
        let mut a = started(83);
        let (r, c) = *all_cells(&a)
            .iter()
            .find(|&&(r, c)| !a.is_revealed(r, c))
            .expect("a hidden cell");
        assert_eq!(flag(&mut a, r, c), EventResult::Consumed);
        assert!(a.is_flagged(r, c), "the right button did not flag");
        assert!(!a.is_revealed(r, c), "the right button uncovered");
    }

    #[test]
    fn the_middle_button_and_a_double_click_both_chord() {
        for kind in [
            MouseEventKind::Press(MouseButton::Middle),
            MouseEventKind::DoubleClick(MouseButton::Left),
        ] {
            let mut a = started(84);
            let (r, c) = a_satisfied_number(&mut a).expect("a number");
            let hidden: Vec<(usize, usize)> = a
                .neighbours(r, c)
                .filter(|&(x, y)| !a.is_revealed(x, y) && !a.is_flagged(x, y))
                .collect();
            assert!(!hidden.is_empty());
            let (x, y) = cell_point(&a, r, c);
            assert_eq!(mouse(&mut a, x, y, kind.clone()), EventResult::Consumed);
            for (hx, hy) in hidden {
                assert!(a.is_revealed(hx, hy), "{kind:?} did not chord");
            }
        }
    }

    #[test]
    fn clicking_the_level_chip_moves_to_the_next_level() {
        let mut a = started(85);
        assert_eq!(
            probe::click(&mut a, Target::Difficulty),
            EventResult::Consumed
        );
        assert_eq!(a.difficulty(), Difficulty::Intermediate);
        probe::click(&mut a, Target::Difficulty);
        assert_eq!(a.difficulty(), Difficulty::Expert);
    }

    #[test]
    fn clicking_the_new_chip_deals_a_board_at_the_same_level() {
        let mut a = started(86);
        a.apply(Action::CycleDifficulty);
        let before = a.seed();
        assert_eq!(probe::click(&mut a, Target::NewGame), EventResult::Consumed);
        assert_ne!(a.seed(), before, "the New chip dealt the same board");
        assert_eq!(
            a.difficulty(),
            Difficulty::Intermediate,
            "New changed level"
        );
        assert_eq!(a.status(), GameStatus::Ready);
    }

    #[test]
    fn the_chips_answer_only_the_left_button() {
        let mut a = started(87);
        let before = a.difficulty();
        assert_eq!(
            probe::click_with(&mut a, Target::Difficulty, MouseButton::Right),
            EventResult::Ignored
        );
        assert_eq!(a.difficulty(), before);
    }

    #[test]
    fn clicking_where_there_is_no_control_does_nothing() {
        let mut a = started(88);
        let before = (a.difficulty(), a.seed(), a.revealed_count());
        assert_eq!(probe::click_background(&mut a), EventResult::Ignored);
        assert_eq!((a.difficulty(), a.seed(), a.revealed_count()), before);
    }

    #[test]
    fn moving_the_pointer_and_letting_go_of_a_button_are_not_clicks() {
        let mut a = started(89);
        let (x, y) = cell_point(&a, 4, 4);
        let before = a.revealed_count();
        for kind in [
            MouseEventKind::Move,
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Enter,
            MouseEventKind::Leave,
            MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        ] {
            assert_eq!(
                mouse(&mut a, x, y, kind.clone()),
                EventResult::Ignored,
                "{kind:?} acted"
            );
        }
        assert_eq!(a.revealed_count(), before);
    }

    #[test]
    fn every_control_the_program_has_is_drawn_where_a_player_can_reach_it() {
        let a = game(90);
        let names = probe::control_names(&a);
        for want in ["Cell", "Difficulty", "NewGame"] {
            assert!(
                names.iter().any(|n| n == want),
                "{want} is never drawn; names were {names:?}"
            );
        }
    }

    #[test]
    fn a_board_too_big_for_its_window_is_still_wholly_clickable() {
        // Expert on a small window: every cell must still have its own box, or
        // some part of the board is unplayable.
        let a = MinesweeperApp::with_seed(Difficulty::Expert, 91);
        let size = (320.0, 240.0);
        let seen: HashSet<(usize, usize)> = all_cells(&a)
            .into_iter()
            .filter(|&(r, c)| {
                probe::rect_of_sized(&a, Target::Cell(r, c), size).is_some_and(|x| !x.is_empty())
            })
            .collect();
        assert_eq!(
            seen.len(),
            a.rows() * a.cols(),
            "cells vanished at {size:?}"
        );
    }

    // ── What the window shows ──────────────────────────────────────────────

    #[test]
    fn the_header_shows_the_counter_the_clock_the_state_and_the_level() {
        let mut a = started(100);
        tick(&mut a, 65_000);
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let all = texts(&f).join(" | ");
        assert!(all.contains("Mines 10"), "no mine counter in {all}");
        assert!(all.contains("01:05"), "no clock in {all}");
        assert!(all.contains("Playing"), "no state in {all}");
        assert!(all.contains("Beginner"), "no level in {all}");
        assert!(all.contains("New"), "no new-game chip in {all}");
    }

    #[test]
    fn the_counter_falls_as_flags_are_planted() {
        let mut a = started(101);
        let hidden: Vec<(usize, usize)> = all_cells(&a)
            .into_iter()
            .filter(|&(r, c)| !a.is_revealed(r, c))
            .collect();
        for &(r, c) in hidden.iter().take(3) {
            a.apply(Action::Flag(r, c));
        }
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            texts(&f).iter().any(|t| t.contains("Mines 7")),
            "the counter did not follow the flags: {:?}",
            texts(&f)
        );
    }

    #[test]
    fn every_state_of_the_game_says_its_own_word_in_its_own_colour() {
        let states = [
            GameStatus::Ready,
            GameStatus::Playing,
            GameStatus::Lost,
            GameStatus::Won,
        ];
        let words: HashSet<&str> = states.iter().map(|&s| status_text(s)).collect();
        assert_eq!(words.len(), states.len(), "two states share a word");
        let colors: HashSet<Color> = states.iter().map(|&s| status_color(s)).collect();
        assert_eq!(colors.len(), states.len(), "two states share a colour");
    }

    #[test]
    fn the_word_in_the_header_follows_the_game() {
        let mut a = game(102);
        assert!(
            texts(&a.frame(WINDOW_WIDTH, WINDOW_HEIGHT))
                .iter()
                .any(|t| t.contains("Ready"))
        );
        reveal(&mut a, 0, 0);
        assert!(
            texts(&a.frame(WINDOW_WIDTH, WINDOW_HEIGHT))
                .iter()
                .any(|t| t.contains("Playing"))
        );
        let (r, c) = *mines_of(&a).first().expect("a mine");
        a.apply(Action::Reveal(r, c));
        assert!(
            texts(&a.frame(WINDOW_WIDTH, WINDOW_HEIGHT))
                .iter()
                .any(|t| t.contains("Boom"))
        );
    }

    #[test]
    fn a_flagged_cell_carries_a_flag_and_an_uncovered_mine_carries_a_star() {
        let mut a = started(103);
        let (r, c) = *mines_of(&a).first().expect("a mine");
        a.apply(Action::Flag(r, c));
        assert!(
            texts(&a.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).contains(&"F".to_string()),
            "a flagged cell is blank"
        );

        let mut b = started(103);
        let (br, bc) = *mines_of(&b).get(1).expect("a second mine");
        b.apply(Action::Reveal(br, bc));
        let stars = texts(&b.frame(WINDOW_WIDTH, WINDOW_HEIGHT))
            .iter()
            .filter(|t| *t == "*")
            .count();
        assert_eq!(stars, b.total_mines(), "not every mine is showing");
    }

    #[test]
    fn the_mine_that_ended_the_game_is_the_one_painted_red() {
        let mut a = started(104);
        let mines = mines_of(&a);
        let (r, c) = *mines.first().expect("a mine");
        a.apply(Action::Reveal(r, c));
        let l = layout_of(&a);
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            fill_color_at(&f, l.cell_rect(r, c)),
            Some(RED),
            "the losing cell is not marked"
        );
        if let Some(&(orr, oc)) = mines.get(1) {
            assert_eq!(
                fill_color_at(&f, l.cell_rect(orr, oc)),
                Some(SURFACE0),
                "a mine that was merely shown is marked as the loss"
            );
        }
    }

    #[test]
    fn a_covered_cell_a_flagged_one_and_an_open_one_are_three_different_faces() {
        let mut a = started(105);
        let (fr, fc) = *all_cells(&a)
            .iter()
            .find(|&&(r, c)| !a.is_revealed(r, c))
            .expect("a hidden cell");
        a.apply(Action::Flag(fr, fc));
        let l = layout_of(&a);
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let hidden = all_cells(&a)
            .into_iter()
            .find(|&(r, c)| !a.is_revealed(r, c) && !a.is_flagged(r, c))
            .expect("another hidden cell");
        let faces = [
            fill_color_at(&f, l.cell_rect(hidden.0, hidden.1)),
            fill_color_at(&f, l.cell_rect(fr, fc)),
            fill_color_at(&f, l.cell_rect(0, 0)),
        ];
        let set: HashSet<Option<Color>> = faces.iter().copied().collect();
        assert_eq!(set.len(), 3, "two cell states look the same: {faces:?}");
    }

    #[test]
    fn each_neighbour_count_is_written_in_its_own_colour() {
        assert_eq!(NUMBER_COLORS.len(), 8, "there are eight possible counts");
        let set: HashSet<Color> = NUMBER_COLORS.iter().copied().collect();
        assert_eq!(set.len(), 8, "two counts share a colour");
    }

    #[test]
    fn an_open_number_is_drawn_in_the_colour_that_count_is_given() {
        let mut a = started(106);
        for n in 1..=8u8 {
            if let Some((r, c)) = a_number(&a, n) {
                let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
                let want = NUMBER_COLORS.get(usize::from(n) - 1).copied();
                assert_eq!(
                    text_color(&f, &n.to_string()),
                    want,
                    "the {n} at {r},{c} is the wrong colour"
                );
            }
        }
        a.apply(Action::NewGame);
    }

    #[test]
    fn an_open_cell_that_touches_nothing_is_left_blank() {
        let a = started(107);
        let zero = all_cells(&a)
            .into_iter()
            .find(|&(r, c)| a.is_revealed(r, c) && a.adjacent(r, c) == 0);
        if zero.is_some() {
            let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            assert!(
                !texts(&f).contains(&"0".to_string()),
                "a zero was written out"
            );
        }
    }

    #[test]
    fn the_cursor_is_outlined_and_only_the_cursor_is() {
        let mut a = game(108);
        key(&mut a, Key::Right);
        key(&mut a, Key::Down);
        let l = layout_of(&a);
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let boxes = strokes(&f);
        assert_eq!(boxes.len(), 1, "{} things are outlined", boxes.len());
        assert_eq!(
            boxes.first().copied(),
            Some(l.cell_rect(1, 1)),
            "the outline is not on the cursor"
        );
    }

    #[test]
    fn the_footer_names_the_keys_that_do_something() {
        let a = game(109);
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let all = texts(&f).join(" | ");
        for &(k, what) in SHORTCUTS {
            assert!(
                all.contains(&format!("{k} {what}")),
                "no '{k} {what}' in {all}"
            );
        }
    }

    #[test]
    fn the_footer_lays_its_hints_out_in_a_row_rather_than_in_a_heap() {
        // Reading the strings back proves every hint was *drawn*; it says
        // nothing about where. With the cursor never advanced, all six are
        // painted at the same x, one on top of the last, and the footer reads
        // as a single unintelligible smear -- while a test that only joins the
        // texts together passes exactly as loudly as before.
        let a = game(111);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT, a.rows(), a.cols());
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut placed: Vec<(f32, String)> = Vec::new();
        for cmd in f.commands() {
            if let RenderCommand::Text { x, y, text, .. } = cmd {
                if *y >= l.footer.y {
                    placed.push((*x, text.clone()));
                }
            }
        }
        assert_eq!(
            placed.len(),
            SHORTCUTS.len(),
            "not every hint reached the footer"
        );
        for pair in placed.windows(2) {
            let Some((x0, s0)) = pair.first() else {
                continue;
            };
            let Some((x1, _)) = pair.get(1) else { continue };
            let w = text::measure(s0, l.font, FontWeightHint::Regular);
            assert!(
                *x1 >= x0 + w,
                "'{s0}' runs from {x0} for {w}, and the next hint starts at {x1}"
            );
        }
    }

    #[test]
    fn every_key_the_footer_advertises_is_a_key_the_board_answers() {
        // The footer is a promise. A line in it naming a key that does nothing
        // is worse than no line at all.
        for &(k, _) in SHORTCUTS {
            let mut a = started(110);
            let event = match k {
                "Arrows" => press(Key::Right),
                "Space" => press(Key::Space),
                "F" => press(Key::F),
                "C" => press(Key::C),
                "D" => press(Key::D),
                "N" => press(Key::N),
                other => panic!("the footer advertises {other}, which no test knows"),
            };
            // Space, F and C each act on the cell under the cursor, so put the
            // cursor somewhere they have work to do: C wants a satisfied
            // number, Space and F want a cell that is still covered.
            let want = if k == "C" {
                a_satisfied_number(&mut a).expect("a number")
            } else {
                *all_cells(&a)
                    .iter()
                    .find(|&&(r, c)| !a.is_revealed(r, c))
                    .expect("a covered cell")
            };
            if matches!(k, "Space" | "F" | "C") {
                walk_cursor_to(&mut a, want.0, want.1);
            }
            assert_eq!(
                probe::key(&mut a, &event),
                EventResult::Consumed,
                "the footer advertises {k}, which does nothing"
            );
        }
    }

    #[test]
    fn the_footer_drops_the_hints_that_do_not_fit_rather_than_running_off_the_edge() {
        let a = game(111);
        let l = Layout::new(240.0, 500.0, 9, 9);
        let f = a.frame(240.0, 500.0);
        for cmd in f.commands() {
            if let RenderCommand::Text { x, y, .. } = cmd
                && *y >= l.footer.y
            {
                assert!(*x <= l.footer.right(), "a hint starts past the right edge");
            }
        }
    }

    // ── The window's own surface ───────────────────────────────────────────

    #[test]
    fn the_program_names_itself_the_same_way_everywhere() {
        let a = game(120);
        assert_eq!(a.title(), "Minesweeper");
        assert_eq!(a.app_id(), "minesweeper", "the id must match `main`'s");
        assert_eq!(a.initial_size(), (940, 660));
    }

    #[test]
    fn a_close_request_closes_the_window() {
        let mut a = started(121);
        assert!(matches!(a.on_event(&Event::CloseRequested), Response::Exit));
    }

    #[test]
    fn an_event_that_changed_something_asks_for_a_repaint_and_one_that_did_not_does_not() {
        let mut a = game(122);
        assert!(matches!(
            a.on_event(&Event::Key(press(Key::Right))),
            Response::Redraw
        ));
        assert!(matches!(
            a.on_event(&Event::Key(press(Key::Up))),
            Response::Idle,
        ));
    }

    #[test]
    fn the_events_this_program_has_no_use_for_are_left_alone() {
        let mut a = started(123);
        for e in [
            Event::FocusIn,
            Event::FocusOut,
            Event::Moved { x: 10, y: 10 },
            Event::ScaleChanged { scale: 2.0 },
        ] {
            assert_eq!(
                handle_event(&mut a, &e),
                EventResult::Ignored,
                "{e:?} acted"
            );
        }
    }

    #[test]
    fn drawing_the_window_is_the_same_as_drawing_the_frame() {
        let mut a = started(124);
        let tree = a.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let frame = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree();
        assert_eq!(tree.commands.len(), frame.commands.len());
    }
}
