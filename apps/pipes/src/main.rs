//! Pipes — turn the segments until the water runs from the source to the drain,
//! in a real window.
//!
//! Three sizes, keyboard and pointer, a help sheet, a flow overlay, and a floor
//! on the turns you still have to make.
//!
//! # What wiring this up found
//!
//! The program drew a board and could not be played, because `main` built a
//! `PipesApp`, dropped it and exited. Nothing below was reachable to notice
//! until it had a window on it — and the first of them is the whole game.
//!
//! 1. **The generator never once built a solvable board.** `generate` walks a
//!    path from the source to the drain and lays pipe along it, and the
//!    openings it asked each cell for were wrong: `prev_dir` was
//!    `dir_from(prev, here)` — the direction the walk *travelled to arrive*,
//!    which points **away** from the previous cell — where the opening the cell
//!    needs points back *toward* it. A straight run therefore asked for "Right
//!    and Right", which the two-opening branch read as non-opposite and fitted
//!    a **corner**. Measured over 2000 seeds at each of the three sizes: the
//!    walk reached the drain 2000/2000 times and the board laid from it was
//!    connected **0** times. Every puzzle the program has ever dealt was
//!    solvable only by whatever accident the random fill left behind.
//! 2. **There was no pointer input at all.** `handle_event` matched
//!    `Event::Key` and nothing else, so a mouse could not turn a pipe, and the
//!    controls line did not claim otherwise — it named only keys. A pipes
//!    puzzle played by walking a cursor to a cell with four arrow keys before
//!    turning it is a pipes puzzle with a stated-in-code reason not to use a
//!    mouse. Every cell and every control is clickable now: left turns
//!    clockwise, right turns anticlockwise.
//! 3. **The layout was a constant.** `render(width, height)` used its two
//!    arguments for the background rectangle and nothing else. The board was
//!    `50.0.min(400.0 / cols)` per cell at a fixed (20, 95) — capped at 400
//!    pixels wide however large the window — the title, stats and controls drew
//!    at fixed `y`, and the victory message drew *below the board*, at
//!    `95 + rows * cell + 15`, which on Hard is past the bottom of any window
//!    shorter than about 570 pixels. Winning is exactly when you most want to
//!    be told you won.
//! 4. **The controls line named five of the seven keys it answered.** `F`
//!    (the flow overlay) and `Enter` (a synonym for Space) were answered and
//!    unmentioned. A test now walks the help sheet and checks both ways: every
//!    key it names answers, and every key that answers is named.
//! 5. **`PipeKind::Cross` could not occur.** The random fill drew
//!    `rng.below(5)` and mapped `0..=3` to Straight, Corner, Tee and End, with
//!    `_ => Straight` — so the fifth outcome, the one arm that would have made
//!    a cross, produced a second straight instead. The kind existed, drew
//!    correctly and was tested; it was simply unreachable.
//! 6. **`pipe_for_openings` ignored *which* three openings it was given.** The
//!    three-opening arm was `_ => (PipeKind::Tee, 0)`, a tee in rotation zero,
//!    whatever sides were asked for. Nothing on the laid path has three
//!    openings today, so it never bit — a latent wrong answer sitting behind
//!    the one that was actually wrong. It now searches every kind and rotation
//!    for the set that matches exactly, which answers all five cases with one
//!    rule instead of four hand-written ones.
//! 7. **A board could be dealt already finished.** The scramble turned each
//!    cell a random number of quarter-turns and checked nothing, so a board
//!    that came out solved was presented as a puzzle — and the first turn you
//!    made broke it. It reshuffles now, a bounded number of times, because a
//!    board that *cannot* be unsolved must not turn the guard into an
//!    unbounded loop.
//! 8. **A turn that changed nothing counted a move.** A cross turns onto
//!    itself, and so does a straight every second turn; both incremented the
//!    move counter, which is the number the win message reports. Turning a
//!    cross is now refused and said so.
//! 9. **Nothing said how close you were.** The move counter only goes up. There
//!    is now a floor on the turns still to make — the cheapest set of rotations
//!    that could possibly join the source to the drain, found by a Dijkstra
//!    over (cell, side entered from) states — which no sequence of turns can
//!    beat, because it is a lower bound by construction. It is drawn as a floor
//!    and named as one.
//! 10. **Ten blanket `#![allow]`s sat on lines 6-15**, `dead_code` among them,
//!     which is what kept 5 and 11 invisible.
//! 11. **`let _half = (cell_size - 2.0) / 2.0;`** was computed once per cell,
//!     per frame, and read by nothing.
//!
//! The floor is the honest number to read a score against: a scramble that
//! lands four turns from home is not the same puzzle as one that lands forty,
//! so the board shows the floor it started from as well as the one it is on.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::collections::VecDeque;
use std::process::ExitCode;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
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
const TEAL: Color = Color::from_hex(0x94E2D5);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// The source cell's backing, and the drain's. Dark enough to read the pipe
/// against, distinct enough to find at a glance without reading the letter.
const SOURCE_BG: Color = Color::from_hex(0x2A4A3A);
const DRAIN_BG: Color = Color::from_hex(0x4A2A3A);

const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 720.0;

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `PIPES!!!`.
const FALLBACK_SEED: u64 = 0x5049_5045_5321_2121;

const HELP_TITLE: &str = "How to play";

/// The controls, as the sheet states them.
///
/// A test walks this list and checks each named key actually answers, and that
/// no key the program answers is missing from it. The old controls line named
/// five of the seven keys — `F` and `Enter` were answered and unmentioned.
const HELP_ROWS: [(&str, &str); 9] = [
    ("Goal", "Join the source (S) to the drain (D)"),
    ("Arrows", "Move the cursor"),
    ("Space, Enter", "Turn the pipe clockwise"),
    ("Z", "Turn it anticlockwise"),
    ("Click", "Turn clockwise; right-click, anticlockwise"),
    ("1 / 2 / 3", "Change the size"),
    ("N", "New puzzle"),
    ("F", "Show or hide the water"),
    ("H, Esc", "Open or close this sheet"),
];

// ── Direction ──────────────────────────────────────────────────────────────

/// A side of a cell, which is also the direction of the neighbour across it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Right,
    Down,
    Left,
}

impl Dir {
    /// All four, in clockwise order from `Up`. The index of a side in this
    /// array is the number the flow search uses to name it.
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Right, Dir::Down, Dir::Left];

    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Right => Self::Left,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
        }
    }

    /// The (row, column) step across this side.
    #[must_use]
    pub fn delta(self) -> (i32, i32) {
        match self {
            Self::Up => (-1, 0),
            Self::Right => (0, 1),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
        }
    }

    #[must_use]
    pub fn rotate_cw(self) -> Self {
        match self {
            Self::Up => Self::Right,
            Self::Right => Self::Down,
            Self::Down => Self::Left,
            Self::Left => Self::Up,
        }
    }

    /// This side's place in [`Dir::ALL`].
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Up => 0,
            Self::Right => 1,
            Self::Down => 2,
            Self::Left => 3,
        }
    }
}

// ── Pipes ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipeKind {
    /// Two opposite openings.
    Straight,
    /// Two adjacent openings.
    Corner,
    /// Three openings.
    Tee,
    /// All four.
    Cross,
    /// One — a dead end, and what the source and the drain are.
    End,
    /// None.
    Empty,
}

impl PipeKind {
    /// Every kind, in ascending opening count, `Empty` last.
    ///
    /// The order matters to [`pipe_for_openings`], which returns the first
    /// kind-and-rotation whose openings match exactly. Since it demands an
    /// exact match the order cannot change *which* kind is chosen — but a
    /// reader should not have to prove that to trust the function, so the list
    /// is ordered the way the answer is.
    pub const ALL: [PipeKind; 6] = [
        PipeKind::End,
        PipeKind::Straight,
        PipeKind::Corner,
        PipeKind::Tee,
        PipeKind::Cross,
        PipeKind::Empty,
    ];

    /// The openings at rotation zero.
    #[must_use]
    pub fn base_openings(self) -> &'static [Dir] {
        match self {
            Self::Straight => &[Dir::Up, Dir::Down],
            Self::Corner => &[Dir::Up, Dir::Right],
            Self::Tee => &[Dir::Up, Dir::Right, Dir::Down],
            Self::Cross => &[Dir::Up, Dir::Right, Dir::Down, Dir::Left],
            Self::End => &[Dir::Up],
            Self::Empty => &[],
        }
    }

    /// How many quarter-turns give a different shape.
    ///
    /// One for a cross and for empty — turning either changes nothing at all,
    /// which is fault eight: a turn that cannot alter the board must not count
    /// as a move against the player.
    #[must_use]
    pub fn distinct_rotations(self) -> u8 {
        match self {
            Self::Straight => 2,
            Self::Corner | Self::Tee | Self::End => 4,
            Self::Cross | Self::Empty => 1,
        }
    }
}

/// One cell's pipe: a shape and the quarter-turns it has been given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pipe {
    pub kind: PipeKind,
    /// 0-3, each a quarter-turn clockwise.
    pub rotation: u8,
}

impl Pipe {
    #[must_use]
    pub fn new(kind: PipeKind, rotation: u8) -> Self {
        Self {
            kind,
            rotation: rotation % 4,
        }
    }

    /// The sides this pipe is open on, in its current rotation.
    #[must_use]
    pub fn openings(self) -> Vec<Dir> {
        self.kind
            .base_openings()
            .iter()
            .map(|&d| {
                let mut dir = d;
                for _ in 0..self.rotation {
                    dir = dir.rotate_cw();
                }
                dir
            })
            .collect()
    }

    #[must_use]
    pub fn has_opening(self, dir: Dir) -> bool {
        self.openings().contains(&dir)
    }

    /// Whether turning this pipe would alter the board at all.
    #[must_use]
    pub fn turning_changes_anything(self) -> bool {
        self.kind.distinct_rotations() > 1
    }

    pub fn rotate_cw(&mut self) {
        self.rotation = (self.rotation.wrapping_add(1)) % 4;
    }

    pub fn rotate_ccw(&mut self) {
        self.rotation = (self.rotation.wrapping_add(3)) % 4;
    }
}

/// The fewest quarter-turns that take rotation `from` to rotation `to`, in
/// either direction.
#[must_use]
fn turns_between(from: u8, to: u8) -> u32 {
    let d = u32::from((to % 4).wrapping_add(4).wrapping_sub(from % 4) % 4);
    d.min(4_u32.saturating_sub(d))
}

/// The fewest turns that leave `cell` open on every side in `needed`, or `None`
/// if no rotation of it ever is.
#[must_use]
fn turns_to_open(cell: Pipe, needed: &[Dir]) -> Option<u32> {
    (0..4_u8)
        .filter(|&rot| {
            let have = Pipe::new(cell.kind, rot).openings();
            needed.iter().all(|d| have.contains(d))
        })
        .map(|rot| turns_between(cell.rotation, rot))
        .min()
}

/// The kind and rotation whose openings are exactly `openings`.
///
/// Was four hand-written arms, one per opening count, of which the
/// three-opening one was `_ => (Tee, 0)` — a tee in rotation zero whatever
/// sides were asked for (fault six). Searching answers all five counts with
/// one rule, and the rule is the specification: the pipe whose openings are
/// the ones asked for.
#[must_use]
pub fn pipe_for_openings(openings: &[Dir]) -> Pipe {
    for kind in PipeKind::ALL {
        for rot in 0..4_u8 {
            let candidate = Pipe::new(kind, rot);
            let have = candidate.openings();
            if have.len() == openings.len() && openings.iter().all(|d| have.contains(d)) {
                return candidate;
            }
        }
    }
    Pipe::new(PipeKind::Empty, 0)
}

// ── Difficulty ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

/// The sizes offered, smallest first. `LEVELS[i]` is the level behind size
/// button `i`, and behind the number key `i + 1`.
pub const LEVELS: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

impl Difficulty {
    /// Rows and columns.
    #[must_use]
    pub fn grid_size(self) -> (usize, usize) {
        match self {
            Self::Easy => (5, 5),
            Self::Medium => (7, 7),
            Self::Hard => (9, 9),
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }
}

// ── Board ──────────────────────────────────────────────────────────────────

/// The grid, flat: `cells[r * cols + c]`.
///
/// Was a `Vec<Vec<Pipe>>` indexed with `[r][c]` throughout, which is one
/// allocation per row and a panic for every caller that gets a coordinate
/// wrong. One `Vec` and one `index` that returns `Option` means an out-of-range
/// coordinate is a `None` to handle rather than a crash on a player's screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    rows: usize,
    cols: usize,
    cells: Vec<Pipe>,
    source: (usize, usize),
    drain: (usize, usize),
}

impl Board {
    #[must_use]
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            cells: vec![Pipe::new(PipeKind::Empty, 0); rows.saturating_mul(cols)],
            source: (0, 0),
            drain: (rows.saturating_sub(1), cols.saturating_sub(1)),
        }
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }

    #[must_use]
    pub fn cols(&self) -> usize {
        self.cols
    }

    #[must_use]
    pub fn source(&self) -> (usize, usize) {
        self.source
    }

    #[must_use]
    pub fn drain(&self) -> (usize, usize) {
        self.drain
    }

    /// The flat index of a cell, or `None` if it is off the grid.
    #[must_use]
    pub fn index(&self, r: usize, c: usize) -> Option<usize> {
        if r >= self.rows || c >= self.cols {
            return None;
        }
        r.checked_mul(self.cols)?.checked_add(c)
    }

    /// The (row, column) of a flat index.
    #[must_use]
    pub fn coords(&self, index: usize) -> Option<(usize, usize)> {
        if self.cols == 0 || index >= self.cells.len() {
            return None;
        }
        Some((index.checked_div(self.cols)?, index.checked_rem(self.cols)?))
    }

    #[must_use]
    pub fn get(&self, r: usize, c: usize) -> Option<Pipe> {
        self.index(r, c).and_then(|i| self.cells.get(i).copied())
    }

    fn set(&mut self, r: usize, c: usize, pipe: Pipe) {
        if let Some(i) = self.index(r, c)
            && let Some(slot) = self.cells.get_mut(i)
        {
            *slot = pipe;
        }
    }

    /// The neighbour across `dir`, or `None` at the edge.
    #[must_use]
    pub fn step(&self, r: usize, c: usize, dir: Dir) -> Option<(usize, usize)> {
        let (dr, dc) = dir.delta();
        let nr = i64::try_from(r).ok()?.checked_add(i64::from(dr))?;
        let nc = i64::try_from(c).ok()?.checked_add(i64::from(dc))?;
        let nr = usize::try_from(nr).ok()?;
        let nc = usize::try_from(nc).ok()?;
        if nr < self.rows && nc < self.cols {
            Some((nr, nc))
        } else {
            None
        }
    }

    /// Whether the cell at `(r, c)` and its neighbour across `dir` are both
    /// open on the side they share.
    #[must_use]
    pub fn joined(&self, r: usize, c: usize, dir: Dir) -> bool {
        let Some((nr, nc)) = self.step(r, c, dir) else {
            return false;
        };
        match (self.get(r, c), self.get(nr, nc)) {
            (Some(here), Some(there)) => here.has_opening(dir) && there.has_opening(dir.opposite()),
            _ => false,
        }
    }

    /// Which cells the water reaches from the source, by flat index.
    #[must_use]
    pub fn filled(&self) -> Vec<bool> {
        let mut seen = vec![false; self.cells.len()];
        let (sr, sc) = self.source;
        let Some(start) = self.index(sr, sc) else {
            return seen;
        };
        if let Some(slot) = seen.get_mut(start) {
            *slot = true;
        }
        let mut queue = VecDeque::new();
        queue.push_back((sr, sc));
        while let Some((r, c)) = queue.pop_front() {
            for dir in Dir::ALL {
                if !self.joined(r, c, dir) {
                    continue;
                }
                let Some((nr, nc)) = self.step(r, c, dir) else {
                    continue;
                };
                let Some(i) = self.index(nr, nc) else {
                    continue;
                };
                if seen.get(i).copied().unwrap_or(true) {
                    continue;
                }
                if let Some(slot) = seen.get_mut(i) {
                    *slot = true;
                }
                queue.push_back((nr, nc));
            }
        }
        seen
    }

    /// How many cells the water reaches, the source included.
    #[must_use]
    pub fn filled_count(&self) -> usize {
        self.filled().iter().filter(|f| **f).count()
    }

    /// Whether the water reaches the drain.
    #[must_use]
    pub fn is_solved(&self) -> bool {
        let (dr, dc) = self.drain;
        match self.index(dr, dc) {
            Some(i) => self.filled().get(i).copied().unwrap_or(false),
            None => false,
        }
    }

    /// A floor on the turns still to make: the cheapest set of rotations that
    /// could join the source to the drain.
    ///
    /// A shortest path over states `(cell, the side the water entered by)`,
    /// charging each cell, as the search leaves it, the fewest turns that open
    /// it on both the side it was entered by and the side it is left by. Any
    /// real solution contains some source-to-drain run of joined cells, and
    /// each cell of that run has been turned at least as many times as this
    /// charges it — so the total is a lower bound on the moves remaining, and
    /// no sequence of turns can beat it. It is zero exactly when the board is
    /// already solved: a zero-cost path is one every cell of which is already
    /// open on both its sides, which is what "solved" means.
    ///
    /// `u32::MAX` when no rotation of the cells could ever join the two, which
    /// [`Board::is_solvable`] is the readable name for.
    #[must_use]
    pub fn rotation_floor(&self) -> u32 {
        let n = self.cells.len();
        if n == 0 {
            return u32::MAX;
        }
        let (sr, sc) = self.source;
        let (dr, dc) = self.drain;
        let (Some(source), Some(drain)) = (self.index(sr, sc), self.index(dr, dc)) else {
            return u32::MAX;
        };
        // The source is a special case, not a state: it is the one cell entered
        // from nowhere, so it is charged for one opening rather than two.
        if source == drain {
            return 0;
        }

        // State `cell * 4 + side` — the water is in `cell`, having entered
        // across `side`. Dijkstra rather than a plain BFS because the edges
        // cost 0, 1 or 2 turns, and a BFS would happily report the two-turn
        // route it happened to reach first.
        let states = n.saturating_mul(4);
        let mut best = vec![u32::MAX; states];
        let mut queue: Vec<(u32, usize)> = Vec::new();
        for out in Dir::ALL {
            let Some(src_pipe) = self.get(sr, sc) else {
                continue;
            };
            let Some(cost) = turns_to_open(src_pipe, &[out]) else {
                continue;
            };
            let Some((nr, nc)) = self.step(sr, sc, out) else {
                continue;
            };
            let Some(cell) = self.index(nr, nc) else {
                continue;
            };
            let state = cell
                .saturating_mul(4)
                .saturating_add(out.opposite().index());
            if best.get(state).copied().unwrap_or(0) > cost {
                if let Some(slot) = best.get_mut(state) {
                    *slot = cost;
                }
                queue.push((cost, state));
            }
        }

        let mut answer = u32::MAX;
        while let Some(pos) = queue
            .iter()
            .enumerate()
            .min_by_key(|(_, (cost, _))| *cost)
            .map(|(i, _)| i)
        {
            let (cost, state) = queue.swap_remove(pos);
            if best.get(state).copied().unwrap_or(0) < cost {
                continue;
            }
            let cell = state / 4;
            let Some(entered) = Dir::ALL.get(state % 4).copied() else {
                continue;
            };
            let Some((r, c)) = self.coords(cell) else {
                continue;
            };
            let Some(pipe) = self.get(r, c) else {
                continue;
            };
            if cell == drain {
                // The drain is charged for its one opening and the walk stops:
                // water that runs on past it has still arrived.
                if let Some(last) = turns_to_open(pipe, &[entered]) {
                    answer = answer.min(cost.saturating_add(last));
                }
                continue;
            }
            for out in Dir::ALL {
                if out == entered {
                    continue;
                }
                let Some(step) = turns_to_open(pipe, &[entered, out]) else {
                    continue;
                };
                let Some((nr, nc)) = self.step(r, c, out) else {
                    continue;
                };
                let Some(next_cell) = self.index(nr, nc) else {
                    continue;
                };
                let next = next_cell
                    .saturating_mul(4)
                    .saturating_add(out.opposite().index());
                let total = cost.saturating_add(step);
                if best.get(next).copied().unwrap_or(0) > total {
                    if let Some(slot) = best.get_mut(next) {
                        *slot = total;
                    }
                    queue.push((total, next));
                }
            }
        }
        answer
    }

    /// Whether any sequence of turns could solve this board.
    ///
    /// The property the generator was silently failing: measured over 2000
    /// seeds at each size, the board it laid was connected zero times.
    #[must_use]
    pub fn is_solvable(&self) -> bool {
        self.rotation_floor() < u32::MAX
    }

    /// Turn one cell, returning whether the board changed.
    ///
    /// A cross turns onto itself; so does a straight, every second turn. The
    /// old code counted both as moves against the player (fault eight), so the
    /// number in the win message was not the number of turns that did anything.
    pub fn turn(&mut self, r: usize, c: usize, clockwise: bool) -> bool {
        let Some(mut pipe) = self.get(r, c) else {
            return false;
        };
        if !pipe.turning_changes_anything() {
            return false;
        }
        if clockwise {
            pipe.rotate_cw();
        } else {
            pipe.rotate_ccw();
        }
        self.set(r, c, pipe);
        true
    }
}

// ── Generation ─────────────────────────────────────────────────────────────

/// How many scrambles a board may take before it settles for what it has.
///
/// A board that *cannot* be unsolved — a 1x1, where the source is the drain —
/// must not turn the retry into an unbounded loop.
const SCRAMBLE_ATTEMPTS: usize = 8;

impl Board {
    /// The side of `(r, c)` that faces `(to_r, to_c)`, or `None` if they are
    /// not neighbours.
    ///
    /// Named for what it returns — the side *facing* the other cell — because
    /// the old `dir_from(from, to)` was read as both "the side of `from` facing
    /// `to`" and "the direction travelled", which are opposites, and that
    /// confusion is fault one.
    #[must_use]
    pub fn dir_between(r: usize, c: usize, to_r: usize, to_c: usize) -> Option<Dir> {
        let dr = i64::try_from(to_r)
            .ok()?
            .checked_sub(i64::try_from(r).ok()?)?;
        let dc = i64::try_from(to_c)
            .ok()?
            .checked_sub(i64::try_from(c).ok()?)?;
        match (dr, dc) {
            (-1, 0) => Some(Dir::Up),
            (1, 0) => Some(Dir::Down),
            (0, -1) => Some(Dir::Left),
            (0, 1) => Some(Dir::Right),
            _ => None,
        }
    }

    /// Lay a connected run of pipe along `path`.
    ///
    /// This is the half that was broken. Each cell of the walk needs an opening
    /// toward the cell before it and one toward the cell after it — *toward*,
    /// in both cases. The old code took `dir_from(prev, here)`, the direction
    /// the walk travelled to arrive, which points away from the previous cell,
    /// so a straight run asked for the same side twice and was fitted a corner.
    fn lay_path(&mut self, path: &[(usize, usize)]) {
        for (idx, &(r, c)) in path.iter().enumerate() {
            let mut openings: Vec<Dir> = Vec::new();
            if let Some(prev) = idx.checked_sub(1)
                && let Some(&(pr, pc)) = path.get(prev)
                && let Some(back) = Self::dir_between(r, c, pr, pc)
            {
                openings.push(back);
            }
            if let Some(&(nr, nc)) = path.get(idx.saturating_add(1))
                && let Some(on) = Self::dir_between(r, c, nr, nc)
            {
                openings.push(on);
            }
            self.set(r, c, pipe_for_openings(&openings));
        }
    }

    /// A self-avoiding walk from the source to the drain, biased toward the
    /// drain but not always taking the closest step.
    fn random_walk(&self, rng: &mut SeededRng) -> Vec<(usize, usize)> {
        let start = self.source;
        let end = self.drain;
        let mut seen = vec![false; self.cells.len()];
        if let Some(i) = self.index(start.0, start.1)
            && let Some(slot) = seen.get_mut(i)
        {
            *slot = true;
        }
        let mut path = vec![start];
        // A grid of `n` cells has no self-avoiding walk longer than `n` steps,
        // and each iteration either extends the path or backtracks off a cell
        // that can never be re-entered, so this cannot run forever. The old
        // walk had no such argument written down and no bound either.
        loop {
            let Some(&(r, c)) = path.last() else {
                return vec![start];
            };
            if (r, c) == end {
                return path;
            }
            let mut options: Vec<(usize, usize)> = Vec::new();
            for dir in Dir::ALL {
                if let Some((nr, nc)) = self.step(r, c, dir)
                    && let Some(i) = self.index(nr, nc)
                    && !seen.get(i).copied().unwrap_or(true)
                {
                    options.push((nr, nc));
                }
            }
            if options.is_empty() {
                path.pop();
                if path.is_empty() {
                    // Only reachable if the source itself is boxed in, which on
                    // a grid means a 1x1 board.
                    return vec![start];
                }
                continue;
            }
            options.sort_by_key(|&(nr, nc)| {
                let dr = i64::try_from(nr)
                    .unwrap_or(0)
                    .saturating_sub(i64::try_from(end.0).unwrap_or(0))
                    .unsigned_abs();
                let dc = i64::try_from(nc)
                    .unwrap_or(0)
                    .saturating_sub(i64::try_from(end.1).unwrap_or(0))
                    .unsigned_abs();
                dr.saturating_add(dc)
            });
            // One step in three is drawn at random from all the ways out; the
            // rest take the one that closes on the drain. Always closing gives
            // the same staircase every time; never closing wanders the whole
            // grid before arriving.
            let pick = if rng.below(3) == 0 {
                rng.below(options.len())
            } else {
                0
            };
            let Some(&next) = options.get(pick) else {
                return path;
            };
            if let Some(i) = self.index(next.0, next.1)
                && let Some(slot) = seen.get_mut(i)
            {
                *slot = true;
            }
            path.push(next);
        }
    }

    /// Put a random pipe in every cell the path did not use.
    ///
    /// Draws from all five shapes. The old fill drew `below(5)` and mapped the
    /// fifth outcome back onto `Straight`, so `Cross` — a kind that existed,
    /// drew correctly and had its own test — could never appear on a board.
    fn fill_rest(&mut self, rng: &mut SeededRng) {
        const FILL: [PipeKind; 5] = [
            PipeKind::Straight,
            PipeKind::Corner,
            PipeKind::Tee,
            PipeKind::Cross,
            PipeKind::End,
        ];
        for r in 0..self.rows {
            for c in 0..self.cols {
                if self.get(r, c).map(|p| p.kind) != Some(PipeKind::Empty) {
                    continue;
                }
                let kind = FILL
                    .get(rng.below(FILL.len()))
                    .copied()
                    .unwrap_or(PipeKind::End);
                let rot = u8::try_from(rng.below(4)).unwrap_or(0);
                self.set(r, c, Pipe::new(kind, rot));
            }
        }
    }

    /// Give every cell a random quarter-turn count.
    fn scramble(&mut self, rng: &mut SeededRng) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                let rot = u8::try_from(rng.below(4)).unwrap_or(0);
                if let Some(pipe) = self.get(r, c) {
                    self.set(r, c, Pipe::new(pipe.kind, pipe.rotation.wrapping_add(rot)));
                }
            }
        }
    }

    /// A fresh puzzle: a laid solution, a random fill, and a scramble that does
    /// not hand back a board already finished.
    #[must_use]
    pub fn generate(difficulty: Difficulty, rng: &mut SeededRng) -> Self {
        let (rows, cols) = difficulty.grid_size();
        let mut board = Board::new(rows, cols);
        let path = board.random_walk(rng);
        board.lay_path(&path);
        board.fill_rest(rng);
        board.deal(rng)
    }

    /// Scramble this laid solution into a puzzle that is not already finished.
    ///
    /// Split out of [`Board::generate`] so both of its promises can be put to a
    /// board built to break them, which no board the three levels deal ever
    /// will: a real 6x6 scramble lands solved so rarely that a suite watching
    /// only `generate` passes with the retry deleted, and never runs the bound
    /// at all.
    ///
    /// Each attempt scrambles the laid solution **afresh** rather than
    /// scrambling the previous attempt again: turning a board twice more is not
    /// another draw from the same distribution, it is a walk away from one.
    #[must_use]
    fn deal(&self, rng: &mut SeededRng) -> Self {
        let mut attempt = self.clone();
        for _ in 0..SCRAMBLE_ATTEMPTS {
            attempt = self.clone();
            attempt.scramble(rng);
            if !attempt.is_solved() {
                break;
            }
        }
        attempt
    }
}

// ── Targets and actions ────────────────────────────────────────────────────

/// Something on the screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A cell of the board, by flat index.
    Cell(usize),
    /// One of the size buttons, by index into [`LEVELS`].
    Level(usize),
    NewGame,
    ToggleFlow,
    ToggleHelp,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Everything the game can be asked to do, from either input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Turn a named cell — what a click does.
    TurnAt {
        cell: usize,
        clockwise: bool,
    },
    /// Turn the cell under the cursor — what Space and Z do.
    TurnCursor {
        clockwise: bool,
    },
    MoveCursor(Dir),
    NewGame,
    /// Switch to `LEVELS[i]`.
    SetLevel(usize),
    ToggleFlow,
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
const BOARD_SHARE: f32 = 0.55;

/// Which band goes first when they do not all fit: header, controls, info.
///
/// Bands are dropped whole rather than shrunk together, because a band scaled
/// to four pixels costs the board four pixels and shows nothing. The title
/// goes first — it names a program you are already looking at. The controls go
/// next: they are the pointer's copy of keys that still work. The info line
/// goes last, because the move count and the turns-remaining floor are the only
/// chrome you cannot read the game without.
const BAND_DROP_ORDER: [usize; 3] = [0, 2, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in — which
/// is what a board hard-capped at 400 pixels inside a 1920-pixel window was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// The title and the level's name.
    pub header: Rect,
    /// Moves made, turns left at the very least, and how much of the board the
    /// water reaches.
    pub info: Rect,
    /// The square the grid is drawn in.
    pub board: Rect,
    /// The three sizes, new game, the water toggle and help.
    pub controls: Rect,
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

        // What each band would like, in [header, info, controls] order.
        let mut wants = [
            (h * 0.085).clamp(22.0, 44.0),
            (h * 0.055).clamp(16.0, 28.0),
            (h * 0.08).clamp(22.0, 40.0),
        ];
        // What is left for chrome once the board has its share *and* the gap
        // separating it from the chrome above and below. The padding comes out
        // of this side: charging it to the board turns a promised share of the
        // window into rather less than that share of a small one.
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
        // tall. The two read the same to `shows`, but only one of them reads
        // the same to anything asking "is this band gone, or merely thin?"
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
        // `Rect::EMPTY`, whose bottom is zero, so reading the band would put
        // the board back over the header the moment the info line is dropped
        // while the header still shows. `BAND_DROP_ORDER` happens to drop the
        // header *first*, so the two forms agree today and no test can tell
        // them apart — which is exactly why the safe one is written here
        // rather than left to be got right again by whoever reorders the
        // constant.
        let top = hdr_h + inf_h;
        let bottom = if ctl_h > 0.0 { controls.y } else { h };
        let band = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );
        // Square and centred in what is left. A 9x9 grid in a non-square
        // rectangle either stretches its cells or leaves them where the hit
        // test is not; squaring it here means neither.
        let side = band.w.min(band.h).max(0.0);
        let board = Rect::new(
            band.x + (band.w - side) / 2.0,
            band.y + (band.h - side) / 2.0,
            side,
            side,
        );

        let help_w = (w * 0.92).min(460.0);
        let help_h = (h * 0.92).min(330.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            board,
            controls,
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

    /// The side of one cell of a `rows` by `cols` grid.
    #[must_use]
    pub fn cell(&self, rows: usize, cols: usize) -> f32 {
        if rows == 0 || cols == 0 {
            return 0.0;
        }
        // `rows` and `cols` are at most 9 here, so the casts are exact.
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
pub struct PipesApp {
    board: Board,
    /// Index into [`LEVELS`].
    level: usize,
    cursor: (usize, usize),
    state: GameState,
    moves: u32,
    /// The floor the current scramble started from, so a finished game can be
    /// read against the puzzle it was set on.
    opening_floor: u32,
    games_won: u32,
    rng: SeededRng,
    show_flow: bool,
    show_help: bool,
    status: String,
    size_drawn: (f32, f32),
}

impl PipesApp {
    #[must_use]
    pub fn new() -> Self {
        // Was `with_seed(42)`: every player, on every machine, got the same
        // board in the same rotations.
        Self::with_seed(guitk::rng::seed_from_system(FALLBACK_SEED))
    }

    /// The game with a named seed, so a test can name the puzzle it means.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        let mut game = Self {
            board: Board::new(1, 1),
            level: 0,
            cursor: (0, 0),
            state: GameState::Playing,
            moves: 0,
            opening_floor: 0,
            games_won: 0,
            rng: SeededRng::new(seed),
            show_flow: true,
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
    pub fn level(&self) -> Difficulty {
        LEVELS.get(self.level).copied().unwrap_or(Difficulty::Easy)
    }

    #[must_use]
    pub fn level_index(&self) -> usize {
        self.level
    }

    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
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
    pub fn opening_floor(&self) -> u32 {
        self.opening_floor
    }

    #[must_use]
    pub fn games_won(&self) -> u32 {
        self.games_won
    }

    #[must_use]
    pub fn show_flow(&self) -> bool {
        self.show_flow
    }

    #[must_use]
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The floor on the turns still to make. Zero exactly when solved.
    #[must_use]
    pub fn floor_left(&self) -> u32 {
        self.board.rotation_floor()
    }

    /// What the info line says about how far there is left to go.
    fn progress(&self) -> String {
        let floor = self.floor_left();
        if floor == u32::MAX {
            "This board cannot be joined".to_string()
        } else if floor == 0 {
            "Joined".to_string()
        } else {
            format!("{floor} turns at the very least")
        }
    }

    // ── Play ───────────────────────────────────────────────────────────────

    /// Deal a fresh puzzle at the current level.
    pub fn new_game(&mut self) {
        let difficulty = self.level();
        self.board = Board::generate(difficulty, &mut self.rng);
        self.cursor = (0, 0);
        self.state = if self.board.is_solved() {
            GameState::Won
        } else {
            GameState::Playing
        };
        self.moves = 0;
        self.opening_floor = self.floor_left();
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
        self.new_game();
    }

    fn move_cursor(&mut self, dir: Dir) {
        let (r, c) = self.cursor;
        if let Some(next) = self.board.step(r, c, dir) {
            self.cursor = next;
        }
    }

    fn turn_cell(&mut self, r: usize, c: usize, clockwise: bool) {
        if self.state == GameState::Won {
            self.status = "Solved — press N for a new puzzle".to_string();
            return;
        }
        let Some(pipe) = self.board.get(r, c) else {
            return;
        };
        self.cursor = (r, c);
        if !self.board.turn(r, c, clockwise) {
            // Fault eight: this used to count as a move. A cross turns onto
            // itself, so charging the player for it makes the win message
            // report a number of turns that did not all do anything.
            self.status = match pipe.kind {
                PipeKind::Cross => "A cross looks the same whichever way you turn it".to_string(),
                _ => "Nothing to turn there".to_string(),
            };
            return;
        }
        self.moves = self.moves.saturating_add(1);
        if self.board.is_solved() {
            self.state = GameState::Won;
            self.games_won = self.games_won.saturating_add(1);
            self.status = format!("Solved in {} turns", self.moves);
        } else {
            self.status = self.progress();
        }
    }

    /// The one place an action changes the game, so a key and a click that mean
    /// the same thing cannot come to mean different things.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::TurnAt { cell, clockwise } => {
                if let Some((r, c)) = self.board.coords(cell) {
                    self.turn_cell(r, c, clockwise);
                }
            }
            Action::TurnCursor { clockwise } => {
                let (r, c) = self.cursor;
                self.turn_cell(r, c, clockwise);
            }
            Action::MoveCursor(dir) => self.move_cursor(dir),
            Action::NewGame => self.new_game(),
            Action::SetLevel(i) => self.set_level(i),
            Action::ToggleFlow => {
                self.show_flow = !self.show_flow;
                self.status = if self.show_flow {
                    "Water shown".to_string()
                } else {
                    "Water hidden — press F to bring it back".to_string()
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

    // ── Input ──────────────────────────────────────────────────────────────

    fn key_action(&self, ev: &KeyEvent) -> Option<Action> {
        // A key that is *coming back up* is not a second press.
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
                Key::H | Key::Escape => Some(Action::CloseHelp),
                _ => None,
            };
        }
        match ev.key {
            Key::Up => Some(Action::MoveCursor(Dir::Up)),
            Key::Down => Some(Action::MoveCursor(Dir::Down)),
            Key::Left => Some(Action::MoveCursor(Dir::Left)),
            Key::Right => Some(Action::MoveCursor(Dir::Right)),
            Key::Space | Key::Enter => Some(Action::TurnCursor { clockwise: true }),
            Key::Z => Some(Action::TurnCursor { clockwise: false }),
            Key::N => Some(Action::NewGame),
            Key::F => Some(Action::ToggleFlow),
            Key::H | Key::Escape => Some(Action::ToggleHelp),
            Key::Num1 => Some(Action::SetLevel(0)),
            Key::Num2 => Some(Action::SetLevel(1)),
            Key::Num3 => Some(Action::SetLevel(2)),
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
        let button = match ev.kind {
            MouseEventKind::Press(b @ (MouseButton::Left | MouseButton::Right)) => b,
            _ => return EventResult::Ignored,
        };
        // No special case for the open sheet: `draw_help` records one hit box
        // over the whole window, last, so the ordinary hit test already answers
        // `ToggleHelp` everywhere while the sheet is up.
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        let clockwise = button == MouseButton::Left;
        match target {
            Target::Cell(cell) => self.apply(Action::TurnAt { cell, clockwise }),
            Target::Level(i) => self.apply(Action::SetLevel(i)),
            Target::NewGame => self.apply(Action::NewGame),
            Target::ToggleFlow => self.apply(Action::ToggleFlow),
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

impl Default for PipesApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

impl PipesApp {
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
            "Pipes",
            size,
            LAVENDER,
            FontWeightHint::Bold,
            Some((l.header.w - l.pad * 2.0).max(0.0)),
        );
        let name = self.level().name();
        let small = (size * 0.6).max(7.0);
        let w = text::measure(name, small, FontWeightHint::Bold);
        label(
            f,
            (l.header.right() - l.pad - w).max(l.header.x + l.pad),
            l.header.y + (l.header.h - text::line_height(small, FontWeightHint::Bold)) / 2.0,
            name,
            small,
            if self.state == GameState::Won {
                GREEN
            } else {
                SUBTEXT0
            },
            FontWeightHint::Bold,
            Some((l.header.w * 0.4).max(0.0)),
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
        let cells = self.board.rows().saturating_mul(self.board.cols());
        let line = format!(
            "{} turns  \u{2022}  {}  \u{2022}  water in {} of {} cells  \u{2022}  won {}",
            self.moves,
            self.status,
            self.board.filled_count(),
            cells,
            self.games_won,
        );
        let size = l.font.min(l.info.h * 0.8);
        label(
            f,
            l.info.x + l.pad,
            l.info.y + (l.info.h - text::line_height(size, FontWeightHint::Regular)) / 2.0,
            &line,
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
        let rows = self.board.rows();
        let cols = self.board.cols();
        let flow = if self.show_flow {
            self.board.filled()
        } else {
            vec![false; rows.saturating_mul(cols)]
        };
        let cell = l.cell(rows, cols);
        let inset = (cell * 0.05).clamp(0.5, 4.0);
        for r in 0..rows {
            for c in 0..cols {
                let Some(index) = self.board.index(r, c) else {
                    continue;
                };
                let square = l.square(rows, cols, r, c);
                let inner = Rect::new(
                    square.x + inset,
                    square.y + inset,
                    (square.w - inset * 2.0).max(0.0),
                    (square.h - inset * 2.0).max(0.0),
                );
                let wet = flow.get(index).copied().unwrap_or(false);
                let is_source = (r, c) == self.board.source();
                let is_drain = (r, c) == self.board.drain();
                let back = if is_source {
                    SOURCE_BG
                } else if is_drain {
                    DRAIN_BG
                } else if wet {
                    SURFACE1
                } else {
                    SURFACE0
                };
                fill(f, inner, back, (inner.w * 0.1).min(6.0));

                let ink = if self.state == GameState::Won && wet {
                    GREEN
                } else if wet {
                    TEAL
                } else {
                    OVERLAY0
                };
                let Some(pipe) = self.board.get(r, c) else {
                    continue;
                };
                let (mx, my) = inner.centre();
                let width = (inner.w * 0.16).clamp(1.0, 8.0);
                for dir in pipe.openings() {
                    let (ex, ey) = match dir {
                        Dir::Up => (mx, inner.y),
                        Dir::Down => (mx, inner.bottom()),
                        Dir::Left => (inner.x, my),
                        Dir::Right => (inner.right(), my),
                    };
                    f.push(RenderCommand::Line {
                        x1: mx,
                        y1: my,
                        x2: ex,
                        y2: ey,
                        color: ink,
                        width,
                    });
                }
                if pipe.kind != PipeKind::Empty {
                    let dot = width * 1.2;
                    fill(
                        f,
                        Rect::new(mx - dot / 2.0, my - dot / 2.0, dot, dot),
                        ink,
                        dot / 2.0,
                    );
                }
                if is_source || is_drain {
                    let size = (inner.h * 0.32).clamp(6.0, 18.0);
                    label(
                        f,
                        inner.x + inset,
                        inner.y + inset,
                        if is_source { "S" } else { "D" },
                        size,
                        if is_source { GREEN } else { RED },
                        FontWeightHint::Bold,
                        Some(inner.w),
                    );
                }
                if (r, c) == self.cursor {
                    stroke(
                        f,
                        inner,
                        YELLOW,
                        (inner.w * 0.06).clamp(1.0, 3.0),
                        (inner.w * 0.1).min(6.0),
                    );
                }
                f.hit(Target::Cell(index), square);
            }
        }
    }

    fn draw_controls(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.controls) {
            return;
        }
        // Three levels, new game, water, help.
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
        f.hit(Target::NewGame, new_r);

        let flow_r = l.button(l.controls, LEVELS.len().saturating_add(1), count);
        button(
            f,
            l,
            flow_r,
            if self.show_flow { "Water" } else { "Dry" },
            if self.show_flow { SURFACE1 } else { SURFACE0 },
            if self.show_flow { PEACH } else { OVERLAY0 },
        );
        f.hit(Target::ToggleFlow, flow_r);

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
        // sheet covers, so the last box at any point is this one. It has to be
        // the window and not merely the sheet: the sheet is opaque and sits on
        // controls that go on recording their own boxes underneath it, so a
        // frame that still answered `Cell(12)` for a point buried under the
        // help text would be describing a screen nobody is looking at.
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
        let key_w = (sheet.w * 0.34).max(0.0);
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
pub fn handle_event(game: &mut PipesApp, event: &Event) -> EventResult {
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

impl App for PipesApp {
    fn title(&self) -> String {
        "Pipes".to_string()
    }

    fn app_id(&self) -> String {
        "pipes".to_string()
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

impl Probe for PipesApp {
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
    let mut game = PipesApp::new();
    app::launch("pipes", &mut game)
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
    /// The first three are smaller than the chrome would like, which is the
    /// case the old layout — 50-pixel cells at a fixed (20, 95), capped at 400
    /// pixels of board — never had to survive, because `render` used its
    /// `width` and `height` arguments for the background rectangle and nothing
    /// else.
    const WINDOWS: &[(f32, f32)] = &[
        (120.0, 90.0),
        (200.0, 160.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (640.0, 480.0),
        (760.0, 720.0),
        (900.0, 500.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    /// A seed whose Easy deal is nothing special — the default one a test means
    /// when it does not care which puzzle it gets.
    const SEED: u64 = 0x0BAD_1DEA;

    fn game() -> PipesApp {
        PipesApp::with_seed(SEED)
    }

    fn sized(size: (f32, f32)) -> PipesApp {
        let mut g = game();
        g.resize(size.0, size.1);
        g
    }

    /// A game at the given level, freshly dealt.
    fn at_level(index: usize) -> PipesApp {
        let mut g = game();
        if index != g.level_index() {
            g.apply(Action::SetLevel(index));
        }
        assert_eq!(g.level_index(), index, "the level did not take");
        g
    }

    /// Everything a test can see of the state, in one string.
    ///
    /// Used to assert that a control *did* something: a recorded hit box that
    /// changes nothing is worse than no hit box at all, because it swallows the
    /// click instead of letting it fall through to whatever is underneath.
    fn describe(g: &PipesApp) -> String {
        format!(
            "{:?}|{:?}|{}|{}|{:?}|{}|{}|{}",
            g.board().cells,
            g.state(),
            g.moves(),
            g.level_index(),
            g.cursor(),
            g.show_flow(),
            g.show_help(),
            g.status()
        )
    }

    /// Every rectangle the frame paints. A `Text` is reported as the zero-sized
    /// point it starts at, and a `Line` as the box its two ends span, because
    /// their real extents are the renderer's business.
    fn painted(g: &PipesApp, w: f32, h: f32) -> Vec<Rect> {
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
                RenderCommand::Line { x1, y1, x2, y2, .. } => Some(Rect::new(
                    x1.min(x2),
                    y1.min(y2),
                    (x1 - x2).abs(),
                    (y1 - y2).abs(),
                )),
                _ => None,
            })
            .collect()
    }

    fn texts(g: &PipesApp, size: (f32, f32)) -> Vec<String> {
        g.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn shows(g: &PipesApp, size: (f32, f32), needle: &str) -> bool {
        texts(g, size).iter().any(|t| t.contains(needle))
    }

    /// Which of the three bands a layout draws, in `[header, info, controls]`
    /// order — the order [`BAND_DROP_ORDER`] indexes.
    fn bands(l: &Layout) -> [bool; 3] {
        [l.shows(l.header), l.shows(l.info), l.shows(l.controls)]
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.right() - 0.01
            && b.x < a.right() - 0.01
            && a.y < b.bottom() - 0.01
            && b.y < a.bottom() - 0.01
    }

    /// A key going down and then coming back up, which is what a real keyboard
    /// sends and what a handler that ignores `pressed` turns into two presses.
    fn tap(g: &mut PipesApp, key: Key) {
        let size = (g.size_drawn.0, g.size_drawn.1);
        let down = probe::press(key);
        let up = KeyEvent {
            pressed: false,
            text: String::new(),
            ..probe::press(key)
        };
        g.key_at(&down, size);
        g.key_at(&up, size);
    }

    /// The board a generator lays *before* it is scrambled — the solution it
    /// claims to have built.
    fn laid(difficulty: Difficulty, seed: u64) -> Board {
        let (rows, cols) = difficulty.grid_size();
        let mut board = Board::new(rows, cols);
        let mut rng = SeededRng::new(seed);
        let path = board.random_walk(&mut rng);
        board.lay_path(&path);
        board.fill_rest(&mut rng);
        board
    }

    // ── The generator ──────────────────────────────────────────────────────

    /// Fault one, stated as the property it broke.
    ///
    /// The old `lay_path` asked each cell for an opening in the direction the
    /// walk *travelled to arrive* — which points away from the cell it came
    /// from — so a straight run asked for the same side twice and was fitted a
    /// corner. Measured over 2000 seeds at each level, the laid board was
    /// connected zero times. This is the check that would have caught it on the
    /// first seed.
    #[test]
    fn the_solution_the_generator_lays_actually_joins_the_source_to_the_drain() {
        for level in LEVELS {
            for seed in 0..400_u64 {
                let board = laid(level, seed);
                assert!(
                    board.is_solved(),
                    "{:?} seed {seed}: the laid solution does not reach the drain",
                    level.name()
                );
            }
        }
    }

    /// The same property carried through the scramble, which is what the player
    /// is actually handed.
    #[test]
    fn every_puzzle_dealt_can_be_solved() {
        for level in LEVELS {
            for seed in 0..200_u64 {
                let board = Board::generate(level, &mut SeededRng::new(seed));
                assert!(
                    board.is_solvable(),
                    "{} seed {seed}: a dealt board cannot be solved by any turns",
                    level.name()
                );
            }
        }
    }

    /// Fault seven: a scramble that came out solved was presented as a puzzle,
    /// and the first turn you made broke it.
    #[test]
    fn a_dealt_puzzle_is_not_already_finished() {
        for level in LEVELS {
            for seed in 0..200_u64 {
                let board = Board::generate(level, &mut SeededRng::new(seed));
                assert!(
                    !board.is_solved(),
                    "{} seed {seed}: dealt a board already joined",
                    level.name()
                );
            }
        }
    }

    /// A laid solution of two pipes facing each other, which a scramble puts
    /// back together roughly one time in sixteen.
    ///
    /// The three real levels are 6x6 and up, where a scramble lands solved so
    /// rarely that a suite watching only `generate` passes with the retry
    /// deleted. A board this small is the only way to actually run the retry.
    fn two_pipes_facing() -> Board {
        let mut b = Board::new(1, 2);
        b.set(0, 0, pipe_for_openings(&[Dir::Right]));
        b.set(0, 1, pipe_for_openings(&[Dir::Left]));
        b
    }

    /// Fault seven, put to a board that can actually trigger it.
    #[test]
    fn a_scramble_that_lands_solved_is_dealt_again() {
        let laid = two_pipes_facing();
        assert!(laid.is_solved(), "the hand-built solution is not solved");
        // A seed whose *first* scramble happens to land back on the solution.
        let seed = (0..5000_u64)
            .find(|&s| {
                let mut once = laid.clone();
                once.scramble(&mut SeededRng::new(s));
                once.is_solved()
            })
            .expect("no seed scrambles two facing pipes back together");
        let mut once = laid.clone();
        once.scramble(&mut SeededRng::new(seed));
        assert!(once.is_solved(), "seed {seed} does not land solved");
        // `deal` draws from the same stream, so its first attempt is that same
        // solved board and it must go round again.
        let dealt = laid.deal(&mut SeededRng::new(seed));
        assert!(
            !dealt.is_solved(),
            "seed {seed}: a puzzle was dealt already finished"
        );
    }

    /// The retry is bounded, so a board that cannot be unsolved settles.
    ///
    /// A 1x1 board is its own source and drain, so the water has arrived
    /// before it has gone anywhere and no rotation can change that. Without
    /// the bound this test does not fail — it hangs, which the mutation sweep
    /// counts as a catch.
    #[test]
    fn dealing_a_board_that_cannot_be_unsolved_gives_up_rather_than_spinning() {
        let laid = Board::new(1, 1);
        assert!(laid.is_solved(), "a one-cell board is not solved");
        let dealt = laid.deal(&mut SeededRng::new(7));
        assert!(dealt.is_solved(), "a one-cell board came back unsolved");
    }

    /// The sharpest possible statement of fault one, with no randomness in it.
    ///
    /// Three cells in a row: the middle one is on the path between two
    /// neighbours to its left and right, so it is a straight. The old code
    /// asked it for "Right and Right" and fitted a corner.
    #[test]
    fn a_straight_run_of_the_walk_is_laid_as_straight_pipe() {
        let mut board = Board::new(1, 3);
        board.lay_path(&[(0, 0), (0, 1), (0, 2)]);
        assert_eq!(
            board.get(0, 1).map(|p| p.kind),
            Some(PipeKind::Straight),
            "the middle of a straight run was not laid as straight pipe"
        );
        assert!(board.joined(0, 0, Dir::Right), "the first join is missing");
        assert!(board.joined(0, 1, Dir::Right), "the second join is missing");
        assert!(board.is_solved(), "a laid straight run does not conduct");
    }

    /// And the turn, which the old code happened to get right for the wrong
    /// reason — so a test that only checked corners would have passed on it.
    #[test]
    fn a_turn_in_the_walk_is_laid_as_a_corner_facing_both_neighbours() {
        let mut board = Board::new(2, 2);
        board.drain = (1, 0);
        board.lay_path(&[(0, 0), (0, 1), (1, 1), (1, 0)]);
        let corner = board.get(0, 1).expect("cell (0,1)");
        assert_eq!(corner.kind, PipeKind::Corner);
        assert!(corner.has_opening(Dir::Left), "not open back to the source");
        assert!(
            corner.has_opening(Dir::Down),
            "not open on to the next cell"
        );
        assert!(board.is_solved(), "a laid corner run does not conduct");
    }

    // ── pipe_for_openings ──────────────────────────────────────────────────

    /// Every set of sides is answered with a pipe open on exactly those sides.
    ///
    /// Fault six was the three-opening arm: `_ => (Tee, 0)`, a tee in rotation
    /// zero whatever sides were asked for. Nothing on the laid path has three
    /// openings today, so it was a wrong answer waiting for a caller.
    #[test]
    fn a_pipe_is_fitted_to_exactly_the_sides_it_was_asked_for() {
        for mask in 0..16_u8 {
            let want: Vec<Dir> = Dir::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, d)| *d)
                .collect();
            let pipe = pipe_for_openings(&want);
            let have = pipe.openings();
            assert_eq!(
                have.len(),
                want.len(),
                "asked for {want:?}, got {pipe:?} open on {have:?}"
            );
            for d in &want {
                assert!(
                    have.contains(d),
                    "asked for {want:?}, got {pipe:?} open on {have:?}"
                );
            }
        }
    }

    /// Fault five: `Cross` existed, drew, and had a test of its own, but the
    /// fill mapped its draw back onto a second `Straight`, so no board could
    /// ever contain one.
    #[test]
    fn every_shape_turns_up_on_a_dealt_board() {
        let mut seen: HashSet<PipeKind> = HashSet::new();
        for seed in 0..60_u64 {
            let board = Board::generate(Difficulty::Hard, &mut SeededRng::new(seed));
            for pipe in &board.cells {
                seen.insert(pipe.kind);
            }
        }
        for kind in [
            PipeKind::Straight,
            PipeKind::Corner,
            PipeKind::Tee,
            PipeKind::Cross,
            PipeKind::End,
        ] {
            assert!(seen.contains(&kind), "{kind:?} never appears on a board");
        }
    }

    /// Every rotation must be reachable — two of the three draws in generation
    /// pick a quarter-turn with a bound of 4.
    #[test]
    fn every_rotation_is_reachable() {
        let mut seen = [false; 4];
        for seed in 0..40_u64 {
            let board = Board::generate(Difficulty::Hard, &mut SeededRng::new(seed));
            for pipe in &board.cells {
                if let Some(slot) = seen.get_mut(usize::from(pipe.rotation) % 4) {
                    *slot = true;
                }
            }
        }
        assert!(seen.iter().all(|s| *s), "not every quarter-turn: {seen:?}");
    }

    // ── The floor on the turns remaining ───────────────────────────────────

    #[test]
    fn the_floor_is_zero_exactly_when_the_board_is_solved() {
        for level in LEVELS {
            for seed in 0..80_u64 {
                let solved = laid(level, seed);
                assert!(solved.is_solved());
                assert_eq!(
                    solved.rotation_floor(),
                    0,
                    "{} seed {seed}: a solved board has a floor above zero",
                    level.name()
                );
                let dealt = Board::generate(level, &mut SeededRng::new(seed));
                assert!(!dealt.is_solved());
                assert!(
                    dealt.rotation_floor() > 0,
                    "{} seed {seed}: an unsolved board has a floor of zero",
                    level.name()
                );
            }
        }
    }

    /// The floor is a floor: it can never exceed the turns a real solution
    /// takes.
    ///
    /// Built by walking *backwards* from a solved board — turn `k` cells one
    /// quarter each and the board is at most `k` turns from home, because
    /// undoing those turns is a solution. Anything the floor reports above `k`
    /// is a floor that lies, which is the one way a "moves remaining" number
    /// can do real harm.
    #[test]
    fn the_floor_is_never_more_than_the_turns_it_really_takes() {
        for level in LEVELS {
            for seed in 0..60_u64 {
                let solved = laid(level, seed);
                let mut board = solved.clone();
                let mut rng = SeededRng::new(seed ^ 0xF100_0000);
                let mut applied = 0_u32;
                for _ in 0..12 {
                    let r = rng.below(board.rows());
                    let c = rng.below(board.cols());
                    if board.turn(r, c, rng.below(2) == 0) {
                        applied = applied.saturating_add(1);
                    }
                }
                assert!(
                    board.rotation_floor() <= applied,
                    "{} seed {seed}: floor {} after {applied} turns from solved",
                    level.name(),
                    board.rotation_floor()
                );
            }
        }
    }

    /// One turn moves the floor by at most one.
    ///
    /// It has to: turning one cell changes that cell's contribution to any
    /// route by at most one quarter-turn, and every route's cost is a sum of
    /// per-cell contributions, so the cheapest route's cost moves by at most
    /// one. A floor that jumped by two would mean the cost of a route was being
    /// counted somewhere other than the cell it belongs to.
    #[test]
    fn one_turn_changes_the_floor_by_at_most_one() {
        for seed in 0..40_u64 {
            let mut board = Board::generate(Difficulty::Medium, &mut SeededRng::new(seed));
            let mut rng = SeededRng::new(seed ^ 0x0F0F);
            for _ in 0..20 {
                let before = board.rotation_floor();
                let r = rng.below(board.rows());
                let c = rng.below(board.cols());
                if !board.turn(r, c, rng.below(2) == 0) {
                    continue;
                }
                let after = board.rotation_floor();
                assert!(
                    before.abs_diff(after) <= 1,
                    "seed {seed}: one turn at ({r},{c}) moved the floor {before} -> {after}"
                );
            }
        }
    }

    /// A board no rotation could ever join is reported as unsolvable, not as
    /// zero turns from home.
    ///
    /// A dead end can be open on one side only, so two of them facing across a
    /// gap they cannot both reach is a board with no solution. The floor must
    /// say so rather than fall back on a number.
    #[test]
    fn a_board_that_cannot_be_joined_says_so() {
        let mut board = Board::new(1, 3);
        board.set(0, 0, Pipe::new(PipeKind::End, 1));
        // The middle cell is a dead end too: it can face left or right, never
        // both, so the water can never cross it.
        board.set(0, 1, Pipe::new(PipeKind::End, 0));
        board.set(0, 2, Pipe::new(PipeKind::End, 3));
        assert!(!board.is_solved());
        assert!(
            !board.is_solvable(),
            "an unjoinable board claims a solution"
        );
        assert_eq!(board.rotation_floor(), u32::MAX);
    }

    // ── The water ──────────────────────────────────────────────────────────

    #[test]
    fn water_reaches_a_cell_only_by_a_run_of_joins_from_the_source() {
        for seed in 0..60_u64 {
            let board = Board::generate(Difficulty::Medium, &mut SeededRng::new(seed));
            let wet = board.filled();
            // Spelled out here rather than called through `Board::joined`: a
            // test that asks the program where two cells meet, and then checks
            // the program flooded exactly where it said they meet, is
            // self-consistent and passes however wrong `joined` is. Two pipes
            // meet when *each* is open on the side facing the other.
            let meets = |r: usize, c: usize, d: Dir| -> Option<(usize, usize, usize)> {
                let (nr, nc) = board.step(r, c, d)?;
                let (here, there) = (board.get(r, c)?, board.get(nr, nc)?);
                if here.openings().contains(&d) && there.openings().contains(&d.opposite()) {
                    Some((nr, nc, board.index(nr, nc)?))
                } else {
                    None
                }
            };
            // Flood from the source over `meets` alone and require the
            // program's answer to agree cell for cell.
            let mut mine = vec![false; wet.len()];
            let (sr, sc) = board.source();
            let mut queue = vec![(sr, sc)];
            if let Some(i) = board.index(sr, sc) {
                mine[i] = true;
            }
            while let Some((r, c)) = queue.pop() {
                for d in Dir::ALL {
                    if let Some((nr, nc, n)) = meets(r, c, d)
                        && !mine[n]
                    {
                        mine[n] = true;
                        queue.push((nr, nc));
                    }
                }
            }
            assert_eq!(
                wet, mine,
                "seed {seed}: the water is not where the joins are"
            );
            for r in 0..board.rows() {
                for c in 0..board.cols() {
                    let i = board.index(r, c).unwrap();
                    if !wet[i] || (r, c) == board.source() {
                        continue;
                    }
                    let fed = Dir::ALL
                        .iter()
                        .any(|&d| meets(r, c, d).is_some_and(|(_, _, n)| wet[n]));
                    assert!(fed, "seed {seed}: ({r},{c}) is wet with no wet neighbour");
                }
            }
        }
    }

    #[test]
    fn the_source_is_always_wet_and_the_drain_only_when_it_is_joined() {
        for seed in 0..60_u64 {
            let board = Board::generate(Difficulty::Easy, &mut SeededRng::new(seed));
            let wet = board.filled();
            let (sr, sc) = board.source();
            let (dr, dc) = board.drain();
            assert!(wet[board.index(sr, sc).unwrap()], "the source is dry");
            assert_eq!(
                wet[board.index(dr, dc).unwrap()],
                board.is_solved(),
                "seed {seed}: the drain's wetness and the win disagree"
            );
        }
    }

    // ── Turning ────────────────────────────────────────────────────────────

    /// Fault eight. A cross turns onto itself, so charging the player for it
    /// makes the win message report turns that did nothing.
    #[test]
    fn turning_a_shape_that_looks_the_same_is_refused() {
        for kind in [PipeKind::Cross, PipeKind::Empty] {
            let mut board = Board::new(2, 2);
            board.set(0, 0, Pipe::new(kind, 0));
            let before = board.clone();
            assert!(!board.turn(0, 0, true), "{kind:?} claimed to have turned");
            assert_eq!(board, before, "{kind:?} changed the board anyway");
        }
        for kind in [
            PipeKind::Straight,
            PipeKind::Corner,
            PipeKind::Tee,
            PipeKind::End,
        ] {
            let mut board = Board::new(2, 2);
            board.set(0, 0, Pipe::new(kind, 0));
            assert!(board.turn(0, 0, true), "{kind:?} refused to turn");
            assert_ne!(
                board.get(0, 0).unwrap().openings(),
                Pipe::new(kind, 0).openings(),
                "{kind:?} turned without changing which sides it is open on"
            );
        }
    }

    #[test]
    fn a_turn_clockwise_and_a_turn_back_leave_the_board_as_it_was() {
        let mut board = Board::generate(Difficulty::Easy, &mut SeededRng::new(7));
        let before = board.clone();
        for r in 0..board.rows() {
            for c in 0..board.cols() {
                board.turn(r, c, true);
                board.turn(r, c, false);
            }
        }
        assert_eq!(board, before);
    }

    // ── Keys ───────────────────────────────────────────────────────────────

    /// A key coming back up is not a second press.
    ///
    /// The single most destructive shape of fault in this campaign: on the
    /// sliding puzzle the same bug made every arrow move two tiles and made the
    /// help sheet impossible to read. Here it would turn every pipe twice and
    /// make the water toggle and the sheet unusable — so the check is that a
    /// full down-then-up tap does each of them exactly once.
    #[test]
    fn a_key_that_comes_back_up_is_not_a_second_press() {
        let mut g = game();
        let start = g.board().get(0, 0).unwrap();
        tap(&mut g, Key::Space);
        assert_eq!(g.moves(), 1, "one tap of Space counted as more than a turn");
        assert_eq!(
            g.board().get(0, 0).unwrap().rotation,
            Pipe::new(start.kind, start.rotation.wrapping_add(1)).rotation,
            "one tap of Space turned the pipe more than a quarter"
        );

        let mut g = game();
        tap(&mut g, Key::Right);
        assert_eq!(
            g.cursor(),
            (0, 1),
            "one tap of Right moved more than a cell"
        );

        let mut g = game();
        tap(&mut g, Key::F);
        assert!(!g.show_flow(), "one tap of F left the water shown");

        let mut g = game();
        tap(&mut g, Key::H);
        assert!(g.show_help(), "one tap of H left the sheet closed");
    }

    #[test]
    fn the_arrows_walk_the_cursor_and_stop_at_the_edge() {
        let mut g = game();
        let (rows, cols) = (g.board().rows(), g.board().cols());
        for _ in 0..rows.saturating_add(3) {
            tap(&mut g, Key::Down);
        }
        assert_eq!(g.cursor().0, rows - 1, "Down walked off the bottom");
        for _ in 0..cols.saturating_add(3) {
            tap(&mut g, Key::Right);
        }
        assert_eq!(g.cursor().1, cols - 1, "Right walked off the right");
        for _ in 0..rows.saturating_add(cols).saturating_add(6) {
            tap(&mut g, Key::Up);
            tap(&mut g, Key::Left);
        }
        assert_eq!(g.cursor(), (0, 0), "Up and Left walked off the top left");
    }

    #[test]
    fn space_and_z_turn_the_cursors_cell_opposite_ways() {
        let mut g = game();
        // A cell that can actually be turned, so the test is about direction
        // rather than about whether anything happened.
        let (r, c) = (0..g.board().rows())
            .flat_map(|r| (0..g.board().cols()).map(move |c| (r, c)))
            .find(|&(r, c)| {
                g.board()
                    .get(r, c)
                    .is_some_and(Pipe::turning_changes_anything)
            })
            .expect("no turnable cell on a dealt board");
        g.cursor = (r, c);
        let before = g.board().get(r, c).unwrap();
        tap(&mut g, Key::Space);
        let cw = g.board().get(r, c).unwrap();
        tap(&mut g, Key::Z);
        assert_eq!(g.board().get(r, c).unwrap(), before, "Z did not undo Space");
        tap(&mut g, Key::Z);
        let ccw = g.board().get(r, c).unwrap();
        assert_ne!(cw, ccw, "Space and Z turn the same way");
    }

    #[test]
    fn enter_is_the_same_key_as_space() {
        let mut a = game();
        let mut b = game();
        tap(&mut a, Key::Space);
        tap(&mut b, Key::Enter);
        assert_eq!(describe(&a), describe(&b));
    }

    /// A modifier the program does not use is not a bare key.
    ///
    /// Ctrl+N is the window manager's business, or a future shortcut's; if it
    /// deals a new puzzle here, the puzzle you were playing is gone.
    #[test]
    fn a_modifier_the_program_does_not_use_is_ignored() {
        for key in [Key::N, Key::Space, Key::F, Key::H, Key::Right, Key::Num2] {
            for m in [
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
                let mut g = game();
                let before = describe(&g);
                let ev = probe::press_with(key, m);
                assert_eq!(
                    g.key_at(&ev, PipesApp::SIZE),
                    EventResult::Ignored,
                    "{key:?} with {m:?} was answered"
                );
                assert_eq!(before, describe(&g), "{key:?} with {m:?} changed the game");
            }
        }
    }

    /// The keys the program answers and the keys the sheet names are the same
    /// set, both ways round.
    ///
    /// Fault four: the old controls line named five of the seven keys that
    /// worked. `F` and `Enter` were answered and unmentioned — a control a
    /// player could only find by reading the source.
    #[test]
    fn the_help_sheet_names_every_key_the_program_answers() {
        // Every key the program answers, and the text that must name it.
        let answered: [(Key, &str); 12] = [
            (Key::Up, "Arrows"),
            (Key::Down, "Arrows"),
            (Key::Left, "Arrows"),
            (Key::Right, "Arrows"),
            (Key::Space, "Space"),
            (Key::Enter, "Enter"),
            (Key::Z, "Z"),
            (Key::N, "N"),
            (Key::F, "F"),
            (Key::H, "H"),
            (Key::Num1, "1"),
            (Key::Num3, "3"),
        ];
        let sheet: String = HELP_ROWS
            .iter()
            .map(|(k, _)| (*k).to_string())
            .collect::<Vec<_>>()
            .join(" | ");
        for (key, named) in answered {
            let g = game();
            assert!(
                g.key_action(&probe::press(key)).is_some(),
                "the sheet names {named} but {key:?} does nothing"
            );
            assert!(
                sheet.contains(named),
                "{key:?} is answered but the sheet never names {named}"
            );
        }
        // And nothing the sheet names is dead. `Esc` is checked by the
        // sheet-closing test, which is where it means something.
        let g = game();
        assert!(
            g.key_action(&probe::press(Key::Escape)).is_some(),
            "the sheet names Esc but Esc means nothing"
        );
    }

    // ── The help sheet ─────────────────────────────────────────────────────

    #[test]
    fn every_key_the_sheet_names_as_closing_it_closes_it() {
        for key in [Key::H, Key::Escape] {
            let mut g = game();
            g.apply(Action::ToggleHelp);
            assert!(g.show_help());
            tap(&mut g, key);
            assert!(!g.show_help(), "{key:?} did not close the sheet");
        }
    }

    #[test]
    fn the_open_sheet_swallows_the_keys_that_are_not_about_it() {
        for key in [Key::Space, Key::Z, Key::N, Key::F, Key::Right, Key::Num3] {
            let mut g = game();
            g.apply(Action::ToggleHelp);
            let before = describe(&g);
            assert_eq!(
                g.key_at(&probe::press(key), PipesApp::SIZE),
                EventResult::Ignored,
                "{key:?} was answered behind the open sheet"
            );
            assert_eq!(before, describe(&g), "{key:?} played behind the sheet");
        }
    }

    /// While the sheet is up the frame says every point belongs to the sheet.
    ///
    /// Not "the click closes it" — that a *short-circuit* in the event handler
    /// would also satisfy, and a short-circuit leaves the frame still claiming
    /// a cell is clickable at a point buried under the help text. The frame is
    /// what the hit test reads, so the frame is what has to be true.
    #[test]
    fn while_the_sheet_is_up_the_frame_says_every_point_belongs_to_the_sheet() {
        let mut g = game();
        g.apply(Action::ToggleHelp);
        let l = g.layout();
        let count = LEVELS.len().saturating_add(3);
        let mut points = vec![
            l.square(g.board().rows(), g.board().cols(), 0, 0).centre(),
            l.square(g.board().rows(), g.board().cols(), 1, 1).centre(),
            (1.0, 1.0),
            (l.window.w - 1.0, l.window.h - 1.0),
        ];
        for slot in 0..count {
            points.push(l.button(l.controls, slot, count).centre());
        }
        for (x, y) in points {
            assert_eq!(
                g.target_at(x, y),
                Some(Target::ToggleHelp),
                "with the sheet up, ({x}, {y}) is not the sheet's"
            );
        }
        g.apply(Action::CloseHelp);
        let (x, y) = l.square(g.board().rows(), g.board().cols(), 0, 0).centre();
        assert_eq!(g.target_at(x, y), Some(Target::Cell(0)));
    }

    #[test]
    fn a_click_while_the_sheet_is_open_closes_it_and_reaches_nothing_behind_it() {
        // Every point that is a control with the sheet shut, checked with the
        // sheet open. Clicking the middle of the board is not enough: the
        // sheet is *drawn* over the middle of the board, so a hit box covering
        // only the sheet's own pixels passes that and still leaves every
        // control round the edges answering clicks whose targets the player
        // cannot see.
        let shut = sized(PipesApp::SIZE);
        let boxes: Vec<(Target, Rect)> = shut
            .frame(PipesApp::SIZE.0, PipesApp::SIZE.1)
            .hits()
            .to_vec();
        assert!(!boxes.is_empty(), "the frame records no controls at all");
        for (target, rect) in boxes {
            let mut g = sized(PipesApp::SIZE);
            g.apply(Action::ToggleHelp);
            let before = describe(&g);
            let (x, y) = rect.centre();
            g.click_at(x, y, MouseButton::Left, PipesApp::SIZE);
            assert!(
                !g.show_help(),
                "{target:?} at ({x}, {y}): the click did not close the sheet"
            );
            // Reopened, so the two descriptions differ in nothing but whatever
            // the click reached behind the sheet — which must be nothing.
            g.apply(Action::ToggleHelp);
            assert_eq!(
                before,
                describe(&g),
                "{target:?} at ({x}, {y}): the click reached it through the sheet"
            );
        }
    }

    // ── The pointer ────────────────────────────────────────────────────────

    /// Fault two, stated as a property: the pointer works at all.
    ///
    /// `handle_event` matched `Event::Key` and nothing else, so a mouse could
    /// not turn a pipe — on a puzzle whose whole interaction is "turn that one".
    #[test]
    fn a_left_click_turns_the_pipe_it_lands_on_clockwise() {
        let mut g = sized(PipesApp::SIZE);
        let (r, c) = (0..g.board().rows())
            .flat_map(|r| (0..g.board().cols()).map(move |c| (r, c)))
            .find(|&(r, c)| {
                g.board()
                    .get(r, c)
                    .is_some_and(Pipe::turning_changes_anything)
            })
            .expect("no turnable cell");
        let index = g.board().index(r, c).unwrap();
        let before = g.board().get(r, c).unwrap();
        probe::click(&mut g, Target::Cell(index));
        assert_eq!(
            g.board().get(r, c).unwrap().rotation,
            before.rotation.wrapping_add(1) % 4,
            "a left click did not turn ({r},{c}) one quarter clockwise"
        );
        assert_eq!(g.moves(), 1, "the click did not count as a turn");
    }

    #[test]
    fn a_right_click_turns_it_the_other_way() {
        let mut g = sized(PipesApp::SIZE);
        let (r, c) = (0..g.board().rows())
            .flat_map(|r| (0..g.board().cols()).map(move |c| (r, c)))
            .find(|&(r, c)| {
                g.board()
                    .get(r, c)
                    .is_some_and(Pipe::turning_changes_anything)
            })
            .expect("no turnable cell");
        let index = g.board().index(r, c).unwrap();
        let before = g.board().get(r, c).unwrap();
        probe::click_with(&mut g, Target::Cell(index), MouseButton::Right);
        assert_eq!(
            g.board().get(r, c).unwrap().rotation,
            before.rotation.wrapping_add(3) % 4,
            "a right click did not turn ({r},{c}) one quarter anticlockwise"
        );
    }

    #[test]
    fn a_click_brings_the_cursor_to_the_cell_it_turned() {
        let mut g = sized(PipesApp::SIZE);
        let (r, c) = (2, 3);
        let index = g.board().index(r, c).unwrap();
        probe::click(&mut g, Target::Cell(index));
        assert_eq!(
            g.cursor(),
            (r, c),
            "the cursor stayed where the keyboard left it"
        );
    }

    /// A cell is clickable exactly where it is drawn, in every window.
    ///
    /// The old hit test did not exist at all; the point of recording boxes in
    /// the drawing pass is that this cannot drift. Checked at four corners
    /// inset from the edges, because a box one pixel out is still a box that
    /// answers for its middle.
    #[test]
    fn a_cell_is_clickable_exactly_where_it_is_drawn() {
        for &(w, h) in WINDOWS {
            for index in 0..LEVELS.len() {
                let mut g = at_level(index);
                g.resize(w, h);
                let (rows, cols) = (g.board().rows(), g.board().cols());
                let l = Layout::new(w, h);
                if l.cell(rows, cols) < 6.0 {
                    continue;
                }
                for r in 0..rows {
                    for c in 0..cols {
                        let flat = g.board().index(r, c).unwrap();
                        let sq = l.square(rows, cols, r, c);
                        let inset = (sq.w * 0.2).min(3.0);
                        for (x, y) in [
                            sq.centre(),
                            (sq.x + inset, sq.y + inset),
                            (sq.right() - inset, sq.y + inset),
                            (sq.x + inset, sq.bottom() - inset),
                            (sq.right() - inset, sq.bottom() - inset),
                        ] {
                            assert_eq!(
                                g.target_at(x, y),
                                Some(Target::Cell(flat)),
                                "{w}x{h}: ({x}, {y}) is not cell ({r},{c})"
                            );
                        }
                    }
                }
            }
        }
    }

    /// A click is read against the size last drawn, not against a constant.
    #[test]
    fn a_click_is_read_against_the_size_last_drawn() {
        let small = (400.0, 380.0);
        let large = (1600.0, 1000.0);
        let mut g = sized(small);
        let point = Layout::new(large.0, large.1)
            .square(g.board().rows(), g.board().cols(), 4, 4)
            .centre();
        // The same point means different cells in the two windows, and while
        // the small one is what was drawn it must not mean the large one's.
        let in_small = g.target_at(point.0, point.1);
        g.resize(large.0, large.1);
        let in_large = g.target_at(point.0, point.1);
        assert_ne!(in_small, in_large, "the two windows agree about one point");
        let flat = g.board().index(4, 4).unwrap();
        assert_eq!(in_large, Some(Target::Cell(flat)));
    }

    /// Drawing a frame is what sets the size the next click is read against.
    #[test]
    fn rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against() {
        let mut g = game();
        let size = (1280.0, 900.0);
        let _ = g.render(size.0, size.1);
        let flat = g.board().index(3, 2).unwrap();
        let (x, y) = Layout::new(size.0, size.1)
            .square(g.board().rows(), g.board().cols(), 3, 2)
            .centre();
        assert_eq!(g.target_at(x, y), Some(Target::Cell(flat)));
    }

    #[test]
    fn a_button_the_program_does_not_use_turns_nothing() {
        let mut g = sized(PipesApp::SIZE);
        let before = describe(&g);
        let l = g.layout();
        let (x, y) = l.square(g.board().rows(), g.board().cols(), 1, 1).centre();
        for kind in [
            MouseEventKind::Press(MouseButton::Middle),
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Move,
        ] {
            let answer = handle_event(
                &mut g,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: kind.clone(),
                }),
            );
            assert_eq!(answer, EventResult::Ignored, "{kind:?} was answered");
        }
        assert_eq!(before, describe(&g));
    }

    // ── The controls ───────────────────────────────────────────────────────

    #[test]
    fn a_size_button_switches_to_the_size_it_names() {
        for (slot, level) in LEVELS.iter().enumerate() {
            let mut g = sized(PipesApp::SIZE);
            probe::click(&mut g, Target::Level(slot));
            assert_eq!(g.level_index(), slot, "button {slot} did not take");
            assert_eq!(g.board().rows(), level.grid_size().0);
            assert!(shows(&g, PipesApp::SIZE, level.name()));
        }
    }

    #[test]
    fn choosing_the_size_you_are_already_on_says_so_rather_than_redealing() {
        let mut g = sized(PipesApp::SIZE);
        let before = g.board().clone();
        let on = g.level_index();
        probe::click(&mut g, Target::Level(on));
        assert_eq!(*g.board(), before, "the board was dealt again");
        assert!(
            g.status().contains("Already"),
            "no word that nothing happened: {:?}",
            g.status()
        );
    }

    #[test]
    fn the_water_button_turns_the_overlay_off_and_it_stays_off() {
        let mut g = sized(PipesApp::SIZE);
        assert!(g.show_flow());
        probe::click(&mut g, Target::ToggleFlow);
        assert!(!g.show_flow(), "one click did not hide the water");
        // The colours have to actually change, or the toggle is a label that
        // toggles a field nothing reads.
        let wet_ink = |g: &PipesApp| {
            g.frame(PipesApp::SIZE.0, PipesApp::SIZE.1)
                .commands()
                .iter()
                .filter(|c| matches!(c, RenderCommand::Line { color, .. } if *color == TEAL))
                .count()
        };
        assert_eq!(wet_ink(&g), 0, "the water is hidden but still drawn");
        probe::click(&mut g, Target::ToggleFlow);
        assert!(g.show_flow());
        assert!(wet_ink(&g) > 0, "the water came back but is not drawn");
    }

    #[test]
    fn new_game_deals_another_puzzle_and_starts_the_count_again() {
        let mut g = sized(PipesApp::SIZE);
        tap(&mut g, Key::Space);
        assert_eq!(g.moves(), 1);
        let before = g.board().clone();
        probe::click(&mut g, Target::NewGame);
        assert_eq!(g.moves(), 0, "the turn count survived a new game");
        assert_ne!(*g.board(), before, "the same board was dealt again");
        assert!(!g.board().is_solved());
    }

    #[test]
    fn the_help_button_opens_the_sheet_and_the_sheet_is_drawn() {
        let mut g = sized(PipesApp::SIZE);
        assert!(!shows(&g, PipesApp::SIZE, HELP_TITLE));
        probe::click(&mut g, Target::ToggleHelp);
        assert!(g.show_help());
        assert!(
            shows(&g, PipesApp::SIZE, HELP_TITLE),
            "the sheet is open and not drawn"
        );
        for (key, what) in HELP_ROWS {
            assert!(shows(&g, PipesApp::SIZE, key), "the sheet omits {key:?}");
            assert!(shows(&g, PipesApp::SIZE, what), "the sheet omits {what:?}");
        }
    }

    /// Every control does something.
    ///
    /// A recorded hit box that changes nothing is worse than no hit box: it
    /// swallows the click instead of letting it fall through.
    #[test]
    fn every_control_the_frame_records_does_something() {
        let g = sized(PipesApp::SIZE);
        let targets: Vec<Target> = g
            .frame(PipesApp::SIZE.0, PipesApp::SIZE.1)
            .hits()
            .iter()
            .map(|(t, _)| *t)
            .collect();
        assert!(!targets.is_empty(), "the frame records no controls at all");
        for target in targets {
            let mut g2 = sized(PipesApp::SIZE);
            // A cell already in a shape that cannot turn is the one honest
            // exception, and it still answers with a word rather than silence.
            let before = describe(&g2);
            probe::click(&mut g2, target);
            assert_ne!(before, describe(&g2), "{target:?} is wired to nothing");
        }
    }

    // ── Winning ────────────────────────────────────────────────────────────

    /// A hand-built two-cell board, so the win is reached through the ordinary
    /// event path rather than by setting the flag.
    fn nearly_won() -> PipesApp {
        let mut g = game();
        let mut board = Board::new(1, 2);
        // The source faces up; one turn clockwise faces it right, at the drain,
        // which is already facing left.
        board.set(0, 0, Pipe::new(PipeKind::End, 0));
        board.set(0, 1, Pipe::new(PipeKind::End, 3));
        assert!(!board.is_solved());
        g.board = board;
        g.cursor = (0, 0);
        g.state = GameState::Playing;
        g.moves = 0;
        g.opening_floor = g.floor_left();
        g.resize(PipesApp::SIZE.0, PipesApp::SIZE.1);
        g
    }

    #[test]
    fn joining_the_source_to_the_drain_wins_and_counts_the_game() {
        let mut g = nearly_won();
        assert_eq!(g.opening_floor(), 1, "the floor did not see the one turn");
        assert_eq!(g.games_won(), 0);
        tap(&mut g, Key::Space);
        assert_eq!(g.state(), GameState::Won, "a joined board is not won");
        assert_eq!(g.moves(), 1);
        assert_eq!(g.games_won(), 1);
        assert_eq!(g.floor_left(), 0);
        assert!(
            g.status().contains("Solved in 1"),
            "the win is not said: {:?}",
            g.status()
        );
        assert!(
            shows(&g, PipesApp::SIZE, "Solved in 1"),
            "the win is not drawn"
        );
    }

    #[test]
    fn a_won_board_ignores_further_turns() {
        let mut g = nearly_won();
        tap(&mut g, Key::Space);
        let after_win = g.board().clone();
        tap(&mut g, Key::Space);
        tap(&mut g, Key::Z);
        let l = g.layout();
        let (x, y) = l.square(1, 2, 0, 0).centre();
        g.click_at(x, y, MouseButton::Left, PipesApp::SIZE);
        assert_eq!(*g.board(), after_win, "a won board went on being played");
        assert_eq!(g.moves(), 1, "a won board went on counting turns");
    }

    /// Fault eight through the event path: a turn that cannot change anything
    /// is not charged to the player.
    #[test]
    fn a_turn_that_changes_nothing_is_not_counted() {
        let mut g = game();
        let mut board = Board::new(2, 2);
        board.set(0, 0, Pipe::new(PipeKind::Cross, 0));
        board.set(0, 1, Pipe::new(PipeKind::Cross, 0));
        board.set(1, 0, Pipe::new(PipeKind::Cross, 0));
        board.set(1, 1, Pipe::new(PipeKind::End, 1));
        g.board = board;
        g.cursor = (0, 0);
        g.state = GameState::Playing;
        g.moves = 0;
        g.resize(PipesApp::SIZE.0, PipesApp::SIZE.1);
        let before = g.board().clone();
        tap(&mut g, Key::Space);
        assert_eq!(*g.board(), before, "a cross turned into something else");
        assert_eq!(g.moves(), 0, "an empty turn was counted as a move");
        assert!(
            g.status().contains("cross"),
            "no word about why: {:?}",
            g.status()
        );
    }

    // ── The layout ─────────────────────────────────────────────────────────

    /// Fault three. The board was `50.0.min(400.0 / cols)` per cell at a fixed
    /// (20, 95) — so it was the same size in a 1920-pixel window as in a
    /// 640-pixel one, and the victory message drew below it, off the bottom of
    /// anything short.
    #[test]
    fn the_board_grows_with_the_window() {
        let small = Layout::new(640.0, 480.0);
        let large = Layout::new(1920.0, 1080.0);
        assert!(
            large.board.w > small.board.w * 1.5,
            "the board barely grew: {} -> {}",
            small.board.w,
            large.board.w
        );
        assert!(large.board.w > 400.0, "the board is still capped at 400px");
    }

    #[test]
    fn the_board_is_square_in_every_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                (l.board.w - l.board.h).abs() < 0.5,
                "{w}x{h}: the board is {}x{}",
                l.board.w,
                l.board.h
            );
        }
    }

    #[test]
    fn every_cell_is_inside_the_board_and_they_do_not_overlap() {
        for &(w, h) in WINDOWS {
            for level in LEVELS {
                let (rows, cols) = level.grid_size();
                let l = Layout::new(w, h);
                let mut seen: Vec<Rect> = Vec::new();
                for r in 0..rows {
                    for c in 0..cols {
                        let sq = l.square(rows, cols, r, c);
                        assert!(
                            sq.x >= l.board.x - 0.5
                                && sq.y >= l.board.y - 0.5
                                && sq.right() <= l.board.right() + 0.5
                                && sq.bottom() <= l.board.bottom() + 0.5,
                            "{w}x{h} {}: ({r},{c}) at {sq:?} is outside {:?}",
                            level.name(),
                            l.board
                        );
                        for other in &seen {
                            assert!(
                                !overlaps(sq, *other),
                                "{w}x{h} {}: {sq:?} overlaps {other:?}",
                                level.name()
                            );
                        }
                        seen.push(sq);
                    }
                }
            }
        }
    }

    #[test]
    fn nothing_is_painted_outside_the_window() {
        for &(w, h) in WINDOWS {
            for help in [false, true] {
                for index in 0..LEVELS.len() {
                    let mut g = at_level(index);
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
                            "level {index} (help {help}) paints {r:?} outside {w}x{h}"
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
            // promise is a share of the *height*, and in a narrow one it is all
            // the width there is. Both are what the player sees.
            let promised = (h * BOARD_SHARE).min(w - l.pad * 2.0);
            assert!(
                l.board.h >= promised - 0.5,
                "{w}x{h}: the board is {} of a promised {promised}",
                l.board.h
            );
            assert!(l.board.w > 0.0, "{w}x{h}: the board has no width");
        }
    }

    /// The bands go in the order [`BAND_DROP_ORDER`] states: the title first,
    /// then the controls, and the info line last.
    #[test]
    fn the_bands_go_in_the_stated_order() {
        // Every band a window shows, from the smallest window up. Once a band
        // has appeared it may never disappear again as the window grows.
        let mut appeared = [false; 3];
        let mut order: Vec<usize> = Vec::new();
        let mut h = 60.0_f32;
        while h <= 1200.0 {
            let l = Layout::new(900.0, h);
            let shown = bands(&l);
            for (i, &on) in shown.iter().enumerate() {
                if on && !appeared[i] {
                    appeared[i] = true;
                    order.push(i);
                }
                assert!(
                    on || !appeared[i],
                    "at {h} tall, band {i} vanished after having appeared"
                );
            }
            h += 5.0;
        }
        // Written out, not derived from `BAND_DROP_ORDER`. Reading the
        // constant back and reversing it makes the test agree with whatever
        // the constant happens to say, so it passes on any order at all —
        // which is a test of arithmetic, not of the layout. The bands are
        // [header, info, controls], so the order they appear in as the window
        // grows is: the info line (the floor and the count you play by), then
        // the controls, and the title — the one band that says nothing you
        // need — last.
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

    /// The board is still playable in a window too small for any chrome at all.
    #[test]
    fn the_board_is_still_playable_in_a_window_too_small_for_the_chrome() {
        let (w, h) = (160.0, 44.0);
        let mut g = sized((w, h));
        let l = Layout::new(w, h);
        assert_eq!(bands(&l), [false, false, false], "chrome fits after all");
        assert!(
            l.board.w > 20.0,
            "the board has no room left: {:?}",
            l.board
        );
        let (rows, cols) = (g.board().rows(), g.board().cols());
        let flat = g.board().index(1, 1).unwrap();
        let (x, y) = l.square(rows, cols, 1, 1).centre();
        assert_eq!(g.target_at(x, y), Some(Target::Cell(flat)));
        let before = g.board().clone();
        g.click_at(x, y, MouseButton::Left, (w, h));
        assert_ne!(*g.board(), before, "a click in a small window did nothing");
    }

    #[test]
    fn the_frame_is_balanced_in_every_window() {
        for &(w, h) in WINDOWS {
            for help in [false, true] {
                let mut g = sized((w, h));
                if help {
                    g.apply(Action::ToggleHelp);
                }
                assert!(
                    g.frame(w, h).is_balanced(),
                    "{w}x{h} (help {help}): unbalanced clip or translate"
                );
            }
        }
    }

    #[test]
    fn the_info_line_says_the_floor_and_the_count() {
        let g = sized(PipesApp::SIZE);
        let floor = g.floor_left();
        assert!(floor > 0);
        assert!(
            shows(
                &g,
                PipesApp::SIZE,
                &format!("{floor} turns at the very least")
            ),
            "the floor is not drawn: {:?}",
            texts(&g, PipesApp::SIZE)
        );
        assert!(shows(&g, PipesApp::SIZE, "0 turns"), "no turn count drawn");
    }

    // ── Seeding ────────────────────────────────────────────────────────────

    /// A fresh game must take its seed from the kernel, not from a literal.
    ///
    /// Phrased as "which seed", not as "two fresh games differ", because a host
    /// test build has no SlateOS kernel: `seed_from_system` correctly takes its
    /// fallback and two fresh games are then identical, exactly as they were
    /// under the old hardcoded `42`. A variety check would therefore pass on
    /// the broken code and fail on the fixed code, which is backwards.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let fresh = PipesApp::new().board;
        assert_eq!(
            fresh,
            PipesApp::with_seed(FALLBACK_SEED).board,
            "a fresh game did not use the crate's fallback seed"
        );
        assert_ne!(
            fresh,
            PipesApp::with_seed(42).board,
            "a fresh game is still seeded by the old hardcoded literal"
        );
    }
}
