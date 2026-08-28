//! Slate OS Sudoku — the classic 9x9 number puzzle, in a window.
//!
//! Three difficulties, a backtracking generator that guarantees a unique
//! solution, pencil marks, undo/redo, hints, a clock, conflict highlighting and
//! per-difficulty statistics.
//!
//! # What this was, and what was wrong with it
//!
//! It drew a sudoku and answered arrow keys for its whole life without ever
//! depending on the window system. `main` was `let _app = SudokuApp::new();` —
//! it generated a puzzle, dropped it, and exited. Wiring it to a window turned
//! up eleven faults that a program nobody could run had no way to show:
//!
//! * **The layout was a picture of a window rather than a window.** `CELL_SIZE`
//!   was 52 pixels, the grid origin was a constant, and `render` computed
//!   `total_width`/`total_height` *from the cell size* — the drawing pass told
//!   the window what size to be. A resize moved nothing.
//! * **Nothing but a cell was clickable, and clicking a cell was all a mouse
//!   could do.** There was no way to enter a digit, take a hint, toggle notes,
//!   undo, pause or start a new game without a keyboard. That is not a
//!   playable program; it is a keyboard program with a picture attached.
//! * **The clock counted its wake-ups, not time.** `handle_tick` ignored its
//!   `elapsed_ms` and did `elapsed_secs += 1`, so the displayed time was a
//!   count of `Event::Tick`s. Nothing generated ticks, so it was always `00:00`.
//! * **`given` meant two different things.** It marked the puzzle's own clues,
//!   *and* it was set on a cell a hint filled in, to lock it. So `given_count`
//!   grew as hints were spent, and "how many clues does this puzzle have" and
//!   "which cells may I not edit" — two different questions — had one answer.
//! * **`hints_remaining` was a counter maintained in three places.** `use_hint`
//!   decremented it, `undo` incremented it, `redo` decremented it again; drop
//!   the oldest entry off the capped undo stack and the three stop agreeing.
//!   `redo`'s decrement was unguarded, so it could also underflow. It is
//!   derived from the cells now.
//! * **A redo could not win the game.** `check_completion` was called from
//!   `input_digit` and `use_hint` but not from `redo`, so redoing the last
//!   digit left a finished grid sitting at `Playing` forever.
//! * **A win or a pause locked out the buttons that undo them.** `handle_mouse`
//!   returned unless the status was `Playing`, and the pause guard in
//!   `handle_key` stood in front of `F2`, so a paused game could not be
//!   restarted and a won one could not be clicked at all.
//! * **The pause hid nothing.** The board stayed on screen while "Paused" was
//!   printed over it, so the only things paused were the clock and the
//!   keyboard — not the part a player pauses to stop looking at.
//! * **`toggle_pause`'s third arm was unreachable.** `Won => Won` sat behind an
//!   earlier `if status == Won { return }`, which is `known-issues.md` lesson
//!   51: a guard standing behind a duplicate of itself is a guard no test can
//!   reach.
//! * **The bounds check on a square was a wrap.** [`at`], `put`,
//!   [`cell`](SudokuApp::cell) and `write` all reached the array through
//!   [`idx`], which is row-major, and then asked `get` whether the index was in
//!   range. A column past the ninth *is* in range — it is the next row's cell —
//!   so `at(grid, 0, 9)` handed back `(1, 0)` while its doc comment promised
//!   `0`. Only the last row's overflow was ever caught.
//! * **A label could start above the box it labelled.** `centred_in` clamped
//!   the horizontal offset and left the vertical one alone, so a line taller
//!   than its box centred to a *negative* offset. The header's heading is one:
//!   a 22.5pt line in a 28pt half-band began six tenths of a pixel above the
//!   top of the window, and the screen edge took the difference.
//!
//! On top of that the crate carried `#![allow(dead_code)]` and eight more
//! crate-wide allows, and did not pass the lane's clippy gate at all.
//!
//! # Shape
//!
//! [`Layout`] is derived from the live window size on every frame and never
//! stored on the model. Every control the renderer paints it also records with
//! [`Frame::hit`](guitk::frame::Frame::hit), which is what lets a test click a
//! cell or a keypad key by name and what lets the pointer find one. Every
//! input — key or click — turns into an [`Intent`] and goes through
//! [`SudokuApp::apply`], so the two can never drift apart.

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
const BASE: Color = Color::from_hex(0x001E_1E2E);
const MANTLE: Color = Color::from_hex(0x0018_1825);
const CRUST: Color = Color::from_hex(0x0011_111B);
const SURFACE0: Color = Color::from_hex(0x0031_3244);
const SURFACE1: Color = Color::from_hex(0x0045_475A);
const SURFACE2: Color = Color::from_hex(0x0058_5B70);
const TEXT_COLOR: Color = Color::from_hex(0x00CD_D6F4);
const SUBTEXT0: Color = Color::from_hex(0x00A6_ADC8);
const BLUE: Color = Color::from_hex(0x0089_B4FA);
const GREEN: Color = Color::from_hex(0x00A6_E3A1);
const RED: Color = Color::from_hex(0x00F3_8BA8);
const YELLOW: Color = Color::from_hex(0x00F9_E2AF);
const PEACH: Color = Color::from_hex(0x00FA_B387);
const LAVENDER: Color = Color::from_hex(0x00B4_BEFE);
const OVERLAY0: Color = Color::from_hex(0x006C_7086);
const TEAL: Color = Color::from_hex(0x0094_E2D5);
const MAUVE: Color = Color::from_hex(0x00CB_A6F7);

// ── Board shape ────────────────────────────────────────────────────────────

/// Cells along one side of the board.
pub const GRID_SIZE: usize = 9;
/// Cells along one side of a box, the 3x3 group a digit may not repeat in.
pub const BOX_SIZE: usize = 3;
/// Cells in the whole board.
pub const TOTAL_CELLS: usize = GRID_SIZE * GRID_SIZE;

/// Hints a player is given per puzzle.
pub const MAX_HINTS: usize = 5;
/// How many moves back the undo history reaches.
pub const MAX_UNDO: usize = 500;

// ── Window and clock ───────────────────────────────────────────────────────

/// The size the window asks for, and the size the tests draw at unless they say
/// otherwise.
pub const WINDOW_WIDTH: f32 = 720.0;
/// See [`WINDOW_WIDTH`].
pub const WINDOW_HEIGHT: f32 = 780.0;

/// How often the clock is woken while a puzzle is being solved.
///
/// A second, because a second is what the clock displays. This program used to
/// have a clock, a `MM:SS` in its header, and nothing on earth to advance it.
pub const CLOCK_MS: u64 = 1_000;

// ── Randomness ─────────────────────────────────────────────────────────────

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `SUDOKU!!`.
const FALLBACK_SEED: u64 = 0x5355_444F_4B55_2121;

// This crate used to carry its own copy of the LCG that got copied into
// sixteen crates, together with its own Fisher-Yates over it, reducing with
// `val % bound`. That is the broken reduction: the generator's modulus is 2^64,
// so bit *k* of its state has period 2^(k+1) and the low bits are a counter
// rather than a draw. Any power-of-two bound reads only those.
//
// A shuffle is the worst possible caller for it, because its bound counts all
// the way down to 2 and so passes through every power of two on the way, and
// both of this crate's shuffles are long ones: the candidate digits at each
// cell of the solver (1 to 9 of them, so often 2, 4 or 8) and the 81 cell
// indices whose order decides which givens get removed (through 64, 32, 16, 8,
// 4 and 2). Those swaps were not draws; they were a fixed function of their
// position in the loop.

// ── Difficulty ─────────────────────────────────────────────────────────────

/// How many clues a puzzle starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    /// 35 to 40 clues.
    Easy,
    /// 28 to 34 clues.
    Medium,
    /// 22 to 27 clues.
    Hard,
}

impl Difficulty {
    /// Every difficulty, in the order the chip cycles through them.
    pub const ALL: [Self; 3] = [Self::Easy, Self::Medium, Self::Hard];

    /// The inclusive range of clues a puzzle at this difficulty aims for.
    #[must_use]
    pub fn givens_range(self) -> (usize, usize) {
        match self {
            Self::Easy => (35, 40),
            Self::Medium => (28, 34),
            Self::Hard => (22, 27),
        }
    }

    /// The word shown in the header.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }

    /// The colour that word is drawn in.
    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Easy => GREEN,
            Self::Medium => YELLOW,
            Self::Hard => RED,
        }
    }

    /// The next difficulty, wrapping from the hardest back to the easiest.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Easy => Self::Medium,
            Self::Medium => Self::Hard,
            Self::Hard => Self::Easy,
        }
    }
}

// ── Cells ──────────────────────────────────────────────────────────────────

/// Where a cell's value came from.
///
/// This replaces a single `given: bool` that meant both "this is one of the
/// puzzle's clues" and "you may not edit this", and so answered the first
/// question wrongly as soon as a hint set it to lock a cell down. They are two
/// questions: [`Origin::Given`] answers the first, [`Cell::fixed`] the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// One of the puzzle's clues.
    Given,
    /// Filled in by a hint, and locked so the player cannot spend a hint and
    /// then paint over it.
    Hint,
    /// Entered by the player, and freely editable.
    Player,
}

/// One square of the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The digit shown, or 0 for an empty cell.
    pub value: u8,
    /// Where [`Cell::value`] came from.
    pub origin: Origin,
    /// Pencil marks; index `d - 1` is the mark for digit `d`.
    pub notes: [bool; GRID_SIZE],
}

impl Cell {
    /// An empty, editable cell.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            value: 0,
            origin: Origin::Player,
            notes: [false; GRID_SIZE],
        }
    }

    /// One of the puzzle's clues.
    #[must_use]
    pub const fn as_given(value: u8) -> Self {
        Self {
            value,
            origin: Origin::Given,
            notes: [false; GRID_SIZE],
        }
    }

    /// Whether this cell holds no digit. Pencil marks are not a digit.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.value == 0
    }

    /// Whether the player may change this cell.
    #[must_use]
    pub fn fixed(&self) -> bool {
        matches!(self.origin, Origin::Given | Origin::Hint)
    }

    /// Erase every pencil mark.
    pub const fn clear_notes(&mut self) {
        self.notes = [false; GRID_SIZE];
    }

    /// Whether any pencil mark is set.
    #[must_use]
    pub fn has_any_note(&self) -> bool {
        self.notes.iter().any(|&n| n)
    }

    /// Turn the mark for `digit` on if it is off, and off if it is on.
    ///
    /// A digit outside 1..=9 is not a digit and is ignored rather than
    /// indexing past the end of the marks.
    pub fn toggle_note(&mut self, digit: u8) {
        if let Some(slot) = note_slot(digit).and_then(|i| self.notes.get_mut(i)) {
            *slot = !*slot;
        }
    }

    /// Whether the mark for `digit` is set.
    #[must_use]
    pub fn has_note(&self, digit: u8) -> bool {
        note_slot(digit)
            .and_then(|i| self.notes.get(i))
            .copied()
            .unwrap_or(false)
    }
}

/// The index in a cell's pencil marks for `digit`, or `None` if that is not a
/// digit this game has.
#[must_use]
fn note_slot(digit: u8) -> Option<usize> {
    if (1..=9).contains(&digit) {
        usize::from(digit).checked_sub(1)
    } else {
        None
    }
}

// ── Undo / redo ────────────────────────────────────────────────────────────

/// One reversible change to the board.
///
/// The *new* value of a hint is deliberately not recorded: it is the solution's
/// digit for that cell, which the app already holds, and a second copy of a
/// fact is a second thing that can be wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// A digit placed or erased by the player.
    SetValue {
        /// Row of the cell that changed.
        row: usize,
        /// Column of the cell that changed.
        col: usize,
        /// What the cell held before.
        old_value: u8,
        /// What the cell holds after.
        new_value: u8,
        /// The pencil marks that placing a value swept away.
        old_notes: [bool; GRID_SIZE],
    },
    /// A pencil mark turned on or off. Its own inverse.
    ToggleNote {
        /// Row of the cell that changed.
        row: usize,
        /// Column of the cell that changed.
        col: usize,
        /// The digit whose mark was toggled.
        digit: u8,
    },
    /// A hint spent on a cell.
    Hint {
        /// Row of the cell that changed.
        row: usize,
        /// Column of the cell that changed.
        col: usize,
        /// What the cell held before.
        old_value: u8,
        /// The pencil marks the hint swept away.
        old_notes: [bool; GRID_SIZE],
    },
}

// ── Statistics ─────────────────────────────────────────────────────────────

/// Games finished and best times, kept per difficulty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    completed: [u32; 3],
    best: [Option<u64>; 3],
}

/// The slot in [`Stats`] a difficulty uses.
fn stat_slot(difficulty: Difficulty) -> usize {
    match difficulty {
        Difficulty::Easy => 0,
        Difficulty::Medium => 1,
        Difficulty::Hard => 2,
    }
}

impl Stats {
    /// How many puzzles at this difficulty have been finished.
    #[must_use]
    pub fn games_completed(&self, difficulty: Difficulty) -> u32 {
        self.completed
            .get(stat_slot(difficulty))
            .copied()
            .unwrap_or(0)
    }

    /// The quickest finish at this difficulty, in seconds.
    #[must_use]
    pub fn best_time(&self, difficulty: Difficulty) -> Option<u64> {
        self.best.get(stat_slot(difficulty)).copied().flatten()
    }

    /// Record a finish. A slower time never replaces a faster one.
    pub fn record_completion(&mut self, difficulty: Difficulty, elapsed_secs: u64) {
        let slot = stat_slot(difficulty);
        if let Some(count) = self.completed.get_mut(slot) {
            *count = count.saturating_add(1);
        }
        if let Some(best) = self.best.get_mut(slot) {
            *best = Some(match *best {
                Some(prev) => prev.min(elapsed_secs),
                None => elapsed_secs,
            });
        }
    }

    /// Puzzles finished at every difficulty together.
    #[must_use]
    pub fn total_completed(&self) -> u32 {
        Difficulty::ALL
            .iter()
            .map(|&d| self.games_completed(d))
            .fold(0u32, u32::saturating_add)
    }
}

// ── Grid utilities ─────────────────────────────────────────────────────────

/// The flat index of `(row, col)`.
#[must_use]
pub fn idx(row: usize, col: usize) -> usize {
    row.saturating_mul(GRID_SIZE).saturating_add(col)
}

/// The `(row, col)` a flat index names.
#[must_use]
pub fn row_col(index: usize) -> (usize, usize) {
    (index.wrapping_div(GRID_SIZE), index.wrapping_rem(GRID_SIZE))
}

/// The top-left corner of the 3x3 box holding `(row, col)`.
#[must_use]
pub fn box_origin(row: usize, col: usize) -> (usize, usize) {
    (
        row.wrapping_div(BOX_SIZE).wrapping_mul(BOX_SIZE),
        col.wrapping_div(BOX_SIZE).wrapping_mul(BOX_SIZE),
    )
}

/// The digit at `(row, col)`, or 0 for a cell off the board.
///
/// The column is checked as well as the index. `idx` is row-major, so a column
/// past the ninth is not off the end of the array -- it is the *next row's*
/// cell, and `get` cannot tell the two apart. Reading `(0, 9)` used to hand
/// back `(1, 0)`, which is a wrap dressed up as a bounds check.
#[must_use]
pub fn at(grid: &[u8; TOTAL_CELLS], row: usize, col: usize) -> u8 {
    if row >= GRID_SIZE || col >= GRID_SIZE {
        return 0;
    }
    grid.get(idx(row, col)).copied().unwrap_or(0)
}

/// Put `digit` at `(row, col)`, doing nothing for a cell off the board.
///
/// The column is checked for the reason given on [`at`]: without it a write to
/// `(0, 9)` lands on `(1, 0)`.
fn put(grid: &mut [u8; TOTAL_CELLS], row: usize, col: usize, digit: u8) {
    if row >= GRID_SIZE || col >= GRID_SIZE {
        return;
    }
    if let Some(slot) = grid.get_mut(idx(row, col)) {
        *slot = digit;
    }
}

/// Whether `digit` at `(row, col)` repeats a digit already in its row, column
/// or box.
///
/// The cell itself is skipped on every one of the three scans, so this answers
/// the same for a digit already written there as for one about to be.
#[must_use]
pub fn has_conflict(grid: &[u8; TOTAL_CELLS], row: usize, col: usize, digit: u8) -> bool {
    if digit == 0 {
        return false;
    }
    if (0..GRID_SIZE).any(|c| c != col && at(grid, row, c) == digit) {
        return true;
    }
    if (0..GRID_SIZE).any(|r| r != row && at(grid, r, col) == digit) {
        return true;
    }
    let (br, bc) = box_origin(row, col);
    (0..BOX_SIZE).any(|dr| {
        (0..BOX_SIZE).any(|dc| {
            let (r, c) = (br.saturating_add(dr), bc.saturating_add(dc));
            (r, c) != (row, col) && at(grid, r, c) == digit
        })
    })
}

/// Whether every cell is filled and no digit repeats.
#[must_use]
pub fn is_grid_complete(grid: &[u8; TOTAL_CELLS]) -> bool {
    !grid.contains(&0) && is_grid_valid(grid)
}

/// Whether no *filled* cell repeats a digit. Empty cells are not a conflict.
#[must_use]
pub fn is_grid_valid(grid: &[u8; TOTAL_CELLS]) -> bool {
    (0..TOTAL_CELLS).all(|i| {
        let (r, c) = row_col(i);
        !has_conflict(grid, r, c, at(grid, r, c))
    })
}

/// The digits of a board of cells, without their marks or their origins.
#[must_use]
pub fn values_array(cells: &[Cell; TOTAL_CELLS]) -> [u8; TOTAL_CELLS] {
    let mut arr = [0u8; TOTAL_CELLS];
    for (slot, cell) in arr.iter_mut().zip(cells.iter()) {
        *slot = cell.value;
    }
    arr
}

// ── Solver ─────────────────────────────────────────────────────────────────

/// The first empty cell, scanning left to right and top to bottom.
#[must_use]
pub fn find_empty(grid: &[u8; TOTAL_CELLS]) -> Option<(usize, usize)> {
    (0..TOTAL_CELLS)
        .map(row_col)
        .find(|&(r, c)| at(grid, r, c) == 0)
}

/// The empty cell to try next, and the digits that could go in it.
///
/// The cell with the *fewest* candidates, not the first one in reading order.
/// This is the standard minimum-remaining-values heuristic, and on this program
/// it is not a micro-optimisation: generating one hard puzzle calls
/// [`count_solutions`] up to eighty-one times, each a search from scratch, and
/// scanning in reading order made that take **1.2 seconds** in a debug build —
/// on the thread that draws the window, so pressing New froze the program.
/// Choosing the most-constrained cell prunes the tree at the top instead of the
/// bottom and cuts it by roughly two orders of magnitude.
///
/// A cell with no candidates at all is returned immediately, because the search
/// below it is already dead and there is nothing to gain by looking further.
#[must_use]
fn next_cell(grid: &[u8; TOTAL_CELLS]) -> Option<(usize, usize, u16)> {
    let mut best: Option<(usize, usize, u16, u32)> = None;
    for i in 0..TOTAL_CELLS {
        let (r, c) = row_col(i);
        if at(grid, r, c) != 0 {
            continue;
        }
        let mask = candidates(grid, r, c);
        let count = mask.count_ones();
        if count == 0 {
            return Some((r, c, mask));
        }
        if best.is_none_or(|(_, _, _, seen)| count < seen) {
            best = Some((r, c, mask, count));
        }
    }
    best.map(|(r, c, mask, _)| (r, c, mask))
}

/// Which digits could go at `(row, col)`, as a bit per digit: bit `d - 1` is
/// set when `d` is a candidate.
#[must_use]
pub fn candidates(grid: &[u8; TOTAL_CELLS], row: usize, col: usize) -> u16 {
    let mut mask: u16 = 0x1FF;
    let mut strike = |v: u8| {
        if let Some(bit) = note_slot(v) {
            mask &= !(1u16 << bit);
        }
    };
    for i in 0..GRID_SIZE {
        strike(at(grid, row, i));
        strike(at(grid, i, col));
    }
    let (br, bc) = box_origin(row, col);
    for dr in 0..BOX_SIZE {
        for dc in 0..BOX_SIZE {
            strike(at(grid, br.saturating_add(dr), bc.saturating_add(dc)));
        }
    }
    mask
}

/// Whether `digit` is one of the candidates in `mask`.
#[must_use]
fn is_candidate(mask: u16, digit: u8) -> bool {
    note_slot(digit).is_some_and(|bit| mask & (1u16 << bit) != 0)
}

/// Fill the grid in place. Returns whether it could be.
pub fn solve(grid: &mut [u8; TOTAL_CELLS]) -> bool {
    let Some((row, col, cands)) = next_cell(grid) else {
        return true;
    };
    for digit in 1..=9u8 {
        if is_candidate(cands, digit) {
            put(grid, row, col, digit);
            if solve(grid) {
                return true;
            }
            put(grid, row, col, 0);
        }
    }
    false
}

/// How many ways the grid can be completed, counting no further than `limit`.
///
/// The limit is what makes this affordable: the generator only ever asks "is
/// there more than one", so it passes 2 and the search stops at the second.
pub fn count_solutions(grid: &mut [u8; TOTAL_CELLS], limit: usize) -> usize {
    // Asking for no answers at all is decided here rather than inside the
    // search, because the search's own bound is the `total >= limit` that ends
    // its loop. A second `found >= limit` at the top of the recursion would be
    // a copy of that bound, not a bound of its own: with two of them, breaking
    // either one leaves the other still stopping the search, so no test could
    // tell a working bound from a broken one. That is `known-issues.md` lesson
    // 51, and this is the shape it takes when the duplicate is a base case.
    if limit == 0 {
        return 0;
    }
    count_solutions_inner(grid, limit, 0)
}

fn count_solutions_inner(grid: &mut [u8; TOTAL_CELLS], limit: usize, found: usize) -> usize {
    let Some((row, col, cands)) = next_cell(grid) else {
        return found.saturating_add(1);
    };
    let mut total = found;
    for digit in 1..=9u8 {
        if total >= limit {
            break;
        }
        if is_candidate(cands, digit) {
            put(grid, row, col, digit);
            total = count_solutions_inner(grid, limit, total);
            put(grid, row, col, 0);
        }
    }
    total
}

/// Fill the grid in place, trying candidate digits in a shuffled order.
///
/// This is what makes two seeds give two different puzzles; [`solve`] always
/// tries 1 first and so always produces the same completion of a given grid.
pub fn solve_shuffled(grid: &mut [u8; TOTAL_CELLS], rng: &mut SeededRng) -> bool {
    let Some((row, col, cands)) = next_cell(grid) else {
        return true;
    };
    let mut digits: Vec<u8> = (1..=9u8).filter(|&d| is_candidate(cands, d)).collect();
    rng.shuffle(&mut digits);
    for digit in digits {
        put(grid, row, col, digit);
        if solve_shuffled(grid, rng) {
            return true;
        }
        put(grid, row, col, 0);
    }
    false
}

// ── Puzzle generation ──────────────────────────────────────────────────────

/// A complete, valid board.
#[must_use]
pub fn generate_full_grid(rng: &mut SeededRng) -> [u8; TOTAL_CELLS] {
    let mut grid = [0u8; TOTAL_CELLS];
    if solve_shuffled(&mut grid, rng) {
        return grid;
    }
    // Unreachable: an empty grid has no constraints to violate, so the search
    // cannot fail. A board is still returned rather than a panic, because a
    // panic here would take a player's unrelated game down with it.
    [
        5, 3, 4, 6, 7, 8, 9, 1, 2, 6, 7, 2, 1, 9, 5, 3, 4, 8, 1, 9, 8, 3, 4, 2, 5, 6, 7, 8, 5, 9,
        7, 6, 1, 4, 2, 3, 4, 2, 6, 8, 5, 3, 7, 9, 1, 7, 1, 3, 9, 2, 4, 8, 5, 6, 9, 6, 1, 5, 3, 7,
        2, 8, 4, 2, 8, 7, 4, 1, 9, 6, 3, 5, 3, 4, 5, 2, 8, 6, 1, 7, 9,
    ]
}

/// A puzzle and the solution it was carved out of.
///
/// Clues are removed one at a time in a shuffled order, and a removal is kept
/// only if the puzzle still has exactly one solution — which is what makes the
/// puzzle solvable by reasoning rather than by guessing.
#[must_use]
pub fn generate_puzzle(
    rng: &mut SeededRng,
    difficulty: Difficulty,
) -> ([Cell; TOTAL_CELLS], [u8; TOTAL_CELLS]) {
    let solution = generate_full_grid(rng);

    let (min_givens, max_givens) = difficulty.givens_range();
    let span = max_givens.saturating_sub(min_givens).saturating_add(1);
    let target_givens = min_givens.saturating_add(rng.below(span));
    let target_removals = TOTAL_CELLS.saturating_sub(target_givens);

    let mut order: Vec<usize> = (0..TOTAL_CELLS).collect();
    rng.shuffle(&mut order);

    let mut puzzle = solution;
    let mut removed = 0usize;
    for &cell_idx in &order {
        if removed >= target_removals {
            break;
        }
        let Some(saved) = puzzle.get(cell_idx).copied() else {
            continue;
        };
        if let Some(slot) = puzzle.get_mut(cell_idx) {
            *slot = 0;
        }
        let mut test = puzzle;
        if count_solutions(&mut test, 2) == 1 {
            removed = removed.saturating_add(1);
        } else if let Some(slot) = puzzle.get_mut(cell_idx) {
            // Removing this clue would have left the puzzle with more than one
            // answer, so it stays. This is why a hard puzzle can come out with
            // more clues than its range asks for: uniqueness wins over count.
            *slot = saved;
        }
    }

    let cells = core::array::from_fn(|i| match puzzle.get(i).copied() {
        Some(v) if v != 0 => Cell::as_given(v),
        _ => Cell::empty(),
    });
    (cells, solution)
}

// ── What can be clicked ────────────────────────────────────────────────────

/// Every control the renderer records a box for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A square of the board, by `(row, column)`.
    Cell(usize, usize),
    /// A keypad digit.
    Digit(u8),
    /// The keypad's erase key.
    Erase,
    /// The pencil-marks toggle.
    Notes,
    /// The hint key.
    Hint,
    /// The undo key.
    Undo,
    /// The redo key.
    Redo,
    /// The pause chip.
    Pause,
    /// The difficulty chip.
    Difficulty,
    /// The new-game chip.
    NewGame,
}

/// The keypad, left to right.
///
/// A mouse-only player could previously select a cell and then do nothing with
/// it, because every other action was a key. This strip is the whole of the
/// game's input in one row.
pub const KEYPAD: [Target; 14] = [
    Target::Digit(1),
    Target::Digit(2),
    Target::Digit(3),
    Target::Digit(4),
    Target::Digit(5),
    Target::Digit(6),
    Target::Digit(7),
    Target::Digit(8),
    Target::Digit(9),
    Target::Erase,
    Target::Notes,
    Target::Hint,
    Target::Undo,
    Target::Redo,
];

/// The word drawn on a keypad key.
#[must_use]
pub fn key_label(target: Target) -> String {
    match target {
        Target::Digit(d) => d.to_string(),
        Target::Erase => "Erase".to_string(),
        Target::Notes => "Notes".to_string(),
        Target::Hint => "Hint".to_string(),
        Target::Undo => "Undo".to_string(),
        Target::Redo => "Redo".to_string(),
        Target::Pause => "Pause".to_string(),
        Target::Difficulty => "Difficulty".to_string(),
        Target::NewGame => "New".to_string(),
        Target::Cell(r, c) => format!("{r},{c}"),
    }
}

/// A direction the selection can move in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Towards row 0.
    Up,
    /// Towards row 8.
    Down,
    /// Towards column 0.
    Left,
    /// Towards column 8.
    Right,
}

/// Everything the player can ask for, however they asked for it.
///
/// One name per intention, so a key and a click that mean the same thing go
/// down the same path. That is what stops the two drifting apart — and this
/// program had no path at all for a click that meant anything but "select".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Put the selection on a cell.
    Select(usize, usize),
    /// Move the selection one cell.
    Move(Dir),
    /// Write a digit, or toggle its pencil mark in note mode.
    Digit(u8),
    /// Empty the selected cell.
    Erase,
    /// Turn pencil-mark mode on or off.
    ToggleNotes,
    /// Fill the selected cell from the solution, spending a hint.
    Hint,
    /// Take back the last change.
    Undo,
    /// Put back the last undone change.
    Redo,
    /// Pause or resume.
    Pause,
    /// Deal a fresh puzzle at the same difficulty.
    NewGame,
    /// Deal a fresh puzzle at the next difficulty.
    CycleDifficulty,
    /// Deal a fresh puzzle at a named difficulty.
    SetDifficulty(Difficulty),
}

// ── Game state ─────────────────────────────────────────────────────────────

/// Whether the puzzle is being solved, finished, or set aside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameStatus {
    /// Being solved.
    Playing,
    /// Finished.
    Won,
    /// Set aside; the clock is stopped and the board is hidden.
    Paused,
}

/// The whole game.
pub struct SudokuApp {
    cells: [Cell; TOTAL_CELLS],
    solution: [u8; TOTAL_CELLS],
    difficulty: Difficulty,
    status: GameStatus,
    selected: (usize, usize),
    note_mode: bool,
    undo_stack: Vec<Change>,
    redo_stack: Vec<Change>,
    elapsed_ms: u64,
    stats: Stats,
    seed_counter: u64,
    size: (f32, f32),
}

impl SudokuApp {
    /// A fresh easy puzzle, seeded from the system.
    #[must_use]
    pub fn new() -> Self {
        // Was `with_seed_and_difficulty(42, ...)`: every player, on every
        // machine, got the same puzzle. The `u64` form is used and not the
        // generator form because this app *stores* its seed: `seed_counter` is
        // incremented to make the next puzzle, and an app holding a generator
        // would have to reseed one generator from another's output, which
        // silently correlates the two.
        Self::with_seed_and_difficulty(seed_from_system(FALLBACK_SEED), Difficulty::Easy)
    }

    /// A puzzle from a named seed, for a test that needs the same board twice.
    #[must_use]
    pub fn with_seed_and_difficulty(seed: u64, difficulty: Difficulty) -> Self {
        let mut rng = SeededRng::new(seed);
        let (cells, solution) = generate_puzzle(&mut rng, difficulty);
        Self::from_puzzle(cells, solution, difficulty).with_seed(seed)
    }

    /// A game on a board that is already known, without running the generator.
    ///
    /// Carving a puzzle is by far the most expensive thing this program does,
    /// and it is not what a caller wanting to *play* a known board needs — a
    /// saved game, a daily puzzle, or a test pressing a key on a fixture.
    /// Keeping it separate means those callers do not pay for it.
    #[must_use]
    pub fn from_puzzle(
        cells: [Cell; TOTAL_CELLS],
        solution: [u8; TOTAL_CELLS],
        difficulty: Difficulty,
    ) -> Self {
        Self {
            cells,
            solution,
            difficulty,
            status: GameStatus::Playing,
            selected: (4, 4),
            note_mode: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            elapsed_ms: 0,
            stats: Stats::default(),
            seed_counter: 0,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// The same game, with the seed the *next* puzzle will be counted up from.
    #[must_use]
    fn with_seed(mut self, seed: u64) -> Self {
        self.seed_counter = seed;
        self
    }

    /// Deal a fresh puzzle, keeping the statistics.
    pub fn new_game(&mut self, difficulty: Difficulty) {
        self.seed_counter = self.seed_counter.wrapping_add(1);
        let mut rng = SeededRng::new(self.seed_counter);
        let (cells, solution) = generate_puzzle(&mut rng, difficulty);
        self.cells = cells;
        self.solution = solution;
        self.difficulty = difficulty;
        self.status = GameStatus::Playing;
        self.selected = (4, 4);
        self.note_mode = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.elapsed_ms = 0;
    }

    // ── Reading the game ───────────────────────────────────────────────────

    /// The cell at `(row, col)`, or an empty one for a square off the board.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        if row >= GRID_SIZE || col >= GRID_SIZE {
            return Cell::empty();
        }
        self.cells
            .get(idx(row, col))
            .copied()
            .unwrap_or(Cell::empty())
    }

    /// The digit at `(row, col)`, or 0 if the cell is empty.
    #[must_use]
    pub fn value(&self, row: usize, col: usize) -> u8 {
        self.cell(row, col).value
    }

    /// The solution's digit for `(row, col)`.
    #[must_use]
    pub fn solution_at(&self, row: usize, col: usize) -> u8 {
        at(&self.solution, row, col)
    }

    /// The cell the keyboard and the keypad act on.
    #[must_use]
    pub fn selected(&self) -> (usize, usize) {
        self.selected
    }

    /// Being solved, finished, or set aside.
    #[must_use]
    pub fn status(&self) -> GameStatus {
        self.status
    }

    /// The difficulty of the puzzle on screen.
    #[must_use]
    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    /// Whether a digit writes a pencil mark rather than a value.
    #[must_use]
    pub fn note_mode(&self) -> bool {
        self.note_mode
    }

    /// The statistics.
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Milliseconds of play so far.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// Whole seconds of play so far, which is what the clock shows.
    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.elapsed_ms.wrapping_div(1_000)
    }

    /// How many hints are left.
    ///
    /// Derived from the board rather than counted down, so it cannot drift from
    /// the cells it describes — which the three separate `+= 1` / `-= 1` sites
    /// it replaces could, and did.
    #[must_use]
    pub fn hints_remaining(&self) -> usize {
        MAX_HINTS.saturating_sub(self.hints_used())
    }

    /// How many cells were filled by a hint.
    #[must_use]
    pub fn hints_used(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| c.origin == Origin::Hint)
            .count()
    }

    /// How many cells hold a digit.
    #[must_use]
    pub fn filled_count(&self) -> usize {
        self.cells.iter().filter(|c| !c.is_empty()).count()
    }

    /// How many of the puzzle's own clues there are.
    ///
    /// A hint does not add one. That it used to is why this counted up as the
    /// player spent hints.
    #[must_use]
    pub fn given_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| c.origin == Origin::Given)
            .count()
    }

    /// How many changes can still be taken back.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// How many undone changes can still be put back.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    /// Every cell whose digit repeats one in its row, column or box.
    #[must_use]
    pub fn conflicts(&self) -> Vec<(usize, usize)> {
        let vals = values_array(&self.cells);
        (0..TOTAL_CELLS)
            .map(row_col)
            .filter(|&(r, c)| has_conflict(&vals, r, c, at(&vals, r, c)))
            .collect()
    }

    /// Whether the board as it stands breaks no rule.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        is_grid_valid(&values_array(&self.cells))
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

    // ── Changing the game ──────────────────────────────────────────────────

    /// Carry out one intention, whoever asked for it.
    ///
    /// Returns [`EventResult::Consumed`] when something on screen changed, so
    /// the window system repaints exactly then.
    pub fn apply(&mut self, intent: Intent) -> EventResult {
        // The three intentions that a paused or finished game must still
        // answer, because they are the ones that undo those states. Putting
        // them in front of the status guard is the fix for a program where
        // pausing locked out `F2` and winning locked out the mouse entirely.
        match intent {
            Intent::Pause => return self.toggle_pause(),
            Intent::NewGame => {
                self.new_game(self.difficulty);
                return EventResult::Consumed;
            }
            Intent::CycleDifficulty => {
                self.new_game(self.difficulty.next());
                return EventResult::Consumed;
            }
            Intent::SetDifficulty(d) => {
                self.new_game(d);
                return EventResult::Consumed;
            }
            _ => {}
        }
        if self.status != GameStatus::Playing {
            return EventResult::Ignored;
        }
        match intent {
            Intent::Select(row, col) => self.select(row, col),
            Intent::Move(dir) => self.move_selection(dir),
            Intent::Digit(digit) => self.enter_digit(digit),
            Intent::Erase => self.erase(),
            Intent::ToggleNotes => {
                self.note_mode = !self.note_mode;
                EventResult::Consumed
            }
            Intent::Hint => self.use_hint(),
            Intent::Undo => self.undo(),
            Intent::Redo => self.redo(),
            Intent::Pause
            | Intent::NewGame
            | Intent::CycleDifficulty
            | Intent::SetDifficulty(_) => EventResult::Ignored,
        }
    }

    fn select(&mut self, row: usize, col: usize) -> EventResult {
        if row >= GRID_SIZE || col >= GRID_SIZE || self.selected == (row, col) {
            return EventResult::Ignored;
        }
        self.selected = (row, col);
        EventResult::Consumed
    }

    fn move_selection(&mut self, dir: Dir) -> EventResult {
        let (row, col) = self.selected;
        // No clamp to the last row and column here. `select` already refuses a
        // row or column off the board, and refusing leaves the selection where
        // it was -- which is exactly what a clamp would have produced. Clamping
        // here as well would be a second copy of that bound rather than a bound
        // of its own, and neither copy could then be tested (`known-issues.md`
        // lesson 51): breaking one leaves the other holding the edge.
        let next = match dir {
            Dir::Up => (row.saturating_sub(1), col),
            Dir::Down => (row.saturating_add(1), col),
            Dir::Left => (row, col.saturating_sub(1)),
            Dir::Right => (row, col.saturating_add(1)),
        };
        self.select(next.0, next.1)
    }

    fn toggle_pause(&mut self) -> EventResult {
        // Two arms, not three. The `Won => Won` arm this replaces was
        // unreachable behind an earlier `if status == Won { return }`, which is
        // exactly the shape `known-issues.md` lesson 51 is about.
        match self.status {
            GameStatus::Playing => {
                self.status = GameStatus::Paused;
                EventResult::Consumed
            }
            GameStatus::Paused => {
                self.status = GameStatus::Playing;
                EventResult::Consumed
            }
            GameStatus::Won => EventResult::Ignored,
        }
    }

    fn enter_digit(&mut self, digit: u8) -> EventResult {
        if note_slot(digit).is_none() {
            return EventResult::Ignored;
        }
        let (row, col) = self.selected;
        let cell = self.cell(row, col);
        if cell.fixed() {
            return EventResult::Ignored;
        }
        if self.note_mode {
            self.write(row, col, |c| c.toggle_note(digit));
            self.record(Change::ToggleNote { row, col, digit });
            return EventResult::Consumed;
        }
        if cell.value == digit {
            return EventResult::Ignored;
        }
        self.record(Change::SetValue {
            row,
            col,
            old_value: cell.value,
            new_value: digit,
            old_notes: cell.notes,
        });
        self.write(row, col, |c| {
            c.value = digit;
            c.clear_notes();
        });
        self.check_completion();
        EventResult::Consumed
    }

    fn erase(&mut self) -> EventResult {
        let (row, col) = self.selected;
        let cell = self.cell(row, col);
        if cell.fixed() || (cell.is_empty() && !cell.has_any_note()) {
            return EventResult::Ignored;
        }
        self.record(Change::SetValue {
            row,
            col,
            old_value: cell.value,
            new_value: 0,
            old_notes: cell.notes,
        });
        self.write(row, col, |c| {
            c.value = 0;
            c.clear_notes();
        });
        EventResult::Consumed
    }

    fn use_hint(&mut self) -> EventResult {
        if self.hints_remaining() == 0 {
            return EventResult::Ignored;
        }
        let (row, col) = self.selected;
        let cell = self.cell(row, col);
        // A clue's value *is* the answer it was cut from, so "this square is
        // already right" refuses clues on its own. Testing `cell.fixed()` here
        // as well would be a copy of the test beside it -- lesson 51 again --
        // and the copy is what the mutation sweep caught: deleting it changed
        // no answer any test could see. `a_clue_holds_the_answer_it_came_from`
        // is the test that keeps that reasoning true.
        if cell.value == self.solution_at(row, col) {
            return EventResult::Ignored;
        }
        let answer = self.solution_at(row, col);
        self.record(Change::Hint {
            row,
            col,
            old_value: cell.value,
            old_notes: cell.notes,
        });
        self.write(row, col, |c| {
            c.value = answer;
            c.origin = Origin::Hint;
            c.clear_notes();
        });
        self.check_completion();
        EventResult::Consumed
    }

    fn undo(&mut self) -> EventResult {
        let Some(change) = self.undo_stack.pop() else {
            return EventResult::Ignored;
        };
        match change {
            Change::SetValue {
                row,
                col,
                old_value,
                old_notes,
                ..
            } => self.write(row, col, |c| {
                c.value = old_value;
                c.notes = old_notes;
            }),
            Change::ToggleNote { row, col, digit } => {
                self.write(row, col, |c| c.toggle_note(digit));
            }
            Change::Hint {
                row,
                col,
                old_value,
                old_notes,
            } => self.write(row, col, |c| {
                c.value = old_value;
                c.notes = old_notes;
                c.origin = Origin::Player;
            }),
        }
        self.redo_stack.push(change);
        EventResult::Consumed
    }

    fn redo(&mut self) -> EventResult {
        let Some(change) = self.redo_stack.pop() else {
            return EventResult::Ignored;
        };
        match change {
            Change::SetValue {
                row,
                col,
                new_value,
                ..
            } => self.write(row, col, |c| {
                c.value = new_value;
                c.clear_notes();
            }),
            Change::ToggleNote { row, col, digit } => {
                self.write(row, col, |c| c.toggle_note(digit));
            }
            Change::Hint { row, col, .. } => {
                let answer = self.solution_at(row, col);
                self.write(row, col, |c| {
                    c.value = answer;
                    c.origin = Origin::Hint;
                    c.clear_notes();
                });
            }
        }
        self.undo_stack.push(change);
        // A redo can fill the last empty cell, and used not to be able to win:
        // `check_completion` was called from the two places that wrote a digit
        // forwards and from neither of the two that wrote one back.
        self.check_completion();
        EventResult::Consumed
    }

    /// Apply `edit` to the cell at `(row, col)`, doing nothing off the board.
    fn write(&mut self, row: usize, col: usize, edit: impl FnOnce(&mut Cell)) {
        if row >= GRID_SIZE || col >= GRID_SIZE {
            return;
        }
        if let Some(cell) = self.cells.get_mut(idx(row, col)) {
            edit(cell);
        }
    }

    /// Push a change onto the undo history, dropping the oldest if it is full.
    fn record(&mut self, change: Change) {
        self.undo_stack.push(change);
        self.redo_stack.clear();
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
    }

    fn check_completion(&mut self) {
        if self.status == GameStatus::Playing && is_grid_complete(&values_array(&self.cells)) {
            self.status = GameStatus::Won;
            self.stats
                .record_completion(self.difficulty, self.elapsed_secs());
        }
    }

    /// Advance the clock by real elapsed time.
    ///
    /// The version this replaces ignored its argument and added one second per
    /// wake-up, so the clock measured how often it was asked rather than how
    /// long the player had taken.
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

    // ── Input ──────────────────────────────────────────────────────────────

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        if key.modifiers.alt || key.modifiers.super_key {
            return EventResult::Ignored;
        }
        if key.modifiers.ctrl {
            let intent = match key.key {
                Key::Z => Intent::Undo,
                Key::Y => Intent::Redo,
                Key::Num1 => Intent::SetDifficulty(Difficulty::Easy),
                Key::Num2 => Intent::SetDifficulty(Difficulty::Medium),
                Key::Num3 => Intent::SetDifficulty(Difficulty::Hard),
                _ => return EventResult::Ignored,
            };
            return self.apply(intent);
        }
        let intent = match key.key {
            Key::Up => Intent::Move(Dir::Up),
            Key::Down => Intent::Move(Dir::Down),
            Key::Left => Intent::Move(Dir::Left),
            Key::Right => Intent::Move(Dir::Right),
            Key::Num1 => Intent::Digit(1),
            Key::Num2 => Intent::Digit(2),
            Key::Num3 => Intent::Digit(3),
            Key::Num4 => Intent::Digit(4),
            Key::Num5 => Intent::Digit(5),
            Key::Num6 => Intent::Digit(6),
            Key::Num7 => Intent::Digit(7),
            Key::Num8 => Intent::Digit(8),
            Key::Num9 => Intent::Digit(9),
            Key::Delete | Key::Backspace => Intent::Erase,
            Key::N => Intent::ToggleNotes,
            Key::H => Intent::Hint,
            Key::P => Intent::Pause,
            Key::D => Intent::CycleDifficulty,
            Key::F2 => Intent::NewGame,
            _ => return EventResult::Ignored,
        };
        self.apply(intent)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        let MouseEventKind::Press(MouseButton::Left) = event.kind else {
            return EventResult::Ignored;
        };
        let hit = self
            .frame(self.size.0, self.size.1)
            .hit_test(event.x, event.y);
        let intent = match hit {
            Some(Target::Cell(r, c)) => Intent::Select(r, c),
            Some(Target::Digit(d)) => Intent::Digit(d),
            Some(Target::Erase) => Intent::Erase,
            Some(Target::Notes) => Intent::ToggleNotes,
            Some(Target::Hint) => Intent::Hint,
            Some(Target::Undo) => Intent::Undo,
            Some(Target::Redo) => Intent::Redo,
            Some(Target::Pause) => Intent::Pause,
            Some(Target::Difficulty) => Intent::CycleDifficulty,
            Some(Target::NewGame) => Intent::NewGame,
            None => return EventResult::Ignored,
        };
        self.apply(intent)
    }
}

impl Default for SudokuApp {
    fn default() -> Self {
        Self::new()
    }
}

/// `MM:SS`, counting past an hour rather than wrapping at one.
#[must_use]
pub fn format_time(secs: u64) -> String {
    let mins = secs.wrapping_div(60);
    let rest = secs.wrapping_rem(60);
    format!("{mins:02}:{rest:02}")
}

/// The word shown beside the clock.
#[must_use]
pub fn status_text(status: GameStatus) -> &'static str {
    match status {
        GameStatus::Playing => "Playing",
        GameStatus::Won => "Completed",
        GameStatus::Paused => "Paused",
    }
}

/// The colour that word is drawn in.
#[must_use]
pub fn status_color(status: GameStatus) -> Color {
    match status {
        GameStatus::Playing => BLUE,
        GameStatus::Won => GREEN,
        GameStatus::Paused => YELLOW,
    }
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// Which band is given up first when the window is too short for all of them.
///
/// The footer is decoration, so it goes first. The keypad goes next even though
/// it is the only way a mouse can enter a digit, because a window with no room
/// for it has no room for the board either, and a keypad without a board is
/// nothing to press. The header carries the clock and the status, so it is last.
pub const BAND_DROP_ORDER: [usize; 3] = [2, 1, 0];

/// How much of the window's height the board is entitled to keep.
pub const BOARD_SHARE: f32 = 0.5;

/// Where everything goes in a window of a given size.
///
/// Derived on every frame and never stored on the model. The version this
/// replaces had no layout at all: a 52-pixel cell constant, a constant grid
/// origin, and a `render` that computed the window's size from them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The strip along the top, or `Rect::EMPTY` if it did not fit.
    pub header: Rect,
    /// The keypad strip, or `Rect::EMPTY` if it did not fit.
    pub keypad: Rect,
    /// The strip along the bottom, or `Rect::EMPTY` if it did not fit.
    pub footer: Rect,
    /// The band the board is drawn in.
    pub board: Rect,
    /// The board's own square inside that band, centred.
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
    /// The layout for a window of the given size.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 46.0).clamp(7.0, 15.0);
        let big = (font * 1.5).clamp(10.0, 24.0);
        // A margin may never be more than a quarter of the side it is taken
        // from: a two-pixel floor is wider than a 1x1 window, and a margin that
        // does not fit inside the thing it is a margin of puts the content it
        // indents outside the window.
        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 4.0);

        // What each band would like, in [header, keypad, footer] order.
        let mut wants = [
            (h * 0.10).clamp(26.0, 56.0),
            (h * 0.075).clamp(22.0, 46.0),
            (h * 0.05).clamp(16.0, 26.0),
        ];
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [head_h, pad_h, foot_h] = wants;

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
        let keypad = if pad_h > 0.0 {
            Rect::new(0.0, (h - foot_h - pad_h).max(0.0), w, pad_h)
        } else {
            Rect::EMPTY
        };
        let board = Rect::new(
            pad,
            head_h + pad,
            (w - pad * 2.0).max(0.0),
            (h - head_h - pad_h - foot_h - pad * 2.0).max(0.0),
        );

        // One cell is a square, so its side is whichever of the two fits.
        let side = GRID_SIZE as f32;
        let natural = (board.w / side).min(board.h / side);
        // Below a pixel a cell there is no board to draw, and the two wrong
        // answers are both worth naming. Rounding the cell *up* to a pixel makes
        // a nine-cell row nine pixels wide in a window one pixel wide, so the
        // board is painted, and clicked, outside the window it belongs to.
        // Leaving the cell at nought instead stacks all eighty-one hit boxes at
        // one point, so a click on the origin lands on whichever cell was
        // recorded last. Neither is a board, so this draws none.
        let (step, cell) = if natural < 1.0 {
            (0.0, 0.0)
        } else {
            // The gap is taken out of the cell rather than added to it, so nine
            // cells never come to more than `9 * step`.
            (
                natural,
                (natural - (natural * 0.06).clamp(0.0, 3.0)).max(1.0),
            )
        };
        let grid_w = step * side;
        let grid = Rect::new(
            board.x + (board.w - grid_w).max(0.0) / 2.0,
            board.y + (board.h - grid_w).max(0.0) / 2.0,
            grid_w,
            grid_w,
        );

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            keypad,
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

    /// The rectangle of the cell at `(row, col)`, or `Rect::EMPTY` for a square
    /// off the board or a window with no room for one. See [`Layout::new`].
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if self.cell <= 0.0 || row >= GRID_SIZE || col >= GRID_SIZE {
            return Rect::EMPTY;
        }
        Rect::new(
            self.grid.x + col as f32 * self.step,
            self.grid.y + row as f32 * self.step,
            self.cell,
            self.cell,
        )
    }

    /// The rectangle enclosing the nine cells of one 3x3 box.
    ///
    /// Derived from the corner cells rather than from a second copy of the
    /// spacing arithmetic, so a box outline cannot come to disagree with the
    /// cells it is meant to enclose.
    #[must_use]
    pub fn box_rect(&self, box_row: usize, box_col: usize) -> Rect {
        if self.cell <= 0.0 || box_row >= BOX_SIZE || box_col >= BOX_SIZE {
            return Rect::EMPTY;
        }
        let first = self.cell_rect(
            box_row.saturating_mul(BOX_SIZE),
            box_col.saturating_mul(BOX_SIZE),
        );
        let last = self.cell_rect(
            box_row
                .saturating_mul(BOX_SIZE)
                .saturating_add(BOX_SIZE.saturating_sub(1)),
            box_col
                .saturating_mul(BOX_SIZE)
                .saturating_add(BOX_SIZE.saturating_sub(1)),
        );
        Rect::new(
            first.x,
            first.y,
            (last.right() - first.x).max(0.0),
            (last.bottom() - first.y).max(0.0),
        )
    }

    /// The `i`th keypad key, counted from the left, or `Rect::EMPTY` if the
    /// keypad did not fit.
    #[must_use]
    pub fn key_rect(&self, i: usize) -> Rect {
        if !self.shows(self.keypad) || i >= KEYPAD.len() {
            return Rect::EMPTY;
        }
        let inner = (self.keypad.w - self.pad * 2.0).max(0.0);
        let step = inner / KEYPAD.len() as f32;
        if step < 1.0 {
            return Rect::EMPTY;
        }
        let gap = (step * 0.12).min(self.pad);
        let kw = (step - gap).max(1.0).min(step);
        let kh = (self.keypad.h - self.pad).max(1.0).min(self.keypad.h);
        Rect::new(
            self.keypad.x + self.pad + i as f32 * step,
            self.keypad.y + (self.keypad.h - kh) / 2.0,
            kw,
            kh,
        )
    }

    /// The `i`th header chip, counted from the right.
    #[must_use]
    pub fn chip(&self, i: usize) -> Rect {
        if !self.shows(self.header) {
            return Rect::EMPTY;
        }
        let w = (self.header.w * 0.15).clamp(40.0, 120.0);
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

/// Draw `s` in the middle of `r`, both across and down.
///
/// The digit-centring constants this replaces were two eyeballed numbers,
/// `DIGIT_HALF_WIDTH` and `DIGIT_HALF_HEIGHT`, subtracted from a cell's middle.
/// They were right for one font at one size, which is exactly as many as a
/// resizable window has.
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
    // Same clamp on the other axis, and for the same reason as the `x` above: a
    // line taller than its box centres to a *negative* offset, and the header's
    // heading is one -- a 22.5pt line in a 28pt half-band started six tenths of
    // a pixel above the window and was clipped by the screen edge.
    label(
        f,
        x,
        r.y + ((r.h - line_h) / 2.0).max(0.0),
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
        // See `centred_in`: a line taller than its box must not start above it.
        r.y + ((r.h - line_h) / 2.0).max(0.0),
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

impl SudokuApp {
    /// Draw the whole window at `width` x `height`, recording a hit box for
    /// every control drawn.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(width, height);
        // The background is the window's, not the board's. This program used to
        // fill a rectangle computed from its cell size, which is a picture of a
        // window rather than the window.
        fill(&mut f, l.window, BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_keypad(&mut f, &l);
        self.draw_footer(&mut f, &l);
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        fill(f, l.header, MANTLE, 0.0);

        chip(
            f,
            l.chip(0),
            Target::Pause,
            if self.status == GameStatus::Paused {
                "Resume"
            } else {
                "Pause"
            },
            l.font,
            YELLOW,
        );
        chip(
            f,
            l.chip(1),
            Target::Difficulty,
            self.difficulty.label(),
            l.font,
            self.difficulty.color(),
        );
        chip(f, l.chip(2), Target::NewGame, "New", l.font, LAVENDER);

        let left = Rect::new(
            l.header.x + l.pad,
            l.header.y,
            (l.chip(2).x - l.header.x - l.pad * 2.0).max(0.0),
            l.header.h,
        );
        if left.is_empty() {
            return;
        }
        let top = Rect::new(left.x, left.y, left.w, left.h / 2.0);
        let bottom = Rect::new(left.x, left.y + left.h / 2.0, left.w, left.h / 2.0);
        left_in(
            f,
            top,
            &format!("Sudoku  {}", format_time(self.elapsed_secs())),
            l.big,
            LAVENDER,
            FontWeightHint::Bold,
        );
        left_in(
            f,
            bottom,
            &format!(
                "{}   {}/{TOTAL_CELLS}   Hints {}   Notes {}",
                status_text(self.status),
                self.filled_count(),
                self.hints_remaining(),
                if self.note_mode { "on" } else { "off" },
            ),
            l.font,
            status_color(self.status),
            FontWeightHint::Regular,
        );
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.grid) {
            return;
        }
        fill(f, l.grid, CRUST, 4.0);
        let conflicts = self.conflicts();
        for i in 0..TOTAL_CELLS {
            let (row, col) = row_col(i);
            self.draw_cell(f, l, row, col, &conflicts);
        }
        for box_row in 0..BOX_SIZE {
            for box_col in 0..BOX_SIZE {
                stroke(f, l.box_rect(box_row, box_col), OVERLAY0, 1.5, 2.0);
            }
        }
    }

    fn draw_cell(
        &self,
        f: &mut Frame,
        l: &Layout,
        row: usize,
        col: usize,
        conflicts: &[(usize, usize)],
    ) {
        let r = l.cell_rect(row, col);
        if r.is_empty() {
            return;
        }
        let hidden = self.status == GameStatus::Paused;
        let cell = self.cell(row, col);
        let (srow, scol) = self.selected;
        let selected = (row, col) == (srow, scol);
        let conflicting = !hidden && conflicts.contains(&(row, col));
        let matching =
            !hidden && !cell.is_empty() && cell.value == self.value(srow, scol) && !selected;
        let in_scope = row == srow || col == scol || box_origin(row, col) == box_origin(srow, scol);

        let bg = if hidden {
            SURFACE0
        } else if selected {
            SURFACE2
        } else if conflicting {
            Color::rgba(243, 139, 168, 60)
        } else if matching {
            Color::rgba(137, 180, 250, 45)
        } else if in_scope {
            SURFACE1
        } else {
            SURFACE0
        };
        fill(f, r, bg, l.cell * 0.06);
        f.hit(Target::Cell(row, col), r);
        if selected {
            stroke(f, r, BLUE, 2.0, l.cell * 0.06);
        }
        if hidden {
            // A pause that leaves the board on screen has paused the clock and
            // the keyboard but not the thing a player pauses to stop looking at.
            return;
        }
        if cell.is_empty() {
            self.draw_notes(f, l, r, cell);
            return;
        }
        let color = if conflicting {
            RED
        } else {
            match cell.origin {
                Origin::Given => TEXT_COLOR,
                Origin::Hint => PEACH,
                Origin::Player => BLUE,
            }
        };
        let weight = if cell.origin == Origin::Given {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };
        centred_in(f, r, &cell.value.to_string(), l.cell * 0.6, color, weight);
    }

    fn draw_notes(&self, f: &mut Frame, l: &Layout, r: Rect, cell: Cell) {
        if !cell.has_any_note() {
            return;
        }
        let third = r.w / BOX_SIZE as f32;
        let size = l.cell * 0.22;
        for digit in 1..=9u8 {
            if !cell.has_note(digit) {
                continue;
            }
            let slot = usize::from(digit).saturating_sub(1);
            let nrow = slot.wrapping_div(BOX_SIZE) as f32;
            let ncol = slot.wrapping_rem(BOX_SIZE) as f32;
            centred_in(
                f,
                Rect::new(r.x + ncol * third, r.y + nrow * third, third, third),
                &digit.to_string(),
                size,
                SUBTEXT0,
                FontWeightHint::Light,
            );
        }
    }

    fn draw_keypad(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.keypad) {
            return;
        }
        fill(f, l.keypad, MANTLE, 0.0);
        for (i, &target) in KEYPAD.iter().enumerate() {
            let r = l.key_rect(i);
            if r.is_empty() {
                continue;
            }
            let accent = match target {
                Target::Notes if self.note_mode => TEAL,
                Target::Notes => SUBTEXT0,
                Target::Hint if self.hints_remaining() == 0 => OVERLAY0,
                Target::Hint => PEACH,
                Target::Undo if self.undo_stack.is_empty() => OVERLAY0,
                Target::Redo if self.redo_stack.is_empty() => OVERLAY0,
                Target::Undo | Target::Redo => MAUVE,
                Target::Erase => SUBTEXT0,
                _ => TEXT_COLOR,
            };
            chip(f, r, target, &key_label(target), l.font, accent);
        }
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.footer) {
            return;
        }
        fill(f, l.footer, MANTLE, 0.0);
        let best = self
            .stats
            .best_time(self.difficulty)
            .map_or_else(|| "--:--".to_string(), format_time);
        let counts = Difficulty::ALL
            .iter()
            .map(|&d| format!("{} {}", d.label(), self.stats.games_completed(d)))
            .collect::<Vec<_>>()
            .join("  ");
        left_in(
            f,
            Rect::new(
                l.footer.x + l.pad,
                l.footer.y,
                (l.footer.w - l.pad * 2.0).max(0.0),
                l.footer.h,
            ),
            &format!(
                "Solved {}  ({counts})   Best {best}   {}",
                self.stats.total_completed(),
                if self.is_valid() {
                    "no conflicts"
                } else {
                    "conflicts"
                }
            ),
            l.font,
            OVERLAY0,
            FontWeightHint::Regular,
        );
    }
}

/// Route one event to the model. The single door every input comes through, so
/// a test drives the program the way a player does.
pub fn handle_event(app: &mut SudokuApp, event: &Event) -> EventResult {
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

impl App for SudokuApp {
    fn title(&self) -> String {
        "Sudoku".to_string()
    }

    fn app_id(&self) -> String {
        "sudoku".to_string()
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
    /// is something for it to move.
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

impl Probe for SudokuApp {
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
    let mut app = SudokuApp::new();
    app::launch("sudoku", &mut app)
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

    /// A finished board, written out here rather than generated.
    ///
    /// The generator is the most expensive thing this program does — a hard
    /// puzzle is eighty-one uniqueness searches — and almost no test about
    /// *playing* needs a carved puzzle. A literal board makes those tests
    /// free, and it also makes them repeatable: a test that asked the
    /// generator for a board would be asserting against whatever that seed
    /// happened to produce, which is a fact about the generator and not about
    /// the rule under test.
    ///
    /// [`the_written_out_solution_really_is_a_finished_sudoku`] is what stops
    /// this being a fixture nobody checked.
    const KNOWN_SOLUTION: [u8; TOTAL_CELLS] = [
        5, 3, 4, 6, 7, 8, 9, 1, 2, //
        6, 7, 2, 1, 9, 5, 3, 4, 8, //
        1, 9, 8, 3, 4, 2, 5, 6, 7, //
        8, 5, 9, 7, 6, 1, 4, 2, 3, //
        4, 2, 6, 8, 5, 3, 7, 9, 1, //
        7, 1, 3, 9, 2, 4, 8, 5, 6, //
        9, 6, 1, 5, 3, 7, 2, 8, 4, //
        2, 8, 7, 4, 1, 9, 6, 3, 5, //
        3, 4, 5, 2, 8, 6, 1, 7, 9, //
    ];

    /// [`KNOWN_SOLUTION`] with the named cells emptied, as a playable game.
    fn board(blanks: &[usize]) -> SudokuApp {
        let cells = core::array::from_fn(|i| {
            if blanks.contains(&i) {
                Cell::empty()
            } else {
                Cell::as_given(KNOWN_SOLUTION[i])
            }
        });
        SudokuApp::from_puzzle(cells, KNOWN_SOLUTION, Difficulty::Easy)
    }

    /// A game with room to play in.
    ///
    /// Three holes, not one. A fixture one digit short of finished turns every
    /// test that writes a digit into a test about a *won* game: the write
    /// completes the board, the status becomes `Won`, and every intent after it
    /// is refused for a reason the test was not asking about. Half a dozen
    /// tests here passed against the wrong rule before the count went up.
    fn playground() -> SudokuApp {
        board(&[idx(1, 7), idx(7, 1), idx(4, 0)])
    }

    /// A game one digit short of finished, selected on the hole.
    fn almost_done() -> SudokuApp {
        let mut a = board(&[80]);
        a.apply(Intent::Select(8, 8));
        a
    }

    /// A generated game. Expensive — only for the tests whose subject is the
    /// generator.
    fn generated(seed: u64, d: Difficulty) -> SudokuApp {
        SudokuApp::with_seed_and_difficulty(seed, d)
    }

    fn select(a: &mut SudokuApp, row: usize, col: usize) {
        a.apply(Intent::Select(row, col));
        assert_eq!(a.selected(), (row, col), "the selection did not move");
    }

    fn tick(a: &mut SudokuApp, ms: u64) -> EventResult {
        handle_event(a, &Event::Tick { elapsed_ms: ms })
    }

    fn key(a: &mut SudokuApp, k: Key) -> EventResult {
        handle_event(a, &Event::Key(press(k)))
    }

    fn mouse(a: &mut SudokuApp, x: f32, y: f32, kind: MouseEventKind) -> EventResult {
        handle_event(a, &Event::Mouse(MouseEvent { x, y, kind }))
    }

    fn layout_of(a: &SudokuApp) -> Layout {
        Layout::new(a.size().0, a.size().1)
    }

    /// The digit key for `d`.
    fn digit_key(d: u8) -> Key {
        match d {
            1 => Key::Num1,
            2 => Key::Num2,
            3 => Key::Num3,
            4 => Key::Num4,
            5 => Key::Num5,
            6 => Key::Num6,
            7 => Key::Num7,
            8 => Key::Num8,
            9 => Key::Num9,
            _ => panic!("{d} is not a digit this game has"),
        }
    }

    /// Walk the selection to `(row, col)`, and give up rather than spin.
    ///
    /// The bound is the point. Written the obvious way — `while a.selected()
    /// != (row, col)` — this loop never returns against any program whose
    /// selection has stopped moving, and a step that stays put, an up that
    /// goes down and a move that reports it moved without moving are all
    /// mutations a sweep will make. Every one of them would turn into the same
    /// four-minute hang, which names no test and tells nobody which fault it
    /// found. That is `known-issues.md` lesson 55.
    ///
    /// Two rows plus two columns of slack is more than any route this walk can
    /// take, since it closes one axis at a time and each step closes it by one.
    fn walk_selection(
        a: &mut SudokuApp,
        row: usize,
        col: usize,
        mut step: impl FnMut(&mut SudokuApp, Dir),
    ) {
        let bound = GRID_SIZE * 2 + 4;
        for _ in 0..bound {
            let (sr, sc) = a.selected();
            if (sr, sc) == (row, col) {
                return;
            }
            if sr < row {
                step(a, Dir::Down);
            } else if sr > row {
                step(a, Dir::Up);
            } else if sc < col {
                step(a, Dir::Right);
            } else {
                step(a, Dir::Left);
            }
        }
        panic!(
            "the selection did not reach {row},{col} in {bound} moves — it is \
             at {:?}, so a move is not moving where it says it does",
            a.selected()
        );
    }

    /// Walk the selection with the arrow keys, through the whole event path.
    fn walk_selection_with_keys(a: &mut SudokuApp, row: usize, col: usize) {
        walk_selection(a, row, col, |a, d| {
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

    fn cell_point(a: &SudokuApp, row: usize, col: usize) -> (f32, f32) {
        let r = layout_of(a).cell_rect(row, col);
        assert!(!r.is_empty(), "cell {row},{col} is not drawn");
        r.centre()
    }

    /// Click a cell the way a player does — through the hit box the drawing
    /// pass recorded, not through `apply`.
    fn click_cell(a: &mut SudokuApp, row: usize, col: usize) -> EventResult {
        let (x, y) = cell_point(a, row, col);
        mouse(a, x, y, MouseEventKind::Press(MouseButton::Left))
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

    /// The text commands whose start lies inside `r`.
    fn texts_in(f: &Frame, r: Rect) -> Vec<String> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, text, .. } if r.contains(*x, *y) => Some(text.clone()),
                _ => None,
            })
            .collect()
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

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    // ── The fixture itself ─────────────────────────────────────────────────

    #[test]
    fn the_written_out_solution_really_is_a_finished_sudoku() {
        // Half the suite is built on this array. A fixture nobody checked is a
        // fixture that can quietly stop being what its name says.
        assert!(
            is_grid_complete(&KNOWN_SOLUTION),
            "the fixture board is not a finished sudoku"
        );
        let mut tally = [0usize; GRID_SIZE];
        for &digit in &KNOWN_SOLUTION {
            let slot = note_slot(digit).expect("the fixture holds something that is not a digit");
            tally[slot] += 1;
        }
        for (slot, &seen) in tally.iter().enumerate() {
            assert_eq!(seen, GRID_SIZE, "{} does not appear nine times", slot + 1);
        }
    }

    // ── Difficulty ─────────────────────────────────────────────────────────

    #[test]
    fn a_harder_level_asks_for_fewer_clues_than_an_easier_one() {
        // Written out rather than derived: these are the numbers that make one
        // level harder than another, and a test that asked the program what
        // its own numbers were could not notice one being changed.
        let want = [
            (Difficulty::Easy, 35, 40),
            (Difficulty::Medium, 28, 34),
            (Difficulty::Hard, 22, 27),
        ];
        for (d, lo, hi) in want {
            assert_eq!(d.givens_range(), (lo, hi), "{}", d.label());
        }
        let mut prev: Option<Difficulty> = None;
        for d in Difficulty::ALL {
            if let Some(p) = prev {
                assert!(
                    d.givens_range().1 < p.givens_range().0,
                    "{} is not harder than {}",
                    d.label(),
                    p.label()
                );
            }
            assert!(
                d.givens_range().0 <= d.givens_range().1,
                "{} wants a range that runs backwards",
                d.label()
            );
            assert!(
                d.givens_range().1 < TOTAL_CELLS,
                "{} would leave nothing to solve",
                d.label()
            );
            prev = Some(d);
        }
    }

    #[test]
    fn cycling_difficulty_visits_all_three_and_comes_home() {
        // The order matters, not just the set: a cycle that runs backwards
        // closes and visits every level too, so "closes and visits all three"
        // passes against a chip that steps the wrong way. Name the steps.
        assert_eq!(Difficulty::Easy.next(), Difficulty::Medium);
        assert_eq!(Difficulty::Medium.next(), Difficulty::Hard);
        assert_eq!(Difficulty::Hard.next(), Difficulty::Easy, "the cycle broke");

        let mut d = Difficulty::Easy;
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

    // ── Cells ──────────────────────────────────────────────────────────────

    #[test]
    fn an_empty_cell_holds_no_digit_no_marks_and_belongs_to_the_player() {
        let c = Cell::empty();
        assert!(c.is_empty(), "an empty cell holds a digit");
        assert!(!c.fixed(), "an empty cell cannot be edited");
        assert!(!c.has_any_note(), "an empty cell came with marks");
        assert_eq!(c.origin, Origin::Player);
        assert_eq!(c.value, 0);
    }

    #[test]
    fn a_clue_and_a_hint_are_both_locked_but_only_one_is_a_clue() {
        // The whole reason `Origin` exists. A single `given: bool` answered
        // "is this one of the puzzle's clues" and "may I edit this" with one
        // bit, so locking a hinted cell made it a clue.
        let clue = Cell::as_given(7);
        assert_eq!(clue.origin, Origin::Given);
        assert!(clue.fixed(), "a clue can be typed over");
        assert!(!clue.is_empty());

        let mut hinted = Cell::empty();
        hinted.origin = Origin::Hint;
        hinted.value = 7;
        assert!(hinted.fixed(), "a hinted cell can be typed over");
        assert_ne!(hinted.origin, Origin::Given, "a hint became a clue");

        let mut mine = Cell::empty();
        mine.value = 7;
        assert!(!mine.fixed(), "the player's own digit is locked");
    }

    #[test]
    fn a_mark_goes_on_and_comes_off_again() {
        let mut c = Cell::empty();
        assert!(!c.has_note(4));
        c.toggle_note(4);
        assert!(c.has_note(4), "the mark did not go on");
        assert!(c.has_any_note());
        assert!(c.is_empty(), "a mark is not a digit");
        c.toggle_note(4);
        assert!(!c.has_note(4), "the mark did not come off");
        assert!(!c.has_any_note());
    }

    #[test]
    fn a_mark_for_something_that_is_not_a_digit_is_ignored() {
        let mut c = Cell::empty();
        for bad in [0u8, 10, 11, 200, 255] {
            c.toggle_note(bad);
            assert!(!c.has_note(bad), "{bad} got a mark");
            assert!(!c.has_any_note(), "{bad} marked something");
        }
    }

    #[test]
    fn every_digit_has_its_own_mark() {
        let mut c = Cell::empty();
        for d in 1..=9u8 {
            c.toggle_note(d);
        }
        for d in 1..=9u8 {
            assert!(c.has_note(d), "the mark for {d} is missing");
        }
        assert_eq!(c.notes.iter().filter(|&&n| n).count(), GRID_SIZE);
    }

    #[test]
    fn clearing_the_marks_clears_all_nine() {
        let mut c = Cell::empty();
        for d in 1..=9u8 {
            c.toggle_note(d);
        }
        c.clear_notes();
        assert!(!c.has_any_note(), "a mark survived");
    }

    #[test]
    fn a_marks_slot_is_the_digit_less_one_and_nothing_else_has_a_slot() {
        for d in 1..=9u8 {
            assert_eq!(note_slot(d), Some(usize::from(d) - 1), "digit {d}");
        }
        for bad in [0u8, 10, 255] {
            assert_eq!(note_slot(bad), None, "{bad} was given a slot");
        }
    }

    // ── Grid arithmetic ────────────────────────────────────────────────────

    #[test]
    fn an_index_and_a_row_and_column_name_the_same_cell() {
        for i in 0..TOTAL_CELLS {
            let (r, c) = row_col(i);
            assert!(r < GRID_SIZE && c < GRID_SIZE, "index {i} is off the board");
            assert_eq!(idx(r, c), i, "index {i} did not round-trip");
        }
        // Deliberately different row and column, so a transposed pair of
        // arithmetic could not pass by symmetry. See `known-issues.md` 54.
        assert_eq!(idx(1, 7), 16);
        assert_eq!(row_col(16), (1, 7));
        assert_eq!(idx(7, 1), 64);
        assert_eq!(row_col(64), (7, 1));
    }

    #[test]
    fn a_box_origin_is_the_top_left_of_the_three_by_three_it_belongs_to() {
        assert_eq!(box_origin(0, 0), (0, 0));
        assert_eq!(box_origin(2, 2), (0, 0));
        assert_eq!(box_origin(1, 7), (0, 6));
        assert_eq!(box_origin(7, 1), (6, 0));
        assert_eq!(box_origin(8, 8), (6, 6));
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let (br, bc) = box_origin(r, c);
                assert!(br <= r && r < br + BOX_SIZE, "{r},{c} is outside its box");
                assert!(bc <= c && c < bc + BOX_SIZE, "{r},{c} is outside its box");
                assert_eq!(br % BOX_SIZE, 0, "{r},{c} box row is not a multiple");
                assert_eq!(bc % BOX_SIZE, 0, "{r},{c} box column is not a multiple");
            }
        }
    }

    #[test]
    fn a_square_off_the_board_reads_as_empty_rather_than_wrapping_round() {
        assert_eq!(at(&KNOWN_SOLUTION, 0, 0), 5);
        assert_eq!(at(&KNOWN_SOLUTION, 1, 7), 4);
        assert_eq!(at(&KNOWN_SOLUTION, 7, 1), 8);
        assert_eq!(at(&KNOWN_SOLUTION, 9, 0), 0, "row 9 read something");
        assert_eq!(at(&KNOWN_SOLUTION, 0, 9), 0, "column 9 wrapped to row 1");
        assert_eq!(at(&KNOWN_SOLUTION, 100, 100), 0);
    }

    #[test]
    fn a_write_off_the_board_lands_nowhere_rather_than_on_the_next_row() {
        let mut g = [0u8; TOTAL_CELLS];
        put(&mut g, 0, 9, 5);
        assert_eq!(
            g, [0u8; TOTAL_CELLS],
            "a write past column 8 landed on row 1"
        );
        put(&mut g, 9, 0, 5);
        assert_eq!(g, [0u8; TOTAL_CELLS], "a write past row 8 landed somewhere");
        put(&mut g, 1, 7, 5);
        assert_eq!(at(&g, 1, 7), 5, "a write on the board landed nowhere");
    }

    #[test]
    fn a_square_off_the_board_cannot_be_read_or_written_through_the_model() {
        let mut a = playground();
        assert_eq!(a.value(0, 9), 0, "column 9 read the next row");
        assert_eq!(a.value(9, 0), 0);
        assert!(a.cell(0, 9).is_empty());
        assert_eq!(a.solution_at(0, 9), 0);

        let before = values_array(&a.cells);
        a.write(0, 9, |c| c.value = 5);
        assert_eq!(
            values_array(&a.cells),
            before,
            "a write to column 9 changed row 1"
        );
    }

    #[test]
    fn a_digit_repeated_in_a_row_a_column_or_a_box_is_a_conflict() {
        let mut g = [0u8; TOTAL_CELLS];
        put(&mut g, 4, 4, 5);
        assert!(has_conflict(&g, 4, 0, 5), "the row did not notice");
        assert!(has_conflict(&g, 0, 4, 5), "the column did not notice");
        assert!(has_conflict(&g, 3, 3, 5), "the box did not notice");
        assert!(!has_conflict(&g, 0, 0, 5), "a far-off cell was a conflict");
        assert!(!has_conflict(&g, 4, 0, 6), "another digit was a conflict");
    }

    #[test]
    fn a_cell_is_never_its_own_conflict() {
        // Every one of the three scans skips the cell itself, so the answer is
        // the same for a digit already written there as for one about to be.
        for (i, &digit) in KNOWN_SOLUTION.iter().enumerate() {
            let (r, c) = row_col(i);
            assert!(
                !has_conflict(&KNOWN_SOLUTION, r, c, digit),
                "cell {r},{c} conflicts with itself"
            );
        }
    }

    #[test]
    fn an_empty_square_is_never_a_conflict() {
        let g = [0u8; TOTAL_CELLS];
        assert!(
            !has_conflict(&g, 0, 0, 0),
            "nothing conflicted with nothing"
        );
        assert!(is_grid_valid(&g), "an empty grid broke a rule");
        assert!(!is_grid_complete(&g), "an empty grid was finished");
    }

    #[test]
    fn a_grid_with_a_hole_is_valid_but_not_complete() {
        let mut g = KNOWN_SOLUTION;
        assert!(is_grid_complete(&g));
        g[40] = 0;
        assert!(is_grid_valid(&g), "a hole is not a broken rule");
        assert!(!is_grid_complete(&g), "a grid with a hole was finished");
    }

    #[test]
    fn a_full_grid_that_repeats_a_digit_is_not_complete() {
        // Filled is not finished. A program that only counted the holes would
        // call this a win.
        let mut g = KNOWN_SOLUTION;
        g[1] = g[0];
        assert!(!g.contains(&0), "the fixture left a hole");
        assert!(!is_grid_valid(&g), "a repeated digit was allowed");
        assert!(!is_grid_complete(&g), "a broken grid was finished");
    }

    #[test]
    fn the_digits_of_a_board_are_its_digits_and_not_its_marks() {
        let mut cells: [Cell; TOTAL_CELLS] = core::array::from_fn(|_| Cell::empty());
        cells[16].value = 4;
        cells[64].toggle_note(9);
        let vals = values_array(&cells);
        assert_eq!(vals[16], 4, "the digit was lost");
        assert_eq!(vals[64], 0, "a mark was read as a digit");
        assert_eq!(vals.iter().filter(|&&v| v != 0).count(), 1);
    }

    // ── Solver ─────────────────────────────────────────────────────────────

    #[test]
    fn the_candidates_are_the_digits_that_would_not_break_a_rule() {
        let mut g = [0u8; TOTAL_CELLS];
        assert_eq!(candidates(&g, 0, 0), 0x1FF, "an empty grid forbids nothing");
        // Each witness must lie *outside* cell (0, 0)'s own 3x3 box, or the box
        // scan strikes it out by itself and the row and column scans are never
        // tested at all. (0, 1) and (1, 0) read like a row witness and a column
        // witness but are both inside that box, and the mutation sweep caught
        // it: either scan could be deleted with this test still green, and the
        // messages below were saying "the row was ignored" about the box.
        put(&mut g, 0, 5, 5);
        assert!(
            !is_candidate(candidates(&g, 0, 0), 5),
            "the row was ignored"
        );
        put(&mut g, 5, 0, 6);
        assert!(
            !is_candidate(candidates(&g, 0, 0), 6),
            "the column was ignored"
        );
        put(&mut g, 2, 2, 7);
        assert!(
            !is_candidate(candidates(&g, 0, 0), 7),
            "the box was ignored"
        );
        for d in [1u8, 2, 3, 4, 8, 9] {
            assert!(is_candidate(candidates(&g, 0, 0), d), "{d} was struck out");
        }
        assert_eq!(candidates(&g, 0, 0).count_ones(), 6);
    }

    #[test]
    fn a_cell_with_eight_of_its_nine_digits_around_it_has_one_candidate() {
        let mut g = KNOWN_SOLUTION;
        g[0] = 0;
        let mask = candidates(&g, 0, 0);
        assert_eq!(mask.count_ones(), 1, "the hole had more than one answer");
        assert!(
            is_candidate(mask, KNOWN_SOLUTION[0]),
            "the answer was excluded"
        );
    }

    #[test]
    fn nothing_at_all_can_go_in_a_cell_whose_row_and_column_use_every_digit() {
        let mut g = [0u8; TOTAL_CELLS];
        for c in 1..GRID_SIZE {
            put(&mut g, 0, c, c as u8);
        }
        put(&mut g, 1, 0, 9);
        assert_eq!(candidates(&g, 0, 0), 0, "a dead cell had a candidate");
        assert!(!solve(&mut g), "an impossible grid was solved");
        assert_eq!(
            count_solutions(&mut g, 2),
            0,
            "an impossible grid was counted"
        );
    }

    #[test]
    fn the_next_cell_to_try_is_the_one_with_the_fewest_answers_not_the_first_one() {
        // This is what took generation from 1.2 seconds a puzzle to a tenth of
        // that, and it is a real difference and not a tidying-up: the first
        // empty cell here has two answers and the last has one.
        let mut g = KNOWN_SOLUTION;
        for slot in g.iter_mut().take(GRID_SIZE * 2) {
            *slot = 0;
        }
        g[80] = 0;
        assert_eq!(
            find_empty(&g),
            Some((0, 0)),
            "reading order starts elsewhere"
        );
        assert_eq!(candidates(&g, 0, 0).count_ones(), 2, "the fixture is wrong");
        let (r, c, mask) = next_cell(&g).expect("a grid with holes has a next cell");
        assert_eq!(
            (r, c),
            (8, 8),
            "the search took the first hole, not the tightest"
        );
        assert_eq!(mask.count_ones(), 1);
        assert!(is_candidate(mask, KNOWN_SOLUTION[80]));
    }

    #[test]
    fn a_finished_grid_has_no_next_cell_and_no_empty_one() {
        assert_eq!(next_cell(&KNOWN_SOLUTION), None, "a full grid had a hole");
        assert_eq!(find_empty(&KNOWN_SOLUTION), None);
    }

    #[test]
    fn the_first_empty_square_is_found_in_reading_order() {
        let mut g = KNOWN_SOLUTION;
        g[40] = 0;
        g[5] = 0;
        assert_eq!(
            find_empty(&g),
            Some((0, 5)),
            "the scan is not left to right"
        );
    }

    #[test]
    fn the_solver_finishes_a_grid_it_can_finish() {
        let mut g = KNOWN_SOLUTION;
        for i in [0usize, 16, 40, 64, 80] {
            g[i] = 0;
        }
        assert!(solve(&mut g), "a solvable grid was refused");
        assert_eq!(g, KNOWN_SOLUTION, "the grid was finished the wrong way");
    }

    #[test]
    fn the_solver_takes_back_a_guess_that_led_nowhere() {
        // Every other solver fixture here is filled by first guesses alone, so
        // the `put(grid, row, col, 0)` that undoes a failed guess never ran in
        // this suite and could be deleted with the whole file still green.
        //
        // These fourteen holes are the smallest set found that forces the
        // solver to walk into a contradiction and back out of it: it has one
        // answer, the solver needs three undos to reach it, and a solver that
        // leaves its wrong guesses behind does not merely finish differently --
        // it gives up, because the abandoned digits block every later cell.
        const HOLES: [usize; 14] = [31, 32, 36, 37, 39, 46, 50, 63, 64, 66, 67, 73, 76, 77];
        let mut g = KNOWN_SOLUTION;
        for i in HOLES {
            g[i] = 0;
        }
        let mut probe = g;
        assert_eq!(
            count_solutions(&mut probe, 2),
            1,
            "the fixture stopped having exactly one answer"
        );
        assert!(solve(&mut g), "the solver gave up on a grid it can finish");
        assert_eq!(g, KNOWN_SOLUTION, "a wrong guess was left on the board");
    }

    #[test]
    fn the_solver_fills_an_empty_grid() {
        let mut g = [0u8; TOTAL_CELLS];
        assert!(solve(&mut g), "an empty grid could not be filled");
        assert!(is_grid_complete(&g), "the filled grid breaks a rule");
    }

    #[test]
    fn a_grid_with_one_hole_has_exactly_one_answer() {
        let mut g = KNOWN_SOLUTION;
        g[40] = 0;
        assert_eq!(count_solutions(&mut g, 2), 1);
        assert_eq!(g[40], 0, "the count left its working behind");
    }

    #[test]
    fn a_grid_with_room_to_guess_has_more_than_one_answer() {
        let mut g = [0u8; TOTAL_CELLS];
        assert_eq!(
            count_solutions(&mut g, 2),
            2,
            "an empty grid had one answer"
        );
    }

    #[test]
    fn counting_answers_stops_at_the_limit_it_was_given() {
        // The limit is what makes the generator affordable: it never asks for
        // more than "is there a second one".
        let mut g = [0u8; TOTAL_CELLS];
        assert_eq!(count_solutions(&mut g, 1), 1);
        assert_eq!(count_solutions(&mut g, 0), 0, "a limit of none counted one");
    }

    #[test]
    fn a_shuffled_solve_still_produces_a_finished_grid() {
        for seed in [1u64, 2, 99, 123_456] {
            let mut rng = SeededRng::new(seed);
            let mut g = [0u8; TOTAL_CELLS];
            assert!(
                solve_shuffled(&mut g, &mut rng),
                "seed {seed} could not fill"
            );
            assert!(is_grid_complete(&g), "seed {seed} broke a rule");
        }
    }

    #[test]
    fn one_seed_is_one_grid_and_two_seeds_are_two() {
        let grid = |seed: u64| {
            let mut rng = SeededRng::new(seed);
            generate_full_grid(&mut rng)
        };
        assert_eq!(grid(7), grid(7), "the same seed gave two grids");
        assert_ne!(grid(7), grid(8), "two seeds gave the same grid");
        // The plain solver always tries 1 first, so it always produces the
        // same completion. That the shuffled one does not is the whole reason
        // it exists.
        let mut plain = [0u8; TOTAL_CELLS];
        solve(&mut plain);
        assert_ne!(grid(7), plain, "the shuffle did not shuffle");
    }

    // ── Generation ─────────────────────────────────────────────────────────

    #[test]
    fn a_generated_puzzle_is_its_solution_with_clues_taken_out() {
        let mut rng = SeededRng::new(2024);
        let (cells, solution) = generate_puzzle(&mut rng, Difficulty::Easy);
        assert!(is_grid_complete(&solution), "the solution is not a sudoku");
        for (i, &cell) in cells.iter().enumerate() {
            if cell.is_empty() {
                assert_eq!(cell.origin, Origin::Player, "a hole was not the player's");
                assert!(!cell.fixed(), "a hole was locked");
            } else {
                assert_eq!(cell.value, solution[i], "clue {i} is not the answer");
                assert_eq!(cell.origin, Origin::Given, "a clue was not a clue");
            }
            assert!(!cell.has_any_note(), "a fresh puzzle came with marks");
        }
    }

    #[test]
    fn a_generated_puzzle_has_exactly_one_answer() {
        // The property that makes a puzzle solvable by reasoning rather than
        // by guessing, and the reason the generator is expensive at all.
        let mut rng = SeededRng::new(31);
        let (cells, _) = generate_puzzle(&mut rng, Difficulty::Medium);
        let mut grid = values_array(&cells);
        assert_eq!(count_solutions(&mut grid, 2), 1, "the puzzle is ambiguous");
    }

    #[test]
    fn a_generated_puzzle_keeps_at_least_the_clues_its_level_asks_for() {
        // Only the floor is a promise. The ceiling is not: a removal that
        // would have left two answers is put back, so a hard puzzle can come
        // out with more clues than its range names. That is uniqueness winning
        // over count, which is the right way round.
        for d in Difficulty::ALL {
            let mut rng = SeededRng::new(5);
            let (cells, _) = generate_puzzle(&mut rng, d);
            let clues = cells.iter().filter(|c| !c.is_empty()).count();
            assert!(
                clues >= d.givens_range().0,
                "{} kept only {clues} clues",
                d.label()
            );
            assert!(clues < TOTAL_CELLS, "{} left nothing to solve", d.label());
        }
    }

    // ── Statistics ─────────────────────────────────────────────────────────

    #[test]
    fn a_fresh_scoreboard_is_empty() {
        let s = Stats::default();
        for d in Difficulty::ALL {
            assert_eq!(s.games_completed(d), 0, "{}", d.label());
            assert_eq!(s.best_time(d), None, "{}", d.label());
        }
        assert_eq!(s.total_completed(), 0);
    }

    #[test]
    fn a_finish_is_counted_against_its_own_level_and_nobody_elses() {
        let mut s = Stats::default();
        s.record_completion(Difficulty::Medium, 300);
        assert_eq!(s.games_completed(Difficulty::Medium), 1);
        assert_eq!(s.games_completed(Difficulty::Easy), 0, "Easy was credited");
        assert_eq!(s.games_completed(Difficulty::Hard), 0, "Hard was credited");
        assert_eq!(s.best_time(Difficulty::Medium), Some(300));
        assert_eq!(s.best_time(Difficulty::Easy), None);
    }

    #[test]
    fn a_slower_finish_never_replaces_a_faster_one() {
        let mut s = Stats::default();
        s.record_completion(Difficulty::Hard, 200);
        s.record_completion(Difficulty::Hard, 500);
        assert_eq!(
            s.best_time(Difficulty::Hard),
            Some(200),
            "the best got worse"
        );
        assert_eq!(s.games_completed(Difficulty::Hard), 2, "a finish was lost");
        s.record_completion(Difficulty::Hard, 100);
        assert_eq!(
            s.best_time(Difficulty::Hard),
            Some(100),
            "a record was refused"
        );
    }

    #[test]
    fn the_total_is_every_level_added_up() {
        let mut s = Stats::default();
        s.record_completion(Difficulty::Easy, 1);
        s.record_completion(Difficulty::Medium, 2);
        s.record_completion(Difficulty::Medium, 3);
        s.record_completion(Difficulty::Hard, 4);
        assert_eq!(s.total_completed(), 4);
        assert_eq!(s.games_completed(Difficulty::Medium), 2);
    }

    #[test]
    fn the_three_levels_keep_three_separate_slots() {
        let slots: HashSet<usize> = Difficulty::ALL.iter().map(|&d| stat_slot(d)).collect();
        assert_eq!(
            slots.len(),
            Difficulty::ALL.len(),
            "two levels share a slot"
        );
    }

    // ── More helpers, for the drawing tests ────────────────────────────────

    /// The one text command drawn inside `r`, with its colour and weight.
    ///
    /// `text_color` cannot answer a question about a cell: nine cells hold a
    /// `5`, so looking a `5` up by its string finds whichever one was painted
    /// first. A cell's digit has to be found by where it is, not by what it
    /// says.
    fn text_in(f: &Frame, r: Rect) -> Option<(String, Color, FontWeightHint)> {
        f.commands().iter().find_map(|c| match c {
            RenderCommand::Text {
                x,
                y,
                text,
                color,
                font_weight,
                ..
            } if r.contains(*x, *y) => Some((text.clone(), *color, *font_weight)),
            _ => None,
        })
    }

    // ── Starting a game ────────────────────────────────────────────────────

    #[test]
    fn a_fresh_game_starts_playing_with_the_clock_at_nothing() {
        let a = SudokuApp::new();
        assert_eq!(a.status(), GameStatus::Playing);
        assert_eq!(a.elapsed_ms(), 0);
        assert_eq!(a.elapsed_secs(), 0);
        assert_eq!(a.difficulty(), Difficulty::Easy);
        assert_eq!(a.selected(), (4, 4), "a fresh game starts in the middle");
        assert_eq!(a.undo_depth(), 0);
        assert_eq!(a.redo_depth(), 0);
        assert_eq!(a.hints_remaining(), MAX_HINTS);
        assert!(a.is_valid(), "a fresh puzzle contradicts itself");
    }

    #[test]
    fn a_named_seed_names_a_named_puzzle() {
        let a = generated(7, Difficulty::Medium);
        let b = generated(7, Difficulty::Medium);
        let c = generated(8, Difficulty::Medium);
        assert_eq!(values_array(&a.cells), values_array(&b.cells));
        assert_ne!(
            values_array(&a.cells),
            values_array(&c.cells),
            "two seeds produced one puzzle"
        );
        assert_eq!(a.difficulty(), Difficulty::Medium);
    }

    #[test]
    fn a_new_game_keeps_the_statistics_and_throws_away_everything_else() {
        let mut a = almost_done();
        a.tick(5_000);
        assert_eq!(a.elapsed_secs(), 5);
        a.apply(Intent::Digit(9));
        assert_eq!(a.status(), GameStatus::Won, "the fixture did not finish");
        let solved = a.stats().total_completed();
        assert_eq!(solved, 1);

        a.new_game(Difficulty::Hard);

        assert_eq!(
            a.stats().total_completed(),
            solved,
            "a new deal wiped the scoreboard"
        );
        assert_eq!(a.difficulty(), Difficulty::Hard);
        assert_eq!(a.status(), GameStatus::Playing);
        assert_eq!(a.elapsed_ms(), 0, "the clock carried over");
        assert_eq!(a.selected(), (4, 4));
        assert_eq!(a.undo_depth(), 0);
        assert_eq!(a.redo_depth(), 0);
        assert_eq!(a.hints_remaining(), MAX_HINTS);
    }

    // ── Selection ──────────────────────────────────────────────────────────

    #[test]
    fn a_square_off_the_board_cannot_be_selected() {
        let mut a = board(&[0]);
        assert_eq!(a.apply(Intent::Select(9, 0)), EventResult::Ignored);
        assert_eq!(a.apply(Intent::Select(0, 9)), EventResult::Ignored);
        assert_eq!(a.selected(), (4, 4), "the selection left the board");
    }

    #[test]
    fn selecting_the_square_already_selected_asks_for_no_repaint() {
        let mut a = board(&[0]);
        assert_eq!(a.apply(Intent::Select(2, 3)), EventResult::Consumed);
        assert_eq!(
            a.apply(Intent::Select(2, 3)),
            EventResult::Ignored,
            "a selection that did not move still asked to be redrawn"
        );
    }

    #[test]
    fn an_arrow_moves_the_selection_one_square_that_way() {
        let mut a = board(&[0]);
        select(&mut a, 4, 4);
        a.apply(Intent::Move(Dir::Up));
        assert_eq!(a.selected(), (3, 4), "up did not go up");
        a.apply(Intent::Move(Dir::Down));
        assert_eq!(a.selected(), (4, 4), "down did not go down");
        a.apply(Intent::Move(Dir::Left));
        assert_eq!(a.selected(), (4, 3), "left did not go left");
        a.apply(Intent::Move(Dir::Right));
        assert_eq!(a.selected(), (4, 4), "right did not go right");
    }

    #[test]
    fn the_selection_stops_at_the_edge_rather_than_wrapping_round() {
        let mut a = board(&[0]);
        select(&mut a, 0, 0);
        assert_eq!(a.apply(Intent::Move(Dir::Up)), EventResult::Ignored);
        assert_eq!(a.apply(Intent::Move(Dir::Left)), EventResult::Ignored);
        assert_eq!(a.selected(), (0, 0), "the top left corner wrapped round");

        select(&mut a, 8, 8);
        assert_eq!(a.apply(Intent::Move(Dir::Down)), EventResult::Ignored);
        assert_eq!(a.apply(Intent::Move(Dir::Right)), EventResult::Ignored);
        assert_eq!(
            a.selected(),
            (8, 8),
            "the bottom right corner wrapped round"
        );
    }

    // ── Writing a digit ────────────────────────────────────────────────────

    #[test]
    fn a_digit_goes_into_the_selected_square_as_the_players_own() {
        let mut a = playground();
        select(&mut a, 1, 7);
        assert_eq!(a.apply(Intent::Digit(4)), EventResult::Consumed);
        assert_eq!(a.value(1, 7), 4);
        assert_eq!(a.cell(1, 7).origin, Origin::Player);
        assert!(!a.cell(1, 7).fixed(), "the player's own digit was locked");
    }

    #[test]
    fn a_clue_cannot_be_written_over() {
        let mut a = board(&[80]);
        select(&mut a, 1, 7);
        // The digit must differ from the clue's own. Writing a 4 over a clue
        // that already holds a 4 is refused as a write of the digit that is
        // already there, not as a write over a clue -- the sweep caught this
        // test passing with the clue guard deleted for exactly that reason.
        let clue = KNOWN_SOLUTION[idx(1, 7)];
        let other = if clue == 9 { 1 } else { clue.saturating_add(1) };
        assert_eq!(
            a.apply(Intent::Digit(other)),
            EventResult::Ignored,
            "a clue accepted a digit"
        );
        assert_eq!(a.value(1, 7), clue);
        assert_eq!(a.undo_depth(), 0, "a refused write went on the history");
    }

    #[test]
    fn writing_the_digit_that_is_already_there_asks_for_no_repaint() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Digit(4));
        assert_eq!(
            a.apply(Intent::Digit(4)),
            EventResult::Ignored,
            "rewriting the same digit still asked to be redrawn"
        );
        assert_eq!(a.undo_depth(), 1, "a no-op went on the history");
    }

    #[test]
    fn a_number_that_is_not_a_sudoku_digit_is_refused() {
        let mut a = playground();
        select(&mut a, 1, 7);
        for bad in [0_u8, 10, 255] {
            assert_eq!(
                a.apply(Intent::Digit(bad)),
                EventResult::Ignored,
                "{bad} was accepted as a digit"
            );
        }
        assert_eq!(a.value(1, 7), 0);
    }

    #[test]
    fn writing_a_digit_sweeps_the_marks_away() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(3));
        a.apply(Intent::Digit(6));
        assert!(a.cell(1, 7).has_any_note(), "the marks were not written");
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(4));
        assert_eq!(a.value(1, 7), 4);
        assert!(
            !a.cell(1, 7).has_any_note(),
            "the marks survived the digit that answers them"
        );
    }

    // ── Erasing ────────────────────────────────────────────────────────────

    #[test]
    fn erasing_empties_the_square_the_player_filled() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Digit(4));
        assert_eq!(a.apply(Intent::Erase), EventResult::Consumed);
        assert_eq!(a.value(1, 7), 0);
        assert!(a.cell(1, 7).is_empty());
    }

    #[test]
    fn a_clue_cannot_be_erased() {
        let mut a = board(&[80]);
        select(&mut a, 1, 7);
        assert_eq!(a.apply(Intent::Erase), EventResult::Ignored);
        assert_eq!(a.value(1, 7), KNOWN_SOLUTION[idx(1, 7)]);
    }

    #[test]
    fn erasing_a_square_that_is_already_bare_asks_for_no_repaint() {
        let mut a = playground();
        select(&mut a, 1, 7);
        assert_eq!(
            a.apply(Intent::Erase),
            EventResult::Ignored,
            "erasing nothing still asked to be redrawn"
        );
        assert_eq!(a.undo_depth(), 0, "erasing nothing went on the history");
    }

    #[test]
    fn erasing_a_square_that_holds_only_marks_clears_the_marks() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(5));
        assert!(a.cell(1, 7).has_note(5));
        assert_eq!(
            a.apply(Intent::Erase),
            EventResult::Consumed,
            "a square holding marks but no digit was treated as bare"
        );
        assert!(!a.cell(1, 7).has_any_note());
    }

    // ── Pencil marks ───────────────────────────────────────────────────────

    #[test]
    fn in_note_mode_a_digit_becomes_a_mark_and_not_an_answer() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        assert!(a.note_mode());
        a.apply(Intent::Digit(4));
        assert_eq!(a.value(1, 7), 0, "a mark was written as an answer");
        assert!(a.cell(1, 7).has_note(4));
    }

    #[test]
    fn a_mark_written_twice_is_a_mark_rubbed_out() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(4));
        a.apply(Intent::Digit(4));
        assert!(!a.cell(1, 7).has_note(4), "the second mark did not rub out");
    }

    #[test]
    fn note_mode_goes_on_and_off_again() {
        let mut a = board(&[0]);
        assert!(!a.note_mode(), "a game started in note mode");
        assert_eq!(a.apply(Intent::ToggleNotes), EventResult::Consumed);
        assert!(a.note_mode());
        a.apply(Intent::ToggleNotes);
        assert!(!a.note_mode(), "note mode would not turn off");
    }

    #[test]
    fn a_clue_takes_no_marks() {
        let mut a = board(&[80]);
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        assert_eq!(a.apply(Intent::Digit(4)), EventResult::Ignored);
        assert!(!a.cell(1, 7).has_any_note(), "a clue was pencilled on");
    }

    // ── Taking it back ─────────────────────────────────────────────────────

    #[test]
    fn undo_takes_a_digit_back_and_redo_puts_it_again() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Digit(4));
        assert_eq!(a.undo_depth(), 1);

        assert_eq!(a.apply(Intent::Undo), EventResult::Consumed);
        assert_eq!(a.value(1, 7), 0, "undo did not take the digit back");
        assert_eq!(a.undo_depth(), 0);
        assert_eq!(a.redo_depth(), 1);

        assert_eq!(a.apply(Intent::Redo), EventResult::Consumed);
        assert_eq!(a.value(1, 7), 4, "redo did not put the digit back");
        assert_eq!(a.undo_depth(), 1);
        assert_eq!(a.redo_depth(), 0);
    }

    #[test]
    fn there_is_nothing_to_take_back_from_a_fresh_game() {
        let mut a = board(&[0]);
        assert_eq!(a.apply(Intent::Undo), EventResult::Ignored);
        assert_eq!(a.apply(Intent::Redo), EventResult::Ignored);
    }

    #[test]
    fn a_fresh_move_throws_away_the_moves_that_were_undone() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Digit(4));
        a.apply(Intent::Undo);
        assert_eq!(a.redo_depth(), 1);

        select(&mut a, 7, 1);
        a.apply(Intent::Digit(8));
        assert_eq!(
            a.redo_depth(),
            0,
            "a new move left the abandoned future in place"
        );
    }

    #[test]
    fn undoing_a_mark_is_rubbing_the_same_mark_out() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(4));
        a.apply(Intent::Undo);
        assert!(!a.cell(1, 7).has_note(4), "an undone mark stayed written");
        a.apply(Intent::Redo);
        assert!(a.cell(1, 7).has_note(4), "a redone mark stayed rubbed out");
    }

    #[test]
    fn undoing_a_hint_gives_the_hint_back() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Hint);
        assert_eq!(a.hints_remaining(), MAX_HINTS - 1);
        assert_eq!(a.cell(1, 7).origin, Origin::Hint);

        a.apply(Intent::Undo);
        assert_eq!(
            a.hints_remaining(),
            MAX_HINTS,
            "an undone hint was still spent"
        );
        assert_eq!(
            a.cell(1, 7).origin,
            Origin::Player,
            "an undone hint left the square locked"
        );
        assert_eq!(a.value(1, 7), 0);

        a.apply(Intent::Redo);
        assert_eq!(a.hints_remaining(), MAX_HINTS - 1, "a redone hint was free");
        assert_eq!(a.value(1, 7), KNOWN_SOLUTION[idx(1, 7)]);
    }

    #[test]
    fn undoing_a_digit_gives_back_the_marks_it_swept_away() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(3));
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(4));
        a.apply(Intent::Undo);
        assert!(
            a.cell(1, 7).has_note(3),
            "the marks the digit swept away did not come back"
        );
    }

    #[test]
    fn the_history_forgets_its_oldest_move_rather_than_growing_for_ever() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        // Marks toggle, so this is one square taking as many changes as we
        // like. An *odd* number of them, which is what makes the two ends of
        // the history tell apart: with an even number every change looks the
        // same as every other and dropping the newest is indistinguishable
        // from dropping the oldest, which is how the sweep found this test
        // passing against a history that forgot the wrong end.
        for _ in 0..=MAX_UNDO {
            a.apply(Intent::Digit(4));
        }
        assert_eq!(
            a.undo_depth(),
            MAX_UNDO,
            "the history grew past the cap it is supposed to keep"
        );
        assert!(a.cell(1, 7).has_note(4), "an odd count left the mark off");

        // One change more than the history holds was made, so walking the whole
        // history back cannot reach the bare square: the first toggle is the
        // one that fell off the end, and it stands. A history that dropped its
        // newest move instead would rewind all the way and clear the mark.
        for _ in 0..MAX_UNDO {
            a.apply(Intent::Undo);
        }
        assert_eq!(a.undo_depth(), 0, "the history would not empty");
        assert!(
            a.cell(1, 7).has_note(4),
            "the history forgot its newest move rather than its oldest"
        );
    }

    // ── Hints ──────────────────────────────────────────────────────────────

    #[test]
    fn a_hint_fills_the_square_from_the_solution_and_locks_it() {
        let mut a = playground();
        select(&mut a, 1, 7);
        assert_eq!(a.apply(Intent::Hint), EventResult::Consumed);
        assert_eq!(a.value(1, 7), a.solution_at(1, 7));
        assert_eq!(a.cell(1, 7).origin, Origin::Hint);
        assert!(a.cell(1, 7).fixed(), "a hint can be typed over");
    }

    #[test]
    fn the_hints_run_out() {
        let mut a = board(&[0, 1, 2, 3, 4, 5, 6]);
        for (n, i) in [0_usize, 1, 2, 3, 4, 5].into_iter().enumerate() {
            let (r, c) = row_col(i);
            select(&mut a, r, c);
            let want = if n < MAX_HINTS {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            };
            assert_eq!(a.apply(Intent::Hint), want, "hint {n} of {MAX_HINTS}");
        }
        assert_eq!(a.hints_remaining(), 0);
        assert_eq!(a.hints_used(), MAX_HINTS);
    }

    #[test]
    fn a_hint_for_a_square_that_is_already_right_is_refused() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Digit(KNOWN_SOLUTION[idx(1, 7)]));
        assert_eq!(
            a.apply(Intent::Hint),
            EventResult::Ignored,
            "a hint was spent on a square that needed none"
        );
        assert_eq!(a.hints_remaining(), MAX_HINTS);
    }

    #[test]
    fn a_hint_for_a_clue_is_refused() {
        let mut a = board(&[80]);
        select(&mut a, 1, 7);
        assert_eq!(a.apply(Intent::Hint), EventResult::Ignored);
        assert_eq!(a.hints_remaining(), MAX_HINTS);
    }

    #[test]
    fn a_clue_holds_the_answer_it_came_from() {
        // `use_hint` refuses clues by way of "this square is already right",
        // which is only the same rule because a clue's value *is* its answer.
        // That is an invariant of the generator, not of the hint code, so it
        // is asserted here rather than left as an unwritten assumption: if a
        // puzzle ever came with a clue that disagreed with its solution, the
        // hint guard would quietly stop refusing clues.
        let a = generated(7, Difficulty::Easy);
        let mut clues = 0usize;
        for i in 0..TOTAL_CELLS {
            let (row, col) = row_col(i);
            let cell = a.cell(row, col);
            if cell.origin == Origin::Given {
                clues = clues.saturating_add(1);
                assert_eq!(
                    cell.value,
                    a.solution_at(row, col),
                    "the clue at {row},{col} is not the answer there"
                );
            }
        }
        assert!(clues > 0, "the puzzle came with no clues at all");
    }

    // ── The faults the wiring exposed ──────────────────────────────────────

    #[test]
    fn spending_a_hint_does_not_add_a_clue_to_the_puzzle() {
        // `given` used to mean both "a clue the puzzle came with" and "a square
        // the player may not type in", so a hint — which is the second — was
        // counted as the first, and the clue count climbed as the player asked
        // for help.
        let mut a = playground();
        let clues = a.given_count();
        select(&mut a, 1, 7);
        a.apply(Intent::Hint);
        assert_eq!(
            a.given_count(),
            clues,
            "a hint was counted as a clue the puzzle came with"
        );
        assert!(a.cell(1, 7).fixed(), "a hint left the square editable");
        assert_eq!(a.hints_used(), 1);
    }

    #[test]
    fn a_redo_can_win_the_game() {
        // `check_completion` was called from the two places that wrote a digit
        // forwards and from neither of the two that wrote one back, so a player
        // who undid the winning move and put it back sat looking at a finished
        // board the program did not think was finished.
        let mut a = board(&[80]);
        a.apply(Intent::Select(8, 8));
        a.apply(Intent::Digit(1));
        assert_eq!(a.status(), GameStatus::Playing, "a wrong digit won");
        a.apply(Intent::Undo);
        a.apply(Intent::Redo);
        assert_eq!(a.value(8, 8), 1);
        assert_eq!(a.status(), GameStatus::Playing, "a wrong redo won");

        let mut b = almost_done();
        b.apply(Intent::Digit(9));
        assert_eq!(b.status(), GameStatus::Won);
        // Put the game back in play so the undo is answered, then let the redo
        // be the move that fills the last square.
        b.status = GameStatus::Playing;
        b.apply(Intent::Undo);
        assert_eq!(b.value(8, 8), 0, "the undo did not empty the square");
        assert_eq!(b.status(), GameStatus::Playing);
        b.apply(Intent::Redo);
        assert_eq!(
            b.status(),
            GameStatus::Won,
            "a redo filled the last square without winning"
        );
    }

    #[test]
    fn a_hint_can_win_the_game() {
        let mut a = almost_done();
        a.apply(Intent::Hint);
        assert_eq!(
            a.status(),
            GameStatus::Won,
            "the hint that filled the last square did not finish the game"
        );
    }

    #[test]
    fn a_paused_game_can_be_restarted_and_resumed() {
        // Pausing used to lock out every intent, including the ones that undo
        // the pause, so a paused game could only be closed.
        let mut a = playground();
        // Select a hole first. Without this the selection sits on a clue, and
        // "a paused game accepted a digit" is really the clue guard answering:
        // the sweep found this test still green with the pause guard deleted.
        select(&mut a, 1, 7);
        a.apply(Intent::Pause);
        assert_eq!(a.status(), GameStatus::Paused);

        assert_eq!(
            a.apply(Intent::Digit(4)),
            EventResult::Ignored,
            "a paused game accepted a digit"
        );
        assert_eq!(a.value(1, 7), 0, "a paused game took a digit anyway");

        assert_eq!(a.apply(Intent::Pause), EventResult::Consumed);
        assert_eq!(a.status(), GameStatus::Playing, "pause would not lift");

        a.apply(Intent::Pause);
        assert_eq!(a.apply(Intent::NewGame), EventResult::Consumed);
        assert_eq!(
            a.status(),
            GameStatus::Playing,
            "a new deal from a paused game came up paused"
        );

        a.apply(Intent::Pause);
        assert_eq!(a.apply(Intent::CycleDifficulty), EventResult::Consumed);
        assert_eq!(a.status(), GameStatus::Playing);
        assert_eq!(a.difficulty(), Difficulty::Medium);
    }

    #[test]
    fn a_won_game_still_answers_the_new_and_difficulty_chips() {
        let mut a = almost_done();
        a.apply(Intent::Digit(9));
        assert_eq!(a.status(), GameStatus::Won);

        assert_eq!(
            a.apply(Intent::Digit(4)),
            EventResult::Ignored,
            "a finished game took another digit"
        );
        assert_eq!(a.apply(Intent::Undo), EventResult::Ignored);

        assert_eq!(a.apply(Intent::NewGame), EventResult::Consumed);
        assert_eq!(
            a.status(),
            GameStatus::Playing,
            "the New chip could not deal a game after a win"
        );

        let mut b = almost_done();
        b.apply(Intent::Digit(9));
        assert_eq!(
            b.apply(Intent::SetDifficulty(Difficulty::Hard)),
            EventResult::Consumed
        );
        assert_eq!(b.difficulty(), Difficulty::Hard);
        assert_eq!(b.status(), GameStatus::Playing);

        // The chip in the header sends `CycleDifficulty`, not `SetDifficulty`.
        // This test was named after the chip while exercising only the other
        // intent, so a win could have locked the chip out and nothing here
        // would have noticed -- which is what the sweep reported.
        let mut c = almost_done();
        c.apply(Intent::Digit(9));
        assert_eq!(c.status(), GameStatus::Won);
        let was = c.difficulty();
        assert_eq!(c.apply(Intent::CycleDifficulty), EventResult::Consumed);
        assert_eq!(c.difficulty(), was.next(), "the chip would not turn");
        assert_eq!(
            c.status(),
            GameStatus::Playing,
            "the difficulty chip left a finished game finished"
        );
    }

    #[test]
    fn pausing_a_finished_game_is_ignored() {
        let mut a = almost_done();
        a.apply(Intent::Digit(9));
        assert_eq!(
            a.apply(Intent::Pause),
            EventResult::Ignored,
            "a finished game was paused"
        );
        assert_eq!(a.status(), GameStatus::Won);
    }

    #[test]
    fn pausing_hides_the_board() {
        // The pause used to change a word in the header and nothing else, so a
        // player who paused to walk away left the puzzle on the screen.
        let mut a = board(&[0]);
        let l = layout_of(&a);
        let shown = text_in(&a.frame(SIZE.0, SIZE.1), l.cell_rect(1, 7));
        assert!(shown.is_some(), "the fixture drew no digit to hide");

        a.apply(Intent::Pause);
        let f = a.frame(SIZE.0, SIZE.1);
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                assert!(
                    text_in(&f, l.cell_rect(row, col)).is_none(),
                    "the digit at {row},{col} was still on screen while paused"
                );
            }
        }
        assert!(
            texts(&f).iter().any(|t| t == "Resume"),
            "a paused game does not say how to come back"
        );
    }

    #[test]
    fn the_hints_left_are_read_off_the_board_and_not_counted_down() {
        // The count used to be a field maintained in three places, one of which
        // — the redo path — decremented it without checking, so a redone hint
        // could take the count below zero.
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Hint);
        select(&mut a, 7, 1);
        a.apply(Intent::Hint);
        assert_eq!(a.hints_remaining(), MAX_HINTS - 2);

        for _ in 0..4 {
            a.apply(Intent::Undo);
            a.apply(Intent::Redo);
        }
        assert_eq!(
            a.hints_remaining(),
            MAX_HINTS - 2,
            "undoing and redoing a hint moved the count"
        );
        assert_eq!(a.hints_used(), 2);
        assert_eq!(
            a.hints_remaining() + a.hints_used(),
            MAX_HINTS,
            "the hints left and the hints spent do not add up"
        );
    }

    // ── The clock ──────────────────────────────────────────────────────────

    #[test]
    fn the_clock_counts_the_time_it_is_given_not_the_times_it_is_asked() {
        let mut a = board(&[0]);
        tick(&mut a, 1_500);
        assert_eq!(a.elapsed_ms(), 1_500);
        assert_eq!(a.elapsed_secs(), 1);
        tick(&mut a, 30_000);
        assert_eq!(a.elapsed_ms(), 31_500);
        assert_eq!(a.elapsed_secs(), 31, "the clock counted wake-ups");
    }

    #[test]
    fn a_tick_that_does_not_move_the_displayed_clock_asks_for_no_repaint() {
        let mut a = board(&[0]);
        assert_eq!(
            tick(&mut a, 100),
            EventResult::Ignored,
            "a tenth of a second asked for a repaint"
        );
        assert_eq!(a.elapsed_ms(), 100, "the time was thrown away with it");
        assert_eq!(
            tick(&mut a, 900),
            EventResult::Consumed,
            "the second that came round asked for nothing"
        );
    }

    #[test]
    fn the_clock_stops_while_paused_and_after_a_win() {
        let mut a = board(&[0]);
        tick(&mut a, 2_000);
        a.apply(Intent::Pause);
        assert_eq!(tick(&mut a, 60_000), EventResult::Ignored);
        assert_eq!(a.elapsed_ms(), 2_000, "the clock ran while paused");
        a.apply(Intent::Pause);
        tick(&mut a, 1_000);
        assert_eq!(a.elapsed_ms(), 3_000, "the clock did not restart");

        let mut b = almost_done();
        b.apply(Intent::Digit(9));
        assert_eq!(tick(&mut b, 60_000), EventResult::Ignored);
        assert_eq!(b.elapsed_ms(), 0, "the clock ran after the game was won");
    }

    #[test]
    fn the_clock_is_only_woken_while_there_is_time_to_count() {
        let mut a = board(&[0]);
        assert_eq!(
            a.tick_interval(),
            Some(Duration::from_millis(CLOCK_MS)),
            "a game in play asked for no wake-ups"
        );
        a.apply(Intent::Pause);
        assert_eq!(a.tick_interval(), None, "a paused game asked to be woken");

        let mut b = almost_done();
        b.apply(Intent::Digit(9));
        assert_eq!(b.tick_interval(), None, "a won game asked to be woken");
    }

    #[test]
    fn the_clock_counts_past_an_hour_rather_than_wrapping() {
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(9), "00:09");
        assert_eq!(format_time(59), "00:59");
        assert_eq!(format_time(60), "01:00");
        assert_eq!(format_time(61), "01:01");
        assert_eq!(format_time(3_599), "59:59");
        assert_eq!(format_time(3_600), "60:00", "an hour wrapped to nothing");
        assert_eq!(format_time(7_384), "123:04");
    }

    // ── Winning ────────────────────────────────────────────────────────────

    #[test]
    fn the_last_digit_wins_and_the_time_goes_on_the_board() {
        let mut a = almost_done();
        tick(&mut a, 42_000);
        assert_eq!(a.status(), GameStatus::Playing);
        a.apply(Intent::Digit(9));
        assert_eq!(a.status(), GameStatus::Won);
        assert_eq!(a.stats().games_completed(Difficulty::Easy), 1);
        assert_eq!(
            a.stats().best_time(Difficulty::Easy),
            Some(42),
            "the finishing time was not the one recorded"
        );
    }

    #[test]
    fn a_full_board_with_a_digit_in_the_wrong_place_is_not_a_win() {
        let mut a = almost_done();
        a.apply(Intent::Digit(1));
        assert_eq!(a.filled_count(), TOTAL_CELLS, "the board is not full");
        assert_eq!(
            a.status(),
            GameStatus::Playing,
            "a board that breaks the rules was called finished"
        );
        assert_eq!(a.stats().total_completed(), 0);
        assert!(!a.conflicts().is_empty(), "the wrong digit is not flagged");
    }

    #[test]
    fn the_status_word_and_its_colour_agree_with_the_state() {
        assert_eq!(status_text(GameStatus::Playing), "Playing");
        assert_eq!(status_text(GameStatus::Won), "Completed");
        assert_eq!(status_text(GameStatus::Paused), "Paused");
        assert_eq!(status_color(GameStatus::Playing), BLUE);
        assert_eq!(status_color(GameStatus::Won), GREEN);
        assert_eq!(status_color(GameStatus::Paused), YELLOW);
        let seen: HashSet<&str> = [GameStatus::Playing, GameStatus::Won, GameStatus::Paused]
            .into_iter()
            .map(status_text)
            .collect();
        assert_eq!(seen.len(), 3, "two states share a word");
    }

    // ── The keyboard ───────────────────────────────────────────────────────

    #[test]
    fn a_number_key_writes_its_own_digit() {
        for digit in 1..=9_u8 {
            let mut a = playground();
            select(&mut a, 1, 7);
            assert_eq!(key(&mut a, digit_key(digit)), EventResult::Consumed);
            assert_eq!(a.value(1, 7), digit, "the {digit} key wrote something else");
        }
    }

    #[test]
    fn delete_and_backspace_both_erase() {
        for k in [Key::Delete, Key::Backspace] {
            let mut a = playground();
            select(&mut a, 1, 7);
            key(&mut a, Key::Num4);
            assert_eq!(key(&mut a, k), EventResult::Consumed, "{k:?} did not erase");
            assert_eq!(a.value(1, 7), 0);
        }
    }

    #[test]
    fn the_arrow_keys_walk_the_selection_about() {
        let mut a = board(&[0]);
        walk_selection_with_keys(&mut a, 0, 0);
        assert_eq!(a.selected(), (0, 0));
        walk_selection_with_keys(&mut a, 8, 8);
        assert_eq!(a.selected(), (8, 8));
        walk_selection_with_keys(&mut a, 1, 7);
        assert_eq!(a.selected(), (1, 7));
        walk_selection_with_keys(&mut a, 7, 1);
        assert_eq!(a.selected(), (7, 1), "the two axes are crossed");
    }

    #[test]
    fn the_letter_keys_do_what_the_keypad_does() {
        let mut a = playground();
        select(&mut a, 1, 7);

        assert_eq!(key(&mut a, Key::N), EventResult::Consumed);
        assert!(a.note_mode(), "N did not turn note mode on");
        key(&mut a, Key::N);

        assert_eq!(key(&mut a, Key::H), EventResult::Consumed);
        assert_eq!(a.hints_used(), 1, "H did not spend a hint");

        assert_eq!(key(&mut a, Key::P), EventResult::Consumed);
        assert_eq!(a.status(), GameStatus::Paused, "P did not pause");
        key(&mut a, Key::P);

        assert_eq!(key(&mut a, Key::D), EventResult::Consumed);
        assert_eq!(a.difficulty(), Difficulty::Medium, "D did not change level");

        assert_eq!(key(&mut a, Key::F2), EventResult::Consumed);
        assert_eq!(a.status(), GameStatus::Playing);
        assert_eq!(
            a.difficulty(),
            Difficulty::Medium,
            "F2 changed the level as well as the deal"
        );
    }

    #[test]
    fn ctrl_z_and_ctrl_y_take_back_and_put_back() {
        let mut a = playground();
        select(&mut a, 1, 7);
        key(&mut a, Key::Num4);

        assert_eq!(
            handle_event(&mut a, &Event::Key(ctrl(Key::Z))),
            EventResult::Consumed
        );
        assert_eq!(a.value(1, 7), 0, "ctrl-z did not take the digit back");

        assert_eq!(
            handle_event(&mut a, &Event::Key(ctrl(Key::Y))),
            EventResult::Consumed
        );
        assert_eq!(a.value(1, 7), 4, "ctrl-y did not put the digit back");
    }

    #[test]
    fn ctrl_and_a_number_deals_that_level() {
        for (k, want) in [
            (Key::Num1, Difficulty::Easy),
            (Key::Num2, Difficulty::Medium),
            (Key::Num3, Difficulty::Hard),
        ] {
            let mut a = board(&[0]);
            assert_eq!(
                handle_event(&mut a, &Event::Key(ctrl(k))),
                EventResult::Consumed
            );
            assert_eq!(a.difficulty(), want, "ctrl-{k:?} dealt the wrong level");
            assert_eq!(a.status(), GameStatus::Playing);
        }
    }

    #[test]
    fn a_plain_z_is_not_an_undo() {
        let mut a = playground();
        select(&mut a, 1, 7);
        key(&mut a, Key::Num4);
        assert_eq!(
            key(&mut a, Key::Z),
            EventResult::Ignored,
            "a bare Z was read as ctrl-Z"
        );
        assert_eq!(a.value(1, 7), 4);
    }

    #[test]
    fn a_key_held_with_alt_or_the_windows_key_belongs_to_the_desktop() {
        for mods in [
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ] {
            let mut a = playground();
            select(&mut a, 1, 7);
            assert_eq!(
                handle_event(&mut a, &Event::Key(press_with(Key::Num4, mods))),
                EventResult::Ignored,
                "{mods:?} was swallowed by the game"
            );
            assert_eq!(a.value(1, 7), 0);
        }
    }

    #[test]
    fn letting_a_key_go_is_not_pressing_it() {
        let mut a = playground();
        select(&mut a, 1, 7);
        let released = KeyEvent {
            pressed: false,
            ..press(Key::Num4)
        };
        assert_eq!(
            handle_event(&mut a, &Event::Key(released)),
            EventResult::Ignored,
            "a key release wrote a digit"
        );
        assert_eq!(a.value(1, 7), 0);
    }

    #[test]
    fn a_key_this_game_has_no_use_for_is_left_alone() {
        let mut a = playground();
        select(&mut a, 1, 7);
        for k in [Key::Escape, Key::Tab, Key::Enter, Key::Q, Key::Num0] {
            assert_eq!(
                key(&mut a, k),
                EventResult::Ignored,
                "{k:?} was claimed by a game that does nothing with it"
            );
        }
        assert_eq!(a.value(1, 7), 0);
        assert_eq!(a.selected(), (1, 7));
    }

    // ── Where everything goes ──────────────────────────────────────────────

    #[test]
    fn the_bands_go_in_the_order_they_are_named() {
        // Footer first, then keypad, then header: the ladder written out, so a
        // reordered `BAND_DROP_ORDER` is a failing test rather than a surprise.
        let w = 600.0;
        let full = Layout::new(w, 200.0);
        assert!(full.shows(full.header), "the header went first");
        assert!(full.shows(full.keypad));
        assert!(full.shows(full.footer));

        let two = Layout::new(w, 120.0);
        assert!(two.shows(two.header));
        assert!(two.shows(two.keypad));
        assert!(!two.shows(two.footer), "the footer outlived the keypad");

        let one = Layout::new(w, 80.0);
        assert!(one.shows(one.header));
        assert!(!one.shows(one.keypad), "the keypad outlived the header");
        assert!(!one.shows(one.footer));

        let none = Layout::new(w, 40.0);
        assert!(!none.shows(none.header));
        assert!(!none.shows(none.keypad));
        assert!(!none.shows(none.footer));
    }

    #[test]
    fn a_centred_label_starts_inside_its_box_and_may_use_only_what_is_left() {
        // Both clamps in `centred_in` were untested. The vertical one only
        // matters when the line is taller than the box it is centred in, and
        // no other test draws into a box that short; the horizontal one only
        // matters when there is slack to centre into, and nothing checked the
        // width the label was allowed. The sweep deleted both unnoticed.
        let mut f = Frame::new(400.0, 200.0);
        let short = Rect::new(10.0, 50.0, 200.0, 4.0);
        centred_in(
            &mut f,
            short,
            "Sudoku",
            30.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
        );

        let (x, y, max) = f
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    x, y, max_width, ..
                } => Some((*x, *y, max_width.expect("the label was given no width"))),
                _ => None,
            })
            .expect("nothing was drawn");

        assert!(
            y >= short.y,
            "a line taller than its box started above it: {y} vs {}",
            short.y
        );
        assert!(
            x > short.x,
            "the text filled the box, so there is no centring here to test"
        );
        assert!(
            (max - (short.right() - x)).abs() < 0.01,
            "the label may use {max} but only {} is left from where it starts",
            short.right() - x
        );
    }

    #[test]
    fn the_bands_stack_down_the_window_in_the_order_they_are_named() {
        // Which bands survive is one question and where they sit is another.
        // The test above asks only the first, so the footer could have been
        // drawn at the top of the window, or the keypad below it, with every
        // band still "shown" -- both of which the mutation sweep did to this
        // layout without a single test noticing.
        let (w, h) = (600.0f32, 400.0f32);
        let l = Layout::new(w, h);
        let bands = [
            ("header", l.header),
            ("board", l.board),
            ("keypad", l.keypad),
            ("footer", l.footer),
        ];
        for (name, r) in bands {
            assert!(!r.is_empty(), "the {name} band is not there to place");
            assert!(
                r.x >= 0.0 && r.right() <= w,
                "the {name} band leaves the window"
            );
        }

        assert!(
            l.header.y.abs() < 0.01,
            "the header does not start at the top: {}",
            l.header.y
        );
        assert!(
            (l.footer.bottom() - h).abs() < 0.01,
            "the footer does not end at the bottom: {} vs {h}",
            l.footer.bottom()
        );
        // The bands are padded apart, so they need not abut -- but each must
        // begin at or below the end of the one named before it, which is what
        // makes the order in the window the order in the list.
        for pair in bands.windows(2) {
            let [(above, a), (below, b)] = pair else {
                unreachable!("windows(2) yields pairs")
            };
            assert!(
                b.y >= a.bottom() - 0.01,
                "the {below} band begins at {}, above where the {above} band \
                 ends at {}",
                b.y,
                a.bottom()
            );
        }
    }

    #[test]
    fn a_band_that_did_not_fit_is_gone_rather_than_flat() {
        let l = Layout::new(600.0, 40.0);
        assert_eq!(l.header, Rect::EMPTY, "a dropped header is a flat strip");
        assert_eq!(l.keypad, Rect::EMPTY);
        assert_eq!(l.footer, Rect::EMPTY);
        assert_eq!(
            l.chip(0),
            Rect::EMPTY,
            "a chip on a header that is not there"
        );
        assert_eq!(
            l.key_rect(0),
            Rect::EMPTY,
            "a key on a keypad that is not there"
        );
    }

    #[test]
    fn the_board_keeps_its_share_of_every_window() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            assert!(
                l.board.h >= h * BOARD_SHARE - 0.01,
                "{w}x{h}: the bands ate the board — {} of {h}",
                l.board.h
            );
        }
    }

    #[test]
    fn no_cell_is_drawn_over_a_band_or_outside_the_window() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    let r = l.cell_rect(row, col);
                    if r.is_empty() {
                        continue;
                    }
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w.max(1.0) + 0.01
                            && r.bottom() <= h.max(1.0) + 0.01,
                        "{w}x{h}: cell {row},{col} at {r:?} is outside the window"
                    );
                    for band in [l.header, l.keypad, l.footer] {
                        assert!(
                            !l.shows(band) || r.intersect(band).is_none(),
                            "{w}x{h}: cell {row},{col} at {r:?} is under {band:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn two_cells_never_cover_the_same_pixel_and_there_is_a_gap_between_them() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            if l.cell <= 0.0 {
                continue;
            }
            assert!(l.cell < l.step, "{w}x{h}: the cells are drawn edge to edge");
            let a = l.cell_rect(1, 7);
            let b = l.cell_rect(1, 8);
            assert!(
                a.intersect(b).is_none(),
                "{w}x{h}: two cells in a row overlap"
            );
            let c = l.cell_rect(2, 7);
            assert!(
                a.intersect(c).is_none(),
                "{w}x{h}: two cells in a column overlap"
            );
        }
    }

    #[test]
    fn a_window_with_no_room_for_a_board_draws_none_rather_than_a_wrong_one() {
        let l = Layout::new(1.0, 1.0);
        assert!(
            close(l.step, 0.0),
            "a one-pixel window drew a step of {}",
            l.step
        );
        assert!(
            close(l.cell, 0.0),
            "a one-pixel window drew a cell of {}",
            l.cell
        );
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                assert_eq!(
                    l.cell_rect(row, col),
                    Rect::EMPTY,
                    "cell {row},{col} was drawn in a one-pixel window"
                );
            }
        }
        assert_eq!(l.box_rect(0, 0), Rect::EMPTY);
    }

    #[test]
    fn a_square_off_the_board_has_no_rectangle() {
        let l = Layout::new(SIZE.0, SIZE.1);
        assert_eq!(l.cell_rect(GRID_SIZE, 0), Rect::EMPTY);
        assert_eq!(l.cell_rect(0, GRID_SIZE), Rect::EMPTY);
        assert_eq!(l.box_rect(BOX_SIZE, 0), Rect::EMPTY);
        assert_eq!(l.box_rect(0, BOX_SIZE), Rect::EMPTY);
    }

    #[test]
    fn the_board_is_a_square_sitting_in_the_middle_of_its_band() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            if l.step <= 0.0 {
                continue;
            }
            assert!(
                close(l.grid.w, l.grid.h),
                "{w}x{h}: the board is not square"
            );
            assert!(
                close(l.grid.w, l.step * GRID_SIZE as f32),
                "{w}x{h}: the board is not nine cells wide"
            );
            let left = l.grid.x - l.board.x;
            let right = l.board.right() - l.grid.right();
            assert!(close(left, right), "{w}x{h}: the board is off to one side");
            let top = l.grid.y - l.board.y;
            let bottom = l.board.bottom() - l.grid.bottom();
            assert!(close(top, bottom), "{w}x{h}: the board is high or low");
        }
    }

    #[test]
    fn a_cell_moves_right_with_its_column_and_down_with_its_row() {
        let l = Layout::new(SIZE.0, SIZE.1);
        // (1,7) and (7,1): a fixture in which the row and the column are equal
        // cannot tell a swapped pair apart. That is lesson 54.
        let a = l.cell_rect(1, 7);
        let b = l.cell_rect(7, 1);
        assert!(a.x > b.x, "the column does not decide how far right");
        assert!(a.y < b.y, "the row does not decide how far down");
        assert!(close(a.x - l.grid.x, 7.0 * l.step));
        assert!(close(a.y - l.grid.y, l.step));
        assert!(
            close(a.w, l.cell) && close(a.h, l.cell),
            "a cell is not square"
        );
    }

    #[test]
    fn a_box_encloses_exactly_its_own_nine_cells() {
        let l = Layout::new(SIZE.0, SIZE.1);
        for box_row in 0..BOX_SIZE {
            for box_col in 0..BOX_SIZE {
                let b = l.box_rect(box_row, box_col);
                assert!(!b.is_empty(), "box {box_row},{box_col} is not drawn");
                for row in 0..GRID_SIZE {
                    for col in 0..GRID_SIZE {
                        let c = l.cell_rect(row, col);
                        let mine = box_origin(row, col) == (box_row * BOX_SIZE, box_col * BOX_SIZE);
                        let inside = c.x >= b.x - 0.01
                            && c.y >= b.y - 0.01
                            && c.right() <= b.right() + 0.01
                            && c.bottom() <= b.bottom() + 0.01;
                        assert_eq!(
                            inside, mine,
                            "box {box_row},{box_col} and cell {row},{col} disagree"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_margin_is_never_more_than_a_quarter_of_the_side_it_is_taken_from() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            let side = w.max(1.0).min(h.max(1.0));
            assert!(
                l.pad <= side / 4.0 + 0.01,
                "{w}x{h}: a {} margin does not fit inside {side}",
                l.pad
            );
            assert!(l.pad >= 0.0);
        }
    }

    #[test]
    fn the_keypad_keys_sit_in_a_row_and_do_not_overlap() {
        let l = Layout::new(SIZE.0, SIZE.1);
        let mut previous: Option<Rect> = None;
        for i in 0..KEYPAD.len() {
            let r = l.key_rect(i);
            assert!(!r.is_empty(), "key {i} is not drawn");
            assert!(
                r.y >= l.keypad.y - 0.01 && r.bottom() <= l.keypad.bottom() + 0.01,
                "key {i} is outside the keypad band"
            );
            assert!(
                r.right() <= l.window.right() + 0.01,
                "key {i} runs off the window"
            );
            if let Some(p) = previous {
                assert!(r.x > p.x, "key {i} is not to the right of the one before");
                assert!(r.intersect(p).is_none(), "key {i} overlaps its neighbour");
                assert!(close(r.h, p.h), "the keys are not the same height");
            }
            previous = Some(r);
        }
        assert_eq!(l.key_rect(KEYPAD.len()), Rect::EMPTY, "a fifteenth key");
    }

    #[test]
    fn the_header_chips_sit_side_by_side_inside_the_header() {
        let l = Layout::new(SIZE.0, SIZE.1);
        let mut previous: Option<Rect> = None;
        for i in 0..3 {
            let r = l.chip(i);
            assert!(!r.is_empty(), "chip {i} is not drawn");
            assert!(
                r.y >= l.header.y - 0.01 && r.bottom() <= l.header.bottom() + 0.01,
                "chip {i} is outside the header"
            );
            assert!(r.right() <= l.header.right() + 0.01);
            if let Some(p) = previous {
                assert!(
                    r.right() <= p.x + 0.01,
                    "chip {i} is counted from the wrong end"
                );
                assert!(r.intersect(p).is_none(), "chip {i} overlaps its neighbour");
            }
            previous = Some(r);
        }
    }

    #[test]
    fn every_frame_is_balanced_at_every_size() {
        let mut a = board(&[0, 40, 80]);
        select(&mut a, 1, 7);
        for (w, h) in SIZES {
            let f = a.frame(w, h);
            assert!(
                f.is_balanced(),
                "{w}x{h}: a clip or translate was left open"
            );
            assert!(
                !f.commands().is_empty(),
                "{w}x{h}: the window was left blank"
            );
        }
    }

    #[test]
    fn resizing_the_window_moves_the_board() {
        let mut a = board(&[0]);
        let before = layout_of(&a).cell_rect(1, 7);
        handle_event(
            &mut a,
            &Event::Resize {
                width: 1200,
                height: 1000,
            },
        );
        assert_eq!(a.size(), (1200.0, 1000.0), "the resize was not remembered");
        let after = layout_of(&a).cell_rect(1, 7);
        assert_ne!(before, after, "the board did not move with the window");
        assert!(after.w > before.w, "a bigger window drew smaller cells");
    }

    // ── Hit boxes ──────────────────────────────────────────────────────────

    #[test]
    fn every_square_records_a_hit_box_where_it_was_painted() {
        let a = board(&[0]);
        let l = layout_of(&a);
        let f = a.frame(SIZE.0, SIZE.1);
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let want = l.cell_rect(row, col);
                let got = f
                    .rect_of(|t| *t == Target::Cell(row, col))
                    .unwrap_or_else(|| panic!("cell {row},{col} cannot be clicked"));
                assert_eq!(
                    got, want,
                    "cell {row},{col} is clicked somewhere other than where it is drawn"
                );
            }
        }
    }

    #[test]
    fn every_keypad_key_records_a_hit_box_where_it_was_painted() {
        let a = board(&[0]);
        let l = layout_of(&a);
        let f = a.frame(SIZE.0, SIZE.1);
        for (i, &target) in KEYPAD.iter().enumerate() {
            let got = f
                .rect_of(|t| *t == target)
                .unwrap_or_else(|| panic!("{target:?} cannot be clicked"));
            assert_eq!(got, l.key_rect(i), "{target:?} is clicked off its key");
        }
        for (i, target) in [Target::Pause, Target::Difficulty, Target::NewGame]
            .into_iter()
            .enumerate()
        {
            let got = f
                .rect_of(|t| *t == target)
                .unwrap_or_else(|| panic!("{target:?} cannot be clicked"));
            assert_eq!(got, l.chip(i), "{target:?} is clicked off its chip");
        }
    }

    #[test]
    fn a_cells_hit_box_moves_right_with_its_column_and_down_with_its_row() {
        let a = board(&[0]);
        let f = a.frame(SIZE.0, SIZE.1);
        let one_seven = f.rect_of(|t| *t == Target::Cell(1, 7)).unwrap();
        let seven_one = f.rect_of(|t| *t == Target::Cell(7, 1)).unwrap();
        assert!(
            one_seven.x > seven_one.x,
            "the two hit boxes have their axes crossed"
        );
        assert!(one_seven.y < seven_one.y);
        assert_eq!(
            f.hit_test(one_seven.centre().0, one_seven.centre().1),
            Some(Target::Cell(1, 7)),
            "the middle of a cell does not hit that cell"
        );
    }

    #[test]
    fn clicking_a_square_selects_that_square_and_no_other() {
        let mut a = board(&[0]);
        for (row, col) in [(0, 0), (1, 7), (7, 1), (8, 8), (4, 4)] {
            assert_eq!(click_cell(&mut a, row, col), EventResult::Consumed);
            assert_eq!(
                a.selected(),
                (row, col),
                "a click on {row},{col} landed elsewhere"
            );
        }
    }

    #[test]
    fn clicking_the_gap_between_two_squares_selects_neither() {
        let mut a = board(&[0]);
        let l = layout_of(&a);
        select(&mut a, 0, 0);
        let first = l.cell_rect(1, 7);
        // The strip of board between one cell and the next.
        let x = first.right() + (l.step - l.cell) / 2.0;
        let y = first.centre().1;
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Press(MouseButton::Left)),
            EventResult::Ignored,
            "the gap between two cells belongs to one of them"
        );
        assert_eq!(a.selected(), (0, 0), "a click on nothing moved the cursor");
    }

    #[test]
    fn clicking_outside_everything_does_nothing() {
        let mut a = board(&[0]);
        select(&mut a, 0, 0);
        let l = layout_of(&a);
        // The middle of the board band, beside the grid, is background.
        let x = f32::midpoint(l.board.x, l.grid.x);
        assert_eq!(
            mouse(
                &mut a,
                x,
                l.grid.centre().1,
                MouseEventKind::Press(MouseButton::Left)
            ),
            EventResult::Ignored
        );
        assert_eq!(a.selected(), (0, 0));
    }

    #[test]
    fn the_keypad_keys_do_what_they_say() {
        let mut a = playground();
        select(&mut a, 1, 7);
        let l = layout_of(&a);
        let press_key = |a: &mut SudokuApp, target: Target| {
            let i = KEYPAD
                .iter()
                .position(|&t| t == target)
                .unwrap_or_else(|| panic!("{target:?} is not on the keypad"));
            let (x, y) = l.key_rect(i).centre();
            mouse(a, x, y, MouseEventKind::Press(MouseButton::Left))
        };

        assert_eq!(press_key(&mut a, Target::Digit(4)), EventResult::Consumed);
        assert_eq!(a.value(1, 7), 4, "the 4 key wrote something else");

        assert_eq!(press_key(&mut a, Target::Erase), EventResult::Consumed);
        assert_eq!(a.value(1, 7), 0, "the erase key did not erase");

        assert_eq!(press_key(&mut a, Target::Notes), EventResult::Consumed);
        assert!(a.note_mode(), "the notes key did not turn note mode on");
        press_key(&mut a, Target::Notes);

        assert_eq!(press_key(&mut a, Target::Hint), EventResult::Consumed);
        assert_eq!(a.hints_used(), 1, "the hint key spent no hint");

        assert_eq!(press_key(&mut a, Target::Undo), EventResult::Consumed);
        assert_eq!(a.hints_used(), 0, "the undo key undid nothing");

        assert_eq!(press_key(&mut a, Target::Redo), EventResult::Consumed);
        assert_eq!(a.hints_used(), 1, "the redo key redid nothing");
    }

    #[test]
    fn the_header_chips_do_what_they_say() {
        let mut a = playground();
        let l = layout_of(&a);
        let press_chip = |a: &mut SudokuApp, i: usize| {
            let (x, y) = l.chip(i).centre();
            mouse(a, x, y, MouseEventKind::Press(MouseButton::Left))
        };

        assert_eq!(press_chip(&mut a, 0), EventResult::Consumed);
        assert_eq!(
            a.status(),
            GameStatus::Paused,
            "the pause chip did not pause"
        );
        assert_eq!(press_chip(&mut a, 0), EventResult::Consumed);
        assert_eq!(
            a.status(),
            GameStatus::Playing,
            "the pause chip did not lift"
        );

        assert_eq!(press_chip(&mut a, 1), EventResult::Consumed);
        assert_eq!(
            a.difficulty(),
            Difficulty::Medium,
            "the difficulty chip did not cycle"
        );

        let before = values_array(&a.cells);
        assert_eq!(press_chip(&mut a, 2), EventResult::Consumed);
        assert_eq!(
            a.difficulty(),
            Difficulty::Medium,
            "the new chip changed level"
        );
        assert_ne!(
            before,
            values_array(&a.cells),
            "the new chip dealt the same game"
        );
    }

    #[test]
    fn only_the_left_button_plays_the_game() {
        let mut a = board(&[0]);
        select(&mut a, 0, 0);
        let (x, y) = cell_point(&a, 1, 7);
        for button in [
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Back,
            MouseButton::Forward,
        ] {
            assert_eq!(
                mouse(&mut a, x, y, MouseEventKind::Press(button)),
                EventResult::Ignored,
                "{button:?} selected a square"
            );
        }
        assert_eq!(a.selected(), (0, 0));
    }

    #[test]
    fn moving_the_mouse_or_letting_the_button_go_is_not_a_click() {
        let mut a = board(&[0]);
        select(&mut a, 0, 0);
        let (x, y) = cell_point(&a, 1, 7);
        for kind in [
            MouseEventKind::Move,
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Enter,
            MouseEventKind::Leave,
        ] {
            assert_eq!(
                mouse(&mut a, x, y, kind.clone()),
                EventResult::Ignored,
                "{kind:?} was read as a click"
            );
        }
        assert_eq!(a.selected(), (0, 0));
    }

    #[test]
    fn every_control_the_program_has_can_be_reached_with_a_mouse() {
        let a = board(&[0]);
        let f = a.frame(SIZE.0, SIZE.1);
        let mut wanted: Vec<Target> = KEYPAD.to_vec();
        wanted.extend([Target::Pause, Target::Difficulty, Target::NewGame]);
        for target in wanted {
            let r = f
                .rect_of(|t| *t == target)
                .unwrap_or_else(|| panic!("{target:?} has no hit box at all"));
            let (x, y) = r.centre();
            assert_eq!(
                f.hit_test(x, y),
                Some(target),
                "{target:?} is covered by something else"
            );
        }
        assert_eq!(
            f.hits().len(),
            TOTAL_CELLS + KEYPAD.len() + 3,
            "the frame records a control nobody named"
        );
    }

    #[test]
    fn a_mouse_alone_can_play_a_game_to_the_end() {
        let mut a = almost_done();
        select(&mut a, 0, 0);
        click_cell(&mut a, 8, 8);
        assert_eq!(a.selected(), (8, 8));
        let l = layout_of(&a);
        let i = KEYPAD.iter().position(|&t| t == Target::Digit(9)).unwrap();
        let (x, y) = l.key_rect(i).centre();
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Press(MouseButton::Left)),
            EventResult::Consumed
        );
        assert_eq!(
            a.status(),
            GameStatus::Won,
            "a game played entirely with the mouse could not be finished"
        );
    }

    #[test]
    fn a_click_is_read_against_the_size_the_frame_was_drawn_at() {
        let mut a = board(&[0]);
        a.resize(1200.0, 1000.0);
        let l = Layout::new(1200.0, 1000.0);
        let (x, y) = l.cell_rect(1, 7).centre();
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Press(MouseButton::Left)),
            EventResult::Consumed
        );
        assert_eq!(
            a.selected(),
            (1, 7),
            "the click was read against the old window size"
        );
    }

    #[test]
    fn every_control_has_a_name_of_its_own() {
        let mut seen: HashSet<String> = HashSet::new();
        let mut targets: Vec<Target> = KEYPAD.to_vec();
        targets.extend([Target::Pause, Target::Difficulty, Target::NewGame]);
        for target in targets {
            let label = key_label(target);
            assert!(!label.is_empty(), "{target:?} is drawn with no name");
            assert!(seen.insert(label.clone()), "two controls are both {label}");
        }
        assert_eq!(key_label(Target::Cell(1, 7)), "1,7");
        assert_eq!(
            key_label(Target::Cell(7, 1)),
            "7,1",
            "a cell name is reversed"
        );
    }

    // ── What is on the screen ──────────────────────────────────────────────

    #[test]
    fn the_header_shows_the_clock_the_state_and_how_far_along_the_player_is() {
        let mut a = board(&[idx(1, 7), idx(7, 1)]);
        tick(&mut a, 65_000);
        let l = layout_of(&a);
        let shown = texts_in(&a.frame(SIZE.0, SIZE.1), l.header).join(" | ");
        assert!(
            shown.contains("Sudoku  01:05"),
            "the header does not show the clock: {shown}"
        );
        assert!(
            shown.contains("Playing"),
            "the header hides the state: {shown}"
        );
        assert!(
            shown.contains(&format!("79/{TOTAL_CELLS}")),
            "the header hides the progress: {shown}"
        );
        assert!(
            shown.contains(&format!("Hints {MAX_HINTS}")),
            "the header hides the hints left: {shown}"
        );
        assert!(
            shown.contains("Notes off"),
            "the header hides note mode: {shown}"
        );

        a.apply(Intent::ToggleNotes);
        let on = texts_in(&a.frame(SIZE.0, SIZE.1), l.header).join(" | ");
        assert!(on.contains("Notes on"), "note mode does not show: {on}");
    }

    #[test]
    fn the_state_line_is_drawn_in_the_colour_of_the_state() {
        let mut a = board(&[0]);
        let playing = a.frame(SIZE.0, SIZE.1);
        let l = layout_of(&a);
        let line = |f: &Frame| {
            texts_in(f, l.header)
                .into_iter()
                .find(|t| t.contains('/'))
                .expect("the state line is not drawn")
        };
        assert_eq!(text_color(&playing, &line(&playing)), Some(BLUE));

        a.apply(Intent::Pause);
        let paused = a.frame(SIZE.0, SIZE.1);
        assert_eq!(text_color(&paused, &line(&paused)), Some(YELLOW));

        let mut b = almost_done();
        b.apply(Intent::Digit(9));
        let won = b.frame(SIZE.0, SIZE.1);
        assert_eq!(text_color(&won, &line(&won)), Some(GREEN));
    }

    #[test]
    fn the_pause_chip_offers_to_come_back_while_the_game_is_paused() {
        let mut a = board(&[0]);
        let l = layout_of(&a);
        let word = |a: &SudokuApp| text_in(&a.frame(SIZE.0, SIZE.1), l.chip(0)).map(|(t, ..)| t);
        assert_eq!(word(&a).as_deref(), Some("Pause"));
        a.apply(Intent::Pause);
        assert_eq!(
            word(&a).as_deref(),
            Some("Resume"),
            "a paused game still offers to pause"
        );
    }

    #[test]
    fn a_clue_a_hint_and_the_players_own_digit_are_three_different_colours() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Digit(KNOWN_SOLUTION[idx(1, 7)]));
        select(&mut a, 7, 1);
        a.apply(Intent::Hint);
        select(&mut a, 0, 0);

        let l = layout_of(&a);
        let f = a.frame(SIZE.0, SIZE.1);
        let (clue, clue_color, clue_weight) =
            text_in(&f, l.cell_rect(4, 4)).expect("a clue is not drawn");
        let (own, own_color, own_weight) =
            text_in(&f, l.cell_rect(1, 7)).expect("the player's digit is not drawn");
        let (hint, hint_color, _) = text_in(&f, l.cell_rect(7, 1)).expect("a hint is not drawn");

        assert_eq!(clue, KNOWN_SOLUTION[idx(4, 4)].to_string());
        assert_eq!(own, KNOWN_SOLUTION[idx(1, 7)].to_string());
        assert_eq!(hint, KNOWN_SOLUTION[idx(7, 1)].to_string());
        assert_eq!(clue_color, TEXT_COLOR);
        assert_eq!(own_color, BLUE, "the player's own digit looks like a clue");
        assert_eq!(hint_color, PEACH, "a hint looks like the player's own work");
        assert_eq!(clue_weight, FontWeightHint::Bold);
        assert_eq!(own_weight, FontWeightHint::Regular);
    }

    #[test]
    fn a_digit_that_breaks_a_rule_is_drawn_in_red() {
        let mut a = playground();
        select(&mut a, 1, 7);
        // Row 1 already holds a 6 at column 0, so a second one conflicts.
        a.apply(Intent::Digit(6));
        select(&mut a, 0, 0);
        let l = layout_of(&a);
        let f = a.frame(SIZE.0, SIZE.1);
        let (_, color, _) = text_in(&f, l.cell_rect(1, 7)).expect("no digit drawn");
        assert_eq!(color, RED, "a digit that breaks a rule is not flagged");
        assert!(a.conflicts().contains(&(1, 7)));
    }

    #[test]
    fn only_the_selected_square_is_outlined() {
        let mut a = board(&[0]);
        select(&mut a, 1, 7);
        let l = layout_of(&a);
        let outlines = strokes(&a.frame(SIZE.0, SIZE.1));
        let cell = l.cell_rect(1, 7);
        assert!(
            outlines.contains(&cell),
            "the selected square is not outlined"
        );
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                if (row, col) == (1, 7) {
                    continue;
                }
                let other = l.cell_rect(row, col);
                assert!(
                    !outlines.contains(&other),
                    "the unselected square {row},{col} is outlined too"
                );
            }
        }
    }

    #[test]
    fn the_selected_square_is_shaded_differently_from_its_neighbours() {
        let mut a = board(&[0]);
        select(&mut a, 1, 7);
        let l = layout_of(&a);
        let f = a.frame(SIZE.0, SIZE.1);
        let mine = fill_color_at(&f, l.cell_rect(1, 7)).expect("the square is not filled");
        let far = fill_color_at(&f, l.cell_rect(5, 2)).expect("a far square is not filled");
        // The squares the selection lights up along its row, column and box are
        // shaded too, so "different from a square across the board" is not
        // enough: the sweep shaded the selection the same as its own row and
        // this test still passed. It has to differ from its neighbours as well.
        let peer = fill_color_at(&f, l.cell_rect(1, 3)).expect("a peer is not filled");
        assert_ne!(mine, far, "the selection is invisible");
        assert_ne!(peer, far, "the selection lights up nothing around it");
        assert_ne!(
            mine, peer,
            "the selected square is shaded like the rest of its row"
        );
    }

    #[test]
    fn nine_marks_go_in_nine_places() {
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        for digit in 1..=9_u8 {
            a.apply(Intent::Digit(digit));
        }
        let l = layout_of(&a);
        let f = a.frame(SIZE.0, SIZE.1);
        let cell = l.cell_rect(1, 7);
        let marks = texts_in(&f, cell);
        assert_eq!(marks.len(), 9, "nine marks were not all drawn: {marks:?}");
        let mut places: HashSet<(i32, i32)> = HashSet::new();
        for c in f.commands() {
            if let RenderCommand::Text { x, y, .. } = c {
                if cell.contains(*x, *y) {
                    assert!(
                        places.insert((*x as i32, *y as i32)),
                        "two marks are drawn on top of each other"
                    );
                }
            }
        }
        assert_eq!(places.len(), 9);
    }

    /// Where each mark of `cell_rect(row, col)` was painted, by digit.
    fn mark_places(a: &SudokuApp, row: usize, col: usize) -> Vec<(String, f32, f32)> {
        let cell = layout_of(a).cell_rect(row, col);
        a.frame(SIZE.0, SIZE.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, text, .. } if cell.contains(*x, *y) => {
                    Some((text.clone(), *x, *y))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_the_marks_that_are_set_are_drawn_and_they_read_across() {
        // Setting all nine marks, as the test above does, hides two faults at
        // once: every slot is occupied, so drawing the unset ones as well
        // changes nothing, and the nine places are symmetric, so laying them
        // out down the columns instead of across the rows still fills them.
        // The sweep made both changes with the whole suite still green. Three
        // marks in an asymmetric pattern answer both questions.
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        for digit in [1_u8, 2, 4] {
            a.apply(Intent::Digit(digit));
        }

        let places = mark_places(&a, 1, 7);
        let drawn: Vec<&str> = places.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(drawn.len(), 3, "a mark nobody set was drawn: {drawn:?}");
        for want in ["1", "2", "4"] {
            assert!(drawn.contains(&want), "mark {want} is missing: {drawn:?}");
        }

        let at = |d: &str| {
            places
                .iter()
                .find(|(t, _, _)| t == d)
                .map(|(_, x, y)| (*x, *y))
                .expect("the mark was drawn")
        };
        let (x1, y1) = at("1");
        let (x2, y2) = at("2");
        let (x4, y4) = at("4");
        assert!(x2 > x1, "mark 2 is not to the right of mark 1");
        assert!(
            (y2 - y1).abs() < 0.01,
            "marks 1 and 2 are not on the same line"
        );
        assert!(y4 > y1, "mark 4 is not below mark 1");
        assert!(
            (x4 - x1).abs() < 0.01,
            "marks 1 and 4 are not in the same column"
        );
    }

    #[test]
    fn a_square_with_a_digit_in_it_draws_no_marks() {
        // Marks and a digit in the same square would overprint each other. The
        // marks are drawn only for an empty square, and the sweep showed that
        // rule was untested: drawing them for a filled one as well passed.
        // Writing a digit clears that square's marks, so the state this is
        // about has to be built the other way round: the digit first, then the
        // marks, which note mode will set over a square that already has one.
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::Digit(3));
        assert_eq!(a.value(1, 7), 3, "the digit did not go in");

        a.apply(Intent::ToggleNotes);
        for digit in [1_u8, 2, 4] {
            a.apply(Intent::Digit(digit));
        }
        assert!(
            a.cell(1, 7).has_note(1),
            "a square with a digit would not take a mark, so there is nothing \
             here to draw over the digit and this test proves nothing"
        );

        let drawn = mark_places(&a, 1, 7);
        let texts: Vec<&str> = drawn.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(
            texts,
            vec!["3"],
            "a filled square drew its marks as well as its digit"
        );
    }

    #[test]
    fn a_key_that_would_do_nothing_is_drawn_greyed_out() {
        let mut a = playground();
        let l = layout_of(&a);
        let accent = |a: &SudokuApp, target: Target| {
            let i = KEYPAD.iter().position(|&t| t == target).unwrap();
            text_in(&a.frame(SIZE.0, SIZE.1), l.key_rect(i))
                .unwrap_or_else(|| panic!("{target:?} is drawn with no label"))
                .1
        };

        assert_eq!(
            accent(&a, Target::Undo),
            OVERLAY0,
            "undo looks live with nothing to undo"
        );
        assert_eq!(accent(&a, Target::Redo), OVERLAY0);
        assert_eq!(accent(&a, Target::Hint), PEACH, "the hints look spent");
        assert_eq!(accent(&a, Target::Notes), SUBTEXT0);

        select(&mut a, 1, 7);
        a.apply(Intent::Digit(4));
        assert_eq!(accent(&a, Target::Undo), MAUVE, "undo stayed grey");
        assert_eq!(accent(&a, Target::Redo), OVERLAY0, "redo lit up too early");
        a.apply(Intent::Undo);
        assert_eq!(accent(&a, Target::Redo), MAUVE, "redo stayed grey");

        a.apply(Intent::ToggleNotes);
        assert_eq!(accent(&a, Target::Notes), TEAL, "note mode does not show");
    }

    #[test]
    fn the_hint_key_greys_out_when_the_hints_are_gone() {
        let mut a = board(&[0, 1, 2, 3, 4]);
        let l = layout_of(&a);
        let i = KEYPAD.iter().position(|&t| t == Target::Hint).unwrap();
        for slot in 0..MAX_HINTS {
            let (r, c) = row_col(slot);
            select(&mut a, r, c);
            a.apply(Intent::Hint);
        }
        assert_eq!(a.hints_remaining(), 0);
        let color = text_in(&a.frame(SIZE.0, SIZE.1), l.key_rect(i)).unwrap().1;
        assert_eq!(color, OVERLAY0, "the hint key looks live with none left");
    }

    #[test]
    fn the_footer_shows_the_record_and_whether_the_board_holds_together() {
        let mut a = almost_done();
        tick(&mut a, 30_000);
        let l = layout_of(&a);
        let clean = texts_in(&a.frame(SIZE.0, SIZE.1), l.footer).join(" | ");
        assert!(
            clean.contains("Solved 0"),
            "the footer hides the tally: {clean}"
        );
        assert!(
            clean.contains("Best --:--"),
            "no record is not shown: {clean}"
        );
        assert!(
            clean.contains("no conflicts"),
            "a sound board is not reported: {clean}"
        );

        a.apply(Intent::Digit(1));
        let bad = texts_in(&a.frame(SIZE.0, SIZE.1), l.footer).join(" | ");
        assert!(
            bad.contains("conflicts") && !bad.contains("no conflicts"),
            "a broken board is reported sound: {bad}"
        );

        a.apply(Intent::Undo);
        a.apply(Intent::Digit(9));
        let won = texts_in(&a.frame(SIZE.0, SIZE.1), l.footer).join(" | ");
        assert!(won.contains("Solved 1"), "a win is not counted: {won}");
        assert!(won.contains("Best 00:30"), "the record is not shown: {won}");
        assert!(won.contains("Easy 1"), "the level tally is wrong: {won}");
    }

    /// Fill every empty square with the answer, one intent at a time.
    fn play_to_a_win(a: &mut SudokuApp) {
        for i in 0..TOTAL_CELLS {
            let (row, col) = row_col(i);
            if a.value(row, col) != 0 {
                continue;
            }
            select(a, row, col);
            let answer = a.solution_at(row, col);
            assert_eq!(
                a.apply(Intent::Digit(answer)),
                EventResult::Consumed,
                "the answer was refused at {row},{col}"
            );
        }
        assert_eq!(a.status(), GameStatus::Won, "the board would not finish");
    }

    #[test]
    fn the_footer_counts_every_level_and_not_just_the_one_being_played() {
        // "Solved N" is the total across all three levels; the tallies beside
        // it are each level's own. After a single win both numbers are 1, so
        // the total could be read from the level in play and the test above
        // could not tell -- and the sweep did exactly that, unnoticed. Two wins
        // at two levels are what make the two numbers disagree.
        let mut a = almost_done();
        a.apply(Intent::Digit(9));
        assert_eq!(a.status(), GameStatus::Won);

        assert_eq!(
            a.apply(Intent::SetDifficulty(Difficulty::Medium)),
            EventResult::Consumed
        );
        play_to_a_win(&mut a);

        let l = layout_of(&a);
        let footer = texts_in(&a.frame(SIZE.0, SIZE.1), l.footer).join(" | ");
        assert!(
            footer.contains("Solved 2"),
            "the total counts one level only: {footer}"
        );
        assert!(
            footer.contains("Easy 1") && footer.contains("Medium 1"),
            "the level tallies are wrong: {footer}"
        );
    }

    #[test]
    fn no_text_is_drawn_outside_the_window_it_belongs_to() {
        // The header's heading is taller than the half-band it is centred in,
        // and centring a tall line in a short box gives a negative offset: the
        // clock started six tenths of a pixel above the top of the window and
        // the screen edge took the difference.
        let mut a = playground();
        select(&mut a, 1, 7);
        a.apply(Intent::ToggleNotes);
        a.apply(Intent::Digit(3));
        for (w, h) in SIZES {
            let f = a.frame(w, h);
            for c in f.commands() {
                if let RenderCommand::Text { x, y, text, .. } = c {
                    assert!(
                        *x >= -0.01 && *y >= -0.01,
                        "{w}x{h}: {text:?} starts at {x},{y}, outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn a_window_too_small_for_anything_still_draws_something() {
        let a = board(&[0]);
        let f = a.frame(1.0, 1.0);
        assert!(!f.commands().is_empty(), "a tiny window was left blank");
        assert!(f.hits().is_empty(), "a board nobody can see can be clicked");
    }

    // ── The window it lives in ─────────────────────────────────────────────

    #[test]
    fn the_program_names_itself_the_same_way_everywhere() {
        let a = SudokuApp::new();
        assert_eq!(a.title(), "Sudoku");
        assert_eq!(a.app_id(), "sudoku");
        assert_eq!(
            a.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32),
            "the window opens at a size the layout was not written for"
        );
        assert!(
            <SudokuApp as App>::resizable(&a),
            "the window cannot be resized"
        );
    }

    #[test]
    fn asking_the_window_to_close_closes_it() {
        let mut a = board(&[0]);
        assert_eq!(a.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn the_window_is_only_repainted_when_something_changed() {
        let mut a = playground();
        select(&mut a, 1, 7);
        assert_eq!(
            a.on_event(&Event::Key(press(Key::Num4))),
            Response::Redraw,
            "a digit did not ask for a repaint"
        );
        assert_eq!(
            a.on_event(&Event::Key(press(Key::Escape))),
            Response::Idle,
            "a key the game ignores asked for a repaint"
        );
        assert_eq!(
            a.on_event(&Event::Tick { elapsed_ms: 10 }),
            Response::Idle,
            "a tick too small to show asked for a repaint"
        );
    }

    #[test]
    fn the_events_this_game_has_no_use_for_are_left_alone() {
        let mut a = board(&[0]);
        for event in [
            Event::FocusIn,
            Event::FocusOut,
            Event::Moved { x: 3, y: 4 },
            Event::ScaleChanged { scale: 2.0 },
        ] {
            assert_eq!(
                handle_event(&mut a, &event),
                EventResult::Ignored,
                "{event:?} was claimed by a game that does nothing with it"
            );
        }
        assert_eq!(a.selected(), (4, 4));
        assert_eq!(a.size(), SIZE, "an event nobody handles resized the window");
    }

    #[test]
    fn what_the_window_draws_is_what_the_frame_drew() {
        let mut a = playground();
        select(&mut a, 1, 7);
        let tree = a.render(900.0, 700.0);
        assert_eq!(
            a.size(),
            (900.0, 700.0),
            "the size drawn at was not the size remembered"
        );
        let expected = a.frame(900.0, 700.0).into_tree();
        assert_eq!(
            tree.commands.len(),
            expected.commands.len(),
            "the window and the frame disagree about what is on it"
        );
    }

    #[test]
    fn the_probe_draws_at_the_size_it_was_given() {
        let a = board(&[0]);
        let small = <SudokuApp as Probe>::draw(&a, (400.0, 400.0));
        let large = <SudokuApp as Probe>::draw(&a, (1200.0, 1000.0));
        assert!(probe::is_visible_sized(
            &a,
            Target::Cell(1, 7),
            (1200.0, 1000.0)
        ));
        assert_ne!(
            small.rect_of(|t| *t == Target::Cell(1, 7)),
            large.rect_of(|t| *t == Target::Cell(1, 7)),
            "the probe drew both sizes the same"
        );
        assert_eq!(<SudokuApp as Probe>::SIZE, SIZE);
    }
}
