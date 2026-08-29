//! Conway's Game of Life, in a real window.
//!
//! A toroidal 80x60 world, run or stepped, drawn to fit whatever window it is
//! given, editable with the pointer or the cursor keys, with ten preset
//! patterns and a live generation and population count.
//!
//! # What wiring this up found
//!
//! `main` built a `LifeApp`, dropped it and exited. Nothing below it was
//! reachable, so nothing below it had ever been run.
//!
//! 1. **The simulation could not run for one generation.** Both key handlers
//!    destructured `Event::Key(KeyEvent { key, .. })`, and the field the `..`
//!    swallowed was `pressed` — so every key ran twice, once on the press and
//!    once on the release. The compositor really does send both
//!    (`gui/compositor/src/lib.rs` builds a `KeyEvent` with `pressed: false`
//!    for each key-up), and the key this ruins is Space: it ran
//!    `self.running = !self.running` on the way down and again on the way up,
//!    so the program started and stopped between one frame and the next. Game
//!    of Life could never have advanced a single generation under the key its
//!    own help bar calls "Play/Pause". The rest of the damage:
//!    - **`Enter` toggled a cell twice, which is not toggling it at all.** The
//!      help bar's "Enter=Toggle" and the help sheet's "Toggle cell at cursor"
//!      described a key that provably did nothing.
//!    - **`F1` and `G` had no reachable effect** for the same reason as Space:
//!      the flag went up on the press and back down on the release with no
//!      frame drawn in between, so the help sheet could never be seen and the
//!      grid lines could never be turned off.
//!    - **`S` — "Single step" — advanced two generations**, and said so.
//!    - **Every arrow key moved the cursor two cells.**
//!    - **Placing a pattern flipped a cell off underneath it.** `Enter` in the
//!      pattern menu placed the pattern *and* set `view = Main`, so the
//!      release was routed to the main handler, whose `Enter` toggles the cell
//!      at the cursor — which is the cell the pattern was centred on. Every
//!      pattern whose own cell list contains `(0, 0)` therefore arrived with
//!      one cell missing: a Blinker placed as a two-cell domino that dies on
//!      the next generation, and a Beacon that is not a beacon.
//!    - **`R` drew two soups and threw the first away**; `C` cleared twice.
//! 2. **The tick handler was correct and no tick ever arrived.** `tick_accum`,
//!    `speed_ms` and the catch-up loop were all written properly against the
//!    `elapsed_ms` a `Tick` carries — this file was the readiest for a clock of
//!    any in the campaign — and there was no `App` impl, hence no
//!    `tick_interval`, hence no clock. `known-issues.md` lesson 47's eighth
//!    application, and the first where the timekeeping itself was already right.
//! 3. **The catch-up loop was unbounded.** `while self.tick_accum >= interval`
//!    with an interval as low as 15ms means a window that was suspended for ten
//!    seconds wakes up owing 666 generations, each a 4800-cell neighbour sweep,
//!    all inside one frame. Catch-up is capped at [`MAX_CATCHUP`] now and the
//!    remainder is dropped rather than banked.
//! 4. **The click-to-cell mapping was a pair of constants, and disagreed with
//!    the drawing.** It read `let grid_y_start = 44.0;` — the same `44.0`
//!    written again in `visible_rows` and a third time in `render` — and a
//!    `cell_size` field that was set to `8.0` in `new` and never written again
//!    by anything. Worse than the duplication: `render` wrapped with
//!    `(self.view_row + row) % self.grid.height` while the click added
//!    `view_row` and *rejected* anything past the end, so the two disagreed
//!    about which cell was where the moment the viewport moved. There is one
//!    mapping now — [`Layout::cell_rect`] — the drawing pass records a hit box
//!    from it for every cell, and the click is read from those boxes, so the
//!    two cannot drift.
//! 5. **The viewport did not exist.** `view_row` and `view_col` were commented
//!    "Viewport offset for scrolling", read by the drawing and the click, and
//!    written by nothing in the program. The module doc's "Zoom in/out with
//!    +/-" described the same absent machinery from the other end: `cell_size`
//!    was a fixed 8.0 and no key anywhere in the file was `+` or `-`. Rather
//!    than build a viewport nobody asked for, the whole world is now fitted to
//!    the window — at any size, every one of the 4800 cells is on screen — and
//!    `+`/`-` do the thing they plausibly should, which is change the speed.
//! 6. **Nothing was clickable but a cell.** `render` returned a bare
//!    `Vec<RenderCommand>` with no hit boxes at all, `handle_pattern_event`
//!    matched only `Event::Key`, and so the pattern menu — a list of ten named
//!    rows, drawn to be picked from — could not be picked from with a pointer.
//! 7. **The two overlays did not block what they covered.** The help sheet
//!    drew over the whole window and the main handler went on taking keys and
//!    clicks underneath it, so a click while reading the help toggled a cell
//!    you could not see. Both sheets are modal now, and both close by clicking
//!    anywhere off them.
//! 8. **`Ctrl` was filtered in one handler and not the other.** `handle_main_event`
//!    returned early on `modifiers.ctrl`; `handle_pattern_event` did not, so
//!    Ctrl-Enter placed a pattern while Ctrl-C did nothing.
//! 9. **The header was six strings at fixed pixel offsets** — 12, 160, 260,
//!    400, 530 and 640 — each with `max_width: None`. In any window narrower
//!    than about 700 pixels the grid size ran off the right edge, and in one
//!    narrower than 530 the speed did too. The bottom help bar was a single
//!    hundred-character line at font size 11, also unbounded, also clipped.
//! 10. **Twelve blanket `#![allow]`s sat on lines 15-26**, `dead_code` and
//!     `unused_imports` among them, which is what kept 5 quiet — along with six
//!     unused palette entries and a hand-written `impl Clone for Grid`
//!     reproducing the derive field for field.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const CRUST: Color = Color::from_hex(0x11_111B);
const MANTLE: Color = Color::from_hex(0x18_1825);
const SURFACE0: Color = Color::from_hex(0x31_3244);
const SURFACE1: Color = Color::from_hex(0x45_475A);
const TEXT_COLOR: Color = Color::from_hex(0xCD_D6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6_ADC8);
const BLUE: Color = Color::from_hex(0x89_B4FA);
const GREEN: Color = Color::from_hex(0xA6_E3A1);
const YELLOW: Color = Color::from_hex(0xF9_E2AF);
const LAVENDER: Color = Color::from_hex(0xB4_BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C_7086);

const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 760.0;

/// The seed a soup falls back to when the kernel has no entropy to give.
///
/// A Life board may be predictable: the worst outcome is that today's random
/// soup evolves the way yesterday's did. Refusing to start would be the worse
/// failure — see [`guitk::rng::seeded_from_system`]. "LIFE!!!!" in ASCII.
const FALLBACK_SEED: u64 = 0x4C49_4645_2121_2121;

/// The most generations one tick may catch up on.
///
/// A tick carries the milliseconds that really passed, not the interval that
/// was asked for, so a window that was suspended — or a machine that stalled —
/// hands back an `elapsed_ms` of any size at all. At speed 9 the interval is
/// 15ms, so ten seconds of absence is 666 generations, each a 4800-cell
/// neighbour sweep: the frame that ran them would take longer than the absence
/// did and the program would appear to hang. Beyond this many the backlog is
/// *dropped* rather than banked, because a Life board has no notion of being
/// behind — the generations nobody watched are ones nobody can miss.
const MAX_CATCHUP: u32 = 8;

// ── Preset patterns ────────────────────────────────────────────────────────

/// One of the shapes the pattern menu can stamp onto the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Glider,
    Blinker,
    Toad,
    Beacon,
    Pulsar,
    GosperGun,
    Lwss,
    Diehard,
    Acorn,
    RPentomino,
}

impl Pattern {
    /// Every pattern, in menu order.
    pub const ALL: &'static [Pattern] = &[
        Pattern::Glider,
        Pattern::Blinker,
        Pattern::Toad,
        Pattern::Beacon,
        Pattern::Pulsar,
        Pattern::GosperGun,
        Pattern::Lwss,
        Pattern::Diehard,
        Pattern::Acorn,
        Pattern::RPentomino,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Glider => "Glider",
            Self::Blinker => "Blinker",
            Self::Toad => "Toad",
            Self::Beacon => "Beacon",
            Self::Pulsar => "Pulsar",
            Self::GosperGun => "Gosper Glider Gun",
            Self::Lwss => "LWSS",
            Self::Diehard => "Diehard",
            Self::Acorn => "Acorn",
            Self::RPentomino => "R-Pentomino",
        }
    }

    /// The pattern's live cells, as `(row, col)` offsets from where it is
    /// placed. Every offset is non-negative, so a pattern grows down and right
    /// from the cursor rather than around it.
    #[must_use]
    pub fn cells(self) -> Vec<(i32, i32)> {
        match self {
            Self::Glider => vec![(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)],
            Self::Blinker => vec![(0, 0), (0, 1), (0, 2)],
            Self::Toad => vec![(0, 1), (0, 2), (0, 3), (1, 0), (1, 1), (1, 2)],
            Self::Beacon => vec![(0, 0), (0, 1), (1, 0), (2, 3), (3, 2), (3, 3)],
            Self::Pulsar => {
                // Symmetric about (6, 6): define the top-left quadrant and
                // mirror it into the other three.
                //
                // Every cell here is strictly inside its quadrant — no row 6,
                // no column 6 — because a cell *on* a mirror line is its own
                // reflection and so contributes two cells to the figure rather
                // than four. The version this replaces put six of its twelve on
                // the centre lines, which is why it drew a 36-cell figure under
                // the name of a 48-cell one, and why that figure was not an
                // oscillator of any period at all: it decayed to a pair of
                // blocks and a blinker in three generations and stopped.
                let mut cells = Vec::new();
                let quarter = [
                    (0, 2),
                    (0, 3),
                    (0, 4),
                    (2, 0),
                    (3, 0),
                    (4, 0),
                    (2, 5),
                    (3, 5),
                    (4, 5),
                    (5, 2),
                    (5, 3),
                    (5, 4),
                ];
                for &(r, c) in &quarter {
                    let (mr, mc) = (12i32.saturating_sub(r), 12i32.saturating_sub(c));
                    cells.push((r, c));
                    cells.push((r, mc));
                    cells.push((mr, c));
                    cells.push((mr, mc));
                }
                cells.sort_unstable();
                cells.dedup();
                cells
            }
            Self::GosperGun => vec![
                (0, 24),
                (1, 22),
                (1, 24),
                (2, 12),
                (2, 13),
                (2, 20),
                (2, 21),
                (2, 34),
                (2, 35),
                (3, 11),
                (3, 15),
                (3, 20),
                (3, 21),
                (3, 34),
                (3, 35),
                (4, 0),
                (4, 1),
                (4, 10),
                (4, 16),
                (4, 20),
                (4, 21),
                (5, 0),
                (5, 1),
                (5, 10),
                (5, 14),
                (5, 16),
                (5, 17),
                (5, 22),
                (5, 24),
                (6, 10),
                (6, 16),
                (6, 24),
                (7, 11),
                (7, 15),
                (8, 12),
                (8, 13),
            ],
            Self::Lwss => vec![
                (0, 1),
                (0, 4),
                (1, 0),
                (2, 0),
                (2, 4),
                (3, 0),
                (3, 1),
                (3, 2),
                (3, 3),
            ],
            Self::Diehard => vec![(0, 6), (1, 0), (1, 1), (2, 1), (2, 5), (2, 6), (2, 7)],
            Self::Acorn => vec![(0, 1), (1, 3), (2, 0), (2, 1), (2, 4), (2, 5), (2, 6)],
            Self::RPentomino => vec![(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)],
        }
    }
}

// ── Grid ───────────────────────────────────────────────────────────────────

const GRID_COLS: usize = 80;
const GRID_ROWS: usize = 60;

/// A toroidal board of live and dead cells, in row-major order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<bool>,
}

impl Grid {
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![false; cols.saturating_mul(rows)],
        }
    }

    #[must_use]
    pub const fn cols(&self) -> usize {
        self.cols
    }

    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// The flat index of a cell, or `None` if it is off the board.
    ///
    /// The one place row/column pairs become indices. Everything that reaches
    /// into `cells` goes through here, which is what makes "off the board"
    /// a `None` rather than a plausible index into the wrong row.
    #[must_use]
    pub fn index(&self, row: usize, col: usize) -> Option<usize> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        row.checked_mul(self.cols)?.checked_add(col)
    }

    /// The `(row, col)` a flat index names, or `None` if it names no cell.
    #[must_use]
    pub fn coords(&self, index: usize) -> Option<(usize, usize)> {
        if self.cols == 0 || index >= self.cells.len() {
            return None;
        }
        Some((index.checked_div(self.cols)?, index.checked_rem(self.cols)?))
    }

    #[must_use]
    pub fn get(&self, row: usize, col: usize) -> bool {
        self.index(row, col)
            .and_then(|i| self.cells.get(i))
            .copied()
            .unwrap_or(false)
    }

    pub fn set(&mut self, row: usize, col: usize, alive: bool) {
        if let Some(i) = self.index(row, col)
            && let Some(cell) = self.cells.get_mut(i)
        {
            *cell = alive;
        }
    }

    pub fn toggle(&mut self, row: usize, col: usize) {
        if let Some(i) = self.index(row, col)
            && let Some(cell) = self.cells.get_mut(i)
        {
            *cell = !*cell;
        }
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = false;
        }
    }

    #[must_use]
    pub fn population(&self) -> usize {
        self.cells.iter().filter(|&&c| c).count()
    }

    /// Live neighbours of a cell, wrapping at every edge.
    #[must_use]
    pub fn neighbours(&self, row: usize, col: usize) -> u8 {
        if self.rows == 0 || self.cols == 0 {
            return 0;
        }
        let mut count: u8 = 0;
        for dr in [-1i32, 0, 1] {
            for dc in [-1i32, 0, 1] {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let Ok(rows) = i32::try_from(self.rows) else {
                    return count;
                };
                let Ok(cols) = i32::try_from(self.cols) else {
                    return count;
                };
                let Ok(r) = i32::try_from(row) else {
                    return count;
                };
                let Ok(c) = i32::try_from(col) else {
                    return count;
                };
                let nr = r.saturating_add(dr).rem_euclid(rows);
                let nc = c.saturating_add(dc).rem_euclid(cols);
                #[allow(clippy::cast_sign_loss)] // rem_euclid on a positive modulus is >= 0
                if self.get(nr as usize, nc as usize) {
                    count = count.saturating_add(1);
                }
            }
        }
        count
    }

    /// One generation of Conway's B3/S23: a live cell with two or three live
    /// neighbours survives, a dead cell with exactly three is born, and
    /// everything else is dead next time.
    #[must_use]
    pub fn stepped(&self) -> Grid {
        let mut next = Grid::new(self.cols, self.rows);
        for row in 0..self.rows {
            for col in 0..self.cols {
                let n = self.neighbours(row, col);
                let alive = if self.get(row, col) {
                    n == 2 || n == 3
                } else {
                    n == 3
                };
                next.set(row, col, alive);
            }
        }
        next
    }

    /// Fill the board at random, `density` cells in a hundred alive.
    pub fn randomize(&mut self, rng: &mut SeededRng, density: u64) {
        for i in 0..self.cells.len() {
            let alive = rng.chance_in(density, 100);
            if let Some(cell) = self.cells.get_mut(i) {
                *cell = alive;
            }
        }
    }

    /// Stamp a pattern's live cells onto the board with its origin at
    /// `(row, col)`, wrapping at the edges as the board itself does.
    pub fn place(&mut self, pattern: Pattern, row: usize, col: usize) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let (Ok(rows), Ok(cols)) = (i32::try_from(self.rows), i32::try_from(self.cols)) else {
            return;
        };
        let (Ok(r0), Ok(c0)) = (i32::try_from(row), i32::try_from(col)) else {
            return;
        };
        for (dr, dc) in pattern.cells() {
            let r = (r0.wrapping_add(dr)).rem_euclid(rows);
            let c = (c0.wrapping_add(dc)).rem_euclid(cols);
            #[allow(clippy::cast_sign_loss)] // rem_euclid on a positive modulus is >= 0
            self.set(r as usize, c as usize, true);
        }
    }
}

// ── Targets, actions and views ─────────────────────────────────────────────

/// Something on the screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A cell of the board, by flat index.
    Cell(usize),
    PlayPause,
    StepOnce,
    Clear,
    Randomize,
    Patterns,
    GridLines,
    Slower,
    Faster,
    Help,
    /// A row of the pattern menu, by index into [`Pattern::ALL`].
    PatternRow(usize),
    PlacePattern,
    /// Anywhere off the pattern sheet.
    ClosePatterns,
    /// Anywhere at all, while the help sheet is up.
    CloseHelp,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Everything the program can be asked to do, from either input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    TogglePlay,
    StepOnce,
    Clear,
    Randomize,
    OpenPatterns,
    ClosePatterns,
    PlacePattern,
    /// Point the menu at `Pattern::ALL[i]`.
    SelectPattern(usize),
    /// Move the menu's selection by a signed number of rows, clamped.
    MoveSelection(i32),
    ToggleGridLines,
    ToggleHelp,
    CloseHelp,
    /// Set the speed to a value in `1..=9`, clamped.
    SetSpeed(u32),
    /// Move the speed by a signed step, clamped to `1..=9`.
    NudgeSpeed(i32),
    /// Move the cursor by `(rows, cols)`, wrapping at the edges.
    MoveCursor(i32, i32),
    /// Flip the cell at a flat index, and put the cursor on it.
    ToggleCell(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Board,
    PatternMenu,
}

/// Every key the program answers, and what it does.
///
/// Drawn on the help sheet, and walked by a test in both directions — every
/// key named here answers, and every key that answers is named here — so the
/// sheet cannot drift from the program the way the old help bar had, which
/// named nine keys for a program that answered twenty-two and described three
/// of them (Space, Enter, F1) as doing things they provably did not do.
const HELP_ROWS: [(&str, &str); 11] = [
    ("Space", "run or pause"),
    ("S", "one generation"),
    ("C", "clear the board"),
    ("R", "random soup"),
    ("P", "pattern menu"),
    ("G", "grid lines"),
    ("Enter", "flip the cell at the cursor"),
    ("Arrows", "move the cursor"),
    ("1 - 9", "speed"),
    ("- and +", "slower and faster"),
    ("F1 / Esc", "this sheet"),
];

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the board keeps no matter what.
const BOARD_SHARE: f32 = 0.72;

/// Which band goes first when they do not both fit: controls, then header.
///
/// The controls go first because every one of them is a button for a key that
/// still works without it. The header goes last: the generation count, the
/// population and the run/pause state are the only things on screen that the
/// board itself does not say.
const BAND_DROP_ORDER: [usize; 2] = [1, 0];

/// Every rectangle in the window, derived from the window's own size and the
/// board's own dimensions.
///
/// Built fresh on every frame and never stored on the model. A layout kept on
/// the model is a layout that can disagree with the window it is drawn in —
/// which is exactly what a `cell_size` field of `8.0` and a `grid_y_start` of
/// `44.0` were.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Title, run state, generation, population, speed and board size.
    pub header: Rect,
    /// The cells. Exactly `cols * cell` by `rows * cell`, centred.
    pub board: Rect,
    /// The buttons along the bottom.
    pub controls: Rect,
    /// The pattern menu.
    pub sheet: Rect,
    /// The help sheet.
    pub help: Rect,
    /// The side of one cell.
    pub cell: f32,
    pub font: f32,
    pub big: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size showing a `cols` by `rows`
    /// board.
    #[must_use]
    pub fn new(width: f32, height: f32, cols: usize, rows: usize) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 44.0).clamp(8.0, 16.0);
        let big = (font * 1.5).clamp(12.0, 24.0);
        // The lower clamp is a floor, and a floor can be taller than the room
        // it stands in: at 1x1 a two-pixel margin put the board's own origin at
        // (2, 2), outside the window it is a margin *within*. A margin may never
        // be more than a quarter of the side it is taken from, twice over, and
        // that is what keeps every band inside the window at every size.
        let pad = (w.min(h) * 0.015).clamp(2.0, 10.0).min(w.min(h) / 4.0);

        // What each band would like, in [header, controls] order.
        let mut wants = [(h * 0.07).clamp(20.0, 38.0), (h * 0.07).clamp(22.0, 38.0)];
        // What is left for chrome once the board has its share and the gaps
        // above and below it. The padding is charged here rather than to the
        // board: taking it from the board's side would turn a promised share of
        // the window into rather less than that share of a small one.
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, ctl_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall. Both read the same to `shows`, but only one of them reads the
        // same to anything asking "is this band gone, or merely thin?"
        let header = if hdr_h > 0.0 {
            Rect::new(0.0, 0.0, w, hdr_h)
        } else {
            Rect::EMPTY
        };
        let controls = if ctl_h > 0.0 {
            Rect::new(0.0, h - ctl_h, w, ctl_h)
        } else {
            Rect::EMPTY
        };

        // From the heights, not from `header.bottom()`: a dropped band's bottom
        // is zero, which would be right by accident today and wrong the moment
        // `BAND_DROP_ORDER` is reordered.
        let top = hdr_h;
        let bottom = if ctl_h > 0.0 { h - ctl_h } else { h };
        let band = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        // The whole world, fitted. Square cells, because a Life board with
        // oblong cells reads as a different shape than it is — a glider stops
        // looking diagonal — and because a square cell is the only one whose
        // hit box is where the eye says it is.
        let cell = if cols == 0 || rows == 0 {
            0.0
        } else {
            (band.w / cols as f32).min(band.h / rows as f32).max(0.0)
        };
        let bw = cell * cols as f32;
        let bh = cell * rows as f32;
        let board = Rect::new(
            band.x + (band.w - bw) / 2.0,
            band.y + (band.h - bh) / 2.0,
            bw,
            bh,
        );

        let sheet_w = (w * 0.9).min(340.0);
        let sheet_h = (h * 0.9).min(400.0);
        let sheet = Rect::new((w - sheet_w) / 2.0, (h - sheet_h) / 2.0, sheet_w, sheet_h);

        let help_w = (w * 0.92).min(430.0);
        let help_h = (h * 0.92).min(340.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            board,
            controls,
            sheet,
            help,
            cell,
            font,
            big,
            pad,
        }
    }

    /// Whether a band has room to say anything.
    #[must_use]
    pub fn shows(&self, band: Rect) -> bool {
        band.h >= 11.0 && band.w >= 60.0
    }

    /// The rectangle a cell occupies.
    ///
    /// The single screen-to-board mapping. The drawing pass records a hit box
    /// from this for every cell and the pointer is read from those boxes, so
    /// "where a cell is drawn" and "where clicking hits it" are one fact.
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        Rect::new(
            self.board.x + col as f32 * self.cell,
            self.board.y + row as f32 * self.cell,
            self.cell,
            self.cell,
        )
    }
}

// ── App ────────────────────────────────────────────────────────────────────

/// Conway's Game of Life.
pub struct LifeApp {
    grid: Grid,
    view: View,
    running: bool,
    generation: u64,
    /// 1 (slowest) to 9 (fastest).
    speed: u32,
    cursor_row: usize,
    cursor_col: usize,
    selected_pattern: usize,
    tick_accum: u64,
    /// Generations dropped because the catch-up cap was hit. Shown nowhere;
    /// read by the tests, which is how [`MAX_CATCHUP`] is checked to be a cap
    /// on work done rather than on time accounted.
    dropped: u64,
    rng: SeededRng,
    show_grid: bool,
    show_help: bool,
    /// The size the window last drew at, so the next click is read against the
    /// pixels the player is looking at.
    size_drawn: (f32, f32),
}

impl LifeApp {
    #[must_use]
    pub fn new() -> Self {
        Self::from_rng(seeded_from_system(FALLBACK_SEED))
    }

    /// A board whose soups come from a known seed, for tests.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        Self::from_rng(SeededRng::new(seed))
    }

    fn from_rng(rng: SeededRng) -> Self {
        let mut app = Self {
            grid: Grid::new(GRID_COLS, GRID_ROWS),
            view: View::Board,
            running: false,
            generation: 0,
            speed: 5,
            cursor_row: GRID_ROWS / 2,
            cursor_col: GRID_COLS / 2,
            selected_pattern: 0,
            tick_accum: 0,
            dropped: 0,
            rng,
            show_grid: true,
            show_help: false,
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        // A gun in the corner, so an empty window is not what the program opens
        // with. Placed away from the cursor so the opening board does not sit
        // under the cursor highlight.
        app.grid.place(Pattern::GosperGun, 6, 6);
        app
    }

    // ── Readers ────────────────────────────────────────────────────────────

    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(
            self.size_drawn.0,
            self.size_drawn.1,
            self.grid.cols(),
            self.grid.rows(),
        )
    }

    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.grid
    }

    #[must_use]
    pub const fn view(&self) -> View {
        self.view
    }

    #[must_use]
    pub const fn running(&self) -> bool {
        self.running
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn speed(&self) -> u32 {
        self.speed
    }

    #[must_use]
    pub const fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    #[must_use]
    pub const fn selected_pattern(&self) -> usize {
        self.selected_pattern
    }

    #[must_use]
    pub fn pattern(&self) -> Pattern {
        Pattern::ALL
            .get(self.selected_pattern)
            .copied()
            .unwrap_or(Pattern::Glider)
    }

    #[must_use]
    pub const fn show_grid(&self) -> bool {
        self.show_grid
    }

    #[must_use]
    pub const fn show_help(&self) -> bool {
        self.show_help
    }

    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Milliseconds between generations at the current speed.
    ///
    /// Never zero, so the catch-up loop cannot spin: an interval of nought
    /// would make every `elapsed_ms` an infinite backlog.
    #[must_use]
    pub const fn speed_ms(&self) -> u64 {
        match self.speed {
            1 => 500,
            2 => 350,
            3 => 250,
            4 => 175,
            5 => 120,
            6 => 80,
            7 => 50,
            8 => 30,
            _ => 15,
        }
    }

    /// Whether a clock is wanted right now.
    #[must_use]
    pub const fn clock_running(&self) -> bool {
        self.running && matches!(self.view, View::Board) && !self.show_help
    }

    // ── Doing ──────────────────────────────────────────────────────────────

    fn step(&mut self) {
        self.grid = self.grid.stepped();
        self.generation = self.generation.saturating_add(1);
    }

    /// Carry out an action, from whichever input asked for it.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::TogglePlay => {
                self.running = !self.running;
                // The accumulator belongs to the run that is starting, not to
                // the one that stopped: keeping it would make the first
                // generation after a pause arrive early by however long the
                // pause interrupted.
                self.tick_accum = 0;
            }
            Action::StepOnce => {
                // A single step while running is meaningless — the clock is
                // already taking them — so it pauses first and then takes one.
                // The old code silently ignored `S` while running, which reads
                // as a broken key.
                self.running = false;
                self.step();
                self.tick_accum = 0;
            }
            Action::Clear => {
                self.grid.clear();
                self.generation = 0;
                self.running = false;
                self.tick_accum = 0;
            }
            Action::Randomize => {
                self.grid.randomize(&mut self.rng, 25);
                self.generation = 0;
                self.tick_accum = 0;
            }
            Action::OpenPatterns => {
                self.view = View::PatternMenu;
                self.running = false;
            }
            Action::ClosePatterns => self.view = View::Board,
            Action::PlacePattern => {
                self.grid
                    .place(self.pattern(), self.cursor_row, self.cursor_col);
                self.view = View::Board;
            }
            Action::SelectPattern(i) => {
                if i < Pattern::ALL.len() {
                    self.selected_pattern = i;
                }
            }
            Action::MoveSelection(delta) => {
                let last = Pattern::ALL.len().saturating_sub(1);
                let Ok(here) = i32::try_from(self.selected_pattern) else {
                    return;
                };
                let Ok(last_i) = i32::try_from(last) else {
                    return;
                };
                let next = here.saturating_add(delta).clamp(0, last_i);
                self.selected_pattern = usize::try_from(next).unwrap_or(0);
            }
            Action::ToggleGridLines => self.show_grid = !self.show_grid,
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::CloseHelp => self.show_help = false,
            Action::SetSpeed(s) => self.speed = s.clamp(1, 9),
            Action::NudgeSpeed(delta) => {
                let Ok(here) = i32::try_from(self.speed) else {
                    return;
                };
                let next = here.saturating_add(delta).clamp(1, 9);
                self.speed = u32::try_from(next).unwrap_or(5);
            }
            Action::MoveCursor(dr, dc) => self.move_cursor(dr, dc),
            Action::ToggleCell(index) => {
                if let Some((row, col)) = self.grid.coords(index) {
                    self.grid.toggle(row, col);
                    self.cursor_row = row;
                    self.cursor_col = col;
                }
            }
        }
    }

    fn move_cursor(&mut self, dr: i32, dc: i32) {
        let (Ok(rows), Ok(cols)) = (
            i32::try_from(self.grid.rows()),
            i32::try_from(self.grid.cols()),
        ) else {
            return;
        };
        if rows == 0 || cols == 0 {
            return;
        }
        let (Ok(r), Ok(c)) = (
            i32::try_from(self.cursor_row),
            i32::try_from(self.cursor_col),
        ) else {
            return;
        };
        let nr = r.saturating_add(dr).rem_euclid(rows);
        let nc = c.saturating_add(dc).rem_euclid(cols);
        self.cursor_row = usize::try_from(nr).unwrap_or(0);
        self.cursor_col = usize::try_from(nc).unwrap_or(0);
    }

    /// Advance the clock by the milliseconds that really passed.
    pub fn tick(&mut self, elapsed_ms: u64) -> EventResult {
        if !self.clock_running() {
            return EventResult::Ignored;
        }
        self.tick_accum = self.tick_accum.saturating_add(elapsed_ms);
        let interval = self.speed_ms();
        let mut taken = 0u32;
        while self.tick_accum >= interval {
            if taken >= MAX_CATCHUP {
                // Drop the rest rather than bank it. See `MAX_CATCHUP`.
                let owed = self.tick_accum.checked_div(interval).unwrap_or(0);
                self.dropped = self.dropped.saturating_add(owed);
                self.tick_accum = self.tick_accum.checked_rem(interval).unwrap_or(0);
                break;
            }
            self.tick_accum = self.tick_accum.saturating_sub(interval);
            self.step();
            taken = taken.saturating_add(1);
        }
        if taken == 0 {
            // No generation ran, so nothing on screen changed. Saying so keeps
            // the window from redrawing 4800 cells to show the same board.
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    // ── Input ──────────────────────────────────────────────────────────────

    /// The action a key asks for, or `None` if the program does not answer it.
    ///
    /// A pure function of the key and the view, so a test can ask what a key
    /// means without the answer depending on what the last key did.
    #[must_use]
    pub fn key_action(&self, key: Key) -> Option<Action> {
        // The help sheet is modal: it covers the board, so letting the board's
        // keys through would edit something the reader cannot see.
        if self.show_help {
            return match key {
                Key::F1 | Key::Escape => Some(Action::CloseHelp),
                _ => None,
            };
        }
        match self.view {
            View::PatternMenu => match key {
                Key::Up => Some(Action::MoveSelection(-1)),
                Key::Down => Some(Action::MoveSelection(1)),
                Key::Enter => Some(Action::PlacePattern),
                Key::Escape | Key::P => Some(Action::ClosePatterns),
                _ => None,
            },
            View::Board => match key {
                Key::Space => Some(Action::TogglePlay),
                Key::S => Some(Action::StepOnce),
                Key::C => Some(Action::Clear),
                Key::R => Some(Action::Randomize),
                Key::P => Some(Action::OpenPatterns),
                Key::G => Some(Action::ToggleGridLines),
                Key::F1 => Some(Action::ToggleHelp),
                Key::Enter => self
                    .grid
                    .index(self.cursor_row, self.cursor_col)
                    .map(Action::ToggleCell),
                Key::Up => Some(Action::MoveCursor(-1, 0)),
                Key::Down => Some(Action::MoveCursor(1, 0)),
                Key::Left => Some(Action::MoveCursor(0, -1)),
                Key::Right => Some(Action::MoveCursor(0, 1)),
                Key::Minus => Some(Action::NudgeSpeed(-1)),
                Key::Equals => Some(Action::NudgeSpeed(1)),
                Key::Num1 => Some(Action::SetSpeed(1)),
                Key::Num2 => Some(Action::SetSpeed(2)),
                Key::Num3 => Some(Action::SetSpeed(3)),
                Key::Num4 => Some(Action::SetSpeed(4)),
                Key::Num5 => Some(Action::SetSpeed(5)),
                Key::Num6 => Some(Action::SetSpeed(6)),
                Key::Num7 => Some(Action::SetSpeed(7)),
                Key::Num8 => Some(Action::SetSpeed(8)),
                Key::Num9 => Some(Action::SetSpeed(9)),
                _ => None,
            },
        }
    }

    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The release, not the press, is the half that used to run everything a
        // second time. One test drives a whole key-down/key-up pair through
        // this and asserts the state moved once.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        // Filtered in one place, so the pattern menu cannot answer a shortcut
        // the board refuses — which is what it did.
        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {
            return EventResult::Ignored;
        }
        match self.key_action(ev.key) {
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
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        let action = match target {
            Target::Cell(i) => Action::ToggleCell(i),
            Target::PlayPause => Action::TogglePlay,
            Target::StepOnce => Action::StepOnce,
            Target::Clear => Action::Clear,
            Target::Randomize => Action::Randomize,
            Target::Patterns => Action::OpenPatterns,
            Target::GridLines => Action::ToggleGridLines,
            Target::Slower => Action::NudgeSpeed(-1),
            Target::Faster => Action::NudgeSpeed(1),
            Target::Help => Action::ToggleHelp,
            Target::PatternRow(i) => Action::SelectPattern(i),
            Target::PlacePattern => Action::PlacePattern,
            Target::ClosePatterns => Action::ClosePatterns,
            Target::CloseHelp => Action::CloseHelp,
        };
        self.apply(action);
        EventResult::Consumed
    }

    /// Remember the size the window is being drawn at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }
}

impl Default for LifeApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

impl LifeApp {
    /// The frame for a window of the given size, hit boxes and all.
    ///
    /// The drawing pass is what records the hit boxes, so a cell is clickable
    /// exactly where it was drawn and the two cannot drift apart.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height, self.grid.cols(), self.grid.rows());
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, CRUST, 0.0);
        self.draw_board(&mut f, &l);
        self.draw_header(&mut f, &l);
        self.draw_controls(&mut f, &l);
        if matches!(self.view, View::PatternMenu) {
            self.draw_sheet(&mut f, &l);
        }
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        fill(f, l.header, MANTLE, 0.0);
        let size = l.big.min(l.header.h * 0.7);
        let y = l.header.y + (l.header.h - text::line_height(size, FontWeightHint::Bold)) / 2.0;
        let title_w = (l.header.w * 0.22)
            .min(text::measure("Game of Life", size, FontWeightHint::Bold) + l.pad);
        label(
            f,
            l.header.x + l.pad,
            y,
            "Game of Life",
            size,
            TEXT_COLOR,
            FontWeightHint::Bold,
            Some((title_w - l.pad).max(0.0)),
        );

        // The remaining width, shared equally. Every string is bounded by its
        // own slot and ellipsised inside it, which is what stops the last one
        // running off the edge of a narrow window as the fixed offsets did.
        let fields: [(String, Color); 5] = [
            (
                if self.running { "Running" } else { "Paused" }.to_string(),
                if self.running { GREEN } else { YELLOW },
            ),
            (format!("Gen {}", self.generation), SUBTEXT0),
            (format!("Pop {}", self.grid.population()), SUBTEXT0),
            (format!("Speed {}", self.speed), SUBTEXT0),
            (
                format!("{}x{}", self.grid.cols(), self.grid.rows()),
                OVERLAY0,
            ),
        ];
        let rest = (l.header.w - title_w - l.pad).max(0.0);
        let slot = rest / fields.len() as f32;
        let small = (size * 0.8).min(l.font);
        let sy =
            l.header.y + (l.header.h - text::line_height(small, FontWeightHint::Regular)) / 2.0;
        for (i, (line, color)) in fields.iter().enumerate() {
            label(
                f,
                l.header.x + title_w + slot * i as f32,
                sy,
                line,
                small,
                *color,
                FontWeightHint::Regular,
                Some((slot - l.pad).max(0.0)),
            );
        }
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.cell <= 0.0 {
            return;
        }
        fill(f, l.board, MANTLE, 0.0);
        // Big enough to see the gap between cells, and to be worth stroking a
        // grid line at all. Below this the lines are more ink than the cells.
        let gap = if l.cell >= 4.0 { 0.5 } else { 0.0 };
        for row in 0..self.grid.rows() {
            for col in 0..self.grid.cols() {
                let r = l.cell_rect(row, col);
                let alive = self.grid.get(row, col);
                let is_cursor = row == self.cursor_row && col == self.cursor_col;
                if alive {
                    let color = if is_cursor { LAVENDER } else { GREEN };
                    fill(
                        f,
                        Rect::new(r.x, r.y, (r.w - gap).max(0.0), (r.h - gap).max(0.0)),
                        color,
                        0.0,
                    );
                } else if is_cursor {
                    fill(
                        f,
                        Rect::new(r.x, r.y, (r.w - gap).max(0.0), (r.h - gap).max(0.0)),
                        SURFACE1,
                        0.0,
                    );
                }
                if self.show_grid && l.cell >= 4.0 {
                    stroke(f, r, SURFACE0, 0.5, 0.0);
                }
                // Recorded whatever the cell looks like: a dead cell is exactly
                // the one you most want to click.
                if let Some(i) = self.grid.index(row, col) {
                    f.hit(Target::Cell(i), r);
                }
            }
        }
    }

    fn draw_controls(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.controls) {
            return;
        }
        let buttons: [(Target, String, Color); 9] = [
            (
                Target::PlayPause,
                if self.running { "Pause" } else { "Play" }.to_string(),
                if self.running { YELLOW } else { GREEN },
            ),
            (Target::StepOnce, "Step".to_string(), BLUE),
            (Target::Clear, "Clear".to_string(), SUBTEXT0),
            (Target::Randomize, "Random".to_string(), SUBTEXT0),
            (Target::Patterns, "Patterns".to_string(), BLUE),
            (
                Target::GridLines,
                "Grid".to_string(),
                if self.show_grid { BLUE } else { OVERLAY0 },
            ),
            (Target::Slower, "-".to_string(), SUBTEXT0),
            (Target::Faster, "+".to_string(), SUBTEXT0),
            (Target::Help, "Help".to_string(), SUBTEXT0),
        ];
        let n = buttons.len() as f32;
        let gap = (l.pad * 0.6).min(6.0);
        let each = ((l.controls.w - l.pad * 2.0 - gap * (n - 1.0)) / n).max(0.0);
        let h = (l.controls.h - l.pad).max(0.0);
        for (i, (target, text_str, color)) in buttons.iter().enumerate() {
            let r = Rect::new(
                l.controls.x + l.pad + (each + gap) * i as f32,
                l.controls.y + (l.controls.h - h) / 2.0,
                each,
                h,
            );
            button(f, l, r, text_str, SURFACE0, *color);
            f.hit(*target, r);
        }
    }

    fn draw_sheet(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.window, Color::rgba(0, 0, 0, 180), 0.0);
        // First, so that every box recorded below it wins. `hit_test` takes the
        // last box at a point, which is what makes a modal backdrop and the
        // things on top of it both work with no special case in the handler.
        f.hit(Target::ClosePatterns, l.window);

        let sheet = l.sheet;
        fill(f, sheet, MANTLE, 12.0);
        stroke(f, sheet, SURFACE1, 1.0, 12.0);
        let pad = l.pad * 1.5;
        let title = Rect::new(sheet.x, sheet.y + pad, sheet.w, l.big * 1.4);
        centred_in(
            f,
            title,
            "Place a pattern",
            l.big.min(title.h * 0.8),
            YELLOW,
            FontWeightHint::Bold,
        );

        let foot_h = l.font * 2.4;
        let list = Rect::new(
            sheet.x + pad,
            title.bottom() + pad * 0.5,
            (sheet.w - pad * 2.0).max(0.0),
            (sheet.bottom() - title.bottom() - pad * 1.5 - foot_h).max(0.0),
        );
        let rows = Pattern::ALL.len().max(1) as f32;
        let row_h = (list.h / rows).min(l.font * 2.2);
        for (i, pattern) in Pattern::ALL.iter().enumerate() {
            let r = Rect::new(list.x, list.y + row_h * i as f32, list.w, row_h);
            if r.bottom() > list.bottom() + 0.5 {
                break;
            }
            let selected = i == self.selected_pattern;
            if selected {
                fill(f, r, SURFACE0, 4.0);
            }
            let size = (row_h * 0.55).clamp(6.0, l.font);
            label(
                f,
                r.x + l.pad,
                r.y + (r.h - text::line_height(size, FontWeightHint::Regular)) / 2.0,
                pattern.name(),
                size,
                if selected { BLUE } else { SUBTEXT0 },
                if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                Some((r.w - l.pad * 2.0).max(0.0)),
            );
            f.hit(Target::PatternRow(i), r);
        }

        let foot = Rect::new(
            sheet.x + pad,
            (sheet.bottom() - pad - foot_h).max(sheet.y),
            (sheet.w - pad * 2.0).max(0.0),
            foot_h,
        );
        let half = ((foot.w - l.pad) / 2.0).max(0.0);
        let place = Rect::new(foot.x, foot.y, half, foot.h);
        let cancel = Rect::new(foot.x + half + l.pad, foot.y, half, foot.h);
        button(f, l, place, "Place", SURFACE1, GREEN);
        f.hit(Target::PlacePattern, place);
        button(f, l, cancel, "Cancel", SURFACE1, SUBTEXT0);
        f.hit(Target::ClosePatterns, cancel);
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.window, Color::rgba(0, 0, 0, 190), 0.0);
        // The whole window closes it: there is nothing on the sheet to press.
        f.hit(Target::CloseHelp, l.window);

        let sheet = l.help;
        fill(f, sheet, MANTLE, 12.0);
        stroke(f, sheet, SURFACE1, 1.0, 12.0);
        let pad = l.pad * 1.5;
        let slots = HELP_ROWS.len().saturating_add(3) as f32;
        let size = l.font.min((sheet.h / slots) * 0.8);
        let line_h = text::line_height(size, FontWeightHint::Regular).max(size);
        label(
            f,
            sheet.x + pad,
            sheet.y + pad,
            "Controls",
            l.big.min(sheet.h * 0.12),
            YELLOW,
            FontWeightHint::Bold,
            Some((sheet.w - pad * 2.0).max(0.0)),
        );
        let key_w = (sheet.w * 0.35).max(0.0);
        for (i, (key, what)) in HELP_ROWS.iter().enumerate() {
            let y = sheet.y + pad * 2.0 + line_h * (i.saturating_add(1)) as f32;
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
    let line_h = text::line_height(size, weight);
    // Centring moves the start left, so the width to fit in has to be measured
    // from the start that was actually chosen — not from the box's. Passing the
    // box's whole width from a start half a box to its left puts the ellipsis
    // point half a box past the box's right edge, which is a promise to clip
    // that clips nothing: the last control's label was free to run to 922 in a
    // 900-wide window. And a string too long to centre must start *at* the box
    // rather than left of it, or the ellipsis trims the end of a string whose
    // beginning has already fallen off the other side.
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

/// A filled, labelled control.
fn button(f: &mut Frame, l: &Layout, r: Rect, text_str: &str, back: Color, fore: Color) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    fill(f, r, back, (r.h * 0.25).min(8.0));
    let size = (r.h * 0.45).clamp(6.0, l.font);
    centred_in(f, r, text_str, size, fore, FontWeightHint::Bold);
}

// ── Window ─────────────────────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a key does
/// in a test is what it does on a screen.
pub fn handle_event(app: &mut LifeApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Tick { elapsed_ms } => app.tick(*elapsed_ms),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for LifeApp {
    fn title(&self) -> String {
        "Game of Life".to_string()
    }

    fn app_id(&self) -> String {
        "life".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Asked after every event, so the clock starts the moment the board is
    /// set running and stops the moment it is paused, a sheet is opened, or
    /// the window is closed. An app that leaves this at the default gets no
    /// ticks at all — which is what this one did, with a correct tick handler
    /// waiting on the other side of it.
    fn tick_interval(&self) -> Option<Duration> {
        self.clock_running()
            .then(|| Duration::from_millis(self.speed_ms()))
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

impl Probe for LifeApp {
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
    let mut app = LifeApp::new();
    app::launch("life", &mut app)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    // ── Harness ────────────────────────────────────────────────────────────

    fn app() -> LifeApp {
        LifeApp::with_seed(0x00C0_FFEE)
    }

    /// A board with nothing on it and the cursor parked in a corner, so that no
    /// test has to work around either the opening gun or the cursor highlight.
    fn empty() -> LifeApp {
        let mut a = app();
        a.apply(Action::Clear);
        a.apply(Action::ToggleCell(0));
        a.apply(Action::ToggleCell(0));
        a
    }

    fn press(a: &mut LifeApp, k: Key) -> EventResult {
        probe::key(a, &probe::press(k))
    }

    fn release(a: &mut LifeApp, k: Key) -> EventResult {
        probe::key(
            a,
            &KeyEvent {
                key: k,
                pressed: false,
                modifiers: Modifiers::NONE,
                text: String::new(),
            },
        )
    }

    /// A whole keystroke as a real keyboard sends one: down, then up.
    ///
    /// Every test that means "the user pressed a key" uses this rather than
    /// `press` alone, because the release is the half that used to run the
    /// action a second time. A suite that only ever sent presses would be green
    /// against the exact program this one was.
    fn stroke(a: &mut LifeApp, k: Key) {
        press(a, k);
        release(a, k);
    }

    fn tick(a: &mut LifeApp, ms: u64) -> EventResult {
        handle_event(a, &Event::Tick { elapsed_ms: ms })
    }

    fn frame_of(a: &LifeApp) -> Frame {
        a.frame(LifeApp::SIZE.0, LifeApp::SIZE.1)
    }

    fn fills(f: &Frame) -> Vec<(Rect, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => Some((Rect::new(x, y, width, height), color)),
                _ => None,
            })
            .collect()
    }

    fn strokes(f: &Frame) -> Vec<(Rect, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => Some((Rect::new(x, y, width, height), color)),
                _ => None,
            })
            .collect()
    }

    /// Every text command as `(x, y, text, max_width, size)`.
    fn texts(f: &Frame) -> Vec<(f32, f32, String, Option<f32>, f32)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    max_width,
                    font_size,
                    ..
                } => Some((*x, *y, text.clone(), *max_width, *font_size)),
                _ => None,
            })
            .collect()
    }

    /// Everything about the program a player could tell apart.
    ///
    /// Compared whole, so a test that means "nothing happened" asserts nothing
    /// happened rather than nothing happened *to the one field it thought to
    /// look at*.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Snap {
        grid: Grid,
        view: View,
        running: bool,
        generation: u64,
        speed: u32,
        cursor: (usize, usize),
        selected: usize,
        show_grid: bool,
        show_help: bool,
    }

    fn snap(a: &LifeApp) -> Snap {
        Snap {
            grid: a.grid().clone(),
            view: a.view(),
            running: a.running(),
            generation: a.generation(),
            speed: a.speed(),
            cursor: a.cursor(),
            selected: a.selected_pattern(),
            show_grid: a.show_grid(),
            show_help: a.show_help(),
        }
    }

    /// The live cells of a board, as a sorted list of coordinates.
    fn live(g: &Grid) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for row in 0..g.rows() {
            for col in 0..g.cols() {
                if g.get(row, col) {
                    out.push((row, col));
                }
            }
        }
        out
    }

    fn set_cells(a: &mut LifeApp, cells: &[(usize, usize)]) {
        for &(r, c) in cells {
            a.grid.set(r, c, true);
        }
    }

    fn about(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    // ── The rules of the game ──────────────────────────────────────────────

    #[test]
    fn a_blinker_oscillates_with_period_two() {
        let mut a = empty();
        set_cells(&mut a, &[(20, 20), (20, 21), (20, 22)]);
        let start = live(a.grid());
        a.apply(Action::StepOnce);
        assert_eq!(
            live(a.grid()),
            vec![(19, 21), (20, 21), (21, 21)],
            "a horizontal blinker must become a vertical one"
        );
        a.apply(Action::StepOnce);
        assert_eq!(
            live(a.grid()),
            start,
            "and back again on the next generation"
        );
    }

    #[test]
    fn a_block_never_changes() {
        let mut a = empty();
        let block = [(30, 30), (30, 31), (31, 30), (31, 31)];
        set_cells(&mut a, &block);
        for generation in 0..8 {
            a.apply(Action::StepOnce);
            assert_eq!(
                live(a.grid()),
                block.to_vec(),
                "a block is a still life; it moved at generation {generation}"
            );
        }
    }

    #[test]
    fn a_glider_walks_one_cell_diagonally_every_four_generations() {
        let mut a = empty();
        let start = [(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)];
        set_cells(&mut a, &start);
        for _ in 0..4 {
            a.apply(Action::StepOnce);
        }
        let want: Vec<(usize, usize)> = start.iter().map(|&(r, c)| (r + 1, c + 1)).collect();
        assert_eq!(
            live(a.grid()),
            want,
            "four generations must move a glider exactly one cell down and right"
        );
    }

    #[test]
    fn a_cell_never_counts_itself_as_its_own_neighbour() {
        let mut a = empty();
        a.grid.set(25, 25, true);
        assert_eq!(
            a.grid().neighbours(25, 25),
            0,
            "the only live cell on the board counted itself"
        );
        for (dr, dc) in [
            (-1i32, -1i32),
            (-1, 0),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, -1),
            (1, 0),
            (1, 1),
        ] {
            let r = (25 + dr) as usize;
            let c = (25 + dc) as usize;
            assert_eq!(
                a.grid().neighbours(r, c),
                1,
                "({r}, {c}) is next to the live cell and should see exactly one"
            );
        }
    }

    #[test]
    fn birth_and_survival_follow_b3_s23_for_every_neighbour_count() {
        // The eight neighbours of (30, 30), far enough from each other's
        // neighbourhoods that turning them on only changes the centre's count.
        let ring = [
            (29usize, 29usize),
            (29, 30),
            (29, 31),
            (30, 29),
            (30, 31),
            (31, 29),
            (31, 30),
            (31, 31),
        ];
        for n in 0..=8usize {
            for centre_alive in [false, true] {
                let mut a = empty();
                a.grid.set(30, 30, centre_alive);
                for &(r, c) in ring.iter().take(n) {
                    a.grid.set(r, c, true);
                }
                assert_eq!(
                    a.grid().neighbours(30, 30),
                    n as u8,
                    "ring of {n} miscounted"
                );
                a.apply(Action::StepOnce);
                let want = if centre_alive {
                    n == 2 || n == 3
                } else {
                    n == 3
                };
                assert_eq!(
                    a.grid().get(30, 30),
                    want,
                    "a {} cell with {n} neighbours",
                    if centre_alive { "live" } else { "dead" }
                );
            }
        }
    }

    #[test]
    fn the_board_wraps_at_every_edge() {
        let g = Grid::new(GRID_COLS, GRID_ROWS);
        let mut g = g;
        // The four corners. Each is a neighbour of every other on a torus.
        for &(r, c) in &[
            (0usize, 0usize),
            (0, GRID_COLS - 1),
            (GRID_ROWS - 1, 0),
            (GRID_ROWS - 1, GRID_COLS - 1),
        ] {
            g.set(r, c, true);
        }
        for &(r, c) in &[
            (0usize, 0usize),
            (0, GRID_COLS - 1),
            (GRID_ROWS - 1, 0),
            (GRID_ROWS - 1, GRID_COLS - 1),
        ] {
            assert_eq!(
                g.neighbours(r, c),
                3,
                "on a torus each corner touches the other three"
            );
        }
    }

    #[test]
    fn a_blinker_astride_the_edge_still_oscillates() {
        let mut a = empty();
        let last = GRID_COLS - 1;
        set_cells(&mut a, &[(5, last - 1), (5, last), (5, 0)]);
        a.apply(Action::StepOnce);
        assert_eq!(
            live(a.grid()),
            vec![(4, last), (5, last), (6, last)],
            "a blinker wrapped round the right edge must turn about its middle"
        );
    }

    #[test]
    fn an_index_and_a_coordinate_pair_say_the_same_thing() {
        let g = Grid::new(GRID_COLS, GRID_ROWS);
        for row in 0..g.rows() {
            for col in 0..g.cols() {
                let i = g.index(row, col).expect("every cell has an index");
                assert_eq!(
                    g.coords(i),
                    Some((row, col)),
                    "index {i} did not name ({row}, {col}) again"
                );
            }
        }
        assert_eq!(
            g.index(GRID_ROWS, 0),
            None,
            "a row past the last is no cell"
        );
        assert_eq!(
            g.index(0, GRID_COLS),
            None,
            "a column past the last is no cell"
        );
        assert_eq!(
            g.coords(GRID_ROWS * GRID_COLS),
            None,
            "an index one past the board names no cell"
        );
    }

    #[test]
    fn population_counts_the_live_cells_and_clear_removes_them() {
        let mut a = empty();
        assert_eq!(a.grid().population(), 0);
        set_cells(&mut a, &[(1, 1), (2, 2), (3, 3)]);
        assert_eq!(a.grid().population(), 3);
        a.apply(Action::Clear);
        assert_eq!(a.grid().population(), 0);
    }

    // ── Patterns ───────────────────────────────────────────────────────────

    #[test]
    fn every_pattern_is_a_named_set_of_distinct_forward_offsets() {
        for &p in Pattern::ALL {
            let cells = p.cells();
            assert!(!p.name().is_empty(), "{p:?} has no name");
            assert!(cells.len() >= 3, "{p:?} has only {} cells", cells.len());
            let mut sorted = cells.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), cells.len(), "{p:?} repeats a cell");
            for &(r, c) in &cells {
                assert!(
                    r >= 0 && c >= 0,
                    "{p:?} has the negative offset ({r}, {c}); patterns grow down and right \
                     from where they are placed"
                );
            }
        }
    }

    #[test]
    fn every_pattern_has_its_own_name() {
        let mut names: Vec<&str> = Pattern::ALL.iter().map(|p| p.name()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two patterns share a name");
    }

    #[test]
    fn the_pulsar_is_symmetric_about_its_middle() {
        let cells = Pattern::Pulsar.cells();
        assert_eq!(cells.len(), 48, "a pulsar has 48 live cells");
        for &(r, c) in &cells {
            for mirrored in [(r, 12 - c), (12 - r, c), (12 - r, 12 - c)] {
                assert!(
                    cells.contains(&mirrored),
                    "({r}, {c}) has no mirror at {mirrored:?}"
                );
            }
        }
    }

    /// The number of generations after which a pattern is itself again, up to
    /// `limit`, or `None` if it never is.
    fn period(p: Pattern, limit: usize) -> Option<usize> {
        let mut a = empty();
        a.grid.place(p, 22, 30);
        let start = live(a.grid());
        for n in 1..=limit {
            a.apply(Action::StepOnce);
            if live(a.grid()) == start {
                return Some(n);
            }
        }
        None
    }

    #[test]
    fn every_pattern_named_for_an_oscillator_oscillates_at_its_own_period() {
        // The periods are what the names mean. A "Pulsar" that settles into a
        // pair of blocks is not a pulsar however many cells it has, and this is
        // the assertion that says so: the figure this file shipped with was
        // stable after three generations and never returned to itself.
        for (p, want) in [
            (Pattern::Blinker, 2usize),
            (Pattern::Toad, 2),
            (Pattern::Beacon, 2),
            (Pattern::Pulsar, 3),
        ] {
            assert_eq!(
                period(p, 40),
                Some(want),
                "{p:?} should be its own self again every {want} generations"
            );
        }
    }

    #[test]
    fn the_lightweight_spaceship_travels_two_cells_every_four_generations() {
        let mut a = empty();
        let p = Pattern::Lwss;
        a.grid.place(p, 22, 30);
        let start = live(a.grid());
        for _ in 0..4 {
            a.apply(Action::StepOnce);
        }
        let now = live(a.grid());
        assert_eq!(
            now.len(),
            start.len(),
            "a spaceship keeps its own cell count"
        );
        let dr = now[0].0 as i32 - start[0].0 as i32;
        let dc = now[0].1 as i32 - start[0].1 as i32;
        let want: Vec<(usize, usize)> = start
            .iter()
            .map(|&(r, c)| ((r as i32 + dr) as usize, (c as i32 + dc) as usize))
            .collect();
        assert_eq!(
            now, want,
            "an LWSS must be its own shape moved, not a new shape"
        );
        assert_eq!(
            (dr, dc.abs()),
            (0, 2),
            "an LWSS is an orthogonal c/2 spaceship: two cells sideways, none down"
        );
    }

    #[test]
    fn the_diehard_dies_hard() {
        // The pattern is named for the fact that it takes 130 generations to
        // vanish and then leaves nothing at all. A "Diehard" that stabilises
        // into a still life, or that dies at generation 12, is some other
        // pattern wearing the name.
        let mut a = empty();
        a.grid.place(Pattern::Diehard, 25, 35);
        for _ in 0..129 {
            a.apply(Action::StepOnce);
        }
        assert!(
            a.grid().population() > 0,
            "the diehard was gone before its 130th generation"
        );
        a.apply(Action::StepOnce);
        assert_eq!(
            a.grid().population(),
            0,
            "the diehard should leave an empty board at generation 130"
        );
    }

    #[test]
    fn the_acorn_grows_from_seven_cells_into_something_much_larger() {
        let mut a = empty();
        a.grid.place(Pattern::Acorn, 28, 38);
        for _ in 0..200 {
            a.apply(Action::StepOnce);
        }
        assert!(
            a.grid().population() > 50,
            "an acorn is a methuselah; after 200 generations it had only {} cells",
            a.grid().population()
        );
    }

    #[test]
    fn placing_a_pattern_puts_exactly_its_own_cells_on_the_board() {
        for &p in Pattern::ALL {
            let mut a = empty();
            a.grid.place(p, 20, 20);
            let want: Vec<(usize, usize)> = {
                let mut v: Vec<(usize, usize)> = p
                    .cells()
                    .into_iter()
                    .map(|(dr, dc)| ((20 + dr) as usize, (20 + dc) as usize))
                    .collect();
                v.sort_unstable();
                v
            };
            assert_eq!(
                live(a.grid()),
                want,
                "{p:?} did not land where it says it does"
            );
        }
    }

    #[test]
    fn a_pattern_placed_at_the_far_corner_wraps_instead_of_being_clipped() {
        let mut a = empty();
        let p = Pattern::Glider;
        a.grid.place(p, GRID_ROWS - 1, GRID_COLS - 1);
        assert_eq!(
            a.grid().population(),
            p.cells().len(),
            "a pattern at the corner lost cells off the edge instead of wrapping"
        );
        for (dr, dc) in p.cells() {
            let r = (GRID_ROWS - 1 + dr as usize) % GRID_ROWS;
            let c = (GRID_COLS - 1 + dc as usize) % GRID_COLS;
            assert!(
                a.grid().get(r, c),
                "the cell that wrapped to ({r}, {c}) is not there"
            );
        }
    }

    #[test]
    fn the_opening_board_is_a_gun_and_the_gun_fires() {
        let mut a = app();
        let opening = a.grid().population();
        assert_eq!(
            opening,
            Pattern::GosperGun.cells().len(),
            "the program should open on a Gosper gun and nothing else"
        );
        for _ in 0..120 {
            a.apply(Action::StepOnce);
        }
        assert!(
            a.grid().population() > opening,
            "after 120 generations a glider gun should have made gliders; \
             population went {opening} -> {}",
            a.grid().population()
        );
    }

    #[test]
    fn the_opening_gun_is_not_under_the_cursor() {
        // A gun drawn under the cursor highlight is a gun whose first cell is
        // drawn in the cursor's colour, which no test of "a live cell is green"
        // could then see.
        let a = app();
        let (r, c) = a.cursor();
        assert!(
            !a.grid().get(r, c),
            "the opening board puts a live cell under the cursor at ({r}, {c})"
        );
    }

    // ── Keys: the press does it, the release does not ──────────────────────

    /// Every key the program answers on the board, grouped as the help sheet
    /// groups them. Walked in both directions by
    /// [`the_help_sheet_names_every_key_the_program_answers`].
    const BOARD_KEYS: [(&str, &[Key]); 11] = [
        ("Space", &[Key::Space]),
        ("S", &[Key::S]),
        ("C", &[Key::C]),
        ("R", &[Key::R]),
        ("P", &[Key::P]),
        ("G", &[Key::G]),
        ("Enter", &[Key::Enter]),
        ("Arrows", &[Key::Up, Key::Down, Key::Left, Key::Right]),
        (
            "1 - 9",
            &[
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ],
        ),
        ("- and +", &[Key::Minus, Key::Equals]),
        ("F1 / Esc", &[Key::F1]),
    ];

    fn every_answered_key() -> Vec<Key> {
        BOARD_KEYS
            .iter()
            .flat_map(|(_, ks)| ks.iter().copied())
            .collect()
    }

    #[test]
    fn a_key_release_on_its_own_changes_nothing() {
        // This is the fault the whole file turned on. Both key handlers
        // destructured `KeyEvent { key, .. }`, so the release ran the action a
        // second time -- which for Space meant the program started and stopped
        // between one frame and the next, and for Enter meant a cell was
        // flipped and flipped back.
        for key in every_answered_key() {
            let mut a = app();
            let before = snap(&a);
            assert_eq!(
                release(&mut a, key),
                EventResult::Ignored,
                "the release of {key:?} was answered"
            );
            assert_eq!(
                snap(&a),
                before,
                "the release of {key:?} changed the program"
            );
        }
    }

    #[test]
    fn space_runs_and_pauses_once_per_whole_keystroke() {
        let mut a = app();
        assert!(!a.running(), "the program should open paused");
        stroke(&mut a, Key::Space);
        assert!(
            a.running(),
            "a press and release of Space must leave it running"
        );
        stroke(&mut a, Key::Space);
        assert!(!a.running(), "and the next one must leave it paused");
    }

    #[test]
    fn enter_flips_the_cell_under_the_cursor_once_per_keystroke() {
        let mut a = empty();
        let (r, c) = a.cursor();
        assert!(!a.grid().get(r, c));
        stroke(&mut a, Key::Enter);
        assert!(
            a.grid().get(r, c),
            "a whole keystroke on Enter must leave the cell flipped, not flipped twice"
        );
        stroke(&mut a, Key::Enter);
        assert!(!a.grid().get(r, c), "and the next one must flip it back");
    }

    #[test]
    fn s_advances_exactly_one_generation_per_keystroke() {
        let mut a = app();
        for want in 1..=4u64 {
            stroke(&mut a, Key::S);
            assert_eq!(
                a.generation(),
                want,
                "a single step must be a single generation"
            );
        }
    }

    #[test]
    fn a_step_while_running_pauses_first_and_then_takes_one() {
        let mut a = app();
        stroke(&mut a, Key::Space);
        assert!(a.running());
        stroke(&mut a, Key::S);
        assert!(
            !a.running(),
            "asking for one step must stop the clock taking more"
        );
        assert_eq!(a.generation(), 1, "and must itself take exactly one");
    }

    #[test]
    fn each_arrow_key_moves_the_cursor_exactly_one_cell() {
        for (key, dr, dc) in [
            (Key::Up, -1i32, 0i32),
            (Key::Down, 1, 0),
            (Key::Left, 0, -1),
            (Key::Right, 0, 1),
        ] {
            let mut a = app();
            let (r0, c0) = a.cursor();
            stroke(&mut a, key);
            assert_eq!(
                a.cursor(),
                ((r0 as i32 + dr) as usize, (c0 as i32 + dc) as usize),
                "{key:?} should move the cursor by ({dr}, {dc}) and no further"
            );
        }
    }

    #[test]
    fn the_cursor_wraps_at_every_edge_as_the_board_does() {
        let mut a = app();
        // Walk to the top-left corner, then one step further in each direction.
        for _ in 0..GRID_ROWS {
            stroke(&mut a, Key::Up);
        }
        for _ in 0..GRID_COLS {
            stroke(&mut a, Key::Left);
        }
        assert_eq!(
            a.cursor(),
            (GRID_ROWS / 2, GRID_COLS / 2),
            "a full lap comes home"
        );
        for _ in 0..GRID_ROWS / 2 {
            stroke(&mut a, Key::Up);
        }
        assert_eq!(a.cursor().0, 0, "the cursor should now be on the top row");
        stroke(&mut a, Key::Up);
        assert_eq!(
            a.cursor().0,
            GRID_ROWS - 1,
            "past the top row the cursor wraps to the bottom"
        );
        for _ in 0..GRID_COLS / 2 {
            stroke(&mut a, Key::Left);
        }
        assert_eq!(a.cursor().1, 0);
        stroke(&mut a, Key::Left);
        assert_eq!(
            a.cursor().1,
            GRID_COLS - 1,
            "past the first column the cursor wraps to the last"
        );
    }

    #[test]
    fn placing_a_pattern_does_not_flip_one_of_its_own_cells_back_off() {
        // The old program's `Enter` in the menu placed the pattern *and* set
        // the view back to the board, so the key's release was routed to the
        // board's handler, whose `Enter` toggles the cell at the cursor -- the
        // very cell the pattern was centred on. Every pattern with a (0, 0) of
        // its own arrived one cell short.
        for (i, &p) in Pattern::ALL.iter().enumerate() {
            let mut a = empty();
            let (r, c) = a.cursor();
            stroke(&mut a, Key::P);
            assert_eq!(a.view(), View::PatternMenu);
            a.apply(Action::SelectPattern(i));
            stroke(&mut a, Key::Enter);
            assert_eq!(
                a.view(),
                View::Board,
                "placing a pattern returns to the board"
            );
            assert_eq!(
                a.grid().population(),
                p.cells().len(),
                "{p:?} arrived with the wrong number of cells"
            );
            assert!(
                a.grid().get(r, c) == p.cells().contains(&(0, 0)),
                "{p:?}'s own cell at its origin was flipped by the keystroke that placed it"
            );
        }
    }

    #[test]
    fn a_modifier_held_down_refuses_the_key_in_both_views() {
        // `handle_main_event` returned early on ctrl and `handle_pattern_event`
        // did not, so Ctrl-Enter placed a pattern while Ctrl-C did nothing.
        for view_key in [None, Some(Key::P)] {
            for key in every_answered_key().into_iter().chain([Key::Escape]) {
                for mods in [
                    Modifiers {
                        ctrl: true,
                        ..Modifiers::NONE
                    },
                    Modifiers {
                        alt: true,
                        ..Modifiers::NONE
                    },
                    Modifiers {
                        super_key: true,
                        ..Modifiers::NONE
                    },
                ] {
                    let mut a = app();
                    if let Some(k) = view_key {
                        stroke(&mut a, k);
                    }
                    let before = snap(&a);
                    let got = probe::key(&mut a, &probe::press_with(key, mods));
                    assert_eq!(
                        got,
                        EventResult::Ignored,
                        "{key:?} with {mods:?} was answered in {:?}",
                        before.view
                    );
                    assert_eq!(
                        snap(&a),
                        before,
                        "{key:?} with {mods:?} changed the program in {:?}",
                        before.view
                    );
                }
            }
        }
    }

    #[test]
    fn shift_is_not_a_modifier_that_refuses_a_key() {
        // Shift is how a keyboard produces `+`, so refusing it would refuse the
        // faster key on every layout that has `+` over `=`.
        let mut a = app();
        assert_eq!(
            probe::key(&mut a, &probe::shift(Key::Equals)),
            EventResult::Consumed,
            "Shift-Equals is how `+` is typed"
        );
        assert_eq!(a.speed(), 6);
    }

    #[test]
    fn a_key_the_program_does_not_answer_is_left_alone() {
        for key in [
            Key::A,
            Key::Z,
            Key::Tab,
            Key::Backspace,
            Key::Delete,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::F2,
            Key::Num0,
            Key::Comma,
        ] {
            let mut a = app();
            let before = snap(&a);
            assert_eq!(
                press(&mut a, key),
                EventResult::Ignored,
                "{key:?} is not a key this program answers"
            );
            assert_eq!(snap(&a), before, "{key:?} changed the program anyway");
        }
    }

    #[test]
    fn the_help_sheet_names_every_key_the_program_answers() {
        assert_eq!(
            HELP_ROWS.len(),
            BOARD_KEYS.len(),
            "the help sheet and the key table have drifted apart"
        );
        for (i, (label, keys)) in BOARD_KEYS.iter().enumerate() {
            assert_eq!(
                HELP_ROWS[i].0, *label,
                "help row {i} names a different key group than the table does"
            );
            assert!(
                !HELP_ROWS[i].1.is_empty(),
                "help row {i} says nothing about what {label} does"
            );
            for &key in *keys {
                let a = app();
                assert!(
                    a.key_action(key).is_some(),
                    "the help sheet names {key:?} under {label} and the program ignores it"
                );
            }
        }
        // And the other direction: Escape is the only key that answers on the
        // board without a row of its own, and it shares one with F1.
        assert_eq!(
            HELP_ROWS[10].0, "F1 / Esc",
            "Escape must be named somewhere, and it shares F1's row"
        );
    }

    // ── Speed ──────────────────────────────────────────────────────────────

    #[test]
    fn every_number_key_sets_its_own_speed() {
        for (n, key) in [
            (1u32, Key::Num1),
            (2, Key::Num2),
            (3, Key::Num3),
            (4, Key::Num4),
            (5, Key::Num5),
            (6, Key::Num6),
            (7, Key::Num7),
            (8, Key::Num8),
            (9, Key::Num9),
        ] {
            let mut a = app();
            stroke(&mut a, key);
            assert_eq!(a.speed(), n, "{key:?} should set speed {n}");
        }
    }

    #[test]
    fn minus_and_plus_move_the_speed_by_one_and_stop_at_the_ends() {
        let mut a = app();
        assert_eq!(a.speed(), 5, "the program opens at the middle speed");
        stroke(&mut a, Key::Minus);
        assert_eq!(a.speed(), 4);
        stroke(&mut a, Key::Equals);
        assert_eq!(a.speed(), 5);
        for _ in 0..20 {
            stroke(&mut a, Key::Minus);
        }
        assert_eq!(a.speed(), 1, "the speed must not fall below one");
        for _ in 0..20 {
            stroke(&mut a, Key::Equals);
        }
        assert_eq!(a.speed(), 9, "the speed must not rise above nine");
    }

    #[test]
    fn a_higher_speed_is_a_shorter_interval() {
        let mut previous = u64::MAX;
        for n in 1..=9u32 {
            let mut a = app();
            a.apply(Action::SetSpeed(n));
            let ms = a.speed_ms();
            assert!(
                ms > 0,
                "speed {n} asks for an interval of zero milliseconds"
            );
            assert!(
                ms < previous,
                "speed {n} is not faster than speed {}: {ms}ms vs {previous}ms",
                n - 1
            );
            previous = ms;
        }
    }

    // ── The clock ──────────────────────────────────────────────────────────

    #[test]
    fn the_clock_is_asked_for_exactly_when_it_is_wanted() {
        // An app that leaves `tick_interval` at its default gets no ticks at
        // all, however correct its tick handler is -- which is what this
        // program was. `known-issues.md` lesson 47.
        let mut a = app();
        assert_eq!(a.tick_interval(), None, "a paused board wants no clock");
        stroke(&mut a, Key::Space);
        assert_eq!(
            a.tick_interval(),
            Some(Duration::from_millis(a.speed_ms())),
            "a running board wants a tick every interval"
        );
        stroke(&mut a, Key::P);
        assert_eq!(
            a.tick_interval(),
            None,
            "opening the pattern menu stops the board, and so the clock"
        );
        stroke(&mut a, Key::Escape);
        stroke(&mut a, Key::Space);
        assert!(a.tick_interval().is_some());
        stroke(&mut a, Key::F1);
        assert_eq!(
            a.tick_interval(),
            None,
            "a board nobody can see should not be running under the help sheet"
        );
    }

    #[test]
    fn the_interval_asked_for_is_the_one_the_speed_names() {
        for n in 1..=9u32 {
            let mut a = app();
            a.apply(Action::SetSpeed(n));
            a.apply(Action::TogglePlay);
            assert_eq!(
                a.tick_interval(),
                Some(Duration::from_millis(a.speed_ms())),
                "speed {n} asked for the wrong interval"
            );
        }
    }

    #[test]
    fn a_tick_advances_by_the_time_that_passed_not_by_the_interval() {
        // The clock hands back the milliseconds that really elapsed, which on a
        // busy machine is not the interval that was asked for. A handler that
        // took one generation per tick would run slow exactly when the machine
        // was already struggling.
        let mut a = app();
        a.apply(Action::SetSpeed(5));
        assert_eq!(a.speed_ms(), 120);
        a.apply(Action::TogglePlay);
        assert_eq!(tick(&mut a, 360), EventResult::Consumed);
        assert_eq!(
            a.generation(),
            3,
            "360ms at 120ms a generation is three generations, not one"
        );
    }

    #[test]
    fn a_tick_shorter_than_the_interval_banks_the_time_rather_than_losing_it() {
        let mut a = app();
        a.apply(Action::SetSpeed(5));
        a.apply(Action::TogglePlay);
        for _ in 0..2 {
            assert_eq!(
                tick(&mut a, 40),
                EventResult::Ignored,
                "a tick that runs no generation changes nothing on screen"
            );
            assert_eq!(a.generation(), 0);
        }
        assert_eq!(tick(&mut a, 40), EventResult::Consumed);
        assert_eq!(
            a.generation(),
            1,
            "three ticks of 40ms are one generation at 120ms"
        );
    }

    #[test]
    fn catching_up_is_capped_and_the_backlog_is_dropped_not_banked() {
        let mut a = app();
        a.apply(Action::SetSpeed(5));
        a.apply(Action::TogglePlay);
        tick(&mut a, 10_000);
        assert_eq!(
            a.generation(),
            u64::from(MAX_CATCHUP),
            "ten seconds of absence must not be run as 83 generations inside one frame"
        );
        assert!(
            a.dropped() > 0,
            "the generations not run should be counted as dropped"
        );
        // The leftover must be under one interval: a capped catch-up that kept
        // the backlog would take another eight generations on the next tick,
        // and the one after that, for as long as it took to work the debt off.
        let before = a.generation();
        tick(&mut a, 1);
        assert_eq!(
            a.generation(),
            before,
            "the dropped backlog was banked and ran on the next tick anyway"
        );
    }

    #[test]
    fn a_tick_while_the_board_is_not_running_does_nothing() {
        for setup in [None, Some(Key::P), Some(Key::F1)] {
            let mut a = app();
            a.apply(Action::TogglePlay);
            if let Some(k) = setup {
                stroke(&mut a, k);
            } else {
                a.apply(Action::TogglePlay);
            }
            let before = snap(&a);
            assert_eq!(
                tick(&mut a, 100_000),
                EventResult::Ignored,
                "a tick arrived and was acted on with the clock stopped ({setup:?})"
            );
            assert_eq!(snap(&a), before);
        }
    }

    #[test]
    fn pausing_forgets_the_part_of_a_generation_that_had_elapsed() {
        let mut a = app();
        a.apply(Action::SetSpeed(5));
        a.apply(Action::TogglePlay);
        tick(&mut a, 100);
        assert_eq!(a.generation(), 0, "100ms is not yet one 120ms generation");
        a.apply(Action::TogglePlay);
        a.apply(Action::TogglePlay);
        tick(&mut a, 100);
        assert_eq!(
            a.generation(),
            0,
            "the 100ms banked before the pause was carried across it, so the first \
             generation after resuming arrived early"
        );
    }

    #[test]
    fn a_step_and_a_clear_also_forget_the_part_generation() {
        for action in [Action::StepOnce, Action::Clear, Action::Randomize] {
            let mut a = app();
            a.apply(Action::SetSpeed(5));
            a.apply(Action::TogglePlay);
            tick(&mut a, 100);
            a.apply(action);
            let at = a.generation();
            a.apply(Action::TogglePlay);
            if !a.running() {
                a.apply(Action::TogglePlay);
            }
            tick(&mut a, 100);
            assert_eq!(
                a.generation(),
                at,
                "{action:?} left 100ms banked, so the next generation came early"
            );
        }
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    /// A spread of window sizes: the default, tall, wide, tiny, and a couple of
    /// awkward ones in between.
    const SIZES: [(f32, f32); 10] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (1920.0, 1080.0),
        (1280.0, 400.0),
        (400.0, 1280.0),
        (640.0, 480.0),
        (320.0, 240.0),
        (200.0, 160.0),
        (120.0, 90.0),
        (60.0, 40.0),
        (1.0, 1.0),
    ];

    fn layout_at(w: f32, h: f32) -> Layout {
        Layout::new(w, h, GRID_COLS, GRID_ROWS)
    }

    #[test]
    fn the_whole_board_is_on_screen_at_every_window_size() {
        // The old program drew 8-pixel cells from a fixed origin and showed
        // whatever fitted, with a viewport that nothing could move. Fitting the
        // world to the window means there is nothing off screen to scroll to.
        for (w, h) in SIZES {
            let l = layout_at(w, h);
            assert!(
                l.board.x >= -0.01
                    && l.board.y >= -0.01
                    && l.board.right() <= w + 0.01
                    && l.board.bottom() <= h + 0.01,
                "at {w}x{h} the board {:?} is not inside the window",
                l.board
            );
            let last = l.cell_rect(GRID_ROWS - 1, GRID_COLS - 1);
            assert!(
                last.right() <= l.board.right() + 0.01 && last.bottom() <= l.board.bottom() + 0.01,
                "at {w}x{h} the last cell {last:?} falls outside the board {:?}",
                l.board
            );
        }
    }

    #[test]
    fn the_board_never_overlaps_the_bands_above_and_below_it() {
        for (w, h) in SIZES {
            let l = layout_at(w, h);
            if l.shows(l.header) {
                assert!(
                    l.board.y >= l.header.bottom() - 0.01,
                    "at {w}x{h} the board starts inside the header"
                );
            }
            if l.shows(l.controls) {
                assert!(
                    l.board.bottom() <= l.controls.y + 0.01,
                    "at {w}x{h} the board runs under the controls"
                );
            }
        }
    }

    #[test]
    fn cells_are_square_and_tile_the_board_with_no_gap_and_no_overlap() {
        for (w, h) in SIZES {
            let l = layout_at(w, h);
            if l.cell <= 0.0 {
                continue;
            }
            for (row, col) in [(0usize, 0usize), (7, 13), (GRID_ROWS - 2, GRID_COLS - 2)] {
                let r = l.cell_rect(row, col);
                assert!(
                    about(r.w, r.h, 0.001),
                    "at {w}x{h} the cell at ({row}, {col}) is {}x{}, not square",
                    r.w,
                    r.h
                );
                assert!(
                    about(l.cell_rect(row, col + 1).x, r.right(), 0.001),
                    "at {w}x{h} there is a seam between column {col} and the next"
                );
                assert!(
                    about(l.cell_rect(row + 1, col).y, r.bottom(), 0.001),
                    "at {w}x{h} there is a seam between row {row} and the next"
                );
            }
        }
    }

    #[test]
    fn the_board_is_the_cells_and_nothing_more() {
        for (w, h) in SIZES {
            let l = layout_at(w, h);
            assert!(
                about(l.board.w, l.cell * GRID_COLS as f32, 0.01)
                    && about(l.board.h, l.cell * GRID_ROWS as f32, 0.01),
                "at {w}x{h} the board {:?} is not exactly {GRID_COLS}x{GRID_ROWS} cells of {}",
                l.board,
                l.cell
            );
        }
    }

    #[test]
    fn a_window_too_short_for_everything_drops_the_controls_before_the_header() {
        // Every control is a button for a key that still works without it. The
        // generation count, the population and the run state are the only
        // things on screen that the board itself does not say.
        let mut dropped_controls_first = false;
        for h in (40..900).step_by(5) {
            let l = layout_at(700.0, h as f32);
            if !l.shows(l.controls) && l.shows(l.header) {
                dropped_controls_first = true;
            }
            assert!(
                l.shows(l.header) || !l.shows(l.controls),
                "at 700x{h} the header went and the controls stayed"
            );
        }
        assert!(
            dropped_controls_first,
            "no window height in the range dropped the controls, so the rule is untested"
        );
    }

    #[test]
    fn a_dropped_band_is_empty_rather_than_a_strip_of_no_height() {
        let l = layout_at(700.0, 60.0);
        for band in [l.header, l.controls] {
            assert!(
                l.shows(band) || band == Rect::EMPTY,
                "a band that is not shown should be Rect::EMPTY, not {band:?}"
            );
        }
    }

    #[test]
    fn the_layout_is_read_from_the_window_and_not_remembered() {
        let mut a = app();
        let wide = a.frame(1600.0, 900.0);
        let narrow = a.frame(500.0, 400.0);
        assert_ne!(
            wide.rect_of(|t| matches!(t, Target::Cell(0))),
            narrow.rect_of(|t| matches!(t, Target::Cell(0))),
            "the same cell drew in the same place in two differently-sized windows"
        );
        a.resize(500.0, 400.0);
        assert_eq!(
            a.layout(),
            layout_at(500.0, 400.0),
            "the layout the model reports is not the one its last window had"
        );
    }

    #[test]
    fn the_first_frame_is_drawn_at_the_size_it_is_given() {
        // A real window submits its first frame before any resize event
        // arrives, so a renderer that trusted a remembered size would draw that
        // frame wrong.
        let a = app();
        let f = a.frame(1234.0, 567.0);
        let l = layout_at(1234.0, 567.0);
        assert_eq!(
            f.rect_of(|t| matches!(t, Target::Cell(0))),
            Some(l.cell_rect(0, 0)),
            "the frame ignored the size it was handed"
        );
    }

    // ── Drawing ────────────────────────────────────────────────────────────

    #[test]
    fn the_frame_is_balanced_at_every_size() {
        for (w, h) in SIZES {
            let mut a = app();
            a.apply(Action::OpenPatterns);
            a.apply(Action::ToggleHelp);
            assert!(
                a.frame(w, h).is_balanced(),
                "at {w}x{h} the frame left a clip or a translation open"
            );
        }
    }

    #[test]
    fn nothing_is_drawn_outside_the_window() {
        for (w, h) in SIZES {
            let mut a = app();
            a.apply(Action::OpenPatterns);
            let f = a.frame(w, h);
            for (r, _) in fills(&f).into_iter().chain(strokes(&f)) {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "at {w}x{h} something is drawn at {r:?}, outside the window"
                );
            }
        }
    }

    #[test]
    fn every_string_is_bounded_by_the_space_it_has() {
        // The old header drew six strings at fixed offsets of 12, 160, 260,
        // 400, 530 and 640, every one of them with `max_width: None`. In a
        // 600-pixel window the last two were off the right edge, and being
        // unbounded they could not even be ellipsised into view.
        for (w, h) in SIZES {
            let mut a = app();
            a.apply(Action::ToggleHelp);
            let f = a.frame(w, h);
            for (x, y, s, max_width, size) in texts(&f) {
                let Some(mw) = max_width else {
                    panic!("at {w}x{h} the string {s:?} is drawn with no width to fit in");
                };
                assert!(
                    x + mw <= w + 0.01,
                    "at {w}x{h} the string {s:?} may run to {} in a {w}-wide window",
                    x + mw
                );
                assert!(
                    x >= -0.01 && y >= -0.01 && y + size <= h + 0.01,
                    "at {w}x{h} the string {s:?} starts at ({x}, {y}), off the window"
                );
            }
        }
    }

    #[test]
    fn the_header_says_what_the_board_actually_is() {
        let mut a = empty();
        set_cells(&mut a, &[(1, 1), (1, 2), (1, 3)]);
        a.apply(Action::StepOnce);
        a.apply(Action::StepOnce);
        let f = frame_of(&a);
        let said: Vec<String> = texts(&f).into_iter().map(|t| t.2).collect();
        assert!(
            said.contains(&"Gen 2".to_string()),
            "the header did not say Gen 2: {said:?}"
        );
        assert!(
            said.contains(&format!("Pop {}", a.grid().population())),
            "the header did not say the population it has: {said:?}"
        );
        assert!(
            said.contains(&"Paused".to_string()),
            "a paused board says Paused"
        );
        a.apply(Action::TogglePlay);
        let said: Vec<String> = texts(&frame_of(&a)).into_iter().map(|t| t.2).collect();
        assert!(
            said.contains(&"Running".to_string()),
            "a running board says Running"
        );
    }

    #[test]
    fn a_live_cell_is_drawn_and_a_dead_one_is_not() {
        let mut a = empty();
        let l = a.layout();
        let bare = fills(&frame_of(&a)).len();
        a.grid.set(11, 17, true);
        let after = fills(&frame_of(&a));
        assert_eq!(
            after.len(),
            bare + 1,
            "turning one cell on should add exactly one filled rectangle"
        );
        let want = l.cell_rect(11, 17);
        assert!(
            after
                .iter()
                .any(|&(r, c)| about(r.x, want.x, 0.01) && about(r.y, want.y, 0.01) && c == GREEN),
            "the live cell was not drawn green at {want:?}"
        );
    }

    #[test]
    fn the_cursor_is_drawn_whether_its_cell_is_alive_or_not() {
        let mut a = empty();
        let l = a.layout();
        let (r, c) = a.cursor();
        let want = l.cell_rect(r, c);
        let at_cursor = |a: &LifeApp| -> Option<Color> {
            fills(&frame_of(a))
                .into_iter()
                .filter(|&(rect, _)| about(rect.x, want.x, 0.01) && about(rect.y, want.y, 0.01))
                .map(|(_, colour)| colour)
                .next_back()
        };
        assert_eq!(
            at_cursor(&a),
            Some(SURFACE1),
            "a dead cell under the cursor should still be marked"
        );
        a.apply(Action::ToggleCell(a.grid().index(r, c).unwrap()));
        assert_eq!(
            at_cursor(&a),
            Some(LAVENDER),
            "a live cell under the cursor should be drawn in the cursor's colour"
        );
    }

    #[test]
    fn the_grid_lines_go_away_when_they_are_switched_off() {
        let mut a = app();
        assert!(a.show_grid(), "the program opens with grid lines on");
        let with = strokes(&frame_of(&a)).len();
        stroke(&mut a, Key::G);
        assert!(!a.show_grid());
        let without = strokes(&frame_of(&a)).len();
        assert!(
            without + GRID_ROWS * GRID_COLS == with,
            "switching the grid off should remove exactly one stroke per cell; \
             {with} -> {without}"
        );
    }

    #[test]
    fn grid_lines_are_not_drawn_when_a_cell_is_too_small_to_have_one() {
        // At two pixels a cell, a half-pixel line around every one of 4800 is
        // more ink than the cells themselves and the board reads as solid grey.
        let a = app();
        let l = layout_at(200.0, 160.0);
        assert!(
            l.cell < 4.0,
            "this test needs a window whose cells are tiny"
        );
        assert_eq!(
            strokes(&a.frame(200.0, 160.0))
                .into_iter()
                .filter(|&(_, c)| c == SURFACE0)
                .count(),
            0,
            "grid lines were drawn at a cell size where they cannot be seen"
        );
    }

    // ── The pointer ────────────────────────────────────────────────────────

    #[test]
    fn every_cell_is_clickable_where_it_is_drawn() {
        let a = app();
        let l = a.layout();
        for (row, col) in [
            (0usize, 0usize),
            (0, GRID_COLS - 1),
            (GRID_ROWS - 1, 0),
            (GRID_ROWS - 1, GRID_COLS - 1),
            (30, 40),
            (17, 3),
        ] {
            let (x, y) = l.cell_rect(row, col).centre();
            assert_eq!(
                a.target_at(x, y),
                a.grid().index(row, col).map(Target::Cell),
                "the point at the middle of ({row}, {col}) does not hit that cell"
            );
        }
    }

    #[test]
    fn a_click_flips_the_cell_it_lands_on_and_takes_the_cursor_there() {
        let mut a = empty();
        let index = a.grid().index(9, 61).unwrap();
        assert_eq!(
            probe::click(&mut a, Target::Cell(index)),
            EventResult::Consumed
        );
        assert!(a.grid().get(9, 61), "the clicked cell did not come alive");
        assert_eq!(a.cursor(), (9, 61), "the cursor should follow the click");
        probe::click(&mut a, Target::Cell(index));
        assert!(
            !a.grid().get(9, 61),
            "clicking it again did not turn it off"
        );
    }

    #[test]
    fn the_pixels_a_cell_is_drawn_on_are_the_pixels_that_click_it() {
        // Deliberately *not* an inverse through `cell_rect`: asking the layout
        // where a cell is and then clicking there proves only that the layout
        // agrees with itself, and would stay green if the whole board were
        // drawn a hundred pixels from where it is read. This goes through the
        // drawing instead -- it finds the one green rectangle the frame
        // actually emitted and clicks its middle.
        let mut a = empty();
        a.grid.set(41, 7, true);
        let green: Vec<Rect> = fills(&frame_of(&a))
            .into_iter()
            .filter(|&(_, c)| c == GREEN)
            .map(|(r, _)| r)
            .collect();
        assert_eq!(
            green.len(),
            1,
            "the board should have exactly one live cell drawn"
        );
        let (x, y) = green[0].centre();
        assert_eq!(
            handle_event(
                &mut a,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Press(MouseButton::Left),
                })
            ),
            EventResult::Consumed
        );
        assert!(
            !a.grid().get(41, 7),
            "clicking the middle of the rectangle the live cell was drawn on did not \
             reach that cell: the drawing and the hit test disagree about where it is"
        );
        assert_eq!(a.cursor(), (41, 7));
    }

    #[test]
    fn only_a_left_press_does_anything() {
        for button in [MouseButton::Right, MouseButton::Middle] {
            let mut a = app();
            let before = snap(&a);
            assert_eq!(
                probe::click_with(&mut a, Target::Cell(100), button),
                EventResult::Ignored,
                "{button:?} should not edit the board"
            );
            assert_eq!(snap(&a), before);
        }
        let mut a = app();
        let before = snap(&a);
        let l = a.layout();
        let (x, y) = l.cell_rect(5, 5).centre();
        assert_eq!(
            handle_event(
                &mut a,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Release(MouseButton::Left),
                })
            ),
            EventResult::Ignored,
            "the release of a click must not act a second time"
        );
        assert_eq!(snap(&a), before);
    }

    #[test]
    fn a_click_outside_everything_is_ignored() {
        let mut a = app();
        let before = snap(&a);
        assert_eq!(
            handle_event(
                &mut a,
                &Event::Mouse(MouseEvent {
                    x: -5.0,
                    y: -5.0,
                    kind: MouseEventKind::Press(MouseButton::Left),
                })
            ),
            EventResult::Ignored
        );
        assert_eq!(snap(&a), before);
    }

    #[test]
    fn every_button_does_what_its_key_does() {
        for (target, key) in [
            (Target::PlayPause, Key::Space),
            (Target::StepOnce, Key::S),
            (Target::Clear, Key::C),
            (Target::Randomize, Key::R),
            (Target::Patterns, Key::P),
            (Target::GridLines, Key::G),
            (Target::Slower, Key::Minus),
            (Target::Faster, Key::Equals),
            (Target::Help, Key::F1),
        ] {
            let mut clicked = app();
            let mut typed = app();
            assert_eq!(
                probe::click(&mut clicked, target),
                EventResult::Consumed,
                "{target:?} is not on the screen to be clicked"
            );
            stroke(&mut typed, key);
            assert_eq!(
                snap(&clicked),
                snap(&typed),
                "the {target:?} button and {key:?} do different things"
            );
        }
    }

    #[test]
    fn the_buttons_are_side_by_side_and_do_not_overlap() {
        let f = frame_of(&app());
        let mut boxes: Vec<(Target, Rect)> = f
            .hits()
            .iter()
            .filter(|(t, _)| !matches!(t, Target::Cell(_)))
            .copied()
            .collect();
        assert_eq!(
            boxes.len(),
            9,
            "nine controls should be on screen: {boxes:?}"
        );
        boxes.sort_by(|a, b| a.1.x.partial_cmp(&b.1.x).unwrap());
        for pair in boxes.windows(2) {
            assert!(
                pair[0].1.right() <= pair[1].1.x + 0.01,
                "{:?} and {:?} overlap",
                pair[0].0,
                pair[1].0
            );
        }
        for (target, r) in &boxes {
            assert!(
                r.w > 4.0 && r.h > 4.0,
                "{target:?} is too small to hit: {r:?}"
            );
        }
    }

    #[test]
    fn a_control_hit_box_is_where_the_control_is_drawn() {
        let f = frame_of(&app());
        for (target, r) in f.hits() {
            if matches!(target, Target::Cell(_)) {
                continue;
            }
            let (x, y) = r.centre();
            assert_eq!(
                f.hit_test(x, y),
                Some(*target),
                "the middle of {target:?}'s box hits something else"
            );
        }
    }

    // ── The pattern sheet, with a pointer ──────────────────────────────────

    /// Click whatever the frame says is at a point, through the real event path.
    ///
    /// Raw pixels rather than `probe::click(target)`: what is being checked here
    /// is *which* target a point reaches, so a helper that looks the target's own
    /// box up first would answer the question with itself.
    fn click(a: &mut LifeApp, x: f32, y: f32) -> EventResult {
        handle_event(
            a,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    /// The middle of the box recorded for `target`, or `None` if it has none.
    fn where_is(f: &Frame, target: Target) -> Option<(f32, f32)> {
        f.hits()
            .iter()
            .rev()
            .find(|(t, _)| *t == target)
            .map(|(_, r)| r.centre())
    }

    #[test]
    fn every_pattern_the_menu_lists_can_be_picked_with_the_pointer() {
        // Not "click where `Pattern::ALL[i]` ought to be": the test reads the
        // rows the frame actually drew, clicks each one, and checks the name the
        // sheet now shows in bold is the name that row was labelled with. A
        // menu that draws six rows and answers the seventh is caught.
        let mut a = app();
        a.apply(Action::OpenPatterns);
        let f = frame_of(&a);
        let rows: Vec<(usize, Rect)> = f
            .hits()
            .iter()
            .filter_map(|(t, r)| match t {
                Target::PatternRow(i) => Some((*i, *r)),
                _ => None,
            })
            .collect();
        assert_eq!(
            rows.len(),
            Pattern::ALL.len(),
            "the sheet offers {} of {} patterns",
            rows.len(),
            Pattern::ALL.len()
        );
        for (i, r) in rows {
            let mut a = app();
            a.apply(Action::OpenPatterns);
            let (x, y) = r.centre();
            assert_eq!(click(&mut a, x, y), EventResult::Consumed);
            assert_eq!(
                a.view(),
                View::PatternMenu,
                "picking a row closed the sheet"
            );
            let name = Pattern::ALL.get(i).map(|p| p.name());
            assert_eq!(
                Some(a.pattern().name()),
                name,
                "clicking row {i} selected a different pattern than it names"
            );
        }
    }

    #[test]
    fn the_pattern_menu_selection_moves_one_row_and_stops_at_the_ends() {
        let last = Pattern::ALL.len().saturating_sub(1);
        let mut a = app();
        a.apply(Action::OpenPatterns);
        a.apply(Action::SelectPattern(0));
        // Off the top: the first row is as far up as there is.
        for _ in 0..3 {
            stroke(&mut a, Key::Up);
            assert_eq!(
                a.selected_pattern(),
                0,
                "the selection went above the first row"
            );
        }
        for want in 1..=last {
            stroke(&mut a, Key::Down);
            assert_eq!(
                a.selected_pattern(),
                want,
                "Down should move exactly one row"
            );
        }
        // Off the bottom, likewise — and an unclamped selection would name a
        // pattern that is not there, so this is also the test that keeps
        // `Pattern::ALL.get(selected)` from ever being asked for row seven.
        for _ in 0..3 {
            stroke(&mut a, Key::Down);
            assert_eq!(
                a.selected_pattern(),
                last,
                "the selection went past the last row"
            );
        }
        assert!(
            Pattern::ALL.get(a.selected_pattern()).is_some(),
            "the selection names a pattern that does not exist"
        );
        for want in (0..last).rev() {
            stroke(&mut a, Key::Up);
            assert_eq!(a.selected_pattern(), want, "Up should move exactly one row");
        }
    }

    #[test]
    fn the_place_button_stamps_the_pattern_the_sheet_has_selected() {
        for (i, p) in Pattern::ALL.iter().enumerate() {
            let mut a = empty();
            a.apply(Action::OpenPatterns);
            a.apply(Action::SelectPattern(i));
            let f = frame_of(&a);
            let (x, y) = where_is(&f, Target::PlacePattern).expect("the sheet has a Place button");
            assert_eq!(click(&mut a, x, y), EventResult::Consumed);
            assert_eq!(a.view(), View::Board, "placing returns to the board");
            assert_eq!(
                a.grid().population(),
                p.cells().len(),
                "{p:?} was placed with the wrong number of cells"
            );
        }
    }

    #[test]
    fn cancelling_the_sheet_leaves_the_board_exactly_as_it_was() {
        for target in [Target::ClosePatterns, Target::PlacePattern] {
            let mut a = app();
            a.apply(Action::OpenPatterns);
            let before = snap(&a);
            let f = frame_of(&a);
            let (x, y) = where_is(&f, target).expect("both footer buttons are drawn");
            click(&mut a, x, y);
            if target == Target::ClosePatterns {
                assert_eq!(
                    snap(&a),
                    Snap {
                        view: View::Board,
                        ..before.clone()
                    },
                    "cancelling changed something other than the view"
                );
            } else {
                assert_ne!(a.grid(), &before.grid, "Place placed nothing");
            }
        }
    }

    #[test]
    fn a_click_anywhere_off_the_sheet_cancels_it() {
        // The four corners of the window and the middle of the board — none of
        // which is on the sheet, and every one of which is over something that
        // would answer a click if the sheet were not up.
        let (w, h) = LifeApp::SIZE;
        for (x, y) in [
            (2.0, 2.0),
            (w - 2.0, 2.0),
            (2.0, h - 2.0),
            (w - 2.0, h - 2.0),
        ] {
            let mut a = app();
            a.apply(Action::OpenPatterns);
            let before = snap(&a);
            assert_eq!(click(&mut a, x, y), EventResult::Consumed);
            assert_eq!(
                snap(&a),
                Snap {
                    view: View::Board,
                    ..before
                },
                "a click at ({x}, {y}) did something besides dismiss the sheet"
            );
        }
    }

    #[test]
    fn while_the_sheet_is_up_no_click_reaches_the_board_beneath_it() {
        // The backdrop is recorded before everything on the sheet, and
        // `hit_test` takes the last box at a point — so the sheet's own rows
        // still work while every cell under it is covered. Both halves are
        // checked here: nothing answers `Target::Cell`, and the rows do.
        let mut a = app();
        a.apply(Action::OpenPatterns);
        let f = frame_of(&a);
        let l = layout_at(LifeApp::SIZE.0, LifeApp::SIZE.1);
        let mut covered = 0u32;
        for row in (0..GRID_ROWS).step_by(7) {
            for col in (0..GRID_COLS).step_by(9) {
                let (x, y) = l.cell_rect(row, col).centre();
                let hit = f.hit_test(x, y);
                assert!(
                    !matches!(hit, Some(Target::Cell(_))),
                    "the cell at ({row}, {col}) is still clickable under the sheet"
                );
                covered = covered.saturating_add(1u32);
            }
        }
        assert!(covered > 20, "the sweep did not actually cover the board");
        assert!(
            f.hits()
                .iter()
                .any(|(t, _)| matches!(t, Target::PatternRow(_))),
            "the backdrop swallowed the sheet's own rows"
        );
    }

    #[test]
    fn opening_the_sheet_stops_the_board_running() {
        // A generation arriving under a modal sheet is a generation the user
        // did not watch happen, and the cursor they are about to stamp at has
        // moved out from under whatever they aimed it at.
        let mut a = app();
        a.apply(Action::TogglePlay);
        assert!(a.running());
        a.apply(Action::OpenPatterns);
        assert!(!a.running(), "the sheet came up over a running board");
        assert!(
            !a.clock_running(),
            "the clock is still wanted under the sheet"
        );
        assert_eq!(
            a.tick_interval(),
            None,
            "a sheet-covered board still asks for ticks"
        );
    }

    #[test]
    fn while_the_help_sheet_is_up_the_board_keys_do_nothing() {
        // The help sheet covers the board, so a board key that still worked
        // would edit something the reader cannot see. Found by mutation: the
        // suite could delete `if self.show_help` from `key_action` entirely and
        // stay green, because every test of the sheet went through the pointer
        // or through the clock, and the clock has its own `!show_help` guard.
        // Nothing had ever pressed a board key with the sheet up.
        for key in every_answered_key() {
            let mut a = app();
            a.apply(Action::ToggleHelp);
            let before = snap(&a);
            stroke(&mut a, key);
            let after = snap(&a);
            if key == Key::F1 {
                // The one key that answers: it is how the sheet is dismissed.
                assert_eq!(
                    after,
                    Snap {
                        show_help: false,
                        ..before
                    },
                    "F1 over the help sheet did more than close it"
                );
            } else {
                assert_eq!(
                    after, before,
                    "{key:?} reached the board through the help sheet"
                );
            }
        }
        // Escape closes it too, and nothing else changes.
        let mut a = app();
        a.apply(Action::ToggleHelp);
        let before = snap(&a);
        stroke(&mut a, Key::Escape);
        assert_eq!(
            snap(&a),
            Snap {
                show_help: false,
                ..before
            },
            "Escape over the help sheet did not close it, or did more"
        );
    }

    #[test]
    fn a_speed_is_never_set_outside_the_range_the_speeds_run_in() {
        // `Action` and `apply` are public, so an out-of-range speed is
        // constructible by any caller even though no key can produce one — and
        // an unclamped `speed` is not merely odd, it falls through `speed_ms`'s
        // catch-all arm and silently reads as the fastest setting whatever
        // number it holds. The clamp is the invariant; this is where it is
        // checked, because the keyboard cannot reach it.
        for s in [0u32, 10, 99, u32::MAX] {
            let mut a = app();
            a.apply(Action::SetSpeed(s));
            assert!(
                (1..=9).contains(&a.speed()),
                "SetSpeed({s}) left the speed at {}, outside 1..=9",
                a.speed()
            );
        }
        for s in 1..=9u32 {
            let mut a = app();
            a.apply(Action::SetSpeed(s));
            assert_eq!(a.speed(), s, "a speed in range was not taken at its word");
        }
    }

    #[test]
    fn a_label_stays_inside_the_control_it_names() {
        // Not the same check as `every_string_is_bounded_by_the_space_it_has`,
        // which bounds a string by the *window*. A centred label that starts
        // left of its own button is still inside the window, and was: found by
        // mutation, because centring subtracts the string's width from the
        // start and a string wider than its box therefore starts before it.
        // Narrow windows are what make the case reachable — at 320x240 the
        // "Patterns" button is about 33 pixels wide and its label wants 38.
        for (w, h) in SIZES {
            let a = app();
            let f = a.frame(w, h);
            let l = layout_at(w, h);
            if !l.shows(l.controls) {
                continue;
            }
            let boxes: Vec<Rect> = f
                .hits()
                .iter()
                .filter(|(t, _)| !matches!(t, Target::Cell(_)))
                .map(|(_, r)| *r)
                .collect();
            for (x, y, s, max_width, _size) in texts(&f) {
                // Only the controls band: the header and the sheets are laid
                // out by hand and are covered by the window-level check.
                if y < l.controls.y || l.controls.is_empty() {
                    continue;
                }
                let mw = max_width.unwrap_or(0.0);
                let inside = boxes
                    .iter()
                    .any(|b| x >= b.x - 0.01 && x + mw <= b.right() + 0.01);
                assert!(
                    inside,
                    "at {w}x{h} the label {s:?} runs from {x} to {} and no control it could \
                     belong to is that wide: {boxes:?}",
                    x + mw
                );
            }
        }
    }

    // ── The help sheet, with a pointer ─────────────────────────────────────

    #[test]
    fn while_the_help_sheet_is_up_a_click_anywhere_only_closes_it() {
        let (w, h) = LifeApp::SIZE;
        let l = layout_at(w, h);
        let mut points = vec![(2.0, 2.0), (w / 2.0, h / 2.0), (w - 2.0, h - 2.0)];
        points.push(l.cell_rect(0, 0).centre());
        points.push(l.cell_rect(GRID_ROWS / 2, GRID_COLS / 2).centre());
        for (x, y) in points {
            let mut a = app();
            a.apply(Action::ToggleHelp);
            let before = snap(&a);
            assert_eq!(click(&mut a, x, y), EventResult::Consumed);
            assert_eq!(
                snap(&a),
                Snap {
                    show_help: false,
                    ..before
                },
                "a click at ({x}, {y}) over the help sheet did more than close it"
            );
        }
    }

    #[test]
    fn the_help_sheet_covers_the_pattern_sheet_and_not_the_other_way_round() {
        // Both up at once: help is drawn last, so it is help that answers.
        let mut a = app();
        a.apply(Action::OpenPatterns);
        a.apply(Action::ToggleHelp);
        let f = frame_of(&a);
        let (x, y) = where_is(&f, Target::PlacePattern).expect("the sheet is still drawn");
        assert_eq!(
            f.hit_test(x, y),
            Some(Target::CloseHelp),
            "the Place button answers through the help sheet on top of it"
        );
        click(&mut a, x, y);
        assert!(!a.show_help(), "the click closed nothing");
        assert_eq!(
            a.view(),
            View::PatternMenu,
            "closing help also cancelled the sheet"
        );
    }

    // ── Clearing and the random soup ───────────────────────────────────────

    #[test]
    fn clearing_empties_the_board_and_starts_the_count_again() {
        let mut a = app();
        a.apply(Action::StepOnce);
        a.apply(Action::StepOnce);
        assert!(a.grid().population() > 0 && a.generation() == 2);
        stroke(&mut a, Key::C);
        assert_eq!(a.grid().population(), 0, "Clear left cells on the board");
        assert_eq!(
            a.generation(),
            0,
            "Clear left the generation count where it was"
        );
    }

    #[test]
    fn a_random_soup_fills_about_the_quarter_of_the_board_it_promises() {
        // `Action::Randomize` asks for 25 in 100. Over 4800 cells the spread of
        // a fair quarter is tight enough that a band of 15% to 35% catches a
        // density constant changed to any other round number, and catches a
        // generator that answers the same thing every time.
        let total = GRID_COLS.saturating_mul(GRID_ROWS);
        for seed in [1u64, 7, 99, 0x00C0_FFEE, u64::MAX] {
            let mut a = LifeApp::with_seed(seed);
            a.apply(Action::Randomize);
            let pop = a.grid().population();
            let share = pop as f64 / total as f64;
            assert!(
                (0.15..0.35).contains(&share),
                "seed {seed} filled {pop} of {total} cells — {share:.3} of the board, not a quarter"
            );
        }
    }

    #[test]
    fn two_seeds_give_two_different_soups_and_one_seed_gives_the_same_one() {
        let soup = |seed| {
            let mut a = LifeApp::with_seed(seed);
            a.apply(Action::Randomize);
            a.grid().clone()
        };
        assert_ne!(soup(1), soup(2), "two seeds drew the same soup");
        assert_eq!(soup(3), soup(3), "one seed drew two different soups");
    }

    #[test]
    fn a_soup_is_a_new_board_and_not_a_layer_over_the_old_one() {
        // The old `Randomize` wrote every cell, so a board that was full before
        // is a quarter full after. A version that only ever turned cells *on*
        // would leave a full board full, and no population check on an empty
        // starting board would notice.
        let mut a = empty();
        for i in 0..GRID_COLS.saturating_mul(GRID_ROWS) {
            a.apply(Action::ToggleCell(i));
        }
        assert_eq!(a.grid().population(), GRID_COLS * GRID_ROWS);
        a.apply(Action::Randomize);
        let pop = a.grid().population();
        assert!(
            pop < GRID_COLS * GRID_ROWS / 2,
            "a soup over a full board left {pop} cells alive"
        );
    }

    #[test]
    fn a_soup_starts_the_generation_count_again_and_forgets_the_part_generation() {
        let mut a = app();
        a.apply(Action::TogglePlay);
        let three_and_a_bit = a.speed_ms().saturating_mul(3).saturating_add(7);
        tick(&mut a, three_and_a_bit);
        assert!(a.generation() > 0);
        a.apply(Action::Randomize);
        assert_eq!(
            a.generation(),
            0,
            "a new soup kept the old board's generation"
        );
        assert_eq!(
            a.tick_accum, 0,
            "a new soup kept part of a generation's time"
        );
    }
}
