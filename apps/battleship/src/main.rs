//! Slate OS Battleship -- classic naval combat with an AI opponent.
//!
//! Two 10x10 grids side by side -- the player's fleet and the opponent's ocean
//! -- a ship placement phase with rotation and arrow-key positioning, an AI
//! that hunts and then targets, hit and miss markers, sinking announcements,
//! and live stats. Every cell of both grids is clickable as well as typeable.
//!
//! The whole picture is solved from the size the window reports each frame:
//! there is no built-in size the drawing falls back on, and every box a click
//! is tested against is one the drawing pass recorded.
//!
//! Randomness comes from the shared `randrange` crate, seeded from the system.
//! Themed with the Catppuccin Mocha palette.

use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

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

// ── The board ───────────────────────────────────────────────────────

/// The side of both grids, in cells.
const GRID_SIZE: usize = 10;
/// The highest index on either axis, for clamping a cursor onto the board.
const LAST_CELL: usize = GRID_SIZE.saturating_sub(1);

/// The fleet, in the order it is placed.
///
/// Sizes are *not* repeated here. They used to be -- `SHIP_DEFS` carried a
/// `(kind, size)` pair beside `ShipKind::size()`, which is the same five
/// numbers written down twice, free to disagree the moment one is edited. The
/// size is asked of the kind.
const FLEET: [ShipKind; 5] = [
    ShipKind::Carrier,
    ShipKind::Battleship,
    ShipKind::Cruiser,
    ShipKind::Submarine,
    ShipKind::Destroyer,
];

/// The size the window asks for on launch, and what the tests draw at.
///
/// A *starting* size, not the size the drawing assumes: every frame is solved
/// from the size the window reports, and this is only what is asked for first.
const WINDOW_WIDTH: f32 = 1000.0;
const WINDOW_HEIGHT: f32 = 720.0;

// ── Randomness ──────────────────────────────────────────────────────
//
// From `randrange`, not a local LCG. The local one reduced with `state % max`,
// which on a modulus-2^64 generator returns the low bits — and the low bits of
// such a generator are a counter, not noise: bit 0 alternates 0,1,0,1 for ever.
// Every AI ship consumed three draws (orientation, row, column) from
// consecutive states, so `row` and `col` came from states of opposite parity
// and `row + col` was *always odd*. The AI's entire fleet was anchored to one
// colour of the checkerboard, at every seed, and half the board could not hold
// the bow of a ship. See `known-issues.md` and `design-decisions.md` §447.
use randrange::{RandomSource, SeededRng};

// ── What a click can land on ────────────────────────────────────────

/// Everything the drawing pass records a box for.
///
/// A click is answered by asking the frame what was drawn where the pointer
/// is, so a control that moves cannot leave its hit box behind. The two grids
/// are separate variants because they mean opposite things: a cell of the
/// player's own grid is where a ship goes, a cell of the opponent's ocean is
/// where a shell goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A cell of the player's own fleet grid, by row and column.
    Own(u8, u8),
    /// A cell of the opponent's ocean, by row and column.
    Ocean(u8, u8),
    /// The whole of the player's grid, behind its cells.
    OwnBoard,
    /// The whole of the opponent's grid, behind its cells.
    OceanBoard,
    Title,
    Phase,
    Message,
    OwnLabel,
    OceanLabel,
    Stats,
    Help,
}

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes, solved from the size the window reports.
///
/// Every measurement below is a share of the window. The old drawing had
/// twenty-odd `const f32` pixel counts -- `CELL_SIZE: f32 = 36.0` among them --
/// and the window was whatever those constants happened to add up to. A grid
/// that cannot be resized is a grid that is wrong at every size but one.
#[derive(Debug, Clone, Copy)]
struct Layout {
    window: Rect,
    header: Rect,
    message: Rect,
    body: Rect,
    stats: Rect,
    help: Rect,
    pad: f32,
    title: f32,
    font: f32,
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // A floor so a small window is not margined by a fraction of a pixel,
        // a ceiling so a 4K one is not margined by thirty, and a cap at half
        // the shorter side so a window narrower than twice its own margin is
        // not given a margin wider than the window it is a margin inside.
        //
        // The cap needs the floor to mean anything: at a bare 2% of the
        // shorter side it could never bind -- 2% of a length is always less
        // than half of it -- and deleting it changed nothing any window could
        // show, which is how it was found.
        let pad = (w.min(h) * 0.02).clamp(2.0, 24.0).min(w.min(h) / 2.0);
        let title = (h * 0.031).clamp(9.0, 30.0);
        let font = (h * 0.021).clamp(8.0, 20.0);
        let small = (h * 0.018).clamp(7.0, font);

        // Each band takes a share of what the bands before it left, and the
        // body is what is left when all four have taken theirs. Written this
        // way the five heights sum to exactly `h`, every one of them is
        // non-negative, and they cannot come out of order -- so none of them
        // needs a guard saying so. The previous spelling measured the lower
        // two back from the bottom and clamped them against the body: three
        // `max` guards for a case the arithmetic could not produce, and
        // deleting any of the three changed no window at any size.
        let header_h = h * 0.075;
        let rest = h - header_h;
        let message_h = rest * 0.055;
        let rest = rest - message_h;
        let stats_h = rest * 0.14;
        let rest = rest - stats_h;
        let help_h = rest * 0.06;
        let body_h = rest - help_h;

        let header = Rect::new(0.0, 0.0, w, header_h);
        let message = Rect::new(0.0, header.bottom(), w, message_h);
        let body = Rect::new(0.0, message.bottom(), w, body_h);
        let stats = Rect::new(0.0, body.bottom(), w, stats_h);
        let help = Rect::new(0.0, stats.bottom(), w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            message,
            body,
            stats,
            help,
            pad,
            title,
            font,
            small,
        }
    }

    /// The two grids, side by side in the body.
    fn grids(&self) -> Grids {
        Grids::fit(self.body, self.pad, self.small)
    }
}

/// The geometry of the pair of grids: one square-celled 10x10 board each.
#[derive(Debug, Clone, Copy)]
struct Grids {
    /// The top-left of the player's own cells, past its row labels.
    own: (f32, f32),
    /// The top-left of the opponent's cells.
    ocean: (f32, f32),
    /// The side of one cell, gap included.
    step: f32,
    /// The side of the drawn square, which is `step` less its gap.
    cell: f32,
    /// Room reserved to the left of and above each grid for its A-J and 1-10.
    label: f32,
    /// Where each grid's caption sits, above its labels.
    caption_y: f32,
}

impl Grids {
    /// Fit two labelled 10x10 grids side by side into `area`.
    ///
    /// The cell is square and is taken from *both* axes -- the width has to
    /// hold two grids and their labels, the height only one. Fitting to width
    /// alone is what let the old fixed layout run its grids off the bottom of
    /// any window shorter than the one it was written for.
    fn fit(area: Rect, pad: f32, label_font: f32) -> Self {
        let side = f32_from_usize(GRID_SIZE);
        // A label column and a label row, plus a caption line above.
        let label = label_font * 1.6;
        let caption = label_font * 1.6;
        let avail_w = (area.w - pad * 3.0 - label * 2.0).max(0.0);
        let avail_h = (area.h - pad * 2.0 - label - caption).max(0.0);
        let step = (avail_w / (side * 2.0)).min(avail_h / side).max(0.0);
        let cell = (step * 0.94).max(0.0);
        let grid_w = step * side;

        // Both grids and the gutter between them, centred in what is left.
        let total = grid_w * 2.0 + label * 2.0 + pad;
        let left = area.x + (area.w - total).max(0.0) / 2.0;
        let top = area.y + pad + caption + label;

        Self {
            own: (left + label, top),
            ocean: (left + label * 2.0 + grid_w + pad, top),
            step,
            cell,
            label,
            caption_y: area.y + pad,
        }
    }

    /// The drawn square of one cell of a grid whose cells start at `origin`.
    fn cell_rect(self, origin: (f32, f32), row: usize, col: usize) -> Rect {
        Rect::new(
            origin.0 + f32_from_usize(col) * self.step,
            origin.1 + f32_from_usize(row) * self.step,
            self.cell,
            self.cell,
        )
    }

    /// The whole of a grid whose cells start at `origin`.
    fn board_rect(self, origin: (f32, f32)) -> Rect {
        let side = self.step * f32_from_usize(GRID_SIZE);
        Rect::new(origin.0, origin.1, side, side)
    }
}

/// `usize` to `f32` without a lint-suppressed cast at every call site.
///
/// The count of cells on a board is far below `f32`'s exact-integer range, so
/// the conversion is lossless here; it is written once so that is stated once.
fn f32_from_usize(v: usize) -> f32 {
    f32::from(u16::try_from(v).unwrap_or(u16::MAX))
}

/// A byte index for a `Target`, saturating rather than wrapping.
fn byte(v: usize) -> u8 {
    u8::try_from(v).unwrap_or(u8::MAX)
}

/// `rect` shrunk by `pad` on every side, and never inside out.
///
/// A window narrower than twice its own padding would otherwise produce a
/// negative width, which is not a smaller box but a box that starts to the
/// right of where it ends.
fn inset(rect: Rect, pad: f32) -> Rect {
    Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - pad * 2.0).max(0.0),
        (rect.h - pad * 2.0).max(0.0),
    )
}

/// `part` as a percentage of `whole`, and zero when nothing has happened yet.
///
/// Written once because it was written twice: the player's hit rate and the
/// AI's carried the same "no shots yet means 0.0, else part/whole*100" three
/// lines each, one in the state and one buried in the drawing code.
fn percent(part: usize, whole: usize) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    f32_from_usize(part) / f32_from_usize(whole) * 100.0
}

// ── Ship types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ShipKind {
    Carrier,
    Battleship,
    Cruiser,
    Submarine,
    Destroyer,
}

impl ShipKind {
    fn name(self) -> &'static str {
        match self {
            Self::Carrier => "Carrier",
            Self::Battleship => "Battleship",
            Self::Cruiser => "Cruiser",
            Self::Submarine => "Submarine",
            Self::Destroyer => "Destroyer",
        }
    }

    fn size(self) -> usize {
        match self {
            Self::Carrier => 5,
            Self::Battleship => 4,
            Self::Cruiser => 3,
            Self::Submarine => 3,
            Self::Destroyer => 2,
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Carrier => TEAL,
            Self::Battleship => LAVENDER,
            Self::Cruiser => GREEN,
            Self::Submarine => YELLOW,
            Self::Destroyer => PEACH,
        }
    }
}

// ── Orientation ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    fn toggle(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

// ── Ship placement ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ship {
    kind: ShipKind,
    row: usize,
    col: usize,
    orientation: Orientation,
}

impl Ship {
    /// Returns all cells occupied by this ship.
    fn cells(self) -> Vec<(usize, usize)> {
        let size = self.kind.size();
        let mut result = Vec::with_capacity(size);
        for i in 0..size {
            let (r, c) = match self.orientation {
                Orientation::Horizontal => (self.row, self.col.saturating_add(i)),
                Orientation::Vertical => (self.row.saturating_add(i), self.col),
            };
            result.push((r, c));
        }
        result
    }

    /// Returns true if all cells are within the 10x10 grid.
    fn is_within_bounds(self) -> bool {
        let size = self.kind.size();
        match self.orientation {
            Orientation::Horizontal => {
                self.row < GRID_SIZE && self.col.saturating_add(size) <= GRID_SIZE
            }
            Orientation::Vertical => {
                self.row.saturating_add(size) <= GRID_SIZE && self.col < GRID_SIZE
            }
        }
    }
}

// ── Cell state on grids ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellMark {
    Empty,
    Miss,
    Hit,
}

// ── AI firing strategy ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum AiMode {
    /// Firing at random unexplored cells.
    Hunt,
    /// Following up on a hit — trying adjacent cells.
    Target { targets: Vec<(usize, usize)> },
}

#[derive(Debug, Clone)]
struct AiState {
    mode: AiMode,
    /// Tracks which cells the AI has already fired at.
    fired: [[bool; GRID_SIZE]; GRID_SIZE],
    shots: usize,
    hits: usize,
}

impl AiState {
    fn new() -> Self {
        Self {
            mode: AiMode::Hunt,
            fired: [[false; GRID_SIZE]; GRID_SIZE],
            shots: 0,
            hits: 0,
        }
    }

    /// Whether the AI has already fired at a cell.
    ///
    /// A cell off the board reads as *already fired at*, which is what keeps
    /// it out of both the hunt list and the follow-up queue without a second
    /// bounds test at either call site.
    fn has_fired(&self, row: usize, col: usize) -> bool {
        self.fired
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(true)
    }

    /// Pick the next cell to fire at.
    fn choose_target(&mut self, rng: &mut SeededRng) -> (usize, usize) {
        match &self.mode {
            AiMode::Target { targets } if !targets.is_empty() => {
                // Pick the first valid unfired target from the list.
                let targets_clone = targets.clone();
                for &(r, c) in &targets_clone {
                    if !self.has_fired(r, c) {
                        return (r, c);
                    }
                }
                // All queued targets already fired — fall back to hunt.
                self.mode = AiMode::Hunt;
                self.pick_random(rng)
            }
            _ => {
                self.mode = AiMode::Hunt;
                self.pick_random(rng)
            }
        }
    }

    /// Pick a random unfired cell.
    fn pick_random(&self, rng: &mut SeededRng) -> (usize, usize) {
        // Count unfired cells.
        let mut unfired = Vec::new();
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if !self.has_fired(r, c) {
                    unfired.push((r, c));
                }
            }
        }
        // One lookup rather than a length test, a draw and an index: "is there
        // a cell to fire at" and "which one" are the same question, and asking
        // it twice leaves two places free to disagree. `below` is total, so an
        // empty list draws 0 and `get` declines it, which is the old
        // "should not happen in a valid game" fallback of (0, 0).
        rng.choose(&unfired).copied().unwrap_or((0, 0))
    }

    /// Record a shot result and update the mode.
    fn record_shot(&mut self, row: usize, col: usize, hit: bool) {
        if let Some(cell) = self.fired.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = true;
        }
        self.shots = self.shots.saturating_add(1);
        if hit {
            self.hits = self.hits.saturating_add(1);
            // Add adjacent cells as targets. `has_fired` reads an off-board
            // cell as already fired at, so the four neighbours of a corner
            // need no separate bounds test -- the two that do not exist are
            // declined by the same call that declines the ones already shot.
            let mut new_targets = Vec::new();
            for (r, c) in [
                (row.wrapping_sub(1), col),
                (row.saturating_add(1), col),
                (row, col.wrapping_sub(1)),
                (row, col.saturating_add(1)),
            ] {
                if !self.has_fired(r, c) {
                    new_targets.push((r, c));
                }
            }
            match &mut self.mode {
                AiMode::Target { targets } => {
                    for t in new_targets {
                        if !targets.contains(&t) {
                            targets.push(t);
                        }
                    }
                }
                AiMode::Hunt => {
                    self.mode = AiMode::Target {
                        targets: new_targets,
                    };
                }
            }
        } else {
            // Remove this cell from target list if present.
            if let AiMode::Target { targets } = &mut self.mode {
                targets.retain(|&(r, c)| !(r == row && c == col));
                if targets.is_empty() {
                    self.mode = AiMode::Hunt;
                }
            }
        }
    }
}

// ── Game phase ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GamePhase {
    /// Placing ships on the player's grid.
    Placement,
    /// Player and AI take turns firing.
    Firing,
    /// Game is over.
    GameOver,
}

// ── Fleet board ─────────────────────────────────────────────────────

/// A player's fleet: ships and the grid tracking incoming fire.
#[derive(Debug, Clone)]
struct Fleet {
    ships: Vec<Ship>,
    /// Grid tracking which cells have been fired at and the result.
    marks: [[CellMark; GRID_SIZE]; GRID_SIZE],
    /// Grid tracking which cells have a ship (and which ship index).
    ship_grid: [[Option<usize>; GRID_SIZE]; GRID_SIZE],
    /// Per-ship hit count.
    ship_hits: Vec<usize>,
}

impl Fleet {
    fn new() -> Self {
        Self {
            ships: Vec::new(),
            marks: [[CellMark::Empty; GRID_SIZE]; GRID_SIZE],
            ship_grid: [[None; GRID_SIZE]; GRID_SIZE],
            ship_hits: Vec::new(),
        }
    }

    /// Which ship, if any, occupies a cell. Off the board is no ship.
    fn ship_at(&self, row: usize, col: usize) -> Option<usize> {
        self.ship_grid.get(row).and_then(|r| r.get(col)).copied()?
    }

    /// What has been fired at a cell. Off the board reads as untouched.
    fn mark_at(&self, row: usize, col: usize) -> CellMark {
        self.marks
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(CellMark::Empty)
    }

    /// How many hits a ship has taken. An index past the fleet has taken none.
    fn hits_on(&self, idx: usize) -> usize {
        self.ship_hits.get(idx).copied().unwrap_or(0)
    }

    /// Try to place a ship. Returns false if it overlaps or is out of bounds.
    fn place_ship(&mut self, ship: Ship) -> bool {
        // One gate, asked once. `would_overlap` already answers "in bounds and
        // clear of every other ship"; this used to re-ask both halves itself,
        // so the rule for whether a ship fits was written twice and the AI's
        // placement loop asked one copy while the player's asked the other.
        if self.would_overlap(&ship) {
            return false;
        }
        let idx = self.ships.len();
        self.ships.push(ship);
        self.ship_hits.push(0);
        for (r, c) in ship.cells() {
            if let Some(cell) = self.ship_grid.get_mut(r).and_then(|row| row.get_mut(c)) {
                *cell = Some(idx);
            }
        }
        true
    }

    /// Fire at a cell. Returns (hit, sunk_ship_kind).
    fn receive_fire(&mut self, row: usize, col: usize) -> (bool, Option<ShipKind>) {
        if row >= GRID_SIZE || col >= GRID_SIZE {
            return (false, None);
        }
        if self.mark_at(row, col) != CellMark::Empty {
            return (false, None); // Already fired here.
        }
        let hit_ship = self.ship_at(row, col);
        if let Some(cell) = self.marks.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = if hit_ship.is_some() {
                CellMark::Hit
            } else {
                CellMark::Miss
            };
        }
        let Some(ship_idx) = hit_ship else {
            return (false, None);
        };
        if let Some(hits) = self.ship_hits.get_mut(ship_idx) {
            *hits = hits.saturating_add(1);
        }
        match self.ships.get(ship_idx) {
            Some(ship) if self.hits_on(ship_idx) >= ship.kind.size() => (true, Some(ship.kind)),
            _ => (true, None),
        }
    }

    /// Returns true if all ships are sunk.
    fn all_sunk(&self) -> bool {
        !self.ships.is_empty() && self.ships_remaining() == 0
    }

    /// Returns the number of ships still afloat.
    fn ships_remaining(&self) -> usize {
        self.ships
            .iter()
            .enumerate()
            .filter(|&(i, ship)| self.hits_on(i) < ship.kind.size())
            .count()
    }

    /// Returns true if a ship at the given index is sunk.
    fn is_ship_sunk(&self, idx: usize) -> bool {
        self.ships
            .get(idx)
            .is_some_and(|ship| self.hits_on(idx) >= ship.kind.size())
    }

    /// Returns true if the cell (row, col) belongs to a sunk ship.
    fn is_cell_sunk(&self, row: usize, col: usize) -> bool {
        self.ship_at(row, col)
            .is_some_and(|idx| self.is_ship_sunk(idx))
    }

    /// Check if placing a ship would overlap with existing ships.
    fn would_overlap(&self, ship: &Ship) -> bool {
        if !ship.is_within_bounds() {
            return true;
        }
        ship.cells()
            .into_iter()
            .any(|(r, c)| self.ship_at(r, c).is_some())
    }

    /// Check if a cell has already been fired upon.
    fn already_fired(&self, row: usize, col: usize) -> bool {
        if row >= GRID_SIZE || col >= GRID_SIZE {
            return true;
        }
        self.mark_at(row, col) != CellMark::Empty
    }
}

// ── Main application ────────────────────────────────────────────────

struct BattleshipApp {
    phase: GamePhase,
    player_fleet: Fleet,
    opponent_fleet: Fleet,
    ai_state: AiState,
    rng: SeededRng,

    // Placement state
    placement_index: usize,
    placement_row: usize,
    placement_col: usize,
    placement_orientation: Orientation,

    // Firing state — cursor on the opponent's grid
    cursor_row: usize,
    cursor_col: usize,

    // Stats
    player_shots: usize,
    player_hits: usize,

    // Messages
    message: String,
    last_sunk_message: String,

    // Game over
    player_won: bool,

    /// The size the last frame was drawn at, and so the size the next click
    /// is read against. Not a size the drawing falls back on: every frame is
    /// solved from the size the window reports for that frame.
    size: (f32, f32),
}

impl BattleshipApp {
    /// A game on a board nobody has seen before.
    ///
    /// The module doc has always said the seed comes from the system "so that
    /// two players do not get the same game". It did not: `new` wrote
    /// `SeededRng::new(0xDEAD_BEEF_CAFE_1234)`, one constant, so the AI's five
    /// ships stood in the same five places on every machine and in every
    /// session for ever. The doc comment was right about the intent and wrong
    /// about the code, which is the worst of the two ways for them to disagree.
    ///
    /// The fallback is not that constant. Where there is no kernel randomness
    /// to open there is one fixed board and nothing to be done about it, but
    /// making it *the* board the old code shipped would have left every
    /// machine without a kernel source playing the same five ships the bug
    /// gave them -- the fault moved rather than fixed, and invisible to a test
    /// run anywhere the fallback is what answers.
    fn new() -> Self {
        Self::with_seed(randrange::seed_from_system(0x4241_5454_4C45_5348))
    }

    /// The same game from a stated seed, so a test can name the board it means.
    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            phase: GamePhase::Placement,
            player_fleet: Fleet::new(),
            opponent_fleet: Fleet::new(),
            ai_state: AiState::new(),
            rng: SeededRng::new(seed),
            placement_index: 0,
            placement_row: 0,
            placement_col: 0,
            placement_orientation: Orientation::Horizontal,
            cursor_row: 0,
            cursor_col: 0,
            player_shots: 0,
            player_hits: 0,
            message: String::new(),
            last_sunk_message: String::new(),
            player_won: false,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        app.place_ai_ships();
        app.message = app.placement_prompt();
        app
    }

    /// Resets the game to the initial state for a new game.
    fn new_game(&mut self) {
        self.phase = GamePhase::Placement;
        self.player_fleet = Fleet::new();
        self.opponent_fleet = Fleet::new();
        self.ai_state = AiState::new();
        self.placement_index = 0;
        self.placement_row = 0;
        self.placement_col = 0;
        self.placement_orientation = Orientation::Horizontal;
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.player_shots = 0;
        self.player_hits = 0;
        self.message = String::new();
        self.last_sunk_message = String::new();
        self.player_won = false;
        self.place_ai_ships();
        self.message = self.placement_prompt();
    }

    /// Place AI ships randomly, ensuring no overlaps and within bounds.
    ///
    /// The loop asks `place_ship` whether the ship fits and believes the
    /// answer. It used to test bounds and overlap itself, place the ship, and
    /// throw away the `bool` that says whether the placement it just made
    /// actually happened -- two copies of the rule with the authoritative one
    /// silently discarded.
    fn place_ai_ships(&mut self) {
        self.opponent_fleet = Fleet::new();
        for &kind in &FLEET {
            // Safety valve: prevent infinite loops if RNG is pathological.
            for _ in 0..1000 {
                let orientation = if self.rng.flip() {
                    Orientation::Horizontal
                } else {
                    Orientation::Vertical
                };
                let ship = Ship {
                    kind,
                    row: self.rng.below(GRID_SIZE),
                    col: self.rng.below(GRID_SIZE),
                    orientation,
                };
                if self.opponent_fleet.place_ship(ship) {
                    break;
                }
            }
        }
    }

    /// The ship kind currently being placed (if any).
    fn current_placement_ship(&self) -> Option<ShipKind> {
        FLEET.get(self.placement_index).copied()
    }

    /// What to tell the player to place next.
    ///
    /// One sentence in one place. It used to be written out three times -- in
    /// `new`, in `new_game` and again after each successful placement -- and
    /// two of those spelled the Carrier and its size as literals, so a change
    /// to the fleet would have left the opening prompt naming a ship the game
    /// no longer starts with.
    fn placement_prompt(&self) -> String {
        match self.current_placement_ship() {
            Some(kind) => format!(
                "Place your {} ({}). R to rotate, Enter to place.",
                kind.name(),
                kind.size()
            ),
            None => String::from("All ships placed! Select target and fire (Enter)."),
        }
    }

    /// Build the preview ship for placement.
    fn placement_preview_ship(&self) -> Option<Ship> {
        self.current_placement_ship().map(|kind| Ship {
            kind,
            row: self.placement_row,
            col: self.placement_col,
            orientation: self.placement_orientation,
        })
    }

    /// Returns whether the current placement preview is valid.
    fn is_placement_valid(&self) -> bool {
        if let Some(ship) = self.placement_preview_ship() {
            ship.is_within_bounds() && !self.player_fleet.would_overlap(&ship)
        } else {
            false
        }
    }

    /// Clamp the placement cursor so the ship stays within bounds.
    ///
    /// Called after *every* change to the cursor or the orientation, not after
    /// some of them. It used to be called on Down and Right and not on Up and
    /// Left, which was harmless only because those two happened to move away
    /// from the edge that the clamp guards -- a coincidence, not a reason, and
    /// one that a click placing the cursor anywhere at all destroys.
    fn clamp_placement(&mut self) {
        let Some(kind) = self.current_placement_ship() else {
            return;
        };
        let size = kind.size();
        // The last origin at which a ship of this size still ends on the board.
        let last_along = GRID_SIZE.saturating_sub(size);
        let (max_row, max_col) = match self.placement_orientation {
            Orientation::Horizontal => (LAST_CELL, last_along),
            Orientation::Vertical => (last_along, LAST_CELL),
        };
        self.placement_row = self.placement_row.min(max_row);
        self.placement_col = self.placement_col.min(max_col);
    }

    /// Handle keyboard input.
    ///
    /// Escape does *not* start a new game. It used to, under a comment reading
    /// "Could quit, but we just reset for now" -- so the key a player presses
    /// to back out of something threw away the game in progress, silently and
    /// with no confirmation, from any phase. Escape now closes the window,
    /// which is what it does everywhere else, and N alone deals a new board.
    fn handle_key(&mut self, key: Key) -> EventResult {
        match key {
            Key::N => {
                self.new_game();
                EventResult::Consumed
            }
            _ => match self.phase {
                GamePhase::Placement => self.handle_placement_key(key),
                GamePhase::Firing => self.handle_firing_key(key),
                // Only N is handled above. Saying so with `Ignored` is what
                // lets the window leave a key it did not use to whatever is
                // behind it, rather than swallowing every keystroke in the
                // one phase where the game has nothing left to do.
                GamePhase::GameOver => EventResult::Ignored,
            },
        }
    }

    fn handle_placement_key(&mut self, key: Key) -> EventResult {
        match key {
            Key::Up => self.placement_row = self.placement_row.saturating_sub(1),
            Key::Down => self.placement_row = self.placement_row.saturating_add(1),
            Key::Left => self.placement_col = self.placement_col.saturating_sub(1),
            Key::Right => self.placement_col = self.placement_col.saturating_add(1),
            Key::R => self.placement_orientation = self.placement_orientation.toggle(),
            Key::Enter => {
                self.try_place_current_ship();
                return EventResult::Consumed;
            }
            _ => return EventResult::Ignored,
        }
        // Every move and every rotation is followed by the same clamp, because
        // every one of them can carry the ship's far end off the board.
        self.clamp_placement();
        EventResult::Consumed
    }

    fn try_place_current_ship(&mut self) {
        if let Some(ship) = self.placement_preview_ship() {
            if self.player_fleet.place_ship(ship) {
                self.placement_index = self.placement_index.saturating_add(1);
                self.placement_row = 0;
                self.placement_col = 0;
                self.placement_orientation = Orientation::Horizontal;
                if self.placement_index >= FLEET.len() {
                    self.phase = GamePhase::Firing;
                }
                // One sentence in one place: `placement_prompt` already says
                // "all placed" when there is nothing left to place, so the
                // last placement does not need its own copy of that string.
                self.message = self.placement_prompt();
            } else {
                self.message = String::from("Invalid placement! Ship overlaps or out of bounds.");
            }
        }
    }

    fn handle_firing_key(&mut self, key: Key) -> EventResult {
        match key {
            Key::Up => self.cursor_row = self.cursor_row.saturating_sub(1),
            Key::Down => self.cursor_row = self.cursor_row.saturating_add(1).min(LAST_CELL),
            Key::Left => self.cursor_col = self.cursor_col.saturating_sub(1),
            Key::Right => self.cursor_col = self.cursor_col.saturating_add(1).min(LAST_CELL),
            Key::Enter | Key::Space => self.fire_at_opponent(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    fn fire_at_opponent(&mut self) {
        if self
            .opponent_fleet
            .already_fired(self.cursor_row, self.cursor_col)
        {
            self.message = String::from("Already fired there! Choose a new target.");
            return;
        }

        let (hit, sunk) = self
            .opponent_fleet
            .receive_fire(self.cursor_row, self.cursor_col);
        self.player_shots = self.player_shots.saturating_add(1);
        if hit {
            self.player_hits = self.player_hits.saturating_add(1);
        }

        if let Some(kind) = sunk {
            self.last_sunk_message = format!("You sank their {}!", kind.name());
            self.message = self.last_sunk_message.clone();
        } else if hit {
            self.message = String::from("Hit!");
        } else {
            self.message = String::from("Miss!");
        }

        // Check if player won.
        if self.opponent_fleet.all_sunk() {
            self.phase = GamePhase::GameOver;
            self.player_won = true;
            self.message = String::from("VICTORY! You sank all enemy ships! Press N for new game.");
            return;
        }

        // AI's turn.
        self.ai_turn();
    }

    fn ai_turn(&mut self) {
        let (ar, ac) = self.ai_state.choose_target(&mut self.rng);
        let (hit, sunk) = self.player_fleet.receive_fire(ar, ac);
        self.ai_state.record_shot(ar, ac, hit);

        if let Some(kind) = sunk {
            self.message = format!("AI sank your {}! Select your next target.", kind.name());
        } else if hit {
            // Keep the player's message if they sank something, otherwise note AI hit.
            if self.last_sunk_message.is_empty() || !self.message.starts_with("You sank") {
                self.message = String::from("AI hit your ship! Select your next target.");
            } else {
                // Player just sank something; append AI info.
                self.message.push_str(" AI hit your ship! Your turn.");
            }
        } else if !self.message.starts_with("You sank") {
            // The AI missed -- this arm is only reached when `hit` is false --
            // so what goes in front of "AI missed" is whatever the player's own
            // shot said. It used to be written `if hit { "Hit!" } else { … }`,
            // an arm that could not be taken, inside the branch that has
            // already established the AI did not hit anything.
            self.message = format!("{} AI missed. Your turn.", self.message);
        }

        // Clear last_sunk_message after the full turn.
        self.last_sunk_message.clear();

        // Check if AI won.
        if self.player_fleet.all_sunk() {
            self.phase = GamePhase::GameOver;
            self.player_won = false;
            self.message = String::from("DEFEAT! All your ships are sunk! Press N for new game.");
        }
    }

    /// Compute the player's hit rate as a percentage.
    fn player_hit_rate(&self) -> f32 {
        percent(self.player_hits, self.player_shots)
    }

    /// Compute the AI's hit rate as a percentage.
    fn ai_hit_rate(&self) -> f32 {
        percent(self.ai_state.hits, self.ai_state.shots)
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// The word for the phase the game is in, and the colour it is said in.
    ///
    /// One `match`, not two. The word and its colour used to be chosen by two
    /// separate `match self.phase` arms twenty lines apart, so a phase could
    /// be renamed in one and left coloured by the other.
    fn phase_label(&self) -> (&'static str, Color) {
        match self.phase {
            GamePhase::Placement => ("Ship Placement", YELLOW),
            GamePhase::Firing => ("Battle", GREEN),
            GamePhase::GameOver if self.player_won => ("Victory!", GREEN),
            GamePhase::GameOver => ("Defeat!", RED),
        }
    }

    /// The one line of key help for the phase the game is in.
    fn help_line(&self) -> &'static str {
        match self.phase {
            GamePhase::Placement => {
                "Arrows or click: move  |  R: rotate  |  Enter or click: place  |  N: new game"
            }
            GamePhase::Firing => {
                "Arrows or click: aim  |  Enter, Space or click: fire  |  N: new game"
            }
            GamePhase::GameOver => "N: new game  |  Esc: close",
        }
    }

    /// Draw one whole frame at `w` x `h`, hit boxes and all.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(l.window.w, l.window.h);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.window.w,
            height: l.window.h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });
        // A window too small for its own contents crops them rather than
        // painting over its neighbours: the clip is the window itself, so the
        // last row of a grid that does not fit is cut off, not spilled.
        f.clip(l.window);
        self.draw_header(&l, &mut f);
        self.draw_message(&l, &mut f);
        self.draw_body(&l, &mut f);
        self.draw_stats(&l, &mut f);
        self.draw_help(&l, &mut f);
        f.unclip();
        f
    }

    /// The title on the left of the header, the phase on the right.
    fn draw_header(&self, l: &Layout, f: &mut Frame<Target>) {
        let ink = Ink::new(l.title, FontWeightHint::Bold, LAVENDER);
        let band = inset(l.header, l.pad);
        let title = label_in(f, band, "Battleship", ink);
        f.hit(Target::Title, title);

        let (word, colour) = self.phase_label();
        let ink = Ink::new(l.title, FontWeightHint::Bold, colour);
        // Right-aligned by measuring the words, not by a column the words are
        // assumed to fit in: the phase names differ in length by half again,
        // and the window they sit in is whatever the user dragged it to.
        let w = ink.width(word).min(band.w);
        let right = Rect::new(band.right() - w, band.y, w, band.h);
        let phase = label_in(f, right, word, ink);
        f.hit(Target::Phase, phase);
    }

    /// The single line of running commentary under the header.
    fn draw_message(&self, l: &Layout, f: &mut Frame<Target>) {
        if l.message.is_empty() {
            return;
        }
        f.push(RenderCommand::FillRect {
            x: l.message.x + l.pad,
            y: l.message.y,
            width: (l.message.w - l.pad * 2.0).max(0.0),
            height: l.message.h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        let band = inset(l.message, l.pad);
        let ink = Ink::new(l.font, FontWeightHint::Regular, TEXT_COLOR);
        label_in(f, band, &self.message, ink);
        // The box a click is answered by is the whole bar, not the glyphs: the
        // message is what the bar is *for*, and an empty message would
        // otherwise leave a strip of the window answering nothing at all.
        f.hit(Target::Message, l.message);
    }

    /// The two grids, side by side, captions and labels and all.
    fn draw_body(&self, l: &Layout, f: &mut Frame<Target>) {
        let g = l.grids();
        self.draw_own_grid(l, f, g);
        self.draw_ocean_grid(l, f, g);
    }

    /// The caption, the A-J column and the 1-10 row around one grid.
    fn draw_grid_chrome(l: &Layout, f: &mut Frame<Target>, g: Grids, side: Side) {
        let origin = side.origin(g);
        let board = g.board_rect(origin);
        let ink = Ink::new(l.small, FontWeightHint::Bold, SUBTEXT0);
        let caption = side.caption();
        // Centred over the grid it names, by measuring it. The old drawing
        // subtracted a hand-tuned half-width -- 40 for "Your Fleet", 55 for
        // "Opponent's Ocean" -- so the two captions were centred to different
        // standards and neither survived a change of font size.
        let w = ink.width(caption);
        let x = board.x + (board.w - w) / 2.0;
        let rect = label(f, x, g.caption_y, caption, ink);
        f.hit(side.label_target(), rect);

        let ink = Ink::new(l.small, FontWeightHint::Regular, OVERLAY0);
        let line = ink.height();
        for c in 0..GRID_SIZE {
            let s = format!("{}", c.saturating_add(1));
            let cell = g.cell_rect(origin, 0, c);
            let x = cell.x + (cell.w - ink.width(&s)) / 2.0;
            let y = origin.1 - g.label + (g.label - line) / 2.0;
            label(f, x, y, &s, ink);
        }
        for r in 0..GRID_SIZE {
            let s = String::from(char::from(b'A'.saturating_add(byte(r))));
            let cell = g.cell_rect(origin, r, 0);
            let x = origin.0 - g.label + (g.label - ink.width(&s)) / 2.0;
            let y = cell.y + (cell.h - line) / 2.0;
            label(f, x, y, &s, ink);
        }
    }

    /// The player's own fleet, ships visible, with the AI's shots marked.
    fn draw_own_grid(&self, l: &Layout, f: &mut Frame<Target>, g: Grids) {
        Self::draw_grid_chrome(l, f, g, Side::Own);
        let origin = g.own;
        let board = g.board_rect(origin);
        Self::draw_board_backing(f, g, board);
        f.hit(Target::OwnBoard, board);

        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let rect = g.cell_rect(origin, r, c);
                let colour = match self.player_fleet.ship_at(r, c) {
                    Some(idx) if self.player_fleet.is_ship_sunk(idx) => OVERLAY0,
                    Some(idx) => self
                        .player_fleet
                        .ships
                        .get(idx)
                        .map_or(SURFACE0, |s| s.kind.color()),
                    None => SURFACE0,
                };
                Self::draw_cell(f, rect, colour, g.cell);
                Self::draw_mark(f, rect, self.player_fleet.mark_at(r, c));
                f.hit(Target::Own(byte(r), byte(c)), rect);
            }
        }

        // The ship the player is placing, floating over the board.
        if self.phase == GamePhase::Placement
            && let Some(ship) = self.placement_preview_ship()
        {
            let tint = if self.is_placement_valid() {
                Color::rgba(166, 227, 161, 120)
            } else {
                Color::rgba(243, 139, 168, 120)
            };
            for (r, c) in ship.cells() {
                if r < GRID_SIZE && c < GRID_SIZE {
                    Self::draw_cell(f, g.cell_rect(origin, r, c), tint, g.cell);
                }
            }
        }
    }

    /// The opponent's ocean: water until fired upon, ships revealed at the end.
    fn draw_ocean_grid(&self, l: &Layout, f: &mut Frame<Target>, g: Grids) {
        Self::draw_grid_chrome(l, f, g, Side::Ocean);
        let origin = g.ocean;
        let board = g.board_rect(origin);
        Self::draw_board_backing(f, g, board);
        f.hit(Target::OceanBoard, board);

        let reveal = self.phase == GamePhase::GameOver;
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let rect = g.cell_rect(origin, r, c);
                let colour = if self.opponent_fleet.is_cell_sunk(r, c) {
                    OVERLAY0
                } else if reveal {
                    self.opponent_fleet
                        .ship_at(r, c)
                        .and_then(|idx| self.opponent_fleet.ships.get(idx))
                        .map_or(SURFACE0, |s| s.kind.color())
                } else {
                    SURFACE0
                };
                Self::draw_cell(f, rect, colour, g.cell);
                Self::draw_mark(f, rect, self.opponent_fleet.mark_at(r, c));
                f.hit(Target::Ocean(byte(r), byte(c)), rect);
            }
        }

        if self.phase == GamePhase::Firing {
            let rect = g.cell_rect(origin, self.cursor_row, self.cursor_col);
            f.push(RenderCommand::StrokeRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: YELLOW,
                line_width: (g.cell * 0.07).max(1.0),
                corner_radii: CornerRadii::all(g.cell * 0.12),
            });
        }
    }

    /// The dark mat a grid's cells sit on, a hair larger than the cells.
    fn draw_board_backing(f: &mut Frame<Target>, g: Grids, board: Rect) {
        let bleed = (g.step - g.cell).max(0.0);
        f.push(RenderCommand::FillRect {
            x: board.x - bleed,
            y: board.y - bleed,
            width: (board.w + bleed).max(0.0),
            height: (board.h + bleed).max(0.0),
            color: CRUST,
            corner_radii: CornerRadii::all(g.cell * 0.2),
        });
    }

    /// One square of water, ship or wreck.
    fn draw_cell(f: &mut Frame<Target>, rect: Rect, colour: Color, cell: f32) {
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: colour,
            corner_radii: CornerRadii::all(cell * 0.12),
        });
    }

    /// What a shot left behind: a red cross for a hit, a blue dot for a miss.
    fn draw_mark(f: &mut Frame<Target>, rect: Rect, mark: CellMark) {
        let (cx, cy) = rect.centre();
        let half = rect.w * 0.26;
        match mark {
            CellMark::Hit => {
                let width = (rect.w * 0.08).max(1.0);
                for (dx, dy) in [(-half, -half), (half, -half)] {
                    f.push(RenderCommand::Line {
                        x1: cx + dx,
                        y1: cy + dy,
                        x2: cx - dx,
                        y2: cy - dy,
                        color: RED,
                        width,
                    });
                }
            }
            CellMark::Miss => {
                f.push(RenderCommand::FillRect {
                    x: cx - half / 2.0,
                    y: cy - half / 2.0,
                    width: half,
                    height: half,
                    color: BLUE,
                    corner_radii: CornerRadii::all(half / 2.0),
                });
            }
            CellMark::Empty => {}
        }
    }

    /// Six figures on two rows: the player's, the fleets', the AI's.
    fn draw_stats(&self, l: &Layout, f: &mut Frame<Target>) {
        if l.stats.is_empty() {
            return;
        }
        f.push(RenderCommand::FillRect {
            x: l.stats.x + l.pad,
            y: l.stats.y,
            width: (l.stats.w - l.pad * 2.0).max(0.0),
            height: l.stats.h,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });
        f.hit(Target::Stats, l.stats);

        let mine = self.player_fleet.ships_remaining();
        let theirs = self.opponent_fleet.ships_remaining();
        let fleet_colour = |left: usize| if left <= 1 { RED } else { GREEN };
        let rows = [
            [
                (format!("Shots: {}", self.player_shots), TEXT_COLOR, false),
                (
                    format!("Your Ships: {}/{}", mine, FLEET.len()),
                    fleet_colour(mine),
                    true,
                ),
                (
                    format!("AI Shots: {}", self.ai_state.shots),
                    TEXT_COLOR,
                    false,
                ),
            ],
            [
                (
                    format!("Hit Rate: {:.1}%", self.player_hit_rate()),
                    TEXT_COLOR,
                    false,
                ),
                (
                    format!("Enemy Ships: {}/{}", theirs, FLEET.len()),
                    fleet_colour(theirs),
                    true,
                ),
                (
                    format!("AI Hit Rate: {:.1}%", self.ai_hit_rate()),
                    TEXT_COLOR,
                    false,
                ),
            ],
        ];

        // Three even columns and two even rows carved out of whatever height
        // the band was given, rather than the old `stats_y + 8` and
        // `stats_y + 28` -- two offsets into a box of a fixed 56 pixels.
        let inner = inset(l.stats, l.pad);
        let row_h = inner.h / 2.0;
        let col_w = inner.w / 3.0;
        for (r, row) in rows.iter().enumerate() {
            for (c, (words, colour, bold)) in row.iter().enumerate() {
                let weight = if *bold {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                };
                let cell = Rect::new(
                    inner.x + f32_from_usize(c) * col_w,
                    inner.y + f32_from_usize(r) * row_h,
                    col_w,
                    row_h,
                );
                label_in(f, cell, words, Ink::new(l.small, weight, *colour));
            }
        }
    }

    /// The key help along the bottom.
    fn draw_help(&self, l: &Layout, f: &mut Frame<Target>) {
        if l.help.is_empty() {
            return;
        }
        let ink = Ink::new(l.small, FontWeightHint::Regular, OVERLAY0);
        label_in(f, inset(l.help, l.pad), self.help_line(), ink);
        f.hit(Target::Help, l.help);
    }

    // ── Input ───────────────────────────────────────────────────────

    /// The size the next frame will be drawn at, and the next click read
    /// against.
    fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// Act on a click at window coordinates, by asking the frame what was
    /// drawn there.
    ///
    /// A 10x10 grid is the most click-natural thing a program can put on a
    /// screen, and this one could not be clicked at all: every cell was
    /// reachable only by walking a cursor to it with the arrow keys.
    fn click(&mut self, x: f32, y: f32, button: MouseButton) -> EventResult {
        if button != MouseButton::Left {
            return EventResult::Ignored;
        }
        let (w, h) = self.size;
        let Some(target) = self.frame(w, h).hit_test(x, y) else {
            return EventResult::Ignored;
        };
        match target {
            // A click on one's own board places the ship being placed there,
            // in one action rather than two: the cursor goes to the cell and
            // the placement is attempted, exactly as arrow keys then Enter
            // would have done. An invalid cell is refused with the same
            // message the keys get, so a mis-click costs nothing.
            Target::Own(r, c) if self.phase == GamePhase::Placement => {
                self.placement_row = usize::from(r);
                self.placement_col = usize::from(c);
                self.clamp_placement();
                self.try_place_current_ship();
                EventResult::Consumed
            }
            Target::Ocean(r, c) if self.phase == GamePhase::Firing => {
                self.cursor_row = usize::from(r);
                self.cursor_col = usize::from(c);
                self.fire_at_opponent();
                EventResult::Consumed
            }
            // Every other box is answered and does nothing: a click on the
            // opponent's ocean while still placing ships must not fall through
            // to the board behind it, and there is no board behind it.
            Target::Own(_, _)
            | Target::Ocean(_, _)
            | Target::OwnBoard
            | Target::OceanBoard
            | Target::Title
            | Target::Phase
            | Target::Message
            | Target::OwnLabel
            | Target::OceanLabel
            | Target::Stats
            | Target::Help => EventResult::Consumed,
        }
    }
}

// ── Which grid ──────────────────────────────────────────────────────

/// One of the two grids, so the chrome around them is drawn once.
///
/// The captions, the A-J column and the 1-10 row were three near-copies of
/// each other, called twice with different origins; the caption was the only
/// part that actually differed, and it differed by a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Own,
    Ocean,
}

impl Side {
    fn origin(self, g: Grids) -> (f32, f32) {
        match self {
            Self::Own => g.own,
            Self::Ocean => g.ocean,
        }
    }

    fn caption(self) -> &'static str {
        match self {
            Self::Own => "Your Fleet",
            Self::Ocean => "Opponent's Ocean",
        }
    }

    fn label_target(self) -> Target {
        match self {
            Self::Own => Target::OwnLabel,
            Self::Ocean => Target::OceanLabel,
        }
    }
}

// ── Text ────────────────────────────────────────────────────────────

/// A size, a weight and a colour -- one run of text's whole appearance.
///
/// It is one value because it travels as one: every caption in the program
/// passed the same three fields down through four levels of drawing helper,
/// and a helper that took them separately took them in the wrong order twice.
#[derive(Debug, Clone, Copy)]
struct Ink {
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

impl Ink {
    const fn new(size: f32, weight: FontWeightHint, color: Color) -> Self {
        Self {
            size,
            weight,
            color,
        }
    }

    /// How wide `s` is when drawn in this ink.
    fn width(self, s: &str) -> f32 {
        text::measure(s, self.size, self.weight)
    }

    /// How tall one line of this ink is.
    fn height(self) -> f32 {
        text::line_height(self.size, self.weight)
    }
}

/// Draw `s` at `(x, y)` and hand back the box its glyphs occupy.
fn label(f: &mut Frame<Target>, x: f32, y: f32, s: &str, ink: Ink) -> Rect {
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color: ink.color,
        font_size: ink.size,
        font_weight: ink.weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    Rect::new(x, y, ink.width(s), ink.height())
}

/// Draw `s` left-aligned and vertically centred in `area`, elided to fit.
///
/// Elided rather than clipped because every string that goes through here is
/// variable-length -- a message, a hit rate, a line of key help -- and a
/// fragment with no mark on it reads as the whole.
fn label_in(f: &mut Frame<Target>, area: Rect, s: &str, ink: Ink) -> Rect {
    let y = area.y + (area.h - ink.height()).max(0.0) / 2.0;
    f.push(RenderCommand::Text {
        x: area.x,
        y,
        text: s.to_string(),
        color: ink.color,
        font_size: ink.size,
        font_weight: ink.weight,
        max_width: Some(area.w.max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
    Rect::new(area.x, y, ink.width(s).min(area.w.max(0.0)), ink.height())
}

// ── The window ──────────────────────────────────────────────────────

/// The one body every event goes through, whichever side it arrives from.
///
/// The window calls it and the tests call it, so a key the tests prove works
/// is the same key the window delivers.
fn handle_event(app: &mut BattleshipApp, event: &Event) -> EventResult {
    match event {
        Event::Key(KeyEvent {
            key, pressed: true, ..
        }) => app.handle_key(*key),
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }) => app.click(*x, *y, *button),
        Event::Resize { width, height } => {
            app.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// A window's `u32` size as the `f32` the layout is solved from.
fn f32_from_u32(v: u32) -> f32 {
    f32::from(u16::try_from(v).unwrap_or(u16::MAX))
}

impl App for BattleshipApp {
    fn title(&self) -> String {
        "Battleship".to_string()
    }

    fn app_id(&self) -> String {
        "battleship".to_string()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the natural size is two small positive whole numbers"
    )]
    fn initial_size(&self) -> (u32, u32) {
        // Converted from the float pair rather than written out again: two
        // spellings of one size are two things that can drift apart.
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        // Escape closes the window. It used to deal a new board -- see
        // `handle_key` -- which is the one thing a player pressing Escape
        // could least afford it to do.
        if matches!(
            event,
            Event::Key(KeyEvent {
                key: Key::Escape,
                pressed: true,
                ..
            })
        ) {
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

impl Probe for BattleshipApp {
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
    let mut game = BattleshipApp::new();
    app::launch("battleship", &mut game)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // Panicking on bad data is what a test is for; these are the lints the
    // production code above is held to and the test code below is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation
    )]

    use guitk::probe;

    use super::*;

    // ── AI fleet placement is not confined to one colour of the board ──
    //
    // The generator's own properties — bounded, deterministic, varied — are
    // tested in `randrange`, which owns it now. What is tested here is the
    // property of *this crate* that the old generator destroyed.

    /// The AI's ships must not all start on squares of one checkerboard
    /// colour.
    ///
    /// They used to. The old generator reduced with `state % max`, which on a
    /// modulus-2^64 LCG hands back the low bits, and bit 0 of such a generator
    /// alternates 0,1,0,1 with no exceptions. Placing a ship drew orientation,
    /// then row, then column from three consecutive states, so `row` and `col`
    /// always came from states of opposite parity, and `% 10` (an even
    /// modulus) preserves parity — so `row + col` was **always odd**. Verified
    /// over 1999 seeds before the fix: five ships each, and not one had an
    /// even `row + col`. Half the board could not hold the bow of a ship, and
    /// a player who spotted it could halve their search.
    ///
    /// Note what this test is *not*: a check that the placements are uniform,
    /// or that each cell is reachable. The old placement passed both — every
    /// row appeared, every column appeared, and the marginal distributions
    /// were fine. Only the *joint* distribution of row against column was
    /// degenerate, which is the shape this whole class of defect takes.
    #[test]
    fn ai_ships_do_not_all_start_on_one_checkerboard_colour() {
        let mut even = 0_u32;
        let mut odd = 0_u32;
        for seed in 1..200_u64 {
            let mut app = BattleshipApp::new();
            app.rng = SeededRng::new(seed);
            app.place_ai_ships();
            for ship in &app.opponent_fleet.ships {
                if (ship.row + ship.col) % 2 == 0 {
                    even += 1;
                } else {
                    odd += 1;
                }
            }
        }
        assert!(
            even > 0 && odd > 0,
            "ship origins are all on one colour: {even} even, {odd} odd"
        );
        // Neither colour may take more than three quarters of the placements.
        // Loose on purpose — the rejection loop skews it slightly and that is
        // legitimate; what is not legitimate is a fixed parity.
        let total = even + odd;
        assert!(
            even * 4 > total && odd * 4 > total,
            "ship origins are lopsided by colour: {even} even, {odd} odd"
        );
    }

    // ── Ship kind tests ─────────────────────────────────────────────

    #[test]
    fn test_ship_kind_sizes() {
        assert_eq!(ShipKind::Carrier.size(), 5);
        assert_eq!(ShipKind::Battleship.size(), 4);
        assert_eq!(ShipKind::Cruiser.size(), 3);
        assert_eq!(ShipKind::Submarine.size(), 3);
        assert_eq!(ShipKind::Destroyer.size(), 2);
    }

    #[test]
    fn test_ship_kind_names() {
        assert_eq!(ShipKind::Carrier.name(), "Carrier");
        assert_eq!(ShipKind::Battleship.name(), "Battleship");
        assert_eq!(ShipKind::Cruiser.name(), "Cruiser");
        assert_eq!(ShipKind::Submarine.name(), "Submarine");
        assert_eq!(ShipKind::Destroyer.name(), "Destroyer");
    }

    // ── Ship placement tests ────────────────────────────────────────

    #[test]
    fn test_ship_cells_horizontal() {
        let ship = Ship {
            kind: ShipKind::Cruiser,
            row: 2,
            col: 3,
            orientation: Orientation::Horizontal,
        };
        let cells = ship.cells();
        assert_eq!(cells, vec![(2, 3), (2, 4), (2, 5)]);
    }

    #[test]
    fn test_ship_cells_vertical() {
        let ship = Ship {
            kind: ShipKind::Cruiser,
            row: 2,
            col: 3,
            orientation: Orientation::Vertical,
        };
        let cells = ship.cells();
        assert_eq!(cells, vec![(2, 3), (3, 3), (4, 3)]);
    }

    #[test]
    fn test_ship_within_bounds_horizontal() {
        let ship = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 5,
            orientation: Orientation::Horizontal,
        };
        assert!(ship.is_within_bounds()); // 5+5 = 10, within 0..10

        let ship_oob = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 6,
            orientation: Orientation::Horizontal,
        };
        assert!(!ship_oob.is_within_bounds()); // 6+5 = 11, out of bounds
    }

    #[test]
    fn test_ship_within_bounds_vertical() {
        let ship = Ship {
            kind: ShipKind::Battleship,
            row: 6,
            col: 0,
            orientation: Orientation::Vertical,
        };
        assert!(ship.is_within_bounds()); // 6+4 = 10

        let ship_oob = Ship {
            kind: ShipKind::Battleship,
            row: 7,
            col: 0,
            orientation: Orientation::Vertical,
        };
        assert!(!ship_oob.is_within_bounds()); // 7+4 = 11
    }

    #[test]
    fn test_ship_at_origin() {
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(ship.is_within_bounds());
        assert_eq!(ship.cells(), vec![(0, 0), (0, 1)]);
    }

    #[test]
    fn test_ship_at_bottom_right_horizontal() {
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 9,
            col: 8,
            orientation: Orientation::Horizontal,
        };
        assert!(ship.is_within_bounds());
        assert_eq!(ship.cells(), vec![(9, 8), (9, 9)]);
    }

    #[test]
    fn test_ship_at_bottom_right_vertical() {
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 8,
            col: 9,
            orientation: Orientation::Vertical,
        };
        assert!(ship.is_within_bounds());
        assert_eq!(ship.cells(), vec![(8, 9), (9, 9)]);
    }

    // ── Orientation toggle ──────────────────────────────────────────

    #[test]
    fn test_orientation_toggle() {
        assert_eq!(Orientation::Horizontal.toggle(), Orientation::Vertical);
        assert_eq!(Orientation::Vertical.toggle(), Orientation::Horizontal);
    }

    // ── Fleet placement and overlap tests ───────────────────────────

    #[test]
    fn test_fleet_place_ship() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship));
        assert_eq!(fleet.ships.len(), 1);
        // Check grid cells
        for c in 0..5 {
            assert_eq!(fleet.ship_grid[0][c], Some(0));
        }
        assert_eq!(fleet.ship_grid[0][5], None);
    }

    #[test]
    fn test_fleet_overlap_detection() {
        let mut fleet = Fleet::new();
        let ship1 = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship1));

        // Try to place overlapping ship
        let ship2 = Ship {
            kind: ShipKind::Battleship,
            row: 0,
            col: 2,
            orientation: Orientation::Vertical,
        };
        assert!(!fleet.place_ship(ship2));
        assert_eq!(fleet.ships.len(), 1); // Should not have been added
    }

    #[test]
    fn test_fleet_no_overlap_adjacent() {
        let mut fleet = Fleet::new();
        let ship1 = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship1));

        // Adjacent ship (row below) should be fine.
        let ship2 = Ship {
            kind: ShipKind::Destroyer,
            row: 1,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship2));
        assert_eq!(fleet.ships.len(), 2);
    }

    #[test]
    fn test_fleet_out_of_bounds_placement() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 8,
            orientation: Orientation::Horizontal,
        };
        assert!(!fleet.place_ship(ship)); // 8 + 5 = 13, out of bounds
    }

    #[test]
    fn test_fleet_would_overlap() {
        let mut fleet = Fleet::new();
        let ship1 = Ship {
            kind: ShipKind::Cruiser,
            row: 3,
            col: 3,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship1);

        let overlapping = Ship {
            kind: ShipKind::Submarine,
            row: 3,
            col: 4,
            orientation: Orientation::Vertical,
        };
        assert!(fleet.would_overlap(&overlapping));

        let not_overlapping = Ship {
            kind: ShipKind::Submarine,
            row: 4,
            col: 3,
            orientation: Orientation::Vertical,
        };
        assert!(!fleet.would_overlap(&not_overlapping));
    }

    // ── Firing tests ────────────────────────────────────────────────

    #[test]
    fn test_fire_hit() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        let (hit, sunk) = fleet.receive_fire(0, 0);
        assert!(hit);
        assert!(sunk.is_none());
        assert_eq!(fleet.marks[0][0], CellMark::Hit);
    }

    #[test]
    fn test_fire_miss() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        let (hit, sunk) = fleet.receive_fire(5, 5);
        assert!(!hit);
        assert!(sunk.is_none());
        assert_eq!(fleet.marks[5][5], CellMark::Miss);
    }

    #[test]
    fn test_fire_sink_ship() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        let (hit1, sunk1) = fleet.receive_fire(0, 0);
        assert!(hit1);
        assert!(sunk1.is_none());

        let (hit2, sunk2) = fleet.receive_fire(0, 1);
        assert!(hit2);
        assert_eq!(sunk2, Some(ShipKind::Destroyer));
    }

    #[test]
    fn test_fire_already_fired() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        fleet.receive_fire(0, 0); // First shot
        let (hit, sunk) = fleet.receive_fire(0, 0); // Duplicate
        assert!(!hit); // Should not count again
        assert!(sunk.is_none());
    }

    #[test]
    fn test_fire_out_of_bounds() {
        let mut fleet = Fleet::new();
        let (hit, sunk) = fleet.receive_fire(10, 10);
        assert!(!hit);
        assert!(sunk.is_none());
    }

    #[test]
    fn test_already_fired_check() {
        let mut fleet = Fleet::new();
        assert!(!fleet.already_fired(5, 5));
        fleet.marks[5][5] = CellMark::Miss;
        assert!(fleet.already_fired(5, 5));
        fleet.marks[3][3] = CellMark::Hit;
        assert!(fleet.already_fired(3, 3));
    }

    #[test]
    fn test_already_fired_out_of_bounds() {
        let fleet = Fleet::new();
        assert!(fleet.already_fired(10, 10));
    }

    // ── Sinking detection ───────────────────────────────────────────

    #[test]
    fn test_all_sunk_empty_fleet() {
        let fleet = Fleet::new();
        assert!(!fleet.all_sunk()); // No ships = not all sunk
    }

    #[test]
    fn test_all_sunk_one_ship() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);
        assert!(!fleet.all_sunk());

        fleet.receive_fire(0, 0);
        assert!(!fleet.all_sunk());

        fleet.receive_fire(0, 1);
        assert!(fleet.all_sunk());
    }

    #[test]
    fn test_all_sunk_multiple_ships() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        fleet.place_ship(Ship {
            kind: ShipKind::Submarine,
            row: 2,
            col: 0,
            orientation: Orientation::Horizontal,
        });

        // Sink destroyer
        fleet.receive_fire(0, 0);
        fleet.receive_fire(0, 1);
        assert!(!fleet.all_sunk());

        // Sink submarine
        fleet.receive_fire(2, 0);
        fleet.receive_fire(2, 1);
        fleet.receive_fire(2, 2);
        assert!(fleet.all_sunk());
    }

    #[test]
    fn test_ships_remaining() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        fleet.place_ship(Ship {
            kind: ShipKind::Cruiser,
            row: 2,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        assert_eq!(fleet.ships_remaining(), 2);

        fleet.receive_fire(0, 0);
        fleet.receive_fire(0, 1);
        assert_eq!(fleet.ships_remaining(), 1);
    }

    #[test]
    fn test_is_ship_sunk() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        assert!(!fleet.is_ship_sunk(0));
        fleet.receive_fire(0, 0);
        assert!(!fleet.is_ship_sunk(0));
        fleet.receive_fire(0, 1);
        assert!(fleet.is_ship_sunk(0));
    }

    #[test]
    fn test_is_cell_sunk() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        assert!(!fleet.is_cell_sunk(0, 0));
        fleet.receive_fire(0, 0);
        fleet.receive_fire(0, 1);
        assert!(fleet.is_cell_sunk(0, 0));
        assert!(fleet.is_cell_sunk(0, 1));
        assert!(!fleet.is_cell_sunk(1, 0)); // No ship here
    }

    // ── AI behavior tests ───────────────────────────────────────────

    #[test]
    fn test_ai_starts_in_hunt_mode() {
        let ai = AiState::new();
        assert_eq!(ai.mode, AiMode::Hunt);
        assert_eq!(ai.shots, 0);
        assert_eq!(ai.hits, 0);
    }

    #[test]
    fn test_ai_switches_to_target_on_hit() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, true);
        assert!(matches!(ai.mode, AiMode::Target { .. }));
        assert_eq!(ai.shots, 1);
        assert_eq!(ai.hits, 1);
    }

    #[test]
    fn test_ai_stays_hunt_on_miss() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, false);
        assert_eq!(ai.mode, AiMode::Hunt);
        assert_eq!(ai.shots, 1);
        assert_eq!(ai.hits, 0);
    }

    #[test]
    fn test_ai_target_has_adjacent_cells() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, true);
        if let AiMode::Target { targets } = &ai.mode {
            // Should have up to 4 adjacent cells
            assert!(!targets.is_empty());
            assert!(targets.contains(&(4, 5))); // Up
            assert!(targets.contains(&(6, 5))); // Down
            assert!(targets.contains(&(5, 4))); // Left
            assert!(targets.contains(&(5, 6))); // Right
        } else {
            panic!("AI should be in Target mode");
        }
    }

    #[test]
    fn test_ai_target_edge_cell() {
        let mut ai = AiState::new();
        ai.record_shot(0, 0, true);
        if let AiMode::Target { targets } = &ai.mode {
            // Corner hit: only 2 adjacent cells
            assert_eq!(targets.len(), 2);
            assert!(targets.contains(&(1, 0)));
            assert!(targets.contains(&(0, 1)));
        } else {
            panic!("AI should be in Target mode");
        }
    }

    #[test]
    fn test_ai_choose_target_hunt() {
        let mut ai = AiState::new();
        let mut rng = SeededRng::new(99);
        let (r, c) = ai.choose_target(&mut rng);
        assert!(r < GRID_SIZE);
        assert!(c < GRID_SIZE);
    }

    #[test]
    fn test_ai_choose_target_from_targets() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, true);
        let mut rng = SeededRng::new(99);
        let (r, c) = ai.choose_target(&mut rng);
        // Should pick from adjacents of (5,5)
        let expected = [(4, 5), (6, 5), (5, 4), (5, 6)];
        assert!(expected.contains(&(r, c)));
    }

    #[test]
    fn test_ai_returns_to_hunt_when_targets_exhausted() {
        let mut ai = AiState::new();
        ai.record_shot(0, 0, true);
        // Fire at all target cells as misses.
        ai.record_shot(1, 0, false);
        ai.record_shot(0, 1, false);
        assert_eq!(ai.mode, AiMode::Hunt);
    }

    #[test]
    fn test_ai_records_fired_cells() {
        let mut ai = AiState::new();
        assert!(!ai.fired[3][7]);
        ai.record_shot(3, 7, false);
        assert!(ai.fired[3][7]);
    }

    #[test]
    fn test_ai_pick_random_avoids_fired() {
        let mut ai = AiState::new();
        let mut rng = SeededRng::new(42);
        // Fire at most cells
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if !(r == 9 && c == 9) {
                    ai.fired[r][c] = true;
                }
            }
        }
        let (r, c) = ai.pick_random(&mut rng);
        assert_eq!((r, c), (9, 9));
    }

    // ── Game phase tests ────────────────────────────────────────────

    #[test]
    fn test_new_game_starts_in_placement() {
        let app = BattleshipApp::new();
        assert_eq!(app.phase, GamePhase::Placement);
    }

    #[test]
    fn test_new_game_has_ai_ships() {
        let app = BattleshipApp::new();
        assert_eq!(app.opponent_fleet.ships.len(), 5);
    }

    #[test]
    fn test_new_game_player_fleet_empty() {
        let app = BattleshipApp::new();
        assert!(app.player_fleet.ships.is_empty());
    }

    #[test]
    fn test_new_game_placement_index() {
        let app = BattleshipApp::new();
        assert_eq!(app.placement_index, 0);
    }

    #[test]
    fn test_current_placement_ship() {
        let app = BattleshipApp::new();
        assert_eq!(app.current_placement_ship(), Some(ShipKind::Carrier));
    }

    #[test]
    fn test_placement_phase_place_all_ships() {
        let mut app = BattleshipApp::new();
        // Place all 5 ships at non-overlapping positions.
        // Carrier(5) at row 0
        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 1);

        // Battleship(4) at row 1
        app.placement_row = 1;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 2);

        // Cruiser(3) at row 2
        app.placement_row = 2;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 3);

        // Submarine(3) at row 3
        app.placement_row = 3;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 4);

        // Destroyer(2) at row 4
        app.placement_row = 4;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);

        // Should now be in Firing phase.
        assert_eq!(app.phase, GamePhase::Firing);
        assert_eq!(app.player_fleet.ships.len(), 5);
    }

    #[test]
    fn test_placement_rotate() {
        let mut app = BattleshipApp::new();
        assert_eq!(app.placement_orientation, Orientation::Horizontal);
        app.handle_key(Key::R);
        assert_eq!(app.placement_orientation, Orientation::Vertical);
        app.handle_key(Key::R);
        assert_eq!(app.placement_orientation, Orientation::Horizontal);
    }

    #[test]
    fn test_placement_arrow_keys() {
        let mut app = BattleshipApp::new();
        assert_eq!(app.placement_row, 0);
        assert_eq!(app.placement_col, 0);

        app.handle_key(Key::Down);
        assert_eq!(app.placement_row, 1);

        app.handle_key(Key::Right);
        assert_eq!(app.placement_col, 1);

        app.handle_key(Key::Up);
        assert_eq!(app.placement_row, 0);

        app.handle_key(Key::Left);
        assert_eq!(app.placement_col, 0);
    }

    #[test]
    fn test_placement_cursor_stays_in_bounds() {
        let mut app = BattleshipApp::new();
        // Try to go above row 0
        app.handle_key(Key::Up);
        assert_eq!(app.placement_row, 0);

        // Try to go left of col 0
        app.handle_key(Key::Left);
        assert_eq!(app.placement_col, 0);
    }

    #[test]
    fn test_placement_invalid_overlap() {
        let mut app = BattleshipApp::new();
        // Place carrier at (0,0) horizontal
        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);

        // Try to place battleship overlapping
        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 1); // Should not advance
        assert!(app.message.contains("Invalid"));
    }

    #[test]
    fn test_placement_clamp_after_rotate() {
        let mut app = BattleshipApp::new();
        // Move to bottom edge and rotate vertical
        app.placement_row = 9;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::R); // Rotate to vertical
        // Carrier(5) vertical at row 9 would go to row 13; should clamp.
        assert!(app.placement_row + ShipKind::Carrier.size() <= GRID_SIZE);
    }

    // ── Firing phase tests ──────────────────────────────────────────

    fn setup_firing_app() -> BattleshipApp {
        let mut app = BattleshipApp::new();
        // Place all player ships quickly in non-overlapping rows.
        let positions = [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)];
        for (i, &(row, col)) in positions.iter().enumerate() {
            app.placement_row = row;
            app.placement_col = col;
            app.placement_orientation = Orientation::Horizontal;
            if i < FLEET.len() {
                app.handle_key(Key::Enter);
            }
        }
        assert_eq!(app.phase, GamePhase::Firing);
        app
    }

    #[test]
    fn test_firing_cursor_movement() {
        let mut app = setup_firing_app();
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);

        app.handle_key(Key::Down);
        assert_eq!(app.cursor_row, 1);

        app.handle_key(Key::Right);
        assert_eq!(app.cursor_col, 1);
    }

    #[test]
    fn test_firing_cursor_bounds() {
        let mut app = setup_firing_app();
        app.handle_key(Key::Up);
        assert_eq!(app.cursor_row, 0); // Should not go below 0

        app.handle_key(Key::Left);
        assert_eq!(app.cursor_col, 0);

        // Move to bottom-right
        for _ in 0..20 {
            app.handle_key(Key::Down);
            app.handle_key(Key::Right);
        }
        assert_eq!(app.cursor_row, 9);
        assert_eq!(app.cursor_col, 9);
    }

    #[test]
    fn test_fire_and_track_stats() {
        let mut app = setup_firing_app();
        assert_eq!(app.player_shots, 0);
        assert_eq!(app.player_hits, 0);

        // Fire at (0,0) on opponent's grid — might be hit or miss.
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(Key::Enter);

        assert_eq!(app.player_shots, 1);
    }

    #[test]
    fn test_fire_duplicate_rejected() {
        let mut app = setup_firing_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(Key::Enter);
        let shots_after_first = app.player_shots;

        // Try to fire at same spot.
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(Key::Enter);
        assert_eq!(app.player_shots, shots_after_first);
        assert!(app.message.contains("Already"));
    }

    #[test]
    fn test_fire_space_key() {
        let mut app = setup_firing_app();
        app.cursor_row = 5;
        app.cursor_col = 5;
        app.handle_key(Key::Space);
        assert_eq!(app.player_shots, 1);
    }

    #[test]
    fn test_player_hit_rate_zero() {
        let app = BattleshipApp::new();
        assert!((app.player_hit_rate() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_player_hit_rate_calculation() {
        let mut app = setup_firing_app();
        app.player_shots = 10;
        app.player_hits = 3;
        let rate = app.player_hit_rate();
        assert!((rate - 30.0).abs() < 0.1);
    }

    // ── Win/loss detection tests ────────────────────────────────────

    #[test]
    fn test_win_detection() {
        let mut app = setup_firing_app();
        // Manually sink all opponent ships.
        for ship in &app.opponent_fleet.ships.clone() {
            for (r, c) in ship.cells() {
                app.opponent_fleet.receive_fire(r, c);
            }
        }
        assert!(app.opponent_fleet.all_sunk());
    }

    #[test]
    fn test_loss_detection() {
        let mut app = setup_firing_app();
        // Manually sink all player ships.
        for ship in &app.player_fleet.ships.clone() {
            for (r, c) in ship.cells() {
                app.player_fleet.receive_fire(r, c);
            }
        }
        assert!(app.player_fleet.all_sunk());
    }

    // ── New game tests ──────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_state() {
        let mut app = setup_firing_app();
        app.player_shots = 15;
        app.player_hits = 5;
        app.handle_key(Key::N);
        assert_eq!(app.phase, GamePhase::Placement);
        assert_eq!(app.player_shots, 0);
        assert_eq!(app.player_hits, 0);
        assert!(app.player_fleet.ships.is_empty());
        assert_eq!(app.opponent_fleet.ships.len(), 5);
    }

    #[test]
    fn test_new_game_from_game_over() {
        let mut app = setup_firing_app();
        app.phase = GamePhase::GameOver;
        app.handle_key(Key::N);
        assert_eq!(app.phase, GamePhase::Placement);
    }

    // ── AI placement tests ──────────────────────────────────────────

    #[test]
    fn test_ai_ships_no_overlap() {
        let app = BattleshipApp::new();
        // Check that no two AI ships occupy the same cell.
        let mut occupied = [[false; GRID_SIZE]; GRID_SIZE];
        for ship in &app.opponent_fleet.ships {
            for (r, c) in ship.cells() {
                assert!(!occupied[r][c], "AI ships overlap at ({r}, {c})");
                occupied[r][c] = true;
            }
        }
    }

    #[test]
    fn test_ai_ships_within_bounds() {
        let app = BattleshipApp::new();
        for ship in &app.opponent_fleet.ships {
            assert!(ship.is_within_bounds(), "AI ship out of bounds: {ship:?}");
        }
    }

    #[test]
    fn test_ai_places_correct_number_of_ships() {
        let app = BattleshipApp::new();
        assert_eq!(app.opponent_fleet.ships.len(), 5);
    }

    #[test]
    fn test_ai_ships_correct_sizes() {
        let app = BattleshipApp::new();
        let mut sizes: Vec<usize> = app
            .opponent_fleet
            .ships
            .iter()
            .map(|s| s.kind.size())
            .collect();
        sizes.sort_unstable();
        assert_eq!(sizes, vec![2, 3, 3, 4, 5]);
    }

    #[test]
    fn test_ai_placement_with_different_seeds() {
        // Different seeds should produce different layouts.
        let mut app1 = BattleshipApp::new();
        app1.rng = SeededRng::new(111);
        app1.place_ai_ships();

        let mut app2 = BattleshipApp::new();
        app2.rng = SeededRng::new(999);
        app2.place_ai_ships();

        // It's possible (but unlikely) for two seeds to produce the same layout.
        // Check that at least one ship differs.
        let differ = app1
            .opponent_fleet
            .ships
            .iter()
            .zip(app2.opponent_fleet.ships.iter())
            .any(|(a, b)| a.row != b.row || a.col != b.col || a.orientation != b.orientation);
        assert!(differ, "Different seeds should produce different layouts");
    }

    // ── Integration / gameplay tests ────────────────────────────────

    #[test]
    fn test_full_game_sink_all_opponent_ships() {
        let mut app = setup_firing_app();
        // Fire at every opponent ship cell.
        let opponent_ships = app.opponent_fleet.ships.clone();
        for ship in &opponent_ships {
            for (r, c) in ship.cells() {
                if !app.opponent_fleet.already_fired(r, c) && app.phase == GamePhase::Firing {
                    app.cursor_row = r;
                    app.cursor_col = c;
                    app.handle_key(Key::Enter);
                }
            }
        }
        // Either the player won, or the AI won first (unlikely but possible).
        assert!(
            app.phase == GamePhase::GameOver,
            "Game should be over after sinking all ships"
        );
    }

    #[test]
    fn test_ai_fires_after_player() {
        let mut app = setup_firing_app();
        let ai_shots_before = app.ai_state.shots;
        // Fire at an empty cell.
        let mut target = (0, 0);
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.opponent_fleet.ship_grid[r][c].is_none() {
                    target = (r, c);
                }
            }
        }
        app.cursor_row = target.0;
        app.cursor_col = target.1;
        app.handle_key(Key::Enter);
        assert_eq!(app.ai_state.shots, ai_shots_before + 1);
    }

    #[test]
    fn test_placement_message_updates() {
        let mut app = BattleshipApp::new();
        assert!(app.message.contains("Carrier"));

        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert!(app.message.contains("Battleship"));

        app.placement_row = 1;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert!(app.message.contains("Cruiser"));
    }

    #[test]
    fn test_sink_message_appears() {
        let mut app = setup_firing_app();
        // Find a destroyer (size 2) in opponent's fleet and sink it.
        let destroyer_idx = app
            .opponent_fleet
            .ships
            .iter()
            .position(|s| s.kind == ShipKind::Destroyer);
        if let Some(idx) = destroyer_idx {
            let cells = app.opponent_fleet.ships[idx].cells();
            for (r, c) in cells {
                if !app.opponent_fleet.already_fired(r, c) && app.phase == GamePhase::Firing {
                    app.cursor_row = r;
                    app.cursor_col = c;
                    app.handle_key(Key::Enter);
                }
            }
            // The message should mention sinking at some point during the sequence.
            // After the last hit that sinks the ship, we check the message
            // (it might be overwritten by AI response, but last_sunk should have been set).
        }
    }

    #[test]
    fn test_game_over_no_firing() {
        let mut app = setup_firing_app();
        app.phase = GamePhase::GameOver;
        let shots = app.player_shots;
        app.cursor_row = 5;
        app.cursor_col = 5;
        app.handle_key(Key::Enter); // Should be ignored.
        assert_eq!(app.player_shots, shots);
    }

    #[test]
    fn test_placement_phase_ignores_fire_keys() {
        let mut app = BattleshipApp::new();
        // Space should not do anything during placement.
        app.handle_key(Key::Space);
        assert_eq!(app.phase, GamePhase::Placement);
    }

    #[test]
    fn test_fleet_place_all_five_ships() {
        let mut fleet = Fleet::new();
        let ships = [
            Ship {
                kind: ShipKind::Carrier,
                row: 0,
                col: 0,
                orientation: Orientation::Horizontal,
            },
            Ship {
                kind: ShipKind::Battleship,
                row: 1,
                col: 0,
                orientation: Orientation::Horizontal,
            },
            Ship {
                kind: ShipKind::Cruiser,
                row: 2,
                col: 0,
                orientation: Orientation::Horizontal,
            },
            Ship {
                kind: ShipKind::Submarine,
                row: 3,
                col: 0,
                orientation: Orientation::Horizontal,
            },
            Ship {
                kind: ShipKind::Destroyer,
                row: 4,
                col: 0,
                orientation: Orientation::Horizontal,
            },
        ];
        for ship in &ships {
            assert!(fleet.place_ship(*ship));
        }
        assert_eq!(fleet.ships.len(), 5);
        assert_eq!(fleet.ships_remaining(), 5);
    }

    #[test]
    fn test_cross_overlap_horizontal_vertical() {
        let mut fleet = Fleet::new();
        let h_ship = Ship {
            kind: ShipKind::Carrier,
            row: 5,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(h_ship));

        // Place vertical ship crossing through it.
        let v_ship = Ship {
            kind: ShipKind::Cruiser,
            row: 3,
            col: 2,
            orientation: Orientation::Vertical,
        };
        // Row 3,4,5 col 2 — row 5, col 2 is occupied by carrier.
        assert!(!fleet.place_ship(v_ship));
    }

    // ── The window ──────────────────────────────────────────────────
    //
    // Everything below drives the program the way the window does: draw at a
    // size, ask the frame what was drawn where, click there. Not one of these
    // tests could exist before this app had a window -- `main` was
    // `let _app = BattleshipApp::new();`, so the drawing ran nowhere and the
    // only thing a test could reach was the state behind it.

    /// The size almost every claim below is checked at.
    const SIZE: (f32, f32) = BattleshipApp::SIZE;

    /// The window sizes the layout claims are checked at.
    ///
    /// A tall thin window and a short wide one are the two shapes that break a
    /// layout written for one aspect ratio, and the 200x150 one is smaller
    /// than the drawing's old fixed extent in both directions.
    const SIZES: [(f32, f32); 6] = [
        (1000.0, 720.0),
        (1400.0, 900.0),
        (700.0, 520.0),
        (420.0, 900.0),
        (1200.0, 300.0),
        (200.0, 150.0),
    ];

    /// A game with all five of the player's ships already placed.
    fn firing_app() -> BattleshipApp {
        setup_firing_app()
    }

    /// The box the frame recorded for `target`, or a panic naming it.
    fn box_of(app: &BattleshipApp, target: Target) -> Rect {
        probe::rect_of(app, target).unwrap_or_else(|| panic!("nothing was drawn for {target:?}"))
    }

    // ── Layout ──────────────────────────────────────────────────────

    #[test]
    fn the_bands_run_down_the_window_in_order_and_do_not_overlap() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            let bands = [
                ("header", l.header),
                ("message", l.message),
                ("body", l.body),
                ("stats", l.stats),
                ("help", l.help),
            ];
            let mut y = 0.0_f32;
            for (name, band) in bands {
                assert!(
                    band.y >= y - 0.001,
                    "{name} starts at {} above {y} in a {w}x{h} window",
                    band.y
                );
                assert!(band.h >= 0.0, "{name} has a negative height at {w}x{h}");
                y = band.bottom();
            }
            assert!(
                y <= h + 0.001,
                "the bands run {y} down a window only {h} tall"
            );
        }
    }

    #[test]
    fn the_bands_fill_the_window_and_span_its_width() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert_eq!(l.window, Rect::new(0.0, 0.0, w, h));
            for band in [l.header, l.message, l.body, l.stats, l.help] {
                assert_eq!(band.x, 0.0, "a band is inset at {w}x{h}");
                assert_eq!(band.w, w, "a band is not the window's width at {w}x{h}");
            }
            assert!(
                (l.help.bottom() - h).abs() < 0.001 || l.body.h == 0.0,
                "the last band ends at {} in a window {h} tall",
                l.help.bottom()
            );
        }
    }

    #[test]
    fn the_bands_are_shares_of_the_height_not_the_width() {
        // A band measured against the width grows when the window is widened,
        // which is how a header comes to eat the board on a wide, short
        // screen. The bands still tile the window when this is wrong, so the
        // test that they tile it cannot see the mistake.
        let narrow = Layout::solve(400.0, 720.0);
        let wide = Layout::solve(1600.0, 720.0);
        for (name, a, b) in [
            ("header", narrow.header.h, wide.header.h),
            ("message", narrow.message.h, wide.message.h),
            ("body", narrow.body.h, wide.body.h),
            ("stats", narrow.stats.h, wide.stats.h),
            ("help", narrow.help.h, wide.help.h),
        ] {
            assert!(
                (a - b).abs() < 0.001,
                "the {name} band is {a} tall in a 400-wide window and {b} in a 1600-wide one"
            );
        }
    }

    #[test]
    fn the_padding_never_vanishes_never_runs_away_and_never_outgrows_the_window() {
        for (w, h) in [
            (0.0, 0.0),
            (1.0, 400.0),
            (400.0, 1.0),
            (3.0, 3.0),
            (200.0, 150.0),
        ] {
            let l = Layout::solve(w, h);
            assert!(l.pad >= 0.0, "a {w}x{h} window was padded by {}", l.pad);
            assert!(
                l.pad <= w.min(h) / 2.0 + 0.001,
                "a {w}x{h} window was padded by {}, wider than half of it",
                l.pad
            );
        }

        // The floor and the ceiling the cap above is measured against. A bare
        // 2% margin is a fifth of a pixel on a small window and thirty on a
        // 4K one -- and without the floor the cap is unreachable, since 2% of
        // a length is always less than half of it.
        assert!(
            (Layout::solve(60.0, 60.0).pad - 2.0).abs() < 0.001,
            "a small window's margin thinned away to {}",
            Layout::solve(60.0, 60.0).pad
        );
        assert!(
            (Layout::solve(4000.0, 3000.0).pad - 24.0).abs() < 0.001,
            "a 4K window's margin ran away to {}",
            Layout::solve(4000.0, 3000.0).pad
        );
    }

    #[test]
    fn a_bigger_window_gets_bigger_cells() {
        // The point of solving the layout each frame rather than reading it
        // off a `const CELL_SIZE: f32 = 36.0`: the same board at two sizes is
        // two different pictures, and the bigger window gets the bigger one.
        let small = Layout::solve(700.0, 520.0).grids();
        let large = Layout::solve(1400.0, 1040.0).grids();
        assert!(
            large.step > small.step * 1.5,
            "doubling the window barely moved the cell: {} -> {}",
            small.step,
            large.step
        );
        assert!(large.cell < large.step, "the gap between cells closed up");
        assert!(large.label > small.label, "the labels did not grow with it");
    }

    #[test]
    fn the_cells_are_square_and_the_two_grids_are_the_same_size() {
        for (w, h) in SIZES {
            let g = Layout::solve(w, h).grids();
            let own = g.board_rect(g.own);
            let ocean = g.board_rect(g.ocean);
            assert!(
                (own.w - own.h).abs() < 0.001,
                "the player's grid is {}x{} at {w}x{h}",
                own.w,
                own.h
            );
            assert_eq!(
                (own.w, own.h),
                (ocean.w, ocean.h),
                "the two grids differ in size at {w}x{h}"
            );
            for r in 0..GRID_SIZE {
                for c in 0..GRID_SIZE {
                    let cell = g.cell_rect(g.own, r, c);
                    assert!(
                        (cell.w - cell.h).abs() < 0.001,
                        "cell {r},{c} is {}x{} at {w}x{h}",
                        cell.w,
                        cell.h
                    );
                }
            }
        }
    }

    #[test]
    fn the_two_grids_never_overlap_and_both_stay_inside_the_body() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            let g = l.grids();
            let own = g.board_rect(g.own);
            let ocean = g.board_rect(g.ocean);
            assert!(
                own.right() <= ocean.x + 0.001,
                "the grids overlap at {w}x{h}: {own:?} and {ocean:?}"
            );
            for (name, board) in [("own", own), ("ocean", ocean)] {
                assert!(
                    board.x >= l.body.x - 0.001
                        && board.right() <= l.body.right() + 0.001
                        && board.y >= l.body.y - 0.001
                        && board.bottom() <= l.body.bottom() + 0.001,
                    "the {name} grid {board:?} leaves the body {:?} at {w}x{h}",
                    l.body
                );
            }
        }
    }

    #[test]
    fn the_row_and_column_labels_have_room_left_of_and_above_the_cells() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            let g = l.grids();
            // The A-J column has to start inside the body, not off its left
            // edge: the old drawing reserved `LABEL_OFFSET` from a fixed grid
            // origin, which is only inside the window at one window size.
            assert!(
                g.own.0 - g.label >= l.body.x - 0.001,
                "the row labels start at {} outside the body at {w}x{h}",
                g.own.0 - g.label
            );
            assert!(
                g.caption_y >= l.body.y - 0.001 && g.caption_y < g.own.1,
                "the caption at {} is not above the cells at {}",
                g.caption_y,
                g.own.1
            );
            // A whole caption line of clearance, not merely "not above the
            // caption". Asserted the loose way, a layout reserving *no* room
            // for the caption satisfies it exactly -- the caption line and the
            // label line are the same height, so dropping the caption from the
            // grid's top leaves the two sides of the inequality equal.
            assert!(
                g.own.1 - g.label >= g.caption_y + g.label - 0.001,
                "the 1-10 row at {} leaves no line for the caption at {} at {w}x{h}",
                g.own.1 - g.label,
                g.caption_y
            );
        }
    }

    #[test]
    fn nothing_is_drawn_outside_the_window() {
        for (w, h) in SIZES {
            let app = firing_app();
            let f = app.draw((w, h));
            for (target, rect) in f.hits() {
                assert!(
                    rect.x >= -0.001
                        && rect.y >= -0.001
                        && rect.right() <= w + 0.001
                        && rect.bottom() <= h + 0.001,
                    "{target:?} was drawn at {rect:?} outside a {w}x{h} window"
                );
            }
            // A hit box is recorded whether or not it would survive the clip,
            // so the boxes above cannot see a missing clip at all -- only the
            // commands can.
            let outer = f.commands().iter().find_map(|c| match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => Some((*x, *y, *width, *height)),
                _ => None,
            });
            assert_eq!(
                outer,
                Some((0.0, 0.0, w, h)),
                "a {w}x{h} window was not clipped to itself"
            );
            assert!(f.is_balanced(), "a clip was pushed and never popped");
        }
    }

    #[test]
    fn a_window_with_no_room_at_all_still_draws_a_frame() {
        // Not "looks right" -- there is no right at this size. Only that the
        // arithmetic stays finite and nothing panics, which is what a window
        // dragged shut actually does to a layout.
        for (w, h) in [(0.0, 0.0), (1.0, 400.0), (400.0, 1.0), (-50.0, -50.0)] {
            let app = firing_app();
            let f = app.draw((w, h));
            for (target, rect) in f.hits() {
                assert!(
                    rect.x.is_finite()
                        && rect.y.is_finite()
                        && rect.w.is_finite()
                        && rect.h.is_finite(),
                    "{target:?} came out as {rect:?} in a {w}x{h} window"
                );
                assert!(rect.w >= 0.0 && rect.h >= 0.0, "{target:?} is inside out");
            }
        }
    }

    #[test]
    fn each_grid_s_caption_is_centred_over_the_grid_it_names() {
        // The old drawing subtracted a hand-measured half-width -- 40.0 for
        // "Your Fleet" and 55.0 for "Opponent's Ocean" -- so the two captions
        // were centred to two different standards, and neither one to the
        // width the text actually takes.
        for (w, h) in SIZES {
            let app = firing_app();
            let g = Layout::solve(w, h).grids();
            for (target, origin) in [(Target::OwnLabel, g.own), (Target::OceanLabel, g.ocean)] {
                let caption = probe::rect_of_sized(&app, target, (w, h))
                    .unwrap_or_else(|| panic!("no caption for {target:?} at {w}x{h}"));
                let board = g.board_rect(origin);
                assert!(
                    (caption.centre().0 - board.centre().0).abs() <= 1.0,
                    "{target:?} sits at {} over a grid centred on {} at {w}x{h}",
                    caption.centre().0,
                    board.centre().0
                );
            }
        }
    }

    // ── What a click lands on ───────────────────────────────────────

    #[test]
    fn every_cell_of_both_grids_records_a_box_a_click_can_find() {
        let app = firing_app();
        let f = app.draw(SIZE);
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                for target in [
                    Target::Own(byte(r), byte(c)),
                    Target::Ocean(byte(r), byte(c)),
                ] {
                    let rect = box_of(&app, target);
                    let (x, y) = rect.centre();
                    assert_eq!(
                        f.hit_test(x, y),
                        Some(target),
                        "a click in the middle of {target:?} lands somewhere else"
                    );
                }
            }
        }
    }

    #[test]
    fn the_cells_are_drawn_where_the_grid_says_they_are() {
        // The box a click is tested against and the square the player sees are
        // the same rectangle, which is the entire claim the hit map makes.
        let app = firing_app();
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        for (origin, cell_of) in [
            (
                g.own,
                (|r, c| Target::Own(byte(r), byte(c))) as fn(usize, usize) -> Target,
            ),
            (
                g.ocean,
                (|r, c| Target::Ocean(byte(r), byte(c))) as fn(usize, usize) -> Target,
            ),
        ] {
            for r in 0..GRID_SIZE {
                for c in 0..GRID_SIZE {
                    let drawn = box_of(&app, cell_of(r, c));
                    // Worked out here rather than asked of `cell_rect`: a test
                    // whose expectation comes from the function it is testing
                    // agrees with that function however wrong it is. Swapping
                    // the row and the column inside `cell_rect` -- which on a
                    // square board transposes every cell -- passed cleanly
                    // until this was written out.
                    let want = Rect::new(
                        origin.0 + f32_from_usize(c) * g.step,
                        origin.1 + f32_from_usize(r) * g.step,
                        g.cell,
                        g.cell,
                    );
                    // Within a pixel rather than to the bit: the recorded box
                    // is the drawn one intersected with the window's clip, and
                    // that intersection is two more float operations on the
                    // same numbers.
                    for (axis, a, b) in [
                        ("x", drawn.x, want.x),
                        ("y", drawn.y, want.y),
                        ("w", drawn.w, want.w),
                        ("h", drawn.h, want.h),
                    ] {
                        assert!(
                            (a - b).abs() < 0.01,
                            "cell {r},{c} has {axis} {a} where the grid puts {b}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_board_behind_the_cells_is_reachable_where_the_cells_are_not() {
        // The gap between two cells is not dead space: it answers as the board
        // it belongs to, so a click that falls a pixel short of a square is
        // still a click on that grid and not on nothing at all.
        let app = firing_app();
        let f = app.draw(SIZE);
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        for (origin, board) in [(g.own, Target::OwnBoard), (g.ocean, Target::OceanBoard)] {
            // The mat has to reach past the last cell it is a mat for. It is
            // measured in whole steps, gap included; measured in drawn squares
            // instead it stops six percent of a board short, and the far corner
            // becomes a strip of grid answering as nothing at all.
            let mat = g.board_rect(origin);
            let last = g.cell_rect(origin, LAST_CELL, LAST_CELL);
            assert!(
                mat.right() >= last.right() - 0.001 && mat.bottom() >= last.bottom() - 0.001,
                "{board:?} ends at {},{} short of its last cell at {},{}",
                mat.right(),
                mat.bottom(),
                last.right(),
                last.bottom()
            );

            let first = g.cell_rect(origin, 0, 0);
            let x = first.right() + (g.step - g.cell) / 2.0;
            let y = first.centre().1;
            assert_eq!(
                f.hit_test(x, y),
                Some(board),
                "the gap right of the first cell answers as nothing"
            );
            // Answered, and doing nothing: a click that misses a square by a
            // pixel must not fire the cell next to it.
            let mut clicked = firing_app();
            assert_eq!(
                clicked.click_at(x, y, MouseButton::Left, SIZE),
                EventResult::Consumed
            );
            assert_eq!(
                clicked.player_shots, 0,
                "a click in the gutter of {board:?} fired a shell"
            );
        }
    }

    #[test]
    fn a_click_on_your_own_grid_places_the_ship_being_placed() {
        let mut app = BattleshipApp::with_seed(7);
        assert_eq!(app.phase, GamePhase::Placement);
        assert_eq!(app.current_placement_ship(), Some(ShipKind::Carrier));

        assert_eq!(
            probe::click(&mut app, Target::Own(3, 2)),
            EventResult::Consumed
        );
        let carrier = app
            .player_fleet
            .ships
            .first()
            .copied()
            .expect("the click placed no ship");
        assert_eq!(carrier.kind, ShipKind::Carrier);
        assert_eq!(
            (carrier.row, carrier.col),
            (3, 2),
            "the ship went somewhere other than the cell that was clicked"
        );
        assert_eq!(
            app.current_placement_ship(),
            Some(ShipKind::Battleship),
            "the placement did not move on to the next ship"
        );
    }

    #[test]
    fn a_click_that_would_hang_a_ship_off_the_board_pulls_it_back_on() {
        // A cursor walked with arrow keys cannot leave the board; a click can
        // put it anywhere at all, which is exactly the case the clamp used to
        // be called for on Down and Right and not on Up and Left.
        let mut app = BattleshipApp::with_seed(7);
        probe::click(&mut app, Target::Own(0, byte(LAST_CELL)));
        let carrier = app
            .player_fleet
            .ships
            .first()
            .copied()
            .expect("the click placed no ship");
        assert_eq!(
            (carrier.row, carrier.col),
            (0, GRID_SIZE - ShipKind::Carrier.size()),
            "the five-cell Carrier was placed with its bow off the board"
        );
        for (r, c) in carrier.cells() {
            assert!(
                r < GRID_SIZE && c < GRID_SIZE,
                "cell {r},{c} is off the board"
            );
        }
    }

    #[test]
    fn a_click_on_a_cell_another_ship_already_holds_is_refused() {
        let mut app = BattleshipApp::with_seed(7);
        probe::click(&mut app, Target::Own(0, 0));
        assert_eq!(app.player_fleet.ships.len(), 1);
        probe::click(&mut app, Target::Own(0, 0));
        assert_eq!(
            app.player_fleet.ships.len(),
            1,
            "a second ship was stacked on the first"
        );
        assert_eq!(
            app.current_placement_ship(),
            Some(ShipKind::Battleship),
            "the refused placement moved on to the next ship anyway"
        );
        assert!(
            app.message.contains("Invalid"),
            "the refusal was silent: {:?}",
            app.message
        );
    }

    #[test]
    fn a_click_on_the_ocean_fires_at_that_cell() {
        let mut app = firing_app();
        let before = app.player_shots;
        assert_eq!(
            probe::click(&mut app, Target::Ocean(4, 6)),
            EventResult::Consumed
        );
        assert_eq!(app.player_shots, before + 1, "the click fired nothing");
        assert_eq!(
            (app.cursor_row, app.cursor_col),
            (4, 6),
            "the cursor did not move to the cell that was clicked"
        );
        assert_ne!(
            app.opponent_fleet.mark_at(4, 6),
            CellMark::Empty,
            "the shell landed on some other cell"
        );
    }

    #[test]
    fn a_click_on_a_cell_already_fired_at_costs_nothing() {
        let mut app = firing_app();
        probe::click(&mut app, Target::Ocean(4, 6));
        let shots = app.player_shots;
        let ai_shots = app.ai_state.shots;
        probe::click(&mut app, Target::Ocean(4, 6));
        assert_eq!(app.player_shots, shots, "the same cell was fired at twice");
        assert_eq!(
            app.ai_state.shots, ai_shots,
            "the AI took a turn for a shot that was never taken"
        );
        assert!(
            app.message.contains("Already fired"),
            "the refusal was silent: {:?}",
            app.message
        );
    }

    #[test]
    fn neither_grid_answers_a_click_in_the_phase_it_is_not_for() {
        let mut placing = BattleshipApp::with_seed(7);
        let before = placing.opponent_fleet.mark_at(0, 0);
        probe::click(&mut placing, Target::Ocean(0, 0));
        assert_eq!(
            placing.player_shots, 0,
            "a shell was fired during placement"
        );
        assert_eq!(placing.opponent_fleet.mark_at(0, 0), before);

        let mut firing = firing_app();
        let ships = firing.player_fleet.ships.len();
        probe::click(&mut firing, Target::Own(9, 9));
        assert_eq!(
            firing.player_fleet.ships.len(),
            ships,
            "a sixth ship was placed in the middle of the battle"
        );
        assert_eq!(firing.player_shots, 0, "clicking one's own grid fired");

        // The phase and the placement index are two fields that agree only
        // because nothing has ever set them apart, so a fleet already placed
        // is not what makes the click on one's own grid harmless in the
        // battle -- the phase guard is. Set the two fields against each other
        // and the guard is the only thing left standing.
        let mut adrift = BattleshipApp::with_seed(11);
        adrift.phase = GamePhase::Firing;
        probe::click(&mut adrift, Target::Own(0, 0));
        assert!(
            adrift.player_fleet.ships.is_empty(),
            "a ship was placed by a click on a board whose phase says the placing is done"
        );
    }

    #[test]
    fn a_click_on_the_chrome_is_answered_and_changes_nothing() {
        for target in [
            Target::Title,
            Target::Phase,
            Target::Message,
            Target::OwnLabel,
            Target::OceanLabel,
            Target::Stats,
            Target::Help,
        ] {
            let mut app = firing_app();
            let before = format!("{:?}", app.player_fleet.marks);
            assert_eq!(
                probe::click(&mut app, target),
                EventResult::Consumed,
                "{target:?} let the click fall through to whatever is behind it"
            );
            assert_eq!(app.player_shots, 0, "{target:?} fired a shell");
            assert_eq!(app.ai_state.shots, 0, "{target:?} gave the AI a turn");
            assert_eq!(
                format!("{:?}", app.player_fleet.marks),
                before,
                "{target:?} changed the board"
            );
        }
    }

    #[test]
    fn only_the_left_button_does_anything() {
        let mut app = firing_app();
        let (x, y) = box_of(&app, Target::Ocean(4, 6)).centre();
        assert_eq!(
            app.click_at(x, y, MouseButton::Right, SIZE),
            EventResult::Ignored
        );
        assert_eq!(app.player_shots, 0, "a right click fired a shell");
    }

    #[test]
    fn a_click_outside_everything_is_left_alone() {
        let mut app = firing_app();
        assert_eq!(
            app.click_at(-10.0, -10.0, MouseButton::Left, SIZE),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_click_and_the_key_place_the_same_ship() {
        let mut by_click = BattleshipApp::with_seed(7);
        probe::click(&mut by_click, Target::Own(2, 3));

        let mut by_key = BattleshipApp::with_seed(7);
        for _ in 0..2 {
            by_key.key_at(&probe::press(Key::Down), SIZE);
        }
        for _ in 0..3 {
            by_key.key_at(&probe::press(Key::Right), SIZE);
        }
        by_key.key_at(&probe::press(Key::Enter), SIZE);

        // That the two agree is only half of it: a click and a key that both
        // do nothing agree perfectly. The ship has to have been placed.
        assert_eq!(
            by_key.player_fleet.ships.first().map(|s| (s.row, s.col)),
            Some((2, 3)),
            "the keys placed nothing"
        );
        assert_eq!(
            by_click.player_fleet.ships, by_key.player_fleet.ships,
            "the click and the keys placed different ships"
        );
    }

    #[test]
    fn the_click_and_the_key_fire_the_same_shell() {
        let mut by_click = firing_app();
        probe::click(&mut by_click, Target::Ocean(0, 2));

        let mut by_key = firing_app();
        for _ in 0..2 {
            by_key.key_at(&probe::press(Key::Right), SIZE);
        }
        by_key.key_at(&probe::press(Key::Enter), SIZE);

        assert_eq!(by_key.player_shots, 1, "the keys fired nothing");
        assert_eq!(
            format!("{:?}", by_click.opponent_fleet.marks),
            format!("{:?}", by_key.opponent_fleet.marks),
            "the click and the keys hit different cells"
        );
    }

    // ── Keys the window delivers ────────────────────────────────────

    #[test]
    fn escape_does_not_deal_a_new_board() {
        // It used to, under the comment "Could quit, but we just reset for
        // now" -- so the key a player presses to back out of something threw
        // the game away, from any phase, with no confirmation.
        let mut app = firing_app();
        probe::click(&mut app, Target::Ocean(0, 0));
        let shots = app.player_shots;
        let ships = app.player_fleet.ships.clone();

        // Asked of `on_event`, which is where Escape is answered. Asked of
        // `key_at` it reaches `handle_key`, and `handle_key` has had no Escape
        // arm since the reset was taken out of it -- so the reset could come
        // back in `on_event` and a test aimed at `handle_key` would not see it.
        assert!(
            matches!(
                app.on_event(&Event::Key(probe::press(Key::Escape))),
                Response::Exit
            ),
            "Escape did something other than close the window"
        );
        assert_eq!(app.phase, GamePhase::Firing, "Escape reset the game");
        assert_eq!(app.player_shots, shots, "Escape threw away the score");
        assert_eq!(app.player_fleet.ships, ships, "Escape unplaced the fleet");
    }

    #[test]
    fn escape_closes_the_window() {
        let mut app = firing_app();
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Escape))),
            Response::Exit
        ));
    }

    #[test]
    fn n_deals_a_new_board_in_every_phase() {
        for phase in [GamePhase::Placement, GamePhase::Firing, GamePhase::GameOver] {
            let mut app = firing_app();
            app.phase = phase;
            app.player_shots = 9;
            assert_eq!(
                app.key_at(&probe::press(Key::N), SIZE),
                EventResult::Consumed
            );
            assert_eq!(
                app.phase,
                GamePhase::Placement,
                "N did nothing in {phase:?}"
            );
            assert_eq!(app.player_shots, 0, "the old score survived N in {phase:?}");
            assert!(
                app.player_fleet.ships.is_empty(),
                "the old fleet survived N in {phase:?}"
            );
            assert_eq!(
                app.opponent_fleet.ships.len(),
                FLEET.len(),
                "the AI did not lay out a new fleet in {phase:?}"
            );
        }
    }

    #[test]
    fn a_key_the_game_has_no_use_for_is_left_alone() {
        // `Ignored` rather than `Consumed`: a window that swallowed every
        // keystroke would take the ones its container wanted too.
        let mut app = firing_app();
        assert_eq!(
            app.key_at(&probe::press(Key::Z), SIZE),
            EventResult::Ignored
        );
        // Every phase, not the one that happened to be to hand: the three
        // phases route a key through three different arms, and an arm that
        // swallows what it cannot use is invisible from the other two.
        let mut placing = BattleshipApp::with_seed(5);
        assert_eq!(
            placing.key_at(&probe::press(Key::Z), SIZE),
            EventResult::Ignored,
            "an unused key was swallowed while ships were being placed"
        );

        let mut over = firing_app();
        over.phase = GamePhase::GameOver;
        assert_eq!(
            over.key_at(&probe::press(Key::Enter), SIZE),
            EventResult::Ignored,
            "Enter did something on a finished board"
        );
    }

    #[test]
    fn a_launch_does_not_place_the_one_fleet_that_was_hardcoded() {
        // `new` wrote `SeededRng::new(0xDEAD_BEEF_CAFE_1234)` under a doc
        // comment saying the seed came from the system, so the AI's five
        // ships stood in the same five places on every machine and in every
        // session for ever. Every other test names its own seed and so cannot
        // see what `new` does; this one calls `new`.
        //
        // Two `new()` games cannot be compared with each other here: off
        // Slate OS there is no kernel randomness to open, so `seed_from_system`
        // answers with its fallback and the two would agree for a reason that
        // has nothing to do with this program. What can be checked is that the
        // fleet is no longer the one that constant placed.
        let fresh = BattleshipApp::new().opponent_fleet.ships;
        let old = BattleshipApp::with_seed(0xDEAD_BEEF_CAFE_1234)
            .opponent_fleet
            .ships;
        assert_ne!(
            fresh, old,
            "a fresh game still places the AI's fleet where the hardcoded seed put it"
        );
    }

    #[test]
    fn the_window_says_what_it_is() {
        let app = BattleshipApp::with_seed(1);
        assert_eq!(app.title(), "Battleship");
        assert_eq!(app.app_id(), "battleship");
        assert_eq!(app.initial_size(), (1000, 720));
    }

    #[test]
    fn a_resize_is_what_the_next_click_is_read_against() {
        // The first frame a real window submits goes out before any resize
        // event arrives, so the size a click is read at has to be the size the
        // last frame was drawn at -- not a remembered constant.
        let mut app = firing_app();
        let big = (1400.0, 900.0);
        let rect = probe::rect_of_sized(&app, Target::Ocean(4, 6), big).expect("no cell drawn");
        let (x, y) = rect.centre();

        app.on_event(&Event::Resize {
            width: 1400,
            height: 900,
        });
        app.on_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(
            (app.cursor_row, app.cursor_col),
            (4, 6),
            "the click was read against a size the window is no longer"
        );
    }

    #[test]
    fn the_window_asks_for_a_redraw_only_when_something_changed() {
        let mut app = firing_app();
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Right))),
            Response::Redraw
        ));
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Z))),
            Response::Idle
        ));
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    // ── What is drawn ───────────────────────────────────────────────

    /// Every run of text in a frame, with the colour it was drawn in.
    fn texts(f: &Frame<Target>) -> Vec<(String, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect()
    }

    /// Whether some run of text in the frame contains `needle`.
    fn drew(f: &Frame<Target>, needle: &str) -> bool {
        texts(f).iter().any(|(t, _)| t.contains(needle))
    }

    /// Every run of text whose origin falls inside `rect`, joined by a space.
    ///
    /// `drew` asks the whole frame, which is the wrong question for a band
    /// that has to *not* say a word the rest of the window says freely: the
    /// stats band carries "Shots fired", so a help line asked through `drew`
    /// can never be shown to have dropped the word "fire".
    fn text_in(f: &Frame<Target>, rect: Rect) -> String {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, text, .. } if rect.contains(*x, *y) => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The colours of every rectangle filled at `rect`'s top-left corner.
    fn fills_at(f: &Frame<Target>, rect: Rect) -> Vec<Color> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { x, y, color, .. }
                    if (x - rect.x).abs() < 0.01 && (y - rect.y).abs() < 0.01 =>
                {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_phase_is_named_in_the_header_and_coloured_by_how_it_is_going() {
        for (phase, won, word, colour) in [
            (GamePhase::Placement, false, "Ship Placement", YELLOW),
            (GamePhase::Firing, false, "Battle", GREEN),
            (GamePhase::GameOver, true, "Victory!", GREEN),
            (GamePhase::GameOver, false, "Defeat!", RED),
        ] {
            let mut app = firing_app();
            app.phase = phase;
            app.player_won = won;
            let f = app.draw(SIZE);
            let said = texts(&f)
                .into_iter()
                .find(|(t, _)| t == word)
                .unwrap_or_else(|| panic!("{word:?} was never drawn"));
            assert_eq!(said.1, colour, "{word:?} was drawn in the wrong colour");
            assert!(drew(&f, "Battleship"), "the title went missing");
        }
    }

    #[test]
    fn the_two_halves_of_the_header_do_not_sit_on_top_of_each_other() {
        // The phase is right-aligned by measuring the words. "Ship Placement"
        // is half again as long as "Battle", and the window is whatever the
        // user dragged it to, so a fixed column for either is wrong at once.
        for (w, h) in SIZES {
            let mut app = firing_app();
            app.phase = GamePhase::Placement;
            let title = probe::rect_of_sized(&app, Target::Title, (w, h)).expect("no title");
            let phase = probe::rect_of_sized(&app, Target::Phase, (w, h)).expect("no phase");
            if w >= 700.0 {
                assert!(
                    title.right() <= phase.x + 0.001,
                    "the title {title:?} runs into the phase {phase:?} at {w}x{h}"
                );
                assert!(
                    phase.right() <= w + 0.001,
                    "the phase runs off the right edge at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_message_bar_carries_the_message_and_answers_across_its_whole_width() {
        let mut app = firing_app();
        app.message = String::from("You sank their Cruiser!");
        let f = app.draw(SIZE);
        assert!(
            drew(&f, "You sank their Cruiser!"),
            "the message was not said"
        );

        let l = Layout::solve(SIZE.0, SIZE.1);
        let band = box_of(&app, Target::Message);
        assert!(
            (band.w - l.message.w).abs() < 0.01 && (band.h - l.message.h).abs() < 0.01,
            "the bar answers only {band:?} of a band {:?} wide",
            l.message
        );
    }

    #[test]
    fn the_aiming_ring_is_drawn_on_the_cell_the_cursor_is_on() {
        let mut app = firing_app();
        app.cursor_row = 3;
        app.cursor_col = 7;
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let want = g.cell_rect(g.ocean, 3, 7);
        let rings: Vec<_> = app
            .draw(SIZE)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect { x, y, .. } => Some((*x, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(rings.len(), 1, "there is not exactly one aiming ring");
        let (x, y) = rings.first().copied().expect("no ring");
        assert!(
            (x - want.x).abs() < 0.01 && (y - want.y).abs() < 0.01,
            "the ring is at {x},{y} and the cursor's cell is at {},{}",
            want.x,
            want.y
        );
    }

    #[test]
    fn the_ring_is_drawn_only_while_there_is_something_to_aim_at() {
        for phase in [GamePhase::Placement, GamePhase::GameOver] {
            let mut app = firing_app();
            app.phase = phase;
            let rings = app
                .draw(SIZE)
                .commands()
                .iter()
                .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
                .count();
            assert_eq!(rings, 0, "an aiming ring was drawn in {phase:?}");
        }
    }

    #[test]
    fn the_ring_follows_the_cursor() {
        let mut app = firing_app();
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let before = app.draw(SIZE);
        app.key_at(&probe::press(Key::Right), SIZE);
        app.key_at(&probe::press(Key::Down), SIZE);
        let after = app.draw(SIZE);
        let ring = |f: &Frame<Target>| {
            f.commands()
                .iter()
                .find_map(|c| match c {
                    RenderCommand::StrokeRect { x, y, .. } => Some((*x, *y)),
                    _ => None,
                })
                .expect("no ring")
        };
        assert_ne!(ring(&before), ring(&after), "the ring did not move");
        let want = g.cell_rect(g.ocean, 1, 1);
        let (x, y) = ring(&after);
        assert!((x - want.x).abs() < 0.01 && (y - want.y).abs() < 0.01);
    }

    #[test]
    fn the_ship_being_placed_is_previewed_over_the_cells_it_would_take() {
        let mut app = BattleshipApp::with_seed(7);
        app.placement_row = 2;
        app.placement_col = 1;
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let f = app.draw(SIZE);
        let tint = Color::rgba(166, 227, 161, 120);
        for c in 1..=ShipKind::Carrier.size() {
            assert!(
                fills_at(&f, g.cell_rect(g.own, 2, c)).contains(&tint),
                "cell 2,{c} was not shown as part of the ship being placed"
            );
        }
        assert!(
            !fills_at(&f, g.cell_rect(g.own, 2, 0)).contains(&tint),
            "the preview reached a cell the ship does not cover"
        );
        assert!(
            !fills_at(&f, g.cell_rect(g.ocean, 2, 1)).contains(&tint),
            "the preview was drawn on the opponent's ocean"
        );
    }

    #[test]
    fn a_placement_that_would_be_refused_is_previewed_in_the_refusing_colour() {
        let mut app = BattleshipApp::with_seed(7);
        probe::click(&mut app, Target::Own(0, 0));
        // The Battleship now starts where the Carrier already lies.
        app.placement_row = 0;
        app.placement_col = 0;
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let f = app.draw(SIZE);
        assert!(
            fills_at(&f, g.cell_rect(g.own, 0, 0)).contains(&Color::rgba(243, 139, 168, 120)),
            "an overlapping placement was shown as if it would be accepted"
        );
    }

    #[test]
    fn the_preview_is_gone_once_the_last_ship_is_placed() {
        let app = firing_app();
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let f = app.draw(SIZE);
        for tint in [
            Color::rgba(166, 227, 161, 120),
            Color::rgba(243, 139, 168, 120),
        ] {
            for c in 0..GRID_SIZE {
                assert!(
                    !fills_at(&f, g.cell_rect(g.own, 0, c)).contains(&tint),
                    "a placement preview is still on the board during the battle"
                );
            }
        }
    }

    #[test]
    fn a_hit_is_a_cross_and_a_miss_is_a_dot() {
        let mut app = firing_app();
        // A cell the opponent's fleet holds, and one it does not.
        let ship = app
            .opponent_fleet
            .ships
            .first()
            .copied()
            .expect("the AI placed no ships");
        let (hr, hc) = ship.cells().first().copied().expect("a ship with no cells");
        let (mr, mc) = (0..GRID_SIZE)
            .flat_map(|r| (0..GRID_SIZE).map(move |c| (r, c)))
            .find(|&(r, c)| app.opponent_fleet.ship_at(r, c).is_none())
            .expect("the AI's ships cover the whole ocean");

        probe::click(&mut app, Target::Ocean(byte(hr), byte(hc)));
        probe::click(&mut app, Target::Ocean(byte(mr), byte(mc)));

        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let f = app.draw(SIZE);
        let hit_cell = g.cell_rect(g.ocean, hr, hc);
        let crosses = f
            .commands()
            .iter()
            .filter(|c| {
                matches!(c, RenderCommand::Line { x1, y1, .. }
                    if hit_cell.contains(*x1, *y1))
            })
            .count();
        assert_eq!(crosses, 2, "a hit was not marked with a two-stroke cross");
        assert!(
            fills_at(&f, g.cell_rect(g.ocean, mr, mc)).is_empty()
                || !fills_at(&f, g.cell_rect(g.ocean, mr, mc)).contains(&RED),
            "a miss was marked in the colour of a hit"
        );
        let dots = f
            .commands()
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == BLUE))
            .count();
        assert!(dots >= 1, "a miss left no mark at all");
    }

    #[test]
    fn the_opponent_s_ships_are_hidden_until_the_game_is_over() {
        let app = firing_app();
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let f = app.draw(SIZE);
        let hull = ShipKind::Carrier.color();
        let ship = app
            .opponent_fleet
            .ships
            .iter()
            .find(|s| s.kind == ShipKind::Carrier)
            .copied()
            .expect("the AI has no Carrier");
        for (r, c) in ship.cells() {
            assert!(
                !fills_at(&f, g.cell_rect(g.ocean, r, c)).contains(&hull),
                "the opponent's Carrier is visible at {r},{c} before a shot was fired"
            );
        }

        let mut over = app;
        over.phase = GamePhase::GameOver;
        let f = over.draw(SIZE);
        for (r, c) in ship.cells() {
            assert!(
                fills_at(&f, g.cell_rect(g.ocean, r, c)).contains(&hull),
                "the fleet was not revealed at {r},{c} when the game ended"
            );
        }
    }

    #[test]
    fn a_sunk_enemy_ship_is_shown_while_the_battle_is_still_on() {
        let mut app = firing_app();
        let destroyer = app
            .opponent_fleet
            .ships
            .iter()
            .find(|s| s.kind == ShipKind::Destroyer)
            .copied()
            .expect("the AI has no Destroyer");
        for (r, c) in destroyer.cells() {
            app.cursor_row = r;
            app.cursor_col = c;
            app.fire_at_opponent();
        }
        let g = Layout::solve(SIZE.0, SIZE.1).grids();
        let f = app.draw(SIZE);
        for (r, c) in destroyer.cells() {
            assert!(
                fills_at(&f, g.cell_rect(g.ocean, r, c)).contains(&OVERLAY0),
                "the wreck at {r},{c} is still drawn as open water"
            );
        }
    }

    #[test]
    fn the_stats_say_what_the_game_recorded() {
        let mut app = firing_app();
        app.player_shots = 8;
        app.player_hits = 2;
        app.ai_state.shots = 5;
        app.ai_state.hits = 1;
        let f = app.draw(SIZE);
        for line in [
            "Shots: 8",
            "Hit Rate: 25.0%",
            "AI Shots: 5",
            "AI Hit Rate: 20.0%",
            "Your Ships: 5/5",
            "Enemy Ships: 5/5",
        ] {
            assert!(drew(&f, line), "the stats never said {line:?}");
        }
    }

    #[test]
    fn a_fleet_down_to_its_last_ship_is_said_in_the_colour_of_alarm() {
        let mut app = firing_app();
        let f = app.draw(SIZE);
        let colour_of = |f: &Frame<Target>, prefix: &str| {
            texts(f)
                .into_iter()
                .find(|(t, _)| t.starts_with(prefix))
                .unwrap_or_else(|| panic!("{prefix:?} was never drawn"))
                .1
        };
        assert_eq!(colour_of(&f, "Your Ships"), GREEN);

        // Sink four of the player's five.
        let doomed: Vec<_> = app.player_fleet.ships.iter().copied().take(4).collect();
        for ship in doomed {
            for (r, c) in ship.cells() {
                app.player_fleet.receive_fire(r, c);
            }
        }
        let f = app.draw(SIZE);
        assert_eq!(
            colour_of(&f, "Your Ships"),
            RED,
            "one ship left was not called out"
        );
        assert!(drew(&f, "Your Ships: 1/5"));
    }

    #[test]
    fn the_help_line_says_what_the_keys_do_in_this_phase() {
        for (phase, needle, absent) in [
            (GamePhase::Placement, "rotate", "fire"),
            (GamePhase::Firing, "fire", "rotate"),
            (GamePhase::GameOver, "new game", "rotate"),
        ] {
            let mut app = firing_app();
            app.phase = phase;
            let f = app.draw(SIZE);
            let help = text_in(&f, box_of(&app, Target::Help));
            assert!(
                help.contains(needle),
                "the help in {phase:?} never said {needle:?}, it said {help:?}"
            );
            assert!(
                !help.contains(absent),
                "the help in {phase:?} still offered {absent:?}: {help:?}"
            );
        }
    }

    #[test]
    fn both_grids_are_labelled_a_to_j_down_and_one_to_ten_across() {
        let app = firing_app();
        let f = app.draw(SIZE);
        let said = texts(&f);
        let count = |needle: &str| said.iter().filter(|(t, _)| t == needle).count();
        for r in 0..GRID_SIZE {
            let letter = String::from(char::from(b'A' + byte(r)));
            assert_eq!(
                count(&letter),
                2,
                "the row label {letter:?} is not on both grids"
            );
        }
        for c in 1..=GRID_SIZE {
            let number = format!("{c}");
            assert!(
                count(&number) >= 2,
                "the column label {number:?} is not on both grids"
            );
        }
    }

    // ── The rules underneath ────────────────────────────────────────

    #[test]
    fn the_clamp_keeps_every_ship_on_the_board_from_every_cell() {
        for kind in FLEET {
            for orientation in [Orientation::Horizontal, Orientation::Vertical] {
                for row in 0..GRID_SIZE {
                    for col in 0..GRID_SIZE {
                        let mut app = BattleshipApp::with_seed(3);
                        app.placement_orientation = orientation;
                        app.placement_index = FLEET
                            .iter()
                            .position(|k| *k == kind)
                            .expect("a ship not in the fleet");
                        app.placement_row = row;
                        app.placement_col = col;
                        app.clamp_placement();
                        let ship = app.placement_preview_ship().expect("nothing to place");
                        assert!(
                            ship.is_within_bounds(),
                            "{kind:?} {orientation:?} from {row},{col} was left off the board"
                        );
                    }
                }
            }
        }

        // And through the key that rotates. A rotation moves the ship's far
        // end without moving its origin, which is the one change that can
        // carry a ship off the board while the cursor stays where it was --
        // so a rotation that returns before the clamp is a rotation no
        // cursor-driven check can catch.
        for kind in FLEET {
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    let mut app = BattleshipApp::with_seed(3);
                    app.placement_index = FLEET
                        .iter()
                        .position(|k| *k == kind)
                        .expect("a ship not in the fleet");
                    app.placement_row = row;
                    app.placement_col = col;
                    app.clamp_placement();
                    app.handle_placement_key(Key::R);
                    let ship = app.placement_preview_ship().expect("nothing to place");
                    assert!(
                        ship.is_within_bounds(),
                        "{kind:?} rotated at {row},{col} was left off the board"
                    );
                }
            }
        }
    }

    #[test]
    fn the_ai_never_fires_at_the_same_cell_twice() {
        let mut app = firing_app();
        let mut seen = [[false; GRID_SIZE]; GRID_SIZE];
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.phase != GamePhase::Firing {
                    break;
                }
                app.cursor_row = r;
                app.cursor_col = c;
                app.fire_at_opponent();
            }
        }
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.ai_state.has_fired(r, c) {
                    assert!(!seen[r][c], "the AI fired at {r},{c} twice");
                    seen[r][c] = true;
                }
            }
        }
        let fired = seen.iter().flatten().filter(|f| **f).count();
        assert_eq!(
            fired, app.ai_state.shots,
            "the AI's shot count does not match the cells it marked"
        );
    }

    #[test]
    fn a_hit_rate_before_the_first_shot_is_zero_and_not_a_division_by_nothing() {
        let app = BattleshipApp::with_seed(11);
        assert_eq!(app.player_hit_rate(), 0.0);
        assert_eq!(app.ai_hit_rate(), 0.0);
        assert_eq!(percent(0, 0), 0.0);
        assert_eq!(percent(3, 4), 75.0);
    }
}
