//! Maze — walk from the corner to the corner, in a real window.
//!
//! Three sizes, keyboard and pointer, a clock that runs, a solution overlay,
//! and a live count of the steps you have left.
//!
//! # What wiring this up found
//!
//! The program drew a maze and could not be played, because `main` built a
//! `MazeApp`, dropped it and exited. Nothing below was reachable to notice
//! until it had a window on it.
//!
//! 1. **Every key ran twice.** Both key handlers destructured
//!    `Event::Key(KeyEvent { key, .. })`, and the field the `..` swallowed was
//!    `pressed` — so the press ran the action and the release ran it again. The
//!    compositor does send releases (`gui/compositor/src/lib.rs` builds a
//!    `KeyEvent` with `pressed: false` for every key-up), so this was not
//!    theoretical; it was what the program would do the first second anyone
//!    used it. What it does to each key is worth spelling out, because three of
//!    the four are worse than "twice":
//!    - **The solution overlay could never be seen at all.** `H` flipped
//!      `show_solution` on at the press and back off at the release. There was
//!      no moment between them in which a frame was drawn, so the feature named
//!      on the help bar — "H=Show/Hide Solution" — had no reachable state in
//!      which it showed anything.
//!    - **The difficulty cycled backwards.** `D` ran `Difficulty::next` twice,
//!      so the order the sizes actually appeared in was Small, Large, Medium —
//!      the reverse of the one `next` spells out — and each press also dealt
//!      two mazes and threw the first away.
//!    - **The move counter was always double**, which is exactly the number the
//!      win box compared against `Optimal`. A player who walked a 10x10 maze
//!      perfectly was shown "Moves: 36 (Optimal: 18)" and told, in effect, that
//!      they had taken twice the best route while standing on it.
//!    - `N` dealt two mazes and showed the second.
//! 2. **The advertised "Timer tracking" never ticked.** `elapsed_secs` was set
//!    to zero in `new` and in `new_maze`, read only by `format_time`, and
//!    incremented nowhere in the program. `format_time` was in turn never
//!    called by `render`, so even a working clock would not have reached the
//!    screen. Both halves were invisible because `#![allow(dead_code)]` sat on
//!    line 13. The clock is real now: [`MazeApp::tick_interval`] asks for one,
//!    `Event::Tick` advances it by the `elapsed_ms` it carries rather than by
//!    the interval asked for, it stops when the maze is solved, and it is drawn.
//!    This is `known-issues.md` lesson 47's sixth application.
//! 3. **The layout was a constant.** `render(width, height)` used its two
//!    arguments for the background rectangle, the help bar's `y` and the win
//!    overlay, and nothing else. The maze drew at a fixed `(20, 50)` with a
//!    cell size read from the *difficulty* — 15 pixels on Large, whatever the
//!    window — and the top bar's five strings drew at fixed `x` of 12, 80, 260,
//!    380 and 530, so "Won: N" fell off the right edge of any window narrower
//!    than about 600 pixels and the maze fell off the bottom of any window
//!    shorter than 500.
//! 4. **There was no pointer input at all**, though `MouseButton`,
//!    `MouseEvent` and `MouseEventKind` were all imported — hidden by
//!    `#![allow(unused_imports)]` on line 24. Clicking a cell now walks the
//!    player to it along the maze's own shortest route, one counted move per
//!    step, which is exactly what the arrow keys would have done.
//! 5. **`handle_won` did not check modifiers** while `handle_playing` did, so
//!    Ctrl-N dealt a fresh maze on the win screen and did nothing during play.
//! 6. **`Maze::solve` stepped out of bounds unchecked.** Its BFS computed
//!    `let nr = (r as i32 + dr) as usize;` with no `in_bounds` test — unlike
//!    `generate`, three functions above, which had one — and relied entirely on
//!    `can_move` never being true at a border. That holds only because
//!    `generate` happens never to remove a border wall; nothing in the type
//!    says so. A single removed border wall turns `-1` into `usize::MAX`, and
//!    `idx` then computes `row * cols + col`, which for that row is not an
//!    out-of-range index that panics but a *plausible* one that silently reads
//!    another cell. Every neighbour is bounds-checked now, in one place
//!    ([`Maze::step`]), which is also the only place that does the arithmetic.
//! 7. **The "Optimal" figure was frozen at the opening.** It was the length of
//!    the route from `(0, 0)`, computed once in `new_maze`, and it went on
//!    describing that journey after twenty moves down a journey you were no
//!    longer on. The opening optimum is still shown, as the thing to read your
//!    score against, but the live number beside it is the count of steps
//!    remaining **from where you actually stand** — which in a perfect maze is
//!    not an estimate but the answer.
//! 8. **One best-move count was shared by all three sizes.** A 10x10 solved in
//!    18 moves set a best that a 30x30 could never beat, and the win box
//!    presented the two as comparable. Best is kept per size now.
//! 9. **Nine blanket `#![allow]`s sat on lines 13-24**, `dead_code` and
//!    `unused_imports` among them, which is what kept 2 and 4 quiet. A
//!    hand-written `impl Clone for Maze` reproduced the derive field for field.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// The cell the player stands on.
const PLAYER_BG: Color = Color::from_hex(0x2A3A5A);
/// The cell to reach.
const GOAL_BG: Color = Color::from_hex(0x2A4A3A);

const WINDOW_WIDTH: f32 = 780.0;
const WINDOW_HEIGHT: f32 = 740.0;

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `MAZE!!!!`.
const FALLBACK_SEED: u64 = 0x4D41_5A45_2121_2121;

/// How often to ask for a tick while the clock is running.
///
/// A quarter of a second: fine enough that the seconds digit turns over within
/// a frame of when it should, coarse enough that a maze left open does not wake
/// the compositor sixty times a second to redraw a number that changes once.
const TICK: Duration = Duration::from_millis(250);

const HELP_TITLE: &str = "How to play";

/// Every key the program answers, and what it does. Drawn on the help sheet,
/// and walked by a test in both directions — every key named here answers, and
/// every key that answers is named here — so the sheet cannot drift from the
/// program the way the old help bar had (it named four keys and the program
/// answered seven).
const HELP_ROWS: [(&str, &str); 8] = [
    ("Arrows", "step one cell"),
    ("Click a cell", "walk there by the shortest route"),
    ("N", "a fresh maze at this size"),
    ("S", "show or hide the way out"),
    ("D", "the next size up"),
    ("1 / 2 / 3", "small, medium or large"),
    ("H / Esc", "open or close this sheet"),
    ("Enter", "a fresh maze once you are out"),
];

// ── Direction ──────────────────────────────────────────────────────────────

/// One of the four sides of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    North,
    South,
    East,
    West,
}

impl Dir {
    /// All four, in a fixed order. The generator shuffles a copy of this; it
    /// must not be relied on for anything but "each exactly once".
    pub const ALL: [Dir; 4] = [Dir::North, Dir::South, Dir::East, Dir::West];

    /// The step this direction takes, as (row, column).
    #[must_use]
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Dir::North => (-1, 0),
            Dir::South => (1, 0),
            Dir::East => (0, 1),
            Dir::West => (0, -1),
        }
    }

    #[must_use]
    pub const fn opposite(self) -> Dir {
        match self {
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::East => Dir::West,
            Dir::West => Dir::East,
        }
    }

    /// The bit that stands for this side's wall in a cell's bitfield.
    #[must_use]
    pub const fn bit(self) -> u8 {
        match self {
            Dir::North => 1,
            Dir::South => 2,
            Dir::East => 4,
            Dir::West => 8,
        }
    }
}

// ── Maze ───────────────────────────────────────────────────────────────────

/// A cell with all four walls standing.
const ALL_WALLS: u8 = 0x0F;

/// A perfect maze: every cell reachable from every other, by exactly one route.
///
/// Walls are stored per cell as a bitfield, which means each wall is stored
/// twice — once on each side of it. [`Maze::carve`] is the only thing that
/// writes one, and it writes both halves, so the two copies cannot disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Maze {
    rows: usize,
    cols: usize,
    cells: Vec<u8>,
}

impl Maze {
    /// A fully walled grid, which is not yet a maze.
    #[must_use]
    pub fn walled(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            cells: vec![ALL_WALLS; rows.saturating_mul(cols)],
        }
    }

    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    #[must_use]
    pub const fn cols(&self) -> usize {
        self.cols
    }

    #[must_use]
    pub const fn cell_count(&self) -> usize {
        self.rows.saturating_mul(self.cols)
    }

    /// The flat index of `(r, c)`, or `None` if it is off the grid.
    #[must_use]
    pub fn index(&self, r: usize, c: usize) -> Option<usize> {
        if r >= self.rows || c >= self.cols {
            return None;
        }
        r.checked_mul(self.cols)?.checked_add(c)
    }

    /// The row and column of a flat index, or `None` if it is past the end.
    #[must_use]
    pub fn coords(&self, i: usize) -> Option<(usize, usize)> {
        if self.cols == 0 || i >= self.cell_count() {
            return None;
        }
        Some((i.checked_div(self.cols)?, i.checked_rem(self.cols)?))
    }

    /// The neighbour of `(r, c)` in `dir`, **bounds-checked**, or `None` at the
    /// edge of the grid.
    ///
    /// The one place in this program that adds a delta to a coordinate. Fault
    /// six was a second place that did it without the check, one function away
    /// from a first place that had it.
    #[must_use]
    pub fn step(&self, r: usize, c: usize, dir: Dir) -> Option<(usize, usize)> {
        if r >= self.rows || c >= self.cols {
            return None;
        }
        let (dr, dc) = dir.delta();
        let nr = i64::try_from(r).ok()?.checked_add(i64::from(dr))?;
        let nc = i64::try_from(c).ok()?.checked_add(i64::from(dc))?;
        let nr = usize::try_from(nr).ok()?;
        let nc = usize::try_from(nc).ok()?;
        if nr >= self.rows || nc >= self.cols {
            return None;
        }
        Some((nr, nc))
    }

    /// Whether the side of `(r, c)` facing `dir` is walled.
    ///
    /// A cell off the grid is walled on every side, which is what makes the
    /// outer border of the maze solid without a special case anywhere else.
    #[must_use]
    pub fn has_wall(&self, r: usize, c: usize, dir: Dir) -> bool {
        let Some(i) = self.index(r, c) else {
            return true;
        };
        self.cells.get(i).copied().unwrap_or(ALL_WALLS) & dir.bit() != 0
    }

    /// Whether a player standing at `(r, c)` may step in `dir`.
    #[must_use]
    pub fn open(&self, r: usize, c: usize, dir: Dir) -> bool {
        self.step(r, c, dir).is_some() && !self.has_wall(r, c, dir)
    }

    /// Knock down the wall between `(r, c)` and its neighbour in `dir`.
    ///
    /// Writes both sides of the one wall. Returns whether anything was there.
    fn carve(&mut self, r: usize, c: usize, dir: Dir) -> bool {
        let Some((nr, nc)) = self.step(r, c, dir) else {
            return false;
        };
        let (Some(here), Some(there)) = (self.index(r, c), self.index(nr, nc)) else {
            return false;
        };
        if let Some(cell) = self.cells.get_mut(here) {
            *cell &= !dir.bit();
        }
        if let Some(cell) = self.cells.get_mut(there) {
            *cell &= !dir.opposite().bit();
        }
        true
    }

    /// A maze by recursive backtracker, iterative with an explicit stack.
    ///
    /// The result is *perfect*: exactly `rows * cols - 1` walls come down and
    /// every cell is reached, so between any two cells there is one route and
    /// no loops. [`Maze::is_perfect`] checks both halves of that and a test
    /// holds every dealt maze to it.
    #[must_use]
    pub fn generate(rows: usize, cols: usize, rng: &mut SeededRng) -> Self {
        let mut maze = Self::walled(rows, cols);
        if rows == 0 || cols == 0 {
            return maze;
        }
        let mut seen = vec![false; maze.cell_count()];
        if let Some(first) = seen.get_mut(0) {
            *first = true;
        }
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        while let Some(&(r, c)) = stack.last() {
            let mut dirs = Dir::ALL;
            rng.shuffle(&mut dirs);
            let next = dirs.into_iter().find_map(|dir| {
                let (nr, nc) = maze.step(r, c, dir)?;
                let i = maze.index(nr, nc)?;
                if seen.get(i).copied().unwrap_or(true) {
                    None
                } else {
                    Some((nr, nc, i, dir))
                }
            });
            match next {
                Some((nr, nc, i, dir)) => {
                    maze.carve(r, c, dir);
                    if let Some(flag) = seen.get_mut(i) {
                        *flag = true;
                    }
                    stack.push((nr, nc));
                }
                None => {
                    stack.pop();
                }
            }
        }
        maze
    }

    /// The number of steps from `from` to every cell, or `None` where the cell
    /// cannot be reached at all.
    ///
    /// A breadth-first sweep, so the numbers are shortest-path distances rather
    /// than the length of the first route found.
    #[must_use]
    pub fn distances(&self, from: (usize, usize)) -> Vec<Option<u32>> {
        let mut dist: Vec<Option<u32>> = vec![None; self.cell_count()];
        let Some(start) = self.index(from.0, from.1) else {
            return dist;
        };
        if let Some(slot) = dist.get_mut(start) {
            *slot = Some(0);
        }
        let mut queue: Vec<usize> = vec![start];
        let mut head = 0usize;
        while let Some(&current) = queue.get(head) {
            head = head.saturating_add(1);
            let Some((r, c)) = self.coords(current) else {
                continue;
            };
            let d = dist.get(current).copied().flatten().unwrap_or(0);
            for dir in Dir::ALL {
                if !self.open(r, c, dir) {
                    continue;
                }
                let Some((nr, nc)) = self.step(r, c, dir) else {
                    continue;
                };
                let Some(i) = self.index(nr, nc) else {
                    continue;
                };
                if dist.get(i).copied().flatten().is_some() {
                    continue;
                }
                if let Some(slot) = dist.get_mut(i) {
                    *slot = Some(d.saturating_add(1));
                }
                queue.push(i);
            }
        }
        dist
    }

    /// How many steps `to` is from `from`, or `None` if it cannot be reached.
    #[must_use]
    pub fn steps_between(&self, from: (usize, usize), to: (usize, usize)) -> Option<u32> {
        let i = self.index(to.0, to.1)?;
        self.distances(from).get(i).copied().flatten()
    }

    /// The shortest route from `from` to `to`, both ends included.
    ///
    /// Empty when there is no route. Walked backwards from `to` by stepping to
    /// whichever neighbour is one closer to `from`, which needs no parent array
    /// and cannot disagree with the distances it is read from.
    #[must_use]
    pub fn path(&self, from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
        let dist = self.distances(from);
        let Some(end) = self.index(to.0, to.1) else {
            return Vec::new();
        };
        let Some(mut d) = dist.get(end).copied().flatten() else {
            return Vec::new();
        };
        let mut route = vec![to];
        let (mut r, mut c) = to;
        while d > 0 {
            let mut moved = false;
            for dir in Dir::ALL {
                if !self.open(r, c, dir) {
                    continue;
                }
                let Some((nr, nc)) = self.step(r, c, dir) else {
                    continue;
                };
                let Some(i) = self.index(nr, nc) else {
                    continue;
                };
                if dist.get(i).copied().flatten() == d.checked_sub(1) {
                    r = nr;
                    c = nc;
                    d = d.saturating_sub(1);
                    route.push((r, c));
                    moved = true;
                    break;
                }
            }
            if !moved {
                break;
            }
        }
        route.reverse();
        route
    }

    /// How many walls have come down.
    #[must_use]
    pub fn passages(&self) -> usize {
        // Each passage is recorded on both of the cells it joins, so counting
        // missing wall bits over the whole grid counts every passage twice —
        // and counting the border, which is never carved, not at all.
        let bits: usize = self
            .cells
            .iter()
            .map(|cell| usize::try_from(cell.count_zeros().saturating_sub(4)).unwrap_or(0))
            .sum();
        bits / 2
    }

    /// Whether this is a perfect maze: every cell reachable, and no loops.
    ///
    /// A connected graph on *n* nodes with exactly *n* - 1 edges is a tree, so
    /// checking both together is checking that there is exactly one route
    /// between any two cells. Either check alone would pass a maze that is
    /// wrong in the other direction.
    #[must_use]
    pub fn is_perfect(&self) -> bool {
        let n = self.cell_count();
        if n == 0 {
            return true;
        }
        let reached = self.distances((0, 0)).into_iter().flatten().count();
        reached == n && self.passages() == n.saturating_sub(1)
    }
}

// ── Difficulty ─────────────────────────────────────────────────────────────

/// How big a maze to deal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Small,
    Medium,
    Large,
}

/// The three sizes, in the order the buttons and the number keys name them.
///
/// The size cycle reads this rather than a hand-written `next`, so "the button
/// walks the sizes in the order the buttons are drawn in" is true by
/// construction instead of by two definitions agreeing.
pub const LEVELS: [Difficulty; 3] = [Difficulty::Small, Difficulty::Medium, Difficulty::Large];

impl Difficulty {
    /// Rows and columns.
    #[must_use]
    pub const fn size(self) -> (usize, usize) {
        match self {
            Self::Small => (10, 10),
            Self::Medium => (20, 20),
            Self::Large => (30, 30),
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    /// The name with the grid, for the header.
    #[must_use]
    pub fn described(self) -> String {
        let (rows, cols) = self.size();
        format!("{} ({rows}x{cols})", self.name())
    }
}

// ── Targets and actions ────────────────────────────────────────────────────

/// Something on the screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A cell of the maze, by flat index.
    Cell(usize),
    /// One of the size buttons, by index into [`LEVELS`].
    Level(usize),
    NewMaze,
    ToggleSolution,
    ToggleHelp,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Everything the game can be asked to do, from either input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// One step in a direction — what an arrow key does.
    Step(Dir),
    /// Walk to a named cell by the shortest legal route — what a click does.
    WalkTo(usize),
    NewMaze,
    /// Switch to `LEVELS[i]`.
    SetLevel(usize),
    ToggleSolution,
    ToggleHelp,
    CloseHelp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Playing,
    Won,
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the maze keeps no matter what.
const BOARD_SHARE: f32 = 0.6;

/// Which band goes first when they do not all fit: header, controls, info.
///
/// Bands are dropped whole rather than shrunk together, because a band squeezed
/// to four pixels costs the maze four pixels and shows nothing legible. The
/// title goes first — it names a program you are looking at. The controls go
/// next, because every one of them is a button for a key that still works
/// without it. The info line goes last: the clock, the move count and the steps
/// remaining are the only chrome that says anything the maze itself does not.
const BAND_DROP_ORDER: [usize; 3] = [0, 2, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in — which
/// is precisely what a maze pinned at `(20, 50)` with a cell size read from the
/// difficulty was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// The title and the size's name.
    pub header: Rect,
    /// The clock, the moves made, the steps left and the best so far.
    pub info: Rect,
    /// The square the grid is drawn in.
    pub board: Rect,
    /// The three sizes, a new maze, the solution toggle and help.
    pub controls: Rect,
    pub help: Rect,
    pub font: f32,
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
        let big = (font * 1.6).clamp(13.0, 28.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, info, controls] order.
        let mut wants = [
            (h * 0.085).clamp(22.0, 44.0),
            (h * 0.055).clamp(16.0, 28.0),
            (h * 0.08).clamp(22.0, 40.0),
        ];
        // What is left for chrome once the maze has its share *and* the gap
        // between it and the chrome above and below. The padding is charged
        // here rather than to the maze: taking it out of the maze's side would
        // turn a promised share of the window into rather less than that share
        // of a small one.
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, inf_h, ctl_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall. Both read the same to `shows`, but only one of them reads the
        // same to anything asking "is this band gone, or merely thin?"
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
        let controls = if ctl_h > 0.0 {
            Rect::new(0.0, h - ctl_h, w, ctl_h)
        } else {
            Rect::EMPTY
        };

        // From the heights, not from `info.bottom()`. A dropped band is
        // `Rect::EMPTY`, whose bottom is zero, so reading the band back would
        // put the maze over the header the moment the info line went while the
        // header stayed. `BAND_DROP_ORDER` drops the header first today, so the
        // two forms agree and no test can tell them apart — which is the reason
        // to write the safe one here rather than leave it to be got right again
        // by whoever reorders the constant.
        let top = hdr_h + inf_h;
        let bottom = if ctl_h > 0.0 { controls.y } else { h };
        let band = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );
        // Square and centred in what is left. A square grid drawn in an oblong
        // either stretches its cells or leaves them somewhere the hit test is
        // not; squaring it here means neither.
        let side = band.w.min(band.h).max(0.0);
        let board = Rect::new(
            band.x + (band.w - side) / 2.0,
            band.y + (band.h - side) / 2.0,
            side,
            side,
        );

        let help_w = (w * 0.92).min(480.0);
        let help_h = (h * 0.92).min(340.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            board,
            controls,
            help,
            font,
            big,
            pad,
        }
    }

    /// Whether a band is tall and wide enough for its text to be worth drawing.
    #[must_use]
    pub fn shows(&self, band: Rect) -> bool {
        band.h >= 11.0 && band.w >= 110.0
    }

    /// The side of one cell of a `rows` by `cols` grid.
    #[must_use]
    pub fn cell(&self, rows: usize, cols: usize) -> f32 {
        if rows == 0 || cols == 0 {
            return 0.0;
        }
        // At most 30 here, so the casts are exact.
        (self.board.w / cols as f32).min(self.board.h / rows as f32)
    }

    /// The rectangle of the cell at `(r, c)` of a `rows` by `cols` grid.
    #[must_use]
    pub fn square(&self, rows: usize, cols: usize, r: usize, c: usize) -> Rect {
        if r >= rows || c >= cols {
            return Rect::EMPTY;
        }
        let cell = self.cell(rows, cols);
        Rect::new(
            self.board.x + c as f32 * cell,
            self.board.y + r as f32 * cell,
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

/// The game.
pub struct MazeApp {
    maze: Maze,
    /// Index into [`LEVELS`].
    level: usize,
    player: (usize, usize),
    goal: (usize, usize),
    /// Which cells have been stood on, for the trail.
    trail: Vec<bool>,
    moves: u32,
    /// How far the goal was when the maze was dealt, so a finished run can be
    /// read against the maze it was run in rather than against another maze.
    opening_steps: u32,
    /// Milliseconds since the maze was dealt, advanced only by `Event::Tick`
    /// and only by the elapsed time the tick reports.
    elapsed_ms: u64,
    state: GameState,
    games_won: u32,
    /// The fewest moves a maze of each size has been solved in. One slot per
    /// size, because a best set on a 10x10 is not a target a 30x30 can meet.
    best: [Option<u32>; LEVELS.len()],
    show_solution: bool,
    show_help: bool,
    status: String,
    rng: SeededRng,
    size_drawn: (f32, f32),
}

impl MazeApp {
    #[must_use]
    pub fn new() -> Self {
        // Was `SeededRng::new(42)`: every player, on every machine, walked the
        // same maze. This asks the kernel and falls back rather than refusing.
        Self::with_seed(guitk::rng::seed_from_system(FALLBACK_SEED))
    }

    /// The game with a named seed, so a test can name the maze it means.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        let mut game = Self {
            maze: Maze::walled(1, 1),
            level: 0,
            player: (0, 0),
            goal: (0, 0),
            trail: Vec::new(),
            moves: 0,
            opening_steps: 0,
            elapsed_ms: 0,
            state: GameState::Playing,
            games_won: 0,
            best: [None; LEVELS.len()],
            show_solution: false,
            show_help: false,
            status: String::new(),
            rng: SeededRng::new(seed),
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        game.new_maze();
        game
    }

    // ── Readers ────────────────────────────────────────────────────────────

    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size_drawn.0, self.size_drawn.1)
    }

    #[must_use]
    pub const fn maze(&self) -> &Maze {
        &self.maze
    }

    #[must_use]
    pub fn level(&self) -> Difficulty {
        LEVELS.get(self.level).copied().unwrap_or(Difficulty::Small)
    }

    #[must_use]
    pub const fn level_index(&self) -> usize {
        self.level
    }

    #[must_use]
    pub const fn player(&self) -> (usize, usize) {
        self.player
    }

    #[must_use]
    pub const fn goal(&self) -> (usize, usize) {
        self.goal
    }

    #[must_use]
    pub const fn state(&self) -> GameState {
        self.state
    }

    #[must_use]
    pub const fn moves(&self) -> u32 {
        self.moves
    }

    #[must_use]
    pub const fn opening_steps(&self) -> u32 {
        self.opening_steps
    }

    #[must_use]
    pub const fn games_won(&self) -> u32 {
        self.games_won
    }

    #[must_use]
    pub fn best_at(&self, level: usize) -> Option<u32> {
        self.best.get(level).copied().flatten()
    }

    #[must_use]
    pub const fn show_solution(&self) -> bool {
        self.show_solution
    }

    #[must_use]
    pub const fn show_help(&self) -> bool {
        self.show_help
    }

    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    #[must_use]
    pub fn on_trail(&self, r: usize, c: usize) -> bool {
        self.maze
            .index(r, c)
            .and_then(|i| self.trail.get(i).copied())
            .unwrap_or(false)
    }

    #[must_use]
    pub const fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// The clock, as minutes and seconds.
    #[must_use]
    pub fn clock(&self) -> String {
        let secs = self.elapsed_ms / 1000;
        format!("{}:{:02}", secs / 60, secs % 60)
    }

    /// How many steps the goal is from where the player stands, or `None` if
    /// there is no route — which a perfect maze never has, and which is
    /// therefore the shape of a bug rather than of a hard puzzle.
    #[must_use]
    pub fn steps_left(&self) -> Option<u32> {
        self.maze.steps_between(self.player, self.goal)
    }

    /// The route out from where the player stands, both ends included.
    #[must_use]
    pub fn solution(&self) -> Vec<(usize, usize)> {
        self.maze.path(self.player, self.goal)
    }

    /// Whether the clock should be running.
    ///
    /// A solved maze stops it, so the time on the win line is the time the run
    /// took rather than the time the window has been open since.
    #[must_use]
    pub const fn clock_running(&self) -> bool {
        matches!(self.state, GameState::Playing)
    }

    /// What the info line says about how far there is left to go.
    fn progress(&self) -> String {
        match self.steps_left() {
            Some(0) => "Out".to_string(),
            Some(1) => "1 step to go".to_string(),
            Some(n) => format!("{n} steps to go"),
            None => "No way through — this maze is broken".to_string(),
        }
    }

    // ── Play ───────────────────────────────────────────────────────────────

    /// Deal a fresh maze at the current size.
    pub fn new_maze(&mut self) {
        let (rows, cols) = self.level().size();
        self.maze = Maze::generate(rows, cols, &mut self.rng);
        self.player = (0, 0);
        self.goal = (rows.saturating_sub(1), cols.saturating_sub(1));
        self.trail = vec![false; self.maze.cell_count()];
        if let Some(first) = self.trail.get_mut(0) {
            *first = true;
        }
        self.moves = 0;
        self.opening_steps = self.steps_left().unwrap_or(0);
        self.elapsed_ms = 0;
        self.state = if self.player == self.goal {
            GameState::Won
        } else {
            GameState::Playing
        };
        self.show_solution = false;
        self.status = self.progress();
    }

    fn set_level(&mut self, index: usize) {
        if index >= LEVELS.len() {
            return;
        }
        if index == self.level {
            self.status = format!("Already {}", self.level().name());
            return;
        }
        self.level = index;
        self.new_maze();
    }

    /// Move onto a cell known to be one open step away, counting the move.
    fn advance_to(&mut self, next: (usize, usize)) {
        self.player = next;
        if let Some(i) = self.maze.index(next.0, next.1) {
            if let Some(seen) = self.trail.get_mut(i) {
                *seen = true;
            }
        }
        self.moves = self.moves.saturating_add(1);
        if self.player == self.goal {
            self.win();
        } else {
            self.status = self.progress();
        }
    }

    fn win(&mut self) {
        self.state = GameState::Won;
        self.games_won = self.games_won.saturating_add(1);
        let moves = self.moves;
        if let Some(slot) = self.best.get_mut(self.level) {
            if slot.is_none_or(|prev| moves < prev) {
                *slot = Some(moves);
            }
        }
        self.status = if moves == self.opening_steps {
            format!("Out in {moves} — the shortest way there was")
        } else {
            format!("Out in {moves} moves (shortest: {})", self.opening_steps)
        };
    }

    /// One step in a direction, if the wall is down.
    fn try_step(&mut self, dir: Dir) {
        if self.state == GameState::Won {
            self.status = "You are already out — N for another".to_string();
            return;
        }
        let (r, c) = self.player;
        if !self.maze.open(r, c, dir) {
            self.status = "A wall".to_string();
            return;
        }
        let Some(next) = self.maze.step(r, c, dir) else {
            return;
        };
        self.advance_to(next);
    }

    /// Walk to a cell along the maze's own shortest route.
    ///
    /// One counted move per step, which is what the arrow keys would have
    /// charged for the same journey — so a maze played with the mouse and a
    /// maze played with the keyboard produce comparable numbers.
    fn walk_to(&mut self, cell: usize) {
        if self.state == GameState::Won {
            self.status = "You are already out — N for another".to_string();
            return;
        }
        let Some(dest) = self.maze.coords(cell) else {
            return;
        };
        if dest == self.player {
            self.status = "You are standing there".to_string();
            return;
        }
        let route = self.maze.path(self.player, dest);
        if route.len() < 2 {
            self.status = "No way there from here".to_string();
            return;
        }
        for &next in route.iter().skip(1) {
            self.advance_to(next);
            // The route to a cell may pass through the goal. Walking on past
            // the exit because the click asked for somewhere else would be a
            // win the player never got to see.
            if self.state == GameState::Won {
                return;
            }
        }
    }

    /// The one place an action changes the game, so a key and a click that mean
    /// the same thing cannot come to mean different things.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Step(dir) => self.try_step(dir),
            Action::WalkTo(cell) => self.walk_to(cell),
            Action::NewMaze => self.new_maze(),
            Action::SetLevel(i) => self.set_level(i),
            Action::ToggleSolution => {
                self.show_solution = !self.show_solution;
                self.status = if self.show_solution {
                    "The way out is marked".to_string()
                } else {
                    self.progress()
                };
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                self.status = if self.show_help {
                    HELP_TITLE.to_string()
                } else {
                    self.progress()
                };
            }
            Action::CloseHelp => {
                if self.show_help {
                    self.show_help = false;
                    self.status = self.progress();
                }
            }
        }
    }

    /// Advance the clock by the time a tick says has actually passed.
    ///
    /// By `elapsed_ms` and never by [`TICK`]: the interval asked for is a floor,
    /// not a promise, so a clock driven by the constant runs slow by however
    /// much the loop was busy — and runs slow *silently*, which is the worst
    /// way for a clock to be wrong.
    pub fn tick(&mut self, elapsed_ms: u64) -> EventResult {
        if !self.clock_running() {
            return EventResult::Ignored;
        }
        let before = self.elapsed_ms / 1000;
        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
        // Only ask for a repaint when the digits the player can see change.
        if self.elapsed_ms / 1000 == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    // ── Input ──────────────────────────────────────────────────────────────

    fn key_action(&self, ev: &KeyEvent) -> Option<Action> {
        // A key that is *coming back up* is not a second press. This is fault
        // one, and it is one line.
        if !ev.pressed {
            return None;
        }
        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {
            return None;
        }
        // One check, for both states. The old program had two handlers and put
        // the modifier test in only one of them, so Ctrl-N did nothing during
        // play and dealt a maze on the win screen.
        if self.show_help {
            return match ev.key {
                Key::H | Key::Escape => Some(Action::CloseHelp),
                _ => None,
            };
        }
        match ev.key {
            Key::Up => Some(Action::Step(Dir::North)),
            Key::Down => Some(Action::Step(Dir::South)),
            Key::Left => Some(Action::Step(Dir::West)),
            Key::Right => Some(Action::Step(Dir::East)),
            Key::N | Key::Enter => Some(Action::NewMaze),
            Key::S => Some(Action::ToggleSolution),
            // The next size *in the order the buttons are drawn in*, wrapping.
            // The old program had a hand-written `Difficulty::next` beside a
            // hand-written button row, which is two places for the order to
            // live and so two places for it to disagree.
            Key::D => Some(Action::SetLevel(
                self.level
                    .saturating_add(1)
                    .checked_rem(LEVELS.len())
                    .unwrap_or(0),
            )),
            Key::Num1 => Some(Action::SetLevel(0)),
            Key::Num2 => Some(Action::SetLevel(1)),
            Key::Num3 => Some(Action::SetLevel(2)),
            Key::H | Key::Escape => Some(Action::ToggleHelp),
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
        // No special case for the open sheet: `draw_help` records one hit box
        // over the whole window, last, so the ordinary hit test already answers
        // `ToggleHelp` everywhere while the sheet is up.
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Cell(cell) => self.apply(Action::WalkTo(cell)),
            Target::Level(i) => self.apply(Action::SetLevel(i)),
            Target::NewMaze => self.apply(Action::NewMaze),
            Target::ToggleSolution => self.apply(Action::ToggleSolution),
            Target::ToggleHelp => self.apply(Action::ToggleHelp),
        }
        EventResult::Consumed
    }

    /// Remember the size the window is being drawn at, so the next click is
    /// read against the pixels the player is actually looking at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }
}

impl Default for MazeApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

impl MazeApp {
    /// The frame for a window of the given size, hit boxes and all.
    ///
    /// The drawing pass is what records the hit boxes, so a cell is clickable
    /// exactly where it was drawn and the two cannot drift apart.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_info(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_controls(&mut f, &l);
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        let size = l.big.min(l.header.h * 0.8);
        let y = l.header.y + (l.header.h - text::line_height(size, FontWeightHint::Bold)) / 2.0;
        label(
            f,
            l.header.x + l.pad,
            y,
            "Maze",
            size,
            LAVENDER,
            FontWeightHint::Bold,
            Some((l.header.w - l.pad * 2.0).max(0.0)),
        );
        let name = self.level().described();
        let small = (size * 0.6).max(7.0);
        let w = text::measure(&name, small, FontWeightHint::Bold);
        label(
            f,
            (l.header.right() - l.pad - w).max(l.header.x + l.pad),
            l.header.y + (l.header.h - text::line_height(small, FontWeightHint::Bold)) / 2.0,
            &name,
            small,
            if self.state == GameState::Won {
                GREEN
            } else {
                SUBTEXT0
            },
            FontWeightHint::Bold,
            Some((l.header.w * 0.45).max(0.0)),
        );
    }

    /// What the info line says. Split out so a test can read it without
    /// searching the frame's text commands for a substring.
    #[must_use]
    pub fn info_line(&self) -> String {
        let best = match self.best_at(self.level) {
            Some(b) => format!("best {b}"),
            None => "no best yet".to_string(),
        };
        format!(
            "{}  \u{2022}  {} moves  \u{2022}  {}  \u{2022}  {best}  \u{2022}  out {}",
            self.clock(),
            self.moves,
            self.status,
            self.games_won,
        )
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
        let size = l.font.min(l.info.h * 0.8);
        label(
            f,
            l.info.x + l.pad,
            l.info.y + (l.info.h - text::line_height(size, FontWeightHint::Regular)) / 2.0,
            &self.info_line(),
            size,
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
        let rows = self.maze.rows();
        let cols = self.maze.cols();
        let cell = l.cell(rows, cols);
        if cell <= 0.0 {
            return;
        }
        let route: Vec<(usize, usize)> = if self.show_solution {
            self.solution()
        } else {
            Vec::new()
        };

        // Backgrounds first, then walls, so that a neighbour's fill cannot be
        // drawn over the wall between them.
        for r in 0..rows {
            for c in 0..cols {
                let rect = l.square(rows, cols, r, c);
                let here = (r, c);
                let back = if here == self.player {
                    PLAYER_BG
                } else if here == self.goal {
                    GOAL_BG
                } else if self.on_trail(r, c) {
                    SURFACE1
                } else {
                    SURFACE0
                };
                fill(f, rect, back, 0.0);
                if let Some(i) = self.maze.index(r, c) {
                    f.hit(Target::Cell(i), rect);
                }
            }
        }

        let wall = (cell * 0.1).clamp(1.0, 3.0);
        for r in 0..rows {
            for c in 0..cols {
                let rect = l.square(rows, cols, r, c);
                if self.maze.has_wall(r, c, Dir::North) {
                    line(f, rect.x, rect.y, rect.right(), rect.y, LAVENDER, wall);
                }
                if self.maze.has_wall(r, c, Dir::West) {
                    line(f, rect.x, rect.y, rect.x, rect.bottom(), LAVENDER, wall);
                }
                // Only the far edges of the grid need the other two sides
                // drawn: every wall inside it is the north or west side of the
                // cell below or to the right of it.
                if c.saturating_add(1) == cols && self.maze.has_wall(r, c, Dir::East) {
                    line(
                        f,
                        rect.right(),
                        rect.y,
                        rect.right(),
                        rect.bottom(),
                        LAVENDER,
                        wall,
                    );
                }
                if r.saturating_add(1) == rows && self.maze.has_wall(r, c, Dir::South) {
                    line(
                        f,
                        rect.x,
                        rect.bottom(),
                        rect.right(),
                        rect.bottom(),
                        LAVENDER,
                        wall,
                    );
                }
            }
        }

        // The route, as a dot per cell, drawn over the walls so it reads as one
        // line rather than as a row of unrelated marks.
        for &(r, c) in &route {
            if (r, c) == self.player || (r, c) == self.goal {
                continue;
            }
            let rect = l.square(rows, cols, r, c);
            let d = (cell * 0.3).max(1.0);
            fill(
                f,
                Rect::new(
                    rect.x + (rect.w - d) / 2.0,
                    rect.y + (rect.h - d) / 2.0,
                    d,
                    d,
                ),
                YELLOW,
                d / 2.0,
            );
        }

        let goal = l.square(rows, cols, self.goal.0, self.goal.1);
        let m = cell * 0.22;
        stroke(
            f,
            Rect::new(
                goal.x + m,
                goal.y + m,
                (goal.w - m * 2.0).max(0.0),
                (goal.h - m * 2.0).max(0.0),
            ),
            GREEN,
            (cell * 0.08).clamp(1.0, 3.0),
            (cell * 0.15).max(0.0),
        );

        let player = l.square(rows, cols, self.player.0, self.player.1);
        let pm = cell * 0.2;
        fill(
            f,
            Rect::new(
                player.x + pm,
                player.y + pm,
                (player.w - pm * 2.0).max(0.0),
                (player.h - pm * 2.0).max(0.0),
            ),
            if self.state == GameState::Won {
                GREEN
            } else {
                MAUVE
            },
            (cell * 0.3).max(0.0),
        );

        if self.state == GameState::Won && !self.show_help {
            self.draw_banner(f, l);
        }
    }

    /// The plate that says you are out.
    ///
    /// It records **no hit box**, deliberately: it is a notice, not a sheet, so
    /// the controls under it go on working and a click on the maze still says
    /// what it says. The help sheet is the one thing in this program that
    /// swallows the window, and it is the one thing that covers it opaquely.
    fn draw_banner(&self, f: &mut Frame, l: &Layout) {
        let w = (l.board.w * 0.8).min(320.0);
        let h = (l.board.h * 0.3).min(96.0);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let plate = Rect::new(
            l.board.x + (l.board.w - w) / 2.0,
            l.board.y + (l.board.h - h) / 2.0,
            w,
            h,
        );
        fill(f, plate, CRUST, (h * 0.12).min(12.0));
        stroke(f, plate, GREEN, 1.5, (h * 0.12).min(12.0));
        let title = (h * 0.3).clamp(8.0, l.big);
        centred_in(
            f,
            Rect::new(plate.x, plate.y + h * 0.12, plate.w, h * 0.34),
            "Out!",
            title,
            GREEN,
            FontWeightHint::Bold,
        );
        centred_in(
            f,
            Rect::new(plate.x, plate.y + h * 0.5, plate.w, h * 0.34),
            &format!(
                "{} moves in {} \u{2022} shortest {}",
                self.moves,
                self.clock(),
                self.opening_steps
            ),
            (h * 0.2).clamp(7.0, l.font),
            TEXT_COLOR,
            FontWeightHint::Regular,
        );
    }

    fn draw_controls(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.controls) {
            return;
        }
        // Three sizes, a new maze, the solution, help.
        let count = LEVELS.len().saturating_add(3);
        for (slot, level) in LEVELS.iter().enumerate() {
            let r = l.button(l.controls, slot, count);
            let active = slot == self.level;
            button(
                f,
                l,
                r,
                level.name(),
                if active { SURFACE1 } else { SURFACE0 },
                if active { LAVENDER } else { SUBTEXT0 },
            );
            f.hit(Target::Level(slot), r);
        }
        let new_r = l.button(l.controls, LEVELS.len(), count);
        button(f, l, new_r, "New", SURFACE0, TEAL);
        f.hit(Target::NewMaze, new_r);

        let sol_r = l.button(l.controls, LEVELS.len().saturating_add(1), count);
        button(
            f,
            l,
            sol_r,
            if self.show_solution {
                "Hide"
            } else {
                "Way out"
            },
            if self.show_solution {
                SURFACE1
            } else {
                SURFACE0
            },
            if self.show_solution { PEACH } else { OVERLAY0 },
        );
        f.hit(Target::ToggleSolution, sol_r);

        let help_r = l.button(l.controls, LEVELS.len().saturating_add(2), count);
        button(f, l, help_r, "?", SURFACE0, YELLOW);
        f.hit(Target::ToggleHelp, help_r);
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        let sheet = l.help;
        if sheet.w <= 0.0 || sheet.h <= 0.0 {
            return;
        }
        // One hit box over the *whole window*, recorded after every control the
        // sheet covers, so that the last box at any point is this one. It has
        // to be the window and not merely the sheet: the sheet is opaque and
        // sits on controls that go on recording their own boxes underneath it,
        // so a frame that still answered `Cell(120)` for a point buried under
        // the help text would be describing a screen nobody can see.
        f.hit(Target::ToggleHelp, l.window);
        let radius = (sheet.w * 0.03).min(12.0);
        fill(f, sheet, SURFACE0, radius);
        stroke(f, sheet, LAVENDER, 1.5, radius);

        let pad = l.pad;
        let line_h = (sheet.h - pad * 3.0) / (HELP_ROWS.len().saturating_add(1)) as f32;
        if line_h <= 0.0 {
            return;
        }
        let size = (line_h * 0.62).clamp(6.0, l.font);
        label(
            f,
            sheet.x + pad,
            sheet.y + pad,
            HELP_TITLE,
            (line_h * 0.7).clamp(7.0, l.big),
            YELLOW,
            FontWeightHint::Bold,
            Some((sheet.w - pad * 2.0).max(0.0)),
        );
        let key_w = (sheet.w * 0.4).max(0.0);
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

fn line(f: &mut Frame, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
    if width <= 0.0 {
        return;
    }
    f.push(RenderCommand::Line {
        x1,
        y1,
        x2,
        y2,
        color,
        width,
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
    label(
        f,
        r.x + (r.w - w) / 2.0,
        r.y + (r.h - line_h) / 2.0,
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

/// The one body both the window and the test probe drive, so what a key does
/// in a test is what it does on a screen.
pub fn handle_event(game: &mut MazeApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => game.handle_key(ev),
        Event::Mouse(ev) => game.handle_mouse(ev),
        Event::Tick { elapsed_ms } => game.tick(*elapsed_ms),
        Event::Resize { width, height } => {
            game.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for MazeApp {
    fn title(&self) -> String {
        "Maze".to_string()
    }

    fn app_id(&self) -> String {
        "maze".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Asked after every event, so the clock stops the moment the maze is
    /// solved and starts again the moment another is dealt. An app that leaves
    /// this at the default gets no ticks at all and its clock reads zero for
    /// the life of the process — which is what this one did.
    fn tick_interval(&self) -> Option<Duration> {
        self.clock_running().then_some(TICK)
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

impl Probe for MazeApp {
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
    let mut game = MazeApp::new();
    app::launch("maze", &mut game)
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

    // ── Harness ────────────────────────────────────────────────────────────

    fn game(seed: u64) -> MazeApp {
        MazeApp::with_seed(seed)
    }

    fn sized(seed: u64, size: (f32, f32)) -> MazeApp {
        let mut g = game(seed);
        g.resize(size.0, size.1);
        g
    }

    /// A key going down.
    fn down(g: &mut MazeApp, key: Key) -> EventResult {
        probe::key(g, &probe::press(key))
    }

    /// The same key coming back up. There is no helper for this in `probe`,
    /// and its absence is why every application in this tree that got `pressed`
    /// wrong got it wrong untested.
    fn up_event(key: Key) -> KeyEvent {
        KeyEvent {
            pressed: false,
            text: String::new(),
            ..probe::press(key)
        }
    }

    fn up(g: &mut MazeApp, key: Key) -> EventResult {
        probe::key(g, &up_event(key))
    }

    /// A full keystroke: down, then up. What a window actually delivers.
    fn tap(g: &mut MazeApp, key: Key) {
        down(g, key);
        up(g, key);
    }

    /// Everything about the game a test might want to find unchanged.
    fn describe(g: &MazeApp) -> String {
        let cells = g
            .maze()
            .cells
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
            });
        format!(
            "{:?}|{}|{}|{:?}|{}|{}|{}|{}|{}|{}|{cells:016x}",
            g.player(),
            g.moves(),
            g.level_index(),
            g.state(),
            g.games_won(),
            g.show_solution(),
            g.show_help(),
            g.elapsed_ms(),
            g.opening_steps(),
            g.status(),
        )
    }

    /// Some direction the player may step in from where they stand.
    fn an_open_way(g: &MazeApp) -> Dir {
        let (r, c) = g.player();
        Dir::ALL
            .into_iter()
            .find(|&d| g.maze().open(r, c, d))
            .expect("every cell of a perfect maze has at least one way out")
    }

    fn key_for(dir: Dir) -> Key {
        match dir {
            Dir::North => Key::Up,
            Dir::South => Key::Down,
            Dir::East => Key::Right,
            Dir::West => Key::Left,
        }
    }

    /// Waste moves, one step out and back at a time, until at least `moves`
    /// have been counted. A run padded past the longest route a maze of this
    /// size can have is a run that is worse than any shortest one, on any maze,
    /// without the test having to know which maze it got.
    ///
    /// Bounded, and it says so when the bound is reached. An unbounded `while
    /// g.moves() < moves` reads more naturally and is a trap: a program that
    /// stopped counting moves would spin here forever, and the suite hanging
    /// says only "something is wrong somewhere" where a failed assertion names
    /// the fault. Measured — with the counter's `saturating_add` deleted, the
    /// unbounded version turned six named failures into a timeout.
    fn pad_to(g: &mut MazeApp, moves: u32) {
        for _ in 0..moves.saturating_mul(2) {
            if g.moves() >= moves {
                return;
            }
            assert_eq!(g.state(), GameState::Playing, "padded past the way out");
            let dir = an_open_way(g);
            down(g, key_for(dir));
            down(g, key_for(dir.opposite()));
        }
        panic!(
            "padding stopped at {} of {moves} moves — a step out and back is not being counted",
            g.moves()
        );
    }

    /// Walk the player out by the shortest route, one counted move at a time.
    fn walk_out(g: &mut MazeApp) {
        for _ in 0..10_000 {
            if g.state() == GameState::Won {
                return;
            }
            let route = g.solution();
            let next = route.get(1).copied().expect("a route out exists");
            let (r, c) = g.player();
            let dir = Dir::ALL
                .into_iter()
                .find(|&d| g.maze().step(r, c, d) == Some(next))
                .expect("the next cell of the route is a neighbour");
            down(g, key_for(dir));
        }
        panic!("the walk out never finished");
    }

    // ── The maze itself ────────────────────────────────────────────────────

    /// How many cells can be reached from the corner, and how many walls are
    /// down, worked out here from `has_wall` alone.
    ///
    /// Deliberately not `Maze::distances`, `Maze::step` or `Maze::passages`: a
    /// test that asks the program where the passages are and then checks the
    /// program flooded exactly where it said they were is self-consistent, and
    /// passes however wrong those three are.
    fn flood_and_count(m: &Maze) -> (usize, usize) {
        let (rows, cols) = (m.rows(), m.cols());
        let mut seen = vec![false; rows * cols];
        if seen.is_empty() {
            return (0, 0);
        }
        seen[0] = true;
        let mut stack = vec![(0_usize, 0_usize)];
        while let Some((r, c)) = stack.pop() {
            for d in Dir::ALL {
                let (dr, dc) = d.delta();
                let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                if nr < 0 || nc < 0 || nr as usize >= rows || nc as usize >= cols {
                    continue;
                }
                if m.has_wall(r, c, d) {
                    continue;
                }
                let (nr, nc) = (nr as usize, nc as usize);
                let i = nr * cols + nc;
                if !seen[i] {
                    seen[i] = true;
                    stack.push((nr, nc));
                }
            }
        }
        let reached = seen.iter().filter(|s| **s).count();
        let mut passages = 0_usize;
        for r in 0..rows {
            for c in 0..cols {
                if c + 1 < cols && !m.has_wall(r, c, Dir::East) {
                    passages += 1;
                }
                if r + 1 < rows && !m.has_wall(r, c, Dir::South) {
                    passages += 1;
                }
            }
        }
        (reached, passages)
    }

    #[test]
    fn every_dealt_maze_is_perfect() {
        for level in LEVELS {
            let (rows, cols) = level.size();
            for seed in 1..12_u64 {
                let m = Maze::generate(rows, cols, &mut SeededRng::new(seed));
                let (reached, passages) = flood_and_count(&m);
                assert_eq!(
                    reached,
                    rows * cols,
                    "{} seed {seed}: only {reached} of {} cells can be reached",
                    level.name(),
                    rows * cols
                );
                assert_eq!(
                    passages,
                    rows * cols - 1,
                    "{} seed {seed}: {passages} walls are down, not {}",
                    level.name(),
                    rows * cols - 1
                );
            }
        }
    }

    /// A 3x3 with the eight outer cells joined in a ring and the middle one
    /// sealed off.
    ///
    /// Every maze the generator deals is a *tree*, and a tree hides two whole
    /// classes of bug: there is exactly one route between any two cells, so a
    /// depth-first search and a breadth-first one give the same numbers, and
    /// there is no cycle, so a connectedness check and an edge count agree.
    /// Nothing in `Maze` forbids a ring — `carve` operates on a grid and makes
    /// no promise about trees — so the only way to test the parts of this
    /// program that are *about* shortest routes and about perfection is to
    /// hand-carve the shape the generator never produces.
    fn ring() -> Maze {
        let mut m = Maze::walled(3, 3);
        for (r, c, d) in [
            (0, 0, Dir::East),
            (0, 1, Dir::East),
            (0, 2, Dir::South),
            (1, 2, Dir::South),
            (2, 2, Dir::West),
            (2, 1, Dir::West),
            (2, 0, Dir::North),
            (1, 0, Dir::North),
        ] {
            assert!(m.carve(r, c, d), "({r}, {c}) has no neighbour to the {d:?}");
        }
        m
    }

    #[test]
    fn the_distance_round_a_ring_is_the_shorter_way_round() {
        let m = ring();
        // Two steps along the top, or six the other way about. A search that
        // took the first route it found rather than the shortest would answer
        // six for the first of these and four for the second.
        assert_eq!(m.steps_between((0, 0), (0, 2)), Some(2), "along the top");
        assert_eq!(
            m.steps_between((0, 0), (2, 2)),
            Some(4),
            "to the far corner"
        );
        assert_eq!(m.steps_between((0, 0), (2, 0)), Some(2), "down the left");
        assert_eq!(
            m.steps_between((0, 0), (1, 1)),
            None,
            "the middle is sealed off"
        );
        // The route must agree with the number, or one of them is decoration.
        let route = m.path((0, 0), (0, 2));
        assert_eq!(
            route,
            vec![(0, 0), (0, 1), (0, 2)],
            "the route round the top"
        );
        assert_eq!(m.path((0, 0), (1, 1)), Vec::new(), "no route to the middle");
    }

    #[test]
    fn a_perfect_maze_is_one_reachable_run_with_no_ring_in_it() {
        // Both halves, each caught by the maze the other half passes.
        let m = ring();
        assert_eq!(m.passages(), 8, "eight walls came down");
        assert!(
            !m.is_perfect(),
            "a maze with a cell nothing reaches is not perfect"
        );

        // Connected, and a ring: every cell reachable, one passage too many.
        let mut loop_2x2 = Maze::walled(2, 2);
        for (r, c, d) in [
            (0, 0, Dir::East),
            (0, 1, Dir::South),
            (1, 1, Dir::West),
            (1, 0, Dir::North),
        ] {
            assert!(loop_2x2.carve(r, c, d));
        }
        assert_eq!(loop_2x2.passages(), 4, "four walls came down");
        assert_eq!(
            flood_and_count(&loop_2x2),
            (4, 4),
            "the ring is reachable throughout"
        );
        assert!(
            !loop_2x2.is_perfect(),
            "a maze you can walk in a circle is not perfect"
        );

        // And the shape the generator actually deals.
        for level in LEVELS {
            let (rows, cols) = level.size();
            let dealt = Maze::generate(rows, cols, &mut SeededRng::new(77));
            assert_eq!(dealt.passages(), rows * cols - 1, "{}", level.name());
            assert!(dealt.is_perfect(), "{} is not perfect", level.name());
        }
    }

    #[test]
    fn a_wall_reads_the_same_from_both_of_the_cells_it_stands_between() {
        let m = Maze::generate(12, 9, &mut SeededRng::new(5));
        for r in 0..m.rows() {
            for c in 0..m.cols() {
                for d in Dir::ALL {
                    let Some((nr, nc)) = m.step(r, c, d) else {
                        continue;
                    };
                    assert_eq!(
                        m.has_wall(r, c, d),
                        m.has_wall(nr, nc, d.opposite()),
                        "the wall between ({r}, {c}) and ({nr}, {nc}) is only there from one side"
                    );
                }
            }
        }
    }

    #[test]
    fn the_border_of_a_dealt_maze_is_solid() {
        let m = Maze::generate(8, 11, &mut SeededRng::new(3));
        for c in 0..m.cols() {
            assert!(m.has_wall(0, c, Dir::North), "a hole in the top at {c}");
            assert!(
                m.has_wall(m.rows() - 1, c, Dir::South),
                "a hole in the bottom at {c}"
            );
        }
        for r in 0..m.rows() {
            assert!(m.has_wall(r, 0, Dir::West), "a hole in the left at {r}");
            assert!(
                m.has_wall(r, m.cols() - 1, Dir::East),
                "a hole in the right at {r}"
            );
        }
    }

    #[test]
    fn a_neighbour_off_the_grid_is_no_neighbour() {
        let m = Maze::walled(3, 4);
        assert_eq!(m.step(0, 0, Dir::North), None);
        assert_eq!(m.step(0, 0, Dir::West), None);
        assert_eq!(m.step(2, 3, Dir::South), None);
        assert_eq!(m.step(2, 3, Dir::East), None);
        assert_eq!(m.step(1, 1, Dir::East), Some((1, 2)));
        assert_eq!(m.step(1, 1, Dir::North), Some((0, 1)));
        // And a cell that is not on the grid at all has no neighbours and every
        // wall, rather than an index into somebody else's row.
        assert_eq!(m.step(9, 9, Dir::West), None);
        assert!(m.has_wall(9, 9, Dir::West));
        assert_eq!(m.index(3, 0), None, "row 3 of a 3-row maze is not a row");
        assert_eq!(
            m.index(0, 4),
            None,
            "column 4 of a 4-column maze is not one"
        );
    }

    #[test]
    fn a_wall_bit_cleared_at_the_border_does_not_walk_the_search_off_the_grid() {
        // Fault six. The old BFS computed `(r as i32 + dr) as usize` with no
        // bounds test and leaned entirely on the border never being carved: a
        // north step out of row 0 gave `usize::MAX`, and `row * cols + col`
        // turned that into a perfectly ordinary index into another cell rather
        // than into a panic anybody would notice. Nothing in the type stops a
        // border bit being cleared, so the guard belongs in the step, not in
        // the habits of the generator.
        let mut m = Maze::generate(4, 4, &mut SeededRng::new(11));
        m.cells[0] &= !Dir::North.bit();
        assert!(!m.has_wall(0, 0, Dir::North), "the bit really is off");
        assert!(
            !m.open(0, 0, Dir::North),
            "a wall that is down at the edge of the grid is still the edge of the grid"
        );
        let d = m.distances((0, 0));
        assert_eq!(d.len(), 16, "the search sized its answer to the grid");
        assert!(
            d.iter().all(Option::is_some),
            "every cell is still reachable"
        );
    }

    #[test]
    fn the_route_out_is_a_run_of_open_steps_and_is_as_long_as_the_distance() {
        let m = Maze::generate(14, 14, &mut SeededRng::new(9));
        let ends = [((0, 0), (13, 13)), ((3, 7), (11, 2)), ((13, 0), (0, 13))];
        for (from, to) in ends {
            let route = m.path(from, to);
            assert_eq!(
                route.first().copied(),
                Some(from),
                "{from:?} is not the start"
            );
            assert_eq!(route.last().copied(), Some(to), "{to:?} is not the end");
            for pair in route.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let joined = Dir::ALL
                    .into_iter()
                    .any(|d| m.open(a.0, a.1, d) && m.step(a.0, a.1, d) == Some(b));
                assert!(joined, "the route jumps from {a:?} to {b:?} through a wall");
            }
            assert_eq!(
                m.steps_between(from, to),
                Some(route.len() as u32 - 1),
                "{from:?} to {to:?}: the distance and the route disagree"
            );
        }
    }

    #[test]
    fn a_cell_with_no_route_to_it_has_no_distance_and_no_route() {
        // A grid nothing has carved: every cell is its own island.
        let m = Maze::walled(3, 3);
        assert_eq!(m.steps_between((0, 0), (2, 2)), None);
        assert!(m.path((0, 0), (2, 2)).is_empty());
        assert_eq!(m.steps_between((0, 0), (0, 0)), Some(0));
        assert_eq!(m.path((0, 0), (0, 0)), vec![(0, 0)]);
    }

    #[test]
    fn the_same_seed_deals_the_same_maze_and_two_seeds_do_not() {
        let a = Maze::generate(10, 10, &mut SeededRng::new(77));
        let b = Maze::generate(10, 10, &mut SeededRng::new(77));
        let c = Maze::generate(10, 10, &mut SeededRng::new(78));
        assert_eq!(a, b, "the same seed dealt two different mazes");
        assert_ne!(a, c, "two seeds dealt the same maze");
    }

    // ── Walking ────────────────────────────────────────────────────────────

    #[test]
    fn an_arrow_key_moves_one_cell_and_counts_one_move() {
        let mut g = game(4);
        let dir = an_open_way(&g);
        let from = g.player();
        let expected = g.maze().step(from.0, from.1, dir).unwrap();
        down(&mut g, key_for(dir));
        assert_eq!(
            g.player(),
            expected,
            "the step did not land where it should"
        );
        assert_eq!(g.moves(), 1, "one keypress, {} moves", g.moves());
    }

    #[test]
    fn a_key_that_comes_back_up_is_not_a_second_press() {
        // Fault one. The old handlers destructured `KeyEvent { key, .. }`, and
        // the field the `..` swallowed was `pressed`, so every key ran on the
        // way down and again on the way up. A full keystroke — which is what a
        // window delivers — must do its thing once.
        let mut g = game(4);
        let dir = an_open_way(&g);
        let one = g.maze().step(g.player().0, g.player().1, dir).unwrap();
        tap(&mut g, key_for(dir));
        assert_eq!(
            g.player(),
            one,
            "a single keystroke walked {} cells",
            g.moves()
        );
        assert_eq!(
            g.moves(),
            1,
            "a single keystroke counted {} moves",
            g.moves()
        );
    }

    #[test]
    fn the_release_of_every_key_the_program_answers_does_nothing() {
        // Not only the arrows: the release ran *whatever* the key did, and for
        // three of them that was worse than running it twice. `S` turned the
        // solution on and straight back off, so there was no state of the
        // program in which it showed; `D` walked two sizes, so the sizes went
        // round backwards; `N` dealt two mazes and threw the first away.
        for key in [
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::N,
            Key::S,
            Key::D,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::H,
            Key::Escape,
            Key::Enter,
        ] {
            let mut g = game(4);
            let before = describe(&g);
            let outcome = up(&mut g, key);
            assert_eq!(
                outcome,
                EventResult::Ignored,
                "{key:?} coming back up was answered"
            );
            assert_eq!(
                describe(&g),
                before,
                "{key:?} coming back up changed the game"
            );
        }
    }

    #[test]
    fn the_solution_can_be_seen_after_a_whole_keystroke() {
        // The consequence of fault one that had no reachable state at all: `S`
        // on the way down turned the overlay on and `S` on the way up turned it
        // off again, with no frame in between.
        let mut g = game(6);
        assert!(!g.show_solution(), "it starts hidden");
        tap(&mut g, Key::S);
        assert!(
            g.show_solution(),
            "a whole keystroke on S left the way out hidden"
        );
        let shown = probe::Probe::draw(&g, MazeApp::SIZE);
        tap(&mut g, Key::S);
        assert!(
            !g.show_solution(),
            "a second keystroke did not hide it again"
        );
        let hidden = probe::Probe::draw(&g, MazeApp::SIZE);
        assert_ne!(
            shown.commands().len(),
            hidden.commands().len(),
            "showing the way out drew exactly the same frame as hiding it"
        );
    }

    #[test]
    fn walking_into_a_wall_moves_nothing_and_counts_nothing() {
        let mut g = game(4);
        let (r, c) = g.player();
        let walled = Dir::ALL
            .into_iter()
            .find(|&d| !g.maze().open(r, c, d))
            .expect("a corner cell of a perfect maze has at least two walls");
        let before = describe(&g);
        down(&mut g, key_for(walled));
        assert_eq!(g.player(), (r, c), "a wall let the player through");
        assert_eq!(g.moves(), 0, "a wall counted a move");
        assert_ne!(before, describe(&g), "a wall said nothing at all");
        assert_eq!(g.status(), "A wall");
    }

    #[test]
    fn the_steps_left_is_the_distance_to_the_goal_from_where_the_player_stands() {
        // Fault seven: the number shown was worked out once, in `new_maze`,
        // from `(0, 0)`, and went on describing that journey for the rest of
        // the game. Written out here rather than asked of `steps_between`, so
        // that a broken distance cannot make the check agree with itself.
        let mut g = game(8);
        for _ in 0..25 {
            if g.state() == GameState::Won {
                break;
            }
            let (reached, _) = flood_and_count(g.maze());
            assert_eq!(reached, g.maze().cell_count(), "the maze came apart");
            let route = g.maze().path(g.player(), g.goal());
            assert_eq!(
                g.steps_left(),
                Some(route.len() as u32 - 1),
                "at {:?} the count of steps left is not the length of the way out",
                g.player()
            );
            let next = route[1];
            let dir = Dir::ALL
                .into_iter()
                .find(|&d| g.maze().step(g.player().0, g.player().1, d) == Some(next))
                .unwrap();
            let before = g.steps_left().unwrap();
            down(&mut g, key_for(dir));
            assert_eq!(
                g.steps_left(),
                Some(before - 1),
                "a step along the way out did not bring the goal one closer"
            );
        }
    }

    #[test]
    fn the_way_out_shown_starts_from_where_the_player_stands() {
        let mut g = game(8);
        assert_eq!(g.solution().first().copied(), Some(g.player()));
        let dir = an_open_way(&g);
        down(&mut g, key_for(dir));
        assert_eq!(
            g.solution().first().copied(),
            Some(g.player()),
            "the way out still starts at the corner the player has left"
        );
        assert_eq!(g.solution().last().copied(), Some(g.goal()));
    }

    #[test]
    fn reaching_the_goal_wins_and_counts_the_run() {
        let mut g = game(12);
        assert_eq!(g.state(), GameState::Playing);
        assert_eq!(g.games_won(), 0);
        walk_out(&mut g);
        assert_eq!(g.player(), g.goal(), "the walk did not reach the goal");
        assert_eq!(g.state(), GameState::Won);
        assert_eq!(g.games_won(), 1);
        assert_eq!(g.steps_left(), Some(0));
        // Walked by the shortest route, so the count and the opening distance
        // are the same number — which is what makes fault one's doubling
        // visible rather than merely suspicious.
        assert_eq!(
            g.moves(),
            g.opening_steps(),
            "a perfect run counted {} moves for a {}-step maze",
            g.moves(),
            g.opening_steps()
        );
    }

    #[test]
    fn a_won_maze_ignores_further_steps() {
        let mut g = game(12);
        walk_out(&mut g);
        let after = describe(&g);
        for key in [Key::Up, Key::Down, Key::Left, Key::Right] {
            let mut h = game(12);
            walk_out(&mut h);
            down(&mut h, key);
            assert_eq!(h.player(), h.goal(), "{key:?} moved a player who is out");
            assert_eq!(h.moves(), g.moves(), "{key:?} counted a move after the win");
        }
        assert_eq!(describe(&g), after);
    }

    #[test]
    fn the_best_is_kept_for_each_size_on_its_own() {
        // Fault eight: one number covered all three sizes, so a 10x10 solved in
        // 18 moves set a target a 30x30 could never come near, and the win line
        // presented the two as if they were comparable.
        let mut g = game(21);
        assert_eq!(g.best_at(0), None);
        walk_out(&mut g);
        let small = g.moves();
        assert_eq!(g.best_at(0), Some(small));
        assert_eq!(g.best_at(1), None, "solving a small maze set a medium best");
        assert_eq!(g.best_at(2), None, "solving a small maze set a large best");
        down(&mut g, Key::Num2);
        assert_eq!(g.level(), Difficulty::Medium);
        walk_out(&mut g);
        assert!(
            g.moves() > small,
            "the medium maze wanted fewer moves than the small one, which makes this a poor test"
        );
        assert_eq!(g.best_at(0), Some(small), "the small best was overwritten");
        assert_eq!(g.best_at(1), Some(g.moves()));
    }

    #[test]
    fn a_better_run_lowers_the_best_and_a_worse_one_leaves_it() {
        // The worse run has to come *first*, and it has to be worse by
        // construction rather than by luck. Two runs on two different mazes are
        // not comparable — the first draft walked both out by the shortest
        // route and asserted the best was the lower of the two, which is true
        // whichever way the mazes happened to fall, and so passed against a
        // program that overwrote the best every time. A padded run of at least
        // 150 moves cannot be beaten by a shortest route on a 10x10, where the
        // longest possible route is 99 steps.
        let mut g = game(31);
        pad_to(&mut g, 150);
        walk_out(&mut g);
        let padded = g.moves();
        assert!(padded >= 150, "the padded run was only {padded} moves");
        assert_eq!(
            g.best_at(0),
            Some(padded),
            "the first run out sets the best"
        );

        // A fresh maze walked straight out: fewer moves than the padded run,
        // so the best must come down to it.
        down(&mut g, Key::N);
        walk_out(&mut g);
        let straight = g.moves();
        assert_eq!(straight, g.opening_steps(), "the shortest way was taken");
        assert!(straight < padded, "{straight} is not better than {padded}");
        assert_eq!(g.best_at(0), Some(straight), "a better run lowers the best");

        // A third maze, padded again: worse than the straight run, and the
        // best must not move.
        down(&mut g, Key::N);
        pad_to(&mut g, 150);
        walk_out(&mut g);
        assert!(
            g.moves() > straight,
            "{} is not worse than {straight}",
            g.moves()
        );
        assert_eq!(
            g.best_at(0),
            Some(straight),
            "a {}-move run overwrote a best of {straight}",
            g.moves()
        );
    }

    // ── The clock ──────────────────────────────────────────────────────────

    #[test]
    fn the_clock_advances_by_the_time_a_tick_reports() {
        // Fault two. `elapsed_secs` was set to zero and read for display and
        // incremented nowhere, and the function that formatted it was never
        // called by `render` either, so the advertised timer had two separate
        // reasons to read 0:00 forever.
        let mut g = game(5);
        assert_eq!(g.clock(), "0:00");
        handle_event(&mut g, &Event::Tick { elapsed_ms: 900 });
        assert_eq!(g.clock(), "0:00", "under a second is not a second");
        handle_event(&mut g, &Event::Tick { elapsed_ms: 200 });
        assert_eq!(g.clock(), "0:01");
        handle_event(&mut g, &Event::Tick { elapsed_ms: 58_900 });
        assert_eq!(g.clock(), "1:00");
        handle_event(
            &mut g,
            &Event::Tick {
                elapsed_ms: 600_000,
            },
        );
        assert_eq!(g.clock(), "11:00");
    }

    #[test]
    fn the_clock_counts_the_time_that_passed_not_the_interval_it_asked_for() {
        // Two ticks that each report five seconds must move the clock ten
        // seconds, not two quarter-seconds. A clock driven by `TICK` rather
        // than by `elapsed_ms` runs slow by however busy the loop was, and does
        // it silently.
        let mut g = game(5);
        handle_event(&mut g, &Event::Tick { elapsed_ms: 5_000 });
        handle_event(&mut g, &Event::Tick { elapsed_ms: 5_000 });
        assert_eq!(g.elapsed_ms(), 10_000);
        assert_eq!(g.clock(), "0:10");
        assert_ne!(
            g.elapsed_ms(),
            TICK.as_millis() as u64 * 2,
            "the clock counted the interval it asked for"
        );
    }

    #[test]
    fn a_tick_asks_for_a_frame_only_when_the_digits_change() {
        let mut g = game(5);
        assert_eq!(
            handle_event(&mut g, &Event::Tick { elapsed_ms: 250 }),
            EventResult::Ignored,
            "a quarter second redrew a clock that reads the same"
        );
        assert_eq!(
            handle_event(&mut g, &Event::Tick { elapsed_ms: 800 }),
            EventResult::Consumed,
            "the second turned over and nothing was redrawn"
        );
    }

    #[test]
    fn the_clock_runs_while_playing_and_stops_once_you_are_out() {
        let mut g = game(12);
        assert_eq!(
            App::tick_interval(&g),
            Some(TICK),
            "a program that ages a clock and asks for no tick has no clock"
        );
        handle_event(&mut g, &Event::Tick { elapsed_ms: 3_000 });
        walk_out(&mut g);
        let stopped = g.elapsed_ms();
        assert_eq!(
            App::tick_interval(&g),
            None,
            "the clock went on being wound after the maze was solved"
        );
        assert_eq!(
            handle_event(&mut g, &Event::Tick { elapsed_ms: 9_000 }),
            EventResult::Ignored
        );
        assert_eq!(g.elapsed_ms(), stopped, "a solved maze went on timing");
    }

    #[test]
    fn a_fresh_maze_starts_the_clock_and_the_count_again() {
        let mut g = game(12);
        handle_event(&mut g, &Event::Tick { elapsed_ms: 30_000 });
        walk_out(&mut g);
        let won = g.games_won();
        down(&mut g, Key::N);
        assert_eq!(g.elapsed_ms(), 0, "the clock carried over to the next maze");
        assert_eq!(g.moves(), 0);
        assert_eq!(g.player(), (0, 0));
        assert_eq!(g.state(), GameState::Playing);
        assert!(
            !g.show_solution(),
            "the way out stayed marked on a fresh maze"
        );
        assert_eq!(g.games_won(), won, "a fresh maze counted a win");
        assert_eq!(
            App::tick_interval(&g),
            Some(TICK),
            "the clock did not start again"
        );
    }

    #[test]
    fn a_dealt_maze_is_not_already_out() {
        for level in LEVELS {
            for seed in 1..8_u64 {
                let mut g = game(seed);
                g.apply(Action::SetLevel(
                    LEVELS.iter().position(|&l| l == level).unwrap(),
                ));
                assert_eq!(
                    g.state(),
                    GameState::Playing,
                    "{} seed {seed}",
                    level.name()
                );
                assert!(g.opening_steps() > 0, "{} seed {seed}", level.name());
            }
        }
    }

    // ── The pointer ────────────────────────────────────────────────────────

    #[test]
    fn a_cell_is_clickable_exactly_where_it_is_drawn() {
        // Fault four: there was no pointer input at all, and the mouse types
        // were imported anyway — which `#![allow(unused_imports)]` kept quiet.
        for size in [MazeApp::SIZE, (520.0, 480.0), (1200.0, 700.0)] {
            let g = sized(3, size);
            let l = g.layout();
            let (rows, cols) = (g.maze().rows(), g.maze().cols());
            let f = g.frame(size.0, size.1);
            for r in 0..rows {
                for c in 0..cols {
                    let rect = l.square(rows, cols, r, c);
                    let (x, y) = rect.centre();
                    let i = g.maze().index(r, c).unwrap();
                    assert_eq!(
                        f.hit_test(x, y),
                        Some(Target::Cell(i)),
                        "{size:?}: ({r}, {c}) is drawn at {rect:?} and answers somewhere else"
                    );
                }
            }
        }
    }

    #[test]
    fn a_click_walks_to_the_cell_it_lands_on_by_the_shortest_route() {
        let mut g = sized(3, MazeApp::SIZE);
        let (rows, cols) = (g.maze().rows(), g.maze().cols());
        let target = (rows / 2, cols / 2);
        let want = g.maze().steps_between(g.player(), target).unwrap();
        assert!(want > 1, "a one-step walk would not test much");
        let i = g.maze().index(target.0, target.1).unwrap();
        probe::click(&mut g, Target::Cell(i));
        assert_eq!(g.player(), target, "the click did not walk there");
        assert_eq!(
            g.moves(),
            want,
            "the walk cost {} moves for a {want}-step journey",
            g.moves()
        );
    }

    #[test]
    fn a_walk_that_passes_the_way_out_stops_there() {
        // The shortest route to some cells runs through the goal. Walking on
        // past the exit because the click asked for somewhere else would be a
        // win the player never saw.
        let mut g = sized(3, MazeApp::SIZE);
        let goal = g.goal();
        // Any cell whose route from the corner passes through the goal: a
        // neighbour of the goal that is further from the corner than it is.
        let beyond = (0..g.maze().rows())
            .flat_map(|r| (0..g.maze().cols()).map(move |c| (r, c)))
            .find(|&cell| cell != goal && g.maze().path((0, 0), cell).contains(&goal));
        let Some(beyond) = beyond else {
            // Nothing lies beyond the far corner in this maze; the case is not
            // reachable here and the test has nothing to say.
            return;
        };
        let i = g.maze().index(beyond.0, beyond.1).unwrap();
        probe::click(&mut g, Target::Cell(i));
        assert_eq!(g.player(), goal, "the walk carried on past the exit");
        assert_eq!(g.state(), GameState::Won);
    }

    #[test]
    fn clicking_where_you_stand_says_so_rather_than_counting_a_move() {
        let mut g = sized(3, MazeApp::SIZE);
        let i = g.maze().index(g.player().0, g.player().1).unwrap();
        probe::click(&mut g, Target::Cell(i));
        assert_eq!(g.moves(), 0);
        assert_eq!(g.status(), "You are standing there");
    }

    #[test]
    fn a_button_the_program_does_not_use_walks_nowhere() {
        for button in [MouseButton::Right, MouseButton::Middle, MouseButton::Back] {
            let mut g = sized(3, MazeApp::SIZE);
            let before = describe(&g);
            let i = g.maze().index(4, 4).unwrap();
            let outcome = probe::click_with(&mut g, Target::Cell(i), button);
            assert_eq!(outcome, EventResult::Ignored, "{button:?} was answered");
            assert_eq!(describe(&g), before, "{button:?} moved the player");
        }
    }

    #[test]
    fn a_click_is_read_against_the_size_last_drawn() {
        let mut g = game(3);
        let mut differed = 0;
        for step in 1..10 {
            let (x, y) = (60.0 * step as f32, 55.0 * step as f32);
            g.resize(1100.0, 900.0);
            let big = g.target_at(x, y);
            g.resize(430.0, 400.0);
            let small = g.target_at(x, y);
            if big != small {
                differed += 1;
            }
        }
        assert!(
            differed > 0,
            "the same point named the same target in a 1100x900 window and a 430x400 one"
        );
    }

    #[test]
    fn rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against() {
        let mut g = game(3);
        let size = (640.0, 610.0);
        let _tree = App::render(&mut g, size.0, size.1);
        assert_eq!(g.layout().window, Rect::new(0.0, 0.0, size.0, size.1));
        let l = g.layout();
        let rect = l.square(g.maze().rows(), g.maze().cols(), 5, 5);
        let (x, y) = rect.centre();
        assert_eq!(
            g.target_at(x, y),
            Some(Target::Cell(g.maze().index(5, 5).unwrap())),
            "the click was read against a size the window is not drawn at"
        );
    }

    #[test]
    fn a_resize_event_is_what_a_window_sends_and_it_is_believed() {
        let mut g = game(3);
        handle_event(
            &mut g,
            &Event::Resize {
                width: 500,
                height: 480,
            },
        );
        assert_eq!(g.layout().window, Rect::new(0.0, 0.0, 500.0, 480.0));
    }

    // ── Controls ───────────────────────────────────────────────────────────

    #[test]
    fn a_size_button_switches_to_the_size_it_names() {
        for (slot, level) in LEVELS.iter().enumerate() {
            let mut g = sized(3, MazeApp::SIZE);
            probe::click(&mut g, Target::Level(slot));
            assert_eq!(g.level(), *level, "button {slot} did not select {level:?}");
            let (rows, cols) = level.size();
            assert_eq!((g.maze().rows(), g.maze().cols()), (rows, cols));
            assert_eq!(g.goal(), (rows - 1, cols - 1));
        }
    }

    #[test]
    fn choosing_the_size_you_are_already_on_says_so_rather_than_redealing() {
        let mut g = sized(3, MazeApp::SIZE);
        let maze = g.maze().clone();
        probe::click(&mut g, Target::Level(0));
        assert_eq!(g.maze(), &maze, "a redeal you did not ask for");
        assert_eq!(g.status(), "Already Small");
    }

    #[test]
    fn the_size_key_walks_the_sizes_in_the_order_the_buttons_name_them() {
        // Fault one again: `D` ran `Difficulty::next` twice, so the sizes went
        // round the other way — Small, Large, Medium — and each press dealt two
        // mazes and discarded the first.
        let mut g = game(3);
        let mut seen = vec![g.level_index()];
        for _ in 0..LEVELS.len() {
            tap(&mut g, Key::D);
            seen.push(g.level_index());
        }
        assert_eq!(
            seen,
            vec![0, 1, 2, 0],
            "the size key walked {seen:?}, not the order the buttons are drawn in"
        );
    }

    #[test]
    fn the_new_maze_button_deals_another_and_starts_the_count_again() {
        let mut g = sized(3, MazeApp::SIZE);
        let dir = an_open_way(&g);
        down(&mut g, key_for(dir));
        let first = g.maze().clone();
        probe::click(&mut g, Target::NewMaze);
        assert_ne!(g.maze(), &first, "the same maze was dealt again");
        assert_eq!(g.moves(), 0);
        assert_eq!(g.player(), (0, 0));
    }

    #[test]
    fn the_way_out_button_marks_it_and_it_stays_marked() {
        let mut g = sized(3, MazeApp::SIZE);
        assert!(!g.show_solution());
        probe::click(&mut g, Target::ToggleSolution);
        assert!(g.show_solution());
        let dir = an_open_way(&g);
        down(&mut g, key_for(dir));
        assert!(g.show_solution(), "a step turned the way out back off");
        probe::click(&mut g, Target::ToggleSolution);
        assert!(!g.show_solution());
    }

    #[test]
    fn a_modifier_the_program_does_not_use_is_ignored() {
        // Fault five: `handle_won` had no modifier check while `handle_playing`
        // did, so Ctrl-N did nothing during play and dealt a fresh maze the
        // moment you were out.
        for won in [false, true] {
            let mut g = sized(12, MazeApp::SIZE);
            if won {
                walk_out(&mut g);
            }
            let before = describe(&g);
            for key in [Key::N, Key::S, Key::D, Key::Enter, Key::Up] {
                for modifiers in [
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
                    let outcome = probe::key(&mut g, &probe::press_with(key, modifiers));
                    assert_eq!(
                        outcome,
                        EventResult::Ignored,
                        "won={won}: {modifiers:?}+{key:?} was answered"
                    );
                    assert_eq!(
                        describe(&g),
                        before,
                        "won={won}: {modifiers:?}+{key:?} changed the game"
                    );
                }
            }
        }
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    fn sizes() -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for w in [120.0_f32, 260.0, 420.0, 780.0, 1280.0, 1920.0] {
            for h in [90.0_f32, 200.0, 380.0, 740.0, 1080.0] {
                out.push((w, h));
            }
        }
        out
    }

    #[test]
    fn the_maze_is_square_in_every_window() {
        for (w, h) in sizes() {
            let l = Layout::new(w, h);
            assert!(
                (l.board.w - l.board.h).abs() < 0.001,
                "{w}x{h}: the maze is {}x{}",
                l.board.w,
                l.board.h
            );
        }
    }

    #[test]
    fn the_maze_keeps_its_share_of_every_window() {
        // Fault three: the maze drew at a cell size read from the difficulty,
        // at a fixed offset, so in a 1920-pixel window a Large maze was 450
        // pixels of grid in a corner and in a 400-pixel one it ran off both
        // edges. Its height is now at least its promised share of the window's,
        // or as much of that as the window is wide enough to hold.
        for (w, h) in sizes() {
            let l = Layout::new(w, h);
            let promised = (h * BOARD_SHARE).min(w - l.pad * 2.0).max(0.0);
            assert!(
                l.board.h >= promised - 0.001,
                "{w}x{h}: the maze got {} of the {promised} it is promised",
                l.board.h
            );
        }
    }

    #[test]
    fn the_bands_go_in_the_stated_order() {
        // The order each band first appears in as the window grows.
        let bands = |l: &Layout| [l.header, l.info, l.controls];
        let mut order: Vec<usize> = Vec::new();
        let mut showing = [false; 3];
        for step in 0..400 {
            let h = 30.0 + step as f32 * 4.0;
            let l = Layout::new(900.0, h);
            for (i, band) in bands(&l).into_iter().enumerate() {
                let now = band != Rect::EMPTY;
                if now && !showing[i] {
                    order.push(i);
                }
                showing[i] = now;
            }
        }
        // Written out, not derived from `BAND_DROP_ORDER`. Reading the constant
        // back and reversing it makes the test agree with whatever the constant
        // happens to say, so it would pass on any order at all — a test of
        // arithmetic rather than of the layout. The bands are
        // [header, info, controls], so as the window grows the info line comes
        // first (the clock and the steps left, which nothing else says), then
        // the controls, and the title — the one band that names a program you
        // are looking at — last.
        assert_eq!(
            order,
            vec![1_usize, 2, 0],
            "the bands appear in {order:?}: info, then controls, then the title"
        );
        // And the constant that drives the dropping says the same thing
        // backwards, so the two cannot drift apart unnoticed.
        let mut from_const: Vec<usize> = BAND_DROP_ORDER.to_vec();
        from_const.reverse();
        assert_eq!(from_const, order, "BAND_DROP_ORDER no longer matches");
    }

    #[test]
    fn the_maze_is_still_playable_in_a_window_too_small_for_the_chrome() {
        let size = (200.0_f32, 40.0_f32);
        let l = Layout::new(size.0, size.1);
        assert_eq!(
            [l.header, l.info, l.controls],
            [Rect::EMPTY; 3],
            "the chrome did not get out of the way"
        );
        assert!(
            l.board.w > 0.0 && l.board.h > 0.0,
            "and neither did the maze"
        );

        let mut g = sized(3, size);
        let dir = an_open_way(&g);
        let want = g.maze().step(g.player().0, g.player().1, dir).unwrap();
        probe::key(&mut g, &probe::press(key_for(dir)));
        assert_eq!(g.player(), want, "the keyboard stopped working");

        let mut h = sized(3, size);
        let i = h.maze().index(want.0, want.1).unwrap();
        let rect = h
            .layout()
            .square(h.maze().rows(), h.maze().cols(), want.0, want.1);
        let (x, y) = rect.centre();
        h.click_at(x, y, MouseButton::Left, size);
        assert_eq!(h.player(), want, "the pointer stopped working");
        assert_eq!(h.target_at(x, y), Some(Target::Cell(i)));
    }

    // ── The help sheet ─────────────────────────────────────────────────────

    /// Which keys each row of the sheet names.
    ///
    /// The first component of every entry is checked against [`HELP_ROWS`], so
    /// this table cannot quietly describe a sheet the program no longer draws.
    const SHEET_KEYS: [(&str, &[Key]); 8] = [
        ("Arrows", &[Key::Up, Key::Down, Key::Left, Key::Right]),
        ("Click a cell", &[]),
        ("N", &[Key::N]),
        ("S", &[Key::S]),
        ("D", &[Key::D]),
        ("1 / 2 / 3", &[Key::Num1, Key::Num2, Key::Num3]),
        ("H / Esc", &[Key::H, Key::Escape]),
        ("Enter", &[Key::Enter]),
    ];

    /// Every key a keyboard has that this program might plausibly be asked
    /// about, so the sweep below cannot miss one by not thinking of it.
    fn every_key() -> Vec<Key> {
        let mut keys = vec![
            Key::A,
            Key::B,
            Key::C,
            Key::D,
            Key::E,
            Key::F,
            Key::G,
            Key::H,
            Key::I,
            Key::J,
            Key::K,
            Key::L,
            Key::M,
            Key::N,
            Key::O,
            Key::P,
            Key::Q,
            Key::R,
            Key::S,
            Key::T,
            Key::U,
            Key::V,
            Key::W,
            Key::X,
            Key::Y,
            Key::Z,
        ];
        keys.extend([
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ]);
        keys.extend([
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Backspace,
            Key::Delete,
            Key::Insert,
            Key::Enter,
            Key::Tab,
            Key::Escape,
            Key::Space,
        ]);
        keys.extend([
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
            Key::F11,
            Key::F12,
        ]);
        keys
    }

    #[test]
    fn every_key_the_sheet_names_answers_and_every_key_that_answers_is_named() {
        // The old help bar named four keys — arrows, N, H, D — and the program
        // answered exactly those four, but nothing checked it, and nothing
        // would have noticed either half drifting. Checked both ways here.
        let named: Vec<&str> = SHEET_KEYS.iter().map(|(row, _)| *row).collect();
        let drawn: Vec<&str> = HELP_ROWS.iter().map(|(row, _)| *row).collect();
        assert_eq!(named, drawn, "this table describes a sheet nobody draws");
        assert!(
            SHEET_KEYS[1].1.is_empty() && SHEET_KEYS[1].0.starts_with("Click"),
            "the one row that names no key is the pointer's"
        );

        let mut expected: Vec<String> = SHEET_KEYS
            .iter()
            .flat_map(|(_, keys)| keys.iter().map(|k| format!("{k:?}")))
            .collect();
        expected.sort();

        let mut answered: Vec<String> = Vec::new();
        for key in every_key() {
            let mut g = sized(3, MazeApp::SIZE);
            if down(&mut g, key) == EventResult::Consumed {
                answered.push(format!("{key:?}"));
            }
        }
        answered.sort();
        assert_eq!(
            answered, expected,
            "the sheet and the program answer different sets of keys"
        );
    }

    #[test]
    fn every_key_the_sheet_names_as_closing_it_closes_it() {
        for key in SHEET_KEYS[6].1 {
            let mut g = sized(3, MazeApp::SIZE);
            g.apply(Action::ToggleHelp);
            assert!(g.show_help());
            let outcome = down(&mut g, *key);
            assert_eq!(outcome, EventResult::Consumed, "{key:?} was not answered");
            assert!(!g.show_help(), "{key:?} did not close the sheet");
        }
    }

    #[test]
    fn the_open_sheet_swallows_the_keys_that_are_not_about_it() {
        let closers = SHEET_KEYS[6].1;
        for key in every_key() {
            if closers.contains(&key) {
                continue;
            }
            let mut g = sized(3, MazeApp::SIZE);
            g.apply(Action::ToggleHelp);
            let before = describe(&g);
            let outcome = down(&mut g, key);
            assert_eq!(
                outcome,
                EventResult::Ignored,
                "{key:?} was answered through the sheet"
            );
            assert_eq!(describe(&g), before, "{key:?} reached the game behind it");
        }
    }

    #[test]
    fn while_the_sheet_is_up_the_frame_says_every_point_belongs_to_the_sheet() {
        let mut g = sized(3, MazeApp::SIZE);
        g.apply(Action::ToggleHelp);
        let (w, h) = MazeApp::SIZE;
        let f = g.frame(w, h);
        for i in 0..20 {
            for j in 0..20 {
                let (x, y) = (w * (i as f32 + 0.5) / 20.0, h * (j as f32 + 0.5) / 20.0);
                assert_eq!(
                    f.hit_test(x, y),
                    Some(Target::ToggleHelp),
                    "({x}, {y}) is not the sheet's"
                );
            }
        }
    }

    #[test]
    fn a_click_while_the_sheet_is_open_closes_it_and_reaches_nothing_behind_it() {
        // Walking every control the closed frame records, rather than clicking
        // the middle of the maze: a hit box that covered only the sheet's own
        // pixels would pass a test that clicked where the sheet is anyway.
        let shut = sized(3, MazeApp::SIZE);
        let boxes: Vec<(Target, Rect)> =
            shut.frame(MazeApp::SIZE.0, MazeApp::SIZE.1).hits().to_vec();
        assert!(!boxes.is_empty(), "the frame records no controls at all");
        for (target, rect) in boxes {
            let mut g = sized(3, MazeApp::SIZE);
            g.apply(Action::ToggleHelp);
            let before = describe(&g);
            let (x, y) = rect.centre();
            g.click_at(x, y, MouseButton::Left, MazeApp::SIZE);
            assert!(
                !g.show_help(),
                "{target:?} at ({x}, {y}): the click did not close the sheet"
            );
            g.apply(Action::ToggleHelp);
            assert_eq!(
                before,
                describe(&g),
                "{target:?} at ({x}, {y}): the click reached it through the sheet"
            );
        }
    }

    // ── The frame ──────────────────────────────────────────────────────────

    #[test]
    fn every_control_the_frame_records_is_wired_to_something() {
        let base = sized(3, MazeApp::SIZE);
        let boxes: Vec<(Target, Rect)> =
            base.frame(MazeApp::SIZE.0, MazeApp::SIZE.1).hits().to_vec();
        for (target, rect) in boxes {
            let mut g = sized(3, MazeApp::SIZE);
            let before = describe(&g);
            let (x, y) = rect.centre();
            let outcome = g.click_at(x, y, MouseButton::Left, MazeApp::SIZE);
            assert_eq!(
                outcome,
                EventResult::Consumed,
                "{target:?} answered nothing"
            );
            assert_ne!(before, describe(&g), "{target:?} is wired to nothing");
        }
    }

    #[test]
    fn the_frame_is_balanced_in_every_window() {
        for (w, h) in sizes() {
            for open in [false, true] {
                let mut g = sized(3, (w, h));
                if open {
                    g.apply(Action::ToggleHelp);
                }
                let f = g.frame(w, h);
                assert!(
                    f.is_balanced(),
                    "{w}x{h} open={open}: the frame is unbalanced"
                );
            }
        }
    }

    #[test]
    fn the_info_line_says_the_clock_the_moves_and_the_steps_left() {
        let mut g = sized(3, MazeApp::SIZE);
        handle_event(&mut g, &Event::Tick { elapsed_ms: 65_000 });
        let line = g.info_line();
        assert!(line.contains("1:05"), "no clock in {line:?}");
        assert!(line.contains("0 moves"), "no move count in {line:?}");
        assert!(
            line.contains(&format!("{} steps to go", g.steps_left().unwrap())),
            "no count of the steps left in {line:?}"
        );
        assert!(line.contains("no best yet"), "no best in {line:?}");
        let dir = an_open_way(&g);
        down(&mut g, key_for(dir));
        assert_ne!(line, g.info_line(), "a move changed nothing the line says");
    }

    #[test]
    fn the_win_notice_covers_nothing_a_click_needs() {
        // It is a notice, not a sheet: it records no hit box, so every control
        // under it goes on working.
        let mut g = sized(12, MazeApp::SIZE);
        walk_out(&mut g);
        let f = g.frame(MazeApp::SIZE.0, MazeApp::SIZE.1);
        for slot in 0..LEVELS.len() {
            let rect = probe::rect_of(&g, Target::Level(slot)).unwrap();
            let (x, y) = rect.centre();
            assert_eq!(f.hit_test(x, y), Some(Target::Level(slot)));
        }
        for target in [Target::NewMaze, Target::ToggleSolution, Target::ToggleHelp] {
            let rect = probe::rect_of(&g, target).unwrap();
            let (x, y) = rect.centre();
            assert_eq!(f.hit_test(x, y), Some(target), "{target:?} is buried");
        }
        // And a cell in the middle, which is what the plate is actually over.
        let mid = (g.maze().rows() / 2, g.maze().cols() / 2);
        let rect = g
            .layout()
            .square(g.maze().rows(), g.maze().cols(), mid.0, mid.1);
        let (x, y) = rect.centre();
        assert_eq!(
            f.hit_test(x, y),
            Some(Target::Cell(g.maze().index(mid.0, mid.1).unwrap())),
            "the notice swallowed the maze"
        );
    }

    #[test]
    fn a_close_request_ends_the_program() {
        let mut g = game(3);
        assert!(matches!(
            App::on_event(&mut g, &Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn the_window_names_itself() {
        let g = game(3);
        assert_eq!(App::title(&g), "Maze");
        assert_eq!(App::app_id(&g), "maze");
        assert_eq!(
            App::initial_size(&g),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
    }
}
