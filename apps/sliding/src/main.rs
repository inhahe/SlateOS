//! Sliding puzzle — slide the tiles back into order, in a real window.
//!
//! Three sizes (3x3, 4x4, 5x5), keyboard and pointer, a help sheet, and a
//! per-size record of the fewest moves you have finished in.
//!
//! # What wiring this up found
//!
//! The program drew a board and could not be played, because `main` built a
//! `SlidingPuzzle`, dropped it and exited. Nothing below was reachable to
//! notice until it had a window on it.
//!
//! 1. **The key handler never read `KeyEvent::pressed`.** The pattern was
//!    `Event::Key(KeyEvent { key, modifiers, .. })`, and the `..` swallowed
//!    exactly the field that says whether the key went down or came back up,
//!    so **every key ran twice**. On this program that is not "twice as fast":
//!    an arrow slid *two* tiles per press, so the move counter went up by two
//!    and the board went somewhere you had not asked for; `H` opened the help
//!    sheet on the press and closed it on the release, so **the only written
//!    record of the controls could never be seen**; `T` turned the numbers off
//!    and straight back on, so **they could not be turned off**; `N` built and
//!    shuffled two boards and showed you the second; and `3`/`4`/`5` did the
//!    same.
//! 2. **The hit test was a second copy of the layout.** `handle_mouse_click`
//!    declared its own `grid_offset_x = 50.0`, `grid_offset_y = 80.0` and
//!    `cell_size = 80.0` beside `render`'s own `grid_x`, `grid_y` and
//!    `cell_size` — two sets of constants that had to agree, with nothing to
//!    make them. Hit boxes are now recorded by the drawing pass itself, so a
//!    tile is clickable exactly where its ink is.
//! 3. **The layout was a constant.** `render(width, height)` used its two
//!    arguments for the background rectangle and nothing else: the title, the
//!    move counter and the size all drew at fixed `x`, the grid was 80px cells
//!    at (50, 80), the best-scores panel was pinned at `50 + 80 * size + 40`
//!    and 180 wide — off the right-hand edge of any window narrower than 670px
//!    at 4x4 and 750px at 5x5 — and the help sheet ran to `y = 440`.
//! 4. **A click had to land on a tile touching the gap.** `slide_pos` returned
//!    false unless the clicked square was orthogonally adjacent, so clicking
//!    the far end of the gap's own row — the one gesture every physical
//!    sliding puzzle supports, and the reason a row of tiles is a row — did
//!    nothing at all. A click now pushes the whole run.
//! 5. **The only pointer control was the board.** Size, new game, numbers and
//!    help were keyboard-only, while the help sheet listed "Click — Slide
//!    tile" as though the pointer were a first-class input. Every control is
//!    now clickable, including the best-scores rows, which switch to the size
//!    they name.
//! 6. **`timer_ticks` was written once and read by nothing.** Its comment
//!    called it a "rough frame counter for animation"; there was no animation.
//! 7. **Twelve blanket `#![allow]`s sat on lines 1-12**, `dead_code` and
//!    `unused_imports` among them, which is what had kept 6 and 8 invisible.
//! 8. **`SURFACE1` and `SURFACE2` were declared and never used.**
//! 9. **A shuffle could hand back a solved board.** `new_game` walked at
//!    random and never checked the result, so a walk that came home left you
//!    looking at a finished puzzle in `Playing` — and the first move you made
//!    would break it. It now reshuffles, but a bounded number of times: a
//!    board that *cannot* be unsolved must not turn the guard into the same
//!    unbounded loop the shuffle itself already had to be rescued from.
//! 10. **Nothing said how close you were.** The move counter only goes up, so
//!     the program could tell you that you had made 300 moves and not that you
//!     were two tiles from home. There is now a floor on the moves remaining —
//!     the sum over tiles of the distance each still has to travel, which no
//!     sequence of moves can beat, because one move moves one tile one square.
//!     It is drawn as a floor and named as one; calling an estimate a
//!     prediction is how a progress bar comes to lie.
//! 11. **Every launch dealt the same board.** The only seed in the program was
//!     `SeededRng::new(42)`, so the 4x4 you were shown the first time is the
//!     4x4 you would be shown every time after — and a "best moves" record
//!     kept against a single fixed scramble is a record for one puzzle, not
//!     for a size. It seeds from the system now, falling back to a constant
//!     only when there is nothing to seed from; `with_seed` stays so the tests
//!     can still be deterministic without the program being.
//!
//! The scores are the fewest moves a size has been finished in. They are not
//! comparable between scrambles — a shuffle that lands eight moves from home
//! is not the same puzzle as one that lands sixty — so the board also shows
//! the floor it started from, which is the only honest way to read a score.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use randrange::{RandomSource, SeededRng};
use std::process::ExitCode;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
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
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const TEAL: Color = Color::from_hex(0x94E2D5);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// The tile palette, cycled by value. Purely decorative — nothing reads a
/// tile's colour to decide anything.
const TILE_COLORS: [Color; 8] = [BLUE, GREEN, PEACH, MAUVE, TEAL, YELLOW, RED, LAVENDER];

const WINDOW_WIDTH: f32 = 720.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// The board sizes offered, smallest first. `SIZES[i]` is the size behind
/// score row `i` and behind the `i`th size button.
pub const SIZES: [usize; 3] = [3, 4, 5];

const HELP_TITLE: &str = "How to play";
/// The controls, as the sheet states them.
///
/// A test walks this list and checks each named key actually answers, and that
/// no key the program answers is missing from it. The row used to read "H or
/// Esc — Open or close this sheet", which was not true in either direction:
/// Escape never opened the sheet, and Enter and Space closed it without being
/// mentioned anywhere.
const HELP_ROWS: [(&str, &str); 8] = [
    ("Goal", "Put the tiles back in order, gap last"),
    ("Arrows", "Slide a tile into the gap"),
    ("Click", "Slide a tile, or a whole row, into the gap"),
    ("3 / 4 / 5", "Change the board size"),
    ("N", "New game"),
    ("T", "Show or hide the tile numbers"),
    ("H", "Open or close this sheet"),
    ("Esc, Enter, Space", "Close this sheet"),
];

// ── Direction ──────────────────────────────────────────────────────────────

/// The direction a *tile* travels. `Up` slides the tile below the gap upwards,
/// which is what the `Up` arrow does and what a player means by it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// The move that undoes this one.
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// All four, in a fixed order, for iteration.
    pub const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];
}

// ── Board ──────────────────────────────────────────────────────────────────

/// A square board of `size * size` squares, one of which is the gap.
///
/// `tiles[i] == 0` marks the gap; every other square holds its face value, and
/// the solved board reads `1, 2, … n-1, 0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    size: usize,
    tiles: Vec<u8>,
    gap: usize,
}

/// The row and column of square `index` on a board `size` wide.
///
/// `Board::new` clamps the size to at least one, so the divisor is never zero
/// in this program. It is still written with `checked_*` rather than `/` and
/// `%`, because a lint that has to take "never zero" on trust is a lint that
/// has stopped checking — and the fallback, square zero, is a real square, so
/// a caller that somehow got here with a zero size draws a wrong tile instead
/// of killing the window.
fn row_col(index: usize, size: usize) -> (usize, usize) {
    match (index.checked_div(size), index.checked_rem(size)) {
        (Some(row), Some(col)) => (row, col),
        _ => (0, 0),
    }
}

impl Board {
    /// The solved board of the given size.
    ///
    /// A size of zero would make a board with no squares and no gap, so it is
    /// rounded up to one: every method below assumes `gap` indexes `tiles`.
    #[must_use]
    pub fn new(size: usize) -> Self {
        let size = size.max(1);
        let total = size.saturating_mul(size);
        let mut tiles = Vec::with_capacity(total);
        for i in 1..total {
            tiles.push(u8::try_from(i).unwrap_or(u8::MAX));
        }
        tiles.push(0);
        Self {
            size,
            tiles,
            gap: total.saturating_sub(1),
        }
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    #[must_use]
    pub fn tiles(&self) -> &[u8] {
        &self.tiles
    }

    /// The index of the gap.
    #[must_use]
    pub fn gap(&self) -> usize {
        self.gap
    }

    #[must_use]
    pub fn is_solved(&self) -> bool {
        let total = self.tiles.len();
        self.tiles.iter().enumerate().all(|(i, &t)| {
            t == if i.saturating_add(1) == total {
                0
            } else {
                goal_value(i)
            }
        })
    }

    #[must_use]
    pub fn gap_row(&self) -> usize {
        row_col(self.gap, self.size).0
    }

    #[must_use]
    pub fn gap_col(&self) -> usize {
        row_col(self.gap, self.size).1
    }

    /// Whether a tile can travel `dir` into the gap — that is, whether there is
    /// a square on the far side of the gap for it to come from.
    #[must_use]
    pub fn can_slide(&self, dir: Direction) -> bool {
        match dir {
            Direction::Up => self.gap_row().saturating_add(1) < self.size,
            Direction::Down => self.gap_row() > 0,
            Direction::Left => self.gap_col().saturating_add(1) < self.size,
            Direction::Right => self.gap_col() > 0,
        }
    }

    /// Slide one tile `dir` into the gap. Returns whether it moved.
    pub fn slide(&mut self, dir: Direction) -> bool {
        if !self.can_slide(dir) {
            return false;
        }
        let from = match dir {
            Direction::Up => self.gap.saturating_add(self.size),
            Direction::Down => self.gap.saturating_sub(self.size),
            Direction::Left => self.gap.saturating_add(1),
            Direction::Right => self.gap.saturating_sub(1),
        };
        if from >= self.tiles.len() {
            return false;
        }
        self.tiles.swap(self.gap, from);
        self.gap = from;
        true
    }

    /// Push the tile at `index`, and every tile between it and the gap, one
    /// square towards the gap. Returns how many tiles moved.
    ///
    /// The old code required the clicked square to be orthogonally adjacent to
    /// the gap and returned false otherwise, so clicking the far end of the
    /// gap's own row did nothing — the one gesture every physical sliding
    /// puzzle supports. A run is a run of legal single moves, so nothing about
    /// the rules changes; each tile it moves counts as a move, because each
    /// tile did move.
    pub fn push_to(&mut self, index: usize) -> usize {
        if index >= self.tiles.len() || index == self.gap {
            return 0;
        }
        let (row, col) = row_col(index, self.size);
        let (grow, gcol) = (self.gap_row(), self.gap_col());
        let (dir, count) = if row == grow {
            let dir = if col > gcol {
                Direction::Left
            } else {
                Direction::Right
            };
            (dir, col.abs_diff(gcol))
        } else if col == gcol {
            let dir = if row > grow {
                Direction::Up
            } else {
                Direction::Down
            };
            (dir, row.abs_diff(grow))
        } else {
            // Not in line with the gap: no run of legal moves reaches it, and
            // guessing which way the player meant would move tiles they did
            // not click on.
            return 0;
        };
        let mut moved = 0_usize;
        for _ in 0..count {
            if !self.slide(dir) {
                break;
            }
            moved = moved.saturating_add(1);
        }
        moved
    }

    /// A floor on the moves still needed: the total distance the tiles have
    /// left to travel.
    ///
    /// One move moves one tile one square, so no solution can be shorter than
    /// this sum — it is a bound, not a guess, and it is drawn as one. The gap
    /// is not counted: it has no home to be away from, and counting it would
    /// break the bound.
    #[must_use]
    pub fn distance_floor(&self) -> u32 {
        let mut total = 0_u32;
        for (i, &tile) in self.tiles.iter().enumerate() {
            if tile == 0 {
                continue;
            }
            let home = usize::from(tile).saturating_sub(1);
            let (r, c) = row_col(i, self.size);
            let (hr, hc) = row_col(home, self.size);
            let d = r.abs_diff(hr).saturating_add(c.abs_diff(hc));
            total = total.saturating_add(u32::try_from(d).unwrap_or(u32::MAX));
        }
        total
    }

    /// Scramble by walking at random over legal moves, which is what keeps the
    /// result solvable: every position reachable from the solved board is a
    /// position the solved board is reachable from.
    ///
    /// The attempt cap is not cosmetic. A board with no legal move at all — a
    /// 1x1 — made this an unbounded `loop` that never slid and never exited,
    /// hanging the program rather than producing a bad shuffle. Bounded, it
    /// simply makes no move, which is the only honest answer for a board that
    /// cannot be shuffled.
    pub fn shuffle(&mut self, rng: &mut SeededRng, moves: usize) {
        let mut last: Option<Direction> = None;
        for _ in 0..moves {
            let mut attempts = 0_u32;
            while attempts < 1000 {
                attempts = attempts.saturating_add(1);
                let Some(&dir) = rng.choose(&Direction::ALL) else {
                    break;
                };
                // Undoing the previous move immediately wastes half the walk.
                if last == Some(dir.opposite()) {
                    continue;
                }
                if self.slide(dir) {
                    last = Some(dir);
                    break;
                }
            }
        }
    }
}

/// The tile that belongs at `index` on a solved board, for every index but the
/// last.
fn goal_value(index: usize) -> u8 {
    u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX)
}

// ── Targets and actions ────────────────────────────────────────────────────

/// Something on the screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A square of the board, by index.
    Tile(usize),
    /// One of the three size buttons, by index into [`SIZES`].
    Size(usize),
    /// A row of the best-scores panel, which also switches to that size.
    Score(usize),
    NewGame,
    ToggleNumbers,
    ToggleHelp,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Everything the game can be asked to do, from either input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Slide(Direction),
    /// Push the run of tiles between `index` and the gap.
    PushTo(usize),
    NewGame,
    SetSize(usize),
    ToggleNumbers,
    ToggleHelp,
    CloseHelp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameState {
    Playing,
    Won,
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the board keeps no matter what.
const BOARD_SHARE: f32 = 0.52;

/// Which band goes first when they do not all fit: scores, header, controls,
/// info.
///
/// Bands are dropped whole rather than shrunk together, because a band scaled
/// to four pixels costs the board four pixels and shows nothing. The scores go
/// first — they are a record of games already finished. The info line goes
/// last: the move count and the moves-remaining floor are the only chrome you
/// cannot read the game without.
const BAND_DROP_ORDER: [usize; 4] = [3, 0, 2, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in — which
/// is exactly how a click at (60, 90) came to move a tile in a window where
/// the board had never been drawn there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// The title.
    pub header: Rect,
    /// Moves made, moves left at the very least, and the size.
    pub info: Rect,
    /// The square the tiles are drawn in.
    pub board: Rect,
    /// The size buttons, new game, numbers and help.
    pub controls: Rect,
    /// One cell per size, showing the fewest moves it has been finished in.
    pub scores: Rect,
    pub help: Rect,
    pub font: f32,
    pub small: f32,
    pub big: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 38.0).clamp(8.0, 17.0);
        let small = (font - 2.0).max(7.0);
        let big = (font * 1.6).clamp(13.0, 28.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, info, controls, scores] order.
        let mut wants = [
            (h * 0.085).clamp(22.0, 44.0),
            (h * 0.055).clamp(16.0, 28.0),
            (h * 0.08).clamp(22.0, 40.0),
            (h * 0.07).clamp(18.0, 34.0),
        ];
        // What is left for chrome once the board has its share *and* the gap
        // that separates it from the chrome above and below. The padding comes
        // out of this side: charging it to the board turns a promised half
        // window into rather less than half of a small one, which is where the
        // tile numbers stop fitting inside their tiles.
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, inf_h, ctl_h, sco_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall. The two read the same to `shows`, but only one of them reads
        // the same to anything asking "is this band gone, or merely thin?" — a
        // rectangle that answers "I am 120 pixels wide" while drawing nothing
        // is one that will eventually be believed.
        let header = if hdr_h > 0.0 {
            Rect::new(0.0, 0.0, w, hdr_h)
        } else {
            Rect::EMPTY
        };
        let info = if inf_h > 0.0 {
            Rect::new(0.0, hdr_h, w, inf_h)
        } else {
            Rect::EMPTY
        };
        // The two lower bands stack up from the bottom edge, so dropping either
        // gives its height straight back to the board.
        let scores = if sco_h > 0.0 {
            Rect::new(0.0, h - sco_h, w, sco_h)
        } else {
            Rect::EMPTY
        };
        let lower = if sco_h > 0.0 { scores.y } else { h };
        let controls = if ctl_h > 0.0 {
            Rect::new(0.0, lower - ctl_h, w, ctl_h)
        } else {
            Rect::EMPTY
        };

        // From the heights, not from `info.bottom()`: a dropped info band is
        // `Rect::EMPTY`, whose bottom is zero, and reading it would put the
        // board back over the header.
        let top = hdr_h + inf_h;
        let bottom = if ctl_h > 0.0 { controls.y } else { lower };
        let band = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );
        // The board is square and centred in whatever is left. A 4x4 grid in a
        // non-square rectangle either stretches its tiles or leaves them where
        // the hit test is not; squaring it here means neither.
        let side = band.w.min(band.h).max(0.0);
        let board = Rect::new(
            band.x + (band.w - side) / 2.0,
            band.y + (band.h - side) / 2.0,
            side,
            side,
        );

        let help_w = (w * 0.92).min(420.0);
        let help_h = (h * 0.92).min(300.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            board,
            controls,
            scores,
            help,
            font,
            small,
            big,
            pad,
        }
    }

    /// Whether a band is tall and wide enough for its text to be worth drawing.
    #[must_use]
    pub fn shows(&self, band: Rect) -> bool {
        band.h >= 11.0 && band.w >= 110.0
    }

    /// The side of one square of a `size`-by-`size` board.
    #[must_use]
    pub fn cell(&self, size: usize) -> f32 {
        if size == 0 {
            return 0.0;
        }
        // `size` is 1..=5 here, so the cast is exact.
        self.board.w / size as f32
    }

    /// The rectangle of board square `index`.
    #[must_use]
    pub fn square(&self, size: usize, index: usize) -> Rect {
        if size == 0 || index >= size.saturating_mul(size) {
            return Rect::EMPTY;
        }
        let cell = self.cell(size);
        let (row, col) = row_col(index, size);
        Rect::new(
            self.board.x + col as f32 * cell,
            self.board.y + row as f32 * cell,
            cell,
            cell,
        )
    }

    /// The `slot`th of `count` buttons spread evenly across `band`.
    #[must_use]
    pub fn button(&self, band: Rect, slot: usize, count: usize) -> Rect {
        if count == 0 || slot >= count || band.w <= 0.0 {
            return Rect::EMPTY;
        }
        let gap = self.pad * 0.5;
        let total = band.w - self.pad * 2.0;
        let each = ((total - gap * (count.saturating_sub(1)) as f32) / count as f32).max(0.0);
        let h = (band.h - self.pad * 0.5).max(0.0);
        Rect::new(
            band.x + self.pad + slot as f32 * (each + gap),
            band.y + (band.h - h) / 2.0,
            each,
            h,
        )
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

/// How many random moves a size is scrambled with.
fn shuffle_moves(size: usize) -> usize {
    match size {
        3 => 100,
        5 => 400,
        _ => 200,
    }
}

/// How many walks a scramble may take before it settles for what it has.
const SCRAMBLE_ATTEMPTS: usize = 8;

/// Scramble `board`, walking again if the walk came home.
///
/// A walk that returns to the solved board leaves a finished puzzle presented
/// as an unstarted one, and the first move you make breaks it — that was fault
/// nine. Walking again fixes it; walking *until* unsolved would hang forever on
/// a board that cannot be unsolved at all (a 1x1 has no legal move), which is
/// the same unbounded loop the walk itself already had to be rescued from. So
/// the retry is bounded, and a board that stays solved is accepted and said so.
///
/// A free function rather than a method on the game, so a test can drive it
/// with a walk short enough to actually come home — through the game it would
/// take a hundred-move walk returning to the start, which never happens and so
/// would never test the guard at all.
fn scramble_board(board: &mut Board, rng: &mut SeededRng, walk: usize) {
    for _ in 0..SCRAMBLE_ATTEMPTS {
        board.shuffle(rng, walk);
        if !board.is_solved() {
            break;
        }
    }
}

/// The game.
pub struct SlidingPuzzle {
    board: Board,
    state: GameState,
    moves: u32,
    size: usize,
    /// The floor the current scramble started from, so a score can be read
    /// against the puzzle it was set on.
    opening_floor: u32,
    best: [Option<u32>; SIZES.len()],
    rng: SeededRng,
    show_numbers: bool,
    show_help: bool,
    status: String,
    size_drawn: (f32, f32),
}

impl SlidingPuzzle {
    #[must_use]
    pub fn new() -> Self {
        Self::with_seed(randrange::seed_from_system(0x5117_D1E5))
    }

    /// The game with a named seed, so a test can name the scramble it means.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        let mut game = Self {
            board: Board::new(4),
            state: GameState::Playing,
            moves: 0,
            size: 4,
            opening_floor: 0,
            best: [None; SIZES.len()],
            rng: SeededRng::new(seed),
            show_numbers: true,
            show_help: false,
            status: String::new(),
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        game.new_game();
        game
    }

    // ── Readers ────────────────────────────────────────────────────────────

    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size_drawn.0, self.size_drawn.1)
    }

    #[must_use]
    pub fn board(&self) -> &Board {
        &self.board
    }

    #[must_use]
    pub fn state(&self) -> GameState {
        self.state
    }

    #[must_use]
    pub fn moves(&self) -> u32 {
        self.moves
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    #[must_use]
    pub fn opening_floor(&self) -> u32 {
        self.opening_floor
    }

    #[must_use]
    pub fn best(&self) -> [Option<u32>; SIZES.len()] {
        self.best
    }

    #[must_use]
    pub fn show_numbers(&self) -> bool {
        self.show_numbers
    }

    #[must_use]
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The index into [`SIZES`] of the size being played, or `None` for a size
    /// not on the list.
    #[must_use]
    pub fn size_index(&self) -> Option<usize> {
        SIZES.iter().position(|&s| s == self.size)
    }

    /// The floor on the moves still to make. Zero exactly when solved.
    #[must_use]
    pub fn floor_left(&self) -> u32 {
        self.board.distance_floor()
    }

    // ── Play ───────────────────────────────────────────────────────────────

    /// Start a fresh scramble at the current size.
    pub fn new_game(&mut self) {
        self.board = Board::new(self.size);
        scramble_board(&mut self.board, &mut self.rng, shuffle_moves(self.size));
        self.state = GameState::Playing;
        self.moves = 0;
        self.opening_floor = self.board.distance_floor();
        self.status = if self.board.is_solved() {
            "This board cannot be scrambled".to_string()
        } else {
            format!("{} moves at the very least", self.opening_floor)
        };
    }

    fn set_size(&mut self, size: usize) {
        if !SIZES.contains(&size) {
            return;
        }
        if size == self.size {
            self.status = format!("Already {size}x{size}");
            return;
        }
        self.size = size;
        self.new_game();
    }

    fn after_move(&mut self, moved: u32) {
        if moved == 0 {
            return;
        }
        self.moves = self.moves.saturating_add(moved);
        if self.board.is_solved() {
            self.state = GameState::Won;
            if let Some(i) = self.size_index()
                && let Some(slot) = self.best.get_mut(i)
            {
                if slot.is_none_or(|b| self.moves < b) {
                    *slot = Some(self.moves);
                }
            }
            self.status = format!("Solved in {} moves", self.moves);
        } else {
            self.status = format!("{} moves at the very least", self.floor_left());
        }
    }

    /// The one place an action changes the game, so a key and a click that mean
    /// the same thing cannot come to mean different things.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Slide(dir) => {
                if self.state == GameState::Playing && self.board.slide(dir) {
                    self.after_move(1);
                } else if self.state == GameState::Playing {
                    self.status = "Nothing to slide that way".to_string();
                }
            }
            Action::PushTo(index) => {
                if self.state == GameState::Playing {
                    let moved = self.board.push_to(index);
                    if moved == 0 {
                        self.status = "That tile is not in line with the gap".to_string();
                    } else {
                        self.after_move(u32::try_from(moved).unwrap_or(u32::MAX));
                    }
                }
            }
            Action::NewGame => self.new_game(),
            Action::SetSize(size) => self.set_size(size),
            Action::ToggleNumbers => {
                self.show_numbers = !self.show_numbers;
                self.status = if self.show_numbers {
                    "Numbers on".to_string()
                } else {
                    "Numbers off \u{2014} press T to bring them back".to_string()
                };
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                self.status = if self.show_help {
                    HELP_TITLE.to_string()
                } else {
                    format!("{} moves at the very least", self.floor_left())
                };
            }
            Action::CloseHelp => {
                if self.show_help {
                    self.show_help = false;
                    self.status = format!("{} moves at the very least", self.floor_left());
                }
            }
        }
    }

    // ── Input ──────────────────────────────────────────────────────────────

    fn key_action(&self, ev: &KeyEvent) -> Option<Action> {
        // A key that is *coming back up* is not a second press. Reading only
        // the field that says which is which is fault one: with the release
        // treated as a press, help opened and shut in one keystroke and the
        // numbers could not be turned off.
        if !ev.pressed {
            return None;
        }
        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {
            return None;
        }
        // With the sheet open the only keys that mean anything are the ones
        // that close it: a sheet you can play behind is a sheet in the way.
        if self.show_help {
            return match ev.key {
                Key::H | Key::Escape | Key::Enter | Key::Space => Some(Action::CloseHelp),
                _ => None,
            };
        }
        match ev.key {
            Key::Up => Some(Action::Slide(Direction::Up)),
            Key::Down => Some(Action::Slide(Direction::Down)),
            Key::Left => Some(Action::Slide(Direction::Left)),
            Key::Right => Some(Action::Slide(Direction::Right)),
            Key::N => Some(Action::NewGame),
            Key::T => Some(Action::ToggleNumbers),
            Key::H => Some(Action::ToggleHelp),
            Key::Num3 => Some(Action::SetSize(3)),
            Key::Num4 => Some(Action::SetSize(4)),
            Key::Num5 => Some(Action::SetSize(5)),
            _ => None,
        }
    }

    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        match self.key_action(ev) {
            Some(action) => {
                self.apply(action);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// The target under a point, read from the frame the window last drew.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.size_drawn.0, self.size_drawn.1)
            .hit_test(x, y)
    }

    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        // No special case for the open sheet. `draw_help` records one hit box
        // over the whole window, last, so the ordinary hit test already answers
        // `ToggleHelp` for every point while the sheet is up. A short-circuit
        // here would work too, but it would leave the frame's account of what
        // is clickable a thing the event path quietly disagrees with — and it
        // was what left the sheet's own hit box unreachable.
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Tile(index) => self.apply(Action::PushTo(index)),
            Target::Size(i) | Target::Score(i) => {
                if let Some(&size) = SIZES.get(i) {
                    self.apply(Action::SetSize(size));
                }
            }
            Target::NewGame => self.apply(Action::NewGame),
            Target::ToggleNumbers => self.apply(Action::ToggleNumbers),
            Target::ToggleHelp => self.apply(Action::ToggleHelp),
        }
        EventResult::Consumed
    }

    /// Remember the size the window is being drawn at, so the next click is
    /// read against the pixels the player is actually looking at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }

    // ── Drawing ────────────────────────────────────────────────────────────

    /// The frame for a window of the given size, hit boxes and all.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_info(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_controls(&mut f, &l);
        self.draw_scores(&mut f, &l);
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn title(&self) -> &'static str {
        match self.size {
            3 => "8-Puzzle",
            5 => "24-Puzzle",
            _ => "15-Puzzle",
        }
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        label(
            f,
            l.header.x + l.pad,
            l.header.y + (l.header.h - text::line_height(l.big, FontWeightHint::Bold)) / 2.0,
            self.title(),
            l.big.min(l.header.h * 0.8),
            LAVENDER,
            FontWeightHint::Bold,
            Some((l.header.w - l.pad * 2.0).max(0.0)),
        );
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.info) {
            return;
        }
        let colour = if self.state == GameState::Won {
            GREEN
        } else {
            SUBTEXT0
        };
        label(
            f,
            l.info.x + l.pad,
            l.info.y + (l.info.h - text::line_height(l.font, FontWeightHint::Regular)) / 2.0,
            &format!(
                "{}x{} \u{2014} {} moves \u{2014} {}",
                self.size, self.size, self.moves, self.status
            ),
            l.font.min(l.info.h * 0.85),
            colour,
            FontWeightHint::Regular,
            Some((l.info.w - l.pad * 2.0).max(0.0)),
        );
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.w <= 0.0 || l.board.h <= 0.0 {
            return;
        }
        fill(f, l.board, CRUST, (l.board.w * 0.02).min(10.0));
        let cell = l.cell(self.size);
        let inset = (cell * 0.06).clamp(0.5, 5.0);
        for (index, &value) in self.board.tiles().iter().enumerate() {
            let square = l.square(self.size, index);
            let inner = Rect::new(
                square.x + inset,
                square.y + inset,
                (square.w - inset * 2.0).max(0.0),
                (square.h - inset * 2.0).max(0.0),
            );
            let radius = (inner.w * 0.12).min(8.0);
            if value == 0 {
                fill(f, inner, MANTLE, radius);
            } else {
                let home = usize::from(value).saturating_sub(1) == index;
                fill(f, inner, tile_color(value), radius);
                if home {
                    // A tile already home gets an outline rather than a
                    // different fill: the fill is what tells tiles apart, and
                    // recolouring it would make "home" and "seven" the same
                    // signal.
                    stroke(f, inner, CRUST, (inner.w * 0.05).clamp(1.0, 3.0), radius);
                }
                if self.show_numbers && inner.w > 8.0 {
                    let size = (inner.h * 0.42).clamp(7.0, 34.0);
                    centred_in(
                        f,
                        inner,
                        &value.to_string(),
                        size,
                        CRUST,
                        FontWeightHint::Bold,
                    );
                }
            }
            // Recorded for the gap too: a click on it is a click that means
            // "nothing to push here", and answering that is more use than
            // letting it fall through to the background.
            f.hit(Target::Tile(index), square);
        }
    }

    fn draw_controls(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.controls) {
            return;
        }
        // Three sizes, new game, numbers, help.
        let count = SIZES.len().saturating_add(3);
        for (slot, &size) in SIZES.iter().enumerate() {
            let r = l.button(l.controls, slot, count);
            let active = size == self.size;
            button(
                f,
                l,
                r,
                &format!("{size}x{size}"),
                if active { SURFACE1 } else { SURFACE0 },
                if active { LAVENDER } else { SUBTEXT0 },
            );
            f.hit(Target::Size(slot), r);
        }
        let new_r = l.button(l.controls, SIZES.len(), count);
        button(f, l, new_r, "New", SURFACE0, TEAL);
        f.hit(Target::NewGame, new_r);

        let num_r = l.button(l.controls, SIZES.len().saturating_add(1), count);
        button(
            f,
            l,
            num_r,
            if self.show_numbers {
                "123"
            } else {
                "\u{2014}\u{2014}\u{2014}"
            },
            if self.show_numbers {
                SURFACE1
            } else {
                SURFACE0
            },
            if self.show_numbers { PEACH } else { OVERLAY0 },
        );
        f.hit(Target::ToggleNumbers, num_r);

        let help_r = l.button(l.controls, SIZES.len().saturating_add(2), count);
        button(f, l, help_r, "?", SURFACE0, YELLOW);
        f.hit(Target::ToggleHelp, help_r);
    }

    fn draw_scores(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.scores) {
            return;
        }
        for (i, &size) in SIZES.iter().enumerate() {
            let r = l.button(l.scores, i, SIZES.len());
            let text = match self.best.get(i).copied().flatten() {
                Some(m) => format!("{size}x{size}: {m}"),
                None => format!("{size}x{size}: \u{2014}"),
            };
            button(
                f,
                l,
                r,
                &text,
                SURFACE0,
                if size == self.size { YELLOW } else { SUBTEXT0 },
            );
            f.hit(Target::Score(i), r);
        }
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        let sheet = l.help;
        if sheet.w <= 0.0 || sheet.h <= 0.0 {
            return;
        }
        // One hit box over the *whole window*, recorded after every control the
        // sheet covers, so the last box at any point is this one.
        //
        // It has to be the window and not merely the sheet. The sheet is opaque
        // and sits on top of controls that go on recording their own boxes
        // underneath it, so a frame that still answered `Tile(5)` for a point
        // buried under the help text would be describing a screen nobody is
        // looking at. Covering everything keeps the frame's account of what is
        // clickable true, which in turn lets the ordinary hit test handle a
        // modal sheet with no special case anywhere: the earlier version
        // short-circuited in `handle_mouse` instead, which worked, but left the
        // frame lying *and* left this hit box unreachable — a control wired to
        // nothing, which is the fault this rewrite exists to remove.
        f.hit(Target::ToggleHelp, l.window);
        fill(f, sheet, SURFACE0, (sheet.w * 0.03).min(12.0));
        stroke(f, sheet, LAVENDER, 1.5, (sheet.w * 0.03).min(12.0));

        let pad = l.pad;
        let line = (sheet.h - pad * 3.0) / (HELP_ROWS.len().saturating_add(1)) as f32;
        if line <= 0.0 {
            return;
        }
        let size = (line * 0.62).clamp(6.0, l.font);
        label(
            f,
            sheet.x + pad,
            sheet.y + pad,
            HELP_TITLE,
            (line * 0.7).clamp(7.0, l.big),
            YELLOW,
            FontWeightHint::Bold,
            Some((sheet.w - pad * 2.0).max(0.0)),
        );
        let key_w = (sheet.w * 0.3).max(0.0);
        for (i, (key, what)) in HELP_ROWS.iter().enumerate() {
            let y = sheet.y + pad * 2.0 + line * (i.saturating_add(1)) as f32;
            if y + size > sheet.bottom() {
                break;
            }
            label(
                f,
                sheet.x + pad,
                y,
                key,
                size,
                BLUE,
                FontWeightHint::Bold,
                Some(key_w),
            );
            label(
                f,
                sheet.x + pad + key_w,
                y,
                what,
                size,
                TEXT_COLOR,
                FontWeightHint::Regular,
                Some((sheet.w - pad * 2.0 - key_w).max(0.0)),
            );
        }
    }
}

impl Default for SlidingPuzzle {
    fn default() -> Self {
        Self::new()
    }
}

fn tile_color(value: u8) -> Color {
    if value == 0 {
        return MANTLE;
    }
    let i = usize::from(value)
        .saturating_sub(1)
        .checked_rem(TILE_COLORS.len())
        .unwrap_or(0);
    TILE_COLORS.get(i).copied().unwrap_or(BLUE)
}

// ── Drawing helpers ────────────────────────────────────────────────────────

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
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
    if r.w <= 0.0 || r.h <= 0.0 {
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

#[allow(clippy::too_many_arguments)]
fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    text_str: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if size <= 0.0 || text_str.is_empty() || max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: text_str.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

/// A string centred in `r`, horizontally and vertically.
fn centred_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.w <= 0.0 || r.h <= 0.0 || size <= 0.0 {
        return;
    }
    let w = text::measure(s, size, weight);
    let line = text::line_height(size, weight);
    label(
        f,
        r.x + (r.w - w) / 2.0,
        r.y + (r.h - line) / 2.0,
        s,
        size,
        color,
        weight,
        Some(r.w),
    );
}

/// A filled, labelled control.
fn button(f: &mut Frame, l: &Layout, r: Rect, text_str: &str, back: Color, fore: Color) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    fill(f, r, back, (r.h * 0.25).min(8.0));
    let size = (r.h * 0.5).clamp(6.0, l.font);
    centred_in(f, r, text_str, size, fore, FontWeightHint::Bold);
}

// ── Window ─────────────────────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(game: &mut SlidingPuzzle, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => game.handle_key(ev),
        Event::Mouse(ev) => game.handle_mouse(ev),
        Event::Resize { width, height } => {
            game.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for SlidingPuzzle {
    fn title(&self) -> String {
        "Sliding Puzzle".to_string()
    }

    fn app_id(&self) -> String {
        "sliding".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
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
        // against — that is the whole point of storing it here.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for SlidingPuzzle {
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
    let mut game = SlidingPuzzle::new();
    app::launch("sliding", &mut game)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;
    use std::collections::{HashMap, HashSet, VecDeque};

    /// The window sizes every layout claim is checked at.
    ///
    /// The first three are smaller than the chrome would like, which is the
    /// case the old layout — a grid of 80px cells at a fixed (50, 80), with a
    /// scores panel pinned at `50 + 80 * size + 40` — never had to survive,
    /// because `render` used its `width` and `height` arguments for the
    /// background rectangle and nothing else.
    const WINDOWS: &[(f32, f32)] = &[
        (120.0, 90.0),
        (200.0, 160.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (640.0, 480.0),
        (720.0, 620.0),
        (900.0, 500.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    /// A seed whose 4x4 scramble is nothing special — the default one a test
    /// means when it does not care which scramble it gets.
    const SEED: u64 = 0x0BAD_1DEA;

    fn game() -> SlidingPuzzle {
        SlidingPuzzle::with_seed(SEED)
    }

    fn sized(size: (f32, f32)) -> SlidingPuzzle {
        let mut g = game();
        g.resize(size.0, size.1);
        g
    }

    /// A game at the given board size, freshly scrambled.
    fn at_size(size: usize) -> SlidingPuzzle {
        let mut g = game();
        g.apply(Action::SetSize(size));
        assert_eq!(g.size(), size, "the size did not take");
        g
    }

    /// Everything a test can see of the state, in one string.
    ///
    /// Used to assert that a control *did* something: a recorded hit box that
    /// changes nothing is worse than no hit box at all, because it swallows
    /// the click instead of letting it fall through to whatever is underneath.
    fn describe(g: &SlidingPuzzle) -> String {
        format!(
            "{:?}|{:?}|{}|{}|{:?}|{}|{}|{}",
            g.board().tiles(),
            g.state(),
            g.moves(),
            g.size(),
            g.best(),
            g.show_numbers(),
            g.show_help(),
            g.status()
        )
    }

    /// Every rectangle the frame paints. A `Text` is reported as the zero-sized
    /// point it starts at, because its width is the renderer's business.
    fn painted(g: &SlidingPuzzle, w: f32, h: f32) -> Vec<Rect> {
        g.frame(w, h)
            .commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                }
                | RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(x, y, width, height)),
                RenderCommand::Text { x, y, .. } => Some(Rect::new(x, y, 0.0, 0.0)),
                _ => None,
            })
            .collect()
    }

    fn texts(g: &SlidingPuzzle, size: (f32, f32)) -> Vec<String> {
        g.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn shows(g: &SlidingPuzzle, size: (f32, f32), needle: &str) -> bool {
        texts(g, size).iter().any(|t| t.contains(needle))
    }

    /// Which of the four bands a layout draws, in `[header, info, controls,
    /// scores]` order — the order [`BAND_DROP_ORDER`] indexes.
    fn bands(l: &Layout) -> [bool; 4] {
        [
            l.shows(l.header),
            l.shows(l.info),
            l.shows(l.controls),
            l.shows(l.scores),
        ]
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.right() - 0.01
            && b.x < a.right() - 0.01
            && a.y < b.bottom() - 0.01
            && b.y < a.bottom() - 0.01
    }

    /// A key going down and then coming back up, which is what a real keyboard
    /// sends and what fault one turned into two presses.
    fn tap(g: &mut SlidingPuzzle, key: Key) {
        let size = (g.layout().window.w, g.layout().window.h);
        let down = probe::press(key);
        let up = KeyEvent {
            pressed: false,
            text: String::new(),
            ..down.clone()
        };
        g.key_at(&down, size);
        g.key_at(&up, size);
    }

    /// A direction this board can actually slide.
    ///
    /// Every board of more than one square has one. Asserting about `Up` in
    /// particular is asserting about where a seed happened to leave the gap,
    /// which is not what any of these tests are about.
    fn any_legal(board: &Board) -> Direction {
        Direction::ALL
            .into_iter()
            .find(|&d| board.can_slide(d))
            .expect("a scrambled board with no legal move")
    }

    fn arrow_for(dir: Direction) -> Key {
        match dir {
            Direction::Up => Key::Up,
            Direction::Down => Key::Down,
            Direction::Left => Key::Left,
            Direction::Right => Key::Right,
        }
    }

    /// The shortest solution for `start`, found by breadth-first search.
    ///
    /// An oracle, not a copy of the program: it knows only that a move is a
    /// legal slide and that the goal is [`Board::is_solved`], so the length it
    /// returns is an independent check on the moves-remaining floor the game
    /// draws. Only ever asked about 3x3 boards — 181440 reachable positions,
    /// which is a fraction of a second; a 4x4 has 10 trillion.
    fn shortest_solution(start: &Board) -> Vec<Direction> {
        assert_eq!(start.size(), 3, "the oracle is only affordable on a 3x3");
        let mut came_from: HashMap<Vec<u8>, (Vec<u8>, Direction)> = HashMap::new();
        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        seen.insert(start.tiles().to_vec());
        let mut queue: VecDeque<Board> = VecDeque::new();
        queue.push_back(start.clone());
        while let Some(board) = queue.pop_front() {
            if board.is_solved() {
                let mut path = Vec::new();
                let mut here = board.tiles().to_vec();
                while let Some((prev, dir)) = came_from.get(&here) {
                    path.push(*dir);
                    here = prev.clone();
                }
                path.reverse();
                return path;
            }
            for dir in Direction::ALL {
                let mut next = board.clone();
                if !next.slide(dir) || !seen.insert(next.tiles().to_vec()) {
                    continue;
                }
                came_from.insert(next.tiles().to_vec(), (board.tiles().to_vec(), dir));
                queue.push_back(next);
            }
        }
        panic!("an unsolvable board reached the oracle");
    }

    /// Whether a board is solvable, from the parity rule rather than by search.
    ///
    /// A position is reachable from the solved board exactly when the number of
    /// out-of-order pairs, plus (on an even-sided board) the number of rows the
    /// gap is from the bottom, is even. Standard, provable, and entirely
    /// independent of how this program shuffles — which is the point: it can
    /// catch a shuffle that produces a board no sequence of moves can solve.
    fn is_solvable(board: &Board) -> bool {
        let values: Vec<u8> = board.tiles().iter().copied().filter(|&t| t != 0).collect();
        let mut inversions = 0_usize;
        for (i, &a) in values.iter().enumerate() {
            for &b in &values[i + 1..] {
                if a > b {
                    inversions += 1;
                }
            }
        }
        let size = board.size();
        if size.is_multiple_of(2) {
            let from_bottom = size - 1 - board.gap_row();
            (inversions + from_bottom).is_multiple_of(2)
        } else {
            inversions.is_multiple_of(2)
        }
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    #[test]
    fn nothing_is_painted_outside_the_window() {
        for &(w, h) in WINDOWS {
            for help in [false, true] {
                for size in SIZES {
                    let mut g = at_size(size);
                    g.resize(w, h);
                    if help {
                        g.apply(Action::ToggleHelp);
                    }
                    for r in painted(&g, w, h) {
                        assert!(
                            r.x >= -0.5
                                && r.y >= -0.5
                                && r.right() <= w + 0.5
                                && r.bottom() <= h + 0.5,
                            "{size}x{size} (help {help}) paints {r:?} outside {w}x{h}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_board_keeps_its_share_of_every_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            // The board is squared, so in a window wider than it is tall the
            // promise is a share of the *height*, and in a narrow one it is
            // all the width there is. Both are what the player sees.
            let promised = (h * BOARD_SHARE).min(w - l.pad * 2.0);
            assert!(
                l.board.h >= promised - 0.5,
                "{w}x{h}: the board is {} of a promised {promised}",
                l.board.h
            );
            assert!(l.board.w > 0.0, "{w}x{h}: the board has no width");
        }
    }

    #[test]
    fn the_board_is_square_in_every_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                (l.board.w - l.board.h).abs() < 0.01,
                "{w}x{h}: the board is {}x{}, so its tiles are not square",
                l.board.w,
                l.board.h
            );
        }
    }

    #[test]
    fn a_band_is_dropped_whole_and_never_half_drawn() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for band in [l.header, l.info, l.controls, l.scores] {
                assert!(
                    band == Rect::EMPTY || band.h >= 16.0,
                    "{w}x{h}: a band survives at {}px, too short to read",
                    band.h
                );
            }
        }
    }

    #[test]
    fn a_taller_window_never_loses_a_band_a_shorter_one_had() {
        let mut previous = [false; 4];
        // From 20px, not 60: the info band is the last to go and survives at
        // any height a window is likely to have, so a sweep that stops at 60
        // never sees it dropped and cannot say what order it went in.
        for h in 20_u16..1200 {
            let now = bands(&Layout::new(1000.0, f32::from(h)));
            for (i, (&was, &is)) in previous.iter().zip(now.iter()).enumerate() {
                assert!(
                    !was || is,
                    "band {i} was drawn at {}px tall and is gone at {h}px",
                    h - 1
                );
            }
            previous = now;
        }
    }

    #[test]
    fn the_bands_go_in_the_stated_order() {
        // Scores, header, controls, info — written out here rather than read
        // from `BAND_DROP_ORDER`, so reordering the constant fails this test
        // instead of quietly redefining what it checks.
        let mut order: Vec<usize> = Vec::new();
        let mut previous = [true; 4];
        for h in (20_u16..1200).rev() {
            let now = bands(&Layout::new(1000.0, f32::from(h)));
            for (i, (&was, &is)) in previous.iter().zip(now.iter()).enumerate() {
                if was && !is {
                    order.push(i);
                }
            }
            previous = now;
        }
        assert_eq!(order, vec![3, 0, 2, 1], "the bands went in the wrong order");
    }

    #[test]
    fn a_band_too_narrow_to_read_is_not_drawn() {
        // A tall, very narrow window has the height for every band and the
        // width for none of them.
        let l = Layout::new(100.0, 1000.0);
        for band in [l.header, l.info, l.controls, l.scores] {
            assert!(!l.shows(band), "a band drawn across 100px");
        }
    }

    #[test]
    fn every_square_is_inside_the_board_and_they_do_not_overlap() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for size in SIZES {
                let squares: Vec<Rect> = (0..size * size).map(|i| l.square(size, i)).collect();
                for (i, &r) in squares.iter().enumerate() {
                    assert!(
                        r.x >= l.board.x - 0.01
                            && r.y >= l.board.y - 0.01
                            && r.right() <= l.board.right() + 0.01
                            && r.bottom() <= l.board.bottom() + 0.01,
                        "{w}x{h} {size}x{size}: square {i} at {r:?} leaves the board {:?}",
                        l.board
                    );
                    for (j, &other) in squares.iter().enumerate().skip(i + 1) {
                        assert!(
                            !overlaps(r, other),
                            "{w}x{h} {size}x{size}: squares {i} and {j} overlap"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_square_off_the_end_of_the_board_has_no_rectangle() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.square(4, 16), Rect::EMPTY);
        assert_eq!(l.square(0, 0), Rect::EMPTY);
    }

    #[test]
    fn every_button_stays_inside_its_band_and_clear_of_the_others() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (band, count) in [(l.controls, SIZES.len() + 3), (l.scores, SIZES.len())] {
                if !l.shows(band) {
                    continue;
                }
                let rects: Vec<Rect> = (0..count).map(|i| l.button(band, i, count)).collect();
                for (i, &r) in rects.iter().enumerate() {
                    assert!(
                        r.x >= band.x - 0.01
                            && r.right() <= band.right() + 0.01
                            && r.y >= band.y - 0.01
                            && r.bottom() <= band.bottom() + 0.01,
                        "{w}x{h}: button {i} at {r:?} leaves its band {band:?}"
                    );
                    for (j, &other) in rects.iter().enumerate().skip(i + 1) {
                        assert!(!overlaps(r, other), "{w}x{h}: buttons {i} and {j} overlap");
                    }
                }
            }
        }
    }

    #[test]
    fn a_button_slot_that_does_not_exist_has_no_rectangle() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.button(l.controls, 6, 6), Rect::EMPTY);
        assert_eq!(l.button(l.controls, 0, 0), Rect::EMPTY);
    }

    #[test]
    fn the_board_is_still_playable_in_a_window_too_small_for_the_chrome() {
        let small = (140.0, 120.0);
        let mut g = sized(small);
        let l = g.layout();
        assert!(l.board.w > 10.0, "no board left at {small:?}");
        let before = g.board().clone();
        // Some tile in the gap's own row must be reachable.
        let target = (0..g.size() * g.size())
            .find(|&i| i != g.board().gap() && row_col(i, g.size()).0 == g.board().gap_row())
            .unwrap();
        let (x, y) = l.square(g.size(), target).centre();
        g.click_at(x, y, MouseButton::Left, small);
        assert_ne!(
            *g.board(),
            before,
            "a click on the board did nothing at {small:?}"
        );
    }

    // ── Pointer ────────────────────────────────────────────────────────────

    #[test]
    fn every_control_the_program_draws_answers_a_click() {
        let reference = game();
        let targets: Vec<Target> = reference
            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
            .hits()
            .iter()
            .map(|(t, _)| *t)
            .collect();
        assert!(!targets.is_empty(), "the game records no hit boxes at all");
        for target in targets {
            let mut g = game();
            let before = describe(&g);
            probe::click(&mut g, target);
            assert_ne!(
                describe(&g),
                before,
                "clicking {target:?} changed nothing at all"
            );
        }
    }

    #[test]
    fn a_tile_is_clickable_exactly_where_it_is_drawn() {
        // Fault two: the hit test kept its own copy of the layout —
        // `grid_offset_x = 50.0`, `grid_offset_y = 80.0`, `cell_size = 80.0` —
        // beside the one `render` drew with. The hit boxes now come from the
        // drawing pass, so this asks the layout where a square is and the
        // frame what is there.
        for &(w, h) in WINDOWS {
            for size in SIZES {
                let mut g = at_size(size);
                g.resize(w, h);
                let l = g.layout();
                if l.cell(size) < 4.0 {
                    continue;
                }
                for index in 0..size * size {
                    let (x, y) = l.square(size, index).centre();
                    assert_eq!(
                        g.target_at(x, y),
                        Some(Target::Tile(index)),
                        "{w}x{h} {size}x{size}: the centre of square {index} is not square {index}"
                    );
                }
            }
        }
    }

    #[test]
    fn clicking_the_far_end_of_the_gaps_row_pushes_the_whole_run() {
        // Fault four: `slide_pos` required the clicked square to be
        // orthogonally adjacent to the gap, so the one gesture every physical
        // sliding puzzle supports — shove a whole row along — did nothing.
        let mut b = Board::new(4);
        // Solved, so the gap is at index 15: the far end of its row is 12.
        assert_eq!(b.gap(), 15);
        let moved = b.push_to(12);
        assert_eq!(moved, 3, "the run was not pushed the whole way");
        assert_eq!(b.gap(), 12, "the gap did not travel to the clicked square");
        assert_eq!(&b.tiles()[12..16], &[0, 13, 14, 15], "the row is wrong");
    }

    #[test]
    fn a_pushed_run_counts_every_tile_it_moved() {
        let mut g = at_size(4);
        // Put the game in a known place by starting from a solved board.
        g.board = Board::new(4);
        g.moves = 0;
        g.state = GameState::Playing;
        g.apply(Action::PushTo(12));
        assert_eq!(g.moves(), 3, "a three-tile shove counted as {}", g.moves());
    }

    #[test]
    fn a_tile_not_in_line_with_the_gap_moves_nothing_and_says_so() {
        let mut g = at_size(4);
        g.board = Board::new(4);
        g.moves = 0;
        let before = g.board().clone();
        // The gap is at 15 (row 3, col 3); square 0 is in neither.
        g.apply(Action::PushTo(0));
        assert_eq!(*g.board(), before, "an out-of-line click moved tiles");
        assert_eq!(g.moves(), 0, "an out-of-line click counted a move");
        assert!(
            g.status().contains("not in line"),
            "the status says {:?} instead",
            g.status()
        );
    }

    #[test]
    fn clicking_the_gap_itself_moves_nothing() {
        let mut g = game();
        let gap = g.board().gap();
        let before = g.board().clone();
        probe::click(&mut g, Target::Tile(gap));
        assert_eq!(*g.board(), before, "clicking the gap moved a tile");
    }

    #[test]
    fn a_click_is_read_against_the_size_last_drawn() {
        // The old program read every click against constants, so a click at
        // (60, 90) moved a tile in a window where no board had ever been
        // drawn there.
        let big = (1400.0, 1000.0);
        let small = (400.0, 340.0);
        let mut g = game();
        g.resize(big.0, big.1);
        let far = Layout::new(big.0, big.1).square(g.size(), 0).centre();
        // Redraw small. The point that was square zero in the big window is
        // now well past the board's right-hand edge.
        g.resize(small.0, small.1);
        let here = Layout::new(small.0, small.1).square(g.size(), 0).centre();
        assert!(
            (far.0 - here.0).abs() > 20.0,
            "the two layouts are too alike to tell apart"
        );
        assert_eq!(
            g.target_at(here.0, here.1),
            Some(Target::Tile(0)),
            "the small window's square zero is not square zero"
        );
        assert_ne!(
            g.target_at(far.0, far.1),
            Some(Target::Tile(0)),
            "the big window's square zero is still live in a small window"
        );
    }

    #[test]
    fn a_click_on_nothing_is_ignored() {
        let mut g = game();
        let l = g.layout();
        // The header carries the title and nothing clickable.
        let (x, y) = (l.header.x + l.pad, l.header.y + l.header.h / 2.0);
        assert_eq!(g.target_at(x, y), None, "the header records a hit box");
        let before = describe(&g);
        let outcome = g.click_at(x, y, MouseButton::Left, SlidingPuzzle::SIZE);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(describe(&g), before, "a click on nothing changed something");
    }

    #[test]
    fn a_release_is_not_a_second_click() {
        let mut g = game();
        let l = g.layout();
        let (x, y) = l.square(g.size(), g.board().gap()).centre();
        let before = describe(&g);
        let outcome = handle_event(
            &mut g,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Release(MouseButton::Left),
            }),
        );
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(describe(&g), before, "a release changed something");
    }

    #[test]
    fn a_score_row_switches_to_the_size_it_names() {
        for (i, &size) in SIZES.iter().enumerate() {
            let mut g = at_size(if size == 3 { 4 } else { 3 });
            probe::click(&mut g, Target::Score(i));
            assert_eq!(
                g.size(),
                size,
                "score row {i} did not switch to {size}x{size}"
            );
        }
    }

    #[test]
    fn a_size_button_switches_to_the_size_it_names() {
        for (i, &size) in SIZES.iter().enumerate() {
            let mut g = at_size(if size == 3 { 4 } else { 3 });
            probe::click(&mut g, Target::Size(i));
            assert_eq!(
                g.size(),
                size,
                "size button {i} did not switch to {size}x{size}"
            );
        }
    }

    #[test]
    fn choosing_the_size_you_are_already_on_says_so_rather_than_reshuffling() {
        let mut g = at_size(4);
        let before = g.board().clone();
        probe::click(&mut g, Target::Size(1));
        assert_eq!(
            *g.board(),
            before,
            "a redundant size click reshuffled the board"
        );
        assert!(
            g.status().contains("Already 4x4"),
            "the status says {:?} instead",
            g.status()
        );
    }

    // ── Keyboard ───────────────────────────────────────────────────────────

    #[test]
    fn a_key_that_comes_back_up_is_not_a_second_press() {
        // Fault one, the one that made the program unplayable even once it had
        // a window: `Event::Key(KeyEvent { key, modifiers, .. })` swallowed
        // `pressed`, so every keystroke ran on the way down and again on the
        // way up.
        let mut g = game();
        let before = describe(&g);
        let up = KeyEvent {
            pressed: false,
            text: String::new(),
            ..probe::press(Key::H)
        };
        let outcome = g.key_at(&up, SlidingPuzzle::SIZE);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(describe(&g), before, "a key release changed something");
    }

    #[test]
    fn one_tap_of_an_arrow_slides_exactly_one_tile() {
        // With the release read as a press, an arrow slid *two* tiles and the
        // move counter went up by two.
        let mut g = game();
        let before = g.board().clone();
        let dir = any_legal(&before);
        tap(&mut g, arrow_for(dir));
        assert_eq!(g.moves(), 1, "one tap counted {} moves", g.moves());
        let mut expected = before;
        assert!(expected.slide(dir), "the board could not slide {dir:?}");
        assert_eq!(
            *g.board(),
            expected,
            "one tap moved the wrong number of tiles"
        );
    }

    #[test]
    fn one_tap_of_h_leaves_the_help_sheet_open_to_be_read() {
        // The sheet opened on the press and closed on the release, so the only
        // written record of the controls could never be seen.
        let mut g = game();
        tap(&mut g, Key::H);
        assert!(g.show_help(), "the help sheet shut itself in one keystroke");
        for (key, what) in HELP_ROWS {
            assert!(
                shows(&g, SlidingPuzzle::SIZE, key),
                "the sheet is open and does not name {key:?}"
            );
            assert!(
                shows(&g, SlidingPuzzle::SIZE, what),
                "the sheet is open and does not explain {key:?}"
            );
        }
        tap(&mut g, Key::H);
        assert!(!g.show_help(), "a second tap did not close the sheet");
    }

    #[test]
    fn one_tap_of_t_turns_the_numbers_off_and_they_stay_off() {
        let mut g = game();
        assert!(g.show_numbers());
        tap(&mut g, Key::T);
        assert!(!g.show_numbers(), "the numbers came straight back on");
        let drawn = texts(&g, SlidingPuzzle::SIZE);
        for value in 1..(g.size() * g.size()) {
            assert!(
                !drawn.iter().any(|t| t == &value.to_string()),
                "the numbers are off and {value} is still drawn"
            );
        }
        // With them on, they are all there.
        tap(&mut g, Key::T);
        let drawn = texts(&g, SlidingPuzzle::SIZE);
        for value in 1..(g.size() * g.size()) {
            assert!(
                drawn.iter().any(|t| t == &value.to_string()),
                "the numbers are on and {value} is missing"
            );
        }
    }

    #[test]
    fn one_tap_of_a_digit_changes_the_size_once() {
        for (key, size) in [(Key::Num3, 3), (Key::Num4, 4), (Key::Num5, 5)] {
            let mut g = at_size(if size == 4 { 3 } else { 4 });
            tap(&mut g, key);
            assert_eq!(g.size(), size, "{key:?} did not reach {size}x{size}");
            assert_eq!(g.board().size(), size, "the board is the wrong size");
        }
    }

    #[test]
    fn one_tap_of_n_deals_one_new_board() {
        let mut g = game();
        // Play a move so the counter has something to be reset from.
        let dir = any_legal(g.board());
        tap(&mut g, arrow_for(dir));
        assert_eq!(g.moves(), 1);
        let first = g.board().clone();
        tap(&mut g, Key::N);
        assert_eq!(g.moves(), 0, "a new game kept the old move count");
        assert_ne!(*g.board(), first, "a new game dealt the same board");
    }

    #[test]
    fn an_arrow_with_nothing_to_slide_says_so_and_counts_nothing() {
        let mut g = at_size(3);
        g.board = Board::new(3);
        g.moves = 0;
        g.state = GameState::Playing;
        // The gap is in the bottom-right corner, so no tile can come up into
        // it from below and none can come left into it from the right.
        let before = g.board().clone();
        g.apply(Action::Slide(Direction::Up));
        assert_eq!(*g.board(), before, "a blocked slide moved a tile");
        assert_eq!(g.moves(), 0, "a blocked slide counted a move");
        assert!(
            g.status().contains("Nothing to slide"),
            "the status says {:?}",
            g.status()
        );
    }

    #[test]
    fn a_modifier_the_program_does_not_use_is_ignored() {
        for modifiers in [Modifiers::ctrl(), Modifiers::alt(), Modifiers::super_key()] {
            let mut g = game();
            let before = describe(&g);
            let outcome = g.key_at(&probe::press_with(Key::N, modifiers), SlidingPuzzle::SIZE);
            assert_eq!(outcome, EventResult::Ignored, "{modifiers:?} was answered");
            assert_eq!(describe(&g), before, "{modifiers:?}+N changed something");
        }
    }

    #[test]
    fn a_key_the_program_does_not_use_is_ignored() {
        for key in [
            Key::A,
            Key::Z,
            Key::Num1,
            Key::Num9,
            Key::Tab,
            Key::Backspace,
        ] {
            let mut g = game();
            let before = describe(&g);
            let outcome = g.key_at(&probe::press(key), SlidingPuzzle::SIZE);
            assert_eq!(outcome, EventResult::Ignored, "{key:?} was answered");
            assert_eq!(describe(&g), before, "{key:?} changed something");
        }
    }

    #[test]
    fn the_open_sheet_swallows_the_keys_that_are_not_about_it() {
        for key in [
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::N,
            Key::T,
            Key::Num3,
        ] {
            let mut g = game();
            g.apply(Action::ToggleHelp);
            let before = describe(&g);
            let outcome = g.key_at(&probe::press(key), SlidingPuzzle::SIZE);
            assert_eq!(
                outcome,
                EventResult::Ignored,
                "{key:?} played behind the sheet"
            );
            assert_eq!(
                describe(&g),
                before,
                "{key:?} changed something behind the sheet"
            );
        }
    }

    #[test]
    fn every_key_the_sheet_names_as_closing_it_closes_it() {
        for key in [Key::H, Key::Escape, Key::Enter, Key::Space] {
            let mut g = game();
            g.apply(Action::ToggleHelp);
            assert!(g.show_help());
            g.key_at(&probe::press(key), SlidingPuzzle::SIZE);
            assert!(!g.show_help(), "{key:?} did not close the sheet");
        }
    }

    #[test]
    fn escape_with_no_sheet_open_does_nothing() {
        let mut g = game();
        let before = describe(&g);
        let outcome = g.key_at(&probe::press(Key::Escape), SlidingPuzzle::SIZE);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(describe(&g), before);
    }

    #[test]
    fn a_click_while_the_sheet_is_open_closes_it_and_reaches_nothing_behind_it() {
        let mut g = game();
        g.apply(Action::ToggleHelp);
        let before = g.board().clone();
        let moves = g.moves();
        // Aim at a tile: with the sheet up, that click must close the sheet
        // and stop there.
        let (x, y) = g.layout().square(g.size(), 0).centre();
        g.click_at(x, y, MouseButton::Left, SlidingPuzzle::SIZE);
        assert!(!g.show_help(), "the click did not close the sheet");
        assert_eq!(*g.board(), before, "the click reached the board underneath");
        assert_eq!(g.moves(), moves, "the click counted a move");
    }

    #[test]
    fn while_the_sheet_is_up_the_frame_says_every_point_belongs_to_the_sheet() {
        // The test above proves the *behaviour*; this one proves the frame is
        // telling the truth about it. The sheet is opaque, so a frame that
        // still answered `Tile(5)` for a point buried under the help text
        // would be describing a screen nobody is looking at — and anything
        // reading the frame to find out what is clickable (this suite's own
        // `every_control_the_program_draws_answers_a_click`, a future
        // screen-reader, a hover highlight) would act on that lie.
        //
        // It is also what makes the modal work with no special case in
        // `handle_mouse`: the ordinary hit test is the only mechanism.
        let mut g = game();
        g.apply(Action::ToggleHelp);
        let l = g.layout();
        let count = SIZES.len().saturating_add(3);
        let mut points = vec![
            l.square(g.size(), 0).centre(),
            l.square(g.size(), 1).centre(),
            (1.0, 1.0),
            (l.window.w - 1.0, l.window.h - 1.0),
        ];
        for slot in 0..count {
            points.push(l.button(l.controls, slot, count).centre());
        }
        for slot in 0..SIZES.len() {
            points.push(l.button(l.scores, slot, SIZES.len()).centre());
        }
        for (x, y) in points {
            assert_eq!(
                g.target_at(x, y),
                Some(Target::ToggleHelp),
                "with the sheet up, ({x}, {y}) still answers something behind it"
            );
        }
        // And once it is shut the frame goes back to answering for the board,
        // so the box above is a cover and not a permanent blindfold.
        g.apply(Action::CloseHelp);
        let (x, y) = l.square(g.size(), 0).centre();
        assert_eq!(g.target_at(x, y), Some(Target::Tile(0)));
    }

    #[test]
    fn the_help_sheet_names_every_key_the_program_answers() {
        // Each key the game acts on, and the row of the sheet that claims it.
        let documented: &[(Key, &str)] = &[
            (Key::Up, "Arrows"),
            (Key::Down, "Arrows"),
            (Key::Left, "Arrows"),
            (Key::Right, "Arrows"),
            (Key::Num3, "3 / 4 / 5"),
            (Key::Num4, "3 / 4 / 5"),
            (Key::Num5, "3 / 4 / 5"),
            (Key::N, "N"),
            (Key::T, "T"),
            (Key::H, "H"),
        ];
        let g = game();
        for &(key, row) in documented {
            assert!(
                g.key_action(&probe::press(key)).is_some(),
                "the sheet names {key:?} and the game ignores it"
            );
            assert!(
                HELP_ROWS.iter().any(|(k, _)| *k == row),
                "no row of the sheet reads {row:?}"
            );
        }
        // And the other direction: nothing the game answers is left unstated.
        let named: Vec<Key> = documented.iter().map(|(k, _)| *k).collect();
        for key in [
            Key::A,
            Key::B,
            Key::Q,
            Key::Z,
            Key::Num1,
            Key::Num2,
            Key::Num6,
            Key::Num9,
            Key::Tab,
            Key::Backspace,
            Key::Delete,
            Key::Home,
        ] {
            if named.contains(&key) {
                continue;
            }
            assert!(
                g.key_action(&probe::press(key)).is_none(),
                "{key:?} does something the sheet never mentions"
            );
        }
        // Escape, Enter and Space are answered only with the sheet open, which
        // is exactly what the last row says.
        let mut open = game();
        open.apply(Action::ToggleHelp);
        for key in [Key::Escape, Key::Enter, Key::Space] {
            assert!(
                g.key_action(&probe::press(key)).is_none(),
                "{key:?} acts with no sheet up"
            );
            assert!(
                open.key_action(&probe::press(key)).is_some(),
                "{key:?} does not close the sheet"
            );
        }
    }

    // ── The rules of the board ─────────────────────────────────────────────

    #[test]
    fn a_solved_board_reads_one_to_n_with_the_gap_last() {
        for size in SIZES {
            let b = Board::new(size);
            assert!(b.is_solved(), "{size}x{size} is not born solved");
            assert_eq!(b.gap(), size * size - 1, "the gap is not last");
            for (i, &t) in b.tiles().iter().enumerate() {
                let want = if i + 1 == size * size {
                    0
                } else {
                    u8::try_from(i + 1).unwrap()
                };
                assert_eq!(t, want, "{size}x{size}: square {i} holds {t}");
            }
        }
    }

    #[test]
    fn a_board_of_no_size_is_rounded_up_to_one_square() {
        let b = Board::new(0);
        assert_eq!(b.size(), 1);
        assert_eq!(b.tiles(), &[0]);
        assert_eq!(b.gap(), 0);
        assert!(b.is_solved());
    }

    #[test]
    fn a_slide_moves_one_tile_and_can_be_undone() {
        for size in SIZES {
            let mut b = Board::new(size);
            let mut rng = SeededRng::new(7);
            b.shuffle(&mut rng, 40);
            for dir in Direction::ALL {
                let before = b.clone();
                if !b.can_slide(dir) {
                    assert!(!b.clone().slide(dir), "{dir:?} slid when it could not");
                    continue;
                }
                assert!(b.slide(dir), "{dir:?} could slide and did not");
                assert_ne!(b, before, "{dir:?} slid and changed nothing");
                assert!(b.slide(dir.opposite()), "{dir:?} could not be undone");
                assert_eq!(b, before, "{dir:?} then its opposite is not the identity");
            }
        }
    }

    #[test]
    fn a_slide_never_loses_or_duplicates_a_tile() {
        for size in SIZES {
            let solved = Board::new(size);
            let mut b = solved.clone();
            let mut rng = SeededRng::new(11);
            for _ in 0..2000 {
                let dir = *rng.choose(&Direction::ALL).unwrap();
                b.slide(dir);
                let mut sorted = b.tiles().to_vec();
                sorted.sort_unstable();
                let mut want = solved.tiles().to_vec();
                want.sort_unstable();
                assert_eq!(sorted, want, "{size}x{size}: the tiles changed identity");
                assert_eq!(b.tiles()[b.gap()], 0, "the gap index does not hold the gap");
            }
        }
    }

    #[test]
    fn a_push_moves_only_tiles_in_line_with_the_gap() {
        for size in SIZES {
            let mut rng = SeededRng::new(3);
            for _ in 0..40 {
                let mut b = Board::new(size);
                b.shuffle(&mut rng, 30);
                let before = b.clone();
                let (grow, gcol) = (b.gap_row(), b.gap_col());
                for index in 0..size * size {
                    let mut candidate = before.clone();
                    let moved = candidate.push_to(index);
                    let (row, col) = row_col(index, size);
                    let in_line = (row == grow || col == gcol) && index != before.gap();
                    if in_line {
                        assert_eq!(
                            moved,
                            row.abs_diff(grow) + col.abs_diff(gcol),
                            "{size}x{size}: the run to {index} was cut short"
                        );
                        assert_eq!(
                            candidate.gap(),
                            index,
                            "{size}x{size}: the gap did not end up where the click was"
                        );
                    } else {
                        assert_eq!(moved, 0, "{size}x{size}: {index} moved out of line");
                        assert_eq!(candidate, before, "{size}x{size}: {index} moved tiles");
                    }
                }
            }
        }
    }

    #[test]
    fn a_push_past_the_end_of_the_board_moves_nothing() {
        let mut b = Board::new(4);
        let before = b.clone();
        assert_eq!(b.push_to(16), 0);
        assert_eq!(b, before);
    }

    #[test]
    fn every_shuffle_leaves_a_board_that_can_be_solved() {
        // Walking at random over legal moves is what keeps a scramble
        // solvable, and the parity rule below is an independent way of saying
        // so: it never looks at how the board was made.
        for size in SIZES {
            for seed in 0..200_u64 {
                let mut b = Board::new(size);
                let mut rng = SeededRng::new(seed);
                b.shuffle(&mut rng, shuffle_moves(size));
                assert!(
                    is_solvable(&b),
                    "{size}x{size} seed {seed} produced an unsolvable board"
                );
            }
        }
    }

    #[test]
    fn a_scramble_that_comes_home_is_walked_again() {
        // Fault nine. A hundred-move walk never returns to the start, so the
        // guard could not be tested through `new_game` at any size the game
        // offers — which is why the guard is a free function taking the walk
        // length. At twelve to sixteen moves a walk does occasionally come
        // home, and the count below asserts that it did, so this test cannot
        // quietly stop exercising the thing it names.
        let mut came_home = 0;
        for walk in [12_usize, 16] {
            for seed in 0..4000_u64 {
                let mut raw = Board::new(3);
                let mut rng = SeededRng::new(seed);
                raw.shuffle(&mut rng, walk);
                if raw.is_solved() {
                    came_home += 1;
                }
                let mut guarded = Board::new(3);
                let mut rng = SeededRng::new(seed);
                scramble_board(&mut guarded, &mut rng, walk);
                assert!(
                    !guarded.is_solved(),
                    "walk {walk} seed {seed}: the scramble handed back a solved board"
                );
            }
        }
        assert!(
            came_home > 0,
            "no walk came home, so nothing here tested the guard"
        );
    }

    #[test]
    fn the_walk_never_undoes_the_move_it_just_made() {
        // A walk that may immediately reverse itself wastes half its length,
        // and at short lengths lands back on a solved board outright — which
        // is the fault the retry above exists to paper over. The guard is what
        // stops it happening in the first place.
        //
        // Two moves is the sharpest possible window on it. Each slide moves
        // one tile one square, so the distance floor changes by exactly one
        // per move: it is 1 after the first, and therefore 0 or 2 after the
        // second — 0 if and only if the second undid the first. Asserting the
        // floor is 2 catches an undo even in the cases where the board happens
        // not to be solved by it, which a bare `is_solved` check would miss.
        for seed in 0..2000_u64 {
            let mut b = Board::new(3);
            let mut rng = SeededRng::new(seed);
            b.shuffle(&mut rng, 2);
            assert!(!b.is_solved(), "seed {seed}: a two-move walk came home");
            assert_eq!(
                b.distance_floor(),
                2,
                "seed {seed}: the second move undid the first"
            );
        }
    }

    #[test]
    fn a_board_that_cannot_be_scrambled_gives_up_instead_of_hanging() {
        // The other half of fault nine. A 1x1 has no legal move, so a scramble
        // that retried until the board was unsolved would spin forever — the
        // same unbounded loop the walk itself had to be rescued from. This
        // test does not assert; it either returns or the suite times out.
        let mut b = Board::new(1);
        let mut rng = SeededRng::new(1);
        scramble_board(&mut b, &mut rng, 100);
        assert!(b.is_solved(), "a 1x1 board found a move to make");

        let mut g = game();
        g.size = 1;
        g.new_game();
        assert!(
            g.status().contains("cannot be scrambled"),
            "the status says {:?} instead",
            g.status()
        );
    }

    // ── The moves-remaining floor ──────────────────────────────────────────

    #[test]
    fn the_floor_is_zero_exactly_when_the_board_is_solved() {
        for size in SIZES {
            let mut b = Board::new(size);
            assert_eq!(b.distance_floor(), 0, "a solved board is not at zero");
            let mut rng = SeededRng::new(5);
            for _ in 0..500 {
                b.slide(*rng.choose(&Direction::ALL).unwrap());
                assert_eq!(
                    b.distance_floor() == 0,
                    b.is_solved(),
                    "{size}x{size}: the floor and the goal disagree"
                );
            }
        }
    }

    #[test]
    fn one_move_changes_the_floor_by_exactly_one() {
        // This is the whole argument that the number is a floor: a move moves
        // one tile one square, so it can close at most one unit of distance,
        // so no solution can be shorter than the distance outstanding.
        for size in SIZES {
            let mut b = Board::new(size);
            let mut rng = SeededRng::new(13);
            for _ in 0..1500 {
                let before = b.distance_floor();
                let dir = *rng.choose(&Direction::ALL).unwrap();
                if !b.slide(dir) {
                    continue;
                }
                let after = b.distance_floor();
                assert_eq!(
                    before.abs_diff(after),
                    1,
                    "{size}x{size}: one move moved the floor by {}",
                    before.abs_diff(after)
                );
            }
        }
    }

    #[test]
    fn the_floor_is_never_more_than_the_moves_it_really_takes() {
        // Checked against a breadth-first search, which knows nothing about
        // distances: an estimate that could exceed the true remaining moves
        // would be a promise the game cannot keep.
        for seed in 0..12_u64 {
            let mut b = Board::new(3);
            let mut rng = SeededRng::new(seed);
            b.shuffle(&mut rng, 100);
            let truth = shortest_solution(&b).len() as u32;
            let floor = b.distance_floor();
            assert!(
                floor <= truth,
                "seed {seed}: the floor claims {floor} and the truth is {truth}"
            );
        }
    }

    #[test]
    fn the_game_reports_the_floor_of_the_board_it_is_showing() {
        let g = game();
        assert_eq!(g.floor_left(), g.board().distance_floor());
        assert_eq!(
            g.opening_floor(),
            g.floor_left(),
            "the opening floor is not the floor it opened at"
        );
        assert!(g.opening_floor() > 0, "the game opened solved");
    }

    // ── Winning and scores ─────────────────────────────────────────────────

    #[test]
    fn solving_the_puzzle_wins_it_and_records_the_score() {
        let mut g = at_size(3);
        let solution = shortest_solution(g.board());
        assert!(!solution.is_empty(), "the game opened solved");
        for dir in &solution {
            g.apply(Action::Slide(*dir));
        }
        assert_eq!(g.state(), GameState::Won, "a solved board is not a win");
        assert!(g.board().is_solved());
        assert_eq!(g.moves(), solution.len() as u32);
        assert_eq!(
            g.best()[0],
            Some(solution.len() as u32),
            "the 3x3 score was not recorded"
        );
        assert_eq!(g.best()[1], None, "a 3x3 win wrote the 4x4 score");
        assert!(
            g.status()
                .contains(&format!("Solved in {} moves", g.moves())),
            "the status says {:?}",
            g.status()
        );
        assert_eq!(g.floor_left(), 0);
    }

    #[test]
    fn a_won_board_ignores_further_moves() {
        let mut g = at_size(3);
        for dir in shortest_solution(g.board()) {
            g.apply(Action::Slide(dir));
        }
        let after_win = describe(&g);
        for dir in Direction::ALL {
            g.apply(Action::Slide(dir));
        }
        g.apply(Action::PushTo(0));
        assert_eq!(after_win, describe(&g), "a won board went on playing");
    }

    /// Play `g` from `board` to the end, wasting `waste` pairs of moves on the
    /// way, and return the number of moves it took.
    ///
    /// The same board twice, deliberately: two *different* scrambles differ in
    /// how long they take to solve by far more than a wasted move or two, so
    /// comparing scores across them says nothing about whether the record is
    /// being kept correctly.
    fn play_out(g: &mut SlidingPuzzle, board: &Board, waste: usize) -> u32 {
        g.board = board.clone();
        g.moves = 0;
        g.state = GameState::Playing;
        for _ in 0..waste {
            let dir = any_legal(g.board());
            g.apply(Action::Slide(dir));
            g.apply(Action::Slide(dir.opposite()));
        }
        let rest = shortest_solution(g.board());
        for dir in rest {
            g.apply(Action::Slide(dir));
        }
        assert_eq!(g.state(), GameState::Won, "the play-out did not finish");
        g.moves()
    }

    #[test]
    fn a_worse_second_solve_does_not_replace_the_score() {
        let mut g = at_size(3);
        let board = g.board().clone();
        let short = play_out(&mut g, &board, 0);
        assert_eq!(g.best()[0], Some(short));
        let long = play_out(&mut g, &board, 3);
        assert!(
            long > short,
            "the second game was not the longer one ({long} against {short})"
        );
        assert_eq!(g.best()[0], Some(short), "a worse game overwrote the score");
    }

    #[test]
    fn a_better_second_solve_replaces_the_score() {
        let mut g = at_size(3);
        let board = g.board().clone();
        let long = play_out(&mut g, &board, 3);
        assert_eq!(g.best()[0], Some(long));
        let short = play_out(&mut g, &board, 0);
        assert!(short < long, "the second game was not the shorter one");
        assert_eq!(g.best()[0], Some(short), "a better game was not recorded");
    }

    #[test]
    fn changing_size_keeps_the_scores_of_the_other_sizes() {
        let mut g = at_size(3);
        for dir in shortest_solution(g.board()) {
            g.apply(Action::Slide(dir));
        }
        let score = g.best()[0];
        assert!(score.is_some());
        g.apply(Action::SetSize(4));
        g.apply(Action::SetSize(5));
        g.apply(Action::SetSize(3));
        assert_eq!(
            g.best()[0],
            score,
            "the 3x3 score was lost on the way round"
        );
    }

    #[test]
    fn a_new_game_clears_the_move_count_and_the_win() {
        let mut g = at_size(3);
        for dir in shortest_solution(g.board()) {
            g.apply(Action::Slide(dir));
        }
        assert_eq!(g.state(), GameState::Won);
        g.apply(Action::NewGame);
        assert_eq!(g.state(), GameState::Playing);
        assert_eq!(g.moves(), 0);
        assert!(!g.board().is_solved(), "a new game opened solved");
    }

    #[test]
    fn a_size_the_game_does_not_offer_is_refused() {
        let mut g = game();
        let before = describe(&g);
        for size in [0_usize, 1, 2, 6, 100] {
            g.apply(Action::SetSize(size));
            assert_eq!(describe(&g), before, "size {size} was accepted");
        }
    }

    // ── What the window shows ──────────────────────────────────────────────

    #[test]
    fn the_info_line_says_the_size_the_moves_and_the_floor() {
        let g = game();
        let expected = format!(
            "{}x{} \u{2014} {} moves \u{2014} {}",
            g.size(),
            g.size(),
            g.moves(),
            g.status()
        );
        assert!(
            shows(&g, SlidingPuzzle::SIZE, &expected),
            "the info line reads none of {expected:?}; it drew {:?}",
            texts(&g, SlidingPuzzle::SIZE)
        );
        assert!(
            g.status().contains(&g.opening_floor().to_string()),
            "the opening status does not name the floor: {:?}",
            g.status()
        );
    }

    #[test]
    fn the_title_names_the_puzzle_the_size_makes() {
        for (size, title) in [(3, "8-Puzzle"), (4, "15-Puzzle"), (5, "24-Puzzle")] {
            let g = at_size(size);
            assert!(
                shows(&g, SlidingPuzzle::SIZE, title),
                "{size}x{size} is not called {title}"
            );
        }
    }

    #[test]
    fn a_score_that_has_not_been_set_is_drawn_as_a_dash_not_a_number() {
        let g = game();
        for size in SIZES {
            assert!(
                shows(&g, SlidingPuzzle::SIZE, &format!("{size}x{size}: \u{2014}")),
                "an unset {size}x{size} score is not drawn as a dash"
            );
        }
    }

    #[test]
    fn a_score_appears_in_its_row_once_it_is_set() {
        let mut g = at_size(3);
        for dir in shortest_solution(g.board()) {
            g.apply(Action::Slide(dir));
        }
        let moves = g.moves();
        assert!(
            shows(&g, SlidingPuzzle::SIZE, &format!("3x3: {moves}")),
            "the score row does not show {moves}; it drew {:?}",
            texts(&g, SlidingPuzzle::SIZE)
        );
    }

    #[test]
    fn a_tile_already_home_is_outlined_rather_than_recoloured() {
        // The fill is what tells tiles apart, so "home" has to be a different
        // signal from "seven" or the board says two things with one colour.
        let mut g = at_size(3);
        g.board = Board::new(3);
        let outlines = g
            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
            .commands()
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(
            outlines, 8,
            "a solved 3x3 outlined {outlines} of its 8 tiles"
        );
        let colours: Vec<Color> = (1..=8_u8).map(tile_color).collect();
        for (i, a) in colours.iter().enumerate() {
            for b in colours.iter().skip(i + 1) {
                assert_ne!(a, b, "two tiles of a 3x3 share a colour");
            }
        }
    }

    // ── The window ─────────────────────────────────────────────────────────

    #[test]
    fn a_resize_event_reaches_the_layout() {
        let mut g = game();
        assert_eq!(
            handle_event(
                &mut g,
                &Event::Resize {
                    width: 1000,
                    height: 800
                }
            ),
            EventResult::Consumed
        );
        let l = g.layout();
        assert!(
            (l.window.w - 1000.0).abs() < 0.01 && (l.window.h - 800.0).abs() < 0.01,
            "the layout is {}x{} after a resize to 1000x800",
            l.window.w,
            l.window.h
        );
    }

    #[test]
    fn rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against() {
        let mut g = game();
        let _ = g.render(500.0, 900.0);
        let l = g.layout();
        assert!((l.window.w - 500.0).abs() < 0.01 && (l.window.h - 900.0).abs() < 0.01);
        let (x, y) = l.square(g.size(), 0).centre();
        assert_eq!(g.target_at(x, y), Some(Target::Tile(0)));
    }

    #[test]
    fn a_close_request_exits_and_a_played_move_asks_for_a_redraw() {
        let mut g = game();
        assert!(matches!(g.on_event(&Event::CloseRequested), Response::Exit));
        assert!(matches!(
            g.on_event(&Event::Key(probe::press(Key::Up))),
            Response::Redraw
        ));
        assert!(matches!(
            g.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle
        ));
    }

    #[test]
    fn the_window_is_named_and_sized() {
        let g = game();
        assert_eq!(g.app_id(), "sliding");
        assert!(!g.title().is_empty());
        let (w, h) = g.initial_size();
        assert!(w > 0 && h > 0);
    }

    #[test]
    fn a_frame_is_drawn_at_every_window_size_without_panicking() {
        for &(w, h) in WINDOWS {
            for size in SIZES {
                let mut g = at_size(size);
                let tree = g.render(w, h);
                assert!(
                    !tree.commands.is_empty(),
                    "{size}x{size} at {w}x{h} drew nothing"
                );
            }
        }
    }
}
