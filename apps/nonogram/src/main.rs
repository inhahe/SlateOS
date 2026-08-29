//! Nonogram — deduce a hidden picture from run-length clues down the side of
//! every row and along the top of every column. Ten built-in pictures at
//! 5x5, 10x10 and 15x15; a cursor you can drive from the keyboard or the
//! pointer; an X mark for "this one is definitely blank"; a check switch that
//! calls out the cells you have got wrong; and a timer.
//!
//! ## What wiring it found
//!
//! `main` was `let _app = NonogramApp::new();` — it built the puzzle
//! catalogue, computed every clue and dropped the lot, so no picture ever
//! reached a screen and no key or click ever arrived.
//!
//! Under that, **the program chose its own window and then drew in absolute
//! pixels inside it.** `render` took no size at all: the select screen
//! declared itself `520.0` wide, and the playing screen measured its own
//! width from the grid. Nothing ever asked how big the window was, so in any
//! window that was not the one the program had in mind the picture was either
//! cut off or marooned in a corner. The layout is solved from the live window
//! size every frame now.
//!
//! **The select list's geometry was written out twice and the two copies had
//! already drifted apart**: the drawing pass drew entries from `x = PADDING`
//! to `x = 520 - PADDING`, and the hit test accepted `PADDING..500.0` — a
//! four-pixel strip down the right-hand edge of every entry that looked
//! clickable and was not (`known-issues.md` lesson 63). The hit boxes are
//! recorded by the drawing pass now, so an entry is clickable exactly where
//! its ink is.
//!
//! **The clue numbers were centred against eyeballed text metrics.**
//! `CLUE_HALF_WIDTH = 4.0` sat under a doc comment claiming "the renderer has
//! no text-metrics call to ask"; [`guitk::text::measure`] has existed all
//! along, and the guess was wrong for every two-digit clue — a `12` was
//! centred as though it were as narrow as a `1`. Every string is measured
//! and given a `max_width` now; there were none before, so nothing was ever
//! clipped to its box.
//!
//! **The pointer could fill a cell and nothing else.** `X`, the mark the
//! whole game is built around, was keyboard-only, as were the check switch
//! and the way back to the menu. Right-click marks, and the footer carries
//! the two switches.
//!
//! Twelve blanket `#![allow(...)]` sat at the top of the file — `dead_code`
//! and `unused_imports` among them, which is what let a program whose `main`
//! discarded its own app compile without a word of complaint.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
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
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Layout proportions ──────────────────────────────────────────────
//
// Everything below is a *share* of something the window gives us, because the
// window is the one measurement the program does not get to choose. The
// figures these replaced were absolute pixels — a 28-pixel cell, a 2-pixel
// gap, a 50-pixel header — which is why the old program had to invent a
// window big enough for them rather than fit itself to one.

/// The gap between two neighbouring cells, per unit of cell size, so that every
/// dimension of the grid is a multiple of the single number `cell`.
const GAP_PER_CELL: f32 = 0.08;

/// The width of one row-clue slot, per unit of cell size. A slot holds one
/// number, and the widest clue in the puzzle decides how many slots the band
/// is deep.
const CLUE_W_PER_CELL: f32 = 0.66;

/// The height of one column-clue slot, per unit of cell size.
const CLUE_H_PER_CELL: f32 = 0.60;

/// The fraction of the window height the picture is guaranteed before the
/// header or the footer keeps its full height. A nonogram with no grid is not
/// a smaller nonogram; it is a blank rectangle.
const BODY_SHARE: f32 = 0.62;

/// The tallest a select-list entry may be, as a fraction of the list's own
/// height, so that four puzzles do not each become a banner.
const ENTRY_SHARE: f32 = 0.16;

const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 720.0;

/// How often the window is asked to send a [`Event::Tick`].
///
/// The old program handled ticks and never received one — nothing in it ever
/// asked for them, so `elapsed_ms` stayed at zero and the timer in the header
/// read `0:00` for the whole game.
const TICK: Duration = Duration::from_millis(200);

// ── Grid sizes ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridSize {
    Small,  // 5x5
    Medium, // 10x10
    Large,  // 15x15
}

impl GridSize {
    fn side(self) -> usize {
        match self {
            Self::Small => 5,
            Self::Medium => 10,
            Self::Large => 15,
        }
    }
}

// ── Cell state ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellMark {
    /// Player has not interacted with this cell.
    Empty,
    /// Player filled this cell (believes it is part of the picture).
    Filled,
    /// Player marked this cell as definitely empty.
    MarkedEmpty,
}

// ── Clue computation ───────────────────────────────────────────────

/// Compute the clue (run-length encoding of filled segments) for a single
/// row or column, given as a slice of booleans (`true` = filled).
fn compute_clue(line: &[bool]) -> Vec<u8> {
    let mut clues = Vec::new();
    let mut run: u8 = 0;
    for &filled in line {
        if filled {
            run = run.saturating_add(1);
        } else {
            if run > 0 {
                clues.push(run);
            }
            run = 0;
        }
    }
    if run > 0 {
        clues.push(run);
    }
    if clues.is_empty() {
        clues.push(0);
    }
    clues
}

/// Compute row clues from a solution grid stored in row-major order.
fn compute_row_clues(solution: &[bool], cols: usize) -> Vec<Vec<u8>> {
    if cols == 0 {
        return Vec::new();
    }
    // A trailing partial row is dropped rather than described, which is what
    // the integer division here used to do by accident.
    solution.chunks_exact(cols).map(compute_clue).collect()
}

/// Compute column clues from a solution grid stored in row-major order.
fn compute_col_clues(solution: &[bool], cols: usize) -> Vec<Vec<u8>> {
    let Some(rows) = solution.len().checked_div(cols) else {
        return Vec::new();
    };
    (0..cols)
        .map(|c| {
            let column: Vec<bool> = solution
                .iter()
                .skip(c)
                .step_by(cols)
                .take(rows)
                .copied()
                .collect();
            compute_clue(&column)
        })
        .collect()
}

// ── Built-in puzzles ───────────────────────────────────────────────

/// A puzzle definition: a name, grid size, and solution bitmap.
#[derive(Clone, Debug)]
struct PuzzleDef {
    name: &'static str,
    size: GridSize,
    /// Row-major solution: `true` means the cell should be filled.
    solution: Vec<bool>,
}

/// Parse a multi-line string picture into a boolean grid.
/// `#` = filled, anything else = empty. Each line is one row.
///
/// A line past the bottom of the grid needs no guard of its own: its cells all
/// index past the end of `grid`, which the `get_mut` below already answers for.
/// A character past the right-hand edge is different — without the `take` it
/// would land on the *next row* rather than off the end — so that one is real
/// (`known-issues.md` lesson 51: a guard in front of a rule that already holds
/// is a line no test can own).
fn parse_picture(s: &str, side: usize) -> Vec<bool> {
    let mut grid = vec![false; side.saturating_mul(side)];
    for (r, line) in s.lines().enumerate() {
        for (c, ch) in line.chars().take(side).enumerate() {
            if ch != '#' {
                continue;
            }
            if let Some(slot) = grid.get_mut(r.saturating_mul(side).saturating_add(c)) {
                *slot = true;
            }
        }
    }
    grid
}

fn builtin_puzzles() -> Vec<PuzzleDef> {
    vec![
        // ── 5x5 puzzles ───────────────────────────────────────
        PuzzleDef {
            name: "Heart",
            size: GridSize::Small,
            solution: parse_picture(
                "\
.#.#.
#####
#####
.###.
..#..",
                5,
            ),
        },
        PuzzleDef {
            name: "Star",
            size: GridSize::Small,
            solution: parse_picture(
                "\
..#..
.###.
#####
.###.
..#..",
                5,
            ),
        },
        PuzzleDef {
            name: "Arrow",
            size: GridSize::Small,
            solution: parse_picture(
                "\
..#..
.##..
#####
.##..
..#..",
                5,
            ),
        },
        PuzzleDef {
            name: "Cross",
            size: GridSize::Small,
            solution: parse_picture(
                "\
.###.
..#..
..#..
..#..
.###.",
                5,
            ),
        },
        // ── 10x10 puzzles ──────────────────────────────────────
        PuzzleDef {
            name: "House",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
....##....
...####...
..######..
.########.
##########
##......##
##.#..#.##
##.#..#.##
##......##
##########",
                10,
            ),
        },
        PuzzleDef {
            name: "Smiley",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
..######..
.########.
#..#..#..#
##.#..#.##
##########
##########
#.######.#
#..####..#
.#..##..#.
..######..",
                10,
            ),
        },
        PuzzleDef {
            name: "Tree",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
....##....
...####...
..######..
.########.
....##....
...####...
..######..
.########.
....##....
....##....",
                10,
            ),
        },
        PuzzleDef {
            name: "Boat",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
.....#....
.....##...
.#...###..
.##..####.
.###.#####
..########
...######.
....####..
..........
##########",
                10,
            ),
        },
        // ── 15x15 puzzles ─────────────────────────────────────
        PuzzleDef {
            name: "Cat",
            size: GridSize::Large,
            solution: parse_picture(
                "\
.#...........#.
##...........##
###.........###
####.......####
#####.....#####
###############
###.##...##.###
###.##...##.###
###############
####.......####
#####.#.#.#####
.#####.#.#####.
..####...####..
...###...###...
....#######....",
                15,
            ),
        },
        PuzzleDef {
            name: "Mushroom",
            size: GridSize::Large,
            solution: parse_picture(
                "\
.....#####.....
...#########...
..###########..
.###..###..###.
###...###...###
###...###...###
.###..###..###.
..###########..
...#########...
.....#####.....
......###......
......###......
.....#####.....
....#######....
....#######....",
                15,
            ),
        },
    ]
}

// ── What a click can land on ────────────────────────────────────────

/// Everything the pointer can reach.
///
/// The drawing pass records one of these against each box it paints, so the
/// hit map is a by-product of the picture rather than a second copy of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// An entry in the puzzle list, by index into the catalogue.
    Puzzle(usize),
    /// A cell of the grid, by row and column.
    Cell(usize, usize),
    /// The switch that highlights wrong cells.
    Check,
    /// Back to the puzzle list.
    Menu,
}

/// Whether an event changed anything the window would need to redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    Consumed,
    Ignored,
}

// ── Layout ──────────────────────────────────────────────────────────

/// The bands a window of a given size is divided into.
///
/// Built fresh every frame and never stored on the model, because a remembered
/// layout is one that can disagree with the window it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Title, timer, and the solved banner.
    pub header: Rect,
    /// Everything between the header and the footer: the grid, or the list.
    pub body: Rect,
    /// Hints, progress, and the two switches.
    pub footer: Rect,
    /// The header title's size.
    pub big: f32,
    /// Body text.
    pub font: f32,
    /// Footer text.
    pub small: f32,
    pub pad: f32,
}

impl Layout {
    /// The bands for a window of the given size.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0);
        let big = (h / 28.0).clamp(9.0, 22.0);
        let font = (h / 42.0).clamp(8.0, 16.0);
        let small = (font - 1.0).max(7.0);

        // The footer gives up its height first and the header second. Both are
        // laid out as full-width strips nought pixels tall rather than
        // `Rect::EMPTY`, because `Rect::is_empty` is `w <= 0.0 || h <= 0.0` —
        // so a dropped band already answers "no" to the only question the
        // drawing code asks, and it still sits where the band would have been.
        // That is what lets the edges below fall out without a guard apiece
        // (`known-issues.md` lesson 51).
        let mut hdr = (h * 0.08).clamp(20.0, 48.0);
        let mut ftr = (h * 0.07).clamp(18.0, 42.0);
        let floor = h * BODY_SHARE;
        if h - hdr - ftr < floor {
            ftr = (h - hdr - floor).max(0.0);
        }
        if h - hdr - ftr < floor {
            hdr = (h - floor).max(0.0);
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
}

/// Where the cells and their clue bands go inside the space left for them.
///
/// The clue bands are part of the picture's own size, not an offset added to
/// it: a puzzle whose widest row clue is four numbers deep gets a wider band
/// and therefore smaller cells in the same window, which is the whole reason
/// this is solved rather than declared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub side: usize,
    /// How many numbers deep the row-clue band is.
    pub row_slots: usize,
    /// How many numbers deep the column-clue band is.
    pub col_slots: usize,
    pub cell: f32,
    pub gap: f32,
    /// The width of one row-clue slot.
    pub clue_w: f32,
    /// The height of one column-clue slot.
    pub clue_h: f32,
    /// The band of row clues down the left of the cells.
    pub row_clues: Rect,
    /// The band of column clues across the top of the cells.
    pub col_clues: Rect,
    /// The cells themselves.
    pub cells: Rect,
    pub clue_font: f32,
}

impl Grid {
    /// The grid for a `side`x`side` puzzle in `area`.
    #[must_use]
    pub fn new(area: Rect, side: usize, row_slots: usize, col_slots: usize) -> Self {
        let across = usize_f32(side);
        if side == 0 || area.is_empty() {
            return Self {
                side,
                row_slots,
                col_slots,
                cell: 0.0,
                gap: 0.0,
                clue_w: 0.0,
                clue_h: 0.0,
                row_clues: Rect::EMPTY,
                col_clues: Rect::EMPTY,
                cells: Rect::EMPTY,
                clue_font: 0.0,
            };
        }

        // One number decides the whole picture. Everything else — the gaps,
        // the clue slots, the bands, the span — is a multiple of it, so the
        // grid cannot come out non-square and a clue cannot drift off the
        // column it describes.
        let spread = across + (across - 1.0) * GAP_PER_CELL;
        let per_w = usize_f32(row_slots) * CLUE_W_PER_CELL + spread;
        let per_h = usize_f32(col_slots) * CLUE_H_PER_CELL + spread;
        let cell = (area.w / per_w).min(area.h / per_h).max(0.0);
        let gap = cell * GAP_PER_CELL;
        let clue_w = cell * CLUE_W_PER_CELL;
        let clue_h = cell * CLUE_H_PER_CELL;
        let span = across * cell + (across - 1.0) * gap;
        let band_w = usize_f32(row_slots) * clue_w;
        let band_h = usize_f32(col_slots) * clue_h;

        // Bands and cells are centred together, so the picture sits in the
        // middle of the window it was given rather than in the corner of a
        // window it chose for itself.
        let x = area.x + (area.w - band_w - span) / 2.0;
        let y = area.y + (area.h - band_h - span) / 2.0;

        Self {
            side,
            row_slots,
            col_slots,
            cell,
            gap,
            clue_w,
            clue_h,
            row_clues: Rect::new(x, y + band_h, band_w, span),
            col_clues: Rect::new(x + band_w, y, span, band_h),
            cells: Rect::new(x + band_w, y + band_h, span, span),
            clue_font: (clue_h * 0.82).clamp(4.0, 20.0),
        }
    }

    /// The step from one cell's near edge to the next one's.
    #[must_use]
    pub fn step(&self) -> f32 {
        self.cell + self.gap
    }

    /// The ink of one cell.
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if row >= self.side || col >= self.side {
            return Rect::EMPTY;
        }
        Rect::new(
            self.cells.x + usize_f32(col) * self.step(),
            self.cells.y + usize_f32(row) * self.step(),
            self.cell,
            self.cell,
        )
    }

    /// The clickable box of one cell: its ink grown by half the gap on every
    /// side.
    ///
    /// Neighbouring boxes abut exactly, so a click in the space between two
    /// cells lands in the nearer one instead of nowhere, and the outside edges
    /// of the grid get the same half-gap of slop as the inside ones. Nonogram
    /// is a game of many rapid clicks, so slop is wanted here; sudoku made the
    /// opposite call for the opposite reason (design-decisions.md §486).
    #[must_use]
    pub fn cell_hit(&self, row: usize, col: usize) -> Rect {
        let r = self.cell_rect(row, col);
        if r.is_empty() {
            return Rect::EMPTY;
        }
        let half = self.gap / 2.0;
        Rect::new(r.x - half, r.y - half, r.w + self.gap, r.h + self.gap)
    }

    /// Slot `slot` of row `row`'s clue, counting from the left of the band.
    #[must_use]
    pub fn row_clue_rect(&self, row: usize, slot: usize) -> Rect {
        if row >= self.side || slot >= self.row_slots {
            return Rect::EMPTY;
        }
        Rect::new(
            self.row_clues.x + usize_f32(slot) * self.clue_w,
            self.cells.y + usize_f32(row) * self.step(),
            self.clue_w,
            self.cell,
        )
    }

    /// Slot `slot` of column `col`'s clue, counting from the top of the band.
    #[must_use]
    pub fn col_clue_rect(&self, col: usize, slot: usize) -> Rect {
        if col >= self.side || slot >= self.col_slots {
            return Rect::EMPTY;
        }
        Rect::new(
            self.cells.x + usize_f32(col) * self.step(),
            self.col_clues.y + usize_f32(slot) * self.clue_h,
            self.cell,
            self.clue_h,
        )
    }
}

/// Where the puzzle entries go on the select screen.
///
/// Every entry is divided the same way — thumbnail hard against the right,
/// size label to its left in a column as wide as the widest label, name in
/// whatever is left — so the three columns line up down the list without
/// anyone choosing a pixel offset for them. The three offsets this replaced
/// (`+12`, `+160`, `+260` from the left margin) were fixed, so a name longer
/// than 144 pixels ran straight through its neighbour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct List {
    pub area: Rect,
    pub count: usize,
    /// The height of one entry's ink.
    pub entry_h: f32,
    /// The step from one entry's top edge to the next one's.
    pub step: f32,
    /// The side of the square thumbnail.
    pub thumb: f32,
    /// The margin inside an entry.
    pub inset: f32,
    /// The width of the size-label column.
    pub size_w: f32,
}

impl List {
    /// The list of `count` entries in `area`, with a size-label column wide
    /// enough for a label of `size_w` pixels.
    #[must_use]
    pub fn new(area: Rect, count: usize, size_w: f32) -> Self {
        if count == 0 || area.is_empty() {
            return Self {
                area,
                count,
                entry_h: 0.0,
                step: 0.0,
                thumb: 0.0,
                inset: 0.0,
                size_w: 0.0,
            };
        }
        let step = (area.h / usize_f32(count)).min(area.h * ENTRY_SHARE);
        let entry_h = step * 0.86;
        let inset = entry_h * 0.12;
        let thumb = (entry_h - inset * 2.0).max(0.0);
        Self {
            area,
            count,
            entry_h,
            step,
            thumb,
            inset,
            size_w: size_w.min((area.w - thumb - inset * 4.0).max(0.0)),
        }
    }

    /// The whole box of entry `i` — its background, and its hit box.
    #[must_use]
    pub fn entry(&self, i: usize) -> Rect {
        if i >= self.count {
            return Rect::EMPTY;
        }
        Rect::new(
            self.area.x,
            self.area.y + usize_f32(i) * self.step,
            self.area.w,
            self.entry_h,
        )
    }

    /// The square thumbnail at the right-hand end of entry `i`.
    #[must_use]
    pub fn thumb_rect(&self, i: usize) -> Rect {
        let e = self.entry(i);
        if e.is_empty() || self.thumb <= 0.0 {
            return Rect::EMPTY;
        }
        Rect::new(
            e.right() - self.inset - self.thumb,
            e.y + self.inset,
            self.thumb,
            self.thumb,
        )
    }

    /// The size label's column in entry `i`.
    #[must_use]
    pub fn size_rect(&self, i: usize) -> Rect {
        let e = self.entry(i);
        if e.is_empty() || self.size_w <= 0.0 {
            return Rect::EMPTY;
        }
        Rect::new(
            e.right() - self.inset * 2.0 - self.thumb - self.size_w,
            e.y,
            self.size_w,
            e.h,
        )
    }

    /// The name's column in entry `i`: whatever the other two leave.
    #[must_use]
    pub fn name_rect(&self, i: usize) -> Rect {
        let e = self.entry(i);
        if e.is_empty() {
            return Rect::EMPTY;
        }
        let x = e.x + self.inset;
        let right = e.right() - self.inset * 3.0 - self.thumb - self.size_w;
        Rect::new(x, e.y, (right - x).max(0.0), e.h)
    }
}

/// A count as a coordinate.
///
/// Grid sides, clue depths and list lengths are all small; the cast is exact
/// for anything under 2^24, and a nonogram with sixteen million rows is not
/// the failure mode worth guarding against.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts here are bounded by the puzzle catalogue"
)]
fn usize_f32(n: usize) -> f32 {
    n as f32
}

// ── Game status ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    /// Puzzle selection screen.
    Select,
    /// Playing a puzzle.
    Playing,
    /// Puzzle solved — show victory.
    Won,
}

// ── Main application struct ────────────────────────────────────────
pub struct NonogramApp {
    screen: Screen,
    /// Index into the puzzle catalogue, or which puzzle is being played.
    selected_puzzle: usize,
    /// All available puzzles.
    puzzles: Vec<PuzzleDef>,
    /// Grid side length (5, 10, or 15).
    grid_side: usize,
    /// The solution grid (row-major booleans).
    solution: Vec<bool>,
    /// The player's current cell marks.
    cells: Vec<CellMark>,
    /// Precomputed row clues.
    row_clues: Vec<Vec<u8>>,
    /// Precomputed column clues.
    col_clues: Vec<Vec<u8>>,
    /// Cursor row.
    cursor_row: usize,
    /// Cursor column.
    cursor_col: usize,
    /// Elapsed milliseconds for the timer.
    elapsed_ms: u64,
    /// Whether check mode is active (highlight errors).
    check_mode: bool,
    /// The index on the select screen that is highlighted.
    select_cursor: usize,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size_drawn: (f32, f32),
}

impl NonogramApp {
    #[must_use]
    pub fn new() -> Self {
        let puzzles = builtin_puzzles();
        Self {
            screen: Screen::Select,
            selected_puzzle: 0,
            puzzles,
            grid_side: 5,
            solution: vec![false; 25],
            cells: vec![CellMark::Empty; 25],
            row_clues: vec![vec![0]; 5],
            col_clues: vec![vec![0]; 5],
            cursor_row: 0,
            cursor_col: 0,
            elapsed_ms: 0,
            check_mode: false,
            select_cursor: 0,
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Start playing a specific puzzle by index.
    fn start_puzzle(&mut self, index: usize) {
        let Some(def) = self.puzzles.get(index).cloned() else {
            return;
        };
        self.selected_puzzle = index;
        self.grid_side = def.size.side();
        let total = self.grid_side.saturating_mul(self.grid_side);
        self.row_clues = compute_row_clues(&def.solution, self.grid_side);
        self.col_clues = compute_col_clues(&def.solution, self.grid_side);
        self.solution = def.solution;
        self.cells = vec![CellMark::Empty; total];
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.elapsed_ms = 0;
        self.check_mode = false;
        self.screen = Screen::Playing;
    }

    /// Where a cell's mark lives in `cells`, and its answer in `solution`, or
    /// `None` if it is off the grid.
    ///
    /// The two vectors are the same shape, so one function answers for both.
    fn index(&self, row: usize, col: usize) -> Option<usize> {
        if row >= self.grid_side || col >= self.grid_side {
            return None;
        }
        row.checked_mul(self.grid_side)?.checked_add(col)
    }

    /// Return the cell mark at (row, col), or `Empty` if out of bounds.
    fn cell_at(&self, row: usize, col: usize) -> CellMark {
        self.index(row, col)
            .and_then(|i| self.cells.get(i))
            .copied()
            .unwrap_or(CellMark::Empty)
    }

    /// Set the cell mark at (row, col).
    fn set_cell(&mut self, row: usize, col: usize, mark: CellMark) {
        if let Some(slot) = self.index(row, col).and_then(|i| self.cells.get_mut(i)) {
            *slot = mark;
        }
    }

    /// Toggle a cell between Empty and Filled.
    fn toggle_fill(&mut self, row: usize, col: usize) {
        let current = self.cell_at(row, col);
        let next = match current {
            CellMark::Empty | CellMark::MarkedEmpty => CellMark::Filled,
            CellMark::Filled => CellMark::Empty,
        };
        self.set_cell(row, col, next);
    }

    /// Toggle a cell between Empty and MarkedEmpty (the X mark).
    fn toggle_mark_empty(&mut self, row: usize, col: usize) {
        let current = self.cell_at(row, col);
        let next = match current {
            CellMark::Empty | CellMark::Filled => CellMark::MarkedEmpty,
            CellMark::MarkedEmpty => CellMark::Empty,
        };
        self.set_cell(row, col, next);
    }

    /// Check whether the player's filled cells match the solution exactly.
    ///
    /// `cells` and `solution` are always the same length — `start_puzzle`
    /// builds both from one side length — so zipping them cannot quietly
    /// ignore a tail of either.
    fn check_win(&self) -> bool {
        self.cells
            .iter()
            .zip(&self.solution)
            .all(|(&mark, &wanted)| (mark == CellMark::Filled) == wanted)
    }

    /// Return whether a cell is an error (filled but should not be, or
    /// marked blank when it should be filled). Used in check mode.
    fn is_error(&self, row: usize, col: usize) -> bool {
        let Some(idx) = self.index(row, col) else {
            return false;
        };
        let Some(&should_fill) = self.solution.get(idx) else {
            return false;
        };
        // A cell you have not touched is not yet a mistake — check mode calls
        // out what you have said, not what you have not said yet.
        match self.cells.get(idx) {
            Some(CellMark::Filled) => !should_fill,
            Some(CellMark::MarkedEmpty) => should_fill,
            Some(CellMark::Empty) | None => false,
        }
    }

    // There was a `filled_correct_count` here -- filled cells that the
    // solution agrees with -- with three tests and no caller.  It is deleted
    // rather than wired in, because the obvious place to wire it is the
    // footer counter, and putting it there would break the game.
    //
    // The footer reads `player_filled_count()/total_filled_in_solution()`:
    // how many cells you have committed against how many the picture needs.
    // That leaks nothing.  Swapping in the correct-count would make the
    // footer a free error detector -- fill a cell, watch whether the number
    // moves -- which is exactly what the `C` key exists to reveal, on
    // purpose, as a deliberate act.  A nonogram is deduction; ambient
    // correctness feedback is not a better version of the same game.
    //
    // Nor does check mode want it: it already flags every wrong cell
    // individually (`is_error`, used at the grid draw), so a summary count
    // would say strictly less than what is already on screen.
    //
    // See known-issues.md lesson 45: a green test on an uncalled function
    // reads to the next person as a statement about the running program.

    /// Count how many cells should be filled in the solution.
    fn total_filled_in_solution(&self) -> usize {
        self.solution.iter().filter(|&&v| v).count()
    }

    /// Number of cells the player has filled (regardless of correctness).
    fn player_filled_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|&&c| c == CellMark::Filled)
            .count()
    }

    /// Format elapsed time as M:SS.
    fn format_time(&self) -> String {
        let total_secs = self.elapsed_ms / 1000;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}:{secs:02}")
    }

    /// Maximum number of clue values among all row clues.
    fn max_row_clue_len(&self) -> usize {
        self.row_clues.iter().map(|c| c.len()).max().unwrap_or(1)
    }

    /// Maximum number of clue values among all column clues.
    fn max_col_clue_len(&self) -> usize {
        self.col_clues.iter().map(|c| c.len()).max().unwrap_or(1)
    }

    // ── Geometry ───────────────────────────────────────────────────
    //
    // Where a cell is painted and which cell a click lands in used to be
    // worked out separately, in absolute pixels, from a `CELL_SIZE` the window
    // had no say in. Both now come from `Grid`, solved from the live window
    // size, and the pointer reads the boxes the drawing pass recorded rather
    // than a second spelling of the same arithmetic (`known-issues.md`
    // lesson 63; design-decisions.md §486).

    /// The grid as it stands in a window of the given size.
    #[must_use]
    pub fn grid(&self, l: &Layout) -> Grid {
        Grid::new(
            l.body,
            self.grid_side,
            self.max_row_clue_len(),
            self.max_col_clue_len(),
        )
    }

    /// The select list as it stands in a window of the given size.
    ///
    /// The size-label column is as wide as the widest label actually in the
    /// catalogue, measured at the size it will be drawn at.
    #[must_use]
    pub fn list(&self, l: &Layout) -> List {
        let widest = self
            .puzzles
            .iter()
            .map(|p| text::measure(&size_label(p.size), l.font, FontWeightHint::Regular))
            .fold(0.0f32, f32::max);
        List::new(l.body, self.puzzles.len(), widest)
    }

    // ── Event handling ─────────────────────────────────────────────

    /// What the window is drawing at, which is what the next click is read
    /// against.
    #[must_use]
    pub fn size_drawn(&self) -> (f32, f32) {
        self.size_drawn
    }

    /// Remember the size the next frame will be drawn at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }

    fn handle_key(&mut self, key_event: &KeyEvent) -> EventResult {
        // A modified key belongs to whatever is listening for shortcuts, not
        // to the grid: Ctrl-X is not the X mark.
        if key_event.modifiers != Modifiers::NONE {
            return EventResult::Ignored;
        }
        match self.screen {
            Screen::Select => self.handle_key_select(key_event),
            Screen::Playing => self.handle_key_playing(key_event),
            Screen::Won => self.handle_key_won(key_event),
        }
    }

    fn handle_key_select(&mut self, key_event: &KeyEvent) -> EventResult {
        match key_event.key {
            Key::Up if self.select_cursor > 0 => {
                self.select_cursor = self.select_cursor.saturating_sub(1);
                EventResult::Consumed
            }
            Key::Down if self.select_cursor.saturating_add(1) < self.puzzles.len() => {
                self.select_cursor = self.select_cursor.saturating_add(1);
                EventResult::Consumed
            }
            Key::Enter | Key::Space => {
                self.start_puzzle(self.select_cursor);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key_playing(&mut self, key_event: &KeyEvent) -> EventResult {
        match key_event.key {
            Key::Up if self.cursor_row > 0 => {
                self.cursor_row = self.cursor_row.saturating_sub(1);
                EventResult::Consumed
            }
            Key::Down if self.cursor_row.saturating_add(1) < self.grid_side => {
                self.cursor_row = self.cursor_row.saturating_add(1);
                EventResult::Consumed
            }
            Key::Left if self.cursor_col > 0 => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
                EventResult::Consumed
            }
            Key::Right if self.cursor_col.saturating_add(1) < self.grid_side => {
                self.cursor_col = self.cursor_col.saturating_add(1);
                EventResult::Consumed
            }
            Key::Enter | Key::Space => {
                self.fill_at(self.cursor_row, self.cursor_col);
                EventResult::Consumed
            }
            Key::X => {
                self.toggle_mark_empty(self.cursor_row, self.cursor_col);
                EventResult::Consumed
            }
            Key::C => {
                self.check_mode = !self.check_mode;
                EventResult::Consumed
            }
            Key::Escape => {
                self.screen = Screen::Select;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key_won(&mut self, key_event: &KeyEvent) -> EventResult {
        match key_event.key {
            Key::Enter | Key::Space | Key::Escape => {
                self.screen = Screen::Select;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Fill or unfill a cell, and notice if that finished the picture.
    ///
    /// The win check lives here rather than at each of the two callers,
    /// because a way of filling a cell that could not also win is a way of
    /// filling a cell that is not in the game.
    fn fill_at(&mut self, row: usize, col: usize) {
        self.toggle_fill(row, col);
        if self.check_win() {
            self.screen = Screen::Won;
        }
    }

    /// Act on whatever the pointer landed on.
    fn activate(&mut self, target: Target, button: MouseButton) -> EventResult {
        match target {
            Target::Puzzle(i) => {
                self.select_cursor = i;
                self.start_puzzle(i);
                EventResult::Consumed
            }
            // Left fills, right marks — the convention every nonogram uses,
            // and the only way the pointer can reach the X mark at all. The
            // old program had none: `X` was keyboard-only.
            Target::Cell(row, col) => {
                if self.screen == Screen::Won {
                    return EventResult::Ignored;
                }
                match button {
                    MouseButton::Right => self.toggle_mark_empty(row, col),
                    _ => self.fill_at(row, col),
                }
                self.cursor_row = row;
                self.cursor_col = col;
                EventResult::Consumed
            }
            Target::Check => {
                self.check_mode = !self.check_mode;
                EventResult::Consumed
            }
            Target::Menu => {
                self.screen = Screen::Select;
                EventResult::Consumed
            }
        }
    }

    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        let button = match ev.kind {
            MouseEventKind::Press(b) => b,
            _ => return EventResult::Ignored,
        };
        let (w, h) = self.size_drawn;
        match self.frame(w, h).hit_test(ev.x, ev.y) {
            Some(target) => self.activate(target, button),
            // Clicking the board away from anything is how you leave the
            // victory screen; anywhere else it means nothing.
            None if self.screen == Screen::Won => {
                self.screen = Screen::Select;
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) -> EventResult {
        if self.screen == Screen::Playing {
            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    // ── Drawing ────────────────────────────────────────────────────

    /// One frame at the given size: the picture and the hit boxes together.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::new(width, height);
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        match self.screen {
            Screen::Select => self.draw_select(&mut f, &l),
            Screen::Playing | Screen::Won => self.draw_playing(&mut f, &l),
        }
        f
    }

    fn draw_select(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, CornerRadii::ZERO);
        label_left(
            f,
            &Label {
                text: "Nonogram — Select Puzzle",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: TEXT_COLOR,
            },
            inset_x(l.header, l.pad * 2.0),
        );

        let list = self.list(l);
        for (i, puzzle) in self.puzzles.iter().enumerate() {
            let entry = list.entry(i);
            if entry.is_empty() {
                continue;
            }
            let selected = i == self.select_cursor;
            fill(
                f,
                entry,
                if selected { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(list.inset),
            );
            label_left(
                f,
                &Label {
                    text: puzzle.name,
                    size: l.font,
                    weight: FontWeightHint::Bold,
                    color: if selected { BLUE } else { TEXT_COLOR },
                },
                list.name_rect(i),
            );
            label_left(
                f,
                &Label {
                    text: &size_label(puzzle.size),
                    size: l.font,
                    weight: FontWeightHint::Regular,
                    color: SUBTEXT0,
                },
                list.size_rect(i),
            );
            draw_thumbnail(
                f,
                puzzle,
                list.thumb_rect(i),
                if selected { BLUE } else { LAVENDER },
            );
            f.hit(Target::Puzzle(i), entry);
        }

        fill(f, l.footer, MANTLE, CornerRadii::ZERO);
        label_left(
            f,
            &Label {
                text: "Up/Down: choose    Enter: play",
                size: l.small,
                weight: FontWeightHint::Regular,
                color: OVERLAY0,
            },
            inset_x(l.footer, l.pad * 2.0),
        );
    }

    fn draw_playing(&self, f: &mut Frame<Target>, l: &Layout) {
        self.draw_header(f, l);
        let g = self.grid(l);
        self.draw_clues(f, &g);
        self.draw_cells(f, &g);
        self.draw_footer(f, l);
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, CornerRadii::ZERO);
        let mut rest = inset_x(l.header, l.pad * 2.0);

        // The timer is carved off the right and the title off the left, so the
        // banner in the middle is whatever the two of them leave rather than a
        // guessed offset from the centre — the old header put "SOLVED!" at
        // `total_width / 2.0 - 30.0` and the timer at `total_width - 80.0`.
        let time = self.format_time();
        let time_w = text::measure(&time, l.font, FontWeightHint::Regular);
        let time_rect = take_right(&mut rest, time_w, l.pad);
        label_left(
            f,
            &Label {
                text: &time,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: SUBTEXT0,
            },
            time_rect,
        );

        let title = self.puzzle_name();
        let title_w = text::measure(title, l.big, FontWeightHint::Bold).min(rest.w);
        let title_rect = take_left(&mut rest, title_w, l.pad);
        label_left(
            f,
            &Label {
                text: title,
                size: l.big,
                weight: FontWeightHint::Bold,
                color: TEXT_COLOR,
            },
            title_rect,
        );

        if self.screen == Screen::Won {
            label_centred(
                f,
                &Label {
                    text: "SOLVED!",
                    size: l.font,
                    weight: FontWeightHint::Bold,
                    color: GREEN,
                },
                rest,
            );
        }
    }

    fn draw_clues(&self, f: &mut Frame<Target>, g: &Grid) {
        // Both bands are drawn against the far edge of their band, so the
        // *last* number of every clue sits in the slot next to the grid — a
        // clue reads towards the picture, which is how the puzzle is meant to
        // be read. `g.row_slots` is the deepest clue in the puzzle, so a
        // shorter one starts further in.
        for (r, clue) in self.row_clues.iter().enumerate() {
            let start = g.row_slots.saturating_sub(clue.len());
            let live = self.screen == Screen::Playing && r == self.cursor_row;
            for (j, &val) in clue.iter().enumerate() {
                self.draw_clue(
                    f,
                    g.row_clue_rect(r, start.saturating_add(j)),
                    val,
                    live,
                    g.clue_font,
                );
            }
        }
        for (c, clue) in self.col_clues.iter().enumerate() {
            let start = g.col_slots.saturating_sub(clue.len());
            let live = self.screen == Screen::Playing && c == self.cursor_col;
            for (j, &val) in clue.iter().enumerate() {
                self.draw_clue(
                    f,
                    g.col_clue_rect(c, start.saturating_add(j)),
                    val,
                    live,
                    g.clue_font,
                );
            }
        }
    }

    /// One clue number, centred in its slot by measurement.
    ///
    /// It was centred by subtracting an eyeballed `CLUE_HALF_WIDTH = 4.0`
    /// from the middle of the row or column, which put every two-digit clue
    /// left of where it belonged.
    fn draw_clue(&self, f: &mut Frame<Target>, slot: Rect, val: u8, live: bool, size: f32) {
        label_centred(
            f,
            &Label {
                text: &val.to_string(),
                size,
                weight: if live {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                color: if live { BLUE } else { SUBTEXT0 },
            },
            slot,
        );
    }

    fn draw_cells(&self, f: &mut Frame<Target>, g: &Grid) {
        let radius = CornerRadii::all(g.cell * 0.11);
        for row in 0..self.grid_side {
            for col in 0..self.grid_side {
                let r = g.cell_rect(row, col);
                let mark = self.cell_at(row, col);
                let wrong = self.check_mode && self.is_error(row, col);
                fill(f, r, self.cell_color(mark, wrong), radius);
                if mark == CellMark::MarkedEmpty {
                    draw_cross(f, r, if wrong { RED } else { OVERLAY0 }, g.cell);
                }
                f.hit(Target::Cell(row, col), g.cell_hit(row, col));
            }
        }

        // A rule down the middle of every fifth gap, to count by. Taken from
        // the cell's own origin, so it always lands between the two cells it
        // separates.
        if self.grid_side >= 10 {
            for group in (5..self.grid_side).step_by(5) {
                let cell = g.cell_rect(group, group);
                let w = (g.gap * 0.75).max(1.0);
                fill(
                    f,
                    Rect::new(cell.x - g.gap / 2.0 - w / 2.0, g.cells.y, w, g.cells.h),
                    OVERLAY0,
                    CornerRadii::ZERO,
                );
                fill(
                    f,
                    Rect::new(g.cells.x, cell.y - g.gap / 2.0 - w / 2.0, g.cells.w, w),
                    OVERLAY0,
                    CornerRadii::ZERO,
                );
            }
        }

        // The cursor is drawn last so it is never painted over by a
        // neighbouring cell, and outside its cell so it does not eat the ink.
        if self.screen == Screen::Playing {
            let r = g.cell_rect(self.cursor_row, self.cursor_col);
            if !r.is_empty() {
                let out = (g.gap / 2.0).max(1.0);
                stroke(
                    f,
                    Rect::new(r.x - out, r.y - out, r.w + out * 2.0, r.h + out * 2.0),
                    YELLOW,
                    out,
                    CornerRadii::all(g.cell * 0.11 + out),
                );
            }
        }
    }

    /// The colour a cell is painted.
    fn cell_color(&self, mark: CellMark, wrong: bool) -> Color {
        match mark {
            CellMark::Filled => {
                if wrong {
                    RED
                } else if self.screen == Screen::Won {
                    BLUE
                } else {
                    LAVENDER
                }
            }
            CellMark::MarkedEmpty if wrong => SURFACE2,
            CellMark::Empty | CellMark::MarkedEmpty => SURFACE0,
        }
    }

    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, MANTLE, CornerRadii::ZERO);
        let mut rest = inset_x(l.footer, l.pad * 2.0);

        // Switches first, right to left, then the progress count, then the
        // hint gets whatever is left — and is told to stop there, which is
        // what `max_width` is for and what every string in this program was
        // missing.
        for (target, text) in [(Target::Menu, "Menu"), (Target::Check, "Check")] {
            let w = text::measure(text, l.small, FontWeightHint::Bold) + l.pad * 2.0;
            let box_rect = take_right(&mut rest, w, l.pad);
            if box_rect.is_empty() {
                continue;
            }
            let on = target == Target::Check && self.check_mode;
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

        let progress = format!(
            "{}/{}",
            self.player_filled_count(),
            self.total_filled_in_solution()
        );
        let progress_w = text::measure(&progress, l.small, FontWeightHint::Regular);
        let progress_rect = take_right(&mut rest, progress_w, l.pad);
        label_left(
            f,
            &Label {
                text: &progress,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: if self.screen == Screen::Won {
                    GREEN
                } else {
                    SUBTEXT0
                },
            },
            progress_rect,
        );

        label_left(
            f,
            &Label {
                text: if self.screen == Screen::Won {
                    "Enter/Esc: back to the list"
                } else {
                    "Arrows: move   Space: fill   X: mark   C: check   Esc: list"
                },
                size: l.small,
                weight: FontWeightHint::Regular,
                color: OVERLAY0,
            },
            rest,
        );
    }

    /// The name of the puzzle being played.
    fn puzzle_name(&self) -> &'static str {
        self.puzzles
            .get(self.selected_puzzle)
            .map_or("Nonogram", |p| p.name)
    }
}

impl Default for NonogramApp {
    fn default() -> Self {
        Self::new()
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

/// The X that says "this cell is definitely blank".
fn draw_cross(f: &mut Frame<Target>, r: Rect, color: Color, cell: f32) {
    if r.is_empty() {
        return;
    }
    let inset = cell * 0.22;
    let width = (cell * 0.08).max(1.0);
    for (x1, x2) in [
        (r.x + inset, r.right() - inset),
        (r.right() - inset, r.x + inset),
    ] {
        f.push(RenderCommand::Line {
            x1,
            y1: r.y + inset,
            x2,
            y2: r.bottom() - inset,
            color,
            width,
        });
    }
}

/// The picture, small, inside `r`.
fn draw_thumbnail(f: &mut Frame<Target>, puzzle: &PuzzleDef, r: Rect, color: Color) {
    let side = puzzle.size.side();
    if r.is_empty() || side == 0 {
        return;
    }
    let step = r.w / usize_f32(side);
    for row in 0..side {
        for col in 0..side {
            if puzzle
                .solution
                .get(row.saturating_mul(side).saturating_add(col))
                != Some(&true)
            {
                continue;
            }
            fill(
                f,
                Rect::new(
                    r.x + usize_f32(col) * step,
                    r.y + usize_f32(row) * step,
                    (step * 0.85).max(0.5),
                    (step * 0.85).max(0.5),
                ),
                color,
                CornerRadii::ZERO,
            );
        }
    }
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
/// Returns `Rect::EMPTY` and takes nothing if there is not room, so a row that
/// runs out of space drops its right-hand items rather than drawing them on
/// top of its left-hand ones.
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

/// The size of a puzzle, as it is written in the list.
///
/// `GridSize::label` used to spell the number out a second time — `Small` was
/// `5` in `side()` and `"5x5"` here — so a grid size could be changed in one
/// place and go on being described by the other.
fn size_label(size: GridSize) -> String {
    let side = size.side();
    format!("{side}x{side}")
}

// ── Window plumbing ─────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(app: &mut NonogramApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) if ev.pressed => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Tick { elapsed_ms } => app.handle_tick(*elapsed_ms),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for NonogramApp {
    fn title(&self) -> String {
        "Nonogram".to_string()
    }

    fn app_id(&self) -> String {
        "nonogram".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

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

impl Probe for NonogramApp {
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
    let mut game = NonogramApp::new();
    app::launch("nonogram", &mut game)
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

    const SIZE: (f32, f32) = <NonogramApp as Probe>::SIZE;

    /// Window shapes every geometric claim is made at.
    ///
    /// `140x900` is there because it is tall enough for every band and narrow
    /// enough that the footer has less room than its own text wants -- the case
    /// where a string measured without regard to its box lands off the left
    /// edge of the window. `60x60` is smaller than the header and footer put
    /// together, which is what makes them give up their height.
    const SHAPES: [(f32, f32); 7] = [
        SIZE,
        (320.0, 900.0),
        (1400.0, 400.0),
        (200.0, 200.0),
        (2000.0, 1400.0),
        (60.0, 60.0),
        (140.0, 900.0),
    ];

    // ── Helpers ────────────────────────────────────────────────────

    /// The index of the puzzle with this name.
    fn index_of(app: &NonogramApp, name: &str) -> usize {
        app.puzzles
            .iter()
            .position(|p| p.name == name)
            .unwrap_or_else(|| panic!("no puzzle called {name}"))
    }

    /// A game already playing the named puzzle.
    fn playing(name: &str) -> NonogramApp {
        let mut app = NonogramApp::new();
        let i = index_of(&app, name);
        app.start_puzzle(i);
        assert_eq!(app.screen, Screen::Playing);
        app
    }

    /// Fill every cell the picture wants, without going through the game -- so
    /// a test can set up a solved grid and then ask a question about it.
    fn paint_solution(app: &mut NonogramApp) {
        for row in 0..app.grid_side {
            for col in 0..app.grid_side {
                if app.solution[row * app.grid_side + col] {
                    app.set_cell(row, col, CellMark::Filled);
                }
            }
        }
    }

    /// The first cell the picture wants filled.
    fn a_filled_cell(app: &NonogramApp) -> (usize, usize) {
        for row in 0..app.grid_side {
            for col in 0..app.grid_side {
                if app.solution[row * app.grid_side + col] {
                    return (row, col);
                }
            }
        }
        panic!("the picture is blank");
    }

    /// The first cell the picture wants left blank.
    fn a_blank_cell(app: &NonogramApp) -> (usize, usize) {
        for row in 0..app.grid_side {
            for col in 0..app.grid_side {
                if !app.solution[row * app.grid_side + col] {
                    return (row, col);
                }
            }
        }
        panic!("the picture is solid");
    }

    fn all_targets(app: &NonogramApp, size: (f32, f32)) -> Vec<Target> {
        app.frame(size.0, size.1)
            .hits()
            .iter()
            .map(|(t, _)| *t)
            .collect()
    }

    // ── Clues, as the puzzle states them ───────────────────────────

    #[test]
    fn a_run_of_filled_cells_is_one_number() {
        assert_eq!(compute_clue(&[true, true, true]), vec![3]);
        assert_eq!(compute_clue(&[false, true, true, false]), vec![2]);
    }

    #[test]
    fn separate_runs_are_listed_in_the_order_they_appear() {
        assert_eq!(
            compute_clue(&[true, false, true, true, false, true]),
            vec![1, 2, 1]
        );
    }

    #[test]
    fn an_empty_line_is_stated_as_a_single_zero() {
        // Not an empty list: a column with nothing in it still needs a number
        // over it, or the player cannot tell it from a column they have not
        // been told about.
        assert_eq!(compute_clue(&[false; 5]), vec![0]);
        assert_eq!(compute_clue(&[]), vec![0]);
    }

    #[test]
    fn a_run_reaching_the_end_of_the_line_is_still_counted() {
        // The run is closed by the end of the line rather than by a blank, so
        // a version that only pushed on the falling edge would drop it.
        assert_eq!(compute_clue(&[false, true, true]), vec![2]);
        assert_eq!(compute_clue(&[true]), vec![1]);
    }

    #[test]
    fn row_clues_read_across_and_column_clues_read_down() {
        // Two rows of two, filled diagonally:  #.
        //                                      .#
        let grid = [true, false, false, true];
        assert_eq!(compute_row_clues(&grid, 2), vec![vec![1], vec![1]]);
        assert_eq!(compute_col_clues(&grid, 2), vec![vec![1], vec![1]]);

        // Now a shape that reads differently across than down:  ##
        //                                                       #.
        let grid = [true, true, true, false];
        assert_eq!(compute_row_clues(&grid, 2), vec![vec![2], vec![1]]);
        assert_eq!(compute_col_clues(&grid, 2), vec![vec![2], vec![1]]);

        // ... and one that is not its own transpose:  #.
        //                                             ##
        let grid = [true, false, true, true];
        assert_eq!(compute_row_clues(&grid, 2), vec![vec![1], vec![2]]);
        assert_eq!(compute_col_clues(&grid, 2), vec![vec![2], vec![1]]);
    }

    #[test]
    fn a_clue_line_with_no_width_is_no_clue_at_all() {
        assert!(compute_row_clues(&[true, false], 0).is_empty());
        assert!(compute_col_clues(&[true, false], 0).is_empty());
    }

    #[test]
    fn a_picture_ignores_anything_past_its_own_edges() {
        // Both the extra column and the extra row are dropped, rather than
        // wrapping into the next row or running off the end of the vector.
        // The picture is deliberately not its own transpose and not symmetric,
        // so a character that wrapped onto the next row would show up as a
        // filled cell the picture did not ask for rather than land on one it
        // did.
        let grid = parse_picture("#.#\n..#\n###", 2);
        assert_eq!(grid, vec![true, false, false, false]);
    }

    #[test]
    fn every_puzzle_in_the_catalogue_is_the_size_it_claims() {
        for puzzle in &builtin_puzzles() {
            let side = puzzle.size.side();
            assert_eq!(
                puzzle.solution.len(),
                side * side,
                "{} says {}x{} and has {} cells",
                puzzle.name,
                side,
                side,
                puzzle.solution.len()
            );
            assert!(
                puzzle.solution.iter().any(|&v| v),
                "{} is a blank picture",
                puzzle.name
            );
        }
    }

    #[test]
    fn a_puzzles_size_is_written_once() {
        // The label used to be a second spelling of the side length, so the two
        // could disagree. Every label in the catalogue is now derived from the
        // number the grid is actually built with.
        for puzzle in &builtin_puzzles() {
            let side = puzzle.size.side();
            assert_eq!(size_label(puzzle.size), format!("{side}x{side}"));
        }
    }

    // ── The rules of the game ──────────────────────────────────────

    #[test]
    fn filling_a_cell_twice_puts_it_back() {
        let mut app = playing("Heart");
        app.toggle_fill(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::Filled);
        app.toggle_fill(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::Empty);
    }

    #[test]
    fn a_mark_replaces_a_fill_and_a_fill_replaces_a_mark() {
        let mut app = playing("Heart");
        app.toggle_fill(1, 1);
        app.toggle_mark_empty(1, 1);
        assert_eq!(
            app.cell_at(1, 1),
            CellMark::MarkedEmpty,
            "marking a filled cell left it filled"
        );
        app.toggle_fill(1, 1);
        assert_eq!(
            app.cell_at(1, 1),
            CellMark::Filled,
            "filling a marked cell left it marked"
        );
        app.toggle_mark_empty(1, 1);
        app.toggle_mark_empty(1, 1);
        assert_eq!(
            app.cell_at(1, 1),
            CellMark::Empty,
            "marking twice did not clear the mark"
        );
    }

    #[test]
    fn a_cell_off_the_grid_is_neither_read_nor_written() {
        let mut app = playing("Heart");
        let side = app.grid_side;
        app.toggle_fill(side, 0);
        app.toggle_fill(0, side);
        assert_eq!(app.cell_at(side, 0), CellMark::Empty);
        assert_eq!(app.cell_at(0, side), CellMark::Empty);
        assert!(
            app.cells.iter().all(|&c| c == CellMark::Empty),
            "a click off the grid changed a cell on it"
        );
    }

    #[test]
    fn the_puzzle_is_won_when_the_filled_cells_are_exactly_the_picture() {
        let mut app = playing("Heart");
        assert!(!app.check_win(), "a blank grid was taken for a solved one");
        paint_solution(&mut app);
        assert!(app.check_win());
    }

    #[test]
    fn a_missing_cell_leaves_the_puzzle_unsolved() {
        let mut app = playing("Heart");
        paint_solution(&mut app);
        let (r, c) = a_filled_cell(&app);
        app.set_cell(r, c, CellMark::Empty);
        assert!(!app.check_win(), "a hole in the picture still counted");
    }

    #[test]
    fn an_extra_cell_leaves_the_puzzle_unsolved() {
        let mut app = playing("Heart");
        paint_solution(&mut app);
        let (r, c) = a_blank_cell(&app);
        app.set_cell(r, c, CellMark::Filled);
        assert!(!app.check_win(), "a spare filled cell still counted");
    }

    #[test]
    fn a_mark_is_not_a_fill_as_far_as_winning_goes() {
        // Marking every blank cell is how the game is played, and doing it must
        // not by itself be mistaken for filling them.
        let mut app = playing("Heart");
        paint_solution(&mut app);
        let (r, c) = a_blank_cell(&app);
        app.set_cell(r, c, CellMark::MarkedEmpty);
        assert!(app.check_win(), "marking a blank cell unsolved the puzzle");
    }

    #[test]
    fn check_mode_calls_out_a_cell_filled_that_should_be_blank() {
        let mut app = playing("Heart");
        let (r, c) = a_blank_cell(&app);
        app.set_cell(r, c, CellMark::Filled);
        assert!(app.is_error(r, c));
    }

    #[test]
    fn check_mode_calls_out_a_cell_marked_blank_that_should_be_filled() {
        let mut app = playing("Heart");
        let (r, c) = a_filled_cell(&app);
        app.set_cell(r, c, CellMark::MarkedEmpty);
        assert!(app.is_error(r, c));
    }

    #[test]
    fn check_mode_says_nothing_about_a_cell_you_have_not_touched() {
        // Otherwise the check would report every unsolved cell of the picture
        // as a mistake the moment you asked, which is not the question.
        let app = playing("Heart");
        let (r, c) = a_filled_cell(&app);
        assert!(
            !app.is_error(r, c),
            "an untouched cell was called a mistake"
        );
        let (r, c) = a_blank_cell(&app);
        assert!(!app.is_error(r, c));
    }

    #[test]
    fn a_right_answer_is_never_a_mistake() {
        let mut app = playing("Heart");
        paint_solution(&mut app);
        for row in 0..app.grid_side {
            for col in 0..app.grid_side {
                assert!(
                    !app.is_error(row, col),
                    "the solved picture reports ({row}, {col}) as wrong"
                );
            }
        }
    }

    #[test]
    fn a_cell_off_the_grid_is_not_a_mistake() {
        let app = playing("Heart");
        assert!(!app.is_error(app.grid_side, 0));
        assert!(!app.is_error(0, app.grid_side));
    }

    #[test]
    fn starting_a_puzzle_clears_the_one_before_it() {
        let mut app = playing("Heart");
        app.toggle_fill(0, 0);
        app.check_mode = true;
        app.cursor_row = 3;
        app.elapsed_ms = 90_000;

        let cat = index_of(&app, "Cat");
        app.start_puzzle(cat);
        assert_eq!(app.grid_side, 15, "the new puzzle kept the old size");
        assert_eq!(app.cells.len(), 15 * 15);
        assert!(app.cells.iter().all(|&c| c == CellMark::Empty));
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);
        assert_eq!(
            app.elapsed_ms, 0,
            "the clock carried over from the last game"
        );
        assert!(
            !app.check_mode,
            "check mode carried over from the last game"
        );
        assert_eq!(app.row_clues.len(), 15);
        assert_eq!(app.col_clues.len(), 15);
    }

    #[test]
    fn a_puzzle_that_is_not_in_the_catalogue_starts_nothing() {
        let mut app = NonogramApp::new();
        app.start_puzzle(app.puzzles.len());
        assert_eq!(
            app.screen,
            Screen::Select,
            "an unknown puzzle started anyway"
        );
    }

    // ── The keyboard ───────────────────────────────────────────────

    #[test]
    fn the_arrows_move_the_cursor_and_stop_at_the_edges() {
        // Each arrow is first pressed from a cell it can actually move off, so
        // that the direction it moves in is asserted and not merely the clamp
        // at the far end of it (`known-issues.md` lesson 70).
        for (key, from, to) in [
            (Key::Up, (2, 2), (1, 2)),
            (Key::Down, (2, 2), (3, 2)),
            (Key::Left, (2, 2), (2, 1)),
            (Key::Right, (2, 2), (2, 3)),
        ] {
            let mut app = playing("Heart");
            app.cursor_row = from.0;
            app.cursor_col = from.1;
            assert_eq!(
                probe::key(&mut app, &probe::press(key)),
                EventResult::Consumed,
                "{key:?} from {from:?} was left for someone else"
            );
            assert_eq!(
                (app.cursor_row, app.cursor_col),
                to,
                "{key:?} from {from:?} did not go to {to:?}"
            );
        }

        let mut app = playing("Heart");
        probe::key(&mut app, &probe::press(Key::Up));
        probe::key(&mut app, &probe::press(Key::Left));
        assert_eq!(
            (app.cursor_row, app.cursor_col),
            (0, 0),
            "the cursor walked off the top-left corner"
        );

        probe::key(&mut app, &probe::press(Key::Down));
        probe::key(&mut app, &probe::press(Key::Right));
        assert_eq!((app.cursor_row, app.cursor_col), (1, 1));

        for _ in 0..20 {
            probe::key(&mut app, &probe::press(Key::Down));
            probe::key(&mut app, &probe::press(Key::Right));
        }
        assert_eq!(
            (app.cursor_row, app.cursor_col),
            (app.grid_side - 1, app.grid_side - 1),
            "the cursor walked off the bottom-right corner"
        );
    }

    #[test]
    fn an_arrow_at_the_edge_is_left_for_whoever_wants_it() {
        // A key that changed nothing must not ask the window to redraw.
        //
        // Every arrow is pressed at the edge it stops at, and the *verdict* is
        // what carries the rule: at the edge the cursor sits still whether the
        // key was refused outright or applied and then clamped by
        // `saturating_sub`, so its position cannot tell a missing guard from a
        // saturating subtraction (`known-issues.md` lesson 70).
        let last = playing("Heart").grid_side - 1;
        for (key, at) in [
            (Key::Up, (0, 2)),
            (Key::Left, (2, 0)),
            (Key::Down, (last, 2)),
            (Key::Right, (2, last)),
        ] {
            let mut app = playing("Heart");
            app.cursor_row = at.0;
            app.cursor_col = at.1;
            assert_eq!(
                probe::key(&mut app, &probe::press(key)),
                EventResult::Ignored,
                "{key:?} at {at:?} was answered rather than left for someone else"
            );
            assert_eq!(
                (app.cursor_row, app.cursor_col),
                at,
                "{key:?} moved the cursor off {at:?}"
            );
        }

        let mut app = playing("Heart");
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Down)),
            EventResult::Consumed
        );
    }

    #[test]
    fn space_and_enter_both_fill_the_cell_under_the_cursor() {
        for key in [Key::Space, Key::Enter] {
            let mut app = playing("Heart");
            app.cursor_row = 2;
            app.cursor_col = 3;
            probe::key(&mut app, &probe::press(key));
            assert_eq!(
                app.cell_at(2, 3),
                CellMark::Filled,
                "{key:?} did not fill the cell"
            );
            assert_eq!(
                app.cell_at(0, 0),
                CellMark::Empty,
                "{key:?} filled the wrong cell"
            );
        }
    }

    #[test]
    fn x_marks_the_cell_under_the_cursor() {
        let mut app = playing("Heart");
        app.cursor_row = 4;
        app.cursor_col = 2;
        probe::key(&mut app, &probe::press(Key::X));
        assert_eq!(app.cell_at(4, 2), CellMark::MarkedEmpty);
    }

    #[test]
    fn c_turns_the_check_on_and_off_again() {
        let mut app = playing("Heart");
        assert!(!app.check_mode);
        probe::key(&mut app, &probe::press(Key::C));
        assert!(app.check_mode);
        probe::key(&mut app, &probe::press(Key::C));
        assert!(!app.check_mode, "the check would not switch off again");
    }

    #[test]
    fn escape_goes_back_to_the_list() {
        let mut app = playing("Heart");
        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(app.screen, Screen::Select);
    }

    #[test]
    fn filling_the_last_cell_of_the_picture_wins_from_the_keyboard() {
        let mut app = playing("Heart");
        paint_solution(&mut app);
        let (r, c) = a_filled_cell(&app);
        app.set_cell(r, c, CellMark::Empty);
        app.cursor_row = r;
        app.cursor_col = c;
        assert_eq!(app.screen, Screen::Playing);
        probe::key(&mut app, &probe::press(Key::Space));
        assert_eq!(app.screen, Screen::Won, "the last cell did not finish it");
    }

    #[test]
    fn a_key_going_back_up_does_nothing() {
        // The event stream carries a press and a release for every keystroke.
        // A handler that does not look at `pressed` acts on both, which moves
        // the cursor two cells for one tap.
        let mut app = playing("Heart");
        let mut release = probe::press(Key::Down);
        release.pressed = false;
        assert_eq!(probe::key(&mut app, &release), EventResult::Ignored);
        assert_eq!(app.cursor_row, 0, "the release moved the cursor as well");
    }

    #[test]
    fn a_modified_key_belongs_to_whoever_is_listening_for_shortcuts() {
        let mut app = playing("Heart");
        assert_eq!(
            probe::key(&mut app, &probe::ctrl(Key::X)),
            EventResult::Ignored
        );
        assert_eq!(
            app.cell_at(0, 0),
            CellMark::Empty,
            "Ctrl-X was taken for the X mark"
        );
        assert_eq!(
            probe::key(&mut app, &probe::shift(Key::Down)),
            EventResult::Ignored
        );
        assert_eq!(app.cursor_row, 0);
    }

    #[test]
    fn a_key_the_game_has_no_use_for_is_left_alone() {
        let mut app = playing("Heart");
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Q)),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_list_cursor_stops_at_both_ends() {
        // The verdict is asserted alongside the position: at either end the
        // cursor stays put whether the arrow was refused or applied and
        // clamped, and only the verdict says which happened -- one asks the
        // window to repaint and the other does not (`known-issues.md` lesson
        // 70).
        let mut app = NonogramApp::new();
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Up)),
            EventResult::Ignored,
            "the top of the list answered an arrow it had no room for"
        );
        assert_eq!(app.select_cursor, 0, "the list cursor walked off the top");

        // One step each way from a row that has room in both directions, so
        // the arrows' directions are asserted and not just their clamps.
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Down)),
            EventResult::Consumed
        );
        assert_eq!(app.select_cursor, 1, "Down did not go down the list");
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Up)),
            EventResult::Consumed
        );
        assert_eq!(app.select_cursor, 0, "Up did not go up the list");

        for _ in 0..app.puzzles.len() + 5 {
            probe::key(&mut app, &probe::press(Key::Down));
        }
        assert_eq!(
            app.select_cursor,
            app.puzzles.len() - 1,
            "the list cursor walked off the bottom"
        );
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Down)),
            EventResult::Ignored,
            "the bottom of the list answered an arrow it had no room for"
        );
    }

    #[test]
    fn enter_on_the_list_starts_the_puzzle_under_the_cursor() {
        let mut app = NonogramApp::new();
        probe::key(&mut app, &probe::press(Key::Down));
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.selected_puzzle, 1, "it started the wrong puzzle");
    }

    #[test]
    fn any_of_three_keys_leaves_the_victory_screen() {
        for key in [Key::Enter, Key::Space, Key::Escape] {
            let mut app = playing("Heart");
            app.screen = Screen::Won;
            probe::key(&mut app, &probe::press(key));
            assert_eq!(app.screen, Screen::Select, "{key:?} did not leave");
        }
    }

    #[test]
    fn the_victory_screen_ignores_the_keys_that_play_the_game() {
        let mut app = playing("Heart");
        app.screen = Screen::Won;
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Down)),
            EventResult::Ignored
        );
        assert_eq!(
            app.cursor_row, 0,
            "the cursor moved after the game was over"
        );
    }

    // ── The clock ──────────────────────────────────────────────────

    #[test]
    fn the_clock_runs_only_while_a_puzzle_is_being_played() {
        let mut app = NonogramApp::new();
        handle_event(&mut app, &Event::Tick { elapsed_ms: 5_000 });
        assert_eq!(app.elapsed_ms, 0, "the clock ran on the puzzle list");

        let mut app = playing("Heart");
        handle_event(&mut app, &Event::Tick { elapsed_ms: 5_000 });
        assert_eq!(app.elapsed_ms, 5_000);
        // A second tick, of a different length. A clock that stored the tick
        // rather than adding it would read 3_000 here, having sailed through
        // the line above: one sample cannot tell a sum from an assignment
        // (`known-issues.md` lesson 70).
        handle_event(&mut app, &Event::Tick { elapsed_ms: 3_000 });
        assert_eq!(
            app.elapsed_ms, 8_000,
            "the clock was set to the tick rather than advanced by it"
        );

        app.screen = Screen::Won;
        handle_event(&mut app, &Event::Tick { elapsed_ms: 5_000 });
        assert_eq!(
            app.elapsed_ms, 8_000,
            "the clock ran after the game was won"
        );
    }

    #[test]
    fn the_clock_reads_minutes_and_seconds() {
        let mut app = playing("Heart");
        assert_eq!(app.format_time(), "0:00");
        app.elapsed_ms = 9_000;
        assert_eq!(app.format_time(), "0:09");
        app.elapsed_ms = 61_500;
        assert_eq!(app.format_time(), "1:01");
        app.elapsed_ms = 3_600_000;
        assert_eq!(app.format_time(), "60:00");
    }

    #[test]
    fn the_window_is_asked_for_the_ticks_the_clock_runs_on() {
        // The clock counted ticks it was never sent: nothing in the program
        // asked for them, so the header read 0:00 for the whole game.
        let app = NonogramApp::new();
        let every = App::tick_interval(&app).expect("the clock asks for no ticks");
        assert!(
            every <= Duration::from_secs(1),
            "a clock that shows seconds is asking for ticks every {every:?}"
        );
    }

    // ── The pointer ────────────────────────────────────────────────

    #[test]
    fn every_cell_is_clickable_where_its_ink_is() {
        // The old program hit-tested the grid against a second copy of the
        // geometry. Here the boxes come from the drawing pass, so this asks
        // whether the box the renderer recorded actually covers the ink it
        // drew — for every cell, at every window shape.
        for size in SHAPES {
            let app = playing("Heart");
            let g = app.grid(&Layout::new(size.0, size.1));
            if g.cell <= 0.0 {
                continue;
            }
            for row in 0..app.grid_side {
                for col in 0..app.grid_side {
                    let ink = g.cell_rect(row, col);
                    let hit = probe::rect_of_sized(&app, Target::Cell(row, col), size)
                        .unwrap_or_else(|| panic!("cell {row},{col} has no box at {size:?}"));
                    assert!(
                        hit.x <= ink.x
                            && hit.y <= ink.y
                            && hit.right() >= ink.right()
                            && hit.bottom() >= ink.bottom(),
                        "at {size:?} cell {row},{col} draws {ink:?} but is clickable at {hit:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_click_between_two_cells_lands_in_the_nearer_one() {
        // The gap belongs to somebody. Boxes are grown by half a gap on every
        // side and abut exactly, so a click a hair to the left of the seam
        // goes to the left cell and a hair to the right goes to the right one
        // — and neither falls through to nothing.
        let mut app = playing("Heart");
        let g = app.grid(&Layout::new(SIZE.0, SIZE.1));
        let left = g.cell_rect(2, 2);
        let seam = left.right() + g.gap / 2.0;
        assert!(
            g.gap > 0.5,
            "the gap is too small for this test to mean anything"
        );

        assert_eq!(
            app.frame(SIZE.0, SIZE.1)
                .hit_test(seam - 0.25, left.centre().1),
            Some(Target::Cell(2, 2))
        );
        assert_eq!(
            app.frame(SIZE.0, SIZE.1)
                .hit_test(seam + 0.25, left.centre().1),
            Some(Target::Cell(2, 3))
        );
        // And the click really does something, so this is a statement about
        // the running program and not about a box nobody reads.
        assert_eq!(app.cell_at(2, 3), CellMark::Empty);
        app.click_at(seam + 0.25, left.centre().1, MouseButton::Left, SIZE);
        assert_eq!(app.cell_at(2, 3), CellMark::Filled);
    }

    #[test]
    fn a_click_outside_the_grid_does_not_reach_a_cell() {
        let app = playing("Heart");
        let g = app.grid(&Layout::new(SIZE.0, SIZE.1));
        let far = g.cell_hit(0, 0);
        let f = app.frame(SIZE.0, SIZE.1);
        assert_eq!(f.hit_test(far.x - 1.0, far.centre().1), None);
        assert_eq!(f.hit_test(far.centre().0, far.y - 1.0), None);
        let last = g.cell_hit(app.grid_side - 1, app.grid_side - 1);
        assert_eq!(f.hit_test(last.right() + 1.0, last.centre().1), None);
    }

    #[test]
    fn right_clicking_marks_and_left_clicking_fills() {
        // The pointer could only ever fill. X was keyboard-only, so a player
        // using the mouse had no way to record "this one is blank" at all.
        let mut app = playing("Heart");
        probe::click_with(&mut app, Target::Cell(1, 1), MouseButton::Right);
        assert_eq!(app.cell_at(1, 1), CellMark::MarkedEmpty);
        probe::click(&mut app, Target::Cell(1, 1));
        assert_eq!(app.cell_at(1, 1), CellMark::Filled);
        probe::click_with(&mut app, Target::Cell(1, 1), MouseButton::Right);
        assert_eq!(app.cell_at(1, 1), CellMark::MarkedEmpty);
    }

    #[test]
    fn clicking_a_cell_moves_the_cursor_to_it() {
        // Otherwise the keyboard carries on from wherever it was, and the two
        // ways of playing disagree about where the player is.
        //
        // The click is aimed at a point taken from the geometry rather than at
        // a `Target::Cell` looked up in the frame. A hit box filed under its
        // own transpose would send `probe::click` to the transposed box and
        // then read the transposed target back out of it, agreeing with itself
        // the whole way round (`known-issues.md` lesson 65).
        let mut app = playing("Heart");
        assert_eq!((app.cursor_row, app.cursor_col), (0, 0));
        let g = app.grid(&Layout::new(SIZE.0, SIZE.1));
        let cell = g.cell_rect(3, 4);
        assert!(!cell.is_empty(), "the fixture puzzle has no cell 3,4");
        let (x, y) = cell.centre();
        app.click_at(x, y, MouseButton::Left, SIZE);
        assert_eq!((app.cursor_row, app.cursor_col), (3, 4));
    }

    #[test]
    fn every_entry_in_the_list_is_clickable_across_its_whole_width() {
        // The fault this replaces: the list was drawn from PADDING to
        // width-PADDING and hit-tested from PADDING to 500.0, so the last four
        // pixels of every entry were drawn as part of it and did nothing.
        for size in SHAPES {
            let app = NonogramApp::new();
            let l = Layout::new(size.0, size.1);
            let list = app.list(&l);
            for i in 0..app.puzzles.len() {
                let drawn = list.entry(i);
                if drawn.is_empty() {
                    continue;
                }
                let hit = probe::rect_of_sized(&app, Target::Puzzle(i), size)
                    .unwrap_or_else(|| panic!("entry {i} has no box at {size:?}"));
                assert_eq!(
                    hit, drawn,
                    "at {size:?} entry {i} is drawn at {drawn:?} and clickable at {hit:?}"
                );
            }
        }
    }

    #[test]
    fn clicking_an_entry_starts_that_puzzle() {
        let mut app = NonogramApp::new();
        let i = index_of(&app, "Cat");
        probe::click(&mut app, Target::Puzzle(i));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.selected_puzzle, i);
        // The list cursor follows, so Escape-then-Enter replays the same one.
        assert_eq!(app.select_cursor, i);
    }

    #[test]
    fn the_check_switch_can_be_reached_by_the_pointer() {
        let mut app = playing("Heart");
        assert!(!app.check_mode);
        probe::click(&mut app, Target::Check);
        assert!(app.check_mode);
        probe::click(&mut app, Target::Check);
        assert!(!app.check_mode);
    }

    #[test]
    fn the_menu_switch_returns_to_the_list() {
        let mut app = playing("Heart");
        probe::click(&mut app, Target::Menu);
        assert_eq!(app.screen, Screen::Select);
    }

    #[test]
    fn a_click_on_the_board_returns_from_the_victory_screen() {
        let mut app = playing("Heart");
        paint_solution(&mut app);
        app.screen = Screen::Won;
        let outcome = probe::click_background(&mut app);
        assert_eq!(outcome, EventResult::Consumed);
        assert_eq!(app.screen, Screen::Select);
    }

    #[test]
    fn the_pointer_cannot_change_the_picture_after_it_is_solved() {
        let mut app = playing("Heart");
        paint_solution(&mut app);
        app.screen = Screen::Won;
        let (row, col) = a_filled_cell(&app);
        let outcome = probe::click(&mut app, Target::Cell(row, col));
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.cell_at(row, col), CellMark::Filled);
        assert_eq!(
            app.screen,
            Screen::Won,
            "the win was undone by a stray click"
        );
    }

    #[test]
    fn a_click_that_is_not_a_press_is_not_a_click() {
        // Movement and release both arrive on the same channel. Acting on all
        // three would fill a cell three times per click, which is twice back
        // to blank and once too many.
        let mut app = playing("Heart");
        let g = app.grid(&Layout::new(SIZE.0, SIZE.1));
        let (x, y) = g.cell_rect(1, 1).centre();
        for kind in [
            MouseEventKind::Move,
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        ] {
            let ev = MouseEvent {
                x,
                y,
                kind: kind.clone(),
            };
            assert_eq!(
                handle_event(&mut app, &Event::Mouse(ev)),
                EventResult::Ignored,
                "{kind:?} was taken for a click"
            );
            assert_eq!(app.cell_at(1, 1), CellMark::Empty);
        }
    }

    // ── The layout, at every window ────────────────────────────────

    #[test]
    fn the_bands_tile_the_window_from_top_to_bottom() {
        // Not a containment assertion: "the body is inside the window" is true
        // of half the wrong answers too (known-issues.md lesson 68). The
        // formula is asserted instead.
        for (w, h) in SHAPES {
            let l = Layout::new(w, h);
            assert_eq!(l.window, Rect::new(0.0, 0.0, w.max(1.0), h.max(1.0)));
            assert_eq!(l.header.y, 0.0, "at {w}x{h}");
            assert_eq!(l.header.w, l.window.w, "at {w}x{h}");
            assert_eq!(l.footer.w, l.window.w, "at {w}x{h}");
            assert_eq!(l.footer.bottom(), l.window.bottom(), "at {w}x{h}");
            assert_eq!(l.body.x, l.pad, "at {w}x{h}");
            assert_eq!(l.body.y, l.header.bottom() + l.pad, "at {w}x{h}");
            assert!(
                l.body.bottom() <= l.footer.y + 0.001,
                "at {w}x{h} the body runs to {} and the footer starts at {}",
                l.body.bottom(),
                l.footer.y
            );
            assert!(l.header.bottom() <= l.footer.y, "at {w}x{h}");
        }
    }

    #[test]
    fn a_window_big_enough_to_play_in_has_a_body_to_play_in() {
        // The body only degenerates below about seven pixels tall, where two
        // pads already exceed the window. Above that it must be real, or the
        // program is drawing a header and a footer and no game.
        for (w, h) in SHAPES {
            if h < 10.0 {
                continue;
            }
            let l = Layout::new(w, h);
            assert!(!l.body.is_empty(), "no body at {w}x{h}: {:?}", l.body);
        }
    }

    #[test]
    fn the_footer_gives_up_its_height_before_the_body_does() {
        // A window too short for header + footer + a playable body has to take
        // the space from somewhere. Taking it from the body first would leave
        // a game with two full-size chrome bars and nothing between them.
        let tall = Layout::new(400.0, 800.0);
        let squat = Layout::new(400.0, 90.0);
        assert!(
            squat.footer.h < tall.footer.h,
            "the footer kept its {} pixels in a 90-pixel window",
            squat.footer.h
        );
        assert!(
            squat.body.h >= 90.0 * BODY_SHARE - squat.pad * 2.0 - 0.001,
            "the body was squeezed to {} instead",
            squat.body.h
        );
    }

    #[test]
    fn a_band_that_runs_out_of_room_is_a_strip_of_no_height_where_it_was() {
        // Rect::is_empty is w <= 0 || h <= 0, so a dropped band already answers
        // "no" to the only question the drawing code asks. It still has to sit
        // at the window edge, or the footer is drawn floating in the middle.
        let l = Layout::new(60.0, 60.0);
        assert!(l.footer.h < 18.0, "the footer kept its minimum at 60x60");
        assert_eq!(l.footer.bottom(), 60.0);
        assert_eq!(l.header.y, 0.0);
        assert_eq!(l.footer.w, 60.0, "a dropped band is still full width");
    }

    #[test]
    fn the_grid_is_square_at_every_window_shape() {
        // One cell number decides the whole picture. A wide window and a tall
        // one both get square cells; the old program computed a width from the
        // grid and let the height fall where it may.
        for size in SHAPES {
            let app = playing("Heart");
            let g = app.grid(&Layout::new(size.0, size.1));
            assert!(
                (g.cells.w - g.cells.h).abs() < 0.001,
                "at {size:?} the cells span {:?}",
                g.cells
            );
            for row in 0..app.grid_side {
                for col in 0..app.grid_side {
                    let r = g.cell_rect(row, col);
                    assert!(!r.is_empty(), "at {size:?} cell {row},{col} is nothing");
                    assert!(
                        (r.w - r.h).abs() < 0.001,
                        "at {size:?} cell {row},{col} is {r:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_cells_are_evenly_spaced_and_never_overlap() {
        for size in SHAPES {
            let app = playing("Heart");
            let g = app.grid(&Layout::new(size.0, size.1));
            if g.cell <= 0.0 {
                continue;
            }
            for row in 0..app.grid_side {
                for col in 1..app.grid_side {
                    let a = g.cell_rect(row, col - 1);
                    let b = g.cell_rect(row, col);
                    assert!(
                        b.x >= a.right() - 0.001,
                        "at {size:?} cells {row},{col} overlap: {a:?} {b:?}"
                    );
                    // Spelled out as `cell + gap` rather than as `g.step()`.
                    // `cell_rect` is laid out *by* `step`, so an assertion
                    // phrased in terms of `step` agrees with any value it
                    // takes, including one that had dropped the gap
                    // (`known-issues.md` lesson 65).
                    assert!(g.gap > 0.0, "at {size:?} there is no gap to count");
                    assert!(
                        (b.x - a.right() - g.gap).abs() < 0.001,
                        "at {size:?} the space between {a:?} and {b:?} is not the gap {}",
                        g.gap
                    );
                    assert!(
                        (b.x - a.x - (g.cell + g.gap)).abs() < 0.001,
                        "at {size:?} the step between {a:?} and {b:?} is not {} + {}",
                        g.cell,
                        g.gap
                    );
                }
            }
        }
    }

    #[test]
    fn the_whole_picture_fits_the_space_it_was_given() {
        // Bands included. The clue band is part of the picture's size, not an
        // offset added afterwards, so a puzzle with deep clues gets smaller
        // cells rather than a picture hanging off the edge of the window.
        for size in SHAPES {
            for name in ["Heart", "Cat", "House"] {
                let app = playing(name);
                let l = Layout::new(size.0, size.1);
                let g = app.grid(&l);
                let whole = Rect::new(
                    g.row_clues.x,
                    g.col_clues.y,
                    g.row_clues.w + g.cells.w,
                    g.col_clues.h + g.cells.h,
                );
                assert!(
                    whole.x >= l.body.x - 0.001
                        && whole.y >= l.body.y - 0.001
                        && whole.right() <= l.body.right() + 0.001
                        && whole.bottom() <= l.body.bottom() + 0.001,
                    "{name} at {size:?} draws {whole:?} in a body of {:?}",
                    l.body
                );
            }
        }
    }

    #[test]
    fn a_deeper_clue_band_takes_its_room_from_the_cells() {
        // The reason the grid is solved rather than declared. Same window, same
        // grid side, deeper clues: the cells must come out smaller.
        //
        // The two bands are deepened one at a time. Deepening both at once
        // lets either half of the fit carry the assertion by itself, so a
        // `per_w` that had stopped counting `row_slots` would still pass on
        // the strength of `per_h`.
        let area = Rect::new(0.0, 0.0, 500.0, 500.0);
        let plain = Grid::new(area, 10, 2, 2);
        let wide = Grid::new(area, 10, 5, 2);
        let tall = Grid::new(area, 10, 2, 5);
        assert!(
            wide.cell < plain.cell,
            "five slots of row clue gave the same {} pixel cell as two",
            wide.cell
        );
        assert!(
            tall.cell < plain.cell,
            "five slots of column clue gave the same {} pixel cell as two",
            tall.cell
        );
        assert!(wide.row_clues.w > plain.row_clues.w);
        assert!(tall.col_clues.h > plain.col_clues.h);
    }

    #[test]
    fn the_picture_sits_in_the_middle_of_the_space_it_was_given() {
        // `Grid::new` sizes the cells by whichever of the two fits worse, so in
        // any one area exactly one axis is filled to the brim and has no slack
        // at all to share out. A single fixture can therefore only test the
        // centring on one axis; on the other, "centred" and "flush against the
        // edge" are both 0 == 0 and the assertion says nothing. One wide area
        // and one tall one, each asserting it has the slack it is measuring
        // (`known-issues.md` lesson 70).
        let area = Rect::new(10.0, 20.0, 400.0, 300.0);
        let g = Grid::new(area, 5, 2, 3);
        let left = g.row_clues.x - area.x;
        let right = area.right() - g.cells.right();
        assert!(left > 0.5, "the wide fixture has no width to share out");
        assert!(
            (left - right).abs() < 0.001,
            "{left} on the left and {right} on the right"
        );

        let area = Rect::new(10.0, 20.0, 300.0, 460.0);
        let g = Grid::new(area, 5, 2, 3);
        let top = g.col_clues.y - area.y;
        let bottom = area.bottom() - g.cells.bottom();
        assert!(top > 0.5, "the tall fixture has no height to share out");
        assert!(
            (top - bottom).abs() < 0.001,
            "{top} above and {bottom} below"
        );
    }

    #[test]
    fn a_column_clue_sits_over_its_own_column() {
        // CLUE_HALF_WIDTH was 4.0, eyeballed, under a comment claiming no text
        // metric was available -- so every two-digit clue sat off its column.
        // Now a clue slot is exactly as wide as the cell it describes.
        for size in SHAPES {
            let app = playing("Cat");
            let g = app.grid(&Layout::new(size.0, size.1));
            for col in 0..app.grid_side {
                let cell = g.cell_rect(0, col);
                for slot in 0..g.col_slots {
                    let clue = g.col_clue_rect(col, slot);
                    assert_eq!(clue.x, cell.x, "at {size:?} column {col} slot {slot}");
                    assert_eq!(clue.w, cell.w, "at {size:?} column {col} slot {slot}");
                }
            }
        }
    }

    #[test]
    fn a_row_clue_sits_beside_its_own_row() {
        for size in SHAPES {
            let app = playing("Cat");
            let g = app.grid(&Layout::new(size.0, size.1));
            for row in 0..app.grid_side {
                let cell = g.cell_rect(row, 0);
                for slot in 0..g.row_slots {
                    let clue = g.row_clue_rect(row, slot);
                    assert_eq!(clue.y, cell.y, "at {size:?} row {row} slot {slot}");
                    assert_eq!(clue.h, cell.h, "at {size:?} row {row} slot {slot}");
                    assert!(
                        clue.right() <= g.cells.x + 0.001,
                        "at {size:?} row {row} slot {slot} runs into the cells"
                    );
                }
            }
        }
    }

    #[test]
    fn the_clue_bands_sit_against_the_cells_with_nothing_between() {
        for size in SHAPES {
            let app = playing("House");
            let g = app.grid(&Layout::new(size.0, size.1));
            assert!(
                (g.row_clues.right() - g.cells.x).abs() < 0.001,
                "at {size:?}"
            );
            assert!(
                (g.col_clues.bottom() - g.cells.y).abs() < 0.001,
                "at {size:?}"
            );
            assert!((g.row_clues.y - g.cells.y).abs() < 0.001, "at {size:?}");
            assert!((g.col_clues.x - g.cells.x).abs() < 0.001, "at {size:?}");
        }
    }

    #[test]
    fn a_grid_with_no_room_is_no_grid_rather_than_a_negative_one() {
        let none = Grid::new(Rect::EMPTY, 10, 2, 2);
        assert_eq!(none.cell, 0.0);
        assert!(none.cells.is_empty());
        assert_eq!(none.cell_rect(0, 0), Rect::EMPTY);
        assert_eq!(none.cell_hit(0, 0), Rect::EMPTY);
        let sideless = Grid::new(Rect::new(0.0, 0.0, 100.0, 100.0), 0, 2, 2);
        assert_eq!(sideless.cell, 0.0);
    }

    #[test]
    fn a_cell_off_the_grid_has_no_box_at_all() {
        let g = Grid::new(Rect::new(0.0, 0.0, 400.0, 400.0), 5, 2, 2);
        assert_eq!(g.cell_rect(5, 0), Rect::EMPTY);
        assert_eq!(g.cell_rect(0, 5), Rect::EMPTY);
        assert_eq!(g.cell_hit(5, 5), Rect::EMPTY);
        assert_eq!(g.row_clue_rect(5, 0), Rect::EMPTY);
        assert_eq!(g.row_clue_rect(0, 2), Rect::EMPTY);
        assert_eq!(g.col_clue_rect(5, 0), Rect::EMPTY);
        assert_eq!(g.col_clue_rect(0, 2), Rect::EMPTY);
    }

    #[test]
    fn the_cell_boxes_abut_without_overlapping() {
        // Half a gap each side means neighbours share an edge exactly. Rect
        // contains is half-open, so the shared edge belongs to the cell on the
        // right and nothing is claimed twice.
        let app = playing("Heart");
        let g = app.grid(&Layout::new(SIZE.0, SIZE.1));
        for row in 0..app.grid_side {
            for col in 1..app.grid_side {
                let a = g.cell_hit(row, col - 1);
                let b = g.cell_hit(row, col);
                assert!(
                    (b.x - a.right()).abs() < 0.001,
                    "boxes {a:?} and {b:?} do not meet"
                );
            }
        }
    }

    #[test]
    fn the_three_columns_of_a_list_entry_do_not_run_into_each_other() {
        // The old select screen put the name at +12, the size at +160 and the
        // thumbnail at +260 from the left margin, so a name over 144 pixels
        // wide ran straight through the size label.
        for size in SHAPES {
            let app = NonogramApp::new();
            let l = Layout::new(size.0, size.1);
            let list = app.list(&l);
            for i in 0..app.puzzles.len() {
                let entry = list.entry(i);
                if entry.is_empty() {
                    continue;
                }
                let name = list.name_rect(i);
                let sized = list.size_rect(i);
                let thumb = list.thumb_rect(i);
                if !sized.is_empty() {
                    assert!(
                        name.right() <= sized.x + 0.001,
                        "at {size:?} entry {i}: name {name:?} runs into size {sized:?}"
                    );
                }
                if !thumb.is_empty() {
                    assert!(
                        sized.right() <= thumb.x + 0.001,
                        "at {size:?} entry {i}: size {sized:?} runs into thumb {thumb:?}"
                    );
                    assert!(
                        thumb.right() <= entry.right() + 0.001,
                        "at {size:?} entry {i}: thumb {thumb:?} leaves entry {entry:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_size_column_is_as_wide_as_the_widest_label_in_the_catalogue() {
        // Measured, not guessed. The label column has to fit "15 x 15", not
        // whatever number looked right when the screen was written.
        let app = NonogramApp::new();
        let l = Layout::new(SIZE.0, SIZE.1);
        let list = app.list(&l);
        let widest = app
            .puzzles
            .iter()
            .map(|p| text::measure(&size_label(p.size), l.font, FontWeightHint::Regular))
            .fold(0.0f32, f32::max);
        assert!(widest > 0.0);
        assert!(
            list.size_w >= widest - 0.001,
            "the column is {} wide and the widest label is {widest}",
            list.size_w
        );
    }

    #[test]
    fn a_list_with_nothing_in_it_asks_for_no_room() {
        let none = List::new(Rect::new(0.0, 0.0, 300.0, 300.0), 0, 40.0);
        assert_eq!(none.step, 0.0);
        assert_eq!(none.entry(0), Rect::EMPTY);
        assert_eq!(none.thumb_rect(0), Rect::EMPTY);
        let roomless = List::new(Rect::EMPTY, 5, 40.0);
        assert_eq!(roomless.entry_h, 0.0);
    }

    #[test]
    fn a_short_list_does_not_stretch_its_entries_to_fill_the_screen() {
        // A three-entry list in a 900-pixel body would give each entry 300
        // pixels of height if the space were simply divided. ENTRY_SHARE caps
        // it, so the entries stay entry-sized and the list stays a list.
        let list = List::new(Rect::new(0.0, 0.0, 400.0, 900.0), 3, 40.0);
        assert!(
            list.step <= 900.0 * ENTRY_SHARE + 0.001,
            "an entry got {} pixels of a 900-pixel body",
            list.step
        );
    }

    #[test]
    fn the_entries_do_not_overlap_each_other() {
        for size in SHAPES {
            let app = NonogramApp::new();
            let list = app.list(&Layout::new(size.0, size.1));
            for i in 1..app.puzzles.len() {
                let a = list.entry(i - 1);
                let b = list.entry(i);
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                assert!(
                    b.y >= a.bottom() - 0.001,
                    "at {size:?} entries {} and {i} overlap: {a:?} {b:?}",
                    i - 1
                );
            }
        }
    }

    #[test]
    fn nothing_is_drawn_outside_the_window() {
        // Every screen, every shape. The old program drew a 520-pixel-wide
        // select screen into whatever window it happened to get.
        for size in SHAPES {
            for app in [NonogramApp::new(), playing("Cat"), {
                let mut won = playing("Heart");
                paint_solution(&mut won);
                won.screen = Screen::Won;
                won
            }] {
                let window = Rect::new(0.0, 0.0, size.0, size.1);
                for (target, r) in app.frame(size.0, size.1).hits() {
                    assert!(
                        r.x >= -0.001
                            && r.y >= -0.001
                            && r.right() <= window.right() + 0.001
                            && r.bottom() <= window.bottom() + 0.001,
                        "at {size:?} {target:?} is clickable at {r:?}, outside {window:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_string_is_told_where_to_stop() {
        // Every string in the old program carried max_width: None, so a name
        // or a hint longer than its column simply ran on across whatever was
        // beside it. 140x900 is in SHAPES because the footer hint is wider
        // than the window there.
        for size in SHAPES {
            for app in [NonogramApp::new(), playing("Cat")] {
                for cmd in app.frame(size.0, size.1).commands() {
                    if let RenderCommand::Text {
                        text, max_width, ..
                    } = cmd
                    {
                        assert!(
                            max_width.is_some(),
                            "at {size:?} {text:?} was drawn with no limit"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_layout_follows_the_window_rather_than_the_program() {
        // The whole point of the rewrite: the same state drawn at two sizes
        // must produce two different pictures.
        let app = playing("Heart");
        let small = app.grid(&Layout::new(400.0, 400.0));
        let large = app.grid(&Layout::new(1200.0, 1200.0));
        assert!(
            large.cell > small.cell * 2.0,
            "a window three times the size gave a {} pixel cell against {}",
            large.cell,
            small.cell
        );
    }

    #[test]
    fn every_control_the_program_has_can_be_reached_somewhere() {
        // A hit box nobody records is a control nobody can click. Between the
        // two screens all four must appear.
        let mut seen: Vec<Target> = all_targets(&NonogramApp::new(), SIZE);
        seen.extend(all_targets(&playing("Heart"), SIZE));
        let names: Vec<String> = seen.iter().map(probe::variant_name).collect();
        for wanted in ["Puzzle", "Cell", "Check", "Menu"] {
            assert!(
                names.iter().any(|n| n == wanted),
                "nothing on either screen records a {wanted}; only {names:?}"
            );
        }
    }

    // ── The window ─────────────────────────────────────────────────

    #[test]
    fn rendering_records_the_size_the_next_click_is_read_against() {
        // The old render took no size at all. This asks for a frame at a size
        // the program is not already at, so finding that size afterwards
        // cannot pass with render having done nothing.
        let mut app = playing("Heart");
        let odd = (SIZE.0 + 137.0, SIZE.1 - 61.0);
        assert!(
            (odd.0 - app.size_drawn().0).abs() > 0.01,
            "the fixture size is the size the program already records"
        );
        let tree = app.render(odd.0, odd.1);
        assert!(!tree.commands.is_empty(), "render produced no commands");
        assert_eq!(app.size_drawn(), odd);
    }

    #[test]
    fn a_resize_moves_the_layout_the_next_click_is_read_against() {
        // A resize is a resize whether or not a frame follows it: the window
        // may tell the program its new size before it asks for a picture, and
        // a click can arrive in between.
        let mut app = playing("Heart");
        let before = app.grid(&Layout::new(app.size_drawn().0, app.size_drawn().1));
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Resize {
                    width: 900,
                    height: 500
                }
            ),
            EventResult::Consumed
        );
        assert_eq!(app.size_drawn(), (900.0, 500.0));
        let after = app.grid(&Layout::new(900.0, 500.0));
        assert_ne!(after.cells, before.cells, "the layout did not follow");
    }

    #[test]
    fn a_click_after_a_resize_is_read_against_the_new_window() {
        // The failure this catches is the one that makes a resized window
        // unplayable: clicks land on the cells the old size put there.
        let mut app = playing("House");
        let big = (1200.0, 1000.0);
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1200,
                height: 1000,
            },
        );
        let g = app.grid(&Layout::new(big.0, big.1));
        let cell = g.cell_rect(6, 7);
        assert!(!cell.is_empty(), "the fixture puzzle has no cell 6,7");
        let (x, y) = cell.centre();
        let ev = MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        assert_eq!(
            handle_event(&mut app, &Event::Mouse(ev)),
            EventResult::Consumed
        );
        assert_eq!((app.cursor_row, app.cursor_col), (6, 7));
    }

    #[test]
    fn a_window_squashed_to_nothing_still_lays_out() {
        // A window can be dragged to nothing. The layout must survive it
        // rather than divide by the zero it was handed.
        let mut app = playing("Heart");
        app.resize(0.0, 0.0);
        assert!(app.size_drawn().0 > 0.0 && app.size_drawn().1 > 0.0);
        let l = Layout::new(0.0, 0.0);
        assert!(l.window.w > 0.0 && l.window.h > 0.0);
        let g = app.grid(&l);
        assert!(g.cell >= 0.0 && g.gap >= 0.0);
        // And it still draws, without panicking on the way.
        let _ = app.frame(l.window.w, l.window.h);
        let _ = NonogramApp::new().frame(l.window.w, l.window.h);
    }

    #[test]
    fn closing_the_window_exits_and_nothing_else_does() {
        let mut app = playing("Heart");
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
        assert_eq!(
            app.on_event(&Event::FocusIn),
            Response::Idle,
            "an event the game does not use should not force a repaint"
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Space))),
            Response::Redraw,
            "a filled cell must repaint, or the grid on screen is a move behind"
        );
    }

    #[test]
    fn a_tick_off_the_board_does_not_repaint() {
        // The clock only runs while a puzzle is being played, so a tick on the
        // select screen changes nothing and must not ask for a frame -- that
        // is five wasted repaints a second, forever, on a screen at rest.
        let mut app = NonogramApp::new();
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 200 }),
            Response::Idle
        );
        let mut playing_now = playing("Heart");
        assert_eq!(
            playing_now.on_event(&Event::Tick { elapsed_ms: 200 }),
            Response::Redraw,
            "the clock advanced without the header being redrawn"
        );
    }

    #[test]
    fn the_window_names_itself_and_says_the_same_thing_twice() {
        let app = NonogramApp::new();
        assert_eq!(app.title(), "Nonogram");
        assert_eq!(app.app_id(), "nonogram");
        let (w, h) = app.initial_size();
        assert_eq!((w, h), (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32));
        assert_eq!(
            (
                f32::from(u16::try_from(w).unwrap()),
                f32::from(u16::try_from(h).unwrap())
            ),
            SIZE,
            "the window opens at one size and the tests read clicks at another"
        );
    }
}
