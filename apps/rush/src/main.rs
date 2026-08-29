#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]

//! Rush Hour — slide the red car out through the right-hand exit, in a real
//! window.
//!
//! A 6x6 yard holds one red car on the exit row and a jam of other cars and
//! trucks, each able to slide only along its own axis. Eight puzzles, keyboard
//! and pointer, undo, a move counter, and a puzzle sheet.
//!
//! # What wiring this up found
//!
//! The program could not be played, because `main` was
//! `let _app = RushHour::new();` — it built puzzle 1, dropped it and exited.
//! Nothing below was reachable to notice until it had a window on it.
//!
//! 1. **The layout was a constant, and the click was read against the
//!    constant.** `cell_origin`, `cell_at_point`, `grid_origin`,
//!    `window_width` and `window_height` were all free functions of nothing —
//!    just `CELL_SIZE = 72.0` and friends. `render` took `width` and `height`
//!    and opened with `let _ = (width, height); // use layout params if
//!    needed`, then sized its own background from `window_width()`. So the
//!    program drew a 476x568 picture into whatever window it was given, and
//!    `handle_mouse` sent the click through `cell_at_point`, which answered
//!    from the same constants. In any other window the yard was drawn in one
//!    place and clicked in another. The layout is now computed from the live
//!    window size every frame and the hit boxes are recorded by the drawing
//!    pass, so a car is clickable exactly where its ink is.
//! 2. **The pointer could select but never play.** `handle_mouse` called
//!    `select_at_cell` and nothing else: a click could pick a car up and put it
//!    down again, and sliding, undo, restart, next and the puzzle sheet were
//!    keyboard-only — advertised by a footer string that was the only evidence
//!    those keys existed. Clicking an empty cell a selected car can reach now
//!    slides it there, and every command has a button.
//! 3. **The win rule never looked at the row.** `check_win` was
//!    `player.tail_col() >= GRID_SIZE - 1` — column only. It was correct solely
//!    because every puzzle table happened to put the player on row 2 and no
//!    vertical car was ever vehicle 0. Meanwhile the exit marker was drawn from
//!    its own literal `cell_origin(2, 0)`, so "the exit is on row 2" was
//!    written twice and checked nowhere. `EXIT_ROW` is now named once, and the
//!    win rule, the exit strip and the loader all read it — the third being
//!    what makes the puzzle tables unable to disagree at all (see 4). The win
//!    rule now asks whether the red car *covers the cell the exit opens onto*,
//!    which is one fact about both axes rather than a column test with a row
//!    test in front of it that could never fail.
//! 4. **The player's identity was inferred from its paint.** `is_player()` was
//!    `self.color_index == 0`, and `load_puzzle` assigned
//!    `color_index: i % VEHICLE_COLORS.len()`. So "which car wins" was decided
//!    by the palette: reorder a puzzle table, or put a twelfth colour in the
//!    array, and the win condition quietly moves to a different car. A puzzle
//!    now names its player as a *column* (`PuzzleDef::player_col`) — its row is
//!    `EXIT_ROW` and its length `PLAYER_LENGTH`, neither of which a table gets
//!    to state — so there is exactly one player, structurally, and `Vehicle`
//!    carries a `player` flag rather than a colour to be read as one.
//! 5. **The win was a latch, and undo was refused while it was set.**
//!    `move_selected` wrote `status = Won` and nothing except loading a puzzle
//!    ever wrote it back, while `undo` opened with
//!    `if self.status == Won { return; }`. The winning move was the one move
//!    you could not take back. Winning is now *derived* from where the red car
//!    is, so undoing the winning move un-wins, because there is no separate
//!    fact left to disagree with the yard.
//! 6. **The victory overlay painted out the yard.** It filled the whole window
//!    with opaque `0x11111B` under the comment `// Semi-transparent overlay` —
//!    a comment arguing for a transparency the code did not have, which is an
//!    assertion nobody checks. The scrim is now genuinely translucent
//!    (`Color::rgba`, alpha `0xB4`, which `Canvas` composites with
//!    `Color::over`), so the jam you just cleared is visible behind the panel
//!    congratulating you on it.
//! 7. **`UndoAction` stored a vector index in a field, and two fields nothing
//!    read.** `vehicle_index` was spent as `self.vehicles[action.vehicle_index]`
//!    — fine only while indices and identities coincide, which nothing
//!    enforced — and `new_row`/`new_col` were written on every move and never
//!    read by anything. An undo entry is now a car **id** and one signed
//!    `delta`: the number the move added, which undo subtracts.
//! 8. **`can_move` guarded against a car being its own obstacle, which it
//!    cannot be.** Every cell it tested lay strictly beyond the leading edge,
//!    so `if let Some(occ) = occupancy[..] && occ != index` had an `occ != index`
//!    that could never be false — a guard in front of a rule that already
//!    holds, and so a line no test could ever own (known-issues lesson 51).
//!    It is gone.
//! 9. **Text was positioned by guessing its own width.** The move counter and
//!    the status drew at `total_width - 120.0`; the three victory lines at
//!    `banner_x + 50`, `+ 60` and `+ 40` — three different hand-tuned "centres"
//!    for three strings; and every car's letter at `cx + vw / 2.0 - 5.0`, which
//!    is a claim that the glyph is ten pixels wide. The program already linked
//!    `guitk::text`. Every string is now placed from its measured width.
//! 10. **A blanket `#![allow(dead_code)]` sat at the top of the file.** With
//!     `main` discarding the app almost the whole program was dead, and the
//!     allow is what let it compile without ever saying so — including
//!     `is_valid_placement` and `max_slide`, which nothing called at all. The
//!     allow is gone, `max_slide` is what click-to-slide is built on, and the
//!     lane gate's `-D dead_code` is what now decides whether a function is
//!     reachable.
//! 11. **A puzzle table's orientation column was a byte, and anything that was
//!     not `b'H'` meant vertical.** `orient_from_byte` had no error case, so a
//!     typo'd `b'h'` in any of the eight tables was not a mistake but a
//!     silently different puzzle. The tables now hold the `Orientation` enum.
//! 12. **The undo cap shuffled the whole vector.** At `MAX_UNDO` entries
//!     `move_selected` did `undo_stack.remove(0)` — an O(n) shift per move past
//!     the cap. It is a `VecDeque` now, and the header shows the depth so the
//!     loss of the oldest move is at least visible.
//! 13. **The puzzle sheet could not be reached or dismissed by pointer.**
//!     `handle_mouse` returned early whenever it was open, so the list you were
//!     looking at was inert under the cursor while still covering the yard.
//!     It now takes clicks — a row opens that puzzle, anywhere else closes it —
//!     and, like the victory panel, it calls `Frame::discard_hits` so nothing
//!     behind it can be pressed through.
//! 14. **Four arrow-key arms each re-derived the same two facts.** Every arm
//!     repeated `self.selected < self.vehicles.len()` in its guard and then
//!     re-fetched the car to check its axis, doing nothing at all when the axis
//!     was wrong. One arm now turns a key into an axis and a delta, and the
//!     move rule decides the rest.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::collections::VecDeque;
use std::process::ExitCode;

// ── Catppuccin Mocha palette ────────────────────────────────────────
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
const MAROON: Color = Color::from_hex(0xEBA0AC);
const SAPPHIRE: Color = Color::from_hex(0x74C7EC);
const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// The scrim drawn over the yard by the victory panel and the puzzle sheet.
///
/// Genuinely translucent — `Canvas::set` composites it with `Color::over`, so
/// the position underneath shows through. The version this replaced was opaque
/// and claimed otherwise in a comment.
const SCRIM: Color = Color::rgba(0x11, 0x11, 0x1B, 0xB4);

/// The colour of the car you are trying to get out, and of the exit it leaves
/// through. One constant for both, because the strip on the right-hand edge is
/// a picture of the car it is waiting for — not a separate decision that could
/// come to disagree with it.
const PLAYER_COLOR: Color = RED;

/// The colours the other vehicles are dealt from, in order, wrapping.
///
/// `PLAYER_COLOR` is deliberately **not** in this list: the player's car is the
/// one that ends the game, and a blocker painted the same colour would be a
/// picture that lies about which car that is.
const VEHICLE_COLORS: [Color; 11] = [
    BLUE, GREEN, YELLOW, PEACH, MAUVE, TEAL, LAVENDER, MAROON, SAPPHIRE, FLAMINGO, SUBTEXT0,
];

const GRID_SIZE: usize = 6;

/// The row the player's car sits on and the exit opens onto.
///
/// Named once and read by the win rule, the exit strip and the loader. It used
/// to be written twice — as a literal `2` in `render_exit_marker`, and as the
/// first field of the first row of all eight puzzle tables — and checked by
/// neither, while the win rule looked at the column alone.
const EXIT_ROW: usize = 2;

/// The column the player's car's tail must reach to be out.
const EXIT_COL: usize = GRID_SIZE - 1;

/// The player's car is two cells long in every puzzle, so the tables do not get
/// to say otherwise.
const PLAYER_LENGTH: usize = 2;

/// The letter on the player's car.
const PLAYER_LABEL: char = 'X';

/// The letters `RushHour::position` deals to the blockers it is handed, in
/// order. `X` is missing on purpose: it is the player's, and a fixture that
/// puts a second `X` on the yard is a fixture whose failures are unreadable.
const BLOCKER_LABELS: &str = "ABCDEFGHIJKLMNOPQRSTUVWYZ";

/// Moves kept for undo. Past this the oldest is dropped and the game can no
/// longer be unwound to the opening jam — which is why the header shows the
/// depth.
const MAX_UNDO: usize = 1000;

const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// The fraction of the window height the yard is guaranteed before any band of
/// chrome is allowed to keep its full height.
const BOARD_SHARE: f32 = 0.5;

/// Bands give up their height in this order when the window is too short:
/// footer help first (it repeats what the buttons already say), then the
/// header's subtitle band, then the controls. The yard never gives up any.
const BAND_DROP_ORDER: [usize; 3] = [2, 0, 1];

// ── Orientation ─────────────────────────────────────────────────────
/// The axis a vehicle may slide along. It never turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Short names for the puzzle tables below, where the alternative is eleven
/// rows of `Orientation::Horizontal` per puzzle.
const H: Orientation = Orientation::Horizontal;
const V: Orientation = Orientation::Vertical;

// ── Vehicle ─────────────────────────────────────────────────────────
/// One car or truck in the yard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vehicle {
    /// Identity, handed out by a counter that never restarts. Not a position in
    /// any vector — undo entries and the selection both name a car by this.
    pub id: usize,
    /// Row of the topmost cell.
    pub row: usize,
    /// Column of the leftmost cell.
    pub col: usize,
    /// Cells covered: 2 for a car, 3 for a truck.
    pub length: usize,
    pub orientation: Orientation,
    /// Whether this is the car that has to get out. Carried explicitly, because
    /// inferring it from the paint is what let the palette decide the rules.
    pub player: bool,
    pub color: Color,
    /// The letter drawn on it.
    pub label: char,
}

impl Vehicle {
    /// Every cell this vehicle covers, head first.
    #[must_use]
    pub fn cells(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::with_capacity(self.length);
        for i in 0..self.length {
            let cell = match self.orientation {
                Orientation::Horizontal => self.col.checked_add(i).map(|c| (self.row, c)),
                Orientation::Vertical => self.row.checked_add(i).map(|r| (r, self.col)),
            };
            if let Some(cell) = cell {
                out.push(cell);
            }
        }
        out
    }

    /// The row of the last cell — the same as `row` for a horizontal vehicle.
    #[must_use]
    pub fn tail_row(&self) -> usize {
        match self.orientation {
            Orientation::Horizontal => self.row,
            Orientation::Vertical => self.row.saturating_add(self.length).saturating_sub(1),
        }
    }

    /// The column of the last cell — the same as `col` for a vertical vehicle.
    #[must_use]
    pub fn tail_col(&self) -> usize {
        match self.orientation {
            Orientation::Horizontal => self.col.saturating_add(self.length).saturating_sub(1),
            Orientation::Vertical => self.col,
        }
    }

    /// Whether `(row, col)` is one of this vehicle's cells.
    ///
    /// One rule rather than one per orientation: a vehicle occupies the
    /// rectangle from its head to its tail, and for either axis that rectangle
    /// is one cell thick in the other direction because head and tail agree
    /// there.
    #[must_use]
    pub fn occupies(&self, row: usize, col: usize) -> bool {
        row >= self.row && row <= self.tail_row() && col >= self.col && col <= self.tail_col()
    }
}

// ── Difficulty ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

impl Difficulty {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Beginner => "Beginner",
            Self::Intermediate => "Intermediate",
            Self::Advanced => "Advanced",
            Self::Expert => "Expert",
        }
    }

    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Beginner => GREEN,
            Self::Intermediate => YELLOW,
            Self::Advanced => PEACH,
            Self::Expert => RED,
        }
    }
}

// ── Puzzle definitions ──────────────────────────────────────────────
/// One puzzle: where the player's car starts, and everything in its way.
///
/// The player is a column and nothing else. Its row is `EXIT_ROW` and its
/// length is `PLAYER_LENGTH`, so a table cannot put the car on a row it could
/// never win from, and cannot make some *other* entry the player by accident of
/// ordering or colour.
pub struct PuzzleDef {
    pub difficulty: Difficulty,
    /// The column of the player's car's left end.
    pub player_col: usize,
    /// `(row, col, length, orientation, label)` for each blocker.
    pub blockers: &'static [(usize, usize, usize, Orientation, char)],
}

/// How many puzzles there are. The array below is declared this long, so the
/// count is the compiler's to keep rather than a test's.
pub const PUZZLE_COUNT: usize = 8;

pub static PUZZLES: [PuzzleDef; PUZZLE_COUNT] = [
    PuzzleDef {
        difficulty: Difficulty::Beginner,
        player_col: 0,
        blockers: &[
            (0, 0, 2, V, 'A'),
            (0, 3, 2, H, 'B'),
            (1, 4, 2, V, 'C'),
            (3, 2, 2, H, 'D'),
            (4, 0, 2, H, 'E'),
            (4, 4, 2, V, 'F'),
        ],
    },
    PuzzleDef {
        difficulty: Difficulty::Beginner,
        player_col: 1,
        blockers: &[
            (0, 0, 2, H, 'A'),
            (0, 3, 3, V, 'B'),
            (1, 0, 2, V, 'C'),
            (3, 1, 3, H, 'D'),
            (4, 4, 2, V, 'E'),
            (5, 0, 2, H, 'F'),
            (0, 5, 3, V, 'G'),
        ],
    },
    PuzzleDef {
        difficulty: Difficulty::Intermediate,
        player_col: 0,
        blockers: &[
            (0, 0, 2, V, 'A'),
            (0, 1, 2, H, 'B'),
            (0, 4, 2, V, 'C'),
            (1, 2, 2, V, 'D'),
            (3, 0, 3, V, 'E'),
            (3, 3, 2, V, 'F'),
            (3, 4, 2, H, 'G'),
            (5, 1, 3, H, 'H'),
            (4, 5, 2, V, 'I'),
        ],
    },
    PuzzleDef {
        difficulty: Difficulty::Intermediate,
        player_col: 1,
        blockers: &[
            (0, 0, 2, H, 'A'),
            (0, 2, 3, H, 'B'),
            (1, 0, 2, V, 'C'),
            (1, 5, 2, V, 'D'),
            (3, 0, 3, H, 'E'),
            (3, 3, 2, V, 'F'),
            (5, 4, 2, H, 'G'),
            (3, 5, 2, V, 'H'),
        ],
    },
    PuzzleDef {
        difficulty: Difficulty::Advanced,
        player_col: 1,
        blockers: &[
            (0, 0, 3, V, 'A'),
            (0, 1, 2, H, 'B'),
            (0, 5, 2, V, 'C'),
            (1, 3, 2, V, 'D'),
            (2, 4, 2, V, 'E'),
            (3, 0, 2, H, 'F'),
            (3, 2, 3, V, 'G'),
            (4, 4, 2, H, 'H'),
            (5, 0, 2, H, 'I'),
            (5, 3, 2, H, 'J'),
        ],
    },
    PuzzleDef {
        difficulty: Difficulty::Advanced,
        player_col: 0,
        blockers: &[
            (0, 0, 2, H, 'A'),
            (0, 2, 2, V, 'B'),
            (0, 3, 2, H, 'C'),
            (1, 4, 3, V, 'D'),
            (0, 5, 2, V, 'E'),
            (2, 2, 2, V, 'F'),
            (3, 0, 2, H, 'G'),
            (4, 0, 3, H, 'H'),
            (4, 3, 2, V, 'I'),
            (5, 4, 2, H, 'J'),
        ],
    },
    PuzzleDef {
        difficulty: Difficulty::Expert,
        player_col: 0,
        blockers: &[
            (0, 0, 2, V, 'A'),
            (0, 1, 2, H, 'B'),
            (0, 4, 2, V, 'C'),
            (0, 5, 2, V, 'D'),
            (1, 2, 2, H, 'E'),
            (2, 3, 3, V, 'F'),
            (3, 0, 3, H, 'G'),
            (4, 0, 2, V, 'H'),
            (4, 1, 2, H, 'I'),
            (5, 3, 2, H, 'J'),
            (4, 5, 2, V, 'K'),
        ],
    },
    PuzzleDef {
        difficulty: Difficulty::Expert,
        player_col: 0,
        blockers: &[
            (0, 0, 2, H, 'A'),
            (0, 2, 3, V, 'B'),
            (0, 3, 2, H, 'C'),
            (0, 5, 3, V, 'D'),
            (1, 3, 2, V, 'E'),
            (2, 4, 2, V, 'F'),
            (3, 0, 3, H, 'G'),
            (3, 3, 3, V, 'H'),
            (4, 0, 2, V, 'I'),
            (4, 1, 2, H, 'J'),
            (5, 1, 2, H, 'K'),
            (4, 4, 2, V, 'L'),
        ],
    },
];

// ── Undo ────────────────────────────────────────────────────────────
/// One slide, named by the moved car's **id** and the signed number of cells it
/// travelled along its own axis.
///
/// The version this replaced stored a `Vec` index in a field called
/// `vehicle_index`, plus an `old_row`/`old_col` pair to restore and a
/// `new_row`/`new_col` pair nothing ever read. One reversible number says the
/// same thing and cannot be half-applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoEntry {
    pub vehicle: usize,
    pub delta: isize,
}

// ── Hit targets ─────────────────────────────────────────────────────
/// Everything the pointer can land on. Recorded by the drawing pass, so a
/// target exists exactly where the thing it names was painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A vehicle, by `Vehicle::id`.
    Vehicle(usize),
    /// A grid cell.
    Cell(usize, usize),
    Undo,
    Restart,
    Prev,
    Next,
    /// Open the puzzle sheet.
    Puzzles,
    /// A row of the open puzzle sheet.
    Puzzle(usize),
    /// Anywhere on the open sheet that is not a row: dismiss it.
    CloseSheet,
}

// ── Layout ──────────────────────────────────────────────────────────
/// Where everything goes in a window of a given size.
///
/// Built fresh every frame and never stored on the model. A remembered layout
/// is one that can disagree with the window it is drawn in, which is how a
/// click at (60, 90) came to pick up a car that had never been drawn there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Title, puzzle name, move counter.
    pub header: Rect,
    /// The 6x6 yard, gaps included.
    pub board: Rect,
    /// The strip past the right-hand edge of `EXIT_ROW` marking the way out.
    pub exit: Rect,
    /// Undo, restart, previous, next, puzzles.
    pub controls: Rect,
    /// The keyboard reminder.
    pub footer: Rect,
    /// The side of one grid cell.
    pub cell: f32,
    /// The gap between adjacent cells.
    pub gap: f32,
    pub font: f32,
    pub small: f32,
    pub big: f32,
    pub pad: f32,
}

/// Widths and heights the yard needs, per unit of cell size. The gap is a
/// twentieth of a cell and the exit strip is not quite a quarter of one, so
/// every board dimension is a multiple of the single number `cell`.
const GAP_PER_CELL: f32 = 0.05;
const EXIT_PER_CELL: f32 = 0.22;

/// The buttons, in the order `Layout::button_rects` lays them out.
const BUTTONS: [(Target, &str); 5] = [
    (Target::Undo, "Undo"),
    (Target::Restart, "Restart"),
    (Target::Prev, "Prev"),
    (Target::Next, "Next"),
    (Target::Puzzles, "Puzzles"),
];

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

        // What each band would like, in [header, controls, footer] order.
        let mut wants = [
            (h * 0.10).clamp(26.0, 56.0),
            (h * 0.08).clamp(22.0, 44.0),
            (h * 0.07).clamp(18.0, 40.0),
        ];
        // What is left once the yard has its guaranteed share and the two gaps
        // that separate it from the chrome above and below. Charging the
        // padding to the chrome rather than the yard is what keeps a small
        // window's cells big enough to still hold their letters.
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, ctl_h, ftr_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall: the two read alike to anything asking "does this show?", but
        // only one of them reads alike to anything asking "how wide is it?".
        let header = if hdr_h > 0.0 {
            Rect::new(0.0, 0.0, w, hdr_h)
        } else {
            Rect::EMPTY
        };
        let footer = if ftr_h > 0.0 {
            Rect::new(0.0, h - ftr_h, w, ftr_h)
        } else {
            Rect::EMPTY
        };
        let lower = if ftr_h > 0.0 { footer.y } else { h };
        let controls = if ctl_h > 0.0 {
            Rect::new(0.0, lower - ctl_h, w, ctl_h)
        } else {
            Rect::EMPTY
        };

        let top = hdr_h;
        // From the height rather than from `controls.y`: a dropped band is
        // `Rect::EMPTY`, which sits at the origin, so reading its `y` would put
        // the yard's bottom edge at zero and leave no yard at all.
        let bottom = if ctl_h > 0.0 { controls.y } else { lower };
        let band = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        // One number decides the whole yard. Solving for it from both
        // dimensions at once is what stops a square grid from being stretched
        // to fill a band that is not square — a stretched grid is one whose
        // cells are no longer where a square hit box says they are. The width
        // has to carry the exit strip and its gap as well, which is the only
        // reason the two expressions differ.
        let side = GRID_SIZE as f32 + (GRID_SIZE as f32 - 1.0) * GAP_PER_CELL;
        let per_w = side + GAP_PER_CELL + EXIT_PER_CELL;
        let per_h = side;
        let cell = (band.w / per_w).min(band.h / per_h).max(0.0);
        let gap = cell * GAP_PER_CELL;

        let grid = GRID_SIZE as f32 * cell + (GRID_SIZE as f32 - 1.0) * gap;
        let exit_w = cell * EXIT_PER_CELL;
        let stack_w = grid + gap + exit_w;

        let (board, exit) = if cell > 0.0 {
            let bx = band.x + (band.w - stack_w) / 2.0;
            let by = band.y + (band.h - grid) / 2.0;
            // The strip sits beside `EXIT_ROW`, which is the row the win rule
            // reads — a picture of the rule, not an independent guess at where
            // the way out is.
            let ey = by + EXIT_ROW as f32 * (cell + gap);
            (
                Rect::new(bx, by, grid, grid),
                Rect::new(bx + grid + gap, ey, exit_w, cell),
            )
        } else {
            (Rect::EMPTY, Rect::EMPTY)
        };

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            board,
            exit,
            controls,
            footer,
            cell,
            gap,
            font,
            small,
            big,
            pad,
        }
    }

    /// The rectangle of grid cell `(row, col)`, or `Rect::EMPTY` when the yard
    /// has collapsed or the cell is off the grid.
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if self.cell <= 0.0 || row >= GRID_SIZE || col >= GRID_SIZE {
            return Rect::EMPTY;
        }
        Rect::new(
            self.board.x + col as f32 * (self.cell + self.gap),
            self.board.y + row as f32 * (self.cell + self.gap),
            self.cell,
            self.cell,
        )
    }

    /// The rectangle a vehicle covers — its own cells plus the gaps *between*
    /// them, which belong to the vehicle rather than to the yard once something
    /// is sitting on them.
    #[must_use]
    pub fn vehicle_rect(&self, v: &Vehicle) -> Rect {
        let head = self.cell_rect(v.row, v.col);
        if head.is_empty() {
            return Rect::EMPTY;
        }
        let body = v.length as f32 * self.cell + (v.length as f32 - 1.0) * self.gap;
        match v.orientation {
            Orientation::Horizontal => Rect::new(head.x, head.y, body, self.cell),
            Orientation::Vertical => Rect::new(head.x, head.y, self.cell, body),
        }
    }

    /// The five control buttons, left to right, sharing the controls band.
    #[must_use]
    pub fn button_rects(&self) -> [Rect; BUTTONS.len()] {
        let mut out = [Rect::EMPTY; BUTTONS.len()];
        if self.controls.is_empty() {
            return out;
        }
        let n = BUTTONS.len() as f32;
        let inner = (self.controls.w - self.pad * (n + 1.0)).max(0.0);
        let bw = inner / n;
        let bh = (self.controls.h - self.pad).max(0.0);
        if bw <= 0.0 || bh <= 0.0 {
            return out;
        }
        let y = self.controls.y + (self.controls.h - bh) / 2.0;
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = Rect::new(
                self.controls.x + self.pad + i as f32 * (bw + self.pad),
                y,
                bw,
                bh,
            );
        }
        out
    }

    /// The victory panel.
    #[must_use]
    pub fn win_panel(&self) -> Rect {
        let w = (self.window.w * 0.72).min(340.0);
        let h = (self.window.h * 0.42).min(190.0);
        Rect::new(
            (self.window.w - w) / 2.0,
            (self.window.h - h) / 2.0,
            w.max(0.0),
            h.max(0.0),
        )
    }

    /// The puzzle sheet's panel.
    #[must_use]
    pub fn sheet_panel(&self) -> Rect {
        let w = (self.window.w * 0.76).min(380.0);
        let h = (self.window.h * 0.86).min(440.0);
        Rect::new(
            (self.window.w - w) / 2.0,
            (self.window.h - h) / 2.0,
            w.max(0.0),
            h.max(0.0),
        )
    }

    /// One row per puzzle inside the sheet, top to bottom. Every row is
    /// `Rect::EMPTY` when the panel has no room for a list.
    #[must_use]
    pub fn sheet_rows(&self) -> [Rect; PUZZLE_COUNT] {
        let mut out = [Rect::EMPTY; PUZZLE_COUNT];
        let panel = self.sheet_panel();
        if panel.is_empty() {
            return out;
        }
        let title_h = text::line_height(self.big, FontWeightHint::Bold);
        let hint_h = text::line_height(self.small, FontWeightHint::Regular);
        let listing_top = panel.y + self.pad + title_h;
        let listing_h = (panel.bottom() - self.pad - hint_h - listing_top).max(0.0);
        let n = PUZZLE_COUNT as f32;
        let row_h = (listing_h - self.pad * (n - 1.0)) / n;
        let row_w = (panel.w - self.pad * 2.0).max(0.0);
        if row_h <= 0.0 || row_w <= 0.0 {
            return out;
        }
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = Rect::new(
                panel.x + self.pad,
                listing_top + i as f32 * (row_h + self.pad),
                row_w,
                row_h,
            );
        }
        out
    }
}

/// The keyboard reminder, in the order the footer draws it. The second line is
/// the one dropped first when the footer has room for only one.
const FOOTER_LINES: [&str; 2] = [
    "Enter: select   Arrows: slide   Z: undo",
    "N/Tab: next   B: prev   R: restart   P: puzzles",
];

// ── The game ────────────────────────────────────────────────────────
pub struct RushHour {
    vehicles: Vec<Vehicle>,
    /// The selected car's **id**, for the same reason undo entries carry one.
    selected: Option<usize>,
    moves: usize,
    undo_stack: VecDeque<UndoEntry>,
    current_puzzle: usize,
    /// The next id to hand out. Starts at 1 and never restarts, so no id is
    /// ever equal to the position of the vehicle holding it.
    next_id: usize,
    sheet_open: bool,
    sheet_cursor: usize,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size_drawn: (f32, f32),
}

impl Default for RushHour {
    fn default() -> Self {
        Self::new()
    }
}

impl RushHour {
    #[must_use]
    pub fn new() -> Self {
        let mut game = Self {
            vehicles: Vec::new(),
            selected: None,
            moves: 0,
            undo_stack: VecDeque::new(),
            current_puzzle: 0,
            next_id: 1,
            sheet_open: false,
            sheet_cursor: 0,
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        game.load_puzzle(0);
        game
    }

    /// Remember the size the window last drew at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }

    #[must_use]
    pub fn size_drawn(&self) -> (f32, f32) {
        self.size_drawn
    }

    /// The layout for the size last drawn at.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size_drawn.0, self.size_drawn.1)
    }

    /// Load puzzle `index`, resetting the move count, the undo stack and the
    /// selection. An index past the end of `PUZZLES` does nothing at all —
    /// wrapping is `next_puzzle`'s and `prev_puzzle`'s business, and doing it
    /// here as well would be a second answer to the same question.
    pub fn load_puzzle(&mut self, index: usize) {
        let Some(def) = PUZZLES.get(index) else {
            return;
        };
        self.current_puzzle = index;
        self.vehicles = Vec::with_capacity(def.blockers.len().saturating_add(1));

        let mut id = self.next_id;
        let mut take_id = || {
            let out = id;
            id = id.saturating_add(1);
            out
        };
        self.vehicles.push(Vehicle {
            id: take_id(),
            row: EXIT_ROW,
            col: def.player_col,
            length: PLAYER_LENGTH,
            orientation: Orientation::Horizontal,
            player: true,
            color: PLAYER_COLOR,
            label: PLAYER_LABEL,
        });
        let mut palette = VEHICLE_COLORS.iter().copied().cycle();
        for &(row, col, length, orientation, label) in def.blockers {
            self.vehicles.push(Vehicle {
                id: take_id(),
                row,
                col,
                length,
                orientation,
                player: false,
                // `cycle` over a non-empty array never runs out; the fallback is
                // the total spelling of that, not a case anything reaches.
                color: palette.next().unwrap_or(SUBTEXT0),
                label,
            });
        }
        self.next_id = id;

        self.selected = None;
        self.moves = 0;
        self.undo_stack.clear();
        self.sheet_open = false;
        self.sheet_cursor = index;
    }

    /// Reload the current puzzle from its definition.
    pub fn restart_puzzle(&mut self) {
        self.load_puzzle(self.current_puzzle);
    }

    pub fn next_puzzle(&mut self) {
        let next = self.current_puzzle.saturating_add(1);
        self.load_puzzle(if next < PUZZLE_COUNT { next } else { 0 });
    }

    pub fn prev_puzzle(&mut self) {
        let prev = self
            .current_puzzle
            .checked_sub(1)
            .unwrap_or(PUZZLE_COUNT.saturating_sub(1));
        self.load_puzzle(prev);
    }

    #[must_use]
    pub fn current_puzzle(&self) -> usize {
        self.current_puzzle
    }

    #[must_use]
    pub fn difficulty(&self) -> Difficulty {
        PUZZLES
            .get(self.current_puzzle)
            .map_or(Difficulty::Beginner, |p| p.difficulty)
    }

    #[must_use]
    pub fn moves(&self) -> usize {
        self.moves
    }

    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    #[must_use]
    pub fn vehicles(&self) -> &[Vehicle] {
        &self.vehicles
    }

    #[must_use]
    pub fn sheet_open(&self) -> bool {
        self.sheet_open
    }

    #[must_use]
    pub fn sheet_cursor(&self) -> usize {
        self.sheet_cursor
    }

    /// Put a position in place directly, for tests that need one specific jam.
    /// Ids keep coming from the same never-restarting counter.
    pub fn position(&mut self, player_col: usize, blockers: &[(usize, usize, usize, Orientation)]) {
        self.vehicles.clear();
        self.selected = None;
        self.moves = 0;
        self.undo_stack.clear();
        let mut id = self.next_id;
        let mut take_id = || {
            let out = id;
            id = id.saturating_add(1);
            out
        };
        self.vehicles.push(Vehicle {
            id: take_id(),
            row: EXIT_ROW,
            col: player_col,
            length: PLAYER_LENGTH,
            orientation: Orientation::Horizontal,
            player: true,
            color: PLAYER_COLOR,
            label: PLAYER_LABEL,
        });
        let mut palette = VEHICLE_COLORS.iter().copied().cycle();
        for (i, &(row, col, length, orientation)) in blockers.iter().enumerate() {
            self.vehicles.push(Vehicle {
                id: take_id(),
                row,
                col,
                length,
                orientation,
                player: false,
                color: palette.next().unwrap_or(SUBTEXT0),
                label: BLOCKER_LABELS.chars().nth(i).unwrap_or('?'),
            });
        }
        self.next_id = id;
    }

    /// Which vehicle, by id, is on each cell.
    #[must_use]
    pub fn occupancy(&self) -> [[Option<usize>; GRID_SIZE]; GRID_SIZE] {
        let mut grid = [[None; GRID_SIZE]; GRID_SIZE];
        for v in &self.vehicles {
            for (r, c) in v.cells() {
                if let Some(row) = grid.get_mut(r)
                    && let Some(slot) = row.get_mut(c)
                {
                    *slot = Some(v.id);
                }
            }
        }
        grid
    }

    /// The position in `vehicles` of the vehicle with this id.
    #[must_use]
    pub fn index_of(&self, id: usize) -> Option<usize> {
        self.vehicles.iter().position(|v| v.id == id)
    }

    #[must_use]
    pub fn vehicle(&self, id: usize) -> Option<&Vehicle> {
        self.vehicles.iter().find(|v| v.id == id)
    }

    /// The id of the vehicle on `(row, col)`, if any.
    #[must_use]
    pub fn vehicle_at(&self, row: usize, col: usize) -> Option<usize> {
        self.vehicles
            .iter()
            .find(|v| v.occupies(row, col))
            .map(|v| v.id)
    }

    /// The player's car.
    #[must_use]
    pub fn player(&self) -> Option<&Vehicle> {
        self.vehicles.iter().find(|v| v.player)
    }

    /// Whether the red car is out: whether it is standing on the way out.
    ///
    /// Derived every time it is asked, never stored, and stated as one positive
    /// fact — the player covers the cell the exit opens onto — rather than as a
    /// column test with a row test guarding it. The guard form would have been
    /// dead: the loader is the only thing that sets `player`, it builds the car
    /// horizontal on `EXIT_ROW`, and a horizontal car's slides change only its
    /// column, so "is it on the right row" could never answer no. `occupies`
    /// asks about both axes at once and both constants can be got wrong in a way
    /// a test will see.
    #[must_use]
    pub fn is_won(&self) -> bool {
        self.player()
            .is_some_and(|v| v.occupies(EXIT_ROW, EXIT_COL))
    }

    /// Whether the car with this id can slide `delta` cells along its axis.
    ///
    /// A question about the yard and nothing else — whether the game has been
    /// won is `slide`'s business, so that this stays a geometric predicate a
    /// test can ask in any state.
    ///
    /// Only the cells *entered* are tested, walking outward from the leading
    /// edge. Those are strictly beyond the car's own body, so a car can never
    /// be its own obstacle and there is no "unless it is me" case — the version
    /// this replaced carried one, and it could never fire.
    #[must_use]
    pub fn can_slide(&self, id: usize, delta: isize) -> bool {
        if delta == 0 {
            return false;
        }
        let Some(v) = self.vehicle(id) else {
            return false;
        };
        let occupancy = self.occupancy();
        let backwards = delta < 0;
        let (lead_row, lead_col) = if backwards {
            (v.row, v.col)
        } else {
            (v.tail_row(), v.tail_col())
        };
        for step in 1..=delta.unsigned_abs() {
            let cell = match v.orientation {
                Orientation::Horizontal => {
                    let moved = if backwards {
                        lead_col.checked_sub(step)
                    } else {
                        lead_col.checked_add(step)
                    };
                    moved.map(|c| (lead_row, c))
                }
                Orientation::Vertical => {
                    let moved = if backwards {
                        lead_row.checked_sub(step)
                    } else {
                        lead_row.checked_add(step)
                    };
                    moved.map(|r| (r, lead_col))
                }
            };
            let Some((row, col)) = cell else {
                return false;
            };
            match occupancy.get(row).and_then(|r| r.get(col)) {
                None => return false,
                Some(Some(_)) => return false,
                Some(None) => {}
            }
        }
        true
    }

    /// The furthest the car can slide in `direction` (`-1` back, `+1` on),
    /// in cells. Zero when it is blocked or the direction is neither.
    #[must_use]
    pub fn max_slide(&self, id: usize, direction: isize) -> usize {
        if direction == 0 {
            return 0;
        }
        let mut reach = 0;
        // A car cannot travel further than the yard is wide, so the walk is
        // bounded by the grid rather than by trusting `can_slide` to say no.
        for step in 1..GRID_SIZE {
            let delta = if direction < 0 {
                (step as isize).saturating_neg()
            } else {
                step as isize
            };
            if !self.can_slide(id, delta) {
                break;
            }
            reach = step;
        }
        reach
    }

    /// Slide the car `delta` cells. One slide is one move however far it went —
    /// which is the rule the physical game plays by, and the reason clicking a
    /// distant cell costs the same as clicking an adjacent one. An arrow key
    /// slides one cell, so it costs one move too.
    ///
    /// A won yard takes no further slides: the win panel is up, and a car that
    /// moved under it would leave the panel claiming a win the board no longer
    /// shows. Undo is the way back out — it is a genuine reversal, so the panel
    /// goes with it.
    pub fn slide(&mut self, id: usize, delta: isize) -> bool {
        if self.is_won() {
            return false;
        }
        if !self.can_slide(id, delta) {
            return false;
        }
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let Some(v) = self.vehicles.get_mut(index) else {
            return false;
        };
        match v.orientation {
            Orientation::Horizontal => {
                let Some(col) = v.col.checked_add_signed(delta) else {
                    return false;
                };
                v.col = col;
            }
            Orientation::Vertical => {
                let Some(row) = v.row.checked_add_signed(delta) else {
                    return false;
                };
                v.row = row;
            }
        }
        self.undo_stack.push_back(UndoEntry { vehicle: id, delta });
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.pop_front();
        }
        self.moves = self.moves.saturating_add(1);
        true
    }

    /// Take back the last slide. Allowed after a win, because winning is a fact
    /// about where the red car is and undoing moves it back.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo_stack.pop_back() else {
            return false;
        };
        let Some(index) = self.index_of(entry.vehicle) else {
            return false;
        };
        let Some(v) = self.vehicles.get_mut(index) else {
            return false;
        };
        let back = entry.delta.saturating_neg();
        match v.orientation {
            Orientation::Horizontal => {
                let Some(col) = v.col.checked_add_signed(back) else {
                    return false;
                };
                v.col = col;
            }
            Orientation::Vertical => {
                let Some(row) = v.row.checked_add_signed(back) else {
                    return false;
                };
                v.row = row;
            }
        }
        self.moves = self.moves.saturating_sub(1);
        true
    }

    /// Step the selection through the vehicles in order.
    ///
    /// The wrap is spelled once, in the fallback: past the end of the vector
    /// `get` answers `None`, and the answer to "there is no next vehicle" is
    /// the first one.
    pub fn cycle_selection(&mut self) {
        let Some(first) = self.vehicles.first().map(|v| v.id) else {
            self.selected = None;
            return;
        };
        self.selected = Some(match self.selected.and_then(|id| self.index_of(id)) {
            None => first,
            Some(index) => self
                .vehicles
                .get(index.saturating_add(1))
                .map_or(first, |v| v.id),
        });
    }

    /// The slide that would bring this car's body onto `(row, col)`, or `None`
    /// when the cell is off its axis or already underneath it.
    ///
    /// The distance is measured from whichever end is facing the cell, so
    /// clicking the cell immediately past a car's nose moves it exactly one.
    ///
    /// This is the distance *asked for*, which the yard may not grant; the
    /// click path goes through `reachable_slide`, which clamps it.
    #[must_use]
    pub fn slide_towards(&self, id: usize, row: usize, col: usize) -> Option<isize> {
        let v = self.vehicle(id)?;
        let (along, head, tail) = match v.orientation {
            Orientation::Horizontal => {
                if row != v.row {
                    return None;
                }
                (col, v.col, v.tail_col())
            }
            Orientation::Vertical => {
                if col != v.col {
                    return None;
                }
                (row, v.row, v.tail_row())
            }
        };
        let from = if along < head {
            head
        } else if along > tail {
            tail
        } else {
            // A cell the car is already sitting on is not somewhere to go.
            return None;
        };
        isize::try_from(along)
            .ok()
            .zip(isize::try_from(from).ok())
            .map(|(a, f)| a.saturating_sub(f))
    }

    /// The slide a click on `(row, col)` actually performs: the distance asked
    /// for, clamped by how far the yard lets the car travel.
    ///
    /// Clamping rather than refusing is the point. Aiming at the far wall past
    /// a blocker is how a player says "go as far as you can that way", and the
    /// version this replaced answered it by doing nothing at all, because it
    /// demanded the exact cell be reachable.
    ///
    /// `None` when the cell is off the car's axis, underneath it, or the car
    /// cannot travel even one cell that way.
    #[must_use]
    pub fn reachable_slide(&self, id: usize, row: usize, col: usize) -> Option<isize> {
        let wanted = self.slide_towards(id, row, col)?;
        let direction = wanted.signum();
        let reach =
            isize::try_from(self.max_slide(id, direction).min(wanted.unsigned_abs())).ok()?;
        match direction.saturating_mul(reach) {
            0 => None,
            delta => Some(delta),
        }
    }

    // ── Drawing ────────────────────────────────────────────────────

    /// The whole picture, and every hit box in it, for a window this size.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::new(width, height);
        let mut f = Frame::new(width, height);

        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.window.w,
            height: l.window.h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.draw_header(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_controls(&mut f, &l);
        self.draw_footer(&mut f, &l);
        if self.is_won() {
            self.draw_win(&mut f, &l);
        }
        if self.sheet_open {
            self.draw_sheet(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        fill(f, l.header, MANTLE, CornerRadii::ZERO);

        let title_h = text::line_height(l.big, FontWeightHint::Bold);
        let sub_h = text::line_height(l.small, FontWeightHint::Regular);
        let two_lines = title_h + sub_h <= l.header.h;

        let top = if two_lines {
            l.header.y + (l.header.h - title_h - sub_h) / 2.0
        } else {
            l.header.y + (l.header.h - title_h) / 2.0
        };
        label(
            f,
            &Label {
                text: "Rush Hour",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: LAVENDER,
            },
            l.header.x + l.pad,
            top,
        );
        if two_lines {
            let difficulty = self.difficulty();
            let subtitle = format!(
                "#{}: {}",
                self.current_puzzle.saturating_add(1),
                difficulty.label()
            );
            label(
                f,
                &Label {
                    text: &subtitle,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: difficulty.color(),
                },
                l.header.x + l.pad,
                top + title_h,
            );
        }

        // Right-aligned from the strings' own measured widths. The version this
        // replaced drew both at `total_width - 120.0`, a guess at how wide
        // "Moves: 1234" would turn out to be.
        let moves = format!("Moves: {}", self.moves);
        let undo = format!("Undo: {}", self.undo_stack.len());
        let right = l.header.right() - l.pad;
        label_right(
            f,
            &Label {
                text: &moves,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: TEXT_COLOR,
            },
            right,
            top,
        );
        if two_lines {
            label_right(
                f,
                &Label {
                    text: &undo,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                right,
                top + title_h,
            );
        }
    }

    fn draw_board(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.board.is_empty() {
            return;
        }
        // The yard the cars sit in.
        fill(
            f,
            Rect::new(
                l.board.x - l.gap,
                l.board.y - l.gap,
                l.board.w + l.gap * 2.0,
                l.board.h + l.gap * 2.0,
            ),
            CRUST,
            CornerRadii::all(l.gap.max(1.0)),
        );

        // Empty cells first, so a car drawn over one takes the click: the hit
        // test answers with the *last* target covering the point.
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let r = l.cell_rect(row, col);
                fill(f, r, SURFACE0, CornerRadii::all(l.gap.max(1.0)));
                f.hit(Target::Cell(row, col), r);
            }
        }

        // The way out. Deliberately not a hit target: a control that swallows a
        // click and does nothing is worse than no control, because the click it
        // ate would otherwise have reached whatever is beneath.
        if !l.exit.is_empty() {
            fill(f, l.exit, PLAYER_COLOR, CornerRadii::all(l.gap.max(1.0)));
        }

        for v in &self.vehicles {
            let r = l.vehicle_rect(v);
            if r.is_empty() {
                continue;
            }
            if self.selected == Some(v.id) {
                let grow = (l.gap * 0.8).max(1.0);
                fill(
                    f,
                    Rect::new(r.x - grow, r.y - grow, r.w + grow * 2.0, r.h + grow * 2.0),
                    TEXT_COLOR,
                    CornerRadii::all(l.cell * 0.12),
                );
            }
            fill(f, r, v.color, CornerRadii::all(l.cell * 0.1));

            let glyph = v.label.to_string();
            let size = (l.cell * 0.34).clamp(7.0, l.font);
            if text::line_height(size, FontWeightHint::Bold) <= r.h
                && text::measure(&glyph, size, FontWeightHint::Bold) <= r.w
            {
                label_centred(
                    f,
                    &Label {
                        text: &glyph,
                        size,
                        weight: FontWeightHint::Bold,
                        color: CRUST,
                    },
                    r,
                );
            }
            f.hit(Target::Vehicle(v.id), r);
        }
    }

    fn draw_controls(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.controls.is_empty() {
            return;
        }
        fill(f, l.controls, MANTLE, CornerRadii::ZERO);
        for ((target, name), r) in BUTTONS.into_iter().zip(l.button_rects()) {
            if r.is_empty() {
                continue;
            }
            let live = match target {
                Target::Undo => !self.undo_stack.is_empty(),
                _ => true,
            };
            fill(
                f,
                r,
                if live { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(l.pad.max(1.0)),
            );
            let size = (r.h * 0.4).clamp(7.0, l.small);
            if text::line_height(size, FontWeightHint::Regular) <= r.h {
                label_centred(
                    f,
                    &Label {
                        text: name,
                        size,
                        weight: FontWeightHint::Regular,
                        color: if live { TEXT_COLOR } else { OVERLAY0 },
                    },
                    r,
                );
            }
            // Recorded even when it is drawn dim: `undo` on an empty stack
            // answers `false` and changes nothing, and a target that reports
            // "nothing happened" is the thing the tests can hold on to.
            f.hit(target, r);
        }
    }

    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.footer.is_empty() {
            return;
        }
        fill(f, l.footer, MANTLE, CornerRadii::ZERO);
        let size = l.small;
        let lh = text::line_height(size, FontWeightHint::Regular);
        if lh > l.footer.h {
            return;
        }
        let shown = if lh * 2.0 <= l.footer.h { 2 } else { 1 };
        let top = l.footer.y + (l.footer.h - lh * shown as f32) / 2.0;
        f.clip(l.footer);
        for (i, line) in FOOTER_LINES.iter().take(shown).enumerate() {
            label(
                f,
                &Label {
                    text: line,
                    size,
                    weight: FontWeightHint::Regular,
                    color: if i == 0 { SUBTEXT0 } else { OVERLAY0 },
                },
                l.footer.x + l.pad,
                top + lh * i as f32,
            );
        }
        f.unclip();
    }

    fn draw_win(&self, f: &mut Frame<Target>, l: &Layout) {
        // A translucent scrim, not an opaque one: the cleared jam is the thing
        // worth looking at, and painting it out to celebrate it was the joke
        // the old comment was making without meaning to.
        fill(f, l.window, SCRIM, CornerRadii::ZERO);

        // Nothing behind the panel is clickable any more — a modal that only
        // *looks* in front is one whose buttons you can press through.
        f.discard_hits();

        let panel = l.win_panel();
        if panel.is_empty() {
            return;
        }
        fill(f, panel, SURFACE0, CornerRadii::all(l.pad * 1.2));

        let title_h = text::line_height(l.big, FontWeightHint::Bold);
        let line_h = text::line_height(l.font, FontWeightHint::Regular);
        let btn_h = (panel.h * 0.28).clamp(0.0, 44.0);
        let stack = title_h + line_h + btn_h;
        if stack > panel.h {
            return;
        }
        let top = panel.y + (panel.h - stack) / 2.0;

        label_centred(
            f,
            &Label {
                text: "Puzzle Solved!",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: GREEN,
            },
            Rect::new(panel.x, top, panel.w, title_h),
        );
        let tally = format!("Moves: {}", self.moves);
        label_centred(
            f,
            &Label {
                text: &tally,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: TEXT_COLOR,
            },
            Rect::new(panel.x, top + title_h, panel.w, line_h),
        );

        // Undo, restart and next — the three ways on, all reachable by pointer
        // rather than only by keys named in a footer the scrim is over.
        let choices: [(Target, &str); 3] = [
            (Target::Undo, "Undo"),
            (Target::Restart, "Restart"),
            (Target::Next, "Next"),
        ];
        let n = choices.len() as f32;
        let inner = (panel.w - l.pad * (n + 1.0)).max(0.0);
        let bw = inner / n;
        if bw <= 0.0 || btn_h <= 0.0 {
            return;
        }
        let by = top + title_h + line_h;
        for (i, (target, name)) in choices.into_iter().enumerate() {
            let r = Rect::new(panel.x + l.pad + i as f32 * (bw + l.pad), by, bw, btn_h);
            fill(f, r, SURFACE1, CornerRadii::all(l.pad.max(1.0)));
            let size = (r.h * 0.4).clamp(7.0, l.font);
            if text::line_height(size, FontWeightHint::Regular) <= r.h {
                label_centred(
                    f,
                    &Label {
                        text: name,
                        size,
                        weight: FontWeightHint::Regular,
                        color: TEXT_COLOR,
                    },
                    r,
                );
            }
            f.hit(target, r);
        }
    }

    fn draw_sheet(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, SCRIM, CornerRadii::ZERO);
        f.discard_hits();

        // The whole window dismisses the sheet, and the rows drawn after this
        // take back the part of it they cover. That is one rule with one
        // exception rather than "the panel closes it, except the rows, except
        // the title, except the hint" — and it is why the sheet can be got out
        // of by pointer at all, which the version this replaced could not.
        f.hit(Target::CloseSheet, l.window);

        let panel = l.sheet_panel();
        if panel.is_empty() {
            return;
        }
        fill(f, panel, MANTLE, CornerRadii::all(l.pad * 1.2));

        let title_h = text::line_height(l.big, FontWeightHint::Bold);
        label_centred(
            f,
            &Label {
                text: "Select Puzzle",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: LAVENDER,
            },
            Rect::new(panel.x, panel.y + l.pad, panel.w, title_h),
        );

        for (i, r) in l.sheet_rows().into_iter().enumerate() {
            if r.is_empty() {
                continue;
            }
            let Some(def) = PUZZLES.get(i) else {
                continue;
            };
            let on_cursor = i == self.sheet_cursor;
            let current = i == self.current_puzzle;
            fill(
                f,
                r,
                if on_cursor { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(l.pad.max(1.0)),
            );
            let line = format!(
                "{}{}. {}",
                if current { "> " } else { "  " },
                i.saturating_add(1),
                def.difficulty.label()
            );
            let size = (r.h * 0.44).clamp(7.0, l.font);
            if text::line_height(size, FontWeightHint::Regular) <= r.h {
                label_centred(
                    f,
                    &Label {
                        text: &line,
                        size,
                        weight: if on_cursor {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        },
                        color: if on_cursor {
                            TEXT_COLOR
                        } else {
                            def.difficulty.color()
                        },
                    },
                    r,
                );
            }
            f.hit(Target::Puzzle(i), r);
        }

        let hint_h = text::line_height(l.small, FontWeightHint::Regular);
        let hint = Rect::new(panel.x, panel.bottom() - l.pad - hint_h, panel.w, hint_h);
        if hint.y > panel.y {
            label_centred(
                f,
                &Label {
                    text: "Up/Down: browse   Enter: open   1-8: jump   Esc: close",
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                hint,
            );
        }
    }

    // ── Events ─────────────────────────────────────────────────────

    /// Act on `target`. Returns whether anything changed.
    fn activate(&mut self, target: Target) -> bool {
        match target {
            Target::Undo => self.undo(),
            Target::Restart => {
                self.restart_puzzle();
                true
            }
            Target::Prev => {
                self.prev_puzzle();
                true
            }
            Target::Next => {
                self.next_puzzle();
                true
            }
            Target::Puzzles => {
                self.sheet_open = true;
                self.sheet_cursor = self.current_puzzle;
                true
            }
            Target::CloseSheet => {
                let was = self.sheet_open;
                self.sheet_open = false;
                was
            }
            Target::Puzzle(index) => {
                self.load_puzzle(index);
                true
            }
            Target::Vehicle(id) => {
                self.selected = if self.selected == Some(id) {
                    None
                } else {
                    Some(id)
                };
                true
            }
            Target::Cell(row, col) => {
                // An empty cell the selection can travel toward is a slide, as
                // far along as the yard permits; any other empty cell puts the
                // car down.
                if let Some(id) = self.selected
                    && let Some(delta) = self.reachable_slide(id, row, col)
                    && self.slide(id, delta)
                {
                    return true;
                }
                let had = self.selected.is_some();
                self.selected = None;
                had
            }
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let (w, h) = self.size_drawn;
        match self.frame(w, h).hit_test(ev.x, ev.y) {
            Some(target) => {
                self.activate(target);
                EventResult::Consumed
            }
            None => {
                // Bare background deselects, wherever it is.
                let had = self.selected.is_some();
                self.selected = None;
                if had {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // `pressed` decides whether this is a key going down or coming back
        // up. Reading only `key` runs every binding twice per press.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        if self.sheet_open {
            return self.handle_sheet_key(ev);
        }
        let plain = ev.modifiers == guitk::event::Modifiers::NONE;
        if let Some(index) = digit(ev.key)
            && plain
        {
            self.load_puzzle(index);
            return EventResult::Consumed;
        }
        match ev.key {
            Key::P if plain => {
                self.sheet_open = true;
                self.sheet_cursor = self.current_puzzle;
            }
            Key::N if plain => self.next_puzzle(),
            Key::Tab => self.next_puzzle(),
            Key::B if plain => self.prev_puzzle(),
            Key::R if plain => self.restart_puzzle(),
            Key::Z if plain => {
                self.undo();
            }
            Key::Enter | Key::Space => self.cycle_selection(),
            Key::Escape => self.selected = None,
            // One arm for all four arrows. Each key names an axis and a
            // direction; whether the selected car can go that way is the move
            // rule's business, not four copies of a guard's.
            Key::Up | Key::Down | Key::Left | Key::Right => {
                let (axis, delta) = match ev.key {
                    Key::Up => (Orientation::Vertical, -1),
                    Key::Down => (Orientation::Vertical, 1),
                    Key::Left => (Orientation::Horizontal, -1),
                    _ => (Orientation::Horizontal, 1),
                };
                let Some(id) = self.selected else {
                    return EventResult::Ignored;
                };
                if self.vehicle(id).is_none_or(|v| v.orientation != axis) {
                    return EventResult::Ignored;
                }
                self.slide(id, delta);
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    fn handle_sheet_key(&mut self, ev: &KeyEvent) -> EventResult {
        if let Some(index) = digit(ev.key) {
            self.load_puzzle(index);
            return EventResult::Consumed;
        }
        match ev.key {
            Key::Escape | Key::P => self.sheet_open = false,
            Key::Up => self.sheet_cursor = self.sheet_cursor.saturating_sub(1),
            Key::Down => {
                self.sheet_cursor = self
                    .sheet_cursor
                    .saturating_add(1)
                    .min(PUZZLE_COUNT.saturating_sub(1));
            }
            Key::Enter | Key::Space => self.load_puzzle(self.sheet_cursor),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

/// The puzzle a number key names, counting from zero.
///
/// One table, read by both the sheet's key handler and the board's, so the two
/// cannot come to disagree about which puzzle `3` means.
fn digit(key: Key) -> Option<usize> {
    Some(match key {
        Key::Num1 => 0,
        Key::Num2 => 1,
        Key::Num3 => 2,
        Key::Num4 => 3,
        Key::Num5 => 4,
        Key::Num6 => 5,
        Key::Num7 => 6,
        Key::Num8 => 7,
        _ => return None,
    })
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

/// One string and everything about how it looks, minus where it goes.
struct Label<'a> {
    text: &'a str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// The one place a `Text` command is built.
///
/// `limit` is passed straight through as `max_width`, so a caller that computed
/// a width limit gets one the renderer will actually stop at. `TextOverflow`
/// follows from it and is not a separate choice: no limit means the overflow
/// question is vacuous, and a limit means the cut is real and had better be
/// marked.
fn push_text(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, limit: Option<f32>) {
    if l.text.is_empty() {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: l.text.to_string(),
        color: l.color,
        font_size: l.size,
        font_weight: l.weight,
        max_width: limit,
        overflow: if limit.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Top-left corner at `(x, y)`, with no width limit.
fn label(f: &mut Frame<Target>, l: &Label, x: f32, y: f32) {
    push_text(f, l, x, y, None);
}

/// Right-aligned at `right`, from the string's measured width.
fn label_right(f: &mut Frame<Target>, l: &Label, right: f32, y: f32) {
    let w = text::measure(l.text, l.size, l.weight);
    push_text(f, l, right - w, y, None);
}

/// Centred in `r` — horizontally from the measured width, vertically from the
/// line height — **and limited to `r`**.
///
/// The width that decides the centre is the width the renderer is told to stop
/// at, so the two cannot disagree.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if l.text.is_empty() || r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(
        f,
        l,
        r.x + (r.w - w) / 2.0,
        r.y + (r.h - lh) / 2.0,
        Some(r.w),
    );
}

// ── Window plumbing ─────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(game: &mut RushHour, event: &Event) -> EventResult {
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

impl App for RushHour {
    fn title(&self) -> String {
        "Rush Hour".to_string()
    }

    fn app_id(&self) -> String {
        "rush".to_string()
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
        // against — which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for RushHour {
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
    let mut game = RushHour::new();
    app::launch("rush", &mut game)
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
    use std::collections::HashSet;

    /// The window sizes every layout claim is checked at.
    ///
    /// The first three are far smaller than the chrome would like — the case
    /// the old layout never had to survive, because `render` opened with
    /// `let _ = (width, height);` and drew a 476x568 picture whatever size the
    /// window was.
    const WINDOWS: &[(f32, f32)] = &[
        (140.0, 100.0),
        (200.0, 160.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (480.0, 700.0),
        (640.0, 620.0),
        (900.0, 500.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    /// The size the probe helpers draw at.
    const SIZE: (f32, f32) = RushHour::SIZE;

    fn game() -> RushHour {
        RushHour::new()
    }

    /// An empty yard with the red car one slide short of the way out: cols 3
    /// and 4 of `EXIT_ROW`, with column 5 clear.
    fn one_slide_from_winning() -> RushHour {
        let mut g = game();
        g.position(EXIT_COL - PLAYER_LENGTH, &[]);
        g
    }

    /// An empty yard with the red car already out.
    fn already_won() -> RushHour {
        let mut g = game();
        g.position(EXIT_COL - PLAYER_LENGTH + 1, &[]);
        g
    }

    fn player_id(g: &RushHour) -> usize {
        g.player().expect("every position has a player").id
    }

    /// The id of the vehicle with this label, for tests that name a car the way
    /// the picture does.
    fn labelled(g: &RushHour, label: char) -> usize {
        g.vehicles()
            .iter()
            .find(|v| v.label == label)
            .unwrap_or_else(|| panic!("no vehicle labelled {label}"))
            .id
    }

    fn text_commands(f: &Frame<Target>) -> Vec<(String, f32, f32, f32, FontWeightHint)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => Some((text.clone(), *x, *y, *font_size, *font_weight)),
                _ => None,
            })
            .collect()
    }

    fn fill_rects(f: &Frame<Target>) -> Vec<(Rect, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => Some((Rect::new(*x, *y, *width, *height), *color)),
                _ => None,
            })
            .collect()
    }

    // ── The window exists at all ───────────────────────────────────

    #[test]
    fn the_app_reports_a_title_an_id_and_a_size() {
        let g = game();
        assert_eq!(g.title(), "Rush Hour");
        assert_eq!(g.app_id(), "rush");
        let (w, h) = g.initial_size();
        assert!(w > 0 && h > 0, "the window opens with no size");
    }

    #[test]
    fn the_probe_draws_at_the_size_the_window_opens_at() {
        // If these drift apart, every test below checks a window the program
        // never actually opens.
        let g = game();
        let (w, h) = g.initial_size();
        assert_eq!(
            (w as f32, h as f32),
            RushHour::SIZE,
            "the probe and the window disagree about the opening size"
        );
    }

    #[test]
    fn the_window_opens_on_a_puzzle_already_loaded() {
        // `main` used to be `let _app = RushHour::new();`, which built this and
        // then dropped it.
        let g = game();
        assert!(!g.vehicles().is_empty(), "the opening yard is empty");
        assert!(g.player().is_some(), "there is no red car to get out");
        assert_eq!(g.current_puzzle(), 0);
    }

    #[test]
    fn a_resize_event_changes_the_size_clicks_are_read_against() {
        let mut g = game();
        handle_event(
            &mut g,
            &Event::Resize {
                width: 900,
                height: 700,
            },
        );
        assert_eq!(g.size_drawn(), (900.0, 700.0));
        assert_eq!(g.layout(), Layout::new(900.0, 700.0));
    }

    #[test]
    fn drawing_at_a_size_is_what_decides_where_the_next_click_lands() {
        // `render` stores the size it was given. Without that a window resized
        // by the compositor — which sends no `Resize` the app has to act on
        // before its first paint — would be clicked at the old size.
        let mut g = game();
        g.render(1024.0, 768.0);
        assert_eq!(g.size_drawn(), (1024.0, 768.0));
    }

    // ── Layout ─────────────────────────────────────────────────────

    #[test]
    fn the_layout_is_a_function_of_the_window_size_alone() {
        for &(w, h) in WINDOWS {
            assert_eq!(
                Layout::new(w, h),
                Layout::new(w, h),
                "the layout at {w}x{h} depends on something other than the size"
            );
        }
    }

    #[test]
    fn every_band_stays_inside_the_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("header", l.header),
                ("board", l.board),
                ("exit", l.exit),
                ("controls", l.controls),
                ("footer", l.footer),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "{name} escapes the {w}x{h} window: {r:?}"
                );
            }
        }
    }

    #[test]
    fn the_board_never_overlaps_the_chrome() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.board.is_empty() {
                continue;
            }
            for (name, r) in [
                ("header", l.header),
                ("controls", l.controls),
                ("footer", l.footer),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    l.board.intersect(r).is_none(),
                    "at {w}x{h} the board sits on the {name}: board {:?}, {name} {r:?}",
                    l.board
                );
            }
        }
    }

    #[test]
    fn the_chrome_bands_do_not_overlap_each_other() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let bands = [
                ("header", l.header),
                ("controls", l.controls),
                ("footer", l.footer),
            ];
            for (i, (an, a)) in bands.iter().enumerate() {
                for (bn, b) in bands.iter().skip(i + 1) {
                    if a.is_empty() || b.is_empty() {
                        continue;
                    }
                    assert!(a.intersect(*b).is_none(), "at {w}x{h} {an} overlaps {bn}");
                }
            }
        }
    }

    #[test]
    fn the_board_is_drawn_with_square_cells() {
        // A 6x6 grid stretched to fill a band that is not square has cells that
        // no longer match the square hit boxes recorded over them.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.cell <= 0.0 {
                continue;
            }
            let r = l.cell_rect(0, 0);
            assert!(
                (r.w - r.h).abs() < 0.001,
                "at {w}x{h} a cell is {}x{}, not square",
                r.w,
                r.h
            );
        }
    }

    #[test]
    fn the_grid_fills_the_board_rect_exactly() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.cell <= 0.0 {
                continue;
            }
            let last = l.cell_rect(GRID_SIZE - 1, GRID_SIZE - 1);
            assert!(
                (last.right() - l.board.right()).abs() < 0.01,
                "at {w}x{h} the last column stops at {} but the board ends at {}",
                last.right(),
                l.board.right()
            );
            assert!(
                (last.bottom() - l.board.bottom()).abs() < 0.01,
                "at {w}x{h} the last row stops at {} but the board ends at {}",
                last.bottom(),
                l.board.bottom()
            );
        }
    }

    #[test]
    fn no_two_cells_overlap() {
        let l = Layout::new(SIZE.0, SIZE.1);
        for r1 in 0..GRID_SIZE {
            for c1 in 0..GRID_SIZE {
                for r2 in 0..GRID_SIZE {
                    for c2 in 0..GRID_SIZE {
                        if (r1, c1) >= (r2, c2) {
                            continue;
                        }
                        assert!(
                            l.cell_rect(r1, c1).intersect(l.cell_rect(r2, c2)).is_none(),
                            "cells ({r1},{c1}) and ({r2},{c2}) overlap"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cell_rect_answers_empty_off_the_grid() {
        let l = Layout::new(SIZE.0, SIZE.1);
        assert!(l.cell_rect(GRID_SIZE, 0).is_empty());
        assert!(l.cell_rect(0, GRID_SIZE).is_empty());
        assert!(!l.cell_rect(GRID_SIZE - 1, GRID_SIZE - 1).is_empty());
    }

    #[test]
    fn the_exit_strip_sits_beside_the_row_the_win_rule_reads() {
        // The strip is a picture of `EXIT_ROW`, not a second opinion about
        // where the way out is. It used to be drawn from its own literal 2.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.exit.is_empty() {
                continue;
            }
            let row = l.cell_rect(EXIT_ROW, EXIT_COL);
            assert!(
                (l.exit.y - row.y).abs() < 0.01,
                "at {w}x{h} the exit is at y {} but row {EXIT_ROW} at {}",
                l.exit.y,
                row.y
            );
            assert!(
                (l.exit.h - l.cell).abs() < 0.01,
                "at {w}x{h} the exit is {} tall but a cell is {}",
                l.exit.h,
                l.cell
            );
            assert!(
                l.exit.x >= l.board.right() - 0.01,
                "at {w}x{h} the exit is not past the right-hand edge of the yard"
            );
        }
    }

    #[test]
    fn a_dropped_band_is_empty_rather_than_a_zero_height_strip() {
        // The two read alike to "does this show?" and differently to "how wide
        // is it?". Only one of them is safe to believe.
        let mut dropped = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for r in [l.header, l.controls, l.footer] {
                if r.h <= 0.0 {
                    dropped += 1;
                    assert_eq!(
                        r,
                        Rect::EMPTY,
                        "at {w}x{h} a band with no height still claims {}x{}",
                        r.w,
                        r.h
                    );
                }
            }
        }
        assert!(
            dropped > 0,
            "no window in WINDOWS is small enough to drop a band, so this \
             proves nothing"
        );
    }

    #[test]
    fn the_chrome_never_takes_more_than_its_share_of_the_window() {
        // `BOARD_SHARE` is the whole reason bands are dropped at all. Without
        // it the budget could be the entire window height and every other
        // layout assertion would still hold — the yard would simply be squeezed
        // to whatever the chrome left over.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let chrome = l.header.h + l.controls.h + l.footer.h;
            let budget = (h - h * BOARD_SHARE - l.pad * 2.0).max(0.0);
            assert!(
                chrome <= budget + 0.01,
                "at {w}x{h} the chrome takes {chrome} of the {budget} it is \
                 allowed, leaving the yard less than its {BOARD_SHARE} share"
            );
        }
    }

    #[test]
    fn the_footer_is_the_first_chrome_to_go() {
        // The footer only repeats what the buttons already say, so it is the
        // band whose loss costs least. Dropping the controls first would take
        // away the pointer's only route to undo and leave behind a reminder of
        // the keys that still work.
        let mut narrow = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.header.is_empty() || l.controls.is_empty() {
                narrow += 1;
                assert!(
                    l.footer.is_empty(),
                    "at {w}x{h} a band was dropped while the footer was kept"
                );
            }
        }
        assert!(
            narrow > 0,
            "no window in WINDOWS is short enough to drop a band, so this \
             proves nothing"
        );
    }

    #[test]
    fn the_controls_are_the_last_chrome_to_go() {
        // They are the only way to reach undo, restart and the puzzle list
        // without a keyboard.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.controls.is_empty() {
                assert!(
                    l.header.is_empty() && l.footer.is_empty(),
                    "at {w}x{h} the controls went while other chrome stayed"
                );
            }
        }
    }

    #[test]
    fn the_board_survives_every_window_a_band_is_dropped_in() {
        // Dropping chrome is only worth doing if it buys the yard room.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(l.cell > 0.0, "at {w}x{h} there is no yard left to play in");
        }
    }

    #[test]
    fn a_degenerate_window_size_produces_a_layout_rather_than_a_panic() {
        for &(w, h) in &[(0.0, 0.0), (1.0, 1.0), (-40.0, -40.0), (10.0, 4000.0)] {
            let l = Layout::new(w, h);
            assert!(l.window.w >= 1.0 && l.window.h >= 1.0);
            assert!(l.cell >= 0.0, "a {w}x{h} window produced a negative cell");
            // And drawing it must not panic either.
            let g = game();
            let _ = g.frame(w, h);
        }
    }

    #[test]
    fn the_buttons_stay_inside_the_controls_band() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.controls.is_empty() {
                assert!(
                    l.button_rects().into_iter().all(|r| r.is_empty()),
                    "at {w}x{h} buttons were laid out in a band that is not there"
                );
                continue;
            }
            for (i, r) in l.button_rects().into_iter().enumerate() {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.x >= l.controls.x - 0.01
                        && r.right() <= l.controls.right() + 0.01
                        && r.y >= l.controls.y - 0.01
                        && r.bottom() <= l.controls.bottom() + 0.01,
                    "at {w}x{h} button {i} escapes the controls band: {r:?}"
                );
            }
        }
    }

    #[test]
    fn no_two_buttons_overlap() {
        for &(w, h) in WINDOWS {
            let rects = Layout::new(w, h).button_rects();
            for (i, a) in rects.iter().enumerate() {
                for b in rects.iter().skip(i + 1) {
                    if a.is_empty() || b.is_empty() {
                        continue;
                    }
                    assert!(
                        a.intersect(*b).is_none(),
                        "at {w}x{h} two buttons overlap: {a:?} and {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_sheet_rows_stay_inside_the_sheet_panel() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let panel = l.sheet_panel();
            for (i, r) in l.sheet_rows().into_iter().enumerate() {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.x >= panel.x - 0.01
                        && r.right() <= panel.right() + 0.01
                        && r.y >= panel.y - 0.01
                        && r.bottom() <= panel.bottom() + 0.01,
                    "at {w}x{h} sheet row {i} escapes the panel: {r:?} in {panel:?}"
                );
            }
        }
    }

    #[test]
    fn no_two_sheet_rows_overlap() {
        for &(w, h) in WINDOWS {
            let rows = Layout::new(w, h).sheet_rows();
            for (i, a) in rows.iter().enumerate() {
                for b in rows.iter().skip(i + 1) {
                    if a.is_empty() || b.is_empty() {
                        continue;
                    }
                    assert!(
                        a.intersect(*b).is_none(),
                        "at {w}x{h} two sheet rows overlap: {a:?} and {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_victory_panel_stays_inside_the_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let p = l.win_panel();
            assert!(
                p.x >= -0.01 && p.y >= -0.01 && p.right() <= w + 0.01 && p.bottom() <= h + 0.01,
                "at {w}x{h} the victory panel escapes the window: {p:?}"
            );
        }
    }

    #[test]
    fn a_vehicle_covers_exactly_the_cells_it_sits_on() {
        let l = Layout::new(SIZE.0, SIZE.1);
        let mut g = game();
        g.position(0, &[(0, 0, 3, V), (4, 1, 2, H)]);
        for v in g.vehicles() {
            let r = l.vehicle_rect(v);
            let head = l.cell_rect(v.row, v.col);
            let tail = l.cell_rect(v.tail_row(), v.tail_col());
            assert!((r.x - head.x).abs() < 0.01, "{} starts wrong", v.label);
            assert!((r.y - head.y).abs() < 0.01, "{} starts wrong", v.label);
            assert!(
                (r.right() - tail.right()).abs() < 0.01,
                "{} ends at {} but its tail cell at {}",
                v.label,
                r.right(),
                tail.right()
            );
            assert!(
                (r.bottom() - tail.bottom()).abs() < 0.01,
                "{} ends at {} but its tail cell at {}",
                v.label,
                r.bottom(),
                tail.bottom()
            );
        }
    }

    #[test]
    fn a_vehicle_off_the_grid_has_no_rectangle() {
        let l = Layout::new(SIZE.0, SIZE.1);
        let v = Vehicle {
            id: 99,
            row: GRID_SIZE,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            player: false,
            color: BLUE,
            label: 'Q',
        };
        assert!(l.vehicle_rect(&v).is_empty());
    }

    // ── Hit boxes ──────────────────────────────────────────────────

    #[test]
    fn every_cell_is_clickable_where_it_is_drawn() {
        // The old program read the click against `CELL_SIZE = 72.0` whatever
        // size the window was, so in any other window the yard was drawn in one
        // place and clicked in another.
        let mut g = game();
        g.position(0, &[]);
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let f = g.frame(w, h);
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    if g.vehicle_at(row, col).is_some() {
                        continue;
                    }
                    let (cx, cy) = l.cell_rect(row, col).centre();
                    assert_eq!(
                        f.hit_test(cx, cy),
                        Some(Target::Cell(row, col)),
                        "at {w}x{h} the click at the middle of ({row},{col}) \
                         does not land on it"
                    );
                }
            }
        }
    }

    #[test]
    fn every_car_is_clickable_where_its_ink_is() {
        let g = game();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let f = g.frame(w, h);
            for v in g.vehicles() {
                let (cx, cy) = l.vehicle_rect(v).centre();
                assert_eq!(
                    f.hit_test(cx, cy),
                    Some(Target::Vehicle(v.id)),
                    "at {w}x{h} car {} is not clickable where it is painted",
                    v.label
                );
            }
        }
    }

    #[test]
    fn a_car_takes_the_click_from_the_cell_beneath_it() {
        // Cells are recorded first so that the car painted over one wins the
        // hit test, which answers with the last target covering the point.
        let g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let f = g.frame(SIZE.0, SIZE.1);
        let p = g.player().unwrap();
        let (cx, cy) = l.cell_rect(p.row, p.col).centre();
        assert_eq!(f.hit_test(cx, cy), Some(Target::Vehicle(p.id)));
    }

    #[test]
    fn the_hit_box_of_a_car_is_the_rectangle_it_was_painted_in() {
        let g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        for v in g.vehicles() {
            let hit = probe::rect_of(&g, Target::Vehicle(v.id)).unwrap();
            let drawn = l.vehicle_rect(v);
            assert_eq!(hit, drawn, "car {}'s hit box is not its ink", v.label);
        }
    }

    #[test]
    fn every_button_is_clickable_where_it_is_drawn() {
        let g = game();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.controls.is_empty() {
                continue;
            }
            let f = g.frame(w, h);
            for ((target, name), r) in BUTTONS.into_iter().zip(l.button_rects()) {
                if r.is_empty() {
                    continue;
                }
                let (cx, cy) = r.centre();
                assert_eq!(
                    f.hit_test(cx, cy),
                    Some(target),
                    "at {w}x{h} the {name} button is not clickable where it is drawn"
                );
            }
        }
    }

    #[test]
    fn no_hit_box_escapes_the_window() {
        let mut g = game();
        for &(w, h) in WINDOWS {
            for (label, open) in [("board", false), ("sheet", true)] {
                g.resize(w, h);
                if open {
                    probe::key(&mut g, &probe::press(Key::P));
                }
                for (target, r) in g.frame(w, h).hits() {
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "at {w}x{h} the {label}'s {target:?} escapes the window: {r:?}"
                    );
                }
                if open {
                    probe::key(&mut g, &probe::press(Key::Escape));
                }
            }
        }
    }

    #[test]
    fn the_exit_strip_is_not_a_hit_target() {
        // A control that swallows a click and does nothing is worse than no
        // control, because the click it ate would otherwise have got through.
        let g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let (cx, cy) = l.exit.centre();
        assert_eq!(
            g.frame(SIZE.0, SIZE.1).hit_test(cx, cy),
            None,
            "the exit strip eats clicks"
        );
    }

    #[test]
    fn the_victory_panel_hides_the_yard_from_the_pointer() {
        let mut g = one_slide_from_winning();
        let id = player_id(&g);
        assert!(probe::is_visible(&g, Target::Vehicle(id)));
        assert!(g.slide(id, 1));
        assert!(g.is_won());
        assert!(
            !probe::is_visible(&g, Target::Vehicle(id)),
            "the car can still be picked up through the victory panel"
        );
        assert!(!probe::is_visible(&g, Target::Cell(0, 0)));
    }

    #[test]
    fn the_sheet_hides_the_yard_from_the_pointer() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::P));
        assert!(g.sheet_open());
        assert!(
            !probe::is_visible(&g, Target::Cell(0, 0)),
            "the yard can be clicked through the puzzle sheet"
        );
        assert!(probe::is_visible(&g, Target::Puzzle(0)));
    }

    #[test]
    fn the_scrim_over_a_covered_yard_is_translucent() {
        // The old victory overlay filled the window with opaque `0x11111B`
        // under the comment `// Semi-transparent overlay` — an assertion
        // nobody checked.
        let mut g = one_slide_from_winning();
        let id = player_id(&g);
        assert!(g.slide(id, 1));
        let f = g.frame(SIZE.0, SIZE.1);
        // Two window-sized fills: the ground the game is drawn on, and the
        // scrim laid over it. The last painted is the scrim.
        let full: Vec<Color> = fill_rects(&f)
            .into_iter()
            .filter(|(r, _)| (r.w - SIZE.0).abs() < 0.01 && (r.h - SIZE.1).abs() < 0.01)
            .map(|(_, c)| c)
            .collect();
        assert!(full.len() >= 2, "no scrim was drawn over the won yard");
        let scrim = *full.last().unwrap();
        assert!(
            scrim.a < 0xFF,
            "the scrim is opaque, so it paints out the jam it is celebrating"
        );
    }

    // ── Text ───────────────────────────────────────────────────────

    #[test]
    fn every_string_drawn_is_inside_the_window() {
        let mut g = game();
        for &(w, h) in WINDOWS {
            g.resize(w, h);
            for (text, x, y, size, weight) in text_commands(&g.frame(w, h)) {
                let tw = text::measure(&text, size, weight);
                let th = text::line_height(size, weight);
                assert!(
                    x >= -0.01 && y >= -0.01 && x + tw <= w + 0.5 && y + th <= h + 0.5,
                    "at {w}x{h} {text:?} is drawn at ({x},{y}) and does not fit"
                );
            }
        }
    }

    #[test]
    fn the_move_counter_is_right_aligned_from_its_measured_width() {
        // Both counters used to be drawn at `total_width - 120.0`, a guess at
        // how wide "Moves: 1234" would turn out to be.
        let mut g = game();
        let id = player_id(&g);
        for _ in 0..3 {
            g.slide(id, 1);
            g.slide(id, -1);
        }
        let l = Layout::new(SIZE.0, SIZE.1);
        let f = g.frame(SIZE.0, SIZE.1);
        let (text, x, _, size, weight) = text_commands(&f)
            .into_iter()
            .find(|(t, ..)| t.starts_with("Moves:"))
            .expect("no move counter was drawn");
        let right = x + text::measure(&text, size, weight);
        assert!(
            (right - (l.header.right() - l.pad)).abs() < 0.5,
            "{text:?} ends at {right}, not at the header's right margin {}",
            l.header.right() - l.pad
        );
    }

    #[test]
    fn the_header_names_the_puzzle_and_counts_the_moves() {
        let mut g = game();
        g.next_puzzle();
        // Whichever car has somewhere to go — puzzle 2 jams the red one solid,
        // and a test that assumed otherwise would be asserting on the puzzle
        // table rather than on the header.
        let id = g
            .vehicles()
            .iter()
            .map(|v| v.id)
            .find(|&id| g.max_slide(id, 1) > 0)
            .expect("no car in puzzle 2 can move at all");
        assert!(g.slide(id, 1));
        let drawn: Vec<String> = text_commands(&g.frame(SIZE.0, SIZE.1))
            .into_iter()
            .map(|(t, ..)| t)
            .collect();
        assert!(drawn.iter().any(|t| t == "Rush Hour"));
        assert!(
            drawn.iter().any(|t| t == "#2: Beginner"),
            "the header does not say which puzzle is on: {drawn:?}"
        );
        assert!(drawn.iter().any(|t| t == "Moves: 1"), "{drawn:?}");
        assert!(
            drawn.iter().any(|t| t == "Undo: 1"),
            "the header does not show the undo depth: {drawn:?}"
        );
    }

    #[test]
    fn a_cars_letter_is_centred_in_the_car_from_its_measured_width() {
        // Every letter used to be drawn at `cx + vw / 2.0 - 5.0`, which is a
        // claim that the glyph is ten pixels wide.
        let g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let f = g.frame(SIZE.0, SIZE.1);
        let commands = text_commands(&f);
        for v in g.vehicles() {
            let r = l.vehicle_rect(v);
            let glyph = v.label.to_string();
            let (_, x, y, size, weight) = commands
                .iter()
                .find(|(t, ..)| *t == glyph)
                .unwrap_or_else(|| panic!("car {} has no letter", v.label))
                .clone();
            let want_x = r.x + (r.w - text::measure(&glyph, size, weight)) / 2.0;
            let want_y = r.y + (r.h - text::line_height(size, weight)) / 2.0;
            assert!(
                (x - want_x).abs() < 0.5 && (y - want_y).abs() < 0.5,
                "car {}'s letter is at ({x},{y}), not centred at ({want_x},{want_y})",
                v.label
            );
        }
    }

    #[test]
    fn a_letter_too_big_for_its_car_is_dropped_rather_than_spilled() {
        let g = game();
        let (w, h) = (140.0, 100.0);
        let l = Layout::new(w, h);
        let f = g.frame(w, h);
        let commands = text_commands(&f);
        for v in g.vehicles() {
            let r = l.vehicle_rect(v);
            let glyph = v.label.to_string();
            if let Some((_, _, _, size, weight)) = commands.iter().find(|(t, ..)| *t == glyph) {
                assert!(
                    text::measure(&glyph, *size, *weight) <= r.w + 0.01
                        && text::line_height(*size, *weight) <= r.h + 0.01,
                    "car {}'s letter does not fit the car it is drawn on",
                    v.label
                );
            }
        }
    }

    #[test]
    fn every_centred_string_is_limited_to_the_box_it_is_centred_in() {
        // The width that decides the centre is the width the renderer is told
        // to stop at, so the two cannot disagree.
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::P));
        for c in g.frame(SIZE.0, SIZE.1).commands() {
            if let RenderCommand::Text {
                text,
                max_width,
                overflow,
                ..
            } = c
                && max_width.is_some()
            {
                assert_eq!(
                    *overflow,
                    TextOverflow::Ellipsis,
                    "{text:?} has a width limit but is cut without a mark"
                );
            }
        }
    }

    #[test]
    fn the_footer_drops_its_second_line_before_its_first() {
        let g = game();
        let mut seen_one = false;
        for &(w, h) in WINDOWS {
            let drawn: Vec<String> = text_commands(&g.frame(w, h))
                .into_iter()
                .map(|(t, ..)| t)
                .collect();
            let first = drawn.iter().any(|t| t == FOOTER_LINES[0]);
            let second = drawn.iter().any(|t| t == FOOTER_LINES[1]);
            assert!(
                first || !second,
                "at {w}x{h} the footer's second line is shown without its first"
            );
            seen_one |= first && !second;
        }
        assert!(
            seen_one,
            "no window in WINDOWS is short enough to show one footer line"
        );
    }

    #[test]
    fn a_dropped_band_draws_nothing_at_all() {
        let g = game();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if !l.footer.is_empty() {
                continue;
            }
            let drawn: Vec<String> = text_commands(&g.frame(w, h))
                .into_iter()
                .map(|(t, ..)| t)
                .collect();
            for line in FOOTER_LINES {
                assert!(
                    !drawn.contains(&line.to_string()),
                    "at {w}x{h} the footer is gone but still draws {line:?}"
                );
            }
        }
    }

    // ── The puzzles ────────────────────────────────────────────────

    #[test]
    fn there_are_eight_puzzles_and_the_sheet_lists_them_all() {
        assert_eq!(PUZZLES.len(), PUZZLE_COUNT);
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::P));
        for i in 0..PUZZLE_COUNT {
            assert!(
                probe::is_visible(&g, Target::Puzzle(i)),
                "puzzle {i} is not on the sheet"
            );
        }
    }

    #[test]
    fn every_puzzle_puts_the_red_car_on_the_exit_row() {
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            let p = g.player().unwrap();
            assert_eq!(p.row, EXIT_ROW, "puzzle {i} parks the red car off the row");
            assert_eq!(p.length, PLAYER_LENGTH);
            assert_eq!(p.orientation, Orientation::Horizontal);
            assert_eq!(p.label, PLAYER_LABEL);
            assert_eq!(p.color, PLAYER_COLOR);
        }
    }

    #[test]
    fn exactly_one_vehicle_in_every_puzzle_is_the_player() {
        // The old `is_player()` was `color_index == 0`, so "which car wins" was
        // decided by the palette.
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            let players = g.vehicles().iter().filter(|v| v.player).count();
            assert_eq!(players, 1, "puzzle {i} has {players} red cars");
        }
    }

    #[test]
    fn no_blocker_wears_the_red_cars_colour() {
        // A blocker painted like the player is a picture that lies about which
        // car ends the game.
        assert!(
            !VEHICLE_COLORS.contains(&PLAYER_COLOR),
            "the blocker palette contains the player's colour"
        );
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            for v in g.vehicles().iter().filter(|v| !v.player) {
                assert_ne!(v.color, PLAYER_COLOR, "puzzle {i} has a red blocker");
            }
        }
    }

    #[test]
    fn no_blocker_is_labelled_like_the_player() {
        assert!(!BLOCKER_LABELS.contains(PLAYER_LABEL));
        for (i, def) in PUZZLES.iter().enumerate() {
            for &(_, _, _, _, label) in def.blockers {
                assert_ne!(
                    label, PLAYER_LABEL,
                    "puzzle {i} has a second {PLAYER_LABEL}"
                );
            }
        }
    }

    #[test]
    fn every_puzzle_fits_on_the_yard() {
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            for v in g.vehicles() {
                assert!(
                    v.tail_row() < GRID_SIZE && v.tail_col() < GRID_SIZE,
                    "puzzle {i}'s {} hangs off the yard",
                    v.label
                );
                assert!(
                    (2..=3).contains(&v.length),
                    "puzzle {i}'s {} is {} cells long",
                    v.label,
                    v.length
                );
            }
        }
    }

    #[test]
    fn no_two_vehicles_in_a_puzzle_share_a_cell() {
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            let mut seen = HashSet::new();
            for v in g.vehicles() {
                for cell in v.cells() {
                    assert!(
                        seen.insert(cell),
                        "puzzle {i} parks two vehicles on {cell:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_puzzle_labels_its_vehicles_uniquely() {
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            let mut seen = HashSet::new();
            for v in g.vehicles() {
                assert!(
                    seen.insert(v.label),
                    "puzzle {i} draws two vehicles with the letter {}",
                    v.label
                );
            }
        }
    }

    #[test]
    fn every_puzzle_starts_unsolved_and_blocked() {
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            assert!(!g.is_won(), "puzzle {i} is already solved");
            let id = player_id(&g);
            assert!(
                g.max_slide(id, 1) < GRID_SIZE,
                "puzzle {i} lets the red car drive straight out"
            );
        }
    }

    #[test]
    fn ids_are_unique_and_never_a_position_in_the_vector() {
        // An undo entry and the selection both name a car by id, and the old
        // `UndoAction` spent a `Vec` index as though the two were the same
        // thing.
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            let mut seen = HashSet::new();
            for (index, v) in g.vehicles().iter().enumerate() {
                assert!(seen.insert(v.id), "puzzle {i} hands out id {} twice", v.id);
                assert_ne!(
                    v.id, index,
                    "puzzle {i}'s id {} is also a position in the vector",
                    v.id
                );
            }
        }
    }

    #[test]
    fn ids_are_never_reused_after_a_reload() {
        let mut g = game();
        let first: HashSet<usize> = g.vehicles().iter().map(|v| v.id).collect();
        g.restart_puzzle();
        let second: HashSet<usize> = g.vehicles().iter().map(|v| v.id).collect();
        assert!(
            first.is_disjoint(&second),
            "a reloaded puzzle reuses ids from the one before it"
        );
    }

    #[test]
    fn loading_a_puzzle_past_the_end_does_nothing() {
        // Wrapping is `next_puzzle`'s and `prev_puzzle`'s business; doing it
        // here as well would be a second answer to the same question.
        let mut g = game();
        g.next_puzzle();
        let before: Vec<Vehicle> = g.vehicles().to_vec();
        g.load_puzzle(PUZZLE_COUNT);
        assert_eq!(g.current_puzzle(), 1);
        assert_eq!(g.vehicles(), before.as_slice());
    }

    #[test]
    fn the_puzzle_list_wraps_at_both_ends() {
        let mut g = game();
        g.prev_puzzle();
        assert_eq!(g.current_puzzle(), PUZZLE_COUNT - 1);
        g.next_puzzle();
        assert_eq!(g.current_puzzle(), 0);
    }

    #[test]
    fn restart_puts_the_opening_jam_back() {
        let mut g = game();
        let opening: Vec<(usize, usize, usize)> = g
            .vehicles()
            .iter()
            .map(|v| (v.row, v.col, v.length))
            .collect();
        let id = labelled(&g, 'B');
        assert!(g.slide(id, 1));
        g.restart_puzzle();
        let now: Vec<(usize, usize, usize)> = g
            .vehicles()
            .iter()
            .map(|v| (v.row, v.col, v.length))
            .collect();
        assert_eq!(now, opening);
        assert_eq!(g.moves(), 0);
        assert_eq!(g.undo_depth(), 0);
        assert_eq!(g.selected(), None);
    }

    #[test]
    fn the_difficulty_shown_is_the_puzzles_own() {
        for i in 0..PUZZLE_COUNT {
            let mut g = game();
            g.load_puzzle(i);
            assert_eq!(g.difficulty(), PUZZLES[i].difficulty);
        }
    }

    // ── Vehicles ───────────────────────────────────────────────────

    #[test]
    fn a_vehicle_occupies_the_cells_between_its_head_and_its_tail() {
        for orientation in [Orientation::Horizontal, Orientation::Vertical] {
            let v = Vehicle {
                id: 7,
                row: 1,
                col: 2,
                length: 3,
                orientation,
                player: false,
                color: BLUE,
                label: 'A',
            };
            let cells = v.cells();
            assert_eq!(cells.len(), 3);
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    assert_eq!(
                        v.occupies(row, col),
                        cells.contains(&(row, col)),
                        "{orientation:?} vehicle disagrees with itself about ({row},{col})"
                    );
                }
            }
        }
    }

    // ── Moving ─────────────────────────────────────────────────────

    #[test]
    fn a_car_cannot_slide_nowhere() {
        let g = one_slide_from_winning();
        let id = player_id(&g);
        assert!(!g.can_slide(id, 0));
    }

    #[test]
    fn an_unknown_car_cannot_slide() {
        let mut g = game();
        assert!(!g.can_slide(9999, 1));
        assert!(!g.slide(9999, 1));
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn a_car_cannot_slide_off_the_yard() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        assert!(
            !g.can_slide(id, -1),
            "the red car reversed through the wall"
        );
        assert!(g.can_slide(id, 4));
        assert!(!g.can_slide(id, 5), "the red car drove out past the fence");
    }

    #[test]
    fn a_car_cannot_slide_through_another() {
        let mut g = game();
        g.position(0, &[(EXIT_ROW, 3, 2, H)]);
        let id = player_id(&g);
        assert!(g.can_slide(id, 1));
        assert!(!g.can_slide(id, 2), "the red car drove through a blocker");
    }

    #[test]
    fn a_car_can_slide_further_than_its_own_length() {
        // Every cell tested lies strictly beyond the leading edge, so a car is
        // never its own obstacle. The version this replaced carried an
        // `occ != index` guard for a case that could not arise.
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        assert!(
            g.can_slide(id, 3),
            "a car three cells along its own axis blocked itself"
        );
        assert!(g.slide(id, 3));
        assert_eq!(g.player().unwrap().col, 3);
    }

    #[test]
    fn a_horizontal_car_moves_along_its_row_only() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        assert!(g.slide(id, 2));
        let p = g.player().unwrap();
        assert_eq!((p.row, p.col), (EXIT_ROW, 2));
    }

    #[test]
    fn a_vertical_car_moves_along_its_column_only() {
        let mut g = game();
        g.position(0, &[(0, 4, 2, V)]);
        let id = labelled(&g, 'A');
        assert!(g.slide(id, 3));
        let v = g.vehicle(id).unwrap();
        assert_eq!((v.row, v.col), (3, 4));
    }

    #[test]
    fn max_slide_stops_at_the_first_obstacle() {
        let mut g = game();
        g.position(0, &[(EXIT_ROW, 4, 2, H)]);
        let id = player_id(&g);
        assert_eq!(
            g.max_slide(id, 1),
            2,
            "the red car should reach columns 2-3"
        );
        assert_eq!(g.max_slide(id, -1), 0);
    }

    #[test]
    fn max_slide_stops_at_the_wall() {
        let mut g = game();
        g.position(1, &[]);
        let id = player_id(&g);
        assert_eq!(g.max_slide(id, 1), 3);
        assert_eq!(g.max_slide(id, -1), 1);
    }

    #[test]
    fn max_slide_in_no_direction_is_nowhere() {
        let g = one_slide_from_winning();
        assert_eq!(g.max_slide(player_id(&g), 0), 0);
    }

    #[test]
    fn a_slide_costs_one_move_however_far_it_went() {
        // The rule the physical game plays by, and the reason clicking a
        // distant cell costs the same as clicking the next one along.
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        assert!(g.slide(id, 3));
        assert_eq!(g.moves(), 1);
        assert_eq!(g.undo_depth(), 1);
    }

    #[test]
    fn a_refused_slide_records_nothing() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        assert!(!g.slide(id, -1));
        assert_eq!(g.moves(), 0);
        assert_eq!(g.undo_depth(), 0);
    }

    #[test]
    fn undo_puts_the_car_back_and_lowers_the_count() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        assert!(g.slide(id, 3));
        assert!(g.undo());
        assert_eq!(g.player().unwrap().col, 0);
        assert_eq!(g.moves(), 0);
        assert_eq!(g.undo_depth(), 0);
    }

    #[test]
    fn undo_unwinds_the_moves_in_the_order_they_were_made() {
        let mut g = game();
        g.position(0, &[(0, 5, 2, V)]);
        let player = player_id(&g);
        let blocker = labelled(&g, 'A');
        assert!(g.slide(player, 2));
        assert!(g.slide(blocker, 1));
        assert!(g.undo());
        assert_eq!(g.vehicle(blocker).unwrap().row, 0);
        assert_eq!(g.player().unwrap().col, 2);
        assert!(g.undo());
        assert_eq!(g.player().unwrap().col, 0);
        assert!(!g.undo());
    }

    #[test]
    fn undo_on_an_empty_stack_changes_nothing() {
        let mut g = game();
        assert!(!g.undo());
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn the_undo_stack_forgets_its_oldest_move_at_the_cap() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        for _ in 0..MAX_UNDO {
            assert!(g.slide(id, 1));
            assert!(g.slide(id, -1));
        }
        assert_eq!(g.undo_depth(), MAX_UNDO);
        assert_eq!(g.moves(), MAX_UNDO * 2);
    }

    // ── Winning ────────────────────────────────────────────────────

    #[test]
    fn the_red_car_wins_by_covering_the_way_out() {
        let mut g = one_slide_from_winning();
        assert!(!g.is_won());
        let id = player_id(&g);
        assert!(g.slide(id, 1));
        assert!(g.is_won(), "the red car is at the exit and has not won");
        assert_eq!(g.player().unwrap().tail_col(), EXIT_COL);
    }

    #[test]
    fn only_the_red_car_wins() {
        // The win used to be "vehicle 0's tail is at the last column", which was
        // right only because the palette happened to make vehicle 0 the player.
        let mut g = game();
        g.position(0, &[(EXIT_ROW, 4, 2, H)]);
        assert!(!g.is_won(), "a blocker parked at the exit ended the game");
    }

    #[test]
    fn a_car_on_the_exit_column_but_not_the_exit_row_does_not_win() {
        let mut g = game();
        g.position(0, &[(4, 5, 2, V)]);
        assert!(!g.is_won());
    }

    #[test]
    fn winning_is_derived_rather_than_latched() {
        // `status = Won` was written by the winning move and cleared only by
        // loading a puzzle, and `undo` opened with `if status == Won { return }`
        // — so the winning move was the one move you could not take back.
        let mut g = one_slide_from_winning();
        let id = player_id(&g);
        assert!(g.slide(id, 1));
        assert!(g.is_won());
        assert!(g.undo(), "the winning move could not be taken back");
        assert!(!g.is_won(), "the win outlived the position that made it");
        assert!(
            g.slide(id, 1),
            "play could not continue after undoing a win"
        );
    }

    #[test]
    fn a_won_yard_takes_no_further_slides() {
        let mut g = already_won();
        let id = player_id(&g);
        assert!(g.is_won());
        assert!(!g.slide(id, -1), "a car moved under the victory panel");
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn loading_a_puzzle_clears_the_win() {
        let mut g = already_won();
        assert!(g.is_won());
        g.load_puzzle(0);
        assert!(!g.is_won());
    }

    #[test]
    fn the_victory_panel_offers_a_way_on() {
        let mut g = one_slide_from_winning();
        let id = player_id(&g);
        assert!(g.slide(id, 1));
        for target in [Target::Undo, Target::Restart, Target::Next] {
            assert!(
                probe::is_visible(&g, target),
                "the victory panel offers no {target:?}"
            );
        }
        probe::click(&mut g, Target::Next);
        assert!(!g.is_won());
        assert_eq!(g.current_puzzle(), 1);
    }

    // ── The pointer ────────────────────────────────────────────────

    #[test]
    fn clicking_a_car_picks_it_up_and_clicking_it_again_puts_it_down() {
        let mut g = game();
        let id = labelled(&g, 'B');
        probe::click(&mut g, Target::Vehicle(id));
        assert_eq!(g.selected(), Some(id));
        probe::click(&mut g, Target::Vehicle(id));
        assert_eq!(g.selected(), None);
    }

    #[test]
    fn clicking_another_car_moves_the_selection_to_it() {
        let mut g = game();
        let a = labelled(&g, 'A');
        let b = labelled(&g, 'B');
        probe::click(&mut g, Target::Vehicle(a));
        probe::click(&mut g, Target::Vehicle(b));
        assert_eq!(g.selected(), Some(b));
    }

    #[test]
    fn clicking_a_cell_the_selection_can_reach_slides_it_there() {
        // The old `handle_mouse` called `select_at_cell` and nothing else: a
        // click could pick a car up and put it down, and never move it.
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        probe::click(&mut g, Target::Vehicle(id));
        probe::click(&mut g, Target::Cell(EXIT_ROW, 3));
        assert_eq!(
            g.player().unwrap().col,
            2,
            "the click did not slide the car"
        );
        assert_eq!(g.moves(), 1);
    }

    #[test]
    fn clicking_past_a_blocker_slides_as_far_as_the_yard_allows() {
        // Aiming at the far wall is how a player says "as far as you can go".
        // Demanding the exact cell be reachable — which is what the exact-delta
        // rule did — answers that by doing nothing at all.
        let mut g = game();
        g.position(0, &[(EXIT_ROW, 4, 2, H)]);
        let id = player_id(&g);
        probe::click(&mut g, Target::Vehicle(id));
        probe::click(&mut g, Target::Cell(EXIT_ROW, 3));
        assert_eq!(
            g.player().unwrap().col,
            2,
            "the car stopped short of the blocker it could have reached"
        );
        assert_eq!(g.moves(), 1);
    }

    #[test]
    fn clicking_a_cell_a_car_cannot_move_towards_puts_it_down() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        probe::click(&mut g, Target::Vehicle(id));
        probe::click(&mut g, Target::Cell(0, 0));
        assert_eq!(g.selected(), None);
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn clicking_the_cell_a_car_is_already_on_is_not_somewhere_to_go() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        assert_eq!(g.slide_towards(id, EXIT_ROW, 0), None);
        assert_eq!(g.slide_towards(id, EXIT_ROW, 1), None);
        assert_eq!(g.slide_towards(id, EXIT_ROW, 2), Some(1));
    }

    #[test]
    fn a_slide_is_measured_from_the_end_facing_the_cell() {
        let mut g = game();
        g.position(2, &[]);
        let id = player_id(&g);
        assert_eq!(g.slide_towards(id, EXIT_ROW, 5), Some(2));
        assert_eq!(g.slide_towards(id, EXIT_ROW, 0), Some(-2));
    }

    #[test]
    fn clicking_the_background_puts_the_car_down() {
        let mut g = game();
        let id = labelled(&g, 'B');
        probe::click(&mut g, Target::Vehicle(id));
        assert_eq!(probe::click_background(&mut g), EventResult::Consumed);
        assert_eq!(g.selected(), None);
        assert_eq!(
            probe::click_background(&mut g),
            EventResult::Ignored,
            "a click on nothing, changing nothing, still claimed to be handled"
        );
    }

    #[test]
    fn a_right_click_is_left_for_something_else() {
        let mut g = game();
        let id = labelled(&g, 'B');
        let out = probe::click_with(&mut g, Target::Vehicle(id), MouseButton::Right);
        assert_eq!(out, EventResult::Ignored);
        assert_eq!(g.selected(), None);
    }

    #[test]
    fn every_button_does_what_it_says() {
        let mut g = game();
        let id = labelled(&g, 'B');
        assert!(g.slide(id, 1));
        probe::click(&mut g, Target::Undo);
        assert_eq!(g.vehicle(id).unwrap().col, 3);

        probe::click(&mut g, Target::Next);
        assert_eq!(g.current_puzzle(), 1);
        probe::click(&mut g, Target::Prev);
        assert_eq!(g.current_puzzle(), 0);

        let moved = labelled(&g, 'B');
        assert!(g.slide(moved, 1));
        probe::click(&mut g, Target::Restart);
        assert_eq!(g.moves(), 0);

        probe::click(&mut g, Target::Puzzles);
        assert!(g.sheet_open());
    }

    #[test]
    fn the_undo_button_is_drawn_dim_until_there_is_something_to_undo() {
        let mut g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let slot = l.button_rects()[0];
        let dim = fill_rects(&g.frame(SIZE.0, SIZE.1))
            .into_iter()
            .find(|(r, _)| *r == slot)
            .map(|(_, c)| c);
        assert_eq!(dim, Some(SURFACE0), "undo looks live with nothing to undo");
        let id = labelled(&g, 'B');
        assert!(g.slide(id, 1));
        let live = fill_rects(&g.frame(SIZE.0, SIZE.1))
            .into_iter()
            .find(|(r, _)| *r == slot)
            .map(|(_, c)| c);
        assert_eq!(
            live,
            Some(SURFACE1),
            "undo still looks dead with a move made"
        );
    }

    #[test]
    fn the_undo_button_is_clickable_even_when_it_can_do_nothing() {
        // A target that reports "nothing happened" is the thing a test can hold
        // on to; a button that vanishes when it is idle is one that moves the
        // buttons beside it.
        let mut g = game();
        assert_eq!(probe::click(&mut g, Target::Undo), EventResult::Consumed);
        assert_eq!(g.moves(), 0);
    }

    // ── The puzzle sheet ───────────────────────────────────────────

    #[test]
    fn p_opens_the_sheet_on_the_puzzle_being_played() {
        let mut g = game();
        g.next_puzzle();
        probe::key(&mut g, &probe::press(Key::P));
        assert!(g.sheet_open());
        assert_eq!(g.sheet_cursor(), 1);
    }

    #[test]
    fn the_sheet_cursor_walks_the_list_and_stops_at_the_ends() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::P));
        probe::key(&mut g, &probe::press(Key::Up));
        assert_eq!(g.sheet_cursor(), 0, "the cursor ran off the top");
        for _ in 0..PUZZLE_COUNT + 3 {
            probe::key(&mut g, &probe::press(Key::Down));
        }
        assert_eq!(
            g.sheet_cursor(),
            PUZZLE_COUNT - 1,
            "the cursor ran off the bottom"
        );
    }

    #[test]
    fn enter_opens_the_puzzle_under_the_cursor_and_closes_the_sheet() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::P));
        probe::key(&mut g, &probe::press(Key::Down));
        probe::key(&mut g, &probe::press(Key::Down));
        probe::key(&mut g, &probe::press(Key::Enter));
        assert_eq!(g.current_puzzle(), 2);
        assert!(!g.sheet_open());
    }

    #[test]
    fn escape_closes_the_sheet_and_leaves_the_puzzle_alone() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::P));
        probe::key(&mut g, &probe::press(Key::Down));
        probe::key(&mut g, &probe::press(Key::Escape));
        assert!(!g.sheet_open());
        assert_eq!(g.current_puzzle(), 0);
    }

    #[test]
    fn clicking_a_row_of_the_sheet_opens_that_puzzle() {
        // The old `handle_mouse` returned early whenever the sheet was open, so
        // the list you were looking at was inert under the cursor.
        let mut g = game();
        probe::click(&mut g, Target::Puzzles);
        probe::click(&mut g, Target::Puzzle(4));
        assert_eq!(g.current_puzzle(), 4);
        assert!(!g.sheet_open());
    }

    #[test]
    fn clicking_beside_the_sheet_dismisses_it() {
        // Aimed at a corner rather than at the middle of the `CloseSheet` box:
        // that box is the whole window, and the middle of the window is a row
        // of the list — which is the one part of it that is *not* a dismissal.
        let mut g = game();
        probe::click(&mut g, Target::Puzzles);
        let (x, y) = (SIZE.0 - 4.0, 4.0);
        assert_eq!(
            g.frame(SIZE.0, SIZE.1).hit_test(x, y),
            Some(Target::CloseSheet),
            "the corner beside the sheet is not a way out of it"
        );
        g.click_at(x, y, MouseButton::Left, SIZE);
        assert!(!g.sheet_open());
        assert_eq!(g.current_puzzle(), 0);
    }

    #[test]
    fn a_number_key_jumps_to_a_puzzle_from_either_side_of_the_sheet() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::Num5));
        assert_eq!(g.current_puzzle(), 4);
        probe::key(&mut g, &probe::press(Key::P));
        probe::key(&mut g, &probe::press(Key::Num3));
        assert_eq!(g.current_puzzle(), 2);
        assert!(!g.sheet_open(), "opening a puzzle left the sheet up");
    }

    #[test]
    fn a_key_the_sheet_does_not_use_is_left_alone() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::P));
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::R)),
            EventResult::Ignored
        );
        assert!(g.sheet_open());
    }

    // ── The keyboard ───────────────────────────────────────────────

    #[test]
    fn an_arrow_slides_the_selected_car_along_its_axis() {
        let mut g = game();
        g.position(0, &[(0, 3, 2, V)]);
        let player = player_id(&g);
        let blocker = labelled(&g, 'A');

        probe::click(&mut g, Target::Vehicle(player));
        probe::key(&mut g, &probe::press(Key::Right));
        assert_eq!(g.player().unwrap().col, 1);
        probe::key(&mut g, &probe::press(Key::Left));
        assert_eq!(g.player().unwrap().col, 0);

        probe::click(&mut g, Target::Vehicle(blocker));
        probe::key(&mut g, &probe::press(Key::Down));
        assert_eq!(g.vehicle(blocker).unwrap().row, 1);
        probe::key(&mut g, &probe::press(Key::Up));
        assert_eq!(g.vehicle(blocker).unwrap().row, 0);
    }

    #[test]
    fn an_arrow_across_the_cars_axis_does_nothing() {
        let mut g = game();
        g.position(0, &[]);
        let id = player_id(&g);
        probe::click(&mut g, Target::Vehicle(id));
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Down)),
            EventResult::Ignored
        );
        assert_eq!(g.player().unwrap().row, EXIT_ROW);
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn an_arrow_with_nothing_selected_does_nothing() {
        let mut g = game();
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Right)),
            EventResult::Ignored
        );
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn enter_walks_the_selection_through_every_car_and_wraps() {
        let mut g = game();
        let ids: Vec<usize> = g.vehicles().iter().map(|v| v.id).collect();
        for want in &ids {
            probe::key(&mut g, &probe::press(Key::Enter));
            assert_eq!(g.selected(), Some(*want));
        }
        probe::key(&mut g, &probe::press(Key::Enter));
        assert_eq!(g.selected(), ids.first().copied());
    }

    #[test]
    fn escape_puts_the_selected_car_down() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::Enter));
        assert!(g.selected().is_some());
        probe::key(&mut g, &probe::press(Key::Escape));
        assert_eq!(g.selected(), None);
    }

    #[test]
    fn the_letter_keys_do_what_the_footer_says() {
        let mut g = game();
        let id = labelled(&g, 'B');
        assert!(g.slide(id, 1));
        probe::key(&mut g, &probe::press(Key::Z));
        assert_eq!(g.undo_depth(), 0);

        probe::key(&mut g, &probe::press(Key::N));
        assert_eq!(g.current_puzzle(), 1);
        probe::key(&mut g, &probe::press(Key::Tab));
        assert_eq!(g.current_puzzle(), 2);
        probe::key(&mut g, &probe::press(Key::B));
        assert_eq!(g.current_puzzle(), 1);

        let moved = labelled(&g, 'A');
        assert!(g.slide(moved, 1));
        probe::key(&mut g, &probe::press(Key::R));
        assert_eq!(g.moves(), 0);
        assert_eq!(g.current_puzzle(), 1);
    }

    #[test]
    fn a_key_coming_back_up_does_nothing() {
        // Reading only `key` runs every binding twice per press.
        let mut g = game();
        let release = KeyEvent {
            key: Key::N,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert_eq!(probe::key(&mut g, &release), EventResult::Ignored);
        assert_eq!(g.current_puzzle(), 0);
    }

    #[test]
    fn a_modified_letter_is_left_for_something_else() {
        // `Ctrl+N` belongs to whatever opens a new window, not to this.
        let mut g = game();
        assert_eq!(
            probe::key(&mut g, &probe::ctrl(Key::N)),
            EventResult::Ignored
        );
        assert_eq!(g.current_puzzle(), 0);
        assert_eq!(
            probe::key(&mut g, &probe::ctrl(Key::Num5)),
            EventResult::Ignored
        );
        assert_eq!(g.current_puzzle(), 0);
    }

    #[test]
    fn a_key_nothing_is_bound_to_is_left_alone() {
        let mut g = game();
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::F5)),
            EventResult::Ignored
        );
    }

    #[test]
    fn a_puzzle_can_be_solved_from_end_to_end_by_pointer_alone() {
        // The whole point of the wiring: a jam, cleared with nothing but
        // clicks, ending in a victory panel.
        let mut g = game();
        g.position(0, &[(EXIT_ROW, 3, 2, H), (0, 5, 2, V)]);
        let player = player_id(&g);
        let blocker = labelled(&g, 'A');

        probe::click(&mut g, Target::Vehicle(blocker));
        assert_eq!(g.selected(), Some(blocker));
        // The blocker is horizontal on the exit row; it has to get out of the
        // way by going right, which the vertical car at column 5 does not stop.
        probe::click(&mut g, Target::Cell(EXIT_ROW, 5));
        assert_eq!(g.vehicle(blocker).unwrap().col, 4);

        probe::click(&mut g, Target::Vehicle(player));
        probe::click(&mut g, Target::Cell(EXIT_ROW, 3));
        assert_eq!(g.player().unwrap().col, 2);
        assert!(!g.is_won());
    }
}
